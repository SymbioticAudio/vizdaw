//! HUI, byte by byte (plan 5f-1 Task 3; spec §8.2, §12.3).

mod common;

use common::view;
use core_model::automation::AutomationTarget as T;
use core_model::ids::StripId;
use core_surface::hui::{Hui, PING};
use core_surface::{Phase, Surface, SurfaceAction as A};
use std::time::{Duration, Instant};

fn has(fb: &[Vec<u8>], m: &[u8]) -> bool {
    fb.iter().any(|x| x.as_slice() == m)
}

fn shown() -> (Hui, Instant) {
    let now = Instant::now();
    let mut h = Hui::new();
    let _ = h.feedback(&view(8), 0, now);
    (h, now)
}

#[test]
fn zone_and_port_pairs_touch_and_move_a_fader() {
    let (mut h, now) = shown();
    assert_eq!(
        h.input(&[0xB0, 0x0F, 0x02, 0xB0, 0x2F, 0x40], now),
        vec![A::Gesture(T::StripFader(StripId(3)), Phase::Touch(None))]
    );
    let a = h.input(&[0xB0, 0x02, 0x7F, 0xB0, 0x22, 0x7F], now);
    assert_eq!(
        a,
        vec![A::Gesture(T::StripFader(StripId(3)), Phase::Value(1.0))]
    );
    assert_eq!(
        h.input(&[0xB0, 0x0F, 0x02, 0xB0, 0x2F, 0x00], now),
        vec![A::Gesture(T::StripFader(StripId(3)), Phase::Release)]
    );
}

#[test]
fn switches_mute_solo_select_arm_bank_and_transport() {
    let (mut h, now) = shown();
    let z = |zone: u8, port: u8| [0xB0, 0x0F, zone, 0xB0, 0x2F, 0x40 | port];
    assert_eq!(
        h.input(&z(0, 2), now)[0],
        A::Gesture(T::StripMute(StripId(1)), Phase::Touch(Some(1.0)))
    );
    assert_eq!(
        h.input(&z(1, 3), now)[0],
        A::Gesture(T::StripSolo(StripId(2)), Phase::Touch(Some(1.0)))
    );
    assert_eq!(h.input(&z(4, 1), now), vec![A::Select(StripId(5))]);
    assert_eq!(h.input(&z(5, 7), now), vec![A::Arm(StripId(6))]);
    assert_eq!(h.input(&z(0x0A, 1), now), vec![A::Bank(-1)]);
    assert_eq!(h.input(&z(0x0A, 3), now), vec![A::Bank(1)]);
    assert_eq!(h.input(&z(0x0A, 0), now), vec![A::Channel(-1)]);
    assert_eq!(
        h.input(&z(0x0E, 4), now),
        vec![A::Command("transport.play".into())]
    );
    assert_eq!(
        h.input(&z(0x0E, 3), now),
        vec![A::Command("transport.stop".into())]
    );
    assert_eq!(
        h.input(&z(0x0E, 5), now),
        vec![A::Command("transport.record".into())]
    );
    assert!(
        h.input(&z(0x0E, 2), now).is_empty(),
        "fast forward waits: tap or hold"
    );
    assert_eq!(
        h.input(&[0xB0, 0x0F, 0x0E, 0xB0, 0x2F, 0x02], now),
        vec![A::Skip(1)],
        "a tap"
    );
    assert_eq!(h.input(&[0xB0, 0x0D, 0x02], now), vec![A::Jog(2)]);
    assert_eq!(h.input(&[0xB0, 0x0D, 0x42], now), vec![A::Jog(-2)]);
    let n = h.ignored();
    assert!(
        h.input(&z(0, 5), now).is_empty(),
        "V-Sel: HUI V-pots are pan only in 5f-1"
    );
    assert_eq!(h.ignored(), n, "and it is not counted");
}

