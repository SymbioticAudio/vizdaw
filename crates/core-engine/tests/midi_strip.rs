//! MIDI strips (plan 5g-1b Task 2; spec §6.8a): an external destination's
//! mix is sent as CC 7 and CC 10 at frame 0 after its default patch, mute
//! drops its notes, `send_now` reaches its target on its channel, and the
//! note-ons sent to it are reported as activity.

use core_engine::BackendError;
use core_engine::backend::{BlockTime, MidiOut, MidiPortInfo};
use core_engine::engine::SinkFactory;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{BAR, Builder, Q, cc32};
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, port_key};
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{EventLog, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::render::Renderer;
use core_engine::render::Sink;
use core_engine::schedule::TargetIdx;
use core_engine::schedule::{CompileOptions, Schedule, TargetKind, TargetTable, WireMsg, compile};
use core_model::command::{Command, RoutingCmd};
use core_model::document::{DestKind, Document, MidiMix, Patch};
use core_model::event::EventBody;
use core_model::ids::{DestId, SeqId};
use core_model::time::Tick;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn compile_all(doc: &Document, seq: SeqId) -> Schedule {
    let mut table = TargetTable::new();
    compile(
        doc,
        seq,
        &CompileOptions::new(48_000.0),
        &mut |k: &TargetKind| table.intern(k.clone()).map(Ok),
    )
    .unwrap()
    .0
}

/// `(frame, bytes)` of every MIDI 1.0 entry.
fn entries(s: &Schedule) -> Vec<(u64, Vec<u8>)> {
    s.entries
        .iter()
        .filter(|e| matches!(e.msg, WireMsg::Midi1(_)))
        .map(|e| (e.frame, s.bytes(&e.msg).to_vec()))
        .collect()
}

fn is_note(b: &[u8]) -> bool {
    matches!(b.first().map(|s| s & 0xF0), Some(0x80 | 0x90))
}

fn ext(b: &mut Builder, name: &str, channel: u8, patch: Option<Patch>) -> DestId {
    b.dest_kind(
        name,
        DestKind::MidiEndpoint {
            endpoint_ref: "virtual:out".into(),
            group: 0,
            channel,
        },
        patch,
    )
    .unwrap()
}

fn set_mix(b: &mut Builder, dest: DestId, mix: MidiMix) {
    b.run(Command::Routing(RoutingCmd::SetMidiMix { dest, mix }))
        .unwrap();
}

/// The non-note entries at frame 0 of a one-note sequence on an external
/// destination on channel 3 with `patch` and `mix`.
fn frame0(patch: Option<Patch>, mix: MidiMix) -> Vec<Vec<u8>> {
    let mut b = Builder::new();
    let d = ext(&mut b, "Ext", 2, patch);
    set_mix(&mut b, d, mix);
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let s = compile_all(&b.finish(), seq);
    entries(&s)
        .into_iter()
        .filter(|(f, b)| *f == 0 && !is_note(b))
        .map(|(_, b)| b)
        .collect()
}

#[test]
fn mix_follows_the_patch_at_frame_0() {
    let patch = Some(Patch {
        program: 5,
        bank: Some((1, 2)),
    });
    let both = MidiMix {
        volume: Some(90),
        pan: Some(20),
        mute: false,
    };
    assert_eq!(
        frame0(patch, both),
        vec![
            vec![0xB2, 0x00, 0x01],
            vec![0xB2, 0x20, 0x02],
            vec![0xC2, 0x05],
            vec![0xB2, 0x07, 0x5A],
            vec![0xB2, 0x0A, 0x14],
        ]
    );
    let volume = MidiMix {
        volume: Some(90),
        ..MidiMix::default()
    };
    assert_eq!(
        frame0(patch, volume),
        vec![
            vec![0xB2, 0x00, 0x01],
            vec![0xB2, 0x20, 0x02],
            vec![0xC2, 0x05],
            vec![0xB2, 0x07, 0x5A],
        ]
    );
    // No default patch: the mix still goes out.
    assert_eq!(
        frame0(None, both),
        vec![vec![0xB2, 0x07, 0x5A], vec![0xB2, 0x0A, 0x14]]
    );
}

#[test]
fn an_untouched_destination_sends_no_cc() {
    for patch in [
        None,
        Some(Patch {
            program: 5,
            bank: Some((1, 2)),
        }),
    ] {
        let mut b = Builder::new();
        let d = ext(&mut b, "Ext", 2, patch);
        let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
        b.notes(seq, lane, &[(0, 60, 0x8000, Q), (BAR, 62, 0x8000, Q)])
            .unwrap();
        let s = compile_all(&b.finish(), seq);
        let ccs: Vec<_> = entries(&s)
            .into_iter()
            .filter(|(_, b)| b[0] & 0xF0 == 0xB0 && matches!(b[1], 7 | 10))
            .collect();
        assert!(ccs.is_empty(), "{ccs:?}");
    }
}

fn blocks(r: &mut Renderer, n: usize, frames: u32, ns: &mut u64) {
    let mut l = vec![0.0f32; frames as usize];
    let mut rr = vec![0.0f32; frames as usize];
    for _ in 0..n {
        let mut chans: [&mut [f32]; 2] = [&mut l, &mut rr];
        r.process(
            &mut chans,
            frames,
            BlockTime {
                host_time_ns: *ns,
                sample_rate: 48_000.0,
            },
        );
        *ns += u64::from(frames) * 1_000_000_000 / 48_000;
    }
}

fn engine(doc: &Document, seq: SeqId) -> (Engine, Renderer, EventLog) {
    let sinks = OfflineSinks::new(1 << 16);
    let log = sinks.log();
    let (mut e, r) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    e.load(doc, seq).unwrap();
    (e, r, log)
}

fn sent(log: &EventLog) -> Vec<Vec<u8>> {
    log.drain().iter().map(|r| r.bytes().to_vec()).collect()
}

fn volumes(seen: &[Vec<u8>]) -> Vec<u8> {
    seen.iter()
        .filter(|b| b.len() == 3 && b[0] == 0xB0 && b[1] == 7)
        .map(|b| b[2])
        .collect()
}

#[test]
fn a_recorded_cc7_wins_over_the_mix_mid_song() {
    let mut b = Builder::new();
    let d = ext(&mut b, "Ext", 0, None);
    set_mix(
        &mut b,
        d,
        MidiMix {
            volume: Some(90),
            ..MidiMix::default()
        },
    );
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q), (7 * BAR, 62, 0x8000, Q)])
        .unwrap();
    b.event(
        seq,
        lane,
        2 * BAR,
        EventBody::Cc {
            index: 7,
            value: cc32(40),
        },
    )
    .unwrap();
    let doc = b.finish();

    // From bar 5: chase sends the recorded 40, not the strip's 90.
    let (mut e, mut r, log) = engine(&doc, seq);
    let mut ns = 0;
    e.locate_tick(Tick(4 * BAR)).unwrap();
    e.play().unwrap();
    blocks(&mut r, 4, 1024, &mut ns);
    assert_eq!(volumes(&sent(&log)), vec![40]);

    // From bar 1: 90 at frame 0, then 40 at bar 3 (2 bars = 4 s).
    let (mut e, mut r, log) = engine(&doc, seq);
    let mut ns = 0;
    e.play().unwrap();
    blocks(&mut r, 200, 1024, &mut ns);
    let recs = log.drain();
    let cc7: Vec<(u64, u8)> = recs
        .iter()
        .filter(|r| r.bytes().len() == 3 && r.bytes()[0] == 0xB0 && r.bytes()[1] == 7)
        .map(|r| (r.frame, r.bytes()[2]))
        .collect();
    assert_eq!(cc7.iter().map(|c| c.1).collect::<Vec<_>>(), vec![90, 40]);
    assert!(cc7[0].0 < 1024, "{cc7:?}");
    assert!(cc7[1].0 >= 192_000, "{cc7:?}");
}

