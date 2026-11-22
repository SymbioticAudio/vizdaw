//! The app's state: the document store, one transport per document and the
//! open windows. One `Entity<AppState>` per app, reachable through
//! [`AppState::global`]; windows observe it and redraw when it notifies.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{App, AppContext, Entity, Global, WindowHandle};
use vizdaw_app::alerts::AlertItem;
use vizdaw_app::transport::{PollResult, TransportModel, TransportReadout};
use vizdaw_app::{DocId, DocumentStore, SaveAction, WindowToken};

use crate::shell::DocumentWindow;

/// One open document window.
#[derive(Clone)]
pub struct WindowEntry {
    pub doc: DocId,
    pub token: WindowToken,
    pub handle: WindowHandle<DocumentWindow>,
}

pub struct AppState {
    pub store: DocumentStore,
    /// Each document's transport, built from that document and only ever
    /// called with it.
    transports: HashMap<DocId, TransportModel>,
    windows: Vec<WindowEntry>,
    next_token: u64,
    /// When each document's session last had its idle tick.
    last_idle: HashMap<DocId, Instant>,
}

/// The least time between two idle ticks of one document.
const IDLE_EVERY: Duration = Duration::from_millis(50);

struct GlobalAppState(Entity<AppState>);

impl Global for GlobalAppState {}

impl AppState {
    pub fn new(store: DocumentStore) -> Self {
        AppState {
            store,
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

    pub fn windows(&self) -> &[WindowEntry] {
        &self.windows
    }

    pub fn window_of(&self, doc: DocId) -> Option<&WindowEntry> {
        self.windows.iter().find(|w| w.doc == doc)
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
    ) {
        self.windows.push(WindowEntry { doc, token, handle });
    }

    /// `doc`'s window went: the document and its transport go too.
    pub fn close_document(&mut self, doc: DocId) {
        self.windows.retain(|w| w.doc != doc);
        self.transports.remove(&doc);
        self.store.close(doc);
    }

    /// Every document closes, with its transport and window entry.
    pub fn close_all(&mut self) {
        let docs: Vec<DocId> = self.windows.iter().map(|w| w.doc).collect();
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
        let before = (t.readout(None), self.store.alert().map(|a| a.id));
        let result = t.poll(&mut self.store);
        let after = (t.readout(None), self.store.alert().map(|a| a.id));
        idled || result == PollResult::Failed || before != after
    }

    /// The session's idle tick, at most every 50 ms; true when a new
    /// snapshot arrived (the title may have changed).
    fn idle(&mut self, doc: DocId) -> bool {
        let now = Instant::now();
        if self
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

    /// How long until `doc`'s next poll; `None` once it is closed.
    pub fn poll_interval(&self, doc: DocId) -> Option<std::time::Duration> {
        self.transports.get(&doc).map(|t| t.poll_interval())
    }

    /// What `doc`'s transport bar shows. Sequence info (the meter map) is
    /// not read this slice, so the signature is a dash; the bar shows the
    /// bar and time readouts only.
    pub fn readout(&self, doc: DocId) -> Option<TransportReadout> {
        self.transports.get(&doc).map(|t| t.readout(None))
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

    /// The name Save As suggests.
    pub fn suggested_name(&self, doc: DocId) -> String {
        let title = self
            .store
            .document(doc)
            .map(|d| d.snapshot().title.clone())
            .unwrap_or_default();
        format!(
            "{}.vizdaw",
            if title.is_empty() { "Untitled" } else { &title }
        )
    }

    /// Dismisses the alert showing; the next queued one takes its place.
    pub fn dismiss_alert(&mut self) {
        self.store.take_alert();
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The existing project `path` would silently replace: `Some` when adding
/// the `.vizdaw` extension changed the path (so the save panel never asked
/// about replacing it) and the result exists.
pub fn silent_replacement(path: &Path) -> Option<PathBuf> {
    let full = with_extension(path);
    (full != path && full.exists()).then_some(full)
}

/// `path`, ending in `.vizdaw`.
pub fn with_extension(path: &Path) -> PathBuf {
    if path.extension().and_then(|e| e.to_str()) == Some("vizdaw") {
        path.to_path_buf()
    } else {
        let mut name = path.as_os_str().to_owned();
        name.push(".vizdaw");
        PathBuf::from(name)
    }
}

/// Creates the app state and makes it reachable through [`AppState::global`].
pub(crate) fn create(store: DocumentStore, cx: &mut App) -> Entity<AppState> {
    let state = cx.new(|_| AppState::new(store));
    AppState::set_global(state.clone(), cx);
    state
}
