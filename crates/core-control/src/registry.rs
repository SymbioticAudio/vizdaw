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
    /// An integer in `min..=max`.
    Int {
        /// Smallest.
        min: i64,
        /// Largest.
        max: i64,
    },
    /// An integer in `min..=max`, or null: null is a value (it reaches the
    /// app as the text [`NULL_TEXT`]), not an absent key.
    IntOrNull {
        /// Smallest.
        min: i64,
        /// Largest.
        max: i64,
    },
    /// An absolute path with any extension.
    File,
    /// One of `values`, or `prefix` followed by 1 to [`MAX_SLUG`] lowercase
    /// ASCII letters, digits and inner hyphens (a user theme, plan 7b).
    ChoiceOrPrefixed {
        /// The fixed choices.
        values: &'static [&'static str],
        /// What starts an open-ended choice (`user.`).
        prefix: &'static str,
    },
}

/// The longest slug after a [`ParamKind::ChoiceOrPrefixed`] prefix.
pub const MAX_SLUG: usize = 64;

/// `s` is 1 to [`MAX_SLUG`] lowercase ASCII letters, digits and hyphens,
/// starting and ending with a letter or digit.
pub fn is_slug(s: &str) -> bool {
    let b = s.as_bytes();
    let alnum = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    !b.is_empty()
        && b.len() <= MAX_SLUG
        && b.iter().all(|c| alnum(c) || *c == b'-')
        && b.first().is_some_and(alnum)
        && b.last().is_some_and(alnum)
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

/// What a command does to the composer's work, for the Co-Producer gate
/// (Co-Producer spec §6.5, plan 8a Ruling 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    /// Reads or plays; changes nothing.
    Read,
    /// Creates something new.
    Add,
    /// Changes something that exists.
    Change,
    /// Deletes something.
    Remove,
    /// Writes files outside the project, never over one.
    Outward,
    /// Never run by Co-Producer.
    Refused,
}

impl Class {
    /// Its name in `commands.list`.
    pub fn name(self) -> &'static str {
        match self {
            Class::Read => "read",
            Class::Add => "add",
            Class::Change => "change",
            Class::Remove => "remove",
            Class::Outward => "outward",
            Class::Refused => "refused",
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
    /// Its default keys give way to a key the user's keys file gives
    /// another command (plan 7d controller ruling I3): true only for
    /// commands whose defaults came after users had keys files.
    pub yields: bool,
    /// Its Co-Producer class (plan 8a Ruling 5); [`Class::Refused`] unless
    /// the entry says otherwise.
    pub class: Class,
}

/// What a null [`ParamKind::IntOrNull`] value reaches the app as.
pub const NULL_TEXT: &str = "none";

/// A validated `command.run` argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArgValue {
    /// A string (paths too).
    Text(String),
    /// A flag.
    Flag(bool),
    /// An integer.
    Int(i64),
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
        yields: false,
        class: Class::Refused,
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

/// A step entry key's command (plan 4f Ruling 12): live only during step
/// entry, with a keypad twin.
const fn step(
    id: &'static str,
    title: &'static str,
    key: &'static str,
    keypad: &'static [&'static str],
) -> CommandSpec {
    CommandSpec {
        extra_keys: keypad,
        context: KeyContext::StepEntry,
        ..cmd(id, "Step", title, Some(key), true)
    }
}

/// A numbered locator's command (plan 4f Ruling 7), of `class`.
const fn locator(
    class: Class,
    id: &'static str,
    title: &'static str,
    key: &'static str,
) -> CommandSpec {
    CommandSpec {
        class,
        ..key_only(cmd(id, "Locators", title, Some(key), true))
    }
}

const ON: &[ParamSpec] = &[ParamSpec {
    name: "on",
    kind: ParamKind::Flag,
    required: false,
    help: "Set on or off; absent toggles.",
}];

/// The built-in themes' IDs (symbiotic-design 0.2.1 `Theme.all`; a Swift
/// test pins the two lists together).
pub const THEME_IDS: &[&str] = &[
    "classic-light",
    "studio-dark",
    "flat",
    "console",
    "symbiotic",
    "eg13",
];

/// The prefix of a user theme's ID (controller ruling R-UT).
pub const USER_THEME_PREFIX: &str = "user.";

/// `plugins.new_channel_eq`'s parameter (plan 6g Task 4).
const NEW_CHANNEL_EQ_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "on",
    kind: ParamKind::Flag,
    required: true,
    help: "Put an EQ on new channels (on) or not (off).",
}];

/// `plugins.install_multiq`'s parameter (plan 6g Task 4).
const INSTALL_MULTIQ_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "replace",
    kind: ParamKind::Flag,
    required: false,
    help: "Replace a Multi-Q.component already in the Components folder (answers the dialog).",
}];

/// `plugins.default_eq`'s parameter (plan 6g-2 Ruling 7).
const PLUGINS_DEFAULT_EQ_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "plugin",
    kind: ParamKind::Text,
    required: true,
    help: "multi-q, or clap:<id> or au:<type/subtype/manufacturer> of an installed effect (see the Plugin Manager).",
}];

/// `settings.co_producer`'s parameter (plan 8a Ruling 10).
const CO_PRODUCER_SWITCH_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "on",
    kind: ParamKind::Flag,
    required: true,
    help: "Turn Co-Producer on (true) or off (false).",
}];

/// `settings.co_producer_answer`'s parameters (plan 8b Ruling 11): the
/// e2e's answer to VizDAW's own approval sheet.
const CO_PRODUCER_ANSWER_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "allow",
        kind: ParamKind::Flag,
        required: true,
        help: "Allow (true) or Don't Allow (false) the change the open Co-Producer approval sheet asks about.",
    },
    ParamSpec {
        name: "always",
        kind: ParamKind::Flag,
        required: false,
        help: "With allow: tick Always allow for the asking app.",
    },
    ParamSpec {
        name: "confirm",
        kind: ParamKind::Flag,
        required: false,
        help: "With always, for an app VizDAW cannot verify: Always Allow Anyway on the warning.",
    },
];

/// `file.open_copy`'s parameter (plan 8a Ruling 9).
const OPEN_COPY_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "path",
    kind: ParamKind::Path("vizdaw"),
    required: false,
    help: "The Co-Producer copy to open as a new untitled project (answers the Open dialog).",
}];

/// `mixer.new_bus`'s parameters (plan 6g-2 Ruling 7).
const MIXER_NEW_BUS_PARAMS: &[ParamSpec] = &[
    opt_id_param(
        "strip",
        "The strip whose new send feeds the bus (see state.get mixer); absent: a bus with no send.",
    ),
    ParamSpec {
        name: "name",
        kind: ParamKind::Text,
        required: false,
        help: "The bus's name (default: the next free Bus N).",
    },
];

/// A required strip ID of `mixer.make_mine` and `mixer.set_colour`
/// (plan 8b Ruling 9).
const STRIP_PARAM: ParamSpec = ParamSpec {
    name: "strip",
    kind: ParamKind::Int {
        min: 1,
        max: i64::MAX / 2,
    },
    required: true,
    help: "The strip (see state.get mixer).",
};

/// `mixer.make_mine` (plan 8b Ruling 1).
const MIXER_MAKE_MINE_PARAMS: &[ParamSpec] = &[STRIP_PARAM];

/// The strip colours `mixer.set_colour` takes (plan 8b Ruling 3): `none`
/// clears the strip's own colour.
pub const STRIP_COLOURS: &[&str] = &[
    "none", "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink",
];

/// `mixer.set_colour` (plan 8b Ruling 3).
const MIXER_SET_COLOUR_PARAMS: &[ParamSpec] = &[
    STRIP_PARAM,
    ParamSpec {
        name: "colour",
        kind: ParamKind::Choice(STRIP_COLOURS),
        required: true,
        help: "The strip's colour; none clears it.",
    },
];

/// The largest strip, slot, bus or index plan 8d's mixer commands take:
/// the largest integer a JSON number carries exactly (2^53 − 1).
pub const MAX_EXACT_ID: i64 = (1 << 53) - 1;

/// The longest plug-in reference, in characters (plan 8d Ruling 3).
pub const PLUGIN_REF_MAX: usize = 512;

/// Plan 8d Ruling 1's thirteen commands: the twelve mixer edits, then
/// `track.new_plugin_instrument`.
pub const MIXER_IDS: &[&str] = &[
    "mixer.set_strip",
    "mixer.set_output",
    "mixer.add_send",
    "mixer.set_send",
    "mixer.remove_send",
    "mixer.add_insert",
    "mixer.remove_insert",
    "mixer.move_insert",
    "mixer.set_bypass",
    "mixer.set_eq",
    "mixer.set_param",
    "mixer.remove_bus",
    "track.new_plugin_instrument",
];

/// A fader or send level's range in dB (plan 8d Ruling 1; −144 is −∞).
const LEVEL_DB: (f64, f64) = (-144.0, 12.0);

/// `mixer.set_param`'s `value` range in the registry: everything
/// [`decimal`]'s shape can spell. The plug-in's own range is the app's
/// check.
const PARAM_VALUE: (f64, f64) = (-1_000_000.0, 1_000_000.0);

/// Ruling 1: `-?[0-9]{1,6}(\.[0-9]{1,6})?` within `min..=max`; the message
/// names the range. So exponents, `NaN`, `inf`, a leading `+`, hex, a
/// comma, spaces and the empty text are all refused.
pub fn decimal(text: &str, min: f64, max: f64) -> Result<f64, String> {
    let refuse = || format!("must be a plain decimal from {min} to {max}, such as \"-6.5\"");
    let digits = |s: &str| (1..=6).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    let body = text.strip_prefix('-').unwrap_or(text);
    let shaped = match body.split_once('.') {
        Some((whole, frac)) => digits(whole) && digits(frac),
        None => digits(body),
    };
    if !shaped {
        return Err(refuse());
    }
    match text.parse::<f64>() {
        Ok(v) if v.is_finite() && (min..=max).contains(&v) => Ok(v),
        _ => Err(refuse()),
    }
}

/// A required mixer target of plan 8d (a strip, slot or bus): 1 to
/// [`MAX_EXACT_ID`].
const fn mixer_id(name: &'static str, help: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Int {
            min: 1,
            max: MAX_EXACT_ID,
        },
        required: true,
        help,
    }
}

/// An optional [`mixer_id`].
const fn opt_mixer_id(name: &'static str, help: &'static str) -> ParamSpec {
    ParamSpec {
        required: false,
        ..mixer_id(name, help)
    }
}

const MIXER_STRIP: ParamSpec = mixer_id("strip", "The strip (see state.get mixer).");
const MIXER_SLOT: ParamSpec = mixer_id(
    "slot",
    "The plug-in slot: an insert, an EQ or an instrument (see state.get mixer).",
);

/// `mixer.set_strip` (plan 8d Ruling 1).
const MIXER_SET_STRIP_PARAMS: &[ParamSpec] = &[
    MIXER_STRIP,
    ParamSpec {
        name: "gain_db",
        kind: ParamKind::Text,
        required: false,
        help: "The fader in dB as text, -144 (silent) to 12, such as \"-6.5\". At least one of gain_db, pan, mute and solo is required.",
    },
    ParamSpec {
        name: "pan",
        kind: ParamKind::Int {
            min: -100,
            max: 100,
        },
        required: false,
        help: "Pan in percent, -100 (left) to 100 (right); 0 is the centre.",
    },
    ParamSpec {
        name: "mute",
        kind: ParamKind::Flag,
        required: false,
        help: "Mute (true) or unmute (false).",
    },
    ParamSpec {
        name: "solo",
        kind: ParamKind::Flag,
        required: false,
        help: "Solo (true) or unsolo (false).",
    },
];

/// `mixer.set_output` (plan 8d Ruling 1).
const MIXER_SET_OUTPUT_PARAMS: &[ParamSpec] = &[
    MIXER_STRIP,
    ParamSpec {
        name: "bus",
        kind: ParamKind::IntOrNull {
            min: 1,
            max: MAX_EXACT_ID,
        },
        required: true,
        help: "The bus strip to route to, or null for master.",
    },
];

