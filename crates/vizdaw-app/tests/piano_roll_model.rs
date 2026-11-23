//! The piano roll's model: Swift `PianoRollModelTests` (the audition
//! cases come with the audition; torn-off windows are out), the
//! `(seq, lane)` reads, `editors.piano_roll` and the double-click rule.

mod support;

use std::sync::Arc;

use support::{FakeSession, block_row, lane_row, note_row, roll_fake};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget};
use vizdaw_app::{DocumentModel, commands};
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
use vizdaw_ffi::events::{EventKindRow, EventRow};
use vizdaw_ffi::selection::TickRange;

fn key(lane: u64) -> LaneRef {
    LaneRef { seq: 3, lane }
}

fn lane_target(lane: u64) -> EditorTarget {
    EditorTarget::Lane { seq: 3, lane }
}

/// The roll open on `target` (with extra shown tracks `lanes`).
fn make(target: EditorTarget, lanes: Option<Vec<u64>>) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(roll_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(target, Some(DetailEditorKind::PianoRoll));
    if let Some(lanes) = lanes {
        m.update_layout(|l| l.detail.as_mut().unwrap().set_lanes(Some(lanes)));
    }
    (f, m)
}

fn detail_lanes(m: &DocumentModel) -> Option<Vec<u64>> {
    m.layout().detail.as_ref().and_then(|d| d.lanes())
}

#[test]
fn a_lane_target_shows_its_lane_plus_extras() {
    let (_, mut m) = make(lane_target(31), None);
    assert_eq!(m.roll_shown_lanes(), vec![key(31)]);
    m.roll_toggle_shown(32);
    assert_eq!(m.roll_shown_lanes(), vec![key(31), key(32)]);
    assert_eq!(detail_lanes(&m), Some(vec![32]));
    m.roll_toggle_shown(31);
    assert!(m.roll_is_shown(31), "the target lane always shows");
    m.roll_toggle_shown(32);
    assert_eq!(m.roll_shown_lanes(), vec![key(31)]);
    assert_eq!(detail_lanes(&m), None, "no extras is none");
}

#[test]
fn a_sequence_target_shows_lanes_with_notes_or_a_destination() {
    let (_, mut m) = make(EditorTarget::Sequence { seq: 3 }, None);
    assert_eq!(m.roll_shown_lanes(), vec![key(31), key(32)]);
    m.roll_toggle_shown(31);
    assert_eq!(m.roll_shown_lanes(), vec![key(32)]);
    assert_eq!(detail_lanes(&m), Some(vec![32]));
    m.roll_toggle_shown(30);
    assert_eq!(
        m.roll_shown_lanes(),
        vec![key(30), key(32)],
        "sequence order"
    );
    m.roll_toggle_shown(30);
    m.roll_toggle_shown(32);
    assert_eq!(m.roll_shown_lanes(), vec![], "a sequence may show none");
    assert_eq!(detail_lanes(&m), Some(vec![]));
}

#[test]
fn a_track_with_regions_shows_by_default() {
    let f = Arc::new(roll_fake());
    let mut audio = lane_row(33, "Vox", false);
    audio.region_count = 1;
    let mut rows = vizdaw_app::DocSession::lanes(f.as_ref(), 3).unwrap();
    rows.push(audio);
    f.set_lanes(3, rows);
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(
        EditorTarget::Sequence { seq: 3 },
        Some(DetailEditorKind::PianoRoll),
    );
    assert_eq!(m.roll_shown_lanes(), vec![key(31), key(32), key(33)]);
    assert!(m.roll_notes(key(33)).is_empty());
}

#[test]
fn colours_follow_the_lane_index_in_its_sequence() {
    let (_, m) = make(EditorTarget::Sequence { seq: 3 }, None);
    assert_eq!(m.roll_color_index(key(30)), 0);
    assert_eq!(m.roll_color_index(key(32)), 2);
    assert_eq!(m.roll_color_index(key(999)), 0, "a missing lane");
}

