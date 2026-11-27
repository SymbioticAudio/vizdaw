//! One open document (Swift: `DocumentModel`, the title, save and undo
//! parts; transport comes with Task 3).

mod block_drag;
mod editing;
mod recovery;

pub use recovery::{
    RECOVERY_FAILED, RECOVERY_IMPORT, RECOVERY_LATER, RECOVERY_TITLE, RECOVERY_TRASH,
    recovery_message,
};

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};

use vizdaw_ffi::VizError;
use vizdaw_ffi::blocks::BlockRow;
use vizdaw_ffi::edit::{EditCommand, LaneEdit, LaneRef};
use vizdaw_ffi::events::{EventRow, SequenceInfo};
use vizdaw_ffi::selection::{SelectionRow, TickRange};
use vizdaw_ffi::snapshot::{DocSnapshot, LaneRow, TransportState};

use crate::alerts::{AlertItem, error_message};
use crate::editor::{EditorModel, GESTURE_OPEN};
use crate::session::DocSession;

/// How many idle problem lines a document keeps (Swift
/// `DocumentModel.swift:854`).
pub const PROBLEMS_KEPT: usize = 50;

/// What File > Save does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveAction {
    Saved,
    /// Untitled: ask for a location; nothing was written.
    NeedsSaveAs,
    Failed(String),
    /// Refused before anything was written (an untitled project while
    /// recording); the status line says why.
    Refused(String),
}

/// What File › Save (or Save As) does now (Swift `SaveAction`): save in
/// place, ask for a location, or refuse with a reason. While recording it
/// never asks for a location: Save As and an untitled project's Save wait
/// for Stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveChoice {
    Save,
    Panel,
    Refused(String),
}

pub struct DocumentModel {
    pub(crate) session: Arc<dyn DocSession>,
    snapshot: DocSnapshot,
    /// The error showing, if any.
    pub alert: Option<AlertItem>,
    selected_seq: Option<u64>,
    lanes: Arc<[LaneRow]>,
    /// What the lanes were fetched for (Swift: `LanesKey`).
    lanes_key: Option<(u64, Option<u64>)>,
    lanes_fetches: u64,
    selection: SelectionRow,
    /// The selection's events, for `is_selected` per block per frame.
    selected_events: HashSet<u64>,
    pub(crate) status: Option<String>,
    /// Tool, saved layout, open drag.
    pub(crate) editor: EditorModel,
    looping: bool,
    /// The fields being typed in (Swift: `EditorModel.textEditors`); the
    /// bare transport keys wait while any is open.
    text_editors: BTreeSet<String>,
    /// Where playing started; the transport model shares it.
    play_start: Arc<Mutex<Option<i64>>>,
    /// Sequence info, read once per snapshot version.
    infos: Vec<(u64, OnceLock<Option<SequenceInfo>>)>,
    /// The last good transport read; the transport model feeds it too.
    transport: Arc<Mutex<TransportState>>,
    /// Each shown track's blocks, read once per lanes fetch.
    lane_reads: Vec<(u64, LaneReads)>,
    /// Any sequence's tracks and any track's events, per snapshot version.
    reads: Mutex<Reads>,
    /// The app's list preferences (default filter, columns), as last given.
    pub(crate) list_prefs: crate::list::ListPrefs,
    /// The list table has the keyboard (Swift `EditorModel.listFocused`).
    pub(crate) list_focused: bool,
    /// The mixer, made when first asked, and its strip selection.
    pub(crate) mixer: crate::mixer::DocMixer,
    /// The plug-in tick's news for the plug-in windows (host H2).
    pub(crate) plugin_news: crate::plugin_window::PluginNews,
    /// The jobs this document's UI started, as last read (slice 7a).
    pub(crate) jobs: Vec<vizdaw_ffi::JobRow>,
    /// A finished job's problems (the Problems banner).
    pub(crate) job_problems: Vec<String>,
    /// The session's idle problem lines (plug-in load failures, autosave,
    /// media reads, why a pass stopped by itself), newest last, at most
    /// [`PROBLEMS_KEPT`] (Swift `pluginProblems`).
    problems: Vec<String>,
    /// The Faults popover is open (Swift `showsFaults`).
    shows_faults: bool,
    /// The project format and mismatches, as last read.
    pub(crate) project_info: Option<vizdaw_ffi::ProjectInfoRow>,
    pub(crate) media_version: u64,
    /// Disk too slow was dismissed; a transport read without an underrun
    /// clears it (the transport model shares it).
    pub(crate) disk_warning_dismissed: Arc<AtomicBool>,
    /// The store made this project for a file it imports (an open): an
    /// import that fails or is cancelled closes it while untouched.
    pub(crate) opened_file: bool,
    /// That import failed (its message) or was cancelled (`None`); the
    /// store closes the project or shows the alert.
    pub(crate) opened_file_end: Option<Option<String>>,
    /// The Import MIDI File sheet, once its read landed (slice 7c).
    pub(crate) midi_import: Option<crate::interchange::MidiImport>,
    /// The Export MIDI File / MIDI Clip File sheet (slice 7c).
    pub(crate) midi_export: crate::interchange::MidiExport,
    /// The Recovered Recordings offer.
    recovery: recovery::RecoveryOffer,
    /// Recording: the options, the popover, the last record state.
    pub(crate) record: crate::record::RecordModel,
    /// The track inputs: picker sources, meters, the opening note.
    pub(crate) inputs: crate::lane_inputs::LaneInputs,
    /// The audio regions' waveforms (rows held, reads to run).
    pub(crate) waves: crate::region_waves::DocWaves,
    /// Step entry: its state and target (off: none).
    pub(crate) step: crate::step_entry::StepEntryModel,
    /// The Relink sheet's offer.
    pub(crate) relink: crate::relink::RelinkOffer,
    /// The Bounce sheet's state (slice 7b Task 6).
    pub(crate) bounce: crate::bounce::Bounce,
    /// The Export for Another DAW sheet's state (slice 7c Task 4).
    pub(crate) for_daw: crate::for_daw::ForDaw,
    /// The Export DAWproject sheet's state (slice 7c Task 4).
    pub(crate) dawproject: crate::dawproject::Dawproject,
    /// The Export AAF sheet's state (slice 7c Task 5).
    pub(crate) aaf_export: crate::aaf::AafExport,
    /// The Import AAF sheet, once its read landed (slice 7c Task 5).
    pub(crate) aaf_import: Option<crate::aaf::AafImport>,
    /// The report of the last import or export this document's UI ran
    /// (Swift `InterchangeModel.report`).
    pub(crate) last_report: Option<vizdaw_ffi::ReportRow>,
    /// That report waits to be shown (Swift `reportShown`).
    pub(crate) report_shown: bool,
    /// What the export that made `last_report` wrote (Swift `written`:
    /// the report sheet's Show in Finder); empty for an import or a
    /// report the session kept from before.
    pub(crate) report_written: Vec<String>,
    /// A DAWproject merge is held, waiting to be confirmed (Swift
    /// `DocumentModel.mergeHeld`; the merge sheet sets it, slice 7c).
    pub(crate) merge_held: bool,
    /// The Merge DAWproject sheet's state (slice 7c Task 6).
    pub(crate) merge: crate::merge::Merge,
    /// The media rows, with the media version they were read at.
    pub(crate) media_rows: Mutex<Option<(u64, Vec<vizdaw_ffi::MediaRow>)>>,
    /// Whether the saved project has Co-Producer copies, and when that
    /// was read.
    pub(crate) copies_read: Mutex<Option<(std::time::Instant, bool)>>,
    /// The jog detents waiting to be located by (slice 9d).
    pub(crate) surface_relay: crate::surface_relay::SurfaceRelay,
    /// What the surfaces asked for, for the window to apply in order.
    pub(crate) surface_steps: Vec<crate::surface_relay::RelayStep>,
    /// The count-in the core last took (`None`: not yet told).
    pub(crate) sent_count_in: Option<bool>,
    /// A surface's button is running (ruling T9-1): the keys a text field
    /// holds back act regardless.
    pub(crate) surface_acting: bool,
    /// MIDI learn: the banner, the maps and their sheet (slice 9d).
    pub learn: crate::learn::LearnModel,
    /// The Select and Modify panel (slice 10b).
    pub(crate) select_modify: crate::select_modify::model::SelectModifyModel,
}

