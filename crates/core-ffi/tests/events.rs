//! Event rows (spec §3.1, §6): every kind round-trips; big lanes are fast.

use core_engine::fixtures::{Builder, Q};
use core_model::command::Command;
use core_model::command::routing::RoutingCmd;
use core_model::document::Document;
use core_model::event::{
    EventBody, EventSpec, Fade, FadeShape, LoopCount, NoteAttribute, PerNoteCtl, PerNoteKind,
    TextKind,
};
use core_model::ids::{AnyId, LaneId, SeqId};
use core_model::time::Tick;
use std::sync::Arc;
use std::time::Instant;
use vizdaw_ffi::events::spec;
use vizdaw_ffi::{EventKindRow, Session, StripKindRow, VizError};

const BAR: i64 = 4 * Q;

/// One lane holding every event kind, plus "Sub" (one bar) to reference.
fn every_kind() -> (Document, SeqId, LaneId) {
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let other = b.dest("Other", "builtin:synth", 3).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddMedia {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: None,
        }))
        .unwrap();
    let media = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .unwrap();
    let (sub, sub_lane) = b.sequence("Sub", "L", Some(piano)).unwrap();
    b.notes(sub, sub_lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let (seq, lane) = b.sequence("All", "Everything", Some(piano)).unwrap();
    let mut note = EventSpec::new(
        Tick(0),
        EventBody::Note {
            pitch: 61,
            velocity: 0x9000,
            release_velocity: 0x1234,
            duration: Tick(Q),
            attribute: Some(NoteAttribute {
                kind: 3,
                data: 0x0102,
            }),
            per_note: [PerNoteCtl {
                kind: PerNoteKind::Registered(7),
                offset: Tick(0),
                value: 0x4000_0000,
            }]
            .into_iter()
            .collect(),
        },
    );
    note.dest = Some(other);
    note.group_channel = Some((0, 5));
    let mut cc = EventSpec::new(
        Tick(100),
        EventBody::Cc {
            index: 74,
            value: 0x8000_0001,
        },
    );
    cc.mute = true;
    let specs = vec![
        note,
        cc,
        EventSpec::new(Tick(200), EventBody::PitchBend { value: 0x9000_0000 }),
        EventSpec::new(Tick(300), EventBody::ChannelPressure { value: 7 }),
        EventSpec::new(
            Tick(400),
            EventBody::PolyPressure {
                pitch: 64,
                value: 9,
            },
        ),
        EventSpec::new(
            Tick(500),
            EventBody::Program {
                program: 33,
                bank: Some((0, 5)),
            },
        ),
        EventSpec::new(
            Tick(600),
            EventBody::SysEx {
                data: Arc::from(&[0x7E_u8, 0x80, 0x01][..]),
                eight_bit: true,
            },
        ),
        EventSpec::new(
            Tick(700),
            EventBody::Text {
                kind: TextKind::Cue,
                text: "verse".into(),
            },
        ),
        EventSpec {
            mute: true,
            ..EventSpec::new(
                Tick(BAR),
                EventBody::Reference {
                    seq: sub,
                    length: None,
                    loop_count: LoopCount::Count(3),
                    transpose: -2,
                },
            )
        },
        EventSpec::new(
            Tick(2 * BAR),
            EventBody::AudioRegion {
                media,
                source_offset: 10,
                length: 48_000,
                gain_db: -3.5,
                fade_in: Fade {
                    length: Tick(Q / 4),
                    shape: FadeShape::EqualPower,
                },
                fade_out: Fade {
                    length: Tick(Q / 8),
                    shape: FadeShape::SCurve,
                },
            },
        ),
    ];
    b.events(seq, lane, specs).unwrap();
    (b.finish(), seq, lane)
}

#[test]
fn every_kind_round_trips_through_a_row() {
    let (doc, seq, lane) = every_kind();
    let originals: Vec<EventSpec> = doc
        .lane(seq, lane)
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .map(|e| e.spec())
        .collect();
    let s = Session::from_document(doc).unwrap();
    let rows = s.lane_events(seq.0, lane.0).unwrap().events;
    assert_eq!(rows.len(), 10);
    let kinds: Vec<EventKindRow> = rows.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        vec![
            EventKindRow::Note,
            EventKindRow::Cc,
            EventKindRow::PitchBend,
            EventKindRow::ChannelPressure,
            EventKindRow::PolyPressure,
            EventKindRow::Program,
            EventKindRow::SysEx,
            EventKindRow::Text,
            EventKindRow::Reference,
            EventKindRow::AudioRegion,
        ]
    );
    for (row, original) in rows.iter().zip(&originals) {
        assert_eq!(&spec(row).unwrap(), original, "{:?}", row.kind);
    }
    let reference = &rows[8];
    assert_eq!(
        reference.duration,
        3 * BAR,
        "span: Sub is one bar, three loops"
    );
    assert_eq!(reference.ref_loops, 3);
    let region = &rows[9];
    assert_eq!(region.duration, 2 * Q, "one second at 120 BPM");
    assert_eq!(rows[5].value2, 5, "bank (0, 5) is 0 × 128 + 5");
}

