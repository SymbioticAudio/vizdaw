//! Swift `StripChartTests` (all but the exact-mode tests, which are Task
//! 7's) and the bar pickers and saved rows of `StripChartModelTests`: the
//! Strip Chart bar's settings, strokes as lines and commands, and the
//! saved layout.

use serde_json::{Value, json};
use vizdaw_app::editor::{DetailEditorKind, DetailState, EditorLayout, EditorTarget};
use vizdaw_app::select_modify::WaveKind;
use vizdaw_app::strip_chart::{
    LineCurve, MAX_ROWS, PeriodKind, ShapeKind, StripBarChoices, StripChartSettings, StripLane,
    StripMode, StripOp, StripPoint, compress_command, controllers_key, erase_command, line_command,
    stroke_label, stroke_line, stroke_op,
};
use vizdaw_ffi::edit::EditCommand;
use vizdaw_ffi::events::{AutomationTargetRow, EventKindRow};
use vizdaw_ffi::modify::{
    ConditionRow, ConditionTest, CurveRow, ModifyRow, RuleRow, SelectModeRow, ValueTargetRow,
    WaveRow,
};
use vizdaw_ffi::strip::{
    LineOpRow, LinePointRow, LineRow, LineShapeRow, LineTargetRow, PeriodRow, SeriesRow,
};

const Q: i64 = 15_360;

fn pt(tick: i64, value: i64) -> StripPoint {
    StripPoint { tick, value }
}

fn time_rule(start: i64, end: i64) -> RuleRow {
    RuleRow {
        conditions: vec![ConditionRow {
            test: ConditionTest::TimeRange { start, end },
            negate: false,
        }],
        match_all: true,
    }
}

fn settings(json: &str) -> StripChartSettings {
    StripChartSettings::decode(&serde_json::from_str(json).unwrap())
}

#[test]
fn a_backward_drag_is_turned_around() {
    let s = StripChartSettings::default();
    let l = stroke_line(pt(2 * Q, 10), pt(0, 50), &[], &s, 1);
    assert_eq!(
        l,
        LineRow {
            from: 0,
            to: 2 * Q,
            start: 50,
            end: 10,
            step: Q / 8,
            shape: LineShapeRow::Straight,
        }
    );
}

#[test]
fn a_click_draws_one_point() {
    let l = stroke_line(pt(Q, 7), pt(Q, 7), &[], &StripChartSettings::default(), 1);
    assert!(l.from == Q && l.to == Q && l.start == 7);
    let cmd = line_command(StripLane::Controller(7), LineOpRow::Draw, l.clone(), false);
    assert_eq!(
        cmd,
        EditCommand::SelectAndModify {
            rule: Some(time_rule(Q, Q + 1)),
            mode: SelectModeRow::Select,
            modify: Some(ModifyRow::Line {
                target: LineTargetRow::Series {
                    series: SeriesRow::Controller { index: 7 },
                },
                op: LineOpRow::Draw,
                line: l,
            }),
            to_copy: false,
        }
    );
}

