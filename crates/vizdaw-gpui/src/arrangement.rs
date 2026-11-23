//! The arrangement canvas beside the track headers (Swift `EditorCanvas`
//! and `ArrangementView`): a 22 px ruler over the grid, row separators,
//! the shown sequence's blocks, the selection's range shaded and the
//! playhead. It records as a `group` "Arrangement" whose children are one
//! `button` per visible block, labelled as Swift labels it for VoiceOver.
//!
//! A click takes the keyboard (so a field edit ends); a ruler click
//! locates, a ruler drag sets the time selection (scrolling near the
//! edges; Escape cancels it). The wheel scrolls (vertically together with
//! the headers) and, with the secondary modifier, zooms about the pointer.
//!
//! Below the ruler a press goes through the tool machine (Swift
//! `ArrangementView.handle`): a click selects a block (⇧ adds), on empty
//! space clears the selection (⇧ keeps it), a double click opens the
//! detail editor on the block, the Eraser deletes the clicked block's
//! events; an Arrow drag moves the block (⌥ copies, onto another row it
//! merges into that track) as one gesture, previewed live and scrolling
//! near the edges; Escape cancels it. A right click opens the block's
//! menu. The tool bar above holds the tool picker, Snap and its division
//! menu, and the zoom buttons. The maths and the drag are
//! `vizdaw_app`'s; this canvas only feeds them pointer events.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, BorderStyle, Bounds, ContentMask, Context, DispatchPhase, FocusHandle, Hsla,
    InteractiveElement, IntoElement, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, ScrollDelta, ScrollEvent,
    SharedString, StatefulInteractiveElement, Styled, Task, TextAlign, TextRun, Window, anchored,
    canvas, deferred, div, fill, point, px, quad, rgb, size,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{
    self as model, ArrangementFrame, ArrangementHit, AutoScroll, BlockDragState, Geometry,
    GridKind, MachineEvent, MeterBars, PageFollow, Phase, RULER_HEIGHT, Rect, RulerSelection,
    TimeAxis, ToolMachine, WheelScroll, ZOOM_STEP, a11y_label_in, bar_length, grid_lines_in,
    hit_test, label_every, sketch,
};
use vizdaw_app::editor::{DetailEditorKind, Division, EditorTarget, Snap, Tool};
use vizdaw_app::transport::TransportModel;
use vizdaw_ffi::blocks::{BlockKind, BlockRow};
use vizdaw_ffi::edit::{EditCommand, EventEdit};
use vizdaw_ffi::events::EventRow;
use vizdaw_ffi::selection::TickRange;

use crate::registry::{ChildRecorder, RegistryScope};
use crate::shell::DocumentWindow;
use crate::transport_bar::{self, colors as bar_colors};

/// The canvas's tree label (Swift `ArrangementView.accessibilityName`).
pub const GROUP: &str = "Arrangement";
pub const ZOOM_OUT: &str = "Zoom Out";
pub const ZOOM_IN: &str = "Zoom In";
/// The Snap toggle (Swift `Toggle("Snap")`) and the division menu's
/// toggles.
pub const SNAP: &str = "Snap";
pub const TRIPLET: &str = "Triplet";
pub const DOTTED: &str = "Dotted";
/// The block menu's items (Swift `ArrangementView.blockMenu`; the other
/// detail editors, Make Reference, Takes and the reference commands come
/// later).
pub const OPEN_IN_LIST: &str = "Open in List";
pub const DELETE: &str = "Delete";
/// The zoom buttons' row above the headers and the canvas.
pub const TOOLBAR_HEIGHT: f32 = 32.;

/// The fixed colours of this slice (themes come later).
pub mod colors {
    pub const CANVAS: u32 = 0x1e1e25;
    pub const RULER: u32 = 0x26262e;
    pub const BAR_LINE: u32 = 0x3c3c48;
    pub const BEAT_LINE: u32 = 0x2b2b34;
    pub const SEPARATOR: u32 = 0x3a3a46;
    pub const TICK: u32 = 0x9a9aa6;
    pub const LABEL: u32 = 0xe8e8ec;
    pub const SELECTION: u32 = 0x4a7dff;
    pub const REGION_FILL: u32 = 0x3d6fb8;
    pub const REGION_LINE: u32 = 0x77a3e6;
    pub const REGION_SELECTED: u32 = 0xffffff;
    pub const REFERENCE: u32 = 0x9a6fd6;
    pub const PLAYHEAD: u32 = 0xff5a4a;
    /// The block fill's alpha (a reference's is 0.30).
    pub const REGION_ALPHA: f32 = 0.55;
    pub const REFERENCE_ALPHA: f32 = 0.30;
    /// A muted block's fill is this much of the usual.
    pub const MUTED_ALPHA: f32 = 0.4;
    /// The selection's shade in the ruler and over the rows.
    pub const RULER_SHADE: f32 = 0.35;
    pub const BODY_SHADE: f32 = 0.10;
    /// The mini notes' alpha.
    pub const SKETCH_ALPHA: f32 = 0.55;
    pub const RADIUS: f32 = 4.;
    /// The piano keyboard's keys and labels (fixed until themes land).
    pub const WHITE_KEY: u32 = 0xe6e6ea;
    pub const BLACK_KEY: u32 = 0x18181d;
    pub const KEY_LABEL: u32 = 0x18181d;
    /// A black key's bar on the keyboard, and its row's shade in the roll
    /// (`LABEL` at this alpha).
    pub const BLACK_KEY_ALPHA: f32 = 0.85;
    pub const BLACK_ROW_ALPHA: f32 = 0.05;
    /// The track colours, in order (red, orange, yellow, green, teal,
    /// blue, purple, pink); a track takes its place in its sequence's
    /// tracks, modulo 8.
    pub const TRACK_SWATCHES: [u32; 8] = [
        0xe5534b, 0xf0883e, 0xe3b341, 0x56c26a, 0x3fb8af, 0x4a8fe7, 0x9a6fd6, 0xe56fb0,
    ];
    /// A note's fill alpha on the edit track, and on the others.
    pub const NOTE_ACTIVE_ALPHA: f32 = 0.85;
    pub const NOTE_ALPHA: f32 = 0.4;
}

