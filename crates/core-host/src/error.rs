//! Errors plugins can cause (spec §12). The host survives every one of
//! them; only a crash ends the process.

/// A plugin operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    /// No plugin with this format and id is known (not scanned, or its
    /// bundle is gone).
    #[error("plugin not found: {0}")]
    NotFound(String),
    /// Loading or instantiating failed.
    #[error("load failed: {0}")]
    LoadFailed(String),
    /// The plugin refused a saved state.
    #[error("state rejected: {0}")]
    StateRejected(String),
    /// Activation (allocating for a sample rate and block size) failed.
    #[error("activate failed: {0}")]
    ActivateFailed(String),
    /// The plugin has no editor, or it could not be shown.
    #[error("editor unavailable: {0}")]
    EditorUnavailable(String),
    /// The plugin is disabled or blocklisted in the Plugin Manager.
    #[error("disabled: {0}")]
    Disabled(String),
}
