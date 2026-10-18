//! Step entry through the session (plan 4f Task 5): chords, rests, ties,
//! taking steps back, keyboard slots, refusals and hostile states.

use proptest::prelude::*;
use vizdaw_ffi::{
    DocSnapshot, EventKindRow, NoteValueRow, Session, StepInputRow, StepKindRow, StepNoteRow,
    StepRecordRow, StepStateRow, VizError, step_length, step_parse_pitches, step_state_new,
};

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

fn id_of(s: &DocSnapshot, name: &str) -> u64 {
    s.sequences.iter().find(|r| r.name == name).unwrap().id
}

/// The demo's Song and its Chords lane, and an empty lane added after.
fn song() -> (std::sync::Arc<Session>, u64, u64) {
    let s = Session::new_demo().unwrap();
    let song = id_of(&s.snapshot().unwrap(), "Song");
    s.edit(
        vizdaw_ffi::EditCommand::Lane {
            edit: vizdaw_ffi::LaneEdit::Add {
                seq: song,
                name: "Steps".into(),
                index: None,
            },
        },
        None,
    )
    .unwrap();
    let lane = s.lanes(song).unwrap().last().unwrap().id;
    (s, song, lane)
}

fn note(pitch: u8) -> StepNoteRow {
    StepNoteRow {
        pitch,
        velocity: u16::from(pitch) << 9,
    }
}

/// Notes in the lane: (tick, pitch, duration).
fn notes(s: &Session, seq: u64, lane: u64) -> Vec<(i64, u8, i64)> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .filter(|e| e.kind == EventKindRow::Note)
        .map(|e| (e.tick, e.pitch, e.duration))
        .collect()
}

fn step(s: &Session, seq: u64, lane: u64, st: &mut StepStateRow, input: StepInputRow) -> bool {
    let r = s.step_entry(seq, lane, st.clone(), input).unwrap();
    *st = r.state;
    r.snapshot.is_some()
}

#[test]
fn a_chord_is_one_undo_step_and_becomes_the_selection() {
    let (s, song, lane) = song();
    let mut st = step_state_new(2 * BAR);
    assert_eq!(st.value, NoteValueRow::Quarter);
    for input in [
        StepInputRow::NoteOn { note: note(64) },
        StepInputRow::NoteOn { note: note(60) },
        StepInputRow::NoteOff { pitch: 60 },
    ] {
        assert!(
            !step(&s, song, lane, &mut st, input),
            "nothing until all keys are up"
        );
    }
    assert!(step(
        &s,
        song,
        lane,
        &mut st,
        StepInputRow::NoteOff { pitch: 64 }
    ));
    assert_eq!(
        notes(&s, song, lane),
        vec![(2 * BAR, 60, Q), (2 * BAR, 64, Q)]
    );
    assert_eq!(st.tick, 2 * BAR + Q);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Step Entry")
    );
    let StepKindRow::Notes { events } = &st.history[0].kind else {
        panic!("{:?}", st.history)
    };
    let mut sel = s.selection().unwrap().events;
    sel.sort_unstable();
    let mut mine = events.clone();
    mine.sort_unstable();
    assert_eq!(sel, mine, "the entered notes are selected");
    s.undo().unwrap();
    assert!(notes(&s, song, lane).is_empty(), "one undo takes the chord");
}

#[test]
fn rests_ties_and_taking_steps_back() {
    let (s, song, lane) = song();
    let mut st = step_state_new(0);
    let eighth = StepInputRow::Value {
        value: NoteValueRow::Eighth,
    };
    assert!(!step(&s, song, lane, &mut st, eighth));
    assert!(
        !step(&s, song, lane, &mut st, StepInputRow::Rest),
        "a rest touches nothing"
    );
    assert_eq!(st.tick, Q / 2);
    let chord = StepInputRow::Notes {
        notes: vec![note(67)],
    };
    assert!(step(&s, song, lane, &mut st, chord));
    assert!(step(&s, song, lane, &mut st, StepInputRow::Tie));
    assert_eq!(
        notes(&s, song, lane),
        vec![(Q / 2, 67, Q)],
        "tied: two eighths"
    );
    assert_eq!(st.tick, Q / 2 + Q);
    assert!(step(&s, song, lane, &mut st, StepInputRow::DeleteLast));
    assert_eq!(
        notes(&s, song, lane),
        vec![(Q / 2, 67, Q / 2)],
        "the tie undone"
    );
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Delete Last Step")
    );
    assert!(step(&s, song, lane, &mut st, StepInputRow::DeleteLast));
    assert!(notes(&s, song, lane).is_empty());
    assert_eq!(st.tick, Q / 2);
    let r = s
        .step_entry(song, lane, st.clone(), StepInputRow::DeleteLast)
        .unwrap();
    assert!(
        r.moved && r.snapshot.is_none(),
        "the rest goes back without an edit"
    );
    assert_eq!(r.state.tick, 0);
    let r = s
        .step_entry(song, lane, r.state, StepInputRow::DeleteLast)
        .unwrap();
    assert!(!r.moved, "nothing left");
}

