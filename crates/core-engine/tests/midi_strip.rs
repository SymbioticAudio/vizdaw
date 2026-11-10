//! MIDI strips (plan 5g-1b Task 2; spec §6.8a): an external destination's
//! mix is sent as CC 7 and CC 10 at frame 0 after its default patch, mute
//! drops its notes, `send_now` reaches its target on its channel, and the
//! note-ons sent to it are reported as activity.

use core_engine::backend::BlockTime;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{BAR, Builder, Q, cc32};
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{EventLog, OfflineSinks};
use core_engine::render::Renderer;
use core_engine::schedule::{CompileOptions, Schedule, TargetKind, TargetTable, WireMsg, compile};
use core_model::command::{Command, RoutingCmd};
use core_model::document::{DestKind, Document, MidiMix, Patch};
use core_model::event::EventBody;
use core_model::ids::{DestId, SeqId};
use core_model::time::Tick;

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

/// Ext A on channel 1 (long note, CC 1, bend, a later note) and Ext B on
/// channel 2 (a note), both on the virtual source.
fn mute_doc(mute: bool) -> (Document, SeqId, DestId, DestId) {
    let mut b = Builder::new();
    let a = ext(&mut b, "A", 0, None);
    let other = ext(&mut b, "B", 1, None);
    let (seq, la) = b.sequence("S", "A", Some(a)).unwrap();
    let lb = b.lane(seq, "B", Some(other)).unwrap();
    b.notes(seq, la, &[(0, 60, 0x8000, 4 * BAR), (BAR, 64, 0x8000, Q)])
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
