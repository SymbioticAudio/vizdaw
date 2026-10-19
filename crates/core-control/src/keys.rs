//! Key strokes for `ui.key` (spec §5.5): named keys and typed text as the
//! macOS virtual key codes of the US ANSI layout plus the characters an
//! `NSEvent` carries. The app sends them into its own key window; menu key
//! equivalents (⌘S, Space for Play) fire as they would from a keyboard.

use serde_json::Value;

/// The most characters one `ui.key` may type.
pub const MAX_TEXT: usize = 256;

/// Modifier keys held during a click, drag or key stroke.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// ⇧
    pub shift: bool,
    /// ⌃
    pub control: bool,
    /// ⌥
    pub option: bool,
    /// ⌘
    pub command: bool,
}

impl Modifiers {
    /// Reads an array of names: `shift`, `control` (`ctrl`), `option`
    /// (`alt`), `command` (`cmd`). Null or absent is none.
    pub fn parse(v: Option<&Value>) -> Result<Modifiers, String> {
        let mut m = Modifiers::default();
        let Some(v) = v.filter(|v| !v.is_null()) else {
            return Ok(m);
        };
        let names = v.as_array().filter(|a| a.len() <= 4).ok_or(
            "`modifiers` must be an array of up to four of shift, control, option, command",
        )?;
        for n in names {
            match n.as_str() {
                Some("shift") => m.shift = true,
                Some("control" | "ctrl") => m.control = true,
                Some("option" | "alt") => m.option = true,
                Some("command" | "cmd") => m.command = true,
                _ => {
                    return Err(
                        "`modifiers` may hold only shift, control, option and command".into(),
                    );
                }
            }
        }
        Ok(m)
    }
}

/// One key press (down and up) as the app sends it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyStroke {
    /// The virtual key code (`NSEvent.keyCode`).
    pub key_code: u16,
    /// `NSEvent.characters`.
    pub characters: String,
    /// `NSEvent.charactersIgnoringModifiers`.
    pub unmodified: String,
    /// Modifiers held.
    pub modifiers: Modifiers,
}

/// Named keys: name, key code, the character AppKit reports (the keypad's
/// keys since plan 4f, which binds step entry to them).
const NAMED: [(&str, u16, char); 44] = [
    ("return", 36, '\r'),
    ("enter", 76, '\u{3}'),
    ("tab", 48, '\t'),
    ("space", 49, ' '),
    ("delete", 51, '\u{7f}'),
    ("escape", 53, '\u{1b}'),
    ("forward_delete", 117, '\u{f728}'),
    ("home", 115, '\u{f729}'),
    ("end", 119, '\u{f72b}'),
    ("page_up", 116, '\u{f72c}'),
    ("page_down", 121, '\u{f72d}'),
    ("left", 123, '\u{f702}'),
    ("right", 124, '\u{f703}'),
    ("down", 125, '\u{f701}'),
    ("up", 126, '\u{f700}'),
    ("f1", 122, '\u{f704}'),
    ("f2", 120, '\u{f705}'),
    ("f3", 99, '\u{f706}'),
    ("f4", 118, '\u{f707}'),
    ("f5", 96, '\u{f708}'),
    ("f6", 97, '\u{f709}'),
    ("f7", 98, '\u{f70a}'),
    ("f8", 100, '\u{f70b}'),
    ("f9", 101, '\u{f70c}'),
    ("f10", 109, '\u{f70d}'),
    ("f11", 103, '\u{f70e}'),
    ("f12", 111, '\u{f70f}'),
    ("keypad_0", 82, '0'),
    ("keypad_1", 83, '1'),
    ("keypad_2", 84, '2'),
    ("keypad_3", 85, '3'),
    ("keypad_4", 86, '4'),
    ("keypad_5", 87, '5'),
    ("keypad_6", 88, '6'),
    ("keypad_7", 89, '7'),
    ("keypad_8", 91, '8'),
    ("keypad_9", 92, '9'),
    ("keypad_decimal", 65, '.'),
    ("keypad_divide", 75, '/'),
    ("keypad_multiply", 67, '*'),
    ("keypad_minus", 78, '-'),
    ("keypad_plus", 69, '+'),
    ("keypad_equals", 81, '='),
    ("keypad_clear", 71, '\u{f739}'),
];

