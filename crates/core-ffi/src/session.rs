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
    /// The folder standing for the Trash (test hook; `None`: the
    /// environment's or the platform's).
    trash_dir: Option<PathBuf>,
    /// The recording in progress (plan 5c Task 9).
    recording: Option<crate::record::Recording>,
    /// A recording is being finished (its files are closing without the
    /// lock): no new one may start.
    finishing: bool,
    /// A finished recording's pass (or `None` when nothing landed), with
    /// its files, waiting for an open edit to end.
    pending_pass: Option<(Option<core_model::command::RecordPass>, Vec<String>)>,
    /// Recorded files no document references that the Trash must not take
    /// (a pass that could not be applied, a take whose lane went or whose
    /// file the writer could not close; pre-flight I3): crash recovery
    /// offers them at the next open.
    keep_recordings: std::collections::BTreeSet<String>,
    /// The files of the pass being finished without the session lock
    /// (pre-flight I1): a save meanwhile must leave them.
    finishing_files: Vec<String>,
    /// Transport fault names the last recording raised (`record-overflow`,
    /// `input-lost`; pre-flight M11), until the next one starts.
    record_faults: Vec<String>,
    /// Recordings a crash left (and kept takes), offered and not yet
    /// answered (plan 5c Task 10): the Trash leaves them until they are.
    recovered: Vec<crate::record::RecoveredFileRow>,
    /// Track Synth's "Mute the MIDI lane after recording" (plan 5c-2
    /// Ruling 6): by (sequence, return lane), the MIDI lane the next pass
    /// that places audio there mutes. Runtime only.
    synth_mutes: std::collections::BTreeMap<(SeqId, LaneId), LaneId>,
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
            trash_dir: None,
            recording: None,
            finishing: false,
            pending_pass: None,
            keep_recordings: Default::default(),
            finishing_files: Vec::new(),
            record_faults: Vec::new(),
            recovered: Vec::new(),
            synth_mutes: Default::default(),
        }
    }

    /// Recorded files the Trash must leave: those being written or
    /// finished, those of a pass waiting to apply, those kept after a
    /// failed pass, and those offered for recovery (plan 5c Ruling 15,
    /// Task 10; pre-flight I1, I3).
    fn recorded_keep(&self) -> Vec<String> {
        let mut out: Vec<String> = self.keep_recordings.iter().cloned().collect();
        out.extend(self.recovered.iter().map(|r| r.path.clone()));
        out.extend(self.finishing_files.iter().cloned());
        if let Some(r) = &self.recording {
            out.extend(r.files());
        }
        if let Some((_, files)) = &self.pending_pass {
            out.extend(files.iter().cloned());
        }
        out
    }

    /// The retention rule on `package` (plan 5b Ruling 11). Skipped while
    /// a job is not applied yet (its files are in no document), unless
    /// `jobs_done` (the jobs were shut down and their files removed).
    /// Failures become problems for the next `idle`; at `close` no `idle`
    /// follows, so `close` also writes them to stderr (preflight M5).
    fn tidy(&mut self, package: &Path, jobs_done: bool) {
        if !jobs_done && self.jobs.busy() {
            return;
        }
        let dir = crate::retention::trash_dir(self.trash_dir.as_deref());
        let mut refs = self.history.referenced_media_anywhere();
        for p in self.recorded_keep() {
            refs.entry(p).or_insert(None);
        }
        let out = crate::retention::run(&refs, package, &|p| {
            crate::retention::trash(dir.as_deref(), p)
        });
        self.news.problems.extend(out.problems);
    }

    /// Writes `package`'s keep file from the kept takes (plan 5c Task 10);
    /// a failure is a problem for the next `idle`.
    fn write_kept(&mut self, package: &Path) {
        if let Err(e) = crate::record::write_kept(package, &self.keep_recordings) {
            self.news
                .problems
                .push(format!("The list of kept recordings was not saved ({e})."));
        }
    }

    /// Every package-relative path a history state references: no job
    /// may give a new file one of these names, even when the file is
    /// missing (plan 5b Ruling 12; final review r2). Their users
    /// (`import_reserved`, `relink`) compare them without case, as macOS
    /// volumes resolve names (preflight I2).
    fn reserved_names(&self) -> std::collections::BTreeSet<String> {
        self.named_media().into_keys().collect()
    }

    /// [`Inner::reserved_names`] with the content hash each path names (a
    /// relink uses a file already in `Media/` in place only when no state
    /// names its path for other audio; preflight M7).
    fn named_media(&self) -> std::collections::BTreeMap<String, Option<String>> {
        self.history.referenced_media_anywhere()
    }

    /// [`Inner::reserved_names`] in lower case: the files a job's cleanup
    /// never removes (Task 6 review m1), compared as macOS volumes
    /// resolve names.
    fn kept(&self) -> std::collections::BTreeSet<String> {
        self.named_media()
            .into_keys()
            .map(|p| p.to_lowercase())
            .collect()
    }

    /// `Busy` while a Format, Convert or Relink job has not been applied:
    /// each captures the project format and the items' files when it
    /// starts, so another such job may not start meanwhile (5a Task 11
    /// review Minor 4; preflight M3). An import does not block them.
    fn media_jobs_idle(&self) -> Result<(), VizError> {
        use crate::jobs::JobKindRow;
        if self
            .jobs
            .pending(&[JobKindRow::Format, JobKindRow::Convert, JobKindRow::Relink])
        {
            return Err(VizError::Busy {
                message: "Wait for the media job to finish.".into(),
            });
        }
        Ok(())
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
        // One session per package (review I-2); released by the guard
        // until the session exists.
        let claim = path.as_deref().map(|p| claim_package(p, id)).transpose()?;
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
        // The session releases it from now on (close, drop, Save As).
        if let Some(c) = claim {
            c.keep();
        }
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
        let mut refs = inner.history.referenced_media_anywhere();
        // Recordings kept for the recovery offer go with the project
        // (pre-flight I3): Save As removes an untitled scratch package.
        for p in inner.recorded_keep() {
            refs.entry(p).or_insert(None);
        }
        crate::media::copy_media(&refs, &inner.root(self.id), path)?;
        crate::overviews::copy_cache(&refs, &inner.root(self.id), path);
        file::save(inner.doc(), path)?;
        write_windows(path, &inner.windows, inner.editor_layout.as_ref());
        inner.write_kept(path);
        // Reads `project.json` and the autosaves under the save lock
        // (preflight M4: whole-document reads, not media-file reads).
        inner.tidy(path, false);
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
        if inner.recording.is_some() || inner.finishing {
            return Err(recording_busy());
        }
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
            let s = Session::with(doc, Some(p.clone()), options.plugins_off)?;
            // Only once the package is this session's (final review M1,
            // M2): a refused open leaves another session's imports alone,
            // and nothing is read while a close is still writing it.
            crate::media::sweep_temp(&p);
            let windows = read_windows(&p);
            let layout = read_layout(&p);
            {
                let mut inner = lock(&s.inner);
                // Plan 5c Task 10: kept takes, and what a crash left.
                let (kept, unread) = crate::record::kept_at_open(&p, inner.doc());
                let recovered = crate::record::offered(&p, inner.doc(), &kept);
                inner.keep_recordings = kept;
                inner.recovered = recovered;
                inner.news.problems.extend(unread);
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
            // Recordings are written under the current root, and a pass
            // waiting for an edit to end names paths under it (pre-flight
            // I1).
            if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
                return Err(recording_busy());
            }
            // A job that just finished applies first, under the old root.
            self.apply_jobs(&mut inner);
            if inner.jobs.busy() {
                return Err(VizError::Busy {
                    message: "Wait for the import or conversion to finish, then save.".into(),
                });
            }
            // A closed window holds no package (re-review m-3).
            if inner.closed {
                return Err(crate::events::bad("this project was closed"));
            }
            let p = with_extension(&path);
            // Another window's package (review I-2): its retention would
            // not know this session's files. Claimed before anything is
            // written (re-review m-2); a failed save releases it.
            let claim = claim_package(&p, self.id)?;
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
                // The old package goes, unless it is the same one spelled
                // another way.
                if let Some(old) = inner.path.clone() {
                    if package_key(&old) != claim.key {
                        release_package(&old, self.id);
                    }
                }
                inner.path = Some(p);
                // The overview worker moves to the new package first, so it
                // writes nothing more into the scratch package (review m3).
                inner.refresh_media(self.id, true);
                // The scratch package of an untitled session is done with.
                let _ = std::fs::remove_dir_all(crate::media::scratch_root(self.id));
                inner.reload_host(self.id);
                self.mark(&mut inner);
            }
            claim.keep();
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
            // Stop while recording finishes the pass (spec §6.5), then
            // stops as any Stop does (pre-flight M4: a rebuild running
            // meanwhile must not resume playing).
            if lock(&self.inner).recording.is_some() {
                self.finish_recording(None)?;
            }
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
            if inner.recording.is_some() {
                return Err(recording_busy());
            }
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
            if inner.recording.is_some() {
                return Err(recording_busy());
            }
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
                        for f in &inner.record_faults {
                            add_fault(&mut h.faults, f);
                        }
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
                        faults: inner
                            .reload_fault
                            .iter()
                            .chain(&inner.record_faults)
                            .cloned()
                            .collect(),
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
            self.service_recording()?;
            let mut inner = lock(&self.inner);
            self.apply_pending_pass(&mut inner);
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
            // In the order found: the latest finding for an item wins
            // (review m1).
            for f in found.found {
                match f {
                    crate::overviews::Finding::Mismatch {
                        media: m,
                        path,
                        hash,
                    } => {
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
                    crate::overviews::Finding::Verified { media: m, path } => {
                        if inner.changed.get(&m).is_some_and(|(p, _)| *p == path) {
                            inner.changed.remove(&m);
                            hash_news = true;
                        }
                    }
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
                    external: d.kind.is_external(),
                    return_latency_ms: d.return_latency_ms,
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
            // What finishing a pass here says: no `idle` follows a close
            // (review minor c).
            let before = lock(&self.inner).news.problems.len();
            let mut inner = loop {
                // A pass another thread is finishing lands first
                // (pre-flight I1): its files are in no document yet.
                self.wait_finished()?;
                // A recording ends cleanly first: its files are finished,
                // its pass applied (what is not saved then goes to the
                // Trash).
                if lock(&self.inner).recording.is_some() {
                    self.finish_recording(None)?;
                }
                // Another thread may have started finishing meanwhile
                // (review minor a): checked again under the lock kept.
                let inner = lock(&self.inner);
                if !inner.finishing && inner.recording.is_none() {
                    break inner;
                }
            };
            for line in inner.news.problems.iter().skip(before) {
                eprintln!("VizDAW: {line}");
            }
            inner.closed = true;
            let root = inner.root(self.id);
            let keep = inner.kept();
            for w in inner.jobs.shutdown() {
                discard(&root, w, &keep);
            }
            inner.overviews.shutdown();
            if let Some(p) = inner.path.clone() {
                // The jobs were shut down and their files removed above,
                // and the overview worker has stopped. Under the save
                // lock, as after a save (an autosave may be writing).
                let _g = lock(&SAVE_LOCK);
                let before = inner.news.problems.len();
                // Takes kept since the last save stay kept (plan 5c Task
                // 10), saved or not.
                inner.write_kept(&p);
                before_close_tidy();
                inner.tidy(&p, true);
                // No `idle` follows a close: the problems would be lost
                // (preflight M5).
                for line in inner.news.problems.iter().skip(before) {
                    eprintln!("VizDAW: {line}");
                }
            }
            host::forget_media(self.id);
            if inner.path.is_none() {
                let scratch = crate::media::scratch_root(self.id);
                // Recordings are never unlinked: an untitled project's go
                // to the Trash before its scratch package is removed
                // (plan 5c Ruling 15).
                let dir = crate::retention::trash_dir(inner.trash_dir.as_deref());
                let mut left = 0usize;
                let recorded = scratch.join(crate::record::RECORDED_DIR);
                match std::fs::read_dir(&recorded) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    // Unreadable: recordings may be in it (review minor b).
                    Err(e) => {
                        eprintln!("VizDAW: {}: not read ({e})", recorded.display());
                        left += 1;
                    }
                    Ok(rd) => {
                        for e in rd.flatten() {
                            if e.path().is_file() {
                                if let Err(why) = crate::retention::trash(dir.as_deref(), &e.path())
                                {
                                    eprintln!(
                                        "VizDAW: {}: not moved to the Trash ({why})",
                                        e.path().display()
                                    );
                                    left += 1;
                                }
                            }
                        }
                    }
                }
                if left == 0 {
                    let _ = std::fs::remove_dir_all(scratch);
                } else {
                    // Never unlinked (pre-flight I2): the package stays.
                    eprintln!(
                        "VizDAW: {left} recording(s) were left in {}",
                        scratch.display()
                    );
                }
            }
            if let Some(k) = inner.key.clone() {
                recovery::closed(&k, self.id);
            }
            // Last (final review M1): an open of this package meanwhile
            // would read a keep file being rewritten and offer files the
            // tidy is moving to the Trash.
            release_packages(self.id);
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
    release_packages(s.id);
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
    /// The engine host's media table for this session: `(media, absolute
    /// path)` of every file reloads play (test hook, preflight M2).
    pub fn debug_engine_media(&self) -> Vec<(u64, String)> {
        host::debug_media(self.id)
    }

    /// The input routes the engine was last given for this session (test
    /// hook, plan 5c Task 8).
    pub fn debug_engine_inputs(&self) -> Option<core_engine::input::InputSpec> {
        host::debug_inputs(self.id)
    }

    /// Whether the engine host has its audio input open for this session
    /// (test hook, Task 11 review I1).
    pub fn debug_input_open(&self) -> Option<bool> {
        host::debug_input_open(self.id)
    }

    /// How many times the engine host opened or closed its input (test
    /// hook).
    pub fn debug_input_changes(&self) -> Option<u64> {
        host::debug_input_changes(self.id)
    }

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

    /// Forgets every loaded overview (test hook).
    pub fn debug_forget_overviews(&self) {
        lock(&self.inner).overviews.forget_all();
    }

    /// Holds (`true`) or releases the finishing of every recording pass
    /// (test hook, pre-flight I1; see [`crate::record::set_hold_finish`]).
    pub fn debug_hold_finish(&self, on: bool) {
        crate::record::set_hold_finish(on);
    }

    /// The folder this session's retention moves files into instead of
    /// the Trash (test hook; `None`: back to the default).
    pub fn debug_set_trash_dir(&self, dir: Option<String>) {
        lock(&self.inner).trash_dir = dir.map(PathBuf::from);
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

#[cfg(feature = "test-hooks")]
thread_local! {
    /// What runs on this thread right before `close` tidies its package
    /// (test hook, final review M1).
    static BEFORE_CLOSE_TIDY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Runs `f` the next time `close` on this thread is about to tidy its
/// package; `None` clears it (test hook).
#[cfg(feature = "test-hooks")]
pub fn set_before_close_tidy_hook(f: Option<Box<dyn FnOnce()>>) {
    BEFORE_CLOSE_TIDY.with(|h| *h.borrow_mut() = f);
}

#[cfg(feature = "test-hooks")]
fn before_close_tidy() {
    if let Some(f) = BEFORE_CLOSE_TIDY.with(|h| h.borrow_mut().take()) {
        f();
    }
}

#[cfg(not(feature = "test-hooks"))]
fn before_close_tidy() {}

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
        let (key, recording) = {
            let inner = lock(&self.inner);
            (inner.key.clone(), inner.recording.is_some())
        };
        // Dropped while recording without `close`: the tap goes with the
        // writer, whose files stay for the recovery offer.
        if recording {
            let _ = host::stop_recording(self.id);
        }
        release_packages(self.id);
        if let Some(k) = key {
            recovery::closed(&k, self.id);
        }
        // The engine goes first when this session's plugin host feeds it;
        // queued if a plugin call is running (re-entrancy, plugins.rs).
        plugins::close_session(self.id);
    }
}

/// Removes the file `rel` a job wrote under `root`, unless `keep` (lower
/// case, [`Inner::kept`]) holds it: a history state references it, so it
/// is no longer only the job's (Task 6 review m1).
fn remove_unkept(root: &Path, rel: &str, keep: &std::collections::BTreeSet<String>) {
    if !keep.contains(&rel.to_lowercase()) {
        let _ = std::fs::remove_file(root.join(rel));
    }
}

/// Removes the files of a job that was never applied (`root` is the
/// session's media root), except those `keep` holds ([`remove_unkept`];
/// job threads, which cannot see the history, pass an empty set).
pub(crate) fn discard(
    root: &Path,
    w: crate::jobs::Work,
    keep: &std::collections::BTreeSet<String>,
) {
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
        // Only files the job wrote; a file it found in `Media/` stays.
        Work::Relink { files } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .filter(|r| r.made)
            .map(|r| r.file.path)
            .collect(),
        Work::Cancelled => Vec::new(),
    };
    for f in files {
        remove_unkept(root, &f, keep);
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
                Work::Relink { files } => {
                    let (done, problems) = apply_relink(inner, &root, files);
                    changed |= !done.is_empty();
                    row(id, kind, JobStateRow::Done, None, problems, done)
                }
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

    /// Applies the jobs that finished (while an edit was open, now that it
    /// closed, or before a new job starts; Task 6 review m7); the next
    /// `idle` reports the media and document change.
    fn apply_jobs_after_edit(&self, inner: &mut Inner) {
        if self.apply_jobs(inner) {
            inner.news.media_changed = true;
            inner.news.doc_changed = true;
        }
        self.apply_pending_pass(inner);
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
    let written: Vec<String> = items.iter().flatten().map(|i| i.path.clone()).collect();
    let keep = inner.kept();
    if items.is_empty() {
        return Ok((Vec::new(), problems));
    }
    let single = items.len() == 1 && items.first().is_some_and(|v| v.len() == 1);
    let filter = ViewFilter::default();
    let h = &mut inner.history;
    let fail = |e: VizError, problems: Vec<String>| {
        for p in &written {
            remove_unkept(root, p, &keep);
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

/// Applies a finished relink job: one undo step "Relink Media" per item
/// (Ruling 9). A refused relink (the file is too short) removes the file
/// the job made and becomes a problem. Returns the relinked items and
/// the problems.
fn apply_relink(
    inner: &mut Inner,
    root: &Path,
    files: Vec<(
        core_model::ids::MediaId,
        Result<crate::jobs::RelinkFile, String>,
    )>,
) -> (Vec<u64>, Vec<String>) {
    use core_model::command::MediaCmd;
    let mut done = Vec::new();
    let mut problems = Vec::new();
    // Files this job wrote for relinks that were refused: removed only
    // after the whole batch, and only if no state names them, since a
    // search gives every item of one hash the same file (5b final review
    // m2).
    let mut refused: Vec<String> = Vec::new();
    for (m, r) in files {
        let f = match r {
            Ok(f) => f,
            Err(e) => {
                problems.push(e);
                continue;
            }
        };
        let name = inner
            .doc()
            .media()
            .get(&m)
            .map_or_else(|| m.to_string(), |x| x.name.clone());
        // Never the model's region-length refusal for a file without
        // audio (Task 1 carry); `relink_file` refuses one already.
        if f.file.length_frames == 0 {
            if f.made {
                refused.push(f.file.path.clone());
            }
            problems.push(format!("{name}: the file holds no audio"));
            continue;
        }
        let cmd = Command::Media(MediaCmd::Relink {
            media: m,
            path: f.file.path.clone(),
            content_hash: Some(f.file.content_hash.clone()),
            length_frames: f.file.length_frames,
            channels: f.file.channels,
            sample_rate: f.file.sample_rate,
        });
        match inner.history.execute(&cmd, &ViewFilter::default()) {
            Ok(_) => {
                inner.changed.remove(&m);
                done.push(m.0);
            }
            Err(e) => {
                if f.made {
                    refused.push(f.file.path.clone());
                }
                let why = match e {
                    core_model::ModelError::OutOfRange {
                        field: "relink.length_frames",
                        ..
                    } => "the file is shorter than the regions using this item".to_string(),
                    other => other.to_string(),
                };
                problems.push(format!("{name}: {why}"));
            }
        }
    }
    let keep = inner.kept();
    for p in refused {
        remove_unkept(root, &p, &keep);
    }
    (done, problems)
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
    reserved: &std::collections::BTreeSet<String>,
    progress: &std::sync::atomic::AtomicU32,
    cancel: &std::sync::atomic::AtomicBool,
    kind: crate::jobs::JobKindRow,
) -> crate::jobs::Work {
    let importer = crate::media::importer();
    let n = items.len().max(1) as f32;
    let mut files: ConvertFiles = Vec::new();
    for (k, (media, path, stem)) in items.into_iter().enumerate() {
        let r = crate::relink::regular_file(&path).and_then(|()| {
            core_media::import::import_reserved(
                importer.as_ref(),
                &path,
                root,
                format,
                &stem,
                reserved,
                &mut |f| {
                    progress.store(
                        ((k as f32 + f) / n).to_bits(),
                        std::sync::atomic::Ordering::Relaxed,
                    )
                },
                cancel,
            )
        });
        match r {
            Err(core_media::MediaError::Cancelled) => {
                discard(
                    root,
                    crate::jobs::Work::Convert {
                        kind,
                        format: None,
                        files,
                    },
                    &std::collections::BTreeSet::new(),
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
                    &std::collections::BTreeSet::new(),
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
    let keep = inner.kept();
    let remove = |files: &ConvertFiles| {
        for (_, r) in files {
            if let Ok(i) = r {
                remove_unkept(root, &i.path, &keep);
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

/// One region [`Session::normalize_regions`] measures, and what it was
/// measured against.
struct Measured {
    id: core_model::ids::EventId,
    media: core_model::ids::MediaId,
    /// The item's path and hash when measured (preflight M4: checked
    /// again before the command applies).
    path: String,
    hash: Option<String>,
    name: String,
    file: std::path::PathBuf,
    /// The overview in memory, if it is this file's (preflight I1).
    overview: Option<Arc<core_media::overview::Overview>>,
    /// The frames the region plays.
    from: u64,
    to: u64,
}

/// A Normalize note as the user reads it: the model names a region by its
/// event ID (`"ev12: gain limited …"`, `"ev4 is silent: …"`); the region's
/// media name replaces the ID only when the ID matches whole, never as a
/// prefix of a longer one (5b final review m1: `ev1` must not take
/// `ev12`'s note).
pub(crate) fn normalize_warning(w: &str, regions: &[(core_model::ids::EventId, &str)]) -> String {
    for (id, name) in regions {
        let id = id.to_string();
        if let Some(rest) = w.strip_prefix(id.as_str()) {
            if rest.starts_with(':') || rest.starts_with(' ') {
                return format!("Normalize: a region of {name}{rest}");
            }
        }
    }
    format!("Normalize: {w}")
}

/// The packages open in this process, by [`package_key`], with the
/// session holding each (review I-2: a second session on one package
/// would move the first one's recordings to the Trash at its save).
static PACKAGES: Mutex<std::collections::BTreeMap<String, u64>> =
    Mutex::new(std::collections::BTreeMap::new());

/// `path` as one package is named whatever the spelling: canonical (its
/// folder's, for a package not made yet), in lower case as macOS volumes
/// resolve names.
fn package_key(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| {
        let dir = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d,
            _ => Path::new("."),
        };
        match (std::fs::canonicalize(dir), path.file_name()) {
            (Ok(d), Some(n)) => d.join(n),
            _ => path.to_path_buf(),
        }
    });
    canonical.to_string_lossy().to_lowercase()
}

/// "Already open" for `path`.
fn already_open(path: &Path) -> VizError {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    VizError::Busy {
        message: format!("{name} is already open in another window."),
    }
}

/// A claim on a package (re-review m-1): released when dropped unless
/// [`PackageClaim::keep`] hands it to the session (which releases it at
/// `close`, drop or a Save As elsewhere). A panic or an early return
/// between claiming and the session existing therefore releases it.
#[must_use = "dropping the claim releases the package"]
struct PackageClaim {
    key: String,
    sid: u64,
    /// This claim inserted the entry (a session re-claiming its own
    /// package releases nothing on drop).
    armed: bool,
}

impl PackageClaim {
    /// The session holds the package from now on.
    fn keep(mut self) {
        self.armed = false;
    }
}

impl Drop for PackageClaim {
    fn drop(&mut self) {
        if self.armed {
            let mut m = lock(&PACKAGES);
            if m.get(&self.key) == Some(&self.sid) {
                m.remove(&self.key);
            }
        }
    }
}

/// Claims `path` for session `sid`, checked and inserted as one step under
/// the registry lock (re-review m-2: two concurrent opens cannot both
/// pass); the path is resolved before the lock is taken (m-3). `Busy`
/// when another session holds it.
fn claim_package(path: &Path, sid: u64) -> Result<PackageClaim, VizError> {
    let key = package_key(path);
    let mut m = lock(&PACKAGES);
    match m.entry(key.clone()) {
        std::collections::btree_map::Entry::Occupied(e) if *e.get() != sid => {
            Err(already_open(path))
        }
        std::collections::btree_map::Entry::Occupied(_) => Ok(PackageClaim {
            key,
            sid,
            armed: false,
        }),
        std::collections::btree_map::Entry::Vacant(e) => {
            e.insert(sid);
            Ok(PackageClaim {
                key,
                sid,
                armed: true,
            })
        }
    }
}

/// Session `sid` lets go of `path` (resolved before the lock).
fn release_package(path: &Path, sid: u64) {
    let key = package_key(path);
    let mut m = lock(&PACKAGES);
    if m.get(&key) == Some(&sid) {
        m.remove(&key);
    }
}

/// Session `sid` holds no package any more.
fn release_packages(sid: u64) {
    lock(&PACKAGES).retain(|_, s| *s != sid);
}

/// Adds fault name `f` to `faults` once.
fn add_fault(faults: &mut Vec<String>, f: &str) {
    if !faults.iter().any(|x| x == f) {
        faults.push(f.to_string());
    }
}

/// `Busy` while a recording runs or finishes.
fn recording_busy() -> VizError {
    VizError::Busy {
        message: "Stop recording first.".into(),
    }
}

/// "Busy" for Normalize.
fn still_building(name: &str) -> VizError {
    VizError::Busy {
        message: format!("The waveform of {name} is still being built; try again in a moment."),
    }
}

/// Region menu commands that need the overviews (plan 5b Task 5).
#[uniffi::export]
impl Session {
    /// Normalize… (spec §4.4; plan 5b Ruling 5): sets each region among
    /// `events` so its loudest sample reaches `peak_db` (−96…0 dBFS), as
    /// one undo step "Normalize". Events that are not regions are ignored.
    /// `Model` error for a bad target, no region, offline media or an open
    /// edit; `Busy` while a region's overview is still being built, or when
    /// an item's file changed while it was measured. No file is read under
    /// the session lock (preflight M4).
    pub fn normalize_regions(
        &self,
        events: Vec<u64>,
        peak_db: f32,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            use core_media::overview::{Overview, Stamp, exact_peak};
            use core_model::event::EventBody;
            if !(peak_db.is_finite() && (-96.0..=0.0).contains(&peak_db)) {
                return Err(crate::events::bad(
                    "the peak must be a level from -96 to 0 dB",
                ));
            }
            // Under the lock: what to measure and the overviews in memory.
            let (root, mut measure) = {
                let mut inner = lock(&self.inner);
                if inner.edit_label.is_some() {
                    return Err(crate::events::bad("finish the open edit first"));
                }
                inner.refresh_media(self.id, false);
                let root = inner.root(self.id);
                let doc = inner.doc().clone();
                let mut measure = Vec::new();
                for e in &events {
                    let id = core_model::ids::EventId(*e);
                    let (ev, _) = doc.event(id)?;
                    let EventBody::AudioRegion {
                        media,
                        source_offset,
                        length,
                        ..
                    } = ev.body
                    else {
                        continue;
                    };
                    let m = doc
                        .media()
                        .get(&media)
                        .ok_or_else(|| crate::events::bad(&format!("unknown media {media}")))?;
                    if inner.offline.contains_key(&media) {
                        return Err(crate::events::bad(&format!(
                            "{} is offline: relink it first",
                            m.name
                        )));
                    }
                    // Only an overview of the audio the item names now.
                    let overview = inner.overviews.get(media).filter(|o| {
                        m.content_hash
                            .as_deref()
                            .is_none_or(|h| h == o.hash.as_str())
                    });
                    measure.push(Measured {
                        id,
                        media,
                        path: m.path.clone(),
                        hash: m.content_hash.clone(),
                        name: m.name.clone(),
                        file: root.join(&m.path),
                        overview,
                        from: source_offset,
                        to: source_offset.saturating_add(length),
                    });
                }
                (root, measure)
            };
            if measure.is_empty() {
                return Err(crate::events::bad("select audio regions to normalize"));
            }
            // Without the lock: the cached overview file when it is this
            // file's (so Normalize works right after an import), and the
            // edge samples.
            for x in &mut measure {
                if x.overview.is_some() {
                    continue;
                }
                x.overview = x.hash.as_deref().and_then(|h| {
                    let o = Overview::read(&Overview::path(&root, h)?).ok()?;
                    let st = Stamp::of(&x.file).ok()?;
                    (o.hash == h && o.has_stamp(&st)).then(|| Arc::new(o))
                });
                if x.overview.is_none() {
                    return Err(still_building(&x.name));
                }
            }
            let io = |e: core_media::MediaError| VizError::Io {
                message: e.to_string(),
            };
            let mut regions = Vec::with_capacity(measure.len());
            for x in &measure {
                let Some(ov) = &x.overview else {
                    return Err(still_building(&x.name));
                };
                let r = core_media::caf::CafReader::open(&x.file).map_err(io)?;
                let mut exact = |a: u64, b: u64| exact_peak(&r, a, b);
                regions.push((x.id, ov.peak(x.from, x.to, &mut exact).map_err(io)?));
            }
            // Under the lock again: apply only if every item still names
            // the file measured and every region the frames measured.
            let cmd = Command::Audio(core_model::command::AudioCmd::Normalize { regions, peak_db });
            let mut inner = lock(&self.inner);
            if inner.edit_label.is_some() {
                return Err(crate::events::bad("finish the open edit first"));
            }
            inner.refresh_media(self.id, false);
            let doc = inner.doc().clone();
            for x in &measure {
                let same_region = matches!(
                    doc.event(x.id).map(|(ev, _)| &ev.body),
                    Ok(EventBody::AudioRegion { media, source_offset, length, .. })
                        if *media == x.media
                            && *source_offset == x.from
                            && source_offset.saturating_add(*length) == x.to
                );
                let same_file = doc
                    .media()
                    .get(&x.media)
                    .is_some_and(|m| m.path == x.path && m.content_hash == x.hash);
                if !(same_region && same_file) || inner.offline.contains_key(&x.media) {
                    return Err(VizError::Busy {
                        message: "The audio changed while it was measured; try again.".into(),
                    });
                }
            }
            // Normalize removes no plugin: nothing to capture first.
            let report = inner.history.execute(&cmd, &ViewFilter::default())?;
            // The model's notes ("silent", "gain limited") name the event
            // ID: the user reads the media name in the next `idle`'s
            // problems (final review m3 / T5 m2).
            let names: Vec<(core_model::ids::EventId, &str)> =
                measure.iter().map(|x| (x.id, x.name.as_str())).collect();
            for w in report.warnings {
                let line = normalize_warning(&w, &names);
                inner.news.problems.push(line);
            }
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            inner.snapshot()
        })
    }
}

/// Recording (plan 5c Task 9; spec §6.2–§6.5).
#[uniffi::export]
impl Session {
    /// Records into sequence `seq` from where its position is (spec §6.4):
    /// the transport rolls (from `pre_roll_bars` bars earlier), each armed
    /// lane with an audio input records its channels into its own file
    /// under `Media/Recorded/`, armed MIDI lanes record what they hear, and
    /// at Stop everything recorded becomes one undo step "Record". `Model`
    /// when no armed lane has an input, an audio lane names a channel the
    /// input does not have, the spec is out of range, or count-in is asked
    /// for (plan 5d plays it); `Busy` while recording or finishing a pass,
    /// while an edit is open, while a project format change has not been
    /// applied (review I-1), or when the disk is almost full. While it
    /// records (plan 5c Ruling 17) Play, Locate, Set Loop, another Record,
    /// Save As and a format change are `Busy`; Stop finishes the pass.
    pub fn record(&self, seq: u64, spec: crate::record::RecordSpecRow) -> Result<(), VizError> {
        self.transport_call(|| self.start_recording(seq, spec))
    }

    /// `record.keep_pass`: keeps every loop pass completed so far (they
    /// survive a later discard). `Model` unless loop recording.
    pub fn keep_pass(&self) -> Result<(), VizError> {
        ffi_guard(|| self.press_pass(true))
    }

    /// `record.discard_pass`: drops every loop pass completed since the
    /// last keep. `Model` unless loop recording.
    pub fn discard_pass(&self) -> Result<(), VizError> {
        ffi_guard(|| self.press_pass(false))
    }

    /// What recording is doing (`state.get record`).
    pub fn record_state(&self) -> Result<crate::record::RecordStateRow, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            Ok(match inner.recording.as_mut() {
                Some(r) => {
                    r.drain();
                    r.row()
                }
                None => crate::record::RecordStateRow {
                    recording: false,
                    seq: None,
                    spec: None,
                    pass: 0,
                    kept: 0,
                    discarded: 0,
                    lanes: Vec::new(),
                    frames: 0,
                    overflows: 0,
                    gap_frames: 0,
                    free_bytes: crate::record::free_bytes(&inner.root(self.id)),
                    underruns: 0,
                    late_starts: 0,
                    recovered: Vec::new(),
                    recovered_files: Vec::new(),
                },
            })
            .map(|mut row| {
                row.recovered = inner.recovered.iter().map(|r| r.path.clone()).collect();
                row.recovered_files = inner.recovered.clone();
                row
            })
        })
    }

    /// Recordings a crash left in this project, and takes kept for this
    /// offer, offered at open (spec §6.5: "Recovered recordings: import
    /// into a new lane / move to Trash"); empty once answered.
    pub fn recovered_recordings(&self) -> Result<Vec<String>, VizError> {
        ffi_guard(|| {
            Ok(lock(&self.inner)
                .recovered
                .iter()
                .map(|r| r.path.clone())
                .collect())
        })
    }

    /// [`Session::recovered_recordings`] with what each file holds: its
    /// frames, whether its size was written (a finished take nothing
    /// names) or not (a crash leftover), and whether it reads at all
    /// (pre-flight M9).
    pub fn recovered_files(&self) -> Result<Vec<crate::record::RecoveredFileRow>, VizError> {
        ffi_guard(|| Ok(lock(&self.inner).recovered.clone()))
    }

    /// Answers the offer: `import` puts each readable offered file on a
    /// new lane of the first top-level sequence at bar 1, as one undo step
    /// "Recover Recordings" (an unreadable one stays kept and is
    /// reported); otherwise each moves to the Trash (never unlinked; a file
    /// the Trash refuses stays kept and is reported). `Model` when there
    /// is nothing to recover; `Busy` while a recording runs, finishes or
    /// waits to be applied, or while an edit is open.
    pub fn recover_recordings(&self, import: bool) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if inner.recovered.is_empty() {
                return Err(crate::events::bad("there are no recovered recordings"));
            }
            if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
                return Err(recording_busy());
            }
            if inner.history.in_transaction() || inner.history.in_group() {
                return Err(VizError::Busy {
                    message: "Finish the open edit first.".into(),
                });
            }
            let root = inner.root(self.id);
            let files: Vec<String> = std::mem::take(&mut inner.recovered)
                .into_iter()
                .map(|r| r.path)
                .collect();
            if !import {
                let dir = crate::retention::trash_dir(inner.trash_dir.as_deref());
                for rel in files {
                    match crate::retention::trash(dir.as_deref(), &root.join(&rel)) {
                        Ok(()) => {
                            inner.keep_recordings.remove(&rel);
                        }
                        Err(e) => {
                            inner
                                .news
                                .problems
                                .push(format!("{rel}: not moved to the Trash ({e})"));
                            // Still in place: kept, offered at the next open.
                            inner.keep_recordings.insert(rel);
                        }
                    }
                }
                if let Some(p) = inner.path.clone() {
                    inner.write_kept(&p);
                }
                return inner.snapshot();
            }
            let (imported, problems) = match recover_into_lanes(&mut inner, &root, &files) {
                Ok(r) => r,
                Err(e) => {
                    // Offered again: nothing was applied.
                    inner.recovered = files
                        .iter()
                        .map(|f| crate::record::recovered_row(&root, f))
                        .collect();
                    return Err(e);
                }
            };
            inner.news.problems.extend(problems);
            for rel in &files {
                if imported.contains(rel) {
                    inner.keep_recordings.remove(rel);
                } else {
                    // Unreadable: kept, offered at the next open.
                    inner.keep_recordings.insert(rel.clone());
                }
            }
            if imported.is_empty() {
                return inner.snapshot();
            }
            inner.version += 1;
            inner.news.doc_changed = true;
            inner.news.media_changed = true;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            inner.snapshot()
        })
    }
}

/// [`Session::recover_recordings`]'s import: one transaction "Recover
/// Recordings" adding each readable file as a recorded media item and a
/// region at bar 1 on its own new lane (named after the file) in the
/// first top-level sequence. Returns the files imported and a problem for
/// each one that could not be read.
fn recover_into_lanes(
    inner: &mut Inner,
    root: &Path,
    files: &[String],
) -> Result<(Vec<String>, Vec<String>), VizError> {
    use core_model::command::{AudioCmd, MediaCmd};
    use core_model::document::{MediaItem, MediaOrigin};
    use core_model::event::Fade;
    use core_model::ids::AnyId;
    let seq = seq_rows(inner.doc())?
        .iter()
        .find(|r| r.top_level)
        .map(|r| SeqId(r.id))
        .ok_or_else(|| crate::events::bad("the project has no sequence to recover into"))?;
    let pass = crate::record::next_pass(inner.doc());
    let mut problems = Vec::new();
    let mut found = Vec::new();
    for rel in files {
        let path = root.join(rel);
        let info = core_media::caf::CafReader::open(&path).map(|r| r.info().clone());
        match (info, core_media::caf::hash_file(&path)) {
            (Ok(i), Ok(h)) if i.frames > 0 => found.push((rel.clone(), i, h)),
            (Ok(_), Ok(_)) => problems.push(format!("{rel}: it holds no audio; it is kept")),
            (Err(e), _) | (_, Err(e)) => problems.push(format!("{rel}: {e}; it is kept")),
        }
    }
    if found.is_empty() {
        return Ok((Vec::new(), problems));
    }
    let filter = ViewFilter::default();
    let h = &mut inner.history;
    h.begin("Recover Recordings")?;
    let run = (|| -> Result<(), VizError> {
        for (rel, info, hash) in &found {
            let (_, stem) = MediaItem::file_parts(rel);
            let r = h.execute(
                &Command::Media(MediaCmd::Add {
                    path: rel.clone(),
                    sample_rate: info.sample_rate,
                    channels: info.channels,
                    length_frames: info.frames,
                    content_hash: Some(hash.clone()),
                    name: Some(stem.clone()),
                    origin: Some(MediaOrigin::Recorded { pass }),
                }),
                &filter,
            )?;
            let media = r
                .created
                .iter()
                .find_map(|id| match id {
                    AnyId::Media(m) => Some(*m),
                    _ => None,
                })
                .ok_or_else(|| VizError::Internal {
                    message: "no media created".into(),
                })?;
            let r = h.execute(
                &Command::Lane(LaneCmd::Add {
                    seq,
                    name: stem,
                    index: None,
                }),
                &filter,
            )?;
            let lane = r
                .created
                .iter()
                .find_map(|id| match id {
                    AnyId::Lane(l) => Some(*l),
                    _ => None,
                })
                .ok_or_else(|| VizError::Internal {
                    message: "no lane created".into(),
                })?;
            h.execute(
                &Command::Audio(AudioCmd::Insert {
                    seq,
                    lane,
                    tick: Tick::ZERO,
                    media,
                    source_offset: 0,
                    length: info.frames,
                    gain_db: 0.0,
                    fade_in: Fade::default(),
                    fade_out: Fade::default(),
                }),
                &filter,
            )?;
        }
        Ok(())
    })();
    match run {
        Ok(()) => {
            h.commit()?;
            Ok((found.into_iter().map(|(r, _, _)| r).collect(), problems))
        }
        Err(e) => {
            let _ = h.abort();
            Err(e)
        }
    }
}

impl Session {
    fn press_pass(&self, keep: bool) -> Result<(), VizError> {
        let mut inner = lock(&self.inner);
        match inner.recording.as_mut() {
            Some(r) => r.press(keep),
            None => Err(crate::events::bad("nothing is recording")),
        }
    }

    /// [`Session::record`].
    fn start_recording(
        &self,
        seq: u64,
        spec: crate::record::RecordSpecRow,
    ) -> Result<(), VizError> {
        use crate::record::{AudioLane, LOW_DISK_STOP, LOW_DISK_WARN, Plan, Recording};
        use core_engine::record::RecordRequest;
        use core_media::record::{WriterConfig, WriterLane};
        use core_model::lane::LaneInput;
        let mut inner = lock(&self.inner);
        if inner.recording.is_some() || inner.finishing {
            return Err(VizError::Busy {
                message: "A recording is already running.".into(),
            });
        }
        if inner.history.in_transaction() || inner.history.in_group() {
            return Err(VizError::Busy {
                message: "Finish the open edit first.".into(),
            });
        }
        // Review I-1: the take would be planned at the old rate and land
        // in a project at the new one.
        if inner.jobs.pending(&[crate::jobs::JobKindRow::Format]) {
            return Err(VizError::Busy {
                message: "Wait for the project format change to finish, then record.".into(),
            });
        }
        if spec.count_in_bars > 4 || spec.pre_roll_bars > 8 {
            return Err(crate::events::bad(
                "count-in is 0–4 bars and pre-roll 0–8 bars",
            ));
        }
        if spec.count_in_bars > 0 {
            return Err(crate::events::bad(
                "count-in arrives with the record controls (plan 5d); record with a count-in of 0",
            ));
        }
        if let Some(p) = &spec.punch {
            core_model::rules::check_ticks("punch", p.start)?;
            core_model::rules::check_ticks("punch", p.end)?;
            if p.start >= p.end {
                return Err(crate::events::bad("the punch range is empty"));
            }
        }
        let doc = inner.doc().clone();
        let id = seq_id(&doc, seq)?;
        let s = doc.sequence(id)?;
        // An undone Track Synth leaves no "mute after" behind (Task 8
        // ruling M10).
        crate::synth::keep_live_mutes(&doc, &mut inner.synth_mutes);
        // Each armed audio lane with the synth whose return it records.
        let mut audio: Vec<(
            core_model::ids::LaneId,
            String,
            core_model::lane::InputChannels,
            Option<DestId>,
        )> = Vec::new();
        let mut midi_lanes = Vec::new();
        for l in s.lanes.iter().filter(|l| l.arm) {
            match &l.input {
                Some(LaneInput::Audio {
                    channels,
                    return_of,
                }) => audio.push((l.id, l.name.clone(), *channels, *return_of)),
                Some(LaneInput::Midi { .. }) => midi_lanes.push(l.id),
                None => {}
            }
        }
        if audio.is_empty() && midi_lanes.is_empty() {
            return Err(crate::events::bad("arm a lane that has an input to record"));
        }
        let root = inner.root(self.id);
        if !audio.is_empty() {
            match crate::record::free_bytes(&root) {
                Some(f) if f < LOW_DISK_STOP => {
                    return Err(VizError::Busy {
                        message: "The disk is almost full: free some space to record.".into(),
                    });
                }
                Some(f) if f < LOW_DISK_WARN => inner
                    .news
                    .problems
                    .push("Recording: less than 1 GB of disk space is left.".into()),
                _ => {}
            }
        }
        // Where it starts: the position as heard, then the pre-roll.
        let format = doc.settings().format;
        let rate = f64::from(format.sample_rate);
        let origin = Tick(-s.start_offset.0);
        let tempo = core_engine::schedule::TempoTable::build(&s.tempo, origin, rate);
        let at = match host::position(self.id) {
            Some((sq, t)) if sq == id => t,
            _ => match (inner.cursor, inner.cursor_seq) {
                (Some(t), None) => t,
                (Some(t), Some(sq)) if sq == id => t,
                _ => Tick::ZERO,
            },
        };
        let bar = s.meter.entry_at(at).meter.bar_ticks();
        let play_from = Tick(
            at.0.saturating_sub(bar.saturating_mul(i64::from(spec.pre_roll_bars)))
                .max(origin.0),
        );
        let frame = |t: i64| tempo.tick_to_frame(Tick(t));
        let punch = spec.punch.as_ref().map(|p| (frame(p.start), frame(p.end)));
        let looped = spec.looped.then(|| {
            let end = core_model::query::seq_length(&doc, id).unwrap_or(Tick::ZERO);
            (frame(0), frame(end.0))
        });
        let range = match (looped, punch) {
            (Some((a, b)), Some((pa, pb))) => crate::passes::RecordRange {
                start: a.max(pa),
                end: Some(b.min(pb)),
            },
            (Some((a, b)), None) => crate::passes::RecordRange {
                start: a,
                end: Some(b),
            },
            (None, Some((pa, pb))) => crate::passes::RecordRange {
                start: pa,
                end: Some(pb),
            },
            (None, None) => crate::passes::RecordRange {
                start: frame(at.0),
                end: None,
            },
        };
        if range.end.is_some_and(|e| e <= range.start) {
            return Err(crate::events::bad(
                "the record range is empty (an empty loop, or a punch outside it)",
            ));
        }
        // MIDI sources plugged in since the engine started are heard too
        // (a live host only: pre-flight M7).
        if !midi_lanes.is_empty() {
            host::listen_to_midi_sources();
        }
        // The engine plays (this loads the sequence and locates); the tap
        // goes in before the Play, so the roll is the first block captured.
        inner.mark(self.id, true);
        let played = plugins::with(self.id, |p| {
            host::play(self.id, &doc, id, Some(play_from), None, spec.looped, p)
        })
        .and_then(|r| r);
        let pending = match played {
            Ok(p) => p,
            Err(e) => {
                inner.reload_owed = true;
                return Err(e);
            }
        };
        let fail = |e: VizError| {
            let _ = host::stop_recording(self.id);
            Err(e)
        };
        // A device at another rate than the project's would record at the
        // wrong speed and place MIDI wrongly (5a Ruling 20: no resampling).
        if let Some((true, rate)) = host::device(self.id) {
            return fail(crate::events::bad(&format!(
                "the device runs at {rate} Hz, not the project's {} Hz: change one of them to record",
                format.sample_rate
            )));
        }
        let inputs = host::input_info(self.id).map_or(0, |(_, i)| i.channels);
        let faults = host::faults(self.id);
        for (_, name, ch, _) in &audio {
            if let Some(why) = crate::record::channel_refusal(name, *ch, inputs, &faults) {
                return fail(crate::events::bad(&why));
            }
        }
        let mut device: Vec<u16> = audio
            .iter()
            .flat_map(|(_, _, ch, _)| {
                let a = ch.first();
                [a, ch.last().unwrap_or(a)]
            })
            .collect();
        device.sort_unstable();
        device.dedup();
        let req = RecordRequest {
            channels: if audio.is_empty() {
                Vec::new()
            } else {
                device.clone()
            },
            range: Some((range.start, range.end.unwrap_or(u64::MAX))),
            ring_frames: crate::record::ring_frames(rate),
        };
        let handles = match host::start_recording(self.id, &req) {
            Ok(h) => h,
            Err(e) => return fail(e),
        };
        let mut reserved = inner.reserved_names();
        let now = std::time::SystemTime::now();
        let mut lanes = Vec::new();
        let mut writer_lanes = Vec::new();
        for (lane, name, ch, ret) in &audio {
            let rel = crate::record::recorded_path(&root, name, now, &reserved);
            reserved.insert(rel.clone());
            let index = |c: u16| device.iter().position(|d| *d == c).unwrap_or(0);
            let mut chans = vec![index(ch.first())];
            if let Some(r) = ch.last().filter(|r| *r != ch.first()) {
                chans.push(index(r));
            }
            writer_lanes.push(WriterLane {
                path: root.join(&rel),
                channels: chans,
            });
            lanes.push(AudioLane {
                lane: *lane,
                name: name.clone(),
                path: rel,
                channels: ch.count(),
                // Read once, at Record start (plan 5c-2 Ruling 4): a change
                // during the recording, even between loop passes, applies
                // from the next recording.
                return_frames: crate::synth::return_frames(&doc, *ret, rate),
            });
        }
        let writer = (!lanes.is_empty()).then_some(WriterConfig {
            rate: format.sample_rate,
            depth: format.depth,
            lanes: writer_lanes,
        });
        let plan = Plan {
            owner: self.id,
            seq: id,
            spec: spec.clone(),
            range,
            audio: lanes,
            midi_lanes,
            root,
            format,
            pass_number: crate::record::next_pass(&doc),
        };
        let recording = match Recording::start(plan, handles, writer) {
            Ok(r) => r,
            Err(e) => {
                return fail(VizError::Io {
                    message: format!("the recording could not start: {e}"),
                });
            }
        };
        if let Err(e) = inner.history.begin_group() {
            return fail(e.into());
        }
        inner.recording = Some(recording);
        inner.record_faults.clear();
        inner.looped = spec.looped;
        inner.reload_owed = false;
        inner.reload_fault = None;
        inner.cursor = None;
        inner.cursor_seq = Some(id);
        drop(inner);
        // A Play that cannot be sent ends the recording it started
        // (pre-flight M4): nothing would ever roll or end it.
        if let Err(e) = host::finish_play(pending) {
            let _ = self.finish_recording(None);
            return Err(e);
        }
        Ok(())
    }

    /// Waits while another thread finishes a pass (up to
    /// [`crate::record::FINISH_WAIT`] and some), `Busy` after.
    fn wait_finished(&self) -> Result<(), VizError> {
        let until = Instant::now() + crate::record::FINISH_WAIT * 4;
        while lock(&self.inner).finishing {
            if Instant::now() >= until {
                return Err(recording_busy());
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }

    /// Ends the recording in progress (spec §6.5): the engine stops and
    /// drops its tap, the files are finished without the session lock,
    /// then the pass is applied as the last step of the recording's group,
    /// which closes as one undo step. `reason` is told to the user.
    fn finish_recording(&self, reason: Option<String>) -> Result<(), VizError> {
        let rec = {
            let mut inner = lock(&self.inner);
            let Some(r) = inner.recording.take() else {
                return Ok(());
            };
            inner.finishing = true;
            // In the keep set while the lock is down (pre-flight I1).
            inner.finishing_files = r.files();
            let _ = host::stop_recording(self.id);
            r
        };
        let done = rec.finish(crate::record::FINISH_WAIT);
        let mut inner = lock(&self.inner);
        inner.finishing = false;
        if let Some(why) = reason {
            inner
                .news
                .problems
                .push(format!("Recording stopped: {why}."));
        }
        // Input lost on the way (pre-flight I4, M11).
        if let Some(line) = crate::record::gaps_problem(&done) {
            inner.news.problems.push(line);
            if done.written.as_ref().is_some_and(|w| w.gap_frames > 0)
                || done.status.lost_blocks() > 0
            {
                add_fault(&mut inner.record_faults, crate::record::RECORD_OVERFLOW);
            }
        }
        let files = done.files();
        let doc = inner.doc().clone();
        let pass = match crate::record::build(&done, &doc) {
            Ok(p) => {
                // Takes no region uses (pre-flight I3).
                let left = crate::record::leftovers(&done, &doc, p.as_ref());
                inner.news.problems.extend(left.problems);
                inner.keep_recordings.extend(left.keep);
                p
            }
            Err(e) => {
                inner.news.problems.push(format!(
                    "The recording could not be placed: {e}. Its files are kept."
                ));
                inner.keep_recordings.extend(files.iter().cloned());
                None
            }
        };
        inner.pending_pass = Some((pass, files));
        inner.finishing_files.clear();
        self.apply_pending_pass(&mut inner);
        Ok(())
    }

    /// Applies a finished recording's pass and closes its group, unless
    /// an edit is still open (then `end_edit` or the next `idle` does).
    fn apply_pending_pass(&self, inner: &mut Inner) {
        if inner.history.in_transaction() {
            return;
        }
        let Some((pass, files)) = inner.pending_pass.take() else {
            return;
        };
        let label = pass
            .as_ref()
            .map_or("Record", core_model::command::RecordPass::name);
        if let Some(p) = pass {
            // Track Synth's "Mute the MIDI lane after recording" (spec
            // §6.7): inside the group, so the mute is part of "Record"
            // (plan 5c-2 Ruling 6).
            let mutes = crate::synth::mutes_after(&p, &inner.synth_mutes);
            match inner
                .history
                .execute(&Command::Record(p), &ViewFilter::default())
            {
                Err(e) => {
                    inner.news.problems.push(format!(
                        "The recording could not be added ({e}). Its files are kept."
                    ));
                    inner.keep_recordings.extend(files);
                }
                Ok(_) => {
                    for (key, seq, lane) in mutes {
                        inner.synth_mutes.remove(&key);
                        let mute = Command::Lane(LaneCmd::SetFlags {
                            seq,
                            lane,
                            mute: Some(true),
                            solo: None,
                            arm: None,
                        });
                        if let Err(e) = inner.history.execute(&mute, &ViewFilter::default()) {
                            inner.news.problems.push(format!(
                                "The MIDI lane could not be muted after recording ({e})."
                            ));
                        }
                    }
                }
            }
        }
        let _ = inner.history.end_group(label);
        inner.version += 1;
        inner.news.doc_changed = true;
        inner.news.media_changed = true;
        inner.reload_host(self.id);
        self.mark(inner);
    }

    /// `idle`'s recording step: ends the recording when the input went
    /// away, the disk failed or filled, or the transport stopped by itself.
    fn service_recording(&self) -> Result<(), VizError> {
        let ending = {
            let mut inner = lock(&self.inner);
            let stopped = host::transport_stopped(self.id).unwrap_or(false);
            let (mut problems, mut faults) = (Vec::new(), Vec::new());
            let e = inner
                .recording
                .as_mut()
                .and_then(|r| r.check(stopped, &mut problems, &mut faults));
            inner.news.problems.extend(problems);
            for f in faults {
                add_fault(&mut inner.record_faults, &f);
            }
            e
        };
        match ending {
            None => Ok(()),
            Some(crate::record::Ending::Ended) => self.finish_recording(None),
            Some(crate::record::Ending::Because(why)) => self.finish_recording(Some(why)),
        }
    }
}

/// Tracking a hardware synth (plan 5c-2; spec §6.7).
#[uniffi::export]
impl Session {
    /// Track Synth… on MIDI lane `lane` of `seq` (spec §6.7), as one undo
    /// step "Track Synth": the lane recording its external synth's return
    /// (reused, else "<synth> audio" just below) records device input
    /// `first` (and the next with `stereo`), has an output path and is
    /// armed; the MIDI lane is armed with `arm_midi`, else disarmed (plan
    /// 5c-2 Ruling 5). With `mute_after`, the next pass that records that
    /// return also mutes the MIDI lane, in its own "Record" step (Ruling
    /// 6). `Model` when the lane plays no external synth or the channels
    /// are out of range; `Busy` while recording (Ruling 14).
    pub fn track_synth(
        &self,
        seq: u64,
        lane: u64,
        first: u16,
        stereo: bool,
        arm_midi: bool,
        mute_after: bool,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            // One lock from the checks to the bookkeeping, as Record start
            // holds it (Task 8 review M2): a Record from another thread
            // runs wholly before or after. Track Synth removes no plugin,
            // so nothing is captured first.
            let mut inner = lock(&self.inner);
            if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
                return Err(recording_busy());
            }
            // An open edit's restart would drop it (review M1).
            if inner.edit_label.is_some() || inner.history.in_transaction() {
                return Err(crate::events::bad("finish the open edit first"));
            }
            let id = seq_id(inner.doc(), seq)?;
            let channels = if stereo {
                core_model::lane::InputChannels::Stereo(first)
            } else {
                core_model::lane::InputChannels::Mono(first)
            };
            let cmd = Command::Lane(LaneCmd::TrackSynth {
                seq: id,
                lane: LaneId(lane),
                channels,
                arm_midi,
            });
            inner.history.execute(&cmd, &ViewFilter::default())?;
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            let doc = inner.doc();
            let audio = doc
                .lane(id, LaneId(lane))
                .ok()
                .and_then(|l| l.midi_dest)
                .and_then(|d| doc.sequence(id).ok()?.return_lane(d, Some(LaneId(lane))));
            if let Some(audio) = audio {
                if mute_after {
                    inner.synth_mutes.insert((id, audio), LaneId(lane));
                } else {
                    inner.synth_mutes.remove(&(id, audio));
                }
            }
            inner.snapshot()
        })
    }

    /// Measure (spec §6.7; plan 5c-2 Rulings 8–9): with the transport
    /// stopped, plays five notes to external destination `dest` and
    /// listens on the input of the lane recording its return; sets its
    /// return latency to what it found, one undo step "Set Return
    /// Latency". Blocks for about 2.7 s (the session stays usable: its
    /// lock is released while it waits); call it off the main thread.
    /// `Model` for a destination that is not an external synth (the
    /// built-in synth included) or that no lane records the return of,
    /// for a channel the input lacks, and with "No signal on the synth's
    /// return input" when fewer than 3 onsets were found (nothing changes
    /// then); `Busy` while this session records or edits, while any
    /// session plays or records, and while another measurement runs;
    /// `Engine` when the measurement stopped before it finished (the
    /// device went away, or its sequence was deleted).
    pub fn measure_return(&self, dest: u64) -> Result<crate::synth::MeasureRow, VizError> {
        self.transport_call(|| {
            let dest = DestId(dest);
            let (doc, seq, name, channels) = {
                let inner = lock(&self.inner);
                if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
                    return Err(recording_busy());
                }
                if inner.edit_label.is_some() || inner.history.in_transaction() {
                    return Err(VizError::Busy {
                        message: "Finish the open edit first.".into(),
                    });
                }
                let doc = inner.doc().clone();
                let d = crate::synth::external_dest(&doc, dest)?;
                let Some((seq, _, name, channels)) = crate::synth::return_lane(&doc, dest) else {
                    return Err(crate::events::bad(&format!(
                        "no lane records the return of {}: set one up with Track Synth first",
                        d.name
                    )));
                };
                (doc, seq, name, channels)
            };
            let status = {
                let mut inner = lock(&self.inner);
                inner.mark(self.id, true);
                plugins::with(self.id, |p| {
                    host::start_measure(self.id, &doc, seq, dest, &name, channels, p)
                })
                .and_then(|r| r)?
            };
            // The notes take about 2.6 s; nothing here holds a lock. A
            // test may hold the measurement past its end (pre-flight M5).
            let until = Instant::now() + crate::synth::measure_wait();
            while (!status.done() && !status.closed() && Instant::now() < until)
                || host::measure_held()
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let done = status.done();
            let ended = host::end_measure(self.id, &doc, seq);
            if !done {
                return Err(VizError::Engine {
                    message: if status.closed() {
                        "the measurement stopped before it finished: the audio device went \
                         away or its sequence was deleted"
                            .into()
                    } else {
                        "the measurement did not finish in time".into()
                    },
                });
            }
            ended?;
            let m = crate::synth::measured_status(&status, crate::record::latency_trim())
                .map_err(|e| crate::events::bad(&e))?;
            let snapshot = self.execute(Command::Routing(RoutingCmd::SetReturnLatency {
                dest,
                ms: m.ms,
            }))?;
            Ok(crate::synth::MeasureRow {
                return_latency_ms: m.ms,
                onsets: m.onsets,
                snapshot,
            })
        })
    }

    /// Sets external destination `dest`'s return latency to `ms` (spec
    /// §4.5a: 0–500), one undo step "Set Return Latency". `Model` for
    /// another destination or a value out of range.
    pub fn set_return_latency(&self, dest: u64, ms: f32) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Routing(RoutingCmd::SetReturnLatency {
                dest: DestId(dest),
                ms,
            }))
        })
    }
}

