//! Engine fixes carried over from the 3a review: the end of playback waits
//! for the compensated tail, the transport position survives a rebuild,
//! meters follow strips and plugin faults follow instances across
//! structural edits.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::ids::{SeqId, StripId};
use core_model::mixer::{AuxSend, StripKind};

const SR: f64 = 48_000.0;

/// One quarter note on the built-in synth through an insert of `latency`.
fn one_note(latency: u32) -> (Document, SeqId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Bar", "Notes", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.insert(synth, TestInserts::plugin(&format!("latency={latency}")))
        .unwrap();
    (b.finish(), seq)
}

/// Frames played until `take_ended`, in blocks of 480.
fn frames_to_end(latency: u32) -> u64 {
    let (doc, seq) = one_note(latency);
    let (mut engine, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(1000))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(SR, 480);
    b.start(r).unwrap();
    engine.play().unwrap();
    for k in 1..1000u64 {
        b.run_frames(480);
        if engine.take_ended() {
            return k * 480;
        }
    }
    panic!("never ended");
}

#[test]
fn ended_waits_for_the_compensated_tail() {
    let plain = frames_to_end(0);
    assert_eq!(frames_to_end(4800), plain + 4800);
}

#[test]
fn the_transport_position_is_not_reduced_by_latency() {
    let (doc, seq) = one_note(480);
    let (mut engine, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(1000))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(SR, 480);
    b.start(r).unwrap();
    engine.play().unwrap();
    b.run_frames(9600);
    assert_eq!(engine.position_frames(), 9600 - 480, "as heard");
    assert_eq!(engine.transport_frames(), 9600, "where to resume");
    assert!(engine.transport_tick() > engine.position_tick());
}

/// Synth → master, with a silent bus A (2) and a bus B (3) fed by a send.
fn buses() -> (Builder, SeqId, StripId, StripId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0xFFFF, 8 * Q)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let a = b.bus("A").unwrap();
    let bus_b = b.bus("B").unwrap();
    b.mixer(MixerCmd::AddSend {
        strip: synth,
        send: AuxSend::new(bus_b),
    })
    .unwrap();
    (b, seq, a, bus_b)
}

#[test]
fn meters_follow_strips_across_structural_edits() {
    let (mut b, seq, a, bus_b) = buses();
    let (mut engine, r) = Engine::new(
        EngineConfig::new(SR),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    engine.load(b.doc(), seq).unwrap();
    let mut be = OfflineBackend::new(SR, 512);
    be.start(r).unwrap();
    engine.play().unwrap();
    be.run_frames(4096);
    // Removing A moves B to A's node index; B's peaks must stay B's.
    b.mixer(MixerCmd::RemoveBus { strip: a }).unwrap();
    engine.load(b.doc(), seq).unwrap();
    let m = engine.meters();
    let peak_b = m.iter().find(|x| x.strip == bus_b).unwrap().peak_l;
    assert!(peak_b > 0.0, "{m:?}");
    assert!(m.iter().all(|x| x.strip != a));
}

#[test]
fn plugin_faults_survive_structural_edits() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, _) = b.sequence("S", "L", Some(d)).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let first = b.insert(synth, TestInserts::plugin("nan")).unwrap();
    let (mut engine, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(b.doc(), seq).unwrap();
    let mut be = OfflineBackend::new(SR, 256);
    be.start(r).unwrap();
    be.run_frames(20 * 256);
    // Not read yet: replace it with another faulty instance at the same
    // insert position, which faults too.
    b.mixer(MixerCmd::RemoveInsert { slot: first }).unwrap();
    let second = b.insert(synth, TestInserts::plugin("nan")).unwrap();
    engine.load(b.doc(), seq).unwrap();
    be.run_frames(20 * 256);
    assert_eq!(engine.plugin_faults(), vec![first, second]);
}
