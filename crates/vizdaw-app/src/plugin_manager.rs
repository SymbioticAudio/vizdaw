//! The Plugin Manager window's model (Swift `PluginManagerModel`,
//! `M/PluginManagerModel.swift`; spec §9): every known plug-in with its
//! status, filtered by format and by a search of name or vendor; a
//! selection that drives Rescan Selected, Clear Blocklist and Reveal; and
//! turning a plug-in on or off, which unloads or loads it in every open
//! project at once. The rows are [`AppPlugins`]' as last read.

use std::collections::BTreeSet;

use vizdaw_ffi::snapshot::PluginFormatTag;
use vizdaw_ffi::{CatalogRowFfi, PluginKey, PluginKindTag, PluginStatusRow};

use crate::DocumentStore;
use crate::plugins::{AppPlugins, format_label, plugin_key};

/// The window's title (`S/VizDAWApp.swift:258`).
pub const TITLE: &str = "Plugin Manager";
/// The footer (`S/PluginManagerView.swift:35`).
pub const FOOTER: &str =
    "Turning a plugin off unloads it from all open projects; its settings are kept.";
/// The column titles (`S/PluginManagerView.swift:15-28`).
pub const COLUMNS: [&str; 7] = [
    "On", "Name", "Vendor", "Format", "Version", "Type", "Status",
];
/// The format picker's name (`S/PluginManagerView.swift:41`).
pub const FORMAT: &str = "Format";
/// The buttons (`S/PluginManagerView.swift:47-53`).
pub const RESCAN_SELECTED: &str = "Rescan Selected";
pub const RESCAN_ALL: &str = "Rescan All";
pub const CLEAR_BLOCKLIST: &str = "Clear Blocklist";
/// The reveal button: "Reveal in Finder" on macOS
/// (`S/PluginManagerView.swift:53`), "Show in Files" elsewhere.
pub const REVEAL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else {
    "Show in Files"
};

/// The format picker's choices (Swift had All/AU/CLAP,
/// `M/PluginManagerModel.swift:17-22`; VST3 is its own here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormatFilter {
    #[default]
    All,
    Au,
    Clap,
    Vst3,
}

impl FormatFilter {
    /// What the picker offers here: AU only on macOS.
    pub fn available() -> &'static [FormatFilter] {
        if cfg!(target_os = "macos") {
            &[Self::All, Self::Au, Self::Clap, Self::Vst3]
        } else {
            &[Self::All, Self::Clap, Self::Vst3]
        }
    }

    /// The segment's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Au => "AU",
            Self::Clap => "CLAP",
            Self::Vst3 => "VST3",
        }
    }

    fn matches(self, format: PluginFormatTag) -> bool {
        match self {
            Self::All => true,
            Self::Au => format == PluginFormatTag::Au,
            Self::Clap => format == PluginFormatTag::Clap,
            Self::Vst3 => format == PluginFormatTag::Vst3,
        }
    }
}

/// One table row (Swift `ManagerRow`, `M/PluginManagerModel.swift:29-61`).
#[derive(Debug, Clone, PartialEq)]
pub struct ManagerRow {
    pub row: CatalogRowFfi,
}

impl ManagerRow {
    /// `"<format>:<id>|<path>"` (`:31`).
    pub fn id(&self) -> String {
        let format = match self.row.plugin.format {
            PluginFormatTag::Au => "au",
            PluginFormatTag::Clap => "clap",
            PluginFormatTag::Vst3 => "vst3",
        };
        format!("{format}:{}|{}", self.row.plugin.id, self.row.path)
    }

    pub fn name(&self) -> &str {
        &self.row.plugin.name
    }

    pub fn vendor(&self) -> &str {
        &self.row.plugin.vendor
    }

    pub fn version(&self) -> &str {
        &self.row.plugin.version
    }

    /// Where it lives (the row's tooltip).
    pub fn path(&self) -> &str {
        &self.row.path
    }

