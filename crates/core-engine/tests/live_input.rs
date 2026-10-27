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
    mic: LaneId,
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
        mic: audio,
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
        lane: d.mic,
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

/// Fix round 1, M2: Auto monitoring switches when the punch points are
/// heard, not `L` frames early. Since plan 5d-1 (Rulings 2 and 4) the
/// audio path reads its regions `L` ahead, so the punch points are tested
/// at that read frame and the input is heard with no compensation delay.
#[test]
fn auto_monitoring_switches_at_the_audible_punch_points() {
    use core_engine::graph::TestInserts;
    use core_model::mixer::StripKind;
    const L: u64 = 2400;
    let d = doc();
    let mut bb = Builder::from_doc(d.doc.clone());
    let strip = bb.strip_of(StripKind::BuiltinSynth).unwrap();
    bb.insert(strip, TestInserts::plugin(&format!("latency={L}")))
        .unwrap();
    let latent = bb.finish();
    let (mut engine, renderer) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10_000))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_inputs(InputSpec {
        midi: vec![],
        monitor: vec![MonitorRoute {
            lane: d.mic,
            path: d.path,
            left: 0,
            right: 0,
            mode: MonitorMode::Auto,
            armed: true,
        }],
    });
    engine.load(&latent, d.seq).unwrap();
    assert_eq!(engine.latency_frames(), L);
    let mut b = OfflineBackend::new(SR, 256).with_input(FakeInput::Signals);
    b.start(renderer).unwrap();
    b.run_frames(256 * 4);
    // Punch in at 40 000, playing from L frames before it.
    engine.locate_frame(40_000 - L).unwrap();
    let _h = engine
        .start_recording(&RecordRequest {
            channels: vec![0],
            range: Some((40_000, 45_000)),
            ring_frames: 48_000,
        })
        .unwrap();
    engine.play().unwrap();
    let peaks: Vec<f32> = (0..60).map(|_| b.block(256)).collect();
    // Block k renders playhead 37 600 + 256·k; the audio path reads
    // 35 200 + 256·k, which is what is heard then. The punch-in is read
    // from block 19 (40 064) and the punch-out from block 39 (45 184), and
    // the input is heard in those same blocks: nothing delays it.
    for (k, p) in peaks.iter().enumerate() {
        match k {
            0..=18 | 39.. => assert!(*p < 0.001, "block {k}: {p} ({peaks:?})"),
            _ => assert!(*p > 0.05, "block {k}: {p} ({peaks:?})"),
        }
    }
}

/// Plan 5d-1 Ruling 2 (spec §6.6; 5c carry PDC M4): a monitored input is
/// delayed only by the plugins on its own path, however latent the rest
/// of the mix is; without leads it waited for the slowest path.
#[test]
fn a_monitored_input_is_heard_through_its_own_path_only() {
    use core_engine::graph::TestInserts;
    use core_model::mixer::StripKind;
    const L: u32 = 2400;
    let d = doc();
    let impulses = |own: u32, leads: bool| -> Vec<usize> {
        let mut bb = Builder::from_doc(d.doc.clone());
        let synth = bb.strip_of(StripKind::BuiltinSynth).unwrap();
        bb.insert(synth, TestInserts::plugin(&format!("latency={L}")))
            .unwrap();
        if own > 0 {
            let path = bb.strip_of(StripKind::AudioPath(d.path)).unwrap();
            bb.insert(path, TestInserts::plugin(&format!("latency={own}")))
                .unwrap();
        }
        let latent = bb.finish();
        let mut cfg = EngineConfig::new(SR);
        cfg.region_leads = leads;
        let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(10_000))).unwrap();
        engine.set_insert_factory(Box::new(TestInserts::new()));
        // Fake input channel 2: an impulse every 0.5 s from frame 0.
        engine.set_inputs(InputSpec {
            midi: vec![],
            monitor: vec![MonitorRoute {
                lane: d.mic,
                path: d.path,
                left: 1,
                right: 1,
                mode: MonitorMode::Input,
                armed: false,
            }],
        });
        engine.load(&latent, d.seq).unwrap();
        assert_eq!(engine.latency_frames(), u64::from(L));
        let mut b = OfflineBackend::new(SR, 256).with_input(FakeInput::Signals);
        b.start(renderer).unwrap();
        let mut left = Vec::new();
        for _ in 0..120 {
            b.block(256);
            left.extend_from_slice(b.output(256).0);
        }
        left.iter()
            .enumerate()
            .filter(|(_, x)| x.abs() > 0.5)
            .map(|(i, _)| i)
            .collect()
    };
    assert_eq!(impulses(0, true), vec![0, 24_000]);
    assert_eq!(impulses(300, true), vec![300, 24_300]);
    assert_eq!(impulses(0, false), vec![2400, 26_400]);
}

