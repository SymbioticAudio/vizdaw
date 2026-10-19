//! Media in the session (plan 5a Task 10): imports as jobs, the media
//! root (scratch while untitled), Save As copying media, offline media.

// Spawning a thread with an `Arc<Session>` checks `Send` through the
// document's persistent maps (E0275 at the default limit).
#![recursion_limit = "256"]

use core_media::caf::{CafWriter, WriteMode};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vizdaw_ffi::{
    ImportTargetRow, JobKindRow, JobRow, JobStateRow, OfflineReasonRow, Session, VizError,
};

/// A 48 kHz float CAF source of `frames` frames (the importer reads CAF on
/// every platform).
fn source(dir: &Path, name: &str, frames: usize, ch: u16) -> PathBuf {
    source_in(dir, name, frames, ch, BitDepth::Float32)
}

/// A 48 kHz CAF of `frames` frames in `depth`.
fn source_in(dir: &Path, name: &str, frames: usize, ch: u16, depth: BitDepth) -> PathBuf {
    source_of(dir, name, frames, ch, depth, 0.25)
}

/// A 48 kHz CAF of `frames` frames in `depth`, every sample `value`.
fn source_of(
    dir: &Path,
    name: &str,
    frames: usize,
    ch: u16,
    depth: BitDepth,
    value: f32,
) -> PathBuf {
    let p = dir.join(name);
    let mut w = CafWriter::create(&p, 48_000, ch, depth, WriteMode::Normal).unwrap();
    w.write(&vec![value; frames * usize::from(ch)]).unwrap();
    w.finish().unwrap();
    p
}

/// The engine host is process-wide: the tests that play run one at a
/// time.
static PLAYING: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn wait(s: &Session, id: u64) -> JobRow {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            return j;
        }
        assert!(Instant::now() < until, "job {id} still running");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn import(s: &Arc<Session>, paths: &[&Path], target: Option<ImportTargetRow>) -> JobRow {
    let id = s
        .import_audio(
            paths.iter().map(|p| p.display().to_string()).collect(),
            target,
        )
        .unwrap();
    wait(s, id)
}

#[test]
fn an_untitled_import_lands_in_the_scratch_package() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "kick.caf", 4800, 2);
    let s = Session::new_empty().unwrap();
    let j = import(&s, &[&src], None);
    assert_eq!(
        (j.kind, j.state),
        (JobKindRow::Import, JobStateRow::Done),
        "{j:?}"
    );
    assert_eq!(j.created.len(), 1);
    let rows = s.media().unwrap();
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(
        (
            r.name.as_str(),
            r.path.as_str(),
            r.channels,
            r.frames,
            r.offline
        ),
        ("kick", "Media/kick.caf", 2, 4800, None)
    );
    assert_eq!(r.original_name.as_deref(), Some("kick.caf"));
    assert_eq!(r.uses, 0);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Import Audio")
    );
    s.close().unwrap();
}

#[test]
fn importing_the_same_file_twice_makes_two_items() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "a.caf", 100, 1);
    let s = Session::new_empty().unwrap();
    import(&s, &[&src], None);
    import(&s, &[&src], None);
    let names: Vec<String> = s.media().unwrap().into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["a".to_string(), "a 2".to_string()]);
    s.close().unwrap();
}

#[test]
fn a_bad_file_fails_alone_and_the_batch_imports() {
    let dir = tempfile::tempdir().unwrap();
    let good = source(dir.path(), "good.caf", 100, 2);
    let bad = dir.path().join("bad.wav");
    std::fs::write(&bad, "not audio").unwrap();
    let s = Session::new_empty().unwrap();
    let j = import(&s, &[&bad, &good], None);
    assert_eq!(j.state, JobStateRow::Done);
    assert_eq!(j.created.len(), 1);
    assert!(
        j.problems.iter().any(|p| p.starts_with("bad.wav: ")),
        "{:?}",
        j.problems
    );
    s.close().unwrap();
}

