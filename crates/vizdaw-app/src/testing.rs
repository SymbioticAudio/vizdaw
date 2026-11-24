//! A scripted `DocSession` (Swift: `FakeSession`): scripted errors, a busy
//! flag and a call log. Behind the `test-support` feature so the GPUI
//! crate's tests drive the same fake as this crate's.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use crate::DocSession;
use crate::session::CoreCalls;
use crate::session::IdleNews;
use std::sync::Arc;
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::{
    AutomationModeRow, AutomationStateRow, GestureHolderRow, GesturePhaseRow, StripModeRow,
};
use vizdaw_ffi::blocks::{BlockKind, BlockRow, BlockRule};
use vizdaw_ffi::control::ControlArg;
use vizdaw_ffi::edit::{EditCommand, LaneEdit, LaneRef, MonitorModeRow};
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::events::{
    EventKindRow, EventRow, LaneEvents, MarkerRow, MeterEntryRow, SequenceInfo, TempoPointRow,
};
use vizdaw_ffi::mixer::{
    BusRow, DestRow, GroupRow, MidiMixRow, MixerLevels, MixerSnapshot, StripColourRow,
    StripKindRow, StripRow, SwatchRow, VcaRow,
};
use vizdaw_ffi::record::RecoveredFileRow;
use vizdaw_ffi::revs::LaneRev;
use vizdaw_ffi::selection::{SelectionRow, TickRange};
use vizdaw_ffi::session::CopiesSizeRow;
use vizdaw_ffi::snapshot::{
    DocSnapshot, LaneRow, PluginFormatTag, PluginRefRow, SeqRow, SlotRow, SlotStatus,
    TransportState,
};
use vizdaw_ffi::tracks::TrackKindRow;
use vizdaw_ffi::{
    AafExportRequestRow, AafImportRequestRow, AafInfoRow, BitDepthRow, BounceCheckRow,
    BounceRequestRow, BounceResultRow, BounceSettingsRow, ClipExportRequestRow, ClipNamesRow,
    DawprojectExportRequestRow, DawprojectImportRequestRow, DawprojectInfoRow,
    DawprojectMergeRequestRow, ForDawCheckRow, ForDawRequestRow, ImportTargetRow, JobKindRow,
    JobRow, JobStateRow, MediaRow, MergePreviewRow, MidiFileInfoRow, MidiImportRequestRow,
    OpenOptions, ProjectFormatRow, ProjectInfoRow, RecoveryInfo, ReportRow, SmfExportRequestRow,
    SnapshotRow, SongInfoRow,
};

struct State {
    snap: DocSnapshot,
    next_snapshot: Option<DocSnapshot>,
    transport: TransportState,
    saved_path: Option<String>,
    play_error: Option<VizError>,
    transport_error: Option<VizError>,
    step_error: Option<VizError>,
    save_error: Option<VizError>,
    calls: Vec<String>,
    lanes: HashMap<u64, Vec<LaneRow>>,
    edits: Vec<EditCommand>,
    edit_error: Option<VizError>,
    unsaved: Option<Result<bool, VizError>>,
    idle_snapshot: Option<DocSnapshot>,
    selection: SelectionRow,
    span: Option<TickRange>,
    marker_ticks: Vec<i64>,
    infos: HashMap<u64, SequenceInfo>,
    blocks: HashMap<(u64, u64), Vec<BlockRow>>,
    events: HashMap<(u64, u64), Vec<EventRow>>,
    plays_from: Vec<(u64, i64, Option<i64>)>,
    step_bars: Vec<bool>,
    key_steps: Vec<bool>,
    winds: Vec<f64>,
    wind_attempts: usize,
    hold_ends: usize,
    jump_error: Option<VizError>,
    loop_error: Option<VizError>,
    looped: bool,
    info_reads: usize,
    locate_error: Option<VizError>,
    selection_error: Option<VizError>,
    keep_selection: Option<KeepSelection>,
    layout: Option<String>,
    /// The label of the edit `begin_edit` opened, while open.
    open_edit: Option<String>,
    gesture_applied: bool,
    gesture_edits: Vec<EditCommand>,
    edit_alls: Vec<(String, Vec<EditCommand>)>,
    /// Errors a call answers with every time, by call name.
    call_errors: HashMap<String, VizError>,
    /// Errors a call answers with once each, first, by call name.
    next_errors: HashMap<String, VecDeque<VizError>>,
    dests: Vec<DestRow>,
    lanes_reads: usize,
    event_reads: usize,
    auditions: Vec<(String, u8, u64, u16)>,
    /// What `audition` answers when it is not refused: false is "this
    /// session does not hold the engine".
    audition_sent: bool,
    mixer: MixerSnapshot,
    mixer_reads: usize,
    levels: MixerLevels,
    levels_reads: usize,
    /// Every gesture phase taken: target, phase, `alone`.
    gestures: Vec<(AutomationTargetRow, GesturePhaseRow, bool)>,
    /// The last value of each open touch, applied at its release.
    touch_values: HashMap<AutomationTargetRow, Option<f64>>,
    automation: AutomationStateRow,
    heard: HashMap<AutomationTargetRow, f64>,
    heard_calls: Vec<Vec<AutomationTargetRow>>,
    mode_calls: Vec<(Vec<u64>, AutomationModeRow)>,
    surface_selections: Vec<Vec<u64>>,
    run_mixers: Vec<(String, Vec<ControlArg>)>,
    /// The holder of each gesture phase taken, in order.
    holders: Vec<GestureHolderRow>,
    /// What `release_channel_touches` answers.
    channel_touches: u32,
    /// What the next `idle()` says of the slots (once).
    idle_slots_changed: bool,
    idle_media_changed: bool,
    /// The slots safe mode holds off.
    held: Vec<u64>,
    /// Every `load_plugins(slots)`.
    loaded: Vec<Vec<u64>>,
    /// Every `preview_midi_mix(dest, volume, pan)`.
    midi_previews: Vec<(u64, Option<u8>, Option<u8>)>,
    /// Every `set_midi_mix(dest, mix)`.
    midi_mixes: Vec<(u64, MidiMixRow)>,
    /// Files and jobs (slice 7a).
    files: Files,
}

/// How a scripted job ends.
// A test script: the size of the done variant does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum JobEnd {
    /// Applied: what it wrote, reported, created, previewed and bounced.
    Done {
        written: Vec<String>,
        report: Option<ReportRow>,
        created: Vec<u64>,
        merge: Option<MergePreviewRow>,
        bounce: Option<BounceResultRow>,
        /// Files that failed while the rest went on.
        problems: Vec<String>,
    },
    /// Nothing changed: why, and the files that failed.
    Failed {
        message: String,
        problems: Vec<String>,
    },
    Cancelled,
}

/// A job's script: its progress at each `job(id)` while running, then
/// its end (the poll after the last step answers the end).
#[derive(Debug, Clone, PartialEq)]
pub struct JobScript {
    pub steps: Vec<f32>,
    pub end: JobEnd,
}

impl JobScript {
    /// Done at the first poll, having done nothing visible.
    pub fn done() -> Self {
        JobScript {
            steps: vec![],
            end: JobEnd::Done {
                written: vec![],
                report: None,
                created: vec![],
                merge: None,
                bounce: None,
                problems: vec![],
            },
        }
    }

    pub fn failed(message: &str) -> Self {
        JobScript {
            steps: vec![],
            end: JobEnd::Failed {
                message: message.into(),
                problems: vec![],
            },
        }
    }

    pub fn cancelled() -> Self {
        JobScript {
            steps: vec![],
            end: JobEnd::Cancelled,
        }
    }

    /// Running at these progresses first, one per poll.
    pub fn steps(mut self, steps: &[f32]) -> Self {
        self.steps = steps.to_vec();
        self
    }

    fn done_mut(&mut self) -> Option<&mut JobEnd> {
        matches!(self.end, JobEnd::Done { .. }).then_some(&mut self.end)
    }

    /// A done job's written paths.
    pub fn written(mut self, paths: &[&str]) -> Self {
        if let Some(JobEnd::Done { written, .. }) = self.done_mut() {
            *written = paths.iter().map(|p| p.to_string()).collect();
        }
        self
    }

    /// A done job's report.
    pub fn report(mut self, r: ReportRow) -> Self {
        if let Some(JobEnd::Done { report, .. }) = self.done_mut() {
            *report = Some(r);
        }
        self
    }

    /// A done job's created media.
    pub fn created(mut self, ids: &[u64]) -> Self {
        if let Some(JobEnd::Done { created, .. }) = self.done_mut() {
            *created = ids.to_vec();
        }
        self
    }

    /// A done merge job's preview.
    pub fn merge(mut self, m: MergePreviewRow) -> Self {
        if let Some(JobEnd::Done { merge, .. }) = self.done_mut() {
            *merge = Some(m);
        }
        self
    }

    /// What a done bounce job's `bounce_result` answers.
    pub fn bounce(mut self, b: BounceResultRow) -> Self {
        if let Some(JobEnd::Done { bounce, .. }) = self.done_mut() {
            *bounce = Some(b);
        }
        self
    }

    /// The problems of a failed job (or of a done one: files that failed
    /// while the rest went on are on its row too).
    pub fn problems(mut self, list: &[&str]) -> Self {
        let list: Vec<String> = list.iter().map(|p| p.to_string()).collect();
        match &mut self.end {
            JobEnd::Failed { problems, .. } | JobEnd::Done { problems, .. } => *problems = list,
            JobEnd::Cancelled => {}
        }
        self
    }
}

/// What started a job, as given.
#[derive(Debug, Clone, PartialEq)]
pub enum JobRequest {
    Import {
        paths: Vec<String>,
        target: Option<ImportTargetRow>,
    },
    Convert,
    Format(ProjectFormatRow),
    Relink {
        media: u64,
        path: String,
    },
    Search(String),
    Bounce(BounceRequestRow),
    ImportMidi(MidiImportRequestRow),
    ExportMidi(SmfExportRequestRow),
    ExportMidiClip(ClipExportRequestRow),
    ForDaw(ForDawRequestRow),
    ExportDawproject(DawprojectExportRequestRow),
    ImportDawproject(DawprojectImportRequestRow),
    MergeDawproject(DawprojectMergeRequestRow),
    ExportAaf(AafExportRequestRow),
    ImportAaf(AafImportRequestRow),
}

/// A job the fake started.
#[derive(Debug, Clone, PartialEq)]
pub struct StartedJob {
    pub id: u64,
    pub kind: JobKindRow,
    pub request: JobRequest,
}

struct FakeJob {
    kind: JobKindRow,
    script: JobScript,
    /// Polls answered so far.
    polls: usize,
    cancelled: bool,
    /// `confirm_merge` or `cancel_merge` answered it.
    merge_closed: bool,
}

#[derive(Default)]
struct Files {
    scripts: VecDeque<JobScript>,
    jobs: Vec<(u64, FakeJob)>,
    started: Vec<StartedJob>,
    next_job: u64,
    cancelled: Vec<u64>,
    /// Reads by call name.
    reads: HashMap<String, usize>,
    media: Vec<MediaRow>,
    recheck: bool,
    project_info: Option<ProjectInfoRow>,
    bounce_check: Option<BounceCheckRow>,
    /// The dry-run reports by call name.
    reports: HashMap<String, ReportRow>,
    for_daw_check: Option<ForDawCheckRow>,
    midi_info: HashMap<String, MidiFileInfoRow>,
    clip_names: Option<ClipNamesRow>,
    snapshots: Vec<SnapshotRow>,
    merge_report: Option<ReportRow>,
    interchange_report: Option<ReportRow>,
    song_info: SongInfoRow,
    recovered: Vec<RecoveredFileRow>,
    copies: Option<CopiesSizeRow>,
    project_id: Option<String>,
    /// The exports whose snapshot the project keeps.
    snapshot_ids: Vec<String>,
}

/// What the session keeps of a selection it is given (after dropping
/// unknown lanes), as the core drops events that do not exist.
pub type KeepSelection = Box<dyn Fn(SelectionRow) -> SelectionRow + Send + Sync>;

