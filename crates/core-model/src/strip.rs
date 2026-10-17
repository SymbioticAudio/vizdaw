//! Strip Chart lines (editors spec §4d; Plan 4d Rulings 15–20): a shaped
//! line over a time range, sampled to draw new values or read at an
//! event's position as the amount of a change, and the controller series a
//! line draws.

use crate::automation::{self, AutomationTarget};
use crate::command::Report;
use crate::command::event::{edit_events, insert_specs};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{EventBody, EventSpec};
use crate::ids::{AnyId, EventId, LaneId, SeqId};
use crate::meter::MeterMap;
use crate::modify::{
    Amount, ChangeOp, Curve, Density, ValueTarget, Wave, apply_change, density, legato, lerp,
    narrow, read_value, splitmix, write_value,
};
use crate::sequence::Sequence;
use crate::tempo::{Bpm, TempoMap, TempoPoint};
use crate::time::Tick;
use im::OrdSet;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

/// What a Strip Chart line acts on (Plan 4d Ruling 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineTarget {
    /// A value of the events in range (velocity, release velocity, length,
    /// or any value on the 32-bit scale).
    Value(ValueTarget),
    /// One series of the events in range; drawing writes it into every
    /// selected lane.
    Series(Series),
    /// The tempo of the selected lanes' sequences, in milli-BPM (Plan 4d
    /// Ruling 20).
    Tempo,
}

/// What a Strip Chart line does (Plan 4d Rulings 15, 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineOp {
    /// Write the line: a series is replaced in range; a note value is set.
    Draw,
    /// Each value becomes the line's.
    Set,
    /// Each value times the line's, in percent.
    Scale,
    /// Each value plus the line's.
    Add,
    /// Each value at most the line's.
    LimitMax,
    /// Each value at least the line's.
    LimitMin,
    /// The lowest value in range becomes `start`, the highest `end`.
    Compress,
    /// Events closer than `step` to the last kept one go.
    Thin,
    /// Straight-line values every `step` between neighbours.
    Thicken,
    /// Notes end where the next note in range starts (lengths only).
    Legato,
}

/// A Strip Chart edit: `op` with `line` over `target` (the one
/// modification every Strip Chart operation is, Plan 4d Ruling 15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineChange {
    /// What it acts on.
    pub target: LineTarget,
    /// What it does.
    pub op: LineOp,
    /// The line.
    pub line: Line,
}

impl LineChange {
    /// The undo label.
    pub fn name(&self) -> &'static str {
        match (self.op, self.target) {
            (LineOp::Draw, LineTarget::Tempo) => "Draw Tempo",
            (LineOp::Draw, LineTarget::Series(Series::Automation(_))) => "Draw Automation",
            (LineOp::Draw, LineTarget::Series(Series::Program)) => "Draw Program Changes",
            (LineOp::Draw, LineTarget::Series(_)) => "Draw Controllers",
            (LineOp::Thin, _) => "Thin Out",
            (LineOp::Thicken, _) => "Thicken",
            (LineOp::Compress, _) => "Compress",
            (LineOp::Legato, _) => "Legato",
            (_, LineTarget::Tempo) => "Change Tempo",
            (_, LineTarget::Value(ValueTarget::Velocity)) => "Change Velocity",
            (_, LineTarget::Value(ValueTarget::ReleaseVelocity)) => "Change Release Velocity",
            (_, LineTarget::Value(ValueTarget::Duration)) => "Change Duration",
            _ => "Change Values",
        }
    }

    /// Refuses a bad line, a bad series, and legato on anything but lengths.
    pub fn check(&self) -> Result<()> {
        self.line.check()?;
        if let LineTarget::Series(s) = self.target {
            s.check()?;
        }
        if self.op == LineOp::Legato && self.target != LineTarget::Value(ValueTarget::Duration) {
            return Err(ModelError::invalid("legato changes note lengths only"));
        }
        Ok(())
    }
}

