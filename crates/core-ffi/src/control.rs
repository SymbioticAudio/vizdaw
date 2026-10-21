//! The control channel's FFI surface (control spec §4): `core-control`'s
//! server validates and queues requests; Swift takes each [`ControlCall`]
//! on the main thread, executes it and answers with a typed
//! [`ControlResult`] or a [`ControlFailure`], which this module turns into
//! the reply's JSON. The wire schema lives here and in `core-control`;
//! Swift never builds JSON.

use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use crate::selection::SelectionRow;
use crate::snapshot::{DocSnapshot, LaneRow, TransportState};
use core_control::element::{self as el, ElementRef};
use core_control::keys::{KeyStroke, Modifiers};
use core_control::params::WindowRef;
use core_control::registry::{self, ArgValue};
use core_control::request::{Button, Command, CrashAnswer, Pointer, StateTarget};
use core_control::server::{Config, Server};
use core_control::wait::{self, DocFacts, Facts, WaitCondition};
use core_control::{ControlError, ErrorCode};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A window a call addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum WindowRefRow {
    /// `NSWindow.windowNumber`.
    Number {
        /// The number.
        number: i64,
    },
    /// The key window.
    Key,
    /// The main window.
    Main,
}

/// An element from `ui.tree`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ElementRefRow {
    /// The ID as given.
    pub id: String,
    /// Child indices from the window down.
    pub path: Vec<u32>,
    /// [`control_element_check`] of the element.
    pub check: u32,
}

/// Where a click or drag lands.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum PointerRow {
    /// An element's centre.
    Element {
        /// Which.
        element: ElementRefRow,
    },
    /// Window points, top-left origin.
    Point {
        /// From the left edge.
        x: f64,
        /// From the top edge.
        y: f64,
    },
}

/// Modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, uniffi::Record)]
pub struct ModifiersRow {
    /// ⇧
    pub shift: bool,
    /// ⌃
    pub control: bool,
    /// ⌥
    pub option: bool,
    /// ⌘
    pub command: bool,
}

/// One key press.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct KeyStrokeRow {
    /// `NSEvent.keyCode`.
    pub key_code: u16,
    /// `NSEvent.characters`.
    pub characters: String,
    /// `NSEvent.charactersIgnoringModifiers`.
    pub unmodified: String,
    /// Held modifiers. (What the stroke runs is the app's: its key
    /// bindings decide, plan 4f Ruling 5.)
    pub modifiers: ModifiersRow,
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum MouseButtonRow {
    /// Primary.
    Left,
    /// Secondary.
    Right,
}

/// What `state.get` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum StateTargetRow {
    /// Transport.
    Transport,
    /// Document.
    Document,
    /// Editors.
    Editor,
    /// Selection.
    Selection,
    /// Windows and app alerts.
    Windows,
    /// Media pool.
    Media,
    /// Project format and device rate.
    Project,
    /// Recording (plan 5c).
    Record,
    /// Audio input (plan 5c).
    Input,
}

/// The after-crash answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum CrashAnswerRow {
    /// Open Normally.
    Normal,
    /// Open with Plugins Off.
    PluginsOff,
    /// Disable the plugins that were loading.
    Disable,
}

/// Answers to the dialogs opening a project may ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, uniffi::Record)]
pub struct RecoveryAnswersRow {
    /// "Open with Plugins Off?"
    pub plugins: Option<CrashAnswerRow>,
    /// "Restore Autosave"
    pub restore_autosave: Option<bool>,
}

/// A `command.run` argument's value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ControlValue {
    /// A string or path.
    Text {
        /// The string.
        value: String,
    },
    /// A flag.
    Flag {
        /// The flag.
        value: bool,
    },
    /// An integer.
    Int {
        /// The integer.
        value: i64,
    },
}

/// A `command.run` argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ControlArg {
    /// Its name.
    pub name: String,
    /// Its value.
    pub value: ControlValue,
}

/// What a `wait` waits for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum WaitConditionRow {
    /// Playing.
    Playing,
    /// Stopped.
    Stopped,
    /// At or past a tick.
    TickAtLeast {
        /// The tick.
        tick: i64,
    },
    /// At least this many documents.
    DocumentsAtLeast {
        /// How many.
        count: u32,
    },
    /// A visible window titled like this.
    WindowTitled {
        /// Part of the title.
        title: String,
    },
    /// No visible window titled like this.
    WindowGone {
        /// Part of the title.
        title: String,
    },
    /// The detail pane shows this kind (`none`, `piano_roll`, ...).
    DetailKind {
        /// The kind.
        kind: String,
    },
}

/// The target document's state at one pump tick.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DocFactsRow {
    /// Its transport plays.
    pub playing: bool,
    /// Its transport's tick.
    pub tick: i64,
    /// Its detail pane's kind.
    pub detail_kind: String,
}

/// What one pump tick saw.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct WaitFactsRow {
    /// The target document, if any.
    pub document: Option<DocFactsRow>,
    /// Open documents.
    pub documents: u32,
    /// Visible window titles.
    pub window_titles: Vec<String>,
}

