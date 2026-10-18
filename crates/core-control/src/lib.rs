//! `core-control`: the agent control channel (design spec
//! `docs/superpowers/specs/2026-10-18-control-channel-design.md`).
//! JSON-lines requests and replies, their limits and errors, element IDs
//! and key strokes (this task); command validation, the menu-command
//! registry and wait conditions (Task 3); the Unix-socket server and
//! client (Task 4). Platform-neutral Unix code: the app executes what
//! needs its windows.
#![forbid(unsafe_code)]
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

pub mod element;
pub mod error;
pub mod keys;
pub mod params;
pub mod protocol;

pub use error::{ControlError, ErrorCode};
pub use protocol::{RawRequest, RequestId};
