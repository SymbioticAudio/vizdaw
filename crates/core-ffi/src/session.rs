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
    /// The label of the edit `begin_edit` opened (`edit_gesture` reopens
    /// it under the same label).
    edit_label: Option<String>,
    /// The editors' layout, opaque JSON from the app, saved in `ui.json`
    /// (plan 4a Ruling 12).
    editor_layout: Option<serde_json::Value>,
    /// Offline media and why (Ruling 22; runtime only, never saved).
    offline: std::collections::BTreeMap<core_model::ids::MediaId, crate::media::OfflineReasonRow>,
    /// The document whose media and format `offline` was computed for, and
    /// the root it was checked under.
    media_seen: Option<(Document, std::path::PathBuf)>,
    /// Imports and conversions (Ruling 15).
    jobs: crate::jobs::Jobs,
    /// Waveform overviews and their worker (plan 5b Ruling 3).
    overviews: crate::overviews::Overviews,
    /// Items whose file failed the content-hash check, with the path and
    /// hash it was checked for (plan 5b Ruling 2; runtime only).
    changed: std::collections::BTreeMap<core_model::ids::MediaId, (String, Option<String>)>,
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
    /// A job was applied outside `idle` (when an edit closed): the next
    /// `idle` reports `media_changed`.
    media_changed: bool,
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
            edit_label: None,
            editor_layout: None,
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
            offline: Default::default(),
            media_seen: None,
            jobs: Default::default(),
            overviews: Default::default(),
            changed: Default::default(),
        }
    }

    /// The media root: the package, or the untitled scratch package.
    fn root(&self, id: u64) -> std::path::PathBuf {
        self.path
            .clone()
            .unwrap_or_else(|| crate::media::scratch_root(id))
    }

    /// Recomputes the offline set when the media pool, the format or the
    /// root changed (or `force`), and hands the online media to the
    /// engine host and to the overview worker (which re-verifies every
    /// item when `force`). True when the offline set changed.
    fn refresh_media(&mut self, id: u64, force: bool) -> bool {
        self.refresh_media_with(id, force, force)
    }

    /// [`Inner::refresh_media`]; `reverify` makes the overview worker
    /// check every item again (a recheck), not only new ones.
    fn refresh_media_with(&mut self, id: u64, force: bool, reverify: bool) -> bool {
        let root = self.root(id);
        let doc = self.history.doc();
        let same = !force
            && self.media_seen.as_ref().is_some_and(|(d, r)| {
                *r == root
                    && d.settings().format == doc.settings().format
                    && (d.media().ptr_eq(doc.media()) || d.media() == doc.media())
            });
        if same {
            return false;
        }
        let mut offline = crate::media::check(doc, &root);
        // A file that failed the hash check stays offline while the item
        // still names that file and hash (plan 5b Ruling 2).
        for (m, (path, hash)) in &self.changed {
            if let Some(item) = doc.media().get(m) {
                if item.path == *path && item.content_hash == *hash {
                    offline
                        .entry(*m)
                        .or_insert(crate::media::OfflineReasonRow::Changed);
                }
            }
        }
        let changed = offline != self.offline;
        host::set_media(id, crate::media::files(doc, &root, &offline));
        // Overviews of online items, and of changed ones so a returning
        // original is noticed.
        let wants = doc
            .media()
            .values()
            .filter(|m| match offline.get(&m.id) {
                None | Some(crate::media::OfflineReasonRow::Changed) => true,
                Some(_) => false,
            })
            .map(|m| crate::overviews::Want {
                media: m.id,
                path: m.path.clone(),
                file: root.join(&m.path),
                hash: m.content_hash.clone(),
                package: root.clone(),
            })
            .collect();
        self.overviews.ensure(wants, reverify);
        self.offline = offline;
        self.media_seen = Some((doc.clone(), root));
        changed
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
        // A format change (or its undo) moved the project rate: the host
        // this session plays through goes, so the next Play asks the
        // device for the new rate (Ruling 20).
        host::reset_if_owner(id, f64::from(self.doc().settings().format.sample_rate));
        self.refresh_media(id, false);
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
            inner.refresh_media(id, true);
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
        let refs = inner.history.referenced_media_anywhere();
        crate::media::copy_media(&refs, &inner.root(self.id), path)?;
        crate::overviews::copy_cache(&refs, &inner.root(self.id), path);
        file::save(inner.doc(), path)?;
        write_windows(path, &inner.windows, inner.editor_layout.as_ref());
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
        self.execute_with(cmd, &ViewFilter::default())
    }

    /// Modify Again (Plan 4c Ruling 8): the last modification on the
    /// current selection, one undo step named after it. It removes no
    /// plugin, so nothing is captured first.
    fn modify_again_with(&self, filter: &ViewFilter) -> Result<DocSnapshot, VizError> {
        let mut inner = lock(&self.inner);
        inner.history.modify_again(filter)?;
        inner.version += 1;
        inner.reload_host(self.id);
        self.mark(&mut inner);
        inner.snapshot()
    }

    /// [`Session::execute`] with a view filter (editor commands that act
    /// on the selection).
    fn execute_with(&self, cmd: Command, filter: &ViewFilter) -> Result<DocSnapshot, VizError> {
        let next = core_model::command::apply(lock(&self.inner).doc(), &cmd)
            .ok()
            .map(|n| n.doc);
        if let Some(next) = next {
            self.capture_removed(&next)?;
        }
        let mut inner = lock(&self.inner);
        inner.history.execute(&cmd, filter)?;
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

    /// Plays `seq` from `start`, else from where this session's position
    /// is (a position set while another session held the engine), up to
    /// `until` or the end.
    fn start_playing(
        &self,
        seq: u64,
        start: Option<Tick>,
        until: Option<Tick>,
    ) -> Result<(), VizError> {
        let mut inner = lock(&self.inner);
        let id = seq_id(inner.doc(), seq)?;
        let start = start.or(match (inner.cursor, inner.cursor_seq) {
            (Some(t), None) => Some(t),
            (Some(t), Some(s)) if s == id => Some(t),
            _ => None,
        });
        let looped = inner.looped;
        inner.mark(self.id, true);
        let played = plugins::with(self.id, |p| {
            host::play(self.id, inner.doc(), id, start, until, looped, p)
        })
        .and_then(|r| r);
        let pending = match played {
            Ok(p) => p,
            Err(e) => {
                // Retried by `idle` (the engine may hold stale plugins).
                inner.reload_owed = true;
                return Err(e);
            }
        };
        inner.reload_owed = false;
        inner.reload_fault = None;
        inner.cursor = None;
        inner.cursor_seq = Some(id);
        // The disk wait (up to 250 ms) holds no lock: this session's
        // transport poll and every other session go on meanwhile.
        drop(inner);
        host::finish_play(pending)
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

    /// An untitled copy of the demo project with an audio lane (4b Plan
    /// Ruling 19).
    #[uniffi::constructor]
    pub fn new_demo_audio() -> Result<Arc<Session>, VizError> {
        ffi_guard(|| {
            let doc = core_engine::fixtures::demo_audio_document()?;
            Session::with(doc, None, false)
        })
    }

    /// An untitled, empty project in `format` (the New Project sheet;
    /// spec §4.1).
    #[uniffi::constructor]
    pub fn new_project(format: crate::format::ProjectFormatRow) -> Result<Arc<Session>, VizError> {
        ffi_guard(|| {
            let f = core_model::format::AudioFormat::from(format);
            let mut doc = Document::new();
            if f != doc.settings().format {
                doc = core_model::command::apply(
                    &doc,
                    &Command::Settings(core_model::command::SettingsCmd::SetFormat {
                        format: f,
                        swaps: Vec::new(),
                    }),
                )?
                .doc;
            }
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
            crate::media::sweep_temp(&p);
            let windows = read_windows(&p);
            let layout = read_layout(&p);
            let s = Session::with(doc, Some(p), options.plugins_off)?;
            {
                let mut inner = lock(&s.inner);
                inner.windows = windows;
                inner.editor_layout = layout;
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
    /// `Busy` while an import or conversion has not been applied: its
    /// files are under the current media root, which Save As moves away
    /// from (plan 5a Ruling 15).
    pub fn save_as(&self, path: String) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            // A job that just finished applies first, under the old root.
            self.apply_jobs(&mut inner);
            if inner.jobs.busy() {
                return Err(VizError::Busy {
                    message: "Wait for the import or conversion to finish, then save.".into(),
                });
            }
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
                // The scratch package of an untitled session is done with.
                let _ = std::fs::remove_dir_all(crate::media::scratch_root(self.id));
                inner.refresh_media(self.id, true);
                inner.reload_host(self.id);
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

    /// The drum lanes of sequence `seq` with their rows and each row's
    /// notes in the active take (the drum grid, 4e).
    pub fn drum_lanes(&self, seq: u64) -> Result<Vec<crate::drum::DrumLaneRow>, VizError> {
        ffi_guard(|| crate::drum::drum_lanes(lock(&self.inner).doc(), seq))
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
        self.transport_call(|| self.start_playing(seq, None, None))
    }

    /// Plays sequence `seq` from `from` (plan 4f Ruling 9): on to its end,
    /// or up to `to`, where it stops (the loop ignored). `Model` error
    /// unless `from < to` and both are within ±2^40 ticks.
    pub fn play_from(&self, seq: u64, from: i64, to: Option<i64>) -> Result<(), VizError> {
        self.transport_call(|| {
            core_model::rules::check_ticks("from", from)?;
            if let Some(to) = to {
                core_model::rules::check_ticks("to", to)?;
                if to <= from {
                    return Err(crate::events::bad("the end must be after the start"));
                }
            }
            self.start_playing(seq, Some(Tick(from)), to.map(Tick))
        })
    }

    /// Where Play Selection plays in `seq`: the selection's range, else
    /// the span of its selected events in `seq` (plan 4f Ruling 9); None
    /// when nothing of `seq` is selected.
    pub fn selection_span(
        &self,
        seq: u64,
    ) -> Result<Option<crate::selection::TickRange>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let id = seq_id(inner.doc(), seq)?;
            let span =
                core_editor::transport::selection_span(inner.doc(), inner.history.selection(), id);
            Ok(span.map(|(a, b)| crate::selection::TickRange {
                start: a.0,
                end: b.0,
            }))
        })
    }

    /// The nearest marker of `seq` after (`forward`) or before `tick`.
    pub fn marker_step(&self, seq: u64, tick: i64, forward: bool) -> Result<Option<i64>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let s = inner.doc().sequence(seq_id(inner.doc(), seq)?)?;
            Ok(core_editor::transport::marker_step(s, Tick(tick), forward).map(|t| t.0))
        })
    }

    /// Moves record arm to the next (`forward`) or previous lane of `seq`
    /// as one undo step "Move Record Arm" (plan 4f Ruling 10). `Model`
    /// error at the first or last lane.
    pub fn move_record_arm(&self, seq: u64, forward: bool) -> Result<DocSnapshot, VizError> {
        let (lanes, m) = ffi_guard(|| {
            let inner = lock(&self.inner);
            let s = inner.doc().sequence(seq_id(inner.doc(), seq)?)?;
            let armed: Vec<bool> = s.lanes.iter().map(|l| l.arm).collect();
            let ids: Vec<u64> = s.lanes.iter().map(|l| l.id.0).collect();
            let m = core_editor::transport::arm_move(&armed, forward).ok_or_else(|| {
                crate::events::bad(if forward {
                    "there is no lane after the armed one"
                } else {
                    "there is no lane before the armed one"
                })
            })?;
            Ok((ids, m))
        })?;
        let lane = |i: usize| lanes.get(i).copied().unwrap_or(0);
        let flags = |i: usize, arm: bool| crate::edit::EditCommand::Lane {
            edit: crate::edit::LaneEdit::SetFlags {
                seq,
                lane: lane(i),
                mute: None,
                solo: None,
                arm: Some(arm),
            },
        };
        let mut cmds: Vec<_> = m
            .disarm
            .iter()
            .filter(|i| **i != m.arm)
            .map(|i| flags(*i, false))
            .collect();
        cmds.push(flags(m.arm, true));
        self.edit_all("Move Record Arm".into(), cmds, None)
    }

    /// Stops; the position is kept.
    pub fn stop(&self) -> Result<(), VizError> {
        self.transport_call(|| {
            // Behind a rebuild this session's poll is running (its drain
            // has taken the host out of the slot): the Stop must not be
            // lost to an empty slot.
            let _inner = lock(&self.inner);
            host::stop(self.id)
        })
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

    /// Sounds (`on`) or releases `pitch` on destination `dest` without
    /// touching the document (spec §3.2). True when it was sent; false
    /// when this session does not hold the engine or the loaded sequence
    /// does not play on `dest`'s target (Plan Ruling 9).
    pub fn audition(
        &self,
        dest: u64,
        pitch: u8,
        velocity: u16,
        on: bool,
    ) -> Result<bool, VizError> {
        self.transport_call(|| {
            let inner = lock(&self.inner);
            host::audition(self.id, inner.doc(), DestId(dest), pitch, velocity, on)
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
                    host_time_ns: h.host_time_ns,
                    ticks_per_second: crate::clock::ticks_per_second(doc, Some(h.seq), h.tick),
                    rate_mismatch: h.rate_mismatch,
                },
                None => {
                    let tick = inner.cursor.unwrap_or(Tick::ZERO);
                    let host_time_ns = crate::clock::now_ns();
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
                        host_time_ns,
                        ticks_per_second: crate::clock::ticks_per_second(doc, seq, tick),
                        rate_mismatch: false,
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
                media_changed: false,
                overviews_changed: false,
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
            // Jobs applied when an edit closed (outside `idle`) count too.
            let owed = std::mem::take(&mut inner.news.media_changed);
            let jobs_changed = self.apply_jobs(&mut inner);
            let mut stream_failed = false;
            for (m, why) in host::take_stream_errors(self.id) {
                inner
                    .offline
                    .insert(m, crate::media::OfflineReasonRow::Unreadable);
                inner
                    .news
                    .problems
                    .push(format!("media {m} went offline: {why}"));
                stream_failed = true;
            }
            if stream_failed {
                // The failed items leave the engine's table (also after a
                // job reloaded it above), so a recheck that finds them
                // back lists them again and the streamer reopens them
                // (final review I2).
                let root = inner.root(self.id);
                let files = crate::media::files(inner.doc(), &root, &inner.offline);
                host::set_media(self.id, files);
                inner.reload_host(self.id);
            }
            // What the overview worker found (plan 5b Ruling 2).
            let found = inner.overviews.take_news();
            let mut hash_news = false;
            for (m, path, hash) in found.mismatched {
                if inner.changed.get(&m) == Some(&(path.clone(), hash.clone())) {
                    continue;
                }
                let name = inner
                    .doc()
                    .media()
                    .get(&m)
                    .map_or_else(|| m.to_string(), |x| x.name.clone());
                inner.news.problems.push(format!(
                    "{name}: the file no longer holds the audio this project expects; it stays silent until it is relinked or restored"
                ));
                inner.changed.insert(m, (path, hash));
                hash_news = true;
            }
            for (m, path) in found.verified {
                if inner.changed.get(&m).is_some_and(|(p, _)| *p == path) {
                    inner.changed.remove(&m);
                    hash_news = true;
                }
            }
            if hash_news {
                // Not a re-verification: a still-changed file is not read
                // again in a loop.
                inner.refresh_media_with(self.id, true, false);
                inner.reload_host(self.id);
            }
            let media_changed = jobs_changed || owed || stream_failed || hash_news;
            let changed = changed || jobs_changed;
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
                media_changed,
                overviews_changed: found.changed,
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
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.history.begin(label.clone())?;
            inner.edit_label = Some(label);
            Ok(())
        })
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
            inner.edit_label = None;
            self.commit_held(&mut inner);
            self.apply_jobs_after_edit(&mut inner);
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

fn read_ui(package: &Path) -> Option<UiState> {
    std::fs::read_to_string(package.join(UI_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<UiState>(&t).ok())
}

fn read_windows(package: &Path) -> std::collections::BTreeSet<u64> {
    read_ui(package)
        .map(|u| u.open_plugin_windows.into_iter().collect())
        .unwrap_or_default()
}

fn read_layout(package: &Path) -> Option<serde_json::Value> {
    read_ui(package).and_then(|u| u.editor_layout)
}

/// Best-effort: a project whose window list cannot be written still saved.
fn write_windows(
    package: &Path,
    windows: &std::collections::BTreeSet<u64>,
    layout: Option<&serde_json::Value>,
) {
    let ui = UiState {
        open_plugin_windows: windows.iter().copied().collect(),
        editor_layout: layout.cloned(),
    };
    if let Ok(text) = serde_json::to_string_pretty(&ui) {
        let _ = file::write_atomic(&package.join(UI_FILE), text.as_bytes());
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UiState {
    open_plugin_windows: Vec<u64>,
    /// The editors' layout (plan 4a); older files have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    editor_layout: Option<serde_json::Value>,
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

    /// Holds this session's job threads before they start (`true`), or
    /// lets them go (test hook).
    pub fn debug_hold_jobs(&self, on: bool) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).jobs.set_held(on);
            Ok(())
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
            let root = inner.root(self.id);
            for w in inner.jobs.shutdown() {
                discard(&root, w);
            }
            inner.overviews.shutdown();
            host::forget_media(self.id);
            if inner.path.is_none() {
                let _ = std::fs::remove_dir_all(crate::media::scratch_root(self.id));
            }
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

    /// Seconds from tick 0 of sequence `seq` at each tick, through its own
    /// tempo map (the list's SMPTE column, 4c).
    pub fn seconds_at(&self, seq: u64, ticks: Vec<i64>) -> Result<Vec<f64>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let tempo = &inner.doc().sequence(SeqId(seq))?.tempo;
            Ok(ticks
                .into_iter()
                .map(|t| tempo.tick_to_seconds(core_model::time::Tick(t)))
                .collect())
        })
    }

    /// The nearest tick at each time in seconds (typed SMPTE positions, 4c).
    pub fn ticks_at(&self, seq: u64, seconds: Vec<f64>) -> Result<Vec<i64>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let tempo = &inner.doc().sequence(SeqId(seq))?.tempo;
            Ok(seconds
                .into_iter()
                .map(|s| tempo.seconds_to_tick(s).0)
                .collect())
        })
    }

    /// The select-and-modify templates, by name (4c).
    pub fn modify_templates(&self) -> Result<Vec<crate::modify::ModifyTemplateRow>, VizError> {
        ffi_guard(|| Ok(crate::modify::template_rows(lock(&self.inner).doc())))
    }

    /// The modification Modify Again repeats, if any (4c).
    pub fn last_modify(&self) -> Result<Option<crate::modify::ModifyRow>, VizError> {
        ffi_guard(|| {
            Ok(lock(&self.inner)
                .history
                .last_modify()
                .map(crate::modify::modify_row))
        })
    }

    /// Tempo, meter, markers, start offset and length of sequence `seq`.
    pub fn sequence_info(&self, seq: u64) -> Result<crate::events::SequenceInfo, VizError> {
        ffi_guard(|| crate::events::sequence_info(lock(&self.inner).doc(), SeqId(seq)))
    }

    /// The arrangement blocks of lane `lane` under `rule` (spec §3.1).
    pub fn blocks(
        &self,
        seq: u64,
        lane: u64,
        rule: crate::blocks::BlockRule,
    ) -> Result<Vec<crate::blocks::BlockRow>, VizError> {
        ffi_guard(|| crate::blocks::blocks(lock(&self.inner).doc(), SeqId(seq), LaneId(lane), rule))
    }
}

