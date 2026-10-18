//! Tempo lines (Plan 4d Ruling 20): drawn tempo is ramps, so it "sounds
//! linear".

use core_model::command::{Command, SequenceCmd, apply};
use core_model::document::Document;
use core_model::history::History;
use core_model::ids::{LaneId, SeqId};
use core_model::modify::{Curve, Modify};
use core_model::rule::SelectMode;
use core_model::selection::{Selection, ViewFilter};
use core_model::strip::{Line, LineChange, LineOp, LineShape, LineTarget};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;
const S: SeqId = SeqId(0);

fn seq_doc(tempo: TempoMap) -> Document {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    apply(
        &d,
        &Command::Sequence(SequenceCmd::SetTempo { seq: S, tempo }),
    )
    .unwrap()
    .doc
}

fn tempo_line(from: i64, to: i64, start: i64, end: i64, shape: LineShape) -> Line {
    Line {
        from: Tick(from),
        to: Tick(to),
        start,
        end,
        step: Tick(Q),
        shape,
    }
}

fn run(d: &Document, lanes: &[LaneId], op: LineOp, line: Line) -> core_model::Result<History> {
    let mut h = History::new(d.clone());
    h.set_selection(Selection {
        time_range: None,
        lanes: lanes.iter().map(|l| (S, *l)).collect(),
        events: Default::default(),
    });
    h.execute(
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(Modify::Line(LineChange {
                target: LineTarget::Tempo,
                op,
                line,
            })),
            to_copy: false,
        },
        &ViewFilter::default(),
    )?;
    Ok(h)
}

fn points(h: &History) -> Vec<(i64, u32, bool)> {
    h.doc()
        .sequence(S)
        .unwrap()
        .tempo
        .points()
        .iter()
        .map(|p| (p.tick.0, p.bpm.0, p.ramp))
        .collect()
}

/// Seconds of a ramp linear in BPM per tick from `b0` to `b1` over `len`
/// ticks, at tick `x` of it.
fn ramp_seconds(x: f64, len: f64, b0: f64, b1: f64) -> f64 {
    let k = (b1 - b0) / len;
    60.0 / (PPQ as f64 * k) * (1.0 + k * x / b0).ln()
}

#[test]
fn a_straight_tempo_line_is_one_linear_ramp() {
    let d = seq_doc(TempoMap::default());
    let h = run(
        &d,
        &[LaneId(0)],
        LineOp::Draw,
        tempo_line(0, 16 * Q, 120_000, 60_000, LineShape::Straight),
    )
    .unwrap();
    assert_eq!(
        points(&h),
        vec![(0, 120_000, true), (16 * Q, 60_000, false)]
    );
    assert_eq!(h.undo_label(), Some("Draw Tempo"));
    let map = &h.doc().sequence(S).unwrap().tempo;
    let want = ramp_seconds((8 * Q) as f64, (16 * Q) as f64, 120.0, 60.0);
    assert!(
        (map.tick_to_seconds(Tick(8 * Q)) - want).abs() < 1e-9,
        "sounds linear"
    );
    assert!((map.bpm_at(Tick(8 * Q)) - 90.0).abs() < 1e-9);
    assert_eq!(map.bpm_at(Tick(20 * Q)), 60.0, "holds after the line");
}

#[test]
fn drawn_curves_are_ramps_between_points_and_keep_what_is_outside() {
    let d = seq_doc(TempoMap::default());
    let h = run(
        &d,
        &[LaneId(0)],
        LineOp::Draw,
        tempo_line(
            2 * Q,
            6 * Q,
            100_000,
            140_000,
            LineShape::Curved {
                curve: Curve::EaseIn,
            },
        ),
    )
    .unwrap();
    let p = points(&h);
    assert_eq!(
        p.first(),
        Some(&(0, 120_000, false)),
        "the point at 0 stays"
    );
    assert_eq!(p.last(), Some(&(6 * Q, 140_000, false)));
    assert!(p.len() > 3, "a curve keeps its corners: {p:?}");
    assert!(
        p[1..p.len() - 1].iter().all(|x| x.2),
        "ramps between drawn points"
    );
    // A value beyond 5–999 BPM is clamped.
    let h = run(
        &d,
        &[LaneId(0)],
        LineOp::Draw,
        tempo_line(0, 0, 5_000_000, 0, LineShape::Flat),
    )
    .unwrap();
    assert_eq!(points(&h), vec![(0, Bpm::MAX.0, false)]);
}

