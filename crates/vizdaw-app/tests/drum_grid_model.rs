//! What the Drum Grid shows (Swift `T/DrumGridModelTests.swift`; slice
//! 10d Task 2): the drum tracks of the shown sequence, read once per
//! snapshot; the layout; the selected row; and opening a track, which
//! marks it a drum track once.

mod support;

use std::sync::Arc;

use support::{FakeSession, drum_fake, edit_calls};
use vizdaw_app::DocumentModel;
use vizdaw_app::drum_grid::{DrumRowKey, drum_grid_offers};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget};
use vizdaw_ffi::VizError;
use vizdaw_ffi::drum::{DrumEdit, DrumSpotRow};
use vizdaw_ffi::edit::{EditCommand, LaneRef};
use vizdaw_ffi::selection::TickRange;

const DRUM: Option<DetailEditorKind> = Some(DetailEditorKind::DrumGrid);

fn busy() -> VizError {
    VizError::Busy {
        message: "busy".into(),
    }
}

fn gone() -> VizError {
    VizError::Model {
        message: "gone".into(),
    }
}

fn lane(seq: u64, lane: u64) -> EditorTarget {
    EditorTarget::Lane { seq, lane }
}

fn mark(lane: u64, drum: bool) -> EditCommand {
    EditCommand::Drum {
        edit: DrumEdit::MarkLane { seq: 3, lane, drum },
    }
}

/// `drum_fake` with the pane's Drum Grid on Drums (40), calls cleared.
fn make() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(drum_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(lane(3, 40), DRUM);
    f.clear_calls();
    (f, m)
}

/// An edit that bumps the snapshot version (Swift's `nextSnapshot` and
/// `markLane`).
fn bump(m: &mut DocumentModel, drum: bool) {
    assert!(m.mark_drum_lane(3, 30, drum));
}

