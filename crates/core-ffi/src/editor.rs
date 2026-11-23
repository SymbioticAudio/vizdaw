//! Plugin editors in the app's windows (spec §9): the app hands over the
//! view a plugin's own view goes into, as an address (an `NSView *` on
//! macOS, an X11 window id on Linux). The only `unsafe` of core-ffi is here.
//!
//! The plug-ins' main-thread run loop (CLAP timers and descriptors,
//! `core_host::run_loop`) is driven from here too: the app waits on
//! [`run_loop_fds`] or a short timeout, then calls [`run_loop_tick`], or
//! simply calls [`run_loop_pump`] from its main loop. All on the thread
//! that loads the plug-ins (the main thread; `Session::idle` loads them).

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

pub use core_host::run_loop_tick;
#[cfg(unix)]
pub use core_host::{FdFlags, run_loop_fds};

/// One pass of the plug-ins' run loop without waiting (main thread): the
/// registered descriptors that are ready now, then [`run_loop_tick`] with
/// them (due timers fire). An app that can wait on descriptors polls
/// [`run_loop_fds`] itself and calls [`run_loop_tick`] instead.
#[cfg(unix)]
pub fn run_loop_pump() {
    let fds = run_loop_fds();
    let mut polled: Vec<libc::pollfd> = fds
        .iter()
        .map(|(fd, flags)| libc::pollfd {
            fd: *fd,
            events: poll_events(*flags),
            revents: 0,
        })
        .collect();
    let mut ready = Vec::new();
    if !polled.is_empty() {
        let n = libc::nfds_t::try_from(polled.len()).unwrap_or(libc::nfds_t::MAX);
        // SAFETY: `polled` is a live array of at least `n` records, and a
        // zero timeout never blocks.
        let r = unsafe { libc::poll(polled.as_mut_ptr(), n, 0) };
        if r > 0 {
            ready.extend(
                polled
                    .iter()
                    .map(|p| (p.fd, ready_flags(p.revents)))
                    .filter(|(_, f)| !f.is_empty()),
            );
        }
    }
    run_loop_tick(std::time::Instant::now(), &ready);
}

/// One pass of the plug-ins' run loop (main thread): due timers fire.
#[cfg(not(unix))]
pub fn run_loop_pump() {
    run_loop_tick(std::time::Instant::now());
}

#[cfg(unix)]
fn poll_events(flags: FdFlags) -> std::ffi::c_short {
    let mut e = 0;
    if flags.contains(FdFlags::READ) {
        e |= libc::POLLIN;
    }
    if flags.contains(FdFlags::WRITE) {
        e |= libc::POLLOUT;
    }
    e
}

#[cfg(unix)]
fn ready_flags(revents: std::ffi::c_short) -> FdFlags {
    let mut f = FdFlags::empty();
    if revents & libc::POLLIN != 0 {
        f |= FdFlags::READ;
    }
    if revents & libc::POLLOUT != 0 {
        f |= FdFlags::WRITE;
    }
    if revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        f |= FdFlags::ERROR;
    }
    f
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn poll_results_map_to_clap_flags() {
        assert_eq!(poll_events(FdFlags::READ), libc::POLLIN);
        assert_eq!(
            poll_events(FdFlags::READ | FdFlags::WRITE | FdFlags::ERROR),
            libc::POLLIN | libc::POLLOUT
        );
        assert_eq!(ready_flags(0), FdFlags::empty());
        assert_eq!(ready_flags(libc::POLLIN), FdFlags::READ);
        assert_eq!(
            ready_flags(libc::POLLOUT | libc::POLLHUP),
            FdFlags::WRITE | FdFlags::ERROR
        );
        assert_eq!(ready_flags(libc::POLLNVAL), FdFlags::ERROR);
    }

    #[test]
    fn a_pump_with_nothing_registered_does_nothing() {
        assert!(run_loop_fds().is_empty());
        run_loop_pump();
    }
}