#[test]
fn importing_into_a_sequence_adds_a_lane_and_a_region_in_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "loop.caf", 48_000, 2);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let lanes_before = s.lanes(seq).unwrap().len();
    let j = import(
        &s,
        &[&src],
        Some(ImportTargetRow {
            seq,
            lane: None,
            tick: 15_360,
        }),
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let lanes = s.lanes(seq).unwrap();
    assert_eq!(lanes.len(), lanes_before + 1);
    let lane = lanes.last().unwrap();
    assert_eq!(lane.name, format!("Audio {}", lanes_before + 1));
    let ev = s.lane_events(seq, lane.id).unwrap();
    assert_eq!(ev.events.len(), 1);
    assert_eq!((ev.events[0].tick, ev.events[0].frames), (15_360, 48_000));
    assert_eq!(
        s.media()
            .unwrap()
            .iter()
            .filter(|r| r.name == "loop")
            .count(),
        1
    );
    assert_eq!(
        s.media()
            .unwrap()
            .iter()
            .find(|r| r.name == "loop")
            .unwrap()
            .uses,
        1
    );
    s.undo().unwrap();
    assert_eq!(s.lanes(seq).unwrap().len(), lanes_before);
    assert!(s.media().unwrap().iter().all(|r| r.name != "loop"));
    s.close().unwrap();
}

#[test]
fn importing_into_a_selected_lane_uses_it() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "v.caf", 1000, 1);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let lane = s.lanes(seq).unwrap()[0].id;
    let before = s.lane_events(seq, lane).unwrap().events.len();
    let j = import(
        &s,
        &[&src],
        Some(ImportTargetRow {
            seq,
            lane: Some(lane),
            tick: -50,
        }),
    );
    assert_eq!(j.state, JobStateRow::Done);
    let ev = s.lane_events(seq, lane).unwrap();
    assert_eq!(ev.events.len(), before + 1);
    // A negative tick imports at 0.
    assert!(ev.events.iter().any(|e| e.tick == 0 && e.frames == 1000));
    s.close().unwrap();
}

#[test]
fn save_as_copies_the_media_and_reopens_online() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "pad.caf", 2000, 2);
    let s = Session::new_empty().unwrap();
    import(&s, &[&src], None);
    let pkg = dir.path().join("Song.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    assert!(pkg.join("Media/pad.caf").is_file());
    s.close().unwrap();
    let again = Session::open(pkg.display().to_string()).unwrap();
    let rows = again.media().unwrap();
    assert_eq!(rows[0].offline, None);
    // Save As elsewhere carries the media too.
    let other = dir.path().join("Copy.vizdaw");
    again.save_as(other.display().to_string()).unwrap();
    assert!(other.join("Media/pad.caf").is_file());
    again.close().unwrap();
}

#[test]
fn missing_and_changed_files_are_offline_until_rechecked() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "x.caf", 500, 1);
    let s = Session::new_empty().unwrap();
    import(&s, &[&src], None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.close().unwrap();
    let file = pkg.join("Media/x.caf");
    let kept = dir.path().join("kept.caf");
    std::fs::rename(&file, &kept).unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    assert_eq!(
        s.media().unwrap()[0].offline,
        Some(OfflineReasonRow::Missing)
    );
    // Something else under its name: unreadable.
    std::fs::write(&file, "nope").unwrap();
    assert!(s.recheck_media().unwrap());
    assert_eq!(
        s.media().unwrap()[0].offline,
        Some(OfflineReasonRow::Unreadable)
    );
    // A CAF of another length (in the project's depth, so only the
    // length differs).
    std::fs::remove_file(&file).unwrap();
    source_in(&pkg.join("Media"), "x.caf", 400, 1, BitDepth::Int24);
    assert!(s.recheck_media().unwrap());
    assert_eq!(
        s.media().unwrap()[0].offline,
        Some(OfflineReasonRow::Length)
    );
    // The real file back: online, and a second recheck changes nothing.
    std::fs::remove_file(&file).unwrap();
    std::fs::rename(&kept, &file).unwrap();
    assert!(s.recheck_media().unwrap());
    assert_eq!(s.media().unwrap()[0].offline, None);
    assert!(!s.recheck_media().unwrap());
    s.close().unwrap();
}

#[test]
fn a_cancelled_import_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "long.caf", 2_000_000, 2);
    let s = Session::new_empty().unwrap();
    let id = s
        .import_audio(vec![src.display().to_string()], None)
        .unwrap();
    s.cancel_job(id).unwrap();
    let j = wait(&s, id);
    assert!(
        matches!(j.state, JobStateRow::Cancelled | JobStateRow::Done),
        "{j:?}"
    );
    if j.state == JobStateRow::Cancelled {
        assert!(s.media().unwrap().is_empty());
    }
    s.close().unwrap();
}

