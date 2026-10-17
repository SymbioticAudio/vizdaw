//! Modifications applied by select-and-modify.

use crate::command::Report;
use crate::command::event::{edit_events, shift_pitch};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{Event, EventBody};
use crate::ids::{AnyId, DestId, EventId};
use crate::rule::{Rule, SelectMode};
use crate::time::Tick;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A change applied to every selected event it fits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Modify {
    /// Change note and poly-pressure pitches.
    Transpose(Transpose),
    /// Move note starts toward a grid or groove.
    Quantize(Quantize),
    /// Change note velocities (clamped to 0..=65535).
    Velocity(ValueOp),
    /// Change note durations.
    Duration(DurationOp),
    /// Extend each note to the next selected note's start in its lane.
    Legato,
    /// Set the destination override.
    ReassignDest {
        /// New override.
        dest: Option<DestId>,
    },
    /// Scale positions (and note durations) around `anchor` by `num/den`.
    ScaleTime {
        /// Numerator (>= 1).
        num: u32,
        /// Denominator (>= 1).
        den: u32,
        /// Fixed point.
        anchor: Tick,
    },
    /// Move by `delta`.
    Move {
        /// Offset.
        delta: Tick,
    },
    /// Change CC, pitch-bend and pressure values (clamped to u32).
    ControllerValue(ValueOp),
    /// Map pitches: a note or poly pressure whose pitch is a `from` takes
    /// its `to`.
    NoteMap {
        /// `(from, to)` pairs, each 0–127.
        map: Vec<(u8, u8)>,
    },
    /// Move note starts by `delta`, keeping their ends (at least 1 tick).
    TrimStart {
        /// Offset of the start.
        delta: Tick,
    },
    /// Turn controller kinds into another, or notes into one pitch.
    Substitute(Substitute),
    /// Thin out or thicken controller series.
    Density(Density),
    /// Delete the events.
    Delete,
    /// Change one value of each fitting event by `op` with `amount`.
    Change {
        /// Which value.
        target: ValueTarget,
        /// How the amount combines with it.
        op: ChangeOp,
        /// The amount (percent for `Scale`).
        amount: Amount,
    },
}

/// What [`Modify::Substitute`] turns events into (Plan 4c Ruling 5).
///
/// The model stores every controller kind as a full-range 32-bit value, so
/// a kind change carries the stored value over unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Substitute {
    /// CC, pitch bend and channel pressure become CC `index`.
    Controller {
        /// Controller number 0–127.
        index: u8,
    },
    /// CC, pitch bend and channel pressure become pitch bend.
    PitchBend,
    /// CC, pitch bend and channel pressure become channel pressure.
    ChannelPressure,
    /// Notes and poly pressure take `pitch`.
    Pitch {
        /// Pitch 0–127.
        pitch: u8,
    },
}

/// Controller-series density changes (Plan 4c Ruling 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Density {
    /// Remove events closer than `spacing` to the last kept one.
    Thin {
        /// Minimum distance (> 0).
        spacing: Tick,
    },
    /// Add straight-line values every `spacing` between neighbours.
    Thicken {
        /// Distance between added events (> 0).
        spacing: Tick,
    },
}

/// The most events one Thicken may add.
const MAX_THICKEN: usize = 100_000;

/// Which value [`Modify::Change`] edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueTarget {
    /// Note velocity (16-bit).
    Velocity,
    /// Note release velocity (16-bit).
    ReleaseVelocity,
    /// Note length in ticks.
    Duration,
    /// CC, pitch-bend and pressure values (32-bit).
    Value,
}

/// How [`Modify::Change`] combines its amount with each value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChangeOp {
    /// The amount.
    Set,
    /// The value times the amount, in percent.
    Scale,
    /// The value plus the amount.
    Add,
    /// The value kept inside `min..=max`.
    Limit {
        /// Lowest kept.
        min: i64,
        /// Highest kept.
        max: i64,
    },
    /// The lowest changed value becomes `low`, the highest `high`, the rest
    /// in proportion.
    Compress {
        /// New lowest.
        low: i64,
        /// New highest.
        high: i64,
    },
    /// Notes end where the next changed note of their lane starts
    /// (durations only).
    Legato,
}

