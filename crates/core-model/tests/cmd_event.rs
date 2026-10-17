use core_model::ModelError;
use core_model::command::{Command, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{AnyId, EventId, LaneId, SeqId, TakeId};
use core_model::rules::validate;
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn ev(c: EventCmd) -> Command {
    Command::Event(c)
}

/// Two new sequences: seq0 (lane0) and seq1 (lane1).
fn two_seqs() -> Document {
    let d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "A".into() }),
    )
    .unwrap();
    run(&d, Command::Sequence(SequenceCmd::New { name: "B".into() })).unwrap()
}

fn insert(
    d: &Document,
    seq: u64,
    lane: u64,
    specs: Vec<EventSpec>,
) -> core_model::Result<Document> {
    run(
        d,
        ev(EventCmd::Insert {
            seq: SeqId(seq),
            lane: LaneId(lane),
            events: specs,
        }),
    )
}

fn note(t: i64, pitch: u8) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::note(pitch, 100, Tick(PPQ)))
}

fn body(d: &Document, id: u64) -> EventBody {
    d.event(EventId(id)).unwrap().0.body.clone()
}

#[test]
fn insert_allocates_ids_and_reports_them() {
    let a = apply(
        &two_seqs(),
        &ev(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![note(0, 60), note(PPQ, 62)],
        }),
    )
    .unwrap();
    assert_eq!(
        a.report.created,
        vec![AnyId::Event(EventId(0)), AnyId::Event(EventId(1))]
    );
    let (_, loc) = a.doc.event(EventId(1)).unwrap();
    assert_eq!(
        (loc.seq, loc.lane, loc.take, loc.tick),
        (SeqId(0), LaneId(0), TakeId(0), Tick(PPQ))
    );
    assert!(validate(&a.doc).is_empty());
}

#[test]
fn insert_goes_to_the_active_take() {
    let d = run(
        &two_seqs(),
        Command::Lane(LaneCmd::AddTake {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "T2".into(),
        }),
    )
    .unwrap();
    let d = insert(&d, 0, 0, vec![note(0, 60)]).unwrap();
    assert_eq!(d.event(EventId(0)).unwrap().1.take, TakeId(2));
}

#[test]
fn failed_insert_changes_nothing() {
    // Through History, which owns the document: a batch whose second spec
    // is invalid must leave no partial change, no undo step and no consumed
    // IDs behind.
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60)]).unwrap();
    let mut h = History::new(d.clone());
    let bad = ev(EventCmd::Insert {
        seq: SeqId(0),
        lane: LaneId(0),
        events: vec![note(0, 61), note(0, 200)],
    });
    let err = h.execute(&bad, &ViewFilter::default()).unwrap_err();
    assert!(matches!(err, ModelError::OutOfRange { field: "pitch", .. }));
    assert_eq!(h.doc(), &d);
    assert_eq!(h.undo_depth(), 0);
    assert_eq!(h.doc().all_events().count(), 1);
    // The first spec of the failed batch was not inserted: inserting it now
    // gets the next ID, so none were consumed by the failed attempt.
    let ok = ev(EventCmd::Insert {
        seq: SeqId(0),
        lane: LaneId(0),
        events: vec![note(0, 61)],
    });
    let r = h.execute(&ok, &ViewFilter::default()).unwrap();
    assert_eq!(r.created, vec![AnyId::Event(EventId(1))]);
    assert_eq!(h.doc().all_events().count(), 2);
}

#[test]
fn self_and_indirect_reference_cycles_are_refused() {
    let d = two_seqs();
    let self_ref = EventSpec::new(Tick(0), EventBody::reference(SeqId(0)));
    assert!(matches!(
        insert(&d, 0, 0, vec![self_ref]),
        Err(ModelError::Cycle { .. })
    ));
    let d = insert(
        &d,
        0,
        0,
        vec![EventSpec::new(Tick(0), EventBody::reference(SeqId(1)))],
    )
    .unwrap();
    let back = EventSpec::new(Tick(0), EventBody::reference(SeqId(0)));
    assert_eq!(
        insert(&d, 1, 1, vec![back]),
        Err(ModelError::Cycle {
            path: vec![SeqId(1), SeqId(0), SeqId(1)]
        })
    );
}

