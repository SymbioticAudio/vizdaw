//! The single error type of the crate.

use crate::ids::{AnyId, SeqId};

/// Every failure the model can report.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    /// A reference would make a sequence reach itself. `path` starts and ends
    /// with the same sequence.
    #[error("reference cycle through sequences {path:?}")]
    Cycle {
        /// The cycle, first element repeated at the end.
        path: Vec<SeqId>,
    },
    /// An ID that does not exist in the document.
    #[error("unknown id {0}")]
    UnknownId(AnyId),
    /// The item is still used by the listed items.
    #[error("still in use by {users:?}")]
    InUse {
        /// The users (events, lanes or drum rows) that block the change.
        users: Vec<AnyId>,
    },
    /// A value outside its allowed range.
    #[error("{field} out of range: {value}")]
    OutOfRange {
        /// The field name.
        field: &'static str,
        /// The offending value, formatted.
        value: String,
    },
    /// A meter change that does not fall on a barline.
    #[error("meter change at tick {tick} is not on a barline")]
    NotOnBarline {
        /// The tick of the offending meter change.
        tick: i64,
    },
    /// The project file was written by a newer VizDAW.
    #[error("project format version {found} is newer than this build supports")]
    NewerFormat {
        /// The version found in the file.
        found: u64,
    },
    /// The project file failed validation; every problem is listed.
    #[error("corrupt project: {problems:?}")]
    Corrupt {
        /// Human-readable problem descriptions.
        problems: Vec<String>,
    },
    /// A structurally invalid request that is not an ID or range problem
    /// (for example committing with no open transaction).
    #[error("invalid: {reason}")]
    Invalid {
        /// Why the request was refused.
        reason: String,
    },
    /// A filesystem error, formatted.
    #[error("i/o error: {0}")]
    Io(String),
}

impl From<std::io::Error> for ModelError {
    fn from(e: std::io::Error) -> Self {
        ModelError::Io(e.to_string())
    }
}

impl ModelError {
    /// Shorthand for [`ModelError::Invalid`].
    pub fn invalid(reason: impl Into<String>) -> Self {
        ModelError::Invalid {
            reason: reason.into(),
        }
    }

    /// Shorthand for [`ModelError::OutOfRange`].
    pub fn range(field: &'static str, value: impl std::fmt::Display) -> Self {
        ModelError::OutOfRange {
            field,
            value: value.to_string(),
        }
    }
}

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, ModelError>;
