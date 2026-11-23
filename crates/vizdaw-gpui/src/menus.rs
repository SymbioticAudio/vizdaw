//! The app's actions, their key bindings and the menus.
//!
//! The menus are a [`MenuModel`] built from the command registry and the
//! front document. macOS shows it through `set_menus`; elsewhere GPUI has
//! no menu bar, and each document window draws one from the same model
//! ([`crate::menu_bar`]).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use core_control::bindings::{self, COMMAND, CONTROL, Chord, Keymap, OPTION, SHIFT};
use core_control::registry;
use gpui::{Action, App, Global, KeyBinding, Menu, MenuItem, SharedString, actions};
use vizdaw_app::DocumentModel;
use vizdaw_app::commands::{self, DOCUMENT_COMMANDS, TRANSPORT_COMMANDS};

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

/// Runs the registry's document command `id` (one of
/// [`DOCUMENT_COMMANDS`]) on the window's document, as its menu item or
/// key does.
#[derive(Clone, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = vizdaw, no_json)]
pub struct RunCommand {
    pub id: SharedString,
}

impl RunCommand {
    pub fn new(id: impl Into<SharedString>) -> Self {
        RunCommand { id: id.into() }
    }
}

/// A macOS menu item that cannot run now. It has no handler anywhere, so
/// AppKit's validation (an action's availability) shows it disabled; a
/// `RunCommand` item would show enabled whatever its command's state.
#[derive(Clone, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = vizdaw, no_json)]
pub struct UnavailableCommand {
    pub id: SharedString,
}

/// The key context of a document window's root.
pub const WINDOW_CONTEXT: &str = "DocumentWindow";
/// The editor commands (`commands::EDITOR_COMMANDS`) the GPUI app routes
/// to its menus, keys and control channel: the grid, the Piano Roll, the
/// List, closing the detail, Insert Event (the List's) and the bottom
/// pane's toggle.
pub const ROUTED_EDITOR_COMMANDS: &[&str] = &[
    "editors.snap",
    "editors.piano_roll",
    "editors.list",
    "editors.close_detail",
    "editors.insert_event",
    "ui.toggle_bottom",
];

/// Where unmodified keys act: in a document window, but never while a text
/// field has focus (typing a space must not start the transport).
pub const UNMODIFIED_KEYS: &str = "DocumentWindow && !TextInput";

/// The menus, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    pub menus: Vec<MenuSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSpec {
    pub title: String,
    pub items: Vec<ItemSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemSpec {
    /// A command: a registry ID for document commands, else one of the
    /// slice 1 items' IDs below. `keys` is the first chord as menus show
    /// it. `checked` is a toggle's state (`None`: not a toggle).
    Command {
        id: &'static str,
        title: String,
        keys: Option<String>,
        enabled: bool,
        checked: Option<bool>,
    },
    Separator,
}

// Slice 1's items. Where the registry has the command its ID is used (for
// the keys shown); the slice 1 titles stay.
const APP_QUIT: &str = "app.quit";
const FILE_NEW: &str = "file.new";
const FILE_NEW_DEMO: &str = "file.new_demo";
const FILE_OPEN: &str = "file.open";
const FILE_CLOSE: &str = "file.close";
const FILE_SAVE: &str = "file.save";
const FILE_SAVE_AS: &str = "file.save_as";
const EDIT_CUT: &str = "edit.cut";
const EDIT_COPY: &str = "edit.copy";
const EDIT_PASTE: &str = "edit.paste";
const EDIT_SELECT_ALL: &str = "edit.select_all";
const PLAY_STOP: &str = "transport.play_stop";
const STOP_AND_RETURN: &str = "transport.stop";

