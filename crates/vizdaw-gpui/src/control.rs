//! The control channel (spec `docs/superpowers/specs/2026-10-18-control-
//! channel-design.md`) in the GPUI app: `core_control::Server` on the
//! `--control` socket, pumped on a GPUI timer from the main thread without
//! blocking, at most [`MAX_CALLS_PER_TICK`] calls a tick so a burst cannot
//! stall drawing.
//!
//! Slice 1 answers `app.new_demo`, `app.open`, `app.quit`, `command.run`
//! for `file.save`, `file.save_as`, `transport.play_stop` and
//! `transport.stop` (stop and return), `state.get` `transport` and
//! `windows`, `ui.tree` and `ui.click {element}`. Everything else answers
//! `unavailable`.
//!
//! Calls are handled in the pump's own top-level update, never inside
//! `AppState::update` or a window's update, so a call may open windows and
//! update window handles directly (GPUI panics on, or refuses, a nested
//! one). Every call is answered in the tick it is taken; `app.quit` is
//! answered first and the app quits once that update has ended
//! (`cx.defer`); the server is stopped while quitting so the reply is
//! written before the process goes.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use core_control::params::WindowRef;
use core_control::registry::ArgValue;
use core_control::request::{Command, Pointer, StateTarget};
use core_control::server::Call;
use core_control::{Config, ControlError, ErrorCode, Server, StartError};
use gpui::{
    AnyWindowHandle, App, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput,
    WindowHandle,
};
use serde_json::{Value, json};
use vizdaw_app::{DocId, SaveAction};

use crate::app_state::{AppState, WindowEntry};
use crate::menus::{PlayStop, StopAndReturn};
use crate::shell::DocumentWindow;

/// The most calls one pump tick handles.
pub const MAX_CALLS_PER_TICK: usize = 16;

/// How often the pump runs: about once a frame.
pub const PUMP_INTERVAL: Duration = Duration::from_millis(16);

/// Where calls come from and replies go: the server, or a test's queue.
pub trait CallSource {
    /// The oldest call not answered yet.
    fn next_call(&self) -> Option<Call>;
    /// Answers call `token`; false when nobody waits for it any more.
    fn reply(&self, token: u64, outcome: Result<Value, ControlError>) -> bool;
}

impl CallSource for Server {
    fn next_call(&self) -> Option<Call> {
        Server::next_call(self)
    }

    fn reply(&self, token: u64, outcome: Result<Value, ControlError>) -> bool {
        Server::reply(self, token, outcome)
    }
}

/// What [`handle`] made of a call.
#[derive(Debug, Clone, PartialEq)]
pub enum Handled {
    /// Answer this.
    Reply(Result<Value, ControlError>),
    /// Answer `{quitting: true}`, then quit without asking.
    Quit,
}

/// What one pump tick did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    /// Calls taken and answered.
    pub handled: usize,
    /// An `app.quit` was answered; the app quits once this update ends.
    pub quit: bool,
}

/// Takes and answers up to [`MAX_CALLS_PER_TICK`] calls. Call it from a
/// top-level update (the pump timer's), not from inside an entity's or a
/// window's update.
pub fn pump(calls: &impl CallSource, cx: &mut App) -> Tick {
    let mut tick = Tick::default();
    while tick.handled < MAX_CALLS_PER_TICK && !tick.quit {
        let Some(call) = calls.next_call() else { break };
        tick.handled += 1;
        match handle(&call.command, cx) {
            Handled::Reply(r) => {
                calls.reply(call.token, r);
            }
            Handled::Quit => {
                calls.reply(call.token, Ok(json!({ "quitting": true })));
                tick.quit = true;
                // After this update: the reply is on its way first.
                cx.defer(crate::quit_now);
            }
        }
    }
    tick
}

/// Answers one call.
pub fn handle(command: &Command, cx: &mut App) -> Handled {
    match command {
        Command::AppQuit { discard } => quit(*discard, cx),
        other => Handled::Reply(answer(other, cx)),
    }
}

