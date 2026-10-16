//! Counts allocations on the render path with the mixer graph and worker
//! threads: the callback thread is armed around `OfflineBackend::block`,
//! and every worker arms itself around each job (`set_arm_workers`).

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, demo_document};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::{Faults, RETIRE_CAPACITY};
use core_engine::rt_check::{self, CountingAlloc};
use core_engine::schedule::BuiltinRoute;
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::ids::{SeqId, StripId};
use core_model::mixer::{AuxSend, ParamId, StripKind};
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
/// `master_db` and `param` (an insert parameter overlay) change values only;
/// `extra_bus` changes the graph's shape.
fn mixed(master_db: f64, param: f64, extra_bus: bool) -> (Document, SeqId) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let verb = b.bus("Verb").unwrap();
    let dry = b.bus("Dry").unwrap();
    let gain = b
        .insert(synth, TestInserts::plugin("gain=0.7,latency=33"))
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
    b.mixer(MixerCmd::SetPluginParams {
        slot: gain,
        params: vec![(ParamId(1), param)],
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

/// Plays `blocks` blocks of the full graph on `workers` workers, swapping
/// in a value-only variant every 300 blocks and a structural one every 700
/// (its delay lines are new; its instances are adopted). Asserts nothing
/// allocated, every swap was applied, and (when the callback does not
/// help) that workers rendered nodes.
fn play_graph(workers: usize, callback_helps: bool, blocks: u32) {
    let (doc, song) = mixed(0.0, 0.0, false);
    let (quieter, _) = mixed(-3.0, 0.5, false);
    let (wider, _) = mixed(-3.0, 0.5, true);
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.workers = workers;
    cfg.route = BuiltinRoute::Both;
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(cfg, Box::new(sinks)).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_callback_helps(callback_helps);
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop(true).unwrap();
    engine.play().unwrap();
    let before = rt_check::count();
    let steps = engine.worker_steps();
    let mut loads = 0u64;
    for i in 0..blocks {
        if i % 300 == 150 {
            engine
                .load(if i % 600 == 150 { &quieter } else { &doc }, song)
                .unwrap();
            loads += 1;
        }
        if i % 700 == 350 {
            engine.load(&wider, song).unwrap();
            loads += 1;
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
    // Every load's graph was swapped in (plus the first one), none rejected.
    assert!(
        engine.graphs_applied() > loads,
        "{} graphs for {loads} loads",
        engine.graphs_applied()
    );
    assert!(engine.faults().contains(Faults::PLUGIN_FAULT));
    assert_eq!(engine.plugin_faults().len(), 1);
    if !callback_helps {
        assert!(
            engine.worker_steps() - steps >= u64::from(blocks),
            "workers rendered {} nodes",
            engine.worker_steps() - steps
        );
    }
    drop(b.stop().unwrap());
    drop(engine);
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

    let (doc, song) = mixed(0.0, 0.0, false);
    let (quieter, _) = mixed(-3.0, 0.5, false);
    let (wider, _) = mixed(-3.0, 0.5, true);

    // Phase 1: both routes, 3000 blocks of varying size, callback helping;
    // then 1500 blocks where only the workers render nodes.
    play_graph(3, true, 3000);
    play_graph(3, false, 1500);

    // Phase 2: graph swaps with the retire ring full (deferred swaps),
    // value-only and structural. Each load queues three commands and each
    // applied swap retires two items, so the retire ring is full after
    // `RETIRE_CAPACITY / 2` blocks; resuming two blocks later keeps the
    // command ring from refusing a load, and stopping ten blocks after that
    // keeps the loop from outrunning the collector (it frees every 10 ms).
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
    for _ in 0..20 {
        armed_block(&mut b, 256);
    }
    let before = rt_check::count();
    let applied = engine.graphs_applied();
    engine.pause_collector(true);
    // Two retire entries per load: the ring fills after
    // `RETIRE_CAPACITY / 2` loads.
    let fill = RETIRE_CAPACITY / 2;
    for i in 0..fill + 10 {
        if i == fill + 2 {
            engine.pause_collector(false);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        engine.load([&quieter, &wider, &doc][i % 3], song).unwrap();
        armed_block(&mut b, 256);
    }
    assert!(engine.counters().swaps_deferred >= 1);
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations during graph swaps"
    );
    // Let the deferred swaps land, then check they all did.
    let drain = std::time::Instant::now();
    while engine.graphs_applied() - applied < (fill + 10) as u64 && drain.elapsed().as_secs() < 5 {
        armed_block(&mut b, 256);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations draining deferred swaps"
    );
    assert_eq!(
        engine.graphs_applied() - applied,
        (fill + 10) as u64,
        "swaps applied"
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
    let c = engine.counters();
    assert!(c.deadline_misses >= 1, "{c:?}");
    flag.store(true, Ordering::Release);
    // Wait (up to 2 s) for a block that neither misses nor finds the
    // stalled node still running: the insert recovered.
    let mut recovered = false;
    for _ in 0..2000 {
        let c = engine.counters();
        armed_block(&mut b, 512);
        let d = engine.counters();
        if d.deadline_misses == c.deadline_misses
            && d.busy_blocks == c.busy_blocks
            && c.busy_blocks >= 1
        {
            recovered = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let c = engine.counters();
    assert!(c.busy_blocks >= 1, "{c:?}");
    assert!(recovered, "the stalled insert never recovered: {c:?}");
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations on the deadline path"
    );
    drop(b.stop().unwrap());
    drop(engine);
    rt_check::set_arm_workers(false);
}