/// Runs `c` for select-and-modify: drawing writes into `lanes`; every other
/// operation changes the events of `targets` its line covers. Returns the
/// events the selection holds afterwards.
pub(crate) fn apply_line(
    d: &mut Document,
    lanes: &OrdSet<(SeqId, LaneId)>,
    targets: &[EventId],
    c: &LineChange,
    r: &mut Report,
) -> Result<Vec<EventId>> {
    c.check()?;
    match (c.target, c.op) {
        (LineTarget::Tempo, op) => {
            tempo_line(d, lanes, op, &c.line)?;
            Ok(targets.to_vec())
        }
        (LineTarget::Series(s), LineOp::Draw) => draw_series(d, lanes, s, &c.line, r),
        (target, op) => change_events(d, targets, target, op, &c.line, r),
    }
}

/// The tempo `v` (milli-BPM) inside 5–999 BPM.
fn bpm(v: i64) -> Bpm {
    let v = v.clamp(i64::from(Bpm::MIN.0), i64::from(Bpm::MAX.0));
    Bpm(u32::try_from(v).unwrap_or(Bpm::MAX.0))
}

/// Runs a tempo line on the tempo map of every sequence among `lanes`
/// (Plan 4d Ruling 20).
fn tempo_line(
    d: &mut Document,
    lanes: &OrdSet<(SeqId, LaneId)>,
    op: LineOp,
    line: &Line,
) -> Result<()> {
    let seqs: BTreeSet<SeqId> = lanes.iter().map(|(s, _)| *s).collect();
    if seqs.is_empty() {
        return Err(ModelError::invalid(
            "select a lane of the sequence whose tempo to change",
        ));
    }
    if line.from.0 < 0 {
        return Err(ModelError::range("line.from", line.from.0));
    }
    for seq in seqs {
        let s = d.sequence(seq)?;
        let ctx = TimeCtx::of(s);
        let points: Vec<TempoPoint> = s.tempo.points().iter().copied().collect();
        let next = match op {
            LineOp::Draw => draw_tempo(&s.tempo, &points, line, ctx)?,
            LineOp::Thin => thin_tempo(&points, line),
            LineOp::Thicken => thicken_tempo(&s.tempo, &points, line)?,
            LineOp::Compress => compress_tempo(&points, line),
            LineOp::Legato => {
                return Err(ModelError::invalid("legato changes note lengths only"));
            }
            LineOp::Set | LineOp::Scale | LineOp::Add | LineOp::LimitMax | LineOp::LimitMin => {
                change_tempo(&points, op, line, ctx)
            }
        };
        let map = TempoMap::new(next)?;
        if map != d.sequence(seq)?.tempo {
            d.seq_mut(seq)?.tempo = map;
        }
    }
    Ok(())
}

/// Points outside the line stay; inside, the line's reduced samples become
/// ramping points (the last holds). Tempo outside the line is unchanged
/// (Rulings P7, P7b):
/// - when the point before the line ramps into it, a point at `from` holds
///   the tempo the map had there and the drawn points begin one tick later
///   (a later sample at that tick wins over the shifted first one);
/// - when a ramp crosses `to`, a ramping point at `to + 1` holds the tempo
///   the map had there, so the rest of that ramp is reproduced.
///
/// A drawn point that lands on a point of the map outside the line wins.
fn draw_tempo(
    map: &TempoMap,
    points: &[TempoPoint],
    line: &Line,
    ctx: TimeCtx<'_>,
) -> Result<Vec<TempoPoint>> {
    let held: Vec<(Tick, i64)> = line
        .samples(ctx)?
        .into_iter()
        .map(|(t, v)| (t, i64::from(bpm(v).0)))
        .collect();
    let mut drawn = reduce(&held, true);
    let at = |t: Tick| bpm((map.bpm_at(t) * 1000.0).round() as i64);
    let mut extra: Vec<TempoPoint> = Vec::new();
    let ramps_in = points
        .iter()
        .rev()
        .find(|p| p.tick < line.from)
        .is_some_and(|p| p.ramp)
        && points.iter().any(|p| p.tick >= line.from);
    if ramps_in {
        extra.push(TempoPoint {
            tick: line.from,
            bpm: at(line.from),
            ramp: false,
        });
        let after = line.from + Tick(1);
        if drawn.get(1).is_some_and(|q| q.0 <= after) {
            drawn.remove(0);
            drawn.retain(|q| q.0 >= after);
        } else if let Some(first) = drawn.first_mut() {
            first.0 = after;
        }
    }
    let crosses = points
        .iter()
        .rev()
        .find(|p| p.tick <= line.to)
        .is_some_and(|p| p.ramp)
        && points.iter().any(|p| p.tick > line.to);
    let beyond = line.to + Tick(1);
    if crosses && !points.iter().any(|p| p.tick == beyond) {
        extra.push(TempoPoint {
            tick: beyond,
            bpm: at(beyond),
            ramp: true,
        });
    }
    let n = drawn.len();
    let mut out: Vec<TempoPoint> = drawn
        .into_iter()
        .enumerate()
        .map(|(i, (tick, v))| TempoPoint {
            tick,
            bpm: bpm(v),
            ramp: i + 1 < n,
        })
        .collect();
    out.extend(extra);
    out.extend(points.iter().copied().filter(|p| !line.covers(p.tick)));
    // On a shared tick the first listed (the drawn one) stays.
    out.sort_by_key(|p| p.tick);
    out.dedup_by_key(|p| p.tick);
    Ok(out)
}

