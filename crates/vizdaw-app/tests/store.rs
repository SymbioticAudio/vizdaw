mod support;

use std::path::Path;
use std::sync::Arc;

use support::{FakeSession, demo_fake};
use vizdaw_app::{AlertItem, DocumentStore, WindowToken};
use vizdaw_ffi::VizError;
use vizdaw_ffi::session::Session;

fn store() -> DocumentStore {
    DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(demo_fake()))),
        Box::new(|path| {
            if path.ends_with("missing.vizdaw") {
                Err(VizError::NotFound { path: path.into() })
            } else {
                Ok(Arc::new(demo_fake()))
            }
        }),
    )
}

#[test]
fn creates_documents_per_window() {
    let mut s = store();
    let a = s.new_empty().unwrap();
    let b = s.new_demo().unwrap();
    assert_ne!(a, b);
    assert_eq!(s.document(b).unwrap().snapshot().sequences.len(), 3);
    s.close(a);
    assert!(s.document(a).is_none());
}

#[test]
fn the_front_document_is_the_last_key_one_still_open() {
    let mut s = store();
    let a = s.new_demo().unwrap();
    let b = s.new_demo().unwrap();
    assert_eq!(s.front(), None, "no window was key yet");
    s.focus_document(a, WindowToken(1));
    s.focus_document(b, WindowToken(2));
    assert_eq!(s.front(), Some(b));
    s.close(b);
    assert_eq!(s.front(), Some(a));
}

#[test]
fn open_refuses_folders_without_the_extension() {
    let mut s = store();
    assert!(s.open(Path::new("/Users/me/Music")).is_err());
    assert_eq!(
        s.alert().unwrap().message,
        "Music is not a .vizdaw project."
    );
}

#[test]
fn open_missing_project_shows_the_error() {
    let mut s = store();
    assert!(s.open(Path::new("/x/missing.vizdaw")).is_err());
    assert_eq!(
        s.alert().unwrap().message,
        "No VizDAW project at /x/missing.vizdaw."
    );
}

#[test]
fn take_alert_clears_the_store_alert() {
    let mut s = store();
    let _ = s.open(Path::new("/Users/me/Music"));
    let a = s.take_alert();
    assert_eq!(a.unwrap().title, "Open Failed");
    assert!(s.alert().is_none());
    assert!(s.take_alert().is_none());
}

#[test]
fn posted_alerts_queue_behind_the_one_showing() {
    let mut s = store();
    let _ = s.open(Path::new("/Users/me/Music"));
    s.post_alert(AlertItem::new(
        "Recordings Moved to the Trash",
        "1 recording",
    ));
    s.post_alert(AlertItem::new("Third", "3"));
    assert_eq!(s.take_alert().unwrap().title, "Open Failed");
    assert_eq!(s.alert().unwrap().title, "Recordings Moved to the Trash");
    assert_eq!(s.take_alert().unwrap().message, "1 recording");
    assert_eq!(s.take_alert().unwrap().title, "Third");
    assert!(s.take_alert().is_none());
    s.post_alert(AlertItem::new("Alone", ""));
    assert_eq!(s.alert().unwrap().title, "Alone");
}

#[test]
fn open_failures_queue_behind_the_one_showing() {
    let mut s = store();
    s.post_alert(AlertItem::new(
        "Recordings Moved to the Trash",
        "1 recording",
    ));
    let _ = s.open(Path::new("/Users/me/Music"));
    let _ = s.open(Path::new("/Users/me/missing.vizdaw"));
    assert_eq!(
        s.take_alert().unwrap().title,
        "Recordings Moved to the Trash"
    );
    assert!(
        s.take_alert()
            .unwrap()
            .message
            .contains("not a .vizdaw project")
    );
    assert_eq!(s.take_alert().unwrap().title, "Open Failed");
    assert!(s.take_alert().is_none());
}

#[test]
fn saved_project_reopens_with_the_same_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("roundtrip.vizdaw");
    let session = Session::new_demo().unwrap();
    session
        .save_as(path.to_string_lossy().into_owned())
        .unwrap();
    let saved = session.snapshot().unwrap();
    let lanes_before: std::collections::HashMap<u64, _> = saved
        .sequences
        .iter()
        .map(|q| (q.id, session.lanes(q.id).unwrap()))
        .collect();
    assert!(lanes_before.values().any(|l| !l.is_empty()));
    session.close().unwrap();
    let reopened = Session::open(path.to_string_lossy().into_owned()).unwrap();
    let again = reopened.snapshot().unwrap();
    assert_eq!(again.sequences, saved.sequences);
    assert!(!again.sequences.is_empty());
    assert_eq!(again.title, "roundtrip");
    for seq in &saved.sequences {
        assert_eq!(
            reopened.lanes(seq.id).unwrap(),
            lanes_before[&seq.id],
            "lanes of sequence {}",
            seq.id
        );
    }
    reopened.close().unwrap();
}

#[test]
fn opening_an_open_project_again_answers_the_same_document() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("a.vizdaw");
    std::fs::create_dir(&pkg).unwrap();
    let real = std::fs::canonicalize(&pkg).unwrap();
    let mut s = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(demo_fake()))),
        Box::new(|path| {
            let f = demo_fake();
            f.set_path(Some(path));
            Ok(Arc::new(f))
        }),
    );
    let a = s.open(&real).unwrap();
    // `.` and a trailing slash name the same package.
    let indirect = dir.path().join(".").join("a.vizdaw").join("");
    assert_eq!(s.open(&indirect).unwrap(), a);
    assert_eq!(s.count(), 1);
}
