//! Swift `GestureTests`: a drag is one open edit that becomes one undo
//! step, or none.

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;

use support::{FakeSession, editor_fake, lane_row};
use vizdaw_app::DocumentModel;
use vizdaw_app::editor::GESTURE_OPEN;
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, EventEdit};

const CHORDS: (u64, u64) = (3, 31);
const BEGIN: &str = "begin_edit(Move Block)";

fn mv(delta: i64) -> EditCommand {
    EditCommand::Event {
        edit: EventEdit::Move {
            events: vec![1],
            delta,
            to_lane: None,
        },
    }
}

fn move1() -> EditCommand {
    mv(100)
}

fn move2() -> EditCommand {
    mv(200)
}

fn busy() -> VizError {
    VizError::Busy {
        message: "busy".into(),
    }
}

fn refused(message: &str) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

fn open() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(editor_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    f.clear_calls();
    assert!(m.begin_gesture("Move Block", vec![CHORDS]));
    (f, m)
}

/// The next snapshot the fake hands out no longer lists Chords (31).
fn remove_chords(f: &FakeSession) {
    f.set_lanes(3, vec![lane_row(30, "Groove", false)]);
}

/// An idle tick brings a snapshot without Chords' revision.
fn idle_without_chords(f: &FakeSession) {
    let mut s = f.snap();
    s.version += 1;
    s.lane_revs.retain(|r| r.lane != 31);
    f.set_idle(Some(s));
}

#[test]
fn a_drag_is_one_edit_ending_with_the_last_target() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    m.update_gesture(Some(move2()));
    m.end_gesture(Some(move2()));
    assert_eq!(
        f.calls(),
        [BEGIN, "edit_gesture", "edit_gesture", "end_edit"]
    );
    assert_eq!(f.gesture_edits(), [move1(), move2()]);
    assert!(m.gesture().is_none());
    assert!(!m.gesture_open());
    assert_eq!(m.snapshot().undo_label.as_deref(), Some("Move Block"));
}

#[test]
fn busy_intermediate_edit_is_dropped_silently() {
    let (f, mut m) = open();
    f.fail_next("edit_gesture", busy());
    m.update_gesture(Some(move1()));
    assert_eq!(m.status(), None);
    assert!(m.gesture().is_some());
    m.update_gesture(Some(move2()));
    m.end_gesture(Some(move2()));
    assert_eq!(
        f.calls(),
        [BEGIN, "edit_gesture refused", "edit_gesture", "end_edit"]
    );
}

#[test]
fn busy_final_edit_is_retried_on_tick() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    f.fail_next("edit_gesture", busy());
    m.end_gesture(Some(move2()));
    assert!(m.gesture().unwrap().finishing);
    assert!(!f.calls().iter().any(|c| c == "end_edit"));
    m.tick();
    assert_eq!(f.gesture_edits(), [move1(), move2()]);
    assert_eq!(f.calls().last().unwrap(), "end_edit");
    assert!(m.gesture().is_none());
}

#[test]
fn the_plugin_timer_retries_the_pending_final() {
    let (f, mut m) = open();
    f.fail_next("edit_gesture", busy());
    m.end_gesture(Some(move1()));
    m.idle();
    assert_eq!(f.calls().last().unwrap(), "end_edit");
    assert!(m.gesture().is_none());
}

#[test]
fn a_busy_end_edit_is_retried_too() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    f.set_call_error("end_edit", Some(busy()));
    m.end_gesture(Some(move1()));
    assert!(m.gesture().is_some());
    f.set_call_error("end_edit", None);
    m.tick();
    assert!(m.gesture().is_none());
    assert_eq!(
        f.gesture_edits(),
        [move1()],
        "the applied target is not sent again"
    );
}

#[test]
fn escape_while_the_final_edit_waits_aborts() {
    let (f, mut m) = open();
    f.fail_next("edit_gesture", busy());
    m.end_gesture(Some(move1()));
    m.cancel_gesture();
    assert_eq!(f.calls().last().unwrap(), "abort_edit");
    m.tick();
    assert_eq!(f.calls().last().unwrap(), "abort_edit");
    assert!(m.gesture().is_none());
}

