//! The Drum Grid (Swift `DrumGridEditor`, `DrumGridBar` and
//! `DrumGridEmptyState`, `S/DrumGridPane.swift`; `E/DrumGridView.swift`):
//! the bottom pane's. A bar (Paint/Erase, View, Step, Spacing, Length,
//! Vel, Add Row, and the current track's menu with its play quantize and
//! nudge), then the row headers (a `group` "Drum Rows", 150 points wide)
//! beside the canvas (a `group` "Drum Grid"). A sequence with no drum
//! track shows "Choose a Track" with a button per MIDI or Instrument
//! track instead (or "No MIDI or Instrument Tracks").
//!
//! The view holds layout only. What the grid shows and every change is
//! the model's (`vizdaw_app::drum_grid`): the bar's settings go through
//! `update_drum_settings`, the tool through `set_tool`, the rows and
//! tracks through `drum_add_row`, `drum_set_quantize`, `drum_nudge` and
//! `drum_open_lane`.

mod canvas;
mod headers;

pub use headers::{REMOVE_ROW, RENAME, SHOW_VELOCITY, SPLIT_ROW};

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    AnyElement, Bounds, Context, FocusHandle, Focusable as _, InteractiveElement, IntoElement,
    ParentElement, Pixels, StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{MeterBars, TimeAxis};
use vizdaw_app::drum_grid::{
    CHOOSE_TEXT, CHOOSE_TITLE, DrumGridSettings, DrumRowKey, DrumView, NO_TRACKS_TEXT,
    NO_TRACKS_TITLE, drum_quantize_choices, drum_quantize_title, lengths_including, nudge_title,
    stepped_velocity,
};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, Tool};
use vizdaw_app::transport::TransportModel;
use vizdaw_ffi::drum::{DrumGridLayoutRow, DrumLaneRow};
use vizdaw_ffi::events::MeterEntryRow;
use vizdaw_ffi::selection::TickRange;

use crate::appearance::ThemedCorners as _;
use crate::arrangement::{bar_button, colors};
use crate::detail_pane;
use crate::list_editor::{Entry, bar_menu};
use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::strip_chart::picker;
use crate::transport_bar::{self, colors as bar_colors};

/// The canvas in the tree (Swift `DrumGridView.accessibilityName`,
/// `E/DrumGridView.swift:74`).
pub const GROUP: &str = "Drum Grid";
/// The row headers in the tree (`E/DrumRowHeaderView.swift:300`).
pub const ROWS_GROUP: &str = "Drum Rows";
/// The bar's controls (`S/DrumGridPane.swift:95-136`).
pub const TOOL_PAINT: &str = "Paint";
pub const TOOL_ERASE: &str = "Erase";
pub const VIEW: &str = "View";
pub const STEP: &str = "Step";
pub const SPACING: &str = "Spacing";
pub const LENGTH: &str = "Length";
/// `S/DrumGridPane.swift:123-128`.
pub const ADD_ROW: &str = "Add Row";
/// `S/DrumGridPane.swift:126`.
pub const ADD_ROW_HELP: &str = "Add Drum Row";
/// `S/DrumGridPane.swift:159`.
pub const TRACK: &str = "Track";
/// The velocity stepper's buttons.
pub const DECREASE_VELOCITY: &str = "Decrease Velocity";
pub const INCREASE_VELOCITY: &str = "Increase Velocity";
/// `S/DrumGridPane.swift:120`, reworded (Plan 10d Ruling 16).
pub const VELOCITY_HELP: &str = "The velocity of painted hits in rows whose velocity is hidden";
/// `S/DrumGridPane.swift:106-108`.
pub const STEP_HELP: &str = "Step: the width of a step matrix cell";
pub const SPACING_HELP: &str = "Spacing: the distance between the hits a drag paints";
pub const LENGTH_HELP: &str = "Length: the length of a painted hit";
/// `S/DrumGridPane.swift:158`.
pub const TRACK_HELP: &str =
    "The track's play quantize (grid or swing) and nudge, applied as it plays";
