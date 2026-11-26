//! Zero allocations on the callback and worker threads with real CLAP and
//! VST3 plugins in the graph: inserts, multi-output instruments, a
//! sidechain, parameter changes in, gestures out, a fault bypass, more
//! ramping automation on one plugin than its queue holds (the collapse
//! branch), and (VST3, host track H1 Task 4) notes, `IMidiMapping`
//! controllers, plain-to-normalised automation, output parameter changes
//! and controller edits routed to a running processor.

mod common;

use common::plugin;
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, Q, demo_document};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::rt_check::{self, CountingAlloc};
use core_host::PluginHost;
use core_host::adapt::automation_collapses;
use core_model::automation::AutomationTarget;
use core_model::command::MixerCmd;
use core_model::event::{EventBody, EventSpec};
use core_model::mixer::{AuxSend, ParamId, PluginFormat, PluginRef, StripKind};
use core_model::time::Tick;
use std::time::Duration;
use test_plugins::{
    BANK_PARAMS, PARAM_BANK_FIRST, VST3_GAIN_ID, VST3_PARAM_GAIN, VST3_PARAM_MODE, VST3_SYNTH_ID,
    Vst3Edit, Vst3Probe,
};

/// The reference of test VST3 class `id`.
fn vst3(id: &str, name: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Vst3,
        id: id.into(),
        name: name.into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

#[test]
fn plugins_render_without_allocating() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let builtin = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let gain = b.insert(builtin, plugin("vizdaw.test.gain")).unwrap();
    let bank = b.insert(builtin, plugin("vizdaw.test.bank")).unwrap();
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
    // Automation of the gain insert's parameter and the built-in fader:
    // plugin parameter changes and split runs on the armed path.
    let auto = b.lane(song, "Auto", None).unwrap();
    let mut points = Vec::new();
    for k in 0..256i64 {
        points.push(EventSpec::new(
            Tick(k * Q / 8),
            EventBody::Automation {
                target: AutomationTarget::PluginParam(gain, ParamId(0)),
                value: (k % 4) as f64 / 4.0,
            },
        ));
        points.push(EventSpec::new(
            Tick(k * Q / 8 + 1),
            EventBody::Automation {
                target: AutomationTarget::StripFader(builtin),
                value: (k % 5) as f64 / 5.0,
            },
        ));
    }
    b.events(song, auto, points).unwrap();
    // VST3: an insert with automation (the adapter makes plain dB and a
    // stepped value of it, the processor normalises them) and an instrument with notes, mapped controllers (CC 1,
    // pitch bend, channel pressure) and poly pressure.
    let vgain = b.insert(builtin, vst3(VST3_GAIN_ID, "Test Gain")).unwrap();
    let vauto = b.lane(song, "VST3 Auto", None).unwrap();
    let vpoints = (0..256i64)
        .flat_map(|k| {
            [
                EventSpec::new(
                    Tick(k * Q / 8),
                    EventBody::Automation {
                        target: AutomationTarget::PluginParam(
                            vgain,
                            ParamId(u64::from(VST3_PARAM_GAIN)),
                        ),
                        value: (k % 8) as f64 / 8.0,
                    },
                ),
                EventSpec::new(
                    Tick(k * Q / 8 + 3),
                    EventBody::Automation {
                        target: AutomationTarget::PluginParam(
                            vgain,
                            ParamId(u64::from(VST3_PARAM_MODE)),
                        ),
                        value: (k % 3) as f64 / 2.0,
                    },
                ),
            ]
        })
        .collect();
    b.events(song, vauto, vpoints).unwrap();
    let (vd, _) = b
        .plugin_dest("VSynth", vst3(VST3_SYNTH_ID, "Test Synth"))
        .unwrap();
    let vlane = b.lane(song, "VKeys", Some(vd)).unwrap();
    let mut vevents = Vec::new();
    for k in 0..64i64 {
        let t = Tick(k * Q / 2);
        vevents.push(EventSpec::new(
            t,
            EventBody::note(48 + (k % 12) as u8, 0xC000, Tick(Q / 4)),
        ));
        vevents.push(EventSpec::new(
            Tick(k * Q / 2 + 5),
            EventBody::Cc {
                index: 1,
                value: (k as u32 % 128) << 25,
            },
        ));
        vevents.push(EventSpec::new(
            Tick(k * Q / 2 + 7),
            EventBody::PitchBend {
                value: (k as u32 % 64) << 26,
            },
        ));
        vevents.push(EventSpec::new(
            Tick(k * Q / 2 + 9),
            EventBody::ChannelPressure {
                value: (k as u32 % 128) << 25,
            },
        ));
        vevents.push(EventSpec::new(
            Tick(k * Q / 2 + 11),
            EventBody::PolyPressure {
                pitch: 48 + (k % 12) as u8,
                value: 1 << 30,
            },
        ));
    }
    b.events(song, vlane, vevents).unwrap();
    // 24 ramping lanes on one plugin (plan 4d follow-up): a long run sends
    // each parameter about 17 values, 400 in all, more than the adapter's
    // 256-entry queue, so it collapses to the latest value per parameter.
    for k in 0..BANK_PARAMS {
        let lane = b.lane(song, &format!("Bank {k}"), None).unwrap();
        let ramp = (0..256i64)
            .map(|j| {
                EventSpec::new(
                    Tick(j * Q / 2 + i64::from(k) * 7),
                    EventBody::Automation {
                        target: AutomationTarget::PluginParam(
                            bank,
                            ParamId(u64::from(PARAM_BANK_FIRST + k)),
                        ),
                        value: (j % 2) as f64,
                    },
                )
            })
            .collect();
        b.events(song, lane, ramp).unwrap();
    }
    let doc = b.finish();

    let mut host = PluginHost::new(48_000.0, 4096).unwrap();
    host.register_path(
        PluginFormat::Clap,
        "vizdaw.test.gain",
        test_plugins::bundle(),
    );
    for id in ["synth4", "ducker", "nan", "bank"] {
        host.register_path(
            PluginFormat::Clap,
            &format!("vizdaw.test.{id}"),
            test_plugins::bundle(),
        );
    }
    for id in [VST3_GAIN_ID, VST3_SYNTH_ID] {
        host.register_path(PluginFormat::Vst3, id, test_plugins::vst3_bundle());
    }
    // The test VST3's latest controller makes edits, as its editor would.
    let vprobe = Vst3Probe::open(&test_plugins::vst3_bundle());
    host.sync(&doc);
    // Generous for a loaded machine; returns as soon as all seven are in.
    let r = host.wait_ready(Duration::from_secs(60));
    assert_eq!(
        r.loaded.len(),
        7,
        "failed: {:?}; still loading: {:?}",
        r.failed,
        host.loading()
    );
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
    let collapses = automation_collapses();
    let before = rt_check::count();
    for i in 0..2000u32 {
        if i % 50 == 0 {
            host.set_param(gain, ParamId(0), 0.5 + f64::from(i % 100) / 400.0);
        }
        if i % 400 == 0 {
            // A UI-style gesture comes out of the plugin.
            host.set_param(gain, ParamId(2), 0.75);
        }
        if i % 100 == 50 {
            // A VST3 controller edit, which the host routes to the
            // running processor; and a host value in plain units.
            vprobe.edit(Vst3Edit::Begin(VST3_PARAM_GAIN));
            vprobe.edit(Vst3Edit::Perform(
                VST3_PARAM_GAIN,
                f64::from(i % 1000) / 1000.0,
            ));
            vprobe.edit(Vst3Edit::End(VST3_PARAM_GAIN));
            host.set_param(vgain, ParamId(u64::from(VST3_PARAM_GAIN)), -6.0);
        }
        rt_check::arm();
        be.block(1 + (i * 37) % 1024);
        rt_check::disarm();
        if i % 100 == 0 || i % 100 == 51 {
            host.idle();
        }
    }
    let allocs = rt_check::count() - before;
    rt_check::set_arm_workers(false);
    assert_eq!(allocs, 0, "allocations while plugins rendered");
    // The VST3 plug-ins ran: blocks, notes, mapped controllers and host
    // values reached them, and nothing came out of order.
    let c = vprobe.counters();
    assert!(c.process_calls > 4_000, "{c:?}");
    assert!(
        c.note_ons > 0 && c.note_offs > 0 && c.poly_pressures > 0,
        "{c:?}"
    );
    for id in [
        VST3_PARAM_GAIN,
        VST3_PARAM_MODE,
        test_plugins::VST3_PARAM_MOD,
        test_plugins::VST3_PARAM_BEND,
        test_plugins::VST3_PARAM_PRESSURE,
    ] {
        assert!(c.param_in(id).is_some(), "{id}: {c:?}");
    }
    assert_eq!(c.bad_process, 0, "{c:?}");
    assert!(engine.counters().runs > 2_000, "automation split blocks");
    assert!(
        automation_collapses() > collapses,
        "the bank's queue never filled: the collapse branch did not run"
    );
    assert!(engine.worker_steps() > 0, "workers rendered nodes");
    assert_eq!(
        engine.plugin_faults().len(),
        1,
        "the NaN plugin was bypassed"
    );
    drop(be.stop().unwrap());
    drop(engine);
}