#[test]
fn vpots_turn_by_their_detents() {
    let (mut h, now) = shown();
    match h.input(&[0xB0, 0x40, 0x43], now).as_slice() {
        [A::Gesture(T::StripPan(StripId(1)), Phase::Touch(Some(x)))] => {
            assert!((x - 0.53).abs() < 1e-9)
        }
        other => panic!("{other:?}"),
    }
    match h.input(&[0xB0, 0x40, 0x01], now).as_slice() {
        [A::Gesture(T::StripPan(StripId(1)), Phase::Value(x))] => {
            assert!((x - 0.52).abs() < 1e-9, "from its own last value")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn ping_display_meters_and_leds() {
    let mut v = view(8);
    v.strips[0].mute = true;
    v.strips[1].meter = (1.0, 0.0);
    v.strips[0].name = "Bass Guitar".into();
    let now = Instant::now();
    let mut h = Hui::new();
    let fb = h.feedback(&v, 0, now);
    assert!(has(&fb, &PING));
    assert!(
        has(&fb, &[0xB0, 0x0C, 0x00]) && has(&fb, &[0xB0, 0x2C, 0x42]),
        "zone 0, mute LED on"
    );
    assert!(has(&fb, &[0xA0, 0x01, 0x0C]), "strip 2 left at 0 dBFS");
    assert!(
        has(&fb, &[0xA0, 0x01, 0x10]),
        "strip 2 right silent: side 1, level 0"
    );
    assert!(has(
        &fb,
        &[
            0xF0, 0x00, 0x00, 0x66, 0x05, 0x00, 0x10, 0x00, b'B', b'a', b's', b's', 0xF7
        ]
    ));
    assert!(
        fb.iter().any(
            |x| x.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x05, 0x00, 0x12, 0x00]) && x.len() == 19
        ),
        "a 10-character zone of the 40-character display"
    );
    assert!(!h.online());
    assert!(h.input(&[0x90, 0x00, 0x7F], now).is_empty());
    assert!(h.online(), "the ping is answered");
    let fb = h.feedback(&v, 0, now + Duration::from_millis(500));
    assert!(!has(&fb, &PING), "once a second");
    let fb = h.feedback(&v, 0, now + Duration::from_millis(1001));
    assert!(has(&fb, &PING));
    let _ = h.feedback(&v, 0, now + Duration::from_millis(4100));
    assert!(!h.online(), "no reply for 3 s");
}

#[test]
fn hui_fader_touch_blocks_feedback() {
    let mut v = view(8);
    let (mut h, now) = shown();
    let _ = h.input(&[0xB0, 0x0F, 0x00, 0xB0, 0x2F, 0x40], now);
    v.strips[0].fader = 0.0;
    let fb = h.feedback(&v, 0, now + Duration::from_millis(20));
    assert!(
        fb.iter()
            .all(|x| !(x.first() == Some(&0xB0) && x.get(1) == Some(&0x00))),
        "{fb:?}"
    );
    let _ = h.input(&[0xB0, 0x0F, 0x00, 0xB0, 0x2F, 0x00], now);
    let fb = h.feedback(&v, 0, now + Duration::from_millis(40));
    assert!(has(&fb, &[0xB0, 0x00, 0x00]) && has(&fb, &[0xB0, 0x20, 0x00]));
}

#[test]
fn malformed_bytes_are_ignored_and_counted() {
    let (mut h, now) = shown();
    let n = h.ignored();
    let _ = h.input(&[0xB0, 0x2F, 0x40], now);
    let _ = h.input(&[0xB0, 0x0F, 0x0C, 0xB0, 0x2F, 0x40], now);
    let _ = h.input(&[0xF0, 0x00], now);
    assert_eq!(
        h.ignored(),
        n + 3,
        "a port with no zone, an unknown zone, a cut SysEx"
    );
}

// Beyond the brief: the Task 1–2 rulings applied to HUI (T1, T2 I1, T1-M4,
// T2-1), the hi/lo pair rules and release_all.

/// A switch press (`on`) or release of `port` in `zone`.
fn sw(zone: u8, port: u8, on: bool) -> [u8; 6] {
    [
        0xB0,
        0x0F,
        zone,
        0xB0,
        0x2F,
        if on { 0x40 | port } else { port },
    ]
}

/// Whether `fb` moves fader `k` (its hi CC).
fn moves_fader(fb: &[Vec<u8>], k: u8) -> bool {
    fb.iter().any(|x| x.len() == 3 && x[0] == 0xB0 && x[1] == k)
}

#[test]
fn hui_touch_up_releases_the_touched_strip_after_its_slot_empties() {
    let (mut h, now) = shown();
    let _ = h.input(&sw(7, 0, true), now);
    // The bank moved on: slot 8 now shows nothing.
    let _ = h.feedback(&view(4), 0, now);
    assert_eq!(
        h.input(&sw(7, 0, false), now),
        vec![A::Gesture(T::StripFader(StripId(8)), Phase::Release)],
        "Ruling T1: the stored strip is released"
    );
}

#[test]
fn hui_touch_down_takes_over_an_idle_hold() {
    let (mut h, now) = shown();
    // A move before the touch opens an idle hold …
    let a = h.input(&[0xB0, 0x00, 0x30, 0xB0, 0x20, 0x00], now);
    assert!(
        matches!(
            a.as_slice(),
            [A::Gesture(T::StripFader(StripId(1)), Phase::Touch(Some(_)))]
        ),
        "{a:?}"
    );
    // … which the touch takes over: no idle release ends the touched gesture.
    let _ = h.input(&sw(0, 0, true), now);
    assert!(h.tick(now + Duration::from_millis(600)).is_empty());
    assert_eq!(h.touched(), vec![T::StripFader(StripId(1))]);
}

#[test]
fn hui_idle_held_fader_gets_no_feedback_then_one_after_its_release() {
    let mut v = view(8);
    let (mut h, now) = shown();
    // An untouched move: held by its idle touch (spec §8, Ruling T2 I1).
    let _ = h.input(&[0xB0, 0x01, 0x10, 0xB0, 0x21, 0x00], now);
    v.strips[1].fader = 0.9;
    let fb = h.feedback(&v, 0, now + Duration::from_millis(200));
    assert!(!moves_fader(&fb, 0x01), "{fb:?}");
    let r = h.tick(now + Duration::from_millis(500));
    assert_eq!(r, vec![A::IdleRelease(T::StripFader(StripId(2)), now)]);
    // Back at the value it was sent before: the cache is cleared all the same.
    v.strips[1].fader = 0.5;
    let fb = h.feedback(&v, 0, now + Duration::from_millis(520));
    assert!(moves_fader(&fb, 0x01), "the motor goes back once: {fb:?}");
}

#[test]
fn hui_fader_pairs_hold_the_hi_byte_and_guard_echoes() {
    let (mut h, now) = shown();
    // A hi byte alone waits for its lo …
    assert!(h.input(&[0xB0, 0x03, 0x30], now).is_empty());
    // … and is applied alone when another hi comes first.
    match h.input(&[0xB0, 0x04, 0x20], now).as_slice() {
        [A::Gesture(T::StripFader(StripId(4)), Phase::Touch(Some(x)))] => {
            assert!((x - f64::from(0x30u16 << 7) / 16383.0).abs() < 1e-12)
        }
        other => panic!("{other:?}"),
    }
    // Its lo completes the pending one.
    match h.input(&[0xB0, 0x24, 0x00], now).as_slice() {
        [A::Gesture(T::StripFader(StripId(5)), Phase::Touch(Some(x)))] => {
            assert!((x - f64::from(0x20u16 << 7) / 16383.0).abs() < 1e-12)
        }
        other => panic!("{other:?}"),
    }
    // A lone lo uses its fader's last hi.
    match h.input(&[0xB0, 0x23, 0x05], now).as_slice() {
        [A::Gesture(T::StripFader(StripId(4)), Phase::Value(x))] => {
            assert!((x - f64::from(0x30u16 << 7 | 5) / 16383.0).abs() < 1e-12)
        }
        other => panic!("{other:?}"),
    }
    // A lone lo of a fader with no hi seen or sent is counted: see
    // `hui_release_all_drops_holds_touches_and_the_pending_hi` (since review
    // M3 the hi `shown` sent counts as seen).
    // The echo guard (Ruling 13): fader 6 was sent 0.5 = 8192 by `shown`.
    let echo = [0xB0, 0x05, 0x40, 0xB0, 0x25, 0x01]; // 8193, within 1 LSB
    let n = h.ignored();
    assert!(
        h.input(&echo, now + Duration::from_millis(50)).is_empty(),
        "an echo"
    );
    assert_eq!(h.ignored(), n, "not counted");
    assert_eq!(
        h.input(&echo, now + Duration::from_millis(100)).len(),
        1,
        "after the window, a move"
    );
}

#[test]
fn hui_release_all_drops_holds_touches_and_the_pending_hi() {
    let (mut h, now) = shown();
    let _ = h.input(&sw(0, 0, true), now);
    let _ = h.input(&[0xB0, 0x01, 0x10, 0xB0, 0x21, 0x00], now);
    let _ = h.input(&[0xB0, 0x40, 0x41], now);
    let _ = h.input(&sw(0x0E, 1, true), now);
    let _ = h.input(&[0xB0, 0x02, 0x10], now);
    let mut held = h.release_all();
    held.sort();
    let mut want = vec![
        T::StripFader(StripId(1)),
        T::StripFader(StripId(2)),
        T::StripPan(StripId(1)),
    ];
    want.sort();
    assert_eq!(
        held, want,
        "the touched fader, the idle-held fader and V-pot"
    );
    assert!(h.touched().is_empty());
    assert!(
        h.tick(now + Duration::from_secs(2)).is_empty(),
        "no idle release, no wind"
    );
    let n = h.ignored();
    assert!(
        h.input(&[0xB0, 0x22, 0x10], now).is_empty(),
        "the pending hi is gone"
    );
    assert_eq!(h.ignored(), n + 1);
    assert!(
        h.input(&sw(0x0E, 1, false), now).is_empty(),
        "the shuttle forgot its press"
    );
    // The faders it held get feedback again.
    let fb = h.feedback(&view(8), 0, now);
    assert!(moves_fader(&fb, 0x00) && moves_fader(&fb, 0x01), "{fb:?}");
}

#[test]
fn hui_rewind_holds_wind_and_a_late_release_still_skips() {
    let (mut h, now) = shown();
    let _ = h.input(&sw(0x0E, 1, true), now);
    let w = h.tick(now + Duration::from_millis(500));
    assert!(matches!(w.as_slice(), [A::Wind(q)] if *q < 0.0), "{w:?}");
    let r = h.input(&sw(0x0E, 1, false), now + Duration::from_millis(600));
    assert!(matches!(r.as_slice(), [A::Wind(q)] if *q < 0.0), "{r:?}");
    // Ruling T1-M4: held past 400 ms with no wind step yet, the release skips.
    let _ = h.input(&sw(0x0E, 1, true), now);
    let r = h.input(&sw(0x0E, 1, false), now + Duration::from_millis(450));
    assert_eq!(r, vec![A::Skip(-1)]);
}

#[test]
fn hui_displays_fit_the_unit() {
    let mut v = view(8);
    v.strips[0].name = "Kd".into();
    v.strips[2].select = true;
    v.position = "12.3.2.120".into();
    let (mut h, now) = (Hui::new(), Instant::now());
    let fb = h.feedback(&v, 0, now);
    let sysex = |cmd: u8| -> Vec<&Vec<u8>> {
        fb.iter()
            .filter(|x| x.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x05, 0x00, cmd]))
            .collect()
    };
    // Eight 4-character names, padded.
    let names = sysex(0x10);
    assert_eq!(names.len(), 8);
    assert!(names.iter().all(|x| x.len() == 13));
    assert!(has(
        &fb,
        &[
            0xF0, 0x00, 0x00, 0x66, 0x05, 0x00, 0x10, 0x00, b'K', b'd', b' ', b' ', 0xF7
        ]
    ));
    // The 40 × 2 display: 8 zones of 10, every zone exactly 10 characters.
    let zones = sysex(0x12);
    assert_eq!(zones.len(), 8);
    let mut text = vec![String::new(); 8];
    for z in zones {
        assert_eq!(z.len(), 19);
        text[usize::from(z[7])] = String::from_utf8(z[8..18].to_vec()).unwrap();
    }
    let line1 = text[..4].concat();
    let line2 = text[4..].concat();
    assert!(line1.starts_with("12.3.2.120"), "{line1:?}");
    assert!(line1.contains("Pan"), "{line1:?}");
    assert!(
        line2.starts_with("Strip 3") && line2.contains("Read"),
        "{line2:?}"
    );
    // Timecode: 8 digits, rightmost first, a dot on the digit before a separator.
    let tc = sysex(0x11);
    assert_eq!(tc.len(), 1);
    assert_eq!(
        tc[0][7..15],
        [0x00, 0x02, 0x01, 0x12, 0x13, 0x12, 0x01, 0x00]
    );
    // Nothing changed: nothing is resent, but the meters every 100 ms.
    assert!(
        h.feedback(&v, 0, now + Duration::from_millis(20))
            .is_empty()
    );
    let fb = h.feedback(&v, 0, now + Duration::from_millis(100));
    assert!(
        !fb.is_empty() && fb.iter().all(|x| x.first() == Some(&0xA0)),
        "{fb:?}"
    );
}

