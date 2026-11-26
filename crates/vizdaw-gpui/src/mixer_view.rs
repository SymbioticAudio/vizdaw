//! The mixer (Swift `MixerView`, `MixerBar`, `ChannelStrips`,
//! `ChannelStrip`): the bar (the DSP load and Add Bus) over the strips,
//! side by side in the model's order with master after a divider. Every
//! strip shares the rows above its fader (`StripGrid`, explicit heights,
//! Decision M8), so faders, level boxes and buttons line up. Top to
//! bottom: the header (colour bar, name, kind; a click selects, ⌘- or
//! ⇧-click adds or removes, a double-click renames, its menu holds
//! Rename…, Set All Strips to ▸, Remove Bus, Colour ▸ and the group and
//! VCA items), the instrument slot, the EQ slot, the inserts (each with
//! its dot and menu, dragged to reorder), the sends (name, PRE chip,
//! knob and level box, their menu; the Send menu and New Bus), the output
//! menu, the pan, the fader with its scale and meter, the level, M S R I
//! and the automation mode. Master has no sends, output, pan or solo (a
//! blank keeps the row). A strip silent because others solo dims to
//! half; the selected strip's edge is the accent. "Open" and a slot's
//! double-click (the EQ slot's click) open its plug-in window, and the
//! Built-in Synth slot's double-click its sound window. What needs the
//! plugin catalogue (the instrument menu, "+", "EQ ＋", "Replace…") shows
//! disabled.
//!
//! A grouped strip shows its group tag under its kind (its menu: Link
//! Mutes, Link Solos, Remove from Group, Ungroup); a drag of its fader is
//! the core's group drag, ⌥ at the press moving it alone. While the mixer
//! has a VCA every strip has the VCA offset row under its level box (a
//! member's fader shows its own level). External MIDI destinations'
//! strips (`midi_strip`) sit before the first bus and the VCA columns
//! (`vca_strip`) after the strips; all of them share the strips' rows.
//!
//! The strips scroll across (and down, when even the shortest fader does
//! not fit); only the strips in view are drawn and recorded.
//!
//! One [`MixerView`] per placement (the Mix view's centre, the bottom
//! pane, the Mixer window, and the left pane's Channel Strip, which
//! shows only the focused track's strip, centred, with no bar; a press
//! whose column goes ends): it says it appeared when made and that it
//! went when dropped (the last placement's going releases every open
//! touch), and the window losing the keyboard cancels its drag.
//!
//! While a placement shows it polls the mixer every 30 Hz period (Swift's
//! `MixerView` task: `tick` reads the levels, the automation state and,
//! while playing, the heard values, at most once per period however many
//! placements ask); every placement redraws when a tick changed what it
//! shows. Between two reads a meter above silence asks for every frame
//! and is drawn fallen by the time since the read (`level_at`).

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Bounds, ClickEvent, Context, DispatchPhase, Entity, Focusable,
    FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Rgba, ScrollDelta, ScrollEvent,
    StatefulInteractiveElement, Styled, Subscription, Task, WeakEntity, Window, anchored, canvas,
    deferred, div, point, px, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::builtin_synth_window::builtin_synth_destination;
use vizdaw_app::lane_inputs::{InputChoice, LaneInputs};
use vizdaw_app::mixer::layout::{
    RowHeights, StripGrid, StripLayout, StripRowKind, midi_rows, rows_of, vca_rows,
};
use vizdaw_app::mixer::meters::{METER_PERIOD, POLL_INTERVAL};
use vizdaw_app::mixer::{
    AutomationModes, GroupLabels, InstrumentRow, MeterLevel, MidiControl, MidiStripLabels,
    MidiStripMath, MixerColumn, MixerModel, MixerPlacement, SendLabels, SlotLabels, SlotMenuItem,
    SlotPlace, SlotState, StripPalette, VcaStripLabels,
};
use vizdaw_app::view::StripFocus;
use vizdaw_ffi::automation::AutomationModeRow;
use vizdaw_ffi::edit::MonitorModeRow;
use vizdaw_ffi::mixer::{DestRow, StripKindRow, StripRow, SwatchRow};
use vizdaw_ffi::snapshot::{LaneRow, SlotRow};

use crate::controls::{
    self, ControlBinding, Entry, Grip, KnobSize, MixerRef, NumberBinding, NumberBox, Toggle, fader,
};
use crate::frame_stats;
use crate::input_picker;
use crate::plugin_windows::{self, Subject};
use crate::registry::RegistryScope;
use crate::strip_palette::{KnobFunction, tokens};
use crate::text_input::{TextInput, TextInputEvent};

/// What a placement shows when the mixer cannot be read (Swift's
/// `ContentUnavailableView`).
pub const UNAVAILABLE: &str = "Mixer Unavailable";
pub const UNAVAILABLE_NOTE: &str = "The mixer of this document could not be read.";
/// The bar's button.
pub const ADD_BUS: &str = "Add Bus";
/// The padding around the strips (`spacing.s`).
pub const PADDING: f64 = 8.;
/// The bar's height.
pub const BAR_HEIGHT: f64 = 32.;
/// How many frames [`MixerView::fits`] keeps.
const FITS_KEPT: usize = 32;
/// The rename field's label.
pub const NAME: &str = "Name";
/// The header's parts: the colour bar, the name and the kind, 2 apart.
const NAME_TOP: f64 = StripLayout::COLOUR_BAR + 2.;
const NAME_HEIGHT: f64 = 16.;
const KIND_TOP: f64 = NAME_TOP + NAME_HEIGHT + 2.;
const KIND_HEIGHT: f64 = 14.;
/// The panel's corner (`radius.m`).
const PANEL_RADIUS: f32 = 5.;
/// A wheel line in points.
const LINE: f32 = 20.;
/// How often a shown placement polls (Swift sleeps 33 ms between ticks;
/// `tick` reads at most every 30 ms).
pub const POLL_EVERY: Duration = METER_PERIOD;

/// A column of the mixer: an audio strip, a VCA, or an external MIDI
/// destination's strip; in the left pane, a MIDI track with no
/// destination (its track).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnKey {
    Strip(u64),
    Vca(u64),
    Dest(u64),
    Track(u64),
}

/// The destination the disabled controls of a MIDI track with no
/// destination name (they never act on it).
const NO_DEST: u64 = u64::MAX;

/// A column's typed box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BoxKind {
    /// The level (a MIDI strip's volume).
    Level,
    Pan,
    /// Send `n`'s (from 0).
    Send(usize),
}

/// A strip's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripMenu {
    Output,
    Automation,
    /// The name's menu (Rename…, Colour ▸, …).
    Strip,
    /// The sends' "Send" menu: the buses, then New Bus.
    AddSend,
    /// Send `n`'s (from 0) menu: Pre-Fader, On, Remove Send.
    Send(usize),
    /// A slot's menu (the slot's ID).
    Slot(u64),
    /// A grouped strip's group tag: Link Mutes, Link Solos, Remove from
    /// Group, Ungroup.
    GroupTag,
    /// A VCA's name menu (its ID with it): Rename VCA…, Delete VCA.
    Vca,
    /// A VCA's automation mode menu (its ID with it).
    VcaMode,
    /// The input picker of an audio path's input track.
    Input,
}

impl StripMenu {
    /// The column the menu belongs to: `id` names a VCA for a VCA's
    /// menus, else a strip.
    fn column(self, id: u64) -> ColumnKey {
        match self {
            StripMenu::Vca | StripMenu::VcaMode => ColumnKey::Vca(id),
            _ => ColumnKey::Strip(id),
        }
    }
}

/// The gap between two inserts and their height: a drag moves an insert
/// one place per this many points.
const INSERT_PITCH: f64 = RowHeights::SLOT + RowHeights::PART_GAP;
/// How far the pointer moves before a press on an insert drags it.
const DRAG_START: f32 = 2.;

/// An insert dragged to another place among its strip's inserts (Swift's
/// drag and drop of a slot): nothing is sent until the drop, which is one
/// `move_insert`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InsertDrag {
    pub strip: u64,
    pub slot: u64,
    /// Its place now (from 0).
    pub from: usize,
    /// The strip's inserts.
    count: usize,
    /// Where the press went down (window y).
    origin: f32,
    /// Where it drops, once the pointer moved: its new place (from 0).
    pub over: Option<usize>,
}

/// How a strip was drawn last frame: its dimming, its edge and its
/// meter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripLook {
    pub strip: u64,
    /// Silent because other strips solo: the panel at half.
    pub dimmed: bool,
    /// Selected: the 2-point accent edge.
    pub selected: bool,
    /// The meter as drawn (the last read fallen by the time since).
    pub meter: MeterLevel,
}

/// What a rename field renames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameTarget {
    /// A strip, in its header: losing the keyboard commits.
    Strip(u64),
    /// A VCA, in a popover under its name: dismissing it cancels.
    Vca(u64),
}

impl RenameTarget {
    fn renaming(self, m: &MixerModel, p: MixerPlacement) -> bool {
        match self {
            RenameTarget::Strip(s) => m.is_renaming(s, p),
            RenameTarget::Vca(v) => m.is_renaming_vca(v, p),
        }
    }

    /// Ends it in the mixer (still renaming here), with the field's text
    /// when `commit`.
    fn end(
        self,
        m: &mut MixerModel,
        d: &mut DocumentModel,
        p: MixerPlacement,
        text: String,
        commit: bool,
    ) {
        if !self.renaming(m, p) {
            return;
        }
        match (self, commit) {
            (RenameTarget::Strip(_), true) => {
                m.rename_draft = text;
                m.commit_rename(d);
            }
            (RenameTarget::Strip(_), false) => m.cancel_rename(),
            (RenameTarget::Vca(_), true) => {
                m.rename_draft = text;
                m.commit_vca_rename(d);
            }
            (RenameTarget::Vca(_), false) => m.cancel_vca_rename(),
        }
    }
}

