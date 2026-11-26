//! The Recovered Recordings offer (Swift `RecordModel`'s
//! `offerRecoveryIfNeeded`, `recoveryMessage`, `answerRecovery`): the
//! recordings a project holds that nothing names, offered when its window
//! opens (never under the control channel, which answers with
//! `record.recover`).

use std::path::Path;

use vizdaw_ffi::VizError;
use vizdaw_ffi::record::RecoveredFileRow;

use super::DocumentModel;
use crate::alerts::AlertItem;

/// The offer's title.
pub const RECOVERY_TITLE: &str = "Recovered Recordings";
/// Its buttons: import (the default), move to the Trash (destructive),
/// and Later (the cancel).
pub const RECOVERY_IMPORT: &str = "Import into New Tracks";
pub const RECOVERY_TRASH: &str = "Move to Trash";
pub const RECOVERY_LATER: &str = "Later";
/// The title of a failed answer's alert.
pub const RECOVERY_FAILED: &str = "Recovery Failed";

/// The offer's text: how many recordings, their names and where they
/// came from. Only a readable file whose size was never written was left
/// by a crash; a finished take nothing names is "not in the project".
pub fn recovery_message(files: &[RecoveredFileRow]) -> String {
    let names: Vec<String> = files
        .iter()
        .map(|f| {
            Path::new(&f.path)
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    let one = names.len() == 1;
    let crashed = files.iter().filter(|f| !f.size_known && f.readable).count();
    let text = if crashed == files.len() {
        format!(
            "{} left by VizDAW quitting unexpectedly",
            if one {
                "A recording was".to_owned()
            } else {
                format!("{} recordings were", names.len())
            }
        )
    } else {
        let also = if crashed == 0 {
            String::new()
        } else {
            format!(" ({crashed} left by VizDAW quitting unexpectedly)")
        };
        format!(
            "{} not in the project{also}",
            if one {
                "A recording is".to_owned()
            } else {
                format!("{} recordings are", names.len())
            }
        )
    };
    format!("{text}: {}.", names.join(", "))
}

/// The offer's state on a document.
#[derive(Debug, Default)]
pub(crate) struct RecoveryOffer {
    files: Vec<RecoveredFileRow>,
    /// A busy or failed answer: the offer waits for the next poll.
    held: bool,
}

impl DocumentModel {
    /// Reads the recordings to offer (the app calls it when the window
    /// opens, never while the control channel is on).
    pub fn offer_recovery(&mut self) {
        self.recovery.held = false;
        self.recovery.files = self.session.recovered_files().unwrap_or_default();
    }

    /// The offer's message while the window shows it.
    pub fn recovery_offer(&self) -> Option<String> {
        (!self.recovery.files.is_empty() && !self.recovery.held)
            .then(|| recovery_message(&self.recovery.files))
    }

    /// Answers the offer: import (`Some(true)`: a new track each, one undo
    /// step), move to the Trash (`Some(false)`), or Later (`None`: offered
    /// again at the next open). A busy core or a failure keeps the offer
    /// for the next poll; a failure is also the alert "Recovery Failed".
    pub fn answer_recovery(&mut self, answer: Option<bool>) {
        let Some(import) = answer else {
            // A held offer survives the dismissal that follows a button.
            if !self.recovery.held {
                self.recovery.files.clear();
            }
            return;
        };
        match self.session.recover_recordings(import) {
            Ok(s) => {
                self.apply(s);
                self.recovery.files.clear();
            }
            Err(VizError::Busy { .. }) => self.recovery.held = true,
            Err(e) => {
                self.recovery.held = true;
                self.alert = Some(AlertItem::from_error(&e, RECOVERY_FAILED));
            }
        }
    }

    /// A held offer shows again (the poll calls this); true when it did.
    pub(crate) fn reoffer_if_held(&mut self) -> bool {
        std::mem::take(&mut self.recovery.held) && !self.recovery.files.is_empty()
    }
}
