//! The app's plug-in catalog, behind a seam (Swift
//! `PluginCatalogProtocol`): the Plugin Manager, the plug-in browser and
//! the launch scan talk to [`PluginCatalogCalls`]; tests use
//! `testing::FakeCatalog`.

use std::collections::VecDeque;
use std::sync::Arc;

use vizdaw_ffi::snapshot::{PluginFormatTag, PluginRefRow};
use vizdaw_ffi::{CatalogRowFfi, PluginCatalog, PluginKey, PluginKindTag, ScanProgress, VizError};

/// What the app needs from the plug-in catalog (`core-ffi`
/// `PluginCatalog`, `catalog.rs:392-515`).
pub trait PluginCatalogCalls: Send + Sync {
    /// Why the catalog file was not read as it was, once.
    fn take_problem(&self) -> Result<Option<String>, VizError>;
    /// Every plug-in, for the Plugin Manager. Reads memory.
    fn rows(&self) -> Result<Vec<CatalogRowFfi>, VizError>;
    /// The usable plug-ins of `kind`, for the browser. Reads memory.
    fn browser(&self, kind: PluginKindTag) -> Result<Vec<CatalogRowFfi>, VizError>;
    /// Turns a plug-in on or off (saved; the hosts unload it at their
    /// next idle).
    fn set_enabled(&self, key: PluginKey, enabled: bool) -> Result<(), VizError>;
    /// Forgets the blocklist entries of `keys` and scans them again (only
    /// starts the background pool).
    fn clear_blocklist(&self, keys: Vec<PluginKey>) -> Result<(), VizError>;
    /// Scans `keys` again (only starts the background pool).
    fn rescan(&self, keys: Vec<PluginKey>) -> Result<(), VizError>;
    /// Finds the installed plug-ins and scans the new or changed ones, or
    /// every one (`all`). Only starts the background pool.
    fn scan(&self, all: bool) -> Result<(), VizError>;
    /// Where the scan stands. **Main thread only**: it applies what the
    /// scan recorded to the hosts.
    fn progress(&self) -> Result<ScanProgress, VizError>;
    /// The bundle to show in the file manager (CLAP and VST3 only).
    fn reveal_path(&self, key: PluginKey) -> Result<Option<String>, VizError>;
}

/// The real catalog, or why it could not open (every call then answers
/// that error).
pub struct LivePluginCatalog(Result<Arc<PluginCatalog>, VizError>);

impl LivePluginCatalog {
    /// Wraps the result of opening the catalog.
    pub fn from_open(opened: Result<Arc<PluginCatalog>, VizError>) -> Self {
        Self(opened)
    }

    fn catalog(&self) -> Result<&PluginCatalog, VizError> {
        self.0.as_deref().map_err(Clone::clone)
    }
}

impl PluginCatalogCalls for LivePluginCatalog {
    fn take_problem(&self) -> Result<Option<String>, VizError> {
        self.catalog()?.take_problem()
    }
    fn rows(&self) -> Result<Vec<CatalogRowFfi>, VizError> {
        self.catalog()?.rows()
    }
    fn browser(&self, kind: PluginKindTag) -> Result<Vec<CatalogRowFfi>, VizError> {
        self.catalog()?.browser(kind)
    }
    fn set_enabled(&self, key: PluginKey, enabled: bool) -> Result<(), VizError> {
        self.catalog()?.set_enabled(key, enabled)
    }
    fn clear_blocklist(&self, keys: Vec<PluginKey>) -> Result<(), VizError> {
        self.catalog()?.clear_blocklist(keys)
    }
    fn rescan(&self, keys: Vec<PluginKey>) -> Result<(), VizError> {
        self.catalog()?.rescan(keys)
    }
    fn scan(&self, all: bool) -> Result<(), VizError> {
        self.catalog()?.scan(all)
    }
    fn progress(&self) -> Result<ScanProgress, VizError> {
        self.catalog()?.progress()
    }
    fn reveal_path(&self, key: PluginKey) -> Result<Option<String>, VizError> {
        self.catalog()?.reveal_path(key)
    }
}

/// The app's catalog (`plugins.json` in the support folder), behind the
/// seam. When it could not open, every call answers why.
pub fn live_plugin_catalog() -> Arc<dyn PluginCatalogCalls> {
    Arc::new(LivePluginCatalog::from_open(crate::session::live_catalog()))
}

/// A format's name in the browser and the Plugin Manager. VST3 reads
/// "VST3" (Swift labelled it "CLAP", `PluginBrowserView.swift:34`,
/// `PluginManagerModel.swift:35`).
pub fn format_label(format: PluginFormatTag) -> &'static str {
    match format {
        PluginFormatTag::Au => "AU",
        PluginFormatTag::Clap => "CLAP",
        PluginFormatTag::Vst3 => "VST3",
    }
}

