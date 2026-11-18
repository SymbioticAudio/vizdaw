//! Multi-level undo with transactions, the document selection and
//! "Modify Again". History is never saved with the project.

use crate::command::strips::fill_new_eqs;
use crate::command::{Command, Context, Report, apply_in};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::ids::{PluginInstanceId, StateBlobId};
use crate::mixer::PluginRef;
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
    /// Which step made it: unique within its history, kept through undo
    /// and redo ([`History::present_serial`]).
    pub serial: u64,
}

#[derive(Debug, Clone)]
struct Txn {
    label: String,
    start: usize,
    base: Version,
    base_future: Vec<Version>,
    /// Whether any command inside the transaction made an undo step.
    executed: bool,
    /// Whether it was begun while acting (Co-Producer's): turning acting
    /// off aborts it, so no hand edit can join it.
    acting: bool,
}

/// A history group (sub-project 5 spec §4.7): every step made between
/// [`History::begin_group`] and [`History::end_group`] becomes one step.
/// Unlike a transaction, steps inside it stay undoable and redoable one by
/// one until it ends, and transactions (drags, edits) may open inside it.
#[derive(Debug, Clone)]
struct Group {
    /// What opened it.
    kind: GroupKind,
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

/// What a [`Group`] is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    /// A recording pass ([`History::begin_group`]).
    Record,
    /// Co-Producer's steps in a request (plan 8a Ruling 3): opened by its
    /// first acting step, ended by any hand action, undo, redo or the end
    /// of the request.
    Request,
}

/// The open Co-Producer request (plan 8a Ruling 3).
#[derive(Debug, Clone)]
struct Request {
    name: String,
    title: String,
    label: String,
    /// `label` plus " (continued)".
    continued_label: String,
    /// An earlier group of it made a step.
    continued: bool,
}

impl Request {
    /// The label its open group's step gets.
    fn group_label(&self) -> &str {
        if self.continued {
            &self.continued_label
        } else {
            &self.label
        }
    }
}

/// What a request is (8a Ruling 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestInfo {
    /// The name as given, trimmed.
    pub name: String,
    /// Its title ([`request_label`]).
    pub title: String,
    /// "Co-Producer: <title>".
    pub label: String,
    /// A group of it is open now.
    pub group_open: bool,
    /// An earlier group of it made a step: the next is "(continued)".
    pub continued: bool,
}

/// Request names are at most this many characters, after trimming.
const MAX_REQUEST_NAME: usize = 200;
/// Titles longer than this are cut to one less plus "…".
const MAX_REQUEST_TITLE: usize = 27;
/// Words kept lower case unless first.
const SMALL_WORDS: [&str; 15] = [
    "a", "an", "and", "as", "at", "but", "by", "for", "in", "of", "on", "or", "the", "to", "with",
];

/// Characters a request name may not hold: controls, bidi controls and
/// marks, and line and paragraph separators (8a Ruling 4, Ruling P11).
fn refused_in_name(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}'
                | '\u{200E}'
                | '\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{2028}'
                | '\u{2029}'
        )
}