/// A validated request for the app.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum ControlRequest {
    /// `command.run`.
    RunCommand {
        /// Registry ID.
        id: String,
        /// Validated arguments.
        args: Vec<ControlArg>,
        /// Whose document.
        window: Option<WindowRefRow>,
    },
    /// `state.get`.
    State {
        /// What.
        target: StateTargetRow,
        /// Whose.
        window: Option<WindowRefRow>,
    },
    /// `ui.tree`.
    Tree {
        /// Which window.
        window: Option<WindowRefRow>,
        /// How deep.
        max_depth: u32,
    },
    /// `ui.click`.
    Click {
        /// Which window.
        window: Option<WindowRefRow>,
        /// Where.
        at: PointerRow,
        /// Which button.
        button: MouseButtonRow,
        /// 1–3.
        count: u32,
        /// Held modifiers.
        modifiers: ModifiersRow,
    },
    /// `ui.drag`.
    Drag {
        /// Which window.
        window: Option<WindowRefRow>,
        /// Press.
        from: PointerRow,
        /// Release.
        to: PointerRow,
        /// Intermediate points.
        steps: u32,
        /// Held modifiers.
        modifiers: ModifiersRow,
    },
    /// `ui.key`.
    Key {
        /// Which window.
        window: Option<WindowRefRow>,
        /// Clicked first.
        focus: Option<ElementRefRow>,
        /// In order.
        strokes: Vec<KeyStrokeRow>,
    },
    /// `ui.screenshot`.
    Screenshot {
        /// Which window.
        window: Option<WindowRefRow>,
        /// Where to write the PNG.
        path: String,
        /// Pixels per point (None: the window's).
        scale: Option<f64>,
    },
    /// `app.new_demo`.
    NewDemo {
        /// With an audio lane.
        audio: bool,
    },
    /// `app.open`.
    Open {
        /// The package.
        path: String,
        /// Dialog answers.
        answers: RecoveryAnswersRow,
    },
    /// `app.quit`.
    Quit,
    /// `wait`.
    Wait {
        /// What.
        condition: WaitConditionRow,
        /// Whose.
        window: Option<WindowRefRow>,
        /// At most this long.
        timeout_ms: u64,
    },
}

/// A request and the token to answer it with.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ControlCall {
    /// Answer with this.
    pub token: u64,
    /// The request.
    pub request: ControlRequest,
}

/// An alert the app would have shown.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct AlertRow {
    /// Its title.
    pub title: String,
    /// Its message.
    pub message: String,
}

/// What a detail pane or torn-off editor shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DetailRow {
    /// `piano_roll`, `list`, `strip_chart` or `drum_grid`.
    pub kind: String,
    /// The target's sequence.
    pub seq: u64,
    /// Its lane (lane and block targets).
    pub lane: Option<u64>,
    /// A block target's start tick.
    pub start: Option<i64>,
    /// A block target's end tick.
    pub end: Option<i64>,
}

/// A torn-off editor window.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct TornRow {
    /// Its layout ID.
    pub id: String,
    /// What it shows.
    pub detail: DetailRow,
}

/// A document's editors.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct EditorStateRow {
    /// The detail pane (None: empty).
    pub detail: Option<DetailRow>,
    /// Torn-off editor windows.
    pub windows: Vec<TornRow>,
    /// `arrow`, `pencil` or `eraser`.
    pub tool: String,
    /// Snap on.
    pub snap_enabled: bool,
    /// Snap division, snake_case: `bar`, `half`, `quarter`, `eighth`,
    /// `sixteenth`, `thirty_second` (plan 4f Ruling 1).
    pub snap_division: String,
    /// Exact mode on.
    pub exact_mode: bool,
    /// A drag is open.
    pub gesture: bool,
    /// The editors' status line.
    pub status: Option<String>,
    /// Step entry while it is on (plan 4f).
    pub step: Option<StepEntryStateRow>,
    /// The transport's marks (plan 4f Ruling 7).
    pub marks: TransportMarksRow,
}

/// Step entry as `state.get editor` shows it (plan 4f).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct StepEntryStateRow {
    /// The sequence steps go into.
    pub seq: u64,
    /// The lane.
    pub lane: u64,
    /// Where the next step goes.
    pub tick: i64,
    /// The note value, snake_case (`quarter`, `thirty_second`).
    pub value: String,
    /// Dotted.
    pub dotted: bool,
    /// Triplet.
    pub triplet: bool,
    /// Steps Delete Last Step can take back.
    pub steps: u32,
}

/// The transport's locators, punch range and record modes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct TransportMarksRow {
    /// Locators 0–9 (None: not set).
    pub locators: Vec<Option<i64>>,
    /// Punch in.
    pub punch_in: i64,
    /// Punch out.
    pub punch_out: i64,
    /// Punch on.
    pub punch: bool,
    /// Overdub on.
    pub overdub: bool,
}

