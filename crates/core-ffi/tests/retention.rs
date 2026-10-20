//! The retention rule (plan 5b Task 7, Ruling 11): on save and on close,
//! media files nothing can bring back go to the Trash; everything an undo
//! or redo state, the saved project or an autosave names stays.

use core_media::caf::{CafWriter, WriteMode, hash_file};
use core_model::command::{Command, MediaCmd, apply};
use core_model::format::BitDepth;
use core_model::history::Timestamp;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vizdaw_ffi::{JobStateRow, Session};

fn caf(path: &Path, frames: usize, value: f32) -> PathBuf {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![value; frames * 2]).unwrap();
    w.finish().unwrap();
    path.to_path_buf()
}

fn import(s: &Arc<Session>, path: &Path) {
    let id = s
        .import_audio(vec![path.display().to_string()], None)
        .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
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

/// An untitled session with its own trash folder, saved as `P.vizdaw`
/// after importing `files` (named after their stems).
fn saved(dir: &Path, files: &[&str]) -> (Arc<Session>, PathBuf, PathBuf) {
    let s = Session::new_empty().unwrap();
    let trash = dir.join("Trash");
    s.debug_set_trash_dir(Some(trash.display().to_string()));
    for (k, f) in files.iter().enumerate() {
        import(&s, &caf(&dir.join("src").join(f), 100 + k, 0.25));
    }
    let pkg = dir.join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    (s, pkg, trash)
}

fn problems(s: &Session) -> Vec<String> {
    s.idle().unwrap().problems
}

#[test]
fn save_trashes_what_nothing_references() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, trash) = saved(dir.path(), &["a.caf", "b.caf"]);
    assert!(!trash.exists(), "nothing to tidy after Save As");
    // Strays, a hidden file, an import leftover and an orphan overview.
    caf(&pkg.join("Media/stray.caf"), 10, 0.5);
    caf(&pkg.join("Media/Sub/old.caf"), 10, 0.5);
    std::fs::write(pkg.join("Media/.keep"), "x").unwrap();
    caf(&pkg.join("Media/.incoming/left.caf"), 10, 0.5);
    let orphan = pkg
        .join("Media/.overviews")
        .join(format!("{}.vzov", "0".repeat(64)));
    std::fs::write(&orphan, "x").unwrap();
    let stray_hash = hash_file(&pkg.join("Media/stray.caf")).unwrap();
    s.save().unwrap();
    assert!(!pkg.join("Media/stray.caf").exists());
    assert!(!pkg.join("Media/Sub/old.caf").exists());
    assert_eq!(
        hash_file(&trash.join("stray.caf")).unwrap(),
        stray_hash,
        "moved, not unlinked"
    );
    assert!(trash.join("old.caf").is_file());
    assert!(
        pkg.join("Media/.keep").is_file(),
        "hidden files are not media"
    );
    assert!(!pkg.join("Media/.incoming/left.caf").exists());
    assert!(!orphan.exists());
    // The items' files and overviews stay.
    for r in s.media().unwrap() {
        assert!(pkg.join(&r.path).is_file(), "{}", r.path);
    }
    let kept = std::fs::read_dir(pkg.join("Media/.overviews"))
        .unwrap()
        .count();
    assert_eq!(kept, 2);
    assert!(problems(&s).is_empty());
    s.close().unwrap();
}

/// Final review T7: an overview write a crash interrupted leaves a
/// `.<hash>.vzov.<pid>-<n>.tmp`; retention deletes it once it is an hour
/// old, never a write that may still be in progress.
#[test]
fn save_deletes_stale_overview_temporaries() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, _trash) = saved(dir.path(), &["a.caf"]);
    let ovs = pkg.join("Media/.overviews");
    let stale = ovs.join(format!(".{}.vzov.1-0.tmp", "0".repeat(64)));
    let fresh = ovs.join(format!(".{}.vzov.1-1.tmp", "1".repeat(64)));
    for f in [&stale, &fresh] {
        std::fs::write(f, "x").unwrap();
    }
    let old = std::time::SystemTime::now() - Duration::from_secs(2 * 60 * 60);
    std::fs::File::options()
        .write(true)
        .open(&stale)
        .unwrap()
        .set_modified(old)
        .unwrap();
    s.save().unwrap();
    assert!(!stale.exists(), "a stale temporary is deleted");
    assert!(fresh.is_file(), "a recent one may still be renamed");
    assert!(problems(&s).is_empty());
    s.close().unwrap();
}

