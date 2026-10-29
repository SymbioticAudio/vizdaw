//! Automation a pass wrote (plan 5e-1 Task 3; sub-project 5 spec §4.7,
//! §7.3, §7.4): each write goes into the first lane of its sequence that
//! already holds its target, else into a new lane at the end; it replaces
//! the target's points in its range in every lane that holds the target
//! (pre-flight I5), and nothing else; a bad write refuses the whole pass;
//! a pass with only automation is "Write Automation".

use core_model::automation::AutomationTarget;
use core_model::command::{
    AutomationWrite, Command, EventCmd, LaneCmd, RecordPass, SequenceCmd, apply,
};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{LaneId, SeqId, StripId};
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;

fn fader(d: &Document) -> AutomationTarget {
    AutomationTarget::StripFader(d.mixer().master())
}

fn point(t: i64, target: AutomationTarget, v: f64) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::Automation { target, value: v })
}

/// Sequence 0 with lanes A (0, a note), B (1, the master fader at 0 and
/// 8Q) and C (2, the master fader at 2Q).
fn doc() -> Document {
    let mut d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    for name in ["B", "C"] {
        d = apply(
            &d,
            &Command::Lane(LaneCmd::Add {
                seq: SeqId(0),
                name: name.into(),
                index: None,
            }),
        )
        .unwrap()
        .doc;
    }
    let f = fader(&d);
    for (lane, events) in [
        (
            0,
            vec![EventSpec::new(
                Tick(0),
                EventBody::note(60, 30_000, Tick(Q)),
            )],
        ),
        (1, vec![point(0, f, 0.2), point(8 * Q, f, 0.8)]),
        (2, vec![point(2 * Q, f, 0.5)]),
    ] {
        d = apply(
            &d,
            &Command::Event(EventCmd::Insert {
                seq: SeqId(0),
                lane: LaneId(lane),
                events,
            }),
        )
        .unwrap()
        .doc;
    }
    d
}

fn write(d: &Document, start: i64, end: i64, points: &[(i64, f64)]) -> AutomationWrite {
    AutomationWrite {
        seq: SeqId(0),
        target: fader(d),
        start: Tick(start),
        end: Tick(end),
        points: points.iter().map(|(t, v)| (Tick(*t), *v)).collect(),
        lane_name: "Master Fader".into(),
    }
}

fn pass(writes: Vec<AutomationWrite>) -> RecordPass {
    RecordPass {
        automation: writes,
        ..RecordPass::default()
    }
}

/// `(tick, value)` of lane `lane`'s automation, by tick.
fn points(d: &Document, lane: u64) -> Vec<(i64, f64)> {
    let mut v: Vec<(i64, f64)> = d
        .lane(SeqId(0), LaneId(lane))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Automation { value, .. } => Some((e.tick.0, value)),
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

#[test]
fn a_write_goes_into_the_first_lane_holding_its_target() {
    let d = doc();
    let w = write(
        &d,
        Q - 1,
        4 * Q + 1,
        &[(Q - 1, 0.3), (Q, 0.9), (4 * Q, 0.9), (4 * Q + 1, 0.4)],
    );
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    // B (the first holding the master fader) gets it; its points outside
    // the range stay; C's point inside the range goes too (every lane
    // plays, pre-flight I5); A is untouched; no lane is added.
    assert_eq!(
        points(&out, 1),
        vec![
            (0, 0.2),
            (Q - 1, 0.3),
            (Q, 0.9),
            (4 * Q, 0.9),
            (4 * Q + 1, 0.4),
            (8 * Q, 0.8)
        ]
    );
    assert!(
        points(&out, 2).is_empty(),
        "C's point inside the range is replaced too: {:?}",
        points(&out, 2)
    );
    assert_eq!(out.sequence(SeqId(0)).unwrap().lanes.len(), 3);
    // A second write at C's old tick still goes to B (C is not first).
    let w = write(&d, 2 * Q, 2 * Q, &[(2 * Q, 0.1)]);
    let out = apply(&out, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert!(
        points(&out, 2).is_empty(),
        "C's point inside the range is replaced too: {:?}",
        points(&out, 2)
    );
    assert!(points(&out, 1).contains(&(2 * Q, 0.1)));
}

/// Amended Ruling 4: a write removes the target's points in its range
/// from every lane holding the target, and leaves their points outside
/// it.
#[test]
fn a_write_leaves_other_lanes_outside_its_range() {
    let d = doc();
    let f = fader(&d);
    let d = apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(2),
            events: vec![point(6 * Q, f, 0.7)],
        }),
    )
    .unwrap()
    .doc;
    let w = write(&d, Q - 1, 4 * Q + 1, &[(Q - 1, 0.3), (4 * Q + 1, 0.4)]);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert_eq!(
        points(&out, 2),
        vec![(6 * Q, 0.7)],
        "C keeps its point outside the range"
    );
    assert_eq!(
        points(&out, 1),
        vec![(0, 0.2), (Q - 1, 0.3), (4 * Q + 1, 0.4), (8 * Q, 0.8)]
    );
}

