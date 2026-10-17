use core_model::command::{
    Applied, Command, Context, EventCmd, SequenceCmd, SettingsCmd, apply, apply_in,
};
use core_model::document::Document;
use core_model::event::{EventBody, EventKind, EventSpec};
use core_model::history::History;
use core_model::ids::{AnyId, EventId, LaneId, SeqId};
use core_model::modify::{
    Amount, ChangeOp, Curve, Density, DurationOp, Modify, ModifyTemplate, Quantize, Substitute,
    Transpose, ValueOp, ValueTarget, Wave,
};
use core_model::rule::{Rule, SelectMode, events_matching};
use core_model::selection::{Selection, ViewFilter};
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;

fn note(t: i64, pitch: u8, vel: u16, dur: i64) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::note(pitch, vel, Tick(dur)))
}

/// seq0/lane0 with: ev0 note 60 @0, ev1 note 64 @Q+10, ev2 note 67 @2Q,
/// ev3 CC7=100 @3Q, ev4 note 72 @4Q (velocities 10, 20, 30, -, 40).
fn doc() -> Document {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let events = vec![
        note(0, 60, 10, Q / 2),
        note(Q + 10, 64, 20, Q / 2),
        note(2 * Q, 67, 30, Q),
        EventSpec::new(
            Tick(3 * Q),
            EventBody::Cc {
                index: 7,
                value: 100,
            },
        ),
        note(4 * Q, 72, 40, 2 * Q),
    ];
    apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events,
        }),
    )
    .unwrap()
    .doc
}

fn lane_sel(events: &[u64]) -> Selection {
    Selection {
        time_range: None,
        lanes: [(SeqId(0), LaneId(0))].into_iter().collect(),
        events: events.iter().map(|e| EventId(*e)).collect(),
    }
}

fn sm(
    d: &Document,
    sel: &Selection,
    filter: &ViewFilter,
    rule: Option<Rule>,
    mode: SelectMode,
    modify: Option<Modify>,
    to_copy: bool,
) -> Applied {
    apply_in(
        d,
        Context {
            selection: sel,
            filter,
        },
        &Command::SelectAndModify {
            rule,
            mode,
            modify,
            to_copy,
        },
    )
    .unwrap()
}

fn ids(s: &Selection) -> Vec<u64> {
    s.events.iter().map(|e| e.0).collect()
}

fn matching(d: &Document, rule: Rule) -> Vec<u64> {
    events_matching(d, &lane_sel(&[]).lanes, &rule, &ViewFilter::default())
        .into_iter()
        .map(|e| e.0)
        .collect()
}

fn body(d: &Document, id: u64) -> EventBody {
    d.event(EventId(id)).unwrap().0.body.clone()
}

#[test]
fn rules_match_the_right_events() {
    let d = doc();
    assert_eq!(
        matching(
            &d,
            Rule::Kind {
                kinds: vec![EventKind::Cc]
            }
        ),
        vec![3]
    );
    assert_eq!(
        matching(&d, Rule::PitchRange { low: 62, high: 70 }),
        vec![1, 2]
    );
    assert_eq!(
        matching(&d, Rule::VelocityRange { low: 15, high: 35 }),
        vec![1, 2]
    );
    assert_eq!(
        matching(
            &d,
            Rule::DurationRange {
                min: Tick(Q),
                max: Tick(4 * Q)
            }
        ),
        vec![2, 4]
    );
    assert_eq!(
        matching(
            &d,
            Rule::TimeRange {
                start: Tick(Q),
                end: Tick(3 * Q)
            }
        ),
        vec![1, 2]
    );
    assert_eq!(
        matching(
            &d,
            Rule::MetricPosition {
                grid: Tick(Q),
                offset: Tick(0),
                tolerance: Tick(0)
            }
        ),
        vec![0, 2, 3, 4]
    );
    assert_eq!(matching(&d, Rule::EveryNth { n: 2, phase: 1 }), vec![1, 3]);
    assert_eq!(matching(&d, Rule::Dest { dest: None }), vec![0, 1, 2, 3, 4]);
    let combo = Rule::All(vec![
        Rule::Kind {
            kinds: vec![EventKind::Note],
        },
        Rule::Not(Box::new(Rule::Any(vec![
            Rule::PitchRange { low: 60, high: 60 },
            Rule::PitchRange { low: 72, high: 72 },
        ]))),
    ]);
    assert_eq!(matching(&d, combo), vec![1, 2]);
}

