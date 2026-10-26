//! The fake synth (plan 5c-2 Task 4; sub-project 5 spec §12.1
//! `VIZDAW_FAKE_SYNTH`): notes sent to the MIDI destination "Fake Synth"
//! play on fake input channel 3 the synth's delay after their audible
//! time: a 5 ms click at each note-on and a 220 Hz tone while a note is
//! held. The delay is at least one block; with the loopback input there
//! is no synth.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{
    FAKE_CLICK_LEVEL, FAKE_SYNTH_CHANNEL, FAKE_SYNTH_PORT, FAKE_TONE_LEVEL, FakeInput, FakeSynth,
    OfflineBackend, OfflineSinks,
};

/// 120 BPM at 48 kHz: one beat is 24 000 frames.
const BEAT: usize = 24_000;

/// Plays notes at beats 1 and 2 (half a beat each) through the fake
/// synth for `total` frames; returns channel 3 and the synth's delay.
fn play(delay_ms: f64, total: usize) -> (Vec<f32>, u64) {
    let mut b = Builder::new();
    let dest = b.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (seq, lane) = b.sequence("S", "Part", Some(dest)).unwrap();
    b.notes(
        seq,
        lane,
        &[(Q, 60, 0x8000, Q / 2), (2 * Q, 64, 0x8000, Q / 2)],
    )
    .unwrap();
    let doc = b.finish();
    let synth = FakeSynth::new(delay_ms);
    let sinks = OfflineSinks::new(1 << 16).with_fake_synth(&synth);
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut backend = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Signals)
        .with_synth(&synth);
    let delay = backend.synth_delay().unwrap();
    backend.start(renderer).unwrap();
    engine.play().unwrap();
    let mut out = Vec::new();
    while out.len() < total {
        backend.block(512);
        out.extend_from_slice(backend.input(FAKE_SYNTH_CHANNEL, 512));
    }
    drop(backend.stop().unwrap());
    assert_eq!(synth.dropped(), 0);
    (out, delay)
}

#[test]
fn a_note_on_clicks_on_channel_3_the_delay_after_it_is_heard() {
    let (ch, delay) = play(25.0, 3 * BEAT);
    assert_eq!(delay, 1200, "25 ms at 48 kHz");
    let first = BEAT + 1200;
    assert!(ch[..first].iter().all(|x| *x == 0.0), "silent before");
    assert!(
        ch[first] >= FAKE_CLICK_LEVEL - 1e-6,
        "the click starts on time"
    );
    // The click lasts 5 ms; the tone sounds while the note is held (half
    // a beat), then the note-off ends it.
    let held = first + 240..first + BEAT / 2;
    assert!(
        ch[held.clone()]
            .iter()
            .all(|x| x.abs() <= FAKE_TONE_LEVEL + 1e-6)
    );
    assert!(ch[held].iter().any(|x| x.abs() > FAKE_TONE_LEVEL / 2.0));
    let second = 2 * BEAT + 1200;
    assert!(
        ch[first + BEAT / 2..second].iter().all(|x| *x == 0.0),
        "the note-off ends the tone"
    );
    assert!(ch[second] >= FAKE_CLICK_LEVEL - 1e-6);
}

#[test]
fn the_delay_is_at_least_one_block() {
    let (ch, delay) = play(1.0, 2 * BEAT);
    assert_eq!(delay, 512);
    assert_eq!(ch.iter().position(|x| x.abs() > 0.01), Some(BEAT + 512));
}

#[test]
fn with_the_loopback_input_there_is_no_synth() {
    let synth = FakeSynth::new(25.0);
    let b = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Loopback(1000))
        .with_synth(&synth);
    assert_eq!(b.synth_delay(), None);
    let b = OfflineBackend::new(48_000.0, 512).with_synth(&synth);
    assert_eq!(b.synth_delay(), None, "no fake input: no synth either");
}