/// How often auto-scroll steps while a ruler drag is near an edge.
const AUTO_SCROLL_EVERY: Duration = Duration::from_micros(16_667);

/// The canvas's state, owned by the document window.
pub struct Arrangement {
    focus: FocusHandle,
    axis: TimeAxis,
    vertical_offset: f32,
    drag: Option<RulerDrag>,
    /// The canvas's bounds at the last draw (zero before the first).
    layout: Rc<Cell<Bounds<Pixels>>>,
    /// The playhead's x in the canvas at the last draw, when shown.
    playhead: Rc<Cell<Option<f32>>>,
    rows_built: Cell<usize>,
    _auto_scroll: Option<Task<()>>,
    /// Presses below the ruler (Swift `ToolMachine`).
    machine: ToolMachine<ArrangementHit>,
    /// The block drag in progress.
    block_drag: Option<BlockDragState>,
    /// The pointer of a press below the ruler (body points) and its
    /// modifiers: auto-scroll moves a drag under a still pointer.
    pointer: Option<(model::Point, model::Modifiers)>,
    /// The block menu, while open.
    block_menu: Option<BlockMenu>,
    /// The tool bar's division menu is open.
    division_open: bool,
}

/// A right-clicked block's menu: the block, and where it opened.
struct BlockMenu {
    lane: u64,
    block: BlockRow,
    at: Point<Pixels>,
}

/// A press in the ruler: a click until it moves past the dead zone, then
/// a range drag.
struct RulerDrag {
    /// The tick pressed (kept as a tick, so it survives auto-scroll).
    anchor: i64,
    start_x: f32,
    /// The pointer's x in the canvas.
    x: f32,
    moved: bool,
    preview: Option<TickRange>,
}

impl Arrangement {
    pub fn new(cx: &mut Context<DocumentWindow>) -> Self {
        Arrangement {
            focus: cx.focus_handle(),
            axis: TimeAxis::default(),
            vertical_offset: 0.,
            drag: None,
            layout: Rc::default(),
            playhead: Rc::default(),
            rows_built: Cell::new(0),
            _auto_scroll: None,
            machine: ToolMachine::new(),
            block_drag: None,
            pointer: None,
            block_menu: None,
            division_open: false,
        }
    }

    /// How the rows are drawn now, for hit testing and drags.
    fn frame(&self) -> ArrangementFrame {
        ArrangementFrame {
            geometry: self.geometry(),
            axis: self.axis,
        }
    }

    /// Window point `p` in body points (the rows' space, below the ruler).
    fn body_point(&self, p: Point<Pixels>) -> model::Point {
        let o = self.layout.get().origin;
        model::Point {
            x: f32::from(p.x - o.x),
            y: f32::from(p.y - o.y) - RULER_HEIGHT,
        }
    }

    /// The tool bar's division menu is open.
    pub fn division_open(&self) -> bool {
        self.division_open
    }

    /// A press below the ruler is down (its release not seen yet).
    pub(crate) fn pressing(&self) -> bool {
        !matches!(self.machine.phase(), Phase::Idle)
    }

    /// Closes the block menu and the division menu; false when none was
    /// open.
    pub(crate) fn close_menus(&mut self) -> bool {
        let open = self.block_menu.is_some() || self.division_open;
        self.block_menu = None;
        self.division_open = false;
        open
    }

    /// How many rows the last frame built state for (only the visible
    /// ones): a test seam.
    #[doc(hidden)]
    pub fn rows_built(&self) -> usize {
        self.rows_built.get()
    }

    /// The rows of `count` tracks the body shows, partly or wholly, for a
    /// body at least `fallback` high (the window's height).
    pub(crate) fn visible_rows(&self, count: usize, fallback: f32) -> std::ops::Range<usize> {
        // Never fewer than the window could show: the canvas records its
        // height only while drawing, after the rows are built, so a canvas
        // that just grew would leave its new rows out.
        let body = self.body_height().max(fallback);
        let lane = Geometry::default().lane_height;
        let first = (self.vertical_offset / lane).floor().max(0.) as usize;
        let last = ((self.vertical_offset + body) / lane).ceil().max(0.) as usize;
        first.min(count)..last.min(count)
    }

    pub fn axis(&self) -> &TimeAxis {
        &self.axis
    }

