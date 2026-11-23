//! The Piano Roll in the bottom pane (Swift `PianoRollEditor`,
//! `PianoRollBar`, `PianoRollContainer`, `PianoRollView` and
//! `PianoKeyboardView`): a bar (the tool picker shared with the
//! arrangement, the Keyboard toggle, the Tracks and Insert Into menus,
//! Shorter and Taller Rows, Zoom Out and Zoom In), then a 64-point side
//! column (the keyboard, or blank while it is hidden) beside the note
//! canvas: a 22-point ruler over the pitch rows (black keys shaded, a line
//! under every C), the grid, the shown tracks' notes in their colours
//! (the edit track's on top), the selected range's shade and the
//! playhead. The region strip and the expression pane are not here yet;
//! no space is kept for them.
//!
//! The roll has its own time axis (`pianoRoll.ticksPerPoint`, saved) and
//! pitch scroll (not saved). The wheel scrolls; with the secondary
//! modifier, or a pinch, it zooms about the pointer. On opening, and on
//! another target, the view centres the shown notes' pitches and scrolls
//! to a block target; the Pencil in the arrangement asks it to show a bar.
//! Only the notes in view are drawn and recorded: a `group` "Piano Roll"
//! with a `button` per note, and a `group` "Keyboard".
//!
//! Editing goes through the model's roll glue (`vizdaw_app::piano_roll`):
//! a press below the ruler feeds the tool machine (click, ⇧-click, rubber
//! band, the Pencil, the Eraser, note moves, ⌥-copies, transposes and
//! resizes, auto-scrolling near an edge), and the pressed note sounds. A
//! ruler click locates (snapped), a ruler drag selects the range over the
//! shown tracks. The keyboard strip sounds the key pressed and each one
//! crossed. Escape, the window losing the keyboard and the roll leaving
//! the pane drop a press and silence it; Delete and Forward Delete delete
//! the selected notes while the canvas has the keyboard.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, BorderStyle, Bounds, ContentMask, Context, CursorStyle, DispatchPhase,
    FocusHandle, Focusable, InteractiveElement, IntoElement, KeyBinding, KeyDownEvent, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, PinchEvent, Pixels,
    Point, ScrollDelta, ScrollEvent, StatefulInteractiveElement, Styled, Task, Window, actions,
    canvas, div, fill, px, quad, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{
    self as model, AutoScroll, DEAD_ZONE, MeterBars, PageFollow, Phase, RULER_HEIGHT, Rect,
    TimeAxis, WheelScroll,
};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, Tool};
use vizdaw_app::piano_roll::{
    CursorKind, LaneNotes, PianoRollSettings, PitchAxis, RollFrame, RollGeometry, RollOutcome,
    RollPointer, a11y_label, is_black, key_label,
};
use vizdaw_app::transport::TransportModel;
use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::selection::SelectionRow;
use vizdaw_ffi::selection::TickRange;

use crate::arrangement::{
    TimeLayer, bar_button, canvas_rect, colors, hsla, machine_modifiers, paint_text, tool_picker,
};
use crate::detail_pane;
use crate::list_editor::{Entry, bar_menu};
use crate::registry::{ChildRecorder, RegistryScope};
use crate::shell::DocumentWindow;
use crate::transport_bar::{self, colors as bar_colors};

/// The note canvas's tree label (Swift `PianoRollView.accessibilityName`).
pub const GROUP: &str = "Piano Roll";
/// The keyboard strip's group, and the bar's toggle that shows it.
pub const KEYBOARD: &str = "Keyboard";
/// The bar's menus and buttons (Swift's titles and help texts).
pub const TRACKS: &str = "Tracks";
pub const INSERT_INTO: &str = "Insert Into";
pub const SHORTER_ROWS: &str = "Shorter Rows";
pub const TALLER_ROWS: &str = "Taller Rows";
pub const ZOOM_OUT: &str = "Zoom Out";
pub const ZOOM_IN: &str = "Zoom In";
/// The side column's width (Swift `PianoRollContainer.sideWidth`): the
/// keyboard, or blank while it is hidden.
pub const SIDE_WIDTH: f32 = 64.;
/// A row-height button's step (Swift ±2).
const ROW_STEP: f64 = 2.;
/// The zoom buttons' factor (Swift ×1.5 and ÷1.5).
const ZOOM_STEP: f64 = 1.5;
/// Keys are labelled only when rows are at least this tall.
const LABEL_ROW_HEIGHT: f32 = 8.;
/// The rubber band's fill alpha.
const BAND_ALPHA: f32 = 0.15;
/// The keyboard's black key bar, as a part of the strip's width.
const BLACK_KEY_WIDTH: f32 = 0.62;

actions!(piano_roll, [DeleteNotes]);

/// The key context of the note canvas: Delete and Forward Delete are its
/// own while it has the keyboard (before the window's key bindings,
/// which bind Delete to step entry).
pub const CONTEXT: &str = "PianoRoll";

/// The canvas's keys (only while it has the keyboard).
pub fn bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("backspace", DeleteNotes, c),
        KeyBinding::new("delete", DeleteNotes, c),
    ]
}

/// How often auto-scroll steps during a drag near an edge (60 Hz).
const AUTO_SCROLL_EVERY: Duration = Duration::from_micros(16_667);

/// A press in the ruler: a click until it moves past the dead zone, then
/// a range drag (Swift `EditorCanvas`'s ruler part).
#[derive(Debug, Clone, Copy)]
struct RulerDrag {
    /// The tick pressed (a tick, so it survives auto-scroll).
    anchor: i64,
    start_x: f32,
    /// The pointer's x in the canvas.
    x: f32,
    moved: bool,
    preview: Option<TickRange>,
}

/// Which bar menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RollMenu {
    Tracks,
    InsertInto,
}

