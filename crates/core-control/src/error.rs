//! Error codes and the error object a failed reply carries (spec §5.3).

use serde_json::{Map, Value};
use std::fmt;

/// Why a request failed: the `error.code` string on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// The line is not UTF-8 JSON, or longer than [`crate::protocol::MAX_LINE`].
    ParseError,
    /// JSON, but not a request object (`id`, `cmd`, `params`).
    InvalidRequest,
    /// `cmd` names no command.
    UnknownCommand,
    /// A parameter is missing, unknown, of the wrong type or out of range.
    InvalidParams,
    /// `command.run` names no registered menu command.
    UnknownId,
    /// No document is open (or the given window shows none).
    NoDocument,
    /// The window number names no VizDAW window.
    NoWindow,
    /// The element ID resolves to nothing (or to an element that changed).
    NoElement,
    /// The call would open a dialog; `dialog` names it, the parameters
    /// that answer it are listed in `commands.list`.
    DialogRequired,
    /// A plugin call is on the main thread's stack (a nested run loop);
    /// try again.
    Busy,
    /// The action ran and failed (the message is what the alert would say).
    Failed,
    /// The command is disabled right now (its menu item would be grey).
    Unavailable,
    /// No answer in time (a wait whose condition never held, or an app
    /// that did not answer).
    Timeout,
    /// The channel is stopping (the app is quitting).
    ShuttingDown,
    /// A bug: a caught panic or an impossible state.
    Internal,
}

impl ErrorCode {
    /// Every code, in the spec's order.
    pub const ALL: [ErrorCode; 15] = [
        ErrorCode::ParseError,
        ErrorCode::InvalidRequest,
        ErrorCode::UnknownCommand,
        ErrorCode::InvalidParams,
        ErrorCode::UnknownId,
        ErrorCode::NoDocument,
        ErrorCode::NoWindow,
        ErrorCode::NoElement,
        ErrorCode::DialogRequired,
        ErrorCode::Busy,
        ErrorCode::Failed,
        ErrorCode::Unavailable,
        ErrorCode::Timeout,
        ErrorCode::ShuttingDown,
        ErrorCode::Internal,
    ];

    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::ParseError => "parse_error",
            ErrorCode::InvalidRequest => "invalid_request",
            ErrorCode::UnknownCommand => "unknown_command",
            ErrorCode::InvalidParams => "invalid_params",
            ErrorCode::UnknownId => "unknown_id",
            ErrorCode::NoDocument => "no_document",
            ErrorCode::NoWindow => "no_window",
            ErrorCode::NoElement => "no_element",
            ErrorCode::DialogRequired => "dialog_required",
            ErrorCode::Busy => "busy",
            ErrorCode::Failed => "failed",
            ErrorCode::Unavailable => "unavailable",
            ErrorCode::Timeout => "timeout",
            ErrorCode::ShuttingDown => "shutting_down",
            ErrorCode::Internal => "internal",
        }
    }

    /// The code of a wire string.
    pub fn parse(s: &str) -> Option<ErrorCode> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// The error of a failed reply.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlError {
    /// What kind of failure.
    pub code: ErrorCode,
    /// One human-readable sentence.
    pub message: String,
    /// The dialog the call would have opened (`dialog_required`).
    pub dialog: Option<String>,
    /// Extra data (a wait's last facts).
    pub details: Option<Value>,
}

impl ControlError {
    /// An error without dialog or details.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> ControlError {
        ControlError {
            code,
            message: message.into(),
            dialog: None,
            details: None,
        }
    }

    /// `invalid_params` for command `cmd`.
    pub fn params(cmd: &str, message: impl fmt::Display) -> ControlError {
        ControlError::new(ErrorCode::InvalidParams, format!("{cmd}: {message}"))
    }

    /// `dialog_required` naming `dialog` and the parameters that answer it.
    pub fn dialog(dialog: &str, answered_by: &[&str]) -> ControlError {
        let params = answered_by
            .iter()
            .map(|p| format!("`{p}`"))
            .collect::<Vec<_>>()
            .join(", ");
        ControlError {
            code: ErrorCode::DialogRequired,
            message: format!("this would open the \"{dialog}\" dialog; pass {params} instead"),
            dialog: Some(dialog.to_string()),
            details: None,
        }
    }

    /// The same error with `details`.
    pub fn with_details(mut self, details: Value) -> ControlError {
        self.details = Some(details);
        self
    }

    /// The wire object: `code`, `message`, then `dialog` and `details`
    /// when present.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("code".into(), Value::from(self.code.as_str()));
        m.insert("message".into(), Value::from(self.message.clone()));
        if let Some(d) = &self.dialog {
            m.insert("dialog".into(), Value::from(d.clone()));
        }
        if let Some(d) = &self.details {
            m.insert("details".into(), d.clone());
        }
        Value::Object(m)
    }

    /// The error a reply's `error` object holds; None when it is not one.
    pub fn from_json(v: &Value) -> Option<ControlError> {
        let code = ErrorCode::parse(v.get("code")?.as_str()?)?;
        Some(ControlError {
            code,
            message: v.get("message")?.as_str()?.to_string(),
            dialog: v.get("dialog").and_then(Value::as_str).map(str::to_string),
            details: v.get("details").cloned(),
        })
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ControlError {}
