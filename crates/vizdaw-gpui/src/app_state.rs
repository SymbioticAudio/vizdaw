//! The app's state: the document store, one transport per document and the
//! open windows. One `Entity<AppState>` per app, reachable through
//! [`AppState::global`]; windows observe it and redraw when it notifies.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use core_control::bindings::Keymap;
use gpui::{App, AppContext, Entity, Global, WindowHandle};
use vizdaw_app::alerts::AlertItem;
use vizdaw_app::transport::{PollResult, TransportModel, TransportReadout};
use vizdaw_app::transport_bar::TransportLayout;
use vizdaw_app::{DocId, DocumentModel, DocumentStore, Prefs, SaveAction, WindowToken};

use crate::shell::DocumentWindow;

/// What a window is, as `state.get windows` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowKind {
    /// A document window.
    #[default]
    Document,
    /// A document's Mixer window: its own transport bar (without the
    /// document-only items) over the mixer. At most one per document.
    Mixer,
}

impl WindowKind {
    /// The channel's name (`document`, `mixer`).
    pub fn name(self) -> &'static str {
        match self {
            WindowKind::Document => "document",
            WindowKind::Mixer => "mixer",
        }
    }
}

/// One open window: a document's own window, or its Mixer window. Both
/// are [`DocumentWindow`]s; `kind` says which.
#[derive(Clone)]
pub struct WindowEntry {
    pub doc: DocId,
    pub token: WindowToken,
    pub handle: WindowHandle<DocumentWindow>,
    pub kind: WindowKind,
}

pub struct AppState {
    pub store: DocumentStore,
    /// The keys of every command (the registry's defaults for now).
    pub keymap: Keymap,
    /// The app's preferences: every document window draws its transport
    /// bar from them, so a change redraws them all. In memory until
    /// [`crate::run`] opens the launch home's.
    pub prefs: Prefs,
    /// Each document's transport, built from that document and only ever
    /// called with it.
    transports: HashMap<DocId, TransportModel>,
    windows: Vec<WindowEntry>,
    next_token: u64,
    /// When each document's session last had its idle tick.
    last_idle: HashMap<DocId, Instant>,
}

/// The poll's interval while a retry waits on Busy (Swift's 60 Hz tick).
pub const FAST_POLL: Duration = Duration::from_millis(16);

/// The least time between two idle ticks of one document.
const IDLE_EVERY: Duration = Duration::from_millis(50);

struct GlobalAppState(Entity<AppState>);

impl Global for GlobalAppState {}

impl AppState {
    pub fn new(store: DocumentStore) -> Self {
        AppState {
            store,
            keymap: Keymap::default(),
            prefs: Prefs::in_memory(),
            transports: HashMap::new(),
            windows: Vec::new(),
            next_token: 1,
            last_idle: HashMap::new(),
        }
    }

    /// The app's state; panics before [`crate::install`].
    pub fn global(cx: &App) -> Entity<AppState> {
        cx.global::<GlobalAppState>().0.clone()
    }

    pub(crate) fn set_global(state: Entity<AppState>, cx: &mut App) {
        cx.set_global(GlobalAppState(state));
    }

    /// Takes `prefs` as the app's preferences; new projects are made in
    /// their default format.
    pub fn set_prefs(&mut self, prefs: Prefs) {
        self.store.set_default_format(prefs.default_format());
        self.prefs = prefs;
    }

    /// File › New's Create: `format` is the default from now on, and is
    /// remembered in the preferences (a failed write keeps it for this
    /// run).
    pub fn remember_default_format(&mut self, format: vizdaw_ffi::ProjectFormatRow) {
        self.store.set_default_format(format);
        let _ = self.prefs.set_default_format(format);
    }

    /// The transport bar every document window draws.
    pub fn transport_layout(&self) -> TransportLayout {
        self.prefs.transport_bar()
    }

    pub fn windows(&self) -> &[WindowEntry] {
        &self.windows
    }

    /// `doc`'s own window.
    pub fn window_of(&self, doc: DocId) -> Option<&WindowEntry> {
        self.windows
            .iter()
            .find(|w| w.doc == doc && w.kind == WindowKind::Document)
    }

    /// `doc`'s Mixer window, while it is open.
    pub fn mixer_window_of(&self, doc: DocId) -> Option<&WindowEntry> {
        self.windows
            .iter()
            .find(|w| w.doc == doc && w.kind == WindowKind::Mixer)
    }

