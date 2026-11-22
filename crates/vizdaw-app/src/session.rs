//! The slice of the core session the document model uses.

use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::EditCommand;
use vizdaw_ffi::selection::SelectionRow;
use vizdaw_ffi::session::Session;
use vizdaw_ffi::snapshot::{DocSnapshot, LaneRow, TransportState};
use vizdaw_ffi::tracks::TrackKindRow;

/// What the app logic needs from a core session. The real `Session`
/// implements it; tests use a scripted fake.
pub trait DocSession: Send + Sync {
    /// The current package path, `None` when untitled.
    fn path(&self) -> Result<Option<String>, VizError>;
    fn save(&self) -> Result<(), VizError>;
    fn save_as(&self, path: String) -> Result<(), VizError>;
    fn snapshot(&self) -> Result<DocSnapshot, VizError>;
    fn undo(&self) -> Result<DocSnapshot, VizError>;
    fn redo(&self) -> Result<DocSnapshot, VizError>;
    fn transport(&self) -> Result<TransportState, VizError>;
    fn play(&self, seq: u64) -> Result<(), VizError>;
    fn play_from(&self, seq: u64, from: i64, to: Option<i64>) -> Result<(), VizError>;
    fn stop(&self) -> Result<(), VizError>;
    fn locate(&self, tick: i64) -> Result<(), VizError>;
    fn close(&self) -> Result<(), VizError>;
    /// The live unsaved state (the snapshot's flag can lag behind).
    fn has_unsaved_changes(&self) -> Result<bool, VizError>;
    /// A plugin timer tick: the new document when gestures became undo steps.
    fn idle(&self) -> Result<Option<DocSnapshot>, VizError>;
    fn lanes(&self, seq: u64) -> Result<Vec<LaneRow>, VizError>;
    fn new_track(
        &self,
        seq: u64,
        kind: TrackKindRow,
        after: Option<u64>,
    ) -> Result<DocSnapshot, VizError>;
    fn duplicate_track(&self, seq: u64, lane: u64) -> Result<DocSnapshot, VizError>;
    fn edit(&self, cmd: EditCommand) -> Result<DocSnapshot, VizError>;
    fn set_lane_mute(&self, seq: u64, lane: u64, mute: bool) -> Result<DocSnapshot, VizError>;
    fn selection(&self) -> Result<SelectionRow, VizError>;
    fn set_selection(&self, s: SelectionRow) -> Result<(), VizError>;
}

impl DocSession for Session {
    fn path(&self) -> Result<Option<String>, VizError> {
        Session::path(self)
    }
    fn save(&self) -> Result<(), VizError> {
        Session::save(self)
    }
    fn save_as(&self, path: String) -> Result<(), VizError> {
        Session::save_as(self, path)
    }
    fn snapshot(&self) -> Result<DocSnapshot, VizError> {
        Session::snapshot(self)
    }
    fn undo(&self) -> Result<DocSnapshot, VizError> {
        Session::undo(self)
    }
    fn redo(&self) -> Result<DocSnapshot, VizError> {
        Session::redo(self)
    }
    fn transport(&self) -> Result<TransportState, VizError> {
        Session::transport(self)
    }
    fn play(&self, seq: u64) -> Result<(), VizError> {
        Session::play(self, seq)
    }
    fn play_from(&self, seq: u64, from: i64, to: Option<i64>) -> Result<(), VizError> {
        Session::play_from(self, seq, from, to)
    }
    fn stop(&self) -> Result<(), VizError> {
        Session::stop(self)
    }
    fn locate(&self, tick: i64) -> Result<(), VizError> {
        Session::locate(self, tick)
    }
    fn close(&self) -> Result<(), VizError> {
        Session::close(self)
    }
    fn has_unsaved_changes(&self) -> Result<bool, VizError> {
        Session::has_unsaved_changes(self)
    }
    fn idle(&self) -> Result<Option<DocSnapshot>, VizError> {
        Session::idle(self).map(|u| u.snapshot)
    }
    fn lanes(&self, seq: u64) -> Result<Vec<LaneRow>, VizError> {
        Session::lanes(self, seq)
    }
    fn new_track(
        &self,
        seq: u64,
        kind: TrackKindRow,
        after: Option<u64>,
    ) -> Result<DocSnapshot, VizError> {
        Session::new_track(self, seq, kind, after)
    }
    fn duplicate_track(&self, seq: u64, lane: u64) -> Result<DocSnapshot, VizError> {
        Session::duplicate_track(self, seq, lane)
    }
    fn edit(&self, cmd: EditCommand) -> Result<DocSnapshot, VizError> {
        Session::edit(self, cmd, None)
    }
    fn set_lane_mute(&self, seq: u64, lane: u64, mute: bool) -> Result<DocSnapshot, VizError> {
        Session::set_lane_mute(self, seq, lane, mute)
    }
    fn selection(&self) -> Result<SelectionRow, VizError> {
        Session::selection(self)
    }
    fn set_selection(&self, s: SelectionRow) -> Result<(), VizError> {
        Session::set_selection(self, s)
    }
}