const SEND_LEVEL: ParamSpec = ParamSpec {
    name: "level_db",
    kind: ParamKind::Text,
    required: false,
    help: "The send level in dB as text, -144 (silent) to 12 (default \"0\").",
};

const PRE_FADER: ParamSpec = ParamSpec {
    name: "pre_fader",
    kind: ParamKind::Flag,
    required: false,
    help: "Send before the fader (default false: after it).",
};

const SEND_INDEX: ParamSpec = mixer_id("index", "The send, counting from 1 (see state.get mixer).");

/// `mixer.add_send` (plan 8d Ruling 1).
const MIXER_ADD_SEND_PARAMS: &[ParamSpec] = &[
    MIXER_STRIP,
    mixer_id("bus", "The bus strip the send feeds."),
    SEND_LEVEL,
    PRE_FADER,
];

/// `mixer.set_send` (plan 8d Ruling 1).
const MIXER_SET_SEND_PARAMS: &[ParamSpec] = &[
    MIXER_STRIP,
    SEND_INDEX,
    ParamSpec {
        help: "The send level in dB as text, -144 (silent) to 12. At least one of level_db, pre_fader and enabled is required.",
        ..SEND_LEVEL
    },
    ParamSpec {
        help: "Send before the fader (true) or after it (false).",
        ..PRE_FADER
    },
    ParamSpec {
        name: "enabled",
        kind: ParamKind::Flag,
        required: false,
        help: "Turn the send on (true) or off (false).",
    },
];

/// `mixer.remove_send` (plan 8d Ruling 1).
const MIXER_REMOVE_SEND_PARAMS: &[ParamSpec] = &[MIXER_STRIP, SEND_INDEX];

const PLUGIN_REF: ParamSpec = ParamSpec {
    name: "plugin",
    kind: ParamKind::Text,
    required: true,
    help: "multi-q, or clap:<id> or au:<type/subtype/manufacturer> of an installed effect (see co_producer.plugins).",
};

/// `mixer.add_insert` (plan 8d Ruling 1).
const MIXER_ADD_INSERT_PARAMS: &[ParamSpec] = &[
    MIXER_STRIP,
    PLUGIN_REF,
    opt_mixer_id(
        "index",
        "Where the insert goes, counting from 1 (default: after the last).",
    ),
];

/// `mixer.remove_insert` (plan 8d Ruling 1).
const MIXER_SLOT_PARAMS: &[ParamSpec] = &[MIXER_SLOT];

/// `mixer.move_insert` (plan 8d Ruling 1).
const MIXER_MOVE_INSERT_PARAMS: &[ParamSpec] = &[
    MIXER_SLOT,
    mixer_id(
        "index",
        "The insert's new place on its own strip, counting from 1.",
    ),
];

/// `mixer.set_bypass` (plan 8d Ruling 1).
const MIXER_SET_BYPASS_PARAMS: &[ParamSpec] = &[
    MIXER_SLOT,
    ParamSpec {
        name: "bypass",
        kind: ParamKind::Flag,
        required: true,
        help: "Bypass the insert (true) or turn it back on (false).",
    },
];

/// `mixer.set_eq` (plan 8d Ruling 1).
const MIXER_SET_EQ_PARAMS: &[ParamSpec] = &[
    MIXER_STRIP,
    ParamSpec {
        help: "The strip's EQ: multi-q, or clap:<id> or au:<type/subtype/manufacturer> of an installed effect (see co_producer.plugins).",
        ..PLUGIN_REF
    },
];

/// `mixer.set_param` (plan 8d Ruling 1).
const MIXER_SET_PARAM_PARAMS: &[ParamSpec] = &[
    MIXER_SLOT,
    ParamSpec {
        name: "param",
        kind: ParamKind::Int {
            min: 0,
            max: u32::MAX as i64,
        },
        required: true,
        help: "The parameter's id (see co_producer.params).",
    },
    ParamSpec {
        name: "value",
        kind: ParamKind::Text,
        required: true,
        help: "The value as text, such as \"0.75\", within the parameter's own range; a stepped parameter takes whole values.",
    },
];

/// `mixer.remove_bus` (plan 8d Ruling 1).
const MIXER_REMOVE_BUS_PARAMS: &[ParamSpec] = &[mixer_id("strip", "The bus strip to remove.")];

/// `track.new_plugin_instrument` (plan 8d Ruling 1).
const NEW_PLUGIN_INSTRUMENT_PARAMS: &[ParamSpec] = &[
    opt_id_param("seq", "The sequence (default: the selected one)."),
    opt_id_param(
        "after",
        "The track to put the new one below (default: the selection's first track, else the end).",
    ),
    ParamSpec {
        name: "plugin",
        kind: ParamKind::Text,
        required: true,
        help: "built-in-synth, or clap:<id> or au:<type/subtype/manufacturer> of an installed instrument (see co_producer.plugins).",
    },
];

/// Ruling 3's shape of a plug-in reference: `multi-q`, `clap:<id>` or
/// `au:<id>` with a non-empty rest (and `built-in-synth` when
/// `instrument`), at most [`PLUGIN_REF_MAX`] characters, with no control,
/// bidi or line-separator character. Resolving it is the app's.
pub fn plugin_ref(text: &str, instrument: bool) -> Result<(), String> {
    let named = text == "multi-q" || (instrument && text == "built-in-synth");
    let prefixed = ["clap:", "au:"]
        .iter()
        .any(|p| text.strip_prefix(p).is_some_and(|rest| !rest.is_empty()));
    if text.chars().count() > PLUGIN_REF_MAX || text.chars().any(crate::request::refused_in_name) {
        return Err(format!(
            "must be at most {PLUGIN_REF_MAX} characters with no control character"
        ));
    }
    if named || prefixed {
        return Ok(());
    }
    Err(if instrument {
        "must be built-in-synth, multi-q, clap:<id> or au:<type/subtype/manufacturer>".into()
    } else {
        "must be multi-q, clap:<id> or au:<type/subtype/manufacturer>".into()
    })
}

/// `ui.theme`'s parameter (plan 7b Ruling 11, R-UT).
const UI_THEME_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "theme",
    kind: ParamKind::ChoiceOrPrefixed {
        values: THEME_IDS,
        prefix: USER_THEME_PREFIX,
    },
    required: true,
    help: "A built-in theme, or a user theme's `user.` ID (see Settings › Appearance).",
}];

/// `ui.pane`'s parameters (plan 7a Task 5).
const UI_PANE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "pane",
        kind: ParamKind::Choice(&["left", "bottom", "right"]),
        required: true,
        help: "The pane to set.",
    },
    ParamSpec {
        name: "view",
        kind: ParamKind::Choice(&[
            "strip",
            "sequences",
            "inspector",
            "piano_roll",
            "list",
            "strip_chart",
            "drum_grid",
            "audio_editor",
            "mixer",
            "controllers",
            "media_pool",
            "plugins",
            "markers",
        ]),
        required: false,
        help: "What the pane shows.",
    },
    ParamSpec {
        name: "shown",
        kind: ParamKind::Flag,
        required: false,
        help: "Show or hide the pane.",
    },
    ParamSpec {
        name: "size",
        kind: ParamKind::Int {
            min: 0,
            max: 10_000,
        },
        required: false,
        help: "The pane's width or height in points.",
    },
    ParamSpec {
        name: "main",
        kind: ParamKind::Choice(&["arrange", "mix"]),
        required: false,
        help: "The main view whose pane this is; absent is the current one.",
    },
];

/// The project rates and depths as the channel spells them (5a Ruling 16).
const RATES: &[&str] = &["44100", "48000", "88200", "96000", "176400", "192000"];
const DEPTHS: &[&str] = &["16", "24", "32f"];

const FORMAT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "rate",
        kind: ParamKind::Choice(RATES),
        required: false,
        help: "Sample rate in Hz.",
    },
    ParamSpec {
        name: "depth",
        kind: ParamKind::Choice(DEPTHS),
        required: false,
        help: "Bit depth: 16, 24 or 32f (float).",
    },
];

const IMPORT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::File,
        required: false,
        help: "The audio file to import (answers the Open dialog).",
    },
    ParamSpec {
        name: "seq",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "Sequence to place a region in (without it, the file only enters the media pool).",
    },
    ParamSpec {
        name: "lane",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "Track for a mono or stereo file; without it, new tracks.",
    },
    ParamSpec {
        name: "tick",
        kind: ParamKind::Int {
            min: -(1 << 40),
            max: 1 << 40,
        },
        required: false,
        help: "Where the region starts (negative imports at 0).",
    },
];

/// `file.bounce` (plan 5f-2 Ruling 2): the Bounce dialog's answers.
const BOUNCE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "source",
        kind: ParamKind::Choice(&["mix", "stems", "selection"]),
        required: false,
        help: "mix (default), stems (one file per strip in `strips`) or selection (the mix over the time selection).",
    },
    ParamSpec {
        name: "strips",
        kind: ParamKind::Text,
        required: false,
        help: "For stems: strip IDs, comma-separated (see state.get mixer).",
    },
    ParamSpec {
        name: "range",
        kind: ParamKind::Choice(&["song", "selection"]),
        required: false,
        help: "song (default) or selection (`start` and `end`, else the editor's time selection).",
    },
    ParamSpec {
        name: "start",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "The selection's first tick (with `range: selection`).",
    },
    ParamSpec {
        name: "end",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "The selection's end tick, exclusive (with `range: selection`).",
    },
    ParamSpec {
        name: "format",
        kind: ParamKind::Choice(&["wav", "aiff", "caf"]),
        required: false,
        help: "wav (default), aiff or caf.",
    },
    ParamSpec {
        name: "bits",
        kind: ParamKind::Choice(DEPTHS),
        required: false,
        help: "16, 24 or 32f (float); default the project's depth.",
    },
    ParamSpec {
        name: "rate",
        kind: ParamKind::Int {
            min: 8_000,
            max: 384_000,
        },
        required: false,
        help: "8000–384000; the project's rate by default.",
    },
    ParamSpec {
        name: "dither",
        kind: ParamKind::Flag,
        required: false,
        help: "TPDF dither at 16 and 24 bits; default on for 16 bits; float ignores it.",
    },
    ParamSpec {
        name: "path",
        kind: ParamKind::File,
        required: false,
        help: "The mix file, or an existing folder for stems (answers the Bounce dialog); stems never replace: taken names are numbered.",
    },
    ParamSpec {
        name: "add_to_project",
        kind: ParamKind::Flag,
        required: false,
        help: "Also place the bounce on new tracks (default false; answers the Bounce dialog). It bounces as heard: a muted or solo-silenced strip, or one routed into a muted bus, is silent; files ignore mute and solo.",
    },
    ParamSpec {
        name: "replace",
        kind: ParamKind::Flag,
        required: false,
        help: "Replace an existing mix file (default false); the old file stays until the new one is complete.",
    },
];

/// `file.import_midi` (plan 6a Task 8): the Open dialog and the import options.
const IMPORT_MIDI_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::File,
        required: false,
        help: "The MIDI file (SMF or MIDI Clip File; answers the Open dialog).",
    },
    ParamSpec {
        name: "new_project",
        kind: ParamKind::Flag,
        required: false,
        help: "Import into a new, empty project instead of this one (default false).",
    },
    ParamSpec {
        name: "seq",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "The sequence (default: the one shown).",
    },
    ParamSpec {
        name: "lane",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "Drop onto this track (default: new tracks at the end).",
    },
    ParamSpec {
        name: "tick",
        kind: ParamKind::Int {
            min: -(1 << 40),
            max: 1 << 40,
        },
        required: false,
        help: "Where the file's start goes (negative is 0; default 0).",
    },
    ParamSpec {
        name: "channel_map",
        kind: ParamKind::Text,
        required: false,
        help: "Where each channel plays: JSON [{\"group\":0,\"channel\":9,\"dest\":3}]; dest null makes a new built-in synth. A pair not listed takes its suggestion.",
    },
    ParamSpec {
        name: "split_channels",
        kind: ParamKind::Flag,
        required: false,
        help: "Split tracks by channel (default: type 0 files only).",
    },
    ParamSpec {
        name: "mpe",
        kind: ParamKind::Flag,
        required: false,
        help: "Decode every track as MPE (default false).",
    },
    ParamSpec {
        name: "use_tempo",
        kind: ParamKind::Flag,
        required: false,
        help: "Use the file's tempo and meter (default false; always into a sequence without events).",
    },
];