#[test]
fn the_edit_lane_follows_the_last_chosen_shown_lane() {
    let (_, mut m) = make(lane_target(31), Some(vec![32]));
    assert_eq!(m.roll_edit_lane(), Some(key(31)));
    m.roll_select(&[(key(32), 3)], false);
    assert_eq!(m.roll_edit_lane(), Some(key(32)));
    m.roll_set_edit_lane(key(31));
    assert_eq!(m.roll_edit_lane(), Some(key(31)), "Insert Into");
}

#[test]
fn the_edit_lane_falls_back_when_its_lane_is_hidden() {
    let (_, mut m) = make(lane_target(31), Some(vec![32]));
    m.roll_set_edit_lane(key(32));
    assert_eq!(m.piano_roll().chosen_lane(), Some(key(32)));
    m.roll_toggle_shown(32);
    assert_eq!(m.roll_edit_lane(), Some(key(31)));
}

#[test]
fn a_sequence_targets_edit_lane_is_its_first_shown_lane() {
    let (_, mut m) = make(EditorTarget::Sequence { seq: 3 }, None);
    assert_eq!(m.roll_edit_lane(), Some(key(31)));
    m.roll_toggle_shown(31);
    m.roll_toggle_shown(32);
    assert_eq!(m.roll_edit_lane(), None, "nothing shown");
    m.close_detail();
    assert_eq!(m.roll_edit_lane(), None);
    assert!(m.roll_shown_lanes().is_empty());
}

#[test]
fn selecting_toggling_and_deleting_notes() {
    let (f, mut m) = make(lane_target(31), Some(vec![32]));
    m.roll_select(&[(key(31), 1), (key(32), 3)], false);
    assert_eq!(m.selection().events, vec![1, 3]);
    assert_eq!(m.selection().lanes, vec![key(31), key(32)]);
    assert_eq!(m.selection().time_range, None);
    m.roll_toggle(1, key(31));
    assert_eq!(m.selection().events, vec![3]);
    assert_eq!(
        m.selection().lanes,
        vec![key(31), key(32)],
        "removing leaves the lane"
    );
    m.roll_delete_selected();
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Event {
            edit: EventEdit::Delete { events: vec![3] }
        })
    );
}

#[test]
fn toggling_adds_a_note_with_its_lane_and_chooses_it() {
    let (_, mut m) = make(lane_target(31), Some(vec![32]));
    m.roll_select(&[(key(31), 1)], false);
    m.roll_toggle(3, key(32));
    assert_eq!(m.selection().events, vec![1, 3]);
    assert_eq!(m.selection().lanes, vec![key(31), key(32)]);
    assert_eq!(m.roll_edit_lane(), Some(key(32)));
    m.roll_select(&[(key(31), 2)], true);
    assert_eq!(m.selection().events, vec![1, 3, 2], "adding keeps the rest");
    assert_eq!(m.roll_edit_lane(), Some(key(31)));
}

#[test]
fn a_selection_replaced_by_the_roll_drops_its_time_range() {
    let (_, mut m) = make(lane_target(31), None);
    m.set_time_range(TickRange { start: 0, end: 100 });
    m.roll_select(&[(key(31), 1)], true);
    assert_eq!(
        m.selection().time_range,
        Some(TickRange { start: 0, end: 100 })
    );
    m.roll_select(&[(key(31), 1)], false);
    assert_eq!(m.selection().time_range, None);
}

#[test]
fn deleting_sends_only_notes_of_shown_lanes() {
    let (f, mut m) = make(lane_target(31), None);
    m.roll_delete_selected();
    assert!(f.edits().is_empty(), "nothing selected sends nothing");
    // Note 3 is Bass's, which is hidden.
    m.roll_select(&[(key(31), 2)], false);
    m.roll_toggle(3, key(32));
    m.roll_delete_selected();
    assert_eq!(
        f.edits(),
        vec![EditCommand::Event {
            edit: EventEdit::Delete { events: vec![2] }
        }]
    );
    let (f, mut m) = make(lane_target(31), None);
    m.roll_toggle(3, key(32));
    m.roll_delete_selected();
    assert!(f.edits().is_empty(), "only a hidden lane's note");
}

