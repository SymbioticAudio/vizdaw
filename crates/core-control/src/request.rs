//! Validated commands (spec §5.5): every parameter of every command is
//! checked here, in Rust, before anything reaches the app. Validation is
//! total: any [`RawRequest`] gives a [`Command`] or an error, never a panic.

use crate::co_producer::{MAX_PLAN, Planned};
use crate::element::{self, ElementRef};
use crate::error::{ControlError, ErrorCode};
use crate::keys::{KeyStroke, Modifiers, named_key, text_strokes};
use crate::params::{MAX_PATH, MAX_STRING, Params, WindowRef};
use crate::protocol::{RawRequest, truncate};
use crate::registry::{self, ArgValue, OPEN_DIALOGS};
use crate::wait::{DEFAULT_TIMEOUT_MS, DETAIL_KINDS, MAX_TIMEOUT_MS, WaitCondition};
use serde_json::{Map, Value};
use std::path::PathBuf;

/// Every command name.
pub const COMMAND_NAMES: [&str; 24] = [
    "commands.list",
    "command.run",
    "state.get",
    "ui.tree",
    "ui.click",
    "ui.drag",
    "ui.key",
    "ui.screenshot",
    "app.new_demo",
    "app.open",
    "app.quit",
    "wait",
    "ping",
    "co_producer.begin",
    "co_producer.end",
    "co_producer.status",
    "co_producer.preview",
    "co_producer.apply",
    "co_producer.notes",
    "co_producer.note",
    "co_producer.export_folder",
    "co_producer.export",
    "co_producer.plugins",
    "co_producer.params",
];

/// The longest Co-Producer request name, in characters after trimming
/// (plan 8a Ruling 11).
pub const MAX_REQUEST_NAME: usize = 200;
/// The longest `asked` of `co_producer.begin`, in characters after
/// trimming (plan 8b Ruling 9).
pub const MAX_ASKED: usize = 500;
/// The longest `client` label of `co_producer.apply`, in characters after
/// trimming (plan 8b Ruling 11).
pub const MAX_CLIENT_LABEL: usize = 100;
/// The longest `co_producer.note` text, in characters (Ruling 9).
pub const MAX_NOTE: usize = 2000;
/// The most lines in one `co_producer.note` (Ruling 9).
pub const MAX_NOTE_LINES: usize = 20;
/// The longest marker name, in characters after trimming (plan 8c
/// Ruling 4).
pub const MAX_MARKER_NAME: usize = 64;

/// What a `co_producer.export_folder` is for (plan 8b Ruling 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportWhat {
    /// Stems.
    Stems,
    /// A mix.
    Mix,
    /// A selection.
    Selection,
    /// MIDI.
    Midi,
    /// A DAWproject file.
    Dawproject,
    /// AAF.
    Aaf,
    /// The export for another DAW.
    OtherDaw,
}

impl ExportWhat {
    /// Every value, in the order the protocol lists them.
    pub const ALL: [ExportWhat; 7] = [
        ExportWhat::Stems,
        ExportWhat::Mix,
        ExportWhat::Selection,
        ExportWhat::Midi,
        ExportWhat::Dawproject,
        ExportWhat::Aaf,
        ExportWhat::OtherDaw,
    ];

    /// Its protocol name.
    pub fn name(self) -> &'static str {
        match self {
            ExportWhat::Stems => "stems",
            ExportWhat::Mix => "mix",
            ExportWhat::Selection => "selection",
            ExportWhat::Midi => "midi",
            ExportWhat::Dawproject => "dawproject",
            ExportWhat::Aaf => "aaf",
            ExportWhat::OtherDaw => "other_daw",
        }
    }

    /// The value named `s` (`"stems"` … `"other_daw"`), exactly.
    pub fn parse(s: &str) -> Option<ExportWhat> {
        ExportWhat::ALL.into_iter().find(|w| w.name() == s)
    }

    /// How a folder name shows it (Ruling 6): "Stems" … "Other DAW".
    pub fn label(self) -> &'static str {
        match self {
            ExportWhat::Stems => "Stems",
            ExportWhat::Mix => "Mix",
            ExportWhat::Selection => "Selection",
            ExportWhat::Midi => "MIDI",
            ExportWhat::Dawproject => "DAWproject",
            ExportWhat::Aaf => "AAF",
            ExportWhat::OtherDaw => "Other DAW",
        }
    }
}