#[test]
fn select_add_and_refine() {
    let d = doc();
    let f = ViewFilter::default();
    let notes = Rule::Kind {
        kinds: vec![EventKind::Note],
    };
    let a = sm(
        &d,
        &lane_sel(&[3]),
        &f,
        Some(notes.clone()),
        SelectMode::Select,
        None,
        false,
    );
    assert_eq!(ids(&a.selection.unwrap()), vec![0, 1, 2, 4]);
    let a = sm(
        &d,
        &lane_sel(&[3]),
        &f,
        Some(Rule::PitchRange { low: 72, high: 72 }),
        SelectMode::Add,
        None,
        false,
    );
    assert_eq!(ids(&a.selection.unwrap()), vec![3, 4]);
    let a = sm(
        &d,
        &lane_sel(&[0, 1, 3]),
        &f,
        Some(notes),
        SelectMode::Refine,
        None,
        false,
    );
    assert_eq!(ids(&a.selection.unwrap()), vec![0, 1]);
    // Selection-only: the document is unchanged.
    assert_eq!(a.doc, d);
}

#[test]
fn hidden_events_cannot_be_selected_or_changed() {
    let d = doc();
    let mut f = ViewFilter::default();
    f.hidden_kinds.insert(EventKind::Cc);
    let a = sm(
        &d,
        &lane_sel(&[0, 3]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::ControllerValue(ValueOp::Set(1))),
        false,
    );
    assert_eq!(ids(&a.selection.unwrap()), vec![0]);
    assert_eq!(
        body(&a.doc, 3),
        EventBody::Cc {
            index: 7,
            value: 100
        }
    );
}

#[test]
fn transpose_interval_and_scale() {
    let d = doc();
    let f = ViewFilter::default();
    let a = sm(
        &d,
        &lane_sel(&[0, 4]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Transpose(Transpose::Interval { semitones: 60 })),
        false,
    );
    assert_eq!(body(&a.doc, 0), EventBody::note(120, 10, Tick(Q / 2)));
    assert_eq!(body(&a.doc, 4), EventBody::note(127, 40, Tick(2 * Q)));
    // C major pentatonic (C D E G A): 66 (F#) snaps to the nearest member, 67 (G).
    let penta = Transpose::ToScale {
        root: 0,
        mask: 0b0010_1001_0101,
    };
    let a = sm(
        &d,
        &lane_sel(&[1]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Transpose(Transpose::Interval { semitones: 2 })),
        false,
    );
    let a = sm(
        &a.doc,
        &lane_sel(&[1]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Transpose(penta)),
        false,
    );
    assert_eq!(body(&a.doc, 1), EventBody::note(67, 20, Tick(Q / 2)));
    // 61 (C#) is equidistant from C and D: ties go down.
    let a = sm(
        &d,
        &lane_sel(&[0]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Transpose(Transpose::Interval { semitones: 1 })),
        false,
    );
    let a = sm(
        &a.doc,
        &lane_sel(&[0]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Transpose(penta)),
        false,
    );
    assert_eq!(body(&a.doc, 0), EventBody::note(60, 10, Tick(Q / 2)));
}

#[test]
fn quantize_grid_and_groove() {
    let d = doc();
    let f = ViewFilter::default();
    let a = sm(
        &d,
        &lane_sel(&[1]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Quantize(Quantize::Grid {
            grid: Tick(Q),
            strength: 100,
        })),
        false,
    );
    assert_eq!(a.doc.event(EventId(1)).unwrap().0.tick, Tick(Q));
    let groove = Quantize::Groove {
        grid: Tick(Q),
        offsets: vec![Tick(0), Tick(30)],
        strength: 100,
    };
    let a = sm(
        &d,
        &lane_sel(&[1]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Quantize(groove)),
        false,
    );
    assert_eq!(a.doc.event(EventId(1)).unwrap().0.tick, Tick(Q + 30));
    let bad = Quantize::Grid {
        grid: Tick(0),
        strength: 100,
    };
    assert!(
        apply_in(
            &d,
            Context {
                selection: &lane_sel(&[1]),
                filter: &f
            },
            &Command::SelectAndModify {
                rule: None,
                mode: SelectMode::Select,
                modify: Some(Modify::Quantize(bad)),
                to_copy: false
            }
        )
        .is_err()
    );
}

