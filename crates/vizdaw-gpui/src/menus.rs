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
use vizdaw_app::commands::{
    self, DOCUMENT_COMMANDS, MIXER_COMMANDS, TRANSPORT_COMMANDS, VIEW_COMMANDS,
};

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

/// A step entry key (`step.*`, T among them): runs as [`RunCommand`],
/// but a held key runs it again only for rest, tie and Delete Last Step
/// (the document window tells a repeat by the key not having come up).
#[derive(Clone, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = vizdaw, no_json)]
pub struct StepKey {
    pub id: SharedString,
}

/// A File menu item on the window's document (Swift `AppCommands`):
/// Import Audio…, Import MIDI File…, Open Co-Producer Copy…, Project
/// Settings…, Bounce…, the Export items, the report and Relink Media….
/// The document window handles it ([`crate::file_menu`]).
#[derive(Clone, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = vizdaw, no_json)]
pub struct FileCommand {
    pub id: SharedString,
}

/// A File menu item that needs no document: Create Demo Project with
/// Audio and Import Project's DAWproject… and AAF… (the app handles it,
/// [`crate::file_menu`]).
#[derive(Clone, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = vizdaw, no_json)]
pub struct AppFileCommand {
    pub id: SharedString,
}

/// A macOS menu item that cannot run now. It has no handler anywhere, so
/// AppKit's validation (an action's availability) shows it disabled; a
/// `RunCommand` item would show enabled whatever its command's state.
#[derive(Clone, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = vizdaw, no_json)]
pub struct UnavailableCommand {
    pub id: SharedString,
}

/// A Mixer menu item (Swift `AppCommands.mixerMenu`): it calls the
/// mixer model on the selected strips, as Swift's does, not the command
/// path (a key's `mixer.new_group` or `mixer.new_vca`).
#[derive(Clone, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = vizdaw, no_json)]
pub struct MixerMenuItem {
    pub op: MixerOp,
}

/// What a Mixer menu item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixerOp {
    /// A group of the selected strips.
    Group,
    /// Deletes the group every selected strip is in.
    Ungroup,
    /// A VCA of the selected strips.
    NewVca,
    /// Assigns the selected strips `vca` lacks to it.
    Assign { vca: u64 },
    /// Takes `vca`'s members among the selected strips out of it.
    Unassign { vca: u64 },
}

/// The key context of a document window's root.
pub const WINDOW_CONTEXT: &str = "DocumentWindow";
/// The editor commands (`commands::EDITOR_COMMANDS`) the GPUI app routes
/// to its menus, keys and control channel: the grid, the Piano Roll, the
/// List, closing the detail and Insert Event (the List's).
pub const ROUTED_EDITOR_COMMANDS: &[&str] = &[
    "editors.snap",
    "editors.piano_roll",
    "editors.list",
    "editors.close_detail",
    "editors.insert_event",
];

/// Whether the menus, keys and control channel route `id` to
/// `commands::run`: the document, editor, view and mixer commands
/// (`commands::VIEW_COMMANDS`, `commands::MIXER_COMMANDS`).
pub fn routed(id: &str) -> bool {
    DOCUMENT_COMMANDS.contains(&id)
        || ROUTED_EDITOR_COMMANDS.contains(&id)
        || VIEW_COMMANDS.contains(&id)
        || MIXER_COMMANDS.contains(&id)
}

/// Where unmodified keys act: in a document window, but never while a text
/// field has focus (typing a space must not start the transport).
pub const UNMODIFIED_KEYS: &str = "DocumentWindow && !TextInput";

/// The document window's key context identifier while step entry is on.
pub const STEP_CONTEXT: &str = "StepEntry";
/// The identifier while "reserve number row" keeps the row the locators'.
pub const RESERVE_CONTEXT: &str = "ReserveNumberRow";
/// Where step entry's keys act: in a document window with step entry on,
/// never while a text field has focus. Bound after the global keys, they
/// win the same keystroke (GPUI takes the later of two equally deep
/// bindings).
pub const STEP_KEYS: &str = "DocumentWindow && StepEntry && !TextInput";
/// Where step entry's number row acts: as [`STEP_KEYS`], unless the row
/// is reserved for the locators.
pub const STEP_NUMBER_KEYS: &str = "DocumentWindow && StepEntry && !ReserveNumberRow && !TextInput";