/// Kinds of VizDAW window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum WindowKindRow {
    /// A document window.
    Document,
    /// A mixer.
    Mixer,
    /// A torn-off editor.
    Editor,
    /// A plugin window.
    Plugin,
    /// A select-and-modify panel.
    ModifyPanel,
    /// The Plugin Manager.
    PluginManager,
    /// Settings.
    Settings,
    /// Anything else (menus' windows are not listed).
    Other,
    /// A media pool window (plan 5b).
    MediaPool,
}

/// A VizDAW window.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct WindowRow {
    /// `NSWindow.windowNumber`.
    pub number: i64,
    /// Its title.
    pub title: String,
    /// Its kind.
    pub kind: WindowKindRow,
    /// Its document's ID, if it has one.
    pub doc: Option<String>,
    /// Key window.
    pub key: bool,
    /// Main window.
    pub main: bool,
    /// On screen.
    pub visible: bool,
    /// Frame in screen points (AppKit, bottom-left origin).
    pub x: f64,
    /// Frame origin y.
    pub y: f64,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
}

/// One accessibility element, in pre-order; `path` places it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UiNodeRow {
    /// Child indices from the window down.
    pub path: Vec<u32>,
    /// `AXButton`, `AXGroup`, ...
    pub role: String,
    /// Subrole, if any.
    pub subrole: Option<String>,
    /// Accessibility label.
    pub label: Option<String>,
    /// Title.
    pub title: Option<String>,
    /// Value as text.
    pub value: Option<String>,
    /// Frame in window points, top-left origin.
    pub x: f64,
    /// Frame y.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
    /// Enabled.
    pub enabled: bool,
    /// Selected.
    pub selected: bool,
    /// Has keyboard focus.
    pub focused: bool,
    /// Actions it supports (`press`, ...).
    pub actions: Vec<String>,
}

/// What a call came to.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum ControlResult {
    /// `state.get media`.
    Media {
        /// Document ID.
        doc: String,
        /// The pool.
        items: Vec<crate::media::MediaRow>,
    },
    /// `state.get project`.
    Project {
        /// Document ID.
        doc: String,
        /// Format, mismatched media, device rate.
        info: crate::format::ProjectInfoRow,
    },
    /// `state.get record` (plan 5c).
    Record {
        /// Document ID.
        doc: String,
        /// What recording is doing.
        state: crate::record::RecordStateRow,
    },
    /// `state.get input` (plan 5c).
    InputState {
        /// Document ID.
        doc: String,
        /// What the input offers.
        info: crate::live::InputInfoRow,
    },
    /// `command.run`.
    Ran {
        /// Registry ID.
        command: String,
        /// The document it acted on or made.
        doc: Option<String>,
        /// The window it opened.
        window: Option<i64>,
        /// The undo label afterwards.
        undo_label: Option<String>,
        /// The editors' status line afterwards.
        status: Option<String>,
    },
    /// `state.get transport`.
    Transport {
        /// Document ID.
        doc: String,
        /// Its transport.
        state: TransportState,
    },
    /// `state.get document`.
    Document {
        /// Document ID.
        doc: String,
        /// Its package, once saved.
        path: Option<String>,
        /// Its snapshot.
        snapshot: DocSnapshot,
        /// The sidebar's sequence.
        selected_seq: Option<u64>,
        /// That sequence's lanes.
        lanes: Vec<LaneRow>,
        /// The alert its window would show.
        alert: Option<AlertRow>,
    },
    /// `state.get editor`.
    Editor {
        /// Document ID.
        doc: String,
        /// Its editors.
        editor: EditorStateRow,
    },
    /// `state.get selection`.
    Selection {
        /// Document ID.
        doc: String,
        /// Its selection.
        selection: SelectionRow,
    },
    /// `state.get windows`.
    Windows {
        /// Every VizDAW window, front to back.
        windows: Vec<WindowRow>,
        /// App-level alerts not shown (the channel suppresses them).
        alerts: Vec<AlertRow>,
    },
    /// `ui.tree`.
    Tree {
        /// The window.
        window: i64,
        /// Pre-order nodes.
        nodes: Vec<UiNodeRow>,
        /// The node cap cut the tree.
        truncated: bool,
    },
    /// `ui.click`, `ui.drag`, `ui.key`.
    Input {
        /// The window.
        window: i64,
        /// Events sent.
        events: u32,
        /// Where the last mouse event went (0 for keys).
        x: f64,
        /// Its y.
        y: f64,
    },
    /// `ui.screenshot`.
    Screenshot {
        /// The window.
        window: i64,
        /// The PNG.
        path: String,
        /// Pixels.
        width: u32,
        /// Pixels.
        height: u32,
    },
    /// `app.new_demo`, `app.open`.
    Opened {
        /// The new document.
        doc: String,
        /// Its window.
        window: i64,
    },
    /// `app.quit`.
    Quitting,
    /// `wait`.
    Waited {
        /// The condition's name.
        condition: String,
        /// How long it took.
        elapsed_ms: u64,
    },
}