fn answer(command: &Command, cx: &mut App) -> Result<Value, ControlError> {
    match command {
        Command::AppNewDemo { audio: true } => Err(unavailable(
            "app.new_demo with `audio`: the audio demo is not in slice 1",
        )),
        Command::AppNewDemo { audio: false } => {
            let before = alert_id(cx);
            match crate::new_demo(cx) {
                Some(h) => opened(h, cx),
                None => Err(failure(before, "no demo was made", cx)),
            }
        }
        Command::AppOpen { path, .. } => {
            let before = alert_id(cx);
            match crate::open_path(path, cx) {
                Some(h) => opened(h, cx),
                None => Err(failure(before, "the project did not open", cx)),
            }
        }
        Command::CommandRun { id, args, window } => {
            let entry = target(window.as_ref(), cx)?;
            run_command(id, args, &entry, cx)
        }
        Command::StateGet {
            target: StateTarget::Transport,
            window,
        } => {
            let entry = target(window.as_ref(), cx)?;
            transport_state(&entry, cx)
        }
        Command::StateGet {
            target: StateTarget::Windows,
            ..
        } => Ok(windows_state(cx)),
        Command::StateGet { target, .. } => Err(unavailable(&format!(
            "state.get {}",
            format!("{target:?}").to_lowercase()
        ))),
        Command::UiTree { window, .. } => {
            let entry = target(window.as_ref(), cx)?;
            let mut tree = entry
                .handle
                .read(cx)
                .map_err(|_| gone())?
                .registry()
                .borrow()
                .tree();
            tree["window"] = json!(number(entry.handle));
            Ok(tree)
        }
        Command::UiClick {
            window,
            at: Pointer::Element(r),
            count,
            ..
        } => {
            let entry = target(window.as_ref(), cx)?;
            click(&entry, &r.id, *count, cx)
        }
        Command::UiClick { .. } => Err(unavailable("ui.click at a point")),
        _ => Err(unavailable("this command")),
    }
}

/// `app.quit`: never loses an unsaved song unasked. Without `discard`,
/// unsaved changes answer `unsaved_changes` naming the songs, and nothing
/// is shown or closed. With it, or with nothing unsaved, quits at once.
fn quit(discard: bool, cx: &mut App) -> Handled {
    let unsaved: Vec<String> = {
        let s = AppState::global(cx);
        let s = s.read(cx);
        s.windows()
            .iter()
            .filter(|w| s.has_unsaved_changes(w.doc))
            .filter_map(|w| s.store.document(w.doc).map(|d| d.snapshot().title.clone()))
            .collect()
    };
    if !discard && !unsaved.is_empty() {
        return Handled::Reply(Err(ControlError::new(
            ErrorCode::UnsavedChanges,
            quit_refusal(&unsaved),
        )));
    }
    Handled::Quit
}

/// The `unsaved_changes` message (as the Swift app words it).
pub fn quit_refusal(titles: &[String]) -> String {
    let names = titles
        .iter()
        .map(|t| format!("\u{201c}{t}\u{201d}"))
        .collect::<Vec<_>>()
        .join(", ");
    let what = if titles.len() == 1 {
        "1 song has".to_owned()
    } else {
        format!("{} songs have", titles.len())
    };
    format!(
        "{what} unsaved changes: {names}. Save or close them first, or pass discard: true \
         (their changes are lost)."
    )
}

fn run_command(
    id: &str,
    args: &[(String, ArgValue)],
    entry: &WindowEntry,
    cx: &mut App,
) -> Result<Value, ControlError> {
    let app = AppState::global(cx);
    let doc = entry.doc;
    let before = alert_id(cx);
    let path = args.iter().find_map(|(k, v)| match (k.as_str(), v) {
        ("path", ArgValue::Text(p)) => Some(PathBuf::from(p)),
        _ => None,
    });
    match id {
        "file.save" | "file.save_as" => {
            let saved = match (id, path) {
                (_, Some(path)) => app.update(cx, |s, cx| {
                    cx.notify();
                    s.save_as(doc, &path)
                }),
                ("file.save", None) => match app.update(cx, |s, cx| {
                    cx.notify();
                    s.save(doc)
                }) {
                    SaveAction::Saved => Ok(()),
                    SaveAction::NeedsSaveAs => return Err(ControlError::dialog("Save", &["path"])),
                    SaveAction::Failed(m) => Err(m),
                },
                _ => return Err(ControlError::dialog("Save", &["path"])),
            };
            if let Err(m) = saved {
                return Err(failure(before, &m, cx));
            }
        }
        "transport.play_stop" => {
            let can = app.read(cx).can_play(doc)
                || app.read(cx).transport(doc).is_some_and(|t| t.is_playing());
            if !can {
                return Err(ControlError::new(
                    ErrorCode::Unavailable,
                    "Play / Stop is disabled: the song has nothing to play",
                ));
            }
            entry
                .handle
                .update(cx, |v, window, cx| v.play_stop(&PlayStop, window, cx))
                .map_err(|_| gone())?;
        }
        "transport.stop" => {
            entry
                .handle
                .update(cx, |v, window, cx| {
                    v.stop_and_return(&StopAndReturn, window, cx)
                })
                .map_err(|_| gone())?;
        }
        _ => return Err(unavailable(&format!("command.run {id}"))),
    }
    if alert_id(cx) != before {
        return Err(failure(before, "the command failed", cx));
    }
    let undo = app
        .read(cx)
        .store
        .document(doc)
        .and_then(|d| d.snapshot().undo_label.clone());
    Ok(json!({
        "command": id,
        "doc": doc_id(doc),
        "window": number(entry.handle),
        "undo": undo,
        "status": Value::Null,
    }))
}

