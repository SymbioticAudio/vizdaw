//! Kontrol S-Series MK3 input, byte by byte (plan 5f-4 Tasks 1–2; spec
//! §8.4, §12.2).

#![allow(clippy::neg_multiply)]
mod common;

use common::view;
use core_model::automation::AutomationTarget as T;
use core_model::ids::{PluginInstanceId, StripId};
use core_model::mixer::ParamId;
use core_surface::kontrol::{
    HELLO_EVERY, KNOB_STEP, Kontrol, Mode, TAP_RESET, TEMPO_RUN, sysex, tempo_bpm, tempo_bytes,
};
use core_surface::{
    Held, IDLE_RELEASE, MixerView, ParamView, Phase, PluginPageView, Surface, SurfaceAction as A,
    SurfaceKind,
};
use std::time::{Duration, Instant};

fn click(t: T, v: f64) -> Vec<A> {
    vec![
        A::Gesture(t, Phase::Touch(Some(v))),
        A::Gesture(t, Phase::Value(v)),
        A::Gesture(t, Phase::Release),
    ]
}

/// A page of `n` parameters (ids 10…) of instance 5, at 0.2, in a chain of two.
fn page(n: u64) -> PluginPageView {
    PluginPageView {
        chain: vec!["Synth".into(), "Delay".into()],
        slot: 1,
        params: (0..n)
            .map(|i| ParamView {
                target: T::PluginParam(PluginInstanceId(5), ParamId(10 + i)),
                name: format!("P{i}"),
                value: 0.2,
                text: "0.20".into(),
            })
            .collect(),
        page: 0,
        pages: 2,
        page_name: "Page 1".into(),
        preset: None,
        presets: 0,
        preset_name: String::new(),
    }
}

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

#[test]
fn transport_buttons_run_registry_commands_on_press() {
    let (mut k, now) = online(&view(2), 0);
    let cmd = |s: &str| A::Command(s.into());
    for (cc, want) in [
        (0x10, vec![cmd("transport.play")]),
        (
            0x11,
            vec![cmd("transport.return_to_start"), cmd("transport.play")],
        ),
        (0x12, vec![cmd("transport.record")]),
        (0x14, vec![cmd("transport.stop")]),
        (0x16, vec![cmd("transport.loop")]),
        (0x20, vec![cmd("edit.undo")]),
        (0x21, vec![cmd("edit.redo")]),
        // The owner, 2026-11-03: Count-in and Automation toggle VizDAW's.
        (0x13, vec![A::CountIn]),
        (0x23, vec![cmd("automation.suspend")]),
    ] {
        assert_eq!(k.input(&[0xBF, cc, 0x01], now), want, "{cc:#x}");
        assert!(
            k.input(&[0xBF, cc, 0x00], now).is_empty(),
            "a release does nothing"
        );
    }
    // Tap (0x18) has its own test: one tap is no tempo yet.
    for cc in [0x15, 0x17, 0x22, 0x04, 0x32, 0x33, 0x35] {
        assert!(k.input(&[0xBF, cc, 0x01], now).is_empty(), "{cc:#x}");
    }
    assert_eq!(
        k.ignored(),
        0,
        "buttons VizDAW has no use for are not faults"
    );
}

