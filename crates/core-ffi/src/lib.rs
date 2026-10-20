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

pub mod blocks;
pub mod catalog;
pub mod clock;
pub mod control;
pub mod drum;
pub mod edit;
pub mod editor;
pub mod error;
pub mod events;
pub mod format;
pub mod guard;
pub mod host;
pub mod input;
pub mod jobs;
pub mod keys;
pub mod media;
pub mod mixer;
pub mod modify;
pub mod overviews;
pub mod passes;
pub mod plugins;
pub mod recovery;
pub mod relink;
pub mod retention;
pub mod revs;
pub mod selection;
pub mod session;
pub mod snapshot;
pub mod step;
pub mod strip;
pub mod templates;

pub use blocks::{BlockKind, BlockRow, BlockRule};
pub use catalog::{
    CatalogConfig, CatalogRowFfi, PluginCatalog, PluginKey, PluginKindTag, PluginStatusRow,
    ScanProgress,
};
pub use control::{
    AlertRow, ControlArg, ControlCall, ControlErrorCode, ControlFailure, ControlRequest,
    ControlResult, ControlServer, ControlValue, CrashAnswerRow, DetailRow, DocFactsRow,
    EditorStateRow, ElementRefRow, KeyStrokeRow, ModifiersRow, MouseButtonRow, PointerRow,
    RecoveryAnswersRow, StateTargetRow, StepEntryStateRow, TornRow, TransportMarksRow, UiNodeRow,
    WaitConditionRow, WaitFactsRow, WindowKindRow, WindowRefRow, WindowRow, control_command_ids,
    control_element_check, control_host_busy, control_wait_met,
};
pub use drum::{
    DrumBandKindRow, DrumBandRow, DrumCellRow, DrumEdit, DrumGridLayoutRow, DrumGridSettingsRow,
    DrumHitRow, DrumLaneRow, DrumNewRow, DrumPaintHitRow, DrumRowRow, DrumSpotRow, DrumStrokeRow,
    DrumViewRow, QuantizeChoiceRow,
};
pub use edit::{
    AudioEdit, EditCommand, EventEdit, GeneratorRow, LaneEdit, LaneRef, QuantizeRow, ReferenceEdit,
    SequenceEdit, ViewFilterRow,
};
pub use editor::EditorSize;
pub use error::VizError;
pub use events::{
    AutomationRow, AutomationTargetRow, EventKindRow, EventRow, FadeRow, FadeShapeRow,
    GroupChannel, LaneEvents, MarkerRow, MeterEntryRow, NoteAttributeRow, PerNoteKindRow,
    PerNotePoint, PerNoteValueRow, SequenceInfo, TempoPointRow, TextKindRow,
};
pub use format::{BitDepthRow, ProjectFormatRow, ProjectInfoRow, format_label, project_rates};
pub use guard::ffi_guard;
pub use input::{NoteInRow, NoteInput};
pub use jobs::{ImportTargetRow, JobKindRow, JobRow, JobStateRow};
pub use keys::{BindingRow, Keymap, chord_text, key_code_for};
pub use media::{MediaOriginRow, MediaRow, MediaUseRow, OfflineReasonRow};
pub use mixer::{
    BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
};
pub use modify::{
    AmountRow, ChangeOpRow, ConditionRow, ConditionTest, CurveRow, ModifyRow, ModifyTemplateRow,
    NoteMapEntry, RuleRow, SelectModeRow, SettingsEdit, SubstituteRow, ValueTargetRow, WaveRow,
};
pub use overviews::{OverviewRows, OverviewStateRow};
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
pub use templates::SequenceTemplate;

/// The app is quitting: stops and drops the engine host so no plugin
/// host is still rendering when thread-local destructors run at exit.
/// Call on the main thread after every session was closed; a later Play
/// would simply create a new host.
#[uniffi::export]
pub fn shutdown() -> Result<(), VizError> {
    ffi_guard(|| {
        host::reset();
        Ok(())
    })
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
