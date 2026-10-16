use core_model::ModelError;
use core_model::command::{Command, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::ids::{AnyId, Counters, DestId, LaneId, OutputPathId, SeqId, TakeId};
use core_model::lane::QuantizeSpec;
use core_model::rules::validate;
use core_model::time::Tick;

fn run(d: &Document, c: LaneCmd) -> core_model::Result<Document> {
    apply(d, &Command::Lane(c)).map(|a| a.doc)
}

/// One destination (dest0) and one new sequence seq0 with lane0/take0.
fn base() -> Document {
    let d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![Destination {
            id: DestId(0),
            name: "Piano".into(),
            kind: DestKind::plugin("p"),
            default_patch: None,
        }],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        mixer: None,
        counters: Counters {
            dest: 1,
            plugin: 1,
            ..Counters::default()
        },
    })
    .unwrap();
    apply(
        &d,
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc
}

fn lane_ids(d: &Document) -> Vec<LaneId> {
    d.sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .map(|l| l.id)
        .collect()
}

#[test]
fn add_inserts_at_index_and_reorder_moves() {
    let d = run(
        &base(),
        LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        },
    )
    .unwrap();
    let d = run(
        &d,
        LaneCmd::Add {
            seq: SeqId(0),
            name: "C".into(),
            index: Some(0),
        },
    )
    .unwrap();
    assert_eq!(lane_ids(&d), vec![LaneId(2), LaneId(0), LaneId(1)]);
    let d = run(
        &d,
        LaneCmd::Reorder {
            seq: SeqId(0),
            lane: LaneId(2),
            to: 99,
        },
    )
    .unwrap();
    assert_eq!(lane_ids(&d), vec![LaneId(0), LaneId(1), LaneId(2)]);
    assert!(validate(&d).is_empty());
}

#[test]
fn remove_reports_lane() {
    let a = apply(
        &base(),
        &Command::Lane(LaneCmd::Remove {
            seq: SeqId(0),
            lane: LaneId(0),
        }),
    )
    .unwrap();
    assert_eq!(a.report.removed, vec![AnyId::Lane(LaneId(0))]);
    assert!(lane_ids(&a.doc).is_empty());
}

#[test]
fn destination_and_output_must_exist() {
    let d = run(
        &base(),
        LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        },
    )
    .unwrap();
    assert_eq!(
        d.lane(SeqId(0), LaneId(0)).unwrap().midi_dest,
        Some(DestId(0))
    );
    assert_eq!(
        run(
            &d,
            LaneCmd::SetDest {
                seq: SeqId(0),
                lane: LaneId(0),
                dest: Some(DestId(7))
            }
        ),
        Err(ModelError::UnknownId(AnyId::Dest(DestId(7))))
    );
    assert_eq!(
        run(
            &d,
            LaneCmd::SetOutput {
                seq: SeqId(0),
                lane: LaneId(0),
                output: Some(OutputPathId(0))
            }
        ),
        Err(ModelError::UnknownId(AnyId::Output(OutputPathId(0))))
    );
}

#[test]
fn flags_change_only_given_values() {
    let d = run(
        &base(),
        LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(0),
            mute: Some(true),
            solo: None,
            arm: Some(true),
        },
    )
    .unwrap();
    let d = run(
        &d,
        LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(0),
            mute: None,
            solo: Some(true),
            arm: None,
        },
    )
    .unwrap();
    let l = d.lane(SeqId(0), LaneId(0)).unwrap();
    assert!(l.mute && l.solo && l.arm);
}

#[test]
fn length_quantize_and_shift_are_validated() {
    let id = (SeqId(0), LaneId(0));
    assert!(matches!(
        run(
            &base(),
            LaneCmd::SetLength {
                seq: id.0,
                lane: id.1,
                length: Some(Tick(0)),
                looped: true,
                locked: false
            }
        ),
        Err(ModelError::OutOfRange { .. })
    ));
    let d = run(
        &base(),
        LaneCmd::SetLength {
            seq: id.0,
            lane: id.1,
            length: Some(Tick(960)),
            looped: true,
            locked: true,
        },
    )
    .unwrap();
    let l = d.lane(id.0, id.1).unwrap();
    assert_eq!(
        (l.length, l.looped, l.length_locked),
        (Some(Tick(960)), true, true)
    );
    assert!(
        run(
            &d,
            LaneCmd::SetPlayQuantize {
                seq: id.0,
                lane: id.1,
                quantize: Some(QuantizeSpec {
                    grid: Tick(0),
                    strength: 100
                })
            }
        )
        .is_err()
    );
    assert!(
        run(
            &d,
            LaneCmd::SetPlayQuantize {
                seq: id.0,
                lane: id.1,
                quantize: Some(QuantizeSpec {
                    grid: Tick(960),
                    strength: 101
                })
            }
        )
        .is_err()
    );
    let d = run(
        &d,
        LaneCmd::SetPlayShift {
            seq: id.0,
            lane: id.1,
            shift: Tick(-30),
        },
    )
    .unwrap();
    assert_eq!(d.lane(id.0, id.1).unwrap().play_shift, Tick(-30));
}

#[test]
fn add_take_becomes_active_and_can_switch_back() {
    let d = run(
        &base(),
        LaneCmd::AddTake {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Take 2".into(),
        },
    )
    .unwrap();
    let l = d.lane(SeqId(0), LaneId(0)).unwrap();
    assert_eq!(l.takes.len(), 2);
    assert_eq!(l.active_take, TakeId(1));
    let d = run(
        &d,
        LaneCmd::SetActiveTake {
            seq: SeqId(0),
            lane: LaneId(0),
            take: TakeId(0),
        },
    )
    .unwrap();
    assert_eq!(d.lane(SeqId(0), LaneId(0)).unwrap().active_take, TakeId(0));
    assert_eq!(
        run(
            &d,
            LaneCmd::SetActiveTake {
                seq: SeqId(0),
                lane: LaneId(0),
                take: TakeId(5)
            }
        ),
        Err(ModelError::UnknownId(AnyId::Take(TakeId(5))))
    );
}
