//! `platform-macos`: CoreAudio HAL output, CoreMIDI output and Apple's DLS
//! synth as the built-in synth, implementing `core-engine`'s backend traits.
//!
//! All `unsafe` lives in `hal.rs`, `dls.rs`, `midi.rs` and `rt.rs`, with a
//! `// SAFETY:` comment per block.
#![cfg(target_os = "macos")]
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

pub mod dls;
pub mod hal;
pub mod midi;
pub mod rt;
pub mod sinks;