/// The roll's view state, owned by the document window. None of it is
/// saved; the zoom it shows is the layout's.
pub struct PianoRoll {
    focus: FocusHandle,
    axis: TimeAxis,
    pitch: PitchAxis,
    /// The note canvas's bounds at the last draw (ruler included; zero
    /// before the first).
    layout: Rc<Cell<Bounds<Pixels>>>,
    /// The target the view last centred on: another one centres again.
    centred_on: Option<EditorTarget>,
    /// The Pencil reveal last applied.
    revealed: Option<u64>,
    pub(crate) menu: Option<RollMenu>,
    /// The detail was a Piano Roll at the last draw.
    mounted: bool,
    notes_drawn: Rc<Cell<usize>>,
    playhead: Rc<Cell<Option<f32>>>,
    /// The roll left the pane since the last draw: its press, drag and
    /// sound are dropped before the view starts afresh.
    detach_pending: bool,
    /// The tool machine and the note drag in progress.
    pointer: RollPointer,
    /// The serial of the roll's released drag whose final edit was still
    /// waiting on Busy (so leaving the roll aborts it, and only it: once
    /// another gesture is open, the serial no longer matches).
    released: Option<u64>,
    /// The gesture open when the press began (another view's, waiting
    /// on Busy): a release leaving it open did not open one of its own.
    open_at_down: Option<u64>,
    /// The pointer of a press below the ruler (note-area points) and its
    /// modifiers: auto-scroll runs the drag again under a still pointer.
    drag_at: Option<(model::Point, model::Modifiers)>,
    /// The rubber band to draw (note-area points).
    band: Option<Rect>,
    ruler: Option<RulerDrag>,
    _auto_scroll: Option<Task<()>>,
    /// The keyboard strip's key held down.
    key: Option<u8>,
    /// The keyboard strip's bounds at the last draw.
    keyboard_layout: Rc<Cell<Bounds<Pixels>>>,
    /// The pointer's shape over the canvas.
    cursor: CursorKind,
}

impl PianoRoll {
    pub fn new(cx: &mut Context<DocumentWindow>) -> Self {
        PianoRoll {
            focus: cx.focus_handle(),
            axis: Self::fresh_axis(),
            pitch: PitchAxis::default(),
            layout: Rc::default(),
            centred_on: None,
            revealed: None,
            menu: None,
            mounted: false,
            notes_drawn: Rc::default(),
            playhead: Rc::default(),
            detach_pending: false,
            pointer: RollPointer::default(),
            released: None,
            open_at_down: None,
            drag_at: None,
            band: None,
            ruler: None,
            _auto_scroll: None,
            key: None,
            keyboard_layout: Rc::default(),
            cursor: CursorKind::Arrow,
        }
    }

    fn fresh_axis() -> TimeAxis {
        TimeAxis::new(PianoRollSettings::default().ticks_per_point, 0)
    }

    /// The roll's time axis.
    pub fn axis(&self) -> &TimeAxis {
        &self.axis
    }