#[test]
fn refused_final_edit_aborts_and_shows_status() {
    let (f, mut m) = open();
    f.fail_next("edit_gesture", refused("would make a cycle"));
    m.end_gesture(Some(move1()));
    assert_eq!(f.calls(), [BEGIN, "edit_gesture refused", "abort_edit"]);
    assert_eq!(m.status(), Some("would make a cycle"));
    assert!(m.gesture().is_none());
    assert!(!f.edit_is_open());
    assert_eq!(m.snapshot().undo_label, None, "no undo step");
}

#[test]
fn a_refused_intermediate_target_is_silent() {
    let (f, mut m) = open();
    f.fail_next("edit_gesture", refused("would make a cycle"));
    m.update_gesture(Some(move1()));
    assert_eq!(m.status(), None);
    assert_eq!(m.gesture().unwrap().applied, None);
}

#[test]
fn a_drag_whose_lane_vanishes_is_cancelled() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    idle_without_chords(&f);
    m.idle();
    assert_eq!(f.calls().last().unwrap(), "abort_edit");
    assert!(m.gesture().is_none());
    assert_eq!(
        m.status(),
        Some("The track was removed; the drag was cancelled.")
    );
}

#[test]
fn ending_without_a_move_closes_an_empty_edit() {
    let (f, mut m) = open();
    m.end_gesture(None);
    assert_eq!(f.calls(), [BEGIN, "end_edit"]);
}

#[test]
fn returning_to_the_start_rewinds() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    m.update_gesture(None);
    assert_eq!(f.calls(), [BEGIN, "edit_gesture", "abort_edit", BEGIN]);
    m.end_gesture(None);
    assert_eq!(f.calls().last().unwrap(), "end_edit");
}

#[test]
fn only_one_gesture_at_a_time() {
    let (f, mut m) = open();
    assert!(!m.begin_gesture("Copy Block", vec![]));
    assert_eq!(f.calls(), [BEGIN]);
    assert_eq!(m.gesture().unwrap().label, "Move Block");
}

#[test]
fn a_busy_intermediate_edit_forgets_the_applied_target() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    f.fail_next("edit_gesture", busy());
    m.update_gesture(Some(move2()));
    m.end_gesture(Some(move1()));
    assert_eq!(
        f.calls(),
        [
            BEGIN,
            "edit_gesture",
            "edit_gesture refused",
            "edit_gesture",
            "end_edit"
        ]
    );
    assert_eq!(f.gesture_edits(), [move1(), move1()]);
    assert!(m.gesture().is_none());
}

#[test]
fn a_busy_intermediate_edit_then_back_to_the_start_rewinds() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    f.fail_next("edit_gesture", busy());
    m.update_gesture(Some(move2()));
    m.update_gesture(None);
    let calls = f.calls();
    assert_eq!(calls[calls.len() - 2..], ["abort_edit", BEGIN]);
}

#[test]
fn a_refused_target_then_mouse_up_without_one_aborts() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    f.fail_next("edit_gesture", refused("refused"));
    m.update_gesture(Some(move2()));
    m.end_gesture(None);
    assert_eq!(f.calls().last().unwrap(), "abort_edit");
    assert!(!f.calls().iter().any(|c| c == "end_edit"));
}

#[test]
fn a_gesture_cancelled_by_a_snapshot_stays_cancelled() {
    let (f, mut m) = open();
    remove_chords(&f);
    m.update_gesture(Some(move1()));
    assert!(m.gesture().is_none());
    assert!(!f.edit_is_open());
}

#[test]
fn an_edit_during_a_gesture_is_refused_without_a_session_call() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    assert!(!m.edit(move2()));
    assert_eq!(m.status(), Some(GESTURE_OPEN));
    assert!(f.edits().is_empty());
    assert_eq!(f.calls(), [BEGIN, "edit_gesture"]);
    assert!(m.gesture().is_some(), "the drag goes on");
}