/// The track menu's first section (`S/DrumGridPane.swift:143`).
pub const PLAY_QUANTIZE: &str = "Play Quantize";
/// `S/DrumGridPane.swift:152-153`.
pub const NUDGE_EARLIER: &str = "Nudge Earlier";
pub const NUDGE_LATER: &str = "Nudge Later";

/// The row headers' width (`E/DrumRowHeaderView.swift:21`).
pub const HEADER_WIDTH: f32 = 150.;
/// The widths in the bar (`S/DrumGridPane.swift:99-157`).
const TOOL_WIDTH: f32 = 100.;
const VIEW_WIDTH: f32 = 100.;
const LENGTH_WIDTH: f32 = 70.;
const TRACK_WIDTH: f32 = 160.;
/// Where a block target scrolls to: this many points before its start
/// (`E/DrumGridView.swift:111`).
const LEAD_POINTS: f64 = 16.;

/// Which bar menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrumMenu {
    View,
    Step,
    Spacing,
    Length,
    Track,
}

impl DrumMenu {
    /// The bar pickers' element ids.
    fn index(self) -> usize {
        self as usize
    }
}

/// The pane's grid view state, owned by the document window. None of it
/// is saved; the zoom it shows is the layout's.
pub struct DrumGridView {
    focus: FocusHandle,
    axis: TimeAxis,
    /// The bands' vertical scroll.
    offset: f32,
    /// The canvas's bounds at the last draw (zero before the first).
    layout: Rc<Cell<Bounds<Pixels>>>,
    /// The grid showed at the last draw.
    mounted: bool,
    /// The target the view last scrolled to.
    shown_target: Option<EditorTarget>,
    pub(crate) menu: Option<DrumMenu>,
    /// The grid left its pane since the last draw: the window drops its
    /// stroke before the view starts afresh.
    detach_pending: bool,
    /// The press on a row, and the ruler's.
    press: Option<canvas::Press>,
    ruler: Option<canvas::RulerDrag>,
    /// The bands' height at the last prepare (the wheel's vertical
    /// clamp).
    content_height: f32,
    /// The row headers: their keyboard, drag, menu and rename field.
    headers: headers::Headers,
}

impl DrumGridView {
    pub fn new(cx: &mut Context<DocumentWindow>) -> Self {
        DrumGridView {
            focus: cx.focus_handle(),
            axis: Self::fresh_axis(),
            offset: 0.,
            layout: Rc::default(),
            mounted: false,
            shown_target: None,
            menu: None,
            detach_pending: false,
            press: None,
            ruler: None,
            content_height: 0.,
            headers: headers::Headers::new(cx),
        }
    }

    fn fresh_axis() -> TimeAxis {
        TimeAxis::new(DrumGridSettings::default().ticks_per_point, 0)
    }

    /// The grid's time axis.
    pub fn axis(&self) -> &TimeAxis {
        &self.axis
    }

    /// The bands' vertical scroll.
    pub fn offset(&self) -> f32 {
        self.offset
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The canvas's bounds at the last draw.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.layout.get()
    }

    /// Closes the bar's menu; false when none was open.
    pub(crate) fn close_menu(&mut self) -> bool {
        self.menu.take().is_some() | self.headers.menu.take().is_some()
    }