#[test]
fn velocity_duration_legato_scale_move_dest() {
    let d = doc();
    let f = ViewFilter::default();
    let s = lane_sel(&[0, 1, 2]);
    let a = sm(
        &d,
        &s,
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Velocity(ValueOp::ScalePercent(200))),
        false,
    );
    assert_eq!(body(&a.doc, 2), EventBody::note(67, 60, Tick(Q)));
    let a = sm(
        &a.doc,
        &s,
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Velocity(ValueOp::Add(-100))),
        false,
    );
    assert_eq!(body(&a.doc, 2), EventBody::note(67, 0, Tick(Q)));
    let a = sm(
        &d,
        &s,
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Duration(DurationOp::Set(Tick(7)))),
        false,
    );
    assert_eq!(body(&a.doc, 0), EventBody::note(60, 10, Tick(7)));
    let a = sm(
        &d,
        &s,
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Legato),
        false,
    );
    assert_eq!(body(&a.doc, 0), EventBody::note(60, 10, Tick(Q + 10)));
    assert_eq!(body(&a.doc, 2), EventBody::note(67, 30, Tick(Q)));
    let a = sm(
        &d,
        &lane_sel(&[2]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::ScaleTime {
            num: 1,
            den: 2,
            anchor: Tick(0),
        }),
        false,
    );
    let (e, _) = a.doc.event(EventId(2)).unwrap();
    assert_eq!(
        (e.tick, e.body.clone()),
        (Tick(Q), EventBody::note(67, 30, Tick(Q / 2)))
    );
    let a = sm(
        &d,
        &lane_sel(&[2]),
        &f,
        None,
        SelectMode::Select,
        Some(Modify::Move { delta: Tick(-Q) }),
        false,
    );
    assert_eq!(a.doc.event(EventId(2)).unwrap().0.tick, Tick(Q));
    let unknown = apply_in(
        &d,
        Context {
            selection: &lane_sel(&[2]),
            filter: &f,
        },
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(Modify::ReassignDest {
                dest: Some(core_model::ids::DestId(5)),
            }),
            to_copy: false,
        },
    );
    assert!(unknown.is_err());
}

#[test]
fn modify_copies_leaves_originals() {
    let d = doc();
    let a = sm(
        &d,
        &lane_sel(&[0]),
        &ViewFilter::default(),
        None,
        SelectMode::Select,
        Some(Modify::Transpose(Transpose::Interval { semitones: 12 })),
        true,
    );
    assert_eq!(a.report.created, vec![AnyId::Event(EventId(5))]);
    assert_eq!(ids(&a.selection.unwrap()), vec![5]);
    assert_eq!(body(&a.doc, 0), EventBody::note(60, 10, Tick(Q / 2)));
    assert_eq!(body(&a.doc, 5), EventBody::note(72, 10, Tick(Q / 2)));
}

#[test]
fn templates_are_stored_in_settings() {
    let t = ModifyTemplate {
        rule: Some(Rule::Kind {
            kinds: vec![EventKind::Note],
        }),
        mode: SelectMode::Select,
        modify: Some(Modify::Quantize(Quantize::Grid {
            grid: Tick(Q / 4),
            strength: 80,
        })),
        to_copy: true,
    };
    let d = apply(
        &doc(),
        &Command::Settings(SettingsCmd::SetTemplate {
            name: "Q16".into(),
            template: Some(t.clone()),
        }),
    )
    .unwrap()
    .doc;
    assert_eq!(d.settings().templates.get("Q16"), Some(&t));
    let json = serde_json::to_string(&d).unwrap();
    let back: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
    let d = apply(
        &d,
        &Command::Settings(SettingsCmd::SetTemplate {
            name: "Q16".into(),
            template: None,
        }),
    )
    .unwrap()
    .doc;
    assert!(d.settings().templates.is_empty());
    assert!(
        apply(
            &d,
            &Command::Settings(SettingsCmd::SetHistoryDepth { depth: 0 })
        )
        .is_err()
    );
}

