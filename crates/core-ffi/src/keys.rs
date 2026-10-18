//! Key bindings for the app (plan 4f Rulings 3–6): the registry's
//! commands as rows for menus and the Keys settings, and a [`Keymap`] the
//! app keeps for its whole run (the bindings, saved, loaded and reset;
//! what a key press runs; whether a keyboard slot clashes).

use crate::control::ModifiersRow;
use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use core_control::bindings::{
    self, COMMAND, CONTROL, Chord, MAX_JSON, OPTION, SHIFT, default_chords,
};
use core_control::registry::{COMMANDS, KeyContext};
use std::sync::{Arc, Mutex};

/// One command with its keys, for menus and the Keys settings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct BindingRow {
    /// Stable ID.
    pub id: String,
    /// Its menu (or, without a menu item, its group).
    pub menu: String,
    /// Its title.
    pub title: String,
    /// It has a menu item.
    pub menu_item: bool,
    /// Its keys are live only during step entry.
    pub step_entry: bool,
    /// Its keys now, as menus show them (`⇧⌘S`).
    pub keys: Vec<String>,
    /// Its default keys.
    pub defaults: Vec<String>,
    /// The first key's key equivalent for a menu item (AppKit's
    /// character), if it has a key.
    pub menu_key: Option<String>,
    /// The first key's modifiers.
    pub menu_modifiers: ModifiersRow,
}

fn mods_row(m: u8) -> ModifiersRow {
    ModifiersRow {
        shift: m & SHIFT != 0,
        control: m & CONTROL != 0,
        option: m & OPTION != 0,
        command: m & COMMAND != 0,
    }
}

fn mods_bits(m: &ModifiersRow) -> u8 {
    (if m.shift { SHIFT } else { 0 })
        | (if m.control { CONTROL } else { 0 })
        | (if m.option { OPTION } else { 0 })
        | (if m.command { COMMAND } else { 0 })
}

fn model(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

fn parse(keys: &[String]) -> Result<Vec<Chord>, VizError> {
    keys.iter()
        .map(|k| Chord::parse(k).map_err(model))
        .collect()
}

/// The app's key bindings (one for the whole app; Swift's `KeyBindings`
/// owns it). Thread-safe.
#[derive(uniffi::Object)]
pub struct Keymap {
    inner: Mutex<bindings::Keymap>,
}

#[uniffi::export]
impl Keymap {
    /// The registry's defaults.
    #[uniffi::constructor]
    pub fn new() -> Arc<Keymap> {
        Arc::new(Keymap {
            inner: Mutex::new(bindings::Keymap::default()),
        })
    }

    /// Every command with its keys, in registry order.
    pub fn rows(&self) -> Vec<BindingRow> {
        let k = lock(&self.inner);
        COMMANDS
            .iter()
            .map(|c| {
                let now = k.chords(c.id);
                BindingRow {
                    id: c.id.to_string(),
                    menu: c.menu.to_string(),
                    title: c.title.to_string(),
                    menu_item: c.menu_item,
                    step_entry: c.context == KeyContext::StepEntry,
                    keys: now.iter().map(Chord::text).collect(),
                    defaults: default_chords(c).iter().map(Chord::text).collect(),
                    menu_key: now.first().map(Chord::menu_key),
                    menu_modifiers: mods_row(now.first().map_or(0, |ch| ch.mods)),
                }
            })
            .collect()
    }

    /// The command a key press runs (plan 4f Ruling 4): during step entry
    /// its keys first, except the number row's when `reserve_numbers`;
    /// then the global keys. None for a key VizDAW does not bind.
    pub fn lookup(
        &self,
        key_code: u16,
        modifiers: ModifiersRow,
        step_entry: bool,
        reserve_numbers: bool,
    ) -> Option<String> {
        let chord = Chord::new(key_code, mods_bits(&modifiers))?;
        lock(&self.inner)
            .lookup(chord, step_entry, reserve_numbers)
            .map(str::to_string)
    }

    /// The global command a keyboard slot's key (`a`, `F5`) with
    /// `modifiers` (the slot's bitmask) would clash with (parent spec §5:
    /// such a slot is refused).
    pub fn slot_clash(&self, key: String, modifiers: u8) -> Option<String> {
        let chord = Chord::of_slot(&key, modifiers)?;
        lock(&self.inner)
            .holder(chord, KeyContext::Global)
            .map(str::to_string)
    }

    /// Gives command `id` exactly `keys`; a key another command (of the
    /// same context) has is refused naming it, unless `replace` takes it.
    pub fn bind(&self, id: String, keys: Vec<String>, replace: bool) -> Result<(), VizError> {
        ffi_guard(|| {
            let chords = parse(&keys)?;
            lock(&self.inner)
                .bind(&id, &chords, replace)
                .map_err(|e| model(e.to_string()))
        })
    }

    /// Command `id` back to its default keys (those still free).
    pub fn reset_one(&self, id: String) {
        lock(&self.inner).reset_one(&id);
    }

    /// Every command back to its defaults.
    pub fn reset(&self) {
        lock(&self.inner).reset();
    }

    /// The bindings document (only the changes).
    pub fn to_json(&self) -> String {
        lock(&self.inner).to_json()
    }

    /// Replaces the bindings with a document's (over the defaults); what
    /// it skipped. Not a bindings document: `Model` error, nothing
    /// changed.
    pub fn load_json(&self, json: String) -> Result<Vec<String>, VizError> {
        ffi_guard(|| {
            let (k, skipped) = bindings::Keymap::from_json(COMMANDS, &json).map_err(model)?;
            *lock(&self.inner) = k;
            Ok(skipped)
        })
    }

    /// Writes the bindings document to `path`.
    pub fn save_file(&self, path: String) -> Result<(), VizError> {
        ffi_guard(|| {
            let text = lock(&self.inner).to_json();
            std::fs::write(&path, text).map_err(|e| VizError::Io {
                message: format!("{path}: {e}"),
            })
        })
    }

    /// Reads a bindings document from `path` (at most 256 KiB); what it
    /// skipped.
    pub fn load_file(&self, path: String) -> Result<Vec<String>, VizError> {
        ffi_guard(|| {
            let io = |e: std::io::Error| VizError::Io {
                message: format!("{path}: {e}"),
            };
            let len = std::fs::metadata(&path).map_err(io)?.len();
            if len > MAX_JSON as u64 {
                return Err(model(format!(
                    "a key bindings file is at most {MAX_JSON} bytes"
                )));
            }
            let text = std::fs::read_to_string(&path).map_err(io)?;
            self.load_json(text)
        })
    }
}
