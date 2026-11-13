//! `Session`: one open document (one window).

use crate::automation::{
    AutomationModeRow, AutomationStateRow, GesturePhaseRow, Holder, Move, PassStep,
    Path as TouchPath, PluginMove, PluginTouches, PreviewPointRow, Release, Sampler, Source,
    StripModeRow, TouchHolder, TouchHolders, Touching, UnsentEnds, Writing,
};
use crate::autowrite::WriteMode;
use crate::editor::{self, EditorSize};
use crate::error::VizError;
use crate::events::{AutomationTargetRow, automation_target_of, automation_target_row};
use crate::guard::{ffi_guard, lock};
use crate::host;
use crate::mixer::{self, DestRow, MidiMixRow, MixerLevels, MixerSnapshot};
use crate::plugins;
use crate::recovery;
use crate::snapshot::{
    DocSnapshot, IdleUpdate, LaneRow, SlotRow, TransportState, lane_rows, position_bbu, seconds,
    seq_id, seq_rows, warnings,
};
use crate::snapshot::{ParamRow, ParamValue, PluginRefRow};
use crate::surfaces::{
    LearnMapRow, LearnModeRow, LearnSourceRow, SurfaceEventRow, SurfaceStateRow,
};
use core_engine::gesture::{GestureMsg, GesturePhase};
use core_engine::schedule::AutoDest;
use core_host::PluginHost;
use core_model::automation::{AutomationMode, AutomationTarget};
use core_model::command::Command;
use core_model::command::MixerCmd;
use core_model::command::RoutingCmd;
use core_model::command::SettingsCmd;
use core_model::command::lane::LaneCmd;
use core_model::command::sequence::SequenceCmd;
use core_model::command::{AutomationWrite, RecordPass};
use core_model::document::DestKind;
use core_model::document::Document;
use core_model::file;
use core_model::history::{History, Timestamp};
use core_model::ids::{DestId, LaneId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{AuxSend, Output, ParamId, PluginRef, PluginSlot};
use core_model::selection::ViewFilter;
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::Tick;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Package extension.
pub const EXTENSION: &str = "vizdaw";

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct Inner {
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
    /// An arm's engine claim found another window's idle engine opening
    /// its input: `idle` tries the claim again for this sequence until it
    /// is taken or refused for another reason (Task 8 parked n2).
    claim_owed: Option<SeqId>,
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
    /// The audio settings epoch this session's engine last loaded under
    /// (plan 5d-1 Ruling 12).
    settings_seen: u64,
    /// A Save came while recording: the pass is saved when it lands (plan
    /// 5d-1 Ruling 19).
    save_after_pass: bool,
    /// The targets touched now, with their paths (plan 5e-1 Rulings 14,
    /// 20).
    touches: std::collections::BTreeMap<AutomationTarget, Touching>,
    /// The shared static edit's label and its open static touches
    /// (Ruling 14).
    static_edit: Option<(String, usize)>,
    /// The 10 ms sampler (Ruling 8).
    sampler: Sampler,
    /// The plugin windows' touches (Ruling 15): a value-only plugin's are
    /// released 500 ms after its last value.
    plugin_touches: PluginTouches,
    /// The document whose plugin-parameter automation `plugin_auto`
    /// holds (Task 9 review I-1): its sequences are compared by pointer.
    plugin_auto_doc: Document,
    /// Every plugin parameter's automation in `plugin_auto_doc`.
    plugin_auto: std::collections::BTreeMap<AutomationTarget, Vec<(SeqId, Tick, u64)>>,
    /// Gesture ends the engine's queue refused (Ruling 10).
    unsent_ends: UnsentEnds,
    /// The pass writing while playing (Rulings 7, 23, 24).
    writing: Option<Writing>,
    /// A finished pass's writes, the Write strips that still exist and
    /// the After Write mode, waiting for an open edit to end.
    /// A pass a device rebuild ended, to start again in this sequence once
    /// the rebuilt engine plays (controller ruling 3 on Task 8).
    restart_writing: Option<SeqId>,
    /// One entry per pass, landed as one step each (review m2), or into
    /// the step its loop passes landed in (plan 5e-1 Task 12).
    pending_automation: Vec<crate::automation::Landing>,
    /// The Write strips (and the After Write mode) whose mode changes
    /// when a recording's pass with automation lands (Ruling 9).
    after_write_pending: Option<(Vec<StripId>, AutomationMode)>,
    /// The undo step the last landing outside a recording made or went
    /// into (Task 12: a looped pass a rebuild restarts keeps its step).
    last_step: Option<u64>,
    /// The step a pass a rebuild ended had, for the pass that restarts.
    restart_step: Option<u64>,
    /// Loop passes of the recording landed into its group (Task 12): its
    /// step is "Write Automation" when it recorded nothing else.
    landed_in_group: bool,
    /// Control surfaces: the app's events, selection, last wind, heard
    /// values' cache (plan 5f-1 Task 8).
    surfaces: crate::surfaces::SessionSurfaces,
    /// A transport bar hold (a wind or a held key's step) moved the
    /// position without landing an open automation pass: its end lands
    /// it once (plan 7d controller ruling I4).
    wind_hold: bool,
    /// The meter holds of the mixer window and the surfaces (plan 5f-1
    /// Ruling 13, pre-flight I4).
    meters: crate::surfaces::MeterHolds,
    /// The running bounce's job and its plugin copies' token (plan 5f-2
    /// Ruling 6): ended once the job is no longer pending, or at close.
    bounce_copies: Option<(u64, u64)>,
    /// Finished bounces' files and media, by job (Ruling 10).
    bounce_results: std::collections::BTreeMap<u64, crate::bounce::BounceResultRow>,
    /// The last MIDI import's or export's report (plan 6a; runtime only).
    interchange_report: Option<crate::interchange::ReportRow>,
    /// Each region's external-edit copy while the project is open (plan
    /// 5f-3 Ruling 3; runtime only): retention keeps them, close lets them go.
    edits: std::collections::BTreeMap<core_model::ids::EventId, crate::external::EditCopy>,
    /// Copies being written without the lock (pre-flight M4): retention
    /// waits for them.
    edits_writing: u32,
    /// MPE settings controllers' MCMs gave lanes (plan 5g-2 Ruling 4), the
    /// last per lane, waiting to be written back once no recording or edit
    /// is open; a user's own setting of the lane drops its entry.
    mpe_owed: std::collections::BTreeMap<LaneId, Option<core_model::mpe::MpeConfig>>,
    /// The model's notes (a crossfade shortened or dropped) from the edit
    /// steps of the open edit, waiting for the snapshot `end_edit` returns
    /// (plan 6c pre-flight F1). Every other snapshot carries only its own
    /// command's notes; a conversion's notes live in its job row.
    last_warnings: Vec<Note>,
    /// Destinations' mix and MPE configuration batches a full engine
    /// command ring refused, oldest first: `idle` sends them again
    /// (final review m4).
    mix_owed: Vec<(DestId, Vec<core_engine::midi1::Midi1Msg>)>,
    /// The next bounce cancels itself once its progress reaches this
    /// (test hook).
    #[cfg(feature = "test-hooks")]
    bounce_cancel_at: Option<f32>,
    /// The next bounce fails its "Add to project" import of this source
    /// (test hook).
    #[cfg(feature = "test-hooks")]
    bounce_fail_import_at: Option<u32>,
    /// The next export for another DAW cancels itself after this stage
    /// (test hook: 1 after its first MIDI file, 2 after the About file).
    #[cfg(feature = "test-hooks")]
    for_daw_cancel_at: Option<u32>,
    /// The slots running here when the plug-in host was last looked at
    /// (on load, failure and restart; at each export): a DAWproject check
    /// on a thread without the host reads it (plan 6d Task 9).
    live_slots: std::collections::BTreeSet<PluginInstanceId>,
    /// The next DAWproject export cancels itself after this stage (test
    /// hook: 1 after the files, 2 after the first WAV, 3 after
    /// `project.xml`, 4 before the rename).
    #[cfg(feature = "test-hooks")]
    dawproject_cancel_at: Option<u32>,
    /// The free bytes DAWproject exports assume (test hook).
    #[cfg(feature = "test-hooks")]
    dawproject_free: Option<u64>,
    /// DAWproject exports and checks treat every running slot's capture as
    /// failed (test hook).
    #[cfg(feature = "test-hooks")]
    dawproject_capture_fails: bool,
    /// A merge between its preview and Merge (plan 6e Ruling 12): it
    /// counts as a running interchange and media job until
    /// `confirm_merge`, `cancel_merge` or close.
    held_merge: Option<HeldMerge>,
}

/// A held merge (plan 6e Task 8): its job, the merged history and the
/// media its job converted.
struct HeldMerge {
    /// The merge job's ID.
    job: u64,
    /// The merge.
    merged: Box<crate::dawproject::Merged>,
    /// What its job converted (removed unless the swapped history keeps it).
    files: ImportFiles,
}

/// Slot `id`'s overlay in `doc` (what a capture into a copy of the
/// history kept, plan 5e-1 Task 9b), to store with the same blob in the
/// real history.
fn kept_overlay(doc: &Document, id: PluginInstanceId) -> std::collections::BTreeMap<ParamId, f64> {
    doc.plugin_slot(id)
        .map(|s| s.params.iter().map(|(k, v)| (*k, *v)).collect())
        .unwrap_or_default()
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
        let plugin_auto = crate::automation::plugin_automation(&doc);
        let plugin_auto_doc = doc.clone();
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
            claim_owed: None,
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
            settings_seen: crate::audio::epoch(),
            save_after_pass: false,
            touches: Default::default(),
            static_edit: None,
            sampler: Sampler::default(),
            plugin_touches: PluginTouches::default(),
            plugin_auto_doc,
            plugin_auto,
            unsent_ends: UnsentEnds::default(),
            writing: None,
            pending_automation: Vec::new(),
            restart_writing: None,
            after_write_pending: None,
            last_step: None,
            restart_step: None,
            landed_in_group: false,
            surfaces: Default::default(),
            wind_hold: false,
            meters: Default::default(),
            bounce_copies: None,
            bounce_results: Default::default(),
            interchange_report: None,
            edits: Default::default(),
            edits_writing: 0,
            mpe_owed: Default::default(),
            last_warnings: Vec::new(),
            mix_owed: Vec::new(),
            #[cfg(feature = "test-hooks")]
            bounce_cancel_at: None,
            #[cfg(feature = "test-hooks")]
            bounce_fail_import_at: None,
            #[cfg(feature = "test-hooks")]
            for_daw_cancel_at: None,
            live_slots: Default::default(),
            #[cfg(feature = "test-hooks")]
            dawproject_cancel_at: None,
            #[cfg(feature = "test-hooks")]
            dawproject_free: None,
            #[cfg(feature = "test-hooks")]
            dawproject_capture_fails: false,
            held_merge: None,
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
        if !jobs_done && (self.jobs.busy() || self.edits_writing > 0) {
            return;
        }
        let dir = crate::retention::trash_dir(self.trash_dir.as_deref());
        let mut refs = self.history.referenced_media_anywhere();
        for p in self.recorded_keep() {
            refs.entry(p).or_insert(None);
        }
        // A held merge's media is the merged history's (plan 6e Task 8
        // review I1): kept until Merge or Cancel.
        for p in self.held_merge_files() {
            refs.entry(p).or_insert(None);
        }
        // Edit copies while the project is open (plan 5f-3 Ruling 3):
        // `abs` is absolute, retention's paths package-relative; a copy in
        // another package (after Save As) is not this package's.
        for e in self.edits.values() {
            if let Some(rel) = crate::external::package_relative(&e.abs, package) {
                refs.entry(rel).or_insert(None);
            }
        }
        let out = crate::retention::run(&refs, package, &|p| {
            crate::retention::trash(dir.as_deref(), p)
        });
        self.news.problems.extend(out.problems);
    }

    /// The package-relative files the held merge's job converted.
    fn held_merge_files(&self) -> Vec<String> {
        self.held_merge
            .iter()
            .flat_map(|h| h.files.iter())
            .filter_map(|(_, r)| r.as_ref().ok())
            .flatten()
            .map(|i| i.path.clone())
            .collect()
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

    /// The media a DAWproject export or check writes no audio for (offline
    /// or too long for a WAV), as of the last media refresh.
    fn dawproject_left_out(&self) -> core_io::dawproject::export::LeftOutAudio {
        let offline = self.offline.keys().copied().collect();
        crate::dawproject::left_out_audio(self.doc(), &offline)
    }

    /// `Busy` while a Format, Convert, Relink, Bounce or External Edit job
    /// has not been applied: each captures the project format and the
    /// items' files when it starts, so another such job may not start
    /// meanwhile (5a Task 11 review Minor 4; preflight M3; a bounce's
    /// streamer reads the media, plan 5f-2 Ruling 9; an external edit
    /// imports in the format it started with and relinks a region, plan
    /// 5f-3 Ruling 6; an export for another DAW renders as a bounce does,
    /// plan 6b Ruling 9; a DAWproject export reads the media, and its
    /// import converts into the project format, plan 6d). An Import Audio
    /// job does not block them.
    fn media_jobs_idle(&self) -> Result<(), VizError> {
        use crate::jobs::JobKindRow;
        if self.jobs.pending(&[
            JobKindRow::Format,
            JobKindRow::Convert,
            JobKindRow::Relink,
            JobKindRow::Bounce,
            JobKindRow::ExternalEdit,
            JobKindRow::ExportProject,
            JobKindRow::ExportDawproject,
            JobKindRow::ImportProject,
            JobKindRow::MergeDawproject,
        ]) {
            return Err(VizError::Busy {
                message: "Wait for the media job to finish.".into(),
            });
        }
        // A held merge's media is in `Media/` (plan 6e Ruling 12).
        if self.held_merge.is_some() {
            return Err(merge_held_busy());
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

    /// The plugin parameters touched or written in an open pass, or
    /// written by a pass not landed yet (plan 5e-1 Task 9b): a capture
    /// keeps their static values like an automated parameter's, since
    /// their lane may not exist yet (a mid-pass autosave).
    fn riding_params(&self) -> std::collections::BTreeSet<(PluginInstanceId, ParamId)> {
        let mut targets: Vec<AutomationTarget> = self
            .touches
            .iter()
            .filter(|(_, x)| matches!(x.path, crate::automation::Path::Write(..)))
            .map(|(t, _)| *t)
            .collect();
        if let Some(w) = &self.writing {
            targets.extend(w.stamps.keys().copied());
            targets.extend(w.moved.iter().copied());
            targets.extend(w.ended.iter().copied());
            targets.extend(w.holds.iter().map(|(t, _)| *t));
        }
        for p in &self.pending_automation {
            targets.extend(p.writes.iter().map(|w| w.target));
        }
        if let Some((Some(pass), _)) = &self.pending_pass {
            targets.extend(pass.automation.iter().map(|w| w.target));
        }
        targets
            .into_iter()
            .filter_map(|t| match t {
                AutomationTarget::PluginParam(slot, p) => Some((slot, p)),
                _ => None,
            })
            .collect()
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
        self.snapshot_with(Vec::new())
    }

    /// [`Inner::snapshot`] whose `edit_notes` are `notes` (the model's
    /// report lines of the edit that made it).
    fn snapshot_with(&mut self, notes: Vec<String>) -> Result<DocSnapshot, VizError> {
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
            edit_notes: notes,
        })
    }
}

/// One edit note: the event it is about (a settled crossfade), if any, and
/// the line.
type Note = (Option<core_model::ids::EventId>, String);

/// The report's warning lines as notes.
fn notes_of(r: core_model::command::Report) -> Vec<Note> {
    // Each settled entry is used once, so two same-named regions that both
    // lose their crossfade in one step keep their own ids.
    let mut settled = r.settled;
    r.warnings
        .into_iter()
        .map(|line| {
            let id = settled
                .iter()
                .position(|(_, l)| *l == line)
                .map(|i| settled.remove(i).0);
            (id, line)
        })
        .collect()
}

/// Adds `new` lines to `notes`, one per event: a later line about the same
/// event replaces the earlier one (the last step decides what is said).
/// Regions are told apart by id, never by name.
fn merge_notes(notes: &mut Vec<Note>, new: Vec<Note>) {
    for n in new {
        notes.retain(|o| match (o.0, n.0) {
            (Some(a), Some(b)) => a != b,
            _ => *o != n,
        });
        notes.push(n);
    }
}

fn note_lines(notes: Vec<Note>) -> Vec<String> {
    notes.into_iter().map(|(_, l)| l).collect()
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

/// A new project's document (plan tracks Ruling 3): `doc` with the
/// sequence "Sequence 1" and none of its tracks, made without a history
/// step, so there is nothing to undo and the project is not dirty.
fn starting_document(doc: Document) -> Result<Document, VizError> {
    let made = core_model::command::apply(
        &doc,
        &Command::Sequence(SequenceCmd::New {
            name: "Sequence 1".into(),
        }),
    )?;
    let lane_id = made.report.created.iter().find_map(|id| match id {
        core_model::ids::AnyId::Lane(l) => Some(*l),
        _ => None,
    });
    let seq = made.report.created.iter().find_map(|id| match id {
        core_model::ids::AnyId::Seq(s) => Some(*s),
        _ => None,
    });
    let (Some(seq), Some(lane)) = (seq, lane_id) else {
        return Err(crate::events::bad("the first sequence was not made"));
    };
    Ok(core_model::command::apply(&made.doc, &Command::Lane(LaneCmd::Remove { seq, lane }))?.doc)
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
        let riding = inner.riding_params();
        let history = &mut inner.history;
        let failed = plugins::with(self.id, |p| {
            p.map(|p| p.capture_all(history, &riding))
                .unwrap_or_default()
        })?;
        let all = failed.is_empty();
        for (id, e) in failed {
            let line = format!("{}: state not saved ({e})", slot_name(inner.doc(), id));
            inner.news.problems.push(line);
        }
        Ok(all)
    }

    /// Notes which slots run here now (`Inner::live_slots`).
    fn look_at_slots(&self, inner: &mut Inner) {
        let all = crate::dawproject::all_slots(inner.doc());
        let live = plugins::with(self.id, |p| {
            p.map(|p| {
                all.iter()
                    .copied()
                    .filter(|id| matches!(p.state(*id), Some(core_host::SlotState::Live)))
                    .collect::<std::collections::BTreeSet<PluginInstanceId>>()
            })
        });
        if let Ok(Some(live)) = live {
            inner.live_slots = live;
        }
    }

    /// Captures every running plug-in's state for a DAWproject export (plan
    /// 6d Task 9), as [`Session::capture_all`] does for a save; returns the
    /// slots it could not capture: `None` for those not running here (a
    /// placeholder, or still loading), else the capture's error. Their
    /// stored states are used. On a thread without the plug-in host, every
    /// slot not seen running at the last look is uncaptured.
    fn capture_for_export(
        &self,
        inner: &mut Inner,
    ) -> Result<crate::dawproject::Uncaptured, VizError> {
        let riding = inner.riding_params();
        let all = crate::dawproject::all_slots(inner.doc());
        let cached = inner.live_slots.clone();
        let history = &mut inner.history;
        let (failed, live) = plugins::with(self.id, |p| match p {
            Some(p) => {
                let failed = p.capture_all(history, &riding);
                let live: std::collections::BTreeSet<PluginInstanceId> = all
                    .iter()
                    .copied()
                    .filter(|id| matches!(p.state(*id), Some(core_host::SlotState::Live)))
                    .collect();
                (failed, Some(live))
            }
            None => (Vec::new(), None),
        })?;
        if let Some(l) = &live {
            inner.live_slots.clone_from(l);
        }
        let live = live.unwrap_or(cached);
        let mut out: crate::dawproject::Uncaptured = all
            .into_iter()
            .filter(|id| !live.contains(id))
            .map(|id| (id, None))
            .collect();
        for (id, e) in failed {
            out.insert(id, Some(e.to_string()));
        }
        #[cfg(feature = "test-hooks")]
        if inner.dawproject_capture_fails {
            for id in live {
                out.entry(id)
                    .or_insert_with(|| Some(crate::dawproject::CAPTURE_FAILS_HOOK.to_owned()));
            }
        }
        Ok(out)
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
    ///
    /// A document without a project ID is written with a fresh one, which
    /// every version of the history then takes once the file is written (a
    /// failed save leaves none behind, final review M6); not inside an open
    /// edit, where the next save stamps it (plan 6a Ruling 16, F23). Save
    /// As keeps it (Ruling T1-a).
    fn save_to(&self, inner: &mut Inner, path: &Path) -> Result<(), VizError> {
        let captured = self.capture_all(inner)?;
        // After the capture: a `Busy` capture leaves the document as it was.
        let stamp = (inner.doc().settings().project_id.is_none()
            && !inner.history.in_transaction())
        .then(crate::project_id::fresh);
        let _g = lock(&SAVE_LOCK);
        let mut refs = inner.history.referenced_media_anywhere();
        // Recordings kept for the recovery offer go with the project
        // (pre-flight I3): Save As removes an untitled scratch package.
        for p in inner.recorded_keep() {
            refs.entry(p).or_insert(None);
        }
        crate::media::copy_media(&refs, &inner.root(self.id), path)?;
        crate::overviews::copy_cache(&refs, &inner.root(self.id), path);
        match &stamp {
            Some(id) => file::save(&inner.doc().clone().with_project_id(id), path)?,
            None => file::save(inner.doc(), path)?,
        }
        // The DAWproject snapshots go with the project, pruned (plan 6d
        // Ruling 16): a failure is news, never a failed save.
        if let Err(e) = crate::dawproject::keep_snapshots(&inner.root(self.id), path) {
            inner
                .news
                .problems
                .push(format!("DAWproject merge records not kept: {e}"));
        }
        if let Some(id) = &stamp {
            inner.history.stamp_project_id(id)?;
        }
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
        self.sync_loaded_plugins(inner, &r.loaded);
        if !r.loaded.is_empty() || !r.failed.is_empty() || !r.restart.is_empty() {
            self.look_at_slots(inner);
        }
        // A plugin window's moves are gestures too (spec §7.1); the
        // parameters they write as automation are not parameter steps
        // (plan 5e-1 Ruling 15, pre-flight M13).
        let written = self.plugin_events(inner, &r.param_events);
        inner
            .held
            .extend(r.gestures.into_iter().filter_map(|mut g| {
                let slot = g.slot;
                g.values.retain(|(p, _)| !written.contains(&(slot, *p)));
                (!g.values.is_empty()).then_some(g)
            }));
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

    /// Add Lane `cmd` and the new lane's monitor `monitor`, one undo step
    /// "Add Lane" (plan 5d-1 Ruling 11).
    fn add_lane_monitored(
        &self,
        cmd: Command,
        seq: SeqId,
        monitor: core_model::lane::MonitorMode,
        filter: &ViewFilter,
    ) -> Result<DocSnapshot, VizError> {
        let mut inner = lock(&self.inner);
        let h = &mut inner.history;
        h.begin("Add Track")?;
        let made = h.execute(&cmd, filter).and_then(|r| {
            let lane = r
                .created
                .iter()
                .find_map(|id| match id {
                    core_model::ids::AnyId::Lane(l) => Some(*l),
                    _ => None,
                })
                .ok_or_else(|| core_model::error::ModelError::invalid("no track was added"))?;
            h.execute(
                &Command::Lane(LaneCmd::SetMonitor { seq, lane, monitor }),
                filter,
            )
        });
        if let Err(e) = made {
            let _ = h.abort();
            return Err(e.into());
        }
        h.commit()?;
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
        before_execute();
        let mut inner = lock(&self.inner);
        // Under the lock the command applies under (Ruling 18, Task 9
        // review M5).
        if crate::edit::retries_input_cmd(&cmd) {
            records_busy(&inner)?;
        }
        let before = inner.doc().clone();
        let report = inner.history.execute(&cmd, filter)?;
        // Inside an open edit the notes wait for its end (a drag's last
        // step decides what is said).
        if inner.edit_label.is_some() {
            inner.last_warnings = notes_of(report.clone());
        }
        inner.version += 1;
        // The user's own MPE setting wins over a controller's MCM still
        // waiting to be written back (plan 5g-2 Task 6 review m1), also
        // one still in the engine's ring (final review m2).
        if let Command::Lane(LaneCmd::SetMpe { lane, .. }) = &cmd {
            for (l, cfg) in host::take_mpe_reports(self.id) {
                inner.mpe_owed.insert(l, cfg);
            }
            inner.mpe_owed.remove(lane);
        }
        // An arm or input change that went through lets a failed input
        // open be tried again, in the engine this session holds (plan
        // 5d-1 Ruling 15; Task 7 review m6), before the reload applies it.
        if crate::edit::retries_input_cmd(&cmd) {
            host::clear_input_failure_of(self.id);
        }
        inner.reload_host(self.id);
        self.mark(&mut inner);
        // Touches of what the edit removed end (plan 5e-1 M9).
        self.release_vanished(&mut inner);
        self.resync_plugin_automation(&mut inner, Some(&before));
        self.send_mix_changes(&mut inner, &before);
        inner.snapshot_with(report.warnings)
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
        let (mut scratch, gone, riding) = {
            let inner = lock(&self.inner);
            let before = mixer::slot_ids(inner.doc());
            let after = mixer::slot_ids(next);
            let gone: Vec<_> = before.difference(&after).copied().collect();
            if gone.is_empty() {
                return Ok(());
            }
            (inner.history.clone(), gone, inner.riding_params())
        };
        let captured = plugins::with(self.id, |p| {
            let Some(p) = p else { return Vec::new() };
            gone.into_iter()
                .map(|id| {
                    let mine = core_model::automation::slot_params(&riding, id);
                    (id, p.capture_with(&mut scratch, id, &mine))
                })
                .collect::<Vec<_>>()
        })?;
        let mut inner = lock(&self.inner);
        for (id, captured) in captured {
            let blob = match captured {
                Ok(blob) => blob,
                // A placeholder has nothing to capture; its saved state
                // stays in the document.
                Err(core_host::HostError::NotFound(_)) => continue,
                // Reported like a save's capture failure (plan 6e Task 3
                // re-review: a refused store, e.g. a blob collision); the
                // document keeps the state it had.
                Err(e) => {
                    let line = format!("{}: state not saved ({e})", slot_name(inner.doc(), id));
                    inner.news.problems.push(line);
                    continue;
                }
            };
            if let Some(bytes) = scratch.doc().state_blob(blob) {
                // The overlay the capture kept (Task 9b), as in `scratch`.
                inner
                    .history
                    .capture_state_keeping(id, bytes, &kept_overlay(scratch.doc(), id))?;
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
        let (mut scratch, riding) = {
            let inner = lock(&self.inner);
            if inner.history.in_transaction() {
                return false;
            }
            (inner.history.clone(), inner.riding_params())
        };
        let Ok(unloaded) = plugins::with(self.id, |p| {
            p.map(|p| p.unload_unavailable(&mut scratch, &riding))
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
                    .capture_state_keeping(id, bytes, &kept_overlay(scratch.doc(), id))
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
        let before = inner.doc().clone();
        if undo {
            inner.history.undo()?;
        } else {
            inner.history.redo()?;
        }
        inner.version += 1;
        inner.reload_host(self.id);
        self.mark(&mut inner);
        self.release_vanished(&mut inner);
        self.resync_plugin_automation(&mut inner, Some(&before));
        self.send_mix_changes(&mut inner, &before);
        let snapshot = inner.snapshot()?;
        // An undone or redone arm, monitor or input edit claims as the
        // edit did (Task 8 review m6).
        let claim = crate::edit::claims_engine_step(&before, inner.doc());
        drop(inner);
        if let Some(seq) = claim {
            self.claim_for_input(seq);
        }
        Ok(snapshot)
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
        // A Play while a pass writes lands it first (plan 5e-1 Ruling 23):
        // the new position is a new pass.
        self.finish_writing(&mut inner);
        self.apply_pending_automation(&mut inner);
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
        // The gesture tap goes in before the Play, so the roll is its
        // first mark (plan 5e-1 Ruling 7); Write strips' controls are held
        // from the roll (Ruling 9), at the tick the Play starts from.
        let at = pending
            .located()
            .or_else(|| host::position(self.id).map(|p| p.1))
            .unwrap_or(Tick::ZERO);
        self.begin_writing(&mut inner, id, at);
        // The disk wait (up to 250 ms) holds no lock: this session's
        // transport poll and every other session go on meanwhile.
        drop(inner);
        let played = host::finish_play(pending);
        if played.is_err() {
            // A Play that did not start writes nothing (Ruling 24).
            let mut inner = lock(&self.inner);
            self.finish_writing(&mut inner);
            self.apply_pending_automation(&mut inner);
        }
        played
    }

    /// After an edit of a lane's arm, monitor or input in `seq` (plan 5d-1
    /// Rulings 16–17): claims an idle engine for `seq` when its lanes want
    /// it, so they are heard before the first Play, and reconnects the MIDI
    /// sources off this thread. Never claims while this session records; a failure
    /// is a fault, never the edit's.
    fn claim_for_input(&self, seq: SeqId) {
        if self.claim(seq) {
            host::listen_to_midi_sources_soon();
        }
    }

    /// [`Session::claim_for_input`]'s claim; whether `seq` wants live
    /// input (the MIDI sources then reconnect). A claim refused only
    /// because another window's idle engine is opening its input is owed:
    /// `idle` tries it again (Task 8 parked n2).
    fn claim(&self, seq: SeqId) -> bool {
        let mut inner = lock(&self.inner);
        inner.claim_owed = None;
        if inner.closed || !host::wants_live(inner.doc(), seq) {
            return false;
        }
        // During a pass nothing is claimed, but the sources reconnect all
        // the same (Ruling 17 as amended; final review m5). Arm and input
        // edits were refused before this point (Ruling 18).
        if inner.recording.is_some() || inner.finishing {
            return true;
        }
        match self.take_idle(&mut inner, seq) {
            Ok(_) => {}
            Err(VizError::Busy { message }) if message == host::INPUT_OPENING => {
                inner.claim_owed = Some(seq);
            }
            Err(e) => inner.reload_fault = Some(format!("the engine could not be claimed: {e}")),
        }
        true
    }

    /// Claims an idle engine for `seq` through [`host::claim_idle`], from
    /// this session's cursor when it belongs to `seq`; true when it was
    /// taken (the engine then holds the position, so the cursor clears).
    /// False when it is held already or busy elsewhere.
    fn take_idle(&self, inner: &mut Inner, seq: SeqId) -> Result<bool, VizError> {
        let at = match (inner.cursor, inner.cursor_seq) {
            (Some(t), None) => t,
            (Some(t), Some(s)) if s == seq => t,
            _ => Tick::ZERO,
        };
        let looped = inner.looped;
        inner.mark(self.id, true);
        let r = plugins::with(self.id, |p| {
            host::claim_idle(self.id, inner.doc(), seq, at, looped, p)
        })
        .and_then(|r| r);
        if r == Ok(true) {
            inner.cursor = None;
            inner.cursor_seq = Some(seq);
        }
        r
    }

    /// Before a MIDI strip send (plan 5g-1b final review I2): when this
    /// session does not hold the engine, claims the idle one for the
    /// shown sequence (`Inner::selected_seq`, as Locate uses), as an arm
    /// does, and only when that sequence plays one of `dests` (re-review
    /// N1, N2): an edit never loads a sequence that is not shown, nor takes
    /// another window's idle engine to send nothing. Nothing during a
    /// pass, after `close`, or when the engine is busy elsewhere; a failed
    /// claim is never the caller's error.
    fn claim_for_mix(&self, inner: &mut Inner, dests: &[DestId]) {
        if inner.closed
            || inner.recording.is_some()
            || inner.finishing
            || host::position(self.id).is_some()
        {
            return;
        }
        let Some(seq) = inner.selected_seq() else {
            return;
        };
        let plays = inner.doc().sequence(seq).is_ok_and(|s| {
            s.lanes
                .iter()
                .any(|l| l.midi_dest.is_some_and(|d| dests.contains(&d)))
        });
        if plays {
            let _ = self.take_idle(inner, seq);
        }
    }

    /// After an edit, undo or redo whose document went from `before` to
    /// the current one (plan 5g-1b final review I1): sends each external
    /// destination's volume (CC 7) and pan (CC 10) that is set and
    /// changed, after [`Session::claim_for_mix`]. An unset value sends
    /// nothing (Ruling 2); a destination the loaded sequence does not
    /// play is not reached (its values go out when it plays). A failed
    /// send leaves the edit; a full command ring delays a destination's
    /// batch whole to the next `idle` (final review m4).
    ///
    /// An external destination whose MPE output changed (plan 5g-2 Task 6
    /// review m4): turned off, or moved to the other zone, the old zone
    /// is turned off (an MCM with no members on its manager channel);
    /// turned on or changed, its configuration goes out, then its set
    /// volume and pan on the new manager channel.
    fn send_mix_changes(&self, inner: &mut Inner, before: &Document) {
        use core_engine::midi1::Midi1Msg;
        // Per destination, its messages on the channels they name (an MPE
        // destination's mix on its manager).
        let mut due: Vec<(DestId, Vec<Midi1Msg>)> = Vec::new();
        for (id, d) in inner.doc().instruments() {
            let DestKind::MidiEndpoint { channel, .. } = &d.kind else {
                continue;
            };
            if !d.kind.is_external() {
                continue;
            }
            let mix_channel = d.mpe.map_or(channel & 0x0F, |m| m.manager());
            let mut msgs = Vec::new();
            let old = before.instruments().get(id);
            let (was_mpe, mpe) = (old.and_then(|o| o.mpe), d.mpe);
            let mix_was = old.map(|o| o.midi_mix);
            let mix = d.midi_mix;
            let mpe_changed = was_mpe != mpe;
            if mpe_changed
                && let Some(o) = was_mpe
                && mpe.is_none_or(|n| n.manager() != o.manager())
            {
                let off = core_model::mpe::MpeConfig { members: 0, ..o };
                // The MCM alone: the member bend range is left as is.
                msgs.extend(core_engine::mpe::configuration(&off).into_iter().take(5));
            }
            let on = mpe.filter(|_| mpe_changed);
            if let Some(n) = on {
                msgs.extend(core_engine::mpe::configuration(&n));
            }
            for (cc, v, was) in [
                (7, mix.volume, mix_was.and_then(|m| m.volume)),
                (10, mix.pan, mix_was.and_then(|m| m.pan)),
            ] {
                if let Some(v) = v.filter(|v| on.is_some() || Some(*v) != was) {
                    msgs.push(Midi1Msg::three(0xB0 | mix_channel, cc, v));
                }
            }
            if !msgs.is_empty() {
                due.push((*id, msgs));
            }
        }
        if due.is_empty() {
            return;
        }
        let dests: Vec<DestId> = due.iter().map(|(d, _)| *d).collect();
        self.claim_for_mix(inner, &dests);
        inner.mix_owed.extend(due);
        Self::send_owed_mix(self.id, inner);
    }

    /// Sends the destinations' owed batches from
    /// [`Session::send_mix_changes`], oldest first, each whole or not at
    /// all (final review m4): one the engine's full command ring refuses
    /// waits, with every later one, for the next `idle`. A destination the
    /// engine does not play (or an engine this session no longer holds)
    /// drops its batch, as before.
    fn send_owed_mix(id: u64, inner: &mut Inner) {
        while let Some((dest, msgs)) = inner.mix_owed.first() {
            if host::send_all_now_on(id, *dest, msgs).is_err() {
                return;
            }
            inner.mix_owed.remove(0);
        }
    }

    /// `Busy` when one of `cmds` changes which lanes record (an arm or an
    /// input) while this session records, finishes a pass or waits to
    /// apply one (plan 5d-1 Ruling 18, 5c carry M6); monitor changes pass.
    /// This early check refuses before any validation or gesture rewind;
    /// [`records_busy`] repeats it under the lock the command applies
    /// under, so a pass another thread starts meanwhile is seen (Task 9
    /// review M5).
    fn check_records(&self, cmds: &[crate::edit::EditCommand]) -> Result<(), VizError> {
        if !cmds.iter().any(crate::edit::retries_input) {
            return Ok(());
        }
        records_busy(&lock(&self.inner))
    }

    /// Why Measure or Track Synth cannot start or take its step now:
    /// `Busy` while this session records or has an edit open. Playback
    /// alone is not checked here: Track Synth stays allowed while playing,
    /// and Measure checks the transport itself.
    fn check_idle(inner: &Inner) -> Result<(), VizError> {
        if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
            return Err(recording_busy());
        }
        if inner.edit_label.is_some() || inner.history.in_transaction() {
            return Err(VizError::Busy {
                message: "Finish the open edit first.".into(),
            });
        }
        Ok(())
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
        ffi_guard(|| Session::with(starting_document(Document::new())?, None, false))
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
            Session::with(starting_document(doc)?, None, false)
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
            self.save_to(&mut inner, &path)?;
            // The take lands at Stop: it is saved then too (plan 5d-1
            // Ruling 19, 5c carry M5). Only a Save that worked asks.
            if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
                inner.save_after_pass = true;
            }
            Ok(())
        })
    }

    /// Whether a Save made while recording saves again once the pass
    /// lands (plan 5d-1 Ruling 19; the app says so).
    pub fn saves_after_pass(&self) -> Result<bool, VizError> {
        ffi_guard(|| Ok(lock(&self.inner).save_after_pass))
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
            // A held merge's media is under the current root (plan 6e
            // Task 8 review I1).
            if inner.held_merge.is_some() {
                return Err(VizError::Busy {
                    message: "Merge or cancel the merge waiting to be confirmed, then save as."
                        .into(),
                });
            }
            // A copy written without the lock lands under the old root
            // (plan 5f-3 review M1).
            if inner.edits_writing > 0 {
                return Err(VizError::Busy {
                    message: "Wait for the edit copy to be written, then save.".into(),
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
            // The same package spelled another way (case, a symlinked
            // parent, a trailing `/`): its edit copies are respelled under
            // `p`, so the save's retention, which strips `p`, keeps them
            // (plan 5f-3 review I2). Same files, so a failed save leaves
            // them right.
            if let Some(old) = inner.path.clone() {
                if package_key(&old) == claim.key {
                    for e in inner.edits.values_mut() {
                        if let Ok(rel) = e.abs.strip_prefix(&old) {
                            e.abs = p.join(rel);
                        }
                    }
                }
            }
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
                // Its edit copies go to the Trash first, never unlinked
                // (plan 5f-3 Ruling 3, owner answer 2026-11-03): Save As
                // does not carry them, so their next apply says they are
                // gone. So do its recordings the project did not carry
                // (ruling F-1; spec §6.5): those it carried were copied
                // into the new package. A titled session has no scratch
                // package.
                let scratch = crate::media::scratch_root(self.id);
                let trash = crate::retention::trash_dir(inner.trash_dir.as_deref());
                let mut carried: std::collections::BTreeSet<String> = inner
                    .history
                    .referenced_media_anywhere()
                    .into_keys()
                    .collect();
                carried.extend(inner.recorded_keep());
                let recorded = crate::record::RECORDED_DIR;
                let left = crate::external::trash_files_in(
                    &scratch.join(crate::external::EDITS_DIR),
                    trash.as_deref(),
                    &crate::external::partial,
                ) + crate::external::trash_files_in(
                    &scratch.join(recorded),
                    trash.as_deref(),
                    &|name| carried.contains(&format!("{recorded}/{name}")),
                );
                if left == 0 {
                    let _ = std::fs::remove_dir_all(&scratch);
                } else {
                    inner.news.problems.push(format!(
                        "{left} recordings or edit copies were left in {}.",
                        scratch.display()
                    ));
                }
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
                    "there is no track after the armed one"
                } else {
                    "there is no track before the armed one"
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
            let mut inner = lock(&self.inner);
            inner.restart_writing = None;
            let stopped = host::stop(self.id);
            // What was written lands now (spec §7.4).
            self.finish_writing(&mut inner);
            self.apply_pending_automation(&mut inner);
            stopped
        })
    }

    /// Moves the position to `tick`. The next bar step or wind starts
    /// from here, not from the last one's target (plan 7d controller
    /// ruling I6): the locate is remembered as their base, so a step at
    /// once does not wait for the engine's position to catch up.
    pub fn locate(&self, tick: i64) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            self.locate_in(&mut inner, tick)?;
            inner.surfaces.wind = Some((crate::surfaces::now(), tick));
            Ok(())
        })
    }

    /// A click on the transport bar's rewind (`forward` false) or fast
    /// forward, their menu items or the channel's command (plan 7d Ruling
    /// 3): one bar, as a ruler click locates (an open automation pass
    /// lands). Starts from the last step's or wind's target when that was
    /// under 250 ms ago. While playing, back from within a beat after a
    /// barline goes to the bar before (`bar_step_from`, review I-1). Busy while recording; a `Model` error with no
    /// sequence. Returns the new tick.
    pub fn transport_step_bar(&self, forward: bool) -> Result<i64, VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            let now = crate::surfaces::now();
            let to = self.bar_target(&inner, forward, now)?;
            self.locate_in(&mut inner, to)?;
            inner.surfaces.wind = Some((now, to));
            Ok(to)
        })
    }

    /// A rewind or fast forward key-down, first or auto-repeat (plan 7d
    /// Ruling 4): one bar, as [`Session::transport_step_bar`] steps, but
    /// moved as a wind is, landing nothing; the key-up's
    /// [`Session::transport_hold_end`] lands once. Returns the new tick.
    pub fn transport_key_step(&self, forward: bool) -> Result<i64, VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            let now = crate::surfaces::now();
            let to = self.bar_target(&inner, forward, now)?;
            self.wind_move(&mut inner, to, now)?;
            Ok(to)
        })
    }

    /// One tick of a held rewind or fast forward (plan 7d Ruling 3):
    /// `quarters` quarter notes on (negative: back) from the last target
    /// (under 250 ms ago) or the position, in `0..=2⁵⁰`. Moves the
    /// position only: an open automation pass goes on writing, and
    /// [`Session::transport_hold_end`] lands it. Non-finite `quarters`
    /// are a `Model` error, as is having no sequence; Busy while
    /// recording. Returns the new tick.
    pub fn transport_wind(&self, quarters: f64) -> Result<i64, VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            let now = crate::surfaces::now();
            let (_, base) = self.jump_base(&inner, now)?;
            let to = core_editor::transport::wind_target(base, quarters).ok_or_else(|| {
                VizError::Model {
                    message: "quarters must be finite".into(),
                }
            })?;
            self.wind_move(&mut inner, to, now)?;
            Ok(to)
        })
    }

    /// The end of a hold (a release, a cancel or a key-up; plan 7d
    /// controller ruling I4): after a wind or a held key's step while an
    /// automation pass is open, it lands the pass once and begins a new
    /// one where the transport is, as one ruler click does, but without
    /// a second locate: a key tap while playing is one locate (review
    /// I1). Otherwise it does nothing. Returns the tick.
    pub fn transport_hold_end(&self) -> Result<i64, VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            let now = crate::surfaces::now();
            let tick = match inner.surfaces.wind {
                Some((at, t)) if now.saturating_duration_since(at) < WIND_RECENT => t,
                _ => self
                    .step_where(&inner)
                    .map_or(inner.cursor.unwrap_or(Tick::ZERO), |(_, t)| t)
                    .0
                    .clamp(0, core_editor::transport::WIND_LIMIT),
            };
            // The hold is cleared before the landing on purpose: a refused
            // landing (Busy while recording) must not leave it owed, so
            // `wind_hold` never stays set (fold check M-7).
            if std::mem::take(&mut inner.wind_hold) && inner.writing.is_some() {
                // A pass is open whenever the transport plays, so the
                // landing does not locate again (review I1): the new pass
                // begins where the transport is.
                let at = host::transport_position(self.id).map_or(tick, |(_, t)| t.0);
                self.land_at(&mut inner, at, false)?;
            }
            Ok(tick)
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
                    counting_in: h.counting_in,
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
                        counting_in: false,
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
            // A bounce's plugin copies load here (plan 5f-2 Ruling 6).
            plugins::tick_bounces();
            self.unload_disabled();
            self.service_recording()?;
            self.service_writing();
            // Control surfaces and MIDI learn (plan 5f-1 Ruling 11).
            self.service_surfaces();
            // An arm's claim another window's opening input put off (Task
            // 8 parked n2); the sources were reconnected at the arm.
            let owed = lock(&self.inner).claim_owed;
            if let Some(seq) = owed {
                self.claim(seq);
            }
            let mut inner = lock(&self.inner);
            self.apply_pending_pass(&mut inner);
            self.apply_pending_automation(&mut inner);
            self.apply_mpe_reports(&mut inner);
            Self::send_owed_mix(self.id, &mut inner);
            // A save at Stop the busy registry put off (Task 9 review M4).
            self.save_after_pass_now(&mut inner);
            // A setting a load publishes changed (plan 5d-1 Ruling 12).
            let epoch = crate::audio::epoch();
            if inner.settings_seen != epoch {
                inner.settings_seen = epoch;
                inner.reload_owed |= host::holder() == Some(self.id);
            }
            if inner.reload_owed {
                inner.reload_host(self.id);
            }
            let changed = self.pump(&mut inner);
            // Every other path that changes a plugin parameter's
            // automation (Task 9 review I-1).
            self.resync_plugin_automation(&mut inner, None);
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

    /// Sends a value to the running plugin now, without an undo step: a
    /// non-gesture edit (a typed value; tests). Kept for those (spec §7.1);
    /// the app's sliders go through `gesture` instead. `commit_params`
    /// records the final value.
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
    /// (`SetPluginParams`, spec §4): the step of a non-gesture edit made
    /// with `set_param` (spec §7.1; gestures land their own step).
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
        // The surfaces read the same peaks (plan 5f-1 Ruling 13): one take
        // feeds both holds, each cleared only by its own reader.
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            self.take_meters(&mut inner);
            Ok(inner
                .meters
                .take_mixer(Instant::now())
                .unwrap_or_else(MixerLevels::empty))
        })
    }

    /// Opens a continuous edit (a drag): commands until `end_edit` are one
    /// undo step named `label`, and each is heard at once.
    pub fn begin_edit(&self, label: String) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.history.begin(label.clone())?;
            inner.edit_label = Some(label);
            inner.last_warnings.clear();
            Ok(())
        })
    }

    /// Closes the edit `begin_edit` opened (nothing happens when none is
    /// open).
    pub fn end_edit(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            // No edit open: nothing was said since the last one closed.
            if inner.edit_label.is_none() {
                inner.last_warnings.clear();
            }
            // The static gestures' shared edit is theirs to close (plan
            // 5e-1 Ruling 14).
            if inner.history.in_transaction() && inner.static_edit.is_none() {
                inner.history.commit()?;
                inner.version += 1;
            }
            inner.edit_label = None;
            self.commit_held(&mut inner);
            self.apply_pending_automation(&mut inner);
            self.apply_jobs_after_edit(&mut inner);
            let notes = note_lines(std::mem::take(&mut inner.last_warnings));
            inner.snapshot_with(notes)
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

    /// Loads `plugin` into a strip's EQ slot (replacing any EQ there; its
    /// state is captured first, so Undo restores it), or empties the slot
    /// (`None`; refused when it is empty). Plan 7b Ruling 7.
    pub fn set_eq(
        &self,
        strip: u64,
        plugin: Option<PluginRefRow>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetEq {
                strip: StripId(strip),
                plugin: plugin.as_ref().map(Into::into),
            }))
        })
    }

    /// Sets a strip's own colour, or clears it (`None`: the kind's
    /// colour). Controller ruling C-1.
    pub fn set_strip_colour(
        &self,
        strip: u64,
        colour: Option<crate::mixer::SwatchRow>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Mixer(MixerCmd::SetColour {
                strip: StripId(strip),
                colour: colour.map(Into::into),
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
                    midi_mix: MidiMixRow {
                        volume: d.midi_mix.volume,
                        pan: d.midi_mix.pan,
                        mute: d.midi_mix.mute,
                    },
                    channel: match &d.kind {
                        DestKind::MidiEndpoint { channel, .. } => Some(*channel),
                        DestKind::PluginInstrument { .. } => None,
                    },
                    mpe: d.mpe.map(Into::into),
                })
                .collect())
        })
    }

    /// "New MIDI Destination…" (plan 5d-1 Ruling 25): a destination named
    /// `name` playing CoreMIDI destination `port` on `channel` (0–15), one
    /// undo step "Add Destination". `Model` for an empty or over-long name
    /// or port (1–255 characters, no control characters), a port
    /// [`crate::host::midi_destination_ports`] does not list (matched as
    /// given, untrimmed) or a reserved reference, or a channel above 15.
    /// The same name, port and channel again make another destination.
    pub fn add_midi_destination(
        &self,
        name: String,
        port: String,
        channel: u8,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let name = name.trim().to_string();
            // The port is matched as given: a CoreMIDI name may have
            // edge spaces (Task 11 review m5).
            for (what, v) in [("name", &name), ("port", &port)] {
                if v.trim().is_empty() || v.chars().count() > 255 || v.chars().any(char::is_control)
                {
                    return Err(crate::events::bad(&format!(
                        "the destination's {what} is 1 to 255 characters"
                    )));
                }
            }
            if channel > 15 {
                return Err(crate::events::bad(
                    "the MIDI channel is 0 to 15 (MIDI channels 1 to 16)",
                ));
            }
            // A device named like a reserved reference would play the
            // built-in synth or the virtual output (Task 11 review m5).
            if port == core_model::mixer::BUILTIN_SYNTH_REF
                || port == core_engine::schedule::targets::VIRTUAL_OUT_REF
            {
                return Err(crate::events::bad(&format!(
                    "{port:?} is reserved; choose a MIDI destination"
                )));
            }
            if !crate::host::midi_destination_ports().contains(&port) {
                return Err(crate::events::bad(&format!(
                    "no MIDI destination named {port:?}"
                )));
            }
            self.execute(Command::Routing(RoutingCmd::AddDestination {
                name,
                kind: DestKind::MidiEndpoint {
                    endpoint_ref: port,
                    group: 0,
                    channel,
                },
                default_patch: None,
            }))
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
        // "‹strip› — EQ ‹plugin›" (plan 7b Ruling 7).
        Some(SlotOwner::Eq(strip)) => {
            return match doc.mixer().strip(strip) {
                Ok(s) => format!("{} — EQ {plugin}", s.name),
                Err(_) => format!("EQ {plugin}"),
            };
        }
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

    /// Slot `slot`'s own editor reports `value` (plain) for `param` with
    /// no gesture events, as a knob that sends values only (test hook):
    /// the plugin path's value-only touch, released 500 ms after its last
    /// value.
    pub fn debug_plugin_value(&self, slot: u64, param: u64, value: f64) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let e = core_host::ParamEvent::Value(core_model::mixer::ParamId(param), value);
            let _ = self.plugin_events(&mut inner, &[(PluginInstanceId(slot), e)]);
            Ok(())
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

    /// The next bounce cancels itself once its progress reaches `at` (None: never).
    pub fn debug_bounce_cancel_at(&self, at: Option<f32>) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).bounce_cancel_at = at;
            Ok(())
        })
    }

    /// The next bounce fails the "Add to project" import of source `at`
    /// (0-based, in import order) as a disk error would (None: never).
    pub fn debug_bounce_fail_import(&self, at: Option<u32>) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).bounce_fail_import_at = at;
            Ok(())
        })
    }

    /// The next export for another DAW cancels itself after `stage` (1:
    /// its first MIDI file is written; 2: its About file is written, before
    /// the rename; None: never; plan 6b F6).
    pub fn debug_for_daw_cancel_at(&self, stage: Option<u32>) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).for_daw_cancel_at = stage;
            Ok(())
        })
    }

    /// The next DAWproject export or import cancels itself after `stage`
    /// (export: 1 its files are made; 2 its first WAV is staged; 3
    /// `project.xml` is in the partial; 4 the partial is synced, before the
    /// rename; import: 5 the file is opened; 6 its first entry is
    /// extracted; 7 its first audio file is converted; 8 it is mapped;
    /// merge: 5–8 as the import, 9 it is planned, 10 it is applied, 11
    /// inside the apply, at its first check; None: never).
    pub fn debug_dawproject_cancel_at(&self, stage: Option<u32>) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).dawproject_cancel_at = stage;
            Ok(())
        })
    }

    /// Parameter `param` of slot `slot` as the running plug-in reports it
    /// now (test hook; L4). `Model` while the slot is not running.
    pub fn debug_plugin_param(&self, slot: u64, param: u64) -> Result<f64, VizError> {
        ffi_guard(|| {
            plugins::with(self.id, |p| {
                p.and_then(|p| p.instance(PluginInstanceId(slot)))
                    .and_then(|i| i.param_value(core_model::mixer::ParamId(param)))
            })?
            .ok_or_else(|| VizError::Model {
                message: format!("slot {slot} is not running, or has no parameter {param}"),
            })
        })
    }

    /// DAWproject exports and imports assume `bytes` free at their
    /// destination (None: the disk's). It stays set until changed.
    pub fn debug_free_bytes(&self, bytes: Option<u64>) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).dawproject_free = bytes;
            Ok(())
        })
    }

    /// DAWproject exports and checks treat every running plug-in's capture
    /// as failed (`true`; the error is
    /// [`CAPTURE_FAILS_HOOK`](crate::dawproject::CAPTURE_FAILS_HOOK)), or
    /// capture as usual (`false`). It stays set until changed.
    pub fn debug_dawproject_capture_fails(&self, on: bool) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).dawproject_capture_fails = on;
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
            // A save at Stop the busy registry put off is made now: no
            // `idle` follows a close, and the take was promised saved
            // (Task 9 parked m3; a failure is printed below).
            self.save_after_pass_now(&mut inner);
            // Touches end and a pass in progress lands (plan 5e-1 Ruling 20).
            self.release_touches(&mut inner);
            // Its learn is cancelled; if it was focused, the surfaces go
            // blank (plan 5f-1 Ruling 11).
            crate::surfaces::session_closed(self.id);
            self.finish_writing(&mut inner);
            self.apply_pending_automation(&mut inner);
            for line in inner.news.problems.iter().skip(before) {
                eprintln!("VizDAW: {line}");
            }
            inner.closed = true;
            // The engine is let go now, not at the drop: a measurement
            // running on another thread ends with the window (Task 9
            // review M4).
            host::release(self.id);
            let root = inner.root(self.id);
            let keep = inner.kept();
            for w in inner.jobs.shutdown() {
                discard(&root, w, &keep);
            }
            // A held merge and its media go (plan 6e Task 8).
            drop_held_merge(&mut inner, self.id);
            // The bounce's plugin copies go with it (pre-flight I2).
            if let Some((_, token)) = inner.bounce_copies.take() {
                plugins::end_bounce(token);
            }
            inner.overviews.shutdown();
            // No job or overview writer is left (review I2): the empty
            // shared folders the imports and merges made go now.
            crate::dawproject::remove_empty_media_folders(&root);
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
                // Watching ends with the project: its copies may go (plan
                // 5f-3 Ruling 3).
                inner.edits.clear();
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
                // Edit copies too (plan 5f-3 Ruling 3, owner answer
                // 2026-11-03): an editor may hold work VizDAW has not
                // imported yet.
                // Every recording, dot-named or not; copies being written
                // (dot-named partials) go with the package (ruling T3-1).
                // A copy still being written keeps the package (review M1).
                let mut left = inner.edits_writing as usize;
                let none = |_: &str| false;
                for (d, skip) in [
                    (crate::record::RECORDED_DIR, &none as &dyn Fn(&str) -> bool),
                    (crate::external::EDITS_DIR, &crate::external::partial),
                ] {
                    left += crate::external::trash_files_in(&scratch.join(d), dir.as_deref(), skip);
                }
                if left == 0 {
                    let _ = std::fs::remove_dir_all(scratch);
                } else {
                    // Never unlinked (pre-flight I2): the package stays.
                    eprintln!(
                        "VizDAW: {left} recording(s) or edit copies were left in {}",
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

    /// How many loop passes of the pass being written have landed (plan
    /// 5e-1 Task 12), or `None` when none is being written (test hook).
    pub fn debug_loop_passes_landed(&self) -> Option<usize> {
        lock(&self.inner).writing.as_ref().map(|w| w.landed)
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

    /// The document as `project.json` text (test hook, plan 6a).
    pub fn debug_document_json(&self) -> String {
        core_model::file::to_json(lock(&self.inner).doc()).unwrap_or_default()
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
    /// What runs on this thread inside an edit's command, between its
    /// checks and the locked apply (test hook, Task 9 review M5).
    static BEFORE_EXECUTE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Runs `f` the next time an edit on this thread is about to apply its
/// command under the session lock; `None` clears it (test hook).
#[cfg(feature = "test-hooks")]
pub fn set_before_execute_hook(f: Option<Box<dyn FnOnce()>>) {
    BEFORE_EXECUTE.with(|h| *h.borrow_mut() = f);
}

#[cfg(feature = "test-hooks")]
fn before_execute() {
    if let Some(f) = BEFORE_EXECUTE.with(|h| h.borrow_mut().take()) {
        f();
    }
}

#[cfg(not(feature = "test-hooks"))]
fn before_execute() {}

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
    /// lanes from commands that act on the selection. `Busy` while an
    /// edit `begin_edit` opened is in progress (a drag, a fader): joining it
    /// would merge the command into that undo step, or lose it on abort
    /// (plan 5d-1 Ruling 20). `Busy` [`RECORDING_LANES`] for an arm or
    /// input change while recording (Ruling 18).
    pub fn edit(
        &self,
        cmd: crate::edit::EditCommand,
        filter: Option<crate::edit::ViewFilterRow>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            if lock(&self.inner).edit_label.is_some() {
                return Err(edit_open_busy());
            }
            self.check_records(std::slice::from_ref(&cmd))?;
            let filter = crate::edit::view_filter(filter.as_ref());
            if matches!(cmd, crate::edit::EditCommand::ModifyAgain) {
                return self.modify_again_with(&filter);
            }
            if let crate::edit::EditCommand::Settings { edit } = &cmd {
                crate::modify::check_settings_edit(lock(&self.inner).doc(), edit)?;
            }
            let c = crate::edit::to_command(&cmd)?;
            // Add Lane gives the lane the default monitor mode (plan 5d-1
            // Ruling 11).
            let monitor = crate::audio::default_monitor();
            if let (
                crate::edit::EditCommand::Lane {
                    edit: crate::edit::LaneEdit::Add { seq, .. },
                },
                false,
            ) = (&cmd, monitor == core_model::lane::MonitorMode::Auto)
            {
                return self.add_lane_monitored(c, SeqId(*seq), monitor, &filter);
            }
            let snapshot = self.execute_with(c, &filter)?;
            if let Some(seq) = crate::edit::claims_engine(&cmd) {
                self.claim_for_input(SeqId(seq));
            }
            Ok(snapshot)
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
                return Err(edit_open_busy());
            }
            self.check_records(&cmds)?;
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
                inner.last_warnings.clear();
            }
            // A panic inside a command must not leave the transaction open:
            // `ffi_guard` alone would report it but keep `edit_label` set.
            let mut notes = Vec::new();
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for (step, c) in core.into_iter().enumerate() {
                    edit_all_hook(&label, step);
                    self.execute_with(c, &filter)?;
                    merge_notes(
                        &mut notes,
                        std::mem::take(&mut lock(&self.inner).last_warnings),
                    );
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
                inner.last_warnings.clear();
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
            let snapshot = inner.snapshot_with(note_lines(notes))?;
            drop(inner);
            if let Some(seq) = cmds.iter().find_map(crate::edit::claims_engine) {
                self.claim_for_input(SeqId(seq));
            }
            Ok(snapshot)
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
            // Before the rewind: a refused step keeps the gesture's last
            // one (plan 5d-1 Ruling 18).
            self.check_records(std::slice::from_ref(&cmd))?;
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
                // A refused step leaves the rewound document: nothing from
                // the step before is said about it.
                inner.last_warnings.clear();
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
            if inner.history.in_transaction() && inner.static_edit.is_none() {
                inner.history.abort()?;
                inner.version += 1;
                inner.reload_host(self.id);
            }
            inner.edit_label = None;
            inner.last_warnings.clear();
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
                    return Err(edit_open_busy());
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
                return Err(edit_open_busy());
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

    /// A new track of `kind` in sequence `seq`, below track `after` (an
    /// unknown or absent `after`: at the end), as one undo step named "New
    /// Audio Track", "New Instrument Track" or "New MIDI Track" (plan
    /// tracks Ruling 4). It may add the built-in synth destination or the
    /// first audio output path in the same step. `Model` for an unknown
    /// sequence; `Busy` while an edit is open or recording.
    pub fn new_track(
        &self,
        seq: u64,
        kind: crate::tracks::TrackKindRow,
        after: Option<u64>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let monitor = crate::audio::default_monitor();
            let mut inner = lock(&self.inner);
            if inner.edit_label.is_some() || inner.history.in_transaction() {
                return Err(edit_open_busy());
            }
            records_busy(&inner)?;
            let seq = crate::snapshot::seq_id(inner.doc(), seq)?;
            let index = crate::tracks::index_below(inner.doc(), seq, after);
            inner.history.begin(crate::tracks::new_track_label(kind))?;
            match crate::tracks::build_track(&mut inner.history, seq, kind, index, monitor) {
                Ok(_) => inner.history.commit()?,
                Err(e) => {
                    inner.history.abort()?;
                    return Err(e);
                }
            }
            inner.version += 1;
            host::clear_input_failure_of(self.id);
            inner.reload_host(self.id);
            self.mark(&mut inner);
            inner.snapshot()
        })
    }

    /// Duplicates track `lane` of sequence `seq` just below it (plan
    /// tracks Ruling 4), as one undo step "Duplicate Track". `Model` for an
    /// unknown id or a Track Synth return track; `Busy` while an edit is
    /// open or recording.
    pub fn duplicate_track(&self, seq: u64, lane: u64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if inner.edit_label.is_some() || inner.history.in_transaction() {
                return Err(edit_open_busy());
            }
            records_busy(&inner)?;
            inner.history.begin("Duplicate Track")?;
            let r = inner.history.execute(
                &Command::Lane(LaneCmd::Duplicate {
                    seq: SeqId(seq),
                    lane: LaneId(lane),
                }),
                &ViewFilter::default(),
            );
            match r {
                Ok(_) => inner.history.commit()?,
                Err(e) => {
                    inner.history.abort()?;
                    return Err(e.into());
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
        // A bounce still running stops first (plan 5f-2 pre-flight I2):
        // its engine holds its plugin copies' processors, which end below;
        // one without copies must not write after its session went (final
        // review T4-M5). So does an export for another DAW, which renders
        // through one (plan 6b F2).
        {
            let mut inner = lock(&self.inner);
            if inner.bounce_copies.is_some()
                || inner.jobs.pending(&[
                    crate::jobs::JobKindRow::Bounce,
                    crate::jobs::JobKindRow::ExportProject,
                    crate::jobs::JobKindRow::ExportDawproject,
                    crate::jobs::JobKindRow::ImportProject,
                    crate::jobs::JobKindRow::MergeDawproject,
                ])
            {
                let root = inner.root(self.id);
                let keep = inner.kept();
                for w in inner.jobs.shutdown() {
                    discard(&root, w, &keep);
                }
            }
            // A held merge's media goes with it (plan 6e Task 8).
            drop_held_merge(&mut inner, self.id);
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
        // Only the media it imported; the files it exported stay.
        Work::Bounce { files, .. } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .flatten()
            .map(|i| i.path)
            .collect(),
        // The stems it added (plan 6b F1); a folder in place stays.
        Work::ExportForDaw { files, .. } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .flatten()
            .map(|i| i.path)
            .collect(),
        // What a DAWproject import or merge converted (plans 6d, 6e).
        Work::ImportProject { files, .. } | Work::MergeDawproject { files, .. } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .flatten()
            .map(|i| i.path)
            .collect(),
        // What an external edit imported; its copy stays (plan 5f-3).
        Work::ExternalEdit { file, .. } => file.into_iter().flatten().map(|i| i.path).collect(),
        // An import wrote nothing; an export's files are in place and its
        // partials already gone (plan 6a).
        Work::ImportMidi { .. } | Work::ExportMidi { .. } => Vec::new(),
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
                Work::Bounce {
                    files,
                    target,
                    exported,
                    problems,
                    failed,
                } => {
                    // Exported files are already in place (Ruling 12): kept
                    // whatever the import's apply does, and named even when
                    // a later rename failed.
                    let applied = if let Some(why) = failed {
                        Err((VizError::Io { message: why }, Vec::new()))
                    } else if files.is_empty() {
                        Ok((Vec::new(), Vec::new()))
                    } else {
                        apply_import_as(inner, &root, files, target, &ImportAs::BOUNCE)
                    };
                    match applied {
                        Ok((created, more)) => {
                            changed |= !created.is_empty();
                            inner.bounce_results.insert(
                                id,
                                crate::bounce::BounceResultRow {
                                    files: exported,
                                    media: created.clone(),
                                },
                            );
                            let all = problems.into_iter().chain(more).collect();
                            row(id, kind, JobStateRow::Done, None, all, created)
                        }
                        Err((e, more)) => {
                            inner.bounce_results.insert(
                                id,
                                crate::bounce::BounceResultRow {
                                    files: exported,
                                    media: Vec::new(),
                                },
                            );
                            let all = problems.into_iter().chain(more).collect();
                            row(
                                id,
                                kind,
                                JobStateRow::Failed,
                                Some(e.to_string()),
                                all,
                                vec![],
                            )
                        }
                    }
                }
                Work::ExternalEdit { event, from, file } => {
                    let r = apply_external_edit_work(inner, &root, id, event, from, file);
                    changed |= r.state == JobStateRow::Done;
                    r
                }
                Work::Import { files, target } => {
                    match apply_import_as(inner, &root, files, target, &ImportAs::IMPORT) {
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
                    }
                }
                Work::ImportMidi { result, place } => {
                    let applied = result
                        .map_err(|message| VizError::Model { message })
                        .and_then(|imp| apply_midi_import(inner, &imp, place));
                    match applied {
                        Ok(report) => {
                            changed = true;
                            let report = crate::interchange::report_row(&report, "Imported");
                            inner.interchange_report = Some(report.clone());
                            let mut r = row(id, kind, JobStateRow::Done, None, vec![], vec![]);
                            r.report = Some(report);
                            r
                        }
                        Err(e) => row(
                            id,
                            kind,
                            JobStateRow::Failed,
                            Some(e.to_string()),
                            vec![],
                            vec![],
                        ),
                    }
                }
                Work::ImportProject { files, result } => {
                    let r = apply_dawproject_work(inner, &root, id, kind, files, result);
                    changed |= r.state == JobStateRow::Done;
                    r
                }
                Work::MergeDawproject { files, result } => {
                    let (r, swapped) = apply_merge_work(inner, &root, id, kind, files, result);
                    changed |= swapped;
                    r
                }
                Work::ExportMidi { result, written } => {
                    let mut r = match result {
                        Ok(report) => {
                            let report = crate::interchange::report_row(&report, "Exported");
                            inner.interchange_report = Some(report.clone());
                            let mut r = row(id, kind, JobStateRow::Done, None, vec![], vec![]);
                            r.report = Some(report);
                            r
                        }
                        Err(why) => row(id, kind, JobStateRow::Failed, Some(why), vec![], vec![]),
                    };
                    // A failed job still names what it put in place.
                    r.written = written;
                    r
                }
                Work::ExportForDaw {
                    files,
                    target,
                    result,
                    written,
                } => {
                    let r = apply_for_daw(inner, &root, id, kind, files, target, result, written);
                    changed |= !r.created.is_empty();
                    r
                }
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
        // A bounce's plugin copies end with its job, however it ended:
        // applied, failed, cancelled or panicked (plan 5f-2 Ruling 6,
        // pre-flight I2); so do an export's for another DAW (plan 6b).
        if inner.bounce_copies.is_some()
            && !inner.jobs.pending(&[
                crate::jobs::JobKindRow::Bounce,
                crate::jobs::JobKindRow::ExportProject,
            ])
        {
            if let Some((_, token)) = inner.bounce_copies.take() {
                plugins::end_bounce(token);
            }
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

/// Applies a parsed MIDI file (plan 6a): into the project as one undo step
/// "Import MIDI File", or as a new project through a scratch history that
/// then replaces the session's, leaving nothing to undo (Ruling 6). Into a
/// sequence without events the file's tempo and meter are used whatever
/// `use_tempo` says. On an error (or a panic) nothing changes.
///
/// A new-project import replaces the whole history: edits made to the
/// empty project while the job ran (song info, tempo) stay in the document
/// but lose their undo steps (Ruling 6 accepts this; the app shows the
/// project only once the import is applied).
fn apply_midi_import(
    inner: &mut Inner,
    imp: &core_io::midi_in::MidiImport,
    mut place: core_io::midi_in::Placement,
) -> Result<core_io::report::Report, VizError> {
    const LABEL: &str = "Import MIDI File";
    let s = inner.doc().sequence(place.seq)?;
    place.use_tempo |= s
        .lanes
        .iter()
        .all(|l| l.takes.iter().all(|t| t.events.is_empty()));
    let mut report = core_io::report::Report::default();
    let run = |h: &mut History, report: &mut core_io::report::Report| -> Result<(), VizError> {
        h.begin(LABEL)?;
        let applied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            core_io::midi_in::apply(h, imp, &place, report)
        }));
        let failed = match applied {
            Ok(Ok(_)) => return h.commit().map_err(VizError::from),
            Ok(Err(e)) => VizError::from(e),
            Err(p) => VizError::Internal {
                message: crate::guard::panic_message(p.as_ref()),
            },
        };
        let _ = h.abort();
        Err(failed)
    };
    if place.new_project {
        let mut scratch = History::new(inner.doc().clone());
        run(&mut scratch, &mut report)?;
        inner.history = History::new(scratch.doc().clone());
        // Serials of the replaced history's steps name nothing now (final
        // review M8): a later step must not try to amend into them.
        inner.last_step = None;
        inner.surfaces.tempo_step = None;
    } else {
        run(&mut inner.history, &mut report)?;
    }
    Ok(report)
}

/// Applies a finished DAWproject import (plan 6d Task 10): as a new
/// project, through a scratch history that then replaces the session's,
/// leaving nothing to undo (spec §6.1). On `Err` (and a cancel) the media
/// it converted is removed, with the overviews built for it, and the row is
/// Failed (Cancelled for a cancel).
fn apply_dawproject_work(
    inner: &mut Inner,
    root: &Path,
    id: u64,
    kind: crate::jobs::JobKindRow,
    files: ImportFiles,
    result: Result<Box<core_io::dawproject::import::DpImport>, String>,
) -> crate::jobs::JobRow {
    use crate::jobs::{JobStateRow, row};
    let applied = result
        .map_err(|message| VizError::Model { message })
        .and_then(|imp| apply_dawproject_import(inner, &imp));
    match applied {
        Ok(report) => {
            let report = crate::interchange::report_row(&report, "Imported");
            inner.interchange_report = Some(report.clone());
            let mut r = row(id, kind, JobStateRow::Done, None, vec![], vec![]);
            r.report = Some(report);
            r
        }
        Err(e) => {
            remove_job_media(inner, root, files);
            match e {
                VizError::Model { message } if message == crate::for_daw::CANCELLED => {
                    row(id, kind, JobStateRow::Cancelled, None, vec![], vec![])
                }
                e => row(
                    id,
                    kind,
                    JobStateRow::Failed,
                    Some(match e {
                        VizError::Model { message } => message,
                        e => e.to_string(),
                    }),
                    vec![],
                    vec![],
                ),
            }
        }
    }
}

/// Removes the media a DAWproject import or merge job converted that no
/// history state references, with the overviews built for audio no state
/// holds.
fn remove_job_media(inner: &Inner, root: &Path, files: ImportFiles) {
    let keep = inner.kept();
    let hashes: std::collections::BTreeSet<String> =
        inner.named_media().into_values().flatten().collect();
    for f in files.into_iter().filter_map(|(_, r)| r.ok()).flatten() {
        remove_unkept(root, &f.path, &keep);
        if !hashes.contains(&f.content_hash)
            && let Some(at) = core_media::overview::Overview::path(root, &f.content_hash)
        {
            let _ = std::fs::remove_file(at);
        }
    }
    // Only files: the shared folders (`Media/.incoming`,
    // `Media/.overviews`) other jobs and the overview worker write into
    // stay until close (review I2).
}

/// `Busy` while a merge waits between its preview and Merge.
fn merge_held_busy() -> VizError {
    VizError::Busy {
        message: "Finish or cancel the merge waiting to be confirmed.".into(),
    }
}

/// Drops the held merge, if any, and the media its job converted.
fn drop_held_merge(inner: &mut Inner, id: u64) {
    if let Some(h) = inner.held_merge.take() {
        let root = inner.root(id);
        remove_job_media(inner, &root, h.files);
    }
}

/// The swap (plan 6e Ruling 12): when the project is still what the merge
/// was made from (its present serial, its present document's referenced
/// plug-in states and its project ID), the merged history replaces the
/// session's and the report is the interchange report; the media nothing
/// references then goes. Otherwise `Model` [`crate::dawproject::CHANGED_SINCE`]
/// and the merge's media goes. The caller reloads the host on `Ok`.
fn swap_merge(
    inner: &mut Inner,
    root: &Path,
    merged: Box<crate::dawproject::Merged>,
    files: ImportFiles,
) -> Result<crate::interchange::ReportRow, VizError> {
    let mut merged = merged;
    let now = crate::dawproject::MergeBase::of(&inner.history);
    let history = merged.history.take().filter(|_| now == merged.base);
    let Some(history) = history else {
        remove_job_media(inner, root, files);
        return Err(VizError::Model {
            message: crate::dawproject::CHANGED_SINCE.into(),
        });
    };
    inner.history = history;
    // Serials of the replaced history's later steps name nothing now.
    inner.last_step = None;
    inner.surfaces.tempo_step = None;
    let report = crate::dawproject::merged_report(&merged);
    inner.interchange_report = Some(report.clone());
    remove_job_media(inner, root, files);
    Ok(report)
}

/// Applies a finished DAWproject merge job (plan 6e Task 8): Preview is
/// Done with its preview and report, its media removed; Hold is Done with
/// its preview and kept as the held merge (a held one before it is
/// dropped); Apply swaps ([`swap_merge`]). A failure or cancel removes its
/// media. Answers the row and whether the history was swapped.
fn apply_merge_work(
    inner: &mut Inner,
    root: &Path,
    id: u64,
    kind: crate::jobs::JobKindRow,
    files: ImportFiles,
    result: Result<Box<crate::dawproject::Merged>, String>,
) -> (crate::jobs::JobRow, bool) {
    use crate::dawproject::MergeStepRow;
    use crate::jobs::{JobStateRow, row};
    let merged = match result {
        Ok(m) => m,
        Err(why) => {
            remove_job_media(inner, root, files);
            let r = if why == crate::for_daw::CANCELLED {
                row(id, kind, JobStateRow::Cancelled, None, vec![], vec![])
            } else {
                row(id, kind, JobStateRow::Failed, Some(why), vec![], vec![])
            };
            return (r, false);
        }
    };
    let mut r = row(id, kind, JobStateRow::Done, None, vec![], vec![]);
    r.merge = Some(merged.preview.clone());
    match merged.step {
        MergeStepRow::Preview => {
            remove_job_media(inner, root, files);
            r.report = Some(crate::dawproject::merged_report(&merged));
            (r, false)
        }
        MergeStepRow::Hold => {
            if let Some(old) = inner.held_merge.take() {
                remove_job_media(inner, root, old.files);
            }
            inner.held_merge = Some(HeldMerge {
                job: id,
                merged,
                files,
            });
            (r, false)
        }
        // Never while recording, as `confirm_merge` (review M1).
        MergeStepRow::Apply
            if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() =>
        {
            remove_job_media(inner, root, files);
            let r = row(
                id,
                kind,
                JobStateRow::Failed,
                Some(crate::dawproject::CHANGED_SINCE.into()),
                vec![],
                vec![],
            );
            (r, false)
        }
        MergeStepRow::Apply => match swap_merge(inner, root, merged, files) {
            Ok(report) => {
                r.report = Some(report);
                (r, true)
            }
            Err(e) => {
                let mut f = row(id, kind, JobStateRow::Failed, None, vec![], vec![]);
                f.message = Some(match e {
                    VizError::Model { message } => message,
                    e => e.to_string(),
                });
                (f, false)
            }
        },
    }
}

/// Applies a mapped DAWproject file as a new project (Ruling 17), as
/// `apply_midi_import`'s new-project path does: a scratch history, one
/// transaction "Import DAWproject" (a panic or error aborts it), the new
/// plug-ins' states captured, then the session's history replaced. The
/// project must have no tracks. On an error nothing changes.
fn apply_dawproject_import(
    inner: &mut Inner,
    imp: &core_io::dawproject::import::DpImport,
) -> Result<core_io::report::Report, VizError> {
    const LABEL: &str = "Import DAWproject";
    let doc = inner.doc();
    if doc.sequences().values().any(|s| !s.lanes.is_empty()) {
        return Err(VizError::Model {
            message: core_io::dawproject::import::NOT_NEW.into(),
        });
    }
    let seq = doc
        .sequences()
        .keys()
        .next()
        .copied()
        .ok_or_else(|| VizError::Model {
            message: "the project has no sequence".into(),
        })?;
    let mut report = core_io::report::Report::default();
    let mut scratch = History::new(doc.clone());
    scratch.begin(LABEL)?;
    let applied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        core_io::dawproject::import::apply(&mut scratch, imp, seq, &mut report)
    }));
    let states = match applied {
        Ok(Ok(states)) => {
            scratch.commit()?;
            states
        }
        Ok(Err(e)) => {
            let _ = scratch.abort();
            return Err(e.into());
        }
        Err(p) => {
            let _ = scratch.abort();
            return Err(VizError::Internal {
                message: crate::guard::panic_message(p.as_ref()),
            });
        }
    };
    for (slot, bytes) in &states {
        scratch.capture_state(*slot, bytes)?;
    }
    inner.history = History::new(scratch.doc().clone());
    // Serials of the replaced history's steps name nothing now.
    inner.last_step = None;
    inner.surfaces.tempo_step = None;
    Ok(report)
}

/// One import job's result per source file: `(file name, the items it
/// became or why it failed)`.
type ImportFiles = Vec<(String, Result<Vec<core_media::import::Imported>, String>)>;

/// What an applied import made (media IDs, per-file problems), or why
/// nothing was applied (with the per-file problems).
type ImportApplied = Result<(Vec<u64>, Vec<String>), (VizError, Vec<String>)>;

/// How an import job's result is applied: Import Audio, or a bounce's
/// "Add to project" (plan 5f-2 Ruling 9).
struct ImportAs {
    /// The undo step's label.
    label: &'static str,
    /// The items' origin.
    origin: fn(&core_media::import::Imported) -> core_model::document::MediaOrigin,
    /// New lanes are named after their item (else "Audio <n>").
    named_lanes: bool,
    /// The tick may be as early as the target sequence's pickup start
    /// (else a negative tick lands at 0).
    pickup: bool,
}

impl ImportAs {
    /// Import Audio: `Imported` items on "Audio <n>" lanes.
    const IMPORT: ImportAs = ImportAs {
        label: "Import Audio",
        origin: |item| core_model::document::MediaOrigin::Imported {
            original_name: item.original_name.clone(),
        },
        named_lanes: false,
        pickup: false,
    };

    /// A bounce's "Add to project": `Bounced` items on lanes named after them.
    const BOUNCE: ImportAs = ImportAs {
        label: "Bounce",
        origin: |_| core_model::document::MediaOrigin::Bounced,
        named_lanes: true,
        pickup: false,
    };

    /// An export's "Add the stems to this project" (plan 6b): `Bounced`
    /// items on tracks named after them, from the pickup's start.
    const FOR_DAW: ImportAs = ImportAs {
        label: "Add Stems",
        origin: |_| core_model::document::MediaOrigin::Bounced,
        named_lanes: true,
        pickup: true,
    };

    /// The name of the new lane for `item`, the sequence's `n`th.
    fn lane_name(&self, item: &core_media::import::Imported, n: usize) -> String {
        if self.named_lanes {
            item.name.clone()
        } else {
            format!("Audio {n}")
        }
    }
}

/// One finished import (Ruling 19): the media items and, with a target,
/// their regions (into the given lane for a single mono/stereo file, else
/// one new lane per item), as one undo step labelled as `how` says.
/// On an error nothing is applied and the written files are removed.
fn apply_import_as(
    inner: &mut Inner,
    root: &Path,
    files: ImportFiles,
    target: Option<crate::jobs::ImportTargetRow>,
    how: &ImportAs,
) -> ImportApplied {
    use core_model::command::{AudioCmd, MediaCmd};
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
    if let Err(e) = h.begin(how.label) {
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
                    origin: Some((how.origin)(item)),
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
                            name: how.lane_name(item, n),
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
                            message: "no track created".into(),
                        })?
                }
            };
            let earliest = if how.pickup {
                0i64.saturating_sub(h.doc().sequence(seq)?.start_offset.0)
            } else {
                0
            };
            h.execute(
                &Command::Audio(AudioCmd::Insert {
                    seq,
                    lane,
                    tick: Tick(t.tick.max(earliest)),
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

/// Applies a finished export for another DAW (plan 6b Ruling 7, F1): on
/// `Ok` its folder is in place, kept whatever the "Add" apply does, and
/// the stems are added as one undo step "Add Stems"; otherwise the
/// imports are removed and the row is Failed, or Cancelled for a cancel.
#[allow(clippy::too_many_arguments)]
fn apply_for_daw(
    inner: &mut Inner,
    root: &Path,
    id: u64,
    kind: crate::jobs::JobKindRow,
    files: ImportFiles,
    target: Option<crate::jobs::ImportTargetRow>,
    result: Result<core_io::report::Report, String>,
    written: Vec<String>,
) -> crate::jobs::JobRow {
    use crate::jobs::{JobStateRow, row};
    let report = match result {
        Ok(r) => crate::interchange::report_row(&r, "Exported"),
        Err(why) => {
            let keep = inner.kept();
            for f in files.into_iter().filter_map(|(_, r)| r.ok()).flatten() {
                remove_unkept(root, &f.path, &keep);
            }
            let mut r = if why == crate::for_daw::CANCELLED {
                row(id, kind, JobStateRow::Cancelled, None, vec![], vec![])
            } else {
                row(id, kind, JobStateRow::Failed, Some(why), vec![], vec![])
            };
            r.written = written;
            return r;
        }
    };
    inner.interchange_report = Some(report.clone());
    let applied = if files.is_empty() {
        Ok((Vec::new(), Vec::new()))
    } else {
        apply_import_as(inner, root, files, target, &ImportAs::FOR_DAW)
    };
    let mut r = match applied {
        Ok((created, problems)) => row(id, kind, JobStateRow::Done, None, problems, created),
        Err((e, problems)) => row(
            id,
            kind,
            JobStateRow::Failed,
            Some(e.to_string()),
            problems,
            vec![],
        ),
    };
    r.report = Some(report);
    r.written = written;
    r
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
            Ok(report) => {
                // A new rate or length can shorten or drop a crossfade.
                problems.extend(report.warnings);
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
        Ok(report) => {
            // The rescale may shorten or drop a crossfade: the user hears of
            // it in the job's problems (plan 6c pre-flight F1).
            problems.extend(report.warnings);
            Ok((
                files
                    .iter()
                    .filter(|(_, r)| r.is_ok())
                    .map(|(m, _)| m.0)
                    .collect(),
                problems,
            ))
        }
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

/// Why an edit waits for the edit a drag or a fader opened (plan 5d-1
/// Ruling 20: `Busy`, as Track Synth and Measure answer).
fn edit_open_busy() -> VizError {
    VizError::Busy {
        message: "Finish the open edit first.".into(),
    }
}

/// Why an arm or input change waits for Stop (plan 5d-1 Ruling 18).
pub const RECORDING_LANES: &str = "Stop recording to change which tracks record.";

/// `Busy` [`RECORDING_LANES`] while `inner` records, finishes a pass or
/// waits to apply one.
fn records_busy(inner: &Inner) -> Result<(), VizError> {
    if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
        return Err(VizError::Busy {
            message: RECORDING_LANES.into(),
        });
    }
    Ok(())
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
                    return Err(edit_open_busy());
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
                return Err(edit_open_busy());
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
    /// input does not have, or the spec is out of range; `Busy` while
    /// recording or finishing a pass,
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

    /// `columns` columns of the take lane `lane` is recording, over file
    /// frames `[from_file_frame, to_file_frame)` (plan 5d-1 Ruling 21;
    /// spec §6.2: the region draws as it grows); place them with
    /// `RecordStateRow.pass_start_tick` and `pass_start_file_frame`.
    /// `Model` when nothing records here, the lane records no audio, or
    /// the arguments are out of range.
    pub fn recording_waveform(
        &self,
        lane: u64,
        from_file_frame: u64,
        to_file_frame: u64,
        columns: u32,
    ) -> Result<crate::overviews::OverviewRows, VizError> {
        ffi_guard(|| {
            crate::overviews::check_columns(columns)?;
            if from_file_frame >= to_file_frame {
                return Err(crate::events::bad(
                    "from_file_frame must be before to_file_frame",
                ));
            }
            let inner = lock(&self.inner);
            let r = inner
                .recording
                .as_ref()
                .ok_or_else(|| crate::events::bad("nothing is recording"))?;
            r.waveform(LaneId(lane), from_file_frame, to_file_frame, columns)
                .ok_or_else(|| {
                    crate::events::bad(&format!("track {lane} records no audio in this pass"))
                })
        })
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
                    pass_start_tick: None,
                    pass_start_file_frame: 0,
                    range_start_tick: 0,
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
                    message: "no track created".into(),
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
        // A Record while a pass writes lands it first (plan 5e-1 Ruling
        // 23), before the recording is planned on the document.
        self.finish_writing(&mut inner);
        self.apply_pending_automation(&mut inner);
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
            return Err(crate::events::bad(
                "arm a track that has an input to record",
            ));
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
        // Count-in (plan 5d-1 Ruling 7): one click per beat of the meter at
        // the roll point, at its tempo, before the transport rolls.
        let count_in = (spec.count_in_bars > 0).then(|| {
            let meter = s.meter.entry_at(play_from).meter;
            let per_bar = u32::from(meter.num.max(1));
            let beat = frame(play_from.0.saturating_add(meter.beat_ticks()))
                .saturating_sub(frame(play_from.0));
            core_engine::count_in::CountIn {
                beats: per_bar * u32::from(spec.count_in_bars),
                per_bar,
                beat_frames: beat as f64,
            }
        });
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
        let mut pending = match played {
            Ok(p) => p,
            Err(e) => {
                inner.reload_owed = true;
                return Err(e);
            }
        };
        pending.count_in(count_in);
        let fail = |e: VizError| {
            let _ = host::stop_recording(self.id);
            Err(e)
        };
        // The input opens on its own thread (plan 5d-1 Ruling 13): a pass
        // that records audio waits for it, a bounded while.
        if !audio.is_empty() && !host::wait_input(host::INPUT_WAIT) {
            return fail(VizError::Busy {
                message: host::INPUT_OPENING.into(),
            });
        }
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
            tempo: tempo.clone(),
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
        inner.landed_in_group = false;
        inner.recording = Some(recording);
        inner.record_faults.clear();
        inner.looped = spec.looped;
        inner.reload_owed = false;
        inner.reload_fault = None;
        inner.cursor = None;
        inner.cursor_seq = Some(id);
        let at = pending
            .located()
            .or_else(|| host::position(self.id).map(|p| p.1))
            .unwrap_or(Tick::ZERO);
        self.begin_writing(&mut inner, id, at);
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
        // Automation written while it recorded joins the pass (spec §7.4).
        let pass = self.writes_into_pass(&mut inner, pass);
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
        // A recording that recorded nothing but whose loop passes landed
        // automation into its group (Task 12) is "Write Automation".
        let label = match &pass {
            Some(p) => p.name(),
            None if inner.landed_in_group => "Write Automation",
            None => "Record",
        };
        inner.landed_in_group = false;
        let mut created: Vec<core_model::ids::AnyId> = Vec::new();
        // The automation that landed, if any (plan 5e-1 Ruling 25).
        let mut landed: Vec<AutomationWrite> = Vec::new();
        if let Some(p) = pass {
            // Track Synth's "Mute the MIDI lane after recording" (spec
            // §6.7): inside the group, so the mute is part of "Record"
            // (plan 5c-2 Ruling 6).
            let mutes = crate::synth::mutes_after(&p, &inner.synth_mutes);
            let automation = p.automation.clone();
            match inner
                .history
                .execute(&Command::Record(p), &ViewFilter::default())
            {
                Err(e) => {
                    inner.news.problems.push(format!(
                        "The recording could not be added ({e}). Its files are kept."
                    ));
                    inner.keep_recordings.extend(files);
                    // Its automation still lands, inside the group (plan
                    // 5e-1 Ruling 25).
                    if let Some(r) = self.land_automation_alone(inner, automation.clone()) {
                        created = r.created;
                        landed = automation;
                    }
                }
                Ok(r) => {
                    created = r.created;
                    landed = automation;
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
                                "The MIDI track could not be muted after recording ({e})."
                            ));
                        }
                    }
                }
            }
        }
        // Write strips become the After Write mode in the same step (plan
        // 5e-1 Ruling 9).
        self.after_write_in_group(inner, !landed.is_empty());
        let _ = inner.history.end_group(label);
        let silent = crate::automation::silent_new_lanes(inner.doc(), &created, &landed);
        inner.news.problems.extend(silent);
        inner.version += 1;
        inner.news.doc_changed = true;
        inner.news.media_changed = true;
        inner.reload_host(self.id);
        self.mark(inner);
        if !landed.is_empty() {
            self.sync_written_plugins(inner, &landed);
            self.resync_plugin_automation(inner, None);
        }
        self.release_vanished(inner);
        self.save_after_pass_now(inner);
    }

    /// Saved while recording: the take is saved once its pass has landed
    /// (Ruling 19). A plugin registry that is busy (a nested run loop)
    /// keeps the mark for the next `idle` (Task 9 review M4); any other
    /// failure is said and the mark goes (the document stays unsaved).
    fn save_after_pass_now(&self, inner: &mut Inner) {
        if !inner.save_after_pass
            || inner.recording.is_some()
            || inner.finishing
            || inner.pending_pass.is_some()
        {
            return;
        }
        let Some(path) = inner.path.clone() else {
            inner.save_after_pass = false;
            return;
        };
        match self.save_to(inner, &path) {
            Err(VizError::Busy { .. }) if plugins::is_busy() => {}
            Err(e) => {
                inner.save_after_pass = false;
                inner
                    .news
                    .problems
                    .push(format!("The take could not be saved: {e}."));
            }
            Ok(()) => inner.save_after_pass = false,
        }
    }

    /// `idle`'s recording step: ends the recording when the input went
    /// away, the disk failed or filled, or the transport stopped by itself.
    fn service_recording(&self) -> Result<(), VizError> {
        let ending = {
            let mut inner = lock(&self.inner);
            let id = self.id;
            let stopped = || host::transport_stopped(id).unwrap_or(false);
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
    /// are out of range; `Busy` while recording (Ruling 14) and while an
    /// edit is open.
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
            // Not while recording, nor with an edit open: its restart
            // would drop the step (Task 8 review M1). Both are `Busy`, as
            // for Measure (Task 10 review I1).
            Self::check_idle(&inner)?;
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
            let snapshot = inner.snapshot()?;
            drop(inner);
            // Track Synth arms the return lane (plan 5d-1 Ruling 16).
            self.claim_for_input(id);
            Ok(snapshot)
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
    /// session plays or records, and while another measurement runs, and
    /// when an edit was opened while it ran (the message names the value
    /// measured, which is not set); `Engine` when the measurement stopped
    /// before it finished (the device went away, its sequence was
    /// deleted or its window closed).
    pub fn measure_return(&self, dest: u64) -> Result<crate::synth::MeasureRow, VizError> {
        self.transport_call(|| {
            let dest = DestId(dest);
            // One lock from the checks to the start (Task 9 review M3):
            // the engine loads the document the checks saw.
            let (seq, status) = {
                let mut inner = lock(&self.inner);
                Self::check_idle(&inner)?;
                let d = crate::synth::external_dest(inner.doc(), dest)?;
                let Some((seq, _, name, channels)) = crate::synth::return_lane(inner.doc(), dest)
                else {
                    return Err(crate::events::bad(&format!(
                        "no track records the return of {}: set one up with Track Synth first",
                        d.name
                    )));
                };
                inner.mark(self.id, true);
                let status = plugins::with(self.id, |p| {
                    host::start_measure(self.id, inner.doc(), seq, dest, &name, channels, p)
                })
                .and_then(|r| r)?;
                (seq, status)
            };
            // The notes take about 2.6 s; nothing here holds a lock. A
            // test may hold the measurement past its end (pre-flight M5).
            let until = Instant::now() + crate::synth::measure_wait();
            while (!status.done() && !status.closed() && Instant::now() < until)
                || host::measure_held()
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            // One lock from the end to the step (Task 9 review I1, as
            // Track Synth): an edit opened meanwhile never absorbs it, and
            // the input follows the current document (review I2).
            let mut inner = lock(&self.inner);
            let ours = host::end_measure(self.id, inner.doc(), seq, &status)?;
            host::after_measure_end();
            // Measured off the plugin thread (the app's task): the engine
            // has no inserts for this document, so the timer's next step
            // claims it with the plugin host (5d-2 Task 8 re-review-2 R1).
            if host::factory_owner() != Some(self.id) && host::wants_live(inner.doc(), seq) {
                inner.claim_owed = Some(seq);
            }
            // Still ours means nothing else removed it; `closed()` is not
            // read here, as our own removal sets it (Task 9 re-review). A
            // renderer gone with its device leaves it unfinished below.
            if !ours {
                return Err(VizError::Engine {
                    message: "the measurement stopped before it finished: the audio device \
                              went away, its sequence was deleted or its window closed"
                        .into(),
                });
            }
            if !status.done() {
                return Err(VizError::Engine {
                    message: "the measurement did not finish: it timed out, or the audio device \
                              stopped"
                        .into(),
                });
            }
            let m = crate::synth::measured_status(&status).map_err(|e| crate::events::bad(&e))?;
            if let Err(e) = Self::check_idle(&inner) {
                let message = match e {
                    VizError::Busy { message } => message,
                    other => return Err(other),
                };
                let message = message.trim_end_matches('.');
                return Err(VizError::Busy {
                    message: format!("{message} (measured {:.1} ms).", m.ms),
                });
            }
            // Setting a latency removes no plugin: nothing is captured.
            let cmd = Command::Routing(RoutingCmd::SetReturnLatency { dest, ms: m.ms });
            inner.history.execute(&cmd, &ViewFilter::default())?;
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            let snapshot = inner.snapshot()?;
            Ok(crate::synth::MeasureRow {
                return_latency_ms: m.ms,
                onsets: m.onsets,
                snapshot,
            })
        })
    }

    /// Sets external destination `dest`'s MIDI strip mix (plan 5g-1b), one
    /// undo step "Set MIDI Mix". The edit sends the volume (CC 7) and pan
    /// (CC 10) that are set and changed, claiming an idle engine first
    /// (`Session::send_mix_changes`); a failed send leaves the edit.
    /// `Model` for another destination or a value above 127.
    pub fn set_midi_mix(&self, dest: u64, mix: MidiMixRow) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let mix = core_model::document::MidiMix {
                volume: mix.volume,
                pan: mix.pan,
                mute: mix.mute,
            };
            self.execute(Command::Routing(RoutingCmd::SetMidiMix {
                dest: DestId(dest),
                mix,
            }))
        })
    }

    /// Sends the volume and pan given to `dest` now without an edit (a
    /// fader or pan drag), claiming an idle engine first as an edit does.
    /// True when something was sent; false when there was nothing to send,
    /// the engine is busy elsewhere or the loaded sequence does not play
    /// `dest`. `Model` for a destination that is not external or a value
    /// above 127.
    pub fn preview_midi_mix(
        &self,
        dest: u64,
        volume: Option<u8>,
        pan: Option<u8>,
    ) -> Result<bool, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let dest = DestId(dest);
            let external = inner
                .doc()
                .instruments()
                .get(&dest)
                .is_some_and(|d| d.kind.is_external());
            if !external {
                return Err(crate::events::bad("only a MIDI destination has a mix"));
            }
            for (what, v) in [("midi_volume", volume), ("midi_pan", pan)] {
                if v.is_some_and(|v| v > 127) {
                    return Err(crate::events::bad(&format!("{what} is 0 to 127")));
                }
            }
            if volume.is_none() && pan.is_none() {
                return Ok(false);
            }
            self.claim_for_mix(&mut inner, &[dest]);
            let mut sent = false;
            for (cc, v) in [(7, volume), (10, pan)] {
                if let Some(v) = v {
                    sent |= host::send_now(
                        self.id,
                        inner.doc(),
                        dest,
                        core_engine::midi1::Midi1Msg::three(0xB0, cc, v),
                    )?;
                }
            }
            Ok(sent)
        })
    }

    /// MPE output for destination `dest` (plan 5g-2 Ruling 7), one undo
    /// step "Set Destination MPE"; `None` turns it off. `Model` for an
    /// unknown destination or a setting out of range (members 1–15, bend
    /// range 1–96).
    pub fn set_destination_mpe(
        &self,
        dest: u64,
        mpe: Option<crate::mpe::MpeRow>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Routing(RoutingCmd::SetMpe {
                dest: DestId(dest),
                mpe: mpe.map(Into::into),
            }))
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