/// The key context of the document window's root: step entry on, the
/// number row reserved.
pub fn window_context(step_entry: bool, reserve_numbers: bool) -> String {
    let mut c = WINDOW_CONTEXT.to_owned();
    if step_entry {
        c.push(' ');
        c.push_str(STEP_CONTEXT);
    }
    if reserve_numbers {
        c.push(' ');
        c.push_str(RESERVE_CONTEXT);
    }
    c
}

/// The views that take Delete while they have the keyboard: during step
/// entry Delete is Delete Last Step over them too (Swift's key monitor
/// runs before the views).
const STEP_OVER: &[&str] = &[crate::piano_roll::CONTEXT, crate::list_editor::CONTEXT];

/// ⇧`digit` as macOS and Linux report it on a US layout: the shifted
/// character with ⇧ folded in (⌥⇧2 arrives as `alt-@`). Other layouts
/// shift the digits to other characters and are not covered.
fn us_shifted_digit(key: &str) -> Option<&'static str> {
    Some(match key {
        "1" => "!",
        "2" => "@",
        "3" => "#",
        "4" => "$",
        "5" => "%",
        "6" => "^",
        "7" => "&",
        "8" => "*",
        "9" => "(",
        "0" => ")",
        _ => return None,
    })
}

/// `keystroke` (`alt-shift-2`) as the platforms deliver it, when that
/// differs: ⇧ and a digit become the US shifted character.
fn delivered(keystroke: &str, chord: &Chord) -> Option<String> {
    if chord.mods & SHIFT == 0 || chord.mods & (COMMAND | CONTROL) != 0 {
        return None;
    }
    let (head, key) = keystroke.rsplit_once('-')?;
    let shifted = us_shifted_digit(key)?;
    let head = head.replace("shift", "");
    let head: Vec<&str> = head.split('-').filter(|p| !p.is_empty()).collect();
    let mut out = head.join("-");
    if !out.is_empty() {
        out.push('-');
    }
    out.push_str(shifted);
    Some(out)
}

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
    /// A Mixer menu item.
    Mixer {
        op: MixerOp,
        title: String,
        enabled: bool,
    },
    /// A submenu: its title, whether it opens, its items.
    Submenu {
        title: String,
        enabled: bool,
        items: Vec<ItemSpec>,
    },
    /// A heading that does nothing ("Selected Strips").
    Heading(String),
    Separator,
}

// Slice 1's items. Where the registry has the command its ID is used (for
// the keys shown).
const APP_QUIT: &str = "app.quit";
const FILE_NEW: &str = "file.new";
const FILE_CREATE_DEMO: &str = "file.create_demo";
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

/// The File menu items the app handles with no document ([`AppFileCommand`]).
pub const APP_FILE_COMMANDS: &[&str] = &[
    "file.create_demo_audio",
    "file.import_dawproject",
    "file.import_aaf",
];

/// The File menu items that show their keys (Swift gives these a
/// `keyboardShortcut`; the others show none whatever the keymap says).
const FILE_KEYED: &[&str] = &[
    "file.new",
    "file.open",
    "file.open_copy",
    "media.import",
    "file.close",
    "file.save",
    "file.save_as",
    "media.relink",
];

/// The File menu items bound from the keymap ([`FileCommand`]); New,
/// Open, Close, Save and Save As keep slice 1's bindings.
const FILE_BOUND: &[&str] = &["file.open_copy", "media.import", "media.relink"];

/// The action a menu item's `id` dispatches: the slice 1 action for a
/// slice 1 item, [`AppFileCommand`] or [`FileCommand`] for the other
/// File items, else [`RunCommand`].
pub fn action_for(id: &str) -> Box<dyn Action> {
    match id {
        APP_QUIT => Box::new(Quit),
        FILE_NEW => Box::new(NewDocument),
        FILE_CREATE_DEMO => Box::new(NewDemo),
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
        id if APP_FILE_COMMANDS.contains(&id) => Box::new(AppFileCommand {
            id: SharedString::new(id),
        }),
        id if commands::FILE_COMMANDS.contains(&id) => Box::new(FileCommand {
            id: SharedString::new(id),
        }),
        id => Box::new(RunCommand::new(SharedString::new(id))),
    }
}