    /// "AU", "CLAP" or "VST3".
    pub fn format_text(&self) -> &'static str {
        format_label(self.row.plugin.format)
    }

    /// "Instrument", "Effect" or "—" (`:36-42`).
    pub fn kind_text(&self) -> &'static str {
        match self.row.kind {
            Some(PluginKindTag::Instrument) => "Instrument",
            Some(PluginKindTag::Effect) => "Effect",
            None => "—",
        }
    }

    /// "OK", "Disabled", "Blocklisted: ‹reason›", "Not validated",
    /// "Missing" (`:45-53`).
    pub fn status_text(&self) -> String {
        match &self.row.status {
            PluginStatusRow::Ok => "OK".into(),
            PluginStatusRow::Disabled => "Disabled".into(),
            PluginStatusRow::Blocklisted { reason } => format!("Blocklisted: {reason}"),
            PluginStatusRow::NotValidated => "Not validated".into(),
            PluginStatusRow::Missing => "Missing".into(),
        }
    }

    /// The status is OK (the status shows in the primary text colour).
    pub fn is_ok(&self) -> bool {
        self.row.status == PluginStatusRow::Ok
    }

    pub fn is_enabled(&self) -> bool {
        self.row.enabled
    }

    pub fn is_blocklisted(&self) -> bool {
        matches!(self.row.status, PluginStatusRow::Blocklisted { .. })
    }

    /// A whole bundle that failed has no plug-in id: nothing to enable
    /// (`:58-59`).
    pub fn has_plugin(&self) -> bool {
        !self.row.plugin.id.is_empty()
    }

    pub fn key(&self) -> PluginKey {
        plugin_key(&self.row.plugin)
    }
}

/// The rows `filter` and `search` (trimmed; name or vendor, any case)
/// leave of `plugins`' (`M/PluginManagerModel.swift:65-76`).
pub fn rows(plugins: &AppPlugins, filter: FormatFilter, search: &str) -> Vec<ManagerRow> {
    let q = search.trim().to_lowercase();
    plugins
        .rows()
        .iter()
        .filter(|r| filter.matches(r.plugin.format))
        .filter(|r| {
            q.is_empty()
                || r.plugin.name.to_lowercase().contains(&q)
                || r.plugin.vendor.to_lowercase().contains(&q)
        })
        .map(|r| ManagerRow { row: r.clone() })
        .collect()
}

/// How a row was clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// Selects that row only.
    Plain,
    /// ⌘-click: adds or removes it.
    Toggle,
    /// ⇧-click: the rows from the last clicked one to it.
    Extend,
}

/// Opens the file manager at a plug-in's bundle. The app's runs a
/// process off the UI thread ([`SystemRevealer`]); tests record.
pub trait Revealer: Send + Sync {
    fn reveal(&self, path: &str);
}

/// `open -R <path>` on macOS (Finder selects the bundle), `xdg-open
/// <parent folder>` elsewhere, as a process on its own thread.
pub struct SystemRevealer;

impl Revealer for SystemRevealer {
    fn reveal(&self, path: &str) {
        let (program, args) = reveal_command_for(path, cfg!(target_os = "macos"));
        std::thread::spawn(move || {
            // Nothing to tell when the file manager does not come.
            let _ = std::process::Command::new(program).args(args).status();
        });
    }
}

/// The command that shows `path` (`mac`: macOS).
pub fn reveal_command_for(path: &str, mac: bool) -> (&'static str, Vec<String>) {
    if mac {
        ("open", vec!["-R".to_owned(), path.to_owned()])
    } else {
        let parent = std::path::Path::new(path)
            .parent()
            .map_or_else(|| path.to_owned(), |p| p.to_string_lossy().into_owned());
        ("xdg-open", vec![parent])
    }
}

/// The window's state: the search, the format filter and the selection
/// (row ids). The rows come from [`AppPlugins`] each time.
#[derive(Debug, Clone, Default)]
pub struct PluginManagerModel {
    pub search: String,
    pub format: FormatFilter,
    /// Selected rows ([`ManagerRow::id`]).
    pub selection: BTreeSet<String>,
    /// The row the last plain or ⌘-click was on (⇧-click extends from it).
    anchor: Option<String>,
}