/// Automation recording (plan 5e-1; sub-project 5 spec §7).
#[uniffi::export]
impl Session {
    /// A gesture on `target` from the app's mouse (spec §7.1; Rulings 14,
    /// 16, 20, 26): [`Session::gesture_from`] with the mouse's hold.
    pub fn gesture(
        &self,
        target: AutomationTargetRow,
        phase: GesturePhaseRow,
    ) -> Result<(), VizError> {
        self.gesture_from(target, phase, crate::automation::GestureHolderRow::Mouse)
    }

    /// A gesture on `target` from `holder` (spec §7.1; Rulings 14, 16, 20,
    /// 26; plan 5f-1 Ruling 2): the target stays touched until each of
    /// its holders released it. A Touch, or a value, from a hand that does
    /// not hold the target joins it; a release from one does nothing.
    /// `Model` for a target that cannot be
    /// gestured or a non-finite value; `Busy` for a static touch's first
    /// value while an edit the gesture path did not open is open.
    pub fn gesture_from(
        &self,
        target: AutomationTargetRow,
        phase: GesturePhaseRow,
        holder: crate::automation::GestureHolderRow,
    ) -> Result<(), VizError> {
        ffi_guard(|| {
            let t = automation_target_of(target);
            let phase = match phase {
                GesturePhaseRow::Touch { value } => core_surface::Phase::Touch(value),
                GesturePhaseRow::Value { value } => core_surface::Phase::Value(value),
                GesturePhaseRow::Release => core_surface::Phase::Release,
            };
            let mut inner = lock(&self.inner);
            self.gesture_held(&mut inner, t, phase, TouchHolder::from(holder), None)
        })
    }

