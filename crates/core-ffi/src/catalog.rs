//! The process-wide plugin catalog (spec §8, §9): where plugins live and
//! which are unavailable. Every session's plugin host is told through
//! [`apply`], with its own marks (one catalog, many hosts). The Plugin
//! Manager and the plugin browser use it through [`PluginCatalog`]; scans
//! run on background threads, one `vizdaw-scan` process per plugin.

use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use crate::snapshot::{PluginFormatTag, PluginRefRow};
use core_host::PluginHost;
use core_host::catalog::{Catalog, CatalogRow, HostMarks, PluginStatus, Source};
use core_host::{PluginKind, scan};
use core_model::mixer::PluginFormat;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static CATALOG: Mutex<Option<Catalog>> = Mutex::new(None);

/// Runs `f` on the catalog (empty until one is loaded).
pub fn with<T>(f: impl FnOnce(&mut Catalog) -> T) -> T {
    let mut g = lock(&CATALOG);
    f(g.get_or_insert_with(Catalog::new))
}

/// Tells `host` what the catalog knows (paths, unavailable plugins);
/// `marks` is what it told this host last time.
pub fn apply(host: &mut PluginHost, marks: &mut HostMarks) {
    with(|c| c.apply_to(host, marks));
}

/// Adds every plugin of the CLAP bundle at `path` as scanned OK, without
/// running the scanner (test hook).
#[cfg(feature = "test-hooks")]
pub fn add_test_bundle(path: &std::path::Path) -> Result<(), crate::VizError> {
    use core_host::catalog::{CatalogEntry, ScanStatus, Source};
    let src = Source::clap_bundle(path);
    let listed = core_host::clap::list_bundle(path).map_err(|e| crate::VizError::Engine {
        message: e.to_string(),
    })?;
    let entries = listed
        .into_iter()
        .map(|(plugin, kind)| CatalogEntry {
            source: src.clone(),
            plugin,
            kind: Some(kind),
            info: None,
            status: ScanStatus::Ok,
        })
        .collect();
    with(|c| c.record(&src, entries));
    crate::plugins::apply_catalog();
    Ok(())
}

/// Instrument or effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PluginKindTag {
    /// Takes notes, makes audio.
    Instrument,
    /// Processes audio.
    Effect,
}

impl From<PluginKind> for PluginKindTag {
    fn from(k: PluginKind) -> Self {
        match k {
            PluginKind::Instrument => PluginKindTag::Instrument,
            PluginKind::Effect => PluginKindTag::Effect,
        }
    }
}

/// A plugin's status in the Plugin Manager (spec §9).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PluginStatusRow {
    /// Usable.
    Ok,
    /// Turned off by the user.
    Disabled,
    /// Crashed, hung or failed while scanned.
    Blocklisted {
        /// `crashed (signal 11)`, `timed out`, `failed: …`.
        reason: String,
    },
    /// Not checked yet (AU v3): checked on first use.
    NotValidated,
    /// Its bundle or component is gone.
    Missing,
}

/// One row of the Plugin Manager or the plugin browser.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CatalogRowFfi {
    /// The plugin (id empty for a bundle that failed as a whole).
    pub plugin: PluginRefRow,
    /// Instrument or effect, when known.
    pub kind: Option<PluginKindTag>,
    /// Status.
    pub status: PluginStatusRow,
    /// Where it lives: a bundle path, or `au:<id>`.
    pub path: String,
}

fn row(r: CatalogRow) -> CatalogRowFfi {
    CatalogRowFfi {
        plugin: (&r.plugin).into(),
        kind: r.kind.map(Into::into),
        status: match r.status {
            PluginStatus::Ok => PluginStatusRow::Ok,
            PluginStatus::Disabled => PluginStatusRow::Disabled,
            PluginStatus::Blocklisted(reason) => PluginStatusRow::Blocklisted { reason },
            PluginStatus::NotValidated => PluginStatusRow::NotValidated,
            PluginStatus::Missing => PluginStatusRow::Missing,
        },
        path: r.source.path.to_string_lossy().into_owned(),
    }
}

/// Where the catalog lives and how to scan.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CatalogConfig {
    /// `plugins.json` (the app passes
    /// `~/Library/Application Support/VizDAW/plugins.json`).
    pub path: String,
    /// The `vizdaw-scan` binary.
    pub scanner: String,
    /// Folders searched for CLAP bundles (empty: the standard ones).
    pub clap_dirs: Vec<String>,
    /// Also find Audio Units.
    pub include_au: bool,
    /// Time each scan may take, in ms (`None`: 10 s, AU v3 30 s).
    pub timeout_ms: Option<u64>,
}

/// Where a scan stands.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ScanProgress {
    /// A scan is running.
    pub running: bool,
    /// Sources scanned so far in this scan.
    pub done: u32,
    /// Sources this scan checks.
    pub total: u32,
}

#[derive(Debug, Default)]
struct ScanState {
    running: bool,
    done: u32,
    total: u32,
}

