//! The EQ new channels get (plan 6g Task 3, Rulings 5–8; plan 6g-2
//! Rulings 3–5): the user's chosen effect, else the bundled Multi-Q, when
//! the catalog has a usable copy and the "Put an EQ on new channels"
//! switch is on. Every session History is made by [`history`] and
//! armed by [`arm`] before each command that can build strips, so a change
//! of the switch or of the catalog applies to the next command.
//!
//! Also "Install Multi-Q for other apps" (plan 6g Task 4, Ruling 9): a copy
//! of the bundled AU into a Components folder, only when asked.

use crate::error::VizError;
use crate::snapshot::PluginFormatTag;
use core_host::PluginKind;
use core_host::bundled::{MULTIQ_AU_BUNDLE, MULTIQ_CLAP_BUNDLE, MULTIQ_CLAP_ID};
use core_host::catalog::PluginStatus;
use core_model::document::Document;
use core_model::history::History;
use core_model::mixer::{PluginFormat, PluginRef};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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

/// The user's choice of EQ for new channels (Ruling 5); None: Multi-Q.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DefaultEqRow {
    /// AU or CLAP.
    pub format: PluginFormatTag,
    /// The plug-in's catalog ID.
    pub id: String,
    /// The name the app showed when it was chosen.
    pub name: String,
}

/// The choice (plan 6g-2 Ruling 5): a machine preference the app pushes at
/// launch and on every change; `None`: Multi-Q.
static CHOSEN: Mutex<Option<DefaultEqRow>> = Mutex::new(None);

/// Sets the EQ new channels get; `None`: Multi-Q. Never fails.
#[uniffi::export]
pub fn set_default_eq(choice: Option<DefaultEqRow>) {
    let _ = crate::guard::ffi_guard(|| {
        *crate::guard::lock(&CHOSEN) = choice;
        Ok(())
    });
}

/// The EQ chosen for new channels; `None`: Multi-Q. Never fails.
#[uniffi::export]
pub fn default_eq() -> Option<DefaultEqRow> {
    crate::guard::ffi_guard(|| Ok(chosen())).unwrap_or(None)
}

/// Ruling 4's Settings sentence; None when the choice is usable, unset, or
/// the switch is off. Never fails.
#[uniffi::export]
pub fn default_eq_note() -> Option<String> {
    crate::guard::ffi_guard(|| {
        Ok(match resolve() {
            Resolved::Fallback {
                name,
                multiq: Some(_),
            } => Some(fallback_sentence(&name)),
            Resolved::Fallback { name, multiq: None } => Some(format!(
                "{name} is not available, and neither is Multi-Q, so new channels get no EQ."
            )),
            _ => None,
        })
    })
    .unwrap_or(None)
}

fn chosen() -> Option<DefaultEqRow> {
    crate::guard::lock(&CHOSEN).clone()
}

/// The longest chosen name a note shows, in characters (Ruling 3).
const NOTE_NAME_CHARS: usize = 100;

/// `name` cut to [`NOTE_NAME_CHARS`] characters, with "…" when cut.
fn short_name(name: &str) -> String {
    let mut short: String = name.chars().take(NOTE_NAME_CHARS).collect();
    if short.len() < name.len() {
        short.push('…');
    }
    short
}

fn fallback_sentence(name: &str) -> String {
    format!("{name} is not available, so new channels get Multi-Q.")
}

/// What Ruling 3 makes of the switch, the choice and the catalog.
enum Resolved {
    /// The switch is off.
    Off,
    /// No choice: Multi-Q, if usable.
    Unset(Option<PluginRef>),
    /// The chosen plug-in, usable.
    Chosen(PluginRef),
    /// The choice is not usable: Multi-Q stands in, if usable.
    Fallback {
        /// The chosen plug-in's name, cut.
        name: String,
        /// [`multiq`].
        multiq: Option<PluginRef>,
    },
}

