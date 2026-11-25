//! The control channel (spec `docs/superpowers/specs/2026-10-18-control-
//! channel-design.md`) in the GPUI app: `core_control::Server` on the
//! `--control` socket, pumped on a GPUI timer from the main thread without
//! blocking, at most [`MAX_CALLS_PER_TICK`] calls a tick so a burst cannot
//! stall drawing.
//!
//! Slices 1 to 3 answer `app.new_demo`, `app.open`, `app.quit`,
//! `command.run` for `file.new`, `file.close` (never asks), `file.save`,
//! `file.save_as`, the transport commands, `ui.set_transport_bar`,
//! `ui.reset_transport_bar` and the document commands (`edit.undo`,
//! `edit.redo`, the Track menu), `state.get` `transport`, `windows`,
//! `document` and `editor` (`step` and the transport marks come with
//! their slices); slice 4 adds `editors.snap`, `editors.list`,
//! `editors.close_detail`, `ui.toggle_bottom`, `state.get selection`, the
//! detail kind's `wait` and `ui.click` at a point. `ui.tree`, `ui.click` (an element or a point,
//! with modifiers and a count), `ui.key`, `ui.screenshot` and `wait`.
//! Slice 6a adds the main views and panes (`ui.view_*`, `ui.toggle_*`,
//! `ui.pane`), `automation.set_mode|gesture|release_all`, `mixer.new_bus`
//! and `state.get mixer|layout|automation`; `window.mixer` opens the
//! document's Mixer window (or brings it to the front), which
//! `state.get windows` lists with kind `mixer` and every window call can
//! name by its number. With `VIZDAW_FRAME_STATS` set (a measurement
//! build's switch, see [`crate::frame_stats`]) each `state.get windows`
//! row carries the window's `frames` since the last read.
//! Host H2 adds the plug-in windows (kind `plugin`, titled "‹strip› —
//! ‹plug-in›") and the Built-in Synth windows (kind `other`) to
//! `state.get windows`; `ui.tree`, `ui.click`, `ui.key` and `ui.screenshot`
//! reach them by their number.
//! Slice 7a adds the File commands (see [`files`]): the new projects and
//! demos (`app.new_demo {audio}` too), `file.open` and `app.open` with
//! their recovery answers, `file.open_copy`, the imports, exports,
//! checks, bounce, project settings, convert and relink, whose job
//! replies wait for the job's end, and `state.get media|project`.
//! Slice 8a adds recording and step entry: Record, its loop passes,
//! Record Options (`record.set`), `record.recover` (never the Recovered
//! Recordings alert), the click, arming, the locators, the marker
//! chores, `midi.inject` and the `step.*` commands; `state.get
//! record|input`, step entry and the transport marks in `state.get
//! editor`, the click in `state.get transport`; `ui.key` finds step
//! entry's keys while it is on. Everything else answers `unavailable`.
//!
//! A `wait` is not answered in the tick it is taken unless it already
//! holds: it joins a list the pump checks every tick against fresh facts,
//! answering it once it holds or its time is up. Pending waits take no
//! share of a tick's [`MAX_CALLS_PER_TICK`] calls.
//!
//! Calls are handled in the pump's own top-level update, never inside
//! `AppState::update` or a window's update, so a call may open windows and
//! update window handles directly (GPUI panics on, or refuses, a nested
//! one). Every other call is answered in the tick it is taken; `app.quit` is
//! answered first and the app quits once that update has ended
//! (`cx.defer`); the server is stopped while quitting so the reply is
//! written before the process goes.

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use core_control::bindings::{self, Chord};
use core_control::keys::{self, KeyStroke};
use core_control::params::WindowRef;
use core_control::registry::ArgValue;
use core_control::request::{Button, Command, Pointer, StateTarget};
use core_control::server::Call;
use core_control::wait::{self, DocFacts, Facts, WaitCondition};
use core_control::{Config, ControlError, ErrorCode, Server, StartError};
use gpui::{
    AnyWindowHandle, App, Global, KeyUpEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseUpEvent, PlatformInput, WindowHandle,
};
use serde_json::{Value, json};
use vizdaw_app::commands::{
    self, APP_COMMANDS, LANE_INPUT_COMMANDS, RECORD_COMMANDS, TRANSPORT_COMMANDS,
};
use vizdaw_app::step_entry::STEP_COMMANDS;
use vizdaw_app::{DocId, SaveAction};
use vizdaw_ffi::control::{
    AlertRow, ControlResult, DetailRow, EditorStateRow, TransportMarksRow, result_json,
};

use crate::app_state::{AppState, WindowEntry, WindowKind};
use crate::menus;
use crate::shell::DocumentWindow;
use crate::transport_fields::NO_FIELDS;

pub(crate) mod files;

/// The theme `state.get windows` names: the one fixed look the GPUI app
/// draws until themes are ported (its dark palette).
pub const THEME: &str = "studio-dark";

thread_local! {
    static CHANNEL_CLICK: Cell<bool> = const { Cell::new(false) };
}

/// True while a `ui.click` from the control channel is being delivered.
/// A control whose mouse click opens a menu (which would track modally in
/// the app the channel drives) does something else for this one: the
/// Add Track row sets the status, a track header's row toggles mute.
pub fn is_channel_click() -> bool {
    CHANNEL_CLICK.with(Cell::get)
}

/// Marks a channel click for its life; cleared even if a handler panics.
struct ChannelClick;

impl ChannelClick {
    fn begin() -> Self {
        CHANNEL_CLICK.with(|c| c.set(true));
        ChannelClick
    }
}

impl Drop for ChannelClick {
    fn drop(&mut self) {
        CHANNEL_CLICK.with(|c| c.set(false));
    }
}

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
    /// Where a `ui.screenshot` without `path` goes; `None`: a new file in
    /// the system's temporary folder.
    fn screenshot_path(&self) -> Option<PathBuf> {
        None
    }
    /// Whether a client still waits for call `token` (a pending `wait`
    /// whose client left is dropped).
    fn is_waiting(&self, _token: u64) -> bool {
        true
    }
    /// Gives call `token` `by` from now to be answered instead of the
    /// server's app timeout (a job's reply waits up to 600 s).
    fn extend(&self, _token: u64, _by: Duration) -> bool {
        true
    }
}

