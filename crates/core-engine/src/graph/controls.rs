//! Automatable mixer controls of a render graph (Plan 4d Rulings 11, 13):
//! every strip's fader, pan, mute and solo and every send's level, the
//! static value the mixer gave each, and the override automation set.
//! Built off the audio thread; set, committed and adopted on it. // RT

use crate::graph::dsp::strip_gains;
use crate::schedule::automation::{AutoDest, StripParam};
use core_model::automation::{fader_db, fader_norm, pan_norm, pan_of, switch_on};
use core_model::ids::StripId;
use core_model::mixer::db_to_gain;
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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
    /// Every commit's overrides, for the control thread.
    mirror: Arc<OverrideSnapshot>,
}

/// No override, in an [`OverrideSnapshot`] slot.
const NONE: u64 = u64::MAX;

fn slot(v: Option<f64>) -> u64 {
    v.map_or(NONE, f64::to_bits)
}

fn unslot(a: &AtomicU64) -> Option<f64> {
    match a.load(Ordering::Relaxed) {
        NONE => None,
        b => Some(f64::from_bits(b)),
    }
}

#[derive(Debug)]
struct SnapStrip {
    strip: StripId,
    base: Vals,
    feeds: Range<usize>,
    /// Fader dB, pan, mute (0/1), solo (0/1).
    over: [AtomicU64; 4],
}

#[derive(Debug)]
struct SnapFeed {
    send: Option<SendSpec>,
    over_db: AtomicU64,
}

/// The automation overrides a graph's controls held at their last commit,
/// readable off the audio thread: a rebuilt engine starts from them, so a
/// device change while stopped keeps automated values until the user moves
/// them (Plan 4d Ruling 13, final review Minor 1). Lock-free; written by
/// the audio thread at each commit.
#[derive(Debug, Default)]
pub struct OverrideSnapshot {
    strips: Box<[SnapStrip]>,
    feeds: Box<[SnapFeed]>,
}

impl OverrideSnapshot {
    /// Whether any override is held.
    pub fn is_empty(&self) -> bool {
        self.strips
            .iter()
            .all(|s| s.over.iter().all(|a| unslot(a).is_none()))
            && self.feeds.iter().all(|f| unslot(&f.over_db).is_none())
    }

    /// The fader override (dB) of `strip`, if any.
    pub fn fader_db(&self, strip: StripId) -> Option<f64> {
        self.strips
            .iter()
            .find(|s| s.strip == strip)
            .and_then(|s| unslot(&s.over[0]))
    }

    /// The override `dest` held at the last commit, normalised as
    /// automation is (plan 5e-1 Ruling 26). Not RT.
    pub fn value(&self, dest: AutoDest) -> Option<f64> {
        let (strip, slot) = match dest {
            AutoDest::Strip(s, p) => (s, Some(p)),
            AutoDest::Send(s, _) => (s, None),
            AutoDest::Plugin(..) => return None,
        };
        let s = self.strips.iter().find(|s| s.strip == strip)?;
        let over = |i: usize| s.over.get(i).and_then(unslot);
        match (slot, dest) {
            (Some(StripParam::Fader), _) => over(0).map(fader_norm),
            (Some(StripParam::Pan), _) => over(1).map(pan_norm),
            (Some(StripParam::Mute), _) => over(2),
            (Some(StripParam::Solo), _) => over(3),
            (None, AutoDest::Send(_, index)) => self
                .feeds
                .get(s.feeds.clone())?
                .iter()
                .find(|f| f.send.is_some_and(|x| x.index == index))
                .and_then(|f| unslot(&f.over_db))
                .map(fader_norm),
            (None, _) => None,
        }
    }

