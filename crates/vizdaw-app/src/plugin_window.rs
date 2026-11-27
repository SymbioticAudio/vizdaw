//! One plug-in window apart from drawing (Swift `PluginWindowModel`, spec
//! §9): titled "‹strip› — ‹plug-in›", the plug-in's own editor or, without
//! one, its parameters, and a header with Bypass and Learn. Closing it
//! never removes the plug-in.
//!
//! The window registry, the native editor host and the drawing are the
//! GPUI crate's; this model holds what they show and every session call.
//! A parameter slider's drag runs through the document's mixer gestures
//! ([`MixerModel::touch`], `move_to`, `release`, `click`), so it is one undo
//! step "Change Plugin Parameter" named by the core, a refused touch is not
//! kept, and a release that meets Busy is retried by the document's tick.
//!
//! Sizes are points. An `editor_resize` from the plug-in tick is followed
//! only while this window's editor is open; one for a slot whose editor is
//! closed is dropped (host H0 carry 3, the app half).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Instant;

use vizdaw_ffi::VizError;
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::snapshot::{ParamRow, SlotRow, SlotStatus};
use vizdaw_ffi::{EditorResize, EditorSize};

use crate::alerts::{AlertItem, error_message};
use crate::document::DocumentModel;
use crate::mixer::MixerModel;

pub use crate::mixer::BYPASS_FAILED;

/// The title without a slot row.
pub const PLUGIN: &str = "Plugin";
/// The editor's text while the plug-in is not running yet.
pub const LOADING: &str = "Loading\u{2026}";
/// The parameter list's text when it is empty and nothing went wrong.
pub const NO_PARAMETERS: &str = "No parameters.";
/// The alert of a refused parameter gesture.
pub const PARAMETER_FAILED: &str = "Parameter Failed";
/// The undo step of a parameter drag (the core names it).
pub const CHANGE_PLUGIN_PARAMETER: &str = "Change Plugin Parameter";
/// The header's bypass toggle.
pub const BYPASS: &str = "Bypass";
/// The header's MIDI Learn button: learns the parameter last touched,
/// once there is one (`S/PluginWindows.swift:267-270`).
pub const LEARN: &str = "Learn";
/// Learn's help before a parameter was touched.
pub const LEARN_HELP: &str = "Touch a parameter, then learn a MIDI control for it";
/// The accessibility value's suffix of a parameter the user cannot set.
pub const READ_ONLY: &str = ", read-only";

/// The header's height in points, its divider included (Swift
/// `headerHeight`).
pub const HEADER_HEIGHT: f64 = 37.0;
/// The parameter list's width, and its width when empty.
pub const PARAMS_WIDTH: f64 = 400.0;
pub const EMPTY_PARAMS_WIDTH: f64 = 360.0;
/// A parameter row's height, the list's padding and its tallest.
pub const PARAM_ROW_HEIGHT: f64 = 30.0;
pub const PARAMS_MAX_HEIGHT: f64 = 420.0;
/// The empty list's height.
pub const EMPTY_PARAMS_HEIGHT: f64 = 120.0;
/// The least content size of a window showing a resizable editor.
pub const MIN_CONTENT: (f64, f64) = (120.0, HEADER_HEIGHT + 60.0);
/// The window's size before its content is known.
pub const INITIAL_SIZE: (f64, f64) = (420.0, 360.0);

/// The plug-in tick's news a document keeps for its plug-in windows: the
/// slots whose parameters changed (for slots with a window) and the
/// latest resize each open editor asked for. Bounded by the windows open.
#[derive(Debug, Default)]
pub(crate) struct PluginNews {
    /// Slots with a window.
    windows: BTreeSet<u64>,
    /// Slots whose window shows an open editor.
    editors: BTreeSet<u64>,
    params_changed: BTreeSet<u64>,
    resizes: HashMap<u64, EditorResize>,
}