/// Ruling 3: nothing is loaded to decide.
fn resolve() -> Resolved {
    if !new_channel_eq() {
        return Resolved::Off;
    }
    let Some(choice) = chosen() else {
        return Resolved::Unset(multiq());
    };
    let format = PluginFormat::from(choice.format);
    let usable = crate::catalog::with(|c| {
        let e = c.preferred(format, &choice.id)?;
        let ok = e.kind == Some(PluginKind::Effect)
            && matches!(
                c.status_of(e),
                PluginStatus::Ok | PluginStatus::NotValidated
            );
        ok.then(|| e.plugin.clone())
    });
    match usable {
        Some(p) => Resolved::Chosen(p),
        None => Resolved::Fallback {
            name: short_name(&choice.name),
            multiq: multiq(),
        },
    }
}

/// The EQ new strips get now (Ruling 3): none with the switch off; the
/// chosen plug-in when the catalog says it is a usable effect; else
/// [`multiq`].
pub fn new_strip_eq() -> Option<PluginRef> {
    eq_and_note().0
}

/// Ruling 3's note, only when Multi-Q stands in for the choice.
pub fn fallback_note() -> Option<String> {
    eq_and_note().1
}

/// [`new_strip_eq`] and [`fallback_note`], from one look at the catalog.
fn eq_and_note() -> (Option<PluginRef>, Option<String>) {
    match resolve() {
        Resolved::Off => (None, None),
        Resolved::Unset(p) => (p, None),
        Resolved::Chosen(p) => (Some(p), None),
        Resolved::Fallback { name, multiq } => {
            let note = multiq.is_some().then(|| fallback_sentence(&name));
            (multiq, note)
        }
    }
}

/// A session History over `doc`, armed ([`arm`]).
pub fn history(doc: Document) -> History {
    let mut h = History::new(doc);
    arm(&mut h);
    h
}

/// Tells `h` which EQ the strips its next commands build get
/// ([`new_strip_eq`]) and the note their steps carry ([`fallback_note`]).
/// Takes the catalog's lock (a leaf lock: nothing under it takes a
/// session's).
pub fn arm(h: &mut History) {
    let (eq, note) = eq_and_note();
    h.set_new_strip_eq(eq);
    h.set_new_strip_eq_note(note);
}

/// The name the app shows for the bundled EQ.
const MULTIQ_NAME: &str = "Multi-Q";

/// `Some("Multi-Q")` when `bundled::dir()/Multi-Q.clap` exists, else None
/// (Ruling 9: the switch needs the CLAP). Never fails.
#[uniffi::export]
pub fn bundled_eq_name() -> Option<String> {
    crate::guard::ffi_guard(|| {
        Ok(core_host::bundled::dir()
            .is_some_and(|d| d.join(MULTIQ_CLAP_BUNDLE).exists())
            .then(|| MULTIQ_NAME.to_string()))
    })
    .unwrap_or(None)
}

/// Whether `bundled::dir()/Multi-Q.component` exists (the Install button
/// needs it). Never fails.
#[uniffi::export]
pub fn bundled_au_present() -> bool {
    crate::guard::ffi_guard(|| Ok(bundled_au().is_some())).unwrap_or(false)
}

/// The bundled AU, when the build has one.
fn bundled_au() -> Option<PathBuf> {
    let p = core_host::bundled::dir()?.join(MULTIQ_AU_BUNDLE);
    p.is_dir().then_some(p)
}

/// Copies the bundled AU into `dest`, the Components folder (None:
/// `$HOME/Library/Audio/Plug-Ins/Components`), with [`install_from`]
/// (Ruling 9). Ok: the installed bundle's path. Err(VizError::Exists)
/// when something is there and `replace` is false. Under tests
/// (`VIZDAW_TEST_NO_LIBRARY=1`) a `dest` inside the real `$HOME/Library` is
/// refused before anything is touched. Never runs `auval` or registers
/// anything.
#[uniffi::export]
pub fn install_multiq(dest: Option<String>, replace: bool) -> Result<String, VizError> {
    crate::guard::ffi_guard(|| {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let dest = match (dest, &home) {
            (Some(d), _) => PathBuf::from(d),
            (None, Some(h)) => h.join("Library/Audio/Plug-Ins/Components"),
            (None, None) => {
                return Err(io_failed("no home folder"));
            }
        };
        let no_library =
            cfg!(test) || std::env::var_os("VIZDAW_TEST_NO_LIBRARY").is_some_and(|v| v == "1");
        if no_library
            && let Some(h) = &home
            && dest.starts_with(h.join("Library"))
        {
            return Err(VizError::Model {
                message: format!(
                    "Tests may not install into the real Library: {}.",
                    dest.display()
                ),
            });
        }
        let src = bundled_au().ok_or_else(|| VizError::Model {
            message: "Multi-Q is not in this build of VizDAW.".into(),
        })?;
        install_from(&src, &dest, replace).map(|p| p.display().to_string())
    })
}