#[test]
fn moving_a_reference_into_its_target_is_a_cycle() {
    let d = insert(
        &two_seqs(),
        0,
        0,
        vec![EventSpec::new(Tick(0), EventBody::reference(SeqId(1)))],
    )
    .unwrap();
    let err = run(
        &d,
        ev(EventCmd::Move {
            events: vec![EventId(0)],
            delta: Tick(0),
            to_lane: Some((SeqId(1), LaneId(1))),
        }),
    )
    .unwrap_err();
    assert!(matches!(err, ModelError::Cycle { .. }));
}

#[test]
fn move_in_time_and_across_lanes() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60)]).unwrap();
    let d = run(
        &d,
        ev(EventCmd::Move {
            events: vec![EventId(0)],
            delta: Tick(PPQ),
            to_lane: None,
        }),
    )
    .unwrap();
    assert_eq!(d.event(EventId(0)).unwrap().0.tick, Tick(PPQ));
    let d = run(
        &d,
        ev(EventCmd::Move {
            events: vec![EventId(0), EventId(0)],
            delta: Tick(-PPQ),
            to_lane: Some((SeqId(1), LaneId(1))),
        }),
    )
    .unwrap();
    let (e, loc) = d.event(EventId(0)).unwrap();
    assert_eq!((e.tick, loc.seq, loc.lane), (Tick(0), SeqId(1), LaneId(1)));
    assert!(validate(&d).is_empty());
}

#[test]
fn delete_removes_and_reports() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60)]).unwrap();
    let a = apply(
        &d,
        &ev(EventCmd::Delete {
            events: vec![EventId(0)],
        }),
    )
    .unwrap();
    assert_eq!(a.report.removed, vec![AnyId::Event(EventId(0))]);
    assert!(a.doc.event(EventId(0)).is_err());
    assert!(matches!(
        run(
            &a.doc,
            ev(EventCmd::Delete {
                events: vec![EventId(0)]
            })
        ),
        Err(ModelError::UnknownId(_))
    ));
}

#[test]
fn setters_touch_only_applicable_kinds() {
    let cc = EventSpec::new(Tick(0), EventBody::Cc { index: 1, value: 5 });
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60), cc]).unwrap();
    let both = vec![EventId(0), EventId(1)];
    let d = run(
        &d,
        ev(EventCmd::SetPitch {
            events: both.clone(),
            pitch: 64,
        }),
    )
    .unwrap();
    let d = run(
        &d,
        ev(EventCmd::SetVelocity {
            events: both.clone(),
            velocity: 9,
        }),
    )
    .unwrap();
    let d = run(
        &d,
        ev(EventCmd::SetDuration {
            events: both.clone(),
            duration: Tick(10),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        ev(EventCmd::SetValue {
            events: both.clone(),
            value: 77,
        }),
    )
    .unwrap();
    assert_eq!(body(&d, 0), EventBody::note(64, 9, Tick(10)));
    assert_eq!(
        body(&d, 1),
        EventBody::Cc {
            index: 1,
            value: 77
        }
    );
    assert!(
        run(
            &d,
            ev(EventCmd::SetDuration {
                events: both.clone(),
                duration: Tick(-1)
            })
        )
        .is_err()
    );
}

#[test]
fn overrides_and_mute() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60)]).unwrap();
    let one = vec![EventId(0)];
    let d = run(
        &d,
        ev(EventCmd::SetGroupChannel {
            events: one.clone(),
            group_channel: Some((2, 9)),
        }),
    )
    .unwrap();
    assert_eq!(d.event(EventId(0)).unwrap().0.group_channel, Some((2, 9)));
    assert!(
        run(
            &d,
            ev(EventCmd::SetGroupChannel {
                events: one.clone(),
                group_channel: Some((16, 0))
            })
        )
        .is_err()
    );
    assert!(matches!(
        run(
            &d,
            ev(EventCmd::SetDest {
                events: one.clone(),
                dest: Some(core_model::ids::DestId(3))
            })
        ),
        Err(ModelError::UnknownId(_))
    ));
    let d = run(
        &d,
        ev(EventCmd::SetMute {
            events: one,
            mute: true,
        }),
    )
    .unwrap();
    assert!(d.event(EventId(0)).unwrap().0.mute);
}

