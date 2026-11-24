//! The plug-in windows (Swift `PluginWindows`, spec §9) and the Built-in
//! Synth windows: the registry, the native editor host and the windows'
//! lifecycle. The models ([`PluginWindowModel`],
//! [`BuiltinSynthWindowModel`]) hold what a window shows and make every
//! session call; this module makes the windows, hosts the editor and runs
//! the windows' tick.
//!
//! - **One window per (document, slot)** (and per (document,
//!   destination) for the Built-in Synth). A second show brings it to the
//!   front. A show is refused while that slot's close is pending, and for
//!   a slot that is gone (a stale saved window).
//! - **Registered before the editor opens:** the window is in the
//!   registry and the windows list before its editor's first open, which
//!   runs from `cx.defer` once the window exists. Later opens (an owed
//!   retry after `Busy`, the next instance after the plug-in went) run on
//!   the tick, outside every window's update.
//! - **The tick** (every [`TICK_EVERY`], while any window or pending close
//!   is left) reads a window's model again when its document or slots
//!   moved, closes the window when its slot is gone (an undo of the add
//!   included), opens the editor once the slot is live, follows the
//!   editor's own resize requests and touched parameters, and retries the
//!   closes that met `Busy`.
//! - **Close order everywhere:** the editor, then its container, then the
//!   GPUI window. The close button and the tick close through `cx.defer`
//!   (never inside the window's own update); a document close closes its
//!   windows' editors before its session drops
//!   ([`crate::AppState::close_document`]), and quit closes every editor
//!   before any session drops (`quit_now` and `on_app_quit`). A close that
//!   meets `Busy` keeps the container alive ([`EditorHost::park`]) and is
//!   retried on the tick until it succeeds; one whose document closed
//!   meanwhile keeps its container until the process ends.
//! - **Sizes are points.** The window's content is the editor plus the
//!   37-point header ([`HEADER_HEIGHT`]), or the parameter list plus the
//!   header, capped to the display's visible area less [`SCREEN_MARGIN`]
//!   (the editor is clipped, no scrolling; the plug-in is told nothing).
//!   A user resize of a resizable editor asks it for the content less the
//!   header and snaps to its answer, deferred, without re-entering the
//!   resize handler. The window is resizable only while a resizable editor
//!   shows.
//! - **Platforms:** macOS shows a container `NSView` through a
//!   `gpui::NativeView` (`macos.rs`); Linux makes a container X window
//!   inside the GPUI window and sets `WM_TRANSIENT_FOR` to the document's
//!   window (`linux.rs`). The GPUI test platform has no native window:
//!   there the editor opens with no parent view (the scripted session
//!   takes it; the real core refuses it as "no parent view").
//!
//! The windows' drawing (the header, the parameter list, the Built-in
//! Synth window's sounds) is `view.rs`'s.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod other;
mod view;
#[cfg(target_os = "linux")]
mod x11;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
use other as platform;

pub use platform::{EditorHost, Parked};
pub use view::PluginWindowView;

use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, TitlebarOptions, Window, WindowBounds, WindowOptions,
    px, size,
};
use vizdaw_app::alerts::AlertItem;
use vizdaw_app::builtin_synth_window::{self, BuiltinSynthWindowModel};
use vizdaw_app::mixer::MixerModel;
use vizdaw_app::plugin_window::{HEADER_HEIGHT, INITIAL_SIZE, MIN_CONTENT, PluginWindowModel};
use vizdaw_app::{DocId, DocumentModel, DocumentStore, WindowToken};

use crate::app_state::{AppState, FAST_POLL, WindowKind};
use crate::registry::SharedRegistry;

/// How often the windows' tick runs (Swift's 60 Hz tick).
pub const TICK_EVERY: Duration = FAST_POLL;

/// What a window's content leaves free of the display's visible area
/// (the title bar and a little room).
pub const SCREEN_MARGIN: f64 = 40.0;

