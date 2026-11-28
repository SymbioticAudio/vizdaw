//! The Drum Grid's row headers (Swift `DrumRowHeaderView`,
//! `E/DrumRowHeaderView.swift`), 150 points wide beside the canvas,
//! sharing its vertical scroll. A track's header band shows its name and
//! destination; a row band its name, the destination its notes play on
//! (dimmed when it is the track's), and its note at the right.
//!
//! A press on a row selects it and takes the keyboard; a double-click
//! renames it in a field over its name (Return or the field losing the
//! keyboard commits, Escape cancels; Plan 10d Ruling 9). A drag past the
//! dead zone, then the release, drops the row where it lands: reordered,
//! moved to another drum track, or split into a track of its own (the
//! core decides). Escape during a drag ends it and the release drops
//! nothing; Escape without one cancels the grid's stroke. A secondary
//! click selects the row and opens its menu: Rename…, Show Velocity,
//! Split Row to Own Track and Remove Row.
//!
//! A drop and the menu act on the row pressed, by track and row ID, never
//! on the row now at its index (Review Focus 2). In the tree the headers
//! are the `group` "Drum Rows" with a `button` per visible row, labelled
//! with [`row_label`] and selected when it is the selected row; a press
//! on one selects it.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, AppContext as _, Bounds, Context, DispatchPhase, Entity, FocusHandle,
    Focusable as _, FontWeight, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point,
    StatefulInteractiveElement, Styled, Subscription, Window, anchored, canvas, deferred, div,
    fill, point, px, rgb,
};
use vizdaw_app::arrangement::{DEAD_ZONE, RULER_HEIGHT};
use vizdaw_app::drum_grid::{DrumRowKey, caption, header_text, row_label};
use vizdaw_app::list::PitchName;
use vizdaw_ffi::drum::{DrumBandKindRow, DrumLaneRow, DrumSpotRow};

use super::{DrumGridState, DrumGridView, HEADER_WIDTH, ROWS_GROUP};
use crate::arrangement::{colors, menu_list, menu_separator, with_alpha};
use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::text_input::{TextInput, TextInputEvent};
use crate::transport_bar::colors as bar_colors;

/// The row menu (`E/DrumRowHeaderView.swift:222-236`).
pub const RENAME: &str = "Rename\u{2026}";
pub const SHOW_VELOCITY: &str = "Show Velocity";
pub const SPLIT_ROW: &str = "Split Row to Own Track";
pub const REMOVE_ROW: &str = "Remove Row";
/// The text-editing owner of the rename field
/// (`E/DrumRowHeaderView.swift:255`).
const RENAME_OWNER: &str = "drumRow";

/// The selected row's tint (`E/DrumRowHeaderView.swift:94`).
const SELECTED_ALPHA: f32 = 0.2;
/// A track header's text inset (`:89`).
const HEADER_INSET: (f32, f32) = (6., 3.);
/// A row's caption: x, y below the band's top, and the width it leaves
/// at the right (`:116`).
const CAPTION: (f32, f32, f32) = (10., 3., 50.);
/// The note: this far in from the right, this wide (`:117-118`).
const NOTE: (f32, f32) = (38., 34.);
/// A text line's height (`:116`).
const LINE: f32 = 16.;
/// The rename field: x, y below the band's top, the width it leaves at
/// the right, and its height (`:250`).
const FIELD: (f32, f32, f32, f32) = (8., 1., 50., 20.);
/// The drop line's thickness (`:131`).
const DROP_LINE: f32 = 2.;
/// The note's digits font.
#[cfg(target_os = "macos")]
const DIGITS: &str = "Menlo";
#[cfg(not(target_os = "macos"))]
const DIGITS: &str = "DejaVu Sans Mono";

/// A row header pressed: the row (by ID, and its indexes at the press),
/// where it went down (header y) and where the pointer is.
#[derive(Debug, Clone, Copy)]
pub(super) struct HeaderDrag {
    key: DrumRowKey,
    start: f32,
    y: f32,
    moved: bool,
}

/// The open row menu: its row and where it opened.
#[derive(Debug, Clone, Copy)]
pub(super) struct RowMenu {
    key: DrumRowKey,
    shown: bool,
    at: Point<Pixels>,
}

