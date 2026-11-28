//! The pane host (Swift `DocumentShell.regions`, `PaneFrame`, `PaneHost`,
//! `PaneDivider` and `PaneGeometry`): the area under the transport bar
//! holds the centre (the arrangement, or the mixer in the Mix view), and
//! the left, bottom and right panes of the current main view's layout,
//! each with its picker over its content, and a 1-point divider on each
//! shown pane's inner edge, dragged to resize it (the size is set once,
//! when the drag ends). A window too small for every shown pane drops
//! some for the frame only (the layout keeps them shown).

use crate::appearance::ThemedCorners as _;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, DispatchPhase, Div, Focusable, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point,
    StatefulInteractiveElement, Styled, Window, anchored, canvas, deferred, div, point, px, rgb,
};
use vizdaw_app::editor::{
    DetailEditorKind, EditorLayout, MainView, PaneSetup, PaneSide, PaneView, ViewLayouts,
};
use vizdaw_app::marker_list::{self, MarkerList, MarkerListRow};
use vizdaw_app::{DocumentModel, view};

use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::transport_bar::colors;

/// The line between two regions (Swift `PaneGeometry.divider`).
pub const DIVIDER: f64 = 1.;
/// What the centre keeps, at least (Swift `centreMinimum`).
pub const CENTRE_MINIMUM: (f64, f64) = (240., 160.);
/// The narrowest the bottom pane shows (Swift `bottomMinimumWidth`).
pub const BOTTOM_MINIMUM_WIDTH: f64 = 240.;
/// A divider's hit area across its line (Swift `PaneDivider.grip`).
const GRIP: f64 = 7.;
/// The Controllers view with no track to chart (`S/PaneViews.swift:233`).
pub const CONTROLLERS_NOTE: &str = "Select a track to see its controllers.";
/// The mixer's group in the tree: the Mix view's centre and the bottom
/// pane's Mixer view.
pub const MIXER: &str = "Mixer";
/// The editor kinds the bottom pane's picker lists, in Swift's order (the
/// others come with their editors).
const KINDS: [DetailEditorKind; 3] = [
    DetailEditorKind::PianoRoll,
    DetailEditorKind::List,
    DetailEditorKind::StripChart,
];

/// A region's rectangle in the area under the transport bar (top-left
/// origin).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0.,
        y: 0.,
        w: 0.,
        h: 0.,
    };

    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }
}

/// The regions' rectangles (Swift `PaneFrames`).
#[derive(Debug, Clone, PartialEq)]
pub struct Frames {
    pub centre: Rect,
    pub left: Option<Rect>,
    pub bottom: Option<Rect>,
    pub right: Option<Rect>,
    /// Panes dropped for this frame only; the layout's `shown` flags are
    /// unchanged.
    pub hidden: Vec<PaneSide>,
}

impl Frames {
    pub fn of(&self, side: PaneSide) -> Option<Rect> {
        match side {
            PaneSide::Left => self.left,
            PaneSide::Bottom => self.bottom,
            PaneSide::Right => self.right,
        }
    }

    /// The size `side` shows at (its width, the bottom pane's height), or
    /// `None` when it does not show.
    pub fn size(&self, side: PaneSide) -> Option<f64> {
        let r = self.of(side)?;
        Some(if side == PaneSide::Bottom { r.h } else { r.w })
    }

    /// `setup` as these frames show it: each shown pane at its displayed
    /// (perhaps shrunk) size, a pane dropped for this frame hidden. A
    /// divider drag starts from it, so its first change does not jump.
    pub fn displayed(&self, setup: &PaneSetup) -> PaneSetup {
        let mut s = *setup;
        for side in PaneSide::ALL {
            match self.size(side) {
                Some(v) => s.pane_mut(side).size = v,
                None => s.pane_mut(side).shown = false,
            }
        }
        s
    }
}

fn lower(side: PaneSide) -> f64 {
    *side.size_range().start()
}

