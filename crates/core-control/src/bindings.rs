//! Key bindings (editors spec §4f, plan 4f Rulings 3–5): every registry
//! command can be bound to keys. A binding is a [`Chord`] (a physical key
//! by its virtual key code, plus modifiers); the registry's shortcuts are
//! the defaults; the user's changes are kept as overrides and saved as a
//! small JSON document. Lookups know two contexts: step entry's keys win
//! over the global ones while step entry is on, unless a preference
//! reserves the number row for locators.

use crate::protocol::truncate;
use crate::registry::{COMMANDS, CommandSpec, KeyContext};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// ⇧ in [`Chord::mods`] (the keyboard slot's bitmask too).
pub const SHIFT: u8 = 1;
/// ⌃
pub const CONTROL: u8 = 2;
/// ⌥
pub const OPTION: u8 = 4;
/// ⌘
pub const COMMAND: u8 = 8;

/// Keys macOS or the app menu keeps, which a keyboard slot may not take
/// (it would shadow them in its document's window): key name, modifier
/// bitmask, title.
pub const RESERVED: [(&str, u8, &str); 7] = [
    ("q", COMMAND, "Quit VizDAW"),
    ("h", COMMAND, "Hide VizDAW"),
    ("h", COMMAND | OPTION, "Hide Others"),
    ("m", COMMAND, "Minimize"),
    (",", COMMAND, "Settings…"),
    ("w", COMMAND, "Close Window"),
    ("tab", COMMAND, "Switch Apps"),
];

/// The most keys one command may have.
pub const MAX_CHORDS: usize = 4;
/// The largest bindings document read (bytes).
pub const MAX_JSON: usize = 256 * 1024;
/// The bindings document's format version.
pub const VERSION: u64 = 1;

/// Display names by virtual key code (US ANSI positions). Letters are
/// upper case; the names are what menus, files and the Keys page show.
const KEYS: [(u16, &str); 79] = [
    (0, "A"),
    (1, "S"),
    (2, "D"),
    (3, "F"),
    (4, "H"),
    (5, "G"),
    (6, "Z"),
    (7, "X"),
    (8, "C"),
    (9, "V"),
    (11, "B"),
    (12, "Q"),
    (13, "W"),
    (14, "E"),
    (15, "R"),
    (16, "Y"),
    (17, "T"),
    (18, "1"),
    (19, "2"),
    (20, "3"),
    (21, "4"),
    (22, "6"),
    (23, "5"),
    (24, "="),
    (25, "9"),
    (26, "7"),
    (27, "-"),
    (28, "8"),
    (29, "0"),
    (30, "]"),
    (31, "O"),
    (32, "U"),
    (33, "["),
    (34, "I"),
    (35, "P"),
    (37, "L"),
    (38, "J"),
    (39, "'"),
    (40, "K"),
    (41, ";"),
    (42, "\\"),
    (43, ","),
    (44, "/"),
    (45, "N"),
    (46, "M"),
    (47, "."),
    (50, "`"),
    (36, "Return"),
    (76, "Enter"),
    (48, "Tab"),
    (49, "Space"),
    (51, "Delete"),
    (53, "Escape"),
    (117, "ForwardDelete"),
    (115, "Home"),
    (119, "End"),
    (116, "PageUp"),
    (121, "PageDown"),
    (123, "←"),
    (124, "→"),
    (125, "↓"),
    (126, "↑"),
    (122, "F1"),
    (120, "F2"),
    (99, "F3"),
    (118, "F4"),
    (96, "F5"),
    (97, "F6"),
    (98, "F7"),
    (100, "F8"),
    (101, "F9"),
    (109, "F10"),
    (103, "F11"),
    (111, "F12"),
    (82, "Keypad0"),
    (83, "Keypad1"),
    (84, "Keypad2"),
    (85, "Keypad3"),
    (86, "Keypad4"),
];

/// The rest of the keypad, and Help.
const KEYPAD: [(u16, &str); 13] = [
    (87, "Keypad5"),
    (88, "Keypad6"),
    (89, "Keypad7"),
    (91, "Keypad8"),
    (92, "Keypad9"),
    (65, "Keypad."),
    (75, "Keypad/"),
    (67, "Keypad*"),
    (78, "Keypad-"),
    (69, "Keypad+"),
    (81, "Keypad="),
    (71, "KeypadClear"),
    (114, "Help"),
];

/// Other spellings a chord's key may be written in.
const ALIASES: [(&str, u16); 7] = [
    ("left", 123),
    ("right", 124),
    ("down", 125),
    ("up", 126),
    ("esc", 53),
    ("backspace", 51),
    ("del", 117),
];

