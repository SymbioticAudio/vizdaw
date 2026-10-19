//! Step entry's state machine (plan 4f Task 4).

use core_editor::step::{
    Effect, MAX_CHORD, MAX_HISTORY, NoteValue, StepInput, StepKind, StepRecord, StepState, commit,
    parse_pitches, press, step_length,
};
use core_model::ids::{EventId, SeqId};
use core_model::time::Tick;
use proptest::prelude::*;

const Q: i64 = 15_360;

fn ids(n: &[u64]) -> Vec<EventId> {
    n.iter().copied().map(EventId).collect()
}

/// Runs `inputs`, committing each effect with the next IDs from `next`.
fn run(mut s: StepState, inputs: Vec<StepInput>, next: &mut u64) -> (StepState, Vec<Effect>) {
    let mut effects = Vec::new();
    for i in inputs {
        let (t, e) = press(&s, i);
        let created: Vec<EventId> = match &e {
            Effect::Insert { notes, .. } => notes
                .iter()
                .map(|_| {
                    *next += 1;
                    EventId(*next)
                })
                .collect(),
            Effect::InsertReference { .. } => {
                *next += 1;
                vec![EventId(*next)]
            }
            _ => vec![],
        };
        s = commit(&t, &e, &created);
        effects.push(e);
    }
    (s, effects)
}

#[test]
fn values_dots_and_triplets_give_the_length() {
    use NoteValue::*;
    let table = [
        (Whole, 4 * Q),
        (Half, 2 * Q),
        (Quarter, Q),
        (Eighth, Q / 2),
        (Sixteenth, Q / 4),
        (ThirtySecond, Q / 8),
    ];
    for (v, t) in table {
        assert_eq!(step_length(v, false, false), Tick(t), "{v:?}");
        assert_eq!(step_length(v, true, false), Tick(t * 3 / 2), "dotted {v:?}");
        assert_eq!(
            step_length(v, false, true),
            Tick(t * 2 / 3),
            "triplet {v:?}"
        );
        assert_eq!(step_length(v, true, true), Tick(t), "both {v:?}");
    }
    let mut s = StepState::new(Tick(0));
    assert_eq!(s.length(), Tick(Q), "quarter notes to start");
    s = press(&s, StepInput::Value(NoteValue::Eighth)).0;
    s = press(&s, StepInput::Dot).0;
    assert_eq!(s.length(), Tick(Q * 3 / 4));
    s = press(&s, StepInput::Value(NoteValue::Half)).0;
    assert!(s.dotted, "dot and triplet stay until toggled");
    s = press(&s, StepInput::Dot).0;
    s = press(&s, StepInput::Triplet).0;
    assert_eq!(s.length(), Tick(2 * Q * 2 / 3));
}

#[test]
fn a_chord_is_every_key_down_until_the_last_is_released() {
    let s = StepState::new(Tick(Q));
    let on = |p: u8| StepInput::NoteOn {
        pitch: p,
        velocity: u16::from(p) << 8,
    };
    let off = |p: u8| StepInput::NoteOff { pitch: p };
    let mut next = 0;
    let (s, effects) = run(
        s,
        vec![
            off(50),
            on(64),
            on(60),
            on(60),
            off(64),
            on(67),
            off(60),
            off(67),
        ],
        &mut next,
    );
    assert!(
        effects[..7].iter().all(|e| *e == Effect::Nothing),
        "{effects:?}"
    );
    assert_eq!(
        effects[7],
        Effect::Insert {
            tick: Tick(Q),
            length: Tick(Q),
            notes: vec![(60, 60 << 8), (64, 64 << 8), (67, 67 << 8)],
        },
        "by pitch, each once, with its own velocity"
    );
    assert_eq!(s.tick, Tick(2 * Q), "moved on by the step");
    assert!(s.held.is_empty() && s.chord.is_empty());
    assert_eq!(
        s.history,
        vec![StepRecord {
            tick: Tick(Q),
            length: Tick(Q),
            kind: StepKind::Notes(ids(&[1, 2, 3])),
        }]
    );
    // Pitches above 127 are masked; a chord never holds more than 16.
    let mut many = StepState::new(Tick(0));
    for p in 0..40u8 {
        many = press(&many, on(p.wrapping_add(200))).0;
    }
    assert_eq!(many.chord.len(), MAX_CHORD);
    assert!(many.chord.iter().all(|(p, _)| *p < 128));
}

