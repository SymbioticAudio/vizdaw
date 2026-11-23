//! The slice of the core session the document model uses.

use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::{
    AutomationModeRow, AutomationStateRow, GestureHolderRow, GesturePhaseRow,
};
use vizdaw_ffi::blocks::{BlockRow, BlockRule};
use vizdaw_ffi::control::ControlArg;
use vizdaw_ffi::edit::EditCommand;
use vizdaw_ffi::events::{AutomationTargetRow, LaneEvents, MarkerRow, SequenceInfo};
use vizdaw_ffi::mixer::{DestRow, MixerLevels, MixerSnapshot, SwatchRow};
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
    /// One track's events, in time order (the arrangement's note sketch).
    fn lane_events(&self, seq: u64, lane: u64) -> Result<LaneEvents, VizError>;
    /// Sequence `seq`'s markers, in order.
    fn markers(&self, seq: u64) -> Result<Vec<MarkerRow>, VizError>;
    /// The destinations, with their return latency and MIDI mix.
    fn destinations(&self) -> Result<Vec<DestRow>, VizError>;
    /// Opens an edit (a drag) that becomes one undo step named `label`.
    fn begin_edit(&self, label: &str) -> Result<(), VizError>;
    /// Inside an open edit: rewinds it and runs `cmd` (a drag's whole
    /// target so far).
    fn edit_gesture(&self, cmd: EditCommand) -> Result<DocSnapshot, VizError>;
    /// Closes the open edit as one undo step.
    fn end_edit(&self) -> Result<DocSnapshot, VizError>;
    /// Drops the open edit; the document is as it was before it.
    fn abort_edit(&self) -> Result<DocSnapshot, VizError>;
    /// Runs `cmds` as one undo step named `label`: all or none.
    fn edit_all(&self, label: &str, cmds: Vec<EditCommand>) -> Result<DocSnapshot, VizError>;
    /// Each tick's time in seconds under `seq`'s tempo map.
    fn seconds_at(&self, seq: u64, ticks: Vec<i64>) -> Result<Vec<f64>, VizError>;
    /// Each time's tick under `seq`'s tempo map.
    fn ticks_at(&self, seq: u64, seconds: Vec<f64>) -> Result<Vec<i64>, VizError>;
    /// The package's `ui.json` as the session holds it (`None`: none yet).
    fn ui_layout(&self) -> Result<Option<String>, VizError>;
    /// Hands `ui.json` to the session; the next save writes it.
    fn set_ui_layout(&self, json: String) -> Result<(), VizError>;
    /// Sounds (`on`) or releases `pitch` on destination `dest`, touching
    /// no document. False: this session does not hold the engine, or the
    /// loaded sequence does not play on `dest`.
    fn audition(&self, dest: u64, pitch: u8, velocity: u16, on: bool) -> Result<bool, VizError>;

    // The mixer (slice 6a).

    /// The whole mixer: strips in display order (master last), buses,
    /// groups and VCAs.
    fn mixer(&self) -> Result<MixerSnapshot, VizError>;
    /// The peaks since the last read, DSP load and faults; empty for a
    /// session that does not hold the engine.
    fn levels(&self) -> Result<MixerLevels, VizError>;
    /// A gesture phase on `target` from the mouse.
    fn gesture(&self, target: AutomationTargetRow, phase: GesturePhaseRow) -> Result<(), VizError>;
    /// A gesture phase from `holder`; `alone` moves the target without its
    /// group.
    fn gesture_as(
        &self,
        target: AutomationTargetRow,
        phase: GesturePhaseRow,
        holder: GestureHolderRow,
        alone: bool,
    ) -> Result<(), VizError>;
    /// The modes, the targets touched now and their values.
    fn automation_state(&self) -> Result<AutomationStateRow, VizError>;
    /// Sets `strips`' automation mode as one undo step.
    fn set_automation_mode(
        &self,
        strips: Vec<u64>,
        mode: AutomationModeRow,
    ) -> Result<DocSnapshot, VizError>;
    /// While playing, each target's heard value (dB, −1…1 for a pan);
    /// stopped, all `None`. At most 4096 targets.
    fn heard_values(&self, targets: Vec<AutomationTargetRow>)
    -> Result<Vec<Option<f64>>, VizError>;
    /// Runs a `mixer.*` (or group) registry command as one undo step.
    fn run_mixer(&self, id: &str, args: Vec<ControlArg>) -> Result<DocSnapshot, VizError>;
    fn add_bus(&self, name: &str) -> Result<DocSnapshot, VizError>;
    /// A bus fed by a post-fader send from `strip`, one undo step "New Bus".
    fn new_bus_from(&self, strip: u64, name: &str) -> Result<DocSnapshot, VizError>;
    fn remove_bus(&self, strip: u64) -> Result<DocSnapshot, VizError>;
    fn rename_strip(&self, strip: u64, name: &str) -> Result<DocSnapshot, VizError>;
    /// Routes `strip` to `bus` (`None`: master).
    fn set_output(&self, strip: u64, bus: Option<u64>) -> Result<DocSnapshot, VizError>;
    fn set_strip_colour(
        &self,
        strip: u64,
        colour: Option<SwatchRow>,
    ) -> Result<DocSnapshot, VizError>;
    /// Releases every touch the control channel holds; how many.
    fn release_channel_touches(&self) -> Result<u32, VizError>;
    /// The app's selected strips, for the control surfaces.
    fn set_surface_selection(&self, strips: Vec<u64>) -> Result<(), VizError>;
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
    fn lane_events(&self, seq: u64, lane: u64) -> Result<LaneEvents, VizError> {
        Session::lane_events(self, seq, lane)
    }
    fn markers(&self, seq: u64) -> Result<Vec<MarkerRow>, VizError> {
        Session::sequence_info(self, seq).map(|i| i.markers)
    }
    fn destinations(&self) -> Result<Vec<DestRow>, VizError> {
        Session::destinations(self)
    }
    fn begin_edit(&self, label: &str) -> Result<(), VizError> {
        Session::begin_edit(self, label.to_owned())
    }
    fn edit_gesture(&self, cmd: EditCommand) -> Result<DocSnapshot, VizError> {
        Session::edit_gesture(self, cmd, None)
    }
    fn end_edit(&self) -> Result<DocSnapshot, VizError> {
        Session::end_edit(self)
    }
    fn abort_edit(&self) -> Result<DocSnapshot, VizError> {
        Session::abort_edit(self)
    }
    fn edit_all(&self, label: &str, cmds: Vec<EditCommand>) -> Result<DocSnapshot, VizError> {
        Session::edit_all(self, label.to_owned(), cmds, None)
    }
    fn seconds_at(&self, seq: u64, ticks: Vec<i64>) -> Result<Vec<f64>, VizError> {
        Session::seconds_at(self, seq, ticks)
    }
    fn ticks_at(&self, seq: u64, seconds: Vec<f64>) -> Result<Vec<i64>, VizError> {
        Session::ticks_at(self, seq, seconds)
    }
    fn ui_layout(&self) -> Result<Option<String>, VizError> {
        Session::ui_layout(self)
    }
    fn set_ui_layout(&self, json: String) -> Result<(), VizError> {
        Session::set_ui_layout(self, json)
    }
    fn audition(&self, dest: u64, pitch: u8, velocity: u16, on: bool) -> Result<bool, VizError> {
        Session::audition(self, dest, pitch, velocity, on)
    }
    fn mixer(&self) -> Result<MixerSnapshot, VizError> {
        Session::mixer(self)
    }
    fn levels(&self) -> Result<MixerLevels, VizError> {
        Session::levels(self)
    }
    fn gesture(&self, target: AutomationTargetRow, phase: GesturePhaseRow) -> Result<(), VizError> {
        Session::gesture(self, target, phase)
    }
    fn gesture_as(
        &self,
        target: AutomationTargetRow,
        phase: GesturePhaseRow,
        holder: GestureHolderRow,
        alone: bool,
    ) -> Result<(), VizError> {
        Session::gesture_as(self, target, phase, holder, alone)
    }
    fn automation_state(&self) -> Result<AutomationStateRow, VizError> {
        Session::automation_state(self)
    }
    fn set_automation_mode(
        &self,
        strips: Vec<u64>,
        mode: AutomationModeRow,
    ) -> Result<DocSnapshot, VizError> {
        Session::set_automation_mode(self, strips, mode)
    }
    fn heard_values(
        &self,
        targets: Vec<AutomationTargetRow>,
    ) -> Result<Vec<Option<f64>>, VizError> {
        Session::heard_values(self, targets)
    }
    fn run_mixer(&self, id: &str, args: Vec<ControlArg>) -> Result<DocSnapshot, VizError> {
        Session::run_mixer(self, id.to_owned(), args)
    }
    fn add_bus(&self, name: &str) -> Result<DocSnapshot, VizError> {
        Session::add_bus(self, name.to_owned())
    }
    fn new_bus_from(&self, strip: u64, name: &str) -> Result<DocSnapshot, VizError> {
        Session::new_bus_from(self, strip, name.to_owned())
    }
    fn remove_bus(&self, strip: u64) -> Result<DocSnapshot, VizError> {
        Session::remove_bus(self, strip)
    }
    fn rename_strip(&self, strip: u64, name: &str) -> Result<DocSnapshot, VizError> {
        Session::rename_strip(self, strip, name.to_owned())
    }
    fn set_output(&self, strip: u64, bus: Option<u64>) -> Result<DocSnapshot, VizError> {
        Session::set_output(self, strip, bus)
    }
    fn set_strip_colour(
        &self,
        strip: u64,
        colour: Option<SwatchRow>,
    ) -> Result<DocSnapshot, VizError> {
        Session::set_strip_colour(self, strip, colour)
    }
    fn release_channel_touches(&self) -> Result<u32, VizError> {
        Session::release_channel_touches(self)
    }
    fn set_surface_selection(&self, strips: Vec<u64>) -> Result<(), VizError> {
        Session::set_surface_selection(self, strips)
    }
}
