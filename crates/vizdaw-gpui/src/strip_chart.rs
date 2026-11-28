//! The Strip Chart (Swift `StripChartEditor`, `StripChartBar` and
//! `StripChartView`, `S/StripChartPane.swift`): the bottom pane's, and the
//! right pane's Controllers view (the focused track's,
//! `S/PaneViews.swift:223-235`). A bar (Mode, Operation, Shape and its
//! parameters, Step, the Track menu of a sequence target, Rows), then the
//! rows' canvas (`E/StripChartView.swift`): a 22-point ruler over the
//! stacked rows, each with its separator, centre line, values (bars, or a
//! held or ramped line with dots), the write in progress and its title
//! "‹title› ▾"; over them the stroke's preview. In the tree it is a
//! `group` "Strip Chart" with a `menu` "Row ‹n›" per row title and a
//! `button` per value in view (at most 200 a row).
//!
//! A press on a row's title opens the row menu (what the row shows, MIDI
//! Learn for an automation row, Add Row and Remove Row); elsewhere it
//! starts a stroke, which moves past the dead zone and ends on the
//! release. Escape, the window losing the keyboard, the chart leaving or
//! hiding in its pane and its document closing each cancel the stroke
//! (no undo step). A ruler click locates, a ruler drag selects the range
//! over the shown track. The wheel scrolls; with the secondary modifier,
//! or a pinch, it zooms about the pointer and saves the zoom.
//!
//! The view holds layout only. What each chart shows and every change
//! is the model's (`vizdaw_app::strip_chart`): the bar's settings go
//! through `update_strip_settings`, the track through
//! `set_strip_chosen_lane`, the rows through `add_strip_row` and
//! `remove_strip_row`.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    AnyElement, App, BorderStyle, Bounds, ContentMask, Context, DispatchPhase, FocusHandle,
    Focusable, Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, PathBuilder, PinchEvent, Pixels, Point,
    ScrollDelta, ScrollEvent, StatefulInteractiveElement, Styled, WeakEntity, Window, canvas, div,
    fill, point, px, quad, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{
    DEAD_ZONE, MeterBars, PageFollow, RULER_HEIGHT, TimeAxis, WheelScroll,
};
use vizdaw_app::editor::{DetailEditorKind, DetailState, EditorTarget, PaneView};
use vizdaw_app::list::PitchName;
use vizdaw_app::position::Bbu;
use vizdaw_app::select_modify::WaveKind;
use vizdaw_app::strip_chart::{
    AutomationTargets, ChartSlot, LineCurve, MAX_ROWS, PeriodKind, ShapeKind, StripBarChoices,
    StripChartSettings, StripFrame, StripLane, StripMode, StripOp, StripPoint, StripStyle,
    StripValue,
};
use vizdaw_app::transport::TransportModel;
use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::events::{AutomationTargetRow, MeterEntryRow};
use vizdaw_ffi::selection::{SelectionRow, TickRange};

use crate::appearance::ThemedCorners as _;
use crate::arrangement::{TimeLayer, bar_button, canvas_rect, colors, paint_text, with_alpha};
use crate::cascade_menu::{CascadeEntry, cascade};
use crate::detail_pane;
use crate::list_editor::{Entry, bar_menu};
use crate::registry::{ChildRecorder, RegistryScope};
use crate::shell::DocumentWindow;
use crate::transport_bar::{self, colors as bar_colors};

/// The rows' canvas in the tree (Swift `StripChartView.accessibilityName`).
pub const GROUP: &str = "Strip Chart";
/// The bar's pickers and buttons (`S/StripChartPane.swift:79-176`).
pub const MODE: &str = "Mode";
pub const OPERATION: &str = "Operation";
pub const SHAPE: &str = "Shape";
pub const CURVE: &str = "Curve";
pub const WAVE: &str = "Wave";
pub const PERIOD_KIND: &str = "Period Kind";
pub const PERIOD: &str = "Period";
pub const PHASE: &str = "Phase";
pub const STEP: &str = "Step";
/// `S/StripChartPane.swift:132`.
pub const STEP_HELP: &str = "Distance between drawn points; the spacing of Thin and Thicken";
pub const TRACK: &str = "Track";
pub const DECREASE_ROWS: &str = "Decrease Rows";
pub const INCREASE_ROWS: &str = "Increase Rows";
/// The Mode picker's width (`S/StripChartPane.swift:84`).
const MODE_WIDTH: f32 = 190.;

/// The row menu (`E/StripChartView.swift:313-381`).
pub const CONTROLLER: &str = "Controller";
pub const POLY_PRESSURE: &str = "Poly Pressure";
pub const AUTOMATION: &str = "Automation";
/// `E/StripChartView.swift:357`.
pub const AUTOMATION_HELP: &str =
    "The mixer's targets could not be read just now (busy); open the menu again.";
/// `E/StripChartView.swift:363`.
pub const MIDI_LEARN: &str = "MIDI Learn";
/// `E/StripChartView.swift:370,373`.
pub const ADD_ROW: &str = "Add Row";
pub const REMOVE_ROW: &str = "Remove Row";
/// A row title's tree label ("Row ‹n›", Ruling 6).
pub fn row_label(i: usize) -> String {
    format!("Row {}", i + 1)
}
/// The most values a row lists in the tree (`E/StripChartView.swift:404`).
const TREE_LIMIT: usize = 200;
/// A value bar's width and alpha, a line's alpha and width, a dot's side
/// and the centre line's alpha (`E/StripChartView.swift:200-310`).
const BAR_WIDTH: f32 = 5.;
const BAR_ALPHA: f32 = 0.45;
const LINE_ALPHA: f32 = 0.8;
const LINE_WIDTH: f32 = 1.5;
const DOT: f32 = 4.;
const CENTRE_ALPHA: f32 = 0.6;
/// The Compress rectangle's fill alpha (`E/StripChartView.swift:288`).
const OVERLAY_FILL: f32 = 0.12;
/// Values just outside the view still draw (their segments cross its
/// edges) and are listed (`E/StripChartView.swift:244,399`).
const CULL: f32 = 3.;

/// The canvas's theme tokens (Swift `EditorTheme`): `valueBar`
/// (`graph.line`), `armColour` (`state.record`), `secondaryLabel`
/// (`text.secondary`) and `toolOverlay` (the accent, else the selected
/// handle or the text, whichever first reads 3:1 on the canvas).
mod tokens {
    use crate::appearance::{contrast, current};

    pub fn value_bar() -> u32 {
        current().graph.line.rgb
    }
    pub fn arm() -> u32 {
        current().state.record.rgb
    }
    pub fn title() -> u32 {
        current().text.secondary.rgb
    }
    pub fn tool_overlay() -> u32 {
        let t = current();
        let canvas = t.surface.inset.rgb;
        [t.accent.accent.rgb, t.graph.handle_selected.rgb]
            .into_iter()
            .find(|&c| contrast(c, canvas) >= 3.)
            .unwrap_or(t.text.primary.rgb)
    }
}