/// Unshifted characters of the US ANSI layout and their key codes.
const ANSI: [(char, u16); 47] = [
    ('a', 0),
    ('s', 1),
    ('d', 2),
    ('f', 3),
    ('h', 4),
    ('g', 5),
    ('z', 6),
    ('x', 7),
    ('c', 8),
    ('v', 9),
    ('b', 11),
    ('q', 12),
    ('w', 13),
    ('e', 14),
    ('r', 15),
    ('y', 16),
    ('t', 17),
    ('1', 18),
    ('2', 19),
    ('3', 20),
    ('4', 21),
    ('6', 22),
    ('5', 23),
    ('=', 24),
    ('9', 25),
    ('7', 26),
    ('-', 27),
    ('8', 28),
    ('0', 29),
    (']', 30),
    ('o', 31),
    ('u', 32),
    ('[', 33),
    ('i', 34),
    ('p', 35),
    ('l', 37),
    ('j', 38),
    ('\'', 39),
    ('k', 40),
    (';', 41),
    ('\\', 42),
    (',', 43),
    ('/', 44),
    ('n', 45),
    ('m', 46),
    ('.', 47),
    ('`', 50),
];

/// Shifted characters and the unshifted key they are on.
const SHIFTED: [(char, char); 21] = [
    ('!', '1'),
    ('@', '2'),
    ('#', '3'),
    ('$', '4'),
    ('%', '5'),
    ('^', '6'),
    ('&', '7'),
    ('*', '8'),
    ('(', '9'),
    (')', '0'),
    ('_', '-'),
    ('+', '='),
    ('{', '['),
    ('}', ']'),
    ('|', '\\'),
    (':', ';'),
    ('"', '\''),
    ('<', ','),
    ('>', '.'),
    ('?', '/'),
    ('~', '`'),
];

fn ansi(c: char) -> Option<u16> {
    ANSI.iter().find(|(k, _)| *k == c).map(|(_, code)| *code)
}

/// The key that types `c` and whether it needs shift; None for a
/// character no US key types (it is still sent, with key code 0xFFFF).
fn key_of(c: char) -> Option<(u16, char, bool)> {
    if c == ' ' {
        return Some((49, ' ', false));
    }
    if let Some(code) = ansi(c) {
        return Some((code, c, false));
    }
    if c.is_ascii_uppercase() {
        let lower = c.to_ascii_lowercase();
        return ansi(lower).map(|code| (code, lower, true));
    }
    SHIFTED
        .iter()
        .find(|(s, _)| *s == c)
        .and_then(|(_, base)| ansi(*base).map(|code| (code, *base, true)))
}

/// The stroke for `key` (a name from the table, or one character) with
/// `modifiers`.
pub fn named_key(key: &str, modifiers: Modifiers) -> Result<KeyStroke, String> {
    if let Some((_, code, ch)) = NAMED.iter().find(|(n, _, _)| *n == key) {
        return Ok(KeyStroke {
            key_code: *code,
            characters: ch.to_string(),
            unmodified: ch.to_string(),
            modifiers,
        });
    }
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return Err(format!(
            "unknown key {:?} (a single character or one of: {})",
            crate::protocol::truncate(key, 32),
            NAMED.iter().map(|n| n.0).collect::<Vec<_>>().join(", ")
        ));
    };
    let (code, base, shift) = key_of(c).unwrap_or((0xFFFF, c, false));
    Ok(KeyStroke {
        key_code: code,
        characters: c.to_string(),
        unmodified: if modifiers.command || modifiers.control {
            base.to_string()
        } else {
            c.to_string()
        },
        modifiers: Modifiers {
            shift: modifiers.shift || shift,
            ..modifiers
        },
    })
}

/// One stroke per character of `text` (at most [`MAX_TEXT`]); newlines
/// are Return, tabs are Tab.
pub fn text_strokes(text: &str) -> Result<Vec<KeyStroke>, String> {
    let n = text.chars().count();
    if n == 0 || n > MAX_TEXT {
        return Err(format!("`text` must hold 1 to {MAX_TEXT} characters"));
    }
    text.chars()
        .map(|c| match c {
            '\n' | '\r' => named_key("return", Modifiers::default()),
            '\t' => named_key("tab", Modifiers::default()),
            c if c.is_control() => Err(format!(
                "`text` holds the control character U+{:04X}",
                c as u32
            )),
            c => {
                let (code, _, shift) = key_of(c).unwrap_or((0xFFFF, c, false));
                Ok(KeyStroke {
                    key_code: code,
                    characters: c.to_string(),
                    unmodified: c.to_string(),
                    modifiers: Modifiers {
                        shift,
                        ..Modifiers::default()
                    },
                })
            }
        })
        .collect()
}
