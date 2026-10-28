//! Project formats (plan 5a Task 11): new projects, format changes that
//! convert every file and rescale regions, converting an old project's
//! media.

mod common;

use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vizdaw_ffi::{
    BitDepthRow, ImportTargetRow, JobRow, JobStateRow, ProjectFormatRow, Session, VizError,
    format_label, project_rates,
};

fn fmt(rate: u32, depth: BitDepthRow) -> ProjectFormatRow {
    ProjectFormatRow {
        sample_rate: rate,
        depth,
    }
}

fn caf(dir: &Path, name: &str, rate: u32, frames: usize) -> PathBuf {
    let p = dir.join(name);
    let mut w = CafWriter::create(&p, rate, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![0.25f32; frames * 2]).unwrap();
    w.finish().unwrap();
    p
}

fn wait(s: &Session, id: u64) -> JobRow {
    let until = Instant::now() + common::wait_limit();
    loop {
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            return j;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The demo with one imported 1 s region at beat 1 in a new lane.
fn demo_with_region(dir: &Path) -> (Arc<Session>, u64, u64) {
    let src = caf(dir, "one.caf", 48_000, 48_000);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let id = s
        .import_audio(
            vec![src.display().to_string()],
            Some(ImportTargetRow {
                seq,
                lane: None,
                tick: 15_360,
            }),
        )
        .unwrap();
    assert_eq!(wait(&s, id).state, JobStateRow::Done);
    let lane = s.lanes(seq).unwrap().last().unwrap().id;
    (s, seq, lane)
}

#[test]
fn new_projects_take_their_format() {
    let s = Session::new_project(fmt(44_100, BitDepthRow::Int16)).unwrap();
    let i = s.project_info().unwrap();
    assert_eq!(i.format, fmt(44_100, BitDepthRow::Int16));
    assert_eq!((i.mismatched, i.rate_mismatch), (0, false));
    assert!(s.snapshot().unwrap().undo_label.is_none());
    assert_eq!(
        format_label(fmt(48_000, BitDepthRow::Int24)),
        "48 kHz, 24-bit"
    );
    assert_eq!(
        project_rates(),
        vec![44_100, 48_000, 88_200, 96_000, 176_400, 192_000]
    );
    // An imported file is stored in the project's format.
    let dir = tempfile::tempdir().unwrap();
    let src = caf(dir.path(), "a.caf", 44_100, 1000);
    let id = s
        .import_audio(vec![src.display().to_string()], None)
        .unwrap();
    assert_eq!(wait(&s, id).state, JobStateRow::Done);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let r = CafReader::open(&pkg.join("Media/a.caf")).unwrap();
    assert_eq!(
        (r.info().sample_rate, r.info().depth),
        (44_100, BitDepth::Int16)
    );
    s.close().unwrap();
}

#[test]
fn a_depth_change_converts_every_file_and_undo_restores() {
    let dir = tempfile::tempdir().unwrap();
    let (s, seq, lane) = demo_with_region(dir.path());
    let before = s.media().unwrap();
    let id = s
        .set_project_format(fmt(48_000, BitDepthRow::Float32))
        .unwrap();
    let j = wait(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Change Project Format")
    );
    assert_eq!(
        s.project_info().unwrap().format,
        fmt(48_000, BitDepthRow::Float32)
    );
    let after = s.media().unwrap();
    let one = after.iter().find(|m| m.name.starts_with("one")).unwrap();
    assert_eq!(one.offline, None);
    assert_ne!(
        one.path,
        before.iter().find(|m| m.name == "one").unwrap().path
    );
    // Same rate: the region keeps its frames.
    assert_eq!(s.lane_events(seq, lane).unwrap().events[0].frames, 48_000);
    s.undo().unwrap();
    assert_eq!(
        s.project_info().unwrap().format,
        fmt(48_000, BitDepthRow::Int24)
    );
    assert_eq!(
        s.media()
            .unwrap()
            .iter()
            .find(|m| m.name == "one")
            .unwrap()
            .offline,
        None
    );
    s.close().unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn set_format_rescales_regions_and_undo_restores() {
    let dir = tempfile::tempdir().unwrap();
    let (s, seq, lane) = demo_with_region(dir.path());
    let id = s
        .set_project_format(fmt(44_100, BitDepthRow::Int16))
        .unwrap();
    let j = wait(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let one = s
        .media()
        .unwrap()
        .into_iter()
        .find(|m| m.name.starts_with("one"))
        .unwrap();
    assert_eq!((one.sample_rate, one.offline), (44_100, None));
    assert!(one.frames.abs_diff(44_100) <= 1, "{}", one.frames);
    let ev = &s.lane_events(seq, lane).unwrap().events[0];
    assert!(ev.frames.abs_diff(44_100) <= 1, "{}", ev.frames);
    s.undo().unwrap();
    assert_eq!(s.lane_events(seq, lane).unwrap().events[0].frames, 48_000);
    s.close().unwrap();
}

#[test]
fn the_same_format_and_missing_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _, _) = demo_with_region(dir.path());
    assert!(
        s.set_project_format(fmt(48_000, BitDepthRow::Int24))
            .is_err()
    );
    // The demo's loop.wav does not exist: a format change must refuse.
    let s2 = Session::new_demo_audio().unwrap();
    let id = s2
        .set_project_format(fmt(48_000, BitDepthRow::Int16))
        .unwrap();
    let j = wait(&s2, id);
    assert_eq!(j.state, JobStateRow::Failed);
    assert!(
        j.message.unwrap_or_default().contains("loop"),
        "names the file"
    );
    s.close().unwrap();
    s2.close().unwrap();
}

// Rate conversion needs the system importer (Ruling 8).
#[cfg(target_os = "macos")]
#[test]
fn old_media_in_another_format_converts_on_request() {
    let dir = tempfile::tempdir().unwrap();
    // A saved 48 kHz project whose one item was replaced by a 44.1 kHz CAF.
    let (s, seq, lane) = demo_with_region(dir.path());
    let pkg = dir.path().join("Old.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.close().unwrap();
    std::fs::remove_file(pkg.join("Media/one.caf")).unwrap();
    caf(&pkg.join("Media"), "one.caf", 44_100, 48_000);
    // Task 11 addition: the conversion never rewrites the old file.
    let old_bytes = std::fs::read(pkg.join("Media/one.caf")).unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    let one = s
        .media()
        .unwrap()
        .into_iter()
        .find(|m| m.name == "one")
        .unwrap();
    assert_eq!(one.offline, Some(vizdaw_ffi::OfflineReasonRow::Format));
    assert_eq!(s.project_info().unwrap().mismatched, 1);
    let id = s.convert_media().unwrap();
    let j = wait(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Convert Media")
    );
    let one = s
        .media()
        .unwrap()
        .into_iter()
        .find(|m| m.name.starts_with("one"))
        .unwrap();
    assert_eq!((one.offline, one.sample_rate), (None, 48_000));
    assert_eq!(s.project_info().unwrap().mismatched, 0);
    // The region keeps its frames: the item said 48 kHz, so no rescale.
    assert_eq!(s.lane_events(seq, lane).unwrap().events[0].frames, 48_000);
    assert_ne!(one.path, "Media/one.caf", "a new file");
    assert_eq!(
        std::fs::read(pkg.join("Media/one.caf")).unwrap(),
        old_bytes,
        "the old file is untouched"
    );
    s.close().unwrap();
}

// ---- Task 11 additions (dispatch rulings) ----

/// Media files are write-once (Save As hard-links them between packages):
/// a format change writes new files under free names and never touches
/// the old ones, which undo goes back to.
#[test]
fn a_format_change_leaves_the_old_file_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _, _) = demo_with_region(dir.path());
    let pkg = dir.path().join("W.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let old = pkg.join("Media/one.caf");
    let bytes = std::fs::read(&old).unwrap();
    let modified = std::fs::metadata(&old).unwrap().modified().unwrap();
    let id = s
        .set_project_format(fmt(48_000, BitDepthRow::Float32))
        .unwrap();
    let j = wait(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let one = s
        .media()
        .unwrap()
        .into_iter()
        .find(|m| m.name == "one")
        .unwrap();
    assert_ne!(one.path, "Media/one.caf");
    assert!(pkg.join(&one.path).is_file());
    assert_eq!(std::fs::read(&old).unwrap(), bytes, "same bytes");
    assert_eq!(
        std::fs::metadata(&old).unwrap().modified().unwrap(),
        modified
    );
    let r = CafReader::open(&pkg.join(&one.path)).unwrap();
    assert_eq!(r.info().depth, BitDepth::Float32);
    // Undo goes back to the old file, online.
    s.undo().unwrap();
    let one = s
        .media()
        .unwrap()
        .into_iter()
        .find(|m| m.name == "one")
        .unwrap();
    assert_eq!((one.path.as_str(), one.offline), ("Media/one.caf", None));
    s.close().unwrap();
}

/// F8: a conversion is a media job, so Save As waits for it.
#[test]
fn save_as_is_refused_during_a_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _, _) = demo_with_region(dir.path());
    s.debug_hold_jobs(true).unwrap();
    let id = s
        .set_project_format(fmt(48_000, BitDepthRow::Int16))
        .unwrap();
    let pkg = dir.path().join("Busy.vizdaw");
    match s.save_as(pkg.display().to_string()) {
        Err(VizError::Busy { .. }) => {}
        other => panic!("expected Busy, got {other:?}"),
    }
    assert!(!pkg.exists());
    assert_eq!(s.path().unwrap(), None);
    assert_eq!(s.job(id).unwrap().state, JobStateRow::Running);
    s.debug_hold_jobs(false).unwrap();
    let j = wait(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    s.save_as(pkg.display().to_string()).unwrap();
    let one = s
        .media()
        .unwrap()
        .into_iter()
        .find(|m| m.name == "one")
        .unwrap();
    assert_eq!(one.offline, None);
    assert!(pkg.join(&one.path).is_file());
    s.close().unwrap();
}

/// A rate round trip 48k → 44.1k → 48k keeps a trimmed, faded region's
/// offset, length and fades within ±1 frame (Convert and SetFormat share
/// one rescale; positions are ticks and do not move).
#[cfg(target_os = "macos")]
#[test]
fn a_rate_round_trip_keeps_regions_within_a_frame() {
    use vizdaw_ffi::{AudioEdit, EditCommand, FadeRow, FadeShapeRow};
    let dir = tempfile::tempdir().unwrap();
    let (s, seq, lane) = demo_with_region(dir.path());
    let event = s.lane_events(seq, lane).unwrap().events[0].id;
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Trim {
                event,
                front_frames: 1001,
                back_frames: 999,
            },
        },
        None,
    )
    .unwrap();
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::SetFades {
                event,
                fade_in: FadeRow {
                    length: 4801,
                    shape: FadeShapeRow::Linear,
                },
                fade_out: FadeRow {
                    length: 2399,
                    shape: FadeShapeRow::EqualPower,
                },
            },
        },
        None,
    )
    .unwrap();
    let ev = |s: &Session| s.lane_events(seq, lane).unwrap().events[0].clone();
    let start = ev(&s);
    let shape = |e: &vizdaw_ffi::EventRow| {
        (
            e.tick,
            e.source_offset,
            e.frames,
            e.fade_in.length,
            e.fade_out.length,
        )
    };
    let (tick, off, len, fi, fo) = shape(&start);
    assert_eq!((off, len, fi, fo), (1001, 46_000, 4801, 2399));
    for (rate, depth) in [(44_100, BitDepthRow::Int24), (48_000, BitDepthRow::Int24)] {
        let id = s.set_project_format(fmt(rate, depth)).unwrap();
        let j = wait(&s, id);
        assert_eq!(j.state, JobStateRow::Done, "{j:?}");
        let e = ev(&s);
        let r = f64::from(rate) / 48_000.0;
        let near = |got: u64, want: u64| (got as f64 - want as f64 * r).abs() <= 1.0;
        assert_eq!(e.tick, tick, "positions are ticks");
        assert!(near(e.source_offset, off), "{e:?}");
        assert!(near(e.frames, len), "{e:?}");
        assert!(near(e.fade_in.length as u64, fi as u64), "{e:?}");
        assert!(near(e.fade_out.length as u64, fo as u64), "{e:?}");
    }
    let (t2, off2, len2, fi2, fo2) = shape(&ev(&s));
    assert_eq!(t2, tick);
    assert!(off2.abs_diff(off) <= 1, "{off2}");
    assert!(len2.abs_diff(len) <= 1, "{len2}");
    assert!(fi2.abs_diff(fi) <= 1, "{fi2}");
    assert!(fo2.abs_diff(fo) <= 1, "{fo2}");
    s.close().unwrap();
}

