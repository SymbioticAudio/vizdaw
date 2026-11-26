//! The right pane's Markers view (slice 8c Task 6; Swift `RightPaneTests`
//! `markersListTheSequencesMarkersAndSetLocators` and
//! `choosingAMarkerMovesThePlayhead`): the rows, their order and
//! positions, the document's rows, and the move with its failure.

mod support;

use std::sync::Arc;

use support::{demo_fake, four_four};
use vizdaw_app::DocumentModel;
use vizdaw_app::editor::TransportMarks;
use vizdaw_app::marker_list::{EMPTY, MarkerList, MarkerListRow};
use vizdaw_ffi::VizError;
use vizdaw_ffi::events::{MarkerRow, SequenceInfo};

const BAR: i64 = 61_440;

fn info(markers: &[(i64, &str)]) -> SequenceInfo {
    let mut i = four_four(3);
    i.markers = markers
        .iter()
        .map(|&(tick, name)| MarkerRow {
            tick,
            name: name.into(),
        })
        .collect();
    i
}

fn names(rows: &[MarkerListRow]) -> Vec<&str> {
    rows.iter().map(|r| r.name.as_str()).collect()
}

#[test]
fn markers_list_the_sequences_markers_and_set_locators() {
    let mut marks = TransportMarks::default();
    marks.set_locator(2, Some(1920));
    marks.set_locator(0, Some(0));
    let rows = MarkerList::rows(Some(&info(&[(1920, "Chorus"), (0, "Intro")])), &marks);
    assert_eq!(names(&rows), ["Intro", "Locator 0", "Chorus", "Locator 2"]);
    assert_eq!(rows[0].position, "1.1.0");
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["m1", "l0", "m0", "l2"]);
}

#[test]
fn a_marker_comes_before_a_locator_at_its_tick_then_their_order() {
    let mut marks = TransportMarks::default();
    marks.set_locator(5, Some(2 * BAR));
    marks.set_locator(3, Some(2 * BAR));
    let rows = MarkerList::rows(
        Some(&info(&[(2 * BAR, "B"), (2 * BAR, "A"), (BAR, "Early")])),
        &marks,
    );
    assert_eq!(names(&rows), ["Early", "B", "A", "Locator 3", "Locator 5"]);
    assert_eq!(rows[0].position, "2.1.0");
    assert_eq!(rows[1].position, "3.1.0");
}

#[test]
fn positions_follow_the_sequences_meter() {
    // 3/4: a bar is three beats of 15360 ticks; a unit is 16 ticks.
    let mut i = info(&[(3 * 15_360 + 15_360 + 120 * 16, "Late")]);
    i.meter[0].num = 3;
    let rows = MarkerList::rows(Some(&i), &TransportMarks::default());
    assert_eq!(rows[0].position, "2.2.120");
}

#[test]
fn no_sequence_lists_only_the_locators() {
    let mut marks = TransportMarks::default();
    marks.set_locator(9, Some(BAR));
    let rows = MarkerList::rows(None, &marks);
    assert_eq!(names(&rows), ["Locator 9"]);
    assert_eq!(rows[0].id, "l9");
    assert_eq!(rows[0].position, "2.1.0");
    assert!(MarkerList::rows(None, &TransportMarks::default()).is_empty());
    assert_eq!(EMPTY, "No markers or locators.");
}

#[test]
fn the_document_lists_its_sequences_markers_and_locators() {
    let f = Arc::new(demo_fake());
    let seq = {
        let d = DocumentModel::new(f.clone()).unwrap();
        d.selected_seq().unwrap()
    };
    let mut i = info(&[(BAR, "Verse")]);
    i.id = seq;
    f.set_info(seq, i);
    let mut d = DocumentModel::new(f.clone()).unwrap();
    assert_eq!(names(&MarkerList::of(&d)), ["Verse"]);
    d.update_layout(|l| {
        l.transport.set_locator(1, Some(0));
    });
    assert_eq!(names(&MarkerList::of(&d)), ["Locator 1", "Verse"]);
}

fn row(tick: i64) -> MarkerListRow {
    MarkerListRow {
        id: "m0".into(),
        name: "Chorus".into(),
        tick,
        position: "1.2.0".into(),
    }
}

#[test]
fn choosing_a_marker_moves_the_playhead() {
    let f = Arc::new(demo_fake());
    let mut d = DocumentModel::new(f.clone()).unwrap();
    MarkerList::go(&row(1920), &mut d);
    assert_eq!(f.calls().last().map(String::as_str), Some("locate(1920)"));
    assert_eq!(d.status(), None);
}

#[test]
fn a_failed_move_shows_the_status() {
    let f = Arc::new(demo_fake());
    let mut d = DocumentModel::new(f.clone()).unwrap();
    f.set_locate_error(Some(VizError::Engine {
        message: "The engine stopped.".into(),
    }));
    MarkerList::go(&row(1920), &mut d);
    assert_eq!(d.status(), Some("Go to Marker Failed: The engine stopped."));
}
