//! The Select and Modify panel of one document (Swift
//! `M/SelectModifyModel.swift` and `M/EditorModel+Modify.swift`): what it
//! is set to, the text typed into it, and what Apply, the templates and
//! Modify Again send. The panel window is the GPUI shell's; it polls
//! [`DocumentModel::take_modify_present`] and reports focus and closing.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use vizdaw_ffi::edit::{EditCommand, LaneRef, ViewFilterRow};
use vizdaw_ffi::events::{EventKindRow, MeterEntryRow};
use vizdaw_ffi::modify::{
    AmountRow, ConditionRow, CurveRow, ModifyRow, ModifyTemplateRow, SelectModeRow, SettingsEdit,
    SubstituteRow, ValueTargetRow, WaveRow,
};
use vizdaw_ffi::selection::SelectionRow;

use super::{
    AmountKind, ChangeOpKind, Comparison, ConditionKind, ModifyConfig, ModifyKind, PanelTask,
    condition_fields, condition_numbers, condition_with, field_text, modify_fields, modify_numbers,
    modify_with, note_map_text, offsets_text, parse_field, parse_note_map, parse_offsets, reseeded,
};
use crate::DocumentModel;
use crate::editing_prefs::ScaleKind;
use crate::editor::{MainView, PaneView};

pub use crate::editor::GESTURE_OPEN;

/// Apply with neither half on (`M/SelectModifyModel.swift:51`).
pub const NOTHING_ON: &str = "Turn on Select or Modify first.";
/// A rule to search with and no tracks to search (`:52`).
pub const NO_TRACKS: &str = "Select tracks to search first: drag in a ruler or select a block.";
/// A field's text does not parse (`:53`).
pub const FIX_FIELDS: &str = "Fix the fields marked in red first.";
/// The dragged events of an Exact Mode request did not take (`:54`).
pub const NOT_SELECTED: &str = "The dragged notes could not be selected.";
/// Save as Template with a blank name (`:55`).
pub const NO_NAME: &str = "Name the template first.";
/// Why the nudges and Modify Again wait in the Mix view
/// (`M/DocumentModel.swift:593-594`); ⌃1 as the menus show it.
pub const HIDDEN_SELECTION: &str = if cfg!(target_os = "macos") {
    "The Mix view shows no editor: press \u{2303}1, or show the Editor in the bottom pane, to edit the selection."
} else {
    "The Mix view shows no editor: press Ctrl-1, or show the Editor in the bottom pane, to edit the selection."
};
/// Modify Again with nothing to repeat (`M/ControlCommands.swift:657`).
pub const NO_MODIFICATION: &str = "There is no modification to repeat.";
/// Modify > Templates with none saved (`M/ControlCommands.swift:664`).
pub const NO_TEMPLATES: &str = "There are no modify templates.";

/// What Apply acts on, as the panel says it (`M/SelectModifyModel.swift:61-62`).
pub const DRAGGED_EVENTS_TARGET: &str = "Apply acts on the dragged events.";
pub const SELECTION_TARGET: &str = "Apply acts on the current selection.";

/// The panel's fields own the keyboard under these names, one per field
/// so moving between fields never clears it (`M/SelectModifyModel.swift:72-73`).
pub const TEXT_OWNER_PREFIX: &str = "selectModify.";

/// Field `key`'s text owner.
pub fn text_owner(key: &str) -> String {
    format!("{TEXT_OWNER_PREFIX}{key}")
}

/// The part of a diagonal drag the panel left out (`M/ExactMode.swift:13-22`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DroppedAxis {
    Vertical,
    Horizontal,
}

impl DroppedAxis {
    pub fn message(self) -> &'static str {
        match self {
            DroppedAxis::Vertical => "The vertical part of the drag was left out.",
            DroppedAxis::Horizontal => "The horizontal part of the drag was left out.",
        }
    }
}

