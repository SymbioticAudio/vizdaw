//! The arrangement canvas beside the track headers (Swift `EditorCanvas`
//! and `ArrangementView`): a 22 px ruler over the grid, row separators,
//! the shown sequence's blocks, the selection's range shaded and the
//! playhead. It records as a `group` "Arrangement" whose children are one
//! `button` per visible block, labelled as Swift labels it for VoiceOver.
//!
//! A click takes the keyboard (so a field edit ends); a ruler click
//! locates, a ruler drag sets the time selection (scrolling near the
//! edges; Escape cancels it). The wheel scrolls (vertically together with
//! the headers) and, with the secondary modifier, zooms about the pointer;
//! the Zoom Out and In buttons above step by 1.5. Blocks are drawn and
//! listed, not yet editable. The maths are `vizdaw_app::arrangement`'s.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, BorderStyle, Bounds, ContentMask, Context, DispatchPhase, FocusHandle, Hsla,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, ScrollDelta, ScrollEvent, SharedString,
    StatefulInteractiveElement, Styled, Task, TextAlign, TextRun, Window, canvas, div, fill, point,
    px, quad, rgb, size,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{
    AutoScroll, Geometry, GridKind, MeterBars, PageFollow, RULER_HEIGHT, Rect, RulerSelection,
    TimeAxis, WheelScroll, ZOOM_STEP, a11y_label_in, bar_length, grid_lines_in, label_every,
    sketch,
};
use vizdaw_app::transport::TransportModel;
use vizdaw_ffi::blocks::{BlockKind, BlockRow};
use vizdaw_ffi::events::EventRow;
use vizdaw_ffi::selection::TickRange;

use crate::registry::{ChildRecorder, RegistryScope};
use crate::shell::DocumentWindow;
use crate::transport_bar::{self, colors as bar_colors};

/// The canvas's tree label (Swift `ArrangementView.accessibilityName`).
pub const GROUP: &str = "Arrangement";
pub const ZOOM_OUT: &str = "Zoom Out";
pub const ZOOM_IN: &str = "Zoom In";
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
        }
    }

    /// How many rows the last frame built state for (only the visible
    /// ones): a test seam.
    #[doc(hidden)]
    pub fn rows_built(&self) -> usize {
        self.rows_built.get()
    }

    /// The rows of `count` tracks the body shows, partly or wholly; before
    /// the first layout the body is taken as `fallback` high.
    pub(crate) fn visible_rows(&self, count: usize, fallback: f32) -> std::ops::Range<usize> {
        let body = if self.layout.get().size.height > px(0.) {
            self.body_height()
        } else {
            fallback
        };
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
        let painter = canvas(
            move |bounds, _, _| {
                layout.set(bounds);
                let prepared = view.prepare(&state, bounds, &mut recorder);
                playhead.set(prepared.playhead);
                prepared
            },
            move |bounds, prepared, window, cx| {
                prepared.paint(bounds, window, cx);
                // A ruler drag follows the pointer anywhere in the window.
                let moved = weak.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        moved
                            .update(cx, |this, cx| this.ruler_drag_to(e.position.x, cx))
                            .ok();
                    }
                });
                let released = weak.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                        released.update(cx, |this, cx| this.ruler_release(cx)).ok();
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
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" && this.arrangement.drag.is_some() {
                    cx.stop_propagation();
                    this.cancel_ruler_drag(cx);
                }
            }))
            .child(painter)
            .child(group.this("group", GROUP, true))
            .child(group.clip())
            .into_any_element()
    }
}

