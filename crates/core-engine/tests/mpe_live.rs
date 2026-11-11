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

// Fix round 1 (Task 3 review): route changes end what they no longer hear.

/// Every logged message, RPN traffic included.
fn all(log: &EventLog) -> Vec<Vec<u8>> {
    log.drain().iter().map(|r| r.bytes().to_vec()).collect()
}

fn offs(got: &[Vec<u8>], status: u8, pitch: u8) -> usize {
    got.iter()
        .filter(|b| b[0] == status && b[1] == pitch)
        .count()
}

#[test]
fn a_zone_off_write_back_with_a_channel_filter_ends_held_notes() {
    let (d, seq, keys, synth) = doc(false);
    let (mut engine, mut b, log) = start(&d, seq, spec(keys, synth, vec![]));
    b.run_frames(256);
    send(&engine, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 0]]);
    b.run_frames(256);
    assert_eq!(engine.take_mpe_reports(), vec![(keys, None)]);
    log.drain();
    // Zoned off, the lane hears every channel.
    send(&engine, &[[0x95, 60, 100]]);
    b.run_frames(256);
    assert_eq!(bytes(&log), vec![vec![0x93, 60, 100]]);
    // The write-back: no MPE, but the stale channel filter (0) narrows.
    let mut s = spec(keys, synth, vec![]);
    s.mpe = None;
    engine.set_inputs(InputSpec {
        midi: vec![s],
        monitor: vec![],
    });
    engine.load(&d, seq).unwrap();
    b.run_frames(256);
    send(&engine, &[[0x85, 60, 0]]);
    b.run_frames(256);
    let got = all(&log);
    assert_eq!(offs(&got, 0x83, 60), 1, "the held note ended: {got:?}");
}

#[test]
fn two_old_routes_carried_into_one_both_end_their_notes() {
    let (d, seq, keys, synth) = doc(true);
    let s = spec(keys, synth, vec![]);
    let (mut engine, mut b, log) = start(&d, seq, s.clone());
    engine.set_inputs(InputSpec {
        midi: vec![s.clone(), s.clone()],
        monitor: vec![],
    });
    engine.load(&d, seq).unwrap();
    b.run_frames(256);
    log.drain();
    send(&engine, &[[0x91, 60, 100]]);
    b.run_frames(256);
    let got = all(&log);
    assert_eq!(offs(&got, 0x91, 60), 2, "each route plays it: {got:?}");
    engine.set_inputs(InputSpec {
        midi: vec![s],
        monitor: vec![],
    });
    engine.load(&d, seq).unwrap();
    b.run_frames(256);
    send(&engine, &[[0x81, 60, 0]]);
    b.run_frames(256);
    let got = all(&log);
    assert_eq!(offs(&got, 0x81, 60), 2, "both copies end: {got:?}");
}

#[test]
fn a_new_mpe_output_ends_old_member_notes_before_configuring() {
    let (d, seq, keys, synth) = doc(true);
    let (mut engine, mut b, log) = start(&d, seq, spec(keys, synth, vec![]));
    b.run_frames(256);
    send(&engine, &[[0x91, 60, 100]]);
    b.run_frames(256);
    log.drain();
    let mut upper = Builder::from_doc(d.clone());
    upper
        .run(Command::Routing(RoutingCmd::SetMpe {
            dest: synth,
            mpe: Some(UPPER3),
        }))
        .unwrap();
    let upper = upper.finish();
    engine.load(&upper, seq).unwrap();
    b.run_frames(256);
    let got = all(&log);
    let off = got.iter().position(|b| b == &vec![0x81, 60, 0]);
    let mcm = got.iter().position(|b| b == &vec![0xBF, 100, 6]);
    assert!(
        off.is_some() && mcm.is_some() && off < mcm,
        "the note ends, then the MCM: {got:?}"
    );
}