impl PluginNews {
    /// Keeps what an idle brought for a window that shows it; true when
    /// anything was kept.
    pub(crate) fn take(&mut self, params: Vec<u64>, resizes: Vec<EditorResize>) -> bool {
        let mut kept = false;
        for slot in params {
            if self.windows.contains(&slot) {
                self.params_changed.insert(slot);
                kept = true;
            }
        }
        for r in resizes {
            if self.editors.contains(&r.slot) {
                self.resizes.insert(r.slot, r);
                kept = true;
            }
        }
        kept
    }

    fn editor_closed(&mut self, slot: u64) {
        self.editors.remove(&slot);
        self.resizes.remove(&slot);
    }

    fn window_closed(&mut self, slot: u64) {
        self.editor_closed(slot);
        self.windows.remove(&slot);
        self.params_changed.remove(&slot);
    }
}

impl DocumentModel {
    /// The slots with a window whose parameters changed outside a gesture
    /// since that window last read them.
    pub fn params_changed(&self) -> &BTreeSet<u64> {
        &self.plugin_news.params_changed
    }
}

/// One plug-in window's state.
#[derive(Debug)]
pub struct PluginWindowModel {
    slot: u64,
    row: Option<SlotRow>,
    strip_name: String,
    params: Vec<ParamRow>,
    /// The open editor's size; `None` while the window shows parameters.
    editor_size: Option<EditorSize>,
    /// Why the editor is not shown (none, not loaded, a failure).
    editor_problem: Option<String>,
    /// The parameter a gesture here, or the plug-in's own editor, last
    /// touched: the one Learn learns.
    last_touched: Option<(u64, u64)>,
    /// The editor open met Busy and is owed.
    open_owed: bool,
    /// The editor could not detach yet: its parent view stays alive.
    close_pending: bool,
    /// The (document, slots) versions of the last good read.
    seen: Option<(u64, u64)>,
    /// Parameters whose slider's editing span is open (mouse-down to
    /// mouse-up), touched or not: a value outside one is a click.
    editing: HashSet<u64>,
    /// Parameters whose drag sent its touch (taken or refused), in order.
    dragging: Vec<u64>,
    /// The plain value shown for each parameter mid-drag.
    live: HashMap<u64, f64>,
}

impl PluginWindowModel {
    /// The window of slot `slot`, read at once. It counts as open for the
    /// document's plug-in news until [`PluginWindowModel::closed`].
    pub fn new(doc: &mut DocumentModel, slot: u64) -> Self {
        doc.plugin_news.windows.insert(slot);
        let mut w = PluginWindowModel {
            slot,
            row: None,
            strip_name: String::new(),
            params: vec![],
            editor_size: None,
            editor_problem: None,
            last_touched: None,
            open_owed: false,
            close_pending: false,
            seen: None,
            editing: HashSet::new(),
            dragging: vec![],
            live: HashMap::new(),
        };
        w.refresh(doc);
        w
    }

    // Derived.

    pub fn slot(&self) -> u64 {
        self.slot
    }

    pub fn row(&self) -> Option<&SlotRow> {
        self.row.as_ref()
    }

    pub fn strip_name(&self) -> &str {
        &self.strip_name
    }

    /// Every parameter, hidden ones included, as last read (a dragged one
    /// at its dragged value).
    pub fn params(&self) -> &[ParamRow] {
        &self.params
    }

    pub fn editor_size(&self) -> Option<EditorSize> {
        self.editor_size
    }

    pub fn editor_problem(&self) -> Option<&str> {
        self.editor_problem.as_deref()
    }

    /// (slot, parameter) of the last touched parameter.
    pub fn last_touched(&self) -> Option<(u64, u64)> {
        self.last_touched
    }

    /// Learn is enabled: a parameter was touched here.
    pub fn can_learn(&self) -> bool {
        self.last_touched.is_some()
    }