/// Why there is no native host for an editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// The platform has no native window to host in (the GPUI test
    /// platform).
    Unsupported,
    /// The host could not be made; the message says why.
    Failed(String),
}

/// What a window shows: a plug-in slot's editor or parameters, or a
/// Built-in Synth destination's sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subject {
    Plugin(u64),
    BuiltinSynth(u64),
}

impl Subject {
    fn kind(self) -> WindowKind {
        match self {
            Subject::Plugin(_) => WindowKind::Plugin,
            Subject::BuiltinSynth(_) => WindowKind::Other,
        }
    }
}

enum Model {
    Plugin(Box<PluginWindowModel>),
    Synth(BuiltinSynthWindowModel),
}

/// One open window.
struct Entry {
    doc: DocId,
    subject: Subject,
    token: WindowToken,
    /// `None` until the GPUI window is made.
    handle: Option<AnyWindowHandle>,
    /// The deferred first open ran (the container is made, or there is
    /// none to make): the tick may open from now on.
    attached: bool,
    model: Model,
    /// The editor's native parent (plug-in windows on a platform that has
    /// one).
    host: Option<EditorHost>,
    /// An open was tried since the slot went live: the next is only an
    /// owed retry.
    tried_live: bool,
    /// The window scale the open editor was opened at.
    opened_scale: Option<f64>,
    /// The content size (points, uncapped) last applied to the window.
    content: Option<(f64, f64)>,
    /// The content size (points, capped) the window was last given.
    applied: Option<(f64, f64)>,
    title: String,
}

/// A plug-in window that closed while its editor could not detach
/// (`Busy`): the model retries, the container stays alive.
struct Pending {
    doc: DocId,
    slot: u64,
    model: Box<PluginWindowModel>,
    host: Option<Parked>,
}

/// The open plug-in and Built-in Synth windows, and the closes still
/// retried.
#[derive(Default)]
pub struct PluginWindows {
    entries: Vec<Entry>,
    pending: Vec<Pending>,
    /// Containers whose editor never detached before their document's
    /// session went: kept until the process ends.
    kept: Vec<Parked>,
    ticking: bool,
}

/// What one tick found for a window.
#[derive(Default)]
struct Step {
    close: bool,
    open: bool,
    resize: bool,
    title: Option<String>,
    changed: bool,
}

impl PluginWindows {
    // Reading.

    fn entry(&self, token: WindowToken) -> Option<&Entry> {
        self.entries.iter().find(|e| e.token == token)
    }

