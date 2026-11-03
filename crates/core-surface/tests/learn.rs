//! MIDI learn (plan 5f-1 Task 5; spec §8.2, §12.3; Rulings 7, 8, 17, 18).

use core_model::automation::AutomationTarget as T;
use core_model::ids::StripId;
use core_model::learn::{LearnKind as K, LearnMap, LearnMode as M, LearnSource};
use core_surface::learn::{CommandMap, LEARN_WINDOW, Learner, Mapper};
use core_surface::{IDLE_RELEASE, Phase, SurfaceAction as A};
use std::time::{Duration, Instant};

fn learn(msgs: &[&[u8]]) -> Option<(LearnSource, M)> {
    let now = Instant::now();
    let mut l = Learner::default();
    for m in msgs {
        l.feed("Knobs", m, now);
    }
    assert!(
        l.decide(now).is_none() || msgs.len() >= 8,
        "waits for the window"
    );
    l.decide(now + LEARN_WINDOW)
}

fn src(kind: K) -> LearnSource {
    LearnSource {
        port: "Knobs".into(),
        kind,
    }
}

#[test]
fn the_next_cc_bend_or_note_is_learnt() {
    assert_eq!(
        learn(&[&[0xB0, 7, 100]]),
        Some((src(K::Cc7 { channel: 0, cc: 7 }), M::Absolute))
    );
    assert_eq!(
        learn(&[&[0xE2, 0, 64]]),
        Some((src(K::PitchBend { channel: 2 }), M::Absolute))
    );
    assert_eq!(
        learn(&[&[0x99, 36, 90]]),
        Some((
            src(K::Note {
                channel: 9,
                note: 36
            }),
            M::Absolute
        ))
    );
    assert_eq!(
        learn(&[&[0x89, 36, 0]]),
        None,
        "a note-off alone teaches nothing"
    );
}

#[test]
fn fourteen_bit_pairs_are_detected() {
    assert_eq!(
        learn(&[
            &[0xB0, 1, 64],
            &[0xB0, 33, 5],
            &[0xB0, 1, 65],
            &[0xB0, 33, 0]
        ]),
        Some((src(K::Cc14 { channel: 0, cc: 1 }), M::Absolute))
    );
    assert_eq!(
        learn(&[&[0xB0, 40, 64], &[0xB0, 72, 5]]),
        Some((src(K::Cc7 { channel: 0, cc: 40 }), M::Absolute)),
        "no pair above CC 31"
    );
}

#[test]
fn encoder_styles_are_detected() {
    assert_eq!(
        learn(&[&[0xB0, 20, 1], &[0xB0, 20, 1], &[0xB0, 20, 127]]),
        Some((src(K::Cc7 { channel: 0, cc: 20 }), M::Relative2sComplement))
    );
    assert_eq!(
        learn(&[&[0xB0, 20, 65], &[0xB0, 20, 63], &[0xB0, 20, 66]]),
        Some((src(K::Cc7 { channel: 0, cc: 20 }), M::RelativeBinOffset))
    );
    assert_eq!(
        learn(&[&[0xB0, 20, 1]]),
        Some((src(K::Cc7 { channel: 0, cc: 20 }), M::Absolute)),
        "one message is absolute"
    );
    let now = Instant::now();
    let mut l = Learner::default();
    for _ in 0..8 {
        l.feed("Knobs", &[0xB0, 20, 65], now);
    }
    assert!(l.decide(now).is_some(), "eight messages decide at once");
}

/// A mapper with one map, pick-up off (the jump), as the tests of the
/// values need; `pick_up_waits_until_the_control_reaches_the_value` turns
/// it on.
fn mapper(mode: M, kind: K, target: T) -> Mapper {
    let mut m = Mapper::default();
    m.set(
        vec![LearnMap {
            source: src(kind),
            target,
            mode,
            pickup: false,
        }],
        vec![],
    );
    m
}

