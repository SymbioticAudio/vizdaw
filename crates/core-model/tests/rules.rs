use core_model::ModelError;
use core_model::document::{Document, DocumentRepr, MediaItem, Settings};
use core_model::event::{EventBody, EventSpec, LoopCount};
use core_model::ids::{Counters, DestId, EventId, LaneId, MediaId, SeqId, TakeId};
use core_model::lane::Lane;
use core_model::rules::{
    check_event, check_reference, find_cycle, path_between, references_to, validate,
};
use core_model::sequence::Sequence;
use core_model::time::Tick;
use im::Vector;
use std::sync::Arc;

/// `n` sequences (ids 0..n, one lane each) with reference events
/// `(owner, target)` placed in the owner's lane, plus one 1000-frame media item.
fn doc_with_refs(n: u64, refs: &[(u64, u64)]) -> Document {
    let mut seqs = Vec::new();
    let mut next_event = 0;
    for i in 0..n {
        let mut lane = Lane::new(LaneId(i), TakeId(i), "L");
        let take = lane.takes.get_mut(0).unwrap();
        for (o, t) in refs.iter().filter(|(o, _)| *o == i) {
            let ev = EventSpec::new(Tick(0), EventBody::reference(SeqId(*t)))
                .with_id(EventId(next_event));
            next_event += 1;
            let _ = o;
            take.events.insert(ev.key(), ev);
        }
        seqs.push(Sequence::new(SeqId(i), format!("S{i}"), Vector::unit(lane)));
    }
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![MediaItem {
            id: MediaId(0),
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 1,
            length_frames: 1000,
            content_hash: None,
        }],
        sequences: seqs,
        counters: Counters {
            seq: n,
            lane: n,
            take: n,
            event: next_event,
            media: 1,
            ..Counters::default()
        },
    })
    .unwrap()
}

#[test]
fn self_reference_is_a_cycle() {
    let d = doc_with_refs(1, &[]);
    assert_eq!(
        check_reference(&d, SeqId(0), SeqId(0)),
        Err(ModelError::Cycle {
            path: vec![SeqId(0), SeqId(0)]
        })
    );
}

#[test]
fn indirect_cycle_reports_full_path() {
    // 0 -> 1 -> 2; adding 2 -> 0 closes the loop.
    let d = doc_with_refs(3, &[(0, 1), (1, 2)]);
    assert_eq!(
        path_between(&d, SeqId(0), SeqId(2)),
        Some(vec![SeqId(0), SeqId(1), SeqId(2)])
    );
    assert_eq!(
        check_reference(&d, SeqId(2), SeqId(0)),
        Err(ModelError::Cycle {
            path: vec![SeqId(2), SeqId(0), SeqId(1), SeqId(2)]
        })
    );
    assert!(check_reference(&d, SeqId(0), SeqId(2)).is_ok());
}

#[test]
fn reference_to_missing_sequence_is_unknown() {
    let d = doc_with_refs(1, &[]);
    assert!(matches!(
        check_reference(&d, SeqId(0), SeqId(9)),
        Err(ModelError::UnknownId(_))
    ));
}

#[test]
fn references_to_lists_users() {
    let d = doc_with_refs(3, &[(0, 2), (1, 2), (0, 1)]);
    let mut users = references_to(&d, SeqId(2));
    users.sort();
    assert_eq!(users, vec![EventId(0), EventId(2)]);
}

#[test]
fn event_range_checks() {
    let d = doc_with_refs(2, &[]);
    let bad = |body| check_event(&d, SeqId(0), &EventSpec::new(Tick(0), body)).is_err();
    assert!(bad(EventBody::note(128, 1, Tick(1))));
    assert!(bad(EventBody::note(60, 1, Tick(-1))));
    assert!(bad(EventBody::Cc {
        index: 200,
        value: 0
    }));
    assert!(bad(EventBody::SysEx {
        data: Arc::from(vec![0x90_u8]),
        eight_bit: false
    }));
    assert!(bad(EventBody::Reference {
        seq: SeqId(1),
        length: Some(Tick(0)),
        loop_count: LoopCount::Count(1),
        transpose: 0
    }));
    assert!(bad(EventBody::Reference {
        seq: SeqId(1),
        length: None,
        loop_count: LoopCount::Count(0),
        transpose: 0
    }));
    assert!(bad(EventBody::AudioRegion {
        media: MediaId(0),
        source_offset: 900,
        length: 200,
        gain_db: 0.0,
        fade_in: Default::default(),
        fade_out: Default::default()
    }));
    assert!(bad(EventBody::AudioRegion {
        media: MediaId(0),
        source_offset: 0,
        length: 10,
        gain_db: f32::NAN,
        fade_in: Default::default(),
        fade_out: Default::default()
    }));
    let mut spec = EventSpec::new(Tick(0), EventBody::note(60, 1, Tick(1)));
    spec.group_channel = Some((0, 16));
    assert!(check_event(&d, SeqId(0), &spec).is_err());
    spec.group_channel = Some((15, 15));
    assert!(check_event(&d, SeqId(0), &spec).is_ok());
    spec.dest = Some(DestId(5));
    assert_eq!(
        check_event(&d, SeqId(0), &spec),
        Err(ModelError::UnknownId(DestId(5).into()))
    );
}

#[test]
fn validate_accepts_a_clean_document() {
    let d = doc_with_refs(3, &[(0, 1), (1, 2)]);
    assert!(validate(&d).is_empty(), "{:?}", validate(&d));
    assert_eq!(find_cycle(&d), None);
}

#[test]
fn validate_reports_every_problem() {
    // A cycle, plus a counter that is too low.
    let d = doc_with_refs(2, &[(0, 1), (1, 0)]);
    let mut v = serde_json::to_value(&d).unwrap();
    v["counters"]["event"] = serde_json::json!(0);
    v["sequences"][0]["tempo"]["points"][0]["bpm"] = serde_json::json!(1);
    let d: Document = serde_json::from_value(v).unwrap();
    let problems = validate(&d);
    assert!(problems.iter().any(|p| p.contains("cycle")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("counters.event")));
    assert!(problems.iter().any(|p| p.contains("tempo.bpm")));
    assert!(problems.len() >= 4);
}