/// The shape of a [`Amount::Line`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    /// Straight.
    Straight,
    /// Slow, then fast.
    EaseIn,
    /// Fast, then slow.
    EaseOut,
    /// Slow at both ends.
    SCurve,
}

/// The wave of an [`Amount::Oscillator`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wave {
    /// Sine, starting in the middle and rising.
    Sine,
    /// High for the first half period, low for the second.
    Square,
    /// From low up to high at half a period, back down.
    Triangle,
    /// From low rising to high, then jumping back.
    Saw,
}

/// An amount that may change with each event's position (Plan 4c Ruling 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Amount {
    /// The same for every event.
    Constant {
        /// The amount.
        value: i64,
    },
    /// From `from` at the first changed event to `to` at the last.
    Line {
        /// At the first event.
        from: i64,
        /// At the last event.
        to: i64,
        /// Shape.
        curve: Curve,
    },
    /// A wave between `low` and `high`, measured from the first event.
    Oscillator {
        /// Wave.
        wave: Wave,
        /// Lowest.
        low: i64,
        /// Highest.
        high: i64,
        /// Period (> 0).
        period: Tick,
        /// Shift of the wave.
        phase: Tick,
    },
    /// A value in `low..=high` per event, fixed by `seed` and the event.
    Random {
        /// Lowest.
        low: i64,
        /// Highest.
        high: i64,
        /// Seed.
        seed: u64,
    },
}

pub(crate) fn lerp(a: i64, b: i64, y: f64) -> i64 {
    (a as f64 + (b as f64 - a as f64) * y).round() as i64
}

pub(crate) fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Curve {
    /// The curve's height (0–1) at `x` (0–1).
    pub fn apply(self, x: f64) -> f64 {
        match self {
            Curve::Straight => x,
            Curve::EaseIn => x * x,
            Curve::EaseOut => 1.0 - (1.0 - x) * (1.0 - x),
            Curve::SCurve => x * x * (3.0 - 2.0 * x),
        }
    }
}

impl Wave {
    /// The wave (−1 … 1) at phase `ph` (0 … 1 of a period).
    pub fn value(self, ph: f64) -> f64 {
        match self {
            Wave::Sine => (std::f64::consts::TAU * ph).sin(),
            Wave::Square => {
                if ph < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Triangle => 1.0 - 4.0 * (ph - 0.5).abs(),
            Wave::Saw => 2.0 * ph - 1.0,
        }
    }
}

impl Amount {
    /// The amount for event `id` at `t` when the changed events start
    /// between `first` and `last`.
    pub fn at(&self, t: Tick, first: Tick, last: Tick, id: EventId) -> i64 {
        match *self {
            Amount::Constant { value } => value,
            Amount::Line { from, to, curve } => {
                let span = (last - first).0;
                let x = if span > 0 {
                    (t - first).0 as f64 / span as f64
                } else {
                    0.0
                };
                lerp(from, to, curve.apply(x))
            }
            Amount::Oscillator {
                wave,
                low,
                high,
                period,
                phase,
            } => {
                let p = period.0.max(1);
                let ph = (t - first).0.saturating_add(phase.0).rem_euclid(p) as f64 / p as f64;
                lerp(low, high, (wave.value(ph) + 1.0) / 2.0)
            }
            Amount::Random { low, high, seed } => {
                let (lo, hi) = if low <= high {
                    (low, high)
                } else {
                    (high, low)
                };
                let span = i128::from(hi) - i128::from(lo) + 1;
                let h = i128::from(splitmix(seed ^ splitmix(id.0)));
                i64::try_from(i128::from(lo) + h % span).unwrap_or(hi)
            }
        }
    }
}

/// Pitch changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Transpose {
    /// By `steps` degrees of a scale ([`scale_step`]).
    Diatonic {
        /// Degrees (negative: down).
        steps: i8,
        /// Root pitch class 0–11.
        root: u8,
        /// 12-bit scale mask.
        mask: u16,
    },
    /// Mirrored around `axis` (clamped at 0 and 127).
    Invert {
        /// The pitch that stays.
        axis: u8,
    },
    /// By an interval (clamped at 0 and 127).
    Interval {
        /// Semitones.
        semitones: i8,
    },
    /// Snap to the nearest pitch of a scale (ties go down). Bit `i` of
    /// `mask` enables pitch class `root + i`.
    ToScale {
        /// Root pitch class 0–11.
        root: u8,
        /// 12-bit scale mask.
        mask: u16,
    },
}