/// Test hooks that are not part of the app's interface.
#[cfg(feature = "test-hooks")]
impl Session {
    /// True when every job thread has ended, applied or not (test hook).
    pub fn debug_jobs_settled(&self) -> bool {
        lock(&self.inner).jobs.settled()
    }

    /// True when the overview worker has nothing to do (test hook).
    pub fn debug_overviews_idle(&self) -> bool {
        lock(&self.inner).overviews.idle()
    }

    /// Holds the overview worker (`true`) or lets it go (test hook).
    pub fn debug_hold_overviews(&self, on: bool) {
        lock(&self.inner).overviews.set_held(on);
    }

    /// Makes the overview worker's next task panic (test hook, preflight
    /// M10).
    pub fn debug_panic_overviews(&self) {
        lock(&self.inner).overviews.panic_next();
    }
}

#[cfg(feature = "test-hooks")]
thread_local! {
    /// The `edit_all` that panics (test hook): its label and the 0-based
    /// command it panics before. Per thread (final review m8): tests run
    /// in parallel threads and each calls `edit_all`/`step_entry` on its
    /// own, so one test's injection never reaches another's call.
    static EDIT_ALL_PANIC: std::cell::RefCell<Option<(String, usize)>> =
        const { std::cell::RefCell::new(None) };
}