    /// Sets the mode of `strips` (spec §4.6) as one undo step "Set
    /// Automation Mode"; `Busy` while a static touch's step is open.
    pub fn set_automation_mode(
        &self,
        strips: Vec<u64>,
        mode: AutomationModeRow,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            // Not inside an open static touch's step (Task 10 review M2).
            if lock(&self.inner).static_edit.is_some() {
                return Err(VizError::Busy {
                    message: "Release the touched control first.".into(),
                });
            }
            let snap = self.execute(Command::Mixer(MixerCmd::SetAutomationMode {
                strips: strips.into_iter().map(StripId).collect(),
                mode: mode.into(),
            }))?;
            // Resting touches take the new mode (Ruling T6-1).
            self.repick_resting(&mut lock(&self.inner), None);
            Ok(snap)
        })
    }

    /// Every strip's mode, the targets touched now, suspend, and whether
    /// this session writes.
    pub fn automation_state(&self) -> Result<AutomationStateRow, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let m = inner.doc().mixer();
            let modes = m
                .order()
                .iter()
                .copied()
                .chain(std::iter::once(m.master()))
                .filter_map(|id| m.strip(id).ok())
                .map(|s| StripModeRow {
                    strip: s.id.0,
                    mode: s.automation.into(),
                })
                .collect();
            Ok(AutomationStateRow {
                modes,
                touched: inner
                    .touches
                    .keys()
                    .map(|t| automation_target_row(*t))
                    .collect(),
                suspended: crate::automation::suspended(),
                writing: inner.writing.is_some(),
            })
        })
    }

    /// The write in progress for `target` in the current pass, as it
    /// would land now, as `(tick, value)` in the playing sequence (Ruling
    /// 18); empty when it is not being written.
    pub fn automation_preview(
        &self,
        target: AutomationTargetRow,
    ) -> Result<Vec<PreviewPointRow>, VizError> {
        ffi_guard(|| {
            let t = automation_target_of(target);
            let mut inner = lock(&self.inner);
            let Some(w) = inner.writing.as_mut() else {
                return Ok(Vec::new());
            };
            drain_tap(w);
            let end_run = w.handles.status.run();
            let stamps = w.stamps.get(&t).cloned().unwrap_or_default();
            let holds: Vec<(AutomationTarget, f64)> =
                w.holds.iter().filter(|h| h.0 == t).copied().collect();
            let Some(w) = inner.writing.as_ref() else {
                return Ok(Vec::new());
            };
            let doc = inner.doc();
            // The pass's cached reading of the existing automation (plan
            // 5e-2 ruling P4): the Strip Chart polls this per row.
            let pd = crate::automation::PassDoc::read_with(
                self.id,
                doc,
                w.seq,
                w.rate,
                std::iter::once(t),
                w.existing.get(doc, w.seq),
            );
            // The loop passes landed at their wraps are in the document
            // already (Task 12).
            let made: Vec<_> = pd
                .writes(
                    doc,
                    &w.marks,
                    end_run,
                    &stamps,
                    &holds,
                    w.thinning,
                    w.after_write,
                )
                .into_iter()
                .filter(|x| x.pass >= w.landed)
                .collect();
            let Some(last) = made.iter().map(|x| x.pass).max() else {
                return Ok(Vec::new());
            };
            // The earlier passes' writes go first, as at landing (their
            // edges inform the last pass's).
            let before: Vec<_> = made.iter().filter(|x| x.pass < last).cloned().collect();
            let skip = pd.to_model(doc, &before).len();
            Ok(pd
                .to_model(doc, &made)
                .into_iter()
                .skip(skip)
                .flat_map(|w| w.points)
                .map(|(tick, value)| PreviewPointRow {
                    tick: tick.0,
                    value,
                })
                .collect())
        })
    }
}

