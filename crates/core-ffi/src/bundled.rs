//! The EQ new channels get (plan 6g Task 3, Rulings 5–8): the bundled
//! Multi-Q, when the catalog has a usable copy and the "Put an EQ on new
//! channels" switch is on. Every session History is made by [`history`] and
//! armed by [`arm`] before each command that can build strips, so a change
//! of the switch or of the catalog applies to the next command.

use core_host::bundled::MULTIQ_CLAP_ID;
use core_host::catalog::PluginStatus;
use core_model::document::Document;
use core_model::history::History;
use core_model::mixer::{PluginFormat, PluginRef};
use std::sync::atomic::{AtomicBool, Ordering};

/// "Put an EQ on new channels" (Ruling 8): a machine preference the app
/// pushes at launch and on every change; on by default.
static NEW_CHANNEL_EQ: AtomicBool = AtomicBool::new(true);

/// Turns "Put an EQ on new channels" on or off. Never fails.
#[uniffi::export]
pub fn set_new_channel_eq(on: bool) {
    let _ = crate::guard::ffi_guard(|| {
        NEW_CHANNEL_EQ.store(on, Ordering::Relaxed);
        Ok(())
    });
}

/// Whether new channels get an EQ (the switch, not whether Multi-Q is
/// usable). Never fails; `true` if anything goes wrong.
#[uniffi::export]
pub fn new_channel_eq() -> bool {
    crate::guard::ffi_guard(|| Ok(NEW_CHANNEL_EQ.load(Ordering::Relaxed))).unwrap_or(true)
}

/// The test override of [`multiq`]: `None` reads the catalog; `Some(x)`
/// answers `x`.
#[cfg(feature = "test-hooks")]
static OVERRIDE: std::sync::Mutex<Option<Option<PluginRef>>> = std::sync::Mutex::new(None);

/// Makes [`multiq`] answer `plugin` instead of reading the catalog (test
/// hook); [`debug_clear_multiq`] undoes it.
#[cfg(feature = "test-hooks")]
pub fn debug_set_multiq(plugin: Option<PluginRef>) {
    *crate::guard::lock(&OVERRIDE) = Some(plugin);
}

/// Makes [`multiq`] read the catalog again (test hook).
#[cfg(feature = "test-hooks")]
pub fn debug_clear_multiq() {
    *crate::guard::lock(&OVERRIDE) = None;
}

/// Multi-Q's catalog reference when usable (Ruling 7): the entry the host
/// uses for `(Clap, MULTIQ_CLAP_ID)` ([`core_host::catalog::Catalog::preferred`])
/// when its status is OK or not validated (so neither disabled, missing nor
/// blocklisted) and its bundle is under [`core_host::bundled::dir`], or no
/// bundled dir exists. Nothing is loaded to decide this.
pub fn multiq() -> Option<PluginRef> {
    #[cfg(feature = "test-hooks")]
    if let Some(o) = crate::guard::lock(&OVERRIDE).clone() {
        return o;
    }
    let dir = core_host::bundled::dir();
    crate::catalog::with(|c| {
        let e = c.preferred(PluginFormat::Clap, MULTIQ_CLAP_ID)?;
        let usable = matches!(
            c.status_of(e),
            PluginStatus::Ok | PluginStatus::NotValidated
        );
        let placed = dir.as_ref().is_none_or(|d| e.source.path.starts_with(d));
        (usable && placed).then(|| e.plugin.clone())
    })
}

/// The EQ new strips get now: [`multiq`] when the switch is on.
pub fn new_strip_eq() -> Option<PluginRef> {
    if new_channel_eq() { multiq() } else { None }
}

/// A session History over `doc`, armed ([`arm`]).
pub fn history(doc: Document) -> History {
    let mut h = History::new(doc);
    arm(&mut h);
    h
}

/// Tells `h` which EQ the strips its next commands build get
/// ([`new_strip_eq`]). Takes the catalog's lock (a leaf lock: nothing
/// under it takes a session's).
pub fn arm(h: &mut History) {
    h.set_new_strip_eq(new_strip_eq());
}
