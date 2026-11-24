//! Step entry in the app (Swift `StepEntryTests`, minus the keyboard
//! slots, and the step rows of `KeyBindingsTests`): turning it on, its
//! keys, typed and played notes, the summary and editor-state row, its
//! end when the sequence or track goes, the refusals, and the key lookup
//! with the number row reserved.

mod support;

use std::sync::Arc;

use core_control::bindings::{Chord, Keymap, OPTION};
use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};
use support::{FakeSession, demo_fake};
use vizdaw_app::commands::{self, Ran, enabled, is_on, run};
use vizdaw_app::step_entry::{
    self, NO_TRACK, OFF, PLAYING, SEQUENCE_CHANGED, STEP_COMMANDS, TRACK_GONE, TYPED_VELOCITY,
};
use vizdaw_app::{DocumentModel, Prefs};
use vizdaw_ffi::NoteInRow;
use vizdaw_ffi::VizError;
use vizdaw_ffi::control::StepEntryStateRow;
use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::step::{NoteValueRow, StepInputRow, StepNoteRow, StepResultRow};

const TEXT: &str = "A text field has the keyboard.";

fn demo() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(demo_fake());
    let doc = DocumentModel::new(f.clone()).unwrap();
    (f, doc)
}

fn go(id: &str, doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    run(id, &[], doc)
}

fn flag(name: &str, on: bool) -> Vec<(String, ArgValue)> {
    vec![(name.to_owned(), ArgValue::Flag(on))]
}

fn pitches(text: &str) -> Vec<(String, ArgValue)> {
    vec![("pitches".to_owned(), ArgValue::Text(text.to_owned()))]
}

fn refusal(r: Result<Ran, ControlError>) -> (ErrorCode, String) {
    let e = r.expect_err("expected a refusal");
    (e.code, e.message)
}

fn tick(f: &FakeSession) -> i64 {
    use vizdaw_app::DocSession;
    f.transport().unwrap().tick
}

fn inputs(f: &FakeSession) -> Vec<StepInputRow> {
    f.step_inputs().into_iter().map(|(_, _, i)| i).collect()
}

/// A new snapshot version, so the model reads the tracks again.
fn bump(doc: &mut DocumentModel) {
    let mut s = doc.snapshot().clone();
    s.version += 1;
    doc.apply(s);
}

fn note(pitch: u8, on: bool) -> NoteInRow {
    NoteInRow {
        pitch,
        velocity: if on { 0x8000 } else { 0 },
        channel: 0,
        on,
    }
}

/// The fake's answer that keeps keys held until let go.
fn holding(f: &FakeSession, state: &vizdaw_ffi::step::StepStateRow, input: &StepInputRow) {
    let mut r = state.clone();
    match input {
        StepInputRow::NoteOn { note } => r.held.push(note.pitch),
        StepInputRow::NoteOff { pitch } => r.held.retain(|p| p != pitch),
        _ => {}
    }
    f.script_step(StepResultRow {
        state: r,
        snapshot: None,
        moved: false,
    });
}

#[test]
fn step_entry_starts_on_the_selected_track_at_the_position() {
    let (f, mut m) = demo();
    f.set_transport(|t| t.tick = 61_440);
    go("step.entry", &mut m).unwrap();
    let s = m.step_entry();
    assert!(s.is_on());
    assert_eq!(
        s.target(),
        Some(LaneRef { seq: 3, lane: 30 }),
        "the first track"
    );
    let st = s.state().unwrap();
    assert_eq!((st.tick, st.value), (61_440, NoteValueRow::Quarter));
    assert_eq!(
        m.step_summary().as_deref(),
        Some("Step Entry · Groove · 1/4")
    );
    go("step.entry", &mut m).unwrap();
    assert!(!m.step_entry_on(), "toggled off");
    m.select_track(31, false);
    run("step.entry", &flag("on", true), &mut m).unwrap();
    assert_eq!(
        m.step_entry().target(),
        Some(LaneRef { seq: 3, lane: 31 }),
        "the selected track"
    );
    run("step.entry", &flag("on", true), &mut m).unwrap();
    assert!(m.step_entry_on(), "on stays on");
    run("step.entry", &flag("on", false), &mut m).unwrap();
    assert!(!m.step_entry_on());
    f.set_playing(true);
    assert_eq!(
        refusal(go("step.entry", &mut m)),
        (ErrorCode::Unavailable, PLAYING.into())
    );
    f.set_playing(false);
    m.select(None);
    assert_eq!(
        refusal(go("step.entry", &mut m)),
        (ErrorCode::Unavailable, NO_TRACK.into())
    );
}