/// Live input (plan 5c Task 8; spec §6.1, §6.6, §12.1).
#[uniffi::export]
impl Session {
    /// What the audio input offers while this document holds the engine
    /// (nothing before its first Play or Record).
    pub fn input_info(&self) -> Result<crate::live::InputInfoRow, VizError> {
        ffi_guard(|| {
            Ok(match host::input_info(self.id) {
                Some((device, i)) => crate::live::InputInfoRow {
                    device,
                    channels: i.channels,
                    names: i.names,
                    input_latency_frames: i.input_latency_frames,
                    output_latency_frames: i.output_latency_frames,
                },
                None => crate::live::InputInfoRow {
                    device: String::new(),
                    channels: 0,
                    names: Vec::new(),
                    input_latency_frames: 0,
                    output_latency_frames: 0,
                },
            })
        })
    }

    /// Feeds `bytes` (channel messages, one after another) to the engine
    /// as if source `port` sent them now (`midi.inject`): lanes listening
    /// to that source play them thru and, armed and recording, record
    /// them. False when this document does not hold the engine (nothing
    /// is sent). `Model` for bytes that are not channel messages or a bad
    /// port name; `Busy` when the input queue is full.
    pub fn inject_midi(&self, port: String, bytes: Vec<u8>) -> Result<bool, VizError> {
        ffi_guard(|| {
            let msgs = crate::live::parse_injection(&port, &bytes)?;
            let Some(tx) = host::midi_sender(self.id) else {
                return Ok(false);
            };
            let key = core_engine::input::port_key(&port);
            for msg in msgs {
                let sent = tx.send(core_engine::input::MidiIn {
                    port: key,
                    host_ns: 0,
                    msg,
                });
                if !sent {
                    return Err(VizError::Busy {
                        message: "the MIDI input queue is full".into(),
                    });
                }
            }
            Ok(true)
        })
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
            // Past the end there is nothing to read or wait for (review I1).
            if from_frame >= m.length_frames {
                return Ok(OverviewRows::silent(
                    OverviewStateRow::Ready,
                    m.channels,
                    columns,
                ));
            }
            let to = to_frame.min(m.length_frames);
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
                    let (a, b) = (frame(t0 + c as f64 * w), frame(t0 + (c + 1) as f64 * w));
                    // NaN before clamping: `max`/`min` would hide it.
                    if a.is_nan() || b.is_nan() {
                        return None;
                    }
                    let (a, b) = (a.max(0.0), b.min(len));
                    if b <= a {
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
            let reserved = inner.reserved_names();
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
                        let stem = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let r = crate::relink::regular_file(&path).and_then(|()| {
                            core_media::import::import_reserved(
                                importer.as_ref(),
                                &path,
                                &root,
                                format,
                                &stem,
                                &reserved,
                                &mut |f| {
                                    progress.store(
                                        ((k as f32 + f) / n).to_bits(),
                                        std::sync::atomic::Ordering::Relaxed,
                                    )
                                },
                                cancel,
                            )
                        });
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
            // A take is planned and filed at the project rate (review
            // I-1): not while one records, finishes or waits to land.
            if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
                return Err(recording_busy());
            }
            self.apply_jobs_after_edit(&mut inner);
            inner.media_jobs_idle()?;
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
            let reserved = inner.reserved_names();
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
                        &reserved,
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
            self.apply_jobs_after_edit(&mut inner);
            inner.media_jobs_idle()?;
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
            let reserved = inner.reserved_names();
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Convert, move |progress, cancel| {
                    convert_files(
                        &root,
                        items,
                        f,
                        &reserved,
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

impl Session {
    /// A media-pool edit as one undo step; refused while an edit (a drag)
    /// is open.
    fn pool_edit(&self, cmd: Command) -> Result<DocSnapshot, VizError> {
        if lock(&self.inner).edit_label.is_some() {
            return Err(crate::events::bad("finish the open edit first"));
        }
        self.execute(cmd)
    }

    /// The item `media`, or a `Model` error.
    fn media_item(&self, media: u64) -> Result<core_model::document::MediaItem, VizError> {
        lock(&self.inner)
            .doc()
            .media()
            .get(&core_model::ids::MediaId(media))
            .cloned()
            .ok_or_else(|| crate::events::bad(&format!("unknown media {media}")))
    }
}

/// The media pool window (spec §10.3; plan 5b Task 6).
#[uniffi::export]
impl Session {
    /// Renames media item `media` (display name only; trimmed, not empty):
    /// one undo step "Rename Media".
    pub fn rename_media(&self, media: u64, name: String) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(crate::events::bad("a media name cannot be empty"));
            }
            self.media_item(media)?;
            self.pool_edit(Command::Media(core_model::command::MediaCmd::Rename {
                media: core_model::ids::MediaId(media),
                name,
            }))
        })
    }

