//! The plugin catalog (spec §8, §9): what scans found, the blocklist, the
//! scan cache and the enabled/disabled preference, stored in
//! `plugins.json`. It is plain data: the scanner fills it, the Plugin
//! Manager (3c) reads [`Catalog::rows`] and changes it with
//! [`Catalog::set_enabled`] and [`Catalog::clear_blocklist`], and
//! [`Catalog::apply`] tells a [`crate::PluginHost`] where plugins live and
//! which are unavailable.

use crate::host::PluginHost;
use crate::info::{PluginInfo, PluginKind};
use core_model::mixer::{PluginFormat, PluginRef};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// File format version of `plugins.json`.
pub const CATALOG_VERSION: u32 = 1;

/// Where plugins come from: a CLAP bundle or an AU component, with what
/// identifies the build that was scanned (the cache key, spec §8).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Format.
    pub format: PluginFormat,
    /// Bundle path (CLAP), or `au:<type/subtype/manufacturer>` (AU).
    pub path: PathBuf,
    /// Bundle (or component) version.
    pub version: String,
    /// Modification time of the bundle's newest file, in seconds since the
    /// epoch (0 for AU).
    pub mtime: u64,
}

impl Source {
    /// The source of a CLAP bundle at `path`: its `CFBundleShortVersionString`
    /// (macOS) and the newest modification time of its files.
    pub fn clap_bundle(path: &Path) -> Source {
        Source {
            format: PluginFormat::Clap,
            path: path.to_path_buf(),
            version: bundle_version(path),
            mtime: newest_mtime(path, 4),
        }
    }

    /// True when this is the same build of the same place as `other`.
    pub fn same_place(&self, other: &Source) -> bool {
        self.format == other.format && self.path == other.path
    }
}

/// Reads `CFBundleShortVersionString` from `Contents/Info.plist`, or "".
fn bundle_version(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path.join("Contents").join("Info.plist")) else {
        return String::new();
    };
    let key = "<key>CFBundleShortVersionString</key>";
    text.split_once(key)
        .and_then(|(_, rest)| rest.split_once("<string>"))
        .and_then(|(_, rest)| rest.split_once("</string>"))
        .map(|(v, _)| v.trim().to_string())
        .unwrap_or_default()
}

fn mtime_of(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// The newest modification time of `path` and anything under it, `depth`
/// levels down.
fn newest_mtime(path: &Path, depth: u32) -> u64 {
    let mut m = mtime_of(path);
    if depth > 0 {
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                m = m.max(newest_mtime(&e.path(), depth - 1));
            }
        }
    }
    m
}

/// What scanning a plugin gave.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    /// Loaded, activated and processed a block.
    Ok,
    /// Crashed, hung or failed; never loaded again until cleared.
    Blocklisted {
        /// `crashed (signal 11)`, `timed out`, `failed: <message>`.
        reason: String,
    },
    /// Could not be checked outside the app (AU v3); checked on first use.
    NotValidated,
}

/// One plugin (or, with an empty id, a whole source that failed to list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    /// Where it came from.
    pub source: Source,
    /// Format, id, name, vendor, version (id empty for a failed source).
    pub plugin: PluginRef,
    /// Instrument or effect, when known.
    pub kind: Option<PluginKind>,
    /// Ports and parameters, when it scanned OK.
    pub info: Option<PluginInfo>,
    /// Scan result.
    pub status: ScanStatus,
}

/// A plugin's status as the Plugin Manager shows it (spec §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    /// Usable.
    Ok,
    /// Turned off by the user.
    Disabled,
    /// Crashed, hung or failed during a scan.
    Blocklisted(String),
    /// Not checked yet (AU v3).
    NotValidated,
    /// Its bundle or component is gone.
    Missing,
}

/// One row of the Plugin Manager.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogRow {
    /// The plugin (id empty for a source that failed as a whole).
    pub plugin: PluginRef,
    /// Instrument or effect, when known.
    pub kind: Option<PluginKind>,
    /// Status.
    pub status: PluginStatus,
    /// Where it lives.
    pub source: Source,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Repr {
    version: u32,
    entries: Vec<CatalogEntry>,
    disabled: Vec<(PluginFormat, String)>,
}

