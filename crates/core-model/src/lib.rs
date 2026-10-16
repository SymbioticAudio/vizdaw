//! `core-model`: the VizDAW project model.
//!
//! A pure-Rust crate holding a VizDAW project and every edit that can be made
//! to it. It has no audio, MIDI I/O, UI or OS dependencies. All edits are
//! [`command::Command`] values applied by the pure function
//! [`command::apply`]; documents are immutable values that share structure.
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

pub mod document;
pub mod error;
pub mod event;
pub mod ids;
pub mod lane;
pub mod meter;
pub mod sequence;
pub mod tempo;
pub mod time;

pub use error::{ModelError, Result};
