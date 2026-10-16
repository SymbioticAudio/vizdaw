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
