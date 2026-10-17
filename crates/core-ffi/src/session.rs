//! `Session`: one open document (one window).

use crate::editor::{self, EditorSize};
use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use crate::host;
use crate::mixer::{self, DestRow, MixerLevels, MixerSnapshot};
use crate::plugins;
use crate::recovery;
use crate::snapshot::{
    DocSnapshot, IdleUpdate, LaneRow, SlotRow, TransportState, lane_rows, position_bbu, seconds,
    seq_id, seq_rows, warnings,
};
use crate::snapshot::{ParamRow, ParamValue, PluginRefRow};
use core_host::PluginHost;
use core_model::command::Command;
use core_model::command::MixerCmd;
use core_model::command::RoutingCmd;
use core_model::command::lane::LaneCmd;
use core_model::command::sequence::SequenceCmd;
use core_model::document::DestKind;
use core_model::document::Document;
use core_model::file;
use core_model::history::{History, Timestamp};
use core_model::ids::{DestId, LaneId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{AuxSend, Output, ParamId, PluginRef, PluginSlot};
use core_model::selection::ViewFilter;
use core_model::time::Tick;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Package extension.
pub const EXTENSION: &str = "vizdaw";

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct Inner {
    history: History,
    path: Option<PathBuf>,
    /// The document as last opened or saved; `dirty` compares against it.
    clean: Document,
    version: u64,
    looped: bool,
    /// Where the next Play starts when this session does not hold the
    /// engine; `None` means bar 1 (or resume when it does).
    cursor: Option<Tick>,
    /// The sequence `cursor` belongs to.
    cursor_seq: Option<SeqId>,
    /// Sticky: the last engine recompile after an edit failed. Cleared by
    /// the next successful recompile or Play.
    reload_fault: Option<String>,
    /// A reload could not run (the plugin registry was busy, or Play's
    /// load failed): `idle` runs it once it can.
    reload_owed: bool,
    /// Plugin gestures waiting for an open edit to end.
    held: Vec<core_host::Gesture>,
    /// Slots whose plugin window is open (saved with the project, spec §9).
    windows: std::collections::BTreeSet<u64>,
    /// What plugin timer steps found since the last `idle`.
    news: News,
    /// The plugins the crash marker names (as last written).
    marked: Vec<PluginRef>,
    /// When the project was last autosaved (or opened).
    last_autosave: Instant,
    /// A plugin loaded: autosave at the next tick (spec §10).
    autosave_soon: bool,
    /// A plugin changed its own state (not a parameter) since the last
    /// save: unsaved, though the document did not change.
    plugin_dirty: bool,
    /// The crash marker's identity (the canonical path when it was
    /// opened), so closing finds it even if the package moved.
    key: Option<PathBuf>,
    /// `close` ran: the marker is gone and must not come back.
    closed: bool,
    /// Lane revisions (spec §3.1), refreshed whenever a snapshot is built.
    revs: crate::revs::Revisions,
}

/// The plugins the crash marker names: those loading now, and with
/// `ahead` also those the next sync is about to start loading. `None`
/// while the registry is busy.
fn marker_names(id: u64, doc: &Document, ahead: bool) -> Option<Vec<PluginRef>> {
    plugins::with(id, |p| {
        let Some(p) = p else { return Vec::new() };
        let mut names: Vec<PluginRef> = p.loading().into_iter().map(|(_, r)| r).collect();
        if ahead {
            for r in p.loads_needed(doc) {
                if !names.contains(&r) {
                    names.push(r);
                }
            }
        }
        names
    })
    .ok()
}

/// What plugin timer steps found, handed out by the next `idle`.
#[derive(Debug, Default)]
struct News {
    doc_changed: bool,
    slots_changed: bool,
    params_changed: std::collections::BTreeSet<u64>,
    problems: Vec<String>,
}

impl Inner {
    fn new(doc: Document, path: Option<PathBuf>) -> Inner {
        let key = path.as_deref().map(recovery::key);
        Inner {
            key,
            revs: crate::revs::Revisions::new(&doc),
            history: History::new(doc.clone()),
            path,
            clean: doc,
            version: 0,
            looped: false,
            cursor: None,
            cursor_seq: None,
            reload_fault: None,
            reload_owed: false,
            held: Vec::new(),
            windows: std::collections::BTreeSet::new(),
            news: News::default(),
            marked: Vec::new(),
            last_autosave: Instant::now(),
            autosave_soon: false,
            plugin_dirty: false,
            closed: false,
        }
    }

    /// The sequence a position set without the engine belongs to: the one
    /// last played, else the first top-level sequence.
    fn selected_seq(&self) -> Option<SeqId> {
        let doc = self.doc();
        if let Some(s) = self.cursor_seq.filter(|s| doc.sequence(*s).is_ok()) {
            return Some(s);
        }
        let rows = seq_rows(doc).ok()?;
        rows.iter()
            .find(|r| r.top_level)
            .or_else(|| rows.first())
            .map(|r| SeqId(r.id))
    }

    /// After an edit: the plugin host follows the document and the engine
    /// recompiles. A failure (or panic, which also resets the host) never
    /// fails the edit: it becomes a sticky fault.
    fn reload_host(&mut self, id: u64) {
        // The marker names any plugin the sync starts loading first.
        self.mark(id, true);
        let (doc, looped) = (self.history.doc(), self.looped);
        let r =
            ffi_guard(|| plugins::with(id, |p| host::reload(id, doc, looped, p)).and_then(|r| r));
        self.reload_owed = matches!(r, Err(VizError::Busy { .. }));
        self.reload_fault = match r {
            Ok(()) => None,
            // Not a fault: `idle` retries.
            Err(VizError::Busy { .. }) => None,
            Err(e) => {
                if let VizError::Internal { .. } = e {
                    host::reset();
                }
                Some(format!("reload failed: {e}"))
            }
        };
    }

    fn doc(&self) -> &Document {
        self.history.doc()
    }

    /// A lane's current revision (0 for a lane that does not exist).
    fn lane_rev(&mut self, seq: SeqId, lane: LaneId) -> u64 {
        self.revs.update(self.history.doc());
        self.revs.rev(seq, lane).unwrap_or(0)
    }

    /// Rewrites the crash marker when the plugins loading changed. With
    /// `ahead` it also names the plugins the next sync will start loading,
    /// so it is on disk before any of them can run code. Skipped while
    /// the registry is busy (no sync can run then) and after `close`.
    fn mark(&mut self, id: u64, ahead: bool) {
        let (Some(key), false) = (self.key.clone(), self.closed) else {
            return;
        };
        let Some(loading) = marker_names(id, self.history.doc(), ahead) else {
            return;
        };
        if loading != self.marked {
            if let Err(e) = recovery::write_marker(&key, id, &loading) {
                self.news
                    .problems
                    .push(format!("crash marker not written: {e}"));
            }
            self.marked = loading;
        }
    }

    /// The document changed since it was opened or saved, or a plugin
    /// changed its own state.
    fn unsaved(&self) -> bool {
        self.plugin_dirty || *self.doc() != self.clean
    }

    fn snapshot(&mut self) -> Result<DocSnapshot, VizError> {
        self.revs.update(self.history.doc());
        let doc = self.doc();
        Ok(DocSnapshot {
            lane_revs: self.revs.rows(),
            version: self.version,
            title: title(self.path.as_deref()),
            dirty: self.unsaved(),
            undo_label: self.history.undo_label().map(str::to_string),
            redo_label: self.history.redo_label().map(str::to_string),
            sequences: seq_rows(doc)?,
            warnings: warnings(doc),
        })
    }
}

/// "demo" for `/x/demo.vizdaw`, "Untitled" without a path.
fn title(path: Option<&Path>) -> String {
    path.and_then(Path::file_stem)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string())
}

