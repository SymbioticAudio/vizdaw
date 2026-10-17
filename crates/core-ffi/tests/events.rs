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
        EventSpec::new(
            Tick(3 * BAR),
            EventBody::Automation {
                target: core_model::automation::AutomationTarget::SendLevel(
                    core_model::ids::StripId(2),
                    core_model::automation::SendIndex(1),
                ),
                value: 0.375,
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
    assert_eq!(rows.len(), 11);
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
            EventKindRow::Automation,
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

#[test]
fn seconds_and_ticks_follow_the_sequence_tempo() {
    let mut b = Builder::new();
    let (seq, _) = b.sequence("S", "L", None).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let bpm = s.sequence_info(seq.0).unwrap().tempo[0].bpm;
    let spq = 60.0 / bpm;
    let secs = s.seconds_at(seq.0, vec![0, Q, -Q]).unwrap();
    assert_eq!(secs[0], 0.0);
    assert!((secs[1] - spq).abs() < 1e-9, "{secs:?}");
    assert!((secs[2] + spq).abs() < 1e-9, "{secs:?}");
    assert_eq!(
        s.ticks_at(seq.0, vec![spq, 2.0 * spq]).unwrap(),
        vec![Q, 2 * Q]
    );
    assert!(matches!(
        s.seconds_at(9_999, vec![0]),
        Err(VizError::Model { .. })
    ));
}

/// 4a ledger T2: the variants `every_kind` does not cover.
#[test]
fn the_other_variants_round_trip_too() {
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddMedia {
            path: "Media/b.wav".into(),
            sample_rate: 48_000,
            channels: 1,
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
    let (seq, lane) = b.sequence("Other", "Variants", Some(piano)).unwrap();
    let mut specs = vec![
        EventSpec::new(
            Tick(0),
            EventBody::Note {
                pitch: 50,
                velocity: 1,
                release_velocity: 2,
                duration: Tick(Q),
                attribute: None,
                per_note: [
                    PerNoteCtl {
                        kind: PerNoteKind::Assignable(3),
                        offset: Tick(5),
                        value: 9,
                    },
                    PerNoteCtl {
                        kind: PerNoteKind::PitchBend,
                        offset: Tick(6),
                        value: 0x8000_0000,
                    },
                ]
                .into_iter()
                .collect(),
            },
        ),
        EventSpec::new(
            Tick(10),
            EventBody::Program {
                program: 5,
                bank: None,
            },
        ),
        EventSpec::new(
            Tick(20),
            EventBody::SysEx {
                data: Arc::from(&[0x7E_u8, 0x7F][..]),
                eight_bit: false,
            },
        ),
    ];
    for (i, kind) in [
        TextKind::Text,
        TextKind::Lyric,
        TextKind::Marker,
        TextKind::Copyright,
    ]
    .into_iter()
    .enumerate()
    {
        specs.push(EventSpec::new(
            Tick(30 + i as i64),
            EventBody::Text {
                kind,
                text: format!("t{i}"),
            },
        ));
    }
    specs.push(EventSpec::new(
        Tick(BAR),
        EventBody::Reference {
            seq: sub,
            length: Some(Tick(Q)),
            loop_count: LoopCount::Fill,
            transpose: 0,
        },
    ));
    specs.push(EventSpec::new(
        Tick(2 * BAR),
        EventBody::AudioRegion {
            media,
            source_offset: 0,
            length: 48_000,
            gain_db: 0.0,
            fade_in: Fade {
                length: Tick(Q / 2),
                shape: FadeShape::Linear,
            },
            fade_out: Fade {
                length: Tick(Q / 2),
                shape: FadeShape::Exponential,
            },
        },
    ));
    b.events(seq, lane, specs).unwrap();
    let doc = b.finish();
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
    assert_eq!(rows.len(), originals.len());
    for (row, original) in rows.iter().zip(&originals) {
        assert_eq!(&spec(row).unwrap(), original, "{:?}", row.kind);
    }
    assert_eq!(rows[1].value2, -1, "no bank");
    let reference = rows
        .iter()
        .find(|r| r.kind == EventKindRow::Reference)
        .unwrap();
    assert_eq!((reference.ref_loops, reference.ref_length), (0, Some(Q)));
}
use core_engine::fixtures::Builder as AutoBuilder;
use vizdaw_ffi::{AutomationRow, AutomationTargetRow};

#[test]
fn automation_rows_round_trip_and_bad_ones_are_refused() {
    let mut b = AutoBuilder::new();
    let (seq, lane) = b.sequence("S", "Auto", None).unwrap();
    let s = vizdaw_ffi::Session::from_document(b.finish()).unwrap();
    let insert = |row: vizdaw_ffi::EventRow| vizdaw_ffi::EditCommand::Event {
        edit: vizdaw_ffi::EventEdit::Insert {
            seq: seq.0,
            lane: lane.0,
            events: vec![row],
        },
    };
    let a = AutomationRow {
        target: AutomationTargetRow::SendLevel { strip: 4, send: 1 },
        value: 0.25,
    };
    let mut r = vizdaw_ffi::EventRow::blank(0, 960, vizdaw_ffi::EventKindRow::Automation);
    r.automation = Some(a);
    s.edit(insert(r), None).unwrap();
    let rows = s.lane_events(seq.0, lane.0).unwrap().events;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, vizdaw_ffi::EventKindRow::Automation);
    assert_eq!(rows[0].tick, 960);
    assert_eq!(rows[0].automation, Some(a));
    assert_eq!(
        rows[0].value,
        i64::from(core_model::automation::to_u32(0.25))
    );
    // Every target kind survives the trip.
    for target in [
        AutomationTargetRow::StripFader { strip: 1 },
        AutomationTargetRow::StripPan { strip: 1 },
        AutomationTargetRow::StripMute { strip: 1 },
        AutomationTargetRow::StripSolo { strip: 1 },
        AutomationTargetRow::PluginParam {
            slot: 9,
            param: 1 << 40,
        },
        AutomationTargetRow::VelocityScale { dest: 2 },
    ] {
        let mut r = vizdaw_ffi::EventRow::blank(0, 0, vizdaw_ffi::EventKindRow::Automation);
        r.automation = Some(AutomationRow { target, value: 1.0 });
        let spec = vizdaw_ffi::events::spec(&r).unwrap();
        let back = match spec.body {
            core_model::event::EventBody::Automation { target, value } => {
                (vizdaw_ffi::events::automation_target_row(target), value)
            }
            b => panic!("{b:?}"),
        };
        assert_eq!(back, (target, 1.0));
    }
    // No automation record, or a value outside 0–1: refused, no undo step.
    let before = s.snapshot().unwrap().undo_label;
    let bare = vizdaw_ffi::EventRow::blank(0, 0, vizdaw_ffi::EventKindRow::Automation);
    assert!(matches!(
        s.edit(insert(bare), None),
        Err(vizdaw_ffi::VizError::Model { .. })
    ));
    let mut high = vizdaw_ffi::EventRow::blank(0, 0, vizdaw_ffi::EventKindRow::Automation);
    high.automation = Some(AutomationRow {
        target: AutomationTargetRow::StripFader { strip: 1 },
        value: 1.5,
    });
    assert!(s.edit(insert(high), None).is_err());
    assert_eq!(s.snapshot().unwrap().undo_label, before);
}

#[test]
fn the_automation_condition_selects_one_target() {
    let mut b = AutoBuilder::new();
    let (seq, lane) = b.sequence("S", "Auto", None).unwrap();
    let fader = core_model::automation::AutomationTarget::StripFader(core_model::ids::StripId(1));
    let pan = core_model::automation::AutomationTarget::StripPan(core_model::ids::StripId(1));
    b.events(
        seq,
        lane,
        vec![
            EventSpec::new(
                Tick(0),
                EventBody::Automation {
                    target: fader,
                    value: 0.1,
                },
            ),
            EventSpec::new(
                Tick(10),
                EventBody::Automation {
                    target: pan,
                    value: 0.2,
                },
            ),
        ],
    )
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.set_selection(vizdaw_ffi::SelectionRow {
        time_range: None,
        lanes: vec![vizdaw_ffi::LaneRef {
            seq: seq.0,
            lane: lane.0,
        }],
        events: vec![],
    })
    .unwrap();
    let rule = vizdaw_ffi::RuleRow {
        conditions: vec![vizdaw_ffi::ConditionRow {
            test: vizdaw_ffi::ConditionTest::Automation {
                target: AutomationTargetRow::StripPan { strip: 1 },
            },
            negate: false,
        }],
        match_all: true,
    };
    s.edit(
        vizdaw_ffi::EditCommand::SelectAndModify {
            rule: Some(rule.clone()),
            mode: vizdaw_ffi::SelectModeRow::Select,
            modify: None,
            to_copy: false,
        },
        None,
    )
    .unwrap();
    let rows = s.lane_events(seq.0, lane.0).unwrap().events;
    assert_eq!(s.selection().unwrap().events, vec![rows[1].id]);
    // The condition reads back from the core rule unchanged.
    let core = vizdaw_ffi::modify::rule(&rule).unwrap();
    assert_eq!(vizdaw_ffi::modify::rule_row(&core), Some(rule));
}