#[test]
fn undo_label_names_the_modification() {
    let c = Command::SelectAndModify {
        rule: None,
        mode: SelectMode::Select,
        modify: Some(Modify::Legato),
        to_copy: false,
    };
    assert_eq!(c.name(), "Legato");
    let c = Command::SelectAndModify {
        rule: None,
        mode: SelectMode::Select,
        modify: None,
        to_copy: false,
    };
    assert_eq!(c.name(), "Select");
}

#[test]
fn value_controller_release_and_bracketing_rules() {
    let d = doc();
    // ev3 is CC7 = 100; the notes have release velocity 0.
    assert_eq!(
        matching(&d, Rule::ValueRange { low: 50, high: 150 }),
        vec![3]
    );
    assert_eq!(
        matching(&d, Rule::ControllerRange { low: 7, high: 7 }),
        vec![3]
    );
    assert!(matching(&d, Rule::ControllerRange { low: 8, high: 127 }).is_empty());
    assert_eq!(
        matching(&d, Rule::ReleaseVelocityRange { low: 0, high: 0 }),
        vec![0, 1, 2, 4]
    );
    // ev5: a CC at 5Q, inside ev4 (72, 4Q..6Q).
    let d = apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![EventSpec::new(
                Tick(5 * Q),
                EventBody::Cc { index: 1, value: 5 },
            )],
        }),
    )
    .unwrap()
    .doc;
    let under_72 = Rule::Bracketed {
        by: Box::new(Rule::PitchRange { low: 72, high: 72 }),
    };
    assert_eq!(matching(&d, under_72), vec![5]);
    // No note starts inside another note; ev3 at 3Q is at ev2's end (excluded).
    let under_any_note = Rule::Bracketed {
        by: Box::new(Rule::Kind {
            kinds: vec![EventKind::Note],
        }),
    };
    assert_eq!(matching(&d, under_any_note), vec![5]);
    // "Outside" is a negated range.
    let outside = Rule::All(vec![
        Rule::Kind {
            kinds: vec![EventKind::Note],
        },
        Rule::Not(Box::new(Rule::PitchRange { low: 60, high: 64 })),
    ]);
    assert_eq!(matching(&d, outside), vec![2, 4]);
}

#[test]
fn program_numbers_match_value_ranges() {
    let d = apply(
        &doc(),
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![EventSpec::new(
                Tick(0),
                EventBody::Program {
                    program: 9,
                    bank: None,
                },
            )],
        }),
    )
    .unwrap()
    .doc;
    assert_eq!(matching(&d, Rule::ValueRange { low: 9, high: 9 }), vec![5]);
}

#[test]
fn a_template_saved_before_to_copy_existed_still_loads() {
    let t: ModifyTemplate =
        serde_json::from_str(r#"{"rule":null,"mode":"select","modify":"legato"}"#).unwrap();
    assert!(!t.to_copy);
    assert_eq!(t.modify, Some(Modify::Legato));
}

const C_MAJOR: u16 = 0b1010_1011_0101;
/// The notes of `doc()` in time order: 60, 64, 67, 72; velocities 10–40.
const NOTES: [u64; 4] = [0, 1, 2, 4];

fn try_sm(d: &Document, sel: &Selection, modify: Modify) -> core_model::Result<Applied> {
    apply_in(
        d,
        Context {
            selection: sel,
            filter: &ViewFilter::default(),
        },
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(modify),
            to_copy: false,
        },
    )
}

fn notes_after(modify: Modify) -> Document {
    try_sm(&doc(), &lane_sel(&NOTES), modify).unwrap().doc
}

fn pitches(d: &Document) -> Vec<u8> {
    NOTES
        .iter()
        .map(|i| match body(d, *i) {
            EventBody::Note { pitch, .. } => pitch,
            b => panic!("{b:?}"),
        })
        .collect()
}

fn velocities(d: &Document) -> Vec<u16> {
    NOTES
        .iter()
        .map(|i| match body(d, *i) {
            EventBody::Note { velocity, .. } => velocity,
            b => panic!("{b:?}"),
        })
        .collect()
}

