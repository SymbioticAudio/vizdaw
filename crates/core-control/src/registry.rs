//! The command registry (spec §6, plan 4f): every command of the app,
//! menu item or not, by a stable ID, with its menu (or group), title,
//! default keys, where its keys are live, parameters and the dialogs its
//! parameters answer. `command.run` runs these; [`crate::bindings`] binds
//! keys to them.

use crate::error::{ControlError, ErrorCode};
use crate::keys::{KeyStroke, Modifiers, named_key};
use crate::params::{MAX_PATH, MAX_STRING, Params};
use serde_json::{Map, Value, json};

/// A parameter's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// A string of at most [`MAX_STRING`] characters.
    Text,
    /// An absolute path ending in `.{0}`.
    Path(&'static str),
    /// true or false.
    Flag,
    /// One of these strings.
    Choice(&'static [&'static str]),
}

/// One parameter of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamSpec {
    /// Its name in `params`.
    pub name: &'static str,
    /// Its type.
    pub kind: ParamKind,
    /// It must be given.
    pub required: bool,
    /// One line for `commands.list`.
    pub help: &'static str,
}

/// A dialog a command opens from the menu; a programmatic call takes its
/// answers as parameters instead (spec §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogSpec {
    /// The dialog's title, as errors name it.
    pub name: &'static str,
    /// When the menu command opens it.
    pub when: &'static str,
    /// The menu command always opens it: without one of `answered_by`
    /// the call fails before it reaches the app.
    pub always: bool,
    /// The parameters that answer it.
    pub answered_by: &'static [&'static str],
}

/// Where a command's keys are live (plan 4f Ruling 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyContext {
    /// In every VizDAW window that shows a document.
    Global,
    /// Only while step entry is on; there it wins over a global key.
    StepEntry,
}

impl KeyContext {
    /// Its name in `commands.list`.
    pub fn name(self) -> &'static str {
        match self {
            KeyContext::Global => "global",
            KeyContext::StepEntry => "step_entry",
        }
    }
}

/// A command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    /// Stable ID (`transport.play_stop`).
    pub id: &'static str,
    /// The menu it is in (for a command without a menu item, the group the
    /// Keys settings list it under).
    pub menu: &'static str,
    /// Its menu title.
    pub title: &'static str,
    /// Its default shortcut, if any (shown in its menu item).
    pub shortcut: Option<&'static str>,
    /// More default keys (a keypad twin); never shown in menus.
    pub extra_keys: &'static [&'static str],
    /// It has a menu item (false: keys, the Keys settings and the channel
    /// only).
    pub menu_item: bool,
    /// Where its keys are live.
    pub context: KeyContext,
    /// It acts on a document (the target window's, else the front one's).
    pub document: bool,
    /// Its parameters.
    pub params: &'static [ParamSpec],
    /// The dialogs its parameters answer.
    pub dialogs: &'static [DialogSpec],
}

/// A validated `command.run` argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArgValue {
    /// A string (paths too).
    Text(String),
    /// A flag.
    Flag(bool),
}

const fn cmd(
    id: &'static str,
    menu: &'static str,
    title: &'static str,
    shortcut: Option<&'static str>,
    document: bool,
) -> CommandSpec {
    CommandSpec {
        id,
        menu,
        title,
        shortcut,
        extra_keys: &[],
        menu_item: true,
        context: KeyContext::Global,
        document,
        params: &[],
        dialogs: &[],
    }
}

const OPEN_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::Path("vizdaw"),
        required: false,
        help: "The .vizdaw package to open (answers the Open dialog).",
    },
    ParamSpec {
        name: "plugins",
        kind: ParamKind::Choice(&["normal", "off", "disable"]),
        required: false,
        help: "After a crash: open normally, with plugins off, or disable the plugins that were loading.",
    },
    ParamSpec {
        name: "restore_autosave",
        kind: ParamKind::Flag,
        required: false,
        help: "A newer autosave exists: true restores it, false opens the last save.",
    },
];

