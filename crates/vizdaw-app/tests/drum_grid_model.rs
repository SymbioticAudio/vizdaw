//! What the Drum Grid shows (Swift `T/DrumGridModelTests.swift`; slice
//! 10d Task 2): the drum tracks of the shown sequence, read once per
//! snapshot; the layout; the selected row; and opening a track, which
//! marks it a drum track once.

mod support;

use std::sync::Arc;

use support::{FakeSession, drum_fake, edit_calls};
use vizdaw_app::DocumentModel;
use vizdaw_app::drum_grid::{DrumRowKey, drum_grid_offers};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, GESTURE_OPEN};
use vizdaw_ffi::VizError;
use vizdaw_ffi::drum::{
    DrumEdit, DrumGridSettingsRow, DrumHitRow, DrumPaintHitRow, DrumSpotRow, DrumStrokeRow,
};
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
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

// MARK: Strokes (Task 3, `T/DrumGridModelTests.swift:107-247`)

const Q: i64 = 15_360;
const S16: i64 = Q / 4;

fn paint(row: u64, hits: Vec<DrumPaintHitRow>) -> EditCommand {
    EditCommand::Drum {
        edit: DrumEdit::Paint {
            seq: 3,
            lane: 40,
            row,
            hits,
            window: S16,
            duration: Q / 8,
        },
    }
}

fn hit(tick: i64, velocity: u16) -> DrumPaintHitRow {
    DrumPaintHitRow {
        tick,
        velocity: velocity << 9,
    }
}

#[test]
fn a_click_on_an_empty_cell_paints_one_hit() {
    let (f, mut m) = make();
    m.drum_stroke_began(0, 1, 2 * S16 + 5, 2 * S16, 1, None, false);
    m.drum_stroke_ended(2 * S16 + 5, 1, None);
    assert_eq!(
        edit_calls(&f),
        ["begin_edit(Paint Drum Hits)", "edit_gesture", "end_edit"]
    );
    assert_eq!(
        f.gesture_edits().last(),
        Some(&paint(2, vec![hit(2 * S16, 100)]))
    );
    assert_eq!(m.drum_selected_row(), Some(DrumRowKey { lane: 40, row: 2 }));
    assert!(m.drum_stroke().is_none());
}

#[test]
fn a_click_on_a_hit_erases_it() {
    let (f, mut m) = make();
    m.drum_stroke_began(0, 0, Q + 9, Q, 1, None, false);
    m.drum_stroke_ended(Q + 9, 1, None);
    assert_eq!(
        edit_calls(&f).first().map(String::as_str),
        Some("begin_edit(Erase Drum Hits)")
    );
    assert_eq!(
        f.gesture_edits().last(),
        Some(&EditCommand::Event {
            edit: EventEdit::Delete { events: vec![12] }
        })
    );
}

#[test]
fn a_drag_paints_a_run_and_the_velocity_lane_sets_each_hit() {
    let (f, mut m) = make();
    m.update_drum_settings(|s| s.spacing = Q / 2);
    m.drum_stroke_began(0, 1, 0, 0, 1, Some(20), false);
    m.drum_stroke_moved(2 * Q + 1, 1, Some(90));
    let hits = vec![
        hit(0, 20),
        hit(Q / 2, 90),
        hit(Q, 90),
        hit(3 * Q / 2, 90),
        hit(2 * Q, 90),
    ];
    assert_eq!(f.gesture_edits().last(), Some(&paint(2, hits.clone())));
    m.drum_stroke_ended(Q / 2 + 3, 1, Some(127));
    assert_eq!(
        f.gesture_edits().last(),
        Some(&paint(2, hits[..2].to_vec())),
        "back towards the start: the run shrinks, each hit keeps its velocity"
    );
    assert_eq!(edit_calls(&f).last().map(String::as_str), Some("end_edit"));
    assert!(m.gesture().is_none());
}

#[test]
fn escape_and_refusals_leave_no_undo_step() {
    let (f, mut m) = make();
    m.drum_stroke_began(0, 1, 0, 0, 1, None, false);
    m.drum_stroke_moved(Q, 1, None);
    m.drum_stroke_cancelled();
    assert_eq!(
        edit_calls(&f),
        ["begin_edit(Paint Drum Hits)", "edit_gesture", "abort_edit"]
    );
    assert!(m.drum_stroke().is_none() && m.gesture().is_none());
    // The eraser on an empty cell erases nothing: no gesture at all.
    f.clear_calls();
    m.drum_stroke_began(1, 0, 0, 0, 1, None, true);
    m.drum_stroke_ended(0, 1, None);
    assert!(edit_calls(&f).is_empty());
    // Another drag still finishing: the stroke is refused and says so.
    let sent = f.gesture_edits().len();
    assert!(m.begin_gesture("Move", vec![(3, 40)]));
    m.drum_stroke_began(0, 1, 0, 0, 1, None, false);
    m.drum_stroke_ended(0, 1, None);
    assert_eq!(m.status(), Some(GESTURE_OPEN));
    assert_eq!(f.gesture_edits().len(), sent);
    assert!(m.drum_stroke().is_none());
}