/// One bar of 4/4, in ticks.
const BAR: i64 = 61_440;

pub struct FakeSession(Mutex<State>);

fn stopped() -> TransportState {
    TransportState {
        playing: false,
        seq: None,
        tick: 0,
        position_bbu: "1.1.0".into(),
        seconds: 0.0,
        sample_rate: 0.0,
        device: String::new(),
        faults: vec![],
        report: vec![],
        host_time_ns: 0,
        ticks_per_second: 30_720.0,
        rate_mismatch: false,
        counting_in: false,
        click: None,
    }
}

impl FakeSession {
    pub fn new(sequences: Vec<SeqRow>) -> Self {
        FakeSession(Mutex::new(State {
            snap: DocSnapshot {
                version: 0,
                step: 0,
                title: "Untitled".into(),
                dirty: false,
                undo_label: None,
                redo_label: None,
                sequences,
                warnings: vec![],
                lane_revs: vec![],
                edit_notes: vec![],
            },
            next_snapshot: None,
            transport: stopped(),
            saved_path: None,
            play_error: None,
            transport_error: None,
            step_error: None,
            save_error: None,
            calls: vec![],
            lanes: HashMap::new(),
            edits: vec![],
            edit_error: None,
            unsaved: None,
            idle_snapshot: None,
            selection: SelectionRow {
                time_range: None,
                lanes: vec![],
                events: vec![],
            },
            span: None,
            marker_ticks: vec![],
            infos: HashMap::new(),
            blocks: HashMap::new(),
            events: HashMap::new(),
            plays_from: vec![],
            step_bars: vec![],
            key_steps: vec![],
            winds: vec![],
            wind_attempts: 0,
            hold_ends: 0,
            jump_error: None,
            loop_error: None,
            looped: false,
            info_reads: 0,
            locate_error: None,
            selection_error: None,
            keep_selection: None,
            layout: None,
            open_edit: None,
            gesture_applied: false,
            gesture_edits: vec![],
            edit_alls: vec![],
            call_errors: HashMap::new(),
            next_errors: HashMap::new(),
            dests: vec![],
            lanes_reads: 0,
            event_reads: 0,
            auditions: vec![],
            audition_sent: true,
            mixer: MixerSnapshot {
                version: 0,
                strips: vec![],
                buses: vec![],
                groups: vec![],
                vcas: vec![],
            },
            mixer_reads: 0,
            levels: MixerLevels::empty(),
            levels_reads: 0,
            gestures: vec![],
            touch_values: HashMap::new(),
            automation: AutomationStateRow {
                modes: vec![],
                touched: vec![],
                touched_values: vec![],
                suspended: false,
                writing: false,
            },
            heard: HashMap::new(),
            heard_calls: vec![],
            mode_calls: vec![],
            surface_selections: vec![],
            run_mixers: vec![],
            holders: vec![],
            channel_touches: 0,
            idle_slots_changed: false,
            idle_media_changed: false,
            held: vec![],
            loaded: vec![],
            midi_previews: vec![],
            midi_mixes: vec![],
            files: Files {
                next_job: 1,
                ..Files::default()
            },
        }))
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap()
    }

    pub fn snap(&self) -> DocSnapshot {
        let mut s = self.st();
        Self::sync_revs(&mut s);
        s.snap.clone()
    }

    /// Makes the next command return `s`.
    pub fn bump(&self, s: DocSnapshot) {
        self.st().next_snapshot = Some(s);
    }

    /// Gives the current snapshot an undo step (`None`: nothing to undo).
    pub fn set_undo_label(&self, label: Option<&str>) {
        self.st().snap.undo_label = label.map(str::to_owned);
    }

    /// Marks the current snapshot edited (or clean).
    pub fn set_dirty(&self, dirty: bool) {
        self.st().snap.dirty = dirty;
    }

    pub fn set_step_error(&self, e: Option<VizError>) {
        self.st().step_error = e;
    }

    pub fn set_path(&self, p: Option<&str>) {
        self.st().saved_path = p.map(String::from);
    }

    pub fn set_save_error(&self, e: Option<VizError>) {
        self.st().save_error = e;
    }

    pub fn set_play_error(&self, e: Option<VizError>) {
        self.st().play_error = e;
    }

    pub fn set_transport_error(&self, e: Option<VizError>) {
        self.st().transport_error = e;
    }

    pub fn set_playing(&self, playing: bool) {
        self.st().transport.playing = playing;
    }

    /// Edits the scripted transport (what the next `transport()` reads).
    pub fn set_transport(&self, f: impl FnOnce(&mut TransportState)) {
        f(&mut self.st().transport);
    }

    /// Scripts the lanes `lanes(seq)` returns.
    pub fn set_lanes(&self, seq: u64, rows: Vec<LaneRow>) {
        self.st().lanes.insert(seq, rows);
    }

    /// What `has_unsaved_changes` answers; `None` follows the snapshot.
    pub fn set_unsaved(&self, v: Option<Result<bool, VizError>>) {
        self.st().unsaved = v;
    }

    /// The snapshot the next `idle()` returns (once).
    pub fn set_idle(&self, s: Option<DocSnapshot>) {
        self.st().idle_snapshot = s;
    }

    /// Makes the next `idle()` say the slots changed (once).
    pub fn set_idle_slots_changed(&self) {
        self.st().idle_slots_changed = true;
    }

    /// The next `idle` says the media rows changed.
    pub fn set_idle_media_changed(&self) {
        self.st().idle_media_changed = true;
    }

    /// Makes the session hold exactly `s` as its selection.
    pub fn force_selection(&self, s: SelectionRow) {
        self.st().selection = s;
    }

    pub fn set_edit_error(&self, e: Option<VizError>) {
        self.st().edit_error = e;
    }

    /// The selection's span `selection_span` answers.
    pub fn set_span(&self, span: Option<TickRange>) {
        self.st().span = span;
    }

    /// Where the markers are (any order).
    pub fn set_marker_ticks(&self, ticks: Vec<i64>) {
        self.st().marker_ticks = ticks;
    }

    /// Gives sequence `seq` a playing length (before the model reads it).
    pub fn set_length(&self, seq: u64, ticks: i64) {
        let mut s = self.st();
        if let Some(q) = s.snap.sequences.iter_mut().find(|q| q.id == seq) {
            q.length_ticks = ticks;
        }
    }

    /// Scripts what `sequence_info(seq)` answers.
    pub fn set_info(&self, seq: u64, info: SequenceInfo) {
        self.st().infos.insert(seq, info);
    }

    /// Scripts what `blocks(seq, lane, _)` answers.
    pub fn set_blocks(&self, seq: u64, lane: u64, rows: Vec<BlockRow>) {
        self.st().blocks.insert((seq, lane), rows);
    }

    /// Scripts what `lane_events(seq, lane)` answers.
    pub fn set_events(&self, seq: u64, lane: u64, rows: Vec<EventRow>) {
        self.st().events.insert((seq, lane), rows);
    }

    /// The error every bar step, key step and wind answers with.
    pub fn set_jump_error(&self, e: Option<VizError>) {
        self.st().jump_error = e;
    }

    pub fn set_locate_error(&self, e: Option<VizError>) {
        self.st().locate_error = e;
    }

    pub fn set_loop_error(&self, e: Option<VizError>) {
        self.st().loop_error = e;
    }

    /// Whether `set_loop` last turned looping on.
    pub fn looped(&self) -> bool {
        self.st().looped
    }

    /// How often `sequence_info` was read (reads stay out of the call log).
    pub fn info_reads(&self) -> usize {
        self.st().info_reads
    }

    /// Every `play_from(seq, from, to)`, in order.
    pub fn plays_from(&self) -> Vec<(u64, i64, Option<i64>)> {
        self.st().plays_from.clone()
    }

    /// The direction of every `transport_step_bar`.
    pub fn step_bars(&self) -> Vec<bool> {
        self.st().step_bars.clone()
    }

    /// The direction of every `transport_key_step`.
    pub fn key_steps(&self) -> Vec<bool> {
        self.st().key_steps.clone()
    }

    /// The quarters of every wind that went through.
    pub fn winds(&self) -> Vec<f64> {
        self.st().winds.clone()
    }

    /// Winds tried, refused ones included.
    pub fn wind_attempts(&self) -> usize {
        self.st().wind_attempts
    }

    /// `transport_hold_end` calls.
    pub fn hold_ends(&self) -> usize {
        self.st().hold_ends
    }

    /// Every `edit` command received, in order.
    pub fn edits(&self) -> Vec<EditCommand> {
        self.st().edits.clone()
    }

    /// Every call, in order, as `name` or `name(arg)`.
    pub fn calls(&self) -> Vec<String> {
        self.st().calls.clone()
    }

    /// Forgets the calls so far.
    pub fn clear_calls(&self) {
        self.st().calls.clear();
    }

    pub fn count(&self, name: &str) -> usize {
        self.calls().iter().filter(|c| c.as_str() == name).count()
    }

    fn lane_mut(s: &mut State, seq: u64, lane: u64) -> Option<&mut LaneRow> {
        s.lanes.get_mut(&seq)?.iter_mut().find(|l| l.id == lane)
    }

    fn command_result(s: &mut State) -> DocSnapshot {
        match s.next_snapshot.take() {
            Some(n) => s.snap = n,
            None => s.snap.version += 1,
        }
        Self::sync_revs(s);
        s.snap.clone()
    }

    /// Like the core: the snapshot lists a revision for every lane.
    fn sync_revs(s: &mut State) {
        let mut seqs: Vec<&u64> = s.lanes.keys().collect();
        seqs.sort();
        let rev = s.snap.version;
        s.snap.lane_revs = seqs
            .into_iter()
            .flat_map(|seq| {
                s.lanes[seq].iter().map(move |l| LaneRev {
                    seq: *seq,
                    lane: l.id,
                    rev,
                })
            })
            .collect();
    }