#[test]
fn a_step_whose_notes_were_deleted_still_goes_back() {
    let (s, song, lane) = song();
    let mut st = step_state_new(0);
    step(
        &s,
        song,
        lane,
        &mut st,
        StepInputRow::Notes {
            notes: vec![note(60)],
        },
    );
    let StepKindRow::Notes { events } = st.history[0].kind.clone() else {
        panic!()
    };
    s.edit(
        vizdaw_ffi::EditCommand::Event {
            edit: vizdaw_ffi::EventEdit::Delete { events },
        },
        None,
    )
    .unwrap();
    let r = s
        .step_entry(song, lane, st.clone(), StepInputRow::DeleteLast)
        .unwrap();
    assert!(r.moved && r.snapshot.is_none() && r.state.tick == 0);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Delete Events")
    );
}

#[test]
fn a_keyboard_slot_enters_a_reference_and_moves_by_its_length() {
    let (s, song, lane) = song();
    let beat = id_of(&s.snapshot().unwrap(), "Beat");
    let beat_len = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.id == beat)
        .unwrap()
        .length_ticks;
    let mut st = step_state_new(BAR);
    assert!(step(
        &s,
        song,
        lane,
        &mut st,
        StepInputRow::Reference { seq: beat }
    ));
    let ev = s.lane_events(song, lane).unwrap().events;
    assert_eq!(ev.len(), 1);
    assert_eq!(
        (ev[0].kind, ev[0].tick, ev[0].ref_seq),
        (EventKindRow::Reference, BAR, Some(beat))
    );
    assert_eq!(st.tick, BAR + beat_len);
    // A reference to the sequence itself is a cycle: refused, nothing moves.
    let before = st.clone();
    let e = s
        .step_entry(
            song,
            lane,
            st.clone(),
            StepInputRow::Reference { seq: song },
        )
        .unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
    assert_eq!(st, before);
    assert_eq!(s.lane_events(song, lane).unwrap().events.len(), 1);
}

#[test]
fn refusals_leave_everything_as_it_was() {
    let (s, song, lane) = song();
    let st = step_state_new(0);
    let input = StepInputRow::Notes {
        notes: vec![note(60)],
    };
    for (seq, l) in [(9_999, lane), (song, 9_999)] {
        assert!(matches!(
            s.step_entry(seq, l, st.clone(), input.clone()),
            Err(VizError::Model { .. })
        ));
    }
    s.begin_edit("Drag".into()).unwrap();
    assert!(matches!(
        s.step_entry(song, lane, st.clone(), input.clone()),
        Err(VizError::Model { .. })
    ));
    s.abort_edit().unwrap();
    // A step past the model's time limit is refused by the core.
    let far = step_state_new(1 << 41);
    assert!(s.step_entry(song, lane, far, input).is_err());
    assert!(notes(&s, song, lane).is_empty());
}

#[test]
fn lengths_pitches_and_a_fresh_state() {
    assert_eq!(step_length(NoteValueRow::Eighth, true, false), Q * 3 / 4);
    assert_eq!(step_length(NoteValueRow::Whole, false, true), 4 * Q * 2 / 3);
    assert_eq!(
        step_parse_pitches("C4 e4 67".into()).unwrap(),
        vec![60, 64, 67]
    );
    assert!(matches!(
        step_parse_pitches("X9".into()),
        Err(VizError::Model { .. })
    ));
    let st = step_state_new(-BAR);
    assert_eq!(st.tick, -BAR);
    assert!(st.history.is_empty() && !st.dotted && !st.triplet);
}

fn any_kind() -> impl Strategy<Value = StepKindRow> {
    prop_oneof![
        prop::collection::vec(0u64..400, 0..4).prop_map(|events| StepKindRow::Notes { events }),
        Just(StepKindRow::Rest),
        (prop::collection::vec(0u64..400, 0..4), any::<i64>())
            .prop_map(|(events, by)| StepKindRow::Tie { events, by }),
        (0u64..400).prop_map(|event| StepKindRow::Reference { event }),
    ]
}

fn any_state() -> impl Strategy<Value = StepStateRow> {
    (
        -(1i64 << 42)..(1i64 << 42),
        prop::collection::vec(any::<u8>(), 0..30),
        prop::collection::vec((any::<i64>(), any::<i64>(), any_kind()), 0..300),
        any::<bool>(),
    )
        .prop_map(|(tick, held, history, dotted)| StepStateRow {
            tick,
            value: NoteValueRow::Sixteenth,
            dotted,
            triplet: !dotted,
            chord: held.iter().map(|p| note(*p & 0x7F)).collect(),
            held,
            history: history
                .into_iter()
                .map(|(tick, length, kind)| StepRecordRow { tick, length, kind })
                .collect(),
        })
}

