//! The Modify menu's commands over the menus and the control channel
//! (slice 10b Task 4; Swift `M/ControlCommands.swift:299-302, 653-673`).

mod support;

use std::sync::Arc;

use core_control::ErrorCode;
use core_control::registry::{self, ArgValue};
use support::{FakeSession, roll_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::commands;
use vizdaw_app::editor::{MainView, PaneView};
use vizdaw_app::modify_commands::{self, MODIFY_COMMANDS};
use vizdaw_app::select_modify::PanelTask;
use vizdaw_app::select_modify::model::{HIDDEN_SELECTION, NO_MODIFICATION, NO_TEMPLATES};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, LaneRef, ViewFilterRow};
use vizdaw_ffi::events::EventKindRow;
use vizdaw_ffi::modify::{ModifyRow, ModifyTemplateRow, SelectModeRow};
use vizdaw_ffi::selection::SelectionRow;

fn doc() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(roll_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq: 3, lane: 31 }],
        events: vec![1],
    });
    (f, m)
}

fn text(name: &str, v: &str) -> (String, ArgValue) {
    (name.to_owned(), ArgValue::Text(v.to_owned()))
}

fn flag(name: &str, v: bool) -> (String, ArgValue) {
    (name.to_owned(), ArgValue::Flag(v))
}

fn hide_selection(m: &mut DocumentModel) {
    m.update_layout(|l| {
        l.views.main = MainView::Mix;
        l.views.mix.bottom.shown = false;
        l.views.mix.bottom.view = PaneView::Editor;
    });
}

fn hidden_cc() -> ViewFilterRow {
    ViewFilterRow {
        hidden_kinds: vec![EventKindRow::Cc],
        hidden_lanes: vec![],
    }
}

fn up_template() -> ModifyTemplateRow {
    ModifyTemplateRow {
        name: "Up".into(),
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::TransposeInterval { semitones: 12 }),
        to_copy: false,
    }
}

const CHORES: [&str; 3] = [
    "modify.quantize_notes",
    "modify.transpose_notes",
    "modify.velocity_notes",
];

#[test]
fn the_list_names_every_modify_id_in_the_registry() {
    assert_eq!(MODIFY_COMMANDS.len(), 11);
    for id in MODIFY_COMMANDS {
        assert!(registry::find(id).is_some(), "{id}");
    }
    for task in PanelTask::ALL {
        assert!(MODIFY_COMMANDS.contains(&task.command_id()));
    }
}

#[test]
fn each_panel_command_opens_the_panel_on_its_task() {
    for task in PanelTask::ALL {
        let (_, mut m) = doc();
        let ran = commands::run(task.command_id(), &[], &mut m).unwrap();
        assert!(ran.open_modify_panel, "{task:?}");
        assert!(!ran.open_mixer_window);
        let s = m.select_modify();
        assert!(s.is_open() && s.task == task, "{task:?}");
        assert_eq!(s.filter(), None, "the menu opens it with no filter");
        assert!(m.take_modify_present());

        let (_, mut m) = doc();
        let ran = commands::perform_from_menu(task.command_id(), &mut m).unwrap();
        assert!(ran.open_modify_panel && m.select_modify().task == task);
    }
    let (_, mut m) = doc();
    for id in ["modify.again", "modify.exact_mode"] {
        let ran = commands::run(id, &[], &mut m);
        assert!(ran.map_or(true, |r| !r.open_modify_panel), "{id}");
    }
}

#[test]
fn modify_again_refusals_say_why() {
    // Hidden comes first, even with nothing to repeat.
    let (f, mut m) = doc();
    hide_selection(&mut m);
    let e = commands::run("modify.again", &[], &mut m).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::Unavailable, HIDDEN_SELECTION)
    );
    f.set_last_modify(Some(ModifyRow::Move { delta: 5 }));
    let e = commands::run("modify.again", &[], &mut m).unwrap_err();
    assert_eq!(e.message, HIDDEN_SELECTION);
    assert!(f.filtered_edits().is_empty());

    // Nothing to repeat.
    let (f, mut m) = doc();
    let e = commands::run("modify.again", &[], &mut m).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::Unavailable, NO_MODIFICATION)
    );
    assert!(f.filtered_edits().is_empty());

    // The core refuses: its words.
    let (f, mut m) = doc();
    f.set_last_modify(Some(ModifyRow::Move { delta: 5 }));
    f.set_edit_error(Some(VizError::Model {
        message: "No events to modify.".into(),
    }));
    let e = commands::run("modify.again", &[], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(Some(e.message.as_str()), m.status());
    assert!(e.message.contains("No events to modify."), "{}", e.message);

    // The menu says it in the status line too.
    let (_, mut m) = doc();
    let e = commands::perform_from_menu("modify.again", &mut m).unwrap_err();
    assert_eq!(e.message, NO_MODIFICATION);
    assert_eq!(m.status(), Some(NO_MODIFICATION));
}