/// The kind of plug-in `co_producer.plugins` lists (plan 8d Ruling 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKindArg {
    /// Takes notes, makes audio.
    Instrument,
    /// Processes audio.
    Effect,
}

/// A preview token's length: 32 lowercase hex characters (Ruling 7).
pub const TOKEN_LEN: usize = 32;

/// The deepest `ui.tree` (and the default).
pub const MAX_TREE_DEPTH: u32 = 64;
/// The most intermediate points of a drag. The app remembers the events
/// it posts (`ControlUI.postedLimit`, with `ControlUI.maxGestureEvents`
/// = this + 2): raise those with this, or the drag's first press is lost.
pub const MAX_DRAG_STEPS: u32 = 200;
/// Intermediate points of a drag when `steps` is absent.
pub const DEFAULT_DRAG_STEPS: u32 = 16;

/// What `state.get` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateTarget {
    /// The target document's transport.
    Transport,
    /// Its snapshot, sequences, lanes and pending alert.
    Document,
    /// Its editors: detail pane, torn-off windows, tool, snap, modes.
    Editor,
    /// Its selection.
    Selection,
    /// Every VizDAW window, plus app-level alerts.
    Windows,
    /// Its media pool (5a).
    Media,
    /// Its project format and device rate (5a).
    Project,
    /// What recording is doing (plan 5c).
    Record,
    /// What the audio input offers (plan 5c). Its `levels` are each
    /// channel's peak since the last read (held, so a quiet read can show
    /// a recent peak), as long as the channel count at the last poll (one
    /// poll stale after a device change; 5d-2 T4 M3).
    Input,
    /// Its strips' automation modes, the touched targets and suspend
    /// (plan 5e-1).
    Automation,
    /// Control surfaces and MIDI learn (plan 5f-1).
    Surfaces,
    /// Its window's main view and panes (plan 7a).
    Layout,
    /// Its mixer's strips: kind and user colours, levels, routing, the
    /// EQ, inserts and sends (plan 7b Ruling 11).
    Mixer,
    /// The audio editor: the shown region, its zoom and its typed boxes
    /// (plan 7c Ruling 10).
    AudioEditor,
}

/// Where a click or drag lands.
#[derive(Debug, Clone, PartialEq)]
pub enum Pointer {
    /// The centre of an element from `ui.tree`.
    Element(ElementRef),
    /// A point in window points, top-left origin (as in screenshots).
    Point {
        /// From the window's left edge.
        x: f64,
        /// From the window's top edge.
        y: f64,
    },
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    /// Primary.
    Left,
    /// Secondary (context menus).
    Right,
}

/// An answer to the after-crash question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CrashAnswer {
    /// Open Normally.
    Normal,
    /// Open with Plugins Off.
    Off,
    /// Disable the plugins that were loading, then open.
    Disable,
}

/// The answers to the dialogs opening a project may ask (spec §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RecoveryAnswers {
    /// "Open with Plugins Off?"
    pub plugins: Option<CrashAnswer>,
    /// "Restore Autosave"
    pub restore_autosave: Option<bool>,
}