fn any_input() -> impl Strategy<Value = StepInputRow> {
    prop_oneof![
        Just(StepInputRow::Tie),
        Just(StepInputRow::Rest),
        Just(StepInputRow::DeleteLast),
        Just(StepInputRow::Dot),
        any::<u8>().prop_map(|pitch| StepInputRow::NoteOff { pitch }),
        prop::collection::vec(any::<u8>(), 0..20).prop_map(|p| StepInputRow::Notes {
            notes: p.into_iter().map(note).collect()
        }),
        (0u64..8).prop_map(|seq| StepInputRow::Reference { seq }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// A stale or hostile state with any input is applied or refused
    /// with a `Model` error; never a panic or `Internal`, and the state
    /// that comes back is bounded.
    #[test]
    fn hostile_states_apply_or_are_refused(st in any_state(), input in any_input()) {
        let (s, song, lane) = song();
        match s.step_entry(song, lane, st, input) {
            Ok(r) => {
                prop_assert!(r.state.history.len() <= 256);
                prop_assert!(r.state.held.len() <= 16 && r.state.chord.len() <= 16);
            }
            Err(e) => prop_assert!(matches!(e, VizError::Model { .. }), "{:?}", e),
        }
        prop_assert!(s.snapshot().is_ok());
    }
}

#[test]
fn a_stale_state_never_touches_another_lanes_notes() {
    let (s, song, lane) = song();
    let chords = s.lanes(song).unwrap()[1].id;
    let before = s.lane_events(song, chords).unwrap().events;
    let theirs: Vec<u64> = before.iter().map(|e| e.id).collect();
    let mut st = step_state_new(0);
    st.history = vec![
        StepRecordRow {
            tick: 0,
            length: Q,
            kind: StepKindRow::Notes {
                events: theirs.clone(),
            },
        },
        StepRecordRow {
            tick: Q,
            length: Q,
            kind: StepKindRow::Tie {
                events: theirs,
                by: Q,
            },
        },
    ];
    let r = s.step_entry(song, lane, st, StepInputRow::Tie).unwrap();
    assert!(r.snapshot.is_none(), "nothing of this lane to tie");
    let r = s
        .step_entry(song, lane, r.state, StepInputRow::DeleteLast)
        .unwrap();
    let r = s
        .step_entry(song, lane, r.state, StepInputRow::DeleteLast)
        .unwrap();
    s.step_entry(song, lane, r.state, StepInputRow::DeleteLast)
        .unwrap();
    assert_eq!(s.lane_events(song, chords).unwrap().events, before);
}

/// Undo rewinds the document's ID counter, so the next insert reuses an
/// old step's event ID: a step's notes count only while their tick matches.
#[test]
fn a_reused_event_id_is_not_the_old_steps_note() {
    for input in [StepInputRow::Tie, StepInputRow::DeleteLast] {
        let (s, song, lane) = song();
        let mut old = step_state_new(0);
        step(
            &s,
            song,
            lane,
            &mut old,
            StepInputRow::Notes {
                notes: vec![note(60)],
            },
        );
        let StepKindRow::Notes { events: c } = old.history[0].kind.clone() else {
            panic!()
        };
        s.undo().unwrap();
        let mut fresh = step_state_new(BAR);
        step(
            &s,
            song,
            lane,
            &mut fresh,
            StepInputRow::Notes {
                notes: vec![note(62)],
            },
        );
        let StepKindRow::Notes { events: d } = fresh.history[0].kind.clone() else {
            panic!()
        };
        assert_eq!(c, d, "the ID was reused");
        let r = s.step_entry(song, lane, old, input).unwrap();
        assert!(r.snapshot.is_none(), "nothing of the old step is left");
        assert_eq!(notes(&s, song, lane), vec![(BAR, 62, Q)], "D untouched");
    }
}

/// A tie records only the notes it lengthened; with none left it is a
/// rest (final review, Task 5 M1).
#[test]
fn a_tie_records_only_the_notes_it_lengthened() {
    let (s, song, lane) = song();
    let mut st = step_state_new(0);
    step(
        &s,
        song,
        lane,
        &mut st,
        StepInputRow::Notes {
            notes: vec![note(60), note(64)],
        },
    );
    let StepKindRow::Notes { events } = st.history[0].kind.clone() else {
        panic!()
    };
    let del = |ids: Vec<u64>| {
        s.edit(
            vizdaw_ffi::EditCommand::Event {
                edit: vizdaw_ffi::EventEdit::Delete { events: ids },
            },
            None,
        )
        .unwrap();
    };
    del(vec![events[1]]);
    assert!(step(&s, song, lane, &mut st, StepInputRow::Tie));
    assert_eq!(
        st.history[1].kind,
        StepKindRow::Tie {
            events: vec![events[0]],
            by: Q
        },
        "only the note still there"
    );
    del(vec![events[0]]);
    assert!(!step(&s, song, lane, &mut st, StepInputRow::Tie));
    assert_eq!(st.history[2].kind, StepKindRow::Rest, "nothing tied");
    assert_eq!(st.tick, 3 * Q);
}
