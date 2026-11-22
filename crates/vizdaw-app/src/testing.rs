//! A scripted `DocSession` (Swift: `FakeSession`): scripted errors, a busy
//! flag and a call log. Behind the `test-support` feature so the GPUI
//! crate's tests drive the same fake as this crate's.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use crate::DocSession;
use vizdaw_ffi::VizError;
use vizdaw_ffi::blocks::{BlockKind, BlockRow, BlockRule};
use vizdaw_ffi::edit::{EditCommand, LaneEdit, LaneRef, MonitorModeRow};
use vizdaw_ffi::events::{
    EventKindRow, EventRow, LaneEvents, MarkerRow, MeterEntryRow, SequenceInfo,
};
use vizdaw_ffi::mixer::DestRow;
use vizdaw_ffi::revs::LaneRev;
use vizdaw_ffi::selection::{SelectionRow, TickRange};
use vizdaw_ffi::snapshot::{DocSnapshot, LaneRow, SeqRow, TransportState};
use vizdaw_ffi::tracks::TrackKindRow;

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
    /// `set_ui_layout`) answer `e` every time (`None`: normally again).
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

    /// Every `edit_all(label, cmds)`, in order.
    pub fn edit_alls(&self) -> Vec<(String, Vec<EditCommand>)> {
        self.st().edit_alls.clone()
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
    fn idle(&self) -> Result<Option<DocSnapshot>, VizError> {
        let mut s = self.st();
        let n = s.idle_snapshot.take();
        if let Some(n) = &n {
            s.snap = n.clone();
        }
        Ok(n)
    }
    fn lanes(&self, seq: u64) -> Result<Vec<LaneRow>, VizError> {
        Ok(self.st().lanes.get(&seq).cloned().unwrap_or_default())
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
        let s = self.st();
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
        Ok(vec![])
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

fn seq(id: u64, name: &str, top_level: bool, uses: u32) -> SeqRow {
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
