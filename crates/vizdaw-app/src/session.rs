//! The slice of the core session the document model uses.

use std::sync::Arc;

use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::{
    AutomationModeRow, AutomationStateRow, GestureHolderRow, GesturePhaseRow,
};
use vizdaw_ffi::blocks::{BlockRow, BlockRule};
use vizdaw_ffi::control::ControlArg;
use vizdaw_ffi::edit::EditCommand;
use vizdaw_ffi::events::{AutomationTargetRow, LaneEvents, MarkerRow, SequenceInfo};
use vizdaw_ffi::mixer::{DestRow, MidiMixRow, MixerLevels, MixerSnapshot, SendRow, SwatchRow};
use vizdaw_ffi::record::RecoveredFileRow;
use vizdaw_ffi::selection::{SelectionRow, TickRange};
use vizdaw_ffi::session::CopiesSizeRow;
use vizdaw_ffi::session::Session;
use vizdaw_ffi::snapshot::{DocSnapshot, LaneRow, PluginRefRow, TransportState};
use vizdaw_ffi::tracks::TrackKindRow;
use vizdaw_ffi::{
    AafExportRequestRow, AafImportRequestRow, AafInfoRow, BounceCheckRow, BounceRequestRow,
    BounceResultRow, BounceSettingsRow, ClipExportRequestRow, ClipNamesRow,
    DawprojectExportRequestRow, DawprojectImportRequestRow, DawprojectInfoRow,
    DawprojectMergeRequestRow, ForDawCheckRow, ForDawRequestRow, ImportTargetRow, JobRow, MediaRow,
    MidiFileInfoRow, MidiImportRequestRow, OpenOptions, ProjectFormatRow, ProjectInfoRow,
    RecoveryInfo, ReportRow, SmfExportRequestRow, SnapshotRow, SongInfoRow,
};