    /// Removes media item `media` and every region using it, as one undo
    /// step "Remove Media" (the app asks first; Ruling 13).
    pub fn remove_media(&self, media: u64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            use core_model::command::{EventCmd, MediaCmd};
            let id = core_model::ids::MediaId(media);
            self.media_item(media)?;
            let mut inner = lock(&self.inner);
            if inner.edit_label.is_some() {
                return Err(crate::events::bad("finish the open edit first"));
            }
            let users = core_model::command::media::regions_using(inner.doc(), id);
            let filter = ViewFilter::default();
            inner.history.begin("Remove Media")?;
            let run = (|| -> Result<(), VizError> {
                if !users.is_empty() {
                    inner
                        .history
                        .execute(&Command::Event(EventCmd::Delete { events: users }), &filter)?;
                }
                inner.history.execute(
                    &Command::Media(MediaCmd::Remove {
                        media: id,
                        replacement: None,
                    }),
                    &filter,
                )?;
                Ok(())
            })();
            match run {
                Ok(()) => inner.history.commit()?,
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

    /// Removes every item no region uses: one undo step "Compact Media".
    /// The files go to the Trash by the retention rule once no undo or
    /// redo state needs them (Ruling 11).
    pub fn compact_media(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| self.pool_edit(Command::Media(core_model::command::MediaCmd::Compact)))
    }

    /// The regions using media item `media`.
    pub fn media_users(&self, media: u64) -> Result<Vec<crate::media::MediaUseRow>, VizError> {
        ffi_guard(|| {
            self.media_item(media)?;
            Ok(crate::media::users(
                lock(&self.inner).doc(),
                core_model::ids::MediaId(media),
            ))
        })
    }

    /// The absolute path of media item `media`'s file (Reveal in Finder).
    pub fn media_file(&self, media: u64) -> Result<String, VizError> {
        ffi_guard(|| {
            let m = self.media_item(media)?;
            let root = lock(&self.inner).root(self.id);
            Ok(root.join(&m.path).to_string_lossy().into_owned())
        })
    }

    /// Relinks media item `media` to the file at `path` (absolute; spec
    /// §10.3 "Locate…"): a job of kind `Relink` that imports (converts)
    /// the file into `Media/` unless it already is a VizDAW CAF in the
    /// project format there, then applies one undo step "Relink Media".
    pub fn relink_media(&self, media: u64, path: String) -> Result<u64, VizError> {
        ffi_guard(|| {
            let item = self.media_item(media)?;
            let src = PathBuf::from(&path);
            if path.is_empty() || !src.is_absolute() {
                return Err(crate::events::bad(
                    "relink needs the absolute path of a file",
                ));
            }
            // Never a FIFO, socket, device or folder: opening one could
            // block the job for good (Task 6 review m2).
            crate::relink::regular_file(&src).map_err(|e| crate::events::bad(&e.to_string()))?;
            let mut inner = lock(&self.inner);
            self.apply_jobs_after_edit(&mut inner);
            inner.media_jobs_idle()?;
            let root = inner.root(self.id);
            let format = inner.doc().settings().format;
            let named = inner.named_media();
            let unapplied = inner.jobs.unapplied();
            let id = core_model::ids::MediaId(media);
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Relink, move |progress, cancel| {
                    let importer = crate::media::importer();
                    let r = crate::relink::relink_file(
                        importer.as_ref(),
                        &root,
                        &src,
                        format,
                        &named,
                        &unapplied,
                        &mut |f| progress.store(f.to_bits(), std::sync::atomic::Ordering::Relaxed),
                        cancel,
                    );
                    match r {
                        Err(core_media::MediaError::Cancelled) => crate::jobs::Work::Cancelled,
                        r => crate::jobs::Work::Relink {
                            files: vec![(id, r.map_err(|e| format!("{}: {e}", item.name)))],
                        },
                    }
                })
        })
    }