fn change_tempo(
    points: &[TempoPoint],
    op: LineOp,
    line: &Line,
    ctx: TimeCtx<'_>,
) -> Vec<TempoPoint> {
    points
        .iter()
        .map(|p| {
            if !line.covers(p.tick) {
                return *p;
            }
            let v = i64::from(p.bpm.0);
            let a = line.at(p.tick, ctx);
            let next = match op {
                LineOp::Scale => narrow(i128::from(v) * i128::from(a) / 100),
                LineOp::Add => v.saturating_add(a),
                LineOp::LimitMax => v.min(a),
                LineOp::LimitMin => v.max(a),
                _ => a,
            };
            TempoPoint {
                bpm: bpm(next),
                ..*p
            }
        })
        .collect()
}

fn compress_tempo(points: &[TempoPoint], line: &Line) -> Vec<TempoPoint> {
    let inside: Vec<i64> = points
        .iter()
        .filter(|p| line.covers(p.tick))
        .map(|p| i64::from(p.bpm.0))
        .collect();
    let (Some(lo), Some(hi)) = (inside.iter().min().copied(), inside.iter().max().copied()) else {
        return points.to_vec();
    };
    points
        .iter()
        .map(|p| {
            if !line.covers(p.tick) {
                return *p;
            }
            let v = i64::from(p.bpm.0);
            let next = if hi == lo {
                line.start
            } else {
                narrow(
                    i128::from(line.start)
                        + (i128::from(v) - i128::from(lo))
                            * (i128::from(line.end) - i128::from(line.start))
                            / (i128::from(hi) - i128::from(lo)),
                )
            };
            TempoPoint {
                bpm: bpm(next),
                ..*p
            }
        })
        .collect()
}

/// Inside the line, a point closer than `step` to the last kept one goes
/// (the point at tick 0 always stays).
fn thin_tempo(points: &[TempoPoint], line: &Line) -> Vec<TempoPoint> {
    let mut out: Vec<TempoPoint> = Vec::with_capacity(points.len());
    let mut last: Option<Tick> = None;
    for p in points {
        if line.covers(p.tick) {
            if p.tick != Tick::ZERO && last.is_some_and(|l| (p.tick - l).0 < line.step.0) {
                continue;
            }
            last = Some(p.tick);
        }
        out.push(*p);
    }
    out
}

/// Points every `step` between neighbouring points inside the line, at the
/// tempo the map has there (ramping where the earlier point ramps), so the
/// timing does not change.
fn thicken_tempo(map: &TempoMap, points: &[TempoPoint], line: &Line) -> Result<Vec<TempoPoint>> {
    let mut out = points.to_vec();
    let mut added = 0usize;
    for pair in points.windows(2) {
        let [a, b] = pair else { continue };
        if !(line.covers(a.tick) && line.covers(b.tick)) {
            continue;
        }
        let mut t = a.tick + line.step;
        while t < b.tick {
            added += 1;
            if added > MAX_SAMPLES {
                return Err(ModelError::range("line.step", line.step.0));
            }
            out.push(TempoPoint {
                tick: t,
                bpm: bpm((map.bpm_at(t) * 1000.0).round() as i64),
                ramp: a.ramp,
            });
            t = t + line.step;
        }
    }
    out.sort_by_key(|p| p.tick);
    Ok(out)
}

