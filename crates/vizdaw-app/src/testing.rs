//! A scripted `DocSession` (Swift: `FakeSession`): scripted errors, a busy
//! flag and a call log. Behind the `test-support` feature so the GPUI
//! crate's tests drive the same fake as this crate's.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::DocSession;
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, LaneEdit, LaneRef, MonitorModeRow};
use vizdaw_ffi::selection::SelectionRow;
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
}

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
        }))
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap()
    }

    pub fn snap(&self) -> DocSnapshot {
        self.st().snap.clone()
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

    /// Every `edit` command received, in order.
    pub fn edits(&self) -> Vec<EditCommand> {
        self.st().edits.clone()
    }

    /// Every call, in order, as `name` or `name(arg)`.
    pub fn calls(&self) -> Vec<String> {
        self.st().calls.clone()
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
        s.snap.clone()
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
        Ok(self.st().snap.clone())
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
        s.calls.push("set_selection".into());
        // Like the core: lanes that do not exist are dropped.
        let known = |s: &State, r: &LaneRef| {
            s.lanes
                .get(&r.seq)
                .is_some_and(|rows| rows.iter().any(|l| l.id == r.lane))
        };
        sel.lanes.retain(|r| known(&s, r));
        s.selection = sel;
        Ok(())
    }
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