/// The first key of `id` in `keymap`, as menus show it on this platform.
fn keys_of(id: &str, keymap: &Keymap) -> Option<String> {
    key_text(id, keymap, cfg!(target_os = "macos"))
}

/// The first key of `id` that works on the platform (`mac`: macOS), as
/// menus show it: off macOS a ⌃ chord a ⌘ chord owns ([`owner`]) does
/// nothing, so it is not shown.
pub fn key_text(id: &str, keymap: &Keymap, mac: bool) -> Option<String> {
    keymap
        .chords(id)
        .into_iter()
        .find(|c| owner(c, keymap, mac).is_none())
        .map(|c| c.text())
}

/// The commands whose chords the window binds ([`bindings`]): the
/// document, editor, view and mixer commands, and the transport commands
/// but the held keys and the slice 1 ones.
fn bound_ids() -> Vec<&'static str> {
    let transport = TRANSPORT_COMMANDS.iter().filter(|id| {
        !matches!(
            **id,
            "transport.play_stop"
                | "transport.stop"
                | "transport.rewind"
                | "transport.fast_forward"
        )
    });
    DOCUMENT_COMMANDS
        .iter()
        .chain(ROUTED_EDITOR_COMMANDS)
        .chain(VIEW_COMMANDS)
        .chain(MIXER_COMMANDS)
        .chain(std::iter::once(&"automation.suspend"))
        .chain(transport)
        .copied()
        .collect()
}

/// `mods` as the keyboard has them off macOS, where ⌘ and ⌃ are both
/// Ctrl.
fn ctrl_folded(mods: u8) -> u8 {
    if mods & (COMMAND | CONTROL) == 0 {
        mods
    } else {
        (mods & !CONTROL) | COMMAND
    }
}

/// The ⌘ chord that owns `chord`'s keystroke on the platform (`mac`:
/// macOS), if any: off macOS, where ⌘ and ⌃ are both Ctrl, a ⌃ chord
/// (⌃1 Arrange View) whose keystroke a bound command's ⌘ chord has (⌘1
/// Piano Roll) is that ⌘ chord's. `None` on macOS, for a chord with ⌘,
/// and for one no ⌘ chord shares. The key bindings, the menus' keys and
/// the channel's `ui.key` all follow it.
pub fn owner(chord: &Chord, keymap: &Keymap, mac: bool) -> Option<Chord> {
    if mac || chord.mods & CONTROL == 0 || chord.mods & COMMAND != 0 {
        return None;
    }
    let folded = ctrl_folded(chord.mods);
    bound_ids()
        .iter()
        .flat_map(|id| keymap.chords(id))
        .find(|c| c.mods & COMMAND != 0 && c.code == chord.code && ctrl_folded(c.mods) == folded)
}

/// File menu item `id`: the registry's title, its keys where Swift shows
/// them, enabled as Swift enables it on `front`.
fn file_item(id: &'static str, front: Option<&DocumentModel>, keymap: &Keymap) -> ItemSpec {
    ItemSpec::Command {
        id,
        title: registry::find(id).map_or(id, |c| c.title).to_owned(),
        keys: FILE_KEYED
            .contains(&id)
            .then(|| keys_of(id, keymap))
            .flatten(),
        enabled: commands::file_enabled(id, front),
        checked: None,
    }
}

