//! Nudge Earlier, Later, Up and Down (slice 10b Task 9; Swift
//! `T/NudgeTests.swift`, `M/Nudge.swift:49-178` and the arm of
//! `M/ControlCommands.swift:640-648`).

mod support;

use std::sync::Arc;

use core_control::ErrorCode;
use support::{FakeSession, four_four, note_row, roll_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::commands::{self, EDITOR_COMMANDS};
use vizdaw_app::editing_prefs::{NudgeSettings, NudgeTime, ScaleKind, key_root};
use vizdaw_app::editor::{
    Division, GESTURE_OPEN, MainView, NudgeDirection, PaneState, PaneView, Snap,
};
use vizdaw_app::select_modify::model::HIDDEN_SELECTION;
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
use vizdaw_ffi::events::MeterEntryRow;
use vizdaw_ffi::selection::{SelectionRow, TickRange};

const NUDGES: [&str; 4] = [
    "editors.nudge_earlier",
    "editors.nudge_later",
    "editors.nudge_up",
    "editors.nudge_down",
];
const NEEDS_EVENTS: &str = "Nudge needs selected events and no text being edited.";

fn meter(num: u8, sharps: i8, minor: bool) -> MeterEntryRow {
    MeterEntryRow {
        tick: 0,
        num,
        den: 4,
        sharps,
        minor,
    }
}

fn chords() -> Vec<LaneRef> {
    vec![LaneRef { seq: 3, lane: 31 }]
}

fn selection(range: Option<TickRange>, events: &[u64]) -> SelectionRow {
    SelectionRow {
        time_range: range,
        lanes: chords(),
        events: events.to_vec(),
    }
}

fn selected(events: &[u64]) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(roll_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.set_selection(selection(None, events));
    (f, m)
}

fn nudge(events: &[u64], ticks: i64, semitones: i8) -> EditCommand {
    EditCommand::Event {
        edit: EventEdit::Nudge {
            events: events.to_vec(),
            ticks,
            semitones,
        },
    }
}

fn in_scale(events: &[u64], steps: i8, root: u8, mask: u16) -> EditCommand {
    EditCommand::Event {
        edit: EventEdit::TransposeInScale {
            events: events.to_vec(),
            steps,
            root,
            mask,
        },
    }
}

fn hide_selection(m: &mut DocumentModel) {
    m.update_layout(|l| {
        l.views.main = MainView::Mix;
        l.views.mix.bottom.shown = false;
        l.views.mix.bottom.view = PaneView::Editor;
    });
}

#[test]
fn time_steps() {
    let four = [meter(4, 0, false)];
    let snap = Snap {
        division: Division::Eighth,
        enabled: false,
        ..Snap::default()
    };
    let mut s = NudgeSettings::default();
    assert_eq!(
        s.ticks(0, &four, &snap),
        7_680,
        "the grid's step even with snap off"
    );
    s.time = NudgeTime::Division(Division::Quarter, true);
    assert_eq!(s.ticks(0, &four, &snap), 10_240);
    s.time = NudgeTime::Division(Division::Bar, false);
    assert_eq!(s.ticks(0, &[meter(3, 0, false)], &snap), 46_080);
    s.time = NudgeTime::Ticks(0);
    assert_eq!(s.ticks(0, &four, &snap), 1);
    s.time = NudgeTime::Ticks(240);
    assert_eq!(s.ticks(0, &four, &snap), 240);
}

#[test]
fn keys_follow_the_signature_or_are_fixed() {
    let mut s = NudgeSettings::default();
    let g_major = [meter(4, 1, false)];
    assert_eq!(s.key(0, &g_major), (7, ScaleKind::Major.mask()));
    let a_minor = [meter(4, 0, true)];
    assert_eq!(s.key(0, &a_minor), (9, ScaleKind::NaturalMinor.mask()));
    s.follow_key_signature = false;
    s.root = 14;
    s.scale = ScaleKind::Dorian;
    assert_eq!(s.key(0, &g_major), (2, ScaleKind::Dorian.mask()));
    // Swift `PitchMathTests`: (7 × sharps) mod 12, plus 9 for minor.
    let major = [11u8, 6, 1, 8, 3, 10, 5, 0, 7, 2, 9, 4, 11, 6, 1];
    for (i, sharps) in (-7i8..=7).enumerate() {
        assert_eq!(key_root(sharps, false), major[i], "major {sharps}");
        assert_eq!(
            key_root(sharps, true),
            (major[i] + 9) % 12,
            "minor {sharps}"
        );
    }
}

#[test]
fn nudge_sends_time_and_pitch_commands() {
    let (f, mut m) = selected(&[1, 2]);
    let mut s = NudgeSettings::default();
    assert!(m.nudge(NudgeDirection::Later, &s));
    assert_eq!(f.edits().last(), Some(&nudge(&[1, 2], 15_360, 0)));
    m.nudge(NudgeDirection::Earlier, &s);
    assert_eq!(f.edits().last(), Some(&nudge(&[1, 2], -15_360, 0)));
    s.amount = 3;
    m.nudge(NudgeDirection::Up, &s);
    assert_eq!(f.edits().last(), Some(&nudge(&[1, 2], 0, 3)));
    s.pitch_scale_steps = true;
    s.amount = 2;
    m.nudge(NudgeDirection::Down, &s);
    assert_eq!(
        f.edits().last(),
        Some(&in_scale(&[1, 2], -2, 0, ScaleKind::Major.mask()))
    );
}

#[test]
fn scale_steps_read_the_key_at_the_anchor() {
    let f = Arc::new(roll_fake());
    let mut info = four_four(3);
    info.meter = vec![
        meter(4, 1, false),
        MeterEntryRow {
            tick: 122_880,
            ..meter(4, 0, true)
        },
    ];
    f.set_info(3, info);
    f.set_events(
        3,
        31,
        vec![note_row(1, 0, 60), note_row(2, 122_880 + 15_360, 64)],
    );
    let mut m = DocumentModel::new(f.clone()).unwrap();
    let s = NudgeSettings {
        pitch_scale_steps: true,
        amount: 1,
        ..NudgeSettings::default()
    };
    let minor = ScaleKind::NaturalMinor.mask();

    m.set_selection(selection(None, &[2]));
    assert!(m.nudge(NudgeDirection::Up, &s));
    assert_eq!(f.edits().last(), Some(&in_scale(&[2], 1, 9, minor)));

    m.set_selection(selection(
        Some(TickRange {
            start: 122_880,
            end: 184_320,
        }),
        &[1],
    ));
    assert!(m.nudge(NudgeDirection::Up, &s));
    assert_eq!(f.edits().last(), Some(&in_scale(&[1], 1, 9, minor)));

    m.set_selection(selection(None, &[1]));
    assert!(m.nudge(NudgeDirection::Up, &s));
    assert_eq!(
        f.edits().last(),
        Some(&in_scale(&[1], 1, 7, ScaleKind::Major.mask()))
    );
}

#[test]
fn nothing_selected_nothing_sent() {
    let (f, mut m) = selected(&[]);
    assert!(!m.nudge(NudgeDirection::Up, &NudgeSettings::default()));
    assert!(f.edits().is_empty());
}

#[test]
fn refused_while_a_drag_is_open() {
    let (f, mut m) = selected(&[1]);
    assert!(m.begin_gesture("Move Notes", vec![(3, 31)]));
    assert!(!m.nudge(NudgeDirection::Up, &NudgeSettings::default()));
    assert!(f.edits().is_empty());
    assert_eq!(m.status(), Some(GESTURE_OPEN));
}

/// Owner decision 5 (plan release-fixes Task 5): in the Mix view with no
/// editor showing, the nudges wait and say why; a key says it in the
/// status line.
#[test]
fn nudges_wait_while_the_mix_view_hides_the_selection() {
    let (f, mut m) = selected(&[1, 2]);
    hide_selection(&mut m);
    let before = f.edits().len();
    for id in NUDGES {
        assert!(EDITOR_COMMANDS.contains(&id), "{id}");
        assert!(!commands::enabled(id, &m), "{id}");
        let e = commands::run(id, &[], &mut m).unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str()),
            (ErrorCode::Unavailable, HIDDEN_SELECTION),
            "{id}"
        );
    }
    assert_eq!(f.edits().len(), before);
    // A key press says why in the status line.
    m.set_status(None);
    assert!(commands::perform_from_menu("editors.nudge_later", &mut m).is_err());
    assert_eq!(m.status(), Some(HIDDEN_SELECTION));
    assert_eq!(f.edits().len(), before);
    // The Editor shown in the Mix view's bottom pane: the nudge runs.
    m.update_layout(|l| {
        l.views.mix.bottom = PaneState {
            shown: true,
            view: PaneView::Editor,
            size: 260.,
        }
    });
    assert!(commands::enabled("editors.nudge_later", &m));
    commands::run("editors.nudge_later", &[], &mut m).unwrap();
    assert_eq!(f.edits().last(), Some(&nudge(&[1, 2], 15_360, 0)));
}