#[test]
fn absolute_maps_touch_move_and_release_after_500_ms() {
    let f = T::StripFader(StripId(3));
    let mut m = mapper(M::Absolute, K::Cc7 { channel: 0, cc: 7 }, f);
    let now = Instant::now();
    let cur = |_: T| Some(0.0);
    assert_eq!(
        m.input("Knobs", &[0xB0, 7, 127], now, &cur),
        vec![A::Gesture(f, Phase::Touch(Some(1.0)))]
    );
    assert_eq!(
        m.input("Knobs", &[0xB0, 7, 0], now, &cur),
        vec![A::Gesture(f, Phase::Value(0.0))]
    );
    assert!(m.input("Other", &[0xB0, 7, 0], now, &cur).is_empty());
    assert!(
        m.input("Knobs", &[0xB1, 7, 0], now, &cur).is_empty(),
        "another channel"
    );
    assert!(m.tick(now + Duration::from_millis(100)).is_empty());
    assert_eq!(
        m.tick(now + IDLE_RELEASE),
        vec![A::IdleRelease(f, now)],
        "stamped at its last value"
    );
}

#[test]
fn fourteen_bit_and_pitch_bend_values() {
    let f = T::StripFader(StripId(1));
    let mut m = mapper(M::Absolute, K::Cc14 { channel: 0, cc: 1 }, f);
    let now = Instant::now();
    let cur = |_: T| Some(0.0);
    let a = m.input("Knobs", &[0xB0, 1, 0x40, 0xB0, 33, 0x00], now, &cur);
    match a.as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert!((x - 8192.0 / 16383.0).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    let mut m = mapper(M::Absolute, K::PitchBend { channel: 0 }, f);
    match m.input("Knobs", &[0xE0, 0x7F, 0x7F], now, &cur).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert_eq!(*x, 1.0),
        other => panic!("{other:?}"),
    }
}

