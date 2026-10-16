//! The process-wide plugin catalog (spec §8, §9): where plugins live and
//! which are unavailable. Every session's plugin host is told through
//! [`apply`], with its own marks (one catalog, many hosts).

use crate::guard::lock;
use core_host::PluginHost;
use core_host::catalog::{Catalog, HostMarks};
use std::sync::Mutex;

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