#[test]
fn rest_and_tie_move_on_and_a_tie_lengthens_the_last_notes() {
    let mut next = 0;
    let s = StepState::new(Tick(0));
    let (s, effects) = run(
        s,
        vec![
            StepInput::Tie,
            StepInput::Notes(vec![(62, 100), (62, 90)]),
            StepInput::Value(NoteValue::Eighth),
            StepInput::Tie,
            StepInput::Tie,
            StepInput::Rest,
            StepInput::Tie,
        ],
        &mut next,
    );
    assert_eq!(
        effects[0],
        Effect::Rest { length: Tick(Q) },
        "a tie with nothing to tie rests"
    );
    assert_eq!(
        effects[1],
        Effect::Insert {
            tick: Tick(Q),
            length: Tick(Q),
            notes: vec![(62, 100)]
        }
    );
    assert_eq!(
        effects[3],
        Effect::Extend {
            notes: ids(&[1]),
            by: Tick(Q / 2)
        }
    );
    assert_eq!(
        effects[4], effects[3],
        "a second tie lengthens the same notes"
    );
    assert_eq!(
        effects[6],
        Effect::Rest {
            length: Tick(Q / 2)
        },
        "after a rest, a tie rests"
    );
    assert_eq!(s.tick, Tick(Q + Q + Q / 2 * 4));
}

#[test]
fn delete_last_takes_each_kind_of_step_back() {
    let mut next = 0;
    let start = StepState::new(Tick(2 * Q));
    let (s, _) = run(
        start.clone(),
        vec![
            StepInput::Notes(vec![(60, 1), (64, 1)]),
            StepInput::Tie,
            StepInput::Rest,
            StepInput::Reference {
                seq: SeqId(7),
                length: Tick(8 * Q),
            },
        ],
        &mut next,
    );
    assert_eq!(s.tick, Tick(2 * Q + Q + Q + Q + 8 * Q));
    let back = |s: &StepState| press(s, StepInput::DeleteLast).1;
    assert_eq!(
        back(&s),
        Effect::TakeBack {
            delete: ids(&[3]),
            shorten: vec![],
            by: Tick(0),
            to: Tick(5 * Q)
        },
        "the reference"
    );
    let s = commit(&s, &back(&s), &[]);
    assert_eq!(s.tick, Tick(5 * Q));
    assert_eq!(
        back(&s),
        Effect::TakeBack {
            delete: vec![],
            shorten: vec![],
            by: Tick(0),
            to: Tick(4 * Q)
        },
        "the rest"
    );
    let s = commit(&s, &back(&s), &[]);
    assert_eq!(
        back(&s),
        Effect::TakeBack {
            delete: vec![],
            shorten: ids(&[1, 2]),
            by: Tick(Q),
            to: Tick(3 * Q)
        },
        "the tie"
    );
    let s = commit(&s, &back(&s), &[]);
    assert_eq!(
        back(&s),
        Effect::TakeBack {
            delete: ids(&[1, 2]),
            shorten: vec![],
            by: Tick(0),
            to: Tick(2 * Q)
        },
        "the notes"
    );
    let s = commit(&s, &back(&s), &[]);
    assert_eq!(s.tick, start.tick);
    assert!(s.history.is_empty());
    assert_eq!(back(&s), Effect::Nothing, "nothing left to take back");
}

#[test]
fn a_keyboard_slot_enters_a_reference_and_moves_by_its_length() {
    let s = StepState::new(Tick(Q));
    let (t, e) = press(
        &s,
        StepInput::Reference {
            seq: SeqId(4),
            length: Tick(8 * Q),
        },
    );
    assert_eq!(
        e,
        Effect::InsertReference {
            tick: Tick(Q),
            seq: SeqId(4),
            length: Tick(8 * Q)
        }
    );
    let after = commit(&t, &e, &ids(&[9]));
    assert_eq!(after.tick, Tick(9 * Q));
    assert_eq!(after.history[0].kind, StepKind::Reference(EventId(9)));
    let (_, neg) = press(
        &s,
        StepInput::Reference {
            seq: SeqId(4),
            length: Tick(-5),
        },
    );
    assert!(matches!(
        neg,
        Effect::InsertReference {
            length: Tick(0),
            ..
        }
    ));
}

