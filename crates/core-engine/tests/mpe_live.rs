//! MPE through the renderer (plan 5g-2 Task 3; Rulings 1, 4, 7, 9; P1,
//! P2, P4): an MPE track's input is decoded before its effects and encoded
//! again for an MPE destination, byte for byte; a plain destination keeps
//! pressure and counts what it drops; an MCM reconfigures, is reported and
//! keeps the route; an MPE destination's mute, meter and `send_now` use
//! its zone.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, port_key};
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{EventLog, FakeInput, OfflineBackend, OfflineSinks};
use core_model::command::{Command, RoutingCmd};
use core_model::document::{DestKind, Document, MidiMix};
use core_model::ids::{DestId, LaneId, SeqId};
use core_model::input_fx::{InputFx, InputFxKind};
use core_model::mpe::{MpeConfig, MpeZone};

const SR: f64 = 48_000.0;

/// Lane "Keys" on destination "Synth" (channel 3), MPE output on it when
/// `mpe_out`.
fn doc(mpe_out: bool) -> (Document, SeqId, LaneId, DestId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 3).unwrap();
    let (seq, keys) = b.sequence("Bar", "Keys", Some(synth)).unwrap();
    b.notes(seq, keys, &[(8 * Q, 30, 0x8000, Q)]).unwrap();
    if mpe_out {
        b.run(Command::Routing(RoutingCmd::SetMpe {
            dest: synth,
            mpe: Some(MpeConfig::default()),
        }))
        .unwrap();
    }
    (b.finish(), seq, keys, synth)
}

fn spec(lane: LaneId, dest: DestId, fx: Vec<InputFx>) -> MidiInputSpec {
    MidiInputSpec {
        lane,
        port: Some(port_key("MPE Keys")),
        channel: Some(0),
        dest: Some(dest),
        thru: true,
        armed: false,
        fx,
        mpe: Some(MpeConfig::default()),
    }
}

