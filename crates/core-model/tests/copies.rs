//! Co-Producer's saved copies (plan 8a Task 3, Ruling 9): a copy is new,
//! never replaces, has a safe name, and Save's pruning keeps every plug-in
//! state a copy names.

use core_model::command::{Command, RoutingCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::error::ModelError;
use core_model::file::{
    COPIES_DIR, copy_name, copy_parent, list_copies, load_copy, save, write_copy,
};
use core_model::ids::PluginInstanceId;
use core_model::state::{blob_file_name, capture};
use std::path::{Path, PathBuf};

const STAMP: &str = "2026-11-14 15-02";

fn temp_package(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let p = std::env::temp_dir().join(format!(
        "vizdaw-copies-{tag}-{}-{nanos}.vizdaw",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    p
}

/// One plugin instrument (instance 0).
fn pad() -> Document {
    apply(
        &Document::new(),
        &Command::Routing(RoutingCmd::AddDestination {
            name: "Pad".into(),
            kind: DestKind::plugin("au:pad"),
            default_patch: None,
        }),
    )
    .unwrap()
    .doc
}

#[test]
fn a_copy_is_new_and_never_replaces() {
    let pkg = temp_package("new");
    let (doc, _) = capture(&pad(), PluginInstanceId(0), b"state").unwrap();
    save(&doc, &pkg).unwrap();
    let first = write_copy(&doc, &pkg, STAMP, "Set Up a Drum Bus").unwrap();
    assert_eq!(
        first,
        pkg.join(COPIES_DIR)
            .join("2026-11-14 15-02 before Set Up a Drum Bus.vizdaw")
    );
    let pj = first.join("project.json");
    assert!(pj.is_file());
    assert_eq!(load_copy(&first).unwrap(), doc);
    let bytes = std::fs::read(&pj).unwrap();
    let (other, _) = capture(&doc, PluginInstanceId(0), b"other").unwrap();
    let second = write_copy(&other, &pkg, STAMP, "Set Up a Drum Bus").unwrap();
    assert_eq!(
        second,
        pkg.join(COPIES_DIR)
            .join("2026-11-14 15-02 before Set Up a Drum Bus 2.vizdaw")
    );
    assert_eq!(std::fs::read(&pj).unwrap(), bytes, "the first is unchanged");
    let mut both = vec![pj, second.join("project.json")];
    both.sort();
    assert_eq!(list_copies(&pkg).unwrap(), both);
    std::fs::remove_dir_all(&pkg).unwrap();
}

#[test]
fn copy_names_are_safe() {
    let dir = temp_package("names");
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(
        copy_name(&dir, STAMP, "a/b:c").unwrap(),
        "2026-11-14 15-02 before a-b-c.vizdaw"
    );
    assert_eq!(
        copy_name(&dir, STAMP, ".hidden").unwrap(),
        "2026-11-14 15-02 before hidden.vizdaw"
    );
    for bad in ["2026-11-14", "2026-11-14 15:02", "x", "2026-11-14 15-02/.."] {
        assert!(
            matches!(copy_name(&dir, bad, "t"), Err(ModelError::Invalid { .. })),
            "{bad}"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn copy_parent_finds_the_package() {
    assert_eq!(
        copy_parent(Path::new("/x/Song.vizdaw/Co-Producer/Copies/a.vizdaw")),
        Some(PathBuf::from("/x/Song.vizdaw"))
    );
    for none in [
        "/x/Song.vizdaw",
        "/x/Copies/a.vizdaw",
        "/x/Song.vizdaw/Co-Producer/Copies/a",
    ] {
        assert_eq!(copy_parent(Path::new(none)), None, "{none}");
    }
}

#[test]
fn prune_keeps_blobs_copies_use() {
    let pkg = temp_package("prune");
    let (d1, old) = capture(&pad(), PluginInstanceId(0), b"old state").unwrap();
    save(&d1, &pkg).unwrap();
    let copy = write_copy(&d1, &pkg, STAMP, "Tidy").unwrap();
    let (d2, new) = capture(&d1, PluginInstanceId(0), b"new state").unwrap();
    assert_ne!(old, new);
    save(&d2, &pkg).unwrap();
    let blobs = pkg.join("PluginState");
    assert!(
        blobs.join(blob_file_name(old)).is_file(),
        "the copy uses it"
    );
    assert!(blobs.join(blob_file_name(new)).is_file());
    assert_eq!(
        load_copy(&copy).unwrap().state_blob(old),
        Some(&b"old state"[..])
    );
    std::fs::remove_dir_all(&pkg).unwrap();
}

#[test]
fn a_damaged_copy_stops_pruning() {
    let pkg = temp_package("damaged");
    let (d1, old) = capture(&pad(), PluginInstanceId(0), b"old state").unwrap();
    save(&d1, &pkg).unwrap();
    let bad = pkg.join(COPIES_DIR).join("bad.vizdaw");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("project.json"), "{").unwrap();
    let (d2, new) = capture(&d1, PluginInstanceId(0), b"new state").unwrap();
    save(&d2, &pkg).unwrap();
    let blobs = pkg.join("PluginState");
    assert!(blobs.join(blob_file_name(old)).is_file(), "nothing pruned");
    assert!(blobs.join(blob_file_name(new)).is_file());
    std::fs::remove_dir_all(&pkg).unwrap();
}

/// A `Co-Producer` or `Copies` folder that is a link is never written
/// through: the copy is refused and nothing lands where the link points
/// (release gate Task 5).
#[test]
fn symlinked_copies_folder_is_refused() {
    use core_model::file::refuse_linked_folders;
    let doc = pad();
    for linked in ["Co-Producer", "Co-Producer/Copies"] {
        let pkg = temp_package("linked");
        save(&doc, &pkg).unwrap();
        let elsewhere = temp_package("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let at = pkg.join(linked);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &at).unwrap();
        let e = write_copy(&doc, &pkg, STAMP, "Tidy Up").unwrap_err();
        assert!(e.to_string().contains("is a link"), "{linked}: {e}");
        assert_eq!(
            std::fs::read_dir(&elsewhere).unwrap().count(),
            0,
            "{linked}: written through the link"
        );
        assert!(matches!(
            refuse_linked_folders(&pkg, Path::new(COPIES_DIR)),
            Err(ModelError::Invalid { .. })
        ));
        let _ = std::fs::remove_dir_all(&pkg);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }
    // A file where a folder goes is refused too; folders not made yet
    // are fine.
    let pkg = temp_package("file");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("Co-Producer"), b"x").unwrap();
    assert!(refuse_linked_folders(&pkg, Path::new(COPIES_DIR)).is_err());
    std::fs::remove_file(pkg.join("Co-Producer")).unwrap();
    assert!(refuse_linked_folders(&pkg, Path::new(COPIES_DIR)).is_ok());
    let _ = std::fs::remove_dir_all(&pkg);
}
