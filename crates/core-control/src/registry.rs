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
    /// An absolute path with any extension.
    File,
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
        help: "Lane for a mono or stereo file; without it, new lanes.",
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
        help: "Kept loop passes become takes.",
    },
];

const RECOVER_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "action",
    kind: ParamKind::Choice(&["import", "trash"]),
    required: true,
    help: "Import the recordings a crash left onto new lanes, or move them to the Trash.",
}];

const SET_INPUT_PARAMS: &[ParamSpec] = &[
    id_param("seq", "The lane's sequence."),
    id_param("lane", "The lane."),
    ParamSpec {
        name: "kind",
        kind: ParamKind::Choice(&["none", "audio", "midi"]),
        required: true,
        help: "What the lane records: nothing, audio or MIDI.",
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
        help: "Audio: the external MIDI destination whose return the lane records (Track Synth); absent: plain audio, which unlinks a lane that recorded a synth's return.",
    },
];

/// `lane.track_synth` (plan 5c-2 Ruling 11).
const TRACK_SYNTH_PARAMS: &[ParamSpec] = &[
    id_param("seq", "The MIDI lane's sequence."),
    id_param("lane", "The MIDI lane playing the external synth."),
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
        help: "Also arm the MIDI lane to record the performance (default: disarm it, so its part plays and is never replaced).",
    },
    ParamSpec {
        name: "mute_after",
        kind: ParamKind::Flag,
        required: false,
        help: "Mute the MIDI lane after the next recording, in the same undo step.",
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
        help: "The audio input device by name (\"\": the output device's own input).",
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

/// `destination.measure_return` (plan 5c-2 Ruling 11).
const MEASURE_RETURN_PARAMS: &[ParamSpec] = &[id_param(
    "dest",
    "The external MIDI destination to measure (a lane must record its return: Track Synth). Blocks for about 3 s (at most 10 s) with the transport stopped.",
)];

const SET_MONITOR_PARAMS: &[ParamSpec] = &[
    id_param("seq", "The lane's sequence."),
    id_param("lane", "The lane."),
    ParamSpec {
        name: "mode",
        kind: ParamKind::Choice(&["auto", "input", "off"]),
        required: true,
        help: "When the lane's input is heard: auto (armed, stopped or recording in range), input (always) or off.",
    },
];

const INJECT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "port",
        kind: ParamKind::Text,
        required: true,
        help: "The MIDI source the bytes come from, as lanes name it.",
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
        params: RENAME_PARAMS,
        dialogs: &[DialogSpec {
            name: "Rename Media",
            when: "always",
            always: true,
            answered_by: &["name"],
        }],
        ..key_only(cmd("media.rename", "Media", "Rename Media…", None, true))
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
    cmd("window.media_pool", "Window", "Media Pool", None, true),
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
    CommandSpec {
        params: NORMALIZE_PARAMS,
        dialogs: &[DialogSpec {
            name: "Normalize",
            when: "always",
            always: true,
            answered_by: &["peak_db"],
        }],
        ..cmd("region.normalize", "Region", "Normalize…", None, true)
    },
    cmd(
        "region.fade_in_to_cursor",
        "Region",
        "Fade In to Cursor",
        None,
        true,
    ),
    cmd(
        "region.fade_out_from_cursor",
        "Region",
        "Fade Out from Cursor",
        None,
        true,
    ),
    cmd("region.remove_fades", "Region", "Remove Fades", None, true),
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
        "transport.go_position",
        "Transport",
        "Go to Position Field",
        Some("P"),
        true,
    ),
    cmd(
        "transport.go_edit_in",
        "Transport",
        "Go to Edit-In Field",
        Some("E"),
        true,
    ),
    cmd(
        "transport.go_punch",
        "Transport",
        "Go to Punch Fields",
        Some("U"),
        true,
    ),
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
        params: SET_INPUT_PARAMS,
        ..key_only(cmd("lane.set_input", "Lane", "Set Lane Input", None, true))
    },
    CommandSpec {
        params: SET_MONITOR_PARAMS,
        ..key_only(cmd(
            "lane.set_monitor",
            "Lane",
            "Set Lane Monitor",
            None,
            true,
        ))
    },
    CommandSpec {
        params: INJECT_PARAMS,
        ..key_only(cmd("midi.inject", "MIDI", "Inject MIDI", None, true))
    },
    CommandSpec {
        params: TRACK_SYNTH_PARAMS,
        ..key_only(cmd("lane.track_synth", "Lane", "Track Synth", None, true))
    },
    CommandSpec {
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
        params: AUDIO_SET_PARAMS,
        ..key_only(cmd("audio.set", "Audio", "Audio Settings", None, false))
    },
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
            ParamKind::Int { min, max } => p.opt_int(s.name, min, max)?.map(ArgValue::Int),
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
        ParamKind::Int { min, max } => json!({"type": "int", "min": min, "max": max}),
        ParamKind::File => json!({"type": "path"}),
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