#[test]
fn shapes_come_from_the_settings() {
    let mut s = StripChartSettings {
        shape: ShapeKind::Flat,
        ..StripChartSettings::default()
    };
    let flat = stroke_line(pt(Q, 30), pt(2 * Q, 90), &[], &s, 1);
    assert!(
        flat.start == 30 && flat.end == 30 && flat.shape == LineShapeRow::Flat,
        "flat holds the pressed value"
    );
    s.shape = ShapeKind::Curved;
    s.curve = LineCurve::SCurve;
    assert_eq!(
        stroke_line(pt(0, 0), pt(Q, 1), &[], &s, 1).shape,
        LineShapeRow::Curved {
            curve: CurveRow::SCurve
        }
    );
    s.shape = ShapeKind::Random;
    assert_eq!(
        stroke_line(pt(0, 0), pt(Q, 1), &[], &s, 42).shape,
        LineShapeRow::Random { seed: 42 }
    );
    s.shape = ShapeKind::Oscillator;
    s.wave = WaveKind::Triangle;
    s.period = PeriodKind::Clock;
    s.period_ms = 250;
    s.phase = 5000;
    assert_eq!(
        stroke_line(pt(0, 0), pt(Q, 1), &[], &s, 1).shape,
        LineShapeRow::Oscillator {
            wave: WaveRow::Triangle,
            period: PeriodRow::Millis { ms: 250 },
            phase: 999,
        }
    );
    s.period = PeriodKind::Bars;
    s.period_bars = 0;
    assert_eq!(
        s.period_row(),
        PeriodRow::Bars { bars: 1 },
        "a period of 0 becomes 1"
    );
    s.shape = ShapeKind::Free;
    let free = stroke_line(
        pt(0, 5),
        pt(3 * Q, 9),
        &[pt(0, 5), pt(2 * Q, 7), pt(Q, 1), pt(Q, 2), pt(3 * Q, 9)],
        &s,
        1,
    );
    let p = |tick, value| LinePointRow { tick, value };
    assert_eq!(
        free,
        LineRow {
            from: 0,
            to: 3 * Q,
            start: 5,
            end: 9,
            step: Q / 8,
            shape: LineShapeRow::Free {
                points: vec![p(0, 5), p(Q, 2), p(2 * Q, 7), p(3 * Q, 9)],
            },
        }
    );
    // A free click with no stroke goes through its two ends.
    let click = stroke_line(pt(Q, 4), pt(Q, 4), &[], &s, 1);
    assert_eq!(
        click.shape,
        LineShapeRow::Free {
            points: vec![p(Q, 4)]
        }
    );
    // The pickers read a row back.
    for k in ShapeKind::ALL {
        let s = StripChartSettings {
            shape: k,
            ..StripChartSettings::default()
        };
        assert_eq!(ShapeKind::of(&s.shape_row(1)), k);
    }
    for k in [PeriodKind::Musical, PeriodKind::Clock, PeriodKind::Bars] {
        let s = StripChartSettings {
            period: k,
            ..StripChartSettings::default()
        };
        assert_eq!(PeriodKind::of(&s.period_row()), k);
    }
    assert_eq!(
        ShapeKind::ALL.map(ShapeKind::title),
        ["Straight", "Curved", "Flat", "Free", "Random", "Oscillator"]
    );
    assert_eq!(
        LineCurve::ALL.map(LineCurve::title),
        ["Ease In", "Ease Out", "S-Curve"]
    );
    assert_eq!(
        LineCurve::ALL.map(LineCurve::row),
        [CurveRow::EaseIn, CurveRow::EaseOut, CurveRow::SCurve]
    );
    assert_eq!(PeriodKind::Bars.title(), "Bars (follow meter)");
}

#[test]
fn commands_follow_the_mode_operation_and_modifier() {
    let mut s = StripChartSettings::default();
    assert_eq!(stroke_op(StripLane::Controller(1), &s), LineOpRow::Draw);
    assert_eq!(
        stroke_op(StripLane::Velocity, &s),
        LineOpRow::Set,
        "drawing a note value sets it"
    );
    s.mode = StripMode::Modify;
    s.op = StripOp::Scale;
    assert_eq!(stroke_op(StripLane::Velocity, &s), LineOpRow::Scale);
    let l = LineRow {
        from: 0,
        to: Q,
        start: 0,
        end: 1,
        step: 1,
        shape: LineShapeRow::Straight,
    };
    let EditCommand::SelectAndModify { mode, .. } =
        line_command(StripLane::Velocity, LineOpRow::Set, l, true)
    else {
        panic!("expected selectAndModify");
    };
    assert_eq!(
        mode,
        SelectModeRow::Refine,
        "⌥ limits the line to the selected events"
    );
    assert_eq!(
        stroke_label(StripLane::Controller(1), LineOpRow::Draw),
        "Draw Controllers"
    );
    assert_eq!(
        stroke_label(StripLane::Tempo, LineOpRow::Draw),
        "Draw Tempo"
    );
    assert_eq!(
        stroke_label(
            StripLane::Automation(AutomationTargetRow::StripFader { strip: 1 }),
            LineOpRow::Draw
        ),
        "Draw Automation"
    );
    assert_eq!(
        stroke_label(StripLane::Velocity, LineOpRow::Set),
        "Change Velocity"
    );
}

