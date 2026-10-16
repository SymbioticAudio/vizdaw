//! The CLAP host's `unsafe` calls: loading a bundle and handing a plugin
//! its parent view.

use crate::error::HostError;
use clack_extensions::gui::{PluginGui, Window};
use clack_host::prelude::*;
use std::ffi::c_void;
use std::path::Path;

/// Loads (or reuses) the entry of the bundle at `path`.
pub(crate) fn load_entry(path: &Path) -> Result<PluginEntry, HostError> {
    // SAFETY: loading a plugin binary runs its initializers; plugins are
    // untrusted code by design (spec §2), which is why the scanner loads
    // unknown bundles in a separate process first. clack keeps the library
    // loaded while the entry or any instance lives.
    unsafe { PluginEntry::load(path) }
        .map_err(|e| HostError::LoadFailed(format!("{}: {e}", path.display())))
}

/// Embeds the plugin's GUI in the `NSView` at `parent`.
pub(crate) fn set_cocoa_parent(
    gui: &PluginGui,
    plugin: &PluginMainThreadHandle,
    parent: *mut c_void,
) -> Result<(), HostError> {
    if parent.is_null() {
        return Err(HostError::EditorUnavailable("no parent view".into()));
    }
    // SAFETY: the caller (the app, through `PluginEditor::open`) passes a
    // live `NSView *` that outlives the editor; the plugin only adds a
    // subview to it.
    let window = unsafe { Window::from_cocoa_nsview(parent) };
    // SAFETY: as above; the window handle is valid for this call.
    unsafe { gui.set_parent(plugin, window) }
        .map_err(|e| HostError::EditorUnavailable(format!("{e:?}")))
}