    /// Learns a MIDI control for the parameter last touched
    /// (`M/PluginWindowModel.swift:255-258`); nothing before any.
    pub fn learn_last_touched(&self, doc: &mut DocumentModel) {
        if let Some((slot, param)) = self.last_touched {
            doc.learn_target(AutomationTargetRow::PluginParam { slot, param });
        }
    }

    pub fn open_owed(&self) -> bool {
        self.open_owed
    }

    pub fn close_pending(&self) -> bool {
        self.close_pending
    }

    /// "‹strip› — ‹plug-in›", the plug-in alone for a strip without a
    /// name, "Plugin" without a row.
    pub fn title(&self) -> String {
        let plugin = self.row.as_ref().map_or(PLUGIN, |r| r.plugin.name.as_str());
        if self.strip_name.is_empty() {
            plugin.to_owned()
        } else {
            format!("{} \u{2014} {plugin}", self.strip_name)
        }
    }

    /// The slot left the document (removed, or an undo of its add): the
    /// window closes.
    pub fn is_gone(&self) -> bool {
        self.row.is_none()
    }

    pub fn is_live(&self) -> bool {
        self.row
            .as_ref()
            .is_some_and(|r| r.status == SlotStatus::Live)
    }

    /// The placeholder reason, if the plug-in is not running.
    pub fn placeholder_reason(&self) -> Option<&str> {
        match self.row.as_ref().map(|r| &r.status) {
            Some(SlotStatus::Placeholder { reason }) => Some(reason),
            _ => None,
        }
    }

    /// The parameters a user sees (not hidden).
    pub fn visible_params(&self) -> Vec<&ParamRow> {
        self.params.iter().filter(|p| !p.hidden).collect()
    }

    /// Whether the user can set `p`: a read-only parameter is shown but not
    /// editable (one that is not automatable is still editable by hand).
    pub fn is_editable(p: &ParamRow) -> bool {
        !p.read_only
    }

    /// "0.50", or a whole number for a stepped parameter.
    pub fn value_text(p: &ParamRow) -> String {
        if p.stepped {
            format!("{}", p.value.round() as i64)
        } else {
            format!("{:.2}", p.value)
        }
    }

    /// The slider's accessibility value: the value text, and ", read-only"
    /// when the user cannot set it.
    pub fn accessibility_value(p: &ParamRow) -> String {
        let text = Self::value_text(p);
        if Self::is_editable(p) {
            text
        } else {
            text + READ_ONLY
        }
    }

    /// Ruling 8: `(v − min) / (max − min)` clamped to 0…1; 0 for a range
    /// with `max <= min` or a value that is not a number, never NaN.
    pub fn normalised(v: f64, p: &ParamRow) -> f64 {
        if p.max.partial_cmp(&p.min) != Some(std::cmp::Ordering::Greater) || v.is_nan() {
            return 0.0;
        }
        let n = (v - p.min) / (p.max - p.min);
        if n.is_nan() { 0.0 } else { n.clamp(0.0, 1.0) }
    }

    /// `v` as the slider shows it: inside the range, never NaN.
    pub fn clamped(v: f64, p: &ParamRow) -> f64 {
        if p.max.partial_cmp(&p.min) != Some(std::cmp::Ordering::Greater) || v.is_nan() {
            return p.min;
        }
        v.clamp(p.min, p.max)
    }

    /// The empty parameter list's text: why there is no editor, else "No
    /// parameters.".
    pub fn empty_text(&self) -> &str {
        self.editor_problem.as_deref().unwrap_or(NO_PARAMETERS)
    }

    /// Learn's help: what touching a parameter does, or the parameter it
    /// would learn.
    pub fn learn_help(&self) -> String {
        let Some((_, id)) = self.last_touched else {
            return LEARN_HELP.to_owned();
        };
        let name = self
            .params
            .iter()
            .find(|p| p.id == id)
            .map_or_else(|| format!("parameter {id}"), |p| p.name.clone());
        format!("MIDI Learn: {name}")
    }