/// Appends `.vizdaw` unless present.
fn with_extension(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.extension().is_some_and(|e| e == EXTENSION) {
        p
    } else {
        let mut s = p.into_os_string();
        s.push(".");
        s.push(EXTENSION);
        PathBuf::from(s)
    }
}

/// One open document with its undo history and transport.
#[derive(uniffi::Object)]
pub struct Session {
    id: u64,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "test-hooks")]
impl Session {
    /// An untitled session for any document (tests build documents with
    /// `core_engine::fixtures::Builder`). Not exported to Swift.
    pub fn from_document(doc: Document) -> Result<Arc<Session>, VizError> {
        ffi_guard(|| Session::with(doc, None, false))
    }
}

/// Saving and autosaving one at a time (the package's blob pruning reads
/// the autosaves; 3a: the FFI serializes them).
static SAVE_LOCK: Mutex<()> = Mutex::new(());

/// What a held slot shows in safe mode.
pub const PLUGINS_OFF: &str = "Plugins off (safe mode)";

impl Session {
    /// A session for `doc`; its plugins start loading on this thread (none
    /// with `plugins_off`: every slot is held, spec §10). A project with a
    /// path gets its crash marker before any plugin loads.
    fn with(
        doc: Document,
        path: Option<PathBuf>,
        plugins_off: bool,
    ) -> Result<Arc<Session>, VizError> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let key = path.as_deref().map(recovery::key);
        // An earlier crash's marker survives a failed open (retrying
        // still offers safe mode).
        let saved = key.as_deref().and_then(recovery::backup);
        // Without a marker the project still opens; recovery is just not
        // offered after a crash.
        let unmarked = key
            .as_deref()
            .and_then(|k| recovery::opened(k, id, &[]).err());
        let fail = |e: VizError| {
            plugins::close(id);
            if let Some(k) = key.as_deref() {
                recovery::closed(k, id);
                recovery::restore(k, saved.clone());
            }
            Err(e)
        };
        // A busy registry fails the open (no plugin host to open with).
        if let Err(e) = plugins::create(id) {
            return fail(e);
        }
        let held = plugins::with(id, |p| {
            if let (Some(p), true) = (p, plugins_off) {
                for slot in mixer::slot_ids(&doc) {
                    p.hold(slot, Some(PLUGINS_OFF.to_string()));
                }
            }
        });
        if let Err(e) = held {
            return fail(e);
        }
        // The marker names the plugins this sync will start loading
        // before any load can run plugin code.
        let mut marked = Vec::new();
        if let Some(k) = key.as_deref() {
            if let Some(names) = marker_names(id, &doc, true) {
                // A failure leaves `marked` empty: the next mark retries.
                if recovery::write_marker(k, id, &names).is_ok() {
                    marked = names;
                }
            }
        }
        recovery::before_sync();
        let synced = plugins::with(id, |p| {
            if let Some(p) = p {
                p.sync(&doc);
            }
        });
        let s = Arc::new(Session {
            id,
            inner: Mutex::new(Inner::new(doc, path)),
        });
        {
            let mut inner = lock(&s.inner);
            inner.marked = marked;
            if let Some(e) = unmarked {
                inner
                    .news
                    .problems
                    .push(format!("crash marker not written: {e}"));
            }
            // A sync that could not run is retried by `idle`.
            inner.reload_owed = synced.is_err();
            inner.mark(id, false);
        }
        Ok(s)
    }

    fn mark(&self, inner: &mut Inner) {
        inner.mark(self.id, false);
    }

    /// Captures every running plugin's state into the document (spec §4:
    /// on save and autosave). Refused inside an open edit. `Busy` when a
    /// plugin call is on the stack: nothing is captured, and the caller
    /// must not write a half-captured file.
    fn capture_all(&self, inner: &mut Inner) -> Result<bool, VizError> {
        let history = &mut inner.history;
        let failed = plugins::with(self.id, |p| {
            p.map(|p| p.capture_all(history)).unwrap_or_default()
        })?;
        let all = failed.is_empty();
        for (id, e) in failed {
            let line = format!("{}: state not saved ({e})", slot_name(inner.doc(), id));
            inner.news.problems.push(line);
        }
        Ok(all)
    }

    /// Autosaves (spec §10) a project with a path and unsaved changes,
    /// every [`recovery::autosave_interval`] and right after a plugin
    /// loaded; never inside an open edit (a drag). A busy plugin registry
    /// skips this autosave; it stays due and is retried.
    fn maybe_autosave(&self, inner: &mut Inner) {
        let Some(path) = inner.path.clone() else {
            return;
        };
        let due =
            inner.autosave_soon || inner.last_autosave.elapsed() >= recovery::autosave_interval();
        if !due || inner.history.in_transaction() {
            return;
        }
        if !inner.unsaved() {
            // Nothing unsaved: nothing to keep.
            inner.autosave_soon = false;
            inner.last_autosave = Instant::now();
            return;
        }
        if self.capture_all(inner).is_err() {
            return;
        }
        inner.autosave_soon = false;
        inner.last_autosave = Instant::now();
        let _g = lock(&SAVE_LOCK);
        if let Err(e) = file::autosave(inner.doc(), &path, Timestamp::now()) {
            inner.news.problems.push(format!("autosave failed: {e}"));
        }
    }

    /// Saves to `path`: plugin states captured first (`Busy`, with
    /// nothing written, when a plugin call is on the stack).
    fn save_to(&self, inner: &mut Inner, path: &Path) -> Result<(), VizError> {
        let captured = self.capture_all(inner)?;
        let _g = lock(&SAVE_LOCK);
        file::save(inner.doc(), path)?;
        write_windows(path, &inner.windows);
        inner.clean = inner.doc().clone();
        // A plugin whose state could not be captured is still unsaved.
        if captured {
            inner.plugin_dirty = false;
        }
        Ok(())
    }

    /// One plugin timer step: the host services its plugins (with the
    /// engine when this session plays), news are kept for the next `idle`,
    /// and finished gestures become undo steps. Returns true when the
    /// document changed. Nothing happens while the registry is busy.
    fn pump(&self, inner: &mut Inner) -> bool {
        inner.mark(self.id, true);
        let report = {
            let doc = inner.doc();
            plugins::with(self.id, |p| p.map(|p| host::service(self.id, doc, p)))
        };
        let Ok(Some(r)) = report else {
            return false;
        };
        for (id, why) in &r.failed {
            let line = format!("{}: {why}", slot_name(inner.doc(), *id));
            inner.news.problems.push(line);
        }
        for (id, why) in &r.rejected_state {
            let line = format!(
                "{}: saved state refused ({why})",
                slot_name(inner.doc(), *id)
            );
            inner.news.problems.push(line);
        }
        inner.news.slots_changed |= !r.loaded.is_empty()
            || !r.failed.is_empty()
            || !r.restart.is_empty()
            || r.compiled.is_some();
        inner
            .news
            .params_changed
            .extend(r.params_changed.iter().map(|i| i.0));
        inner.autosave_soon |= !r.loaded.is_empty();
        if !r.state_dirty.is_empty() && !inner.plugin_dirty {
            inner.plugin_dirty = true;
            // The title shows Edited.
            inner.news.doc_changed = true;
        }
        inner.held.extend(r.gestures);
        let changed = self.commit_held(inner);
        self.mark(inner);
        self.maybe_autosave(inner);
        changed
    }

    /// Commits the gestures waiting for an open edit (a drag) to end.
    /// Inside the edit they would merge into its undo step, so they wait.
    fn commit_held(&self, inner: &mut Inner) -> bool {
        if inner.history.in_transaction() {
            return false;
        }
        let gestures = std::mem::take(&mut inner.held);
        self.commit_gestures(inner, &gestures)
    }

    /// Commits plugin-UI gestures as undo steps (spec §4), then lets the
    /// plugin host and the engine follow (the host must see the committed
    /// document before any undo, 3b `PluginHost::commit_gestures`).
    fn commit_gestures(&self, inner: &mut Inner, gestures: &[core_host::Gesture]) -> bool {
        if gestures.is_empty() {
            return false;
        }
        let r = PluginHost::commit_gestures(&mut inner.history, gestures);
        inner.version += 1;
        inner.reload_host(self.id);
        if let Err(e) = r {
            inner.reload_fault = Some(format!("plugin gesture refused: {e}"));
        }
        true
    }

    /// Applies one command as one undo step and recompiles if playing.
    /// Running plugins it removes are captured first (spec §4), so an undo
    /// brings them back with their state.
    fn execute(&self, cmd: Command) -> Result<DocSnapshot, VizError> {
        let next = core_model::command::apply(lock(&self.inner).doc(), &cmd)
            .ok()
            .map(|n| n.doc);
        if let Some(next) = next {
            self.capture_removed(&next)?;
        }
        let mut inner = lock(&self.inner);
        inner.history.execute(&cmd, &ViewFilter::default())?;
        inner.version += 1;
        inner.reload_host(self.id);
        self.mark(&mut inner);
        inner.snapshot()
    }

    /// Captures the state of every running plugin that `next` (the
    /// document an edit, undo or redo is about to make) no longer has
    /// (Plan Ruling 8; the model's `Report.removed` does not list them
    /// all). Plugin code (`save_state` may spin a run loop) runs without
    /// the session lock, against a copy of the history; blobs are
    /// content-addressed, so storing the same bytes in the real history
    /// afterwards gives the ids the host recorded. Nothing is captured
    /// inside an open edit (a drag removes no plugin). `Busy` when a
    /// plugin call is on the stack: the removal is refused rather than
    /// losing the state.
    fn capture_removed(&self, next: &Document) -> Result<(), VizError> {
        let (mut scratch, gone) = {
            let inner = lock(&self.inner);
            let before = mixer::slot_ids(inner.doc());
            let after = mixer::slot_ids(next);
            let gone: Vec<_> = before.difference(&after).copied().collect();
            if gone.is_empty() {
                return Ok(());
            }
            (inner.history.clone(), gone)
        };
        let captured = plugins::with(self.id, |p| {
            let Some(p) = p else { return Vec::new() };
            gone.into_iter()
                // A placeholder has nothing to capture; its saved state
                // stays in the document.
                .filter_map(|id| p.capture(&mut scratch, id).ok().map(|blob| (id, blob)))
                .collect::<Vec<_>>()
        })?;
        let mut inner = lock(&self.inner);
        for (id, blob) in captured {
            if let Some(bytes) = scratch.doc().state_blob(blob) {
                inner.history.capture_state(id, bytes)?;
            }
        }
        Ok(())
    }

    /// Unloads the running plugins the catalog turned off since (spec §9:
    /// the Plugin Manager, or the crash prompt's "Disable"): their state is
    /// captured first, like a removal's (plugin code runs without the
    /// session lock, against a copy of the history; the blobs are then
    /// stored in the real one, no undo step), then the host drops them and
    /// their slots show "Disabled in Plugin Manager", and the engine is
    /// reloaded so it no longer plays them. Waits while an edit is open
    /// (no capture inside a transaction) and while the registry is busy
    /// (a catalog change made then is applied by the next registry
    /// access). Returns true when something was unloaded.
    fn unload_disabled(&self) -> bool {
        let due = plugins::with(self.id, |p| p.is_some_and(|p| p.unloads_due()));
        if due != Ok(true) {
            return false;
        }
        let mut scratch = {
            let inner = lock(&self.inner);
            if inner.history.in_transaction() {
                return false;
            }
            inner.history.clone()
        };
        let Ok(unloaded) = plugins::with(self.id, |p| {
            p.map(|p| p.unload_unavailable(&mut scratch))
                .unwrap_or_default()
        }) else {
            return false;
        };
        let mut inner = lock(&self.inner);
        for (id, captured) in unloaded {
            let stored = captured.map_err(|e| e.to_string()).and_then(|blob| {
                let bytes = scratch
                    .doc()
                    .state_blob(blob)
                    .ok_or_else(|| "captured state missing".to_string())?;
                inner
                    .history
                    .capture_state(id, bytes)
                    .map_err(|e| e.to_string())
            });
            // Reported like a save's capture failure; the document keeps
            // the state it had.
            if let Err(e) = stored {
                let line = format!("{}: state not saved ({e})", slot_name(inner.doc(), id));
                inner.news.problems.push(line);
            }
        }
        inner.news.slots_changed = true;
        // The title shows Edited when a captured state is new.
        inner.news.doc_changed = true;
        inner.reload_host(self.id);
        true
    }

    /// Undo or redo, then recompile if playing. Busy (a nested run loop
    /// inside a plugin call) is refused before history is touched: the
    /// gesture commit below could not run, so undo would act on an older
    /// step and the gesture, committed later, would clear its redo.
    fn step(&self, undo: bool) -> Result<DocSnapshot, VizError> {
        if plugins::is_busy() {
            return Err(VizError::Busy {
                message: "plugin host is busy".into(),
            });
        }
        let next = {
            let mut inner = lock(&self.inner);
            // A gesture the plugin finished since the last timer tick is
            // the newest step: undo must see it (3b: sync after commit,
            // before undo).
            if self.pump(&mut inner) {
                inner.news.doc_changed = true;
            }
            if undo {
                inner.history.peek_undo().cloned()
            } else {
                inner.history.peek_redo().cloned()
            }
        };
        if let Some(next) = next {
            self.capture_removed(&next)?;
        }
        let mut inner = lock(&self.inner);
        if undo {
            inner.history.undo()?;
        } else {
            inner.history.redo()?;
        }
        inner.version += 1;
        inner.reload_host(self.id);
        self.mark(&mut inner);
        inner.snapshot()
    }

    /// Runs a transport call; an `Internal` error drops the engine host so
    /// the next Play starts clean.
    fn transport_call<T>(&self, f: impl FnOnce() -> Result<T, VizError>) -> Result<T, VizError> {
        let r = ffi_guard(f);
        if let Err(VizError::Internal { .. }) = &r {
            host::reset();
        }
        r
    }
}