/// Error codes (control spec §5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ControlErrorCode {
    /// `parse_error`
    ParseError,
    /// `invalid_request`
    InvalidRequest,
    /// `unknown_command`
    UnknownCommand,
    /// `invalid_params`
    InvalidParams,
    /// `unknown_id`
    UnknownId,
    /// `no_document`
    NoDocument,
    /// `no_window`
    NoWindow,
    /// `no_element`
    NoElement,
    /// `dialog_required`
    DialogRequired,
    /// `busy`
    Busy,
    /// `failed`
    Failed,
    /// `unavailable`
    Unavailable,
    /// `timeout`
    Timeout,
    /// `shutting_down`
    ShuttingDown,
    /// `internal` (`Internal` is a Swift keyword when lower-cased).
    InternalFailure,
}

/// A failed call.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ControlFailure {
    /// The code.
    pub code: ControlErrorCode,
    /// One sentence.
    pub message: String,
    /// The dialog it would have opened.
    pub dialog: Option<String>,
}

fn code_of(c: ControlErrorCode) -> ErrorCode {
    match c {
        ControlErrorCode::ParseError => ErrorCode::ParseError,
        ControlErrorCode::InvalidRequest => ErrorCode::InvalidRequest,
        ControlErrorCode::UnknownCommand => ErrorCode::UnknownCommand,
        ControlErrorCode::InvalidParams => ErrorCode::InvalidParams,
        ControlErrorCode::UnknownId => ErrorCode::UnknownId,
        ControlErrorCode::NoDocument => ErrorCode::NoDocument,
        ControlErrorCode::NoWindow => ErrorCode::NoWindow,
        ControlErrorCode::NoElement => ErrorCode::NoElement,
        ControlErrorCode::DialogRequired => ErrorCode::DialogRequired,
        ControlErrorCode::Busy => ErrorCode::Busy,
        ControlErrorCode::Failed => ErrorCode::Failed,
        ControlErrorCode::Unavailable => ErrorCode::Unavailable,
        ControlErrorCode::Timeout => ErrorCode::Timeout,
        ControlErrorCode::ShuttingDown => ErrorCode::ShuttingDown,
        ControlErrorCode::InternalFailure => ErrorCode::Internal,
    }
}

fn window_row(w: Option<WindowRef>) -> Option<WindowRefRow> {
    w.map(|w| match w {
        WindowRef::Number(number) => WindowRefRow::Number { number },
        WindowRef::Key => WindowRefRow::Key,
        WindowRef::Main => WindowRefRow::Main,
    })
}

fn element_row(e: ElementRef) -> ElementRefRow {
    ElementRefRow {
        id: e.id,
        path: e.path,
        check: e.check,
    }
}

fn pointer_row(p: Pointer) -> PointerRow {
    match p {
        Pointer::Element(e) => PointerRow::Element {
            element: element_row(e),
        },
        Pointer::Point { x, y } => PointerRow::Point { x, y },
    }
}

fn modifiers_row(m: Modifiers) -> ModifiersRow {
    ModifiersRow {
        shift: m.shift,
        control: m.control,
        option: m.option,
        command: m.command,
    }
}

fn stroke_row(k: KeyStroke) -> KeyStrokeRow {
    KeyStrokeRow {
        key_code: k.key_code,
        characters: k.characters,
        unmodified: k.unmodified,
        modifiers: modifiers_row(k.modifiers),
    }
}

/// The FFI form of a wait condition.
pub fn condition_row(c: WaitCondition) -> WaitConditionRow {
    match c {
        WaitCondition::Playing => WaitConditionRow::Playing,
        WaitCondition::Stopped => WaitConditionRow::Stopped,
        WaitCondition::TickAtLeast(tick) => WaitConditionRow::TickAtLeast { tick },
        WaitCondition::DocumentsAtLeast(count) => WaitConditionRow::DocumentsAtLeast { count },
        WaitCondition::WindowTitled(title) => WaitConditionRow::WindowTitled { title },
        WaitCondition::WindowGone(title) => WaitConditionRow::WindowGone { title },
        WaitCondition::DetailKind(kind) => WaitConditionRow::DetailKind { kind },
    }
}

fn condition_of(c: WaitConditionRow) -> WaitCondition {
    match c {
        WaitConditionRow::Playing => WaitCondition::Playing,
        WaitConditionRow::Stopped => WaitCondition::Stopped,
        WaitConditionRow::TickAtLeast { tick } => WaitCondition::TickAtLeast(tick),
        WaitConditionRow::DocumentsAtLeast { count } => WaitCondition::DocumentsAtLeast(count),
        WaitConditionRow::WindowTitled { title } => WaitCondition::WindowTitled(title),
        WaitConditionRow::WindowGone { title } => WaitCondition::WindowGone(title),
        WaitConditionRow::DetailKind { kind } => WaitCondition::DetailKind(kind),
    }
}