/// Sizes after shrinking (left, bottom, right), or `None` when the shown
/// panes cannot fit (Swift `PaneGeometry.fit`).
fn fit(w: f64, h: f64, shown: &[PaneSide], setup: &PaneSetup) -> Option<[Option<f64>; 3]> {
    let clamped = |side: PaneSide| {
        let r = side.size_range();
        let v = setup.pane(side).size;
        (if v.is_finite() { v } else { *r.start() }).clamp(*r.start(), *r.end())
    };
    let on = |side| shown.contains(&side).then(|| clamped(side));
    let (mut left, mut bottom, mut right) = (
        on(PaneSide::Left),
        on(PaneSide::Bottom),
        on(PaneSide::Right),
    );
    let d = DIVIDER;
    let (cw, ch) = CENTRE_MINIMUM;
    let left_cost = left.unwrap_or(-d) + d;
    // Right: the column beside the left pane must hold the centre (or the
    // bottom pane's minimum) plus a divider plus the right pane.
    if let Some(r) = right {
        let avail = w - left_cost;
        right = Some(lower(PaneSide::Right).max(r.min(avail - cw - d)));
    }
    if let Some(b) = bottom {
        bottom = Some(lower(PaneSide::Bottom).max(b.min(h - d - ch)));
    }
    if let Some(l) = left {
        let need = right.map_or(cw, |r| cw + d + r);
        left = Some(lower(PaneSide::Left).max(l.min(w - d - need)));
    }
    // Verify.
    let x0 = left.unwrap_or(-d) + d;
    let right_cost = right.map_or(0., |r| r + d);
    if w - x0 - right_cost < cw {
        return None;
    }
    match bottom {
        Some(b) => {
            if h - d - b < ch || w - x0 - right_cost < BOTTOM_MINIMUM_WIDTH {
                return None;
            }
        }
        None => {
            if h < ch {
                return None;
            }
        }
    }
    Some([left, bottom, right])
}

/// The regions of an area `w` × `h` for `setup` (Swift
/// `PaneGeometry.frames`): what does not fit drops, the right pane first,
/// then the left, then the bottom.
pub fn frames(w: f64, h: f64, setup: &PaneSetup) -> Frames {
    let mut shown: Vec<PaneSide> = PaneSide::ALL
        .into_iter()
        .filter(|s| setup.pane(*s).shown)
        .collect();
    let mut hidden = vec![];
    let mut sizes = fit(w, h, &shown, setup);
    for side in [PaneSide::Right, PaneSide::Left, PaneSide::Bottom] {
        if sizes.is_some() {
            break;
        }
        if !shown.contains(&side) {
            continue;
        }
        shown.retain(|s| *s != side);
        hidden.push(side);
        sizes = fit(w, h, &shown, setup);
    }
    let [left, bottom, right] = sizes.unwrap_or([None; 3]);
    let d = DIVIDER;
    let x0 = left.map_or(0., |l| l + d);
    let column = w - x0;
    let beside_right = right.map_or(0., |r| r + d);
    let centre = Rect::new(
        x0,
        0.,
        column - if bottom.is_none() { beside_right } else { 0. },
        bottom.map_or(h, |b| h - b - d),
    );
    Frames {
        centre,
        left: left.map(|l| Rect::new(0., 0., l, h)),
        bottom: bottom.map(|b| Rect::new(x0, h - b, column - beside_right, b)),
        right: right
            .map(|r| Rect::new(w - r, bottom.map_or(0., |b| h - b), r, bottom.unwrap_or(h))),
        hidden,
    }
}