#[test]
fn nudge_clamps_pitch_at_0_and_127() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 120), note(0, 3)]).unwrap();
    let d = run(
        &d,
        ev(EventCmd::Nudge {
            events: vec![EventId(0)],
            ticks: Tick(5),
            semitones: 12,
        }),
    )
    .unwrap();
    let d = run(
        &d,
        ev(EventCmd::Nudge {
            events: vec![EventId(1)],
            ticks: Tick(0),
            semitones: -12,
        }),
    )
    .unwrap();
    assert_eq!(body(&d, 0), EventBody::note(127, 100, Tick(PPQ)));
    assert_eq!(d.event(EventId(0)).unwrap().0.tick, Tick(5));
    assert_eq!(body(&d, 1), EventBody::note(0, 100, Tick(PPQ)));
}

#[test]
fn move_across_lanes_reports_moved() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60), note(1, 62)]).unwrap();
    let a = core_model::command::apply(
        &d,
        &ev(EventCmd::Move {
            events: vec![EventId(1), EventId(0), EventId(1)],
            delta: Tick(0),
            to_lane: Some((SeqId(1), LaneId(1))),
        }),
    )
    .unwrap();
    assert_eq!(
        a.report.moved,
        vec![
            core_model::ids::AnyId::Event(EventId(0)),
            core_model::ids::AnyId::Event(EventId(1))
        ]
    );
}

use core_model::event::{PerNoteCtl, PerNoteKind};
use core_model::modify::scale_step;

const C_MAJOR: u16 = 0b1010_1011_0101;

fn pitch_of(d: &Document, id: u64) -> u8 {
    match body(d, id) {
        EventBody::Note { pitch, .. } => pitch,
        b => panic!("not a note: {b:?}"),
    }
}

#[test]
fn duplicate_copies_in_place_with_offsets() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 120), note(PPQ, 60)]).unwrap();
    let a = core_model::command::apply(
        &d,
        &ev(EventCmd::Duplicate {
            events: vec![EventId(0), EventId(1)],
            ticks: Tick(10),
            semitones: 12,
        }),
    )
    .unwrap();
    assert_eq!(a.report.created.len(), 2);
    let d = a.doc;
    assert_eq!(pitch_of(&d, 0), 120, "originals stay");
    assert_eq!(pitch_of(&d, 2), 127, "clamped at 127");
    assert_eq!(d.event(EventId(2)).unwrap().0.tick, Tick(10));
    assert_eq!(pitch_of(&d, 3), 72);
    assert_eq!(d.event(EventId(3)).unwrap().1.lane, LaneId(0));
}

#[test]
fn resize_is_relative_and_keeps_one_tick() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60), note(0, 62)]).unwrap();
    let d = run(
        &d,
        ev(EventCmd::Resize {
            events: vec![EventId(0), EventId(1)],
            delta: Tick(-PPQ / 2),
        }),
    )
    .unwrap();
    assert_eq!(body(&d, 0), EventBody::note(60, 100, Tick(PPQ / 2)));
    let d = run(
        &d,
        ev(EventCmd::Resize {
            events: vec![EventId(0)],
            delta: Tick(-10 * PPQ),
        }),
    )
    .unwrap();
    assert_eq!(body(&d, 0), EventBody::note(60, 100, Tick(1)));
    assert_eq!(body(&d, 1), EventBody::note(62, 100, Tick(PPQ / 2)));
}

