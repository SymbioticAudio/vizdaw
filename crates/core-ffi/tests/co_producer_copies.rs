//! Co-Producer's saved copies through the session (plan 8a Task 3, Ruling
//! 9): Save's retention keeps the media a copy names, a damaged copy stops
//! the pass, a copy opens only as a new untitled project, and Settings can
//! count them.

mod common;

use core_media::caf::{CafWriter, WriteMode};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::jobs::ImportTargetRow;
use vizdaw_ffi::record::set_free_bytes;
use vizdaw_ffi::{JobStateRow, Session, VizError};

const STAMP: &str = "2026-11-14 15-02";
const COPY_LINE: &str =
    "Unused media files were not moved to the Trash: a Co-Producer copy could not be read.";
const AUTOSAVE_LINE: &str = "Unused media files were not moved to the Trash: the saved project or one of its autosaves could not be read.";
const NOT_A_PROJECT: &str =
    "That is a Co-Producer copy: open it with File › Open Co-Producer Copy….";

const SAVE_OVER_COPY: &str = "That is a Co-Producer copy: Save As cannot replace it.";

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    set_free_bytes(None);
    g
}

fn caf(path: &Path, frames: usize) -> PathBuf {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![0.25; frames * 2]).unwrap();
    w.finish().unwrap();
    path.to_path_buf()
}

fn first_seq(s: &Session) -> u64 {
    s.snapshot().unwrap().sequences[0].id
}

/// Imports `path` onto a new track of the first sequence.
fn import(s: &Arc<Session>, path: &Path) {
    let id = s
        .import_audio(
            vec![path.display().to_string()],
            Some(ImportTargetRow {
                seq: first_seq(s),
                lane: None,
                tick: 0,
            }),
        )
        .unwrap();
    let until = Instant::now() + common::wait_limit();
    loop {
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            assert_eq!(j.state, JobStateRow::Done, "{j:?}");
            return;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A saved package `P.vizdaw` with one imported track, and one copy
/// ("Tidy") written while the track was there; then the track is deleted,
/// the media compacted and the package saved, so only the history (gone
/// once closed) and the copy name the file. Returns the package, the copy
/// and the media file's package-relative path.
fn set_up(dir: &Path) -> (PathBuf, PathBuf, String) {
    let s = Session::new_empty().unwrap();
    s.debug_set_trash_dir(Some(dir.join("Trash").display().to_string()));
    import(&s, &caf(&dir.join("src/kick.caf"), 200));
    let pkg = dir.join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let rel = s.media().unwrap()[0].path.clone();
    let copy = s.debug_write_co_producer_copy(STAMP, "Tidy").unwrap();
    assert_eq!(
        copy,
        pkg.join("Co-Producer/Copies/2026-11-14 15-02 before Tidy.vizdaw")
    );
    let seq = first_seq(&s);
    let lane = s.lanes(seq).unwrap()[0].id;
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Remove { seq, lane },
        },
        None,
    )
    .unwrap();
    s.compact_media().unwrap();
    assert!(s.media().unwrap().is_empty());
    s.save().unwrap();
    s.close().unwrap();
    drop(s);
    (pkg, copy, rel)
}

/// Opens `pkg` in a fresh session (its history names no media) with the
/// trash folder `trash`, and saves it.
fn reopen_and_save(pkg: &Path, trash: &Path) -> (Arc<Session>, Vec<String>) {
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(trash.display().to_string()));
    s.save().unwrap();
    let problems = s.idle().unwrap().problems;
    (s, problems)
}

fn trash_is_empty(trash: &Path) -> bool {
    std::fs::read_dir(trash).map_or(true, |mut d| d.next().is_none())
}

