//! The app's state: the document store, one transport per document and the
//! open windows. One `Entity<AppState>` per app, reachable through
//! [`AppState::global`]; windows observe it and redraw when it notifies.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, App, AppContext, Entity, Global, WindowHandle};
use vizdaw_app::alerts::AlertItem;
use vizdaw_app::transport::{PollResult, TransportModel, TransportReadout};
use vizdaw_app::transport_bar::TransportLayout;
use vizdaw_app::{DocId, DocumentModel, DocumentStore, Prefs, SaveAction, SaveChoice, WindowToken};

use crate::plugin_windows::PluginWindows;
use crate::registry::SharedRegistry;
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
    /// A plug-in window ([`crate::plugin_windows`]): one per (document,
    /// slot), titled "‹strip› — ‹plug-in›".
    Plugin,
    /// Another window of a document: the Built-in Synth window, one per
    /// (document, destination).
    Other,
}

impl WindowKind {
    /// The channel's name (`document`, `mixer`, `plugin`, `other`).
    pub fn name(self) -> &'static str {
        match self {
            WindowKind::Document => "document",
            WindowKind::Mixer => "mixer",
            WindowKind::Plugin => "plugin",
            WindowKind::Other => "other",
        }
    }

    /// A document's own window or its Mixer window: a [`DocumentWindow`].
    pub fn is_document_window(self) -> bool {
        matches!(self, WindowKind::Document | WindowKind::Mixer)
    }
}

/// One open window: a document's own window, its Mixer window (both
/// [`DocumentWindow`]s), a plug-in window or a Built-in Synth window;
/// `kind` says which.
#[derive(Clone)]
pub struct WindowEntry {
    pub doc: DocId,
    pub token: WindowToken,
    pub handle: AnyWindowHandle,
    pub kind: WindowKind,
}

impl WindowEntry {
    /// The window's [`DocumentWindow`] (a document or Mixer window);
    /// `None` for the other kinds.
    pub fn document_window(&self) -> Option<WindowHandle<DocumentWindow>> {
        self.handle.downcast::<DocumentWindow>()
    }

    /// The elements the window drew last (`ui.tree`, `ui.click`).
    pub fn registry(&self, cx: &App) -> Option<SharedRegistry> {
        if let Some(h) = self.document_window() {
            return h.read(cx).ok().map(|v| v.registry().clone());
        }
        crate::plugin_windows::registry(self.handle, cx)
    }
}

pub struct AppState {
    pub store: DocumentStore,
    /// The keys of every command, kept in the preferences.
    pub keys: vizdaw_app::key_prefs::KeyBindings,
    /// The app's preferences: every document window draws its transport
    /// bar from them, so a change redraws them all. In memory until
    /// [`crate::run`] opens the launch home's.
    pub prefs: Prefs,
    /// Settings › Audio and Recording, over `prefs` (handed to the core
    /// at launch, changed core first).
    pub audio: vizdaw_app::audio_prefs::AudioPrefs,
    /// Settings › Appearance: the theme and the colour bars, over `prefs`.
    pub appearance: vizdaw_app::appearance_prefs::AppearancePrefs,
    /// Settings › Automation.
    pub automation: vizdaw_app::automation_prefs::AutomationPrefs,
    pub editing: vizdaw_app::editing_prefs::EditingPrefs,
    /// Settings › Plug-ins: the new-channel EQ, pushed to the core at launch.
    pub plugins: vizdaw_app::plugin_prefs::PluginPrefs,
    /// The keyboard layout id the windows use instead of the platform's
    /// (tests; `None`: `cx.keyboard_layout().id()`).
    pub keyboard_layout_override: Option<String>,
    /// Each document's transport, built from that document and only ever
    /// called with it.
    transports: HashMap<DocId, TransportModel>,
    windows: Vec<WindowEntry>,
    next_token: u64,
    /// When each document's session last had its idle tick.
    last_idle: HashMap<DocId, Instant>,
    /// The plug-in and Built-in Synth windows, with their models and
    /// native editor hosts.
    pub(crate) plugin_windows: PluginWindows,
    /// The Settings window, while it is open. It belongs to no document,
    /// so it is not one of `windows`.
    pub settings_window: Option<WindowHandle<crate::settings_window::SettingsWindow>>,
}