    fn entry_mut(&mut self, token: WindowToken) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.token == token)
    }

    fn find(&self, doc: DocId, subject: Subject) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.doc == doc && e.subject == subject)
    }

    /// The window of (`doc`, `subject`), once made.
    pub fn window(&self, doc: DocId, subject: Subject) -> Option<AnyWindowHandle> {
        self.find(doc, subject).and_then(|e| e.handle)
    }

    /// Whether (`doc`, `subject`) has a window.
    pub fn is_open(&self, doc: DocId, subject: Subject) -> bool {
        self.find(doc, subject).is_some()
    }

    /// The number of open windows.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The plug-in window model of (`doc`, `slot`).
    pub fn plugin_model(&self, doc: DocId, slot: u64) -> Option<&PluginWindowModel> {
        match &self.find(doc, Subject::Plugin(slot))?.model {
            Model::Plugin(m) => Some(m),
            Model::Synth(_) => None,
        }
    }

    /// The Built-in Synth window model of (`doc`, `dest`).
    pub fn synth_model(&self, doc: DocId, dest: u64) -> Option<&BuiltinSynthWindowModel> {
        match &self.find(doc, Subject::BuiltinSynth(dest))?.model {
            Model::Synth(m) => Some(m),
            Model::Plugin(_) => None,
        }
    }

    /// Whether (`doc`, `slot`)'s window closed while its editor could not
    /// detach yet: a new window waits until it has.
    pub fn is_close_pending(&self, doc: DocId, slot: u64) -> bool {
        self.pending.iter().any(|p| p.doc == doc && p.slot == slot)
    }

    /// The closes still retried.
    pub fn pending_closes(&self) -> usize {
        self.pending.len()
    }

    /// The window `token`'s title.
    pub fn title(&self, token: WindowToken) -> Option<String> {
        self.entry(token).map(|e| e.title.clone())
    }

    /// The documents with a window.
    pub(crate) fn documents(&self) -> Vec<DocId> {
        let mut docs: Vec<DocId> = self.entries.iter().map(|e| e.doc).collect();
        docs.extend(self.pending.iter().map(|p| p.doc));
        docs.sort();
        docs.dedup();
        docs
    }

    /// The content size the window `token` should have (points, uncapped)
    /// and whether the user may resize it.
    fn wanted(&self, token: WindowToken) -> Option<((f64, f64), bool)> {
        let e = self.entry(token)?;
        Some(match &e.model {
            Model::Plugin(m) => (
                m.content_size(),
                m.editor_size().is_some_and(|s| s.resizable),
            ),
            Model::Synth(_) => (builtin_synth_window::SIZE, false),
        })
    }

    /// Runs `f` with window `token`'s plug-in model and its document.
    fn with_plugin<R>(
        &mut self,
        token: WindowToken,
        store: &mut DocumentStore,
        f: impl FnOnce(&mut PluginWindowModel, &mut DocumentModel) -> R,
    ) -> Option<R> {
        let e = self.entries.iter_mut().find(|e| e.token == token)?;
        let Model::Plugin(m) = &mut e.model else {
            return None;
        };
        let d = store.document_mut(e.doc)?;
        Some(f(m, d))
    }

    /// Runs `f` with window `token`'s Built-in Synth model and its
    /// document.
    fn with_synth<R>(
        &mut self,
        token: WindowToken,
        store: &mut DocumentStore,
        f: impl FnOnce(&mut BuiltinSynthWindowModel, &mut DocumentModel) -> R,
    ) -> Option<R> {
        let e = self.entries.iter_mut().find(|e| e.token == token)?;
        let Model::Synth(m) = &mut e.model else {
            return None;
        };
        let d = store.document_mut(e.doc)?;
        Some(f(m, d))
    }

    // Registering and closing.

    /// Registers (`doc`, `subject`)'s window as `token`, its model read at
    /// once; false (nothing registered) for a slot or destination that is
    /// gone, a slot whose close is pending, or a closed document.
    fn register(
        &mut self,
        doc: DocId,
        subject: Subject,
        token: WindowToken,
        store: &mut DocumentStore,
    ) -> bool {
        let Some(d) = store.document_mut(doc) else {
            return false;
        };
        let model = match subject {
            Subject::Plugin(slot) => {
                if self.is_close_pending(doc, slot) || !slot_exists(d, slot) {
                    return false;
                }
                Model::Plugin(Box::new(PluginWindowModel::new(d, slot)))
            }
            Subject::BuiltinSynth(dest) => {
                if !synth_exists(d, dest) {
                    return false;
                }
                Model::Synth(BuiltinSynthWindowModel::new(d, dest))
            }
        };
        let title = match &model {
            Model::Plugin(m) => m.title(),
            Model::Synth(_) => builtin_synth_window::TITLE.to_owned(),
        };
        self.entries.push(Entry {
            doc,
            subject,
            token,
            handle: None,
            attached: false,
            model,
            host: None,
            tried_live: false,
            opened_scale: None,
            content: None,
            applied: None,
            title,
        });
        true
    }

    /// Removes window `token`'s entry and closes it: a plug-in window's
    /// editor detaches (one try), then its container goes; on `Busy` the
    /// container is kept and the close retried on the tick. Returns the
    /// GPUI window, for the caller to remove last.
    fn close(&mut self, token: WindowToken, store: &mut DocumentStore) -> Option<AnyWindowHandle> {
        let i = self.entries.iter().position(|e| e.token == token)?;
        let e = self.entries.remove(i);
        let handle = e.handle;
        let Model::Plugin(mut model) = e.model else {
            return handle;
        };
        let done = match store.document_mut(e.doc) {
            Some(d) => model.closed(d, Instant::now()),
            // No session: nothing can detach the editor now.
            None => false,
        };
        match (done, e.host) {
            (true, Some(host)) => host.destroy(),
            (true, None) => {}
            (false, host) => {
                let host = host.map(EditorHost::park);
                if store.document(e.doc).is_some() {
                    self.pending.push(Pending {
                        doc: e.doc,
                        slot: model.slot(),
                        model,
                        host,
                    });
                } else {
                    self.kept.extend(host);
                }
            }
        }
        handle
    }

    /// `doc` is closing: each of its windows closes (one try each, before
    /// its session drops). Containers whose editor could not detach are
    /// kept until the process ends, as are those of `doc`'s pending
    /// closes. Returns the GPUI windows, which close themselves when they
    /// see their entries gone.
    pub(crate) fn close_document(
        &mut self,
        doc: DocId,
        store: &mut DocumentStore,
    ) -> Vec<AnyWindowHandle> {
        let tokens: Vec<WindowToken> = self
            .entries
            .iter()
            .filter(|e| e.doc == doc)
            .map(|e| e.token)
            .collect();
        let mut windows = vec![];
        for t in tokens {
            windows.extend(self.close(t, store));
        }
        let (gone, kept): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| p.doc == doc);
        self.pending = kept;
        for p in gone {
            self.kept.extend(p.host);
        }
        windows
    }

    /// Retries every close that met `Busy`; a container goes once its
    /// editor has detached.
    fn retry_pending(&mut self, store: &mut DocumentStore) {
        let mut still = vec![];
        for mut p in std::mem::take(&mut self.pending) {
            match store.document(p.doc) {
                Some(d) => {
                    if p.model.retry_close(d) {
                        if let Some(h) = p.host {
                            h.destroy();
                        }
                    } else {
                        still.push(p);
                    }
                }
                None => self.kept.extend(p.host),
            }
        }
        self.pending = still;
    }

    // The tick.

    /// What window `token` needs now: read again, close, open, resize,
    /// a new title.
    fn step(&mut self, token: WindowToken, store: &mut DocumentStore) -> Step {
        let mut step = Step::default();
        let Some(e) = self.entries.iter_mut().find(|e| e.token == token) else {
            return step;
        };
        let Some(d) = store.document_mut(e.doc) else {
            step.close = true;
            return step;
        };
        match &mut e.model {
            Model::Plugin(m) => {
                if m.needs_refresh(d) {
                    step.changed = true;
                    // Only a good read without the slot makes it gone.
                    if m.refresh(d) && m.is_gone() {
                        step.close = true;
                        return step;
                    }
                }
                if !m.is_live() {
                    // The next instance's editor opens once it runs.
                    e.tried_live = false;
                    if m.editor_problem().is_none() {
                        // Not running yet: the problem text says why.
                        m.open_editor(d, 0, 1.0);
                        step.changed = true;
                    }
                } else if m.editor_size().is_none() && (!e.tried_live || m.open_owed()) {
                    step.open = e.attached;
                }
                if m.take_editor_resize(d).is_some() {
                    step.changed = true;
                }
                m.follow_core_touches(d);
                let title = m.title();
                if title != e.title {
                    e.title = title.clone();
                    step.title = Some(title);
                }
            }
            Model::Synth(m) => {
                if m.needs_refresh(d) {
                    m.refresh(d);
                    step.changed = true;
                }
            }
        }
        step.resize = self
            .wanted(token)
            .is_some_and(|(content, _)| Some(content) != self.entry(token).and_then(|e| e.content));
        step
    }

    /// Opens window `token`'s editor in its container at `scale`; true
    /// when it opened.
    fn open_editor(&mut self, token: WindowToken, scale: f64, store: &mut DocumentStore) -> bool {
        let Some(e) = self.entry_mut(token) else {
            return false;
        };
        let Some(d) = store.document_mut(e.doc) else {
            return false;
        };
        let Model::Plugin(m) = &mut e.model else {
            return false;
        };
        let parent = e.host.as_ref().map_or(0, EditorHost::parent);
        if m.is_live() {
            e.tried_live = true;
        }
        let opened = m.open_editor(d, parent, scale);
        e.opened_scale = opened.then_some(scale);
        opened
    }

    /// The window's scale moved off the one its open editor was opened at:
    /// the editor closes, to open again at the new scale (a scale applies
    /// at open). False while the close meets `Busy` (tried again next
    /// tick).
    fn close_for_rescale(&mut self, token: WindowToken, scale: f64, store: &DocumentStore) -> bool {
        let Some(e) = self.entry_mut(token) else {
            return false;
        };
        let (Some(was), Model::Plugin(m)) = (e.opened_scale, &e.model) else {
            return false;
        };
        if was == scale || m.editor_size().is_none() {
            return false;
        }
        let Some(d) = store.document(e.doc) else {
            return false;
        };
        if d.session().close_editor(m.slot()).is_err() {
            return false;
        }
        e.opened_scale = None;
        true
    }

    /// The user resized window `token` to `content` (points): a resizable
    /// editor is asked for it less the header. True when the window must
    /// take the editor's answer.
    fn user_resize(
        &mut self,
        token: WindowToken,
        content: (f64, f64),
        store: &mut DocumentStore,
    ) -> bool {
        let Some(e) = self.entry_mut(token) else {
            return false;
        };
        if e.applied.is_some_and(|a| near(a, content)) {
            // The window took the size it was given.
            return false;
        }
        let Model::Plugin(m) = &mut e.model else {
            return false;
        };
        if !m.editor_size().is_some_and(|s| s.resizable) {
            return false;
        }
        let Some(d) = store.document(e.doc) else {
            return false;
        };
        let width = content.0.max(1.0).round() as u32;
        let height = (content.1 - HEADER_HEIGHT).max(1.0).round() as u32;
        m.resize(d, width, height);
        // Snap to the answer, or back to the size it kept.
        e.content = None;
        true
    }
}