/// A request's title and undo label (8a Ruling 4): runs of whitespace
/// become one space, each word is capitalised but small words after the
/// first, and a title over 27 characters is cut to 26 plus "…", so the
/// label "Co-Producer: <title>" is at most 40 characters.
/// `ModelError::Invalid` for a name that is empty or over 200 characters
/// after trimming, or that holds a refused character.
pub fn request_label(name: &str) -> Result<(String, String)> {
    if name.chars().any(refused_in_name) {
        return Err(ModelError::invalid(
            "A request name cannot hold control characters, direction marks or line breaks.",
        ));
    }
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ModelError::invalid("Name the request."));
    }
    if trimmed.chars().count() > MAX_REQUEST_NAME {
        return Err(ModelError::invalid(
            "A request name is at most 200 characters.",
        ));
    }
    let words: Vec<String> = trimmed
        .split_whitespace()
        .enumerate()
        .map(|(i, w)| {
            if i > 0 && SMALL_WORDS.contains(&w) {
                return w.to_string();
            }
            let mut cs = w.chars();
            cs.next()
                .map(|c| c.to_uppercase().chain(cs).collect())
                .unwrap_or_default()
        })
        .collect();
    let mut title = words.join(" ");
    if title.chars().count() > MAX_REQUEST_TITLE {
        title = title.chars().take(MAX_REQUEST_TITLE - 1).collect();
        title.push('…');
    }
    let label = format!("Co-Producer: {title}");
    Ok((title, label))
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
    /// The serial the next step gets.
    next_serial: u64,
    /// The EQ every new strip but master gets (plan 6g Rulings 5, 6; plan
    /// 6g-2 Ruling 1); `None`: none.
    new_strip_eq: Option<PluginRef>,
    /// A line added to the warnings of each step whose fill put in at least
    /// one slot (plan 6g-2 Ruling 4); `None`: none.
    new_strip_eq_note: Option<String>,
    /// The open Co-Producer request (8a Ruling 3).
    request: Option<Request>,
    /// Whether steps now are Co-Producer's ([`History::set_acting`]).
    acting: bool,
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
                serial: 0,
            },
            future: Vec::new(),
            last_modify: None,
            selection: Selection::default(),
            txn: None,
            group: None,
            next_serial: 1,
            new_strip_eq: None,
            new_strip_eq_note: None,
            request: None,
            acting: false,
        }
    }

    /// The EQ every new strip but master gets (plan 6g Rulings 5, 6; plan
    /// 6g-2 Ruling 1); `None`: none. Read by each [`History::execute`];
    /// undo and redo restore stored versions and never read it.
    pub fn set_new_strip_eq(&mut self, plugin: Option<PluginRef>) {
        self.new_strip_eq = plugin;
    }

    /// A line added to the warnings of each step whose fill put in at least
    /// one slot (6g-2 Ruling 4).
    pub fn set_new_strip_eq_note(&mut self, note: Option<String>) {
        self.new_strip_eq_note = note;
    }

    /// The EQ new strips get, if any ([`History::set_new_strip_eq`]).
    pub fn new_strip_eq(&self) -> Option<&PluginRef> {
        self.new_strip_eq.as_ref()
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

    /// Number of steps that can be undone. An open request group that
    /// made steps counts as the one step [`History::undo`] closes it into.
    pub fn undo_depth(&self) -> usize {
        self.pending_request_start()
            .map_or(self.past.len(), |start| start + 1)
    }

    /// Label of the step undo would revert: for an open request group
    /// that made steps, the label it closes with.
    pub fn undo_label(&self) -> Option<&str> {
        if self.pending_request_start().is_some()
            && let Some(r) = &self.request
        {
            return Some(r.group_label());
        }
        self.past.last().map(|_| self.present.label.as_str())
    }

    /// Where the open request group starts, when it made steps and the
    /// next undo or redo would close it (no transaction is open): undo
    /// and its previews then see the group as one closed step.
    fn pending_request_start(&self) -> Option<usize> {
        let g = self.group.as_ref()?;
        (g.kind == GroupKind::Request && self.txn.is_none() && self.past.len() > g.start)
            .then_some(g.start)
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
    /// transaction, only its own steps can be undone; an open request
    /// group counts as the one step undo closes it into).
    pub fn peek_undo(&self) -> Option<&Document> {
        if let Some(start) = self.pending_request_start() {
            return self.past.get(start).map(|v| &v.doc);
        }
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

    /// Whether a recording group is open. A Co-Producer request's group
    /// does not count: any hand action ends it first.
    pub fn in_group(&self) -> bool {
        self.group
            .as_ref()
            .is_some_and(|g| g.kind == GroupKind::Record)
    }

    /// Whether a Co-Producer request's group is open.
    fn request_group_open(&self) -> bool {
        self.group
            .as_ref()
            .is_some_and(|g| g.kind == GroupKind::Request)
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
    /// Every new strip but master the command made gets
    /// [`History::new_strip_eq`] in its EQ slot, in the same step; when at
    /// least one slot is filled, the note
    /// ([`History::set_new_strip_eq_note`]) is added to the warnings.
    /// Then Co-Producer's marks settle ([`crate::marks::settle`]): undo
    /// and redo restore whole versions, so they need nothing.
    pub fn execute(&mut self, cmd: &Command, filter: &ViewFilter) -> Result<Report> {
        if !self.acting {
            self.before_hand_action();
        }
        let mut applied = apply_in(
            &self.present.doc,
            Context {
                selection: &self.selection,
                filter,
            },
            cmd,
        )?;
        if let Some(p) = &self.new_strip_eq {
            let filled = fill_new_eqs(&mut applied.doc, &mut applied.report, p);
            if filled > 0
                && let Some(note) = &self.new_strip_eq_note
            {
                applied.report.warnings.push(note.clone());
            }
        }
        crate::marks::settle(&self.present.doc, &mut applied.doc, self.acting);
        // Only a select-and-modify is remembered for Modify Again: a
        // `ModifyNotes` (plan 8c Ruling 2) leaves it, and the selection,
        // as they were.
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
            if self.txn.is_none() {
                self.open_request_group();
            }
            let serial = self.next_serial;
            self.next_serial = self.next_serial.saturating_add(1);
            let next = Version {
                doc: applied.doc,
                label: cmd.name().to_string(),
                at: Timestamp::now(),
                serial,
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

    /// The present version's serial: which step made it. Each step gets
    /// its own; undo and redo keep them, a transaction or group that
    /// collapses into one step keeps its last step's.
    pub fn present_serial(&self) -> u64 {
        self.present.serial
    }

    /// Applies `cmd` into the present step instead of making a new one
    /// (plan 5e-1 Task 12: a looped pass lands at each wrap into its one
    /// undo step), when the present is the step with serial `step`, there
    /// is a step to undo, and no transaction or group is open: one Undo
    /// then reverts the step with everything amended into it. Clears the
    /// redo stack when it changes the document. Answers the command's
    /// report, or `None`, changing nothing, when it may not amend; on an
    /// error nothing changes.
    pub fn amend(
        &mut self,
        cmd: &Command,
        filter: &ViewFilter,
        step: u64,
    ) -> Result<Option<Report>> {
        // Co-Producer never amends a step: that could be the composer's.
        if self.acting {
            return Ok(None);
        }
        self.before_hand_action();
        if self.txn.is_some()
            || self.group.is_some()
            || self.past.is_empty()
            || self.present.serial != step
        {
            return Ok(None);
        }
        let mut applied = apply_in(
            &self.present.doc,
            Context {
                selection: &self.selection,
                filter,
            },
            cmd,
        )?;
        // Never acting here (see above).
        crate::marks::settle(&self.present.doc, &mut applied.doc, false);
        if applied.doc != self.present.doc {
            self.present.doc = applied.doc;
            self.present.at = Timestamp::now();
            self.future.clear();
        }
        let sel = applied.selection.unwrap_or_else(|| self.selection.clone());
        self.selection = sel.retain_existing(&self.present.doc);
        Ok(Some(applied.report))
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

    /// Gives the project ID `id` to the present and to every undo and redo
    /// version that has none (plan 6a), so undoing past the stamp keeps it.
    /// It changes no undo step and does nothing when the present already
    /// has an ID. Refused for a malformed ID and inside a transaction.
    pub fn stamp_project_id(&mut self, id: &str) -> Result<()> {
        if !crate::document::check_project_id(id) {
            return Err(ModelError::invalid(
                "a project ID is 36 lowercase hex characters",
            ));
        }
        if self.txn.is_some() {
            return Err(ModelError::invalid(
                "the project ID cannot be stamped inside a transaction",
            ));
        }
        if self.present.doc.settings().project_id.is_some() {
            return Ok(());
        }
        let stamp = |v: &mut Version| {
            v.doc = std::mem::take(&mut v.doc).with_project_id(id);
        };
        stamp(&mut self.present);
        self.past.iter_mut().for_each(stamp);
        self.future.iter_mut().for_each(stamp);
        if let Some(g) = self.group.as_mut() {
            g.base_future.iter_mut().for_each(stamp);
        }
        Ok(())
    }

    /// Steps back one version. Inside a transaction, only the transaction's
    /// own steps can be undone. An open request group ends first, whoever
    /// undoes (8a Ruling 3).
    pub fn undo(&mut self) -> Result<()> {
        self.before_hand_action();
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
    /// and inside a group for steps undone before it opened. An open
    /// request group ends first, whoever redoes (8a Ruling 3).
    pub fn redo(&mut self) -> Result<()> {
        self.before_hand_action();
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
        if self.acting {
            self.open_request_group();
        } else {
            self.before_hand_action();
        }
        self.txn = Some(Txn {
            label: label.into(),
            start: self.past.len(),
            base: self.present.clone(),
            base_future: self.future.clone(),
            executed: false,
            acting: self.acting,
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
        if !self.acting {
            self.before_hand_action();
        }
        if self.group.is_some() {
            return Err(ModelError::invalid("a group is already open"));
        }
        if self.txn.is_some() {
            return Err(ModelError::invalid("finish the open edit first"));
        }
        self.group = Some(Group {
            kind: GroupKind::Record,
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
        if !self.in_group() {
            return Err(ModelError::invalid("no open group"));
        }
        let g = self
            .group
            .take()
            .ok_or_else(|| ModelError::invalid("no open group"))?;
        self.close_group(g, label.into());
        Ok(())
    }

    /// Makes the steps of the group `g` (already taken) one step named
    /// `label`; one that made none adds nothing and keeps the redo stack
    /// it found. Whether it made a step.
    fn close_group(&mut self, g: Group, label: String) -> bool {
        let made = self.past.len() > g.start;
        if made {
            self.past.truncate(g.start + 1);
            self.present.label = label;
            self.present.at = Timestamp::now();
        }
        // Versions undone inside the group are intermediate states.
        self.future = if g.executed {
            Vec::new()
        } else {
            g.base_future
        };
        self.trim();
        made
    }

    /// Opens a request group for an acting step, when a request is open
    /// and no group or transaction is (8a Ruling 3: the group opens lazily).
    fn open_request_group(&mut self) {
        if !self.acting || self.request.is_none() || self.group.is_some() || self.txn.is_some() {
            return;
        }
        self.group = Some(Group {
            kind: GroupKind::Request,
            start: self.past.len(),
            base_future: self.future.clone(),
            redo_floor: self.future.len(),
            executed: false,
        });
    }

    /// Ends an open request group as one step; its label, if it made one.
    /// Does nothing while a transaction is open (the step would cut it).
    fn close_request_group(&mut self) -> Option<String> {
        if self.txn.is_some() || !self.request_group_open() {
            return None;
        }
        let g = self.group.take()?;
        let label = self.request.as_ref().map_or_else(
            || "Co-Producer".to_string(),
            |r| r.group_label().to_string(),
        );
        if !self.close_group(g, label.clone()) {
            return None;
        }
        if let Some(r) = self.request.as_mut() {
            r.continued = true;
        }
        Some(label)
    }

    /// A hand action (or any undo or redo) is about to run: an open
    /// request group ends first, so it holds only Co-Producer's steps and
    /// undoing it never removes the composer's edits.
    fn before_hand_action(&mut self) {
        let _ = self.close_request_group();
    }

    /// Opens a Co-Producer request named `name` ([`request_label`]).
    /// Refused while one is open.
    pub fn begin_request(&mut self, name: &str) -> Result<RequestInfo> {
        if let Some(r) = &self.request {
            return Err(ModelError::invalid(format!(
                "Co-Producer request \"{}\" is still open.",
                r.label
            )));
        }
        let (title, label) = request_label(name)?;
        self.request = Some(Request {
            name: name.trim().to_string(),
            title,
            continued_label: format!("{label} (continued)"),
            label,
            continued: false,
        });
        self.request()
            .ok_or_else(|| ModelError::invalid("no open request"))
    }

    /// Why acting cannot start now: "Finish the drag first." (a hand
    /// transaction is open) or "Stop recording first." (a recording group
    /// is open); `None` when it can.
    pub fn acting_refusal(&self) -> Option<&'static str> {
        if self.txn.is_some() {
            Some("Finish the drag first.")
        } else if self.in_group() {
            Some("Stop recording first.")
        } else {
            None
        }
    }

    /// Ends the request, its open group as one step; the label of that
    /// step, if one was made. Refused when no request is open, and while a
    /// transaction is open.
    pub fn end_request(&mut self) -> Result<Option<String>> {
        if self.request.is_none() {
            return Err(ModelError::invalid("No Co-Producer request is open."));
        }
        if self.txn.is_some() && self.request_group_open() {
            return Err(ModelError::invalid("Finish the drag first."));
        }
        let label = self.close_request_group();
        self.request = None;
        Ok(label)
    }

    /// The open request, if any.
    pub fn request(&self) -> Option<RequestInfo> {
        let r = self.request.as_ref()?;
        Some(RequestInfo {
            name: r.name.clone(),
            title: r.title.clone(),
            label: r.label.clone(),
            group_open: self.request_group_open(),
            continued: r.continued,
        })
    }

    /// Whether the steps that follow are Co-Producer's (8a Ruling 3).
    /// Turning it on is `ModelError::Invalid` with
    /// [`History::acting_refusal`]'s message during a hand transaction or
    /// a recording group (the crate has no Busy variant; Task 4 maps this
    /// to `VizError::Busy`). Turning it off never fails: it aborts a
    /// transaction begun while acting, and leaves an open request group
    /// open until a hand action ends it.
    pub fn set_acting(&mut self, on: bool) -> Result<()> {
        if on
            && !self.acting
            && let Some(why) = self.acting_refusal()
        {
            return Err(ModelError::invalid(why));
        }
        // A Co-Producer transaction left open is aborted: a hand edit that
        // joined it would leave with one undo of Co-Producer's step.
        if !on && self.txn.as_ref().is_some_and(|t| t.acting) {
            let _ = self.abort();
        }
        self.acting = on;
        Ok(())
    }

    /// Whether steps now are Co-Producer's ([`History::set_acting`]).
    pub fn acting(&self) -> bool {
        self.acting
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
        self.capture_state_keeping(slot, bytes, &std::collections::BTreeMap::new())
    }

    /// Like [`History::capture_state`], but the slot's overlay becomes
    /// `keep` (its automated parameters' static values; see
    /// [`crate::state::capture_keeping`]).
    pub fn capture_state_keeping(
        &mut self,
        slot: PluginInstanceId,
        bytes: &[u8],
        keep: &std::collections::BTreeMap<crate::mixer::ParamId, f64>,
    ) -> Result<StateBlobId> {
        if self.txn.is_some() {
            return Err(ModelError::invalid(
                "cannot capture plugin state inside a transaction",
            ));
        }
        let (doc, id) = crate::state::capture_keeping(&self.present.doc, slot, bytes, keep)?;
        self.present.doc = doc;
        Ok(id)
    }
}