/// A gesture value, finite (Ruling 16), clamped to 0–1.
fn finite(v: f64) -> Result<f64, VizError> {
    if v.is_finite() {
        Ok(core_model::automation::unit(v))
    } else {
        Err(crate::events::bad("gesture values must be finite"))
    }
}

/// The host time of a gesture: the given one, else now on a live device
/// and 0 offline (Ruling 7).
fn host_time(host_ns: Option<u64>) -> u64 {
    host_ns.unwrap_or_else(|| {
        if host::offline_mode() {
            // A live-like offline device (test hook) has a clock.
            host::offline_clock_ns().unwrap_or(0)
        } else {
            crate::clock::now_ns()
        }
    })
}

/// The stamp tag of a path (its write mode; 0 off a writing path).
fn tag_of(path: TouchPath) -> u8 {
    match path {
        TouchPath::Write(m, _) => m.tag(),
        TouchPath::Static | TouchPath::Live => 0,
    }
}

/// Moves the tap's marks and stamps into the pass (stamps by target); a
/// wrap mark sets `wrapped`.
fn drain_tap(w: &mut Writing) {
    while let Ok(m) = w.handles.marks.pop() {
        if m.wrap {
            w.wrapped = true;
        }
        w.marks.push(m);
    }
    let run = w.handles.status.run();
    while let Ok(s) = w.handles.stamps.pop() {
        // A writing stamp for a pass already landed (Task 12 review I1).
        if w.landed > 0
            && s.tag != 0
            && crate::autowrite::pass_of(&w.marks, run, s.run).is_some_and(|p| p < w.landed)
        {
            w.late = w.late.saturating_add(1);
        }
        w.stamps.entry(s.dest.target()).or_default().push(s);
    }
}

/// The controls a Write strip holds from the roll (Ruling 9): fader, pan
/// (not master), mute and send levels.
fn held_controls(doc: &Document, strip: StripId) -> Vec<AutomationTarget> {
    let Ok(s) = doc.mixer().strip(strip) else {
        return Vec::new();
    };
    let mut out = vec![AutomationTarget::StripFader(strip)];
    if strip != doc.mixer().master() {
        out.push(AutomationTarget::StripPan(strip));
    }
    out.push(AutomationTarget::StripMute(strip));
    for i in 0..s.sends.len() {
        if let Ok(i) = u16::try_from(i) {
            out.push(AutomationTarget::SendLevel(
                strip,
                core_model::automation::SendIndex(i),
            ));
        }
    }
    out
}

/// The gesture path and the writing lifecycle (plan 5e-1 Task 8).
impl Session {
    /// Ruling 16: what cannot be gestured is refused.
    fn check_gesture(&self, inner: &Inner, t: AutomationTarget) -> Result<(), VizError> {
        if matches!(t, AutomationTarget::VelocityScale(_)) {
            return Err(crate::events::bad("velocity scale cannot be gestured"));
        }
        if !core_model::automation::target_exists(inner.doc(), t) {
            return Err(crate::events::bad(&format!(
                "{} does not exist",
                t.describe()
            )));
        }
        if matches!(t, AutomationTarget::PluginParam(..))
            && crate::automation::param_info(self.id, t).is_none()
        {
            return Err(crate::events::bad(&format!(
                "{} is not a parameter of the running plugin",
                t.describe()
            )));
        }
        Ok(())
    }

    /// Whether `t` is stepped (mute, solo, a stepped plugin parameter).
    fn stepped(&self, t: AutomationTarget) -> bool {
        t.is_stepped() || crate::automation::param_info(self.id, t).is_some_and(|p| p.stepped)
    }

    /// The write mode a touch of `t` takes now (Rulings 3, 9, 14): when
    /// this session writes a pass and plays (or is about to roll), the
    /// target's strip writes, and automation is not suspended.
    /// Whether pass `w` plays now (or its Play is meant before the roll).
    fn pass_plays(&self, w: &Writing) -> bool {
        // The roll first, then the state (the renderer publishes the
        // state before it sets `rolled`).
        let rolled = w.handles.status.rolled();
        match host::transport_stopped(self.id) {
            Some(false) => true,
            // Before the roll the transport still reads stopped, while the
            // Play is meant (a Play that ended at once is not; final
            // review m4).
            Some(true) => !rolled && host::wants_play(self.id) == Some(true),
            None => false,
        }
    }

    fn write_mode_of(&self, inner: &Inner, t: AutomationTarget) -> Option<WriteMode> {
        let w = inner.writing.as_ref()?;
        if crate::automation::suspended() || !self.pass_plays(w) {
            return None;
        }
        let doc = inner.doc();
        let strip = core_model::automation::strip_of(doc, t)?;
        let mode = doc.mixer().strip(strip).ok()?.automation;
        match mode {
            // From the first wrap on, the After Write mode (Ruling 9).
            AutomationMode::Write if w.write_strips.contains(&strip) && w.wrapped => {
                WriteMode::of(w.after_write)
            }
            AutomationMode::Write if w.write_strips.contains(&strip) => Some(WriteMode::Write),
            // Switched to Write while playing: Latch until the next Play.
            AutomationMode::Write => Some(WriteMode::Latch),
            m => WriteMode::of(m),
        }
    }

    /// How a writing touch of `t` in `mode` ends (Rulings 10, 11).
    fn release_of(&self, inner: &Inner, t: AutomationTarget, mode: WriteMode) -> Release {
        let stepped = self.stepped(t);
        match mode {
            WriteMode::Latch | WriteMode::Write => Release::Hold,
            _ if stepped => Release::Hold,
            WriteMode::Touch => Release::Glide,
            WriteMode::Trim => {
                // The pass's cached reading (plan 5e-2 ruling P4).
                let automated = inner.writing.as_ref().is_some_and(|w| {
                    w.existing
                        .get(inner.doc(), w.seq)
                        .get(&t)
                        .is_some_and(|p| !p.is_empty())
                });
                if automated {
                    Release::Offsets
                } else {
                    Release::Glide
                }
            }
        }
    }

    /// The value heard now (Ruling 26).
    fn heard_value(&self, inner: &Inner, t: AutomationTarget) -> f64 {
        self.heard_from(inner, t, None)
    }

    /// [`Session::heard_value`], reading the playing sequence's automation
    /// from `cached` (its flattened existing automation) when it is that
    /// sequence's (plan 5f-1 Ruling 13: the surfaces read many targets a
    /// pass).
    fn heard_from(
        &self,
        inner: &Inner,
        t: AutomationTarget,
        cached: Option<&(SeqId, Arc<crate::automation::Existing>)>,
    ) -> f64 {
        let doc = inner.doc();
        if let AutomationTarget::PluginParam(..) = t {
            let info = crate::automation::param_info(self.id, t);
            return match info.as_ref().and_then(|p| p.value.map(|v| p.norm(v))) {
                Some(v) => v,
                None => crate::automation::fixed_value(doc, t, info.as_ref()),
            };
        }
        if let Some(v) = host::heard(self.id, t) {
            return v;
        }
        if host::transport_stopped(self.id) == Some(false) {
            if let Some((seq, tick)) = host::position(self.id) {
                let owned;
                let points: &[(Tick, f64)] = match cached {
                    Some((s, all)) if *s == seq => all.get(&t).map_or(&[][..], Vec::as_slice),
                    _ => {
                        owned = crate::automation::existing(doc, seq, t);
                        &owned
                    }
                };
                if let Some(v) = crate::automation::value_at(points, tick, t.is_stepped()) {
                    return v;
                }
            }
        }
        crate::automation::fixed_value(doc, t, None)
    }

    /// Sends one gesture message for `t`; an end the queue refused while
    /// this session holds the engine is kept for `idle` (pre-flight I3).
    fn send_gesture(
        &self,
        inner: &mut Inner,
        t: AutomationTarget,
        phase: GesturePhase,
        tag: u8,
        host_ns: u64,
    ) {
        let Some(dest) = AutoDest::of(t) else {
            return;
        };
        let msg = GestureMsg {
            dest,
            phase,
            host_ns,
            tag,
        };
        if host::gesture(self.id, msg) {
            inner.unsent_ends.clear(t);
        } else if matches!(
            phase,
            GesturePhase::Release | GesturePhase::Glide(_) | GesturePhase::Hold
        ) && host::holder() == Some(self.id)
        {
            inner.unsent_ends.put(t, msg);
        }
    }

    /// A `Ui` value of a plugin parameter goes to the plugin (plain;
    /// Ruling 15).
    fn set_plugin(&self, t: AutomationTarget, v: f64) {
        let AutomationTarget::PluginParam(slot, param) = t else {
            return;
        };
        let Some(info) = crate::automation::param_info(self.id, t) else {
            return;
        };
        let plain = core_model::automation::param_plain(v, info.min, info.max, info.stepped);
        let _ = plugins::with(self.id, |p| {
            if let Some(p) = p {
                p.set_param(slot, param, plain);
            }
        });
    }

    /// Sets `t`'s static value to `v` inside the shared static edit.
    fn run_static(&self, inner: &mut Inner, t: AutomationTarget, v: f64) {
        let Some(cmd) = crate::automation::static_command(inner.doc(), t, v) else {
            return;
        };
        if inner.history.execute(&cmd, &ViewFilter::default()).is_ok() {
            inner.version += 1;
            inner.news.doc_changed = true;
            inner.reload_host(self.id);
            self.mark(inner);
        }
    }

    /// A gesture phase on `t` from holder `h` (plan 5f-1 Ruling 2): the
    /// one path of the mouse, the channel, the surfaces and learn, as
    /// `Source::Ui`, stamped at `host_ns` (`None`: now). `Model` for a
    /// target that cannot be gestured or a non-finite value; `Busy` for a
    /// static touch's first value while another edit is open.
    pub(crate) fn gesture_held(
        &self,
        inner: &mut Inner,
        t: AutomationTarget,
        phase: core_surface::Phase,
        h: TouchHolder,
        host_ns: Option<u64>,
    ) -> Result<(), VizError> {
        use core_surface::Phase;
        // A release with no touch does nothing (Ruling 20), also for a
        // target that went meanwhile; nor does one from a hand that does
        // not hold it.
        if phase == Phase::Release {
            self.release_from(inner, t, h, host_ns);
            return Ok(());
        }
        self.check_gesture(inner, t)?;
        match phase {
            Phase::Touch(value) => {
                let v = value.map(finite).transpose()?;
                self.touch(inner, t, v, Source::Ui, h, host_ns)
            }
            Phase::Value(value) => {
                let v = finite(value)?;
                self.move_to(inner, t, v, Source::Ui, h, host_ns)
            }
            Phase::Release => Ok(()),
        }
    }

    /// A touch (Rulings 14, 20, 26): its value resolved, its path picked
    /// once, the engine told.
    fn touch(
        &self,
        inner: &mut Inner,
        t: AutomationTarget,
        value: Option<f64>,
        source: Source,
        holder: TouchHolder,
        host_ns: Option<u64>,
    ) -> Result<(), VizError> {
        if inner.touches.contains_key(&t) {
            // Touched already: the holder joins, and a touch at a value
            // moves it (plan 5f-1 Ruling 2; `move_to` joins it too).
            return match value {
                Some(v) => self.move_to(inner, t, v, source, holder, host_ns),
                None => {
                    self.join(inner, t, holder);
                    Ok(())
                }
            };
        }
        let v = match value {
            Some(v) => core_model::automation::unit(v),
            None => self.heard_value(inner, t),
        };
        let path = self.path_for(inner, t, source);
        // The static edit opens at the first value, never at the touch
        // (plan 5f-1 Ruling 2, pre-flight C2): a hand resting on a
        // touch-sensing fader holds no transaction open.
        let shared = path == TouchPath::Static && value.is_some();
        if shared {
            self.open_static_share(inner, t)?;
        }
        if let Some(w) = inner.writing.as_mut() {
            w.moved.insert(t);
        }
        inner.touches.insert(
            t,
            Touching {
                path,
                value: v,
                base: v,
                source,
                holders: TouchHolders::of(holder),
                shared,
                valued: value.is_some(),
            },
        );
        let ns = host_time(host_ns);
        // The touch is a sample (Ruling 8).
        inner.sampler.flush(t);
        let _ = inner.sampler.value(t, v, crate::automation::now(), ns);
        // Only a touch that carries a value sets it (review I-3): a click
        // on an automated fader (`Touch{None}`) changes nothing until it
        // moves.
        if value.is_some() {
            if path == TouchPath::Static {
                self.run_static(inner, t, v);
            }
            if source == Source::Ui {
                self.set_plugin(t, v);
            }
        }
        // Trim on automation grabs at an offset of 0 (pre-flight I12).
        let phase = match path {
            TouchPath::Write(_, Release::Offsets) => GesturePhase::Offset(0.0),
            _ => GesturePhase::Touch(v),
        };
        self.send_gesture(inner, t, phase, tag_of(path), ns);
        Ok(())
    }

    /// A move (a value with no touch is a touch first; Ruling 20), through
    /// the sampler (Ruling 8).
    fn move_to(
        &self,
        inner: &mut Inner,
        t: AutomationTarget,
        v: f64,
        source: Source,
        holder: TouchHolder,
        host_ns: Option<u64>,
    ) -> Result<(), VizError> {
        let v = core_model::automation::unit(v);
        if !inner.touches.contains_key(&t) {
            return self.touch(inner, t, Some(v), source, holder, host_ns);
        }
        // A value from any hand moves the target, and a hand that did not
        // hold it joins, as on an untouched target (review M5).
        let joined = self.join(inner, t, holder);
        // A resting touch takes the path its strip gives now (Ruling
        // T6-1), before its first value.
        self.repick_resting(inner, Some(t));
        // A resting static touch's first value opens (or joins) the
        // static edit; refused, the value is not applied (pre-flight C2),
        // and a hand that joined with it holds nothing it would have to
        // release.
        if let Err(e) = self.share_static(inner, t) {
            if joined && let Some(x) = inner.touches.get_mut(&t) {
                x.holders = x.holders.without(holder);
                if x.holders.is_empty() {
                    self.release(inner, t, host_ns);
                }
            }
            return Err(e);
        }
        let Some(touching) = inner.touches.get_mut(&t) else {
            return Ok(());
        };
        touching.value = v;
        touching.valued = true;
        let ns = host_time(host_ns);
        if let Some(v) = inner.sampler.value(t, v, crate::automation::now(), ns) {
            self.send_value(inner, t, v, ns);
        }
        Ok(())
    }

    /// The path a touch of `t` from `source` takes now (Ruling 14): the
    /// strip's write while a pass plays, else the plugin's heard-only path
    /// (the host's grouped gesture is the plugin's static step, Ruling 15)
    /// or the static edit.
    fn path_for(&self, inner: &Inner, t: AutomationTarget, source: Source) -> TouchPath {
        match self.write_mode_of(inner, t) {
            Some(m) => TouchPath::Write(m, self.release_of(inner, t, m)),
            None if source == Source::Plugin => TouchPath::Live,
            None => TouchPath::Static,
        }
    }

    /// `holder` joins touched `t` (plan 5f-1 Ruling 2); whether it is new.
    /// A touch's source follows its holders (review I1): a hand that is
    /// not the plugin takes a plugin's touch over. The touch becomes
    /// `Source::Ui` (its values reach the plugin), the plugin's hold and
    /// its 500 ms release go ("a target the app touches is the app's",
    /// 5e-1 Ruling 15), and a heard-only touch takes the app's path: the
    /// static edit, from its next value.
    fn join(&self, inner: &mut Inner, t: AutomationTarget, holder: TouchHolder) -> bool {
        let Some(x) = inner.touches.get(&t).copied() else {
            return false;
        };
        let joined = !x.holders.has(holder);
        let mut y = x;
        y.holders = y.holders.with(holder);
        if holder != TouchHolder::Plugin && x.source == Source::Plugin {
            y.source = Source::Ui;
            y.holders = y.holders.without(TouchHolder::Plugin);
            if y.path == TouchPath::Live {
                y.path = self.path_for(inner, t, Source::Ui);
                y.shared = false;
                if y.path != TouchPath::Live {
                    y.base = y.value;
                    self.send_gesture(
                        inner,
                        t,
                        GesturePhase::Touch(y.value),
                        tag_of(y.path),
                        host_time(None),
                    );
                }
            }
            inner.plugin_touches.forget(t);
        }
        inner.touches.insert(t, y);
        joined
    }

    /// Rulings T6-1 and T6-2 (review M1, re-review m1): while a pass
    /// plays, a touch that rests with no value and no share, on a path
    /// that writes nothing (static or heard only), takes the path its
    /// strip gives now, as if touched now, after a mode change, a suspend
    /// toggle or an Undo of either. A touch that has a value or a share
    /// keeps its path (Ruling 14), and so does a resting touch on a write
    /// path: it has written its held value since its touch, and ends as
    /// its mode's release says, so nothing it wrote is cut. A touch
    /// resting on a Write strip at the first wrap therefore keeps its
    /// Write path; it does not take the After Write path (re-review m3).
    /// The value is the one heard now (as at the pass start, re-review
    /// N1). `only`: just that target.
    fn repick_resting(&self, inner: &mut Inner, only: Option<AutomationTarget>) {
        let Some(w) = inner.writing.as_ref() else {
            return;
        };
        if !self.pass_plays(w) {
            return;
        }
        let resting: Vec<(AutomationTarget, Touching)> = inner
            .touches
            .iter()
            .filter(|(t, x)| {
                !x.valued
                    && !x.shared
                    && matches!(x.path, TouchPath::Static | TouchPath::Live)
                    && only.is_none_or(|o| o == **t)
            })
            .map(|(t, x)| (*t, *x))
            .collect();
        let ns = host_time(None);
        for (t, x) in resting {
            let path = self.path_for(inner, t, x.source);
            // The same mode keeps its release as the pass start picked it
            // (an absolute touch stays absolute, 5e-1 review I-1).
            let same = match (path, x.path) {
                (TouchPath::Write(a, _), TouchPath::Write(b, _)) => a == b,
                (a, b) => a == b,
            };
            if same {
                continue;
            }
            let v = host::heard(self.id, t).unwrap_or(x.value);
            if let Some(y) = inner.touches.get_mut(&t) {
                y.path = path;
                y.value = v;
                y.base = v;
            }
            if let Some(w) = inner.writing.as_mut() {
                w.moved.insert(t);
            }
            // The engine's entry takes the new path's tag; Trim on
            // automation grabs at an offset of 0 (pre-flight I12).
            self.send_gesture(inner, t, GesturePhase::Touch(v), tag_of(path), ns);
            if let TouchPath::Write(_, Release::Offsets) = path {
                self.send_gesture(inner, t, GesturePhase::Offset(0.0), tag_of(path), ns);
            }
        }
    }

    /// A sampled value of touched `t`: the static command on the static
    /// path, the plugin for a `Ui` plugin parameter, and the engine.
    fn send_value(&self, inner: &mut Inner, t: AutomationTarget, v: f64, host_ns: u64) {
        let Some(touching) = inner.touches.get(&t).copied() else {
            return;
        };
        // Only inside the static edit: a resting touch has no share, and
        // a value outside it would be a step of its own (pre-flight C2).
        if touching.path == TouchPath::Static && touching.shared {
            self.run_static(inner, t, v);
        }
        if touching.source == Source::Ui {
            self.set_plugin(t, v);
        }
        let phase = match touching.path {
            TouchPath::Write(_, Release::Offsets) => {
                let d = match AutoDest::of(t) {
                    Some(dest) if dest.is_db() => {
                        core_model::automation::fader_db(v)
                            - core_model::automation::fader_db(touching.base)
                    }
                    _ => v - touching.base,
                };
                GesturePhase::Offset(d)
            }
            _ => GesturePhase::Value(v),
        };
        self.send_gesture(inner, t, phase, tag_of(touching.path), host_ns);
    }

    /// A release (Rulings 10, 14, 15): the kept value first, then the
    /// path's end; a static release ends its share of the static edit.
    fn release(&self, inner: &mut Inner, t: AutomationTarget, host_ns: Option<u64>) {
        let Some(touching) = inner.touches.get(&t).copied() else {
            return;
        };
        if let Some((v, ns)) = inner.sampler.flush(t) {
            self.send_value(inner, t, v, ns);
        }
        inner.touches.remove(&t);
        if touching.source == Source::Plugin {
            inner.plugin_touches.forget(t);
        }
        let return_frames = inner
            .writing
            .as_ref()
            .map_or(0, |w| u32::try_from(w.return_frames).unwrap_or(u32::MAX));
        // Stopped, or the pass landed: a Hold or Glide would play on
        // through the next Play (pre-flight I2).
        let phase = match (inner.writing.is_some(), touching.path) {
            (false, _) | (true, TouchPath::Static | TouchPath::Live) => GesturePhase::Release,
            (true, TouchPath::Write(_, Release::Hold)) => GesturePhase::Hold,
            (true, TouchPath::Write(_, Release::Glide | Release::Offsets)) => {
                GesturePhase::Glide(return_frames)
            }
        };
        self.send_gesture(inner, t, phase, tag_of(touching.path), host_time(host_ns));
        if let (Some(w), GesturePhase::Glide(_) | GesturePhase::Hold) =
            (inner.writing.as_mut(), phase)
        {
            w.ended.insert(t);
        }
        if touching.shared {
            self.keep_static_param(inner, t, touching);
            self.end_static_share(inner);
        }
        // A Touch or Trim plugin parameter no lane drives plays its static
        // value again (pre-flight M14).
        if let (
            TouchPath::Write(WriteMode::Touch | WriteMode::Trim, _),
            AutomationTarget::PluginParam(..),
        ) = (touching.path, t)
        {
            let seq = inner
                .writing
                .as_ref()
                .map(|w| w.seq)
                .or_else(|| host::position(self.id).map(|p| p.0));
            let driven =
                seq.is_some_and(|s| !crate::automation::existing(inner.doc(), s, t).is_empty());
            if !driven {
                self.reset_plugin_param(t);
            }
        }
    }

    /// `h`'s release of `t` (plan 5f-1 Ruling 2): nothing when `h` does
    /// not hold it; the touch's [`Session::release`] when `h` was its last
    /// holder; else the kept value goes out (the other hands' last move is
    /// not lost), with no Glide or Hold: the touch goes on.
    fn release_from(
        &self,
        inner: &mut Inner,
        t: AutomationTarget,
        h: TouchHolder,
        host_ns: Option<u64>,
    ) {
        let Some(x) = inner.touches.get_mut(&t) else {
            return;
        };
        if !x.holders.has(h) {
            return;
        }
        x.holders = x.holders.without(h);
        if x.holders.is_empty() {
            self.release(inner, t, host_ns);
            return;
        }
        if let Some((v, ns)) = inner.sampler.flush(t) {
            self.send_value(inner, t, v, ns);
        }
    }

    /// A static touch's share of the static edit, from its first value
    /// (plan 5f-1 Ruling 2, pre-flight C2): opened or joined once; `Busy`
    /// while another edit is open. Nothing for a touch already sharing,
    /// or not static.
    fn share_static(&self, inner: &mut Inner, t: AutomationTarget) -> Result<(), VizError> {
        let Some(x) = inner.touches.get(&t) else {
            return Ok(());
        };
        if x.path != TouchPath::Static || x.shared {
            return Ok(());
        }
        self.open_static_share(inner, t)?;
        if let Some(x) = inner.touches.get_mut(&t) {
            x.shared = true;
        }
        Ok(())
    }

    /// Opens the shared static edit, labelled by `t`, or joins it (Ruling
    /// 14); `Busy` while an edit the gesture path did not open is open.
    fn open_static_share(&self, inner: &mut Inner, t: AutomationTarget) -> Result<(), VizError> {
        match inner.static_edit.as_mut() {
            Some((_, n)) => *n += 1,
            None => {
                if inner.history.in_transaction() {
                    return Err(VizError::Busy {
                        message: "Finish the open edit first.".into(),
                    });
                }
                let label = crate::automation::static_label(t);
                inner.history.begin(label)?;
                inner.static_edit = Some((label.to_string(), 1));
            }
        }
        Ok(())
    }

