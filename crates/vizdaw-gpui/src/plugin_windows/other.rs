//! Platforms without a plug-in editor host yet: every editor window shows
//! its parameters.

use gpui::{App, NativeView, Window};

use super::HostError;

pub struct EditorHost;

pub struct Parked;

impl EditorHost {
    pub fn create(_window: &mut Window, _cx: &mut App) -> Result<EditorHost, HostError> {
        Err(HostError::Unsupported)
    }

    pub fn parent(&self) -> u64 {
        0
    }

    pub fn native_view(&self) -> Option<&NativeView> {
        None
    }

    pub fn set_frame(&self, _window: &Window, _header: f64, _editor: (f64, f64)) {}

    pub fn park(self) -> Parked {
        Parked
    }

    pub fn destroy(self) {}
}

impl Parked {
    pub fn destroy(self) {}
}

pub fn set_resizable(_window: &Window, _resizable: bool, _content: (f64, f64)) {}

pub fn window_id(_window: &Window) -> Option<u64> {
    None
}

pub fn set_transient_for(_window: &Window, _owner: u64) {}
