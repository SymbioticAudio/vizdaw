//! Strip Chart lines that change events (Plan 4d Rulings 15–19).

use core_model::automation::AutomationTarget;
use core_model::command::{Command, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{EventId, LaneId, SeqId, StripId};
use core_model::modify::{Modify, ValueTarget};
use core_model::rule::{Rule, SelectMode};
use core_model::selection::{Selection, ViewFilter};
use core_model::strip::{Line, LineChange, LineOp, LineShape, LineTarget, Series};
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;
const S: SeqId = SeqId(0);
const L0: LaneId = LaneId(0);

/// seq0: lane 0 with notes (ids 0–3) at 0, Q, 2Q, 3Q (velocities 10, 20,
/// 30, 40, half a quarter long) and CC7 = 100 (ids 4–5) at 0 and 2Q;
/// lane 1 empty.
fn doc() -> (Document, LaneId) {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let mut events: Vec<EventSpec> = (0..4)
        .map(|k| {
            EventSpec::new(
                Tick(k * Q),
                EventBody::note(60, 10 * (k as u16 + 1), Tick(Q / 2)),
            )
        })
        .collect();
    for t in [0, 2 * Q] {
        events.push(EventSpec::new(
            Tick(t),
            EventBody::Cc {
                index: 7,
                value: 100,
            },
        ));
    }
    let d = apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: S,
            lane: L0,
            events,
        }),
    )
    .unwrap()
    .doc;
    let a = apply(
        &d,
        &Command::Lane(LaneCmd::Add {
            seq: S,
            name: "Other".into(),
            index: None,
        }),
    )
    .unwrap();
    let l1 = a
        .doc
        .sequence(S)
        .unwrap()
        .lanes
        .iter()
        .map(|l| l.id)
        .find(|id| *id != L0)
        .unwrap();
    (a.doc, l1)
}

fn line(from: i64, to: i64, start: i64, end: i64) -> Line {
    Line {
        from: Tick(from),
        to: Tick(to),
        start,
        end,
        step: Tick(Q),
        shape: LineShape::Straight,
    }
}

/// Runs `change` over its line's range in `lanes` as the Strip Chart does
/// (rule: the range; Select, or Refine on `selected` events).
fn run(
    h: &mut History,
    lanes: &[LaneId],
    selected: &[u64],
    refine: bool,
    change: LineChange,
) -> core_model::Result<()> {
    h.set_selection(Selection {
        time_range: None,
        lanes: lanes.iter().map(|l| (S, *l)).collect(),
        events: selected.iter().map(|e| EventId(*e)).collect(),
    });
    let (from, to) = (change.line.from, change.line.to);
    h.execute(
        &Command::SelectAndModify {
            rule: Some(Rule::TimeRange {
                start: from,
                end: to + Tick(1),
            }),
            mode: if refine {
                SelectMode::Refine
            } else {
                SelectMode::Select
            },
            modify: Some(Modify::Line(change)),
            to_copy: false,
        },
        &ViewFilter::default(),
    )
    .map(|_| ())
}

fn velocities(h: &History) -> Vec<u16> {
    (0..4)
        .map(|i| match h.doc().event(EventId(i)).unwrap().0.body {
            EventBody::Note { velocity, .. } => velocity,
            ref b => panic!("{b:?}"),
        })
        .collect()
}

fn velocity_line(op: LineOp, l: Line) -> LineChange {
    LineChange {
        target: LineTarget::Value(ValueTarget::Velocity),
        op,
        line: l,
    }
}

/// `(tick, body)` of lane `lane`'s events of series `s`, in time order.
fn series(h: &History, lane: LaneId, s: Series) -> Vec<(i64, EventBody)> {
    h.doc()
        .lane(S, lane)
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter(|e| Series::of(&e.body) == Some(s))
        .map(|e| (e.tick.0, e.body.clone()))
        .collect()
}

