//! The app's key bindings (Swift `KeyBindings`; editors spec §4f): every
//! registry command with its keys, kept in the preferences under `keys`
//! as the keymap's JSON object, saved to and loaded from `.vizkeys`
//! files, and reset. The bindings themselves (what a key runs, clashes,
//! the file's format) are `core_control::bindings::Keymap`. "The number
//! row is for locators" stays the preferences' own `reserve_number_row`.

use std::io::Read;
use std::path::Path;

use core_control::bindings::{BindError, Chord, Keymap, MAX_JSON};
use core_control::registry::{ArgValue, COMMANDS};
use core_control::{ControlError, ErrorCode};
use serde_json::Value;

use crate::commands::Ran;
use crate::prefs::Prefs;

/// The preferences key of the bindings document.
pub const KEYS_KEY: &str = "keys";

/// The commands `run_keys` handles.
pub const KEYS_COMMANDS: &[&str] = &["keys.save", "keys.load", "keys.reset"];

/// A key the Keys page recorded that another command has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingKey {
    pub id: String,
    pub key: String,
}

/// The bindings, the last message for the Keys page and a pending
/// Replace.
#[derive(Debug, Clone)]
pub struct KeyBindings {
    keymap: Keymap,
    message: Option<String>,
    pending: Option<PendingKey>,
}

impl KeyBindings {
    /// The bindings in the preferences; a missing or damaged document
    /// leaves the defaults.
    pub fn load(prefs: &Prefs) -> KeyBindings {
        let keymap = prefs
            .raw(KEYS_KEY)
            .filter(|v| v.is_object())
            .and_then(|v| Keymap::from_json(COMMANDS, &v.to_string()).ok())
            .map(|(k, _)| k)
            .unwrap_or_default();
        KeyBindings {
            keymap,
            message: None,
            pending: None,
        }
    }

    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// What the last record or load said, for the Keys page.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Says `message` on the Keys page (a save that failed).
    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    /// A recorded key another command has, waiting for Replace.
    pub fn pending(&self) -> Option<&PendingKey> {
        self.pending.as_ref()
    }

    /// The number row jumps to locators even during step entry.
    pub fn reserve_number_row(&self, prefs: &Prefs) -> bool {
        prefs.reserve_number_row()
    }

    pub fn set_reserve_number_row(&mut self, on: bool, prefs: &mut Prefs) {
        if let Err(e) = prefs.set_reserve_number_row(on) {
            self.message = Some(e);
        }
    }

    /// The Keys page recorded `chord` for `id`: it becomes the command's
    /// only key; one another command has waits in `pending` for
    /// `confirm_replace`.
    pub fn record(&mut self, id: &str, chord: &str, prefs: &mut Prefs) {
        self.pending = None;
        let c = match Chord::parse(chord) {
            Ok(c) => c,
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        match self.keymap.bind(id, &[c], false) {
            Ok(()) => {
                self.message = None;
                self.changed(prefs);
            }
            Err(e) => {
                // Replace is offered only for a key another command has.
                if matches!(e, BindError::Taken { .. }) {
                    self.pending = Some(PendingKey {
                        id: id.into(),
                        key: chord.into(),
                    });
                    self.message = Some(format!("{e}. Replace it?"));
                } else {
                    self.message = Some(e.to_string());
                }
            }
        }
    }

    /// Takes the pending key from the command that has it.
    pub fn confirm_replace(&mut self, prefs: &mut Prefs) {
        let Some(p) = self.pending.take() else {
            return;
        };
        self.message = None;
        if let Ok(c) = Chord::parse(&p.key) {
            if self.keymap.bind(&p.id, &[c], true).is_ok() {
                self.changed(prefs);
            }
        }
    }

    pub fn cancel_replace(&mut self) {
        self.pending = None;
        self.message = None;
    }

    pub fn reset(&mut self, id: &str, prefs: &mut Prefs) {
        self.keymap.reset_one(id);
        self.forget();
        self.changed(prefs);
    }

    pub fn reset_all(&mut self, prefs: &mut Prefs) {
        self.keymap.reset();
        self.forget();
        self.changed(prefs);
    }

    /// A reset or a load replaces what a pending Replace was about.
    fn forget(&mut self) {
        self.pending = None;
        self.message = None;
    }

    /// Writes the bindings to `path`.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, self.keymap.to_json()).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Replaces the bindings with the file's (a regular file of at most
    /// `MAX_JSON` bytes); `message` lists what it skipped. A file that
    /// is no bindings file changes nothing and says why.
    pub fn load_file(&mut self, path: &Path, prefs: &mut Prefs) -> Result<(), String> {
        self.pending = None;
        match read_keys(path).and_then(|t| Keymap::from_json(COMMANDS, &t)) {
            Ok((k, skipped)) => {
                self.keymap = k;
                self.message =
                    (!skipped.is_empty()).then(|| format!("Skipped: {}", skipped.join("; ")));
                self.changed(prefs);
                Ok(())
            }
            Err(e) => {
                self.message = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Writes the document to the preferences after a change.
    fn changed(&mut self, prefs: &mut Prefs) {
        let doc: Value = serde_json::from_str(&self.keymap.to_json()).unwrap_or(Value::Null);
        if let Err(e) = prefs.set_raw(KEYS_KEY, doc) {
            self.message = Some(e);
        }
    }
}

/// The text of a bindings file: only a regular file, never read past the
/// bound, so a device or a FIFO neither hangs nor fills memory.
fn read_keys(path: &Path) -> Result<String, String> {
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    let too_big = || format!("a key bindings file is at most {MAX_JSON} bytes");
    let meta = std::fs::metadata(path).map_err(io)?;
    if !meta.is_file() {
        return Err(format!("{} is not a key bindings file", path.display()));
    }
    if meta.len() > MAX_JSON as u64 {
        return Err(too_big());
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .map_err(io)?
        .take(MAX_JSON as u64 + 1)
        .read_to_string(&mut text)
        .map_err(io)?;
    if text.len() > MAX_JSON {
        return Err(too_big());
    }
    Ok(text)
}

/// Runs `keys.save`, `keys.load` or `keys.reset`; needs no document.
/// Without `path` a save or load is `dialog_required`.
pub fn run_keys(
    id: &str,
    args: &[(String, ArgValue)],
    keys: &mut KeyBindings,
    prefs: &mut Prefs,
) -> Result<Ran, ControlError> {
    let ran = Ran {
        undo: None,
        status: None,
        focus: None,
        open_mixer_window: false,
        open_modify_panel: false,
    };
    let path = || {
        args.iter().find_map(|(n, v)| match (n.as_str(), v) {
            ("path", ArgValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
    };
    let failed = |title: &str, e: String| {
        ControlError::new(ErrorCode::Failed, format!("{title} Failed: {e}"))
    };
    match id {
        "keys.save" => {
            let p = path().ok_or_else(|| ControlError::dialog("Save", &["path"]))?;
            keys.save(Path::new(&p))
                .map_err(|e| failed("Save Key Bindings", e))?;
            Ok(ran)
        }
        "keys.load" => {
            let p = path().ok_or_else(|| ControlError::dialog("Open", &["path"]))?;
            keys.load_file(Path::new(&p), prefs)
                .map_err(|e| failed("Load Key Bindings", e))?;
            Ok(ran)
        }
        "keys.reset" => {
            keys.reset_all(prefs);
            Ok(ran)
        }
        _ => Err(ControlError::new(
            ErrorCode::UnknownId,
            format!("There is no command {id}."),
        )),
    }
}