/// Makes the next `edit_all` (or step entry edit) named `label` called on
/// this thread panic before command `step` (0-based); `None` clears it
/// (test hook, Plan 4d Ruling 1).
#[cfg(feature = "test-hooks")]
pub fn inject_edit_all_panic(label: Option<&str>, step: usize) {
    EDIT_ALL_PANIC.with(|p| *p.borrow_mut() = label.map(|l| (l.to_string(), step)));
}

/// Panics when `edit_all` `label` reaches the injected `step`.
#[cfg(feature = "test-hooks")]
#[allow(clippy::panic)]
fn edit_all_hook(label: &str, step: usize) {
    let hit = EDIT_ALL_PANIC.with(|p| {
        p.borrow()
            .as_ref()
            .is_some_and(|(l, s)| l == label && *s == step)
    });
    if hit {
        panic!("injected edit_all panic");
    }
}

#[cfg(not(feature = "test-hooks"))]
fn edit_all_hook(_label: &str, _step: usize) {}

/// Editor edits (spec §3.2, §3.8).
#[uniffi::export]
impl Session {
    /// Applies an editor command as one undo step. `filter` hides kinds and
    /// lanes from commands that act on the selection. `Model` error while an
    /// edit `begin_edit` opened is in progress (a drag, a fader): joining it
    /// would merge the command into that undo step, or lose it on abort.
    pub fn edit(
        &self,
        cmd: crate::edit::EditCommand,
        filter: Option<crate::edit::ViewFilterRow>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            if lock(&self.inner).edit_label.is_some() {
                return Err(crate::events::bad("finish the open edit first"));
            }
            let filter = crate::edit::view_filter(filter.as_ref());
            if matches!(cmd, crate::edit::EditCommand::ModifyAgain) {
                return self.modify_again_with(&filter);
            }
            if let crate::edit::EditCommand::Settings { edit } = &cmd {
                crate::modify::check_settings_edit(lock(&self.inner).doc(), edit)?;
            }
            let c = crate::edit::to_command(&cmd)?;
            self.execute_with(c, &filter)
        })
    }

    /// Runs `cmds` in order as one undo step named `label`: all of them or
    /// none (a refused command restores the document from before the
    /// first, and its error is returned; a panic inside a command does too
    /// and returns `Internal`). For a list Delete of mixed rows. None of
    /// these commands removes a running plugin: mixer edits, which capture
    /// a plugin's state before it goes, are not editor commands.
    pub fn edit_all(
        &self,
        label: String,
        cmds: Vec<crate::edit::EditCommand>,
        filter: Option<crate::edit::ViewFilterRow>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            if lock(&self.inner).edit_label.is_some() {
                return Err(crate::events::bad("finish the open edit first"));
            }
            let filter = crate::edit::view_filter(filter.as_ref());
            let mut core = Vec::with_capacity(cmds.len());
            for cmd in &cmds {
                match cmd {
                    crate::edit::EditCommand::ModifyAgain => {
                        return Err(crate::events::bad(
                            "Modify Again repeats a modification; it is not a command",
                        ));
                    }
                    crate::edit::EditCommand::Settings { edit } => {
                        crate::modify::check_settings_edit(lock(&self.inner).doc(), edit)?;
                    }
                    _ => {}
                }
                core.push(crate::edit::to_command(cmd)?);
            }
            {
                let mut inner = lock(&self.inner);
                inner.history.begin(label.clone())?;
                inner.edit_label = Some(label.clone());
            }
            // A panic inside a command must not leave the transaction open:
            // `ffi_guard` alone would report it but keep `edit_label` set.
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for (step, c) in core.into_iter().enumerate() {
                    edit_all_hook(&label, step);
                    self.execute_with(c, &filter)?;
                }
                Ok::<(), VizError>(())
            }));
            let failure = match run {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e),
                Err(payload) => Some(VizError::Internal {
                    message: crate::guard::panic_message(payload.as_ref()),
                }),
            };
            if let Some(e) = failure {
                let mut inner = lock(&self.inner);
                inner.edit_label = None;
                if inner.history.in_transaction() {
                    inner.history.abort()?;
                }
                inner.version += 1;
                inner.reload_host(self.id);
                self.commit_held(&mut inner);
                self.apply_jobs_after_edit(&mut inner);
                return Err(e);
            }
            let mut inner = lock(&self.inner);
            inner.edit_label = None;
            inner.history.commit()?;
            inner.version += 1;
            self.commit_held(&mut inner);
            self.apply_jobs_after_edit(&mut inner);
            inner.snapshot()
        })
    }

    /// Inside the edit `begin_edit` opened (a drag): replaces everything the
    /// edit did so far with `cmd` (Plan Ruling 7), so each pointer move
    /// sends the full target. A refused `cmd` leaves the document as it was
    /// when the edit began, with the edit still open. `Model` error when no
    /// edit is open.
    pub fn edit_gesture(
        &self,
        cmd: crate::edit::EditCommand,
        filter: Option<crate::edit::ViewFilterRow>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let c = crate::edit::to_command(&cmd)?;
            {
                let mut inner = lock(&self.inner);
                let label = match (&inner.edit_label, inner.history.in_transaction()) {
                    (Some(l), true) => l.clone(),
                    _ => return Err(crate::events::bad("no edit is open")),
                };
                inner.history.abort()?;
                inner.history.begin(label)?;
                inner.version += 1;
            }
            match self.execute_with(c, &crate::edit::view_filter(filter.as_ref())) {
                Ok(s) => Ok(s),
                Err(e) => {
                    // The rewind changed the document: the engine follows.
                    lock(&self.inner).reload_host(self.id);
                    Err(e)
                }
            }
        })
    }

    /// Closes the open edit and restores the document from before it
    /// (Escape during a drag). Nothing happens when no edit is open.
    pub fn abort_edit(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if inner.history.in_transaction() {
                inner.history.abort()?;
                inner.version += 1;
                inner.reload_host(self.id);
            }
            inner.edit_label = None;
            self.commit_held(&mut inner);
            self.apply_jobs_after_edit(&mut inner);
            inner.snapshot()
        })
    }
}