    /// The scripted error of `call`, if any (a one-shot one first).
    fn scripted(s: &mut State, call: &str) -> Result<(), VizError> {
        if let Some(e) = s.next_errors.get_mut(call).and_then(VecDeque::pop_front) {
            return Err(e);
        }
        match s.call_errors.get(call) {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }

    /// Makes `call` (`begin_edit`, `edit_gesture`, `end_edit`,
    /// `abort_edit`, `edit_all`, `seconds_at`, `ticks_at`, `ui_layout`,
    /// `set_ui_layout`, `destinations`, `lanes`, `lane_events`, `audition`) answer `e` every time (`None`: normally again).
    pub fn set_call_error(&self, call: &str, e: Option<VizError>) {
        let mut s = self.st();
        match e {
            Some(e) => s.call_errors.insert(call.to_owned(), e),
            None => s.call_errors.remove(call),
        };
    }

    /// Makes the next `call` answer `e` (queued: several answer in turn).
    pub fn fail_next(&self, call: &str, e: VizError) {
        self.st()
            .next_errors
            .entry(call.to_owned())
            .or_default()
            .push_back(e);
    }

    /// The error `set_selection` answers with (the selection stays).
    pub fn set_selection_error(&self, e: Option<VizError>) {
        self.st().selection_error = e;
    }

    /// What the session keeps of each selection it is given.
    pub fn set_keep_selection(&self, keep: Option<KeepSelection>) {
        self.st().keep_selection = keep;
    }

    /// The `ui.json` the session holds (as read at open).
    pub fn set_layout(&self, json: Option<&str>) {
        self.st().layout = json.map(String::from);
    }

    /// The `ui.json` the session holds now.
    pub fn layout(&self) -> Option<String> {
        self.st().layout.clone()
    }

    /// Whether an edit is open (`begin_edit` without `end_edit`/`abort_edit`).
    pub fn edit_is_open(&self) -> bool {
        self.st().open_edit.is_some()
    }

    /// Every `edit_gesture` command, in order.
    pub fn gesture_edits(&self) -> Vec<EditCommand> {
        self.st().gesture_edits.clone()
    }

    /// The instrument list `destinations` answers with.
    pub fn set_destinations(&self, rows: Vec<DestRow>) {
        self.st().dests = rows;
    }

    /// `lanes` calls.
    pub fn lanes_reads(&self) -> usize {
        self.st().lanes_reads
    }

    /// `lane_events` calls.
    pub fn event_reads(&self) -> usize {
        self.st().event_reads
    }

    /// Every audition note sent, in order: `("on" | "off", pitch, dest,
    /// velocity)` (a note-off carries velocity 0).
    pub fn auditions(&self) -> Vec<(String, u8, u64, u16)> {
        self.st().auditions.clone()
    }

    /// Whether `audition` sends (the default) or answers false, as a
    /// session that does not hold the engine does. A Busy refusal is
    /// scripted with `set_call_error("audition", ..)` or `fail_next`.
    pub fn set_audition_sent(&self, sent: bool) {
        self.st().audition_sent = sent;
    }

    /// Every `edit_all(label, cmds)`, in order.
    pub fn edit_alls(&self) -> Vec<(String, Vec<EditCommand>)> {
        self.st().edit_alls.clone()
    }

    // The mixer. Errors are scripted by call name (`mixer`, `levels`,
    // `gesture`, `automation_state`, `set_automation_mode`,
    // `heard_values`, `run_mixer`, `add_bus`, `new_bus_from`,
    // `remove_bus`, `rename_strip`, `set_output`, `add_send`, `edit_send`,
    // `remove_send`, `set_strip_colour`,
    // `release_channel_touches`, `set_surface_selection`, `remove_insert`,
    // `move_insert`, `set_bypass`, `set_sidechain`, `set_eq`,
    // `load_plugins`, `held_slots`, `set_midi_mix`, `preview_midi_mix`); a gesture phase
    // also answers `gesture_touch`, `gesture_value` or `gesture_release`
    // first.

    /// The mixer `mixer()` answers with.
    pub fn set_mixer(&self, m: MixerSnapshot) {
        self.st().mixer = m;
    }

    /// Edits the scripted mixer.
    pub fn edit_mixer(&self, f: impl FnOnce(&mut MixerSnapshot)) {
        f(&mut self.st().mixer);
    }

    pub fn mixer_snapshot(&self) -> MixerSnapshot {
        self.st().mixer.clone()
    }

    /// `mixer` calls (refused ones included).
    pub fn mixer_reads(&self) -> usize {
        self.st().mixer_reads
    }

    /// What every `levels()` answers with.
    pub fn set_levels(&self, l: MixerLevels) {
        self.st().levels = l;
    }

    /// `levels` calls (refused ones included).
    pub fn levels_reads(&self) -> usize {
        self.st().levels_reads
    }

    /// Every gesture phase the session took, in order, with `alone`.
    pub fn gestures(&self) -> Vec<(AutomationTargetRow, GesturePhaseRow, bool)> {
        self.st().gestures.clone()
    }

    pub fn clear_gestures(&self) {
        self.st().gestures.clear();
    }

    /// What `automation_state` answers with.
    pub fn set_automation_state(&self, a: AutomationStateRow) {
        self.st().automation = a;
    }

    /// The heard values `heard_values` answers while playing (dB, −1…1
    /// for a pan); other targets are `None`.
    pub fn set_heard(&self, heard: HashMap<AutomationTargetRow, f64>) {
        self.st().heard = heard;
    }

    /// The targets of every `heard_values` call.
    pub fn heard_calls(&self) -> Vec<Vec<AutomationTargetRow>> {
        self.st().heard_calls.clone()
    }

    /// Every `set_automation_mode(strips, mode)`.
    pub fn mode_calls(&self) -> Vec<(Vec<u64>, AutomationModeRow)> {
        self.st().mode_calls.clone()
    }

    /// Every `set_surface_selection(strips)`.
    pub fn surface_selections(&self) -> Vec<Vec<u64>> {
        self.st().surface_selections.clone()
    }

    /// The holder of every gesture phase the session took, in order.
    pub fn holders(&self) -> Vec<GestureHolderRow> {
        self.st().holders.clone()
    }

    /// How many touches `release_channel_touches` says it released.
    pub fn set_channel_touches(&self, n: u32) {
        self.st().channel_touches = n;
    }

    /// The slots safe mode holds off (`held_slots`); `load_plugins` takes
    /// them out.
    pub fn set_held(&self, slots: Vec<u64>) {
        self.st().held = slots;
    }

    /// Every `load_plugins(slots)`.
    pub fn loaded(&self) -> Vec<Vec<u64>> {
        self.st().loaded.clone()
    }

    /// Every `preview_midi_mix(dest, volume, pan)`.
    pub fn midi_previews(&self) -> Vec<(u64, Option<u8>, Option<u8>)> {
        self.st().midi_previews.clone()
    }

    /// Every `set_midi_mix(dest, mix)` the session took.
    pub fn midi_mixes(&self) -> Vec<(u64, MidiMixRow)> {
        self.st().midi_mixes.clone()
    }

    /// Edits the scripted destinations.
    pub fn edit_destinations(&self, f: impl FnOnce(&mut Vec<DestRow>)) {
        f(&mut self.st().dests);
    }

    /// Every `run_mixer(id, args)`.
    pub fn run_mixers(&self) -> Vec<(String, Vec<ControlArg>)> {
        self.st().run_mixers.clone()
    }

    /// A mixer edit: scripted error, logged call, the change, a new
    /// document version (the mixer's too).
    fn mixer_edit(
        &self,
        call: &str,
        logged: String,
        change: impl FnOnce(&mut MixerSnapshot) -> Result<(), VizError>,
    ) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, call)?;
        s.calls.push(logged);
        change(&mut s.mixer)?;
        let snap = Self::command_result(&mut s);
        s.mixer.version = snap.version;
        Ok(snap)
    }

    /// Applies a released touch's value to the mixer, as the core's step.
    fn land(m: &mut MixerSnapshot, t: AutomationTargetRow, v: f64) {
        use core_model::automation::{fader_db, pan_of};
        match t {
            AutomationTargetRow::StripFader { strip } => {
                if let Some(s) = m.strips.iter_mut().find(|s| s.id == strip) {
                    s.gain_db = fader_db(v);
                }
            }
            AutomationTargetRow::StripPan { strip } => {
                if let Some(s) = m.strips.iter_mut().find(|s| s.id == strip) {
                    s.pan = pan_of(v);
                }
            }
            AutomationTargetRow::StripMute { strip } => {
                if let Some(s) = m.strips.iter_mut().find(|s| s.id == strip) {
                    s.mute = v >= 0.5;
                }
            }
            AutomationTargetRow::StripSolo { strip } => {
                if let Some(s) = m.strips.iter_mut().find(|s| s.id == strip) {
                    s.solo = v >= 0.5;
                }
            }
            AutomationTargetRow::SendLevel { strip, send } => {
                if let Some(x) = m
                    .strips
                    .iter_mut()
                    .find(|s| s.id == strip)
                    .and_then(|s| s.sends.get_mut(usize::from(send)))
                {
                    x.level_db = fader_db(v);
                }
            }
            AutomationTargetRow::VcaFader { vca } => {
                if let Some(x) = m.vcas.iter_mut().find(|x| x.id == vca) {
                    x.gain_db = fader_db(v);
                }
            }
            AutomationTargetRow::PluginParam { .. } | AutomationTargetRow::VelocityScale { .. } => {
            }
        }
    }

    fn take_gesture(
        &self,
        target: AutomationTargetRow,
        phase: GesturePhaseRow,
        holder: GestureHolderRow,
        alone: bool,
    ) -> Result<(), VizError> {
        let mut s = self.st();
        let name = match phase {
            GesturePhaseRow::Touch { .. } => "gesture_touch",
            GesturePhaseRow::Value { .. } => "gesture_value",
            GesturePhaseRow::Release => "gesture_release",
        };
        Self::scripted(&mut s, name)?;
        Self::scripted(&mut s, "gesture")?;
        s.gestures.push((target, phase, alone));
        s.holders.push(holder);
        match phase {
            GesturePhaseRow::Touch { value } => {
                s.touch_values.insert(target, value);
            }
            GesturePhaseRow::Value { value } => {
                s.touch_values.insert(target, Some(value));
            }
            GesturePhaseRow::Release => {
                if let Some(Some(v)) = s.touch_values.remove(&target) {
                    Self::land(&mut s.mixer, target, v);
                    let snap = Self::command_result(&mut s);
                    s.mixer.version = snap.version;
                }
            }
        }
        Ok(())
    }
}

/// Files and jobs (slice 7a): scripted jobs and the file reads.
impl FakeSession {
    /// The next job started runs `script` (queued: several start in turn;
    /// with none queued a job is done at its first poll).
    pub fn script_job(&self, script: JobScript) {
        self.st().files.scripts.push_back(script);
    }

    /// Every job started, in order, with what started it.
    pub fn jobs_started(&self) -> Vec<StartedJob> {
        self.st().files.started.clone()
    }

    /// Every `cancel_job(id)`, in order.
    pub fn cancelled_jobs(&self) -> Vec<u64> {
        self.st().files.cancelled.clone()
    }

    /// How many times read call `name` (`job`, `media`, `project_info`,
    /// the checks …) was answered.
    pub fn reads(&self, name: &str) -> usize {
        self.st().files.reads.get(name).copied().unwrap_or(0)
    }

    pub fn set_media(&self, rows: Vec<MediaRow>) {
        self.st().files.media = rows;
    }

    /// What `recheck_media` answers (the offline set changed).
    pub fn set_recheck_media(&self, changed: bool) {
        self.st().files.recheck = changed;
    }

    /// What `project_info` answers (default: 48 kHz, 24-bit, nothing
    /// mismatched, no device rate).
    pub fn set_project_info(&self, info: ProjectInfoRow) {
        self.st().files.project_info = Some(info);
    }

    /// What `bounce_check` answers (default: no frames, no files).
    pub fn set_bounce_check(&self, row: BounceCheckRow) {
        self.st().files.bounce_check = Some(row);
    }

    /// What dry run `call` (`export_midi_check`, `export_midi_clip_check`,
    /// `export_dawproject_check`, `export_aaf_check`) answers (default: an
    /// empty report).
    pub fn set_check_report(&self, call: &str, report: ReportRow) {
        self.st().files.reports.insert(call.to_owned(), report);
    }

    /// What `export_for_daw_check` answers (unscripted: refused).
    pub fn set_for_daw_check(&self, row: ForDawCheckRow) {
        self.st().files.for_daw_check = Some(row);
    }

    /// What `midi_file_info(path)` answers (unscripted: refused as
    /// unreadable).
    pub fn set_midi_file_info(&self, path: &str, info: MidiFileInfoRow) {
        self.st().files.midi_info.insert(path.to_owned(), info);
    }

    /// What `export_midi_clip_names` answers (default: none).
    pub fn set_clip_names(&self, row: ClipNamesRow) {
        self.st().files.clip_names = Some(row);
    }

    pub fn set_snapshots(&self, rows: Vec<SnapshotRow>) {
        self.st().files.snapshots = rows;
    }

    /// What `confirm_merge` answers (default: an empty report).
    pub fn set_merge_report(&self, report: ReportRow) {
        self.st().files.merge_report = Some(report);
    }

    pub fn set_interchange_report(&self, report: Option<ReportRow>) {
        self.st().files.interchange_report = report;
    }

    /// The song info the session holds now.
    pub fn song(&self) -> SongInfoRow {
        self.st().files.song_info.clone()
    }

    pub fn set_recovered_files(&self, rows: Vec<RecoveredFileRow>) {
        self.st().files.recovered = rows;
    }

    /// What `project_id` answers.
    pub fn set_project_id(&self, id: Option<&str>) {
        self.st().files.project_id = id.map(str::to_owned);
    }

    /// The exports `has_snapshot` answers true for.
    pub fn set_snapshot_ids(&self, ids: &[&str]) {
        self.st().files.snapshot_ids = ids.iter().map(|i| i.to_string()).collect();
    }

