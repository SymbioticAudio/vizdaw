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
//! with a `button` per note, and a `group` "Keyboard". Editing comes in
//! the next task; this view is read-only.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    AnyElement, App, BorderStyle, Bounds, ContentMask, Context, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, PinchEvent, Pixels, ScrollDelta, ScrollEvent,
    StatefulInteractiveElement, Styled, Window, canvas, div, fill, px, quad, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{MeterBars, PageFollow, RULER_HEIGHT, Rect, TimeAxis, WheelScroll};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, Tool};
use vizdaw_app::piano_roll::{
    LaneNotes, PianoRollSettings, PitchAxis, RollGeometry, a11y_label, is_black, key_label,
};
use vizdaw_app::transport::TransportModel;
use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::selection::TickRange;

use crate::arrangement::{
    TimeLayer, bar_button, canvas_rect, colors, hsla, paint_text, tool_picker,
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
/// The keyboard's black key bar, as a part of the strip's width.
const BLACK_KEY_WIDTH: f32 = 0.62;

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
    /// makes a new view), at the saved zoom.
    fn unmount(&mut self) {
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
                self.unmount();
            }
            return false;
        };
        self.mounted = true;
        let target = detail.target;
        let s = &d.layout().piano_roll;
        let h = self.body_height();
        let rows = s.row_height as f32;
        if rows != self.pitch.row_height() {
            self.pitch = self.pitch.with_row_height(rows, h / 2.);
        }
        if s.ticks_per_point != self.axis.ticks_per_point() {
            self.axis = self.axis.with_zoom(s.ticks_per_point);
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
    let keyboard = keyboard(roll, &state, &mut scope.container());
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
fn keyboard(roll: &PianoRoll, state: &RollState, scope: &mut RegistryScope) -> AnyElement {
    let column = div()
        .relative()
        .flex_none()
        .w(px(SIDE_WIDTH))
        .h_full()
        .overflow_hidden();
    if !state.show_keyboard {
        return column.into_any_element();
    }
    let pitch = roll.pitch;
    column
        .child(
            canvas(
                |_, _, _| {},
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
    let scene = Scene {
        axis: roll.axis,
        pitch: roll.pitch,
        state,
    };
    let layout = roll.layout.clone();
    let drawn = roll.notes_drawn.clone();
    let playhead = roll.playhead.clone();
    let weak = cx.entity().downgrade();
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
        |bounds, prepared, window, cx| prepared.paint(bounds, window, cx),
    )
    .size_full();
    div()
        .id("piano-roll-canvas")
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_hidden()
        // A click takes the keyboard (GPUI focuses a tracked element on
        // mouse-down), which ends a field edit.
        .track_focus(&roll.focus)
        .child(painter)
        .child(scope.this("group", GROUP, true))
        .child(scope.clip())
        .into_any_element()
}

/// The view's part of a frame.
struct Scene {
    axis: TimeAxis,
    pitch: PitchAxis,
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
