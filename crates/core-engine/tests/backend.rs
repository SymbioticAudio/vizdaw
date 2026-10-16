use core_engine::backend::{MAX_FRAMES, MidiPortInfo, StereoBuf};
use core_engine::{BackendError, EngineError};
use core_model::ModelError;

#[test]
fn stereo_buf_is_zeroed_and_clears_a_prefix() {
    let mut b = StereoBuf::new(8);
    assert_eq!(b.capacity(), 8);
    assert!(b.left.iter().chain(b.right.iter()).all(|x| *x == 0.0));
    b.left.fill(1.0);
    b.right.fill(-1.0);
    b.clear(3);
    assert_eq!(&b.left[..4], &[0.0, 0.0, 0.0, 1.0]);
    assert_eq!(&b.right[..4], &[0.0, 0.0, 0.0, -1.0]);
    b.clear(100); // more than the capacity: clamps, never panics
    assert!(b.left.iter().all(|x| *x == 0.0));
    assert_eq!(MAX_FRAMES, 4096);
}

#[test]
fn errors_read_well() {
    let os = BackendError::Os {
        call: "AudioDeviceStart",
        status: -50,
    };
    assert_eq!(os.to_string(), "AudioDeviceStart failed with status -50");
    let e: EngineError = os.clone().into();
    assert_eq!(e, EngineError::Backend(os));
    let m: EngineError = ModelError::invalid("bad").into();
    assert!(matches!(m, EngineError::Model(_)));
    assert_eq!(
        EngineError::UnknownSequence("Song".into()).to_string(),
        "no sequence named \"Song\""
    );
    assert_eq!(
        EngineError::Busy.to_string(),
        "engine command queue is full"
    );
}

#[test]
fn port_info_compares_by_name_and_id() {
    let a = MidiPortInfo {
        name: "IAC Bus 1".into(),
        unique_id: Some(7),
    };
    assert_eq!(a.clone(), a);
    assert_ne!(
        a,
        MidiPortInfo {
            name: "IAC Bus 1".into(),
            unique_id: None
        }
    );
}
