//! Waveform overviews in the session (plan 5b Task 4, Rulings 2–4, 17):
//! built in the background, served at any zoom, verified by content hash.

use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vizdaw_ffi::{
    AudioEdit, BitDepthRow, EditCommand, EventKindRow, FadeRow, FadeShapeRow, ImportTargetRow,
    JobRow, JobStateRow, OfflineReasonRow, OverviewStateRow, ProjectFormatRow, SequenceEdit,
    Session, TempoPointRow, VizError,
};

/// A 48 kHz CAF in `depth` holding `samples` (interleaved, `ch` channels).
fn caf(path: &Path, ch: u16, depth: BitDepth, samples: &[f32]) -> PathBuf {
    let mut w = CafWriter::create(path, 48_000, ch, depth, WriteMode::Normal).unwrap();
    w.write(samples).unwrap();
    w.finish().unwrap();
    path.to_path_buf()
}

/// A stereo ramp of `frames` frames: channel 0 repeats every 1000 frames
/// (±0.4), channel 1 every 77 (±0.25).
fn ramp(frames: usize) -> Vec<f32> {
    let mut s = Vec::with_capacity(frames * 2);
    for f in 0..frames {
        s.push(((f % 1000) as f32 / 1000.0 - 0.5) * 0.8);
        s.push(((f % 77) as f32 / 77.0 - 0.5) * 0.5);
    }
    s
}

