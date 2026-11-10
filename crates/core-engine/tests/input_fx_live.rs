//! The input effect chain in the renderer (plan 5g-1 Task 4; spec §6.6,
//! §6.8, §12.3; Rulings 5, 10–13): thru plays the chain's output at once
//! (Input Quantize never delays it), arpeggiator steps land on their frames stopped and playing,
//! the recorder hears what thru played, a changed chain ends its notes
//! while an unchanged one plays on, and overflow is counted. Offline
//! backend and sinks only.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, port_key};
use core_engine::input_fx::LiveMsg;
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{EventLog, FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_model::document::Document;
use core_model::ids::{DestId, LaneId, SeqId};
use core_model::input_fx::{Arp, ArpMode, InputFx, InputFxKind};
use core_model::time::Tick;

const SR: f64 = 48_000.0;

struct Doc {
    doc: Document,
    seq: SeqId,
    keys: LaneId,
    pads: LaneId,
    synth: DestId,
}

/// Lanes "Keys" and "Pads" on the synth (channel 3); a note on Pads at
/// beat 8 gives the sequence its length.
fn doc() -> Doc {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 3).unwrap();
    let (seq, keys) = b.sequence("Bar", "Keys", Some(synth)).unwrap();
    let pads = b.lane(seq, "Pads", Some(synth)).unwrap();
    b.notes(seq, pads, &[(8 * Q, 30, 0x8000, Q)]).unwrap();
    Doc {
        doc: b.finish(),
        seq,
        keys,
        pads,
        synth,
    }
}

fn spec(d: &Doc, lane: LaneId, port: &str, fx: Vec<InputFx>, armed: bool) -> MidiInputSpec {
    MidiInputSpec {
        lane,
        port: Some(port_key(port)),
        channel: None,
        dest: Some(d.synth),
        thru: true,
        armed,
        fx,
    }
}

fn arp(f: impl FnOnce(&mut Arp)) -> InputFx {
    let mut a = Arp::default();
    f(&mut a);
    InputFx::new(InputFxKind::Arpeggiator(a))
}

