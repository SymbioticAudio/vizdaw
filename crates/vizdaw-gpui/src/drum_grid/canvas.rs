//! The Drum Grid's canvas (Swift `DrumGridView`, `E/DrumGridView.swift`):
//! a 22-point ruler over the bands of the shown drum tracks. A track's
//! header band is filled with the ruler's colour; a row band shows its
//! cells (the step matrix) or its hits (the timeline), its velocity lane
//! when shown, and the selected row's tint; the last band says where to
//! drop a row to give it a track of its own. In the tree it is the
//! `group` "Drum Grid" with a `button` per visible cell or hit (at most
//! [`MAX_TREE_ITEMS`]), labelled with [`cell_label`]; a click on one is a
//! real click at its centre, which toggles it (Plan 10d Ruling 8).
//!
//! A press on a row starts a paint or erase stroke: moves past the dead
//! zone extend it and the release ends it as one undo step; a release
//! before the dead zone is a click. Escape, the window losing the
//! keyboard, the grid leaving or hiding in its pane and the document
//! closing each cancel it (no undo step). A ruler click locates; a ruler
//! drag selects the range over the drum tracks. The wheel scrolls; with
//! the secondary modifier, or a pinch, it zooms about the pointer and
//! saves the zoom.
//!
//! Everything drawn comes from the frame's [`DrumGridState`]: the lanes
//! and layout the model read once per snapshot and settings. The strokes
//! are the model's (`drum_stroke_*`).

use std::sync::Arc;

