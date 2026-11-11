//! MPE never allocates on the render path (plan 5g-2 Task 3; P6, P15): an
//! MPE lane with a transpose and a chord plays an MPE destination with 3
//! members (so notes are stolen) while recording, and a second lane plays
//! the same phrase on a muted external MPE destination. Member notes,
//! bends, pressure and CC 74 arrive throughout, with an MCM, an RPN 0, a
//! carried route change (the write-back: encoders move into the new box),
//! a flushing one (another zone), a stop and a new play.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, port_key};
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::rt_check::{self, CountingAlloc};
use core_model::command::{Command, RoutingCmd};
use core_model::document::{DestKind, MidiMix};
use core_model::ids::LaneId;
use core_model::input_fx::{InputFx, InputFxKind};
use core_model::mpe::{MpeConfig, MpeZone};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

#[test]
fn mpe_never_allocates() {
    let mut bld = Builder::new();
    let synth = bld.dest("Synth", "builtin:synth", 0).unwrap();
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
    let (seq, keys) = bld.sequence("Bar", "Keys", Some(synth)).unwrap();
    let echo = bld.lane(seq, "Echo", Some(ext)).unwrap();
    bld.notes(seq, keys, &[(0, 40, 0x8000, Q), (16 * Q, 41, 0x8000, Q)])
        .unwrap();
    let three = |zone| MpeConfig {
        zone,
        members: 3,
        bend_range: 48,
    };
    bld.run(Command::Routing(RoutingCmd::SetMpe {
        dest: synth,
        mpe: Some(three(MpeZone::Lower)),
    }))
    .unwrap();
    bld.run(Command::Routing(RoutingCmd::SetMpe {
        dest: ext,
        mpe: Some(three(MpeZone::Upper)),
    }))
    .unwrap();
    bld.run(Command::Routing(RoutingCmd::SetMidiMix {
        dest: ext,
        mix: MidiMix {
            mute: true,
            ..MidiMix::default()
        },
    }))
    .unwrap();
    let doc = bld.finish();
    let fx = || {
        vec![
            InputFx::new(InputFxKind::Transpose { semitones: 5 }),
            InputFx::new(InputFxKind::Chord {
                intervals: vec![4, 7],
            }),
        ]
    };
    let lane = |lane: LaneId, dest, mpe| MidiInputSpec {
        lane,
        port: Some(port_key("MPE Keys")),
        channel: None,
        dest: Some(dest),
        thru: true,
        armed: lane == keys,
        fx: if lane == keys { fx() } else { vec![] },
        mpe: Some(mpe),
    };
    let both = |k: MpeConfig, e: MpeConfig| InputSpec {
        midi: vec![lane(keys, synth, k), lane(echo, ext, e)],
        monitor: vec![],
    };
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.set_inputs(both(MpeConfig::default(), MpeConfig::default()));
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
    let send = |m: Midi1Msg| {
        midi.send(MidiIn {
            port: port_key("MPE Keys"),
            host_ns: 0,
            msg: m,
        });
    };
    let mut live = (MpeConfig::default(), MpeConfig::default());
    let before = rt_check::count();
    for i in 0..2_000u32 {
        armed_block(&mut b, 1 + (i * 37) % 1024);
        if i % 50 == 0 {
            log.drain();
            while h.midi.pop().is_ok() {}
        }
        // Channels 1-3 are members of every zone used here.
        let ch = 1 + (i / 6 % 3) as u8;
        let v = (i % 128) as u8;
        match i % 6 {
            0 => send(Midi1Msg::three(0x90 | ch, 48 + (i % 24) as u8, 100)),
            1 => send(Midi1Msg::three(0xE0 | ch, v, 64)),
            2 => send(Midi1Msg::two(0xD0 | ch, v)),
            3 => send(Midi1Msg::three(0xB0 | ch, 74, v)),
            4 if i % 12 == 4 => send(Midi1Msg::three(0x80 | ch, 48 + ((i - 4) % 24) as u8, 0)),
            _ => {}
        }
        if i == 300 {
            // An MCM: 7 members.
            for m in [[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 7]] {
                send(Midi1Msg::three(m[0], m[1], m[2]));
            }
        }
        if i == 600 {
            // RPN 0 on a member: a 24-semitone range.
            for m in [[0xB1, 101, 0], [0xB1, 100, 0], [0xB1, 6, 24]] {
                send(Midi1Msg::three(m[0], m[1], m[2]));
            }
        }
        if i == 900 {
            // The write-back: both routes are carried, their encoders move.
            for (l, c) in engine.take_mpe_reports() {
                let c = c.expect("still on");
                if l == keys {
                    live.0 = c;
                } else {
                    live.1 = c;
                }
            }
            assert_eq!(live.0.members, 7);
            assert_eq!(live.0.bend_range, 24);
            engine.set_inputs(both(live.0, live.1));
            engine.load(&doc, seq).unwrap();
        }
        if i == 1200 {
            // Another zone: Keys' route flushes and its new one configures.
            let upper = MpeConfig {
                zone: MpeZone::Upper,
                ..MpeConfig::default()
            };
            engine.set_inputs(both(upper, live.1));
            engine.load(&doc, seq).unwrap();
        }
        if i == 1400 {
            engine.stop().unwrap();
        }
        if i == 1500 {
            engine.play().unwrap();
        }
    }
    assert_eq!(rt_check::count() - before, 0, "allocations in MPE");
    assert!(engine.counters().mpe_stolen > 0, "notes were stolen");
}