/// Whether two sizes are the same to half a point.
fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 0.5 && (a.1 - b.1).abs() < 0.5
}

/// Whether `d` has slot `slot`; a refused read lets the window open (its
/// tick reads again).
fn slot_exists(d: &DocumentModel, slot: u64) -> bool {
    match d.session().mixer() {
        Ok(m) => m
            .strips
            .iter()
            .any(|s| MixerModel::slots(s).any(|x| x.id == slot)),
        Err(_) => true,
    }
}

/// Whether `d` has a Built-in Synth destination `dest`.
fn synth_exists(d: &DocumentModel, dest: u64) -> bool {
    match d.session().destinations() {
        Ok(ds) => ds
            .iter()
            .any(|x| x.id == dest && builtin_synth_window::is_builtin_synth(x)),
        Err(_) => true,
    }
}

/// `content` (points) capped to `visible` (the display's visible size)
/// less [`SCREEN_MARGIN`], never under the least content size.
pub fn capped(content: (f64, f64), visible: Option<(f64, f64)>) -> (f64, f64) {
    let Some((vw, vh)) = visible else {
        return content;
    };
    let most = (
        (vw - SCREEN_MARGIN).max(MIN_CONTENT.0),
        (vh - SCREEN_MARGIN).max(MIN_CONTENT.1),
    );
    (content.0.min(most.0), content.1.min(most.1))
}