/// The panel opened by Exact Mode, with what it acts on
/// (`M/ExactMode.swift:25-46`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifyRequest {
    pub task: PanelTask,
    pub config: ModifyConfig,
    /// The dragged events (`None`: the selection's events).
    pub events: Option<Vec<u64>>,
    /// The `(seq, lane)` tracks dragged on.
    pub lanes: Vec<(u64, u64)>,
    pub range: Option<(i64, i64)>,
    /// Set when a diagonal drag was reduced to its stronger axis.
    pub dropped: Option<DroppedAxis>,
}

/// What Modify Again repeats and the saved templates, as one read.
#[derive(Debug, Clone, Default)]
struct Reads {
    last: Option<ModifyRow>,
    templates: Vec<ModifyTemplateRow>,
}

/// The reads of this snapshot version, and the last good ones (the answer
/// while the core is busy).
#[derive(Debug, Default)]
struct ReadCache {
    current: OnceLock<Reads>,
    previous: Reads,
}

/// One document's panel (Swift `SelectModifyModel`).
#[derive(Debug)]
pub struct SelectModifyModel {
    pub task: PanelTask,
    pub config: ModifyConfig,
    pub template_name: String,
    /// The opening list's hidden kinds, else none.
    filter: Option<ViewFilterRow>,
    /// Field text as typed, by key (`c<i>.<j>`, `m<j>`, `map`, `offsets`).
    drafts: BTreeMap<String, String>,
    /// Drafts that do not parse (drawn red; Apply waits for them).
    invalid: BTreeSet<String>,
    /// The dragged events an Exact Mode panel acts on.
    pending: Option<ModifyRequest>,
    /// The selection the request was opened over.
    baseline: Option<SelectionRow>,
    /// Why the panel shows what it shows (a dropped drag axis).
    notice: Option<String>,
    is_open: bool,
    is_key: bool,
    wants_present: bool,
    /// Applies so far; mixed into each Random seed.
    seeds: u64,
    reads: ReadCache,
}

impl Default for SelectModifyModel {
    fn default() -> Self {
        SelectModifyModel {
            task: PanelTask::Change,
            config: ModifyConfig::preset(PanelTask::Change, None),
            template_name: String::new(),
            filter: None,
            drafts: BTreeMap::new(),
            invalid: BTreeSet::new(),
            pending: None,
            baseline: None,
            notice: None,
            is_open: false,
            is_key: false,
            wants_present: false,
            seeds: 0,
            reads: ReadCache::default(),
        }
    }
}

impl SelectModifyModel {
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// The panel window is key: the document's transport keys are off.
    pub fn is_key(&self) -> bool {
        self.is_key
    }

    /// The filter Apply sends (the opening list's).
    pub fn filter(&self) -> Option<&ViewFilterRow> {
        self.filter.as_ref()
    }

    /// The keys whose text does not parse, in order.
    pub fn invalid_keys(&self) -> Vec<&str> {
        self.invalid.iter().map(String::as_str).collect()
    }

    /// Some field holds typed text.
    pub fn has_drafts(&self) -> bool {
        !self.drafts.is_empty()
    }

    /// A new snapshot version: the reads are taken again.
    pub(crate) fn snapshot_moved(&mut self) {
        if let Some(r) = self.reads.current.take() {
            self.reads.previous = r;
        }
    }

    /// A new random seed per Apply (Plan 4c Ruling 3): the time mixed with
    /// a counter, so two Applies always differ.
    fn next_seed(&mut self) -> u64 {
        self.seeds += 1;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        // splitmix64 of the counter, xored with the clock: distinct
        // counters give distinct values whatever the clock says.
        let mut z = self.seeds.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        z ^ (now & 0xFFFF_0000_0000_0000)
    }

    fn drop_request(&mut self) {
        self.pending = None;
        self.notice = None;
        self.baseline = None;
    }

    fn clear_fields(&mut self) {
        self.drafts.clear();
        self.invalid.clear();
    }
}

/// A field key: condition `i`'s field `j`, modify field `j`, the note map
/// or the groove offsets.
enum Key {
    Condition(usize, usize),
    Modify(usize),
    Map,
    Offsets,
}

