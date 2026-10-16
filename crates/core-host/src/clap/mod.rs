//! The CLAP host (spec §5) on `clack-host`: bundles are found by the
//! catalog, loaded on the main thread (Plan Ruling 2) and wrapped as
//! [`ClapInstance`] / [`ClapProcessor`].

mod ffi;
mod handlers;
mod instance;
mod processor;

pub use instance::ClapInstance;
pub use processor::ClapProcessor;

use crate::error::HostError;
use crate::info::PluginKind;
use crate::instance::{LoadRequest, PluginFormat, PluginInstance, PrepareJob, Prepared};
use core_model::mixer::{PluginFormat as Format, PluginRef};
use std::path::{Path, PathBuf};

/// The standard CLAP search folders (spec §5): the user's, then the
/// system's.
pub fn search_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        let mut v: Vec<PathBuf> = home
            .map(|h| h.join("Library/Audio/Plug-Ins/CLAP"))
            .into_iter()
            .collect();
        v.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
        v
    } else {
        let mut v: Vec<PathBuf> = home.map(|h| h.join(".clap")).into_iter().collect();
        v.push(PathBuf::from("/usr/lib/clap"));
        v
    }
}

/// Every `.clap` bundle (or file) under `dirs`, recursively, sorted.
pub fn find_bundles(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "clap") {
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

/// The plugins a bundle's factory lists (loads the bundle into this
/// process: the scanner does this, the app does not, spec §8).
pub fn list_bundle(path: &Path) -> Result<Vec<(PluginRef, PluginKind)>, HostError> {
    let entry = ffi::load_entry(path)?;
    let factory = entry
        .get_plugin_factory()
        .ok_or_else(|| HostError::LoadFailed(format!("{}: no plugin factory", path.display())))?;
    let text = |c: Option<&std::ffi::CStr>| c.map(|s| s.to_string_lossy().into_owned());
    Ok(factory
        .plugin_descriptors()
        .filter_map(|d| {
            let id = text(d.id())?;
            let instrument = d.features().any(|f| f.to_bytes() == b"instrument");
            Some((
                PluginRef {
                    format: Format::Clap,
                    name: text(d.name()).unwrap_or_else(|| id.clone()),
                    vendor: text(d.vendor()).unwrap_or_default(),
                    version: text(d.version()).unwrap_or_default(),
                    id,
                },
                if instrument {
                    PluginKind::Instrument
                } else {
                    PluginKind::Effect
                },
            ))
        })
        .collect())
}

/// The CLAP [`PluginFormat`].
#[derive(Debug, Default)]
pub struct ClapFormat;

impl ClapFormat {
    /// The format.
    pub fn new() -> ClapFormat {
        ClapFormat
    }
}

impl PluginFormat for ClapFormat {
    fn format(&self) -> Format {
        Format::Clap
    }

    fn prepare(&self, req: &LoadRequest) -> PrepareJob {
        let path = req.path.clone();
        let id = req.plugin.id.clone();
        Box::new(move || {
            let path = path.ok_or_else(|| HostError::NotFound(id.clone()))?;
            if !path.exists() {
                return Err(HostError::NotFound(format!("{id} ({})", path.display())));
            }
            Ok(Prepared(Box::new(path)))
        })
    }

    fn finish(
        &mut self,
        req: &LoadRequest,
        prepared: Prepared,
    ) -> Result<Box<dyn PluginInstance>, HostError> {
        let path = prepared
            .0
            .downcast::<PathBuf>()
            .map_err(|_| HostError::LoadFailed("not a CLAP load".into()))?;
        Ok(Box::new(ClapInstance::load(&path, &req.plugin.id)?))
    }
}