/// "Multi-Q could not be installed: <why>." as an `Io` error.
fn io_failed(why: impl std::fmt::Display) -> VizError {
    VizError::Io {
        message: format!("Multi-Q could not be installed: {why}."),
    }
}

/// "Something that is not Multi-Q is in the way" for a symlink at `p`.
fn in_the_way(p: &Path) -> VizError {
    VizError::Model {
        message: format!(
            "Something that is not Multi-Q is in the way: {}.",
            p.display()
        ),
    }
}

/// Copies bundle `src` into folder `dest_dir` under its own name (Ruling 9):
/// lstat the target (a symlink is refused, never followed; anything else
/// without `replace` is `Exists`), create the folder, copy into a hidden
/// staging name (symlinks inside `src` refused), move an old bundle aside,
/// rename the copy into place, then remove the old one. A failure removes
/// the staging copy and puts the old bundle back. Returns the target path.
pub fn install_from(src: &Path, dest_dir: &Path, replace: bool) -> Result<PathBuf, VizError> {
    let name = src
        .file_name()
        .ok_or_else(|| io_failed(format!("{} has no name", src.display())))?;
    let target = dest_dir.join(name);
    let present = match fs::symlink_metadata(&target) {
        Ok(m) if m.file_type().is_symlink() => return Err(in_the_way(&target)),
        Ok(_) => true,
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => return Err(io_failed(e)),
    };
    if present && !replace {
        return Err(VizError::Exists {
            path: target.display().to_string(),
        });
    }
    fs::create_dir_all(dest_dir).map_err(io_failed)?;
    let pid = std::process::id();
    let hidden = |tag: &str| {
        let mut n = std::ffi::OsString::from(".");
        n.push(name);
        n.push(format!(".vizdaw-{tag}-{pid}"));
        dest_dir.join(n)
    };
    let staging = hidden("partial");
    let old = hidden("old");
    // Leftovers of an earlier run of this process are ours.
    remove_ours(&staging);
    if let Err(e) = copy_tree(src, &staging) {
        remove_ours(&staging);
        return Err(e);
    }
    if present {
        remove_ours(&old);
        if let Err(e) = fs::rename(&target, &old) {
            remove_ours(&staging);
            return Err(io_failed(e));
        }
    }
    if let Err(e) = fs::rename(&staging, &target) {
        remove_ours(&staging);
        if present {
            let _ = fs::rename(&old, &target);
        }
        return Err(io_failed(e));
    }
    if present {
        remove_ours(&old);
    }
    Ok(target)
}

/// Removes `p` (one of our hidden names) without following a symlink.
fn remove_ours(p: &Path) {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => {
            let _ = fs::remove_dir_all(p);
        }
        Ok(_) => {
            let _ = fs::remove_file(p);
        }
        Err(_) => {}
    }
}

/// Copies folder or file `src` to `to`, refusing symlinks and anything
/// that is neither a file nor a folder.
fn copy_tree(src: &Path, to: &Path) -> Result<(), VizError> {
    let m = fs::symlink_metadata(src).map_err(io_failed)?;
    let ft = m.file_type();
    if ft.is_dir() {
        fs::create_dir(to).map_err(io_failed)?;
        for entry in fs::read_dir(src).map_err(io_failed)? {
            let entry = entry.map_err(io_failed)?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else if ft.is_file() {
        fs::copy(src, to).map(|_| ()).map_err(io_failed)
    } else {
        Err(io_failed(format!(
            "{} is a link or a special file",
            src.display()
        )))
    }
}
