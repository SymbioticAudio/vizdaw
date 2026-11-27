//! The per-document Select and Modify panel model (slice 10b Task 3; Swift
//! `SelectModifyModelTests`, `ModifyPanelWiringTests`, `ModifyReadTests`).

mod support;

use std::sync::Arc;

use support::{FakeSession, roll_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::PPQ;
use vizdaw_app::editing_prefs::ScaleKind;
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, GESTURE_OPEN, MainView, PaneView};
use vizdaw_app::list::ListKind;
use vizdaw_app::select_modify::model::{
    DRAGGED_EVENTS_TARGET, DroppedAxis, FIX_FIELDS, HIDDEN_SELECTION, ModifyRequest, NO_NAME,
    NO_TRACKS, NOTHING_ON, SELECTION_TARGET, TEXT_OWNER_PREFIX, text_owner,
};
use vizdaw_app::select_modify::{
    AmountKind, ChangeOpKind, Comparison, ConditionKind, ModifyConfig, ModifyKind, PanelTask,
    modify_again_title,
};
use vizdaw_app::session::DocSession;
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef, ViewFilterRow};
use vizdaw_ffi::events::EventKindRow;
use vizdaw_ffi::modify::{
    AmountRow, ChangeOpRow, ConditionTest, CurveRow, ModifyRow, ModifyTemplateRow, NoteMapEntry,
    SelectModeRow, SettingsEdit, ValueTargetRow,
};
use vizdaw_ffi::selection::{SelectionRow, TickRange};

fn panel() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(roll_fake());
    let m = DocumentModel::new(f.clone()).unwrap();
    (f, m)
}

fn chords(range: Option<TickRange>) -> SelectionRow {
    SelectionRow {
        time_range: range,
        lanes: vec![LaneRef { seq: 3, lane: 31 }],
        events: vec![1],
    }
}

fn select_chords(m: &mut DocumentModel) {
    m.set_selection(chords(None));
}

fn hidden_cc() -> ViewFilterRow {
    ViewFilterRow {
        hidden_kinds: vec![EventKindRow::Cc],
        hidden_lanes: vec![],
    }
}

fn sam(modify: ModifyRow) -> EditCommand {
    EditCommand::SelectAndModify {
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(modify),
        to_copy: false,
    }
}

fn drag_request() -> ModifyRequest {
    ModifyRequest {
        task: PanelTask::Move,
        config: ModifyConfig::preset(PanelTask::Move, None),
        events: Some(vec![1]),
        lanes: vec![(3, 31)],
        range: None,
        dropped: None,
    }
}

fn dropped_request(events: Vec<u64>) -> ModifyRequest {
    ModifyRequest {
        events: Some(events),
        dropped: Some(DroppedAxis::Vertical),
        ..drag_request()
    }
}

fn sets(f: &FakeSession) -> usize {
    f.count("set_selection")
}