/// The File menu (Swift `AppCommands`): the new-item group, then Close,
/// Save and Save As, then the project's own items. Import Project opens
/// with no document; Export only with one.
fn file_items(front: Option<&DocumentModel>, keymap: &Keymap) -> Vec<ItemSpec> {
    let item = |id| file_item(id, front, keymap);
    vec![
        item("file.new"),
        item("file.open"),
        item("file.open_copy"),
        item("media.import"),
        item("file.import_midi"),
        ItemSpec::Submenu {
            title: "Import Project".into(),
            enabled: true,
            items: vec![item("file.import_dawproject"), item("file.import_aaf")],
        },
        item("file.create_demo"),
        item("file.create_demo_audio"),
        ItemSpec::Separator,
        item("file.close"),
        item("file.save"),
        item("file.save_as"),
        ItemSpec::Separator,
        item("file.project_settings"),
        item("file.bounce"),
        // The most complete export comes first (Swift, plan 6d).
        ItemSpec::Submenu {
            title: "Export".into(),
            enabled: front.is_some(),
            items: vec![
                item("file.export_dawproject"),
                item("file.export_aaf"),
                item("file.export_midi"),
                item("file.export_midi_clip"),
                item("file.export_for_daw"),
            ],
        },
        item("file.interchange_report"),
        item("media.relink"),
    ]
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
    let checked = matches!(
        id,
        "automation.suspend"
            | "editors.snap"
            | "ui.view_arrange"
            | "ui.view_mix"
            | "ui.toggle_left"
            | "ui.toggle_bottom"
            | "ui.toggle_right"
    )
    .then(|| front.is_some_and(|d| commands::is_on(id, d)));
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
            menu("File", file_items(front, keymap)),
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
            // The bar's customizing comes with its sheet.
            menu(
                "View",
                vec![
                    cmd("ui.view_arrange"),
                    cmd("ui.view_mix"),
                    ItemSpec::Separator,
                    cmd("ui.toggle_left"),
                    cmd("ui.toggle_bottom"),
                    cmd("ui.toggle_right"),
                ],
            ),
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
            menu(
                "Automation",
                vec![
                    ItemSpec::Heading("Selected Strips".into()),
                    cmd("automation.mode.off"),
                    cmd("automation.mode.read"),
                    cmd("automation.mode.touch"),
                    cmd("automation.mode.latch"),
                    cmd("automation.mode.write"),
                    cmd("automation.mode.trim"),
                    ItemSpec::Separator,
                    cmd("automation.suspend"),
                ],
            ),
            menu("Mixer", mixer_items(front)),
            // The Plug-in Manager, Media Pool and Song Notes come with
            // their windows; macOS adds the window list.
            menu("Window", vec![cmd("window.mixer")]),
        ],
    }
}

/// The Mixer menu (Swift `AppCommands.mixerMenu`): Group, Ungroup, New
/// VCA, Assign to VCA ▸ (each VCA, then New VCA) and Remove from VCA ▸,
/// on the selected strips; each is off where its command would be
/// refused. None has a key.
fn mixer_items(front: Option<&DocumentModel>) -> Vec<ItemSpec> {
    let mixer = front.and_then(|d| d.mixer().map(|m| (m, d)));
    let strips = mixer.map_or_else(Vec::new, |(m, d)| m.selected_members(d));
    let item = |op, title: &str, enabled| ItemSpec::Mixer {
        op,
        title: title.to_owned(),
        enabled,
    };
    let new_vca = || item(MixerOp::NewVca, "New VCA", mixer.is_some());
    let vcas = mixer.map_or(&[][..], |(m, _)| m.vcas());
    let mut assign: Vec<ItemSpec> = vcas
        .iter()
        .map(|v| {
            let add = mixer.map_or_else(Vec::new, |(m, _)| m.assignable(&strips, v));
            item(MixerOp::Assign { vca: v.id }, &v.name, !add.is_empty())
        })
        .collect();
    if !vcas.is_empty() {
        assign.push(ItemSpec::Separator);
    }
    assign.push(new_vca());
    let holding: Vec<ItemSpec> = mixer
        .map(|(m, _)| m.vcas_holding(&strips))
        .unwrap_or_default()
        .into_iter()
        .map(|v| item(MixerOp::Unassign { vca: v.id }, &v.name, true))
        .collect();
    vec![
        item(
            MixerOp::Group,
            "Group",
            mixer.is_some_and(|(m, d)| m.can_group_selection(d)),
        ),
        item(
            MixerOp::Ungroup,
            "Ungroup",
            mixer.is_some_and(|(m, d)| m.selection_group(d).is_some()),
        ),
        ItemSpec::Separator,
        new_vca(),
        ItemSpec::Submenu {
            title: "Assign to VCA".into(),
            enabled: !strips.is_empty(),
            items: assign,
        },
        ItemSpec::Submenu {
            title: "Remove from VCA".into(),
            enabled: !holding.is_empty(),
            items: holding,
        },
    ]
}