/// The catalog's key for `plugin`.
pub fn plugin_key(plugin: &PluginRefRow) -> PluginKey {
    PluginKey {
        format: plugin.format,
        id: plugin.id.clone(),
    }
}

/// The alert titles (Swift `AppPlugins.swift:43,67,95,99,106,116`).
pub const UNAVAILABLE_TITLE: &str = "Plugins Unavailable";
pub const SETTINGS_TITLE: &str = "Plugin Settings";
pub const SCAN_FAILED_TITLE: &str = "Scan Failed";
pub const RESCAN_FAILED_TITLE: &str = "Rescan Failed";

/// The most catalog problems read in one go: a catalog repeating one
/// problem cannot spin the loop (`AppPlugins.swift:64-68`).
const PROBLEMS_PER_READ: usize = 16;

/// The core's refusal of a scan while one runs
/// (`core-ffi/src/catalog.rs:261`, `:438`).
const ALREADY_RUNNING: &str = "a scan is already running";

/// The app's plug-in catalog (Swift `AppPlugins`, `M/AppPlugins.swift`):
/// one per process, started at launch, scanned for new or changed
/// plug-ins, and read by the plug-in browser and the Plugin Manager. Each
/// problem (a catalog it cannot read or write, a failed scan) is one
/// alert. Main thread only: [`poll`](Self::poll) reads the scan's
/// progress, which applies its results to the hosts.
pub struct AppPlugins {
    /// `None`: the app has none (tests that never start one).
    catalog: Option<Arc<dyn PluginCatalogCalls>>,
    /// Started, and the catalog answered.
    open: bool,
    rows: Vec<CatalogRowFfi>,
    progress: ScanProgress,
    /// Waiting alerts (title, message), oldest first.
    alerts: VecDeque<(String, String)>,
    /// Bumped whenever `rows` is read again.
    version: u64,
}

impl Default for AppPlugins {
    /// No catalog: every call answers nothing.
    fn default() -> Self {
        AppPlugins {
            catalog: None,
            open: false,
            rows: vec![],
            progress: idle_progress(),
            alerts: VecDeque::new(),
            version: 0,
        }
    }
}

fn idle_progress() -> ScanProgress {
    ScanProgress {
        running: false,
        done: 0,
        total: 0,
    }
}

impl AppPlugins {
    /// Over `catalog`; nothing is read until [`start`](Self::start).
    pub fn new(catalog: Arc<dyn PluginCatalogCalls>) -> Self {
        AppPlugins {
            catalog: Some(catalog),
            ..Self::default()
        }
    }

    /// Reads the catalog's problems and rows (a catalog that cannot be
    /// read is the alert "Plugins Unavailable"), then, with `scan`, scans
    /// the new or changed plug-ins in the background
    /// (`AppPlugins.swift:39-48`).
    pub fn start(&mut self, scan: bool) {
        let Some(c) = self.catalog.clone() else {
            return;
        };
        // A catalog that did not open answers every call with why.
        match c.take_problem() {
            Ok(first) => {
                self.open = true;
                if let Some(p) = first {
                    self.enqueue(SETTINGS_TITLE, p);
                }
            }
            Err(e) => {
                self.open = false;
                self.report(&e, UNAVAILABLE_TITLE, false);
                return;
            }
        }
        self.refresh();
        if scan {
            self.scan_new();
        }
    }

    /// The catalog answered at [`start`](Self::start).
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Every known plug-in, as last read (the Plugin Manager).
    pub fn rows(&self) -> &[CatalogRowFfi] {
        &self.rows
    }

    /// Bumped whenever [`rows`](Self::rows) is read again.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The browser's list for `kind`: the usable plug-ins only (reads
    /// memory).
    pub fn browser(&self, kind: PluginKindTag) -> Vec<CatalogRowFfi> {
        self.live()
            .and_then(|c| c.browser(kind).ok())
            .unwrap_or_default()
    }

    /// Where the scan stood at the last poll.
    pub fn progress(&self) -> ScanProgress {
        self.progress.clone()
    }

    /// A scan runs (as last read): the app polls a few times a second.
    pub fn needs_poll(&self) -> bool {
        self.open && self.progress.running
    }

