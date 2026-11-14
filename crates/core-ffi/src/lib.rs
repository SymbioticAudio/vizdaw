//! `core-ffi` (library `vizdaw_ffi`): the UniFFI surface the SwiftUI app
//! uses. Rust owns all state; Swift gets snapshot records and calls
//! commands. No panic crosses the FFI: every export runs in [`ffi_guard`].
// `im` maps nest deeply; the `Send + Sync` check of `Session` needs more depth.
#![recursion_limit = "256"]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

uniffi::setup_scaffolding!();

pub mod aaf;
pub mod audio;
pub mod automation;
pub mod autowrite;
pub mod blocks;
pub mod bounce;
pub mod bundled;
pub mod catalog;
pub mod clock;
pub mod control;
pub mod dawproject;
pub mod drum;
pub mod edit;
pub mod editor;
pub mod error;
pub mod events;
pub mod external;
pub mod for_daw;
pub mod format;
pub mod guard;
pub mod host;
pub mod input;
pub mod input_fx;
pub mod interchange;
pub mod jobs;
pub mod keys;
pub mod live;
pub mod media;
pub mod mixer;
pub mod modify;
pub mod mpe;
pub mod overviews;
pub mod passes;
pub mod plugins;
pub mod project_id;
pub mod record;
pub mod recovery;
pub mod relink;
pub mod retention;
pub mod revs;
pub mod selection;
pub mod session;
pub mod snapshot;
pub mod step;
pub mod strip;
pub mod surfaces;
pub mod synth;
pub mod templates;
pub mod tracks;

pub use aaf::{
    AafDepthRow, AafExportRequestRow, AafFpsRow, AafImportRequestRow, AafInfoRow, AafStereoRow,
};
pub use audio::AudioSettingsRow;
pub use automation::{
    AutomationModeRow, AutomationSettingsRow, AutomationStateRow, GesturePhaseRow, PreviewPointRow,
    StripModeRow, ThinningRow,
};
pub use blocks::{BlockKind, BlockRow, BlockRule};
pub use bounce::{
    BounceCheckRow, BounceRangeRow, BounceRequestRow, BounceResultRow, BounceSettingsRow,
    BounceSourceRow, BouncedFileRow, ContainerRow, bounce_settings, set_bounce_settings,
};
pub use bundled::{new_channel_eq, set_new_channel_eq};
pub use catalog::{
    CatalogConfig, CatalogRowFfi, PluginCatalog, PluginKey, PluginKindTag, PluginStatusRow,
    ScanProgress,
};
pub use control::{
    AlertRow, AudioEditorBoxRow, AudioEditorRegionRow, AudioEditorStateRow, ControlArg,
    ControlCall, ControlErrorCode, ControlFailure, ControlRequest, ControlResult, ControlServer,
    ControlValue, CrashAnswerRow, DetailRow, DocFactsRow, EditorStateRow, ElementRefRow,
    KeyStrokeRow, LayoutStateRow, ModifiersRow, MouseButtonRow, PaneRow, PointerRow,
    RecoveryAnswersRow, StateTargetRow, StepEntryStateRow, TornRow, TransportMarksRow, UiNodeRow,
    ViewPanesRow, WaitConditionRow, WaitFactsRow, WindowKindRow, WindowRefRow, WindowRow,
    control_command_ids, control_element_check, control_host_busy, control_wait_met,
};
pub use dawproject::{
    DawprojectExportRequestRow, DawprojectImportRequestRow, DawprojectInfoRow,
    DawprojectMergeRequestRow, MergePreviewRow, MergeStepRow, RegionGainRow, SnapshotRow,
};
pub use drum::{
    DrumBandKindRow, DrumBandRow, DrumCellRow, DrumEdit, DrumGridLayoutRow, DrumGridSettingsRow,
    DrumHitRow, DrumLaneRow, DrumNewRow, DrumPaintHitRow, DrumRowRow, DrumSpotRow, DrumStrokeRow,
    DrumViewRow, QuantizeChoiceRow,
};
pub use edit::{
    AudioEdit, CrossfadeLengthRow, EditCommand, EventEdit, GeneratorRow, LaneEdit, LaneInputRow,
    LaneRef, MonitorModeRow, QuantizeRow, ReferenceEdit, SequenceEdit, ViewFilterRow,
};
pub use editor::EditorSize;
pub use error::VizError;
pub use events::{
    AutomationRow, AutomationTargetRow, EventKindRow, EventRow, FadeRow, FadeShapeRow,
    GroupChannel, LaneEvents, MarkerRow, MeterEntryRow, NoteAttributeRow, PerNoteKindRow,
    PerNotePoint, PerNoteValueRow, SequenceInfo, TempoPointRow, TextKindRow,
};
pub use external::{ExternalEditRow, ExternalEditStateRow};
pub use for_daw::{ForDawCheckRow, ForDawRequestRow};
pub use format::{BitDepthRow, ProjectFormatRow, ProjectInfoRow, format_label, project_rates};
pub use guard::ffi_guard;
pub use input::{NoteInRow, NoteInput};
pub use input_fx::{ArpModeRow, FeelRow, InputFxKindRow, InputFxRow, input_fx_from_json};
pub use interchange::{
    ChannelMapRow, ChannelUseRow, ClipExportRequestRow, ClipNamesRow, MidiFileInfoRow,
    MidiFileKindRow, MidiImportRequestRow, ReportItemRow, ReportKindRow, ReportRow,
    SmfExportRequestRow, SongInfoRow,
};
pub use jobs::{ImportTargetRow, JobKindRow, JobRow, JobStateRow};
pub use keys::{BindingRow, Keymap, chord_text, key_code_for};
pub use live::InputInfoRow;
pub use media::{MediaOriginRow, MediaRow, MediaUseRow, OfflineReasonRow};
pub use mixer::{
    BusRow, DestRow, MeterRow, MidiLevelRow, MidiMixRow, MixerLevels, MixerSnapshot, SendRow,
    StripColourRow, StripKindRow, StripRow, SwatchRow,
};
pub use modify::{
    AmountRow, ChangeOpRow, ConditionRow, ConditionTest, CurveRow, ModifyRow, ModifyTemplateRow,
    NoteMapEntry, RuleRow, SelectModeRow, SettingsEdit, SubstituteRow, ValueTargetRow, WaveRow,
};
pub use mpe::{MpeRow, MpeZoneRow};
pub use overviews::{OverviewRows, OverviewStateRow};
pub use record::{RecordLaneRow, RecordSpecRow, RecordStateRow, set_record_latency_trim};
pub use recovery::RecoveryInfo;
pub use revs::LaneRev;
pub use selection::{SelectionRow, TickRange};
pub use session::{OpenOptions, Session};
pub use snapshot::{
    DocSnapshot, IdleUpdate, KeySlotRow, LaneRow, ParamRow, ParamValue, PluginFormatTag,
    PluginRefRow, SeqRow, SlotRow, SlotStatus, TransportState,
};
pub use step::{
    NoteValueRow, StepInputRow, StepKindRow, StepNoteRow, StepRecordRow, StepResultRow,
    StepStateRow, step_length, step_parse_pitches, step_state_new,
};
pub use strip::{
    LineOpRow, LinePointRow, LineRow, LineShapeRow, LineTargetRow, PeriodRow, SeriesRow,
};
pub use synth::MeasureRow;
pub use templates::SequenceTemplate;
pub use tracks::TrackKindRow;