    /// A static `Ui` touch of a plugin parameter keeps its value in the
    /// document as it leaves the static edit.
    fn keep_static_param(&self, inner: &mut Inner, t: AutomationTarget, touching: Touching) {
        let (Source::Ui, AutomationTarget::PluginParam(slot, param)) = (touching.source, t) else {
            return;
        };
        let Some(info) = crate::automation::param_info(self.id, t) else {
            return;
        };
        let plain =
            core_model::automation::param_plain(touching.value, info.min, info.max, info.stepped);
        let cmd = Command::Mixer(MixerCmd::SetPluginParams {
            slot,
            params: vec![(param, plain)],
        });
        if inner.history.execute(&cmd, &ViewFilter::default()).is_ok() {
            inner.version += 1;
            inner.reload_host(self.id);
        }
    }

    /// One static touch ended: the last closes the shared edit (Ruling
    /// 14), and what waited for it lands.
    fn end_static_share(&self, inner: &mut Inner) {
        let Some((_, n)) = inner.static_edit.as_mut() else {
            return;
        };
        *n = n.saturating_sub(1);
        if *n > 0 {
            return;
        }
        inner.static_edit = None;
        if inner.history.in_transaction() && inner.history.commit().is_ok() {
            inner.version += 1;
            // `idle` sends the snapshot (review m3).
            inner.news.doc_changed = true;
        }
        self.commit_held(inner);
        self.apply_pending_pass(inner);
        self.apply_pending_automation(inner);
        self.apply_jobs_after_edit(inner);
    }

    /// Releases every touch (close).
    fn release_touches(&self, inner: &mut Inner) {
        let all: Vec<AutomationTarget> = inner.touches.keys().copied().collect();
        for t in all {
            self.release(inner, t, None);
        }
    }

    /// Releases the touches whose target no longer exists (pre-flight
    /// M9), after a document change.
    fn release_vanished(&self, inner: &mut Inner) {
        let gone: Vec<AutomationTarget> = inner
            .touches
            .keys()
            .filter(|t| !core_model::automation::target_exists(inner.doc(), **t))
            .copied()
            .collect();
        for t in gone {
            self.release(inner, t, None);
        }
    }

    /// A plugin window's parameter events (spec §7.1; Rulings 15, 27;
    /// pre-flight I8, I9, M12): each, its value normalised by the
    /// parameter's range (one the instance does not list is skipped), goes
    /// through [`PluginTouches`] onto the gesture path as `Source::Plugin`,
    /// stamped at the drain time. A target the app touches (plan 5e-2's
    /// sliders) is the app's: the plugin's report of it is skipped (so a
    /// knob the plugin grabbed while the app held it is, once the app lets
    /// go, a value-only touch, released 500 ms after its last move; review
    /// m4). Answers
    /// the parameters on a writing path (touched as a write in these
    /// events, or still touched as one), whose grouped gestures are not
    /// parameter steps (pre-flight M13).
    fn plugin_events(
        &self,
        inner: &mut Inner,
        events: &[(PluginInstanceId, core_host::ParamEvent)],
    ) -> std::collections::BTreeSet<(PluginInstanceId, ParamId)> {
        use core_host::ParamEvent;
        let mut written = std::collections::BTreeSet::new();
        if !events.is_empty() {
            let slots: std::collections::BTreeSet<PluginInstanceId> =
                events.iter().map(|(s, _)| *s).collect();
            // `pump` has just borrowed the registry, so this does not fail
            // today; if it did, Begin and Value wait for nothing (they are
            // dropped) but an End still releases (review m3).
            let ranges = plugins::with(self.id, |p| {
                let mut out = std::collections::BTreeMap::new();
                let Some(p) = p else {
                    return out;
                };
                for slot in slots {
                    if let Some(inst) = p.instance(slot) {
                        for i in &inst.info().params {
                            out.insert((slot, i.id), (i.min, i.max));
                        }
                    }
                }
                out
            });
            let busy = ranges.is_err();
            let ranges = ranges.unwrap_or_default();
            // Up to one timer tick late (pre-flight M12).
            let ns = host_time(None);
            let now = crate::automation::now();
            // Ruling 27: what the playing schedule automates.
            let seq = (host::holder() == Some(self.id))
                .then(|| {
                    inner
                        .writing
                        .as_ref()
                        .map(|w| w.seq)
                        .or_else(|| host::position(self.id).map(|p| p.0))
                })
                .flatten();
            let mut automated: std::collections::BTreeMap<AutomationTarget, bool> =
                Default::default();
            for (slot, e) in events {
                let (param, m) = match *e {
                    ParamEvent::Begin(p) => (p, PluginMove::Begin),
                    ParamEvent::Value(p, v) => (p, PluginMove::Value(v)),
                    ParamEvent::End(p) => (p, PluginMove::End),
                };
                let m = match (ranges.get(&(*slot, param)), m) {
                    (Some(&(min, max)), PluginMove::Value(v)) => {
                        PluginMove::Value(crate::automation::norm(v, min, max))
                    }
                    (Some(_), m) => m,
                    (None, PluginMove::End) if busy => PluginMove::End,
                    // A parameter the instance does not list.
                    (None, _) => continue,
                };
                let t = AutomationTarget::PluginParam(*slot, param);
                if inner
                    .touches
                    .get(&t)
                    .is_some_and(|x| x.source == Source::Ui)
                {
                    continue;
                }
                let doc = inner.doc();
                let auto = *automated.entry(t).or_insert_with(|| {
                    seq.is_some_and(|s| !crate::automation::existing(doc, s, t).is_empty())
                });
                let writes = |inner: &Inner| {
                    inner
                        .touches
                        .get(&t)
                        .is_some_and(|x| matches!(x.path, TouchPath::Write(..)))
                };
                let was = writes(inner);
                for mv in inner.plugin_touches.event(t, m, now, auto) {
                    // The plugin path never opens an edit, so it is never
                    // refused.
                    let _ = match mv {
                        Move::Touch(v) => {
                            self.touch(inner, t, v, Source::Plugin, TouchHolder::Plugin, Some(ns))
                        }
                        Move::Value(v) => {
                            self.move_to(inner, t, v, Source::Plugin, TouchHolder::Plugin, Some(ns))
                        }
                        Move::Release => {
                            self.release_from(inner, t, TouchHolder::Plugin, Some(ns));
                            Ok(())
                        }
                    };
                }
                if was || writes(inner) {
                    written.insert((*slot, param));
                }
            }
        }
        for (t, x) in &inner.touches {
            if let (AutomationTarget::PluginParam(slot, param), TouchPath::Write(..)) = (t, x.path)
            {
                written.insert((*slot, *param));
            }
        }
        written
    }

    /// Releases the plugin windows' touches that ended by themselves
    /// (spec §7.1, pre-flight I7): value-only touches 500 ms after their
    /// last value (stamped then on a live device), and the touches of
    /// slots that no longer run.
    fn release_plugin_touches(&self, inner: &mut Inner) {
        let now = crate::automation::now();
        let mut ended: Vec<(AutomationTarget, Option<u64>)> = Vec::new();
        for (t, at) in inner.plugin_touches.idle(now) {
            let ns = if host::offline_mode() {
                0
            } else {
                let ago = u64::try_from(now.saturating_sub(at).as_nanos()).unwrap_or(u64::MAX);
                crate::clock::now_ns().saturating_sub(ago)
            };
            ended.push((t, Some(ns)));
        }
        let slots = inner.plugin_touches.slots();
        if !slots.is_empty() {
            let live = plugins::with(self.id, |p| {
                slots
                    .iter()
                    .filter(|s| {
                        p.as_ref()
                            .is_some_and(|p| p.state(**s) == Some(core_host::SlotState::Live))
                    })
                    .copied()
                    .collect::<std::collections::BTreeSet<PluginInstanceId>>()
            });
            // A busy registry says nothing: they wait for the next idle.
            if let Ok(live) = live {
                for t in inner.plugin_touches.release_slots(|s| live.contains(&s)) {
                    ended.push((t, None));
                }
            }
        }
        for (t, ns) in ended {
            if inner
                .touches
                .get(&t)
                .is_some_and(|x| x.source == Source::Plugin)
            {
                self.release_from(inner, t, TouchHolder::Plugin, ns);
            }
        }
    }

    /// Task 9 review I-1 (Ruling 15, "the static value stays"): after a
    /// change that added, removed or changed a plugin parameter's
    /// automation (an undo or redo of a pass, a lane deleted, a landing),
    /// the plugin is sent what now plays there: the automation's value at
    /// the position where the playing (or last) sequence automates it,
    /// else its static value. A target touched now is left to the hand.
    ///
    /// `before` is the document the change started from, when the caller
    /// has it; else the one last seen here (every edit, undo, redo and
    /// landing calls this, and `idle` catches the rest).
    fn resync_plugin_automation(&self, inner: &mut Inner, before: Option<&Document>) {
        let doc = inner.doc().clone();
        let seen = inner.plugin_auto_doc.sequences().ptr_eq(doc.sequences());
        // The change from `before` (an undo can come back to the very
        // document last seen here, so that alone says nothing).
        let from_before = before
            .filter(|b| !b.sequences().ptr_eq(doc.sequences()))
            .map(crate::automation::plugin_automation);
        if seen && from_before.is_none() {
            return;
        }
        let now = crate::automation::plugin_automation(&doc);
        let cached = std::mem::take(&mut inner.plugin_auto);
        let mut changed: std::collections::BTreeSet<AutomationTarget> = Default::default();
        for old in std::iter::once(&cached).chain(from_before.as_ref()) {
            changed.extend(
                old.keys()
                    .chain(now.keys())
                    .filter(|t| old.get(t) != now.get(t))
                    .copied(),
            );
        }
        inner.plugin_auto = now;
        inner.plugin_auto_doc = doc;
        self.sync_plugin_targets(inner, changed);
    }

    /// Plan 5e-1 Task 9b, item 4: a plugin that finished loading (at open,
    /// or loaded again) got its state, then its overlay (the static
    /// values); its automated parameters are put where their automation
    /// plays at the position, once, through the same path as
    /// [`Session::resync_plugin_automation`].
    fn sync_loaded_plugins(&self, inner: &mut Inner, loaded: &[PluginInstanceId]) {
        if loaded.is_empty() {
            return;
        }
        // One scan of the document for every loaded slot (review m4).
        let all = core_model::automation::automated_targets(inner.doc());
        let targets: std::collections::BTreeSet<AutomationTarget> = all
            .into_iter()
            .filter(|(slot, _)| loaded.contains(slot))
            .map(|(slot, p)| AutomationTarget::PluginParam(slot, p))
            .collect();
        self.sync_plugin_targets(inner, targets);
    }

    /// Sends each plugin parameter in `targets` the value its automation
    /// plays at the position (the playing sequence's, else the cursor's),
    /// else its static value. A target touched now is left to the hand.
    fn sync_plugin_targets(
        &self,
        inner: &mut Inner,
        changed: std::collections::BTreeSet<AutomationTarget>,
    ) {
        if changed.is_empty() {
            return;
        }
        let (seq, tick) = match host::position(self.id) {
            Some((s, t)) => (Some(s), Some(t)),
            None => (inner.cursor_seq, inner.cursor),
        };
        let tick = tick.unwrap_or(Tick::ZERO);
        for t in changed {
            if inner.touches.contains_key(&t) {
                continue;
            }
            let AutomationTarget::PluginParam(slot, param) = t else {
                continue;
            };
            let points =
                seq.map_or_else(Vec::new, |s| crate::automation::existing(inner.doc(), s, t));
            if points.is_empty() {
                self.reset_plugin_param(t);
                continue;
            }
            let Some(info) = crate::automation::param_info(self.id, t) else {
                continue;
            };
            let Some(v) = crate::automation::value_at(&points, tick, self.stepped(t)) else {
                continue;
            };
            let plain = core_model::automation::param_plain(v, info.min, info.max, info.stepped);
            let _ = plugins::with(self.id, |p| {
                if let Some(p) = p {
                    p.set_param(slot, param, plain);
                }
            });
        }
    }

    /// Puts plugin parameter `t` back at its static value: the document's
    /// overlay, else the plugin's own value before it (no commit).
    fn reset_plugin_param(&self, t: AutomationTarget) {
        let AutomationTarget::PluginParam(slot, param) = t else {
            return;
        };
        let _ = plugins::with(self.id, |p| {
            if let Some(p) = p {
                p.reset_param(slot, param);
            }
        });
    }

    /// Ends kept gesture messages the queue refused, while it refuses them
    /// (pre-flight I3).
    fn resend_ends(&self, inner: &mut Inner) {
        let id = self.id;
        inner.unsent_ends.resend(|m| host::gesture(id, m));
    }

    /// Starts writing a pass of `seq` once Play went out (Rulings 7, 9,
    /// 19, 20): the tap, the settings, Write strips' holds, and every held
    /// touch's path picked again.
    fn begin_writing(&self, inner: &mut Inner, seq: SeqId, at: Tick) {
        inner.restart_writing = None;
        if inner.writing.is_some() {
            return;
        }
        let Some(handles) = host::start_gestures(self.id) else {
            inner
                .news
                .problems
                .push("Automation is not being written this pass: the engine was busy.".into());
            return;
        };
        let (after_write, return_ms, thinning) = crate::automation::pass_settings();
        let rate = host::device(self.id)
            .map(|(_, r)| r)
            .filter(|r| *r > 0.0)
            .unwrap_or_else(|| f64::from(inner.doc().settings().format.sample_rate));
        let return_frames = (f64::from(return_ms) * rate / 1000.0).round() as u64;
        let doc = inner.doc();
        let m = doc.mixer();
        let write_strips: std::collections::BTreeSet<StripId> = m
            .order()
            .iter()
            .copied()
            .chain(std::iter::once(m.master()))
            .filter(|s| {
                m.strip(*s)
                    .is_ok_and(|x| x.automation == AutomationMode::Write)
            })
            .collect();
        // Every Write strip's controls, at their values at the play-start
        // tick (Ruling 9; the punch and record range play no part, Ruling
        // 28). None while suspended (Ruling 3).
        let mut holds: Vec<(AutomationTarget, f64)> = Vec::new();
        let existing = crate::automation::ExistingCache::default();
        if !crate::automation::suspended() {
            // The pass's own start: the engine's published position still
            // shows where it stood before a queued Locate (review I-2).
            let tick = at;
            let all = existing.get(doc, seq);
            for s in &write_strips {
                for t in held_controls(doc, *s) {
                    // A control the hand holds over the pass's start is held
                    // at the hand's value (re-review 2 N4).
                    let v = match inner.touches.get(&t) {
                        Some(x) => x.value,
                        None => all
                            .get(&t)
                            .and_then(|p| crate::automation::value_at(p, tick, t.is_stepped()))
                            .unwrap_or_else(|| crate::automation::fixed_value(doc, t, None)),
                    };
                    holds.push((t, v));
                }
            }
        }
        inner.writing = Some(Writing {
            seq,
            handles,
            marks: Vec::new(),
            stamps: Default::default(),
            holds: holds.clone(),
            write_strips,
            wrapped: false,
            return_frames,
            after_write,
            thinning,
            dropped_at_start: host::gestures_dropped(self.id).unwrap_or(0),
            rate,
            ended: holds.iter().map(|h| h.0).collect(),
            holds_resent: false,
            // Touches held over the pass's start count as moved: the holds
            // re-sent at the roll spare them (re-review 2 N4).
            moved: inner.touches.keys().copied().collect(),
            landed: 0,
            step: None,
            after_done: false,
            late: 0,
            late_said: 0,
            existing,
        });
        let tag = WriteMode::Write.tag();
        let ns = host_time(None);
        for (t, v) in holds {
            self.send_gesture(inner, t, GesturePhase::Touch(v), tag, ns);
            // A touched control stays touched; its release holds it.
            if !inner.touches.contains_key(&t) {
                self.send_gesture(inner, t, GesturePhase::Hold, tag, ns);
            }
        }
        // Touches held over the pass's start (Ruling 20 as amended, M2):
        // each writes when its strip writes now, else is heard only. A
        // static touch (held over Play from while stopped) writes too when
        // its strip writes now (re-review 3 n1, fix round 4): a fader held
        // on a Write strip while Play is pressed writes its ride, and its
        // release follows the strip's mode. It leaves the static edit (its
        // share ends below); on a strip that does not write it keeps the
        // static path.
        let held: Vec<(AutomationTarget, Touching)> =
            inner.touches.iter().map(|(t, x)| (*t, *x)).collect();
        let mut left_static = 0usize;
        for (t, x) in held {
            let mut converted = false;
            // By the touch's own path and share (plan 5f-1 Ruling 2): a
            // resting static touch holds no share, yet stays static on a
            // strip that does not write and converts on one that does.
            if x.path == TouchPath::Static {
                if self.write_mode_of(inner, t).is_none() {
                    continue;
                }
                if x.shared {
                    self.keep_static_param(inner, t, x);
                    left_static += 1;
                }
                converted = true;
            }
            // A Trim offset stays an offset from its own grab (review I-1,
            // Ruling 11: the engine keeps `Offset` entries over a Stop or a
            // Locate); an absolute touch stays absolute. A converted static
            // touch on an automated Trim lane grabs at an offset of 0 from
            // the hand's value now, as a Trim grab does (re-review 4 N1,
            // pre-flight I12): its stopped moves were static edits, so it
            // carries no offset.
            let was_offset = matches!(x.path, TouchPath::Write(_, Release::Offsets));
            let path = match self.write_mode_of(inner, t) {
                Some(m) => TouchPath::Write(
                    m,
                    match self.release_of(inner, t, m) {
                        Release::Offsets if !was_offset && !converted => Release::Glide,
                        r => r,
                    },
                ),
                None => TouchPath::Live,
            };
            let is_offset = matches!(path, TouchPath::Write(_, Release::Offsets));
            if was_offset && !is_offset {
                // The engine's entry becomes absolute at the value heard,
                // so the next value does not jump from an offset.
                let v = host::heard(self.id, t).unwrap_or(x.value);
                self.send_gesture(inner, t, GesturePhase::Touch(v), tag_of(path), ns);
            }
            if converted && is_offset {
                self.send_gesture(inner, t, GesturePhase::Offset(0.0), tag_of(path), ns);
            }
            if let Some(y) = inner.touches.get_mut(&t) {
                y.path = path;
                if !is_offset || converted {
                    y.base = y.value;
                }
                // A converted touch leaves the static edit.
                if converted {
                    y.shared = false;
                }
            }
        }
        // The static touches that now write end their share of the static
        // edit as their release would: the last closes it as its own step
        // (what was set while stopped), and what waited for it lands.
        for _ in 0..left_static {
            self.end_static_share(inner);
        }
    }

    /// `idle`'s writing step: the kept ends, the sampler's due values, the
    /// tap's marks and stamps, and the pass's end (Ruling 24).
    fn service_writing(&self) {
        let mut inner = lock(&self.inner);
        self.resend_ends(&mut inner);
        self.release_plugin_touches(&mut inner);
        // A suspend toggle or an undone mode change (Ruling T6-1).
        self.repick_resting(&mut inner, None);
        for (t, v, ns) in inner.sampler.due(crate::automation::now()) {
            self.send_value(&mut inner, t, v, ns);
        }
        let holder = match host::holder() {
            Some(h) if h == self.id => Holder::This,
            Some(_) => Holder::Other,
            None => Holder::Nobody,
        };
        // The roll is read before the state (see `write_mode_of`).
        let rolled = inner
            .writing
            .as_ref()
            .is_some_and(|w| w.handles.status.rolled());
        let stopped = host::transport_stopped(self.id);
        // The intent, not the renderer's state: a rebuilt engine's Play
        // reads stopped until it applies it.
        let wants_play = host::wants_play(self.id) == Some(true);
        let Some(w) = inner.writing.as_mut() else {
            // A pass a rebuild ended starts again once the rebuilt engine
            // plays; a Stop or another session's engine ends the wait.
            if let Some(seq) = inner.restart_writing {
                match (holder, wants_play) {
                    (Holder::This, true) => {
                        // The old pass lands first (re-review N1).
                        inner.last_step = None;
                        self.apply_pending_automation(&mut inner);
                        let step = inner.last_step.take().or(inner.restart_step.take());
                        let at = self.resume_tick();
                        self.begin_writing(&mut inner, seq, at);
                        self.keep_loop_step(&mut inner, step);
                    }
                    (Holder::Nobody, _) => {}
                    _ => {
                        inner.restart_writing = None;
                        inner.restart_step = None;
                    }
                }
            }
            return;
        };
        // The run before the stamps: every stamp of a block up to it is
        // in the ring by then (Task 12's landing at a wrap).
        let run = w.handles.status.run();
        drain_tap(w);
        // A stamp for a pass already landed is lost: said once per batch
        // (Task 12 review I1).
        if w.late > w.late_said {
            w.late_said = w.late;
            inner.news.problems.push(
                "Some automation moves came after their loop pass had landed and were lost.".into(),
            );
        }
        let Some(w) = inner.writing.as_mut() else {
            return;
        };
        let (closed, seq) = (w.handles.status.closed(), w.seq);
        // Re-review m4: the holds went out right after the Play's (or the
        // Locate's) command; a command swap the renderer put off could
        // apply that Locate, whose `end_holds` ends them, after them. The
        // tap is installed by a command queued after the Locate, so once it
        // rolled the Locate applied: the holds go again, at the same
        // values (a flat stretch gains no point), sparing targets touched
        // now or earlier in the pass.
        let resend: Vec<(AutomationTarget, f64)> = if rolled && !w.holds_resent && !closed {
            w.holds_resent = true;
            // Not one a touch already moved this pass.
            w.holds
                .iter()
                .filter(|(t, _)| !w.moved.contains(t))
                .copied()
                .collect()
        } else {
            Vec::new()
        };
        if !resend.is_empty() {
            let (tag, ns) = (WriteMode::Write.tag(), host_time(None));
            for (t, v) in resend {
                if inner.touches.contains_key(&t) {
                    continue;
                }
                self.send_gesture(&mut inner, t, GesturePhase::Touch(v), tag, ns);
                self.send_gesture(&mut inner, t, GesturePhase::Hold, tag, ns);
            }
        }
        // A recording ends its own pass and takes the writes with it; its
        // loop passes land into its group as they end (Task 12).
        if inner.finishing {
            return;
        }
        if inner.recording.is_some() {
            if !closed {
                self.land_wraps(&mut inner, run);
            }
            return;
        }
        match crate::automation::pass_step(closed, rolled, stopped, wants_play, holder) {
            PassStep::Go => self.land_wraps(&mut inner, run),
            PassStep::Land => self.finish_writing(&mut inner),
            PassStep::Restart => {
                // The rebuilt engine plays on (Ruling 21; controller
                // ruling 3): what was written lands as Stop lands it (After
                // Write included, so the new pass holds the strips' new
                // modes), then writing goes on from where it resumed
                // (re-review N1). A looped pass keeps its one step (Task
                // 12).
                let old = inner.writing.as_ref().and_then(|w| w.step);
                inner.last_step = None;
                self.finish_writing(&mut inner);
                self.apply_pending_automation(&mut inner);
                let step = inner.last_step.take().or(old);
                let at = self.resume_tick();
                self.begin_writing(&mut inner, seq, at);
                self.keep_loop_step(&mut inner, step);
            }
            PassStep::RestartLater => {
                let old = inner.writing.as_ref().and_then(|w| w.step);
                inner.last_step = None;
                self.finish_writing(&mut inner);
                inner.restart_writing = Some(seq);
                inner.restart_step = old;
            }
        }
    }

    /// A looped pass a rebuild restarted goes on landing into the step its
    /// part before the rebuild landed in (plan 5e-1 Task 12: one undo step
    /// per looped pass).
    fn keep_loop_step(&self, inner: &mut Inner, step: Option<u64>) {
        let looped = inner.looped;
        if let Some(w) = inner.writing.as_mut().filter(|_| looped) {
            w.step = step;
        }
    }

    /// Plan 5e-1 Task 12 (owner 2026-10-29; spec §7.4): while looping, the
    /// loop passes that are over as of tap run `run` (the next pass heard
    /// for [`crate::automation::WRAP_MARGIN_MS`]) land now, so the next
    /// cycle plays what was just written: into the pass's one undo step,
    /// or into the recording's group. Put off while an edit is open (the
    /// next `idle` lands them). A pass that wrote nothing makes no step.
    fn land_wraps(&self, inner: &mut Inner, run: u64) {
        let Some(w) = inner.writing.as_ref() else {
            return;
        };
        // Heard, not rendered (Task 12 review I1): a device with a clock
        // renders two buffers and its output latency ahead of what is
        // heard; the graph latency is in the pass's marks.
        let forced = *crate::automation::WRAP_LAG
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let lag = if let Some(l) = forced {
            l
        } else if !host::offline_mode() || host::offline_clock_ns().is_some() {
            host::input_info(self.id).map_or(0, |(_, i)| {
                u64::from(i.output_latency_frames)
                    .saturating_add(u64::from(i.buffer_frames).saturating_mul(2))
            })
        } else {
            0
        };
        let margin = ((crate::automation::WRAP_MARGIN_MS * w.rate / 1000.0).round() as u64)
            .saturating_add(lag);
        let over = crate::autowrite::passes_over(&w.marks, run, margin);
        if over <= w.landed {
            return;
        }
        let recording = inner.recording.is_some();
        if inner.history.in_transaction()
            || inner.static_edit.is_some()
            || inner.history.in_group() != recording
        {
            return;
        }
        let writes = w.model_writes(self.id, inner.doc(), run, Some(over));
        let (step, after, after_done) = (w.step, w.after_write, w.after_done);
        let strips: Vec<StripId> = w.write_strips.iter().copied().collect();
        if let Some(w) = inner.writing.as_mut() {
            w.landed = over;
            // The landed passes' stamps are done with: a later pass's
            // writes never read them (a wrap reopens nothing), so each
            // landing costs only its own passes (Task 12 review m5).
            let pieces =
                crate::passes::writing_pieces(&w.marks, i64::try_from(run).unwrap_or(i64::MAX));
            for list in w.stamps.values_mut() {
                list.retain(|s| {
                    crate::passes::piece_of(&pieces, s.run).is_none_or(|p| p.pass >= over)
                });
            }
            w.stamps.retain(|_, list| !list.is_empty());
        }
        let landing = crate::automation::Landing {
            writes,
            strips,
            after,
            step,
            after_done,
        };
        inner.last_step = None;
        let landed = self.land(inner, landing);
        // The landed cycle is existing automation now: the next read of
        // the pass's cache flattens the document that holds it (plan 5e-2
        // ruling P4; its sequences changed, so the cache would see that
        // anyway).
        if let Some(w) = inner.writing.as_ref() {
            w.existing.clear();
        }
        if landed {
            let step = inner.last_step.take();
            if let Some(w) = inner.writing.as_mut() {
                w.after_done = true;
                if !recording {
                    w.step = step;
                }
            }
        }
    }

    /// Where a rebuilt engine resumed: the rebuild's tick (its renderer
    /// may not have published a position yet), else the position.
    fn resume_tick(&self) -> Tick {
        host::resumed_at(self.id)
            .or_else(|| host::position(self.id).map(|p| p.1))
            .unwrap_or(Tick::ZERO)
    }

