//! One open document (Swift: `DocumentModel`, the title, save and undo
//! parts; transport comes with Task 3).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, LaneEdit, LaneRef};
use vizdaw_ffi::selection::SelectionRow;
use vizdaw_ffi::snapshot::{DocSnapshot, LaneRow};

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
}

impl DocumentModel {
    pub fn new(session: Arc<dyn DocSession>) -> Result<Self, VizError> {
        let snapshot = session.snapshot()?;
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
        };
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
        self.refresh_lanes();
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

    /// Reads the selection back (after edits, undo and redo).
    fn refresh_selection(&mut self) {
        if let Ok(s) = self.session.selection() {
            self.selection = s;
        }
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