/// Save As while a job runs is refused (Ruling 15, review F8): the job
/// wrote under the old media root, which Save As would move or delete.
#[test]
fn save_as_is_refused_while_a_job_runs() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "held.caf", 300, 2);
    let s = Session::new_empty().unwrap();
    s.debug_hold_jobs(true).unwrap();
    let id = s
        .import_audio(vec![src.display().to_string()], None)
        .unwrap();
    let pkg = dir.path().join("Held.vizdaw");
    let e = s.save_as(pkg.display().to_string()).unwrap_err();
    match &e {
        VizError::Busy { message } => {
            assert!(
                message.contains("Wait for the import or conversion to finish"),
                "{message}"
            )
        }
        other => panic!("expected Busy, got {other:?}"),
    }
    // Nothing written, still untitled, the job still running.
    assert!(!pkg.exists());
    assert_eq!(s.path().unwrap(), None);
    assert_eq!(s.job(id).unwrap().state, JobStateRow::Running);
    // The job goes on and lands in the (untouched) scratch package.
    s.debug_hold_jobs(false).unwrap();
    let j = wait(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let rows = s.media().unwrap();
    assert_eq!((rows.len(), rows[0].offline), (1, None));
    s.close().unwrap();
}

/// Once the job is done (and applied), Save As goes through and carries
/// its media.
#[test]
fn save_as_succeeds_once_the_job_is_done() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "after.caf", 300, 2);
    let s = Session::new_empty().unwrap();
    s.debug_hold_jobs(true).unwrap();
    let id = s
        .import_audio(vec![src.display().to_string()], None)
        .unwrap();
    let pkg = dir.path().join("After.vizdaw");
    assert!(s.save_as(pkg.display().to_string()).is_err());
    s.debug_hold_jobs(false).unwrap();
    // Save As right away: a job that finished but was not applied yet is
    // applied first (its files are under the old root).
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        match s.save_as(pkg.display().to_string()) {
            Ok(()) => break,
            Err(VizError::Busy { .. }) => {
                assert!(Instant::now() < until, "job {id} never finished");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("{e:?}"),
        }
    }
    assert_eq!(s.job(id).unwrap().state, JobStateRow::Done);
    assert!(pkg.join("Media/after.caf").is_file());
    assert_eq!(s.media().unwrap()[0].offline, None);
    s.close().unwrap();
    let again = Session::open(pkg.display().to_string()).unwrap();
    assert_eq!(again.media().unwrap()[0].offline, None);
    again.close().unwrap();
}

/// A Play waiting for the disk (up to 250 ms, `Engine::play`'s prime)
/// does not hold the engine host's lock (Task 9 review carry): the
/// transport poll answers meanwhile.
#[test]
fn a_play_waiting_for_the_disk_does_not_block_the_transport_poll() {
    use vizdaw_ffi::host::{force_offline, reset_host, stall_disk};
    let _g = PLAYING.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "slow.caf", 96_000, 2);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let j = import(
        &s,
        &[&src],
        Some(ImportTargetRow {
            seq,
            lane: None,
            tick: 0,
        }),
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    // The disk never catches up: the Play waits its full 250 ms.
    stall_disk(true);
    let player = Arc::clone(&s);
    let t0 = Instant::now();
    let play = std::thread::spawn(move || {
        player.play_from(seq, 0, None).unwrap();
        t0.elapsed()
    });
    std::thread::sleep(Duration::from_millis(40));
    let mut worst = Duration::ZERO;
    while !play.is_finished() {
        let t = Instant::now();
        s.transport().unwrap();
        worst = worst.max(t.elapsed());
        std::thread::sleep(Duration::from_millis(10));
    }
    let played = play.join().unwrap();
    stall_disk(false);
    assert!(
        played >= Duration::from_millis(200),
        "the Play did not wait: {played:?}"
    );
    assert!(
        worst < Duration::from_millis(100),
        "a poll waited {worst:?}"
    );
    // The Play went through once the wait was over.
    let until = Instant::now() + Duration::from_secs(5);
    while !s.transport().unwrap().playing {
        assert!(Instant::now() < until, "never started playing");
        std::thread::sleep(Duration::from_millis(5));
    }
    s.stop().unwrap();
    s.close().unwrap();
    reset_host();
}

