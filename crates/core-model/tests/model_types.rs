use core_model::document::{
    DestKind, Destination, Document, DocumentRepr, MediaItem, MediaOrigin, OutputPath, Settings,
};
use core_model::event::{
    EventBody, EventKind, EventSpec, Fade, FadeShape, LoopCount, NoteAttribute, PerNoteCtl,
    PerNoteKind, TextKind,
};
use core_model::ids::{Counters, DestId, EventId, LaneId, MediaId, OutputPathId, SeqId, TakeId};
use core_model::lane::{DrumRow, Lane, QuantizeSpec};
use core_model::sequence::{GeneratorParams, SeqKind, Sequence};
use core_model::time::{PPQ, Tick};
use im::Vector;
use std::sync::Arc;

fn bodies() -> Vec<EventBody> {
    vec![
        EventBody::Note {
            pitch: 60,
            velocity: 40_000,
            release_velocity: 100,
            duration: Tick(PPQ),
            attribute: Some(NoteAttribute { kind: 3, data: 512 }),
            per_note: Vector::unit(PerNoteCtl {
                kind: PerNoteKind::PitchBend,
                offset: Tick(10),
                value: 7,
            }),
        },
        EventBody::Cc {
            index: 7,
            value: u32::MAX,
        },
        EventBody::PitchBend { value: 0x8000_0000 },
        EventBody::ChannelPressure { value: 1 },
        EventBody::PolyPressure {
            pitch: 61,
            value: 2,
        },
        EventBody::Program {
            program: 5,
            bank: Some((1, 2)),
        },
        EventBody::SysEx {
            data: Arc::from(vec![0x7e_u8, 0xff, 0x00]),
            eight_bit: true,
        },
        EventBody::Text {
            kind: TextKind::Lyric,
            text: "la".into(),
        },
        EventBody::Reference {
            seq: SeqId(1),
            length: Some(Tick(PPQ)),
            loop_count: LoopCount::Fill,
            transpose: -3,
        },
        EventBody::AudioRegion {
            media: MediaId(0),
            source_offset: 10,
            length: 4800,
            gain_db: -6.0,
            fade_in: Fade::default(),
            fade_out: Fade {
                length_frames: 100,
                shape: FadeShape::EqualPower,
            },
        },
    ]
}

/// Builds a document by hand: seq 0 holds one lane with every event kind
/// (event ids assigned in reverse so array order is by tick, not id).
fn sample() -> Document {
    let mut lane = Lane::new(LaneId(0), TakeId(0), "Lane");
    lane.midi_dest = Some(DestId(0));
    lane.audio_out = Some(OutputPathId(0));
    let n = bodies().len() as u64;
    let take = lane.takes.get_mut(0).unwrap();
    for (i, b) in bodies().into_iter().enumerate() {
        let ev = EventSpec::new(Tick(i as i64 * 10), b).with_id(EventId(n - 1 - i as u64));
        take.events.insert(ev.key(), ev);
    }
    let mut top = Sequence::new(SeqId(0), "Song", Vector::unit(lane));
    top.kind = SeqKind::Generated {
        generator: GeneratorParams {
            generator: "arp".into(),
            params: [("rate".to_string(), "1/16".to_string())]
                .into_iter()
                .collect(),
        },
    };
    let sub = Sequence::new(
        SeqId(1),
        "Verse",
        Vector::unit(Lane::new(LaneId(1), TakeId(1), "L")),
    );
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![Destination {
            id: DestId(0),
            name: "Synth".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "ep".into(),
                group: 1,
                channel: 9,
            },
            default_patch: None,
        }],
        outputs: vec![OutputPath {
            id: OutputPathId(0),
            name: "Main".into(),
            reference: "bus:0".into(),
        }],
        media: vec![MediaItem {
            id: MediaId(0),
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 48_000,
            content_hash: None,
            name: "a".into(),
            origin: MediaOrigin::default(),
        }],
        sequences: vec![top, sub],
        mixer: None,
        counters: Counters {
            strip: 0,
            plugin: 0,
            seq: 2,
            lane: 2,
            event: n,
            dest: 1,
            output: 1,
            media: 1,
            take: 2,
            drum_row: 0,
        },
    })
    .unwrap()
}

// Review focus #4: JSON round-trip of im collections and Arc<[u8]>.
#[test]
fn json_round_trip_preserves_every_event_kind() {
    let doc = sample();
    let json = serde_json::to_string(&doc).unwrap();
    let back: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(back, doc);
    let (ev, _) = back.event(EventId(3)).unwrap();
    assert_eq!(
        ev.body,
        EventBody::SysEx {
            data: Arc::from(vec![0x7e_u8, 0xff, 0x00]),
            eight_bit: true
        }
    );
}

#[test]
fn events_serialize_as_array_ordered_by_tick() {
    let v = serde_json::to_value(sample()).unwrap();
    let events = &v["sequences"][0]["lanes"][0]["takes"][0]["events"];
    let ids: Vec<u64> = events
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![9, 8, 7, 6, 5, 4, 3, 2, 1, 0]);
    assert_eq!(events[0]["body"]["type"], "note");
    assert!(v["sequences"][0]["id"].is_u64());
}

#[test]
fn index_locates_events() {
    let doc = sample();
    let (ev, loc) = doc.event(EventId(9)).unwrap();
    assert_eq!(ev.body.kind(), EventKind::Note);
    assert_eq!(
        (loc.seq, loc.lane, loc.take),
        (SeqId(0), LaneId(0), TakeId(0))
    );
    assert!(doc.event(EventId(99)).is_err());
    assert_eq!(doc.all_events().count(), 10);
}

#[test]
fn unknown_fields_are_rejected() {
    let mut v = serde_json::to_value(sample()).unwrap();
    v["sequences"][0]["lanes"][0]["bogus"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Document>(v).is_err());
    let mut v = serde_json::to_value(sample()).unwrap();
    v["sequences"][0]["lanes"][0]["takes"][0]["events"][0]["bogus"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Document>(v).is_err());
}

#[test]
fn duplicate_ids_are_rejected() {
    let mut v = serde_json::to_value(sample()).unwrap();
    let seqs = v["sequences"].as_array_mut().unwrap();
    let dup = seqs[0].clone();
    seqs.push(dup);
    assert!(serde_json::from_value::<Document>(v).is_err());

    let mut v = serde_json::to_value(sample()).unwrap();
    v["sequences"][1]["lanes"][0]["takes"][0]["events"] =
        v["sequences"][0]["lanes"][0]["takes"][0]["events"].clone();
    assert!(serde_json::from_value::<Document>(v).is_err());
}

#[test]
fn quantize_spec_applies_strength() {
    let q = QuantizeSpec::grid(Tick(100), 50);
    assert_eq!(q.apply(Tick(140)), Tick(120));
    assert_eq!(q.apply(Tick(160)), Tick(180));
    let zero = QuantizeSpec::grid(Tick(0), 100);
    assert_eq!(zero.apply(Tick(7)), Tick(7));
}

#[test]
fn drum_row_matches_pitch_and_dest() {
    let row = DrumRow {
        id: core_model::ids::DrumRowId(0),
        name: "Kick".into(),
        note: 36,
        dest: None,
        show_velocity: false,
    };
    assert!(row.matches(36, None));
    assert!(!row.matches(36, Some(DestId(0))));
    assert!(!row.matches(38, None));
}

#[test]
fn empty_document_round_trips() {
    let d = Document::new();
    let back: Document = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(back, d);
    assert_eq!(d.settings().history_depth, 200);
    assert_eq!(d.settings().autosave_keep, 10);
}