#[test]
fn rows_are_in_time_order_and_carry_the_lane_revision() {
    let s = Session::new_demo().unwrap();
    let snap = s.snapshot().unwrap();
    let song = snap.sequences.iter().find(|r| r.name == "Song").unwrap().id;
    let chords = s.lanes(song).unwrap()[1].id;
    let lane = s.lane_events(song, chords).unwrap();
    assert!(lane.events.windows(2).all(|w| w[0].tick <= w[1].tick));
    let rev = snap
        .lane_revs
        .iter()
        .find(|r| r.seq == song && r.lane == chords)
        .unwrap()
        .rev;
    assert_eq!(lane.rev, rev);
    assert_eq!((lane.seq, lane.lane), (song, chords));
}

#[test]
fn unknown_lanes_are_model_errors() {
    let s = Session::new_demo().unwrap();
    let song = s.snapshot().unwrap().sequences[0].id;
    assert!(matches!(
        s.lane_events(song, 9_999),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        s.sequence_info(9_999),
        Err(VizError::Model { .. })
    ));
}

#[test]
fn out_of_range_row_values_are_refused() {
    let mut row = vizdaw_ffi::EventRow::blank(0, 0, EventKindRow::Cc);
    row.value2 = 300;
    assert!(matches!(spec(&row), Err(VizError::Model { .. })));
    let mut row = vizdaw_ffi::EventRow::blank(0, 0, EventKindRow::Reference);
    row.ref_seq = None;
    assert!(matches!(spec(&row), Err(VizError::Model { .. })));
}

#[test]
fn sequence_info_reads_tempo_meter_and_length() {
    let s = Session::new_demo().unwrap();
    let snap = s.snapshot().unwrap();
    let song = snap.sequences.iter().find(|r| r.name == "Song").unwrap();
    let info = s.sequence_info(song.id).unwrap();
    assert_eq!(info.name, "Song");
    assert_eq!(info.length_ticks, song.length_ticks);
    assert_eq!(info.length_ticks, 12 * BAR);
    assert_eq!(info.tempo.first().unwrap().bpm, 120.0);
    assert_eq!(info.tempo.last().unwrap().bpm, 90.0);
    assert_eq!(info.meter.len(), 1);
    assert_eq!((info.meter[0].num, info.meter[0].den), (4, 4));
    assert!(!info.generated);
}

#[test]
fn lane_events_of_100_000_events_is_fast() {
    // Spec §3.1: under 50 ms in a release build; a generous bound in debug
    // builds (Plan Ruling 21; CI also runs this test with --release).
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Big", "Notes", Some(piano)).unwrap();
    let specs: Vec<EventSpec> = (0..100_000_i64)
        .map(|i| {
            EventSpec::new(
                Tick(i * Q / 8),
                EventBody::note(36 + (i % 60) as u8, 0x8000, Tick(Q / 8)),
            )
        })
        .collect();
    b.events(seq, lane, specs).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let start = Instant::now();
    let lane = s.lane_events(seq.0, lane.0).unwrap();
    let took = start.elapsed();
    assert_eq!(lane.events.len(), 100_000);
    let bound_ms = if cfg!(debug_assertions) { 1_000 } else { 50 };
    assert!(
        took.as_millis() < bound_ms,
        "lane_events took {took:?} (bound {bound_ms} ms)"
    );
}

#[test]
fn the_demo_with_audio_has_a_loop_lane_with_two_regions() {
    const BAR: i64 = 4 * 15_360;
    let s = vizdaw_ffi::Session::new_demo_audio().unwrap();
    let snap = s.snapshot().unwrap();
    let song = snap.sequences.iter().find(|r| r.name == "Song").unwrap().id;
    let lane = s
        .lanes(song)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Loop")
        .unwrap();
    let output = lane.audio_out.expect("routed to an output path");
    let regions: Vec<(i64, i64)> = s
        .lane_events(song, lane.id)
        .unwrap()
        .events
        .iter()
        .filter(|r| r.kind == EventKindRow::AudioRegion)
        .map(|r| (r.tick, r.duration))
        .collect();
    assert_eq!(regions, vec![(0, 2 * BAR), (4 * BAR, 2 * BAR)]);
    let strips = s.mixer().unwrap().strips;
    assert!(strips.iter().any(
        |st| matches!(st.kind, StripKindRow::AudioPath { path } if path == output)
            && st.name == "Audio 1-2"
    ));
}