fn start(d: &Doc, midi: Vec<MidiInputSpec>) -> (Engine, OfflineBackend, EventLog) {
    let sinks = OfflineSinks::new(100_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.set_inputs(InputSpec {
        midi,
        monitor: vec![],
    });
    engine.load(&d.doc, d.seq).unwrap();
    let mut b = OfflineBackend::new(SR, 256).with_input(FakeInput::Signals);
    b.start(renderer).unwrap();
    (engine, b, log)
}

fn send(engine: &Engine, port: &str, bytes: [u8; 3]) {
    assert!(engine.midi_sender().send(MidiIn {
        port: port_key(port),
        host_ns: 0,
        msg: Midi1Msg::three(bytes[0], bytes[1], bytes[2]),
    }));
}

/// The log's note-ons as `(frame, pitch)`.
fn ons(log: &EventLog) -> Vec<(u64, u8)> {
    log.drain()
        .into_iter()
        .filter(|r| r.bytes().len() == 3 && r.bytes()[0] & 0xF0 == 0x90 && r.bytes()[2] > 0)
        .map(|r| (r.frame, r.bytes()[1]))
        .collect()
}

#[test]
fn thru_plays_the_chains_output() {
    let d = doc();
    let fx = vec![
        InputFx::new(InputFxKind::Transpose { semitones: 12 }),
        InputFx::new(InputFxKind::InputQuantize {
            grid: Tick(Q / 4),
            strength: 100,
        }),
    ];
    let (engine, mut b, log) = start(&d, vec![spec(&d, d.keys, "Keys", fx, false)]);
    b.run_frames(512);
    log.drain();
    send(&engine, "Keys", [0x90, 64, 100]);
    b.run_frames(256);
    let got: Vec<(u64, Vec<u8>)> = log
        .drain()
        .into_iter()
        .map(|r| (r.frame, r.bytes().to_vec()))
        .collect();
    assert_eq!(got, vec![(512, vec![0x93, 76, 100])]);
}

#[test]
fn arp_steps_play_at_their_frames_while_stopped() {
    let d = doc();
    let (engine, mut b, log) = start(&d, vec![spec(&d, d.keys, "Keys", vec![arp(|_| {})], false)]);
    b.run_frames(512);
    log.drain();
    send(&engine, "Keys", [0x90, 60, 100]);
    send(&engine, "Keys", [0x90, 64, 100]);
    b.run_frames(12_800);
    assert_eq!(ons(&log), vec![(512, 60), (6512, 64), (12_512, 60)]);
}

#[test]
fn arp_steps_follow_the_beat_while_playing() {
    let d = doc();
    let (mut engine, mut b, log) =
        start(&d, vec![spec(&d, d.keys, "Keys", vec![arp(|_| {})], false)]);
    engine.play().unwrap();
    b.run_frames(256);
    log.drain();
    send(&engine, "Keys", [0x90, 60, 100]);
    send(&engine, "Keys", [0x90, 64, 100]);
    b.run_frames(12_800);
    // The transport rolled at device frame 0: steps at the sixteenths after
    // the press (Ruling 10).
    assert_eq!(ons(&log), vec![(6000, 60), (12_000, 64)]);
}

#[test]
fn the_recorder_hears_what_thru_played() {
    let d = doc();
    let fx = vec![arp(|a| a.mode = ArpMode::UpDown)];
    let (mut engine, mut b, log) = start(&d, vec![spec(&d, d.keys, "Keys", fx, true)]);
    let mut h = engine
        .start_recording(&RecordRequest {
            channels: vec![],
            range: None,
            ring_frames: 4800,
        })
        .unwrap();
    engine.play().unwrap();
    b.run_frames(256);
    log.drain();
    for p in [60, 64, 67] {
        send(&engine, "Keys", [0x90, p, 100]);
    }
    b.run_frames(48_000);
    let thru: Vec<u8> = ons(&log).into_iter().map(|(_, p)| p).collect();
    let mut rec = Vec::new();
    while let Ok(m) = h.midi.pop() {
        assert_eq!(m.quantize, None);
        if let LiveMsg::NoteOn { pitch, .. } = m.msg {
            rec.push(pitch);
        }
    }
    // Sixteenths at frames 6000, 12 000, …, 48 000 of 256..48 256.
    assert_eq!(thru.len(), 8, "{thru:?}");
    assert_eq!(rec, thru);
}

#[test]
fn a_changed_chain_ends_its_notes_and_a_kept_one_plays_on() {
    let d = doc();
    let keys = spec(&d, d.keys, "Keys", vec![arp(|a| a.latch = true)], false);
    let pads = |s: i8| {
        spec(
            &d,
            d.pads,
            "Pads",
            vec![InputFx::new(InputFxKind::Transpose { semitones: s })],
            false,
        )
    };
    let (mut engine, mut b, log) = start(&d, vec![keys.clone(), pads(12)]);
    b.run_frames(512);
    send(&engine, "Keys", [0x90, 60, 100]);
    send(&engine, "Keys", [0x80, 60, 0]);
    send(&engine, "Pads", [0x90, 50, 100]);
    b.run_frames(256);
    log.drain();
    // Pads' chain changes; Keys' (latched) does not.
    engine.set_inputs(InputSpec {
        midi: vec![keys, pads(5)],
        monitor: vec![],
    });
    engine.load(&d.doc, d.seq).unwrap();
    b.run_frames(12_288);
    let got: Vec<(u64, Vec<u8>)> = log
        .drain()
        .into_iter()
        .map(|r| (r.frame, r.bytes().to_vec()))
        .collect();
    assert_eq!(
        got.iter().filter(|(_, m)| m == &vec![0x83, 62, 0]).count(),
        1,
        "Pads' transposed note ended once: {got:?}"
    );
    let steps: Vec<u64> = got
        .iter()
        .filter(|(_, m)| m == &vec![0x93, 60, 100])
        .map(|(f, _)| *f)
        .collect();
    assert_eq!(steps, vec![6512, 12_512], "the latched arpeggio played on");
}

#[test]
fn bypassing_while_held_leaves_no_note_hanging() {
    let d = doc();
    let chord = |bypass| InputFx {
        kind: InputFxKind::Chord {
            intervals: vec![4, 7],
        },
        bypass,
    };
    let (mut engine, mut b, log) = start(
        &d,
        vec![spec(&d, d.keys, "Keys", vec![chord(false)], false)],
    );
    b.run_frames(256);
    send(&engine, "Keys", [0x90, 60, 100]);
    b.run_frames(256);
    log.drain();
    engine.set_inputs(InputSpec {
        midi: vec![spec(&d, d.keys, "Keys", vec![chord(true)], false)],
        monitor: vec![],
    });
    engine.load(&d.doc, d.seq).unwrap();
    b.run_frames(256);
    send(&engine, "Keys", [0x80, 60, 0]);
    b.run_frames(256);
    let offs: Vec<Vec<u8>> = log
        .drain()
        .iter()
        .map(|r| r.bytes().to_vec())
        .filter(|m| m[0] & 0xF0 == 0x80)
        .collect();
    assert_eq!(
        offs,
        vec![vec![0x83, 60, 0], vec![0x83, 64, 0], vec![0x83, 67, 0]]
    );
}

#[test]
fn overflow_reaches_the_counters() {
    let d = doc();
    let (engine, mut b, _log) = start(&d, vec![spec(&d, d.keys, "Keys", vec![arp(|_| {})], false)]);
    for p in 30..63u8 {
        send(&engine, "Keys", [0x90, p, 100]);
    }
    b.run_frames(256);
    assert_eq!(engine.counters().midi_fx_dropped, 1);
}

/// Task 3 review M4: one lane's arpeggiator steps follow another's within
/// a block, so outputs are put in frame order before they leave the
/// renderer (the recorder's arrival order shows it).
#[test]
fn live_outputs_leave_in_frame_order() {
    use core_model::input_fx::{Feel, NoteValue};
    let d = doc();
    let rate = |division, feel| {
        arp(move |a| {
            a.rate = NoteValue { division, feel };
        })
    };
    let midi = vec![
        spec(&d, d.keys, "Keys", vec![rate(64, Feel::Triplet)], true),
        spec(&d, d.pads, "Pads", vec![rate(64, Feel::Straight)], true),
    ];
    let sinks = OfflineSinks::new(100_000);
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.set_inputs(InputSpec {
        midi,
        monitor: vec![],
    });
    engine.load(&d.doc, d.seq).unwrap();
    let mut b = OfflineBackend::new(SR, 4096).with_input(FakeInput::Signals);
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
    b.block(256);
    send(&engine, "Keys", [0x90, 60, 100]);
    send(&engine, "Pads", [0x90, 50, 100]);
    for _ in 0..4 {
        b.block(4096);
    }
    let mut runs = Vec::new();
    let mut lanes = std::collections::BTreeSet::new();
    while let Ok(m) = h.midi.pop() {
        runs.push(m.run);
        lanes.insert(m.lane);
    }
    assert_eq!(lanes.len(), 2, "both lanes recorded");
    assert!(runs.len() > 10, "{runs:?}");
    assert!(
        runs.windows(2).all(|w| w[0] <= w[1]),
        "out of order: {runs:?}"
    );
}

/// What [`RunNode`] saw: each message with its frame, and every message
/// whose offset was not inside the render call that followed it.
#[derive(Default)]
struct RunSeen {
    msgs: Vec<(u64, Vec<u8>)>,
    outside: Vec<(u32, u32)>,
    /// `(earlier offset, later offset)` pairs that arrived in reverse
    /// within one render call.
    reversed: Vec<(u32, u32)>,
}

/// An instrument that checks `InstrumentNode::midi`'s promise: an offset
/// within the next `render` call.
struct RunNode {
    seen: std::sync::Arc<std::sync::Mutex<RunSeen>>,
    queued: Vec<u32>,
    rendered: u64,
}

impl core_engine::backend::InstrumentNode for RunNode {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        if let Ok(mut s) = self.seen.lock() {
            s.msgs
                .push((self.rendered + u64::from(offset), msg.to_vec()));
        }
        if let (Some(&last), Ok(mut s)) = (self.queued.last(), self.seen.lock()) {
            if offset < last {
                s.reversed.push((last, offset));
            }
        }
        if self.queued.len() < self.queued.capacity() {
            self.queued.push(offset);
        }
    }
    fn render(&mut self, frames: u32, out: &mut core_engine::backend::StereoBuf) {
        out.clear(frames);
        if let Ok(mut s) = self.seen.lock() {
            for o in self.queued.iter().filter(|o| **o >= frames) {
                s.outside.push((*o, frames));
            }
        }
        self.queued.clear();
        self.rendered += u64::from(frames);
    }
    fn reset(&mut self) {}
}