    /// The rows' scroll, shared with the track headers.
    pub fn vertical_offset(&self) -> f32 {
        self.vertical_offset
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The playhead's x in the canvas as last drawn; `None` when hidden
    /// (another sequence plays, or it is off to a side).
    pub fn playhead_x(&self) -> Option<f32> {
        self.playhead.get()
    }

    /// The range a ruler drag is selecting.
    pub fn preview(&self) -> Option<TickRange> {
        self.drag.as_ref().and_then(|d| d.preview)
    }

    fn geometry(&self) -> Geometry {
        Geometry {
            vertical_offset: self.vertical_offset,
            ..Geometry::default()
        }
    }

    fn body_height(&self) -> f32 {
        (f32::from(self.layout.get().size.height) - RULER_HEIGHT).max(0.)
    }

    fn width(&self) -> f32 {
        f32::from(self.layout.get().size.width)
    }

    /// Keeps the vertical offset inside `rows` tracks and the add row.
    fn clamp_vertical(&mut self, rows: usize) {
        let body = self.body_height();
        self.vertical_offset = self
            .geometry()
            .clamp_vertical(self.vertical_offset, rows, body);
    }

    /// Before drawing: while playing the shown sequence, the view follows
    /// the playhead a page at a time.
    pub(crate) fn follow(&mut self, state: &ArrangementState) {
        let width = self.width();
        if state.playing
            && width > 0.
            && let Some(tick) = state.playhead
            && let Some(origin) = PageFollow::origin(tick, &self.axis, width)
        {
            self.axis = self.axis.scrolled_to(origin);
        }
    }

    /// The canvas, recorded in `scope` (the group and its blocks).
    pub(crate) fn render(
        &self,
        state: ArrangementState,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<DocumentWindow>,
    ) -> AnyElement {
        self.rows_built.set(state.rows.len());
        let mut group = scope.container();
        let mut recorder = group.children();
        let view = Scene {
            axis: self.axis,
            geometry: self.geometry(),
            range: self.preview().or(state.selection),
        };
        let layout = self.layout.clone();
        let playhead = self.playhead.clone();
        let weak = cx.entity().downgrade();
        let grown = weak.clone();
        let painter = canvas(
            move |bounds, _, cx| {
                // A new height shows other rows than were built for this
                // frame: draw again with them.
                if layout.get().size.height != bounds.size.height {
                    let grown = grown.clone();
                    cx.defer(move |cx| {
                        grown.update(cx, |_, cx| cx.notify()).ok();
                    });
                }
                layout.set(bounds);
                let prepared = view.prepare(&state, bounds, &mut recorder);
                playhead.set(prepared.time.playhead);
                prepared
            },
            move |bounds, prepared, window, cx| {
                prepared.paint(bounds, window, cx);
                // A ruler drag follows the pointer anywhere in the window.
                let moved = weak.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        moved
                            .update(cx, |this, cx| {
                                this.ruler_drag_to(e.position.x, cx);
                                this.block_drag_to(e.position, e.modifiers, cx);
                            })
                            .ok();
                    }
                });
                let released = weak.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                        released
                            .update(cx, |this, cx| {
                                this.ruler_release(cx);
                                this.block_release(e.position, e.modifiers, cx);
                            })
                            .ok();
                    }
                });
            },
        )
        .size_full();
        div()
            .id("arrangement")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            // A click takes the keyboard (GPUI focuses a tracked element on
            // mouse-down), which ends a field edit.
            .track_focus(&self.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| this.arrangement_mouse_down(e, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, e: &MouseDownEvent, _, cx| this.block_menu_press(e, cx)),
            )
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key != "escape" {
                    return;
                }
                if this.arrangement.drag.is_some() {
                    cx.stop_propagation();
                    this.cancel_ruler_drag(cx);
                } else if this.cancel_block_drag(cx) {
                    cx.stop_propagation();
                }
            }))
            .child(painter)
            .child(group.this("group", GROUP, true))
            .child(group.clip())
            .when_some(self.block_menu.as_ref(), |d, menu| {
                d.children(block_menu(menu, &mut scope.container(), window, cx))
            })
            .into_any_element()
    }
}

/// What the tool bar shows of the document.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolbarState {
    pub tool: Tool,
    pub snap: Snap,
}

impl ToolbarState {
    pub fn of(doc: &DocumentModel) -> Self {
        ToolbarState {
            tool: doc.tool(),
            snap: doc.layout().snap,
        }
    }
}

/// The open block menu: a backdrop that closes it, and its items at the
/// press.
fn block_menu(
    menu: &BlockMenu,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> Vec<AnyElement> {
    let size = window.viewport_size();
    let backdrop = deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("block-menu-backdrop")
                .w(size.width)
                .h(size.height)
                .occlude()
                .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                    this.arrangement.block_menu = None;
                    cx.notify();
                })),
        ),
    )
    .with_priority(1)
    .into_any_element();
    let mut items: Vec<Option<&'static str>> = vec![];
    if menu.block.kind == BlockKind::Events {
        items.extend([Some(OPEN_IN_LIST), None]);
    }
    items.push(Some(DELETE));
    let mut list = menu_list("block-menu");
    for (row, item) in items.into_iter().enumerate() {
        list = list.child(match item {
            None => menu_separator(),
            Some(title) => menu_item(("block-menu-item", row), title, scope)
                .on_click(cx.listener(move |this, _, _, cx| this.block_menu_choose(title, cx)))
                .into_any_element(),
        });
    }
    let list = deferred(anchored().position(menu.at).snap_to_window().child(list))
        .with_priority(2)
        .into_any_element();
    vec![backdrop, list]
}

pub(crate) fn menu_list(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .occlude()
        .flex()
        .flex_col()
        .min_w(px(160.))
        .py_1()
        .bg(rgb(bar_colors::LIST))
        .border_1()
        .border_color(rgb(bar_colors::BORDER))
        .rounded_md()
        .shadow_md()
        .text_color(rgb(bar_colors::TEXT))
}

pub(crate) fn menu_separator() -> AnyElement {
    div()
        .h(px(1.))
        .my_1()
        .bg(rgb(bar_colors::BORDER))
        .into_any_element()
}

fn menu_item(
    id: impl Into<gpui::ElementId>,
    title: &str,
    scope: &mut RegistryScope,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .relative()
        .px_3()
        .py_1()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(bar_colors::BUTTON)))
        .child(title.to_owned())
        .child(scope.control("menuitem", title, true))
}

/// A tool bar button showing `text`, recorded as a `button` labelled
/// `label` with `value` and `selected`.
pub(crate) fn bar_button(
    id: impl Into<gpui::ElementId>,
    text: &str,
    label: &str,
    width: f32,
    on: bool,
    value: Option<&str>,
    scope: &mut RegistryScope,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .relative()
        .h(px(24.))
        .w(px(width))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .text_xs()
        .bg(rgb(if on {
            bar_colors::ON
        } else {
            bar_colors::BUTTON
        }))
        .text_color(rgb(bar_colors::TEXT))
        .child(text.to_owned())
        .child(scope.control_selected("button", label, true, value, on && value.is_none()))
}

/// The segmented tool picker (Swift `Picker("Tool")`, labels hidden, 210
/// points wide): one button per tool, the current one selected. The
/// arrangement's tool bar and the piano roll's bar share the tool.
pub(crate) fn tool_picker(
    id: &'static str,
    current: Tool,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> gpui::Div {
    let mut picker = div().flex().flex_row().flex_none().gap(px(1.));
    for tool in Tool::ALL {
        picker = picker.child(
            bar_button(
                (id, tool as usize),
                tool.title(),
                tool.title(),
                70.,
                current == tool,
                None,
                scope,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.with_model(cx, |d| d.set_tool(tool));
            })),
        );
    }
    picker
}

