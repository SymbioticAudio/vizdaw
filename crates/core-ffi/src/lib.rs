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
pub mod edit;
pub mod editor;
pub mod error;
pub mod events;
pub mod guard;
pub mod host;
pub mod mixer;
pub mod plugins;
pub mod recovery;
pub mod revs;
pub mod selection;
pub mod session;
pub mod snapshot;
pub mod templates;

pub use blocks::{BlockKind, BlockRow, BlockRule};
pub use catalog::{
    CatalogConfig, CatalogRowFfi, PluginCatalog, PluginKey, PluginKindTag, PluginStatusRow,
    ScanProgress,
};
pub use edit::{
    AudioEdit, EditCommand, EventEdit, GeneratorRow, LaneEdit, LaneRef, QuantizeRow, ReferenceEdit,
    SequenceEdit, ViewFilterRow,
};
pub use editor::EditorSize;
pub use error::VizError;
pub use events::{
    EventKindRow, EventRow, FadeRow, FadeShapeRow, GroupChannel, LaneEvents, MarkerRow,
    MeterEntryRow, NoteAttributeRow, PerNoteKindRow, PerNotePoint, SequenceInfo, TempoPointRow,
    TextKindRow,
};
pub use guard::ffi_guard;
pub use mixer::{
    BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
};
pub use recovery::RecoveryInfo;
pub use revs::LaneRev;
pub use selection::{SelectionRow, TickRange};
pub use session::{OpenOptions, Session};
pub use snapshot::{
    DocSnapshot, IdleUpdate, KeySlotRow, LaneRow, ParamRow, ParamValue, PluginFormatTag,
    PluginRefRow, SeqRow, SlotRow, SlotStatus, TransportState,
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
