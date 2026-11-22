//! The slice of the core session the document model uses.

use vizdaw_ffi::VizError;
use vizdaw_ffi::blocks::{BlockRow, BlockRule};
use vizdaw_ffi::edit::EditCommand;
use vizdaw_ffi::events::{MarkerRow, SequenceInfo};
use vizdaw_ffi::mixer::DestRow;
use vizdaw_ffi::selection::{SelectionRow, TickRange};
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
    /// Turns the loop (the whole sequence) on or off.
    fn set_loop(&self, on: bool) -> Result<(), VizError>;
    /// The marker nearest `tick` in the given direction.
    fn marker_step(&self, seq: u64, tick: i64, forward: bool) -> Result<Option<i64>, VizError>;
    /// One bar on or back; the new tick.
    fn transport_step_bar(&self, forward: bool) -> Result<i64, VizError>;
    /// A held key's bar step (landed by `transport_hold_end`).
    fn transport_key_step(&self, forward: bool) -> Result<i64, VizError>;
    /// A held button's wind by `quarters` (negative: back).
    fn transport_wind(&self, quarters: f64) -> Result<i64, VizError>;
    /// The end of a hold: lands an open pass once.
    fn transport_hold_end(&self) -> Result<i64, VizError>;
    /// The time span the selection covers in `seq`, if any.
    fn selection_span(&self, seq: u64) -> Result<Option<TickRange>, VizError>;
    /// What rulers and readouts need about `seq` (meter map, markers).
    fn sequence_info(&self, seq: u64) -> Result<SequenceInfo, VizError>;
    /// The arrangement blocks of one track.
    fn blocks(&self, seq: u64, lane: u64, rule: BlockRule) -> Result<Vec<BlockRow>, VizError>;
    /// Sequence `seq`'s markers, in order.
    fn markers(&self, seq: u64) -> Result<Vec<MarkerRow>, VizError>;
    /// The destinations, with their return latency and MIDI mix.
    fn destinations(&self) -> Result<Vec<DestRow>, VizError>;
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
    fn set_loop(&self, on: bool) -> Result<(), VizError> {
        Session::set_loop(self, on)
    }
    fn marker_step(&self, seq: u64, tick: i64, forward: bool) -> Result<Option<i64>, VizError> {
        Session::marker_step(self, seq, tick, forward)
    }
    fn transport_step_bar(&self, forward: bool) -> Result<i64, VizError> {
        Session::transport_step_bar(self, forward)
    }
    fn transport_key_step(&self, forward: bool) -> Result<i64, VizError> {
        Session::transport_key_step(self, forward)
    }
    fn transport_wind(&self, quarters: f64) -> Result<i64, VizError> {
        Session::transport_wind(self, quarters)
    }
    fn transport_hold_end(&self) -> Result<i64, VizError> {
        Session::transport_hold_end(self)
    }
    fn selection_span(&self, seq: u64) -> Result<Option<TickRange>, VizError> {
        Session::selection_span(self, seq)
    }
    fn sequence_info(&self, seq: u64) -> Result<SequenceInfo, VizError> {
        Session::sequence_info(self, seq)
    }
    fn blocks(&self, seq: u64, lane: u64, rule: BlockRule) -> Result<Vec<BlockRow>, VizError> {
        Session::blocks(self, seq, lane, rule)
    }
    fn markers(&self, seq: u64) -> Result<Vec<MarkerRow>, VizError> {
        Session::sequence_info(self, seq).map(|i| i.markers)
    }
    fn destinations(&self) -> Result<Vec<DestRow>, VizError> {
        Session::destinations(self)
    }
}