impl CallSource for Server {
    fn next_call(&self) -> Option<Call> {
        Server::next_call(self)
    }

    fn reply(&self, token: u64, outcome: Result<Value, ControlError>) -> bool {
        Server::reply(self, token, outcome)
    }

    fn screenshot_path(&self) -> Option<PathBuf> {
        Some(Server::screenshot_path(self))
    }

    fn is_waiting(&self, token: u64) -> bool {
        Server::is_waiting(self, token)
    }

    fn extend(&self, token: u64, by: Duration) -> bool {
        Server::extend(self, token, by)
    }
}

/// What [`handle`] made of a call.
#[derive(Debug, Clone, PartialEq)]
pub enum Handled {
    /// Answer this.
    Reply(Result<Value, ControlError>),
    /// Answer `{quitting: true}`, then quit without asking.
    Quit,
    /// Job `job` of `doc` started: answered when it ends (at most
    /// [`vizdaw_app::jobs::JOB_WAIT_MS`]).
    Job {
        command: String,
        doc: DocId,
        job: u64,
    },
    /// Work runs off the UI thread (a file read, a dry run): answered
    /// once it lands. The number is its generation.
    Later(u64),
}

/// What one pump tick did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    /// Calls taken and answered.
    pub handled: usize,
    /// An `app.quit` was answered; the app quits once this update ends.
    pub quit: bool,
    /// `wait` calls still waiting after this tick.
    pub waiting: usize,
    /// Job replies and off-thread calls still waiting after this tick.
    pub deferred: usize,
}

/// The control channel is on (Swift `ControlChannel.active`): a server
/// was started, or calls were pumped. No prompt, sheet or offer shows
/// then; the channel answers and reports them.
#[derive(Default)]
struct ChannelOn(bool);

impl Global for ChannelOn {}

/// Whether the control channel is on ([`start`] ran, or [`pump`] did).
pub fn channel_on(cx: &App) -> bool {
    cx.try_global::<ChannelOn>().is_some_and(|c| c.0)
}

fn set_channel_on(cx: &mut App) {
    if !channel_on(cx) {
        cx.set_global(ChannelOn(true));
    }
}

/// Takes and answers up to [`MAX_CALLS_PER_TICK`] calls, then checks the
/// pending waits. Call it from a top-level update (the pump timer's), not
/// from inside an entity's or a window's update.
pub fn pump(calls: &impl CallSource, cx: &mut App) -> Tick {
    set_channel_on(cx);
    let mut tick = Tick::default();
    while tick.handled < MAX_CALLS_PER_TICK && !tick.quit {
        let Some(call) = calls.next_call() else { break };
        tick.handled += 1;
        match &call.command {
            Command::Wait {
                condition,
                window,
                timeout_ms,
            } => start_wait(calls, call.token, condition, *window, *timeout_ms, cx),
            Command::UiScreenshot {
                window,
                path,
                scale,
                live,
            } => {
                let path = path.clone().or_else(|| calls.screenshot_path());
                let r = screenshot(window.as_ref(), path, *scale, *live, cx);
                calls.reply(call.token, r);
            }
            command => match handle(command, cx) {
                Handled::Reply(r) => {
                    calls.reply(call.token, r);
                }
                Handled::Quit => {
                    calls.reply(call.token, Ok(json!({ "quitting": true })));
                    tick.quit = true;
                    // After this update: the reply is on its way first.
                    cx.defer(crate::quit_now);
                }
                later => files::defer(calls, call.token, &later, cx),
            },
        }
    }
    if !tick.quit {
        check_waits(calls, cx);
        answer_questions(cx);
        files::check(calls, cx);
        files::settle(cx);
    }
    tick.waiting = cx.try_global::<Waits>().map_or(0, |w| w.0.len());
    tick.deferred = files::deferred_count(cx);
    tick
}

/// The notes the channel took instead of asking (Swift
/// `ControlChannel.alerts`): `state.get windows` lists the last 20.
#[derive(Default)]
struct Notes(Vec<vizdaw_app::AlertItem>);

impl Global for Notes {}

/// The most notes kept.
const NOTES_KEPT: usize = 20;

/// Under the channel nothing asks: a return prompt is answered Cancel, a
/// crash question Cancel and an autosave question Open Last Saved; each
/// is noted.
fn answer_questions(cx: &mut App) {
    let app = AppState::global(cx);
    let notes = app.update(cx, |s, _| s.store.answer_for_channel());
    if notes.is_empty() {
        return;
    }
    let kept = &mut cx.default_global::<Notes>().0;
    for n in notes {
        eprintln!("vizdaw: alert {}: {}", n.title, n.message);
        kept.push(n);
    }
    if kept.len() > NOTES_KEPT {
        let extra = kept.len() - NOTES_KEPT;
        kept.drain(..extra);
    }
}

/// A `wait` not answered yet.
struct PendingWait {
    token: u64,
    condition: WaitCondition,
    window: Option<WindowRef>,
    started: Instant,
    timeout_ms: u64,
}

/// The waits the pump checks every tick.
#[derive(Default)]
struct Waits(Vec<PendingWait>);

impl Global for Waits {}

/// Takes a `wait`: a stale window number is `no_window` at once (for a
/// condition that reads a document); a condition that holds is answered
/// at once; else it waits in the list.
fn start_wait(
    calls: &impl CallSource,
    token: u64,
    condition: &WaitCondition,
    window: Option<WindowRef>,
    timeout_ms: u64,
    cx: &mut App,
) {
    if condition.needs_document()
        && let Some(WindowRef::Number(n)) = window
        && target(window.as_ref(), cx).is_err()
    {
        calls.reply(
            token,
            Err(ControlError::new(
                ErrorCode::NoWindow,
                format!("no VizDAW window {n}"),
            )),
        );
        return;
    }
    let pending = PendingWait {
        token,
        condition: condition.clone(),
        window,
        started: cx.background_executor().now(),
        timeout_ms,
    };
    let facts = facts(window.as_ref(), cx);
    if wait::met(condition, &facts) {
        calls.reply(token, Ok(waited(&pending, pending.started)));
        return;
    }
    cx.default_global::<Waits>().0.push(pending);
}

