//! The panic guard and poison-tolerant locking.

use crate::error::VizError;
use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Runs `f`, turning a panic into `VizError::Internal` (logged to stderr).
pub fn ffi_guard<T>(f: impl FnOnce() -> Result<T, VizError>) -> Result<T, VizError> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            eprintln!("vizdaw-ffi: caught panic: {message}");
            Err(VizError::Internal { message })
        }
    }
}

/// The text of a panic payload.
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Locks `m`, recovering a poisoned mutex (2a documents are replaced in
/// one assignment, so a panic never leaves a half-edited document).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