    /// Searches `folder` (absolute) for every item offline as missing,
    /// changed in length or changed in content (spec §10.3 "Search
    /// folder…", Ruling 10): a job of kind `Relink`, one undo step per
    /// item relinked, a problem per item not found.
    pub fn search_media(&self, folder: String) -> Result<u64, VizError> {
        ffi_guard(|| {
            use crate::media::OfflineReasonRow;
            let dir = PathBuf::from(&folder);
            if folder.is_empty() || !dir.is_absolute() {
                return Err(crate::events::bad(
                    "search needs the absolute path of a folder",
                ));
            }
            let mut inner = lock(&self.inner);
            self.apply_jobs_after_edit(&mut inner);
            inner.media_jobs_idle()?;
            inner.refresh_media(self.id, false);
            let root = inner.root(self.id);
            let lost: Vec<crate::relink::Lost> = inner
                .doc()
                .media()
                .values()
                .filter(|m| {
                    matches!(
                        inner.offline.get(&m.id),
                        Some(
                            OfflineReasonRow::Missing
                                | OfflineReasonRow::Length
                                | OfflineReasonRow::Changed
                        )
                    )
                })
                .map(|m| crate::relink::Lost {
                    media: m.id,
                    name: m.name.clone(),
                    stem: core_model::document::MediaItem::file_parts(&m.path).1,
                    hash: m.content_hash.clone(),
                    seconds: m.length_frames as f64 / f64::from(m.sample_rate.max(1)),
                    file: root.join(&m.path),
                    changed: matches!(inner.offline.get(&m.id), Some(OfflineReasonRow::Changed)),
                })
                .collect();
            if lost.is_empty() {
                return Err(crate::events::bad("no media item is missing"));
            }
            let format = inner.doc().settings().format;
            let named = inner.named_media();
            let unapplied = inner.jobs.unapplied();
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Relink, move |progress, cancel| {
                    let importer = crate::media::importer();
                    crate::relink::search(
                        importer.as_ref(),
                        &root,
                        &dir,
                        lost,
                        format,
                        &named,
                        &unapplied,
                        progress,
                        cancel,
                    )
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_warning;
    use core_model::ids::EventId;

    /// 5b final review m1: a note about `ev12` is never named after
    /// `ev1`'s media, whichever comes first.
    #[test]
    fn a_normalize_note_names_the_region_its_id_matches_whole() {
        let regions = [(EventId(1), "kick"), (EventId(12), "snare")];
        assert_eq!(
            normalize_warning("ev12: gain limited to +24.0 dB", &regions),
            "Normalize: a region of snare: gain limited to +24.0 dB"
        );
        assert_eq!(
            normalize_warning("ev1 is silent: not normalized", &regions),
            "Normalize: a region of kick is silent: not normalized"
        );
        assert_eq!(
            normalize_warning("ev123: gain limited to +24.0 dB", &regions),
            "Normalize: ev123: gain limited to +24.0 dB"
        );
        assert_eq!(
            normalize_warning("nothing to normalize", &regions),
            "Normalize: nothing to normalize"
        );
    }
}