/// `state.get transport`, read fresh.
fn transport_state(entry: &WindowEntry, cx: &mut App) -> Result<Value, ControlError> {
    let app = AppState::global(cx);
    let doc = entry.doc;
    app.update(cx, |s, cx| {
        if s.poll(doc) {
            cx.notify();
        }
    });
    let s = app.read(cx);
    let t = s
        .transport(doc)
        .ok_or_else(|| ControlError::new(ErrorCode::NoDocument, "the document is closed"))?
        .state();
    Ok(json!({
        "doc": doc_id(doc), "playing": t.playing, "seq": t.seq, "tick": t.tick,
        "position": t.position_bbu, "seconds": t.seconds, "sample_rate": t.sample_rate,
        "device": t.device, "faults": t.faults, "report": t.report,
        "rate_mismatch": t.rate_mismatch, "counting_in": t.counting_in,
    }))
}

/// `state.get windows`: the document windows, front first, and the alert
/// the store holds. `frame` is the window's bounds in GPUI's screen
/// coordinates (top-left origin).
fn windows_state(cx: &mut App) -> Value {
    let app = AppState::global(cx);
    let (mut entries, front, key) = {
        let s = app.read(cx);
        (
            s.windows().to_vec(),
            s.store.front(),
            s.store.key_document(),
        )
    };
    entries.sort_by_key(|w| Some(w.doc) != front);
    let mut rows = Vec::new();
    for w in entries {
        let Ok(bounds) = w.handle.update(cx, |_, window, _| window.bounds()) else {
            continue;
        };
        let title = app.read(cx).title(w.doc).unwrap_or_default();
        rows.push(json!({
            "number": number(w.handle),
            "title": title,
            "kind": "document",
            "doc": doc_id(w.doc),
            "key": key == Some(w.doc),
            "main": key == Some(w.doc),
            "visible": true,
            "frame": {
                "x": f32::from(bounds.origin.x),
                "y": f32::from(bounds.origin.y),
                "width": f32::from(bounds.size.width),
                "height": f32::from(bounds.size.height),
            },
        }));
    }
    let alerts: Vec<Value> = app
        .read(cx)
        .store
        .alert()
        .map(|a| json!({ "title": a.title, "message": a.message }))
        .into_iter()
        .collect();
    json!({ "windows": rows, "alerts": alerts })
}

/// `ui.click {element}`: a mouse down and up at the element's centre,
/// dispatched through the window's hit-testing as a user's click is
/// (left button, no modifiers this slice).
fn click(entry: &WindowEntry, id: &str, count: u32, cx: &mut App) -> Result<Value, ControlError> {
    let at = entry
        .handle
        .read(cx)
        .map_err(|_| gone())?
        .registry()
        .borrow()
        .resolve(id)?
        .bounds
        .center();
    let any: AnyWindowHandle = entry.handle.into();
    let count = count.max(1) as usize;
    // Through the window, not its view: the click's handlers update the
    // view, which a view update would hold.
    any.update(cx, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: Modifiers::none(),
                click_count: count,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: Modifiers::none(),
                click_count: count,
            }),
            cx,
        );
    })
    .map_err(|_| gone())?;
    Ok(json!({
        "window": number(entry.handle),
        "events": 2,
        "x": f32::from(at.x),
        "y": f32::from(at.y),
    }))
}

/// The document window `window` names; absent: the front document's.
fn target(window: Option<&WindowRef>, cx: &App) -> Result<WindowEntry, ControlError> {
    let app = AppState::global(cx);
    let s = app.read(cx);
    match window {
        Some(WindowRef::Number(n)) => s
            .windows()
            .iter()
            .find(|w| number(w.handle) == *n)
            .cloned()
            .ok_or_else(|| ControlError::new(ErrorCode::NoWindow, format!("no VizDAW window {n}"))),
        Some(WindowRef::Key | WindowRef::Main) => s
            .store
            .key_document()
            .and_then(|d| s.window_of(d))
            .cloned()
            .ok_or_else(|| ControlError::new(ErrorCode::NoWindow, "no document window is key")),
        None => s
            .store
            .front()
            .and_then(|d| s.window_of(d))
            .or_else(|| s.windows().first())
            .cloned()
            .ok_or_else(|| ControlError::new(ErrorCode::NoDocument, "no document is open")),
    }
}