/// Fix round 1, M1: a thru destination that cannot play is reported in
/// the load's report, not dropped silently.
#[test]
fn a_thru_destination_that_cannot_play_is_reported() {
    let mut b = Builder::new();
    let gone = b.dest("Gone", "Unplugged Port", 0).unwrap();
    let (seq, keys) = b.sequence("Bar", "Keys", None).unwrap();
    let doc = b.finish();
    let (mut engine, _renderer) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(100))).unwrap();
    let spec = |dest| InputSpec {
        midi: vec![MidiInputSpec {
            lane: keys,
            port: None,
            channel: None,
            dest,
            thru: true,
            armed: true,
        }],
        monitor: vec![],
    };
    engine.set_inputs(spec(Some(gone)));
    let r = engine.load(&doc, seq).unwrap();
    assert_eq!(r.thru_unroutable.len(), 1, "{r:?}");
    assert_eq!(r.thru_unroutable[0].0, keys);
    assert!(
        r.thru_unroutable[0].1.contains("Unplugged Port"),
        "{:?}",
        r.thru_unroutable
    );
    assert!(
        r.warnings.iter().any(|w| w.contains("thru")),
        "{:?}",
        r.warnings
    );
    // A destination no longer in the document.
    engine.set_inputs(spec(Some(DestId(9999))));
    let r = engine.load(&doc, seq).unwrap();
    assert_eq!(r.thru_unroutable.len(), 1, "{r:?}");
    // No thru, or no destination: nothing to report.
    let mut quiet = spec(Some(gone));
    quiet.midi[0].thru = false;
    engine.set_inputs(quiet);
    assert!(engine.load(&doc, seq).unwrap().thru_unroutable.is_empty());
    engine.set_inputs(spec(None));
    assert!(engine.load(&doc, seq).unwrap().thru_unroutable.is_empty());
}

/// Fix round 1, M1: thru past the target limit is reported too.
#[test]
fn thru_past_the_target_limit_is_reported() {
    use core_engine::backend::MidiPortInfo;
    use core_engine::schedule::MAX_TARGETS;
    let ports: Vec<MidiPortInfo> = (0..=MAX_TARGETS)
        .map(|i| MidiPortInfo {
            name: format!("Port {i}"),
            unique_id: None,
        })
        .collect();
    let mut b = Builder::new();
    let (seq, keys) = b.sequence("Many", "Keys", None).unwrap();
    for i in 0..MAX_TARGETS {
        let name = format!("Port {i}");
        let d = b.dest(&name, &name, 0).unwrap();
        let l = b.lane(seq, &name, Some(d)).unwrap();
        b.notes(seq, l, &[(0, 60, 0x8000, Q)]).unwrap();
    }
    let extra = format!("Port {MAX_TARGETS}");
    let over = b.dest(&extra, &extra, 0).unwrap();
    let doc = b.finish();
    let (mut engine, _renderer) = Engine::new(
        EngineConfig::new(SR),
        Box::new(OfflineSinks::new(100).with_ports(ports)),
    )
    .unwrap();
    engine.set_inputs(InputSpec {
        midi: vec![MidiInputSpec {
            lane: keys,
            port: None,
            channel: None,
            dest: Some(over),
            thru: true,
            armed: true,
        }],
        monitor: vec![],
    });
    let r = engine.load(&doc, seq).unwrap();
    assert_eq!(r.thru_unroutable.len(), 1, "{r:?}");
    assert_eq!(r.thru_unroutable[0].0, keys);
}