/// Answers every pending wait that holds now or whose time is up. The
/// facts are read once per window the waits name.
fn check_waits(calls: &impl CallSource, cx: &mut App) {
    let Some(waits) = cx.try_global::<Waits>() else {
        return;
    };
    if waits.0.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut cx.global_mut::<Waits>().0);
    let now = cx.background_executor().now();
    let mut seen: Vec<(Option<WindowRef>, Facts)> = Vec::new();
    let mut kept = Vec::new();
    for w in pending.into_iter().filter(|w| calls.is_waiting(w.token)) {
        let f = match seen.iter().find(|(r, _)| *r == w.window) {
            Some((_, f)) => f.clone(),
            None => {
                let f = facts(w.window.as_ref(), cx);
                seen.push((w.window, f.clone()));
                f
            }
        };
        if wait::met(&w.condition, &f) {
            calls.reply(w.token, Ok(waited(&w, now)));
        } else if now.saturating_duration_since(w.started) >= Duration::from_millis(w.timeout_ms) {
            let e = ControlError::new(
                ErrorCode::Timeout,
                format!(
                    "{} did not happen within {} ms",
                    w.condition.describe(),
                    w.timeout_ms
                ),
            )
            .with_details(wait::timeout_details(&w.condition, w.timeout_ms, &f));
            calls.reply(w.token, Err(e));
        } else {
            kept.push(w);
        }
    }
    cx.global_mut::<Waits>().0.extend(kept);
}

/// A met wait's answer: `{condition, elapsed_ms}`.
fn waited(w: &PendingWait, now: Instant) -> Value {
    let ms = now.saturating_duration_since(w.started).as_millis();
    json!({ "condition": w.condition.name(), "elapsed_ms": u64::try_from(ms).unwrap_or(u64::MAX) })
}

/// What a wait reads now: the target document's transport (polled fresh)
/// and detail kind, the open documents and the windows' titles.
fn facts(window: Option<&WindowRef>, cx: &mut App) -> Facts {
    let app = AppState::global(cx);
    let doc = target(window, cx).ok().map(|e| e.doc);
    let document = doc.and_then(|doc| {
        app.update(cx, |s, cx| {
            if s.poll(doc) {
                cx.notify();
            }
        });
        let s = app.read(cx);
        let t = s.transport(doc)?.state();
        // The detail's kind, whether or not the bottom pane shows.
        let detail_kind = s.store.document(doc)?.detail_kind_name().into();
        Some(DocFacts {
            playing: t.playing,
            tick: t.tick,
            detail_kind,
        })
    });
    let s = app.read(cx);
    Facts {
        document,
        documents: u32::try_from(s.store.count()).unwrap_or(u32::MAX),
        window_titles: s
            .windows()
            .iter()
            .filter_map(|w| s.window_title(w))
            .collect(),
    }
}

/// Answers one call.
pub fn handle(command: &Command, cx: &mut App) -> Handled {
    match command {
        Command::AppQuit { discard } => quit(*discard, cx),
        Command::CommandRun { id, args, window } if files::DOC_IDS.contains(&id.as_str()) => {
            match target(window.as_ref(), cx) {
                Ok(entry) => files::run(id, args, &entry, cx),
                Err(e) => Handled::Reply(Err(e)),
            }
        }
        other => Handled::Reply(answer(other, cx)),
    }
}