// Task 3 fix round 1: Ruling T3-1 and the review's Minors.

const REPLY: [u8; 3] = [0x90, 0x00, 0x7F];

#[test]
fn hui_going_offline_releases_what_it_holds_once() {
    let (mut h, now) = shown();
    let _ = h.input(&REPLY, now);
    let at = now + Duration::from_millis(2800);
    let _ = h.input(&sw(2, 0, true), at);
    let _ = h.input(&[0xB0, 0x40, 0x41], at);
    let _ = h.input(&[0xB0, 0x01, 0x10, 0xB0, 0x21, 0x00], at);
    let _ = h.input(&sw(0x0E, 2, true), at);
    // The feedback sees the timeout first; the next tick releases.
    let fb = h.feedback(&view(8), 0, now + Duration::from_millis(3000));
    assert!(!h.online());
    assert!(
        moves_fader(&fb, 0x02) && moves_fader(&fb, 0x01),
        "its faders are resent: {fb:?}"
    );
    let mut r = h.tick(now + Duration::from_millis(3001));
    r.sort_by_key(|a| format!("{a:?}"));
    let mut want = vec![
        A::Gesture(T::StripFader(StripId(3)), Phase::Release),
        A::IdleRelease(T::StripPan(StripId(1)), at),
        A::IdleRelease(T::StripFader(StripId(2)), at),
    ];
    want.sort_by_key(|a| format!("{a:?}"));
    assert_eq!(r, want, "Ruling T3-1");
    assert!(h.touched().is_empty());
    assert!(
        h.tick(now + Duration::from_millis(3600)).is_empty(),
        "once; no wind"
    );
}