#[test]
fn step_keys_send_inputs_and_steps_move_the_transport() {
    let (f, mut m) = demo();
    assert_eq!(
        refusal(go("step.rest", &mut m)),
        (ErrorCode::Unavailable, OFF.into())
    );
    go("step.entry", &mut m).unwrap();
    go("step.eighth", &mut m).unwrap();
    assert_eq!(
        inputs(&f).last(),
        Some(&StepInputRow::Value {
            value: NoteValueRow::Eighth
        })
    );
    assert_eq!(tick(&f), 0, "a value is no step");
    go("step.rest", &mut m).unwrap();
    assert_eq!(inputs(&f).last(), Some(&StepInputRow::Rest));
    assert_eq!(tick(&f), 7_680, "the transport follows");
    let version = m.snapshot().version;
    run("step.notes", &pitches("C4 E4"), &mut m).unwrap();
    let typed = |pitch| StepNoteRow {
        pitch,
        velocity: TYPED_VELOCITY,
    };
    assert_eq!(TYPED_VELOCITY, 100 << 9);
    assert_eq!(
        inputs(&f).last(),
        Some(&StepInputRow::Notes {
            notes: vec![typed(60), typed(64)]
        })
    );
    assert!(
        m.snapshot().version > version,
        "the new document is applied"
    );
    assert_eq!(m.undo_title(), "Undo Step Entry");
    f.set_transport(|t| t.tick = 0);
    go("step.tie", &mut m).unwrap();
    assert_eq!(
        m.step_entry().state().unwrap().tick,
        0,
        "moving the transport moves the next step"
    );
    let (code, message) = refusal(run("step.notes", &pitches("H9"), &mut m));
    assert_eq!(code, ErrorCode::Failed);
    assert!(message.contains("not a pitch"), "{message}");
    for (id, input) in [
        ("step.dot", StepInputRow::Dot),
        ("step.triplet", StepInputRow::Triplet),
        ("step.delete_last", StepInputRow::DeleteLast),
        (
            "step.whole",
            StepInputRow::Value {
                value: NoteValueRow::Whole,
            },
        ),
        (
            "step.half",
            StepInputRow::Value {
                value: NoteValueRow::Half,
            },
        ),
        (
            "step.quarter",
            StepInputRow::Value {
                value: NoteValueRow::Quarter,
            },
        ),
        (
            "step.sixteenth",
            StepInputRow::Value {
                value: NoteValueRow::Sixteenth,
            },
        ),
        (
            "step.thirty_second",
            StepInputRow::Value {
                value: NoteValueRow::ThirtySecond,
            },
        ),
    ] {
        go(id, &mut m).unwrap();
        assert_eq!(inputs(&f).last(), Some(&input), "{id}");
    }
    assert!(f.step_inputs().iter().all(|(s, l, _)| (*s, *l) == (3, 30)));
}

#[test]
fn the_summary_names_the_value_dot_and_triplet() {
    let (_f, mut m) = demo();
    assert_eq!(m.step_summary(), None, "off");
    go("step.entry", &mut m).unwrap();
    go("step.eighth", &mut m).unwrap();
    go("step.dot", &mut m).unwrap();
    assert_eq!(
        m.step_summary().as_deref(),
        Some("Step Entry · Groove · 1/8 dotted")
    );
    go("step.triplet", &mut m).unwrap();
    assert_eq!(
        m.step_summary().as_deref(),
        Some("Step Entry · Groove · 1/8 dotted triplet")
    );
    for (v, title, wire) in [
        (NoteValueRow::Whole, "1/1", "whole"),
        (NoteValueRow::Half, "1/2", "half"),
        (NoteValueRow::Quarter, "1/4", "quarter"),
        (NoteValueRow::Eighth, "1/8", "eighth"),
        (NoteValueRow::Sixteenth, "1/16", "sixteenth"),
        (NoteValueRow::ThirtySecond, "1/32", "thirty_second"),
    ] {
        assert_eq!(step_entry::title(v), title);
        assert_eq!(step_entry::wire_name(v), wire);
    }
}