struct RunFactory(std::sync::Arc<std::sync::Mutex<RunSeen>>);

impl core_engine::engine::SinkFactory for RunFactory {
    fn ports(&self) -> Vec<core_engine::backend::MidiPortInfo> {
        vec![]
    }
    fn make(
        &mut self,
        _: core_engine::schedule::TargetIdx,
        _: &core_engine::schedule::TargetKind,
        _: f64,
        _: u32,
    ) -> Result<core_engine::render::Sink, core_engine::BackendError> {
        Ok(core_engine::render::Sink::Instrument(Box::new(RunNode {
            seen: self.0.clone(),
            queued: Vec::with_capacity(4096),
            rendered: 0,
        })))
    }
}

/// Review I1: automation cuts blocks into runs, and an arpeggiator step
/// reaches the instrument in the run that holds it, at its frame.
#[test]
fn arp_steps_reach_the_run_that_holds_them() {
    use core_model::automation::AutomationTarget;
    use core_model::event::{EventBody, EventSpec};
    use core_model::mixer::StripKind;
    let mut d = doc();
    let mut b = core_engine::fixtures::Builder::from_doc(d.doc.clone());
    let strip = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let auto = b.lane(d.seq, "Auto", None).unwrap();
    // A fader point every 64 ticks (100 frames at 120 BPM).
    let events = (0..(8 * Q / 64))
        .map(|k| {
            EventSpec::new(
                Tick(k * 64),
                EventBody::Automation {
                    target: AutomationTarget::StripFader(strip),
                    value: (k % 10) as f64 / 10.0,
                },
            )
        })
        .collect();
    b.events(d.seq, auto, events).unwrap();
    d.doc = b.finish();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(RunSeen::default()));
    let (mut engine, renderer) =
        Engine::new(EngineConfig::new(SR), Box::new(RunFactory(seen.clone()))).unwrap();
    engine.set_inputs(InputSpec {
        midi: vec![spec(&d, d.keys, "Keys", vec![arp(|_| {})], false)],
        monitor: vec![],
    });
    engine.load(&d.doc, d.seq).unwrap();
    let mut be = OfflineBackend::new(SR, 512).with_input(FakeInput::Signals);
    be.start(renderer).unwrap();
    engine.play().unwrap();
    be.run_frames(512);
    send(&engine, "Keys", [0x90, 60, 100]);
    let runs = engine.counters().runs;
    be.run_frames(12_800);
    assert!(
        engine.counters().runs - runs > 2 * 12_800 / 512,
        "automation cut the blocks into runs"
    );
    let s = seen.lock().unwrap();
    assert!(
        s.outside.is_empty(),
        "offsets past their render call: {:?}",
        s.outside
    );
    let steps: Vec<u64> = s
        .msgs
        .iter()
        .filter(|(_, m)| m.len() == 3 && m[0] & 0xF0 == 0x90 && m[1] == 60 && m[2] > 0)
        .map(|(f, _)| *f)
        .collect();
    assert_eq!(steps, vec![6000, 12_000]);
}