fn delete1() -> EditCommand {
    EditCommand::Event {
        edit: EventEdit::Delete { events: vec![1] },
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

// MARK: SelectModifyModelTests

#[test]
fn opening_a_task_presets_and_presents_the_panel() {
    let (_, mut m) = panel();
    assert!(!m.take_modify_present());
    m.open_modify(PanelTask::Quantize, None);
    let s = m.select_modify();
    assert!(s.is_open() && s.task == PanelTask::Quantize);
    assert_eq!(s.config, ModifyConfig::preset(PanelTask::Quantize, None));
    assert!(m.take_modify_present());
    assert!(!m.take_modify_present(), "presented once");
    m.set_selection(chords(Some(TickRange {
        start: 0,
        end: 4 * 4 * PPQ,
    })));
    m.open_modify(PanelTask::SelectByRule, None);
    assert_eq!(
        m.select_modify()
            .config
            .select
            .conditions
            .last()
            .map(|c| c.test.clone()),
        Some(ConditionTest::TimeRange {
            start: 0,
            end: 4 * 4 * PPQ
        })
    );
    m.modify_panel_closed();
    assert!(!m.select_modify().is_open() && !m.select_modify().is_key());
}

#[test]
fn apply_sends_one_command_with_the_opening_lists_filter() {
    let (f, mut m) = panel();
    select_chords(&mut m);
    m.open_modify(PanelTask::Transpose, Some(hidden_cc()));
    m.set_modify_field("m0", "7");
    assert!(m.apply_modify());
    assert_eq!(
        f.filtered_edits(),
        vec![(
            sam(ModifyRow::TransposeInterval { semitones: 7 }),
            Some(hidden_cc())
        )]
    );
}

#[test]
fn apply_without_tracks_says_what_to_select() {
    let (f, mut m) = panel();
    m.open_modify(PanelTask::SelectByRule, None);
    assert!(!m.apply_modify());
    assert_eq!(m.status(), Some(NO_TRACKS));
    assert!(f.edits().is_empty());
    select_chords(&mut m);
    assert!(m.apply_modify());
    assert_eq!(f.edits().len(), 1);
}

#[test]
fn apply_with_nothing_turned_on_says_so() {
    let (f, mut m) = panel();
    m.open_modify(PanelTask::Change, None);
    m.select_modify_mut().config.modify.enabled = false;
    assert!(!m.apply_modify());
    assert_eq!(m.status(), Some(NOTHING_ON));
    assert!(f.edits().is_empty());
}

#[test]
fn typed_fields_keep_drafts_and_refuse_bad_text() {
    let (f, mut m) = panel();
    select_chords(&mut m);
    m.open_modify(PanelTask::Change, None);
    m.set_modify_field("m0", "abc");
    assert_eq!(m.modify_field_text("m0"), "abc");
    assert!(m.modify_field_invalid("m0"));
    assert_eq!(m.select_modify().invalid_keys(), vec!["m0"]);
    assert_eq!(
        m.select_modify().config.modify.op,
        ModifyRow::Change {
            target: ValueTargetRow::Velocity,
            op: ChangeOpRow::Add,
            amount: AmountRow::Constant { value: 0 },
        }
    );
    assert!(!m.apply_modify());
    assert_eq!(m.status(), Some(FIX_FIELDS));
    assert!(f.edits().is_empty());
    m.set_modify_field("m0", "10");
    assert!(m.select_modify().invalid_keys().is_empty());
    assert_eq!(
        m.select_modify().config.modify.op,
        ModifyRow::Change {
            target: ValueTargetRow::Velocity,
            op: ChangeOpRow::Add,
            amount: AmountRow::Constant { value: 5120 },
        }
    );
    m.set_modify_kind(ModifyKind::Move);
    assert!(!m.select_modify().has_drafts());
    assert_eq!(m.modify_field_text("m0"), "0.0");
}

#[test]
fn conditions_and_change_operations_edit() {
    let (_, mut m) = panel();
    m.open_modify(PanelTask::Change, None);
    m.add_modify_condition(ConditionKind::Velocity);
    let s = m.select_modify();
    assert!(s.config.select.enabled && s.config.select.conditions.len() == 1);
    m.set_modify_field("c0.0", "100");
    m.set_modify_comparison(0, Comparison::Is);
    assert_eq!(
        m.select_modify().config.select.conditions[0].test,
        ConditionTest::Velocity {
            low: 100 << 9,
            high: (100 << 9) | 511
        }
    );
    assert_eq!(m.modify_field_text("c0.1"), "100");
    m.set_modify_target(ValueTargetRow::Duration);
    m.set_modify_op(ChangeOpKind::Scale);
    assert_eq!(
        m.select_modify().config.modify.op,
        ModifyRow::Change {
            target: ValueTargetRow::Duration,
            op: ChangeOpRow::Scale,
            amount: AmountRow::Constant { value: 100 },
        }
    );
    m.set_modify_amount(AmountKind::Line);
    m.set_modify_curve(CurveRow::SCurve);
    assert_eq!(
        m.select_modify().config.modify.op,
        ModifyRow::Change {
            target: ValueTargetRow::Duration,
            op: ChangeOpRow::Scale,
            amount: AmountRow::Line {
                from: 0,
                to: 0,
                curve: CurveRow::SCurve
            },
        }
    );
    m.set_modify_kind(ModifyKind::Diatonic);
    m.set_modify_scale(2, ScaleKind::Dorian);
    assert_eq!(
        m.select_modify().config.modify.op,
        ModifyRow::TransposeDiatonic {
            steps: 1,
            root: 2,
            mask: ScaleKind::Dorian.mask()
        }
    );
    m.set_modify_kind(ModifyKind::NoteMap);
    m.set_modify_field("map", "C2>D2");
    assert_eq!(
        m.select_modify().config.modify.op,
        ModifyRow::NoteMap {
            map: vec![NoteMapEntry { from: 36, to: 38 }]
        }
    );
    m.remove_modify_condition(0);
    assert!(m.select_modify().config.select.conditions.is_empty());
}

#[test]
fn templates_save_load_run_and_delete() {
    let (f, mut m) = panel();
    select_chords(&mut m);
    m.open_modify(PanelTask::Transpose, None);
    m.set_modify_field("m0", "12");
    m.select_modify_mut().template_name = "  ".into();
    assert!(!m.save_modify_template());
    assert_eq!(m.status(), Some(NO_NAME));
    m.select_modify_mut().template_name = "Up".into();
    assert!(m.save_modify_template());
    let t = up_template();
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Settings {
            edit: SettingsEdit::SaveTemplate {
                template: t.clone()
            }
        })
    );
    assert_eq!(m.modify_templates(), std::slice::from_ref(&t));
    m.open_modify(PanelTask::Move, None);
    assert_eq!(m.run_modify_template("Up"), Ok(()));
    assert_eq!(
        f.edits().last(),
        Some(&sam(ModifyRow::TransposeInterval { semitones: 12 }))
    );
    m.delete_modify_template("Up");
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Settings {
            edit: SettingsEdit::DeleteTemplate { name: "Up".into() }
        })
    );
    assert!(m.modify_templates().is_empty());
    m.load_modify_template(&t);
    assert_eq!(m.select_modify().config, ModifyConfig::from_template(&t));
    assert_eq!(m.select_modify().template_name, "Up");
}