/// Ext A on channel 1 (long note, CC 1, bend, a note at bar 2 and one at
/// bar 4) and Ext B on channel 2 (a note), both on the virtual source.
fn mute_doc(mute: bool) -> (Document, SeqId, DestId, DestId) {
    let mut b = Builder::new();
    let a = ext(&mut b, "A", 0, None);
    let other = ext(&mut b, "B", 1, None);
    let (seq, la) = b.sequence("S", "A", Some(a)).unwrap();
    let lb = b.lane(seq, "B", Some(other)).unwrap();
    b.notes(
        seq,
        la,
        &[
            (0, 60, 0x8000, 4 * BAR),
            (BAR, 64, 0x8000, Q),
            (3 * BAR, 65, 0x8000, Q),
        ],
    )
    .unwrap();
    b.event(
        seq,
        la,
        Q,
        EventBody::Cc {
            index: 1,
            value: cc32(50),
        },
    )
    .unwrap();
    b.event(seq, la, 2 * Q, EventBody::PitchBend { value: 0x9000_0000 })
        .unwrap();
    b.notes(seq, lb, &[(0, 72, 0x8000, Q), (BAR, 74, 0x8000, Q)])
        .unwrap();
    if mute {
        set_mix(
            &mut b,
            a,
            MidiMix {
                mute: true,
                ..MidiMix::default()
            },
        );
    }
    (b.finish(), seq, a, other)
}