/// The zoom buttons' row (Swift's arrangement tool bar, its zoom part).
pub(crate) fn toolbar(scope: &mut RegistryScope, cx: &mut Context<DocumentWindow>) -> AnyElement {
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
        .child(zoom_out)
        .child(zoom_in)
        .into_any_element()
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
                        doc.lane_events(l.id)
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
    width: f32,
    height: f32,
    grid: Vec<(f32, GridKind)>,
    ruler: Vec<(f32, GridKind)>,
    labels: Vec<(f32, String)>,
    separators: Vec<f32>,
    blocks: Vec<BlockDraw>,
    shade: Option<(f32, f32)>,
    playhead: Option<f32>,
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
        let tpp = axis.ticks_per_point();
        let snap = |tick: i64| axis.x(tick).round();
        let body = Rect {
            x: 0.,
            y: RULER_HEIGHT,
            width: w,
            height: (h - RULER_HEIGHT).max(0.),
        };
        let grid = grid_lines_in(&state.bars, axis.tick_at(-1.), axis.tick_at(w + 1.), tpp)
            .into_iter()
            .map(|l| (snap(l.tick), l.kind))
            .collect();
        let every = label_every(bar_length(&state.bars.first()), tpp);
        let lines = grid_lines_in(&state.bars, axis.tick_at(-60.), axis.tick_at(w), tpp);
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
                recorder.record(
                    "button",
                    &a11y_label_in(b, &row.name, &state.bars),
                    Bounds::new(
                        point(bounds.origin.x + px(rect.x), bounds.origin.y + px(rect.y)),
                        size(px(rect.width), px(rect.height)),
                    ),
                    true,
                );
                blocks.push(self.block(b, rect, row, state));
            }
        }
        let shade = self.range.map(|r| {
            let x0 = axis.x(r.start);
            (x0, (axis.x(r.end) - x0).max(1.))
        });
        let playhead = state.playhead.and_then(|tick| {
            let x = ((tick - axis.origin() as f64) / tpp).round() as f32;
            (x >= -1. && x <= w + 1.).then_some(x)
        });
        Prepared {
            width: w,
            height: h,
            grid,
            ruler,
            labels,
            separators,
            blocks,
            shade,
            playhead,
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

fn hsla(c: u32, alpha: f32) -> Hsla {
    Hsla::from(rgb(c)).opacity(alpha)
}

impl Prepared {
    fn paint(self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let o = bounds.origin;
        let rect = |x: f32, y: f32, w: f32, h: f32| {
            Bounds::new(point(o.x + px(x), o.y + px(y)), size(px(w), px(h)))
        };
        let (w, h) = (self.width, self.height);
        let body_h = (h - RULER_HEIGHT).max(0.);
        let ruler = rect(0., 0., w, RULER_HEIGHT);
        let body = rect(0., RULER_HEIGHT, w, body_h);

        // The rows: canvas, grid, separators, blocks, shade, playhead.
        window.with_content_mask(Some(ContentMask { bounds: body }), |window| {
            window.paint_quad(fill(body, rgb(colors::CANVAS)));
            for (x, kind) in &self.grid {
                let c = match kind {
                    GridKind::Bar(_) => colors::BAR_LINE,
                    GridKind::Beat => colors::BEAT_LINE,
                };
                window.paint_quad(fill(rect(*x, RULER_HEIGHT, 1., body_h), rgb(c)));
            }
            for y in &self.separators {
                window.paint_quad(fill(rect(0., *y, w, 1.), rgb(colors::SEPARATOR)));
            }
            for b in &self.blocks {
                paint_block(b, &rect, window, cx);
            }
            if let Some((x, width)) = self.shade {
                window.paint_quad(fill(
                    rect(x, RULER_HEIGHT, width, body_h),
                    hsla(colors::SELECTION, colors::BODY_SHADE),
                ));
            }
            if let Some(x) = self.playhead {
                window.paint_quad(fill(
                    rect(x, RULER_HEIGHT, 1., body_h),
                    rgb(colors::PLAYHEAD),
                ));
            }
        });

        // The ruler: shade, bar and beat ticks, bar numbers, its bottom line.
        window.with_content_mask(Some(ContentMask { bounds: ruler }), |window| {
            window.paint_quad(fill(ruler, rgb(colors::RULER)));
            if let Some((x, width)) = self.shade {
                window.paint_quad(fill(
                    rect(x, 0., width, RULER_HEIGHT),
                    hsla(colors::SELECTION, colors::RULER_SHADE),
                ));
            }
            for (x, kind) in &self.ruler {
                let from = match kind {
                    GridKind::Bar(_) => RULER_HEIGHT * 0.35,
                    GridKind::Beat => RULER_HEIGHT * 0.7,
                };
                window.paint_quad(fill(
                    rect(*x, from, 1., RULER_HEIGHT - from),
                    rgb(colors::TICK),
                ));
            }
            for (x, text) in &self.labels {
                paint_text(text, o.x + px(*x), o.y + px(2.), colors::LABEL, window, cx);
            }
            window.paint_quad(fill(
                rect(0., RULER_HEIGHT - 1., w, 1.),
                rgb(colors::SEPARATOR),
            ));
        });
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

fn paint_text(text: &str, x: Pixels, y: Pixels, colour: u32, window: &mut Window, cx: &mut App) {
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
    /// drag. (Block editing comes in a later slice.)
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
        }
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
        if a._auto_scroll.is_none() && AutoScroll::speed(d.x, a.width()) != 0. {
            a._auto_scroll = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(AUTO_SCROLL_EVERY).await;
                    let going = this.update(cx, |this, cx| this.auto_scroll_step(cx));
                    if !matches!(going, Ok(true)) {
                        break;
                    }
                }
            }));
        }
        cx.notify();
    }

    /// One auto-scroll step; false once the drag has ended or the pointer
    /// left the edges.
    fn auto_scroll_step(&mut self, cx: &mut Context<Self>) -> bool {
        let a = &mut self.arrangement;
        let width = a.width();
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