    /// Before drawing: whether the grid shows (the pane's detail is the
    /// Drum Grid). It takes the saved zoom (keeping the origin) and, the
    /// first time it shows a block target, scrolls to 16 points before
    /// the block's start (`E/DrumGridView.swift:106-114`). Leaving the
    /// pane sets a detach the window acts on.
    pub(crate) fn prepare(&mut self, d: &DocumentModel) -> bool {
        let Some(target) = d
            .layout()
            .detail
            .as_ref()
            .filter(|x| x.kind == DetailEditorKind::DrumGrid)
            .map(|x| x.target)
        else {
            if self.mounted {
                self.mounted = false;
                self.detach_pending = true;
            }
            return false;
        };
        self.mounted = true;
        self.content_height = d.drum_layout().height as f32;
        self.clamp_vertical();
        let tpp = TimeAxis::new(d.drum_settings().ticks_per_point, 0).ticks_per_point();
        if tpp != self.axis.ticks_per_point() {
            self.axis = self.axis.with_zoom(tpp);
        }
        if self.shown_target != Some(target) {
            self.shown_target = Some(target);
            if let Some(r) = d.drum_focus_range() {
                let to = r.start as f64 - LEAD_POINTS * self.axis.ticks_per_point();
                self.axis = self.axis.scrolled_to(to);
            }
        }
        true
    }

    /// The grid left its pane: shown again, it starts afresh (Swift makes
    /// a new view), at the saved zoom.
    fn unmount(&mut self) {
        self.axis = Self::fresh_axis();
        self.offset = 0.;
        self.layout.set(Bounds::default());
        self.shown_target = None;
        self.menu = None;
        self.mounted = false;
        self.drop_press();
    }
}

/// The current track's menu: its id, name, play quantize title and shift.
struct TrackMenu {
    lane: u64,
    name: String,
    quantize: String,
    shift: i64,
}

/// What the grid draws of the document this frame.
pub struct DrumGridState {
    settings: DrumGridSettings,
    erasing: bool,
    lanes: Arc<[DrumLaneRow]>,
    /// The tracks the empty state offers (id, name).
    tracks: Vec<(u64, String)>,
    track: Option<TrackMenu>,
    /// The bands (the model's, once per row shape and settings).
    layout: Arc<DrumGridLayoutRow>,
    selected_row: Option<DrumRowKey>,
    /// The selected events (a hit's selected outline).
    selected: Arc<HashSet<u64>>,
    meter: Vec<MeterEntryRow>,
    bars: MeterBars,
    /// The shaded range: the selection's, else a block target's.
    range: Option<TickRange>,
    follow: bool,
    /// The playhead's tick, when the transport is on the grid's sequence.
    playhead: Option<f64>,
    playing: bool,
}

