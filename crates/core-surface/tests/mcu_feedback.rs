//! MCU feedback and banks (plan 5f-1 Task 2; spec §8.1, §8.2, §12.3).

mod common;

use common::view;
use core_model::automation::{AutomationMode, AutomationTarget as T};
use core_model::ids::{PluginInstanceId, StripId};
use core_model::mixer::ParamId;
use core_surface::mcu::Mcu;
use core_surface::{
    Bank, ECHO_WINDOW, IDLE_RELEASE, ParamView, Phase, Surface, SurfaceAction as A, seg,
};
use std::time::{Duration, Instant};

fn has(fb: &[Vec<u8>], m: &[u8]) -> bool {
    fb.iter().any(|x| x.as_slice() == m)
}

#[test]
fn faders_leds_and_rings_follow_the_view() {
    let mut v = view(8);
    v.strips[0].fader = 1.0;
    v.strips[1].mute = true;
    v.strips[2].solo = true;
    v.strips[3].select = true;
    v.strips[3].mode = AutomationMode::Touch;
    v.strips[0].pan = 0.0;
    v.playing = true;
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let fb = m.feedback(&v, 0, now);
    assert!(has(&fb, &[0xE0, 0x7F, 0x7F]), "strip 1 at the top");
    assert!(
        has(&fb, &[0xE1, 0x00, 0x40]),
        "0.5 → 8192 (lsb 0, msb 0x40)"
    );
    assert!(has(&fb, &[0x90, 0x11, 0x7F]), "mute 2 lit");
    assert!(has(&fb, &[0x90, 0x0A, 0x7F]), "solo 3 lit");
    assert!(has(&fb, &[0x90, 0x1B, 0x7F]), "select 4 lit");
    assert!(
        has(&fb, &[0x90, 0x4D, 0x7F]),
        "Touch lit for the selected strip"
    );
    assert!(has(&fb, &[0x90, 0x5E, 0x7F]), "play lit");
    assert!(
        has(&fb, &[0xB0, 0x30, 0x11]),
        "pan hard left: mode 1, position 1"
    );
    assert!(has(&fb, &[0xB0, 0x31, 0x16]), "centre: position 6");
    assert!(has(&fb, &[0xE8, 0x00, 0x40]), "the master on the main unit");
    let xt = Mcu::new(true).feedback(&v, 0, now);
    assert!(
        xt.iter().all(|x| x.first() != Some(&0xE8)),
        "no master on an extender"
    );
}

#[test]
fn feedback_sends_only_what_changed() {
    let mut v = view(8);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let first = m.feedback(&v, 0, now);
    assert!(first.len() > 20);
    assert!(
        m.feedback(&v, 0, now + Duration::from_millis(20))
            .is_empty()
    );
    v.strips[4].mute = true;
    assert_eq!(
        m.feedback(&v, 0, now + Duration::from_millis(40)),
        vec![vec![0x90, 0x14, 0x7F]]
    );
    m.refresh();
    assert_eq!(
        m.feedback(&v, 0, now + Duration::from_millis(60)).len(),
        first.len()
    );
}

#[test]
fn no_fader_feedback_while_touched_then_one_on_release() {
    let mut v = view(8);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let _ = m.feedback(&v, 0, now);
    let _ = m.input(&[0x90, 0x68, 0x7F], now);
    v.strips[0].fader = 0.25;
    let fb = m.feedback(&v, 0, now + Duration::from_millis(20));
    assert!(fb.iter().all(|x| x.first() != Some(&0xE0)), "{fb:?}");
    let _ = m.input(&[0x90, 0x68, 0x00], now);
    let fb = m.feedback(&v, 0, now + Duration::from_millis(40));
    assert_eq!(fb.iter().filter(|x| x.first() == Some(&0xE0)).count(), 1);
    assert!(
        has(&fb, &[0xE0, 0x00, 0x20]),
        "0.25 → 4096: lsb 0, msb 0x20"
    );
    // The master the same way (touch note 0x70).
    let _ = m.input(&[0x90, 0x70, 0x7F], now);
    v.master.as_mut().expect("the view has a master").fader = 0.25;
    let fb = m.feedback(&v, 0, now + Duration::from_millis(60));
    assert!(fb.iter().all(|x| x.first() != Some(&0xE8)), "{fb:?}");
    let _ = m.input(&[0x90, 0x70, 0x00], now);
    let fb = m.feedback(&v, 0, now + Duration::from_millis(80));
    assert_eq!(fb.iter().filter(|x| x.first() == Some(&0xE8)).count(), 1);
    assert!(has(&fb, &[0xE8, 0x00, 0x20]));
}