    /// Reads the scan's progress (**main thread**: it applies the scan's
    /// results to the hosts); a scan that advanced or finished rereads the
    /// rows (`AppPlugins.swift:81-87`). Then takes the catalog's problems.
    /// True when anything changed (the progress, the rows or the alerts).
    pub fn poll(&mut self) -> bool {
        let Some(c) = self.live() else {
            return false;
        };
        let alerts = self.alerts.len();
        let mut changed = false;
        if let Ok(p) = c.progress() {
            let moved = p.running != self.progress.running || p.done != self.progress.done;
            changed = p != self.progress;
            self.progress = p;
            if moved {
                self.read_rows();
            }
        }
        self.take_problems();
        changed || self.alerts.len() != alerts
    }

    /// Scans the new or changed plug-ins (silent when a scan already
    /// runs; `AppPlugins.swift:109-111`).
    pub fn scan_new(&mut self) {
        self.perform(SCAN_FAILED_TITLE, true, |c| c.scan(false));
    }

    /// Scans every plug-in again ("Rescan All").
    pub fn scan_all(&mut self) {
        self.perform(SCAN_FAILED_TITLE, false, |c| c.scan(true));
    }

    /// Scans `keys` again.
    pub fn rescan(&mut self, keys: Vec<PluginKey>) {
        self.perform(RESCAN_FAILED_TITLE, false, |c| c.rescan(keys));
    }

    /// Forgets the blocklist entries of `keys`; their bundles are scanned
    /// again (one scan).
    pub fn clear_blocklist(&mut self, keys: Vec<PluginKey>) {
        self.perform(RESCAN_FAILED_TITLE, false, |c| c.clear_blocklist(keys));
    }

    /// Turns a plug-in on or off; true when the catalog took it (false:
    /// not started, or it refused).
    pub fn set_enabled(&mut self, key: PluginKey, on: bool) -> bool {
        self.perform(SETTINGS_TITLE, false, |c| c.set_enabled(key, on))
    }

    /// The bundle to show in the file manager (CLAP and VST3), if any.
    pub fn reveal_path(&self, key: PluginKey) -> Option<String> {
        self.live().and_then(|c| c.reveal_path(key).ok().flatten())
    }

    /// The oldest waiting alert (title, message), taken.
    pub fn next_alert(&mut self) -> Option<(String, String)> {
        self.alerts.pop_front()
    }

    fn live(&self) -> Option<Arc<dyn PluginCatalogCalls>> {
        self.catalog.clone().filter(|_| self.open)
    }

    /// Rereads the rows and takes the problems (`AppPlugins.swift:59-72`).
    fn refresh(&mut self) {
        self.read_rows();
        self.take_problems();
    }

    fn read_rows(&mut self) {
        let Some(c) = self.live() else {
            return;
        };
        match c.rows() {
            Ok(rows) => {
                self.rows = rows;
                self.version += 1;
            }
            Err(e) => self.report(&e, UNAVAILABLE_TITLE, false),
        }
    }

    fn take_problems(&mut self) {
        let Some(c) = self.live() else {
            return;
        };
        for _ in 0..PROBLEMS_PER_READ {
            match c.take_problem() {
                Ok(Some(p)) => self.enqueue(SETTINGS_TITLE, p),
                Ok(None) => break,
                Err(e) => {
                    self.report(&e, UNAVAILABLE_TITLE, false);
                    break;
                }
            }
        }
    }

    /// Queues an alert unless an identical one already waits
    /// (`AppPlugins.swift:124-128`).
    fn enqueue(&mut self, title: &str, message: String) {
        if self.alerts.iter().any(|(t, m)| t == title && *m == message) {
            return;
        }
        self.alerts.push_back((title.to_owned(), message));
    }

    /// Queues an alert for `error`: `Busy` never alerts (it is retried),
    /// and with `quiet_when_running` neither does the core's refusal while
    /// a scan runs (`AppPlugins.swift:130-135`).
    fn report(&mut self, error: &VizError, title: &str, quiet_when_running: bool) {
        match error {
            VizError::Busy { .. } => {}
            VizError::Engine { message } if quiet_when_running && message == ALREADY_RUNNING => {}
            _ => self.enqueue(title, crate::alerts::error_message(error)),
        }
    }

    /// Runs `body` on the catalog, alerting `title` when it fails, then
    /// rereads the rows and the progress (`AppPlugins.swift:137-153`).
    /// False when not started or it failed.
    fn perform(
        &mut self,
        title: &str,
        quiet_when_running: bool,
        body: impl FnOnce(&dyn PluginCatalogCalls) -> Result<(), VizError>,
    ) -> bool {
        let Some(c) = self.live() else {
            return false;
        };
        let ok = match body(&*c) {
            Ok(()) => true,
            Err(e) => {
                self.report(&e, title, quiet_when_running);
                false
            }
        };
        self.refresh();
        self.poll();
        ok
    }
}