#[test]
fn set_scale_add_and_limits_follow_the_line_at_each_note() {
    let (d, _) = doc();
    let check = |op, l: Line, want: [u16; 4]| {
        let mut h = History::new(d.clone());
        run(&mut h, &[L0], &[], false, velocity_line(op, l)).unwrap();
        assert_eq!(velocities(&h), want, "{op:?}");
    };
    check(LineOp::Set, line(0, 3 * Q, 0, 3000), [0, 1000, 2000, 3000]);
    check(LineOp::Draw, line(0, 3 * Q, 0, 3000), [0, 1000, 2000, 3000]);
    // Scale amounts are percent along the line.
    check(LineOp::Scale, line(0, 3 * Q, 100, 400), [10, 40, 90, 160]);
    check(LineOp::Add, line(0, 3 * Q, -10, 20), [0, 20, 40, 60]);
    check(LineOp::LimitMax, line(0, 3 * Q, 15, 15), [10, 15, 15, 15]);
    check(LineOp::LimitMin, line(0, 3 * Q, 25, 25), [25, 25, 30, 40]);
    // Only the line's range changes.
    check(LineOp::Set, line(Q, 2 * Q, 7, 7), [10, 7, 7, 40]);
    let mut h = History::new(d.clone());
    run(
        &mut h,
        &[L0],
        &[],
        false,
        velocity_line(LineOp::Set, line(0, 3 * Q, 0, 3000)),
    )
    .unwrap();
    assert_eq!(h.undo_label(), Some("Change Velocity"));
}

#[test]
fn a_modifier_limits_the_line_to_the_selected_events() {
    let (d, _) = doc();
    let mut h = History::new(d);
    run(
        &mut h,
        &[L0],
        &[1, 3],
        true,
        velocity_line(LineOp::Set, line(0, 3 * Q, 5, 5)),
    )
    .unwrap();
    assert_eq!(velocities(&h), [10, 5, 30, 5]);
}

#[test]
fn a_line_over_no_events_changes_nothing() {
    let (d, _) = doc();
    let mut h = History::new(d.clone());
    run(
        &mut h,
        &[L0],
        &[],
        false,
        velocity_line(LineOp::Set, line(10 * Q, 12 * Q, 1, 1)),
    )
    .unwrap();
    assert_eq!(h.doc(), &d);
    assert_eq!(h.undo_label(), None, "no undo step");
}

#[test]
fn a_series_line_changes_only_that_series() {
    let (d, _) = doc();
    let mut h = History::new(d);
    let cc7 = Series::Controller(7);
    run(
        &mut h,
        &[L0],
        &[],
        false,
        LineChange {
            target: LineTarget::Series(cc7),
            op: LineOp::Set,
            line: line(0, 2 * Q, 0, i64::from(u32::MAX)),
        },
    )
    .unwrap();
    assert_eq!(
        series(&h, L0, cc7),
        vec![
            (0, EventBody::Cc { index: 7, value: 0 }),
            (
                2 * Q,
                EventBody::Cc {
                    index: 7,
                    value: u32::MAX,
                },
            ),
        ]
    );
    assert_eq!(velocities(&h), [10, 20, 30, 40], "notes untouched");
    assert_eq!(h.undo_label(), Some("Change Values"));
}

#[test]
fn drawing_replaces_the_series_in_every_selected_lane() {
    let (d, l1) = doc();
    let cc7 = Series::Controller(7);
    let mut h = History::new(d.clone());
    let full = i64::from(u32::MAX);
    run(
        &mut h,
        &[L0, l1],
        &[],
        false,
        LineChange {
            target: LineTarget::Series(cc7),
            op: LineOp::Draw,
            line: line(0, 2 * Q, 0, full),
        },
    )
    .unwrap();
    let want: Vec<i64> = vec![0, Q, 2 * Q];
    for lane in [L0, l1] {
        let got = series(&h, lane, cc7);
        assert_eq!(
            got.iter().map(|g| g.0).collect::<Vec<_>>(),
            want,
            "{lane:?}"
        );
        assert_eq!(
            got[1].1,
            EventBody::Cc {
                index: 7,
                value: u32::MAX / 2 + 1
            }
        );
    }
    assert_eq!(velocities(&h), [10, 20, 30, 40]);
    assert_eq!(
        h.selection().events.len(),
        6,
        "the drawn events are selected"
    );
    assert_eq!(h.undo_label(), Some("Draw Controllers"));
    // Continuous automation ramps: a straight line is two points.
    let fader = Series::Automation(AutomationTarget::StripFader(StripId(1)));
    let mut h = History::new(d.clone());
    run(
        &mut h,
        &[L0],
        &[],
        false,
        LineChange {
            target: LineTarget::Series(fader),
            op: LineOp::Draw,
            line: line(0, 8 * Q, 0, full),
        },
    )
    .unwrap();
    assert_eq!(series(&h, L0, fader).len(), 2);
    assert_eq!(h.undo_label(), Some("Draw Automation"));
    // Programs hold: one event per new program number.
    let mut h = History::new(d.clone());
    run(
        &mut h,
        &[L0],
        &[],
        false,
        LineChange {
            target: LineTarget::Series(Series::Program),
            op: LineOp::Draw,
            line: Line {
                step: Tick(Q / 4),
                ..line(0, 4 * Q, 0, 4 << 25)
            },
        },
    )
    .unwrap();
    let programs: Vec<u8> = series(&h, L0, Series::Program)
        .into_iter()
        .map(|(_, b)| match b {
            EventBody::Program { program, .. } => program,
            b => panic!("{b:?}"),
        })
        .collect();
    assert_eq!(programs, vec![0, 1, 2, 3, 4]);
    // Drawing needs a lane.
    let mut h = History::new(d);
    assert!(
        run(
            &mut h,
            &[],
            &[],
            false,
            LineChange {
                target: LineTarget::Series(cc7),
                op: LineOp::Draw,
                line: line(0, Q, 0, 1),
            },
        )
        .is_err()
    );
}

