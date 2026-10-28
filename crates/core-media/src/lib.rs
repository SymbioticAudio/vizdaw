//! `core-media`: VizDAW's audio files (sub-project 5 spec §5). The CAF
//! reader and writer every media file goes through, the import pipeline
//! over a platform `MediaImporter`, the disk streamer the real-time
//! thread reads regions from, and the recording ring and writer (5c).
//! Platform-neutral: no OS audio APIs here.
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

pub mod caf;
mod error;
pub mod import;
pub mod overview;
pub mod record;
pub mod stream;

pub use error::MediaError;
