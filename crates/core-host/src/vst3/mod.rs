//! The VST3 host (host track H1) on the `vst3` bindings: modules are found
//! in the VST3 folders, loaded (bundle resolution, the module entry and
//! `GetPluginFactory`) and their factory's plug-in classes listed. The
//! scanner (`vizdaw-scan --format vst3`) does this in its own process; the
//! app never loads an unscanned bundle (spec §8).
//!
//! A plug-in's ID is its class ID as 32 upper-case hex digits
//! ([`class_id_hex`]). A loaded plug-in is a [`Vst3Instance`]; the
//! [`Vst3Format`] makes them and shares one loaded module between the
//! instances of a bundle.

mod ffi;
mod instance;
mod module;

pub use instance::{Vst3Format, Vst3Instance};
pub(crate) use module::bundle_version;
pub use module::{binary_path, class_id_hex, parse_class_id};

use crate::error::HostError;
use crate::info::PluginKind;
use core_model::mixer::PluginRef;
use ffi::Module;
use ffi::sdk::Vst::IComponent;
use std::path::{Path, PathBuf};

/// The VST3 search folders: the app's bundled folder
/// ([`crate::bundled::dir`]) first, then the user's, then the system's.
pub fn search_paths() -> Vec<PathBuf> {
    search_paths_with(crate::bundled::dir())
}

/// [`search_paths`] with an explicit bundled folder `first`.
pub fn search_paths_with(first: Option<PathBuf>) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut v: Vec<PathBuf> = first.into_iter().collect();
    if cfg!(target_os = "macos") {
        v.extend(home.map(|h| h.join("Library/Audio/Plug-Ins/VST3")));
        v.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
    } else {
        v.extend(home.map(|h| h.join(".vst3")));
        v.push(PathBuf::from("/usr/lib/vst3"));
        v.push(PathBuf::from("/usr/local/lib/vst3"));
    }
    v
}

/// Every `.vst3` bundle under `dirs`, recursively (but not inside a
/// bundle), sorted.
pub fn find_bundles(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("vst3"))
            {
                out.push(p);
            } else if p.is_dir() && depth < 8 {
                walk(&p, out, depth + 1);
            }
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        walk(d, &mut out, 0);
    }
    out.sort();
    out
}

/// The plug-ins a bundle's factory lists (loads the bundle into this
/// process: the scanner does this, the app does not, spec §8).
pub fn list_bundle(path: &Path) -> Result<Vec<(PluginRef, PluginKind)>, HostError> {
    let m = Module::load(path)?;
    Ok(module::plugins(&m.classes(), &m.vendor()))
}

/// Checks plug-in `id` (a class ID) of the bundle at `path`: loads the
/// module and creates and releases the class's component. Loads the
/// bundle into this process, as [`list_bundle`] does.
pub fn check_class(path: &Path, id: &str) -> Result<(), HostError> {
    let cid = parse_class_id(id).ok_or_else(|| HostError::NotFound(id.to_string()))?;
    let m = Module::load(path)?;
    let is_plugin = m
        .classes()
        .iter()
        .any(|c| c.cid == cid && c.category == module::AUDIO_MODULE_CLASS);
    if !is_plugin {
        return Err(HostError::NotFound(format!("{id} ({})", path.display())));
    }
    let component = m.create::<IComponent>(&cid)?;
    // Released before the module goes.
    drop(component);
    Ok(())
}