    /// Ends the pass and answers its landing: the writes of the passes
    /// not landed yet, the Write strips that still exist (none once After
    /// Write went), the After Write mode and the pass's step (nothing
    /// without a pass).
    fn take_writes(&self, inner: &mut Inner) -> crate::automation::Landing {
        let Some(mut w) = inner.writing.take() else {
            return crate::automation::Landing {
                writes: Vec::new(),
                strips: Vec::new(),
                after: AutomationMode::Latch,
                step: None,
                after_done: false,
            };
        };
        if host::holder() == Some(self.id) {
            host::stop_gestures(self.id);
            // The renderer marks the Stop first (M1).
            let until = Instant::now() + std::time::Duration::from_millis(250);
            while !w.handles.status.closed() && Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
        drain_tap(&mut w);
        let end_run = w.handles.status.run();
        // A glide or hold the renderer took after the Stop (or that a Play
        // while playing, which ends no hold, would carry on) ends now; a
        // Release with no live entry does nothing. Touched targets keep
        // their touch (Ruling 20).
        let ns = host_time(None);
        for t in std::mem::take(&mut w.ended) {
            if !inner.touches.contains_key(&t) {
                self.send_gesture(inner, t, GesturePhase::Release, 0, ns);
            }
        }
        let dropped = host::gestures_dropped(self.id).is_some_and(|d| d > w.dropped_at_start);
        if w.handles.status.lost_stamps() > 0
            || w.handles.status.lost_marks() > 0
            || dropped
            || w.late > w.late_said
        {
            inner
                .news
                .problems
                .push("Some automation moves were lost while writing.".into());
        }
        // The loop passes landed at their wraps are in the document
        // (Task 12): only the rest lands now.
        let writes = w.model_writes(self.id, inner.doc(), end_run, None);
        let writes = self.drop_gone(inner, writes);
        let strips = if writes.is_empty() {
            Vec::new()
        } else {
            let m = inner.doc().mixer();
            w.write_strips
                .iter()
                .copied()
                .filter(|s| m.strip(*s).is_ok())
                .collect()
        };
        crate::automation::Landing {
            writes,
            strips,
            after: w.after_write,
            step: w.step,
            after_done: w.after_done,
        }
    }

    /// Ruling 25: writes whose target or sequence went are dropped, one
    /// problem line per target.
    fn drop_gone(&self, inner: &mut Inner, writes: Vec<AutomationWrite>) -> Vec<AutomationWrite> {
        let mut said: std::collections::BTreeSet<AutomationTarget> = Default::default();
        let mut kept = Vec::with_capacity(writes.len());
        for w in writes {
            let doc = inner.doc();
            if core_model::automation::target_exists(doc, w.target) && doc.sequence(w.seq).is_ok() {
                kept.push(w);
            } else if said.insert(w.target) {
                inner.news.problems.push(format!(
                    "Automation of {} was not written: it no longer exists.",
                    w.target.describe()
                ));
            }
        }
        kept
    }

    /// Ends the pass; its writes wait to land as "Write Automation"
    /// (Stop, the jumps of Ruling 23, a failed Play, close).
    fn finish_writing(&self, inner: &mut Inner) {
        let landing = self.take_writes(inner);
        if landing.writes.is_empty() {
            return;
        }
        inner.pending_automation.push(landing);
    }

    /// Ends the pass into a recording's pass (spec §7.4): its writes join
    /// `pass`, or make one when the recording recorded nothing (Ruling
    /// 25).
    fn writes_into_pass(&self, inner: &mut Inner, pass: Option<RecordPass>) -> Option<RecordPass> {
        let crate::automation::Landing {
            writes,
            strips,
            after,
            after_done,
            ..
        } = self.take_writes(inner);
        if writes.is_empty() {
            return pass;
        }
        // After Write went with a loop pass's landing: only strips still
        // in Write get it now (Task 12 review m6).
        let strips = if after_done {
            let m = inner.doc().mixer();
            strips
                .into_iter()
                .filter(|s| {
                    m.strip(*s)
                        .is_ok_and(|x| x.automation == AutomationMode::Write)
                })
                .collect()
        } else {
            strips
        };
        inner.after_write_pending = Some((strips, after));
        Some(match pass {
            Some(mut p) => {
                p.automation.extend(writes);
                p
            }
            None => RecordPass {
                automation: writes,
                ..Default::default()
            },
        })
    }

    /// Inside a recording's group: the Write strips become the After
    /// Write mode when automation landed (Ruling 9).
    fn after_write_in_group(&self, inner: &mut Inner, landed: bool) {
        let Some((strips, mode)) = inner.after_write_pending.take() else {
            return;
        };
        let m = inner.doc().mixer();
        let strips: Vec<StripId> = strips.into_iter().filter(|s| m.strip(*s).is_ok()).collect();
        if !landed || strips.is_empty() {
            return;
        }
        let cmd = Command::Mixer(MixerCmd::SetAutomationMode { strips, mode });
        if let Err(e) = inner.history.execute(&cmd, &ViewFilter::default()) {
            inner
                .news
                .problems
                .push(format!("The automation mode could not be set ({e})."));
        }
    }

    /// A recording's pass the model refused: its automation alone, inside
    /// the group (Ruling 25). Its report when it landed.
    fn land_automation_alone(
        &self,
        inner: &mut Inner,
        automation: Vec<AutomationWrite>,
    ) -> Option<core_model::command::Report> {
        let automation = self.drop_gone(inner, automation);
        if automation.is_empty() {
            return None;
        }
        let pass = RecordPass {
            automation,
            ..Default::default()
        };
        match inner
            .history
            .execute(&Command::Record(pass), &ViewFilter::default())
        {
            Ok(r) => Some(r),
            Err(e) => {
                inner
                    .news
                    .problems
                    .push(format!("The automation could not be written ({e})."));
                None
            }
        }
    }

    /// Lands a finished pass's writes as one undo step "Write Automation"
    /// (with After Write in the same step), or into the step its loop
    /// passes landed in (Task 12), once no edit is open.
    fn apply_pending_automation(&self, inner: &mut Inner) {
        if inner.history.in_transaction() || inner.history.in_group() || inner.static_edit.is_some()
        {
            return;
        }
        // Oldest first, each pass its own step (Ruling 23, review m2).
        for landing in std::mem::take(&mut inner.pending_automation) {
            self.land(inner, landing);
        }
    }

    /// Writes the MPE settings controllers' MCMs gave lanes back into the
    /// document (plan 5g-2 Ruling 4; a zone turned off is `None`), each
    /// lane whose setting differs as one undo step "Set Track MPE". The
    /// reports wait while a recording runs, finishes or has a pass to
    /// land, and while an edit, transaction or group is open. The step is
    /// the controller's, not an input change: it takes no view filter and
    /// skips `retries_input_cmd` and the claim handling on purpose.
    fn apply_mpe_reports(&self, inner: &mut Inner) {
        for (lane, cfg) in host::take_mpe_reports(self.id) {
            inner.mpe_owed.insert(lane, cfg);
        }
        if inner.mpe_owed.is_empty()
            || inner.recording.is_some()
            || inner.finishing
            || inner.pending_pass.is_some()
            || inner.static_edit.is_some()
            || inner.history.in_transaction()
            || inner.history.in_group()
        {
            return;
        }
        let mut wrote = false;
        for (lane, mpe) in std::mem::take(&mut inner.mpe_owed) {
            // Lane IDs are unique in the document: the lane's sequence, if
            // its setting differs.
            let Some(seq) = inner.doc().sequences().iter().find_map(|(id, s)| {
                s.lanes
                    .iter()
                    .any(|l| l.id == lane && l.mpe != mpe)
                    .then_some(*id)
            }) else {
                continue;
            };
            let cmd = Command::Lane(LaneCmd::SetMpe { seq, lane, mpe });
            if let Err(e) = inner.history.execute(&cmd, &ViewFilter::default()) {
                inner.news.problems.push(format!(
                    "The track's MPE setting could not be updated ({e})."
                ));
                continue;
            }
            inner.version += 1;
            inner.news.doc_changed = true;
            wrote = true;
        }
        if wrote {
            // One reload for every lane written: `idle` runs it next.
            inner.reload_owed = true;
            self.mark(inner);
        }
    }

    /// Lands one pass's writes (see [`Session::apply_pending_automation`];
    /// plan 5e-1 Task 12): inside a recording's group as part of its step;
    /// else into `step` while that step is still the present (one Undo
    /// takes back the whole looped pass), else as a new step "Write
    /// Automation"; After Write for `strips` with it. True when it landed;
    /// outside a recording `last_step` is then its step.
    fn land(&self, inner: &mut Inner, landing: crate::automation::Landing) -> bool {
        let crate::automation::Landing {
            writes,
            strips,
            after,
            step,
            after_done,
        } = landing;
        let writes = self.drop_gone(inner, writes);
        if writes.is_empty() {
            return false;
        }
        let m = inner.doc().mixer();
        let strips: Vec<StripId> = strips.into_iter().filter(|s| m.strip(*s).is_ok()).collect();
        // After Write went with an earlier landing; if that step was undone
        // (or another step covers it) the strips still in Write get it
        // again with this one (Task 12 review m6).
        let still_write: Vec<StripId> = strips
            .iter()
            .copied()
            .filter(|s| {
                m.strip(*s)
                    .is_ok_and(|x| x.automation == AutomationMode::Write)
            })
            .collect();
        let mode_of = |strips: Vec<StripId>| {
            (!strips.is_empty()).then(|| {
                Command::Mixer(MixerCmd::SetAutomationMode {
                    strips,
                    mode: after,
                })
            })
        };
        let pass = Command::Record(RecordPass {
            automation: writes.clone(),
            ..Default::default()
        });
        let f = ViewFilter::default();
        let in_group = inner.history.in_group();
        let amended = match step {
            Some(serial) if !in_group => inner.history.amend(&pass, &f, serial),
            _ => Ok(None),
        };
        let mode_failed = |inner: &mut Inner, e: core_model::error::ModelError| {
            inner
                .news
                .problems
                .push(format!("The automation mode could not be set ({e})."));
        };
        let landed: Result<core_model::command::Report, core_model::error::ModelError> =
            match amended {
                Err(e) => Err(e),
                Ok(Some(report)) => {
                    let mode = if after_done { None } else { mode_of(strips) };
                    if let Some(cmd) = &mode
                        && let Some(serial) = step
                        && let Err(e) = inner.history.amend(cmd, &f, serial)
                    {
                        mode_failed(inner, e);
                    }
                    Ok(report)
                }
                Ok(None) => {
                    let mode = if after_done {
                        mode_of(still_write)
                    } else {
                        mode_of(strips)
                    };
                    if in_group {
                        let r = inner.history.execute(&pass, &f);
                        if r.is_ok() {
                            inner.landed_in_group = true;
                            if let Some(cmd) = &mode
                                && let Err(e) = inner.history.execute(cmd, &f)
                            {
                                mode_failed(inner, e);
                            }
                        }
                        r
                    } else {
                        match &mode {
                            None => inner.history.execute(&pass, &f),
                            Some(cmd) => match inner.history.begin_group() {
                                Err(e) => Err(e),
                                Ok(()) => {
                                    let r = inner.history.execute(&pass, &f);
                                    if r.is_ok()
                                        && let Err(e) = inner.history.execute(cmd, &f)
                                    {
                                        mode_failed(inner, e);
                                    }
                                    let _ = inner.history.end_group("Write Automation");
                                    r
                                }
                            },
                        }
                    }
                }
            };
        let report = match landed {
            Ok(r) => r,
            Err(e) => {
                inner
                    .news
                    .problems
                    .push(format!("The automation could not be written ({e})."));
                return false;
            }
        };
        if !in_group {
            inner.last_step = Some(inner.history.present_serial());
        }
        let silent = crate::automation::silent_new_lanes(inner.doc(), &report.created, &writes);
        inner.news.problems.extend(silent);
        inner.version += 1;
        inner.news.doc_changed = true;
        inner.reload_host(self.id);
        self.mark(inner);
        self.sync_written_plugins(inner, &writes);
        self.resync_plugin_automation(inner, None);
        self.release_vanished(inner);
        true
    }

    /// After a landing, each written plugin parameter is sent the landed
    /// automation's value at the transport tick (Ruling 15, M13).
    fn sync_written_plugins(&self, inner: &mut Inner, writes: &[AutomationWrite]) {
        let tick = host::position(self.id)
            .map(|p| p.1)
            .or(inner.cursor)
            .unwrap_or(Tick::ZERO);
        let mut done: std::collections::BTreeSet<AutomationTarget> = Default::default();
        for w in writes {
            let t = w.target;
            let AutomationTarget::PluginParam(slot, param) = t else {
                continue;
            };
            if !done.insert(t) {
                continue;
            }
            let Some(info) = crate::automation::param_info(self.id, t) else {
                continue;
            };
            let points = crate::automation::existing(inner.doc(), w.seq, t);
            let Some(v) = crate::automation::value_at(&points, tick, self.stepped(t)) else {
                continue;
            };
            let plain = core_model::automation::param_plain(v, info.min, info.max, info.stepped);
            let _ = plugins::with(self.id, |p| {
                if let Some(p) = p {
                    p.set_param(slot, param, plain);
                }
            });
        }
    }
}

/// Live input (plan 5c Task 8; spec §6.1, §6.6, §12.1).
#[uniffi::export]
impl Session {
    /// Each device input channel's peak since the last call, linear (plan
    /// 5d-1 Ruling 27; spec §12.1 `state.get input` meters): empty unless
    /// this document holds the engine with its input open.
    pub fn input_levels(&self) -> Result<Vec<f32>, VizError> {
        ffi_guard(|| Ok(host::input_levels(self.id)))
    }

    /// What the audio input offers while this document holds the engine
    /// (nothing before its first Play or Record).
    pub fn input_info(&self) -> Result<crate::live::InputInfoRow, VizError> {
        ffi_guard(|| {
            let counters = host::counters(self.id);
            Ok(match host::input_info(self.id) {
                Some((device, i)) => crate::live::InputInfoRow {
                    device,
                    channels: i.channels,
                    round_trip_frames: i.round_trip_frames(),
                    names: i.names,
                    input_latency_frames: i.input_latency_frames,
                    output_latency_frames: i.output_latency_frames,
                    buffer_frames: i.buffer_frames,
                    opening: host::input_opening(self.id),
                    sample_rate: host::device(self.id).map_or(0.0, |(_, r)| r),
                    midi_fx_dropped: counters.map_or(0, |c| c.midi_fx_dropped),
                    per_note_dropped: counters.map_or(0, |c| c.per_note_dropped),
                    mpe_stolen: counters.map_or(0, |c| c.mpe_stolen),
                },
                None => crate::live::InputInfoRow {
                    device: String::new(),
                    channels: 0,
                    names: Vec::new(),
                    input_latency_frames: 0,
                    output_latency_frames: 0,
                    buffer_frames: 0,
                    round_trip_frames: 0,
                    opening: false,
                    sample_rate: 0.0,
                    midi_fx_dropped: 0,
                    per_note_dropped: 0,
                    mpe_stolen: 0,
                },
            })
        })
    }

    /// Feeds `bytes` (channel messages, one after another) to the engine
    /// as if source `port` sent them now (`midi.inject`): lanes listening
    /// to that source play them thru and, armed and recording, record
    /// them. True when something took them: a surface, a learn or a
    /// learned map (which then act in this window: it takes the surfaces'
    /// focus, as `surface.inject` does; final review N6), or the engine.
    /// False when nothing did, e.g. this document does not hold the engine
    /// and no surface or map takes them (final review T8 m7). `Model` for
    /// bytes that are not channel messages or a bad port name; `Busy` when
    /// the input queue is full.
    pub fn inject_midi(&self, port: String, bytes: Vec<u8>) -> Result<bool, VizError> {
        ffi_guard(|| {
            let parsed = crate::live::parse_injection(&port, &bytes)?;
            // Surfaces, MIDI learn and learned maps hear injected bytes
            // too (plan 5f-1 Ruling 16), serviced by the session's idle;
            // what they take stays out of the lanes (Ruling 17).
            // This window's own document decides (final re-review): when
            // a surface, a learn, a command map or one of its maps hears the
            // port, it takes the focus first, so the bytes act here.
            if crate::surfaces::would_listen(self.id, &port) && self.focus_here() {
                // The units show this window before the bytes are routed
                // (final review N1).
                self.feed_surfaces(crate::surfaces::now(), &[]);
            }
            let queued = crate::surfaces::inject(&port, &bytes);
            let key = core_engine::input::port_key(&port);
            let count = parsed.len();
            let msgs: Vec<_> = parsed
                .into_iter()
                .filter(|m| !crate::surfaces::takes(key, m.as_slice()))
                .collect();
            // Taken: a surface port's every message, a learn's or a map's.
            let learnt = queued && msgs.len() < count;
            if msgs.is_empty() {
                return Ok(learnt);
            }
            let Some(tx) = host::midi_sender(self.id) else {
                return Ok(learnt);
            };
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
                ..
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
            let xfades = crate::overviews::crossfades_over(&doc, core_model::ids::EventId(event));
            let gain = |c: usize| -> f32 {
                let Some(Some((a, b))) = spans.get(c) else {
                    return 0.0;
                };
                let r = (a + b) / 2 - source_offset.min((a + b) / 2);
                lin * crate::overviews::fade_gain(r, length, fade_in, fade_out, &xfades)
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

    /// The gain the engine plays across a region, one value per column
    /// (plan 7c Ruling 4): the region gain times its fades at the centre
    /// of each of `columns` equal slices of media frames `[from_frame,
    /// to_frame)`, and 0 outside the region. Read-only; needs no file, so
    /// an offline region answers too.
    pub fn region_envelope(
        &self,
        event: u64,
        from_frame: u64,
        to_frame: u64,
        columns: u32,
    ) -> Result<Vec<f32>, VizError> {
        ffi_guard(|| {
            use core_model::event::EventBody;
            crate::overviews::check_columns(columns)?;
            if from_frame >= to_frame {
                return Err(crate::events::bad("from_frame must be before to_frame"));
            }
            let shape = {
                let inner = lock(&self.inner);
                let doc = inner.doc();
                let (ev, _) = doc.event(core_model::ids::EventId(event))?;
                let EventBody::AudioRegion {
                    media,
                    source_offset,
                    length,
                    gain_db,
                    fade_in,
                    fade_out,
                    ..
                } = ev.body
                else {
                    return Err(crate::events::bad(&format!(
                        "{event} is not an audio region"
                    )));
                };
                if !doc.media().contains_key(&media) {
                    return Err(crate::events::bad(&format!("unknown media {media}")));
                }
                crate::overviews::RegionShape {
                    source_offset,
                    length,
                    gain_db,
                    fade_in,
                    fade_out,
                    xfades: crate::overviews::crossfades_over(doc, core_model::ids::EventId(event)),
                }
            };
            Ok(crate::overviews::envelope(
                &shape, from_frame, to_frame, columns,
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
            let mut rows = crate::media::rows(inner.doc(), &inner.offline);
            // The overview's peak, once it is built (plan 5d-1 Ruling 21).
            for r in &mut rows {
                r.peak = inner
                    .overviews
                    .get(core_model::ids::MediaId(r.id))
                    .map(|ov| crate::overviews::peak_of(&ov));
            }
            Ok(rows)
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

    /// Starts a bounce (plan 5f-2; spec §9): the job ID; `job` reports it
    /// and `bounce_result` names what it wrote. Every refusal of the
    /// request is `Model`, with nothing written (Ruling 3); `Busy` inside
    /// an open edit, while another bounce of this session is unapplied,
    /// and with "Add to project" while a Format, Convert or Relink job is
    /// (Ruling 9).
    pub fn bounce(&self, req: crate::bounce::BounceRequestRow) -> Result<u64, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            let plan = self.bounce_plan(&inner, &req)?;
            if inner.history.in_transaction() {
                return Err(VizError::Busy {
                    message: "Finish the open edit first.".into(),
                });
            }
            self.apply_jobs_after_edit(&mut inner);
            if inner.jobs.pending(&[crate::jobs::JobKindRow::Bounce]) {
                return Err(VizError::Busy {
                    message: "A bounce is running.".into(),
                });
            }
            if inner
                .jobs
                .pending(&[crate::jobs::JobKindRow::ExportProject])
            {
                return Err(VizError::Busy {
                    message: "Wait for the export to finish.".into(),
                });
            }
            if req.add_to_project {
                inner.media_jobs_idle()?;
            }
            self.spawn_render(
                &mut inner,
                plan,
                crate::jobs::JobKindRow::Bounce,
                crate::bounce::run,
            )
        })
    }

    /// What `bounce` would do with `req`, checked as `bounce` checks it
    /// (nothing starts): the range's frames, the files in order (stems
    /// numbered), the lanes on external MIDI and Ruling 8's sentences.
    pub fn bounce_check(
        &self,
        req: crate::bounce::BounceRequestRow,
    ) -> Result<crate::bounce::BounceCheckRow, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            let plan = self.bounce_plan(&inner, &req)?;
            let root = inner.root(self.id);
            Ok(plan.check_row(
                crate::bounce::bounce_settings().tail_cap_ms,
                core_media::record::free_bytes(&root),
            ))
        })
    }

    /// The files and media bounce job `job` wrote (Ruling 10), applying it
    /// first when it just finished; `None` until it finished, or when it
    /// was cancelled or is no bounce.
    pub fn bounce_result(
        &self,
        job: u64,
    ) -> Result<Option<crate::bounce::BounceResultRow>, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            self.apply_jobs(&mut inner);
            Ok(inner.bounce_results.get(&job).cloned())
        })
    }
}

/// MIDI files (plan 6a): import and export jobs, their checks, the last
/// report and song info.
#[uniffi::export]
impl Session {
    /// What the import sheet shows for the file at `path` (read and parsed
    /// here, off the session lock; the app calls it off the main actor,
    /// F12): its kind, tracks, channels with their suggested destinations,
    /// tempo and MPE. `Model` for a path, file or content refused; `Io` for
    /// a failed read.
    pub fn midi_file_info(
        &self,
        path: String,
    ) -> Result<crate::interchange::MidiFileInfoRow, VizError> {
        ffi_guard(|| {
            let p = crate::interchange::file_path(&path, crate::interchange::MIDI_FILE)?;
            let imp = core_io::midi_in::parse_file(
                &p,
                &core_io::midi_in::ImportOptions::default(),
                &core_io::limits::Limits::default(),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .map_err(crate::interchange::io_error)?;
            Ok(crate::interchange::file_info(lock(&self.inner).doc(), &imp))
        })
    }

    /// Starts importing a MIDI file (plan 6a): the job ID; `job` reports it
    /// and applies it as one undo step "Import MIDI File" (as a new project,
    /// Ruling 6, the history is replaced instead: nothing to undo). Every refusal of the request is `Model` with
    /// nothing read; `Busy` while recording, inside an open edit, and while
    /// a bounce or another MIDI import or export is unapplied.
    pub fn import_midi(
        &self,
        req: crate::interchange::MidiImportRequestRow,
    ) -> Result<u64, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            // Finished jobs apply first, so the request is checked against
            // the document the job will land in.
            self.interchange_idle(&mut inner)?;
            let plan = crate::interchange::import_plan(inner.doc(), &req)?;
            inner.jobs.spawn(
                crate::jobs::JobKindRow::ImportMidi,
                crate::interchange::import_job(plan),
            )
        })
    }

    /// Starts a Standard MIDI File export of the document as it is now: the
    /// job ID. Refusals and `Busy` as `import_midi`; the file is written as
    /// a partial and renamed into place.
    pub fn export_midi(
        &self,
        req: crate::interchange::SmfExportRequestRow,
    ) -> Result<u64, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            self.interchange_idle(&mut inner)?;
            let opts = crate::interchange::smf_options(inner.doc(), &req)?;
            let dest =
                crate::interchange::file_dest(&req.path, req.replace, &self.export_roots(&inner))?;
            let job = crate::interchange::smf_job(inner.doc().clone(), opts, dest, req.replace);
            inner.jobs.spawn(crate::jobs::JobKindRow::ExportMidi, job)
        })
    }

    /// What `export_midi` would report (nothing is written; `path` and
    /// `replace` are not looked at, F2).
    pub fn export_midi_check(
        &self,
        req: crate::interchange::SmfExportRequestRow,
    ) -> Result<crate::interchange::ReportRow, VizError> {
        ffi_guard(|| {
            let (doc, opts) = {
                let inner = lock(&self.inner);
                let opts = crate::interchange::smf_options(inner.doc(), &req)?;
                (inner.doc().clone(), opts)
            };
            crate::interchange::smf_check(&doc, &opts)
        })
    }

    /// Starts a MIDI Clip File export: one file per track (`path` is the
    /// file for one track, the folder for several, named `NN <track>.midi2`;
    /// every name is checked first). Refusals and `Busy` as `import_midi`.
    pub fn export_midi_clip(
        &self,
        req: crate::interchange::ClipExportRequestRow,
    ) -> Result<u64, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            self.interchange_idle(&mut inner)?;
            let plan = crate::interchange::clip_plan(inner.doc(), &req)?;
            let dests = crate::interchange::clip_dests(&plan, &req, &self.export_roots(&inner))?;
            let job = crate::interchange::clip_job(inner.doc().clone(), plan, dests, req.replace);
            inner.jobs.spawn(crate::jobs::JobKindRow::ExportMidi, job)
        })
    }

    /// What `export_midi_clip` would report (nothing is written; `path`
    /// and `replace` are not looked at, F2).
    pub fn export_midi_clip_check(
        &self,
        req: crate::interchange::ClipExportRequestRow,
    ) -> Result<crate::interchange::ReportRow, VizError> {
        ffi_guard(|| {
            let (doc, plan) = {
                let inner = lock(&self.inner);
                let plan = crate::interchange::clip_plan(inner.doc(), &req)?;
                (inner.doc().clone(), plan)
            };
            crate::interchange::clip_check(&doc, &plan)
        })
    }

    /// The files `export_midi_clip` would write for `req` (`path`: the file
    /// for one track, the folder for several) and which of them already
    /// exist, a link counting even when it dangles, as the export sees
    /// them: the app asks about those before passing `replace`. Nothing is
    /// written; the folder is looked at off the session lock. `Model` for
    /// an unknown sequence or track, no tracks, an empty, relative or
    /// otherwise refused path, or (several tracks) a path that is a file.
    pub fn export_midi_clip_names(
        &self,
        req: crate::interchange::ClipExportRequestRow,
    ) -> Result<crate::interchange::ClipNamesRow, VizError> {
        ffi_guard(|| {
            let plan = crate::interchange::clip_plan(lock(&self.inner).doc(), &req)?;
            crate::interchange::clip_names(&plan, &req)
        })
    }

    /// Starts an export for another DAW (plan 6b; spec §7.1, §7.6): the
    /// job ID. The job writes "<Song> for another DAW" (numbered when
    /// taken, Ruling 1) in the folder `req.path` and, with
    /// `add_to_project`, adds the stems as heard in one undo step "Add
    /// Stems"; `job` reports it, its report and the folder. `Model` at once
    /// for an unknown sequence, a strip that is unknown, listed twice or
    /// the master, a format refused, path text refused, and "Add" with no
    /// strips; every check of the file system runs in the job, which fails
    /// with its sentence (Ruling 8). `Busy` while recording, inside an open
    /// edit, while a bounce, a MIDI import or export or another such
    /// export is unapplied, and with "Add" while a media job is. Apart
    /// from the media refresh and the plug-in capture `bounce` also makes,
    /// nothing touches the disk before the job (F13).
    pub fn export_for_daw(&self, req: crate::for_daw::ForDawRequestRow) -> Result<u64, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            self.interchange_idle(&mut inner)?;
            if req.add_to_project {
                inner.media_jobs_idle()?;
            }
            inner.refresh_media(self.id, false);
            let c = crate::for_daw::check(inner.doc(), &req, true, &inner.offline)?;
            let parent = c.parent.ok_or_else(|| VizError::Model {
                message: "Choose where to export.".into(),
            })?;
            let roots = self.export_roots(&inner).to_vec();
            #[cfg(feature = "test-hooks")]
            let cancel_at = inner.for_daw_cancel_at.take();
            #[cfg(not(feature = "test-hooks"))]
            let cancel_at = None;
            let layout = c.layout;
            self.spawn_render(
                &mut inner,
                c.plan,
                crate::jobs::JobKindRow::ExportProject,
                move |bounce, progress, cancel| {
                    crate::for_daw::run(
                        crate::for_daw::ForDawJob {
                            bounce,
                            parent,
                            roots,
                            layout,
                            cancel_at,
                        },
                        progress,
                        cancel,
                    )
                },
            )
        })
    }

    /// What `export_for_daw` would do with `req`, checked as it checks it
    /// (Busy aside): "What won't transfer", the folder's name (numbered
    /// when `req.path` names a folder, looked at with lstat), the files,
    /// the strips, where the files start, the stems' frames and the
    /// bounce's warnings. Nothing is written; the folder is looked at and
    /// the MIDI report rendered off the session lock (F13).
    pub fn export_for_daw_check(
        &self,
        req: crate::for_daw::ForDawRequestRow,
    ) -> Result<crate::for_daw::ForDawCheckRow, VizError> {
        ffi_guard(|| {
            let (doc, c, roots, root) = {
                let mut inner = lock(&self.inner);
                inner.refresh_media(self.id, false);
                let c = crate::for_daw::check(inner.doc(), &req, false, &inner.offline)?;
                let roots = self.export_roots(&inner).to_vec();
                (inner.doc().clone(), c, roots, inner.root(self.id))
            };
            crate::for_daw::check_row(
                &doc,
                &c,
                &roots,
                crate::bounce::bounce_settings().tail_cap_ms,
                core_media::record::free_bytes(&root),
            )
        })
    }

    /// Starts a DAWproject export of sequence `req.seq` (plan 6d Task 9;
    /// spec §6): the job ID; `job` reports it (kind `ExportDawproject`),
    /// its report and the file. `Model` at once for an unknown sequence and
    /// a path refused (not absolute, inside the project, a folder, taken
    /// without `replace`, not ending in `.dawproject`, a name too long for
    /// the files made beside it); `Busy` as `export_midi`. Before the job
    /// it looks at the disk only for the destination's lstat, the media
    /// refresh and the plug-in capture (as `export_midi` and `bounce`
    /// do). A project with no ID gets one, with no undo step (Ruling 16).
    pub fn export_dawproject(
        &self,
        req: crate::dawproject::DawprojectExportRequestRow,
    ) -> Result<u64, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            self.interchange_idle(&mut inner)?;
            inner.media_jobs_idle()?;
            let mut opts = crate::dawproject::options(inner.doc(), &req)?;
            let dest = crate::dawproject::dest(&req.path, req.replace, &self.export_roots(&inner))?;
            inner.refresh_media(self.id, false);
            opts.left_out = inner.dawproject_left_out();
            let uncaptured = self.capture_for_export(&mut inner)?;
            let project_id = match inner.doc().settings().project_id.clone() {
                Some(id) => id,
                None => {
                    let id = crate::project_id::fresh();
                    inner.history.stamp_project_id(&id)?;
                    // The title shows Edited.
                    inner.news.doc_changed = true;
                    id
                }
            };
            let exported_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            #[cfg(feature = "test-hooks")]
            let (cancel_at, free) = (inner.dawproject_cancel_at.take(), inner.dawproject_free);
            #[cfg(not(feature = "test-hooks"))]
            let (cancel_at, free) = (None, None);
            let job = crate::dawproject::ExportJob {
                doc: inner.doc().clone(),
                opts,
                dest,
                replace: req.replace,
                root: inner.root(self.id),
                project_id,
                export_id: crate::project_id::fresh(),
                exported_at,
                uncaptured,
                cancel_at,
                free,
            };
            inner.jobs.spawn(
                crate::jobs::JobKindRow::ExportDawproject,
                crate::dawproject::job(job),
            )
        })
    }

    /// What `export_dawproject` would report for `req`, checked as it
    /// checks it (`Busy` and the path aside): the plug-ins are captured
    /// here, the report is made off the session lock. Nothing is written
    /// and no project ID is stamped. Offline and over-long audio get the
    /// job's lines (the same classification).
    ///
    /// On the plug-in host's thread every call captures each running
    /// plug-in's state into the history, as an export does; off it (the
    /// app's detached check) nothing is captured and the last look at the
    /// slots is used. A caller on the host's thread (Task 11's control
    /// channel) should run it off that thread, or accept a capture per
    /// call.
    pub fn export_dawproject_check(
        &self,
        req: crate::dawproject::DawprojectExportRequestRow,
    ) -> Result<crate::interchange::ReportRow, VizError> {
        ffi_guard(|| {
            let (doc, opts, uncaptured, will_capture) = {
                let mut inner = lock(&self.inner);
                let mut opts = crate::dawproject::options(inner.doc(), &req)?;
                inner.refresh_media(self.id, false);
                opts.left_out = inner.dawproject_left_out();
                let here = plugins::with(self.id, |p| p.is_some())?;
                let uncaptured = self.capture_for_export(&mut inner)?;
                // Off the host's thread the job captures what runs now.
                let will_capture = if here {
                    std::collections::BTreeSet::new()
                } else {
                    inner.live_slots.clone()
                };
                (inner.doc().clone(), opts, uncaptured, will_capture)
            };
            let report = crate::dawproject::check(&doc, &opts, &uncaptured, &will_capture)?;
            Ok(crate::interchange::report_row(&report, "Exported"))
        })
    }

    /// Starts importing the DAWproject file `req.path` into this project,
    /// which must have no tracks (plan 6d Task 10; Ruling 17: the app makes
    /// a new project at `dawproject_info`'s rate first): the job ID; `job`
    /// reports it (kind `ImportProject`) and its report, and applies it
    /// with nothing to undo. `Model` at once for path text refused and a
    /// project with a track; `Busy` as `export_dawproject`, and while a
    /// media job is unapplied. Nothing is read before the job: every
    /// refusal of the file fails the job with the reader's sentence.
    pub fn import_dawproject(
        &self,
        req: crate::dawproject::DawprojectImportRequestRow,
    ) -> Result<u64, VizError> {
        ffi_guard(|| {
            // The catalog's lock is not taken under the session's.
            let installed = crate::dawproject::installed();
            let mut inner = lock(&self.inner);
            self.interchange_idle(&mut inner)?;
            inner.media_jobs_idle()?;
            let path =
                crate::interchange::file_path(&req.path, crate::dawproject::DAWPROJECT_FILE)?;
            if inner
                .doc()
                .sequences()
                .values()
                .any(|s| !s.lanes.is_empty())
            {
                return Err(VizError::Model {
                    message: core_io::dawproject::import::NOT_NEW.into(),
                });
            }
            #[cfg(feature = "test-hooks")]
            let (cancel_at, free) = (inner.dawproject_cancel_at.take(), inner.dawproject_free);
            #[cfg(not(feature = "test-hooks"))]
            let (cancel_at, free) = (None, None);
            let job = crate::dawproject::ImportJob {
                path,
                root: inner.root(self.id),
                format: inner.doc().settings().format,
                reserved: inner.reserved_names(),
                installed,
                cancel_at,
                free,
            };
            inner.jobs.spawn(
                crate::jobs::JobKindRow::ImportProject,
                crate::dawproject::import_job(job),
            )
        })
    }

    /// Starts merging the returning DAWproject file `req.path` into this
    /// project (plan 6e Task 8; spec §7.8; Rulings 2, 12, 13): the job ID;
    /// `job` reports it (kind `MergeDawproject`) with its preview (`merge`)
    /// and, for Preview and Apply, its report. Preview changes nothing;
    /// Hold keeps the merged history and its media until `confirm_merge`
    /// or `cancel_merge`; Apply swaps it in at once, one undo step "Merge
    /// DAWproject". `Busy` while recording, inside an open edit, while an
    /// interchange or media job is unapplied, or while a merge is held.
    /// `Model` at once for path text refused, a `date` that is not
    /// "YYYY-MM-DD" and a `snapshot` that is not an export ID. Before the
    /// job it looks at the disk only for `file_path`'s lstat and the
    /// plug-in capture (as `export_dawproject` does); every refusal of the
    /// file fails the job with its sentence.
    pub fn merge_dawproject(
        &self,
        req: crate::dawproject::DawprojectMergeRequestRow,
    ) -> Result<u64, VizError> {
        ffi_guard(|| {
            // The catalog's lock is not taken under the session's.
            let installed = crate::dawproject::installed();
            let mut inner = lock(&self.inner);
            self.interchange_idle(&mut inner)?;
            inner.media_jobs_idle()?;
            let path =
                crate::interchange::file_path(&req.path, crate::dawproject::DAWPROJECT_FILE)?;
            crate::dawproject::check_date(&req.date)?;
            crate::dawproject::check_snapshot(req.snapshot.as_deref())?;
            // Ours' plug-in states are current (Ruling 10), with no step; a
            // capture that failed is said as a Save's is (review M6).
            let uncaptured = self.capture_for_export(&mut inner)?;
            for (slot, why) in uncaptured {
                if let Some(e) = why {
                    let line = format!("{}: state not saved ({e})", slot_name(inner.doc(), slot));
                    inner.news.problems.push(line);
                }
            }
            #[cfg(feature = "test-hooks")]
            let (cancel_at, free) = (inner.dawproject_cancel_at.take(), inner.dawproject_free);
            #[cfg(not(feature = "test-hooks"))]
            let (cancel_at, free) = (None, None);
            let job = crate::dawproject::MergeJob {
                io: crate::dawproject::ImportJob {
                    path,
                    root: inner.root(self.id),
                    format: inner.doc().settings().format,
                    reserved: inner.reserved_names(),
                    installed,
                    cancel_at,
                    free,
                },
                history: inner.history.clone(),
                base: crate::dawproject::MergeBase::of(&inner.history),
                req,
            };
            inner.jobs.spawn(
                crate::jobs::JobKindRow::MergeDawproject,
                crate::dawproject::merge_job(job),
            )
        })
    }

    /// Merge (Ruling 12): swaps in the held merge of job `job` when the
    /// project has not changed since its preview, and answers its report
    /// (also the interchange report). `Model` "no merge is waiting" for any
    /// other job; `Busy` while recording or inside an open edit (the hold
    /// stays); `Model` "The project changed after the preview; preview the
    /// merge again." when it changed (an undo step, or a plug-in capture
    /// with none), and the hold and its media are dropped.
    pub fn confirm_merge(&self, job: u64) -> Result<crate::interchange::ReportRow, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            // A Hold job that finished becomes the held merge first.
            self.apply_jobs_after_edit(&mut inner);
            if !inner.held_merge.as_ref().is_some_and(|h| h.job == job) {
                return Err(VizError::Model {
                    message: crate::dawproject::NO_MERGE_WAITING.into(),
                });
            }
            if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
                return Err(recording_busy());
            }
            if inner.history.in_transaction() || inner.edit_label.is_some() {
                return Err(edit_open_busy());
            }
            let Some(held) = inner.held_merge.take() else {
                return Err(VizError::Model {
                    message: crate::dawproject::NO_MERGE_WAITING.into(),
                });
            };
            let root = inner.root(self.id);
            let report = swap_merge(&mut inner, &root, held.merged, held.files)?;
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            inner.news.doc_changed = true;
            inner.news.media_changed = true;
            Ok(report)
        })
    }

    /// Cancel (Ruling 12): drops the held merge of job `job` and the media
    /// its job converted. `Model` "no merge is waiting" for any other job
    /// (a running merge is cancelled with `cancel_job`).
    pub fn cancel_merge(&self, job: u64) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            // A Hold job that finished becomes the held merge first.
            self.apply_jobs_after_edit(&mut inner);
            if !inner.held_merge.as_ref().is_some_and(|h| h.job == job) {
                return Err(VizError::Model {
                    message: crate::dawproject::NO_MERGE_WAITING.into(),
                });
            }
            drop_held_merge(&mut inner, self.id);
            Ok(())
        })
    }

    /// The project's snapshots (Ruling 13): newest first, at most 10, each
    /// with its export's ID, time and track count; a damaged one, or one
    /// of another project, is left out. Reads every snapshot file: call it
    /// off the main thread.
    pub fn dawproject_snapshots(&self) -> Result<Vec<crate::dawproject::SnapshotRow>, VizError> {
        ffi_guard(|| {
            let (root, id) = {
                let inner = lock(&self.inner);
                (
                    inner.root(self.id),
                    inner.doc().settings().project_id.clone(),
                )
            };
            Ok(crate::dawproject::snapshot_rows(&root, id.as_deref()))
        })
    }

    /// The project's ID (stamped by its first DAWproject export), if any.
    pub fn project_id(&self) -> Result<Option<String>, VizError> {
        ffi_guard(|| Ok(lock(&self.inner).doc().settings().project_id.clone()))
    }

    /// Whether the project keeps the snapshot of export `export_id` (Ruling
    /// 13's open match): one lstat; false for a malformed ID.
    pub fn has_snapshot(&self, export_id: String) -> Result<bool, VizError> {
        ffi_guard(|| {
            let root = lock(&self.inner).root(self.id);
            Ok(crate::dawproject::has_snapshot(&root, &export_id))
        })
    }

    /// The file name a DAWproject export of sequence `seq` suggests:
    /// "<Song>.dawproject" (the song title, else the sequence's name, made
    /// safe). `Model` for an unknown sequence.
    pub fn dawproject_name(&self, seq: u64) -> Result<String, VizError> {
        ffi_guard(|| {
            core_io::dawproject::export::file_name(lock(&self.inner).doc(), SeqId(seq))
                .map_err(crate::interchange::io_error)
        })
    }

    /// The report of this document's last MIDI import or export job, if any.
    pub fn interchange_report(&self) -> Result<Option<crate::interchange::ReportRow>, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            self.apply_jobs(&mut inner);
            Ok(inner.interchange_report.clone())
        })
    }

    /// The project's song info.
    pub fn song_info(&self) -> Result<crate::interchange::SongInfoRow, VizError> {
        ffi_guard(|| Ok((&lock(&self.inner).doc().settings().info).into()))
    }

    /// Sets the song info, one undo step; `Model` for a field the rules
    /// refuse (over 4 096 bytes, control characters).
    pub fn set_song_info(
        &self,
        info: crate::interchange::SongInfoRow,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Settings(SettingsCmd::SetInfo {
                info: info.into(),
            }))
        })
    }
}