/// Pre-flight I6: messages arrive one per call, so the MSB must wait for
/// its LSB, or every move writes a coarse value before the fine one.
#[test]
fn an_msb_and_its_lsb_in_separate_calls_make_one_value() {
    let f = T::StripFader(StripId(1));
    let mut m = mapper(M::Absolute, K::Cc14 { channel: 0, cc: 1 }, f);
    let now = Instant::now();
    let cur = |_: T| Some(0.0);
    assert!(
        m.input("Knobs", &[0xB0, 1, 0x40], now, &cur).is_empty(),
        "the MSB waits for its LSB"
    );
    match m.input("Knobs", &[0xB0, 33, 0x00], now, &cur).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert!((x - 8192.0 / 16383.0).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    assert!(m.tick(now).is_empty(), "nothing is held over");
}

#[test]
fn a_lone_msb_applies_at_tick() {
    let f = T::StripFader(StripId(1));
    let mut m = mapper(M::Absolute, K::Cc14 { channel: 0, cc: 1 }, f);
    let now = Instant::now();
    let cur = |_: T| Some(0.0);
    assert!(m.input("Knobs", &[0xB0, 1, 127], now, &cur).is_empty());
    match m.tick(now).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => {
            assert!(
                (x - 127.0 * 128.0 / 16383.0).abs() < 1e-9,
                "msb << 7 over 16383, not msb / 127"
            )
        }
        other => panic!("{other:?}"),
    }
    let _ = m.input("Knobs", &[0xB0, 1, 0x40], now, &cur);
    match m.input("Knobs", &[0xB0, 33, 5], now, &cur).as_slice() {
        [A::Gesture(_, Phase::Value(x))] => {
            assert!((x - f64::from(0x40 << 7 | 5) / 16383.0).abs() < 1e-9)
        }
        other => panic!("{other:?}"),
    }
    assert!(m.input("Knobs", &[0xB0, 1, 0x41], now, &cur).is_empty());
    match m.tick(now).as_slice() {
        [A::Gesture(_, Phase::Value(x))] => {
            assert!(
                (x - f64::from(0x41 << 7 | 5) / 16383.0).abs() < 1e-9,
                "a lone MSB keeps the last LSB"
            )
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn relative_maps_step_from_the_current_value() {
    let p = T::StripPan(StripId(2));
    let now = Instant::now();
    let mut m = mapper(M::Relative2sComplement, K::Cc7 { channel: 0, cc: 20 }, p);
    match m
        .input("Knobs", &[0xB0, 20, 127], now, &|_| Some(0.5))
        .as_slice()
    {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert!((x - (0.5 - 1.0 / 127.0)).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    let mut m = mapper(M::RelativeBinOffset, K::Cc7 { channel: 0, cc: 20 }, p);
    match m
        .input("Knobs", &[0xB0, 20, 66], now, &|_| Some(0.5))
        .as_slice()
    {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert!((x - (0.5 + 2.0 / 127.0)).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    assert!(
        m.input("Knobs", &[0xB0, 20, 64], now, &|_| Some(0.5))
            .is_empty(),
        "64 is no move"
    );
}

#[test]
fn notes_toggle_switches_and_run_commands() {
    let mute = T::StripMute(StripId(4));
    let mut m = mapper(
        M::Absolute,
        K::Note {
            channel: 9,
            note: 36,
        },
        mute,
    );
    let now = Instant::now();
    assert_eq!(
        m.input("Knobs", &[0x99, 36, 100], now, &|_| Some(0.0)),
        vec![
            A::Gesture(mute, Phase::Touch(Some(1.0))),
            A::Gesture(mute, Phase::Value(1.0)),
            A::Gesture(mute, Phase::Release),
        ]
    );
    assert!(
        m.input("Knobs", &[0x89, 36, 0], now, &|_| Some(1.0))
            .is_empty()
    );
    let mut c = Mapper::default();
    c.set(
        vec![],
        vec![CommandMap {
            source: src(K::Cc7 { channel: 0, cc: 64 }),
            command: "transport.play_stop".into(),
        }],
    );
    assert!(c.listens("Knobs") && !c.listens("Other"));
    assert_eq!(
        c.input("Knobs", &[0xB0, 64, 127], now, &|_| None),
        vec![A::Command("transport.play_stop".into())]
    );
    assert!(
        c.input("Knobs", &[0xB0, 64, 120], now, &|_| None)
            .is_empty(),
        "once per rise through 64"
    );
    let _ = c.input("Knobs", &[0xB0, 64, 0], now, &|_| None);
    assert_eq!(c.input("Knobs", &[0xB0, 64, 127], now, &|_| None).len(), 1);
}

#[test]
fn absolute_maps_send_their_value_back() {
    let f = T::StripFader(StripId(3));
    let mut m = mapper(M::Absolute, K::Cc7 { channel: 2, cc: 7 }, f);
    assert_eq!(
        m.feedback(&|_| Some(1.0)),
        vec![("Knobs".to_string(), vec![0xB2, 7, 127])]
    );
    assert!(m.feedback(&|_| Some(1.0)).is_empty(), "unchanged");
    let mut r = mapper(M::RelativeBinOffset, K::Cc7 { channel: 0, cc: 20 }, f);
    assert!(
        r.feedback(&|_| Some(1.0)).is_empty(),
        "relative maps send nothing"
    );
    let _ = m.input("Knobs", &[0xB2, 7, 10], Instant::now(), &|_| Some(1.0));
    assert!(
        m.feedback(&|_| Some(0.5)).is_empty(),
        "nothing while learn holds it"
    );
}

/// Owner answer 2026-11-02 (Ruling 18): an absolute control takes over
/// only once it passes the target's value, and again after each release.
#[test]
fn pick_up_waits_until_the_control_reaches_the_value() {
    let f = T::StripFader(StripId(3));
    let mut m = Mapper::default();
    m.set(
        vec![LearnMap {
            source: src(K::Cc7 { channel: 0, cc: 7 }),
            target: f,
            mode: M::Absolute,
            pickup: true,
        }],
        vec![],
    );
    let now = Instant::now();
    let half = |_: T| Some(0.5);
    assert!(
        m.input("Knobs", &[0xB0, 7, 10], now, &half).is_empty(),
        "far below the value: no jump"
    );
    assert!(
        m.input("Knobs", &[0xB0, 7, 40], now, &half).is_empty(),
        "still below it"
    );
    match m.input("Knobs", &[0xB0, 7, 70], now, &half).as_slice() {
        [A::Gesture(t, Phase::Touch(Some(x)))] if *t == f => {
            assert!(
                (x - 70.0 / 127.0).abs() < 1e-9,
                "it crossed 0.5: it takes over"
            )
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        m.input("Knobs", &[0xB0, 7, 0], now, &half),
        vec![A::Gesture(f, Phase::Value(0.0))],
        "engaged: it follows anywhere"
    );
    assert_eq!(m.tick(now + IDLE_RELEASE), vec![A::IdleRelease(f, now)]);
    let later = now + IDLE_RELEASE * 2;
    let moved = |_: T| Some(0.8);
    assert!(
        m.input("Knobs", &[0xB0, 7, 6], later, &moved).is_empty(),
        "released: automation moved the target away"
    );
    match m.input("Knobs", &[0xB0, 7, 101], later, &moved).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => {
            assert!(
                (x - 101.0 / 127.0).abs() < 1e-9,
                "within 0.02 of 0.8: it meets the value"
            )
        }
        other => panic!("{other:?}"),
    }
    let mut jump = mapper(M::Absolute, K::Cc7 { channel: 0, cc: 7 }, f);
    assert_eq!(
        jump.input("Knobs", &[0xB0, 7, 0], now, &half),
        vec![A::Gesture(f, Phase::Touch(Some(0.0)))],
        "pick-up off: the jump"
    );
}

/// Owner answer 2026-11-02 (Ruling 17): what a map takes stays out of
/// the lanes; everything else from the same port passes.
#[test]
fn the_maps_say_which_messages_they_take() {
    let map = |kind: K, target: T| LearnMap {
        source: src(kind),
        target,
        mode: M::Absolute,
        pickup: true,
    };
    let mut m = Mapper::default();
    m.set(
        vec![
            map(K::Cc7 { channel: 0, cc: 7 }, T::StripFader(StripId(1))),
            map(K::Cc14 { channel: 1, cc: 2 }, T::StripPan(StripId(1))),
            map(K::PitchBend { channel: 2 }, T::StripFader(StripId(2))),
            map(
                K::Note {
                    channel: 9,
                    note: 36,
                },
                T::StripMute(StripId(1)),
            ),
        ],
        vec![CommandMap {
            source: src(K::Note {
                channel: 9,
                note: 37,
            }),
            command: "edit.undo".into(),
        }],
    );
    for msg in [
        &[0xB0, 7, 1][..],
        &[0xB1, 2, 1],
        &[0xB1, 34, 1],
        &[0xE2, 0, 64],
        &[0x99, 36, 100],
        &[0x89, 36, 0],
        &[0x99, 37, 1],
    ] {
        assert!(m.takes("Knobs", msg), "{msg:02x?}");
    }
    for msg in [
        &[0xB0, 8, 1][..],
        &[0xB1, 7, 1],
        &[0xE0, 0, 64],
        &[0x99, 38, 100],
        &[0x90, 36, 100],
        &[0xA9, 36, 5],
    ] {
        assert!(!m.takes("Knobs", msg), "unlearned: {msg:02x?}");
    }
    assert!(!m.takes("Other", &[0xB0, 7, 1]), "another port");
}

/// Ruling 17's re-read: `set` keeps the state of maps whose source and
/// target are unchanged, and forgets the idle holds no map drives any more
/// (their releases come from the next `tick`, Ruling T5-1).
#[test]
fn set_keeps_unchanged_maps_and_drops_undriven_holds() {
    let f = T::StripFader(StripId(3));
    let p = T::StripPan(StripId(3));
    let fader = LearnMap {
        source: src(K::Cc7 { channel: 0, cc: 7 }),
        target: f,
        mode: M::Absolute,
        pickup: true,
    };
    let pan = LearnMap {
        source: src(K::Cc7 { channel: 0, cc: 8 }),
        target: p,
        mode: M::Absolute,
        pickup: false,
    };
    let mut m = Mapper::default();
    m.set(vec![fader.clone(), pan.clone()], vec![]);
    let now = Instant::now();
    let half = |_: T| Some(0.5);
    assert_eq!(
        m.input("Knobs", &[0xB0, 7, 64], now, &half).len(),
        1,
        "engaged"
    );
    assert_eq!(m.input("Knobs", &[0xB0, 8, 0], now, &half).len(), 1);
    m.set(vec![fader.clone()], vec![]);
    assert_eq!(m.held(), vec![f], "the pan's hold is dropped");
    assert_eq!(
        m.input("Knobs", &[0xB0, 7, 0], now, &half),
        vec![A::IdleRelease(p, now), A::Gesture(f, Phase::Value(0.0))],
        "the pan's queued release, then the fader: still engaged, still held"
    );
}

/// Ruling T5-1 (review I1): `set` never drops a release. A hold no map
/// drives any more is released by the next `tick`, at its last value's time.
#[test]
fn set_releases_the_holds_it_drops_at_the_next_tick() {
    let p = T::StripPan(StripId(2));
    let mut m = mapper(M::Absolute, K::Cc7 { channel: 0, cc: 8 }, p);
    let t0 = Instant::now();
    assert_eq!(
        m.input("Knobs", &[0xB0, 8, 10], t0, &|_| Some(0.5)).len(),
        1
    );
    m.set(vec![], vec![]);
    let later = t0 + Duration::from_millis(50);
    assert_eq!(
        m.tick(later),
        vec![A::IdleRelease(p, t0)],
        "once, stamped at its last value"
    );
    assert!(m.tick(later + IDLE_RELEASE).is_empty(), "never twice");
}

/// Review M4: a focus change releases everything the learned gestures
/// hold, at their last values' times, and forgets engagement.
#[test]
fn release_all_ends_every_hold_and_engagement() {
    let f = T::StripFader(StripId(3));
    let mut m = Mapper::default();
    m.set(
        vec![LearnMap {
            source: src(K::Cc7 { channel: 0, cc: 7 }),
            target: f,
            mode: M::Absolute,
            pickup: true,
        }],
        vec![],
    );
    let t0 = Instant::now();
    let half = |_: T| Some(0.5);
    assert_eq!(m.input("Knobs", &[0xB0, 7, 64], t0, &half).len(), 1);
    assert_eq!(m.release_all(), vec![A::IdleRelease(f, t0)]);
    assert!(m.held().is_empty() && m.tick(t0 + IDLE_RELEASE).is_empty());
    assert!(
        m.input("Knobs", &[0xB0, 7, 70], t0, &half).is_empty(),
        "disengaged: 70/127 neither passes nor nears 0.5"
    );
}

/// Review M1: a spring-loaded wheel rests at the centre, so a pitch-bend
/// command fires only well above it, once per push.
#[test]
fn a_bend_command_fires_once_per_push_up_not_on_return() {
    let mut c = Mapper::default();
    c.set(
        vec![],
        vec![CommandMap {
            source: src(K::PitchBend { channel: 0 }),
            command: "edit.undo".into(),
        }],
    );
    let now = Instant::now();
    let fired = |c: &mut Mapper, lsb: u8, msb: u8| {
        c.input("Knobs", &[0xE0, lsb, msb], now, &|_| None).len()
    };
    assert_eq!(fired(&mut c, 0, 0x40), 0, "centre");
    assert_eq!(fired(&mut c, 0, 0x00), 0, "down");
    assert_eq!(fired(&mut c, 0, 0x40), 0, "back to centre: nothing");
    assert_eq!(fired(&mut c, 0x7F, 0x7F), 1, "up");
    assert_eq!(fired(&mut c, 0, 0x40), 0, "centre");
    assert_eq!(fired(&mut c, 0x7F, 0x7F), 1, "up again");
}

/// Review M2: `takes` answers for one message; a buffer of several is not
/// taken, so in doubt nothing unlearned is lost.
#[test]
fn takes_answers_for_one_message_only() {
    let m = mapper(
        M::Absolute,
        K::Cc7 { channel: 0, cc: 7 },
        T::StripFader(StripId(1)),
    );
    assert!(m.takes("Knobs", &[0xB0, 7, 1]));
    assert!(
        !m.takes("Knobs", &[0xB0, 7, 1, 0x90, 60, 100]),
        "a learned CC packed with a note"
    );
    assert!(!m.takes("Knobs", &[]));
}

/// Review M5: pick-up on 14-bit and pitch-bend maps.
#[test]
fn fourteen_bit_and_bend_maps_pick_up() {
    let f = T::StripFader(StripId(3));
    let map = |kind: K| LearnMap {
        source: src(kind),
        target: f,
        mode: M::Absolute,
        pickup: true,
    };
    let now = Instant::now();
    let half = |_: T| Some(0.5);
    let mut m = Mapper::default();
    m.set(vec![map(K::Cc14 { channel: 0, cc: 1 })], vec![]);
    assert!(
        m.input("Knobs", &[0xB0, 1, 0x10], now, &half).is_empty(),
        "held"
    );
    assert!(
        m.tick(now).is_empty(),
        "a lone MSB far from 0.5 does not jump"
    );
    assert!(
        m.input("Knobs", &[0xB0, 33, 0x7F], now, &half).is_empty(),
        "a fine update, still far"
    );
    match m
        .input("Knobs", &[0xB0, 1, 0x40, 0xB0, 33, 0], now, &half)
        .as_slice()
    {
        [A::Gesture(_, Phase::Touch(Some(x)))] => assert!((x - 8192.0 / 16383.0).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    let mut b = Mapper::default();
    b.set(vec![map(K::PitchBend { channel: 0 })], vec![]);
    assert!(
        b.input("Knobs", &[0xE0, 0, 0x10], now, &half).is_empty(),
        "far below"
    );
    match b.input("Knobs", &[0xE0, 0, 0x50], now, &half).as_slice() {
        [A::Gesture(_, Phase::Touch(Some(x)))] => {
            assert!((x - f64::from(0x50 << 7) / 16383.0).abs() < 1e-9)
        }
        other => panic!("{other:?}"),
    }
}

/// Review M5: hostile bytes are ignored, never a panic.
#[test]
fn hostile_bytes_are_ignored() {
    let bad: [&[u8]; 8] = [
        &[],
        &[0xB0],
        &[0xB0, 7],
        &[0xB0, 0x87, 1],
        &[0x07, 0x07, 0x07],
        &[0xF0, 0x01, 0x02],
        &[0xF0, 0xB0, 7, 1, 0xF7],
        &[0xFF, 0xF8, 0xF4],
    ];
    let mut l = Learner::default();
    let mut m = mapper(
        M::Absolute,
        K::Cc7 { channel: 0, cc: 7 },
        T::StripFader(StripId(1)),
    );
    let now = Instant::now();
    for b in bad {
        l.feed("Knobs", b, now);
        assert!(!m.takes("Knobs", b), "{b:02x?}");
        let _ = m.input("Knobs", b, now, &|_| Some(0.5));
    }
    let _ = l.decide(now + LEARN_WINDOW);
    let _ = m.tick(now + IDLE_RELEASE);
}

/// Review M5: `set` keeps a command map's rise state.
#[test]
fn set_keeps_a_command_maps_rise_state() {
    let cmd = CommandMap {
        source: src(K::Cc7 { channel: 0, cc: 64 }),
        command: "transport.play_stop".into(),
    };
    let mut c = Mapper::default();
    c.set(vec![], vec![cmd.clone()]);
    let now = Instant::now();
    assert_eq!(c.input("Knobs", &[0xB0, 64, 127], now, &|_| None).len(), 1);
    c.set(vec![], vec![cmd]);
    assert!(
        c.input("Knobs", &[0xB0, 64, 120], now, &|_| None)
            .is_empty(),
        "still high"
    );
}

/// A queued release goes out before any new gesture: a map learned again
/// to the same target and moved before the `tick` touches after it.
#[test]
fn a_queued_release_comes_before_a_new_touch() {
    let p = T::StripPan(StripId(2));
    let map = LearnMap {
        source: src(K::Cc7 { channel: 0, cc: 8 }),
        target: p,
        mode: M::Absolute,
        pickup: false,
    };
    let mut m = Mapper::default();
    m.set(vec![map.clone()], vec![]);
    let t0 = Instant::now();
    let _ = m.input("Knobs", &[0xB0, 8, 10], t0, &|_| Some(0.5));
    m.set(vec![], vec![]);
    m.set(vec![map], vec![]);
    let t1 = t0 + Duration::from_millis(10);
    assert_eq!(
        m.input("Knobs", &[0xB0, 8, 0], t1, &|_| Some(0.5)),
        vec![
            A::IdleRelease(p, t0),
            A::Gesture(p, Phase::Touch(Some(0.0)))
        ]
    );
    assert!(m.tick(t1).is_empty());
}
