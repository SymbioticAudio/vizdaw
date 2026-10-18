//! Automatable mixer controls of a render graph (Plan 4d Rulings 11, 13):
//! every strip's fader, pan, mute and solo and every send's level, the
//! static value the mixer gave each, and the override automation set.
//! Built off the audio thread; set, committed and adopted on it. // RT

use crate::graph::dsp::strip_gains;
use crate::schedule::automation::{AutoDest, StripParam};
use core_model::automation::{fader_db, pan_of, switch_on};
use core_model::ids::StripId;
use core_model::mixer::db_to_gain;
use std::collections::BTreeMap;
use std::ops::Range;

/// A send as the mixer set it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SendSpec {
    /// Position in the strip's sends.
    pub index: u16,
    /// Level in dB.
    pub level_db: f64,
    /// On.
    pub enabled: bool,
}

/// An edge a strip feeds: input `input` (its position in that node's
/// `StripSpec::inputs`) of node `to` — the strip's main output, or a send.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeedSpec {
    /// Target node.
    pub to: usize,
    /// Input position in the target's spec.
    pub input: usize,
    /// The send, or `None` for the main output.
    pub send: Option<SendSpec>,
}

/// One strip's static controls and what it feeds, from the mixer.
#[derive(Debug, Clone, PartialEq)]
pub struct StripControlSpec {
    /// The strip.
    pub strip: StripId,
    /// Its graph node.
    pub node: usize,
    /// Fader in dB.
    pub gain_db: f64,
    /// Pan −1 … +1.
    pub pan: f64,
    /// Pan applies (not master).
    pub apply_pan: bool,
    /// Muted.
    pub mute: bool,
    /// Soloed.
    pub solo: bool,
    /// Its output and sends.
    pub feeds: Vec<FeedSpec>,
    /// Strips audible while it is soloed: itself, everything it reaches and
    /// everything reaching it (sidechains excluded).
    pub solo_keeps: Vec<StripId>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Vals {
    db: f64,
    pan: f64,
    mute: bool,
    solo: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Over {
    db: Option<f64>,
    pan: Option<f64>,
    mute: Option<bool>,
    solo: Option<bool>,
}

#[derive(Debug)]
struct CtlStrip {
    strip: StripId,
    node: usize,
    apply_pan: bool,
    base: Vals,
    over: Over,
    feeds: Range<usize>,
    keeps: Range<usize>,
}

#[derive(Debug)]
struct CtlFeed {
    /// Index into the graph's inputs.
    input: usize,
    send: Option<SendSpec>,
    over_db: Option<f64>,
}

/// A graph's controls (empty for graphs built without a mixer).
#[derive(Debug, Default)]
pub struct Controls {
    /// Sorted by strip.
    strips: Box<[CtlStrip]>,
    feeds: Box<[CtlFeed]>,
    /// Per strip, bit words of the strips its solo keeps.
    keeps: Box<[u64]>,
    /// Scratch: the audible strips.
    audible: Box<[u64]>,
    any_override: bool,
    /// Overrides were dropped: the next commit writes the static values.
    dirty: bool,
}

impl Controls {
    /// The controls of `specs`; `input(node, k)` is the graph's index of
    /// input `k` of node `node` (`None`: the graph dropped that edge). Not
    /// RT.
    pub(crate) fn build(
        specs: &[StripControlSpec],
        input: impl Fn(usize, usize) -> Option<usize>,
    ) -> Controls {
        let mut sorted: Vec<&StripControlSpec> = specs.iter().collect();
        sorted.sort_by_key(|s| s.strip);
        sorted.dedup_by_key(|s| s.strip);
        let index: BTreeMap<StripId, usize> = sorted
            .iter()
            .enumerate()
            .map(|(i, s)| (s.strip, i))
            .collect();
        let n = sorted.len();
        let words = n.div_ceil(64);
        let mut strips = Vec::with_capacity(n);
        let mut feeds = Vec::new();
        let mut keeps = vec![0u64; n * words];
        for (i, s) in sorted.iter().enumerate() {
            let first = feeds.len();
            for f in &s.feeds {
                if let Some(input) = input(f.to, f.input) {
                    feeds.push(CtlFeed {
                        input,
                        send: f.send,
                        over_db: None,
                    });
                }
            }
            for k in &s.solo_keeps {
                if let Some(j) = index.get(k) {
                    if let Some(w) = keeps.get_mut(i * words + j / 64) {
                        *w |= 1u64 << (j % 64);
                    }
                }
            }
            strips.push(CtlStrip {
                strip: s.strip,
                node: s.node,
                apply_pan: s.apply_pan,
                base: Vals {
                    db: s.gain_db,
                    pan: s.pan,
                    mute: s.mute,
                    solo: s.solo,
                },
                over: Over::default(),
                feeds: first..feeds.len(),
                keeps: i * words..(i + 1) * words,
            });
        }
        Controls {
            strips: strips.into_boxed_slice(),
            feeds: feeds.into_boxed_slice(),
            keeps: keeps.into_boxed_slice(),
            audible: vec![0u64; words].into_boxed_slice(),
            any_override: false,
            dirty: false,
        }
    }

    fn find(&self, strip: StripId) -> Option<usize> {
        self.strips.binary_search_by_key(&strip, |s| s.strip).ok()
    }

    /// Automation's value (normalised) for `strip`'s `p`. // RT
    pub(crate) fn set_strip(&mut self, strip: StripId, p: StripParam, v: f64) {
        let Some(i) = self.find(strip) else {
            return;
        };
        let Some(s) = self.strips.get_mut(i) else {
            return;
        };
        match p {
            StripParam::Fader => s.over.db = Some(fader_db(v)),
            StripParam::Pan => s.over.pan = Some(pan_of(v)),
            StripParam::Mute => s.over.mute = Some(switch_on(v)),
            StripParam::Solo => s.over.solo = Some(switch_on(v)),
        }
        self.any_override = true;
    }

    /// Automation's level (normalised) for send `index` of `strip`. // RT
    pub(crate) fn set_send(&mut self, strip: StripId, index: u16, v: f64) {
        let Some(range) = self
            .find(strip)
            .and_then(|i| self.strips.get(i))
            .map(|s| s.feeds.clone())
        else {
            return;
        };
        for f in self.feeds.get_mut(range).into_iter().flatten() {
            if f.send.is_some_and(|s| s.index == index) {
                f.over_db = Some(fader_db(v));
                self.any_override = true;
            }
        }
    }

    /// Drops the overrides of the controls `driven` says no lane drives;
    /// the next commit writes their static values. // RT
    pub(crate) fn release(&mut self, driven: impl Fn(AutoDest) -> bool) {
        for s in self.strips.iter_mut() {
            let id = s.strip;
            if !driven(AutoDest::Strip(id, StripParam::Fader)) {
                s.over.db = None;
            }
            if !driven(AutoDest::Strip(id, StripParam::Pan)) {
                s.over.pan = None;
            }
            if !driven(AutoDest::Strip(id, StripParam::Mute)) {
                s.over.mute = None;
            }
            if !driven(AutoDest::Strip(id, StripParam::Solo)) {
                s.over.solo = None;
            }
            for f in self.feeds.get_mut(s.feeds.clone()).into_iter().flatten() {
                if let Some(send) = f.send {
                    if !driven(AutoDest::Send(id, send.index)) {
                        f.over_db = None;
                    }
                }
            }
        }
        self.dirty = true;
        self.any_override = self.has_overrides();
    }

    fn has_overrides(&self) -> bool {
        self.strips.iter().any(|s| s.over != Over::default())
            || self.feeds.iter().any(|f| f.over_db.is_some())
    }

    /// Takes `old`'s overrides for the controls whose static value did not
    /// change (Plan 4d Ruling 13); true when one was taken. // RT
    pub(crate) fn adopt(&mut self, old: &Controls) -> bool {
        for s in self.strips.iter_mut() {
            let Some(o) = old.find(s.strip).and_then(|j| old.strips.get(j)) else {
                continue;
            };
            if o.base.db == s.base.db {
                s.over.db = o.over.db;
            }
            if o.base.pan == s.base.pan {
                s.over.pan = o.over.pan;
            }
            if o.base.mute == s.base.mute {
                s.over.mute = o.over.mute;
            }
            if o.base.solo == s.base.solo {
                s.over.solo = o.over.solo;
            }
            let olds = old.feeds.get(o.feeds.clone()).unwrap_or(&[]);
            for f in self.feeds.get_mut(s.feeds.clone()).into_iter().flatten() {
                let Some(send) = f.send else {
                    continue;
                };
                let same = olds.iter().find(|of| {
                    of.send.is_some_and(|x| {
                        x.index == send.index
                            && x.level_db == send.level_db
                            && x.enabled == send.enabled
                    })
                });
                if let Some(of) = same {
                    f.over_db = of.over_db;
                }
            }
        }
        self.any_override = self.has_overrides();
        self.any_override
    }

    /// Writes the effective gains: `strip(node, gain_l, gain_r, audible)`
    /// per strip and `edge(input, gain)` per fed edge, as the mixer
    /// translation computes them (`graph::mixer`). Writes nothing (false)
    /// when no override exists and none was dropped. // RT
    pub(crate) fn commit(
        &mut self,
        mut strip: impl FnMut(usize, f32, f32, bool),
        mut edge: impl FnMut(usize, f32),
    ) -> bool {
        if !self.any_override && !self.dirty {
            return false;
        }
        self.dirty = false;
        let any_solo = self
            .strips
            .iter()
            .any(|s| s.over.solo.unwrap_or(s.base.solo));
        if any_solo {
            self.audible.fill(0);
            for s in self.strips.iter() {
                if s.over.solo.unwrap_or(s.base.solo) {
                    let keeps = self.keeps.get(s.keeps.clone()).unwrap_or(&[]);
                    for (a, k) in self.audible.iter_mut().zip(keeps) {
                        *a |= *k;
                    }
                }
            }
        } else {
            self.audible.fill(u64::MAX);
        }
        for (i, s) in self.strips.iter().enumerate() {
            let kept = self
                .audible
                .get(i / 64)
                .is_some_and(|w| w & (1u64 << (i % 64)) != 0);
            let on = kept && !s.over.mute.unwrap_or(s.base.mute);
            let (gl, gr) = strip_gains(
                s.over.db.unwrap_or(s.base.db),
                s.over.pan.unwrap_or(s.base.pan),
                s.apply_pan,
            );
            strip(s.node, gl, gr, on);
            let pass = if on { 1.0 } else { 0.0 };
            for f in self.feeds.get(s.feeds.clone()).unwrap_or(&[]) {
                let gain = match f.send {
                    None => pass,
                    Some(send) => {
                        let level = if send.enabled {
                            db_to_gain(f.over_db.unwrap_or(send.level_db)) as f32
                        } else {
                            0.0
                        };
                        pass * level
                    }
                };
                edge(f.input, gain);
            }
        }
        true
    }
}