/// The new, clamped size of `side` after a divider drag of `delta` points
/// in an area `w` × `h` (Swift `PaneGeometry.drag`): left grows with a
/// positive delta, right with a negative one, bottom with a positive one
/// (the divider passes the pointer's upward travel). A non-finite delta
/// keeps the size.
pub fn drag(setup: &PaneSetup, side: PaneSide, delta: f64, w: f64, h: f64) -> f64 {
    let range = side.size_range();
    let (lo, hi) = (*range.start(), *range.end());
    let current = setup.pane(side).size;
    let start = if current.is_finite() {
        current.clamp(lo, hi)
    } else {
        lo
    };
    if !delta.is_finite() {
        return start;
    }
    let signed = if side == PaneSide::Right {
        -delta
    } else {
        delta
    };
    // Never grow past what leaves the centre its minimum.
    let d = DIVIDER;
    let (cw, ch) = CENTRE_MINIMUM;
    let upper = match side {
        PaneSide::Left => {
            let others = if setup.right.shown {
                cw + d + setup.right.size
            } else {
                cw
            };
            hi.min(w - d - others)
        }
        PaneSide::Right => {
            let left = if setup.left.shown {
                setup.left.size + d
            } else {
                0.
            };
            hi.min(w - left - cw - d)
        }
        PaneSide::Bottom => hi.min(h - d - ch),
    }
    .max(lo);
    (start + signed).clamp(lo, upper)
}

/// A divider's line (Swift `PaneDivider.line`): the left pane's right
/// edge, the bottom row's top edge (across the right pane too), the right
/// pane's left edge.
pub fn line(f: &Frames, side: PaneSide, w: f64, h: f64) -> Option<Rect> {
    let d = DIVIDER;
    match side {
        PaneSide::Left => f.left.map(|r| Rect::new(r.x + r.w, 0., d, h)),
        PaneSide::Bottom => f.bottom.map(|r| Rect::new(r.x, r.y - d, w - r.x, d)),
        PaneSide::Right => f.right.map(|r| Rect::new(r.x - d, r.y, d, r.h)),
    }
}

/// A pane's name (Swift `PaneSide.title`): its group, its picker and its
/// transport bar toggle.
pub fn side_title(side: PaneSide) -> &'static str {
    match side {
        PaneSide::Left => "Left Pane",
        PaneSide::Bottom => "Bottom Pane",
        PaneSide::Right => "Right Pane",
    }
}

/// A main view's name (Swift `MainView.title`).
pub fn main_title(main: MainView) -> &'static str {
    match main {
        MainView::Arrange => "Arrange",
        MainView::Mix => "Mix",
    }
}

/// A pane view's name (Swift `PaneView.title`).
pub fn view_title(v: PaneView) -> &'static str {
    match v {
        PaneView::Strip => "Channel Strip",
        PaneView::Sequences => "Sequences",
        PaneView::Inspector => "Inspector",
        PaneView::Editor => "Editor",
        PaneView::Mixer => "Mixer",
        PaneView::Controllers => "Controllers",
        PaneView::MediaPool => "Media Pool",
        PaneView::Plugins => "Plug-ins",
        PaneView::Markers => "Markers",
        PaneView::AudioEditor => "Audio Editor",
    }
}

/// What a pane's picker can choose: a view, or (the bottom pane) an editor
/// kind (Swift `PaneChoice`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    View(PaneView),
    Kind(DetailEditorKind),
}

impl Choice {
    pub fn title(self) -> &'static str {
        match self {
            Choice::View(v) => view_title(v),
            Choice::Kind(k) => k.title(),
        }
    }

    /// The pane's choices in `main`: its views; the bottom pane lists the
    /// editor kinds, then its other views.
    pub fn all(side: PaneSide, main: MainView) -> Vec<Choice> {
        let views = side.views(main).iter().copied();
        if side != PaneSide::Bottom {
            return views.map(Choice::View).collect();
        }
        KINDS
            .into_iter()
            .map(Choice::Kind)
            .chain(views.filter(|v| *v != PaneView::Editor).map(Choice::View))
            .collect()
    }

    /// What the pane shows now: the bottom pane's editor is the open
    /// detail's kind, else the last used.
    pub fn current(side: PaneSide, l: &EditorLayout) -> Choice {
        let p = l.views.current().pane(side);
        if side == PaneSide::Bottom && p.view == PaneView::Editor {
            return Choice::Kind(l.detail.as_ref().map_or(l.last_kind, |d| d.kind));
        }
        Choice::View(p.view)
    }
}