/// Waits until every job thread of `s` has ended (applied or not).
fn settled(s: &Session) {
    let until = Instant::now() + common::wait_limit();
    while !s.debug_jobs_settled() {
        assert!(Instant::now() < until, "job threads still running");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Final review I1: a format change that finishes during a drag is not
/// merged into the drag's undo step, and a pointer move (which rewinds
/// the drag) does not roll it back; it applies once the drag ends.
#[test]
fn a_format_change_finishing_during_a_drag_is_its_own_step() {
    use vizdaw_ffi::edit::{EditCommand, SequenceEdit};
    let dir = tempfile::tempdir().unwrap();
    let (s, seq, _) = demo_with_region(dir.path());
    let rename = |name: &str| EditCommand::Sequence {
        edit: SequenceEdit::Rename {
            seq,
            name: name.into(),
        },
    };
    s.begin_edit("Drag".into()).unwrap();
    s.edit_gesture(rename("Moved 1"), None).unwrap();
    let id = s
        .set_project_format(fmt(48_000, BitDepthRow::Float32))
        .unwrap();
    settled(&s);
    assert_eq!(s.job(id).unwrap().state, JobStateRow::Running);
    s.idle().unwrap();
    // The next pointer move: the format change is not there to lose.
    s.edit_gesture(rename("Moved 2"), None).unwrap();
    assert_eq!(s.job(id).unwrap().state, JobStateRow::Running);
    assert_eq!(
        s.project_info().unwrap().format,
        fmt(48_000, BitDepthRow::Int24)
    );
    s.end_edit().unwrap();
    let j = s.job(id).unwrap();
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(
        s.project_info().unwrap().format,
        fmt(48_000, BitDepthRow::Float32)
    );
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Change Project Format")
    );
    // A later pointer move cannot touch it (no edit is open), and one
    // Undo reverts only the format.
    assert!(s.edit_gesture(rename("Moved 3"), None).is_err());
    s.undo().unwrap();
    assert_eq!(
        s.project_info().unwrap().format,
        fmt(48_000, BitDepthRow::Int24)
    );
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Drag"));
    assert!(
        s.snapshot()
            .unwrap()
            .sequences
            .iter()
            .any(|r| r.name == "Moved 2")
    );
    s.close().unwrap();
}

/// Final review I3: Untitled → import → format change → Save As → Undo
/// leaves the original files online (Save As carries media only the
/// undo history uses; the scratch package is gone).
#[test]
fn save_as_after_a_format_change_keeps_the_original_files_for_undo() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(dir.path(), "gtr.caf", 48_000, 4800);
    let s = Session::new_empty().unwrap();
    let id = s
        .import_audio(vec![src.display().to_string()], None)
        .unwrap();
    assert_eq!(wait(&s, id).state, JobStateRow::Done);
    let id = s
        .set_project_format(fmt(48_000, BitDepthRow::Float32))
        .unwrap();
    assert_eq!(wait(&s, id).state, JobStateRow::Done);
    let pkg = dir.path().join("Gtr.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    assert!(pkg.join("Media/gtr.caf").is_file());
    s.undo().unwrap();
    let rows = s.media().unwrap();
    assert_eq!(rows[0].path, "Media/gtr.caf");
    assert_eq!(rows[0].offline, None, "{rows:?}");
    s.close().unwrap();
}