/// Quantize kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Quantize {
    /// Snap to `grid` with `strength` percent.
    Grid {
        /// Grid (> 0).
        grid: Tick,
        /// Strength 0–100.
        strength: u8,
    },
    /// Snap to `grid`, then add `offsets[k % len]` for grid line `k`.
    Groove {
        /// Grid (> 0).
        grid: Tick,
        /// Per-line offsets (non-empty).
        offsets: Vec<Tick>,
        /// Strength 0–100.
        strength: u8,
    },
}

/// Integer value changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueOp {
    /// Set to a value.
    Set(u32),
    /// Scale by a percentage.
    ScalePercent(u32),
    /// Add a signed amount.
    Add(i64),
}

/// Duration changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurationOp {
    /// Set to a length (>= 0).
    Set(Tick),
    /// Scale by a percentage.
    ScalePercent(u32),
}

/// A named select-and-modify template, stored in document settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModifyTemplate {
    /// Selection rule.
    pub rule: Option<Rule>,
    /// Selection mode.
    pub mode: SelectMode,
    /// Modification.
    pub modify: Option<Modify>,
    /// Modify copies inserted beside the originals (a harmonizing copy).
    #[serde(default)]
    pub to_copy: bool,
}

impl Modify {
    /// Stable name, used as the undo label.
    pub fn name(&self) -> &'static str {
        match self {
            Modify::Transpose(_) => "Transpose",
            Modify::Quantize(_) => "Quantize",
            Modify::Velocity(_) => "Change Velocity",
            Modify::Duration(_) => "Change Duration",
            Modify::Legato => "Legato",
            Modify::ReassignDest { .. } => "Reassign Destination",
            Modify::ScaleTime { .. } => "Scale Time",
            Modify::Move { .. } => "Move",
            Modify::ControllerValue(_) => "Change Controller Values",
            Modify::NoteMap { .. } => "Transpose",
            Modify::TrimStart { .. } => "Trim Start",
            Modify::Substitute(_) => "Substitute",
            Modify::Density(Density::Thin { .. }) => "Thin Out",
            Modify::Density(Density::Thicken { .. }) => "Thicken",
            Modify::Delete => "Delete Events",
            Modify::Change { target, .. } => match target {
                ValueTarget::Velocity => "Change Velocity",
                ValueTarget::ReleaseVelocity => "Change Release Velocity",
                ValueTarget::Duration => "Change Duration",
                ValueTarget::Value => "Change Values",
            },
        }
    }
}

fn apply_op(v: u64, op: ValueOp, max: u64) -> u64 {
    let r: i128 = match op {
        ValueOp::Set(x) => i128::from(x),
        ValueOp::ScalePercent(p) => i128::from(v) * i128::from(p) / 100,
        ValueOp::Add(a) => i128::from(v) + i128::from(a),
    };
    u64::try_from(r.clamp(0, i128::from(max))).unwrap_or(max)
}

fn snap_to_scale(p: u8, root: u8, mask: u16) -> u8 {
    if mask & 0x0fff == 0 {
        return p;
    }
    let in_scale = |q: i32| {
        let pc = (q - i32::from(root % 12)).rem_euclid(12);
        mask & (1 << pc) != 0
    };
    for dist in 0..12 {
        for q in [i32::from(p) - dist, i32::from(p) + dist] {
            if (0..=127).contains(&q) && in_scale(q) {
                return shift_pitch(0, q);
            }
        }
    }
    p
}