/// What the host draws of the document's layout.
#[derive(Debug, Clone, PartialEq)]
pub struct HostState {
    pub main: MainView,
    pub setup: PaneSetup,
    pub current: [Choice; 3],
    /// No track is selected or armed: the strip and controllers views say
    /// so.
    pub nothing_focused: bool,
    /// What the left pane's Channel Strip view shows.
    pub left_strip: LeftStrip,
    /// The Markers view's rows, read only while the right pane shows it:
    /// they follow the snapshot's markers and the layout's locators.
    pub markers: Vec<MarkerListRow>,
}

/// What the left pane's Channel Strip view shows (Swift `LeftStripPane`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftStrip {
    /// No track is focused: the note.
    Nothing,
    /// A track is focused but the mixer is not read yet: the placement
    /// shows (and reads it).
    Unread,
    /// The focused track's strip: the placement shows it.
    Strip,
    /// The focused track has no strip: the inspector's placeholder.
    Inspector,
    /// The focus names a strip the mixer does not have: the note.
    Missing,
}

impl LeftStrip {
    pub fn of(d: &DocumentModel) -> Self {
        if d.focus_lanes().is_empty() {
            return LeftStrip::Nothing;
        }
        let Some(m) = d.mixer() else {
            return LeftStrip::Unread;
        };
        if crate::mixer_view::has_focused_column(m, d) {
            LeftStrip::Strip
        } else if d.left_strip(&m.snapshot().strips, m.destinations())
            == view::StripFocus::Inspector
        {
            LeftStrip::Inspector
        } else {
            LeftStrip::Missing
        }
    }

    /// The left pane's strip placement shows.
    pub fn placed(self) -> bool {
        matches!(self, LeftStrip::Unread | LeftStrip::Strip)
    }
}

impl HostState {
    pub fn of(d: &DocumentModel) -> Self {
        let l = d.layout();
        let right = l.views.current().right;
        HostState {
            main: l.views.main,
            setup: *l.views.current(),
            current: PaneSide::ALL.map(|s| Choice::current(s, l)),
            nothing_focused: d.focus_lanes().is_empty(),
            left_strip: LeftStrip::of(d),
            markers: if right.shown && right.view == PaneView::Markers {
                MarkerList::of(d)
            } else {
                vec![]
            },
        }
    }

    /// The default layout's, for a window whose document is gone.
    pub fn fallback() -> Self {
        let v = ViewLayouts::default();
        HostState {
            main: v.main,
            setup: v.arrange,
            current: [
                Choice::View(v.arrange.left.view),
                Choice::Kind(DetailEditorKind::PianoRoll),
                Choice::View(v.arrange.right.view),
            ],
            nothing_focused: true,
            left_strip: LeftStrip::Nothing,
            markers: vec![],
        }
    }

    pub fn choice(&self, side: PaneSide) -> Choice {
        self.current[side as usize]
    }

    /// The bottom pane's view is its editor (shown or not).
    pub fn editor_chosen(&self) -> bool {
        self.setup.bottom.view == PaneView::Editor
    }
}

/// A divider drag in progress: the size shows only on the line until the
/// release sets it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneDrag {
    pub side: PaneSide,
    /// The size the pane showed at when the drag began.
    start: f64,
    origin: Point<Pixels>,
    /// The layout as the frames showed it, `side` at `start`.
    seeded: PaneSetup,
    area: (f64, f64),
    /// The size the drag would give; `None` before the first move.
    live: Option<f64>,
}