impl PluginManagerModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// The rows the table shows now.
    pub fn rows(&self, plugins: &AppPlugins) -> Vec<ManagerRow> {
        rows(plugins, self.format, &self.search)
    }

    /// The shown rows that are selected.
    pub fn selected(&self, plugins: &AppPlugins) -> Vec<ManagerRow> {
        self.rows(plugins)
            .into_iter()
            .filter(|r| self.selection.contains(&r.id()))
            .collect()
    }

    /// A click on row `id` (one of the shown rows).
    pub fn click(&mut self, id: &str, how: Click, plugins: &AppPlugins) {
        match how {
            Click::Plain => {
                self.selection = [id.to_owned()].into();
                self.anchor = Some(id.to_owned());
            }
            Click::Toggle => {
                if !self.selection.remove(id) {
                    self.selection.insert(id.to_owned());
                }
                self.anchor = Some(id.to_owned());
            }
            Click::Extend => {
                let ids: Vec<String> = self.rows(plugins).iter().map(ManagerRow::id).collect();
                let to = ids.iter().position(|i| i == id);
                let from = self
                    .anchor
                    .as_ref()
                    .and_then(|a| ids.iter().position(|i| i == a))
                    .or(to);
                if let (Some(a), Some(b)) = (from, to) {
                    let (lo, hi) = (a.min(b), a.max(b));
                    self.selection = ids[lo..=hi].iter().cloned().collect();
                }
                if self.anchor.is_none() {
                    self.anchor = Some(id.to_owned());
                }
            }
        }
    }

    /// "Scanning ‹done› of ‹total›…" while a scan runs, else ""
    /// (`M/PluginManagerModel.swift:80-84`).
    pub fn progress_text(plugins: &AppPlugins) -> String {
        let p = plugins.progress();
        if p.running {
            format!("Scanning {} of {}…", p.done, p.total)
        } else {
            String::new()
        }
    }

    /// Rescan Selected: something selected and no scan running.
    pub fn can_rescan_selected(&self, plugins: &AppPlugins) -> bool {
        !plugins.progress().running && !self.selected(plugins).is_empty()
    }

    /// Rescan All: no scan running.
    pub fn can_rescan_all(plugins: &AppPlugins) -> bool {
        !plugins.progress().running
    }

    /// Clear Blocklist: a selected row with a plug-in is blocklisted and
    /// no scan runs.
    pub fn can_clear_blocklist(&self, plugins: &AppPlugins) -> bool {
        !plugins.progress().running
            && self
                .selected(plugins)
                .iter()
                .any(|r| r.is_blocklisted() && r.has_plugin())
    }

    /// The first selected row's bundle (CLAP and VST3 only).
    pub fn reveal_path(&self, plugins: &AppPlugins) -> Option<String> {
        self.selected(plugins)
            .iter()
            .filter(|r| r.has_plugin())
            .find_map(|r| plugins.reveal_path(r.key()))
    }

    pub fn can_reveal(&self, plugins: &AppPlugins) -> bool {
        self.reveal_path(plugins).is_some()
    }

    /// Turns `row`'s plug-in on or off (a bundle with no plug-in has
    /// nothing to turn). When the catalog took it, every open document
    /// idles at once, so the plug-in unloads (its slots become "Disabled
    /// in Plugin Manager", keeping their settings) or its waiting slots
    /// load (owner rule). True when the catalog took it.
    pub fn set_enabled(
        &self,
        plugins: &mut AppPlugins,
        store: &mut DocumentStore,
        row: &ManagerRow,
        on: bool,
    ) -> bool {
        if !row.has_plugin() {
            return false;
        }
        let took = plugins.set_enabled(row.key(), on);
        if took {
            store.idle_every_document();
        }
        took
    }

    /// Scans the selected plug-ins again.
    pub fn rescan_selected(&self, plugins: &mut AppPlugins) {
        if !self.can_rescan_selected(plugins) {
            return;
        }
        let keys: Vec<PluginKey> = self
            .selected(plugins)
            .iter()
            .filter(|r| r.has_plugin())
            .map(ManagerRow::key)
            .collect();
        if !keys.is_empty() {
            plugins.rescan(keys);
        }
    }

    /// Scans every plug-in again.
    pub fn rescan_all(plugins: &mut AppPlugins) {
        if Self::can_rescan_all(plugins) {
            plugins.scan_all();
        }
    }

    /// Clears the selected blocklist entries in one call; each bundle is
    /// scanned again. Nothing is asked while a scan runs (the core would
    /// refuse).
    pub fn clear_blocklist(&self, plugins: &mut AppPlugins) {
        if !self.can_clear_blocklist(plugins) {
            return;
        }
        let keys: Vec<PluginKey> = self
            .selected(plugins)
            .iter()
            .filter(|r| r.is_blocklisted() && r.has_plugin())
            .map(ManagerRow::key)
            .collect();
        plugins.clear_blocklist(keys);
    }

    /// Shows the first selected bundle in the file manager; false when
    /// there is none.
    pub fn reveal(&self, plugins: &AppPlugins, revealer: &dyn Revealer) -> bool {
        match self.reveal_path(plugins) {
            Some(p) => {
                revealer.reveal(&p);
                true
            }
            None => false,
        }
    }
}