/// A window's number, as `state.get windows` lists it.
pub fn number(handle: WindowHandle<DocumentWindow>) -> i64 {
    let any: AnyWindowHandle = handle.into();
    any.window_id().as_u64() as i64
}

/// A document's ID on the channel.
pub fn doc_id(doc: DocId) -> String {
    doc.0.to_string()
}

fn opened(h: WindowHandle<DocumentWindow>, cx: &App) -> Result<Value, ControlError> {
    let doc = h.read(cx).map_err(|_| gone())?.doc();
    Ok(json!({ "doc": doc_id(doc), "window": number(h) }))
}

fn alert_id(cx: &App) -> Option<u64> {
    AppState::global(cx).read(cx).store.alert().map(|a| a.id)
}

/// The call's `failed` error: the alert it raised, taken so it is not
/// shown, or `fallback`.
fn failure(before: Option<u64>, fallback: &str, cx: &mut App) -> ControlError {
    let app = AppState::global(cx);
    let raised = app.update(cx, |s, cx| {
        if s.store.alert().map(|a| a.id) == before {
            return None;
        }
        cx.notify();
        s.store.take_alert()
    });
    let message = match raised {
        Some(a) if !a.message.is_empty() => a.message,
        Some(a) => a.title,
        None => fallback.to_owned(),
    };
    ControlError::new(ErrorCode::Failed, message)
}

fn gone() -> ControlError {
    ControlError::new(ErrorCode::NoWindow, "the window closed")
}

fn unavailable(what: &str) -> ControlError {
    ControlError::new(
        ErrorCode::Unavailable,
        format!(
            "{what} is not available in the GPUI app yet (slice 1 answers app.new_demo, \
             app.open, app.quit, command.run file.save / file.save_as / transport.play_stop / \
             transport.stop, state.get transport / windows, ui.tree and ui.click {{element}})"
        ),
    )
}

/// Starts the server at `path` and pumps it every [`PUMP_INTERVAL`] for
/// the app's life. Quitting (any quit) stops it first: pending calls are
/// answered `shutting_down` and the last replies written.
pub fn start(path: &Path, cx: &mut App) -> Result<(), StartError> {
    let server = Rc::new(RefCell::new(Server::start(path, Config::default())?));
    eprintln!("control channel on {}", path.display());
    let on_quit = server.clone();
    cx.on_app_quit(move |_| {
        match on_quit.try_borrow_mut() {
            Ok(mut s) => s.stop(),
            Err(_) => eprintln!("control channel: could not stop the server while quitting"),
        }
        async {}
    })
    .detach();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(PUMP_INTERVAL).await;
            let tick = cx.update(|cx| pump(&*server.borrow(), cx));
            if tick.quit {
                break;
            }
        }
    })
    .detach();
    Ok(())
}

/// What the command line asks for. `--no-scan` and
/// `-ApplePersistenceIgnoreState YES` (passed by `vizctl launch`) are
/// accepted and ignored, as is anything else unknown.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LaunchOptions {
    /// `--control PATH`, else `VIZDAW_CONTROL`.
    pub control: Option<PathBuf>,
    /// `--demo`: the first window is the demo project.
    pub demo: bool,
}

impl LaunchOptions {
    /// Reads `args` (without the program name); `env_control` is
    /// `VIZDAW_CONTROL`.
    pub fn parse(
        args: impl IntoIterator<Item = impl Into<OsString>>,
        env_control: Option<OsString>,
    ) -> LaunchOptions {
        let mut o = LaunchOptions::default();
        let mut args = args.into_iter().map(Into::into);
        while let Some(a) = args.next() {
            match a.to_str() {
                Some("--control") => o.control = args.next().map(PathBuf::from),
                Some("--demo") => o.demo = true,
                Some("-ApplePersistenceIgnoreState") => {
                    args.next();
                }
                _ => {}
            }
        }
        if o.control.is_none() {
            o.control = env_control.filter(|c| !c.is_empty()).map(PathBuf::from);
        }
        o
    }

    /// This process's options.
    pub fn from_env() -> LaunchOptions {
        LaunchOptions::parse(
            std::env::args_os().skip(1),
            std::env::var_os("VIZDAW_CONTROL"),
        )
    }
}
