//! The plug-in browser (Swift `PluginBrowserModel`,
//! `M/PluginBrowser.swift:8-54`; its view `S/PluginBrowserView.swift`): the
//! usable plug-ins of one kind, grouped by vendor and filtered by a
//! search. The mixer makes one per presentation from
//! [`AppPlugins::browser`](crate::plugins::AppPlugins::browser), so the
//! rows stay as they were while it shows (a scan that ends meanwhile is
//! seen at the next opening).

use std::cmp::Ordering;

use vizdaw_ffi::{CatalogRowFfi, PluginKindTag};

use crate::plugins::format_label;

/// The empty list with no search (`S/PluginBrowserView.swift:20`).
pub const NO_PLUGINS: &str = "No plugins found. Scan in Window > Plugin Manager.";
/// The empty list with a search (`S/PluginBrowserView.swift:20`).
pub const NO_MATCH: &str = "No match.";
/// The search field's placeholder (`S/PluginBrowserView.swift:16`).
pub const SEARCH: &str = "Search";
/// The group of plug-ins with no vendor, listed last.
pub const OTHER_VENDOR: &str = "Other";

/// One presentation's list: its kind and the rows it was opened with.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginBrowserModel {
    pub kind: PluginKindTag,
    rows: Vec<CatalogRowFfi>,
}

impl PluginBrowserModel {
    pub fn new(kind: PluginKindTag, rows: Vec<CatalogRowFfi>) -> Self {
        PluginBrowserModel { kind, rows }
    }

    /// The rows it was opened with.
    pub fn rows(&self) -> &[CatalogRowFfi] {
        &self.rows
    }

    /// The groups the popover shows for `search` (see [`Self::group`]).
    pub fn groups(&self, search: &str) -> Vec<(String, Vec<CatalogRowFfi>)> {
        Self::group(&self.rows, search)
    }

    /// The line an empty list shows: "No match." with a search, else
    /// "No plugins found. …".
    pub fn empty_text(&self, search: &str) -> &'static str {
        if search.trim().is_empty() {
            NO_PLUGINS
        } else {
            NO_MATCH
        }
    }

    /// A row's format caption: "AU", "CLAP" or "VST3" (Swift showed a
    /// VST3 as "CLAP", `S/PluginBrowserView.swift:34`).
    pub fn format(row: &CatalogRowFfi) -> &'static str {
        format_label(row.plugin.format)
    }

    /// `rows` grouped by vendor: vendors by name in any case, "Other" (no
    /// vendor) last; plug-ins by name in any case. A search, trimmed,
    /// matches the name, the vendor or the id in any case
    /// (`M/PluginBrowser.swift:32-53`).
    pub fn group(rows: &[CatalogRowFfi], search: &str) -> Vec<(String, Vec<CatalogRowFfi>)> {
        let q = search.trim().to_lowercase();
        let matches = |r: &&CatalogRowFfi| {
            let p = &r.plugin;
            q.is_empty()
                || p.name.to_lowercase().contains(&q)
                || p.vendor.to_lowercase().contains(&q)
                || p.id.to_lowercase().contains(&q)
        };
        let mut groups: Vec<(String, Vec<CatalogRowFfi>)> = vec![];
        for r in rows.iter().filter(matches) {
            match groups.iter_mut().find(|(v, _)| *v == r.plugin.vendor) {
                Some((_, list)) => list.push(r.clone()),
                None => groups.push((r.plugin.vendor.clone(), vec![r.clone()])),
            }
        }
        groups.sort_by(|(a, _), (b, _)| match (a.is_empty(), b.is_empty()) {
            (false, true) => Ordering::Less,
            (true, false) => Ordering::Greater,
            _ => any_case(a, b),
        });
        for (vendor, list) in &mut groups {
            list.sort_by(|a, b| any_case(&a.plugin.name, &b.plugin.name));
            if vendor.is_empty() {
                *vendor = OTHER_VENDOR.into();
            }
        }
        groups
    }
}

/// By name in any case, then as written (so the order is stable).
fn any_case(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}