#[test]
fn volume_and_pan_knobs_ride_the_shown_strips_and_release_after_idle() {
    assert_eq!(KNOB_STEP, 0.005);
    let (mut k, now) = online(&view(10), 2);
    let later = now + Duration::from_millis(100);
    assert_eq!(
        k.input(&[0xBF, 0x50, 0x05], now),
        vec![A::Gesture(
            T::StripFader(StripId(3)),
            Phase::Touch(Some(0.5 + 5.0 * KNOB_STEP))
        )]
    );
    assert_eq!(
        k.input(&[0xBF, 0x50, 0x7B], later),
        vec![A::Gesture(
            T::StripFader(StripId(3)),
            Phase::Value(0.5 + 5.0 * KNOB_STEP + -5.0 * KNOB_STEP)
        )]
    );
    assert_eq!(
        k.input(&[0xBF, 0x59, 0x7F], now),
        vec![A::Gesture(
            T::StripPan(StripId(4)),
            Phase::Touch(Some(0.5 + -1.0 * KNOB_STEP))
        )]
    );
    assert_eq!(
        k.input(&[0xBF, 0x57, 0x3F], now),
        vec![A::Gesture(
            T::StripFader(StripId(10)),
            Phase::Touch(Some(0.5 + 63.0 * KNOB_STEP))
        )]
    );
    for _ in 0..10 {
        let _ = k.input(&[0xBF, 0x57, 0x3F], now);
    }
    match k.input(&[0xBF, 0x57, 0x3F], now).as_slice() {
        [A::Gesture(_, Phase::Value(v))] => assert_eq!(*v, 1.0, "clamped"),
        other => panic!("{other:?}"),
    }
    assert!(k.input(&[0xBF, 0x50, 0x00], now).is_empty(), "no movement");
    assert_eq!(k.ignored(), 0);
    let rel = k.tick(later + IDLE_RELEASE);
    assert!(rel.contains(&A::IdleRelease(T::StripFader(StripId(3)), later)));
    assert!(rel.contains(&A::IdleRelease(T::StripPan(StripId(4)), now)));
    assert!(rel.contains(&A::IdleRelease(T::StripFader(StripId(10)), now)));
}

#[test]
fn track_buttons_select_mute_solo_and_arm_the_shown_strips() {
    let (mut k, now) = online(&view(4), 0);
    assert_eq!(
        k.input(&sysex(0x42, 1, 2, &[]), now),
        vec![A::Select(StripId(3))]
    );
    assert_eq!(
        k.input(&sysex(0x43, 1, 0, &[]), now),
        click(T::StripMute(StripId(1)), 1.0)
    );
    assert_eq!(
        k.input(&sysex(0x44, 1, 1, &[]), now),
        click(T::StripSolo(StripId(2)), 1.0)
    );
    assert_eq!(
        k.input(&sysex(0x45, 1, 3, &[]), now),
        vec![A::Arm(StripId(4))]
    );
    assert_eq!(k.ignored(), 0);
    assert!(
        k.input(&sysex(0x43, 1, 8, &[]), now).is_empty(),
        "no slot 8"
    );
    assert!(
        k.input(&sysex(0x42, 1, 5, &[]), now).is_empty(),
        "an empty slot"
    );
    assert_eq!(k.ignored(), 2);
}

#[test]
fn the_selected_track_takes_the_4d_encoder_and_its_mute_and_solo() {
    let mut v = view(4);
    v.strips[2].select = true;
    let (mut k, now) = online(&v, 0);
    assert_eq!(
        k.input(&[0xBF, 0x64, 0x02], now),
        vec![A::Gesture(
            T::StripFader(StripId(3)),
            Phase::Touch(Some(0.5 + 2.0 * KNOB_STEP))
        )]
    );
    assert_eq!(
        k.input(&[0xBF, 0x65, 0x7F], now),
        vec![A::Gesture(
            T::StripPan(StripId(3)),
            Phase::Touch(Some(0.5 + -1.0 * KNOB_STEP))
        )]
    );
    assert_eq!(
        k.input(&[0xBF, 0x66, 0x01], now),
        click(T::StripMute(StripId(3)), 1.0)
    );
    assert_eq!(
        k.input(&[0xBF, 0x67, 0x01], now),
        click(T::StripSolo(StripId(3)), 1.0)
    );
    // Pre-flight M6: the release (value 0) of mute and solo does nothing.
    assert!(k.input(&[0xBF, 0x66, 0x00], now).is_empty());
    assert!(k.input(&[0xBF, 0x67, 0x00], now).is_empty());
    assert_eq!(k.ignored(), 0, "a release is no fault");
    let (mut none, now) = online(&view(4), 0);
    assert!(none.input(&[0xBF, 0x64, 0x02], now).is_empty());
    assert_eq!(none.ignored(), 1, "no selected strip");
}