fn change(target: ValueTarget, op: ChangeOp, amount: Amount) -> Modify {
    Modify::Change { target, op, amount }
}

fn velocity_change(op: ChangeOp, amount: Amount) -> Vec<u16> {
    velocities(&notes_after(change(ValueTarget::Velocity, op, amount)))
}

const fn constant(value: i64) -> Amount {
    Amount::Constant { value }
}

#[test]
fn diatonic_inversion_and_note_map() {
    let d = notes_after(Modify::Transpose(Transpose::Diatonic {
        steps: 2,
        root: 0,
        mask: C_MAJOR,
    }));
    assert_eq!(
        pitches(&d),
        vec![64, 67, 71, 76],
        "two degrees up in C major"
    );
    let d = notes_after(Modify::Transpose(Transpose::Invert { axis: 64 }));
    assert_eq!(pitches(&d), vec![68, 64, 61, 56], "mirrored around E4");
    let d = notes_after(Modify::NoteMap {
        map: vec![(60, 36), (67, 38)],
    });
    assert_eq!(pitches(&d), vec![36, 64, 38, 72]);
    let sel = lane_sel(&NOTES);
    for bad in [
        Modify::Transpose(Transpose::Diatonic {
            steps: 1,
            root: 12,
            mask: C_MAJOR,
        }),
        Modify::Transpose(Transpose::Diatonic {
            steps: 1,
            root: 0,
            mask: 0,
        }),
        Modify::Transpose(Transpose::Invert { axis: 128 }),
        Modify::NoteMap {
            map: vec![(60, 200)],
        },
    ] {
        assert!(try_sm(&doc(), &sel, bad.clone()).is_err(), "{bad:?}");
    }
    assert_eq!(Modify::NoteMap { map: vec![] }.name(), "Transpose");
}

#[test]
fn constant_value_changes() {
    assert_eq!(
        velocity_change(ChangeOp::Add, constant(5)),
        vec![15, 25, 35, 45]
    );
    assert_eq!(
        velocity_change(ChangeOp::Scale, constant(50)),
        vec![5, 10, 15, 20]
    );
    assert_eq!(
        velocity_change(ChangeOp::Set, constant(70_000)),
        vec![65_535; 4]
    );
    assert_eq!(velocity_change(ChangeOp::Add, constant(-100)), vec![0; 4]);
    assert_eq!(
        velocity_change(ChangeOp::Limit { min: 15, max: 35 }, constant(0)),
        vec![15, 20, 30, 35]
    );
    assert_eq!(
        velocity_change(
            ChangeOp::Compress {
                low: 100,
                high: 400
            },
            constant(0)
        ),
        vec![100, 200, 300, 400]
    );
}

#[test]
fn shaped_and_oscillating_amounts_follow_the_event_times() {
    // The notes start at 0, Q+10, 2Q and 4Q.
    let line = |curve| {
        velocity_change(
            ChangeOp::Set,
            Amount::Line {
                from: 0,
                to: 400,
                curve,
            },
        )
    };
    assert_eq!(line(Curve::Straight), vec![0, 100, 200, 400]);
    assert_eq!(line(Curve::EaseIn), vec![0, 25, 100, 400]);
    let osc = |wave, period| {
        velocity_change(
            ChangeOp::Set,
            Amount::Oscillator {
                wave,
                low: 10,
                high: 90,
                period: Tick(period),
                phase: Tick(0),
            },
        )
    };
    assert_eq!(osc(Wave::Square, 2 * Q), vec![90, 10, 90, 90]);
    assert_eq!(osc(Wave::Saw, 4 * Q), vec![10, 30, 50, 10]);
    let random = || {
        velocity_change(
            ChangeOp::Set,
            Amount::Random {
                low: 10,
                high: 20,
                seed: 7,
            },
        )
    };
    let a = random();
    assert_eq!(a, random(), "the same seed gives the same values");
    assert!(a.iter().all(|v| (10..=20).contains(v)), "{a:?}");
}