/// Review Focus 1.
#[test]
fn retention_never_trashes_what_undo_redo_or_disk_documents_reference() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, trash) = saved(dir.path(), &["a.caf"]);
    // B imported, then undone: only the redo history names it.
    import(&s, &caf(&dir.path().join("src/b.caf"), 50, 0.5));
    s.undo().unwrap();
    s.save().unwrap();
    assert!(pkg.join("Media/b.caf").is_file(), "redo can bring B back");
    // Preflight M1: inside an open drag the redo history is gone from
    // the present, but aborting the drag restores it: only the
    // transaction's base names B, and B stays.
    s.begin_edit("Drag".into()).unwrap();
    s.add_bus("Bus".into()).unwrap();
    s.save().unwrap();
    assert!(
        pkg.join("Media/b.caf").is_file(),
        "the open transaction's base can bring B back"
    );
    s.end_edit().unwrap();
    // A new edit drops the redo history: now nothing names B.
    let a = s.media().unwrap()[0].id;
    s.rename_media(a, "Drums".into()).unwrap();
    s.save().unwrap();
    assert!(!pkg.join("Media/b.caf").exists());
    assert!(trash.join("b.caf").is_file());
    // Compacting A (unused) keeps its file: undo can bring it back.
    s.compact_media().unwrap();
    s.save().unwrap();
    assert!(pkg.join("Media/a.caf").is_file());
    s.close().unwrap();
    // Files only the saved project or an autosave name stay too.
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(trash.display().to_string()));
    let doc = core_model::file::load(&pkg).unwrap();
    let with = |path: &str| {
        apply(
            &doc,
            &Command::Media(MediaCmd::Add {
                path: path.into(),
                sample_rate: 48_000,
                channels: 2,
                length_frames: 10,
                content_hash: None,
                name: None,
                origin: None,
            }),
        )
        .unwrap()
        .doc
    };
    caf(&pkg.join("Media/disk.caf"), 10, 0.5);
    caf(&pkg.join("Media/auto.caf"), 10, 0.5);
    core_model::file::save(&with("Media/disk.caf"), &pkg).unwrap();
    core_model::file::autosave(&with("Media/auto.caf"), &pkg, Timestamp(1)).unwrap();
    s.close().unwrap();
    assert!(
        pkg.join("Media/disk.caf").is_file(),
        "the saved project names it"
    );
    assert!(pkg.join("Media/auto.caf").is_file(), "an autosave names it");
}

/// Review Focus 1.
#[test]
fn an_unreadable_autosave_trashes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, _trash) = saved(dir.path(), &["a.caf"]);
    caf(&pkg.join("Media/stray.caf"), 10, 0.5);
    std::fs::write(
        pkg.join("Autosave/project-00000000000000000001.json"),
        "{ not json",
    )
    .unwrap();
    s.save().unwrap();
    assert!(pkg.join("Media/stray.caf").is_file());
    assert!(
        problems(&s).iter().any(|p| p.contains("could not be read")),
        "the user is told why"
    );
    s.close().unwrap();
}

/// Review Focus 1.
#[test]
fn a_failed_trash_keeps_the_file_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, _trash) = saved(dir.path(), &["a.caf"]);
    // The "trash folder" is a file: nothing can move into it.
    let blocked = dir.path().join("blocked");
    std::fs::write(&blocked, "x").unwrap();
    s.debug_set_trash_dir(Some(blocked.display().to_string()));
    caf(&pkg.join("Media/stray.caf"), 10, 0.5);
    s.save().unwrap();
    assert!(pkg.join("Media/stray.caf").is_file());
    let p = problems(&s);
    assert!(
        p.iter()
            .any(|l| l.starts_with("Media/stray.caf: not moved to the Trash")),
        "{p:?}"
    );
    s.close().unwrap();
}