/// The arrangement tool bar (Swift `ArrangementBar`, its tool, snap and
/// zoom parts): the tool picker, the Snap toggle and its division menu,
/// then the zoom buttons.
pub(crate) fn toolbar(
    state: ToolbarState,
    division_open: bool,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let picker = tool_picker("tool", state.tool, scope, cx);
    let snap = state.snap;
    let toggle = bar_button(
        "snap",
        SNAP,
        SNAP,
        52.,
        snap.enabled,
        Some(if snap.enabled { "on" } else { "off" }),
        scope,
    )
    .on_click(cx.listener(|this, _, _, cx| {
        this.with_model(cx, |d| {
            d.update_layout(|l| l.snap.enabled = !l.snap.enabled)
        });
    }));
    let title = snap.division.title();
    let mut division = bar_button("division", title, title, 70., false, None, scope).on_click(
        cx.listener(|this, _, _, cx| {
            this.arrangement.division_open = !this.arrangement.division_open;
            cx.notify();
        }),
    );
    if division_open {
        division = division.children(division_menu(snap, &mut scope.container(), window, cx));
    }
    let zoom_out = transport_bar::cluster_button(
        "zoom-out",
        "\u{2212}",
        ZOOM_OUT,
        true,
        bar_colors::BUTTON,
        scope,
    )
    .on_click(cx.listener(|this, _, _, cx| this.zoom_by(ZOOM_STEP, cx)));
    let zoom_in =
        transport_bar::cluster_button("zoom-in", "+", ZOOM_IN, true, bar_colors::BUTTON, scope)
            .on_click(cx.listener(|this, _, _, cx| this.zoom_by(1. / ZOOM_STEP, cx)));
    div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .justify_end()
        .gap(px(4.))
        .px(px(8.))
        .h(px(TOOLBAR_HEIGHT))
        .bg(rgb(bar_colors::BAR))
        .child(picker)
        .child(toggle)
        .child(division)
        .child(div().flex_1())
        .child(zoom_out)
        .child(zoom_in)
        .into_any_element()
}

/// The division menu (Swift's `Menu(snap.division.title)`): the note
/// values, then the Triplet and Dotted toggles.
fn division_menu(
    snap: Snap,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> Vec<AnyElement> {
    let size = window.viewport_size();
    let backdrop = deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("division-backdrop")
                .w(size.width)
                .h(size.height)
                .occlude()
                .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                    this.arrangement.division_open = false;
                    cx.notify();
                })),
        ),
    )
    .with_priority(1)
    .into_any_element();
    let mut list = menu_list("division-menu");
    for d in Division::ALL {
        let current = snap.division == d;
        list = list.child(
            div()
                .id(("division-item", d as usize))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(bar_colors::BUTTON)))
                .child(format!(
                    "{}{}",
                    if current { "\u{2713} " } else { "" },
                    d.title()
                ))
                .child(scope.control_selected("menuitem", d.title(), true, None, current))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.arrangement.division_open = false;
                    this.with_model(cx, |m| m.update_layout(|l| l.snap.division = d));
                })),
        );
    }
    list = list.child(menu_separator());
    for (row, (title, on)) in [(TRIPLET, snap.triplet), (DOTTED, snap.dotted)]
        .into_iter()
        .enumerate()
    {
        list = list.child(
            div()
                .id(("division-toggle", row))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(bar_colors::BUTTON)))
                .child(format!("{}{title}", if on { "\u{2713} " } else { "    " }))
                .child(scope.control_with(
                    "menuitem",
                    title,
                    true,
                    Some(if on { "on" } else { "off" }),
                    false,
                ))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.arrangement.division_open = false;
                    this.with_model(cx, |m| {
                        m.update_layout(|l| {
                            if title == TRIPLET {
                                l.snap.triplet = !l.snap.triplet;
                            } else {
                                l.snap.dotted = !l.snap.dotted;
                            }
                        })
                    });
                })),
        );
    }
    let list = div()
        .absolute()
        .top_full()
        .left_0()
        .child(deferred(anchored().snap_to_window().child(list)).with_priority(2))
        .into_any_element();
    vec![backdrop, list]
}

/// What the canvas draws of the document: only the rows in view, their
/// blocks and events shared with the document, not copied.
pub struct ArrangementState {
    pub rows: Vec<RowState>,
    /// The shown sequence's meter, its bar table built once a frame.
    pub bars: MeterBars,
    pub selection: Option<TickRange>,
    pub selected_events: Vec<u64>,
    /// The playhead's tick, when the transport is on the shown sequence.
    pub playhead: Option<f64>,
    /// Playing the shown sequence.
    pub playing: bool,
}

/// One track's row.
pub struct RowState {
    /// Its place among the shown sequence's tracks.
    pub index: usize,
    pub name: String,
    pub blocks: Arc<[BlockRow]>,
    pub events: Arc<[EventRow]>,
}

impl ArrangementState {
    /// Nothing to draw (no document).
    pub fn empty() -> Self {
        ArrangementState {
            rows: vec![],
            bars: MeterBars::new(&[]),
            selection: None,
            selected_events: vec![],
            playhead: None,
            playing: false,
        }
    }

    /// The `visible` tracks of `doc`'s shown sequence, with `transport`'s
    /// playhead at `now` (extrapolated from the last poll while playing).
    pub fn of(
        doc: &DocumentModel,
        transport: Option<&TransportModel>,
        now: Instant,
        visible: std::ops::Range<usize>,
    ) -> Self {
        let seq = doc.selected_seq();
        let lanes = doc.lanes();
        let rows = lanes
            .get(visible.clone())
            .unwrap_or_default()
            .iter()
            .zip(visible)
            .map(|(l, index)| {
                let blocks = doc.blocks(l.id);
                let sketched = blocks.iter().any(|b| b.kind == BlockKind::Events);
                RowState {
                    index,
                    name: l.name.clone(),
                    events: if sketched {
                        doc.shown_lane_events(l.id)
                    } else {
                        Arc::from([])
                    },
                    blocks,
                }
            })
            .collect();
        let bars = MeterBars::new(
            seq.and_then(|s| doc.sequence_info(s))
                .map_or(&[][..], |i| i.meter.as_slice()),
        );
        let (playhead, playing) = match transport {
            Some(t) if seq.is_some() && t.state().seq == seq => {
                let tick = if t.clock().last().is_some() {
                    t.clock().tick_at(now)
                } else {
                    t.state().tick
                };
                (Some(tick as f64), t.state().playing)
            }
            _ => (None, false),
        };
        ArrangementState {
            rows,
            bars,
            selection: doc.selection().time_range,
            selected_events: doc.selection().events.clone(),
            playhead,
            playing,
        }
    }
}