#[test]
fn tempo_changes_compress_thin_and_thicken() {
    let ramp = TempoMap::new(vec![
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(120),
            ramp: true,
        },
        TempoPoint {
            tick: Tick(16 * Q),
            bpm: Bpm::whole(60),
            ramp: false,
        },
    ])
    .unwrap();
    let d = seq_doc(ramp.clone());
    let scaled = run(
        &d,
        &[LaneId(0)],
        LineOp::Scale,
        tempo_line(0, 16 * Q, 50, 50, LineShape::Flat),
    )
    .unwrap();
    assert_eq!(
        points(&scaled),
        vec![(0, 60_000, true), (16 * Q, 30_000, false)]
    );
    assert_eq!(scaled.undo_label(), Some("Change Tempo"));
    let compressed = run(
        &d,
        &[LaneId(0)],
        LineOp::Compress,
        tempo_line(0, 16 * Q, 100_000, 110_000, LineShape::Flat),
    )
    .unwrap();
    assert_eq!(
        points(&compressed),
        vec![(0, 110_000, true), (16 * Q, 100_000, false)]
    );
    // Thickening a ramp adds points on it and keeps its timing.
    let thick = run(
        &d,
        &[LaneId(0)],
        LineOp::Thicken,
        Line {
            step: Tick(4 * Q),
            ..tempo_line(0, 16 * Q, 0, 0, LineShape::Flat)
        },
    )
    .unwrap();
    assert_eq!(
        points(&thick),
        vec![
            (0, 120_000, true),
            (4 * Q, 105_000, true),
            (8 * Q, 90_000, true),
            (12 * Q, 75_000, true),
            (16 * Q, 60_000, false)
        ]
    );
    let t = |h: &History| {
        h.doc()
            .sequence(S)
            .unwrap()
            .tempo
            .tick_to_seconds(Tick(16 * Q))
    };
    assert!((t(&thick) - ramp.tick_to_seconds(Tick(16 * Q))).abs() < 1e-9);
    let thinned = run(
        thick.doc(),
        &[LaneId(0)],
        LineOp::Thin,
        Line {
            step: Tick(6 * Q),
            ..tempo_line(0, 16 * Q, 0, 0, LineShape::Flat)
        },
    )
    .unwrap();
    assert_eq!(
        points(&thinned).iter().map(|p| p.0).collect::<Vec<_>>(),
        vec![0, 8 * Q, 16 * Q]
    );
}

#[test]
fn tempo_lines_need_a_lane_and_a_start_at_or_after_zero() {
    let d = seq_doc(TempoMap::default());
    assert!(
        run(
            &d,
            &[],
            LineOp::Draw,
            tempo_line(0, Q, 1, 1, LineShape::Flat)
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            &[LaneId(0)],
            LineOp::Draw,
            tempo_line(-Q, Q, 1, 1, LineShape::Flat)
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            &[LaneId(0)],
            LineOp::Legato,
            tempo_line(0, Q, 1, 1, LineShape::Flat)
        )
        .is_err()
    );
}

#[test]
fn drawing_after_a_ramp_leaves_the_tempo_before_the_line_unchanged() {
    // Ruling P7: a ramp 120 -> 180 over bars 1-5 (4/4: a bar is 4 quarters).
    let bar = 4 * Q;
    let ramp = TempoMap::new(vec![
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(120),
            ramp: true,
        },
        TempoPoint {
            tick: Tick(4 * bar),
            bpm: Bpm::whole(180),
            ramp: false,
        },
    ])
    .unwrap();
    let d = seq_doc(ramp.clone());
    let h = run(
        &d,
        &[LaneId(0)],
        LineOp::Draw,
        tempo_line(2 * bar, 3 * bar, 100_000, 100_000, LineShape::Flat),
    )
    .unwrap();
    let after = &h.doc().sequence(S).unwrap().tempo;
    let mut t = 0;
    while t < 2 * bar {
        assert!(
            (after.bpm_at(Tick(t)) - ramp.bpm_at(Tick(t))).abs() < 1e-9,
            "bpm changed at tick {t}"
        );
        assert!(
            (after.tick_to_seconds(Tick(t)) - ramp.tick_to_seconds(Tick(t))).abs() < 1e-9,
            "timing changed at tick {t}"
        );
        t += 1;
    }
    assert_eq!(
        after.bpm_at(Tick(2 * bar + 1)),
        100.0,
        "the line starts one tick on"
    );
}

