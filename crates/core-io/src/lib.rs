//! Shared base for file import and export: the hostile-input limits, guarded
//! reads, partial-file writes and the report of what changed on the way.

pub mod limits;
pub mod read;
pub mod report;
pub mod text;
pub mod write;

/// Why a read, write or conversion did not happen.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IoError {
    /// The input or request is not acceptable; the sentence says why.
    #[error("{0}")]
    Refused(String),
    /// The operating system failed; the sentence says how.
    #[error("{0}")]
    Io(String),
    /// The user cancelled.
    #[error("Cancelled")]
    Cancelled,
}

impl From<std::io::Error> for IoError {
    fn from(e: std::io::Error) -> Self {
        IoError::Io(e.to_string())
    }
}