#[test]
fn transpose_in_scale_moves_by_degrees() {
    // E4 F4 C#4 B4 C4
    let d = insert(
        &two_seqs(),
        0,
        0,
        vec![
            note(0, 64),
            note(0, 65),
            note(0, 61),
            note(0, 71),
            note(0, 60),
        ],
    )
    .unwrap();
    let up = |steps: i8| {
        run(
            &d,
            ev(EventCmd::TransposeInScale {
                events: (0..5).map(EventId).collect(),
                steps,
                root: 0,
                mask: C_MAJOR,
            }),
        )
        .unwrap()
    };
    let d1 = up(1);
    let got: Vec<u8> = (0..5).map(|i| pitch_of(&d1, i)).collect();
    assert_eq!(got, vec![65, 67, 62, 72, 62]);
    let d2 = up(-1);
    let got: Vec<u8> = (0..5).map(|i| pitch_of(&d2, i)).collect();
    assert_eq!(got, vec![62, 64, 60, 69, 59]);
    for bad in [
        EventCmd::TransposeInScale {
            events: vec![EventId(0)],
            steps: 1,
            root: 12,
            mask: C_MAJOR,
        },
        EventCmd::TransposeInScale {
            events: vec![EventId(0)],
            steps: 1,
            root: 0,
            mask: 0,
        },
    ] {
        assert!(matches!(
            run(&d, ev(bad)),
            Err(ModelError::OutOfRange { .. })
        ));
    }
}

#[test]
fn scale_step_stops_at_the_range_edges() {
    assert_eq!(scale_step(127, 1, 0, C_MAJOR), 127, "G9 is the top");
    assert_eq!(scale_step(126, 1, 0, C_MAJOR), 127);
    assert_eq!(scale_step(0, -1, 0, C_MAJOR), 0);
    assert_eq!(scale_step(60, 7, 0, C_MAJOR), 72, "an octave");
    assert_eq!(scale_step(60, 0, 0, C_MAJOR), 60);
    assert_eq!(
        scale_step(61, 0, 0, C_MAJOR),
        61,
        "zero steps keep an off-scale pitch"
    );
}

#[test]
fn set_per_note_replaces_values_in_the_range() {
    let d = insert(&two_seqs(), 0, 0, vec![note(PPQ, 60)]).unwrap();
    let pb = PerNoteKind::PitchBend;
    let set = |d: &Document, from: i64, to: i64, points: Vec<(i64, u32)>| {
        run(
            d,
            ev(EventCmd::SetPerNote {
                events: vec![EventId(0)],
                kind: pb,
                from: Tick(from),
                to: Tick(to),
                points: points.into_iter().map(|(t, v)| (Tick(t), v)).collect(),
            }),
        )
        .unwrap()
    };
    // Absolute ticks; the point before the note, the one at its note-off
    // (no effect there) and the one after it are dropped.
    let d = set(
        &d,
        0,
        3 * PPQ,
        vec![
            (0, 1),
            (PPQ, 2),
            (PPQ + 100, 3),
            (2 * PPQ, 4),
            (2 * PPQ + 1, 5),
        ],
    );
    let values = |d: &Document| match body(d, 0) {
        EventBody::Note { per_note, .. } => per_note
            .iter()
            .map(|p: &PerNoteCtl| (p.offset.0, p.value))
            .collect::<Vec<_>>(),
        b => panic!("{b:?}"),
    };
    assert_eq!(values(&d), vec![(0, 2), (100, 3)]);
    // The last tick inside the note is kept.
    let d = set(
        &d,
        2 * PPQ - 1,
        2 * PPQ,
        vec![(2 * PPQ - 1, 4), (2 * PPQ, 6)],
    );
    assert_eq!(values(&d), vec![(0, 2), (100, 3), (PPQ - 1, 4)]);
    // Erase the middle one only.
    let d = set(&d, PPQ + 50, PPQ + 150, vec![]);
    assert_eq!(values(&d), vec![(0, 2), (PPQ - 1, 4)]);
    // Another kind is untouched by a pitch-bend write.
    let d = run(
        &d,
        ev(EventCmd::SetPerNote {
            events: vec![EventId(0)],
            kind: PerNoteKind::Registered(1),
            from: Tick(0),
            to: Tick(10 * PPQ),
            points: vec![(Tick(PPQ + 10), 9)],
        }),
    )
    .unwrap();
    assert_eq!(values(&d), vec![(0, 2), (10, 9), (PPQ - 1, 4)]);
    assert!(matches!(
        run(
            &d,
            ev(EventCmd::SetPerNote {
                events: vec![EventId(0)],
                kind: pb,
                from: Tick(5),
                to: Tick(4),
                points: vec![],
            })
        ),
        Err(ModelError::OutOfRange { .. })
    ));
}