/// Every sample of the CAF at `path`, interleaved.
fn samples(path: &Path) -> Vec<f32> {
    let r = CafReader::open(path).unwrap();
    let ch = usize::from(r.info().channels);
    let mut out = vec![0.0f32; r.info().frames as usize * ch];
    let mut scratch = Vec::new();
    assert_eq!(
        r.read(0, &mut out, &mut scratch).unwrap() as u64,
        r.info().frames
    );
    out
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

fn import(s: &Arc<Session>, path: &Path, target: Option<ImportTargetRow>) -> JobRow {
    let id = s
        .import_audio(vec![path.display().to_string()], target)
        .unwrap();
    let j = wait(s, id);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    j
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

/// The only region of the last lane of the demo's first sequence:
/// `(seq, event, tick, duration ticks)`.
fn last_region(s: &Session) -> (u64, u64, i64, i64) {
    let seq = s.snapshot().unwrap().sequences[0].id;
    let lane = s.lanes(seq).unwrap().last().unwrap().id;
    let ev = s.lane_events(seq, lane).unwrap();
    let r = ev
        .events
        .iter()
        .find(|e| e.kind == EventKindRow::AudioRegion)
        .unwrap();
    (seq, r.id, r.tick, r.duration)
}

fn brute(s: &[f32], c: usize, from: usize, to: usize) -> (f32, f32) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for f in from..to {
        lo = lo.min(s[f * 2 + c]);
        hi = hi.max(s[f * 2 + c]);
    }
    (lo, hi)
}

#[test]
fn an_import_draws_at_every_zoom() {
    let dir = tempfile::tempdir().unwrap();
    // 102 400 frames: 100 columns of 1024 frames, four level-256 peaks each.
    let src = caf(
        &dir.path().join("ramp.caf"),
        2,
        BitDepth::Float32,
        &ramp(102_400),
    );
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    settle(&s);
    let m = s.media().unwrap()[0].clone();
    // The stored file (24-bit) is what the overview describes.
    let stored = samples(&pkg.join(&m.path));
    let o = s.overview(m.id, 0, 102_400, 100).unwrap();
    assert_eq!(
        (o.state, o.channels, o.columns),
        (OverviewStateRow::Ready, 2, 100)
    );
    for c in 0..100usize {
        for k in 0..2usize {
            let (lo, hi) = brute(&stored, k, c * 1024, (c + 1) * 1024);
            let (a, b) = (o.min[k * 100 + c], o.max[k * 100 + c]);
            assert!(
                a <= lo && lo - a < 1e-4,
                "column {c} channel {k}: {a} vs {lo}"
            );
            assert!(
                b >= hi && b - hi < 1e-4,
                "column {c} channel {k}: {b} vs {hi}"
            );
        }
    }
    // One frame per column: the samples themselves, once the worker read them.
    let first = s.overview(m.id, 5000, 5100, 100).unwrap();
    assert!(
        matches!(
            first.state,
            OverviewStateRow::Coarse | OverviewStateRow::Ready
        ),
        "{:?}",
        first.state
    );
    settle(&s);
    let exact = s.overview(m.id, 5000, 5100, 100).unwrap();
    assert_eq!(exact.state, OverviewStateRow::Ready);
    for c in 0..100usize {
        for k in 0..2usize {
            let v = stored[(5000 + c) * 2 + k];
            assert_eq!(
                (exact.min[k * 100 + c], exact.max[k * 100 + c]),
                (v, v),
                "column {c}"
            );
        }
    }
    s.close().unwrap();
}

#[test]
fn region_waveform_follows_gain_fades_and_tempo() {
    let dir = tempfile::tempdir().unwrap();
    // Mono, every sample 0.5 (exact at 24 bits), 2 s.
    let src = caf(
        &dir.path().join("flat.caf"),
        1,
        BitDepth::Float32,
        &vec![0.5; 96_000],
    );
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    import(
        &s,
        &src,
        Some(ImportTargetRow {
            seq,
            lane: None,
            tick: 0,
        }),
    );
    let (seq, ev, tick, dur) = last_region(&s);
    // +6.0206 dB doubles; a 1 s linear fade in.
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::SetGain {
                event: ev,
                gain_db: 6.0206,
            },
        },
        None,
    )
    .unwrap();
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::SetFades {
                event: ev,
                fade_in: FadeRow {
                    length: 48_000,
                    shape: FadeShapeRow::Linear,
                },
                fade_out: FadeRow {
                    length: 0,
                    shape: FadeShapeRow::Linear,
                },
            },
        },
        None,
    )
    .unwrap();
    // A tempo change at the region's midpoint (1 s in): ticks are no longer
    // linear in frames (preflight M11). 120 bpm, then 240.
    let mid = s.seconds_at(seq, vec![tick]).unwrap()[0];
    assert!(mid.abs() < 1e-9 && tick == 0, "the region starts at 0");
    let ppq = 15_360;
    s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::SetTempo {
                seq,
                tempo: vec![
                    TempoPointRow {
                        tick: 0,
                        bpm: 120.0,
                        ramp: false,
                    },
                    TempoPointRow {
                        tick: 2 * ppq,
                        bpm: 240.0,
                        ramp: false,
                    },
                ],
            },
        },
        None,
    )
    .unwrap();
    let _ = dur;
    settle(&s);
    // 200 columns over 8 beats: the region ends at 6 (2 at 120 bpm, then 4
    // at 240), so the last quarter is past its end.
    let span = 8 * ppq;
    let w = s.region_waveform(ev, tick, tick + span, 200).unwrap();
    assert_eq!(
        (w.state, w.channels, w.columns),
        (OverviewStateRow::Ready, 1, 200)
    );
    let step = span as f64 / 200.0;
    let at = |f: f64| -> Vec<i64> {
        (0..200)
            .map(|c| tick + ((c as f64 + f) * step) as i64)
            .collect()
    };
    let secs = s.seconds_at(seq, at(0.5)).unwrap();
    let lo = s.seconds_at(seq, at(0.0)).unwrap();
    let hi = s.seconds_at(seq, at(1.0)).unwrap();
    let start = s.seconds_at(seq, vec![tick]).unwrap()[0];
    let mut past = 0;
    for (c, sec) in secs.iter().enumerate() {
        let r = (sec - start) * 48_000.0;
        // The column holding the region's end is drawn, but its centre
        // may lie past it: skip it.
        let (a, b) = ((lo[c] - start) * 48_000.0, (hi[c] - start) * 48_000.0);
        if a < 96_000.0 && b > 96_000.0 {
            continue;
        }
        let want = if a >= 96_000.0 {
            past += 1;
            0.0
        } else if r < 48_000.0 {
            r / 48_000.0
        } else {
            1.0
        };
        assert!(
            (f64::from(w.max[c]) - want).abs() < 0.03,
            "column {c} at frame {r}: {} vs {want}",
            w.max[c]
        );
    }
    // Linear in ticks the region would end at column 100; through the tempo
    // map it ends at column 150.
    assert!((49..=50).contains(&past), "{past} silent columns");
    s.close().unwrap();
}

