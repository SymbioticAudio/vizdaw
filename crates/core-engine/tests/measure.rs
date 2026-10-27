//! Measuring a synth's return latency (plan 5c-2 Task 5; sub-project 5
//! spec §6.7): with the transport stopped, five notes 500 ms apart reach
//! the synth's destination and each one's onset is found on the return
//! input the synth's delay later; a silent input, or one that never falls
//! quiet (a ringing release, a hum), finds none; only a destination the
//! load interned can be measured.

use core_engine::backend::{AudioBackend, MidiPortInfo};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::Builder;
use core_engine::graph::TestInserts;
use core_engine::measure::{MeasureStatus, PINGS};
use core_engine::offline::{FAKE_SYNTH_PORT, FakeInput, FakeSynth, OfflineBackend, OfflineSinks};
use core_engine::schedule::TargetKind;
use core_model::ids::DestId;
use std::sync::Arc;

/// Measures the destination named `port` (its lane's return on `inputs`)
/// against a fake synth 25 ms late; returns the status and the synth's
/// delay in frames.
fn measure(port: &str, inputs: Vec<u16>) -> (Arc<MeasureStatus>, i64) {
    measure_with(port, inputs, 0, 0)
}

/// Like [`measure`], with the engine's MIDI offset `offset_ns` and a graph
/// latency of `latency` frames (a latent insert on an aux bus); the delay
/// returned is the synth's after the run (it grows to cover a negative
/// offset).
fn measure_with(
    port: &str,
    inputs: Vec<u16>,
    offset_ns: i64,
    latency: u64,
) -> (Arc<MeasureStatus>, i64) {
    let mut b = Builder::new();
    let dest = b.dest(port, port, 0).unwrap();
    let (seq, _) = b.sequence("S", "Part", Some(dest)).unwrap();
    if latency > 0 {
        let bus = b.bus("Latent").unwrap();
        b.insert(bus, TestInserts::plugin(&format!("latency={latency}")))
            .unwrap();
    }
    let doc = b.finish();
    let synth = FakeSynth::new(25.0);
    // "Other" is a MIDI destination nothing answers.
    let sinks = OfflineSinks::new(1 << 16)
        .with_ports(vec![MidiPortInfo {
            name: "Other".into(),
            unique_id: None,
        }])
        .with_fake_synth(&synth);
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.midi_offset_ns = offset_ns;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(sinks)).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_return_dests(vec![dest]);
    engine.load(&doc, seq).unwrap();
    assert_eq!(engine.latency_frames(), latency);
    let mut backend = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Signals)
        .with_synth(&synth);
    backend.start(renderer).unwrap();
    let status = engine
        .start_measure(&doc, dest, inputs, 0)
        .unwrap()
        .unwrap();
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
    let delay = backend.synth_delay().unwrap() as i64;
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
    // A pair: either channel's onset counts. Fake input 2's impulses fall
    // at 0, 24 000, 48 000 … and every note's window opens after its
    // impulse, so each onset is the synth's on channel 3.
    let (s, delay) = measure(FAKE_SYNTH_PORT, vec![1, 2]);
    for i in 0..PINGS {
        assert_eq!(s.onset(i), Some(s.sent(i).unwrap() + delay), "note {i}");
    }
}

/// Fix round 1: a negative MIDI offset sends each note early, so it is
/// heard before the block that sends it; `sent` is that earlier run, and
/// the onset still follows it by exactly the synth's delay.
#[test]
fn a_negative_midi_offset_is_part_of_when_a_note_was_heard() {
    let (s, delay) = measure_with(FAKE_SYNTH_PORT, vec![2], -50_000_000, 0);
    assert!(s.done());
    // 50 ms early: 2400 frames before the block the note went out in.
    let sent0 = s.sent(0).unwrap();
    assert!((4800 - 2400..4800 - 2400 + 512).contains(&sent0), "{sent0}");
    for i in 0..PINGS {
        assert_eq!(s.onset(i), Some(s.sent(i).unwrap() + delay), "note {i}");
    }
}

