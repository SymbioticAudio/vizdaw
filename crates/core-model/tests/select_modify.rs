use core_model::command::{
    Applied, Command, Context, EventCmd, SequenceCmd, SettingsCmd, apply, apply_in,
};
use core_model::document::Document;
use core_model::event::{EventBody, EventKind, EventSpec};
use core_model::ids::{AnyId, EventId, LaneId, SeqId};
use core_model::modify::{DurationOp, Modify, ModifyTemplate, Quantize, Transpose, ValueOp};
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