impl PaneDrag {
    /// Starts a drag of `side`'s divider at `origin` in an area of `area`;
    /// `None` when the pane does not show.
    pub fn begin(
        side: PaneSide,
        origin: Point<Pixels>,
        setup: &PaneSetup,
        area: (f64, f64),
    ) -> Option<PaneDrag> {
        let f = frames(area.0, area.1, setup);
        let start = f.size(side)?;
        let mut seeded = f.displayed(setup);
        seeded.pane_mut(side).size = start;
        Some(PaneDrag {
            side,
            start,
            origin,
            seeded,
            area,
            live: None,
        })
    }

    /// The pointer moved to `at`: the size the drag would give now.
    pub fn moved(&mut self, at: Point<Pixels>) {
        let delta = if self.side == PaneSide::Bottom {
            f64::from(f32::from(self.origin.y - at.y))
        } else {
            f64::from(f32::from(at.x - self.origin.x))
        };
        self.live = Some(drag(
            &self.seeded,
            self.side,
            delta,
            self.area.0,
            self.area.1,
        ));
    }

    /// The size to set at the release, when it changed.
    pub fn result(&self) -> Option<f64> {
        self.live.filter(|l| *l != self.start)
    }

    /// How far the line moved: (x, y).
    fn offset(&self) -> (f64, f64) {
        let delta = self.live.map_or(0., |l| l - self.start);
        match self.side {
            PaneSide::Left => (delta, 0.),
            PaneSide::Right => (-delta, 0.),
            PaneSide::Bottom => (0., -delta),
        }
    }
}

impl DocumentWindow {
    /// A press on `side`'s divider: the grip takes it (nothing under it
    /// sees it), and the keyboard goes to the window, as a press on any
    /// chrome gives it (a text edit ends).
    fn pane_drag_begin(
        &mut self,
        side: PaneSide,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle(cx), cx);
        let area = self.regions_area();
        let Some(setup) = self.read_model(cx, |d| *d.layout().views.current()) else {
            return;
        };
        self.pane_drag = PaneDrag::begin(side, at, &setup, area);
        cx.notify();
    }

    fn pane_drag_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(drag) = &mut self.pane_drag {
            drag.moved(at);
            cx.notify();
        }
    }

    /// The release: the size is set, once, in the current main view.
    fn pane_drag_end(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.pane_drag.take() else {
            return;
        };
        if let Some(size) = drag.result() {
            self.with_model(cx, |d| {
                d.update_layout(|l| {
                    let main = l.views.main;
                    // No view is given: nothing to refuse.
                    let _ = l.views.set_pane(drag.side, None, None, Some(size), main);
                })
            });
        }
        cx.notify();
    }

    /// A drag whose release may never come (the window lost the
    /// keyboard) ends where it was, setting nothing.
    pub(crate) fn cancel_pane_drag(&mut self, cx: &mut Context<Self>) {
        if self.pane_drag.take().is_some() {
            cx.notify();
        }
    }

    /// A pick in `side`'s picker, through `ui.pane`'s path; a refusal
    /// shows in the status line.
    fn choose_pane(&mut self, side: PaneSide, choice: Choice, cx: &mut Context<Self>) {
        self.pane_menu = None;
        self.with_model(cx, |d| {
            let chosen = match choice {
                Choice::View(v) => view::choose(side, v, None, None, None, None, d),
                Choice::Kind(k) => {
                    view::choose(side, PaneView::Editor, Some(k), None, None, None, d)
                }
            };
            if let Err(e) = chosen {
                d.set_status(Some(e));
            }
        });
    }
}

/// `div` placed at `r` in the area; what does not fit is cut at the right
/// and bottom.
pub(crate) fn place(r: Rect) -> Div {
    div()
        .absolute()
        .left(px(r.x as f32))
        .top(px(r.y as f32))
        .w(px(r.w.max(0.) as f32))
        .h(px(r.h.max(0.) as f32))
        .overflow_hidden()
}