#[test]
fn hui_a_tick_alone_sees_the_timeout() {
    let (mut h, now) = shown();
    let _ = h.input(&REPLY, now);
    let _ = h.input(&sw(0, 0, true), now);
    assert_eq!(
        h.tick(now + Duration::from_secs(3)),
        vec![A::Gesture(T::StripFader(StripId(1)), Phase::Release)]
    );
}

#[test]
fn hui_coming_back_online_refreshes_it() {
    let (mut h, now) = shown();
    let _ = h.input(&REPLY, now);
    let _ = h.tick(now + Duration::from_secs(3));
    assert!(!h.online());
    let _ = h.input(&REPLY, now + Duration::from_millis(3100));
    assert!(h.online());
    let fb = h.feedback(&view(8), 0, now + Duration::from_millis(3110));
    assert!(
        has(
            &fb,
            &[
                0xF0, 0x00, 0x00, 0x66, 0x05, 0x00, 0x10, 0x00, b'S', b't', b'r', b'i', 0xF7
            ]
        ),
        "{fb:?}"
    );
}

#[test]
fn hui_hello_is_the_first_ping_and_release_all_goes_offline() {
    let now = Instant::now();
    let mut h = Hui::new();
    assert_eq!(h.hello(), vec![PING.to_vec()]);
    assert!(!has(&h.feedback(&view(8), 0, now), &PING), "hello pinged");
    assert!(has(
        &h.feedback(&view(8), 0, now + Duration::from_secs(1)),
        &PING
    ));
    let _ = h.input(&REPLY, now);
    let _ = h.release_all();
    assert!(
        !h.online(),
        "a reconnected unit is online only once it answers"
    );
}