    /// The roll's pitch axis (row height and vertical scroll).
    pub fn pitch(&self) -> &PitchAxis {
        &self.pitch
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// How many notes the last frame drew (only those in view): a test
    /// seam.
    #[doc(hidden)]
    pub fn notes_drawn(&self) -> usize {
        self.notes_drawn.get()
    }

    /// The playhead's x in the canvas as last drawn; `None` when hidden.
    pub fn playhead_x(&self) -> Option<f32> {
        self.playhead.get()
    }

    /// The pointer's shape over the canvas.
    pub fn cursor(&self) -> CursorKind {
        self.cursor
    }

    /// The rubber band being drawn (note-area points).
    pub fn band(&self) -> Option<Rect> {
        self.band
    }

    /// The range a ruler drag is selecting.
    pub fn preview(&self) -> Option<TickRange> {
        self.ruler.and_then(|r| r.preview)
    }

    /// A press below the ruler is down (its release not seen yet).
    pub(crate) fn pressing(&self) -> bool {
        !matches!(self.pointer.machine.phase(), Phase::Idle)
    }

    /// Window point `p` in note-area points (below the ruler).
    fn content_point(&self, p: Point<Pixels>) -> model::Point {
        let o = self.layout.get().origin;
        model::Point {
            x: f32::from(p.x - o.x),
            y: f32::from(p.y - o.y) - RULER_HEIGHT,
        }
    }

    /// Closes the bar's menu; false when none was open.
    pub(crate) fn close_menu(&mut self) -> bool {
        self.menu.take().is_some()
    }

    /// The notes' area height (under the ruler) at the last draw.
    fn body_height(&self) -> f32 {
        (f32::from(self.layout.get().size.height) - RULER_HEIGHT).max(0.)
    }

    fn width(&self) -> f32 {
        f32::from(self.layout.get().size.width)
    }

    /// Keeps the vertical scroll inside the 128 rows.
    fn clamp_vertical(&mut self) {
        let h = self.body_height();
        if h > 0. {
            self.pitch = self.pitch.clamped(h);
        }
    }

    /// The roll left the pane: opened again, it starts afresh (Swift
    /// makes a new view), at the saved zoom. Its press is the window's to
    /// drop first ([`DocumentWindow::roll_detach`]).
    fn unmount(&mut self) {
        self.pointer = RollPointer::default();
        self.released = None;
        self.open_at_down = None;
        self.drag_at = None;
        self.band = None;
        self.ruler = None;
        self._auto_scroll = None;
        self.key = None;
        self.cursor = CursorKind::Arrow;
        self.axis = Self::fresh_axis();
        self.pitch = PitchAxis::default();
        self.layout.set(Bounds::default());
        self.centred_on = None;
        self.revealed = None;
        self.menu = None;
        self.mounted = false;
        self.notes_drawn.set(0);
        self.playhead.set(None);
    }

    /// Before drawing: whether the roll shows (the detail is a Piano
    /// Roll). It takes the saved row height (keeping the middle pitch) and
    /// zoom (keeping the origin), centres on a new target once its height
    /// is known, and applies a new Pencil reveal for its target.
    pub(crate) fn prepare(&mut self, d: &DocumentModel) -> bool {
        let Some(detail) = d
            .layout()
            .detail
            .as_ref()
            .filter(|x| x.kind == DetailEditorKind::PianoRoll)
        else {
            if self.mounted {
                self.mounted = false;
                self.detach_pending = true;
            }
            return false;
        };
        self.mounted = true;
        let target = detail.target;
        let s = &d.layout().piano_roll;
        let h = self.body_height();
        // Compared as the axes would hold them: a saved value out of
        // range would otherwise re-anchor every frame.
        let rows = PitchAxis::new(s.row_height as f32, 0.).row_height();
        if rows != self.pitch.row_height() {
            self.pitch = self.pitch.with_row_height(rows, h / 2.);
        }
        let tpp = TimeAxis::new(s.ticks_per_point, 0).ticks_per_point();
        if tpp != self.axis.ticks_per_point() {
            self.axis = self.axis.with_zoom(tpp);
        }
        if h > 0. && self.centred_on != Some(target) {
            self.centre(d, target, h);
        }
        if let Some(r) = d.piano_roll().reveal()
            && r.target == target
            && self.revealed != Some(r.serial)
        {
            self.revealed = Some(r.serial);
            let tpp = self.axis.ticks_per_point();
            self.axis = self.axis.scrolled_to((r.tick as f64 - 4. * tpp).max(0.));
        }
        self.clamp_vertical();
        true
    }

    /// The shown notes' middle pitch (middle C with none) in the middle
    /// of the view; a block target scrolls to 16 points before its start.
    fn centre(&mut self, d: &DocumentModel, target: EditorTarget, height: f32) {
        self.centred_on = Some(target);
        let lanes = d.roll_lane_notes();
        let span = lanes
            .iter()
            .flat_map(|l| l.notes().iter().map(|n| n.pitch))
            .fold(None, |r: Option<(u8, u8)>, p| match r {
                None => Some((p, p)),
                Some((lo, hi)) => Some((lo.min(p), hi.max(p))),
            });
        let middle = span.map_or(60, |(lo, hi)| ((u16::from(lo) + u16::from(hi)) / 2) as u8);
        self.pitch = self.pitch.centering(middle, height);
        if let Some(r) = d.roll_focus_range() {
            let tpp = self.axis.ticks_per_point();
            self.axis = self.axis.scrolled_to(r.start as f64 - 16. * tpp);
        }
    }

    /// Before drawing: while its sequence plays (and the layout follows),
    /// the view follows the playhead a page at a time.
    pub(crate) fn follow(&mut self, state: &RollState) {
        let width = self.width();
        if state.playing
            && state.follow
            && width > 0.
            && let Some(tick) = state.playhead
            && let Some(origin) = PageFollow::origin(tick, &self.axis, width)
        {
            self.axis = self.axis.scrolled_to(origin);
        }
    }
}

/// One shown track as the roll draws it.
struct LaneDraw {
    notes: LaneNotes,
    colour: u32,
    /// Its name in the notes' labels (only with more than one shown).
    name: Option<String>,
    /// The edit track (drawn on top, brighter).
    active: bool,
}

/// What the roll draws of the document this frame. The notes are the
/// document's, shared; their labels are made while drawing, for the notes
/// in view only.
pub struct RollState {
    tool: Tool,
    show_keyboard: bool,
    lanes: Vec<LaneDraw>,
    selected: HashSet<u64>,
    bars: MeterBars,
    range: Option<TickRange>,
    /// The playhead's tick, when the transport is on the roll's sequence.
    playhead: Option<f64>,
    playing: bool,
    follow: bool,
    /// The Tracks menu: each track of the sequence, its name, whether it
    /// shows, whether it can be hidden (not the target's own).
    tracks: Vec<(u64, String, bool, bool)>,
    /// The Insert Into menu: the shown tracks and their names.
    shown: Vec<(LaneRef, String)>,
    edit_lane: Option<LaneRef>,
}

impl RollState {
    /// The roll on `doc`'s detail target, with `transport`'s playhead at
    /// `now`; `None` when the detail is not a Piano Roll.
    pub fn of(
        doc: &DocumentModel,
        transport: Option<&TransportModel>,
        now: Instant,
    ) -> Option<Self> {
        let detail = doc
            .layout()
            .detail
            .as_ref()
            .filter(|d| d.kind == DetailEditorKind::PianoRoll)?;
        let target = detail.target;
        let seq = target.seq();
        let rows = doc.lane_rows(seq);
        let name = |id: u64| {
            rows.iter()
                .find(|r| r.id == id)
                .map_or_else(|| "Track".to_owned(), |r| r.name.clone())
        };
        let lane_notes = doc.roll_lane_notes();
        let several = lane_notes.len() > 1;
        let edit_lane = doc.roll_edit_lane();
        let swatches = &colors::TRACK_SWATCHES;
        let lanes = lane_notes
            .iter()
            .map(|l| LaneDraw {
                notes: l.clone(),
                colour: swatches[doc.roll_color_index(l.lane) % swatches.len()],
                name: several.then(|| name(l.lane.lane)),
                active: Some(l.lane) == edit_lane,
            })
            .collect();
        let shown: Vec<(LaneRef, String)> = lane_notes
            .iter()
            .map(|l| (l.lane, name(l.lane.lane)))
            .collect();
        let tracks = rows
            .iter()
            .map(|r| {
                (
                    r.id,
                    r.name.clone(),
                    shown.iter().any(|(l, _)| l.lane == r.id),
                    target.lane() != Some(r.id),
                )
            })
            .collect();
        let (playhead, playing) = match transport {
            Some(t) if t.state().seq == Some(seq) => {
                let tick = if t.clock().last().is_some() {
                    t.clock().tick_at(now)
                } else {
                    t.state().tick
                };
                (Some(tick as f64), t.state().playing)
            }
            _ => (None, false),
        };
        Some(RollState {
            tool: doc.tool(),
            show_keyboard: doc.layout().piano_roll.show_keyboard,
            lanes,
            selected: doc.selection().events.iter().copied().collect(),
            bars: MeterBars::new(
                doc.sequence_info(seq)
                    .map_or(&[][..], |i| i.meter.as_slice()),
            ),
            range: doc
                .selection()
                .time_range
                .or_else(|| doc.roll_focus_range()),
            playhead,
            playing,
            follow: doc.layout().follow,
            tracks,
            shown,
            edit_lane,
        })
    }