/// The poll's interval while a retry waits on Busy (Swift's 60 Hz tick).
pub const FAST_POLL: Duration = Duration::from_millis(16);

/// How often a window's surface tick looks again while no surface is set
/// up (it does nothing then; the transport poll still takes events).
const SURFACE_IDLE: Duration = Duration::from_millis(250);

/// The least time between two idle ticks of one document.
const IDLE_EVERY: Duration = Duration::from_millis(50);

struct GlobalAppState(Entity<AppState>);

impl Global for GlobalAppState {}

impl AppState {
    pub fn new(store: DocumentStore) -> Self {
        crate::appearance::set_current(vizdaw_app::theme::DEFAULT_THEME_ID);
        crate::appearance::set_colour_bars(true);
        AppState {
            store,
            keys: vizdaw_app::key_prefs::KeyBindings::load(&Prefs::in_memory()),
            prefs: Prefs::in_memory(),
            audio: Default::default(),
            appearance: Default::default(),
            automation: Default::default(),
            editing: Default::default(),
            plugins: Default::default(),
            keyboard_layout_override: None,
            transports: HashMap::new(),
            windows: Vec::new(),
            next_token: 1,
            last_idle: HashMap::new(),
            plugin_windows: PluginWindows::default(),
            settings_window: None,
        }
    }

    /// The keyboard layout's id: the override, else the platform's.
    pub fn keyboard_layout_id(&self, cx: &App) -> String {
        self.keyboard_layout_override
            .clone()
            .unwrap_or_else(|| cx.keyboard_layout().id().to_owned())
    }

    /// The app's state; panics before [`crate::install`].
    pub fn global(cx: &App) -> Entity<AppState> {
        cx.global::<GlobalAppState>().0.clone()
    }

    pub(crate) fn set_global(state: Entity<AppState>, cx: &mut App) {
        cx.set_global(GlobalAppState(state));
    }

