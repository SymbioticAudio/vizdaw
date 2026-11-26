mod support;

use std::sync::Arc;

use core_control::registry::{self, ArgValue};
use core_control::{ControlError, ErrorCode};
use support::demo_fake;
use vizdaw_app::DocumentModel;
use vizdaw_app::commands::{DOCUMENT_COMMANDS, enabled, perform_from_menu, run};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::snapshot::DocSnapshot;
use vizdaw_ffi::tracks::TrackKindRow;

fn model(f: &Arc<support::FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

fn int(name: &str, v: i64) -> (String, ArgValue) {
    (name.to_string(), ArgValue::Int(v))
}

fn text(name: &str, v: &str) -> (String, ArgValue) {
    (name.to_string(), ArgValue::Text(v.to_string()))
}

fn code(r: Result<vizdaw_app::commands::Ran, ControlError>) -> ErrorCode {
    r.expect_err("expected an error").code
}

fn labelled(f: &support::FakeSession, undo: Option<&str>, redo: Option<&str>) {
    let mut s: DocSnapshot = f.snap();
    s.undo_label = undo.map(String::from);
    s.redo_label = redo.map(String::from);
    f.bump(s);
}

#[test]
fn each_new_item_makes_its_kind_below_the_selection() {
    for (id, kind) in [
        ("track.new_audio", TrackKindRow::Audio),
        ("track.new_instrument", TrackKindRow::Instrument),
        ("track.new_midi", TrackKindRow::Midi),
    ] {
        let f = Arc::new(demo_fake());
        let mut m = model(&f);
        m.select_track(30, false);
        let ran = run(id, &[], &mut m).unwrap();
        assert_eq!(last_call(&f), format!("new_track(3,{kind:?},Some(30))"));
        let ids: Vec<u64> = m.lanes().iter().map(|l| l.id).collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(m.lanes()[1].kind, kind);
        assert_eq!(m.selected_lanes(), vec![m.lanes()[1].id]);
        assert_eq!(ran.status.as_deref(), m.status());
    }
}

#[test]
fn explicit_seq_and_after_are_used() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    run("track.new_audio", &[int("after", 31)], &mut m).unwrap();
    assert_eq!(last_call(&f), "new_track(3,Audio,Some(31))");
    // Another sequence: sent there, the shown selection untouched.
    run("track.new_audio", &[int("seq", 1)], &mut m).unwrap();
    assert_eq!(last_call(&f), "new_track(1,Audio,None)");
}

#[test]
fn track_items_need_a_sequence() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select(None);
    assert!(!enabled("track.new_audio", &m));
    let e = run("track.new_audio", &[], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "Select a sequence first.");
    // A sequence but no track: the lane commands say so.
    m.select(Some(3));
    assert!(enabled("track.new_audio", &m));
    assert!(!enabled("track.delete", &m));
    let e = run("track.delete", &[], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "Select a track in the arrangement first.");
    m.select_track(30, false);
    assert!(enabled("track.delete", &m));
    assert!(enabled("track.rename", &m));
    assert!(enabled("track.duplicate", &m));
    assert!(!enabled("nope", &m));
}

#[test]
fn a_midi_track_without_destination_says_so() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    let ran = run("track.new_midi", &[], &mut m).unwrap();
    let want = "No external MIDI destination yet: choose one in the track's destination menu.";
    assert_eq!(m.status(), Some(want));
    assert_eq!(ran.status.as_deref(), Some(want));
}

#[test]
fn duplicate_selects_the_copy() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select_track(31, false);
    run("track.duplicate", &[], &mut m).unwrap();
    assert_eq!(last_call(&f), "duplicate_track(3,31)");
    assert_eq!(m.selected_lanes(), vec![m.lanes()[2].id]);
    assert_ne!(m.lanes()[2].id, 31);
}