// Showing.

/// Opens slot `slot`'s plug-in window in `doc`, or brings the open one to
/// the front. `None`: the slot is gone, its last window's close is still
/// pending, or the window could not be made (the alert "Window Failed").
/// Call it outside `AppState::update`.
pub fn show_plugin_window(doc: DocId, slot: u64, cx: &mut App) -> Option<AnyWindowHandle> {
    show(doc, Subject::Plugin(slot), cx)
}

/// Opens destination `dest`'s Built-in Synth window in `doc`, or brings
/// the open one to the front; `None` for a destination that is not a
/// Built-in Synth.
pub fn show_builtin_synth_window(doc: DocId, dest: u64, cx: &mut App) -> Option<AnyWindowHandle> {
    show(doc, Subject::BuiltinSynth(dest), cx)
}

/// Opens (`doc`, `subject`)'s window, or brings it to the front, on a
/// later turn of the main loop: what a gesture or a menu item calls, never
/// inside the event's own dispatch or the platform's callback.
pub fn show_later(doc: DocId, subject: Subject, cx: &App) {
    later(cx, move |cx| {
        show(doc, subject, cx);
    });
}

fn show(doc: DocId, subject: Subject, cx: &mut App) -> Option<AnyWindowHandle> {
    let app = AppState::global(cx);
    if let Some(h) = app.read(cx).plugin_windows.window(doc, subject) {
        h.update(cx, |_, window, _| window.activate_window()).ok();
        return Some(h);
    }
    if app.read(cx).plugin_windows.is_open(doc, subject) {
        // Registered, its window being made.
        return None;
    }
    let token = app.update(cx, |s, _| s.new_token());
    let registered = app.update(cx, |s, _| {
        s.plugin_windows.register(doc, subject, token, &mut s.store)
    });
    if !registered {
        return None;
    }
    let title = app.read(cx).plugin_windows.title(token).unwrap_or_default();
    let initial = match subject {
        Subject::Plugin(_) => INITIAL_SIZE,
        Subject::BuiltinSynth(_) => builtin_synth_window::SIZE,
    };
    let bounds = Bounds::centered(None, size(px(initial.0 as f32), px(initial.1 as f32)), cx);
    let state = app.clone();
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(title.into()),
                ..Default::default()
            }),
            kind: gpui::WindowKind::Normal,
            is_resizable: false,
            is_minimizable: true,
            window_min_size: Some(size(px(MIN_CONTENT.0 as f32), px(MIN_CONTENT.1 as f32))),
            ..Default::default()
        },
        move |window, cx| {
            cx.new(|cx| PluginWindowView::new(state, doc, subject, token, window, cx))
        },
    );
    let handle: AnyWindowHandle = match opened {
        Ok(h) => h.into(),
        Err(e) => {
            app.update(cx, |s, cx| {
                s.plugin_windows.close(token, &mut s.store);
                s.store
                    .post_alert(AlertItem::new("Window Failed", e.to_string()));
                cx.notify();
            });
            crate::alerts::present(cx);
            return None;
        }
    };
    app.update(cx, |s, cx| {
        if let Some(e) = s.plugin_windows.entry_mut(token) {
            e.handle = Some(handle);
            if let (Model::Plugin(m), Some(d)) = (&mut e.model, s.store.document(doc)) {
                // The project reopens it.
                m.opened(d);
            }
        }
        s.add_window(doc, token, handle, subject.kind());
        cx.notify();
    });
    ensure_ticking(cx);
    match subject {
        Subject::Plugin(_) => cx.defer(move |cx| attach(token, cx)),
        Subject::BuiltinSynth(_) => {
            handle
                .update(cx, |_, window, cx| apply_size(token, window, cx))
                .ok();
        }
    }
    Some(handle)
}

