//! `core-engine`: compiles a core-model sequence into an immutable schedule
//! and plays it sample-accurately through platform backends, with a
//! real-time path that never blocks or allocates.
//!
//! Everything reachable from `render::Renderer::process` is marked
//! `// RT`; such code must not allocate, free, lock, do I/O or log.
//! Graph workers (`pool`) are `// RT` while they run a job; `unsafe` lives
//! only in `graph/exec.rs`, `pool.rs` and `rt_check.rs`.
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

pub mod backend;
pub mod engine;
pub mod error;
#[cfg(feature = "fixtures")]
pub mod fixtures;
pub mod graph;
pub mod midi1;
pub mod notes;
pub mod offline;
pub mod pool;
pub mod record;
pub mod render;
pub mod rt_check;
pub mod schedule;
pub mod transport;

pub use error::{BackendError, EngineError};
