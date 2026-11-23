//! Application logic for the VizDAW UI. Never depends on GPUI.
#![recursion_limit = "256"]

pub mod alerts;
pub mod arrangement;
pub mod audition;
pub mod commands;
pub mod document;
pub mod editor;
pub mod list;
pub mod piano_roll;
pub mod position;
pub mod prefs;
pub mod session;
pub mod store;
#[cfg(feature = "test-support")]
pub mod testing;
pub mod transport;
pub mod transport_bar;
pub mod transport_fields;

pub use alerts::AlertItem;
pub use document::{DocumentModel, SaveAction};
pub use prefs::Prefs;
pub use session::DocSession;
pub use store::{DocId, DocumentStore, WindowToken};