/// `p` moved `steps` degrees of a scale (bit `i` of `mask` enables pitch
/// class `root + i`): an off-scale pitch reaching the next scale pitch in
/// the step's direction counts as one step; it stops at the last scale
/// pitch inside 0–127. An empty mask or zero steps leave `p` unchanged.
pub fn scale_step(p: u8, steps: i32, root: u8, mask: u16) -> u8 {
    if mask & 0x0fff == 0 || steps == 0 {
        return p;
    }
    let in_scale = |q: i32| {
        let pc = (q - i32::from(root % 12)).rem_euclid(12);
        mask & (1 << pc) != 0
    };
    let dir = steps.signum();
    let mut q = i32::from(p);
    for _ in 0..steps.unsigned_abs() {
        match (1..=12).map(|d| q + dir * d).find(|c| in_scale(*c)) {
            Some(c) if (0..=127).contains(&c) => q = c,
            _ => break,
        }
    }
    shift_pitch(0, q)
}

fn quantize_tick(t: Tick, q: &Quantize) -> Result<Tick> {
    let (grid, strength, offsets): (Tick, u8, &[Tick]) = match q {
        Quantize::Grid { grid, strength } => (*grid, *strength, &[]),
        Quantize::Groove {
            grid,
            offsets,
            strength,
        } => {
            if offsets.is_empty() {
                return Err(ModelError::invalid("groove has no offsets"));
            }
            (*grid, *strength, offsets.as_slice())
        }
    };
    if grid.0 <= 0 || strength > 100 {
        return Err(ModelError::range("quantize.grid", grid.0));
    }
    let g = grid.0;
    let k = t.0.saturating_add(g / 2).div_euclid(g);
    let mut target = k.saturating_mul(g);
    if !offsets.is_empty() {
        let len = i64::try_from(offsets.len()).unwrap_or(1);
        let idx = usize::try_from(k.rem_euclid(len)).unwrap_or(0);
        target = target.saturating_add(offsets.get(idx).map_or(0, |o| o.0));
    }
    let delta = i128::from(target - t.0) * i128::from(strength) / 100;
    Ok(Tick(t.0.saturating_add(i64::try_from(delta).unwrap_or(0))))
}

fn scale(t: i64, num: u32, den: u32) -> i64 {
    let r = i128::from(t) * i128::from(num) / i128::from(den.max(1));
    i64::try_from(r).unwrap_or(if r < 0 { i64::MIN } else { i64::MAX })
}