#[test]
fn a_zero_width_compress_is_one_tick() {
    let c = compress_command(StripLane::Velocity, pt(Q, 900), pt(Q, 100), 0, false);
    let line = LineRow {
        from: Q,
        to: Q,
        start: 100,
        end: 900,
        step: 1,
        shape: LineShapeRow::Flat,
    };
    assert_eq!(
        c,
        EditCommand::SelectAndModify {
            rule: Some(time_rule(Q, Q + 1)),
            mode: SelectModeRow::Select,
            modify: Some(ModifyRow::Line {
                target: LineTargetRow::Value {
                    target: ValueTargetRow::Velocity
                },
                op: LineOpRow::Compress,
                line,
            }),
            to_copy: false,
        }
    );
}

#[test]
fn erase_deletes_the_rows_series_only() {
    // Pinned literally, not against `erase_rule` itself (parked T11).
    let rule = RuleRow {
        conditions: vec![
            ConditionRow {
                test: ConditionTest::TimeRange {
                    start: 0,
                    end: Q + 1,
                },
                negate: false,
            },
            ConditionRow {
                test: ConditionTest::Kinds {
                    kinds: vec![EventKindRow::PitchBend],
                },
                negate: false,
            },
        ],
        match_all: true,
    };
    assert_eq!(
        erase_command(StripLane::PitchBend, Q, 0),
        Some(EditCommand::SelectAndModify {
            rule: Some(rule),
            mode: SelectModeRow::Select,
            modify: Some(ModifyRow::Delete),
            to_copy: false,
        })
    );
    assert_eq!(erase_command(StripLane::Velocity, 0, Q), None);
}

#[test]
fn a_backward_compress_rectangle_is_turned_around() {
    // Parked T11: ticks as well as values are ordered.
    let c = compress_command(StripLane::Velocity, pt(2 * Q, 900), pt(Q, 100), 4, false);
    let line = LineRow {
        from: Q,
        to: 2 * Q,
        start: 100,
        end: 900,
        step: 4,
        shape: LineShapeRow::Flat,
    };
    let EditCommand::SelectAndModify {
        rule: Some(rule),
        modify:
            Some(ModifyRow::Line {
                op: LineOpRow::Compress,
                line: got,
                ..
            }),
        ..
    } = c
    else {
        panic!("not a compress");
    };
    assert_eq!(got, line);
    assert_eq!(
        rule.conditions.first().map(|c| c.test.clone()),
        Some(ConditionTest::TimeRange {
            start: Q,
            end: 2 * Q + 1
        })
    );
}

#[test]
fn a_flat_line_drawn_right_to_left_holds_the_pressed_value() {
    let s = StripChartSettings {
        shape: ShapeKind::Flat,
        ..StripChartSettings::default()
    };
    let l = stroke_line(pt(2 * Q, 30), pt(Q, 90), &[], &s, 1);
    assert_eq!(
        l,
        LineRow {
            from: Q,
            to: 2 * Q,
            start: 30,
            end: 30,
            step: Q / 8,
            shape: LineShapeRow::Flat,
        }
    );
}