#[uniffi::export]
impl Session {
    /// An untitled, empty document.
    #[uniffi::constructor]
    pub fn new_empty() -> Result<Arc<Session>, VizError> {
        ffi_guard(|| Session::with(Document::new(), None, false))
    }

    /// An untitled copy of the demo project.
    #[uniffi::constructor]
    pub fn new_demo() -> Result<Arc<Session>, VizError> {
        ffi_guard(|| {
            let doc = core_engine::fixtures::demo_document()?;
            Session::with(doc, None, false)
        })
    }

    /// Opens the `.vizdaw` package at `path`.
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Session>, VizError> {
        Session::open_with(
            path,
            OpenOptions {
                plugins_off: false,
                autosave: None,
            },
        )
    }

    /// Opens the `.vizdaw` package at `path` after the app asked what
    /// `recovery_check` found (spec §10): with every plugin slot held
    /// ("Open with Plugins Off"), and/or from an autosave copy ("Restore
    /// autosave": it opens as Edited, so Save keeps it).
    #[uniffi::constructor]
    pub fn open_with(path: String, options: OpenOptions) -> Result<Arc<Session>, VizError> {
        ffi_guard(|| {
            let p = PathBuf::from(&path);
            if !p.join(file::PROJECT_FILE).is_file() {
                return Err(VizError::NotFound { path });
            }
            let (doc, clean) = match &options.autosave {
                Some(a) => {
                    let doc = file::load_autosave(&p, Path::new(a))?;
                    let clean = file::load(&p).unwrap_or_else(|_| doc.clone());
                    (doc, clean)
                }
                None => {
                    let doc = file::load(&p)?;
                    (doc.clone(), doc)
                }
            };
            let windows = read_windows(&p);
            let s = Session::with(doc, Some(p), options.plugins_off)?;
            {
                let mut inner = lock(&s.inner);
                inner.windows = windows;
                inner.clean = clean;
            }
            Ok(s)
        })
    }

