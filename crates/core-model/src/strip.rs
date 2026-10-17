//! Strip Chart lines (editors spec §4d; Plan 4d Rulings 15–20): a shaped
//! line over a time range, sampled to draw new values or read at an
//! event's position as the amount of a change, and the controller series a
//! line draws.

use crate::automation::{self, AutomationTarget};
use crate::error::{ModelError, Result};
use crate::event::EventBody;
use crate::meter::MeterMap;
use crate::modify::{Curve, Wave, lerp, splitmix};
use crate::sequence::Sequence;
use crate::tempo::TempoMap;
use crate::time::Tick;
use serde::{Deserialize, Serialize};

/// The most samples one line may write.
pub const MAX_SAMPLES: usize = 100_000;

/// How an oscillator's period is measured (Plan 4d Ruling 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    /// Musical: this many ticks.
    Ticks(Tick),
    /// Clock: this many milliseconds, through the sequence's tempo map,
    /// counted from the line's `from`.
    Millis(u32),
    /// Following the meter: this many bars, each as long as its meter.
    /// Like `Millis`, counted from the line's `from`, not from a barline: a
    /// line that starts mid-bar starts its wave mid-bar.
    Bars(u32),
}

/// A line's shape between its ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LineShape {
    /// From `start` to `end` in a straight line.
    Straight,
    /// From `start` to `end` along a curve (fades).
    Curved {
        /// The curve.
        curve: Curve,
    },
    /// `start` all along.
    Flat,
    /// Through drawn points (ascending ticks), holding the first before it
    /// and the last after it.
    Free {
        /// `(tick, value)` points.
        points: Vec<(Tick, i64)>,
    },
    /// A value per tick between `start` and `end`, fixed by `seed`.
    Random {
        /// Seed.
        seed: u64,
    },
    /// A wave from `start` (its low) to `end` (its high).
    Oscillator {
        /// Wave.
        wave: Wave,
        /// Period.
        period: Period,
        /// Phase shift in thousandths of a period (0–999).
        phase: u16,
    },
}

/// A Strip Chart line over `[from, to]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    /// First tick.
    pub from: Tick,
    /// Last tick (at or after `from`).
    pub to: Tick,
    /// Value at `from` (an oscillator's low, a random line's one bound).
    pub start: i64,
    /// Value at `to` (an oscillator's high, a random line's other bound).
    pub end: i64,
    /// Distance between drawn samples, and the spacing of Thin and Thicken.
    pub step: Tick,
    /// Shape.
    pub shape: LineShape,
}

/// What clock and meter-following periods are measured against: the
/// sequence's maps.
#[derive(Debug, Clone, Copy)]
pub struct TimeCtx<'a> {
    /// Tempo map.
    pub tempo: &'a TempoMap,
    /// Meter map.
    pub meter: &'a MeterMap,
}