#[test]
fn track_navigation_selects_the_next_strip_and_the_bank_follows() {
    let mut v = view(12);
    v.strips[7].select = true;
    let (mut k, now) = online(&v, 0);
    assert_eq!(
        k.input(&[0xBF, 0x30, 0x01], now),
        vec![A::Select(StripId(9)), A::Channel(1)]
    );
    assert_eq!(
        k.input(&[0xBF, 0x30, 0x7F], now),
        vec![A::Select(StripId(7))]
    );
    let mut v = view(12);
    v.strips[4].select = true;
    let (mut k, now) = online(&v, 4);
    assert_eq!(
        k.input(&[0xBF, 0x30, 0x7F], now),
        vec![A::Select(StripId(4)), A::Channel(-1)]
    );
    let mut v = view(12);
    v.strips[11].select = true;
    let (mut k, now) = online(&v, 4);
    assert!(
        k.input(&[0xBF, 0x30, 0x01], now).is_empty(),
        "the last strip"
    );
    let (mut k, now) = online(&view(12), 4);
    assert_eq!(
        k.input(&[0xBF, 0x30, 0x01], now),
        vec![A::Select(StripId(5))],
        "none selected: the first shown"
    );
    assert_eq!(k.ignored(), 0);
}

#[test]
fn bank_buttons_move_the_bank_in_mixer_mode_and_the_page_in_plugin_mode() {
    let (mut k, now) = online(&view(20), 0);
    assert_eq!(k.mode(), Mode::Mixer);
    assert!(!k.shows_page());
    assert_eq!(k.input(&[0xBF, 0x31, 0x01], now), vec![A::Bank(1)]);
    assert_eq!(k.input(&[0xBF, 0x31, 0x7F], now), vec![A::Bank(-1)]);
    assert!(k.input(&[0xBF, 0x05, 0x01], now).is_empty());
    assert_eq!(k.mode(), Mode::Plugin);
    assert!(k.shows_page());
    assert_eq!(k.input(&[0xBF, 0x31, 0x01], now), vec![A::PluginPage(1)]);
    assert!(k.input(&[0xBF, 0x05, 0x00], now).is_empty());
    assert_eq!(k.mode(), Mode::Mixer);
    assert!(k.input(&[0xBF, 0x05, 0x09], now).is_empty());
    assert_eq!(k.ignored(), 1, "an unknown mode");
}

#[test]
fn plugin_knobs_move_the_shown_pages_parameters_in_plugin_mode() {
    let mut v = view(2);
    v.page = Some(page(3));
    let (mut k, now) = online(&v, 0);
    assert!(k.input(&[0xBF, 0x70, 0x04], now).is_empty(), "mixer mode");
    assert_eq!(k.ignored(), 1);
    let _ = k.input(&[0xBF, 0x05, 0x01], now);
    let _ = k.feedback(&v, 0, now);
    let p = |n| T::PluginParam(PluginInstanceId(5), ParamId(n));
    assert_eq!(
        k.input(&[0xBF, 0x70, 0x04], now),
        vec![A::Gesture(p(10), Phase::Touch(Some(0.2 + 4.0 * KNOB_STEP)))]
    );
    assert_eq!(
        k.input(&[0xBF, 0x72, 0x7E], now),
        vec![A::Gesture(
            p(12),
            Phase::Touch(Some(0.2 + -2.0 * KNOB_STEP))
        )]
    );
    assert!(
        k.input(&[0xBF, 0x73, 0x01], now).is_empty(),
        "knob 4 has no parameter"
    );
    assert_eq!(k.ignored(), 2);
}