    pub fn set_copies_size(&self, row: CopiesSizeRow) {
        self.st().files.copies = Some(row);
    }

    fn read(s: &mut State, call: &str) -> Result<(), VizError> {
        Self::scripted(s, call)?;
        *s.files.reads.entry(call.to_owned()).or_default() += 1;
        Ok(())
    }

    /// Starts a job of `kind` for `request`: its ID.
    fn start_job(
        &self,
        call: &str,
        logged: String,
        kind: JobKindRow,
        request: JobRequest,
    ) -> Result<u64, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, call)?;
        s.calls.push(logged);
        let f = &mut s.files;
        let id = f.next_job;
        f.next_job += 1;
        let script = f.scripts.pop_front().unwrap_or_else(JobScript::done);
        f.jobs.push((
            id,
            FakeJob {
                kind,
                script,
                polls: 0,
                cancelled: false,
                merge_closed: false,
            },
        ));
        f.started.push(StartedJob { id, kind, request });
        Ok(id)
    }

    fn job_mut(f: &mut Files, id: u64) -> Result<&mut FakeJob, VizError> {
        f.jobs
            .iter_mut()
            .find(|(j, _)| *j == id)
            .map(|(_, j)| j)
            .ok_or_else(|| VizError::Model {
                message: format!("unknown job {id}"),
            })
    }

    /// The row of a job, as a poll answers it (advancing it one step).
    fn poll(f: &mut Files, id: u64) -> Result<JobRow, VizError> {
        let job = Self::job_mut(f, id)?;
        let mut row = JobRow {
            id,
            kind: job.kind,
            progress: 0.0,
            state: JobStateRow::Running,
            message: None,
            problems: vec![],
            created: vec![],
            report: None,
            written: vec![],
            merge: None,
        };
        let step = job.polls;
        job.polls += 1;
        if job.cancelled {
            row.state = JobStateRow::Cancelled;
            return Ok(row);
        }
        if let Some(p) = job.script.steps.get(step) {
            row.progress = *p;
            return Ok(row);
        }
        match &job.script.end {
            JobEnd::Done {
                written,
                report,
                created,
                merge,
                problems,
                ..
            } => {
                row.progress = 1.0;
                row.state = JobStateRow::Done;
                row.written = written.clone();
                row.report = report.clone();
                row.created = created.clone();
                row.merge = merge.clone();
                row.problems = problems.clone();
            }
            JobEnd::Failed { message, problems } => {
                row.state = JobStateRow::Failed;
                row.message = Some(message.clone());
                row.problems = problems.clone();
            }
            JobEnd::Cancelled => row.state = JobStateRow::Cancelled,
        }
        Ok(row)
    }

    /// A job that ended done just now: what it changes in the fake.
    fn landed(f: &mut Files, id: u64) {
        let Some(StartedJob {
            request: JobRequest::Format(format),
            ..
        }) = f.started.iter().find(|j| j.id == id)
        else {
            return;
        };
        let format = *format;
        let info = f.project_info.get_or_insert_with(default_project_info);
        info.format = format;
    }

    /// A merge job done, not cancelled, and not yet confirmed or dropped.
    fn held_merge(f: &mut Files, job: u64) -> Result<&mut FakeJob, VizError> {
        let no = || VizError::Model {
            message: vizdaw_ffi::dawproject::NO_MERGE_WAITING.into(),
        };
        let j = Self::job_mut(f, job).map_err(|_| no())?;
        let ended = j.polls > j.script.steps.len();
        let done = matches!(j.script.end, JobEnd::Done { .. });
        if j.kind != JobKindRow::MergeDawproject || !ended || !done || j.cancelled || j.merge_closed
        {
            return Err(no());
        }
        Ok(j)
    }

    fn check_report(&self, call: &str) -> Result<ReportRow, VizError> {
        let mut s = self.st();
        Self::read(&mut s, call)?;
        Ok(s.files
            .reports
            .get(call)
            .cloned()
            .unwrap_or_else(empty_report))
    }

    /// The name an export of `seq` suggests: the song title, else the
    /// sequence's name.
    fn song_name(s: &State, seq: u64) -> Result<String, VizError> {
        if let Some(t) = s.files.song_info.title.as_ref().filter(|t| !t.is_empty()) {
            return Ok(t.clone());
        }
        s.snap
            .sequences
            .iter()
            .find(|q| q.id == seq)
            .map(|q| q.name.clone())
            .ok_or_else(|| VizError::Model {
                message: format!("unknown sequence {seq}"),
            })
    }
}

fn default_project_info() -> ProjectInfoRow {
    ProjectInfoRow {
        format: ProjectFormatRow {
            sample_rate: 48_000,
            depth: BitDepthRow::Int24,
        },
        mismatched: 0,
        rate_mismatch: false,
        device_rate: 0.0,
    }
}

fn empty_report() -> ReportRow {
    ReportRow {
        summary: String::new(),
        items: vec![],
    }
}