#[test]
fn a_write_with_no_lane_for_its_target_makes_one_at_the_end() {
    let d = doc();
    let pan = AutomationTarget::StripPan(StripId(99));
    let mut w = write(&d, 0, Q, &[(0, 0.5), (Q, 0.5)]);
    w.target = pan;
    assert!(
        apply(&d, &Command::Record(pass(vec![w.clone()]))).is_err(),
        "strip 99 does not exist"
    );
    w.target = AutomationTarget::StripPan(d.mixer().master());
    w.lane_name = "Master Pan".into();
    let mut h = History::new(d.clone());
    h.execute(&Command::Record(pass(vec![w])), &ViewFilter::default())
        .unwrap();
    assert_eq!(h.undo_label(), Some("Write Automation"));
    let s = h.doc().sequence(SeqId(0)).unwrap();
    assert_eq!(s.lanes.len(), 4);
    let last = s.lanes.last().unwrap();
    assert_eq!(last.name, "Master Pan");
    assert_eq!(points(h.doc(), last.id.0), vec![(0, 0.5), (Q, 0.5)]);
    h.undo().unwrap();
    assert_eq!(h.doc(), &d, "one undo takes the lane and its points back");
}

#[test]
fn a_loop_passes_later_write_wins_where_they_overlap() {
    let d = doc();
    let first = write(&d, 0, 4 * Q, &[(0, 0.1), (4 * Q, 0.1)]);
    let second = write(&d, 2 * Q, 6 * Q, &[(2 * Q, 0.6), (6 * Q, 0.6)]);
    let out = apply(&d, &Command::Record(pass(vec![first, second])))
        .unwrap()
        .doc;
    assert_eq!(
        points(&out, 1),
        vec![(0, 0.1), (2 * Q, 0.6), (6 * Q, 0.6), (8 * Q, 0.8)]
    );
}

#[test]
fn bad_writes_refuse_the_whole_pass() {
    let d = doc();
    let good = write(&d, 0, Q, &[(0, 0.5)]);
    let mut cases: Vec<AutomationWrite> = Vec::new();
    let mut w = good.clone();
    w.points = vec![(Tick(0), f64::NAN)];
    cases.push(w);
    let mut w = good.clone();
    w.points = vec![(Tick(0), 1.5)];
    cases.push(w);
    let mut w = good.clone();
    w.points = vec![(Tick(2 * Q), 0.5)];
    cases.push(w); // outside its range
    let mut w = good.clone();
    w.start = Tick(Q);
    w.end = Tick(0);
    cases.push(w);
    let mut w = good.clone();
    w.end = Tick(1 << 41);
    cases.push(w);
    let mut w = good.clone();
    w.seq = SeqId(7);
    cases.push(w);
    let mut w = good.clone();
    w.target = AutomationTarget::StripMute(StripId(99));
    cases.push(w);
    let mut w = good.clone();
    w.points = (0..=core_model::command::MAX_WRITE_POINTS as i64)
        .map(|t| (Tick(t.min(Q)), 0.5))
        .collect();
    cases.push(w);
    let mut w = AutomationWrite {
        lane_name: "  ".into(),
        ..good.clone()
    };
    w.target = AutomationTarget::StripSolo(d.mixer().master());
    cases.push(w); // a new lane needs a name
    for bad in cases {
        let r = apply(&d, &Command::Record(pass(vec![good.clone(), bad.clone()])));
        assert!(r.is_err(), "{bad:?}");
    }
}

#[test]
fn a_pass_with_lanes_keeps_its_record_label() {
    let d = doc();
    let mut p = pass(vec![write(&d, 0, Q, &[(0, 0.5)])]);
    p.lanes.push(core_model::command::LaneRecord {
        seq: SeqId(0),
        lane: LaneId(0),
        clear: Vec::new(),
        active: core_model::command::RecordedContent {
            events: vec![EventSpec::new(
                Tick(Q),
                EventBody::note(62, 30_000, Tick(Q)),
            )],
            regions: Vec::new(),
        },
        takes: Vec::new(),
    });
    assert_eq!(Command::Record(p.clone()).name(), "Record");
    p.lanes.clear();
    assert_eq!(Command::Record(p).name(), "Write Automation");
}