/// The view's part of a frame: where it looks.
#[derive(Clone, Copy)]
struct Scene {
    axis: TimeAxis,
    geometry: Geometry,
    range: Option<TickRange>,
}

/// One frame, laid out in canvas points (y = 0 is the ruler's top).
struct Prepared {
    time: TimeLayer,
    separators: Vec<f32>,
    blocks: Vec<BlockDraw>,
}

/// The time parts every editor canvas draws (Swift `EditorCanvas`): the
/// grid, the ruler's ticks and bar numbers, the selected range's shade
/// and the playhead, laid out in canvas points (y = 0 is the ruler's
/// top, the rows start at `RULER_HEIGHT`).
pub(crate) struct TimeLayer {
    width: f32,
    height: f32,
    grid: Vec<(f32, GridKind)>,
    ruler: Vec<(f32, GridKind)>,
    labels: Vec<(f32, String)>,
    shade: Option<(f32, f32)>,
    /// The playhead's x, when it shows.
    pub(crate) playhead: Option<f32>,
}

/// A rectangle in canvas points as window bounds, from the canvas origin.
pub(crate) fn canvas_rect(o: Point<Pixels>, x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(point(o.x + px(x), o.y + px(y)), size(px(w), px(h)))
}

impl TimeLayer {
    /// The lines, labels, shade (`range`) and playhead (at tick
    /// `playhead`) of a canvas `width` × `height` showing `axis`.
    pub(crate) fn new(
        axis: &TimeAxis,
        bars: &MeterBars,
        width: f32,
        height: f32,
        range: Option<TickRange>,
        playhead: Option<f64>,
    ) -> Self {
        let w = width;
        let tpp = axis.ticks_per_point();
        let snap = |tick: i64| axis.x(tick).round();
        let grid = grid_lines_in(bars, axis.tick_at(-1.), axis.tick_at(w + 1.), tpp)
            .into_iter()
            .map(|l| (snap(l.tick), l.kind))
            .collect();
        let every = label_every(bar_length(&bars.first()), tpp);
        let lines = grid_lines_in(bars, axis.tick_at(-60.), axis.tick_at(w), tpp);
        let labels = lines
            .iter()
            .filter_map(|l| match l.kind {
                GridKind::Bar(n) if (n - 1).div_euclid(every) * every == n - 1 => {
                    Some((snap(l.tick) + 3., n.to_string()))
                }
                _ => None,
            })
            .collect();
        let ruler = lines.into_iter().map(|l| (snap(l.tick), l.kind)).collect();
        let shade = range.map(|r| {
            let x0 = axis.x(r.start);
            (x0, (axis.x(r.end) - x0).max(1.))
        });
        let playhead = playhead.and_then(|tick| {
            let x = ((tick - axis.origin() as f64) / tpp).round() as f32;
            (x >= -1. && x <= w + 1.).then_some(x)
        });
        TimeLayer {
            width,
            height,
            grid,
            ruler,
            labels,
            shade,
            playhead,
        }
    }

    fn body_height(&self) -> f32 {
        (self.height - RULER_HEIGHT).max(0.)
    }

    /// The rows' area in window bounds.
    pub(crate) fn body(&self, o: Point<Pixels>) -> Bounds<Pixels> {
        canvas_rect(o, 0., RULER_HEIGHT, self.width, self.body_height())
    }

    /// Under the rows: the canvas colour and the grid lines.
    pub(crate) fn paint_grid(&self, o: Point<Pixels>, window: &mut Window) {
        let body_h = self.body_height();
        window.paint_quad(fill(self.body(o), rgb(colors::CANVAS)));
        for (x, kind) in &self.grid {
            let c = match kind {
                GridKind::Bar(_) => colors::BAR_LINE,
                GridKind::Beat => colors::BEAT_LINE,
            };
            window.paint_quad(fill(canvas_rect(o, *x, RULER_HEIGHT, 1., body_h), rgb(c)));
        }
    }

    /// Over the rows: the selected range's shade and the playhead.
    pub(crate) fn paint_overlay(&self, o: Point<Pixels>, window: &mut Window) {
        let body_h = self.body_height();
        if let Some((x, width)) = self.shade {
            window.paint_quad(fill(
                canvas_rect(o, x, RULER_HEIGHT, width, body_h),
                hsla(colors::SELECTION, colors::BODY_SHADE),
            ));
        }
        if let Some(x) = self.playhead {
            window.paint_quad(fill(
                canvas_rect(o, x, RULER_HEIGHT, 1., body_h),
                rgb(colors::PLAYHEAD),
            ));
        }
    }

    /// The ruler: shade, bar and beat ticks, bar numbers, its bottom line.
    pub(crate) fn paint_ruler(&self, o: Point<Pixels>, window: &mut Window, cx: &mut App) {
        let w = self.width;
        let ruler = canvas_rect(o, 0., 0., w, RULER_HEIGHT);
        window.with_content_mask(Some(ContentMask { bounds: ruler }), |window| {
            window.paint_quad(fill(ruler, rgb(colors::RULER)));
            if let Some((x, width)) = self.shade {
                window.paint_quad(fill(
                    canvas_rect(o, x, 0., width, RULER_HEIGHT),
                    hsla(colors::SELECTION, colors::RULER_SHADE),
                ));
            }
            for (x, kind) in &self.ruler {
                let from = match kind {
                    GridKind::Bar(_) => RULER_HEIGHT * 0.35,
                    GridKind::Beat => RULER_HEIGHT * 0.7,
                };
                window.paint_quad(fill(
                    canvas_rect(o, *x, from, 1., RULER_HEIGHT - from),
                    rgb(colors::TICK),
                ));
            }
            for (x, text) in &self.labels {
                paint_text(text, o.x + px(*x), o.y + px(2.), colors::LABEL, window, cx);
            }
            window.paint_quad(fill(
                canvas_rect(o, 0., RULER_HEIGHT - 1., w, 1.),
                rgb(colors::SEPARATOR),
            ));
        });
    }
}

struct BlockDraw {
    rect: Rect,
    reference: bool,
    muted: bool,
    selected: bool,
    label: Option<String>,
    sketch: Vec<Rect>,
}