#[test]
fn delete_the_only_track_leaves_no_selection() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select(Some(1));
    m.select_track(10, false);
    run("track.delete", &[], &mut m).unwrap();
    assert_eq!(
        f.edits(),
        [EditCommand::Lane {
            edit: LaneEdit::Remove { seq: 1, lane: 10 }
        }]
    );
    assert!(m.lanes().is_empty());
    assert!(m.selected_lanes().is_empty());
    assert!(!enabled("track.delete", &m));
}

#[test]
fn rename_sends_the_name_and_blank_is_the_cores_refusal() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select_track(31, false);
    run("track.rename", &[text("name", "Keys")], &mut m).unwrap();
    assert_eq!(m.lanes()[1].name, "Keys");
    f.set_edit_error(Some(VizError::Model {
        message: "A track needs a name.".into(),
    }));
    let e = run("track.rename", &[text("name", "  ")], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "A track needs a name.");
}

#[test]
fn track_commands_refuse_hostile_input() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select_track(30, false);
    let before = f.calls().len();
    type Args = Vec<(String, ArgValue)>;
    let cases: Vec<(&str, Args, ErrorCode)> = vec![
        (
            "track.new_audio",
            vec![int("seq", -1)],
            ErrorCode::InvalidParams,
        ),
        (
            "track.new_audio",
            vec![int("after", -5)],
            ErrorCode::InvalidParams,
        ),
        (
            "track.new_audio",
            vec![text("seq", "1")],
            ErrorCode::InvalidParams,
        ),
        (
            "track.new_audio",
            vec![("after".into(), ArgValue::Flag(true))],
            ErrorCode::InvalidParams,
        ),
        (
            "track.new_audio",
            vec![int("seq", i64::MIN)],
            ErrorCode::InvalidParams,
        ),
        (
            "track.new_audio",
            vec![int("bogus", 1)],
            ErrorCode::InvalidParams,
        ),
        (
            "track.new_audio",
            vec![int("seq", 1), int("seq", 2)],
            ErrorCode::InvalidParams,
        ),
        ("track.new_audio", vec![int("seq", 99)], ErrorCode::Failed),
        ("track.new_audio", vec![int("after", 99)], ErrorCode::Failed),
        (
            "track.duplicate",
            vec![int("lane", -1)],
            ErrorCode::InvalidParams,
        ),
        ("track.duplicate", vec![int("lane", 99)], ErrorCode::Failed),
        ("track.delete", vec![int("lane", 99)], ErrorCode::Failed),
        (
            "track.rename",
            vec![int("name", 5)],
            ErrorCode::InvalidParams,
        ),
        (
            "track.rename",
            vec![int("lane", 99), text("name", "x")],
            ErrorCode::Failed,
        ),
        ("track.nonsense", vec![], ErrorCode::UnknownId),
    ];
    for (id, args, want) in cases {
        assert_eq!(code(run(id, &args, &mut m)), want, "{id} {args:?}");
    }
    let e = run("track.new_audio", &[int("after", 99)], &mut m).unwrap_err();
    assert_eq!(e.message, "There is no track 99 in this sequence.");
    assert_eq!(f.calls().len(), before, "no session call was made");
    assert!(f.edits().is_empty());
}

#[test]
fn rename_without_a_name_asks_for_one() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select_track(30, false);
    let e = run("track.rename", &[], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::DialogRequired);
    assert_eq!(e.dialog.as_deref(), Some("Rename Track"));
    assert!(f.edits().is_empty());
}

#[test]
fn undo_and_redo_report_the_label_and_run() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    labelled(&f, Some("New Track"), None);
    m.undo(); // takes the labelled snapshot
    assert!(enabled("edit.undo", &m));
    assert!(!enabled("edit.redo", &m));
    labelled(&f, None, Some("New Track"));
    let ran = run("edit.undo", &[], &mut m).unwrap();
    assert_eq!(ran.undo, None);
    assert!(enabled("edit.redo", &m));
    assert_eq!(f.calls().last().unwrap(), "undo");
}

#[test]
fn undo_with_nothing_to_undo_is_unavailable() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    let e = run("edit.undo", &[], &mut m).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::Unavailable, "Nothing to undo.")
    );
    let e = run("edit.redo", &[], &mut m).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::Unavailable, "Nothing to redo.")
    );
    assert_eq!(f.count("undo") + f.count("redo"), 0);
}