/// The number row's key codes, `1` … `9`, `0`.
pub const NUMBER_ROW: [u16; 10] = [18, 19, 20, 21, 23, 22, 26, 28, 25, 29];

fn keys() -> impl Iterator<Item = &'static (u16, &'static str)> {
    KEYS.iter().chain(KEYPAD.iter())
}

/// The display name of key `code`, if it is one VizDAW binds.
pub fn key_name(code: u16) -> Option<&'static str> {
    keys().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

fn key_code(name: &str) -> Option<u16> {
    keys()
        .find(|(_, n)| n.eq_ignore_ascii_case(name))
        .map(|(c, _)| *c)
        .or_else(|| {
            ALIASES
                .iter()
                .find(|(a, _)| a.eq_ignore_ascii_case(name))
                .map(|(_, c)| *c)
        })
}

/// A key and the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    /// The virtual key code (`NSEvent.keyCode`): a physical key, so a
    /// binding stays on its key whatever the keyboard layout types.
    pub code: u16,
    /// [`SHIFT`] | [`CONTROL`] | [`OPTION`] | [`COMMAND`].
    pub mods: u8,
}

impl Chord {
    /// `code` with `mods` (bits beyond the four modifiers dropped), or
    /// None for a key VizDAW does not bind.
    pub fn new(code: u16, mods: u8) -> Option<Chord> {
        key_name(code).map(|_| Chord {
            code,
            mods: mods & (SHIFT | CONTROL | OPTION | COMMAND),
        })
    }

    /// Reads `⌃⌥⇧⌘K` style text: modifier glyphs (each at most once, any
    /// order), then one key name (`A`, `1`, `Space`, `←`, `F5`,
    /// `Keypad1`; letters and names in any case).
    pub fn parse(text: &str) -> Result<Chord, String> {
        let shown = || truncate(text, 32);
        let mut mods = 0u8;
        let mut rest = text.trim();
        loop {
            let mut chars = rest.chars();
            let bit = match chars.next() {
                Some('⇧') => SHIFT,
                Some('⌃') => CONTROL,
                Some('⌥') => OPTION,
                Some('⌘') => COMMAND,
                _ => break,
            };
            if mods & bit != 0 {
                return Err(format!("{:?} repeats a modifier", shown()));
            }
            mods |= bit;
            rest = chars.as_str();
        }
        match key_code(rest) {
            Some(code) => Ok(Chord { code, mods }),
            None if rest.is_empty() => Err(format!("{:?} has no key", shown())),
            None => Err(format!("{:?} is not a key VizDAW binds", shown())),
        }
    }

    /// The chord as menus show it: `⌃⌥⇧⌘` then the key's name.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for (bit, glyph) in [(CONTROL, '⌃'), (OPTION, '⌥'), (SHIFT, '⇧'), (COMMAND, '⌘')] {
            if self.mods & bit != 0 {
                s.push(glyph);
            }
        }
        s.push_str(key_name(self.code).unwrap_or("?"));
        s
    }

    /// A keyboard slot's key (`a`, `F5`) and bitmask as a chord.
    pub fn of_slot(key: &str, modifiers: u8) -> Option<Chord> {
        let code = key_code(key.trim())?;
        Chord::new(code, modifiers)
    }

    /// The key is on the number row (not the keypad).
    pub fn number_row(&self) -> bool {
        NUMBER_ROW.contains(&self.code)
    }

    /// The character AppKit's menus use as this key's key equivalent
    /// (letters lower case; function keys as their private-use
    /// characters).
    pub fn menu_key(&self) -> String {
        let name = key_name(self.code).unwrap_or("");
        let c = match name {
            "Return" => '\r',
            "Enter" => '\u{3}',
            "Tab" => '\t',
            "Space" => ' ',
            "Delete" => '\u{8}',
            "Escape" => '\u{1b}',
            "ForwardDelete" => '\u{f728}',
            "Home" => '\u{f729}',
            "End" => '\u{f72b}',
            "PageUp" => '\u{f72c}',
            "PageDown" => '\u{f72d}',
            "←" => '\u{f702}',
            "→" => '\u{f703}',
            "↓" => '\u{f701}',
            "↑" => '\u{f700}',
            "KeypadClear" => '\u{f739}',
            "Help" => '\u{f746}',
            n => match (n.strip_prefix('F'), n.strip_prefix("Keypad")) {
                (Some(k), _) if !k.is_empty() => {
                    let k: u32 = k.parse().unwrap_or(1);
                    char::from_u32(0xf703 + k).unwrap_or(' ')
                }
                (_, Some(k)) => k.chars().next().unwrap_or(' '),
                _ => n.chars().next().map_or(' ', |c| c.to_ascii_lowercase()),
            },
        };
        c.to_string()
    }
}

