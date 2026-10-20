//! Region menu commands (plan 5b Task 5, Rulings 5 and 6): Normalize from
//! the overview, fades to the cursor, Remove Fades.

use core_media::caf::{CafReader, CafWriter, WriteMode, hash_file};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vizdaw_ffi::{
    AudioEdit, BitDepthRow, EditCommand, EventKindRow, EventRow, ImportTargetRow, JobStateRow,
    ProjectFormatRow, Session, VizError,
};

fn caf(path: &Path, ch: u16, samples: &[f32]) -> PathBuf {
    let mut w = CafWriter::create(path, 48_000, ch, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(samples).unwrap();
    w.finish().unwrap();
    path.to_path_buf()
}

/// The demo with `src` imported into a new lane at tick 0: `(session,
/// sequence, lane, region)`.
fn demo_with(src: &Path) -> (Arc<Session>, u64, u64, u64) {
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let id = s
        .import_audio(
            vec![src.display().to_string()],
            Some(ImportTargetRow {
                seq,
                lane: None,
                tick: 0,
            }),
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while s.job(id).unwrap().state == JobStateRow::Running {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    let lane = s.lanes(seq).unwrap().last().unwrap().id;
    let ev = region(&s, seq, lane).id;
    (s, seq, lane, ev)
}

fn region(s: &Session, seq: u64, lane: u64) -> EventRow {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .into_iter()
        .find(|e| e.kind == EventKindRow::AudioRegion)
        .unwrap()
}

/// A note of the demo's first lane.
fn a_note(s: &Session, seq: u64) -> u64 {
    let lane = s.lanes(seq).unwrap()[0].id;
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .find(|e| e.kind == EventKindRow::Note)
        .unwrap()
        .id
}

/// Saves the session into `dir` and answers the media file's path, its
/// `hash_file` and its bytes (preflight M13: media stays write-once).
fn saved_media(s: &Session, dir: &Path) -> (PathBuf, String, Vec<u8>) {
    let pkg = dir.join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let file = pkg.join(&s.media().unwrap()[0].path);
    let hash = hash_file(&file).unwrap();
    let bytes = std::fs::read(&file).unwrap();
    (file, hash, bytes)
}

fn assert_untouched(media: &(PathBuf, String, Vec<u8>)) {
    let (file, hash, bytes) = media;
    assert_eq!(&hash_file(file).unwrap(), hash, "media is write-once");
    assert!(
        &std::fs::read(file).unwrap() == bytes,
        "media is write-once"
    );
}

#[test]
fn normalize_reaches_the_peak_from_the_overview() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("quarter.caf"), 2, &vec![0.25; 9600]);
    let (s, seq, lane, ev) = demo_with(&src);
    let media = saved_media(&s, dir.path());
    let note = a_note(&s, seq);
    let snap = s.normalize_regions(vec![note, ev], -1.0).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Normalize"));
    // −1 − 20·log10(0.25) = 11.04 dB.
    let g = region(&s, seq, lane).gain_db;
    assert!((g - 11.0412).abs() < 0.01, "{g}");
    s.undo().unwrap();
    assert_eq!(region(&s, seq, lane).gain_db, 0.0);
    assert_untouched(&media);
    s.close().unwrap();
}

#[test]
fn normalize_measures_only_what_the_region_plays() {
    let dir = tempfile::tempdir().unwrap();
    // 0.1 everywhere, 0.9 at frame 50 (cut off below), 0.5 at frame 2000.
    let mut v = vec![0.1f32; 4800];
    v[50] = 0.9;
    v[2000] = 0.5;
    let src = caf(&dir.path().join("spike.caf"), 1, &v);
    let (s, seq, lane, ev) = demo_with(&src);
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Trim {
                event: ev,
                front_frames: 100,
                back_frames: 0,
            },
        },
        None,
    )
    .unwrap();
    s.normalize_regions(vec![ev], 0.0).unwrap();
    // 0 − 20·log10(0.5) = 6.02 dB: the 0.9 spike is outside the region.
    let g = region(&s, seq, lane).gain_db;
    assert!((g - 6.0206).abs() < 0.01, "{g}");
    s.close().unwrap();
}