impl Session {
    /// Where exports may not write: the media root and the scratch package.
    fn export_roots(&self, inner: &Inner) -> [PathBuf; 2] {
        [inner.root(self.id), crate::media::scratch_root(self.id)]
    }

    /// `Busy` while recording, inside an open edit, or while a bounce, a
    /// MIDI import or export, an export for another DAW or a DAWproject
    /// export or import is unapplied (finished jobs apply first).
    fn interchange_idle(&self, inner: &mut Inner) -> Result<(), VizError> {
        if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
            return Err(recording_busy());
        }
        if inner.history.in_transaction() {
            return Err(edit_open_busy());
        }
        self.apply_jobs_after_edit(inner);
        use crate::jobs::JobKindRow;
        if inner.jobs.pending(&[
            JobKindRow::Bounce,
            JobKindRow::ImportMidi,
            JobKindRow::ExportMidi,
            JobKindRow::ExportProject,
            JobKindRow::ExportDawproject,
            JobKindRow::ImportProject,
            JobKindRow::MergeDawproject,
        ]) {
            return Err(VizError::Busy {
                message: "Wait for the running import, export or bounce to finish.".into(),
            });
        }
        // A held merge counts as running (plan 6e Ruling 12).
        if inner.held_merge.is_some() {
            return Err(merge_held_busy());
        }
        Ok(())
    }
}

/// The external sample editor (plan 5f-3; spec §11).
#[uniffi::export]
impl Session {
    /// Open in External Editor (Ruling 2): writes region `event`'s span
    /// (no gain, no fades, no dither) as a WAV in the project's rate and
    /// depth to `Media/Edits/<name> edit <n>.wav` and keeps it as the
    /// region's copy while the project is open (Ruling 3). `Model` for an
    /// unknown event, a non-region or an offline region; `Busy` while a
    /// Format, Convert or Relink job is unapplied; `Io` when `Media/Edits`
    /// is a file or the copy cannot be written. The copy is written
    /// without the session lock (pre-flight M4); a region that changed
    /// meanwhile is refused and the file left to retention.
    pub fn open_external(&self, event: u64) -> Result<crate::external::ExternalEditRow, VizError> {
        use crate::jobs::JobKindRow;
        ffi_guard(|| {
            let ev = core_model::ids::EventId(event);
            let (source, start, frames, dir, stem, fmt, from) = {
                let mut inner = lock(&self.inner);
                if inner.closed {
                    return Err(crate::events::bad("this project was closed"));
                }
                if inner.jobs.pending(&[
                    JobKindRow::Format,
                    JobKindRow::Convert,
                    JobKindRow::Relink,
                    JobKindRow::ExportProject,
                ]) {
                    return Err(VizError::Busy {
                        message: "Wait for the media job to finish.".into(),
                    });
                }
                inner.refresh_media(self.id, false);
                let (media, start, frames) = crate::external::region_span(inner.doc(), ev)?;
                if inner.offline.contains_key(&media) {
                    return Err(crate::events::bad(crate::external::OFFLINE));
                }
                let format = inner.doc().settings().format;
                let root = inner.root(self.id);
                let item = inner
                    .doc()
                    .media()
                    .get(&media)
                    .ok_or_else(|| crate::events::bad(crate::external::OFFLINE))?;
                let name = crate::external::root_name(inner.doc(), media)
                    .unwrap_or_else(|| item.name.clone());
                let out = (
                    root.join(&item.path),
                    start,
                    frames,
                    root.join(crate::external::EDITS_DIR),
                    crate::external::safe_stem(&name),
                    core_media::export::ExportFormat {
                        container: core_media::export::Container::Wav,
                        depth: format.depth,
                        rate: format.sample_rate,
                        dither: false,
                        dither_stream: 0,
                    },
                    media,
                );
                inner.edits_writing += 1;
                out
            };
            let written = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::external::write_copy(&source, start, frames, &dir, &stem, &fmt)
            }));
            let mut inner = lock(&self.inner);
            inner.edits_writing = inner.edits_writing.saturating_sub(1);
            let written = match written {
                Ok(w) => w?,
                // Counted down first, so retention runs again; `ffi_guard`
                // turns the panic into `Internal` (fold check M9).
                Err(p) => {
                    drop(inner);
                    std::panic::resume_unwind(p)
                }
            };
            match crate::external::region_span(inner.doc(), ev) {
                // The same media and span (review M4).
                Ok(now) if now == (from, start, frames) && !inner.closed => {}
                _ => {
                    return Err(crate::events::bad(
                        "The region changed while its copy was written; try again.",
                    ));
                }
            }
            inner.edits.insert(
                ev,
                crate::external::EditCopy {
                    abs: written.clone(),
                    from,
                },
            );
            Ok(crate::external::ExternalEditRow {
                event,
                path: written.display().to_string(),
                from: from.0,
            })
        })
    }

    /// What Reload finds for region `event` (Ruling 7; pre-flight I10):
    /// its copy in this session; `Gone` when that copy is not on disk, or
    /// when there is none in this session and the region's media came from
    /// an external edit (the project was reopened) that no live copy here
    /// shares; `NoEdit` otherwise (final review M12).
    /// `Model` for an unknown event or a non-region.
    pub fn external_edit(
        &self,
        event: u64,
    ) -> Result<crate::external::ExternalEditStateRow, VizError> {
        use crate::external::{ExternalEditRow, ExternalEditStateRow};
        ffi_guard(|| {
            let ev = core_model::ids::EventId(event);
            let inner = lock(&self.inner);
            let (media, ..) = crate::external::region_span(inner.doc(), ev)?;
            if let Some(c) = inner.edits.get(&ev) {
                return Ok(
                    if std::fs::symlink_metadata(&c.abs).is_ok_and(|m| m.is_file()) {
                        ExternalEditStateRow::Copy {
                            row: ExternalEditRow {
                                event,
                                path: c.abs.display().to_string(),
                                from: c.from.0,
                            },
                        }
                    } else {
                        ExternalEditStateRow::Gone
                    },
                );
            }
            let origin = inner
                .doc()
                .media()
                .get(&media)
                .and_then(|m| match m.origin {
                    core_model::document::MediaOrigin::ExternalEdit { from } => Some(from),
                    _ => None,
                });
            // A region sharing an edit whose copy is live here (a split
            // part, a pasted duplicate) has no edit of its own (final
            // review M12).
            let sibling = inner.edits.iter().any(|(e, c)| {
                Some(c.from) == origin
                    || crate::external::region_span(inner.doc(), *e).is_ok_and(|(m, ..)| m == media)
            });
            Ok(if origin.is_some() && !sibling {
                ExternalEditStateRow::Gone
            } else {
                ExternalEditStateRow::NoEdit
            })
        })
    }

    /// Imports `path` — region `event`'s copy, nothing else — as an
    /// External Edit job (Ruling 6): the job ID; `job` reports it and
    /// applies it as one undo step "External Edit". `Model` for a path
    /// that is not the region's copy (relative, over
    /// [`crate::external::MAX_PATH_BYTES`], holding `..`, or naming another
    /// file; the same file under another spelling of its package, as after
    /// Save As, is the copy: final review I1), a
    /// region without a copy, a region that is gone (its copy is then
    /// forgotten), or a copy that is gone; `Busy` inside an open edit and
    /// while a Format, Convert, Relink, Bounce or External Edit job is
    /// unapplied (pre-flight I11).
    pub fn apply_external_edit(&self, event: u64, path: String) -> Result<u64, VizError> {
        use crate::external::{GONE, MAX_PATH_BYTES, NO_EDIT, REGION_DELETED};
        ffi_guard(|| {
            if path.len() > MAX_PATH_BYTES {
                return Err(crate::events::bad("the path is too long"));
            }
            let p = PathBuf::from(&path);
            if !p.is_absolute() {
                return Err(VizError::Model {
                    message: format!("{path} is not an absolute path"),
                });
            }
            let ev = core_model::ids::EventId(event);
            let mut inner = lock(&self.inner);
            if inner.history.in_transaction() {
                return Err(VizError::Busy {
                    message: "Finish the open edit first.".into(),
                });
            }
            self.apply_jobs_after_edit(&mut inner);
            let Some(copy) = inner.edits.get(&ev).cloned() else {
                return Err(crate::events::bad(NO_EDIT));
            };
            let on_disk = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file());
            // The copy under any spelling of its package (final review I1);
            // a copy that is gone says so below, whatever `path` was.
            if !crate::external::names_copy(&copy.abs, &p) && on_disk(&copy.abs) {
                return Err(VizError::Model {
                    message: format!("{path} is not this region's edit copy"),
                });
            }
            if crate::external::region_span(inner.doc(), ev).is_err() {
                inner.edits.remove(&ev);
                return Err(crate::events::bad(REGION_DELETED));
            }
            if !on_disk(&copy.abs) {
                return Err(crate::events::bad(GONE));
            }
            // Imported under the core's spelling.
            let p = copy.abs.clone();
            inner.media_jobs_idle()?;
            let root = inner.root(self.id);
            let format = inner.doc().settings().format;
            let reserved = inner.reserved_names();
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let from = copy.from;
            inner.jobs.spawn(
                crate::jobs::JobKindRow::ExternalEdit,
                move |progress, cancel| {
                    let importer = crate::media::importer();
                    let file = crate::relink::regular_file(&p).and_then(|()| {
                        core_media::import::import_reserved(
                            importer.as_ref(),
                            &p,
                            &root,
                            format,
                            &stem,
                            &reserved,
                            &mut |f| {
                                progress.store(f.to_bits(), std::sync::atomic::Ordering::Relaxed)
                            },
                            cancel,
                        )
                    });
                    match file {
                        Err(core_media::MediaError::Cancelled) => crate::jobs::Work::Cancelled,
                        file => crate::jobs::Work::ExternalEdit {
                            event: ev,
                            from,
                            file: file.map_err(|e| e.to_string()),
                        },
                    }
                },
            )
        })
    }
}

/// Applies a finished external edit (Ruling 6; pre-flight I3): one undo
/// step "External Edit" adding the new item (origin `ExternalEdit { from }`)
/// and pointing that region alone at it (offset 0, its length, fades
/// clamped). Failed — with what it imported removed — when the import
/// failed, when it made more than one item (a file wider than stereo),
/// when the region is gone (its copy is then forgotten, so the app stops
/// watching it; pre-flight M10), or when the model refuses.
fn apply_external_edit_work(
    inner: &mut Inner,
    root: &Path,
    id: u64,
    event: core_model::ids::EventId,
    from: core_model::ids::MediaId,
    file: Result<Vec<core_media::import::Imported>, String>,
) -> crate::jobs::JobRow {
    use crate::jobs::{JobKindRow, JobStateRow, row};
    use core_model::command::{AudioCmd, MediaCmd};
    let failed = |m: String| {
        row(
            id,
            JobKindRow::ExternalEdit,
            JobStateRow::Failed,
            Some(m),
            vec![],
            vec![],
        )
    };
    let items = match file {
        Ok(v) => v,
        Err(why) => return failed(why),
    };
    let keep = inner.kept();
    let remove = |items: &[core_media::import::Imported]| {
        for i in items {
            remove_unkept(root, &i.path, &keep);
        }
    };
    let item = match items.as_slice() {
        [one] => one.clone(),
        _ => {
            remove(&items);
            return failed(format!(
                "The edit has {} channels; regions hold one or two.",
                items.len()
            ));
        }
    };
    if crate::external::region_span(inner.doc(), event).is_err() {
        remove(&items);
        inner.edits.remove(&event);
        return failed(crate::external::REGION_DELETED.into());
    }
    let filter = ViewFilter::default();
    let h = &mut inner.history;
    if let Err(e) = h.begin("External Edit") {
        remove(&items);
        return failed(VizError::from(e).to_string());
    }
    let run = (|| -> Result<u64, VizError> {
        let r = h.execute(
            &Command::Media(MediaCmd::Add {
                path: item.path.clone(),
                sample_rate: item.sample_rate,
                channels: item.channels,
                length_frames: item.length_frames,
                content_hash: Some(item.content_hash.clone()),
                name: Some(item.name.clone()),
                origin: Some(core_model::document::MediaOrigin::ExternalEdit { from }),
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
        h.execute(
            &Command::Audio(AudioCmd::SetSource {
                event,
                media,
                source_offset: 0,
                length: item.length_frames,
            }),
            &filter,
        )?;
        Ok(media.0)
    })();
    let done = run.and_then(|media| h.commit().map(|()| media).map_err(VizError::from));
    match done {
        Ok(media) => row(
            id,
            JobKindRow::ExternalEdit,
            JobStateRow::Done,
            None,
            vec![],
            vec![media],
        ),
        Err(e) => {
            if h.in_transaction() {
                let _ = h.abort();
            }
            remove(&items);
            failed(e.to_string())
        }
    }
}

/// `doc` with `sent` in its slots' overlays (a bounce's copy of the live
/// state; plan 5f-2 Ruling 6). A value the model refuses is left out.
fn with_params(doc: Document, sent: Vec<(PluginInstanceId, Vec<(ParamId, f64)>)>) -> Document {
    if sent.is_empty() {
        return doc;
    }
    let mut h = History::new(doc);
    for (slot, params) in sent {
        let _ = h.execute(
            &Command::Mixer(MixerCmd::SetPluginParams { slot, params }),
            &ViewFilter::default(),
        );
    }
    h.doc().clone()
}

impl Session {
    /// Starts a render job of `kind` for `plan` (a bounce, or an export
    /// for another DAW whose `body` wraps one): the live plug-in states
    /// captured into a copy of the document, the plug-in copies started
    /// (when it renders anything, Ruling 6) and the test hooks taken.
    fn spawn_render(
        &self,
        inner: &mut Inner,
        plan: crate::bounce::Plan,
        kind: crate::jobs::JobKindRow,
        body: impl FnOnce(
            crate::bounce::Job,
            &std::sync::atomic::AtomicU32,
            &std::sync::atomic::AtomicBool,
        ) -> crate::jobs::Work
        + Send
        + 'static,
    ) -> Result<u64, VizError> {
        // The live states, as Save captures them (Ruling 6), with the
        // values the plugins were sent and the document does not have
        // yet (read first: a capture counts them as saved).
        // An automated or riding parameter keeps its static value, as a
        // capture keeps it (final review T4-M2; plan 5e-1 Ruling 15).
        let slots = mixer::slot_ids(inner.doc());
        let mut kept = core_model::automation::automated_targets(inner.doc());
        kept.extend(inner.riding_params());
        let sent: Vec<(PluginInstanceId, Vec<(ParamId, f64)>)> = plugins::with(self.id, |p| {
            p.map(|p| {
                slots
                    .iter()
                    .map(|id| {
                        let mut v = p.uncommitted(*id);
                        v.retain(|(param, _)| !kept.contains(&(*id, *param)));
                        (*id, v)
                    })
                    .filter(|(_, v)| !v.is_empty())
                    .collect()
            })
            .unwrap_or_default()
        })?;
        self.capture_all(inner)?;
        let doc = with_params(inner.doc().clone(), sent);
        let root = inner.root(self.id);
        let format = doc.settings().format;
        // MIDI files only render nothing: no copies.
        let copies = if mixer::slot_ids(&doc).is_empty() || plan.sources() == 0 {
            None
        } else {
            Some(plugins::start_bounce(
                self.id,
                &doc,
                f64::from(format.sample_rate),
                crate::bounce::BLOCK,
            )?)
        };
        let token = copies.as_ref().map(|c| c.token);
        #[cfg(feature = "test-hooks")]
        let (cancel_at, fail_import_at) = (
            inner.bounce_cancel_at.take(),
            inner
                .bounce_fail_import_at
                .take()
                .and_then(|k| usize::try_from(k).ok()),
        );
        #[cfg(not(feature = "test-hooks"))]
        let (cancel_at, fail_import_at) = (None, None);
        let job = crate::bounce::Job {
            problems: plan.offline_sentence().into_iter().collect(),
            plan,
            media: crate::media::files(&doc, &root, &inner.offline),
            doc,
            root,
            reserved: inner.reserved_names(),
            format,
            tail_cap_ms: crate::bounce::bounce_settings().tail_cap_ms,
            copies: copies.map(|c| crate::bounce::Copies {
                factory: c.factory,
                ready: c.ready,
                problems: c.problems,
            }),
            cancel_at,
            fail_import_at,
        };
        let spawned = inner
            .jobs
            .spawn(kind, move |progress, cancel| body(job, progress, cancel));
        match spawned {
            Ok(id) => {
                if let Some(t) = token {
                    inner.bounce_copies = Some((id, t));
                }
                Ok(id)
            }
            Err(e) => {
                if let Some(t) = token {
                    plugins::end_bounce(t);
                }
                Err(e)
            }
        }
    }

    /// `req` checked against the document and this session's media roots.
    fn bounce_plan(
        &self,
        inner: &Inner,
        req: &crate::bounce::BounceRequestRow,
    ) -> Result<crate::bounce::Plan, VizError> {
        let roots = [inner.root(self.id), crate::media::scratch_root(self.id)];
        crate::bounce::plan(inner.doc(), req, &roots, &inner.offline)
    }
}

impl Session {
    /// A media-pool edit as one undo step; refused while an edit (a drag)
    /// is open.
    fn pool_edit(&self, cmd: Command) -> Result<DocSnapshot, VizError> {
        if lock(&self.inner).edit_label.is_some() {
            return Err(edit_open_busy());
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
                return Err(edit_open_busy());
            }
            let users = core_model::command::media::regions_using(inner.doc(), id);
            let filter = ViewFilter::default();
            inner.history.begin("Remove Media")?;
            let run = (|| -> Result<Vec<Note>, VizError> {
                let mut notes = Vec::new();
                if !users.is_empty() {
                    let r = inner
                        .history
                        .execute(&Command::Event(EventCmd::Delete { events: users }), &filter)?;
                    merge_notes(&mut notes, notes_of(r));
                }
                let r = inner.history.execute(
                    &Command::Media(MediaCmd::Remove {
                        media: id,
                        replacement: None,
                    }),
                    &filter,
                )?;
                merge_notes(&mut notes, notes_of(r));
                Ok(notes)
            })();
            let notes = match run {
                Ok(notes) => {
                    inner.history.commit()?;
                    notes
                }
                Err(e) => {
                    inner.history.abort()?;
                    return Err(e);
                }
            };
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            inner.snapshot_with(note_lines(notes))
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

/// Control surfaces and MIDI learn (plan 5f-1 Rulings 7, 8, 11–13, 17, 18).
#[uniffi::export]
impl Session {
    /// Makes this session the one the surfaces drive: the app's command
    /// target (Ruling 11; the ticker calls it when the target changes).
    pub fn focus_surfaces(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            if self.focus_here() {
                // The units show this project before any input is routed
                // against the old one's strips (final review N1).
                self.feed_surfaces(crate::surfaces::now(), &[]);
            }
            Ok(())
        })
    }

    /// The app's selected strips (Ruling 12): the surfaces' select LEDs and
    /// the strip whose plugin Plug-in mode shows. Unknown IDs are ignored;
    /// more than 1024 strips is refused (`Model`).
    pub fn set_surface_selection(&self, strips: Vec<u64>) -> Result<(), VizError> {
        ffi_guard(|| {
            if strips.len() > MAX_SURFACE_SELECTION {
                return Err(crate::events::bad(&format!(
                    "at most {MAX_SURFACE_SELECTION} strips"
                )));
            }
            let mut inner = lock(&self.inner);
            let m = inner.doc().mixer();
            let keep: Vec<StripId> = strips
                .into_iter()
                .map(StripId)
                .filter(|s| m.strip(*s).is_ok())
                .collect();
            inner.surfaces.selection = keep;
            Ok(())
        })
    }

    /// The document's record count-in is on (`RecordModel.countInBars >
    /// 0`): a Kontrol lights its Count-in button by it (plan 5f-4 Ruling
    /// 16).
    pub fn set_surface_count_in(&self, on: bool) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).surfaces.count_in = on;
            Ok(())
        })
    }

    /// Feeds `bytes` to surface `surface` as if from its input, focuses
    /// this session and services it at once (`surface.inject`; Ruling 11).
    /// Several refused messages in one call count once in the unit's
    /// `ignored` (final review T8 m9).
    /// `Model` for no bytes, more than `MAX_INJECT_BYTES` or an unknown
    /// surface.
    pub fn inject_surface(&self, surface: u32, bytes: Vec<u8>) -> Result<(), VizError> {
        ffi_guard(|| {
            let max = crate::live::MAX_INJECT_BYTES;
            if bytes.is_empty() || bytes.len() > max {
                return Err(crate::events::bad(&format!(
                    "`bytes` must hold 1 to {max} bytes"
                )));
            }
            let unfed = crate::surfaces::inject_unit(surface, &bytes)?;
            let moved = self.focus_here();
            // A unit addresses strips only once feedback showed them
            // (Ruling 3): a window that just took the focus (none was key,
            // so the ticker focused nothing) or a unit never fed gets its
            // refresh before the bytes are routed (plan 5f-1 Task 11), and
            // only then (final review T11 m1).
            if moved || unfed {
                self.feed_surfaces(crate::surfaces::now(), &[]);
            }
            self.service_surfaces();
            Ok(())
        })
    }

    /// `state.get surfaces` for this window: with what feedback sent.
    pub fn surface_state(&self) -> Result<SurfaceStateRow, VizError> {
        ffi_guard(|| Ok(crate::surfaces::state_with_feedback()))
    }

    /// The surfaces' commands, selects, arms and jogs since the last call
    /// (Ruling 12), oldest first.
    pub fn take_surface_events(&self) -> Result<Vec<SurfaceEventRow>, VizError> {
        ffi_guard(|| Ok(std::mem::take(&mut lock(&self.inner).surfaces.events)))
    }

    /// Learns the next control for `target` into this project (Rulings 7,
    /// 11): this session decides it in its own `idle`. `Model` for a target
    /// that does not exist or a velocity scale.
    pub fn learn_target(&self, target: AutomationTargetRow) -> Result<(), VizError> {
        ffi_guard(|| {
            let t = automation_target_of(target);
            let text = {
                let inner = lock(&self.inner);
                if matches!(t, AutomationTarget::VelocityScale(_)) {
                    return Err(crate::events::bad("a velocity scale cannot be learned"));
                }
                if !core_model::automation::target_exists(inner.doc(), t) {
                    return Err(crate::events::bad("the learn target does not exist"));
                }
                crate::automation::lane_name(inner.doc(), t, None)
            };
            crate::surfaces::learn_target(self.id, t, text);
            Ok(())
        })
    }

    /// The project's learn maps.
    pub fn learn_maps(&self) -> Result<Vec<LearnMapRow>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let doc = inner.doc();
            Ok(doc
                .settings()
                .learn
                .iter()
                .map(|m| {
                    let missing = !core_model::automation::target_exists(doc, m.target);
                    LearnMapRow::from((m, missing))
                })
                .collect())
        })
    }

    /// Sets the mode of the map from `source`: an undo step "MIDI Learn".
    pub fn set_learn_mode(
        &self,
        source: LearnSourceRow,
        mode: LearnModeRow,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| self.change_learn(&source, |m| m.mode = mode.into()))
    }

    /// Ruling 18: a `SetLearn` of the changed map ("MIDI Learn").
    pub fn set_learn_pickup(
        &self,
        source: LearnSourceRow,
        on: bool,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| self.change_learn(&source, |m| m.pickup = on))
    }

    /// Removes the map from `source` ("Remove MIDI Learn").
    pub fn remove_learn(&self, source: LearnSourceRow) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let source = source.source()?;
            self.learn_edit(SettingsCmd::RemoveLearn {
                source: Some(source),
                target: None,
            })
        })
    }

    /// Every map to `target`, as one step (`RemoveLearn { target }`).
    pub fn unlearn_target(&self, target: AutomationTargetRow) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.learn_edit(SettingsCmd::RemoveLearn {
                source: None,
                target: Some(automation_target_of(target)),
            })
        })
    }
}

/// The most strips `set_surface_selection` takes.
const MAX_SURFACE_SELECTION: usize = 1024;

/// The most surface events kept for the app (it drains them every tick).
const MAX_SURFACE_EVENTS: usize = 1024;

/// A skip or wind this recent winds on from where it went (Ruling 5).
const WIND_RECENT: std::time::Duration = std::time::Duration::from_millis(250);

/// How long the focused slot's presets are read once (plan 5f-4 Ruling 7).
const PRESETS_FRESH: std::time::Duration = std::time::Duration::from_millis(500);

/// The mixer's strips in display order, master excluded (Ruling 4).
fn display_order(doc: &Document) -> Vec<StripId> {
    let m = doc.mixer();
    m.order()
        .iter()
        .copied()
        .filter(|id| *id != m.master() && m.strip(*id).is_ok())
        .collect()
}

/// Whether AudioPath strip `kind` is armed (Ruling 12): any lane of any
/// sequence playing through its path is.
fn path_armed(doc: &Document, kind: core_model::mixer::StripKind) -> bool {
    let core_model::mixer::StripKind::AudioPath(p) = kind else {
        return false;
    };
    doc.sequences()
        .values()
        .any(|s| s.lanes.iter().any(|l| l.audio_out == Some(p) && l.arm))
}

impl Session {
    /// Moves the position to `tick` (the body of [`Session::locate`], also
    /// the surfaces' skips and winds; plan 5f-1 Ruling 5).
    fn locate_in(&self, inner: &mut Inner, tick: i64) -> Result<(), VizError> {
        self.land_at(inner, tick, true)
    }

    /// [`Session::locate_in`]'s body. Lands an open pass and begins a new
    /// one at `tick` (plan 5e-1 Ruling 23); with `relocate` the transport
    /// moves to `tick` first. Without it (a hold's end, plan 7d review
    /// I1) the transport stays where the hold left it, so a key tap is
    /// one locate.
    fn land_at(&self, inner: &mut Inner, tick: i64, relocate: bool) -> Result<(), VizError> {
        if inner.recording.is_some() {
            return Err(recording_busy());
        }

        // A Locate while a pass writes lands it; the new position is a
        // new pass (plan 5e-1 Ruling 23).
        let writing = inner
            .writing
            .as_ref()
            .map(|w| (w.seq, w.handles.status.rolled()));
        self.finish_writing(inner);
        self.apply_pending_automation(inner);
        // This landed what a hold wound over (plan 7d ruling I4).
        inner.wind_hold = false;
        if relocate && !host::locate(self.id, Tick(tick))? {
            inner.cursor = Some(Tick(tick));
            inner.cursor_seq = inner.selected_seq();
        }
        // Again from the new position: when it plays, or when the old
        // pass had not rolled yet (check I-2: the Play is still on its
        // way to the renderer).
        let stopped = host::transport_stopped(self.id);
        if let Some((seq, _)) =
            writing.filter(|(_, rolled)| stopped == Some(false) || (!rolled && stopped.is_some()))
        {
            self.begin_writing(inner, seq, Tick(tick));
        }
        Ok(())
    }

