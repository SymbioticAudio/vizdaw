//! Crash recovery of recordings (plan 5c Task 10; sub-project 5 spec
//! §6.5, §12.3 "crash-recovered files are found and offered"): files a
//! crash left in `Media/Recorded/` are offered at open, kept from the
//! Trash until answered, imported onto new lanes as one undo step, or
//! moved to the Trash, never deleted.

#![recursion_limit = "256"]

mod common;

use core_engine::fixtures::Builder;
use core_engine::offline::FakeInput;
use core_media::caf::{CafWriter, WriteMode};
use core_model::command::{Command, LaneCmd};
use core_model::format::BitDepth;
use core_model::lane::{InputChannels, LaneInput, MonitorMode};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host, set_fake_input};
use vizdaw_ffi::session::simulate_crash;
use vizdaw_ffi::{MediaOriginRow, RecordSpecRow, Session, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(Some(FakeInput::Signals));
    reset_host();
    g
}

/// A saved project `P.vizdaw` in `dir`: sequence "S" with an armed lane
/// "Mic" recording fake input 1 (monitor Off).
fn project(dir: &Path) -> (std::sync::Arc<Session>, std::path::PathBuf, u64) {
    let mut b = Builder::new();
    let (seq, mic) = b.sequence("S", "Mic", None).unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: mic,
        input: Some(LaneInput::Audio {
            channels: InputChannels::Mono(0),
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetMonitor {
        seq,
        lane: mic,
        monitor: MonitorMode::Off,
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq,
        lane: mic,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.debug_set_trash_dir(Some(dir.join("Trash").display().to_string()));
    let pkg = dir.join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    (s, pkg, seq.0)
}

/// A recording-mode CAF of `frames` frames left unfinished (as a crash
/// leaves it).
fn unfinished(path: &Path, frames: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut w = CafWriter::create(path, 48_000, 1, BitDepth::Int24, WriteMode::Recording).unwrap();
    w.write(&vec![0.25; frames]).unwrap();
    drop(w);
}

#[test]
fn a_recording_a_crash_left_is_found_and_offered() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, seq) = project(dir.path());
    s.record(
        seq,
        RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    let rel = s.record_state().unwrap().lanes[0].path.clone().unwrap();
    let until = Instant::now() + common::wait_limit();
    while s.record_state().unwrap().frames < 9600 {
        assert!(Instant::now() < until, "nothing recorded");
        std::thread::sleep(Duration::from_millis(5));
    }
    // VizDAW crashes: nothing finishes the file or applies the pass.
    simulate_crash(s);
    reset_host();
    // Things that are not crash leftovers: a hidden file, a file that is
    // not a recording, and garbage.
    unfinished(&pkg.join("Media/Recorded/.hidden.caf"), 10);
    let mut w = CafWriter::create(
        &pkg.join("Media/Recorded/plain.caf"),
        48_000,
        1,
        BitDepth::Int24,
        WriteMode::Normal,
    )
    .unwrap();
    w.write(&[0.1; 10]).unwrap();
    w.finish().unwrap();
    std::fs::write(pkg.join("Media/Recorded/junk.caf"), b"not audio").unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    assert_eq!(s.recovered_recordings().unwrap(), vec![rel.clone()]);
    assert_eq!(s.record_state().unwrap().recovered, vec![rel.clone()]);
    // Unanswered, a save keeps it from the Trash.
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    s.save().unwrap();
    assert!(pkg.join(&rel).is_file());
    // Imported: on a new lane at bar 1, one undo step.
    s.recover_recordings(true).unwrap();
    assert!(s.recovered_recordings().unwrap().is_empty());
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Recover Recordings")
    );
    let m = s.media().unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(
        (m[0].origin, m[0].offline, &m[0].path),
        (MediaOriginRow::Recorded, None, &rel)
    );
    assert!(m[0].frames >= 9600, "{}", m[0].frames);
    let lanes = s.lanes(seq).unwrap();
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[1].region_count, 1);
    s.undo().unwrap();
    assert!(s.media().unwrap().is_empty());
    assert!(pkg.join(&rel).is_file(), "the file stays for redo");
    assert!(matches!(
        s.recover_recordings(true),
        Err(VizError::Model { .. })
    ));
    s.close().unwrap();
}

#[test]
fn declined_recordings_go_to_the_trash() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, _) = project(dir.path());
    s.close().unwrap();
    let rel = "Media/Recorded/Mic 2026-10-20 120000.caf";
    unfinished(&pkg.join(rel), 4800);
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    assert_eq!(s.recovered_recordings().unwrap(), vec![rel.to_string()]);
    s.recover_recordings(false).unwrap();
    assert!(s.recovered_recordings().unwrap().is_empty());
    assert!(!pkg.join(rel).exists());
    assert!(
        dir.path().join("Trash/Mic 2026-10-20 120000.caf").is_file(),
        "moved, never deleted"
    );
    assert!(s.media().unwrap().is_empty());
    s.close().unwrap();
    // Nothing is offered again.
    let s = Session::open(pkg.display().to_string()).unwrap();
    assert!(s.recovered_recordings().unwrap().is_empty());
    s.close().unwrap();
}

// Task 10 carries (dispatch rulings).

