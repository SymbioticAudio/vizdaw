//! Automation a pass wrote (plan 5e-1 Task 3; sub-project 5 spec §4.7,
//! §7.3, §7.4): each write goes into the first lane of its sequence that
//! plays and already holds its target, else into a new lane at the end;
//! it replaces the target's points in its range in every playing lane
//! that holds the target (pre-flight I5; final review I1: muted and
//! solo-silenced lanes are left), and nothing else; a bad write refuses
//! the whole pass;
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
    let notes = |d: &Document| {
        d.lane(SeqId(0), LaneId(0))
            .unwrap()
            .active()
            .unwrap()
            .clone()
    };
    assert_eq!(notes(&out), notes(&d), "A is untouched");
    // A second write at C's old tick still goes to B (C is not first).
    let w = write(&d, 2 * Q, 2 * Q, &[(2 * Q, 0.1)]);
    let out = apply(&out, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert!(
        points(&out, 2).is_empty(),
        "C's point inside the range is replaced too: {:?}",
        points(&out, 2)
    );
    assert_eq!(
        points(&out, 1),
        vec![
            (0, 0.2),
            (Q - 1, 0.3),
            (Q, 0.9),
            (2 * Q, 0.1),
            (4 * Q, 0.9),
            (4 * Q + 1, 0.4),
            (8 * Q, 0.8)
        ]
    );
}

/// Amended Ruling 4: a write removes the target's points in its range
/// from every playing lane holding the target, and leaves their points outside
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
    // Each bad write with the words its refusal names (Task 3 review
    // minors 4 and 6: each case is refused for its own reason).
    let mut cases: Vec<(AutomationWrite, &str)> = Vec::new();
    let mut w = good.clone();
    w.points = vec![(Tick(0), f64::NAN)];
    cases.push((w, "automation.value"));
    let mut w = good.clone();
    w.points = vec![(Tick(0), 1.5)];
    cases.push((w, "automation.value"));
    let mut w = good.clone();
    w.points = vec![(Tick(2 * Q), 0.5)];
    cases.push((w, "automation.tick")); // outside its range
    let mut w = good.clone();
    w.start = Tick(Q);
    w.end = Tick(0);
    w.points.clear(); // only the reversed range is wrong
    cases.push((w, "automation.end"));
    let mut w = good.clone();
    w.end = Tick(1 << 41);
    cases.push((w, "automation.end"));
    let mut w = good.clone();
    w.seq = SeqId(7);
    cases.push((w, "unknown id"));
    let mut w = good.clone();
    w.target = AutomationTarget::StripMute(StripId(99));
    cases.push((w, "does not exist"));
    let mut w = good.clone();
    w.points = (0..=core_model::command::MAX_WRITE_POINTS as i64)
        .map(|t| (Tick(t.min(Q)), 0.5))
        .collect();
    cases.push((w, "automation.points"));
    let mut w = AutomationWrite {
        lane_name: "  ".into(),
        ..good.clone()
    };
    w.target = AutomationTarget::StripSolo(d.mixer().master());
    cases.push((w, "lane name")); // a new lane needs a name
    for (bad, why) in cases {
        let r = apply(&d, &Command::Record(pass(vec![good.clone(), bad.clone()])));
        let e = r
            .err()
            .unwrap_or_else(|| panic!("refused: {bad:?}"))
            .to_string();
        assert!(e.contains(why), "{why}: {e} for {bad:?}");
    }
}

/// Task 3 review minor 1: the range is inclusive at both ends; points
/// just outside it stay.
#[test]
fn a_write_replaces_its_range_inclusive_at_both_ends() {
    let f = fader(&doc());
    let d = insert(
        &doc(),
        1,
        vec![
            point(Q - 1, f, 0.1),
            point(Q, f, 0.2),
            point(3 * Q, f, 0.3),
            point(3 * Q + 1, f, 0.4),
        ],
    );
    let w = write(&d, Q, 3 * Q, &[(Q, 0.6), (3 * Q, 0.7)]);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert_eq!(
        points(&out, 1),
        vec![
            (0, 0.2),
            (Q - 1, 0.1),
            (Q, 0.6),
            (3 * Q, 0.7),
            (3 * Q + 1, 0.4),
            (8 * Q, 0.8)
        ]
    );
}