/// A validated request.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Answered by the server: the registry.
    CommandsList,
    /// Answered by the server: `{ "pong": true }`.
    Ping,
    /// Runs a registered menu command.
    CommandRun {
        /// Registry ID.
        id: String,
        /// Validated arguments.
        args: Vec<(String, ArgValue)>,
        /// The window whose document it acts on.
        window: Option<WindowRef>,
    },
    /// Reads state.
    StateGet {
        /// What.
        target: StateTarget,
        /// Whose (a document window); absent: the front document.
        window: Option<WindowRef>,
    },
    /// A window's accessibility tree.
    UiTree {
        /// Which window; absent: the front document window.
        window: Option<WindowRef>,
        /// How deep.
        max_depth: u32,
    },
    /// A click.
    UiClick {
        /// Which window.
        window: Option<WindowRef>,
        /// Where.
        at: Pointer,
        /// Which button.
        button: Button,
        /// 1 to 3 (double, triple click).
        count: u32,
        /// Modifiers held.
        modifiers: Modifiers,
    },
    /// A drag.
    UiDrag {
        /// Which window.
        window: Option<WindowRef>,
        /// Press here.
        from: Pointer,
        /// Release here.
        to: Pointer,
        /// Intermediate points.
        steps: u32,
        /// Modifiers held.
        modifiers: Modifiers,
    },
    /// Key strokes.
    UiKey {
        /// Which window.
        window: Option<WindowRef>,
        /// Clicked first, to give it the keyboard.
        focus: Option<ElementRef>,
        /// In order.
        strokes: Vec<KeyStroke>,
    },
    /// A PNG of a window.
    UiScreenshot {
        /// Which window.
        window: Option<WindowRef>,
        /// Where; absent: a new file in the socket's directory.
        path: Option<PathBuf>,
        /// Pixels per point; absent: the window's backing scale.
        scale: Option<f64>,
        /// Capture the window as the window server shows it (needs Screen
        /// Recording access; never prompts) instead of drawing it in-app.
        live: bool,
    },
    /// Creates the demo project in a new window.
    AppNewDemo {
        /// The demo with an audio lane.
        audio: bool,
    },
    /// Opens a project in a new window.
    AppOpen {
        /// The package.
        path: PathBuf,
        /// Answers to the recovery dialogs.
        answers: RecoveryAnswers,
    },
    /// Quits the app (after replying).
    AppQuit,
    /// Waits for a condition.
    Wait {
        /// What.
        condition: WaitCondition,
        /// Whose (for document conditions).
        window: Option<WindowRef>,
        /// At most this long.
        timeout_ms: u64,
    },
    /// Opens a Co-Producer request: one named undo step (plan 8a).
    CoProducerBegin {
        /// What the composer asked for, trimmed.
        name: String,
        /// The composer's own words for the notes (plan 8b Ruling 9):
        /// trimmed, newlines as spaces; None when absent or blank.
        asked: Option<String>,
        /// Whose document.
        window: Option<WindowRef>,
    },
    /// Ends the open request.
    CoProducerEnd {
        /// Whose document.
        window: Option<WindowRef>,
    },
    /// The open request, if any.
    CoProducerStatus {
        /// Whose document.
        window: Option<WindowRef>,
    },
    /// Works out what a plan of change and remove commands would do.
    CoProducerPreview {
        /// The planned commands, in order.
        plan: Vec<Planned>,
        /// Whose document.
        window: Option<WindowRef>,
    },
    /// Runs a previewed plan with the composer's yes.
    CoProducerApply {
        /// The preview's one-time token.
        token: String,
        /// The previewed commands, again.
        plan: Vec<Planned>,
        /// Whose document.
        window: Option<WindowRef>,
        /// What the host calls itself (`vizdaw-mcp` passes its client's
        /// `clientInfo.name`): a label VizDAW shows, never proof of who
        /// is asking (plan 8b Ruling 11).
        client: Option<String>,
    },
    /// Reads the song's notes (plan 8b Ruling 9).
    CoProducerNotes {
        /// Whose document.
        window: Option<WindowRef>,
    },
    /// Adds a note to the open request's entry.
    CoProducerNote {
        /// 1 to [`MAX_NOTE`] characters in at most [`MAX_NOTE_LINES`] lines.
        text: String,
        /// Whose document.
        window: Option<WindowRef>,
    },
    /// Makes a new dated export folder (Ruling 6).
    CoProducerExportFolder {
        /// What the export is.
        what: ExportWhat,
        /// The folder to make it in; absent: the remembered one, else the
        /// song's own. Checked by the gate, not here.
        parent: Option<PathBuf>,
        /// Whose document.
        window: Option<WindowRef>,
    },
    /// Exports the song in one call into a new dated folder (plan 8c
    /// Ruling 1).
    CoProducerExport {
        /// What to export; never [`ExportWhat::Selection`].
        what: ExportWhat,
        /// The folder to make the dated folder in; absent: the remembered
        /// one, else the song's own. Checked by the gate, not here.
        parent: Option<PathBuf>,
        /// The strips to bounce, only for stems and other_daw; absent:
        /// For Another DAW's default strips.
        strips: Option<Vec<u64>>,
        /// Whose document.
        window: Option<WindowRef>,
    },
    /// Lists the installed plug-ins Co-Producer may use (plan 8d
    /// Ruling 5): a read.
    CoProducerPlugins {
        /// Only instruments or only effects; absent: both.
        kind: Option<PluginKindArg>,
    },
    /// Reads a loaded plug-in's parameters (plan 8d Ruling 5): a read.
    CoProducerParams {
        /// The slot, from 1.
        slot: u64,
        /// Whose document.
        window: Option<WindowRef>,
    },
}