#[test]
fn opening_a_midi_lane_gives_it_a_default_layout() {
    let f = Arc::new(drum_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(lane(3, 30), DRUM);
    assert_eq!(f.edits(), [mark(30, true)]);
    assert_eq!(m.layout().detail.as_ref().unwrap().target, lane(3, 30));
    assert!(m.drum_showing());
    m.open_detail(lane(3, 30), DRUM);
    assert_eq!(
        f.edits(),
        [mark(30, true)],
        "opening it again sends nothing"
    );
}

#[test]
fn a_lane_with_a_layout_keeps_it_and_an_audio_lane_is_not_offered() {
    let f = Arc::new(drum_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(lane(3, 40), DRUM);
    m.open_detail(
        EditorTarget::Block {
            seq: 3,
            lane: 41,
            start: 0,
            end: 100,
        },
        DRUM,
    );
    m.open_detail(lane(3, 42), DRUM);
    m.open_detail(EditorTarget::Sequence { seq: 3 }, DRUM);
    assert!(f.edits().is_empty());
    let tracks = m.drum_tracks();
    assert_eq!(
        tracks.iter().map(|l| l.id).collect::<Vec<_>>(),
        [30, 40, 41]
    );
    assert_eq!(
        tracks.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
        ["Groove", "Drums", "Perc"]
    );
    assert!(!drum_grid_offers(&m.lane_rows(3)[3]), "Vocals is audio");
    // Another editor kind marks nothing.
    m.open_detail(lane(3, 30), Some(DetailEditorKind::List));
    assert!(f.edits().is_empty());
    // A track the sequence does not have marks nothing.
    m.open_detail(lane(3, 99), DRUM);
    assert!(f.edits().is_empty());
}

#[test]
fn a_busy_track_read_does_not_mark_the_track_twice() {
    let f = Arc::new(drum_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    // Every tracks read after the mark is Busy, for a while: the older
    // rows say Groove is not a drum track, but they are not this
    // version's.
    for _ in 0..4 {
        f.fail_next("lanes", busy());
    }
    m.open_detail(lane(3, 30), DRUM);
    assert_eq!(f.edits(), [mark(30, true)]);
    m.open_detail(lane(3, 30), DRUM);
    assert_eq!(f.edits(), [mark(30, true)], "not marked twice");
}

#[test]
fn the_drum_grid_opens_a_track_of_the_shown_sequence() {
    let (f, mut m) = make();
    m.drum_open_lane(30);
    assert_eq!(f.edits(), [mark(30, true)]);
    assert_eq!(m.layout().detail.as_ref().unwrap().target, lane(3, 30));
    assert_eq!(
        m.layout().detail.as_ref().unwrap().kind,
        DetailEditorKind::DrumGrid
    );
    assert_eq!(m.drum_seq(), Some(3));
}

#[test]
fn a_block_target_scrolls_to_its_range() {
    let (_f, mut m) = make();
    assert_eq!(m.drum_focus_range(), None);
    m.open_detail(
        EditorTarget::Block {
            seq: 3,
            lane: 41,
            start: 50,
            end: 50,
        },
        DRUM,
    );
    assert_eq!(m.drum_focus_range(), Some(TickRange { start: 50, end: 51 }));
}

#[test]
fn shows_the_drum_lanes_of_the_shown_sequence() {
    let (f, m) = make();
    let names: Vec<String> = m.drum_lanes().iter().map(|l| l.name.clone()).collect();
    assert_eq!(names, ["Drums", "Perc"]);
    // Header, Kick, Snare, header, Clap, the new-lane band.
    assert_eq!(m.drum_layout().bands.len(), 6);
    assert_eq!(
        m.drum_spot(30.),
        DrumSpotRow::Row {
            lane: 0,
            row: 0,
            velocity: None
        }
    );
    assert_eq!(m.drum_spot(10.), DrumSpotRow::Header { lane: 0 });
    assert_eq!(m.drum_spot(1000.), DrumSpotRow::NewLane);
    assert_eq!(m.drum_current_lane().map(|l| l.lane), Some(40));
    let _ = m.drum_lanes();
    let _ = m.drum_layout();
    assert_eq!(f.drum_lanes_reads(), 1, "read once per snapshot");
    assert_eq!(m.drum_row(0, 1).map(|r| r.name), Some("Snare".into()));
    assert_eq!(m.drum_row(2, 0), None);
}

#[test]
fn busy_keeps_the_lanes_and_a_failure_shows_once() {
    let (f, mut m) = make();
    assert_eq!(m.drum_lanes().len(), 2);
    f.set_call_error("drum_lanes", Some(busy()));
    bump(&mut m, true);
    assert_eq!(m.drum_lanes().len(), 2, "Busy keeps what was read");
    f.set_call_error("drum_lanes", Some(gone()));
    bump(&mut m, false);
    assert!(m.drum_lanes().is_empty());
    let reads = f.drum_lanes_reads();
    let _ = m.drum_lanes();
    assert_eq!(
        f.drum_lanes_reads(),
        reads,
        "not retried at the same version"
    );
    assert_eq!(m.status(), None, "a read does not set the status");
    m.idle();
    assert_eq!(m.status(), Some("gone"));
    m.set_status(None);
    m.idle();
    assert_eq!(m.status(), None, "it shows once");
}

#[test]
fn a_busy_read_then_a_fresh_one_does_not_trap() {
    let (f, mut m) = make();
    let two = m.drum_lanes();
    assert_eq!(two.len(), 2);
    bump(&mut m, true);
    // The first read is Busy (the two old tracks); every later one has one.
    f.fail_next("drum_lanes", busy());
    f.set_drum_lanes(3, vec![two[0].clone()]);
    m.drum_select(1, 0);
    assert_eq!(m.drum_selected_row(), Some(DrumRowKey { lane: 41, row: 3 }));
    // Fresh data has no track 1 any more: refused, not a trap.
    assert_eq!(m.drum_row(1, 0), None);
    assert!(m.drum_selected().is_none());
    assert_eq!(m.drum_current_lane().map(|l| l.lane), Some(40));
}

#[test]
fn a_failure_forgets_the_rows_read_before_it() {
    let (f, mut m) = make();
    assert_eq!(m.drum_lanes().len(), 2);
    f.set_call_error("drum_lanes", Some(gone()));
    bump(&mut m, true);
    assert!(m.drum_lanes().is_empty());
    f.set_call_error("drum_lanes", Some(busy()));
    bump(&mut m, false);
    assert!(
        m.drum_lanes().is_empty(),
        "Busy after a failure must not bring the old rows back"
    );
}

#[test]
fn selecting_a_row_and_out_of_range_indexes() {
    let (_f, mut m) = make();
    assert_eq!(m.drum_selected_row(), None);
    m.drum_select(0, 1);
    let (l, r) = m.drum_selected().unwrap();
    assert_eq!((l.lane, r.name.as_str()), (40, "Snare"));
    m.drum_select(5, 0);
    m.drum_select(0, 9);
    assert_eq!(m.drum_selected_row(), Some(DrumRowKey { lane: 40, row: 2 }));
    m.drum_select(1, 0);
    assert_eq!(m.drum_current_lane().map(|l| l.lane), Some(41));
}

#[test]
fn leaving_the_drum_grid_clears_the_selected_row() {
    let (_f, mut m) = make();
    m.drum_select(0, 1);
    assert_eq!(m.drum_selected_row(), Some(DrumRowKey { lane: 40, row: 2 }));
    m.show_detail(DetailEditorKind::List);
    assert_eq!(m.drum_selected_row(), None);
    assert!(!m.drum_showing());
    assert!(m.drum_lanes().is_empty(), "no grid shown: no tracks");
}

#[test]
fn the_settings_and_a_ruler_range() {
    let (f, mut m) = make();
    let step = m.drum_settings().step;
    m.update_drum_settings(|s| s.step = step * 2);
    assert_eq!(m.drum_settings().step, step * 2);
    assert_eq!(m.layout().drum_grid.step, step * 2);
    m.drum_select_range(TickRange { start: 0, end: 960 });
    let sel = m.selection().clone();
    assert_eq!(sel.time_range, Some(TickRange { start: 0, end: 960 }));
    assert_eq!(
        sel.lanes,
        [LaneRef { seq: 3, lane: 40 }, LaneRef { seq: 3, lane: 41 }]
    );
    assert!(sel.events.is_empty());
    assert!(edit_calls(&f).is_empty(), "no edit");
}