/// Task 3 review minor 2: a write with no points for a target no lane
/// plays makes no empty lane; its range is still cleared.
#[test]
fn an_empty_write_makes_no_lane() {
    let d = doc();
    let mut w = write(&d, 0, Q, &[]);
    w.target = AutomationTarget::StripPan(d.mixer().master());
    w.lane_name = "Master Pan".into();
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert_eq!(out.sequence(SeqId(0)).unwrap().lanes.len(), 3);
    let w = write(&d, Q - 1, 4 * Q + 1, &[]);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert!(points(&out, 2).is_empty(), "the range is cleared");
}

/// Task 3 review minor 3: of two points of one write at one tick, the
/// later one is kept, so playback does not depend on event IDs.
#[test]
fn a_write_keeps_the_last_of_equal_ticks() {
    let d = doc();
    let w = write(&d, Q, 2 * Q, &[(Q, 0.3), (Q, 0.6), (2 * Q, 0.1)]);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert_eq!(
        points(&out, 1),
        vec![(0, 0.2), (Q, 0.6), (2 * Q, 0.1), (8 * Q, 0.8)]
    );
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

fn set_flags(d: &Document, lane: u64, mute: Option<bool>, solo: Option<bool>) -> Document {
    apply(
        d,
        &Command::Lane(LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(lane),
            mute,
            solo,
            arm: None,
        }),
    )
    .unwrap()
    .doc
}

fn insert(d: &Document, lane: u64, events: Vec<EventSpec>) -> Document {
    apply(
        d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(lane),
            events,
        }),
    )
    .unwrap()
    .doc
}

