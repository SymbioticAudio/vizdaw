//! Plugin editors in the app's windows (spec §9): the app hands over the
//! view a plugin's own view goes into, as an address (an `NSView *` on
//! macOS, an X11 window id on Linux). The only `unsafe` of core-ffi is here.
//!
//! The plug-ins' main-thread run loop (CLAP timers and descriptors, VST3
//! `Linux::IRunLoop`; `core_host::run_loop`) is driven from here too: the
//! app waits on [`run_loop_fds`] or a short timeout, then calls
//! [`run_loop_tick`], or simply calls [`run_loop_pump`] from its main loop
//! (the GPUI app: every 10 ms, on macOS and Linux). All on the thread that
//! loads the plug-ins (the main thread; `Session::idle` loads them, and
//! bounce copies load there too).

use crate::error::VizError;
use core_host::ParentView;
use std::ffi::c_void;

/// An editor's size in points, and whether its window may resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct EditorSize {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// The user may resize it.
    pub resizable: bool,
}

/// An open editor asked for a new size (CLAP `request_resize`): its
/// window should follow (`IdleUpdate.editor_resize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct EditorResize {
    /// The slot whose editor it is.
    pub slot: u64,
    /// Width in points.
    pub width: u32,
    /// Height in points.
    pub height: u32,
}

/// The parent view at address `view` (an `NSView *` or an X11 window id
/// from the app).
///
/// The app's contract (Swift `PluginWindow`): the view lives until it has
/// called `close_editor` for the slot, and every call is made on the main
/// thread.
pub fn parent(view: u64) -> Result<ParentView, VizError> {
    if !on_main_thread() {
        return Err(VizError::Engine {
            message: "editor unavailable: plugin editors open on the main thread only".into(),
        });
    }
    if view == 0 {
        return Err(VizError::Engine {
            message: "editor unavailable: no parent view".into(),
        });
    }
    let ptr = usize::try_from(view).map_err(|_| VizError::Engine {
        message: "editor unavailable: bad view address".into(),
    })? as *mut c_void;
    // SAFETY: `ptr` is the address of an `NSView` the app keeps alive until
    // it closes the editor (the contract above, which is `ParentView`'s).
    Ok(unsafe { ParentView::new(ptr) })
}

#[cfg(target_os = "macos")]
fn on_main_thread() -> bool {
    unsafe extern "C" {
        fn pthread_main_np() -> std::ffi::c_int;
    }
    // SAFETY: `pthread_main_np` takes no arguments, touches no memory of
    // ours and is always callable.
    unsafe { pthread_main_np() != 0 }
}

#[cfg(target_os = "linux")]
fn on_main_thread() -> bool {
    // SAFETY: neither call takes arguments or touches memory of ours; both
    // always succeed. The main thread's id is the process id.
    unsafe { libc::gettid() == libc::getpid() }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn on_main_thread() -> bool {
    true
}

#[cfg(unix)]
pub use core_host::{FdFlags, run_loop_fds};

/// Runs `f` under the plug-in registry's busy mark (final review M1): the
/// run loop's callbacks are plug-in code, and a nested loop one runs must
/// not get an `open_editor`, `close_editor` or `idle` in.
fn under_busy_mark<T>(f: impl FnOnce() -> T) -> T {
    crate::plugins::while_busy(f)
}

/// One pass of the plug-ins' run loop without waiting
/// ([`core_host::run_loop_pump`]), under the registry's busy mark.
pub fn run_loop_pump() {
    under_busy_mark(core_host::run_loop_pump);
}

/// One tick of the plug-ins' run loop with the descriptors found ready
/// ([`core_host::run_loop_tick`]), under the registry's busy mark.
#[cfg(unix)]
pub fn run_loop_tick(now: std::time::Instant, ready: &[(std::os::fd::RawFd, FdFlags)]) {
    under_busy_mark(|| core_host::run_loop_tick(now, ready));
}

/// One tick of the plug-ins' run loop ([`core_host::run_loop_tick`]),
/// under the registry's busy mark.
#[cfg(not(unix))]
pub fn run_loop_tick(now: std::time::Instant) {
    under_busy_mark(|| core_host::run_loop_tick(now));
}

/// `open_editor`'s `Busy` message while an editor's view is still on its
/// way (an AU v3 view controller): the window retries on its tick.
pub const OPENING: &str = "editor opening";

#[cfg(feature = "test-hooks")]
static DEBUG_OPENING: std::sync::Mutex<Vec<(u64, u64, u32)>> = std::sync::Mutex::new(Vec::new());

/// `Session::debug_editor_opening`: the next `opens` opens of `slot` in
/// session `session` answer "opening".
#[cfg(feature = "test-hooks")]
pub(crate) fn debug_opening(session: u64, slot: u64, opens: u32) {
    let mut d = DEBUG_OPENING.lock().unwrap_or_else(|e| e.into_inner());
    d.retain(|(s, sl, _)| (*s, *sl) != (session, slot));
    if opens > 0 {
        d.push((session, slot, opens));
    }
}

/// Whether this open of `slot` answers "opening" (counting it down).
#[cfg(feature = "test-hooks")]
pub(crate) fn take_debug_opening(session: u64, slot: u64) -> bool {
    let mut d = DEBUG_OPENING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = d.iter_mut().find(|(s, sl, _)| (*s, *sl) == (session, slot)) else {
        return false;
    };
    e.2 = e.2.saturating_sub(1);
    d.retain(|(_, _, n)| *n > 0);
    true
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_pump_with_nothing_registered_does_nothing() {
        assert!(run_loop_fds().is_empty());
        run_loop_pump();
    }

    /// Final review M1: plug-in run-loop callbacks run under the
    /// registry's busy mark, so one that runs a nested loop cannot get a
    /// registry call in (it answers `Busy`); a pump inside a plug-in call
    /// (already busy) still runs.
    #[test]
    fn the_pump_runs_under_the_busy_mark() {
        assert!(!crate::plugins::is_busy());
        let mut seen = None;
        under_busy_mark(|| seen = Some(crate::plugins::is_busy()));
        assert_eq!(seen, Some(true));
        assert!(!crate::plugins::is_busy(), "the mark is lifted after");
        let mut nested = None;
        under_busy_mark(|| under_busy_mark(|| nested = Some(crate::plugins::is_busy())));
        assert_eq!(nested, Some(true), "a nested pump still runs");
        assert!(!crate::plugins::is_busy());
    }
}
