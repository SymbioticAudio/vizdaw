//! The Select and Modify panel's vocabulary and config (slice 10b Task 1;
//! Swift `SelectModifyTests.swift`).

use vizdaw_app::editing_prefs::ScaleKind;
use vizdaw_app::select_modify::{
    AmountKind, ChangeOpKind, ChangeTarget, Comparison, ConditionKind, CurveKind,
    MODIFY_AGAIN_HELP, ModifyConfig, ModifyKind, PANEL_TITLE_PREFIX, PanelTask, SubstituteChoice,
    WaveKind, modify_again_title, modify_title, reseeded,
};
use vizdaw_ffi::edit::EditCommand;
use vizdaw_ffi::events::{AutomationTargetRow, EventKindRow};
use vizdaw_ffi::modify::{
    AmountRow, ChangeOpRow, ConditionRow, ConditionTest, CurveRow, ModifyRow, RuleRow,
    SelectModeRow, SubstituteRow, ValueTargetRow, WaveRow,
};
use vizdaw_ffi::strip::{LineOpRow, LineRow, LineShapeRow, LineTargetRow, SeriesRow};

const Q: i64 = 15_360;

fn cond(test: ConditionTest) -> ConditionRow {
    ConditionRow {
        test,
        negate: false,
    }
}

fn line_op() -> ModifyRow {
    ModifyRow::Line {
        target: LineTargetRow::Value {
            target: ValueTargetRow::Velocity,
        },
        op: LineOpRow::Set,
        line: LineRow {
            from: 0,
            to: 4 * Q,
            start: 0,
            end: 0xFFFF,
            step: Q / 8,
            shape: LineShapeRow::Straight,
        },
    }
}

#[test]
fn presets_make_their_tasks_command() {
    assert_eq!(
        ModifyConfig::preset(PanelTask::Transpose, None).command(),
        Some(EditCommand::SelectAndModify {
            rule: None,
            mode: SelectModeRow::Select,
            modify: Some(ModifyRow::TransposeInterval { semitones: 0 }),
            to_copy: false,
        })
    );
    assert_eq!(
        ModifyConfig::preset(PanelTask::Quantize, None).modify.op,
        ModifyRow::Quantize {
            grid: Q / 4,
            strength: 100,
            offsets: vec![]
        }
    );
    assert_eq!(
        ModifyConfig::preset(PanelTask::Move, None).modify.op,
        ModifyRow::Move { delta: 0 }
    );
    let change = ModifyRow::Change {
        target: ValueTargetRow::Velocity,
        op: ChangeOpRow::Add,
        amount: AmountRow::Constant { value: 0 },
    };
    assert_eq!(
        ModifyConfig::preset(PanelTask::Change, None).modify.op,
        change
    );
    assert_eq!(
        ModifyConfig::default(),
        ModifyConfig::preset(PanelTask::Change, None)
    );
    let rule = ModifyConfig::preset(PanelTask::SelectByRule, Some((0, 4 * Q)));
    assert_eq!(
        rule.command(),
        Some(EditCommand::SelectAndModify {
            rule: Some(RuleRow {
                conditions: vec![
                    cond(ConditionTest::Kinds {
                        kinds: vec![EventKindRow::Note]
                    }),
                    cond(ConditionTest::TimeRange {
                        start: 0,
                        end: 4 * Q
                    }),
                ],
                match_all: true,
            }),
            mode: SelectModeRow::Select,
            modify: None,
            to_copy: false,
        })
    );
    let no_range = ModifyConfig::preset(PanelTask::SelectByRule, None);
    assert_eq!(no_range.select.conditions.len(), 1);
    assert!(!no_range.modify.enabled);

    assert_eq!(
        PanelTask::ALL.map(PanelTask::title),
        [
            "Select by Rule",
            "Transpose",
            "Quantize",
            "Change Values",
            "Move"
        ]
    );
    assert_eq!(
        PanelTask::ALL.map(PanelTask::command_id),
        [
            "modify.select_by_rule",
            "modify.transpose",
            "modify.quantize",
            "modify.change",
            "modify.move"
        ]
    );
    assert_eq!(
        MODIFY_AGAIN_HELP,
        "Modify Again changes the selection in place."
    );
    assert_eq!(PANEL_TITLE_PREFIX, "Select and Modify \u{2014} ");
}

#[test]
fn nothing_turned_on_is_no_command() {
    let mut c = ModifyConfig::preset(PanelTask::Change, None);
    c.modify.enabled = false;
    assert_eq!(c.command(), None);
    c.select.enabled = true;
    assert_eq!(
        c.command(),
        None,
        "a select half without conditions does nothing"
    );
    assert_eq!(c.rule(), None);
    assert_eq!(c.modification(), None);
}

