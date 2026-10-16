use core_engine::{BackendError, EngineError};
use core_model::ModelError;
use vizdaw_ffi::{VizError, debug_panic, ffi_guard};

#[test]
fn model_errors_map_to_model_except_io() {
    let e: VizError = ModelError::invalid("nothing to undo").into();
    assert_eq!(
        e,
        VizError::Model {
            message: "invalid: nothing to undo".into()
        }
    );
    let e: VizError = ModelError::Io("disk full".into()).into();
    assert_eq!(
        e,
        VizError::Io {
            message: "disk full".into()
        }
    );
    let e: VizError = ModelError::NewerFormat { found: 99 }.into();
    assert!(matches!(e, VizError::Model { message } if message.contains("99")));
}

#[test]
fn engine_errors_map_to_engine_except_no_device() {
    let e: VizError = EngineError::Busy.into();
    assert!(matches!(e, VizError::Engine { .. }));
    let e: VizError = EngineError::Backend(BackendError::NoDevice).into();
    assert!(matches!(e, VizError::NoDevice { .. }));
    let e: VizError = BackendError::ComponentMissing.into();
    assert!(matches!(e, VizError::Engine { .. }));
}

#[test]
fn guard_passes_values_and_errors_through() {
    assert_eq!(ffi_guard(|| Ok(7)), Ok(7));
    let e = ffi_guard(|| -> Result<(), VizError> {
        Err(VizError::Io {
            message: "x".into(),
        })
    });
    assert_eq!(
        e,
        Err(VizError::Io {
            message: "x".into()
        })
    );
}

#[test]
fn debug_panic_returns_internal_and_does_not_abort() {
    let e = debug_panic().unwrap_err();
    assert_eq!(
        e,
        VizError::Internal {
            message: "debug_panic called".into()
        }
    );
    // The process is still alive and the guard still works.
    assert_eq!(ffi_guard(|| Ok(1)), Ok(1));
}