/// What a plugin timer tick brought (the core's `IdleUpdate`, the parts
/// the app reads).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IdleNews {
    /// The document, when plugin-UI gestures became undo steps.
    pub snapshot: Option<DocSnapshot>,
    /// Slots finished loading, failed, restarted or came back: the mixer
    /// is read again.
    pub slots_changed: bool,
    /// The media rows changed (a job finished, media went offline).
    pub media_changed: bool,
}

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
    /// A plugin timer tick: the new document when gestures became undo
    /// steps, and whether the slots changed.
    fn idle(&self) -> Result<IdleNews, VizError>;
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
    /// A post-fader 0 dB send from `strip` to bus `target`, last.
    fn add_send(&self, strip: u64, target: u64) -> Result<DocSnapshot, VizError>;
    /// Replaces `strip`'s send `index` (counting from 0) with `send`.
    fn edit_send(&self, strip: u64, index: u32, send: SendRow) -> Result<DocSnapshot, VizError>;
    /// Removes `strip`'s send `index` (counting from 0).
    fn remove_send(&self, strip: u64, index: u32) -> Result<DocSnapshot, VizError>;
    fn set_strip_colour(
        &self,
        strip: u64,
        colour: Option<SwatchRow>,
    ) -> Result<DocSnapshot, VizError>;
    /// Releases every touch the control channel holds; how many.
    fn release_channel_touches(&self) -> Result<u32, VizError>;
    /// The app's selected strips, for the control surfaces.
    fn set_surface_selection(&self, strips: Vec<u64>) -> Result<(), VizError>;

    // Slots and MIDI strips (slice 6b).

    /// Removes insert `slot`.
    fn remove_insert(&self, slot: u64) -> Result<DocSnapshot, VizError>;
    /// Moves insert `slot` to position `index` of `strip`'s inserts.
    fn move_insert(&self, slot: u64, strip: u64, index: u32) -> Result<DocSnapshot, VizError>;
    fn set_bypass(&self, slot: u64, bypass: bool) -> Result<DocSnapshot, VizError>;
    /// Feeds `slot`'s sidechain from strip `source` (`None`: nothing).
    fn set_sidechain(&self, slot: u64, source: Option<u64>) -> Result<DocSnapshot, VizError>;
    /// Loads `plugin` into `strip`'s EQ slot, or empties it (`None`).
    fn set_eq(&self, strip: u64, plugin: Option<PluginRefRow>) -> Result<DocSnapshot, VizError>;
    /// Loads slots safe mode held off.
    fn load_plugins(&self, slots: Vec<u64>) -> Result<(), VizError>;
    /// The slots safe mode holds off.
    fn held_slots(&self) -> Result<Vec<u64>, VizError>;
    /// Sets an external destination's MIDI strip mix, one undo step.
    fn set_midi_mix(&self, dest: u64, mix: MidiMixRow) -> Result<DocSnapshot, VizError>;
    /// Sends a drag's volume and pan to `dest` without touching the
    /// document (`None` leaves that controller). False: nothing was sent.
    fn preview_midi_mix(
        &self,
        dest: u64,
        volume: Option<u8>,
        pan: Option<u8>,
    ) -> Result<bool, VizError>;

    // Files and jobs (slice 7a).

    /// The media pool, with offline items and use counts.
    fn media(&self) -> Result<Vec<MediaRow>, VizError>;
    /// Checks every media file again; true when the offline set changed.
    fn recheck_media(&self) -> Result<bool, VizError>;
    /// The project's format, its mismatched media and the device's rate.
    fn project_info(&self) -> Result<ProjectInfoRow, VizError>;
    /// Starts changing the project format (a `Format` job); the job ID.
    fn set_project_format(&self, format: ProjectFormatRow) -> Result<u64, VizError>;
    /// Starts converting media to the project format (a `Convert` job).
    fn convert_media(&self) -> Result<u64, VizError>;
    /// Starts importing `paths` (an `Import` job); `target` places regions.
    fn import_audio(
        &self,
        paths: Vec<String>,
        target: Option<ImportTargetRow>,
    ) -> Result<u64, VizError>;
    /// Job `id`, applied first when it just finished.
    fn job(&self, id: u64) -> Result<JobRow, VizError>;
    /// Asks job `id` to stop (nothing it did is applied).
    fn cancel_job(&self, id: u64) -> Result<(), VizError>;
    /// Starts relinking media item `media` to `path` (a `Relink` job).
    fn relink_media(&self, media: u64, path: String) -> Result<u64, VizError>;
    /// Starts searching `folder` for offline media (a `Relink` job).
    fn search_media(&self, folder: String) -> Result<u64, VizError>;
    /// Starts a bounce (a `Bounce` job).
    fn bounce(&self, req: BounceRequestRow) -> Result<u64, VizError>;
    /// What `bounce` would do with `req` (nothing starts).
    fn bounce_check(&self, req: BounceRequestRow) -> Result<BounceCheckRow, VizError>;
    /// What bounce job `job` wrote, once it is done.
    fn bounce_result(&self, job: u64) -> Result<Option<BounceResultRow>, VizError>;
    /// What a MIDI file holds (slow: call it off the UI thread).
    fn midi_file_info(&self, path: String) -> Result<MidiFileInfoRow, VizError>;
    /// Starts a MIDI file import (an `ImportMidi` job).
    fn import_midi(&self, req: MidiImportRequestRow) -> Result<u64, VizError>;
    /// Starts a Standard MIDI File export (an `ExportMidi` job).
    fn export_midi(&self, req: SmfExportRequestRow) -> Result<u64, VizError>;
    /// The dry run of `export_midi`.
    fn export_midi_check(&self, req: SmfExportRequestRow) -> Result<ReportRow, VizError>;
    /// Starts a MIDI clip file export (an `ExportMidi` job).
    fn export_midi_clip(&self, req: ClipExportRequestRow) -> Result<u64, VizError>;
    /// The dry run of `export_midi_clip`.
    fn export_midi_clip_check(&self, req: ClipExportRequestRow) -> Result<ReportRow, VizError>;
    /// The file names a clip export into a folder writes, and those taken.
    fn export_midi_clip_names(&self, req: ClipExportRequestRow) -> Result<ClipNamesRow, VizError>;
    /// Starts an export for another DAW (an `ExportProject` job).
    fn export_for_daw(&self, req: ForDawRequestRow) -> Result<u64, VizError>;
    /// The dry run of `export_for_daw`.
    fn export_for_daw_check(&self, req: ForDawRequestRow) -> Result<ForDawCheckRow, VizError>;
    /// Starts a DAWproject export (an `ExportDawproject` job).
    fn export_dawproject(&self, req: DawprojectExportRequestRow) -> Result<u64, VizError>;
    /// The dry run of `export_dawproject`.
    fn export_dawproject_check(
        &self,
        req: DawprojectExportRequestRow,
    ) -> Result<ReportRow, VizError>;
    /// Starts a DAWproject import into this (new) project (an
    /// `ImportProject` job).
    fn import_dawproject(&self, req: DawprojectImportRequestRow) -> Result<u64, VizError>;
    /// The file name a DAWproject export of `seq` suggests.
    fn dawproject_name(&self, seq: u64) -> Result<String, VizError>;
    /// The project's DAWproject snapshots, newest first (slow).
    fn dawproject_snapshots(&self) -> Result<Vec<SnapshotRow>, VizError>;
    /// Starts a DAWproject merge (a `MergeDawproject` job).
    fn merge_dawproject(&self, req: DawprojectMergeRequestRow) -> Result<u64, VizError>;
    /// Applies the merge job `job` holds; its report.
    fn confirm_merge(&self, job: u64) -> Result<ReportRow, VizError>;
    /// Drops the merge job `job` holds.
    fn cancel_merge(&self, job: u64) -> Result<(), VizError>;
    /// Starts an AAF export (an `ExportAaf` job).
    fn export_aaf(&self, req: AafExportRequestRow) -> Result<u64, VizError>;
    /// The dry run of `export_aaf`.
    fn export_aaf_check(&self, req: AafExportRequestRow) -> Result<ReportRow, VizError>;
    /// Starts an AAF import (an `ImportAaf` job).
    fn import_aaf(&self, req: AafImportRequestRow) -> Result<u64, VizError>;
    /// The folder name an AAF export of `seq` suggests.
    fn aaf_folder_name(&self, seq: u64) -> Result<String, VizError>;
    /// The report of the last import or export job, if any.
    fn interchange_report(&self) -> Result<Option<ReportRow>, VizError>;
    fn song_info(&self) -> Result<SongInfoRow, VizError>;
    /// Sets the song info, one undo step "Set Song Info".
    fn set_song_info(&self, info: SongInfoRow) -> Result<DocSnapshot, VizError>;
    /// The recordings offered for recovery.
    fn recovered_files(&self) -> Result<Vec<RecoveredFileRow>, VizError>;
    /// Answers the recovery offer (`import`: onto new tracks).
    fn recover_recordings(&self, import: bool) -> Result<DocSnapshot, VizError>;
    /// How many Co-Producer copies the project keeps, and their bytes.
    fn co_producer_copies_size(&self) -> Result<CopiesSizeRow, VizError>;
    /// The project's ID (its record's head), if it has one.
    fn project_id(&self) -> Result<Option<String>, VizError>;
    /// Whether the project keeps the snapshot of export `export_id` (one
    /// lstat).
    fn has_snapshot(&self, export_id: String) -> Result<bool, VizError>;
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
    fn idle(&self) -> Result<IdleNews, VizError> {
        Session::idle(self).map(|u| IdleNews {
            snapshot: u.snapshot,
            slots_changed: u.slots_changed,
            media_changed: u.media_changed,
        })
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
    fn add_send(&self, strip: u64, target: u64) -> Result<DocSnapshot, VizError> {
        Session::add_send(self, strip, target)
    }
    fn edit_send(&self, strip: u64, index: u32, send: SendRow) -> Result<DocSnapshot, VizError> {
        Session::edit_send(self, strip, index, send)
    }
    fn remove_send(&self, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
        Session::remove_send(self, strip, index)
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
    fn remove_insert(&self, slot: u64) -> Result<DocSnapshot, VizError> {
        Session::remove_insert(self, slot)
    }
    fn move_insert(&self, slot: u64, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
        Session::move_insert(self, slot, strip, index)
    }
    fn set_bypass(&self, slot: u64, bypass: bool) -> Result<DocSnapshot, VizError> {
        Session::set_bypass(self, slot, bypass)
    }
    fn set_sidechain(&self, slot: u64, source: Option<u64>) -> Result<DocSnapshot, VizError> {
        Session::set_sidechain(self, slot, source)
    }
    fn set_eq(&self, strip: u64, plugin: Option<PluginRefRow>) -> Result<DocSnapshot, VizError> {
        Session::set_eq(self, strip, plugin)
    }
    fn load_plugins(&self, slots: Vec<u64>) -> Result<(), VizError> {
        Session::load_plugins(self, slots)
    }
    fn held_slots(&self) -> Result<Vec<u64>, VizError> {
        Session::held_slots(self)
    }
    fn set_midi_mix(&self, dest: u64, mix: MidiMixRow) -> Result<DocSnapshot, VizError> {
        Session::set_midi_mix(self, dest, mix)
    }
    fn preview_midi_mix(
        &self,
        dest: u64,
        volume: Option<u8>,
        pan: Option<u8>,
    ) -> Result<bool, VizError> {
        Session::preview_midi_mix(self, dest, volume, pan)
    }
    fn media(&self) -> Result<Vec<MediaRow>, VizError> {
        Session::media(self)
    }
    fn recheck_media(&self) -> Result<bool, VizError> {
        Session::recheck_media(self)
    }
    fn project_info(&self) -> Result<ProjectInfoRow, VizError> {
        Session::project_info(self)
    }
    fn set_project_format(&self, format: ProjectFormatRow) -> Result<u64, VizError> {
        Session::set_project_format(self, format)
    }
    fn convert_media(&self) -> Result<u64, VizError> {
        Session::convert_media(self)
    }
    fn import_audio(
        &self,
        paths: Vec<String>,
        target: Option<ImportTargetRow>,
    ) -> Result<u64, VizError> {
        Session::import_audio(self, paths, target)
    }
    fn job(&self, id: u64) -> Result<JobRow, VizError> {
        Session::job(self, id)
    }
    fn cancel_job(&self, id: u64) -> Result<(), VizError> {
        Session::cancel_job(self, id)
    }
    fn relink_media(&self, media: u64, path: String) -> Result<u64, VizError> {
        Session::relink_media(self, media, path)
    }
    fn search_media(&self, folder: String) -> Result<u64, VizError> {
        Session::search_media(self, folder)
    }
    fn bounce(&self, req: BounceRequestRow) -> Result<u64, VizError> {
        Session::bounce(self, req)
    }
    fn bounce_check(&self, req: BounceRequestRow) -> Result<BounceCheckRow, VizError> {
        Session::bounce_check(self, req)
    }
    fn bounce_result(&self, job: u64) -> Result<Option<BounceResultRow>, VizError> {
        Session::bounce_result(self, job)
    }
    fn midi_file_info(&self, path: String) -> Result<MidiFileInfoRow, VizError> {
        Session::midi_file_info(self, path)
    }
    fn import_midi(&self, req: MidiImportRequestRow) -> Result<u64, VizError> {
        Session::import_midi(self, req)
    }
    fn export_midi(&self, req: SmfExportRequestRow) -> Result<u64, VizError> {
        Session::export_midi(self, req)
    }
    fn export_midi_check(&self, req: SmfExportRequestRow) -> Result<ReportRow, VizError> {
        Session::export_midi_check(self, req)
    }
    fn export_midi_clip(&self, req: ClipExportRequestRow) -> Result<u64, VizError> {
        Session::export_midi_clip(self, req)
    }
    fn export_midi_clip_check(&self, req: ClipExportRequestRow) -> Result<ReportRow, VizError> {
        Session::export_midi_clip_check(self, req)
    }
    fn export_midi_clip_names(&self, req: ClipExportRequestRow) -> Result<ClipNamesRow, VizError> {
        Session::export_midi_clip_names(self, req)
    }
    fn export_for_daw(&self, req: ForDawRequestRow) -> Result<u64, VizError> {
        Session::export_for_daw(self, req)
    }
    fn export_for_daw_check(&self, req: ForDawRequestRow) -> Result<ForDawCheckRow, VizError> {
        Session::export_for_daw_check(self, req)
    }
    fn export_dawproject(&self, req: DawprojectExportRequestRow) -> Result<u64, VizError> {
        Session::export_dawproject(self, req)
    }
    fn export_dawproject_check(
        &self,
        req: DawprojectExportRequestRow,
    ) -> Result<ReportRow, VizError> {
        Session::export_dawproject_check(self, req)
    }
    fn import_dawproject(&self, req: DawprojectImportRequestRow) -> Result<u64, VizError> {
        Session::import_dawproject(self, req)
    }
    fn dawproject_name(&self, seq: u64) -> Result<String, VizError> {
        Session::dawproject_name(self, seq)
    }
    fn dawproject_snapshots(&self) -> Result<Vec<SnapshotRow>, VizError> {
        Session::dawproject_snapshots(self)
    }
    fn merge_dawproject(&self, req: DawprojectMergeRequestRow) -> Result<u64, VizError> {
        Session::merge_dawproject(self, req)
    }
    fn confirm_merge(&self, job: u64) -> Result<ReportRow, VizError> {
        Session::confirm_merge(self, job)
    }
    fn cancel_merge(&self, job: u64) -> Result<(), VizError> {
        Session::cancel_merge(self, job)
    }
    fn export_aaf(&self, req: AafExportRequestRow) -> Result<u64, VizError> {
        Session::export_aaf(self, req)
    }
    fn export_aaf_check(&self, req: AafExportRequestRow) -> Result<ReportRow, VizError> {
        Session::export_aaf_check(self, req)
    }
    fn import_aaf(&self, req: AafImportRequestRow) -> Result<u64, VizError> {
        Session::import_aaf(self, req)
    }
    fn aaf_folder_name(&self, seq: u64) -> Result<String, VizError> {
        Session::aaf_folder_name(self, seq)
    }
    fn interchange_report(&self) -> Result<Option<ReportRow>, VizError> {
        Session::interchange_report(self)
    }
    fn song_info(&self) -> Result<SongInfoRow, VizError> {
        Session::song_info(self)
    }
    fn set_song_info(&self, info: SongInfoRow) -> Result<DocSnapshot, VizError> {
        Session::set_song_info(self, info)
    }
    fn recovered_files(&self) -> Result<Vec<RecoveredFileRow>, VizError> {
        Session::recovered_files(self)
    }
    fn recover_recordings(&self, import: bool) -> Result<DocSnapshot, VizError> {
        Session::recover_recordings(self, import)
    }
    fn co_producer_copies_size(&self) -> Result<CopiesSizeRow, VizError> {
        Session::co_producer_copies_size(self)
    }
    fn project_id(&self) -> Result<Option<String>, VizError> {
        Session::project_id(self)
    }
    fn has_snapshot(&self, export_id: String) -> Result<bool, VizError> {
        Session::has_snapshot(self, export_id)
    }
}

/// The core's constructors and free functions the app uses (open, new,
/// file reads before an open). The store holds one: `LiveCore` over the
/// real core, `FakeCore` (testing) in tests.
pub trait CoreCalls: Send + Sync {
    /// An empty untitled document (the default format).
    fn new_empty(&self) -> Result<Arc<dyn DocSession>, VizError>;
    /// The demo song.
    fn new_demo(&self) -> Result<Arc<dyn DocSession>, VizError>;
    /// The demo song with audio.
    fn new_demo_audio(&self) -> Result<Arc<dyn DocSession>, VizError>;
    /// An empty untitled document in `format`.
    fn new_project(&self, format: ProjectFormatRow) -> Result<Arc<dyn DocSession>, VizError>;
    /// Opens the `.vizdaw` package at `path`.
    fn open(&self, path: &str) -> Result<Arc<dyn DocSession>, VizError>;
    /// Opens `path` with recovery options (plugins off, an autosave).
    fn open_with(&self, path: &str, options: OpenOptions) -> Result<Arc<dyn DocSession>, VizError>;
    /// Opens a Co-Producer copy (refused when `path` is not one).
    fn open_copy(&self, path: &str) -> Result<Arc<dyn DocSession>, VizError>;
    /// What a DAWproject file holds before an import (slow).
    fn dawproject_info(&self, path: &str) -> Result<DawprojectInfoRow, VizError>;
    /// What an AAF file holds before an import (slow).
    fn aaf_info(&self, path: &str) -> Result<AafInfoRow, VizError>;
    /// What to ask before opening `path` (a crash, a newer autosave).
    fn recovery_check(&self, path: &str) -> Result<RecoveryInfo, VizError>;
    /// The allowed project rates.
    fn project_rates(&self) -> Vec<u32>;
    /// Display text, e.g. "44.1 kHz, 16-bit".
    fn format_label(&self, format: ProjectFormatRow) -> String;
    /// The bounce settings (the tail cap).
    fn bounce_settings(&self) -> BounceSettingsRow;
    /// Turns `plugins` off in the app's plug-in settings (the Plugin
    /// Manager's catalog, saved), the crash question's "Disable <names>".
    /// Without a catalog: refused.
    fn disable_plugins(&self, plugins: &[PluginRefRow]) -> Result<(), VizError> {
        let _ = plugins;
        Err(VizError::Model {
            message: "there are no plug-in settings to change".into(),
        })
    }
}

/// `CoreCalls` over the real core.
pub struct LiveCore;

fn doc(s: Result<Arc<Session>, VizError>) -> Result<Arc<dyn DocSession>, VizError> {
    Ok(s? as Arc<dyn DocSession>)
}

impl CoreCalls for LiveCore {
    fn new_empty(&self) -> Result<Arc<dyn DocSession>, VizError> {
        doc(Session::new_empty())
    }
    fn new_demo(&self) -> Result<Arc<dyn DocSession>, VizError> {
        doc(Session::new_demo())
    }
    fn new_demo_audio(&self) -> Result<Arc<dyn DocSession>, VizError> {
        doc(Session::new_demo_audio())
    }
    fn new_project(&self, format: ProjectFormatRow) -> Result<Arc<dyn DocSession>, VizError> {
        doc(Session::new_project(format))
    }
    fn open(&self, path: &str) -> Result<Arc<dyn DocSession>, VizError> {
        doc(Session::open(path.to_owned()))
    }
    fn open_with(&self, path: &str, options: OpenOptions) -> Result<Arc<dyn DocSession>, VizError> {
        doc(Session::open_with(path.to_owned(), options))
    }
    fn open_copy(&self, path: &str) -> Result<Arc<dyn DocSession>, VizError> {
        doc(Session::open_copy(path.to_owned()))
    }
    fn dawproject_info(&self, path: &str) -> Result<DawprojectInfoRow, VizError> {
        vizdaw_ffi::dawproject_info(path.to_owned())
    }
    fn aaf_info(&self, path: &str) -> Result<AafInfoRow, VizError> {
        vizdaw_ffi::aaf_info(path.to_owned())
    }
    fn recovery_check(&self, path: &str) -> Result<RecoveryInfo, VizError> {
        vizdaw_ffi::recovery::recovery_check(path.to_owned())
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
        let catalog = live_catalog()?;
        for p in plugins {
            catalog.set_enabled(
                vizdaw_ffi::PluginKey {
                    format: p.format,
                    id: p.id.clone(),
                },
                false,
            )?;
        }
        Ok(())
    }
}

/// The app's plug-in catalog (`plugins.json` in the support folder),
/// opened when first needed. Swift opened it at launch; the GPUI app has
/// no Plugin Manager yet, so only "Disable <names>" opens it.
fn live_catalog() -> Result<Arc<vizdaw_ffi::PluginCatalog>, VizError> {
    static CATALOG: std::sync::Mutex<Option<Arc<vizdaw_ffi::PluginCatalog>>> =
        std::sync::Mutex::new(None);
    let mut c = CATALOG.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = c.as_ref() {
        return Ok(c.clone());
    }
    let support = vizdaw_ffi::recovery::support_dir();
    let scanner = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("vizdaw-scan")))
        .unwrap_or_default();
    let opened = vizdaw_ffi::PluginCatalog::open(vizdaw_ffi::CatalogConfig {
        path: support.join("plugins.json").to_string_lossy().into_owned(),
        scanner: scanner.to_string_lossy().into_owned(),
        clap_dirs: vec![],
        include_au: cfg!(target_os = "macos"),
        timeout_ms: None,
        include_vst3: true,
        vst3_dirs: vec![],
    })?;
    *c = Some(opened.clone());
    Ok(opened)
}