/// Which bar menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChartMenu {
    Operation,
    Shape,
    Curve,
    Wave,
    PeriodKind,
    Period,
    Phase,
    Step,
    Track,
    /// Row `i`'s menu, at the pointer.
    Row(usize),
}

impl ChartMenu {
    /// The bar pickers' element ids.
    fn index(self) -> usize {
        match self {
            ChartMenu::Operation => 0,
            ChartMenu::Shape => 1,
            ChartMenu::Curve => 2,
            ChartMenu::Wave => 3,
            ChartMenu::PeriodKind => 4,
            ChartMenu::Period => 5,
            ChartMenu::Phase => 6,
            ChartMenu::Step => 7,
            ChartMenu::Track => 8,
            ChartMenu::Row(i) => 9 + i,
        }
    }
}

/// A press below the ruler: its row and where it went down (content
/// points, below the ruler).
#[derive(Debug, Clone, Copy)]
struct Press {
    row: usize,
    at: (f32, f32),
}

/// A press in the ruler: a click until it moves past the dead zone, then
/// a range drag.
#[derive(Debug, Clone, Copy)]
struct RulerDrag {
    anchor: i64,
    start_x: f32,
    x: f32,
    moved: bool,
    preview: Option<TickRange>,
}

/// One chart's view state, owned by the document window. None of it is
/// saved; the zoom it shows is the layout's.
pub struct StripChart {
    slot: ChartSlot,
    focus: FocusHandle,
    axis: TimeAxis,
    /// The rows' vertical scroll.
    offset: f32,
    /// The canvas's bounds at the last draw (zero before the first).
    layout: Rc<Cell<Bounds<Pixels>>>,
    /// The chart showed at the last draw.
    mounted: bool,
    /// The target the view last scrolled to.
    shown_target: Option<EditorTarget>,
    pub(crate) menu: Option<ChartMenu>,
    /// The row menu's open submenus.
    pub(crate) menu_path: Vec<usize>,
    /// Where the row menu opened (window points).
    menu_at: Point<Pixels>,
    /// The chart left its pane since the last draw: the window drops its
    /// stroke before the view starts afresh.
    detach_pending: bool,
    /// The press below the ruler, and whether it moved past the dead zone.
    press: Option<Press>,
    moved: bool,
    /// The stroke's pointer path in track space (tick, and y in the rows),
    /// drawn as its preview: it stays on its values while the view
    /// scrolls.
    path: Vec<(i64, f32)>,
    ruler: Option<RulerDrag>,
    /// The rows' height and count at the last prepare (the wheel's
    /// vertical clamp).
    row_height: f32,
    rows: usize,
}

impl StripChart {
    pub fn new(slot: ChartSlot, cx: &mut Context<DocumentWindow>) -> Self {
        StripChart {
            slot,
            focus: cx.focus_handle(),
            axis: Self::fresh_axis(),
            offset: 0.,
            layout: Rc::default(),
            mounted: false,
            shown_target: None,
            menu: None,
            menu_path: Vec::new(),
            menu_at: Point::default(),
            detach_pending: false,
            press: None,
            moved: false,
            path: Vec::new(),
            ruler: None,
            row_height: StripChartSettings::default().row_height as f32,
            rows: 1,
        }
    }

    fn fresh_axis() -> TimeAxis {
        TimeAxis::new(StripChartSettings::default().ticks_per_point, 0)
    }

    pub fn slot(&self) -> ChartSlot {
        self.slot
    }

    /// The chart's time axis.
    pub fn axis(&self) -> &TimeAxis {
        &self.axis
    }