#[test]
fn muting_ends_sounding_notes_and_keeps_controllers() {
    let (open, seq, _, _) = mute_doc(false);
    let (muted, _, _, _) = mute_doc(true);

    // While the long note sounds, the muted schedule's swap ends it.
    let (mut e, mut r, log) = engine(&open, seq);
    let mut ns = 0;
    e.play().unwrap();
    blocks(&mut r, 4, 1024, &mut ns);
    let before = sent(&log);
    assert!(before.contains(&vec![0x90, 60, 64]), "{before:?}");
    assert!(!before.iter().any(|b| b[..2] == [0x80, 60]), "{before:?}");
    e.load(&muted, seq).unwrap();
    blocks(&mut r, 1, 1024, &mut ns);
    let after = sent(&log);
    assert!(after.iter().any(|b| b[..2] == [0x80, 60]), "{after:?}");
    // Past bar 2: A's later note does not play, B's does.
    blocks(&mut r, 100, 1024, &mut ns);
    let later = sent(&log);
    assert!(!later.iter().any(|b| b[..2] == [0x90, 64]), "{later:?}");
    assert!(later.iter().any(|b| b[..2] == [0x91, 74]), "{later:?}");

    // Unmuting (an Undo of the mute is the same load): the next note, at
    // bar 4, plays (final review I3).
    e.load(&open, seq).unwrap();
    blocks(&mut r, 200, 1024, &mut ns);
    let unmuted = sent(&log);
    assert!(unmuted.contains(&vec![0x90, 65, 64]), "{unmuted:?}");

    // The muted schedule has no notes on channel 1 but keeps its
    // controllers; channel 2 is untouched.
    let s = compile_all(&muted, seq);
    let all = entries(&s);
    let ch = |c: u8| -> Vec<Vec<u8>> {
        all.iter()
            .filter(|(_, b)| b[0] & 0x0F == c)
            .map(|(_, b)| b.clone())
            .collect()
    };
    let a = ch(0);
    assert!(!a.iter().any(|b| is_note(b)), "{a:?}");
    assert!(a.contains(&vec![0xB0, 1, 50]), "{a:?}");
    assert!(a.iter().any(|b| b[0] == 0xE0), "{a:?}");
    let b = ch(1);
    let open_b: Vec<Vec<u8>> = entries(&compile_all(&open, seq))
        .into_iter()
        .filter(|(_, b)| b[0] & 0x0F == 1)
        .map(|(_, b)| b)
        .collect();
    assert_eq!(b, open_b);
    assert_eq!(b.iter().filter(|b| is_note(b)).count(), 4, "{b:?}");
}