/// The action a menu item's `id` dispatches: the slice 1 action for a
/// slice 1 item, else [`RunCommand`].
pub fn action_for(id: &str) -> Box<dyn Action> {
    match id {
        APP_QUIT => Box::new(Quit),
        FILE_NEW => Box::new(NewDocument),
        FILE_NEW_DEMO => Box::new(NewDemo),
        FILE_OPEN => Box::new(OpenDocument),
        FILE_CLOSE => Box::new(CloseWindow),
        FILE_SAVE => Box::new(Save),
        FILE_SAVE_AS => Box::new(SaveAs),
        EDIT_CUT => Box::new(text_input::Cut),
        EDIT_COPY => Box::new(text_input::Copy),
        EDIT_PASTE => Box::new(text_input::Paste),
        EDIT_SELECT_ALL => Box::new(text_input::SelectAll),
        PLAY_STOP => Box::new(PlayStop),
        STOP_AND_RETURN => Box::new(StopAndReturn),
        id => Box::new(RunCommand::new(SharedString::new(id))),
    }
}

/// The first key of `id` in `keymap`, as menus show it.
fn keys_of(id: &str, keymap: &Keymap) -> Option<String> {
    keymap.chords(id).first().map(Chord::text)
}

/// A slice 1 item: its slice 1 title, the registry's keys where it has the
/// command, else ⌘`key` (as slice 1 binds it); always enabled (whether its
/// action has a handler decides).
fn slice1(id: &'static str, title: &str, key: Option<&str>, keymap: &Keymap) -> ItemSpec {
    let keys = if registry::find(id).is_some() {
        keys_of(id, keymap)
    } else {
        key.and_then(|k| Chord::of_slot(k, COMMAND))
            .map(|c| c.text())
    };
    ItemSpec::Command {
        id,
        title: title.to_owned(),
        keys,
        enabled: true,
        checked: None,
    }
}

/// A document command: the registry's title (Undo and Redo: the
/// document's label) and keys, enabled when it can run on `front`.
fn command(id: &'static str, front: Option<&DocumentModel>, keymap: &Keymap) -> ItemSpec {
    let title = match (id, front) {
        ("edit.undo", Some(d)) => d.undo_title(),
        ("edit.redo", Some(d)) => d.redo_title(),
        _ => registry::find(id).map_or(id, |c| c.title).to_owned(),
    };
    let checked = match id {
        "editors.snap" => Some(front.is_some_and(|d| d.layout().snap.enabled)),
        "ui.toggle_bottom" => Some(front.is_some_and(|d| d.layout().views.current().bottom.shown)),
        _ => None,
    };
    ItemSpec::Command {
        id,
        title,
        keys: keys_of(id, keymap),
        enabled: front.is_some_and(|d| commands::enabled(id, d)),
        checked,
    }
}

fn menu(title: &str, items: Vec<ItemSpec>) -> MenuSpec {
    MenuSpec {
        title: title.to_owned(),
        items,
    }
}