#[test]
fn templates_round_trip() {
    let mut c = ModifyConfig::preset(PanelTask::SelectByRule, Some((0, Q)));
    c.select.mode = SelectModeRow::Refine;
    c.select.match_all = false;
    c.modify.enabled = true;
    c.modify.op = ModifyRow::Change {
        target: ValueTargetRow::Velocity,
        op: ChangeOpRow::Add,
        amount: AmountRow::Line {
            from: 0,
            to: 2048,
            curve: CurveRow::EaseIn,
        },
    };
    c.modify.to_copy = true;
    let t = c.template("Swell");
    assert!(t.name == "Swell" && t.to_copy && t.mode == SelectModeRow::Refine);
    assert_eq!(ModifyConfig::from_template(&t), c);

    let mut octave = ModifyConfig::preset(PanelTask::Transpose, None);
    octave.modify.op = ModifyRow::TransposeInterval { semitones: 12 };
    let t = octave.template("Octave Up");
    assert_eq!(t.rule, None);
    assert_eq!(ModifyConfig::from_template(&t), octave);
}

#[test]
fn comparisons_read_and_set_ranges() {
    let hundred = cond(ConditionTest::Velocity {
        low: 100 << 9,
        high: (100 << 9) | 511,
    });
    assert_eq!(Comparison::of(&hundred), Comparison::Is);
    assert_eq!(
        Comparison::of(&Comparison::Outside.applied(&hundred)),
        Comparison::IsNot,
        "still one step"
    );
    let wide = cond(ConditionTest::Velocity {
        low: 50 << 9,
        high: 0xFFFF,
    });
    assert_eq!(Comparison::of(&wide), Comparison::Inside);
    assert_eq!(
        Comparison::Is.applied(&wide).test,
        ConditionTest::Velocity {
            low: 50 << 9,
            high: (50 << 9) | 511
        }
    );
    assert!(Comparison::Outside.applied(&wide).negate);
    assert_eq!(
        Comparison::choices(&ConditionTest::TimeRange { start: 0, end: 1 }),
        &[Comparison::Inside, Comparison::Outside]
    );
    assert_eq!(
        Comparison::choices(&ConditionTest::Kinds {
            kinds: vec![EventKindRow::Note]
        }),
        &[Comparison::Is, Comparison::IsNot]
    );
    assert_eq!(
        Comparison::choices(&ConditionTest::Pitch { low: 0, high: 1 }),
        &[
            Comparison::Is,
            Comparison::IsNot,
            Comparison::Inside,
            Comparison::Outside
        ]
    );
    assert_eq!(
        Comparison::of(&ConditionRow {
            test: ConditionTest::Dest { dest: Some(5) },
            negate: true
        }),
        Comparison::IsNot
    );
    assert_eq!(
        [
            Comparison::Is,
            Comparison::IsNot,
            Comparison::Inside,
            Comparison::Outside
        ]
        .map(Comparison::title),
        ["is", "is not", "inside", "outside"]
    );
    // Value: a 7-bit step is 2^25 wide.
    let v = cond(ConditionTest::Value {
        low: 3 << 25,
        high: u32::MAX,
    });
    assert_eq!(
        Comparison::Is.applied(&v).test,
        ConditionTest::Value {
            low: 3 << 25,
            high: (3 << 25) | 0x1FF_FFFF
        }
    );
    // Length: one beat to two reads as inside; "is" makes them one length.
    let d = cond(ConditionTest::Duration { min: Q, max: 2 * Q });
    assert_eq!(Comparison::of(&d), Comparison::Inside);
    assert_eq!(
        Comparison::IsNot.applied(&d),
        ConditionRow {
            test: ConditionTest::Duration { min: Q, max: Q },
            negate: true
        }
    );
}

#[test]
fn choosing_is_on_a_range_sets_high_to_low() {
    let pitch = cond(ConditionTest::Pitch { low: 60, high: 72 });
    assert_eq!(
        Comparison::Is.applied(&pitch),
        ConditionRow {
            test: ConditionTest::Pitch { low: 60, high: 60 },
            negate: false
        }
    );
    let negated = ConditionRow {
        test: ConditionTest::Controller { low: 7, high: 10 },
        negate: true,
    };
    assert_eq!(
        Comparison::Is.applied(&negated),
        cond(ConditionTest::Controller { low: 7, high: 7 })
    );
    // Inside leaves the range alone.
    assert_eq!(Comparison::Inside.applied(&pitch), pitch);
}