    /// The rows' vertical scroll.
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// The row menu's open submenus.
    pub fn menu_path(&self) -> &[usize] {
        &self.menu_path
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The stroke preview's points where the view shows them now
    /// (content points, below the ruler).
    pub fn preview_points(&self) -> Vec<(f32, f32)> {
        let f = self.frame();
        self.path.iter().map(|&p| f.content_point(p)).collect()
    }

    /// Its row menu is open.
    pub(crate) fn row_menu_open(&self) -> bool {
        matches!(self.menu, Some(ChartMenu::Row(_)))
    }

    /// A press (below or in the ruler) is down.
    pub fn pressing(&self) -> bool {
        self.press.is_some() || self.ruler.is_some()
    }

    fn frame(&self) -> StripFrame {
        StripFrame {
            axis: self.axis,
            row_height: self.row_height,
            offset: self.offset,
        }
    }

    /// Window point `p` in content points (below the ruler).
    fn content_point(&self, p: Point<Pixels>) -> (f32, f32) {
        let o = self.layout.get().origin;
        (f32::from(p.x - o.x), f32::from(p.y - o.y) - RULER_HEIGHT)
    }

    fn width(&self) -> f32 {
        f32::from(self.layout.get().size.width)
    }

    /// The rows' area height (under the ruler) at the last draw.
    fn body_height(&self) -> f32 {
        (f32::from(self.layout.get().size.height) - RULER_HEIGHT).max(0.)
    }

    /// Keeps the vertical scroll inside the rows.
    fn clamp_vertical(&mut self) {
        let h = self.body_height();
        let max = (self.frame().content_height(self.rows) - h).max(0.);
        self.offset = if self.offset.is_finite() {
            self.offset.clamp(0., max)
        } else {
            0.
        };
    }

    /// Drops the press and its preview (the window cancels its stroke).
    fn drop_press(&mut self) -> bool {
        let had = self.pressing();
        self.press = None;
        self.moved = false;
        self.path.clear();
        self.ruler = None;
        had
    }

    /// Before drawing: while its sequence plays (and the layout follows),
    /// the view pages with the playhead.
    pub(crate) fn follow(&mut self, state: &StripChartState) {
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

    /// Closes the bar's menu; false when none was open.
    pub(crate) fn close_menu(&mut self) -> bool {
        self.menu_path.clear();
        self.menu.take().is_some()
    }

    /// What the chart shows when it shows: the pane's detail while it is a
    /// Strip Chart; the Controllers chart while the right pane shows the
    /// Controllers view and a track is focused.
    fn showing(&self, d: &DocumentModel) -> Option<DetailState> {
        match self.slot {
            ChartSlot::Pane => d
                .layout()
                .detail
                .clone()
                .filter(|x| x.kind == DetailEditorKind::StripChart),
            ChartSlot::Controllers => {
                let right = d.layout().views.current().right;
                (right.shown && right.view == PaneView::Controllers)
                    .then(|| d.strip_state(ChartSlot::Controllers))
                    .flatten()
            }
        }
    }

    /// Before drawing: whether the chart shows. It takes the saved zoom
    /// (keeping the origin) and, the first time it shows a block target,
    /// scrolls to the block's start (`E/StripChartView.swift:111-119`).
    /// Leaving the pane sets a detach the window acts on.
    pub(crate) fn prepare(&mut self, d: &DocumentModel) -> bool {
        let Some(state) = self.showing(d) else {
            if self.mounted {
                self.mounted = false;
                self.detach_pending = true;
            }
            return false;
        };
        self.mounted = true;
        self.row_height = d.strip_settings().row_height as f32;
        self.rows = d.strip_rows(self.slot).len();
        self.clamp_vertical();
        let tpp = TimeAxis::new(d.strip_settings().ticks_per_point, 0).ticks_per_point();
        if tpp != self.axis.ticks_per_point() {
            self.axis = self.axis.with_zoom(tpp);
        }
        let target = state.target;
        if self.shown_target != Some(target) {
            self.shown_target = Some(target);
            if let EditorTarget::Block { start, .. } = target {
                self.axis = self.axis.scrolled_to(start as f64);
            }
        }
        true
    }

    /// The chart left its pane: shown again, it starts afresh (Swift makes
    /// a new view), at the saved zoom.
    fn unmount(&mut self) {
        self.axis = Self::fresh_axis();
        self.offset = 0.;
        self.layout.set(Bounds::default());
        self.shown_target = None;
        self.menu = None;
        self.menu_path.clear();
        self.mounted = false;
        self.drop_press();
    }
}

/// One row as the chart draws it.
struct RowDraw {
    lane: StripLane,
    title: String,
    values: Vec<StripValue>,
    /// The write in progress on it (Ruling 10).
    preview: Vec<StripValue>,
}

/// What a chart draws of the document this frame.
pub struct StripChartState {
    slot: ChartSlot,
    settings: StripChartSettings,
    rows: usize,
    drawn: Vec<RowDraw>,
    /// The automation targets: read only while a row automates one or the
    /// row menu is open (once per snapshot and slots version).
    targets: Arc<AutomationTargets>,
    selected: HashSet<u64>,
    meter: Vec<MeterEntryRow>,
    bars: MeterBars,
    range: Option<TickRange>,
    follow: bool,
    /// The stroke's kind: a Compress rectangle, a free line.
    stroke: Option<(bool, bool)>,
    /// For a sequence target: the track shown's name ("—" with none) and
    /// the sequence's tracks.
    tracks: Option<(String, Vec<(u64, String)>)>,
    lane: Option<(u64, u64)>,
    /// The playhead's tick, when the transport is on the chart's sequence.
    playhead: Option<f64>,
    playing: bool,
}

impl StripChartState {
    /// `slot`'s chart on `doc`, with `transport`'s playhead at `now`;
    /// `None` when it shows nothing (the pane's detail is not a Strip
    /// Chart; no track is focused for the Controllers). `row_menu`: its
    /// row menu is open (it lists the automation targets).
    pub fn of(
        doc: &DocumentModel,
        slot: ChartSlot,
        transport: Option<&TransportModel>,
        now: Instant,
        row_menu: bool,
    ) -> Option<Self> {
        let state = match slot {
            ChartSlot::Pane => doc
                .layout()
                .detail
                .clone()
                .filter(|x| x.kind == DetailEditorKind::StripChart)?,
            ChartSlot::Controllers => doc.strip_state(slot)?,
        };
        let seq = state.target.seq();
        let lane = doc.strip_lane(slot);
        let tracks = state.target.lane().is_none().then(|| {
            let rows = doc.lane_rows(seq);
            let name = lane
                .and_then(|(_, l)| rows.iter().find(|r| r.id == l))
                .map_or_else(|| "\u{2014}".to_owned(), |r| r.name.clone());
            (name, rows.iter().map(|r| (r.id, r.name.clone())).collect())
        });
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
        let rows = doc.strip_rows(slot);
        let targets = if row_menu || rows.iter().any(|l| matches!(l, StripLane::Automation(_))) {
            doc.automation_targets()
        } else {
            Arc::default()
        };
        let playing_seq = playing.then_some(seq);
        let drawn = rows
            .iter()
            .map(|&l| RowDraw {
                lane: l,
                title: l.title(&targets.names),
                values: doc.strip_values(slot, l),
                preview: doc.strip_preview(slot, l, playing_seq),
            })
            .collect();
        let settings = *doc.strip_settings();
        let meter = doc.strip_meter(slot);
        Some(StripChartState {
            slot,
            settings,
            rows: rows.len(),
            drawn,
            targets,
            selected: doc.selection().events.iter().copied().collect(),
            bars: MeterBars::new(&meter),
            meter,
            range: doc.selection().time_range,
            follow: doc.layout().follow,
            stroke: doc
                .strip_stroke(slot)
                .map(|s| (s.compress, settings.shape == ShapeKind::Free)),
            tracks,
            lane,
            playhead,
            playing,
        })
    }

    /// Playing the chart's sequence (the playhead moves every frame).
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// The playhead's tick, when the transport is on the chart's sequence.
    pub fn playhead(&self) -> Option<f64> {
        self.playhead
    }

    /// The bar's settings shown.
    pub fn settings(&self) -> &StripChartSettings {
        &self.settings
    }
}

/// The chart: its bar, a divider, then the rows' canvas. `scope` is its
/// own container.
pub(crate) fn render(
    chart: &StripChart,
    state: StripChartState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let bar = bar(chart, &state, &mut scope.container(), window, cx);
    let body = body(chart, state, &mut scope.container(), cx);
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .child(bar)
        .child(detail_pane::divider())
        .child(body)
        .into_any_element()
}

/// The rows' canvas: the ruler and rows drawn, the tree recorded, the
/// pointer, keys, wheel and row menu handled.
fn body(
    chart: &StripChart,
    state: StripChartState,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let slot = chart.slot;
    let weak = cx.entity().downgrade();
    // The menu's container first: the recorder numbers the rows' nodes
    // after it.
    let mut menu_scope = scope.container();
    let menu = match chart.menu {
        Some(ChartMenu::Row(i)) => cascade(
            "strip-row-menu",
            row_menu(slot, i, &state, &weak),
            &chart.menu_path,
            Some(chart.menu_at),
            {
                let weak = weak.clone();
                Rc::new(move |path, _, cx: &mut App| {
                    weak.update(cx, |this, cx| {
                        this.chart_mut(slot).menu_path = path;
                        cx.notify();
                    })
                    .ok();
                })
            },
            {
                let weak = weak.clone();
                Rc::new(move |_, cx: &mut App| {
                    weak.update(cx, |this, cx| {
                        this.chart_mut(slot).close_menu();
                        cx.notify();
                    })
                    .ok();
                })
            },
            &mut menu_scope,
        ),
        _ => vec![],
    };
    let overlay = state
        .stroke
        .filter(|_| chart.press.is_some())
        .map(|(compress, free)| Overlay {
            points: chart.preview_points(),
            compress,
            free,
        });
    let scene = Scene {
        frame: chart.frame(),
        state,
        overlay,
    };
    let mut recorder = scope.children();
    let layout = chart.layout.clone();
    let (relayout, events) = (weak.clone(), weak);
    let painter = canvas(
        move |bounds, _, cx| {
            // Another size shows other values than were laid out: draw
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
                        .update(cx, |this, cx| {
                            this.chart_drag_to(slot, e.position, held, cx)
                        })
                        .ok();
                }
            });
            let released = events.clone();
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                    released
                        .update(cx, |this, cx| this.chart_release(slot, e.position, cx))
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
        .id(("strip-chart", slot as usize))
        .track_focus(&chart.focus)
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .bg(rgb(colors::canvas()))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                this.chart_mouse_down(slot, e, window, cx)
            }),
        )
        .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
            if e.keystroke.key == "escape" && this.cancel_chart(slot, cx) {
                cx.stop_propagation();
            }
        }))
        .on_scroll(cx.listener(move |this, e, _, cx| this.chart_wheel(slot, e, cx)))
        .on_pinch(cx.listener(move |this, e, _, cx| this.chart_pinch(slot, e, cx)))
        .child(painter)
        .child(scope.this("group", GROUP, true))
        .child(scope.clip())
        .children(menu)
        .into_any_element()
}