/// The menus for the front document (`None`: no document window), in the
/// Swift app's order. Menus not built yet are left out.
pub fn menu_model(front: Option<&DocumentModel>, keymap: &Keymap) -> MenuModel {
    // Quit's key and title as the registry keeps them for macOS.
    let (quit_key, quit_title) = bindings::RESERVED
        .iter()
        .find(|(k, m, _)| *k == "q" && *m == COMMAND)
        .map_or(("q", "Quit VizDAW"), |(k, _, t)| (*k, *t));
    let cmd = |id| command(id, front, keymap);
    MenuModel {
        menus: vec![
            menu(
                "VizDAW",
                vec![slice1(APP_QUIT, quit_title, Some(quit_key), keymap)],
            ),
            menu(
                "File",
                vec![
                    slice1(FILE_NEW, "New", None, keymap),
                    slice1(FILE_NEW_DEMO, "New Demo Project", None, keymap),
                    slice1(FILE_OPEN, "Open\u{2026}", None, keymap),
                    ItemSpec::Separator,
                    slice1(FILE_CLOSE, "Close Window", None, keymap),
                    slice1(FILE_SAVE, "Save", None, keymap),
                    slice1(FILE_SAVE_AS, "Save As\u{2026}", None, keymap),
                ],
            ),
            menu(
                "Edit",
                vec![
                    cmd("edit.undo"),
                    cmd("edit.redo"),
                    ItemSpec::Separator,
                    slice1(EDIT_CUT, "Cut", Some("x"), keymap),
                    slice1(EDIT_COPY, "Copy", Some("c"), keymap),
                    slice1(EDIT_PASTE, "Paste", Some("v"), keymap),
                    slice1(EDIT_SELECT_ALL, "Select All", Some("a"), keymap),
                ],
            ),
            // The main views, the left and right panes and the bar's
            // customizing come with their views.
            menu("View", vec![cmd("ui.toggle_bottom")]),
            // The other detail editors, their windows and the nudges come
            // with their views.
            menu(
                "Editors",
                vec![
                    cmd("editors.piano_roll"),
                    cmd("editors.list"),
                    ItemSpec::Separator,
                    cmd("editors.close_detail"),
                    cmd("editors.insert_event"),
                    ItemSpec::Separator,
                    cmd("editors.snap"),
                ],
            ),
            menu(
                "Track",
                vec![
                    cmd("track.new_audio"),
                    cmd("track.new_instrument"),
                    cmd("track.new_midi"),
                    ItemSpec::Separator,
                    cmd("track.duplicate"),
                    cmd("track.delete"),
                    cmd("track.rename"),
                ],
            ),
            menu("Transport", transport_items(front, keymap)),
            // macOS adds the window list.
            menu("Window", vec![]),
        ],
    }
}

/// The Transport menu: every transport command with a menu item, in
/// registry order, with Swift's dividers (after the markers, after Click,
/// after the field keys). Recording, Overdub, Punch, Click, the punch
/// fields and arming are not handled yet, so they show disabled.
fn transport_items(front: Option<&DocumentModel>, keymap: &Keymap) -> Vec<ItemSpec> {
    let mut items = Vec::new();
    for c in registry::COMMANDS
        .iter()
        .filter(|c| c.id.starts_with("transport.") && c.menu_item)
    {
        items.push(command(c.id, front, keymap));
        if matches!(
            c.id,
            "transport.next_marker" | "transport.click" | "transport.go_punch"
        ) {
            items.push(ItemSpec::Separator);
        }
    }
    items
}

/// The GPUI key name of a registry key, if GPUI has one.
pub(crate) fn gpui_key(name: &str) -> Option<String> {
    let key = match name {
        "Return" | "Enter" => "enter",
        "Tab" => "tab",
        "Space" => "space",
        "Delete" => "backspace",
        "Escape" => "escape",
        "ForwardDelete" => "delete",
        "Home" => "home",
        "End" => "end",
        "PageUp" => "pageup",
        "PageDown" => "pagedown",
        "←" => "left",
        "→" => "right",
        "↓" => "down",
        "↑" => "up",
        "KeypadClear" | "Help" => return None,
        n if n.len() > 1 && n.starts_with('F') => return Some(n.to_ascii_lowercase()),
        // GPUI does not tell the keypad from the main keys.
        n => return Some(n.strip_prefix("Keypad").unwrap_or(n).to_ascii_lowercase()),
    };
    Some(key.to_owned())
}

/// A registry chord as a GPUI keystroke: ⌘ is `cmd-` on macOS and `ctrl-`
/// elsewhere (as `secondary!`), ⌃ `ctrl-`, ⌥ `alt-`, ⇧ `shift-`.
pub fn keystroke(chord: &Chord) -> Option<String> {
    let key = gpui_key(bindings::key_name(chord.code)?)?;
    let has = |bit| chord.mods & bit != 0;
    let mut s = String::new();
    if cfg!(target_os = "macos") {
        if has(CONTROL) {
            s.push_str("ctrl-");
        }
        if has(COMMAND) {
            s.push_str("cmd-");
        }
    } else if has(CONTROL) || has(COMMAND) {
        s.push_str("ctrl-");
    }
    if has(OPTION) {
        s.push_str("alt-");
    }
    if has(SHIFT) {
        s.push_str("shift-");
    }
    s.push_str(&key);
    Some(s)
}