/// The open rename field.
pub(super) struct DrumRename {
    key: DrumRowKey,
    field: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

/// The headers' part of the view.
pub(super) struct Headers {
    pub(super) focus: FocusHandle,
    /// The headers' bounds at the last draw.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(super) drag: Option<HeaderDrag>,
    pub(super) menu: Option<RowMenu>,
    pub(super) rename: Option<DrumRename>,
}

impl Headers {
    pub(super) fn new(cx: &mut Context<DocumentWindow>) -> Self {
        Headers {
            focus: cx.focus_handle(),
            bounds: Rc::default(),
            drag: None,
            menu: None,
            rename: None,
        }
    }

    /// Header y (from the headers' top) of window point `p`.
    fn local_y(&self, p: Point<Pixels>) -> f32 {
        f32::from(p.y - self.bounds.get().origin.y)
    }
}

/// The indexes of row `key` in `lanes`, while it exists.
fn indexes(lanes: &[DrumLaneRow], key: DrumRowKey) -> Option<(usize, usize)> {
    let l = lanes.iter().position(|l| l.lane == key.lane)?;
    let r = lanes[l].rows.iter().position(|r| r.id == key.row)?;
    Some((l, r))
}

/// The headers: the bands in view, the drop line, the rename field and
/// the row menu; the tree recorded. `scope` is the headers' container.
pub(super) fn row_headers(
    view: &DrumGridView,
    state: &DrumGridState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let h = &view.headers;
    let height = f32::from(h.bounds.get().size.height);
    let body = (height - RULER_HEIGHT).max(0.);
    let mut bands = div()
        .absolute()
        .top(px(RULER_HEIGHT))
        .left_0()
        .w(px(HEADER_WIDTH))
        .bottom_0()
        .overflow_hidden();
    let w = HEADER_WIDTH;
    let mut field_top = None;
    for (n, b) in state.layout.bands.iter().enumerate() {
        let top = b.top as f32 - view.offset;
        let bh = b.height as f32;
        // Before the first draw every band is laid out.
        if height > 0. && (top + bh <= 0. || top >= body) {
            continue;
        }
        let band = div()
            .id(("drum-row-header", n))
            .absolute()
            .left_0()
            .top(px(top))
            .w(px(w))
            .h(px(bh));
        let separator = div()
            .absolute()
            .left_0()
            .bottom_0()
            .w(px(w))
            .h(px(1.))
            .bg(rgb(colors::separator()));
        match b.kind {
            DrumBandKindRow::Header { lane } => {
                let Some(l) = state.lanes.get(lane as usize) else {
                    continue;
                };
                bands = bands.child(
                    band.child(
                        div()
                            .absolute()
                            .left(px(HEADER_INSET.0))
                            .top(px(HEADER_INSET.1))
                            .w(px(w - 2. * HEADER_INSET.0))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors::label()))
                            .child(header_text(l)),
                    )
                    .child(separator),
                );
            }
            DrumBandKindRow::Row { lane, row } => {
                let Some(l) = state.lanes.get(lane as usize) else {
                    continue;
                };
                let Some(r) = l.rows.get(row as usize) else {
                    continue;
                };
                let key = DrumRowKey {
                    lane: l.lane,
                    row: r.id,
                };
                let selected = state.selected_row == Some(key);
                if h.rename.as_ref().is_some_and(|x| x.key == key) {
                    field_top = Some(top);
                }
                let (name, destination, inherited) = caption(r);
                let mut text = div()
                    .absolute()
                    .left(px(CAPTION.0))
                    .top(px(CAPTION.1))
                    .w(px(w - CAPTION.2))
                    .h(px(LINE))
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_xs()
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(colors::label()))
                            .child(name),
                    );
                if let Some(d) = destination {
                    let c = if inherited {
                        bar_colors::disabled()
                    } else {
                        bar_colors::dim()
                    };
                    text = text.child(
                        div()
                            .flex_none()
                            .text_color(rgb(c))
                            .child(format!(" \u{b7} {d}")),
                    );
                }
                let note = div()
                    .absolute()
                    .left(px(w - NOTE.0))
                    .top(px(CAPTION.1))
                    .w(px(NOTE.1))
                    .h(px(LINE))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_xs()
                    .font_family(DIGITS)
                    .text_color(rgb(bar_colors::dim()))
                    .child(PitchName::name(r.note));
                let mut band = band;
                if selected {
                    band = band.bg(with_alpha(colors::selection(), SELECTED_ALPHA));
                }
                bands = bands.child(band.child(text).child(note).child(separator).child(
                    scope.control_selected("button", &row_label(l, r), true, None, selected),
                ));
            }
            DrumBandKindRow::NewLane => {}
        }
    }

    let weak = cx.entity().downgrade();
    let (relayout, moved, released) = (weak.clone(), weak.clone(), weak);
    let layout = h.bounds.clone();
    let line = h.drag.filter(|d| d.moved).map(|d| d.y);
    let painter = canvas(
        move |bounds, _, cx| {
            if layout.get().size != bounds.size {
                let weak = relayout.clone();
                cx.defer(move |cx| {
                    weak.update(cx, |_, cx| cx.notify()).ok();
                });
            }
            layout.set(bounds);
        },
        move |bounds, _, window, _| {
            if let Some(y) = line {
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.origin.x, bounds.origin.y + px(y - DROP_LINE / 2.)),
                        gpui::size(bounds.size.width, px(DROP_LINE)),
                    ),
                    rgb(colors::region_selected()),
                ));
            }
            // A drag follows the pointer anywhere in the window.
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && e.pressed_button == Some(MouseButton::Left) {
                    moved
                        .update(cx, |this, cx| this.header_drag_to(e.position, cx))
                        .ok();
                }
            });
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                    released
                        .update(cx, |this, cx| this.header_release(e.position, cx))
                        .ok();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();

    let mut el = div()
        .id("drum-rows")
        .track_focus(&h.focus)
        .relative()
        .flex_none()
        .w(px(HEADER_WIDTH))
        .h_full()
        .overflow_hidden()
        .bg(rgb(colors::ruler()))
        .border_r_1()
        .border_color(rgb(colors::separator()))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.header_mouse_down(e, window, cx)
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.header_menu_press(e, window, cx)
            }),
        )
        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
            if e.keystroke.key == "escape" && this.header_escape(cx) {
                cx.stop_propagation();
            }
        }))
        // The wheel over the headers scrolls the canvas (`:280-282`).
        .on_scroll(cx.listener(|this, e, _, cx| this.drum_wheel(e, cx)))
        .child(bands)
        .child(painter)
        // The ruler's height stays clear, as over the canvas.
        .child(
            div()
                .id("drum-rows-top")
                .absolute()
                .top_0()
                .left_0()
                .w(px(HEADER_WIDTH))
                .h(px(RULER_HEIGHT))
                .bg(rgb(colors::ruler())),
        )
        .child(scope.this("group", ROWS_GROUP, true))
        .child(scope.clip());
    if let (Some(r), Some(top)) = (&h.rename, field_top) {
        el = el.child(
            div()
                .id("drum-row-rename")
                .absolute()
                .left(px(FIELD.0))
                .top(px(RULER_HEIGHT + top + FIELD.1))
                .w(px(HEADER_WIDTH - FIELD.2))
                .h(px(FIELD.3))
                .overflow_hidden()
                .text_color(rgb(bar_colors::field_text()))
                // A press in the field is the field's, not a row's.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(r.field.clone()),
        );
    }
    if let Some(m) = h.menu {
        el = el.children(row_menu(m, &mut scope.container(), window, cx));
    }
    el.into_any_element()
}