#[test]
fn hui_a_pending_hi_dies_with_its_touch_or_its_strip() {
    let (mut h, now) = shown();
    let _ = h.input(&sw(0, 0, true), now);
    let _ = h.input(&[0xB0, 0x00, 0x30], now);
    assert_eq!(
        h.input(&sw(0, 0, false), now),
        vec![A::Gesture(T::StripFader(StripId(1)), Phase::Release)]
    );
    assert!(
        h.input(&[0xB0, 0x01, 0x30], now).is_empty(),
        "nothing flushed"
    );
    // Fader 2's hi is pending; the bank moves, slot 2 shows another strip.
    let _ = h.feedback(&view(8), 1, now);
    assert!(
        h.input(&[0xB0, 0x02, 0x30], now).is_empty(),
        "nothing flushed"
    );
}

#[test]
fn hui_a_hi_alone_is_checked_for_an_echo() {
    let mut v = view(8);
    v.strips[0].fader = f64::from(0x40u16 << 7 | 0x50) / 16383.0;
    let now = Instant::now();
    let mut h = Hui::new();
    let _ = h.feedback(&v, 0, now);
    let _ = h.input(&[0xB0, 0x00, 0x40], now);
    assert!(
        h.input(&[0xB0, 0x01, 0x30], now).is_empty(),
        "the echo's hi, flushed alone"
    );
}