use gpui::{
    AnyElement, App, BorderStyle, Bounds, ContentMask, Context, DispatchPhase, Hsla,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, PinchEvent, Pixels, Point, ScrollDelta, ScrollEvent, Styled,
    Window, canvas, div, fill, px, quad, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{DEAD_ZONE, PageFollow, RULER_HEIGHT, TimeAxis, WheelScroll};
use vizdaw_app::drum_grid::{DrumView, MAX_TREE_ITEMS, NEW_TRACK_BAND, cell_label};
use vizdaw_app::editor::Tool;
use vizdaw_ffi::drum::{
    DrumBandKindRow, DrumBandRow, DrumCellRow, DrumGridLayoutRow, DrumHitRow, DrumRowRow,
    DrumSpotRow, drum_grid_cells, drum_row_velocity,
};
use vizdaw_ffi::selection::TickRange;

use super::{DrumGridState, DrumGridView, GROUP};
use crate::arrangement::{TimeLayer, canvas_rect, colors, paint_text, with_alpha};
use crate::registry::{ChildRecorder, RegistryScope};
use crate::shell::DocumentWindow;
use crate::transport_bar::colors as bar_colors;

/// The selected row's tint (`E/DrumGridView.swift:162`).
const SELECTED_ROW_ALPHA: f32 = 0.08;
/// An empty cell: the separator at this alpha (`:186`).
const EMPTY_CELL_ALPHA: f32 = 0.15;
/// A hit cell's fill: `0.35 + 0.65 × velocity / 65 535` (`:189`).
const CELL_BASE_ALPHA: f32 = 0.35;
const CELL_VELOCITY_ALPHA: f32 = 0.65;
/// A timeline hit's fill, muted or not (`:203`).
const MUTED_HIT_ALPHA: f32 = 0.3;
const HIT_ALPHA: f32 = 0.85;
/// A timeline hit is at least this wide (`:132`).
const MIN_HIT_WIDTH: f32 = 4.;
/// A velocity lane's background: the label at this alpha (`:219`).
const VELOCITY_LANE_ALPHA: f32 = 0.04;
/// A velocity bar's width (`:226`).
const VELOCITY_BAR_WIDTH: f32 = 3.;
/// The new-track band's text inset (`:174`).
const NEW_TRACK_TEXT: (f32, f32) = (8., 7.);
/// A cell's inset (`:182`).
const CELL_INSET: (f32, f32) = (1., 2.);

/// A press on a row: its layout indexes and where it went down (content
/// points, below the ruler).
#[derive(Debug, Clone, Copy)]
pub(super) struct Press {
    lane: u32,
    row: u32,
    at: (f32, f32),
    moved: bool,
}

/// A press in the ruler: a click until it moves past the dead zone, then
/// a range drag.
#[derive(Debug, Clone, Copy)]
pub(super) struct RulerDrag {
    anchor: i64,
    start_x: f32,
    moved: bool,
    preview: Option<TickRange>,
}

impl DrumGridView {
    /// Window point `p` in content points (below the ruler).
    fn content_point(&self, p: Point<Pixels>) -> (f32, f32) {
        let o = self.layout.get().origin;
        (f32::from(p.x - o.x), f32::from(p.y - o.y) - RULER_HEIGHT)
    }

    fn width(&self) -> f32 {
        f32::from(self.layout.get().size.width)
    }

    /// The bands' area height (under the ruler) at the last draw.
    fn body_height(&self) -> f32 {
        (f32::from(self.layout.get().size.height) - RULER_HEIGHT).max(0.)
    }

    /// Keeps the vertical scroll inside the bands
    /// (`drum_layout().height`).
    pub(super) fn clamp_vertical(&mut self) {
        let max = (self.content_height - self.body_height()).max(0.);
        self.offset = if self.offset.is_finite() {
            self.offset.clamp(0., max)
        } else {
            0.
        };
    }

    /// A press (on a row or in the ruler) is down.
    pub fn pressing(&self) -> bool {
        self.press.is_some() || self.ruler.is_some()
    }

    /// Drops the press (the window cancels its stroke). Whether there was
    /// one.
    pub(super) fn drop_press(&mut self) -> bool {
        let had = self.pressing();
        self.press = None;
        self.ruler = None;
        had
    }

    /// Before drawing: while its sequence plays (and the layout follows),
    /// the view pages with the playhead (as the piano roll's does).
    pub(crate) fn follow(&mut self, state: &DrumGridState) {
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

    /// A hit is at least this wide on screen, in ticks
    /// (`E/DrumGridView.swift:122`).
    fn grab(&self) -> i64 {
        (4. * self.axis.ticks_per_point()).ceil() as i64
    }
}

/// The canvas: the ruler and bands drawn, the tree recorded, the pointer,
/// keys and wheel handled. `scope` is the grid's container.
pub(super) fn grid_canvas(
    view: &DrumGridView,
    state: &DrumGridState,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let weak = cx.entity().downgrade();
    let range = view.ruler.and_then(|r| r.preview).or(state.range);
    let scene = Scene {
        axis: view.axis,
        offset: view.offset,
        lanes: state.lanes.clone(),
        layout: state.layout.clone(),
        selected_row: state.selected_row,
        selected: state.selected.clone(),
        meter: state.meter.clone(),
        bars: state.bars.clone(),
        range,
        playhead: state.playhead,
        matrix: state.settings.view == DrumView::Matrix,
        step: state.settings.step,
    };
    let mut recorder = scope.children();
    let layout = view.layout.clone();
    let (relayout, events) = (weak.clone(), weak);
    let painter = canvas(
        move |bounds, _, cx| {
            // Another size shows other cells than were laid out: draw
            // again.
            if layout.get().size != bounds.size {
                let weak = relayout.clone();
                cx.defer(move |cx| {
                    weak.update(cx, |_, cx| cx.notify()).ok();
                });
            }
            layout.set(bounds);
            scene.prepare(bounds, &mut recorder)
        },
        move |bounds, prepared, window, cx| {
            prepared.paint(bounds, window, cx);
            // A press follows the pointer anywhere in the window.
            let moved = events.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    let held = e.pressed_button == Some(MouseButton::Left);
                    moved
                        .update(cx, |this, cx| this.drum_drag_to(e.position, held, cx))
                        .ok();
                }
            });
            let released = events.clone();
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                    released
                        .update(cx, |this, cx| this.drum_release(e.position, cx))
                        .ok();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    div()
        .id("drum-grid")
        .track_focus(&view.focus)
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_hidden()
        .bg(rgb(colors::canvas()))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, e: &MouseDownEvent, window, cx| this.drum_mouse_down(e, window, cx)),
        )
        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
            if e.keystroke.key == "escape" && this.cancel_drum(cx) {
                cx.stop_propagation();
            }
        }))
        .on_scroll(cx.listener(|this, e, _, cx| this.drum_wheel(e, cx)))
        .on_pinch(cx.listener(|this, e, _, cx| this.drum_pinch(e, cx)))
        .child(painter)
        .child(scope.this("group", GROUP, true))
        .child(scope.clip())
        .into_any_element()
}