/// Review Focus 2.
#[test]
fn a_same_length_replacement_goes_offline_changed_and_back() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("x.caf"),
        2,
        BitDepth::Float32,
        &vec![0.25; 9600],
    );
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    settle(&s);
    assert_eq!(s.media().unwrap()[0].offline, None);
    // Another file in the project format, of the same length, other audio.
    let file = pkg.join(&s.media().unwrap()[0].path);
    let kept = dir.path().join("kept.caf");
    std::fs::rename(&file, &kept).unwrap();
    caf(&file, 2, BitDepth::Int24, &vec![0.5; 9600]);
    s.recheck_media().unwrap();
    settle(&s);
    let row = s.media().unwrap()[0].clone();
    assert_eq!(row.offline, Some(OfflineReasonRow::Changed));
    let w = s.overview(row.id, 0, 4800, 10).unwrap();
    assert_eq!(w.state, OverviewStateRow::Offline);
    // A changed file must be relinked before a format change converts it
    // (preflight M16).
    let id = s
        .set_project_format(ProjectFormatRow {
            sample_rate: 48_000,
            depth: BitDepthRow::Int16,
        })
        .unwrap();
    let j = wait(&s, id);
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    assert!(
        j.message.unwrap_or_default().contains("relinked first"),
        "says why"
    );
    // The original comes back.
    std::fs::remove_file(&file).unwrap();
    std::fs::rename(&kept, &file).unwrap();
    s.recheck_media().unwrap();
    settle(&s);
    assert_eq!(s.media().unwrap()[0].offline, None);
    s.close().unwrap();
}

/// Review Focus 5.
#[test]
fn waveform_requests_survive_hostile_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("h.caf"), 2, BitDepth::Float32, &ramp(4800));
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    import(
        &s,
        &src,
        Some(ImportTargetRow {
            seq,
            lane: None,
            tick: 0,
        }),
    );
    let (seq, ev, tick, dur) = last_region(&s);
    let media = s.media().unwrap()[0].id;
    let model = |r: Result<vizdaw_ffi::OverviewRows, VizError>| {
        assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}")
    };
    for cols in [0, 8193, u32::MAX] {
        model(s.region_waveform(ev, tick, tick + dur, cols));
        model(s.overview(media, 0, 100, cols));
    }
    model(s.region_waveform(ev, tick + 10, tick + 10, 10));
    model(s.region_waveform(ev, tick + 10, tick, 10));
    model(s.overview(media, 100, 100, 10));
    model(s.region_waveform(999_999, 0, 10, 10));
    model(s.overview(999_999, 0, 10, 10));
    // A note of the demo is not a region.
    let lane0 = s.lanes(seq).unwrap()[0].id;
    let note = s
        .lane_events(seq, lane0)
        .unwrap()
        .events
        .iter()
        .find(|e| e.kind == EventKindRow::Note)
        .map(|e| e.id)
        .unwrap();
    model(s.region_waveform(note, 0, 100, 10));
    // Absurd but well-formed ranges answer, with silence outside the region.
    let w = s.region_waveform(ev, i64::MIN, i64::MAX, 8192).unwrap();
    assert_eq!((w.min.len(), w.max.len()), (2 * 8192, 2 * 8192));
    assert!(w.min.iter().chain(&w.max).all(|v| v.is_finite()));
    let o = s.overview(media, 0, u64::MAX, 10).unwrap();
    assert_eq!(o.columns, 10);
    // The region goes (Undo the import): its ID is unknown now.
    s.undo().unwrap();
    model(s.region_waveform(ev, tick, tick + dur, 10));
    s.close().unwrap();
}