/// `file.export_midi` (plan 6a Task 8): a Standard MIDI File.
const EXPORT_MIDI_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::Path("mid"),
        required: false,
        help: "The .mid file to write (answers the Save dialog; needed unless dry_run).",
    },
    ParamSpec {
        name: "replace",
        kind: ParamKind::Flag,
        required: false,
        help: "Replace an existing file (default false); the old file stays until the new one is complete.",
    },
    ParamSpec {
        name: "seq",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "The sequence (default: the one shown).",
    },
    ParamSpec {
        name: "type",
        kind: ParamKind::Choice(&["0", "1"]),
        required: false,
        help: "0 (one track) or 1 (default).",
    },
    ParamSpec {
        name: "ppq",
        kind: ParamKind::Choice(&["480", "960", "15360"]),
        required: false,
        help: "Ticks per quarter note: 480, 960 (default) or 15360.",
    },
    ParamSpec {
        name: "lanes",
        kind: ParamKind::Text,
        required: false,
        help: "Track IDs, comma-separated (default: every track; see state.get sequence).",
    },
    ParamSpec {
        name: "mpe",
        kind: ParamKind::Flag,
        required: false,
        help: "Write per-note expression as MPE (default false).",
    },
    ParamSpec {
        name: "dry_run",
        kind: ParamKind::Flag,
        required: false,
        help: "Only check: answer what the export would do, write nothing.",
    },
];

/// `file.export_midi_clip` (plan 6a Task 8): MIDI Clip Files, one per track.
const EXPORT_MIDI_CLIP_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::File,
        required: false,
        help: "A file for one track, an existing folder for several (answers the Save dialog; needed unless dry_run).",
    },
    ParamSpec {
        name: "replace",
        kind: ParamKind::Flag,
        required: false,
        help: "Replace an existing file (default false); the old file stays until the new one is complete.",
    },
    ParamSpec {
        name: "seq",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "The sequence (default: the one shown).",
    },
    ParamSpec {
        name: "lanes",
        kind: ParamKind::Text,
        required: false,
        help: "Track IDs, comma-separated (default: every track; see state.get sequence).",
    },
    ParamSpec {
        name: "start",
        kind: ParamKind::Int {
            min: 0,
            max: 1 << 40,
        },
        required: false,
        help: "The range's first tick for every track (needs `end`; default: the pickup's start).",
    },
    ParamSpec {
        name: "end",
        kind: ParamKind::Int {
            min: 0,
            max: 1 << 40,
        },
        required: false,
        help: "The range's end tick, exclusive (default: the sequence's end).",
    },
    ParamSpec {
        name: "dry_run",
        kind: ParamKind::Flag,
        required: false,
        help: "Only check: answer what the export would do, write nothing.",
    },
];

/// `file.import_dawproject` (plan 6d Task 11): a DAWproject file as a new
/// project, or merged into the project it came from (plan 6e Task 9).
const IMPORT_DAWPROJECT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::Path("dawproject"),
        required: false,
        help: "The .dawproject file to open as a new project (answers the Open dialog).",
    },
    ParamSpec {
        name: "mode",
        kind: ParamKind::Choice(&["new", "merge"]),
        required: false,
        help: "new: open the file as a new project; merge: merge it into the project it came from. Needed when the file came from VizDAW.",
    },
    ParamSpec {
        name: "project",
        kind: ParamKind::Path("vizdaw"),
        required: false,
        help: "For merge: the project the file came from, when it is not open (answers the Open dialog).",
    },
    ParamSpec {
        name: "snapshot",
        kind: ParamKind::Text,
        required: false,
        help: "For merge: the export the file came from (an export ID), when the file lost its own.",
    },
    ParamSpec {
        name: "preview",
        kind: ParamKind::Flag,
        required: false,
        help: "For merge: return the merge preview and change nothing.",
    },
];

/// `file.export_dawproject` (plan 6d Task 11): a DAWproject file.
const EXPORT_DAWPROJECT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::Path("dawproject"),
        required: false,
        help: "The .dawproject file to write (answers the Save dialog; needed unless dry_run).",
    },
    ParamSpec {
        name: "replace",
        kind: ParamKind::Flag,
        required: false,
        help: "Replace an existing file (default false); the old file stays until the new one is complete.",
    },
    ParamSpec {
        name: "seq",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "The sequence (default: the one shown).",
    },
    ParamSpec {
        name: "region_gain",
        kind: ParamKind::Choice(&["apply", "leave_out"]),
        required: false,
        help: "apply (default): region gain is applied to the exported audio; leave_out: it is left out.",
    },
    ParamSpec {
        name: "dry_run",
        kind: ParamKind::Flag,
        required: false,
        help: "Return what won't transfer; write nothing.",
    },
];

/// `file.import_aaf` (plan 6f Task 6): an AAF file as a new project, or
/// added to this one.
const IMPORT_AAF_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::Path("aaf"),
        required: false,
        help: "The .aaf file (answers the Open dialog).",
    },
    ParamSpec {
        name: "new_project",
        kind: ParamKind::Flag,
        required: false,
        help: "Open the file as a new project (default true); false adds its tracks to this project, timed by its tempo map. One undo step.",
    },
    ParamSpec {
        name: "midi_path",
        kind: ParamKind::Path("mid"),
        required: false,
        help: "The MIDI file with the tempo map and MIDI tracks (default: the one beside the AAF with its name).",
    },
    ParamSpec {
        name: "media_folder",
        kind: ParamKind::File,
        required: false,
        help: "A folder to look in, by file name, for audio the file cannot find.",
    },
    ParamSpec {
        name: "join_pairs",
        kind: ParamKind::Flag,
        required: false,
        help: "Join tracks named as left and right into stereo tracks (default true).",
    },
];

/// `file.export_aaf` (plan 6f Task 6): an AAF export folder.
const EXPORT_AAF_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::File,
        required: false,
        help: "The folder to put the \"<song> AAF\" folder in (answers the Save dialog; needed unless dry_run). An existing export folder is never replaced: the new one is numbered.",
    },
    ParamSpec {
        name: "seq",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "The sequence (default: the one shown).",
    },
    ParamSpec {
        name: "embed",
        kind: ParamKind::Flag,
        required: false,
        help: "Embed the audio in the AAF file (default true); false writes it beside the file.",
    },
    ParamSpec {
        name: "depth",
        kind: ParamKind::Choice(&["16", "24"]),
        required: false,
        help: "The audio's bit depth (default 24).",
    },
    ParamSpec {
        name: "stereo",
        kind: ParamKind::Choice(&["mono_pairs", "stereo"]),
        required: false,
        help: "mono_pairs: a stereo track is two mono tracks; stereo: one stereo track.",
    },
    ParamSpec {
        name: "start_tc",
        kind: ParamKind::Choice(&["00:00:00:00", "01:00:00:00"]),
        required: false,
        help: "The timecode the sequence starts at (default 00:00:00:00).",
    },
    ParamSpec {
        name: "fps",
        kind: ParamKind::Choice(&["24", "25", "29.97df", "30"]),
        required: false,
        help: "The timecode frame rate (default 25).",
    },
    ParamSpec {
        name: "clip_gain",
        kind: ParamKind::Flag,
        required: false,
        help: "Write clip gain (default true).",
    },
    ParamSpec {
        name: "automation",
        kind: ParamKind::Flag,
        required: false,
        help: "Write fader and pan automation (default true).",
    },
    ParamSpec {
        name: "dry_run",
        kind: ParamKind::Flag,
        required: false,
        help: "Return what won't transfer; write nothing.",
    },
];

/// `file.export_for_daw` (plan 6b Task 3): an export folder for another DAW.
/// It never replaces anything: an existing folder's name gets " 2".
const FOR_DAW_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::File,
        required: false,
        help: "The folder to put the export folder in (answers the Save dialog; needed unless dry_run). An existing export folder is never replaced: the new one is numbered.",
    },
    ParamSpec {
        name: "seq",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "The sequence (default: the one shown).",
    },
    ParamSpec {
        name: "strips",
        kind: ParamKind::Text,
        required: false,
        help: "Strip IDs whose stems are written, comma-separated, or none for MIDI files only (default: the usual strips).",
    },
    ParamSpec {
        name: "container",
        kind: ParamKind::Choice(&["wav", "aiff", "caf"]),
        required: false,
        help: "The stems' file type: wav (default), aiff or caf.",
    },
    ParamSpec {
        name: "depth",
        kind: ParamKind::Choice(&["16", "24", "32f"]),
        required: false,
        help: "The stems' bit depth: 16, 24 or 32f (default: the project's).",
    },
    ParamSpec {
        name: "rate",
        kind: ParamKind::Int {
            min: 8000,
            max: 384000,
        },
        required: false,
        help: "The stems' sample rate in Hz (default: the project's).",
    },
    ParamSpec {
        name: "dither",
        kind: ParamKind::Flag,
        required: false,
        help: "Dither the stems (default: on at 16 bits only, off at 24; 32f never dithers, whatever this says).",
    },
    ParamSpec {
        name: "add_to_project",
        kind: ParamKind::Flag,
        required: false,
        help: "Add the stems to this project as heard (default false); one undo step.",
    },
    ParamSpec {
        name: "dry_run",
        kind: ParamKind::Flag,
        required: false,
        help: "Only check: answer what the export would do, write nothing.",
    },
];

/// A media item by ID (plan 5b).
const MEDIA_ID: ParamSpec = ParamSpec {
    name: "media",
    kind: ParamKind::Int {
        min: 0,
        max: i64::MAX / 2,
    },
    required: false,
    help: "The media item (see state.get media).",
};

const RELINK_PARAMS: &[ParamSpec] = &[
    MEDIA_ID,
    ParamSpec {
        name: "path",
        kind: ParamKind::File,
        required: false,
        help: "The file to relink the item to (answers the Relink dialog).",
    },
];

const RENAME_PARAMS: &[ParamSpec] = &[
    MEDIA_ID,
    ParamSpec {
        name: "name",
        kind: ParamKind::Text,
        required: false,
        help: "The item's new name (answers the Rename Media dialog).",
    },
];

const NORMALIZE_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "peak_db",
    kind: ParamKind::Text,
    required: false,
    help: "The peak level in dBFS, -96 to 0, as text (\"-0.3\"); answers the Normalize dialog.",
}];

/// `region.open_external` (plan 5f-3 Ruling 5).
const EXTERNAL_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "app",
    kind: ParamKind::Path("app"),
    required: false,
    help: "The editor application (an absolute path to a .app); default: Settings > Editing (answers the Choose External Editor dialog).",
}];

/// An ID parameter (a sequence, a lane), required.
const fn id_param(name: &'static str, help: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: true,
        help,
    }
}

/// An optional id parameter (an integer of at least 0).
const fn opt_id_param(name: &'static str, help: &'static str) -> ParamSpec {
    ParamSpec {
        required: false,
        ..id_param(name, help)
    }
}

/// `region.crossfade` (plan 6c Task 5).
const CROSSFADE_PARAMS: &[ParamSpec] = &[
    opt_id_param(
        "seq",
        "The sequence of the later region's track (give `seq`, `lane` and `event` together, or none: the selection).",
    ),
    opt_id_param(
        "lane",
        "The later region's track (give `seq`, `lane` and `event` together, or none: the selection).",
    ),
    opt_id_param(
        "event",
        "The later region (give `seq`, `lane` and `event` together, or none: the selection).",
    ),
    ParamSpec {
        name: "length_ms",
        kind: ParamKind::Text,
        required: false,
        help: "Milliseconds, more than 0, as text (\"12.5\"); default: the whole overlap, at most Settings > Editing's default crossfade.",
    },
    ParamSpec {
        name: "shape",
        kind: ParamKind::Choice(&["linear", "equal_power", "exponential", "s_curve"]),
        required: false,
        help: "The crossfade's shape (default: equal_power).",
    },
];

