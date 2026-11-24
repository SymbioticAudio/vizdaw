mod support;

use std::path::Path;
use std::sync::Arc;

use support::demo_fake;
use vizdaw_app::{DocumentModel, SaveAction};
use vizdaw_ffi::VizError;

fn model(f: &Arc<support::FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

#[test]
fn undo_and_redo_menu_titles() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert_eq!(m.undo_title(), "Undo");
    assert_eq!(m.redo_title(), "Redo");
    assert!(!m.can_undo() && !m.can_redo());
    let mut next = f.snap();
    next.version += 1;
    next.undo_label = Some("Rename Sequence".into());
    next.redo_label = Some("Set Lane Mute/Solo/Arm".into());
    f.bump(next);
    // The Swift test applied the bump through a rename command; the trait
    // has no rename, so undo carries the next snapshot in.
    m.undo();
    assert_eq!(m.undo_title(), "Undo Rename Sequence");
    assert_eq!(m.redo_title(), "Redo Set Lane Mute/Solo/Arm");
    assert!(m.can_undo() && m.can_redo());
}

#[test]
fn dirty_title() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert_eq!(m.title(), "Untitled");
    let mut next = f.snap();
    next.version += 1;
    next.dirty = true;
    f.bump(next);
    m.undo();
    assert_eq!(m.title(), "Untitled \u{2014} Edited");
}

#[test]
fn save_when_untitled_needs_save_as() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert!(m.needs_save_as());
    assert_eq!(m.save(), SaveAction::NeedsSaveAs);
    assert_eq!(f.count("save"), 0, "an untitled document is never written");
    m.save_as(Path::new("/tmp/x/demo.vizdaw")).unwrap();
    assert!(!m.needs_save_as());
    assert_eq!(m.title(), "demo");
    assert!(m.alert.is_none());
    assert_eq!(m.save(), SaveAction::Saved);
}

#[test]
fn save_while_untitled_surfaces_use_save_as() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert!(m.needs_save_as());
    // Untitled routes to Save As: no write, no alert.
    assert_eq!(m.save(), SaveAction::NeedsSaveAs);
    assert_eq!(f.count("save"), 0);
    assert!(m.alert.is_none());
    assert!(m.needs_save_as());
    // A titled document whose core refuses shows the core's sentence.
    let g = Arc::new(demo_fake());
    g.set_path(Some("/tmp/x/demo.vizdaw"));
    g.set_save_error(Some(VizError::Io {
        message: "use Save As".into(),
    }));
    let mut t = model(&g);
    assert!(!t.needs_save_as());
    assert_eq!(t.save(), SaveAction::Failed("use Save As".into()));
    let a = t.alert.as_ref().unwrap();
    assert_eq!(a.title, "Save Failed");
    assert_eq!(a.message, "use Save As");
}

#[test]
fn needs_save_as_follows_the_session_path() {
    let f = Arc::new(demo_fake());
    let m = model(&f);
    assert!(m.needs_save_as());
    f.set_path(Some("/tmp/x/demo.vizdaw"));
    assert!(!m.needs_save_as());
}

#[test]
fn undo_and_redo_while_busy_do_nothing_silently() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    let before = m.snapshot().version;
    f.set_step_error(Some(VizError::Busy {
        message: "plugin host is busy".into(),
    }));
    m.undo();
    m.redo();
    assert!(m.alert.is_none());
    assert_eq!(m.snapshot().version, before);
    // Other failures still alert.
    f.set_step_error(Some(VizError::Model {
        message: "nothing to undo".into(),
    }));
    m.undo();
    assert_eq!(m.alert.as_ref().unwrap().title, "Undo Failed");
}

fn lane_ids(m: &DocumentModel) -> Vec<String> {
    m.lanes().iter().map(|l| l.name.clone()).collect()
}

#[test]
fn first_top_level_sequence_is_selected_at_start() {
    let f = Arc::new(demo_fake());
    let m = model(&f);
    assert_eq!(m.selected_seq(), Some(3));
    assert_eq!(lane_ids(&m), ["Groove", "Chords"]);
}

#[test]
fn empty_document_selects_nothing() {
    let f = Arc::new(support::FakeSession::new(vec![]));
    let m = model(&f);
    assert_eq!(m.selected_seq(), None);
    assert!(m.lanes().is_empty());
    assert_eq!(m.lanes_fetches(), 0);
}

