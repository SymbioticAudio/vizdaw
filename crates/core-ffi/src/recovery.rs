//! Crash recovery (spec §10): the crash marker a project keeps while it is
//! open, what the app checks before opening a project (a leftover marker,
//! a newer autosave), and the autosave interval.

use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use crate::snapshot::PluginRefRow;
use core_model::file;
use core_model::mixer::PluginRef;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// How often a project with unsaved changes is autosaved (spec §10).
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(120);

static SUPPORT: Mutex<Option<PathBuf>> = Mutex::new(None);
/// Projects open in this process (their markers are not crashes).
static OPEN: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
static INTERVAL_MS: AtomicU64 = AtomicU64::new(120_000);

/// Where VizDAW keeps its own files: set by the app, else
/// `~/Library/Application Support/VizDAW`. Test builds (`test-hooks`)
/// default to a per-process temporary folder, so tests never touch the
/// user's.
pub fn support_dir() -> PathBuf {
    if let Some(p) = lock(&SUPPORT).clone() {
        return p;
    }
    #[cfg(feature = "test-hooks")]
    {
        std::env::temp_dir().join(format!("vizdaw-test-support-{}", std::process::id()))
    }
    #[cfg(not(feature = "test-hooks"))]
    {
        default_support_dir()
    }
}

/// `~/Library/Application Support/VizDAW` (macOS), `~/.local/share/vizdaw`
/// elsewhere.
pub fn default_support_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/VizDAW")
    } else {
        home.join(".local/share/vizdaw")
    }
}

/// Makes `path` the folder for crash markers (and, in the app, the plugin
/// catalog's folder).
#[uniffi::export]
pub fn set_support_dir(path: String) -> Result<(), VizError> {
    ffi_guard(|| {
        *lock(&SUPPORT) = Some(PathBuf::from(path));
        Ok(())
    })
}

/// The autosave interval (tests shorten it).
pub fn autosave_interval() -> Duration {
    Duration::from_millis(INTERVAL_MS.load(Ordering::Relaxed))
}

/// Sets the autosave interval (test hook).
#[cfg(feature = "test-hooks")]
pub fn set_autosave_interval_ms(ms: u64) {
    INTERVAL_MS.store(ms, Ordering::Relaxed);
}

fn key(project: &Path) -> PathBuf {
    std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf())
}

/// `open-projects/<FNV-1a of the project path>.json`.
fn marker_path(project: &Path) -> PathBuf {
    let k = key(project);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in k.to_string_lossy().as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    support_dir()
        .join("open-projects")
        .join(format!("{h:016x}.json"))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Marker {
    project: String,
    /// The plugins being loaded when it was written.
    loading: Vec<PluginRef>,
}

/// Notes that `project` is open here and writes its marker.
pub fn opened(project: &Path, loading: &[PluginRef]) -> Result<(), String> {
    lock(&OPEN).insert(key(project));
    write_marker(project, loading)
}

/// Rewrites `project`'s marker naming the plugins loading now.
pub fn write_marker(project: &Path, loading: &[PluginRef]) -> Result<(), String> {
    let path = marker_path(project);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let m = Marker {
        project: key(project).to_string_lossy().into_owned(),
        loading: loading.to_vec(),
    };
    let text = serde_json::to_string(&m).map_err(|e| e.to_string())?;
    file::write_atomic(&path, text.as_bytes()).map_err(|e| e.to_string())
}

/// A clean close: the marker goes.
pub fn closed(project: &Path) {
    lock(&OPEN).remove(&key(project));
    let _ = std::fs::remove_file(marker_path(project));
}

/// Forgets that `project` is open without removing its marker, as a crash
/// would (test hook).
#[cfg(feature = "test-hooks")]
pub fn forget_open(project: &Path) {
    lock(&OPEN).remove(&key(project));
}

/// What the app must ask before opening a project (spec §10).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecoveryInfo {
    /// VizDAW quit unexpectedly while it was open (a marker is left and no
    /// window here has it open): offer "Open with Plugins Off".
    pub crashed: bool,
    /// The plugins it was loading then (offer to disable them).
    pub loading: Vec<PluginRefRow>,
    /// The newest autosave, when it is newer than `project.json`: offer
    /// "Restore autosave".
    pub autosave: Option<String>,
}

/// Checks `path` before it is opened.
#[uniffi::export]
pub fn recovery_check(path: String) -> Result<RecoveryInfo, VizError> {
    ffi_guard(|| {
        let p = PathBuf::from(&path);
        let marker = std::fs::read_to_string(marker_path(&p)).ok();
        let crashed = marker.is_some() && !lock(&OPEN).contains(&key(&p));
        let loading = marker
            .filter(|_| crashed)
            .and_then(|t| serde_json::from_str::<Marker>(&t).ok())
            .map(|m| m.loading.iter().map(Into::into).collect())
            .unwrap_or_default();
        let mtime = |f: &Path| std::fs::metadata(f).and_then(|m| m.modified()).ok();
        let saved = mtime(&p.join(file::PROJECT_FILE));
        let autosave = file::list_autosaves(&p)
            .ok()
            .and_then(|v| v.into_iter().next())
            .filter(|a| match (mtime(a), saved) {
                (Some(a), Some(s)) => a > s,
                (Some(_), None) => true,
                _ => false,
            })
            .map(|a| a.to_string_lossy().into_owned());
        Ok(RecoveryInfo {
            crashed,
            loading,
            autosave,
        })
    })
}
