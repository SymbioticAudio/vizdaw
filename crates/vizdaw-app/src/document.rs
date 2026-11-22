//! One open document (Swift: `DocumentModel`, the title, save and undo
//! parts; transport comes with Task 3).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vizdaw_ffi::VizError;
use vizdaw_ffi::snapshot::DocSnapshot;

use crate::alerts::{AlertItem, error_message};
use crate::session::DocSession;

/// What File > Save does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveAction {
    Saved,
    /// Untitled: ask for a location; nothing was written.
    NeedsSaveAs,
    Failed(String),
}

pub struct DocumentModel {
    session: Arc<dyn DocSession>,
    snapshot: DocSnapshot,
    /// The error showing, if any.
    pub alert: Option<AlertItem>,
}

impl DocumentModel {
    pub fn new(session: Arc<dyn DocSession>) -> Result<Self, VizError> {
        let snapshot = session.snapshot()?;
        Ok(DocumentModel {
            session,
            snapshot,
            alert: None,
        })
    }

    pub fn session(&self) -> &Arc<dyn DocSession> {
        &self.session
    }

    pub fn snapshot(&self) -> &DocSnapshot {
        &self.snapshot
    }

    /// The session's package path, read each time; `None` when untitled.
    pub fn path(&self) -> Option<PathBuf> {
        self.session.path().ok().flatten().map(PathBuf::from)
    }

    /// True when Save needs a path first (untitled).
    pub fn needs_save_as(&self) -> bool {
        self.path().is_none()
    }

    /// The window title: the name, marked when edited.
    pub fn title(&self) -> String {
        if self.snapshot.dirty {
            format!("{} \u{2014} Edited", self.snapshot.title)
        } else {
            self.snapshot.title.clone()
        }
    }

    pub fn undo_title(&self) -> String {
        label_title("Undo", &self.snapshot.undo_label)
    }

    pub fn redo_title(&self) -> String {
        label_title("Redo", &self.snapshot.redo_label)
    }

    pub fn can_undo(&self) -> bool {
        self.snapshot.undo_label.is_some()
    }

    pub fn can_redo(&self) -> bool {
        self.snapshot.redo_label.is_some()
    }

    /// Undo and Redo do nothing, silently, while the plugin host is busy.
    pub fn undo(&mut self) {
        self.step("Undo Failed", |s| s.undo());
    }

    pub fn redo(&mut self) {
        self.step("Redo Failed", |s| s.redo());
    }

    fn step(
        &mut self,
        title: &str,
        body: impl FnOnce(&dyn DocSession) -> Result<DocSnapshot, VizError>,
    ) {
        match body(self.session.as_ref()) {
            Ok(snapshot) => self.snapshot = snapshot,
            Err(VizError::Busy { .. }) => {}
            Err(e) => self.alert = Some(AlertItem::from_error(&e, title)),
        }
    }

    /// Saves in place. An untitled document is never written: the caller
    /// asks for a location and calls `save_as`.
    pub fn save(&mut self) -> SaveAction {
        if self.path().is_none() {
            return SaveAction::NeedsSaveAs;
        }
        match self.session.save().and_then(|()| self.session.snapshot()) {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                SaveAction::Saved
            }
            Err(e) => {
                self.alert = Some(AlertItem::from_error(&e, "Save Failed"));
                SaveAction::Failed(error_message(&e))
            }
        }
    }

    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
        let result = self
            .session
            .save_as(path.to_string_lossy().into_owned())
            .and_then(|()| self.session.snapshot());
        match result {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                Ok(())
            }
            Err(e) => {
                self.alert = Some(AlertItem::from_error(&e, "Save Failed"));
                Err(error_message(&e))
            }
        }
    }
}

fn label_title(verb: &str, label: &Option<String>) -> String {
    match label {
        Some(l) => format!("{verb} {l}"),
        None => verb.to_string(),
    }
}