    /// The window's content size in points: the editor under the header,
    /// or the parameter list (400 wide, 30 a row plus 30, at most 420; 360
    /// × 120 when empty) under it.
    pub fn content_size(&self) -> (f64, f64) {
        if let Some(s) = self.editor_size {
            return (f64::from(s.width), f64::from(s.height) + HEADER_HEIGHT);
        }
        let n = self.params.iter().filter(|p| !p.hidden).count();
        if n == 0 {
            return (EMPTY_PARAMS_WIDTH, EMPTY_PARAMS_HEIGHT + HEADER_HEIGHT);
        }
        let h = (n as f64 * PARAM_ROW_HEIGHT + PARAM_ROW_HEIGHT).min(PARAMS_MAX_HEIGHT);
        (PARAMS_WIDTH, h + HEADER_HEIGHT)
    }

    // Updates.

    /// The document or its slots moved since the last good read, the last
    /// read failed, or the plug-in tick said this slot's parameters
    /// changed.
    pub fn needs_refresh(&self, doc: &DocumentModel) -> bool {
        self.seen != Some((doc.snapshot().version, doc.slots_version()))
            || doc.plugin_news.params_changed.contains(&self.slot)
    }

    /// Reads the slot, its strip and its parameters again. A failed read
    /// (`Busy` during another editor's open) keeps what was known and
    /// returns false; only a good read without the slot makes the window
    /// gone. A drag whose parameter or slot a good read no longer has is
    /// released once, quietly.
    pub fn refresh(&mut self, doc: &mut DocumentModel) -> bool {
        let session = doc.session().clone();
        let mut ok = true;
        match session.mixer() {
            Ok(m) => {
                let found = m.strips.iter().find_map(|s| {
                    MixerModel::slots(s)
                        .find(|x| x.id == self.slot)
                        .map(|x| (s.name.clone(), x.clone()))
                });
                let (name, row) = found.map_or((String::new(), None), |(n, r)| (n, Some(r)));
                self.row = row;
                self.strip_name = name;
                // The instance went (its plug-in was turned off): the host
                // closed its editor. The window shows the placeholder and
                // opens the next instance's editor once it runs.
                if self.row.is_some() && !self.is_live() && self.editor_size.is_some() {
                    self.editor_size = None;
                    doc.plugin_news.editor_closed(self.slot);
                }
            }
            Err(_) => ok = false,
        }
        let slot_gone = ok && self.row.is_none();
        let mut present = None;
        match session.params(self.slot) {
            Ok(p) => {
                self.params = p;
                // A slider mid-drag keeps showing where it is.
                for (id, v) in &self.live {
                    if let Some(p) = self.params.iter_mut().find(|p| p.id == *id) {
                        p.value = *v;
                    }
                }
                present = Some(self.params.iter().map(|p| p.id).collect::<HashSet<u64>>());
            }
            Err(_) => ok = false,
        }
        self.forget_gone(doc, slot_gone, present.as_ref());
        if ok {
            doc.plugin_news.params_changed.remove(&self.slot);
            self.seen = Some((doc.snapshot().version, doc.slots_version()));
        } else {
            self.seen = None;
        }
        ok
    }

    /// Opens the plug-in's editor in the view at address `parent` at
    /// window scale `scale`; false (the reason kept) when it has none or
    /// is not running. `Busy` (an editor still opening, a busy host) leaves
    /// the open owed: the window calls again on its tick with the same
    /// parent.
    pub fn open_editor(&mut self, doc: &mut DocumentModel, parent: u64, scale: f64) -> bool {
        if !self.is_live() {
            self.editor_problem = Some(self.placeholder_reason().unwrap_or(LOADING).to_owned());
            self.open_owed = false;
            return false;
        }
        match doc.session().open_editor(self.slot, parent, scale) {
            Ok(size) => {
                self.editor_size = Some(size);
                self.editor_problem = None;
                self.open_owed = false;
                doc.plugin_news.editors.insert(self.slot);
                // Nothing asked before this open is this editor's.
                doc.plugin_news.resizes.remove(&self.slot);
                true
            }
            Err(e) => {
                self.editor_size = None;
                self.editor_problem = Some(error_message(&e));
                self.open_owed = matches!(e, VizError::Busy { .. });
                false
            }
        }
    }