#[test]
fn release_velocity_duration_and_value_targets() {
    let d = notes_after(change(
        ValueTarget::ReleaseVelocity,
        ChangeOp::Set,
        constant(7),
    ));
    for i in NOTES {
        assert!(matches!(
            body(&d, i),
            EventBody::Note {
                release_velocity: 7,
                ..
            }
        ));
    }
    let d = notes_after(change(
        ValueTarget::Duration,
        ChangeOp::Add,
        constant(-Q / 4),
    ));
    let durations: Vec<i64> = NOTES
        .iter()
        .map(|i| match body(&d, *i) {
            EventBody::Note { duration, .. } => duration.0,
            b => panic!("{b:?}"),
        })
        .collect();
    assert_eq!(durations, vec![Q / 4, Q / 4, 3 * Q / 4, 7 * Q / 4]);
    let d = notes_after(change(ValueTarget::Duration, ChangeOp::Legato, constant(0)));
    assert!(matches!(body(&d, 0), EventBody::Note { duration: Tick(t), .. } if t == Q + 10));
    // The CC (ev3) is the only value target.
    let d = try_sm(
        &doc(),
        &lane_sel(&[0, 3]),
        change(ValueTarget::Value, ChangeOp::Add, constant(10)),
    )
    .unwrap()
    .doc;
    assert_eq!(
        body(&d, 3),
        EventBody::Cc {
            index: 7,
            value: 110
        }
    );
    assert!(matches!(body(&d, 0), EventBody::Note { velocity: 10, .. }));
}

fn durations(d: &Document) -> Vec<i64> {
    NOTES
        .iter()
        .map(|i| match body(d, *i) {
            EventBody::Note { duration, .. } => duration.0,
            b => panic!("{b:?}"),
        })
        .collect()
}

#[test]
fn shortening_notes_keeps_at_least_one_tick_unless_set_to_zero() {
    let dur = |op, amount| durations(&notes_after(change(ValueTarget::Duration, op, amount)));
    assert_eq!(dur(ChangeOp::Add, constant(-100 * Q)), vec![1; 4]);
    assert_eq!(dur(ChangeOp::Scale, constant(0)), vec![1; 4]);
    assert_eq!(
        dur(ChangeOp::Limit { min: 0, max: 0 }, constant(0)),
        vec![1; 4]
    );
    assert_eq!(
        dur(ChangeOp::Compress { low: -5, high: 0 }, constant(0)),
        vec![1; 4]
    );
    assert_eq!(dur(ChangeOp::Set, constant(0)), vec![0; 4]);
    // A zero-length note stays zero when changed relatively.
    let zero = notes_after(change(ValueTarget::Duration, ChangeOp::Set, constant(0)));
    let d = try_sm(
        &zero,
        &lane_sel(&NOTES),
        change(ValueTarget::Duration, ChangeOp::Add, constant(-5)),
    )
    .unwrap()
    .doc;
    assert_eq!(durations(&d), vec![0; 4]);
}

#[test]
fn bad_amounts_and_targets_are_refused() {
    let sel = lane_sel(&NOTES);
    for bad in [
        change(ValueTarget::Velocity, ChangeOp::Legato, constant(0)),
        change(
            ValueTarget::Velocity,
            ChangeOp::Limit { min: 5, max: 4 },
            constant(0),
        ),
        change(
            ValueTarget::Velocity,
            ChangeOp::Set,
            Amount::Oscillator {
                wave: Wave::Sine,
                low: 0,
                high: 1,
                period: Tick(0),
                phase: Tick(0),
            },
        ),
    ] {
        assert!(try_sm(&doc(), &sel, bad.clone()).is_err(), "{bad:?}");
    }
    assert_eq!(
        change(ValueTarget::Velocity, ChangeOp::Set, constant(0)).name(),
        "Change Velocity"
    );
    assert_eq!(
        change(ValueTarget::Value, ChangeOp::Set, constant(0)).name(),
        "Change Values"
    );
}

/// seq0/lane0 with CC7 at ticks `at` (value = tick) and nothing else.
fn controllers(at: &[i64]) -> Document {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let events = at
        .iter()
        .map(|t| {
            EventSpec::new(
                Tick(*t),
                EventBody::Cc {
                    index: 7,
                    value: *t as u32,
                },
            )
        })
        .collect();
    apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events,
        }),
    )
    .unwrap()
    .doc
}