impl Scene {
    /// Lays out the frame in `bounds` and records the visible blocks.
    fn prepare(
        &self,
        state: &ArrangementState,
        bounds: Bounds<Pixels>,
        recorder: &mut ChildRecorder,
    ) -> Prepared {
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let axis = &self.axis;
        let body = Rect {
            x: 0.,
            y: RULER_HEIGHT,
            width: w,
            height: (h - RULER_HEIGHT).max(0.),
        };

        let g = &self.geometry;
        let mut separators = vec![];
        let mut blocks = vec![];
        for row in &state.rows {
            let i = row.index;
            let top = RULER_HEIGHT + g.row_top(i);
            if top + g.lane_height < RULER_HEIGHT || top > h {
                continue;
            }
            separators.push(top + g.lane_height - 1.);
            for b in row.blocks.iter() {
                let mut rect = g.block_rect(b, i, axis);
                rect.y += RULER_HEIGHT;
                if !rect.intersects(&body) {
                    continue;
                }
                let drawn = self.block(b, rect, row, state);
                recorder.record(
                    "button",
                    &a11y_label_in(b, &row.name, &state.bars),
                    Bounds::new(
                        point(bounds.origin.x + px(rect.x), bounds.origin.y + px(rect.y)),
                        size(px(rect.width), px(rect.height)),
                    ),
                    true,
                    drawn.selected,
                );
                blocks.push(drawn);
            }
        }
        Prepared {
            time: TimeLayer::new(axis, &state.bars, w, h, self.range, state.playhead),
            separators,
            blocks,
        }
    }

    fn block(
        &self,
        b: &BlockRow,
        rect: Rect,
        row: &RowState,
        state: &ArrangementState,
    ) -> BlockDraw {
        let reference = b.kind == BlockKind::Reference;
        let selected =
            !b.events.is_empty() && b.events.iter().all(|e| state.selected_events.contains(e));
        let wide = rect.width > 24.;
        BlockDraw {
            rect,
            reference,
            muted: b.muted,
            selected,
            label: (reference && wide)
                .then(|| format!("\u{21bb} {}", b.label.as_deref().unwrap_or(""))),
            sketch: if !reference && wide {
                sketch(b, &rect, &row.events, &self.axis)
            } else {
                vec![]
            },
        }
    }
}

pub(crate) fn hsla(c: u32, alpha: f32) -> Hsla {
    Hsla::from(rgb(c)).opacity(alpha)
}

impl Prepared {
    fn paint(self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let o = bounds.origin;
        let rect = |x: f32, y: f32, w: f32, h: f32| canvas_rect(o, x, y, w, h);
        let w = self.time.width;
        let body = self.time.body(o);

        // The rows: canvas, grid, separators, blocks, shade, playhead.
        window.with_content_mask(Some(ContentMask { bounds: body }), |window| {
            self.time.paint_grid(o, window);
            for y in &self.separators {
                window.paint_quad(fill(rect(0., *y, w, 1.), rgb(colors::SEPARATOR)));
            }
            for b in &self.blocks {
                paint_block(b, &rect, window, cx);
            }
            self.time.paint_overlay(o, window);
        });
        self.time.paint_ruler(o, window, cx);
    }
}

fn paint_block(
    b: &BlockDraw,
    rect: &impl Fn(f32, f32, f32, f32) -> Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let r = b.rect;
    let (fill_colour, fill_alpha, line) = if b.reference {
        (
            colors::REFERENCE,
            colors::REFERENCE_ALPHA,
            colors::REFERENCE,
        )
    } else {
        (
            colors::REGION_FILL,
            colors::REGION_ALPHA,
            colors::REGION_LINE,
        )
    };
    let alpha = fill_alpha * if b.muted { colors::MUTED_ALPHA } else { 1. };
    let (line, width) = if b.selected {
        (colors::REGION_SELECTED, 2.)
    } else {
        (line, 1.)
    };
    window.paint_quad(quad(
        rect(r.x + 0.5, r.y + 0.5, r.width - 1., r.height - 1.),
        px(colors::RADIUS),
        hsla(fill_colour, alpha),
        px(width),
        rgb(line),
        if b.muted {
            BorderStyle::Dashed
        } else {
            BorderStyle::Solid
        },
    ));
    let inside = rect(r.x, r.y, r.width, r.height);
    for n in &b.sketch {
        window.paint_quad(fill(
            rect(n.x, n.y, n.width, n.height),
            hsla(colors::LABEL, colors::SKETCH_ALPHA),
        ));
    }
    if let Some(text) = &b.label {
        let clip = rect(
            r.x + 3.,
            r.y + 1.,
            (r.width - 6.).max(0.),
            (r.height - 2.).max(0.),
        );
        window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
            paint_text(
                text,
                inside.origin.x + px(4.),
                inside.origin.y + px(3.),
                colors::LABEL,
                window,
                cx,
            );
        });
    }
}

