//! Transport keys' logic (plan 4f): what Play Selection plays, the marker
//! before or after a position, and where record arm moves; and (plan 7d)
//! the transport bar's bar steps and winds.

use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::meter::MeterMap;
use core_model::query::event_end;
use core_model::selection::Selection;
use core_model::sequence::Sequence;
use core_model::time::{PPQ, Tick};

/// What Play Selection plays in sequence `seq` (plan 4f Ruling 9): the
/// selection's range when it has one, else from the earliest selected
/// event of `seq` to the latest end among them (an event ends at least a
/// tick after it starts). None when nothing of `seq` is selected.
pub fn selection_span(doc: &Document, sel: &Selection, seq: SeqId) -> Option<(Tick, Tick)> {
    if let Some((a, b)) = sel.time_range.filter(|(a, b)| b > a) {
        return Some((a, b));
    }
    let mut span: Option<(Tick, Tick)> = None;
    for id in &sel.events {
        let Ok((e, loc)) = doc.event(*id) else {
            continue;
        };
        if loc.seq != seq {
            continue;
        }
        let end = event_end(doc, seq, e)
            .unwrap_or(e.tick)
            .max(e.tick + Tick(1));
        span = Some(match span {
            Some((a, b)) => (a.min(e.tick), b.max(end)),
            None => (e.tick, end),
        });
    }
    span
}

/// The nearest marker of `seq` after `tick` (`forward`) or before it.
pub fn marker_step(seq: &Sequence, tick: Tick, forward: bool) -> Option<Tick> {
    let ticks = seq.markers.iter().map(|m| m.tick);
    if forward {
        ticks.filter(|t| *t > tick).min()
    } else {
        ticks.filter(|t| *t < tick).max()
    }
}

/// A record-arm move: the lanes (by index) to disarm and the one to arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmMove {
    /// Armed lanes that lose their arm.
    pub disarm: Vec<usize>,
    /// The lane armed instead.
    pub arm: usize,
}

/// Moves record arm to the next (`forward`) or previous lane (plan 4f
/// Ruling 10): from the last armed lane forward or the first backward;
/// with none armed, the first or last lane. Every other armed lane is
/// disarmed, so one lane is armed after. None at either end (or without
/// lanes).
pub fn arm_move(armed: &[bool], forward: bool) -> Option<ArmMove> {
    let n = armed.len();
    let arm = if forward {
        match armed.iter().rposition(|a| *a) {
            Some(l) => l.checked_add(1).filter(|i| *i < n)?,
            None if n > 0 => 0,
            None => return None,
        }
    } else {
        match armed.iter().position(|a| *a) {
            Some(f) => f.checked_sub(1)?,
            None => n.checked_sub(1)?,
        }
    };
    let disarm = armed
        .iter()
        .enumerate()
        .filter(|(_, a)| **a)
        .map(|(i, _)| i)
        .collect();
    Some(ArmMove { disarm, arm })
}

/// The furthest a step or wind goes (2⁵⁰ ticks, the app's time axis
/// limit; plan 7d Ruling 3).
pub const WIND_LIMIT: i64 = 1 << 50;

/// One bar from `tick` (plan 7d Ruling 3): forward to the first barline
/// after it, back to the last barline before it (0 at or before 0), the
/// barlines following the meter in effect. Always in `0..=WIND_LIMIT`.
pub fn bar_step(meter: &MeterMap, tick: Tick, forward: bool) -> Tick {
    let t = tick.0.min(WIND_LIMIT);
    let to = if forward {
        if t >= WIND_LIMIT {
            return Tick(WIND_LIMIT);
        }
        meter.barline_at_or_after(Tick(t.saturating_add(1))).0
    } else if t <= 0 {
        0
    } else {
        meter.barline_at_or_before(Tick(t.saturating_sub(1))).0
    };
    Tick(to.clamp(0, WIND_LIMIT))
}

/// Where a wind of `quarters` quarter notes from `base` goes (plan 7d
/// Ruling 5): `base + round(quarters × PPQ)`, saturating, in
/// `0..=WIND_LIMIT`. None when `quarters` is not finite.
pub fn wind_target(base: i64, quarters: f64) -> Option<i64> {
    if !quarters.is_finite() {
        return None;
    }
    // `as` saturates.
    let by = (quarters * PPQ as f64).round() as i64;
    Some(base.saturating_add(by).clamp(0, WIND_LIMIT))
}