fn start(d: &Document, seq: SeqId, s: MidiInputSpec) -> (Engine, OfflineBackend, EventLog) {
    let sinks = OfflineSinks::new(100_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.set_inputs(InputSpec {
        midi: vec![s],
        monitor: vec![],
    });
    engine.load(d, seq).unwrap();
    let mut b = OfflineBackend::new(SR, 256).with_input(FakeInput::Signals);
    b.start(renderer).unwrap();
    (engine, b, log)
}

fn send(engine: &Engine, bytes: &[[u8; 3]]) {
    for b in bytes {
        let msg = match b[0] & 0xF0 {
            0xD0 => Midi1Msg::two(b[0], b[1]),
            _ => Midi1Msg::three(b[0], b[1], b[2]),
        };
        assert!(engine.midi_sender().send(MidiIn {
            port: port_key("MPE Keys"),
            host_ns: 0,
            msg
        }));
    }
}

/// The log's bytes, without RPN traffic (the configuration message).
fn bytes(log: &EventLog) -> Vec<Vec<u8>> {
    log.drain()
        .into_iter()
        .map(|r| r.bytes().to_vec())
        .filter(|b| !(b[0] & 0xF0 == 0xB0 && [101, 100, 6, 38].contains(&b[1])))
        .collect()
}

const PHRASE: [[u8; 3]; 12] = [
    [0xE1, 0x00, 0x50],
    [0xB1, 74, 0x30],
    [0xD1, 0x20, 0],
    [0x91, 60, 100],
    [0xE1, 0x10, 0x60],
    [0xD1, 0x40, 0],
    [0x81, 60, 0],
    [0xE2, 0x00, 0x40],
    [0xB2, 74, 0x40],
    [0xD2, 0x00, 0],
    [0x92, 64, 100],
    [0x82, 64, 0],
];

#[test]
fn live_mpe_round_trips_to_an_mpe_destination() {
    let (d, seq, keys, synth) = doc(true);
    let (engine, mut b, log) = start(&d, seq, spec(keys, synth, vec![]));
    b.run_frames(256);
    let first = log.drain();
    assert!(
        first.iter().any(|r| r.bytes() == [0xB0, 100, 6]),
        "the route's install sent the MCM: {:?}",
        first.iter().map(|r| r.bytes().to_vec()).collect::<Vec<_>>()
    );
    send(&engine, &PHRASE);
    b.run_frames(256);
    let want: Vec<Vec<u8>> = PHRASE
        .iter()
        .map(|b| {
            if b[0] & 0xF0 == 0xD0 {
                vec![b[0], b[1]]
            } else {
                b.to_vec()
            }
        })
        .collect();
    assert_eq!(bytes(&log), want);
}

#[test]
fn effects_move_expression_with_the_note() {
    let (d, seq, keys, synth) = doc(true);
    let fx = vec![InputFx::new(InputFxKind::Transpose { semitones: 12 })];
    let (engine, mut b, log) = start(&d, seq, spec(keys, synth, fx));
    b.run_frames(256);
    log.drain();
    send(
        &engine,
        &[[0x91, 60, 100], [0xE1, 0x10, 0x60], [0x81, 60, 0]],
    );
    b.run_frames(256);
    let got = bytes(&log);
    assert!(got.contains(&vec![0x91, 72, 100]), "{got:?}");
    assert!(
        got.contains(&vec![0xE1, 0x10, 0x60]),
        "the bend follows the transposed note: {got:?}"
    );
}

#[test]
fn a_plain_destination_keeps_pressure_and_counts_the_rest() {
    let (d, seq, keys, synth) = doc(false);
    let (engine, mut b, log) = start(&d, seq, spec(keys, synth, vec![]));
    b.run_frames(256);
    log.drain();
    send(
        &engine,
        &[
            [0x91, 60, 100],
            [0xD1, 0x40, 0],
            [0xE1, 0x10, 0x60],
            [0xB1, 74, 9],
            [0x81, 60, 0],
        ],
    );
    b.run_frames(256);
    assert_eq!(
        bytes(&log),
        vec![vec![0x93, 60, 100], vec![0xA3, 60, 0x40], vec![0x83, 60, 0]],
        "on the destination's channel; pressure as poly pressure"
    );
    assert_eq!(engine.counters().per_note_dropped, 2);
}

#[test]
fn an_mcm_is_reported_and_keeps_the_route() {
    let (d, seq, keys, synth) = doc(true);
    let (mut engine, mut b, log) = start(&d, seq, spec(keys, synth, vec![]));
    b.run_frames(256);
    send(&engine, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 7]]);
    b.run_frames(256);
    let seven = MpeConfig {
        zone: MpeZone::Lower,
        members: 7,
        bend_range: 48,
    };
    assert_eq!(engine.take_mpe_reports(), vec![(keys, Some(seven))]);
    assert!(engine.take_mpe_reports().is_empty());
    // Channel 9 is no longer a member.
    log.drain();
    send(&engine, &[[0x99, 60, 100]]);
    b.run_frames(256);
    assert!(bytes(&log).is_empty());
    // The control side writes the setting back: the route is carried (no MCM resent, nothing flushed).
    send(&engine, &[[0x91, 62, 100]]);
    b.run_frames(256);
    log.drain();
    let mut s = spec(keys, synth, vec![]);
    s.mpe = Some(seven);
    engine.set_inputs(InputSpec {
        midi: vec![s],
        monitor: vec![],
    });
    engine.load(&d, seq).unwrap();
    b.run_frames(256);
    let got: Vec<Vec<u8>> = log.drain().iter().map(|r| r.bytes().to_vec()).collect();
    assert!(
        !got.iter().any(|b| b[0] & 0xF0 == 0x80),
        "the held note was not cut: {got:?}"
    );
    assert!(
        !got.iter().any(|b| b == &vec![0xB0, 100, 6]),
        "no MCM resent: {got:?}"
    );
}

#[test]
fn mpe_stolen_is_counted() {
    let (d, seq, keys, synth) = {
        let mut b = Builder::new();
        let synth = b.dest("Synth", "builtin:synth", 3).unwrap();
        let (seq, keys) = b.sequence("Bar", "Keys", Some(synth)).unwrap();
        b.run(Command::Routing(RoutingCmd::SetMpe {
            dest: synth,
            mpe: Some(MpeConfig {
                zone: MpeZone::Lower,
                members: 2,
                bend_range: 48,
            }),
        }))
        .unwrap();
        (b.finish(), seq, keys, synth)
    };
    let (engine, mut b, _log) = start(&d, seq, spec(keys, synth, vec![]));
    send(
        &engine,
        &[[0x91, 60, 100], [0x92, 62, 100], [0x93, 64, 100]],
    );
    b.run_frames(256);
    assert_eq!(engine.counters().mpe_stolen, 1);
}

// An external MPE destination: the builtin synth does not take 5g-1b's
// mute, meter or `send_now` paths.

/// An external destination on `"virtual:out"` (as `midi_strip.rs`'s).
fn ext(b: &mut Builder, name: &str, channel: u8) -> DestId {
    b.dest_kind(
        name,
        DestKind::MidiEndpoint {
            endpoint_ref: "virtual:out".into(),
            group: 0,
            channel,
        },
        None,
    )
    .unwrap()
}