pub(crate) fn paint_text(
    text: &str,
    x: Pixels,
    y: Pixels,
    colour: u32,
    window: &mut Window,
    cx: &mut App,
) {
    let font = window.text_style().font();
    let run = TextRun {
        len: text.len(),
        font,
        color: rgb(colour).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line =
        window
            .text_system()
            .shape_line(SharedString::from(text.to_owned()), px(10.), &[run], None);
    line.paint(point(x, y), px(14.), TextAlign::Left, None, window, cx)
        .ok();
}

impl DocumentWindow {
    /// The arrangement canvas's state.
    pub fn arrangement(&self) -> &Arrangement {
        &self.arrangement
    }

    pub(crate) fn shown_rows(&self, cx: &App) -> usize {
        let s = self.app().read(cx);
        s.store.document(self.doc()).map_or(0, |d| d.lanes().len())
    }

    /// A press in the canvas: in the ruler it starts a click or a range
    /// drag; below it, a press of the tool machine on what it hits.
    fn arrangement_mouse_down(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) {
        let b = self.arrangement.layout.get();
        let x = f32::from(e.position.x - b.origin.x);
        let y = f32::from(e.position.y - b.origin.y);
        if y < RULER_HEIGHT {
            self.arrangement.drag = Some(RulerDrag {
                anchor: self.arrangement.axis.tick_at(x),
                start_x: x,
                x,
                moved: false,
                preview: None,
            });
            cx.notify();
            return;
        }
        let at = self.arrangement.body_point(e.position);
        let modifiers = machine_modifiers(e.modifiers);
        let Some(hit) = self.hit(at, cx) else { return };
        let clicks = u32::try_from(e.click_count).unwrap_or(u32::MAX).max(1);
        let events = self.arrangement.machine.down(hit, at, clicks, modifiers);
        self.arrangement.pointer = Some((at, modifiers));
        self.machine_events(events, cx);
        cx.notify();
    }

    /// What body point `at` lands on, with the blocks as the document
    /// holds them now.
    fn hit(&self, at: model::Point, cx: &App) -> Option<ArrangementHit> {
        let s = self.app().read(cx);
        let d = s.store.document(self.doc())?;
        let rows: Vec<(u64, Arc<[BlockRow]>)> =
            d.lanes().iter().map(|l| (l.id, d.blocks(l.id))).collect();
        let frame = self.arrangement.frame();
        Some(hit_test(&frame.geometry, &frame.axis, &rows, at))
    }

    /// The machine's events: the drag part goes to the document's block
    /// drag, clicks to [`Self::arrangement_click`].
    fn machine_events(
        &mut self,
        events: Vec<MachineEvent<ArrangementHit>>,
        cx: &mut Context<Self>,
    ) {
        if events.is_empty() {
            return;
        }
        let frame = self.arrangement.frame();
        let mut drag = self.arrangement.block_drag.take();
        self.with_model(cx, |d| d.block_drag_events(&mut drag, &events, &frame));
        self.arrangement.block_drag = drag;
        for e in events {
            if let MachineEvent::Click {
                hit,
                clicks,
                modifiers,
                ..
            } = e
            {
                self.arrangement_click(hit, clicks, modifiers, cx);
            }
        }
    }

    /// A click below the ruler (Swift `ArrangementView.click`). The
    /// Pencil does nothing yet (it opens the piano roll in a later slice).
    fn arrangement_click(
        &mut self,
        hit: ArrangementHit,
        clicks: u32,
        modifiers: model::Modifiers,
        cx: &mut Context<Self>,
    ) {
        self.with_model(cx, |d| {
            let tool = d.tool();
            if tool == Tool::Pencil {
                return;
            }
            match hit {
                ArrangementHit::Block { lane, index } => {
                    let Some(block) = d.blocks(lane).get(index).cloned() else {
                        return;
                    };
                    if tool == Tool::Eraser {
                        d.edit(delete(&block));
                    } else if clicks >= 2 {
                        // A reference opens its sequence in a later slice.
                        if block.kind == BlockKind::Events {
                            open_block_detail(d, lane, &block, None);
                        }
                    } else {
                        d.select_block(lane, &block, modifiers.shift);
                    }
                }
                ArrangementHit::Empty { .. } => {
                    if !modifiers.shift {
                        d.clear_selection();
                    }
                }
            }
        });
    }

    /// The pointer moved to window point `at` during a press below the
    /// ruler: the machine drags (past the dead zone a drag begins), and
    /// near an edge the view scrolls.
    fn block_drag_to(&mut self, at: Point<Pixels>, m: Modifiers, cx: &mut Context<Self>) {
        if !self.arrangement.pressing() {
            return;
        }
        let at = self.arrangement.body_point(at);
        let modifiers = machine_modifiers(m);
        self.arrangement.pointer = Some((at, modifiers));
        let events = self.arrangement.machine.drag(at, modifiers);
        self.machine_events(events, cx);
        let a = &self.arrangement;
        if matches!(a.machine.phase(), Phase::Dragging { .. })
            && AutoScroll::speed(at.x, a.width()) != 0.
        {
            self.start_auto_scroll(cx);
        }
        cx.notify();
    }

    /// The button is up at window point `at`: a click, or the drag's end.
    fn block_release(&mut self, at: Point<Pixels>, m: Modifiers, cx: &mut Context<Self>) {
        if !self.arrangement.pressing() {
            return;
        }
        let at = self.arrangement.body_point(at);
        self.arrangement.pointer = None;
        self.arrangement._auto_scroll = None;
        let events = self.arrangement.machine.up(at, machine_modifiers(m));
        self.machine_events(events, cx);
        cx.notify();
    }

    /// Escape below the ruler: the drag is cancelled, and so is a drag
    /// whose final edit still waits on Busy; the mouse-up is then
    /// swallowed. Whether there was anything to cancel.
    pub(crate) fn cancel_block_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let open = {
            let s = self.app().read(cx);
            s.store
                .document(self.doc())
                .is_some_and(DocumentModel::gesture_open)
        };
        if !self.arrangement.pressing() && !open {
            return false;
        }
        let frame = self.arrangement.frame();
        let mut machine = std::mem::take(&mut self.arrangement.machine);
        let mut drag = self.arrangement.block_drag.take();
        self.with_model(cx, |d| d.cancel_block_drag(&mut machine, &mut drag, &frame));
        self.arrangement.machine = machine;
        self.arrangement.block_drag = drag;
        self.arrangement.pointer = None;
        self.arrangement._auto_scroll = None;
        cx.notify();
        true
    }

    /// A right press below the ruler: on a block, the block is selected
    /// (unless it already is) and its menu opens there.
    fn block_menu_press(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) {
        let at = self.arrangement.body_point(e.position);
        self.arrangement.block_menu = None;
        if at.y < 0. {
            return;
        }
        let Some(ArrangementHit::Block { lane, index }) = self.hit(at, cx) else {
            cx.notify();
            return;
        };
        let block = self.with_model(cx, |d| {
            let block = d.blocks(lane).get(index).cloned()?;
            if !d.is_selected(&block) {
                d.select_block(lane, &block, false);
            }
            Some(block)
        });
        if let Some(Some(block)) = block {
            self.arrangement.block_menu = Some(BlockMenu {
                lane,
                block,
                at: e.position,
            });
        }
        cx.notify();
    }

    /// A block menu item.
    fn block_menu_choose(&mut self, title: &str, cx: &mut Context<Self>) {
        let Some(menu) = self.arrangement.block_menu.take() else {
            return;
        };
        self.with_model(cx, |d| match title {
            OPEN_IN_LIST => {
                open_block_detail(d, menu.lane, &menu.block, Some(DetailEditorKind::List))
            }
            _ => {
                d.edit(delete(&menu.block));
            }
        });
        cx.notify();
    }

    fn start_auto_scroll(&mut self, cx: &mut Context<Self>) {
        if self.arrangement._auto_scroll.is_some() {
            return;
        }
        self.arrangement._auto_scroll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(AUTO_SCROLL_EVERY).await;
                let going = this.update(cx, |this, cx| this.auto_scroll_step(cx));
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        }));
    }

    /// The pointer moved to window x `at` during a ruler press.
    fn ruler_drag_to(&mut self, at: Pixels, cx: &mut Context<Self>) {
        let origin = self.arrangement.layout.get().origin.x;
        let a = &mut self.arrangement;
        let Some(d) = a.drag.as_mut() else { return };
        d.x = f32::from(at - origin);
        if !d.moved {
            if (d.x - d.start_x).abs() <= vizdaw_app::arrangement::DEAD_ZONE {
                return;
            }
            d.moved = true;
        }
        d.preview = RulerSelection::range(d.anchor, d.x, &a.axis);
        if AutoScroll::speed(d.x, a.width()) != 0. {
            self.start_auto_scroll(cx);
        }
        cx.notify();
    }

    /// One auto-scroll step; false once the drag has ended or the pointer
    /// left the edges. A block drag runs again with the pointer where it
    /// is, so the block follows the scrolled view.
    fn auto_scroll_step(&mut self, cx: &mut Context<Self>) -> bool {
        let a = &mut self.arrangement;
        let width = a.width();
        if a.drag.is_none()
            && matches!(a.machine.phase(), Phase::Dragging { .. })
            && let Some((at, modifiers)) = a.pointer
        {
            let v = AutoScroll::speed(at.x, width);
            if v == 0. {
                a._auto_scroll = None;
                return false;
            }
            a.axis = a.axis.scrolled_by(v / 60.);
            let events = a.machine.drag(at, modifiers);
            self.machine_events(events, cx);
            cx.notify();
            return true;
        }
        let Some(d) = a.drag.as_mut() else {
            a._auto_scroll = None;
            return false;
        };
        let v = AutoScroll::speed(d.x, width);
        if v == 0. {
            a._auto_scroll = None;
            return false;
        }
        a.axis = a.axis.scrolled_by(v / 60.);
        d.preview = RulerSelection::range(d.anchor, d.x, &a.axis);
        cx.notify();
        true
    }

    /// The press ends: a click locates, a drag selects its range over
    /// every track.
    fn ruler_release(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.arrangement.drag.take() else {
            return;
        };
        self.arrangement._auto_scroll = None;
        if d.moved {
            if let Some(range) = d.preview {
                self.with_model(cx, |m| m.select_range(range));
            }
        } else {
            self.with_model(cx, |m| m.locate(d.anchor));
            let doc = self.doc();
            self.app().update(cx, |s, cx| {
                s.poll(doc);
                cx.notify();
            });
        }
        cx.notify();
    }

    /// Escape, or the window losing the keyboard: the press is dropped.
    pub(crate) fn cancel_ruler_drag(&mut self, cx: &mut Context<Self>) {
        if self.arrangement.drag.take().is_some() {
            self.arrangement._auto_scroll = None;
            cx.notify();
        }
    }

    /// The wheel over the headers or the canvas: scrolls both rows
    /// together and the canvas sideways; with the secondary modifier it
    /// zooms about the pointer.
    pub(crate) fn arrangement_wheel(&mut self, e: &ScrollEvent, cx: &mut Context<Self>) {
        let (precise, dx, dy) = match e.delta {
            ScrollDelta::Pixels(p) => (true, f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(p) => (false, p.x, p.y),
        };
        let rows = self.shown_rows(cx);
        let a = &mut self.arrangement;
        if e.modifiers.secondary(cx) {
            let x = f32::from(e.position.x - a.layout.get().origin.x);
            a.axis = a.axis.zoom_about(WheelScroll::zoom_factor(dy, precise), x);
        } else {
            let (dx, dy) = WheelScroll::points(dx, dy, precise);
            a.axis = a.axis.scrolled_by(-dx);
            a.vertical_offset -= dy;
            a.clamp_vertical(rows);
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// A zoom button: the zoom times `factor`, the origin kept.
    fn zoom_by(&mut self, factor: f64, cx: &mut Context<Self>) {
        let a = &mut self.arrangement;
        a.axis = a.axis.with_zoom(a.axis.ticks_per_point() * factor);
        cx.notify();
    }

    /// The rows changed (a track went): the offset stays in the content.
    pub(crate) fn clamp_arrangement(&mut self, rows: usize) {
        if self.arrangement.layout.get().size.height > px(0.) {
            self.arrangement.clamp_vertical(rows);
        }
    }
}

/// GPUI's modifiers as the tool machine's: ⌥ is `alt`, ⌘ `platform`.
pub(crate) fn machine_modifiers(m: Modifiers) -> model::Modifiers {
    model::Modifiers {
        shift: m.shift,
        control: m.control,
        option: m.alt,
        command: m.platform,
    }
}

/// Deleting `block`'s events (the Eraser, the block menu's Delete).
fn delete(block: &BlockRow) -> EditCommand {
    EditCommand::Event {
        edit: EventEdit::Delete {
            events: block.events.clone(),
        },
    }
}

/// Opens the detail editor on `block` of track `lane`: in `kind` (which
/// becomes the last-used editor), else in the last-used editor. The List
/// is the only one this slice has, so a last-used editor that is not here
/// yet opens the List and stays the saved last-used one.
fn open_block_detail(
    d: &mut DocumentModel,
    lane: u64,
    block: &BlockRow,
    kind: Option<DetailEditorKind>,
) {
    let Some(seq) = d.selected_seq() else { return };
    let last = d.layout().last_kind;
    let target = EditorTarget::Block {
        seq,
        lane,
        start: block.start,
        end: block.end,
    };
    match kind {
        Some(k) => d.open_detail(target, Some(k)),
        None => {
            d.open_detail(target, Some(DetailEditorKind::List));
            d.update_layout(|l| l.last_kind = last);
        }
    }
}