/// Replaces series `s` in `[from, to]` of every lane in `lanes` with the
/// line's reduced samples (Plan 4d Ruling 16).
fn draw_series(
    d: &mut Document,
    lanes: &OrdSet<(SeqId, LaneId)>,
    s: Series,
    line: &Line,
    r: &mut Report,
) -> Result<Vec<EventId>> {
    if lanes.is_empty() {
        return Err(ModelError::invalid("select a lane to draw in"));
    }
    let mut created = Vec::new();
    for (seq, lane) in lanes.iter().copied() {
        // Each sample as the series can hold it (programs are 7-bit), so
        // reduce sees the values that will be written.
        let held: Vec<(Tick, i64)> = line
            .samples(TimeCtx::of(d.sequence(seq)?))?
            .into_iter()
            .map(|(t, v)| (t, Series::value(&s.body(v)).unwrap_or(v)))
            .collect();
        let samples = reduce(&held, s.ramps());
        let old: Vec<EventId> = d
            .lane(seq, lane)?
            .active()
            .map(|t| {
                t.events
                    .values()
                    .filter(|e| line.covers(e.tick) && Series::of(&e.body) == Some(s))
                    .map(|e| e.id)
                    .collect()
            })
            .unwrap_or_default();
        for id in old {
            d.remove_event(id)?;
            r.removed.push(AnyId::Event(id));
        }
        let specs: Vec<EventSpec> = samples
            .iter()
            .map(|(t, v)| EventSpec::new(*t, s.body(*v)))
            .collect();
        created.extend(insert_specs(d, seq, lane, &specs, r)?);
    }
    Ok(created)
}

/// Changes the events of `ids` that `target` covers inside the line's range.
fn change_events(
    d: &mut Document,
    ids: &[EventId],
    target: LineTarget,
    op: LineOp,
    line: &Line,
    r: &mut Report,
) -> Result<Vec<EventId>> {
    let vt = match target {
        LineTarget::Value(vt) => vt,
        LineTarget::Series(_) => ValueTarget::Value,
        // `apply_line` runs tempo lines itself.
        LineTarget::Tempo => return Ok(Vec::new()),
    };
    let mut hit: Vec<(EventId, Tick)> = Vec::new();
    for id in ids.iter().copied().collect::<BTreeSet<_>>() {
        let (ev, _) = d.event(id)?;
        let fits = match target {
            LineTarget::Value(vt) => read_value(ev, vt).is_some(),
            LineTarget::Series(s) => Series::of(&ev.body) == Some(s),
            LineTarget::Tempo => false,
        };
        if fits && line.covers(ev.tick) {
            hit.push((id, ev.tick));
        }
    }
    let changed: Vec<EventId> = hit.iter().map(|h| h.0).collect();
    match op {
        LineOp::Legato => legato(d, &changed, r)?,
        LineOp::Compress => apply_change(
            d,
            &changed,
            vt,
            ChangeOp::Compress {
                low: line.start,
                high: line.end,
            },
            &Amount::Constant { value: 0 },
            r,
        )?,
        LineOp::Thin => {
            return density(d, &changed, Density::Thin { spacing: line.step }, r);
        }
        LineOp::Thicken => {
            return density(d, &changed, Density::Thicken { spacing: line.step }, r);
        }
        LineOp::Draw
        | LineOp::Set
        | LineOp::Scale
        | LineOp::Add
        | LineOp::LimitMax
        | LineOp::LimitMin => {
            for (id, tick) in &hit {
                let (a, v) = {
                    let (ev, loc) = d.event(*id)?;
                    let s = d.sequence(loc.seq)?;
                    (
                        line.at(*tick, TimeCtx::of(s)),
                        read_value(ev, vt).unwrap_or(0),
                    )
                };
                let next = match op {
                    LineOp::Scale => narrow(i128::from(v) * i128::from(a) / 100),
                    LineOp::Add => v.saturating_add(a),
                    LineOp::LimitMax => v.min(a),
                    LineOp::LimitMin => v.max(a),
                    _ => a,
                };
                // A note that had a length keeps at least one tick unless
                // the line sets it (as Change does).
                let next = if vt == ValueTarget::Duration
                    && v > 0
                    && !matches!(op, LineOp::Draw | LineOp::Set)
                {
                    next.max(1)
                } else {
                    next
                };
                edit_events(d, &[*id], r, |e| {
                    write_value(e, vt, next);
                    Ok(())
                })?;
            }
        }
    }
    Ok(changed)
}