#[test]
fn chain_page_and_preset_buttons_ask_the_session() {
    let mut v = view(2);
    v.page = Some(page(3));
    let (mut k, now) = online(&v, 0);
    let _ = k.input(&[0xBF, 0x05, 0x01], now);
    let _ = k.feedback(&v, 0, now);
    assert_eq!(
        k.input(&sysex(0x70, 0, 0, &[]), now),
        vec![A::PluginSlot(0)]
    );
    assert!(
        k.input(&sysex(0x70, 0, 2, &[]), now).is_empty(),
        "a chain of two"
    );
    assert_eq!(
        k.input(&sysex(0x74, 0, 1, &[]), now),
        vec![A::PluginPage(1)]
    );
    assert!(
        k.input(&sysex(0x74, 0, 0, &[]), now).is_empty(),
        "the page shown already"
    );
    assert!(
        k.input(&sysex(0x74, 0, 2, &[]), now).is_empty(),
        "two pages"
    );
    assert_eq!(k.ignored(), 2);
    assert_eq!(k.input(&[0xBF, 0x36, 0x01], now), vec![A::Preset(1)]);
    assert_eq!(k.input(&[0xBF, 0x36, 0x7F], now), vec![A::Preset(-1)]);
    let _ = k.input(&[0xBF, 0x05, 0x00], now);
    assert_eq!(
        k.input(&[0xBF, 0x36, 0x01], now),
        vec![A::Preset(1)],
        "in either mode"
    );
}

#[test]
fn the_transport_encoder_jogs() {
    let (mut k, now) = online(&view(2), 0);
    assert_eq!(k.input(&[0xBF, 0x34, 0x03], now), vec![A::Jog(3)]);
    assert_eq!(k.input(&[0xBF, 0x34, 0x7E], now), vec![A::Jog(-2)]);
}

#[test]
fn a_goodbye_from_the_keyboard_releases_its_knobs() {
    let (mut k, now) = online(&view(2), 0);
    let _ = k.input(&[0xBF, 0x50, 0x01], now);
    let later = now + Duration::from_millis(50);
    assert_eq!(
        k.input(&[0xBF, 0x02, 0x00], later),
        vec![A::IdleRelease(T::StripFader(StripId(1)), now)]
    );
    assert!(!k.online());
    assert!(k.tick(later + IDLE_RELEASE).is_empty(), "released once");
}

#[test]
fn release_all_and_release_holds_answer_the_knobs_it_holds() {
    let (mut k, now) = online(&view(2), 0);
    let _ = k.input(&[0xBF, 0x50, 0x01], now);
    assert_eq!(
        k.release_holds(),
        Held {
            touched: vec![],
            idle: vec![T::StripFader(StripId(1))]
        }
    );
    assert!(k.online(), "a focus move keeps it online");
    let _ = k.input(&[0xBF, 0x58, 0x01], now);
    assert_eq!(
        k.release_all(),
        Held {
            touched: vec![],
            idle: vec![T::StripPan(StripId(1))]
        }
    );
    assert!(!k.online(), "a disconnect: it answers the handshake again");
    assert!(k.touched().is_empty());
}

#[test]
fn strip_controls_before_any_feedback_are_ignored_and_counted() {
    let now = Instant::now();
    let mut k = Kontrol::new();
    let _ = k.hello();
    let _ = k.input(&V4, now);
    assert!(k.input(&[0xBF, 0x50, 0x01], now).is_empty());
    assert!(k.input(&sysex(0x43, 1, 0, &[]), now).is_empty());
    assert_eq!(k.ignored(), 2);
    assert_eq!(
        k.input(&[0xBF, 0x10, 0x01], now),
        vec![A::Command("transport.play".into())],
        "the transport needs no strips"
    );
    let (mut k, now) = online(&view(3), 0);
    assert!(
        k.input(&[0xBF, 0x55, 0x01], now).is_empty(),
        "slot 5 shows nothing"
    );
    assert_eq!(k.ignored(), 1);
}

