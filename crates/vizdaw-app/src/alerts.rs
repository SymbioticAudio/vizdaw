//! Errors shown to the user (Swift: `AlertItem`, `VizError.message`).

use std::sync::atomic::{AtomicU64, Ordering};

use vizdaw_ffi::VizError;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// An error shown to the user.
#[derive(Debug, Clone)]
pub struct AlertItem {
    /// Unique per alert, so a repeat of the same text is still one alert.
    pub id: u64,
    pub title: String,
    pub message: String,
}

impl AlertItem {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        AlertItem {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            title: title.into(),
            message: message.into(),
        }
    }

    /// An alert for a core error; its message is the human-readable text.
    pub fn from_error(error: &VizError, title: impl Into<String>) -> Self {
        AlertItem::new(title, error_message(error))
    }
}

impl PartialEq for AlertItem {
    fn eq(&self, other: &Self) -> bool {
        self.title == other.title && self.message == other.message
    }
}

/// The human-readable text of a core error (Swift: `VizError.message`).
pub fn error_message(error: &VizError) -> String {
    match error {
        VizError::NotFound { path } => format!("No VizDAW project at {path}."),
        VizError::Model { message }
        | VizError::Io { message }
        | VizError::Engine { message }
        | VizError::Busy { message } => message.clone(),
        VizError::NoDevice { message } => format!("No audio output: {message}"),
        VizError::Exists { path } => {
            format!("Something that is not Multi-Q is in the way: {path}.")
        }
        VizError::Internal { message } => format!("Internal error: {message}"),
    }
}