impl Command {
    /// Whether the server answers it without the app.
    pub fn is_local(&self) -> bool {
        matches!(self, Command::CommandsList | Command::Ping)
    }

    /// Whether it is one of the `co_producer.*` commands, which only the
    /// Co-Producer socket accepts.
    pub fn is_co_producer(&self) -> bool {
        matches!(
            self,
            Command::CoProducerBegin { .. }
                | Command::CoProducerEnd { .. }
                | Command::CoProducerStatus { .. }
                | Command::CoProducerPreview { .. }
                | Command::CoProducerApply { .. }
                | Command::CoProducerNotes { .. }
                | Command::CoProducerNote { .. }
                | Command::CoProducerExportFolder { .. }
                | Command::CoProducerExport { .. }
                | Command::CoProducerPlugins { .. }
                | Command::CoProducerParams { .. }
        )
    }
}

/// Validates `raw` into a command.
pub fn validate(raw: &RawRequest) -> Result<Command, ControlError> {
    let p = Params::new(&raw.cmd, &raw.params);
    match raw.cmd.as_str() {
        "commands.list" => {
            p.only(&[])?;
            Ok(Command::CommandsList)
        }
        "ping" => {
            p.only(&[])?;
            Ok(Command::Ping)
        }
        "command.run" => command_run(&p, &raw.params),
        "state.get" => {
            p.only(&["target", "window"])?;
            let target = match p.str("target", 32)?.as_str() {
                "transport" => StateTarget::Transport,
                "document" => StateTarget::Document,
                "editor" => StateTarget::Editor,
                "selection" => StateTarget::Selection,
                "windows" => StateTarget::Windows,
                "media" => StateTarget::Media,
                "project" => StateTarget::Project,
                "record" => StateTarget::Record,
                "input" => StateTarget::Input,
                "automation" => StateTarget::Automation,
                "surfaces" => StateTarget::Surfaces,
                "layout" => StateTarget::Layout,
                "mixer" => StateTarget::Mixer,
                "audio_editor" => StateTarget::AudioEditor,
                _ => {
                    return Err(p.error(
                        "`target` must be transport, document, editor, selection, windows, media, project, record, input, automation, surfaces, layout, mixer or audio_editor",
                    ));
                }
            };
            Ok(Command::StateGet {
                target,
                window: p.window()?,
            })
        }
        "ui.tree" => {
            p.only(&["window", "max_depth"])?;
            Ok(Command::UiTree {
                window: p.window()?,
                max_depth: p
                    .opt_int("max_depth", 1, i64::from(MAX_TREE_DEPTH))?
                    .map_or(MAX_TREE_DEPTH, |d| d as u32),
            })
        }
        "ui.click" => {
            p.only(&["window", "element", "point", "button", "count", "modifiers"])?;
            let at = match (p.raw("element"), p.raw("point")) {
                (Some(e), None) => pointer(&p, "element", e)?,
                (None, Some(pt)) => {
                    let (x, y) = p.point_of("point", pt)?;
                    Pointer::Point { x, y }
                }
                _ => return Err(p.error("give exactly one of `element` and `point`")),
            };
            let button = match p.opt_str("button", 8)?.as_deref() {
                None | Some("left") => Button::Left,
                Some("right") => Button::Right,
                Some(_) => return Err(p.error("`button` must be left or right")),
            };
            Ok(Command::UiClick {
                window: p.window()?,
                at,
                button,
                count: p.opt_int("count", 1, 3)?.map_or(1, |c| c as u32),
                modifiers: modifiers(&p)?,
            })
        }
        "ui.drag" => {
            p.only(&["window", "from", "to", "steps", "modifiers"])?;
            let from = p.raw("from").ok_or_else(|| p.error("missing `from`"))?;
            let to = p.raw("to").ok_or_else(|| p.error("missing `to`"))?;
            Ok(Command::UiDrag {
                window: p.window()?,
                from: pointer(&p, "from", from)?,
                to: pointer(&p, "to", to)?,
                steps: p
                    .opt_int("steps", 1, i64::from(MAX_DRAG_STEPS))?
                    .map_or(DEFAULT_DRAG_STEPS, |s| s as u32),
                modifiers: modifiers(&p)?,
            })
        }
        "ui.key" => {
            p.only(&["window", "key", "text", "modifiers", "element"])?;
            let m = modifiers(&p)?;
            let strokes = match (p.opt_str("key", 32)?, p.opt_str("text", MAX_STRING)?) {
                (Some(k), None) => vec![named_key(&k, m).map_err(|e| p.error(e))?],
                (None, Some(t)) if !p.has("modifiers") => {
                    text_strokes(&t).map_err(|e| p.error(e))?
                }
                (None, Some(_)) => return Err(p.error("`modifiers` go with `key`, not `text`")),
                _ => return Err(p.error("give exactly one of `key` and `text`")),
            };
            let focus = match p.opt_str("element", 512)? {
                None => None,
                Some(e) => Some(element::parse(&e).map_err(|m| p.error(m))?),
            };
            Ok(Command::UiKey {
                window: p.window()?,
                focus,
                strokes,
            })
        }
        "ui.screenshot" => {
            p.only(&["window", "path", "scale", "live"])?;
            let path = p.opt_path("path", "png")?;
            if let Some(dir) = path.as_ref().and_then(|p| p.parent()) {
                if !dir.is_dir() {
                    return Err(
                        p.error(format!("`path`'s folder {} does not exist", dir.display()))
                    );
                }
            }
            Ok(Command::UiScreenshot {
                window: p.window()?,
                path,
                scale: p.opt_num("scale", 0.25, 4.0)?,
                live: p.opt_bool("live")?.unwrap_or(false),
            })
        }
        "app.new_demo" => {
            p.only(&["audio"])?;
            Ok(Command::AppNewDemo {
                audio: p.opt_bool("audio")?.unwrap_or(false),
            })
        }
        "app.open" => {
            p.only(&["path", "plugins", "restore_autosave"])?;
            let Some(path) = p.opt_path("path", "vizdaw")? else {
                return match OPEN_DIALOGS.first() {
                    Some(d) => Err(ControlError::dialog(d.name, d.answered_by)),
                    None => Err(p.error("missing `path`")),
                };
            };
            let plugins = match p.opt_str("plugins", 16)?.as_deref() {
                None => None,
                Some("normal") => Some(CrashAnswer::Normal),
                Some("off") => Some(CrashAnswer::Off),
                Some("disable") => Some(CrashAnswer::Disable),
                Some(_) => return Err(p.error("`plugins` must be normal, off or disable")),
            };
            Ok(Command::AppOpen {
                path,
                answers: RecoveryAnswers {
                    plugins,
                    restore_autosave: p.opt_bool("restore_autosave")?,
                },
            })
        }
        "app.quit" => {
            p.only(&[])?;
            Ok(Command::AppQuit)
        }
        "wait" => wait(&p),
        "co_producer.begin" => {
            p.only(&["name", "asked", "window"])?;
            Ok(Command::CoProducerBegin {
                name: request_name(&p)?,
                asked: asked(&p)?,
                window: p.window()?,
            })
        }
        "co_producer.end" => {
            p.only(&["window"])?;
            Ok(Command::CoProducerEnd {
                window: p.window()?,
            })
        }
        "co_producer.status" => {
            p.only(&["window"])?;
            Ok(Command::CoProducerStatus {
                window: p.window()?,
            })
        }
        "co_producer.preview" => {
            p.only(&["commands", "window"])?;
            Ok(Command::CoProducerPreview {
                plan: plan(&p)?,
                window: p.window()?,
            })
        }
        "co_producer.apply" => {
            p.only(&["token", "commands", "window", "client"])?;
            Ok(Command::CoProducerApply {
                token: token(&p)?,
                plan: plan(&p)?,
                window: p.window()?,
                client: client(&p)?,
            })
        }
        "co_producer.notes" => {
            p.only(&["window"])?;
            Ok(Command::CoProducerNotes {
                window: p.window()?,
            })
        }
        "co_producer.note" => {
            p.only(&["text", "window"])?;
            Ok(Command::CoProducerNote {
                text: note(&p)?,
                window: p.window()?,
            })
        }
        "co_producer.export_folder" => {
            p.only(&["what", "parent", "window"])?;
            Ok(Command::CoProducerExportFolder {
                what: export_what(&p)?,
                parent: export_parent(&p)?,
                window: p.window()?,
            })
        }
        "co_producer.export" => {
            p.only(&["what", "parent", "strips", "window"])?;
            let what = chore_export_what(&p)?;
            let strips = match p.opt_str("strips", MAX_STRING)? {
                None => None,
                Some(_) if !matches!(what, ExportWhat::Stems | ExportWhat::OtherDaw) => {
                    return Err(p.error("strips is only for stems and other_daw."));
                }
                Some(s) => Some(
                    crate::co_producer::id_list(&s)
                        .map_err(|m| p.error(format!("`strips`: {m}")))?,
                ),
            };
            Ok(Command::CoProducerExport {
                what,
                parent: export_parent(&p)?,
                strips,
                window: p.window()?,
            })
        }
        "co_producer.plugins" => {
            p.only(&["kind"])?;
            let kind = match p.opt_str("kind", 32)?.as_deref() {
                None => None,
                Some("instrument") => Some(PluginKindArg::Instrument),
                Some("effect") => Some(PluginKindArg::Effect),
                Some(_) => return Err(p.error("`kind` must be instrument or effect")),
            };
            Ok(Command::CoProducerPlugins { kind })
        }
        "co_producer.params" => {
            p.only(&["slot", "window"])?;
            Ok(Command::CoProducerParams {
                slot: p.int("slot", 1, registry::MAX_EXACT_ID)? as u64,
                window: p.window()?,
            })
        }
        other => Err(ControlError::new(
            ErrorCode::UnknownCommand,
            format!(
                "no command {:?} (commands: {})",
                truncate(other, 64),
                COMMAND_NAMES.join(", ")
            ),
        )),
    }
}