/// Runs Mixer menu item `op` on `doc`'s mixer and its selected strips.
pub fn run_mixer_item(op: MixerOp, doc: &mut DocumentModel) {
    doc.with_mixer(|m, d| match op {
        MixerOp::Group => m.group_selection(d),
        MixerOp::Ungroup => m.ungroup_selection(d),
        MixerOp::NewVca => m.new_vca_from_selection(d, true),
        MixerOp::Assign { vca } | MixerOp::Unassign { vca } => {
            let Some(v) = m.vca(vca).cloned() else { return };
            let strips = m.selected_members(d);
            let on = matches!(op, MixerOp::Assign { .. });
            let which: Vec<u64> = if on {
                m.assignable(&strips, &v)
            } else {
                strips
                    .into_iter()
                    .filter(|s| v.members.contains(s))
                    .collect()
            };
            m.assign(d, &which, vca, on);
        }
    });
}

/// The Transport menu: every transport command with a menu item, in
/// registry order, with Swift's dividers (after the markers, after Click,
/// after the field keys). Each item is enabled as its command says.
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
    // The File items Swift binds from the keymap (⇧⌘I Import Audio…).
    for &id in FILE_BOUND {
        for chord in keymap.chords(id) {
            if chord.mods & (COMMAND | CONTROL) == 0 {
                continue;
            }
            let Some(k) = keystroke(&chord) else { continue };
            keys.push(KeyBinding::new(
                interned(k),
                FileCommand {
                    id: SharedString::new_static(id),
                },
                Some(WINDOW_CONTEXT),
            ));
        }
    }
    keys.extend(text_input::bindings());
    keys.extend(crate::list_editor::bindings());
    keys.extend(crate::piano_roll::bindings());
    // Rewind and Fast Forward are held keys: the window's key handler
    // steps them per key-down and lands on key-up. Their keypad twins
    // (Keypad- and Keypad+) are not bound: GPUI does not tell the keypad
    // from the main keys, and the main - and + are not theirs.
    let ids = bound_ids();
    for &id in &ids {
        for chord in keymap.chords(id) {
            if bindings::key_name(chord.code).is_some_and(|n| n.starts_with("Keypad")) {
                continue;
            }
            let Some(k) = keystroke(&chord) else { continue };
            // Off macOS a ⌃ chord a ⌘ chord owns is left to it.
            if owner(&chord, keymap, cfg!(target_os = "macos")).is_some() {
                continue;
            }
            // A chord without ⌘ or ⌃ (bare, ⇧ or ⌥) waits while a text
            // field has the keyboard, which types it (Swift's bare-key
            // rule, as the control channel's `bound_command`).
            let context = if chord.mods & (COMMAND | CONTROL) == 0 {
                UNMODIFIED_KEYS
            } else {
                WINDOW_CONTEXT
            };
            if let Some(alias) = delivered(&k, &chord) {
                keys.push(KeyBinding::new(
                    interned(alias),
                    RunCommand::new(id),
                    Some(context),
                ));
            }
            keys.push(KeyBinding::new(
                interned(k),
                RunCommand::new(id),
                Some(context),
            ));
        }
    }
    keys.extend(step_bindings(keymap));
    keys
}