/// Writes 5 s of 0.25 stereo to `a.caf` in `dir`.
fn tone_file(dir: &std::path::Path) -> std::path::PathBuf {
    use core_media::caf::{CafWriter, WriteMode};
    let file = dir.join("a.caf");
    let mut w = CafWriter::create(
        &file,
        48_000,
        2,
        core_model::format::BitDepth::Float32,
        WriteMode::Normal,
    )
    .unwrap();
    w.write(&vec![0.25f32; 2 * 240_000]).unwrap();
    w.finish().unwrap();
    file
}

/// Per 256-frame block, whether anything loud is heard, with `route`
/// (if any) monitored and `range` recorded on fake input channel 3 (silent:
/// only regions can sound).
fn loud_blocks(
    doc: &Document,
    seq: SeqId,
    media: (core_model::ids::MediaId, &std::path::Path),
    route: Option<MonitorRoute>,
    range: Option<(u64, u64)>,
) -> Vec<bool> {
    use core_media::stream::{DiskMode, MediaFile};
    let mut cfg = EngineConfig::new(SR);
    cfg.disk = DiskMode::Manual;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(10_000))).unwrap();
    engine.set_media(vec![MediaFile {
        id: media.0,
        path: media.1.to_path_buf(),
    }]);
    engine.set_inputs(InputSpec {
        midi: vec![],
        monitor: route.into_iter().collect(),
    });
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(SR, 256).with_input(FakeInput::Signals);
    out.start(renderer).unwrap();
    let _tap = range.map(|r| {
        engine
            .start_recording(&RecordRequest {
                channels: vec![2],
                range: Some(r),
                ring_frames: 48_000,
            })
            .unwrap()
    });
    engine.play().unwrap();
    (0..220)
        .map(|_| {
            engine.streamer().service();
            out.block(256) > 0.1
        })
        .collect()
}

/// Plan 5d-1 Ruling 6 (5c carry M10): while a pass records, an armed lane
/// whose input is heard plays none of its own regions inside the record
/// range; outside it, unarmed, or with no recording, they play.
#[test]
fn the_recording_lanes_own_regions_are_silent_while_its_input_is_heard() {
    let dir = tempfile::tempdir().unwrap();
    let file = tone_file(dir.path());
    // Lane "Mic" plays a 5 s region of 0.25 on "Audio 1"; nothing else.
    let mut b = Builder::new();
    let (seq, mic) = b.sequence("Song", "Mic", None).unwrap();
    let m = b.media("Media/a.caf", 240_000, 2).unwrap();
    b.region(
        seq,
        mic,
        0,
        m,
        0,
        240_000,
        0.0,
        core_model::event::Fade::default(),
        core_model::event::Fade::default(),
    )
    .unwrap();
    let doc = b.finish();
    let path = doc.lane(seq, mic).unwrap().audio_out.unwrap();
    let route = |mode, armed| MonitorRoute {
        lane: mic,
        path,
        left: 2,
        right: 2,
        mode,
        armed,
    };
    let blocks = |r: Option<MonitorRoute>, range| loud_blocks(&doc, seq, (m, &file), r, range);
    // Block k reads frames from 256·k: the range [24 000, 48 000) is read
    // from block 94 (24 064) to block 187 (47 872).
    let silent_in_range: Vec<bool> = (0..220).map(|k| !(94..=187).contains(&k)).collect();
    let range = Some((24_000, 48_000));
    assert_eq!(
        blocks(Some(route(MonitorMode::Auto, true)), range),
        silent_in_range,
        "auto, armed, recording"
    );
    assert_eq!(
        blocks(Some(route(MonitorMode::Input, true)), range),
        silent_in_range,
        "input, armed, recording"
    );
    for (r, rec, why) in [
        (route(MonitorMode::Auto, true), None, "not recording"),
        (route(MonitorMode::Input, false), range, "unarmed"),
        (route(MonitorMode::Off, true), range, "monitor off"),
    ] {
        assert!(blocks(Some(r), rec).iter().all(|x| *x), "{why}");
    }
}