/// What the arrangement reads of one track, read when first asked for.
#[derive(Default)]
struct LaneReads {
    blocks: OnceLock<Arc<[BlockRow]>>,
}

/// The `(seq, lane)` reads, each with the snapshot version it was read
/// at: an older one is read again, and is the answer while the core is
/// busy.
#[derive(Default)]
struct Reads {
    rows: HashMap<u64, Versioned<LaneRow>>,
    events: HashMap<(u64, u64), Versioned<EventRow>>,
}

/// Rows and the snapshot version they were read at.
type Versioned<T> = (u64, Arc<[T]>);

/// A read and whether it is `version`'s answer (`false`: a busy core's
/// older rows, to be read again).
pub(crate) struct Read<T> {
    pub(crate) rows: Arc<[T]>,
    pub(crate) fresh: bool,
}

/// `cache[key]` at `version`, else read by `read`. A busy core answers
/// with the older rows (or none), not fresh, read again next time;
/// another refusal is none for the rest of the version (Swift
/// `laneRowFailures`), so a failing read costs one call per version.
fn read_at<K: std::hash::Hash + Eq, T>(
    cache: &mut HashMap<K, Versioned<T>>,
    key: K,
    version: u64,
    read: impl FnOnce() -> Result<Vec<T>, VizError>,
) -> Read<T> {
    if let Some((v, rows)) = cache.get(&key)
        && *v == version
    {
        return Read {
            rows: rows.clone(),
            fresh: true,
        };
    }
    let rows: Arc<[T]> = match read() {
        Ok(rows) => rows.into(),
        Err(VizError::Busy { .. }) => {
            return Read {
                rows: cache
                    .get(&key)
                    .map_or_else(|| Arc::from([]), |(_, r)| r.clone()),
                fresh: false,
            };
        }
        Err(_) => Arc::from([]),
    };
    cache.insert(key, (version, rows.clone()));
    Read { rows, fresh: true }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl DocumentModel {
    pub fn new(session: Arc<dyn DocSession>) -> Result<Self, VizError> {
        let snapshot = session.snapshot()?;
        let transport = session
            .transport()
            .unwrap_or_else(|_| crate::transport::stopped());
        let selection = session.selection().unwrap_or_else(|_| empty_selection());
        // Read once; a missing or damaged `ui.json` is the default layout
        // (and stays on disk until the next save).
        let editor = EditorModel::new(session.ui_layout().ok().flatten().as_deref());
        let waves = crate::region_waves::DocWaves::new(session.clone());
        let learn = crate::learn::LearnModel::new(session.clone());
        let mut model = DocumentModel {
            waves,
            session,
            selected_seq: first_sequence(&snapshot),
            snapshot,
            alert: None,
            lanes: Arc::from([]),
            lanes_key: None,
            lanes_fetches: 0,
            selected_events: selection.events.iter().copied().collect(),
            selection,
            status: None,
            editor,
            looping: false,
            text_editors: BTreeSet::new(),
            play_start: Arc::new(Mutex::new(None)),
            infos: vec![],
            transport: Arc::new(Mutex::new(transport)),
            lane_reads: vec![],
            reads: Mutex::new(Reads::default()),
            list_prefs: crate::list::ListPrefs::default(),
            list_focused: false,
            mixer: crate::mixer::DocMixer::default(),
            plugin_news: crate::plugin_window::PluginNews::default(),
            jobs: vec![],
            job_problems: vec![],
            problems: vec![],
            shows_faults: false,
            project_info: None,
            media_version: 0,
            disk_warning_dismissed: Arc::new(AtomicBool::new(false)),
            opened_file: false,
            opened_file_end: None,
            midi_import: None,
            midi_export: crate::interchange::MidiExport::default(),
            recovery: recovery::RecoveryOffer::default(),
            record: crate::record::RecordModel::default(),
            inputs: crate::lane_inputs::LaneInputs::default(),
            step: crate::step_entry::StepEntryModel::default(),
            relink: crate::relink::RelinkOffer::default(),
            bounce: crate::bounce::Bounce::default(),
            for_daw: crate::for_daw::ForDaw::default(),
            dawproject: crate::dawproject::Dawproject::default(),
            aaf_export: crate::aaf::AafExport::default(),
            aaf_import: None,
            last_report: None,
            report_shown: false,
            report_written: vec![],
            merge_held: false,
            merge: crate::merge::Merge::default(),
            media_rows: Mutex::new(None),
            copies_read: Mutex::new(None),
            surface_relay: crate::surface_relay::SurfaceRelay::default(),
            surface_steps: vec![],
            sent_count_in: None,
            surface_acting: false,
            learn,
            select_modify: Default::default(),
        };
        model.refresh_project_info();
        // The time left reads the core's free space from the start.
        model.refresh_record(true);
        model.reset_infos();
        model.refresh_lanes();
        Ok(model)
    }

    /// Takes a new snapshot: keeps the shown sequence if it still exists
    /// (else the first top-level one) and refetches lanes if needed.
    pub fn apply(&mut self, s: DocSnapshot) {
        let moved = s.version != self.snapshot.version;
        if !s.edit_notes.is_empty() {
            self.status = Some(s.edit_notes.join(" "));
        }
        let gone = self
            .selected_seq
            .is_none_or(|id| !s.sequences.iter().any(|q| q.id == id));
        if gone {
            self.selected_seq = first_sequence(&s);
        }
        self.editor.prune(&s);
        {
            let reads = self.reads.get_mut().unwrap_or_else(|e| e.into_inner());
            reads
                .rows
                .retain(|seq, _| s.sequences.iter().any(|q| q.id == *seq));
            reads.events.retain(|(seq, lane), _| {
                s.lane_revs.iter().any(|r| r.seq == *seq && r.lane == *lane)
            });
        }
        if moved {
            self.waves.cache.document_changed();
            // Templates and Modify Again are read again for this version.
            self.select_modify.snapshot_moved();
        }
        self.snapshot = s;
        // A selected strip can go with no mixer showing (an Undo of Add
        // Bus): no refetch prunes the selection then.
        if moved && !self.selected_strips().is_empty() {
            let _ = self.selected_strips_that_exist();
        }
        self.reset_infos();
        self.refresh_lanes();
        // Step entry ends the moment its track is gone.
        self.check_step_target();
        self.after_sync();
    }

    fn reset_infos(&mut self) {
        self.infos = self
            .snapshot
            .sequences
            .iter()
            .map(|q| (q.id, OnceLock::new()))
            .collect();
    }

    /// What rulers and readouts need about `seq` (tempo, meter, markers),
    /// read once per snapshot version; `None` when the core refuses.
    pub fn sequence_info(&self, seq: u64) -> Option<&SequenceInfo> {
        let (_, cell) = self.infos.iter().find(|(id, _)| *id == seq)?;
        cell.get_or_init(|| self.session.sequence_info(seq).ok())
            .as_ref()
    }

    /// One plugin-timer tick: a drag's mouse-up or Escape that met Busy is
    /// retried first. True when a new snapshot arrived, the slots changed
    /// (the mixer reads them again) or a drag was pending.
    pub fn idle(&mut self) -> bool {
        let pending = self.gesture_pending() || self.has_pending_note_offs();
        self.tick();
        let (mut arrived, _) = self.take_idle_news();
        // The device rate moved (the transport says so first): the
        // banner reads the project info again.
        let mismatch = self.last_transport().rate_mismatch;
        if self
            .project_info
            .as_ref()
            .is_some_and(|i| i.rate_mismatch != mismatch)
        {
            self.refresh_project_info();
            arrived = true;
        }
        if !self.jobs.is_empty() {
            self.poll_jobs();
            arrived = true;
        }
        // A held Recovered Recordings offer shows again.
        if self.reoffer_if_held() {
            arrived = true;
        }
        // The count-in for the surfaces, and what they asked for.
        if self.surface_tick(std::time::Instant::now()) {
            arrived = true;
        }
        // A waiting learn's banner and the open maps sheet follow the core.
        if self.learn_tick() {
            arrived = true;
        }
        pending || arrived
    }

    /// The session's idle news taken in: a snapshot, the slots or the
    /// media changing, problem lines. True when something arrived. The
    /// new lines join the document's list (Faults shows them) and are
    /// also returned, for a caller that answers with its own.
    pub(crate) fn take_idle_news(&mut self) -> (bool, Vec<String>) {
        let mut arrived = false;
        let Ok(news) = self.session.idle() else {
            return (false, vec![]);
        };
        if news.slots_changed {
            self.mixer.slots_version += 1;
            arrived = true;
        }
        if let Some(s) = news.snapshot {
            self.apply(s);
            arrived = true;
        }
        if self
            .plugin_news
            .take(news.params_changed, news.editor_resize)
        {
            arrived = true;
        }
        if news.overviews_changed {
            self.waves.cache.overviews_changed();
            arrived = true;
        }
        if news.media_changed {
            self.media_version += 1;
            self.refresh_project_info();
            self.relink_due();
            arrived = true;
        }
        if !news.problems.is_empty() {
            self.problems.extend(news.problems.iter().cloned());
            let over = self.problems.len().saturating_sub(PROBLEMS_KEPT);
            self.problems.drain(..over);
            arrived = true;
        }
        (arrived, news.problems)
    }

    /// What the Faults popover lists: the transport's faults, its compile
    /// report, then the idle problem lines (Swift `messages`).
    pub fn fault_lines(&self) -> Vec<String> {
        let t = self.transport.lock().unwrap();
        t.faults
            .iter()
            .chain(&t.report)
            .chain(&self.problems)
            .cloned()
            .collect()
    }

    /// The transport has real-time faults (Faults takes the error tint).
    pub fn has_faults(&self) -> bool {
        !self.transport.lock().unwrap().faults.is_empty()
    }

    /// A compile report or problem lines (Faults shows, warning tint).
    pub fn has_report(&self) -> bool {
        !self.problems.is_empty() || !self.transport.lock().unwrap().report.is_empty()
    }

    /// Whether the Faults popover is open.
    pub fn shows_faults(&self) -> bool {
        self.shows_faults
    }

    pub fn set_shows_faults(&mut self, on: bool) {
        self.shows_faults = on;
    }

    /// The Faults button and its » entry.
    pub fn toggle_faults(&mut self) {
        self.shows_faults = !self.shows_faults;
    }

    /// The session's live answer; the snapshot's flag can lag. A session
    /// that cannot answer counts as unsaved.
    pub fn has_unsaved_changes(&self) -> bool {
        self.session.has_unsaved_changes().unwrap_or(true)
    }

    pub fn selected_seq(&self) -> Option<u64> {
        self.selected_seq
    }

    /// Shows another sequence (`None` for nothing).
    pub fn select(&mut self, seq: Option<u64>) {
        if seq == self.selected_seq {
            return;
        }
        self.selected_seq = seq;
        self.refresh_lanes();
        // Step entry ends the moment another sequence is shown.
        self.check_step_target();
    }

    /// The selection as the model holds it.
    pub fn selection(&self) -> &SelectionRow {
        &self.selection
    }

    pub fn lanes(&self) -> &[LaneRow] {
        &self.lanes
    }

    /// The track that path `path`'s strip records from and monitors: of
    /// the shown sequence's tracks playing through it, the first armed
    /// one, else the first (one rule for the mixer and the surfaces).
    pub fn input_lane(&self, path: u64) -> Option<&LaneRow> {
        let mut through = self.lanes.iter().filter(|l| l.audio_out == Some(path));
        let first = through.clone().next();
        through.find(|l| l.arm).or(first)
    }

    #[doc(hidden)]
    pub fn lanes_fetches(&self) -> u64 {
        self.lanes_fetches
    }

    /// The selected lanes of the shown sequence, in selection order.
    pub fn selected_lanes(&self) -> Vec<u64> {
        let Some(seq) = self.selected_seq else {
            return vec![];
        };
        self.selection
            .lanes
            .iter()
            .filter(|r| r.seq == seq)
            .map(|r| r.lane)
            .collect()
    }

    /// The first selected lane of the shown sequence that still exists.
    pub fn selected_lane(&self) -> Option<u64> {
        let first = *self.selected_lanes().first()?;
        self.lanes.iter().any(|l| l.id == first).then_some(first)
    }

    /// Selects a track; `extend` toggles it in the selection. The time
    /// range stays; selected events go.
    pub fn select_track(&mut self, lane: u64, extend: bool) {
        let Some(seq) = self.selected_seq else { return };
        let r = LaneRef { seq, lane };
        let mut lanes = vec![r];
        if extend {
            lanes = self
                .selection
                .lanes
                .iter()
                .filter(|l| l.seq == seq)
                .cloned()
                .collect();
            match lanes.iter().position(|l| *l == r) {
                Some(i) => {
                    lanes.remove(i);
                }
                None => lanes.push(r),
            }
        }
        self.set_selection(SelectionRow {
            time_range: self.selection.time_range,
            lanes,
            events: vec![],
        });
    }

    /// Sets the selection's time range (the Edit In and Out fields); the
    /// tracks and events stay. A failure is the status, as for
    /// [`DocumentModel::set_selection`].
    pub fn set_time_range(&mut self, range: TickRange) {
        self.set_selection(SelectionRow {
            time_range: Some(range),
            ..self.selection.clone()
        });
    }

    /// A ruler drag: the time range over every track of the shown
    /// sequence, no events (Swift `EditorModel.selectRange`).
    pub fn select_range(&mut self, range: TickRange) {
        let Some(seq) = self.selected_seq else { return };
        self.set_selection(SelectionRow {
            time_range: Some(range),
            lanes: self
                .lanes
                .iter()
                .map(|l| LaneRef { seq, lane: l.id })
                .collect(),
            events: vec![],
        });
    }

    /// Track `lane`'s arrangement blocks (under
    /// [`crate::arrangement::BLOCK_RULE`]), read once per snapshot; none
    /// while the core cannot answer (a refusal is read again next time).
    pub fn blocks(&self, lane: u64) -> Arc<[BlockRow]> {
        let Some((seq, reads)) = self.reads(lane) else {
            return Arc::from([]);
        };
        read_once(&reads.blocks, || {
            self.session
                .blocks(seq, lane, crate::arrangement::BLOCK_RULE)
        })
    }

    /// Track `lane` (of the shown sequence)'s events in time order: see
    /// [`DocumentModel::lane_events`]. None for a track it does not have.
    pub fn shown_lane_events(&self, lane: u64) -> Arc<[EventRow]> {
        let Some((seq, _)) = self.reads(lane) else {
            return Arc::from([]);
        };
        self.lane_events(seq, lane)
    }

    /// Sequence `seq`'s tracks (any sequence, not only the shown one),
    /// read once per snapshot version; none when the core refuses (a busy
    /// core: the last ones, asked again next time).
    pub fn lane_rows(&self, seq: u64) -> Arc<[LaneRow]> {
        self.lane_rows_read(seq).rows
    }

    /// Track `lane` of `seq`'s events in time order, read once per
    /// snapshot version; none when the core refuses (a busy core: the
    /// last ones, asked again next time).
    pub fn lane_events(&self, seq: u64, lane: u64) -> Arc<[EventRow]> {
        self.lane_events_read(seq, lane).rows
    }

    /// [`DocumentModel::lane_rows`] with whether it is this version's.
    pub(crate) fn lane_rows_read(&self, seq: u64) -> Read<LaneRow> {
        let version = self.snapshot.version;
        read_at(&mut lock(&self.reads).rows, seq, version, || {
            self.session.lanes(seq)
        })
    }

    /// [`DocumentModel::lane_events`] with whether it is this version's.
    pub(crate) fn lane_events_read(&self, seq: u64, lane: u64) -> Read<EventRow> {
        let version = self.snapshot.version;
        read_at(&mut lock(&self.reads).events, (seq, lane), version, || {
            self.session.lane_events(seq, lane).map(|e| e.events)
        })
    }

    fn reads(&self, lane: u64) -> Option<(u64, &LaneReads)> {
        let seq = self.selected_seq?;
        let (_, reads) = self.lane_reads.iter().find(|(id, _)| *id == lane)?;
        Some((seq, reads))
    }

    /// Reads the selection back (after edits, undo and redo).
    pub(crate) fn refresh_selection(&mut self) {
        if let Ok(s) = self.session.selection() {
            self.hold(s);
        }
    }

    /// Holds `s` as the selection.
    fn hold(&mut self, s: SelectionRow) {
        if s == self.selection {
            return;
        }
        self.selected_events = s.events.iter().copied().collect();
        self.selection = s;
    }

    /// Applies the result of an edit call: the snapshot is taken and the
    /// selection re-read; an error is handed back untouched (the command
    /// runner turns it into a control error).
    pub(crate) fn apply_result(
        &mut self,
        result: Result<DocSnapshot, VizError>,
    ) -> Result<(), VizError> {
        let snapshot = result?;
        self.apply(snapshot);
        self.refresh_selection();
        Ok(())
    }

    pub fn looping(&self) -> bool {
        self.looping
    }

    /// Turns the loop on or off; a failure is the alert "Loop Failed" and
    /// the loop stays as it was.
    pub fn toggle_loop(&mut self) {
        match self.session.set_loop(!self.looping) {
            Ok(()) => self.looping = !self.looping,
            Err(e) => self.alert = Some(AlertItem::from_error(&e, "Loop Failed")),
        }
    }

    /// Moves the transport to `tick` (a click in a ruler); a failure is the
    /// alert "Locate Failed".
    pub fn locate(&mut self, tick: i64) {
        if let Err(e) = self.session.locate(tick) {
            self.alert = Some(AlertItem::from_error(&e, "Locate Failed"));
        }
    }

    /// Moves the transport to `tick`; the error is the caller's to show
    /// (the bar's buttons report it in the status line).
    pub fn move_to(&mut self, tick: i64) -> Result<(), VizError> {
        self.session.locate(tick)
    }

    pub fn return_to_start(&mut self) {
        self.locate(0);
    }

    /// A field is being typed in; the bare transport keys wait.
    pub fn begin_text_editing(&mut self, owner: &str) {
        self.text_editors.insert(owner.to_owned());
    }

    pub fn end_text_editing(&mut self, owner: &str) {
        self.text_editors.remove(owner);
    }

    /// Every owner whose name starts with `prefix` gives the keyboard back
    /// (a panel that resigned key or closed).
    pub(crate) fn end_text_editing_prefix(&mut self, prefix: &str) {
        self.text_editors.retain(|o| !o.starts_with(prefix));
    }

    /// The fields typed in now, by owner name.
    pub fn text_owners(&self) -> Vec<String> {
        self.text_editors.iter().cloned().collect()
    }

    /// Space and the other bare transport keys work: no field is typed in
    /// and the Modify panel is not key (`M/DocumentModel.swift:582`). A
    /// surface's button is no key (ruling T9-1): it acts regardless.
    pub fn transport_keys_enabled(&self) -> bool {
        self.surface_acting || (self.text_editors.is_empty() && !self.select_modify.is_key())
    }

    /// Return (Return to Start) also waits while the list table has the
    /// keyboard; Space still plays.
    pub fn return_key_enabled(&self) -> bool {
        self.surface_acting || (self.transport_keys_enabled() && !self.list_focused)
    }

    /// Where the last Play started; Stop returns there.
    pub fn play_start(&self) -> Option<i64> {
        *self.play_start.lock().unwrap()
    }

    pub(crate) fn shared_play_start(&self) -> Arc<Mutex<Option<i64>>> {
        self.play_start.clone()
    }

    /// The last transport read that worked (kept while the core is busy,
    /// as Swift's `poll()` keeps it).
    pub fn last_transport(&self) -> TransportState {
        self.transport.lock().unwrap().clone()
    }

    pub(crate) fn shared_transport(&self) -> Arc<Mutex<TransportState>> {
        self.transport.clone()
    }

    pub(crate) fn shared_disk_warning(&self) -> Arc<AtomicBool> {
        self.disk_warning_dismissed.clone()
    }

    /// Reads the transport. A read that fails (Busy included) leaves the
    /// last good one in place, and is returned for the caller to judge.
    pub fn poll_transport(&self) -> Result<TransportState, VizError> {
        let t = self.session.transport()?;
        crate::banners::note_transport(&self.disk_warning_dismissed, &t);
        *self.transport.lock().unwrap() = t.clone();
        Ok(t)
    }

    /// Playing, as last read.
    pub fn is_playing(&self) -> bool {
        self.transport.lock().unwrap().playing
    }

    /// Plays the shown sequence from `from` (`None`: where the transport
    /// stands, or bar 1 at the end), up to `to` where it stops, and
    /// remembers where it started. The one start-playing path.
    pub fn start_playing(&self, from: Option<i64>, to: Option<i64>) -> Result<(), VizError> {
        let _ = self.poll_transport();
        let state = self.last_transport();
        let seq = self.selected_seq.ok_or_else(|| VizError::Model {
            message: "Select a sequence to play.".into(),
        })?;
        // The engine restarts at bar 1 from the end, or from another
        // sequence's position.
        let length = self
            .snapshot
            .sequences
            .iter()
            .find(|s| s.id == seq)
            .map_or(0, |s| s.length_ticks);
        let there = state.seq.is_none() || state.seq == Some(seq);
        let start = from.unwrap_or(if there && state.tick < length {
            state.tick
        } else {
            0
        });
        match from {
            Some(f) => self.session.play_from(seq, f, to)?,
            None => self.session.play(seq)?,
        }
        *self.play_start.lock().unwrap() = Some(start);
        let _ = self.poll_transport();
        Ok(())
    }

    /// Stops when playing, else plays the shown sequence (nothing with no
    /// sequence). On a failure: whether it was playing, and the error.
    pub(crate) fn try_play_stop(&self) -> Result<(), (bool, VizError)> {
        let _ = self.poll_transport();
        let playing = self.is_playing();
        let result = if playing {
            self.session.stop()
        } else if self.selected_seq.is_some() {
            self.start_playing(None, None)
        } else {
            Ok(())
        };
        let _ = self.poll_transport();
        result.map_err(|e| (playing, e))
    }

    /// Stops or plays. A failure is an alert, "Stop Failed" or "Play
    /// Failed".
    pub fn play_stop(&mut self) {
        if let Err((playing, e)) = self.try_play_stop() {
            let title = if playing {
                "Stop Failed"
            } else {
                "Play Failed"
            };
            self.alert = Some(AlertItem::from_error(&e, title));
        }
    }

    /// Stops and goes back to where playing started.
    pub fn stop_and_return(&self) -> Result<(), VizError> {
        stop_and_return(self.session.as_ref(), &self.play_start)?;
        let _ = self.poll_transport();
        Ok(())
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn set_status(&mut self, s: Option<String>) {
        self.status = s;
    }

    /// Renames a track of the shown sequence. A blank or unchanged name
    /// sends nothing.
    pub fn rename_track(&mut self, lane: u64, name: &str) {
        let Some(seq) = self.selected_seq else { return };
        let name = name.trim();
        let unchanged = self.lanes.iter().any(|l| l.id == lane && l.name == name);
        if name.is_empty() || unchanged {
            return;
        }
        let edit = LaneEdit::Rename {
            seq,
            lane,
            name: name.to_string(),
        };
        self.command("Rename Failed", |s| s.edit(EditCommand::Lane { edit }));
    }

    /// Sets the header flags; `None` leaves a flag as it is.
    pub fn set_flags(
        &mut self,
        lane: u64,
        mute: Option<bool>,
        solo: Option<bool>,
        arm: Option<bool>,
    ) {
        let Some(seq) = self.selected_seq else { return };
        let edit = LaneEdit::SetFlags {
            seq,
            lane,
            mute,
            solo,
            arm,
        };
        self.command("Flags Failed", |s| s.edit(EditCommand::Lane { edit }));
    }

    pub fn toggle_mute(&mut self, lane: u64) {
        let Some(seq) = self.selected_seq else { return };
        let Some(row) = self.lanes.iter().find(|l| l.id == lane) else {
            return;
        };
        let mute = !row.mute;
        self.command("Mute Failed", |s| s.set_lane_mute(seq, lane, mute));
    }

    /// Runs an edit: its snapshot is applied and the selection re-read; an
    /// error is the alert titled `title`, except `Busy`, which is the
    /// status text.
    fn command(
        &mut self,
        title: &str,
        body: impl FnOnce(&dyn DocSession) -> Result<DocSnapshot, VizError>,
    ) {
        match body(self.session.as_ref()) {
            Ok(snapshot) => {
                self.apply(snapshot);
                self.refresh_selection();
            }
            Err(VizError::Busy { message }) => self.status = Some(message),
            Err(e) => self.alert = Some(AlertItem::from_error(&e, title)),
        }
    }

    fn refresh_lanes(&mut self) {
        let key = (self.snapshot.version, self.selected_seq);
        if self.lanes_key == Some(key) {
            return;
        }
        self.lanes_key = Some(key);
        self.lane_reads.clear();
        let Some(seq) = self.selected_seq else {
            self.lanes = Arc::from([]);
            return;
        };
        self.lanes_fetches += 1;
        match self.session.lanes(seq) {
            Ok(rows) => {
                self.lanes = rows.into();
                let version = self.snapshot.version;
                lock(&self.reads)
                    .rows
                    .insert(seq, (version, self.lanes.clone()));
            }
            Err(e) => {
                self.lanes = Arc::from([]);
                self.alert = Some(AlertItem::from_error(&e, "Tracks Unavailable"));
            }
        }
        self.lane_reads = self
            .lanes
            .iter()
            .map(|l| (l.id, LaneReads::default()))
            .collect(); // The return badges' names, with the lanes (views never read them).
        self.warm_dest_names();
    }

    pub fn session(&self) -> &Arc<dyn DocSession> {
        &self.session
    }

    pub fn snapshot(&self) -> &DocSnapshot {
        &self.snapshot
    }

    /// The session's package path, read each time; `None` when untitled.
    pub fn path(&self) -> Option<PathBuf> {
        self.session.path().ok().flatten().map(PathBuf::from)
    }

    /// True when Save needs a path first (untitled).
    pub fn needs_save_as(&self) -> bool {
        self.path().is_none()
    }

    /// The window title: the name, marked when edited.
    pub fn title(&self) -> String {
        if self.snapshot.dirty {
            format!("{} \u{2014} Edited", self.snapshot.title)
        } else {
            self.snapshot.title.clone()
        }
    }

    pub fn undo_title(&self) -> String {
        label_title("Undo", &self.snapshot.undo_label)
    }

    pub fn redo_title(&self) -> String {
        label_title("Redo", &self.snapshot.redo_label)
    }

    pub fn can_undo(&self) -> bool {
        self.snapshot.undo_label.is_some()
    }

    pub fn can_redo(&self) -> bool {
        self.snapshot.redo_label.is_some()
    }

    /// Undo and Redo do nothing, silently, while the plugin host is busy;
    /// while a drag is open they are refused in the status line.
    pub fn undo(&mut self) {
        self.step("Undo Failed", |s| s.undo());
    }

    pub fn redo(&mut self) {
        self.step("Redo Failed", |s| s.redo());
    }

    fn step(
        &mut self,
        title: &str,
        body: impl FnOnce(&dyn DocSession) -> Result<DocSnapshot, VizError>,
    ) {
        if self.editor.gesture_open() {
            self.status = Some(GESTURE_OPEN.into());
            return;
        }
        match body(self.session.as_ref()) {
            Ok(snapshot) => {
                self.apply(snapshot);
                self.refresh_selection();
            }
            Err(VizError::Busy { .. }) => {}
            Err(e) => self.alert = Some(AlertItem::from_error(&e, title)),
        }
    }

    /// What File › Save (`force_as` false) or Save As does now. Reads the
    /// recording first (a pass can end by itself) and sets no status (the
    /// caller shows a refusal).
    pub fn save_choice(&mut self, force_as: bool) -> SaveChoice {
        self.refresh_record(true);
        let untitled = self.path().is_none();
        if self.record.is_recording() {
            if force_as {
                return SaveChoice::Refused(crate::record::SAVE_AS_WHILE_RECORDING.into());
            }
            if untitled {
                return SaveChoice::Refused(crate::record::SAVE_UNTITLED_WHILE_RECORDING.into());
            }
            return SaveChoice::Save;
        }
        if !(force_as || untitled) {
            return SaveChoice::Save;
        }
        // The core refuses Save As while a merge is held: the panel never
        // opens for it.
        match self.session.save_as_refusal() {
            Ok(Some(why)) => SaveChoice::Refused(why),
            _ => SaveChoice::Panel,
        }
    }

    /// Saves in place. An untitled document is never written: the caller
    /// asks for a location and calls `save_as` (refused while recording,
    /// with the status line saying why). A Save while recording says the
    /// take follows at Stop.
    pub fn save(&mut self) -> SaveAction {
        if self.path().is_none() {
            self.refresh_record(true);
            if self.record.is_recording() {
                let why = crate::record::SAVE_UNTITLED_WHILE_RECORDING.to_owned();
                self.status = Some(why.clone());
                return SaveAction::Refused(why);
            }
            return SaveAction::NeedsSaveAs;
        }
        self.store_layout();
        match self.session.save().and_then(|()| self.session.snapshot()) {
            Ok(snapshot) => {
                self.apply(snapshot);
                if self.save_after_pass_pending() {
                    self.status = Some(crate::record::SAVED_DURING_PASS.into());
                }
                SaveAction::Saved
            }
            Err(e) => {
                self.alert = Some(AlertItem::from_error(&e, "Save Failed"));
                SaveAction::Failed(error_message(&e))
            }
        }
    }

    /// A Save made while recording lands with the pass: closing then saves
    /// the song whatever the close prompt says.
    pub fn save_after_pass_pending(&self) -> bool {
        self.session.saves_after_pass().unwrap_or(false)
    }

    /// Don't Save: a Save made while recording is no longer owed, so the
    /// close that ends the recording saves nothing. A no-op when none is.
    pub fn cancel_save_after_pass(&self) {
        let _ = self.session.cancel_save_after_pass();
    }

    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
        self.store_layout();
        let result = self
            .session
            .save_as(path.to_string_lossy().into_owned())
            .and_then(|()| self.session.snapshot());
        match result {
            Ok(snapshot) => {
                self.apply(snapshot);
                Ok(())
            }
            Err(e) => {
                self.alert = Some(AlertItem::from_error(&e, "Save Failed"));
                Err(error_message(&e))
            }
        }
    }
}

/// `cell`'s rows (shared, not copied), read by `read` the first time it
/// answers.
fn read_once<T>(
    cell: &OnceLock<Arc<[T]>>,
    read: impl FnOnce() -> Result<Vec<T>, VizError>,
) -> Arc<[T]> {
    if let Some(rows) = cell.get() {
        return rows.clone();
    }
    match read() {
        Ok(rows) => cell.get_or_init(|| rows.into()).clone(),
        Err(_) => Arc::from([]),
    }
}

/// Stops and locates back to where playing started (the one copy).
pub(crate) fn stop_and_return(
    session: &dyn DocSession,
    play_start: &Mutex<Option<i64>>,
) -> Result<(), VizError> {
    session.stop()?;
    let at = *play_start.lock().unwrap();
    session.locate(at.unwrap_or(0))
}

fn label_title(verb: &str, label: &Option<String>) -> String {
    match label {
        Some(l) => format!("{verb} {l}"),
        None => verb.to_string(),
    }
}

fn first_sequence(s: &DocSnapshot) -> Option<u64> {
    s.sequences
        .iter()
        .find(|q| q.top_level)
        .or(s.sequences.first())
        .map(|q| q.id)
}

fn empty_selection() -> SelectionRow {
    SelectionRow {
        time_range: None,
        lanes: vec![],
        events: vec![],
    }
}
