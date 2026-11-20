//! The metronome (groups-vcas-transport Ruling 19): the count-in's click
//! on every beat while the transport plays (or only while a record pass
//! runs), the first beat of each bar accented. Beats follow the tempo map,
//! ramps included, and the meter map: a beat is the meter's denominator
//! note, and each starts on the exact frame `TempoTable::tick_to_frame`
//! gives its tick. Like the count-in's, the clicks are delayed by the
//! graph latency, so a beat is heard with the audio of its frame.
//!
//! Only the live engine's renderer clicks (`Engine::set_click`): a bounce
//! and every export never send it a [`ClickSpec`], so their renderers
//! stay silent.

use crate::count_in::{CLICK_SECONDS, click_sample};
use crate::schedule::TempoTable;
use core_model::meter::MeterMap;
use core_model::time::Tick;

/// Ticks on either side of a run's ends searched for beats: more than a
/// frame's worth at any tempo, so the frame test decides alone.
const SEARCH_TICKS: i64 = 64;

/// Before every meter entry: the first segment also governs the pickup
/// bars before tick 0.
const BEFORE_ALL: i64 = i64::MIN / 4;

/// One meter segment: beats of `beat` ticks from `anchor` (a barline),
/// `per_bar` to a bar, for ticks `from..to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BeatSegment {
    from: i64,
    to: i64,
    anchor: i64,
    beat: i64,
    per_bar: u32,
}

/// Per meter segment: start tick, end tick, beat ticks, beats per bar.
/// Built off the audio thread.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BeatGrid {
    segments: Box<[BeatSegment]>,
}

impl BeatGrid {
    /// The beats of `meter` up to tick `end`. Not RT.
    pub fn build(meter: &MeterMap, end: Tick) -> BeatGrid {
        let entries: Vec<_> = meter.entries().iter().copied().collect();
        let mut segments = Vec::with_capacity(entries.len());
        for (i, e) in entries.iter().enumerate() {
            let from = if i == 0 { BEFORE_ALL } else { e.tick.0 };
            let to = entries.get(i + 1).map_or(end.0, |n| n.tick.0);
            if to <= from {
                continue;
            }
            segments.push(BeatSegment {
                from,
                to,
                anchor: e.tick.0,
                beat: e.meter.beat_ticks().max(1),
                per_bar: u32::from(e.meter.num.max(1)),
            });
        }
        BeatGrid {
            segments: segments.into_boxed_slice(),
        }
    }

    /// Each beat starting in frames [from, to), as (frame, accented),
    /// in order. Beats before the tempo table's origin are never given.
    /// Walks only the segments the run overlaps. // RT
    pub fn beats_in(
        &self,
        tempo: &TempoTable,
        from: u64,
        to: u64,
        out: &mut impl FnMut(u64, bool),
    ) {
        if to <= from {
            return;
        }
        let origin = tempo.knots().first().map_or(0, |k| k.tick);
        let lo = tempo
            .frame_to_tick(from)
            .0
            .saturating_sub(SEARCH_TICKS)
            .max(origin);
        let hi = tempo.frame_to_tick(to).0.saturating_add(SEARCH_TICKS);
        let first = self.segments.partition_point(|s| s.to <= lo);
        for s in self.segments.get(first..).unwrap_or(&[]) {
            if s.from > hi {
                break;
            }
            let start = lo.max(s.from);
            let end = hi.min(s.to.saturating_sub(1));
            // The first beat at or after `start`.
            let mut n = (start.saturating_sub(s.anchor)).div_euclid(s.beat);
            if s.anchor.saturating_add(n.saturating_mul(s.beat)) < start {
                n = n.saturating_add(1);
            }
            loop {
                let t = s.anchor.saturating_add(n.saturating_mul(s.beat));
                if t > end {
                    break;
                }
                let f = tempo.tick_to_frame(Tick(t));
                if f >= to {
                    break;
                }
                if f >= from {
                    out(f, n.rem_euclid(i64::from(s.per_bar)) == 0);
                }
                n = n.saturating_add(1);
            }
        }
    }
}