#[test]
fn kinds_round_trip_through_their_defaults() {
    for k in ConditionKind::ALL {
        assert_eq!(ConditionKind::of(&k.default_test(None)), k, "{k:?}");
    }
    for k in ModifyKind::ALL {
        assert_eq!(ModifyKind::of(&k.default_op()), k, "{k:?}");
    }
    assert_eq!(ModifyKind::of(&line_op()), ModifyKind::Line);
    assert_eq!(
        modify_title(&ModifyRow::Change {
            target: ValueTargetRow::Duration,
            op: ChangeOpRow::Add,
            amount: AmountRow::Constant { value: 1 }
        }),
        "Change Duration"
    );
    assert_eq!(
        modify_title(&ModifyRow::TransposeDiatonic {
            steps: 1,
            root: 0,
            mask: 1
        }),
        "Transpose"
    );
    assert_eq!(
        modify_title(&ModifyRow::SetDest { dest: None }),
        "Reassign Destination"
    );
    assert_eq!(modify_title(&line_op()), "Change Velocity");
    assert_eq!(
        modify_title(&ModifyRow::Line {
            target: LineTargetRow::Series {
                series: SeriesRow::Program
            },
            op: LineOpRow::Draw,
            line: LineRow {
                from: 0,
                to: Q,
                start: 0,
                end: 1,
                step: Q,
                shape: LineShapeRow::Straight
            }
        }),
        "Draw Program Changes"
    );
    for k in ChangeOpKind::ALL {
        assert_eq!(ChangeOpKind::of(&k.row(ValueTargetRow::Velocity)), k);
    }
    assert_eq!(
        ChangeOpKind::Limit.row(ValueTargetRow::Duration),
        ChangeOpRow::Limit { min: 0, max: 4 * Q }
    );
    for k in AmountKind::ALL {
        assert_eq!(AmountKind::of(&k.row(1)), k);
    }
    for k in ChangeTarget::ALL {
        assert_eq!(ChangeTarget::of(k.row()), k);
    }
    for k in CurveKind::ALL {
        assert_eq!(CurveKind::of(k.row()), k);
    }
    for k in WaveKind::ALL {
        assert_eq!(WaveKind::of(k.row()), k);
    }
    for k in SubstituteChoice::ALL {
        assert_eq!(SubstituteChoice::of(&k.row()), k);
    }
    assert_eq!(
        SubstituteChoice::Pitch.row(),
        SubstituteRow::Pitch { pitch: 36 }
    );
    assert_eq!(
        ScaleKind::of_mask(ScaleKind::Dorian.mask()),
        Some(ScaleKind::Dorian)
    );
    assert_eq!(ScaleKind::of_mask(1), None);

    assert_eq!(
        ConditionKind::ALL.map(ConditionKind::title),
        [
            "Position",
            "Kind",
            "Pitch",
            "Velocity",
            "Release Velocity",
            "Length",
            "Value",
            "Controller",
            "Destination",
            "Metric Position",
            "Every Nth",
            "Inside a Note",
            "Automation Target"
        ]
    );
    assert_eq!(
        ModifyKind::ALL.map(ModifyKind::title),
        [
            "Transpose",
            "Transpose in Scale",
            "Snap to Scale",
            "Invert",
            "Note Map",
            "Quantize",
            "Change Values",
            "Move",
            "Scale Time",
            "Trim Start",
            "Set Destination",
            "Substitute",
            "Thin Out",
            "Thicken",
            "Delete",
            "Strip Chart Line"
        ]
    );
    assert_eq!(
        ChangeTarget::ALL.map(ChangeTarget::title),
        ["Velocity", "Release Velocity", "Length", "Value"]
    );
    assert_eq!(
        ChangeOpKind::ALL.map(ChangeOpKind::title),
        [
            "Set to",
            "Scale by",
            "Add",
            "Limit to",
            "Compress to",
            "Legato"
        ]
    );
    assert_eq!(
        AmountKind::ALL.map(AmountKind::title),
        ["Constant", "Along a Line", "Oscillating", "Random"]
    );
    assert_eq!(
        CurveKind::ALL.map(CurveKind::title),
        ["Straight", "Ease In", "Ease Out", "S-Curve"]
    );
    assert_eq!(
        WaveKind::ALL.map(WaveKind::title),
        ["Sine", "Square", "Triangle", "Saw"]
    );
    assert_eq!(
        SubstituteChoice::ALL.map(SubstituteChoice::title),
        ["Controller", "Pitch Bend", "Channel Pressure", "One Pitch"]
    );
}