fn facts_of(f: WaitFactsRow) -> Facts {
    Facts {
        document: f.document.map(|d| DocFacts {
            playing: d.playing,
            tick: d.tick,
            detail_kind: d.detail_kind,
        }),
        documents: f.documents,
        window_titles: f.window_titles,
    }
}

/// The request for the app, or None for a command the server answers.
pub fn request_of(c: Command, screenshot: impl FnOnce() -> PathBuf) -> Option<ControlRequest> {
    Some(match c {
        Command::CommandsList | Command::Ping => return None,
        Command::CommandRun { id, args, window } => ControlRequest::RunCommand {
            id,
            args: args
                .into_iter()
                .map(|(name, v)| ControlArg {
                    name,
                    value: match v {
                        ArgValue::Text(value) => ControlValue::Text { value },
                        ArgValue::Flag(value) => ControlValue::Flag { value },
                        ArgValue::Int(value) => ControlValue::Int { value },
                    },
                })
                .collect(),
            window: window_row(window),
        },
        Command::StateGet { target, window } => ControlRequest::State {
            target: match target {
                StateTarget::Transport => StateTargetRow::Transport,
                StateTarget::Document => StateTargetRow::Document,
                StateTarget::Editor => StateTargetRow::Editor,
                StateTarget::Selection => StateTargetRow::Selection,
                StateTarget::Windows => StateTargetRow::Windows,
                StateTarget::Media => StateTargetRow::Media,
                StateTarget::Project => StateTargetRow::Project,
                StateTarget::Record => StateTargetRow::Record,
                StateTarget::Input => StateTargetRow::Input,
            },
            window: window_row(window),
        },
        Command::UiTree { window, max_depth } => ControlRequest::Tree {
            window: window_row(window),
            max_depth,
        },
        Command::UiClick {
            window,
            at,
            button,
            count,
            modifiers,
        } => ControlRequest::Click {
            window: window_row(window),
            at: pointer_row(at),
            button: match button {
                Button::Left => MouseButtonRow::Left,
                Button::Right => MouseButtonRow::Right,
            },
            count,
            modifiers: modifiers_row(modifiers),
        },
        Command::UiDrag {
            window,
            from,
            to,
            steps,
            modifiers,
        } => ControlRequest::Drag {
            window: window_row(window),
            from: pointer_row(from),
            to: pointer_row(to),
            steps,
            modifiers: modifiers_row(modifiers),
        },
        Command::UiKey {
            window,
            focus,
            strokes,
        } => ControlRequest::Key {
            window: window_row(window),
            focus: focus.map(element_row),
            strokes: strokes.into_iter().map(stroke_row).collect(),
        },
        Command::UiScreenshot {
            window,
            path,
            scale,
        } => ControlRequest::Screenshot {
            window: window_row(window),
            path: path
                .unwrap_or_else(screenshot)
                .to_string_lossy()
                .into_owned(),
            scale,
        },
        Command::AppNewDemo { audio } => ControlRequest::NewDemo { audio },
        Command::AppOpen { path, answers } => ControlRequest::Open {
            path: path.to_string_lossy().into_owned(),
            answers: RecoveryAnswersRow {
                plugins: answers.plugins.map(|a| match a {
                    CrashAnswer::Normal => CrashAnswerRow::Normal,
                    CrashAnswer::Off => CrashAnswerRow::PluginsOff,
                    CrashAnswer::Disable => CrashAnswerRow::Disable,
                }),
                restore_autosave: answers.restore_autosave,
            },
        },
        Command::AppQuit => ControlRequest::Quit,
        Command::Wait {
            condition,
            window,
            timeout_ms,
        } => ControlRequest::Wait {
            condition: condition_row(condition),
            window: window_row(window),
            timeout_ms,
        },
    })
}

/// The name an element ID and its check are made of: the label, or the
/// title when the label is absent or empty (control spec §5.4).
fn id_label<'a>(label: Option<&'a str>, title: Option<&'a str>) -> &'a str {
    label.filter(|l| !l.is_empty()).or(title).unwrap_or("")
}

fn frame(x: f64, y: f64, width: f64, height: f64) -> Value {
    json!({ "x": x, "y": y, "width": width, "height": height })
}

fn alert_json(a: &AlertRow) -> Value {
    json!({ "title": a.title, "message": a.message })
}

fn detail_json(d: &DetailRow) -> Value {
    json!({ "kind": d.kind, "seq": d.seq, "lane": d.lane, "start": d.start, "end": d.end })
}

fn node_json(n: &UiNodeRow) -> Value {
    let mut o = Map::new();
    let name = id_label(n.label.as_deref(), n.title.as_deref());
    o.insert("id".into(), json!(el::element_id(&n.path, &n.role, name)));
    o.insert("role".into(), json!(n.role));
    for (k, v) in [
        ("subrole", &n.subrole),
        ("label", &n.label),
        ("title", &n.title),
        ("value", &n.value),
    ] {
        if let Some(v) = v.as_ref().filter(|v| !v.is_empty()) {
            o.insert(k.into(), json!(v));
        }
    }
    o.insert("frame".into(), frame(n.x, n.y, n.width, n.height));
    o.insert("enabled".into(), json!(n.enabled));
    if n.selected {
        o.insert("selected".into(), json!(true));
    }
    if n.focused {
        o.insert("focused".into(), json!(true));
    }
    if !n.actions.is_empty() {
        o.insert("actions".into(), json!(n.actions));
    }
    Value::Object(o)
}

