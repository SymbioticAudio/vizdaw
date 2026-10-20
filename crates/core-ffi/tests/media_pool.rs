//! The media pool (plan 5b Task 6, Rulings 9, 10, 12, 13): rows, rename,
//! remove, compact, users, relinking by file and by folder, and names no
//! job may take.

use core_media::caf::{CafWriter, WriteMode, hash_file};
use core_model::command::{Command, MediaCmd, apply};
use core_model::document::Document;
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vizdaw_ffi::{
    AudioEdit, EditCommand, EventKindRow, ImportTargetRow, JobKindRow, JobRow, JobStateRow,
    OfflineReasonRow, Session, VizError,
};

/// A CAF at `rate` in `depth`, every sample `value`.
fn caf(path: &Path, rate: u32, ch: u16, frames: usize, depth: BitDepth, value: f32) -> PathBuf {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut w = CafWriter::create(path, rate, ch, depth, WriteMode::Normal).unwrap();
    w.write(&vec![value; frames * usize::from(ch)]).unwrap();
    w.finish().unwrap();
    path.to_path_buf()
}

/// A 16-bit PCM WAV of silence at 48 kHz.
fn wav(path: &Path, channels: u16, frames: u32) {
    wav_at(path, 48_000, channels, frames);
}

/// A 16-bit PCM WAV of silence at `rate`.
fn wav_at(path: &Path, rate: u32, channels: u16, frames: u32) {
    let data = frames * u32::from(channels) * 2;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    b.extend_from_slice(&(channels * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.resize(b.len() + data as usize, 0);
    std::fs::write(path, b).unwrap();
}

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

fn import(s: &Arc<Session>, path: &Path, target: Option<ImportTargetRow>) {
    let j = wait(
        s,
        s.import_audio(vec![path.display().to_string()], target)
            .unwrap(),
    );
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
}

/// The demo with `src` imported into a new lane at tick 0, saved as
/// `P.vizdaw` in `dir`: `(session, package, media id)`.
fn saved_demo_with(dir: &Path, src: &Path) -> (Arc<Session>, PathBuf, u64) {
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    import(
        &s,
        src,
        Some(ImportTargetRow {
            seq,
            lane: None,
            tick: 0,
        }),
    );
    let pkg = dir.join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let id = s.media().unwrap()[0].id;
    (s, pkg, id)
}

fn row(s: &Session, id: u64) -> vizdaw_ffi::MediaRow {
    s.media().unwrap().into_iter().find(|r| r.id == id).unwrap()
}

/// Regular files directly in `Media/` (the hidden cache folder aside).
fn media_files(pkg: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(pkg.join("Media"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    v.sort();
    v
}

#[test]
fn pool_rows_rename_remove_and_compact() {
    let dir = tempfile::tempdir().unwrap();
    let used = caf(
        &dir.path().join("used.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let spare = caf(
        &dir.path().join("spare.caf"),
        48_000,
        1,
        100,
        BitDepth::Float32,
        0.25,
    );
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    import(
        &s,
        &used,
        Some(ImportTargetRow {
            seq,
            lane: None,
            tick: 0,
        }),
    );
    import(&s, &spare, None);
    let rows = s.media().unwrap();
    let (u, p) = (rows[0].clone(), rows[1].clone());
    assert_eq!(
        (u.name.as_str(), u.uses, u.used_percent),
        ("used", 1, 100.0)
    );
    assert_eq!((p.name.as_str(), p.uses, p.used_percent), ("spare", 0, 0.0));
    // Trimming 1200 of 4800 frames off the front: 75 % is used.
    let lane = s.lanes(seq).unwrap().last().unwrap().id;
    let ev = s.lane_events(seq, lane).unwrap().events[0].id;
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Trim {
                event: ev,
                front_frames: 1200,
                back_frames: 0,
            },
        },
        None,
    )
    .unwrap();
    assert_eq!(row(&s, u.id).used_percent, 75.0);
    // Users, with the names the delete confirmation lists.
    let users = s.media_users(u.id).unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(
        (users[0].seq, users[0].lane, users[0].event),
        (seq, lane, ev)
    );
    assert_eq!(
        users[0].lane_name,
        s.lanes(seq).unwrap().last().unwrap().name
    );
    assert!(s.media_users(p.id).unwrap().is_empty());
    assert!(Path::new(&s.media_file(u.id).unwrap()).is_file());
    // Rename.
    let snap = s.rename_media(u.id, "  Drums  ".into()).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Rename Media"));
    assert_eq!(row(&s, u.id).name, "Drums");
    assert!(matches!(
        s.rename_media(u.id, "   ".into()),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        s.rename_media(999, "x".into()),
        Err(VizError::Model { .. })
    ));
    // Compact removes the unused item only.
    let snap = s.compact_media().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Compact Media"));
    assert_eq!(
        s.media().unwrap().iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![u.id]
    );
    // Removing an item in use removes its regions too, as one step.
    let snap = s.remove_media(u.id).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Remove Media"));
    assert!(s.media().unwrap().is_empty());
    assert!(s.lane_events(seq, lane).unwrap().events.is_empty());
    s.undo().unwrap();
    assert_eq!(s.media().unwrap().len(), 1);
    assert_eq!(s.lane_events(seq, lane).unwrap().events.len(), 1);
    // Not while an edit is open.
    s.begin_edit("Drag".into()).unwrap();
    assert!(matches!(s.compact_media(), Err(VizError::Model { .. })));
    s.end_edit().unwrap();
    s.close().unwrap();
}

#[test]
fn relink_copies_a_file_into_the_package_as_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("x.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let (s, pkg, id) = saved_demo_with(dir.path(), &src);
    let path = row(&s, id).path;
    let moved = dir.path().join("elsewhere/x.caf");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(pkg.join(&path), &moved).unwrap();
    s.recheck_media().unwrap();
    assert_eq!(row(&s, id).offline, Some(OfflineReasonRow::Missing));
    let j = wait(&s, s.relink_media(id, moved.display().to_string()).unwrap());
    assert_eq!(
        (j.kind, j.state, j.created.clone()),
        (JobKindRow::Relink, JobStateRow::Done, vec![id]),
        "{j:?}"
    );
    let r = row(&s, id);
    // `x.caf` is still named by the undo history: the copy takes `x 2`.
    assert_eq!((r.path.as_str(), r.offline), ("Media/x 2.caf", None));
    assert_eq!(
        hash_file(&pkg.join(&r.path)).unwrap(),
        hash_file(&moved).unwrap()
    );
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Relink Media")
    );
    s.undo().unwrap();
    assert_eq!(row(&s, id).path, path);
    s.close().unwrap();
}

/// Review Focus 4.
#[test]
fn relink_uses_a_file_already_in_the_package() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("y.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let (s, pkg, id) = saved_demo_with(dir.path(), &src);
    let path = row(&s, id).path;
    std::fs::rename(pkg.join(&path), pkg.join("Media/renamed.caf")).unwrap();
    s.recheck_media().unwrap();
    let j = wait(
        &s,
        s.relink_media(id, pkg.join("Media/renamed.caf").display().to_string())
            .unwrap(),
    );
    assert_eq!(j.created, vec![id], "{j:?}");
    assert_eq!(row(&s, id).path, "Media/renamed.caf");
    assert_eq!(
        media_files(&pkg),
        vec!["renamed.caf".to_string()],
        "no copy"
    );
    s.close().unwrap();
}

/// Review Focus 4.
#[test]
fn relink_refuses_short_and_wide_files_and_converts_rates() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("z.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let (s, pkg, id) = saved_demo_with(dir.path(), &src);
    let before = media_files(&pkg);
    // Shorter than the region (4800 frames): refused, its copy removed.
    let short = caf(
        &dir.path().join("short.caf"),
        48_000,
        2,
        1000,
        BitDepth::Float32,
        0.25,
    );
    let j = wait(&s, s.relink_media(id, short.display().to_string()).unwrap());
    assert!(j.created.is_empty() && j.problems.len() == 1, "{j:?}");
    assert!(j.problems[0].contains("shorter"), "{j:?}");
    assert_eq!(media_files(&pkg), before);
    // Four channels, and not audio at all: problems, nothing written.
    let wide = dir.path().join("wide.wav");
    wav(&wide, 4, 9600);
    let j = wait(&s, s.relink_media(id, wide.display().to_string()).unwrap());
    assert!(
        j.created.is_empty() && j.problems[0].contains("two channels"),
        "{j:?}"
    );
    let text = dir.path().join("notes.wav");
    std::fs::write(&text, "not audio").unwrap();
    let j = wait(&s, s.relink_media(id, text.display().to_string()).unwrap());
    assert!(j.created.is_empty() && !j.problems.is_empty(), "{j:?}");
    assert_eq!(media_files(&pkg), before);
    // Another rate: converted to 48 kHz, long enough, relinked.
    let other = caf(
        &dir.path().join("other.caf"),
        44_100,
        2,
        8820,
        BitDepth::Int16,
        0.25,
    );
    let j = wait(&s, s.relink_media(id, other.display().to_string()).unwrap());
    assert_eq!(j.created, vec![id], "{j:?}");
    let r = row(&s, id);
    assert_eq!((r.sample_rate, r.offline), (48_000, None));
    assert!(r.frames.abs_diff(9600) <= 1, "{r:?}");
    // Hostile arguments.
    assert!(matches!(
        s.relink_media(999, other.display().to_string()),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        s.relink_media(id, "relative.caf".into()),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        s.relink_media(id, String::new()),
        Err(VizError::Model { .. })
    ));
    s.close().unwrap();
}

/// Review Focus 4.
#[test]
fn search_prefers_the_hash_and_reports_what_it_misses() {
    let dir = tempfile::tempdir().unwrap();
    let a = caf(
        &dir.path().join("in/a.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let b = caf(
        &dir.path().join("in/b.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.5,
    );
    let s = Session::new_empty().unwrap();
    import(&s, &a, None);
    import(&s, &b, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let rows = s.media().unwrap();
    let (ia, ib) = (rows[0].clone(), rows[1].clone());
    let original = hash_file(&pkg.join(&ia.path)).unwrap();
    // The folder: a's audio under another name (deeper), a file named like
    // a with other audio (found first by name), nothing for b.
    let folder = dir.path().join("found");
    std::fs::create_dir_all(folder.join("zz")).unwrap();
    std::fs::rename(pkg.join(&ia.path), folder.join("zz/renamed.caf")).unwrap();
    caf(
        &folder.join("a/a.caf"),
        48_000,
        2,
        4800,
        BitDepth::Int24,
        0.75,
    );
    std::fs::remove_file(pkg.join(&ib.path)).unwrap();
    s.recheck_media().unwrap();
    let undo_depth_before = s.snapshot().unwrap().undo_label.clone();
    let j = wait(&s, s.search_media(folder.display().to_string()).unwrap());
    assert_eq!(
        (j.kind, j.created.clone()),
        (JobKindRow::Relink, vec![ia.id]),
        "{j:?}"
    );
    assert_eq!(j.problems.len(), 1, "{j:?}");
    assert!(j.problems[0].starts_with("b: not found in"), "{j:?}");
    let r = row(&s, ia.id);
    assert_eq!(r.offline, None);
    assert_eq!(
        hash_file(&pkg.join(&r.path)).unwrap(),
        original,
        "the hash match won"
    );
    assert_eq!(row(&s, ib.id).offline, Some(OfflineReasonRow::Missing));
    // One relink, one undo step.
    s.undo().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label, undo_depth_before);
    // A folder that does not exist: the item is not found.
    let j = wait(
        &s,
        s.search_media(dir.path().join("nowhere").display().to_string())
            .unwrap(),
    );
    assert!(j.created.is_empty() && j.problems.len() == 2, "{j:?}");
    assert!(matches!(
        s.search_media("relative".into()),
        Err(VizError::Model { .. })
    ));
    s.close().unwrap();
}

/// Final review r2: a path the history names is never given to a new file.
#[test]
fn an_import_never_takes_a_name_the_history_references() {
    let dir = tempfile::tempdir().unwrap();
    let a = caf(
        &dir.path().join("a.caf"),
        48_000,
        2,
        100,
        BitDepth::Float32,
        0.25,
    );
    let s = Session::new_empty().unwrap();
    import(&s, &a, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    assert_eq!(s.media().unwrap()[0].path, "Media/a.caf");
    // The file goes missing; the item still names it.
    std::fs::remove_file(pkg.join("Media/a.caf")).unwrap();
    import(&s, &a, None);
    let paths: Vec<String> = s.media().unwrap().into_iter().map(|r| r.path).collect();
    assert_eq!(
        paths,
        vec!["Media/a.caf".to_string(), "Media/a 2.caf".to_string()]
    );
    // Only the redo history names it now: still reserved.
    s.undo().unwrap();
    s.undo().unwrap();
    std::fs::remove_file(pkg.join("Media/a 2.caf")).unwrap();
    let c = caf(
        &dir.path().join("c/a.caf"),
        48_000,
        2,
        100,
        BitDepth::Float32,
        0.5,
    );
    import(&s, &c, None);
    assert_eq!(s.media().unwrap()[0].path, "Media/a 3.caf");
    s.close().unwrap();
}

/// 5a Task 11 review Minor 6: converting an item never claims the name of
/// a missing one.
#[test]
fn a_conversion_never_claims_a_missing_items_name() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("P.vizdaw");
    let add = |d: &Document, path: &str, rate: u32| {
        apply(
            d,
            &Command::Media(MediaCmd::Add {
                path: path.into(),
                sample_rate: rate,
                channels: 2,
                length_frames: 1000,
                content_hash: None,
                name: None,
                origin: None,
            }),
        )
        .unwrap()
        .doc
    };
    let d = add(&Document::new(), "Media/two.caf", 48_000);
    let d = add(&d, "Media/Other/two.caf", 44_100);
    core_model::file::save(&d, &pkg).unwrap();
    // `Media/two.caf` is missing; `Media/Other/two.caf` is at 44.1 kHz.
    caf(
        &pkg.join("Media/Other/two.caf"),
        44_100,
        2,
        1000,
        BitDepth::Int24,
        0.25,
    );
    let s = Session::open(pkg.display().to_string()).unwrap();
    let j = wait(&s, s.convert_media().unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let rows = s.media().unwrap();
    assert_eq!(rows[0].path, "Media/two.caf");
    assert_eq!(rows[0].offline, Some(OfflineReasonRow::Missing));
    assert_eq!(
        rows[1].path, "Media/two 2.caf",
        "not the missing item's name"
    );
    assert_eq!(rows[1].offline, None);
    s.close().unwrap();
}

#[test]
fn the_region_kinds_stay_put() {
    // Sanity for the helpers above: a demo import makes one region.
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("k.caf"),
        48_000,
        1,
        480,
        BitDepth::Float32,
        0.25,
    );
    let (s, _pkg, id) = saved_demo_with(dir.path(), &src);
    let u = s.media_users(id).unwrap();
    let seq = u[0].seq;
    let kinds: Vec<EventKindRow> = s
        .lane_events(seq, u[0].lane)
        .unwrap()
        .events
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, vec![EventKindRow::AudioRegion]);
    s.close().unwrap();
}

/// Preflight I2: names compare case-insensitively. A relink's copy never
/// takes a name the history holds in another case; a rename is display
/// only and never takes (or frees) a file name.
#[test]
fn names_compare_without_case() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("x.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let spare = caf(
        &dir.path().join("y.caf"),
        48_000,
        2,
        100,
        BitDepth::Float32,
        0.5,
    );
    let (s, pkg, id) = saved_demo_with(dir.path(), &src);
    import(&s, &spare, None);
    let other = s.media().unwrap()[1].id;
    assert_eq!(row(&s, id).path, "Media/x.caf");
    // The history names `Media/x.caf`: a file called `X.caf` is copied as
    // `X 2.caf`, never as `X.caf` (the same file on macOS volumes).
    let upper = dir.path().join("up/X.caf");
    std::fs::create_dir_all(upper.parent().unwrap()).unwrap();
    std::fs::copy(&src, &upper).unwrap();
    let j = wait(&s, s.relink_media(id, upper.display().to_string()).unwrap());
    assert_eq!(j.created, vec![id], "{j:?}");
    assert_eq!(row(&s, id).path, "Media/X 2.caf");
    // A rename to another item's file name in another case: allowed, and
    // no path changes.
    let before: Vec<String> = s.media().unwrap().into_iter().map(|r| r.path).collect();
    s.rename_media(other, "X".into()).unwrap();
    assert_eq!(row(&s, other).name, "X");
    let after: Vec<String> = s.media().unwrap().into_iter().map(|r| r.path).collect();
    assert_eq!(before, after);
    assert!(pkg.join("Media/y.caf").is_file());
    s.close().unwrap();
}

/// Preflight M2: a relink to a file at another rate rescales the item's
/// regions by the rate ratio, and the engine's media table names the new
/// file.
#[test]
fn a_relink_at_another_rate_rescales_regions_and_reaches_the_engine() {
    use core_model::command::{AudioCmd, SequenceCmd};
    use core_model::event::{Fade, FadeShape};
    use core_model::ids::{LaneId, MediaId, SeqId};
    use core_model::time::Tick;
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("P.vizdaw");
    let run = |d: &Document, c: Command| apply(d, &c).unwrap().doc;
    let d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "S".into() }),
    );
    let d = run(
        &d,
        Command::Media(MediaCmd::Add {
            path: "Media/a.caf".into(),
            sample_rate: 44_100,
            channels: 2,
            length_frames: 88_200,
            content_hash: None,
            name: None,
            origin: None,
        }),
    );
    let d = run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            media: MediaId(0),
            source_offset: 4410,
            length: 44_100,
            gain_db: 0.0,
            fade_in: Fade {
                length_frames: 441,
                shape: FadeShape::Linear,
            },
            fade_out: Fade {
                length_frames: 882,
                shape: FadeShape::EqualPower,
            },
        }),
    );
    core_model::file::save(&d, &pkg).unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    let r = row(&s, 0);
    assert_eq!(
        (r.sample_rate, r.offline),
        (44_100, Some(OfflineReasonRow::Missing))
    );
    assert!(
        s.debug_engine_media().iter().all(|(m, _)| *m != 0),
        "offline"
    );
    // A 44.1 kHz WAV, converted to the project's 48 kHz.
    let src = dir.path().join("b.wav");
    wav_at(&src, 44_100, 2, 88_200);
    let j = wait(&s, s.relink_media(0, src.display().to_string()).unwrap());
    assert_eq!(
        (j.state, j.created.clone()),
        (JobStateRow::Done, vec![0]),
        "{j:?}"
    );
    let r = row(&s, 0);
    assert_eq!(
        (r.path.as_str(), r.sample_rate, r.frames, r.offline),
        ("Media/b.caf", 48_000, 96_000, None)
    );
    // 48 / 44.1: 4410 → 4800, 44 100 → 48 000, 441 → 480, 882 → 960.
    let seq = s.snapshot().unwrap().sequences[0].id;
    let lane = s.lanes(seq).unwrap()[0].id;
    let e = s.lane_events(seq, lane).unwrap().events[0].clone();
    assert_eq!(
        (
            e.source_offset,
            e.frames,
            e.fade_in.length,
            e.fade_out.length
        ),
        (4800, 48_000, 480, 960)
    );
    let engine = s.debug_engine_media();
    assert!(
        engine
            .iter()
            .any(|(m, p)| *m == 0 && p.ends_with("P.vizdaw/Media/b.caf")),
        "{engine:?}"
    );
    s.close().unwrap();
}