#[test]
fn the_editor_state_shows_step_entry() {
    let (f, mut m) = demo();
    assert_eq!(m.step_state_row(), None);
    go("step.entry", &mut m).unwrap();
    go("step.thirty_second", &mut m).unwrap();
    go("step.rest", &mut m).unwrap();
    assert_eq!(
        m.step_state_row(),
        Some(StepEntryStateRow {
            seq: 3,
            lane: 30,
            tick: 1_920,
            value: "thirty_second".into(),
            dotted: false,
            triplet: false,
            steps: 1,
        })
    );
    f.set_transport(|t| t.tick = 61_440);
    assert_eq!(
        m.step_state_row().map(|r| r.tick),
        Some(61_440),
        "the transport's position, not the last input's"
    );
    m.select(Some(1));
    assert_eq!(m.step_state_row(), None, "ended step entry is not shown");
}

#[test]
fn a_refused_step_shows_in_the_status_line() {
    let (f, mut m) = demo();
    go("step.entry", &mut m).unwrap();
    f.fail_next(
        "step_entry",
        VizError::Model {
            message: "a reference cannot contain itself".into(),
        },
    );
    assert_eq!(
        refusal(go("step.rest", &mut m)),
        (
            ErrorCode::Failed,
            "a reference cannot contain itself".into()
        )
    );
    assert_eq!(m.status(), Some("a reference cannot contain itself"));
    assert_eq!(m.step_entry().state().unwrap().tick, 0, "the state stays");
    f.fail_next(
        "step_entry",
        VizError::Busy {
            message: "The engine is busy.".into(),
        },
    );
    assert_eq!(refusal(go("step.rest", &mut m)).0, ErrorCode::Busy);
}

#[test]
fn played_notes_go_to_step_entry() {
    let (f, mut m) = demo();
    go("step.entry", &mut m).unwrap();
    m.receive_step_notes(&[note(60, true), note(60, false)]);
    assert_eq!(
        inputs(&f),
        [
            StepInputRow::NoteOn {
                note: StepNoteRow {
                    pitch: 60,
                    velocity: 0x8000
                }
            },
            StepInputRow::NoteOff { pitch: 60 }
        ]
    );
}

#[test]
fn a_refused_note_off_does_not_stick_the_chord() {
    let (f, mut m) = demo();
    go("step.entry", &mut m).unwrap();
    let st = m.step_entry().state().unwrap().clone();
    let on = StepInputRow::NoteOn {
        note: StepNoteRow {
            pitch: 60,
            velocity: 0x8000,
        },
    };
    holding(&f, &st, &on);
    m.receive_step_notes(&[note(60, true)]);
    assert_eq!(m.step_entry().state().unwrap().held, [60]);
    // The key let go while playing: refused, and the held keys forgotten.
    f.set_playing(true);
    m.receive_step_notes(&[note(60, false)]);
    f.set_playing(false);
    let st = m.step_entry().state().unwrap();
    assert!(
        st.held.is_empty() && st.chord.is_empty(),
        "the stale key was let go"
    );
}

#[test]
fn a_note_off_refused_for_any_reason_does_not_stick_the_chord() {
    let (f, mut m) = demo();
    go("step.entry", &mut m).unwrap();
    let st = m.step_entry().state().unwrap().clone();
    let on = StepInputRow::NoteOn {
        note: StepNoteRow {
            pitch: 60,
            velocity: 0x8000,
        },
    };
    holding(&f, &st, &on);
    m.receive_step_notes(&[note(60, true)]);
    f.fail_next(
        "step_entry",
        VizError::Model {
            message: "busy".into(),
        },
    );
    m.receive_step_notes(&[note(60, false)]);
    let st = m.step_entry().state().unwrap();
    assert!(
        st.held.is_empty() && st.chord.is_empty(),
        "the stale key was let go"
    );
    // Other refused inputs keep the held keys.
    let mut held = st.clone();
    held.held = vec![64];
    f.script_step(StepResultRow {
        state: held,
        snapshot: None,
        moved: false,
    });
    m.receive_step_notes(&[note(64, true)]);
    f.fail_next(
        "step_entry",
        VizError::Model {
            message: "no".into(),
        },
    );
    let _ = go("step.rest", &mut m);
    assert_eq!(m.step_entry().state().unwrap().held, [64]);
}