    /// Makes this session the focused one, with its document's maps. True
    /// when the focus moved here.
    fn focus_here(&self) -> bool {
        let maps = lock(&self.inner).doc().settings().learn.clone();
        crate::surfaces::focus(self.id, maps)
    }

    /// The surfaces get this document's maps now (a learn edit; Ruling 17).
    fn sync_learn(&self) {
        let maps = lock(&self.inner).doc().settings().learn.clone();
        crate::surfaces::sync_maps(self.id, &maps);
    }

    /// A learn edit: one undo step, then the surfaces' maps follow.
    /// `Busy` while a static touch's step is open.
    fn learn_edit(&self, cmd: SettingsCmd) -> Result<DocSnapshot, VizError> {
        if lock(&self.inner).static_edit.is_some() {
            return Err(VizError::Busy {
                message: "Release the touched control first.".into(),
            });
        }
        let snap = self.execute(Command::Settings(cmd))?;
        self.sync_learn();
        Ok(snap)
    }

    /// The map from `source`, changed by `f`, as a `SetLearn` ("MIDI
    /// Learn"). `Model` for a bad source or one with no map.
    fn change_learn(
        &self,
        source: &LearnSourceRow,
        f: impl FnOnce(&mut core_model::learn::LearnMap),
    ) -> Result<DocSnapshot, VizError> {
        let source = source.source()?;
        let mut map = lock(&self.inner)
            .doc()
            .settings()
            .learn
            .iter()
            .find(|m| m.source == source)
            .cloned()
            .ok_or_else(|| {
                crate::events::bad(&format!(
                    "no MIDI control is learned from {}",
                    source.describe()
                ))
            })?;
        f(&mut map);
        self.learn_edit(SettingsCmd::SetLearn { map })
    }

    /// Folds one take of the engine's read-and-clear peaks into both meter
    /// holds (Ruling 13, pre-flight I4).
    fn take_meters(&self, inner: &mut Inner) {
        if let Some(l) = host::levels(self.id, inner.doc()) {
            inner.meters.fold(l, Instant::now());
        }
    }

    /// `idle`'s surfaces step (plan 5f-1 Task 8): every session takes the
    /// releases it is owed and decides a learn it asked for; the focused
    /// one also routes input, applies the actions and ticks, then sends
    /// feedback. Lock order: `inner`, then the registry in short calls.
    fn service_surfaces(&self) {
        let now = crate::surfaces::now();
        let mut moved = Vec::new();
        let focused = {
            let mut inner = lock(&self.inner);
            self.take_meters(&mut inner);
            let (owed, focused) = crate::surfaces::begin(self.id, now);
            for (h, t, idle) in owed {
                // An idle hold ends at its last value's time; a touch now
                // (final review T8 m6).
                let last = inner.surfaces.last_ns.remove(&(h, t));
                self.release_from(&mut inner, t, h, last.filter(|_| idle));
            }
            // Every session's maps are kept: the engine holder's stay out of
            // its lanes while it is not focused (ruling F-1).
            crate::surfaces::sync_maps(self.id, &inner.doc().settings().learn);
            if focused {
                let strips = display_order(inner.doc()).len();
                let acts = {
                    let shared: &Inner = &inner;
                    let current = |t| Some(self.surface_heard(shared, t, None));
                    crate::surfaces::route_and_tick(self.id, now, strips, &current)
                };
                self.apply_surface_acts(&mut inner, acts, now, &mut moved);
            }
            focused
        };
        self.decide_learn(now);
        if focused {
            self.feed_surfaces(now, &moved);
        }
    }

    /// Step 6: a learn this session asked for decides and becomes a
    /// `SetLearn` ("MIDI Learn", pick-up on; Rulings 7, 18). Not while an
    /// edit is open: the step would join it.
    fn decide_learn(&self, now: Instant) {
        if !crate::surfaces::learning_for(self.id) || lock(&self.inner).history.in_transaction() {
            return;
        }
        let Some(l) = crate::surfaces::decide(self.id, now) else {
            return;
        };
        let map = core_model::learn::LearnMap {
            source: l.source,
            target: l.target,
            mode: l.mode,
            pickup: true,
        };
        match self.execute(Command::Settings(SettingsCmd::SetLearn { map })) {
            Ok(_) => lock(&self.inner).news.doc_changed = true,
            Err(e) => {
                // Its control is no longer claimed (final review N4).
                crate::surfaces::learn_failed(self.id);
                lock(&self.inner)
                    .news
                    .problems
                    .push(format!("MIDI Learn: {e}"));
            }
        }
        self.sync_learn();
    }

    /// Step 4: applies the surfaces' and learn's actions; a refused one
    /// counts once per message in its unit's `ignored`. What a hand moved
    /// goes to `moved` (the partners' feedback).
    fn apply_surface_acts(
        &self,
        inner: &mut Inner,
        acts: Vec<crate::surfaces::Act>,
        now: Instant,
        moved: &mut Vec<crate::surfaces::Moved>,
    ) {
        use core_surface::{Phase, SurfaceAction as A};
        let mut refused = Vec::new();
        for a in acts {
            let ok = match a.action {
                A::Gesture(t, phase) => {
                    let ok = self
                        .gesture_held(inner, t, phase, a.holder, a.host_ns)
                        .is_ok();
                    let key = (a.holder, t);
                    match phase {
                        Phase::Release => {
                            inner.surfaces.last_ns.remove(&key);
                        }
                        Phase::Touch(Some(_)) | Phase::Value(_) if ok => {
                            let ns = host_time(a.host_ns);
                            inner.surfaces.last_ns.insert(key, ns);
                        }
                        _ => {}
                    }
                    if ok && phase != Phase::Release {
                        moved.push(crate::surfaces::Moved {
                            holder: a.holder,
                            target: t,
                            strip: core_model::automation::strip_of(inner.doc(), t),
                        });
                    }
                    ok
                }
                A::IdleRelease(t, at) => {
                    // Stamped at its last value's time (Ruling 2): the host
                    // time that value was applied at, else its age on the
                    // surfaces clock.
                    let ns = inner
                        .surfaces
                        .last_ns
                        .remove(&(a.holder, t))
                        .unwrap_or_else(|| {
                            let ago = now.saturating_duration_since(at).as_nanos();
                            host_time(None).saturating_sub(u64::try_from(ago).unwrap_or(u64::MAX))
                        });
                    self.release_from(inner, t, a.holder, Some(ns));
                    true
                }
                A::Skip(dir) => self.surface_skip(inner, dir, now),
                A::Wind(q) => self.surface_wind(inner, q, now),
                A::Select(s) => {
                    self.surface_event(inner, SurfaceEventRow::Select { strip: s.0 });
                    true
                }
                A::Arm(s) => {
                    let path = match inner.doc().mixer().strip(s).map(|x| x.kind) {
                        Ok(core_model::mixer::StripKind::AudioPath(p)) => Some(p.0),
                        _ => None,
                    };
                    self.surface_event(inner, SurfaceEventRow::Arm { strip: s.0, path });
                    true
                }
                A::Jog(detents) => {
                    self.surface_event(inner, SurfaceEventRow::Jog { detents });
                    true
                }
                A::Command(id) => {
                    let programmatic = a.programmatic;
                    self.surface_event(inner, SurfaceEventRow::Command { id, programmatic });
                    true
                }
                // The registry moved the bank already.
                A::Bank(_) | A::Channel(_) => true,
                A::PluginSlot(i) => self.surface_plugin_slot(inner, i),
                A::PluginPage(n) => self.surface_plugin_page(inner, n),
                A::Preset(dir) => self.surface_preset(inner, dir, now),
                A::Tempo { bpm, new_step } => self.surface_tempo(inner, a.unit, bpm, new_step),
                A::CountIn => {
                    self.surface_event(inner, SurfaceEventRow::CountIn);
                    true
                }
            };
            if !ok && let Some(u) = a.unit {
                refused.push((u, a.tag));
            }
        }
        crate::surfaces::refused(&refused);
    }

    /// Queues `e` for `take_surface_events` (the oldest go past the bound).
    fn surface_event(&self, inner: &mut Inner, e: SurfaceEventRow) {
        let events = &mut inner.surfaces.events;
        if events.len() >= MAX_SURFACE_EVENTS {
            events.remove(0);
        }
        events.push(e);
    }

    /// Where a skip or wind starts (Ruling 5): the tick the last one went
    /// to when it was less than 250 ms ago, else the transport's. `None`
    /// while recording or with no sequence in the engine.
    fn wind_base(&self, inner: &Inner, now: Instant) -> Option<(SeqId, i64)> {
        if inner.recording.is_some() {
            return None;
        }
        // The schedule's position, as a transport bar step starts from
        // (review I2), so a latency never holds a skip or wind back.
        let (seq, tick) = host::transport_position(self.id)?;
        let tick = match inner.surfaces.wind {
            Some((at, t)) if now.saturating_duration_since(at) < WIND_RECENT => t,
            _ => tick.0,
        };
        Some((seq, tick))
    }

    /// `Skip(±1)`: the previous/next marker of the transport's sequence,
    /// else the previous/next bar line of its meter map, never below 0.
    fn surface_skip(&self, inner: &mut Inner, dir: i32, now: Instant) -> bool {
        let Some((seq, tick)) = self.wind_base(inner, now) else {
            return false;
        };
        let Ok(s) = inner.doc().sequence(seq) else {
            return false;
        };
        let forward = dir > 0;
        let to = core_editor::transport::marker_step(s, Tick(tick), forward)
            .unwrap_or_else(|| core_editor::transport::bar_step(&s.meter, Tick(tick), forward));
        self.surface_locate(inner, to.0.max(0), now)
    }

    /// `Wind(q)`: `base + round(q × PPQ)`, in `0..=WIND_LIMIT`.
    fn surface_wind(&self, inner: &mut Inner, q: f64, now: Instant) -> bool {
        let Some((_, base)) = self.wind_base(inner, now) else {
            return false;
        };
        let Some(to) = core_editor::transport::wind_target(base, q) else {
            return false;
        };
        self.surface_locate(inner, to, now)
    }

    /// Where a transport bar step stands with no recent target: the
    /// transport's schedule position (not reduced by the graph latency,
    /// review I2), else the cursor of the sequence a play would start, as
    /// [`Session::surface_where`] chooses it.
    fn step_where(&self, inner: &Inner) -> Option<(SeqId, Tick)> {
        let doc = inner.doc();
        host::transport_position(self.id)
            .filter(|(s, _)| doc.sequence(*s).is_ok())
            .or_else(|| Some((inner.selected_seq()?, inner.cursor.unwrap_or(Tick::ZERO))))
    }

    /// Where a transport bar step or wind starts (plan 7d Ruling 3): the
    /// sequence of [`Session::step_where`], and the last step's or wind's
    /// (or locate's) target when under [`WIND_RECENT`] ago, else its tick.
    fn jump_base(&self, inner: &Inner, now: Instant) -> Result<(SeqId, i64), VizError> {
        if inner.recording.is_some() {
            return Err(recording_busy());
        }
        let (seq, tick) = self.step_where(inner).ok_or_else(|| VizError::Model {
            message: "select a sequence first".into(),
        })?;
        let tick = match inner.surfaces.wind {
            Some((at, t)) if now.saturating_duration_since(at) < WIND_RECENT => t,
            _ => tick.0,
        };
        Ok((seq, tick))
    }

    /// One bar on from [`Session::jump_base`], by its sequence's meter.
    fn bar_target(&self, inner: &Inner, forward: bool, now: Instant) -> Result<i64, VizError> {
        let (seq, base) = self.jump_base(inner, now)?;
        let s = inner.doc().sequence(seq)?;
        // Playing, a back step from just after a barline goes one bar
        // further (plan 7d final review I-1).
        let playing = host::transport_playing(self.id);
        Ok(core_editor::transport::bar_step_from(&s.meter, Tick(base), forward, playing).0)
    }

    /// A hold's move (plan 7d controller ruling I4): the engine's position
    /// only (the cursor with no engine), nothing landed; remembered as the
    /// next step's base, and the hold's end owed.
    fn wind_move(&self, inner: &mut Inner, tick: i64, now: Instant) -> Result<(), VizError> {
        if !host::locate(self.id, Tick(tick))? {
            inner.cursor = Some(Tick(tick));
            inner.cursor_seq = inner.selected_seq();
        }
        inner.surfaces.wind = Some((now, tick));
        inner.wind_hold = true;
        Ok(())
    }

    /// A surface's locate, remembered as the next wind's base.
    fn surface_locate(&self, inner: &mut Inner, tick: i64, now: Instant) -> bool {
        let ok = self.locate_in(inner, tick).is_ok();
        if ok {
            inner.surfaces.wind = Some((now, tick));
        }
        ok
    }

    /// Where a surface's position, tempo and tempo edit are (plan 5f-4
    /// Ruling 15; Task 4 review m2): the transport's sequence and tick,
    /// else the sequence a play would start (the cursor sequence while it
    /// still exists, else the first top-level one) at the cursor. One
    /// helper, so the keyboard edits exactly the tempo it shows.
    fn surface_where(&self, inner: &Inner) -> Option<(SeqId, Tick)> {
        let doc = inner.doc();
        host::position(self.id)
            .filter(|(s, _)| doc.sequence(*s).is_ok())
            .or_else(|| Some((inner.selected_seq()?, inner.cursor.unwrap_or(Tick::ZERO))))
    }

    /// `Tempo` (Ruling 15): the tempo point in effect at the playhead set to
    /// `bpm` (clamped to 5–999), a gesture's values amended into its one
    /// "Set Tempo" step. Refused while recording or finishing a pass,
    /// inside an open transaction or group, for a non-finite `bpm`, and
    /// with no sequence.
    fn surface_tempo(
        &self,
        inner: &mut Inner,
        unit: Option<crate::automation::SurfaceId>,
        bpm: f64,
        new_step: bool,
    ) -> bool {
        if inner.recording.is_some()
            || inner.finishing
            || inner.history.in_transaction()
            || inner.history.in_group()
            || !bpm.is_finite()
        {
            return false;
        }
        let Some((seq, tick)) = self.surface_where(inner) else {
            return false;
        };
        let Ok(s) = inner.doc().sequence(seq) else {
            return false;
        };
        let at = tick.max(Tick::ZERO);
        let mut points: Vec<TempoPoint> = s.tempo.points().iter().copied().collect();
        // The point in effect: the last at or before the playhead (the
        // first governs negative ticks).
        let k = points.iter().rposition(|p| p.tick <= at).unwrap_or(0);
        let Some(p) = points.get_mut(k) else {
            return false;
        };
        // In Bpm::MIN..=Bpm::MAX after the clamp, so the conversion is exact.
        let milli = (bpm * 1000.0)
            .round()
            .clamp(f64::from(Bpm::MIN.0), f64::from(Bpm::MAX.0));
        p.bpm = Bpm(milli as u32);
        let Ok(tempo) = TempoMap::new(points) else {
            return false;
        };
        if tempo == s.tempo {
            return true;
        }
        let cmd = Command::Sequence(SequenceCmd::SetTempo { seq, tempo });
        let filter = ViewFilter::default();
        // Only the same keyboard's gesture is amended (final review T7 m5).
        let amended = match (new_step, unit, inner.surfaces.tempo_step) {
            (false, Some(u), Some((from, serial))) => {
                if from == u {
                    inner.history.amend(&cmd, &filter, serial).ok().flatten()
                } else {
                    None
                }
            }
            _ => None,
        };
        // A tempo change moves overlaps in frames: what settle did to a
        // crossfade reaches the next `idle` as a problem.
        let report = match amended {
            Some(r) => r,
            None => {
                let Ok(r) = inner.history.execute(&cmd, &filter) else {
                    return false;
                };
                inner.surfaces.tempo_step = unit.map(|u| (u, inner.history.present_serial()));
                r
            }
        };
        inner.news.problems.extend(report.warnings);
        inner.version += 1;
        inner.news.doc_changed = true;
        inner.reload_host(self.id);
        self.mark(inner);
        true
    }

    /// The value a surface or map shows for `t` (Ruling 13): a hand's
    /// value while it holds a valued touch (what the engine plays a block
    /// later, so a partner follows at once), else the heard value. A Trim
    /// touch shows the heard value (its hand is an offset).
    fn surface_heard(
        &self,
        inner: &Inner,
        t: AutomationTarget,
        cached: Option<&(SeqId, Arc<crate::automation::Existing>)>,
    ) -> f64 {
        if let Some(x) = inner.touches.get(&t)
            && x.valued
            && !matches!(x.path, TouchPath::Write(WriteMode::Trim, _))
        {
            return x.value;
        }
        self.heard_from(inner, t, cached)
    }

    /// The playing sequence's flattened automation, cached per (document
    /// version, sequence) (Ruling 13); `None` while stopped.
    fn surface_existing(
        &self,
        inner: &mut Inner,
    ) -> Option<(SeqId, Arc<crate::automation::Existing>)> {
        if host::transport_stopped(self.id) != Some(false) {
            return None;
        }
        let (seq, _) = host::position(self.id)?;
        let v = inner.version;
        if let Some((ver, s, all)) = &inner.surfaces.existing
            && *ver == v
            && *s == seq
        {
            return Some((seq, Arc::clone(all)));
        }
        let all = Arc::new(crate::automation::existing_all(inner.doc(), seq));
        inner.surfaces.existing = Some((v, seq, Arc::clone(&all)));
        Some((seq, all))
    }

    /// Steps 7–8: when a unit or the maps are due, the MixerView of heard
    /// values (only for the strips the due units show), then feedback.
    fn feed_surfaces(&self, now: Instant, moved: &[crate::surfaces::Moved]) {
        let mut inner = lock(&self.inner);
        let order = display_order(inner.doc());
        let Some(due) = crate::surfaces::due(self.id, now, &order, moved) else {
            return;
        };
        let cached = self.surface_existing(&mut inner);
        let meters = inner.meters.take_surfaces();
        Self::sync_page_focus(&mut inner);
        if due.page {
            self.sync_presets(&mut inner, now);
        }
        let shared: &Inner = &inner;
        let view = self.surface_view(shared, &order, &due, cached.as_ref(), &meters);
        let current = |t| Some(self.surface_heard(shared, t, cached.as_ref()));
        crate::surfaces::feedback(self.id, now, &due, &view, &current);
    }

    /// The MixerView (Ruling 13): heard values of the strips `due` shows,
    /// the selection, the plugin of the first selected strip's first
    /// insert, the arm LEDs (Ruling 12), the meters and the transport.
    fn surface_view(
        &self,
        inner: &Inner,
        order: &[StripId],
        due: &crate::surfaces::Due,
        cached: Option<&(SeqId, Arc<crate::automation::Existing>)>,
        meters: &std::collections::BTreeMap<u64, (f32, f32)>,
    ) -> core_surface::MixerView {
        use core_model::automation::SendIndex;
        let doc = inner.doc();
        let m = doc.mixer();
        let heard = |t| self.surface_heard(inner, t, cached);
        let strip = |s: &core_model::mixer::Strip, shown: bool| {
            let id = s.id;
            let mut v = core_surface::StripView {
                id,
                name: s.name.clone(),
                fader: 0.0,
                pan: 0.5,
                send: None,
                mute: false,
                solo: false,
                arm: false,
                select: inner.surfaces.selection.contains(&id),
                mode: s.automation,
                meter: (0.0, 0.0),
                kind: crate::surfaces::track_kind(s.kind),
            };
            if shown {
                v.fader = heard(AutomationTarget::StripFader(id));
                v.pan = heard(AutomationTarget::StripPan(id));
                v.send = (!s.sends.is_empty())
                    .then(|| heard(AutomationTarget::SendLevel(id, SendIndex(0))));
                v.mute = heard(AutomationTarget::StripMute(id)) >= 0.5;
                v.solo = heard(AutomationTarget::StripSolo(id)) >= 0.5;
                v.arm = path_armed(doc, s.kind);
                v.meter = meters.get(&id.0).copied().unwrap_or((0.0, 0.0));
            }
            v
        };
        let strips = order
            .iter()
            .enumerate()
            .filter_map(|(k, id)| {
                m.strip(*id)
                    .ok()
                    .map(|s| strip(s, due.visible.contains(&k)))
            })
            .collect();
        let master = m.strip(m.master()).ok().map(|s| strip(s, true));
        let plugin = if due.plugin {
            self.surface_params(inner, cached)
        } else {
            Vec::new()
        };
        let (seq, tick) = self
            .surface_where(inner)
            .map_or((None, Tick::ZERO), |(s, t)| (Some(s), t));
        core_surface::MixerView {
            strips,
            master,
            plugin,
            position: seq.map_or_else(|| "1.1.0".to_string(), |s| position_bbu(doc, s, tick)),
            playing: host::transport_stopped(self.id) == Some(false),
            recording: inner.recording.is_some(),
            looping: inner.looped,
            // With no transport or cursor sequence yet, the sequence a
            // play would start (the demo shows 120 before any play):
            // `surface_where`, which the keyboard's tempo edit uses too.
            tempo: seq
                .and_then(|q| doc.sequence(q).ok())
                .map_or(0.0, |q| q.tempo.bpm_at(tick)),
            soloing: m
                .strips()
                .keys()
                .any(|id| heard(AutomationTarget::StripSolo(*id)) >= 0.5),
            page: if due.page {
                self.surface_page(inner, cached)
            } else {
                None
            },
            count_in: inner.surfaces.count_in,
            automation_suspended: crate::automation::suspended(),
        }
    }

    /// The first 8 automatable parameters of the first selected strip's
    /// first chain plug-in (its EQ, else its first insert), by the
    /// plugin's list order, with heard values (Ruling 5).
    fn surface_params(
        &self,
        inner: &Inner,
        cached: Option<&(SeqId, Arc<crate::automation::Existing>)>,
    ) -> Vec<core_surface::ParamView> {
        let m = inner.doc().mixer();
        let Some(slot) = inner
            .surfaces
            .selection
            .iter()
            .find_map(|s| m.strip(*s).ok())
            .and_then(|s| s.chain().next().map(|i| i.id))
        else {
            return Vec::new();
        };
        self.slot_param_views(inner, slot, 0, cached).0
    }

    /// How many automatable, non-hidden parameters `slot` has (the MCU's
    /// filter); `None` when it is not running (plan 5f-4 Ruling 6).
    fn slot_param_count(&self, slot: PluginInstanceId) -> Option<usize> {
        plugins::with(self.id, |p| {
            p.and_then(|p| p.instance(slot)).map(|inst| {
                inst.info()
                    .params
                    .iter()
                    .filter(|i| i.automatable && !i.hidden)
                    .count()
            })
        })
        .ok()
        .flatten()
    }

    /// Parameters `skip..skip+8` of `slot` as the surfaces show them, and
    /// how many there are: the automatable, non-hidden ones in the plugin's
    /// list order, with heard values and their plain text (Rulings 5, 6).
    fn slot_param_views(
        &self,
        inner: &Inner,
        slot: PluginInstanceId,
        skip: usize,
        cached: Option<&(SeqId, Arc<crate::automation::Existing>)>,
    ) -> (Vec<core_surface::ParamView>, usize) {
        let (infos, n) = plugins::with(self.id, |p| {
            p.and_then(|p| p.instance(slot))
                .map(|inst| {
                    let all = inst
                        .info()
                        .params
                        .iter()
                        .filter(|i| i.automatable && !i.hidden);
                    let n = all.clone().count();
                    let infos = all
                        .skip(skip)
                        .take(8)
                        .map(|i| (i.id, i.name.clone(), i.min, i.max, i.stepped))
                        .collect::<Vec<_>>();
                    (infos, n)
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
        let views = infos
            .into_iter()
            .map(|(id, name, min, max, stepped)| {
                let target = AutomationTarget::PluginParam(slot, id);
                let value = self.surface_heard(inner, target, cached);
                let plain = core_model::automation::param_plain(value, min, max, stepped);
                core_surface::ParamView {
                    target,
                    name,
                    value,
                    text: format!("{plain:.2}"),
                }
            })
            .collect();
        (views, n)
    }

    /// Resets the focus when the first selected strip changed (Ruling 6):
    /// that strip, slot 0, page 0.
    fn sync_page_focus(inner: &mut Inner) {
        let first = {
            let m = inner.doc().mixer();
            inner
                .surfaces
                .selection
                .iter()
                .copied()
                .find(|s| m.strip(*s).is_ok())
        };
        if inner.surfaces.page.strip != first {
            inner.surfaces.page = crate::surfaces::PageFocus {
                strip: first,
                slot: 0,
                page: 0,
            };
        }
    }

    /// The focused chain and its slot (clamped to the chain), or `None`
    /// with no focused strip or an empty chain (Ruling 6).
    fn page_chain(inner: &Inner) -> Option<(PageChain, usize, PluginInstanceId)> {
        let f = inner.surfaces.page;
        let chain = crate::surfaces::chain(inner.doc(), f.strip?);
        let slot = f.slot.min(chain.len().checked_sub(1)?);
        let id = chain.get(slot)?.0;
        Some((chain, slot, id))
    }

    /// Ruling 6's page count: `min(127, max(1, ceil(n / 8)))`.
    fn page_count(n: usize) -> usize {
        n.div_ceil(8).clamp(1, 127)
    }

    /// The plug-in page a Kontrol in Plug-in mode shows (Ruling 6): the
    /// focused strip's chain, its slot's page of parameters; `None` with
    /// no selected strip, an empty chain or a plug-in that is not running.
    fn surface_page(
        &self,
        inner: &Inner,
        cached: Option<&(SeqId, Arc<crate::automation::Existing>)>,
    ) -> Option<core_surface::PluginPageView> {
        let (chain, slot, id) = Self::page_chain(inner)?;
        let pages = Self::page_count(self.slot_param_count(id)?);
        let page = inner.surfaces.page.page.min(pages.saturating_sub(1));
        let (params, _) = self.slot_param_views(inner, id, page.saturating_mul(8), cached);
        // From the cache `feed_surfaces` keeps (Ruling 7: no call per feedback).
        let shown = inner
            .surfaces
            .presets
            .as_ref()
            .filter(|(s, _, _)| *s == id)
            .map(|(_, p, _)| p);
        let preset = shown.and_then(|p| p.current);
        let presets = shown.map_or(0, |p| p.names.len());
        let preset_name = shown
            .and_then(|p| p.current.and_then(|c| p.names.get(c)))
            .cloned()
            .unwrap_or_default();
        Some(core_surface::PluginPageView {
            chain: chain.into_iter().map(|(_, name)| name).collect(),
            slot,
            params,
            page,
            pages,
            page_name: format!("Page {}", page.saturating_add(1)),
            preset,
            presets,
            preset_name,
        })
    }

    /// Keeps `inner.surfaces.presets` for the focused slot: read again when
    /// it is for another slot, older than [`PRESETS_FRESH`], or a parameter
    /// notice named the slot (Ruling 7: no preset call per feedback).
    fn sync_presets(&self, inner: &mut Inner, now: Instant) {
        let Some((_, _, id)) = Self::page_chain(inner) else {
            inner.surfaces.presets = None;
            return;
        };
        let fresh = matches!(
            &inner.surfaces.presets,
            Some((s, _, at)) if *s == id
                && now.saturating_duration_since(*at) < PRESETS_FRESH
                && !inner.news.params_changed.contains(&id.0)
        );
        if !fresh {
            inner.surfaces.presets = plugins::presets(self.id, id).map(|p| (id, p, now));
        }
    }

    /// `Preset(dir)`: steps the focused plug-in's presets without wrapping;
    /// from none, Next loads the first and Previous the last. Nothing to
    /// step, or a step to the one loaded, is an uncounted no-op; a plug-in
    /// not running or a failed load is refused (Ruling 7). A load is no
    /// undo step: a change made in the plug-in.
    fn surface_preset(&self, inner: &mut Inner, dir: i32, now: Instant) -> bool {
        Self::sync_page_focus(inner);
        let Some((_, _, id)) = Self::page_chain(inner) else {
            return false;
        };
        let Some(p) = plugins::presets(self.id, id) else {
            return false;
        };
        let Some(last) = p.names.len().checked_sub(1) else {
            return true;
        };
        let to = match (p.current, dir) {
            (_, 0) => return true,
            (None, d) if d > 0 => 0,
            (None, _) => last,
            (Some(c), d) => {
                let to = i64::try_from(c)
                    .unwrap_or(i64::MAX)
                    .saturating_add(i64::from(d))
                    .clamp(0, i64::try_from(last).unwrap_or(i64::MAX));
                usize::try_from(to).unwrap_or(last)
            }
        };
        if p.current == Some(to) {
            return true;
        }
        match plugins::load_preset(self.id, id, to) {
            Ok(()) => {
                inner.plugin_dirty = true;
                inner.news.doc_changed = true;
                inner.news.params_changed.insert(id.0);
                inner.surfaces.presets = plugins::presets(self.id, id).map(|p| (id, p, now));
                true
            }
            Err(e) => {
                inner.news.problems.push(format!("Preset: {e}"));
                false
            }
        }
    }

    /// `PluginSlot(i)`: slot `i` of the focused chain, page 0; refused
    /// past the chain, with no focused strip or a plug-in not running.
    fn surface_plugin_slot(&self, inner: &mut Inner, i: usize) -> bool {
        Self::sync_page_focus(inner);
        let Some(f) = inner.surfaces.page.strip else {
            return false;
        };
        let Some(id) = crate::surfaces::chain(inner.doc(), f).get(i).map(|x| x.0) else {
            return false;
        };
        if self.slot_param_count(id).is_none() {
            return false;
        }
        inner.surfaces.page.slot = i;
        inner.surfaces.page.page = 0;
        true
    }

    /// `PluginPage(n)`: the page moves by `n`; past either end nothing
    /// happens (uncounted). Refused with no focused strip, an empty chain
    /// or a plug-in not running.
    fn surface_plugin_page(&self, inner: &mut Inner, n: i32) -> bool {
        Self::sync_page_focus(inner);
        let Some((_, slot, id)) = Self::page_chain(inner) else {
            return false;
        };
        let Some(count) = self.slot_param_count(id) else {
            return false;
        };
        let pages = Self::page_count(count);
        let shown = inner.surfaces.page.page.min(pages.saturating_sub(1));
        let to = i64::try_from(shown)
            .unwrap_or(i64::MAX)
            .saturating_add(i64::from(n));
        inner.surfaces.page.slot = slot;
        inner.surfaces.page.page = shown;
        if let Ok(to) = usize::try_from(to)
            && to < pages
        {
            inner.surfaces.page.page = to;
        }
        true
    }
}

/// A chain's slots and names (plan 5f-4 Ruling 6).
type PageChain = Vec<(PluginInstanceId, String)>;

#[cfg(test)]
mod tests {
    use super::{normalize_warning, slot_name};
    use core_model::ids::EventId;

    /// Plan 7b Ruling 7: a message about an EQ names its strip and "EQ".
    #[test]
    fn an_eq_slot_is_named_after_its_strip() {
        use core_model::command::{Command, MixerCmd, apply};
        use core_model::document::Document;
        use core_model::ids::PluginInstanceId;
        use core_model::mixer::PluginRef;
        let d = Document::new();
        let m = d.mixer().master();
        let mut p = PluginRef::from_legacy("clap:eq");
        p.name = "Tone".into();
        let d = apply(
            &d,
            &Command::Mixer(MixerCmd::SetEq {
                strip: m,
                plugin: Some(p),
            }),
        )
        .unwrap()
        .doc;
        assert_eq!(slot_name(&d, PluginInstanceId(0)), "Master — EQ Tone");
    }

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
