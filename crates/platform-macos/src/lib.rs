//! `platform-macos`: CoreAudio HAL output, CoreMIDI output and Apple's DLS
//! synth as the built-in synth, implementing `core-engine`'s backend traits,
//! and the Audio Unit plugin host (`au`).
//!
//! All `unsafe` lives in `hal.rs`, `dls.rs`, `midi.rs`, `rt.rs` and
//! `au/ffi.rs`, with a `// SAFETY:` comment per block.
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

pub mod au;
pub mod dls;
pub mod hal;
pub mod midi;
pub mod rt;
pub mod sinks;
