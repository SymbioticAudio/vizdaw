use core_model::ModelError;
use core_model::command::{Command, DrumCmd, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{
    AnyId, Counters, DestId, DrumRowId, EventId, LaneId, PluginInstanceId, SeqId,
};
use core_model::mixer::{PluginRef, PluginSlot};
use core_model::rules::validate;
use core_model::time::{PPQ, Tick};

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