#[test]
fn save_as_template_without_a_name_says_so() {
    let (f, mut m) = panel();
    m.open_modify(PanelTask::Transpose, None);
    assert!(!m.save_modify_template(), "a new panel has no name");
    assert_eq!(m.status(), Some(NO_NAME));
    assert!(f.edits().is_empty());
}

#[test]
fn modify_again_uses_the_lists_filter_like_the_original_apply() {
    let (f, mut m) = panel();
    f.set_last_modify(Some(ModifyRow::Move { delta: 5 }));
    select_chords(&mut m);
    // Launched from a list: the panel holds that list's filter.
    m.open_modify(PanelTask::Transpose, Some(hidden_cc()));
    m.modify_panel_focused(true);
    assert_eq!(m.modify_again(), Ok(()));
    assert_eq!(
        f.filtered_edits().last(),
        Some(&(EditCommand::ModifyAgain, Some(hidden_cc())))
    );
    // Open but not key, no list: still the panel's.
    m.modify_panel_focused(false);
    assert_eq!(m.modify_again_filter(), Some(hidden_cc()));
    m.modify_panel_closed();
    assert_eq!(m.modify_again(), Ok(()));
    assert_eq!(
        f.filtered_edits().last(),
        Some(&(EditCommand::ModifyAgain, None)),
        "no list, no panel: nothing hidden"
    );
    // A list is the command surface: its filter.
    m.open_detail(
        EditorTarget::Lane { seq: 3, lane: 31 },
        Some(DetailEditorKind::List),
    );
    m.list_toggle_kind(ListKind::PitchBend);
    assert_eq!(m.modify_again(), Ok(()));
    let want = ViewFilterRow {
        hidden_kinds: vec![EventKindRow::PitchBend],
        hidden_lanes: vec![],
    };
    assert_eq!(
        f.filtered_edits().last(),
        Some(&(EditCommand::ModifyAgain, Some(want)))
    );
}