fn answer(command: &Command, cx: &mut App) -> Result<Value, ControlError> {
    match command {
        Command::AppNewDemo { audio: true } => files::new_demo_audio(cx),
        Command::AppNewDemo { audio: false } => {
            let before = alert_id(cx);
            match crate::new_demo(cx) {
                Some(h) => opened(h, cx),
                None => Err(failure(before, "no demo was made", cx)),
            }
        }
        Command::AppOpen { path, answers } => {
            let doc = files::open_answered(path, answers, cx)?;
            let app = AppState::global(cx);
            let before = alert_id(cx);
            let h = match app
                .read(cx)
                .window_of(doc)
                .and_then(|w| w.document_window())
            {
                Some(h) => h,
                None => DocumentWindow::open(&app, doc, cx).map_err(|m| failure(before, &m, cx))?,
            };
            opened(h, cx)
        }
        Command::CommandRun { id, args, window } => {
            if let Some(r) = app_wide(id, args, cx) {
                return r;
            }
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
        Command::StateGet {
            target: StateTarget::Document,
            window,
        } => {
            let entry = target(window.as_ref(), cx)?;
            document_state(entry.doc, cx)
        }
        Command::StateGet {
            target: StateTarget::Editor,
            window,
        } => {
            let entry = target(window.as_ref(), cx)?;
            editor_state(entry.doc, cx)
        }
        Command::StateGet {
            target: StateTarget::Selection,
            window,
        } => {
            let entry = target(window.as_ref(), cx)?;
            selection_state(entry.doc, cx)
        }
        Command::StateGet {
            target: what @ (StateTarget::Mixer | StateTarget::Layout | StateTarget::Automation),
            window,
        } => {
            let entry = target(window.as_ref(), cx)?;
            mixer_state(*what, entry.doc, cx)
        }
        Command::StateGet {
            target: what @ (StateTarget::Media | StateTarget::Project),
            window,
        } => {
            let entry = target(window.as_ref(), cx)?;
            file_state(*what, entry.doc, cx)
        }
        Command::StateGet {
            target: what @ (StateTarget::Record | StateTarget::Input),
            window,
        } => {
            let entry = target(window.as_ref(), cx)?;
            record_state(*what, entry.doc, cx)
        }
        Command::StateGet { target, .. } => Err(not_yet(&format!(
            "state.get {}",
            format!("{target:?}").to_lowercase()
        ))),
        Command::UiTree { window, .. } => {
            let entry = target(window.as_ref(), cx)?;
            fresh_frame(&entry, cx)?;
            let mut tree = entry.registry(cx).ok_or_else(gone)?.borrow().tree();
            tree["window"] = json!(number(entry.handle));
            Ok(tree)
        }
        Command::UiClick {
            window,
            at: Pointer::Element(r),
            button,
            count,
            modifiers,
        } => {
            let entry = target(window.as_ref(), cx)?;
            click(&entry, &r.id, *button, *count, *modifiers, cx)
        }
        Command::UiClick {
            window,
            at: Pointer::Point { x, y },
            button,
            count,
            modifiers,
        } => {
            let entry = target(window.as_ref(), cx)?;
            fresh_frame(&entry, cx)?;
            let at = gpui::point(gpui::px(*x as f32), gpui::px(*y as f32));
            click_at(&entry, at, *button, *count, *modifiers, cx)
        }
        Command::UiKey {
            window,
            focus,
            strokes,
        } => {
            let entry = target(window.as_ref(), cx)?;
            key_strokes(&entry, focus.as_ref().map(|f| f.id.as_str()), strokes, cx)
        }
        Command::UiScreenshot {
            window,
            path,
            scale,
            live,
        } => screenshot(window.as_ref(), path.clone(), *scale, *live, cx),
        _ => Err(not_yet("this command")),
    }
}

/// `app.quit`: never loses an unsaved song unasked. Without `discard`,
/// unsaved changes answer `unsaved_changes` naming the songs, and nothing
/// is shown or closed. With it, or with nothing unsaved, quits at once.
fn quit(discard: bool, cx: &mut App) -> Handled {
    // A typed name is in the songs checked (and lost only with them).
    crate::commit_renames(cx);
    let unsaved: Vec<String> = {
        let s = AppState::global(cx);
        let s = s.read(cx);
        s.document_windows()
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
        _ if menus::routed(id) => {
            let ran = app.update(cx, |s, cx| {
                cx.notify();
                s.store.with_document(doc, |d, _| {
                    // A result's `status` is this command's own: what an
                    // earlier action left is cleared first.
                    d.set_status(None);
                    commands::run(id, args, d)
                })
            });
            let ran = ran.ok_or_else(closed)??;
            if alert_id(cx) != before {
                return Err(failure(before, "the command failed", cx));
            }
            if ran.open_mixer_window {
                // Opened here, once: the key is not delivered to the
                // window as well (see `key_strokes`), so its binding does
                // not open it again.
                if let Err(m) = DocumentWindow::open_mixer(&app, doc, cx) {
                    return Err(failure(before, &m, cx));
                }
            }
            return Ok(ran_json(id, doc, entry, ran.undo, ran.status));
        }
        // Record, step entry, the channel's record commands and the track
        // input edits run as the transport's: an alert they raise is the
        // call's failure, and the window reads the transport again (a step
        // moves it).
        _ if TRANSPORT_COMMANDS.contains(&id)
            || RECORD_COMMANDS.contains(&id)
            || LANE_INPUT_COMMANDS.contains(&id)
            || STEP_COMMANDS.contains(&id) =>
        {
            return run_transport(id, args, entry, cx);
        }
        // A key (`ui.key`) runs a File command without arguments: it asks
        // for its dialog, which is the answer. One that would start a job
        // or a background read is refused before it starts: nobody waits
        // for its reply.
        _ if files::DOC_IDS.contains(&id) => return files::run_now(id, args, entry, cx),
        "file.close" => {
            // Never asks (Swift `control_close_never_prompts`): the window
            // closes without its save prompt and the document goes, saved
            // or not. Aimed at the Mixer window, it is the document's own
            // window that closes; the Mixer window closes itself when it
            // sees its document gone.
            let own = app
                .read(cx)
                .window_of(doc)
                .map_or(entry.handle, |w| w.handle);
            own.update(cx, |_, window, _| window.remove_window())
                .map_err(|_| gone())?;
            app.update(cx, |s, cx| {
                s.close_document(doc);
                cx.notify();
            });
            return Ok(ran_json(id, doc, entry, None, None));
        }
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
                    SaveAction::Failed(m) | SaveAction::Refused(m) => Err(m),
                },
                _ => return Err(ControlError::dialog("Save", &["path"])),
            };
            if let Err(m) = saved {
                return Err(failure(before, &m, cx));
            }
        }
        _ => return Err(not_yet(&format!("command.run {id}"))),
    }
    if alert_id(cx) != before {
        return Err(failure(before, "the command failed", cx));
    }
    let undo = app
        .read(cx)
        .store
        .document(doc)
        .and_then(|d| d.snapshot().undo_label.clone());
    Ok(ran_json(id, doc, entry, undo, None))
}

/// A `command.run` reply: `{command, doc, window, undo, status}`.
fn ran_json(
    id: &str,
    doc: DocId,
    entry: &WindowEntry,
    undo: Option<String>,
    status: Option<String>,
) -> Value {
    json!({
        "command": id,
        "doc": doc_id(doc),
        "window": number(entry.handle),
        "undo": undo,
        "status": status,
    })
}

/// A transport command on `entry`'s document, as `commands::run` runs it
/// for the channel: an alert it raised is the call's failure (and is not
/// left showing); the window reads the transport again; a field key's
/// field takes the keyboard.
fn run_transport(
    id: &str,
    args: &[(String, ArgValue)],
    entry: &WindowEntry,
    cx: &mut App,
) -> Result<Value, ControlError> {
    let app = AppState::global(cx);
    let doc = entry.doc;
    let before = alert_id(cx);
    let ran = app
        .update(cx, |s, cx| {
            cx.notify();
            s.store.with_document(doc, |d, _| {
                d.set_status(None);
                commands::run(id, args, d)
            })
        })
        .ok_or_else(closed)?;
    if alert_id(cx) != before {
        return Err(failure(before, "the command failed", cx));
    }
    let ran = ran?;
    // A plug-in window has no transport bar: its document's window reads
    // the transport again.
    let bar = entry
        .document_window()
        .or_else(|| {
            AppState::global(cx)
                .read(cx)
                .window_of(entry.doc)
                .and_then(|w| w.document_window())
        })
        .ok_or_else(gone)?;
    bar.update(cx, |v, window, cx| v.after_transport(window, cx))
        .map_err(|_| gone())?;
    if let Some(focus) = ran.focus {
        let field = focus.field();
        let shown = bar
            .update(cx, |v, window, cx| {
                let shown = v.transport_fields.shown().contains(&field);
                if shown {
                    v.begin_field(field, window, cx);
                }
                shown
            })
            .map_err(|_| gone())?;
        if !shown {
            return Err(ControlError::new(ErrorCode::Unavailable, NO_FIELDS));
        }
    }
    Ok(ran_json(id, doc, entry, ran.undo, ran.status))
}