impl DocSession for FakeSession {
    fn path(&self) -> Result<Option<String>, VizError> {
        Ok(self.st().saved_path.clone())
    }
    fn save(&self) -> Result<(), VizError> {
        let mut s = self.st();
        s.calls.push("save".into());
        if let Some(e) = s.save_error.clone() {
            return Err(e);
        }
        if s.saved_path.is_none() {
            return Err(VizError::Io {
                message: "use Save As".into(),
            });
        }
        s.snap.dirty = false;
        Ok(())
    }
    fn save_as(&self, path: String) -> Result<(), VizError> {
        let mut s = self.st();
        s.calls.push(format!("save_as({path})"));
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        s.saved_path = Some(path);
        s.snap.dirty = false;
        s.snap.title = name;
        Ok(())
    }
    fn snapshot(&self) -> Result<DocSnapshot, VizError> {
        Ok(self.snap())
    }
    fn undo(&self) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        if let Some(e) = s.step_error.clone() {
            return Err(e);
        }
        s.calls.push("undo".into());
        Ok(Self::command_result(&mut s))
    }
    fn redo(&self) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        if let Some(e) = s.step_error.clone() {
            return Err(e);
        }
        s.calls.push("redo".into());
        Ok(Self::command_result(&mut s))
    }
    fn transport(&self) -> Result<TransportState, VizError> {
        let s = self.st();
        match s.transport_error.clone() {
            Some(e) => Err(e),
            None => Ok(s.transport.clone()),
        }
    }
    fn play(&self, seq: u64) -> Result<(), VizError> {
        let mut s = self.st();
        if let Some(e) = s.play_error.clone() {
            return Err(e);
        }
        s.calls.push(format!("play({seq})"));
        s.transport.playing = true;
        s.transport.seq = Some(seq);
        Ok(())
    }
    fn play_from(&self, seq: u64, from: i64, to: Option<i64>) -> Result<(), VizError> {
        let mut s = self.st();
        if let Some(e) = s.play_error.clone() {
            return Err(e);
        }
        s.plays_from.push((seq, from, to));
        s.calls.push(format!("play_from({seq},{from},{to:?})"));
        s.transport.playing = true;
        s.transport.seq = Some(seq);
        s.transport.tick = from;
        Ok(())
    }
    fn stop(&self) -> Result<(), VizError> {
        let mut s = self.st();
        s.calls.push("stop".into());
        s.transport.playing = false;
        Ok(())
    }
    fn locate(&self, tick: i64) -> Result<(), VizError> {
        let mut s = self.st();
        if let Some(e) = s.locate_error.clone() {
            return Err(e);
        }
        s.calls.push(format!("locate({tick})"));
        s.transport.tick = tick;
        Ok(())
    }
    fn close(&self) -> Result<(), VizError> {
        self.st().calls.push("close".into());
        Ok(())
    }
    fn has_unsaved_changes(&self) -> Result<bool, VizError> {
        let s = self.st();
        match s.unsaved.clone() {
            Some(r) => r,
            None => Ok(s.snap.dirty),
        }
    }
    fn idle(&self) -> Result<IdleNews, VizError> {
        let mut s = self.st();
        let n = s.idle_snapshot.take();
        if let Some(n) = &n {
            s.snap = n.clone();
        }
        Ok(IdleNews {
            snapshot: n,
            slots_changed: std::mem::take(&mut s.idle_slots_changed),
            media_changed: std::mem::take(&mut s.idle_media_changed),
        })
    }
    fn lanes(&self, seq: u64) -> Result<Vec<LaneRow>, VizError> {
        let mut s = self.st();
        s.lanes_reads += 1;
        Self::scripted(&mut s, "lanes")?;
        Ok(s.lanes.get(&seq).cloned().unwrap_or_default())
    }
    fn new_track(
        &self,
        seq: u64,
        kind: TrackKindRow,
        after: Option<u64>,
    ) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        s.calls.push(format!("new_track({seq},{kind:?},{after:?})"));
        let id = s.lanes.values().flatten().map(|l| l.id).max().unwrap_or(0) + 1;
        let mut row = lane_row(id, &format!("Track {id}"), false);
        row.kind = kind;
        let rows = s.lanes.entry(seq).or_default();
        let at = after
            .and_then(|a| rows.iter().position(|l| l.id == a))
            .map_or(rows.len(), |i| i + 1);
        rows.insert(at, row);
        Ok(Self::command_result(&mut s))
    }
    fn duplicate_track(&self, seq: u64, lane: u64) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        s.calls.push(format!("duplicate_track({seq},{lane})"));
        let id = s.lanes.values().flatten().map(|l| l.id).max().unwrap_or(0) + 1;
        if let Some(rows) = s.lanes.get_mut(&seq)
            && let Some(i) = rows.iter().position(|l| l.id == lane)
        {
            let mut copy = rows[i].clone();
            copy.id = id;
            copy.name = format!("{} copy", copy.name);
            rows.insert(i + 1, copy);
        }
        Ok(Self::command_result(&mut s))
    }
    fn edit(&self, cmd: EditCommand) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        if s.open_edit.is_some() {
            return Err(VizError::Busy {
                message: "Finish the open edit first.".into(),
            });
        }
        if let Some(e) = s.edit_error.clone() {
            return Err(e);
        }
        s.calls.push("edit".into());
        if let EditCommand::Lane { edit } = &cmd {
            match edit {
                LaneEdit::Rename { seq, lane, name } => {
                    if let Some(l) = Self::lane_mut(&mut s, *seq, *lane) {
                        l.name = name.clone();
                    }
                }
                LaneEdit::SetFlags {
                    seq,
                    lane,
                    mute,
                    solo,
                    arm,
                } => {
                    if let Some(l) = Self::lane_mut(&mut s, *seq, *lane) {
                        l.mute = mute.unwrap_or(l.mute);
                        l.solo = solo.unwrap_or(l.solo);
                        l.arm = arm.unwrap_or(l.arm);
                    }
                }
                LaneEdit::SetMonitor { seq, lane, monitor } => {
                    if let Some(l) = Self::lane_mut(&mut s, *seq, *lane) {
                        l.monitor = *monitor;
                    }
                }
                LaneEdit::Remove { seq, lane } => {
                    if let Some(rows) = s.lanes.get_mut(seq) {
                        rows.retain(|l| l.id != *lane);
                    }
                    // Like the core: a removed lane leaves the selection.
                    s.selection
                        .lanes
                        .retain(|r| !(r.seq == *seq && r.lane == *lane));
                }
                _ => {}
            }
        }
        s.edits.push(cmd);
        Ok(Self::command_result(&mut s))
    }
    fn set_lane_mute(&self, seq: u64, lane: u64, mute: bool) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        if let Some(e) = s.edit_error.clone() {
            return Err(e);
        }
        s.calls.push(format!("set_lane_mute({seq},{lane},{mute})"));
        if let Some(l) = Self::lane_mut(&mut s, seq, lane) {
            l.mute = mute;
        }
        Ok(Self::command_result(&mut s))
    }
    fn selection(&self) -> Result<SelectionRow, VizError> {
        Ok(self.st().selection.clone())
    }
    fn set_selection(&self, mut sel: SelectionRow) -> Result<(), VizError> {
        let mut s = self.st();
        if let Some(e) = s.selection_error.clone() {
            return Err(e);
        }
        s.calls.push("set_selection".into());
        // Like the core: lanes that do not exist are dropped.
        let known = |s: &State, r: &LaneRef| {
            s.lanes
                .get(&r.seq)
                .is_some_and(|rows| rows.iter().any(|l| l.id == r.lane))
        };
        sel.lanes.retain(|r| known(&s, r));
        if let Some(keep) = &s.keep_selection {
            sel = keep(sel);
        }
        s.selection = sel;
        Ok(())
    }
    fn set_loop(&self, on: bool) -> Result<(), VizError> {
        let mut s = self.st();
        if let Some(e) = s.loop_error.clone() {
            return Err(e);
        }
        s.calls.push(format!("set_loop({on})"));
        s.looped = on;
        Ok(())
    }
    fn marker_step(&self, seq: u64, tick: i64, forward: bool) -> Result<Option<i64>, VizError> {
        let mut s = self.st();
        s.calls.push(format!("marker_step({seq},{tick},{forward})"));
        let ticks = s.marker_ticks.iter().copied();
        Ok(if forward {
            ticks.filter(|t| *t > tick).min()
        } else {
            ticks.filter(|t| *t < tick).max()
        })
    }
    fn transport_step_bar(&self, forward: bool) -> Result<i64, VizError> {
        let mut s = self.st();
        if let Some(e) = s.jump_error.clone() {
            return Err(e);
        }
        s.calls.push(format!("transport_step_bar({forward})"));
        s.step_bars.push(forward);
        let to = (s.transport.tick + if forward { BAR } else { -BAR }).max(0);
        s.transport.tick = to;
        Ok(to)
    }
    fn transport_key_step(&self, forward: bool) -> Result<i64, VizError> {
        let mut s = self.st();
        if let Some(e) = s.jump_error.clone() {
            return Err(e);
        }
        s.calls.push(format!("transport_key_step({forward})"));
        s.key_steps.push(forward);
        let to = (s.transport.tick + if forward { BAR } else { -BAR }).max(0);
        s.transport.tick = to;
        Ok(to)
    }
    fn transport_wind(&self, quarters: f64) -> Result<i64, VizError> {
        let mut s = self.st();
        s.wind_attempts += 1;
        if let Some(e) = s.jump_error.clone() {
            return Err(e);
        }
        s.calls.push(format!("transport_wind({quarters})"));
        s.winds.push(quarters);
        let to = (s.transport.tick + (quarters * 15_360.0).round() as i64).max(0);
        s.transport.tick = to;
        Ok(to)
    }
    fn transport_hold_end(&self) -> Result<i64, VizError> {
        let mut s = self.st();
        s.calls.push("transport_hold_end".into());
        s.hold_ends += 1;
        Ok(s.transport.tick)
    }
    fn selection_span(&self, seq: u64) -> Result<Option<TickRange>, VizError> {
        let mut s = self.st();
        s.calls.push(format!("selection_span({seq})"));
        Ok(s.span)
    }
    fn sequence_info(&self, seq: u64) -> Result<SequenceInfo, VizError> {
        let mut s = self.st();
        s.info_reads += 1;
        if let Some(i) = s.infos.get(&seq) {
            return Ok(i.clone());
        }
        let row = s.snap.sequences.iter().find(|q| q.id == seq).cloned();
        let Some(row) = row else {
            return Err(VizError::Model {
                message: format!("There is no sequence {seq}."),
            });
        };
        Ok(SequenceInfo {
            id: seq,
            name: row.name,
            comment: String::new(),
            keyboard_slot: None,
            start_offset: 0,
            length_ticks: row.length_ticks,
            tempo: vec![],
            meter: vec![MeterEntryRow {
                tick: 0,
                num: 4,
                den: 4,
                sharps: 0,
                minor: false,
            }],
            markers: vec![],
            generated: false,
        })
    }
    fn blocks(&self, seq: u64, lane: u64, _rule: BlockRule) -> Result<Vec<BlockRow>, VizError> {
        let s = self.st();
        Ok(s.blocks.get(&(seq, lane)).cloned().unwrap_or_default())
    }
    fn lane_events(&self, seq: u64, lane: u64) -> Result<LaneEvents, VizError> {
        let mut s = self.st();
        s.event_reads += 1;
        Self::scripted(&mut s, "lane_events")?;
        Ok(LaneEvents {
            seq,
            lane,
            rev: s.snap.version,
            events: s.events.get(&(seq, lane)).cloned().unwrap_or_default(),
        })
    }
    fn markers(&self, _seq: u64) -> Result<Vec<MarkerRow>, VizError> {
        Ok(vec![])
    }
    fn destinations(&self) -> Result<Vec<DestRow>, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "destinations")?;
        Ok(s.dests.clone())
    }
    fn begin_edit(&self, label: &str) -> Result<(), VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "begin_edit")?;
        if s.open_edit.is_some() {
            return Err(VizError::Model {
                message: "an edit is already open".into(),
            });
        }
        s.calls.push(format!("begin_edit({label})"));
        s.open_edit = Some(label.to_owned());
        s.gesture_applied = false;
        Ok(())
    }
    fn edit_gesture(&self, cmd: EditCommand) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        if let Err(e) = Self::scripted(&mut s, "edit_gesture") {
            // Logged: a drag's refused or busy move is part of its story.
            s.calls.push("edit_gesture refused".into());
            return Err(e);
        }
        if s.open_edit.is_none() {
            return Err(VizError::Model {
                message: "no edit is open".into(),
            });
        }
        s.calls.push("edit_gesture".into());
        s.gesture_edits.push(cmd);
        s.gesture_applied = true;
        Ok(Self::command_result(&mut s))
    }
    fn end_edit(&self) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "end_edit")?;
        s.calls.push("end_edit".into());
        let label = s.open_edit.take();
        if s.gesture_applied {
            s.gesture_applied = false;
            let mut snap = Self::command_result(&mut s);
            snap.undo_label = label;
            snap.redo_label = None;
            s.snap = snap.clone();
            return Ok(snap);
        }
        Self::sync_revs(&mut s);
        Ok(s.snap.clone())
    }
    fn abort_edit(&self) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "abort_edit")?;
        s.calls.push("abort_edit".into());
        s.open_edit = None;
        s.gesture_applied = false;
        Ok(Self::command_result(&mut s))
    }
    fn edit_all(&self, label: &str, cmds: Vec<EditCommand>) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "edit_all")?;
        if s.open_edit.is_some() {
            return Err(VizError::Busy {
                message: "Finish the open edit first.".into(),
            });
        }
        s.calls.push(format!("edit_all({label})"));
        s.edit_alls.push((label.to_owned(), cmds));
        Ok(Self::command_result(&mut s))
    }
    fn seconds_at(&self, seq: u64, ticks: Vec<i64>) -> Result<Vec<f64>, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "seconds_at")?;
        s.calls.push(format!("seconds_at({seq})"));
        let per_second = s.transport.ticks_per_second;
        Ok(ticks.into_iter().map(|t| t as f64 / per_second).collect())
    }
    fn ticks_at(&self, seq: u64, seconds: Vec<f64>) -> Result<Vec<i64>, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "ticks_at")?;
        s.calls.push(format!("ticks_at({seq})"));
        let per_second = s.transport.ticks_per_second;
        Ok(seconds
            .into_iter()
            .map(|x| (x * per_second).round() as i64)
            .collect())
    }
    fn ui_layout(&self) -> Result<Option<String>, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "ui_layout")?;
        Ok(s.layout.clone())
    }
    fn audition(&self, dest: u64, pitch: u8, velocity: u16, on: bool) -> Result<bool, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "audition")?;
        if !s.audition_sent {
            return Ok(false);
        }
        s.auditions.push((
            (if on { "on" } else { "off" }).into(),
            pitch,
            dest,
            velocity,
        ));
        Ok(true)
    }
    fn set_ui_layout(&self, json: String) -> Result<(), VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "set_ui_layout")?;
        // Like the core: only JSON is held.
        if let Err(e) = serde_json::from_str::<serde_json::Value>(&json) {
            return Err(VizError::Model {
                message: format!("the layout is not JSON: {e}"),
            });
        }
        s.calls.push("set_ui_layout".into());
        s.layout = Some(json);
        Ok(())
    }
    fn mixer(&self) -> Result<MixerSnapshot, VizError> {
        let mut s = self.st();
        s.mixer_reads += 1;
        Self::scripted(&mut s, "mixer")?;
        Ok(s.mixer.clone())
    }
    fn levels(&self) -> Result<MixerLevels, VizError> {
        let mut s = self.st();
        s.levels_reads += 1;
        Self::scripted(&mut s, "levels")?;
        Ok(s.levels.clone())
    }
    fn gesture(&self, target: AutomationTargetRow, phase: GesturePhaseRow) -> Result<(), VizError> {
        self.take_gesture(target, phase, GestureHolderRow::Mouse, false)
    }
    fn gesture_as(
        &self,
        target: AutomationTargetRow,
        phase: GesturePhaseRow,
        holder: GestureHolderRow,
        alone: bool,
    ) -> Result<(), VizError> {
        self.take_gesture(target, phase, holder, alone)
    }
    fn automation_state(&self) -> Result<AutomationStateRow, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "automation_state")?;
        Ok(s.automation.clone())
    }
    fn set_automation_mode(
        &self,
        strips: Vec<u64>,
        mode: AutomationModeRow,
    ) -> Result<DocSnapshot, VizError> {
        self.st().mode_calls.push((strips.clone(), mode));
        let logged = format!("set_automation_mode({strips:?},{mode:?})");
        self.mixer_edit("set_automation_mode", logged, |m| {
            for s in m.strips.iter_mut().filter(|s| strips.contains(&s.id)) {
                s.automation = mode;
            }
            Ok(())
        })
    }
    fn heard_values(
        &self,
        targets: Vec<AutomationTargetRow>,
    ) -> Result<Vec<Option<f64>>, VizError> {
        let mut s = self.st();
        s.heard_calls.push(targets.clone());
        Self::scripted(&mut s, "heard_values")?;
        if targets.len() > 4096 {
            return Err(VizError::Model {
                message: "at most 4096 targets".into(),
            });
        }
        let playing = s.transport.playing;
        Ok(targets
            .iter()
            .map(|t| s.heard.get(t).copied().filter(|_| playing))
            .collect())
    }
    fn run_mixer(&self, id: &str, args: Vec<ControlArg>) -> Result<DocSnapshot, VizError> {
        self.st().run_mixers.push((id.to_owned(), args));
        self.mixer_edit("run_mixer", format!("run_mixer({id})"), |_| Ok(()))
    }
    fn add_bus(&self, name: &str) -> Result<DocSnapshot, VizError> {
        let name = name.to_owned();
        self.mixer_edit("add_bus", format!("add_bus({name})"), |m| {
            add_bus_strip(m, &name);
            Ok(())
        })
    }
    fn new_bus_from(&self, strip: u64, name: &str) -> Result<DocSnapshot, VizError> {
        let name = name.to_owned();
        self.mixer_edit(
            "new_bus_from",
            format!("new_bus_from({strip},{name})"),
            |m| {
                let bus = add_bus_strip(m, &name);
                if let Some(s) = m.strips.iter_mut().find(|s| s.id == strip) {
                    s.sends.push(vizdaw_ffi::mixer::SendRow {
                        target: bus,
                        level_db: 0.0,
                        pre_fader: false,
                        enabled: true,
                    });
                }
                Ok(())
            },
        )
    }
    fn remove_bus(&self, strip: u64) -> Result<DocSnapshot, VizError> {
        self.mixer_edit("remove_bus", format!("remove_bus({strip})"), |m| {
            m.strips.retain(|s| s.id != strip);
            m.buses.retain(|b| b.id != strip);
            for s in &mut m.strips {
                if s.output == Some(strip) {
                    s.output = None;
                }
            }
            Ok(())
        })
    }
    fn rename_strip(&self, strip: u64, name: &str) -> Result<DocSnapshot, VizError> {
        let name = name.to_owned();
        self.mixer_edit(
            "rename_strip",
            format!("rename_strip({strip},{name})"),
            |m| {
                if let Some(s) = m.strips.iter_mut().find(|s| s.id == strip) {
                    s.name = name.clone();
                }
                if let Some(b) = m.buses.iter_mut().find(|b| b.id == strip) {
                    b.name = name;
                }
                Ok(())
            },
        )
    }
    fn set_output(&self, strip: u64, bus: Option<u64>) -> Result<DocSnapshot, VizError> {
        self.mixer_edit("set_output", format!("set_output({strip},{bus:?})"), |m| {
            if let Some(s) = m.strips.iter_mut().find(|s| s.id == strip) {
                s.output = bus;
            }
            Ok(())
        })
    }
    fn add_send(&self, strip: u64, target: u64) -> Result<DocSnapshot, VizError> {
        self.mixer_edit("add_send", format!("add_send({strip},{target})"), |m| {
            if let Some(s) = m.strips.iter_mut().find(|s| s.id == strip) {
                s.sends.push(vizdaw_ffi::mixer::SendRow {
                    target,
                    level_db: 0.0,
                    pre_fader: false,
                    enabled: true,
                });
            }
            Ok(())
        })
    }
    fn edit_send(
        &self,
        strip: u64,
        index: u32,
        send: vizdaw_ffi::mixer::SendRow,
    ) -> Result<DocSnapshot, VizError> {
        let logged = format!(
            "edit_send({strip},{index},{},{:?},{},{})",
            send.target, send.level_db, send.pre_fader, send.enabled
        );
        self.mixer_edit("edit_send", logged, |m| {
            if let Some(x) = m
                .strips
                .iter_mut()
                .find(|s| s.id == strip)
                .and_then(|s| s.sends.get_mut(index as usize))
            {
                *x = send;
            }
            Ok(())
        })
    }
    fn remove_send(&self, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
        self.mixer_edit(
            "remove_send",
            format!("remove_send({strip},{index})"),
            |m| {
                if let Some(s) = m.strips.iter_mut().find(|s| s.id == strip)
                    && (index as usize) < s.sends.len()
                {
                    s.sends.remove(index as usize);
                }
                Ok(())
            },
        )
    }
    fn set_strip_colour(
        &self,
        strip: u64,
        colour: Option<SwatchRow>,
    ) -> Result<DocSnapshot, VizError> {
        self.mixer_edit(
            "set_strip_colour",
            format!("set_strip_colour({strip},{colour:?})"),
            |m| {
                if let Some(s) = m.strips.iter_mut().find(|s| s.id == strip) {
                    s.colour = colour;
                }
                Ok(())
            },
        )
    }
    fn release_channel_touches(&self) -> Result<u32, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "release_channel_touches")?;
        s.calls.push("release_channel_touches".into());
        Ok(s.channel_touches)
    }
    fn set_surface_selection(&self, strips: Vec<u64>) -> Result<(), VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "set_surface_selection")?;
        s.surface_selections.push(strips);
        Ok(())
    }
    fn remove_insert(&self, slot: u64) -> Result<DocSnapshot, VizError> {
        self.mixer_edit("remove_insert", format!("remove_insert({slot})"), |m| {
            for s in &mut m.strips {
                s.inserts.retain(|x| x.id != slot);
            }
            Ok(())
        })
    }
    fn move_insert(&self, slot: u64, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
        self.mixer_edit(
            "move_insert",
            format!("move_insert({slot},{strip},{index})"),
            |m| {
                let mut moved = None;
                for s in &mut m.strips {
                    if let Some(at) = s.inserts.iter().position(|x| x.id == slot) {
                        moved = Some(s.inserts.remove(at));
                    }
                }
                if let (Some(x), Some(s)) = (moved, m.strips.iter_mut().find(|s| s.id == strip)) {
                    let at = (index as usize).min(s.inserts.len());
                    s.inserts.insert(at, x);
                }
                Ok(())
            },
        )
    }
    fn set_bypass(&self, slot: u64, bypass: bool) -> Result<DocSnapshot, VizError> {
        self.mixer_edit("set_bypass", format!("set_bypass({slot},{bypass})"), |m| {
            for x in slots_mut(m) {
                if x.id == slot {
                    x.bypass = bypass;
                }
            }
            Ok(())
        })
    }
    fn set_sidechain(&self, slot: u64, source: Option<u64>) -> Result<DocSnapshot, VizError> {
        self.mixer_edit(
            "set_sidechain",
            format!("set_sidechain({slot},{source:?})"),
            |m| {
                for x in slots_mut(m) {
                    if x.id == slot {
                        x.sidechain = source;
                    }
                }
                Ok(())
            },
        )
    }
    fn set_eq(&self, strip: u64, plugin: Option<PluginRefRow>) -> Result<DocSnapshot, VizError> {
        let name = plugin.as_ref().map(|p| p.id.clone());
        self.mixer_edit("set_eq", format!("set_eq({strip},{name:?})"), |m| {
            if let Some(s) = m.strips.iter_mut().find(|s| s.id == strip) {
                s.eq = plugin.map(|p| SlotRow {
                    plugin: p,
                    ..slot_row(900 + strip, "EQ")
                });
            }
            Ok(())
        })
    }
    fn load_plugins(&self, slots: Vec<u64>) -> Result<(), VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "load_plugins")?;
        s.calls.push(format!("load_plugins({slots:?})"));
        s.held.retain(|h| !slots.contains(h));
        s.loaded.push(slots);
        Ok(())
    }
    fn held_slots(&self) -> Result<Vec<u64>, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "held_slots")?;
        Ok(s.held.clone())
    }
    fn set_midi_mix(&self, dest: u64, mix: MidiMixRow) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "set_midi_mix")?;
        s.calls.push(format!("set_midi_mix({dest})"));
        s.midi_mixes.push((dest, mix.clone()));
        if let Some(d) = s.dests.iter_mut().find(|d| d.id == dest) {
            d.midi_mix = mix;
        }
        Ok(Self::command_result(&mut s))
    }
    fn preview_midi_mix(
        &self,
        dest: u64,
        volume: Option<u8>,
        pan: Option<u8>,
    ) -> Result<bool, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "preview_midi_mix")?;
        s.midi_previews.push((dest, volume, pan));
        Ok(true)
    }
    fn media(&self) -> Result<Vec<MediaRow>, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "media")?;
        Ok(s.files.media.clone())
    }
    fn recheck_media(&self) -> Result<bool, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "recheck_media")?;
        s.calls.push("recheck_media".into());
        Ok(std::mem::take(&mut s.files.recheck))
    }
    fn project_info(&self) -> Result<ProjectInfoRow, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "project_info")?;
        Ok(s.files
            .project_info
            .clone()
            .unwrap_or_else(default_project_info))
    }
    fn set_project_format(&self, format: ProjectFormatRow) -> Result<u64, VizError> {
        self.start_job(
            "set_project_format",
            format!(
                "set_project_format({},{:?})",
                format.sample_rate, format.depth
            ),
            JobKindRow::Format,
            JobRequest::Format(format),
        )
    }
    fn convert_media(&self) -> Result<u64, VizError> {
        self.start_job(
            "convert_media",
            "convert_media".into(),
            JobKindRow::Convert,
            JobRequest::Convert,
        )
    }
    fn import_audio(
        &self,
        paths: Vec<String>,
        target: Option<ImportTargetRow>,
    ) -> Result<u64, VizError> {
        self.start_job(
            "import_audio",
            format!("import_audio({paths:?})"),
            JobKindRow::Import,
            JobRequest::Import { paths, target },
        )
    }
    fn job(&self, id: u64) -> Result<JobRow, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "job")?;
        let row = Self::poll(&mut s.files, id)?;
        let just_ended = Self::job_mut(&mut s.files, id)
            .is_ok_and(|j| j.polls == j.script.steps.len() + 1 && !j.cancelled);
        if row.state == JobStateRow::Done && just_ended {
            Self::landed(&mut s.files, id);
        }
        Ok(row)
    }
    fn cancel_job(&self, id: u64) -> Result<(), VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "cancel_job")?;
        s.calls.push(format!("cancel_job({id})"));
        s.files.cancelled.push(id);
        // Like the core: a job that ended stays as it ended.
        if let Ok(j) = Self::job_mut(&mut s.files, id)
            && j.polls <= j.script.steps.len()
        {
            j.cancelled = true;
        }
        Ok(())
    }
    fn relink_media(&self, media: u64, path: String) -> Result<u64, VizError> {
        self.start_job(
            "relink_media",
            format!("relink_media({media},{path})"),
            JobKindRow::Relink,
            JobRequest::Relink { media, path },
        )
    }
    fn search_media(&self, folder: String) -> Result<u64, VizError> {
        self.start_job(
            "search_media",
            format!("search_media({folder})"),
            JobKindRow::Relink,
            JobRequest::Search(folder),
        )
    }
    fn bounce(&self, req: BounceRequestRow) -> Result<u64, VizError> {
        self.start_job(
            "bounce",
            format!("bounce({:?})", req.path),
            JobKindRow::Bounce,
            JobRequest::Bounce(req),
        )
    }
    fn bounce_check(&self, _req: BounceRequestRow) -> Result<BounceCheckRow, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "bounce_check")?;
        Ok(s.files.bounce_check.clone().unwrap_or(BounceCheckRow {
            external_lanes: 0,
            frames: 0,
            files: vec![],
            warning: None,
        }))
    }
    fn bounce_result(&self, job: u64) -> Result<Option<BounceResultRow>, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "bounce_result")?;
        let j = Self::job_mut(&mut s.files, job)?;
        let ended = j.polls > j.script.steps.len() && !j.cancelled;
        Ok(match &j.script.end {
            JobEnd::Done { bounce, .. } if ended => bounce.clone(),
            _ => None,
        })
    }
    fn midi_file_info(&self, path: String) -> Result<MidiFileInfoRow, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "midi_file_info")?;
        s.files
            .midi_info
            .get(&path)
            .cloned()
            .ok_or_else(|| VizError::Io {
                message: format!("{path} could not be read"),
            })
    }
    fn import_midi(&self, req: MidiImportRequestRow) -> Result<u64, VizError> {
        self.start_job(
            "import_midi",
            format!("import_midi({})", req.path),
            JobKindRow::ImportMidi,
            JobRequest::ImportMidi(req),
        )
    }
    fn export_midi(&self, req: SmfExportRequestRow) -> Result<u64, VizError> {
        self.start_job(
            "export_midi",
            format!("export_midi({})", req.path),
            JobKindRow::ExportMidi,
            JobRequest::ExportMidi(req),
        )
    }
    fn export_midi_check(&self, _req: SmfExportRequestRow) -> Result<ReportRow, VizError> {
        self.check_report("export_midi_check")
    }
    fn export_midi_clip(&self, req: ClipExportRequestRow) -> Result<u64, VizError> {
        self.start_job(
            "export_midi_clip",
            format!("export_midi_clip({})", req.path),
            JobKindRow::ExportMidi,
            JobRequest::ExportMidiClip(req),
        )
    }
    fn export_midi_clip_check(&self, _req: ClipExportRequestRow) -> Result<ReportRow, VizError> {
        self.check_report("export_midi_clip_check")
    }
    fn export_midi_clip_names(&self, _req: ClipExportRequestRow) -> Result<ClipNamesRow, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "export_midi_clip_names")?;
        Ok(s.files.clip_names.clone().unwrap_or(ClipNamesRow {
            names: vec![],
            taken: vec![],
        }))
    }
    fn export_for_daw(&self, req: ForDawRequestRow) -> Result<u64, VizError> {
        self.start_job(
            "export_for_daw",
            format!("export_for_daw({})", req.path),
            JobKindRow::ExportProject,
            JobRequest::ForDaw(req),
        )
    }
    fn export_for_daw_check(&self, _req: ForDawRequestRow) -> Result<ForDawCheckRow, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "export_for_daw_check")?;
        s.files
            .for_daw_check
            .clone()
            .ok_or_else(|| VizError::Model {
                message: "export_for_daw_check is not scripted".into(),
            })
    }
    fn export_dawproject(&self, req: DawprojectExportRequestRow) -> Result<u64, VizError> {
        self.start_job(
            "export_dawproject",
            format!("export_dawproject({})", req.path),
            JobKindRow::ExportDawproject,
            JobRequest::ExportDawproject(req),
        )
    }
    fn export_dawproject_check(
        &self,
        _req: DawprojectExportRequestRow,
    ) -> Result<ReportRow, VizError> {
        self.check_report("export_dawproject_check")
    }
    fn import_dawproject(&self, req: DawprojectImportRequestRow) -> Result<u64, VizError> {
        self.start_job(
            "import_dawproject",
            format!("import_dawproject({})", req.path),
            JobKindRow::ImportProject,
            JobRequest::ImportDawproject(req),
        )
    }
    fn dawproject_name(&self, seq: u64) -> Result<String, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "dawproject_name")?;
        Ok(format!("{}.dawproject", Self::song_name(&s, seq)?))
    }
    fn dawproject_snapshots(&self) -> Result<Vec<SnapshotRow>, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "dawproject_snapshots")?;
        Ok(s.files.snapshots.clone())
    }
    fn merge_dawproject(&self, req: DawprojectMergeRequestRow) -> Result<u64, VizError> {
        self.start_job(
            "merge_dawproject",
            format!("merge_dawproject({},{:?})", req.path, req.step),
            JobKindRow::MergeDawproject,
            JobRequest::MergeDawproject(req),
        )
    }
    fn confirm_merge(&self, job: u64) -> Result<ReportRow, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "confirm_merge")?;
        Self::held_merge(&mut s.files, job)?.merge_closed = true;
        s.calls.push(format!("confirm_merge({job})"));
        let report = s.files.merge_report.clone().unwrap_or_else(empty_report);
        s.files.interchange_report = Some(report.clone());
        Self::command_result(&mut s);
        Ok(report)
    }
    fn cancel_merge(&self, job: u64) -> Result<(), VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "cancel_merge")?;
        Self::held_merge(&mut s.files, job)?.merge_closed = true;
        s.calls.push(format!("cancel_merge({job})"));
        Ok(())
    }
    fn export_aaf(&self, req: AafExportRequestRow) -> Result<u64, VizError> {
        self.start_job(
            "export_aaf",
            format!("export_aaf({})", req.path),
            JobKindRow::ExportAaf,
            JobRequest::ExportAaf(req),
        )
    }
    fn export_aaf_check(&self, _req: AafExportRequestRow) -> Result<ReportRow, VizError> {
        self.check_report("export_aaf_check")
    }
    fn import_aaf(&self, req: AafImportRequestRow) -> Result<u64, VizError> {
        self.start_job(
            "import_aaf",
            format!("import_aaf({})", req.path),
            JobKindRow::ImportAaf,
            JobRequest::ImportAaf(req),
        )
    }
    fn aaf_folder_name(&self, seq: u64) -> Result<String, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "aaf_folder_name")?;
        Ok(format!("{} AAF", Self::song_name(&s, seq)?))
    }
    fn interchange_report(&self) -> Result<Option<ReportRow>, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "interchange_report")?;
        Ok(s.files.interchange_report.clone())
    }
    fn song_info(&self) -> Result<SongInfoRow, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "song_info")?;
        Ok(s.files.song_info.clone())
    }
    fn set_song_info(&self, info: SongInfoRow) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "set_song_info")?;
        s.calls.push("set_song_info".into());
        s.files.song_info = info;
        Ok(Self::command_result(&mut s))
    }
    fn recovered_files(&self) -> Result<Vec<RecoveredFileRow>, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "recovered_files")?;
        Ok(s.files.recovered.clone())
    }
    fn recover_recordings(&self, import: bool) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "recover_recordings")?;
        if s.files.recovered.is_empty() {
            return Err(VizError::Model {
                message: "there are no recovered recordings".into(),
            });
        }
        s.calls.push(format!("recover_recordings({import})"));
        s.files.recovered.clear();
        Ok(Self::command_result(&mut s))
    }
    fn co_producer_copies_size(&self) -> Result<CopiesSizeRow, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "co_producer_copies_size")?;
        Ok(s.files
            .copies
            .clone()
            .unwrap_or(CopiesSizeRow { count: 0, bytes: 0 }))
    }
    fn project_id(&self) -> Result<Option<String>, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "project_id")?;
        Ok(s.files.project_id.clone())
    }
    fn has_snapshot(&self, export_id: String) -> Result<bool, VizError> {
        let mut s = self.st();
        Self::read(&mut s, "has_snapshot")?;
        Ok(s.files.snapshot_ids.contains(&export_id))
    }
}