/// Ruling 5 (owner, 2026-11-03): the keyboard's tempo control sets VizDAW's
/// tempo; messages less than `TEMPO_RUN` apart are one gesture.
#[test]
fn the_keyboards_tempo_sets_vizdaws_in_runs() {
    assert_eq!(TEMPO_RUN, Duration::from_millis(500));
    let (mut k, now) = online(&view(2), 0);
    let msg = |bpm: f64| sysex(0x19, 0, 0, &tempo_bytes(bpm).unwrap());
    let heard = |bpm: f64| tempo_bpm(tempo_bytes(bpm).unwrap()).unwrap();
    assert_eq!(
        k.input(&msg(100.0), now),
        vec![A::Tempo {
            bpm: 100.0,
            new_step: true
        }],
        "the first of a turn starts a step"
    );
    let soon = now + Duration::from_millis(100);
    assert_eq!(
        k.input(&msg(101.0), soon),
        vec![A::Tempo {
            bpm: heard(101.0),
            new_step: false
        }]
    );
    let later = soon + TEMPO_RUN;
    assert_eq!(
        k.input(&msg(102.0), later),
        vec![A::Tempo {
            bpm: heard(102.0),
            new_step: true
        }],
        "a pause of TEMPO_RUN starts another step"
    );
    assert_eq!(k.ignored(), 0);
    for bad in [sysex(0x19, 0, 0, &[0x00, 0x61]), sysex(0x19, 0, 0, &[0; 5])] {
        assert!(k.input(&bad, later).is_empty(), "{bad:02x?}");
    }
    assert_eq!(k.ignored(), 2, "a cut tempo and a zero length");
}

/// Ruling 5 (owner): Tap sets the tempo from the taps' intervals, the mean
/// of the last four, rounded to 0.1 BPM; a series is one gesture.
#[test]
fn tap_tempo_sets_the_tempo_from_the_taps_intervals() {
    assert_eq!(TAP_RESET, Duration::from_secs(2));
    let (mut k, now) = online(&view(2), 0);
    let at = |ms: u64| now + Duration::from_millis(ms);
    let tap = [0xBF, 0x18, 0x01];
    let t = |bpm: f64, new_step: bool| vec![A::Tempo { bpm, new_step }];
    assert!(k.input(&tap, at(0)).is_empty(), "one tap is no tempo yet");
    assert!(
        k.input(&[0xBF, 0x18, 0x00], at(100)).is_empty(),
        "a release does nothing"
    );
    assert_eq!(k.input(&tap, at(500)), t(120.0, true));
    assert_eq!(k.input(&tap, at(1000)), t(120.0, false));
    assert_eq!(
        k.input(&tap, at(1600)),
        t(112.5, false),
        "the mean of 0.5, 0.5 and 0.6 s"
    );
    for (ms, bpm) in [(2200, 109.1), (2800, 104.3), (3400, 100.0)] {
        assert_eq!(k.input(&tap, at(ms)), t(bpm, false), "the last four, {ms}");
    }
    assert!(
        k.input(&tap, at(3400 + 2100)).is_empty(),
        "more than TAP_RESET after the last: a new series"
    );
    assert_eq!(k.input(&tap, at(5900)), t(150.0, true));
    assert_eq!(k.ignored(), 0, "taps are never faults");
}

#[test]
fn hostile_kontrol_bytes_are_ignored_and_counted() {
    let (mut k, now) = online(&view(4), 0);
    let mut cut = sysex(0x48, 0, 0, b"x");
    cut.pop();
    let mut other = sysex(0x42, 1, 0, &[]);
    other[8] = 0x02;
    let cases: Vec<Vec<u8>> = vec![
        cut,
        other,
        vec![0xF0, 0x00, 0x21, 0xF7],
        sysex(0x55, 0, 0, &[]),
        sysex(0x7F, 0, 0, &[0x01, 0x00]),
        vec![0x90, 0x3C, 0x7F],
        vec![0xB0, 0x10, 0x01],
        vec![0xBF, 0x7E, 0x01],
        vec![0xBF, 0x10],
    ];
    for (n, m) in cases.iter().enumerate() {
        assert!(k.input(m, now).is_empty(), "{m:02x?}");
        assert_eq!(k.ignored(), n as u64 + 1, "{m:02x?}");
    }
    let mut x: u32 = 0x1234_5678;
    let noise: Vec<u8> = (0..20_000)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (x >> 24) as u8
        })
        .collect();
    let _ = k.input(&noise, now);
    let _ = k.feedback(&view(4), 0, now);
    let _ = k.tick(now + IDLE_RELEASE);
    assert!(k.ignored() > 9);
}