#[test]
fn a_notes_sound_goes_to_its_own_destination_else_its_lanes() {
    let (_, m) = make(lane_target(30), None);
    let mut row = note_row(9, 0, 60);
    assert_eq!(m.roll_dest(&row, key(30)), None, "Groove has none");
    assert_eq!(m.roll_dest(&row, key(31)), Some(5));
    row.dest = Some(9);
    assert_eq!(m.roll_dest(&row, key(30)), Some(9), "the event's own wins");
}

#[test]
fn the_notes_are_the_lanes_notes_only() {
    let f = Arc::new(roll_fake());
    f.set_events(
        3,
        31,
        vec![
            note_row(1, 0, 60),
            EventRow::blank(7, 10, EventKindRow::Cc),
            note_row(2, 15_360, 64),
        ],
    );
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(lane_target(31), Some(DetailEditorKind::PianoRoll));
    let ids: Vec<u64> = m.roll_notes(key(31)).iter().map(|n| n.id).collect();
    assert_eq!(ids, vec![1, 2]);
    let lanes = m.roll_lane_notes();
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].lane, key(31));
    assert_eq!(lanes[0].notes.len(), 2);
}

#[test]
fn a_shown_lane_that_vanishes_is_dropped() {
    let (f, mut m) = make(lane_target(31), Some(vec![32]));
    let rows = vizdaw_app::DocSession::lanes(f.as_ref(), 3).unwrap();
    f.set_lanes(3, rows.into_iter().filter(|l| l.id != 32).collect());
    m.rename_track(31, "Kit");
    assert_eq!(detail_lanes(&m), None);
    assert!(
        m.layout().detail.is_some(),
        "the target lane is still there"
    );
    assert_eq!(m.roll_shown_lanes(), vec![key(31)]);
}

#[test]
fn a_vanished_extra_lane_keeps_the_others() {
    let f = Arc::new(roll_fake());
    let mut rows = vizdaw_app::DocSession::lanes(f.as_ref(), 3).unwrap();
    rows.push(lane_row(33, "Lead", false));
    f.set_lanes(3, rows.clone());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(lane_target(31), Some(DetailEditorKind::PianoRoll));
    m.update_layout(|l| l.detail.as_mut().unwrap().set_lanes(Some(vec![32, 33])));
    f.set_lanes(3, rows.into_iter().filter(|l| l.id != 32).collect());
    m.rename_track(31, "Kit");
    assert_eq!(detail_lanes(&m), Some(vec![33]));
}

#[test]
fn a_block_target_has_a_focus_range() {
    let block = EditorTarget::Block {
        seq: 3,
        lane: 31,
        start: 61_440,
        end: 122_880,
    };
    let (_, m) = make(block, None);
    assert_eq!(
        m.roll_focus_range(),
        Some(TickRange {
            start: 61_440,
            end: 122_880
        })
    );
    assert_eq!(m.roll_shown_lanes(), vec![key(31)]);
    let empty = EditorTarget::Block {
        seq: 3,
        lane: 31,
        start: 50,
        end: 50,
    };
    let (_, m) = make(empty, None);
    assert_eq!(m.roll_focus_range(), Some(TickRange { start: 50, end: 51 }));
    let (_, m) = make(lane_target(31), None);
    assert_eq!(m.roll_focus_range(), None);
}

#[test]
fn lane_rows_are_cached_per_snapshot_version() {
    let (f, mut m) = make(lane_target(31), None);
    let before = f.lanes_reads();
    let a = m.lane_rows(2);
    let b = m.lane_rows(2);
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(f.lanes_reads(), before + 1);
    assert_eq!(a[0].id, 20);
    m.rename_track(31, "Kit");
    let mid = f.lanes_reads();
    let _ = m.lane_rows(2);
    let _ = m.lane_rows(2);
    assert_eq!(f.lanes_reads(), mid + 1);
}