    /// The document windows (not the Mixer windows), one per document.
    pub fn document_windows(&self) -> impl Iterator<Item = &WindowEntry> {
        self.windows
            .iter()
            .filter(|w| w.kind == WindowKind::Document)
    }

    /// What window `w`'s title bar says: the document's title (the
    /// window marks an edit itself), or "Mixer — <title>".
    pub fn window_title(&self, w: &WindowEntry) -> Option<String> {
        match w.kind {
            WindowKind::Document => self.title(w.doc),
            WindowKind::Mixer => self.store.document(w.doc).map(mixer_title),
        }
    }

    /// Whether `w` is the key window.
    pub fn is_key(&self, w: &WindowEntry) -> bool {
        self.store.key_window() == Some((w.doc, w.token))
    }

    pub fn transport(&self, doc: DocId) -> Option<&TransportModel> {
        self.transports.get(&doc)
    }

    /// Builds `doc`'s transport. A failure is an alert and the document
    /// closes.
    pub(crate) fn attach(&mut self, doc: DocId) -> bool {
        let built = match self.store.document(doc) {
            Some(d) => TransportModel::new(d),
            None => return false,
        };
        match built {
            Ok(t) => {
                self.transports.insert(doc, t);
                // The app's list columns and default filter.
                let prefs = self.prefs.list();
                self.store
                    .with_document(doc, |d, _| d.set_list_prefs(prefs));
                true
            }
            Err(e) => {
                self.store
                    .post_alert(AlertItem::from_error(&e, "Transport Failed"));
                self.store.close(doc);
                false
            }
        }
    }

    pub(crate) fn new_token(&mut self) -> WindowToken {
        let token = WindowToken(self.next_token);
        self.next_token += 1;
        token
    }

    pub(crate) fn add_window(
        &mut self,
        doc: DocId,
        token: WindowToken,
        handle: WindowHandle<DocumentWindow>,
        kind: WindowKind,
    ) {
        self.windows.push(WindowEntry {
            doc,
            token,
            handle,
            kind,
        });
    }

    /// The window `token` went (a Mixer window closing; its document
    /// stays).
    pub(crate) fn remove_window(&mut self, token: WindowToken) {
        self.windows.retain(|w| w.token != token);
    }

    /// The list preferences changed: every open document takes them.
    pub fn push_list_prefs(&mut self) {
        let prefs = self.prefs.list();
        let docs: Vec<DocId> = self.document_windows().map(|w| w.doc).collect();
        for doc in docs {
            self.store
                .with_document(doc, |d, _| d.set_list_prefs(prefs.clone()));
        }
    }

    /// `doc`'s window went: the document and its transport go too, and
    /// its Mixer window's entry (the window closes itself when it sees
    /// its document gone).
    pub fn close_document(&mut self, doc: DocId) {
        self.windows.retain(|w| w.doc != doc);
        self.transports.remove(&doc);
        self.store.close(doc);
    }

    /// Every document closes, with its transport and window entry.
    pub fn close_all(&mut self) {
        let docs: Vec<DocId> = self.document_windows().map(|w| w.doc).collect();
        for doc in docs {
            self.close_document(doc);
        }
        self.transports.clear();
        self.store.close_all();
    }

    /// Plays or stops `doc`'s transport.
    pub fn play_stop(&mut self, doc: DocId) {
        let Some(t) = self.transports.get_mut(&doc) else {
            return;
        };
        self.store
            .with_document(doc, |d, store| t.play_stop(d, store));
    }

    /// Stops and goes back to where playing started; nothing while stopped.
    /// A refusal is an alert (there is no status line this slice).
    pub fn stop_and_return(&mut self, doc: DocId) {
        let Some(t) = self.transports.get_mut(&doc) else {
            return;
        };
        if !t.is_playing() {
            return;
        }
        if let Err(e) = t.stop_and_return(&mut self.store) {
            self.store
                .post_alert(AlertItem::from_error(&e, "Stop Failed"));
        }
    }

    /// Polls `doc`'s transport; true when what the window shows changed.
    pub fn poll(&mut self, doc: DocId) -> bool {
        let idled = self.idle(doc);
        let Some(t) = self.transports.get_mut(&doc) else {
            return idled;
        };
        // The playhead draws the tick and sequence too.
        let seen = |t: &TransportModel, store: &DocumentStore| {
            let s = t.state();
            (t.readout(None), s.tick, s.seq, store.alert().map(|a| a.id))
        };
        let before = seen(t, &self.store);
        let result = t.poll(&mut self.store);
        let after = seen(t, &self.store);
        idled || result == PollResult::Failed || before != after
    }