#[test]
fn settings_decode_tolerantly() {
    let s = settings(r#"{"mode":"modify","rowHeight":5000,"shape":"bogus","periodMs":250}"#);
    assert!(
        s.mode == StripMode::Modify
            && s.row_height == 400.
            && s.shape == ShapeKind::Straight
            && s.period_ms == 250
    );
    let mut l = EditorLayout::default();
    l.strip_chart.op = StripOp::Thin;
    assert_eq!(
        EditorLayout::decode(Some(&l.encoded())).strip_chart.op,
        StripOp::Thin
    );
    assert_eq!(
        EditorLayout::decode(Some("{}")).strip_chart,
        StripChartSettings::default()
    );
    // Parked T11: zero periods, a phase past 999, a bad zoom.
    let o = settings(
        r#"{"periodTicks":0,"periodMs":0,"periodBars":0,"phase":4000,"step":0,"ticksPerPoint":-3}"#,
    );
    assert!(o.period_ticks == 1 && o.period_ms == 1 && o.period_bars == 1);
    assert!(o.phase == 999 && o.step == 1);
    assert_eq!(
        o.ticks_per_point,
        StripChartSettings::default().ticks_per_point,
        "an insane zoom falls back"
    );
    let f = settings(r#"{"ticksPerPoint":1e12}"#);
    assert!(EditorLayout::TICKS_PER_POINT_RANGE.contains(&f.ticks_per_point));
    // The defaults, and anything but an object.
    let d = StripChartSettings::default();
    assert_eq!(
        (d.mode, d.op, d.shape, d.curve, d.wave, d.period),
        (
            StripMode::Draw,
            StripOp::Set,
            ShapeKind::Straight,
            LineCurve::EaseOut,
            WaveKind::Sine,
            PeriodKind::Musical
        )
    );
    assert_eq!(
        (
            d.period_ticks,
            d.period_ms,
            d.period_bars,
            d.phase,
            d.step,
            d.row_height,
            d.ticks_per_point
        ),
        (Q, 500, 1, 0, Q / 8, 96., 96.)
    );
    assert_eq!(StripChartSettings::decode(&json!([1, 2])), d);
    assert_eq!(StripChartSettings::decode(&json!({"rowHeight": "x"})), d);
    // Every key under Swift's name.
    let all = settings(
        r#"{"mode":"erase","op":"limitMax","shape":"oscillator","curve":"sCurve","wave":"saw",
            "period":"bars","periodTicks":7,"periodMs":9,"periodBars":3,"phase":12,"step":5,
            "rowHeight":50,"ticksPerPoint":10}"#,
    );
    assert_eq!(
        all,
        StripChartSettings {
            mode: StripMode::Erase,
            op: StripOp::LimitMax,
            shape: ShapeKind::Oscillator,
            curve: LineCurve::SCurve,
            wave: WaveKind::Saw,
            period: PeriodKind::Bars,
            period_ticks: 7,
            period_ms: 9,
            period_bars: 3,
            phase: 12,
            step: 5,
            row_height: 50.,
            ticks_per_point: 10.,
        }
    );
    assert_eq!(
        StripChartSettings::decode(&serde_json::to_value(all).unwrap()),
        all
    );
}

#[test]
fn bar_pickers_keep_an_out_of_list_value() {
    let labels = |v: Vec<(String, i64)>| v.last().cloned();
    assert_eq!(
        StripBarChoices::steps(Q / 8).len(),
        StripBarChoices::STEPS.len()
    );
    assert_eq!(
        labels(StripBarChoices::steps(100)),
        Some(("100 ticks".into(), 100))
    );
    assert_eq!(
        labels(StripBarChoices::musical_periods(7)),
        Some(("7 ticks".into(), 7))
    );
    assert_eq!(
        StripBarChoices::clock_periods(333).last().cloned(),
        Some(("333 ms".into(), 333))
    );
    assert_eq!(
        StripBarChoices::clock_periods(500).len(),
        StripBarChoices::CLOCK_PERIODS.len()
    );
    assert_eq!(
        StripBarChoices::bar_periods(3).last().cloned(),
        Some(("3 bars".into(), 3))
    );
    assert_eq!(
        StripBarChoices::bar_periods(1).first().cloned(),
        Some(("1 bar".into(), 1))
    );
    assert_eq!(
        StripBarChoices::phases(5).last().cloned(),
        Some(("1.8°".into(), 5))
    );
    assert_eq!(
        StripBarChoices::phases(125)
            .into_iter()
            .map(|c| c.0)
            .collect::<Vec<_>>(),
        ["0°", "45°", "90°", "180°", "270°"]
    );
    assert_eq!(
        StripBarChoices::steps(Q)
            .into_iter()
            .map(|c| c.0)
            .collect::<Vec<_>>(),
        ["1/4", "1/8", "1/16", "1/32", "1/64", "1/128"]
    );
    assert_eq!(
        StripBarChoices::musical_periods(Q),
        [
            ("1 Bar of 4/4".to_string(), 4 * Q),
            ("1/2".into(), 2 * Q),
            ("1/4".into(), Q),
            ("1/8".into(), Q / 2),
            ("1/16".into(), Q / 4),
            ("1/32".into(), Q / 8),
        ]
    );
    assert_eq!(
        StripBarChoices::clock_periods(25)
            .into_iter()
            .map(|c| c.0)
            .collect::<Vec<_>>(),
        [
            "25 ms", "50 ms", "100 ms", "250 ms", "500 ms", "1000 ms", "2000 ms", "4000 ms"
        ]
    );
    assert_eq!(
        StripBarChoices::bar_periods(16)
            .into_iter()
            .map(|c| c.0)
            .collect::<Vec<_>>(),
        ["1 bar", "2 bars", "4 bars", "8 bars", "16 bars"]
    );
}

#[test]
fn a_detail_saved_without_its_rows_lane_decodes() {
    let mut d = DetailState::new(
        DetailEditorKind::StripChart,
        EditorTarget::Lane { seq: 3, lane: 30 },
    );
    d.set_strip_rows(Some(vec![StripLane::Velocity]));
    d.set_strip_rows_lane(Some((3, 30)));
    let mut json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["stripRowsLane"], json!({"seq": 3, "lane": 30}));
    let back: DetailState = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(back.strip_rows_lane(), Some((3, 30)));
    json.as_object_mut().unwrap().remove("stripRowsLane");
    let old: DetailState = serde_json::from_value(json).unwrap();
    assert!(old.strip_rows() == Some(vec![StripLane::Velocity]) && old.strip_rows_lane().is_none());
    let mut cleared = back;
    cleared.set_strip_rows(None);
    cleared.set_strip_rows_lane(None);
    assert!(!cleared.extra.contains_key("stripRows"));
    assert!(!cleared.extra.contains_key("stripRowsLane"));
}

