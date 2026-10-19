//! Regions through the whole engine (plan 5a Task 9): the offline backend,
//! the streamer in Manual mode (serviced between blocks, as the disk
//! thread would), loops, locates, starvation, offline media.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_engine::transport::PlayState;
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile, StreamConfig};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::format::BitDepth;
use core_model::ids::{MediaId, SeqId};
use core_model::time::Tick;
use proptest::prelude::*;
use std::path::Path;

const QF: u64 = 24_000;
const FILE_FRAMES: u64 = 480_000;
const BLOCK: u32 = 512;

fn value(f: u64) -> f32 {
    ((f % 997) as f32 + 1.0) / 2000.0
}

fn caf(path: &Path) {
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    let data: Vec<f32> = (0..FILE_FRAMES)
        .flat_map(|f| [value(f), -value(f)])
        .collect();
    w.write(&data).unwrap();
    w.finish().unwrap();
}

/// Regions `(beat, offset, frames)` of one 10 s media item on one lane.
fn doc(regions: &[(i64, u64, u64)]) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FILE_FRAMES, 2).unwrap();
    for &(beat, off, n) in regions {
        b.region(
            seq,
            lane,
            beat * Q,
            m,
            off,
            n,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    }
    (b.finish(), seq)
}

struct Rig {
    engine: Engine,
    out: OfflineBackend,
    _dir: tempfile::TempDir,
}

/// `cached`: files are read whole; otherwise every region streams.
fn rig(doc: &Document, seq: SeqId, cached: bool, online: bool) -> Rig {
    let stream = StreamConfig {
        cache_file_bytes: if cached { 64 << 20 } else { 0 },
        ..StreamConfig::default()
    };
    rig_with(doc, seq, stream, online)
}

fn rig_with(doc: &Document, seq: SeqId, stream: StreamConfig, online: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"));
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = DiskMode::Manual;
    cfg.stream = stream;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    if online {
        engine.set_media(vec![MediaFile {
            id: MediaId(0),
            path: dir.path().join("a.caf"),
        }]);
    }
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    Rig {
        engine,
        out,
        _dir: dir,
    }
}

impl Rig {
    /// Renders `blocks` blocks (servicing the disk first unless `starve`);
    /// returns the left channel.
    fn render(&mut self, blocks: usize, starve: bool) -> Vec<f32> {
        let mut left = Vec::new();
        for _ in 0..blocks {
            if !starve {
                self.engine.streamer().service();
            }
            self.out.block(BLOCK);
            left.extend_from_slice(self.out.output(BLOCK).0);
        }
        left
    }
}

#[test]
fn a_region_plays_through_the_engine() {
    let (d, seq) = doc(&[(1, 100, 2 * QF)]);
    let mut r = rig(&d, seq, false, true);
    r.engine.play().unwrap();
    let left = r.render(200, false);
    assert!(left[..QF as usize].iter().all(|x| *x == 0.0));
    // The strip's pan law scales by a constant k.
    let k = left[QF as usize] / value(100);
    assert!(k > 0.5 && k <= 1.0, "{k}");
    for i in [0u64, 1, 999, QF, 2 * QF - 1] {
        let got = left[(QF + i) as usize];
        assert!((got - k * value(100 + i)).abs() < 1e-6, "frame {i}: {got}");
    }
    assert_eq!(left[(3 * QF) as usize], 0.0);
    assert!(
        r.engine.faults().is_empty(),
        "{:?}",
        r.engine.faults().names()
    );
}

#[test]
fn a_loop_inside_a_region_wraps_seamlessly() {
    let (d, seq) = doc(&[(0, 0, 8 * QF)]);
    let mut r = rig(&d, seq, false, true);
    // Loop beats 1 to 3 (48 000 frames), well inside the region.
    r.engine.set_loop_range(Some((QF, 3 * QF))).unwrap();
    r.engine.locate_frame(QF).unwrap();
    r.engine.play().unwrap();
    let left = r.render(400, false);
    let k = left[0] / value(QF);
    for (i, x) in left.iter().enumerate() {
        let f = QF + (i as u64 % (2 * QF));
        assert!((x - k * value(f)).abs() < 1e-6, "out {i}: {x}");
    }
    assert!(
        r.engine.faults().is_empty(),
        "{:?}",
        r.engine.faults().names()
    );
}

#[test]
fn starved_disk_plays_silence_and_flags_underruns() {
    let (d, seq) = doc(&[(0, 0, 8 * QF)]);
    let mut r = rig(&d, seq, false, true);
    // Play waits for the disk (and services it in Manual mode), so starve
    // after a locate the disk has not seen.
    r.engine.play().unwrap();
    r.render(2, false);
    r.engine.locate_frame(5 * QF).unwrap();
    let left = r.render(60, true);
    assert!(left.iter().all(|x| *x == 0.0));
    let f = r.engine.faults();
    assert!(f.contains(Faults::LATE_START), "{:?}", f.names());
    assert!(f.contains(Faults::STREAM_UNDERRUN), "{:?}", f.names());
    assert_eq!(r.engine.state(), PlayState::Playing);
    // The disk catches up: audio returns.
    let left = r.render(10, false);
    assert!(left.iter().any(|x| *x != 0.0));
}