#[test]
fn compress_thin_thicken_and_legato() {
    let (d, _) = doc();
    let mut h = History::new(d.clone());
    // Compress maps the lowest (10) to 100 and the highest (40) to 400.
    run(
        &mut h,
        &[L0],
        &[],
        false,
        velocity_line(LineOp::Compress, line(0, 3 * Q, 100, 400)),
    )
    .unwrap();
    assert_eq!(velocities(&h), [100, 200, 300, 400]);
    assert_eq!(h.undo_label(), Some("Compress"));
    // A flat line sets every velocity to 100 but Compress still maps the
    // lowest to `start` and the highest to `end`.
    let flat = |op| {
        velocity_line(
            op,
            Line {
                shape: LineShape::Flat,
                ..line(0, 3 * Q, 100, 400)
            },
        )
    };
    let mut hc = History::new(d.clone());
    run(&mut hc, &[L0], &[], false, flat(LineOp::Compress)).unwrap();
    assert_eq!(velocities(&hc), [100, 200, 300, 400]);
    let mut hs = History::new(d.clone());
    run(&mut hs, &[L0], &[], false, flat(LineOp::Set)).unwrap();
    assert_eq!(velocities(&hs), [100, 100, 100, 100]);
    // Thicken CC7 between 0 and 2Q every half quarter, then thin it again.
    let cc7 = Series::Controller(7);
    let thick = LineChange {
        target: LineTarget::Series(cc7),
        op: LineOp::Thicken,
        line: Line {
            step: Tick(Q / 2),
            ..line(0, 2 * Q, 0, 0)
        },
    };
    let mut h = History::new(d.clone());
    run(&mut h, &[L0], &[], false, thick).unwrap();
    assert_eq!(series(&h, L0, cc7).len(), 5, "0, Q/2, Q, 3Q/2, 2Q");
    let thin = LineChange {
        target: LineTarget::Series(cc7),
        op: LineOp::Thin,
        line: line(0, 2 * Q, 0, 0),
    };
    run(&mut h, &[L0], &[], false, thin).unwrap();
    assert_eq!(
        series(&h, L0, cc7).iter().map(|s| s.0).collect::<Vec<_>>(),
        vec![0, Q, 2 * Q]
    );
    // Legato lengthens notes to the next one; it needs the length target.
    let mut h = History::new(d.clone());
    run(
        &mut h,
        &[L0],
        &[],
        false,
        LineChange {
            target: LineTarget::Value(ValueTarget::Duration),
            op: LineOp::Legato,
            line: line(0, 3 * Q, 0, 0),
        },
    )
    .unwrap();
    let first = match h.doc().event(EventId(0)).unwrap().0.body {
        EventBody::Note { duration, .. } => duration,
        ref b => panic!("{b:?}"),
    };
    assert_eq!(first, Tick(Q));
    let mut h = History::new(d);
    assert!(
        run(
            &mut h,
            &[L0],
            &[],
            false,
            velocity_line(LineOp::Legato, line(0, Q, 0, 0))
        )
        .is_err()
    );
}