/// `region.set_fade_shape` (plan release-fixes Task 7).
const SET_FADE_SHAPE_PARAMS: &[ParamSpec] = &[
    opt_id_param(
        "seq",
        "The sequence of the region's track (give `seq`, `lane` and `event` together, or none: the selection).",
    ),
    opt_id_param(
        "lane",
        "The region's track (give `seq`, `lane` and `event` together, or none: the selection).",
    ),
    opt_id_param(
        "event",
        "The region (give `seq`, `lane` and `event` together, or none: the selection).",
    ),
    ParamSpec {
        name: "fade",
        kind: ParamKind::Choice(&["in", "out"]),
        required: true,
        help: "The fade in or the fade out (a crossfaded start: use region.crossfade's shape).",
    },
    ParamSpec {
        name: "shape",
        kind: ParamKind::Choice(&["linear", "equal_power", "exponential", "s_curve"]),
        required: true,
        help: "The fade's new shape; its length is kept.",
    },
];

/// `region.remove_crossfade` (plan 6c Task 5).
const REMOVE_CROSSFADE_PARAMS: &[ParamSpec] = &[
    opt_id_param(
        "seq",
        "The sequence of the later region's track (give `seq`, `lane` and `event` together, or none: the selection).",
    ),
    opt_id_param(
        "lane",
        "The later region's track (give `seq`, `lane` and `event` together, or none: the selection).",
    ),
    opt_id_param(
        "event",
        "The later region (give `seq`, `lane` and `event` together, or none: the selection).",
    ),
];

/// The quantize grids of `modify.quantize_notes` and their lengths in
/// ticks at 15 360 per quarter note (plan 8c Ruling 2).
pub const GRIDS: &[(&str, i64)] = &[
    ("1/4", 15360),
    ("1/8", 7680),
    ("1/16", 3840),
    ("1/32", 1920),
    ("1/8t", 5120),
    ("1/16t", 2560),
];

/// [`GRIDS`]' names, as `grid`'s choices (a test pins the two together).
const GRID_NAMES: &[&str] = &["1/4", "1/8", "1/16", "1/32", "1/8t", "1/16t"];

/// The latest tick a note chore or a marker takes.
const MAX_TICK: i64 = 1 << 40;

/// A required sequence ID (plan 8c Rulings 2 and 4). A song's first
/// sequence is 0, so 0 is allowed (plan 8c Task 2).
const CHORE_SEQ: ParamSpec = ParamSpec {
    name: "seq",
    kind: ParamKind::Int {
        min: 0,
        max: i64::MAX / 2,
    },
    required: true,
    help: "The sequence (see state.get document).",
};

/// The target parameters every note chore shares (plan 8c Ruling 2).
const NOTE_TARGET_PARAMS: [ParamSpec; 4] = [
    CHORE_SEQ,
    ParamSpec {
        name: "lanes",
        kind: ParamKind::Text,
        required: true,
        help: "Track IDs, comma-separated: 1 to 64, each once (see state.get document).",
    },
    ParamSpec {
        name: "start",
        kind: ParamKind::Int {
            min: 0,
            max: MAX_TICK,
        },
        required: false,
        help: "The range's first tick (give `start` and `end` together, or neither: the whole track).",
    },
    ParamSpec {
        name: "end",
        kind: ParamKind::Int {
            min: 0,
            max: MAX_TICK,
        },
        required: false,
        help: "The range's end tick, exclusive and after `start`.",
    },
];

/// `modify.quantize_notes` (plan 8c Ruling 2).
const QUANTIZE_NOTES_PARAMS: &[ParamSpec] = &[
    NOTE_TARGET_PARAMS[0],
    NOTE_TARGET_PARAMS[1],
    NOTE_TARGET_PARAMS[2],
    NOTE_TARGET_PARAMS[3],
    ParamSpec {
        name: "grid",
        kind: ParamKind::Choice(GRID_NAMES),
        required: true,
        help: "The grid: 1/4, 1/8, 1/16, 1/32, 1/8t or 1/16t.",
    },
    ParamSpec {
        name: "strength",
        kind: ParamKind::Int { min: 1, max: 100 },
        required: false,
        help: "How far each note moves toward the grid, in percent (default 100).",
    },
];

/// `modify.transpose_notes` (plan 8c Ruling 2).
const TRANSPOSE_NOTES_PARAMS: &[ParamSpec] = &[
    NOTE_TARGET_PARAMS[0],
    NOTE_TARGET_PARAMS[1],
    NOTE_TARGET_PARAMS[2],
    NOTE_TARGET_PARAMS[3],
    ParamSpec {
        name: "semitones",
        kind: ParamKind::Int { min: -48, max: 48 },
        required: true,
        help: "Semitones up (positive) or down (negative), -48 to 48, not 0.",
    },
];

/// `modify.velocity_notes` (plan 8c Ruling 2).
const VELOCITY_NOTES_PARAMS: &[ParamSpec] = &[
    NOTE_TARGET_PARAMS[0],
    NOTE_TARGET_PARAMS[1],
    NOTE_TARGET_PARAMS[2],
    NOTE_TARGET_PARAMS[3],
    ParamSpec {
        name: "op",
        kind: ParamKind::Choice(&["set", "add", "scale"]),
        required: true,
        help: "set: every velocity to `amount`; add: `amount` to each; scale: each to `amount` percent.",
    },
    ParamSpec {
        name: "amount",
        kind: ParamKind::Int {
            min: -126,
            max: 400,
        },
        required: true,
        help: "set: 1 to 127; add: -126 to 126, not 0; scale: 10 to 400.",
    },
];

/// A marker's 1-based place in its sequence, sorted by tick (plan 8c
/// Ruling 4).
const MARKER_INDEX: ParamSpec = ParamSpec {
    name: "index",
    kind: ParamKind::Int {
        min: 1,
        max: i64::MAX / 2,
    },
    required: true,
    help: "The marker, from 1 in tick order (see state.get document `markers`).",
};

/// A marker's name (plan 8c Ruling 4).
const MARKER_NAME: ParamSpec = ParamSpec {
    name: "name",
    kind: ParamKind::Text,
    required: true,
    help: "The marker's name: 1 to 64 characters.",
};

/// `marker.add` (plan 8c Ruling 4).
const MARKER_ADD_PARAMS: &[ParamSpec] = &[
    CHORE_SEQ,
    ParamSpec {
        name: "tick",
        kind: ParamKind::Int {
            min: 0,
            max: MAX_TICK,
        },
        required: true,
        help: "Where the marker goes, in ticks.",
    },
    MARKER_NAME,
];

/// `marker.rename` (plan 8c Ruling 4).
const MARKER_RENAME_PARAMS: &[ParamSpec] = &[CHORE_SEQ, MARKER_INDEX, MARKER_NAME];

/// `marker.remove` (plan 8c Ruling 4).
const MARKER_REMOVE_PARAMS: &[ParamSpec] = &[CHORE_SEQ, MARKER_INDEX];

/// `track.new_*` (plan tracks Ruling 5).
const NEW_TRACK_PARAMS: &[ParamSpec] = &[
    opt_id_param("seq", "The sequence (default: the selected one)."),
    opt_id_param(
        "after",
        "The track to put the new one below (default: the selection's first track, else the end).",
    ),
];

/// `track.duplicate` and `track.delete`.
const TRACK_PARAMS: &[ParamSpec] = &[
    opt_id_param("seq", "The sequence (default: the selected one)."),
    opt_id_param("lane", "The track (default: the selection's first track)."),
];

/// `track.rename`.
const TRACK_RENAME_PARAMS: &[ParamSpec] = &[
    opt_id_param("seq", "The sequence (default: the selected one)."),
    opt_id_param("lane", "The track (default: the selection's first track)."),
    ParamSpec {
        name: "name",
        kind: ParamKind::Text,
        required: false,
        help: "The track's new name (answers the Rename Track dialog).",
    },
];

/// `track.choose_take` and `track.delete_take` (plan 5c-2 takes Ruling 6).
const TAKE_PARAMS: &[ParamSpec] = &[
    opt_id_param("seq", "The sequence (default: the selected one)."),
    opt_id_param("lane", "The track (default: the selection's first track)."),
    id_param("take", "The take (see state.get `lanes` take_list)."),
];

/// `track.rename_take` (plan 5c-2 takes Ruling 6).
const TAKE_RENAME_PARAMS: &[ParamSpec] = &[
    opt_id_param("seq", "The sequence (default: the selected one)."),
    opt_id_param("lane", "The track (default: the selection's first track)."),
    id_param("take", "The take (see state.get `lanes` take_list)."),
    ParamSpec {
        name: "name",
        kind: ParamKind::Text,
        required: true,
        help: "The take's new name.",
    },
];

/// `record.set` (plan 5c Ruling 18): every option of the next Record.
const RECORD_SET_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "replace",
        kind: ParamKind::Flag,
        required: false,
        help: "Replace what the record range holds (true) or overdub it (false); sets transport.overdub.",
    },
    ParamSpec {
        name: "punch",
        kind: ParamKind::Flag,
        required: false,
        help: "Record only between the punch fields (sets transport.punch).",
    },
    ParamSpec {
        name: "loop",
        kind: ParamKind::Flag,
        required: false,
        help: "Loop recording over the transport loop (sets transport.loop).",
    },
    ParamSpec {
        name: "count_in",
        kind: ParamKind::Int { min: 0, max: 4 },
        required: false,
        help: "Bars of count-in: clicks before the transport rolls.",
    },
    ParamSpec {
        name: "pre_roll",
        kind: ParamKind::Int { min: 0, max: 8 },
        required: false,
        help: "Bars of pre-roll.",
    },
    ParamSpec {
        name: "passes_as_takes",
        kind: ParamKind::Flag,
        required: false,
        help: "MIDI loop passes become takes (audio always does).",
    },
];

const RECOVER_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "action",
    kind: ParamKind::Choice(&["import", "trash"]),
    required: true,
    help: "Import the recordings a crash left onto new tracks, or move them to the Trash.",
}];

const SET_INPUT_FX_PARAMS: &[ParamSpec] = &[
    id_param("seq", "The track's sequence."),
    id_param("lane", "The track."),
    ParamSpec {
        name: "chain",
        kind: ParamKind::Text,
        required: true,
        help: "The effect chain as JSON, first effect first: [{\"kind\": {\"transpose\": {\"semitones\": 12}}}, …]; [] clears it.",
    },
];

/// `zone`, `members` and `bend_range` of `lane.set_mpe` and
/// `destination.set_mpe` (plan 5g-2 Task 7).
const MPE_ZONE_PARAM: ParamSpec = ParamSpec {
    name: "zone",
    kind: ParamKind::Choice(&["off", "lower", "upper"]),
    required: true,
    help: "Which end of the channels the MPE zone sits at (lower: manager on channel 1; upper: manager on channel 16), or off for no MPE.",
};
const MPE_MEMBERS_PARAM: ParamSpec = ParamSpec {
    name: "members",
    kind: ParamKind::Int { min: 1, max: 15 },
    required: false,
    help: "Member channels, 1 to 15 (default 15). Not with zone off.",
};
const MPE_BEND_PARAM: ParamSpec = ParamSpec {
    name: "bend_range",
    kind: ParamKind::Int { min: 1, max: 96 },
    required: false,
    help: "Member pitch-bend range in semitones, 1 to 96 (default 48). Not with zone off.",
};

/// `lane.set_mpe`.
const SET_MPE_PARAMS: &[ParamSpec] = &[
    id_param("seq", "The track's sequence."),
    id_param("lane", "The track."),
    MPE_ZONE_PARAM,
    MPE_MEMBERS_PARAM,
    MPE_BEND_PARAM,
];

/// `destination.set_mpe`.
const DEST_MPE_PARAMS: &[ParamSpec] = &[
    id_param(
        "dest",
        "The destination (an external MIDI endpoint or a plugin instrument).",
    ),
    MPE_ZONE_PARAM,
    MPE_MEMBERS_PARAM,
    MPE_BEND_PARAM,
];