const UPPER3: MpeConfig = MpeConfig {
    zone: MpeZone::Upper,
    members: 3,
    bend_range: 48,
};

/// Lane "Keys" on an external destination on endpoint channel 3 with MPE
/// output Upper, 3 members; muted when `mute`.
fn ext_doc(mute: bool) -> (Document, SeqId, LaneId, DestId) {
    let mut b = Builder::new();
    let dest = ext(&mut b, "Ext", 2);
    let (seq, keys) = b.sequence("Bar", "Keys", Some(dest)).unwrap();
    b.notes(seq, keys, &[(8 * Q, 30, 0x8000, Q)]).unwrap();
    b.run(Command::Routing(RoutingCmd::SetMpe {
        dest,
        mpe: Some(UPPER3),
    }))
    .unwrap();
    b.run(Command::Routing(RoutingCmd::SetMidiMix {
        dest,
        mix: MidiMix {
            mute,
            ..MidiMix::default()
        },
    }))
    .unwrap();
    (b.finish(), seq, keys, dest)
}

#[test]
fn a_muted_mpe_destination_plays_no_member_notes() {
    let (d, seq, keys, dest) = ext_doc(true);
    let (engine, mut b, log) = start(&d, seq, spec(keys, dest, vec![]));
    b.run_frames(256);
    log.drain();
    send(&engine, &PHRASE);
    send(&engine, &[[0xB0, 64, 127]]);
    b.run_frames(256);
    let got = bytes(&log);
    let member = |b: &Vec<u8>| UPPER3.is_member(b[0] & 0x0F);
    assert!(
        !got.iter().any(|b| matches!(b[0] & 0xF0, 0x80 | 0x90)),
        "no notes: {got:?}"
    );
    assert!(
        !got.iter().any(|b| member(b)
            && (b[0] & 0xF0 == 0xE0 || b[0] & 0xF0 == 0xD0 || (b[0] & 0xF0 == 0xB0 && b[1] == 74))),
        "no member expression: {got:?}"
    );
    assert_eq!(
        got,
        vec![vec![0xBF, 64, 127]],
        "the sustain, on the manager channel"
    );
    send(
        &engine,
        &[
            [0x91, 60, 100],
            [0x92, 62, 100],
            [0x93, 64, 100],
            [0x94, 65, 100],
        ],
    );
    b.run_frames(256);
    assert!(bytes(&log).is_empty());
    assert_eq!(engine.counters().mpe_stolen, 0);
}

#[test]
fn an_mpe_destinations_meter_sees_rotated_notes() {
    let (d, seq, keys, dest) = ext_doc(false);
    let (engine, mut b, log) = start(&d, seq, spec(keys, dest, vec![]));
    b.run_frames(256);
    log.drain();
    send(&engine, &[[0x91, 60, 40], [0x92, 62, 100], [0x93, 64, 70]]);
    b.run_frames(256);
    let ons: Vec<Vec<u8>> = bytes(&log)
        .into_iter()
        .filter(|b| b[0] & 0xF0 == 0x90)
        .collect();
    assert_eq!(
        ons,
        vec![vec![0x9E, 60, 40], vec![0x9D, 62, 100], vec![0x9C, 64, 70]]
    );
    assert_eq!(engine.take_midi_activity(&d), vec![(dest, 100)]);
    assert_eq!(engine.take_midi_activity(&d), vec![]);
}

#[test]
fn send_now_uses_the_mpe_manager_channel() {
    let (d, seq, _keys, dest) = ext_doc(false);
    let sinks = OfflineSinks::new(100_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.load(&d, seq).unwrap();
    let mut b = OfflineBackend::new(SR, 256);
    b.start(renderer).unwrap();
    b.run_frames(256);
    log.drain();
    assert!(matches!(
        engine.send_now(&d, dest, Midi1Msg::three(0xB0, 7, 90)),
        Ok(true)
    ));
    b.run_frames(256);
    assert_eq!(bytes(&log), vec![vec![0xBF, 7, 90]]);
    let mut off = Builder::from_doc(d.clone());
    off.run(Command::Routing(RoutingCmd::SetMpe { dest, mpe: None }))
        .unwrap();
    let off = off.finish();
    assert!(matches!(
        engine.send_now(&off, dest, Midi1Msg::three(0xB0, 7, 90)),
        Ok(true)
    ));
    b.run_frames(256);
    assert_eq!(bytes(&log), vec![vec![0xB2, 7, 90]]);
}