    /// Saves to the current path; `Io` when untitled.
    pub fn save(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let path = inner.path.clone().ok_or_else(|| VizError::Io {
                message: "the document has no file yet; use Save As".into(),
            })?;
            self.save_to(&mut inner, &path)
        })
    }

    /// Saves to `path` (`.vizdaw` appended if missing) and makes it the
    /// current path.
    pub fn save_as(&self, path: String) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let p = with_extension(&path);
            self.save_to(&mut inner, &p)?;
            if inner.path.as_deref() != Some(p.as_path()) {
                // The marker follows the project to its new place (not
                // after `close`).
                if let Some(old) = inner.key.take() {
                    recovery::closed(&old, self.id);
                }
                inner.marked.clear();
                if !inner.closed {
                    let key = recovery::key(&p);
                    if let Err(e) = recovery::opened(&key, self.id, &[]) {
                        inner
                            .news
                            .problems
                            .push(format!("crash marker not written: {e}"));
                    }
                    inner.key = Some(key);
                }
                inner.path = Some(p);
                self.mark(&mut inner);
            }
            Ok(())
        })
    }

    /// The current package path.
    pub fn path(&self) -> Result<Option<String>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            Ok(inner
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()))
        })
    }

    /// The document at sequence level.
    pub fn snapshot(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| lock(&self.inner).snapshot())
    }

    /// The lanes of sequence `seq`.
    pub fn lanes(&self, seq: u64) -> Result<Vec<LaneRow>, VizError> {
        ffi_guard(|| lane_rows(lock(&self.inner).doc(), seq))
    }

    /// Renames sequence `seq` (one undo step).
    pub fn rename_sequence(&self, seq: u64, name: String) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Sequence(SequenceCmd::Rename {
                seq: SeqId(seq),
                name,
            }))
        })
    }

    /// Mutes or unmutes lane `lane` of sequence `seq` (one undo step).
    pub fn set_lane_mute(&self, seq: u64, lane: u64, mute: bool) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Lane(LaneCmd::SetFlags {
                seq: SeqId(seq),
                lane: LaneId(lane),
                mute: Some(mute),
                solo: None,
                arm: None,
            }))
        })
    }

    /// Reverts the last step.
    pub fn undo(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| self.step(true))
    }

    /// Reapplies the last undone step.
    pub fn redo(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| self.step(false))
    }

    /// Plays sequence `seq`, stopping any other session. Resumes where
    /// this session stopped when the same sequence is still loaded.
    pub fn play(&self, seq: u64) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            let id = seq_id(inner.doc(), seq)?;
            // A position set while another session held the engine.
            let start = match (inner.cursor, inner.cursor_seq) {
                (Some(t), None) => Some(t),
                (Some(t), Some(s)) if s == id => Some(t),
                _ => None,
            };
            let looped = inner.looped;
            inner.mark(self.id, true);
            let played = plugins::with(self.id, |p| {
                host::play(self.id, inner.doc(), id, start, looped, p)
            })
            .and_then(|r| r);
            if let Err(e) = played {
                // Retried by `idle` (the engine may hold stale plugins).
                inner.reload_owed = true;
                return Err(e);
            }
            inner.reload_owed = false;
            inner.reload_fault = None;
            inner.cursor = None;
            inner.cursor_seq = Some(id);
            Ok(())
        })
    }

    /// Stops; the position is kept.
    pub fn stop(&self) -> Result<(), VizError> {
        self.transport_call(|| host::stop(self.id))
    }

    /// Moves the position to `tick`.
    pub fn locate(&self, tick: i64) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            if !host::locate(self.id, Tick(tick))? {
                inner.cursor = Some(Tick(tick));
                inner.cursor_seq = inner.selected_seq();
            }
            Ok(())
        })
    }

    /// Loops from bar 1 to the end of the sequence, or stops looping.
    pub fn set_loop(&self, on: bool) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            inner.looped = on;
            host::set_loop(self.id, on)
        })
    }

    /// Where the transport stands.
    pub fn transport(&self) -> Result<TransportState, VizError> {
        self.transport_call(|| {
            let inner = lock(&self.inner);
            let doc = inner.doc();
            let state = plugins::with(self.id, |p| host::state(self.id, doc, p)).and_then(|r| r)?;
            Ok(match state {
                Some(mut h) => TransportState {
                    playing: h.playing,
                    seq: Some(h.seq.0),
                    tick: h.tick.0,
                    position_bbu: position_bbu(doc, h.seq, h.tick),
                    seconds: seconds(doc, h.seq, h.tick),
                    sample_rate: h.sample_rate,
                    device: h.device,
                    faults: {
                        h.faults.extend(inner.reload_fault.clone());
                        h.faults
                    },
                    report: h.report,
                },
                None => {
                    let tick = inner.cursor.unwrap_or(Tick::ZERO);
                    let seq = inner.cursor_seq;
                    TransportState {
                        playing: false,
                        seq: seq.map(|s| s.0),
                        tick: tick.0,
                        position_bbu: seq
                            .map_or_else(|| "1.1.0".to_string(), |s| position_bbu(doc, s, tick)),
                        seconds: seq.map_or(0.0, |s| seconds(doc, s, tick)),
                        sample_rate: 0.0,
                        device: String::new(),
                        faults: inner.reload_fault.iter().cloned().collect(),
                        report: Vec::new(),
                    }
                }
            })
        })
    }
}