/// Preflight M3: relink, search, convert and format change wait for a
/// running Format, Convert or Relink job (`Busy`); an import does not
/// block them.
#[test]
fn media_jobs_are_busy_while_a_format_convert_or_relink_job_runs() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("x.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let (s, _pkg, id) = saved_demo_with(dir.path(), &src);
    let other = caf(
        &dir.path().join("o.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.5,
    );
    let o = other.display().to_string();
    let f44 = vizdaw_ffi::ProjectFormatRow {
        sample_rate: 44_100,
        depth: vizdaw_ffi::BitDepthRow::Int16,
    };
    let busy = |s: &Session| {
        assert!(matches!(
            s.relink_media(id, o.clone()),
            Err(VizError::Busy { .. })
        ));
        assert!(matches!(
            s.search_media(dir.path().display().to_string()),
            Err(VizError::Busy { .. })
        ));
        assert!(matches!(s.convert_media(), Err(VizError::Busy { .. })));
        assert!(matches!(
            s.set_project_format(f44),
            Err(VizError::Busy { .. })
        ));
    };
    s.debug_hold_jobs(true).unwrap();
    // A relink is running.
    let r = s.relink_media(id, o.clone()).unwrap();
    busy(&s);
    s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait(&s, r).state, JobStateRow::Done);
    // A format change is running.
    s.debug_hold_jobs(true).unwrap();
    let f = s.set_project_format(f44).unwrap();
    busy(&s);
    s.cancel_job(f).unwrap();
    s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait(&s, f).state, JobStateRow::Cancelled);
    // A conversion is running.
    s.debug_hold_jobs(true).unwrap();
    let c = s.convert_media().unwrap();
    busy(&s);
    s.debug_hold_jobs(false).unwrap();
    wait(&s, c);
    // An import is not such a job.
    s.debug_hold_jobs(true).unwrap();
    let i = s.import_audio(vec![o.clone()], None).unwrap();
    let r = s.relink_media(id, o.clone()).unwrap();
    s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait(&s, i).state, JobStateRow::Done);
    assert_eq!(wait(&s, r).state, JobStateRow::Done);
    s.close().unwrap();
}

