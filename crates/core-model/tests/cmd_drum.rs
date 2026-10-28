use core_model::ModelError;
use core_model::command::drum::{DrumHit, MAX_PAINT_HITS};
use core_model::command::{Command, DrumCmd, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{
    AnyId, Counters, DestId, DrumRowId, EventId, LaneId, PluginInstanceId, SeqId,
};
use core_model::mixer::{PluginRef, PluginSlot};
use core_model::rules::validate;
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn drum(c: DrumCmd) -> Command {
    Command::Drum(c)
}

fn note(t: i64, pitch: u8) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::note(pitch, 100, Tick(PPQ / 4)))
}

/// dest0 and dest1; seq0 with lane0 (dest0, drum lane) and lane1 (dest1).
fn kit() -> Document {
    let dest = |i: u64| Destination {
        id: DestId(i),
        name: format!("D{i}"),
        kind: DestKind::PluginInstrument {
            slot: PluginSlot::new(PluginInstanceId(i), PluginRef::from_legacy("p")),
        },
        default_patch: None,
        return_latency_ms: 0.0,
    };
    let d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![dest(0), dest(1)],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        mixer: None,
        counters: Counters {
            dest: 2,
            plugin: 2,
            ..Counters::default()
        },
    })
    .unwrap();
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::New {
            name: "Beat".into(),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(1),
            dest: Some(DestId(1)),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![note(0, 36), note(PPQ, 38), note(PPQ * 2, 36)],
        }),
    )
    .unwrap();
    run(
        &d,
        drum(DrumCmd::MarkDrumLane {
            seq: SeqId(0),
            lane: LaneId(0),
            drum: true,
        }),
    )
    .unwrap()
}