#[test]
fn bad_lines_and_copies_are_refused_and_modify_again_repeats() {
    let (d, _) = doc();
    let mut h = History::new(d.clone());
    let bad = Line {
        from: Tick(5),
        to: Tick(1),
        ..line(0, 0, 0, 0)
    };
    assert!(run(&mut h, &[L0], &[], false, velocity_line(LineOp::Set, bad)).is_err());
    assert!(
        run(
            &mut h,
            &[L0],
            &[],
            false,
            LineChange {
                target: LineTarget::Series(Series::Controller(200)),
                op: LineOp::Draw,
                line: line(0, Q, 0, 0),
            },
        )
        .is_err()
    );
    h.set_selection(Selection {
        time_range: None,
        lanes: [(S, L0)].into_iter().collect(),
        events: [EventId(0)].into_iter().collect(),
    });
    assert!(
        h.execute(
            &Command::SelectAndModify {
                rule: None,
                mode: SelectMode::Select,
                modify: Some(Modify::Line(velocity_line(
                    LineOp::Add,
                    line(0, 3 * Q, 1, 1)
                ))),
                to_copy: true,
            },
            &ViewFilter::default(),
        )
        .is_err(),
        "a line cannot change copies"
    );
    assert_eq!(h.doc(), &d);
    // Modify Again repeats a line on the selection.
    run(
        &mut h,
        &[L0],
        &[],
        false,
        velocity_line(LineOp::Add, line(0, 3 * Q, 1, 1)),
    )
    .unwrap();
    h.modify_again(&ViewFilter::default()).unwrap();
    assert_eq!(velocities(&h), [12, 22, 32, 42]);
}

#[test]
fn thin_and_thicken_work_on_automation_series() {
    let (d, _) = doc();
    let fader = Series::Automation(AutomationTarget::StripFader(StripId(1)));
    // Draw a ramp (two points), then thicken it, then thin it again.
    let mut h = History::new(d);
    let full = i64::from(u32::MAX);
    run(
        &mut h,
        &[L0],
        &[],
        false,
        LineChange {
            target: LineTarget::Series(fader),
            op: LineOp::Draw,
            line: line(0, 4 * Q, 0, full),
        },
    )
    .unwrap();
    assert_eq!(series(&h, L0, fader).len(), 2);
    let thick = LineChange {
        target: LineTarget::Series(fader),
        op: LineOp::Thicken,
        line: Line {
            step: Tick(Q),
            ..line(0, 4 * Q, 0, 0)
        },
    };
    run(&mut h, &[L0], &[], false, thick).unwrap();
    let got = series(&h, L0, fader);
    assert_eq!(
        got.iter().map(|g| g.0).collect::<Vec<_>>(),
        vec![0, Q, 2 * Q, 3 * Q, 4 * Q]
    );
    // Thickened values are interpolated, written through from_u32.
    let mid = match got[2].1 {
        EventBody::Automation { value, .. } => value,
        ref b => panic!("{b:?}"),
    };
    assert!((mid - 0.5).abs() < 1e-6, "{mid}");
    let thin = LineChange {
        target: LineTarget::Series(fader),
        op: LineOp::Thin,
        line: Line {
            step: Tick(2 * Q),
            ..line(0, 4 * Q, 0, 0)
        },
    };
    run(&mut h, &[L0], &[], false, thin).unwrap();
    assert_eq!(
        series(&h, L0, fader)
            .iter()
            .map(|s| s.0)
            .collect::<Vec<_>>(),
        vec![0, 2 * Q, 4 * Q]
    );
}

/// `d` with `specs` inserted into lane 0.
fn with_events(d: &Document, specs: Vec<EventSpec>) -> Document {
    apply(
        d,
        &Command::Event(EventCmd::Insert {
            seq: S,
            lane: L0,
            events: specs,
        }),
    )
    .unwrap()
    .doc
}

fn programs(h: &History) -> Vec<(i64, u8)> {
    series(h, L0, Series::Program)
        .into_iter()
        .map(|(t, b)| match b {
            EventBody::Program { program, .. } => (t, program),
            b => panic!("{b:?}"),
        })
        .collect()
}

fn fader_values(h: &History, fader: Series) -> Vec<f64> {
    series(h, L0, fader)
        .into_iter()
        .map(|(_, b)| match b {
            EventBody::Automation { value, .. } => value,
            b => panic!("{b:?}"),
        })
        .collect()
}

fn prog(t: i64, program: u8) -> EventSpec {
    EventSpec::new(
        Tick(t),
        EventBody::Program {
            program,
            bank: None,
        },
    )
}