fn spec() -> RecordSpecRow {
    RecordSpecRow {
        replace: true,
        punch: None,
        looped: false,
        count_in_bars: 0,
        pre_roll_bars: 0,
        passes_as_takes: true,
    }
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Task 9 carry: no answer while a pass records, finishes or waits for an
/// open edit to end (its files are in no document yet).
#[test]
fn recovery_waits_for_a_recording() {
    use vizdaw_ffi::record::set_hold_finish;
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, seq) = project(dir.path());
    s.close().unwrap();
    unfinished(&pkg.join("Media/Recorded/Old 2026-10-19 120000.caf"), 4800);
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    assert_eq!(s.recovered_recordings().unwrap().len(), 1);
    let busy = |s: &Session| {
        matches!(s.recover_recordings(true), Err(VizError::Busy { .. }))
            && matches!(s.recover_recordings(false), Err(VizError::Busy { .. }))
    };
    s.record(seq, spec()).unwrap();
    wait_until("audio", || s.record_state().unwrap().frames >= 4800);
    assert!(busy(&s), "recording");
    // Finishing (held), then pending behind an open edit.
    set_hold_finish(true);
    let stopper = {
        let s = std::sync::Arc::clone(&s);
        std::thread::spawn(move || s.stop())
    };
    wait_until("the pass to be finishing", || {
        !s.record_state().unwrap().recording
    });
    assert!(busy(&s), "finishing");
    set_hold_finish(false);
    stopper.join().unwrap().unwrap();
    s.record(seq, spec()).unwrap();
    wait_until("audio", || s.record_state().unwrap().frames >= 4800);
    s.begin_edit("Drag".into()).unwrap();
    s.stop().unwrap();
    assert!(busy(&s), "a pass waits for the edit");
    s.end_edit().unwrap();
    assert_eq!(s.recovered_recordings().unwrap().len(), 1, "still offered");
    s.recover_recordings(false).unwrap();
    assert!(dir.path().join("Trash/Old 2026-10-19 120000.caf").is_file());
    s.close().unwrap();
}

/// Pre-flight M9: each offered file says whether its size was written (a
/// cleanly finished take nothing names, e.g. undone then saved) or not (a
/// crash leftover).
#[test]
fn offered_files_tell_a_finished_take_from_a_crash_leftover() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, seq) = project(dir.path());
    s.record(seq, spec()).unwrap();
    let rel = s.record_state().unwrap().lanes[0].path.clone().unwrap();
    wait_until("audio", || s.record_state().unwrap().frames >= 4800);
    s.stop().unwrap();
    s.undo().unwrap();
    s.save().unwrap();
    s.close().unwrap();
    assert!(pkg.join(&rel).is_file(), "redo named it");
    let left = "Media/Recorded/Mic 2026-10-19 120000.caf";
    unfinished(&pkg.join(left), 4800);
    let s = Session::open(pkg.display().to_string()).unwrap();
    let got: Vec<(String, bool, bool)> = s
        .recovered_files()
        .unwrap()
        .into_iter()
        .map(|f| (f.path, f.size_known, f.readable))
        .collect();
    let mut want = vec![(left.to_string(), false, true), (rel.clone(), true, true)];
    want.sort();
    assert_eq!(got, want);
    assert_eq!(
        s.record_state().unwrap().recovered_files,
        s.recovered_files().unwrap()
    );
    let frames: Vec<u64> = s
        .recovered_files()
        .unwrap()
        .iter()
        .map(|f| f.frames)
        .collect();
    assert!(frames.iter().all(|f| *f >= 4800), "{frames:?}");
    s.close().unwrap();
}

/// Removes lane "Mic" while it records into `pkg`: its take is kept (no
/// region uses it) for the recovery offer. Returns the take's path.
fn kept_take(s: &Session, seq: u64) -> String {
    use vizdaw_ffi::edit::{EditCommand, LaneEdit};
    let lane = s.lanes(seq).unwrap()[0].id;
    s.record(seq, spec()).unwrap();
    let rel = s.record_state().unwrap().lanes[0].path.clone().unwrap();
    wait_until("audio", || s.record_state().unwrap().frames >= 4800);
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Remove { seq, lane },
        },
        None,
    )
    .unwrap();
    s.stop().unwrap();
    rel
}

/// Task 9 carry: kept takes go with the project, so a save in a later
/// session never moves them to the Trash.
#[test]
fn a_kept_take_survives_saves_in_later_sessions() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, seq) = project(dir.path());
    let rel = kept_take(&s, seq);
    s.save().unwrap();
    s.close().unwrap();
    assert!(pkg.join(&rel).is_file());
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    assert_eq!(s.recovered_recordings().unwrap(), vec![rel.clone()]);
    s.save().unwrap();
    s.close().unwrap();
    assert!(pkg.join(&rel).is_file(), "still in Media/Recorded");
    // Even a kept file that no longer reads as audio stays, and is
    // offered.
    let mut bytes = std::fs::read(pkg.join(&rel)).unwrap();
    bytes[..4].copy_from_slice(b"junk");
    std::fs::write(pkg.join(&rel), &bytes).unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    let f = s.recovered_files().unwrap();
    assert_eq!((f.len(), f[0].readable), (1, false), "{f:?}");
    s.save().unwrap();
    s.close().unwrap();
    assert!(pkg.join(&rel).is_file(), "kept, never trashed by a save");
    // Saved As elsewhere: the take and its keeping go along.
    let s = Session::open(pkg.display().to_string()).unwrap();
    let other = dir.path().join("Q.vizdaw");
    s.save_as(other.display().to_string()).unwrap();
    s.close().unwrap();
    let s = Session::open(other.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    s.save().unwrap();
    assert!(other.join(&rel).is_file());
    // Declined: moved to the Trash, and no longer kept or offered.
    s.recover_recordings(false).unwrap();
    assert!(!other.join(&rel).exists());
    s.save().unwrap();
    s.close().unwrap();
    let s = Session::open(other.display().to_string()).unwrap();
    assert!(s.recovered_recordings().unwrap().is_empty());
    s.close().unwrap();
}
