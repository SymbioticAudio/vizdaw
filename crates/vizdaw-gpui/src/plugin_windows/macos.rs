//! The macOS editor host: a plain container `NSView` we allocate, shown in
//! the plug-in window through a [`gpui::NativeView`] element sized to the
//! editor's points. The plug-in attaches its own view inside the container,
//! never into GPUI's view: GPUI owns the `NativeView`'s frame, and the
//! container must outlive the window for a close that meets `Busy`.
//!
//! No action is reserved on the `NativeView`: while it has the keyboard,
//! plain keys go to the plug-in and ⌘-keys reach the menu as key
//! equivalents (README "In a plugin window plain keys go to the plugin").

use std::ffi::c_void;
use std::ptr::NonNull;

use gpui::{App, NativeView, Window};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindowStyleMask};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::HostError;

/// An open plug-in window's container view.
pub struct EditorHost {
    container: Retained<NSView>,
    /// GPUI's hold on the container; dropping it takes the container out
    /// of the window.
    native: Option<NativeView>,
}

/// A container whose editor could not detach yet (`Busy`): out of every
/// window, alive until the retried close succeeds.
pub struct Parked {
    _container: Retained<NSView>,
}

/// The `NSView` GPUI draws into; the test platform has none.
fn gpui_view(window: &Window) -> Result<NonNull<c_void>, HostError> {
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(RawWindowHandle::AppKit(h)) => Ok(h.ns_view),
        Ok(_) => Err(HostError::Failed("not an AppKit window".into())),
        Err(_) => Err(HostError::Unsupported),
    }
}

impl EditorHost {
    /// A container in `window` (main thread only).
    pub fn create(window: &mut Window, cx: &mut App) -> Result<EditorHost, HostError> {
        gpui_view(window)?;
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| HostError::Failed("not on the main thread".into()))?;
        let container = NSView::new(mtm);
        let ptr = NonNull::from(&*container).cast::<c_void>();
        // SAFETY: `container` is a live `NSView` in no view hierarchy; it is
        // retained by `self` for as long as GPUI shows it, and nothing but
        // GPUI sets its frame, visibility or superview.
        let native = unsafe { NativeView::new(ptr, window, cx) }
            .map_err(|e| HostError::Failed(e.to_string()))?;
        Ok(EditorHost {
            container,
            native: Some(native),
        })
    }

    /// The address the plug-in's view attaches to.
    pub fn parent(&self) -> u64 {
        Retained::as_ptr(&self.container) as usize as u64
    }

    /// The element that shows the container.
    pub fn native_view(&self) -> Option<&NativeView> {
        self.native.as_ref()
    }

    /// The editor's area: GPUI's layout sizes the container (the element
    /// is drawn at the editor's points), so nothing to do here.
    pub fn show(&self, _window: &Window, _header: f64, _editor: (f64, f64)) {}

    /// No editor is open: GPUI hides a `NativeView` it does not paint, and
    /// the view paints it only for an open editor, so nothing to do here.
    pub fn hide(&self) {}

    /// The editor could not detach: the container leaves the window and
    /// stays alive.
    pub fn park(mut self) -> Parked {
        self.native = None;
        Parked {
            _container: self.container,
        }
    }

    /// The editor is closed: the container goes.
    pub fn destroy(self) {}
}

impl Parked {
    /// The retried close succeeded: the container goes.
    pub fn destroy(self) {}
}

/// Lets the user resize `window` only while `resizable` (the window's
/// style mask; GPUI sets it only at open).
pub fn set_resizable(window: &Window, resizable: bool, _content: (f64, f64)) {
    let Ok(view) = gpui_view(window) else {
        return;
    };
    let Some(_mtm) = MainThreadMarker::new() else {
        return;
    };
    // SAFETY: GPUI's own live view of this window, on the main thread.
    let view = unsafe { view.cast::<NSView>().as_ref() };
    let Some(w) = view.window() else {
        return;
    };
    let mut mask = w.styleMask();
    mask.set(NSWindowStyleMask::Resizable, resizable);
    if mask != w.styleMask() {
        w.setStyleMask(mask);
    }
}

/// Linux only (ruling D1: macOS editor windows are free standalone
/// windows, which can go behind their document's window).
pub fn window_id(_window: &Window) -> Option<u64> {
    None
}

/// Linux only (ruling D1).
pub fn set_transient_for(_window: &Window, _owner: u64) {}