/// The row menu at its press (`E/DrumRowHeaderView.swift:222-236`).
fn row_menu(
    m: RowMenu,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> Vec<AnyElement> {
    let size = window.viewport_size();
    let backdrop = deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("drum-row-menu-backdrop")
                .w(size.width)
                .h(size.height)
                .occlude()
                .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                    this.drum_grid.headers.menu = None;
                    cx.notify();
                })),
        ),
    )
    .with_priority(1)
    .into_any_element();
    let mut list = menu_list("drum-row-menu");
    let items = [
        Some((RENAME, false)),
        Some((SHOW_VELOCITY, m.shown)),
        None,
        Some((SPLIT_ROW, false)),
        Some((REMOVE_ROW, false)),
    ];
    for (n, item) in items.into_iter().enumerate() {
        list = list.child(match item {
            None => menu_separator(),
            Some((title, checked)) => {
                let mark = if checked { "\u{2713} " } else { "    " };
                div()
                    .id(("drum-row-menu-item", n))
                    .relative()
                    .px_3()
                    .py_1()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(bar_colors::button())))
                    .child(format!("{mark}{title}"))
                    .child(scope.control_selected("menuitem", title, true, None, checked))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.row_menu_choose(title, window, cx)
                    }))
                    .into_any_element()
            }
        });
    }
    let list = deferred(anchored().position(m.at).snap_to_window().child(list))
        .with_priority(2)
        .into_any_element();
    vec![backdrop, list]
}

