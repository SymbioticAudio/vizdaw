//! The app's plug-in catalog, behind a seam (Swift
//! `PluginCatalogProtocol`): the Plugin Manager, the plug-in browser and
//! the launch scan talk to [`PluginCatalogCalls`]; tests use
//! `testing::FakeCatalog`.

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
