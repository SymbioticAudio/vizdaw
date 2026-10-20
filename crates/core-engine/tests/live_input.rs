//! Live input (plan 5c Task 6; sub-project 5 spec §6.3, §6.6, §6.8's
//! hook): MIDI a lane listens to plays through to its destination at once
//! (stopped or playing) and, while recording an armed lane, is recorded at
//! the run frame heard when it arrived; re-routing ends thru notes; and an
//! audio lane's input is heard through its path by the monitor mode.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::input::{
    InputSpec, MidiIn, MidiInputSpec, MonitorRoute, channel_message, port_key,
};
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{EventLog, FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::{RecordRequest, RecordedMidi};
use core_model::command::{Command, LaneCmd, RoutingCmd};
use core_model::document::Document;
use core_model::ids::{AnyId, DestId, LaneId, OutputPathId, SeqId};
use core_model::lane::MonitorMode;

const SR: f64 = 48_000.0;

struct Doc {
    doc: Document,
    seq: SeqId,
    keys: LaneId,
    synth: DestId,
    path: OutputPathId,
}

/// A bar with one note on lane "Notes"; lane "Keys" plays on the synth
/// (channel 3) and has no events; lane "Mic" plays on output "Audio 1".
fn doc() -> Doc {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 3).unwrap();
    let (seq, notes) = b.sequence("Bar", "Notes", Some(synth)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, Q)]).unwrap();
    let keys = b.lane(seq, "Keys", Some(synth)).unwrap();
    let audio = b.lane(seq, "Mic", None).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: "Audio 1".into(),
            reference: "audio:1-2".into(),
        }))
        .unwrap();
    let path = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .unwrap();
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane: audio,
        output: Some(path),
    }))
    .unwrap();
    Doc {
        doc: b.finish(),
        seq,
        keys,
        synth,
        path,
    }
}

fn keys_spec(d: &Doc, thru: bool, armed: bool) -> MidiInputSpec {
    MidiInputSpec {
        lane: d.keys,
        port: Some(port_key("Keys")),
        channel: Some(0),
        dest: Some(d.synth),
        thru,
        armed,
    }
}