/// The master fader's automation as it plays (`query::flatten`, which
/// honours lane mute, solo and the active take).
fn heard(d: &Document) -> Vec<(i64, f64)> {
    let mut v: Vec<(i64, f64)> =
        core_model::query::flatten(d, SeqId(0), None, &ViewFilter::default())
            .unwrap()
            .filter_map(|e| match e.body {
                EventBody::Automation { value, .. } => Some((e.tick.0, value)),
                _ => None,
            })
            .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

const RIDE: [(i64, f64); 4] = [(Q - 1, 0.5), (Q, 0.9), (4 * Q, 0.9), (4 * Q + 1, 0.5)];

/// Final review I1: a write goes into the first lane that PLAYS its
/// target, so what is written is what plays; a muted lane keeps its own
/// points, inside the range too.
#[test]
fn a_write_skips_a_muted_first_lane() {
    let d = doc();
    let f = fader(&d);
    let d = insert(&d, 1, vec![point(3 * Q, f, 0.1)]);
    let d = set_flags(&d, 1, Some(true), None);
    assert_eq!(heard(&d), vec![(2 * Q, 0.5)]);
    let w = write(&d, Q - 1, 4 * Q + 1, &RIDE);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert_eq!(heard(&out), RIDE.to_vec(), "the ride is what plays");
    assert_eq!(
        points(&out, 2),
        RIDE.to_vec(),
        "C (the playing lane) has it"
    );
    assert_eq!(
        points(&out, 1),
        vec![(0, 0.2), (3 * Q, 0.1), (8 * Q, 0.8)],
        "the muted lane B is left alone"
    );
    assert_eq!(out.sequence(SeqId(0)).unwrap().lanes.len(), 3);
}

/// A lane silenced by another lane's solo does not play: the write goes
/// into the soloed lane holding the target and leaves the silenced one.
#[test]
fn a_write_skips_a_lane_another_lanes_solo_silences() {
    let d = set_flags(&doc(), 2, None, Some(true));
    let w = write(&d, Q - 1, 4 * Q + 1, &RIDE);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert_eq!(heard(&out), RIDE.to_vec());
    assert_eq!(points(&out, 2), RIDE.to_vec());
    assert_eq!(points(&out, 1), vec![(0, 0.2), (8 * Q, 0.8)]);
}

/// When no lane plays the target (here the note lane is soloed, which
/// silences B and C), the write goes into B, the first lane holding the
/// target that only the solo silences (Task 12 review I2): silent until
/// the solo clears, and no new lane. With every lane holding the target
/// muted and no solo, a new, unsoloed lane is made.
#[test]
fn a_write_with_no_playing_lane_goes_into_the_first_solo_silenced_lane() {
    let d = set_flags(&doc(), 0, None, Some(true));
    assert!(heard(&d).is_empty());
    let w = write(&d, Q - 1, 4 * Q + 1, &RIDE);
    let mut h = History::new(d.clone());
    h.execute(&Command::Record(pass(vec![w])), &ViewFilter::default())
        .unwrap();
    let out = h.doc().clone();
    assert_eq!(
        out.sequence(SeqId(0)).unwrap().lanes.len(),
        3,
        "no new lane"
    );
    assert!(heard(&out).is_empty(), "silent while A is soloed");
    assert_eq!(
        points(&out, 1),
        vec![
            (0, 0.2),
            (Q - 1, 0.5),
            (Q, 0.9),
            (4 * Q, 0.9),
            (4 * Q + 1, 0.5),
            (8 * Q, 0.8)
        ]
    );
    assert_eq!(points(&out, 2), vec![(2 * Q, 0.5)], "C keeps its point");
    // Unsoloing A: the ride plays.
    let open = set_flags(&out, 0, None, Some(false));
    let all = heard(&open);
    for p in RIDE {
        assert!(
            all.contains(&p),
            "{p:?} plays once the solo clears: {all:?}"
        );
    }
    h.undo().unwrap();
    assert_eq!(h.doc(), &d, "one undo takes it back");
    // Every lane holding the target muted, no solo: a new, unsoloed lane.
    let d = set_flags(&set_flags(&doc(), 1, Some(true), None), 2, Some(true), None);
    let w = write(&d, Q - 1, 4 * Q + 1, &RIDE);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    let s = out.sequence(SeqId(0)).unwrap();
    assert_eq!(s.lanes.len(), 4);
    assert!(!s.lanes.last().unwrap().solo);
    assert_eq!(heard(&out), RIDE.to_vec());
    assert_eq!(points(&out, 2), vec![(2 * Q, 0.5)]);
}

/// The reviewer's probe (final review I1): B (first, muted) and C
/// (playing, a point at 2Q); before the fix the ride went into B and C's
/// point went, so nothing was heard.
#[test]
fn the_final_review_probe_hears_the_ride() {
    let d = set_flags(&doc(), 1, Some(true), None);
    assert_eq!(heard(&d), vec![(2 * Q, 0.5)]);
    let w = write(&d, Q - 1, 4 * Q + 1, &RIDE);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert_eq!(heard(&out), RIDE.to_vec());
}

/// Re-review O1 (controller ruling): a lane whose events are transformed
/// (a play shift, its own length, its own loop) does not play them at
/// their ticks, so a write never goes into it: here B is the only lane
/// that plays the fader (C muted). B keeps its points and the ride lands
/// in a new, untransformed lane, whose points play at their own ticks.
#[test]
fn a_write_skips_a_shifted_or_looped_lane() {
    let base = set_flags(&doc(), 2, Some(true), None);
    let shifted = apply(
        &base,
        &Command::Lane(LaneCmd::SetPlayShift {
            seq: SeqId(0),
            lane: LaneId(1),
            shift: Tick(100),
        }),
    )
    .unwrap()
    .doc;
    let looped = apply(
        &base,
        &Command::Lane(LaneCmd::SetLength {
            seq: SeqId(0),
            lane: LaneId(1),
            length: Some(Tick(2 * Q)),
            looped: true,
            locked: false,
        }),
    )
    .unwrap()
    .doc;
    let cut = apply(
        &base,
        &Command::Lane(LaneCmd::SetLength {
            seq: SeqId(0),
            lane: LaneId(1),
            length: Some(Tick(16 * Q)),
            looped: false,
            locked: false,
        }),
    )
    .unwrap()
    .doc;
    for (name, d) in [
        ("shifted", shifted),
        ("looped", looped),
        ("own length", cut),
    ] {
        let w = write(&d, Q - 1, 4 * Q + 1, &RIDE);
        let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
        assert_eq!(
            points(&out, 1),
            vec![(0, 0.2), (8 * Q, 0.8)],
            "{name}: B keeps its points"
        );
        let s = out.sequence(SeqId(0)).unwrap();
        assert_eq!(s.lanes.len(), 4, "{name}: a new lane");
        let new = s.lanes.last().unwrap();
        assert_eq!(new.name, "Master Fader", "{name}");
        assert_eq!(
            points(&out, 3),
            RIDE.to_vec(),
            "{name}: the new lane has the ride"
        );
        let heard = heard(&out);
        for p in RIDE {
            assert!(
                heard.contains(&p),
                "{name}: {p:?} plays at its tick: {heard:?}"
            );
        }
    }
    // An untransformed lane after a transformed one still takes the write.
    let d = apply(
        &doc(),
        &Command::Lane(LaneCmd::SetPlayShift {
            seq: SeqId(0),
            lane: LaneId(1),
            shift: Tick(100),
        }),
    )
    .unwrap()
    .doc;
    let w = write(&d, Q - 1, 4 * Q + 1, &RIDE);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert_eq!(points(&out, 2), RIDE.to_vec(), "C (untransformed) has it");
    assert_eq!(points(&out, 1), vec![(0, 0.2), (8 * Q, 0.8)]);
    assert_eq!(out.sequence(SeqId(0)).unwrap().lanes.len(), 3);
}

/// Task 12 review I2 (controller ruling): when no playing lane holds the
/// target, a write goes into the first unmuted, untransformed lane holding
/// it that only a solo silences, before a new lane is made. So under a
/// solo every write for the target goes into one lane. With B and C
/// muted (so no lane qualifies at first), two writes landed one after the
/// other (two wraps) and two writes in one pass each leave one new lane,
/// not soloed (owner ruling N1), which holds both rides.
#[test]
fn writes_under_a_solo_go_into_one_lane() {
    let base = set_flags(
        &set_flags(&set_flags(&doc(), 0, None, Some(true)), 1, Some(true), None),
        2,
        Some(true),
        None,
    );
    let w1 = write(
        &base,
        Q - 1,
        2 * Q + 1,
        &[(Q - 1, 0.5), (Q, 0.9), (2 * Q, 0.9), (2 * Q + 1, 0.5)],
    );
    let w2 = write(
        &base,
        5 * Q - 1,
        6 * Q + 1,
        &[
            (5 * Q - 1, 0.5),
            (5 * Q, 0.3),
            (6 * Q, 0.3),
            (6 * Q + 1, 0.5),
        ],
    );
    // (a) two landings.
    let one = apply(&base, &Command::Record(pass(vec![w1.clone()])))
        .unwrap()
        .doc;
    let two = apply(&one, &Command::Record(pass(vec![w2.clone()])))
        .unwrap()
        .doc;
    // (b) one pass.
    let both = apply(&base, &Command::Record(pass(vec![w1, w2])))
        .unwrap()
        .doc;
    for (name, d) in [("two landings", two), ("one pass", both)] {
        let s = d.sequence(SeqId(0)).unwrap();
        assert_eq!(s.lanes.len(), 4, "{name}: one new lane");
        assert!(!s.lanes.last().unwrap().solo, "{name}: not soloed");
        let p = points(&d, 3);
        assert!(
            p.contains(&(Q, 0.9)) && p.contains(&(5 * Q, 0.3)),
            "{name}: {p:?}"
        );
        assert_eq!(
            points(&d, 1),
            vec![(0, 0.2), (8 * Q, 0.8)],
            "{name}: B kept"
        );
        assert_eq!(points(&d, 2), vec![(2 * Q, 0.5)], "{name}: C kept");
    }
}

/// Task 12 review I2: the solo-silenced lane that takes the write must be
/// unmuted: a muted lane holding the target keeps its points and is not
/// chosen; the first unmuted one is (and loses its points in the range).
#[test]
fn a_write_under_a_solo_skips_a_muted_lane_for_the_first_silenced_one() {
    // A soloed; B muted; C (2Q) silenced only by A's solo.
    let d = set_flags(&set_flags(&doc(), 0, None, Some(true)), 1, Some(true), None);
    let w = write(&d, Q - 1, 4 * Q + 1, &RIDE);
    let out = apply(&d, &Command::Record(pass(vec![w]))).unwrap().doc;
    assert_eq!(
        out.sequence(SeqId(0)).unwrap().lanes.len(),
        3,
        "no new lane"
    );
    assert_eq!(
        points(&out, 1),
        vec![(0, 0.2), (8 * Q, 0.8)],
        "the muted lane B is left"
    );
    assert_eq!(points(&out, 2), RIDE.to_vec(), "C takes the ride");
}