#[test]
fn send_now_uses_the_destination_channel() {
    let mut b = Builder::new();
    let d = ext(&mut b, "Ext", 2, None);
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let doc = b.finish();
    let (mut e, mut r, log) = engine(&doc, seq);
    let mut ns = 0;
    blocks(&mut r, 1, 256, &mut ns);
    log.drain();
    assert!(
        e.send_now(&doc, d, Midi1Msg::three(0xB0, 0x07, 0x64))
            .unwrap()
    );
    blocks(&mut r, 1, 256, &mut ns);
    assert_eq!(sent(&log), vec![vec![0xB2, 0x07, 0x64]]);
    assert!(
        !e.send_now(&doc, DestId(9_999), Midi1Msg::three(0xB0, 7, 1))
            .unwrap()
    );
    // Hostile: a system message is refused; data bytes are kept to 7 bits.
    assert!(!e.send_now(&doc, d, Midi1Msg::two(0xF8, 0)).unwrap());
    assert!(!e.send_now(&doc, d, Midi1Msg::three(0x40, 7, 1)).unwrap());
    assert!(
        e.send_now(&doc, d, Midi1Msg::three(0xB5, 0x87, 0xFF))
            .unwrap()
    );
    blocks(&mut r, 1, 256, &mut ns);
    assert_eq!(sent(&log), vec![vec![0xB2, 0x07, 0x7F]]);
}

#[test]
fn activity_reports_the_peak_velocity_once() {
    let mut b = Builder::new();
    let d = ext(&mut b, "Ext", 0, None);
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    let other = b.lane(seq, "Synth", Some(synth)).unwrap();
    b.notes(seq, lane, &[(0, 60, 30 << 9, Q), (Q, 62, 100 << 9, Q)])
        .unwrap();
    b.notes(seq, other, &[(0, 60, 120 << 9, Q)]).unwrap();
    let doc = b.finish();
    let (mut e, mut r, _log) = engine(&doc, seq);
    let mut ns = 0;
    assert!(e.take_midi_activity(&doc).is_empty());
    e.play().unwrap();
    blocks(&mut r, 40, 1024, &mut ns);
    assert_eq!(e.take_midi_activity(&doc), vec![(d, 100)]);
    assert_eq!(e.take_midi_activity(&doc), vec![]);
}

#[test]
fn a_mid_song_start_chases_the_mix() {
    let mut b = Builder::new();
    let d = ext(&mut b, "Ext", 0, None);
    set_mix(
        &mut b,
        d,
        MidiMix {
            volume: Some(90),
            pan: Some(20),
            mute: false,
        },
    );
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q), (7 * BAR, 62, 0x8000, Q)])
        .unwrap();
    let doc = b.finish();
    let (mut e, mut r, log) = engine(&doc, seq);
    let mut ns = 0;
    e.locate_tick(Tick(4 * BAR)).unwrap();
    e.play().unwrap();
    blocks(&mut r, 4, 1024, &mut ns);
    let seen = sent(&log);
    assert_eq!(volumes(&seen), vec![90]);
    let pans: Vec<u8> = seen
        .iter()
        .filter(|b| b.len() == 3 && b[0] == 0xB0 && b[1] == 10)
        .map(|b| b[2])
        .collect();
    assert_eq!(pans, vec![20]);
}

#[test]
fn send_now_sends_the_length_the_status_has() {
    let mut b = Builder::new();
    let d = ext(&mut b, "Ext", 2, None);
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let doc = b.finish();
    let (mut e, mut r, log) = engine(&doc, seq);
    let mut ns = 0;
    blocks(&mut r, 1, 256, &mut ns);
    log.drain();
    let long_program = Midi1Msg {
        len: 3,
        bytes: [0xC0, 5, 0],
    };
    let short_cc = Midi1Msg {
        len: 1,
        bytes: [0xB0, 7, 9],
    };
    assert!(e.send_now(&doc, d, long_program).unwrap());
    assert!(e.send_now(&doc, d, short_cc).unwrap());
    blocks(&mut r, 1, 256, &mut ns);
    assert_eq!(sent(&log), vec![vec![0xC2, 5], vec![0xB2, 7, 9]]);
}

