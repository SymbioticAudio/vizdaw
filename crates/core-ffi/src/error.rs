//! `VizError`, the one error type of the FFI (Swift: `throws VizError`).

use core_engine::{BackendError, EngineError};
use core_model::ModelError;

/// Every failure the app can see.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum VizError {
    /// No project package (or no `project.json` in it) at `path`.
    #[error("no VizDAW project at {path}")]
    NotFound {
        /// The path that was tried.
        path: String,
    },
    /// The model refused a command or a file.
    #[error("{message}")]
    Model {
        /// Why.
        message: String,
    },
    /// A file-system failure.
    #[error("{message}")]
    Io {
        /// Why.
        message: String,
    },
    /// The engine or an audio/MIDI backend failed.
    #[error("{message}")]
    Engine {
        /// Why.
        message: String,
    },
    /// There is no audio output device.
    #[error("{message}")]
    NoDevice {
        /// Why.
        message: String,
    },
    /// A plugin call is already running on this thread (a nested run
    /// loop re-entered the FFI); try again later.
    #[error("{message}")]
    Busy {
        /// Why.
        message: String,
    },
    /// A bug: a caught panic or an impossible state.
    #[error("internal error: {message}")]
    Internal {
        /// The panic message.
        message: String,
    },
}

impl From<ModelError> for VizError {
    fn from(e: ModelError) -> Self {
        match e {
            ModelError::Io(message) => VizError::Io { message },
            other => VizError::Model {
                message: other.to_string(),
            },
        }
    }
}

impl From<BackendError> for VizError {
    fn from(e: BackendError) -> Self {
        match e {
            BackendError::NoDevice => VizError::NoDevice {
                message: e.to_string(),
            },
            other => VizError::Engine {
                message: other.to_string(),
            },
        }
    }
}

impl From<EngineError> for VizError {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::Backend(b) => b.into(),
            other => VizError::Engine {
                message: other.to_string(),
            },
        }
    }
}