/// Every slot of every strip: instrument, EQ, inserts.
fn slots_mut(m: &mut MixerSnapshot) -> impl Iterator<Item = &mut SlotRow> {
    m.strips.iter_mut().flat_map(|s| {
        s.instrument
            .iter_mut()
            .chain(s.eq.iter_mut())
            .chain(s.inserts.iter_mut())
    })
}

/// A live, not bypassed CLAP slot named `name` ("V" its vendor), with
/// everything else defaulted.
pub fn slot_row(id: u64, name: &str) -> SlotRow {
    SlotRow {
        id,
        plugin: PluginRefRow {
            format: PluginFormatTag::Clap,
            id: format!("x.{name}"),
            name: name.into(),
            vendor: "V".into(),
            version: "1".into(),
        },
        instrument: false,
        bypass: false,
        sidechain: None,
        status: SlotStatus::Live,
        latency: 0,
        has_sidechain_input: false,
        outputs: 2,
        has_editor: true,
    }
}

/// An external MIDI destination with `mix`, everything else defaulted.
pub fn ext_dest(id: u64, name: &str, mix: MidiMixRow) -> DestRow {
    DestRow {
        external: true,
        midi_mix: mix,
        ..dest_row(id, name)
    }
}

/// An events block with everything else defaulted.
pub fn block_row(start: i64, end: i64, events: &[u64]) -> BlockRow {
    BlockRow {
        start,
        end,
        kind: BlockKind::Events,
        events: events.to_vec(),
        label: None,
        ref_seq: None,
        muted: false,
    }
}