/// Review I1: a fader without touch sensing is treated as touched by its
/// first value and released 500 ms after its last, so it gets no feedback
/// until then, and one after.
#[test]
fn an_idle_held_fader_gets_no_feedback_until_its_release() {
    let mut v = view(8);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let _ = m.feedback(&v, 0, now);
    match m.input(&[0xE0, 0x00, 0x20], now).as_slice() {
        [A::Gesture(T::StripFader(StripId(1)), Phase::Touch(Some(_)))] => {}
        other => panic!("an untouched move: {other:?}"),
    }
    v.strips[0].fader = 0.3;
    let fb = m.feedback(&v, 0, now + Duration::from_millis(20));
    assert!(fb.iter().all(|x| x.first() != Some(&0xE0)), "held: {fb:?}");
    let at = now + IDLE_RELEASE;
    assert!(matches!(
        m.tick(at).as_slice(),
        [A::IdleRelease(T::StripFader(StripId(1)), _)]
    ));
    let fb = m.feedback(&v, 0, at + Duration::from_millis(20));
    assert_eq!(
        fb.iter().filter(|x| x.first() == Some(&0xE0)).count(),
        1,
        "{fb:?}"
    );
    // 0.3 → round(4914.9) = 4915: lsb 0x33, msb 0x26.
    assert!(has(&fb, &[0xE0, 0x33, 0x26]));
}

/// Review M2: a flood of host connection queries keeps one reply, the latest.
#[test]
fn a_flood_of_handshake_queries_keeps_one_reply() {
    let v = view(8);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    for c in 0..100u8 {
        let mut q = vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x01, 1, 2, 3, 4, 5, 6, 7];
        q.extend_from_slice(&[c, 0x20, 0x30, 0x40]);
        q.push(0xF7);
        let _ = m.input(&q, now);
    }
    let fb = m.feedback(&v, 0, now);
    let replies: Vec<_> = fb.iter().filter(|x| x.get(5) == Some(&0x02)).collect();
    assert_eq!(replies.len(), 1);
    let r = core_surface::mcu::handshake_response([99, 0x20, 0x30, 0x40]);
    assert_eq!(replies[0][13..17], r, "the latest challenge's answer");
}

#[test]
fn scribble_strips_hold_names_and_values() {
    let mut v = view(2);
    v.strips[0].name = "Lead Vocal Double".into();
    let now = Instant::now();
    let mut m = Mcu::new(true);
    let fb = m.feedback(&v, 0, now);
    let row1 = fb
        .iter()
        .find(|x| x.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x15, 0x12, 0x00]))
        .expect("row 1 for the extender");
    let text: String = row1[7..row1.len() - 1].iter().map(|b| *b as char).collect();
    assert_eq!(text.len(), 56);
    // Ruling T2-1: each strip owns a 7-character zone (8 × 7 = 56): up to 6
    // characters, cut (never wrapped), and a space.
    assert!(text.starts_with("Lead V Strip  "), "{text:?}");
    assert!(text[14..].chars().all(|c| c == ' '), "blank slots");
    let row2 = fb
        .iter()
        .find(|x| x.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x15, 0x12, 56]))
        .expect("row 2");
    let text: String = row2[7..row2.len() - 1].iter().map(|b| *b as char).collect();
    assert!(text.starts_with("  C    "), "pan centre: {text:?}");
}