fn start(d: &Doc, spec: InputSpec, input: FakeInput) -> (Engine, OfflineBackend, EventLog) {
    let sinks = OfflineSinks::new(10_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.set_inputs(spec);
    engine.load(&d.doc, d.seq).unwrap();
    let mut b = OfflineBackend::new(SR, 256).with_input(input);
    b.start(renderer).unwrap();
    (engine, b, log)
}

fn note(on: bool, pitch: u8) -> Midi1Msg {
    if on {
        Midi1Msg::three(0x90, pitch, 100)
    } else {
        Midi1Msg::three(0x80, pitch, 0)
    }
}

fn send(engine: &Engine, port: &str, host_ns: u64, msg: Midi1Msg) {
    assert!(engine.midi_sender().send(MidiIn {
        port: port_key(port),
        host_ns,
        msg,
    }));
}

#[test]
fn channel_messages_are_parsed_and_others_refused() {
    assert_eq!(
        channel_message(&[0x90, 60, 100]),
        Some(Midi1Msg::three(0x90, 60, 100))
    );
    assert_eq!(channel_message(&[0xC5, 7]), Some(Midi1Msg::two(0xC5, 7)));
    for bad in [
        &[][..],
        &[0x90, 60],
        &[0x90, 60, 100, 1],
        &[0x90, 0x80, 100],
        &[0xF0, 1, 0xF7],
        &[0x40, 1, 2],
        &[0xF8],
    ] {
        assert_eq!(channel_message(bad), None, "{bad:?}");
    }
    assert_ne!(port_key("Keys"), port_key("keys"));
}

#[test]
fn thru_plays_at_once_on_the_destination_channel_stopped_or_playing() {
    let d = doc();
    let (engine, mut b, log) = start(
        &d,
        InputSpec {
            midi: vec![keys_spec(&d, true, false)],
            monitor: vec![],
        },
        FakeInput::Signals,
    );
    b.run_frames(512);
    log.drain();
    send(&engine, "Keys", 0, note(true, 64));
    // Another source, another channel: not this lane's.
    send(&engine, "Pads", 0, note(true, 65));
    send(&engine, "Keys", 0, Midi1Msg::three(0x91, 66, 100));
    b.run_frames(256);
    let got: Vec<(u64, Vec<u8>)> = log
        .drain()
        .into_iter()
        .map(|r| (r.frame, r.bytes().to_vec()))
        .collect();
    // On the synth's channel 3, at the block that took it.
    assert_eq!(got, vec![(512, vec![0x93, 64, 100])]);
}

#[test]
fn rerouting_ends_notes_thru_started() {
    let d = doc();
    let (mut engine, mut b, log) = start(
        &d,
        InputSpec {
            midi: vec![keys_spec(&d, true, true)],
            monitor: vec![],
        },
        FakeInput::Signals,
    );
    send(&engine, "Keys", 0, note(true, 64));
    b.run_frames(256);
    log.drain();
    // The lane is disarmed: its routes go and the held note ends.
    engine.set_inputs(InputSpec::default());
    engine.load(&d.doc, d.seq).unwrap();
    b.run_frames(256);
    let got: Vec<Vec<u8>> = log.drain().iter().map(|r| r.bytes().to_vec()).collect();
    assert!(got.contains(&vec![0x83, 64, 0]), "{got:?}");
    // Nothing more goes through.
    send(&engine, "Keys", 0, note(true, 67));
    b.run_frames(256);
    assert!(log.drain().is_empty());
}

#[test]
fn armed_lanes_record_at_the_run_heard_when_the_message_arrived() {
    let d = doc();
    let mut quiet = keys_spec(&d, false, false);
    quiet.lane = LaneId(99);
    quiet.channel = None;
    let (mut engine, mut b, _log) = start(
        &d,
        InputSpec {
            midi: vec![keys_spec(&d, true, true), quiet],
            monitor: vec![],
        },
        FakeInput::Signals,
    );
    let mut h = engine
        .start_recording(&RecordRequest {
            channels: vec![],
            range: None,
            ring_frames: 4800,
        })
        .unwrap();
    // Before the roll: not recorded.
    send(&engine, "Keys", 0, note(true, 50));
    b.run_frames(256);
    engine.play().unwrap();
    // The roll is at absolute frame 256 (run 0).
    b.run_frames(256 * 4);
    // Arrived at absolute frame 256 + 300 (the offline backend's host time
    // is frames ÷ rate), taken by a later block: recorded at run 300.
    let ns = |frame: u64| (frame as f64 * 1e9 / SR).round() as u64;
    send(&engine, "Keys", ns(256 + 300), note(true, 60));
    send(&engine, "Keys", ns(256 + 700), note(false, 60));
    // "Now": at the run of the block that takes it.
    send(&engine, "Keys", 0, note(true, 62));
    b.run_frames(256);
    let mut got: Vec<RecordedMidi> = Vec::new();
    while let Ok(m) = h.midi.pop() {
        got.push(m);
    }
    let runs: Vec<(u64, i64, u8)> = got
        .iter()
        .map(|m| (m.lane.0, m.run, m.msg.bytes[1]))
        .collect();
    assert_eq!(
        runs,
        vec![
            (d.keys.0, 300, 60),
            (d.keys.0, 700, 60),
            (d.keys.0, 1024, 62)
        ]
    );
    // Recorded as played (the chain is empty until plan 5g), not on the
    // destination's channel.
    assert_eq!(got[0].msg, note(true, 60));
    assert_eq!(h.status.lost_midi(), 0);
}

fn peak(b: &mut OfflineBackend, blocks: usize) -> f32 {
    let mut p = 0.0f32;
    for _ in 0..blocks {
        p = p.max(b.block(256));
    }
    p
}

#[test]
fn input_is_heard_through_its_path_by_the_monitor_mode() {
    let d = doc();
    let route = |mode, armed| MonitorRoute {
        path: d.path,
        left: 0,
        right: 0,
        mode,
        armed,
    };
    // Off: silence. Input: the sine, stopped or playing.
    for (mode, armed, heard) in [
        (MonitorMode::Off, true, false),
        (MonitorMode::Input, false, true),
        (MonitorMode::Auto, false, false),
        (MonitorMode::Auto, true, true),
    ] {
        let (_e, mut b, _log) = start(
            &d,
            InputSpec {
                midi: vec![],
                monitor: vec![route(mode, armed)],
            },
            FakeInput::Signals,
        );
        let p = peak(&mut b, 8);
        assert_eq!(p > 0.05, heard, "{mode:?} armed {armed}: {p}");
    }
    // Auto while armed: heard when stopped, not while only playing, again
    // while recording inside the record range, not outside it.
    let (mut engine, mut b, _log) = start(
        &d,
        InputSpec {
            midi: vec![],
            monitor: vec![route(MonitorMode::Auto, true)],
        },
        FakeInput::Signals,
    );
    assert!(peak(&mut b, 4) > 0.05, "stopped");
    engine.locate_frame(40_000).unwrap();
    engine.play().unwrap();
    b.block(256);
    assert!(peak(&mut b, 4) < 0.001, "playing back");
    let _h = engine
        .start_recording(&RecordRequest {
            channels: vec![0],
            range: Some((40_000, 45_000)),
            ring_frames: 48_000,
        })
        .unwrap();
    b.block(256);
    assert!(peak(&mut b, 4) > 0.05, "recording in range");
    // 40 000 + 6 × 256 played; run on past the range end.
    peak(&mut b, 16);
    assert!(peak(&mut b, 4) < 0.001, "recording past the range");
}

/// Pre-flight M12 (spec §6.3: external MIDI is scheduled at the time it is
/// heard) and M16 (thru reaches a destination the sequence never plays):
/// with `FakeInput::Loopback(1000)` the device reports 500 frames of
/// output latency, so a MIDI-out event for output frame `f` gets host
/// time `(f + 500)·1e9/sr`. The recording MIDI output logs
/// `host_ns·sr/1e9` as its frame, so it logs `f + 500`.
#[test]
fn external_midi_is_scheduled_at_the_audible_time() {
    use core_engine::backend::MidiPortInfo;
    let mut b = Builder::new();
    let ext = b.dest("Ext", "Ext Port", 2).unwrap();
    // No lane plays on "Other": only thru reaches it.
    let other = b.dest("Other", "Other Port", 5).unwrap();
    let (seq, notes) = b.sequence("Bar", "Notes", Some(ext)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, Q)]).unwrap();
    let keys = b.lane(seq, "Keys", None).unwrap();
    let doc = b.finish();
    let ports = ["Ext Port", "Other Port"]
        .iter()
        .map(|n| MidiPortInfo {
            name: (*n).into(),
            unique_id: None,
        })
        .collect();
    let sinks = OfflineSinks::new(10_000).with_ports(ports);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.set_inputs(InputSpec {
        midi: vec![MidiInputSpec {
            lane: keys,
            port: None,
            channel: None,
            dest: Some(other),
            thru: true,
            armed: false,
        }],
        monitor: vec![],
    });
    engine.load(&doc, seq).unwrap();
    let mut be = OfflineBackend::new(SR, 256).with_input(FakeInput::Loopback(1000));
    be.start(renderer).unwrap();
    be.run_frames(1024);
    assert!(log.drain().is_empty());
    // The sequence's note at output frame 1024.
    engine.play().unwrap();
    be.run_frames(256);
    let got: Vec<(u64, Vec<u8>)> = log
        .drain()
        .into_iter()
        .map(|r| (r.frame, r.bytes().to_vec()))
        .collect();
    assert!(got.contains(&(1024 + 500, vec![0x92, 60, 64])), "{got:?}");
    // Thru, taken by the block at output frame 1280, on "Other"'s
    // channel 6 (it plays at the block's start).
    send(&engine, "Anything", 0, note(true, 70));
    be.run_frames(256);
    let thru: Vec<(u64, Vec<u8>)> = log
        .drain()
        .into_iter()
        .map(|r| (r.frame, r.bytes().to_vec()))
        .filter(|(_, m)| m.get(1) == Some(&70))
        .collect();
    assert_eq!(thru, vec![(1280 + 500, vec![0x95, 70, 100])]);
}

