//! The Select and Modify seam on `DocSession` and the fake's model of it
//! (slice 10b Task 3).

use vizdaw_app::session::DocSession;
use vizdaw_app::testing::roll_fake;
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, ViewFilterRow};
use vizdaw_ffi::events::EventKindRow;
use vizdaw_ffi::modify::{ModifyRow, ModifyTemplateRow, SelectModeRow, SettingsEdit};

fn template(name: &str, semitones: i8) -> ModifyTemplateRow {
    ModifyTemplateRow {
        name: name.into(),
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::TransposeInterval { semitones }),
        to_copy: false,
    }
}

fn save(t: ModifyTemplateRow) -> EditCommand {
    EditCommand::Settings {
        edit: SettingsEdit::SaveTemplate { template: t },
    }
}

#[test]
fn the_fake_saves_lists_and_deletes_templates() {
    let f = roll_fake();
    assert_eq!(f.modify_templates().unwrap(), vec![]);
    f.set_templates(vec![template("Up", 12)]);
    assert_eq!(f.modify_templates().unwrap(), vec![template("Up", 12)]);

    f.edit(save(template("Down", -12))).unwrap();
    f.edit(save(template("Up", 7))).unwrap();
    assert_eq!(
        f.modify_templates().unwrap(),
        vec![template("Down", -12), template("Up", 7)],
        "by name; a save replaces one of the same name"
    );

    f.edit(EditCommand::Settings {
        edit: SettingsEdit::DeleteTemplate { name: "Up".into() },
    })
    .unwrap();
    assert_eq!(f.modify_templates().unwrap(), vec![template("Down", -12)]);
    assert_eq!(f.edits().len(), 3);
}

#[test]
fn a_modify_sets_last_modify_and_modify_again_needs_one() {
    let f = roll_fake();
    assert_eq!(f.last_modify().unwrap(), None);
    let err = f.edit(EditCommand::ModifyAgain).unwrap_err();
    assert_eq!(
        err,
        VizError::Model {
            message: "There is no modification to repeat.".into()
        }
    );
    assert!(f.edits().is_empty(), "a refused edit is not recorded");

    // A select-only command leaves it alone.
    f.edit(EditCommand::SelectAndModify {
        rule: None,
        mode: SelectModeRow::Select,
        modify: None,
        to_copy: false,
    })
    .unwrap();
    assert_eq!(f.last_modify().unwrap(), None);

    f.edit(EditCommand::SelectAndModify {
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::Move { delta: 5 }),
        to_copy: false,
    })
    .unwrap();
    assert_eq!(f.last_modify().unwrap(), Some(ModifyRow::Move { delta: 5 }));
    f.edit(EditCommand::ModifyAgain).unwrap();
    assert_eq!(f.edits().last(), Some(&EditCommand::ModifyAgain));

    f.set_last_modify(Some(ModifyRow::Delete));
    assert_eq!(f.last_modify().unwrap(), Some(ModifyRow::Delete));

    // Reads fail while scripted to (the Busy read test).
    f.set_fail_template_reads(Some(VizError::Busy {
        message: "busy".into(),
    }));
    assert!(f.last_modify().is_err() && f.modify_templates().is_err());
}

#[test]
fn filtered_edits_record_their_filter() {
    let f = roll_fake();
    let hidden = ViewFilterRow {
        hidden_kinds: vec![EventKindRow::Cc],
        hidden_lanes: vec![],
    };
    let cmd = EditCommand::SelectAndModify {
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::Move { delta: 1 }),
        to_copy: false,
    };
    f.edit_filtered(cmd.clone(), Some(hidden.clone())).unwrap();
    f.edit_filtered(EditCommand::ModifyAgain, None).unwrap();
    assert_eq!(
        f.filtered_edits(),
        vec![
            (cmd.clone(), Some(hidden)),
            (EditCommand::ModifyAgain, None)
        ]
    );
    assert_eq!(f.edits(), vec![cmd, EditCommand::ModifyAgain]);

    // Busy while an edit is open, as `edit` is; nothing is recorded.
    f.begin_edit("Drag").unwrap();
    assert!(matches!(
        f.edit_filtered(EditCommand::ModifyAgain, None),
        Err(VizError::Busy { .. })
    ));
    assert_eq!(f.filtered_edits().len(), 2);
}