fn key_of(key: &str) -> Option<Key> {
    match key {
        "map" => return Some(Key::Map),
        "offsets" => return Some(Key::Offsets),
        _ => {}
    }
    if let Some(rest) = key.strip_prefix('c') {
        let (i, j) = rest.split_once('.')?;
        return Some(Key::Condition(i.parse().ok()?, j.parse().ok()?));
    }
    key.strip_prefix('m')?.parse().ok().map(Key::Modify)
}

impl DocumentModel {
    pub fn select_modify(&self) -> &SelectModifyModel {
        &self.select_modify
    }

    pub fn select_modify_mut(&mut self) -> &mut SelectModifyModel {
        &mut self.select_modify
    }

    // MARK: Opening and closing

    /// A Modify menu command: the panel set to `task` over the selection's
    /// range, Apply through `filter` (a list's hidden kinds).
    pub fn open_modify(&mut self, task: PanelTask, filter: Option<ViewFilterRow>) {
        let range = self.selection().time_range.map(|r| (r.start, r.end));
        let s = &mut self.select_modify;
        s.task = task;
        s.config = ModifyConfig::preset(task, range);
        s.filter = filter;
        s.drop_request();
        s.clear_fields();
        s.is_open = true;
        s.wants_present = true;
    }

    /// The panel asked to be shown since the last call (the GPUI shell
    /// polls it after each command or event).
    pub fn take_modify_present(&mut self) -> bool {
        std::mem::take(&mut self.select_modify.wants_present)
    }

    /// Holds Exact Mode request `r` as Swift's `open(_ request:)` does,
    /// without showing the panel: the task, the config, `pending` when it
    /// has events, the notice, and the selection as its baseline.
    #[doc(hidden)]
    pub fn set_modify_request_for_test(&mut self, r: ModifyRequest) {
        self.hold_modify_request(r);
    }

    pub(crate) fn hold_modify_request(&mut self, r: ModifyRequest) {
        let baseline = self.selection().clone();
        let s = &mut self.select_modify;
        s.task = r.task;
        s.config = r.config.clone();
        s.filter = None;
        s.notice = r.dropped.map(|d| d.message().to_string());
        s.pending = r.events.is_some().then_some(r);
        s.baseline = Some(baseline);
        s.clear_fields();
    }

    /// The panel window became key (`on`) or resigned it: resigning gives
    /// every panel field's keyboard back.
    pub fn modify_panel_focused(&mut self, on: bool) {
        self.select_modify.is_key = on;
        if !on {
            self.end_text_editing_prefix(TEXT_OWNER_PREFIX);
        }
    }

    /// The panel window closed (also with its document).
    pub fn modify_panel_closed(&mut self) {
        let s = &mut self.select_modify;
        s.is_open = false;
        s.is_key = false;
        s.drop_request();
        self.end_text_editing_prefix(TEXT_OWNER_PREFIX);
    }

    /// Panel field `key` has the keyboard (`on`) or gave it back.
    pub fn set_modify_text_editing(&mut self, key: &str, on: bool) {
        let owner = text_owner(key);
        if on {
            self.begin_text_editing(&owner);
        } else {
            self.end_text_editing(&owner);
        }
    }

    // MARK: The request

    /// The request no longer fits: the selection moved on, or a dragged
    /// event is gone. Other snapshots (a plug-in load) leave it alone, and
    /// a track the core cannot read now is no evidence.
    fn modify_request_is_stale(&self) -> bool {
        let s = &self.select_modify;
        let Some(b) = &s.baseline else { return false };
        if self.selection() != b {
            return true;
        }
        let Some(p) = &s.pending else { return false };
        let Some(ids) = &p.events else { return false };
        if p.lanes.is_empty() {
            return false;
        }
        let mut present = BTreeSet::new();
        for &(seq, lane) in &p.lanes {
            let read = self.lane_events_read(seq, lane);
            if !read.fresh {
                return false;
            }
            present.extend(read.rows.iter().map(|e| e.id));
        }
        !ids.iter().all(|id| present.contains(id))
    }

