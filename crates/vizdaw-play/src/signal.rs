//! Ctrl-C: a `SIGINT` handler that only sets a flag, polled by the main loop.

use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigint(_: i32) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

unsafe extern "C" {
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
}

const SIGINT: i32 = 2;

/// Installs the handler.
pub fn install() {
    // SAFETY: `on_sigint` is async-signal-safe (one relaxed atomic store);
    // `signal(2)` installs it with BSD (sigaction) semantics on macOS.
    unsafe {
        signal(SIGINT, on_sigint);
    }
}

/// True once Ctrl-C was pressed.
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}