#[test]
fn a_line_at_the_last_tick_does_not_overflow() {
    let l = LineRow {
        from: 0,
        to: i64::MAX,
        start: 0,
        end: 1,
        step: 1,
        shape: LineShapeRow::Straight,
    };
    let EditCommand::SelectAndModify { rule, .. } =
        line_command(StripLane::Controller(1), LineOpRow::Draw, l, false)
    else {
        panic!("expected selectAndModify");
    };
    assert_eq!(rule, Some(time_rule(0, i64::MAX)));
    let c = compress_command(StripLane::Velocity, pt(i64::MAX, 1), pt(0, 2), 1, false);
    let EditCommand::SelectAndModify { rule, .. } = c else {
        panic!("expected selectAndModify");
    };
    assert_eq!(rule, Some(time_rule(0, i64::MAX)));
}

#[test]
fn malformed_rows_read_as_one_velocity_row() {
    let detail = |rows: Value| {
        serde_json::from_value::<DetailState>(json!({
            "kind": "stripChart",
            "target": {"lane": {"seq": 3, "lane": 31}},
            "stripRows": rows,
        }))
        .unwrap()
    };
    assert_eq!(detail(json!([{"bogus": {}}])).strip_rows(), None);
    assert_eq!(
        detail(json!([{"velocity": {}}, {"bogus": {}}])).strip_rows(),
        None
    );
    assert_eq!(detail(json!("velocity")).strip_rows(), None);
    assert_eq!(detail(json!([])).strip_rows(), None);
    let six = Value::Array(vec![json!({"velocity": {}}); 6]);
    assert_eq!(detail(six).strip_rows(), None);
    let five = vec![StripLane::Velocity; MAX_ROWS + 1];
    assert_eq!(
        detail(serde_json::to_value(&five).unwrap()).strip_rows(),
        Some(five)
    );
    let mut d = detail(json!([{"velocity": {}}]));
    d.extra.insert("stripRowsLane".into(), json!({"seq": "x"}));
    assert_eq!(d.strip_rows_lane(), None);
    // A bad `controllers` entry is dropped; the good ones stay.
    let l = EditorLayout::decode(Some(
        &json!({"controllers": {
            "3:31": [{"controller": {"_0": 7}}],
            "3:32": [{"bogus": {}}],
            "3:33": "x",
        }})
        .to_string(),
    ));
    assert_eq!(l.controllers.len(), 1);
    assert_eq!(
        l.controllers.get("3:31"),
        Some(&vec![StripLane::Controller(7)])
    );
    assert!(
        EditorLayout::decode(Some(r#"{"controllers":[1]}"#))
            .controllers
            .is_empty()
    );
}

#[test]
fn the_settings_and_rows_survive_a_save() {
    assert_eq!(controllers_key(3, 31), "3:31");
    let existing = json!({
        "stripChart": {"rowHeight": 120, "future": {"x": 1}},
        "controllers": {"9:9": [{"tempo": {}}]},
        "other": true
    })
    .to_string();
    let mut l = EditorLayout::decode(Some(&existing));
    assert_eq!(l.strip_chart.row_height, 120.);
    l.strip_chart.shape = ShapeKind::Curved;
    let fader = StripLane::Automation(AutomationTargetRow::StripFader { strip: 1 });
    l.controllers
        .insert(controllers_key(3, 31), vec![StripLane::Velocity, fader]);
    l.controllers.remove("9:9");
    let mut d = DetailState::new(
        DetailEditorKind::StripChart,
        EditorTarget::Lane { seq: 3, lane: 31 },
    );
    d.set_strip_rows(Some(vec![StripLane::PitchBend, StripLane::Controller(74)]));
    d.set_strip_rows_lane(Some((3, 31)));
    l.detail = Some(d.clone());
    let text = l.merge_into(Some(&existing));
    let saved: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        saved["stripChart"]["future"],
        json!({"x": 1}),
        "an unknown key stays"
    );
    assert_eq!(saved["stripChart"]["shape"], json!("curved"));
    assert_eq!(saved["stripChart"]["rowHeight"], json!(120.0));
    assert_eq!(saved["other"], json!(true));
    assert!(
        saved["controllers"].get("9:9").is_none(),
        "controllers is replaced whole"
    );
    let back = EditorLayout::decode(Some(&text));
    assert_eq!(back.strip_chart, l.strip_chart);
    assert_eq!(back.controllers, l.controllers);
    let detail = back.detail.unwrap();
    assert_eq!(
        detail.strip_rows(),
        Some(vec![StripLane::PitchBend, StripLane::Controller(74)])
    );
    assert_eq!(detail.strip_rows_lane(), Some((3, 31)));
    // A fresh save writes the bar's thirteen keys and an empty map.
    let fresh: Value = serde_json::from_str(&EditorLayout::default().encoded()).unwrap();
    assert_eq!(
        fresh["stripChart"],
        json!({
            "mode": "draw", "op": "set", "shape": "straight", "curve": "easeOut",
            "wave": "sine", "period": "musical", "periodTicks": Q, "periodMs": 500,
            "periodBars": 1, "phase": 0, "step": Q / 8, "rowHeight": 96.0,
            "ticksPerPoint": 96.0
        })
    );
    assert_eq!(fresh["controllers"], json!({}));
}
