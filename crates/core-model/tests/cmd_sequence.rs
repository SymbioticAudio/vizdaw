use core_model::ModelError;
use core_model::command::{Command, SequenceCmd, apply};
use core_model::document::{Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{AnyId, Counters, EventId, LaneId, SeqId, TakeId};
use core_model::lane::Lane;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::rules::validate;
use core_model::sequence::{KeySlot, SeqLength, Sequence};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};
use im::Vector;

fn run(d: &Document, c: SequenceCmd) -> core_model::Result<Document> {
    apply(d, &Command::Sequence(c)).map(|a| a.doc)
}

/// Seq 0 ("Song") references seq 1 ("Verse") twice; seq 1 holds one note.
fn song() -> Document {
    let mut song_lane = Lane::new(LaneId(0), TakeId(0), "L");
    for (i, t) in [0, PPQ * 4].into_iter().enumerate() {
        let ev = EventSpec::new(Tick(t), EventBody::reference(SeqId(1))).with_id(EventId(i as u64));
        song_lane
            .takes
            .get_mut(0)
            .unwrap()
            .events
            .insert(ev.key(), ev);
    }
    let mut verse_lane = Lane::new(LaneId(1), TakeId(1), "L");
    let n = EventSpec::new(Tick(0), EventBody::note(60, 100, Tick(PPQ))).with_id(EventId(2));
    verse_lane
        .takes
        .get_mut(0)
        .unwrap()
        .events
        .insert(n.key(), n);
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![],
        sequences: vec![
            Sequence::new(SeqId(0), "Song", Vector::unit(song_lane)),
            Sequence::new(SeqId(1), "Verse", Vector::unit(verse_lane)),
        ],
        counters: Counters {
            seq: 2,
            lane: 2,
            take: 2,
            event: 3,
            ..Counters::default()
        },
    })
    .unwrap()
}

#[test]
fn new_sequence_has_one_lane_with_one_take() {
    let a = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "A".into() }),
    )
    .unwrap();
    assert_eq!(
        a.report.created,
        vec![
            AnyId::Seq(SeqId(0)),
            AnyId::Lane(LaneId(0)),
            AnyId::Take(TakeId(0))
        ]
    );
    let s = a.doc.sequence(SeqId(0)).unwrap();
    assert_eq!(s.lanes.len(), 1);
    assert_eq!(s.lanes[0].takes.len(), 1);
    assert!(validate(&a.doc).is_empty());
}

#[test]
fn apply_is_pure() {
    let d = song();
    let before = d.clone();
    let _ = run(
        &d,
        SequenceCmd::Rename {
            seq: SeqId(0),
            name: "X".into(),
        },
    )
    .unwrap();
    assert_eq!(d, before);
}

#[test]
fn duplicate_gets_fresh_ids_and_no_slot() {
    let d = run(
        &song(),
        SequenceCmd::SetKeyboardSlot {
            seq: SeqId(1),
            slot: Some(KeySlot {
                key: "v".into(),
                modifiers: 0,
            }),
        },
    )
    .unwrap();
    let d = run(&d, SequenceCmd::Duplicate { seq: SeqId(1) }).unwrap();
    let copy = d.sequence(SeqId(2)).unwrap();
    assert_eq!(copy.name, "Verse copy");
    assert_eq!(copy.keyboard_slot, None);
    assert_ne!(copy.lanes[0].id, LaneId(1));
    let ev = copy.lanes[0]
        .active()
        .unwrap()
        .events
        .values()
        .next()
        .unwrap();
    assert_eq!(ev.id, EventId(3));
    assert!(d.event(EventId(3)).is_ok());
    assert!(validate(&d).is_empty(), "{:?}", validate(&d));
}