/// Fix round 1: with a graph latency, a note is heard that much after the
/// block that sends it (MIDI waits for the delayed audio): `sent` is
/// later by the latency, and the onset follows it by the synth's delay.
#[test]
fn the_graph_latency_is_part_of_when_a_note_was_heard() {
    const L: i64 = 480;
    let (s, delay) = measure_with(FAKE_SYNTH_PORT, vec![2], 0, L as u64);
    assert!(s.done());
    let sent0 = s.sent(0).unwrap();
    assert!((4800 + L..4800 + L + 512).contains(&sent0), "{sent0}");
    for i in 0..PINGS {
        assert_eq!(s.onset(i), Some(s.sent(i).unwrap() + delay), "note {i}");
    }
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
    assert!(
        engine
            .start_measure(&doc, dest, vec![2], 0)
            .unwrap()
            .is_err()
    );
    assert!(
        engine
            .start_measure(&doc, DestId(99), vec![2], 0)
            .unwrap()
            .is_err()
    );
    engine.set_return_dests(vec![dest]);
    engine.load(&doc, seq).unwrap();
    assert!(
        engine
            .start_measure(&doc, dest, vec![2], 0)
            .unwrap()
            .is_ok()
    );
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
    let status = engine
        .start_measure(&doc, dest, vec![2], 0)
        .unwrap()
        .unwrap();
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

/// Fix round 1, I1: an onset is credited only to the latest note whose
/// window has begun, never to an older one. A synth answering 3 ms late
/// (under the shortfall of a 23 552-frame gap at 512-frame blocks) leaves
/// note 2 unanswered: note 3's answer, inside note 2's 500 ms window too,
/// must not be taken for note 2's.
#[test]
fn an_onset_is_credited_only_to_the_latest_note_whose_window_began() {
    use core_engine::measure::{MeasureRequest, tap};
    use core_engine::schedule::TargetIdx;
    const BLOCK: u32 = 512;
    const ANSWER: i64 = 144; // 3 ms at 48 kHz
    let req = MeasureRequest {
        target: TargetIdx(0),
        channel: 0,
        inputs: vec![0],
        trim: 0,
    };
    let (mut t, s) = tap(&req, 48_000.0);
    let mut clicks: Vec<i64> = Vec::new();
    let mut buf = vec![0.0f32; BLOCK as usize];
    let mut run = 0i64;
    let mut ons = 0usize;
    while !s.done() && run < 3 * 48_000 {
        t.sends(0, |m| {
            if m.as_slice().first().is_some_and(|b| b & 0xF0 == 0x90) {
                if ons != 2 {
                    clicks.push(run + ANSWER);
                }
                ons += 1;
            }
        });
        for (j, x) in buf.iter_mut().enumerate() {
            let k = run + j as i64;
            let on = clicks.iter().any(|c| k >= *c && k - c < 240);
            *x = if on { 0.5 } else { 0.0 };
        }
        t.capture(&[&buf], BLOCK, 0);
        run += i64::from(BLOCK);
    }
    assert!(s.done());
    assert_eq!(ons, PINGS);
    for i in 0..PINGS {
        let want = (i != 2).then(|| s.sent(i).unwrap() + ANSWER);
        assert_eq!(s.onset(i), want, "note {i}");
    }
}

/// Drives a bare measurement whose device reports a round trip of
/// `delay` frames while its input really arrives `true_delay` after the
/// output, against a synth answering `answer` frames after each note is
/// heard, with the recording latency trim `trim`; returns the status.
fn measure_round_trip(delay: i64, true_delay: i64, trim: i64, answer: i64) -> Arc<MeasureStatus> {
    use core_engine::measure::{MeasureRequest, tap};
    use core_engine::schedule::TargetIdx;
    const BLOCK: u32 = 512;
    let req = MeasureRequest {
        target: TargetIdx(0),
        channel: 0,
        inputs: vec![0],
        trim,
    };
    let (mut t, s) = tap(&req, 48_000.0);
    let mut clicks: Vec<i64> = Vec::new();
    let mut buf = vec![0.0f32; BLOCK as usize];
    let mut run = 0i64;
    while !s.done() && run < 4 * 48_000 {
        t.sends(0, |m| {
            if m.as_slice().first().is_some_and(|b| b & 0xF0 == 0x90) {
                clicks.push(run + answer + true_delay);
            }
        });
        for (j, x) in buf.iter_mut().enumerate() {
            let k = run + j as i64;
            let on = clicks.iter().any(|c| k >= *c && k - c < 240);
            *x = if on { 0.5 } else { 0.0 };
        }
        t.capture(&[&buf], BLOCK, delay);
        run += i64::from(BLOCK);
    }
    assert!(s.done());
    assert_eq!(s.trim(), trim);
    s
}

/// Final review I1 (plan 5c-2 Ruling 8): each note's window starts at its
/// heard run plus the round trip as recording uses it, `delay − trim`. A
/// device reporting 1000 frames whose true round trip is 760 (a +240
/// trim) and a 3 ms synth: every note finds its own onset 3 ms after it,
/// none is credited to the note before it (which read about 504 ms).
#[test]
fn the_latency_trim_starts_each_notes_window() {
    const ANSWER: i64 = 144; // 3 ms at 48 kHz
    let s = measure_round_trip(1000, 760, 240, ANSWER);
    for i in 0..PINGS {
        let want = s.sent(i).unwrap() + 760 + ANSWER;
        assert_eq!(s.onset(i), Some(want), "note {i}");
        // What the control side reads: onset − heard − (delay − trim).
        assert_eq!(want - s.sent(i).unwrap() - (1000 - 240), ANSWER);
    }
}

/// Final review I1 (b): with the over-report left uncorrected (no trim),
/// each answer comes after a later note was heard but before that note's
/// window: it is ambiguous and credited to no note, so the measurement
/// finds nothing (a clean "no signal"), never about 500 ms.
#[test]
fn an_onset_before_its_notes_window_is_credited_to_no_note() {
    let s = measure_round_trip(1000, 760, 0, 144);
    for i in 0..PINGS {
        assert_eq!(s.onset(i), None, "note {i}");
    }
}
