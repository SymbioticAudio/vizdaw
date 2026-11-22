//! Application logic for the VizDAW UI. Never depends on GPUI.
#![recursion_limit = "256"]

pub mod alerts;
pub mod commands;
pub mod document;
pub mod session;
pub mod store;
#[cfg(feature = "test-support")]
pub mod testing;
pub mod transport;

pub use alerts::AlertItem;
pub use document::{DocumentModel, SaveAction};
pub use session::DocSession;
pub use store::{DocId, DocumentStore, WindowToken};