/// Step entry's keys, after the global ones: T (Step Entry) wherever bare
/// keys act, the rest only while step entry is on ([`STEP_KEYS`]; the
/// number row [`STEP_NUMBER_KEYS`]), Delete also over the views that take
/// it. Keypad twins are not bound: GPUI does not tell the keypad from the
/// main keys.
fn step_bindings(keymap: &Keymap) -> Vec<KeyBinding> {
    let mut keys = Vec::new();
    for &id in vizdaw_app::step_entry::STEP_COMMANDS {
        let global = registry::find(id).is_some_and(|c| c.context == registry::KeyContext::Global);
        for chord in keymap.chords(id) {
            if bindings::key_name(chord.code).is_some_and(|n| n.starts_with("Keypad")) {
                continue;
            }
            let Some(k) = keystroke(&chord) else { continue };
            let step = || StepKey {
                id: SharedString::new_static(id),
            };
            let bare = chord.mods & (COMMAND | CONTROL) == 0;
            let context = match (global, bare) {
                (true, true) => UNMODIFIED_KEYS.to_owned(),
                (true, false) => WINDOW_CONTEXT.to_owned(),
                (false, _) if chord.number_row() => STEP_NUMBER_KEYS.to_owned(),
                (false, _) => STEP_KEYS.to_owned(),
            };
            let k = interned(k);
            keys.push(KeyBinding::new(k, step(), Some(&context)));
            if !global && k == "backspace" {
                for over in STEP_OVER {
                    let context = format!("{STEP_CONTEXT} > {over} && !TextInput");
                    keys.push(KeyBinding::new(k, step(), Some(&context)));
                }
            }
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
        .map(|m| Menu::new(m.title.clone()).items(m.items.iter().map(menu_item)))
        .collect()
}

/// An unavailable item: an action with no handler, shown disabled.
fn unavailable_item(title: &str) -> MenuItem {
    MenuItem::action(
        title.to_owned(),
        UnavailableCommand {
            id: SharedString::new_static(""),
        },
    )
    .disabled(true)
}

fn menu_item(i: &ItemSpec) -> MenuItem {
    match i {
        ItemSpec::Separator => MenuItem::separator(),
        ItemSpec::Heading(title) => unavailable_item(title),
        ItemSpec::Submenu {
            title,
            enabled,
            items,
        } => MenuItem::submenu(
            Menu::new(title.clone())
                .items(items.iter().map(menu_item))
                .disabled(!enabled),
        ),
        ItemSpec::Mixer { title, enabled, .. } if !enabled => unavailable_item(title),
        ItemSpec::Mixer { op, title, .. } => {
            MenuItem::action(title.clone(), MixerMenuItem { op: *op })
        }
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
}

/// The menu model the platform menus show now, and whether they show it
/// with every item off (a file panel is up).
struct ShownMenus {
    model: MenuModel,
    blocked: bool,
}

impl Global for ShownMenus {}

/// Shows `model` in the platform menus, unless they already show it.
/// While a file panel or the New Project format panel is up every item
/// is off (Swift's panels and its format alert were app-modal).
pub fn show(model: MenuModel, cx: &mut App) {
    let blocked = crate::file_panels::is_up(cx) || crate::file_menu::format_panel_up(cx);
    if cx
        .try_global::<ShownMenus>()
        .is_some_and(|s| s.model == model && s.blocked == blocked)
    {
        return;
    }
    let menus = if blocked {
        menus(&blocked_model(&model))
    } else {
        menus(&model)
    };
    cx.set_global(ShownMenus { model, blocked });
    cx.set_menus(menus);
}

/// Shows the current model again (a file panel came up or went away).
pub fn refresh(cx: &mut App) {
    if let Some(model) = cx.try_global::<ShownMenus>().map(|s| s.model.clone()) {
        show(model, cx);
    }
}

/// What the platform menus show now: the model as shown, with every item
/// off while a file panel is up.
pub fn shown(cx: &App) -> Option<MenuModel> {
    cx.try_global::<ShownMenus>().map(|s| {
        if s.blocked {
            blocked_model(&s.model)
        } else {
            s.model.clone()
        }
    })
}

/// `model` with every item off.
pub fn blocked_model(model: &MenuModel) -> MenuModel {
    fn off(i: &ItemSpec) -> ItemSpec {
        match i {
            ItemSpec::Command {
                id,
                title,
                keys,
                checked,
                ..
            } => ItemSpec::Command {
                id,
                title: title.clone(),
                keys: keys.clone(),
                enabled: false,
                checked: *checked,
            },
            ItemSpec::Mixer { op, title, .. } => ItemSpec::Mixer {
                op: *op,
                title: title.clone(),
                enabled: false,
            },
            ItemSpec::Submenu { title, items, .. } => ItemSpec::Submenu {
                title: title.clone(),
                enabled: false,
                items: items.iter().map(off).collect(),
            },
            other => other.clone(),
        }
    }
    MenuModel {
        menus: model
            .menus
            .iter()
            .map(|m| MenuSpec {
                title: m.title.clone(),
                items: m.items.iter().map(off).collect(),
            })
            .collect(),
    }
}

/// Binds the keys and sets the menus (no document yet).
pub fn install(keymap: &Keymap, cx: &mut App) {
    cx.bind_keys(bindings(keymap));
    show(menu_model(None, keymap), cx);
}
