//! Kontrol S-Series MK3 input, byte by byte (plan 5f-4 Tasks 1–2; spec
//! §8.4, §12.2).

mod common;

use common::view;
use core_surface::kontrol::{HELLO_EVERY, Kontrol, sysex};
use core_surface::{MixerView, Surface, SurfaceKind};
use std::time::{Duration, Instant};

/// The keyboard's answer: protocol version 4.
const V4: [u8; 3] = [0xBF, 0x01, 0x04];

/// A keyboard that answered the handshake and showed `v` from strip `first`.
fn online(v: &MixerView, first: usize) -> (Kontrol, Instant) {
    let now = Instant::now();
    let mut k = Kontrol::new();
    let _ = k.hello();
    assert!(k.input(&V4, now).is_empty());
    let _ = k.feedback(v, first, now);
    (k, now)
}

#[test]
fn the_handshake_asks_for_version_4_and_waits_for_the_answer() {
    let now = Instant::now();
    let mut k = Kontrol::new();
    assert_eq!(k.kind(), SurfaceKind::KontrolMk3);
    assert_eq!(k.hello(), vec![vec![0xBF, 0x01, 0x04]]);
    assert!(!k.online());
    assert!(
        k.feedback(&view(3), 0, now).is_empty(),
        "nothing before the answer"
    );
    assert!(
        k.input(&[0xBF, 0x01, 0x05], now).is_empty(),
        "a newer keyboard speaks 4 too"
    );
    assert!(k.online());
    assert_eq!(k.ignored(), 0);
    assert_eq!(k.take_problem(), None);
    let first = k.feedback(&MixerView::default(), 0, now);
    assert_eq!(
        first.first(),
        Some(&sysex(0x07, 1, 0, b"Generic")),
        "VizDAW names itself first"
    );
}

#[test]
fn a_wrong_version_is_a_problem_and_keeps_it_offline() {
    let now = Instant::now();
    let mut k = Kontrol::new();
    let _ = k.hello();
    assert!(k.input(&[0xBF, 0x01, 0x03], now).is_empty());
    assert!(!k.online());
    let p = k.take_problem().unwrap();
    assert!(p.contains("version 3") && p.contains("firmware 2.0"), "{p}");
    assert_eq!(k.take_problem(), None, "said once");
    assert_eq!(k.ignored(), 0, "the answer itself is a valid message");
    assert!(k.input(&[0xBF, 0x10, 0x01], now).is_empty());
    assert_eq!(k.ignored(), 1);
    assert!(k.input(&V4, now).is_empty());
    assert!(k.online(), "a later version-4 answer is taken");
}

/// Pre-flight M13: a version below 4 while online takes the unit offline,
/// as the keyboard's goodbye does, and says why once.
#[test]
fn a_lower_version_while_online_takes_it_offline() {
    let (mut k, now) = online(&view(2), 0);
    assert!(k.input(&[0xBF, 0x01, 0x03], now).is_empty());
    assert!(!k.online(), "as a goodbye does");
    let p = k.take_problem().unwrap();
    assert!(p.contains("version 3") && p.contains("firmware 2.0"), "{p}");
    assert_eq!(k.take_problem(), None, "said once");
    assert_eq!(k.ignored(), 0, "the answer is a valid message");
    assert!(k.input(&V4, now).is_empty());
    assert!(k.online());
}

#[test]
fn everything_before_the_answer_is_ignored_and_counted() {
    let now = Instant::now();
    let mut k = Kontrol::new();
    let _ = k.hello();
    for m in [
        vec![0xBF, 0x10, 0x01],
        vec![0xBF, 0x50, 0x01],
        sysex(0x42, 0, 0, &[]),
    ] {
        assert!(k.input(&m, now).is_empty());
    }
    assert_eq!(k.ignored(), 3);
    assert!(!k.online());
}

#[test]
fn hello_is_repeated_every_2_s_until_answered() {
    assert_eq!(HELLO_EVERY, Duration::from_secs(2));
    let now = Instant::now();
    let mut k = Kontrol::new();
    let _ = k.hello();
    assert!(k.feedback(&view(2), 0, now).is_empty());
    let early = now + HELLO_EVERY - Duration::from_millis(1);
    assert!(k.feedback(&view(2), 0, early).is_empty());
    assert_eq!(
        k.feedback(&view(2), 0, now + HELLO_EVERY),
        vec![vec![0xBF, 0x01, 0x04]]
    );
    let soon = now + HELLO_EVERY + Duration::from_millis(20);
    assert!(k.feedback(&view(2), 0, soon).is_empty());
}

/// Ruling 4 (owner, 2026-11-03: VizDAW pulls the keyboard back): after the
/// keyboard's goodbye the hello goes again 2 s later, and an answer names
/// VizDAW again.
#[test]
fn the_keyboards_goodbye_takes_it_offline_until_it_answers_again() {
    let (mut k, now) = online(&view(2), 0);
    let _ = k.input(&[0xBF, 0x02, 0x00], now);
    assert!(!k.online());
    let early = now + HELLO_EVERY - Duration::from_millis(1);
    assert!(k.feedback(&view(2), 0, early).is_empty());
    let at = now + HELLO_EVERY;
    assert_eq!(
        k.feedback(&view(2), 0, at),
        vec![vec![0xBF, 0x01, 0x04]],
        "VizDAW asks again: it pulls the keyboard back"
    );
    assert!(k.input(&V4, at).is_empty());
    assert!(k.online());
    assert_eq!(
        k.feedback(&view(2), 0, at).first(),
        Some(&sysex(0x07, 1, 0, b"Generic")),
        "named again"
    );
}

#[test]
fn goodbye_lets_the_keyboard_go() {
    let (mut k, _) = online(&view(2), 0);
    let bye = k.goodbye();
    assert_eq!(bye.last(), Some(&vec![0xBF, 0x02, 0x00]));
    assert!(!k.online());
    assert_eq!(
        k.goodbye().last(),
        Some(&vec![0xBF, 0x02, 0x00]),
        "the bytes do not depend on being online (the registry decides)"
    );
}