#[test]
fn activity_is_per_channel_on_a_shared_port() {
    let mut b = Builder::new();
    let one = ext(&mut b, "One", 0, None);
    let two = ext(&mut b, "Two", 1, None);
    let (seq, lane) = b.sequence("S", "One", Some(one)).unwrap();
    let other = b.lane(seq, "Two", Some(two)).unwrap();
    b.notes(seq, lane, &[(0, 60, 80 << 9, Q)]).unwrap();
    // Two's lane has only a controller: its target is loaded, no note.
    b.event(
        seq,
        other,
        0,
        EventBody::Cc {
            index: 1,
            value: cc32(3),
        },
    )
    .unwrap();
    let doc = b.finish();
    let (mut e, mut r, _log) = engine(&doc, seq);
    let mut ns = 0;
    e.play().unwrap();
    blocks(&mut r, 4, 1024, &mut ns);
    assert_eq!(e.take_midi_activity(&doc), vec![(one, 80)]);
}

#[test]
fn live_thru_to_a_muted_destination_drops_notes_and_still_records() {
    let mut b = Builder::new();
    let d = ext(&mut b, "Ext", 2, None);
    let (seq, lane) = b.sequence("S", "Keys", Some(d)).unwrap();
    b.notes(seq, lane, &[(4 * BAR, 40, 0x8000, Q)]).unwrap();
    set_mix(
        &mut b,
        d,
        MidiMix {
            mute: true,
            ..MidiMix::default()
        },
    );
    let doc = b.finish();
    let sinks = OfflineSinks::new(1 << 16);
    let log = sinks.log();
    let (mut e, mut r) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    e.set_inputs(InputSpec {
        midi: vec![MidiInputSpec {
            lane,
            port: Some(port_key("Keys")),
            channel: None,
            dest: Some(d),
            thru: true,
            armed: true,
            fx: Vec::new(),
        }],
        monitor: vec![],
    });
    e.load(&doc, seq).unwrap();
    let mut h = e
        .start_recording(&RecordRequest {
            channels: vec![],
            range: None,
            ring_frames: 4800,
        })
        .unwrap();
    e.play().unwrap();
    let mut ns = 0;
    blocks(&mut r, 2, 256, &mut ns);
    log.drain();
    for msg in [
        Midi1Msg::three(0x90, 60, 100),
        Midi1Msg::three(0xB0, 1, 7),
        Midi1Msg::three(0x80, 60, 0),
    ] {
        assert!(e.midi_sender().send(MidiIn {
            port: port_key("Keys"),
            host_ns: 0,
            msg,
        }));
    }
    blocks(&mut r, 2, 256, &mut ns);
    assert_eq!(sent(&log), vec![vec![0xB2, 1, 7]], "only the controller");
    let mut recorded = 0;
    while h.midi.pop().is_ok() {
        recorded += 1;
    }
    assert_eq!(recorded, 3, "the notes are still recorded");
}

type PortSeen = Arc<Mutex<Vec<Vec<u8>>>>;

/// MIDI sinks on ports listed from `ports`, counting the listings.
#[derive(Clone, Default)]
struct Ports {
    ports: Arc<Mutex<Vec<MidiPortInfo>>>,
    listed: Arc<AtomicUsize>,
    seen: PortSeen,
}

struct PortOut(PortSeen);

impl MidiOut for PortOut {
    fn send(&mut self, _host_time_ns: u64, msg: &[u8]) -> bool {
        self.0.lock().unwrap().push(msg.to_vec());
        true
    }
}

impl SinkFactory for Ports {
    fn ports(&self) -> Vec<MidiPortInfo> {
        self.listed.fetch_add(1, Ordering::Relaxed);
        self.ports.lock().unwrap().clone()
    }
    fn make(
        &mut self,
        _target: TargetIdx,
        _kind: &TargetKind,
        _sr: f64,
        _max: u32,
    ) -> Result<Sink, BackendError> {
        Ok(Sink::Midi(Box::new(PortOut(self.seen.clone()))))
    }
}