/// An open rename field.
pub(crate) struct RenameField {
    pub(crate) target: RenameTarget,
    pub(crate) field: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

/// One placement of a document's mixer: its scroll, its typed boxes, its
/// open menu and rename field.
pub struct MixerView {
    pub(crate) mixer: MixerRef,
    /// The strips' scroll (x, y), in points.
    scroll: (f64, f64),
    /// How far the strips scrolled last frame at most (x, y).
    room: (f64, f64),
    /// The strips' area, as last drawn.
    area: Rc<Cell<Bounds<Pixels>>>,
    boxes: HashMap<(ColumnKey, BoxKind), Entity<NumberBox>>,
    pub(crate) menu: Option<(u64, StripMenu)>,
    /// The row of the open menu whose submenu is open.
    submenu: Option<usize>,
    insert_drag: Option<InsertDrag>,
    pub(crate) rename: Option<RenameField>,
    /// The placement told the mixer it shows.
    appeared: bool,
    /// The mixer was asked for (to show, or to be read again) less than
    /// [`POLL_INTERVAL`] ago: a busy core is not asked again until this
    /// ends, which draws a frame.
    hold: Option<Task<()>>,
    drawn: Vec<StripLook>,
    /// Each frame's strips' height (padding included) and the height of
    /// the area they were made for, the last [`FITS_KEPT`] frames.
    fits: Vec<(f64, f64)>,
    /// The MIDI strips drawn last frame: each destination and the note
    /// activity its meter showed.
    midi_drawn: Vec<(u64, f64)>,
    /// The mixer's `shown_version` this placement last drew for.
    seen: Option<u64>,
    /// When the placement came (the synthetic meters' clock).
    started: Instant,
    /// Polls every [`POLL_EVERY`] while the placement lives.
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl MixerView {
    /// The placement `mixer` names, in `window`.
    pub fn new(mixer: MixerRef, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let p = mixer.placement;
        let gone = mixer.clone();
        cx.on_release(move |v: &mut MixerView, cx| {
            if let Some(r) = v.rename.take() {
                let owner = rename_owner(p);
                let text = r.field.read(cx).text().to_owned();
                gone.app.update(cx, |s, cx| {
                    s.store
                        .with_document(gone.doc, |d, _| d.end_text_editing(&owner));
                    cx.notify();
                });
                // The field goes with its placement (the bottom pane hidden
                // for a frame): a strip's rename commits, as a field
                // losing the keyboard does (Swift's `endPaneEdits`); a
                // VCA's popover goes with it, cancelled.
                let target = r.target;
                let commit = matches!(target, RenameTarget::Strip(_));
                gone.with(cx, |m, d| target.end(m, d, p, text, commit));
                drop(r);
            }
            if v.appeared {
                gone.with(cx, |m, d| m.disappeared(d, p, Instant::now()));
            }
        })
        .detach();
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |v, cx| v.poll(cx)).is_err() {
                    break;
                }
            }
        });
        let mut v = MixerView {
            _subscriptions: vec![controls::watch_activation(mixer.clone(), window, cx)],
            seen: None,
            started: cx.background_executor().now(),
            _poll: poll,
            mixer,
            scroll: (0., 0.),
            room: (0., 0.),
            area: Rc::default(),
            boxes: HashMap::new(),
            menu: None,
            submenu: None,
            insert_drag: None,
            rename: None,
            appeared: false,
            hold: None,
            drawn: vec![],
            fits: vec![],
            midi_drawn: vec![],
        };
        // Now, so the frame that shows the placement has its strips; a
        // busy core is asked again when the hold ends.
        if !v.appear(cx) && !v.refused(cx) {
            v.may_ask(cx);
        }
        v
    }

    /// Tells the mixer (made first if need be) this placement shows; true
    /// when it could. Nothing redraws for it: the caller draws next.
    fn appear(&mut self, cx: &mut Context<Self>) -> bool {
        if self.appeared {
            return true;
        }
        let (doc, p) = (self.mixer.doc, self.mixer.placement);
        let (shown, alerted) = self.mixer.app.update(cx, |s, _| {
            let shown = s
                .store
                .with_document(doc, |d, _| d.with_mixer(|m, _| m.appeared(p)).is_some())
                .unwrap_or(false);
            (shown, s.store.alert().is_some())
        });
        self.appeared = shown;
        // An alert the read raised shows at the next frame.
        if !shown && alerted {
            let app = self.mixer.app.clone();
            cx.defer(move |cx| app.update(cx, |_, cx| cx.notify()));
        }
        shown
    }

    /// The mixer was refused at this document version (not Busy): it is
    /// not asked again until the document moves, which draws anyway.
    fn refused(&self, cx: &App) -> bool {
        self.mixer
            .app
            .read(cx)
            .store
            .document(self.mixer.doc)
            .is_some_and(DocumentModel::mixer_refused)
    }

    /// Whether the mixer may be asked for now: not within
    /// [`POLL_INTERVAL`] of the last ask. Asking holds it that long, and
    /// the hold's end draws a frame (which asks again if still needed).
    fn may_ask(&mut self, cx: &mut Context<Self>) -> bool {
        if self.hold.is_some() {
            return false;
        }
        self.hold = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(POLL_INTERVAL).await;
            this.update(cx, |v, cx| {
                v.hold = None;
                cx.notify();
            })
            .ok();
        }));
        true
    }

    pub fn placement(&self) -> MixerPlacement {
        self.mixer.placement
    }

    /// One poll of the shown placement (Swift `MixerModel.tick`): a
    /// release waiting on Busy is retried; at most once per period the
    /// mixer is read again if the document moved, and the levels, the
    /// automation state and the heard values are read. The placement
    /// redraws when that changed what it shows (another placement's tick
    /// counts too); the app redraws when the document or its alert moved.
    fn poll(&mut self, cx: &mut Context<Self>) {
        if !self.appeared {
            return;
        }
        let now = cx.background_executor().now();
        let (doc, started) = (self.mixer.doc, self.started);
        let shown = self.mixer.app.update(cx, |s, cx| {
            let alert = s.store.alert().map(|a| a.id);
            let ticked = s.store.with_document(doc, |d, _| {
                let seen =
                    |d: &DocumentModel| (d.snapshot().version, d.alert.as_ref().map(|a| a.id));
                let before = seen(d);
                let shown = d.with_mixer(|m, d| {
                    if m.tick(d, now) && frame_stats::synthetic_meters() {
                        let strips = m.snapshot().strips.iter().map(|s| s.id);
                        let l = frame_stats::synthetic_levels(strips, started, now);
                        m.take_levels(&l, now);
                    }
                    m.shown_version()
                });
                (shown, seen(d) != before)
            });
            let (shown, moved) = ticked.unwrap_or((None, false));
            if moved || s.store.alert().map(|a| a.id) != alert {
                cx.notify();
            }
            shown
        });
        if shown.is_some() && shown != self.seen {
            self.seen = shown;
            cx.notify();
        }
    }

    /// The strips drawn last frame, left to right.
    pub fn drawn(&self) -> &[StripLook] {
        &self.drawn
    }

    /// The last frames' strips: each one's height (padding included) and
    /// the height of the area it was made for, oldest first.
    pub fn fits(&self) -> &[(f64, f64)] {
        &self.fits
    }

    /// The MIDI strips drawn last frame, left to right: each destination
    /// and the note activity (0…1) its meter showed.
    pub fn midi_drawn(&self) -> &[(u64, f64)] {
        &self.midi_drawn
    }

    /// The strips' scroll (x, y).
    pub fn scroll(&self) -> (f64, f64) {
        self.scroll
    }

    /// How far the strips scrolled at most last frame (x, y).
    pub fn room(&self) -> (f64, f64) {
        self.room
    }

    /// Scrolls the strips to (x, y) (kept inside them at the next draw).
    pub fn scroll_to(&mut self, x: f64, y: f64, cx: &mut Context<Self>) {
        self.scroll = (x.max(0.), y.max(0.));
        cx.notify();
    }

    /// The open menu: its strip and which.
    pub fn menu(&self) -> Option<(u64, StripMenu)> {
        self.menu
    }

    /// Starts the rename of `strip` in this placement: a field holding the
    /// name, focused. Return commits (blank or unchanged sends nothing),
    /// Escape cancels, the field losing the keyboard commits. A rename
    /// already open commits first.
    pub fn begin_rename(&mut self, strip: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.begin_field(RenameTarget::Strip(strip), window, cx);
    }

    /// Starts the rename of VCA `vca` in this placement (Swift's popover
    /// under its name): Return commits, Escape cancels, and the popover
    /// going (a click elsewhere, the field losing the keyboard) cancels.
    pub fn begin_vca_rename(&mut self, vca: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.begin_field(RenameTarget::Vca(vca), window, cx);
    }

    /// The open rename field's target.
    pub fn renaming(&self) -> Option<RenameTarget> {
        self.rename.as_ref().map(|r| r.target)
    }

    fn begin_field(&mut self, target: RenameTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.end_rename(true, cx);
        let p = self.mixer.placement;
        let Some(name) = self.mixer.with(cx, |m, _| {
            match target {
                RenameTarget::Strip(strip) => {
                    m.strip(strip)?;
                    m.begin_rename(strip, p);
                }
                RenameTarget::Vca(vca) => {
                    m.vca(vca)?;
                    m.begin_vca_rename(vca, p);
                }
            }
            Some(m.rename_draft.clone())
        }) else {
            return;
        };
        let Some(name) = name else { return };
        let label = match target {
            RenameTarget::Strip(_) => NAME,
            RenameTarget::Vca(_) => VcaStripLabels::RENAME_FIELD,
        };
        let field = cx.new(|cx| TextInput::new(label, cx).with_text(&name));
        let focus = field.focus_handle(cx);
        // A strip's field losing the keyboard commits; a VCA's popover
        // going cancels (Swift).
        let blur_commits = matches!(target, RenameTarget::Strip(_));
        let subscriptions = vec![
            cx.subscribe_in(
                &field,
                window,
                |this, _, e: &TextInputEvent, _, cx| match e {
                    TextInputEvent::Confirm => this.end_rename(true, cx),
                    TextInputEvent::Cancel => this.end_rename(false, cx),
                    TextInputEvent::Tab | TextInputEvent::BackTab => {}
                },
            ),
            cx.on_blur(&focus, window, move |this, _, cx| {
                this.end_rename(blur_commits, cx)
            }),
            // The mixer holds what is typed, so a commit from elsewhere (a
            // view change, Save) takes it.
            cx.observe(&field, move |this, field, cx| {
                let text = field.read(cx).text().to_owned();
                let p = this.mixer.placement;
                // Only a changed text reaches the mixer: `with` redraws
                // every window, and the field notifies on every caret
                // move and blink.
                let changed = this
                    .mixer
                    .read(cx, |m, _| target.renaming(m, p) && m.rename_draft != text)
                    .unwrap_or(false);
                if changed {
                    this.mixer.with(cx, |m, _| m.rename_draft = text);
                }
            }),
        ];
        window.focus(&focus, cx);
        let owner = rename_owner(p);
        let doc = self.mixer.doc;
        self.mixer.app.update(cx, |s, cx| {
            s.store
                .with_document(doc, |d, _| d.begin_text_editing(&owner));
            cx.notify();
        });
        self.rename = Some(RenameField {
            target,
            field,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    /// Ends the open rename field: the strip or VCA is renamed to its
    /// text when `commit` and the mixer still renames it here.
    pub fn end_rename(&mut self, commit: bool, cx: &mut Context<Self>) {
        let Some(r) = self.rename.take() else { return };
        let text = r.field.read(cx).text().to_owned();
        let p = self.mixer.placement;
        let (doc, owner) = (self.mixer.doc, rename_owner(p));
        self.mixer.app.update(cx, |s, cx| {
            s.store
                .with_document(doc, |d, _| d.end_text_editing(&owner));
            cx.notify();
        });
        let target = r.target;
        self.mixer
            .with(cx, |m, d| target.end(m, d, p, text, commit));
        cx.notify();
    }

    /// The open menu's open submenu: its row.
    pub fn submenu(&self) -> Option<usize> {
        self.submenu
    }

    pub(crate) fn toggle_menu(&mut self, strip: u64, which: StripMenu, cx: &mut Context<Self>) {
        self.menu = if self.menu == Some((strip, which)) {
            None
        } else {
            Some((strip, which))
        };
        self.submenu = None;
        cx.notify();
    }

    /// Opens a context menu (a right-click).
    pub(crate) fn open_menu(&mut self, strip: u64, which: StripMenu, cx: &mut Context<Self>) {
        self.menu = Some((strip, which));
        self.submenu = None;
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.submenu = None;
        cx.notify();
    }

    /// Escape: closes an open input picker. True when one was open.
    pub(crate) fn close_input_picker(&mut self, cx: &mut Context<Self>) -> bool {
        if self.menu.is_some_and(|(_, m)| m == StripMenu::Input) {
            self.close_menu(cx);
            return true;
        }
        false
    }

    fn open_submenu(&mut self, row: usize, cx: &mut Context<Self>) {
        if self.submenu != Some(row) {
            self.submenu = Some(row);
            cx.notify();
        }
    }

    /// The insert being dragged.
    pub fn insert_drag(&self) -> Option<InsertDrag> {
        self.insert_drag
    }

    /// A press on insert `slot` at place `from` of `strip`'s `count`.
    fn press_insert(&mut self, strip: u64, slot: u64, from: usize, count: usize, y: Pixels) {
        self.insert_drag = Some(InsertDrag {
            strip,
            slot,
            from,
            count,
            origin: f32::from(y),
            over: None,
        });
    }

    /// The pointer moved while an insert may be dragged: past
    /// [`DRAG_START`] it drags, a place per [`INSERT_PITCH`]; a move
    /// without the button (its up went elsewhere) ends the drag, moving
    /// nothing.
    fn drag_insert(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(d) = self.insert_drag.as_mut() else {
            return;
        };
        if e.pressed_button != Some(MouseButton::Left) {
            self.insert_drag = None;
            cx.notify();
            return;
        }
        let dy = f32::from(e.position.y) - d.origin;
        if d.over.is_none() && dy.abs() < DRAG_START {
            return;
        }
        let places = (f64::from(dy) / INSERT_PITCH).round();
        let last = d.count.saturating_sub(1) as f64;
        let over = (d.from as f64 + places).clamp(0., last) as usize;
        if d.over != Some(over) {
            d.over = Some(over);
            cx.notify();
        }
    }

    /// A press while an insert drag is open: its mouse-up never came, so
    /// it ends where it started.
    fn cancel_insert_drag(&mut self, cx: &mut Context<Self>) {
        if self.insert_drag.take().is_some() {
            cx.notify();
        }
    }

    /// The mouse-up: a dragged insert lands at its new place (one core
    /// call); back where it was, or never dragged, nothing is sent.
    fn drop_insert(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.insert_drag.take() else {
            return;
        };
        cx.notify();
        if let Some(to) = d.over.filter(|to| *to != d.from) {
            self.mixer
                .with(cx, |m, doc| m.move_insert(doc, d.slot, d.strip, to));
        }
    }

    fn wheel(&mut self, e: &ScrollEvent, cx: &mut Context<Self>) {
        let (dx, dy) = match e.delta {
            ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(p) => (p.x * LINE, p.y * LINE),
        };
        // ⇧, or no room down: a vertical wheel scrolls across.
        let across = e.modifiers.shift || self.room.1 <= 0.;
        let (dx, dy) = if across && dx == 0. {
            (dy, 0.)
        } else {
            (dx, dy)
        };
        self.scroll = (
            (self.scroll.0 - f64::from(dx)).max(0.),
            (self.scroll.1 - f64::from(dy)).max(0.),
        );
        cx.notify();
    }

    /// The box `key`, made once and kept while its strip is there.
    pub(crate) fn number_box(
        &mut self,
        key: (ColumnKey, BoxKind),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<NumberBox> {
        self.boxes
            .entry(key)
            .or_insert_with(|| cx.new(|cx| NumberBox::new(window, cx)))
            .clone()
    }
}

fn rename_owner(p: MixerPlacement) -> String {
    format!("mixer rename {p:?}")
}

/// An output menu choice: its title and its bus (`None`: master).
type OutputChoice = (String, Option<u64>);

/// What a slot row shows.
struct SlotPart {
    id: u64,
    /// The plugin's name.
    name: String,
    /// Its record's label ("Comp", "Pad, instrument").
    label: String,
    dot: Rgba,
    bypass: bool,
    /// It passes audio through (a placeholder, faulted): the name dims.
    dim: bool,
}

/// What a strip's instrument row shows.
enum InstrumentPart {
    Plugin(SlotPart),
    BuiltinSynth,
}

/// What one send shows.
struct SendPart {
    index: usize,
    /// The bus it feeds.
    name: String,
    enabled: bool,
    pre_fader: bool,
    knob: ControlBinding,
    level: NumberBinding,
}

/// What one strip in view shows.
struct StripParts {
    index: usize,
    id: u64,
    name: String,
    kind_label: String,
    instrument: Option<InstrumentPart>,
    eq: Option<SlotPart>,
    inserts: Vec<SlotPart>,
    /// The sends (none for master).
    sends: Option<Vec<SendPart>>,
    bar: Rgba,
    cap: Rgba,
    dimmed: bool,
    selected: bool,
    fader: ControlBinding,
    level: NumberBinding,
    pan: Option<(ControlBinding, NumberBinding)>,
    mute: Toggle,
    solo: Option<Toggle>,
    arm: Toggle,
    monitor: Toggle,
    /// The input row: the input track's picker and monitor button.
    input: Option<InputParts>,
    /// The output menu's face and its choices (title, bus).
    output: Option<(String, Vec<OutputChoice>)>,
    mode: AutomationModeRow,
    meter: MeterLevel,
    /// Its group's tag, when grouped.
    group: Option<GroupTag>,
    /// The VCA offset row, while the mixer has a VCA.
    offset: Option<OffsetParts>,
}

/// A strip's input row (Swift `StripInputView`): the input track's picker
/// and its monitor button.
struct InputParts {
    lane: LaneRow,
    /// The picker's face (Swift `pickerTitle`).
    title: String,
    /// "Input of <track>[, recording ⇠ <synth>]".
    help: String,
    /// The monitor button, `<track>: <help>`.
    monitor: Toggle,
    monitor_help: &'static str,
}

/// A grouped strip's tag (Swift `StripGroupTag`).
struct GroupTag {
    id: u64,
    name: String,
    colour: Rgba,
    link_mute: bool,
    link_solo: bool,
}

/// A strip's VCA offset row (Swift `StripVcaOffset`).
struct OffsetParts {
    /// "VCA −3.0"; none at 0.0.
    text: Option<String>,
    /// The "VCA" mark, lit while a VCA mutes or solos it; none for a
    /// strip in no VCA.
    mark: Option<bool>,
}

/// What a VCA column shows (Swift `VcaStripCells`).
pub(crate) struct VcaParts {
    pub(crate) index: usize,
    pub(crate) id: u64,
    pub(crate) name: String,
    /// Its label: "VCA <name>".
    pub(crate) title: String,
    /// "VCA · 3 members".
    pub(crate) kind_line: String,
    pub(crate) colour: Rgba,
    pub(crate) fader: ControlBinding,
    pub(crate) level: NumberBinding,
    pub(crate) mute: Toggle,
    pub(crate) solo: Toggle,
    pub(crate) mode: AutomationModeRow,
}

/// What an external MIDI destination's strip shows (Swift
/// `MidiStripCells`); in the left pane a MIDI track's, which may have no
/// destination ("No Destination": only R works).
pub(crate) struct MidiParts {
    pub(crate) index: usize,
    /// The destination; `None` for a track with none.
    pub(crate) id: Option<u64>,
    pub(crate) key: ColumnKey,
    pub(crate) name: String,
    pub(crate) kind_label: String,
    pub(crate) cap: Rgba,
    pub(crate) pan: ControlBinding,
    pub(crate) pan_box: NumberBinding,
    /// The pan is set (unset values are dimmed).
    pub(crate) pan_set: bool,
    pub(crate) fader: ControlBinding,
    pub(crate) volume: NumberBinding,
    pub(crate) volume_set: bool,
    pub(crate) mute: Toggle,
    pub(crate) arm: Toggle,
    /// The track R arms.
    pub(crate) lane: Option<u64>,
    /// The note activity, 0…1.
    pub(crate) activity: f64,
    /// A focused track plays it (in the mixer's placements).
    pub(crate) selected: bool,
}

/// One column in view (made and drawn once a frame each, so not boxed).
#[allow(clippy::large_enum_variant)]
enum ColumnParts {
    Audio(StripParts),
    Vca(VcaParts),
    Midi(MidiParts),
}

impl ColumnParts {
    fn index(&self) -> usize {
        match self {
            ColumnParts::Audio(s) => s.index,
            ColumnParts::Vca(v) => v.index,
            ColumnParts::Midi(m) => m.index,
        }
    }
}

/// What the placement draws this frame.
struct Gathered {
    grid: StripGrid,
    columns: usize,
    divider_before: Option<usize>,
    scroll: (f64, f64),
    /// How far right the columns are drawn (the left pane centres its
    /// strip).
    shift: f64,
    /// The most it scrolls (x, y).
    room: (f64, f64),
    strips: Vec<ColumnParts>,
    /// Every column, drawn or not.
    keys: HashSet<ColumnKey>,
    renaming: Option<(u64, MixerPlacement)>,
    renaming_vca: Option<(u64, MixerPlacement)>,
    /// The column whose control this placement's press holds.
    pressed: Option<ColumnKey>,
}

/// A column a placement shows: one of the mixer's, or (the left pane) a
/// MIDI track's strip, with or without its external destination.
#[derive(Clone, Copy)]
enum Column<'a> {
    Mixer(MixerColumn<'a>),
    Track {
        lane: &'a LaneRow,
        dest: Option<&'a DestRow>,
    },
}

/// The left pane has a column to show for the focused track.
pub(crate) fn has_focused_column(m: &MixerModel, d: &DocumentModel) -> bool {
    !focused_column(m, d).is_empty()
}

/// The left pane's column (Swift `LeftStripPane`): the strip the focused
/// track plays through, or its MIDI strip; none for the inspector or
/// nothing focused (the pane shows its note).
fn focused_column<'a>(m: &'a MixerModel, d: &'a DocumentModel) -> Vec<Column<'a>> {
    match d.left_strip(&m.snapshot().strips, m.destinations()) {
        StripFocus::Strip(id) => m
            .strip(id)
            .map(|s| Column::Mixer(MixerColumn::Audio(s)))
            .into_iter()
            .collect(),
        StripFocus::Midi { dest, lane } => d
            .lanes()
            .iter()
            .find(|l| l.id == lane)
            .map(|lane| Column::Track {
                lane,
                dest: dest.and_then(|x| m.destinations().iter().find(|r| r.id == x)),
            })
            .into_iter()
            .collect(),
        StripFocus::Inspector | StripFocus::Nothing => vec![],
    }
}

#[allow(clippy::too_many_arguments)]
fn gather(
    m: &MixerModel,
    d: &DocumentModel,
    mixer: &MixerRef,
    view: (f64, f64),
    scroll: (f64, f64),
    now: Instant,
) -> Gathered {
    let p = mixer.placement;
    // The columns (strips, MIDI strips before the first bus, VCAs), then
    // master after the divider; the left pane's one strip (no divider).
    let (all, divider_before) = if p == MixerPlacement::Strip {
        (focused_column(m, d), None)
    } else {
        let mut all: Vec<Column<'_>> = m.columns().into_iter().map(Column::Mixer).collect();
        let divider_before = m.master().map(|_| all.len());
        all.extend(m.master().map(|s| Column::Mixer(MixerColumn::Audio(s))));
        (all, divider_before)
    };
    let rows: Vec<BTreeMap<StripRowKind, f64>> = all
        .iter()
        .map(|c| match c {
            Column::Mixer(MixerColumn::Audio(s)) => rows_of(s, m.can_arm(d, s)),
            Column::Mixer(MixerColumn::Vca(_)) => vca_rows(),
            Column::Mixer(MixerColumn::Midi(_)) | Column::Track { .. } => midi_rows(),
        })
        .collect();
    let has_vcas = !m.vcas().is_empty();
    let grid = StripGrid::with_offset(&rows, (view.1 - 2. * PADDING).max(0.), has_vcas);
    let content = (
        2. * PADDING + StripGrid::width(all.len(), divider_before),
        2. * PADDING + grid.total,
    );
    let room = ((content.0 - view.0).max(0.), (content.1 - view.1).max(0.));
    let scroll = (scroll.0.clamp(0., room.0), scroll.1.clamp(0., room.1));
    // The left pane's strip is centred across a pane wider than it.
    let shift = if p == MixerPlacement::Strip {
        ((view.0 - content.0) / 2.).max(0.)
    } else {
        0.
    };
    let selected = d.selected_strips();
    use vizdaw_ffi::events::AutomationTargetRow as Target;
    let pressed = m
        .pressed(p)
        .and_then(|t| match t {
            Target::StripFader { strip }
            | Target::StripPan { strip }
            | Target::SendLevel { strip, .. } => Some(ColumnKey::Strip(strip)),
            Target::VcaFader { vca } => Some(ColumnKey::Vca(vca)),
            _ => None,
        })
        .or_else(|| m.midi_pressed(p).map(|(dest, _)| ColumnKey::Dest(dest)));
    let key_of = |c: &Column<'_>| match c {
        Column::Mixer(MixerColumn::Audio(s)) => ColumnKey::Strip(s.id),
        Column::Mixer(MixerColumn::Vca(v)) => ColumnKey::Vca(v.id),
        Column::Mixer(MixerColumn::Midi(x)) => ColumnKey::Dest(x.id),
        Column::Track { dest: Some(x), .. } => ColumnKey::Dest(x.id),
        Column::Track { lane, dest: None } => ColumnKey::Track(lane.id),
    };
    let mut strips = vec![];
    for (index, c) in all.iter().enumerate() {
        let left = PADDING + StripGrid::x(index, divider_before);
        let in_view = left + StripLayout::WIDTH > scroll.0 && left < scroll.0 + view.0;
        // A column whose control the mouse holds is drawn wherever it is,
        // so a scroll does not end the drag.
        if !in_view && pressed != Some(key_of(c)) {
            continue;
        }
        let part = match c {
            Column::Mixer(MixerColumn::Audio(s)) => {
                audio_parts(m, d, mixer, s, index, selected, has_vcas, now).map(ColumnParts::Audio)
            }
            Column::Mixer(MixerColumn::Vca(v)) => {
                vca_parts(m, mixer, v.id, index).map(ColumnParts::Vca)
            }
            Column::Mixer(MixerColumn::Midi(x)) => midi_parts(
                m,
                mixer,
                Some(x),
                m.midi_lane(d, x.id, p),
                m.is_midi_selected(d, x.id),
                index,
            )
            .map(ColumnParts::Midi),
            // The left pane's own MIDI strip is not marked: it shows by
            // focus.
            Column::Track { lane, dest } => {
                midi_parts(m, mixer, *dest, Some(lane), false, index).map(ColumnParts::Midi)
            }
        };
        strips.extend(part);
    }
    Gathered {
        grid,
        columns: all.len(),
        divider_before,
        scroll,
        shift,
        room,
        strips,
        keys: all.iter().map(key_of).collect(),
        renaming: m.renaming(),
        renaming_vca: m.renaming_vca(),
        pressed,
    }
}

/// Audio strip `s` (column `index`), as it shows.
#[allow(clippy::too_many_arguments)]
fn audio_parts(
    m: &MixerModel,
    d: &DocumentModel,
    mixer: &MixerRef,
    s: &StripRow,
    index: usize,
    selected: &std::collections::BTreeSet<u64>,
    has_vcas: bool,
    now: Instant,
) -> Option<StripParts> {
    let t = tokens();
    let master = s.kind == StripKindRow::Master;
    let fader = ControlBinding::fader(mixer, m, s.id)?;
    let level = NumberBinding::level(mixer, m, s.id)?;
    let mute = Toggle::mute(m, s.id)?;
    let lane = m.input_lane(d, s);
    let armable = lane.is_some();
    let monitor = lane.map_or(MonitorModeRow::Off, |l| l.monitor);
    let output = (!master).then(|| {
        let mut choices = vec![("Master".to_owned(), None)];
        choices.extend(m.targets(s).iter().map(|b| (b.name.clone(), Some(b.id))));
        (format!("\u{2192} {}", m.output_name(s)), choices)
    });
    let slot = |x: &SlotRow, place| slot_part(m, x, place);
    let sends = (!master).then(|| {
        (0..s.sends.len())
            .filter_map(|i| {
                let send = &s.sends[i];
                Some(SendPart {
                    index: i,
                    name: m.send_name(send),
                    enabled: send.enabled,
                    pre_fader: send.pre_fader,
                    knob: ControlBinding::send(mixer, m, s.id, i)?,
                    level: NumberBinding::send(mixer, m, s.id, i)?,
                })
            })
            .collect()
    });
    let group = m.group_of(s).map(|g| GroupTag {
        id: g.id,
        name: g.name.clone(),
        colour: t.swatch(g.colour),
        link_mute: g.link_mute,
        link_solo: g.link_solo,
    });
    let offset = has_vcas.then(|| OffsetParts {
        text: MixerModel::vca_offset_text(m.vca_offset_db(s)),
        mark: (!s.vcas.is_empty()).then(|| m.vca_mark(s)),
    });
    Some(StripParts {
        index,
        id: s.id,
        name: s.name.clone(),
        kind_label: s.kind_label.clone(),
        instrument: MixerModel::instrument_row(s).map(|r| match r {
            InstrumentRow::Plugin(x) => InstrumentPart::Plugin(slot(x, SlotPlace::Instrument)),
            InstrumentRow::BuiltinSynth => InstrumentPart::BuiltinSynth,
        }),
        eq: s.eq.as_ref().map(|x| slot(x, SlotPlace::Eq)),
        inserts: s
            .inserts
            .iter()
            .enumerate()
            .map(|(i, x)| slot(x, SlotPlace::Insert(i)))
            .collect(),
        sends,
        bar: t.bar(s),
        cap: t.cap(s.kind_colour),
        dimmed: s.solo_muted,
        selected: selected.contains(&s.id),
        fader,
        level,
        pan: if master {
            None
        } else {
            ControlBinding::pan(mixer, m, s.id).zip(NumberBinding::pan(mixer, m, s.id))
        },
        mute,
        solo: if master { None } else { Toggle::solo(m, s.id) },
        arm: Toggle {
            letter: "R",
            on: lane.is_some_and(|l| l.arm),
            colour: t.state_record,
            label: format!("Record arm {}", s.name),
            enabled: armable,
            touched: false,
        },
        monitor: Toggle {
            letter: "I",
            on: monitor != MonitorModeRow::Off,
            colour: if monitor == MonitorModeRow::Input {
                t.state_warning
            } else {
                t.state_play
            },
            label: format!("Input monitor {}", s.name),
            enabled: armable,
            touched: false,
        },
        input: lane.map(|l| input_parts(d, l)),
        output,
        mode: s.automation,
        meter: m.meters().level_at(s.id, now),
        group,
        offset,
    })
}

/// The strip input picker's help (Swift `StripInputView`): "Input of
/// <track>", then ", recording ⇠ <synth>" for a synth's return.
pub fn input_help(d: &DocumentModel, lane: &LaneRow) -> String {
    match d.input_badge(lane) {
        Some(b) => format!("Input of {}, recording {b}", lane.name),
        None => format!("Input of {}", lane.name),
    }
}

/// The input row of a strip whose input track is `lane` (Swift
/// `StripInputView`).
fn input_parts(d: &DocumentModel, lane: &LaneRow) -> InputParts {
    let help = LaneInputs::monitor_help(lane.monitor);
    InputParts {
        lane: lane.clone(),
        title: d.input_picker_title(lane),
        help: input_help(d, lane),
        monitor: Toggle {
            letter: LaneInputs::monitor_label(lane.monitor),
            on: lane.monitor != MonitorModeRow::Off,
            colour: if lane.monitor == MonitorModeRow::Input {
                tokens().state_warning
            } else {
                tokens().state_play
            },
            label: format!("{}: {help}", lane.name),
            enabled: true,
            touched: false,
        },
        monitor_help: help,
    }
}

/// VCA `vca` (column `index`), as it shows (Swift `VcaStripCells`).
fn vca_parts(m: &MixerModel, mixer: &MixerRef, vca: u64, index: usize) -> Option<VcaParts> {
    let t = tokens();
    let v = m.vca(vca)?;
    Some(VcaParts {
        index,
        id: v.id,
        name: v.name.clone(),
        title: VcaStripLabels::title(&v.name),
        kind_line: VcaStripLabels::kind_line(v.members.len()),
        colour: t.swatch(v.colour),
        fader: ControlBinding::vca(mixer, m, vca)?,
        level: NumberBinding::vca(mixer, m, vca)?,
        mute: Toggle {
            letter: "M",
            on: v.mute,
            colour: t.state_mute,
            label: VcaStripLabels::mute(&v.name),
            enabled: true,
            touched: false,
        },
        solo: Toggle {
            letter: "S",
            on: v.solo,
            colour: t.state_solo,
            label: VcaStripLabels::solo(&v.name),
            enabled: true,
            touched: false,
        },
        mode: v.automation,
    })
}

/// MIDI strip `x` (column `index`), as it shows (Swift
/// `MidiStripCells`): an external destination's, R arming `lane`; with no
/// destination (a MIDI track's in the left pane) "No Destination", its
/// pan, fader, volume and M off and R still arming the track.
fn midi_parts(
    m: &MixerModel,
    mixer: &MixerRef,
    x: Option<&DestRow>,
    lane: Option<&LaneRow>,
    selected: bool,
    index: usize,
) -> Option<MidiParts> {
    let t = tokens();
    let (name, mix) = match x {
        Some(x) => (x.name.clone(), Some(m.midi_mix(x.id)?)),
        None => (MidiStripMath::NO_DESTINATION.to_owned(), None),
    };
    let dest = x.map(|x| x.id);
    let off = |b: ControlBinding| ControlBinding {
        enabled: false,
        ..b
    };
    let off_box = |b: NumberBinding| NumberBinding {
        enabled: false,
        ..b
    };
    let (pan, pan_box, fader, volume) = match dest {
        Some(dest) => (
            ControlBinding::midi(mixer, m, dest, MidiControl::Pan)?,
            NumberBinding::midi_pan(mixer, m, dest)?,
            ControlBinding::midi(mixer, m, dest, MidiControl::Volume)?,
            NumberBinding::midi_volume(mixer, m, dest)?,
        ),
        None => {
            let grip = |control| ControlBinding {
                mixer: mixer.clone(),
                target: Grip::Midi {
                    dest: NO_DEST,
                    control,
                },
                label: String::new(),
                value: String::new(),
                position: 0.,
                enabled: false,
                touched: false,
            };
            let entry = |entry| NumberBinding {
                mixer: mixer.clone(),
                entry,
                label: String::new(),
                value: String::new(),
                enabled: false,
            };
            (
                off(ControlBinding {
                    label: MidiStripLabels::pan_knob(&name),
                    value: MidiStripMath::pan_label(None),
                    position: MidiStripMath::pan_position(None),
                    ..grip(MidiControl::Pan)
                }),
                off_box(NumberBinding {
                    label: MidiStripLabels::pan_box(&name),
                    value: MidiStripMath::pan_label(None),
                    ..entry(Entry::MidiPan { dest: NO_DEST })
                }),
                off(ControlBinding {
                    label: MidiStripLabels::fader(&name),
                    value: MidiStripMath::volume_label(None),
                    position: MidiStripMath::position(None),
                    ..grip(MidiControl::Volume)
                }),
                off_box(NumberBinding {
                    label: MidiStripLabels::volume_box(&name),
                    value: MidiStripMath::volume_label(None),
                    ..entry(Entry::MidiVolume { dest: NO_DEST })
                }),
            )
        }
    };
    let key = match (dest, lane) {
        (Some(dest), _) => ColumnKey::Dest(dest),
        (None, Some(l)) => ColumnKey::Track(l.id),
        (None, None) => return None,
    };
    Some(MidiParts {
        index,
        id: dest,
        key,
        kind_label: MidiStripMath::kind_label(
            x.and_then(|x| x.channel),
            x.and_then(|x| x.mpe.as_ref()),
        ),
        cap: t.cap(vizdaw_ffi::mixer::StripColourRow::Instrument),
        pan,
        pan_box,
        pan_set: mix.as_ref().is_some_and(|x| x.pan.is_some()),
        fader,
        volume,
        volume_set: mix.as_ref().is_some_and(|x| x.volume.is_some()),
        mute: Toggle {
            letter: "M",
            on: mix.as_ref().is_some_and(|x| x.mute),
            colour: t.state_mute,
            label: MidiStripLabels::mute(&name),
            enabled: dest.is_some(),
            touched: false,
        },
        arm: Toggle {
            letter: "R",
            on: lane.is_some_and(|l| l.arm),
            colour: t.state_record,
            label: MidiStripLabels::arm(lane.map_or(name.as_str(), |l| l.name.as_str())),
            enabled: lane.is_some(),
            touched: false,
        },
        lane: lane.map(|l| l.id),
        activity: dest.map_or(0., |dest| m.midi_activity(dest)),
        selected,
        name,
    })
}

/// Slot `x` at `place`, as its row shows it.
fn slot_part(m: &MixerModel, x: &SlotRow, place: SlotPlace) -> SlotPart {
    SlotPart {
        id: x.id,
        name: x.plugin.name.clone(),
        label: SlotLabels::slot(x, place),
        dot: dot(m.slot_state(x)),
        bypass: x.bypass,
        dim: m.slot_reason(x).is_some(),
    }
}

/// A slot's dot (Swift `SlotView.dot`): faulted, live, bypassed, loading
/// or a placeholder.
fn dot(state: SlotState) -> Rgba {
    let t = tokens();
    match state {
        SlotState::Faulted => rgb(t.state_error),
        SlotState::Live => rgb(t.state_play),
        SlotState::Bypassed => {
            let mut c = rgb(t.text_secondary);
            c.a = 0.5;
            c
        }
        SlotState::Loading => rgb(t.state_offline),
        SlotState::Placeholder => rgb(t.state_warning),
    }
}

/// The placement `view` draws: the bar (not in the left pane), then the
/// strips. `scope` is the mixer's own container.
pub fn render(
    view: &Entity<MixerView>,
    scope: &mut RegistryScope,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let (mixer, appeared) = {
        let v = view.read(cx);
        (v.mixer.clone(), v.appeared)
    };
    // The poll's clock (the test platform's is its own).
    let now = cx.background_executor().now();
    let doc = mixer.doc;
    let sample_rate = mixer
        .app
        .read(cx)
        .transport(doc)
        .map_or(0., |t| t.state().sample_rate);
    // A mixer that could not be made when the placement came: asked again
    // (at most every poll interval), and the placement registers then.
    if !appeared {
        view.update(cx, |v, cx| {
            if !v.refused(cx) && v.may_ask(cx) {
                v.appear(cx);
            }
        });
    }
    let stale = mixer.read(cx, |m, d| m.needs_refresh(d)).unwrap_or(false);
    if stale && view.update(cx, |v, cx| v.may_ask(cx)) {
        // Not while drawing: the next turn reads it again.
        let again = mixer.clone();
        cx.defer(move |cx| {
            again.with(cx, |m, d| m.refresh_if_needed(d));
        });
    }
    let Some(load) = mixer.read(cx, |m, _| m.load_text(sample_rate)) else {
        view.update(cx, |v, _| v.drawn.clear());
        // A busy core is quiet (asked again); a refusal says so.
        let refused = mixer
            .app
            .read(cx)
            .store
            .document(doc)
            .is_some_and(DocumentModel::mixer_refused);
        return unavailable(refused, scope);
    };
    let weak = view.downgrade();
    // The left pane's strip has no bar.
    let bar = (mixer.placement != MixerPlacement::Strip)
        .then(|| bar(&load, &mixer, &mut scope.container()));
    let area_scope = scope.container();
    let clip = area_scope.clip();
    let t = tokens();
    let dragging = weak.clone();
    // An insert's drag follows the pointer anywhere in the window: these
    // listeners are registered on every paint and see at the event whether
    // an insert is pressed (the mouse-up may come before the frame after
    // the press). They listen in the capture phase, so no element that
    // stops the event can keep the drop from them; and a press while a drag
    // is still open (its mouse-up was lost) ends that drag first, moving
    // nothing.
    let listeners = canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let pressed = dragging.clone();
            window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture {
                    pressed.update(cx, |v, cx| v.cancel_insert_drag(cx)).ok();
                }
            });
            let moved = dragging.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture {
                    moved.update(cx, |v, cx| v.drag_insert(e, cx)).ok();
                }
            });
            let dropped = dragging.clone();
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
                    dropped.update(cx, |v, cx| v.drop_insert(cx)).ok();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    // The strips are made once the area's bounds are known, in this frame:
    // they never fit an area measured a frame before (a status line that
    // came or went, a pane resized, a placement just made).
    let shown = view.clone();
    let strips = crate::fit::fitted(move |bounds, window, cx| {
        strips_in(&shown, bounds, area_scope, now, window, cx)
    });
    let wheel = weak.clone();
    let strips_area = div()
        .id("mixer-strips")
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .child(listeners)
        .child(strips)
        .child(clip)
        .on_scroll(move |e, _, cx| {
            wheel.update(cx, |v, cx| v.wheel(e, cx)).ok();
            cx.stop_propagation();
        });
    div()
        .relative()
        .flex()
        .flex_col()
        .size_full()
        .bg(rgb(t.surface_panel))
        .text_color(rgb(t.text_primary))
        .when_some(bar, |d, bar| {
            d.child(bar)
                .child(div().h(px(1.)).flex_none().bg(rgb(t.surface_separator)))
        })
        .child(strips_area)
        .into_any_element()
}