#[test]
fn condition_defaults_match_swift() {
    assert_eq!(
        ConditionKind::Time.default_test(None),
        ConditionTest::TimeRange {
            start: 0,
            end: 16 * Q
        }
    );
    assert_eq!(
        ConditionKind::Time.default_test(Some((Q, 2 * Q))),
        ConditionTest::TimeRange {
            start: Q,
            end: 2 * Q
        }
    );
    assert_eq!(
        ConditionKind::Metric.default_test(None),
        ConditionTest::Metric {
            grid: Q,
            offset: 0,
            tolerance: Q / 16
        }
    );
    assert_eq!(
        ConditionKind::Automation.default_test(None),
        ConditionTest::Automation {
            target: AutomationTargetRow::StripFader { strip: 0 }
        }
    );
    assert_eq!(
        ModifyKind::Diatonic.default_op(),
        ModifyRow::TransposeDiatonic {
            steps: 1,
            root: 0,
            mask: ScaleKind::Major.mask()
        }
    );
}

#[test]
fn the_line_and_automation_kinds_are_offered() {
    // Swift's `allCases` (Ruling 8).
    assert_eq!(ConditionKind::OFFERED, ConditionKind::ALL);
    assert_eq!(ConditionKind::OFFERED.len(), 13);
    assert_eq!(ModifyKind::OFFERED, ModifyKind::ALL);
    assert_eq!(ModifyKind::OFFERED.len(), 16);
}

#[test]
fn a_line_modification_is_never_sent_to_a_copy() {
    let mut c = ModifyConfig::preset(PanelTask::Transpose, None);
    c.modify.op = line_op();
    c.modify.to_copy = true;
    let Some(EditCommand::SelectAndModify {
        to_copy, modify, ..
    }) = c.command()
    else {
        panic!("a line is still a command");
    };
    assert!(!to_copy);
    assert_eq!(modify, Some(line_op()));
    // The template keeps what the panel holds.
    assert!(c.template("Line").to_copy);

    c.modify.op = ModifyRow::TransposeInterval { semitones: 4 };
    let Some(EditCommand::SelectAndModify { to_copy, .. }) = c.command() else {
        panic!("a transpose is a command");
    };
    assert!(to_copy);

    // A select-only command never goes to a copy.
    let mut s = ModifyConfig::preset(PanelTask::SelectByRule, None);
    s.modify.to_copy = true;
    let Some(EditCommand::SelectAndModify { to_copy, .. }) = s.command() else {
        panic!("a rule is a command");
    };
    assert!(!to_copy);
}

#[test]
fn modify_again_names_the_last_modify() {
    assert_eq!(
        modify_again_title(Some(&ModifyRow::TransposeDiatonic {
            steps: 2,
            root: 0,
            mask: ScaleKind::Major.mask()
        })),
        "Modify Again: Transpose"
    );
    assert_eq!(
        modify_again_title(Some(&ModifyRow::Change {
            target: ValueTargetRow::Velocity,
            op: ChangeOpRow::Set,
            amount: AmountRow::Constant { value: 0 }
        })),
        "Modify Again: Change Velocity"
    );
    assert_eq!(modify_again_title(None), "Modify Again");
    assert_eq!(
        modify_again_title(Some(&ModifyRow::Delete)),
        "Modify Again: Delete Events"
    );
}

#[test]
fn random_amounts_get_the_given_seed() {
    let cmd = |seed| EditCommand::SelectAndModify {
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::Change {
            target: ValueTargetRow::Velocity,
            op: ChangeOpRow::Add,
            amount: AmountRow::Random {
                low: -10,
                high: 10,
                seed,
            },
        }),
        to_copy: true,
    };
    assert_eq!(reseeded(cmd(1), 99), cmd(99));
    let constant = EditCommand::SelectAndModify {
        rule: None,
        mode: SelectModeRow::Add,
        modify: Some(ModifyRow::Change {
            target: ValueTargetRow::Value,
            op: ChangeOpRow::Set,
            amount: AmountRow::Oscillator {
                wave: WaveRow::Saw,
                low: 0,
                high: 1,
                period: Q,
                phase: 0,
            },
        }),
        to_copy: false,
    };
    assert_eq!(reseeded(constant.clone(), 7), constant);
    assert_eq!(
        reseeded(EditCommand::ModifyAgain, 7),
        EditCommand::ModifyAgain
    );
}

#[test]
fn scale_masks_match_swift() {
    assert_eq!(
        ScaleKind::ALL.map(ScaleKind::mask),
        [
            0b1010_1011_0101,
            0b0101_1010_1101,
            0b1001_1010_1101,
            0b0110_1010_1101,
            0b0110_1011_0101,
            0b0010_1001_0101,
            0b0100_1010_1001,
        ]
    );
}