#[test]
fn retention_and_prune_keep_what_copies_use() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (pkg, copy, rel) = set_up(dir.path());
    let trash = dir.path().join("Trash2");
    let (s, problems) = reopen_and_save(&pkg, &trash);
    assert!(pkg.join(&rel).is_file(), "the copy still uses {rel}");
    assert!(trash_is_empty(&trash));
    assert!(problems.is_empty(), "{problems:?}");
    s.close().unwrap();
    drop(s);

    let c = Session::open_copy(copy.display().to_string()).unwrap();
    let snap = c.snapshot().unwrap();
    assert_eq!(snap.title, "Untitled");
    let seq = snap.sequences[0].id;
    let lanes = c.lanes(seq).unwrap();
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].region_count, 1);
    let media = c.media().unwrap();
    assert_eq!(media.len(), 1);
    let file = PathBuf::from(c.media_file(media[0].id).unwrap());
    assert!(file.is_file(), "{}", file.display());
    assert!(!file.starts_with(&pkg), "copied into the scratch root");
    assert!(c.idle().unwrap().problems.is_empty());
    c.close().unwrap();
}

#[test]
fn a_damaged_copy_stops_trashing() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (pkg, _copy, rel) = set_up(dir.path());
    let junk = pkg.join("Co-Producer/Copies/junk.vizdaw");
    std::fs::create_dir_all(&junk).unwrap();
    std::fs::write(junk.join("project.json"), "junk").unwrap();
    // A file nothing names: with every copy readable it would go.
    caf(&pkg.join("Media/stray.caf"), 10);
    let trash = dir.path().join("Trash2");
    let (s, problems) = reopen_and_save(&pkg, &trash);
    assert!(pkg.join(&rel).is_file());
    assert!(pkg.join("Media/stray.caf").is_file());
    assert!(trash_is_empty(&trash));
    assert!(problems.iter().any(|p| p == COPY_LINE), "{problems:?}");
    assert!(!problems.iter().any(|p| p == AUTOSAVE_LINE), "{problems:?}");
    s.close().unwrap();
}

#[test]
fn open_copy_refuses_what_is_not_a_copy() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_empty().unwrap();
    let pkg = dir.path().join("Q.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.close().unwrap();
    let missing = pkg.join("Co-Producer/Copies/none.vizdaw");
    let long = format!("/{}", "a".repeat(70_000));
    let nul = format!("{}/Co-Producer/Copies/a\0b.vizdaw", pkg.display());
    for p in [
        pkg.display().to_string(),
        missing.display().to_string(),
        long,
        nul,
    ] {
        assert!(Session::open_copy(p.clone()).is_err(), "{:.80}", p);
    }
}

#[test]
fn a_copy_never_opens_as_a_project() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_empty().unwrap();
    let pkg = dir.path().join("Q.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let copy = s.debug_write_co_producer_copy(STAMP, "Tidy").unwrap();
    s.close().unwrap();
    drop(s);
    let before = std::fs::read(copy.join("project.json")).unwrap();
    let e = Session::open(copy.display().to_string()).unwrap_err();
    assert_eq!(e.to_string(), NOT_A_PROJECT);
    assert_eq!(std::fs::read(copy.join("project.json")).unwrap(), before);
}

#[test]
fn copies_size_counts_them() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_empty().unwrap();
    let pkg = dir.path().join("Q.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let none = s.co_producer_copies_size().unwrap();
    assert_eq!((none.count, none.bytes), (0, 0));
    let a = s.debug_write_co_producer_copy(STAMP, "One").unwrap();
    let b = s.debug_write_co_producer_copy(STAMP, "Two").unwrap();
    let size = |p: &Path| std::fs::metadata(p.join("project.json")).unwrap().len();
    let row = s.co_producer_copies_size().unwrap();
    assert_eq!(row.count, 2);
    assert_eq!(row.bytes, size(&a) + size(&b));
    s.close().unwrap();
}

/// No route saves over a copy (Ruling 9): Save As to a copy's path is
/// refused and leaves its bytes.
#[test]
fn save_as_never_writes_over_a_copy() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_empty().unwrap();
    let pkg = dir.path().join("Q.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let copy = s.debug_write_co_producer_copy(STAMP, "Tidy").unwrap();
    let before = std::fs::read(copy.join("project.json")).unwrap();
    s.add_bus("Bus".into()).unwrap();
    let other = Session::new_empty().unwrap();
    for target in [&s, &other] {
        let e = target.save_as(copy.display().to_string()).unwrap_err();
        assert_eq!(e.to_string(), SAVE_OVER_COPY);
    }
    assert_eq!(std::fs::read(copy.join("project.json")).unwrap(), before);
    other.close().unwrap();
    s.close().unwrap();
}

