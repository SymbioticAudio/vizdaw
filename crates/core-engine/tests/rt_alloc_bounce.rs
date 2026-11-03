//! A bounce's stem taps never allocate on the render path (plan 5f-2 Task
//! 2, pre-flight C1): the counting allocator is armed only around
//! `OfflineBackend::block`, while taps on the synth, a latent bus and master
//! capture every run of blocks of varying size that a fader lane and a mute
//! lane split into many runs. The taps are read back between blocks,
//! unarmed, as `bounce` does.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, demo_document};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::rt_check::{self, CountingAlloc};
use core_model::automation::AutomationTarget;
use core_model::command::MixerCmd;
use core_model::event::{EventBody, EventSpec};
use core_model::mixer::{AuxSend, StripKind};
use core_model::time::Tick;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

/// One test, so no other test thread allocates while the global counter
/// is compared.
#[test]
fn stem_taps_never_allocate() {
    let before = rt_check::count();
    rt_check::arm();
    drop(Vec::<u8>::with_capacity(10));
    rt_check::disarm();
    assert_eq!(
        rt_check::count() - before,
        2,
        "the counter works on this thread"
    );

    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let bus = b.bus("Latent").unwrap();
    b.insert(bus, TestInserts::plugin("latency=300")).unwrap();
    b.mixer(MixerCmd::AddSend {
        strip: synth,
        send: AuxSend::new(bus),
    })
    .unwrap();
    let lane = b.lane(song, "Auto", None).unwrap();
    let q = 15_360i64;
    let mut events = Vec::new();
    // A fader point every 64 ticks (100 frames) and a mute toggled every beat.
    for k in 0..(16 * q / 64) {
        events.push(EventSpec::new(
            Tick(k * 64),
            EventBody::Automation {
                target: AutomationTarget::StripFader(synth),
                value: (k % 50) as f64 / 50.0,
            },
        ));
    }
    for k in 0..16 {
        events.push(EventSpec::new(
            Tick(k * q),
            EventBody::Automation {
                target: AutomationTarget::StripMute(synth),
                value: (k % 2) as f64,
            },
        ));
    }
    b.events(song, lane, events).unwrap();
    let doc = b.finish();
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, song).unwrap();
    let mut out = OfflineBackend::new(48_000.0, 512);
    out.start(renderer).unwrap();
    out.reserve(4096);
    out.renderer_mut().unwrap().set_stem_taps(&[synth, bus]);
    engine.play().unwrap();
    let before = rt_check::count();
    let (mut most_gates, mut loud, mut taps) = (0usize, false, 0usize);
    for i in 0..3000u32 {
        rt_check::arm();
        out.block(1 + (i * 37) % 1024);
        rt_check::disarm();
        let r = out.renderer_mut().unwrap();
        let t = r.stem_taps();
        taps = t.len();
        for tap in &t {
            most_gates = most_gates.max(tap.gates.len());
            loud |= tap.left.iter().any(|x| *x != 0.0);
        }
        if i % 100 == 0 {
            log.drain();
        }
    }
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations while the taps captured"
    );
    assert_eq!(taps, 3, "synth, bus and master");
    assert!(loud, "the taps captured the synth");
    assert!(
        most_gates >= 2,
        "a mute toggled inside a block was recorded per run"
    );
    drop(out.stop().unwrap());
    drop(engine);
}