/// [`demo_fake`] with Song's tracks filled (Swift `editorFake`): Chords
/// (31) holds note 1 at bar 1 in one block; Groove (30) holds a
/// reference block to Groove.
pub fn editor_fake() -> FakeSession {
    let f = demo_fake();
    f.set_events(3, 31, vec![EventRow::blank(1, 0, EventKindRow::Note)]);
    f.set_events(3, 30, vec![]);
    f.set_blocks(
        3,
        30,
        vec![BlockRow {
            kind: BlockKind::Reference,
            label: Some("Groove".into()),
            ref_seq: Some(2),
            ..block_row(0, 4 * BAR, &[7])
        }],
    );
    f.set_blocks(3, 31, vec![block_row(0, BAR, &[1])]);
    f
}

/// A 4/4, 120 BPM sequence info, 4 bars long (Swift `fourFour`).
pub fn four_four(seq: u64) -> SequenceInfo {
    SequenceInfo {
        id: seq,
        name: "S".into(),
        comment: String::new(),
        keyboard_slot: None,
        start_offset: 0,
        length_ticks: 4 * BAR,
        tempo: vec![TempoPointRow {
            tick: 0,
            bpm: 120.,
            ramp: false,
        }],
        meter: vec![MeterEntryRow {
            tick: 0,
            num: 4,
            den: 4,
            sharps: 0,
            minor: false,
        }],
        markers: vec![],
        generated: false,
    }
}

/// A note row: velocity 0x8000, a quarter long (Swift `noteRow`).
pub fn note_row(id: u64, tick: i64, pitch: u8) -> EventRow {
    EventRow {
        pitch,
        velocity: 0x8000,
        duration: 15_360,
        ..EventRow::blank(id, tick, EventKindRow::Note)
    }
}

/// A new bus strip named `name`, before master; its ID.
fn add_bus_strip(m: &mut MixerSnapshot, name: &str) -> u64 {
    let id = m.strips.iter().map(|s| s.id).max().unwrap_or(0) + 1;
    let at = m
        .strips
        .iter()
        .position(|s| s.kind == StripKindRow::Master)
        .unwrap_or(m.strips.len());
    m.strips.insert(at, strip_row(id, name, StripKindRow::Bus));
    m.buses.push(BusRow {
        id,
        name: name.to_owned(),
    });
    id
}

/// The kind colour the core gives `kind` (a bus taken as fed by an
/// output).
pub fn kind_colour(kind: &StripKindRow) -> StripColourRow {
    match kind {
        StripKindRow::BuiltinSynth
        | StripKindRow::Instrument { .. }
        | StripKindRow::InstrumentOutput { .. } => StripColourRow::Instrument,
        StripKindRow::AudioPath { .. } => StripColourRow::Channel,
        StripKindRow::Bus => StripColourRow::Bus,
        StripKindRow::Master => StripColourRow::Master,
    }
}

/// A strip at 0 dB, centre, Read, with everything else defaulted.
pub fn strip_row(id: u64, name: &str, kind: StripKindRow) -> StripRow {
    StripRow {
        id,
        name: name.into(),
        kind_label: format!("{kind:?}"),
        kind_colour: kind_colour(&kind),
        kind,
        instrument: None,
        inserts: vec![],
        gain_db: 0.0,
        pan: 0.0,
        mute: false,
        solo: false,
        solo_muted: false,
        sends: vec![],
        output: None,
        free_outputs: vec![],
        automation: AutomationModeRow::Read,
        eq: None,
        colour: None,
        co_producer: false,
        group: None,
        vcas: vec![],
        vca_offset_db: 0.0,
        vca_mark: false,
    }
}

/// [`demo_fake`] with a mixer (Swift `mixerFake`): Synth (1, the built-in
/// synth) and Bus 1 (2), then Master (0); bus 2.
pub fn mixer_fake() -> FakeSession {
    let f = demo_fake();
    f.set_mixer(MixerSnapshot {
        version: 0,
        strips: vec![
            strip_row(1, "Synth", StripKindRow::BuiltinSynth),
            strip_row(2, "Bus 1", StripKindRow::Bus),
            strip_row(0, "Master", StripKindRow::Master),
        ],
        buses: vec![BusRow {
            id: 2,
            name: "Bus 1".into(),
        }],
        groups: vec![],
        vcas: vec![],
    });
    f
}

/// A VCA row (blue, in Read) at `db`.
pub fn vca_row(id: u64, name: &str, members: &[u64], db: f64) -> VcaRow {
    VcaRow {
        id,
        name: name.into(),
        colour: SwatchRow::Blue,
        members: members.to_vec(),
        gain_db: db,
        mute: false,
        solo: false,
        automation: AutomationModeRow::Read,
    }
}

/// A red fader group with both links on.
pub fn group_row(id: u64, name: &str, members: &[u64]) -> GroupRow {
    GroupRow {
        id,
        name: name.into(),
        colour: SwatchRow::Red,
        members: members.to_vec(),
        link_mute: true,
        link_solo: true,
    }
}

