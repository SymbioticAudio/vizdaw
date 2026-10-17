//! Strip Chart lines through the FFI (Plan 4d Rulings 15, 25).

use core_engine::fixtures::{Builder, Q};
use core_model::event::{EventBody, EventSpec};
use core_model::time::Tick;
use vizdaw_ffi::strip::{
    LineOpRow, LinePointRow, LineRow, LineShapeRow, LineTargetRow, PeriodRow, SeriesRow,
};
use vizdaw_ffi::{
    AutomationTargetRow, ConditionRow, ConditionTest, CurveRow, EditCommand, LaneRef, ModifyRow,
    RuleRow, SelectModeRow, SelectionRow, Session, ValueTargetRow, VizError, WaveRow,
};

fn line(from: i64, to: i64, start: i64, end: i64, shape: LineShapeRow) -> LineRow {
    LineRow {
        from,
        to,
        start,
        end,
        step: Q,
        shape,
    }
}

fn strip_edit(target: LineTargetRow, op: LineOpRow, l: LineRow) -> EditCommand {
    EditCommand::SelectAndModify {
        rule: Some(RuleRow {
            conditions: vec![ConditionRow {
                test: ConditionTest::TimeRange {
                    start: l.from,
                    end: l.to + 1,
                },
                negate: false,
            }],
            match_all: true,
        }),
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::Line {
            target,
            op,
            line: l,
        }),
        to_copy: false,
    }
}

#[test]
fn a_drawn_controller_line_is_one_undo_step() {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("S", "L", None).unwrap();
    b.events(
        seq,
        lane,
        vec![EventSpec::new(
            Tick(Q),
            EventBody::Cc { index: 7, value: 5 },
        )],
    )
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef {
            seq: seq.0,
            lane: lane.0,
        }],
        events: vec![],
    })
    .unwrap();
    let draw = strip_edit(
        LineTargetRow::Series {
            series: SeriesRow::Controller { index: 7 },
        },
        LineOpRow::Draw,
        line(0, 2 * Q, 0, i64::from(u32::MAX), LineShapeRow::Straight),
    );
    let snap = s.edit(draw, None).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Draw Controllers"));
    let rows = s.lane_events(seq.0, lane.0).unwrap().events;
    assert_eq!(
        rows.iter().map(|r| (r.tick, r.value2)).collect::<Vec<_>>(),
        vec![(0, 7), (Q, 7), (2 * Q, 7)],
        "the old CC at Q was replaced"
    );
    assert_eq!(rows[2].value, i64::from(u32::MAX));
    s.undo().unwrap();
    assert_eq!(s.lane_events(seq.0, lane.0).unwrap().events.len(), 1);
}

#[test]
fn line_rows_round_trip_and_bad_lines_are_refused() {
    let shapes = vec![
        LineShapeRow::Straight,
        LineShapeRow::Curved {
            curve: CurveRow::SCurve,
        },
        LineShapeRow::Flat,
        LineShapeRow::Free {
            points: vec![
                LinePointRow { tick: 0, value: 1 },
                LinePointRow { tick: 9, value: 2 },
            ],
        },
        LineShapeRow::Random { seed: 42 },
        LineShapeRow::Oscillator {
            wave: WaveRow::Triangle,
            period: PeriodRow::Bars { bars: 2 },
            phase: 500,
        },
        LineShapeRow::Oscillator {
            wave: WaveRow::Saw,
            period: PeriodRow::Millis { ms: 250 },
            phase: 0,
        },
        LineShapeRow::Oscillator {
            wave: WaveRow::Sine,
            period: PeriodRow::Ticks { ticks: Q },
            phase: 0,
        },
    ];
    let targets = [
        LineTargetRow::Value {
            target: ValueTargetRow::Duration,
        },
        LineTargetRow::Series {
            series: SeriesRow::PolyPressure { pitch: 64 },
        },
        LineTargetRow::Series {
            series: SeriesRow::Automation {
                target: AutomationTargetRow::PluginParam { slot: 3, param: 9 },
            },
        },
    ];
    for shape in shapes {
        for target in targets {
            let row = ModifyRow::Line {
                target,
                op: LineOpRow::LimitMin,
                line: line(0, Q, 3, 4, shape.clone()),
            };
            let core = vizdaw_ffi::modify::modify(&row).unwrap();
            assert_eq!(vizdaw_ffi::modify::modify_row(&core), row);
        }
    }
    let s = Session::new_demo().unwrap();
    let backwards = strip_edit(
        LineTargetRow::Value {
            target: ValueTargetRow::Velocity,
        },
        LineOpRow::Set,
        line(Q, 0, 0, 0, LineShapeRow::Straight),
    );
    // The rule's empty range is refused first; a line whose `to` is before
    // `from` would be refused by the core.
    assert!(matches!(
        s.edit(backwards, None),
        Err(VizError::Model { .. })
    ));
    let legato = EditCommand::SelectAndModify {
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::Line {
            target: LineTargetRow::Value {
                target: ValueTargetRow::Velocity,
            },
            op: LineOpRow::Legato,
            line: line(0, Q, 0, 0, LineShapeRow::Flat),
        }),
        to_copy: false,
    };
    assert!(matches!(s.edit(legato, None), Err(VizError::Model { .. })));
}

#[test]
fn a_tempo_line_draws_ramps_in_the_selected_lanes_sequence() {
    let s = Session::new_demo().unwrap();
    let song = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id;
    let lane = s.lanes(song).unwrap()[0].id;
    s.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq: song, lane }],
        events: vec![],
    })
    .unwrap();
    let draw = EditCommand::SelectAndModify {
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::Line {
            target: LineTargetRow::Tempo,
            op: LineOpRow::Draw,
            line: line(0, 16 * Q, 120_000, 60_000, LineShapeRow::Straight),
        }),
        to_copy: false,
    };
    let snap = s.edit(draw.clone(), None).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Draw Tempo"));
    let tempo = s.sequence_info(song).unwrap().tempo;
    let inside: Vec<(i64, f64, bool)> = tempo
        .iter()
        .filter(|p| p.tick <= 16 * Q)
        .map(|p| (p.tick, p.bpm, p.ramp))
        .collect();
    assert_eq!(inside, vec![(0, 120.0, true), (16 * Q, 60.0, false)]);
    let EditCommand::SelectAndModify {
        modify: Some(row), ..
    } = draw
    else {
        unreachable!()
    };
    let core = vizdaw_ffi::modify::modify(&row).unwrap();
    assert_eq!(vizdaw_ffi::modify::modify_row(&core), row);
}
