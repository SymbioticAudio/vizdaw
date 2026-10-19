//! Composable event predicates for select-and-modify.

use crate::automation::AutomationTarget;
use crate::document::Document;
use crate::event::{Event, EventBody, EventKind};
use crate::ids::{DestId, EventId, LaneId, SeqId};
use crate::query::routing::resolve_with;
use crate::selection::ViewFilter;
use crate::time::Tick;
use im::OrdSet;
use serde::{Deserialize, Serialize};

/// A predicate over events. Ranges are inclusive except `TimeRange`
/// (`[start, end)`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    /// Tick in `[start, end)`.
    TimeRange {
        /// Start.
        start: Tick,
        /// End (exclusive).
        end: Tick,
    },
    /// Kind is one of these.
    Kind {
        /// Accepted kinds.
        kinds: Vec<EventKind>,
    },
    /// Note or poly-pressure pitch in range.
    PitchRange {
        /// Lowest.
        low: u8,
        /// Highest.
        high: u8,
    },
    /// Note velocity in range.
    VelocityRange {
        /// Lowest.
        low: u16,
        /// Highest.
        high: u16,
    },
    /// Note duration in range.
    DurationRange {
        /// Shortest.
        min: Tick,
        /// Longest.
        max: Tick,
    },
    /// Resolved destination equals `dest` (None = unresolved).
    Dest {
        /// Destination.
        dest: Option<DestId>,
    },
    /// `(tick - offset) mod grid` within `tolerance` of a grid line.
    MetricPosition {
        /// Grid (> 0).
        grid: Tick,
        /// Grid offset.
        offset: Tick,
        /// Allowed distance from the line.
        tolerance: Tick,
    },
    /// Every `n`th candidate (in `(tick, id)` order), starting at `phase`.
    EveryNth {
        /// Period (>= 1).
        n: u32,
        /// Index of the first match.
        phase: u32,
    },
    /// All sub-rules match.
    All(Vec<Rule>),
    /// Any sub-rule matches.
    Any(Vec<Rule>),
    /// The sub-rule does not match.
    Not(Box<Rule>),
    /// Note release velocity in range.
    ReleaseVelocityRange {
        /// Lowest.
        low: u16,
        /// Highest.
        high: u16,
    },
    /// CC, pitch-bend and pressure value, or program number, in range.
    ValueRange {
        /// Lowest.
        low: u32,
        /// Highest.
        high: u32,
    },
    /// Control-change controller number in range.
    ControllerRange {
        /// Lowest.
        low: u8,
        /// Highest.
        high: u8,
    },
    /// Starts inside a note of the same lane and take (`note.tick <= tick <
    /// note.tick + duration`, the event itself excluded) that matches `by`.
    Bracketed {
        /// What the bracketing note must match.
        by: Box<Rule>,
    },
    /// An automation event driving `target`.
    Automation {
        /// The target.
        target: AutomationTarget,
    },
}

/// How a rule combines with the current selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectMode {
    /// Replace the selected events with the matches in the selected lanes.
    Select,
    /// Add the matches in the selected lanes.
    Add,
    /// Keep only the selected events that match.
    Refine,
}