#[uniffi::export]
impl Session {
    /// The plugin timer's step (spec §5 `host_idle`; the app calls it about
    /// 60 times a second on the main thread): finishes plugin loads, runs
    /// plugin callbacks, lets the engine follow, and turns plugin-UI
    /// gestures into undo steps.
    pub fn idle(&self) -> Result<IdleUpdate, VizError> {
        ffi_guard(|| {
            let nothing = || IdleUpdate {
                snapshot: None,
                slots_changed: false,
                params_changed: Vec::new(),
                problems: Vec::new(),
            };
            // A nested run loop inside a plugin call: skip this tick (before
            // taking the session lock, which that call may hold).
            if plugins::is_busy() {
                return Ok(nothing());
            }
            plugins::tick_closing();
            self.unload_disabled();
            let mut inner = lock(&self.inner);
            if inner.reload_owed {
                inner.reload_host(self.id);
            }
            let changed = self.pump(&mut inner);
            let n = std::mem::take(&mut inner.news);
            Ok(IdleUpdate {
                snapshot: if changed || n.doc_changed {
                    Some(inner.snapshot()?)
                } else {
                    None
                },
                slots_changed: n.slots_changed,
                params_changed: n.params_changed.into_iter().collect(),
                problems: n.problems,
            })
        })
    }

    /// Every plugin slot with its status: instruments' own slots, then
    /// inserts.
    pub fn plugin_slots(&self) -> Result<Vec<SlotRow>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            plugins::with(self.id, |p| plugins::slot_rows(inner.doc(), p))
        })
    }
}

/// Plugin parameters (spec §11): the list with current values, a live
/// set for controls being dragged, and the commit that makes one undo step.
#[uniffi::export]
impl Session {
    /// Slot `slot`'s parameters with their current values (empty while it
    /// is not running).
    pub fn params(&self, slot: u64) -> Result<Vec<ParamRow>, VizError> {
        ffi_guard(|| {
            plugins::with(self.id, |p| {
                let Some(inst) = p.and_then(|p| p.instance(PluginInstanceId(slot))) else {
                    return Vec::new();
                };
                let infos = inst.info().params.clone();
                infos
                    .into_iter()
                    .map(|i| ParamRow {
                        id: i.id.0,
                        value: inst.param_value(i.id).unwrap_or(i.default),
                        name: i.name,
                        module: i.module,
                        min: i.min,
                        max: i.max,
                        default: i.default,
                        stepped: i.stepped,
                        automatable: i.automatable,
                        hidden: i.hidden,
                    })
                    .collect()
            })
        })
    }