fn undoable(f: &Arc<support::FakeSession>) -> DocumentModel {
    let mut m = model(f);
    let mut s = f.snap();
    s.undo_label = Some("New Track".into());
    s.redo_label = Some("Rename".into());
    m.apply(s);
    m
}

/// The last call that was not a selection write.
fn last_call(f: &support::FakeSession) -> String {
    f.calls()
        .into_iter()
        .rfind(|c| c != "set_selection")
        .unwrap()
}

#[test]
fn undo_and_redo_on_a_busy_core_are_busy_errors() {
    let f = Arc::new(demo_fake());
    let mut m = undoable(&f);
    f.set_step_error(Some(VizError::Busy {
        message: "A plug-in is busy.".into(),
    }));
    for id in ["edit.undo", "edit.redo"] {
        let e = run(id, &[], &mut m).unwrap_err();
        assert_eq!(e.code, ErrorCode::Busy, "{id}");
        assert_eq!(e.message, "A plug-in is busy.");
    }
}

#[test]
fn the_menu_path_swallows_busy_undo() {
    let f = Arc::new(demo_fake());
    let mut m = undoable(&f);
    f.set_step_error(Some(VizError::Busy {
        message: "busy".into(),
    }));
    assert!(perform_from_menu("edit.undo", &mut m).is_ok());
    assert!(perform_from_menu("edit.redo", &mut m).is_ok());
    assert!(m.alert.is_none());
    f.set_step_error(None);
    perform_from_menu("edit.undo", &mut m).unwrap();
    assert_eq!(f.calls().last().unwrap(), "undo");
}

#[test]
fn the_menu_path_uses_the_selection() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select_track(31, false);
    perform_from_menu("track.delete", &mut m).unwrap();
    assert_eq!(m.lanes().len(), 1);
    // Nothing selected now: the refusal shows in the status line.
    let e = perform_from_menu("track.delete", &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(m.status(), Some("Select a track in the arrangement first."));
}

#[test]
fn rename_from_the_menu_asks_for_the_name() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select_track(30, false);
    let e = perform_from_menu("track.rename", &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::DialogRequired);
    assert_eq!(e.dialog.as_deref(), Some("Rename Track"));
}

#[test]
fn every_document_command_is_in_the_registry() {
    assert_eq!(DOCUMENT_COMMANDS.len(), 8);
    for id in DOCUMENT_COMMANDS {
        let spec = registry::find(id).unwrap_or_else(|| panic!("{id} is not registered"));
        assert!(spec.document, "{id}");
    }
}

#[test]
fn a_refused_menu_command_shows_in_the_status_line() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select_track(30, false);
    f.set_edit_error(Some(VizError::Model {
        message: "The core refused.".into(),
    }));
    let e = perform_from_menu("track.delete", &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(m.status(), Some("The core refused."));
}

#[test]
fn a_failed_menu_undo_raises_the_alert() {
    let f = Arc::new(demo_fake());
    let mut m = undoable(&f);
    f.set_step_error(Some(VizError::Model {
        message: "Cannot undo.".into(),
    }));
    perform_from_menu("edit.undo", &mut m).unwrap();
    let alert = m.alert.as_ref().expect("an alert");
    assert_eq!(alert.title, "Undo Failed");
    assert_eq!(alert.message, "Cannot undo.");
}

#[test]
fn a_failed_channel_undo_is_prefixed() {
    let f = Arc::new(demo_fake());
    let mut m = undoable(&f);
    f.set_step_error(Some(VizError::Model {
        message: "Cannot undo.".into(),
    }));
    let e = run("edit.undo", &[], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "Undo Failed: Cannot undo.");
}

#[test]
fn menu_rename_without_a_track_is_unavailable_first() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    let e = perform_from_menu("track.rename", &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(m.status(), Some("Select a track in the arrangement first."));
}