    /// Playing the roll's sequence (the playhead moves every frame).
    pub fn playing(&self) -> bool {
        self.playing
    }
}

/// The roll: its bar, then the side column beside the canvas. `scope` is
/// its own container.
pub(crate) fn render(
    roll: &PianoRoll,
    state: RollState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let bar = bar(roll, &state, &mut scope.container(), window, cx);
    let keyboard = keyboard(roll, &state, &mut scope.container(), cx);
    let notes = note_canvas(roll, state, &mut scope.container(), cx);
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(bar)
        .child(detail_pane::divider())
        .child(
            div()
                .id("piano-roll")
                .relative()
                .flex()
                .flex_row()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                // The keyboard's wheel is the canvas's (Swift's container
                // forwards it).
                .on_scroll(cx.listener(|this, e, _, cx| this.roll_wheel(e, cx)))
                .on_pinch(cx.listener(|this, e, _, cx| this.roll_pinch(e, cx)))
                .child(keyboard)
                .child(notes),
        )
        .into_any_element()
}

fn close_roll_menu(this: &mut DocumentWindow) {
    this.piano_roll.menu = None;
}

/// The bar (Swift `PianoRollBar`), left to right.
fn bar(
    roll: &PianoRoll,
    state: &RollState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let picker = tool_picker("roll-tool", state.tool, scope, cx);
    let on = state.show_keyboard;
    let keyboard = bar_button(
        "roll-keyboard",
        KEYBOARD,
        KEYBOARD,
        80.,
        on,
        Some(if on { "on" } else { "off" }),
        scope,
    )
    .on_click(cx.listener(|this, _, _, cx| {
        this.piano_roll.menu = None;
        this.with_model(cx, |d| {
            d.update_layout(|l| l.piano_roll.show_keyboard = !l.piano_roll.show_keyboard)
        });
    }));
    let mut tracks = bar_button(
        "roll-tracks",
        &format!("{TRACKS} \u{25be}"),
        TRACKS,
        80.,
        false,
        None,
        scope,
    )
    .on_click(cx.listener(|this, _, _, cx| this.toggle_roll_menu(RollMenu::Tracks, cx)));
    if roll.menu == Some(RollMenu::Tracks) {
        let items = state
            .tracks
            .iter()
            .map(|(id, name, shown, enabled)| {
                let id = *id;
                Entry::toggle(name, *shown, move |this, cx| {
                    this.with_model(cx, |d| d.roll_toggle_shown(id));
                })
                .enabled(*enabled)
            })
            .collect();
        tracks = tracks.children(bar_menu(
            "roll-tracks-menu",
            items,
            close_roll_menu,
            &mut scope.container(),
            window,
            cx,
        ));
    }
    let edit_name = state
        .edit_lane
        .and_then(|e| state.shown.iter().find(|(l, _)| *l == e))
        .map_or("\u{2014}", |(_, n)| n.as_str());
    let title = format!("{INSERT_INTO}: {edit_name}");
    let mut insert = bar_button(
        "roll-insert-into",
        &format!("{title} \u{25be}"),
        &title,
        190.,
        false,
        None,
        scope,
    )
    // A long track name is cut at the button's width.
    .overflow_hidden()
    .whitespace_nowrap()
    .on_click(cx.listener(|this, _, _, cx| this.toggle_roll_menu(RollMenu::InsertInto, cx)));
    if roll.menu == Some(RollMenu::InsertInto) {
        let items = state
            .shown
            .iter()
            .map(|(lane, name)| {
                let lane = *lane;
                Entry::item(
                    name,
                    None,
                    state.edit_lane == Some(lane),
                    move |this, cx| {
                        this.with_model(cx, |d| d.roll_set_edit_lane(lane));
                    },
                )
            })
            .collect();
        insert = insert.children(bar_menu(
            "roll-insert-menu",
            items,
            close_roll_menu,
            &mut scope.container(),
            window,
            cx,
        ));
    }
    let button = |id: &'static str, glyph: &str, label: &str, scope: &mut RegistryScope| {
        transport_bar::cluster_button(id, glyph, label, true, bar_colors::BUTTON, scope)
    };
    let shorter = button("roll-shorter", "\u{25be}", SHORTER_ROWS, scope)
        .on_click(cx.listener(|this, _, _, cx| this.set_roll_rows(-ROW_STEP, cx)));
    let taller = button("roll-taller", "\u{25b4}", TALLER_ROWS, scope)
        .on_click(cx.listener(|this, _, _, cx| this.set_roll_rows(ROW_STEP, cx)));
    let zoom_out = button("roll-zoom-out", "\u{2212}", ZOOM_OUT, scope)
        .on_click(cx.listener(|this, _, _, cx| this.zoom_roll_by(ZOOM_STEP, cx)));
    let zoom_in = button("roll-zoom-in", "+", ZOOM_IN, scope)
        .on_click(cx.listener(|this, _, _, cx| this.zoom_roll_by(1. / ZOOM_STEP, cx)));
    div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .px(px(8.))
        .py(px(4.))
        .bg(rgb(bar_colors::BAR))
        .child(picker)
        .child(keyboard)
        .child(tracks)
        .child(insert)
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .gap(px(4.))
                .child(shorter)
                .child(taller)
                .child(zoom_out)
                .child(zoom_in),
        )
        .into_any_element()
}

/// The side column: the keyboard strip, or a blank column of the same
/// width while it is hidden.
fn keyboard(
    roll: &PianoRoll,
    state: &RollState,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let column = div()
        .id("piano-roll-keyboard")
        .relative()
        .flex_none()
        .w(px(SIDE_WIDTH))
        .h_full()
        .overflow_hidden();
    if !state.show_keyboard {
        return column.into_any_element();
    }
    let pitch = roll.pitch;
    let layout = roll.keyboard_layout.clone();
    column
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, e: &MouseDownEvent, _, cx| this.roll_key_press(e.position, cx)),
        )
        .child(
            canvas(
                move |bounds, _, _| layout.set(bounds),
                move |bounds, _, window, cx| paint_keyboard(bounds, &pitch, window, cx),
            )
            .size_full(),
        )
        .child(scope.this("group", KEYBOARD, true))
        .into_any_element()
}

/// The keys of the pitches in view, under a ruler-coloured strip (Swift
/// `PianoKeyboardView.draw`).
fn paint_keyboard(bounds: Bounds<Pixels>, pitch: &PitchAxis, window: &mut Window, cx: &mut App) {
    let o = bounds.origin;
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let body_h = (h - RULER_HEIGHT).max(0.);
    let rect = |x: f32, y: f32, w: f32, h: f32| canvas_rect(o, x, y, w, h);
    window.paint_quad(fill(bounds, rgb(colors::RULER)));
    let body = rect(0., RULER_HEIGHT, w, body_h);
    if body_h > 0. {
        window.with_content_mask(Some(ContentMask { bounds: body }), |window| {
            let rh = pitch.row_height();
            for p in pitch.visible(body_h) {
                let y = RULER_HEIGHT + pitch.top(p);
                window.paint_quad(fill(rect(0., y, w, rh), rgb(colors::WHITE_KEY)));
                if is_black(p) {
                    window.paint_quad(fill(
                        rect(0., y + 1., w * BLACK_KEY_WIDTH, (rh - 2.).max(0.)),
                        hsla(colors::BLACK_KEY, colors::BLACK_KEY_ALPHA),
                    ));
                }
                window.paint_quad(fill(rect(0., y + rh - 0.5, w, 0.5), rgb(colors::SEPARATOR)));
                if rh >= LABEL_ROW_HEIGHT
                    && let Some(label) = key_label(p)
                {
                    paint_text(
                        &label,
                        o.x + px(w - 26.),
                        o.y + px(y + (rh - 14.) / 2.),
                        colors::KEY_LABEL,
                        window,
                        cx,
                    );
                }
            }
        });
    }
    window.paint_quad(fill(rect(w - 1., 0., 1., h), rgb(colors::SEPARATOR)));
}