/// Nests pre-order nodes by their paths (a node whose parent is missing
/// stays at the top).
fn tree_json(nodes: &[UiNodeRow]) -> Vec<Value> {
    let mut built: BTreeMap<Vec<u32>, Value> = nodes
        .iter()
        .map(|n| (n.path.clone(), node_json(n)))
        .collect();
    let mut paths: Vec<Vec<u32>> = built.keys().cloned().collect();
    // Deepest first, siblings in index order (the sort is stable).
    paths.sort_by_key(|p| std::cmp::Reverse(p.len()));
    let mut roots = Vec::new();
    for p in paths {
        let Some(v) = built.remove(&p) else { continue };
        let parent = p.split_last().map(|(_, rest)| rest).unwrap_or(&[]);
        match built.get_mut(parent).and_then(Value::as_object_mut) {
            Some(o) if !parent.is_empty() => {
                if let Some(a) = o
                    .entry("children")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                {
                    a.push(v);
                }
            }
            _ => roots.push(v),
        }
    }
    // Children were added deepest-first; sibling order holds per parent,
    // roots come out in index order.
    roots
}

/// A lane's input as the channel spells it (`channel`s from 1).
fn lane_input_json(i: &crate::edit::LaneInputRow) -> Value {
    match i {
        crate::edit::LaneInputRow::Audio { first, stereo } => {
            json!({"kind": "audio", "channel": u32::from(*first) + 1, "stereo": stereo})
        }
        crate::edit::LaneInputRow::Midi { port, channel } => json!({
            "kind": "midi", "port": port, "midi_channel": channel.map(|c| u32::from(c) + 1),
        }),
    }
}

fn monitor_name(m: crate::edit::MonitorModeRow) -> &'static str {
    match m {
        crate::edit::MonitorModeRow::Auto => "auto",
        crate::edit::MonitorModeRow::Input => "input",
        crate::edit::MonitorModeRow::Off => "off",
    }
}

fn transport_json(doc: &str, t: &TransportState) -> Value {
    json!({
        "doc": doc, "playing": t.playing, "seq": t.seq, "tick": t.tick,
        "position": t.position_bbu, "seconds": t.seconds, "sample_rate": t.sample_rate,
        "device": t.device, "faults": t.faults, "report": t.report,
        "rate_mismatch": t.rate_mismatch,
    })
}

