//! Zero allocations on the callback and worker threads with real CLAP
//! plugins in the graph: inserts, a multi-output instrument, a sidechain,
//! parameter changes in, gestures out and a fault bypass.

mod common;

use common::plugin;
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, Q, demo_document};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::rt_check::{self, CountingAlloc};
use core_host::PluginHost;
use core_model::command::MixerCmd;
use core_model::event::{EventBody, EventSpec};
use core_model::mixer::{AuxSend, ParamId, PluginFormat, StripKind};
use core_model::time::Tick;
use std::time::Duration;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

#[test]
fn plugins_render_without_allocating() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let builtin = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let gain = b.insert(builtin, plugin("vizdaw.test.gain")).unwrap();
    let (d, _) = b
        .plugin_dest("Synth4", plugin("vizdaw.test.synth4"))
        .unwrap();
    let lane = b.lane(song, "Keys", Some(d)).unwrap();
    let mut notes = Vec::new();
    for ch in 0..4u8 {
        let mut e = EventSpec::new(
            Tick(i64::from(ch) * Q),
            EventBody::note(60 + ch, 0xFFFF, Tick(2 * Q)),
        );
        e.group_channel = Some((0, ch));
        notes.push(e);
    }
    b.events(song, lane, notes).unwrap();
    b.mixer(MixerCmd::AddInstrumentOutput {
        dest: d,
        index: 3,
        name: "Four".into(),
    })
    .unwrap();
    let keys = b.strip_of(StripKind::Instrument(d)).unwrap();
    let duck = b.insert(builtin, plugin("vizdaw.test.ducker")).unwrap();
    b.mixer(MixerCmd::SetSidechain {
        slot: duck,
        source: Some(keys),
    })
    .unwrap();
    let verb = b.bus("Verb").unwrap();
    b.insert(verb, plugin("vizdaw.test.nan")).unwrap();
    b.mixer(MixerCmd::AddSend {
        strip: builtin,
        send: AuxSend::new(verb),
    })
    .unwrap();
    let doc = b.finish();

    let mut host = PluginHost::new(48_000.0, 4096).unwrap();
    host.register_path(
        PluginFormat::Clap,
        "vizdaw.test.gain",
        test_plugins::bundle(),
    );
    for id in ["synth4", "ducker", "nan"] {
        host.register_path(
            PluginFormat::Clap,
            &format!("vizdaw.test.{id}"),
            test_plugins::bundle(),
        );
    }
    host.sync(&doc);
    let r = host.wait_ready(Duration::from_secs(10));
    assert_eq!(r.loaded.len(), 4, "{:?}", r.failed);
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.workers = 3;
    let (mut engine, renderer) =
        Engine::new(cfg, Box::new(OfflineSinks::new(1 << 20).with_tone())).unwrap();
    engine.set_insert_factory(Box::new(host.factory()));
    let report = engine.load(&doc, song).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    let mut be = OfflineBackend::new(48_000.0, 512);
    be.start(renderer).unwrap();
    be.reserve(4096);
    engine.set_loop(true).unwrap();
    engine.play().unwrap();
    // Warm up: first process calls start processing.
    for _ in 0..8 {
        be.block(256);
    }
    rt_check::set_arm_workers(true);
    let before = rt_check::count();
    for i in 0..2000u32 {
        if i % 50 == 0 {
            host.set_param(gain, ParamId(0), 0.5 + f64::from(i % 100) / 400.0);
        }
        if i % 400 == 0 {
            // A UI-style gesture comes out of the plugin.
            host.set_param(gain, ParamId(2), 0.75);
        }
        rt_check::arm();
        be.block(1 + (i * 37) % 1024);
        rt_check::disarm();
        if i % 100 == 0 {
            host.idle();
        }
    }
    let allocs = rt_check::count() - before;
    rt_check::set_arm_workers(false);
    assert_eq!(allocs, 0, "allocations while plugins rendered");
    assert!(engine.worker_steps() > 0, "workers rendered nodes");
    assert_eq!(
        engine.plugin_faults().len(),
        1,
        "the NaN plugin was bypassed"
    );
    drop(be.stop().unwrap());
    drop(engine);
}
