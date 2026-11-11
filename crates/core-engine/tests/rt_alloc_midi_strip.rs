//! MIDI strips never allocate on the render path (plan 5g-1b Task 2): an
//! external destination with a mix plays notes (each counted as activity)
//! while `send_now` sends controllers and live thru plays, through a mute, an unmute, a stop
//! and a new play.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, port_key};
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::rt_check::{self, CountingAlloc};
use core_model::command::{Command, RoutingCmd};
use core_model::document::{DestKind, MidiMix};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

#[test]
fn midi_strips_never_allocate() {
    let mut bld = Builder::new();
    let ext = bld
        .dest_kind(
            "Ext",
            DestKind::MidiEndpoint {
                endpoint_ref: "virtual:out".into(),
                group: 0,
                channel: 3,
            },
            None,
        )
        .unwrap();
    let (seq, lane) = bld.sequence("S", "L", Some(ext)).unwrap();
    let notes: Vec<(i64, u8, u16, i64)> = (0..64)
        .map(|i| {
            (
                i * Q / 2,
                48 + (i % 24) as u8,
                ((i % 127 + 1) as u16) << 9,
                Q / 4,
            )
        })
        .collect();
    bld.notes(seq, lane, &notes).unwrap();
    let mix = |mute| MidiMix {
        volume: Some(90),
        pan: Some(20),
        mute,
    };
    bld.run(Command::Routing(RoutingCmd::SetMidiMix {
        dest: ext,
        mix: mix(false),
    }))
    .unwrap();
    let open = bld.doc().clone();
    bld.run(Command::Routing(RoutingCmd::SetMidiMix {
        dest: ext,
        mix: mix(true),
    }))
    .unwrap();
    let muted = bld.finish();

    let sinks = OfflineSinks::new(1 << 20);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    // Thru plays on it too (and drops notes while it is muted).
    engine.set_inputs(InputSpec {
        midi: vec![MidiInputSpec {
            lane,
            port: Some(port_key("Keys")),
            channel: None,
            dest: Some(ext),
            thru: true,
            armed: false,
            fx: Vec::new(),
            mpe: None,
        }],
        monitor: vec![],
    });
    engine.load(&open, seq).unwrap();
    let midi = engine.midi_sender();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.play().unwrap();
    let before = rt_check::count();
    let mut peaks = 0usize;
    for i in 0..2_000u32 {
        armed_block(&mut b, 1 + (i * 37) % 1024);
        if i % 50 == 0 {
            log.drain();
            peaks += engine.take_midi_activity(&open).len();
        }
        if i % 5 == 0 {
            let on = i % 10 == 0;
            let msg = Midi1Msg::three(if on { 0x90 } else { 0x80 }, 60 + (i % 12) as u8, 99);
            midi.send(MidiIn {
                port: port_key("Keys"),
                host_ns: 0,
                msg,
            });
        }
        if i % 7 == 0 {
            let v = (i % 128) as u8;
            engine
                .send_now(&open, ext, Midi1Msg::three(0xB0, 7, v))
                .unwrap();
        }
        if i == 400 {
            engine.load(&muted, seq).unwrap();
        }
        if i == 700 {
            engine.load(&open, seq).unwrap();
        }
        if i == 1200 {
            engine.stop().unwrap();
        }
        if i == 1300 {
            engine.play().unwrap();
        }
    }
    assert_eq!(rt_check::count() - before, 0, "allocations in MIDI strips");
    assert!(peaks > 0, "notes were sent and counted");
}