#[test]
fn history_keeps_the_newest_steps() {
    let mut s = StepState::new(Tick(0));
    for _ in 0..MAX_HISTORY + 10 {
        let (t, e) = press(&s, StepInput::Rest);
        s = commit(&t, &e, &[]);
    }
    assert_eq!(s.history.len(), MAX_HISTORY);
    assert_eq!(s.history[0].tick, Tick(10 * Q), "the oldest went");
}

#[test]
fn typed_pitches_are_numbers_or_names() {
    assert_eq!(parse_pitches("60 64 67"), Ok(vec![60, 64, 67]));
    assert_eq!(parse_pitches(" C4, E4,G4 "), Ok(vec![60, 64, 67]));
    assert_eq!(
        parse_pitches("c#4 Eb3 B-1 G9 C-1"),
        Ok(vec![61, 51, 11, 127, 0])
    );
    for bad in [
        "", " , ", "128", "G#9", "Cb-1", "H4", "C", "C#", "60x", "4C", "C4.5",
    ] {
        assert!(parse_pitches(bad).is_err(), "{bad:?}");
    }
    assert!(parse_pitches(&"60 ".repeat(17)).is_err(), "more than 16");
    assert_eq!(parse_pitches(&"60 ".repeat(16)).map(|v| v.len()), Ok(16));
    assert!(parse_pitches(&"6".repeat(300)).is_err(), "too long");
}

fn any_input() -> impl Strategy<Value = StepInput> {
    use NoteValue::*;
    prop_oneof![
        prop_oneof![
            Just(Whole),
            Just(Half),
            Just(Quarter),
            Just(Eighth),
            Just(Sixteenth),
            Just(ThirtySecond)
        ]
        .prop_map(StepInput::Value),
        Just(StepInput::Dot),
        Just(StepInput::Triplet),
        Just(StepInput::Tie),
        Just(StepInput::Rest),
        Just(StepInput::DeleteLast),
        (any::<u8>(), any::<u16>())
            .prop_map(|(pitch, velocity)| StepInput::NoteOn { pitch, velocity }),
        any::<u8>().prop_map(|pitch| StepInput::NoteOff { pitch }),
        prop::collection::vec((any::<u8>(), any::<u16>()), 0..40).prop_map(StepInput::Notes),
        (any::<u64>(), any::<i64>()).prop_map(|(s, l)| StepInput::Reference {
            seq: SeqId(s),
            length: Tick(l)
        }),
    ]
}

proptest! {
    /// Any input sequence, from any position, keeps the bounds; an
    /// entered step taken back returns to where it began.
    #[test]
    fn hostile_inputs_keep_the_bounds(
        tick in any::<i64>(),
        inputs in prop::collection::vec(any_input(), 0..60),
        notes in prop::collection::vec((any::<u8>(), any::<u16>()), 1..20),
    ) {
        let mut next = 0;
        let (s, effects) = run(StepState::new(Tick(tick)), inputs, &mut next);
        prop_assert!(s.history.len() <= MAX_HISTORY);
        prop_assert!(s.held.len() <= MAX_CHORD && s.chord.len() <= MAX_CHORD);
        for e in effects {
            if let Effect::Insert { notes, .. } = e {
                prop_assert!(!notes.is_empty() && notes.len() <= MAX_CHORD);
                prop_assert!(notes.windows(2).all(|w| w[0].0 < w[1].0));
            }
        }
        let before = s.clone();
        let (s2, _) = run(s, vec![StepInput::Notes(notes), StepInput::DeleteLast], &mut next);
        prop_assert_eq!(s2.tick, before.tick);
        if before.history.len() < MAX_HISTORY {
            prop_assert_eq!(s2.history, before.history);
        }
    }

    /// Typed pitches never panic; every note's name reads back as it.
    #[test]
    fn typed_pitches_never_panic(text in ".{0,40}", n in 0u8..128) {
        let _ = parse_pitches(&text);
        let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
        let name = format!("{}{}", names[usize::from(n % 12)], i32::from(n / 12) - 1);
        prop_assert_eq!(parse_pitches(&name), Ok(vec![n]));
    }
}

#[test]
fn a_huge_octave_is_refused_not_a_panic() {
    for bad in ["C2147483647", "B-2147483648", "C-2147483648", "G2147483646"] {
        assert!(parse_pitches(bad).is_err(), "{bad:?}");
    }
}