/// A Stop that comes while a Play waits for the disk wins: the Play is
/// not sent after it.
#[test]
fn a_stop_during_the_disk_wait_cancels_the_play() {
    use vizdaw_ffi::host::{force_offline, reset_host, stall_disk};
    let _g = PLAYING.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "stop.caf", 96_000, 2);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let j = import(
        &s,
        &[&src],
        Some(ImportTargetRow {
            seq,
            lane: None,
            tick: 0,
        }),
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    stall_disk(true);
    let player = Arc::clone(&s);
    let play = std::thread::spawn(move || player.play_from(seq, 0, None).unwrap());
    std::thread::sleep(Duration::from_millis(60));
    s.stop().unwrap();
    play.join().unwrap();
    stall_disk(false);
    std::thread::sleep(Duration::from_millis(100));
    assert!(!s.transport().unwrap().playing);
    s.close().unwrap();
    reset_host();
}

/// Save As (Replace) onto a package whose `Media/<name>.caf` holds other
/// audio replaces that file (review I1): the saved project plays the
/// source's audio, online.
#[test]
fn save_as_replace_overwrites_a_different_same_named_file() {
    use core_media::caf::hash_file;
    let dir = tempfile::tempdir().unwrap();
    let old_src = dir.path().join("old");
    let new_src = dir.path().join("new");
    std::fs::create_dir_all(&old_src).unwrap();
    std::fs::create_dir_all(&new_src).unwrap();
    // The same name and length, other audio.
    let a = source_of(&old_src, "pad.caf", 1000, 2, BitDepth::Float32, 0.5);
    let b = source_of(&new_src, "pad.caf", 1000, 2, BitDepth::Float32, 0.25);
    let pkg = dir.path().join("Old.vizdaw");
    let first = Session::new_empty().unwrap();
    import(&first, &[&a], None);
    first.save_as(pkg.display().to_string()).unwrap();
    first.close().unwrap();
    let old_hash = hash_file(&pkg.join("Media/pad.caf")).unwrap();
    // The new project, saved once elsewhere to know its file's hash.
    let s = Session::new_empty().unwrap();
    import(&s, &[&b], None);
    let reference = dir.path().join("Ref.vizdaw");
    s.save_as(reference.display().to_string()).unwrap();
    let want = hash_file(&reference.join("Media/pad.caf")).unwrap();
    assert_ne!(want, old_hash);
    s.save_as(pkg.display().to_string()).unwrap();
    assert_eq!(hash_file(&pkg.join("Media/pad.caf")).unwrap(), want);
    assert_eq!(s.media().unwrap()[0].offline, None);
    s.close().unwrap();
    // No temp file is left behind (the overview cache folder aside).
    let names: Vec<String> = std::fs::read_dir(pkg.join("Media"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != ".overviews")
        .collect();
    assert_eq!(names, vec!["pad.caf".to_string()]);
    let again = Session::open(pkg.display().to_string()).unwrap();
    assert_eq!(again.media().unwrap()[0].offline, None);
    again.close().unwrap();
}

/// Save As onto a package whose same-named file holds the same audio
/// leaves that file alone (no copy).
#[test]
fn save_as_keeps_an_identical_same_named_file() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "same.caf", 1000, 2);
    let pkg = dir.path().join("Same.vizdaw");
    let first = Session::new_empty().unwrap();
    import(&first, &[&src], None);
    first.save_as(pkg.display().to_string()).unwrap();
    first.close().unwrap();
    let file = pkg.join("Media/same.caf");
    let before = std::fs::metadata(&file).unwrap();
    // Another session imports the same source into its own scratch file
    // (another inode, the same audio).
    let s = Session::new_empty().unwrap();
    import(&s, &[&src], None);
    std::thread::sleep(Duration::from_millis(20));
    s.save_as(pkg.display().to_string()).unwrap();
    let after = std::fs::metadata(&file).unwrap();
    assert_eq!(after.ino(), before.ino());
    assert_eq!(after.modified().unwrap(), before.modified().unwrap());
    assert_eq!(s.media().unwrap()[0].offline, None);
    s.close().unwrap();
}