/// The dialogs `file.open` and `app.open` answer.
pub const OPEN_DIALOGS: &[DialogSpec] = &[
    DialogSpec {
        name: "Open",
        when: "always",
        always: true,
        answered_by: &["path"],
    },
    DialogSpec {
        name: "Open with Plugins Off?",
        when: "VizDAW quit unexpectedly while the project was open, or Shift is held",
        always: false,
        answered_by: &["plugins"],
    },
    DialogSpec {
        name: "Restore Autosave",
        when: "the project has an autosave newer than its last save",
        always: false,
        answered_by: &["restore_autosave"],
    },
];

const SAVE_PATH: &[ParamSpec] = &[ParamSpec {
    name: "path",
    kind: ParamKind::Path("vizdaw"),
    required: false,
    help: "Where to save (answers the Save dialog).",
}];

const KEYS_PATH: &[ParamSpec] = &[ParamSpec {
    name: "path",
    kind: ParamKind::Path("vizkeys"),
    required: false,
    help: "The key bindings file (answers the dialog).",
}];

/// `c` without a menu item: keys, the Keys settings and the channel only.
const fn key_only(c: CommandSpec) -> CommandSpec {
    CommandSpec {
        menu_item: false,
        ..c
    }
}

/// A numbered locator's command (plan 4f Ruling 7).
const fn locator(id: &'static str, title: &'static str, key: &'static str) -> CommandSpec {
    key_only(cmd(id, "Locators", title, Some(key), true))
}

const ON: &[ParamSpec] = &[ParamSpec {
    name: "on",
    kind: ParamKind::Flag,
    required: false,
    help: "Set on or off; absent toggles.",
}];