/// The strips of `view` for its area's `bounds` this frame (made while
/// the area is laid out): the columns in view and master's divider, as
/// large as the area.
fn strips_in(
    view: &Entity<MixerView>,
    bounds: Bounds<Pixels>,
    mut area_scope: RegistryScope,
    now: Instant,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let (mixer, scroll, area) = {
        let v = view.read(cx);
        (v.mixer.clone(), v.scroll, v.area.clone())
    };
    area.set(bounds);
    let size = (
        f64::from(f32::from(bounds.size.width)),
        f64::from(f32::from(bounds.size.height)),
    );
    let Some(g) = mixer.read(cx, |m, d| gather(m, d, &mixer, size, scroll, now)) else {
        return div().into_any_element();
    };
    let weak = view.downgrade();
    // Boxes of strips that went are dropped (an edit in one ends).
    view.update(cx, |v, cx| {
        v.scroll = g.scroll;
        v.room = g.room;
        let drawn_in = f64::from(f32::from(bounds.size.height));
        v.fits.push((g.grid.total + 2. * PADDING, drawn_in));
        if v.fits.len() > FITS_KEPT {
            v.fits.remove(0);
        }
        v.boxes.retain(|(key, _), _| g.keys.contains(key));
        v.drawn = g
            .strips
            .iter()
            .filter_map(|c| match c {
                ColumnParts::Audio(s) => Some(StripLook {
                    strip: s.id,
                    dimmed: s.dimmed,
                    selected: s.selected,
                    meter: s.meter,
                }),
                _ => None,
            })
            .collect();
        v.midi_drawn = g
            .strips
            .iter()
            .filter_map(|c| match c {
                ColumnParts::Midi(x) => x.id.map(|id| (id, x.activity)),
                _ => None,
            })
            .collect();
        if v.menu
            .is_some_and(|(id, which)| !g.keys.contains(&which.column(id)))
        {
            v.menu = None;
            v.submenu = None;
        }
        // The column a press holds went (the left pane's focus moved): its
        // drag ends, released once, not left touching.
        if g.pressed.is_some_and(|k| !g.keys.contains(&k)) {
            let gone = v.mixer.clone();
            cx.defer(move |cx| gone.cancel_drags(cx));
        }
        // The strip of a dragged insert went: the drag ends.
        if v.insert_drag
            .is_some_and(|d| !g.keys.contains(&ColumnKey::Strip(d.strip)))
        {
            v.insert_drag = None;
        }
        // The mixer ended the rename (a pane change committed it, the
        // strip or VCA went): the field goes.
        let p = v.mixer.placement;
        let still = |t: RenameTarget| match t {
            RenameTarget::Strip(s) => g.renaming == Some((s, p)),
            RenameTarget::Vca(x) => g.renaming_vca == Some((x, p)),
        };
        if let Some(r) = &v.rename
            && !still(r.target)
        {
            let w = weak.clone();
            cx.defer(move |cx| {
                w.update(cx, |v, cx| v.end_rename(false, cx)).ok();
            });
        }
    });
    let mut strips = vec![];
    let mut parts = g.strips.into_iter().peekable();
    for index in 0..g.columns {
        // Every column takes its place in the tree, drawn or not, so a
        // strip's controls keep their IDs as the strips scroll.
        let mut strip_scope = area_scope.container();
        let Some(c) = parts.next_if(|c| c.index() == index) else {
            continue;
        };
        let place = Place {
            grid: &g.grid,
            divider_before: g.divider_before,
            scroll: (g.scroll.0 - g.shift, g.scroll.1),
        };
        strips.push(match c {
            ColumnParts::Audio(s) => strip(view, s, &place, &mut strip_scope, window, cx),
            ColumnParts::Vca(v) => {
                crate::vca_strip::vca_strip(view, v, &place, &mut strip_scope, window, cx)
            }
            ColumnParts::Midi(x) => {
                crate::midi_strip::midi_strip(view, x, &place, &mut strip_scope, window, cx)
            }
        });
    }
    let t = tokens();
    let divider = g.divider_before.map(|d| {
        div()
            .absolute()
            .left(px(
                (PADDING + StripGrid::divider_x(d) + g.shift - g.scroll.0) as f32,
            ))
            .top(px((PADDING - g.scroll.1) as f32))
            .w(px(1.))
            .h(px(g.grid.total as f32))
            .bg(rgb(t.surface_separator))
    });
    div()
        .relative()
        .w(bounds.size.width)
        .h(bounds.size.height)
        .children(divider)
        .children(strips)
        .into_any_element()
}

