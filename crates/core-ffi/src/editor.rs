//! Plugin editors in the app's windows (spec §9): the app hands over the
//! `NSView` a plugin's own view goes into, as an address. The only
//! `unsafe` of core-ffi is here.

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

/// The parent view at address `view` (an `NSView *` from the app).
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

#[cfg(not(target_os = "macos"))]
fn on_main_thread() -> bool {
    true
}