impl DrumGridState {
    /// The pane's grid on `doc`, with `transport`'s playhead at `now`;
    /// `None` when the pane's detail is not the Drum Grid.
    pub fn of(
        doc: &DocumentModel,
        transport: Option<&TransportModel>,
        now: Instant,
    ) -> Option<Self> {
        let seq = doc.drum_seq()?;
        let lanes = doc.drum_lanes();
        let tracks = if lanes.is_empty() {
            doc.drum_tracks()
                .into_iter()
                .map(|l| (l.id, l.name))
                .collect()
        } else {
            vec![]
        };
        let track = doc.drum_current_lane().map(|l| TrackMenu {
            lane: l.lane,
            quantize: drum_quantize_title(&l),
            shift: l.play_shift,
            name: l.name,
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
        let meter = canvas::drum_meter(doc);
        Some(DrumGridState {
            settings: *doc.drum_settings(),
            erasing: doc.tool() == Tool::Eraser,
            layout: doc.drum_layout(),
            lanes,
            tracks,
            track,
            selected_row: doc.drum_selected_row(),
            selected: Arc::new(doc.selection().events.iter().copied().collect()),
            bars: MeterBars::new(&meter),
            meter,
            range: doc
                .selection()
                .time_range
                .or_else(|| doc.drum_focus_range()),
            follow: doc.layout().follow,
            playhead,
            playing,
        })
    }

    /// Playing the grid's sequence (the playhead moves every frame).
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// The playhead's tick, when the transport is on the grid's sequence.
    pub fn playhead(&self) -> Option<f64> {
        self.playhead
    }

    /// The bar's settings shown.
    pub fn settings(&self) -> &DrumGridSettings {
        &self.settings
    }

    /// The drum tracks shown.
    pub fn lanes(&self) -> &[DrumLaneRow] {
        &self.lanes
    }
}

/// The grid: its bar, a divider, then the headers and canvas, or the
/// empty state. `scope` is its own container.
pub(crate) fn render(
    view: &DrumGridView,
    state: DrumGridState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let bar = bar(view, &state, &mut scope.container(), window, cx);
    let body = if state.lanes.is_empty() {
        empty_state(&state, &mut scope.container(), cx)
    } else {
        body(view, &state, &mut scope.container(), window, cx)
    };
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

/// The row headers beside the canvas (`DrumGridContainer`,
/// `E/DrumRowHeaderView.swift:328-360`).
fn body(
    view: &DrumGridView,
    state: &DrumGridState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let headers = headers::row_headers(view, state, &mut scope.container(), window, cx);
    let canvas = canvas::grid_canvas(view, state, &mut scope.container(), cx);
    div()
        .flex()
        .flex_row()
        .flex_1()
        .min_h_0()
        .child(headers)
        .child(canvas)
        .into_any_element()
}

/// A title and its description, centred (Swift's
/// `ContentUnavailableView`).
fn unavailable(title: &str, text: &str, scope: &mut RegistryScope) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .child(
            div()
                .relative()
                .text_color(rgb(bar_colors::text()))
                .child(title.to_owned())
                .child(scope.control("text", title, true)),
        )
        .child(
            div()
                .relative()
                .text_xs()
                .text_color(rgb(bar_colors::dim()))
                .child(text.to_owned())
                .child(scope.control("text", text, true)),
        )
}

/// No drum track yet (`S/DrumGridPane.swift:27-53`): every MIDI and
/// Instrument track of the sequence can be opened as drums.
fn empty_state(
    state: &DrumGridState,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let area = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .gap_3()
        .bg(rgb(colors::canvas()));
    if state.tracks.is_empty() {
        return area
            .child(unavailable(NO_TRACKS_TITLE, NO_TRACKS_TEXT, scope))
            .into_any_element();
    }
    let mut buttons = div().flex().flex_row().flex_wrap().gap_2();
    for (n, (lane, name)) in state.tracks.iter().enumerate() {
        let lane = *lane;
        buttons = buttons.child(
            div()
                .id(("drum-choose-track", n))
                .relative()
                .h(px(24.))
                .px(px(10.))
                .flex()
                .items_center()
                .round_md()
                .cursor_pointer()
                .text_xs()
                .bg(rgb(bar_colors::button()))
                .text_color(rgb(bar_colors::text()))
                .child(name.clone())
                .child(scope.control("button", name, true))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.drum_grid.close_menu();
                    this.with_model(cx, |d| d.drum_open_lane(lane));
                })),
        );
    }
    area.child(unavailable(CHOOSE_TITLE, CHOOSE_TEXT, scope))
        .child(buttons)
        .into_any_element()
}

fn close_drum_menu(this: &mut DocumentWindow) {
    this.drum_grid.close_menu();
}

/// A picker's current title and menu: `list`, the current one checked; a
/// pick sets it.
fn pick<T: Copy + PartialEq + 'static>(
    list: Vec<(String, T)>,
    current: T,
    set: fn(&mut DrumGridSettings, T),
) -> (String, Vec<Entry>) {
    let title = list
        .iter()
        .find(|(_, v)| *v == current)
        .map(|(t, _)| t.clone())
        .unwrap_or_default();
    let entries = list
        .into_iter()
        .map(|(title, v)| {
            Entry::item(&title, None, v == current, move |this, cx| {
                this.drum_grid.close_menu();
                this.with_model(cx, |d| d.update_drum_settings(|s| set(s, v)));
            })
        })
        .collect();
    (title, entries)
}