/// The note canvas, recorded in `scope` (the group and its notes).
fn note_canvas(
    roll: &PianoRoll,
    state: RollState,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let mut recorder = scope.children();
    let mut state = state;
    if let Some(r) = roll.preview() {
        state.range = Some(r);
    }
    let scene = Scene {
        axis: roll.axis,
        pitch: roll.pitch,
        band: roll.band,
        state,
    };
    let layout = roll.layout.clone();
    let drawn = roll.notes_drawn.clone();
    let playhead = roll.playhead.clone();
    let weak = cx.entity().downgrade();
    let weak_events = weak.clone();
    let painter = canvas(
        move |bounds, _, cx| {
            // Another size shows other notes than were laid out for this
            // frame, and centring waits for a height: draw again.
            if layout.get().size != bounds.size {
                let weak = weak.clone();
                cx.defer(move |cx| {
                    weak.update(cx, |_, cx| cx.notify()).ok();
                });
            }
            layout.set(bounds);
            let prepared = scene.prepare(bounds, &mut recorder);
            drawn.set(prepared.count);
            playhead.set(prepared.time.playhead);
            prepared
        },
        move |bounds, prepared, window, cx| {
            prepared.paint(bounds, window, cx);
            // A press follows the pointer anywhere in the window.
            let moved = weak_events.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    moved
                        .update(cx, |this, cx| {
                            let held = e.pressed_button == Some(MouseButton::Left);
                            this.roll_drag_to(e.position, e.modifiers, held, cx)
                        })
                        .ok();
                }
            });
            let released = weak_events.clone();
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                    released
                        .update(cx, |this, cx| {
                            this.roll_release(e.position, e.modifiers, cx)
                        })
                        .ok();
                }
            });
        },
    )
    .size_full();
    div()
        .id("piano-roll-canvas")
        .key_context(CONTEXT)
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_hidden()
        .cursor(cursor_style(roll.cursor))
        // A click takes the keyboard (GPUI focuses a tracked element on
        // mouse-down), which ends a field edit.
        .track_focus(&roll.focus)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, e: &MouseDownEvent, window, cx| this.roll_mouse_down(e, window, cx)),
        )
        .on_mouse_move(
            cx.listener(|this, e: &MouseMoveEvent, _, cx| this.roll_hover(e.position, cx)),
        )
        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
            if e.keystroke.key == "escape" && this.cancel_roll(cx) {
                cx.stop_propagation();
            }
        }))
        .on_action(cx.listener(|this, _: &DeleteNotes, _, cx| this.roll_delete(cx)))
        .child(painter)
        .child(scope.this("group", GROUP, true))
        .child(scope.clip())
        .into_any_element()
}

/// The roll's pointer shapes as GPUI's (it has no disappearing-item
/// cursor: the eraser shows "not allowed").
fn cursor_style(c: CursorKind) -> CursorStyle {
    match c {
        CursorKind::Arrow => CursorStyle::Arrow,
        CursorKind::OpenHand => CursorStyle::OpenHand,
        CursorKind::UpDown => CursorStyle::ResizeUpDown,
        CursorKind::ResizeRight => CursorStyle::ResizeRight,
        CursorKind::Crosshair => CursorStyle::Crosshair,
        CursorKind::Eraser => CursorStyle::OperationNotAllowed,
    }
}

/// The view's part of a frame.
struct Scene {
    axis: TimeAxis,
    pitch: PitchAxis,
    band: Option<Rect>,
    state: RollState,
}

/// One frame, laid out in canvas points (y = 0 is the ruler's top).
struct Prepared {
    time: TimeLayer,
    row_height: f32,
    /// Each pitch row in view: its top, a black key's, a C's.
    rows: Vec<(f32, bool, bool)>,
    /// The notes in view, in paint order (the edit track's last).
    notes: Vec<NoteDraw>,
    count: usize,
    /// The rubber band (canvas points).
    band: Option<Rect>,
}

struct NoteDraw {
    rect: Rect,
    colour: u32,
    alpha: f32,
    selected: bool,
}

impl Scene {
    /// Lays out the frame in `bounds`, recording the notes in view (track
    /// by track, in time order).
    fn prepare(&self, bounds: Bounds<Pixels>, recorder: &mut ChildRecorder) -> Prepared {
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let body_h = (h - RULER_HEIGHT).max(0.);
        let (axis, pitch, state) = (&self.axis, &self.pitch, &self.state);
        let time = TimeLayer::new(axis, &state.bars, w, h, state.range, state.playhead);
        let visible = pitch.visible(body_h);
        let rows = if body_h > 0. {
            visible
                .clone()
                .map(|p| (RULER_HEIGHT + pitch.top(p), is_black(p), p % 12 == 0))
                .collect()
        } else {
            vec![]
        };
        let body = Rect {
            x: 0.,
            y: 0.,
            width: w,
            height: body_h,
        };
        let mut lanes: Vec<Vec<NoteDraw>> = vec![];
        for l in &state.lanes {
            let mut drawn = vec![];
            if body_h > 0. {
                for n in RollGeometry::candidates(&l.notes, axis, 0., w) {
                    if !visible.contains(&n.pitch) {
                        continue;
                    }
                    let r = RollGeometry::rect(n, axis, pitch);
                    if !r.intersects(&body) {
                        continue;
                    }
                    let selected = state.selected.contains(&n.id);
                    recorder.record(
                        "button",
                        &a11y_label(n, l.name.as_deref(), &state.bars),
                        canvas_rect(bounds.origin, r.x, r.y + RULER_HEIGHT, r.width, r.height),
                        true,
                        selected,
                    );
                    let alpha = if l.active {
                        colors::NOTE_ACTIVE_ALPHA
                    } else {
                        colors::NOTE_ALPHA
                    } * if n.mute { colors::MUTED_ALPHA } else { 1. };
                    drawn.push(NoteDraw {
                        rect: Rect {
                            y: r.y + RULER_HEIGHT,
                            ..r
                        },
                        colour: l.colour,
                        alpha,
                        selected,
                    });
                }
            }
            lanes.push(drawn);
        }
        let count = lanes.iter().map(Vec::len).sum();
        // Bottom to top: the others in reverse order, then the edit
        // track's (the hit test's order, reversed).
        let order: Vec<usize> = (0..lanes.len())
            .rev()
            .filter(|&i| !state.lanes[i].active)
            .chain((0..lanes.len()).filter(|&i| state.lanes[i].active))
            .collect();
        let mut notes = Vec::with_capacity(count);
        for i in order {
            notes.append(&mut lanes[i]);
        }
        Prepared {
            time,
            row_height: pitch.row_height(),
            rows,
            notes,
            count,
            band: self.band.map(|b| Rect {
                y: b.y + RULER_HEIGHT,
                ..b
            }),
        }
    }
}

