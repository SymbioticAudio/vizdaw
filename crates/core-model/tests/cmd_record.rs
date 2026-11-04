//! Recording passes and history groups (plan 5c Task 3; sub-project 5
//! spec §4.7, §6.4, §6.5): `Command::Record` applies a whole pass or
//! nothing, replace clears only MIDI in the recorded range, takes are
//! added with the last made active, and a group turns everything made
//! while recording into one "Record" step.

use core_model::ModelError;
use core_model::command::{
    Command, EventCmd, LaneCmd, LaneRecord, RecordPass, RecordedContent, RecordedMedia,
    RecordedRegion, SequenceCmd, apply,
};
use core_model::document::{Document, MediaOrigin};
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{LaneId, SeqId};
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;

const Q: i64 = PPQ;

/// Sequence 0 with lanes 0 and 1; lane 0 holds notes at beats 0, 2 and 4
/// (pitches 60, 62, 64) and a CC at beat 2.
fn doc() -> Document {
    let mut d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    d = apply(
        &d,
        &Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        }),
    )
    .unwrap()
    .doc;
    apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![
                EventSpec::new(Tick(0), EventBody::note(60, 30_000, Tick(Q))),
                EventSpec::new(Tick(2 * Q), EventBody::note(62, 30_000, Tick(Q))),
                EventSpec::new(Tick(2 * Q), EventBody::Cc { index: 1, value: 5 }),
                EventSpec::new(Tick(4 * Q), EventBody::note(64, 30_000, Tick(Q))),
            ],
        }),
    )
    .unwrap()
    .doc
}

fn media(frames: u64) -> RecordedMedia {
    RecordedMedia {
        path: "Media/Recorded/B 2026-10-20 120000.caf".into(),
        sample_rate: 48_000,
        channels: 1,
        length_frames: frames,
        content_hash: "ab".repeat(32),
        name: "B 2026-10-20 120000".into(),
        pass: 1,
    }
}

fn lane_record(lane: u64) -> LaneRecord {
    LaneRecord {
        seq: SeqId(0),
        lane: LaneId(lane),
        clear: Vec::new(),
        active: RecordedContent::default(),
        takes: Vec::new(),
    }
}

fn pitches(d: &Document, lane: u64) -> Vec<(i64, u8)> {
    d.lane(SeqId(0), LaneId(lane))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Note { pitch, .. } => Some((e.tick.0 / Q, pitch)),
            _ => None,
        })
        .collect()
}

fn note(beat: i64, pitch: u8) -> EventSpec {
    EventSpec::new(Tick(beat * Q), EventBody::note(pitch, 40_000, Tick(Q / 2)))
}

#[test]
fn replace_clears_only_midi_starting_in_the_range() {
    let mut l = lane_record(0);
    l.clear = vec![(Tick(Q), Tick(3 * Q))];
    l.active.events = vec![note(2, 70)];
    let pass = RecordPass {
        automation: Vec::new(),
        lanes: vec![l],
        ..RecordPass::default()
    };
    let d = apply(&doc(), &Command::Record(pass)).unwrap().doc;
    // 62 at beat 2 went, the CC at beat 2 went, 60 and 64 stay.
    assert_eq!(pitches(&d, 0), vec![(0, 60), (2, 70), (4, 64)]);
    let take = d.lane(SeqId(0), LaneId(0)).unwrap().active().unwrap();
    assert!(
        !take
            .events
            .values()
            .any(|e| matches!(e.body, EventBody::Cc { .. }))
    );
}

#[test]
fn overdub_merges_with_what_is_there() {
    let mut l = lane_record(0);
    l.active.events = vec![note(2, 70), note(3, 71)];
    let pass = RecordPass {
        automation: Vec::new(),
        lanes: vec![l],
        ..RecordPass::default()
    };
    let d = apply(&doc(), &Command::Record(pass)).unwrap().doc;
    assert_eq!(
        pitches(&d, 0),
        vec![(0, 60), (2, 62), (2, 70), (3, 71), (4, 64)]
    );
}

