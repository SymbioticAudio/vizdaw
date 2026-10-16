//! Counts allocations on the render path with the mixer graph and worker
//! threads: the callback thread is armed around `OfflineBackend::block`,
//! and every worker arms itself around each job (`set_arm_workers`).

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, demo_document};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_engine::rt_check::{self, CountingAlloc};
use core_engine::schedule::BuiltinRoute;
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::ids::{SeqId, StripId};
use core_model::mixer::{AuxSend, StripKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

/// The demo with: a gain/latency insert and a NaN insert (it faults after
/// 16 blocks) on the synth, a post send to "Verb" (noise, latency 200) whose
/// second insert adds the synth's sidechain, and a pre-fader send to "Dry".
fn mixed(master_db: f64, extra_bus: bool) -> (Document, SeqId) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let verb = b.bus("Verb").unwrap();
    let dry = b.bus("Dry").unwrap();
    b.insert(synth, TestInserts::plugin("gain=0.7,latency=33"))
        .unwrap();
    b.insert(synth, TestInserts::plugin("nan")).unwrap();
    b.insert(verb, TestInserts::plugin("noise=3,latency=200"))
        .unwrap();
    let sc = b.insert(verb, TestInserts::plugin("sc")).unwrap();
    b.mixer(MixerCmd::SetSidechain {
        slot: sc,
        source: Some(synth),
    })
    .unwrap();
    b.mixer(MixerCmd::AddSend {
        strip: synth,
        send: AuxSend::new(verb),
    })
    .unwrap();
    let mut pre = AuxSend::new(dry);
    pre.pre_fader = true;
    b.mixer(MixerCmd::AddSend {
        strip: synth,
        send: pre,
    })
    .unwrap();
    b.mixer(MixerCmd::SetGain {
        strip: StripId(0),
        gain_db: master_db,
    })
    .unwrap();
    if extra_bus {
        b.bus("Extra").unwrap();
    }
    (b.finish(), song)
}

/// One test: the allocation counter is global.
#[test]
fn graph_render_path_never_allocates() {
    rt_check::set_arm_workers(true);
    // The counter works on this thread.
    let before = rt_check::count();
    rt_check::arm();
    drop(Vec::<u8>::with_capacity(10));
    rt_check::disarm();
    assert_eq!(rt_check::count() - before, 2);

    let (doc, song) = mixed(0.0, false);
    let (quieter, _) = mixed(-3.0, false);
    let (wider, _) = mixed(-3.0, true);

    // Phase 1: three workers, both routes, 3000 blocks of varying size,
    // with a parameter swap every 300 blocks and a structural swap every
    // 700 (its delay lines are new; its instances are adopted).
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.workers = 3;
    cfg.route = BuiltinRoute::Both;
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(cfg, Box::new(sinks)).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop(true).unwrap();
    engine.play().unwrap();
    let before = rt_check::count();
    for i in 0..3000u32 {
        if i % 300 == 150 {
            engine
                .load(if i % 600 == 150 { &quieter } else { &doc }, song)
                .unwrap();
        }
        if i % 700 == 350 {
            engine.load(&wider, song).unwrap();
        }
        armed_block(&mut b, 1 + (i * 37) % 1024);
        if i % 100 == 0 {
            log.drain();
        }
    }
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations while playing the graph"
    );
    assert!(engine.faults().contains(Faults::PLUGIN_FAULT));
    assert_eq!(engine.plugin_faults().len(), 1);

    // Phase 2: graph swaps with the retire ring full (deferred swaps).
    // Each load queues three commands and each applied swap retires two
    // items, so the 16-slot retire ring is full after eight blocks;
    // resuming at block 10 keeps the 64-slot command ring from refusing a
    // load, and stopping at 20 keeps the loop from outrunning the collector
    // (it frees every 10 ms; an optimised loop makes 20 edits in far less).
    engine.pause_collector(true);
    for i in 0..20 {
        if i == 10 {
            engine.pause_collector(false);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        engine
            .load(if i % 2 == 0 { &quieter } else { &doc }, song)
            .unwrap();
        armed_block(&mut b, 256);
    }
    assert!(engine.counters().swaps_deferred >= 1);
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations during graph swaps"
    );
    log.drain();
    drop(b.stop().unwrap());
    drop(engine);

    // Phase 3: a stalled insert on a worker misses the deadline; the next
    // blocks find it busy; then it recovers.
    let flag = Arc::new(AtomicBool::new(false));
    let (stalled, seq) = {
        let mut b = Builder::from_doc(demo_document().unwrap());
        let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
        b.insert(synth, TestInserts::plugin("stall")).unwrap();
        let song = find_sequence(b.doc(), "Song").unwrap();
        (b.finish(), song)
    };
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.workers = 1;
    cfg.deadline = Some(0.5);
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::with_stall(Arc::clone(&flag))));
    engine.set_callback_helps(false);
    engine.load(&stalled, seq).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    engine.play().unwrap();
    let before = rt_check::count();
    for _ in 0..3 {
        armed_block(&mut b, 512);
    }
    flag.store(true, Ordering::Release);
    std::thread::sleep(std::time::Duration::from_millis(50));
    for _ in 0..10 {
        armed_block(&mut b, 512);
    }
    let c = engine.counters();
    assert!(c.deadline_misses >= 1 && c.busy_blocks >= 1, "{c:?}");
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations on the deadline path"
    );
    drop(b.stop().unwrap());
    drop(engine);
    rt_check::set_arm_workers(false);
}