#[test]
fn deleting_a_used_sequence_needs_confirmation() {
    let d = song();
    let err = run(
        &d,
        SequenceCmd::Delete {
            seq: SeqId(1),
            delete_references: false,
        },
    )
    .unwrap_err();
    assert_eq!(
        err,
        ModelError::InUse {
            users: vec![AnyId::Event(EventId(0)), AnyId::Event(EventId(1))]
        }
    );
    let a = apply(
        &d,
        &Command::Sequence(SequenceCmd::Delete {
            seq: SeqId(1),
            delete_references: true,
        }),
    )
    .unwrap();
    assert_eq!(
        a.report.removed,
        vec![
            AnyId::Event(EventId(0)),
            AnyId::Event(EventId(1)),
            AnyId::Seq(SeqId(1))
        ]
    );
    assert!(a.doc.sequence(SeqId(1)).is_err());
    assert!(a.doc.event(EventId(2)).is_err());
    assert!(validate(&a.doc).is_empty());
}

#[test]
fn unused_sequence_deletes_without_confirmation() {
    let d = run(
        &song(),
        SequenceCmd::Delete {
            seq: SeqId(0),
            delete_references: false,
        },
    )
    .unwrap();
    assert_eq!(d.sequences().len(), 1);
}

#[test]
fn keyboard_slots_are_unique() {
    let slot = KeySlot {
        key: "a".into(),
        modifiers: 8,
    };
    let d = run(
        &song(),
        SequenceCmd::SetKeyboardSlot {
            seq: SeqId(0),
            slot: Some(slot.clone()),
        },
    )
    .unwrap();
    assert!(matches!(
        run(
            &d,
            SequenceCmd::SetKeyboardSlot {
                seq: SeqId(1),
                slot: Some(slot.clone())
            }
        ),
        Err(ModelError::Invalid { .. })
    ));
    // Re-assigning the same slot to its owner is fine.
    assert!(
        run(
            &d,
            SequenceCmd::SetKeyboardSlot {
                seq: SeqId(0),
                slot: Some(slot)
            }
        )
        .is_ok()
    );
}

#[test]
fn tempo_meter_and_ranges_are_validated() {
    let d = song();
    let bad_tempo = TempoMap::constant(Bpm::whole(120)).unwrap();
    assert!(
        run(
            &d,
            SequenceCmd::SetTempo {
                seq: SeqId(0),
                tempo: bad_tempo
            }
        )
        .is_ok()
    );
    let ramp = TempoMap::new(vec![
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(60),
            ramp: true,
        },
        TempoPoint {
            tick: Tick(PPQ * 8),
            bpm: Bpm::whole(180),
            ramp: false,
        },
    ])
    .unwrap();
    let d2 = run(
        &d,
        SequenceCmd::SetTempo {
            seq: SeqId(0),
            tempo: ramp.clone(),
        },
    )
    .unwrap();
    assert_eq!(d2.sequence(SeqId(0)).unwrap().tempo, ramp);
    let meter = MeterMap::new(vec![MeterEntry {
        tick: Tick(0),
        meter: Meter { num: 3, den: 4 },
        key: KeySig::default(),
    }])
    .unwrap();
    assert!(
        run(
            &d,
            SequenceCmd::SetMeter {
                seq: SeqId(0),
                meter
            }
        )
        .is_ok()
    );
    assert!(matches!(
        run(
            &d,
            SequenceCmd::SetStartOffset {
                seq: SeqId(0),
                offset: Tick(-1)
            }
        ),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(
        run(
            &d,
            SequenceCmd::SetStartOffset {
                seq: SeqId(0),
                offset: Tick(PPQ)
            }
        )
        .is_ok()
    );
    assert!(matches!(
        run(
            &d,
            SequenceCmd::SetLength {
                seq: SeqId(0),
                length: SeqLength::Locked(Tick(0))
            }
        ),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(matches!(
        run(
            &d,
            SequenceCmd::Rename {
                seq: SeqId(9),
                name: "x".into()
            }
        ),
        Err(ModelError::UnknownId(AnyId::Seq(SeqId(9))))
    ));
}

#[test]
fn command_names_are_stable() {
    assert_eq!(
        Command::Sequence(SequenceCmd::New {
            name: String::new()
        })
        .name(),
        "New Sequence"
    );
}