#[test]
fn hui_a_lone_lo_uses_the_hi_last_sent() {
    let (mut h, now) = shown();
    // `shown` sent 0.5 = 0x40 << 7 to fader 1.
    match h
        .input(&[0xB0, 0x20, 0x05], now + Duration::from_millis(200))
        .as_slice()
    {
        [A::Gesture(T::StripFader(StripId(1)), Phase::Touch(Some(x)))] => {
            assert!((x - f64::from(0x40u16 << 7 | 5) / 16383.0).abs() < 1e-12)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn hui_an_emptied_slot_meters_go_dark_once() {
    let (mut h, now) = shown();
    let fb = h.feedback(&view(4), 0, now + Duration::from_millis(100));
    assert!(
        has(&fb, &[0xA0, 0x04, 0x00]) && has(&fb, &[0xA0, 0x04, 0x10]),
        "{fb:?}"
    );
    let fb = h.feedback(&view(4), 0, now + Duration::from_millis(200));
    assert!(!has(&fb, &[0xA0, 0x04, 0x00]), "once");
}

#[test]
fn hui_a_switch_release_on_an_empty_slot_is_not_counted() {
    let (mut h, now) = shown();
    let _ = h.feedback(&view(4), 0, now);
    let n = h.ignored();
    let _ = h.input(&sw(5, 1, false), now);
    assert_eq!(h.ignored(), n);
    let _ = h.input(&sw(5, 1, true), now);
    assert_eq!(h.ignored(), n + 1, "the press is");
}