#[test]
fn the_shown_sequences_rows_are_not_read_twice() {
    let (f, m) = make(lane_target(31), None);
    let before = f.lanes_reads();
    let rows = m.lane_rows(3);
    assert_eq!(rows.len(), 3);
    assert_eq!(m.lanes().len(), 3);
    assert_eq!(f.lanes_reads(), before, "the shown sequence's fetch serves");
}

#[test]
fn lane_events_of_any_sequence_are_cached_per_snapshot_version() {
    let (f, mut m) = make(lane_target(31), None);
    f.set_events(2, 20, vec![note_row(8, 0, 36)]);
    let before = f.event_reads();
    let a = m.lane_events(2, 20);
    assert!(Arc::ptr_eq(&a, &m.lane_events(2, 20)));
    assert_eq!(a.len(), 1);
    assert_eq!(f.event_reads(), before + 1);
    // The shown sequence's reads are the same cache.
    let shown = m.shown_lane_events(31);
    assert!(Arc::ptr_eq(&shown, &m.lane_events(3, 31)));
    f.set_events(2, 20, vec![note_row(8, 0, 36), note_row(9, 10, 37)]);
    m.rename_track(31, "Kit");
    assert_eq!(m.lane_events(2, 20).len(), 2, "a new version reads again");
}

#[test]
fn the_roll_is_rebuilt_when_the_target_or_lanes_change() {
    let (_, mut m) = make(lane_target(31), None);
    let a = m.roll_lane_notes();
    let builds = m.piano_roll().builds();
    let b = m.roll_lane_notes();
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(m.piano_roll().builds(), builds);
    m.roll_toggle_shown(32);
    assert_eq!(m.roll_lane_notes().len(), 2);
    m.open_detail(lane_target(32), None);
    assert_eq!(m.roll_lane_notes()[0].lane, key(32));
    assert_eq!(m.roll_lane_notes().len(), 1, "extras go with the target");
    assert!(m.piano_roll().builds() >= builds + 2);
}

#[test]
fn editors_piano_roll_opens_the_roll() {
    let f = Arc::new(roll_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    assert!(commands::EDITOR_COMMANDS.contains(&"editors.piano_roll"));
    assert!(commands::enabled("editors.piano_roll", &m));
    commands::run("editors.piano_roll", &[], &mut m).unwrap();
    let d = m.layout().detail.clone().unwrap();
    assert_eq!(d.kind, DetailEditorKind::PianoRoll);
    assert_eq!(d.target, EditorTarget::Sequence { seq: 3 });
    m.open_detail(lane_target(31), Some(DetailEditorKind::List));
    commands::perform_from_menu("editors.piano_roll", &mut m).unwrap();
    let d = m.layout().detail.clone().unwrap();
    assert_eq!(d.kind, DetailEditorKind::PianoRoll);
    assert_eq!(d.target, lane_target(31), "on the current target");
    assert_eq!(m.detail_kind_name(), "piano_roll");
}

#[test]
fn double_clicking_a_block_opens_the_last_used_editor() {
    let f = Arc::new(roll_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    let b = block_row(0, 61_440, &[1]);
    let target = EditorTarget::Block {
        seq: 3,
        lane: 31,
        start: 0,
        end: 61_440,
    };
    m.open_block_detail(31, &b);
    let d = m.layout().detail.clone().unwrap();
    assert_eq!((d.kind, d.target), (DetailEditorKind::PianoRoll, target));
    assert_eq!(m.layout().last_kind, DetailEditorKind::PianoRoll);
    m.update_layout(|l| l.last_kind = DetailEditorKind::List);
    m.close_detail();
    m.open_block_detail(31, &b);
    let d = m.layout().detail.clone().unwrap();
    assert_eq!(d.kind, DetailEditorKind::List);
    assert_eq!(m.layout().last_kind, DetailEditorKind::List, "unchanged");
}