#[test]
fn the_anchor_is_the_range_start_else_the_earliest_selected_event() {
    let (_, mut m) = selected(&[2]);
    assert_eq!(m.nudge_anchor(), Some((3, 15_360)));
    m.set_selection(selection(
        Some(TickRange {
            start: 61_440,
            end: 122_880,
        }),
        &[2],
    ));
    assert_eq!(m.nudge_anchor(), Some((3, 61_440)));
    m.set_selection(selection(None, &[]));
    assert_eq!(m.nudge_anchor(), Some((3, 0)), "no event: tick 0");
    m.clear_selection();
    assert_eq!(m.nudge_anchor(), None, "no selected tracks");
}

/// Each refusal of the channel's arm (`M/ControlCommands.swift:640-648`).
#[test]
fn each_nudge_refusal_says_why() {
    let unavailable = |m: &mut DocumentModel, id: &str| {
        let e = commands::run(id, &[], m).unwrap_err();
        (e.code, e.message)
    };
    // No selected events.
    let (f, mut m) = selected(&[]);
    for id in NUDGES {
        assert!(!commands::enabled(id, &m), "{id}");
        assert_eq!(
            unavailable(&mut m, id),
            (ErrorCode::Unavailable, NEEDS_EVENTS.to_owned()),
            "{id}"
        );
    }
    // A text field has the keyboard (⌥-arrows move its cursor).
    let (_, mut m) = selected(&[1]);
    for id in NUDGES {
        assert!(commands::enabled(id, &m), "{id}");
    }
    m.begin_text_editing("field");
    for id in NUDGES {
        assert!(!commands::enabled(id, &m), "{id}");
        assert_eq!(
            unavailable(&mut m, id),
            (ErrorCode::Unavailable, NEEDS_EVENTS.to_owned()),
            "{id}"
        );
    }
    m.end_text_editing("field");
    // The core refused: its words, else "The nudge was refused.".
    let (f2, mut m) = selected(&[1]);
    f2.set_edit_error(Some(VizError::Model {
        message: "Out of range.".into(),
    }));
    assert_eq!(
        unavailable(&mut m, "editors.nudge_up"),
        (ErrorCode::Failed, "Out of range.".to_owned())
    );
    f2.set_edit_error(Some(VizError::Model {
        message: String::new(),
    }));
    m.set_status(None);
    assert_eq!(
        unavailable(&mut m, "editors.nudge_up"),
        (ErrorCode::Failed, "The nudge was refused.".to_owned())
    );
    // An open drag.
    f2.set_edit_error(None);
    assert!(m.begin_gesture("Move Notes", vec![(3, 31)]));
    assert_eq!(
        unavailable(&mut m, "editors.nudge_later"),
        (ErrorCode::Failed, GESTURE_OPEN.to_owned())
    );
    assert!(f.edits().is_empty());
}

/// The channel and the menus hand the app's settings to the arm: a
/// document runs the settings it was last given.
#[test]
fn the_arm_uses_the_settings_it_was_handed() {
    let (f, mut m) = selected(&[1]);
    m.set_nudge_settings(NudgeSettings {
        time: NudgeTime::Ticks(240),
        ..NudgeSettings::default()
    });
    commands::run("editors.nudge_earlier", &[], &mut m).unwrap();
    assert_eq!(f.edits().last(), Some(&nudge(&[1], -240, 0)));
    commands::perform_from_menu("editors.nudge_later", &mut m).unwrap();
    assert_eq!(f.edits().last(), Some(&nudge(&[1], 240, 0)));
}