#[test]
fn set_scale_and_add_use_the_program_value_space() {
    let (d, _) = doc();
    let d = with_events(&d, vec![prog(0, 10), prog(Q, 20), prog(2 * Q, 30)]);
    let check = |op, l: Line, want: [u8; 3]| {
        let mut h = History::new(d.clone());
        let c = LineChange {
            target: LineTarget::Series(Series::Program),
            op,
            line: l,
        };
        run(&mut h, &[L0], &[], false, c).unwrap();
        let got: Vec<u8> = programs(&h).into_iter().map(|p| p.1).collect();
        assert_eq!(got, want, "{op:?}");
        assert_eq!(h.undo_label(), Some("Change Values"));
    };
    // Programs are the 7-bit number times 2^25 on the line's scale.
    check(LineOp::Set, line(0, 2 * Q, 0, 100 << 25), [0, 50, 100]);
    check(LineOp::Scale, line(0, 2 * Q, 100, 300), [10, 40, 90]);
    check(LineOp::Add, line(0, 2 * Q, 5 << 25, 15 << 25), [15, 30, 45]);
}

#[test]
fn set_scale_and_add_use_the_automation_value_space() {
    let (d, _) = doc();
    let fader = Series::Automation(AutomationTarget::StripFader(StripId(1)));
    let auto = |t, value| {
        EventSpec::new(
            Tick(t),
            EventBody::Automation {
                target: AutomationTarget::StripFader(StripId(1)),
                value,
            },
        )
    };
    let d = with_events(&d, vec![auto(0, 0.25), auto(Q, 0.5), auto(2 * Q, 0.75)]);
    let full = i64::from(u32::MAX);
    let check = |op, l: Line, want: [f64; 3]| {
        let mut h = History::new(d.clone());
        let c = LineChange {
            target: LineTarget::Series(fader),
            op,
            line: l,
        };
        run(&mut h, &[L0], &[], false, c).unwrap();
        let got = fader_values(&h, fader);
        assert_eq!(got.len(), 3);
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-6, "{op:?}: {got:?} vs {want:?}");
        }
    };
    // 0..1 on the 32-bit scale; results clamp at 1.
    check(LineOp::Set, line(0, 2 * Q, 0, full), [0.0, 0.5, 1.0]);
    check(LineOp::Scale, line(0, 2 * Q, 100, 200), [0.25, 0.75, 1.0]);
    check(LineOp::Add, line(0, 2 * Q, 0, full / 4), [0.25, 0.625, 1.0]);
}

#[test]
fn thin_and_thicken_work_on_program_changes() {
    let (d, _) = doc();
    let d = with_events(&d, vec![prog(0, 0), prog(4 * Q, 100)]);
    let mut h = History::new(d);
    let change = |op, step| LineChange {
        target: LineTarget::Series(Series::Program),
        op,
        line: Line {
            step: Tick(step),
            ..line(0, 4 * Q, 0, 0)
        },
    };
    run(&mut h, &[L0], &[], false, change(LineOp::Thicken, Q)).unwrap();
    assert_eq!(
        programs(&h),
        vec![(0, 0), (Q, 25), (2 * Q, 50), (3 * Q, 75), (4 * Q, 100)]
    );
    run(&mut h, &[L0], &[], false, change(LineOp::Thin, 2 * Q)).unwrap();
    assert_eq!(programs(&h), vec![(0, 0), (2 * Q, 50), (4 * Q, 100)]);
}

#[test]
fn thickened_program_changes_carry_the_bank() {
    // Thickening clones the event before each gap, so the new program
    // changes keep its bank select (parked T4 test gap).
    let (d, _) = doc();
    let banked = |t: i64, program: u8, bank| {
        EventSpec::new(
            Tick(t),
            EventBody::Program {
                program,
                bank: Some(bank),
            },
        )
    };
    let d = with_events(&d, vec![banked(0, 0, (1, 2)), banked(4 * Q, 100, (3, 4))]);
    let mut h = History::new(d);
    let thick = LineChange {
        target: LineTarget::Series(Series::Program),
        op: LineOp::Thicken,
        line: Line {
            step: Tick(Q),
            ..line(0, 4 * Q, 0, 0)
        },
    };
    run(&mut h, &[L0], &[], false, thick).unwrap();
    type Banked = (i64, u8, Option<(u8, u8)>);
    let got: Vec<Banked> = series(&h, L0, Series::Program)
        .into_iter()
        .map(|(t, b)| match b {
            EventBody::Program { program, bank } => (t, program, bank),
            b => panic!("{b:?}"),
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (0, 0, Some((1, 2))),
            (Q, 25, Some((1, 2))),
            (2 * Q, 50, Some((1, 2))),
            (3 * Q, 75, Some((1, 2))),
            (4 * Q, 100, Some((3, 4))),
        ]
    );
}