impl<'a> TimeCtx<'a> {
    /// The maps of sequence `s`.
    pub fn of(s: &'a Sequence) -> TimeCtx<'a> {
        TimeCtx {
            tempo: &s.tempo,
            meter: &s.meter,
        }
    }
}

impl Line {
    /// Refuses a line that cannot be drawn: `to` before `from`, a step of 0
    /// or less, free points out of order, an oscillator period of 0 or a
    /// phase above 999.
    pub fn check(&self) -> Result<()> {
        if self.to < self.from {
            return Err(ModelError::range("line.to", self.to.0));
        }
        if self.step.0 <= 0 {
            return Err(ModelError::range("line.step", self.step.0));
        }
        match &self.shape {
            LineShape::Free { points } => {
                if !points.windows(2).all(|w| matches!(w, [a, b] if a.0 < b.0)) {
                    return Err(ModelError::invalid(
                        "a free line's points must be in time order",
                    ));
                }
            }
            LineShape::Oscillator { period, phase, .. } => {
                let ok = match period {
                    Period::Ticks(t) => t.0 > 0,
                    Period::Millis(ms) => *ms > 0,
                    Period::Bars(n) => *n > 0,
                };
                if !ok {
                    return Err(ModelError::invalid(
                        "an oscillator's period must be longer than 0",
                    ));
                }
                if *phase > 999 {
                    return Err(ModelError::range("line.phase", phase));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Whether tick `t` lies in `[from, to]`.
    pub fn covers(&self, t: Tick) -> bool {
        t >= self.from && t <= self.to
    }

    /// The line's value at `t` (ends held outside the range).
    pub fn at(&self, t: Tick, ctx: TimeCtx<'_>) -> i64 {
        let span = self.to.0.saturating_sub(self.from.0);
        let x = if span > 0 {
            (t.0.saturating_sub(self.from.0) as f64 / span as f64).clamp(0.0, 1.0)
        } else if t > self.from {
            1.0
        } else {
            0.0
        };
        match &self.shape {
            LineShape::Straight => lerp(self.start, self.end, x),
            LineShape::Curved { curve } => lerp(self.start, self.end, curve.apply(x)),
            LineShape::Flat => self.start,
            LineShape::Free { points } => free_at(points, t, self.start),
            LineShape::Random { seed } => random_at(self.start, self.end, *seed, t),
            LineShape::Oscillator {
                wave,
                period,
                phase,
            } => {
                let c = cycles(self.from, t, *period, ctx) + f64::from(*phase) / 1000.0;
                lerp(
                    self.start,
                    self.end,
                    (wave.value(c.rem_euclid(1.0)) + 1.0) / 2.0,
                )
            }
        }
    }

    /// The values to draw: every `step` from `from`, at `to`, and at a free
    /// line's own points inside the range.
    pub fn samples(&self, ctx: TimeCtx<'_>) -> Result<Vec<(Tick, i64)>> {
        self.check()?;
        let span = self.to.0.saturating_sub(self.from.0);
        let count = usize::try_from(span / self.step.0).unwrap_or(usize::MAX);
        if count > MAX_SAMPLES {
            return Err(ModelError::range("line.step", self.step.0));
        }
        let mut ticks: Vec<Tick> = Vec::with_capacity(count.saturating_add(2));
        let mut t = self.from;
        while t < self.to {
            ticks.push(t);
            t = t + self.step;
        }
        ticks.push(self.to);
        if let LineShape::Free { points } = &self.shape {
            ticks.extend(points.iter().map(|p| p.0).filter(|t| self.covers(*t)));
            ticks.sort();
            ticks.dedup();
        }
        Ok(ticks.into_iter().map(|t| (t, self.at(t, ctx))).collect())
    }
}

fn free_at(points: &[(Tick, i64)], t: Tick, fallback: i64) -> i64 {
    let n = points.partition_point(|p| p.0 <= t);
    match (n.checked_sub(1).and_then(|i| points.get(i)), points.get(n)) {
        (None, None) => fallback,
        (None, Some(next)) => next.1,
        (Some(prev), None) => prev.1,
        (Some(prev), Some(next)) => {
            let span = next.0.0.saturating_sub(prev.0.0);
            let x = if span > 0 {
                t.0.saturating_sub(prev.0.0) as f64 / span as f64
            } else {
                0.0
            };
            lerp(prev.1, next.1, x)
        }
    }
}

fn random_at(a: i64, b: i64, seed: u64, t: Tick) -> i64 {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let span = i128::from(hi) - i128::from(lo) + 1;
    let h = i128::from(splitmix(seed ^ splitmix(t.0 as u64)));
    i64::try_from(i128::from(lo) + h % span).unwrap_or(hi)
}

/// Periods from `from` to `t`.
fn cycles(from: Tick, t: Tick, p: Period, ctx: TimeCtx<'_>) -> f64 {
    match p {
        Period::Ticks(n) => t.0.saturating_sub(from.0) as f64 / n.0.max(1) as f64,
        Period::Millis(ms) => {
            (ctx.tempo.tick_to_seconds(t) - ctx.tempo.tick_to_seconds(from)) * 1000.0
                / f64::from(ms.max(1))
        }
        Period::Bars(n) => (ctx.meter.bars_at(t) - ctx.meter.bars_at(from)) / f64::from(n.max(1)),
    }
}

/// The samples a draw writes (Plan 4d Ruling 16, fix round 1): the first and
/// last always; between them, on a holding series (`ramps` false) a sample
/// equal to the previous kept one goes; on a ramping series a sample goes
/// when the straight line from the last kept sample to the sample after it
/// stays within one unit of every sample dropped since, so error never adds
/// up (a straight line writes two points, a curve stays a curve).
pub fn reduce(samples: &[(Tick, i64)], ramps: bool) -> Vec<(Tick, i64)> {
    let mut out: Vec<(Tick, i64)> = Vec::with_capacity(samples.len());
    if !ramps {
        for (i, p) in samples.iter().enumerate() {
            let last = i + 1 == samples.len();
            if last || out.last().is_none_or(|prev| prev.1 != p.1) {
                out.push(*p);
            }
        }
        return out;
    }
    // Slopes (per tick) of the lines from the anchor that pass within one
    // unit of every sample seen since it.
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    let mut prev: Option<(Tick, i64)> = None;
    for p in samples {
        let Some(anchor) = out.last().copied() else {
            out.push(*p);
            prev = Some(*p);
            continue;
        };
        let dt = p.0.0.saturating_sub(anchor.0.0) as f64;
        let rise = p.1.saturating_sub(anchor.1) as f64;
        let fits = dt > 0.0 && {
            let slope = rise / dt;
            slope >= lo - 1e-9 && slope <= hi + 1e-9
        };
        if fits {
            lo = lo.max((rise - 1.0) / dt);
            hi = hi.min((rise + 1.0) / dt);
        } else {
            // Keep the sample before this one and start a new stretch.
            let keep = prev.filter(|q| *q != anchor).unwrap_or(*p);
            out.push(keep);
            let a = keep;
            let dt = p.0.0.saturating_sub(a.0.0) as f64;
            let rise = p.1.saturating_sub(a.1) as f64;
            if dt > 0.0 && keep != *p {
                lo = (rise - 1.0) / dt;
                hi = (rise + 1.0) / dt;
            } else {
                (lo, hi) = (f64::NEG_INFINITY, f64::INFINITY);
            }
        }
        prev = Some(*p);
    }
    if let (Some(last), Some(kept)) = (prev, out.last().copied())
        && last != kept
    {
        out.push(last);
    }
    out
}

/// A controller-like series a line draws (Plan 4d Ruling 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Series {
    /// Control change `n`.
    Controller(u8),
    /// Pitch bend.
    PitchBend,
    /// Channel pressure.
    ChannelPressure,
    /// Polyphonic pressure on one pitch.
    PolyPressure(u8),
    /// Program changes.
    Program,
    /// Automation of one target.
    Automation(AutomationTarget),
}

impl Series {
    /// The series `b` belongs to.
    pub fn of(b: &EventBody) -> Option<Series> {
        Some(match b {
            EventBody::Cc { index, .. } => Series::Controller(*index),
            EventBody::PitchBend { .. } => Series::PitchBend,
            EventBody::ChannelPressure { .. } => Series::ChannelPressure,
            EventBody::PolyPressure { pitch, .. } => Series::PolyPressure(*pitch),
            EventBody::Program { .. } => Series::Program,
            EventBody::Automation { target, .. } => Series::Automation(*target),
            _ => return None,
        })
    }

    /// `b`'s value on the 32-bit scale (Plan 4d Ruling 18): controller,
    /// bend and pressure values as they are, programs × 2²⁵, automation
    /// × (2³² − 1).
    pub fn value(b: &EventBody) -> Option<i64> {
        match b {
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. } => Some(i64::from(*value)),
            EventBody::Program { program, .. } => Some(i64::from(*program) << 25),
            EventBody::Automation { value, .. } => Some(i64::from(automation::to_u32(*value))),
            _ => None,
        }
    }

    /// Sets `b`'s value from the 32-bit scale (clamped; a program keeps its
    /// bank).
    pub fn set_value(b: &mut EventBody, v: i64) {
        let v32 = u32::try_from(v.clamp(0, i64::from(u32::MAX))).unwrap_or(u32::MAX);
        match b {
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. } => *value = v32,
            EventBody::Program { program, .. } => {
                *program = u8::try_from(v32 >> 25).unwrap_or(127);
            }
            EventBody::Automation { value, .. } => *value = automation::from_u32(v32),
            _ => {}
        }
    }

    /// A body of this series holding 32-bit-scale value `v`.
    pub fn body(self, v: i64) -> EventBody {
        let mut b = match self {
            Series::Controller(index) => EventBody::Cc { index, value: 0 },
            Series::PitchBend => EventBody::PitchBend { value: 0 },
            Series::ChannelPressure => EventBody::ChannelPressure { value: 0 },
            Series::PolyPressure(pitch) => EventBody::PolyPressure { pitch, value: 0 },
            Series::Program => EventBody::Program {
                program: 0,
                bank: None,
            },
            Series::Automation(target) => EventBody::Automation { target, value: 0.0 },
        };
        Series::set_value(&mut b, v);
        b
    }

    /// Values ramp between points (continuous automation); everything
    /// else holds until the next event.
    pub fn ramps(self) -> bool {
        matches!(self, Series::Automation(t) if !t.is_stepped())
    }

    /// Refuses a controller number or pitch above 127.
    pub fn check(self) -> Result<()> {
        match self {
            Series::Controller(n) | Series::PolyPressure(n) if n > 127 => {
                Err(ModelError::range("series", n))
            }
            _ => Ok(()),
        }
    }
}