/// The current track's menu (`S/DrumGridPane.swift:139-159`): its play
/// quantize choices, the current one checked, then its nudge.
fn track_entries(t: &TrackMenu) -> Vec<Entry> {
    let lane = t.lane;
    let mut entries = vec![Entry::Label(PLAY_QUANTIZE.to_owned())];
    for c in drum_quantize_choices() {
        let q = c.quantize.clone();
        entries.push(Entry::item(
            &c.title,
            None,
            c.title == t.quantize,
            move |this, cx| {
                let q = q.clone();
                this.with_model(cx, |d| d.drum_set_quantize(lane, q));
            },
        ));
    }
    entries.push(Entry::Separator);
    entries.push(Entry::Label(nudge_title(t.shift)));
    for (title, steps) in [(NUDGE_EARLIER, -1), (NUDGE_LATER, 1)] {
        entries.push(Entry::item(title, None, false, move |this, cx| {
            this.with_model(cx, |d| d.drum_nudge(lane, steps));
        }));
    }
    entries
}

/// A bar picker and, while open, its menu.
#[allow(clippy::too_many_arguments)]
fn add(
    view: &DrumGridView,
    row: gpui::Div,
    which: DrumMenu,
    label: &str,
    face: &str,
    width: f32,
    help: Option<&'static str>,
    entries: Vec<Entry>,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> gpui::Div {
    let mut p = picker(
        ("drum-picker", which.index()),
        label,
        face,
        face,
        width,
        scope,
    )
    .on_click(cx.listener(move |this, _, _, cx| this.toggle_drum_menu(which, cx)));
    if let Some(help) = help {
        p = crate::sheet_host::with_help(p, help);
    }
    if view.menu == Some(which) {
        p = p.children(bar_menu(
            "drum-grid-menu",
            entries,
            close_drum_menu,
            &mut scope.container(),
            window,
            cx,
        ));
    }
    row.child(p)
}

/// The bar (Swift `DrumGridBar`), left to right in one row.
fn bar(
    view: &DrumGridView,
    state: &DrumGridState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let s = state.settings;
    let mut row = div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .px(px(8.))
        .py(px(4.))
        .bg(rgb(bar_colors::bar()));

    // The tool: two segments, 100 in all (Ruling 6).
    let mut tool = div().flex().flex_row().flex_none().gap(px(1.));
    let segment = (TOOL_WIDTH - 1.) / 2.;
    for (n, (title, erase)) in [(TOOL_PAINT, false), (TOOL_ERASE, true)]
        .into_iter()
        .enumerate()
    {
        tool = tool.child(
            bar_button(
                ("drum-tool", n),
                title,
                title,
                segment,
                state.erasing == erase,
                None,
                scope,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.drum_grid.close_menu();
                let t = if erase { Tool::Eraser } else { Tool::Pencil };
                this.with_model(cx, |d| d.set_tool(t));
            })),
        );
    }
    row = row.child(tool);

    let views = DrumView::ALL
        .iter()
        .map(|&v| (v.title().to_owned(), v))
        .collect();
    let (t, e) = pick(views, s.view, |s, v| s.view = v);
    row = add(
        view,
        row,
        DrumMenu::View,
        VIEW,
        &t,
        VIEW_WIDTH,
        None,
        e,
        scope,
        window,
        cx,
    );
    let (t, e) = pick(lengths_including(s.step), s.step, |s, v| s.step = v);
    row = add(
        view,
        row,
        DrumMenu::Step,
        STEP,
        &t,
        LENGTH_WIDTH,
        Some(STEP_HELP),
        e,
        scope,
        window,
        cx,
    );
    let (t, e) = pick(lengths_including(s.spacing), s.spacing, |s, v| {
        s.spacing = v
    });
    row = add(
        view,
        row,
        DrumMenu::Spacing,
        SPACING,
        &t,
        LENGTH_WIDTH,
        Some(SPACING_HELP),
        e,
        scope,
        window,
        cx,
    );
    let (t, e) = pick(lengths_including(s.duration), s.duration, |s, v| {
        s.duration = v
    });
    row = add(
        view,
        row,
        DrumMenu::Length,
        LENGTH,
        &t,
        LENGTH_WIDTH,
        Some(LENGTH_HELP),
        e,
        scope,
        window,
        cx,
    );

    // The velocity stepper (Rulings 5 and 16).
    let vel = format!("Vel {}", s.velocity);
    let step = |id: &'static str, glyph: &str, label: &str, up: bool, scope: &mut _| {
        crate::sheet_host::with_help(
            transport_bar::cluster_button(id, glyph, label, true, bar_colors::button(), scope),
            VELOCITY_HELP,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.drum_grid.close_menu();
            this.with_model(cx, |d| {
                d.update_drum_settings(|s| s.velocity = stepped_velocity(s.velocity, up))
            });
        }))
    };
    let decrease = step("drum-vel-down", "\u{2212}", DECREASE_VELOCITY, false, scope);
    let increase = step("drum-vel-up", "+", INCREASE_VELOCITY, true, scope);
    row = row.child(
        div()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .relative()
                    .flex_none()
                    .text_xs()
                    .text_color(rgb(bar_colors::text()))
                    .child(vel.clone())
                    .child(scope.control("text", &vel, true)),
            )
            .child(decrease)
            .child(increase),
    );

    // Add Row.
    let add_row = crate::sheet_host::with_help(
        transport_bar::cluster_button(
            "drum-add-row",
            "+",
            ADD_ROW,
            true,
            bar_colors::button(),
            scope,
        ),
        ADD_ROW_HELP,
    )
    .on_click(cx.listener(|this, _, _, cx| {
        this.drum_grid.close_menu();
        this.with_model(cx, |d| d.drum_add_row());
    }));
    row = row.child(add_row).child(div().flex_1());

    // The current track's menu.
    if let Some(t) = &state.track {
        let face = format!("{}: {}", t.name, t.quantize);
        row = add(
            view,
            row,
            DrumMenu::Track,
            TRACK,
            &face,
            TRACK_WIDTH,
            Some(TRACK_HELP),
            track_entries(t),
            scope,
            window,
            cx,
        );
    }
    row.into_any_element()
}