fn modify_one(e: &mut Event, m: &Modify) -> Result<()> {
    match m {
        Modify::Transpose(t) => match &mut e.body {
            EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                *pitch = match t {
                    Transpose::Interval { semitones } => shift_pitch(*pitch, i32::from(*semitones)),
                    Transpose::ToScale { root, mask } => snap_to_scale(*pitch, *root, *mask),
                    Transpose::Diatonic { steps, root, mask } => {
                        scale_step(*pitch, i32::from(*steps), *root, *mask)
                    }
                    Transpose::Invert { axis } => {
                        shift_pitch(0, 2 * i32::from(*axis) - i32::from(*pitch))
                    }
                }
            }
            _ => {}
        },
        Modify::NoteMap { map } => {
            if let EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } =
                &mut e.body
            {
                if let Some((_, to)) = map.iter().find(|(from, _)| *from == *pitch) {
                    *pitch = *to;
                }
            }
        }
        // Applied to the whole set in `apply_modify`.
        Modify::Change { .. } | Modify::Density(_) | Modify::Delete => {}
        Modify::TrimStart { delta } => {
            if let EventBody::Note { duration, .. } = &mut e.body {
                if duration.0 > 0 {
                    let end = e.tick + *duration;
                    let start = (e.tick + *delta).min(end - Tick(1));
                    *duration = end - start;
                    e.tick = start;
                }
            }
        }
        Modify::Substitute(s) => {
            let value = match e.body {
                EventBody::Cc { value, .. }
                | EventBody::PitchBend { value }
                | EventBody::ChannelPressure { value } => Some(value),
                _ => None,
            };
            match (s, value) {
                (Substitute::Controller { index }, Some(value)) => {
                    e.body = EventBody::Cc {
                        index: *index,
                        value,
                    }
                }
                (Substitute::PitchBend, Some(value)) => e.body = EventBody::PitchBend { value },
                (Substitute::ChannelPressure, Some(value)) => {
                    e.body = EventBody::ChannelPressure { value }
                }
                (Substitute::Pitch { pitch: to }, _) => {
                    if let EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } =
                        &mut e.body
                    {
                        *pitch = *to;
                    }
                }
                _ => {}
            }
        }
        Modify::Quantize(q) => {
            if let EventBody::Note { .. } = e.body {
                e.tick = quantize_tick(e.tick, q)?;
            }
        }
        Modify::Velocity(op) => {
            if let EventBody::Note { velocity, .. } = &mut e.body {
                let v = apply_op(u64::from(*velocity), *op, u64::from(u16::MAX));
                *velocity = u16::try_from(v).unwrap_or(u16::MAX);
            }
        }
        Modify::Duration(op) => {
            if let EventBody::Note { duration, .. } = &mut e.body {
                *duration = match op {
                    DurationOp::Set(t) => *t,
                    DurationOp::ScalePercent(p) => Tick(scale(duration.0, *p, 100)),
                };
            }
        }
        Modify::Legato => {}
        Modify::ReassignDest { dest } => e.dest = *dest,
        Modify::ScaleTime { num, den, anchor } => {
            if *num == 0 || *den == 0 {
                return Err(ModelError::range("scale_time", format!("{num}/{den}")));
            }
            e.tick = *anchor + Tick(scale((e.tick - *anchor).0, *num, *den));
            if let EventBody::Note { duration, .. } = &mut e.body {
                // A note that had a length keeps at least one tick, as
                // Resize and relative changes do (Plan 4d Ruling 1).
                let scaled = scale(duration.0, *num, *den);
                *duration = Tick(if duration.0 > 0 {
                    scaled.max(1)
                } else {
                    scaled
                });
            }
        }
        Modify::Move { delta } => e.tick = e.tick + *delta,
        Modify::ControllerValue(op) => match &mut e.body {
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. } => {
                let v = apply_op(u64::from(*value), *op, u64::from(u32::MAX));
                *value = u32::try_from(v).unwrap_or(u32::MAX);
            }
            _ => {}
        },
    }
    Ok(())
}

/// Notes end where the next of `ids` in their lane starts.
fn legato(d: &mut Document, ids: &[EventId], r: &mut Report) -> Result<()> {
    let mut by_lane: BTreeMap<_, Vec<(Tick, EventId)>> = BTreeMap::new();
    for id in ids {
        let (ev, loc) = d.event(*id)?;
        if let EventBody::Note { .. } = ev.body {
            by_lane
                .entry((loc.seq, loc.lane))
                .or_default()
                .push((ev.tick, *id));
        }
    }
    for notes in by_lane.values_mut() {
        notes.sort();
        for pair in notes.windows(2) {
            if let [(t0, id), (t1, _)] = pair {
                if t1 > t0 {
                    let len = *t1 - *t0;
                    edit_events(d, &[*id], r, |e| {
                        if let EventBody::Note { duration, .. } = &mut e.body {
                            *duration = len;
                        }
                        Ok(())
                    })?;
                }
            }
        }
    }
    Ok(())
}

fn read_value(e: &Event, t: ValueTarget) -> Option<i64> {
    match (t, &e.body) {
        (ValueTarget::Velocity, EventBody::Note { velocity, .. }) => Some(i64::from(*velocity)),
        (
            ValueTarget::ReleaseVelocity,
            EventBody::Note {
                release_velocity, ..
            },
        ) => Some(i64::from(*release_velocity)),
        (ValueTarget::Duration, EventBody::Note { duration, .. }) => Some(duration.0),
        (
            ValueTarget::Value,
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. },
        ) => Some(i64::from(*value)),
        _ => None,
    }
}

fn write_value(e: &mut Event, t: ValueTarget, v: i64) {
    let v16 = u16::try_from(v.clamp(0, i64::from(u16::MAX))).unwrap_or(u16::MAX);
    let v32 = u32::try_from(v.clamp(0, i64::from(u32::MAX))).unwrap_or(u32::MAX);
    match (t, &mut e.body) {
        (ValueTarget::Velocity, EventBody::Note { velocity, .. }) => *velocity = v16,
        (
            ValueTarget::ReleaseVelocity,
            EventBody::Note {
                release_velocity, ..
            },
        ) => *release_velocity = v16,
        (ValueTarget::Duration, EventBody::Note { duration, .. }) => *duration = Tick(v.max(0)),
        (
            ValueTarget::Value,
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. },
        ) => *value = v32,
        _ => {}
    }
}