#[test]
fn modify_again_sends_the_panel_filter() {
    let (f, mut m) = doc();
    f.set_last_modify(Some(ModifyRow::Move { delta: 5 }));
    m.open_modify(PanelTask::Change, Some(hidden_cc()));
    m.modify_panel_focused(true);
    let ran = commands::run("modify.again", &[], &mut m).unwrap();
    assert!(!ran.open_modify_panel);
    assert_eq!(
        f.filtered_edits(),
        vec![(EditCommand::ModifyAgain, Some(hidden_cc()))]
    );
    m.modify_panel_closed();
    commands::perform_from_menu("modify.again", &mut m).unwrap();
    assert_eq!(
        f.filtered_edits().last(),
        Some(&(EditCommand::ModifyAgain, None))
    );
}

#[test]
fn modify_template_runs_by_name_and_refuses_unknown_and_none() {
    let (f, mut m) = doc();
    let e = commands::run("modify.template", &[text("name", "Up")], &mut m).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::Unavailable, NO_TEMPLATES)
    );

    f.set_templates(vec![up_template()]);
    let (f, mut m) = (f.clone(), DocumentModel::new(f).unwrap());
    let e = commands::run("modify.template", &[text("name", "Down")], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert_eq!(e.message, "modify.template: no template named \"Down\"");
    assert!(f.edits().is_empty());

    m.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq: 3, lane: 31 }],
        events: vec![1],
    });
    let ran = commands::run("modify.template", &[text("name", "Up")], &mut m).unwrap();
    assert!(!ran.open_modify_panel, "a template runs without the panel");
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::SelectAndModify {
            rule: None,
            mode: SelectModeRow::Select,
            modify: Some(ModifyRow::TransposeInterval { semitones: 12 }),
            to_copy: false,
        })
    );

    f.set_edit_error(Some(VizError::Model {
        message: "No events to modify.".into(),
    }));
    let e = commands::run("modify.template", &[text("name", "Up")], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(Some(e.message.as_str()), m.status());
}

#[test]
fn exact_mode_toggles_or_takes_on() {
    let (_, mut m) = doc();
    commands::run("modify.exact_mode", &[], &mut m).unwrap();
    assert!(m.exact_mode());
    commands::run("modify.exact_mode", &[], &mut m).unwrap();
    assert!(!m.exact_mode());
    commands::run("modify.exact_mode", &[flag("on", true)], &mut m).unwrap();
    commands::run("modify.exact_mode", &[flag("on", true)], &mut m).unwrap();
    assert!(m.exact_mode());
    commands::run("modify.exact_mode", &[flag("on", false)], &mut m).unwrap();
    assert!(!m.exact_mode());
    commands::perform_from_menu("modify.exact_mode", &mut m).unwrap();
    assert!(m.exact_mode() && m.editor().exact_mode());
    let e = commands::run("modify.exact_mode", &[text("on", "yes")], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
}

#[test]
fn each_chore_makes_one_run_chore_call() {
    let (f, mut m) = doc();
    let args = [
        vec![text("grid", "1/16")],
        vec![(String::from("semitones"), ArgValue::Int(12))],
        vec![
            text("op", "set"),
            (String::from("amount"), ArgValue::Int(100)),
        ],
    ];
    for (i, (id, args)) in CHORES.iter().zip(args).enumerate() {
        commands::run(id, &args, &mut m).unwrap();
        let runs = f.chores_run();
        assert_eq!(runs.len(), i + 1, "{id}: one call");
        let (ran_id, ran_args) = runs.last().unwrap();
        assert_eq!(ran_id, id);
        let names: Vec<&str> = ran_args.iter().map(|a| a.name.as_str()).collect();
        let want: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, want, "{id}: the arguments as given");
    }
    assert_eq!(f.chores_run().len(), 3);
    // An unknown parameter never reaches the core.
    let e = commands::run(CHORES[0], &[text("bogus", "x")], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert_eq!(f.chores_run().len(), 3);
}

#[test]
fn the_menu_disables_modify_again_and_templates_when_empty() {
    let (f, m) = doc();
    for task in PanelTask::ALL {
        assert!(modify_commands::enabled(task.command_id(), &m));
        assert!(commands::enabled(task.command_id(), &m));
    }
    assert!(commands::enabled("modify.exact_mode", &m));
    assert!(!commands::enabled("modify.again", &m), "nothing to repeat");
    assert!(!commands::enabled("modify.template", &m), "no templates");

    f.set_last_modify(Some(ModifyRow::Move { delta: 5 }));
    f.set_templates(vec![up_template()]);
    let mut m = DocumentModel::new(f.clone()).unwrap();
    assert!(commands::enabled("modify.again", &m));
    assert!(commands::enabled("modify.template", &m));
    hide_selection(&mut m);
    assert!(
        !commands::enabled("modify.again", &m),
        "the selection is hidden"
    );
    assert!(commands::enabled("modify.template", &m));
}