    /// Sends a value to the running plugin now, without an undo step (a
    /// control being dragged); `commit_params` records the final value.
    pub fn set_param(&self, slot: u64, param: u64, value: f64) -> Result<(), VizError> {
        ffi_guard(|| {
            if !value.is_finite() {
                return Err(VizError::Model {
                    message: "parameter values must be finite".into(),
                });
            }
            plugins::with(self.id, |p| {
                if let Some(p) = p {
                    p.set_param(PluginInstanceId(slot), ParamId(param), value);
                }
            })
        })
    }

    /// Records parameter values in the slot's overlay as one undo step
    /// (`SetPluginParams`, spec §4).
    pub fn commit_params(
        &self,
        slot: u64,
        values: Vec<ParamValue>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetPluginParams {
                slot: PluginInstanceId(slot),
                params: values.iter().map(|v| (ParamId(v.id), v.value)).collect(),
            }))
        })
    }

    /// Closes the gesture slot `slot`'s editor left open (its window
    /// closed); it becomes an undo step at the next timer tick.
    pub fn end_plugin_gestures(&self, slot: u64) -> Result<(), VizError> {
        ffi_guard(|| {
            plugins::with(self.id, |p| {
                if let Some(p) = p {
                    p.end_gestures(PluginInstanceId(slot));
                }
            })
        })
    }
}

/// Mixer reads and edits (spec §11). Every edit is one undo step; a drag
/// (fader, pan, send level) is bracketed by `begin_edit` / `end_edit` and
/// becomes one step.
#[uniffi::export]
impl Session {
    /// The mixer: strips in display order (master last) with their slots.
    pub fn mixer(&self) -> Result<MixerSnapshot, VizError> {
        ffi_guard(|| {
            // A copy, so the session lock is not held while the registry is.
            let (doc, version) = {
                let inner = lock(&self.inner);
                (inner.doc().clone(), inner.version)
            };
            plugins::with(self.id, |p| mixer::snapshot(&doc, version, p))
        })
    }

    /// Meters, DSP load, latency and fault-bypassed plugins (empty unless
    /// this session holds the engine). The mixer polls it at 30 Hz.
    pub fn levels(&self) -> Result<MixerLevels, VizError> {
        ffi_guard(|| Ok(host::levels(self.id).unwrap_or_else(MixerLevels::empty)))
    }

    /// Opens a continuous edit (a drag): commands until `end_edit` are one
    /// undo step named `label`, and each is heard at once.
    pub fn begin_edit(&self, label: String) -> Result<(), VizError> {
        ffi_guard(|| Ok(lock(&self.inner).history.begin(label)?))
    }

    /// Closes the edit `begin_edit` opened (nothing happens when none is
    /// open).
    pub fn end_edit(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if inner.history.in_transaction() {
                inner.history.commit()?;
                inner.version += 1;
            }
            self.commit_held(&mut inner);
            inner.snapshot()
        })
    }

    /// Adds an aux bus at the end.
    pub fn add_bus(&self, name: String) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| self.execute(Command::Mixer(MixerCmd::AddBus { name, index: None })))
    }

    /// Removes a bus (outputs to it go to master, sends to it go).
    pub fn remove_bus(&self, strip: u64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::RemoveBus {
                strip: StripId(strip),
            }))
        })
    }

    /// Renames a strip.
    pub fn rename_strip(&self, strip: u64, name: String) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::RenameStrip {
                strip: StripId(strip),
                name,
            }))
        })
    }

    /// Moves a strip (not master) to display position `index`.
    pub fn move_strip(&self, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::MoveStrip {
                strip: StripId(strip),
                index: index as usize,
            }))
        })
    }

    /// Sets a fader (dB; −144 is −∞, at most +12).
    pub fn set_gain(&self, strip: u64, gain_db: f64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetGain {
                strip: StripId(strip),
                gain_db,
            }))
        })
    }

    /// Sets a pan (−1 … +1).
    pub fn set_pan(&self, strip: u64, pan: f64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetPan {
                strip: StripId(strip),
                pan,
            }))
        })
    }

    /// Mutes or unmutes a strip.
    pub fn set_strip_mute(&self, strip: u64, mute: bool) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetMute {
                strip: StripId(strip),
                mute,
            }))
        })
    }

    /// Solos or unsolos a strip.
    pub fn set_strip_solo(&self, strip: u64, solo: bool) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetSolo {
                strip: StripId(strip),
                solo,
            }))
        })
    }

    /// Routes a strip's output to bus `bus`, or to master (`None`).
    pub fn set_output(&self, strip: u64, bus: Option<u64>) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetOutput {
                strip: StripId(strip),
                output: bus.map_or(Output::Master, |b| Output::Bus(StripId(b))),
            }))
        })
    }

    /// Adds a post-fader send at 0 dB to bus `target`.
    pub fn add_send(&self, strip: u64, target: u64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::AddSend {
                strip: StripId(strip),
                send: AuxSend::new(StripId(target)),
            }))
        })
    }

    /// Changes send `index` of a strip.
    pub fn edit_send(
        &self,
        strip: u64,
        index: u32,
        send: crate::mixer::SendRow,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::EditSend {
                strip: StripId(strip),
                index: index as usize,
                send: AuxSend {
                    target: StripId(send.target),
                    level_db: send.level_db,
                    pre_fader: send.pre_fader,
                    enabled: send.enabled,
                },
            }))
        })
    }

    /// Removes send `index` of a strip.
    pub fn remove_send(&self, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::RemoveSend {
                strip: StripId(strip),
                index: index as usize,
            }))
        })
    }

    /// Inserts `plugin` at `index` of a strip's chain (default: the end);
    /// it starts loading at once.
    pub fn add_insert(
        &self,
        strip: u64,
        index: Option<u32>,
        plugin: PluginRefRow,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::AddInsert {
                strip: StripId(strip),
                index: index.map(|i| i as usize),
                plugin: (&plugin).into(),
            }))
        })
    }

    /// Removes an insert (its state is captured first, so Undo restores
    /// it).
    pub fn remove_insert(&self, slot: u64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::RemoveInsert {
                slot: PluginInstanceId(slot),
            }))
        })
    }

    /// Moves an insert to position `index` of `strip` (drag to reorder).
    pub fn move_insert(&self, slot: u64, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::MoveInsert {
                slot: PluginInstanceId(slot),
                strip: StripId(strip),
                index: index as usize,
            }))
        })
    }

    /// Bypasses a plugin, or turns it back on.
    pub fn set_bypass(&self, slot: u64, bypass: bool) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetBypass {
                slot: PluginInstanceId(slot),
                bypass,
            }))
        })
    }

    /// Feeds an insert's sidechain from strip `source` (post-fader), or
    /// from nothing.
    pub fn set_sidechain(&self, slot: u64, source: Option<u64>) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetSidechain {
                slot: PluginInstanceId(slot),
                source: source.map(StripId),
            }))
        })
    }

    /// Adds a strip for extra output `index` of a multi-output plugin
    /// instrument ("Add Outputs…").
    pub fn add_instrument_output(
        &self,
        dest: u64,
        index: u16,
        name: String,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::AddInstrumentOutput {
                dest: DestId(dest),
                index,
                name,
            }))
        })
    }

    /// Adds a plugin instrument destination named `name` (and its
    /// instrument strip): "New Plugin Instrument…".
    pub fn add_plugin_instrument(
        &self,
        name: String,
        plugin: PluginRefRow,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Routing(RoutingCmd::AddDestination {
                name,
                kind: DestKind::PluginInstrument {
                    slot: PluginSlot::new(PluginInstanceId(0), (&plugin).into()),
                },
                default_patch: None,
            }))
        })
    }

    /// The instrument list: every destination, with the plugin it plays
    /// (none: a MIDI endpoint or the built-in synth).
    pub fn destinations(&self) -> Result<Vec<DestRow>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            Ok(inner
                .doc()
                .instruments()
                .values()
                .map(|d| DestRow {
                    id: d.id.0,
                    name: d.name.clone(),
                    plugin: match &d.kind {
                        DestKind::PluginInstrument { slot } => Some((&slot.plugin).into()),
                        DestKind::MidiEndpoint { .. } => None,
                    },
                })
                .collect())
        })
    }

    /// Makes destination `dest` play `plugin` (every lane using it now
    /// plays the plugin; one undo step). The same plugin keeps its
    /// instance and state (3a Plan Ruling 10).
    pub fn set_destination_plugin(
        &self,
        dest: u64,
        plugin: PluginRefRow,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let default_patch = lock(&self.inner)
                .doc()
                .instruments()
                .get(&DestId(dest))
                .ok_or_else(|| VizError::Model {
                    message: format!("unknown id {}", DestId(dest)),
                })?
                .default_patch;
            self.execute(Command::Routing(RoutingCmd::EditDestination {
                dest: DestId(dest),
                kind: DestKind::PluginInstrument {
                    slot: PluginSlot::new(PluginInstanceId(0), (&plugin).into()),
                },
                default_patch,
            }))
        })
    }
}