fn cc_ticks(d: &Document) -> Vec<(i64, u32)> {
    d.lane(SeqId(0), LaneId(0))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Cc { value, .. } => Some((e.tick.0, value)),
            _ => None,
        })
        .collect()
}

#[test]
fn thin_out_keeps_events_at_least_the_spacing_apart() {
    let d = controllers(&[0, 10, 20, 100, 200]);
    let a = try_sm(
        &d,
        &lane_sel(&[0, 1, 2, 3, 4]),
        Modify::Density(Density::Thin { spacing: Tick(50) }),
    )
    .unwrap();
    assert_eq!(cc_ticks(&a.doc), vec![(0, 0), (100, 100), (200, 200)]);
    assert_eq!(ids(a.selection.as_ref().unwrap()), vec![0, 3, 4]);
}

#[test]
fn thicken_fills_gaps_on_a_straight_line() {
    let d = controllers(&[0, 100]);
    let a = try_sm(
        &d,
        &lane_sel(&[0, 1]),
        Modify::Density(Density::Thicken { spacing: Tick(30) }),
    )
    .unwrap();
    assert_eq!(
        cc_ticks(&a.doc),
        vec![(0, 0), (30, 30), (60, 60), (90, 90), (100, 100)]
    );
    assert_eq!(a.selection.unwrap().events.len(), 5);
    assert!(
        try_sm(
            &d,
            &lane_sel(&[0, 1]),
            Modify::Density(Density::Thin { spacing: Tick(0) })
        )
        .is_err()
    );
}

#[test]
fn trim_start_keeps_the_end() {
    // ev2: 67 at 2Q, one quarter long.
    let d = try_sm(
        &doc(),
        &lane_sel(&[2]),
        Modify::TrimStart { delta: Tick(Q / 4) },
    )
    .unwrap()
    .doc;
    let (ev, _) = d.event(EventId(2)).unwrap();
    assert_eq!(ev.tick, Tick(2 * Q + Q / 4));
    assert!(matches!(ev.body, EventBody::Note { duration: Tick(t), .. } if t == 3 * Q / 4));
    let d = try_sm(
        &doc(),
        &lane_sel(&[2]),
        Modify::TrimStart { delta: Tick(2 * Q) },
    )
    .unwrap()
    .doc;
    let (ev, _) = d.event(EventId(2)).unwrap();
    assert_eq!(ev.tick, Tick(3 * Q - 1), "one tick is left");
}

#[test]
fn substitute_changes_kind_or_pitch() {
    let sub = |s: Substitute, ids: &[u64]| try_sm(&doc(), &lane_sel(ids), Modify::Substitute(s));
    let d = sub(Substitute::Controller { index: 11 }, &[3]).unwrap().doc;
    assert_eq!(
        body(&d, 3),
        EventBody::Cc {
            index: 11,
            value: 100
        }
    );
    let d = sub(Substitute::PitchBend, &[3]).unwrap().doc;
    assert_eq!(body(&d, 3), EventBody::PitchBend { value: 100 });
    let d = sub(Substitute::Pitch { pitch: 36 }, &NOTES).unwrap().doc;
    assert_eq!(pitches(&d), vec![36; 4]);
    assert!(sub(Substitute::Controller { index: 200 }, &[3]).is_err());
}

#[test]
fn delete_removes_the_selection_and_undoes() {
    let mut h = History::new(doc());
    h.set_selection(lane_sel(&[0, 1]));
    h.execute(
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(Modify::Delete),
            to_copy: false,
        },
        &ViewFilter::default(),
    )
    .unwrap();
    assert!(h.doc().event(EventId(0)).is_err());
    assert!(h.selection().events.is_empty());
    assert_eq!(h.undo_label(), Some("Delete Events"));
    h.undo().unwrap();
    assert!(h.doc().event(EventId(0)).is_ok());
}