/// The stroke's preview: the pointer path in content points.
struct Overlay {
    points: Vec<(f32, f32)>,
    compress: bool,
    free: bool,
}

/// The view's part of a frame.
struct Scene {
    frame: StripFrame,
    state: StripChartState,
    overlay: Option<Overlay>,
}

/// One row laid out (canvas points: y = 0 is the ruler's top).
struct RowPaint {
    top: f32,
    height: f32,
    centre: Option<f32>,
    /// Bars: x, y, height, selected.
    bars: Vec<(f32, f32, f32, bool)>,
    line: Vec<(f32, f32)>,
    /// Dots: x, y, selected.
    dots: Vec<(f32, f32, bool)>,
    preview: Vec<(f32, f32)>,
    title: String,
}

/// One frame laid out.
struct Prepared {
    time: TimeLayer,
    width: f32,
    rows: Vec<RowPaint>,
    overlay: Option<Overlay>,
}

impl Scene {
    /// Lays out the frame in `bounds`, recording each row's title and the
    /// values in view (`E/StripChartView.swift:200-310,392-410`).
    fn prepare(self, bounds: Bounds<Pixels>, recorder: &mut ChildRecorder) -> Prepared {
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let body_h = (h - RULER_HEIGHT).max(0.);
        let Scene {
            frame,
            state,
            overlay,
        } = self;
        let axis = frame.axis;
        let time = TimeLayer::new(&axis, &state.bars, w, h, state.range, state.playhead);
        let o = bounds.origin;
        let selected = |v: &StripValue| v.id.is_some_and(|id| state.selected.contains(&id));
        let mut rows = vec![];
        for (i, row) in state.drawn.iter().enumerate() {
            let top = frame.row_top(i);
            let height = frame.row_height;
            if body_h <= 0. || top + height <= 0. || top >= body_h {
                continue;
            }
            let s = frame.scale(i, row.lane);
            let title = format!("{} \u{25be}", row.title);
            recorder.record_value(
                "menu",
                &row_label(i),
                canvas_rect(
                    o,
                    0.,
                    RULER_HEIGHT + top,
                    vizdaw_app::strip_chart::TITLE_WIDTH.min(w),
                    vizdaw_app::strip_chart::TITLE_HEIGHT,
                ),
                false,
                &row.title,
            );
            // The values in view, listed (at most 200).
            let mut listed = 0;
            for v in &row.values {
                let x = axis.x(v.tick);
                if x < -CULL || x > w + CULL {
                    continue;
                }
                if listed == TREE_LIMIT {
                    break;
                }
                listed += 1;
                let b = Bbu::of(v.tick, &state.meter);
                recorder.record(
                    "button",
                    &format!(
                        "{} {}, bar {} beat {}",
                        row.title,
                        row.lane.text(v.value, &state.targets.names),
                        b.bar,
                        b.beat
                    ),
                    canvas_rect(o, x - 3., RULER_HEIGHT + s.y(v.value) - 3., 6., 6.),
                    true,
                    selected(v),
                );
            }
            let mut paint = RowPaint {
                top,
                height,
                centre: row.lane.center().map(|c| s.y(c)),
                bars: vec![],
                line: vec![],
                dots: vec![],
                preview: row
                    .preview
                    .iter()
                    .map(|v| (axis.x(v.tick), s.y(v.value)))
                    .collect(),
                title,
            };
            if row.lane.style() == StripStyle::Bars {
                let bottom = s.y(row.lane.range().0);
                for v in &row.values {
                    let x = axis.x(v.tick);
                    if x + BAR_WIDTH + 1. < 0. || x > w {
                        continue;
                    }
                    let y = s.y(v.value);
                    paint.bars.push((x, y, (bottom - y).max(1.), selected(v)));
                }
            } else {
                line_of(&row.values, &axis, &s, w, &selected, &mut paint);
            }
            rows.push(paint);
        }
        Prepared {
            time,
            width: w,
            rows,
            overlay,
        }
    }
}

/// A held or ramped line through `values` with a dot at each, culled to
/// the view plus one value on each side (`E/StripChartView.swift:228-269`).
fn line_of(
    values: &[StripValue],
    axis: &TimeAxis,
    s: &vizdaw_app::strip_chart::StripScale,
    w: f32,
    selected: &dyn Fn(&StripValue) -> bool,
    out: &mut RowPaint,
) {
    let (lo, hi) = (-CULL, w + CULL);
    let mut prev: Option<((f32, f32), bool)> = None;
    let mut past_right = false;
    for v in values {
        let p = (axis.x(v.tick), s.y(v.value));
        if p.0 < lo {
            prev = Some((p, v.ramp));
            continue;
        }
        match prev {
            Some((q, ramp)) => {
                if out.line.is_empty() {
                    out.line.push(q);
                }
                if !ramp {
                    out.line.push((p.0, q.1));
                }
                out.line.push(p);
            }
            None => {
                out.line.push((p.0.min(0.), p.1));
                out.line.push(p);
            }
        }
        prev = Some((p, v.ramp));
        out.dots.push((p.0, p.1, selected(v)));
        if p.0 > hi {
            past_right = true;
            break;
        }
    }
    if let Some((q, _)) = prev
        && !past_right
    {
        if out.line.is_empty() {
            out.line.push((q.0.min(0.), q.1));
        }
        out.line.push((q.0.max(w), q.1));
    }
}

/// A polyline through canvas points `pts` from origin `o`.
fn stroke_path(
    o: Point<Pixels>,
    pts: &[(f32, f32)],
    width: f32,
    colour: Hsla,
    window: &mut Window,
) {
    if pts.len() < 2 {
        return;
    }
    let mut p = PathBuilder::stroke(px(width));
    for (i, &(x, y)) in pts.iter().enumerate() {
        let at = point(o.x + px(x), o.y + px(y));
        if i == 0 {
            p.move_to(at);
        } else {
            p.line_to(at);
        }
    }
    if let Ok(path) = p.build() {
        window.paint_path(path, colour);
    }
}