/// Review M2: a route that goes away releases its pedal even where a
/// carried route keeps the target (both lanes play the synth).
#[test]
fn a_dropped_routes_pedal_is_released_on_a_kept_target() {
    let d = doc();
    let keys = spec(&d, d.keys, "Keys", vec![arp(|a| a.latch = true)], false);
    let pads = spec(&d, d.pads, "Pads", vec![], false);
    let (mut engine, mut b, log) = start(&d, vec![keys.clone(), pads]);
    b.run_frames(512);
    send(&engine, "Pads", [0xB0, 64, 127]);
    b.run_frames(256);
    log.drain();
    engine.set_inputs(InputSpec {
        midi: vec![keys],
        monitor: vec![],
    });
    engine.load(&d.doc, d.seq).unwrap();
    b.run_frames(256);
    let got: Vec<Vec<u8>> = log.drain().iter().map(|r| r.bytes().to_vec()).collect();
    assert!(got.contains(&vec![0xB3, 64, 0]), "{got:?}");
}

/// Review M4: an armed lane's recorded messages carry its Input Quantize
/// (Ruling 12).
#[test]
fn recorded_input_carries_the_chains_quantize() {
    use core_engine::input_fx::RecordQuantize;
    let d = doc();
    let fx = vec![InputFx::new(InputFxKind::InputQuantize {
        grid: Tick(Q / 4),
        strength: 100,
    })];
    let (mut engine, mut b, _log) = start(&d, vec![spec(&d, d.keys, "Keys", fx, true)]);
    let mut h = engine
        .start_recording(&RecordRequest {
            channels: vec![],
            range: None,
            ring_frames: 4800,
        })
        .unwrap();
    engine.play().unwrap();
    b.run_frames(256);
    send(&engine, "Keys", [0x90, 60, 100]);
    b.run_frames(256);
    let m = h.midi.pop().unwrap();
    assert_eq!(
        m.quantize,
        Some(RecordQuantize {
            grid: Q / 4,
            strength: 100
        })
    );
    assert!(matches!(m.msg, LiveMsg::NoteOn { pitch: 60, .. }));
}

