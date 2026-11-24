//! Relink Media… (Swift `MediaModel`'s relink part and `RelinkSheet`): the
//! items that are missing or changed ([`DocumentModel::relinkable`]),
//! each relinked to a file the user locates, or all of them searched for
//! in a folder — each a `Relink` job ("Relink Failed" when it fails). The
//! sheet is offered by itself when the project opens with such items, and
//! again when idle or a recheck finds new ones (each item once; an item
//! back online is forgotten); never under the control channel, where the
//! items stay unoffered until it is off. A held DAWproject merge blocks a
//! relink ([`HELD_HELP`]).

use std::collections::BTreeSet;

use vizdaw_ffi::{JobKindRow, MediaRow, OfflineReasonRow};

use crate::document::DocumentModel;

/// The sheet's title (also its label).
pub const TITLE: &str = "Some audio files of this project are missing or changed";
/// The line under the title.
pub const MESSAGE: &str = "Their regions stay silent until each item is relinked to its file. Each relink is one undo step.";
/// Each row's button.
pub const LOCATE: &str = "Locate\u{2026}";
/// The folder search's button.
pub const SEARCH_FOLDER: &str = "Search Folder\u{2026}";
/// The sheet's cancel button (Esc).
pub const LEAVE_OFFLINE: &str = "Leave Offline";
/// The alert title of a refused or failed relink.
pub const FAILED: &str = "Relink Failed";
/// Why Locate… and Search Folder… wait while a merge is held (Swift
/// `MergeModel.heldHelp`).
pub const HELD_HELP: &str = crate::project_settings::HELD_HELP;

/// Why an item is offline, as the sheet and the pool say it (Swift
/// `MediaModel.reasonText`).
pub fn reason_text(r: OfflineReasonRow) -> &'static str {
    match r {
        OfflineReasonRow::Missing => "Missing",
        OfflineReasonRow::Unreadable => "Not a VizDAW file",
        OfflineReasonRow::Format => "Another format",
        OfflineReasonRow::Length => "Changed length",
        OfflineReasonRow::Changed => "Changed",
    }
}

/// A row's second line: `<reason> · <path>`.
pub fn detail(r: &MediaRow) -> String {
    format!(
        "{} \u{b7} {}",
        r.offline.map(reason_text).unwrap_or(""),
        r.path
    )
}

/// A row's Locate… accessibility label.
pub fn locate_label(name: &str) -> String {
    format!("Locate {name}")
}

/// The offer's state on a document.
#[derive(Debug)]
pub(crate) struct RelinkOffer {
    /// The relinkable items the sheet was already offered for, so Leave
    /// Offline is not undone by the same items.
    offered: BTreeSet<u64>,
    /// The sheet is asked for (Swift `relinkShown`).
    shown: bool,
    /// Something changed that the offer must look at: the open, idle's
    /// media news, a recheck that found changes.
    due: bool,
}

impl Default for RelinkOffer {
    fn default() -> Self {
        RelinkOffer {
            offered: BTreeSet::new(),
            shown: false,
            // A project that opens with missing media offers the sheet.
            due: true,
        }
    }
}

impl DocumentModel {
    /// A DAWproject merge is held: the core refuses a relink beside it,
    /// so Locate… and Search Folder… wait.
    pub fn relink_blocked(&self) -> bool {
        self.merge_held()
    }

    /// Locate…: relinks item `media` to the file at `path` (a job).
    pub fn relink(&mut self, media: u64, path: &str) {
        if self.relink_blocked() {
            return;
        }
        let path = path.to_owned();
        self.start_job(JobKindRow::Relink, FAILED, |s| s.relink_media(media, path));
    }

    /// Search Folder…: every missing or changed item, looked for in
    /// `folder` (a job).
    pub fn search_folder(&mut self, folder: &str) {
        if self.relink_blocked() {
            return;
        }
        let folder = folder.to_owned();
        self.start_job(JobKindRow::Relink, FAILED, |s| s.search_media(folder));
    }

    /// The app became active: files may have changed on disk. True when
    /// the offline set changed (the media is read again and the offer is
    /// due).
    pub fn recheck_media(&mut self) -> bool {
        if !matches!(self.session.recheck_media(), Ok(true)) {
            return false;
        }
        self.media_version += 1;
        self.refresh_project_info();
        self.relink.due = true;
        true
    }

    /// Idle found the media changed: the offer is due.
    pub(crate) fn relink_due(&mut self) {
        self.relink.due = true;
    }

    /// [`DocumentModel::offer_relink_if_needed`] when something changed
    /// since the last offer (the app calls it as it presents; on open,
    /// after idle's media news and after a recheck, as Swift does).
    pub fn offer_relink_if_due(&mut self, channel_on: bool) {
        if std::mem::take(&mut self.relink.due) {
            self.offer_relink_if_needed(channel_on);
        }
    }

    /// Asks for the sheet when items are missing or changed that it was
    /// not yet offered for. Items back online are forgotten, so a new loss
    /// is offered again. Under the control channel nothing is asked for
    /// and nothing new is marked offered (it is offered once the channel
    /// is off).
    pub fn offer_relink_if_needed(&mut self, channel_on: bool) {
        let ids: BTreeSet<u64> = self.relinkable().iter().map(|r| r.id).collect();
        if channel_on {
            self.relink.offered.retain(|id| ids.contains(id));
            return;
        }
        let fresh = ids.difference(&self.relink.offered).next().is_some();
        self.relink.offered = ids;
        if fresh {
            self.relink.shown = true;
        }
    }

    /// Relink Media…: the sheet again.
    pub fn show_relink(&mut self) {
        self.relink.shown = true;
    }

    /// Leave Offline.
    pub fn leave_offline(&mut self) {
        self.relink.shown = false;
    }

    /// The sheet is asked for.
    pub fn relink_shown(&self) -> bool {
        self.relink.shown
    }

    /// Whether the window shows the sheet: asked for, something left to
    /// relink, and never while the control channel is on.
    pub fn shows_relink_sheet(&self, channel_on: bool) -> bool {
        self.relink.shown && !channel_on && !self.relinkable().is_empty()
    }
}