/// The windows `doc`'s project had open when it was saved, opened again
/// (those whose slot is gone are left out).
pub(crate) fn reopen_saved(doc: DocId, cx: &mut App) {
    let slots = AppState::global(cx)
        .read(cx)
        .store
        .document(doc)
        .and_then(|d| d.session().open_windows().ok())
        .unwrap_or_default();
    for slot in slots {
        show_plugin_window(doc, slot, cx);
    }
}

/// The deferred first open: the container is made in the window (Linux:
/// the window becomes transient for its document's), then the editor
/// opens in it.
fn attach(token: WindowToken, cx: &mut App) {
    let app = AppState::global(cx);
    let Some((handle, doc)) = app
        .read(cx)
        .plugin_windows
        .entry(token)
        .and_then(|e| e.handle.map(|h| (h, e.doc)))
    else {
        // Closed before the open ran.
        return;
    };
    let owner = app
        .read(cx)
        .window_of(doc)
        .map(|w| w.handle)
        .and_then(|h| {
            h.update(cx, |_, window, _| platform::window_id(window))
                .ok()
        })
        .flatten();
    let host = handle.update(cx, |_, window, cx| {
        if let Some(owner) = owner {
            platform::set_transient_for(window, owner);
        }
        EditorHost::create(window, cx)
    });
    let host = match host {
        Ok(Ok(h)) => Some(h),
        Ok(Err(HostError::Unsupported)) => None,
        Ok(Err(HostError::Failed(why))) => {
            eprintln!("plug-in window: no editor container: {why}");
            None
        }
        Err(_) => return,
    };
    app.update(cx, |s, _| {
        if let Some(e) = s.plugin_windows.entry_mut(token) {
            e.host = host;
            e.attached = true;
        }
    });
    open_now(token, cx);
}