/// A command that needs no document window: `file.new` and the app-wide
/// ones (`ui.set_transport_bar`, `ui.reset_transport_bar`, answered with
/// no `doc` or `window`, as Swift's). `None` for any other.
fn app_wide(
    id: &str,
    args: &[(String, ArgValue)],
    cx: &mut App,
) -> Option<Result<Value, ControlError>> {
    if files::APP_IDS.contains(&id) {
        return Some(files::run_app(id, args, cx));
    }
    if !APP_COMMANDS.contains(&id) {
        return None;
    }
    let app = AppState::global(cx);
    let ran = app.update(cx, |s, cx| {
        let ran = commands::run_app(id, args, &mut s.prefs);
        // Every window draws its bar from the preferences.
        cx.notify();
        ran
    });
    Some(ran.map(|ran| {
        json!({
            "command": id, "doc": Value::Null, "window": Value::Null,
            "undo": ran.undo, "status": ran.status,
        })
    }))
}

/// `state.get document`, in the Swift app's fields: the latest snapshot,
/// the shown sequence and its lanes and markers, the destinations, and the
/// alert the window shows.
fn document_state(doc: DocId, cx: &App) -> Result<Value, ControlError> {
    let s = AppState::global(cx);
    let s = s.read(cx);
    let d = s.store.document(doc).ok_or_else(closed)?;
    let session = d.session();
    let selected_seq = d.selected_seq();
    let alert = d.alert.as_ref().or(s.store.alert()).map(|a| AlertRow {
        title: a.title.clone(),
        message: a.message.clone(),
    });
    Ok(result_json(&ControlResult::Document {
        doc: doc_id(doc),
        path: d.path().map(|p| p.to_string_lossy().into_owned()),
        snapshot: d.snapshot().clone(),
        selected_seq,
        lanes: d.lanes().to_vec(),
        markers: selected_seq
            .and_then(|q| session.markers(q).ok())
            .unwrap_or_default(),
        destinations: session.destinations().unwrap_or_default(),
        alert,
    }))
}

/// `state.get editor`, in the Swift shape (`ControlResult::Editor`): the
/// detail, no torn-off windows, the tool, the grid, no exact mode, whether
/// a drag is open and the status, step entry (`step`, `null` while off
/// or ended; its position read now) and the transport marks (locators,
/// punch points, punch and overdub; `transport` on the wire).
fn editor_state(doc: DocId, cx: &App) -> Result<Value, ControlError> {
    let s = AppState::global(cx);
    let s = s.read(cx);
    let d = s.store.document(doc).ok_or_else(closed)?;
    let detail = d.layout().detail.as_ref().map(|detail| {
        let t = detail.target;
        let (start, end) = match t {
            vizdaw_app::editor::EditorTarget::Block { start, end, .. } => (Some(start), Some(end)),
            _ => (None, None),
        };
        DetailRow {
            kind: detail.kind.name().to_owned(),
            seq: t.seq(),
            lane: t.lane(),
            start,
            end,
        }
    });
    let snap = d.layout().snap;
    let marks = &d.layout().transport;
    Ok(result_json(&ControlResult::Editor {
        doc: doc_id(doc),
        editor: EditorStateRow {
            detail,
            windows: vec![],
            tool: d.tool().name().to_owned(),
            snap_enabled: snap.enabled,
            snap_division: snap.division.name().to_owned(),
            exact_mode: false,
            gesture: d.gesture_open(),
            status: d.status().map(String::from),
            step: d.step_state_row(),
            marks: TransportMarksRow {
                locators: marks.locators.to_vec(),
                punch_in: marks.punch_in,
                punch_out: marks.punch_out,
                punch: marks.punch,
                overdub: marks.overdub,
            },
        },
    }))
}

/// `state.get mixer`, `layout` and `automation`, in the Swift shapes
/// (`ControlResult::{Mixer, Layout, Automation}`), the mixer and the
/// automation read fresh from the session: Busy is the call's `busy`,
/// another refusal its `failed`. The layout's left strip follows the
/// session's mixer and destinations (none when they cannot be read).
fn mixer_state(target: StateTarget, doc: DocId, cx: &App) -> Result<Value, ControlError> {
    let s = AppState::global(cx);
    let s = s.read(cx);
    let d = s.store.document(doc).ok_or_else(closed)?;
    let session = d.session();
    let read = read_error;
    let result = match target {
        StateTarget::Mixer => ControlResult::Mixer {
            doc: doc_id(doc),
            mixer: session.mixer().map_err(read)?,
        },
        StateTarget::Automation => ControlResult::Automation {
            doc: doc_id(doc),
            state: session.automation_state().map_err(read)?,
        },
        _ => {
            let strips = session.mixer().map(|m| m.strips).unwrap_or_default();
            let dests = session.destinations().unwrap_or_default();
            ControlResult::Layout {
                doc: doc_id(doc),
                layout: vizdaw_app::view::layout_row(d, &strips, &dests),
            }
        }
    };
    Ok(result_json(&result))
}

/// `state.get media` (the pool, read fresh; none when it cannot be read)
/// and `state.get project` (the format and what does not match it; a
/// refused read is no state at all, Swift's "No document is open in that
/// window.").
fn file_state(target: StateTarget, doc: DocId, cx: &App) -> Result<Value, ControlError> {
    let s = AppState::global(cx);
    let s = s.read(cx);
    let d = s.store.document(doc).ok_or_else(closed)?;
    let session = d.session();
    let result = match target {
        StateTarget::Media => ControlResult::Media {
            doc: doc_id(doc),
            items: session.media().unwrap_or_default(),
        },
        _ => ControlResult::Project {
            doc: doc_id(doc),
            info: session.project_info().map_err(|_| {
                ControlError::new(ErrorCode::NoDocument, "No document is open in that window.")
            })?,
        },
    };
    Ok(result_json(&result))
}