/// Every menu command, in menu order.
pub const COMMANDS: &[CommandSpec] = &[
    cmd("file.new", "File", "New", Some("⌘N"), false),
    CommandSpec {
        params: OPEN_PARAMS,
        dialogs: OPEN_DIALOGS,
        ..cmd("file.open", "File", "Open…", Some("⌘O"), false)
    },
    cmd(
        "file.create_demo",
        "File",
        "Create Demo Project",
        None,
        false,
    ),
    cmd(
        "file.create_demo_audio",
        "File",
        "Create Demo Project with Audio",
        None,
        false,
    ),
    cmd("file.close", "File", "Close", Some("⌘W"), true),
    CommandSpec {
        params: SAVE_PATH,
        dialogs: &[DialogSpec {
            name: "Save",
            when: "the document was never saved",
            always: false,
            answered_by: &["path"],
        }],
        ..cmd("file.save", "File", "Save", Some("⌘S"), true)
    },
    CommandSpec {
        params: SAVE_PATH,
        dialogs: &[DialogSpec {
            name: "Save",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..cmd("file.save_as", "File", "Save As…", Some("⇧⌘S"), true)
    },
    cmd("edit.undo", "Edit", "Undo", Some("⌘Z"), true),
    cmd("edit.redo", "Edit", "Redo", Some("⇧⌘Z"), true),
    cmd("window.mixer", "Window", "Mixer", Some("⌥⌘M"), true),
    cmd(
        "window.plugin_manager",
        "Window",
        "Plugin Manager",
        Some("⌥⌘P"),
        false,
    ),
    cmd(
        "editors.piano_roll",
        "Editors",
        "Piano Roll",
        Some("⌘1"),
        true,
    ),
    cmd("editors.list", "Editors", "List", Some("⌘2"), true),
    cmd(
        "editors.strip_chart",
        "Editors",
        "Strip Chart",
        Some("⌘3"),
        true,
    ),
    cmd(
        "editors.drum_grid",
        "Editors",
        "Drum Grid",
        Some("⌘4"),
        true,
    ),
    cmd(
        "editors.piano_roll_window",
        "Editors",
        "Open Piano Roll in Window",
        Some("⌥⌘1"),
        true,
    ),
    cmd(
        "editors.list_window",
        "Editors",
        "Open List in Window",
        Some("⌥⌘2"),
        true,
    ),
    cmd(
        "editors.strip_chart_window",
        "Editors",
        "Open Strip Chart in Window",
        Some("⌥⌘3"),
        true,
    ),
    cmd(
        "editors.drum_grid_window",
        "Editors",
        "Open Drum Grid in Window",
        Some("⌥⌘4"),
        true,
    ),
    cmd(
        "editors.close_detail",
        "Editors",
        "Close Detail Editor",
        None,
        true,
    ),
    cmd(
        "editors.insert_event",
        "Editors",
        "Insert Event",
        Some("⌘I"),
        true,
    ),
    cmd(
        "editors.nudge_earlier",
        "Editors",
        "Nudge Earlier",
        Some("⌥←"),
        true,
    ),
    cmd(
        "editors.nudge_later",
        "Editors",
        "Nudge Later",
        Some("⌥→"),
        true,
    ),
    cmd("editors.nudge_up", "Editors", "Nudge Up", Some("⌥↑"), true),
    cmd(
        "editors.nudge_down",
        "Editors",
        "Nudge Down",
        Some("⌥↓"),
        true,
    ),
    CommandSpec {
        params: ON,
        ..cmd("editors.snap", "Editors", "Snap to Grid", Some("G"), true)
    },
    cmd(
        "modify.select_by_rule",
        "Modify",
        "Select by Rule…",
        Some("⌃⌘S"),
        true,
    ),
    cmd(
        "modify.transpose",
        "Modify",
        "Transpose…",
        Some("⌃⌘T"),
        true,
    ),
    cmd("modify.quantize", "Modify", "Quantize…", Some("⌃⌘U"), true),
    cmd(
        "modify.change",
        "Modify",
        "Change Values…",
        Some("⌃⌘M"),
        true,
    ),
    cmd("modify.move", "Modify", "Move…", Some("⌃⌘V"), true),
    cmd("modify.again", "Modify", "Modify Again", Some("⌃⌘R"), true),
    CommandSpec {
        params: &[ParamSpec {
            name: "name",
            kind: ParamKind::Text,
            required: true,
            help: "The template to run (Modify > Templates).",
        }],
        ..cmd("modify.template", "Modify", "Templates", None, true)
    },
    CommandSpec {
        params: ON,
        ..cmd(
            "modify.exact_mode",
            "Modify",
            "Exact Mode",
            Some("⌃⌘E"),
            true,
        )
    },
    cmd(
        "sequence.make_reference",
        "Sequence",
        "Make Reference from Selection",
        Some("⌥⌘R"),
        true,
    ),
    cmd(
        "sequence.back",
        "Sequence",
        "Back to Parent Sequence",
        Some("⌘["),
        true,
    ),
    CommandSpec {
        params: ON,
        ..cmd("drum.mark_lane", "Drum", "Drum Lane", None, true)
    },
    cmd("drum.add_row", "Drum", "Add Drum Row", None, true),
    CommandSpec {
        params: ON,
        ..cmd("drum.show_velocity", "Drum", "Show Velocity", None, true)
    },
    cmd(
        "drum.split_row",
        "Drum",
        "Split Row to Own Lane",
        None,
        true,
    ),
    cmd("drum.remove_row", "Drum", "Remove Drum Row", None, true),
    CommandSpec {
        params: ON,
        ..cmd("drum.timeline", "Drum", "Timeline View", None, true)
    },
    cmd(
        "transport.play_stop",
        "Transport",
        "Play / Stop",
        Some("Space"),
        true,
    ),
    cmd("transport.stop", "Transport", "Stop", Some("S"), true),
    cmd(
        "transport.play_selection",
        "Transport",
        "Play Selection",
        Some("⌥Space"),
        true,
    ),
    cmd(
        "transport.play_from_selection",
        "Transport",
        "Play from Selection",
        Some("⇧Space"),
        true,
    ),
    cmd(
        "transport.play_from_edit_in",
        "Transport",
        "Play from Edit-In",
        Some("Y"),
        true,
    ),
    cmd("transport.record", "Transport", "Record", Some("R"), true),
    cmd("transport.scrub", "Transport", "Scrub", Some("K"), true),
    cmd(
        "transport.return_to_start",
        "Transport",
        "Return to Start",
        Some("Return"),
        true,
    ),
    cmd(
        "transport.previous_marker",
        "Transport",
        "Previous Marker",
        Some(","),
        true,
    ),
    cmd(
        "transport.next_marker",
        "Transport",
        "Next Marker",
        Some("."),
        true,
    ),
    CommandSpec {
        params: ON,
        ..cmd("transport.loop", "Transport", "Loop", Some("L"), true)
    },
    CommandSpec {
        params: ON,
        ..cmd("transport.overdub", "Transport", "Overdub", Some("O"), true)
    },
    CommandSpec {
        params: ON,
        ..cmd("transport.punch", "Transport", "Punch", Some("I"), true)
    },
    cmd(
        "transport.arm_previous",
        "Transport",
        "Arm Previous Lane",
        Some("⌥⇧↑"),
        true,
    ),
    cmd(
        "transport.arm_next",
        "Transport",
        "Arm Next Lane",
        Some("⌥⇧↓"),
        true,
    ),
    key_only(cmd(
        "record.keep_pass",
        "Record",
        "Keep Loop Pass",
        None,
        true,
    )),
    key_only(cmd(
        "record.discard_pass",
        "Record",
        "Discard Since Last Keep",
        None,
        true,
    )),
    locator("locator.go_0", "Go to Locator 0", "0"),
    locator("locator.go_1", "Go to Locator 1", "1"),
    locator("locator.go_2", "Go to Locator 2", "2"),
    locator("locator.go_3", "Go to Locator 3", "3"),
    locator("locator.go_4", "Go to Locator 4", "4"),
    locator("locator.go_5", "Go to Locator 5", "5"),
    locator("locator.go_6", "Go to Locator 6", "6"),
    locator("locator.go_7", "Go to Locator 7", "7"),
    locator("locator.go_8", "Go to Locator 8", "8"),
    locator("locator.go_9", "Go to Locator 9", "9"),
    locator("locator.set_0", "Set Locator 0", "⌥0"),
    locator("locator.set_1", "Set Locator 1", "⌥1"),
    locator("locator.set_2", "Set Locator 2", "⌥2"),
    locator("locator.set_3", "Set Locator 3", "⌥3"),
    locator("locator.set_4", "Set Locator 4", "⌥4"),
    locator("locator.set_5", "Set Locator 5", "⌥5"),
    locator("locator.set_6", "Set Locator 6", "⌥6"),
    locator("locator.set_7", "Set Locator 7", "⌥7"),
    locator("locator.set_8", "Set Locator 8", "⌥8"),
    locator("locator.set_9", "Set Locator 9", "⌥9"),
    locator("locator.clear_0", "Clear Locator 0", "⌥⇧0"),
    locator("locator.clear_1", "Clear Locator 1", "⌥⇧1"),
    locator("locator.clear_2", "Clear Locator 2", "⌥⇧2"),
    locator("locator.clear_3", "Clear Locator 3", "⌥⇧3"),
    locator("locator.clear_4", "Clear Locator 4", "⌥⇧4"),
    locator("locator.clear_5", "Clear Locator 5", "⌥⇧5"),
    locator("locator.clear_6", "Clear Locator 6", "⌥⇧6"),
    locator("locator.clear_7", "Clear Locator 7", "⌥⇧7"),
    locator("locator.clear_8", "Clear Locator 8", "⌥⇧8"),
    locator("locator.clear_9", "Clear Locator 9", "⌥⇧9"),
    CommandSpec {
        params: KEYS_PATH,
        dialogs: &[DialogSpec {
            name: "Save",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..key_only(cmd(
            "keys.save",
            "Key Bindings",
            "Save Key Bindings…",
            None,
            false,
        ))
    },
    CommandSpec {
        params: KEYS_PATH,
        dialogs: &[DialogSpec {
            name: "Open",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..key_only(cmd(
            "keys.load",
            "Key Bindings",
            "Load Key Bindings…",
            None,
            false,
        ))
    },
    key_only(cmd(
        "keys.reset",
        "Key Bindings",
        "Reset Key Bindings",
        None,
        false,
    )),
];

/// The command with `id`.
pub fn find(id: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// Checks `params` against `spec`: no unknown or missing parameter, every
/// value of its type, and an always-opened dialog answered.
pub fn validate_args(
    spec: &CommandSpec,
    params: &Map<String, Value>,
) -> Result<Vec<(String, ArgValue)>, ControlError> {
    let p = Params::new(spec.id, params);
    let names: Vec<&str> = spec.params.iter().map(|s| s.name).collect();
    p.only(&names)?;
    let mut out = Vec::new();
    for s in spec.params {
        let v = match s.kind {
            ParamKind::Text => p.opt_str(s.name, MAX_STRING)?.map(ArgValue::Text),
            ParamKind::Path(ext) => p
                .opt_path(s.name, ext)?
                .map(|path| ArgValue::Text(path.to_string_lossy().into_owned())),
            ParamKind::Flag => p.opt_bool(s.name)?.map(ArgValue::Flag),
            ParamKind::Choice(values) => match p.opt_str(s.name, MAX_PATH)? {
                None => None,
                Some(v) if values.contains(&v.as_str()) => Some(ArgValue::Text(v)),
                Some(_) => {
                    return Err(p.error(format!(
                        "`{}` must be one of {}",
                        s.name,
                        values.join(", ")
                    )));
                }
            },
        };
        match v {
            Some(v) => out.push((s.name.to_string(), v)),
            None if s.required => return Err(p.error(format!("missing `{}`", s.name))),
            None => {}
        }
    }
    for d in spec.dialogs.iter().filter(|d| d.always) {
        if !d.answered_by.iter().any(|a| p.has(a)) {
            return Err(ControlError::dialog(d.name, d.answered_by));
        }
    }
    Ok(out)
}

/// `unknown_id` for a `command.run` id.
pub fn unknown(id: &str) -> ControlError {
    ControlError::new(
        ErrorCode::UnknownId,
        format!(
            "no menu command {:?} (see commands.list)",
            crate::protocol::truncate(id, 64)
        ),
    )
}

/// The key and modifiers of a shortcut as the registry writes it
/// (`⇧⌘S`, `⌥←`, `Space`).
pub fn shortcut_stroke(shortcut: &str) -> Option<KeyStroke> {
    let mut m = Modifiers::default();
    let mut rest = shortcut;
    loop {
        let mut chars = rest.chars();
        match chars.next() {
            Some('⇧') => m.shift = true,
            Some('⌃') => m.control = true,
            Some('⌥') => m.option = true,
            Some('⌘') => m.command = true,
            _ => break,
        }
        rest = chars.as_str();
    }
    let key = match rest {
        "Space" => "space".to_string(),
        "Return" => "return".to_string(),
        "←" => "left".to_string(),
        "→" => "right".to_string(),
        "↑" => "up".to_string(),
        "↓" => "down".to_string(),
        other => other.to_lowercase(),
    };
    named_key(&key, m).ok()
}

/// The global command whose default shortcut `stroke` is, if any
/// (control spec §5.5). Step entry's keys are not global.
pub fn command_for_stroke(stroke: &KeyStroke) -> Option<&'static str> {
    COMMANDS
        .iter()
        .filter(|c| c.context == KeyContext::Global)
        .find_map(|c| {
            let s = shortcut_stroke(c.shortcut?)?;
            (s.key_code == stroke.key_code && s.modifiers == stroke.modifiers).then_some(c.id)
        })
}

fn kind_json(k: ParamKind) -> Value {
    match k {
        ParamKind::Text => json!({"type": "string"}),
        ParamKind::Path(ext) => json!({"type": "path", "extension": ext}),
        ParamKind::Flag => json!({"type": "bool"}),
        ParamKind::Choice(values) => json!({"type": "choice", "values": values}),
    }
}

/// The `commands.list` result.
pub fn list_json() -> Value {
    let commands: Vec<Value> = COMMANDS
        .iter()
        .map(|c| {
            let params: Vec<Value> = c
                .params
                .iter()
                .map(|p| {
                    let mut v = kind_json(p.kind);
                    if let Some(o) = v.as_object_mut() {
                        o.insert("name".into(), json!(p.name));
                        o.insert("required".into(), json!(p.required));
                        o.insert("help".into(), json!(p.help));
                    }
                    v
                })
                .collect();
            let dialogs: Vec<Value> = c
                .dialogs
                .iter()
                .map(|d| json!({"name": d.name, "when": d.when, "answered_by": d.answered_by}))
                .collect();
            let keys: Vec<String> = crate::bindings::default_chords(c)
                .iter()
                .map(crate::bindings::Chord::text)
                .collect();
            json!({
                "id": c.id, "menu": c.menu, "title": c.title, "shortcut": c.shortcut,
                "keys": keys, "menu_item": c.menu_item, "context": c.context.name(),
                "document": c.document, "params": params, "dialogs": dialogs,
            })
        })
        .collect();
    json!({ "commands": commands })
}