#[test]
fn a_pass_adds_its_media_regions_and_takes_and_selects_the_last() {
    let mut l = lane_record(1);
    let region = |offset: u64| RecordedRegion {
        media: 0,
        tick: Tick(0),
        source_offset: offset,
        length: 48_000,
    };
    l.takes = vec![
        (
            "Take 2".into(),
            RecordedContent {
                events: vec![],
                regions: vec![region(1000)],
            },
        ),
        (
            "Take 3".into(),
            RecordedContent {
                events: vec![],
                regions: vec![region(49_000)],
            },
        ),
    ];
    let pass = RecordPass {
        automation: Vec::new(),
        media: vec![media(100_000)],
        lanes: vec![l],
        ..RecordPass::default()
    };
    let a = apply(&doc(), &Command::Record(pass.clone())).unwrap();
    let d = a.doc;
    let item = d.media().values().next().unwrap();
    assert_eq!(item.origin, MediaOrigin::Recorded { pass: 1 });
    assert_eq!(item.content_hash.as_deref(), Some("ab".repeat(32).as_str()));
    let lane = d.lane(SeqId(0), LaneId(1)).unwrap();
    let names: Vec<&str> = lane.takes.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["Take 1", "Take 2", "Take 3"]);
    assert_eq!(lane.active().unwrap().name, "Take 3");
    let offsets: Vec<u64> = lane
        .takes
        .iter()
        .flat_map(|t| t.events.values())
        .filter_map(|e| match e.body {
            EventBody::AudioRegion {
                media,
                source_offset,
                ..
            } if media == item.id => Some(source_offset),
            _ => None,
        })
        .collect();
    assert_eq!(offsets, vec![1000, 49_000]);
    // The lane had no output: the region gave it the first one ("Audio 1").
    assert!(lane.audio_out.is_some());
    assert_eq!(Command::Record(pass).name(), "Record");
    let gaps = RecordPass {
        automation: Vec::new(),
        with_gaps: true,
        ..RecordPass::default()
    };
    assert_eq!(Command::Record(gaps).name(), "Record (with gaps)");
}

#[test]
fn a_bad_part_refuses_the_whole_pass() {
    let d = doc();
    let mut ok = lane_record(0);
    ok.active.events = vec![note(1, 72)];
    let bad_region = RecordedRegion {
        media: 0,
        tick: Tick(0),
        source_offset: 90_000,
        length: 20_000,
    };
    let mut bad = lane_record(1);
    bad.active.regions = vec![bad_region];
    for pass in [
        // A region past its file's end.
        RecordPass {
            automation: Vec::new(),
            media: vec![media(100_000)],
            lanes: vec![ok.clone(), bad.clone()],
            ..RecordPass::default()
        },
        // A region naming a file the pass does not have.
        RecordPass {
            automation: Vec::new(),
            media: vec![],
            lanes: vec![ok.clone(), bad.clone()],
            ..RecordPass::default()
        },
        // An empty file.
        RecordPass {
            automation: Vec::new(),
            media: vec![media(0)],
            lanes: vec![ok.clone()],
            ..RecordPass::default()
        },
        // A path outside the package.
        RecordPass {
            automation: Vec::new(),
            media: vec![RecordedMedia {
                path: "../x.caf".into(),
                ..media(10)
            }],
            lanes: vec![ok.clone()],
            ..RecordPass::default()
        },
        // An empty clear range, and an unknown lane.
        RecordPass {
            automation: Vec::new(),
            lanes: vec![LaneRecord {
                clear: vec![(Tick(Q), Tick(Q))],
                ..ok.clone()
            }],
            ..RecordPass::default()
        },
        RecordPass {
            automation: Vec::new(),
            lanes: vec![ok.clone(), lane_record(9)],
            ..RecordPass::default()
        },
    ] {
        let mut h = History::new(d.clone());
        assert!(
            h.execute(&Command::Record(pass.clone()), &ViewFilter::default())
                .is_err(),
            "{pass:?}"
        );
        assert_eq!(h.doc(), &d, "nothing applied");
        assert_eq!(h.undo_depth(), 0);
    }
}