/// Final review I1: the keyboard's own tempo, then its late echo of
/// VizDAW's earlier tempo in the same batch: only the keyboard's own acts.
#[test]
fn a_late_echo_after_the_keyboards_own_tempo_acts_on_nothing() {
    let mut v = view(2);
    v.tempo = 120.0;
    let (mut k, now) = online(&v, 0);
    let msg = |bpm: f64| sysex(0x19, 0, 0, &tempo_bytes(bpm).unwrap());
    let both = [msg(100.0), msg(120.0)].concat();
    assert_eq!(
        k.input(&both, now),
        vec![A::Tempo {
            bpm: 100.0,
            new_step: true
        }],
        "the echo of 120 does not undo the turn"
    );
    assert_eq!(k.ignored(), 0);
}

/// Final review T2 m1 = T7 m1: a tap after a turn starts a step of its own.
#[test]
fn a_tap_after_a_turn_starts_a_new_step() {
    let (mut k, now) = online(&view(2), 0);
    let at = |ms: u64| now + Duration::from_millis(ms);
    let tap = [0xBF, 0x18, 0x01];
    let _ = k.input(&tap, at(0));
    assert_eq!(
        k.input(&tap, at(500)),
        vec![A::Tempo {
            bpm: 120.0,
            new_step: true
        }]
    );
    let turn = sysex(0x19, 0, 0, &tempo_bytes(130.0).unwrap());
    assert_eq!(k.input(&turn, at(700)).len(), 1);
    assert_eq!(
        k.input(&tap, at(1000)),
        vec![A::Tempo {
            bpm: 120.0,
            new_step: true
        }],
        "the tap does not amend the turn's step"
    );
}

/// Task 4 review m1 (Ruling 9): the goodbye neither forgets nor answers a
/// hold; the registry's `release_all` still gets it.
#[test]
fn goodbye_keeps_the_holds_for_the_registry() {
    let (mut k, now) = online(&view(2), 0);
    let _ = k.input(&[0xBF, 0x50, 0x05], now);
    let _ = k.goodbye();
    assert_eq!(
        k.release_all(),
        Held {
            touched: vec![],
            idle: vec![T::StripFader(StripId(1))]
        },
        "the goodbye neither forgets nor answers the hold"
    );
}

/// Final review T4 m4: going offline leaves Plug-in mode, so a keyboard
/// that comes back in Mixer mode is not shown a page.
#[test]
fn going_offline_leaves_plugin_mode() {
    let mut v = view(2);
    v.page = Some(page(2));
    for bye in ["keyboard", "goodbye", "release_all", "old version"] {
        let (mut k, now) = online(&v, 0);
        let _ = k.input(&[0xBF, 0x05, 0x01], now);
        assert!(k.shows_page(), "{bye}");
        match bye {
            "keyboard" => drop(k.input(&[0xBF, 0x02, 0x00], now)),
            "goodbye" => drop(k.goodbye()),
            "release_all" => drop(k.release_all()),
            _ => drop(k.input(&[0xBF, 0x01, 0x03], now)),
        }
        assert!(!k.online(), "{bye}");
        assert!(!k.shows_page(), "{bye}: no page while offline");
        assert_eq!(k.mode(), Mode::Mixer, "{bye}");
        let _ = k.input(&V4, now);
        let sent = k.feedback(&v, 0, now);
        assert!(sent.contains(&vec![0xBF, 0x05, 0x00]), "{bye}: {sent:02x?}");
    }
}
