//! The input effect chain never allocates on the render path (plan 5g-1
//! Task 4; spec §3, §12.3 "`rt-check` covers the chain"): a lane with an
//! arpeggiator (latched, Up-Down, 2 octaves), a chord, a transpose, a
//! velocity and an Input Quantize plays thru and records while notes
//! arrive, through a chain change (the flush), another lane's arming (the
//! carry), a stop and a new play.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, port_key};
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::rt_check::{self, CountingAlloc};
use core_model::input_fx::{Arp, ArpMode, InputFx, InputFxKind, VelocityFx};
use core_model::time::Tick;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

#[test]
fn the_input_chain_never_allocates() {
    let mut bld = Builder::new();
    let synth = bld.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = bld.sequence("Bar", "Keys", Some(synth)).unwrap();
    let pads = bld.lane(seq, "Pads", Some(synth)).unwrap();
    bld.notes(seq, pads, &[(0, 40, 0x8000, Q), (16 * Q, 41, 0x8000, Q)])
        .unwrap();
    let doc = bld.finish();
    let chain = |semis: i8| {
        let a = Arp {
            mode: ArpMode::UpDown,
            octaves: 2,
            latch: true,
            ..Arp::default()
        };
        vec![
            InputFx::new(InputFxKind::Velocity(VelocityFx {
                scale: 120,
                ..VelocityFx::default()
            })),
            InputFx::new(InputFxKind::Chord {
                intervals: vec![3, 7, 10],
            }),
            InputFx::new(InputFxKind::Arpeggiator(a)),
            InputFx::new(InputFxKind::Transpose { semitones: semis }),
            InputFx::new(InputFxKind::InputQuantize {
                grid: Tick(Q / 4),
                strength: 100,
            }),
        ]
    };
    let lane = |lane, port: &str, fx| MidiInputSpec {
        lane,
        port: Some(port_key(port)),
        channel: None,
        dest: Some(synth),
        thru: true,
        armed: true,
        fx,
    };
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.set_inputs(InputSpec {
        midi: vec![lane(keys, "Keys", chain(0))],
        monitor: vec![],
    });
    engine.load(&doc, seq).unwrap();
    let midi = engine.midi_sender();
    let mut b = OfflineBackend::new(48_000.0, 512).with_input(FakeInput::Signals);
    b.start(renderer).unwrap();
    b.reserve(4096);
    let mut h = engine
        .start_recording(&RecordRequest {
            channels: vec![],
            range: None,
            ring_frames: 4800,
        })
        .unwrap();
    engine.play().unwrap();
    let before = rt_check::count();
    for i in 0..2_000u32 {
        armed_block(&mut b, 1 + (i * 37) % 1024);
        if i % 50 == 0 {
            log.drain();
            while h.midi.pop().is_ok() {}
        }
        if i % 9 == 0 {
            let on = i % 18 == 0;
            let msg = Midi1Msg::three(if on { 0x90 } else { 0x80 }, 48 + (i % 24) as u8, 100);
            midi.send(MidiIn {
                port: port_key("Keys"),
                host_ns: 0,
                msg,
            });
        }
        if i == 500 {
            // The chain changes: it flushes.
            engine.set_inputs(InputSpec {
                midi: vec![lane(keys, "Keys", chain(5))],
                monitor: vec![],
            });
            engine.load(&doc, seq).unwrap();
        }
        if i == 800 {
            // Another lane arms: Keys' chain is carried.
            engine.set_inputs(InputSpec {
                midi: vec![lane(keys, "Keys", chain(5)), lane(pads, "Pads", vec![])],
                monitor: vec![],
            });
            engine.load(&doc, seq).unwrap();
        }
        if i == 1200 {
            engine.stop().unwrap();
        }
        if i == 1300 {
            engine.play().unwrap();
        }
    }
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations in the input chain"
    );
    assert!(engine.counters().midi_fx_dropped < u64::MAX);
}