#[test]
fn a_group_makes_one_record_step_of_everything_inside_it() {
    let f = ViewFilter::default();
    let d = doc();
    let mut h = History::new(d.clone());
    h.execute(
        &Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(1),
            name: "Before".into(),
        }),
        &f,
    )
    .unwrap();
    h.undo().unwrap();
    // A step undone before recording is not redone into the pass.
    h.begin_group().unwrap();
    assert!(h.in_group());
    assert!(h.redo().is_err());
    assert!(h.peek_redo().is_none());
    assert!(h.begin_group().is_err(), "one group at a time");
    let rename = |name: &str| {
        Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(0),
            name: name.into(),
        })
    };
    h.execute(&rename("X"), &f).unwrap();
    h.execute(&rename("Y"), &f).unwrap();
    // Steps inside stay undoable and redoable one by one.
    h.undo().unwrap();
    assert_eq!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().name, "X");
    h.redo().unwrap();
    // A drag (transaction) inside the group is fine; the group cannot end
    // while it is open.
    h.begin("Drag").unwrap();
    h.execute(&rename("Z"), &f).unwrap();
    assert!(h.end_group("Record").is_err());
    h.commit().unwrap();
    // Undo never reaches below the group's start.
    let mut l = lane_record(0);
    l.active.events = vec![note(6, 80)];
    h.execute(
        &Command::Record(RecordPass {
            automation: Vec::new(),
            lanes: vec![l],
            ..RecordPass::default()
        }),
        &f,
    )
    .unwrap();
    h.end_group("Record").unwrap();
    assert!(!h.in_group());
    assert_eq!(h.undo_label(), Some("Record"));
    assert_eq!(h.undo_depth(), 1);
    assert_eq!(
        pitches(h.doc(), 0),
        vec![(0, 60), (2, 62), (4, 64), (6, 80)]
    );
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
    h.redo().unwrap();
    assert_eq!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().name, "Z");
    assert!(
        h.redo().is_err(),
        "the rename undone before the pass is gone"
    );
}

#[test]
fn a_group_that_made_nothing_keeps_the_redo_stack() {
    let f = ViewFilter::default();
    let mut h = History::new(doc());
    h.execute(
        &Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "X".into(),
        }),
        &f,
    )
    .unwrap();
    h.undo().unwrap();
    h.begin_group().unwrap();
    assert!(h.undo().is_err(), "nothing of its own to undo");
    h.end_group("Record").unwrap();
    assert_eq!(h.undo_depth(), 0);
    h.redo().unwrap();
    assert_eq!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().name, "X");
    // A group cannot open inside a transaction, nor end without opening.
    h.begin("Drag").unwrap();
    assert!(h.begin_group().is_err());
    h.abort().unwrap();
    assert!(matches!(
        h.end_group("Record"),
        Err(ModelError::Invalid { .. })
    ));
}

#[test]
fn an_aborted_drag_in_a_group_keeps_the_redo_stack_before_it() {
    let f = ViewFilter::default();
    let rename = |name: &str| {
        Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(0),
            name: name.into(),
        })
    };
    let mut h = History::new(doc());
    h.execute(&rename("Before"), &f).unwrap();
    h.undo().unwrap();
    h.begin_group().unwrap();
    h.begin("Drag").unwrap();
    h.execute(&rename("Drag"), &f).unwrap();
    h.abort().unwrap();
    assert!(h.peek_redo().is_none(), "not reachable inside the group");
    assert!(h.redo().is_err());
    h.end_group("Record").unwrap();
    assert_eq!(h.undo_depth(), 0, "nothing folded");
    assert_eq!(h.redo_label(), Some("Rename Lane"));
    h.redo().unwrap();
    assert_eq!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().name, "Before");
}

#[test]
fn a_pass_with_too_many_takes_is_refused_whole() {
    use core_model::command::record::MAX_PASS_TAKES;
    let d = doc();
    let mut l = lane_record(0);
    l.takes = (0..=MAX_PASS_TAKES)
        .map(|i| (format!("Take {}", i + 2), RecordedContent::default()))
        .collect();
    let mut h = History::new(d.clone());
    let pass = RecordPass {
        automation: Vec::new(),
        lanes: vec![l],
        ..RecordPass::default()
    };
    assert!(
        h.execute(&Command::Record(pass), &ViewFilter::default())
            .is_err()
    );
    assert_eq!(h.doc(), &d);
    assert_eq!(h.undo_depth(), 0);
}

#[test]
fn media_paths_are_write_once() {
    let f = ViewFilter::default();
    let with = |paths: &[&str]| RecordPass {
        automation: Vec::new(),
        media: paths
            .iter()
            .map(|p| RecordedMedia {
                path: (*p).into(),
                ..media(10)
            })
            .collect(),
        ..RecordPass::default()
    };
    let mut h = History::new(doc());
    // Repeated inside one pass.
    assert!(
        h.execute(
            &Command::Record(with(&["Media/Recorded/a.caf", "Media/Recorded/a.caf"])),
            &f
        )
        .is_err()
    );
    h.execute(&Command::Record(with(&["Media/Recorded/a.caf"])), &f)
        .unwrap();
    let before = h.doc().clone();
    // Already in the pool.
    assert!(
        h.execute(
            &Command::Record(with(&["Media/Recorded/b.caf", "Media/Recorded/a.caf"])),
            &f
        )
        .is_err()
    );
    assert_eq!(h.doc(), &before);
    assert_eq!(h.undo_depth(), 1);
}

