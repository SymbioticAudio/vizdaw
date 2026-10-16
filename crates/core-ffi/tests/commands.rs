use vizdaw_ffi::{DocSnapshot, Session, VizError};

fn id_of(s: &DocSnapshot, name: &str) -> u64 {
    s.sequences.iter().find(|r| r.name == name).unwrap().id
}

fn has(s: &DocSnapshot, name: &str) -> bool {
    s.sequences.iter().any(|r| r.name == name)
}

#[test]
fn rename_undo_redo_updates_names_labels_and_dirty() {
    let s = Session::new_demo().unwrap();
    let beat = id_of(&s.snapshot().unwrap(), "Beat");

    let a = s.rename_sequence(beat, "Kit".into()).unwrap();
    assert!(has(&a, "Kit") && !has(&a, "Beat"));
    assert_eq!(a.undo_label.as_deref(), Some("Rename Sequence"));
    assert_eq!(a.redo_label, None);
    assert!(a.dirty);
    assert_eq!(a.version, 1);

    let b = s.undo().unwrap();
    assert!(has(&b, "Beat") && !has(&b, "Kit"));
    assert_eq!(b.undo_label, None);
    assert_eq!(b.redo_label.as_deref(), Some("Rename Sequence"));
    // Back at the opened state: clean again.
    assert!(!b.dirty);
    assert_eq!(b.version, 2);

    let c = s.redo().unwrap();
    assert!(has(&c, "Kit"));
    assert!(c.dirty);
    assert_eq!(c.version, 3);
}

#[test]
fn lane_mute_is_one_undo_step() {
    let s = Session::new_demo().unwrap();
    let song = id_of(&s.snapshot().unwrap(), "Song");
    let chords = s.lanes(song).unwrap()[1].id;
    let snap = s.set_lane_mute(song, chords, true).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Set Lane Mute/Solo/Arm"));
    assert!(s.lanes(song).unwrap()[1].mute);
    s.undo().unwrap();
    assert!(!s.lanes(song).unwrap()[1].mute);
}

#[test]
fn unknown_ids_are_model_errors_and_change_nothing() {
    let s = Session::new_demo().unwrap();
    let song = id_of(&s.snapshot().unwrap(), "Song");
    let e = s.rename_sequence(9_999, "x".into()).unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
    let e = s.set_lane_mute(song, 9_999, true).unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.version, 0);
    assert_eq!(snap.undo_label, None);
}

#[test]
fn undo_with_empty_history_is_a_model_error() {
    let s = Session::new_empty().unwrap();
    assert!(matches!(s.undo(), Err(VizError::Model { .. })));
    assert!(matches!(s.redo(), Err(VizError::Model { .. })));
}

#[test]
fn renaming_to_the_same_name_adds_no_undo_step() {
    let s = Session::new_demo().unwrap();
    let beat = id_of(&s.snapshot().unwrap(), "Beat");
    let snap = s.rename_sequence(beat, "Beat".into()).unwrap();
    assert_eq!(snap.undo_label, None);
    assert!(!snap.dirty);
}

#[test]
fn a_poisoned_session_recovers() {
    let s = Session::new_demo().unwrap();
    let e = s.debug_panic_locked().unwrap_err();
    assert!(matches!(e, VizError::Internal { .. }));
    // The mutex is poisoned; the next call still works.
    assert_eq!(s.snapshot().unwrap().sequences.len(), 3);
    let beat = id_of(&s.snapshot().unwrap(), "Beat");
    assert!(has(&s.rename_sequence(beat, "Kit".into()).unwrap(), "Kit"));
}

#[test]
fn save_after_rename_writes_the_new_name_and_clears_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let saved = dir.path().join("p.vizdaw");
    let s = Session::new_demo().unwrap();
    s.save_as(saved.to_string_lossy().into_owned()).unwrap();
    let beat = id_of(&s.snapshot().unwrap(), "Beat");
    assert!(s.rename_sequence(beat, "Kit".into()).unwrap().dirty);
    s.save().unwrap();
    assert!(!s.snapshot().unwrap().dirty);
    let reopened = Session::open(saved.to_string_lossy().into_owned()).unwrap();
    assert!(has(&reopened.snapshot().unwrap(), "Kit"));
    // Undo after saving makes the document dirty again.
    assert!(s.undo().unwrap().dirty);
}