/// What the live engine's metronome does: whether it clicks, its gain
/// (linear, from the level in dB) and whether only while recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClickSpec {
    /// Whether it clicks.
    pub on: bool,
    /// Linear gain on both peaks.
    pub gain: f32,
    /// Clicks only while a record pass runs.
    pub record_only: bool,
}

impl ClickSpec {
    /// No click (what every renderer starts with).
    pub const OFF: ClickSpec = ClickSpec {
        on: false,
        gain: 1.0,
        record_only: false,
    };
}

impl Default for ClickSpec {
    fn default() -> Self {
        ClickSpec::OFF
    }
}

/// Clicks waiting for the latency or sounding at once: more than a beat
/// of latency at the fastest beats still fits.
const MAX_CLICKS: usize = 32;

/// The renderer's metronome. `Copy`, so it never allocates: its
/// [`ClickSpec`], and per waiting or sounding click its start (in frames
/// rendered) and accent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metronome {
    spec: ClickSpec,
    rate: f64,
    /// Frames rendered so far: the clock clicks are placed on.
    now: u64,
    clicks: [Option<(u64, bool)>; MAX_CLICKS],
}

impl Metronome {
    /// A silent metronome at `rate` Hz.
    pub fn new(rate: f64) -> Metronome {
        Metronome {
            spec: ClickSpec::OFF,
            rate: rate.max(1.0),
            now: 0,
            clicks: [None; MAX_CLICKS],
        }
    }

    /// The spec in force.
    pub fn spec(&self) -> ClickSpec {
        self.spec
    }

    /// Takes `spec`. Turned off, the clicks waiting and sounding stop.
    /// A gain that is not finite is silence. // RT
    pub fn set(&mut self, spec: ClickSpec) {
        let gain = if spec.gain.is_finite() {
            spec.gain.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.spec = ClickSpec { gain, ..spec };
        if !spec.on {
            self.clicks = [None; MAX_CLICKS];
        }
    }

    /// Whether a click waits or sounds.
    pub fn sounding(&self) -> bool {
        self.clicks.iter().any(Option::is_some)
    }

    fn click_len(&self) -> u64 {
        (CLICK_SECONDS * self.rate).round() as u64
    }

    /// Adds this block's clicks to channels 0–1 of `out`, `frames`
    /// frames. `run`: the timeline frames these frames play (`None`
    /// stopped or counting in); its beats are heard `latency` frames
    /// later. A click that starts in one call plays its tail into the
    /// next. A loop wrap is two calls, one per run, so the first beat
    /// after the wrap sounds. // RT
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        out: &mut [&mut [f32]],
        grid: &BeatGrid,
        tempo: &TempoTable,
        run: Option<(u64, u64)>,
        recording: bool,
        latency: u64,
        frames: u32,
    ) {
        let now = self.now;
        let clicks_now = self.spec.on && (recording || !self.spec.record_only);
        if let (true, Some((from, to))) = (clicks_now, run) {
            let clicks = &mut self.clicks;
            grid.beats_in(tempo, from, to, &mut |f, accent| {
                let at = now
                    .saturating_add(f.saturating_sub(from))
                    .saturating_add(latency);
                // A full list drops the click: never more work here.
                if let Some(slot) = clicks.iter_mut().find(|c| c.is_none()) {
                    *slot = Some((at, accent));
                }
            });
        }
        let end = now.saturating_add(u64::from(frames));
        let len = self.click_len();
        let (rate, gain) = (self.rate, self.spec.gain);
        for slot in self.clicks.iter_mut() {
            let Some((at, accent)) = *slot else {
                continue;
            };
            let a = at.max(now);
            let b = at.saturating_add(len).min(end);
            let k = if accent { 0 } else { 1 };
            for f in a..b {
                let v = click_sample(k, f - at, rate, gain);
                let i = (f - now) as usize;
                for ch in out.iter_mut().take(2) {
                    if let Some(x) = ch.get_mut(i) {
                        *x += v;
                    }
                }
            }
            if at.saturating_add(len) <= end {
                *slot = None;
            }
        }
        self.now = end;
    }
}