/// The reply's `result` for `r` (control spec §5.5).
pub fn result_json(r: &ControlResult) -> Value {
    match r {
        ControlResult::Ran {
            command,
            doc,
            window,
            undo_label,
            status,
        } => json!({
            "command": command, "doc": doc, "window": window, "undo": undo_label, "status": status,
        }),
        ControlResult::Transport { doc, state } => transport_json(doc, state),
        ControlResult::Media { doc, items } => json!({
            "doc": doc,
            "items": items.iter().map(|m| json!({
                "id": m.id, "name": m.name, "path": m.path, "channels": m.channels,
                "frames": m.frames, "rate": m.sample_rate,
                "origin": match m.origin {
                    crate::media::MediaOriginRow::Imported => "imported",
                    crate::media::MediaOriginRow::Recorded => "recorded",
                    crate::media::MediaOriginRow::Bounced => "bounced",
                    crate::media::MediaOriginRow::ExternalEdit => "external_edit",
                },
                "original_name": m.original_name,
                "offline": m.offline.is_some(),
                "offline_reason": m.offline.map(|r| match r {
                    crate::media::OfflineReasonRow::Missing => "missing",
                    crate::media::OfflineReasonRow::Unreadable => "unreadable",
                    crate::media::OfflineReasonRow::Format => "format",
                    crate::media::OfflineReasonRow::Length => "length",
                    crate::media::OfflineReasonRow::Changed => "changed",
                }),
                "uses": m.uses,
                "used_percent": m.used_percent,
            })).collect::<Vec<_>>(),
        }),
        ControlResult::Record { doc, state: r } => json!({
            "doc": doc, "recording": r.recording, "seq": r.seq,
            "spec": r.spec.as_ref().map(|s| json!({
                "replace": s.replace,
                "punch": s.punch.as_ref().map(|p| json!({"start": p.start, "end": p.end})),
                "loop": s.looped, "count_in": s.count_in_bars, "pre_roll": s.pre_roll_bars,
                "passes_as_takes": s.passes_as_takes,
            })),
            "pass": r.pass, "kept": r.kept, "discarded": r.discarded,
            "lanes": r.lanes.iter().map(|l| json!({
                "lane": l.lane, "audio": l.audio, "path": l.path, "clipped": l.clipped,
            })).collect::<Vec<_>>(),
            "frames": r.frames, "overflows": r.overflows, "gap_frames": r.gap_frames,
            "underruns": r.underruns, "late_starts": r.late_starts,
            "free_bytes": r.free_bytes, "recovered": r.recovered,
            "recovered_files": r.recovered_files.iter().map(|f| json!({
                "path": f.path, "frames": f.frames, "size_known": f.size_known,
                "readable": f.readable,
            })).collect::<Vec<_>>(),
        }),
        ControlResult::InputState { doc, info } => json!({
            "doc": doc, "device": info.device, "channels": info.channels,
            "names": info.names, "input_latency": info.input_latency_frames,
            "output_latency": info.output_latency_frames,
        }),
        ControlResult::Project { doc, info } => json!({
            "doc": doc,
            "rate": info.format.sample_rate,
            "depth": match info.format.depth {
                crate::format::BitDepthRow::Int16 => "16",
                crate::format::BitDepthRow::Int24 => "24",
                crate::format::BitDepthRow::Float32 => "32f",
            },
            "label": crate::format::format_label(info.format),
            "mismatched": info.mismatched,
            "rate_mismatch": info.rate_mismatch,
            "device_rate": info.device_rate,
        }),
        ControlResult::Document {
            doc,
            path,
            snapshot: s,
            selected_seq,
            lanes,
            alert,
        } => json!({
            "doc": doc, "title": s.title, "path": path, "dirty": s.dirty, "version": s.version,
            "undo": s.undo_label, "redo": s.redo_label, "selected_seq": selected_seq,
            "sequences": s.sequences.iter().map(|q| json!({
                "id": q.id, "name": q.name, "top_level": q.top_level, "uses": q.use_count,
                "length": q.length_ticks, "lanes": q.lane_count,
            })).collect::<Vec<_>>(),
            "lanes": lanes.iter().map(|l| json!({
                "id": l.id, "name": l.name, "destination": l.destination,
                "events": l.midi_event_count, "regions": l.region_count,
                "references": l.reference_count, "mute": l.mute, "solo": l.solo, "arm": l.arm,
                "takes": l.take_count, "active_take": l.active_take,
                "input": l.input.as_ref().map(lane_input_json),
                "monitor": monitor_name(l.monitor),
            })).collect::<Vec<_>>(),
            "warnings": s.warnings,
            "alert": alert.as_ref().map(alert_json),
        }),
        ControlResult::Editor { doc, editor: e } => json!({
            "doc": doc,
            "detail": e.detail.as_ref().map(detail_json),
            "windows": e.windows.iter().map(|w| json!({
                "id": w.id, "detail": detail_json(&w.detail),
            })).collect::<Vec<_>>(),
            "tool": e.tool,
            "snap": { "enabled": e.snap_enabled, "division": e.snap_division },
            "exact_mode": e.exact_mode, "gesture": e.gesture, "status": e.status,
            "step": e.step.as_ref().map(|s| json!({
                "seq": s.seq, "lane": s.lane, "tick": s.tick, "value": s.value,
                "dotted": s.dotted, "triplet": s.triplet, "steps": s.steps,
            })),
            "transport": {
                "locators": e.marks.locators, "punch_in": e.marks.punch_in,
                "punch_out": e.marks.punch_out, "punch": e.marks.punch, "overdub": e.marks.overdub,
            },
        }),
        ControlResult::Selection { doc, selection: s } => json!({
            "doc": doc,
            "time_range": s.time_range.map(|t| json!({ "start": t.start, "end": t.end })),
            "lanes": s.lanes.iter().map(|l| json!({ "seq": l.seq, "lane": l.lane })).collect::<Vec<_>>(),
            "events": s.events,
        }),
        ControlResult::Windows { windows, alerts } => json!({
            "windows": windows.iter().map(|w| json!({
                "number": w.number, "title": w.title, "kind": window_kind(w.kind), "doc": w.doc,
                "key": w.key, "main": w.main, "visible": w.visible,
                "frame": frame(w.x, w.y, w.width, w.height),
            })).collect::<Vec<_>>(),
            "alerts": alerts.iter().map(alert_json).collect::<Vec<_>>(),
        }),
        ControlResult::Tree {
            window,
            nodes,
            truncated,
        } => json!({ "window": window, "truncated": truncated, "nodes": tree_json(nodes) }),
        ControlResult::Input {
            window,
            events,
            x,
            y,
        } => {
            json!({ "window": window, "events": events, "x": x, "y": y })
        }
        ControlResult::Screenshot {
            window,
            path,
            width,
            height,
        } => json!({ "window": window, "path": path, "width": width, "height": height }),
        ControlResult::Opened { doc, window } => json!({ "doc": doc, "window": window }),
        ControlResult::Quitting => json!({ "quitting": true }),
        ControlResult::Waited {
            condition,
            elapsed_ms,
        } => json!({ "condition": condition, "elapsed_ms": elapsed_ms }),
    }
}