/// Step entry (plan 4f Rulings 11–15).
#[uniffi::export]
impl Session {
    /// One step entry `input` on lane `lane` of `seq`, from `state` (the
    /// app's copy). A step that enters notes or a reference, or ties, is
    /// one undo step "Step Entry"; taking one back is "Delete Last Step".
    /// Entered events become the selection. A refused edit leaves the
    /// document and `state` as they were and is the error. `Model` error
    /// while an edit is open, or for an unknown sequence or lane.
    pub fn step_entry(
        &self,
        seq: u64,
        lane: u64,
        state: crate::step::StepStateRow,
        input: crate::step::StepInputRow,
    ) -> Result<crate::step::StepResultRow, VizError> {
        use core_editor::step::{Effect, commit, press};
        use core_model::command::EventCmd;
        use core_model::event::{EventBody, EventSpec};
        ffi_guard(|| {
            let (seq_id, lane_id, next, effect, live) = {
                let inner = lock(&self.inner);
                if inner.edit_label.is_some() {
                    return Err(crate::events::bad("finish the open edit first"));
                }
                let doc = inner.doc();
                let seq_id = seq_id(doc, seq)?;
                let lane_id = LaneId(lane);
                doc.lane(seq_id, lane_id)?;
                let input = crate::step::input(&input, |r| {
                    core_model::query::seq_length(doc, r).unwrap_or(Tick::ZERO)
                });
                let st = crate::step::state(&state);
                // Where each event of a past step began.
                let began: std::collections::HashMap<core_model::ids::EventId, Tick> = st
                    .history
                    .iter()
                    .flat_map(|h| {
                        match &h.kind {
                            core_editor::step::StepKind::Notes(ids) => ids.clone(),
                            core_editor::step::StepKind::Reference(e) => vec![*e],
                            _ => Vec::new(),
                        }
                        .into_iter()
                        .map(move |e| (e, h.tick))
                    })
                    .collect();
                let (next, effect) = press(&st, input);
                // A tie or a step taken back acts only on its events still
                // in this lane and still where the step put them (the state
                // is the app's copy; it may be old, and undo rewinds the
                // document's ID counter, so an ID can be reused).
                let live = |ids: &[core_model::ids::EventId]| -> Vec<core_model::ids::EventId> {
                    ids.iter()
                        .copied()
                        .filter(|e| {
                            doc.locate(*e).is_ok_and(|l| {
                                l.seq == seq_id
                                    && l.lane == lane_id
                                    && began.get(e) == Some(&l.tick)
                            })
                        })
                        .collect()
                };
                let live = match &effect {
                    Effect::Extend { notes, .. } => live(notes),
                    Effect::TakeBack {
                        delete, shorten, ..
                    } => [live(delete), live(shorten)].concat(),
                    _ => Vec::new(),
                };
                (seq_id, lane_id, next, effect, live)
            };
            let cmds: Vec<Command> = match &effect {
                Effect::Nothing | Effect::Rest { .. } => Vec::new(),
                Effect::Insert {
                    tick,
                    length,
                    notes,
                } => vec![Command::Event(EventCmd::Insert {
                    seq: seq_id,
                    lane: lane_id,
                    events: notes
                        .iter()
                        .map(|(p, v)| EventSpec::new(*tick, EventBody::note(*p, *v, *length)))
                        .collect(),
                })],
                Effect::InsertReference { tick, seq: r, .. } => {
                    vec![Command::Event(EventCmd::Insert {
                        seq: seq_id,
                        lane: lane_id,
                        events: vec![EventSpec::new(*tick, EventBody::reference(*r))],
                    })]
                }
                Effect::Extend { by, .. } if !live.is_empty() => {
                    vec![Command::Event(EventCmd::Resize {
                        events: live.clone(),
                        delta: *by,
                    })]
                }
                Effect::Extend { .. } => Vec::new(),
                Effect::TakeBack {
                    delete,
                    shorten,
                    by,
                    ..
                } => {
                    let mut c = Vec::new();
                    let gone: Vec<_> = delete
                        .iter()
                        .filter(|e| live.contains(e))
                        .copied()
                        .collect();
                    if !gone.is_empty() {
                        c.push(Command::Event(EventCmd::Delete { events: gone }));
                    }
                    let short: Vec<_> = shorten
                        .iter()
                        .filter(|e| live.contains(e))
                        .copied()
                        .collect();
                    if !short.is_empty() {
                        c.push(Command::Event(EventCmd::Resize {
                            events: short,
                            delta: -*by,
                        }));
                    }
                    c
                }
            };
            let moved = effect != Effect::Nothing;
            // A tie records only the notes it lengthened; with none left
            // it is a rest.
            let effect = match effect {
                Effect::Extend { by, .. } if live.is_empty() => Effect::Rest { length: by },
                Effect::Extend { by, .. } => Effect::Extend {
                    notes: live.clone(),
                    by,
                },
                e => e,
            };
            if cmds.is_empty() {
                return Ok(crate::step::StepResultRow {
                    state: crate::step::state_row(&commit(&next, &effect, &[])),
                    snapshot: None,
                    moved,
                });
            }
            let label = match effect {
                Effect::TakeBack { .. } => "Delete Last Step",
                _ => "Step Entry",
            };
            let mut inner = lock(&self.inner);
            inner.history.begin(label.to_string())?;
            // A panic inside a command must not leave the transaction open
            // (as in `edit_all`).
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut created = Vec::new();
                for (i, c) in cmds.iter().enumerate() {
                    edit_all_hook(label, i);
                    let r = inner.history.execute(c, &ViewFilter::default())?;
                    created.extend(r.created.iter().filter_map(|a| match a {
                        core_model::ids::AnyId::Event(e) => Some(*e),
                        _ => None,
                    }));
                }
                Ok::<_, VizError>(created)
            }));
            let created = match run {
                Ok(Ok(created)) => created,
                Ok(Err(e)) => {
                    if inner.history.in_transaction() {
                        inner.history.abort()?;
                    }
                    return Err(e);
                }
                Err(payload) => {
                    if inner.history.in_transaction() {
                        inner.history.abort()?;
                    }
                    return Err(VizError::Internal {
                        message: crate::guard::panic_message(payload.as_ref()),
                    });
                }
            };
            inner.history.commit()?;
            if !created.is_empty() {
                inner
                    .history
                    .set_selection(core_model::selection::Selection {
                        time_range: None,
                        lanes: std::iter::once((seq_id, lane_id)).collect(),
                        events: created.iter().copied().collect(),
                    });
            }
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            Ok(crate::step::StepResultRow {
                state: crate::step::state_row(&commit(&next, &effect, &created)),
                snapshot: Some(inner.snapshot()?),
                moved,
            })
        })
    }
}

