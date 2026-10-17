//! Editor commands (spec §3.2, §6): every variant applies and undoes;
//! gestures replace their content; a refused target leaves the start.

use core_engine::fixtures::{Builder, Q};
use core_model::command::Command;
use core_model::command::lane::LaneCmd;
use core_model::command::routing::RoutingCmd;
use core_model::document::Document;
use core_model::event::{EventBody, Fade};
use core_model::ids::{AnyId, DestId, EventId, LaneId, MediaId, OutputPathId, SeqId, TakeId};
use std::collections::HashMap;
use vizdaw_ffi::edit::to_command;
use vizdaw_ffi::{
    AudioEdit, EditCommand, EventEdit, EventKindRow, EventRow, FadeRow, FadeShapeRow, GeneratorRow,
    GroupChannel, KeySlotRow, LaneEdit, LaneRef, MarkerRow, MeterEntryRow, PerNoteKindRow,
    PerNotePoint, PerNoteValueRow, QuantizeRow, ReferenceEdit, SequenceEdit, Session,
    TempoPointRow, VizError,
};

const BAR: i64 = 4 * Q;

struct Fixture {
    doc: Document,
    s: SeqId,
    l1: LaneId,
    l2: LaneId,
    sub2: SeqId,
    sub: SeqId,
    note: EventId,
    cc: EventId,
    region: EventId,
    reference: EventId,
    dest2: DestId,
    out: OutputPathId,
    media: MediaId,
    take1: TakeId,
}

fn created<T>(r: &core_model::command::Report, pick: impl Fn(&AnyId) -> Option<T>) -> T {
    r.created.iter().find_map(pick).unwrap()
}

fn fixture() -> Fixture {
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let dest2 = b.dest("Bass", "builtin:synth", 1).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: "Out 3-4".into(),
            reference: "hw:3".into(),
        }))
        .unwrap();
    let out = created(&r, |id| match id {
        AnyId::Output(o) => Some(*o),
        _ => None,
    });
    let r = b
        .run(Command::Routing(RoutingCmd::AddMedia {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 480_000,
            content_hash: None,
        }))
        .unwrap();
    let media = created(&r, |id| match id {
        AnyId::Media(m) => Some(*m),
        _ => None,
    });
    let (sub, sub_lane) = b.sequence("Sub", "L", Some(piano)).unwrap();
    b.notes(sub, sub_lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let (sub2, sub2_lane) = b.sequence("Sub2", "L", Some(piano)).unwrap();
    b.notes(sub2, sub2_lane, &[(0, 67, 0x8000, Q)]).unwrap();
    let (s, l1) = b.sequence("S", "One", Some(piano)).unwrap();
    let l2 = b.lane(s, "Two", Some(piano)).unwrap();
    let note = b.notes(s, l1, &[(0, 60, 0x8000, Q)]).unwrap()[0];
    let cc = b
        .event(
            s,
            l1,
            Q,
            EventBody::Cc {
                index: 7,
                value: 1 << 30,
            },
        )
        .unwrap();
    let reference = b.reference(s, l1, BAR, sub, 1, 0).unwrap();
    let region = b
        .event(
            s,
            l1,
            2 * BAR,
            EventBody::AudioRegion {
                media,
                source_offset: 0,
                length: 48_000,
                gain_db: 0.0,
                fade_in: Fade::default(),
                fade_out: Fade::default(),
            },
        )
        .unwrap();
    let take1 = b.doc().lane(s, l2).unwrap().active_take;
    b.run(Command::Lane(LaneCmd::AddTake {
        seq: s,
        lane: l2,
        name: "Take 2".into(),
    }))
    .unwrap();
    Fixture {
        doc: b.finish(),
        s,
        l1,
        l2,
        sub2,
        sub,
        note,
        cc,
        region,
        reference,
        dest2,
        out,
        media,
        take1,
    }
}

/// Everything the app can read, without revisions.
fn state(s: &Session) -> String {
    let snap = s.snapshot().unwrap();
    let mut out = format!("{:?}\n", snap.sequences);
    for row in &snap.sequences {
        out += &format!("{:?}\n", s.sequence_info(row.id).unwrap());
        for l in s.lanes(row.id).unwrap() {
            out += &format!("{l:?}\n{:?}\n", s.lane_events(row.id, l.id).unwrap().events);
        }
    }
    out
}

fn fade(length: i64) -> FadeRow {
    FadeRow {
        length,
        shape: FadeShapeRow::EqualPower,
    }
}

fn ev(e: EventEdit) -> EditCommand {
    EditCommand::Event { edit: e }
}