/// Waits until every job thread of `s` has ended (applied or not).
fn settled(s: &Session) {
    let until = Instant::now() + Duration::from_secs(60);
    while !s.debug_jobs_settled() {
        assert!(Instant::now() < until, "job threads still running");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Final review I1: an import that finishes while an edit (a drag) is
/// open waits for it to close, then applies as its own undo step with
/// its files kept; it is never reported Done before that.
#[test]
fn an_import_finishing_during_an_edit_applies_after_it_as_its_own_step() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "late.caf", 1000, 2);
    let s = Session::new_empty().unwrap();
    s.begin_edit("Drag".into()).unwrap();
    s.add_bus("Bus".into()).unwrap();
    let id = s
        .import_audio(vec![src.display().to_string()], None)
        .unwrap();
    settled(&s);
    // Finished but not applied: still running, nothing in the pool.
    let j = s.job(id).unwrap();
    assert_eq!(j.state, JobStateRow::Running, "{j:?}");
    s.idle().unwrap();
    assert_eq!(s.job(id).unwrap().state, JobStateRow::Running);
    assert!(s.media().unwrap().is_empty());
    // Closing the edit applies it at once (no poll needed).
    s.end_edit().unwrap();
    let rows = s.media().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].offline, None);
    let j = s.job(id).unwrap();
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    // Its files were kept (online means the file is there and reads).
    let pkg = dir.path().join("Late.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    assert!(pkg.join("Media/late.caf").is_file());
    // Its own undo step, after the drag's.
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Import Audio")
    );
    s.undo().unwrap();
    assert!(s.media().unwrap().is_empty());
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Drag"));
    s.close().unwrap();
}

/// Final review I1: aborting the edit (Escape) also lets the job apply.
#[test]
fn an_import_finishing_during_an_aborted_edit_applies_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "esc.caf", 1000, 2);
    let s = Session::new_empty().unwrap();
    s.begin_edit("Drag".into()).unwrap();
    let id = s
        .import_audio(vec![src.display().to_string()], None)
        .unwrap();
    settled(&s);
    assert_eq!(s.job(id).unwrap().state, JobStateRow::Running);
    s.abort_edit().unwrap();
    assert_eq!(s.media().unwrap().len(), 1);
    assert_eq!(s.job(id).unwrap().state, JobStateRow::Done);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Import Audio")
    );
    s.close().unwrap();
}

/// Final review I3: Untitled → import → Undo → Save As → Redo leaves the
/// item online: Save As carries media only the redo history uses.
#[test]
fn save_as_carries_media_only_the_redo_history_uses() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "redo.caf", 1000, 2);
    let s = Session::new_empty().unwrap();
    assert_eq!(import(&s, &[&src], None).state, JobStateRow::Done);
    s.undo().unwrap();
    assert!(s.media().unwrap().is_empty());
    let pkg = dir.path().join("Redo.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    assert!(pkg.join("Media/redo.caf").is_file());
    s.redo().unwrap();
    let rows = s.media().unwrap();
    assert_eq!((rows.len(), rows[0].offline), (1, None), "{rows:?}");
    s.close().unwrap();
}

/// Task 10 m10: leftover Save As temp files are swept when a project
/// opens.
#[test]
fn opening_a_project_sweeps_leftover_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "keep.caf", 100, 1);
    let s = Session::new_empty().unwrap();
    import(&s, &[&src], None);
    let pkg = dir.path().join("Sweep.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.close().unwrap();
    let media = pkg.join("Media");
    std::fs::write(media.join(".keep.caf.12345.vizdaw-tmp"), b"junk").unwrap();
    std::fs::write(media.join("other.vizdaw-tmp"), b"not ours").unwrap();
    let again = Session::open(pkg.display().to_string()).unwrap();
    assert!(!media.join(".keep.caf.12345.vizdaw-tmp").exists());
    assert!(media.join("other.vizdaw-tmp").exists(), "not a temp name");
    assert!(media.join("keep.caf").is_file());
    assert_eq!(again.media().unwrap()[0].offline, None);
    again.close().unwrap();
}