/// The app is quitting: stops and drops the engine host so no plugin
/// host is still rendering when thread-local destructors run at exit.
/// Call on the main thread after every session was closed; a later Play
/// would simply create a new host.
#[uniffi::export]
pub fn shutdown() -> Result<(), VizError> {
    ffi_guard(|| {
        // Surfaces that say goodbye hear it before the app goes (plan 5f-4 Ruling 4).
        surfaces::goodbye_all();
        host::reset();
        Ok(())
    })
}

/// What a DAWproject file at `path` holds for the app before an import
/// (plan 6d L8): the project rate to make, its tracks and audio files and
/// whether it came from VizDAW. A free function, so it works with no
/// document open; call it off the main thread (it opens and parses the
/// file). Nothing outside the package is looked at. `Model` for a path or
/// file refused (with the reader's sentence), `Io` when it cannot be read.
#[uniffi::export]
pub fn dawproject_info(path: String) -> Result<DawprojectInfoRow, VizError> {
    ffi_guard(|| dawproject::info(&path))
}

/// What an AAF file at `path` holds for the app before an import (plan
/// 6f Task 5): the project rate to make, its tracks, audio files and
/// offline ones, whether it came from VizDAW, and the MIDI file beside it
/// the import would read. A free function, so it works with no document
/// open; call it off the main thread (it opens and reads the file).
/// `Model` for a path or file refused (with the reader's sentence), `Io`
/// when it cannot be read.
#[uniffi::export]
pub fn aaf_info(path: String) -> Result<AafInfoRow, VizError> {
    ffi_guard(|| aaf::info(&path))
}

/// Panics inside the guard (test hook): returns `VizError::Internal`.
#[cfg(feature = "test-hooks")]
#[uniffi::export]
pub fn debug_panic() -> Result<(), VizError> {
    ffi_guard(|| -> Result<(), VizError> {
        #[allow(clippy::panic)]
        {
            panic!("debug_panic called")
        }
    })
}