/// The package file holding which plugin windows were open (not part of
/// the document: no undo, Plan Ruling 13).
pub const UI_FILE: &str = "ui.json";

fn read_windows(package: &Path) -> std::collections::BTreeSet<u64> {
    std::fs::read_to_string(package.join(UI_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<UiState>(&t).ok())
        .map(|u| u.open_plugin_windows.into_iter().collect())
        .unwrap_or_default()
}

/// Best-effort: a project whose window list cannot be written still saved.
fn write_windows(package: &Path, windows: &std::collections::BTreeSet<u64>) {
    let ui = UiState {
        open_plugin_windows: windows.iter().copied().collect(),
    };
    if let Ok(text) = serde_json::to_string_pretty(&ui) {
        let _ = file::write_atomic(&package.join(UI_FILE), text.as_bytes());
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UiState {
    open_plugin_windows: Vec<u64>,
}

/// Plugin windows (spec §9): the plugin's own editor inside a window the
/// app owns, and which windows are open (they reopen with the project).
#[uniffi::export]
impl Session {
    /// Shows slot `slot`'s editor inside the `NSView` at address `parent`
    /// (main thread; the view lives until `close_editor`). Fails with
    /// "editor unavailable" when the plugin has none (the app then shows
    /// its parameters).
    ///
    /// RE-ENTRANCY CONTRACT: an AU v3 editor spins the main run loop (up to
    /// about 3 s) inside the plugin while it opens, so the app's 60 Hz timer
    /// and other main-queue work call back into core-ffi meanwhile. This
    /// function therefore holds NO session `inner` lock (nor any other mutex
    /// a nested FFI call could take) across the open. Only the plugin
    /// registry borrow of `plugins::with` is held; nested calls get
    /// `VizError::Busy` and idle skips.
    pub fn open_editor(&self, slot: u64, parent: u64) -> Result<EditorSize, VizError> {
        ffi_guard(|| {
            let parent = editor::parent(parent)?;
            plugins::with(self.id, |p| {
                let ed = p
                    .and_then(|p| p.instance(PluginInstanceId(slot)))
                    .and_then(|i| i.editor())
                    .ok_or_else(|| unavailable("the plugin has no editor"))?;
                let (width, height) = ed.open(parent).map_err(|e| unavailable(&e.to_string()))?;
                Ok(EditorSize {
                    width,
                    height,
                    resizable: ed.can_resize(),
                })
            })
            .and_then(|r| r)
        })
    }

    /// Asks slot `slot`'s editor for a new size; returns what it took.
    pub fn resize_editor(
        &self,
        slot: u64,
        width: u32,
        height: u32,
    ) -> Result<EditorSize, VizError> {
        ffi_guard(|| {
            plugins::with(self.id, |p| {
                let ed = p
                    .and_then(|p| p.instance(PluginInstanceId(slot)))
                    .and_then(|i| i.editor())
                    .ok_or_else(|| unavailable("the plugin has no editor"))?;
                let (width, height) = ed
                    .resize(width, height)
                    .map_err(|e| unavailable(&e.to_string()))?;
                Ok(EditorSize {
                    width,
                    height,
                    resizable: ed.can_resize(),
                })
            })
            .and_then(|r| r)
        })
    }

    /// Hides slot `slot`'s editor (its window is closing) and ends any
    /// gesture it left open. Like `open_editor`, holds no session lock
    /// while the plugin runs (closing may also run the main loop).
    ///
    /// Returns VizError::Busy while another editor is opening (nested run
    /// loop); the caller must keep the parent view alive and retry until
    /// this returns Ok.
    pub fn close_editor(&self, slot: u64) -> Result<(), VizError> {
        ffi_guard(|| {
            plugins::with(self.id, |p| {
                if let Some(p) = p {
                    if let Some(ed) = p.instance(PluginInstanceId(slot)).and_then(|i| i.editor()) {
                        ed.close();
                    }
                    p.end_gestures(PluginInstanceId(slot));
                }
            })
        })
    }

    /// Records that slot `slot`'s window is open or closed (saved with the
    /// project).
    pub fn set_window_open(&self, slot: u64, open: bool) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if open {
                inner.windows.insert(slot);
            } else {
                inner.windows.remove(&slot);
            }
            Ok(())
        })
    }

    /// The plugin windows to open now: those open when the project was
    /// saved whose slot still exists.
    pub fn open_windows(&self) -> Result<Vec<u64>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let slots = mixer::slot_ids(inner.doc());
            Ok(inner
                .windows
                .iter()
                .copied()
                .filter(|w| slots.contains(&PluginInstanceId(*w)))
                .collect())
        })
    }
}

