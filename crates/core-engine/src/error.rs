//! Engine and backend errors. Real-time faults are bits, never errors (see
//! `render::Faults`).

use core_model::ModelError;

/// Failures of an audio or MIDI backend.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    /// No output device (or no device with the requested name).
    #[error("no audio output device")]
    NoDevice,
    /// The device's stream format is not f32 linear PCM.
    #[error("unsupported stream format: {detail}")]
    Format {
        /// What was found.
        detail: String,
    },
    /// An OS call failed.
    #[error("{call} failed with status {status}")]
    Os {
        /// The failing call.
        call: &'static str,
        /// Its `OSStatus` (or errno).
        status: i32,
    },
    /// The built-in synth's audio component is not installed.
    #[error("audio component missing")]
    ComponentMissing,
    /// This factory does not make sinks of this kind (plugin instruments
    /// come from the plugin factory).
    #[error("{0} is not made by this sink factory")]
    NotSupported(String),
}

/// Failures of the non-real-time engine API.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EngineError {
    /// The model refused something (e.g. an unknown sequence ID).
    #[error(transparent)]
    Model(#[from] ModelError),
    /// No sequence with this name.
    #[error("no sequence named {0:?}")]
    UnknownSequence(String),
    /// A backend call failed.
    #[error(transparent)]
    Backend(#[from] BackendError),
    /// The command ring to the audio thread is full; retry later.
    #[error("engine command queue is full")]
    Busy,
    /// The document needs more than `MAX_TARGETS` (64) playback targets.
    #[error("more than 64 playback targets")]
    TooManyTargets,
    /// A helper thread could not be started.
    #[error("cannot start thread {name:?}: {reason}")]
    Thread {
        /// The thread's name.
        name: &'static str,
        /// The OS error.
        reason: String,
    },
}