/// `state.get selection`: the time range, tracks and events selected.
fn selection_state(doc: DocId, cx: &App) -> Result<Value, ControlError> {
    let s = AppState::global(cx);
    let s = s.read(cx);
    let d = s.store.document(doc).ok_or_else(closed)?;
    Ok(result_json(&ControlResult::Selection {
        doc: doc_id(doc),
        selection: d.selection().clone(),
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
    // The Swift shape, the click (`null` until the session says) with it.
    Ok(result_json(&ControlResult::Transport {
        doc: doc_id(doc),
        state: t.clone(),
    }))
}

/// `state.get record` (what recording is doing, read fresh from the
/// session, nothing else touched) and `state.get input` (what the input
/// offers, read fresh, and each channel's peak since the channel's last
/// read). The peaks come from the document's own hold, which the meters'
/// reads feed too, so neither reader loses a peak to the other. Busy is
/// the call's `busy`, another refusal its `failed`.
fn record_state(target: StateTarget, doc: DocId, cx: &mut App) -> Result<Value, ControlError> {
    let app = AppState::global(cx);
    app.update(cx, |s, _| {
        let d = s.store.document_mut(doc).ok_or_else(closed)?;
        let result = match target {
            StateTarget::Record => ControlResult::Record {
                doc: doc_id(doc),
                state: d.session().record_state().map_err(read_error)?,
            },
            _ => ControlResult::InputState {
                doc: doc_id(doc),
                info: d.session().input_info().map_err(read_error)?,
                levels: d.take_channel_levels().map_err(read_error)?,
            },
        };
        Ok(result_json(&result))
    })
}

/// A refused session read as the call's error: Busy is `busy`.
fn read_error(e: vizdaw_ffi::VizError) -> ControlError {
    let code = if matches!(e, vizdaw_ffi::VizError::Busy { .. }) {
        ErrorCode::Busy
    } else {
        ErrorCode::Failed
    };
    ControlError::new(code, vizdaw_app::alerts::error_message(&e))
}

/// `state.get windows`: the document and Mixer windows, the front
/// document's first, and the alert the store holds. `frame` is the
/// window's bounds in GPUI's screen coordinates (top-left origin),
/// `content` the size of what it draws (no title bar). `key`
/// (and `main`) is the one window that has the keyboard.
fn windows_state(cx: &mut App) -> Value {
    let app = AppState::global(cx);
    let (mut entries, front) = {
        let s = app.read(cx);
        (s.windows().to_vec(), s.store.front())
    };
    entries.sort_by_key(|w| Some(w.doc) != front);
    let mut rows = Vec::new();
    for w in entries {
        let now = cx.background_executor().now();
        let shown = match w.document_window() {
            Some(h) => h.update(cx, |v, window, _| {
                (
                    window.bounds(),
                    window.viewport_size(),
                    v.take_frame_stats(now),
                )
            }),
            None => w.handle.update(cx, |_, window, _| {
                (window.bounds(), window.viewport_size(), None)
            }),
        };
        let Ok((bounds, content, frames)) = shown else {
            continue;
        };
        let (title, key) = {
            let s = app.read(cx);
            (s.window_title(&w).unwrap_or_default(), s.is_key(&w))
        };
        let mut row = json!({
            "number": number(w.handle),
            "title": title,
            "kind": w.kind.name(),
            "doc": doc_id(w.doc),
            "key": key,
            "main": key,
            "visible": true,
            "frame": {
                "x": f32::from(bounds.origin.x),
                "y": f32::from(bounds.origin.y),
                "width": f32::from(bounds.size.width),
                "height": f32::from(bounds.size.height),
            },
            "content": {
                "width": f32::from(content.width),
                "height": f32::from(content.height),
            },
        });
        if let Some(frames) = frames {
            row["frames"] = frames;
        }
        rows.push(row);
    }
    let notes = cx
        .try_global::<Notes>()
        .map(|n| n.0.clone())
        .unwrap_or_default();
    let alerts: Vec<Value> = notes
        .iter()
        .chain(app.read(cx).store.alert())
        .map(|a| json!({ "title": a.title, "message": a.message }))
        .collect();
    json!({ "windows": rows, "alerts": alerts, "theme": THEME })
}

/// `ui.click {element}`: a mouse down and up at the element's centre,
/// dispatched through the window's hit-testing as a user's click is, with
/// the button, click count and modifiers asked for. Handlers see
/// [`is_channel_click`] while it is delivered. An element partly scrolled
/// out of view is clicked at the centre of the part that shows; one
/// wholly scrolled out is refused (the click would land on whatever is
/// drawn there).
fn click(
    entry: &WindowEntry,
    id: &str,
    button: Button,
    count: u32,
    modifiers: keys::Modifiers,
    cx: &mut App,
) -> Result<Value, ControlError> {
    fresh_frame(entry, cx)?;
    let at = {
        let registry = entry.registry(cx).ok_or_else(gone)?;
        let registry = registry.borrow();
        let e = registry.resolve(id)?;
        // The centre of the part that shows: a block wider than the
        // canvas is clicked where it is drawn.
        let Some(shown) = registry.visible_part(e) else {
            return Err(ControlError::new(
                ErrorCode::Failed,
                format!("{} is scrolled out of view.", e.label),
            ));
        };
        shown.center()
    };
    click_at(entry, at, button, count, modifiers, cx)
}

/// A mouse down and up at window point `at` (top-left origin, in points),
/// through the window's hit-testing as a user's click: the element under
/// it handles it, as a real press there would. `ui.click {point}` and
/// `ui.click {element}` both end here.
fn click_at(
    entry: &WindowEntry,
    at: gpui::Point<gpui::Pixels>,
    button: Button,
    count: u32,
    modifiers: keys::Modifiers,
    cx: &mut App,
) -> Result<Value, ControlError> {
    let any: AnyWindowHandle = entry.handle;
    let count = count.max(1) as usize;
    let button = match button {
        Button::Left => MouseButton::Left,
        Button::Right => MouseButton::Right,
    };
    let modifiers = gpui_modifiers(modifiers);
    // Through the window, not its view: the click's handlers update the
    // view, which a view update would hold.
    let _channel = ChannelClick::begin();
    any.update(cx, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button,
                position: at,
                modifiers,
                click_count: count,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button,
                position: at,
                modifiers,
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

/// Draws `entry`'s window now, so its element registry and scene are this
/// moment's. The registry is filled while drawing, and a window draws only
/// on the platform's frame callbacks, which stop while the screen is
/// locked or the window is hidden. The draw marks the frame for the next
/// real frame to present. The pump calls this from its own top-level
/// update (a timer), never from inside a draw. Always drawn: the calls
/// that need it (`ui.tree`, `ui.click`, `ui.screenshot`, and `ui.key`
/// before every stroke, so the document's keyboard focus is this
/// moment's when the stroke is routed) are rare next to frames.
fn fresh_frame(entry: &WindowEntry, cx: &mut App) -> Result<(), ControlError> {
    let any: AnyWindowHandle = entry.handle;
    any.update(cx, |_, window, cx| {
        // Whole, reusing no cached view: what this tick changed may not
        // have reached the window's dirty views yet.
        window.refresh();
        window.draw(cx).clear(cx);
    })
    .map_err(|_| gone())
}

/// The channel's modifiers as GPUI's: ⌘ is `platform` on macOS and
/// Control elsewhere, as the key bindings read it (`menus::keystroke`).
fn gpui_modifiers(m: keys::Modifiers) -> Modifiers {
    let mac = cfg!(target_os = "macos");
    Modifiers {
        shift: m.shift,
        alt: m.option,
        control: m.control || (m.command && !mac),
        platform: m.command && mac,
        function: false,
    }
}

/// `ui.key`: `focus` is clicked first (a plain left click), then each
/// stroke is routed as the keyboard's: a plain key while a text field has
/// the keyboard types into it; else a key the keymap binds runs its
/// command as `command.run` would (one event). A command that answers
/// `unavailable`, `no_document` or `unknown_id` lets the key through
/// (Swift `channelDelivers`); another failure ends the call. A key that
/// runs nothing goes to the window as a key down and up (two events),
/// through GPUI's own dispatch: its key bindings, the focused element and
/// the focused text input.
fn key_strokes(
    entry: &WindowEntry,
    focus: Option<&str>,
    strokes: &[KeyStroke],
    cx: &mut App,
) -> Result<Value, ControlError> {
    if let Some(id) = focus {
        click(entry, id, Button::Left, 1, keys::Modifiers::default(), cx)?;
    }
    let mut sent: u32 = 0;
    for k in strokes {
        // Drawn first, so what the keyboard's focus is now (the List's
        // table, a field) is the document's before the stroke is routed:
        // a window that is not drawing frames (a background app on macOS)
        // would otherwise route it on a stale focus.
        fresh_frame(entry, cx)?;
        if let Some(id) = bound_command(entry, k, cx)
            && !closes_itself(entry, id)
        {
            let ran = match app_wide(id, &[], cx) {
                Some(r) => r,
                None => run_command(id, &[], entry, cx),
            };
            match ran {
                Ok(_) => {
                    sent += 1;
                    continue;
                }
                // The window's own key bindings and key handler run the
                // document and transport commands: delivering the key there
                // would run the refused command again (a second refusal in
                // the status line or an alert). Swift delivers it to the
                // first responder only, which does nothing with it; so it is
                // counted as delivered and not sent.
                Err(e) if delivers(e.code) && window_runs(id) => {
                    sent += 2;
                    continue;
                }
                Err(e) if delivers(e.code) => {}
                Err(e) => return Err(e),
            }
        }
        deliver(entry, k, cx)?;
        sent += 2;
    }
    Ok(json!({ "window": number(entry.handle), "events": sent, "x": 0.0, "y": 0.0 }))
}

/// Whether `id` is Close in a plug-in or Built-in Synth window, which
/// closes that window, not its document (its own ⌘W binding, as Swift's
/// `performClose`): the key is delivered to the window.
fn closes_itself(entry: &WindowEntry, id: &str) -> bool {
    id == "file.close" && matches!(entry.kind, WindowKind::Plugin | WindowKind::Other)
}

/// Whether a document window's key bindings or key handler run command
/// `id` (`menus::bindings` binds the document, transport and step entry
/// commands; the window's key handler steps Rewind and Fast Forward).
fn window_runs(id: &str) -> bool {
    menus::routed(id) || TRANSPORT_COMMANDS.contains(&id) || STEP_COMMANDS.contains(&id)
}

/// A command refusal after which the key still goes to the window, as
/// the keyboard passes it (Swift `KeyDispatch.channelDelivers`).
fn delivers(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::Unavailable | ErrorCode::NoDocument | ErrorCode::UnknownId
    )
}

/// The command stroke `k` runs in `entry`'s window, by the app's keymap
/// (the one the key bindings are made from): step entry's chords first
/// while it is on (the number row skipped while "reserve number row"
/// keeps it the locators'), then the global ones. A plain key (no ⌘ or
/// ⌃) while a text field has the keyboard runs none: it is typed.
fn bound_command(entry: &WindowEntry, k: &KeyStroke, cx: &App) -> Option<&'static str> {
    let s = AppState::global(cx).read(cx);
    let typing = s
        .store
        .document(entry.doc)
        .is_some_and(|d| !d.transport_keys_enabled());
    let m = k.modifiers;
    if typing && !m.command && !m.control {
        return None;
    }
    let step_entry = s
        .store
        .document(entry.doc)
        .is_some_and(|d| d.step_entry_on());
    let reserve = s.prefs.reserve_number_row();
    let mut mods = 0;
    for (on, bit) in [
        (m.shift, bindings::SHIFT),
        (m.control, bindings::CONTROL),
        (m.option, bindings::OPTION),
        (m.command, bindings::COMMAND),
    ] {
        if on {
            mods |= bit;
        }
    }
    let chord = Chord::new(k.key_code, mods)?;
    // As the keyboard: off macOS a ⌃ chord a ⌘ chord owns runs the ⌘ one.
    let chord = menus::owner(&chord, &s.keymap, cfg!(target_os = "macos")).unwrap_or(chord);
    let id = s.keymap.lookup(chord, step_entry, reserve);
    // Step entry's keys win over the List's and the Piano Roll's own
    // (Delete is Delete Last Step there too, as Swift's key monitor).
    if step_entry && id.is_some_and(|id| STEP_COMMANDS.contains(&id)) {
        return id;
    }
    // The List's table, with the keyboard, takes Return (it edits the
    // row's Position) and Delete and Forward Delete (they delete the
    // selected rows): Swift `ListEditorView.key`.
    let table = s
        .store
        .document(entry.doc)
        .is_some_and(|d| d.list_focused());
    if table && !m.command && !m.control && !m.option && matches!(k.key_code, 36 | 76 | 51 | 117) {
        return None;
    }
    // The Piano Roll's canvas, with the keyboard, takes Delete and
    // Forward Delete (they delete the selected notes): Swift
    // `PianoRollView.keyDown`.
    let roll = s
        .store
        .document(entry.doc)
        .is_some_and(|d| d.roll_focused());
    if roll && !m.command && !m.control && !m.option && matches!(k.key_code, 51 | 117) {
        return None;
    }
    id
}

/// Sends `k` into the window: a key down through GPUI's keystroke
/// dispatch (bindings, then the focused text input), then its key up.
/// One update per stroke, so what a stroke changed (a field taking the
/// keyboard) is drawn before the next.
fn deliver(entry: &WindowEntry, k: &KeyStroke, cx: &mut App) -> Result<(), ControlError> {
    let keystroke = gpui_keystroke(k);
    let any: AnyWindowHandle = entry.handle;
    any.update(cx, |_, window, cx| {
        window.dispatch_keystroke(keystroke.clone(), cx);
        window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
    })
    .map_err(|_| gone())
}

/// `k` as a GPUI keystroke: the key named as GPUI names it (from the key
/// code: the keypad's keys are the main keys', as GPUI does not tell
/// them apart), and the character it types when no ⌘ or ⌃ is held.
fn gpui_keystroke(k: &KeyStroke) -> Keystroke {
    let modifiers = gpui_modifiers(k.modifiers);
    let key = bindings::key_name(k.key_code)
        .and_then(crate::menus::gpui_key)
        .unwrap_or_else(|| k.unmodified.to_lowercase());
    let typed = !k.modifiers.command
        && !k.modifiers.control
        && k.characters
            .chars()
            .all(|c| !c.is_control() && !is_private_use(c))
        && !k.characters.is_empty();
    Keystroke {
        modifiers,
        key,
        key_char: typed.then(|| k.characters.clone()),
    }
}

/// AppKit's function-key characters (U+F700 and on) type nothing.
fn is_private_use(c: char) -> bool {
    ('\u{e000}'..='\u{f8ff}').contains(&c)
}

/// `ui.screenshot`: the window's PNG at `path` (or the channel's next
/// screenshot path), written atomically. `live` (a capture of what the
/// window server shows) is the Swift app's.
fn screenshot(
    window: Option<&WindowRef>,
    path: Option<PathBuf>,
    scale: Option<f64>,
    live: bool,
    cx: &mut App,
) -> Result<Value, ControlError> {
    if live {
        return Err(ControlError::new(
            ErrorCode::Unavailable,
            "ui.screenshot with `live`: the GPUI app draws the window itself; leave `live` out",
        ));
    }
    let entry = target(window, cx)?;
    let path = path.unwrap_or_else(temp_screenshot_path);
    fresh_frame(&entry, cx)?;
    let capture = crate::screenshot::capture_fn(cx);
    let any: AnyWindowHandle = entry.handle;
    let shot = any
        .update(cx, |_, window, _| capture(window, scale))
        .map_err(|_| gone())??;
    crate::screenshot::write_png(&shot, &path)?;
    Ok(json!({
        "window": number(entry.handle),
        "path": path.to_string_lossy(),
        "width": shot.width,
        "height": shot.height,
    }))
}

/// A new screenshot path in the temporary folder (no server to name one).
fn temp_screenshot_path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("vizdaw-screenshot-{}-{n}.png", std::process::id()))
}

/// The window `window` names (a number names a document or a Mixer
/// window; `key` and `main` the window with the keyboard); absent: the
/// front document's own window.
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
            .key_window()
            .and_then(|(doc, token)| {
                // A Mixer window that was key and closed: its document's.
                s.windows()
                    .iter()
                    .find(|w| w.token == token)
                    .or_else(|| s.window_of(doc))
            })
            .cloned()
            .ok_or_else(|| ControlError::new(ErrorCode::NoWindow, "no document window is key")),
        None => s
            .store
            .front()
            .and_then(|d| s.window_of(d))
            .or_else(|| s.document_windows().next())
            .cloned()
            .ok_or_else(|| ControlError::new(ErrorCode::NoDocument, "no document is open")),
    }
}