/// The catalog as the app sees it (one per process): the Plugin Manager's
/// rows and actions, the browser's lists, and scans. Every change is
/// saved to `plugins.json` (never over a file the app cannot read) and
/// told to every open document's plugin host.
#[derive(uniffi::Object)]
pub struct PluginCatalog {
    config: CatalogConfig,
    /// Why the catalog file was not read or not written, if it was not.
    problem: Mutex<Option<String>>,
    scan: Arc<Mutex<ScanState>>,
    /// Bumped by scan threads after recording; `progress` applies the
    /// catalog to the hosts when it moved.
    recorded: Arc<AtomicU64>,
    applied: AtomicU64,
}

impl std::fmt::Debug for PluginCatalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginCatalog")
            .field("path", &self.config.path)
            .finish_non_exhaustive()
    }
}

impl PluginCatalog {
    fn save(&self) {
        let r = with(|c| c.save(&PathBuf::from(&self.config.path)));
        if let Err(e) = r {
            *lock(&self.problem) = Some(format!("plugin settings not saved: {e}"));
        }
    }

    /// Saves, then tells every host on this (the main) thread.
    fn changed(&self) {
        self.save();
        crate::plugins::apply_catalog();
    }

    fn timeout(&self, src: &Source, v3: &[PathBuf]) -> Duration {
        if let Some(ms) = self.config.timeout_ms {
            return Duration::from_millis(ms);
        }
        if src.format == PluginFormat::Au && v3.contains(&src.path) {
            scan::TIMEOUT_AU_V3
        } else {
            scan::TIMEOUT
        }
    }

    /// Every source discovery finds now.
    fn discover(&self) -> Vec<Source> {
        let dirs: Vec<PathBuf> = if self.config.clap_dirs.is_empty() {
            core_host::clap::search_paths()
        } else {
            self.config.clap_dirs.iter().map(PathBuf::from).collect()
        };
        #[allow(unused_mut)]
        let mut found: Vec<Source> = core_host::clap::find_bundles(&dirs)
            .iter()
            .map(|p| Source::clap_bundle(p))
            .collect();
        #[cfg(target_os = "macos")]
        if self.config.include_au {
            found.extend(platform_macos::au::sources());
        }
        found
    }

    /// Scans `sources` on a background thread, up to [`scan::PARALLEL`] at
    /// a time, recording (and saving) each as it finishes.
    fn start(&self, sources: Vec<Source>) -> Result<(), VizError> {
        let exe = PathBuf::from(&self.config.scanner);
        if !exe.is_file() {
            return Err(VizError::NotFound {
                path: self.config.scanner.clone(),
            });
        }
        {
            let mut st = lock(&self.scan);
            if st.running {
                return Err(VizError::Engine {
                    message: "a scan is already running".into(),
                });
            }
            *st = ScanState {
                running: true,
                done: 0,
                total: u32::try_from(sources.len()).unwrap_or(u32::MAX),
            };
        }
        #[cfg(target_os = "macos")]
        let v3: Vec<PathBuf> = if self.config.include_au {
            platform_macos::au::components()
                .into_iter()
                .filter(|c| c.v3)
                .map(|c| PathBuf::from(format!("au:{}", c.plugin.id)))
                .collect()
        } else {
            Vec::new()
        };
        #[cfg(not(target_os = "macos"))]
        let v3: Vec<PathBuf> = Vec::new();
        let jobs: Vec<(Source, Duration)> = sources
            .into_iter()
            .map(|s| {
                let t = self.timeout(&s, &v3);
                (s, t)
            })
            .collect();
        let state = Arc::clone(&self.scan);
        let recorded = Arc::clone(&self.recorded);
        let path = PathBuf::from(&self.config.path);
        let queue = Arc::new(Mutex::new(jobs));
        let spawned = std::thread::Builder::new()
            .name("vizdaw-scan-pool".into())
            .spawn(move || {
                std::thread::scope(|sc| {
                    for _ in 0..scan::PARALLEL {
                        sc.spawn(|| {
                            loop {
                                let job = lock(&queue).pop();
                                let Some((src, timeout)) = job else {
                                    break;
                                };
                                let entries = scan::scan_source(&exe, &src, timeout);
                                with(|c| c.record(&src, entries));
                                lock(&state).done += 1;
                                recorded.fetch_add(1, Ordering::Release);
                            }
                        });
                    }
                });
                // A file the app cannot read is never overwritten (the
                // problem shows on the next action).
                let _ = with(|c| c.save(&path));
                lock(&state).running = false;
                recorded.fetch_add(1, Ordering::Release);
            });
        if let Err(e) = spawned {
            lock(&self.scan).running = false;
            return Err(VizError::Engine {
                message: format!("cannot start the scan: {e}"),
            });
        }
        Ok(())
    }

    fn sources_of(&self, keys: &[PluginKey]) -> Vec<Source> {
        with(|c| {
            let mut v: Vec<Source> = keys
                .iter()
                .filter_map(|k| c.get(k.format.into(), &k.id).map(|e| e.source.clone()))
                .collect();
            v.sort();
            v.dedup();
            v
        })
    }
}