#[test]
fn muting_an_mpe_destination_ends_a_held_note() {
    let (open, seq, keys, dest) = ext_doc(false);
    let (muted, ..) = ext_doc(true);
    let (mut engine, mut b, log) = start(&open, seq, spec(keys, dest, vec![]));
    b.run_frames(256);
    log.drain();
    send(&engine, &[[0x91, 60, 100]]);
    b.run_frames(256);
    assert_eq!(offs(&all(&log), 0x9E, 60), 1);
    engine.load(&muted, seq).unwrap();
    b.run_frames(256);
    send(&engine, &[[0x81, 60, 0], [0x92, 62, 100]]);
    b.run_frames(256);
    let got = all(&log);
    assert_eq!(
        offs(&got, 0x8E, 60),
        1,
        "ended once, on its member channel: {got:?}"
    );
    assert!(!got.iter().any(|b| b[0] & 0xF0 == 0x90), "muted: {got:?}");
    // Unmuted again: nothing was left sounding to end.
    engine.load(&open, seq).unwrap();
    b.run_frames(256);
    let got = all(&log);
    assert!(!got.iter().any(|b| b[0] & 0xF0 == 0x80), "{got:?}");
}

#[test]
fn a_chain_change_ends_chord_notes_on_their_member_channels() {
    let (d, seq, keys, synth) = doc(true);
    let chord = |i: Vec<i8>| vec![InputFx::new(InputFxKind::Chord { intervals: i })];
    let (mut engine, mut b, log) = start(&d, seq, spec(keys, synth, chord(vec![4, 7])));
    b.run_frames(256);
    log.drain();
    send(&engine, &[[0x91, 60, 100]]);
    b.run_frames(256);
    let ons: Vec<Vec<u8>> = all(&log)
        .into_iter()
        .filter(|b| b[0] & 0xF0 == 0x90)
        .collect();
    assert_eq!(
        ons,
        vec![
            vec![0x91, 60, 100],
            vec![0x92, 64, 100],
            vec![0x93, 67, 100]
        ]
    );
    engine.set_inputs(InputSpec {
        midi: vec![spec(keys, synth, chord(vec![3, 7]))],
        monitor: vec![],
    });
    engine.load(&d, seq).unwrap();
    b.run_frames(256);
    let mut got: Vec<Vec<u8>> = all(&log)
        .into_iter()
        .filter(|b| b[0] & 0xF0 == 0x80)
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![vec![0x81, 60, 0], vec![0x82, 64, 0], vec![0x83, 67, 0]],
        "no manager-channel off"
    );
}

#[test]
fn a_muted_plain_destination_counts_no_dropped_per_note_data() {
    let mut b = Builder::new();
    let dest = ext(&mut b, "Ext", 2);
    let (seq, keys) = b.sequence("Bar", "Keys", Some(dest)).unwrap();
    b.run(Command::Routing(RoutingCmd::SetMidiMix {
        dest,
        mix: MidiMix {
            mute: true,
            ..MidiMix::default()
        },
    }))
    .unwrap();
    let d = b.finish();
    let (engine, mut b, _log) = start(&d, seq, spec(keys, dest, vec![]));
    b.run_frames(256);
    send(
        &engine,
        &[
            [0x91, 60, 100],
            [0xE1, 0x10, 0x60],
            [0xB1, 74, 9],
            [0x81, 60, 0],
        ],
    );
    b.run_frames(256);
    assert_eq!(
        engine.counters().per_note_dropped,
        0,
        "nothing would have played"
    );
}

