//! The close and quit questions' texts (Swift `ClosePrompts`). While a
//! Save made during recording is still owed (it lands with the pass and
//! saves the whole song), the close question offers no Don't Save: that
//! would drop a save the user asked for.

pub const SAVE: &str = "Save";
pub const DONT_SAVE: &str = "Don't Save";
pub const CANCEL: &str = "Cancel";
/// The close question's informative text (Swift
/// `M/ClosePrompts.swift:25`).
pub const CHANGES_LOST: &str = "Your changes will be lost if you don't save them.";
/// Its informative text while a save is owed after the recording pass.
pub const SAVE_PENDING_INFORMATIVE: &str =
    "Recording is still going. The take and your changes will be saved when it stops.";
/// The quit question's informative text.
pub const QUIT_DETAIL: &str = "Some documents have unsaved changes.";

/// A question's text and buttons, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosePrompt {
    pub message: String,
    pub detail: &'static str,
    pub buttons: Vec<&'static str>,
}

/// The close question for document `name`; `save_pending`: a Save made
/// while recording is still owed (no Don't Save then).
pub fn close_prompt(name: &str, save_pending: bool) -> ClosePrompt {
    // Swift `M/ClosePrompts.swift:24`.
    let message = format!("Do you want to save the changes you made to \u{201c}{name}\u{201d}?");
    if save_pending {
        ClosePrompt {
            message,
            detail: SAVE_PENDING_INFORMATIVE,
            buttons: vec![SAVE, CANCEL],
        }
    } else {
        ClosePrompt {
            message,
            detail: CHANGES_LOST,
            buttons: vec![SAVE, DONT_SAVE, CANCEL],
        }
    }
}

/// The quit question's informative text: `saving_after_pass` are the
/// songs whose Save waits for their recording pass, which Discard Changes
/// still saves.
pub fn quit_detail(saving_after_pass: &[String]) -> String {
    let quoted: Vec<String> = saving_after_pass
        .iter()
        .map(|t| format!("\u{201c}{t}\u{201d}"))
        .collect();
    match quoted.as_slice() {
        [] => QUIT_DETAIL.into(),
        [one] => format!(
            "{QUIT_DETAIL} If you discard changes, {one} will still be saved when its recording ends."
        ),
        [rest @ .., last] => format!(
            "{QUIT_DETAIL} If you discard changes, {} and {last} will still be saved when their recordings end.",
            rest.join(", ")
        ),
    }
}