fn narrow(v: i128) -> i64 {
    i64::try_from(v).unwrap_or(if v < 0 { i64::MIN } else { i64::MAX })
}

fn apply_change(
    d: &mut Document,
    ids: &[EventId],
    target: ValueTarget,
    op: ChangeOp,
    amount: &Amount,
    r: &mut Report,
) -> Result<()> {
    if let ChangeOp::Legato = op {
        if target != ValueTarget::Duration {
            return Err(ModelError::invalid("legato changes note lengths only"));
        }
        return legato(d, ids, r);
    }
    if let Amount::Oscillator { period, .. } = amount {
        if period.0 <= 0 {
            return Err(ModelError::range("amount.period", period.0));
        }
    }
    if let ChangeOp::Limit { min, max } = op {
        if min > max {
            return Err(ModelError::range("limit", format!("{min}..{max}")));
        }
    }
    let mut found: Vec<(EventId, Tick, i64)> = Vec::new();
    for id in ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
    {
        let (ev, _) = d.event(id)?;
        if let Some(v) = read_value(ev, target) {
            found.push((id, ev.tick, v));
        }
    }
    let (Some(first), Some(last)) = (
        found.iter().map(|f| f.1).min(),
        found.iter().map(|f| f.1).max(),
    ) else {
        return Ok(());
    };
    let lo = found.iter().map(|f| f.2).min().unwrap_or(0);
    let hi = found.iter().map(|f| f.2).max().unwrap_or(0);
    for (id, tick, v) in found {
        let a = amount.at(tick, first, last, id);
        let next = match op {
            ChangeOp::Set => a,
            ChangeOp::Scale => narrow(i128::from(v) * i128::from(a) / 100),
            ChangeOp::Add => v.saturating_add(a),
            ChangeOp::Limit { min, max } => v.clamp(min, max),
            ChangeOp::Compress { low, high } => {
                if hi == lo {
                    low
                } else {
                    narrow(
                        i128::from(low)
                            + (i128::from(v) - i128::from(lo))
                                * (i128::from(high) - i128::from(low))
                                / (i128::from(hi) - i128::from(lo)),
                    )
                }
            }
            ChangeOp::Legato => v,
        };
        // A note that had a length keeps at least one tick (as a resize
        // does); only an explicit Set can make it zero.
        let next = if target == ValueTarget::Duration && v > 0 && op != ChangeOp::Set {
            next.max(1)
        } else {
            next
        };
        edit_events(d, &[id], r, |e| {
            write_value(e, target, next);
            Ok(())
        })?;
    }
    Ok(())
}

/// The series a controller event belongs to inside its lane and take.
fn series(e: &Event) -> Option<(u8, u8)> {
    match e.body {
        EventBody::Cc { index, .. } => Some((0, index)),
        EventBody::PitchBend { .. } => Some((1, 0)),
        EventBody::ChannelPressure { .. } => Some((2, 0)),
        EventBody::PolyPressure { pitch, .. } => Some((3, pitch)),
        _ => None,
    }
}

fn series_value(b: &EventBody) -> u32 {
    match b {
        EventBody::Cc { value, .. }
        | EventBody::PitchBend { value }
        | EventBody::ChannelPressure { value }
        | EventBody::PolyPressure { value, .. } => *value,
        _ => 0,
    }
}

fn with_value(b: &mut EventBody, v: u32) {
    if let EventBody::Cc { value, .. }
    | EventBody::PitchBend { value }
    | EventBody::ChannelPressure { value }
    | EventBody::PolyPressure { value, .. } = b
    {
        *value = v;
    }
}