#[test]
fn send_now_and_activity_use_the_loaded_targets_not_the_port_list() {
    let mut b = Builder::new();
    let d = ext_on(&mut b, "Synth Port", 2);
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 90 << 9, Q)]).unwrap();
    let doc = b.finish();
    let f = Ports::default();
    f.ports.lock().unwrap().push(MidiPortInfo {
        name: "Synth Port".into(),
        unique_id: None,
    });
    let (mut e, mut r) = Engine::new(EngineConfig::new(48_000.0), Box::new(f.clone())).unwrap();
    e.load(&doc, seq).unwrap();
    // The port goes away after the load: its sink is still loaded.
    f.ports.lock().unwrap().clear();
    let listed = f.listed.load(Ordering::Relaxed);
    let mut ns = 0;
    e.play().unwrap();
    blocks(&mut r, 2, 1024, &mut ns);
    assert!(e.send_now(&doc, d, Midi1Msg::three(0xB0, 7, 33)).unwrap());
    assert!(e.audition(&doc, d, 70, 0x8000, true).unwrap());
    blocks(&mut r, 1, 1024, &mut ns);
    assert_eq!(e.take_midi_activity(&doc), vec![(d, 90)]);
    assert_eq!(f.listed.load(Ordering::Relaxed), listed, "no port listing");
    let seen = f.seen.lock().unwrap().clone();
    assert!(seen.contains(&vec![0xB2, 7, 33]), "{seen:?}");
    assert!(seen.contains(&vec![0x92, 70, 64]), "{seen:?}");
}

fn ext_on(b: &mut Builder, port: &str, channel: u8) -> DestId {
    b.dest_kind(
        "Ext",
        DestKind::MidiEndpoint {
            endpoint_ref: port.into(),
            group: 0,
            channel,
        },
        None,
    )
    .unwrap()
}

#[test]
fn muting_ends_a_held_thru_note() {
    let (open, seq, a, _) = mute_doc(false);
    let (muted, _, _, _) = mute_doc(true);
    let lane = open.sequence(seq).unwrap().lanes[0].id;
    let sinks = OfflineSinks::new(1 << 16);
    let log = sinks.log();
    let (mut e, mut r) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    e.set_inputs(InputSpec {
        midi: vec![MidiInputSpec {
            lane,
            port: Some(port_key("Keys")),
            channel: None,
            dest: Some(a),
            thru: true,
            armed: false,
            fx: Vec::new(),
        }],
        monitor: vec![],
    });
    e.load(&open, seq).unwrap();
    let mut ns = 0;
    assert!(e.midi_sender().send(MidiIn {
        port: port_key("Keys"),
        host_ns: 0,
        msg: Midi1Msg::three(0x90, 50, 100),
    }));
    blocks(&mut r, 1, 256, &mut ns);
    assert_eq!(sent(&log), vec![vec![0x90, 50, 100]]);
    e.load(&muted, seq).unwrap();
    blocks(&mut r, 1, 256, &mut ns);
    assert!(sent(&log).contains(&vec![0x80, 50, 0]));
    // While muted a played note is not heard; once unmuted it is (final
    // review I3).
    let key = |msg| {
        assert!(e.midi_sender().send(MidiIn {
            port: port_key("Keys"),
            host_ns: ns,
            msg,
        }));
    };
    key(Midi1Msg::three(0x90, 51, 90));
    key(Midi1Msg::three(0x80, 51, 0));
    blocks(&mut r, 1, 256, &mut ns);
    assert!(!sent(&log).iter().any(|b| b[0] == 0x90), "muted");
    e.load(&open, seq).unwrap();
    blocks(&mut r, 1, 256, &mut ns);
    log.drain();
    assert!(e.midi_sender().send(MidiIn {
        port: port_key("Keys"),
        host_ns: ns,
        msg: Midi1Msg::three(0x90, 52, 100),
    }));
    blocks(&mut r, 1, 256, &mut ns);
    assert_eq!(sent(&log), vec![vec![0x90, 52, 100]]);
}