impl Prepared {
    fn paint(self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let o = bounds.origin;
        let w = f32::from(bounds.size.width);
        let rect = |x: f32, y: f32, w: f32, h: f32| canvas_rect(o, x, y, w, h);
        let rh = self.row_height;
        let body = self.time.body(o);
        window.with_content_mask(Some(ContentMask { bounds: body }), |window| {
            self.time.paint_grid(o, window);
            for &(y, black, c) in &self.rows {
                if black {
                    window.paint_quad(fill(
                        rect(0., y, w, rh),
                        hsla(colors::LABEL, colors::BLACK_ROW_ALPHA),
                    ));
                }
                if c {
                    window.paint_quad(fill(rect(0., y + rh - 0.5, w, 1.), rgb(colors::SEPARATOR)));
                }
            }
            for n in &self.notes {
                let r = n.rect;
                let (line, width) = if n.selected {
                    (colors::REGION_SELECTED, 2.)
                } else {
                    (colors::REGION_LINE, 1.)
                };
                let (nw, nh) = ((r.width - 1.).max(0.), (r.height - 1.).max(0.));
                window.paint_quad(quad(
                    rect(r.x + 0.5, r.y + 0.5, nw, nh),
                    px(colors::RADIUS.min(nh / 2.)),
                    hsla(n.colour, n.alpha),
                    px(width),
                    rgb(line),
                    BorderStyle::Solid,
                ));
            }
            self.time.paint_overlay(o, window);
            if let Some(b) = self.band {
                window.paint_quad(quad(
                    rect(b.x, b.y, b.width, b.height),
                    px(0.),
                    hsla(colors::REGION_SELECTED, BAND_ALPHA),
                    px(1.),
                    rgb(colors::REGION_SELECTED),
                    BorderStyle::Solid,
                ));
            }
        });
        self.time.paint_ruler(o, window, cx);
    }
}

impl DocumentWindow {
    /// The Piano Roll's view state.
    pub fn piano_roll(&self) -> &PianoRoll {
        &self.piano_roll
    }

    fn toggle_roll_menu(&mut self, m: RollMenu, cx: &mut Context<Self>) {
        let r = &mut self.piano_roll;
        r.menu = if r.menu == Some(m) { None } else { Some(m) };
        cx.notify();
    }

    /// Shorter or Taller Rows: the saved row height by `step`, kept in
    /// 4…40 (the view takes it at the next draw).
    fn set_roll_rows(&mut self, step: f64, cx: &mut Context<Self>) {
        self.piano_roll.menu = None;
        self.with_model(cx, |d| {
            d.update_layout(|l| {
                let range = PianoRollSettings::ROW_HEIGHT_RANGE;
                l.piano_roll.row_height =
                    (l.piano_roll.row_height + step).clamp(*range.start(), *range.end());
            })
        });
    }

    /// Zoom Out or In: the saved zoom times `factor`, clamped; the origin
    /// stays (no anchor, as Swift's buttons).
    fn zoom_roll_by(&mut self, factor: f64, cx: &mut Context<Self>) {
        self.piano_roll.menu = None;
        self.with_model(cx, |d| {
            d.update_layout(|l| {
                l.piano_roll.ticks_per_point = (l.piano_roll.ticks_per_point * factor)
                    .clamp(TimeAxis::MIN_TICKS_PER_POINT, TimeAxis::MAX_TICKS_PER_POINT);
            })
        });
    }

    /// The live zoom is saved in the layout when it changed.
    fn save_roll_zoom(&mut self, cx: &mut Context<Self>) {
        let tpp = self.piano_roll.axis.ticks_per_point();
        let changed = {
            let s = self.app().read(cx);
            s.store
                .document(self.doc())
                .is_some_and(|d| d.layout().piano_roll.ticks_per_point != tpp)
        };
        if changed {
            self.with_model(cx, |d| {
                d.update_layout(|l| l.piano_roll.ticks_per_point = tpp)
            });
        }
    }

    /// The wheel over the keyboard or the canvas: scrolls the time and
    /// the pitches; with the secondary modifier it zooms about the
    /// pointer.
    fn roll_wheel(&mut self, e: &ScrollEvent, cx: &mut Context<Self>) {
        let (precise, dx, dy) = match e.delta {
            ScrollDelta::Pixels(p) => (true, f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(p) => (false, p.x, p.y),
        };
        let r = &mut self.piano_roll;
        if e.modifiers.secondary(cx) {
            let x = f32::from(e.position.x - r.layout.get().origin.x);
            r.axis = r.axis.zoom_about(WheelScroll::zoom_factor(dy, precise), x);
            self.save_roll_zoom(cx);
        } else {
            let (dx, dy) = WheelScroll::points(dx, dy, precise);
            r.axis = r.axis.scrolled_by(-dx);
            r.pitch.set_offset(r.pitch.offset() - dy);
            r.clamp_vertical();
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// A pinch zooms about its centre.
    fn roll_pinch(&mut self, e: &PinchEvent, cx: &mut Context<Self>) {
        let r = &mut self.piano_roll;
        let x = f32::from(e.position.x - r.layout.get().origin.x);
        r.axis = r.axis.zoom_about(1. + f64::from(e.delta), x);
        self.save_roll_zoom(cx);
        cx.stop_propagation();
        cx.notify();
    }

    /// Runs `f` on the document with the roll's pointer and its frame now,
    /// then acts on the outcome: the sounds asked for, the band to draw.
    fn with_roll_pointer(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut DocumentModel, &mut RollPointer, &RollFrame) -> RollOutcome,
    ) {
        let r = &mut self.piano_roll;
        let mut pointer = std::mem::take(&mut r.pointer);
        let (axis, pitch, w, h) = (r.axis, r.pitch, r.width(), r.body_height());
        let out = self.with_model(cx, |d| {
            let frame = d.roll_frame(axis, pitch, w, h);
            let out = f(d, &mut pointer, &frame);
            d.roll_audition(&out.audition);
            out
        });
        self.piano_roll.pointer = pointer;
        self.piano_roll.band = out.and_then(|o| o.band);
        cx.notify();
    }

    /// Reads the window's document (`None` once it closed).
    pub(crate) fn read_model<R>(&self, cx: &App, f: impl FnOnce(&DocumentModel) -> R) -> Option<R> {
        let s = self.app().read(cx);
        s.store.document(self.doc()).map(f)
    }

    /// The document knows whether the roll has the keyboard (the
    /// channel's keys and Delete read it; GPUI's focus events come late).
    fn set_roll_focused(&mut self, on: bool, cx: &mut Context<Self>) {
        if self
            .read_model(cx, DocumentModel::roll_focused)
            .is_some_and(|f| f != on)
        {
            self.with_model(cx, |d| d.set_roll_focused(on));
        }
    }

    /// While drawing: the roll has the keyboard exactly while its canvas
    /// is focused and shown.
    pub(crate) fn sync_roll_focus(&mut self, open: bool, window: &Window, cx: &mut Context<Self>) {
        let on = open && self.piano_roll.focus.is_focused(window);
        self.set_roll_focused(on, cx);
    }

    /// A press in the note canvas: it takes the keyboard; in the ruler it
    /// starts a click or a range drag; below, a press of the tool machine
    /// (which may sound the note).
    fn roll_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.piano_roll.focus, cx);
        self.set_roll_focused(true, cx);
        let at = self.piano_roll.content_point(e.position);
        if at.y < 0. {
            self.piano_roll.ruler = Some(RulerDrag {
                anchor: self.piano_roll.axis.tick_at(at.x),
                start_x: at.x,
                x: at.x,
                moved: false,
                preview: None,
            });
            cx.notify();
            return;
        }
        let m = machine_modifiers(e.modifiers);
        let clicks = u32::try_from(e.click_count).unwrap_or(u32::MAX).max(1);
        self.piano_roll.drag_at = Some((at, m));
        self.piano_roll.released = None;
        self.piano_roll.open_at_down = self.read_model(cx, DocumentModel::gesture_serial).flatten();
        self.with_roll_pointer(cx, |d, p, f| d.roll_mouse_down(p, f, at, clicks, m));
    }