/// The note when the mixer could not be read (`refused`; nothing while
/// a busy core is asked again).
fn unavailable(refused: bool, scope: &mut RegistryScope) -> AnyElement {
    let t = tokens();
    let mut note = div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .bg(rgb(t.surface_panel));
    if refused {
        note = note
            .child(
                div()
                    .relative()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text_primary))
                    .child(UNAVAILABLE)
                    .child(scope.control("text", UNAVAILABLE, true)),
            )
            .child(
                div()
                    .relative()
                    .text_xs()
                    .text_color(rgb(t.text_secondary))
                    .child(UNAVAILABLE_NOTE)
                    .child(scope.control("text", UNAVAILABLE_NOTE, true)),
            );
    }
    note.into_any_element()
}

/// The mixer's bar: the load, then Add Bus.
fn bar(load: &str, mixer: &MixerRef, scope: &mut RegistryScope) -> AnyElement {
    let t = tokens();
    let adding = mixer.clone();
    div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .justify_end()
        .gap(px(12.))
        .h(px(BAR_HEIGHT as f32))
        .px(px(8.))
        .text_xs()
        .child(
            div()
                .relative()
                .text_color(rgb(t.text_secondary))
                .whitespace_nowrap()
                .child(load.to_owned())
                .child(scope.control("text", load, true)),
        )
        .child(
            div()
                .id("mixer-add-bus")
                .relative()
                .h(px(22.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(t.radius))
                .border_1()
                .border_color(rgb(t.surface_separator))
                .bg(rgb(t.surface_inset))
                .cursor_pointer()
                .whitespace_nowrap()
                .child(ADD_BUS)
                .child(scope.control("button", ADD_BUS, true))
                .on_click(move |_, _, cx| {
                    adding.with(cx, |m, d| m.add_bus(d));
                }),
        )
        .into_any_element()
}

/// A part of a strip at (`x`, `y`) inside it, `w` × `h`.
pub(crate) fn at(x: f64, y: f64, w: f64, h: f64) -> gpui::Div {
    div()
        .absolute()
        .left(px(x as f32))
        .top(px(y as f32))
        .w(px(w as f32))
        .h(px(h as f32))
}

/// Where the columns go: the shared rows, the divider, the scroll.
pub(crate) struct Place<'a> {
    pub(crate) grid: &'a StripGrid,
    pub(crate) divider_before: Option<usize>,
    pub(crate) scroll: (f64, f64),
}