/// Why a binding was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// No registry command has this ID.
    UnknownCommand(String),
    /// More than [`MAX_CHORDS`] keys.
    TooMany,
    /// The chord is another command's in the same context.
    Taken {
        /// The chord.
        chord: Chord,
        /// The command that has it.
        other: &'static str,
    },
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::UnknownCommand(id) => write!(f, "no command {:?}", truncate(id, 64)),
            BindError::TooMany => write!(f, "a command has at most {MAX_CHORDS} keys"),
            BindError::Taken { chord, other } => {
                let title = crate::registry::find(other).map_or(*other, |c| c.title);
                write!(f, "{} is the key of “{title}”", chord.text())
            }
        }
    }
}

/// The keys of every command: the registry's defaults plus the user's
/// overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    table: &'static [CommandSpec],
    /// Commands whose keys differ from their defaults (an empty list
    /// unbinds).
    overrides: BTreeMap<&'static str, Vec<Chord>>,
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap::new(COMMANDS)
    }
}

/// A command's default chords: its shortcut, then its extra keys.
pub fn default_chords(spec: &CommandSpec) -> Vec<Chord> {
    spec.shortcut
        .into_iter()
        .chain(spec.extra_keys.iter().copied())
        .filter_map(|s| Chord::parse(s).ok())
        .collect()
}

impl Keymap {
    /// The defaults of `table`.
    pub fn new(table: &'static [CommandSpec]) -> Keymap {
        Keymap {
            table,
            overrides: BTreeMap::new(),
        }
    }