#[test]
fn close_tidies_and_an_unapplied_job_holds_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, trash) = saved(dir.path(), &["a.caf"]);
    caf(&pkg.join("Media/stray.caf"), 10, 0.5);
    // A job not applied yet: its new file is not in the history.
    s.debug_hold_jobs(true).unwrap();
    let id = s
        .import_audio(
            vec![
                caf(&dir.path().join("src/c.caf"), 20, 0.5)
                    .display()
                    .to_string(),
            ],
            None,
        )
        .unwrap();
    s.save().unwrap();
    assert!(
        pkg.join("Media/stray.caf").is_file(),
        "held back while a job runs"
    );
    s.debug_hold_jobs(false).unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while s.job(id).unwrap().state == JobStateRow::Running {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    caf(&pkg.join("Media/late.caf"), 10, 0.5);
    s.close().unwrap();
    assert!(!pkg.join("Media/stray.caf").exists() && !pkg.join("Media/late.caf").exists());
    assert!(trash.join("stray.caf").is_file() && trash.join("late.caf").is_file());
    assert!(
        pkg.join("Media/c.caf").is_file(),
        "the applied import's file stays"
    );
}

#[test]
fn tests_never_reach_the_real_trash() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_empty().unwrap();
    import(&s, &caf(&dir.path().join("src/a.caf"), 100, 0.25));
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    caf(&pkg.join("Media/stray.caf"), 10, 0.5);
    s.save().unwrap();
    assert!(pkg.join("Media/stray.caf").is_file());
    assert!(
        problems(&s)
            .iter()
            .any(|p| p.contains("no trash folder in tests"))
    );
    s.close().unwrap();
}

/// Final review r2, after the Trash: a freed name may be reused, and every
/// path the history names still holds the audio it recorded.
#[test]
fn after_the_trash_every_history_path_names_one_audio() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, trash) = saved(dir.path(), &["x.caf"]);
    caf(&pkg.join("Media/y.caf"), 10, 0.5);
    s.save().unwrap();
    assert!(trash.join("y.caf").is_file());
    import(&s, &caf(&dir.path().join("src2/y.caf"), 30, 0.75));
    let rows = s.media().unwrap();
    assert_eq!(rows[1].path, "Media/y.caf", "the freed name is free");
    s.save().unwrap();
    for r in rows {
        let file = pkg.join(&r.path);
        assert!(file.is_file(), "{}", r.path);
    }
    assert_ne!(
        hash_file(&trash.join("y.caf")).unwrap(),
        hash_file(&pkg.join("Media/y.caf")).unwrap()
    );
    s.close().unwrap();
}

/// True when the volume holding `dir` resolves names without case.
fn case_insensitive(dir: &Path) -> bool {
    let probe = dir.join("CaseProbe");
    std::fs::write(&probe, "x").unwrap();
    let insensitive = dir.join("caseprobe").exists();
    std::fs::remove_file(&probe).unwrap();
    insensitive
}

/// Preflight I2: a file renamed on disk to another case still is the
/// item's file (the volume resolves it), so Save keeps it, matched by
/// its (device, inode) rather than its spelling.
#[test]
fn a_file_renamed_to_another_case_stays() {
    let dir = tempfile::tempdir().unwrap();
    if !case_insensitive(dir.path()) {
        eprintln!("skipped: the temp volume is case-sensitive");
        return;
    }
    let (s, pkg, trash) = saved(dir.path(), &["kick.caf"]);
    assert_eq!(s.media().unwrap()[0].path, "Media/kick.caf");
    std::fs::rename(pkg.join("Media/kick.caf"), pkg.join("Media/Kick.caf")).unwrap();
    let names: Vec<String> = std::fs::read_dir(pkg.join("Media"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"Kick.caf".to_string()), "{names:?}");
    s.recheck_media().unwrap();
    s.save().unwrap();
    assert!(pkg.join("Media/Kick.caf").is_file());
    assert!(!trash.join("Kick.caf").exists());
    let names: Vec<String> = std::fs::read_dir(pkg.join("Media"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"Kick.caf".to_string()), "{names:?}");
    s.recheck_media().unwrap();
    let row = &s.media().unwrap()[0];
    assert_eq!(row.offline, None, "still online");
    assert!(problems(&s).is_empty());
    s.close().unwrap();
    assert!(pkg.join("Media/Kick.caf").is_file(), "close keeps it too");
}