impl DocumentWindow {
    /// The row being renamed.
    pub fn drum_renaming(&self) -> Option<DrumRowKey> {
        self.drum_grid.headers.rename.as_ref().map(|r| r.key)
    }

    /// The row under window point `at` of the headers, by indexes and ID.
    fn header_row(&self, at: Point<Pixels>, cx: &Context<Self>) -> Option<(u32, u32, DrumRowKey)> {
        let g = &self.drum_grid;
        let local = g.headers.local_y(at);
        // The ruler's height is no row's.
        if local < RULER_HEIGHT {
            return None;
        }
        let y = f64::from(local - RULER_HEIGHT + g.offset);
        self.read_model(cx, |d| {
            let DrumSpotRow::Row { lane, row, .. } = d.drum_spot(y) else {
                return None;
            };
            let lanes = d.drum_lanes();
            let l = lanes.get(lane as usize)?;
            let r = l.rows.get(row as usize)?;
            Some((
                lane,
                row,
                DrumRowKey {
                    lane: l.lane,
                    row: r.id,
                },
            ))
        })
        .flatten()
    }

    /// A press in the headers (`E/DrumRowHeaderView.swift:154-169`): a
    /// stroke whose release was lost goes first; on a row it selects it
    /// and takes the keyboard; a double-click renames it.
    fn header_mouse_down(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.end_drum_stroke(cx);
        self.drum_grid.headers.menu = None;
        self.drum_grid.headers.drag = None;
        window.focus(&self.drum_grid.headers.focus, cx);
        // The keyboard is placed here (or in the rename field).
        window.prevent_default();
        let Some((lane, row, key)) = self.header_row(e.position, cx) else {
            cx.notify();
            return;
        };
        self.with_model(cx, |d| d.drum_select(lane as usize, row as usize));
        if e.click_count >= 2 {
            self.begin_drum_rename(key, window, cx);
        } else {
            let y = self.drum_grid.headers.local_y(e.position);
            self.drum_grid.headers.drag = Some(HeaderDrag {
                key,
                start: y,
                y,
                moved: false,
            });
        }
        cx.notify();
    }