const SET_INPUT_PARAMS: &[ParamSpec] = &[
    id_param("seq", "The track's sequence."),
    id_param("lane", "The track."),
    ParamSpec {
        name: "kind",
        kind: ParamKind::Choice(&["none", "audio", "midi"]),
        required: true,
        help: "What the track records: nothing, audio or MIDI.",
    },
    ParamSpec {
        name: "channel",
        kind: ParamKind::Int { min: 1, max: 256 },
        required: false,
        help: "Audio: the input channel, from 1 (default 1).",
    },
    ParamSpec {
        name: "stereo",
        kind: ParamKind::Flag,
        required: false,
        help: "Audio: the channel and the next as a pair.",
    },
    ParamSpec {
        name: "port",
        kind: ParamKind::Text,
        required: false,
        help: "MIDI: the source's name (absent: every source).",
    },
    ParamSpec {
        name: "midi_channel",
        kind: ParamKind::Int { min: 1, max: 16 },
        required: false,
        help: "MIDI: the channel 1–16 (absent: every channel).",
    },
    ParamSpec {
        name: "return_of",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "Audio: the external MIDI destination whose return the track records (Track Synth); absent: plain audio, which unlinks a track that recorded a synth's return.",
    },
];

/// `lane.track_synth` (plan 5c-2 Ruling 11).
const TRACK_SYNTH_PARAMS: &[ParamSpec] = &[
    id_param("seq", "The MIDI track's sequence."),
    id_param("lane", "The MIDI track playing the external synth."),
    ParamSpec {
        name: "channel",
        kind: ParamKind::Int { min: 1, max: 256 },
        required: false,
        help: "The input channel the synth's output is wired to, from 1 (default 1).",
    },
    ParamSpec {
        name: "stereo",
        kind: ParamKind::Flag,
        required: false,
        help: "The channel and the next as a pair.",
    },
    ParamSpec {
        name: "arm_midi",
        kind: ParamKind::Flag,
        required: false,
        help: "Also arm the MIDI track to record the performance (default: disarm it, so its part plays and is never replaced).",
    },
    ParamSpec {
        name: "mute_after",
        kind: ParamKind::Flag,
        required: false,
        help: "Mute the MIDI track after the next recording, in the same undo step.",
    },
];

/// `destination.set_return_latency` (plan 5c-2 Ruling 11).
const RETURN_LATENCY_PARAMS: &[ParamSpec] = &[
    id_param("dest", "The external MIDI destination."),
    ParamSpec {
        name: "ms",
        kind: ParamKind::Text,
        required: true,
        help: "Its return latency in milliseconds, 0 to 500, as text (\"12.5\").",
    },
];

/// `destination.set_sound` (plan release-fixes Task 8).
const SET_SOUND_PARAMS: &[ParamSpec] = &[
    id_param("dest", "The Built-in Synth destination."),
    ParamSpec {
        name: "sound",
        kind: ParamKind::Choice(&["piano", "bass", "pad", "lead", "drums"]),
        required: true,
        help: "Its sound; every track that plays this Built-in Synth changes.",
    },
];

/// `destination.set_midi_mix` (plan 5g-1b Task 4).
const MIDI_MIX_PARAMS: &[ParamSpec] = &[
    id_param("dest", "The external MIDI destination."),
    ParamSpec {
        name: "volume",
        kind: ParamKind::IntOrNull { min: 0, max: 127 },
        required: false,
        help: "Volume (CC 7), 0 to 127, or null to send none. Absent keeps it.",
    },
    ParamSpec {
        name: "pan",
        kind: ParamKind::IntOrNull { min: 0, max: 127 },
        required: false,
        help: "Pan (CC 10), 0 to 127 (64 is the centre), or null to send none. Absent keeps it.",
    },
    ParamSpec {
        name: "mute",
        kind: ParamKind::Flag,
        required: false,
        help: "Mute the destination's notes. Absent keeps it. At least one of volume, pan and mute is required.",
    },
];

/// `audio.set` (plan 5d-1 Ruling 26): the app's audio settings.
const AUDIO_SET_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "software_monitoring",
        kind: ParamKind::Flag,
        required: false,
        help: "Play monitored inputs back through their strips (off: direct monitoring on the interface).",
    },
    ParamSpec {
        name: "buffer_frames",
        kind: ParamKind::Int { min: 32, max: 1024 },
        required: false,
        help: "The I/O buffer size: 32, 64, 128, 256, 512 or 1024 frames (refused while recording).",
    },
    ParamSpec {
        name: "input_device",
        kind: ParamKind::Text,
        required: false,
        help: "The input device's uid, or its name if no other device has that name.",
    },
    ParamSpec {
        name: "latency_trim",
        kind: ParamKind::Int {
            min: -192_000,
            max: 192_000,
        },
        required: false,
        help: "Frames added to where recorded audio lands, for a device that misreports its latency.",
    },
];

/// `destination.add_midi` (plan 5d-1 Ruling 26): New MIDI Destination.
const ADD_MIDI_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "name",
        kind: ParamKind::Text,
        required: true,
        help: "The destination's name.",
    },
    ParamSpec {
        name: "port",
        kind: ParamKind::Text,
        required: true,
        help: "The MIDI destination it plays (its CoreMIDI name).",
    },
    ParamSpec {
        name: "channel",
        kind: ParamKind::Int { min: 1, max: 16 },
        required: false,
        help: "The MIDI channel, 1 to 16 (default 1).",
    },
];

/// `automation.set_mode` (plan 5e-1 Ruling 17).
const AUTOMATION_MODE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "strips",
        kind: ParamKind::Text,
        required: true,
        help: "`all`, or strip IDs separated by commas (see `state.get automation`).",
    },
    ParamSpec {
        name: "mode",
        kind: ParamKind::Choice(&["off", "read", "touch", "latch", "write", "trim"]),
        required: true,
        help: "off, read, touch, latch, write or trim.",
    },
];

/// `automation.gesture` (plan 5e-1 Rulings 17, 20, 26).
const AUTOMATION_GESTURE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "target",
        kind: ParamKind::Text,
        required: true,
        help: "`fader:<strip>`, `pan:<strip>`, `mute:<strip>`, `solo:<strip>`, `send:<strip>:<n>` (n from 1) or `param:<slot>:<param>`.",
    },
    ParamSpec {
        name: "phase",
        kind: ParamKind::Choice(&["touch", "value", "release"]),
        required: true,
        help: "touch, value or release.",
    },
    ParamSpec {
        name: "value",
        kind: ParamKind::Text,
        required: false,
        help: "The normalised value 0–1 as text (\"0.5\"); needed for value, optional for touch.",
    },
];

/// A surface's ID (plan 5f-1 Ruling 4; `state.get surfaces` lists them).
const SURFACE_ID: ParamSpec = ParamSpec {
    name: "surface",
    kind: ParamKind::Int {
        min: 0,
        max: u32::MAX as i64,
    },
    required: true,
    help: "The surface's ID (see `state.get surfaces`).",
};

/// `surface.add` (plan 5f-1 Task 9).
const SURFACE_ADD_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "type",
        kind: ParamKind::Choice(&["mcu", "mcu_xt", "hui", "kontrol_mk3"]),
        required: true,
        help: "mcu, mcu_xt, hui or kontrol_mk3 (a host-integration keyboard's DAW port).",
    },
    ParamSpec {
        name: "input",
        kind: ParamKind::Text,
        required: true,
        help: "The MIDI source it is heard on (its CoreMIDI name).",
    },
    ParamSpec {
        name: "output",
        kind: ParamKind::Text,
        required: true,
        help: "The MIDI destination it is sent to (its CoreMIDI name).",
    },
    ParamSpec {
        name: "position",
        kind: ParamKind::Int { min: 0, max: 3 },
        required: false,
        help: "Its place in the MCU bank group, 0 to 3 (default: the first free).",
    },
];

/// `surface.remove` (plan 5f-1 Task 9).
const SURFACE_ID_PARAMS: &[ParamSpec] = &[SURFACE_ID];

/// `surface.inject` (plan 5f-1 Task 9).
const SURFACE_INJECT_PARAMS: &[ParamSpec] = &[
    SURFACE_ID,
    ParamSpec {
        name: "bytes",
        kind: ParamKind::Text,
        required: true,
        help: "What the surface sends, as hex bytes, e.g. \"90 68 7f\" (at most 1024).",
    },
];

/// `surface.learn` and `surface.unlearn` (plan 5f-1 Ruling 16): exactly
/// one of `target` and `command` (the app checks it).
const SURFACE_LEARN_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "target",
        kind: ParamKind::Text,
        required: false,
        help: "An automatable control, spelt as `automation.gesture` spells it (`fader:<strip>`, `pan:<strip>`, …).",
    },
    ParamSpec {
        name: "command",
        kind: ParamKind::Text,
        required: false,
        help: "A key-bindable registry command ID.",
    },
];

/// `destination.measure_return` (plan 5c-2 Ruling 11).
const MEASURE_RETURN_PARAMS: &[ParamSpec] = &[id_param(
    "dest",
    "The external MIDI destination to measure (a track must record its return: Track Synth). Blocks for about 3 s (at most 10 s) with the transport stopped.",
)];

const SET_MONITOR_PARAMS: &[ParamSpec] = &[
    id_param("seq", "The track's sequence."),
    id_param("lane", "The track."),
    ParamSpec {
        name: "mode",
        kind: ParamKind::Choice(&["auto", "input", "off"]),
        required: true,
        help: "When the track's input is heard: auto (armed, stopped or recording in range), input (always) or off.",
    },
];

/// `lane.set_output` (plan release-fixes Task 6).
const SET_OUTPUT_PARAMS: &[ParamSpec] = &[
    opt_id_param("seq", "The track's sequence (default: the shown one)."),
    opt_id_param(
        "lane",
        "The audio track (default: the selection's first track).",
    ),
    ParamSpec {
        name: "output",
        kind: ParamKind::IntOrNull {
            min: 0,
            max: i64::MAX / 2,
        },
        required: true,
        help: "The output path's id (state.get mixer), or null for no output.",
    },
];

const INJECT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "port",
        kind: ParamKind::Text,
        required: true,
        help: "The MIDI source the bytes come from, as tracks name it.",
    },
    ParamSpec {
        name: "bytes",
        kind: ParamKind::Text,
        required: true,
        help: "Channel messages as hex bytes, e.g. \"90 3c 64 80 3c 00\".",
    },
];