/// A plugin, by format and id.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PluginKey {
    /// Format.
    pub format: PluginFormatTag,
    /// Id.
    pub id: String,
}

#[uniffi::export]
impl PluginCatalog {
    /// Reads the catalog at `config.path` and makes it the app's. A newer
    /// or corrupt file is reported by [`PluginCatalog::problem`] (a corrupt
    /// one is moved to `plugins.json.corrupt`; a newer one is left alone
    /// and never overwritten), and the app goes on with an empty catalog.
    #[uniffi::constructor]
    pub fn open(config: CatalogConfig) -> Result<Arc<PluginCatalog>, VizError> {
        ffi_guard(|| {
            let (cat, problem) = match Catalog::load(&PathBuf::from(&config.path)) {
                Ok(c) => (c, None),
                Err(e) => (
                    Catalog::new(),
                    Some(format!("plugin settings not read: {e}")),
                ),
            };
            *lock(&CATALOG) = Some(cat);
            crate::plugins::apply_catalog();
            Ok(Arc::new(PluginCatalog {
                config,
                problem: Mutex::new(problem),
                scan: Arc::new(Mutex::new(ScanState::default())),
                recorded: Arc::new(AtomicU64::new(0)),
                applied: AtomicU64::new(0),
            }))
        })
    }

    /// Why the catalog file was not read or written, if so (shown once by
    /// the app: it is cleared by this call).
    pub fn take_problem(&self) -> Result<Option<String>, VizError> {
        ffi_guard(|| Ok(lock(&self.problem).take()))
    }

    /// Every known plugin, by name (the Plugin Manager's table).
    pub fn rows(&self) -> Result<Vec<CatalogRowFfi>, VizError> {
        ffi_guard(|| Ok(with(|c| c.rows()).into_iter().map(row).collect()))
    }

    /// What the plugin browser lists for `kind`: usable plugins only
    /// (disabled, blocklisted and missing ones are hidden).
    pub fn browser(&self, kind: PluginKindTag) -> Result<Vec<CatalogRowFfi>, VizError> {
        ffi_guard(|| {
            let k = match kind {
                PluginKindTag::Instrument => PluginKind::Instrument,
                PluginKindTag::Effect => PluginKind::Effect,
            };
            Ok(with(|c| c.browser(k)).into_iter().map(row).collect())
        })
    }

    /// Turns a plugin on or off (an app preference). Slots naming a
    /// disabled plugin load as placeholders "Disabled in Plugin Manager";
    /// enabling it loads the waiting slots of every open document.
    pub fn set_enabled(&self, plugin: PluginKey, enabled: bool) -> Result<(), VizError> {
        ffi_guard(|| {
            with(|c| c.set_enabled(plugin.format.into(), &plugin.id, enabled));
            self.changed();
            Ok(())
        })
    }

    /// Forgets a plugin's blocklist entry and scans its bundle again.
    pub fn clear_blocklist(&self, plugin: PluginKey) -> Result<(), VizError> {
        ffi_guard(|| {
            let src = with(|c| c.clear_blocklist(plugin.format.into(), &plugin.id));
            self.changed();
            match src {
                Some(s) => self.start(vec![s]),
                None => Ok(()),
            }
        })
    }

    /// Scans the bundles of `plugins` again.
    pub fn rescan(&self, plugins: Vec<PluginKey>) -> Result<(), VizError> {
        ffi_guard(|| self.start(self.sources_of(&plugins)))
    }

    /// Finds plugins and scans the new or changed ones (at launch), or
    /// every one (`all`, "Rescan All"). Gone ones become Missing.
    pub fn scan(&self, all: bool) -> Result<(), VizError> {
        ffi_guard(|| {
            let found = self.discover();
            let todo = with(|c| c.mark_found(&found));
            if all {
                self.start(found)
            } else if todo.is_empty() {
                Ok(())
            } else {
                self.start(todo)
            }
        })
    }

    /// Where the running scan stands. Call it from the main thread: what
    /// scans recorded since the last call reaches the open documents here.
    pub fn progress(&self) -> Result<ScanProgress, VizError> {
        ffi_guard(|| {
            let now = self.recorded.load(Ordering::Acquire);
            if self.applied.swap(now, Ordering::AcqRel) != now {
                crate::plugins::apply_catalog();
            }
            let st = lock(&self.scan);
            Ok(ScanProgress {
                running: st.running,
                done: st.done,
                total: st.total,
            })
        })
    }

    /// The bundle to reveal in Finder (CLAP); `None` for Audio Units,
    /// which live in the component registry.
    pub fn reveal_path(&self, plugin: PluginKey) -> Result<Option<String>, VizError> {
        ffi_guard(|| {
            Ok(with(|c| {
                c.get(plugin.format.into(), &plugin.id)
                    .filter(|e| e.source.format == PluginFormat::Clap)
                    .map(|e| e.source.path.to_string_lossy().into_owned())
            }))
        })
    }
}