/// Sets every `content_hash` in a project's JSON to `hash`.
fn set_hashes(v: &mut serde_json::Value, hash: &str) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if k == "content_hash" {
                    *x = serde_json::Value::String(hash.into());
                } else {
                    set_hashes(x, hash);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(|x| set_hashes(x, hash)),
        _ => {}
    }
}

/// Every file under `dir`, recursively.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// Review Focus 3.
#[test]
fn a_hostile_content_hash_never_names_a_path() {
    for hostile in ["../../../evil", "AB", &"f".repeat(100_000)] {
        let dir = tempfile::tempdir().unwrap();
        let src = caf(&dir.path().join("y.caf"), 2, BitDepth::Float32, &ramp(3000));
        let s = Session::new_empty().unwrap();
        import(&s, &src, None);
        let pkg = dir.path().join("P.vizdaw");
        s.save_as(pkg.display().to_string()).unwrap();
        s.close().unwrap();
        let pj = pkg.join("project.json");
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&pj).unwrap()).unwrap();
        set_hashes(&mut v, hostile);
        std::fs::write(&pj, serde_json::to_string(&v).unwrap()).unwrap();
        let s = Session::open(pkg.display().to_string()).unwrap();
        settle(&s);
        assert_eq!(
            s.media().unwrap()[0].offline,
            Some(OfflineReasonRow::Changed),
            "{hostile:.20}"
        );
        for f in walk(dir.path()) {
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            assert!(!name.contains("evil"), "{f:?}");
            if f.parent().unwrap().ends_with(".overviews") {
                let stem = name.trim_end_matches(".vzov");
                assert!(
                    stem.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                    "{f:?}"
                );
            }
        }
        s.close().unwrap();
    }
}

#[test]
fn save_as_carries_the_overviews_and_a_damaged_one_is_rebuilt() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("z.caf"), 2, BitDepth::Float32, &ramp(5000));
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    let a = dir.path().join("A.vizdaw");
    s.save_as(a.display().to_string()).unwrap();
    settle(&s);
    let b = dir.path().join("B.vizdaw");
    s.save_as(b.display().to_string()).unwrap();
    let list = |p: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(p.join("Media/.overviews"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "vzov"))
            .collect()
    };
    assert_eq!(list(&b).len(), 1, "Save As copied the overview");
    s.close().unwrap();
    // Garbage in the cache: refused, rebuilt, served.
    let ov = list(&b).remove(0);
    std::fs::write(&ov, b"garbage").unwrap();
    let s = Session::open(b.display().to_string()).unwrap();
    settle(&s);
    let m = s.media().unwrap()[0].clone();
    assert_eq!(m.offline, None);
    // 1000 frames per column: served from the overview itself.
    assert_eq!(
        s.overview(m.id, 0, 5000, 5).unwrap().state,
        OverviewStateRow::Ready
    );
    assert!(
        core_media::overview::Overview::read(&ov).is_ok(),
        "rewritten"
    );
    s.close().unwrap();
}

#[test]
fn reads_never_wait_for_the_worker() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("w.caf"),
        2,
        BitDepth::Float32,
        &ramp(48_000),
    );
    let s = Session::new_empty().unwrap();
    // The worker is held: the new item's overview (written by the import)
    // is not loaded yet.
    s.debug_hold_overviews(true);
    import(&s, &src, None);
    let m = s.media().unwrap()[0].clone();
    let started = Instant::now();
    let o = s.overview(m.id, 0, 48_000, 100).unwrap();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(o.state, OverviewStateRow::Pending);
    assert!(o.min.iter().chain(&o.max).all(|v| *v == 0.0));
    s.debug_hold_overviews(false);
    settle(&s);
    assert_eq!(
        s.overview(m.id, 0, 48_000, 100).unwrap().state,
        OverviewStateRow::Ready
    );
    s.close().unwrap();
}