fn modifiers(p: &Params<'_>) -> Result<Modifiers, ControlError> {
    Modifiers::parse(p.raw("modifiers")).map_err(|e| p.error(e))
}

/// An element ID (a string) or a point (`{x, y}`).
fn pointer(p: &Params<'_>, name: &str, v: &Value) -> Result<Pointer, ControlError> {
    match v {
        Value::String(s) => element::parse(s)
            .map(Pointer::Element)
            .map_err(|m| p.error(format!("`{name}`: {m}"))),
        _ => {
            let (x, y) = p.point_of(name, v)?;
            Ok(Pointer::Point { x, y })
        }
    }
}

fn command_run(p: &Params<'_>, all: &Map<String, Value>) -> Result<Command, ControlError> {
    p.only(&["id", "params", "window"])?;
    let id = p.str("id", 64)?;
    let spec = registry::find(&id).ok_or_else(|| registry::unknown(&id))?;
    let empty = Map::new();
    let args = match all.get("params") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(m)) => m,
        Some(_) => return Err(p.error("`params` must be an object")),
    };
    Ok(Command::CommandRun {
        id,
        args: registry::validate_args(spec, args)?,
        window: p.window()?,
    })
}

/// A character a request name may not hold (plan 8a Ruling 4): a C0 or C1
/// control, a bidi control or mark, or a line or paragraph separator.
pub(crate) fn refused_in_name(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}'
                | '\u{200E}'
                | '\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{2028}'
                | '\u{2029}'
        )
}

