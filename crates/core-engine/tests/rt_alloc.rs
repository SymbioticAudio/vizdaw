//! Counts allocations made on the render path: the counting allocator is
//! armed only around `OfflineBackend::block`, which only calls
//! `Renderer::process`.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::demo_document;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_engine::rt_check::{self, CountingAlloc};
use core_engine::schedule::BuiltinRoute;
use core_engine::transport::PlayState;
use core_model::time::Tick;

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
fn render_path_never_allocates() {
    // The counter works: one allocation and one free.
    let before = rt_check::count();
    rt_check::arm();
    let v: Vec<u8> = Vec::with_capacity(10);
    drop(v);
    rt_check::disarm();
    assert_eq!(rt_check::count() - before, 2);

    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let before = rt_check::count();

    // 10k blocks of play, stop, locate and loop wraps on the built-in
    // synth plus a MIDI output (route both).
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.route = BuiltinRoute::Both;
    let sinks = OfflineSinks::new(1 << 20);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(cfg, Box::new(sinks)).unwrap();
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop(true).unwrap();
    engine.play().unwrap();
    // What the phases exercised (read outside the armed blocks).
    let (mut wraps, mut stops, mut locates, mut tails, mut ended) = (0, 0, 0, 0, 0);
    for i in 0..10_000u32 {
        let mut moved = false;
        match i % 1000 {
            300 => {
                engine.stop().unwrap();
                stops += 1;
            }
            310 => engine.play().unwrap(),
            500 => {
                engine.locate_tick(Tick(15_360 * 40)).unwrap();
                locates += 1;
                moved = true;
            }
            700 => engine.set_loop(false).unwrap(),
            900 => engine.set_loop(true).unwrap(),
            _ => {}
        }
        let at = engine.position_frames();
        armed_block(&mut b, 1 + (i * 37) % 1024);
        if !moved && engine.position_frames() < at {
            wraps += 1;
        }
        if i % 100 == 0 {
            log.drain();
        }
    }
    assert_eq!(rt_check::count() - before, 0, "allocations while playing");

    // Loop off: play past the end into the tail until the end is reported.
    engine.set_loop(false).unwrap();
    engine
        .locate_frame(engine.length_frames() - 10_000)
        .unwrap();
    locates += 1;
    engine.play().unwrap();
    for i in 0..1_000u32 {
        armed_block(&mut b, 1 + (i * 53) % 1024);
        if engine.state() == PlayState::Tail {
            tails += 1;
        }
        if engine.take_ended() {
            ended += 1;
            break;
        }
    }
    log.drain();
    assert_eq!(engine.state(), PlayState::Stopped);
    assert_eq!(rt_check::count() - before, 0, "allocations into the tail");
    assert!(wraps >= 1, "no loop wrap was rendered");
    assert!(
        stops >= 1 && locates >= 1,
        "stops {stops}, locates {locates}"
    );
    assert!(tails >= 1, "the tail was never rendered");
    assert_eq!(ended, 1, "the end was never reported");

    // 100 schedule swaps; with the collector paused the retire ring
    // (capacity 8) fills and the ninth swap is deferred.
    engine.pause_collector(true);
    for i in 0..100 {
        if i == 12 {
            engine.pause_collector(false);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        engine.load(&doc, song).unwrap();
        armed_block(&mut b, 256);
    }
    assert!(engine.counters().swaps_deferred >= 1);
    assert_eq!(rt_check::count() - before, 0, "allocations during swaps");

    // Blocks larger than the prepared size.
    armed_block(&mut b, 8192);
    assert!(engine.faults().contains(Faults::XRUN));
    assert_eq!(rt_check::count() - before, 0, "allocations on xrun");
    drop(b.stop().unwrap());
    drop(engine);

    // A full MIDI queue drops instead of allocating.
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.route = BuiltinRoute::Virtual;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(8))).unwrap();
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    engine.play().unwrap();
    for _ in 0..2_000 {
        armed_block(&mut b, 512);
    }
    assert!(engine.counters().midi_dropped > 0);
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations with a full MIDI queue"
    );
}
