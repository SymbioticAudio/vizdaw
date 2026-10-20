//! Recording never allocates on the render path (plan 5c Task 5; spec
//! §3, §12.3 "`rt-check` covers … the input ring"): the counting
//! allocator is armed only around `OfflineBackend::block`, while a record
//! tap captures fake input through loop wraps, a full ring, tap swaps, an
//! xrun, a stop and the tap's removal.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::rt_check::{self, CountingAlloc};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

/// One test, so no other test thread allocates while the global counter
/// is compared.
#[test]
fn recording_never_allocates() {
    let mut bld = Builder::new();
    let synth = bld.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = bld.sequence("Bar", "Notes", Some(synth)).unwrap();
    bld.notes(seq, lane, &[(0, 60, 0x8000, Q), (2 * Q, 67, 0x8000, Q)])
        .unwrap();
    let doc = bld.finish();
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512).with_input(FakeInput::Loopback(1000));
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop_range(Some((0, 30_000))).unwrap();
    let mut h = engine
        .start_recording(&RecordRequest {
            channels: vec![0, 1, 2],
            range: Some((1000, 20_000)),
            ring_frames: 48_000,
        })
        .unwrap();
    engine.play().unwrap();
    let before = rt_check::count();
    let mut buf = Vec::with_capacity(1 << 20);
    for i in 0..3_000u32 {
        armed_block(&mut b, 1 + (i * 37) % 1024);
        // Drained (unarmed) except between blocks 1000 and 1500: the ring
        // fills and blocks are lost.
        if !(1000..1500).contains(&i) {
            while h.ring.pop(&mut buf).is_some() {}
            while h.marks.pop().is_ok() {}
        }
        if i % 100 == 0 {
            log.drain();
        }
        if i == 2500 {
            // A jump while recording is marked (review fix M1).
            engine.locate_frame(5000).unwrap();
        }
        if i == 2000 {
            // A new tap replaces the old one while playing.
            h = engine
                .start_recording(&RecordRequest {
                    channels: vec![2],
                    range: None,
                    ring_frames: 4800,
                })
                .unwrap();
        }
    }
    assert!(h.status.run() > 0);
    assert_eq!(rt_check::count() - before, 0, "allocations while recording");
    // An xrun while recording, then the tap goes.
    armed_block(&mut b, 8192);
    // The transport stops (the tap marks it, pre-flight M2), then the tap
    // goes: the removed tap is retired, never freed here.
    engine.stop().unwrap();
    armed_block(&mut b, 512);
    // Playing again resumes the tap (a resume mark and a fresh delay).
    engine.play().unwrap();
    armed_block(&mut b, 512);
    engine.stop_recording().unwrap();
    armed_block(&mut b, 512);
    assert!(h.status.closed());
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations on xrun and stop"
    );
    drop(b.stop().unwrap());
}