#[test]
fn selection_survives_a_snapshot_that_keeps_the_sequence() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select(Some(2));
    let mut next = f.snap();
    next.version += 1;
    f.bump(next);
    m.undo();
    assert_eq!(m.selected_seq(), Some(2));
}

#[test]
fn selection_falls_back_when_the_sequence_is_deleted() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select(Some(1));
    let mut next = f.snap();
    next.version += 1;
    next.sequences.retain(|s| s.id != 1);
    f.bump(next);
    m.undo();
    assert_eq!(m.selected_seq(), Some(3));
    assert_eq!(lane_ids(&m), ["Groove", "Chords"]);
}

#[test]
fn lanes_are_refetched_only_when_version_or_selection_changes() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert_eq!(m.lanes_fetches(), 1);
    m.select(Some(3)); // already shown
    assert_eq!(m.lanes_fetches(), 1);
    // Same version again: no fetch.
    f.bump(f.snap());
    m.undo();
    assert_eq!(m.lanes_fetches(), 1);
    // New version: fetch.
    m.redo();
    assert_eq!(m.lanes_fetches(), 2);
    m.select(Some(2));
    assert_eq!(m.lanes_fetches(), 3);
}

#[test]
fn has_unsaved_changes_reads_the_session() {
    let f = Arc::new(demo_fake());
    let m = model(&f);
    assert!(!m.snapshot().dirty);
    assert!(!m.has_unsaved_changes());
    // The snapshot says clean; the session knows better.
    f.set_unsaved(Some(Ok(true)));
    assert!(!m.snapshot().dirty);
    assert!(m.has_unsaved_changes());
    // An unreadable answer is treated as unsaved.
    f.set_unsaved(Some(Err(VizError::Internal {
        message: "gone".into(),
    })));
    assert!(m.has_unsaved_changes());
}

#[test]
fn idle_applies_a_new_snapshot() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert!(!m.idle());
    assert_eq!(m.title(), "Untitled");
    let mut next = f.snap();
    next.version += 1;
    next.dirty = true;
    f.set_idle(Some(next));
    assert!(m.idle());
    assert_eq!(m.title(), "Untitled \u{2014} Edited");
    assert!(!m.idle());
    assert_eq!(m.title(), "Untitled \u{2014} Edited");
}

#[test]
fn undo_to_the_saved_state_is_clean() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    let mut dirty = f.snap();
    dirty.version += 1;
    dirty.dirty = true;
    let mut clean = f.snap();
    clean.version += 2;
    f.bump(dirty.clone());
    m.redo();
    assert_eq!(m.title(), "Untitled \u{2014} Edited");
    f.bump(clean);
    m.undo();
    assert_eq!(m.title(), "Untitled");
    f.bump(dirty);
    m.redo();
    assert_eq!(m.title(), "Untitled \u{2014} Edited");
}

#[test]
fn a_selected_track_that_is_deleted_falls_back() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select_track(31, false);
    assert_eq!(m.selected_lane(), Some(31));
    assert_eq!(m.selected_lanes(), [31]);
    // The session no longer names the lane after the next step.
    f.force_selection(vizdaw_ffi::selection::SelectionRow {
        time_range: None,
        lanes: vec![],
        events: vec![],
    });
    m.undo();
    assert_eq!(m.selected_lane(), None);
    assert!(m.selected_lanes().is_empty());
}

#[test]
fn select_track_replaces_or_extends() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select_track(30, false);
    m.select_track(31, true);
    assert_eq!(m.selected_lanes(), [30, 31]);
    assert_eq!(m.selected_lane(), Some(30));
    m.select_track(30, true); // toggles off
    assert_eq!(m.selected_lanes(), [31]);
    m.select_track(30, false);
    assert_eq!(m.selected_lanes(), [30]);
}

// ---------------------------------------------------------------------
// Recovered Recordings (Swift `RecordModel` offer, `RecordModelTests`)

fn recovered(path: &str, size_known: bool) -> vizdaw_ffi::record::RecoveredFileRow {
    vizdaw_ffi::record::RecoveredFileRow {
        path: path.into(),
        frames: 48_000,
        size_known,
        readable: true,
    }
}