#[test]
fn undo_and_redo_during_a_gesture_are_refused() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    m.undo();
    m.redo();
    assert_eq!(f.count("undo"), 0);
    assert_eq!(f.count("redo"), 0);
    assert_eq!(m.status(), Some(GESTURE_OPEN));
    assert!(m.gesture().is_some());
    m.end_gesture(Some(move1()));
    m.undo();
    assert_eq!(f.count("undo"), 1, "allowed again once the drag ended");
}

#[test]
fn a_rewind_whose_gesture_was_cancelled_does_not_reopen_the_edit() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    // The abort's snapshot lacks the gesture's lane: it is cancelled.
    remove_chords(&f);
    m.update_gesture(None);
    assert!(m.gesture().is_none());
    assert_eq!(f.count(BEGIN), 1, "no orphan edit: {:?}", f.calls());
    assert!(!f.edit_is_open());
}

// Beyond the Swift suite.

#[test]
fn a_tick_with_nothing_pending_calls_nothing() {
    let f = Arc::new(editor_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    f.clear_calls();
    m.tick();
    assert!(f.calls().is_empty());
    assert!(m.begin_gesture("Move Block", vec![CHORDS]));
    m.update_gesture(Some(move1()));
    let before = f.calls();
    m.tick();
    assert_eq!(f.calls(), before, "an open drag waits for its mouse-up");
}

#[test]
fn a_busy_abort_is_retried_on_tick_and_holds_the_drag_meanwhile() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    f.fail_next("abort_edit", busy());
    assert!(!m.cancel_gesture());
    assert!(m.gesture_open(), "the edit is still open");
    m.update_gesture(Some(move2()));
    m.end_gesture(Some(move2()));
    assert_eq!(f.gesture_edits(), [move1()], "nothing else touches it");
    m.tick();
    assert!(m.gesture().is_none());
    assert!(!f.edit_is_open());
    assert_eq!(f.calls().last().unwrap(), "abort_edit");
}

#[test]
fn a_refused_begin_opens_nothing_and_says_why() {
    let f = Arc::new(editor_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    f.fail_next("begin_edit", refused("no sequence"));
    assert!(!m.begin_gesture("Move Block", vec![CHORDS]));
    assert!(!m.gesture_open());
    assert_eq!(m.status(), Some("no sequence"));
}

#[test]
fn the_gesture_watches_the_lanes_it_is_given() {
    let (_, mut m) = open();
    m.set_gesture_lanes(vec![CHORDS, (3, 30), CHORDS]);
    let lanes: BTreeSet<_> = [CHORDS, (3, 30)].into();
    assert_eq!(m.gesture().unwrap().lanes, lanes);
}

/// The snapshot of a move drops the drag's track and the abort meets
/// Busy: the drag stays reverting (a later mouse-up never commits it).
#[test]
fn a_busy_abort_as_a_move_drops_the_track_is_never_committed() {
    let (f, mut m) = open();
    remove_chords(&f);
    f.fail_next("abort_edit", busy());
    m.update_gesture(Some(move1()));
    m.end_gesture(Some(move1()));
    assert!(
        !f.calls().iter().any(|c| c == "end_edit"),
        "{:?}",
        f.calls()
    );
    m.tick();
    assert_eq!(f.calls().last().unwrap(), "abort_edit");
    assert!(m.gesture().is_none());
    assert!(!f.calls().iter().any(|c| c == "end_edit"));
    assert_ne!(m.snapshot().undo_label.as_deref(), Some("Move Block"));
}

/// The same when the final edit's snapshot drops the track.
#[test]
fn a_busy_abort_as_the_final_edit_drops_the_track_is_never_committed() {
    let (f, mut m) = open();
    m.update_gesture(Some(move1()));
    remove_chords(&f);
    f.fail_next("abort_edit", busy());
    m.end_gesture(Some(move2()));
    assert!(
        !f.calls().iter().any(|c| c == "end_edit"),
        "{:?}",
        f.calls()
    );
    m.tick();
    assert_eq!(f.calls().last().unwrap(), "abort_edit");
    assert!(m.gesture().is_none());
    assert!(!f.calls().iter().any(|c| c == "end_edit"));
}