/// [`demo_fake`] with the groups' mixer (Swift `MixerGroupsTests.groupsFake`):
/// Kick (2), Snare (3) and Bass (4), audio paths 1 to 3, Bus 1 (5) and
/// Master (0); each strip's `group` and `vcas` follow `groups` and `vcas`.
pub fn groups_fake(vcas: Vec<VcaRow>, groups: Vec<GroupRow>) -> FakeSession {
    let f = demo_fake();
    let mut strips = vec![
        strip_row(2, "Kick", StripKindRow::AudioPath { path: 1 }),
        strip_row(3, "Snare", StripKindRow::AudioPath { path: 2 }),
        strip_row(4, "Bass", StripKindRow::AudioPath { path: 3 }),
        strip_row(5, "Bus 1", StripKindRow::Bus),
        strip_row(0, "Master", StripKindRow::Master),
    ];
    for s in &mut strips {
        s.vcas = vcas
            .iter()
            .filter(|v| v.members.contains(&s.id))
            .map(|v| v.id)
            .collect();
        s.group = groups
            .iter()
            .find(|g| g.members.contains(&s.id))
            .map(|g| g.id);
    }
    f.set_mixer(MixerSnapshot {
        version: 0,
        strips,
        buses: vec![BusRow {
            id: 5,
            name: "Bus 1".into(),
        }],
        groups,
        vcas,
    });
    f
}

/// Every strip's mode, for an [`AutomationStateRow`].
pub fn modes_of(m: &MixerSnapshot) -> Vec<StripModeRow> {
    m.strips
        .iter()
        .map(|s| StripModeRow {
            strip: s.id,
            mode: s.automation,
        })
        .collect()
}

/// A destination row with everything else defaulted.
pub fn dest_row(id: u64, name: &str) -> DestRow {
    DestRow {
        id,
        name: name.into(),
        plugin: None,
        external: false,
        return_latency_ms: 0.,
        midi_mix: Default::default(),
        channel: None,
        mpe: None,
        builtin_sound: None,
        port: None,
    }
}

/// [`editor_fake`] for the detail editors (Swift `rollFake`): Song (3)
/// has Groove (30), Chords (31, destination 5: notes 1 at 0 and 2 at a
/// quarter) and Bass (32, destination 6: note 3 at 0); a 4/4, 120 BPM
/// map; destinations Piano (5) and Bass Synth (6).
pub fn roll_fake() -> FakeSession {
    let f = editor_fake();
    let mut chords = lane_row(31, "Chords", false);
    chords.midi_dest = Some(5);
    chords.midi_event_count = 2;
    let mut bass = lane_row(32, "Bass", false);
    bass.midi_dest = Some(6);
    bass.midi_event_count = 1;
    f.set_lanes(3, vec![lane_row(30, "Groove", false), chords, bass]);
    f.set_events(3, 31, vec![note_row(1, 0, 60), note_row(2, 15_360, 64)]);
    f.set_events(3, 32, vec![note_row(3, 0, 40)]);
    f.set_info(3, four_four(3));
    f.set_destinations(vec![dest_row(5, "Piano"), dest_row(6, "Bass Synth")]);
    f
}

pub fn seq(id: u64, name: &str, top_level: bool, uses: u32) -> SeqRow {
    SeqRow {
        id,
        name: name.into(),
        use_count: uses,
        top_level,
        length_ticks: 0,
        lane_count: 0,
        comment: String::new(),
        keyboard_slot: None,
    }
}

/// A lane row with everything else defaulted.
pub fn lane_row(id: u64, name: &str, mute: bool) -> LaneRow {
    LaneRow {
        id,
        name: name.into(),
        destination: None,
        midi_event_count: 0,
        region_count: 0,
        reference_count: 0,
        mute,
        solo: false,
        looped: false,
        take_count: 1,
        active_take: "Take 1".into(),
        is_drum: false,
        arm: false,
        midi_dest: None,
        audio_out: None,
        input: None,
        monitor: MonitorModeRow::Auto,
        kind: TrackKindRow::Midi,
        audio_out_name: None,
        input_fx: vec![],
        mpe: None,
        takes: vec![],
        active_take_id: 0,
        co_producer: false,
    }
}

/// Three sequences: Beat and Groove (referenced), Song (top level).
pub fn demo_fake() -> FakeSession {
    let f = FakeSession::new(vec![
        seq(1, "Beat", false, 1),
        seq(2, "Groove", false, 2),
        seq(3, "Song", true, 0),
    ]);
    f.set_lanes(1, vec![lane_row(10, "Kick", false)]);
    f.set_lanes(2, vec![lane_row(20, "Bass", false)]);
    f.set_lanes(
        3,
        vec![lane_row(30, "Groove", false), lane_row(31, "Chords", false)],
    );
    f
}

#[derive(Default)]
struct CoreState {
    calls: Vec<String>,
    call_errors: HashMap<String, VizError>,
    next_errors: HashMap<String, VecDeque<VizError>>,
    /// Sessions to hand out, by constructor name.
    sessions: HashMap<String, VecDeque<Arc<FakeSession>>>,
    made: Vec<(String, Arc<FakeSession>)>,
    dawproject: HashMap<String, DawprojectInfoRow>,
    aaf: HashMap<String, AafInfoRow>,
    recovery: HashMap<String, RecoveryInfo>,
    /// The plug-ins `disable_plugins` turned off, in order.
    disabled: Vec<(vizdaw_ffi::snapshot::PluginFormatTag, String)>,
}

/// A scripted `CoreCalls` (constructors and free functions): a call log,
/// scripted errors, the sessions it hands out and the file reads.
#[derive(Default)]
pub struct FakeCore(Mutex<CoreState>);

impl FakeCore {
    pub fn new() -> Self {
        FakeCore::default()
    }

    fn st(&self) -> std::sync::MutexGuard<'_, CoreState> {
        self.0.lock().unwrap()
    }

    /// Every call, in order, as `name` or `name(arg)`.
    pub fn calls(&self) -> Vec<String> {
        self.st().calls.clone()
    }

    /// Makes `call` answer `e` every time (`None`: normally again).
    pub fn set_call_error(&self, call: &str, e: Option<VizError>) {
        let mut s = self.st();
        match e {
            Some(e) => s.call_errors.insert(call.to_owned(), e),
            None => s.call_errors.remove(call),
        };
    }

    /// Makes the next `call` answer `e` (queued).
    pub fn fail_next(&self, call: &str, e: VizError) {
        self.st()
            .next_errors
            .entry(call.to_owned())
            .or_default()
            .push_back(e);
    }

    /// The next constructor `call` (`new_empty`, `new_demo`,
    /// `new_demo_audio`, `new_project`, `open`, `open_with`, `open_copy`)
    /// hands out `session` (queued). Unscripted, `new_demo` and
    /// `new_demo_audio` make a `demo_fake`, the others an empty fake; an
    /// opened one has its path.
    pub fn hand_out(&self, call: &str, session: Arc<FakeSession>) {
        self.st()
            .sessions
            .entry(call.to_owned())
            .or_default()
            .push_back(session);
    }

    /// Every session made, with the constructor that made it.
    pub fn made(&self) -> Vec<(String, Arc<FakeSession>)> {
        self.st().made.clone()
    }

    /// The last session made.
    pub fn last_made(&self) -> Option<Arc<FakeSession>> {
        self.st().made.last().map(|(_, s)| s.clone())
    }

    pub fn set_dawproject_info(&self, path: &str, row: DawprojectInfoRow) {
        self.st().dawproject.insert(path.to_owned(), row);
    }

    pub fn set_aaf_info(&self, path: &str, row: AafInfoRow) {
        self.st().aaf.insert(path.to_owned(), row);
    }

    /// The plug-ins `disable_plugins` turned off, by format and ID.
    pub fn disabled_plugins(&self) -> Vec<(vizdaw_ffi::snapshot::PluginFormatTag, String)> {
        self.st().disabled.clone()
    }

    /// What `recovery_check(path)` answers (default: nothing to ask).
    pub fn set_recovery(&self, path: &str, row: RecoveryInfo) {
        self.st().recovery.insert(path.to_owned(), row);
    }

    fn scripted(s: &mut CoreState, call: &str) -> Result<(), VizError> {
        if let Some(e) = s.next_errors.get_mut(call).and_then(VecDeque::pop_front) {
            return Err(e);
        }
        match s.call_errors.get(call) {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }

    fn make(
        &self,
        call: &str,
        logged: String,
        path: Option<&str>,
    ) -> Result<Arc<FakeSession>, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, call)?;
        s.calls.push(logged);
        let session = match s.sessions.get_mut(call).and_then(VecDeque::pop_front) {
            Some(f) => f,
            None => {
                let f = match call {
                    "new_demo" | "new_demo_audio" => demo_fake(),
                    _ => FakeSession::new(vec![]),
                };
                f.set_path(path);
                Arc::new(f)
            }
        };
        s.made.push((call.to_owned(), session.clone()));
        Ok(session)
    }

    fn info<T: Clone>(
        &self,
        call: &str,
        path: &str,
        pick: impl FnOnce(&CoreState) -> Option<T>,
    ) -> Result<Option<T>, VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, call)?;
        s.calls.push(format!("{call}({path})"));
        Ok(pick(&s))
    }
}

fn as_doc(f: Arc<FakeSession>) -> Arc<dyn DocSession> {
    f
}

fn unreadable(path: &str) -> VizError {
    VizError::Io {
        message: format!("{path} could not be read"),
    }
}

impl CoreCalls for FakeCore {
    fn new_empty(&self) -> Result<Arc<dyn DocSession>, VizError> {
        self.make("new_empty", "new_empty".into(), None).map(as_doc)
    }
    fn new_demo(&self) -> Result<Arc<dyn DocSession>, VizError> {
        self.make("new_demo", "new_demo".into(), None).map(as_doc)
    }
    fn new_demo_audio(&self) -> Result<Arc<dyn DocSession>, VizError> {
        self.make("new_demo_audio", "new_demo_audio".into(), None)
            .map(as_doc)
    }
    fn new_project(&self, format: ProjectFormatRow) -> Result<Arc<dyn DocSession>, VizError> {
        let made = self.make(
            "new_project",
            format!("new_project({},{:?})", format.sample_rate, format.depth),
            None,
        )?;
        made.set_project_info(ProjectInfoRow {
            format,
            ..default_project_info()
        });
        Ok(made)
    }
    fn open(&self, path: &str) -> Result<Arc<dyn DocSession>, VizError> {
        self.make("open", format!("open({path})"), Some(path))
            .map(as_doc)
    }
    fn open_with(&self, path: &str, options: OpenOptions) -> Result<Arc<dyn DocSession>, VizError> {
        self.make(
            "open_with",
            format!(
                "open_with({path},plugins_off={},autosave={:?})",
                options.plugins_off, options.autosave
            ),
            Some(path),
        )
        .map(as_doc)
    }
    fn open_copy(&self, path: &str) -> Result<Arc<dyn DocSession>, VizError> {
        self.make("open_copy", format!("open_copy({path})"), Some(path))
            .map(as_doc)
    }
    fn dawproject_info(&self, path: &str) -> Result<DawprojectInfoRow, VizError> {
        self.info("dawproject_info", path, |s| s.dawproject.get(path).cloned())?
            .ok_or_else(|| unreadable(path))
    }
    fn aaf_info(&self, path: &str) -> Result<AafInfoRow, VizError> {
        self.info("aaf_info", path, |s| s.aaf.get(path).cloned())?
            .ok_or_else(|| unreadable(path))
    }
    fn recovery_check(&self, path: &str) -> Result<RecoveryInfo, VizError> {
        Ok(self
            .info("recovery_check", path, |s| s.recovery.get(path).cloned())?
            .unwrap_or(RecoveryInfo {
                crashed: false,
                loading: vec![],
                autosave: None,
            }))
    }
    fn project_rates(&self) -> Vec<u32> {
        vizdaw_ffi::project_rates()
    }
    fn format_label(&self, format: ProjectFormatRow) -> String {
        vizdaw_ffi::format_label(format)
    }
    fn bounce_settings(&self) -> BounceSettingsRow {
        vizdaw_ffi::bounce_settings()
    }
    fn disable_plugins(&self, plugins: &[PluginRefRow]) -> Result<(), VizError> {
        let mut s = self.st();
        Self::scripted(&mut s, "disable_plugins")?;
        for p in plugins {
            s.calls.push(format!("disable_plugin({})", p.id));
            s.disabled.push((p.format, p.id.clone()));
        }
        Ok(())
    }
}
