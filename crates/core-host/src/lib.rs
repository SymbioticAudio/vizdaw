//! `core-host`: the plugin abstraction (spec §5) and its platform-neutral
//! hosts. A plugin is a main-thread [`PluginInstance`] plus the real-time
//! [`AudioProcessor`] it hands out when activated (Plan Ruling 1);
//! [`adapt`] wraps processors as the engine's inserts and instruments.
//!
//! `unsafe` lives only in `clap/ffi.rs`, with a `// SAFETY:` comment per
//! block. Everything marked `// RT` must not allocate, lock or block.
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

pub mod error;
pub mod info;
pub mod instance;
pub mod process;
pub mod queue;

pub use error::HostError;
pub use info::{AudioPortInfo, ParamInfo, PluginInfo, PluginKind, PortRole, Tail};
pub use instance::{
    AudioProcessor, InstanceEvents, LoadRequest, ParentView, PluginEditor, PluginFormat,
    PluginInstance, PrepareJob, Prepared,
};
pub use process::{
    MAX_EVENTS, MAX_PARAM_EVENTS, MidiEvent, ParamChange, ParamEvent, ParamOut, ProcessCtx,
};