/// Opens window `token`'s editor now, outside every window's update (the
/// plug-in may do anything while it attaches), then sizes the window.
fn open_now(token: WindowToken, cx: &mut App) {
    let app = AppState::global(cx);
    let Some(handle) = app
        .read(cx)
        .plugin_windows
        .entry(token)
        .and_then(|e| e.handle)
    else {
        return;
    };
    let Ok(scale) = handle.update(cx, |_, window, _| f64::from(window.scale_factor())) else {
        return;
    };
    app.update(cx, |s, cx| {
        s.plugin_windows.open_editor(token, scale, &mut s.store);
        cx.notify();
    });
    handle
        .update(cx, |_, window, cx| apply_size(token, window, cx))
        .ok();
}

/// Gives window `token` the content size its model wants, capped to the
/// screen, places the container at the editor's size, and lets the user
/// resize it only while a resizable editor shows.
fn apply_size(token: WindowToken, window: &mut Window, cx: &mut App) {
    let app = AppState::global(cx);
    let Some((content, resizable)) = app.read(cx).plugin_windows.wanted(token) else {
        return;
    };
    let visible = window.display(cx).map(|d| {
        let s = d.visible_bounds().size;
        (
            f64::from(f32::from(s.width)),
            f64::from(f32::from(s.height)),
        )
    });
    let shown = capped(content, visible);
    app.update(cx, |s, _| {
        if let Some(e) = s.plugin_windows.entry_mut(token) {
            e.content = Some(content);
            e.applied = Some(shown);
            if let (Some(host), Model::Plugin(m)) = (&e.host, &e.model)
                && let Some(ed) = m.editor_size()
            {
                host.set_frame(
                    window,
                    HEADER_HEIGHT,
                    (f64::from(ed.width), f64::from(ed.height)),
                );
            }
        }
    });
    platform::set_resizable(window, resizable, shown);
    if !near(
        (
            f64::from(f32::from(window.viewport_size().width)),
            f64::from(f32::from(window.viewport_size().height)),
        ),
        shown,
    ) {
        window.resize(size(px(shown.0 as f32), px(shown.1 as f32)));
    }
    window.refresh();
}

/// Closes window `token`: the editor detaches (or its close is kept
/// pending), its container goes, then the GPUI window.
pub(crate) fn close(token: WindowToken, cx: &mut App) {
    let app = AppState::global(cx);
    let handle = app.update(cx, |s, cx| {
        let h = s.plugin_windows.close(token, &mut s.store);
        s.remove_window(token);
        cx.notify();
        h
    });
    if let Some(h) = handle {
        h.update(cx, |_, window, _| window.remove_window()).ok();
    }
}

/// Closes (`doc`, `subject`)'s window as its close button does; false
/// when it has none.
pub fn close_window(doc: DocId, subject: Subject, cx: &mut App) -> bool {
    let token = AppState::global(cx)
        .read(cx)
        .plugin_windows
        .find(doc, subject)
        .map(|e| e.token);
    match token {
        Some(t) => {
            close(t, cx);
            true
        }
        None => false,
    }
}

