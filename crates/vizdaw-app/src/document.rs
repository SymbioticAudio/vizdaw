//! One open document (Swift: `DocumentModel`, the title, save and undo
//! parts; transport comes with Task 3).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use vizdaw_ffi::VizError;
use vizdaw_ffi::blocks::BlockRow;
use vizdaw_ffi::edit::{EditCommand, LaneEdit, LaneRef};
use vizdaw_ffi::events::{EventRow, SequenceInfo};
use vizdaw_ffi::selection::{SelectionRow, TickRange};
use vizdaw_ffi::snapshot::{DocSnapshot, LaneRow, TransportState};

use crate::alerts::{AlertItem, error_message};
use crate::session::DocSession;

/// What File > Save does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveAction {
    Saved,
    /// Untitled: ask for a location; nothing was written.
    NeedsSaveAs,
    Failed(String),
}

pub struct DocumentModel {
    session: Arc<dyn DocSession>,
    snapshot: DocSnapshot,
    /// The error showing, if any.
    pub alert: Option<AlertItem>,
    selected_seq: Option<u64>,
    lanes: Vec<LaneRow>,
    /// What the lanes were fetched for (Swift: `LanesKey`).
    lanes_key: Option<(u64, Option<u64>)>,
    lanes_fetches: u64,
    selection: SelectionRow,
    status: Option<String>,
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
    /// Each shown track's blocks and events, read once per lanes fetch.
    lane_reads: Vec<(u64, LaneReads)>,
}

/// What the arrangement reads of one track, read when first asked for.
#[derive(Default)]
struct LaneReads {
    blocks: OnceLock<Arc<[BlockRow]>>,
    events: OnceLock<Arc<[EventRow]>>,
}

impl DocumentModel {
    pub fn new(session: Arc<dyn DocSession>) -> Result<Self, VizError> {
        let snapshot = session.snapshot()?;
        let transport = session
            .transport()
            .unwrap_or_else(|_| crate::transport::stopped());
        let selection = session.selection().unwrap_or_else(|_| empty_selection());
        let mut model = DocumentModel {
            session,
            selected_seq: first_sequence(&snapshot),
            snapshot,
            alert: None,
            lanes: vec![],
            lanes_key: None,
            lanes_fetches: 0,
            selection,
            status: None,
            looping: false,
            text_editors: BTreeSet::new(),
            play_start: Arc::new(Mutex::new(None)),
            infos: vec![],
            transport: Arc::new(Mutex::new(transport)),
            lane_reads: vec![],
        };
        model.reset_infos();
        model.refresh_lanes();
        Ok(model)
    }