impl Place<'_> {
    /// Row `k`'s top and height, when a column has it.
    pub(crate) fn row(&self, k: StripRowKind) -> (Option<f64>, Option<f64>) {
        (
            self.grid.tops.get(&k).copied(),
            self.grid.rows.get(&k).copied(),
        )
    }

    /// Column `index`'s panel, in the scrolled area (half dimmed when
    /// `dimmed`).
    pub(crate) fn panel(&self, index: usize, dimmed: bool) -> gpui::Div {
        let t = tokens();
        let left = PADDING + StripGrid::x(index, self.divider_before) - self.scroll.0;
        let top = PADDING - self.scroll.1;
        let mut fill = rgb(t.surface_raised);
        if dimmed {
            fill.a = 0.5;
        }
        div()
            .absolute()
            .left(px(left as f32))
            .top(px(top as f32))
            .w(px(StripLayout::WIDTH as f32))
            .h(px(self.grid.total as f32))
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(PANEL_RADIUS))
                    .bg(fill),
            )
    }
}

/// A panel's edge, over every row: the accent when `selected`.
pub(crate) fn edge(selected: bool) -> gpui::Div {
    let t = tokens();
    let (edge, width) = if selected {
        (t.accent, 2.)
    } else {
        (t.surface_separator, 1.)
    };
    div()
        .absolute()
        .size_full()
        .rounded(px(PANEL_RADIUS))
        .border(px(width))
        .border_color(rgb(edge))
}

/// One strip, placed in the scrolled area.
fn strip(
    view: &Entity<MixerView>,
    s: StripParts,
    place: &Place<'_>,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let t = tokens();
    let mixer = view.read(cx).mixer.clone();
    let g = place.grid;
    let side = StripLayout::SIDE_PADDING;
    let inner = StripLayout::inner();
    let row = |k: StripRowKind| place.row(k);
    let mut out = place.panel(s.index, s.dimmed);
    // The header.
    if let (Some(y), Some(h)) = row(StripRowKind::Header) {
        out = out.child(header(view, &s, (side, y, inner, h), scope, window, cx));
    }
    // The instrument, the EQ, the inserts and the sends.
    if let (Some(part), (Some(y), Some(_))) = (&s.instrument, row(StripRowKind::Instrument)) {
        let el = match part {
            InstrumentPart::Plugin(x) => slot_holder(view, s.id, x, None, scope, window, cx),
            InstrumentPart::BuiltinSynth => builtin_synth_slot(view, scope, cx),
        };
        out = out.child(at(side, y, inner, RowHeights::SLOT).child(el));
    }
    if let (Some(part), (Some(y), Some(h))) = (&s.input, row(StripRowKind::Input)) {
        let el = input_row(view, s.id, part, scope, window, cx);
        out = out.child(at(side, y, inner, h.min(controls::menu_face::HEIGHT)).child(el));
    }
    if let (Some(y), Some(_)) = row(StripRowKind::Eq) {
        let el = eq_slot(view, &s, scope, window, cx);
        out = out.child(at(side, y, inner, RowHeights::EQ).child(el));
    }
    if let (Some(y), Some(h)) = row(StripRowKind::Inserts) {
        let el = inserts(view, &s, scope, window, cx);
        out = out.child(at(side, y, inner, h).child(el));
    }
    if let (Some(sends), (Some(y), Some(h))) = (&s.sends, row(StripRowKind::Sends)) {
        let el = sends_row(view, &s, sends, scope, window, cx);
        out = out.child(at(side, y, inner, h).child(el));
    }
    // The output menu.
    if let (Some((face, choices)), (Some(y), Some(h))) = (&s.output, row(StripRowKind::Output)) {
        let label = format!("Output of {}", s.name);
        let open = view.read(cx).menu == Some((s.id, StripMenu::Output));
        let w = view.downgrade();
        let id = s.id;
        let face_el = controls::menu_face(&label, face, None, true, scope, move |_, cx| {
            w.update(cx, |v, cx| v.toggle_menu(id, StripMenu::Output, cx))
                .ok();
        });
        let mut holder = at(side, y, inner, h.min(controls::menu_face::HEIGHT)).child(face_el);
        if open {
            let items = choices
                .iter()
                .map(|(title, bus)| {
                    let (mx, bus) = (mixer.clone(), *bus);
                    MenuEntry::item(title.clone(), move |_, cx| {
                        mx.with(cx, |m, d| m.set_output(d, id, bus));
                    })
                })
                .collect();
            holder = holder.children(popup(view, "mixer-output-menu", items, scope, window, cx));
        }
        out = out.child(holder);
    }
    // The pan.
    if let (Some((knob, text)), (Some(y), Some(h))) = (&s.pan, row(StripRowKind::Pan)) {
        let nb = view.update(cx, |v, cx| {
            v.number_box((ColumnKey::Strip(s.id), BoxKind::Pan), window, cx)
        });
        let knob_el = controls::knob(
            knob,
            KnobSize::Medium,
            t.knob(KnobFunction::Pan),
            0.5,
            scope,
            window,
            cx,
        );
        let box_el = controls::number_box(&nb, text.clone(), scope, cx);
        out = out.child(
            at(side, y, inner, h)
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(knob_el)
                .child(box_el),
        );
    }
    // The fader, its scale and the meter, centred.
    let throw = g.throw;
    let fader_el = controls::fader(&s.fader, throw, s.cap, scope, window, cx);
    let meter_el = controls::meter(s.meter, throw + 2. * StripLayout::METER_OVERHANG, window);
    out = out.child(
        at(side, g.fader_top, inner, throw + StripLayout::FADER_CAP)
            .flex()
            .flex_row()
            .justify_center()
            .items_start()
            .gap(px(4.))
            .child(fader_el)
            .child(
                div()
                    .pt(px((fader::CAP.1 / 2. - StripLayout::METER_OVERHANG) as f32))
                    .child(meter_el),
            ),
    );
    // The level.
    let level_box = view.update(cx, |v, cx| {
        v.number_box((ColumnKey::Strip(s.id), BoxKind::Level), window, cx)
    });
    let level_el = controls::number_box(&level_box, s.level.clone(), scope, cx);
    out = out.child(
        at(side, g.level_top, inner, StripLayout::LEVEL_BOX)
            .flex()
            .justify_center()
            .child(level_el),
    );
    // The VCA offset and mark, while the mixer has a VCA.
    if let Some(o) = &s.offset {
        out =
            out.child(at(side, g.offset_top, inner, g.offset).child(vca_offset(&s.name, o, scope)));
    }
    // M S R I; master keeps the solo's place blank.
    let strip_id = s.id;
    let mut buttons = at(side, g.buttons_top, inner, StripLayout::TOGGLE_ROW)
        .flex()
        .flex_row()
        .justify_center()
        .gap(px(4.))
        .child(controls::toggle(
            s.mute.clone(),
            scope,
            controls::toggle::toggle_mute(&mixer, strip_id),
        ));
    buttons = match &s.solo {
        Some(solo) => buttons.child(controls::toggle(
            solo.clone(),
            scope,
            controls::toggle::toggle_solo(&mixer, strip_id),
        )),
        None => buttons.child(
            div()
                .flex_none()
                .w(px(controls::toggle::SIDE as f32))
                .h(px(controls::toggle::SIDE as f32)),
        ),
    };
    let (armed, monitored) = (mixer.clone(), mixer.clone());
    buttons = buttons
        .child(controls::toggle(s.arm.clone(), scope, move |_, cx| {
            armed.with(cx, |m, d| m.toggle_arm(d, strip_id));
        }))
        .child(controls::toggle(s.monitor.clone(), scope, move |_, cx| {
            monitored.with(cx, |m, d| m.cycle_monitor(d, strip_id));
        }));
    out = out.child(buttons);
    // The automation mode.
    let mode_label = format!("Automation of {}", s.name);
    let open = view.read(cx).menu == Some((s.id, StripMenu::Automation));
    let w = view.downgrade();
    let face = controls::menu_face(
        &mode_label,
        AutomationModes::title(s.mode),
        Some(t.automation(s.mode)),
        true,
        scope,
        move |_, cx| {
            w.update(cx, |v, cx| {
                v.toggle_menu(strip_id, StripMenu::Automation, cx)
            })
            .ok();
        },
    );
    let mut mode = at(side, g.mode_top, inner, StripLayout::MODE_MENU).child(face);
    if open {
        let items = AutomationModes::ALL
            .iter()
            .map(|&m| {
                let mx = mixer.clone();
                MenuEntry::item(AutomationModes::title(m), move |_, cx| {
                    mx.with(cx, |mm, d| mm.set_mode(d, strip_id, m));
                })
                .checked(m == s.mode)
            })
            .collect();
        mode = mode.children(popup(view, "mixer-mode-menu", items, scope, window, cx));
    }
    out = out.child(mode);
    out.child(edge(s.selected)).into_any_element()
}