/// Starts the windows' tick, unless it runs; it stops once no window and
/// no pending close is left.
fn ensure_ticking(cx: &mut App) {
    let app = AppState::global(cx);
    if app.read(cx).plugin_windows.ticking {
        return;
    }
    app.update(cx, |s, _| s.plugin_windows.ticking = true);
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(TICK_EVERY).await;
            if !cx.update(tick) {
                break;
            }
        }
    })
    .detach();
}

/// One pass of the windows' tick; false once nothing is left to tick (the
/// tick has stopped).
pub fn tick(cx: &mut App) -> bool {
    let app = AppState::global(cx);
    app.update(cx, |s, _| s.plugin_windows.retry_pending(&mut s.store));
    let tokens: Vec<WindowToken> = app
        .read(cx)
        .plugin_windows
        .entries
        .iter()
        .map(|e| e.token)
        .collect();
    for token in tokens {
        let step = app.update(cx, |s, cx| {
            let step = s.plugin_windows.step(token, &mut s.store);
            if step.changed {
                cx.notify();
            }
            step
        });
        if step.close {
            close(token, cx);
            continue;
        }
        let Some(handle) = app
            .read(cx)
            .plugin_windows
            .entry(token)
            .and_then(|e| e.handle)
        else {
            continue;
        };
        if let Some(title) = &step.title {
            handle
                .update(cx, |_, window, _| window.set_window_title(title))
                .ok();
        }
        // Linux tells the plug-in its scale at open: a new scale reopens.
        if cfg!(target_os = "linux")
            && let Ok(scale) = handle.update(cx, |_, w, _| f64::from(w.scale_factor()))
            && app.update(cx, |s, _| {
                s.plugin_windows.close_for_rescale(token, scale, &s.store)
            })
        {
            open_now(token, cx);
            continue;
        }
        if step.open {
            open_now(token, cx);
        } else if step.resize {
            handle
                .update(cx, |_, window, cx| apply_size(token, window, cx))
                .ok();
        }
    }
    let s = app.read(cx);
    let more = !s.plugin_windows.entries.is_empty() || !s.plugin_windows.pending.is_empty();
    if !more {
        app.update(cx, |s, _| s.plugin_windows.ticking = false);
    }
    more
}

/// Quitting by any path (the app menu, the system) closes every editor
/// first, before any session drops.
pub(crate) fn install(cx: &mut App) {
    cx.on_app_quit(|cx| {
        if let Some(app) = crate::app_state::try_global(cx) {
            app.update(cx, |s, cx| {
                s.close_plugin_windows();
                cx.notify();
            });
        }
        async {}
    })
    .detach();
}

/// The registry of a plug-in or Built-in Synth window.
pub(crate) fn registry(handle: AnyWindowHandle, cx: &App) -> Option<SharedRegistry> {
    let h = handle.downcast::<PluginWindowView>()?;
    h.read(cx).ok().map(|v| v.registry().clone())
}

/// Runs `f` on a later turn of the main loop (a task), never inside the
/// platform callback that is running now.
fn later(cx: &App, f: impl FnOnce(&mut App) + 'static) {
    cx.spawn(async move |cx| cx.update(f)).detach();
}

/// The user resized window `token` to `content`: a resizable editor takes
/// it, and the window snaps to its answer.
fn user_resized(token: WindowToken, content: (f64, f64), cx: &mut App) {
    let app = AppState::global(cx);
    let asked = app.update(cx, |s, _| {
        s.plugin_windows.user_resize(token, content, &mut s.store)
    });
    if !asked {
        return;
    }
    if let Some(h) = app
        .read(cx)
        .plugin_windows
        .entry(token)
        .and_then(|e| e.handle)
    {
        h.update(cx, |_, window, cx| apply_size(token, window, cx))
            .ok();
    }
}