impl Prepared {
    fn paint(self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let o = bounds.origin;
        let w = self.width;
        let r = RULER_HEIGHT;
        let rect = |x: f32, y: f32, w: f32, h: f32| canvas_rect(o, x, y, w, h);
        let body = self.time.body(o);
        window.with_content_mask(Some(ContentMask { bounds: body }), |window| {
            self.time.paint_grid(o, window);
            let accent = tokens::value_bar();
            let picked = colors::region_selected();
            for row in &self.rows {
                window.paint_quad(fill(
                    rect(0., r + row.top + row.height - 1., w, 1.),
                    rgb(colors::separator()),
                ));
                if let Some(c) = row.centre {
                    window.paint_quad(fill(
                        rect(0., r + c, w, 0.5),
                        with_alpha(colors::separator(), CENTRE_ALPHA),
                    ));
                }
                for &(x, y, h, sel) in &row.bars {
                    let c = if sel {
                        rgb(picked).into()
                    } else {
                        with_alpha(accent, BAR_ALPHA)
                    };
                    window.paint_quad(fill(rect(x, r + y, BAR_WIDTH, h), c));
                }
                let line: Vec<_> = row.line.iter().map(|&(x, y)| (x, r + y)).collect();
                stroke_path(o, &line, LINE_WIDTH, with_alpha(accent, LINE_ALPHA), window);
                for &(x, y, sel) in &row.dots {
                    let c = if sel {
                        rgb(picked).into()
                    } else {
                        with_alpha(accent, LINE_ALPHA)
                    };
                    window.paint_quad(fill(rect(x - DOT / 2., r + y - DOT / 2., DOT, DOT), c));
                }
                let preview: Vec<_> = row.preview.iter().map(|&(x, y)| (x, r + y)).collect();
                stroke_path(o, &preview, LINE_WIDTH, rgb(tokens::arm()).into(), window);
                paint_text(
                    &row.title,
                    o.x + px(6.),
                    o.y + px(r + row.top + 3.),
                    tokens::title(),
                    window,
                    cx,
                );
            }
            self.time.paint_overlay(o, window);
            if let Some(ov) = &self.overlay
                && let (Some(&first), Some(&last)) = (ov.points.first(), ov.points.last())
            {
                let tool = tokens::tool_overlay();
                if ov.compress {
                    let (x, y) = (first.0.min(last.0), first.1.min(last.1));
                    let (bw, bh) = ((last.0 - first.0).abs(), (last.1 - first.1).abs());
                    window.paint_quad(quad(
                        rect(x, r + y, bw, bh),
                        px(0.),
                        with_alpha(tool, OVERLAY_FILL),
                        px(1.),
                        rgb(tool),
                        BorderStyle::Solid,
                    ));
                } else {
                    let pts: Vec<_> = if ov.free {
                        ov.points.iter().map(|&(x, y)| (x, r + y)).collect()
                    } else {
                        vec![(first.0, r + first.1), (last.0, r + last.1)]
                    };
                    stroke_path(o, &pts, 1., rgb(tool).into(), window);
                }
            }
        });
        self.time.paint_ruler(o, window, cx);
    }
}

/// Row `i`'s menu (`E/StripChartView.swift:313-381`): what it can show,
/// the current lane checked; MIDI Learn for an automation row but a
/// velocity scale; Add Row and Remove Row.
fn row_menu(
    slot: ChartSlot,
    i: usize,
    state: &StripChartState,
    weak: &WeakEntity<DocumentWindow>,
) -> Vec<CascadeEntry> {
    let names = &state.targets.names;
    let current = state.drawn.get(i).map_or(StripLane::Velocity, |r| r.lane);
    let n = state.rows;
    let act = |f: Rc<dyn Fn(&mut DocumentModel)>| {
        let weak = weak.clone();
        move |_: &mut Window, cx: &mut App| {
            let f = f.clone();
            weak.update(cx, |this, cx| {
                this.with_model(cx, |d| f(d));
            })
            .ok();
        }
    };
    let item = |lane: StripLane, title: Option<String>| CascadeEntry::Item {
        title: title.unwrap_or_else(|| lane.title(names)),
        checked: lane == current,
        enabled: true,
        pick: Rc::new(act(Rc::new(move |d: &mut DocumentModel| {
            d.set_strip_row(slot, i, lane)
        }))),
    };
    let mut menu = vec![
        item(StripLane::Velocity, None),
        item(StripLane::ReleaseVelocity, None),
        item(StripLane::Duration, None),
        CascadeEntry::Separator,
    ];
    let mut controllers: Vec<_> = StripLane::CONTROLLERS
        .iter()
        .map(|&c| item(StripLane::Controller(c), None))
        .collect();
    controllers.push(CascadeEntry::Separator);
    for group in 0..4u8 {
        let lo = group * 32;
        controllers.push(CascadeEntry::submenu(
            format!("{lo}\u{2013}{}", lo + 31),
            (lo..=lo + 31)
                .map(|c| item(StripLane::Controller(c), Some(format!("Controller {c}"))))
                .collect(),
        ));
    }
    menu.push(CascadeEntry::submenu(CONTROLLER, controllers));
    menu.push(item(StripLane::PitchBend, None));
    menu.push(item(StripLane::ChannelPressure, None));
    let octaves = (0..11u8)
        .map(|octave| {
            let lo = octave * 12;
            let hi = (octave * 12 + 11).min(127);
            CascadeEntry::submenu(
                format!("{}\u{2013}{}", PitchName::name(lo), PitchName::name(hi)),
                (lo..=hi)
                    .map(|p| item(StripLane::PolyPressure(p), None))
                    .collect(),
            )
        })
        .collect();
    menu.push(CascadeEntry::submenu(POLY_PRESSURE, octaves));
    menu.push(item(StripLane::Program, None));
    menu.push(item(StripLane::Tempo, None));
    menu.push(CascadeEntry::Separator);
    let groups: Vec<_> = state
        .targets
        .menus
        .iter()
        .map(|g| {
            CascadeEntry::submenu(
                g.title.clone(),
                g.lanes.iter().map(|&l| item(l, None)).collect(),
            )
        })
        .collect();
    let empty = groups.is_empty();
    menu.push(CascadeEntry::Submenu {
        title: AUTOMATION.into(),
        enabled: !empty,
        help: empty.then(|| AUTOMATION_HELP.to_owned()),
        items: groups,
    });
    // A velocity scale cannot be learned (Ruling 6, plan 5f-1 Task 11).
    if let StripLane::Automation(t) = current
        && learnable(&t)
    {
        menu.push(CascadeEntry::item(
            MIDI_LEARN,
            act(Rc::new(move |d: &mut DocumentModel| d.learn_target(t))),
        ));
    }
    menu.push(CascadeEntry::Separator);
    menu.push(CascadeEntry::Item {
        title: ADD_ROW.into(),
        checked: false,
        enabled: n < MAX_ROWS,
        pick: Rc::new(act(Rc::new(move |d: &mut DocumentModel| {
            d.add_strip_row(slot)
        }))),
    });
    menu.push(CascadeEntry::Item {
        title: REMOVE_ROW.into(),
        checked: false,
        enabled: n > 1,
        pick: Rc::new(act(Rc::new(move |d: &mut DocumentModel| {
            d.remove_strip_row(slot, i)
        }))),
    });
    menu
}

