//! Select-and-modify through the FFI (plan 4c): rules and modifications
//! round-trip, run as one undo step, repeat with Modify Again and save as
//! templates.

use core_engine::fixtures::{Builder, Q};
use core_model::modify::{DurationOp, Modify, ValueOp};
use std::sync::Arc;
use vizdaw_ffi::modify::{modify, modify_row, rule, rule_row};
use vizdaw_ffi::{
    AmountRow, ChangeOpRow, ConditionRow, ConditionTest, CurveRow, EditCommand, EventKindRow,
    LaneRef, ModifyRow, ModifyTemplateRow, NoteMapEntry, RuleRow, SelectModeRow, SelectionRow,
    Session, SettingsEdit, SubstituteRow, ValueTargetRow, VizError, WaveRow,
};

const C_MAJOR: u16 = 0b1010_1011_0101;

/// "S"/"L" with notes 60, 64, 67 a quarter apart (velocities 0x8000,
/// 0x4000, 0x2000).
fn session() -> (Arc<Session>, u64, u64, Vec<u64>) {
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (s, l) = b.sequence("S", "L", Some(piano)).unwrap();
    let ids = b
        .notes(
            s,
            l,
            &[
                (0, 60, 0x8000, Q),
                (Q, 64, 0x4000, Q),
                (2 * Q, 67, 0x2000, Q),
            ],
        )
        .unwrap();
    let s2 = Session::from_document(b.finish()).unwrap();
    (s2, s.0, l.0, ids.iter().map(|e| e.0).collect())
}

fn pitches(s: &Session, seq: u64, lane: u64) -> Vec<u8> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .map(|r| r.pitch)
        .collect()
}

fn select(s: &Session, seq: u64, lane: u64, events: Vec<u64>) {
    s.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq, lane }],
        events,
    })
    .unwrap();
}

fn cond(test: ConditionTest, negate: bool) -> ConditionRow {
    ConditionRow { test, negate }
}

fn sm(rule: Option<RuleRow>, modify: Option<ModifyRow>) -> EditCommand {
    EditCommand::SelectAndModify {
        rule,
        mode: SelectModeRow::Select,
        modify,
        to_copy: false,
    }
}

fn velocity_set(value: i64) -> ModifyRow {
    ModifyRow::Change {
        target: ValueTargetRow::Velocity,
        op: ChangeOpRow::Set,
        amount: AmountRow::Constant { value },
    }
}