impl DocumentWindow {
    /// The pane's Drum Grid.
    pub fn drum_grid(&self) -> &DrumGridView {
        &self.drum_grid
    }

    fn toggle_drum_menu(&mut self, m: DrumMenu, cx: &mut Context<Self>) {
        let g = &mut self.drum_grid;
        g.menu = if g.menu == Some(m) { None } else { Some(m) };
        cx.notify();
    }

    /// After a draw without the grid: one that left its pane drops its
    /// stroke, gives back the keyboard and starts afresh.
    pub(crate) fn drum_detach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.drum_grid.detach_pending) {
            return;
        }
        self.end_drum_stroke(cx);
        // A rename ends without renaming (Ruling 9).
        self.end_drum_rename(false, false, window, cx);
        self.drum_grid.unmount();
        self.drum_unfocus(window, cx);
    }

    /// While drawing a grid that is mounted but out of sight (the bottom
    /// pane hidden or showing another view): it keeps no press, no
    /// stroke, no menu and no keyboard.
    pub(crate) fn drum_hidden(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_drum_stroke(cx);
        self.end_drum_rename(false, false, window, cx);
        self.drum_grid.close_menu();
        self.drum_unfocus(window, cx);
    }

    /// A grid out of sight that kept the keyboard gives it back to the
    /// window.
    fn drum_unfocus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let held = |g: &DrumGridView, window: &Window| {
            g.focus.is_focused(window) || g.headers.focus.is_focused(window)
        };
        if !held(&self.drum_grid, window) {
            return;
        }
        cx.defer_in(window, move |this, window, cx| {
            if held(&this.drum_grid, window) {
                window.focus(&this.focus_handle(cx), cx);
            }
        });
    }
}