fn rows(d: &Document, lane: u64) -> Vec<(u8, Option<DestId>, String)> {
    d.lane(SeqId(0), LaneId(lane))
        .unwrap()
        .drum
        .as_ref()
        .map(|l| {
            l.rows
                .iter()
                .map(|r| (r.note, r.dest, r.name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn marking_creates_rows_for_existing_notes() {
    assert_eq!(
        rows(&kit(), 0),
        vec![(36, None, "C2".into()), (38, None, "D2".into())]
    );
    assert!(validate(&kit()).is_empty());
}

#[test]
fn inserting_a_new_note_auto_creates_a_row() {
    let a = apply(
        &kit(),
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![note(0, 42), note(PPQ, 42)],
        }),
    )
    .unwrap();
    assert_eq!(rows(&a.doc, 0).len(), 3);
    assert_eq!(rows(&a.doc, 0)[2].0, 42);
    // Exactly one row created for two notes.
    assert_eq!(
        a.report
            .created
            .iter()
            .filter(|i| matches!(i, AnyId::DrumRow(_)))
            .count(),
        1
    );
}

#[test]
fn changing_a_note_from_another_editor_auto_creates_a_row() {
    let d = run(
        &kit(),
        Command::Event(EventCmd::SetPitch {
            events: vec![EventId(1)],
            pitch: 46,
        }),
    )
    .unwrap();
    assert!(rows(&d, 0).iter().any(|r| r.0 == 46));
    let d = run(
        &d,
        Command::Event(EventCmd::SetDest {
            events: vec![EventId(1)],
            dest: Some(DestId(1)),
        }),
    )
    .unwrap();
    assert!(
        rows(&d, 0)
            .iter()
            .any(|r| r.0 == 46 && r.1 == Some(DestId(1)))
    );
}

#[test]
fn add_rename_reorder_remove_rows() {
    let d = run(
        &kit(),
        drum(DrumCmd::AddRow {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Hat".into(),
            note: 42,
            dest: None,
        }),
    )
    .unwrap();
    assert!(matches!(
        run(
            &d,
            drum(DrumCmd::AddRow {
                seq: SeqId(0),
                lane: LaneId(0),
                name: "Dup".into(),
                note: 42,
                dest: None
            })
        ),
        Err(ModelError::Invalid { .. })
    ));
    let d = run(
        &d,
        drum(DrumCmd::RenameRow {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
            name: "Kick".into(),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        drum(DrumCmd::ReorderRow {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(2),
            to: 0,
        }),
    )
    .unwrap();
    assert_eq!(
        rows(&d, 0).iter().map(|r| r.2.as_str()).collect::<Vec<_>>(),
        vec!["Hat", "Kick", "D2"]
    );
    let a = apply(
        &d,
        &drum(DrumCmd::RemoveRow {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
        }),
    )
    .unwrap();
    assert_eq!(
        a.report.removed,
        vec![
            AnyId::Event(EventId(0)),
            AnyId::Event(EventId(2)),
            AnyId::DrumRow(DrumRowId(0))
        ]
    );
    assert!(
        run(
            &d,
            drum(DrumCmd::AddRow {
                seq: SeqId(0),
                lane: LaneId(1),
                name: "x".into(),
                note: 1,
                dest: None
            })
        )
        .is_err()
    );
}

#[test]
fn move_row_keeps_sound_by_pinning_destination() {
    let d = run(
        &kit(),
        drum(DrumCmd::MoveRowToLane {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
            to_lane: LaneId(1),
        }),
    )
    .unwrap();
    assert_eq!(rows(&d, 0), vec![(38, None, "D2".into())]);
    assert_eq!(rows(&d, 1), vec![(36, Some(DestId(0)), "C2".into())]);
    for id in [0, 2] {
        let (e, loc) = d.event(EventId(id)).unwrap();
        assert_eq!((loc.lane, e.dest), (LaneId(1), Some(DestId(0))));
    }
    assert!(validate(&d).is_empty());
}

#[test]
fn split_row_creates_lane_after_source() {
    let a = apply(
        &kit(),
        &drum(DrumCmd::SplitRowToLane {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(1),
        }),
    )
    .unwrap();
    let s = a.doc.sequence(SeqId(0)).unwrap();
    assert_eq!(
        s.lanes.iter().map(|l| l.id).collect::<Vec<_>>(),
        vec![LaneId(0), LaneId(2), LaneId(1)]
    );
    let new_lane = &s.lanes[1];
    assert_eq!(
        (new_lane.name.as_str(), new_lane.midi_dest),
        ("D2", Some(DestId(0)))
    );
    assert_eq!(rows(&a.doc, 2), vec![(38, None, "D2".into())]);
    assert_eq!(a.doc.event(EventId(1)).unwrap().1.lane, LaneId(2));
    assert!(validate(&a.doc).is_empty());
}

#[test]
fn move_row_into_a_normal_lane_keeps_its_notes_visible() {
    let d = run(
        &kit(),
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(1),
            events: vec![note(0, 50), note(PPQ, 51)],
        }),
    )
    .unwrap();
    let a = apply(
        &d,
        &drum(DrumCmd::MoveRowToLane {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
            to_lane: LaneId(1),
        }),
    )
    .unwrap();
    let d = a.doc;
    // Lane 1 is now a drum lane with rows for its own notes plus the moved row.
    assert_eq!(
        rows(&d, 1),
        vec![
            (50, None, "D3".into()),
            (51, None, "D#3".into()),
            (36, Some(DestId(0)), "C2".into())
        ]
    );
    let view = core_model::query::drum_view(&d, SeqId(0), LaneId(1)).unwrap();
    let shown: usize = view.iter().map(|(_, ids)| ids.len()).sum();
    assert_eq!(shown, 4);
    assert_eq!(
        a.report
            .created
            .iter()
            .filter(|i| matches!(i, AnyId::DrumRow(_)))
            .count(),
        2
    );
    assert!(validate(&d).is_empty());
}

#[test]
fn move_row_to_the_same_lane_still_validates() {
    for (lane, row) in [(0, 99), (7, 0)] {
        assert!(
            run(
                &kit(),
                drum(DrumCmd::MoveRowToLane {
                    seq: SeqId(0),
                    lane: LaneId(lane),
                    row: DrumRowId(row),
                    to_lane: LaneId(lane),
                }),
            )
            .is_err(),
            "lane {lane} row {row}"
        );
    }
    // A real row moved onto its own lane is a no-op.
    let d = run(
        &kit(),
        drum(DrumCmd::MoveRowToLane {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
            to_lane: LaneId(0),
        }),
    )
    .unwrap();
    assert_eq!(rows(&d, 0), rows(&kit(), 0));
}

#[test]
fn move_row_reports_moved_events() {
    let a = apply(
        &kit(),
        &drum(DrumCmd::MoveRowToLane {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
            to_lane: LaneId(1),
        }),
    )
    .unwrap();
    assert_eq!(
        a.report.moved,
        vec![AnyId::Event(EventId(0)), AnyId::Event(EventId(2))]
    );
}

/// The row's notes in lane 0's active take: `(tick, pitch, velocity, dest)`.
fn row_notes(d: &Document, pitch: u8) -> Vec<(i64, u8, u16, Option<DestId>)> {
    d.lane(SeqId(0), LaneId(0))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Note {
                pitch: p, velocity, ..
            } if p == pitch => Some((e.tick.0, p, velocity, e.dest)),
            _ => None,
        })
        .collect()
}

fn paint(row: u64, hits: &[(i64, u16)], window: i64) -> Command {
    drum(DrumCmd::Paint {
        seq: SeqId(0),
        lane: LaneId(0),
        row: DrumRowId(row),
        hits: hits
            .iter()
            .map(|&(t, v)| DrumHit {
                tick: Tick(t),
                velocity: v,
            })
            .collect(),
        window: Tick(window),
        duration: Tick(PPQ / 8),
    })
}

#[test]
fn showing_a_rows_velocity_is_one_undo_step() {
    // Carried from plan 4a (Task 5): the command had no core test.
    let mut h = History::new(kit());
    let show = |on| {
        drum(DrumCmd::SetRowShowVelocity {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(1),
            show: on,
        })
    };
    h.execute(&show(true), &ViewFilter::default()).unwrap();
    let shown = |d: &Document| {
        d.lane(SeqId(0), LaneId(0))
            .unwrap()
            .drum
            .as_ref()
            .unwrap()
            .rows[1]
            .show_velocity
    };
    assert!(shown(h.doc()));
    assert_eq!(h.undo_label(), Some("Show Drum Row Velocity"));
    h.undo().unwrap();
    assert!(!shown(h.doc()));
    assert!(
        run(
            &kit(),
            drum(DrumCmd::SetRowShowVelocity {
                seq: SeqId(0),
                lane: LaneId(0),
                row: DrumRowId(9),
                show: true,
            })
        )
        .is_err(),
        "an unknown row is refused"
    );
}

#[test]
fn paint_adds_hits_where_the_row_has_none_within_a_window() {
    let q = PPQ / 4;
    // Row 0 (C2) has notes at 0 and 2 beats. A window of a 16th skips the
    // hit at 0, the one a little before beat 2, and the repeated one.
    let a = apply(
        &kit(),
        &paint(
            0,
            &[
                (0, 512),
                (q, 40_000),
                (2 * q, 512),
                (2 * PPQ - 100, 512),
                (q, 512),
            ],
            q,
        ),
    )
    .unwrap();
    assert_eq!(
        row_notes(&a.doc, 36),
        vec![
            (0, 36, 100, None),
            (q, 36, 40_000, None),
            (2 * q, 36, 512, None),
            (2 * PPQ, 36, 100, None),
        ]
    );
    assert_eq!(a.report.created.len(), 2, "{:?}", a.report);
    assert!(validate(&a.doc).is_empty());
    // A row that names a destination gives its notes that destination.
    let d = run(
        &kit(),
        drum(DrumCmd::AddRow {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Clap".into(),
            note: 39,
            dest: Some(DestId(1)),
        }),
    )
    .unwrap();
    let d = run(&d, paint(2, &[(0, 900)], q)).unwrap();
    assert_eq!(row_notes(&d, 39), vec![(0, 39, 900, Some(DestId(1)))]);
    // The drum view lists it in its own row (no new row was made).
    assert_eq!(rows(&d, 0).len(), 3);
}

#[test]
fn paint_refuses_bad_input_and_changes_nothing() {
    let max = 1i64 << 40;
    let bad = [
        paint(0, &[(0, 0)], 10),
        paint(0, &[(max + 1, 1)], 10),
        paint(0, &[(0, 1)], 0),
        paint(0, &[(0, 1)], max + 1),
        paint(9, &[(0, 1)], 10),
        drum(DrumCmd::Paint {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
            hits: vec![],
            window: Tick(10),
            duration: Tick(0),
        }),
        drum(DrumCmd::Paint {
            seq: SeqId(0),
            lane: LaneId(1),
            row: DrumRowId(0),
            hits: vec![],
            window: Tick(10),
            duration: Tick(10),
        }),
        drum(DrumCmd::Paint {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
            hits: vec![
                DrumHit {
                    tick: Tick(0),
                    velocity: 1
                };
                MAX_PAINT_HITS + 1
            ],
            window: Tick(10),
            duration: Tick(10),
        }),
    ];
    for c in bad {
        let mut h = History::new(kit());
        assert!(h.execute(&c, &ViewFilter::default()).is_err(), "{c:?}");
        assert_eq!(h.doc(), &kit());
        assert_eq!(h.undo_depth(), 0);
    }
    // A paint whose every hit is already there adds no undo step.
    let mut h = History::new(kit());
    h.execute(
        &paint(0, &[(0, 1), (2 * PPQ, 1)], 10),
        &ViewFilter::default(),
    )
    .unwrap();
    assert_eq!(h.undo_depth(), 0);
}

fn arb_hits() -> impl Strategy<Value = Vec<(i64, u16)>> {
    let tick = prop_oneof![
        -(4 * PPQ)..(8 * PPQ),
        Just(i64::MIN),
        Just(i64::MAX),
        Just((1 << 40) - 1),
        Just(-(1 << 40)),
    ];
    prop::collection::vec((tick, any::<u16>()), 0..40)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Hostile paints (any ticks, velocities, windows, durations, rows,
    /// lanes) never panic; one that applies leaves a valid document whose
    /// new notes are the row's, at distinct ticks, none with an older note
    /// of the row starting within its window, and undo brings the
    /// original back.
    #[test]
    fn hostile_paints_apply_cleanly_or_change_nothing(
        hits in arb_hits(),
        window in prop_oneof![-5i64..PPQ, Just(i64::MAX), Just(1i64 << 40)],
        duration in prop_oneof![-5i64..PPQ, Just(i64::MAX)],
        row in 0u64..4,
        lane in 0u64..3,
    ) {
        let mut h = History::new(kit());
        let cmd = drum(DrumCmd::Paint {
            seq: SeqId(0),
            lane: LaneId(lane),
            row: DrumRowId(row),
            hits: hits.iter().map(|&(t, v)| DrumHit { tick: Tick(t), velocity: v }).collect(),
            window: Tick(window),
            duration: Tick(duration),
        });
        match h.execute(&cmd, &ViewFilter::default()) {
            Err(_) => prop_assert_eq!(h.doc(), &kit()),
            Ok(report) => {
                prop_assert!(validate(h.doc()).is_empty(), "{:?}", validate(h.doc()));
                let pitch = if row == 0 { 36 } else { 38 };
                let before: Vec<i64> = row_notes(&kit(), pitch).iter().map(|n| n.0).collect();
                let after: Vec<i64> = row_notes(h.doc(), pitch).iter().map(|n| n.0).collect();
                prop_assert_eq!(after.len(), before.len() + report.created.len());
                let added: Vec<i64> = after.iter().copied().filter(|t| !before.contains(t)).collect();
                prop_assert_eq!(added.len(), report.created.len(), "distinct new ticks");
                for a in &added {
                    prop_assert!(
                        !before.iter().any(|b| b >= a && *b < a.saturating_add(window)),
                        "{a} has an older note within {window}"
                    );
                }
                let text = core_model::file::to_json(h.doc()).unwrap();
                prop_assert_eq!(&core_model::file::from_json(&text).unwrap(), h.doc());
                while h.undo_depth() > 0 {
                    h.undo().unwrap();
                }
                prop_assert_eq!(h.doc(), &kit());
            }
        }
    }
}