    /// The session's idle tick, at most every 50 ms; true when a new
    /// snapshot arrived (the title may have changed).
    fn idle(&mut self, doc: DocId) -> bool {
        let now = Instant::now();
        // A retry waiting on Busy runs every poll.
        let fast = self
            .store
            .document(doc)
            .is_some_and(DocumentModel::wants_fast_poll);
        if !fast
            && self
                .last_idle
                .get(&doc)
                .is_some_and(|t| now.duration_since(*t) < IDLE_EVERY)
        {
            return false;
        }
        self.last_idle.insert(doc, now);
        self.store
            .with_document(doc, |d, _| d.idle())
            .unwrap_or(false)
    }

    /// How long until `doc`'s next poll; `None` once it is closed. While
    /// a note-off or a drag's final edit waits on Busy, at most
    /// [`FAST_POLL`].
    pub fn poll_interval(&self, doc: DocId) -> Option<std::time::Duration> {
        let fast = self
            .store
            .document(doc)
            .is_some_and(DocumentModel::wants_fast_poll);
        self.transports.get(&doc).map(|t| {
            let every = t.poll_interval();
            if fast { every.min(FAST_POLL) } else { every }
        })
    }

    /// What `doc`'s transport bar shows; tempo and signature come from the
    /// sequence info.
    pub fn readout(&self, doc: DocId) -> Option<TransportReadout> {
        let t = self.transports.get(&doc)?;
        Some(match self.store.document(doc) {
            Some(d) => t.readout_of(d),
            None => t.readout(None),
        })
    }

    /// Whether Play does anything for `doc`.
    pub fn can_play(&self, doc: DocId) -> bool {
        match (self.transports.get(&doc), self.store.document(doc)) {
            (Some(t), Some(d)) => t.can_play(d),
            _ => false,
        }
    }

    pub fn title(&self, doc: DocId) -> Option<String> {
        self.store.document(doc).map(|d| d.title())
    }

    /// Whether closing or quitting must ask first: the session's live
    /// answer, not the snapshot's flag.
    pub fn has_unsaved_changes(&self, doc: DocId) -> bool {
        self.store
            .document(doc)
            .is_some_and(|d| d.has_unsaved_changes())
    }

    /// The snapshot's edited flag (the window title mark).
    pub fn is_dirty(&self, doc: DocId) -> bool {
        self.store.document(doc).is_some_and(|d| d.snapshot().dirty)
    }

    /// File › Save. `NeedsSaveAs` means the caller asks for a location;
    /// a failure is an alert.
    pub fn save(&mut self, doc: DocId) -> SaveAction {
        self.store
            .with_document(doc, |d, _| d.save())
            .unwrap_or_else(|| SaveAction::Failed("the document is closed".into()))
    }

    /// File › Save As to `path`, with the `.vizdaw` extension added when it
    /// is missing. A failure is an alert.
    pub fn save_as(&mut self, doc: DocId, path: &Path) -> Result<(), String> {
        let path = with_extension(path);
        self.store
            .with_document(doc, |d, _| d.save_as(&path))
            .unwrap_or_else(|| Err("the document is closed".into()))
    }

    /// Where Save As starts: the document's folder, else the home folder.
    pub fn save_directory(&self, doc: DocId) -> PathBuf {
        self.store
            .document(doc)
            .and_then(|d| d.path())
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(home)
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    /// The title Save As names the file after ("Untitled" when empty).
    pub fn save_title(&self, doc: DocId) -> String {
        let title = self
            .store
            .document(doc)
            .map(|d| d.snapshot().title.clone())
            .unwrap_or_default();
        if title.is_empty() {
            "Untitled".to_owned()
        } else {
            title
        }
    }
}

/// A Mixer window's title (Swift `MixerWindow`): "Mixer — <title>".
pub fn mixer_title(d: &DocumentModel) -> String {
    format!("Mixer \u{2014} {}", d.snapshot().title)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// `path`, ending in `.vizdaw` (any case, as the save panel's check:
/// `vizdaw_app::panels::save_target`).
pub fn with_extension(path: &Path) -> PathBuf {
    let (path, _) =
        vizdaw_app::formats::with_extension(&path.to_string_lossy(), "vizdaw", &["vizdaw"]);
    PathBuf::from(path)
}

/// Creates the app state and makes it reachable through [`AppState::global`].
pub(crate) fn create(store: DocumentStore, cx: &mut App) -> Entity<AppState> {
    let state = cx.new(|_| AppState::new(store));
    AppState::set_global(state.clone(), cx);
    state
}