    fn spec(&self, id: &str) -> Option<&'static CommandSpec> {
        self.table.iter().find(|c| c.id == id)
    }

    /// The command table this keymap binds.
    pub fn table(&self) -> &'static [CommandSpec] {
        self.table
    }

    /// The keys of command `id` now (empty for an unknown ID).
    pub fn chords(&self, id: &str) -> Vec<Chord> {
        match self.spec(id) {
            Some(c) => self
                .overrides
                .get(c.id)
                .cloned()
                .unwrap_or_else(|| default_chords(c)),
            None => Vec::new(),
        }
    }

    /// The command that has `chord` in `context`, if any.
    pub fn holder(&self, chord: Chord, context: KeyContext) -> Option<&'static str> {
        self.table
            .iter()
            .filter(|c| c.context == context)
            .find(|c| self.chords(c.id).contains(&chord))
            .map(|c| c.id)
    }

    /// The command a keyboard slot's key (`a`, `F5`) with `modifiers` (the
    /// slot's bitmask) clashes with: the global context first, then step
    /// entry's, since a slot can be pressed in either. A key macOS or the
    /// app menu keeps ([`RESERVED`], bound or not) clashes as its title.
    pub fn slot_clash(&self, key: &str, modifiers: u8) -> Option<&'static str> {
        let chord = Chord::of_slot(key, modifiers)?;
        self.holder(chord, KeyContext::Global)
            .or_else(|| self.holder(chord, KeyContext::StepEntry))
            .or_else(|| {
                RESERVED
                    .iter()
                    .find(|(k, m, _)| Chord::of_slot(k, *m) == Some(chord))
                    .map(|(_, _, title)| *title)
            })
    }

    /// The command a key press runs: with step entry on, its own keys
    /// first (except the number row's when `reserve_numbers` keeps the
    /// row for locators), then the global ones.
    pub fn lookup(
        &self,
        chord: Chord,
        step_entry: bool,
        reserve_numbers: bool,
    ) -> Option<&'static str> {
        if step_entry && !(reserve_numbers && chord.number_row()) {
            if let Some(id) = self.holder(chord, KeyContext::StepEntry) {
                return Some(id);
            }
        }
        self.holder(chord, KeyContext::Global)
    }

    /// Gives command `id` exactly `chords` (deduplicated, in order). A
    /// chord another command of the same context has is refused, unless
    /// `replace`, which takes it from that command.
    pub fn bind(&mut self, id: &str, chords: &[Chord], replace: bool) -> Result<(), BindError> {
        let spec = self
            .spec(id)
            .ok_or_else(|| BindError::UnknownCommand(id.to_string()))?;
        let mut want: Vec<Chord> = Vec::new();
        for c in chords {
            if !want.contains(c) {
                want.push(*c);
            }
        }
        if want.len() > MAX_CHORDS {
            return Err(BindError::TooMany);
        }
        let mut taken = Vec::new();
        for c in &want {
            if let Some(other) = self.holder(*c, spec.context).filter(|o| *o != spec.id) {
                if !replace {
                    return Err(BindError::Taken { chord: *c, other });
                }
                taken.push((other, *c));
            }
        }
        for (other, c) in taken {
            let left: Vec<Chord> = self.chords(other).into_iter().filter(|x| *x != c).collect();
            self.set(other, left);
        }
        self.set(spec.id, want);
        Ok(())
    }

    fn set(&mut self, id: &'static str, chords: Vec<Chord>) {
        let Some(spec) = self.spec(id) else { return };
        if chords == default_chords(spec) {
            self.overrides.remove(spec.id);
        } else {
            self.overrides.insert(spec.id, chords);
        }
    }

    /// Command `id` back to its default keys; a default another command
    /// took meanwhile stays with that command.
    pub fn reset_one(&mut self, id: &str) {
        let Some(spec) = self.spec(id) else { return };
        self.overrides.remove(spec.id);
        let mine: Vec<Chord> = default_chords(spec)
            .into_iter()
            .filter(|c| {
                self.table
                    .iter()
                    .filter(|o| o.id != spec.id && o.context == spec.context)
                    .all(|o| !self.chords(o.id).contains(c))
            })
            .collect();
        self.set(spec.id, mine);
    }

    /// Every command back to its defaults.
    pub fn reset(&mut self) {
        self.overrides.clear();
    }

    /// The commands whose keys differ from their defaults.
    pub fn changed(&self) -> Vec<&'static str> {
        self.overrides.keys().copied().collect()
    }

    /// The bindings document: `{"vizdaw_keys": 1, "bindings": {id:
    /// [chord text, ...]}}`, holding only the changed commands.
    pub fn to_json(&self) -> String {
        let mut b = Map::new();
        for (id, chords) in &self.overrides {
            b.insert(
                (*id).to_string(),
                Value::from(chords.iter().map(Chord::text).collect::<Vec<_>>()),
            );
        }
        json!({"vizdaw_keys": VERSION, "bindings": Value::Object(b)}).to_string()
    }

    /// Reads a bindings document over the defaults of `table`. Not a
    /// bindings document (not JSON, too large, another version) is an
    /// error; inside one, an unknown command, an unreadable key, a fifth
    /// key or a key another command already has is skipped and named in
    /// the returned list, so a file from another version still loads.
    pub fn from_json(
        table: &'static [CommandSpec],
        text: &str,
    ) -> Result<(Keymap, Vec<String>), String> {
        if text.len() > MAX_JSON {
            return Err(format!("a key bindings file is at most {MAX_JSON} bytes"));
        }
        let v: Value =
            serde_json::from_str(text).map_err(|e| format!("not a key bindings file: {e}"))?;
        if v.get("vizdaw_keys").and_then(Value::as_u64) != Some(VERSION) {
            return Err("not a VizDAW key bindings file (version 1)".into());
        }
        let Some(map) = v.get("bindings").and_then(Value::as_object) else {
            return Err("a key bindings file needs a `bindings` object".into());
        };
        let mut k = Keymap::new(table);
        let mut skipped = Vec::new();
        // First every listed command loses its default keys, so keys that
        // move between commands do not collide with the old owner.
        let mut wanted: Vec<(&'static str, Vec<Chord>)> = Vec::new();
        for (id, keys) in map {
            let Some(spec) = k.spec(id) else {
                skipped.push(format!("no command {:?}", truncate(id, 64)));
                continue;
            };
            let Some(list) = keys.as_array() else {
                skipped.push(format!("{}: the keys are not a list", spec.id));
                continue;
            };
            let mut chords = Vec::new();
            for key in list {
                let parsed = key
                    .as_str()
                    .ok_or_else(|| "a key is not text".to_string())
                    .and_then(Chord::parse);
                match parsed {
                    Ok(c) if chords.contains(&c) => {}
                    Ok(_) if chords.len() == MAX_CHORDS => {
                        skipped.push(format!("{}: more than {MAX_CHORDS} keys", spec.id));
                        break;
                    }
                    Ok(c) => chords.push(c),
                    Err(e) => skipped.push(format!("{}: {e}", spec.id)),
                }
            }
            k.set(spec.id, Vec::new());
            wanted.push((spec.id, chords));
        }
        for (id, chords) in wanted {
            let mut kept = Vec::new();
            for c in chords {
                let context = k.spec(id).map_or(KeyContext::Global, |s| s.context);
                match k.holder(c, context) {
                    Some(other) if other != id => {
                        skipped.push(format!("{id}: {} is already the key of {other}", c.text()))
                    }
                    _ => kept.push(c),
                }
            }
            k.set(id, kept);
        }
        Ok((k, skipped))
    }
}