    /// Takes a new snapshot: keeps the shown sequence if it still exists
    /// (else the first top-level one) and refetches lanes if needed.
    pub fn apply(&mut self, s: DocSnapshot) {
        if !s.edit_notes.is_empty() {
            self.status = Some(s.edit_notes.join(" "));
        }
        let gone = self
            .selected_seq
            .is_none_or(|id| !s.sequences.iter().any(|q| q.id == id));
        if gone {
            self.selected_seq = first_sequence(&s);
        }
        self.snapshot = s;
        self.reset_infos();
        self.refresh_lanes();
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

    /// One plugin-timer tick. True when a new snapshot arrived.
    pub fn idle(&mut self) -> bool {
        match self.session.idle() {
            Ok(Some(s)) => {
                self.apply(s);
                true
            }
            _ => false,
        }
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
    }

    /// The selection as the model holds it.
    pub fn selection(&self) -> &SelectionRow {
        &self.selection
    }

    pub fn lanes(&self) -> &[LaneRow] {
        &self.lanes
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
        let want = SelectionRow {
            time_range: self.selection.time_range,
            lanes,
            events: vec![],
        };
        self.selection = want.clone();
        if let Err(e) = self.session.set_selection(want) {
            self.alert = Some(AlertItem::from_error(&e, "Selection Failed"));
        }
        // Hold exactly what the session holds (it drops unknown lanes).
        self.refresh_selection();
    }

    /// Sets the selection's time range (the Edit In and Out fields); the
    /// tracks and events stay. A failure is the alert "Selection Failed".
    pub fn set_time_range(&mut self, range: TickRange) {
        let want = SelectionRow {
            time_range: Some(range),
            ..self.selection.clone()
        };
        self.selection = want.clone();
        if let Err(e) = self.session.set_selection(want) {
            self.alert = Some(AlertItem::from_error(&e, "Selection Failed"));
        }
        self.refresh_selection();
    }

    /// A ruler drag: the time range over every track of the shown
    /// sequence, no events (Swift `EditorModel.selectRange`). A failure is
    /// the alert "Selection Failed".
    pub fn select_range(&mut self, range: TickRange) {
        let Some(seq) = self.selected_seq else { return };
        let want = SelectionRow {
            time_range: Some(range),
            lanes: self
                .lanes
                .iter()
                .map(|l| LaneRef { seq, lane: l.id })
                .collect(),
            events: vec![],
        };
        self.selection = want.clone();
        if let Err(e) = self.session.set_selection(want) {
            self.alert = Some(AlertItem::from_error(&e, "Selection Failed"));
        }
        self.refresh_selection();
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

    /// Track `lane`'s events in time order, read once per snapshot; none
    /// while the core cannot answer.
    pub fn lane_events(&self, lane: u64) -> Arc<[EventRow]> {
        let Some((seq, reads)) = self.reads(lane) else {
            return Arc::from([]);
        };
        read_once(&reads.events, || {
            self.session.lane_events(seq, lane).map(|e| e.events)
        })
    }

    fn reads(&self, lane: u64) -> Option<(u64, &LaneReads)> {
        let seq = self.selected_seq?;
        let (_, reads) = self.lane_reads.iter().find(|(id, _)| *id == lane)?;
        Some((seq, reads))
    }

    /// Reads the selection back (after edits, undo and redo).
    fn refresh_selection(&mut self) {
        if let Ok(s) = self.session.selection() {
            self.selection = s;
        }
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

    /// Space and the other bare transport keys work: no field is typed in.
    pub fn transport_keys_enabled(&self) -> bool {
        self.text_editors.is_empty()
    }

    /// Return (Return to Start) also waits for a list that has the
    /// keyboard, once the editors have lists.
    pub fn return_key_enabled(&self) -> bool {
        self.transport_keys_enabled()
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

    /// Reads the transport. A read that fails (Busy included) leaves the
    /// last good one in place, and is returned for the caller to judge.
    pub fn poll_transport(&self) -> Result<TransportState, VizError> {
        let t = self.session.transport()?;
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
            self.lanes = vec![];
            return;
        };
        self.lanes_fetches += 1;
        match self.session.lanes(seq) {
            Ok(rows) => self.lanes = rows,
            Err(e) => {
                self.lanes = vec![];
                self.alert = Some(AlertItem::from_error(&e, "Tracks Unavailable"));
            }
        }
        self.lane_reads = self
            .lanes
            .iter()
            .map(|l| (l.id, LaneReads::default()))
            .collect();
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

    /// Undo and Redo do nothing, silently, while the plugin host is busy.
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
        match body(self.session.as_ref()) {
            Ok(snapshot) => {
                self.apply(snapshot);
                self.refresh_selection();
            }
            Err(VizError::Busy { .. }) => {}
            Err(e) => self.alert = Some(AlertItem::from_error(&e, title)),
        }
    }

    /// Saves in place. An untitled document is never written: the caller
    /// asks for a location and calls `save_as`.
    pub fn save(&mut self) -> SaveAction {
        if self.path().is_none() {
            return SaveAction::NeedsSaveAs;
        }
        match self.session.save().and_then(|()| self.session.snapshot()) {
            Ok(snapshot) => {
                self.apply(snapshot);
                SaveAction::Saved
            }
            Err(e) => {
                self.alert = Some(AlertItem::from_error(&e, "Save Failed"));
                SaveAction::Failed(error_message(&e))
            }
        }
    }

    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
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
