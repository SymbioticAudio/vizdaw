//! The Select and Modify panel's Strip Chart Line and Automation Target
//! condition (slice 10c Task 7; Swift `T/LinePanelTests.swift`,
//! `M/SelectModifyModel.swift:459-528`, `M/PanelFields.swift:183-260`).

mod support;

use std::sync::Arc;

use support::{FakeSession, mixer_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::list::PitchName;
use vizdaw_app::select_modify::{
    ConditionKind, FieldKind, LINE_OPS, LINE_TARGET_BASICS, ModifyKind, PanelTask, WaveKind,
    line_target_controllers, line_target_octaves, modify_fields, modify_numbers, modify_with,
    remap,
};
use vizdaw_app::strip_chart::{LineCurve, PeriodKind, ShapeKind, StripLane};
use vizdaw_ffi::edit::{EditCommand, LaneRef};
use vizdaw_ffi::events::{AutomationTargetRow as T, EventKindRow};
use vizdaw_ffi::modify::{ConditionTest, CurveRow, ModifyRow, ValueTargetRow, WaveRow};
use vizdaw_ffi::selection::SelectionRow;
use vizdaw_ffi::strip::{LineOpRow, LinePointRow, LineRow, LineShapeRow, LineTargetRow, PeriodRow};

const Q: i64 = 15_360;

/// `mixer_fake`, the panel open on Change set to Strip Chart Line, seed 9.
fn panel() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(mixer_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.set_modify_seed_for_test(9);
    m.open_modify(PanelTask::Change, None);
    m.set_modify_kind(ModifyKind::Line);
    (f, m)
}

fn line(m: &DocumentModel) -> (LineTargetRow, LineOpRow, LineRow) {
    match &m.select_modify().config.modify.op {
        ModifyRow::Line { target, op, line } => (*target, *op, line.clone()),
        other => panic!("not a line: {other:?}"),
    }
}

fn op(m: &DocumentModel) -> ModifyRow {
    m.select_modify().config.modify.op.clone()
}

#[test]
fn line_pickers_edit_the_line() {
    let (_f, mut m) = panel();
    m.set_modify_line_target(StripLane::Tempo);
    assert_eq!(line(&m).0, LineTargetRow::Tempo);
    assert_eq!(line(&m).1, LineOpRow::Set);
    assert_eq!(modify_fields(&op(&m))[2].kind, FieldKind::Bpm);
    m.set_modify_line_op(LineOpRow::Draw);
    assert_eq!(line(&m).1, LineOpRow::Draw);
    m.set_modify_line_shape(ShapeKind::Oscillator);
    assert_eq!(
        line(&m).2.shape,
        LineShapeRow::Oscillator {
            wave: WaveRow::Sine,
            period: PeriodRow::Ticks { ticks: Q },
            phase: 0
        }
    );
    m.set_modify_line_wave(WaveRow::Saw);
    m.set_modify_line_period_kind(PeriodKind::Bars);
    assert_eq!(
        line(&m).2.shape,
        LineShapeRow::Oscillator {
            wave: WaveRow::Saw,
            period: PeriodRow::Bars { bars: 1 },
            phase: 0
        }
    );
    let labels: Vec<_> = modify_fields(&op(&m)).iter().map(|f| f.label).collect();
    assert_eq!(
        labels[labels.len() - 2..],
        ["period (bars)", "phase (\u{2030})"]
    );
    m.set_modify_line_shape(ShapeKind::Random);
    assert_eq!(line(&m).2.shape, LineShapeRow::Random { seed: 9 });
    m.set_modify_line_shape(ShapeKind::Curved);
    m.set_modify_line_curve(CurveRow::EaseIn);
    assert_eq!(
        line(&m).2.shape,
        LineShapeRow::Curved {
            curve: CurveRow::EaseIn
        }
    );
    m.set_modify_line_shape(ShapeKind::Free);
    let l = line(&m).2;
    assert_eq!(
        l.shape,
        LineShapeRow::Free {
            points: vec![
                LinePointRow {
                    tick: l.from,
                    value: l.start
                },
                LinePointRow {
                    tick: l.to,
                    value: l.end
                },
            ]
        }
    );
    assert_eq!(ShapeKind::of(&l.shape), ShapeKind::Free);
    m.set_modify_line_op(LineOpRow::Legato);
    m.set_modify_line_target(StripLane::Velocity);
    assert_eq!(line(&m).1, LineOpRow::Set, "legato is for lengths only");
    m.set_modify_line_target(StripLane::Duration);
    m.set_modify_line_op(LineOpRow::Legato);
    assert_eq!(line(&m).1, LineOpRow::Legato);
}

#[test]
fn the_line_setters_clear_the_drafts() {
    let (_f, mut m) = panel();
    m.set_modify_field("m2", "nope");
    assert!(m.modify_field_invalid("m2") && m.select_modify().has_drafts());
    m.set_modify_line_target(StripLane::Tempo);
    assert!(!m.modify_field_invalid("m2") && !m.select_modify().has_drafts());
    m.set_modify_field("m2", "nope");
    m.set_modify_line_op(LineOpRow::Add);
    assert!(!m.select_modify().has_drafts());
    m.set_modify_field("m2", "nope");
    m.set_modify_line_shape(ShapeKind::Oscillator);
    assert!(!m.select_modify().has_drafts());
    m.set_modify_field("m5", "nope");
    m.set_modify_line_period_kind(PeriodKind::Clock);
    assert!(m.select_modify().invalid_keys().is_empty());
}

#[test]
fn automation_conditions_pick_their_target() {
    let (_f, mut m) = panel();
    m.add_modify_condition(ConditionKind::Automation);
    let i = m.select_modify().config.select.conditions.len() - 1;
    m.set_modify_condition_target(i, T::StripMute { strip: 1 });
    assert_eq!(
        m.select_modify().config.select.conditions[i].test,
        ConditionTest::Automation {
            target: T::StripMute { strip: 1 }
        }
    );
    let targets = m.automation_targets();
    assert_eq!(
        targets.menus.first().map(|g| g.title.as_str()),
        Some("Synth")
    );
    assert_eq!(
        targets.names.title(&T::StripMute { strip: 1 }),
        "Mute \u{b7} Synth"
    );
}

#[test]
fn shape_defaults_come_from_the_strip_chart_bar() {
    let (_f, mut m) = panel();
    m.update_strip_settings(|s| {
        s.curve = LineCurve::SCurve;
        s.wave = WaveKind::Triangle;
        s.period = PeriodKind::Clock;
        s.period_ms = 250;
        s.period_bars = 4;
    });
    m.set_modify_line_shape(ShapeKind::Curved);
    assert_eq!(
        line(&m).2.shape,
        LineShapeRow::Curved {
            curve: CurveRow::SCurve
        }
    );
    m.set_modify_line_shape(ShapeKind::Oscillator);
    assert_eq!(
        line(&m).2.shape,
        LineShapeRow::Oscillator {
            wave: WaveRow::Triangle,
            period: PeriodRow::Millis { ms: 250 },
            phase: 0
        }
    );
    m.set_modify_line_period_kind(PeriodKind::Bars);
    assert_eq!(
        line(&m).2.shape,
        LineShapeRow::Oscillator {
            wave: WaveRow::Triangle,
            period: PeriodRow::Bars { bars: 4 },
            phase: 0
        }
    );
}

#[test]
fn editing_a_free_lines_range_moves_its_points() {
    let pts = |p: &[(i64, i64)]| LineShapeRow::Free {
        points: p
            .iter()
            .map(|&(tick, value)| LinePointRow { tick, value })
            .collect(),
    };
    let free = LineRow {
        from: 0,
        to: 4 * Q,
        start: 0,
        end: 10,
        step: Q,
        shape: pts(&[(0, 0), (Q, 5), (4 * Q, 10)]),
    };
    let op = ModifyRow::Line {
        target: LineTargetRow::Value {
            target: ValueTargetRow::Velocity,
        },
        op: LineOpRow::Set,
        line: free.clone(),
    };
    let mut n = modify_numbers(&op);
    n[0] = 4 * Q;
    n[1] = 12 * Q;
    let Some(ModifyRow::Line { line: moved, .. }) = modify_with(&op, &n) else {
        panic!("not a line");
    };
    assert_eq!((moved.from, moved.to), (4 * Q, 12 * Q));
    assert_eq!(moved.shape, pts(&[(4 * Q, 0), (6 * Q, 5), (12 * Q, 10)]));
    // An unchanged range: the points stay put.
    let Some(ModifyRow::Line { line: same, .. }) = modify_with(&op, &modify_numbers(&op)) else {
        panic!("not a line");
    };
    assert_eq!(same.shape, free.shape);
    // A zero-width range maps to the new start.
    assert_eq!(remap(7, (5, 5), (100, 200)), 100);
    assert_eq!(remap(i64::MAX, (0, 10), (0, 20)), 20, "inside the range");
}

#[test]
fn line_numbers_are_clamped() {
    let (_f, mut m) = panel();
    m.set_modify_line_shape(ShapeKind::Oscillator);
    let op = op(&m);
    let n = modify_numbers(&op);
    assert_eq!(n.len(), 7);
    let Some(ModifyRow::Line { line: l, .. }) = modify_with(&op, &[5 * Q, Q, 0, 1, 0, 0, 4000])
    else {
        panic!("not a line");
    };
    assert_eq!(
        (l.from, l.to, l.step),
        (5 * Q, 5 * Q, 1),
        "to ≥ from, step ≥ 1"
    );
    assert_eq!(
        l.shape,
        LineShapeRow::Oscillator {
            wave: WaveRow::Sine,
            period: PeriodRow::Ticks { ticks: 1 },
            phase: 999
        }
    );
    assert_eq!(modify_with(&op, &[0; 5]), None, "one number per field");
}

#[test]
fn every_automation_target_can_be_a_condition_and_poly_pressure_a_line() {
    let (_f, mut m) = panel();
    m.add_modify_condition(ConditionKind::Automation);
    let i = m.select_modify().config.select.conditions.len() - 1;
    let all = [
        T::StripFader { strip: 1 },
        T::StripPan { strip: 1 },
        T::StripSolo { strip: 2 },
        T::SendLevel { strip: 1, send: 3 },
        T::PluginParam { slot: 10, param: 4 },
        T::VelocityScale { dest: 2 },
        T::VcaFader { vca: 1 },
    ];
    for t in all {
        m.set_modify_condition_target(i, t);
        assert_eq!(
            m.select_modify().config.select.conditions[i].test,
            ConditionTest::Automation { target: t }
        );
    }
    let octaves = line_target_octaves();
    let octave = octaves
        .iter()
        .find(|(_, lanes)| lanes.contains(&StripLane::PolyPressure(60)))
        .expect("an octave with 60");
    assert_eq!(octave.1.len(), 12);
    assert_eq!(octaves.len(), 11);
    assert_eq!(octaves.iter().map(|(_, l)| l.len()).sum::<usize>(), 128);
    assert_eq!(
        octaves[0].0,
        format!("{}\u{2013}{}", PitchName::name(0), PitchName::name(11))
    );
    assert_eq!(
        octaves[10].0,
        format!("{}\u{2013}{}", PitchName::name(120), PitchName::name(127))
    );
    m.set_modify_line_target(StripLane::PolyPressure(60));
    assert_eq!(line(&m).0, StripLane::PolyPressure(60).line_target());
}

#[test]
fn the_line_pickers_choices_are_swifts() {
    assert_eq!(
        LINE_TARGET_BASICS,
        [
            StripLane::Velocity,
            StripLane::ReleaseVelocity,
            StripLane::Duration,
            StripLane::PitchBend,
            StripLane::ChannelPressure,
            StripLane::Program,
            StripLane::Tempo,
        ]
    );
    let cc = line_target_controllers();
    assert_eq!(cc.len(), 128);
    assert_eq!(cc[0], StripLane::Controller(0));
    assert_eq!(cc[127], StripLane::Controller(127));
    let titles: Vec<_> = LINE_OPS.iter().map(|(t, _)| *t).collect();
    assert_eq!(
        titles,
        [
            "Draw",
            "Set",
            "Scale",
            "Add",
            "Max Limit",
            "Min Limit",
            "Compress",
            "Thin",
            "Thicken",
            "Legato"
        ]
    );
    assert_eq!(LINE_OPS[4].1, LineOpRow::LimitMax);
    assert_eq!(LINE_OPS[9].1, LineOpRow::Legato);
}

#[test]
fn applying_a_line_sends_one_select_and_modify() {
    let (f, mut m) = panel();
    m.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq: 3, lane: 31 }],
        events: vec![1],
    });
    m.select_modify_mut().config.modify.to_copy = true;
    f.clear_calls();
    assert!(m.apply_modify());
    let edits = f.edits();
    assert_eq!(edits.len(), 1);
    match edits.last() {
        Some(EditCommand::SelectAndModify {
            modify, to_copy, ..
        }) => {
            assert!(matches!(modify, Some(ModifyRow::Line { .. })));
            assert!(!to_copy, "a line never goes to a copy");
        }
        other => panic!("nothing sent: {other:?}"),
    }
}

#[test]
fn the_kind_condition_can_pick_automation() {
    let (_f, mut m) = panel();
    m.add_modify_condition(ConditionKind::Kind);
    let i = m.select_modify().config.select.conditions.len() - 1;
    m.set_modify_kinds(i, vec![EventKindRow::Note, EventKindRow::Automation]);
    assert_eq!(
        m.select_modify().config.select.conditions[i].test,
        ConditionTest::Kinds {
            kinds: vec![EventKindRow::Note, EventKindRow::Automation]
        }
    );
}