#[test]
fn toggling_step_entry_forgets_a_half_held_chord() {
    let (f, mut m) = demo();
    go("step.entry", &mut m).unwrap();
    let st = m.step_entry().state().unwrap().clone();
    let on = StepInputRow::NoteOn {
        note: StepNoteRow {
            pitch: 60,
            velocity: 0x8000,
        },
    };
    holding(&f, &st, &on);
    m.receive_step_notes(&[note(60, true)]);
    assert!(!m.step_entry().state().unwrap().held.is_empty());
    go("step.entry", &mut m).unwrap();
    go("step.entry", &mut m).unwrap();
    let st = m.step_entry().state().unwrap();
    assert!(st.held.is_empty() && st.chord.is_empty());
}

#[test]
fn check_marks_and_enablement_follow_the_state() {
    let (_f, mut m) = demo();
    assert!(!is_on("step.entry", &m) && !enabled("step.rest", &m));
    assert!(enabled("step.entry", &m));
    go("step.entry", &mut m).unwrap();
    assert!(is_on("step.entry", &m));
    assert!(is_on("step.quarter", &m) && !is_on("step.eighth", &m));
    assert!(enabled("step.rest", &m) && !is_on("step.dot", &m));
    go("step.dot", &mut m).unwrap();
    go("step.triplet", &mut m).unwrap();
    assert!(is_on("step.dot", &m) && is_on("step.triplet", &m));
    for id in STEP_COMMANDS {
        assert!(enabled(id, &m), "{id}");
    }
}

#[test]
fn step_entry_ends_at_once_when_another_sequence_is_shown() {
    let (f, mut m) = demo();
    go("step.entry", &mut m).unwrap();
    m.select(Some(1));
    assert!(!m.step_entry_on());
    assert_eq!(m.status(), Some(SEQUENCE_CHANGED), "said when it ends");
    assert!(!is_on("step.entry", &m), "the menu's check mark is off");
    let before = f.step_inputs().len();
    assert_eq!(
        refusal(go("step.rest", &mut m)),
        (ErrorCode::Unavailable, OFF.into())
    );
    assert_eq!(f.step_inputs().len(), before);
    go("step.entry", &mut m).unwrap();
    assert_eq!(
        m.step_entry().target().map(|t| t.seq),
        Some(1),
        "one T starts it in the shown sequence"
    );
}

#[test]
fn step_entry_ends_at_once_when_its_track_is_gone() {
    let (f, mut m) = demo();
    go("step.entry", &mut m).unwrap();
    f.set_lanes(3, vec![support::lane_row(31, "Chords", false)]);
    bump(&mut m);
    assert!(!m.step_entry_on());
    assert_eq!(m.status(), Some(TRACK_GONE), "said when it ends");
    let before = f.step_inputs().len();
    assert!(go("step.rest", &mut m).is_err());
    assert_eq!(f.step_inputs().len(), before);
}

/// Record is refused while step entry is on (the model, not a flag).
#[test]
fn record_is_refused_during_step_entry() {
    let (f, mut m) = demo();
    go("step.entry", &mut m).unwrap();
    assert_eq!(
        refusal(go("transport.record", &mut m)),
        (ErrorCode::Failed, vizdaw_app::record::STEP_ENTRY_ON.into())
    );
    assert_eq!(f.count("record(3)"), 0);
}

/// Swift M-3: while playing, a step key is refused and used up (Delete
/// must not reach the editor and delete the selection).
#[test]
fn step_keys_while_playing_are_used_up() {
    let (f, mut m) = demo();
    go("step.entry", &mut m).unwrap();
    f.set_playing(true);
    let before = f.step_inputs().len();
    assert_eq!(
        refusal(commands::perform_from_menu("step.delete_last", &mut m)),
        (ErrorCode::Failed, PLAYING.into())
    );
    assert_eq!(m.status(), Some(PLAYING));
    assert_eq!(f.step_inputs().len(), before);
}