/// Ruling 6 / pre-flight M1: the decision is per route, whatever path the
/// route feeds: a lane whose reference plays a child's region to a second
/// output is silenced there too, while its input is heard on the first.
#[test]
fn a_recording_lane_is_silent_on_other_outputs_through_references() {
    let dir = tempfile::tempdir().unwrap();
    let file = tone_file(dir.path());
    let mut b = Builder::new();
    let (child, cl) = b.sequence("Loop", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", 240_000, 2).unwrap();
    let add_output = |b: &mut Builder, name: &str, reference: &str| {
        let r = b
            .run(Command::Routing(RoutingCmd::AddOutput {
                name: name.into(),
                reference: reference.into(),
            }))
            .unwrap();
        r.created
            .iter()
            .find_map(|id| match id {
                AnyId::Output(o) => Some(*o),
                _ => None,
            })
            .unwrap()
    };
    let first = add_output(&mut b, "Audio 1", "audio:1-2");
    let second = add_output(&mut b, "Audio 2", "audio:3-4");
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq: child,
        lane: cl,
        output: Some(second),
    }))
    .unwrap();
    b.region(
        child,
        cl,
        0,
        m,
        0,
        240_000,
        0.0,
        core_model::event::Fade::default(),
        core_model::event::Fade::default(),
    )
    .unwrap();
    let (song, mic) = b.sequence("Song", "Mic", None).unwrap();
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq: song,
        lane: mic,
        output: Some(first),
    }))
    .unwrap();
    b.reference(song, mic, 0, child, 1, 0).unwrap();
    let doc = b.finish();
    let route = |mode| MonitorRoute {
        lane: mic,
        path: first,
        left: 2,
        right: 2,
        mode,
        armed: true,
    };
    let range = Some((24_000, 48_000));
    let silent_in_range: Vec<bool> = (0..220).map(|k| !(94..=187).contains(&k)).collect();
    assert_eq!(
        loud_blocks(
            &doc,
            song,
            (m, &file),
            Some(route(MonitorMode::Auto)),
            range
        ),
        silent_in_range,
        "the referenced region on the second output is silent in range"
    );
    assert!(
        loud_blocks(&doc, song, (m, &file), Some(route(MonitorMode::Off)), range)
            .iter()
            .all(|x| *x),
        "off: it plays"
    );
}

