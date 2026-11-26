//! The right pane's Markers view (Swift `MarkerList` and `MarkerListRow`,
//! `M/PaneContent.swift:191-229`): the sequence's markers and the set
//! locators, in time order; a row moves the transport there.

use vizdaw_ffi::events::SequenceInfo;

use crate::alerts::error_message;
use crate::document::DocumentModel;
use crate::editor::TransportMarks;
use crate::position::Bbu;

/// What the view says with nothing to list (`V/PaneViews.swift:201`).
pub const EMPTY: &str = "No markers or locators.";

/// One row: a marker (`m<i>`, its name) or a set locator (`l<N>`,
/// `Locator N`), at `tick`, shown at `position` (bar.beat.unit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerListRow {
    pub id: String,
    pub name: String,
    pub tick: i64,
    pub position: String,
}

/// The rows and the move.
pub enum MarkerList {}

impl MarkerList {
    /// The sequence's markers and the set locators, by tick, then marker
    /// before locator, then their order.
    pub fn rows(info: Option<&SequenceInfo>, marks: &TransportMarks) -> Vec<MarkerListRow> {
        let meter = info.map_or(&[][..], |i| &i.meter[..]);
        let position = |t: i64| {
            let b = Bbu::of(t, meter);
            format!("{}.{}.{}", b.bar, b.beat, b.unit)
        };
        let markers = info.into_iter().flat_map(|i| &i.markers).enumerate();
        let mut items: Vec<(MarkerListRow, u8)> = markers
            .map(|(i, m)| {
                let row = MarkerListRow {
                    id: format!("m{i}"),
                    name: m.name.clone(),
                    tick: m.tick,
                    position: position(m.tick),
                };
                (row, 0)
            })
            .collect();
        for n in 0..TransportMarks::LOCATOR_COUNT {
            let Some(t) = marks.locator(n) else {
                continue;
            };
            let row = MarkerListRow {
                id: format!("l{n}"),
                name: format!("Locator {n}"),
                tick: t,
                position: position(t),
            };
            items.push((row, 1));
        }
        // Stable: rows at one tick and rank keep their order.
        items.sort_by_key(|(row, rank)| (row.tick, *rank));
        items.into_iter().map(|(row, _)| row).collect()
    }

    /// The rows of the document's sequence.
    pub fn of(doc: &DocumentModel) -> Vec<MarkerListRow> {
        let info = doc.selected_seq().and_then(|q| doc.sequence_info(q));
        Self::rows(info, &doc.layout().transport)
    }

    /// Moves the transport to `row`; a failure is the status line's.
    pub fn go(row: &MarkerListRow, doc: &mut DocumentModel) {
        if let Err(e) = doc.move_to(row.tick) {
            doc.set_status(Some(format!("Go to Marker Failed: {}", error_message(&e))));
        }
    }
}