#[test]
fn displays_and_meters() {
    let mut v = view(2);
    v.position = "12.3.2.120".into();
    v.strips[0].meter = (1.0, 0.5);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let fb = m.feedback(&v, 0, now);
    assert!(has(&fb, &[0xB0, 0x4B, seg('P', false)]));
    assert!(has(&fb, &[0xB0, 0x4A, seg('N', false)]));
    assert!(has(&fb, &[0xB0, 0x40, seg('0', false)]), "rightmost digit");
    assert!(has(&fb, &[0xD0, 0x0C]), "strip 1 at 0 dBFS: level 12");
    assert_eq!(seg('A', false), 0x01);
    assert_eq!(seg('7', true), 0x77);
    v.strips[0].meter = (0.0, 0.0);
    let soon = m.feedback(&v, 0, now + Duration::from_millis(100));
    assert!(
        soon.iter().all(|x| x.first() != Some(&0xD0)),
        "meters wait 300 ms"
    );
    let later = m.feedback(&v, 0, now + Duration::from_millis(310));
    assert!(has(&later, &[0xD0, 0x00]));
}

#[test]
fn banks_of_one_main_and_three_extenders() {
    let v = view(40);
    let mut bank = Bank {
        offset: 0,
        units: 4,
    };
    assert_eq!(bank.width(), 32);
    assert_eq!(
        (0..4).map(|k| bank.first(k)).collect::<Vec<_>>(),
        vec![0, 8, 16, 24]
    );
    let now = Instant::now();
    let mut xt = Mcu::new(true);
    let _ = xt.feedback(&v, bank.first(2), now);
    assert_eq!(
        xt.input(&[0x90, 0x18, 0x7F], now),
        vec![A::Select(StripId(17))],
        "the third unit's first strip"
    );
    bank.shift(&A::Bank(1), v.strips.len());
    assert_eq!(bank.offset, 8, "40 − 32: clamped");
}

#[test]
fn bank_and_channel_clamp_to_the_strips() {
    let mut b = Bank {
        offset: 0,
        units: 1,
    };
    b.shift(&A::Bank(-1), 20);
    assert_eq!(b.offset, 0);
    b.shift(&A::Bank(1), 20);
    assert_eq!(b.offset, 8);
    b.shift(&A::Bank(1), 20);
    assert_eq!(b.offset, 12);
    b.shift(&A::Channel(-1), 20);
    assert_eq!(b.offset, 11);
    b.clamp(3);
    assert_eq!(b.offset, 0, "fewer strips than the unit");
}

#[test]
fn an_empty_slot_is_blank_and_ignored() {
    let v = view(3);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let fb = m.feedback(&v, 0, now);
    assert!(
        has(&fb, &[0xE5, 0x00, 0x00]),
        "an empty slot's fader goes down"
    );
    assert!(has(&fb, &[0x90, 0x15, 0x00]), "and its mute is dark");
    let n = m.ignored();
    assert!(
        m.input(&[0x90, 0x15, 0x7F, 0xE5, 0x00, 0x40], now)
            .is_empty()
    );
    assert_eq!(m.ignored(), n + 2);
}

/// Pre-flight M18: V-pots in Plug-in drive the selected strip's first
/// insert's parameters, whose names fill row 1.
#[test]
fn plugin_mode_drives_the_selected_inserts_parameters() {
    let mut v = view(8);
    v.plugin = (0..8)
        .map(|i| ParamView {
            target: T::PluginParam(PluginInstanceId(5), ParamId(i)),
            name: format!("Prm{i}"),
            value: 0.5,
            text: format!("{i}"),
        })
        .collect();
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let _ = m.feedback(&v, 0, now);
    let _ = m.input(&[0x90, 0x2B, 0x7F], now);
    match m.input(&[0xB0, 0x12, 0x01], now).as_slice() {
        [A::Gesture(T::PluginParam(PluginInstanceId(5), ParamId(2)), Phase::Touch(Some(x)))] => {
            assert!(
                (x - 0.51).abs() < 1e-9,
                "V-pot 3: the third parameter, one detent up"
            )
        }
        other => panic!("{other:?}"),
    }
    let fb = m.feedback(&v, 0, now + Duration::from_millis(20));
    let row1 = fb
        .iter()
        .find(|x| x.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x14, 0x12, 0x00]))
        .expect("row 1 changes to the parameter names");
    let text: String = row1[7..row1.len() - 1].iter().map(|b| *b as char).collect();
    assert!(text.starts_with("Prm0   Prm1   Prm2   "), "{text:?}");
    assert_eq!(
        &text[49..],
        "Prm7   ",
        "Ruling T2-1: the eighth strip shows in its own zone"
    );
    assert!(has(&fb, &[0x90, 0x2B, 0x7F]), "Plug-in lit");
    assert!(
        has(&fb, &[0xB0, 0x4B, seg('P', false)]) && has(&fb, &[0xB0, 0x4A, seg('L', false)]),
        "PL"
    );
}