/// Preflight M7: a VizDAW CAF in `Media/` is used in place only when no
/// history state names its path for other audio; else it is copied.
#[test]
fn an_in_place_relink_never_adopts_a_path_named_for_other_audio() {
    let dir = tempfile::tempdir().unwrap();
    let a = caf(
        &dir.path().join("a.caf"),
        48_000,
        2,
        4800,
        BitDepth::Int24,
        0.25,
    );
    let b = caf(
        &dir.path().join("b.caf"),
        48_000,
        2,
        4800,
        BitDepth::Int24,
        0.5,
    );
    let s = Session::new_empty().unwrap();
    import(&s, &a, None);
    import(&s, &b, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let rows = s.media().unwrap();
    let (ia, ib) = (rows[0].id, rows[1].id);
    assert_eq!(
        (rows[0].path.as_str(), rows[1].path.as_str()),
        ("Media/a.caf", "Media/b.caf")
    );
    // `Media/a.caf` now holds other audio than the history recorded.
    std::fs::remove_file(pkg.join("Media/a.caf")).unwrap();
    caf(
        &pkg.join("Media/a.caf"),
        48_000,
        2,
        4800,
        BitDepth::Int24,
        0.75,
    );
    let now = hash_file(&pkg.join("Media/a.caf")).unwrap();
    let j = wait(
        &s,
        s.relink_media(ib, pkg.join("Media/a.caf").display().to_string())
            .unwrap(),
    );
    assert_eq!(j.created, vec![ib], "{j:?}");
    let r = row(&s, ib);
    assert_eq!(r.path, "Media/a 2.caf", "a copy, not the named path");
    assert_eq!(hash_file(&pkg.join(&r.path)).unwrap(), now);
    // `Media/b.caf` still holds the audio the history names for it: used
    // in place, no copy.
    let files = media_files(&pkg);
    let j = wait(
        &s,
        s.relink_media(ia, pkg.join("Media/b.caf").display().to_string())
            .unwrap(),
    );
    assert_eq!(j.created, vec![ia], "{j:?}");
    assert_eq!(row(&s, ia).path, "Media/b.caf");
    assert_eq!(media_files(&pkg), files, "no copy");
    s.close().unwrap();
}

/// Preflight M16a: two files share the item's name and neither has its
/// audio: the first in walk order wins.
#[test]
fn search_takes_the_first_of_two_files_sharing_the_name() {
    let dir = tempfile::tempdir().unwrap();
    let b = caf(
        &dir.path().join("in/b.caf"),
        48_000,
        2,
        4800,
        BitDepth::Int24,
        0.25,
    );
    let s = Session::new_empty().unwrap();
    import(&s, &b, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let ib = s.media().unwrap()[0].clone();
    std::fs::remove_file(pkg.join(&ib.path)).unwrap();
    s.recheck_media().unwrap();
    let folder = dir.path().join("found");
    let first = caf(
        &folder.join("p/b.caf"),
        48_000,
        2,
        4800,
        BitDepth::Int24,
        0.5,
    );
    caf(
        &folder.join("q/b.caf"),
        48_000,
        2,
        4800,
        BitDepth::Int24,
        0.75,
    );
    let j = wait(&s, s.search_media(folder.display().to_string()).unwrap());
    assert_eq!(j.created, vec![ib.id], "{j:?}");
    let r = row(&s, ib.id);
    assert_eq!(r.offline, None);
    assert_eq!(
        hash_file(&pkg.join(&r.path)).unwrap(),
        hash_file(&first).unwrap()
    );
    s.close().unwrap();
}

/// Task 1 carry: a file with no audio is refused clearly, whether it is
/// copied in or already in `Media/`.
#[test]
fn a_relink_to_an_empty_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("e.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let (s, pkg, id) = saved_demo_with(dir.path(), &src);
    let path = row(&s, id).path;
    let before = media_files(&pkg);
    let empty = caf(
        &dir.path().join("empty.caf"),
        48_000,
        2,
        0,
        BitDepth::Int24,
        0.0,
    );
    let j = wait(&s, s.relink_media(id, empty.display().to_string()).unwrap());
    assert!(j.created.is_empty() && j.problems.len() == 1, "{j:?}");
    assert!(j.problems[0].contains("no audio"), "{j:?}");
    assert_eq!(media_files(&pkg), before);
    // In `Media/`, in the project format, named by nothing.
    let inside = caf(
        &pkg.join("Media/hollow.caf"),
        48_000,
        2,
        0,
        BitDepth::Int24,
        0.0,
    );
    let j = wait(
        &s,
        s.relink_media(id, inside.display().to_string()).unwrap(),
    );
    assert!(j.created.is_empty() && j.problems.len() == 1, "{j:?}");
    assert!(j.problems[0].contains("no audio"), "{j:?}");
    assert!(inside.is_file(), "a file it did not write stays");
    assert_eq!(row(&s, id).path, path);
    s.close().unwrap();
}

/// Fix round 1, review m1: an in-place relink never adopts a file a job
/// has written but not applied: when that job then fails, its cleanup
/// would remove the file the relinked item plays.
#[test]
fn an_in_place_relink_never_adopts_an_unapplied_jobs_file() {
    use vizdaw_ffi::LaneEdit;
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("x.caf"),
        48_000,
        2,
        4800,
        BitDepth::Int24,
        0.25,
    );
    let (s, pkg, id) = saved_demo_with(dir.path(), &src);
    std::fs::remove_file(pkg.join(row(&s, id).path)).unwrap();
    s.recheck_media().unwrap();
    assert_eq!(row(&s, id).offline, Some(OfflineReasonRow::Missing));
    let seq = s.snapshot().unwrap().sequences[0].id;
    let lane_edit = |edit: LaneEdit| s.edit(EditCommand::Lane { edit }, None).unwrap();
    lane_edit(LaneEdit::Add {
        seq,
        name: "Target".into(),
        index: None,
    });
    let lane = s.lanes(seq).unwrap().last().unwrap().id;
    // An import into that lane; the lane goes before it applies, so the
    // import fails and removes the file it wrote.
    let k = caf(
        &dir.path().join("k.caf"),
        48_000,
        2,
        4800,
        BitDepth::Int24,
        0.5,
    );
    s.debug_hold_jobs(true).unwrap();
    let imp = s
        .import_audio(
            vec![k.display().to_string()],
            Some(ImportTargetRow {
                seq,
                lane: Some(lane),
                tick: 0,
            }),
        )
        .unwrap();
    lane_edit(LaneEdit::Remove { seq, lane });
    // An open edit keeps finished jobs from applying.
    s.begin_edit("Drag".into()).unwrap();
    s.debug_hold_jobs(false).unwrap();
    let settle = |s: &Session| {
        let until = Instant::now() + Duration::from_secs(60);
        while !s.debug_jobs_settled() {
            assert!(Instant::now() < until, "jobs still running");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    settle(&s);
    let linked = pkg.join("Media/k.caf");
    assert!(linked.is_file(), "the import linked its file");
    let r = s.relink_media(id, linked.display().to_string()).unwrap();
    settle(&s);
    s.end_edit().unwrap();
    assert_eq!(s.job(imp).unwrap().state, JobStateRow::Failed);
    let j = s.job(r).unwrap();
    assert_eq!(j.created, vec![id], "{j:?}");
    let got = row(&s, id);
    assert_eq!(got.offline, None, "{got:?}");
    assert_ne!(got.path, "Media/k.caf", "a copy, not the import's file");
    assert!(pkg.join(&got.path).is_file());
    s.close().unwrap();
}

/// Fix round 1, review m2: a FIFO, a folder or another non-regular file
/// is refused at once (a FIFO would block the job forever), and later
/// jobs still run.
#[test]
fn non_regular_files_are_refused_promptly() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("x.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let (s, pkg, id) = saved_demo_with(dir.path(), &src);
    let fifo_dir = dir.path().join("fifo");
    std::fs::create_dir_all(&fifo_dir).unwrap();
    let fifo = fifo_dir.join("x.caf");
    let ok = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success();
    assert!(ok, "mkfifo");
    let t = Instant::now();
    // Locate…: refused before any job starts.
    let e = s.relink_media(id, fifo.display().to_string());
    assert!(matches!(e, Err(VizError::Model { .. })), "{e:?}");
    let e = s.relink_media(id, fifo_dir.display().to_string());
    assert!(matches!(e, Err(VizError::Model { .. })), "{e:?}");
    // Import: the FIFO fails alone.
    let j = wait(
        &s,
        s.import_audio(vec![fifo.display().to_string()], None)
            .unwrap(),
    );
    assert!(j.created.is_empty() && j.problems.len() == 1, "{j:?}");
    // Search: a FIFO named like the missing item is not a candidate.
    std::fs::remove_file(pkg.join(row(&s, id).path)).unwrap();
    s.recheck_media().unwrap();
    let j = wait(&s, s.search_media(fifo_dir.display().to_string()).unwrap());
    assert!(
        j.created.is_empty() && j.problems[0].starts_with("x: not found"),
        "{j:?}"
    );
    assert!(t.elapsed() < Duration::from_secs(20), "{:?}", t.elapsed());
    // Later jobs still run.
    let j = wait(&s, s.relink_media(id, src.display().to_string()).unwrap());
    assert_eq!(j.created, vec![id], "{j:?}");
    s.close().unwrap();
}

/// Runs `idle` until the overview worker has been idle across two ticks.
fn settle(s: &Session) {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        s.idle().unwrap();
        if s.debug_overviews_idle() {
            s.idle().unwrap();
            if s.debug_overviews_idle() {
                return;
            }
        }
        assert!(Instant::now() < until, "the overview worker never settled");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Final review I1: searching a folder that holds the package never
/// brings a `Changed` item back with the changed audio in its own file,
/// nor with a copy of it under the same name elsewhere: a changed item
/// matches only by content hash.
#[test]
fn search_never_relinks_a_changed_item_to_the_changed_audio() {
    let dir = tempfile::tempdir().unwrap();
    let srcdir = tempfile::tempdir().unwrap();
    let src = caf(
        &srcdir.path().join("x.caf"),
        48_000,
        2,
        9600,
        BitDepth::Float32,
        0.25,
    );
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    settle(&s);
    let item = s.media().unwrap()[0].clone();
    // The same name and length, other audio, in place (in the project
    // format, so only the content differs).
    let file = pkg.join(&item.path);
    let original = srcdir.path().join("original.caf");
    std::fs::rename(&file, &original).unwrap();
    caf(&file, 48_000, 2, 9600, BitDepth::Int24, 0.5);
    // And a copy of the changed audio by the same name beside the package.
    std::fs::copy(&file, dir.path().join(file.file_name().unwrap())).unwrap();
    s.recheck_media().unwrap();
    settle(&s);
    assert_eq!(row(&s, item.id).offline, Some(OfflineReasonRow::Changed));
    let before = media_files(&pkg);
    let j = wait(
        &s,
        s.search_media(dir.path().display().to_string()).unwrap(),
    );
    assert!(j.created.is_empty(), "{j:?}");
    assert_eq!(j.problems.len(), 1, "{j:?}");
    assert!(j.problems[0].contains("not found in"), "{j:?}");
    assert_eq!(row(&s, item.id).offline, Some(OfflineReasonRow::Changed));
    assert_eq!(media_files(&pkg), before, "nothing copied in");
    // The original audio, under any name, still brings it back.
    std::fs::create_dir_all(dir.path().join("deep")).unwrap();
    std::fs::copy(&original, dir.path().join("deep/other.caf")).unwrap();
    let j = wait(
        &s,
        s.search_media(dir.path().display().to_string()).unwrap(),
    );
    assert_eq!(j.created, vec![item.id], "{j:?}");
    assert_eq!(row(&s, item.id).offline, None);
    s.close().unwrap();
}

/// Task 6 review m5: two missing items of one audio share the one file a
/// search finds.
#[test]
fn search_relinks_every_item_of_a_found_hash() {
    let dir = tempfile::tempdir().unwrap();
    let a = caf(
        &dir.path().join("in/a.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let s = Session::new_empty().unwrap();
    import(&s, &a, None);
    import(&s, &a, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let rows = s.media().unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(
        hash_file(&pkg.join(&rows[0].path)).unwrap(),
        hash_file(&pkg.join(&rows[1].path)).unwrap()
    );
    let folder = dir.path().join("found");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::rename(pkg.join(&rows[0].path), folder.join("only.caf")).unwrap();
    std::fs::remove_file(pkg.join(&rows[1].path)).unwrap();
    s.recheck_media().unwrap();
    let j = wait(&s, s.search_media(folder.display().to_string()).unwrap());
    assert!(j.problems.is_empty(), "{j:?}");
    let mut created = j.created.clone();
    created.sort_unstable();
    let mut want = vec![rows[0].id, rows[1].id];
    want.sort_unstable();
    assert_eq!(created, want, "{j:?}");
    for r in &rows {
        assert_eq!(row(&s, r.id).offline, None);
    }
    s.close().unwrap();
}

/// Final review I2 / Task 6 review m9: a search of a wide tree can be
/// cancelled, and close never waits for the walk.
#[test]
fn a_search_can_be_cancelled_mid_walk() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("in/a.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let (s, pkg, id) = saved_demo_with(dir.path(), &src);
    std::fs::remove_file(pkg.join(row(&s, id).path)).unwrap();
    s.recheck_media().unwrap();
    // A wide tree of near-empty folders.
    let tree = dir.path().join("tree");
    for a in 0..40 {
        for b in 0..40 {
            std::fs::create_dir_all(tree.join(format!("{a}/{b}/x/y"))).unwrap();
        }
    }
    let job = s.search_media(tree.display().to_string()).unwrap();
    s.cancel_job(job).unwrap();
    let t = Instant::now();
    let j = wait(&s, job);
    assert_eq!(j.state, JobStateRow::Cancelled, "{j:?}");
    assert!(t.elapsed() < Duration::from_secs(20), "{:?}", t.elapsed());
    assert_eq!(row(&s, id).offline, Some(OfflineReasonRow::Missing));
    let t = Instant::now();
    s.close().unwrap();
    assert!(t.elapsed() < Duration::from_secs(20), "{:?}", t.elapsed());
}

/// Task 6 review m7: a job that finished before a relink, search or
/// conversion starts is applied there and still reported by `idle`.
#[test]
fn a_job_applied_when_a_search_starts_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let a = caf(
        &dir.path().join("in/a.caf"),
        48_000,
        2,
        4800,
        BitDepth::Float32,
        0.25,
    );
    let s = Session::new_empty().unwrap();
    while s.idle().unwrap().media_changed {}
    s.import_audio(vec![a.display().to_string()], None).unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        // Refused (nothing is missing), but only after applying what
        // finished.
        assert!(s.search_media(dir.path().display().to_string()).is_err());
        if !s.media().unwrap().is_empty() {
            break;
        }
        assert!(Instant::now() < until, "the import never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(s.idle().unwrap().media_changed);
    s.close().unwrap();
}
