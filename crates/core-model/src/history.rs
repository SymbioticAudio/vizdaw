//! Multi-level undo with transactions, the document selection and
//! "Modify Again". History is never saved with the project.

use crate::command::{Command, Context, Report, apply_in};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::ids::{PluginInstanceId, StateBlobId};
use crate::modify::Modify;
use crate::rule::SelectMode;
use crate::selection::{Selection, ViewFilter};
use std::collections::BTreeMap;

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
    /// Whether any command inside the transaction made an undo step.
    executed: bool,
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

    /// The document [`History::undo`] would go back to (inside a
    /// transaction, only its own steps can be undone).
    pub fn peek_undo(&self) -> Option<&Document> {
        let floor = self.txn.as_ref().map_or(0, |t| t.start);
        if self.past.len() <= floor {
            return None;
        }
        self.past.last().map(|v| &v.doc)
    }

    /// The document [`History::redo`] would bring back (none while a
    /// transaction is open).
    pub fn peek_redo(&self) -> Option<&Document> {
        if self.txn.is_some() {
            return None;
        }
        self.future.last().map(|v| &v.doc)
    }

    /// Every media file the history references: the package-relative
    /// path of each item of the present document, of every undo and redo
    /// state, and (while a transaction is open) of the states it would
    /// restore on abort, with the item's content hash when any state
    /// knows it. Media files are write-once, so a path names one audio.
    ///
    /// This is the set of files a project must keep (final review I3):
    /// Save As carries all of them, so Undo and Redo after Save As find
    /// their media; plan 5b's retention rule (which files may go to the
    /// Trash on save or close) is defined over this same set, never over
    /// the present document alone.
    pub fn referenced_media_anywhere(&self) -> BTreeMap<String, Option<String>> {
        let mut out: BTreeMap<String, Option<String>> = BTreeMap::new();
        let txn = self
            .txn
            .iter()
            .flat_map(|t| std::iter::once(&t.base).chain(t.base_future.iter()));
        let mut last: Option<&Document> = None;
        for v in self
            .past
            .iter()
            .chain(std::iter::once(&self.present))
            .chain(self.future.iter())
            .chain(txn)
        {
            // Neighbouring states usually share their media map.
            if last.is_some_and(|d| d.media().ptr_eq(v.doc.media())) {
                continue;
            }
            last = Some(&v.doc);
            for m in v.doc.media().values() {
                let e = out.entry(m.path.clone()).or_default();
                if e.is_none() {
                    e.clone_from(&m.content_hash);
                }
            }
        }
        out
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

    /// Applies a command. On error nothing changes; a command that changes
    /// nothing adds no undo step. A selection-only
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
        // A command that changes nothing (e.g. a modify of an empty
        // selection) is not an undo step and keeps the redo stack.
        if !selection_only && applied.doc != self.present.doc {
            let next = Version {
                doc: applied.doc,
                label: cmd.name().to_string(),
                at: Timestamp::now(),
            };
            self.past.push(std::mem::replace(&mut self.present, next));
            self.future.clear();
            if let Some(t) = self.txn.as_mut() {
                t.executed = true;
            }
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

    /// Steps forward one version. Refused while a transaction is open.
    pub fn redo(&mut self) -> Result<()> {
        if self.txn.is_some() {
            return Err(ModelError::invalid("nothing to redo"));
        }
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
            executed: false,
        });
        Ok(())
    }

    /// Closes the transaction, collapsing its steps into one. If it made
    /// any step, nothing is left to redo.
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
        // Versions undone inside the transaction are intermediate states
        // that must not be redone; an empty transaction keeps the old redo
        // stack.
        self.future = if t.executed {
            Vec::new()
        } else {
            t.base_future
        };
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

    /// Stores `bytes` as plugin instance `slot`'s state and clears its
    /// overlay in the present version, without an undo step (Plan Ruling
    /// 9). Refused inside an open transaction.
    pub fn capture_state(&mut self, slot: PluginInstanceId, bytes: &[u8]) -> Result<StateBlobId> {
        if self.txn.is_some() {
            return Err(ModelError::invalid(
                "cannot capture plugin state inside a transaction",
            ));
        }
        let (doc, id) = crate::state::capture(&self.present.doc, slot, bytes)?;
        self.present.doc = doc;
        Ok(id)
    }
}