    /// The user resized the window to an editor area of `width` × `height`
    /// points: a resizable editor is asked, and the window snaps to what it
    /// answers (returned). `None`: a fixed editor, none, or a refusal; the
    /// size stays.
    pub fn resize(&mut self, doc: &DocumentModel, width: u32, height: u32) -> Option<EditorSize> {
        if !self.editor_size.is_some_and(|s| s.resizable) {
            return None;
        }
        let s = doc.session().resize_editor(self.slot, width, height).ok()?;
        self.editor_size = Some(s);
        Some(s)
    }

    /// The size the open editor asked for at the last plug-in ticks, once
    /// (the latest); the window follows it. `None` without a request or an
    /// open editor (a request for a closed one was dropped).
    pub fn take_editor_resize(&mut self, doc: &mut DocumentModel) -> Option<EditorSize> {
        let r = doc.plugin_news.resizes.remove(&self.slot)?;
        let size = self.editor_size.as_mut()?;
        size.width = r.width;
        size.height = r.height;
        Some(*size)
    }

    /// The window was made: the project reopens it.
    pub fn opened(&mut self, doc: &DocumentModel) {
        let _ = doc.session().set_window_open(self.slot, true);
    }

    /// The window closes: a drag still open ends (one undo step), the
    /// project no longer reopens it and the editor detaches (ending any
    /// gesture it left open). False when the editor could not detach yet
    /// (`Busy`): its parent view must stay alive until
    /// [`PluginWindowModel::retry_close`] returns true.
    pub fn closed(&mut self, doc: &mut DocumentModel, now: Instant) -> bool {
        self.end_open_drags(doc, now);
        let _ = doc.session().set_window_open(self.slot, false);
        self.open_owed = false;
        doc.plugin_news.window_closed(self.slot);
        let done = self.detach(doc);
        self.editor_size = None;
        done
    }

    /// Retries a close that met `Busy`; true once the editor is detached
    /// (or the failure is not retryable), and when nothing waits.
    pub fn retry_close(&mut self, doc: &DocumentModel) -> bool {
        if !self.close_pending {
            return true;
        }
        self.detach(doc)
    }

    fn detach(&mut self, doc: &DocumentModel) -> bool {
        if let Err(VizError::Busy { .. }) = doc.session().close_editor(self.slot) {
            self.close_pending = true;
            return false;
        }
        self.close_pending = false;
        true
    }

    /// Releases every slider still mid-drag here (the window closing),
    /// each once: the core lands the step and no gesture is left open.
    pub fn end_open_drags(&mut self, doc: &mut DocumentModel, now: Instant) {
        self.editing.clear();
        self.live.clear();
        let open = std::mem::take(&mut self.dragging);
        if open.is_empty() {
            return;
        }
        let slot = self.slot;
        doc.with_mixer(|m, d| {
            for id in open {
                m.release(d, param_target(slot, id), now);
            }
        });
    }

    // Intents.

    /// The header's Bypass: one undo step; a refusal is the alert "Bypass
    /// Failed".
    pub fn toggle_bypass(&mut self, doc: &mut DocumentModel) {
        let Some(on) = self.row.as_ref().map(|r| r.bypass) else {
            return;
        };
        match doc.session().set_bypass(self.slot, !on) {
            Ok(s) => doc.apply(s),
            Err(e) => doc.alert = Some(AlertItem::from_error(&e, BYPASS_FAILED)),
        }
        self.refresh(doc);
    }

    /// The slider's editing span begins (its mouse-down). Nothing is sent
    /// until it moves.
    pub fn begin_param(&mut self, id: u64) {
        self.editing.insert(id);
        self.touched(id);
    }