/// Whether `ev` (on a lane with destination `lane_dest`, at position
/// `index` among the candidates) matches `rule`.
pub fn matches(
    doc: &Document,
    rule: &Rule,
    lane_dest: Option<DestId>,
    ev: &Event,
    index: usize,
) -> bool {
    match rule {
        Rule::TimeRange { start, end } => ev.tick >= *start && ev.tick < *end,
        Rule::Kind { kinds } => kinds.contains(&ev.body.kind()),
        Rule::PitchRange { low, high } => match ev.body {
            EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                (*low..=*high).contains(&pitch)
            }
            _ => false,
        },
        Rule::VelocityRange { low, high } => {
            matches!(ev.body, EventBody::Note { velocity, .. } if (*low..=*high).contains(&velocity))
        }
        Rule::DurationRange { min, max } => {
            matches!(ev.body, EventBody::Note { duration, .. } if duration >= *min && duration <= *max)
        }
        Rule::Dest { dest } => resolve_with(doc, lane_dest, ev).dest == *dest,
        Rule::MetricPosition {
            grid,
            offset,
            tolerance,
        } => {
            if grid.0 <= 0 {
                return false;
            }
            let m = (ev.tick.0.saturating_sub(offset.0)).rem_euclid(grid.0);
            m <= tolerance.0 || grid.0 - m <= tolerance.0
        }
        Rule::EveryNth { n, phase } => {
            let n = usize::try_from((*n).max(1)).unwrap_or(1);
            let phase = usize::try_from(*phase).unwrap_or(0);
            index >= phase && (index - phase) % n == 0
        }
        Rule::All(rs) => rs.iter().all(|r| matches(doc, r, lane_dest, ev, index)),
        Rule::Any(rs) => rs.iter().any(|r| matches(doc, r, lane_dest, ev, index)),
        Rule::Not(r) => !matches(doc, r, lane_dest, ev, index),
        Rule::ReleaseVelocityRange { low, high } => matches!(
            ev.body,
            EventBody::Note { release_velocity, .. } if (*low..=*high).contains(&release_velocity)
        ),
        Rule::ValueRange { low, high } => match ev.body {
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. } => (*low..=*high).contains(&value),
            EventBody::Program { program, .. } => (*low..=*high).contains(&u32::from(program)),
            _ => false,
        },
        Rule::ControllerRange { low, high } => {
            matches!(ev.body, EventBody::Cc { index, .. } if (*low..=*high).contains(&index))
        }
        Rule::Bracketed { by } => bracketed(doc, by, ev),
        Rule::Automation { target } => {
            matches!(ev.body, EventBody::Automation { target: t, .. } if t == *target)
        }
    }
}

/// Whether a note of `ev`'s own lane and take that matches `by` sounds at
/// `ev`'s start.
fn bracketed(doc: &Document, by: &Rule, ev: &Event) -> bool {
    let Ok(loc) = doc.locate(ev.id) else {
        return false;
    };
    let Ok(lane) = doc.lane(loc.seq, loc.lane) else {
        return false;
    };
    let Some(take) = lane.take(loc.take) else {
        return false;
    };
    take.events
        .range(..=(ev.tick, EventId(u64::MAX)))
        .any(|(_, b)| {
            b.id != ev.id
                && matches!(b.body, EventBody::Note { duration, .. } if ev.tick < b.tick + duration)
                && matches(doc, by, lane.midi_dest, b, 0)
        })
}

/// Filters `candidates` (ordered by `(tick, id)` first) to those visible
/// through `filter` and matching `rule`.
pub fn filter_events(
    doc: &Document,
    candidates: impl IntoIterator<Item = EventId>,
    rule: &Rule,
    filter: &ViewFilter,
) -> Vec<EventId> {
    let mut cands: Vec<(Tick, EventId, Option<DestId>, Event)> = candidates
        .into_iter()
        .filter_map(|id| {
            let (ev, loc) = doc.event(id).ok()?;
            let lane = doc.lane(loc.seq, loc.lane).ok()?;
            filter
                .allows(loc.lane, ev.body.kind())
                .then(|| (ev.tick, id, lane.midi_dest, ev.clone()))
        })
        .collect();
    cands.sort_by_key(|c| (c.0, c.1));
    cands
        .iter()
        .enumerate()
        .filter(|(i, c)| matches(doc, rule, c.2, &c.3, *i))
        .map(|(_, c)| c.1)
        .collect()
}

/// Active-take events of `lanes` that are visible and match `rule`.
pub fn events_matching(
    doc: &Document,
    lanes: &OrdSet<(SeqId, LaneId)>,
    rule: &Rule,
    filter: &ViewFilter,
) -> Vec<EventId> {
    let mut ids = Vec::new();
    for (s, l) in lanes {
        if let Ok(lane) = doc.lane(*s, *l) {
            if let Some(t) = lane.active() {
                ids.extend(t.events.values().map(|e| e.id));
            }
        }
    }
    filter_events(doc, ids, rule, filter)
}