/// A window's number, as `state.get windows` lists it.
pub fn number(handle: impl Into<AnyWindowHandle>) -> i64 {
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

fn closed() -> ControlError {
    ControlError::new(ErrorCode::NoDocument, "the document is closed")
}

fn gone() -> ControlError {
    ControlError::new(ErrorCode::NoWindow, "the window closed")
}

fn not_yet(what: &str) -> ControlError {
    ControlError::new(
        ErrorCode::Unavailable,
        format!(
            "{what} is not available in the GPUI app yet (slice 8a answers app.new_demo, \
             app.open, app.quit, command.run file.new / file.create_demo / \
             file.create_demo_audio / file.open / file.open_copy / file.close / file.save / \
             file.save_as / file.project_settings / file.bounce / file.import_* / file.export_* / \
             file.interchange_report / media.import / media.convert / media.relink / \
             transport.* / locator.* / record.* / marker.* / midi.inject / step.* / ui.set_transport_bar / \
             ui.reset_transport_bar / edit.undo / edit.redo / track.* / editors.snap / \
             editors.list / editors.close_detail / editors.insert_event / ui.view_arrange / \
             ui.view_mix / ui.toggle_left / ui.toggle_bottom / ui.toggle_right / ui.pane / \
             automation.set_mode / automation.gesture / automation.release_all / \
             automation.mode.* / automation.suspend / mixer.new_bus / mixer.set_strip / \
             mixer.set_output / mixer.add_send / mixer.set_send / mixer.remove_send / \
             mixer.remove_bus / mixer.set_colour / mixer.*group* / mixer.*vca* / window.mixer, \
             state.get transport / windows / document / editor / selection / \
             mixer / layout / automation / media / project / record / input, ui.tree, ui.click, ui.key, ui.screenshot and wait)"
        ),
    )
}

/// Starts the server at `path` and pumps it every [`PUMP_INTERVAL`] for
/// the app's life. Quitting (any quit) stops it first: pending calls are
/// answered `shutting_down` and the last replies written.
pub fn start(path: &Path, cx: &mut App) -> Result<(), StartError> {
    let server = Rc::new(RefCell::new(Server::start(path, Config::default())?));
    eprintln!("control channel on {}", path.display());
    set_channel_on(cx);
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