/// Swift M-5: with a text field typed in, the step commands wait
/// (step.notes, the channel's alone, still runs).
#[test]
fn step_keys_wait_while_a_text_field_has_the_keyboard() {
    let (f, mut m) = demo();
    m.begin_text_editing("sheet");
    assert_eq!(
        refusal(go("step.entry", &mut m)),
        (ErrorCode::Unavailable, TEXT.into())
    );
    assert!(!m.step_entry_on() && !enabled("step.entry", &m));
    m.end_text_editing("sheet");
    go("step.entry", &mut m).unwrap();
    m.begin_text_editing("sheet");
    let before = f.step_inputs().len();
    assert_eq!(
        refusal(go("step.delete_last", &mut m)),
        (ErrorCode::Unavailable, TEXT.into())
    );
    assert_eq!(
        refusal(go("step.entry", &mut m)),
        (ErrorCode::Unavailable, TEXT.into())
    );
    assert!(m.step_entry_on(), "T does not end it behind the sheet");
    assert!(!enabled("step.rest", &m));
    assert_eq!(f.step_inputs().len(), before);
    run("step.notes", &pitches("C4"), &mut m).unwrap();
    assert_eq!(f.step_inputs().len(), before + 1);
}

/// Swift item 7: the engine's state is read before the playing check,
/// and the menu's enablement matches what running would do.
#[test]
fn step_entry_reads_the_transport_first_and_the_menu_matches() {
    let (f, mut m) = demo();
    f.set_playing(true);
    assert_eq!(
        refusal(go("step.entry", &mut m)),
        (ErrorCode::Unavailable, PLAYING.into()),
        "not the stale cache"
    );
    assert!(!enabled("step.entry", &m));
    f.set_playing(false);
    let _ = m.poll_transport();
    go("step.entry", &mut m).unwrap();
    f.set_playing(true);
    let before = f.step_inputs().len();
    assert_eq!(
        refusal(go("step.rest", &mut m)),
        (ErrorCode::Failed, PLAYING.into())
    );
    assert_eq!(f.step_inputs().len(), before);
    assert!(
        !enabled("step.rest", &m),
        "step inputs are off while playing"
    );
    f.set_playing(false);
    let _ = m.poll_transport();
    go("step.entry", &mut m).unwrap();
    f.set_lanes(3, vec![]);
    bump(&mut m);
    assert!(!enabled("step.entry", &m), "no track to enter into");
    assert!(go("step.entry", &mut m).is_err());
}

/// Only rest, tie and Delete Last Step repeat while held (Swift
/// `KeyDispatch.repeating`, the step rows).
#[test]
fn only_rest_tie_and_delete_last_repeat() {
    for id in STEP_COMMANDS {
        let repeats = matches!(*id, "step.rest" | "step.tie" | "step.delete_last");
        assert_eq!(step_entry::repeats(id), repeats, "{id}");
    }
}

/// Swift `stepKeysWinOnlyDuringStepEntry` and the reserve rows of
/// `KeyBindingsTests`: step entry's keys first while it is on, the number
/// row the locators' when reserved.
#[test]
fn step_keys_win_only_during_step_entry() {
    let keys = Keymap::default();
    let one = Chord::of_slot("1", 0).unwrap();
    let dot = Chord::of_slot(".", 0).unwrap();
    let alt2 = Chord::of_slot("2", OPTION).unwrap();
    assert_eq!(keys.lookup(one, false, false), Some("locator.go_1"));
    assert_eq!(keys.lookup(one, true, false), Some("step.whole"));
    assert_eq!(
        keys.lookup(dot, false, false),
        Some("transport.next_marker")
    );
    assert_eq!(keys.lookup(dot, true, false), Some("step.dot"));
    assert_eq!(keys.lookup(one, true, true), Some("locator.go_1"));
    assert_eq!(
        keys.lookup(dot, true, true),
        Some("step.dot"),
        "only the number row"
    );
    assert_eq!(keys.lookup(alt2, true, false), Some("locator.set_2"));
}

/// The reserve-number-row preference: off by default, kept in the file.
#[test]
fn reserve_number_row_is_a_preference() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.json");
    let mut p = Prefs::at(&path);
    assert!(!p.reserve_number_row());
    p.set_reserve_number_row(true).unwrap();
    assert!(Prefs::at(&path).reserve_number_row());
    assert!(!Prefs::in_memory().reserve_number_row());
}