    /// Apply selects the dragged events of an Exact Mode request first.
    pub fn modify_acts_on_dragged_events(&self) -> bool {
        self.select_modify
            .pending
            .as_ref()
            .is_some_and(|p| p.events.is_some())
            && !self.modify_request_is_stale()
    }

    /// What Apply acts on, as the panel says it.
    pub fn apply_target_caption(&self) -> &'static str {
        if self.modify_acts_on_dragged_events() {
            DRAGGED_EVENTS_TARGET
        } else {
            SELECTION_TARGET
        }
    }

    /// Why the panel shows what it shows, while its request holds.
    pub fn modify_notice(&self) -> Option<&str> {
        if self.modify_request_is_stale() {
            return None;
        }
        self.select_modify.notice.as_deref()
    }

    // MARK: Running

    /// Runs the config on the selection (`M/SelectModifyModel.swift:164-212`):
    /// one undo step when it modifies, a selection change otherwise.
    /// Whether it applied; a refusal is the status.
    pub fn apply_modify(&mut self) -> bool {
        if !self.select_modify.invalid.is_empty() {
            self.status = Some(FIX_FIELDS.into());
            return false;
        }
        let Some(cmd) = self.select_modify.config.command() else {
            self.status = Some(NOTHING_ON.into());
            return false;
        };
        if self.editor.gesture_open() {
            self.status = Some(GESTURE_OPEN.into());
            return false;
        }
        if self.modify_request_is_stale() {
            self.select_modify.drop_request();
        }
        // An Exact Mode panel acts on the dragged events: select them first.
        let before = self.selection().clone();
        let dragged = self
            .select_modify
            .pending
            .as_ref()
            .and_then(|p| Some((p.events.clone()?, p.lanes.clone())));
        let pending = self.select_modify.pending.is_some();
        if let Some((events, lanes)) = dragged {
            let status = self.status.clone();
            self.set_selection(SelectionRow {
                time_range: before.time_range,
                lanes: lanes
                    .into_iter()
                    .map(|(seq, lane)| LaneRef { seq, lane })
                    .collect(),
                events: events.clone(),
            });
            // A refused selection was rolled back and reported: do not run
            // on the old one.
            let held: BTreeSet<u64> = self.selection().events.iter().copied().collect();
            if held != events.into_iter().collect() {
                if self.status == status {
                    self.status = Some(NOT_SELECTED.into());
                }
                return false;
            }
        }
        let c = &self.select_modify.config;
        if c.rule().is_some()
            && c.select.mode != SelectModeRow::Refine
            && self.selection().lanes.is_empty()
        {
            self.status = Some(NO_TRACKS.into());
            if pending {
                self.set_selection(before);
            }
            return false;
        }
        let seed = self.select_modify.next_seed();
        let filter = self.select_modify.filter.clone();
        let ok = self.edit_filtered(reseeded(cmd, seed), filter);
        if ok {
            self.select_modify.drop_request();
        } else if pending {
            self.set_selection(before);
        }
        ok
    }

    /// Saves the config under the template name (trimmed). Whether it
    /// saved; a blank name is the status [`NO_NAME`].
    pub fn save_modify_template(&mut self) -> bool {
        let name = self.select_modify.template_name.trim().to_string();
        if name.is_empty() {
            self.status = Some(NO_NAME.into());
            return false;
        }
        let template = self.select_modify.config.template(&name);
        self.edit(EditCommand::Settings {
            edit: SettingsEdit::SaveTemplate { template },
        })
    }

    /// Deletes template `name` (trimmed); a blank name does nothing.
    pub fn delete_modify_template(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.edit(EditCommand::Settings {
            edit: SettingsEdit::DeleteTemplate { name: name.into() },
        });
    }

    /// The panel shows template `t`.
    pub fn load_modify_template(&mut self, t: &ModifyTemplateRow) {
        let s = &mut self.select_modify;
        s.config = ModifyConfig::from_template(t);
        s.template_name = t.name.clone();
        s.drop_request();
        s.clear_fields();
    }

    /// Modify > Templates > `name`: loads it and applies it at once. The
    /// error is the status line's text (or why there is no such template).
    pub fn run_modify_template(&mut self, name: &str) -> Result<(), String> {
        let templates = self.modify_templates();
        if templates.is_empty() {
            return Err(NO_TEMPLATES.into());
        }
        let Some(t) = templates.iter().find(|t| t.name == name).cloned() else {
            return Err(format!("modify.template: no template named \"{name}\""));
        };
        self.load_modify_template(&t);
        if self.apply_modify() {
            Ok(())
        } else {
            Err(self
                .status
                .clone()
                .unwrap_or_else(|| "The template was refused.".into()))
        }
    }

    /// Modify Again's filter (`M/SelectModifyModel.swift:260-264`): the
    /// panel's while it is open and key, else the list's while a list is
    /// the command surface, else the open panel's.
    pub fn modify_again_filter(&self) -> Option<ViewFilterRow> {
        let s = &self.select_modify;
        if s.is_open && s.is_key {
            return s.filter.clone();
        }
        if self.list_open() {
            return self.list_filter().view_filter();
        }
        if s.is_open { s.filter.clone() } else { None }
    }

    /// Repeats the last modification on the selection in place. The error
    /// is the status line's text.
    pub fn modify_again(&mut self) -> Result<(), String> {
        let filter = self.modify_again_filter();
        if self.edit_filtered(EditCommand::ModifyAgain, filter) {
            Ok(())
        } else {
            Err(self
                .status
                .clone()
                .unwrap_or_else(|| "Modify Again was refused.".into()))
        }
    }

    // MARK: Reads

    /// Read once per snapshot version; a refused read (Busy) keeps the
    /// last values, silently, and is tried again next time.
    fn modify_reads(&self) -> &Reads {
        let cache = &self.select_modify.reads;
        if let Some(r) = cache.current.get() {
            return r;
        }
        let read = self
            .session()
            .last_modify()
            .and_then(|last| Ok((last, self.session().modify_templates()?)));
        match read {
            Ok((last, templates)) => cache.current.get_or_init(|| Reads { last, templates }),
            Err(_) => &cache.previous,
        }
    }

    /// The saved templates, by name.
    pub fn modify_templates(&self) -> &[ModifyTemplateRow] {
        &self.modify_reads().templates
    }

    /// The modification Modify Again repeats, if any.
    pub fn last_modify(&self) -> Option<&ModifyRow> {
        self.modify_reads().last.as_ref()
    }

    /// Modify Again needs a last modification and no open drag.
    pub fn can_modify_again(&self) -> bool {
        self.last_modify().is_some() && self.gesture().is_none()
    }

    /// The Mix view shows no editor, so the selection is out of sight
    /// (`M/DocumentModel.swift:587-591`).
    pub fn selection_hidden(&self) -> bool {
        let views = &self.layout().views;
        views.main == MainView::Mix
            && !(views.mix.bottom.shown
                && matches!(
                    views.mix.bottom.view,
                    PaneView::Editor | PaneView::AudioEditor
                ))
    }

    pub fn exact_mode(&self) -> bool {
        self.editor.exact_mode()
    }

    pub fn set_exact_mode(&mut self, on: bool) {
        self.editor.set_exact_mode(on);
    }

    // MARK: Fields

    /// The meter positions are typed in: the shown sequence's.
    pub fn modify_meter(&self) -> Vec<MeterEntryRow> {
        self.selected_seq()
            .and_then(|seq| self.sequence_info(seq))
            .map(|i| i.meter.clone())
            .unwrap_or_default()
    }

    /// Field `key`'s text: as typed, else its value.
    pub fn modify_field_text(&self, key: &str) -> String {
        if let Some(d) = self.select_modify.drafts.get(key) {
            return d.clone();
        }
        let c = &self.select_modify.config;
        match key_of(key) {
            Some(Key::Condition(i, j)) => {
                let Some(cond) = c.select.conditions.get(i) else {
                    return String::new();
                };
                let f = condition_fields(&cond.test);
                let n = condition_numbers(&cond.test);
                match (f.get(j), n.get(j)) {
                    (Some(f), Some(&n)) => field_text(n, f.kind, &self.modify_meter()),
                    _ => String::new(),
                }
            }
            Some(Key::Modify(j)) => {
                let f = modify_fields(&c.modify.op);
                let n = modify_numbers(&c.modify.op);
                match (f.get(j), n.get(j)) {
                    (Some(f), Some(&n)) => field_text(n, f.kind, &self.modify_meter()),
                    _ => String::new(),
                }
            }
            Some(Key::Map) => match &c.modify.op {
                ModifyRow::NoteMap { map } => note_map_text(map),
                _ => String::new(),
            },
            Some(Key::Offsets) => match &c.modify.op {
                ModifyRow::Quantize { offsets, .. } => offsets_text(offsets),
                _ => String::new(),
            },
            None => String::new(),
        }
    }

    /// The text of field `key` does not parse.
    pub fn modify_field_invalid(&self, key: &str) -> bool {
        self.select_modify.invalid.contains(key)
    }

    /// Text typed into field `key`: kept as typed; when it parses, the
    /// config takes it, else the field is marked invalid.
    pub fn set_modify_field(&mut self, key: &str, text: &str) {
        let Some(k) = key_of(key) else { return };
        let meter = self.modify_meter();
        let s = &mut self.select_modify;
        s.drafts.insert(key.to_string(), text.to_string());
        let parsed = match k {
            Key::Condition(i, j) => {
                let Some(cond) = s.config.select.conditions.get(i) else {
                    return;
                };
                let f = condition_fields(&cond.test);
                f.get(j)
                    .and_then(|f| parse_field(text, f.kind, &meter))
                    .and_then(|v| {
                        let mut n = condition_numbers(&cond.test);
                        *n.get_mut(j)? = v;
                        condition_with(&cond.test, &n)
                    })
                    .map(|t| s.config.select.conditions[i].test = t)
            }
            Key::Modify(j) => {
                let op = &s.config.modify.op;
                modify_fields(op)
                    .get(j)
                    .and_then(|f| parse_field(text, f.kind, &meter))
                    .and_then(|v| {
                        let mut n = modify_numbers(op);
                        *n.get_mut(j)? = v;
                        modify_with(op, &n)
                    })
                    .map(|op| s.config.modify.op = op)
            }
            Key::Map => {
                parse_note_map(text).map(|map| s.config.modify.op = ModifyRow::NoteMap { map })
            }
            Key::Offsets => match (&s.config.modify.op, parse_offsets(text)) {
                (ModifyRow::Quantize { grid, strength, .. }, Some(offsets)) => {
                    s.config.modify.op = ModifyRow::Quantize {
                        grid: *grid,
                        strength: *strength,
                        offsets,
                    };
                    Some(())
                }
                _ => None,
            },
        };
        if parsed.is_some() {
            s.invalid.remove(key);
        } else {
            s.invalid.insert(key.to_string());
        }
    }

    // MARK: The select half (`M/SelectModifyModel.swift:271-323`)

    /// Adds a condition of kind `k` (Position over the selection's range)
    /// and turns Select on.
    pub fn add_modify_condition(&mut self, k: ConditionKind) {
        let range = self.selection().time_range.map(|r| (r.start, r.end));
        let s = &mut self.select_modify;
        s.config.select.conditions.push(ConditionRow {
            test: k.default_test(range),
            negate: false,
        });
        s.config.select.enabled = true;
    }

    pub fn remove_modify_condition(&mut self, i: usize) {
        let s = &mut self.select_modify;
        if i >= s.config.select.conditions.len() {
            return;
        }
        s.config.select.conditions.remove(i);
        s.clear_fields();
    }

    pub fn set_modify_comparison(&mut self, i: usize, c: Comparison) {
        let s = &mut self.select_modify;
        let Some(cond) = s.config.select.conditions.get_mut(i) else {
            return;
        };
        *cond = c.applied(cond);
        let prefix = format!("c{i}.");
        s.drafts.retain(|k, _| !k.starts_with(&prefix));
    }

    pub fn set_modify_kinds(&mut self, i: usize, kinds: Vec<EventKindRow>) {
        if let Some(cond) = self.select_modify.config.select.conditions.get_mut(i) {
            cond.test = vizdaw_ffi::modify::ConditionTest::Kinds { kinds };
        }
    }

    pub fn set_modify_condition_dest(&mut self, i: usize, dest: Option<u64>) {
        if let Some(cond) = self.select_modify.config.select.conditions.get_mut(i) {
            cond.test = vizdaw_ffi::modify::ConditionTest::Dest { dest };
        }
    }

    // MARK: The modify half (`M/SelectModifyModel.swift:327-433`)

    /// A new operation of kind `k`, with Modify on.
    pub fn set_modify_kind(&mut self, k: ModifyKind) {
        let s = &mut self.select_modify;
        s.config.modify.op = k.default_op();
        s.config.modify.enabled = true;
        s.clear_fields();
    }

    /// Change Values: the value it changes.
    pub fn set_modify_target(&mut self, t: ValueTargetRow) {
        let s = &mut self.select_modify;
        if let ModifyRow::Change { op, amount, .. } = &s.config.modify.op {
            s.config.modify.op = ModifyRow::Change {
                target: t,
                op: ChangeOpKind::of(op).row(t),
                amount: *amount,
            };
            s.drafts.clear();
        }
    }

    /// Change Values: the operation; Scale starts at 100%.
    pub fn set_modify_op(&mut self, k: ChangeOpKind) {
        let s = &mut self.select_modify;
        if let ModifyRow::Change { target, amount, .. } = &s.config.modify.op {
            let amount = if k == ChangeOpKind::Scale {
                AmountRow::Constant { value: 100 }
            } else {
                *amount
            };
            s.config.modify.op = ModifyRow::Change {
                target: *target,
                op: k.row(*target),
                amount,
            };
            s.drafts.clear();
        }
    }

    /// Change Values: the amount's kind (a Random one gets a new seed).
    pub fn set_modify_amount(&mut self, k: AmountKind) {
        let seed = self.select_modify.next_seed();
        let s = &mut self.select_modify;
        if let ModifyRow::Change { target, op, .. } = &s.config.modify.op {
            s.config.modify.op = ModifyRow::Change {
                target: *target,
                op: *op,
                amount: k.row(seed),
            };
            s.drafts.clear();
        }
    }

    pub fn set_modify_curve(&mut self, c: CurveRow) {
        if let ModifyRow::Change {
            amount: AmountRow::Line { curve, .. },
            ..
        } = &mut self.select_modify.config.modify.op
        {
            *curve = c;
        }
    }

    pub fn set_modify_wave(&mut self, w: WaveRow) {
        if let ModifyRow::Change {
            amount: AmountRow::Oscillator { wave, .. },
            ..
        } = &mut self.select_modify.config.modify.op
        {
            *wave = w;
        }
    }

    /// Transpose in Scale and Snap to Scale: the root and scale.
    pub fn set_modify_scale(&mut self, root: u8, scale: ScaleKind) {
        let op = &mut self.select_modify.config.modify.op;
        match op {
            ModifyRow::TransposeDiatonic { steps, .. } => {
                *op = ModifyRow::TransposeDiatonic {
                    steps: *steps,
                    root,
                    mask: scale.mask(),
                }
            }
            ModifyRow::TransposeToScale { .. } => {
                *op = ModifyRow::TransposeToScale {
                    root,
                    mask: scale.mask(),
                }
            }
            _ => {}
        }
    }

    pub fn set_modify_dest(&mut self, dest: Option<u64>) {
        self.select_modify.config.modify.op = ModifyRow::SetDest { dest };
    }

    pub fn set_modify_substitute(&mut self, to: SubstituteRow) {
        let s = &mut self.select_modify;
        s.config.modify.op = ModifyRow::Substitute { to };
        s.drafts.clear();
    }
}
