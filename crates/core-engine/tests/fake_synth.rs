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
    let (ch, delay) = play_with(
        delay_ms,
        total,
        512,
        0,
        &[(Q, 60, Q / 2), (2 * Q, 64, Q / 2)],
    );
    assert_eq!(
        delay,
        play_delay(delay_ms),
        "the delay does not grow with a block this size"
    );
    (ch, delay)
}

/// The delay in frames `play` expects for a block of 512.
fn play_delay(delay_ms: f64) -> u64 {
    ((delay_ms * 48.0).round() as u64).max(512)
}

/// Like `play`, with blocks of `block` frames, the engine's MIDI offset
/// `offset_ns` and notes `(tick, key, ticks held)`; returns the synth's
/// delay after the run (it grows to cover the blocks rendered).
fn play_with(
    delay_ms: f64,
    total: usize,
    block: u32,
    offset_ns: i64,
    notes: &[(i64, u8, i64)],
) -> (Vec<f32>, u64) {
    let mut b = Builder::new();
    let dest = b.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (seq, lane) = b.sequence("S", "Part", Some(dest)).unwrap();
    let specs: Vec<_> = notes.iter().map(|(t, k, d)| (*t, *k, 0x8000, *d)).collect();
    b.notes(seq, lane, &specs).unwrap();
    let doc = b.finish();
    let synth = FakeSynth::new(delay_ms);
    let sinks = OfflineSinks::new(1 << 16).with_fake_synth(&synth);
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.midi_offset_ns = offset_ns;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(sinks)).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut backend = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Signals)
        .with_synth(&synth);
    backend.start(renderer).unwrap();
    engine.play().unwrap();
    let mut out = Vec::new();
    while out.len() < total {
        backend.block(block);
        out.extend_from_slice(backend.input(FAKE_SYNTH_CHANNEL, block));
    }
    drop(backend.stop().unwrap());
    assert_eq!(synth.dropped(), 0);
    (out, backend.synth_delay().unwrap())
}

#[test]
fn a_note_on_clicks_on_channel_3_the_delay_after_it_is_heard() {
    assert_eq!(FAKE_SYNTH_CHANNEL, 2, "channel 3, 0-based");
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
    let held = first + 241..first + BEAT / 2;
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

#[test]
fn the_delay_holds_for_the_blocks_actually_rendered() {
    // Blocks of 1024 on a backend configured for 512: a note sent in a
    // block sounds the (grown) delay after its audible frame, not at the
    // next block's start.
    let (ch, delay) = play_with(10.0, 2 * BEAT, 1024, 0, &[(Q, 60, Q / 2)]);
    assert_eq!(delay, 1024);
    assert!(ch[..BEAT + 1024].iter().all(|x| *x == 0.0), "never early");
    assert!(ch[BEAT + 1024] >= FAKE_CLICK_LEVEL - 1e-6);
}

#[test]
fn a_negative_midi_offset_never_makes_the_synth_early() {
    // The note is heard 50 ms (2400 frames) before its beat; the synth
    // answers its delay (25 ms) after that, however late the block that
    // carried it ends.
    let (ch, delay) = play_with(25.0, 2 * BEAT, 512, -50_000_000, &[(Q, 60, Q / 2)]);
    assert_eq!(delay, 512 + 2400, "a block plus the offset");
    let at = BEAT - 2400 + delay as usize;
    assert!(ch[..at].iter().all(|x| *x == 0.0), "never early");
    assert!(ch[at] >= FAKE_CLICK_LEVEL - 1e-6, "and on time");
}

#[test]
fn overlapping_notes_sound_until_the_last_is_off() {
    // A holds beats 1-2, B beats 1.5-2.5.
    let (ch, delay) = play_with(25.0, 4 * BEAT, 512, 0, &[(Q, 60, Q), (Q + Q / 2, 64, Q)]);
    let d = delay as usize;
    let a_off = 2 * BEAT + d;
    let b_off = 2 * BEAT + BEAT / 2 + d;
    assert!(
        ch[a_off + 10..b_off]
            .iter()
            .any(|x| x.abs() > FAKE_TONE_LEVEL / 2.0)
    );
    assert!(
        ch[b_off..].iter().all(|x| *x == 0.0),
        "silent once both are off"
    );
}
