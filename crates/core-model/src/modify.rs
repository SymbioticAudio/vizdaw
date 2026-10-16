//! Modifications applied by select-and-modify.

use crate::command::Report;
use crate::command::event::{edit_events, shift_pitch};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{Event, EventBody};
use crate::ids::{DestId, EventId};
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
}

/// Pitch changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Transpose {
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
                }
            }
            _ => {}
        },
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
                *duration = Tick(scale(duration.0, *num, *den));
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

/// Applies `m` to the events `ids`.
pub(crate) fn apply_modify(
    d: &mut Document,
    ids: &[EventId],
    m: &Modify,
    r: &mut Report,
) -> Result<()> {
    if let Modify::Legato = m {
        // Group notes by lane, then extend each to the next start.
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
        return Ok(());
    }
    edit_events(d, ids, r, |e| modify_one(e, m))
}
