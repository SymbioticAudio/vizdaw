//! Multi-level undo with transactions, the document selection and
//! "Modify Again". History is never saved with the project.

use crate::command::{Command, Context, Report, apply_in};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::modify::Modify;
use crate::rule::SelectMode;
use crate::selection::{Selection, ViewFilter};

/// Milliseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// The current time (0 if the clock is before the epoch).
    pub fn now() -> Timestamp {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Timestamp(u64::try_from(ms).unwrap_or(u64::MAX))
    }
}

/// One document state with the label of the step that produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct Version {
    /// The document.
    pub doc: Document,
    /// Undo label ("Quantize", or a transaction label).
    pub label: String,
    /// When it was made.
    pub at: Timestamp,
}

#[derive(Debug, Clone)]
struct Txn {
    label: String,
    start: usize,
    base: Version,
    base_future: Vec<Version>,
}

/// Undo/redo stacks plus the selection and the last modification.
#[derive(Debug, Clone)]
pub struct History {
    past: Vec<Version>,
    present: Version,
    future: Vec<Version>,
    last_modify: Option<Modify>,
    selection: Selection,
    txn: Option<Txn>,
}

impl History {
    /// A history whose present is `doc` with nothing to undo.
    pub fn new(doc: Document) -> History {
        History {
            past: Vec::new(),
            present: Version {
                doc,
                label: String::new(),
                at: Timestamp::now(),
            },
            future: Vec::new(),
            last_modify: None,
            selection: Selection::default(),
            txn: None,
        }
    }

    /// The current document.
    pub fn doc(&self) -> &Document {
        &self.present.doc
    }

    /// The current version.
    pub fn present(&self) -> &Version {
        &self.present
    }

    /// The selection.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Replaces the selection (not an undo step).
    pub fn set_selection(&mut self, s: Selection) {
        self.selection = s.retain_existing(&self.present.doc);
    }

    /// The last modification applied by select-and-modify.
    pub fn last_modify(&self) -> Option<&Modify> {
        self.last_modify.as_ref()
    }

    /// Number of steps that can be undone.
    pub fn undo_depth(&self) -> usize {
        self.past.len()
    }

    /// Label of the step undo would revert.
    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|_| self.present.label.as_str())
    }

    /// Label of the step redo would re-apply.
    pub fn redo_label(&self) -> Option<&str> {
        self.future.last().map(|v| v.label.as_str())
    }

    /// Whether a transaction is open.
    pub fn in_transaction(&self) -> bool {
        self.txn.is_some()
    }

    fn trim(&mut self) {
        if self.txn.is_some() {
            return;
        }
        let depth =
            usize::try_from(self.present.doc.settings().history_depth.max(1)).unwrap_or(usize::MAX);
        if self.past.len() > depth {
            let excess = self.past.len() - depth;
            self.past.drain(..excess);
        }
    }

    /// Applies a command. On error nothing changes. A selection-only
    /// select-and-modify updates the selection without an undo step.
    pub fn execute(&mut self, cmd: &Command, filter: &ViewFilter) -> Result<Report> {
        let applied = apply_in(
            &self.present.doc,
            Context {
                selection: &self.selection,
                filter,
            },
            cmd,
        )?;
        let selection_only = matches!(cmd, Command::SelectAndModify { modify: None, .. });
        if let Command::SelectAndModify {
            modify: Some(m), ..
        } = cmd
        {
            self.last_modify = Some(m.clone());
        }
        if !selection_only {
            let next = Version {
                doc: applied.doc,
                label: cmd.name().to_string(),
                at: Timestamp::now(),
            };
            self.past.push(std::mem::replace(&mut self.present, next));
            self.future.clear();
            self.trim();
        }
        let sel = applied.selection.unwrap_or_else(|| self.selection.clone());
        self.selection = sel.retain_existing(&self.present.doc);
        Ok(applied.report)
    }

    /// Replays the last modification on the current selection.
    pub fn modify_again(&mut self, filter: &ViewFilter) -> Result<Report> {
        let m = self
            .last_modify
            .clone()
            .ok_or_else(|| ModelError::invalid("nothing to modify again"))?;
        self.execute(
            &Command::SelectAndModify {
                rule: None,
                mode: SelectMode::Select,
                modify: Some(m),
                to_copy: false,
            },
            filter,
        )
    }

    /// Steps back one version. Inside a transaction, only the transaction's
    /// own steps can be undone.
    pub fn undo(&mut self) -> Result<()> {
        let floor = self.txn.as_ref().map_or(0, |t| t.start);
        if self.past.len() <= floor {
            return Err(ModelError::invalid("nothing to undo"));
        }
        let prev = self
            .past
            .pop()
            .ok_or_else(|| ModelError::invalid("nothing to undo"))?;
        self.future.push(std::mem::replace(&mut self.present, prev));
        self.selection = self.selection.retain_existing(&self.present.doc);
        Ok(())
    }

    /// Steps forward one version.
    pub fn redo(&mut self) -> Result<()> {
        let next = self
            .future
            .pop()
            .ok_or_else(|| ModelError::invalid("nothing to redo"))?;
        self.past.push(std::mem::replace(&mut self.present, next));
        self.selection = self.selection.retain_existing(&self.present.doc);
        Ok(())
    }

    /// Opens a transaction: commands until [`History::commit`] become one
    /// undo step named `label`.
    pub fn begin(&mut self, label: impl Into<String>) -> Result<()> {
        if self.txn.is_some() {
            return Err(ModelError::invalid("a transaction is already open"));
        }
        self.txn = Some(Txn {
            label: label.into(),
            start: self.past.len(),
            base: self.present.clone(),
            base_future: self.future.clone(),
        });
        Ok(())
    }

    /// Closes the transaction, collapsing its steps into one.
    pub fn commit(&mut self) -> Result<()> {
        let t = self
            .txn
            .take()
            .ok_or_else(|| ModelError::invalid("no open transaction"))?;
        if self.past.len() > t.start {
            self.past.truncate(t.start);
            self.past.push(t.base);
            self.present.label = t.label;
            self.present.at = Timestamp::now();
        }
        self.trim();
        Ok(())
    }

    /// Closes the transaction and restores the state from before it.
    pub fn abort(&mut self) -> Result<()> {
        let t = self
            .txn
            .take()
            .ok_or_else(|| ModelError::invalid("no open transaction"))?;
        self.past.truncate(t.start);
        self.present = t.base;
        self.future = t.base_future;
        self.selection = self.selection.retain_existing(&self.present.doc);
        Ok(())
    }
}