/// Final review m1: a reload between an MCM and its write-back (another
/// track's input changing) keeps the stored setting, so the route is carried
/// with the decoder's live setting: a held note keeps its later bend,
/// nothing is cut, and the MCM's zone still holds.
#[test]
fn an_unrelated_reload_before_the_write_back_carries_the_mcm() {
    let (d, seq, keys, synth, pads) = {
        let mut b = Builder::new();
        let synth = b.dest("Synth", "builtin:synth", 3).unwrap();
        let (seq, keys) = b.sequence("Bar", "Keys", Some(synth)).unwrap();
        let pads = b.lane(seq, "Pads", Some(synth)).unwrap();
        b.run(Command::Routing(RoutingCmd::SetMpe {
            dest: synth,
            mpe: Some(MpeConfig::default()),
        }))
        .unwrap();
        (b.finish(), seq, keys, synth, pads)
    };
    let (mut engine, mut b, log) = start(&d, seq, spec(keys, synth, vec![]));
    b.run_frames(256);
    send(&engine, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 7]]);
    b.run_frames(256);
    assert_eq!(engine.take_mpe_reports().len(), 1);
    send(&engine, &[[0x91, 62, 100]]);
    b.run_frames(256);
    log.drain();
    // The reload: another track starts listening; this track's stored
    // setting is unchanged (its write-back is still owed).
    let other = MidiInputSpec {
        lane: pads,
        port: Some(port_key("Other Keys")),
        channel: None,
        dest: Some(synth),
        thru: false,
        armed: false,
        fx: vec![],
        mpe: None,
    };
    engine.set_inputs(InputSpec {
        midi: vec![spec(keys, synth, vec![]), other],
        monitor: vec![],
    });
    engine.load(&d, seq).unwrap();
    b.run_frames(256);
    send(&engine, &[[0xE1, 0x00, 0x50]]);
    b.run_frames(256);
    let got = all(&log);
    assert!(
        !got.iter().any(|b| b[0] & 0xF0 == 0x80),
        "the held note was not cut: {got:?}"
    );
    assert!(
        !got.iter().any(|b| b == &vec![0xB0, 100, 6]),
        "no MCM resent: {got:?}"
    );
    assert!(
        got.contains(&vec![0xE1, 0x00, 0x50]),
        "the held note kept its later bend: {got:?}"
    );
    // The MCM's seven members still hold: channel 9 is not one.
    send(&engine, &[[0x99, 60, 100]]);
    b.run_frames(256);
    assert!(bytes(&log).is_empty());
}

/// Final review m1 with Ruling T3-a: a zone-off MCM, an unrelated reload
/// (carried, the lane still hears every channel), then the write-back with
/// a channel filter: the held note still ends, none hangs.
#[test]
fn a_zone_off_carried_across_a_reload_still_ends_at_the_write_back() {
    let mut bld = Builder::new();
    let synth = bld.dest("Synth", "builtin:synth", 3).unwrap();
    let (seq, keys) = bld.sequence("Bar", "Keys", Some(synth)).unwrap();
    let pads = bld.lane(seq, "Pads", Some(synth)).unwrap();
    let d = bld.finish();
    let other = MidiInputSpec {
        lane: pads,
        port: Some(port_key("Other Keys")),
        channel: None,
        dest: Some(synth),
        thru: false,
        armed: false,
        fx: vec![],
        mpe: None,
    };
    let (mut engine, mut b, log) = start(&d, seq, spec(keys, synth, vec![]));
    b.run_frames(256);
    send(&engine, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 0]]);
    b.run_frames(256);
    assert_eq!(engine.take_mpe_reports(), vec![(keys, None)]);
    send(&engine, &[[0x95, 60, 100]]);
    b.run_frames(256);
    log.drain();
    engine.set_inputs(InputSpec {
        midi: vec![spec(keys, synth, vec![]), other.clone()],
        monitor: vec![],
    });
    engine.load(&d, seq).unwrap();
    b.run_frames(256);
    let got = all(&log);
    assert_eq!(offs(&got, 0x83, 60), 0, "carried, not cut: {got:?}");
    let mut s = spec(keys, synth, vec![]);
    s.mpe = None;
    engine.set_inputs(InputSpec {
        midi: vec![s, other],
        monitor: vec![],
    });
    engine.load(&d, seq).unwrap();
    b.run_frames(256);
    send(&engine, &[[0x85, 60, 0]]);
    b.run_frames(256);
    let got = all(&log);
    assert_eq!(offs(&got, 0x83, 60), 1, "the held note ended: {got:?}");
}

