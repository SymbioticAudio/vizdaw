//! The Strip Chart (Swift `StripChartEditor`, `StripChartBar` and
//! `StripChartView`, `S/StripChartPane.swift`): the bottom pane's, and the
//! right pane's Controllers view (the focused track's,
//! `S/PaneViews.swift:223-235`). A bar (Mode, Operation, Shape and its
//! parameters, Step, the Track menu of a sequence target, Rows), then the
//! rows' canvas, a `group` "Strip Chart" (Task 6 draws it).
//!
//! The view holds layout only. What each chart shows and every change
//! is the model's (`vizdaw_app::strip_chart`): the bar's settings go
//! through `update_strip_settings`, the track through
//! `set_strip_chosen_lane`, the rows through `add_strip_row` and
//! `remove_strip_row`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    AnyElement, Bounds, Context, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Pixels, StatefulInteractiveElement, Styled, Window, canvas, div, px, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::TimeAxis;
use vizdaw_app::editor::{DetailEditorKind, DetailState, EditorTarget, PaneView};
use vizdaw_app::select_modify::WaveKind;
use vizdaw_app::strip_chart::{
    ChartSlot, LineCurve, MAX_ROWS, PeriodKind, ShapeKind, StripBarChoices, StripChartSettings,
    StripMode, StripOp,
};
use vizdaw_app::transport::TransportModel;

use crate::appearance::ThemedCorners as _;
use crate::arrangement::{bar_button, colors};
use crate::detail_pane;
use crate::list_editor::{Entry, bar_menu};
use crate::registry::RegistryScope;
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
    /// The row menu's open submenus (Task 6).
    pub(crate) menu_path: Vec<usize>,
    /// The chart left its pane since the last draw: the window drops its
    /// stroke before the view starts afresh.
    detach_pending: bool,
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
            detach_pending: false,
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
    }
}

/// What a chart draws of the document this frame.
pub struct StripChartState {
    slot: ChartSlot,
    settings: StripChartSettings,
    rows: usize,
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
    /// Chart; no track is focused for the Controllers).
    pub fn of(
        doc: &DocumentModel,
        slot: ChartSlot,
        transport: Option<&TransportModel>,
        now: Instant,
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
        Some(StripChartState {
            slot,
            settings: *doc.strip_settings(),
            rows: doc.strip_rows(slot).len(),
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
    let body = body(chart, &mut scope.container());
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

/// The rows' canvas: its bounds measured for the view (Task 6 draws the
/// rows).
fn body(chart: &StripChart, scope: &mut RegistryScope) -> AnyElement {
    let layout = chart.layout.clone();
    div()
        .id(("strip-chart", chart.slot as usize))
        .track_focus(&chart.focus)
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .bg(rgb(colors::canvas()))
        .child(
            canvas(move |bounds, _, _| layout.set(bounds), |_, _, _, _| {})
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
        )
        .child(scope.this("group", GROUP, true))
        .into_any_element()
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
    /// drops its stroke and starts afresh.
    pub(crate) fn chart_detach(&mut self, slot: ChartSlot, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.chart_mut(slot).detach_pending) {
            return;
        }
        if self
            .read_model(cx, |d| d.strip_stroke(slot).is_some())
            .unwrap_or(false)
        {
            self.with_model(cx, |d| d.strip_stroke_cancelled(slot));
        }
        self.chart_mut(slot).unmount();
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
            ("strip-picker", which as usize),
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