/// `div` placed at `r` in an area measured `area` (w, h) last frame,
/// anchored so it follows the area as it is laid out this frame (a status
/// line that came or went): an axis in `stretch` keeps `r`'s distance
/// from both edges; any other keeps its size, on the area's far edge when
/// `r` ends there, else from the near edge. What does not fit is cut.
pub(crate) fn place_in(r: Rect, area: (f64, f64), stretch: (bool, bool)) -> Div {
    let near = |a: f64| a.abs() < 0.5;
    let (right, bottom) = (area.0 - r.x - r.w, area.1 - r.y - r.h);
    let mut d = div().absolute().overflow_hidden();
    d = if stretch.0 || (near(r.x) && near(right)) {
        d.left(px(r.x as f32)).right(px(right as f32))
    } else if near(right) {
        d.right(px(0.)).w(px(r.w.max(0.) as f32))
    } else {
        d.left(px(r.x as f32)).w(px(r.w.max(0.) as f32))
    };
    if stretch.1 || (near(r.y) && near(bottom)) {
        d.top(px(r.y as f32)).bottom(px(bottom as f32))
    } else if near(bottom) {
        d.bottom(px(0.)).h(px(r.h.max(0.) as f32))
    } else {
        d.top(px(r.y as f32)).h(px(r.h.max(0.) as f32))
    }
}

/// A pane at `rect` (`None`: the bottom pane hidden, kept mounted at zero
/// size and out of sight): its picker, a divider, then `content`. `scope`
/// is the pane's own container (a scratch one when hidden).
#[allow(clippy::too_many_arguments)]
pub(crate) fn frame(
    side: PaneSide,
    rect: Option<Rect>,
    area: (f64, f64),
    state: &HostState,
    menu_open: bool,
    content: AnyElement,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let shown = rect.is_some();
    let picker = picker(side, state, menu_open && shown, scope, window, cx);
    // The bottom pane spans the centre's column; the others keep their
    // width. A hidden pane has no size.
    let placed = match rect {
        Some(r) => place_in(r, area, (side == PaneSide::Bottom, false)),
        None => place(Rect::ZERO),
    };
    placed
        .flex()
        .flex_col()
        .when(!shown, |d| d.invisible())
        .bg(rgb(colors::window()))
        .child(picker)
        .child(crate::detail_pane::divider())
        .child(
            div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(content),
        )
        .child(scope.this("group", side_title(side), true))
        .into_any_element()
}

/// The pane's header: its picker (Swift's menu-style `Picker`, labelled
/// with the pane's name), showing the current choice.
fn picker(
    side: PaneSide,
    state: &HostState,
    menu_open: bool,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let title = state.choice(side).title();
    let label = side_title(side);
    let mut button = div()
        .id(("pane-picker", side as usize))
        .relative()
        .h(px(24.))
        .flex()
        .flex_none()
        .items_center()
        .px_2()
        .gap_1()
        .round_md()
        .cursor_pointer()
        .text_xs()
        .bg(rgb(colors::button()))
        .text_color(rgb(colors::text()))
        .whitespace_nowrap()
        .child(format!("{title} \u{25be}"))
        .child(scope.control_with("button", label, true, Some(title), false))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.pane_menu = if this.pane_menu == Some(side) {
                None
            } else {
                Some(side)
            };
            cx.notify();
        }));
    if menu_open {
        button = button.children(choice_menu(side, state, &mut scope.container(), window, cx));
    }
    div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .px(px(8.))
        .py(px(4.))
        .bg(rgb(colors::bar()))
        .child(button)
        .child(div().flex_1())
        .into_any_element()
}

/// The picker's menu: the pane's choices, the current one checked.
fn choice_menu(
    side: PaneSide,
    state: &HostState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> Vec<AnyElement> {
    let size = window.viewport_size();
    let backdrop = deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id(("pane-backdrop", side as usize))
                .w(size.width)
                .h(size.height)
                .occlude()
                .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                    this.pane_menu = None;
                    cx.notify();
                })),
        ),
    )
    .with_priority(1)
    .into_any_element();
    let current = state.choice(side);
    let mut list = crate::arrangement::menu_list("pane-menu");
    for (row, choice) in Choice::all(side, state.main).into_iter().enumerate() {
        let on = current == choice;
        list = list.child(
            div()
                .id(("pane-choice", row))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::button())))
                .child(format!(
                    "{}{}",
                    if on { "\u{2713} " } else { "" },
                    choice.title()
                ))
                .child(scope.control_selected("menuitem", choice.title(), true, None, on))
                .on_click(cx.listener(move |this, _, _, cx| this.choose_pane(side, choice, cx))),
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