/// Every menu command, in menu order.
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        params: FORMAT_PARAMS,
        ..cmd("file.new", "File", "New", Some("⌘N"), false)
    },
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
    CommandSpec {
        class: Class::Add,
        params: IMPORT_PARAMS,
        dialogs: &[DialogSpec {
            name: "Open",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..cmd("media.import", "File", "Import Audio…", Some("⇧⌘I"), true)
    },
    cmd("file.close", "File", "Close", Some("⌘W"), true),
    CommandSpec {
        class: Class::Change,
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
    CommandSpec {
        class: Class::Change,
        params: FORMAT_PARAMS,
        dialogs: &[DialogSpec {
            name: "Project Settings",
            when: "always",
            always: true,
            answered_by: &["rate", "depth"],
        }],
        ..cmd(
            "file.project_settings",
            "File",
            "Project Settings…",
            None,
            true,
        )
    },
    CommandSpec {
        class: Class::Outward,
        params: BOUNCE_PARAMS,
        dialogs: &[DialogSpec {
            name: "Bounce",
            when: "always",
            always: true,
            answered_by: &["path", "add_to_project"],
        }],
        ..cmd("file.bounce", "File", "Bounce…", None, true)
    },
    CommandSpec {
        class: Class::Add,
        params: IMPORT_MIDI_PARAMS,
        dialogs: &[DialogSpec {
            name: "Open",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..cmd("file.import_midi", "File", "Import MIDI File…", None, true)
    },
    CommandSpec {
        class: Class::Outward,
        params: EXPORT_MIDI_PARAMS,
        dialogs: &[DialogSpec {
            name: "Save",
            when: "no path and no dry_run",
            always: false,
            answered_by: &["path"],
        }],
        ..cmd("file.export_midi", "File", "MIDI File…", None, true)
    },
    CommandSpec {
        class: Class::Outward,
        params: EXPORT_MIDI_CLIP_PARAMS,
        dialogs: &[DialogSpec {
            name: "Save",
            when: "no path and no dry_run",
            always: false,
            answered_by: &["path"],
        }],
        ..cmd(
            "file.export_midi_clip",
            "File",
            "MIDI Clip File…",
            None,
            true,
        )
    },
    CommandSpec {
        class: Class::Outward,
        params: FOR_DAW_PARAMS,
        dialogs: &[DialogSpec {
            name: "Save",
            when: "no path and no dry_run",
            always: false,
            answered_by: &["path"],
        }],
        ..cmd(
            "file.export_for_daw",
            "File",
            "For Another DAW…",
            None,
            true,
        )
    },
    CommandSpec {
        params: IMPORT_DAWPROJECT_PARAMS,
        dialogs: &[
            DialogSpec {
                name: "Open",
                when: "always",
                always: true,
                answered_by: &["path"],
            },
            DialogSpec {
                name: "Return",
                when: "the file came from VizDAW and no mode",
                always: false,
                answered_by: &["mode"],
            },
            DialogSpec {
                name: "Open",
                when: "merge with no matching open project",
                always: false,
                answered_by: &["project"],
            },
        ],
        ..cmd("file.import_dawproject", "File", "DAWproject…", None, true)
    },
    CommandSpec {
        class: Class::Outward,
        params: EXPORT_DAWPROJECT_PARAMS,
        dialogs: &[DialogSpec {
            name: "Save",
            when: "no path and no dry_run",
            always: false,
            answered_by: &["path"],
        }],
        ..cmd("file.export_dawproject", "File", "DAWproject…", None, true)
    },
    CommandSpec {
        params: IMPORT_AAF_PARAMS,
        dialogs: &[DialogSpec {
            name: "Open",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..cmd("file.import_aaf", "File", "AAF…", None, true)
    },
    CommandSpec {
        class: Class::Outward,
        params: EXPORT_AAF_PARAMS,
        dialogs: &[DialogSpec {
            name: "Save",
            when: "no path and no dry_run",
            always: false,
            answered_by: &["path"],
        }],
        ..cmd("file.export_aaf", "File", "AAF…", None, true)
    },
    CommandSpec {
        class: Class::Read,
        ..cmd(
            "file.interchange_report",
            "File",
            "Last Import or Export Report…",
            None,
            true,
        )
    },
    CommandSpec {
        class: Class::Change,
        menu_item: false,
        ..cmd(
            "media.convert",
            "File",
            "Convert Media to Project Format",
            None,
            true,
        )
    },
    CommandSpec {
        class: Class::Change,
        params: RELINK_PARAMS,
        dialogs: &[DialogSpec {
            name: "Relink",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..cmd("media.relink", "File", "Relink Media…", None, true)
    },
    key_only(cmd("media.compact", "Media", "Compact Media", None, true)),
    CommandSpec {
        class: Class::Change,
        params: RENAME_PARAMS,
        dialogs: &[DialogSpec {
            name: "Rename Media",
            when: "always",
            always: true,
            answered_by: &["name"],
        }],
        ..key_only(cmd("media.rename", "Media", "Rename Media…", None, true))
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("edit.undo", "Edit", "Undo", Some("⌘Z"), true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("edit.redo", "Edit", "Redo", Some("⇧⌘Z"), true)
    },
    cmd("window.mixer", "Window", "Mixer", Some("⌥⌘M"), true),
    cmd(
        "window.plugin_manager",
        "Window",
        "Plugin Manager",
        Some("⌥⌘P"),
        false,
    ),
    cmd("window.media_pool", "Window", "Media Pool", None, true),
    // Plan 8b Ruling 9: over a channel it only answers the notes' path.
    cmd("window.song_notes", "Window", "Song Notes", None, true),
    cmd("ui.view_arrange", "View", "Arrange View", Some("⌃1"), true),
    cmd("ui.view_mix", "View", "Mix View", Some("⌃2"), true),
    CommandSpec {
        params: ON,
        ..cmd("ui.toggle_left", "View", "Left Pane", Some("⌃⌘←"), true)
    },
    CommandSpec {
        params: ON,
        ..cmd("ui.toggle_bottom", "View", "Bottom Pane", Some("⌃⌘↓"), true)
    },
    CommandSpec {
        params: ON,
        ..cmd("ui.toggle_right", "View", "Right Pane", Some("⌃⌘→"), true)
    },
    CommandSpec {
        params: UI_PANE_PARAMS,
        ..key_only(cmd("ui.pane", "View", "Set Pane", None, true))
    },
    CommandSpec {
        params: UI_THEME_PARAMS,
        ..key_only(cmd("ui.theme", "View", "Theme", None, false))
    },
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
        "editors.audio_editor",
        "Editors",
        "Audio Editor",
        Some("⌘5"),
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
    CommandSpec {
        class: Class::Change,
        ..cmd(
            "editors.insert_event",
            "Editors",
            "Insert Event",
            Some("⌘I"),
            true,
        )
    },
    CommandSpec {
        class: Class::Change,
        ..cmd(
            "editors.nudge_earlier",
            "Editors",
            "Nudge Earlier",
            Some("⌥←"),
            true,
        )
    },
    CommandSpec {
        class: Class::Change,
        ..cmd(
            "editors.nudge_later",
            "Editors",
            "Nudge Later",
            Some("⌥→"),
            true,
        )
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("editors.nudge_up", "Editors", "Nudge Up", Some("⌥↑"), true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd(
            "editors.nudge_down",
            "Editors",
            "Nudge Down",
            Some("⌥↓"),
            true,
        )
    },
    CommandSpec {
        params: ON,
        ..cmd("editors.snap", "Editors", "Snap to Grid", Some("G"), true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd(
            "modify.select_by_rule",
            "Modify",
            "Select by Rule…",
            Some("⌃⌘S"),
            true,
        )
    },
    CommandSpec {
        class: Class::Change,
        ..cmd(
            "modify.transpose",
            "Modify",
            "Transpose…",
            Some("⌃⌘T"),
            true,
        )
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("modify.quantize", "Modify", "Quantize…", Some("⌃⌘U"), true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd(
            "modify.change",
            "Modify",
            "Change Values…",
            Some("⌃⌘M"),
            true,
        )
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("modify.move", "Modify", "Move…", Some("⌃⌘V"), true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("modify.again", "Modify", "Modify Again", Some("⌃⌘R"), true)
    },
    CommandSpec {
        class: Class::Change,
        params: &[ParamSpec {
            name: "name",
            kind: ParamKind::Text,
            required: true,
            help: "The template to run (Modify > Templates).",
        }],
        ..cmd("modify.template", "Modify", "Templates", None, true)
    },
    CommandSpec {
        class: Class::Change,
        params: ON,
        ..cmd(
            "modify.exact_mode",
            "Modify",
            "Exact Mode",
            Some("⌃⌘E"),
            true,
        )
    },
    CommandSpec {
        class: Class::Change,
        ..cmd(
            "sequence.make_reference",
            "Sequence",
            "Make Reference from Selection",
            Some("⌥⌘R"),
            true,
        )
    },
    cmd(
        "sequence.back",
        "Sequence",
        "Back to Parent Sequence",
        Some("⌘["),
        true,
    ),
    CommandSpec {
        class: Class::Change,
        ..cmd("drum.add_row", "Drum", "Add Drum Row", None, true)
    },
    CommandSpec {
        params: ON,
        ..cmd("drum.show_velocity", "Drum", "Show Velocity", None, true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd(
            "drum.split_row",
            "Drum",
            "Split Row to Own Track",
            None,
            true,
        )
    },
    CommandSpec {
        class: Class::Remove,
        ..cmd("drum.remove_row", "Drum", "Remove Drum Row", None, true)
    },
    CommandSpec {
        params: ON,
        ..cmd("drum.timeline", "Drum", "Timeline View", None, true)
    },
    CommandSpec {
        class: Class::Change,
        params: NORMALIZE_PARAMS,
        dialogs: &[DialogSpec {
            name: "Normalize",
            when: "always",
            always: true,
            answered_by: &["peak_db"],
        }],
        ..cmd("region.normalize", "Region", "Normalize…", None, true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd(
            "region.fade_in_to_cursor",
            "Region",
            "Fade In to Cursor",
            None,
            true,
        )
    },
    CommandSpec {
        class: Class::Change,
        ..cmd(
            "region.fade_out_from_cursor",
            "Region",
            "Fade Out from Cursor",
            None,
            true,
        )
    },
    CommandSpec {
        class: Class::Remove,
        ..cmd("region.remove_fades", "Region", "Remove Fades", None, true)
    },
    CommandSpec {
        class: Class::Change,
        params: CROSSFADE_PARAMS,
        ..cmd("region.crossfade", "Region", "Crossfade", None, true)
    },
    CommandSpec {
        class: Class::Change,
        params: SET_FADE_SHAPE_PARAMS,
        ..key_only(cmd(
            "region.set_fade_shape",
            "Region",
            "Set Fade Shape",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Remove,
        params: REMOVE_CROSSFADE_PARAMS,
        ..cmd(
            "region.remove_crossfade",
            "Region",
            "Remove Crossfade",
            None,
            true,
        )
    },
    CommandSpec {
        params: EXTERNAL_PARAMS,
        dialogs: &[DialogSpec {
            name: "Choose External Editor",
            when: "no editor is set in Settings > Editing",
            always: false,
            answered_by: &["app"],
        }],
        ..cmd(
            "region.open_external",
            "Region",
            "Open in External Editor",
            None,
            true,
        )
    },
    cmd(
        "region.reload_external",
        "Region",
        "Reload External Edit",
        None,
        true,
    ),
    CommandSpec {
        class: Class::Read,
        ..cmd(
            "transport.play_stop",
            "Transport",
            "Play / Stop",
            Some("Space"),
            true,
        )
    },
    CommandSpec {
        class: Class::Read,
        ..cmd("transport.stop", "Transport", "Stop", Some("S"), true)
    },
    // A surface's Play: only starts (plan 5f-1 Ruling 5).
    CommandSpec {
        class: Class::Read,
        ..key_only(cmd("transport.play", "Transport", "Play", None, true))
    },
    CommandSpec {
        class: Class::Read,
        ..cmd(
            "transport.play_selection",
            "Transport",
            "Play Selection",
            Some("⌥Space"),
            true,
        )
    },
    CommandSpec {
        class: Class::Read,
        ..cmd(
            "transport.play_from_selection",
            "Transport",
            "Play from Selection",
            Some("⇧Space"),
            true,
        )
    },
    CommandSpec {
        class: Class::Read,
        ..cmd(
            "transport.play_from_edit_in",
            "Transport",
            "Play from Edit-In",
            Some("Y"),
            true,
        )
    },
    cmd("transport.record", "Transport", "Record", Some("R"), true),
    CommandSpec {
        class: Class::Read,
        ..cmd("transport.scrub", "Transport", "Scrub", Some("K"), true)
    },
    CommandSpec {
        class: Class::Read,
        ..cmd(
            "transport.return_to_start",
            "Transport",
            "Return to Start",
            Some("Return"),
            true,
        )
    },
    // Plan 7d Ruling 4: a click (or the menu, or the channel) steps one
    // bar; the keys are new defaults, so they give way (ruling I3).
    CommandSpec {
        class: Class::Read,
        extra_keys: &["Keypad-"],
        yields: true,
        ..cmd("transport.rewind", "Transport", "Rewind", Some("⇧,"), true)
    },
    CommandSpec {
        class: Class::Read,
        extra_keys: &["Keypad+"],
        yields: true,
        ..cmd(
            "transport.fast_forward",
            "Transport",
            "Fast Forward",
            Some("⇧."),
            true,
        )
    },
    CommandSpec {
        class: Class::Read,
        ..cmd(
            "transport.previous_marker",
            "Transport",
            "Previous Marker",
            Some(","),
            true,
        )
    },
    CommandSpec {
        class: Class::Read,
        ..cmd(
            "transport.next_marker",
            "Transport",
            "Next Marker",
            Some("."),
            true,
        )
    },
    CommandSpec {
        class: Class::Read,
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
    CommandSpec {
        class: Class::Read,
        ..cmd(
            "transport.go_position",
            "Transport",
            "Go to Position Field",
            Some("P"),
            true,
        )
    },
    CommandSpec {
        class: Class::Read,
        ..cmd(
            "transport.go_edit_in",
            "Transport",
            "Go to Edit-In Field",
            Some("E"),
            true,
        )
    },
    CommandSpec {
        class: Class::Read,
        ..cmd(
            "transport.go_punch",
            "Transport",
            "Go to Punch Fields",
            Some("U"),
            true,
        )
    },
    cmd(
        "transport.arm_previous",
        "Transport",
        "Arm Previous Track",
        Some("⌥⇧↑"),
        true,
    ),
    cmd(
        "transport.arm_next",
        "Transport",
        "Arm Next Track",
        Some("⌥⇧↓"),
        true,
    ),
    CommandSpec {
        params: ON,
        ..cmd("step.entry", "Step", "Step Entry", Some("T"), true)
    },
    step("step.whole", "Whole Note", "1", &["Keypad1"]),
    step("step.half", "Half Note", "2", &["Keypad2"]),
    step("step.quarter", "Quarter Note", "3", &["Keypad3"]),
    step("step.eighth", "Eighth Note", "4", &["Keypad4"]),
    step("step.sixteenth", "Sixteenth Note", "5", &["Keypad5"]),
    step(
        "step.thirty_second",
        "Thirty-Second Note",
        "6",
        &["Keypad6"],
    ),
    step("step.dot", "Dotted", ".", &["Keypad."]),
    step("step.triplet", "Triplet", "/", &["Keypad/"]),
    step("step.tie", "Tie", "=", &["Keypad+"]),
    step("step.rest", "Rest", "0", &["Keypad0"]),
    step("step.delete_last", "Delete Last Step", "Delete", &[]),
    CommandSpec {
        params: &[ParamSpec {
            name: "pitches",
            kind: ParamKind::Text,
            required: true,
            help: "The chord to enter: note numbers or names (`60 64 67`, `C4 E4 G4`).",
        }],
        ..key_only(cmd("step.notes", "Step", "Enter Notes", None, true))
    },
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
    CommandSpec {
        params: RECORD_SET_PARAMS,
        ..key_only(cmd("record.set", "Record", "Record Options", None, true))
    },
    CommandSpec {
        params: RECOVER_PARAMS,
        ..key_only(cmd(
            "record.recover",
            "Record",
            "Recover Recordings",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: SET_INPUT_PARAMS,
        ..key_only(cmd(
            "lane.set_input",
            "Track",
            "Set Track Input",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: SET_MONITOR_PARAMS,
        ..key_only(cmd(
            "lane.set_monitor",
            "Track",
            "Set Track Monitor",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: SET_OUTPUT_PARAMS,
        ..key_only(cmd(
            "lane.set_output",
            "Track",
            "Set Track Output",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: SET_INPUT_FX_PARAMS,
        ..key_only(cmd(
            "lane.set_input_fx",
            "Track",
            "Set Input Effects",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: SET_MPE_PARAMS,
        ..key_only(cmd("lane.set_mpe", "Track", "Set Track MPE", None, true))
    },
    CommandSpec {
        params: INJECT_PARAMS,
        ..key_only(cmd("midi.inject", "MIDI", "Inject MIDI", None, true))
    },
    CommandSpec {
        class: Class::Change,
        params: TRACK_SYNTH_PARAMS,
        ..key_only(cmd("lane.track_synth", "Track", "Track Synth", None, true))
    },
    CommandSpec {
        class: Class::Change,
        params: RETURN_LATENCY_PARAMS,
        ..key_only(cmd(
            "destination.set_return_latency",
            "Destination",
            "Set Return Latency",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: SET_SOUND_PARAMS,
        ..key_only(cmd(
            "destination.set_sound",
            "Destination",
            "Set Built-in Synth Sound",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: MIDI_MIX_PARAMS,
        ..key_only(cmd(
            "destination.set_midi_mix",
            "Destination",
            "Set MIDI Mix",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: DEST_MPE_PARAMS,
        ..key_only(cmd(
            "destination.set_mpe",
            "Destination",
            "Set Destination MPE",
            None,
            true,
        ))
    },
    CommandSpec {
        params: MEASURE_RETURN_PARAMS,
        ..key_only(cmd(
            "destination.measure_return",
            "Destination",
            "Measure Return Latency",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: ADD_MIDI_PARAMS,
        ..key_only(cmd(
            "destination.add_midi",
            "Destination",
            "New MIDI Destination",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: AUTOMATION_MODE_PARAMS,
        ..key_only(cmd(
            "automation.set_mode",
            "Mixer",
            "Set Automation Mode",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: AUTOMATION_GESTURE_PARAMS,
        ..key_only(cmd(
            "automation.gesture",
            "Mixer",
            "Automation Gesture",
            None,
            true,
        ))
    },
    // Release every touch the control channel holds, e.g. after a script
    // stopped mid-gesture (release-fixes Task 4, Decision 2).
    CommandSpec {
        class: Class::Change,
        ..key_only(cmd(
            "automation.release_all",
            "Mixer",
            "Release Channel Touches",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("automation.mode.off", "Automation", "Off", None, true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("automation.mode.read", "Automation", "Read", None, true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("automation.mode.touch", "Automation", "Touch", None, true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("automation.mode.latch", "Automation", "Latch", None, true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("automation.mode.write", "Automation", "Write", None, true)
    },
    CommandSpec {
        class: Class::Change,
        ..cmd("automation.mode.trim", "Automation", "Trim", None, true)
    },
    CommandSpec {
        class: Class::Change,
        params: ON,
        ..cmd(
            "automation.suspend",
            "Automation",
            "Suspend Automation",
            None,
            false,
        )
    },
    CommandSpec {
        params: SURFACE_ADD_PARAMS,
        ..key_only(cmd("surface.add", "Surfaces", "Add Surface", None, false))
    },
    CommandSpec {
        params: SURFACE_ID_PARAMS,
        ..key_only(cmd(
            "surface.remove",
            "Surfaces",
            "Remove Surface",
            None,
            false,
        ))
    },
    CommandSpec {
        params: SURFACE_INJECT_PARAMS,
        ..key_only(cmd(
            "surface.inject",
            "Surfaces",
            "Inject Surface Bytes",
            None,
            true,
        ))
    },
    CommandSpec {
        params: SURFACE_LEARN_PARAMS,
        ..key_only(cmd("surface.learn", "Surfaces", "MIDI Learn", None, true))
    },
    CommandSpec {
        params: SURFACE_LEARN_PARAMS,
        ..key_only(cmd(
            "surface.unlearn",
            "Surfaces",
            "Remove MIDI Learn",
            None,
            true,
        ))
    },
    CommandSpec {
        params: AUDIO_SET_PARAMS,
        ..key_only(cmd("audio.set", "Audio", "Audio Settings", None, false))
    },
    locator(Class::Read, "locator.go_0", "Go to Locator 0", "0"),
    locator(Class::Read, "locator.go_1", "Go to Locator 1", "1"),
    locator(Class::Read, "locator.go_2", "Go to Locator 2", "2"),
    locator(Class::Read, "locator.go_3", "Go to Locator 3", "3"),
    locator(Class::Read, "locator.go_4", "Go to Locator 4", "4"),
    locator(Class::Read, "locator.go_5", "Go to Locator 5", "5"),
    locator(Class::Read, "locator.go_6", "Go to Locator 6", "6"),
    locator(Class::Read, "locator.go_7", "Go to Locator 7", "7"),
    locator(Class::Read, "locator.go_8", "Go to Locator 8", "8"),
    locator(Class::Read, "locator.go_9", "Go to Locator 9", "9"),
    locator(Class::Change, "locator.set_0", "Set Locator 0", "⌥0"),
    locator(Class::Change, "locator.set_1", "Set Locator 1", "⌥1"),
    locator(Class::Change, "locator.set_2", "Set Locator 2", "⌥2"),
    locator(Class::Change, "locator.set_3", "Set Locator 3", "⌥3"),
    locator(Class::Change, "locator.set_4", "Set Locator 4", "⌥4"),
    locator(Class::Change, "locator.set_5", "Set Locator 5", "⌥5"),
    locator(Class::Change, "locator.set_6", "Set Locator 6", "⌥6"),
    locator(Class::Change, "locator.set_7", "Set Locator 7", "⌥7"),
    locator(Class::Change, "locator.set_8", "Set Locator 8", "⌥8"),
    locator(Class::Change, "locator.set_9", "Set Locator 9", "⌥9"),
    locator(Class::Change, "locator.clear_0", "Clear Locator 0", "⌥⇧0"),
    locator(Class::Change, "locator.clear_1", "Clear Locator 1", "⌥⇧1"),
    locator(Class::Change, "locator.clear_2", "Clear Locator 2", "⌥⇧2"),
    locator(Class::Change, "locator.clear_3", "Clear Locator 3", "⌥⇧3"),
    locator(Class::Change, "locator.clear_4", "Clear Locator 4", "⌥⇧4"),
    locator(Class::Change, "locator.clear_5", "Clear Locator 5", "⌥⇧5"),
    locator(Class::Change, "locator.clear_6", "Clear Locator 6", "⌥⇧6"),
    locator(Class::Change, "locator.clear_7", "Clear Locator 7", "⌥⇧7"),
    locator(Class::Change, "locator.clear_8", "Clear Locator 8", "⌥⇧8"),
    locator(Class::Change, "locator.clear_9", "Clear Locator 9", "⌥⇧9"),
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
    CommandSpec {
        class: Class::Add,
        params: NEW_TRACK_PARAMS,
        yields: true,
        ..cmd(
            "track.new_audio",
            "Track",
            "New Audio Track",
            Some("⌥⌘A"),
            true,
        )
    },
    CommandSpec {
        class: Class::Add,
        params: NEW_TRACK_PARAMS,
        yields: true,
        ..cmd(
            "track.new_instrument",
            "Track",
            "New Instrument Track",
            Some("⌥⌘S"),
            true,
        )
    },
    CommandSpec {
        class: Class::Add,
        params: NEW_TRACK_PARAMS,
        yields: true,
        ..cmd(
            "track.new_midi",
            "Track",
            "New MIDI Track",
            Some("⌥⌘X"),
            true,
        )
    },
    CommandSpec {
        class: Class::Add,
        params: TRACK_PARAMS,
        ..cmd("track.duplicate", "Track", "Duplicate Track", None, true)
    },
    CommandSpec {
        class: Class::Remove,
        params: TRACK_PARAMS,
        ..cmd("track.delete", "Track", "Delete Track", None, true)
    },
    CommandSpec {
        class: Class::Change,
        params: TRACK_RENAME_PARAMS,
        dialogs: &[DialogSpec {
            name: "Rename Track",
            when: "always",
            always: true,
            answered_by: &["name"],
        }],
        ..cmd("track.rename", "Track", "Rename Track", None, true)
    },
    // Plan 8b Ruling 1: the composer's act alone, so refused to Co-Producer.
    CommandSpec {
        params: TRACK_PARAMS,
        ..cmd("track.make_mine", "Track", "Make Mine", None, true)
    },
    // Plan 5c-2 takes Ruling 6: the Takes menu's three, without dialogs.
    CommandSpec {
        class: Class::Change,
        params: TAKE_PARAMS,
        ..cmd("track.choose_take", "Track", "Choose Take", None, true)
    },
    CommandSpec {
        class: Class::Change,
        params: TAKE_RENAME_PARAMS,
        ..cmd("track.rename_take", "Track", "Rename Take", None, true)
    },
    CommandSpec {
        class: Class::Remove,
        params: TAKE_PARAMS,
        ..cmd("track.delete_take", "Track", "Delete Take", None, true)
    },
    CommandSpec {
        params: NEW_CHANNEL_EQ_PARAMS,
        ..key_only(cmd(
            "plugins.new_channel_eq",
            "Settings",
            "Put an EQ on New Channels",
            None,
            false,
        ))
    },
    CommandSpec {
        params: INSTALL_MULTIQ_PARAMS,
        dialogs: &[DialogSpec {
            name: "Replace",
            when: "a Multi-Q.component is already in the Components folder",
            always: false,
            answered_by: &["replace"],
        }],
        ..key_only(cmd(
            "plugins.install_multiq",
            "Settings",
            "Install Multi-Q for Other Apps",
            None,
            false,
        ))
    },
    CommandSpec {
        params: PLUGINS_DEFAULT_EQ_PARAMS,
        ..key_only(cmd(
            "plugins.default_eq",
            "Settings",
            "EQ for New Channels",
            None,
            false,
        ))
    },
    CommandSpec {
        class: Class::Add,
        params: MIXER_NEW_BUS_PARAMS,
        ..key_only(cmd("mixer.new_bus", "Mixer", "New Bus", None, true))
    },
    // Plan 8b Ruling 1: the composer's act alone.
    CommandSpec {
        params: MIXER_MAKE_MINE_PARAMS,
        ..key_only(cmd("mixer.make_mine", "Mixer", "Make Mine", None, true))
    },
    // Plan 8b Ruling 3: a strip's colour, previewable for Co-Producer.
    CommandSpec {
        class: Class::Change,
        params: MIXER_SET_COLOUR_PARAMS,
        ..key_only(cmd("mixer.set_colour", "Mixer", "Colour", None, true))
    },
    // Plan 8c Ruling 2: note chores with explicit values, previewable.
    CommandSpec {
        class: Class::Change,
        params: QUANTIZE_NOTES_PARAMS,
        ..key_only(cmd(
            "modify.quantize_notes",
            "Modify",
            "Quantize Notes",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: TRANSPOSE_NOTES_PARAMS,
        ..key_only(cmd(
            "modify.transpose_notes",
            "Modify",
            "Transpose Notes",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: VELOCITY_NOTES_PARAMS,
        ..key_only(cmd(
            "modify.velocity_notes",
            "Modify",
            "Change Velocity",
            None,
            true,
        ))
    },
    // Plan 8c Ruling 4: markers; a rename or removal is previewable.
    CommandSpec {
        class: Class::Add,
        params: MARKER_ADD_PARAMS,
        ..key_only(cmd("marker.add", "Transport", "Add Marker", None, true))
    },
    CommandSpec {
        class: Class::Change,
        params: MARKER_RENAME_PARAMS,
        ..key_only(cmd(
            "marker.rename",
            "Transport",
            "Rename Marker",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Remove,
        params: MARKER_REMOVE_PARAMS,
        ..key_only(cmd(
            "marker.remove",
            "Transport",
            "Remove Marker",
            None,
            true,
        ))
    },
    // Plan 8d Ruling 1: Co-Producer's mixer edits, each previewable, and
    // a new track on a plug-in instrument.
    CommandSpec {
        class: Class::Change,
        params: MIXER_SET_STRIP_PARAMS,
        ..key_only(cmd("mixer.set_strip", "Mixer", "Set Strip", None, true))
    },
    CommandSpec {
        class: Class::Change,
        params: MIXER_SET_OUTPUT_PARAMS,
        ..key_only(cmd(
            "mixer.set_output",
            "Mixer",
            "Set Strip Output",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: MIXER_ADD_SEND_PARAMS,
        ..key_only(cmd("mixer.add_send", "Mixer", "Add Send", None, true))
    },
    CommandSpec {
        class: Class::Change,
        params: MIXER_SET_SEND_PARAMS,
        ..key_only(cmd("mixer.set_send", "Mixer", "Set Send", None, true))
    },
    CommandSpec {
        class: Class::Remove,
        params: MIXER_REMOVE_SEND_PARAMS,
        ..key_only(cmd("mixer.remove_send", "Mixer", "Remove Send", None, true))
    },
    CommandSpec {
        class: Class::Change,
        params: MIXER_ADD_INSERT_PARAMS,
        ..key_only(cmd("mixer.add_insert", "Mixer", "Add Insert", None, true))
    },
    CommandSpec {
        class: Class::Remove,
        params: MIXER_SLOT_PARAMS,
        ..key_only(cmd(
            "mixer.remove_insert",
            "Mixer",
            "Remove Insert",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: MIXER_MOVE_INSERT_PARAMS,
        ..key_only(cmd("mixer.move_insert", "Mixer", "Move Insert", None, true))
    },
    CommandSpec {
        class: Class::Change,
        params: MIXER_SET_BYPASS_PARAMS,
        ..key_only(cmd(
            "mixer.set_bypass",
            "Mixer",
            "Bypass Insert",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Change,
        params: MIXER_SET_EQ_PARAMS,
        ..key_only(cmd("mixer.set_eq", "Mixer", "Set EQ", None, true))
    },
    CommandSpec {
        class: Class::Change,
        params: MIXER_SET_PARAM_PARAMS,
        ..key_only(cmd(
            "mixer.set_param",
            "Mixer",
            "Set Plug-in Parameter",
            None,
            true,
        ))
    },
    CommandSpec {
        class: Class::Remove,
        params: MIXER_REMOVE_BUS_PARAMS,
        ..key_only(cmd("mixer.remove_bus", "Mixer", "Remove Bus", None, true))
    },
    CommandSpec {
        class: Class::Add,
        params: NEW_PLUGIN_INSTRUMENT_PARAMS,
        ..key_only(cmd(
            "track.new_plugin_instrument",
            "Track",
            "New Plug-in Instrument Track",
            None,
            true,
        ))
    },
    // Plan 8a Ruling 10: the Co-Producer switch, for the e2e.
    CommandSpec {
        params: CO_PRODUCER_SWITCH_PARAMS,
        ..key_only(cmd(
            "settings.co_producer",
            "Settings",
            "Co-Producer",
            None,
            false,
        ))
    },
    // Plan 8b Ruling 11: the e2e's answer to VizDAW's approval sheet,
    // only with VIZDAW_TEST_HOOKS=1 and never on the Co-Producer socket.
    CommandSpec {
        params: CO_PRODUCER_ANSWER_PARAMS,
        ..key_only(cmd(
            "settings.co_producer_answer",
            "Settings",
            "Answer Co-Producer Approval",
            None,
            true,
        ))
    },
    // Plan 8a Ruling 9: a saved copy opens as a project of its own.
    CommandSpec {
        params: OPEN_COPY_PARAMS,
        dialogs: &[DialogSpec {
            name: "Open",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..cmd(
            "file.open_copy",
            "File",
            "Open Co-Producer Copy…",
            None,
            false,
        )
    },
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
            ParamKind::Int { min, max } => p.opt_int(s.name, min, max)?.map(ArgValue::Int),
            ParamKind::IntOrNull { min, max } => {
                p.opt_int_or_null(s.name, min, max)?.map(|v| match v {
                    Some(i) => ArgValue::Int(i),
                    None => ArgValue::Text(NULL_TEXT.to_string()),
                })
            }
            ParamKind::File => p
                .opt_file(s.name)?
                .map(|path| ArgValue::Text(path.to_string_lossy().into_owned())),
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
            ParamKind::ChoiceOrPrefixed { values, prefix } => match p.opt_str(s.name, MAX_PATH)? {
                None => None,
                Some(v)
                    if values.contains(&v.as_str())
                        || v.strip_prefix(prefix).is_some_and(is_slug) =>
                {
                    Some(ArgValue::Text(v))
                }
                Some(_) => {
                    return Err(p.error(format!(
                        "`{}` must be one of {}, or {prefix}name (lowercase letters, digits and hyphens)",
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
    if spec.id == "destination.set_midi_mix"
        && !["volume", "pan", "mute"]
            .iter()
            .any(|k| out.iter().any(|(n, _)| n == k))
    {
        return Err(p.error("give at least one of `volume`, `pan` and `mute`"));
    }
    mixer_rules(&p, spec.id, &out)?;
    chore_rules(&p, spec.id, &mut out)?;
    for d in spec.dialogs.iter().filter(|d| d.always) {
        if !d.answered_by.iter().any(|a| p.has(a)) {
            return Err(ControlError::dialog(d.name, d.answered_by));
        }
    }
    Ok(out)
}

fn int_arg(out: &[(String, ArgValue)], name: &str) -> Option<i64> {
    out.iter().find_map(|(n, v)| match v {
        ArgValue::Int(i) if n == name => Some(*i),
        _ => None,
    })
}

fn text_arg<'a>(out: &'a [(String, ArgValue)], name: &str) -> Option<&'a str> {
    out.iter().find_map(|(n, v)| match v {
        ArgValue::Text(t) if n == name => Some(t.as_str()),
        _ => None,
    })
}

/// Plan 8c Rulings 2 and 4's rules across parameters: a note chore's
/// `lanes` list, its `start`/`end` pair and its values, and a marker's
/// name (stored trimmed).
fn chore_rules(
    p: &Params<'_>,
    id: &str,
    out: &mut [(String, ArgValue)],
) -> Result<(), ControlError> {
    let note_chore = matches!(
        id,
        "modify.quantize_notes" | "modify.transpose_notes" | "modify.velocity_notes"
    );
    if note_chore {
        if let Some(lanes) = text_arg(out, "lanes") {
            crate::co_producer::id_list(lanes).map_err(|m| p.error(format!("`lanes`: {m}")))?;
        }
        match (int_arg(out, "start"), int_arg(out, "end")) {
            (None, None) => {}
            (Some(start), Some(end)) if end > start => {}
            (Some(_), Some(_)) => return Err(p.error("`end` must be after `start`")),
            _ => return Err(p.error("give `start` and `end` together, or neither")),
        }
    }
    if id == "modify.transpose_notes" && int_arg(out, "semitones") == Some(0) {
        return Err(p.error("`semitones` must not be 0"));
    }
    if id == "modify.velocity_notes" {
        let amount = int_arg(out, "amount").unwrap_or(0);
        let ok = match text_arg(out, "op") {
            Some("set") => (1..=127).contains(&amount),
            Some("add") => (-126..=126).contains(&amount) && amount != 0,
            Some("scale") => (10..=400).contains(&amount),
            _ => false,
        };
        if !ok {
            return Err(p.error(
                "`amount` must be 1 to 127 for set, -126 to 126 (not 0) for add, and 10 to 400 for scale",
            ));
        }
    }
    if matches!(id, "marker.add" | "marker.rename") {
        for (n, v) in out.iter_mut() {
            if let (true, ArgValue::Text(t)) = (n == "name", &*v) {
                *v = ArgValue::Text(crate::request::marker_name(t).map_err(|m| p.error(m))?);
            }
        }
    }
    Ok(())
}

/// Plan 8d Rulings 1 and 3's rules across parameters, for both sockets:
/// at least one of `mixer.set_strip`'s and `mixer.set_send`'s settings,
/// decimals as [`decimal`] spells them, and a plug-in reference's shape.
fn mixer_rules(p: &Params<'_>, id: &str, out: &[(String, ArgValue)]) -> Result<(), ControlError> {
    if !MIXER_IDS.contains(&id) {
        return Ok(());
    }
    let none_of = |keys: &[&str]| !keys.iter().any(|k| out.iter().any(|(n, _)| n == k));
    if id == "mixer.set_strip" && none_of(&["gain_db", "pan", "mute", "solo"]) {
        return Err(p.error("give at least one of `gain_db`, `pan`, `mute` and `solo`"));
    }
    if id == "mixer.set_send" && none_of(&["level_db", "pre_fader", "enabled"]) {
        return Err(p.error("give at least one of `level_db`, `pre_fader` and `enabled`"));
    }
    for (name, (min, max)) in [
        ("gain_db", LEVEL_DB),
        ("level_db", LEVEL_DB),
        ("value", PARAM_VALUE),
    ] {
        if let Some(t) = text_arg(out, name) {
            decimal(t, min, max).map_err(|m| p.error(format!("`{name}` {m}")))?;
        }
    }
    if let Some(t) = text_arg(out, "plugin") {
        plugin_ref(t, id == "track.new_plugin_instrument")
            .map_err(|m| p.error(format!("`plugin` {m}")))?;
    }
    Ok(())
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
        ParamKind::Int { min, max } => json!({"type": "int", "min": min, "max": max}),
        ParamKind::IntOrNull { min, max } => {
            json!({"type": "int_or_null", "min": min, "max": max})
        }
        ParamKind::File => json!({"type": "path"}),
        ParamKind::ChoiceOrPrefixed { values, prefix } => {
            json!({"type": "choice", "values": values, "or_prefixed": prefix})
        }
    }
}

/// The `commands.list` result.
pub fn list_json() -> Value {
    let commands: Vec<Value> = COMMANDS.iter().map(command_json).collect();
    json!({ "commands": commands })
}

/// One command as `commands.list` gives it, with its Co-Producer class.
pub fn command_json(c: &CommandSpec) -> Value {
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
        "class": c.class.name(),
    })
}