#[test]
fn modify_again_sends_modify_again() {
    let (f, mut m) = panel();
    f.set_last_modify(Some(ModifyRow::Move { delta: 5 }));
    assert_eq!(m.modify_again(), Ok(()));
    assert_eq!(f.edits().last(), Some(&EditCommand::ModifyAgain));
}

#[test]
fn random_amounts_get_a_new_seed_at_each_apply() {
    let (f, mut m) = panel();
    select_chords(&mut m);
    m.open_modify(PanelTask::Change, None);
    m.select_modify_mut().config.modify.op = ModifyRow::Change {
        target: ValueTargetRow::Velocity,
        op: ChangeOpRow::Set,
        amount: AmountRow::Random {
            low: 0,
            high: 10,
            seed: 0,
        },
    };
    assert!(m.apply_modify());
    assert!(m.apply_modify());
    let used: Vec<u64> = f
        .edits()
        .iter()
        .filter_map(|e| match e {
            EditCommand::SelectAndModify {
                modify:
                    Some(ModifyRow::Change {
                        amount: AmountRow::Random { seed, .. },
                        ..
                    }),
                ..
            } => Some(*seed),
            _ => None,
        })
        .collect();
    assert_eq!(used.len(), 2);
    assert_ne!(used[0], used[1]);
}

#[test]
fn a_key_panel_turns_the_transport_keys_off() {
    let (_, mut m) = panel();
    m.modify_panel_focused(true);
    assert!(m.select_modify().is_key());
    assert!(!m.transport_keys_enabled() && !m.return_key_enabled());
    m.modify_panel_focused(false);
    assert!(m.transport_keys_enabled() && m.return_key_enabled());
}

#[test]
fn delete_template_trims_the_name() {
    let (f, mut m) = panel();
    m.delete_modify_template("  Up ");
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Settings {
            edit: SettingsEdit::DeleteTemplate { name: "Up".into() }
        })
    );
    m.delete_modify_template("   ");
    assert_eq!(f.edits().len(), 1, "a blank name sends nothing");
}

#[test]
fn a_refused_selection_sends_no_edit() {
    let (f, mut m) = panel();
    m.set_modify_request_for_test(drag_request());
    f.set_selection_error(Some(VizError::Busy {
        message: "busy".into(),
    }));
    assert!(!m.apply_modify());
    assert!(f.edits().is_empty());
    assert!(m.status().is_some());
}

#[test]
fn a_failed_edit_restores_the_selection() {
    let (f, mut m) = panel();
    m.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq: 3, lane: 31 }],
        events: vec![2],
    });
    let before = m.selection().clone();
    let n = sets(&f);
    m.set_modify_request_for_test(drag_request());
    f.set_edit_error(Some(VizError::Model {
        message: "no".into(),
    }));
    assert!(!m.apply_modify());
    assert_eq!(sets(&f), n + 2);
    assert_eq!(m.selection(), &before);
    assert_eq!(f.selection().unwrap(), before);
}

#[test]
fn a_track_check_failure_restores_the_selection() {
    let (f, mut m) = panel();
    let before = m.selection().clone();
    m.set_modify_request_for_test(ModifyRequest {
        lanes: vec![],
        config: ModifyConfig::preset(PanelTask::SelectByRule, None),
        ..drag_request()
    });
    assert!(!m.apply_modify());
    assert_eq!(m.status(), Some(NO_TRACKS));
    assert_eq!(m.selection(), &before);
    assert_eq!(f.selection().unwrap(), before);
    assert!(f.edits().is_empty());
}