/// The note a view whose content comes in a later version shows (Swift
/// `PaneNote`); the strip view with no track to follow says so, and the
/// Controllers view whenever it has no chart to show
/// (`S/PaneViews.swift:223-235`), as Swift's do.
pub fn note(view: PaneView, nothing_focused: bool) -> String {
    match view {
        PaneView::Strip if nothing_focused => "Select a track to see its channel strip.".into(),
        PaneView::Controllers => CONTROLLERS_NOTE.into(),
        v => format!("{} comes in a later version.", view_title(v)),
    }
}

/// A pane's content for `view` (not the bottom pane's editor): the mixer's
/// group, the Markers view, or the view's note. `scope` is its own
/// container.
pub(crate) fn content(
    view: PaneView,
    state: &HostState,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    match view {
        PaneView::Mixer => mixer(div().into_any_element(), scope),
        PaneView::Markers => markers(&state.markers, scope, cx),
        _ => note_view(&note(view, state.nothing_focused), scope),
    }
}

/// The Markers view (Swift `MarkersPane`): the note with nothing to list;
/// else a row per marker and set locator, its name and its position, a
/// button (labelled with its name, its position the value, its help
/// "Go to <name>") that moves the transport there.
fn markers(
    rows: &[MarkerListRow],
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    if rows.is_empty() {
        return note_view(marker_list::EMPTY, scope);
    }
    let mut list = div()
        .id("markers")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .py(px(4.));
    for (i, row) in rows.iter().enumerate() {
        let go = row.clone();
        let item = crate::sheet_host::with_help(
            div().id(("marker-row", i)),
            format!("Go to {}", row.name),
        );
        list = list.child(
            item.relative()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .h(px(22.))
                .cursor_pointer()
                .text_xs()
                .text_color(rgb(colors::text()))
                .hover(|s| s.bg(rgb(colors::button())))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(row.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(colors::dim()))
                        .child(row.position.clone()),
                )
                .child(scope.control_with("button", &row.name, true, Some(&row.position), false))
                .on_click(cx.listener(move |this, _, _, cx| this.go_to_marker(&go, cx))),
        );
    }
    list.into_any_element()
}

/// A pane's note, centred (Swift `PaneNote`).
pub(crate) fn note_view(text: &str, scope: &mut RegistryScope) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .p(px(16.))
        .child(
            div()
                .relative()
                .text_xs()
                .text_color(rgb(colors::dim()))
                .child(text.to_owned())
                .child(scope.control("text", text, true)),
        )
        .into_any_element()
}

/// The mixer's place, holding `content` (the placement's strips): the Mix
/// view's centre and the bottom pane's Mixer view. `scope` is its own
/// container, the one `content` was drawn in.
pub(crate) fn mixer(content: AnyElement, scope: &RegistryScope) -> AnyElement {
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .size_full()
        .overflow_hidden()
        .bg(rgb(colors::window()))
        .child(content)
        .child(scope.this("group", MIXER, true))
        .into_any_element()
}