#[test]
fn normalize_refuses_bad_targets_offline_media_and_open_edits() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("x.caf"), 2, &vec![0.25; 4800]);
    let (s, seq, _lane, ev) = demo_with(&src);
    let model = |r: Result<vizdaw_ffi::DocSnapshot, VizError>| match r {
        Err(VizError::Model { message }) => message,
        other => panic!("expected a Model error, got {other:?}"),
    };
    for bad in [0.5, -96.5, f32::NAN, f32::NEG_INFINITY] {
        model(s.normalize_regions(vec![ev], bad));
    }
    model(s.normalize_regions(vec![], -1.0));
    model(s.normalize_regions(vec![a_note(&s, seq)], -1.0));
    model(s.normalize_regions(vec![999_999], -1.0));
    s.begin_edit("Drag".into()).unwrap();
    assert!(model(s.normalize_regions(vec![ev], -1.0)).contains("open edit"));
    s.end_edit().unwrap();
    // Offline media cannot be measured.
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let path = s.media().unwrap()[0].path.clone();
    std::fs::remove_file(pkg.join(&path)).unwrap();
    s.recheck_media().unwrap();
    assert!(model(s.normalize_regions(vec![ev], -1.0)).contains("offline"));
    s.close().unwrap();
}

#[test]
fn normalize_is_busy_while_no_overview_can_be_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("y.caf"), 2, &vec![0.25; 4800]);
    let (s, seq, lane, ev) = demo_with(&src);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    // Nothing in memory, nothing on disk, the worker held.
    s.debug_hold_overviews(true);
    s.debug_forget_overviews();
    std::fs::remove_dir_all(pkg.join("Media/.overviews")).unwrap();
    assert!(matches!(
        s.normalize_regions(vec![ev], -1.0),
        Err(VizError::Busy { .. })
    ));
    s.debug_hold_overviews(false);
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        s.idle().unwrap();
        if s.normalize_regions(vec![ev], -1.0).is_ok() {
            break;
        }
        assert!(Instant::now() < until, "the overview was never rebuilt");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!((region(&s, seq, lane).gain_db - 11.0412).abs() < 0.01);
    s.close().unwrap();
}

/// Preflight I1: after the item gets a new file (a rate change, so the
/// region's span is rescaled too), Normalize never measures the old
/// file's overview: it measures the new file, or it is `Busy`.
#[test]
fn normalize_never_measures_the_previous_file() {
    let dir = tempfile::tempdir().unwrap();
    // 0.1, with 0.8 over frames 6000..6100. The region keeps 0..4800 (0.1
    // only); at 96 kHz it is 0..9600 of the new file, which the old
    // overview's frames 6000..6100 would put inside it.
    let mut v = vec![0.1f32; 9600];
    for x in &mut v[6000..6100] {
        *x = 0.8;
    }
    let src = caf(&dir.path().join("w.caf"), 1, &v);
    let (s, seq, lane, ev) = demo_with(&src);
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Trim {
                event: ev,
                front_frames: 0,
                back_frames: 4800,
            },
        },
        None,
    )
    .unwrap();
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    // Measured from the 48 kHz file: −1 − 20·log10(0.1) = 19 dB.
    s.normalize_regions(vec![ev], -1.0).unwrap();
    assert!((region(&s, seq, lane).gain_db - 19.0).abs() < 0.01);
    s.undo().unwrap();
    let before = s.media().unwrap()[0].clone();
    s.debug_hold_overviews(true);
    let id = s
        .set_project_format(ProjectFormatRow {
            sample_rate: 96_000,
            depth: BitDepthRow::Float32,
        })
        .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while s.job(id).unwrap().state == JobStateRow::Running {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(s.job(id).unwrap().state, JobStateRow::Done);
    s.idle().unwrap();
    let after = s.media().unwrap()[0].clone();
    assert_ne!(after.path, before.path, "a new file");
    assert_eq!(after.sample_rate, 96_000);
    // The gain the new file's samples call for (the resampled 0.1 rings a
    // little at the start); the old overview would give about
    // −1 − 20·log10(0.8) ≈ 0.9 dB.
    let r = region(&s, seq, lane);
    let new = CafReader::open(&pkg.join(&after.path)).unwrap();
    let mut buf = vec![0.0f32; r.frames as usize];
    let mut scratch = Vec::new();
    new.read(r.source_offset, &mut buf, &mut scratch).unwrap();
    let peak = buf.iter().fold(0.0f32, |p, v| p.max(v.abs()));
    let want = -1.0 - 20.0 * peak.log10();
    assert!(want > 15.0, "{want}");
    let near_new = |g: f32| (g - want).abs() < 0.01;
    // The job wrote the new file's overview: it is measured, worker held.
    s.normalize_regions(vec![ev], -1.0).unwrap();
    let g = region(&s, seq, lane).gain_db;
    assert!(near_new(g), "{g}");
    s.undo().unwrap();
    // Without that cache, and with nothing in memory: Busy.
    std::fs::remove_dir_all(pkg.join("Media/.overviews")).unwrap();
    assert!(matches!(
        s.normalize_regions(vec![ev], -1.0),
        Err(VizError::Busy { .. })
    ));
    assert_eq!(region(&s, seq, lane).gain_db, 0.0);
    s.debug_hold_overviews(false);
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        s.idle().unwrap();
        if s.normalize_regions(vec![ev], -1.0).is_ok() {
            break;
        }
        assert!(Instant::now() < until, "the overview was never rebuilt");
        std::thread::sleep(Duration::from_millis(10));
    }
    let g = region(&s, seq, lane).gain_db;
    assert!(near_new(g), "{g}");
    s.close().unwrap();
}

