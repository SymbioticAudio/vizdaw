//! Zero-crossing snapping for trims (plan release-fixes Task 7): the frame
//! of a media item nearest a trim where the channel-summed signal changes
//! sign.

mod common;

use core_media::caf::{CafWriter, WriteMode};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::force_offline;
use vizdaw_ffi::{ImportTargetRow, JobStateRow, Session, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    g
}

/// The fixture's length in frames (not a multiple of 240).
const FRAMES: u64 = 4900;

/// A 100 Hz sine at 48 kHz, exactly 0 every 240 frames (where it crosses).
fn sine(k: u64) -> f32 {
    if k % 240 == 0 {
        0.0
    } else {
        (2.0 * std::f64::consts::PI * 100.0 * k as f64 / 48_000.0).sin() as f32
    }
}

/// A CAF of the sine: mono, or stereo with the right channel `right` × the
/// left.
fn caf(path: &Path, channels: u16, right: f32) -> PathBuf {
    let mut w =
        CafWriter::create(path, 48_000, channels, BitDepth::Float32, WriteMode::Normal).unwrap();
    let data: Vec<f32> = (0..FRAMES)
        .flat_map(|k| {
            let x = sine(k);
            if channels == 2 {
                vec![x, right * x]
            } else {
                vec![x]
            }
        })
        .collect();
    w.write(&data).unwrap();
    w.finish().unwrap();
    path.to_path_buf()
}

/// The demo with `src` imported: `(session, media)`.
fn demo_with(src: &Path) -> (Arc<Session>, u64) {
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
    let until = Instant::now() + common::wait_limit();
    while s.job(id).unwrap().state == JobStateRow::Running {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    let media = s.media().unwrap().last().unwrap().id;
    (s, media)
}

#[test]
fn zero_crossing_finds_the_nearest_sign_change() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, m) = demo_with(&caf(&dir.path().join("sine.caf"), 1, 1.0));
    assert_eq!(s.media().unwrap().last().unwrap().frames, FRAMES);
    // Crossings at 0, 240, 480, …: 240 is 10 frames from 250.
    assert_eq!(s.zero_crossing(m, 250, 144).unwrap(), 240);
    // None within 5 frames: the frame itself.
    assert_eq!(s.zero_crossing(m, 250, 5).unwrap(), 250);
    // A tie (120 is 120 from 0 and from 240) goes to the earlier frame.
    assert_eq!(s.zero_crossing(m, 120, 120).unwrap(), 0);
    assert_eq!(s.zero_crossing(m, 0, 0).unwrap(), 0);
    // Past the end clamps to the length, then snaps from there.
    assert_eq!(s.zero_crossing(m, 10_000_000, 5).unwrap(), FRAMES);
    assert_eq!(s.zero_crossing(m, u64::MAX, 144).unwrap(), 4800);
    // The largest radius is a second at 48 kHz.
    assert_eq!(s.zero_crossing(m, 250, 48_000).unwrap(), 240);
    assert!(matches!(
        s.zero_crossing(m, 250, 48_001),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        s.zero_crossing(m, 250, u64::MAX),
        Err(VizError::Model { .. })
    ));
    // An unknown media item.
    for bad in [m + 1000, u64::MAX] {
        assert!(matches!(
            s.zero_crossing(bad, 250, 144),
            Err(VizError::Model { .. })
        ));
    }
    s.close().unwrap();
}

#[test]
fn opposite_channels_sum_to_silence_everywhere() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, m) = demo_with(&caf(&dir.path().join("wide.caf"), 2, -1.0));
    // The sum is 0 at every frame: the frame itself is a crossing.
    assert_eq!(s.zero_crossing(m, 250, 144).unwrap(), 250);
    s.close().unwrap();
    // In phase the channels add, and cross where the sine does.
    let (s, m) = demo_with(&caf(&dir.path().join("mid.caf"), 2, 1.0));
    assert_eq!(s.zero_crossing(m, 250, 144).unwrap(), 240);
    s.close().unwrap();
}

#[test]
fn offline_media_keeps_the_frame() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, m) = demo_with(&caf(&dir.path().join("sine.caf"), 1, 1.0));
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let path = s.media().unwrap().last().unwrap().path.clone();
    std::fs::remove_file(pkg.join(&path)).unwrap();
    s.recheck_media().unwrap();
    assert_eq!(s.zero_crossing(m, 250, 144).unwrap(), 250);
    assert_eq!(s.zero_crossing(m, 10_000_000, 144).unwrap(), FRAMES);
    s.close().unwrap();
}

/// Review Focus 5: a relink drops the cached reader; the next snap reads
/// the new file.
#[test]
fn the_cached_reader_follows_a_relink() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let step = |name: &str, flip: u64| {
        let path = dir.path().join(name);
        let mut w =
            CafWriter::create(&path, 48_000, 1, BitDepth::Float32, WriteMode::Normal).unwrap();
        let data: Vec<f32> = (0..FRAMES)
            .map(|k| if k < flip { 1.0 } else { -1.0 })
            .collect();
        w.write(&data).unwrap();
        w.finish().unwrap();
        path
    };
    let (s, m) = demo_with(&step("a.caf", 100));
    // The sign changes at 100: from 250 that is the nearest.
    assert_eq!(s.zero_crossing(m, 250, 1000).unwrap(), 100);
    assert_eq!(s.zero_crossing(m, 250, 1000).unwrap(), 100);
    let b = step("b.caf", 300);
    let job = s.relink_media(m, b.display().to_string()).unwrap();
    let until = Instant::now() + common::wait_limit();
    while s.job(job).unwrap().state == JobStateRow::Running {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(s.job(job).unwrap().state, JobStateRow::Done);
    assert_eq!(s.zero_crossing(m, 250, 1000).unwrap(), 300);
    s.close().unwrap();
}