/// The dividers of the shown panes, over the regions (Swift
/// `PaneDivider`): the line, a hit area 7 points across it, recorded as
/// the pane's size (`slider`, "<pane> Size", "<n> points"). While a drag
/// runs only the line moves, two points thick, and the window's pointer
/// events follow it.
pub(crate) fn dividers(
    f: &Frames,
    area: (f64, f64),
    state: &HostState,
    drag: Option<PaneDrag>,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> Vec<AnyElement> {
    let mut out = vec![];
    for side in PaneSide::ALL {
        let Some(r) = line(f, side, area.0, area.1) else {
            continue;
        };
        let dragging = drag.filter(|d| d.side == side);
        let (dx, dy) = dragging.map_or((0., 0.), |d| d.offset());
        let thick = if dragging.is_some() { 2. } else { 1. };
        let across = side == PaneSide::Bottom;
        let (line_w, line_h) = if across { (r.w, thick) } else { (thick, r.h) };
        let (hit_w, hit_h) = if across { (r.w, GRIP) } else { (GRIP, r.h) };
        let hit = Rect::new(
            r.x + r.w / 2. - hit_w / 2. + dx,
            r.y + r.h / 2. - hit_h / 2. + dy,
            hit_w,
            hit_h,
        );
        let label = format!("{} Size", side_title(side));
        let value = format!("{} points", state.setup.pane(side).size as i64);
        let color = if dragging.is_some() {
            colors::accent()
        } else {
            colors::border()
        };
        let mut grip = place(hit)
            .id(("pane-divider", side as usize))
            // The grip overlaps its neighbours by 3 points: a press there
            // is the divider's alone.
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .when(across, |d| d.cursor_row_resize())
            .when(!across, |d| d.cursor_col_resize())
            .child(
                div()
                    .w(px(line_w as f32))
                    .h(px(line_h as f32))
                    .flex_none()
                    .bg(rgb(color)),
            )
            .child(scope.control_with("slider", &label, true, Some(&value), false))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.pane_drag_begin(side, e.position, window, cx)
                }),
            );
        // Registered on every paint and checked when the event comes:
        // GPUI dispatches mouse events against the last frame drawn, so a
        // release before the frame after the press must still find them.
        let weak = cx.entity().downgrade();
        grip = grip.child(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let moved = weak.clone();
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            moved
                                .update(cx, |this, cx| {
                                    if !this.pane_drag.is_some_and(|d| d.side == side) {
                                        return;
                                    }
                                    if e.pressed_button == Some(MouseButton::Left) {
                                        this.pane_drag_to(e.position, cx)
                                    } else {
                                        // The release went unseen (outside
                                        // the window, or while the pane
                                        // was not drawn): the drag ends,
                                        // setting nothing.
                                        this.cancel_pane_drag(cx)
                                    }
                                })
                                .ok();
                        }
                    });
                    let released = weak.clone();
                    window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                            released
                                .update(cx, |this, cx| {
                                    if this.pane_drag.is_some_and(|d| d.side == side) {
                                        this.pane_drag_end(cx)
                                    }
                                })
                                .ok();
                        }
                    });
                },
            )
            .absolute()
            .size_full(),
        );
        out.push(grip.into_any_element());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shrunk_pane_drags_from_its_shown_size() {
        // 600 wide: the right pane shows at 180 and the left at 178.
        let mut setup = ViewLayouts::default().arrange;
        setup.right.shown = true;
        let f = frames(600., 500., &setup);
        assert_eq!(f.size(PaneSide::Left), Some(178.));
        let mut d = PaneDrag::begin(
            PaneSide::Left,
            point(px(178.), px(10.)),
            &setup,
            (600., 500.),
        )
        .unwrap();
        assert_eq!(d.result(), None);
        d.moved(point(px(170.), px(10.)));
        assert_eq!(d.result(), Some(170.));
        // A move back to the start sets nothing.
        d.moved(point(px(178.), px(10.)));
        assert_eq!(d.result(), None);
    }

    #[test]
    fn the_bottom_line_spans_the_right_pane() {
        let mut setup = ViewLayouts::default().arrange;
        setup.right.shown = true;
        let f = frames(1000., 560., &setup);
        assert_eq!(
            line(&f, PaneSide::Bottom, 1000., 560.),
            Some(Rect::new(221., 299., 779., 1.))
        );
        assert_eq!(
            line(&f, PaneSide::Right, 1000., 560.),
            Some(Rect::new(739., 300., 1., 260.))
        );
    }
}
