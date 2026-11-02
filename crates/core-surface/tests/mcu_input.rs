//! MCU input, byte by byte (plan 5f-1 Task 1; spec §8.2, §12.3).

mod common;

use common::view;
use core_model::automation::AutomationTarget as T;
use core_model::ids::StripId;
use core_surface::mcu::{Mcu, handshake_response};
use core_surface::{IDLE_RELEASE, MixerView, Phase, Surface, SurfaceAction as A, midi, wind_rate};
use std::time::{Duration, Instant};

/// A unit that has shown `v` from strip 0.
fn shown(extender: bool, v: &MixerView) -> (Mcu, Instant) {
    let now = Instant::now();
    let mut m = Mcu::new(extender);
    let _ = m.feedback(v, 0, now);
    (m, now)
}

#[test]
fn split_finds_whole_messages_and_skips_garbage() {
    let b = [
        0x90, 0x68, 0x7F, 0xF0, 0x00, 0x00, 0x66, 0x14, 0x00, 0xF7, 0xE0, 0x10,
    ];
    let parts = midi::split(&b);
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], Ok(b[0..3].to_vec()));
    assert_eq!(parts[1], Ok(b[3..10].to_vec()));
    assert_eq!(parts[2], Err(2), "a cut pitch bend is skipped and reported");
    assert_eq!(
        midi::split(&[0x68, 0x7F]),
        vec![Err(1), Err(1)],
        "running status is not supported"
    );
    assert_eq!(
        midi::split(&[0x90, 0x80, 0x00]),
        vec![Err(3)],
        "a data byte above 0x7f"
    );
    assert_eq!(
        midi::split(&[0xF8, 0x90, 0xFE, 0x68, 0x7F, 0xF0, 0x00, 0xF8, 0xF7, 0xFF]),
        vec![Ok(vec![0x90, 0x68, 0x7F]), Ok(vec![0xF0, 0x00, 0xF7])],
        "real-time bytes are skipped anywhere, even inside a message, and not reported"
    );
    assert_eq!(
        midi::split(&[0xF4]),
        vec![Err(1)],
        "an undefined status starts nothing"
    );
    assert_eq!(midi::hex(&[0x90, 0x68, 0x7F]), "90 68 7f");
}