/// The view's part of a frame.
struct Scene {
    axis: TimeAxis,
    offset: f32,
    lanes: Arc<[vizdaw_ffi::drum::DrumLaneRow]>,
    layout: Arc<DrumGridLayoutRow>,
    selected_row: Option<vizdaw_app::drum_grid::DrumRowKey>,
    selected: Arc<std::collections::HashSet<u64>>,
    meter: Vec<vizdaw_ffi::events::MeterEntryRow>,
    bars: vizdaw_app::arrangement::MeterBars,
    range: Option<TickRange>,
    playhead: Option<f64>,
    matrix: bool,
    step: i64,
}

/// A rectangle in content points (y = 0 is the ruler's bottom).
#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn inset(self, dx: f32, dy: f32) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            w: self.w - 2. * dx,
            h: self.h - 2. * dy,
        }
    }

    fn empty(&self) -> bool {
        self.w <= 0. || self.h <= 0.
    }

    /// Its part inside `0..w` × `0..h`.
    fn clipped(self, w: f32, h: f32) -> Rect {
        let (x0, y0) = (self.x.max(0.), self.y.max(0.));
        let (x1, y1) = ((self.x + self.w).min(w), (self.y + self.h).min(h));
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }
}

/// A row's velocity lane: its top, its height, and its bars (x, y,
/// height).
type VelocityLane = (f32, f32, Vec<(f32, f32, f32)>);

/// A filled, outlined shape: a cell or a hit.
struct Shape {
    rect: Rect,
    fill: Hsla,
    /// Outlined: selected (2 wide, the selected colour) or not.
    outline: Option<bool>,
}

/// One band laid out.
enum BandPaint {
    Header {
        top: f32,
        height: f32,
    },
    Row {
        top: f32,
        height: f32,
        tinted: bool,
        shapes: Vec<Shape>,
        velocity: Option<VelocityLane>,
    },
    NewTrack {
        top: f32,
        height: f32,
    },
}

/// One frame laid out.
struct Prepared {
    time: TimeLayer,
    width: f32,
    bands: Vec<BandPaint>,
}

impl Scene {
    fn top(&self, b: &DrumBandRow) -> f32 {
        b.top as f32 - self.offset
    }

    /// Matrix cell `tick`'s rect in band `b` (`E/DrumGridView.swift:
    /// 125-130`).
    fn cell_rect(&self, tick: i64, b: &DrumBandRow) -> Rect {
        let x0 = self.axis.x(tick);
        let x1 = self.axis.x(tick.saturating_add(self.step.max(1)));
        Rect {
            x: x0,
            y: self.top(b),
            w: (x1 - x0).max(1.),
            h: b.cells as f32,
        }
    }

    /// A timeline hit's rect in band `b` (`:132-136`).
    fn hit_rect(&self, h: &DrumHitRow, b: &DrumBandRow) -> Rect {
        Rect {
            x: self.axis.x(h.tick),
            y: self.top(b),
            w: self.axis.width_of_ticks(h.duration).max(MIN_HIT_WIDTH),
            h: b.cells as f32,
        }
    }

    /// Row `r`'s cells over the view's width (`:146-150`).
    fn cells(&self, r: &DrumRowRow, w: f32) -> Vec<DrumCellRow> {
        drum_grid_cells(
            r.hits.clone(),
            self.axis.tick_at(0.),
            self.axis.tick_at(w).saturating_add(1),
            self.step,
        )
        .unwrap_or_default()
    }

    fn row(&self, lane: u32, row: u32) -> Option<(&vizdaw_ffi::drum::DrumLaneRow, &DrumRowRow)> {
        let l = self.lanes.get(lane as usize)?;
        Some((l, l.rows.get(row as usize)?))
    }