    /// Controls holding these overrides, for [`Controls::adopt`]. Not RT.
    fn controls(&self) -> Controls {
        let strips = self
            .strips
            .iter()
            .map(|s| CtlStrip {
                strip: s.strip,
                node: 0,
                apply_pan: false,
                base: s.base,
                over: Over {
                    db: unslot(&s.over[0]),
                    pan: unslot(&s.over[1]),
                    mute: unslot(&s.over[2]).map(|v| v != 0.0),
                    solo: unslot(&s.over[3]).map(|v| v != 0.0),
                },
                feeds: s.feeds.clone(),
                keeps: 0..0,
            })
            .collect();
        let feeds = self
            .feeds
            .iter()
            .map(|f| CtlFeed {
                input: 0,
                send: f.send,
                over_db: unslot(&f.over_db),
            })
            .collect();
        Controls {
            strips,
            feeds,
            ..Controls::default()
        }
    }
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
        let mirror = Arc::new(OverrideSnapshot {
            strips: strips
                .iter()
                .map(|s: &CtlStrip| SnapStrip {
                    strip: s.strip,
                    base: s.base,
                    feeds: s.feeds.clone(),
                    over: std::array::from_fn(|_| AtomicU64::new(NONE)),
                })
                .collect(),
            feeds: feeds
                .iter()
                .map(|f: &CtlFeed| SnapFeed {
                    send: f.send,
                    over_db: AtomicU64::new(NONE),
                })
                .collect(),
        });
        Controls {
            strips: strips.into_boxed_slice(),
            feeds: feeds.into_boxed_slice(),
            keeps: keeps.into_boxed_slice(),
            audible: vec![0u64; words].into_boxed_slice(),
            any_override: false,
            dirty: false,
            mirror,
        }
    }

    /// The overrides of every commit, for the control thread. Not RT.
    pub(crate) fn snapshot(&self) -> Arc<OverrideSnapshot> {
        Arc::clone(&self.mirror)
    }

    /// Takes the overrides `snap` holds, as [`Controls::adopt`] takes an
    /// old graph's (another engine's, after a device rebuild); true when
    /// one was taken. Not RT.
    pub(crate) fn adopt_snapshot(&mut self, snap: &OverrideSnapshot) -> bool {
        self.adopt(&snap.controls())
    }

    /// Mirrors the overrides for the control thread. // RT
    pub(crate) fn publish(&self) {
        for (s, m) in self.strips.iter().zip(self.mirror.strips.iter()) {
            let o = s.over;
            let b = |v: Option<bool>| v.map(|v| if v { 1.0 } else { 0.0 });
            for (a, v) in m.over.iter().zip([o.db, o.pan, b(o.mute), b(o.solo)]) {
                a.store(slot(v), Ordering::Relaxed);
            }
        }
        for (f, m) in self.feeds.iter().zip(self.mirror.feeds.iter()) {
            m.over_db.store(slot(f.over_db), Ordering::Relaxed);
        }
    }

    fn find(&self, strip: StripId) -> Option<usize> {
        self.strips.binary_search_by_key(&strip, |s| s.strip).ok()
    }

    /// The static value (normalised, as automation's) of a strip control
    /// or a send; `None` for plugins and unknown controls. // RT
    pub(crate) fn base_of(&self, dest: AutoDest) -> Option<f64> {
        let strip = match dest {
            AutoDest::Strip(s, _) | AutoDest::Send(s, _) => s,
            AutoDest::Plugin(..) => return None,
        };
        let s = self.find(strip).and_then(|i| self.strips.get(i))?;
        let on = |b: bool| if b { 1.0 } else { 0.0 };
        match dest {
            AutoDest::Strip(_, StripParam::Fader) => Some(fader_norm(s.base.db)),
            AutoDest::Strip(_, StripParam::Pan) => Some(pan_norm(s.base.pan)),
            AutoDest::Strip(_, StripParam::Mute) => Some(on(s.base.mute)),
            AutoDest::Strip(_, StripParam::Solo) => Some(on(s.base.solo)),
            AutoDest::Send(_, index) => self
                .feeds
                .get(s.feeds.clone())?
                .iter()
                .find_map(|f| f.send.filter(|x| x.index == index))
                .map(|x| fader_norm(x.level_db)),
            AutoDest::Plugin(..) => None,
        }
    }

    /// The value (normalised, as automation's) a strip control or a send
    /// plays now: its override, else its static value; `None` for plugins
    /// and unknown controls (Task 5 review I1). // RT
    pub(crate) fn heard_of(&self, dest: AutoDest) -> Option<f64> {
        let strip = match dest {
            AutoDest::Strip(s, _) | AutoDest::Send(s, _) => s,
            AutoDest::Plugin(..) => return None,
        };
        let s = self.find(strip).and_then(|i| self.strips.get(i))?;
        let on = |b: bool| if b { 1.0 } else { 0.0 };
        let (o, b) = (s.over, s.base);
        match dest {
            AutoDest::Strip(_, StripParam::Fader) => Some(fader_norm(o.db.unwrap_or(b.db))),
            AutoDest::Strip(_, StripParam::Pan) => Some(pan_norm(o.pan.unwrap_or(b.pan))),
            AutoDest::Strip(_, StripParam::Mute) => Some(on(o.mute.unwrap_or(b.mute))),
            AutoDest::Strip(_, StripParam::Solo) => Some(on(o.solo.unwrap_or(b.solo))),
            AutoDest::Send(_, index) => self.feeds.get(s.feeds.clone())?.iter().find_map(|f| {
                let send = f.send.filter(|x| x.index == index)?;
                Some(fader_norm(f.over_db.unwrap_or(send.level_db)))
            }),
            AutoDest::Plugin(..) => None,
        }
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
    /// per strip, which returns whether the strip's audibility changed,
    /// and `edge(input, gain, switched)` per fed edge, `switched` passing
    /// that change on (mute and solo switch hard, Plan 4d Ruling 11). The
    /// gains are those the mixer translation computes (`graph::mixer`).
    /// Writes nothing (false) when no override exists and none was
    /// dropped. // RT
    pub(crate) fn commit(
        &mut self,
        mut strip: impl FnMut(usize, f32, f32, bool) -> bool,
        mut edge: impl FnMut(usize, f32, bool),
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
            let switched = strip(s.node, gl, gr, on);
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
                edge(f.input, gain, switched);
            }
        }
        self.publish();
        true
    }
}
