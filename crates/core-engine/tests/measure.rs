//! Measuring a synth's return latency (plan 5c-2 Task 5; sub-project 5
//! spec §6.7): with the transport stopped, five notes 500 ms apart reach
//! the synth's destination and each one's onset is found on the return
//! input the synth's delay later; a silent input, or one that never falls
//! quiet (a ringing release, a hum), finds none; only a destination the
//! load interned can be measured.

use core_engine::backend::{AudioBackend, MidiPortInfo};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::Builder;
use core_engine::measure::{MeasureStatus, PINGS};
use core_engine::offline::{FAKE_SYNTH_PORT, FakeInput, FakeSynth, OfflineBackend, OfflineSinks};
use core_engine::schedule::TargetKind;
use core_model::ids::DestId;
use std::sync::Arc;

/// Measures the destination named `port` (its lane's return on `inputs`)
/// against a fake synth 25 ms late; returns the status and the synth's
/// delay in frames.
fn measure(port: &str, inputs: Vec<u16>) -> (Arc<MeasureStatus>, i64) {
    let mut b = Builder::new();
    let dest = b.dest(port, port, 0).unwrap();
    let (seq, _) = b.sequence("S", "Part", Some(dest)).unwrap();
    let doc = b.finish();
    let synth = FakeSynth::new(25.0);
    // "Other" is a MIDI destination nothing answers.
    let sinks = OfflineSinks::new(1 << 16)
        .with_ports(vec![MidiPortInfo {
            name: "Other".into(),
            unique_id: None,
        }])
        .with_fake_synth(&synth);
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.set_return_dests(vec![dest]);
    engine.load(&doc, seq).unwrap();
    let mut backend = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Signals)
        .with_synth(&synth);
    let delay = backend.synth_delay().unwrap() as i64;
    backend.start(renderer).unwrap();
    let status = engine.start_measure(&doc, dest, inputs).unwrap().unwrap();
    // The lead, five notes 500 ms apart and the last window: under 3 s.
    for _ in 0..(3 * 48_000 / 512) {
        if status.done() {
            break;
        }
        backend.block(512);
    }
    engine.stop_measure().unwrap();
    backend.block(512);
    assert!(status.closed(), "removed, it reads closed");
    drop(backend.stop().unwrap());
    (status, delay)
}

#[test]
fn every_note_onset_is_found_the_synth_delay_after_it_was_heard() {
    let (s, delay) = measure(FAKE_SYNTH_PORT, vec![2]);
    assert!(s.done());
    assert_eq!(s.delay(), Some(0), "the signals input has no round trip");
    assert_eq!(s.window(), 24_000);
    let sent: Vec<i64> = (0..PINGS).map(|i| s.sent(i).unwrap()).collect();
    // The first after the 100 ms lead, then 500 ms apart (each at the
    // first block after it falls due).
    assert!((4800..4800 + 512).contains(&sent[0]), "{sent:?}");
    for w in sent.windows(2) {
        assert!((w[1] - w[0] - 24_000).abs() < 512, "{sent:?}");
    }
    for (i, u) in sent.iter().enumerate() {
        assert_eq!(s.onset(i), Some(u + delay), "note {i}");
    }
}

#[test]
fn a_silent_return_finds_no_onset() {
    let (s, _) = measure("Other", vec![2]);
    assert!(s.done());
    assert!((0..PINGS).all(|i| s.sent(i).is_some() && s.onset(i).is_none()));
}

/// Plan 5c-2 Ruling 8: an onset follows 5 ms of quiet. Fake input 1 (the
/// sine) never falls quiet that long, as a ringing release or a hum would
/// not: no onset is taken from it.
#[test]
fn an_input_that_never_falls_quiet_finds_no_onset() {
    let (s, _) = measure(FAKE_SYNTH_PORT, vec![0]);
    assert!(s.done());
    assert!((0..PINGS).all(|i| s.onset(i).is_none()));
    // A pair: either channel's onset counts.
    let (s, delay) = measure(FAKE_SYNTH_PORT, vec![1, 2]);
    assert!((0..PINGS).all(|i| s.onset(i).is_some()));
    let _ = delay;
}

/// Plan 5c-2 Ruling 10: the return destinations are interned at load, so
/// a synth the sequence never plays can be measured; another cannot.
#[test]
fn only_a_destination_the_load_interned_can_be_measured() {
    let mut b = Builder::new();
    let dest = b.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (seq, _) = b.sequence("S", "Audio", None).unwrap();
    let doc = b.finish();
    let synth = FakeSynth::new(25.0);
    let (mut engine, _renderer) = Engine::new(
        EngineConfig::new(48_000.0),
        Box::new(OfflineSinks::new(64).with_fake_synth(&synth)),
    )
    .unwrap();
    engine.load(&doc, seq).unwrap();
    assert!(engine.start_measure(&doc, dest, vec![2]).unwrap().is_err());
    assert!(
        engine
            .start_measure(&doc, DestId(99), vec![2])
            .unwrap()
            .is_err()
    );
    engine.set_return_dests(vec![dest]);
    engine.load(&doc, seq).unwrap();
    assert!(engine.start_measure(&doc, dest, vec![2]).unwrap().is_ok());
}

/// Pre-flight M8: a measurement removed while its note sounds ends the
/// note, so it cannot hang: the fake synth's log ends with its note-off.
#[test]
fn removing_a_measurement_mid_note_sends_its_note_off() {
    let mut b = Builder::new();
    let dest = b.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (seq, _) = b.sequence("S", "Part", Some(dest)).unwrap();
    let doc = b.finish();
    let synth = FakeSynth::new(25.0);
    let sinks = OfflineSinks::new(1 << 16).with_fake_synth(&synth);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.set_return_dests(vec![dest]);
    engine.load(&doc, seq).unwrap();
    let target = engine
        .targets()
        .find(&TargetKind::MidiDestination(MidiPortInfo {
            name: FAKE_SYNTH_PORT.into(),
            unique_id: None,
        }))
        .unwrap();
    let mut backend = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Signals)
        .with_synth(&synth);
    backend.start(renderer).unwrap();
    let status = engine.start_measure(&doc, dest, vec![2]).unwrap().unwrap();
    // The first note goes out at block 10 (run 5120) and would end 100 ms
    // later, at block 20: blocks 0..15 leave it sounding.
    for _ in 0..15 {
        backend.block(512);
    }
    assert!(status.sent(0).is_some() && status.sent(1).is_none());
    // Block 15 applies the removal.
    engine.stop_measure().unwrap();
    backend.block(512);
    assert!(status.closed());
    drop(backend.stop().unwrap());
    let sent: Vec<Vec<u8>> = log
        .drain()
        .into_iter()
        .filter(|r| r.target == target)
        .map(|r| r.bytes().to_vec())
        .collect();
    let last = sent.last().expect("the synth heard the measurement");
    assert_eq!(last.get(..2), Some(&[0x80, 60][..]), "{sent:x?}");
    assert!(
        sent.iter().any(|m| m.get(..2) == Some(&[0x90, 60][..])),
        "{sent:x?}"
    );
}
