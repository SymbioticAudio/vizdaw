#![cfg(target_os = "macos")]

use core_engine::backend::{InstrumentNode, StereoBuf};
use platform_macos::dls::DlsSynth;

fn rms_dbfs(buf: &StereoBuf, frames: usize) -> f64 {
    let sum: f64 = buf.left[..frames]
        .iter()
        .chain(&buf.right[..frames])
        .map(|x| f64::from(*x) * f64::from(*x))
        .sum();
    let rms = (sum / (2 * frames) as f64).sqrt();
    20.0 * rms.max(1e-12).log10()
}

#[test]
fn dls_renders_sound() {
    let mut synth = DlsSynth::new(48_000.0, 4096).unwrap();
    let mut buf = StereoBuf::new(512);
    synth.midi(0, &[0xC0, 0]); // Acoustic Grand Piano
    synth.midi(10, &[0x90, 60, 100]);
    let mut loudest = f64::NEG_INFINITY;
    for _ in 0..20 {
        synth.render(512, &mut buf);
        loudest = loudest.max(rms_dbfs(&buf, 512));
    }
    assert!(loudest > -40.0, "note too quiet: {loudest:.1} dBFS");

    // The DLS synth's built-in reverb tail is still about -78 dBFS after
    // 2 s, so the decay check waits 3 s.
    synth.midi(0, &[0x80, 60, 0]);
    let mut last = 0.0;
    for _ in 0..(3 * 48_000u32).div_ceil(512) {
        synth.render(512, &mut buf);
        last = rms_dbfs(&buf, 512);
    }
    assert!(last < -80.0, "note did not decay: {last:.1} dBFS");
}

#[test]
fn reset_silences_held_notes() {
    let mut synth = DlsSynth::new(44_100.0, 4096).unwrap();
    let mut buf = StereoBuf::new(1024);
    synth.midi(0, &[0x90, 64, 110]);
    synth.render(1024, &mut buf);
    synth.reset();
    for _ in 0..(3 * 44_100 / 1024) {
        synth.render(1024, &mut buf);
    }
    assert!(rms_dbfs(&buf, 1024) < -80.0);
}