#[test]
fn fades_follow_the_cursor_and_go() {
    let dir = tempfile::tempdir().unwrap();
    // 2 s of stereo: 96 000 frames.
    let src = caf(&dir.path().join("z.caf"), 2, &vec![0.25; 2 * 96_000]);
    let (s, seq, lane, ev) = demo_with(&src);
    let media = saved_media(&s, dir.path());
    // The cursor half a second into the region.
    let at = s.ticks_at(seq, vec![0.5]).unwrap()[0];
    let frames = (s.seconds_at(seq, vec![at]).unwrap()[0] * 48_000.0).round() as i64;
    let snap = s
        .edit(
            EditCommand::Audio {
                edit: AudioEdit::FadeTo {
                    events: vec![ev],
                    at,
                    fade_in: true,
                },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Fade In to Cursor"));
    assert_eq!(region(&s, seq, lane).fade_in.length, frames);
    let snap = s
        .edit(
            EditCommand::Audio {
                edit: AudioEdit::FadeTo {
                    events: vec![ev],
                    at,
                    fade_in: false,
                },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Fade Out from Cursor"));
    let r = region(&s, seq, lane);
    assert_eq!(
        (r.fade_in.length, r.fade_out.length),
        (frames, 96_000 - frames)
    );
    let snap = s
        .edit(
            EditCommand::Audio {
                edit: AudioEdit::RemoveFades { events: vec![ev] },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Remove Fades"));
    let r = region(&s, seq, lane);
    assert_eq!((r.fade_in.length, r.fade_out.length), (0, 0));
    // The cursor outside every selected region: refused.
    assert!(matches!(
        s.edit(
            EditCommand::Audio {
                edit: AudioEdit::FadeTo {
                    events: vec![ev],
                    at: 0,
                    fade_in: true,
                },
            },
            None,
        ),
        Err(VizError::Model { .. })
    ));
    assert_untouched(&media);
    s.close().unwrap();
}

/// T1 review: a fade out from the cursor shortens a fade in that would
/// overlap it, and unknown ids are refused.
#[test]
fn a_fade_out_shortens_the_fade_in_and_unknown_ids_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("f.caf"), 1, &vec![0.25; 96_000]);
    let (s, seq, lane, ev) = demo_with(&src);
    let fade = |events: Vec<u64>, secs: f64, fade_in: bool| {
        let at = s.ticks_at(seq, vec![secs]).unwrap()[0];
        let frames = (s.seconds_at(seq, vec![at]).unwrap()[0] * 48_000.0).round() as i64;
        let r = s.edit(
            EditCommand::Audio {
                edit: AudioEdit::FadeTo {
                    events,
                    at,
                    fade_in,
                },
            },
            None,
        );
        (r, frames)
    };
    let (r, _) = fade(vec![ev], 1.5, true);
    r.unwrap();
    let (r, out_at) = fade(vec![ev], 0.5, false);
    r.unwrap();
    let r = region(&s, seq, lane);
    assert_eq!(
        (r.fade_in.length, r.fade_out.length),
        (out_at, 96_000 - out_at),
        "the fade in ends where the fade out starts"
    );
    let model = |r: Result<vizdaw_ffi::DocSnapshot, VizError>| {
        assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}");
    };
    model(fade(vec![999_999], 0.5, true).0);
    model(fade(vec![ev, 999_999], 0.5, true).0);
    model(s.edit(
        EditCommand::Audio {
            edit: AudioEdit::RemoveFades {
                events: vec![999_999],
            },
        },
        None,
    ));
    model(s.edit(
        EditCommand::Audio {
            edit: AudioEdit::RemoveFades {
                events: vec![ev, 999_999],
            },
        },
        None,
    ));
    // Nothing changed.
    let r2 = region(&s, seq, lane);
    assert_eq!(
        (r2.fade_in.length, r2.fade_out.length),
        (r.fade_in.length, r.fade_out.length)
    );
    s.close().unwrap();
}
