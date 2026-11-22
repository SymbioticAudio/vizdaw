//! The app's actions, their key bindings and the menu bar.
//!
//! macOS shows the menus through `set_menus`; on Linux GPUI shows none, and
//! the shortcuts below are the way in this slice.
// slice 2: Linux menu bar

use gpui::{App, KeyBinding, Menu, MenuItem, actions};

use crate::text_input;

/// `secondary-KEY` as a static keystroke: Cmd on macOS, Ctrl elsewhere
/// (`KeyBinding::new` no longer resolves `secondary-` on this GPUI rev).
#[cfg(target_os = "macos")]
macro_rules! secondary {
    ($k:literal) => {
        concat!("cmd-", $k)
    };
}
#[cfg(not(target_os = "macos"))]
macro_rules! secondary {
    ($k:literal) => {
        concat!("ctrl-", $k)
    };
}
pub(crate) use secondary;

actions!(
    vizdaw,
    [
        NewDocument,
        NewDemo,
        OpenDocument,
        Save,
        SaveAs,
        CloseWindow,
        Quit,
        PlayStop,
        StopAndReturn,
    ]
);

/// The key context of a document window's root.
pub const WINDOW_CONTEXT: &str = "DocumentWindow";
/// Where unmodified keys act: in a document window, but never while a text
/// field has focus (typing a space must not start the transport).
pub const UNMODIFIED_KEYS: &str = "DocumentWindow && !TextInput";

pub fn bindings() -> Vec<KeyBinding> {
    let mut keys = vec![
        KeyBinding::new(secondary!("n"), NewDocument, None),
        KeyBinding::new(secondary!("o"), OpenDocument, None),
        KeyBinding::new(secondary!("q"), Quit, None),
        KeyBinding::new(secondary!("s"), Save, Some(WINDOW_CONTEXT)),
        KeyBinding::new(secondary!("shift-s"), SaveAs, Some(WINDOW_CONTEXT)),
        KeyBinding::new(secondary!("w"), CloseWindow, Some(WINDOW_CONTEXT)),
        // As the command registry binds them: `transport.play_stop` Space,
        // `transport.stop` (stop and return) S.
        KeyBinding::new("space", PlayStop, Some(UNMODIFIED_KEYS)),
        KeyBinding::new("s", StopAndReturn, Some(UNMODIFIED_KEYS)),
    ];
    keys.extend(text_input::bindings());
    keys
}

pub fn menus() -> Vec<Menu> {
    vec![
        Menu::new("VizDAW").items([MenuItem::action("Quit VizDAW", Quit)]),
        Menu::new("File").items([
            MenuItem::action("New", NewDocument),
            MenuItem::action("New Demo Project", NewDemo),
            MenuItem::action("Open\u{2026}", OpenDocument),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
            MenuItem::action("Save", Save),
            MenuItem::action("Save As\u{2026}", SaveAs),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Cut", text_input::Cut),
            MenuItem::action("Copy", text_input::Copy),
            MenuItem::action("Paste", text_input::Paste),
            MenuItem::action("Select All", text_input::SelectAll),
        ]),
        Menu::new("Transport").items([
            MenuItem::action("Play / Stop", PlayStop),
            MenuItem::action("Stop and Return", StopAndReturn),
        ]),
    ]
}

/// Binds the keys and sets the menu bar.
pub fn install(cx: &mut App) {
    cx.bind_keys(bindings());
    cx.set_menus(menus());
}