/// `co_producer.begin`'s `name`: 1 to [`MAX_REQUEST_NAME`] characters after
/// trimming, with no control, bidi or separator character.
fn request_name(p: &Params<'_>) -> Result<String, ControlError> {
    let raw = p.str("name", MAX_STRING)?;
    let name = raw.trim();
    if name.is_empty() {
        return Err(p.error("`name` must not be empty"));
    }
    if name.chars().count() > MAX_REQUEST_NAME {
        return Err(p.error(format!(
            "`name` is longer than {MAX_REQUEST_NAME} characters"
        )));
    }
    if name.chars().any(refused_in_name) {
        return Err(p.error("`name` must not hold control, direction or line-separator characters"));
    }
    Ok(name.to_string())
}

/// `co_producer.begin`'s optional `asked`: at most [`MAX_ASKED`] characters
/// after trimming, newlines as spaces, and no other control, bidi or
/// separator character (plan 8b Ruling 9). Blank is absent.
fn asked(p: &Params<'_>) -> Result<Option<String>, ControlError> {
    let Some(raw) = p.opt_str("asked", MAX_STRING)? else {
        return Ok(None);
    };
    let asked = raw.trim().replace('\n', " ");
    if asked.is_empty() {
        return Ok(None);
    }
    if asked.chars().count() > MAX_ASKED {
        return Err(p.error(format!("`asked` is longer than {MAX_ASKED} characters")));
    }
    if asked.chars().any(refused_in_name) {
        return Err(
            p.error("`asked` must not hold control, direction or line-separator characters")
        );
    }
    Ok(Some(asked))
}

