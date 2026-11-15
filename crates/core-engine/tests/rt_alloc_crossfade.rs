//! Crossfades never allocate on the render path (plan 6c Task 3): two
//! regions on one track crossfade over 4 800 frames, with a loop range
//! around the span and locates every 500 blocks. The disk is serviced
//! outside the armed blocks; the output must carry the regions' audio.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_engine::rt_check::{self, CountingAlloc};
use core_engine::transport::PlayState;
use core_model::command::{AudioCmd, Command, CrossfadeLength};
use core_model::event::{Fade, FadeShape};
use std::path::Path;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

fn dc(path: &Path, frames: usize, level: f32) {
    let mut w = core_media::caf::CafWriter::create(
        path,
        48_000,
        2,
        core_model::format::BitDepth::Float32,
        core_media::caf::WriteMode::Normal,
    )
    .unwrap();
    w.write(&vec![level; 2 * frames]).unwrap();
    w.finish().unwrap();
}

#[test]
fn crossfades_never_allocate() {
    let dir = tempfile::tempdir().unwrap();
    let (pa, pb) = (dir.path().join("a.caf"), dir.path().join("b.caf"));
    dc(&pa, 240_000, 0.5);
    dc(&pb, 240_000, 0.25);
    let mut bld = Builder::new();
    let (seq, lane) = bld.sequence("Song", "Audio", None).unwrap();
    let ma = bld.media("Media/a.caf", 240_000, 2).unwrap();
    let mb = bld.media("Media/b.caf", 240_000, 2).unwrap();
    // A: beats 0–4 (96 000 frames); B from beat 3, crossfading the last
    // 24 000 frames of A over 4 800.
    bld.region(
        seq,
        lane,
        0,
        ma,
        0,
        96_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let b_ev = bld
        .region(
            seq,
            lane,
            3 * Q,
            mb,
            0,
            96_000,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    bld.run(Command::Audio(AudioCmd::Crossfade {
        event: b_ev,
        length: CrossfadeLength::Frames(4800),
        shape: FadeShape::EqualPower,
    }))
    .unwrap();
    let doc = bld.finish();

    let before = rt_check::count();
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = core_media::stream::DiskMode::Manual;
    cfg.stream = core_media::stream::StreamConfig {
        cache_file_bytes: 0,
        ..Default::default()
    };
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_media(vec![
        core_media::stream::MediaFile { id: ma, path: pa },
        core_media::stream::MediaFile { id: mb, path: pb },
    ]);
    let report = engine.load(&doc, seq).unwrap();
    // A before B, B, and A's complement under the span.
    assert_eq!(
        (report.audio_segments, report.audio_offline),
        (3, 0),
        "{report:?}"
    );
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(4096);
    // The span is frames 72 000–76 800; loop around it.
    let (lo, hi) = (70_000u64, 80_000u64);
    engine.set_loop_range(Some((lo, hi))).unwrap();
    engine.locate_frame(lo).unwrap();
    engine.play().unwrap();
    let (mut sounding, mut peak, mut blocks) = (0u32, 0f32, 0u32);
    for i in 0..2_000u32 {
        if i % 500 == 250 {
            engine.locate_frame(lo + u64::from(i) * 3).unwrap();
        }
        engine.streamer().service();
        let frames = 1 + (i * 37) % 1024;
        armed_block(&mut b, frames);
        blocks += 1;
        let (l, r) = b.output(frames);
        let p = l.iter().chain(r).fold(0f32, |m, x| m.max(x.abs()));
        if p > 0.0 {
            sounding += 1;
        }
        peak = peak.max(p);
    }
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations while playing a crossfade"
    );
    assert_eq!(engine.state(), PlayState::Playing);
    assert!(
        sounding > blocks / 2 && peak > 0.2,
        "the crossfade was not heard: {sounding} sounding block(s), peak {peak}"
    );
    assert!(
        engine.streamer().counters().stream_voices > 0,
        "nothing streamed"
    );
    let f = engine.faults();
    assert!(!f.contains(Faults::STREAM_UNDERRUN), "{:?}", f.names());
    drop(b.stop().unwrap());
}
