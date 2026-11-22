//! Application logic for the VizDAW UI. Never depends on GPUI.
#![recursion_limit = "256"]

pub mod alerts;
pub mod document;
pub mod session;
pub mod store;

pub use alerts::AlertItem;
pub use document::{DocumentModel, SaveAction};
pub use session::DocSession;
pub use store::{DocId, DocumentStore, WindowToken};
