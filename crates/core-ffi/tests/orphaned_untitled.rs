//! Recordings an untitled project lost in a crash (plan 5c Task 10;
//! pre-flight I5): recording-mode files left under
//! `VizDAW-untitled/<pid>-<session>.vizdaw/Media/Recorded/` by a VizDAW
//! that is no longer running are moved to the Trash (never deleted) with
//! one line naming them; a running VizDAW's packages are left alone.
//!
//! One test per binary: the scratch folder override is process-wide.

use core_media::caf::{CafWriter, WriteMode};
use core_model::format::BitDepth;
use std::path::Path;
use vizdaw_ffi::media::{scratch_parent, set_scratch_dir};
use vizdaw_ffi::record::trash_orphaned_untitled;

fn recording(path: &Path, frames: usize, mode: WriteMode) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut w = CafWriter::create(path, 48_000, 1, BitDepth::Int24, mode).unwrap();
    w.write(&vec![0.25; frames]).unwrap();
    if mode == WriteMode::Normal {
        w.finish().unwrap();
    }
}

#[test]
fn recordings_a_dead_untitled_project_left_go_to_the_trash() {
    let dir = tempfile::tempdir().unwrap();
    // The override: nothing here looks at the real `$TMPDIR`.
    set_scratch_dir(Some(dir.path().join("scratch")));
    assert_eq!(scratch_parent(), dir.path().join("scratch"));
    let untitled = dir.path().join("scratch/VizDAW-untitled");
    // A pid that has exited, one that runs (a child), launchd (running,
    // not ours to signal) and this process.
    let dead = std::process::Command::new("/usr/bin/true").spawn().unwrap();
    let dead_pid = dead.id();
    let mut dead = dead;
    dead.wait().unwrap();
    let mut live = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let rec = |pid: u32, name: &str| untitled.join(format!("{pid}-1.vizdaw/Media/Recorded/{name}"));
    recording(&rec(dead_pid, "x.caf"), 10, WriteMode::Recording);
    recording(&rec(dead_pid, "y.caf"), 10, WriteMode::Recording);
    // Not recordings, or empty: left.
    recording(&rec(dead_pid, "plain.caf"), 10, WriteMode::Normal);
    recording(&rec(dead_pid, "empty.caf"), 0, WriteMode::Recording);
    recording(&rec(live.id(), "x.caf"), 10, WriteMode::Recording);
    recording(&rec(1, "x.caf"), 10, WriteMode::Recording);
    recording(&rec(std::process::id(), "x.caf"), 10, WriteMode::Recording);
    std::fs::create_dir_all(untitled.join("junk.vizdaw")).unwrap();
    let trash = dir.path().join("Trash");
    let line = trash_orphaned_untitled(&scratch_parent(), Some(&trash));
    assert_eq!(
        line.as_deref(),
        Some(
            "2 recordings from an untitled project VizDAW lost were moved to the Trash: x.caf, y.caf"
        )
    );
    assert!(trash.join("x.caf").is_file());
    assert!(trash.join("y.caf").is_file());
    assert!(!rec(dead_pid, "x.caf").exists());
    assert!(rec(dead_pid, "plain.caf").is_file());
    assert!(rec(dead_pid, "empty.caf").is_file());
    for pid in [live.id(), 1, std::process::id()] {
        assert!(rec(pid, "x.caf").is_file(), "pid {pid} untouched");
    }
    // Nothing more to move: no line.
    assert_eq!(
        trash_orphaned_untitled(&scratch_parent(), Some(&trash)),
        None
    );
    // The app's call (no test Trash: the files are never deleted, the
    // line says where they are).
    recording(&rec(dead_pid, "z.caf"), 10, WriteMode::Recording);
    let line = vizdaw_ffi::record::recover_untitled_recordings()
        .unwrap()
        .unwrap();
    assert!(
        line.starts_with(
            "1 recording from an untitled project VizDAW lost could not be moved to the Trash"
        ),
        "{line}"
    );
    assert!(rec(dead_pid, "z.caf").is_file(), "never deleted");
    live.kill().unwrap();
    live.wait().unwrap();
    set_scratch_dir(None);
}
