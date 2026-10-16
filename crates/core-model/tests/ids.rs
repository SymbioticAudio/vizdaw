use core_model::ModelError;
use core_model::ids::{AnyId, Counters, EventId, SeqId};

#[test]
fn counters_allocate_increasing_ids_per_type() {
    let mut c = Counters::default();
    assert_eq!(c.alloc_seq(), SeqId(0));
    assert_eq!(c.alloc_seq(), SeqId(1));
    assert_eq!(c.alloc_event(), EventId(0));
    assert_eq!(c.seq, 2);
    assert_eq!(c.event, 1);
}

#[test]
fn counters_saturate_instead_of_overflowing() {
    let mut c = Counters {
        event: u64::MAX,
        ..Counters::default()
    };
    assert_eq!(c.alloc_event(), EventId(u64::MAX));
    assert_eq!(c.alloc_event(), EventId(u64::MAX));
}

#[test]
fn ids_serialize_as_plain_integers() {
    assert_eq!(serde_json::to_string(&SeqId(7)).unwrap(), "7");
    let back: EventId = serde_json::from_str("42").unwrap();
    assert_eq!(back, EventId(42));
}

#[test]
fn any_id_displays_with_prefix() {
    assert_eq!(AnyId::from(SeqId(3)).to_string(), "seq3");
    let e = ModelError::UnknownId(EventId(9).into());
    assert_eq!(e.to_string(), "unknown id ev9");
}

#[test]
fn io_errors_convert() {
    let e: ModelError = std::io::Error::other("boom").into();
    assert!(matches!(e, ModelError::Io(ref m) if m.contains("boom")));
}