#[test]
fn a_touched_fader_moves_and_releases() {
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    assert_eq!(
        m.input(&[0x90, 0x68, 0x7F], now),
        vec![A::Gesture(T::StripFader(StripId(1)), Phase::Touch(None))]
    );
    assert_eq!(m.touched(), vec![T::StripFader(StripId(1))]);
    let a = m.input(&[0xE0, 0x7F, 0x7F], now);
    assert_eq!(
        a,
        vec![A::Gesture(T::StripFader(StripId(1)), Phase::Value(1.0))]
    );
    let a = m.input(&[0xE7, 0x00, 0x20], now);
    match a.as_slice() {
        [A::Gesture(T::StripFader(StripId(8)), Phase::Touch(Some(x)))] => {
            assert!(
                (x - 4096.0 / 16383.0).abs() < 1e-9,
                "an untouched move is an idle touch"
            )
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        m.input(&[0x90, 0x68, 0x00], now),
        vec![A::Gesture(T::StripFader(StripId(1)), Phase::Release)]
    );
    assert_eq!(
        m.tick(now + IDLE_RELEASE + Duration::from_millis(1)),
        vec![A::IdleRelease(T::StripFader(StripId(8)), now)],
        "500 ms after its last value, stamped at that value"
    );
}

#[test]
fn the_master_fader_is_the_main_units_only() {
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    assert_eq!(
        m.input(&[0x90, 0x70, 0x7F], now),
        vec![A::Gesture(T::StripFader(StripId(100)), Phase::Touch(None))]
    );
    let (mut xt, now) = shown(true, &v);
    assert!(
        xt.input(&[0x90, 0x70, 0x7F, 0xE8, 0x00, 0x40], now)
            .is_empty()
    );
    assert_eq!(xt.ignored(), 2, "an extender has no master");
}

#[test]
fn buttons_click_select_arm_and_run_commands() {
    let mut v = view(8);
    v.strips[1].mute = true;
    let (mut m, now) = shown(false, &v);
    assert_eq!(
        m.input(&[0x90, 0x11, 0x7F], now),
        vec![
            A::Gesture(T::StripMute(StripId(2)), Phase::Touch(Some(0.0))),
            A::Gesture(T::StripMute(StripId(2)), Phase::Value(0.0)),
            A::Gesture(T::StripMute(StripId(2)), Phase::Release),
        ],
        "a lit mute unmutes"
    );
    assert!(
        m.input(&[0x90, 0x11, 0x00], now).is_empty(),
        "releases do nothing"
    );
    assert_eq!(
        m.input(&[0x90, 0x08, 0x7F], now)[0],
        A::Gesture(T::StripSolo(StripId(1)), Phase::Touch(Some(1.0)))
    );
    assert_eq!(
        m.input(&[0x90, 0x1A, 0x7F], now),
        vec![A::Select(StripId(3))]
    );
    assert_eq!(m.input(&[0x90, 0x03, 0x7F], now), vec![A::Arm(StripId(4))]);
    for (note, id) in [
        (0x5E, "transport.play"),
        (0x5D, "transport.stop"),
        (0x5F, "transport.record"),
        (0x56, "transport.loop"),
        (0x51, "edit.undo"),
        (0x50, "file.save"),
        (0x4A, "automation.mode.read"),
        (0x4B, "automation.mode.write"),
        (0x4C, "automation.mode.trim"),
        (0x4D, "automation.mode.touch"),
        (0x4E, "automation.mode.latch"),
    ] {
        assert_eq!(
            m.input(&[0x90, note, 0x7F], now),
            vec![A::Command(id.into())],
            "{note:#x}"
        );
    }
    assert_eq!(m.input(&[0x90, 0x2E, 0x7F], now), vec![A::Bank(-1)]);
    assert_eq!(m.input(&[0x90, 0x2F, 0x7F], now), vec![A::Bank(1)]);
    assert_eq!(m.input(&[0x90, 0x30, 0x7F], now), vec![A::Channel(-1)]);
    assert_eq!(m.input(&[0x90, 0x31, 0x7F], now), vec![A::Channel(1)]);
    assert_eq!(m.input(&[0xB0, 0x3C, 0x03], now), vec![A::Jog(3)]);
    assert_eq!(m.input(&[0xB0, 0x3C, 0x41], now), vec![A::Jog(-1)]);
}

/// Owner answer 2026-11-02 (Ruling 5): a tap skips; a hold winds, faster
/// the longer it is held; the rate curve on the passed clock.
#[test]
fn rewind_and_forward_skip_on_a_tap_and_wind_while_held() {
    let ms = Duration::from_millis;
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    assert!(
        m.input(&[0x90, 0x5C, 0x7F], now).is_empty(),
        "a press waits: tap or hold"
    );
    assert_eq!(
        m.input(&[0x90, 0x5C, 0x00], now + ms(150)),
        vec![A::Skip(1)],
        "a tap: the next marker or bar"
    );
    assert!(m.input(&[0x90, 0x5B, 0x7F], now).is_empty());
    assert!(m.tick(now + ms(399)).is_empty(), "not held long enough");
    assert!(m.tick(now + ms(450)).is_empty(), "one wind per 100 ms");
    let wind = |a: Vec<A>| match a.as_slice() {
        [A::Wind(q)] => *q,
        other => panic!("{other:?}"),
    };
    assert!(
        (wind(m.tick(now + ms(500))) + 0.1 * 4.0).abs() < 1e-9,
        "0.1 s at 4 quarter notes a second, back"
    );
    assert!(
        (wind(m.tick(now + ms(1_900))) + (0.9 * 4.0 + 0.5 * 8.0)).abs() < 1e-9,
        "the rate doubles after a second of winding"
    );
    assert!(
        (wind(m.input(&[0x90, 0x5B, 0x00], now + ms(1_950))) + 0.05 * 8.0).abs() < 1e-9,
        "the release winds the rest, and does not skip"
    );
    assert!(m.tick(now + ms(3_000)).is_empty(), "released");
    assert_eq!(wind_rate(ms(399)), 0.0);
    assert_eq!(
        [
            wind_rate(ms(400)),
            wind_rate(ms(1_399)),
            wind_rate(ms(1_400)),
            wind_rate(ms(2_400)),
            wind_rate(ms(3_400)),
            wind_rate(ms(60_000))
        ],
        [4.0, 4.0, 8.0, 16.0, 32.0, 32.0]
    );
    let (mut xt, now) = shown(true, &v);
    assert!(
        xt.input(&[0x90, 0x5C, 0x7F, 0x90, 0x5C, 0x00], now)
            .is_empty(),
        "an extender has no transport"
    );
}

#[test]
fn vpots_move_pan_send_and_plugin_parameters() {
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    match m.input(&[0xB0, 0x10, 0x05], now).as_slice() {
        [A::Gesture(T::StripPan(StripId(1)), Phase::Touch(Some(x)))] => {
            assert!((x - 0.55).abs() < 1e-9)
        }
        other => panic!("{other:?}"),
    }
    match m.input(&[0xB0, 0x10, 0x42], now).as_slice() {
        [A::Gesture(T::StripPan(StripId(1)), Phase::Value(x))] => {
            assert!(
                (x - 0.53).abs() < 1e-9,
                "counter-clockwise from its own last value while held"
            )
        }
        other => panic!("{other:?}"),
    }
    let _ = m.input(&[0x90, 0x29, 0x7F], now);
    match m.input(&[0xB0, 0x11, 0x01], now).as_slice() {
        [A::Gesture(T::SendLevel(StripId(2), s), Phase::Touch(Some(x)))] => {
            assert_eq!(s.0, 0);
            assert!((x - 0.26).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn release_all_forgets_every_hold() {
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    let _ = m.input(&[0x90, 0x68, 0x7F], now);
    let _ = m.input(&[0xE7, 0x00, 0x20], now);
    let _ = m.input(&[0xB0, 0x11, 0x01], now);
    let _ = m.input(&[0x90, 0x5C, 0x7F], now);
    let mut held = m.release_all();
    held.sort();
    let mut want = vec![
        T::StripFader(StripId(1)),
        T::StripFader(StripId(8)),
        T::StripPan(StripId(2)),
    ];
    want.sort();
    assert_eq!(held, want, "the touched fader and both idle holds");
    assert!(m.touched().is_empty());
    assert!(
        m.tick(now + IDLE_RELEASE * 2).is_empty(),
        "nothing left to release, and no wind"
    );
    assert!(m.release_all().is_empty());
}

#[test]
// The formula is written term by term as Ruling 14 gives it; with this
// challenge `c3 << 4` is 0, which clippy flags.
#[allow(clippy::identity_op)]
fn the_handshake_is_answered_and_marks_the_unit_online() {
    let v = view(8);
    let (mut m, now) = shown(false, &v);
    assert_eq!(
        m.hello(),
        vec![vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x00, 0xF7]]
    );
    assert!(!m.online());
    let mut q = vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x01];
    q.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7]);
    q.extend_from_slice(&[0x10, 0x20, 0x30, 0x40]);
    q.push(0xF7);
    assert!(m.input(&q, now).is_empty());
    let r = handshake_response([0x10, 0x20, 0x30, 0x40]);
    assert_eq!(
        r,
        [
            0x7F & 0x10u8.wrapping_add(0x20 ^ 0x0A).wrapping_sub(0x40),
            0x7F & ((0x30 >> 4) ^ 0x10u8.wrapping_add(0x40)),
            0x7F & (0x40u8.wrapping_sub(0x30 << 2) ^ (0x10 | 0x20)),
            0x7F & 0x20u8.wrapping_sub(0x30).wrapping_add(0xF0 ^ (0x40u8 << 4)),
        ]
    );
    let mut fb = m.feedback(&v, 0, now);
    let mut want = vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x02, 1, 2, 3, 4, 5, 6, 7];
    want.extend_from_slice(&r);
    want.push(0xF7);
    assert!(
        fb.contains(&want),
        "the reply goes out with the next feedback"
    );
    fb.clear();
    assert!(m.online(), "a valid query marks it online");
    assert_eq!(Mcu::new(true).device_id(), 0x15);
}

#[test]
fn malformed_bytes_are_ignored_and_counted() {
    let v = view(3);
    let (mut m, now) = shown(false, &v);
    let before = m.ignored();
    for b in [
        &[0x90, 0x68][..],
        &[0xF0, 0x00, 0x00][..],
        &[0x90, 0x7F, 0x7F][..],
        &[0x91, 0x68, 0x7F][..],
        &[0xF4][..],
        &[0x90, 0x05, 0x7F][..],
    ] {
        let _ = m.input(b, now);
    }
    assert_eq!(
        m.ignored(),
        before + 6,
        "cut, unterminated, unknown note, channel 2, an undefined status, rec on an empty slot"
    );
    let _ = m.input(&[0xF8, 0xFE, 0xFE], now);
    assert_eq!(
        m.ignored(),
        before + 6,
        "clock and active sensing are not counted"
    );
    let mut fresh = Mcu::new(false);
    assert!(
        fresh.input(&[0x90, 0x10, 0x7F], now).is_empty(),
        "nothing shown yet"
    );
    assert_eq!(fresh.ignored(), 1);
}