/// Pre-flight M8: a unit that echoes motor moves does not ride its own fader.
#[test]
fn an_echo_of_a_motor_move_is_ignored() {
    let v = view(8);
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let _ = m.feedback(&v, 0, now);
    let n = m.ignored();
    assert!(
        m.input(&[0xE0, 0x01, 0x40], now + Duration::from_millis(50))
            .is_empty(),
        "8193 within 1 LSB of the 8192 sent 50 ms ago: an echo"
    );
    assert_eq!(m.ignored(), n, "and not counted");
    match m
        .input(&[0xE0, 0x00, 0x40], now + Duration::from_millis(150))
        .as_slice()
    {
        [A::Gesture(T::StripFader(StripId(1)), Phase::Touch(Some(_)))] => {}
        other => panic!("after the echo window it is a move: {other:?}"),
    }
    // Review M1: "less than ECHO_WINDOW": at exactly the window it is a move.
    let mut m = Mcu::new(false);
    let _ = m.feedback(&v, 0, now);
    match m.input(&[0xE0, 0x01, 0x40], now + ECHO_WINDOW).as_slice() {
        [A::Gesture(T::StripFader(StripId(1)), Phase::Touch(Some(_)))] => {}
        other => panic!("at the window's end it is a move: {other:?}"),
    }
}

/// Ruling T1-M2: an extender cannot see the main unit (Ruling 3), so the
/// group manager passes the main unit's V-pot assignment to every extender;
/// their V-pots, rings and value row follow it.
#[test]
fn extenders_follow_the_main_units_assignment() {
    use core_surface::{Assign, SurfaceKind, new_surface};
    let v = view(32);
    let now = Instant::now();
    let bank = Bank {
        offset: 0,
        units: 4,
    };
    let mut units: Vec<Box<dyn Surface>> = vec![
        new_surface(SurfaceKind::Mcu),
        new_surface(SurfaceKind::McuXt),
        new_surface(SurfaceKind::McuXt),
        new_surface(SurfaceKind::McuXt),
    ];
    for (k, u) in units.iter_mut().enumerate() {
        let _ = u.feedback(&v, bank.first(k), now);
    }
    let _ = units[0].input(&[0x90, 0x29, 0x7F], now);
    let a = units[0].assign().expect("an MCU has a V-pot assignment");
    assert_eq!(a, Assign::Send);
    for u in &mut units[1..] {
        u.set_assign(a);
    }
    for (k, u) in units.iter_mut().enumerate().skip(1) {
        let id = StripId(bank.first(k) as u64 + 1);
        match u.input(&[0xB0, 0x10, 0x01], now).as_slice() {
            [A::Gesture(T::SendLevel(s, i), Phase::Touch(Some(x)))] => {
                assert_eq!((*s, i.0), (id, 0), "unit {k}: its first strip's first send");
                assert!((x - 0.26).abs() < 1e-9);
            }
            other => panic!("unit {k}: {other:?}"),
        }
        let fb = u.feedback(&v, bank.first(k), now + Duration::from_millis(20));
        assert!(
            has(&fb, &[0xB0, 0x30, 0x24]),
            "send 0.25: mode 2, position 4: {fb:?}"
        );
        let row2 = fb
            .iter()
            .find(|x| x.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x15, 0x12, 56]))
            .expect("row 2 shows the send levels");
        let text: String = row2[7..row2.len() - 1].iter().map(|b| *b as char).collect();
        assert!(text.starts_with("-24.1  -24.1  "), "{text:?}");
    }
}

// Task 3 fix round 1: Ruling T3-1 and the release_all parity fix.