#[test]
fn an_open_drag_refuses_apply_without_touching_the_selection() {
    let (f, mut m) = panel();
    m.set_modify_request_for_test(drag_request());
    assert!(m.begin_gesture("Drag", vec![(3, 31)]));
    let n = sets(&f);
    assert!(!m.apply_modify());
    assert_eq!(m.status(), Some(GESTURE_OPEN));
    assert_eq!(sets(&f), n);
    assert!(f.edits().is_empty());
}

#[test]
fn a_second_apply_acts_on_the_current_selection() {
    let (f, mut m) = panel();
    m.set_modify_request_for_test(drag_request());
    assert!(m.apply_modify());
    let n = sets(&f);
    select_chords(&mut m);
    let after = sets(&f);
    assert!(m.apply_modify());
    assert_eq!(sets(&f), after);
    assert_eq!(after, n + 1);
    assert_eq!(f.edits().len(), 2);
}

#[test]
fn a_changed_selection_drops_the_stale_request() {
    let (f, mut m) = panel();
    m.set_modify_request_for_test(dropped_request(vec![1]));
    assert_eq!(m.modify_notice(), Some(DroppedAxis::Vertical.message()));
    select_chords(&mut m);
    assert_eq!(m.modify_notice(), None);
    let n = sets(&f);
    assert!(m.apply_modify());
    assert_eq!(sets(&f), n, "Apply acts on the current selection");
}

#[test]
fn an_unrelated_snapshot_keeps_the_request() {
    let (f, mut m) = panel();
    m.set_modify_request_for_test(dropped_request(vec![1]));
    let v = m.snapshot().version;
    let mut s = f.snap();
    s.version += 1;
    m.apply(s);
    assert_eq!(m.snapshot().version, v + 1);
    assert_eq!(m.modify_notice(), Some(DroppedAxis::Vertical.message()));
}

#[test]
fn a_vanished_event_drops_the_request() {
    let (_, mut m) = panel();
    m.set_modify_request_for_test(dropped_request(vec![99]));
    assert_eq!(m.modify_notice(), None);
    assert_eq!(m.apply_target_caption(), SELECTION_TARGET);
}

// MARK: ModifyPanelWiringTests

#[test]
fn closing_the_panel_clears_focus_key_and_text_owners() {
    let (_, mut m) = panel();
    m.open_modify(PanelTask::Transpose, None);
    m.modify_panel_focused(true);
    m.set_modify_text_editing("m0", true);
    m.set_modify_text_editing("m1", true);
    m.set_modify_text_editing("m1", false);
    assert_eq!(m.text_owners(), vec![text_owner("m0")]);
    m.begin_text_editing("list");
    m.modify_panel_closed();
    assert!(!m.select_modify().is_key() && !m.select_modify().is_open());
    assert_eq!(
        m.text_owners(),
        vec!["list".to_string()],
        "only the panel's owners go"
    );
    m.end_text_editing("list");
    assert!(m.transport_keys_enabled());
}

#[test]
fn resigning_key_gives_the_keyboard_back_to_the_transport() {
    let (_, mut m) = panel();
    m.modify_panel_focused(true);
    m.begin_text_editing(&text_owner("c0.1"));
    m.modify_panel_focused(false);
    assert!(m.text_owners().is_empty());
    assert!(m.transport_keys_enabled());
}

#[test]
fn the_apply_target_follows_the_dragged_request() {
    let (_, mut m) = panel();
    m.open_modify(PanelTask::Change, None);
    assert_eq!(m.apply_target_caption(), SELECTION_TARGET);
    m.set_modify_request_for_test(drag_request());
    assert_eq!(m.apply_target_caption(), DRAGGED_EVENTS_TARGET);
    assert!(m.apply_modify());
    assert_eq!(m.apply_target_caption(), SELECTION_TARGET);
    m.set_modify_request_for_test(drag_request());
    m.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq: 3, lane: 31 }],
        events: vec![2],
    });
    assert_eq!(
        m.apply_target_caption(),
        SELECTION_TARGET,
        "a moved-on selection drops the request"
    );
}

