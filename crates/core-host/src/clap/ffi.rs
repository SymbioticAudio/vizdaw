//! The CLAP host's `unsafe` calls: loading a bundle and handing a plugin
//! its parent view.

use crate::error::HostError;
use clack_extensions::gui::{PluginGui, Window};
use clack_host::entry::{EntryDescriptor, EntryProvider};
use clack_host::prelude::*;
use std::collections::HashMap;
use std::ffi::{CString, c_void};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Mutex;

/// The entry of a CLAP library this process opened, keyed by the binary's
/// path. A library is never closed once opened (host track H1 Task 4, as
/// for VST3 in Task 3): a plug-in may have given a thread a thread-exit
/// destructor in its own code (Rust's `std`, many C++ runtimes), and a
/// thread that exits after an unload runs code that is gone (SIGSEGV on
/// Linux). clack still runs the entry's `init` when the first handle to
/// it is made and `deinit` when the last handle and instance go (once
/// per `init`, as CLAP asks); a later load runs `init` again on the same
/// image.
static LIBRARIES: Mutex<Option<HashMap<PathBuf, KeptEntry>>> = Mutex::new(None);

/// A `clap_entry` in a library that stays loaded for the process's life.
#[derive(Clone, Copy)]
struct KeptEntry(NonNull<EntryDescriptor>);

// SAFETY: the pointer is the library's `clap_entry`, a static that never
// moves or goes (the library is never closed); CLAP entries and
// factories are thread-safe by the CLAP spec.
unsafe impl Send for KeptEntry {}
// SAFETY: as above.
unsafe impl Sync for KeptEntry {}

// SAFETY: the pointer stays valid for the process's life (the library is
// never closed), as `EntryProvider` asks.
unsafe impl EntryProvider for KeptEntry {
    fn entry_pointer(&self) -> NonNull<EntryDescriptor> {
        self.0
    }
}

fn load_failed(path: &Path, why: impl std::fmt::Display) -> HostError {
    HostError::LoadFailed(format!("{}: {why}", path.display()))
}

/// The binary of the CLAP at `path`: the file itself, or (a macOS bundle)
/// its `Contents/MacOS` executable.
fn binary(path: &Path) -> Result<PathBuf, HostError> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    #[cfg(target_os = "macos")]
    {
        // A CLAP bundle has the layout of any macOS bundle.
        crate::vst3::binary_path(path)
    }
    #[cfg(not(target_os = "macos"))]
    Err(load_failed(path, "not a CLAP library"))
}

/// The library's entry: the one already open, else opened now (and never
/// closed).
fn kept_entry(path: &Path) -> Result<KeptEntry, HostError> {
    let binary = binary(path)?;
    let mut guard = LIBRARIES.lock().unwrap_or_else(|e| e.into_inner());
    let libraries = guard.get_or_insert_with(HashMap::new);
    if let Some(e) = libraries.get(&binary) {
        return Ok(*e);
    }
    // SAFETY: loading a plugin binary runs its initializers; plugins are
    // untrusted code by design (spec §2), which is why the scanner loads
    // unknown bundles in a separate process first.
    let library = std::mem::ManuallyDrop::new(
        unsafe { libloading::Library::new(&binary) }.map_err(|e| load_failed(path, e))?,
    );
    // SAFETY: CLAP's `clap_entry` is a `const clap_plugin_entry_t`, whose
    // address the symbol gives.
    let symbol = unsafe { library.get::<*const EntryDescriptor>(b"clap_entry\0") }
        .map_err(|e| load_failed(path, e))?;
    let entry =
        NonNull::new((*symbol).cast_mut()).ok_or_else(|| load_failed(path, "a null clap_entry"))?;
    let kept = KeptEntry(entry);
    libraries.insert(binary, kept);
    Ok(kept)
}

/// Loads (or reuses) the entry of the bundle at `path`; the library stays
/// loaded for the process's life ([`LIBRARIES`]).
pub(crate) fn load_entry(path: &Path) -> Result<PluginEntry, HostError> {
    let entry = kept_entry(path)?;
    let bundle =
        CString::new(path.to_string_lossy().into_owned()).map_err(|e| load_failed(path, e))?;
    // SAFETY: the entry is a CLAP library's `clap_entry`, valid for the
    // process's life; clack runs its `init` with the bundle path (the
    // first time, or again after a `deinit`) and checks its version.
    unsafe { PluginEntry::load_from(entry, &bundle) }.map_err(|e| load_failed(path, e))
}

/// Embeds the plugin's GUI in the `NSView` at `parent`.
pub(crate) fn set_native_parent(
    gui: &PluginGui,
    plugin: &PluginMainThreadHandle,
    parent: *mut c_void,
) -> Result<(), HostError> {
    if parent.is_null() {
        return Err(HostError::EditorUnavailable("no parent view".into()));
    }
    // macOS: SAFETY: `parent` comes from `ParentView::new`, whose contract
    // is that the `NSView *` stays valid until the editor is closed, i.e.
    // until `gui.destroy`, which `ClapInstance` calls before releasing it;
    // the plugin only adds a subview to it.
    #[cfg(target_os = "macos")]
    let window = unsafe { Window::from_cocoa_nsview(parent) };
    // Linux: the pointer carries an X11 window ID (`ParentView::new`).
    #[cfg(not(target_os = "macos"))]
    let window = Window::from_x11_handle(parent as usize as std::ffi::c_ulong);
    // SAFETY: as above; the view (and so the window handle) is valid until
    // `gui.destroy`.
    unsafe { gui.set_parent(plugin, window) }
        .map_err(|e| HostError::EditorUnavailable(format!("{e:?}")))
}