fn unavailable(why: &str) -> VizError {
    VizError::Engine {
        message: format!("editor unavailable: {why}"),
    }
}

/// "Strip — Plugin" for messages about slot `id`.
fn slot_name(doc: &Document, id: core_model::ids::PluginInstanceId) -> String {
    use core_model::document::SlotOwner;
    let plugin = doc
        .plugin_slot(id)
        .map_or_else(|| id.to_string(), |s| s.plugin.name.clone());
    let place = match doc.find_slot(id) {
        Some(SlotOwner::Insert(strip, _)) => doc.mixer().strip(strip).ok().map(|s| s.name.clone()),
        Some(SlotOwner::Instrument(d)) => doc.instruments().get(&d).map(|d| d.name.clone()),
        None => None,
    };
    match place {
        Some(p) => format!("{p} — {plugin}"),
        None => plugin,
    }
}

#[cfg(feature = "test-hooks")]
#[uniffi::export]
impl Session {
    /// The sample rate this session's plugins are activated for (test
    /// hook).
    pub fn debug_plugin_rate(&self) -> Result<f64, VizError> {
        ffi_guard(|| plugins::with(self.id, |p| p.map_or(0.0, |p| p.config().0)))
    }

    /// Sets parameter `param` of slot `slot` as one undo step, like a
    /// plugin-UI gesture (test hook).
    pub fn debug_set_slot_param(
        &self,
        slot: u64,
        param: u64,
        value: f64,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetPluginParams {
                slot: PluginInstanceId(slot),
                params: vec![(core_model::mixer::ParamId(param), value)],
            }))
        })
    }

    /// Slot `slot`'s state blob in the document, if it has one (test
    /// hook).
    pub fn debug_slot_state(&self, slot: u64) -> Result<Option<u64>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            Ok(inner
                .doc()
                .plugin_slot(PluginInstanceId(slot))
                .and_then(|s| s.state)
                .map(|b| b.0))
        })
    }

    /// Panics while holding the session lock (test hook): poisons it.
    pub fn debug_panic_locked(&self) -> Result<(), VizError> {
        ffi_guard(|| -> Result<(), VizError> {
            let _inner = lock(&self.inner);
            #[allow(clippy::panic)]
            {
                panic!("panic while locked")
            }
        })
    }
}

/// Safe mode (spec §10) and closing.
#[uniffi::export]
impl Session {
    /// Loads held slots (safe mode's "Load" on a slot, "Load All" on a
    /// strip or the document): they load from their saved state. `Busy`
    /// when a plugin call is on the stack (nothing changes).
    pub fn load_plugins(&self, slots: Vec<u64>) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            plugins::with(self.id, |p| {
                if let Some(p) = p {
                    for s in slots {
                        p.hold(PluginInstanceId(s), None);
                    }
                }
            })?;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            inner.news.slots_changed = true;
            Ok(())
        })
    }

    /// The slots safe mode holds (placeholders until loaded).
    pub fn held_slots(&self) -> Result<Vec<u64>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let slots = mixer::slot_ids(inner.doc());
            Ok(
                plugins::with(self.id, |p| p.map(|p| p.held()).unwrap_or_default())?
                    .into_iter()
                    .filter(|s| slots.contains(s))
                    .map(|s| s.0)
                    .collect(),
            )
        })
    }

    /// A clean close (the window closes, or the app quits): the crash
    /// marker goes. Safe to call more than once.
    pub fn close(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.closed = true;
            if let Some(k) = inner.key.clone() {
                recovery::closed(&k, self.id);
            }
            Ok(())
        })
    }
}

/// How to open a project after a crash or with a newer autosave.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OpenOptions {
    /// Hold every plugin slot ("Open with Plugins Off").
    pub plugins_off: bool,
    /// Open this autosave copy instead of `project.json`.
    pub autosave: Option<String>,
}

/// Leaves session `s` the way a crash would: its crash marker stays and it
/// no longer counts as open here (test hook).
#[cfg(feature = "test-hooks")]
pub fn simulate_crash(s: Arc<Session>) {
    if let Some(p) = lock(&s.inner).path.clone() {
        recovery::forget_open(&p);
    }
    std::mem::forget(s);
}

/// Editor reads (spec §3.1).
#[uniffi::export]
impl Session {
    /// The active take of lane `lane` of sequence `seq` as rows in time
    /// order, with the lane's revision.
    pub fn lane_events(&self, seq: u64, lane: u64) -> Result<crate::events::LaneEvents, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let rev = inner.lane_rev(SeqId(seq), LaneId(lane));
            crate::events::lane_events(inner.doc(), SeqId(seq), LaneId(lane), rev)
        })
    }

    /// Tempo, meter, markers, start offset and length of sequence `seq`.
    pub fn sequence_info(&self, seq: u64) -> Result<crate::events::SequenceInfo, VizError> {
        ffi_guard(|| crate::events::sequence_info(lock(&self.inner).doc(), SeqId(seq)))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(k) = lock(&self.inner).key.clone() {
            recovery::closed(&k, self.id);
        }
        // The engine goes first when this session's plugin host feeds it;
        // queued if a plugin call is running (re-entrancy, plugins.rs).
        plugins::close_session(self.id);
    }
}