#[test]
fn modify_again_names_the_last_modify_and_waits_for_an_open_drag() {
    let (f, mut m) = panel();
    assert!(!m.can_modify_again());
    assert_eq!(modify_again_title(m.last_modify()), "Modify Again");
    f.set_last_modify(Some(ModifyRow::Move { delta: 5 }));
    assert!(m.edit(delete1()));
    assert!(m.can_modify_again());
    assert_eq!(modify_again_title(m.last_modify()), "Modify Again: Move");
    assert!(m.begin_gesture("Drag", vec![(3, 31)]));
    assert!(!m.can_modify_again());
}

#[test]
fn text_owners_are_unique_per_field() {
    assert_ne!(text_owner("m0"), text_owner("m1"));
    assert!(text_owner("m0").starts_with(TEXT_OWNER_PREFIX));
    assert_eq!(text_owner("c0.1"), "selectModify.c0.1");
}

// MARK: ModifyReadTests

#[test]
fn modify_reads_happen_once_per_snapshot_version() {
    let f = Arc::new(roll_fake());
    f.set_last_modify(Some(ModifyRow::Move { delta: 5 }));
    let mut m = DocumentModel::new(f.clone()).unwrap();
    assert_eq!(m.last_modify(), Some(&ModifyRow::Move { delta: 5 }));
    assert_eq!(m.last_modify(), Some(&ModifyRow::Move { delta: 5 }));
    assert!(m.modify_templates().is_empty());
    assert_eq!(f.last_modify_reads(), 1);
    assert_eq!(f.template_reads(), 1);
    f.set_last_modify(Some(ModifyRow::Delete));
    assert!(m.edit(delete1()));
    assert_eq!(m.last_modify(), Some(&ModifyRow::Delete));
    assert_eq!(f.last_modify_reads(), 2);
}

#[test]
fn a_failed_read_keeps_the_last_values() {
    let f = Arc::new(roll_fake());
    f.set_templates(vec![up_template()]);
    let mut m = DocumentModel::new(f.clone()).unwrap();
    let names = |m: &DocumentModel| -> Vec<String> {
        m.modify_templates()
            .iter()
            .map(|t| t.name.clone())
            .collect()
    };
    assert_eq!(names(&m), vec!["Up"]);
    f.set_fail_template_reads(Some(VizError::Busy {
        message: "busy".into(),
    }));
    assert!(m.edit(delete1()));
    assert_eq!(names(&m), vec!["Up"]);
    assert_eq!(m.status(), None, "reads stay silent");
    // The read is tried again until it answers.
    f.set_fail_template_reads(None);
    f.set_templates(vec![]);
    assert!(names(&m).is_empty());
}

// MARK: New

#[test]
fn the_mix_view_hides_the_selection() {
    let (_, mut m) = panel();
    assert!(!m.selection_hidden(), "the Arrange view shows it");
    let set = |m: &mut DocumentModel, shown: bool, view: PaneView| {
        m.update_layout(|l| {
            l.views.main = MainView::Mix;
            l.views.mix.bottom.shown = shown;
            l.views.mix.bottom.view = view;
        });
    };
    set(&mut m, false, PaneView::Editor);
    assert!(m.selection_hidden());
    set(&mut m, true, PaneView::Mixer);
    assert!(m.selection_hidden());
    set(&mut m, true, PaneView::Editor);
    assert!(!m.selection_hidden());
    set(&mut m, true, PaneView::AudioEditor);
    assert!(!m.selection_hidden());
    let key = if cfg!(target_os = "macos") {
        "\u{2303}1"
    } else {
        "Ctrl-1"
    };
    assert_eq!(
        HIDDEN_SELECTION,
        format!(
            "The Mix view shows no editor: press {key}, or show the Editor in the bottom pane, to edit the selection."
        )
    );
}

#[test]
fn exact_mode_starts_off_and_toggles() {
    let (_, mut m) = panel();
    assert!(!m.exact_mode() && !m.editor().exact_mode());
    m.set_exact_mode(true);
    assert!(m.exact_mode());
    m.set_exact_mode(false);
    assert!(!m.exact_mode());
}
