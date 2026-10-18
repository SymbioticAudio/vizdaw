//! Validated commands (spec §5.5): every parameter of every command is
//! checked here, in Rust, before anything reaches the app. Validation is
//! total: any [`RawRequest`] gives a [`Command`] or an error, never a panic.

use crate::element::{self, ElementRef};
use crate::error::{ControlError, ErrorCode};
use crate::keys::{KeyStroke, Modifiers, named_key, text_strokes};
use crate::params::{MAX_STRING, Params, WindowRef};
use crate::protocol::{RawRequest, truncate};
use crate::registry::{self, ArgValue, OPEN_DIALOGS};
use crate::wait::{DEFAULT_TIMEOUT_MS, DETAIL_KINDS, MAX_TIMEOUT_MS, WaitCondition};
use serde_json::{Map, Value};
use std::path::PathBuf;

/// Every command name.
pub const COMMAND_NAMES: [&str; 13] = [
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
];

/// The deepest `ui.tree` (and the default).
pub const MAX_TREE_DEPTH: u32 = 64;
/// The most intermediate points of a drag.
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
}

impl Command {
    /// Whether the server answers it without the app.
    pub fn is_local(&self) -> bool {
        matches!(self, Command::CommandsList | Command::Ping)
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
                _ => {
                    return Err(p.error(
                        "`target` must be transport, document, editor, selection or windows",
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
            p.only(&["window", "path", "scale"])?;
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