/// Fix round (review m2, m3): silencing follows the lead. With a latent
/// insert elsewhere the recording lane's Regions node reads `L` ahead, and
/// its regions fall silent exactly where the heard record range is read
/// (block start `256·k − L` inside it), no earlier and no later; a region
/// on another lane of the same path keeps playing throughout.
#[test]
fn silencing_follows_the_lead_and_spares_other_lanes() {
    use core_engine::graph::TestInserts;
    use core_media::stream::{DiskMode, MediaFile};
    const L: u64 = 2400;
    let dir = tempfile::tempdir().unwrap();
    let file = tone_file(dir.path());
    let mut b = Builder::new();
    let (seq, mic) = b.sequence("Song", "Mic", None).unwrap();
    let gtr = b.lane(seq, "Gtr", None).unwrap();
    let m = b.media("Media/a.caf", 240_000, 2).unwrap();
    let fade = core_model::event::Fade::default;
    for lane in [mic, gtr] {
        b.region(seq, lane, 0, m, 0, 240_000, 0.0, fade(), fade())
            .unwrap();
    }
    let path = b.doc().lane(seq, mic).unwrap().audio_out.unwrap();
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane: gtr,
        output: Some(path),
    }))
    .unwrap();
    let strip = b.bus("Latent").unwrap();
    b.insert(strip, TestInserts::plugin(&format!("latency={L}")))
        .unwrap();
    let doc = b.finish();
    let mut cfg = EngineConfig::new(SR);
    cfg.disk = DiskMode::Manual;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(10_000))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_media(vec![MediaFile { id: m, path: file }]);
    engine.set_inputs(InputSpec {
        midi: vec![],
        monitor: vec![MonitorRoute {
            lane: mic,
            path,
            left: 2,
            right: 2,
            mode: MonitorMode::Auto,
            armed: true,
        }],
    });
    engine.load(&doc, seq).unwrap();
    assert_eq!(engine.latency_frames(), L);
    let mut out = OfflineBackend::new(SR, 256).with_input(FakeInput::Signals);
    out.start(renderer).unwrap();
    let _tap = engine
        .start_recording(&RecordRequest {
            channels: vec![2],
            range: Some((24_000, 48_000)),
            ring_frames: 48_000,
        })
        .unwrap();
    engine.play().unwrap();
    for k in 0..220u64 {
        engine.streamer().service();
        let peak = out.block(256);
        if k * 256 < L + 256 {
            continue; // the lead has not played yet
        }
        let read = k * 256 - L;
        if (24_000..48_000).contains(&read) {
            // Only "Gtr" sounds (0.25 at the centre pan law, 0.177).
            assert!((0.15..0.2).contains(&peak), "block {k}: {peak}");
        } else {
            // "Mic" and "Gtr" together (0.354).
            assert!(peak > 0.3, "block {k}: {peak}");
        }
    }
}

/// Plan 5d-1 Ruling 27 (spec §12.1 meters): every input channel's peak
/// is held whether or not a lane hears it, and a read clears it.
#[test]
fn input_channels_are_metered_whether_heard_or_not() {
    use core_engine::offline::FAKE_SINE_AMPLITUDE;
    let d = doc();
    let (engine, mut b, _log) = start(&d, InputSpec::default(), FakeInput::Signals);
    b.run_frames(2048);
    let p = engine.input_peaks(3);
    assert_eq!(p.len(), 3);
    assert!((p[0] - FAKE_SINE_AMPLITUDE).abs() < 0.01, "{p:?}");
    assert_eq!(p[1], 1.0, "the impulse at frame 0");
    assert_eq!(p[2], 0.0);
    b.run_frames(2048);
    let q = engine.input_peaks(3);
    assert!(q[0] > 0.2 && q[1] == 0.0, "{q:?}");
    assert_eq!(engine.input_peaks(3), vec![0.0; 3], "read and cleared");
}

/// Review M-a: clearing the held peaks (done when the input config
/// changes) leaves nothing stale for the first read after it.
#[test]
fn clearing_input_peaks_leaves_no_stale_peak() {
    let d = doc();
    let (engine, mut b, _log) = start(&d, InputSpec::default(), FakeInput::Signals);
    b.run_frames(2048);
    engine.clear_input_peaks();
    assert_eq!(engine.input_peaks(64), vec![0.0; 64]);
}

/// Review M-b: a non-finite sample is a clip, not silence.
#[test]
fn a_non_finite_input_sample_meters_as_a_clip() {
    use core_engine::render::block_peak;
    assert_eq!(block_peak(&[0.1, f32::NAN, 0.2], 3), 1.0);
    assert_eq!(block_peak(&[0.1, f32::INFINITY], 2), 1.0);
    assert_eq!(block_peak(&[0.1, f32::NEG_INFINITY], 2), 1.0);
    assert_eq!(
        block_peak(&[0.1, -0.4, f32::NAN], 2),
        0.4,
        "beyond n unseen"
    );
    assert_eq!(block_peak(&[0.3, -0.7], 2), 0.7);
}