#[test]
fn select_by_rule_then_transpose_is_one_undo_step() {
    let (s, seq, lane, ids) = session();
    select(&s, seq, lane, vec![]);
    let rule = RuleRow {
        conditions: vec![cond(ConditionTest::Pitch { low: 60, high: 64 }, false)],
        match_all: true,
    };
    let snap = s
        .edit(
            sm(
                Some(rule),
                Some(ModifyRow::TransposeInterval { semitones: 12 }),
            ),
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Transpose"));
    assert_eq!(pitches(&s, seq, lane), vec![72, 76, 67]);
    assert_eq!(s.selection().unwrap().events, ids[..2].to_vec());
    s.undo().unwrap();
    assert_eq!(pitches(&s, seq, lane), vec![60, 64, 67]);
}

#[test]
fn a_select_only_command_changes_the_selection_without_an_undo_step() {
    let (s, seq, lane, ids) = session();
    select(&s, seq, lane, vec![]);
    let not_67 = RuleRow {
        conditions: vec![cond(ConditionTest::Pitch { low: 67, high: 67 }, true)],
        match_all: true,
    };
    let snap = s.edit(sm(Some(not_67), None), None).unwrap();
    assert_eq!(snap.undo_label, None);
    assert_eq!(s.selection().unwrap().events, ids[..2].to_vec());
}

#[test]
fn modify_again_repeats_on_the_current_selection() {
    let (s, seq, lane, ids) = session();
    assert!(matches!(
        s.edit(EditCommand::ModifyAgain, None),
        Err(VizError::Model { .. })
    ));
    assert_eq!(s.last_modify().unwrap(), None);
    select(&s, seq, lane, vec![ids[0]]);
    s.edit(sm(None, Some(velocity_set(100))), None).unwrap();
    assert_eq!(s.last_modify().unwrap(), Some(velocity_set(100)));
    select(&s, seq, lane, vec![ids[2]]);
    let snap = s.edit(EditCommand::ModifyAgain, None).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Change Velocity"));
    let rows = s.lane_events(seq, lane).unwrap().events;
    assert_eq!(rows[2].velocity, 100);
    assert_eq!(rows[1].velocity, 0x4000, "not selected");
}

#[test]
fn templates_save_list_and_delete() {
    let (s, _, _, _) = session();
    let t = ModifyTemplateRow {
        name: "Accent".into(),
        rule: Some(RuleRow {
            conditions: vec![cond(
                ConditionTest::Kinds {
                    kinds: vec![EventKindRow::Note],
                },
                false,
            )],
            match_all: true,
        }),
        mode: SelectModeRow::Refine,
        modify: Some(ModifyRow::Change {
            target: ValueTargetRow::Velocity,
            op: ChangeOpRow::Add,
            amount: AmountRow::Line {
                from: 0,
                to: 4096,
                curve: CurveRow::EaseOut,
            },
        }),
        to_copy: true,
    };
    let snap = s
        .edit(
            EditCommand::Settings {
                edit: SettingsEdit::SaveTemplate {
                    template: t.clone(),
                },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Set Modify Template"));
    assert_eq!(s.modify_templates().unwrap(), vec![t]);
    s.edit(
        EditCommand::Settings {
            edit: SettingsEdit::DeleteTemplate {
                name: "Accent".into(),
            },
        },
        None,
    )
    .unwrap();
    assert!(s.modify_templates().unwrap().is_empty());
    let blank = ModifyTemplateRow {
        name: "  ".into(),
        rule: None,
        mode: SelectModeRow::Select,
        modify: None,
        to_copy: false,
    };
    assert!(matches!(
        s.edit(
            EditCommand::Settings {
                edit: SettingsEdit::SaveTemplate { template: blank }
            },
            None
        ),
        Err(VizError::Model { .. })
    ));
}

#[test]
fn every_modify_row_round_trips() {
    let osc = AmountRow::Oscillator {
        wave: WaveRow::Triangle,
        low: 0,
        high: 10,
        period: Q,
        phase: 0,
    };
    let all = vec![
        ModifyRow::TransposeInterval { semitones: -3 },
        ModifyRow::TransposeDiatonic {
            steps: 2,
            root: 0,
            mask: C_MAJOR,
        },
        ModifyRow::TransposeToScale {
            root: 2,
            mask: C_MAJOR,
        },
        ModifyRow::TransposeInvert { axis: 64 },
        ModifyRow::NoteMap {
            map: vec![NoteMapEntry { from: 36, to: 38 }],
        },
        ModifyRow::Quantize {
            grid: Q / 4,
            strength: 80,
            offsets: vec![],
        },
        ModifyRow::Quantize {
            grid: Q / 2,
            strength: 100,
            offsets: vec![0, 120],
        },
        velocity_set(5),
        ModifyRow::Change {
            target: ValueTargetRow::ReleaseVelocity,
            op: ChangeOpRow::Scale,
            amount: AmountRow::Line {
                from: 50,
                to: 150,
                curve: CurveRow::SCurve,
            },
        },
        ModifyRow::Change {
            target: ValueTargetRow::Duration,
            op: ChangeOpRow::Legato,
            amount: AmountRow::Constant { value: 0 },
        },
        ModifyRow::Change {
            target: ValueTargetRow::Value,
            op: ChangeOpRow::Limit { min: 1, max: 2 },
            amount: osc,
        },
        ModifyRow::Change {
            target: ValueTargetRow::Value,
            op: ChangeOpRow::Compress { low: 0, high: 9 },
            amount: AmountRow::Random {
                low: 1,
                high: 2,
                seed: 7,
            },
        },
        ModifyRow::Move { delta: -Q },
        ModifyRow::ScaleTime {
            num: 3,
            den: 2,
            anchor: Q,
        },
        ModifyRow::TrimStart { delta: 10 },
        ModifyRow::SetDest { dest: Some(3) },
        ModifyRow::SetDest { dest: None },
        ModifyRow::Substitute {
            to: SubstituteRow::Controller { index: 11 },
        },
        ModifyRow::Substitute {
            to: SubstituteRow::PitchBend,
        },
        ModifyRow::Substitute {
            to: SubstituteRow::ChannelPressure,
        },
        ModifyRow::Substitute {
            to: SubstituteRow::Pitch { pitch: 36 },
        },
        ModifyRow::Thin { spacing: Q / 8 },
        ModifyRow::Thicken { spacing: Q / 16 },
        ModifyRow::Delete,
    ];
    for m in all {
        assert_eq!(modify_row(&modify(&m).unwrap()), m);
    }
}

#[test]
fn every_condition_round_trips() {
    let tests = vec![
        ConditionTest::TimeRange { start: 0, end: Q },
        ConditionTest::Kinds {
            kinds: vec![EventKindRow::Note, EventKindRow::Cc],
        },
        ConditionTest::Pitch { low: 1, high: 2 },
        ConditionTest::Velocity { low: 3, high: 4 },
        ConditionTest::ReleaseVelocity { low: 5, high: 6 },
        ConditionTest::Duration { min: 7, max: 8 },
        ConditionTest::Value { low: 9, high: 10 },
        ConditionTest::Controller { low: 11, high: 12 },
        ConditionTest::Dest { dest: Some(4) },
        ConditionTest::Dest { dest: None },
        ConditionTest::Metric {
            grid: Q,
            offset: 0,
            tolerance: 10,
        },
        ConditionTest::EveryNth { n: 2, phase: 1 },
        ConditionTest::BracketedBy { low: 36, high: 36 },
    ];
    for match_all in [true, false] {
        let r = RuleRow {
            conditions: tests
                .iter()
                .enumerate()
                .map(|(i, t)| cond(t.clone(), i % 2 == 1))
                .collect(),
            match_all,
        };
        assert_eq!(rule_row(&rule(&r).unwrap()), Some(r));
    }
}

#[test]
fn legacy_modifications_read_as_change_rows() {
    assert_eq!(
        modify_row(&Modify::Velocity(ValueOp::Add(-5))),
        ModifyRow::Change {
            target: ValueTargetRow::Velocity,
            op: ChangeOpRow::Add,
            amount: AmountRow::Constant { value: -5 },
        }
    );
    assert_eq!(
        modify_row(&Modify::Duration(DurationOp::ScalePercent(50))),
        ModifyRow::Change {
            target: ValueTargetRow::Duration,
            op: ChangeOpRow::Scale,
            amount: AmountRow::Constant { value: 50 },
        }
    );
    assert_eq!(
        modify_row(&Modify::Legato),
        ModifyRow::Change {
            target: ValueTargetRow::Duration,
            op: ChangeOpRow::Legato,
            amount: AmountRow::Constant { value: 0 },
        }
    );
    assert_eq!(
        modify_row(&Modify::ControllerValue(ValueOp::Set(9))),
        ModifyRow::Change {
            target: ValueTargetRow::Value,
            op: ChangeOpRow::Set,
            amount: AmountRow::Constant { value: 9 },
        }
    );
}

#[test]
fn bad_rules_and_modifications_are_refused_and_change_nothing() {
    let (s, seq, lane, ids) = session();
    select(&s, seq, lane, ids.clone());
    let one = |test| RuleRow {
        conditions: vec![cond(test, false)],
        match_all: true,
    };
    let bad = vec![
        sm(
            Some(one(ConditionTest::Metric {
                grid: 0,
                offset: 0,
                tolerance: 0,
            })),
            None,
        ),
        sm(Some(one(ConditionTest::EveryNth { n: 0, phase: 0 })), None),
        sm(
            Some(one(ConditionTest::TimeRange { start: 5, end: 5 })),
            None,
        ),
        sm(
            None,
            Some(ModifyRow::TransposeDiatonic {
                steps: 1,
                root: 12,
                mask: C_MAJOR,
            }),
        ),
        sm(
            None,
            Some(ModifyRow::Change {
                target: ValueTargetRow::Velocity,
                op: ChangeOpRow::Set,
                amount: AmountRow::Oscillator {
                    wave: WaveRow::Sine,
                    low: 0,
                    high: 1,
                    period: 0,
                    phase: 0,
                },
            }),
        ),
        sm(None, Some(ModifyRow::Thin { spacing: 0 })),
    ];
    for cmd in bad {
        assert!(
            matches!(s.edit(cmd.clone(), None), Err(VizError::Model { .. })),
            "{cmd:?}"
        );
    }
    assert_eq!(pitches(&s, seq, lane), vec![60, 64, 67]);
    assert_eq!(s.snapshot().unwrap().undo_label, None);
}

#[test]
fn modify_again_is_not_a_gesture_command() {
    assert!(vizdaw_ffi::edit::to_command(&EditCommand::ModifyAgain).is_err());
}

/// A project saved before 4c has templates without `to_copy`; it still opens
/// through the FFI and reads them as not copying.
#[test]
fn a_project_saved_before_to_copy_existed_still_opens() {
    let (s, _, _, _) = session();
    let t = ModifyTemplateRow {
        name: "Old".into(),
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::Delete),
        to_copy: true,
    };
    s.edit(
        EditCommand::Settings {
            edit: SettingsEdit::SaveTemplate { template: t },
        },
        None,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let saved = dir.path().join("old.vizdaw");
    s.save_as(saved.to_string_lossy().into_owned()).unwrap();
    let file = saved.join("project.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    fn strip(v: &mut serde_json::Value) -> usize {
        match v {
            serde_json::Value::Object(m) => {
                let n = usize::from(m.remove("to_copy").is_some());
                n + m.values_mut().map(strip).sum::<usize>()
            }
            serde_json::Value::Array(a) => a.iter_mut().map(strip).sum(),
            _ => 0,
        }
    }
    assert_eq!(
        strip(&mut json),
        1,
        "the template's to_copy was in the file"
    );
    std::fs::write(&file, serde_json::to_string(&json).unwrap()).unwrap();
    let reopened = Session::open(saved.to_string_lossy().into_owned()).unwrap();
    let templates = reopened.modify_templates().unwrap();
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].name, "Old");
    assert!(!templates[0].to_copy);
}