    /// The pointer moved with the button held: a drag past the dead zone
    /// (vertically) shows its drop line.
    fn header_drag_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let h = &mut self.drum_grid.headers;
        let Some(mut d) = h.drag else {
            return;
        };
        let y = h.local_y(at);
        d.moved = d.moved || (y - d.start).abs() > DEAD_ZONE;
        d.y = y;
        h.drag = Some(d);
        cx.notify();
    }

    /// The button is up: a drag drops its row, found by its ID, at the
    /// content height of the release (`:183-192`); a press without one
    /// only selected.
    fn header_release(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(d) = self.drum_grid.headers.drag.take() else {
            return;
        };
        cx.notify();
        if !d.moved {
            return;
        }
        let g = &self.drum_grid;
        let y = f64::from(g.headers.local_y(at) - RULER_HEIGHT + g.offset);
        self.with_model(cx, |m| {
            if let Some((lane, row)) = indexes(&m.drum_lanes(), d.key) {
                m.drum_drop(lane, row, y);
            }
        });
    }

    /// Escape in the headers (`:201-212`): a drag ends and its release
    /// drops nothing; without one, the grid's stroke is cancelled.
    /// Whether there was anything to end.
    fn header_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if self.drum_grid.headers.drag.take().is_some() {
            cx.notify();
            return true;
        }
        self.cancel_drum(cx)
    }

    /// A secondary click on a row selects it and opens its menu
    /// (`:214-219`).
    fn header_menu_press(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.end_drum_stroke(cx);
        self.drum_grid.headers.menu = None;
        window.focus(&self.drum_grid.headers.focus, cx);
        window.prevent_default();
        let Some((lane, row, key)) = self.header_row(e.position, cx) else {
            cx.notify();
            return;
        };
        let shown = self
            .with_model(cx, |d| {
                d.drum_select(lane as usize, row as usize);
                d.drum_row(lane as usize, row as usize)
                    .is_some_and(|r| r.show_velocity)
            })
            .unwrap_or(false);
        self.drum_grid.headers.menu = Some(RowMenu {
            key,
            shown,
            at: e.position,
        });
        cx.notify();
    }

    /// A row menu item: it acts on the menu's row, selected again by its
    /// ID; a row gone since does nothing.
    fn row_menu_choose(&mut self, title: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(m) = self.drum_grid.headers.menu.take() else {
            return;
        };
        cx.notify();
        let found = self
            .with_model(cx, |d| {
                let (lane, row) = indexes(&d.drum_lanes(), m.key)?;
                d.drum_select(lane, row);
                Some(())
            })
            .flatten()
            .is_some();
        if !found {
            return;
        }
        match title {
            RENAME => self.begin_drum_rename(m.key, window, cx),
            SHOW_VELOCITY => {
                let show = !m.shown;
                self.with_model(cx, |d| d.drum_show_velocity(Some(show)));
            }
            SPLIT_ROW => {
                self.with_model(cx, |d| d.drum_split_selected_row());
            }
            REMOVE_ROW => {
                self.with_model(cx, |d| d.drum_remove_selected_row());
            }
            _ => {}
        }
    }

    /// Opens the rename field over row `key`'s name, holding it, all
    /// selected (`:239-256`). A rename already open is committed first;
    /// a row not shown does nothing.
    fn begin_drum_rename(&mut self, key: DrumRowKey, window: &mut Window, cx: &mut Context<Self>) {
        self.end_drum_rename(true, true, window, cx);
        let Some(name) = self
            .read_model(cx, |d| {
                let lanes = d.drum_lanes();
                let (l, r) = indexes(&lanes, key)?;
                Some(lanes[l].rows[r].name.clone())
            })
            .flatten()
        else {
            return;
        };
        let field = cx.new(|cx| TextInput::new("Name", cx).with_text(&name));
        let focus = field.focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe_in(&field, window, |this, _, event, window, cx| match event {
                TextInputEvent::Confirm => this.end_drum_rename(true, true, window, cx),
                TextInputEvent::Cancel => this.end_drum_rename(false, true, window, cx),
                TextInputEvent::Tab | TextInputEvent::BackTab => {}
            }),
            cx.on_blur(&focus, window, |this, window, cx| {
                this.end_drum_rename(true, true, window, cx)
            }),
        ];
        window.focus(&focus, cx);
        // The bare transport keys wait while the name is typed.
        self.with_model(cx, |d| d.begin_text_editing(RENAME_OWNER));
        self.drum_grid.headers.rename = Some(DrumRename {
            key,
            field,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    /// Ends the open rename, renaming the row if `commit` (the model
    /// sends nothing for a blank or unchanged name, or a row gone). The
    /// keyboard returns to the headers (`refocus`) or the window, unless
    /// it moved elsewhere.
    pub(crate) fn end_drum_rename(
        &mut self,
        commit: bool,
        refocus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(r) = self.drum_grid.headers.rename.take() else {
            return;
        };
        let focus = r.field.focus_handle(cx);
        let name = r.field.read(cx).text().to_owned();
        self.with_model(cx, |d| d.end_text_editing(RENAME_OWNER));
        if commit {
            self.with_model(cx, |d| d.drum_rename(r.key, &name));
        }
        if window.focused(cx).is_none_or(|f| f == focus) {
            let to = if refocus {
                self.drum_grid.headers.focus.clone()
            } else {
                self.focus_handle(cx)
            };
            window.focus(&to, cx);
        }
        cx.notify();
    }
}

impl DrumGridView {
    /// The window closes mid-rename: the field goes without renaming and
    /// the transport keys come back (`E/DrumRowHeaderView.swift:270-277`).
    pub(super) fn end_rename_closing(&mut self, d: &mut vizdaw_app::DocumentModel) {
        if self.headers.rename.take().is_some() {
            d.end_text_editing(RENAME_OWNER);
        }
    }
}