/// Every automation target but a velocity scale can be MIDI-learned
/// (`E/StripChartView.swift:384-388`).
fn learnable(t: &AutomationTargetRow) -> bool {
    !matches!(t, AutomationTargetRow::VelocityScale { .. })
}

/// A content point as row `row`'s tick and value on `d`'s chart in
/// `slot`, the stroke's own lane while one is held.
fn strip_point(
    d: &DocumentModel,
    slot: ChartSlot,
    frame: &StripFrame,
    row: usize,
    at: (f32, f32),
) -> StripPoint {
    let lane = d
        .strip_stroke(slot)
        .map(|s| s.lane)
        .or_else(|| d.strip_rows(slot).get(row).copied())
        .unwrap_or(StripLane::Velocity);
    let meter = d.strip_meter(slot);
    frame.point(
        at.0,
        at.1,
        row,
        lane,
        d.strip_settings(),
        &d.layout().snap,
        &meter,
    )
}

impl DocumentWindow {
    pub(crate) fn chart_mut(&mut self, slot: ChartSlot) -> &mut StripChart {
        match slot {
            ChartSlot::Pane => &mut self.strip_chart,
            ChartSlot::Controllers => &mut self.controllers_chart,
        }
    }

    /// The chart in `slot`.
    pub fn strip_chart(&self, slot: ChartSlot) -> &StripChart {
        match slot {
            ChartSlot::Pane => &self.strip_chart,
            ChartSlot::Controllers => &self.controllers_chart,
        }
    }

    fn toggle_chart_menu(&mut self, slot: ChartSlot, m: ChartMenu, cx: &mut Context<Self>) {
        let c = self.chart_mut(slot);
        c.menu = if c.menu == Some(m) { None } else { Some(m) };
        c.menu_path.clear();
        cx.notify();
    }

    /// After a draw without the chart in `slot`: one that left its pane
    /// drops its stroke, gives back the keyboard and starts afresh.
    pub(crate) fn chart_detach(
        &mut self,
        slot: ChartSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !std::mem::take(&mut self.chart_mut(slot).detach_pending) {
            return;
        }
        self.end_chart_stroke(slot, cx);
        self.chart_mut(slot).unmount();
        self.chart_unfocus(slot, window, cx);
    }

    /// While drawing a chart that is mounted but out of sight (its pane
    /// hidden or showing another view): it keeps no press, no stroke and
    /// no keyboard.
    pub(crate) fn chart_hidden(
        &mut self,
        slot: ChartSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.end_chart_stroke(slot, cx);
        self.chart_mut(slot).close_menu();
        self.chart_unfocus(slot, window, cx);
    }

    /// The window lost the keyboard: each chart's press is dropped and its
    /// stroke cancelled (its release may go elsewhere).
    pub(crate) fn charts_deactivated(&mut self, cx: &mut Context<Self>) {
        for slot in [ChartSlot::Pane, ChartSlot::Controllers] {
            self.end_chart_stroke(slot, cx);
        }
    }

    /// The document window closes: the charts' strokes go first.
    pub(crate) fn charts_closing(
        pane: &mut StripChart,
        controllers: &mut StripChart,
        d: &mut DocumentModel,
    ) {
        for c in [pane, controllers] {
            c.drop_press();
            d.strip_stroke_cancelled(c.slot);
        }
    }

    /// Drops `slot`'s press and cancels its stroke (no undo step).
    fn end_chart_stroke(&mut self, slot: ChartSlot, cx: &mut Context<Self>) {
        let pressed = self.chart_mut(slot).drop_press();
        let stroke = self
            .read_model(cx, |d| d.strip_stroke(slot).is_some())
            .unwrap_or(false);
        if stroke {
            self.with_model(cx, |d| d.strip_stroke_cancelled(slot));
        }
        if pressed {
            cx.notify();
        }
    }

    /// A chart out of sight that kept the keyboard gives it back to the
    /// window.
    fn chart_unfocus(&mut self, slot: ChartSlot, window: &mut Window, cx: &mut Context<Self>) {
        if !self.strip_chart(slot).focus.is_focused(window) {
            return;
        }
        cx.defer_in(window, move |this, window, cx| {
            if this.strip_chart(slot).focus.is_focused(window) {
                window.focus(&this.focus_handle(cx), cx);
            }
        });
    }

    /// Escape on the canvas: a ruler press is dropped, a stroke cancelled
    /// without an undo step. Whether there was anything to cancel.
    fn cancel_chart(&mut self, slot: ChartSlot, cx: &mut Context<Self>) -> bool {
        let stroke = self
            .read_model(cx, |d| d.strip_stroke(slot).is_some())
            .unwrap_or(false);
        if !self.strip_chart(slot).pressing() && !stroke {
            return false;
        }
        self.end_chart_stroke(slot, cx);
        cx.notify();
        true
    }

    /// A press in the canvas: it takes the keyboard; in the ruler it
    /// starts a click or a range drag; on a row's title it opens the
    /// row's menu; elsewhere it starts a stroke (⌥, Alt off macOS: the
    /// selected events only).
    fn chart_mouse_down(
        &mut self,
        slot: ChartSlot,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let c = self.chart_mut(slot);
        window.focus(&c.focus, cx);
        let at = c.content_point(e.position);
        if at.1 < 0. {
            c.press = None;
            c.ruler = Some(RulerDrag {
                anchor: c.axis.tick_at(at.0),
                start_x: at.0,
                x: at.0,
                moved: false,
                preview: None,
            });
            cx.notify();
            return;
        }
        let frame = c.frame();
        let Some(rows) = self.read_model(cx, |d| d.strip_rows(slot).len()) else {
            return;
        };
        let Some(row) = frame.row_at(at.1, rows) else {
            return;
        };
        let c = self.chart_mut(slot);
        if frame.in_title(at.0, at.1, row) {
            c.menu = Some(ChartMenu::Row(row));
            c.menu_at = e.position;
            c.menu_path.clear();
            cx.notify();
            return;
        }
        c.ruler = None;
        c.press = Some(Press { row, at });
        c.moved = false;
        c.path = vec![frame.lane_point(at.0, at.1)];
        let alt = e.modifiers.alt;
        self.with_model(cx, |d| {
            let p = strip_point(d, slot, &frame, row, at);
            d.strip_stroke_began(slot, row, p, alt);
        });
    }

    /// The snapped range between a ruler press at `anchor` and canvas x
    /// `x`; `None` when empty.
    fn chart_ruler_range(
        &self,
        slot: ChartSlot,
        anchor: i64,
        x: f32,
        cx: &App,
    ) -> Option<TickRange> {
        let other = self.strip_chart(slot).axis.tick_at(x);
        self.read_model(cx, |d| {
            let (snap, meter) = (d.layout().snap, d.strip_meter(slot));
            let a = snap.apply(anchor.min(other), &meter);
            let b = snap.apply(anchor.max(other), &meter);
            (b > a).then_some(TickRange { start: a, end: b })
        })
        .flatten()
    }