#[test]
fn the_recovery_message_counts_and_names_the_recordings() {
    use vizdaw_app::document::recovery_message as m;
    let crash = recovered("Media/Recorded/Take 1.caf", false);
    let kept = recovered("Media/Recorded/Take 2.caf", true);
    assert_eq!(
        m(std::slice::from_ref(&crash)),
        "A recording was left by VizDAW quitting unexpectedly: Take 1."
    );
    assert_eq!(
        m(&[crash.clone(), crash.clone()]),
        "2 recordings were left by VizDAW quitting unexpectedly: Take 1, Take 1."
    );
    assert_eq!(
        m(std::slice::from_ref(&kept)),
        "A recording is not in the project: Take 2."
    );
    assert_eq!(
        m(&[crash.clone(), kept.clone(), kept]),
        "3 recordings are not in the project (1 left by VizDAW quitting unexpectedly): \
         Take 1, Take 2, Take 2."
    );
    // An unreadable file a crash left is "not in the project".
    let unreadable = vizdaw_ffi::record::RecoveredFileRow {
        readable: false,
        ..crash
    };
    assert_eq!(
        m(&[unreadable]),
        "A recording is not in the project: Take 1."
    );
}

#[test]
fn the_offer_shows_until_answered_and_import_brings_them_in() {
    let f = Arc::new(demo_fake());
    f.set_recovered_files(vec![recovered("Media/Recorded/Take 1.caf", false)]);
    let mut m = model(&f);
    assert_eq!(m.recovery_offer(), None, "offered only when asked");
    m.offer_recovery();
    assert_eq!(
        m.recovery_offer().as_deref(),
        Some("A recording was left by VizDAW quitting unexpectedly: Take 1.")
    );
    m.answer_recovery(Some(true));
    assert_eq!(f.count("recover_recordings(true)"), 1);
    assert_eq!(m.recovery_offer(), None);
}

#[test]
fn later_leaves_the_files_for_the_next_open_and_trash_moves_them() {
    let f = Arc::new(demo_fake());
    f.set_recovered_files(vec![recovered("Media/Recorded/Take 1.caf", true)]);
    let mut m = model(&f);
    m.offer_recovery();
    m.answer_recovery(None);
    assert_eq!(m.recovery_offer(), None);
    assert_eq!(f.count("recover_recordings(true)"), 0);
    assert_eq!(f.count("recover_recordings(false)"), 0);
    // Offered again on the next open.
    m.offer_recovery();
    assert!(m.recovery_offer().is_some());
    m.answer_recovery(Some(false));
    assert_eq!(f.count("recover_recordings(false)"), 1);
    // Nothing left: no offer.
    m.offer_recovery();
    assert_eq!(m.recovery_offer(), None);
}

#[test]
fn a_busy_or_failed_answer_holds_the_offer_until_the_next_poll() {
    let f = Arc::new(demo_fake());
    f.set_recovered_files(vec![recovered("Media/Recorded/Take 1.caf", false)]);
    let mut m = model(&f);
    m.offer_recovery();
    f.fail_next(
        "recover_recordings",
        VizError::Busy {
            message: "busy".into(),
        },
    );
    m.answer_recovery(Some(true));
    assert_eq!(m.recovery_offer(), None, "held");
    assert!(m.alert.is_none(), "busy is no alert");
    m.idle();
    assert!(m.recovery_offer().is_some(), "offered again");
    f.fail_next(
        "recover_recordings",
        VizError::Io {
            message: "the disk went".into(),
        },
    );
    m.answer_recovery(Some(true));
    assert_eq!(m.recovery_offer(), None, "held");
    let a = m.alert.take().unwrap();
    assert_eq!(
        (a.title.as_str(), a.message.as_str()),
        ("Recovery Failed", "the disk went")
    );
    // "Later" after a held offer keeps it (the dismissal follows a button).
    m.answer_recovery(None);
    m.idle();
    assert!(m.recovery_offer().is_some());
}

// ---- Save while recording (Swift `RecordOptionsTests`) ----

/// Song (3) with an armed stereo audio track, recording-ready.
fn recording_ready() -> (Arc<support::FakeSession>, DocumentModel) {
    use vizdaw_ffi::edit::LaneInputRow;
    let f = Arc::new(demo_fake());
    let mut l = support::lane_row(30, "Vocal", false);
    l.arm = true;
    l.input = Some(LaneInputRow::Audio {
        first: 0,
        stereo: true,
        return_of: None,
    });
    f.set_lanes(3, vec![l]);
    let mut m = model(&f);
    m.select(Some(3));
    (f, m)
}