#[test]
fn misses_right_after_play_count_as_late_start() {
    let (d, seq) = doc(&[(0, 0, 8 * QF)]);
    let mut r = rig(&d, seq, false, true);
    r.engine.play().unwrap();
    r.render(2, false);
    r.engine.faults();
    r.engine.locate_frame(4 * QF).unwrap();
    // 250 ms is 12 000 frames: 23 blocks of 512 are within it.
    r.render(20, true);
    let f = r.engine.faults();
    assert!(
        f.contains(Faults::LATE_START) && !f.contains(Faults::STREAM_UNDERRUN),
        "{:?}",
        f.names()
    );
    assert_eq!(Faults::LATE_START.names(), vec!["late-start"]);
    assert_eq!(Faults::STREAM_UNDERRUN.names(), vec!["stream-underrun"]);
}

/// Ruling 10 (F16): the late-start window is the first 250 ms played
/// after Play or Locate, not a range of schedule frames, so misses right
/// after a loop wrap (back below where Play started) are underruns.
#[test]
fn misses_after_a_loop_wrap_count_as_underruns() {
    let (d, seq) = doc(&[(0, 0, 16 * QF)]);
    // One stream voice: the loop start cannot be held in a second voice,
    // so with the disk starved from the end of the first pass the wrap
    // misses.
    let stream = StreamConfig {
        cache_file_bytes: 0,
        stream_voices: 1,
        ..StreamConfig::default()
    };
    let mut r = rig_with(&d, seq, stream, true);
    // A 4 s loop from frame 0: 375 blocks a pass.
    r.engine.set_loop_range(Some((0, 8 * QF))).unwrap();
    r.engine.play().unwrap();
    let first = r.render(374, false);
    assert!(first.iter().skip(1).all(|x| *x != 0.0));
    assert!(
        r.engine.faults().is_empty(),
        "{:?}",
        r.engine.faults().names()
    );
    // The pass's last block, the wrap, then the first 15 360 frames of
    // the second pass (more than 250 ms), all starved.
    let left = r.render(31, true);
    assert!(left[..BLOCK as usize].iter().all(|x| *x != 0.0));
    assert!(left[BLOCK as usize..].iter().all(|x| *x == 0.0));
    let f = r.engine.faults();
    assert!(f.contains(Faults::STREAM_UNDERRUN), "{:?}", f.names());
    assert!(!f.contains(Faults::LATE_START), "{:?}", f.names());
    assert_eq!(r.engine.state(), PlayState::Playing);
}

/// Fix round 1, I1: during Play Selection the transport ignores the
/// loop, so an edit (`load`), `set_loop` or `set_loop_range` must not
/// give the streamer a loop; the next plain Play loops seamlessly.
fn selection_then_edit(edit: impl FnOnce(&mut Rig, &Document, SeqId)) {
    let (d, seq) = doc(&[(0, 0, 16 * QF)]);
    let mut r = rig(&d, seq, false, true);
    r.engine.set_loop_range(Some((0, 8 * QF))).unwrap();
    r.engine.play_until(Tick(16 * Q)).unwrap();
    edit(&mut r, &d, seq);
    let lp = r.engine.streamer().loop_range();
    assert_eq!(lp, None, "the streamer loops during Play Selection");
    // 400 blocks run past the loop end (192 000) without wrapping.
    let left = r.render(400, false);
    let k = left[0] / value(0);
    assert!(k > 0.5 && k <= 1.0, "{k}");
    for (i, x) in left.iter().enumerate() {
        assert!((x - k * value(i as u64)).abs() < 1e-6, "selection {i}: {x}");
    }
    let f = r.engine.faults();
    assert!(f.is_empty(), "{:?}", f.names());
    // A plain Play from the start loops again, seamlessly.
    r.engine.stop().unwrap();
    r.engine.locate_frame(0).unwrap();
    r.engine.play().unwrap();
    let (ls, le) = r.engine.streamer().loop_range().expect("loop restored");
    assert_eq!(ls, 0);
    let blocks = (le + 2 * QF) as usize / BLOCK as usize;
    let left = r.render(blocks, false);
    for (i, x) in left.iter().enumerate() {
        let f = i as u64 % le;
        assert!((x - k * value(f)).abs() < 1e-6, "loop {i}: {x}");
    }
    let f = r.engine.faults();
    assert!(f.is_empty(), "{:?}", f.names());
}

#[test]
fn an_edit_during_play_selection_keeps_the_streamer_unlooped() {
    selection_then_edit(|r, d, seq| {
        r.engine.load(d, seq).unwrap();
    });
}

#[test]
fn set_loop_during_play_selection_keeps_the_streamer_unlooped() {
    selection_then_edit(|r, _, _| r.engine.set_loop(true).unwrap());
}

#[test]
fn set_loop_range_during_play_selection_keeps_the_streamer_unlooped() {
    selection_then_edit(|r, _, _| r.engine.set_loop_range(Some((0, 8 * QF))).unwrap());
}