    /// The pointer moved to window point `at` (`held`: the left button is
    /// down): a ruler drag past the dead zone selects; a stroke past it
    /// sends the stroke so far.
    fn chart_drag_to(
        &mut self,
        slot: ChartSlot,
        at: Point<Pixels>,
        held: bool,
        cx: &mut Context<Self>,
    ) {
        if !held {
            return;
        }
        let c = self.chart_mut(slot);
        let p = c.content_point(at);
        if let Some(mut r) = c.ruler {
            r.x = p.0;
            if !r.moved && (r.x - r.start_x).abs() <= DEAD_ZONE {
                return;
            }
            r.moved = true;
            r.preview = self.chart_ruler_range(slot, r.anchor, r.x, cx);
            self.chart_mut(slot).ruler = Some(r);
            cx.notify();
            return;
        }
        let Some(press) = c.press else {
            return;
        };
        if !c.moved {
            let (dx, dy) = (p.0 - press.at.0, p.1 - press.at.1);
            if dx.hypot(dy) <= DEAD_ZONE {
                return;
            }
            c.moved = true;
        }
        let frame = c.frame();
        c.path.push(frame.lane_point(p.0, p.1));
        self.with_model(cx, |d| {
            let pt = strip_point(d, slot, &frame, press.row, p);
            d.strip_stroke_moved(slot, pt);
        });
        cx.notify();
    }