/// `KeyBinding::new` wants a `&'static str` (`KeyBinding::load`, for
/// dynamic text, wants an `App`). Keystrokes come from a small, bounded
/// set (the registry's chords), so each is leaked once and reused.
fn interned(s: String) -> &'static str {
    static SEEN: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut seen = SEEN
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(&k) = seen.get(s.as_str()) {
        return k;
    }
    let k: &'static str = Box::leak(s.into_boxed_str());
    seen.insert(k);
    k
}

/// Slice 1's bindings, then one per chord of each document and transport
/// command (a chord without a modifier only where [`UNMODIFIED_KEYS`]
/// holds).
pub fn bindings(keymap: &Keymap) -> Vec<KeyBinding> {
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
    keys.extend(crate::list_editor::bindings());
    keys.extend(crate::piano_roll::bindings());
    // Rewind and Fast Forward are held keys: the window's key handler
    // steps them per key-down and lands on key-up. Their keypad twins
    // (Keypad- and Keypad+) are not bound: GPUI does not tell the keypad
    // from the main keys, and the main - and + are not theirs.
    let transport = TRANSPORT_COMMANDS.iter().filter(|id| {
        !matches!(
            **id,
            "transport.play_stop"
                | "transport.stop"
                | "transport.rewind"
                | "transport.fast_forward"
        )
    });
    for &id in DOCUMENT_COMMANDS
        .iter()
        .chain(ROUTED_EDITOR_COMMANDS)
        .chain(transport)
    {
        for chord in keymap.chords(id) {
            if bindings::key_name(chord.code).is_some_and(|n| n.starts_with("Keypad")) {
                continue;
            }
            let Some(k) = keystroke(&chord) else { continue };
            // A chord without ⌘ or ⌃ (bare, ⇧ or ⌥) waits while a text
            // field has the keyboard, which types it (Swift's bare-key
            // rule, as the control channel's `bound_command`).
            let context = if chord.mods & (COMMAND | CONTROL) == 0 {
                UNMODIFIED_KEYS
            } else {
                WINDOW_CONTEXT
            };
            keys.push(KeyBinding::new(
                interned(k),
                RunCommand::new(id),
                Some(context),
            ));
        }
    }
    keys
}

/// `model` as GPUI menus. An item that cannot run gets an action with no
/// handler, so macOS shows it disabled (its key is then not shown).
pub fn menus(model: &MenuModel) -> Vec<Menu> {
    model
        .menus
        .iter()
        .map(|m| {
            Menu::new(m.title.clone()).items(m.items.iter().map(|i| {
                match i {
                    ItemSpec::Separator => MenuItem::separator(),
                    ItemSpec::Command {
                        id, title, enabled, ..
                    } if !enabled => MenuItem::action(
                        title.clone(),
                        UnavailableCommand {
                            id: SharedString::new_static(id),
                        },
                    )
                    .disabled(true),
                    ItemSpec::Command {
                        id, title, checked, ..
                    } => MenuItem::Action {
                        name: title.clone().into(),
                        action: action_for(id),
                        os_action: None,
                        checked: checked.unwrap_or(false),
                        disabled: false,
                    },
                }
            }))
        })
        .collect()
}

/// The menu model the platform menus show now.
struct ShownMenus(MenuModel);

impl Global for ShownMenus {}

/// Shows `model` in the platform menus, unless they already show it.
pub fn show(model: MenuModel, cx: &mut App) {
    if cx.try_global::<ShownMenus>().is_some_and(|s| s.0 == model) {
        return;
    }
    let menus = menus(&model);
    cx.set_global(ShownMenus(model));
    cx.set_menus(menus);
}

/// Binds the keys and sets the menus (no document yet).
pub fn install(keymap: &Keymap, cx: &mut App) {
    cx.bind_keys(bindings(keymap));
    show(menu_model(None, keymap), cx);
}
