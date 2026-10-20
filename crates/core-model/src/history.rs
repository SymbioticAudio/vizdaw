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

/// A history group (sub-project 5 spec §4.7): every step made between
/// [`History::begin_group`] and [`History::end_group`] becomes one step.
/// Unlike a transaction, steps inside it stay undoable and redoable one by
/// one until it ends, and transactions (drags, edits) may open inside it.
#[derive(Debug, Clone)]
struct Group {
    /// `past.len()` when it opened: undo inside it stops there.
    start: usize,
    /// The redo stack when it opened (restored when it made no step).
    base_future: Vec<Version>,
    /// Redo inside it may not reach below this many future versions:
    /// those were undone before it opened (0 once it made a step, which
    /// clears the redo stack).
    redo_floor: usize,
    /// Whether any step was made inside it.
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
    group: Option<Group>,
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
            group: None,
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

    /// The lowest `past.len()` undo may reach: inside a transaction or a
    /// group, only their own steps can be undone.
    fn undo_floor(&self) -> usize {
        let t = self.txn.as_ref().map_or(0, |t| t.start);
        let g = self.group.as_ref().map_or(0, |g| g.start);
        t.max(g)
    }

    /// Whether redo may run: never inside a transaction; inside a group,
    /// only steps undone inside it.
    fn can_redo(&self) -> bool {
        self.txn.is_none()
            && self
                .group
                .as_ref()
                .is_none_or(|g| self.future.len() > g.redo_floor)
    }

    /// The document [`History::undo`] would go back to (inside a
    /// transaction, only its own steps can be undone).
    pub fn peek_undo(&self) -> Option<&Document> {
        if self.past.len() <= self.undo_floor() {
            return None;
        }
        self.past.last().map(|v| &v.doc)
    }

    /// The document [`History::redo`] would bring back (none while a
    /// transaction is open).
    pub fn peek_redo(&self) -> Option<&Document> {
        if !self.can_redo() {
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
            .flat_map(|t| std::iter::once(&t.base).chain(t.base_future.iter()))
            .chain(self.group.iter().flat_map(|g| g.base_future.iter()));
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

    /// Whether a group is open.
    pub fn in_group(&self) -> bool {
        self.group.is_some()
    }

    fn trim(&mut self) {
        if self.txn.is_some() || self.group.is_some() {
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
            // Inside a transaction the group is marked at commit: an
            // aborted drag must leave it (and its redo floor) untouched.
            if self.txn.is_none() {
                self.group_stepped();
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
        if self.past.len() <= self.undo_floor() {
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

    /// Steps forward one version. Refused while a transaction is open,
    /// and inside a group for steps undone before it opened.
    pub fn redo(&mut self) -> Result<()> {
        if !self.can_redo() {
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
        if t.executed {
            // The only place a transaction's steps count for the group.
            self.group_stepped();
        }
        self.trim();
        Ok(())
    }

    /// A step was made: an open group made one, and its redo stack is gone.
    fn group_stepped(&mut self) {
        if let Some(g) = self.group.as_mut() {
            g.executed = true;
            g.redo_floor = 0;
        }
    }

    /// Opens a group (a recording pass, spec §4.7): every step until
    /// [`History::end_group`] becomes one. Refused while a group or a
    /// transaction is open.
    pub fn begin_group(&mut self) -> Result<()> {
        if self.group.is_some() {
            return Err(ModelError::invalid("a group is already open"));
        }
        if self.txn.is_some() {
            return Err(ModelError::invalid("finish the open edit first"));
        }
        self.group = Some(Group {
            start: self.past.len(),
            base_future: self.future.clone(),
            redo_floor: self.future.len(),
            executed: false,
        });
        Ok(())
    }

    /// Closes the group: its steps become one step named `label` ("Undo
    /// Record"); a group that made none adds nothing and keeps the redo
    /// stack it found. Refused while a transaction is open (the step
    /// would cut it in two) or when no group is open.
    pub fn end_group(&mut self, label: impl Into<String>) -> Result<()> {
        if self.txn.is_some() {
            return Err(ModelError::invalid("finish the open edit first"));
        }
        let g = self
            .group
            .take()
            .ok_or_else(|| ModelError::invalid("no open group"))?;
        if self.past.len() > g.start {
            self.past.truncate(g.start + 1);
            self.present.label = label.into();
            self.present.at = Timestamp::now();
        }
        // Versions undone inside the group are intermediate states.
        self.future = if g.executed {
            Vec::new()
        } else {
            g.base_future
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
