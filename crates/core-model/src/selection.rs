//! The document-wide selection and view filters.

use crate::document::Document;
use crate::event::EventKind;
use crate::ids::{EventId, LaneId, SeqId};
use crate::time::Tick;
use im::OrdSet;

/// The single selection of an open document: an edit-in/edit-out range,
/// selected lanes and selected events. Changing it is not an undo step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// `[start, end)` time range.
    pub time_range: Option<(Tick, Tick)>,
    /// Selected lanes.
    pub lanes: OrdSet<(SeqId, LaneId)>,
    /// Selected events.
    pub events: OrdSet<EventId>,
}

impl Selection {
    /// Drops lanes and events that no longer exist in `doc` (after undo,
    /// delete and so on).
    pub fn retain_existing(&self, doc: &Document) -> Selection {
        Selection {
            time_range: self.time_range,
            lanes: self
                .lanes
                .iter()
                .filter(|(s, l)| doc.lane(*s, *l).is_ok())
                .copied()
                .collect(),
            events: self
                .events
                .iter()
                .filter(|e| doc.locate(**e).is_ok())
                .copied()
                .collect(),
        }
    }
}

/// What a view hides. Hidden events can't be selected or changed through
/// that view's commands or queries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewFilter {
    /// Event kinds the view hides.
    pub hidden_kinds: OrdSet<EventKind>,
    /// Lanes the view hides.
    pub hidden_lanes: OrdSet<LaneId>,
}

impl ViewFilter {
    /// Whether an event of `kind` on `lane` is visible.
    pub fn allows(&self, lane: LaneId, kind: EventKind) -> bool {
        !self.hidden_lanes.contains(&lane) && !self.hidden_kinds.contains(&kind)
    }
}