fn bar_ramp() -> TempoMap {
    TempoMap::new(vec![
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(120),
            ramp: true,
        },
        TempoPoint {
            tick: Tick(16 * Q),
            bpm: Bpm::whole(180),
            ramp: false,
        },
    ])
    .unwrap()
}

#[test]
fn drawing_inside_a_ramp_leaves_the_tempo_after_the_line_unchanged() {
    // Ruling P7b: ramp 120 -> 180 over bars 1-5, draw bars 3-4.
    let bar = 4 * Q;
    let ramp = bar_ramp();
    let d = seq_doc(ramp.clone());
    let h = run(
        &d,
        &[LaneId(0)],
        LineOp::Draw,
        tempo_line(2 * bar, 3 * bar, 100_000, 100_000, LineShape::Flat),
    )
    .unwrap();
    let after = &h.doc().sequence(S).unwrap().tempo;
    for t in (3 * bar + 1)..=(5 * bar) {
        assert!(
            // The restore point is stored in milli-BPM: within 0.0005 BPM.
            (after.bpm_at(Tick(t)) - ramp.bpm_at(Tick(t))).abs() < 1e-3,
            "bpm changed at tick {t}"
        );
    }
    assert_eq!(
        after.bpm_at(Tick(3 * bar)),
        100.0,
        "the line holds until to"
    );
}

#[test]
fn a_one_tick_line_with_step_one_inside_a_ramp_keeps_its_last_sample() {
    let d = seq_doc(bar_ramp());
    let h = run(
        &d,
        &[LaneId(0)],
        LineOp::Draw,
        Line {
            step: Tick(1),
            ..tempo_line(8 * Q, 8 * Q + 1, 100_000, 140_000, LineShape::Straight)
        },
    )
    .unwrap();
    let p = points(&h);
    assert!(p.contains(&(8 * Q + 1, 140_000, false)), "{p:?}");
    assert!(
        p.iter().all(|x| x.0 != 8 * Q || !x.2),
        "the hold point does not ramp: {p:?}"
    );
    let ramp = bar_ramp();
    let after = &h.doc().sequence(S).unwrap().tempo;
    for t in [0, 4 * Q, 8 * Q - 1, 8 * Q] {
        assert!(
            (after.bpm_at(Tick(t)) - ramp.bpm_at(Tick(t))).abs() < 1e-3,
            "tick {t}"
        );
    }
    for t in [8 * Q + 2, 12 * Q, 16 * Q] {
        assert!(
            (after.bpm_at(Tick(t)) - ramp.bpm_at(Tick(t))).abs() < 1e-3,
            "tick {t}"
        );
    }
}

#[test]
fn a_draw_that_changes_nothing_records_no_undo_step() {
    let ramp = TempoMap::new(vec![
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(120),
            ramp: true,
        },
        TempoPoint {
            tick: Tick(4 * Q),
            bpm: Bpm::whole(60),
            ramp: false,
        },
    ])
    .unwrap();
    let d = seq_doc(ramp);
    let h = run(
        &d,
        &[LaneId(0)],
        LineOp::Draw,
        tempo_line(0, 4 * Q, 120_000, 60_000, LineShape::Straight),
    )
    .unwrap();
    assert_eq!(h.undo_depth(), 0);
    let d = seq_doc(TempoMap::default());
    let h = run(
        &d,
        &[LaneId(0)],
        LineOp::Draw,
        tempo_line(0, 0, 120_000, 0, LineShape::Flat),
    )
    .unwrap();
    assert_eq!(h.undo_depth(), 0);
}

