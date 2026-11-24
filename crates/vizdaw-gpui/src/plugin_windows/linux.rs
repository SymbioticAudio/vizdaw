//! The Linux editor host (X11, or XWayland: ruling D2 runs the app as X11
//! whenever `DISPLAY` is set). The plug-in window is a GPUI window; the
//! editor's parent is a container X window we make inside it, at (0,
//! header × scale), editor points × scale in size. The container is made
//! unmapped and mapped only while an editor is open: an X child is drawn
//! over its parent's content and takes the pointer, so a mapped one would
//! hide the parameter list, "Loading…" and the problem text under a black
//! box (final review I1). Close order: the
//! editor closes, then the container is destroyed, then GPUI destroys its
//! window, so no plug-in ever draws into a destroyed parent (the
//! BadWindow exit). A close that meets `Busy` reparents the container to
//! the root and unmaps it: it survives GPUI's `DestroyWindow` (which
//! destroys children) until the retried close succeeds.
//!
//! The window is `WM_TRANSIENT_FOR` its document's window (ruling D1),
//! set by us, never GPUI's focused-window guess.

use gpui::{App, NativeView, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::HostError;
use super::x11;

/// The X11 id of a GPUI window; `Unsupported` without one (the test
/// platform, a Wayland window).
fn x11_id(window: &Window) -> Result<u32, HostError> {
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Xcb(h)) => Ok(h.window.get()),
        Ok(RawWindowHandle::Xlib(h)) => {
            u32::try_from(h.window).map_err(|_| HostError::Failed("bad X11 window id".into()))
        }
        Ok(_) => Err(HostError::Failed(
            "plug-in editors need X11 (run under X11 or XWayland)".into(),
        )),
        Err(_) => Err(HostError::Unsupported),
    }
}

fn px(points: f64, scale: f64) -> i32 {
    (points * scale).round() as i32
}

/// An open plug-in window's container X window.
pub struct EditorHost {
    container: u32,
}

/// A container whose editor could not detach yet (`Busy`): under the
/// root, unmapped, alive until the retried close succeeds.
pub struct Parked {
    container: u32,
}

impl EditorHost {
    /// A hidden (unmapped) container under the header of `window`,
    /// filling the rest of it until the editor's size is known.
    pub fn create(window: &mut Window, _cx: &mut App) -> Result<EditorHost, HostError> {
        let parent = x11_id(window)?;
        let scale = f64::from(window.scale_factor());
        let view = window.viewport_size();
        let header = vizdaw_app::plugin_window::HEADER_HEIGHT;
        let width = px(f64::from(f32::from(view.width)), scale);
        let height = px(f64::from(f32::from(view.height)) - header, scale);
        let container = x11::create_container(
            parent,
            0,
            i16::try_from(px(header, scale)).unwrap_or(i16::MAX),
            u16::try_from(width.max(1)).unwrap_or(u16::MAX),
            u16::try_from(height.max(1)).unwrap_or(u16::MAX),
        )
        .map_err(HostError::Failed)?;
        Ok(EditorHost { container })
    }

    /// The X window the plug-in's window is made inside.
    pub fn parent(&self) -> u64 {
        u64::from(self.container)
    }

    /// No GPUI element: the container is an X child over the window.
    pub fn native_view(&self) -> Option<&NativeView> {
        None
    }

    /// Shows (maps) the container at (0, `header` points) at the editor's
    /// size in points, in `window`'s device pixels: an editor is open.
    pub fn show(&self, window: &Window, header: f64, editor: (f64, f64)) {
        let scale = f64::from(window.scale_factor());
        let (w, h) = (px(editor.0, scale), px(editor.1, scale));
        if let Err(e) = x11::show(
            self.container,
            0,
            px(header, scale),
            u32::try_from(w.max(1)).unwrap_or(1),
            u32::try_from(h.max(1)).unwrap_or(1),
        ) {
            eprintln!("plug-in window: the container could not show: {e}");
        }
    }

    /// Hides (unmaps) the container: no editor is open, and what GPUI
    /// draws under it (the parameter list, the loading or problem text)
    /// shows and takes the clicks.
    pub fn hide(&self) {
        if let Err(e) = x11::hide(self.container) {
            eprintln!("plug-in window: the container could not hide: {e}");
        }
    }

    /// The editor could not detach: the container goes to the root,
    /// unmapped, so GPUI destroying its window leaves it alive.
    pub fn park(self) -> Parked {
        if let Err(e) = x11::park(self.container) {
            eprintln!("plug-in window: the container could not be parked: {e}");
        }
        Parked {
            container: self.container,
        }
    }

    /// The editor is closed: the container goes.
    pub fn destroy(self) {
        if let Err(e) = x11::destroy(self.container) {
            eprintln!("plug-in window: the container could not be destroyed: {e}");
        }
    }
}

impl Parked {
    /// The retried close succeeded: the container goes.
    pub fn destroy(self) {
        if let Err(e) = x11::destroy(self.container) {
            eprintln!("plug-in window: the container could not be destroyed: {e}");
        }
    }
}

/// Lets the user resize `window` only while `resizable` (`WM_NORMAL_HINTS`;
/// a fixed window's minimum and maximum are its `content` size in points).
pub fn set_resizable(window: &Window, resizable: bool, content: (f64, f64)) {
    let Ok(id) = x11_id(window) else {
        return;
    };
    let scale = f64::from(window.scale_factor());
    let (mw, mh) = vizdaw_app::plugin_window::MIN_CONTENT;
    let size = (px(content.0, scale), px(content.1, scale));
    let min = (px(mw, scale), px(mh, scale));
    if let Err(e) = x11::set_size_hints(id, resizable, size, min) {
        eprintln!("plug-in window: the size hints could not be set: {e}");
    }
}

/// The X11 id of `window` (to name it as a plug-in window's owner).
pub fn window_id(window: &Window) -> Option<u64> {
    x11_id(window).ok().map(u64::from)
}

/// `window` is `WM_TRANSIENT_FOR` the window `owner` (its document's,
/// ruling D1): the window manager keeps it above its document's window
/// and minimizes it with it.
pub fn set_transient_for(window: &Window, owner: u64) {
    let (Ok(id), Ok(owner)) = (x11_id(window), u32::try_from(owner)) else {
        return;
    };
    if let Err(e) = x11::set_transient_for(id, owner) {
        eprintln!("plug-in window: WM_TRANSIENT_FOR could not be set: {e}");
    }
}