    /// The theme's id `state.get windows` reports: the saved one, or
    /// `classic-light` when it names no theme.
    pub fn theme_id(&self) -> &'static str {
        &self.appearance.resolved().id
    }

    /// `ui.theme`: sets and saves the theme and redraws every window.
    /// `invalid_params` and no change for an id that names no theme.
    pub fn run_ui_theme(
        &mut self,
        args: &[(String, core_control::registry::ArgValue)],
        cx: &mut gpui::App,
    ) -> Result<vizdaw_app::commands::Ran, core_control::ControlError> {
        let ran = vizdaw_app::appearance_prefs::run_ui_theme(
            args,
            &mut self.appearance,
            &mut self.prefs,
        )?;
        crate::appearance::apply(&self.appearance.theme, cx);
        Ok(ran)
    }

    /// Settings › Appearance: makes `id` the theme, saves it (a failed write
    /// keeps it for this run, as `appearance.problem`) and redraws every
    /// window. `ui.theme` ends in the same `apply`. An id that names no
    /// theme changes nothing.
    pub fn set_theme(&mut self, id: &str, cx: &mut gpui::App) {
        if self.appearance.set_theme(id, &mut self.prefs).is_ok() {
            crate::appearance::apply(&self.appearance.theme, cx);
        }
    }

    /// Show channel colour bars: sets and saves it (a failed write keeps
    /// it for this run) and redraws every window.
    pub fn set_colour_bars(&mut self, on: bool, cx: &mut gpui::App) {
        self.appearance.set_colour_bars(on, &mut self.prefs);
        crate::appearance::set_colour_bars(on);
        cx.refresh_windows();
    }

    /// Takes `prefs` as the app's preferences; new projects are made in
    /// their default format. The saved audio and automation settings go to
    /// the core now (call it before any document opens, so the first
    /// engine opens with them), and new documents start from the record
    /// defaults.
    pub fn set_prefs(&mut self, mut prefs: Prefs) {
        self.store.set_default_format(prefs.default_format());
        let core = self.store.core();
        self.audio = vizdaw_app::audio_prefs::AudioPrefs::load(&prefs);
        self.audio.apply_at_launch(core.as_ref());
        self.automation = vizdaw_app::automation_prefs::AutomationPrefs::load(&prefs);
        self.automation.apply_at_launch(core.as_ref());
        self.plugins = vizdaw_app::plugin_prefs::PluginPrefs::load(&mut prefs, core.as_ref());
        self.editing = vizdaw_app::editing_prefs::EditingPrefs::load(&prefs);
        self.store.set_record_defaults(self.audio.record_defaults());
        self.appearance = vizdaw_app::appearance_prefs::AppearancePrefs::load(&prefs);
        crate::appearance::set_current(&self.appearance.theme);
        crate::appearance::set_colour_bars(self.appearance.colour_bars);
        self.keys = vizdaw_app::key_prefs::KeyBindings::load(&prefs);
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

    /// The plug-in and Built-in Synth windows' registry.
    pub fn plugin_windows(&self) -> &PluginWindows {
        &self.plugin_windows
    }

    /// The document windows (not the Mixer windows), one per document.
    pub fn document_windows(&self) -> impl Iterator<Item = &WindowEntry> {
        self.windows
            .iter()
            .filter(|w| w.kind == WindowKind::Document)
    }

    /// What window `w`'s title bar says: the document's title (the
    /// window marks an edit itself), "Mixer — <title>", a plug-in
    /// window's "‹strip› — ‹plug-in›" or "Built-in Synth".
    pub fn window_title(&self, w: &WindowEntry) -> Option<String> {
        match w.kind {
            WindowKind::Document => self.title(w.doc),
            WindowKind::Mixer => self.store.document(w.doc).map(mixer_title),
            WindowKind::Plugin | WindowKind::Other => self.plugin_windows.title(w.token),
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
        handle: impl Into<AnyWindowHandle>,
        kind: WindowKind,
    ) {
        self.windows.push(WindowEntry {
            doc,
            token,
            handle: handle.into(),
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

    /// `doc`'s window went: its plug-in windows close their editors
    /// first (before the session drops), then the document and its
    /// transport go too, and its other windows' entries (each window
    /// closes itself when it sees its entry or document gone).
    pub fn close_document(&mut self, doc: DocId) {
        self.plugin_windows.close_document(doc, &mut self.store);
        self.windows.retain(|w| w.doc != doc);
        self.transports.remove(&doc);
        self.store.close(doc);
    }

    /// Every plug-in window's editor closes (the windows close themselves
    /// when they see their entries gone): quit does this before any
    /// session drops.
    pub fn close_plugin_windows(&mut self) {
        let docs = self.plugin_windows.documents();
        for doc in docs {
            self.plugin_windows.close_document(doc, &mut self.store);
        }
        let windows = &mut self.windows;
        windows.retain(|w| w.kind.is_document_window());
    }

    /// Every document closes, with its transport and window entry; every
    /// plug-in editor first.
    pub fn close_all(&mut self) {
        self.close_plugin_windows();
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

    /// Reads `surfaces.json` at launch, on the UI thread as Swift did
    /// (`surfaces_load`; Ruling 3), and marks the surfaces' ports in the
    /// pickers.
    pub fn load_surfaces(&mut self) {
        self.store.load_surfaces();
    }

    /// Reads the input picker's sources once, off the UI thread, for the
    /// documents opened from now on (the app at launch; Swift
    /// `warmSources`).
    pub fn warm_sources(&mut self) {
        self.store.warm_sources();
    }

    /// Polls `doc`'s transport; true when what the window shows changed.
    pub fn poll(&mut self, doc: DocId) -> bool {
        // The surfaces drive the front document, refocused when it
        // changes (`S/VizDAWApp.swift:590-603`).
        self.store.follow_surface_focus();
        let idled = self.idle(doc);
        let Some(t) = self.transports.get_mut(&doc) else {
            return idled;
        };
        // The playhead draws the tick and sequence too; Faults the
        // faults and the compile report.
        let seen = |t: &TransportModel, store: &DocumentStore| {
            let s = t.state();
            (
                t.readout(None),
                s.tick,
                s.seq,
                store.alert().map(|a| a.id),
                s.faults.clone(),
                s.report.clone(),
            )
        };
        let before = seen(t, &self.store);
        let (result, recorded) = t.poll_document(&mut self.store, doc);
        let after = seen(t, &self.store);
        idled || recorded || result == PollResult::Failed || before != after
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

    /// How long until `doc`'s next surface tick; `None` once it is closed.
    /// [`FAST_POLL`] while a surface is set up or steps or detents wait,
    /// else [`SURFACE_IDLE`], when the tick does nothing.
    pub fn surface_interval(&self, doc: DocId) -> Option<std::time::Duration> {
        let d = self.store.document(doc)?;
        Some(if self.store.has_surfaces() || d.surfaces_waiting() {
            FAST_POLL
        } else {
            SURFACE_IDLE
        })
    }

    /// `doc`'s surface tick (Swift's 60 Hz timer, `S/VizDAWApp.swift:
    /// 587-603`): the focus follows the front document, the session's
    /// events are taken and the jog flushed, outside the idle's 50 ms
    /// throttle. The steps are for `doc`'s window to run. No work while no
    /// surface is set up and nothing waits.
    pub fn surface_tick(&mut self, doc: DocId) -> Vec<vizdaw_app::surface_relay::RelayStep> {
        let has = self.store.has_surfaces();
        if has {
            self.store.follow_surface_focus();
        }
        let Some(d) = self.store.document_mut(doc) else {
            return vec![];
        };
        if !has && !d.surfaces_waiting() {
            return vec![];
        }
        d.surface_tick(Instant::now());
        d.take_surface_steps()
    }

    /// How long until `doc`'s next poll; `None` once it is closed. While
    /// a note-off or a drag's final edit waits on Busy, at most
    /// [`FAST_POLL`].
    pub fn poll_interval(&self, doc: DocId) -> Option<std::time::Duration> {
        let fast = self
            .store
            .document(doc)
            .is_some_and(DocumentModel::wants_fast_poll);
        let d = self.store.document(doc);
        self.transports.get(&doc).map(|t| {
            let every = d.map_or_else(|| t.poll_interval(), |d| t.poll_interval_for(d));
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

    /// What File › Save (or Save As, `force_as`) does now; a refusal
    /// (recording, a held merge) is put in the status line.
    pub fn save_choice(&mut self, doc: DocId, force_as: bool) -> SaveChoice {
        self.store
            .with_document(doc, |d, _| {
                let c = d.save_choice(force_as);
                if let SaveChoice::Refused(why) = &c {
                    d.set_status(Some(why.clone()));
                }
                c
            })
            .unwrap_or_else(|| SaveChoice::Refused("the document is closed".into()))
    }

    /// Don't Save: the save owed at the end of a recording pass is dropped.
    pub fn cancel_save_after_pass(&mut self, doc: DocId) {
        if let Some(d) = self.store.document(doc) {
            d.cancel_save_after_pass();
        }
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

/// The app's state, once [`crate::install`] made it.
pub(crate) fn try_global(cx: &App) -> Option<Entity<AppState>> {
    cx.try_global::<GlobalAppState>().map(|g| g.0.clone())
}

/// Creates the app state and makes it reachable through [`AppState::global`].
pub(crate) fn create(store: DocumentStore, cx: &mut App) -> Entity<AppState> {
    let state = cx.new(|_| AppState::new(store));
    AppState::set_global(state.clone(), cx);
    state
}