    /// Lays out the bands in view and records the cells (matrix) or hits
    /// (timeline) in view, at most [`MAX_TREE_ITEMS`]
    /// (`E/DrumGridView.swift:152-231,307-337`).
    fn prepare(self, bounds: Bounds<Pixels>, recorder: &mut ChildRecorder) -> Prepared {
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let body_h = (h - RULER_HEIGHT).max(0.);
        let time = TimeLayer::new(&self.axis, &self.bars, w, h, self.range, self.playhead);
        let o = bounds.origin;
        let mut listed = 0;
        let mut bands = vec![];
        for b in &self.layout.bands {
            let (top, height) = (self.top(b), b.height as f32);
            if body_h <= 0. || top + height < 0. || top > body_h {
                continue;
            }
            match b.kind {
                DrumBandKindRow::Header { .. } => bands.push(BandPaint::Header { top, height }),
                DrumBandKindRow::NewLane => bands.push(BandPaint::NewTrack { top, height }),
                DrumBandKindRow::Row { lane, row } => {
                    let Some((l, r)) = self.row(lane, row) else {
                        continue;
                    };
                    // The tree: the cells or hits in view.
                    let items: Vec<(i64, bool, u16, Rect)> = if self.matrix {
                        self.cells(r, w)
                            .into_iter()
                            .map(|c| {
                                (
                                    c.tick,
                                    !c.hits.is_empty(),
                                    c.velocity,
                                    self.cell_rect(c.tick, b),
                                )
                            })
                            .collect()
                    } else {
                        r.hits
                            .iter()
                            .map(|x| (x.tick, true, x.velocity, self.hit_rect(x, b)))
                            .collect()
                    };
                    for (tick, hit, velocity, rect) in &items {
                        if listed == MAX_TREE_ITEMS {
                            break;
                        }
                        let f = rect.clipped(w, body_h);
                        if f.empty() {
                            continue;
                        }
                        listed += 1;
                        recorder.record(
                            "button",
                            &cell_label(&r.name, *tick, *hit, *velocity, &self.meter),
                            canvas_rect(o, f.x, RULER_HEIGHT + f.y, f.w, f.h),
                            true,
                            false,
                        );
                    }
                    let shapes = if self.matrix {
                        self.cell_shapes(r, b, w)
                    } else {
                        self.hit_shapes(r, b, w)
                    };
                    let velocity = (b.height > b.cells).then(|| self.velocities(r, b, w));
                    bands.push(BandPaint::Row {
                        top,
                        height,
                        tinted: self.selected_row
                            == Some(vizdaw_app::drum_grid::DrumRowKey {
                                lane: l.lane,
                                row: r.id,
                            }),
                        shapes,
                        velocity,
                    });
                }
            }
        }
        Prepared {
            time,
            width: w,
            bands,
        }
    }

    /// The matrix's cells (`E/DrumGridView.swift:180-195`): an empty one
    /// is the separator at 0.15; a hit one is the region fill by its
    /// velocity, outlined (selected when one of its hits is).
    fn cell_shapes(&self, r: &DrumRowRow, b: &DrumBandRow, w: f32) -> Vec<Shape> {
        let mut out = vec![];
        for c in self.cells(r, w) {
            let rect = self.cell_rect(c.tick, b).inset(CELL_INSET.0, CELL_INSET.1);
            // Narrower than its inset: nothing shows (Swift's inset rect
            // is null).
            if rect.empty() {
                continue;
            }
            if c.hits.is_empty() {
                out.push(Shape {
                    rect,
                    fill: with_alpha(colors::separator(), EMPTY_CELL_ALPHA),
                    outline: None,
                });
            } else {
                let strength =
                    CELL_BASE_ALPHA + CELL_VELOCITY_ALPHA * f32::from(c.velocity) / 65_535.;
                out.push(Shape {
                    rect,
                    fill: region_fill_at(strength),
                    outline: Some(c.hits.iter().any(|id| self.selected.contains(id))),
                });
            }
        }
        out
    }

    /// The timeline's hits (`:198-206`): muted at 0.3, others at 0.85.
    fn hit_shapes(&self, r: &DrumRowRow, b: &DrumBandRow, w: f32) -> Vec<Shape> {
        r.hits
            .iter()
            .filter_map(|x| {
                let rect = self.hit_rect(x, b).inset(0., CELL_INSET.1);
                (rect.x + rect.w >= 0. && rect.x <= w && !rect.empty()).then(|| Shape {
                    rect,
                    fill: region_fill_at(if x.mute { MUTED_HIT_ALPHA } else { HIT_ALPHA }),
                    outline: Some(self.selected.contains(&x.id)),
                })
            })
            .collect()
    }