/// A strip's VCA offset row (Swift `StripVcaOffset`): "VCA −3.0" while its
/// VCAs change what is heard, and the "VCA" mark, lit while a VCA mutes
/// or solos it.
fn vca_offset(name: &str, o: &OffsetParts, scope: &mut RegistryScope) -> AnyElement {
    let t = tokens();
    let mut row = div()
        .size_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .text_size(px(9.));
    if let Some(text) = &o.text {
        row = row.child(
            div()
                .relative()
                .whitespace_nowrap()
                .text_color(rgb(t.text_secondary))
                .child(text.clone())
                .child(scope.control("text", &VcaStripLabels::offset(text), true)),
        );
    }
    if let Some(lit) = o.mark {
        let (fill, legend) = if lit {
            (rgb(t.state_mute), t.legend_on(t.state_mute))
        } else {
            let mut off = rgb(t.text_secondary);
            off.a = 0.5;
            (rgb(t.surface_inset), off)
        };
        row = row.child(
            div()
                .relative()
                .px(px(2.))
                .rounded(px(t.radius))
                .bg(fill)
                .font_weight(FontWeight::BOLD)
                .text_color(legend)
                .child(VcaStripLabels::MARK_TEXT)
                .child(scope.control_with(
                    "text",
                    &VcaStripLabels::mark(name),
                    true,
                    Some(if lit { "on" } else { "off" }),
                    false,
                )),
        );
    }
    row.into_any_element()
}

/// The header: the colour bar, the name (or its rename field) and the
/// kind. A click on the name selects the strip (⌘ or ⇧ adds or removes
/// it); a double-click renames it.
fn header(
    view: &Entity<MixerView>,
    s: &StripParts,
    (x, y, w, h): (f64, f64, f64, f64),
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let t = tokens();
    let field = view
        .read(cx)
        .rename
        .as_ref()
        .filter(|r| r.target == RenameTarget::Strip(s.id))
        .map(|r| r.field.clone());
    let name: AnyElement = match field {
        Some(field) => {
            let text = field.read(cx).text().to_owned();
            let focused = field.focus_handle(cx).is_focused(window);
            div()
                .relative()
                .size_full()
                .child(field)
                .child(scope.control_with("textfield", NAME, true, Some(&text), focused))
                .into_any_element()
        }
        None => {
            let id = s.id;
            let weak: WeakEntity<MixerView> = view.downgrade();
            div()
                .id(gpui::ElementId::Name(
                    format!("mixer-strip-name:{id}").into(),
                ))
                .relative()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_primary))
                .child(s.name.clone())
                .child(scope.control("text", &s.name, true))
                .on_mouse_down(MouseButton::Right, {
                    let weak = weak.clone();
                    move |_, _, cx| {
                        weak.update(cx, |v, cx| v.open_menu(id, StripMenu::Strip, cx))
                            .ok();
                        cx.stop_propagation();
                    }
                })
                .on_click(move |e: &ClickEvent, window, cx| {
                    let Some(v) = weak.upgrade() else { return };
                    if e.click_count() >= 2 {
                        v.update(cx, |v, cx| v.begin_rename(id, window, cx));
                        return;
                    }
                    let mods = e.modifiers();
                    let adding = mods.secondary(cx) || mods.shift;
                    let mixer = v.read(cx).mixer.clone();
                    mixer.with(cx, |m, d| m.select(d, id, adding));
                })
                .into_any_element()
        }
    };
    let mut name_holder = at(0., NAME_TOP, w, NAME_HEIGHT).child(name);
    if view.read(cx).menu == Some((s.id, StripMenu::Strip)) {
        let weak = view.downgrade();
        let items = mixer_entries(view, cx, |mx, m, d| strip_entries(&weak, mx, m, d, s.id));
        name_holder =
            name_holder.children(popup(view, "mixer-strip-menu", items, scope, window, cx));
    }
    let tag = s
        .group
        .as_ref()
        .map(|g| group_tag(view, s.id, g, w, scope, window, cx));
    at(x, y, w, h)
        // Show channel colour bars (Swift `appearancePreferences.colourBars`).
        .children(crate::appearance::colour_bars().then(|| {
            at(0., 0., w, StripLayout::COLOUR_BAR)
                .rounded(px(t.radius))
                .bg(s.bar)
        }))
        .child(name_holder)
        .children(tag)
        .child(
            at(0., KIND_TOP, w, KIND_HEIGHT)
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(10.))
                .text_color(rgb(t.text_secondary))
                .child(s.kind_label.clone())
                .child(scope.control("text", &s.kind_label, true)),
        )
        .into_any_element()
}

/// A grouped strip's group tag under its kind (Swift `StripGroupTag`):
/// the group's colour and name, `menu` "Group <name>"; its menu holds Link
/// Mutes and Link Solos (checked while on), Remove from Group (this
/// strip) and Ungroup.
fn group_tag(
    view: &Entity<MixerView>,
    strip: u64,
    g: &GroupTag,
    w: f64,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let label = VcaStripLabels::group(&g.name);
    let opening = view.downgrade();
    let face = controls::menu_face(
        &label,
        &g.name,
        Some(g.colour),
        true,
        scope,
        move |_, cx| {
            opening
                .update(cx, |v, cx| v.toggle_menu(strip, StripMenu::GroupTag, cx))
                .ok();
        },
    );
    let top = RowHeights::HEADER + RowHeights::PART_GAP;
    let mut holder = at(0., top, w, RowHeights::GROUP_TAG)
        .id(gpui::ElementId::Name(
            format!("mixer-group-tag:{strip}").into(),
        ))
        .child(face);
    if view.read(cx).menu == Some((strip, StripMenu::GroupTag)) {
        let mixer = view.read(cx).mixer.clone();
        let (group, mute, solo) = (g.id, g.link_mute, g.link_solo);
        let (a, b, c, e) = (mixer.clone(), mixer.clone(), mixer.clone(), mixer);
        let items = vec![
            MenuEntry::item(GroupLabels::LINK_MUTES, move |_, cx| {
                a.with(cx, |m, d| m.set_group_links(d, group, Some(!mute), None));
            })
            .checked(mute),
            MenuEntry::item(GroupLabels::LINK_SOLOS, move |_, cx| {
                b.with(cx, |m, d| m.set_group_links(d, group, None, Some(!solo)));
            })
            .checked(solo),
            MenuEntry::Separator,
            MenuEntry::item(GroupLabels::REMOVE, move |_, cx| {
                c.with(cx, |m, d| m.remove_from_group(d, group, &[strip]));
            }),
            MenuEntry::item(GroupLabels::UNGROUP, move |_, cx| {
                e.with(cx, |m, d| m.delete_group(d, group));
            }),
        ];
        holder = holder.children(popup(view, "mixer-group-menu", items, scope, window, cx));
    }
    holder.into_any_element()
}

/// A strip section's small heading (Swift `StripHeading`; not in the
/// tree, as Swift hides it from VoiceOver).
fn heading(text: &str) -> gpui::Div {
    let t = tokens();
    div()
        .h(px(RowHeights::HEADING as f32))
        .flex()
        .items_center()
        .text_size(px(9.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(t.text_secondary))
        .child(text.to_uppercase())
}

/// A slot row's face (Swift `SlotView`): its dot, then the plugin's name
/// (struck through while bypassed, dimmed while it passes audio through).
fn slot_face(dot: Rgba, name: &str, bypass: bool, dim: bool) -> gpui::Div {
    let t = tokens();
    let mut text = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(10.))
        .text_color(rgb(if dim {
            t.text_secondary
        } else {
            t.text_primary
        }))
        .child(name.to_owned());
    if bypass {
        text = text.line_through();
    }
    div()
        .size_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .px(px(4.))
        .rounded(px(t.radius))
        .bg(rgb(t.surface_inset))
        .border_1()
        .border_color(rgb(t.surface_separator))
        .child(div().flex_none().size(px(6.)).rounded_full().bg(dot))
        .child(text)
}

/// The open menu `which` of `strip`, if it is this one: its entries.
fn open_entries(
    view: &Entity<MixerView>,
    strip: u64,
    which: StripMenu,
    cx: &App,
    f: impl FnOnce(&MixerRef, &MixerModel, &DocumentModel) -> Vec<MenuEntry>,
) -> Option<Vec<MenuEntry>> {
    (view.read(cx).menu == Some((strip, which))).then(|| mixer_entries(view, cx, f))
}

/// A slot of `strip` (the instrument's, or insert `place` of `count`):
/// `button` in the tree, its label the plugin ("<plugin>, instrument" for
/// an instrument). A right-click opens its menu; a double-click opens its
/// plug-in window (an insert's arms no drag); an insert's single press is
/// pressed to be dragged. An instrument's single click (the instrument
/// menu) comes with the catalogue.
fn slot_holder(
    view: &Entity<MixerView>,
    strip: u64,
    x: &SlotPart,
    insert: Option<(usize, usize)>,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let slot = x.id;
    let menu = view.downgrade();
    let mut el = div()
        .id(gpui::ElementId::Name(format!("mixer-slot:{slot}").into()))
        .relative()
        .size_full()
        .child(slot_face(x.dot, &x.name, x.bypass, x.dim))
        .child(scope.control("button", &x.label, true))
        .on_mouse_down(MouseButton::Right, move |_, _, cx| {
            menu.update(cx, |v, cx| v.open_menu(strip, StripMenu::Slot(slot), cx))
                .ok();
            cx.stop_propagation();
        });
    let doc = view.read(cx).mixer.doc;
    let pressed = view.downgrade();
    el = el.on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, _, cx| {
        if e.click_count >= 2 {
            // The window, not a drag: the first click's up ended its press.
            pressed.update(cx, |v, _| v.insert_drag = None).ok();
            plugin_windows::show_later(doc, Subject::Plugin(slot), cx);
            cx.stop_propagation();
        } else if let Some((place, count)) = insert {
            pressed
                .update(cx, |v, _| {
                    v.press_insert(strip, slot, place, count, e.position.y)
                })
                .ok();
            cx.stop_propagation();
        }
    });
    let mut holder = div().relative().size_full().child(el);
    if let Some(items) = open_entries(view, strip, StripMenu::Slot(slot), cx, |mx, m, _| {
        slot_entries(mx, m, slot)
    }) {
        holder = holder.children(popup(view, "mixer-slot-menu", items, scope, window, cx));
    }
    holder.into_any_element()
}

/// The Built-in Synth's slot (Swift `BuiltinSynthSlotView`): a
/// double-click opens the sound window of the Built-in Synth the focused
/// track plays (else the first). Its click (the instrument menu) comes
/// with the catalogue.
fn builtin_synth_slot(view: &Entity<MixerView>, scope: &mut RegistryScope, cx: &App) -> AnyElement {
    let t = tokens();
    let mixer = view.read(cx).mixer.clone();
    div()
        .relative()
        .size_full()
        .child(slot_face(
            rgb(t.state_play),
            SlotLabels::BUILTIN_SYNTH,
            false,
            false,
        ))
        .child(scope.control("button", SlotLabels::BUILTIN_SYNTH_LABEL, true))
        .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, _, cx| {
            if e.click_count < 2 {
                return;
            }
            let dest = mixer
                .read(cx, |m, d| {
                    let focus = d.focus_lanes().first().and_then(|l| l.midi_dest);
                    builtin_synth_destination(m.destinations(), focus)
                })
                .flatten();
            if let Some(dest) = dest {
                plugin_windows::show_later(mixer.doc, Subject::BuiltinSynth(dest), cx);
            }
            cx.stop_propagation();
        })
        .into_any_element()
}

/// A dashed add face ("EQ ＋", the inserts' "+"), disabled: the plugin
/// catalogue comes later.
fn add_face(text: &str, label: &str, value: Option<&str>, scope: &mut RegistryScope) -> AnyElement {
    let t = tokens();
    div()
        .relative()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(t.radius))
        .border_1()
        .border_dashed()
        .border_color(rgb(t.surface_separator))
        .opacity(0.45)
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(t.text_secondary))
        .child(text.to_owned())
        .child(scope.control_with("button", label, false, value, false))
        .into_any_element()
}

