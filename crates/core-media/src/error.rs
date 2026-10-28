//! Media errors.

/// What went wrong reading, writing or importing audio.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    /// The file system refused.
    #[error("{0}")]
    Io(String),
    /// Not a CAF file (or a damaged one).
    #[error("not a VizDAW CAF file: {0}")]
    NotCaf(String),
    /// A format VizDAW does not read or write here.
    #[error("unsupported audio: {0}")]
    Unsupported(String),
    /// The operation was cancelled.
    #[error("cancelled")]
    Cancelled,
}

impl From<std::io::Error> for MediaError {
    fn from(e: std::io::Error) -> MediaError {
        MediaError::Io(e.to_string())
    }
}