    /// A row's velocity lane (`:216-229`): its top and height, and a
    /// 3-wide bar per hit at `velocity / 65 535` of the lane.
    fn velocities(&self, r: &DrumRowRow, b: &DrumBandRow, w: f32) -> VelocityLane {
        let lane_top = self.top(b) + b.cells as f32;
        let h = (b.height - b.cells) as f32 - 4.;
        let bars = r
            .hits
            .iter()
            .filter_map(|x| {
                let at = self.axis.x(x.tick);
                (at >= -4. && at <= w).then(|| {
                    let bar = (h * f32::from(x.velocity) / 65_535.).max(1.);
                    (at, lane_top + 2. + h - bar, bar)
                })
            })
            .collect();
        (lane_top, h + 4., bars)
    }
}

impl Prepared {
    fn paint(self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let o = bounds.origin;
        let w = self.width;
        let r = RULER_HEIGHT;
        let rect = |x: f32, y: f32, w: f32, h: f32| canvas_rect(o, x, r + y, w, h);
        let body = self.time.body(o);
        window.with_content_mask(Some(ContentMask { bounds: body }), |window| {
            self.time.paint_grid(o, window);
            for band in &self.bands {
                let (top, height) = match band {
                    BandPaint::Header { top, height } => {
                        window.paint_quad(fill(rect(0., *top, w, *height), rgb(colors::ruler())));
                        (*top, *height)
                    }
                    BandPaint::NewTrack { top, height } => {
                        paint_text(
                            NEW_TRACK_BAND,
                            o.x + px(NEW_TRACK_TEXT.0),
                            o.y + px(r + top + NEW_TRACK_TEXT.1),
                            bar_colors::disabled(),
                            window,
                            cx,
                        );
                        (*top, *height)
                    }
                    BandPaint::Row {
                        top,
                        height,
                        tinted,
                        shapes,
                        velocity,
                    } => {
                        if *tinted {
                            window.paint_quad(fill(
                                rect(0., *top, w, *height),
                                with_alpha(colors::selection(), SELECTED_ROW_ALPHA),
                            ));
                        }
                        for s in shapes {
                            let (line, width) = match s.outline {
                                Some(true) => (rgb(colors::region_selected()).into(), 2.),
                                Some(false) => (rgb(colors::region_line()).into(), 1.),
                                None => (Hsla::transparent_black(), 0.),
                            };
                            let q = s.rect;
                            window.paint_quad(quad(
                                rect(q.x, q.y, q.w, q.h),
                                px(crate::appearance::corner(colors::RADIUS.min(q.h / 2.))),
                                s.fill,
                                px(width),
                                line,
                                BorderStyle::Solid,
                            ));
                        }
                        if let Some((lane_top, lane_h, bars)) = velocity {
                            window.paint_quad(fill(
                                rect(0., *lane_top, w, *lane_h),
                                with_alpha(colors::label(), VELOCITY_LANE_ALPHA),
                            ));
                            for &(x, y, bh) in bars {
                                window.paint_quad(fill(
                                    rect(x, y, VELOCITY_BAR_WIDTH, bh),
                                    rgb(colors::region_line()),
                                ));
                            }
                        }
                        (*top, *height)
                    }
                };
                window.paint_quad(fill(
                    rect(0., top + height - 0.5, w, 1.),
                    rgb(colors::separator()),
                ));
            }
            self.time.paint_overlay(o, window);
        });
        self.time.paint_ruler(o, window, cx);
    }
}

impl DocumentWindow {
    /// The window lost the keyboard: the grid's press is dropped and its
    /// stroke cancelled (its release may go elsewhere).
    pub(crate) fn drum_deactivated(&mut self, cx: &mut Context<Self>) {
        self.end_drum_stroke(cx);
    }

    /// The document window closes: the grid's stroke goes first.
    pub(crate) fn drum_closing(grid: &mut DrumGridView, d: &mut DocumentModel) {
        grid.drop_press();
        d.drum_stroke_cancelled();
    }

