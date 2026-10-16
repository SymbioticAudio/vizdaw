//! Tempo maps with constant segments and linear ramps.

use crate::error::{ModelError, Result};
use crate::time::{PPQ, Tick};
use im::Vector;
use serde::{Deserialize, Serialize};

/// Tempo as fixed-point milli-BPM (120 BPM is `Bpm(120_000)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bpm(pub u32);

impl Bpm {
    /// Lowest allowed tempo, 5 BPM.
    pub const MIN: Bpm = Bpm(5_000);
    /// Highest allowed tempo, 999 BPM.
    pub const MAX: Bpm = Bpm(999_000);

    /// Whole beats per minute.
    pub fn whole(bpm: u32) -> Bpm {
        Bpm(bpm.saturating_mul(1000))
    }

    /// Tempo as floating-point BPM.
    pub fn as_f64(self) -> f64 {
        f64::from(self.0) / 1000.0
    }

    /// True when within 5–999 BPM.
    pub fn in_range(self) -> bool {
        (Bpm::MIN..=Bpm::MAX).contains(&self)
    }
}

/// One tempo point. When `ramp` is true the tempo changes linearly (in BPM
/// per tick) until the next point; the last point's ramp flag is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TempoPoint {
    /// Where the point starts.
    pub tick: Tick,
    /// Tempo at that tick.
    pub bpm: Bpm,
    /// Ramp linearly to the next point.
    pub ramp: bool,
}

/// Ordered tempo points; the first is at tick 0 and governs negative ticks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TempoMap {
    points: Vector<TempoPoint>,
}

impl Default for TempoMap {
    fn default() -> Self {
        TempoMap {
            points: Vector::unit(TempoPoint {
                tick: Tick::ZERO,
                bpm: Bpm::whole(120),
                ramp: false,
            }),
        }
    }
}

/// Seconds per tick at `bpm`.
fn spt(bpm: f64) -> f64 {
    60.0 / (bpm * PPQ as f64)
}

impl TempoMap {
    /// A single constant tempo.
    pub fn constant(bpm: Bpm) -> Result<TempoMap> {
        TempoMap::new(vec![TempoPoint {
            tick: Tick::ZERO,
            bpm,
            ramp: false,
        }])
    }

    /// Builds a validated map.
    pub fn new(points: Vec<TempoPoint>) -> Result<TempoMap> {
        let map = TempoMap {
            points: points.into_iter().collect(),
        };
        map.check()?;
        Ok(map)
    }

    /// The points, in order.
    pub fn points(&self) -> &Vector<TempoPoint> {
        &self.points
    }

    /// Returns the first rule violation, if any. Used by commands and by
    /// project-file validation (a deserialized map bypasses [`TempoMap::new`]).
    pub fn check(&self) -> Result<()> {
        let first = self
            .points
            .front()
            .ok_or_else(|| ModelError::invalid("tempo map is empty"))?;
        if first.tick != Tick::ZERO {
            return Err(ModelError::range("tempo.first_tick", first.tick.0));
        }
        let mut prev: Option<Tick> = None;
        for p in &self.points {
            if !p.bpm.in_range() {
                return Err(ModelError::range("tempo.bpm", p.bpm.as_f64()));
            }
            if prev.is_some_and(|t| p.tick <= t) {
                return Err(ModelError::range("tempo.tick_order", p.tick.0));
            }
            prev = Some(p.tick);
        }
        Ok(())
    }

    /// Segments as `(start, end, bpm_start, bpm_end)`; the last has
    /// `end == None`.
    fn segments(&self) -> impl Iterator<Item = (i64, Option<i64>, f64, f64)> + '_ {
        let n = self.points.len();
        (0..n).filter_map(move |i| {
            let p = self.points.get(i)?;
            let next = self.points.get(i + 1);
            let b0 = p.bpm.as_f64().max(1.0);
            let b1 = match next {
                Some(q) if p.ramp => q.bpm.as_f64().max(1.0),
                _ => b0,
            };
            Some((p.tick.0, next.map(|q| q.tick.0), b0, b1))
        })
    }

    fn first_bpm(&self) -> f64 {
        self.points
            .front()
            .map_or(120.0, |p| p.bpm.as_f64().max(1.0))
    }

    /// Seconds from tick 0 to `t` (negative for negative ticks).
    pub fn tick_to_seconds(&self, t: Tick) -> f64 {
        if t.0 < 0 || self.points.is_empty() {
            return t.0 as f64 * spt(self.first_bpm());
        }
        let mut secs = 0.0;
        for (start, end, b0, b1) in self.segments() {
            let seg_end = end.unwrap_or(i64::MAX);
            if t.0 < start {
                break;
            }
            let len = (seg_end - start) as f64;
            let dx = (t.0.min(seg_end) - start) as f64;
            secs += seg_seconds(dx, len, b0, b1);
            if t.0 < seg_end {
                break;
            }
        }
        secs
    }

    /// The tick at `s` seconds, rounded to the nearest tick.
    pub fn seconds_to_tick(&self, s: f64) -> Tick {
        if s.is_nan() {
            return Tick::MIN;
        }
        if s.is_infinite() {
            return if s > 0.0 { Tick::MAX } else { Tick::MIN };
        }
        if s < 0.0 || self.points.is_empty() {
            return Tick((s / spt(self.first_bpm())).round() as i64);
        }
        let mut acc = 0.0;
        for (start, end, b0, b1) in self.segments() {
            match end {
                Some(e) => {
                    let len = (e - start) as f64;
                    let seg = seg_seconds(len, len, b0, b1);
                    if s < acc + seg {
                        let dx = seg_ticks(s - acc, len, b0, b1);
                        return Tick((start as f64 + dx).round() as i64);
                    }
                    acc += seg;
                }
                None => {
                    let dx = (s - acc) / spt(b0);
                    return Tick((start as f64 + dx).round() as i64);
                }
            }
        }
        Tick::ZERO
    }
}

/// Seconds spent covering `dx` ticks of a segment `len` ticks long ramping
/// linearly from `b0` to `b1` BPM.
fn seg_seconds(dx: f64, len: f64, b0: f64, b1: f64) -> f64 {
    if (b1 - b0).abs() < 1e-9 || len <= 0.0 {
        return dx * spt(b0);
    }
    let k = (b1 - b0) / len;
    60.0 / (PPQ as f64 * k) * (k * dx / b0).ln_1p()
}

/// Inverse of [`seg_seconds`]: ticks covered after `s` seconds.
fn seg_ticks(s: f64, len: f64, b0: f64, b1: f64) -> f64 {
    if (b1 - b0).abs() < 1e-9 || len <= 0.0 {
        return s / spt(b0);
    }
    let k = (b1 - b0) / len;
    b0 * (s * PPQ as f64 * k / 60.0).exp_m1() / k
}