    /// A parameter slider moves to plain `value` (in the parameter's
    /// range). Inside an editing span the first move touches the parameter
    /// and every move sends its normalised value; after a refused touch
    /// (or a refused value, which releases the touch) the drag sends
    /// nothing more. A value with no editing span open (a key or an
    /// accessibility increment) is a click: touch, value and release at
    /// once.
    pub fn set_param(&mut self, doc: &mut DocumentModel, id: u64, value: f64, now: Instant) {
        let Some(i) = self.params.iter().position(|p| p.id == id) else {
            return;
        };
        self.touched(id);
        let n = Self::normalised(value, &self.params[i]);
        let shown = Self::clamped(value, &self.params[i]);
        let t = param_target(self.slot, id);
        let first = !self.dragging.contains(&id);
        if first && !self.editing.contains(&id) {
            if doc.with_mixer(|m, d| m.click(d, t, n, now)) == Some(true) {
                self.params[i].value = shown;
            }
            return;
        }
        if first {
            self.dragging.push(id);
        }
        let held = doc
            .with_mixer(|m, d| {
                if first {
                    m.touch(d, t, false);
                }
                if !m.holds_touch(t) {
                    return false;
                }
                m.move_to(d, t, n, now);
                m.holds_touch(t)
            })
            .unwrap_or(false);
        if held {
            self.live.insert(id, shown);
            self.params[i].value = shown;
        }
    }

    /// The slider is let go: a drag under way is released (the core lands
    /// it as one undo step) and the document shows it. The slider keeps its
    /// last value until the next read.
    pub fn commit_param(&mut self, doc: &mut DocumentModel, id: u64, now: Instant) {
        self.editing.remove(&id);
        let Some(i) = self.dragging.iter().position(|d| *d == id) else {
            return;
        };
        self.dragging.remove(i);
        self.live.remove(&id);
        let t = param_target(self.slot, id);
        doc.with_mixer(|m, d| m.release(d, t, now));
    }

    /// The plug-in's own editor touched a parameter (the core lists it as
    /// touched): this slot's first becomes the one Learn learns.
    pub fn follow_touched(&mut self, targets: &[AutomationTargetRow]) {
        let ours = targets.iter().find_map(|t| match *t {
            AutomationTargetRow::PluginParam { slot, param } if slot == self.slot => Some(param),
            _ => None,
        });
        if let Some(p) = ours {
            self.touched(p);
        }
    }

    /// With the editor open, follows the core's touched parameters (the
    /// window's tick). A refused read keeps the last.
    pub fn follow_core_touches(&mut self, doc: &DocumentModel) {
        if self.editor_size.is_none() {
            return;
        }
        if let Ok(a) = doc.session().automation_state() {
            self.follow_touched(&a.touched);
        }
    }

    // Plumbing.

    fn touched(&mut self, id: u64) {
        self.last_touched = Some((self.slot, id));
    }

    /// Ends (once, quietly: the control went) the drags whose slot or
    /// parameter a good read no longer has; a refused drag's mark goes with
    /// them, so the parameter's next drag touches again.
    fn forget_gone(
        &mut self,
        doc: &mut DocumentModel,
        slot_gone: bool,
        present: Option<&HashSet<u64>>,
    ) {
        let gone = |id: &u64| slot_gone || present.is_some_and(|p| !p.contains(id));
        self.editing.retain(|id| !gone(id));
        let (lost, kept): (Vec<u64>, Vec<u64>) = self.dragging.iter().partition(|id| gone(id));
        if lost.is_empty() {
            return;
        }
        self.dragging = kept;
        for id in &lost {
            self.live.remove(id);
        }
        let slot = self.slot;
        doc.with_mixer(|m, d| {
            for id in lost {
                m.drop_touch(d, param_target(slot, id));
            }
        });
    }
}

fn param_target(slot: u64, param: u64) -> AutomationTargetRow {
    AutomationTargetRow::PluginParam { slot, param }
}