/// Preflight I1: once an item names another file or hash, its previous
/// overview is never served; it is `Pending` until the new one is loaded.
#[test]
fn a_new_file_for_an_item_is_pending_never_the_old_overview() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("v.caf"), 2, BitDepth::Float32, &ramp(9600));
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    settle(&s);
    let before = s.media().unwrap()[0].clone();
    assert_eq!(
        s.overview(before.id, 0, 9600, 10).unwrap().state,
        OverviewStateRow::Ready
    );
    // The worker is held; a format change gives the item a new file with
    // other audio (16-bit) and another hash.
    s.debug_hold_overviews(true);
    let id = s
        .set_project_format(ProjectFormatRow {
            sample_rate: 48_000,
            depth: BitDepthRow::Int16,
        })
        .unwrap();
    assert_eq!(wait(&s, id).state, JobStateRow::Done);
    s.idle().unwrap();
    let after = s.media().unwrap()[0].clone();
    assert_eq!(after.id, before.id);
    assert_ne!(after.path, before.path, "a new file");
    let o = s.overview(after.id, 0, 9600, 10).unwrap();
    assert_eq!(o.state, OverviewStateRow::Pending, "never the old file's");
    assert!(o.min.iter().chain(&o.max).all(|v| *v == 0.0));
    // Undo goes back to the first file: still pending while held.
    s.undo().unwrap();
    s.idle().unwrap();
    assert_eq!(
        s.overview(before.id, 0, 9600, 10).unwrap().state,
        OverviewStateRow::Pending
    );
    s.debug_hold_overviews(false);
    settle(&s);
    assert_eq!(
        s.overview(before.id, 0, 9600, 10).unwrap().state,
        OverviewStateRow::Ready
    );
    s.close().unwrap();
}

/// Preflight M10: a panic in an overview task drops that task; the worker
/// lives on and serves the next request.
#[test]
fn a_panicking_overview_task_leaves_the_worker_alive() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("p.caf"), 2, BitDepth::Float32, &ramp(9600));
    let s = Session::new_empty().unwrap();
    s.debug_hold_overviews(true);
    import(&s, &src, None);
    let m = s.media().unwrap()[0].clone();
    assert_eq!(
        s.overview(m.id, 0, 9600, 10).unwrap().state,
        OverviewStateRow::Pending
    );
    // The next task panics.
    s.debug_panic_overviews();
    s.debug_hold_overviews(false);
    settle(&s);
    assert_eq!(
        s.overview(m.id, 0, 9600, 10).unwrap().state,
        OverviewStateRow::Pending,
        "the panicking task was dropped"
    );
    // A recheck asks again: the same worker answers.
    s.recheck_media().unwrap();
    settle(&s);
    assert_eq!(
        s.overview(m.id, 0, 9600, 10).unwrap().state,
        OverviewStateRow::Ready
    );
    s.close().unwrap();
}

/// Review I1: a request past the end of the file answers at once and asks
/// the worker for nothing, so it never loops through `overviews_changed`.
#[test]
fn a_request_past_the_end_answers_at_once_and_asks_for_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("e.caf"), 2, BitDepth::Float32, &ramp(9600));
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    settle(&s);
    let m = s.media().unwrap()[0].clone();
    for (from, to) in [(9600, 9700), (50_000, 60_000), (u64::MAX - 1, u64::MAX)] {
        let o = s.overview(m.id, from, to, 100).unwrap();
        assert_eq!(o.state, OverviewStateRow::Ready, "{from}..{to}");
        assert!(o.min.iter().chain(&o.max).all(|v| *v == 0.0));
        assert!(s.debug_overviews_idle(), "nothing queued for {from}..{to}");
        assert!(!s.idle().unwrap().overviews_changed);
    }
    s.close().unwrap();
}