    /// The pointer over the canvas with no press: its shape follows what
    /// it is over.
    fn roll_hover(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let r = &self.piano_roll;
        if r.pressing() || r.ruler.is_some() || r.key.is_some() {
            return;
        }
        let p = r.content_point(at);
        let (axis, pitch, w, h) = (r.axis, r.pitch, r.width(), r.body_height());
        let Some(kind) =
            self.read_model(cx, |d| d.roll_cursor(&d.roll_frame(axis, pitch, w, h), p))
        else {
            return;
        };
        if kind != self.piano_roll.cursor {
            self.piano_roll.cursor = kind;
            cx.notify();
        }
    }

    /// The pitch of the keyboard strip's key at window y `y`; `None` in
    /// the strip's top, the ruler's height (Swift
    /// `PianoKeyboardView.pitch(atY:)`).
    fn key_pitch(&self, y: Pixels) -> Option<u8> {
        let o = self.piano_roll.keyboard_layout.get().origin;
        let y = f32::from(y - o.y) - RULER_HEIGHT;
        (y >= 0.).then(|| self.piano_roll.pitch.pitch_at(y))
    }

    /// A press on the keyboard strip (below its top): the key sounds
    /// through the edit track.
    fn roll_key_press(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(pitch) = self.key_pitch(at.y) else {
            return;
        };
        self.piano_roll.key = Some(pitch);
        self.with_model(cx, |d| d.roll_key_sound(pitch));
    }

    /// A key held on the strip is let go (its release, Escape, or a move
    /// with no button down when the release was missed). Whether one was.
    pub(crate) fn release_roll_key(&mut self, cx: &mut Context<Self>) -> bool {
        if self.piano_roll.key.take().is_none() {
            return false;
        }
        self.with_model(cx, DocumentModel::roll_key_release);
        true
    }

    /// The snapped range between a ruler press at `anchor` and canvas x
    /// `x` (Swift `RulerSelection.range`); `None` when empty.
    fn roll_ruler_range(&self, anchor: i64, x: f32, cx: &App) -> Option<TickRange> {
        let other = self.piano_roll.axis.tick_at(x);
        self.read_model(cx, |d| {
            let (snap, meter) = roll_snap(d);
            let a = snap.apply(anchor.min(other), &meter);
            let b = snap.apply(anchor.max(other), &meter);
            (b > a).then_some(TickRange { start: a, end: b })
        })
        .flatten()
    }

    /// The pointer moved to window point `at` during a press (`held`: the
    /// left button is down): a ruler drag selects, the keyboard plays
    /// each key crossed (a move with no button lets the key go), a press
    /// below the ruler drags (near an edge the view scrolls).
    fn roll_drag_to(
        &mut self,
        at: Point<Pixels>,
        m: Modifiers,
        held: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(mut d) = self.piano_roll.ruler {
            d.x = f32::from(at.x - self.piano_roll.layout.get().origin.x);
            if !d.moved && (d.x - d.start_x).abs() <= DEAD_ZONE {
                return;
            }
            d.moved = true;
            d.preview = self.roll_ruler_range(d.anchor, d.x, cx);
            self.piano_roll.ruler = Some(d);
            if AutoScroll::speed(d.x, self.piano_roll.width()) != 0. {
                self.start_roll_auto_scroll(cx);
            }
            cx.notify();
            return;
        }
        if let Some(key) = self.piano_roll.key {
            if !held {
                self.release_roll_key(cx);
            } else if let Some(pitch) = self.key_pitch(at.y)
                && pitch != key
            {
                self.piano_roll.key = Some(pitch);
                self.with_model(cx, |d| d.roll_key_sound(pitch));
            }
            return;
        }
        if !self.piano_roll.pressing() {
            return;
        }
        let at = self.piano_roll.content_point(at);
        let m = machine_modifiers(m);
        self.piano_roll.drag_at = Some((at, m));
        self.with_roll_pointer(cx, |d, p, f| d.roll_mouse_drag(p, f, at, m));
        let r = &self.piano_roll;
        if matches!(r.pointer.machine.phase(), Phase::Dragging { .. })
            && AutoScroll::speed(at.x, r.width()) != 0.
        {
            self.start_roll_auto_scroll(cx);
        }
    }

    /// The button is up at window point `at`: a ruler click locates
    /// (snapped) and a ruler drag selects its range over the shown
    /// tracks; a key is released; below the ruler, a click or the drag's
    /// end. Every release silences.
    fn roll_release(&mut self, at: Point<Pixels>, m: Modifiers, cx: &mut Context<Self>) {
        self.piano_roll._auto_scroll = None;
        if let Some(d) = self.piano_roll.ruler.take() {
            if d.moved {
                if let Some(range) = d.preview {
                    self.with_model(cx, |m| {
                        let lanes = m.roll_shown_lanes();
                        m.set_selection(SelectionRow {
                            time_range: Some(range),
                            lanes,
                            events: vec![],
                        });
                    });
                }
            } else {
                self.with_model(cx, |m| {
                    let (snap, meter) = roll_snap(m);
                    m.locate(snap.apply(d.anchor, &meter));
                });
                let doc = self.doc();
                self.app().update(cx, |s, cx| {
                    s.poll(doc);
                    cx.notify();
                });
            }
            cx.notify();
            return;
        }
        if self.release_roll_key(cx) {
            return;
        }
        if !self.piano_roll.pressing() {
            return;
        }
        let at = self.piano_roll.content_point(at);
        let m = machine_modifiers(m);
        self.piano_roll.drag_at = None;
        self.with_roll_pointer(cx, |d, p, f| d.roll_mouse_up(p, f, at, m));
        // Only a gesture this press opened is the roll's.
        let open_at_down = self.piano_roll.open_at_down.take();
        self.piano_roll.released = self
            .read_model(cx, DocumentModel::gesture_serial)
            .flatten()
            .filter(|serial| Some(*serial) != open_at_down);
    }