/// The EQ slot (Swift `EqSlotView`): empty, the disabled "EQ ＋" (`EQ of
/// <strip>`, value `empty`); loaded, a `group` `EQ of <strip>` valued
/// with its plugin: the dot, the name and the bypass button. A click on
/// it (not its bypass) opens its plug-in window; a right-click opens its
/// menu.
fn eq_slot(
    view: &Entity<MixerView>,
    s: &StripParts,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let label = SlotLabels::eq(&s.name);
    let Some(eq) = &s.eq else {
        return add_face(
            SlotLabels::EQ_ADD_FACE,
            &label,
            Some(SlotLabels::EQ_EMPTY),
            scope,
        );
    };
    let t = tokens();
    let (strip, slot) = (s.id, eq.id);
    let mut group = scope.container();
    let bypassing = view.read(cx).mixer.clone();
    let bypass_label = SlotLabels::eq_bypass(&s.name, eq.bypass);
    let bypass = div()
        .id(gpui::ElementId::Name(
            format!("mixer-eq-bypass:{slot}").into(),
        ))
        .relative()
        .flex_none()
        .w(px(controls::toggle::SIDE as f32))
        .h_full()
        .flex()
        .flex_row()
        .items_end()
        .justify_center()
        .gap(px(1.))
        .pb(px(7.))
        .cursor_pointer()
        .children([5., 9., 6.].map(|h: f32| {
            div().w(px(2.)).h(px(h)).bg(rgb(if eq.bypass {
                t.text_secondary
            } else {
                t.state_play
            }))
        }))
        .child(group.control("button", &bypass_label, true))
        .on_click(move |_, _, cx| {
            bypassing.with(cx, |m, d| m.toggle_bypass(d, slot));
            cx.stop_propagation();
        });
    let doc = view.read(cx).mixer.doc;
    let menu = view.downgrade();
    let mut holder = div()
        .relative()
        .size_full()
        .flex()
        .flex_row()
        .items_center()
        .rounded(px(t.radius))
        .bg(rgb(t.surface_inset))
        .child(
            div()
                .id(gpui::ElementId::Name(
                    format!("mixer-eq-face:{slot}").into(),
                ))
                .flex_1()
                .min_w_0()
                .h(px(RowHeights::SLOT as f32))
                .child(slot_face(eq.dot, &eq.name, eq.bypass, eq.dim).border_0())
                .on_click(move |_, _, cx| {
                    plugin_windows::show_later(doc, Subject::Plugin(slot), cx);
                }),
        )
        .child(bypass)
        .child(scope.control_with("group", &label, true, Some(&eq.name), false))
        .id(gpui::ElementId::Name(format!("mixer-eq:{strip}").into()))
        .on_mouse_down(MouseButton::Right, move |_, _, cx| {
            menu.update(cx, |v, cx| v.open_menu(strip, StripMenu::Slot(slot), cx))
                .ok();
            cx.stop_propagation();
        });
    if let Some(items) = open_entries(view, strip, StripMenu::Slot(slot), cx, |mx, m, _| {
        slot_entries(mx, m, slot)
    }) {
        holder = holder.children(popup(view, "mixer-slot-menu", items, scope, window, cx));
    }
    holder.into_any_element()
}

/// The inserts (Swift `StripInserts`): the heading, each slot (dragged
/// to reorder: the place it would drop at is marked), then the disabled
/// "+".
fn inserts(
    view: &Entity<MixerView>,
    s: &StripParts,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let t = tokens();
    let gap = RowHeights::PART_GAP;
    let count = s.inserts.len();
    let drop = view
        .read(cx)
        .insert_drag
        .filter(|d| d.strip == s.id)
        .and_then(|d| d.over.map(|to| (d.from, to)));
    let mut out = div()
        .relative()
        .size_full()
        .child(heading(SlotLabels::INSERTS));
    let top = |i: usize| RowHeights::HEADING + gap + i as f64 * (RowHeights::SLOT + gap);
    for (i, x) in s.inserts.iter().enumerate() {
        let el = slot_holder(view, s.id, x, Some((i, count)), scope, window, cx);
        out = out.child(at(0., top(i), StripLayout::inner(), RowHeights::SLOT).child(el));
    }
    if let Some((from, to)) = drop.filter(|(from, to)| from != to) {
        // Above the slot it moves before, below the one it moves after.
        let y = if to < from {
            top(to) - gap / 2.
        } else {
            top(to) + RowHeights::SLOT + gap / 2.
        };
        out = out.child(
            at(0., y - 1., StripLayout::inner(), 2.)
                .rounded(px(1.))
                .bg(rgb(t.accent)),
        );
    }
    out.child(
        at(0., top(count), StripLayout::inner(), RowHeights::ADD).child(add_face(
            SlotLabels::ADD_FACE,
            &SlotLabels::add_effect(&s.name),
            None,
            scope,
        )),
    )
    .into_any_element()
}

/// The sends (Swift `SendKnobRow`, `SendKnob`): the heading; each send's
/// bus (struck through while off) and PRE chip over its small knob and
/// level box, a right-click opening its menu; then the "Send" menu and,
/// while the strip has no sends, New Bus's plus.
fn sends_row(
    view: &Entity<MixerView>,
    s: &StripParts,
    sends: &[SendPart],
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let t = tokens();
    let mixer = view.read(cx).mixer.clone();
    let gap = RowHeights::PART_GAP;
    let inner = StripLayout::inner();
    let strip = s.id;
    let mut out = div()
        .relative()
        .size_full()
        .child(heading(SendLabels::HEADING));
    let top = |i: usize| RowHeights::HEADING + gap + i as f64 * (RowHeights::SEND + gap);
    for send in sends {
        let i = send.index;
        let mut name = div()
            .relative()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_size(px(10.))
            .text_color(rgb(if send.enabled {
                t.text_primary
            } else {
                t.text_secondary
            }))
            .child(send.name.clone())
            .child(scope.control("text", &send.name, true));
        if !send.enabled {
            name = name.line_through();
        }
        let chip = send.pre_fader.then(|| {
            div()
                .relative()
                .flex_none()
                .px(px(4.))
                .rounded(px(t.radius))
                .bg(rgb(t.control_send))
                .text_size(px(9.))
                .font_weight(FontWeight::BOLD)
                .text_color(t.legend_on(t.control_send))
                .child(SendLabels::PRE_CHIP)
                .child(scope.control("text", SendLabels::PRE_CHIP, true))
        });
        let line = div()
            .h(px(RowHeights::SEND_NAME as f32))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .child(name)
            .children(chip);
        let nb = view.update(cx, |v, cx| {
            v.number_box((ColumnKey::Strip(strip), BoxKind::Send(i)), window, cx)
        });
        let knob = controls::knob(
            &send.knob,
            KnobSize::Small,
            t.knob(KnobFunction::Send),
            0.,
            scope,
            window,
            cx,
        );
        let level = controls::number_box(&nb, send.level.clone(), scope, cx);
        let knobs = div()
            .h(px(RowHeights::SEND_KNOB as f32))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .opacity(if send.enabled { 1. } else { 0.45 })
                    .child(knob),
            )
            .child(level);
        let menu = view.downgrade();
        let mut block = at(0., top(i), inner, RowHeights::SEND)
            .id(gpui::ElementId::Name(
                format!("mixer-send:{strip}:{i}").into(),
            ))
            .flex()
            .flex_col()
            .gap(px(gap as f32))
            .child(line)
            .child(knobs)
            .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                menu.update(cx, |v, cx| v.open_menu(strip, StripMenu::Send(i), cx))
                    .ok();
                cx.stop_propagation();
            });
        if view.read(cx).menu == Some((strip, StripMenu::Send(i))) {
            let items = send_entries(&mixer, strip, send);
            block = block.children(popup(view, "mixer-send-menu", items, scope, window, cx));
        }
        out = out.child(block);
    }
    // The Send menu and, with no sends yet, the plus.
    let label = SendLabels::add_menu(&s.name);
    let w = view.downgrade();
    let face = controls::menu_face(
        &label,
        SendLabels::MENU_TEXT,
        None,
        true,
        scope,
        move |_, cx| {
            w.update(cx, |v, cx| v.toggle_menu(strip, StripMenu::AddSend, cx))
                .ok();
        },
    );
    let mut menu = div().relative().flex_1().min_w_0().child(face);
    if let Some(items) = open_entries(view, strip, StripMenu::AddSend, cx, |mx, m, _| {
        add_send_entries(mx, m, strip)
    }) {
        menu = menu.children(popup(view, "mixer-send-add-menu", items, scope, window, cx));
    }
    let plus = sends.is_empty().then(|| {
        let mx = mixer.clone();
        div()
            .id(gpui::ElementId::Name(
                format!("mixer-new-bus:{strip}").into(),
            ))
            .relative()
            .flex_none()
            .size(px(RowHeights::ADD as f32))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(rgb(t.text_secondary))
            .child("+")
            .child(scope.control("button", &SendLabels::new_bus_plus(&s.name), true))
            .on_click(move |_, _, cx| {
                mx.with(cx, |m, d| m.new_bus(d, strip));
            })
    });
    out.child(
        at(0., top(sends.len()), inner, RowHeights::ADD)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .child(menu)
            .children(plus),
    )
    .into_any_element()
}

/// A strip's input row (Swift `StripInputView`): the picker, its face
/// the input (led by the track's name when the path is shared), then the
/// monitor button. The picker reads the sources again as it opens.
fn input_row(
    view: &Entity<MixerView>,
    strip: u64,
    part: &InputParts,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let mixer = view.read(cx).mixer.clone();
    let lane = part.lane.id;
    let open = view.read(cx).menu == Some((strip, StripMenu::Input));
    let label = format!("Input of {}", part.lane.name);
    let w = view.downgrade();
    let refresh = mixer.clone();
    let face = controls::menu_face(&label, &part.title, None, true, scope, move |_, cx| {
        let opening = w
            .update(cx, |v, cx| {
                v.toggle_menu(strip, StripMenu::Input, cx);
                v.menu.is_some()
            })
            .unwrap_or(false);
        if opening {
            let doc = refresh.doc;
            refresh.app.update(cx, |s, cx| {
                s.store.refresh_inputs(doc);
                cx.notify();
            });
        }
    });
    let mut picker = crate::sheet_host::with_help(
        div()
            .id(("strip-input", strip))
            .relative()
            .flex_1()
            .min_w_0()
            .child(face),
        part.help.clone(),
    );
    if open {
        let rows = mixer
            .read(cx, |_, d| {
                d.lanes()
                    .iter()
                    .find(|l| l.id == lane)
                    .map(|l| input_picker::rows(d.inputs().groups(l), l.input.as_ref()))
            })
            .flatten()
            .unwrap_or_default();
        let closing = view.downgrade();
        let close: input_picker::Close = Rc::new(move |_, cx| {
            closing.update(cx, |v, cx| v.close_menu(cx)).ok();
        });
        let chooser = mixer.clone();
        let choose: input_picker::Choose = Rc::new(move |c: &InputChoice, _, cx| {
            let c = c.clone();
            let doc = chooser.doc;
            chooser.app.update(cx, |s, cx| {
                s.store.with_document(doc, |d, _| {
                    if let Some(l) = d.lanes().iter().find(|l| l.id == lane).cloned() {
                        d.choose_input(&c, &l);
                    }
                });
                cx.notify();
            });
        });
        picker = picker.children(input_picker::popup(
            "strip-input-picker",
            rows,
            scope,
            window,
            close,
            choose,
        ));
    }
    let cycler = mixer.clone();
    let monitor = crate::sheet_host::with_help(
        div()
            .id(("strip-input-monitor", strip))
            .flex_none()
            .child(controls::toggle(
                part.monitor.clone(),
                scope,
                move |_, cx| {
                    let doc = cycler.doc;
                    cycler.app.update(cx, |s, cx| {
                        s.store.with_document(doc, |d, _| {
                            if let Some(l) = d.lanes().iter().find(|l| l.id == lane).cloned() {
                                d.cycle_lane_monitor(&l);
                            }
                        });
                        cx.notify();
                    });
                },
            )),
        part.monitor_help,
    );
    div()
        .size_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .child(picker)
        .child(monitor)
        .into_any_element()
}

/// What a menu item does when picked (the menu has closed).
pub(crate) type Pick = Box<dyn Fn(&mut Window, &mut App)>;

/// One row of a strip menu.
pub(crate) enum MenuEntry {
    Item {
        title: String,
        /// The current one (checked).
        checked: bool,
        enabled: bool,
        /// A colour swatch before the title (Colour ▸).
        swatch: Option<Rgba>,
        pick: Pick,
    },
    Separator,
    /// A submenu, opened by a click or the pointer over it.
    Submenu {
        title: String,
        enabled: bool,
        items: Vec<MenuEntry>,
    },
}

impl MenuEntry {
    pub(crate) fn item(
        title: impl Into<String>,
        pick: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        MenuEntry::Item {
            title: title.into(),
            checked: false,
            enabled: true,
            swatch: None,
            pick: Box::new(pick),
        }
    }

    /// An item that waits for a later slice: shown, disabled.
    fn waiting(title: impl Into<String>) -> Self {
        Self::item(title, |_, _| {}).enabled(false)
    }

    fn submenu(title: impl Into<String>, enabled: bool, items: Vec<MenuEntry>) -> Self {
        MenuEntry::Submenu {
            title: title.into(),
            enabled,
            items,
        }
    }