/// Sets `channels` of every media item in a project's JSON to `n`.
fn set_channels(v: &mut serde_json::Value, n: u64) {
    match v {
        serde_json::Value::Object(m) => {
            if m.contains_key("content_hash") && m.contains_key("channels") {
                m.insert("channels".into(), n.into());
            }
            m.values_mut().for_each(|x| set_channels(x, n));
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(|x| set_channels(x, n)),
        _ => {}
    }
}

/// Review I2: a project claiming 65 535 channels never sizes a read by it.
#[test]
fn a_hostile_channel_count_never_sizes_a_read() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("c.caf"), 2, BitDepth::Float32, &ramp(3000));
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.close().unwrap();
    let pj = pkg.join("project.json");
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&pj).unwrap()).unwrap();
    set_channels(&mut v, 65_535);
    std::fs::write(&pj, serde_json::to_string(&v).unwrap()).unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    let m = s.media().unwrap()[0].clone();
    assert!(m.offline.is_some(), "the file has 2 channels, not 65 535");
    let o = s.overview(m.id, 0, 1, 8192).unwrap();
    assert_eq!(o.state, OverviewStateRow::Offline);
    assert!((1..=2).contains(&o.channels), "{}", o.channels);
    assert_eq!(o.min.len(), usize::from(o.channels) * 8192);
    assert_eq!(o.max.len(), o.min.len());
    s.close().unwrap();
}

/// Controller ruling (fix round 1): a recheck, or a Save As, that leaves an
/// item's path and content hash as they were keeps its waveform drawn.
#[test]
fn a_recheck_of_an_unchanged_file_keeps_its_waveform() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("k.caf"), 2, BitDepth::Float32, &ramp(9600));
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    s.save_as(dir.path().join("A.vizdaw").display().to_string())
        .unwrap();
    settle(&s);
    let m = s.media().unwrap()[0].clone();
    assert_eq!(
        s.overview(m.id, 0, 9600, 10).unwrap().state,
        OverviewStateRow::Ready
    );
    s.debug_hold_overviews(true);
    s.recheck_media().unwrap();
    assert_eq!(
        s.overview(m.id, 0, 9600, 10).unwrap().state,
        OverviewStateRow::Ready,
        "a recheck"
    );
    s.save_as(dir.path().join("B.vizdaw").display().to_string())
        .unwrap();
    assert_eq!(
        s.overview(m.id, 0, 9600, 10).unwrap().state,
        OverviewStateRow::Ready,
        "a Save As"
    );
    s.debug_hold_overviews(false);
    settle(&s);
    assert_eq!(
        s.overview(m.id, 0, 9600, 10).unwrap().state,
        OverviewStateRow::Ready
    );
    s.close().unwrap();
}

/// Review m1: a verification followed by a mismatch, both found before one
/// `idle`, leaves the item `Changed`.
#[test]
fn a_replacement_between_two_rechecks_is_not_lost() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(
        &dir.path().join("r.caf"),
        2,
        BitDepth::Float32,
        &vec![0.25; 9600],
    );
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    settle(&s);
    // Waits for the worker without `idle`, so findings pile up.
    let drain = |s: &Session| {
        let until = Instant::now() + Duration::from_secs(60);
        while !s.debug_overviews_idle() {
            assert!(Instant::now() < until, "the worker never settled");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    s.recheck_media().unwrap();
    drain(&s);
    let file = pkg.join(&s.media().unwrap()[0].path);
    std::fs::remove_file(&file).unwrap();
    caf(&file, 2, BitDepth::Int24, &vec![0.5; 9600]);
    s.recheck_media().unwrap();
    drain(&s);
    s.idle().unwrap();
    settle(&s);
    assert_eq!(
        s.media().unwrap()[0].offline,
        Some(OfflineReasonRow::Changed)
    );
    s.close().unwrap();
}