/// The selection (spec §3.3), sequence templates and the editor layout.
#[uniffi::export]
impl Session {
    /// The document-wide selection.
    pub fn selection(&self) -> Result<crate::selection::SelectionRow, VizError> {
        ffi_guard(|| Ok(crate::selection::row(lock(&self.inner).history.selection())))
    }

    /// Replaces the selection (not an undo step); unknown lanes and events
    /// are dropped.
    pub fn set_selection(&self, selection: crate::selection::SelectionRow) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let sel = crate::selection::selection(&selection).retain_existing(inner.doc());
            inner.history.set_selection(sel);
            Ok(())
        })
    }

    /// A new sequence from `template` named `name`, as one undo step.
    pub fn new_sequence_from_template(
        &self,
        name: String,
        template: crate::templates::SequenceTemplate,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if inner.history.in_transaction() {
                return Err(crate::events::bad("finish the open edit first"));
            }
            inner.history.begin("New Sequence from Template")?;
            match crate::templates::build(&mut inner.history, &name, &template) {
                Ok(_) => inner.history.commit()?,
                Err(e) => {
                    inner.history.abort()?;
                    return Err(e);
                }
            }
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            inner.snapshot()
        })
    }

    /// The editor layout saved with the project (JSON), if any.
    pub fn ui_layout(&self) -> Result<Option<String>, VizError> {
        ffi_guard(|| {
            Ok(lock(&self.inner)
                .editor_layout
                .as_ref()
                .map(|v| v.to_string()))
        })
    }

    /// Replaces the editor layout (JSON; written by the next save, never an
    /// undo step).
    pub fn set_ui_layout(&self, json: String) -> Result<(), VizError> {
        ffi_guard(|| {
            let v: serde_json::Value = serde_json::from_str(&json)
                .map_err(|e| crate::events::bad(&format!("the layout is not JSON: {e}")))?;
            lock(&self.inner).editor_layout = Some(v);
            Ok(())
        })
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

/// Removes the files of a job that was never applied (`root` is the
/// session's media root).
pub(crate) fn discard(root: &Path, w: crate::jobs::Work) {
    use crate::jobs::Work;
    let files: Vec<String> = match w {
        Work::Import { files, .. } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .flatten()
            .map(|i| i.path)
            .collect(),
        Work::Convert { files, .. } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .map(|i| i.path)
            .collect(),
        Work::Cancelled => Vec::new(),
    };
    for f in files {
        let _ = std::fs::remove_file(root.join(f));
    }
}

impl Session {
    /// Applies every finished job (one undo step each); true when the
    /// document changed.
    ///
    /// Not while an edit (a drag, a fader move) is open (final review
    /// I1): the job would join that edit's undo step, be refused ("a
    /// transaction is already open"), or be rolled back by the next
    /// pointer move. Finished jobs then stay pending (reported Running,
    /// `busy()` stays true) and apply when the edit closes:
    /// `end_edit`/`abort_edit` call [`Session::apply_jobs_after_edit`],
    /// and `idle`/`job` apply them too.
    fn apply_jobs(&self, inner: &mut Inner) -> bool {
        use crate::jobs::{JobStateRow, Work, row};
        if inner.history.in_transaction() {
            return false;
        }
        let mut changed = false;
        for (id, kind, work) in inner.jobs.finished() {
            let root = inner.root(self.id);
            let row = match work {
                Work::Cancelled => row(id, kind, JobStateRow::Cancelled, None, vec![], vec![]),
                Work::Import { files, target } => match apply_import(inner, &root, files, target) {
                    Ok((created, problems)) => {
                        changed |= !created.is_empty();
                        row(id, kind, JobStateRow::Done, None, problems, created)
                    }
                    Err((e, problems)) => row(
                        id,
                        kind,
                        JobStateRow::Failed,
                        Some(e.to_string()),
                        problems,
                        vec![],
                    ),
                },
                Work::Convert { format, files, .. } => {
                    let was_format = format.is_some();
                    match apply_convert(inner, &root, format, files) {
                        Ok((converted, problems)) => {
                            changed |= !converted.is_empty() || was_format;
                            row(id, kind, JobStateRow::Done, None, problems, converted)
                        }
                        Err((e, problems)) => row(
                            id,
                            kind,
                            JobStateRow::Failed,
                            Some(e.to_string()),
                            problems,
                            vec![],
                        ),
                    }
                }
            };
            inner.jobs.finish(id, row);
        }
        if changed {
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(inner);
        }
        changed
    }

    /// Applies the jobs that finished while an edit was open, now that it
    /// closed; the next `idle` reports the media change.
    fn apply_jobs_after_edit(&self, inner: &mut Inner) {
        if self.apply_jobs(inner) {
            inner.news.media_changed = true;
            inner.news.doc_changed = true;
        }
    }
}

/// One import job's result per source file: `(file name, the items it
/// became or why it failed)`.
type ImportFiles = Vec<(String, Result<Vec<core_media::import::Imported>, String>)>;

/// What an applied import made (media IDs, per-file problems), or why
/// nothing was applied (with the per-file problems).
type ImportApplied = Result<(Vec<u64>, Vec<String>), (VizError, Vec<String>)>;

/// One finished import (Ruling 19): the media items and, with a target,
/// their regions (into the given lane for a single mono/stereo file, else
/// one new "Audio n" lane per item), as one undo step "Import Audio".
/// On an error nothing is applied and the written files are removed.
fn apply_import(
    inner: &mut Inner,
    root: &Path,
    files: ImportFiles,
    target: Option<crate::jobs::ImportTargetRow>,
) -> ImportApplied {
    use core_model::command::{AudioCmd, MediaCmd};
    use core_model::document::MediaOrigin;
    use core_model::event::Fade;
    let mut problems = Vec::new();
    let mut items = Vec::new();
    for (name, r) in files {
        match r {
            Ok(v) => items.push(v),
            Err(e) => problems.push(format!("{name}: {e}")),
        }
    }
    let written: Vec<std::path::PathBuf> =
        items.iter().flatten().map(|i| root.join(&i.path)).collect();
    if items.is_empty() {
        return Ok((Vec::new(), problems));
    }
    let single = items.len() == 1 && items.first().is_some_and(|v| v.len() == 1);
    let filter = ViewFilter::default();
    let h = &mut inner.history;
    let fail = |e: VizError, problems: Vec<String>| {
        for p in &written {
            let _ = std::fs::remove_file(p);
        }
        (e, problems)
    };
    if let Err(e) = h.begin("Import Audio") {
        return Err(fail(e.into(), problems));
    }
    let run = (|| -> Result<Vec<u64>, VizError> {
        let mut created = Vec::new();
        for item in items.iter().flatten() {
            let r = h.execute(
                &Command::Media(MediaCmd::Add {
                    path: item.path.clone(),
                    sample_rate: item.sample_rate,
                    channels: item.channels,
                    length_frames: item.length_frames,
                    content_hash: Some(item.content_hash.clone()),
                    name: Some(item.name.clone()),
                    origin: Some(MediaOrigin::Imported {
                        original_name: item.original_name.clone(),
                    }),
                }),
                &filter,
            )?;
            let media = r
                .created
                .iter()
                .find_map(|id| match id {
                    core_model::ids::AnyId::Media(m) => Some(*m),
                    _ => None,
                })
                .ok_or_else(|| VizError::Internal {
                    message: "no media created".into(),
                })?;
            created.push(media.0);
            let Some(t) = target else { continue };
            let seq = SeqId(t.seq);
            let lane = match t.lane {
                Some(l) if single => LaneId(l),
                _ => {
                    let n = h.doc().sequence(seq)?.lanes.len() + 1;
                    let r = h.execute(
                        &Command::Lane(LaneCmd::Add {
                            seq,
                            name: format!("Audio {n}"),
                            index: None,
                        }),
                        &filter,
                    )?;
                    r.created
                        .iter()
                        .find_map(|id| match id {
                            core_model::ids::AnyId::Lane(l) => Some(*l),
                            _ => None,
                        })
                        .ok_or_else(|| VizError::Internal {
                            message: "no lane created".into(),
                        })?
                }
            };
            h.execute(
                &Command::Audio(AudioCmd::Insert {
                    seq,
                    lane,
                    tick: Tick(t.tick.max(0)),
                    media,
                    source_offset: 0,
                    length: item.length_frames,
                    gain_db: 0.0,
                    fade_in: Fade::default(),
                    fade_out: Fade::default(),
                }),
                &filter,
            )?;
        }
        Ok(created)
    })();
    match run {
        Ok(created) => match h.commit() {
            Ok(()) => Ok((created, problems)),
            Err(e) => Err(fail(e.into(), problems)),
        },
        Err(e) => {
            let _ = h.abort();
            Err(fail(e, problems))
        }
    }
}

/// `(media, absolute file, stem)` per item to convert.
type ConvertList = Vec<(core_model::ids::MediaId, PathBuf, String)>;

/// One conversion job's result per media item: the new file, or why it
/// failed.
type ConvertFiles = Vec<(
    core_model::ids::MediaId,
    Result<core_media::import::Imported, String>,
)>;

/// What an applied conversion converted (media IDs, per-item problems), or
/// why nothing was applied (with the per-item problems).
type ConvertApplied = ImportApplied;

/// `(media, absolute file, stem)` of the items `pick` chooses.
fn convert_list(
    inner: &Inner,
    id: u64,
    pick: impl Fn(core_model::ids::MediaId) -> bool,
) -> ConvertList {
    let root = inner.root(id);
    inner
        .doc()
        .media()
        .values()
        .filter(|m| pick(m.id))
        .map(|m| {
            let (_, stem) = core_model::document::MediaItem::file_parts(&m.path);
            (m.id, root.join(&m.path), stem)
        })
        .collect()
}

/// The conversion job's body: each file through the platform importer
/// into the project's `Media/` in `format`. Every result is a new file
/// under the first free name after the item's stem; the old files are
/// only read (media files are write-once: Save As hard-links them between
/// packages), and stay for undo.
fn convert_files(
    root: &Path,
    items: ConvertList,
    format: core_model::format::AudioFormat,
    progress: &std::sync::atomic::AtomicU32,
    cancel: &std::sync::atomic::AtomicBool,
    kind: crate::jobs::JobKindRow,
) -> crate::jobs::Work {
    let importer = crate::media::importer();
    let n = items.len().max(1) as f32;
    let mut files: ConvertFiles = Vec::new();
    for (k, (media, path, stem)) in items.into_iter().enumerate() {
        let r = core_media::import::import_as(
            importer.as_ref(),
            &path,
            root,
            format,
            &stem,
            &mut |f| {
                progress.store(
                    ((k as f32 + f) / n).to_bits(),
                    std::sync::atomic::Ordering::Relaxed,
                )
            },
            cancel,
        );
        match r {
            Err(core_media::MediaError::Cancelled) => {
                discard(
                    root,
                    crate::jobs::Work::Convert {
                        kind,
                        format: None,
                        files,
                    },
                );
                return crate::jobs::Work::Cancelled;
            }
            Ok(mut v) if v.len() == 1 => files.push((media, Ok(v.remove(0)))),
            Ok(v) => {
                discard(
                    root,
                    crate::jobs::Work::Import {
                        files: vec![(stem.clone(), Ok(v))],
                        target: None,
                    },
                );
                files.push((media, Err(format!("{stem}: has more than two channels"))));
            }
            Err(e) => files.push((media, Err(format!("{stem}: {e}")))),
        }
    }
    crate::jobs::Work::Convert {
        kind,
        format: (kind == crate::jobs::JobKindRow::Format).then_some(format),
        files,
    }
}

/// Applies a finished conversion. A format change applies only when every
/// item converted (else nothing, the converted files are removed, and the
/// failures are the message); a media conversion applies what converted
/// and lists the rest as problems. Both swap the items to the new files
/// and rescale their regions in one model step (`swap_media`); the old
/// files are left as they are.
fn apply_convert(
    inner: &mut Inner,
    root: &Path,
    format: Option<core_model::format::AudioFormat>,
    files: ConvertFiles,
) -> ConvertApplied {
    use core_model::command::{MediaCmd, MediaSwap, SettingsCmd};
    let mut swaps = Vec::new();
    let mut problems = Vec::new();
    for (m, r) in &files {
        match r {
            Ok(i) => swaps.push(MediaSwap {
                media: *m,
                path: i.path.clone(),
                length_frames: i.length_frames,
                content_hash: Some(i.content_hash.clone()),
            }),
            Err(e) => problems.push(e.clone()),
        }
    }
    // Only this job's new files: never referenced by any document yet.
    let remove = |files: &ConvertFiles| {
        for (_, r) in files {
            if let Ok(i) = r {
                let _ = std::fs::remove_file(root.join(&i.path));
            }
        }
    };
    let cmd = match format {
        Some(f) => {
            if !problems.is_empty() {
                remove(&files);
                return Err((
                    VizError::Model {
                        message: problems.join("; "),
                    },
                    problems,
                ));
            }
            Command::Settings(SettingsCmd::SetFormat { format: f, swaps })
        }
        None => {
            if swaps.is_empty() {
                return Ok((Vec::new(), problems));
            }
            Command::Media(MediaCmd::Convert { swaps })
        }
    };
    match inner.history.execute(&cmd, &ViewFilter::default()) {
        Ok(_) => Ok((
            files
                .iter()
                .filter(|(_, r)| r.is_ok())
                .map(|(m, _)| m.0)
                .collect(),
            problems,
        )),
        Err(e) => {
            remove(&files);
            Err((e.into(), problems))
        }
    }
}

/// Waveform overviews (plan 5b Task 4).
#[uniffi::export]
impl Session {
    /// `columns` columns of media item `media` over frames `[from_frame,
    /// to_frame)` (cut to the file). Never waits for a file: what is not
    /// loaded answers `Pending` (spec §10.1; plan 5b Ruling 3).
    pub fn overview(
        &self,
        media: u64,
        from_frame: u64,
        to_frame: u64,
        columns: u32,
    ) -> Result<crate::overviews::OverviewRows, VizError> {
        ffi_guard(|| {
            use crate::overviews::{OverviewRows, OverviewStateRow};
            crate::overviews::check_columns(columns)?;
            if from_frame >= to_frame {
                return Err(crate::events::bad("from_frame must be before to_frame"));
            }
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            let id = core_model::ids::MediaId(media);
            let m = inner
                .doc()
                .media()
                .get(&id)
                .cloned()
                .ok_or_else(|| crate::events::bad(&format!("unknown media {media}")))?;
            if inner.offline.contains_key(&id) {
                return Ok(OverviewRows::silent(
                    OverviewStateRow::Offline,
                    m.channels,
                    columns,
                ));
            }
            let to = to_frame.min(m.length_frames.max(from_frame + 1));
            let n = columns as usize;
            let w = (to - from_frame) as f64 / n as f64;
            let spans: Vec<Option<(u64, u64)>> = (0..n)
                .map(|c| {
                    let a = from_frame + (c as f64 * w).floor() as u64;
                    let b = (from_frame + ((c + 1) as f64 * w).ceil() as u64).min(to);
                    (b > a).then_some((a, b))
                })
                .collect();
            let file = inner.root(self.id).join(&m.path);
            Ok(crate::overviews::columns(
                &inner.overviews,
                id,
                &file,
                m.channels,
                &spans,
                &|_| 1.0,
            ))
        })
    }

    /// `columns` columns of audio region `event` over ticks `[from_tick,
    /// to_tick)` of its sequence, mapped through the tempo map, scaled by
    /// the region's gain and shaped by its fades; columns outside the
    /// region are silent (plan 5b Ruling 4).
    pub fn region_waveform(
        &self,
        event: u64,
        from_tick: i64,
        to_tick: i64,
        columns: u32,
    ) -> Result<crate::overviews::OverviewRows, VizError> {
        ffi_guard(|| {
            use crate::overviews::{OverviewRows, OverviewStateRow};
            use core_model::event::EventBody;
            crate::overviews::check_columns(columns)?;
            if from_tick >= to_tick {
                return Err(crate::events::bad("from_tick must be before to_tick"));
            }
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            let doc = inner.doc().clone();
            let (ev, loc) = doc.event(core_model::ids::EventId(event))?;
            let EventBody::AudioRegion {
                media,
                source_offset,
                length,
                gain_db,
                fade_in,
                fade_out,
            } = ev.body
            else {
                return Err(crate::events::bad(&format!(
                    "{event} is not an audio region"
                )));
            };
            let m = doc
                .media()
                .get(&media)
                .ok_or_else(|| crate::events::bad(&format!("unknown media {media}")))?;
            if inner.offline.contains_key(&media) {
                return Ok(OverviewRows::silent(
                    OverviewStateRow::Offline,
                    m.channels,
                    columns,
                ));
            }
            let tempo = &doc.sequence(loc.seq)?.tempo;
            let rate = f64::from(m.sample_rate.max(1));
            let start = tempo.tick_to_seconds(ev.tick);
            // Region frame at tick `t` (f64 ticks; `as` saturates).
            let frame =
                |t: f64| (tempo.tick_to_seconds(core_model::time::Tick(t as i64)) - start) * rate;
            let n = columns as usize;
            let (t0, t1) = (from_tick as f64, to_tick as f64);
            let w = (t1 - t0) / n as f64;
            let len = length as f64;
            let spans: Vec<Option<(u64, u64)>> = (0..n)
                .map(|c| {
                    let a = frame(t0 + c as f64 * w).max(0.0);
                    let b = frame(t0 + (c + 1) as f64 * w).min(len);
                    if a.is_nan() || b.is_nan() || b <= a {
                        return None;
                    }
                    let a = a.floor() as u64;
                    let b = (b.ceil() as u64).clamp(a + 1, length.max(a + 1));
                    Some((
                        source_offset.saturating_add(a),
                        source_offset.saturating_add(b),
                    ))
                })
                .collect();
            let lin = 10f32.powf(gain_db / 20.0);
            let gain = |c: usize| -> f32 {
                let Some(Some((a, b))) = spans.get(c) else {
                    return 0.0;
                };
                let r = (a + b) / 2 - source_offset.min((a + b) / 2);
                lin * crate::overviews::fade_gain(r, length, fade_in, fade_out)
            };
            let file = inner.root(self.id).join(&m.path);
            Ok(crate::overviews::columns(
                &inner.overviews,
                media,
                &file,
                m.channels,
                &spans,
                &gain,
            ))
        })
    }
}

/// Media and jobs (plan 5a Task 10).
#[uniffi::export]
impl Session {
    /// The media pool, with offline items and use counts.
    pub fn media(&self) -> Result<Vec<crate::media::MediaRow>, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            Ok(crate::media::rows(inner.doc(), &inner.offline))
        })
    }

    /// Checks every media file again (the app became active, or the user
    /// asked); true when the offline set changed.
    pub fn recheck_media(&self) -> Result<bool, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let changed = inner.refresh_media(self.id, true);
            if changed {
                inner.reload_host(self.id);
            }
            Ok(changed)
        })
    }

    /// Starts importing `paths` (converted into the project format; spec
    /// §5.1) as a job; `target` places regions (Ruling 19). Returns the
    /// job ID; `job` reports it and applies it when done.
    pub fn import_audio(
        &self,
        paths: Vec<String>,
        target: Option<crate::jobs::ImportTargetRow>,
    ) -> Result<u64, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if let Some(t) = target {
                let doc = inner.doc();
                doc.sequence(SeqId(t.seq))?;
                if let Some(l) = t.lane {
                    doc.lane(SeqId(t.seq), LaneId(l))?;
                }
            }
            let root = inner.root(self.id);
            let format = inner.doc().settings().format;
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Import, move |progress, cancel| {
                    let importer = crate::media::importer();
                    let n = paths.len().max(1) as f32;
                    let mut files: ImportFiles = Vec::new();
                    for (k, p) in paths.iter().enumerate() {
                        let path = std::path::PathBuf::from(p);
                        let name = path
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| p.clone());
                        let r = core_media::import::import_file(
                            importer.as_ref(),
                            &path,
                            &root,
                            format,
                            &mut |f| {
                                progress.store(
                                    ((k as f32 + f) / n).to_bits(),
                                    std::sync::atomic::Ordering::Relaxed,
                                )
                            },
                            cancel,
                        );
                        match r {
                            Err(core_media::MediaError::Cancelled) => {
                                // Remove what earlier files of this batch wrote.
                                for (_, done) in &files {
                                    if let Ok(items) = done {
                                        for i in items {
                                            let _ = std::fs::remove_file(root.join(&i.path));
                                        }
                                    }
                                }
                                return crate::jobs::Work::Cancelled;
                            }
                            r => files.push((name, r.map_err(|e| e.to_string()))),
                        }
                    }
                    crate::jobs::Work::Import { files, target }
                })
        })
    }

    /// The project's format, its mismatched media and the device's rate
    /// (known once this session has played: the host asks the device at
    /// Play, Ruling 20 / F19).
    pub fn project_info(&self) -> Result<crate::format::ProjectInfoRow, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            let mismatched = inner
                .offline
                .values()
                .filter(|r| {
                    matches!(
                        r,
                        crate::media::OfflineReasonRow::Format
                            | crate::media::OfflineReasonRow::Unreadable
                    )
                })
                .count();
            let (rate_mismatch, device_rate) = host::device(self.id).unwrap_or((false, 0.0));
            Ok(crate::format::ProjectInfoRow {
                format: inner.doc().settings().format.into(),
                mismatched: u32::try_from(mismatched).unwrap_or(u32::MAX),
                rate_mismatch,
                device_rate,
            })
        })
    }

    /// Starts changing the project format: every media file is converted
    /// into the new format, as new files (a job; spec §4.1, §5.1). Refused
    /// at once for the same format or an invalid rate; the job fails,
    /// changing nothing, when a file is missing, of another length or
    /// cannot be converted. Applied as one undo step "Change Project
    /// Format"; the engine host then reopens at the new rate.
    pub fn set_project_format(
        &self,
        format: crate::format::ProjectFormatRow,
    ) -> Result<u64, VizError> {
        ffi_guard(|| {
            let f = core_model::format::AudioFormat::from(format);
            let mut inner = lock(&self.inner);
            if !f.is_valid() {
                return Err(VizError::Model {
                    message: format!("{} Hz is not a project rate", f.sample_rate),
                });
            }
            if f == inner.doc().settings().format {
                return Err(VizError::Model {
                    message: "the project is already in that format".into(),
                });
            }
            inner.refresh_media(self.id, false);
            let blocked: Vec<String> = inner
                .doc()
                .media()
                .values()
                .filter(|m| {
                    matches!(
                        inner.offline.get(&m.id),
                        Some(
                            crate::media::OfflineReasonRow::Missing
                                | crate::media::OfflineReasonRow::Length
                                | crate::media::OfflineReasonRow::Changed
                        )
                    )
                })
                .map(|m| m.name.clone())
                .collect();
            let items = convert_list(&inner, self.id, |_| true);
            let root = inner.root(self.id);
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Format, move |progress, cancel| {
                    if !blocked.is_empty() {
                        return crate::jobs::Work::Convert {
                            kind: crate::jobs::JobKindRow::Format,
                            format: Some(f),
                            files: vec![(
                                core_model::ids::MediaId(u64::MAX),
                                Err(format!(
                                    "missing or changed media must be relinked first: {}",
                                    blocked.join(", ")
                                )),
                            )],
                        };
                    }
                    convert_files(
                        &root,
                        items,
                        f,
                        progress,
                        cancel,
                        crate::jobs::JobKindRow::Format,
                    )
                })
        })
    }

    /// Starts converting the media that is not in the project format
    /// (another rate or depth, or not a VizDAW CAF) into it, as new files
    /// (the banner's "Convert"; a job, one undo step "Convert Media").
    pub fn convert_media(&self) -> Result<u64, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            let offline = inner.offline.clone();
            let items = convert_list(&inner, self.id, |m| {
                matches!(
                    offline.get(&m),
                    Some(
                        crate::media::OfflineReasonRow::Format
                            | crate::media::OfflineReasonRow::Unreadable
                    )
                )
            });
            let f = inner.doc().settings().format;
            let root = inner.root(self.id);
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Convert, move |progress, cancel| {
                    convert_files(
                        &root,
                        items,
                        f,
                        progress,
                        cancel,
                        crate::jobs::JobKindRow::Convert,
                    )
                })
        })
    }

    /// Job `id`, applying it first when it just finished.
    pub fn job(&self, id: u64) -> Result<crate::jobs::JobRow, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            self.apply_jobs(&mut inner);
            inner.jobs.row(id).ok_or_else(|| VizError::Model {
                message: format!("unknown job {id}"),
            })
        })
    }

    /// Asks job `id` to stop (nothing it did is applied).
    pub fn cancel_job(&self, id: u64) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).jobs.cancel(id);
            Ok(())
        })
    }
}