    /// Drops the grid's press and cancels its stroke (no undo step).
    pub(super) fn end_drum_stroke(&mut self, cx: &mut Context<Self>) {
        let pressed = self.drum_grid.drop_press();
        let stroke = self
            .read_model(cx, |d| d.drum_stroke().is_some())
            .unwrap_or(false);
        if stroke {
            self.with_model(cx, |d| d.drum_stroke_cancelled());
        }
        if pressed {
            cx.notify();
        }
    }

    /// Escape on the canvas: a press is dropped, a stroke cancelled
    /// without an undo step. Whether there was anything to cancel.
    fn cancel_drum(&mut self, cx: &mut Context<Self>) -> bool {
        let stroke = self
            .read_model(cx, |d| d.drum_stroke().is_some())
            .unwrap_or(false);
        if !self.drum_grid.pressing() && !stroke {
            return false;
        }
        self.end_drum_stroke(cx);
        cx.notify();
        true
    }

    /// The tick under content x `x`, and on the shared edit grid.
    fn drum_ticks(&self, x: f32, cx: &App) -> (i64, i64) {
        let t = self.drum_grid.axis.tick_at(x);
        let snapped = self
            .read_model(cx, |d| d.layout().snap.floor(t, &drum_meter(d)))
            .unwrap_or(t);
        (t, snapped)
    }

    /// The velocity row `(lane, row)`'s lane means at content y `y`.
    fn drum_velocity(&self, lane: u32, row: u32, y: f32, cx: &App) -> Option<u8> {
        let offset = self.drum_grid.offset;
        self.read_model(cx, |d| {
            drum_row_velocity((*d.drum_layout()).clone(), lane, row, f64::from(y + offset))
                .ok()
                .flatten()
        })
        .flatten()
    }