    /// Escape on the canvas: a ruler press is dropped; a drag is
    /// cancelled without an undo step (also one whose final edit waits on
    /// Busy) and the sound stops. Whether there was anything to cancel.
    fn cancel_roll(&mut self, cx: &mut Context<Self>) -> bool {
        if self.piano_roll.ruler.take().is_some() {
            self.piano_roll._auto_scroll = None;
            cx.notify();
            return true;
        }
        let open = self
            .read_model(cx, DocumentModel::gesture_open)
            .unwrap_or(false);
        if !self.piano_roll.pressing() && !open {
            return false;
        }
        self.piano_roll.drag_at = None;
        self.piano_roll._auto_scroll = None;
        self.piano_roll.released = None;
        self.with_roll_pointer(cx, |d, p, f| d.cancel_roll_drag(p, f));
        true
    }

    /// The window lost the keyboard: a ruler press and a held key end,
    /// and a press still held is dropped (its release may go elsewhere);
    /// a released drag waiting on Busy is left to the poll's tick.
    pub(crate) fn roll_deactivated(&mut self, cx: &mut Context<Self>) {
        self.piano_roll._auto_scroll = None;
        if self.piano_roll.ruler.take().is_some() {
            cx.notify();
        }
        self.release_roll_key(cx);
        if self.piano_roll.pressing() {
            self.piano_roll.drag_at = None;
            self.with_roll_pointer(cx, |d, p, f| d.roll_deactivated(p, f));
        }
    }

    /// The roll goes out of sight: its ruler press, held key and drag
    /// are dropped, a released drag's final edit still waiting on Busy
    /// too (Swift: the view leaves its window), and the sound stops.
    fn drop_roll_presses(&mut self, cx: &mut Context<Self>) {
        let r = &mut self.piano_roll;
        r._auto_scroll = None;
        r.ruler = None;
        r.drag_at = None;
        if r.key.take().is_some() {
            self.with_model(cx, DocumentModel::roll_key_release);
        }
        // Still the roll's own drag (not one begun since elsewhere).
        let open = self.piano_roll.released.take().is_some_and(|own| {
            self.read_model(cx, DocumentModel::gesture_serial).flatten() == Some(own)
        });
        if self.piano_roll.pressing() || open {
            self.with_roll_pointer(cx, |d, p, f| d.roll_detached(p, f));
        }
    }

    /// While drawing a hidden pane: the roll keeps no press and no
    /// keyboard (so Delete deletes nothing out of sight).
    pub(crate) fn roll_hidden(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let r = &self.piano_roll;
        if r.pressing() || r.ruler.is_some() || r.key.is_some() || r.released.is_some() {
            self.drop_roll_presses(cx);
        }
        self.set_roll_focused(false, cx);
        if self.piano_roll.focus.is_focused(window) {
            cx.defer_in(window, |this, window, cx| {
                if this.piano_roll.focus.is_focused(window) {
                    window.focus(&this.focus_handle(cx), cx);
                }
            });
        }
    }

    /// Delete or Forward Delete while the canvas has the keyboard: the
    /// selected notes go, as one "Delete Events" step.
    fn roll_delete(&mut self, cx: &mut Context<Self>) {
        if self
            .read_model(cx, DocumentModel::roll_focused)
            .unwrap_or(false)
        {
            self.with_model(cx, DocumentModel::roll_delete_selected);
        }
    }

    fn start_roll_auto_scroll(&mut self, cx: &mut Context<Self>) {
        if self.piano_roll._auto_scroll.is_some() {
            return;
        }
        self.piano_roll._auto_scroll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(AUTO_SCROLL_EVERY).await;
                let going = this.update(cx, |this, cx| this.roll_auto_scroll_step(cx));
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        }));
    }

    /// One auto-scroll step; false once the press has ended or the
    /// pointer left the edges. A drag runs again with the pointer where
    /// it is, so what it moves follows the scrolled view.
    fn roll_auto_scroll_step(&mut self, cx: &mut Context<Self>) -> bool {
        let r = &mut self.piano_roll;
        let width = r.width();
        if let Some(mut d) = r.ruler {
            let v = AutoScroll::speed(d.x, width);
            if v == 0. {
                r._auto_scroll = None;
                return false;
            }
            r.axis = r.axis.scrolled_by(v / 60.);
            d.preview = self.roll_ruler_range(d.anchor, d.x, cx);
            self.piano_roll.ruler = Some(d);
            cx.notify();
            return true;
        }
        let Some((at, m)) = r
            .drag_at
            .filter(|_| matches!(r.pointer.machine.phase(), Phase::Dragging { .. }))
        else {
            r._auto_scroll = None;
            return false;
        };
        let v = AutoScroll::speed(at.x, width);
        if v == 0. {
            r._auto_scroll = None;
            return false;
        }
        r.axis = r.axis.scrolled_by(v / 60.);
        self.with_roll_pointer(cx, |d, p, f| d.roll_mouse_drag(p, f, at, m));
        true
    }

    /// Before drawing, once the roll has left the pane: its presses are
    /// dropped, it gives up the keyboard, and the view starts afresh.
    pub(crate) fn roll_detach(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.piano_roll.detach_pending) {
            return;
        }
        self.drop_roll_presses(cx);
        self.set_roll_focused(false, cx);
        self.piano_roll.unmount();
    }

    /// The window closes: a press still held is dropped and its sound
    /// stops, before the document goes.
    pub(crate) fn roll_closing(roll: &mut PianoRoll, d: &mut DocumentModel) {
        if roll.key.take().is_some() {
            d.roll_key_release();
        }
        if roll.pressing() {
            let frame = d.roll_frame(roll.axis, roll.pitch, roll.width(), roll.body_height());
            let out = d.roll_detached(&mut roll.pointer, &frame);
            d.roll_audition(&out.audition);
        }
    }

    /// After a draw without the roll: a canvas that kept the keyboard
    /// gives it back to the window.
    pub(crate) fn roll_unmounted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.piano_roll.focus.is_focused(window) {
            return;
        }
        cx.defer_in(window, |this, window, cx| {
            if this.piano_roll.focus.is_focused(window) {
                window.focus(&this.focus_handle(cx), cx);
            }
        });
    }
}

/// The snap and the meter the roll's sequence uses.
fn roll_snap(
    d: &DocumentModel,
) -> (
    vizdaw_app::editor::Snap,
    Vec<vizdaw_ffi::events::MeterEntryRow>,
) {
    let meter = d
        .layout()
        .detail
        .as_ref()
        .and_then(|x| d.sequence_info(x.target.seq()))
        .map(|i| i.meter.clone())
        .unwrap_or_default();
    (d.layout().snap, meter)
}