    pub(crate) fn checked(mut self, on: bool) -> Self {
        if let MenuEntry::Item { checked, .. } = &mut self {
            *checked = on;
        }
        self
    }

    fn enabled(mut self, on: bool) -> Self {
        match &mut self {
            MenuEntry::Item { enabled, .. } | MenuEntry::Submenu { enabled, .. } => *enabled = on,
            MenuEntry::Separator => {}
        }
        self
    }

    fn swatch(mut self, colour: Rgba) -> Self {
        if let MenuEntry::Item { swatch, .. } = &mut self {
            *swatch = Some(colour);
        }
        self
    }
}

/// The entries `f` builds from the mixer (none without one).
fn mixer_entries(
    view: &Entity<MixerView>,
    cx: &App,
    f: impl FnOnce(&MixerRef, &MixerModel, &DocumentModel) -> Vec<MenuEntry>,
) -> Vec<MenuEntry> {
    let mixer = view.read(cx).mixer.clone();
    mixer.read(cx, |m, d| f(&mixer, m, d)).unwrap_or_default()
}

/// The strip name's menu (Swift `StripHeader.menu`, `StripGroupItems`,
/// `StripColourMenu`): Rename…, Set All Strips to ▸, Remove Bus (a bus),
/// Colour ▸ (None and the eight colours), then, but on master, Group,
/// Assign to VCA ▸ (each VCA, then New VCA) and Remove from VCA ▸, on
/// the selection when it holds the strip, else on the strip. MIDI Learn
/// Maps… comes with the surfaces.
fn strip_entries(
    view: &WeakEntity<MixerView>,
    mixer: &MixerRef,
    m: &MixerModel,
    d: &DocumentModel,
    strip: u64,
) -> Vec<MenuEntry> {
    let Some(s) = m.strip(strip) else {
        return vec![];
    };
    let t = tokens();
    let renaming = view.clone();
    let mut out = vec![MenuEntry::item("Rename\u{2026}", move |window, cx| {
        renaming
            .update(cx, |v, cx| v.begin_rename(strip, window, cx))
            .ok();
    })];
    let modes = AutomationModes::ALL
        .iter()
        .map(|&mode| {
            let mx = mixer.clone();
            MenuEntry::item(AutomationModes::title(mode), move |_, cx| {
                mx.with(cx, |m, d| m.set_all_modes(d, mode));
            })
        })
        .collect();
    out.push(MenuEntry::submenu("Set All Strips to", true, modes));
    if s.kind == StripKindRow::Bus {
        let mx = mixer.clone();
        out.push(MenuEntry::item("Remove Bus", move |_, cx| {
            mx.with(cx, |m, d| m.remove_bus(d, strip));
        }));
    }
    let colour = |c: Option<SwatchRow>| {
        let mx = mixer.clone();
        let title = c.map_or(StripPalette::NO_COLOUR_TITLE, StripPalette::title);
        let item = MenuEntry::item(title, move |_, cx| {
            mx.with(cx, |m, d| m.set_colour(d, strip, c));
        })
        .checked(s.colour == c);
        match c {
            Some(c) => item.swatch(t.swatch(c)),
            None => item,
        }
    };
    let colours = std::iter::once(None)
        .chain(StripPalette::SWATCHES.map(Some))
        .map(colour)
        .collect();
    out.push(MenuEntry::submenu("Colour", true, colours));
    if s.kind == StripKindRow::Master {
        return out;
    }
    let strips = m.menu_strips(d, strip);
    out.push(MenuEntry::Separator);
    let (mx, grouped) = (mixer.clone(), strips.clone());
    out.push(
        MenuEntry::item("Group", move |_, cx| {
            mx.with(cx, |m, d| m.new_group(d, &grouped));
        })
        .enabled(m.can_group(&strips)),
    );
    let mut assign: Vec<MenuEntry> = m
        .vcas()
        .iter()
        .map(|v| {
            let add = m.assignable(&strips, v);
            let (mx, vca, enabled) = (mixer.clone(), v.id, !add.is_empty());
            MenuEntry::item(v.name.clone(), move |_, cx| {
                mx.with(cx, |m, d| m.assign(d, &add, vca, true));
            })
            .enabled(enabled)
        })
        .collect();
    if !m.vcas().is_empty() {
        assign.push(MenuEntry::Separator);
    }
    let (mx, members) = (mixer.clone(), strips.clone());
    assign.push(MenuEntry::item("New VCA", move |_, cx| {
        mx.with(cx, |m, d| m.new_vca(d, &members));
    }));
    out.push(MenuEntry::submenu("Assign to VCA", true, assign));
    let holding: Vec<MenuEntry> = m
        .vcas_holding(&strips)
        .into_iter()
        .map(|v| {
            let off: Vec<u64> = strips
                .iter()
                .copied()
                .filter(|x| v.members.contains(x))
                .collect();
            let (mx, vca) = (mixer.clone(), v.id);
            MenuEntry::item(v.name.clone(), move |_, cx| {
                mx.with(cx, |m, d| m.assign(d, &off, vca, false));
            })
        })
        .collect();
    let any = !holding.is_empty();
    out.push(MenuEntry::submenu("Remove from VCA", any, holding));
    out
}

/// Slot `slot`'s menu (Swift `SlotView.menu`, `EqSlotView.menu`): Open
/// (its plug-in window), Load (held), Replace… (the EQ; disabled: the
/// catalogue comes later), Bypass or Turn On, Sidechain ▸, Remove (an
/// insert) or Remove EQ.
fn slot_entries(mixer: &MixerRef, m: &MixerModel, slot: u64) -> Vec<MenuEntry> {
    let Some((s, x, place)) = m.slot(slot) else {
        return vec![];
    };
    let strip = s.id;
    m.slot_menu(s, x, place)
        .into_iter()
        .map(|item| {
            let mx = mixer.clone();
            match item {
                SlotMenuItem::Divider => MenuEntry::Separator,
                SlotMenuItem::Open => {
                    let doc = mixer.doc;
                    MenuEntry::item(item.title(), move |_, cx| {
                        plugin_windows::show_later(doc, Subject::Plugin(slot), cx);
                    })
                }
                SlotMenuItem::Replace => MenuEntry::waiting(item.title()),
                SlotMenuItem::Sidechain(ref choices) => {
                    let items = choices
                        .iter()
                        .map(|c| {
                            let (mx, source) = (mixer.clone(), c.source);
                            MenuEntry::item(c.title.clone(), move |_, cx| {
                                mx.with(cx, |m, d| m.set_sidechain(d, slot, source));
                            })
                        })
                        .collect();
                    MenuEntry::submenu(item.title(), true, items)
                }
                SlotMenuItem::Load => MenuEntry::item(item.title(), move |_, cx| {
                    mx.with(cx, |m, d| m.load(d, vec![slot]));
                }),
                SlotMenuItem::Bypass { .. } => MenuEntry::item(item.title(), move |_, cx| {
                    mx.with(cx, |m, d| m.toggle_bypass(d, slot));
                }),
                SlotMenuItem::Remove => MenuEntry::item(item.title(), move |_, cx| {
                    mx.with(cx, |m, d| m.remove_insert(d, slot));
                }),
                SlotMenuItem::RemoveEq => MenuEntry::item(item.title(), move |_, cx| {
                    mx.with(cx, |m, d| m.clear_eq(d, strip));
                }),
            }
        })
        .collect()
}

/// Send `index`'s menu (Swift `SendKnob`'s): Pre-Fader and On, checked
/// while on, then Remove Send. MIDI Learn comes with the surfaces.
fn send_entries(mixer: &MixerRef, strip: u64, send: &SendPart) -> Vec<MenuEntry> {
    let index = send.index;
    let (pre, on) = (send.pre_fader, send.enabled);
    let (a, b, c) = (mixer.clone(), mixer.clone(), mixer.clone());
    vec![
        MenuEntry::item(SendLabels::PRE_FADER, move |_, cx| {
            a.with(cx, |m, d| m.set_send_pre_fader(d, strip, index, !pre));
        })
        .checked(pre),
        MenuEntry::item(SendLabels::ON, move |_, cx| {
            b.with(cx, |m, d| m.set_send_enabled(d, strip, index, !on));
        })
        .checked(on),
        MenuEntry::Separator,
        MenuEntry::item(SendLabels::REMOVE, move |_, cx| {
            c.with(cx, |m, d| m.remove_send(d, strip, index));
        }),
    ]
}

/// The "Send" menu (Swift `SendKnobRow`): a send to each bus the strip
/// may feed, then New Bus.
fn add_send_entries(mixer: &MixerRef, m: &MixerModel, strip: u64) -> Vec<MenuEntry> {
    let Some(s) = m.strip(strip) else {
        return vec![];
    };
    let mut out: Vec<MenuEntry> = m
        .targets(s)
        .into_iter()
        .map(|b| {
            let (mx, bus) = (mixer.clone(), b.id);
            MenuEntry::item(b.name.clone(), move |_, cx| {
                mx.with(cx, |m, d| m.add_send(d, strip, bus));
            })
        })
        .collect();
    out.push(MenuEntry::Separator);
    let mx = mixer.clone();
    out.push(MenuEntry::item(SendLabels::NEW_BUS, move |_, cx| {
        mx.with(cx, |m, d| m.new_bus(d, strip));
    }));
    out
}

/// A strip menu open under its face: a backdrop that closes it, and the
/// list of `items`, recorded as menu items (an open submenu's after them).
pub(crate) fn popup(
    view: &Entity<MixerView>,
    id: &'static str,
    items: Vec<MenuEntry>,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &App,
) -> Vec<AnyElement> {
    let size = window.viewport_size();
    let closing = view.downgrade();
    let backdrop = deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("mixer-menu-backdrop")
                .w(size.width)
                .h(size.height)
                .occlude()
                .on_any_mouse_down(move |_, _, cx| {
                    closing.update(cx, |v, cx| v.close_menu(cx)).ok();
                }),
        ),
    )
    .with_priority(1)
    .into_any_element();
    let submenu = view.read(cx).submenu;
    let list = menu_rows(view, id, items, Some(submenu), scope);
    let list = div()
        .absolute()
        .top_full()
        .left_0()
        .child(deferred(anchored().snap_to_window().child(list)).with_priority(2))
        .into_any_element();
    vec![backdrop, list]
}

/// A menu's list. `submenu`: the row whose submenu is open (`None`: this
/// is a submenu's list, whose rows open none).
fn menu_rows(
    view: &Entity<MixerView>,
    id: &'static str,
    items: Vec<MenuEntry>,
    submenu: Option<Option<usize>>,
    scope: &mut RegistryScope,
) -> gpui::Stateful<gpui::Div> {
    let t = tokens();
    let mut list = crate::arrangement::menu_list(id);
    let row_div = |row: usize, enabled: bool| {
        div()
            .id(("mixer-menu-item", row))
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .px_3()
            .py_1()
            .text_xs()
            .when_enabled(enabled)
    };
    for (row, item) in items.into_iter().enumerate() {
        match item {
            MenuEntry::Separator => list = list.child(crate::arrangement::menu_separator()),
            MenuEntry::Item {
                title,
                checked,
                enabled,
                swatch,
                pick,
            } => {
                let closing = view.downgrade();
                let mut el = row_div(row, enabled)
                    .child(if checked { "\u{2713}" } else { "" }.to_owned())
                    .children(swatch.map(|c| div().flex_none().size(px(8.)).rounded_full().bg(c)))
                    .child(title.clone())
                    .child(scope.control_selected("menuitem", &title, enabled, None, checked));
                if enabled {
                    el = el.on_click(move |_, window, cx| {
                        closing.update(cx, |v, cx| v.close_menu(cx)).ok();
                        pick(window, cx);
                    });
                }
                list = list.child(el);
            }
            MenuEntry::Submenu {
                title,
                enabled,
                items,
            } => {
                let opens = enabled && submenu.is_some();
                let open = opens && submenu == Some(Some(row));
                let mut el = row_div(row, enabled)
                    .child(title.clone())
                    .child(div().flex_1())
                    .child(div().text_color(rgb(t.text_secondary)).child("\u{25B8}"))
                    .child(scope.control("menuitem", &title, enabled));
                if opens {
                    let (clicked, hovered) = (view.downgrade(), view.downgrade());
                    el = el
                        .on_click(move |_, _, cx| {
                            clicked.update(cx, |v, cx| v.open_submenu(row, cx)).ok();
                        })
                        .on_hover(move |over, _, cx| {
                            if *over {
                                hovered.update(cx, |v, cx| v.open_submenu(row, cx)).ok();
                            }
                        });
                }
                if open {
                    let nested = menu_rows(view, "mixer-submenu", items, None, scope);
                    el = el.child(div().absolute().top_0().left_full().child(
                        deferred(anchored().snap_to_window().child(nested)).with_priority(3),
                    ));
                }
                list = list.child(el);
            }
        }
    }
    list
}

/// A menu row's look: enabled rows take the pointer and light up under
/// it, disabled ones are dimmed.
trait MenuRowLook {
    fn when_enabled(self, enabled: bool) -> Self;
}

impl MenuRowLook for gpui::Stateful<gpui::Div> {
    fn when_enabled(self, enabled: bool) -> Self {
        if enabled {
            self.cursor_pointer()
                .hover(|s| s.bg(rgb(crate::transport_bar::colors::button())))
        } else {
            self.text_color(rgb(crate::transport_bar::colors::disabled()))
        }
    }
}
