//! Transport keys' logic (plan 4f): what Play Selection plays, the marker
//! before or after a position, and where record arm moves.

use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::query::event_end;
use core_model::selection::Selection;
use core_model::sequence::Sequence;
use core_model::time::Tick;

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