const OFFLINE: [u8; 7] = [0xF0, 0x00, 0x00, 0x66, 0x14, 0x04, 0xF7];
const CONFIRMED: [u8; 7] = [0xF0, 0x00, 0x00, 0x66, 0x14, 0x03, 0xF7];

#[test]
fn going_offline_releases_what_the_unit_holds_once() {
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let _ = m.feedback(&view(8), 0, now);
    let _ = m.input(&[0x90, 0x68, 0x7F], now);
    let _ = m.input(&[0xB0, 0x10, 0x01], now);
    let _ = m.input(&[0x90, 0x5B, 0x7F], now);
    assert!(m.online());
    assert_eq!(
        m.input(&OFFLINE, now),
        vec![
            A::Gesture(T::StripFader(StripId(1)), Phase::Release),
            A::IdleRelease(T::StripPan(StripId(1)), now),
        ],
        "Ruling T3-1: the touched fader and the idle hold"
    );
    assert!(!m.online());
    assert!(m.touched().is_empty());
    assert!(m.input(&OFFLINE, now).is_empty(), "once");
    assert!(
        m.tick(now + Duration::from_secs(2)).is_empty(),
        "no wind, no idle release"
    );
    assert!(
        m.input(&[0x90, 0x5B, 0x00], now).is_empty(),
        "the shuttle forgot its press"
    );
}

#[test]
fn coming_back_online_refreshes_the_unit() {
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let _ = m.input(&CONFIRMED, now);
    let _ = m.feedback(&view(8), 0, now);
    let _ = m.input(&OFFLINE, now);
    let _ = m.input(&CONFIRMED, now);
    assert!(m.online());
    let fb = m.feedback(&view(8), 0, now + Duration::from_millis(20));
    assert!(
        fb.iter()
            .any(|x| x.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x14, 0x12])),
        "a full refresh: {fb:?}"
    );
    assert!(has(&fb, &[0xE0, 0x00, 0x40]), "faders too");
}

#[test]
fn release_all_resends_an_idle_held_fader() {
    let now = Instant::now();
    let mut m = Mcu::new(false);
    let _ = m.feedback(&view(8), 0, now);
    let _ = m.input(&[0xE1, 0x00, 0x10], now);
    assert_eq!(m.release_all().all(), vec![T::StripFader(StripId(2))]);
    let fb = m.feedback(&view(8), 0, now + Duration::from_millis(20));
    assert!(has(&fb, &[0xE1, 0x00, 0x40]), "the motor goes back: {fb:?}");
}

/// Final review T8 m2 and N2: a focus move releases the unit's holds but
/// keeps it online, and a hand still resting on a fader keeps the motor
/// still (spec §8.1) until it lifts; the touch-up then does nothing (no
/// hold in the new project) and the fader is sent once.
#[test]
fn release_holds_keeps_the_link_and_a_resting_hand_stops_the_motor() {
    let now = Instant::now();
    let ms = Duration::from_millis;
    let mut m = Mcu::new(false);
    let _ = m.feedback(&view(8), 0, now);
    let _ = m.input(&[0x90, 0x68, 0x7F], now);
    let _ = m.input(&[0xB0, 0x11, 0x01], now);
    assert!(m.online(), "a valid message");
    let held = m.release_holds();
    assert_eq!(held.touched, vec![T::StripFader(StripId(1))]);
    assert_eq!(held.idle, vec![T::StripPan(StripId(2))]);
    assert!(m.online(), "still online after a focus move");
    assert!(m.touched().is_empty(), "the new project holds nothing");
    let mut other = view(8);
    other.strips[0].fader = 1.0;
    let fb = m.feedback(&other, 0, now + ms(20));
    assert!(
        !fb.iter().any(|x| x.first() == Some(&0xE0)),
        "no motor under the resting hand: {fb:?}"
    );
    assert!(
        m.input(&[0x90, 0x68, 0x00], now + ms(30)).is_empty(),
        "its touch-up releases nothing"
    );
    let fb = m.feedback(&other, 0, now + ms(40));
    assert!(has(&fb, &[0xE0, 0x7F, 0x7F]), "then sent once: {fb:?}");
}