fn commands(f: &Fixture) -> Vec<EditCommand> {
    let (s, l1, l2) = (f.s.0, f.l1.0, f.l2.0);
    let one = |id: EventId| vec![id.0];
    let mut row = EventRow::blank(0, 3 * Q, EventKindRow::Note);
    row.pitch = 64;
    row.velocity = 0x8000;
    row.duration = Q;
    vec![
        ev(EventEdit::Insert {
            seq: s,
            lane: l2,
            events: vec![row],
        }),
        ev(EventEdit::Delete {
            events: one(f.note),
        }),
        ev(EventEdit::Move {
            events: one(f.note),
            delta: Q,
            to_lane: None,
        }),
        ev(EventEdit::Move {
            events: one(f.note),
            delta: 0,
            to_lane: Some(LaneRef { seq: s, lane: l2 }),
        }),
        ev(EventEdit::SetPitch {
            events: one(f.note),
            pitch: 61,
        }),
        ev(EventEdit::SetVelocity {
            events: one(f.note),
            velocity: 0x1000,
        }),
        ev(EventEdit::SetDuration {
            events: one(f.note),
            duration: 2 * Q,
        }),
        ev(EventEdit::SetValue {
            events: one(f.cc),
            value: 5,
        }),
        ev(EventEdit::SetDest {
            events: one(f.note),
            dest: Some(f.dest2.0),
        }),
        ev(EventEdit::SetGroupChannel {
            events: one(f.note),
            group_channel: Some(GroupChannel {
                group: 0,
                channel: 2,
            }),
        }),
        ev(EventEdit::SetMute {
            events: one(f.note),
            mute: true,
        }),
        ev(EventEdit::Nudge {
            events: one(f.note),
            ticks: 10,
            semitones: 2,
        }),
        ev(EventEdit::Duplicate {
            events: one(f.note),
            ticks: Q,
            semitones: 3,
        }),
        ev(EventEdit::Resize {
            events: one(f.note),
            delta: Q / 2,
        }),
        ev(EventEdit::TransposeInScale {
            events: one(f.note),
            steps: 2,
            root: 0,
            mask: 0b1010_1011_0101,
        }),
        ev(EventEdit::SetPerNote {
            events: one(f.note),
            kind: PerNoteKindRow::PitchBend,
            from: 0,
            to: Q,
            points: vec![PerNoteValueRow {
                tick: Q / 2,
                value: 7,
            }],
        }),
        EditCommand::Reference {
            edit: ReferenceEdit::Insert {
                seq: s,
                lane: l2,
                tick: 0,
                target: f.sub.0,
                length: None,
                loop_count: 1,
                transpose: 0,
            },
        },
        EditCommand::Reference {
            edit: ReferenceEdit::RePoint {
                event: f.reference.0,
                target: f.sub2.0,
            },
        },
        EditCommand::Reference {
            edit: ReferenceEdit::MakePrivateCopy {
                event: f.reference.0,
            },
        },
        EditCommand::Reference {
            edit: ReferenceEdit::Flatten {
                event: f.reference.0,
            },
        },
        EditCommand::Reference {
            edit: ReferenceEdit::SetParams {
                event: f.reference.0,
                length: Some(2 * BAR),
                loop_count: 2,
                transpose: 3,
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::Add {
                seq: s,
                name: "Three".into(),
                index: Some(0),
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::Remove { seq: s, lane: l2 },
        },
        EditCommand::Lane {
            edit: LaneEdit::Reorder {
                seq: s,
                lane: l2,
                to: 0,
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::Rename {
                seq: s,
                lane: l1,
                name: "Uno".into(),
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::SetDest {
                seq: s,
                lane: l1,
                dest: None,
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::SetOutput {
                seq: s,
                lane: l1,
                output: Some(f.out.0),
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: s,
                lane: l1,
                mute: Some(true),
                solo: None,
                arm: Some(true),
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::SetLength {
                seq: s,
                lane: l1,
                length: Some(BAR),
                looped: true,
                locked: false,
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::SetPlayQuantize {
                seq: s,
                lane: l1,
                quantize: Some(QuantizeRow {
                    grid: Q / 4,
                    strength: 50,
                }),
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::SetPlayShift {
                seq: s,
                lane: l1,
                shift: -20,
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::AddTake {
                seq: s,
                lane: l1,
                name: "Alt".into(),
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::SetActiveTake {
                seq: s,
                lane: l2,
                take: f.take1.0,
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::New {
                name: "Fresh".into(),
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::Duplicate { seq: s },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::Rename {
                seq: s,
                name: "Song".into(),
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::Delete {
                seq: f.sub.0,
                delete_references: true,
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetKind {
                seq: s,
                generator: Some(GeneratorRow {
                    generator: "arp".into(),
                    params: HashMap::from([("rate".to_string(), "8".to_string())]),
                }),
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetKeyboardSlot {
                seq: s,
                slot: Some(KeySlotRow {
                    key: "F5".into(),
                    modifiers: 8,
                }),
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetTempo {
                seq: s,
                tempo: vec![
                    TempoPointRow {
                        tick: 0,
                        bpm: 100.0,
                        ramp: true,
                    },
                    TempoPointRow {
                        tick: BAR,
                        bpm: 140.5,
                        ramp: false,
                    },
                ],
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetMeter {
                seq: s,
                meter: vec![MeterEntryRow {
                    tick: 0,
                    num: 3,
                    den: 4,
                    sharps: -2,
                    minor: true,
                }],
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetMarkers {
                seq: s,
                markers: vec![MarkerRow {
                    tick: BAR,
                    name: "B".into(),
                }],
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetStartOffset { seq: s, offset: Q },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetLength {
                seq: s,
                locked: Some(8 * BAR),
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetComment {
                seq: s,
                comment: "intro".into(),
            },
        },
        EditCommand::Audio {
            edit: AudioEdit::Insert {
                seq: s,
                lane: l2,
                tick: 0,
                media: f.media.0,
                source_offset: 100,
                length: 4_800,
                gain_db: -6.0,
                fade_in: fade(10),
                fade_out: fade(20),
            },
        },
        EditCommand::Audio {
            edit: AudioEdit::Trim {
                event: f.region.0,
                front_frames: 100,
                back_frames: 100,
            },
        },
        EditCommand::Audio {
            edit: AudioEdit::Split {
                event: f.region.0,
                at: 2 * BAR + Q,
            },
        },
        EditCommand::Audio {
            edit: AudioEdit::SetGain {
                event: f.region.0,
                gain_db: 3.0,
            },
        },
        EditCommand::Audio {
            edit: AudioEdit::SetFades {
                event: f.region.0,
                fade_in: fade(Q / 8),
                fade_out: fade(Q / 4),
            },
        },
    ]
}

#[test]
fn every_variant_applies_and_undoes() {
    let f = fixture();
    let all = commands(&f);
    assert_eq!(
        all.len(),
        50,
        "every variant (Move twice) but FromSelection, with 4b's four"
    );
    for cmd in all {
        let s = Session::from_document(f.doc.clone()).unwrap();
        let (before, revs) = (state(&s), s.snapshot().unwrap().lane_revs);
        let label = to_command(&cmd).unwrap().name();
        let snap = s
            .edit(cmd.clone(), None)
            .unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
        assert_eq!(snap.undo_label.as_deref(), Some(label), "{cmd:?}");
        assert!(
            state(&s) != before || snap.lane_revs != revs,
            "{cmd:?} changed nothing"
        );
        s.undo().unwrap();
        assert_eq!(state(&s), before, "{cmd:?} did not undo");
    }
}

#[test]
fn a_new_lane_gets_a_revision_and_a_removed_one_loses_it() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    let before = s.snapshot().unwrap().lane_revs.len();
    let snap = s
        .edit(
            EditCommand::Lane {
                edit: LaneEdit::Add {
                    seq: f.s.0,
                    name: "New".into(),
                    index: None,
                },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.lane_revs.len(), before + 1);
    let snap = s
        .edit(
            EditCommand::Lane {
                edit: LaneEdit::Remove {
                    seq: f.s.0,
                    lane: f.l2.0,
                },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.lane_revs.len(), before);
    assert!(!snap.lane_revs.iter().any(|r| r.lane == f.l2.0));
}

fn move_note(f: &Fixture, delta: i64, to: Option<LaneRef>) -> EditCommand {
    ev(EventEdit::Move {
        events: vec![f.note.0],
        delta,
        to_lane: to,
    })
}

fn note_tick(s: &Session, f: &Fixture) -> i64 {
    s.lane_events(f.s.0, f.l1.0)
        .unwrap()
        .events
        .iter()
        .find(|r| r.id == f.note.0)
        .unwrap()
        .tick
}

#[test]
fn a_gesture_is_one_undo_step_with_the_last_target() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    s.begin_edit("Move Block".into()).unwrap();
    s.edit_gesture(move_note(&f, Q, None), None).unwrap();
    assert_eq!(note_tick(&s, &f), Q);
    s.edit_gesture(move_note(&f, 2 * Q, None), None).unwrap();
    assert_eq!(note_tick(&s, &f), 2 * Q, "replaced, not added to");
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Move Block"));
    s.undo().unwrap();
    assert_eq!(note_tick(&s, &f), 0);
}

#[test]
fn a_refused_gesture_target_leaves_the_start_state() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    let start = state(&s);
    // Moving the reference to Sub into Sub's own lane would be a cycle.
    let sub_lane = s.lanes(f.sub.0).unwrap()[0].id;
    s.begin_edit("Move Block".into()).unwrap();
    s.edit_gesture(move_note(&f, Q, None), None).unwrap();
    let cycle = ev(EventEdit::Move {
        events: vec![f.reference.0],
        delta: 0,
        to_lane: Some(LaneRef {
            seq: f.sub.0,
            lane: sub_lane,
        }),
    });
    assert!(matches!(
        s.edit_gesture(cycle, None),
        Err(VizError::Model { .. })
    ));
    assert_eq!(state(&s), start, "the earlier target is gone too");
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label, None, "no undo step");
}

#[test]
fn abort_edit_restores_and_closes_the_edit() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    let start = state(&s);
    s.begin_edit("Move Block".into()).unwrap();
    s.edit_gesture(move_note(&f, Q, None), None).unwrap();
    s.abort_edit().unwrap();
    assert_eq!(state(&s), start);
    assert!(matches!(
        s.edit_gesture(move_note(&f, Q, None), None),
        Err(VizError::Model { .. })
    ));
    // Nothing open: abort is a no-op.
    s.abort_edit().unwrap();
}

#[test]
fn an_edit_during_an_open_edit_is_refused_and_changes_nothing() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    s.begin_edit("Move Block".into()).unwrap();
    s.edit_gesture(move_note(&f, Q, None), None).unwrap();
    let during = state(&s);
    assert!(matches!(
        s.edit(move_note(&f, 2 * Q, None), None),
        Err(VizError::Model { .. })
    ));
    assert_eq!(state(&s), during, "the refused edit changed nothing");
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Move Block"));
    assert_eq!(note_tick(&s, &f), Q, "the gesture's target stands alone");
    // Closed: edits apply again.
    s.edit(move_note(&f, Q, None), None).unwrap();
    assert_eq!(note_tick(&s, &f), 2 * Q);
}

#[test]
fn bad_rows_and_values_are_model_errors_and_change_nothing() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    let start = state(&s);
    let mut row = EventRow::blank(0, 0, EventKindRow::Cc);
    row.value2 = 999;
    let bad = ev(EventEdit::Insert {
        seq: f.s.0,
        lane: f.l1.0,
        events: vec![row],
    });
    assert!(matches!(s.edit(bad, None), Err(VizError::Model { .. })));
    let tempo = EditCommand::Sequence {
        edit: SequenceEdit::SetTempo {
            seq: f.s.0,
            tempo: vec![TempoPointRow {
                tick: 0,
                bpm: f64::NAN,
                ramp: false,
            }],
        },
    };
    assert!(matches!(s.edit(tempo, None), Err(VizError::Model { .. })));
    assert_eq!(state(&s), start);
}

#[test]
fn an_event_only_edit_bumps_that_lane_and_no_other() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    let before = s.snapshot().unwrap().lane_revs;
    let mut row = EventRow::blank(0, 0, EventKindRow::Note);
    row.pitch = 64;
    row.velocity = 0x8000;
    row.duration = Q;
    let snap = s
        .edit(
            ev(EventEdit::Insert {
                seq: f.s.0,
                lane: f.l2.0,
                events: vec![row],
            }),
            None,
        )
        .unwrap();
    assert_eq!(snap.lane_revs.len(), before.len());
    for (old, new) in before.iter().zip(&snap.lane_revs) {
        assert_eq!((old.seq, old.lane), (new.seq, new.lane));
        if (old.seq, old.lane) == (f.s.0, f.l2.0) {
            assert_ne!(old.rev, new.rev, "the edited lane changed");
        } else {
            assert_eq!(old.rev, new.rev, "lane {} must not change", old.lane);
        }
    }
}

#[test]
fn per_note_values_read_back_as_offsets() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    s.edit(
        ev(EventEdit::SetPerNote {
            events: vec![f.note.0],
            kind: PerNoteKindRow::Registered { index: 74 },
            from: 0,
            to: Q,
            points: vec![PerNoteValueRow {
                tick: Q / 4,
                value: 1 << 31,
            }],
        }),
        None,
    )
    .unwrap();
    let rows = s.lane_events(f.s.0, f.l1.0).unwrap().events;
    let note = rows.iter().find(|r| r.id == f.note.0).unwrap();
    assert_eq!(
        note.per_note,
        vec![PerNotePoint {
            kind: PerNoteKindRow::Registered { index: 74 },
            offset: Q / 4,
            value: 1 << 31,
        }]
    );
}
