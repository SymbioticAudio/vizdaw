#![cfg(target_os = "macos")]
//! AU transport through `kAudioUnitProperty_HostCallbacks` (spec §5): the
//! units take the callbacks, and the callbacks report what the engine's
//! blocks said, the way a tempo-synced unit asks for it.

use core_engine::backend::BlockInfo;
use platform_macos::au;

fn block(playing: bool, frame: u64, beats: f64, tempo_bpm: f64) -> BlockInfo {
    BlockInfo {
        playing,
        frame,
        beats,
        tempo_bpm,
    }
}

#[test]
fn apple_units_take_the_host_callbacks() {
    for id in ["aufx/dely/appl", "aumu/dls /appl"] {
        let u = au::load(id, 48_000.0, 512).unwrap();
        assert!(u.host_callbacks_installed(), "{id}");
    }
}

#[test]
fn the_callbacks_report_tempo_position_and_jumps() {
    let seen = au::host_callbacks_after(
        48_000.0,
        &[
            (block(false, 0, 0.0, 120.0), 512),
            (block(true, 0, 0.0, 120.0), 512),
            (block(true, 512, 512.0 / 24_000.0, 120.0), 512),
            (block(true, 96_000, 6.5, 90.0), 512),
        ],
    );
    assert!(!seen[0].playing && !seen[0].changed, "{:?}", seen[0]);
    assert!(seen[1].playing && seen[1].changed, "start: {:?}", seen[1]);
    assert!(!seen[2].changed, "a following block: {:?}", seen[2]);
    assert_eq!(seen[2].sample, 512.0);
    assert_eq!(seen[2].tempo, 120.0);
    let s = seen[3];
    assert!(s.changed, "a locate is a jump");
    assert_eq!((s.beat, s.tempo, s.sample), (6.5, 90.0, 96_000.0));
    // Half a beat at 90 BPM and 48 kHz is 16 000 frames.
    assert_eq!(s.to_next_beat, 16_000);
    assert_eq!((s.numerator, s.denominator, s.downbeat), (4.0, 4, 4.0));
}