/// `co_producer.apply`'s optional `client`: at most [`MAX_CLIENT_LABEL`]
/// characters after trimming, with no control, bidi or separator
/// character (plan 8b Ruling 11). Blank is absent.
fn client(p: &Params<'_>) -> Result<Option<String>, ControlError> {
    let Some(raw) = p.opt_str("client", MAX_STRING)? else {
        return Ok(None);
    };
    let client = raw.trim();
    if client.is_empty() {
        return Ok(None);
    }
    if client.chars().count() > MAX_CLIENT_LABEL {
        return Err(p.error(format!(
            "`client` is longer than {MAX_CLIENT_LABEL} characters"
        )));
    }
    if client.chars().any(refused_in_name) {
        return Err(
            p.error("`client` must not hold control, direction or line-separator characters")
        );
    }
    Ok(Some(client.to_string()))
}

/// `co_producer.note`'s `text`: 1 to [`MAX_NOTE`] characters, not blank,
/// in at most [`MAX_NOTE_LINES`] lines, with no control but `\n` and no
/// bidi or separator character (plan 8b Ruling 9).
fn note(p: &Params<'_>) -> Result<String, ControlError> {
    let text = p.str("text", MAX_STRING)?;
    if text.trim().is_empty() {
        return Err(p.error("`text` must not be empty"));
    }
    if text.chars().count() > MAX_NOTE {
        return Err(p.error(format!("`text` is longer than {MAX_NOTE} characters")));
    }
    if text.split('\n').count() > MAX_NOTE_LINES {
        return Err(p.error(format!("`text` has more than {MAX_NOTE_LINES} lines")));
    }
    if text.chars().any(|c| c != '\n' && refused_in_name(c)) {
        return Err(p.error(
            "`text` must not hold control (but newline), direction or line-separator characters",
        ));
    }
    Ok(text)
}

/// `co_producer.export_folder`'s `what`.
fn export_what(p: &Params<'_>) -> Result<ExportWhat, ControlError> {
    let what = p.str("what", 32)?;
    ExportWhat::parse(&what).ok_or_else(|| {
        let names: Vec<&str> = ExportWhat::ALL.iter().map(|w| w.name()).collect();
        p.error(format!("`what` must be one of {}", names.join(", ")))
    })
}

/// `co_producer.export`'s `what` (plan 8c Ruling 1): any but `selection`,
/// which a one-call export cannot bounce.
fn chore_export_what(p: &Params<'_>) -> Result<ExportWhat, ControlError> {
    let what = p.str("what", 32)?;
    match ExportWhat::parse(&what) {
        Some(ExportWhat::Selection) => {
            Err(p.error("Bounce a selection with export_folder and file_bounce."))
        }
        Some(w) => Ok(w),
        None => {
            let names: Vec<&str> = ExportWhat::ALL
                .iter()
                .filter(|w| **w != ExportWhat::Selection)
                .map(|w| w.name())
                .collect();
            Err(p.error(format!("`what` must be one of {}", names.join(", "))))
        }
    }
}

/// A marker's `name` (plan 8c Ruling 4): 1 to [`MAX_MARKER_NAME`]
/// characters after trimming, with a request name's character rules
/// (plan 8a P11). Answers the trimmed name, or the broken rule.
pub(crate) fn marker_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("`name` must not be empty".into());
    }
    if name.chars().count() > MAX_MARKER_NAME {
        return Err(format!(
            "`name` is longer than {MAX_MARKER_NAME} characters"
        ));
    }
    if name.chars().any(refused_in_name) {
        return Err("`name` must not hold control, direction or line-separator characters".into());
    }
    Ok(name.to_string())
}

/// `co_producer.export_folder`'s optional `parent`: a string of at most
/// [`MAX_PATH`] bytes with no NUL. Whether it is a folder Co-Producer may
/// write in is the gate's check (`co_producer::check_output`), so a
/// relative or linked one is `not_allowed`, not `invalid_params`.
fn export_parent(p: &Params<'_>) -> Result<Option<PathBuf>, ControlError> {
    let Some(s) = p.opt_str("parent", MAX_PATH)? else {
        return Ok(None);
    };
    if s.len() > MAX_PATH || s.contains('\0') || s.is_empty() {
        return Err(p.error(format!(
            "`parent` must be a folder's path of 1 to {MAX_PATH} bytes"
        )));
    }
    Ok(Some(PathBuf::from(s)))
}

