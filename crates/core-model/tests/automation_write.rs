//! Automation writes (plan 8e Ruling 2): `Command::WriteAutomation`
//! runs a Write pass's own checks and placement, so the range is
//! replaced, a missing automation track is made once, and the write is
//! one undo step named "Write Automation".

use core_model::automation::AutomationTarget;
use core_model::command::{
    AutomationWrite, Command, EventCmd, LaneCmd, MAX_WRITE_POINTS, MixerCmd, SequenceCmd,
};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{LaneId, SeqId, StripId};
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};

/// One 4/4 bar.
const BAR: i64 = 4 * PPQ;

fn exec(h: &mut History, c: Command) {
    h.execute(&c, &ViewFilter::default()).unwrap();
}

/// Sequence 0 with lane 0 "Kick"; buses Kick (the composer's) and
/// "Drums (Co-Producer)" (marked).
fn history() -> History {
    let mut h = History::new(Document::new());
    exec(
        &mut h,
        Command::Sequence(SequenceCmd::New {
            name: "Song".into(),
        }),
    );
    exec(
        &mut h,
        Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Kick".into(),
        }),
    );
    exec(
        &mut h,
        Command::Mixer(MixerCmd::AddBus {
            name: "Kick".into(),
            index: None,
        }),
    );
    h.begin_request("help").unwrap();
    h.set_acting(true).unwrap();
    exec(
        &mut h,
        Command::Mixer(MixerCmd::AddBus {
            name: "Drums".into(),
            index: None,
        }),
    );
    h.set_acting(false).unwrap();
    h.end_request().unwrap();
    h
}

fn strip(h: &History, name: &str) -> StripId {
    *h.doc()
        .mixer()
        .strips()
        .iter()
        .find(|(_, s)| s.name == name)
        .unwrap()
        .0
}

fn kick_fader(h: &History) -> AutomationTarget {
    AutomationTarget::StripFader(strip(h, "Kick"))
}

fn write(target: AutomationTarget, start: i64, end: i64, points: &[(i64, f64)]) -> Command {
    Command::WriteAutomation(AutomationWrite {
        seq: SeqId(0),
        target,
        start: Tick(start),
        end: Tick(end),
        points: points.iter().map(|(t, v)| (Tick(*t), *v)).collect(),
        lane_name: "Kick fader Automation".into(),
    })
}

/// Points at `ticks` on `target` in lane 0.
fn insert(h: &mut History, target: AutomationTarget, ticks: &[i64]) {
    exec(
        h,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: ticks
                .iter()
                .map(|t| EventSpec::new(Tick(*t), EventBody::Automation { target, value: 0.5 }))
                .collect(),
        }),
    );
}

/// `(tick, value)` of every point on `target` in sequence 0, by tick.
fn points(d: &Document, target: AutomationTarget) -> Vec<(i64, f64)> {
    let mut v: Vec<(i64, f64)> = d
        .sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .flat_map(|l| l.takes.iter())
        .flat_map(|t| t.events.values())
        .filter_map(|e| match e.body {
            EventBody::Automation { target: t, value } if t == target => Some((e.tick.0, value)),
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

fn lane_names(d: &Document) -> Vec<String> {
    d.sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .map(|l| l.name.clone())
        .collect()
}

#[test]
fn write_replaces_the_range() {
    let mut h = history();
    let f = kick_fader(&h);
    insert(&mut h, f, &[0, BAR, BAR + 2 * PPQ]);
    exec(
        &mut h,
        write(
            f,
            BAR,
            2 * BAR - 1,
            &[(BAR + PPQ, 0.25), (BAR + 3 * PPQ, 0.75)],
        ),
    );
    assert_eq!(
        points(h.doc(), f),
        vec![(0, 0.5), (BAR + PPQ, 0.25), (BAR + 3 * PPQ, 0.75)]
    );
    // Into the track that holds the target: no new one.
    assert_eq!(lane_names(h.doc()), vec!["Kick"]);
}

#[test]
fn an_empty_write_clears_the_range() {
    let mut h = history();
    let f = kick_fader(&h);
    insert(&mut h, f, &[0, BAR, BAR + 2 * PPQ, 2 * BAR]);
    exec(&mut h, write(f, BAR, 2 * BAR - 1, &[]));
    assert_eq!(points(h.doc(), f), vec![(0, 0.5), (2 * BAR, 0.5)]);
}

#[test]
fn write_makes_its_track_once() {
    let mut h = history();
    let f = kick_fader(&h);
    exec(&mut h, write(f, 0, BAR, &[(0, 0.5)]));
    assert_eq!(lane_names(h.doc()), vec!["Kick", "Kick fader Automation"]);
    exec(&mut h, write(f, BAR, 2 * BAR, &[(BAR, 0.25)]));
    assert_eq!(lane_names(h.doc()), vec!["Kick", "Kick fader Automation"]);
    assert_eq!(points(h.doc(), f), vec![(0, 0.5), (BAR, 0.25)]);
    // A write on the marked bus makes its own track the same way.
    let drums = AutomationTarget::StripFader(strip(&h, "Drums (Co-Producer)"));
    exec(&mut h, write(drums, 0, BAR, &[(0, 0.5)]));
    assert_eq!(lane_names(h.doc()).len(), 3);
}

#[test]
fn write_is_one_undo_step() {
    let mut h = history();
    let f = kick_fader(&h);
    insert(&mut h, f, &[0, BAR]);
    let before = h.doc().clone();
    let depth = h.undo_depth();
    exec(&mut h, write(f, 0, BAR, &[(PPQ, 1.0)]));
    assert_eq!(h.undo_depth(), depth + 1);
    assert_eq!(h.undo_label(), Some("Write Automation"));
    assert_eq!(write(f, 0, BAR, &[]).name(), "Write Automation");
    h.undo().unwrap();
    assert_eq!(h.doc(), &before);
    assert_eq!(points(h.doc(), f), vec![(0, 0.5), (BAR, 0.5)]);
}

#[test]
fn write_refuses_what_a_pass_refuses() {
    let mut h = history();
    let f = kick_fader(&h);
    insert(&mut h, f, &[0]);
    let before = h.doc().clone();
    let depth = h.undo_depth();
    let missing = AutomationTarget::StripFader(StripId(999));
    let too_many: Vec<(i64, f64)> = (0..=MAX_WRITE_POINTS as i64).map(|t| (t, 0.5)).collect();
    for (i, bad) in [
        write(missing, 0, BAR, &[(0, 0.5)]),
        write(f, 0, MAX_WRITE_POINTS as i64 + 1, &too_many),
    ]
    .iter()
    .enumerate()
    {
        assert!(h.execute(bad, &ViewFilter::default()).is_err(), "case {i}");
        assert_eq!(h.doc(), &before);
        assert_eq!(h.undo_depth(), depth);
    }
}
