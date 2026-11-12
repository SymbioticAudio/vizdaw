use std::fs;
use vizdaw_ffi::{DocSnapshot, Session, VizError};

fn without_version(mut s: DocSnapshot) -> DocSnapshot {
    s.version = 0;
    s
}

#[test]
fn save_as_then_open_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_demo().unwrap();
    // `.vizdaw` is appended when missing.
    let path = dir.path().join("demo");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    let saved = dir.path().join("demo.vizdaw");
    assert!(saved.join("project.json").is_file());
    assert_eq!(
        s.path().unwrap(),
        Some(saved.to_string_lossy().into_owned())
    );
    let before = s.snapshot().unwrap();
    assert!(!before.dirty);
    assert_eq!(before.title, "demo");

    // One session per package (Task 9 review I-2): the first lets go.
    s.close().unwrap();
    let reopened = Session::open(saved.to_string_lossy().into_owned()).unwrap();
    assert_eq!(
        without_version(reopened.snapshot().unwrap()),
        without_version(before)
    );
}

#[test]
fn save_keeps_an_existing_extension() {
    let dir = tempfile::tempdir().unwrap();
    let saved = dir.path().join("p.vizdaw");
    let s = Session::new_demo().unwrap();
    s.save_as(saved.to_string_lossy().into_owned()).unwrap();
    assert!(saved.join("project.json").is_file());
    assert!(!dir.path().join("p.vizdaw.vizdaw").exists());
    // Saving again to the current path works.
    s.save().unwrap();
    assert!(!s.snapshot().unwrap().dirty);
}

#[test]
fn save_while_untitled_is_an_io_error() {
    let s = Session::new_demo().unwrap();
    assert!(matches!(s.save(), Err(VizError::Io { .. })));
}

#[test]
fn open_missing_path_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir
        .path()
        .join("nope.vizdaw")
        .to_string_lossy()
        .into_owned();
    let e = Session::open(p.clone()).unwrap_err();
    assert_eq!(e, VizError::NotFound { path: p });
    // A folder without project.json is not a project either.
    let empty = dir.path().join("empty.vizdaw");
    fs::create_dir(&empty).unwrap();
    let e = Session::open(empty.to_string_lossy().into_owned()).unwrap_err();
    assert!(matches!(e, VizError::NotFound { .. }), "{e:?}");
}

#[test]
fn open_corrupt_json_is_a_model_error() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("bad.vizdaw");
    fs::create_dir(&p).unwrap();
    fs::write(p.join("project.json"), "{ not json").unwrap();
    let e = Session::open(p.to_string_lossy().into_owned()).unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
}

#[test]
fn open_newer_format_names_the_version() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("new.vizdaw");
    fs::create_dir(&p).unwrap();
    fs::write(
        p.join("project.json"),
        r#"{"format":"vizdaw-project","version":999,"document":{}}"#,
    )
    .unwrap();
    let e = Session::open(p.to_string_lossy().into_owned()).unwrap_err();
    match e {
        VizError::Model { message } => assert!(message.contains("999"), "{message}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn save_into_an_unwritable_place_is_an_io_error() {
    let dir = tempfile::tempdir().unwrap();
    // A regular file where the package's parent directory should be.
    let blocker = dir.path().join("file");
    fs::write(&blocker, "x").unwrap();
    let s = Session::new_empty().unwrap();
    let e = s
        .save_as(blocker.join("p.vizdaw").to_string_lossy().into_owned())
        .unwrap_err();
    assert!(matches!(e, VizError::Io { .. }), "{e:?}");
    assert_eq!(s.path().unwrap(), None);
}

#[test]
fn opens_the_checked_in_demo_fixture() {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/demo.vizdaw");
    let s = Session::open(p.to_string()).unwrap();
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.title, "demo");
    assert_eq!(snap.sequences.len(), 3);
    assert!(!snap.dirty);
}

/// The project ID in the package at `p`.
fn project_id(p: &std::path::Path) -> Option<String> {
    core_model::file::load(p)
        .unwrap()
        .settings()
        .project_id
        .clone()
}

#[test]
fn save_stamps_a_project_id_and_save_as_keeps_it() {
    vizdaw_ffi::host::force_offline();
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.vizdaw");
    let s = Session::new_empty().unwrap();
    s.save_as(a.to_string_lossy().into_owned()).unwrap();
    let id = project_id(&a).expect("stamped on the first save");
    assert!(core_model::document::check_project_id(&id), "{id}");
    assert!(!s.snapshot().unwrap().dirty);
    // Save As is the same song: the same ID (Ruling T1-a).
    let b = dir.path().join("b.vizdaw");
    s.save_as(b.to_string_lossy().into_owned()).unwrap();
    assert_eq!(project_id(&b).as_deref(), Some(id.as_str()));
    s.save().unwrap();
    assert_eq!(project_id(&b).as_deref(), Some(id.as_str()));
    s.close().unwrap();
    let again = Session::open(b.to_string_lossy().into_owned()).unwrap();
    again.save().unwrap();
    assert_eq!(project_id(&b).as_deref(), Some(id.as_str()));
    again.close().unwrap();

    // Another project gets another ID.
    let c = dir.path().join("c.vizdaw");
    let other = Session::new_empty().unwrap();
    other.save_as(c.to_string_lossy().into_owned()).unwrap();
    assert_ne!(project_id(&c), Some(id.clone()));
    other.close().unwrap();

    // A version-7 project gets one on its next save.
    let old = dir.path().join("old.vizdaw");
    fs::create_dir(&old).unwrap();
    let json = core_model::file::to_json(&core_model::document::Document::new()).unwrap();
    let v7 = json.replacen(
        &format!("\"version\": {}", core_model::file::VERSION),
        "\"version\": 7",
        1,
    );
    assert_ne!(v7, json, "the version was rewritten");
    fs::write(old.join("project.json"), v7).unwrap();
    assert_eq!(project_id(&old), None);
    let s = Session::open(old.to_string_lossy().into_owned()).unwrap();
    s.save().unwrap();
    let stamped = project_id(&old).expect("stamped");
    assert!(core_model::document::check_project_id(&stamped));
    s.close().unwrap();

    // A save while an edit is open skips the stamp; the next save stamps.
    let d = dir.path().join("d.vizdaw");
    let s = Session::new_empty().unwrap();
    s.begin_edit("Drag".into()).unwrap();
    s.save_as(d.to_string_lossy().into_owned()).unwrap();
    assert_eq!(project_id(&d), None);
    s.end_edit().unwrap();
    s.save().unwrap();
    assert!(project_id(&d).is_some_and(|i| core_model::document::check_project_id(&i)));
    s.close().unwrap();
}