#[test]
fn media_in_an_open_group_stays_referenced() {
    let f = ViewFilter::default();
    let path = "Media/Recorded/B 2026-10-20 120000.caf";
    let pass = RecordPass {
        automation: Vec::new(),
        media: vec![media(10)],
        ..RecordPass::default()
    };
    // Made and undone inside the group.
    let mut h = History::new(doc());
    h.begin_group().unwrap();
    h.execute(&Command::Record(pass.clone()), &f).unwrap();
    h.undo().unwrap();
    assert!(h.referenced_media_anywhere().contains_key(path));
    h.end_group("Record").unwrap();
    // Undone before the group opened.
    let mut h = History::new(doc());
    h.execute(&Command::Record(pass), &f).unwrap();
    h.undo().unwrap();
    h.begin_group().unwrap();
    assert!(h.referenced_media_anywhere().contains_key(path));
}

#[derive(Debug, Clone)]
enum Part {
    Clear(i64, i64),
    Note(i64, u8),
    Region(u64, u64, i64),
    Take(Vec<(i64, u8)>, Vec<(u64, u64)>),
}

fn part() -> impl Strategy<Value = Part> {
    prop_oneof![
        (0i64..8, 1i64..8).prop_map(|(a, n)| Part::Clear(a, a + n)),
        (0i64..8, 0u8..128).prop_map(|(b, p)| Part::Note(b, p)),
        (0u64..80_000, 1u64..20_000, 0i64..8).prop_map(|(o, n, b)| Part::Region(o, n, b)),
        (
            proptest::collection::vec((0i64..8, 0u8..128), 0..4),
            proptest::collection::vec((0u64..90_000, 1u64..10_000), 0..3)
        )
            .prop_map(|(n, r)| Part::Take(n, r)),
    ]
}

fn pass_of(parts: &[(u64, Part)]) -> RecordPass {
    let mut lanes = vec![lane_record(0), lane_record(1)];
    for (lane, p) in parts {
        let Some(l) = lanes.get_mut(*lane as usize % 2) else {
            continue;
        };
        match p {
            Part::Clear(a, b) => l.clear.push((Tick(a * Q), Tick(b * Q))),
            Part::Note(b, p) => l.active.events.push(note(*b, *p)),
            Part::Region(o, n, b) => l.active.regions.push(RecordedRegion {
                media: 0,
                tick: Tick(b * Q),
                source_offset: *o,
                length: *n,
            }),
            Part::Take(notes, regions) => l.takes.push((
                format!("Take {}", l.takes.len() + 2),
                RecordedContent {
                    events: notes.iter().map(|(b, p)| note(*b, *p)).collect(),
                    regions: regions
                        .iter()
                        .map(|(o, n)| RecordedRegion {
                            media: 0,
                            tick: Tick(0),
                            source_offset: *o,
                            length: *n,
                        })
                        .collect(),
                },
            )),
        }
    }
    RecordPass {
        automation: Vec::new(),
        with_gaps: false,
        media: vec![media(100_000)],
        lanes,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Spec §12.3: applying a pass and undoing it restores the document
    /// exactly; redo brings the same pass back.
    #[test]
    fn record_then_undo_restores_the_document(parts in proptest::collection::vec((0u64..2, part()), 0..12)) {
        let d = doc();
        let mut h = History::new(d.clone());
        h.begin_group().unwrap();
        h.execute(&Command::Record(pass_of(&parts)), &ViewFilter::default()).unwrap();
        h.end_group("Record").unwrap();
        let after = h.doc().clone();
        if after != d {
            prop_assert_eq!(h.undo_depth(), 1);
            h.undo().unwrap();
        }
        prop_assert_eq!(h.doc(), &d);
        if after != d {
            h.redo().unwrap();
        }
        prop_assert_eq!(h.doc(), &after);
        prop_assert!(core_model::rules::validate(&after).is_empty());
    }
}