#[test]
fn drawing_inside_a_held_tempo_leaves_the_tempo_after_the_line_unchanged() {
    // Ruling P7c: held 120 from bar 1 with a point at bar 5, draw bars 3-4.
    let bar = 4 * Q;
    let held = TempoMap::new(vec![
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(120),
            ramp: false,
        },
        TempoPoint {
            tick: Tick(4 * bar),
            bpm: Bpm::whole(90),
            ramp: false,
        },
    ])
    .unwrap();
    let d = seq_doc(held);
    let h = run(
        &d,
        &[LaneId(0)],
        LineOp::Draw,
        tempo_line(2 * bar, 3 * bar, 100_000, 100_000, LineShape::Flat),
    )
    .unwrap();
    let after = &h.doc().sequence(S).unwrap().tempo;
    assert_eq!(after.bpm_at(Tick(3 * bar)), 100.0);
    for t in [3 * bar + 1, 3 * bar + Q, 4 * bar - 1] {
        assert_eq!(after.bpm_at(Tick(t)), 120.0, "tick {t}");
    }
    assert_eq!(after.bpm_at(Tick(4 * bar)), 90.0);
}

#[test]
fn a_zero_length_line_inside_a_ramp_keeps_the_tempo_around_it() {
    let ramp = bar_ramp();
    let d = seq_doc(ramp.clone());
    let h = run(
        &d,
        &[LaneId(0)],
        LineOp::Draw,
        tempo_line(8 * Q, 8 * Q, 200_000, 0, LineShape::Flat),
    )
    .unwrap();
    let after = &h.doc().sequence(S).unwrap().tempo;
    for t in [
        0,
        4 * Q,
        8 * Q - 1,
        8 * Q,
        8 * Q + 2,
        10 * Q,
        12 * Q,
        16 * Q,
    ] {
        let want = if t == 8 * Q + 1 {
            200.0
        } else {
            ramp.bpm_at(Tick(t))
        };
        assert!((after.bpm_at(Tick(t)) - want).abs() < 1e-3, "tick {t}");
    }
    assert_eq!(
        after.bpm_at(Tick(8 * Q + 1)),
        200.0,
        "the drawn point is one tick on"
    );
}

#[test]
fn a_tempo_line_keeps_the_selection_it_was_drawn_with() {
    // The Strip Chart runs a tempo line as a select-and-modify over the
    // line's time range: the selection afterwards is what it was, not every
    // event under the line (final review Minor 2).
    use core_model::command::EventCmd;
    use core_model::event::{EventBody, EventSpec};
    use core_model::ids::EventId;
    use core_model::rule::Rule;
    let d = seq_doc(TempoMap::default());
    let notes: Vec<EventSpec> = (0..4)
        .map(|k| EventSpec::new(Tick(k * Q), EventBody::note(60, 0xFFFF, Tick(Q))))
        .collect();
    let d = apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: S,
            lane: LaneId(0),
            events: notes,
        }),
    )
    .unwrap()
    .doc;
    let ids: Vec<EventId> = d.sequence(S).unwrap().lanes[0]
        .takes
        .iter()
        .flat_map(|t| t.events.iter().map(|(k, _)| k.1))
        .collect();
    assert_eq!(ids.len(), 4);
    let mut h = History::new(d);
    h.set_selection(Selection {
        time_range: None,
        lanes: [(S, LaneId(0))].into_iter().collect(),
        events: [ids[1]].into_iter().collect(),
    });
    h.execute(
        &Command::SelectAndModify {
            rule: Some(Rule::TimeRange {
                start: Tick(0),
                end: Tick(8 * Q),
            }),
            mode: SelectMode::Select,
            modify: Some(Modify::Line(LineChange {
                target: LineTarget::Tempo,
                op: LineOp::Draw,
                line: tempo_line(0, 8 * Q, 100_000, 140_000, LineShape::Straight),
            })),
            to_copy: false,
        },
        &ViewFilter::default(),
    )
    .unwrap();
    assert_eq!(points(&h).last(), Some(&(8 * Q, 140_000, false)));
    let sel: Vec<EventId> = h.selection().events.iter().copied().collect();
    assert_eq!(sel, vec![ids[1]], "the notes under the line were selected");
}