/// Fix round 2: at a loop wrap mid-block, live thru before the wrap
/// reaches the instrument before the wrap's releases (frame order), and no
/// note is left sounding.
#[test]
fn live_thru_before_a_wrap_goes_before_its_releases() {
    use core_model::input_fx::{Feel, NoteValue};
    let mut bld = Builder::new();
    let synth = bld.dest("Synth", "builtin:synth", 3).unwrap();
    let (seq, keys) = bld.sequence("Bar", "Keys", Some(synth)).unwrap();
    let pads = bld.lane(seq, "Pads", Some(synth)).unwrap();
    // Sounding when the loop wraps at frame 3800: the wrap releases it.
    bld.notes(seq, pads, &[(0, 30, 0x8000, 4 * Q)]).unwrap();
    let d = Doc {
        doc: bld.finish(),
        seq,
        keys,
        pads,
        synth,
    };
    let seen = std::sync::Arc::new(std::sync::Mutex::new(RunSeen::default()));
    let (mut engine, renderer) =
        Engine::new(EngineConfig::new(SR), Box::new(RunFactory(seen.clone()))).unwrap();
    // 1/64 triplets (1000 frames), gate 50 %: a note-off at 3500, in the
    // block 3072..4096 before its wrap at 3800.
    let fx = vec![arp(|a| {
        a.rate = NoteValue {
            division: 64,
            feel: Feel::Triplet,
        };
        a.latch = true;
    })];
    engine.set_inputs(InputSpec {
        midi: vec![spec(&d, d.keys, "Keys", fx, false)],
        monitor: vec![],
    });
    engine.load(&d.doc, d.seq).unwrap();
    engine.set_loop_range(Some((0, 3800))).unwrap();
    let mut be = OfflineBackend::new(SR, 1024).with_input(FakeInput::Signals);
    be.start(renderer).unwrap();
    be.run_frames(1024);
    send(&engine, "Keys", [0x90, 60, 100]);
    engine.play().unwrap();
    // The transport rolls at device frame 1024: the fourth block holds
    // the wrap.
    be.run_frames(4 * 1024);
    engine.stop().unwrap();
    engine.set_inputs(InputSpec {
        midi: vec![],
        monitor: vec![],
    });
    engine.load(&d.doc, d.seq).unwrap();
    be.run_frames(2048);
    let s = seen.lock().unwrap();
    assert!(
        s.msgs.iter().any(|(_, m)| m == &vec![0x83, 30, 0]),
        "the wrap released the playback note: {:?}",
        s.msgs
    );
    assert!(
        s.reversed.is_empty(),
        "arrived out of frame order: {:?}",
        s.reversed
    );
    let mut sounding = std::collections::BTreeSet::new();
    for (_, m) in &s.msgs {
        match (m.first().map(|b| b & 0xF0), m.get(1), m.get(2)) {
            (Some(0x90), Some(&p), Some(&v)) if v > 0 => {
                sounding.insert(p);
            }
            (Some(0x80 | 0x90), Some(&p), _) => {
                sounding.remove(&p);
            }
            _ => {}
        }
    }
    assert!(sounding.is_empty(), "left sounding: {sounding:?}");
}
