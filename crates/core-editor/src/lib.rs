//! `core-editor`: editor logic that is neither document state nor
//! drawing — what a point is over, cells, strokes, where a dragged row
//! lands — as pure functions of plain data, so a front end only draws
//! (editors spec §3.5: hit-testing and geometry are pure functions; Plan
//! 4e Ruling 1); the transport keys' logic (plan 4f).
//! Platform-neutral; it depends only on `core-model`.
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

pub mod drum;
pub mod transport;