/// Consistent with the tap (plan 5c Task 5): after a Stop only what was
/// heard before it is recorded; thru plays on regardless.
#[test]
fn after_a_stop_only_messages_heard_before_it_are_recorded() {
    let d = doc();
    let (mut engine, mut b, log) = start(
        &d,
        InputSpec {
            midi: vec![keys_spec(&d, true, true)],
            monitor: vec![],
        },
        FakeInput::Signals,
    );
    let mut h = engine
        .start_recording(&RecordRequest {
            channels: vec![],
            range: None,
            ring_frames: 4800,
        })
        .unwrap();
    engine.play().unwrap();
    // Rolls at absolute frame 0 (run 0).
    b.run_frames(1024);
    engine.stop().unwrap();
    let ns = |frame: u64| (frame as f64 * 1e9 / SR).round() as u64;
    // Heard at run 1000, before the stop: recorded.
    send(&engine, "Keys", ns(1000), note(true, 60));
    // "Now", in the block that stopped: not.
    send(&engine, "Keys", 0, note(true, 61));
    b.run_frames(256);
    log.drain();
    // Stopped: not recorded, but thru still plays.
    send(&engine, "Keys", 0, note(true, 62));
    b.run_frames(256);
    let thru: Vec<Vec<u8>> = log.drain().iter().map(|r| r.bytes().to_vec()).collect();
    assert!(thru.contains(&vec![0x93, 62, 100]), "{thru:?}");
    let mut got = Vec::new();
    while let Ok(m) = h.midi.pop() {
        got.push((m.run, m.msg.bytes[1]));
    }
    assert_eq!(got, vec![(1000, 60)]);
}