/// `co_producer.apply`'s `token`: [`TOKEN_LEN`] lowercase hex characters.
fn token(p: &Params<'_>) -> Result<String, ControlError> {
    let t = p.str("token", TOKEN_LEN)?;
    if t.len() != TOKEN_LEN || !t.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(p.error(format!(
            "`token` must be the {TOKEN_LEN} lowercase hex characters the preview gave"
        )));
    }
    Ok(t)
}

/// A plan's `commands`: 1 to [`MAX_PLAN`] `{id, params?}`, each checked by
/// the registry as `command.run` checks it.
fn plan(p: &Params<'_>) -> Result<Vec<Planned>, ControlError> {
    let shape = || {
        p.error(format!(
            "`commands` must be an array of 1 to {MAX_PLAN} {{\"id\", \"params\"}} objects"
        ))
    };
    let list = match p.raw("commands") {
        Some(Value::Array(a)) if (1..=MAX_PLAN).contains(&a.len()) => a,
        Some(_) => return Err(shape()),
        None => return Err(p.error("missing `commands`")),
    };
    let empty = Map::new();
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        let Value::Object(m) = item else {
            return Err(shape());
        };
        let at = |message: String| p.error(format!("`commands[{i}]`: {message}"));
        if let Some(k) = m.keys().find(|k| *k != "id" && *k != "params") {
            return Err(at(format!(
                "unknown key {:?} (allowed: id, params)",
                truncate(k, 64)
            )));
        }
        let id = match m.get("id") {
            Some(Value::String(s)) if s.chars().count() <= 64 => s.clone(),
            Some(Value::String(_)) => return Err(at("`id` is longer than 64 characters".into())),
            Some(_) => return Err(at("`id` must be a string".into())),
            None => return Err(at("missing `id`".into())),
        };
        let spec = registry::find(&id).ok_or_else(|| registry::unknown(&id))?;
        let args = match m.get("params") {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(a)) => a,
            Some(_) => return Err(at("`params` must be an object".into())),
        };
        out.push(Planned {
            id,
            args: registry::validate_args(spec, args)?,
        });
    }
    Ok(out)
}

fn wait(p: &Params<'_>) -> Result<Command, ControlError> {
    p.only(&[
        "condition",
        "tick",
        "count",
        "title",
        "kind",
        "window",
        "timeout_ms",
    ])?;
    let name = p.str("condition", 32)?;
    let (condition, extra): (WaitCondition, &[&str]) = match name.as_str() {
        "playing" => (WaitCondition::Playing, &[]),
        "stopped" => (WaitCondition::Stopped, &[]),
        "tick_at_least" => (
            WaitCondition::TickAtLeast(p.int("tick", 0, i64::MAX / 2)?),
            &["tick"],
        ),
        "documents_at_least" => (
            WaitCondition::DocumentsAtLeast(p.int("count", 0, 1000)? as u32),
            &["count"],
        ),
        "window_titled" | "window_gone" => {
            let t = p.str("title", 256)?;
            if t.is_empty() {
                return Err(p.error("`title` must not be empty"));
            }
            let c = if name == "window_titled" {
                WaitCondition::WindowTitled(t)
            } else {
                WaitCondition::WindowGone(t)
            };
            (c, &["title"])
        }
        "detail_kind" => {
            let k = p.str("kind", 32)?;
            if !DETAIL_KINDS.contains(&k.as_str()) {
                return Err(p.error(format!("`kind` must be one of {}", DETAIL_KINDS.join(", "))));
            }
            (WaitCondition::DetailKind(k), &["kind"])
        }
        _ => {
            return Err(p.error(
                "`condition` must be playing, stopped, tick_at_least, documents_at_least, window_titled, window_gone or detail_kind",
            ));
        }
    };
    for k in ["tick", "count", "title", "kind"] {
        if p.has(k) && !extra.contains(&k) {
            return Err(p.error(format!("`{k}` does not go with condition {name}")));
        }
    }
    Ok(Command::Wait {
        condition,
        window: p.window()?,
        timeout_ms: p
            .opt_int("timeout_ms", 1, MAX_TIMEOUT_MS as i64)?
            .map_or(DEFAULT_TIMEOUT_MS, |t| t as u64),
    })
}