/// An untitled song: one imported track, its own trash folder.
fn untitled(dir: &Path) -> Arc<Session> {
    let s = Session::new_empty().unwrap();
    s.debug_set_trash_dir(Some(dir.join("Trash").display().to_string()));
    import(&s, &caf(&dir.join("src/kick.caf"), 200));
    s
}

#[cfg(unix)]
fn inode(p: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).unwrap().ino()
}

/// Fix round 1 (I-1): an untitled song's copy holds its own media, so
/// Save As and Close (which delete the untitled scratch folder) leave the
/// copy whole; the copy step only reads the scratch folder.
#[test]
fn an_untitled_copy_keeps_its_media() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = untitled(dir.path());
    let item = s.media().unwrap()[0].clone();
    let scratch = PathBuf::from(s.media_file(item.id).unwrap());
    let before = std::fs::read(&scratch).unwrap();
    let copy = s.debug_write_co_producer_copy(STAMP, "Tidy").unwrap();
    let own = copy.join(&item.path);
    assert!(own.is_file(), "{}", own.display());
    assert_eq!(std::fs::read(&own).unwrap(), before);
    assert!(scratch.is_file(), "the scratch file stays");
    assert_eq!(std::fs::read(&scratch).unwrap(), before);
    #[cfg(unix)]
    assert_ne!(inode(&own), inode(&scratch), "a copy, never the same file");
    assert_eq!(
        s.media().unwrap()[0].path,
        item.path,
        "the song is unchanged"
    );
    s.save_as(dir.path().join("Song.vizdaw").display().to_string())
        .unwrap();
    s.close().unwrap();
    drop(s);
    assert!(!scratch.exists(), "Save As removed the scratch folder");
    assert!(own.is_file());

    let c = Session::open_copy(copy.display().to_string()).unwrap();
    let media = c.media().unwrap();
    assert_eq!(media.len(), 1);
    assert_eq!(media[0].offline, None, "it plays");
    assert!(PathBuf::from(c.media_file(media[0].id).unwrap()).is_file());
    let problems = c.idle().unwrap().problems;
    assert!(problems.is_empty(), "{problems:?}");
    c.close().unwrap();
}

/// Fix round 1 (I-1): the copy's media are charged against free space
/// first; without room nothing is written and no project ID is stamped.
#[test]
fn an_untitled_copy_needs_room_for_its_media() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = untitled(dir.path());
    set_free_bytes(Some(10));
    let e = s.debug_write_co_producer_copy(STAMP, "Tidy").unwrap_err();
    set_free_bytes(None);
    assert!(e.to_string().contains("free space"), "{e}");
    assert_eq!(s.project_id().unwrap(), None);
    assert_eq!(s.co_producer_copies_size().unwrap().count, 0);
    s.close().unwrap();
}

/// Fix round 1 (I-2): a copy captures live plug-in states first; a Busy
/// capture writes nothing and stamps no project ID.
#[test]
fn a_busy_capture_writes_no_copy() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = untitled(dir.path());
    s.debug_capture_busy(true);
    let e = s.debug_write_co_producer_copy(STAMP, "Tidy").unwrap_err();
    assert!(matches!(e, VizError::Busy { .. }), "{e:?}");
    assert_eq!(s.project_id().unwrap(), None);
    let pkg = dir.path().join("Q.vizdaw");
    s.debug_capture_busy(false);
    s.save_as(pkg.display().to_string()).unwrap();
    s.debug_capture_busy(true);
    assert!(matches!(
        s.debug_write_co_producer_copy(STAMP, "Tidy"),
        Err(VizError::Busy { .. })
    ));
    assert!(core_model::file::list_copies(&pkg).unwrap().is_empty());
    s.debug_capture_busy(false);
    s.close().unwrap();
}