    /// The button is up at window point `at`: a ruler click locates
    /// (snapped) and a ruler drag selects its range over the shown track;
    /// a stroke ends as one undo step.
    fn chart_release(&mut self, slot: ChartSlot, at: Point<Pixels>, cx: &mut Context<Self>) {
        let c = self.chart_mut(slot);
        if let Some(r) = c.ruler.take() {
            if r.moved {
                if let Some(range) = r.preview {
                    self.with_model(cx, |d| {
                        let Some((seq, lane)) = d.strip_lane(slot) else {
                            return;
                        };
                        d.set_selection(SelectionRow {
                            time_range: Some(range),
                            lanes: vec![LaneRef { seq, lane }],
                            events: vec![],
                        });
                    });
                }
            } else {
                self.with_model(cx, |d| {
                    let (snap, meter) = (d.layout().snap, d.strip_meter(slot));
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
        let Some(press) = c.press.take() else {
            return;
        };
        c.moved = false;
        c.path.clear();
        let frame = c.frame();
        let p = c.content_point(at);
        self.with_model(cx, |d| {
            let pt = strip_point(d, slot, &frame, press.row, p);
            d.strip_stroke_ended(slot, pt);
        });
        // An Exact Mode stroke opens the panel with its line.
        self.present_modify_if_wanted(cx);
        cx.notify();
    }

    /// The live zoom is saved when it changed (both charts share it).
    fn save_chart_zoom(&mut self, slot: ChartSlot, cx: &mut Context<Self>) {
        let tpp = self.strip_chart(slot).axis.ticks_per_point();
        let changed = self
            .read_model(cx, |d| d.strip_settings().ticks_per_point != tpp)
            .unwrap_or(false);
        if changed {
            self.with_model(cx, |d| d.update_strip_settings(|s| s.ticks_per_point = tpp));
        }
    }

    /// The wheel: scrolls the time and the rows (inside them); with the
    /// secondary modifier it zooms about the pointer.
    fn chart_wheel(&mut self, slot: ChartSlot, e: &ScrollEvent, cx: &mut Context<Self>) {
        let (precise, dx, dy) = match e.delta {
            ScrollDelta::Pixels(p) => (true, f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(p) => (false, p.x, p.y),
        };
        let secondary = e.modifiers.secondary(cx);
        let c = self.chart_mut(slot);
        if secondary {
            let x = f32::from(e.position.x - c.layout.get().origin.x);
            c.axis = c.axis.zoom_about(WheelScroll::zoom_factor(dy, precise), x);
            self.save_chart_zoom(slot, cx);
        } else {
            let (dx, dy) = WheelScroll::points(dx, dy, precise);
            c.axis = c.axis.scrolled_by(-dx);
            c.offset -= dy;
            c.clamp_vertical();
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// A pinch zooms about its centre.
    fn chart_pinch(&mut self, slot: ChartSlot, e: &PinchEvent, cx: &mut Context<Self>) {
        let c = self.chart_mut(slot);
        let x = f32::from(e.position.x - c.layout.get().origin.x);
        c.axis = c.axis.zoom_about(1. + f64::from(e.delta), x);
        self.save_chart_zoom(slot, cx);
        cx.stop_propagation();
        cx.notify();
    }
}

fn close_pane_menu(this: &mut DocumentWindow) {
    this.strip_chart.close_menu();
}

fn close_controllers_menu(this: &mut DocumentWindow) {
    this.controllers_chart.close_menu();
}

/// Operations that draw no line shape (`S/StripChartPane.swift:73-77`).
fn shapeless(s: &StripChartSettings) -> bool {
    s.mode == StripMode::Erase
        || (s.mode == StripMode::Modify
            && matches!(
                s.op,
                StripOp::Compress | StripOp::Thin | StripOp::Thicken | StripOp::Legato
            ))
}

/// A bar picker's face: its value and the menu arrow; `menu` in the tree,
/// its value the current title.
fn picker(
    id: (&'static str, usize),
    label: &str,
    face: &str,
    value: &str,
    width: f32,
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
        .px(px(6.))
        .gap(px(4.))
        .round_md()
        .cursor_pointer()
        .text_xs()
        .bg(rgb(bar_colors::button()))
        .text_color(rgb(bar_colors::text()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(face.to_owned()),
        )
        .child(div().flex_none().child("\u{25be}"))
        .child(scope.control_with("menu", label, true, Some(value), false))
}

/// A picker's current title and menu: `list`, the current one checked; a
/// pick sets it.
fn pick<T: Copy + PartialEq + 'static>(
    slot: ChartSlot,
    list: Vec<(String, T)>,
    current: T,
    set: fn(&mut StripChartSettings, T),
) -> (String, Vec<Entry>) {
    let title = title_of(&list, current);
    let entries = list
        .into_iter()
        .map(|(title, v)| {
            Entry::item(&title, None, v == current, move |this, cx| {
                this.chart_mut(slot).close_menu();
                this.with_model(cx, |d| d.update_strip_settings(|s| set(s, v)));
            })
        })
        .collect();
    (title, entries)
}

/// The title of `current` in `list` (the list always holds it).
fn title_of<T: PartialEq>(list: &[(String, T)], current: T) -> String {
    list.iter()
        .find(|(_, v)| *v == current)
        .map(|(t, _)| t.clone())
        .unwrap_or_default()
}

/// Titled choices of a fixed list.
fn titled<T: Copy>(all: &[T], title: fn(T) -> &'static str) -> Vec<(String, T)> {
    all.iter().map(|&v| (title(v).to_owned(), v)).collect()
}

/// The bar (Swift `StripChartBar`), left to right.
fn bar(
    chart: &StripChart,
    state: &StripChartState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let slot = state.slot;
    let s = state.settings;
    let close: fn(&mut DocumentWindow) = match slot {
        ChartSlot::Pane => close_pane_menu,
        ChartSlot::Controllers => close_controllers_menu,
    };
    let mut row = div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .px(px(8.))
        .py(px(4.))
        .bg(rgb(bar_colors::bar()));

    // Mode: three segments, 190 in all.
    let mut mode = div().flex().flex_row().flex_none().gap(px(1.));
    let segment = (MODE_WIDTH - 2.) / 3.;
    for m in StripMode::ALL {
        mode = mode.child(
            bar_button(
                ("strip-mode", m as usize),
                m.title(),
                m.title(),
                segment,
                s.mode == m,
                None,
                scope,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.chart_mut(slot).close_menu();
                this.with_model(cx, |d| d.update_strip_settings(|s| s.mode = m));
            })),
        );
    }
    row = row.child(mode);

    // A picker and, while open, its menu.
    let mut add = |row: gpui::Div,
                   which: ChartMenu,
                   label: &str,
                   face: &str,
                   value: &str,
                   width: f32,
                   entries: Vec<Entry>,
                   scope: &mut RegistryScope| {
        let mut p = picker(
            ("strip-picker", which.index()),
            label,
            face,
            value,
            width,
            scope,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_chart_menu(slot, which, cx)));
        if which == ChartMenu::Step {
            p = crate::sheet_host::with_help(p, STEP_HELP);
        }
        if chart.menu == Some(which) {
            p = p.children(bar_menu(
                "strip-chart-menu",
                entries,
                close,
                &mut scope.container(),
                window,
                cx,
            ));
        }
        row.child(p)
    };
    if s.mode == StripMode::Modify {
        let (t, e) = pick(slot, titled(&StripOp::ALL, StripOp::title), s.op, |s, v| {
            s.op = v
        });
        row = add(row, ChartMenu::Operation, OPERATION, &t, &t, 120., e, scope);
    }
    if !shapeless(&s) {
        let list = titled(&ShapeKind::ALL, ShapeKind::title);
        let (t, e) = pick(slot, list, s.shape, |s, v| s.shape = v);
        row = add(row, ChartMenu::Shape, SHAPE, &t, &t, 110., e, scope);
        if s.shape == ShapeKind::Curved {
            let list = titled(&LineCurve::ALL, LineCurve::title);
            let (t, e) = pick(slot, list, s.curve, |s, v| s.curve = v);
            row = add(row, ChartMenu::Curve, CURVE, &t, &t, 100., e, scope);
        }
        if s.shape == ShapeKind::Oscillator {
            let list = titled(&WaveKind::ALL, WaveKind::title);
            let (t, e) = pick(slot, list, s.wave, |s, v| s.wave = v);
            row = add(row, ChartMenu::Wave, WAVE, &t, &t, 90., e, scope);
            let list = titled(&PeriodKind::ALL, PeriodKind::title);
            let (t, e) = pick(slot, list, s.period, |s, v| s.period = v);
            row = add(
                row,
                ChartMenu::PeriodKind,
                PERIOD_KIND,
                &t,
                &t,
                150.,
                e,
                scope,
            );
            let ((t, e), width) = match s.period {
                PeriodKind::Musical => (
                    pick(
                        slot,
                        StripBarChoices::musical_periods(s.period_ticks),
                        s.period_ticks,
                        |s, v| s.period_ticks = v,
                    ),
                    140.,
                ),
                PeriodKind::Clock => (
                    pick(
                        slot,
                        StripBarChoices::clock_periods(s.period_ms),
                        s.period_ms,
                        |s, v| s.period_ms = v,
                    ),
                    120.,
                ),
                PeriodKind::Bars => (
                    pick(
                        slot,
                        StripBarChoices::bar_periods(s.period_bars),
                        s.period_bars,
                        |s, v| s.period_bars = v,
                    ),
                    120.,
                ),
            };
            row = add(row, ChartMenu::Period, PERIOD, &t, &t, width, e, scope);
            let list = StripBarChoices::phases(s.phase);
            let (t, e) = pick(slot, list, s.phase, |s, v| s.phase = v);
            row = add(row, ChartMenu::Phase, PHASE, &t, &t, 100., e, scope);
        }
    }
    let (t, e) = pick(slot, StripBarChoices::steps(s.step), s.step, |s, v| {
        s.step = v
    });
    row = add(row, ChartMenu::Step, STEP, &t, &t, 110., e, scope);

    // The Track menu of a sequence target (Ruling 13).
    if let Some((name, tracks)) = &state.tracks {
        let entries = tracks
            .iter()
            .map(|(id, track)| {
                let lane = state.lane.map(|(seq, _)| (seq, *id));
                Entry::item(
                    track,
                    None,
                    state.lane.map(|l| l.1) == Some(*id),
                    move |this, cx| {
                        this.chart_mut(slot).close_menu();
                        if let Some(lane) = lane {
                            this.with_model(cx, |d| d.set_strip_chosen_lane(slot, lane));
                        }
                    },
                )
            })
            .collect();
        let face = format!("{TRACK}: {name}");
        row = add(
            row,
            ChartMenu::Track,
            TRACK,
            &face,
            name,
            150.,
            entries,
            scope,
        );
    }

    // The rows stepper.
    let n = state.rows;
    let count = format!("Rows: {n}");
    let button = |id: &'static str, glyph: &str, label: &str, on: bool, scope: &mut _| {
        transport_bar::cluster_button(id, glyph, label, on, bar_colors::button(), scope)
    };
    let (fewer, more) = (n > 1, n < MAX_ROWS);
    let mut decrease = button("strip-rows-down", "\u{2212}", DECREASE_ROWS, fewer, scope);
    if fewer {
        decrease = decrease.on_click(cx.listener(move |this, _, _, cx| {
            this.chart_mut(slot).close_menu();
            this.with_model(cx, |d| {
                let last = d.strip_rows(slot).len().saturating_sub(1);
                d.remove_strip_row(slot, last);
            });
        }));
    }
    let mut increase = button("strip-rows-up", "+", INCREASE_ROWS, more, scope);
    if more {
        increase = increase.on_click(cx.listener(move |this, _, _, cx| {
            this.chart_mut(slot).close_menu();
            this.with_model(cx, |d| d.add_strip_row(slot));
        }));
    }
    row.child(div().flex_1())
        .child(
            div()
                .relative()
                .flex_none()
                .text_xs()
                .text_color(rgb(bar_colors::text()))
                .child(count.clone())
                .child(scope.control("text", &count, true)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .gap(px(4.))
                .child(decrease)
                .child(increase),
        )
        .into_any_element()
}