fn density(
    d: &mut Document,
    ids: &[EventId],
    den: Density,
    r: &mut Report,
) -> Result<Vec<EventId>> {
    let spacing = match den {
        Density::Thin { spacing } | Density::Thicken { spacing } => spacing,
    };
    if spacing.0 <= 0 {
        return Err(ModelError::range("density.spacing", spacing.0));
    }
    let mut groups: BTreeMap<_, Vec<(Tick, EventId)>> = BTreeMap::new();
    let mut out = Vec::new();
    for id in ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
    {
        let (ev, loc) = d.event(id)?;
        match series(ev) {
            Some((a, b)) => groups
                .entry((loc.seq, loc.lane, loc.take, a, b))
                .or_default()
                .push((ev.tick, id)),
            None => out.push(id),
        }
    }
    let mut added = 0usize;
    for ((seq, lane, take, _, _), mut evs) in groups {
        evs.sort();
        match den {
            Density::Thin { .. } => {
                let mut last: Option<Tick> = None;
                for (t, id) in evs {
                    if last.is_some_and(|l| (t - l).0 < spacing.0) {
                        d.remove_event(id)?;
                        r.removed.push(AnyId::Event(id));
                    } else {
                        last = Some(t);
                        out.push(id);
                    }
                }
            }
            Density::Thicken { .. } => {
                for pair in evs.windows(2) {
                    let [(t0, a), (t1, b)] = pair else { continue };
                    let first = d.event(*a)?.0.clone();
                    let va = i128::from(series_value(&first.body));
                    let vb = i128::from(series_value(&d.event(*b)?.0.body));
                    let gap = i128::from((*t1 - *t0).0);
                    let mut t = t0.0.saturating_add(spacing.0);
                    while t < t1.0 {
                        added += 1;
                        if added > MAX_THICKEN {
                            return Err(ModelError::range("density.spacing", spacing.0));
                        }
                        let v = va + (vb - va) * i128::from(t - t0.0) / gap;
                        let mut spec = first.spec();
                        spec.tick = Tick(t);
                        with_value(&mut spec.body, u32::try_from(v).unwrap_or(0));
                        let id = d.counters_mut().alloc_event();
                        d.insert_event(seq, lane, Some(take), spec.with_id(id))?;
                        r.created.push(AnyId::Event(id));
                        out.push(id);
                        t = t.saturating_add(spacing.0);
                    }
                }
                out.extend(evs.iter().map(|(_, id)| *id));
            }
        }
    }
    Ok(out)
}

/// Refuses arguments that no event could carry, before anything runs, so a
/// selection without notes still rejects them.
fn validate(m: &Modify) -> Result<()> {
    match m {
        Modify::Transpose(Transpose::Diatonic { root, mask, .. })
            if *root > 11 || mask & 0x0fff == 0 =>
        {
            Err(ModelError::range(
                "transpose.scale",
                format!("{root}/{mask:#x}"),
            ))
        }
        Modify::Transpose(Transpose::Invert { axis }) if *axis > 127 => {
            Err(ModelError::range("transpose.axis", axis))
        }
        Modify::NoteMap { map } if map.iter().any(|(a, b)| *a > 127 || *b > 127) => {
            Err(ModelError::range("note_map", "a pitch above 127"))
        }
        Modify::Substitute(Substitute::Controller { index }) if *index > 127 => {
            Err(ModelError::range("substitute.index", index))
        }
        Modify::Substitute(Substitute::Pitch { pitch }) if *pitch > 127 => {
            Err(ModelError::range("substitute.pitch", pitch))
        }
        _ => Ok(()),
    }
}

/// Applies `m` to the events `ids`; returns the events the selection holds
/// afterwards.
pub(crate) fn apply_modify(
    d: &mut Document,
    ids: &[EventId],
    m: &Modify,
    r: &mut Report,
) -> Result<Vec<EventId>> {
    validate(m)?;
    match m {
        Modify::Legato => legato(d, ids, r)?,
        Modify::Density(den) => return density(d, ids, *den, r),
        Modify::Delete => {
            for id in ids
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
            {
                d.remove_event(id)?;
                r.removed.push(AnyId::Event(id));
            }
            return Ok(Vec::new());
        }
        Modify::Change { target, op, amount } => apply_change(d, ids, *target, *op, amount, r)?,
        _ => edit_events(d, ids, r, |e| modify_one(e, m))?,
    }
    Ok(ids.to_vec())
}