#[test]
fn a_modify_that_changes_nothing_adds_no_undo_step() {
    let mut h = History::new(doc());
    // No events selected, no lanes: nothing to change.
    h.execute(
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(Modify::Delete),
            to_copy: false,
        },
        &ViewFilter::default(),
    )
    .unwrap();
    assert_eq!(h.undo_label(), None);
    assert_eq!(
        Modify::Density(Density::Thin { spacing: Tick(1) }).name(),
        "Thin Out"
    );
    assert_eq!(Modify::TrimStart { delta: Tick(1) }.name(), "Trim Start");
}

#[test]
fn substitute_keeps_the_stored_value_across_controller_kinds() {
    // Every controller kind stores the same 32-bit range, so the value
    // carries over exactly, including a full-scale 7-bit value.
    let full = 127u32 << 25;
    let d = controllers(&[0]);
    let d = apply(
        &d,
        &Command::Event(EventCmd::SetValue {
            events: vec![EventId(0)],
            value: full,
        }),
    )
    .unwrap()
    .doc;
    let v = full;
    for (s, want) in [
        (Substitute::PitchBend, EventBody::PitchBend { value: v }),
        (
            Substitute::ChannelPressure,
            EventBody::ChannelPressure { value: v },
        ),
        (
            Substitute::Controller { index: 1 },
            EventBody::Cc { index: 1, value: v },
        ),
    ] {
        let a = try_sm(&d, &lane_sel(&[0]), Modify::Substitute(s)).unwrap();
        assert_eq!(body(&a.doc, 0), want);
    }
}

#[test]
fn invalid_arguments_are_refused_even_without_notes() {
    use core_model::ModelError;
    let d = controllers(&[0]);
    for m in [
        Modify::Transpose(Transpose::Diatonic {
            steps: 1,
            root: 12,
            mask: C_MAJOR,
        }),
        Modify::Transpose(Transpose::Diatonic {
            steps: 1,
            root: 0,
            mask: 0,
        }),
        Modify::Transpose(Transpose::Invert { axis: 128 }),
        Modify::NoteMap {
            map: vec![(1, 200)],
        },
        Modify::Substitute(Substitute::Controller { index: 200 }),
        Modify::Substitute(Substitute::Pitch { pitch: 200 }),
    ] {
        assert!(
            matches!(
                try_sm(&d, &lane_sel(&[0]), m.clone()),
                Err(ModelError::OutOfRange { .. })
            ),
            "{m:?}"
        );
    }
    assert!(matches!(
        try_sm(
            &d,
            &lane_sel(&[0]),
            Modify::Density(Density::Thin { spacing: Tick(0) })
        ),
        Err(ModelError::OutOfRange { .. })
    ));
}

#[test]
fn to_copy_delete_and_no_op_changes_add_no_undo_step() {
    let mut h = History::new(doc());
    h.set_selection(lane_sel(&[0, 1]));
    let before = h.doc().clone();
    h.execute(
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(Modify::Delete),
            to_copy: true,
        },
        &ViewFilter::default(),
    )
    .unwrap();
    assert_eq!(*h.doc(), before);
    assert_eq!(h.undo_label(), None);
    assert_eq!(ids(h.selection()), vec![0, 1]);
    // Thinning a series that is already sparse changes nothing either.
    let mut h = History::new(controllers(&[0, 100]));
    h.set_selection(lane_sel(&[0, 1]));
    h.execute(
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(Modify::Density(Density::Thin { spacing: Tick(10) })),
            to_copy: false,
        },
        &ViewFilter::default(),
    )
    .unwrap();
    assert_eq!(h.undo_label(), None);
}

#[test]
fn scale_time_keeps_a_note_with_a_length_at_least_one_tick_long() {
    // 1/1 000 000 of a half note or a quarter is under one tick.
    let d = notes_after(Modify::ScaleTime {
        num: 1,
        den: 1_000_000,
        anchor: Tick(0),
    });
    assert_eq!(durations(&d), vec![1; 4]);
    // A zero-length note stays zero.
    let zero = notes_after(change(ValueTarget::Duration, ChangeOp::Set, constant(0)));
    let d = try_sm(
        &zero,
        &lane_sel(&NOTES),
        Modify::ScaleTime {
            num: 3,
            den: 1,
            anchor: Tick(0),
        },
    )
    .unwrap()
    .doc;
    assert_eq!(durations(&d), vec![0; 4]);
}
