//! The Drum menu (Swift `M/DrumCommands.swift`; slice 10d Task 4): its
//! items follow the grid and act on its selected row, from the menu and
//! the control channel; `editors.drum_grid` shows the grid.

mod support;

use std::sync::Arc;

use core_control::ErrorCode;
use core_control::registry::ArgValue;
use support::{FakeSession, drum_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::commands::{self, DRUM_COMMANDS, EDITOR_COMMANDS};
use vizdaw_app::drum_grid::{DrumView, NO_GRID, NO_ROW};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget};
use vizdaw_ffi::VizError;
use vizdaw_ffi::drum::DrumEdit;
use vizdaw_ffi::edit::EditCommand;

const DRUMS: EditorTarget = EditorTarget::Lane { seq: 3, lane: 40 };

fn on(v: bool) -> Vec<(String, ArgValue)> {
    vec![("on".into(), ArgValue::Flag(v))]
}

fn drum(edit: DrumEdit) -> EditCommand {
    EditCommand::Drum { edit }
}

fn new() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(drum_fake());
    let m = DocumentModel::new(f.clone()).unwrap();
    (f, m)
}

/// `new` with the pane's Drum Grid on Drums (40), calls cleared.
fn grid() -> (Arc<FakeSession>, DocumentModel) {
    let (f, mut m) = new();
    m.open_detail(DRUMS, Some(DetailEditorKind::DrumGrid));
    f.clear_calls();
    (f, m)
}

#[test]
fn the_drum_menu_follows_the_grid() {
    assert_eq!(
        DRUM_COMMANDS,
        [
            "drum.add_row",
            "drum.show_velocity",
            "drum.split_row",
            "drum.remove_row",
            "drum.timeline"
        ]
    );
    let (f, mut m) = new();
    for id in DRUM_COMMANDS {
        assert_eq!(commands::enabled(id, &m), id == "drum.timeline", "{id}");
        assert_eq!(
            commands::drum_enabled(id, &m),
            id == "drum.timeline",
            "{id}"
        );
    }
    let e = commands::run_drum("drum.split_row", &[], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "Open a drum grid first.");
    assert_eq!(e.message, NO_GRID);
    // The List on Drums: still no grid.
    m.open_detail(DRUMS, Some(DetailEditorKind::List));
    assert!(!commands::enabled("drum.add_row", &m));
    m.open_detail(DRUMS, Some(DetailEditorKind::DrumGrid));
    assert!(commands::enabled("drum.add_row", &m));
    assert!(!commands::enabled("drum.remove_row", &m));
    let e = commands::run("drum.remove_row", &[], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "Select a drum row first.");
    assert_eq!(e.message, NO_ROW);
    let e = commands::perform_from_menu("drum.split_row", &mut m).unwrap_err();
    assert_eq!(e.message, NO_ROW);
    assert_eq!(m.status(), Some(NO_ROW), "the menu shows why");
    m.drum_select(0, 1);
    for id in DRUM_COMMANDS {
        assert!(commands::enabled(id, &m), "{id}");
    }
    assert!(f.edits().is_empty());
}

#[test]
fn the_drum_menu_runs_its_commands() {
    let (f, mut m) = grid();
    assert!(!commands::is_on("drum.timeline", &m));
    commands::run("drum.timeline", &on(true), &mut m).unwrap();
    assert_eq!(m.drum_settings().view, DrumView::Timeline);
    assert!(commands::is_on("drum.timeline", &m));
    assert!(commands::drum_is_on("drum.timeline", &m));
    commands::run("drum.timeline", &[], &mut m).unwrap();
    assert_eq!(m.drum_settings().view, DrumView::Matrix);
    assert!(!commands::is_on("drum.timeline", &m));
    commands::perform_from_menu("drum.timeline", &mut m).unwrap();
    assert_eq!(m.drum_settings().view, DrumView::Timeline);

    m.drum_select(0, 1);
    assert!(!commands::is_on("drum.show_velocity", &m));
    commands::run("drum.show_velocity", &on(true), &mut m).unwrap();
    assert_eq!(
        f.edits().last(),
        Some(&drum(DrumEdit::ShowVelocity {
            seq: 3,
            lane: 40,
            row: 2,
            show: true,
        }))
    );
    let e = commands::run(
        "drum.show_velocity",
        &[("on".into(), ArgValue::Int(1))],
        &mut m,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);

    commands::perform_from_menu("drum.add_row", &mut m).unwrap();
    assert_eq!(
        f.edits().last(),
        Some(&drum(DrumEdit::AddRow {
            seq: 3,
            lane: 40,
            name: "Closed Hat".into(),
            note: 42,
            dest: None,
        }))
    );
    commands::run("drum.split_row", &[], &mut m).unwrap();
    assert_eq!(
        f.edits().last(),
        Some(&drum(DrumEdit::SplitRow {
            seq: 3,
            lane: 40,
            row: 2,
        }))
    );
    commands::run("drum.remove_row", &[], &mut m).unwrap();
    assert_eq!(
        f.edits().last(),
        Some(&drum(DrumEdit::RemoveRow {
            seq: 3,
            lane: 40,
            row: 2,
        }))
    );
    assert_eq!(f.edits().len(), 4, "one edit each");

    f.set_edit_error(Some(VizError::Model {
        message: "No such row.".into(),
    }));
    let e = commands::run("drum.remove_row", &[], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "No such row.");
    assert_eq!(f.edits().len(), 4);
}

/// The selected row is a row of the grid's own track: once the pane
/// shows another editor the row commands are off, and Remove sends
/// nothing (never a row of another track or editor).
#[test]
fn the_drum_menu_leaves_other_editors_alone() {
    let (f, mut m) = grid();
    m.drum_select(1, 0);
    commands::run("drum.remove_row", &[], &mut m).unwrap();
    assert_eq!(
        f.edits().last(),
        Some(&drum(DrumEdit::RemoveRow {
            seq: 3,
            lane: 41,
            row: 3,
        }))
    );
    m.open_detail(DRUMS, Some(DetailEditorKind::PianoRoll));
    let n = f.edits().len();
    for id in ["drum.remove_row", "drum.split_row", "drum.show_velocity"] {
        assert!(!commands::enabled(id, &m), "{id}");
        let e = commands::run(id, &[], &mut m).unwrap_err();
        assert_eq!(e.message, NO_GRID, "{id}");
    }
    m.open_detail(DRUMS, Some(DetailEditorKind::DrumGrid));
    let e = commands::run("drum.remove_row", &[], &mut m).unwrap_err();
    assert_eq!(e.message, NO_ROW, "the selection did not come back");
    assert_eq!(f.edits().len(), n);
}

#[test]
fn editors_drum_grid_shows_the_grid() {
    let (_f, mut m) = new();
    assert!(EDITOR_COMMANDS.contains(&"editors.drum_grid"));
    assert!(!EDITOR_COMMANDS.contains(&"editors.drum_grid_window"));
    m.open_detail(DRUMS, Some(DetailEditorKind::List));
    assert!(commands::enabled("editors.drum_grid", &m));
    commands::run("editors.drum_grid", &[], &mut m).unwrap();
    assert_eq!(m.detail_kind_name(), "drum_grid");
    assert_eq!(m.layout().detail.as_ref().unwrap().target, DRUMS);
    commands::perform_from_menu("editors.list", &mut m).unwrap();
    commands::perform_from_menu("editors.drum_grid", &mut m).unwrap();
    assert_eq!(m.detail_kind_name(), "drum_grid");
}