fn bad_extend(
    _: DrumStrokeRow,
    _: Vec<DrumHitRow>,
    _: i64,
    _: i64,
    _: Option<u8>,
    _: DrumGridSettingsRow,
) -> Result<DrumStrokeRow, VizError> {
    Err(VizError::Model {
        message: "bad".into(),
    })
}

#[test]
fn a_failed_extend_on_mouse_up_aborts_the_gesture() {
    let (f, mut m) = make();
    m.drum_stroke_began(0, 1, 0, 0, 1, None, false);
    m.drum_stroke_moved(Q, 1, None);
    m.set_drum_extend_for_test(bad_extend);
    m.drum_stroke_ended(Q, 1, None);
    assert_eq!(
        edit_calls(&f).last().map(String::as_str),
        Some("abort_edit")
    );
    assert!(m.drum_stroke().is_none() && m.gesture().is_none());
}

#[test]
fn a_new_press_cancels_a_stroke_whose_release_was_lost() {
    let (f, mut m) = make();
    m.drum_stroke_began(0, 1, 0, 0, 1, None, false);
    m.drum_stroke_moved(Q, 1, None);
    assert!(m.gesture().is_some());
    m.drum_stroke_began(0, 0, 2 * S16, 2 * S16, 1, None, false);
    assert_eq!(
        edit_calls(&f).last().map(String::as_str),
        Some("abort_edit")
    );
    assert!(m.gesture().is_none());
    let s = m.drum_stroke().expect("the new stroke");
    assert_eq!((s.lane, s.row), (0, 0));
    assert_eq!(m.drum_selected_row(), Some(DrumRowKey { lane: 40, row: 1 }));
}

#[test]
fn a_grid_that_stops_showing_drops_its_stroke() {
    let (f, mut m) = make();
    m.drum_stroke_began(0, 1, 0, 0, 1, None, false);
    m.drum_stroke_moved(Q, 1, None);
    m.show_detail(DetailEditorKind::List);
    assert_eq!(
        edit_calls(&f).last().map(String::as_str),
        Some("abort_edit")
    );
    assert!(m.gesture().is_none() && m.drum_stroke().is_none());

    let (f, mut m) = make();
    m.drum_stroke_began(0, 1, 0, 0, 1, None, false);
    m.drum_stroke_moved(Q, 1, None);
    m.close_detail();
    assert_eq!(
        edit_calls(&f).last().map(String::as_str),
        Some("abort_edit")
    );
    assert!(m.gesture().is_none() && m.drum_stroke().is_none());

    // Another target in the grid drops it too; the same one keeps it.
    let (_f, mut m) = make();
    m.drum_stroke_began(0, 1, 0, 0, 1, None, false);
    m.open_detail(lane(3, 40), DRUM);
    assert!(m.drum_stroke().is_some(), "the same detail keeps it");
    m.open_detail(lane(3, 41), DRUM);
    assert!(m.drum_stroke().is_none());
}

#[test]
fn a_stroke_paints_the_row_it_began_on_after_the_rows_reorder() {
    let (f, mut m) = make();
    m.drum_stroke_began(0, 1, 2 * S16, 2 * S16, 1, None, false);
    let mut lanes = m.drum_lanes().to_vec();
    lanes[0].rows.swap(0, 1);
    f.set_drum_lanes(3, lanes);
    bump(&mut m, true);
    assert_eq!(m.drum_row(0, 1).map(|r| r.id), Some(1), "Kick is row 1 now");
    m.drum_stroke_moved(3 * S16, 1, None);
    m.drum_stroke_ended(3 * S16, 1, None);
    let Some(EditCommand::Drum {
        edit: DrumEdit::Paint { row, .. },
    }) = f.gesture_edits().last().cloned()
    else {
        panic!("a paint")
    };
    assert_eq!(row, 2, "Snare's ID");
    assert_eq!(edit_calls(&f).last().map(String::as_str), Some("end_edit"));
}