    /// A press in the canvas: it takes the keyboard; in the ruler it
    /// starts a click or a range drag; on a row it starts a stroke
    /// (`E/DrumGridView.swift:240-248,283-287`).
    fn drum_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // A stroke still held lost its release: it goes first, wherever
        // this press lands (Swift's `ToolMachine.down`).
        self.end_drum_stroke(cx);
        let g = &mut self.drum_grid;
        window.focus(&g.focus, cx);
        let at = g.content_point(e.position);
        if at.1 < 0. {
            g.press = None;
            g.ruler = Some(RulerDrag {
                anchor: g.axis.tick_at(at.0),
                start_x: at.0,
                moved: false,
                preview: None,
            });
            cx.notify();
            return;
        }
        g.ruler = None;
        let y = f64::from(at.1 + g.offset);
        let Some(DrumSpotRow::Row { lane, row, .. }) = self.read_model(cx, |d| d.drum_spot(y))
        else {
            return;
        };
        self.drum_grid.press = Some(Press {
            lane,
            row,
            at,
            moved: false,
        });
        let (t, snapped) = self.drum_ticks(at.0, cx);
        let grab = self.drum_grid.grab();
        let velocity = self.drum_velocity(lane, row, at.1, cx);
        self.with_model(cx, |d| {
            let erase = d.tool() == Tool::Eraser;
            d.drum_stroke_began(
                lane as usize,
                row as usize,
                t,
                snapped,
                grab,
                velocity,
                erase,
            );
        });
        cx.notify();
    }

    /// The snapped range between a ruler press at `anchor` and content x
    /// `x`; `None` when empty.
    fn drum_ruler_range(&self, anchor: i64, x: f32, cx: &App) -> Option<TickRange> {
        let other = self.drum_grid.axis.tick_at(x);
        self.read_model(cx, |d| {
            let (snap, meter) = (d.layout().snap, drum_meter(d));
            let a = snap.apply(anchor.min(other), &meter);
            let b = snap.apply(anchor.max(other), &meter);
            (b > a).then_some(TickRange { start: a, end: b })
        })
        .flatten()
    }

    /// The pointer moved to window point `at` (`held`: the left button is
    /// down): a ruler drag past the dead zone previews its range; a stroke
    /// past it extends.
    fn drum_drag_to(&mut self, at: Point<Pixels>, held: bool, cx: &mut Context<Self>) {
        if !held {
            return;
        }
        let g = &mut self.drum_grid;
        let p = g.content_point(at);
        if let Some(mut r) = g.ruler {
            if !r.moved && (p.0 - r.start_x).abs() <= DEAD_ZONE {
                return;
            }
            r.moved = true;
            r.preview = self.drum_ruler_range(r.anchor, p.0, cx);
            self.drum_grid.ruler = Some(r);
            cx.notify();
            return;
        }
        let Some(mut press) = g.press else {
            return;
        };
        if !press.moved {
            let (dx, dy) = (p.0 - press.at.0, p.1 - press.at.1);
            if dx.hypot(dy) <= DEAD_ZONE {
                return;
            }
            press.moved = true;
            g.press = Some(press);
        }
        let (t, _) = self.drum_ticks(p.0, cx);
        let grab = self.drum_grid.grab();
        let velocity = self.drum_velocity(press.lane, press.row, p.1, cx);
        self.with_model(cx, |d| d.drum_stroke_moved(t, grab, velocity));
        cx.notify();
    }

    /// The button is up at window point `at`: a ruler click locates
    /// (snapped) and a ruler drag selects its range over the drum tracks;
    /// a stroke ends as one undo step, and a release before the dead zone
    /// is a click at the press.
    fn drum_release(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let g = &mut self.drum_grid;
        if let Some(r) = g.ruler.take() {
            if r.moved {
                if let Some(range) = r.preview {
                    self.with_model(cx, |d| d.drum_select_range(range));
                }
            } else {
                self.with_model(cx, |d| {
                    let (snap, meter) = (d.layout().snap, drum_meter(d));
                    d.locate(snap.apply(r.anchor, &meter));
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
        let Some(press) = g.press.take() else {
            return;
        };
        let p = if press.moved {
            g.content_point(at)
        } else {
            press.at
        };
        let (t, _) = self.drum_ticks(p.0, cx);
        let grab = self.drum_grid.grab();
        let velocity = self.drum_velocity(press.lane, press.row, p.1, cx);
        self.with_model(cx, |d| d.drum_stroke_ended(t, grab, velocity));
        cx.notify();
    }

    /// The live zoom is saved when it changed.
    fn save_drum_zoom(&mut self, cx: &mut Context<Self>) {
        let tpp = self.drum_grid.axis.ticks_per_point();
        let changed = self
            .read_model(cx, |d| d.drum_settings().ticks_per_point != tpp)
            .unwrap_or(false);
        if changed {
            self.with_model(cx, |d| d.update_drum_settings(|s| s.ticks_per_point = tpp));
        }
    }

    /// The wheel: scrolls the time and the bands (inside them); with the
    /// secondary modifier it zooms about the pointer.
    fn drum_wheel(&mut self, e: &ScrollEvent, cx: &mut Context<Self>) {
        let (precise, dx, dy) = match e.delta {
            ScrollDelta::Pixels(p) => (true, f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(p) => (false, p.x, p.y),
        };
        let secondary = e.modifiers.secondary(cx);
        let g = &mut self.drum_grid;
        if secondary {
            let x = f32::from(e.position.x - g.layout.get().origin.x);
            g.axis = g.axis.zoom_about(WheelScroll::zoom_factor(dy, precise), x);
            self.save_drum_zoom(cx);
        } else {
            let (dx, dy) = WheelScroll::points(dx, dy, precise);
            g.axis = g.axis.scrolled_by(-dx);
            g.offset -= dy;
            g.clamp_vertical();
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// A pinch zooms about its centre.
    fn drum_pinch(&mut self, e: &PinchEvent, cx: &mut Context<Self>) {
        let g = &mut self.drum_grid;
        let x = f32::from(e.position.x - g.layout.get().origin.x);
        g.axis = g.axis.zoom_about(1. + f64::from(e.delta), x);
        self.save_drum_zoom(cx);
        cx.stop_propagation();
        cx.notify();
    }
}

/// The region fill at alpha `a` (Swift's `withAlphaComponent`, which
/// replaces the theme's alpha).
fn region_fill_at(a: f32) -> Hsla {
    let mut c = colors::region_fill();
    c.a = a;
    c
}

/// The grid's sequence's meter.
pub(super) fn drum_meter(d: &DocumentModel) -> Vec<vizdaw_ffi::events::MeterEntryRow> {
    d.drum_seq()
        .and_then(|seq| d.sequence_info(seq))
        .map(|i| i.meter.clone())
        .unwrap_or_default()
}