#[test]
fn save_while_recording_says_when_the_take_is_saved() {
    use vizdaw_app::SaveChoice;
    use vizdaw_app::record::{
        SAVE_AS_WHILE_RECORDING, SAVE_UNTITLED_WHILE_RECORDING, SAVED_DURING_PASS,
    };
    let (f, mut m) = recording_ready();
    f.set_path(Some("/tmp/P.vizdaw"));
    m.toggle_record().unwrap();
    assert_eq!(m.save_choice(false), SaveChoice::Save);
    assert_eq!(m.save(), SaveAction::Saved);
    assert_eq!(m.status(), Some(SAVED_DURING_PASS));
    assert!(m.alert.is_none());
    // Save As cannot move a recording project: it waits for Stop.
    m.set_status(None);
    assert_eq!(
        m.save_choice(true),
        SaveChoice::Refused(SAVE_AS_WHILE_RECORDING.into())
    );
    assert_eq!(m.status(), None, "no side effect");
    // Untitled: its own words.
    f.set_path(None);
    assert_eq!(
        m.save_choice(false),
        SaveChoice::Refused(SAVE_UNTITLED_WHILE_RECORDING.into())
    );
    assert_eq!(
        m.save(),
        SaveAction::Refused(SAVE_UNTITLED_WHILE_RECORDING.into())
    );
    assert_eq!(m.status(), Some(SAVE_UNTITLED_WHILE_RECORDING));
    m.toggle_record().unwrap();
    assert_eq!(m.save_choice(false), SaveChoice::Panel);
    f.set_path(Some("/tmp/P.vizdaw"));
    assert_eq!(m.save_choice(true), SaveChoice::Panel);
    assert_eq!(m.save_choice(false), SaveChoice::Save);
}

/// Next small fixes Task 6: Don't Save drops the save owed at Stop.
#[test]
fn cancel_save_after_pass_drops_the_owed_save() {
    let (f, mut m) = recording_ready();
    f.set_path(Some("/tmp/P.vizdaw"));
    m.toggle_record().unwrap();
    m.save();
    assert!(m.save_after_pass_pending());
    m.cancel_save_after_pass();
    assert_eq!(f.count("cancel_save_after_pass"), 1);
    assert!(!m.save_after_pass_pending());
}

/// 5d-2 T2 M2: a pass that ended by itself since the last poll does not
/// refuse a Save As.
#[test]
fn the_save_choice_reads_the_recording_now() {
    use vizdaw_app::SaveChoice;
    let (f, mut m) = recording_ready();
    f.set_path(Some("/tmp/P.vizdaw"));
    m.toggle_record().unwrap();
    f.end_pass_by_itself();
    assert_eq!(m.save_choice(true), SaveChoice::Panel);
}

/// 5d-2 T2 M3: a titled Save while recording reaches the session.
#[test]
fn save_while_recording_reaches_the_session() {
    let (f, mut m) = recording_ready();
    f.set_path(Some("/tmp/P.vizdaw"));
    f.set_dirty(true);
    m.toggle_record().unwrap();
    m.save();
    assert!(!f.snap().dirty, "the session saved");
    assert_eq!(f.count("save"), 1);
}

/// Plan 6e Task 8 review I1: the core's Save As refusal (a held merge)
/// is the choice's, for Save As and an untitled Save; a titled Save saves.
#[test]
fn the_cores_save_as_refusal_refuses_the_panel() {
    use vizdaw_app::SaveChoice;
    let (f, mut m) = recording_ready();
    f.set_save_as_refusal(Some("Finish or cancel the merge first."));
    assert_eq!(
        m.save_choice(false),
        SaveChoice::Refused("Finish or cancel the merge first.".into())
    );
    assert_eq!(
        m.save_choice(true),
        SaveChoice::Refused("Finish or cancel the merge first.".into())
    );
    f.set_path(Some("/tmp/P.vizdaw"));
    assert_eq!(m.save_choice(false), SaveChoice::Save);
}

/// The poll offers a held Recovered Recordings offer again.
#[test]
fn the_record_poll_reoffers_a_held_recovery() {
    let f = Arc::new(demo_fake());
    f.set_recovered_files(vec![recovered("Media/Recorded/Take 1.caf", false)]);
    let mut m = model(&f);
    m.offer_recovery();
    f.fail_next(
        "recover_recordings",
        VizError::Busy {
            message: "busy".into(),
        },
    );
    m.answer_recovery(Some(true));
    assert_eq!(m.recovery_offer(), None, "held");
    assert!(m.poll_record());
    assert!(m.recovery_offer().is_some(), "offered again");
}