/// Fix round 1, I2: Stop, Locate and Play back to back while playing (no
/// block between, so the published state still reads Playing) still wait
/// for the disk at the new position. In Manual mode the wait is what
/// services the disk: skipping it would start late.
#[test]
fn stop_locate_play_while_playing_waits_for_the_disk() {
    let (d, seq) = doc(&[(0, 0, 8 * QF)]);
    let mut r = rig(&d, seq, false, true);
    r.engine.play().unwrap();
    let left = r.render(4, false);
    let k = left[0] / value(0);
    r.engine.faults();
    r.engine.stop().unwrap();
    r.engine.locate_frame(5 * QF).unwrap();
    r.engine.play().unwrap();
    assert!(r.engine.streamer().ready(5 * QF));
    // The first block from the new position, disk not serviced.
    let left = r.render(1, true);
    for (i, x) in left.iter().enumerate() {
        let f = 5 * QF + i as u64;
        assert!((x - k * value(f)).abs() < 1e-6, "frame {i}: {x}");
    }
    let f = r.engine.faults();
    assert!(f.is_empty(), "{:?}", f.names());
}

#[test]
fn offline_media_plays_silence_and_counts_nothing() {
    let (d, seq) = doc(&[(0, 0, 8 * QF)]);
    let mut r = rig(&d, seq, false, false);
    r.engine.play().unwrap();
    let left = r.render(100, false);
    assert!(left.iter().all(|x| *x == 0.0));
    assert!(
        r.engine.faults().is_empty(),
        "{:?}",
        r.engine.faults().names()
    );
}

#[test]
fn muted_regions_play_silence() {
    let (d, seq) = doc(&[(0, 0, 8 * QF)]);
    let mut r = rig(&d, seq, true, true);
    r.engine.set_regions_muted(true);
    r.engine.play().unwrap();
    assert!(r.render(50, false).iter().all(|x| *x == 0.0));
    r.engine.set_regions_muted(false);
    assert!(r.render(5, false).iter().any(|x| *x != 0.0));
}

/// Plays both rigs through the same actions; returns their left channels.
fn both(
    regions: &[(i64, u64, u64)],
    lp: Option<(u64, u64)>,
    actions: &[(u8, u64)],
) -> (Vec<f32>, Vec<f32>, Faults) {
    let (d, seq) = doc(regions);
    let mut a = rig(&d, seq, true, true);
    let mut b = rig(&d, seq, false, true);
    for r in [&mut a, &mut b] {
        r.engine.set_loop_range(lp).unwrap();
        r.engine.play().unwrap();
    }
    let (mut la, mut lb) = (Vec::new(), Vec::new());
    for &(what, n) in actions {
        if what == 0 {
            let at = n % (16 * QF);
            a.engine.locate_frame(at).unwrap();
            b.engine.locate_frame(at).unwrap();
        }
        la.extend(a.render(4, false));
        lb.extend(b.render(4, false));
    }
    (la, lb, b.engine.faults())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 16, .. ProptestConfig::default() })]

    /// Streaming is invisible: a streamed render equals a fully cached one
    /// sample for sample, through loops and locates (late starts aside,
    /// which only happen when the disk was not serviced).
    #[test]
    fn streamed_render_equals_cached_render(
        regions in prop::collection::vec((0i64..12, 0u64..200_000, 1u64..200_000), 1..5),
        lp in prop::option::of((0u64..8, 1u64..8)),
        actions in prop::collection::vec((0u8..6, 0u64..1_000_000), 1..30),
    ) {
        let regions: Vec<(i64, u64, u64)> = regions
            .into_iter()
            .map(|(b, off, n)| (b, off, n.min(FILE_FRAMES - off)))
            .collect();
        let lp = lp.map(|(s, l)| (s * QF, (s + l) * QF));
        let (a, b, faults) = both(&regions, lp, &actions);
        prop_assert_eq!(a, b);
        prop_assert!(!faults.contains(Faults::STREAM_UNDERRUN), "{:?}", faults.names());
    }

    /// A disk that never runs: silence, never garbage, never a stop.
    #[test]
    fn a_starved_disk_is_only_ever_silent(
        regions in prop::collection::vec((0i64..12, 0u64..200_000, 1u64..200_000), 1..5),
        at in (6 * QF)..(12 * QF),
    ) {
        // `play` primes the disk at frame 0 (its 3 s window ends at
        // 6 QF), so the locate goes past what it read; a long last region
        // keeps the sequence playing past every locate (F4).
        let regions: Vec<(i64, u64, u64)> = regions
            .into_iter()
            .map(|(b, off, n)| (b, off, n.min(FILE_FRAMES - off)))
            .chain([(24, 0, 400_000)])
            .collect();
        let (d, seq) = doc(&regions);
        let mut r = rig(&d, seq, false, true);
        r.engine.play().unwrap();
        r.engine.locate_frame(at).unwrap();
        let left = r.render(30, true);
        prop_assert!(left.iter().all(|x| *x == 0.0));
        prop_assert_eq!(r.engine.state(), PlayState::Playing);
    }
}
