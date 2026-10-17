//! The document-wide selection as the app sees it (spec §3.3).

use crate::edit::LaneRef;
use core_model::ids::{EventId, LaneId, SeqId};
use core_model::selection::Selection;
use core_model::time::Tick;

/// A `[start, end)` tick range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct TickRange {
    /// First tick.
    pub start: i64,
    /// End tick (exclusive).
    pub end: i64,
}

/// The selection: an edit range, lanes and events.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct SelectionRow {
    /// The edit-in/edit-out range.
    pub time_range: Option<TickRange>,
    /// Selected lanes.
    pub lanes: Vec<LaneRef>,
    /// Selected events.
    pub events: Vec<u64>,
}

/// The selection as a row.
pub fn row(s: &Selection) -> SelectionRow {
    SelectionRow {
        time_range: s.time_range.map(|(a, b)| TickRange {
            start: a.0,
            end: b.0,
        }),
        lanes: s
            .lanes
            .iter()
            .map(|(seq, lane)| LaneRef {
                seq: seq.0,
                lane: lane.0,
            })
            .collect(),
        events: s.events.iter().map(|e| e.0).collect(),
    }
}

/// A row as a selection; a range whose end is not after its start is
/// dropped. Unknown IDs are kept here (the caller drops them against the
/// document).
pub fn selection(r: &SelectionRow) -> Selection {
    Selection {
        time_range: r
            .time_range
            .filter(|t| t.end > t.start)
            .map(|t| (Tick(t.start), Tick(t.end))),
        lanes: r
            .lanes
            .iter()
            .map(|l| (SeqId(l.seq), LaneId(l.lane)))
            .collect(),
        events: r.events.iter().copied().map(EventId).collect(),
    }
}