/// The catalog.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    entries: Vec<CatalogEntry>,
    disabled: BTreeSet<(PluginFormat, String)>,
    /// Sources found missing by the last [`Catalog::mark_found`].
    missing: BTreeSet<Source>,
    /// Plugins the last [`Catalog::apply`] marked unavailable on the host.
    marked: BTreeSet<(PluginFormat, String)>,
}

impl Catalog {
    /// An empty catalog.
    pub fn new() -> Catalog {
        Catalog::default()
    }

    /// `~/Library/Application Support/VizDAW/plugins.json` on macOS;
    /// `$XDG_DATA_HOME/vizdaw/plugins.json` (or `~/.local/share/…`)
    /// elsewhere.
    pub fn default_path() -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        if cfg!(target_os = "macos") {
            home.join("Library/Application Support/VizDAW/plugins.json")
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share"))
                .join("vizdaw/plugins.json")
        }
    }

    /// Reads `path`; a missing file is an empty catalog, an unreadable or
    /// corrupt one is an error (the caller decides whether to start over).
    pub fn load(path: &Path) -> Result<Catalog, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Catalog::new()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let fail = |e: String| format!("{}: {e}", path.display());
        let corrupt = |e: String| {
            // Keep the unreadable file for inspection; a fresh save must not
            // destroy it.
            let mut name = path.as_os_str().to_owned();
            name.push(".corrupt");
            let _ = std::fs::rename(path, PathBuf::from(name));
            fail(format!("{e} (moved to .corrupt)"))
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => return Err(corrupt(e.to_string())),
        };
        if let Some(v) = value.get("version").and_then(|v| v.as_u64()) {
            if v > u64::from(CATALOG_VERSION) {
                return Err(fail(format!(
                    "catalog version {v} is newer than this app understands ({CATALOG_VERSION}); left untouched"
                )));
            }
        }
        let r: Repr = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(e) => return Err(corrupt(e.to_string())),
        };
        if r.version != CATALOG_VERSION {
            return Err(corrupt(format!(
                "catalog version {} (expected {CATALOG_VERSION})",
                r.version
            )));
        }
        Ok(Catalog {
            entries: r.entries,
            disabled: r.disabled.into_iter().collect(),
            missing: BTreeSet::new(),
            marked: BTreeSet::new(),
        })
    }

    /// Writes `path` (creating its folder) through a temporary file, so a
    /// crash never leaves half a catalog.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let r = Repr {
            version: CATALOG_VERSION,
            entries: self.entries.clone(),
            disabled: self.disabled.iter().cloned().collect(),
        };
        if let Ok(old) = std::fs::read_to_string(path) {
            let newer = serde_json::from_str::<serde_json::Value>(&old)
                .ok()
                .and_then(|v| v.get("version").and_then(|v| v.as_u64()))
                .is_some_and(|v| v > u64::from(CATALOG_VERSION));
            if newer {
                return Err(format!(
                    "{}: written by a newer version; not overwriting",
                    path.display()
                ));
            }
        }
        let text = serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let write = || -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        };
        write().map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }

    /// Every entry.
    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }

    /// The entry of plugin `id` of `format`.
    pub fn get(&self, format: PluginFormat, id: &str) -> Option<&CatalogEntry> {
        self.entries
            .iter()
            .find(|e| e.plugin.format == format && e.plugin.id == id)
    }

    /// Of `found` (every source discovery saw now), the ones that must be
    /// scanned: new, or changed since their scan (another version or
    /// modification time). Entries of changed sources are dropped;
    /// entries whose source was not found are marked missing.
    pub fn mark_found(&mut self, found: &[Source]) -> Vec<Source> {
        let mut todo = Vec::new();
        for s in found {
            let known = self.entries.iter().any(|e| &e.source == s);
            if !known {
                self.entries.retain(|e| !e.source.same_place(s));
                todo.push(s.clone());
            }
        }
        self.missing = self
            .entries
            .iter()
            .map(|e| e.source.clone())
            .filter(|src| !found.iter().any(|f| f.same_place(src)))
            .collect();
        todo
    }

    /// Records what scanning `source` gave (replacing earlier entries of
    /// that place).
    pub fn record(&mut self, source: &Source, entries: Vec<CatalogEntry>) {
        self.entries.retain(|e| !e.source.same_place(source));
        self.missing.remove(source);
        self.entries.extend(entries);
    }

    /// Turns a plugin on or off (an app preference, not part of projects).
    pub fn set_enabled(&mut self, format: PluginFormat, id: &str, enabled: bool) {
        let key = (format, id.to_string());
        if enabled {
            self.disabled.remove(&key);
        } else {
            self.disabled.insert(key);
        }
    }

    /// Forgets a blocklist entry; returns its source, to be scanned again.
    pub fn clear_blocklist(&mut self, format: PluginFormat, id: &str) -> Option<Source> {
        let source = self
            .entries
            .iter()
            .find(|e| {
                e.plugin.format == format
                    && e.plugin.id == id
                    && matches!(e.status, ScanStatus::Blocklisted { .. })
            })?
            .source
            .clone();
        self.entries.retain(|e| !e.source.same_place(&source));
        Some(source)
    }

    /// A plugin's status.
    pub fn status(&self, format: PluginFormat, id: &str) -> Option<PluginStatus> {
        let e = self.get(format, id)?;
        Some(self.status_of(e))
    }

    fn status_of(&self, e: &CatalogEntry) -> PluginStatus {
        if self.missing.contains(&e.source) {
            return PluginStatus::Missing;
        }
        match &e.status {
            ScanStatus::Blocklisted { reason } => PluginStatus::Blocklisted(reason.clone()),
            _ if self
                .disabled
                .contains(&(e.plugin.format, e.plugin.id.clone())) =>
            {
                PluginStatus::Disabled
            }
            ScanStatus::NotValidated => PluginStatus::NotValidated,
            ScanStatus::Ok => PluginStatus::Ok,
        }
    }

    /// The Plugin Manager's rows, by name.
    pub fn rows(&self) -> Vec<CatalogRow> {
        let mut rows: Vec<CatalogRow> = self
            .entries
            .iter()
            .map(|e| CatalogRow {
                plugin: e.plugin.clone(),
                kind: e.kind,
                status: self.status_of(e),
                source: e.source.clone(),
            })
            .collect();
        rows.sort_by(|a, b| {
            a.plugin
                .name
                .to_lowercase()
                .cmp(&b.plugin.name.to_lowercase())
                .then_with(|| a.plugin.id.cmp(&b.plugin.id))
        });
        rows
    }

    /// What the plugin browser lists for `kind`: usable plugins (OK or not
    /// validated), hidden when disabled, blocklisted or missing (spec §9).
    pub fn browser(&self, kind: PluginKind) -> Vec<CatalogRow> {
        self.rows()
            .into_iter()
            .filter(|r| r.kind == Some(kind) && !r.plugin.id.is_empty())
            .filter(|r| matches!(r.status, PluginStatus::Ok | PluginStatus::NotValidated))
            .collect()
    }

    /// Tells `host` where every plugin lives and which ones are
    /// unavailable (their slots load as placeholders with the reason, at
    /// the next load; live instances are not stopped). It clears only
    /// reasons it set itself earlier, never a failure of the host's own.
    pub fn apply(&mut self, host: &mut PluginHost) {
        let mut now = BTreeSet::new();
        for e in &self.entries {
            if e.plugin.id.is_empty() {
                continue;
            }
            if e.plugin.format == PluginFormat::Clap {
                host.register_path(e.plugin.format, &e.plugin.id, e.source.path.clone());
            }
            let why = match self.status_of(e) {
                PluginStatus::Ok | PluginStatus::NotValidated => None,
                PluginStatus::Disabled => Some("Disabled in Plugin Manager".to_string()),
                PluginStatus::Blocklisted(r) => Some(format!("Blocklisted: {r}")),
                PluginStatus::Missing => Some("Missing".to_string()),
            };
            if let Some(w) = why {
                let key = (e.plugin.format, e.plugin.id.clone());
                host.set_unavailable(key.0, &key.1, Some(w));
                now.insert(key);
            }
        }
        for (format, id) in self.marked.difference(&now) {
            host.set_unavailable(*format, id, None);
        }
        self.marked = now;
    }
}