fn window_kind(k: WindowKindRow) -> &'static str {
    match k {
        WindowKindRow::Document => "document",
        WindowKindRow::Mixer => "mixer",
        WindowKindRow::Editor => "editor",
        WindowKindRow::Plugin => "plugin",
        WindowKindRow::ModifyPanel => "modify_panel",
        WindowKindRow::PluginManager => "plugin_manager",
        WindowKindRow::Settings => "settings",
        WindowKindRow::Other => "other",
        WindowKindRow::MediaPool => "media_pool",
    }
}

/// The control socket the app listens on (`--control` / `VIZDAW_CONTROL`).
#[derive(uniffi::Object)]
pub struct ControlServer {
    server: Mutex<Server>,
    path: String,
}

#[uniffi::export]
impl ControlServer {
    /// Listens on `path` (control spec §8); a call the app does not answer
    /// within `app_timeout_ms` is answered `timeout`.
    #[uniffi::constructor]
    pub fn start(path: String, app_timeout_ms: u64) -> Result<Arc<ControlServer>, VizError> {
        ffi_guard(|| {
            let config = Config {
                app_timeout: Duration::from_millis(app_timeout_ms.max(1)),
                ..Config::default()
            };
            let server =
                Server::start(std::path::Path::new(&path), config).map_err(|e| VizError::Io {
                    message: e.to_string(),
                })?;
            Ok(Arc::new(ControlServer {
                server: Mutex::new(server),
                path,
            }))
        })
    }

    /// The socket's path.
    pub fn path(&self) -> String {
        self.path.clone()
    }

    /// The oldest call for the app, if any.
    pub fn next_call(&self) -> Result<Option<ControlCall>, VizError> {
        ffi_guard(|| {
            let server = lock(&self.server);
            while let Some(call) = server.next_call() {
                let token = call.token;
                if let Some(request) = request_of(call.command, || server.screenshot_path()) {
                    return Ok(Some(ControlCall { token, request }));
                }
            }
            Ok(None)
        })
    }

    /// Answers `token` with `result`; false when nobody waits any more.
    pub fn reply(&self, token: u64, result: ControlResult) -> Result<bool, VizError> {
        ffi_guard(|| Ok(lock(&self.server).reply(token, Ok(result_json(&result)))))
    }

    /// Answers `token` with `failure`.
    pub fn fail(&self, token: u64, failure: ControlFailure) -> Result<bool, VizError> {
        ffi_guard(|| {
            let mut e = ControlError::new(code_of(failure.code), failure.message);
            e.dialog = failure.dialog;
            Ok(lock(&self.server).reply(token, Err(e)))
        })
    }

    /// Answers wait `token` with `timeout`, the condition and the last facts.
    pub fn wait_timed_out(
        &self,
        token: u64,
        condition: WaitConditionRow,
        facts: WaitFactsRow,
        timeout_ms: u64,
    ) -> Result<bool, VizError> {
        ffi_guard(|| {
            let c = condition_of(condition);
            let details = wait::timeout_details(&c, timeout_ms, &facts_of(facts));
            let e = ControlError::new(
                ErrorCode::Timeout,
                format!("{} did not happen within {timeout_ms} ms", c.describe()),
            )
            .with_details(details);
            Ok(lock(&self.server).reply(token, Err(e)))
        })
    }

    /// Gives `token` `timeout_ms` from now to be answered instead of the
    /// app timeout (a job that runs long); false when nobody waits.
    pub fn extend_timeout(&self, token: u64, timeout_ms: u64) -> Result<bool, VizError> {
        ffi_guard(|| Ok(lock(&self.server).extend(token, Duration::from_millis(timeout_ms))))
    }

    /// Whether a client still waits for `token`.
    pub fn is_waiting(&self, token: u64) -> bool {
        lock(&self.server).is_waiting(token)
    }

    /// Stops listening (pending calls get `shutting_down`). Idempotent.
    pub fn stop(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.server).stop();
            Ok(())
        })
    }
}

/// Whether `condition` holds for `facts`.
#[uniffi::export]
pub fn control_wait_met(condition: WaitConditionRow, facts: WaitFactsRow) -> bool {
    wait::met(&condition_of(condition), &facts_of(facts))
}

/// The check an element ID carries: of its role and its label (its title
/// when it has no label).
#[uniffi::export]
pub fn control_element_check(role: String, label: Option<String>, title: Option<String>) -> u32 {
    el::check(&role, id_label(label.as_deref(), title.as_deref()))
}

/// Every registered menu command ID (the app checks it handles each).
#[uniffi::export]
pub fn control_command_ids() -> Vec<String> {
    registry::COMMANDS
        .iter()
        .map(|c| c.id.to_string())
        .collect()
}

/// Whether a plugin call is on this (the main) thread's stack: a control
/// call now must answer `busy` (control spec §7).
#[uniffi::export]
pub fn control_host_busy() -> bool {
    crate::plugins::is_busy()
}