// Final review m3: an external MPE destination removed, or moved to
// another endpoint, turns its old device's zone off after that device's
// notes end. Fake ports "Synth A" and "Synth B".

fn port_dest(b: &mut Builder, name: &str, port: &str, channel: u8) -> DestId {
    b.dest_kind(
        name,
        DestKind::MidiEndpoint {
            endpoint_ref: port.into(),
            group: 0,
            channel,
        },
        None,
    )
    .unwrap()
}

/// Dest "A" (MPE, Lower 15) on "Synth A" and plain dest "P" on `p_port`;
/// lane "Keys" plays a long note to A. Playing, the note sounding; returns
/// the engine, backend, log, the note's target, A and P.
fn playing_on_a(
    p_port: &str,
) -> (
    Engine,
    OfflineBackend,
    EventLog,
    core_engine::schedule::TargetIdx,
    Builder,
    SeqId,
    DestId,
    DestId,
) {
    let mut b = Builder::new();
    let a = port_dest(&mut b, "A", "Synth A", 0);
    let p = port_dest(&mut b, "P", p_port, 4);
    let (seq, keys) = b.sequence("Bar", "Keys", Some(a)).unwrap();
    b.notes(seq, keys, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
    b.run(Command::Routing(RoutingCmd::SetMpe {
        dest: a,
        mpe: Some(MpeConfig::default()),
    }))
    .unwrap();
    let info = |name: &str| core_engine::backend::MidiPortInfo {
        name: name.into(),
        unique_id: None,
    };
    let sinks = OfflineSinks::new(100_000).with_ports(vec![info("Synth A"), info("Synth B")]);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.load(b.doc(), seq).unwrap();
    let mut be = OfflineBackend::new(SR, 256);
    be.start(renderer).unwrap();
    engine.play().unwrap();
    be.run_frames(4096);
    let on = log
        .drain()
        .into_iter()
        .find(|r| r.bytes()[0] & 0xF0 == 0x90)
        .expect("the note started");
    (engine, be, log, on.target, b, seq, a, p)
}

/// The zone-off MCM on Lower's manager channel.
const LOWER_OFF: [[u8; 3]; 5] = [
    [0xB0, 101, 0],
    [0xB0, 100, 6],
    [0xB0, 6, 0],
    [0xB0, 101, 127],
    [0xB0, 100, 127],
];

/// What `t` got after the edit, and whether its note ended before the
/// zone-off MCM.
fn after_edit(
    engine: &mut Engine,
    be: &mut OfflineBackend,
    log: &EventLog,
    t: core_engine::schedule::TargetIdx,
    doc: &Document,
    seq: SeqId,
) -> Vec<Vec<u8>> {
    engine.load(doc, seq).unwrap();
    be.run_frames(4096);
    log.drain()
        .into_iter()
        .filter(|r| r.target == t)
        .map(|r| r.bytes().to_vec())
        .collect()
}

fn zone_off_after_note_off(got: &[Vec<u8>]) {
    let off = got
        .iter()
        .position(|b| b[0] & 0xF0 == 0x80 && b[1] == 60)
        .unwrap_or_else(|| panic!("the note ended: {got:?}"));
    let mcm = got
        .windows(5)
        .position(|w| w.iter().zip(LOWER_OFF).all(|(a, b)| a.as_slice() == b))
        .unwrap_or_else(|| panic!("the zone went off: {got:?}"));
    assert!(off < mcm, "notes end first: {got:?}");
    assert_eq!(
        got.iter().filter(|b| b.as_slice() == [0xB0, 6, 0]).count(),
        1,
        "one zone-off: {got:?}"
    );
}

#[test]
fn moving_an_mpe_destination_to_another_port_turns_the_old_zone_off() {
    let (mut engine, mut be, log, t, mut b, seq, a, _) = playing_on_a("Synth B");
    b.run(Command::Routing(RoutingCmd::EditDestination {
        dest: a,
        kind: DestKind::MidiEndpoint {
            endpoint_ref: "Synth B".into(),
            group: 0,
            channel: 0,
        },
        default_patch: None,
    }))
    .unwrap();
    let got = after_edit(&mut engine, &mut be, &log, t, b.doc(), seq);
    zone_off_after_note_off(&got);
}

#[test]
fn removing_an_mpe_destination_turns_its_zone_off_on_a_port_that_stays() {
    // P stays on "Synth A": the device's sink is kept.
    let (mut engine, mut be, log, t, mut b, seq, a, p) = playing_on_a("Synth A");
    b.run(Command::Routing(RoutingCmd::RemoveDestination {
        dest: a,
        replacement: Some(p),
    }))
    .unwrap();
    let got = after_edit(&mut engine, &mut be, &log, t, b.doc(), seq);
    zone_off_after_note_off(&got);
}

#[test]
fn removing_an_mpe_destination_turns_its_zone_off_on_a_port_that_goes() {
    let (mut engine, mut be, log, t, mut b, seq, a, p) = playing_on_a("Synth B");
    b.run(Command::Routing(RoutingCmd::RemoveDestination {
        dest: a,
        replacement: Some(p),
    }))
    .unwrap();
    let got = after_edit(&mut engine, &mut be, &log, t, b.doc(), seq);
    zone_off_after_note_off(&got);
}

#[test]
fn a_channel_edit_on_the_same_port_sends_no_zone_off() {
    let (mut engine, mut be, log, t, mut b, seq, a, _) = playing_on_a("Synth B");
    b.run(Command::Routing(RoutingCmd::EditDestination {
        dest: a,
        kind: DestKind::MidiEndpoint {
            endpoint_ref: "Synth A".into(),
            group: 0,
            channel: 5,
        },
        default_patch: None,
    }))
    .unwrap();
    let got = after_edit(&mut engine, &mut be, &log, t, b.doc(), seq);
    assert!(
        !got.iter().any(|b| b.as_slice() == [0xB0, 6, 0]),
        "no zone-off: {got:?}"
    );
}

/// Final review m4: a batch goes out whole or not at all. With room for
/// five commands, an eleven-message configuration is refused and takes
/// none of them.
#[test]
fn a_configuration_is_sent_whole_or_not_at_all() {
    let (d, seq, _, dest) = ext_doc(false);
    let sinks = OfflineSinks::new(100_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.load(&d, seq).unwrap();
    let mut be = OfflineBackend::new(SR, 256);
    be.start(renderer).unwrap();
    be.run_frames(256);
    log.drain();
    let one = Midi1Msg::three(0xB2, 7, 1);
    // Fill the ring (no block runs meanwhile), counting its room.
    let mut room = 0;
    while engine.send_now_on(dest, one).unwrap_or(false) {
        room += 1;
    }
    be.run_frames(256);
    log.drain();
    for _ in 0..room - 5 {
        assert!(engine.send_now_on(dest, one).unwrap());
    }
    let config = core_engine::mpe::configuration(&UPPER3);
    assert!(engine.send_all_now_on(dest, &config).is_err());
    for _ in 0..5 {
        assert!(engine.send_now_on(dest, one).unwrap(), "nothing was taken");
    }
    assert!(engine.send_now_on(dest, one).is_err());
    be.run_frames(256);
    assert!(
        log.drain().iter().all(|r| r.bytes() == [0xB2, 7, 1]),
        "no part of the configuration went out"
    );
    // With room, all of it.
    assert!(engine.send_all_now_on(dest, &config).unwrap());
    be.run_frames(256);
    let got: Vec<Vec<u8>> = log.drain().iter().map(|r| r.bytes().to_vec()).collect();
    let want: Vec<Vec<u8>> = config.iter().map(|m| m.as_slice().to_vec()).collect();
    assert_eq!(got, want);
}
