//! The mixer (Swift `MixerView`, `MixerBar`, `ChannelStrips`,
//! `ChannelStrip`): the bar (the DSP load and Add Bus) over the strips,
//! side by side in the model's order with master after a divider. Every
//! strip shares the rows above its fader (`StripGrid`, explicit heights,
//! Decision M8), so faders, level boxes and buttons line up; the rows the
//! next slice fills (instrument, input, EQ, inserts, sends) are left blank
//! at their heights. Top to bottom: the header (colour bar, name, kind;
//! a click selects, ⌘- or ⇧-click adds or removes, a double-click
//! renames), the output menu, the pan, the fader with its scale and meter,
//! the level, M S R I and the automation mode. Master has no output, pan
//! or solo (a blank keeps the row). A strip silent because others solo
//! dims to half; the selected strip's edge is the accent.
//!
//! The strips scroll across (and down, when even the shortest fader does
//! not fit); only the strips in view are drawn and recorded.
//!
//! One [`MixerView`] per placement (the Mix view's centre, the bottom
//! pane, the Mixer window): it says it appeared when made and that it
//! went when dropped (the last placement's going releases every open
//! touch), and the window losing the keyboard cancels its drag.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    AnyElement, App, AppContext, Bounds, ClickEvent, Context, Entity, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Pixels, Rgba, ScrollDelta, ScrollEvent,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, anchored, canvas,
    deferred, div, point, px, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::mixer::layout::{StripGrid, StripLayout, StripRowKind, rows_of};
use vizdaw_app::mixer::{AutomationModes, MeterLevel, MixerModel, MixerPlacement};
use vizdaw_ffi::automation::AutomationModeRow;
use vizdaw_ffi::edit::MonitorModeRow;
use vizdaw_ffi::mixer::{StripKindRow, StripRow};

use crate::controls::{
    self, ControlBinding, KnobSize, MixerRef, NumberBinding, NumberBox, Toggle, fader,
};
use crate::registry::RegistryScope;
use crate::strip_palette::{KnobFunction, tokens};
use crate::text_input::{TextInput, TextInputEvent};

/// What a placement shows when the mixer cannot be read (Swift's
/// `ContentUnavailableView`).
pub const UNAVAILABLE: &str = "Mixer Unavailable";
pub const UNAVAILABLE_NOTE: &str = "The mixer of this document could not be read.";
/// The bar's button.
pub const ADD_BUS: &str = "Add Bus";
/// The padding around the strips (`spacing.s`).
pub const PADDING: f64 = 8.;
/// The bar's height.
pub const BAR_HEIGHT: f64 = 32.;
/// The rename field's label.
pub const NAME: &str = "Name";
/// The header's parts: the colour bar, the name and the kind, 2 apart.
const NAME_TOP: f64 = StripLayout::COLOUR_BAR + 2.;
const NAME_HEIGHT: f64 = 16.;
const KIND_TOP: f64 = NAME_TOP + NAME_HEIGHT + 2.;
const KIND_HEIGHT: f64 = 14.;
/// The panel's corner (`radius.m`).
const PANEL_RADIUS: f32 = 5.;
/// A wheel line in points.
const LINE: f32 = 20.;

/// A strip's typed box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BoxKind {
    Level,
    Pan,
}

/// A strip's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripMenu {
    Output,
    Automation,
}

/// How a strip was drawn last frame: its dimming and its edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripLook {
    pub strip: u64,
    /// Silent because other strips solo: the panel at half.
    pub dimmed: bool,
    /// Selected: the 2-point accent edge.
    pub selected: bool,
}

/// An open rename field.
struct RenameField {
    strip: u64,
    field: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

/// One placement of a document's mixer: its scroll, its typed boxes, its
/// open menu and rename field.
pub struct MixerView {
    mixer: MixerRef,
    /// The strips' scroll (x, y), in points.
    scroll: (f64, f64),
    /// How far the strips scrolled last frame at most (x, y).
    room: (f64, f64),
    /// The strips' area, as last drawn.
    area: Rc<Cell<Bounds<Pixels>>>,
    boxes: HashMap<(u64, BoxKind), Entity<NumberBox>>,
    menu: Option<(u64, StripMenu)>,
    rename: Option<RenameField>,
    /// The placement told the mixer it shows.
    appeared: bool,
    drawn: Vec<StripLook>,
    _subscriptions: Vec<Subscription>,
}

impl MixerView {
    /// The placement `mixer` names, in `window`.
    pub fn new(mixer: MixerRef, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let p = mixer.placement;
        // After this update: the mixer is made (when need be) then.
        let weak = cx.entity().downgrade();
        let shown = mixer.clone();
        cx.defer(move |cx| {
            if weak.upgrade().is_none() {
                return;
            }
            shown.with(cx, |m, _| m.appeared(p));
            weak.update(cx, |v, cx| {
                v.appeared = true;
                cx.notify();
            })
            .ok();
        });
        let gone = mixer.clone();
        cx.on_release(move |v: &mut MixerView, cx| {
            if let Some(r) = v.rename.take() {
                let owner = rename_owner(p);
                gone.app.update(cx, |s, cx| {
                    s.store
                        .with_document(gone.doc, |d, _| d.end_text_editing(&owner));
                    cx.notify();
                });
                drop(r);
            }
            if v.appeared {
                gone.with(cx, |m, d| m.disappeared(d, p, Instant::now()));
            }
        })
        .detach();
        MixerView {
            _subscriptions: vec![controls::watch_activation(mixer.clone(), window, cx)],
            mixer,
            scroll: (0., 0.),
            room: (0., 0.),
            area: Rc::default(),
            boxes: HashMap::new(),
            menu: None,
            rename: None,
            appeared: false,
            drawn: vec![],
        }
    }

    pub fn placement(&self) -> MixerPlacement {
        self.mixer.placement
    }

    /// The strips drawn last frame, left to right.
    pub fn drawn(&self) -> &[StripLook] {
        &self.drawn
    }

    /// The strips' scroll (x, y).
    pub fn scroll(&self) -> (f64, f64) {
        self.scroll
    }

    /// Scrolls the strips to (x, y) (kept inside them at the next draw).
    pub fn scroll_to(&mut self, x: f64, y: f64, cx: &mut Context<Self>) {
        self.scroll = (x.max(0.), y.max(0.));
        cx.notify();
    }

    /// The open menu: its strip and which.
    pub fn menu(&self) -> Option<(u64, StripMenu)> {
        self.menu
    }

    /// Starts the rename of `strip` in this placement: a field holding the
    /// name, focused. Return commits (blank or unchanged sends nothing),
    /// Escape cancels, the field losing the keyboard commits. A rename
    /// already open commits first.
    pub fn begin_rename(&mut self, strip: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.end_rename(true, cx);
        let p = self.mixer.placement;
        let Some(name) = self.mixer.with(cx, |m, _| {
            m.strip(strip)?;
            m.begin_rename(strip, p);
            Some(m.rename_draft.clone())
        }) else {
            return;
        };
        let Some(name) = name else { return };
        let field = cx.new(|cx| TextInput::new(NAME, cx).with_text(&name));
        let focus = field.focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe_in(
                &field,
                window,
                |this, _, e: &TextInputEvent, _, cx| match e {
                    TextInputEvent::Confirm => this.end_rename(true, cx),
                    TextInputEvent::Cancel => this.end_rename(false, cx),
                    TextInputEvent::Tab | TextInputEvent::BackTab => {}
                },
            ),
            cx.on_blur(&focus, window, |this, _, cx| this.end_rename(true, cx)),
        ];
        window.focus(&focus, cx);
        let owner = rename_owner(p);
        let doc = self.mixer.doc;
        self.mixer.app.update(cx, |s, cx| {
            s.store
                .with_document(doc, |d, _| d.begin_text_editing(&owner));
            cx.notify();
        });
        self.rename = Some(RenameField {
            strip,
            field,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    /// Ends the open rename field: the strip is renamed to its text when
    /// `commit` and the mixer still renames it here.
    pub fn end_rename(&mut self, commit: bool, cx: &mut Context<Self>) {
        let Some(r) = self.rename.take() else { return };
        let text = r.field.read(cx).text().to_owned();
        let p = self.mixer.placement;
        let (doc, owner) = (self.mixer.doc, rename_owner(p));
        self.mixer.app.update(cx, |s, cx| {
            s.store
                .with_document(doc, |d, _| d.end_text_editing(&owner));
            cx.notify();
        });
        let strip = r.strip;
        self.mixer.with(cx, |m, d| {
            if !m.is_renaming(strip, p) {
                return;
            }
            if commit {
                m.rename_draft = text;
                m.commit_rename(d);
            } else {
                m.cancel_rename();
            }
        });
        cx.notify();
    }

    fn toggle_menu(&mut self, strip: u64, which: StripMenu, cx: &mut Context<Self>) {
        self.menu = if self.menu == Some((strip, which)) {
            None
        } else {
            Some((strip, which))
        };
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }

    fn wheel(&mut self, e: &ScrollEvent, cx: &mut Context<Self>) {
        let (dx, dy) = match e.delta {
            ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(p) => (p.x * LINE, p.y * LINE),
        };
        // ⇧, or no room down: a vertical wheel scrolls across.
        let across = e.modifiers.shift || self.room.1 <= 0.;
        let (dx, dy) = if across && dx == 0. {
            (dy, 0.)
        } else {
            (dx, dy)
        };
        self.scroll = (
            (self.scroll.0 - f64::from(dx)).max(0.),
            (self.scroll.1 - f64::from(dy)).max(0.),
        );
        cx.notify();
    }

    /// The box `key`, made once and kept while its strip is there.
    fn number_box(
        &mut self,
        key: (u64, BoxKind),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<NumberBox> {
        self.boxes
            .entry(key)
            .or_insert_with(|| cx.new(|cx| NumberBox::new(window, cx)))
            .clone()
    }
}

fn rename_owner(p: MixerPlacement) -> String {
    format!("mixer rename {p:?}")
}

/// An output menu choice: its title and its bus (`None`: master).
type OutputChoice = (String, Option<u64>);

/// What one strip in view shows.
struct StripParts {
    index: usize,
    id: u64,
    name: String,
    kind_label: String,
    bar: Rgba,
    cap: Rgba,
    dimmed: bool,
    selected: bool,
    fader: ControlBinding,
    level: NumberBinding,
    pan: Option<(ControlBinding, NumberBinding)>,
    mute: Toggle,
    solo: Option<Toggle>,
    arm: Toggle,
    monitor: Toggle,
    /// The output menu's face and its choices (title, bus).
    output: Option<(String, Vec<OutputChoice>)>,
    mode: AutomationModeRow,
    meter: MeterLevel,
}

/// What the placement draws this frame.
struct Gathered {
    load: String,
    grid: StripGrid,
    columns: usize,
    divider_before: Option<usize>,
    scroll: (f64, f64),
    /// The most it scrolls (x, y).
    room: (f64, f64),
    strips: Vec<StripParts>,
    ids: Vec<u64>,
    renaming: Option<(u64, MixerPlacement)>,
}

/// The view's area: its measured size, else the window's.
fn viewport(area: Bounds<Pixels>, window: &Window) -> (f64, f64) {
    let s = if area.size.width > Pixels::ZERO {
        area.size
    } else {
        let v = window.viewport_size();
        gpui::size(v.width, v.height - px(BAR_HEIGHT as f32))
    };
    (
        f64::from(f32::from(s.width)),
        f64::from(f32::from(s.height)),
    )
}

#[allow(clippy::too_many_arguments)]
fn gather(
    m: &MixerModel,
    d: &DocumentModel,
    mixer: &MixerRef,
    view: (f64, f64),
    scroll: (f64, f64),
    sample_rate: f64,
    now: Instant,
) -> Gathered {
    let mut all: Vec<&StripRow> = m
        .columns()
        .into_iter()
        .filter_map(|c| match c {
            vizdaw_app::mixer::MixerColumn::Audio(s) => Some(s),
            // MIDI destination strips and VCAs come with slice 6b.
            _ => None,
        })
        .collect();
    let divider_before = m.master().map(|_| all.len());
    all.extend(m.master());
    let rows: Vec<BTreeMap<StripRowKind, f64>> =
        all.iter().map(|s| rows_of(s, m.can_arm(d, s))).collect();
    let grid = StripGrid::new(&rows, (view.1 - 2. * PADDING).max(0.));
    let content = (
        2. * PADDING + StripGrid::width(all.len(), divider_before),
        2. * PADDING + grid.total,
    );
    let room = ((content.0 - view.0).max(0.), (content.1 - view.1).max(0.));
    let scroll = (scroll.0.clamp(0., room.0), scroll.1.clamp(0., room.1));
    let selected = d.selected_strips();
    let t = tokens();
    let mut strips = vec![];
    for (index, s) in all.iter().enumerate() {
        let left = PADDING + StripGrid::x(index, divider_before);
        if left + StripLayout::WIDTH <= scroll.0 || left >= scroll.0 + view.0 {
            continue;
        }
        let master = s.kind == StripKindRow::Master;
        let (Some(fader), Some(level), Some(mute)) = (
            ControlBinding::fader(mixer, m, s.id),
            NumberBinding::level(mixer, m, s.id),
            Toggle::mute(m, s.id),
        ) else {
            continue;
        };
        let lane = m.input_lane(d, s);
        let armable = lane.is_some();
        let monitor = lane.map_or(MonitorModeRow::Off, |l| l.monitor);
        let output = (!master).then(|| {
            let mut choices = vec![("Master".to_owned(), None)];
            choices.extend(m.targets(s).iter().map(|b| (b.name.clone(), Some(b.id))));
            (format!("\u{2192} {}", m.output_name(s)), choices)
        });
        strips.push(StripParts {
            index,
            id: s.id,
            name: s.name.clone(),
            kind_label: s.kind_label.clone(),
            bar: t.bar(s),
            cap: t.cap(s.kind_colour),
            dimmed: s.solo_muted,
            selected: selected.contains(&s.id),
            fader,
            level,
            pan: if master {
                None
            } else {
                ControlBinding::pan(mixer, m, s.id).zip(NumberBinding::pan(mixer, m, s.id))
            },
            mute,
            solo: if master { None } else { Toggle::solo(m, s.id) },
            arm: Toggle {
                letter: "R",
                on: lane.is_some_and(|l| l.arm),
                colour: t.state_record,
                label: format!("Record arm {}", s.name),
                enabled: armable,
                touched: false,
            },
            monitor: Toggle {
                letter: "I",
                on: monitor != MonitorModeRow::Off,
                colour: if monitor == MonitorModeRow::Input {
                    t.state_warning
                } else {
                    t.state_play
                },
                label: format!("Input monitor {}", s.name),
                enabled: armable,
                touched: false,
            },
            output,
            mode: s.automation,
            meter: m.meters().level_at(s.id, now),
        });
    }
    Gathered {
        load: m.load_text(sample_rate),
        grid,
        columns: all.len(),
        divider_before,
        scroll,
        room,
        strips,
        ids: all.iter().map(|s| s.id).collect(),
        renaming: m.renaming(),
    }
}

/// The placement `view` draws: the bar, then the strips. `scope` is the
/// mixer's own container.
pub fn render(
    view: &Entity<MixerView>,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let (mixer, scroll, area, appeared) = {
        let v = view.read(cx);
        (v.mixer.clone(), v.scroll, v.area.clone(), v.appeared)
    };
    let now = Instant::now();
    let doc = mixer.doc;
    let size = viewport(area.get(), window);
    let sample_rate = mixer
        .app
        .read(cx)
        .transport(doc)
        .map_or(0., |t| t.state().sample_rate);
    let stale = mixer.read(cx, |m, d| m.needs_refresh(d)).unwrap_or(false);
    if stale {
        // Not while drawing: the next turn reads it again.
        let again = mixer.clone();
        cx.defer(move |cx| {
            again.with(cx, |m, d| m.refresh_if_needed(d));
        });
    }
    let Some(g) = mixer.read(cx, |m, d| {
        gather(m, d, &mixer, size, scroll, sample_rate, now)
    }) else {
        view.update(cx, |v, _| v.drawn.clear());
        return unavailable(appeared, scope);
    };
    let weak = view.downgrade();
    // Boxes of strips that went are dropped (an edit in one ends).
    view.update(cx, |v, cx| {
        v.scroll = g.scroll;
        v.room = g.room;
        v.boxes.retain(|(id, _), _| g.ids.contains(id));
        v.drawn = g
            .strips
            .iter()
            .map(|s| StripLook {
                strip: s.id,
                dimmed: s.dimmed,
                selected: s.selected,
            })
            .collect();
        if v.menu.is_some_and(|(id, _)| !g.ids.contains(&id)) {
            v.menu = None;
        }
        // The mixer ended the rename (a pane change committed it, the
        // strip went): the field goes.
        if let Some(r) = &v.rename
            && g.renaming != Some((r.strip, v.mixer.placement))
        {
            let w = weak.clone();
            cx.defer(move |cx| {
                w.update(cx, |v, cx| v.end_rename(false, cx)).ok();
            });
        }
    });
    let bar = bar(&g.load, &mixer, &mut scope.container());
    let mut area_scope = scope.container();
    let mut strips = vec![];
    let mut parts = g.strips.into_iter().peekable();
    for index in 0..g.columns {
        // Every column takes its place in the tree, drawn or not, so a
        // strip's controls keep their IDs as the strips scroll.
        let mut strip_scope = area_scope.container();
        let Some(s) = parts.next_if(|s| s.index == index) else {
            continue;
        };
        strips.push(strip(
            view,
            s,
            &g.grid,
            g.divider_before,
            g.scroll,
            &mut strip_scope,
            window,
            cx,
        ));
    }
    let t = tokens();
    let divider = g.divider_before.map(|d| {
        div()
            .absolute()
            .left(px((PADDING + StripGrid::divider_x(d) - g.scroll.0) as f32))
            .top(px((PADDING - g.scroll.1) as f32))
            .w(px(1.))
            .h(px(g.grid.total as f32))
            .bg(rgb(t.surface_separator))
    });
    let measured = area.clone();
    let remeasure = weak.clone();
    let measure = canvas(
        move |bounds, _, cx| {
            if measured.get().size != bounds.size {
                let w = remeasure.clone();
                cx.defer(move |cx| {
                    w.update(cx, |_, cx| cx.notify()).ok();
                });
            }
            measured.set(bounds);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let wheel = weak.clone();
    let strips_area = div()
        .id("mixer-strips")
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .child(measure)
        .children(divider)
        .children(strips)
        .child(area_scope.clip())
        .on_scroll(move |e, _, cx| {
            wheel.update(cx, |v, cx| v.wheel(e, cx)).ok();
            cx.stop_propagation();
        });
    div()
        .relative()
        .flex()
        .flex_col()
        .size_full()
        .bg(rgb(t.surface_panel))
        .text_color(rgb(t.text_primary))
        .child(bar)
        .child(div().h(px(1.)).flex_none().bg(rgb(t.surface_separator)))
        .child(strips_area)
        .into_any_element()
}

/// The note when the mixer could not be read (nothing before the
/// placement first asked).
fn unavailable(appeared: bool, scope: &mut RegistryScope) -> AnyElement {
    let t = tokens();
    let mut note = div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .bg(rgb(t.surface_panel));
    if appeared {
        note = note
            .child(
                div()
                    .relative()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text_primary))
                    .child(UNAVAILABLE)
                    .child(scope.control("text", UNAVAILABLE, true)),
            )
            .child(
                div()
                    .relative()
                    .text_xs()
                    .text_color(rgb(t.text_secondary))
                    .child(UNAVAILABLE_NOTE)
                    .child(scope.control("text", UNAVAILABLE_NOTE, true)),
            );
    }
    note.into_any_element()
}

/// The mixer's bar: the load, then Add Bus.
fn bar(load: &str, mixer: &MixerRef, scope: &mut RegistryScope) -> AnyElement {
    let t = tokens();
    let adding = mixer.clone();
    div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .justify_end()
        .gap(px(12.))
        .h(px(BAR_HEIGHT as f32))
        .px(px(8.))
        .text_xs()
        .child(
            div()
                .relative()
                .text_color(rgb(t.text_secondary))
                .whitespace_nowrap()
                .child(load.to_owned())
                .child(scope.control("text", load, true)),
        )
        .child(
            div()
                .id("mixer-add-bus")
                .relative()
                .h(px(22.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(t.radius))
                .border_1()
                .border_color(rgb(t.surface_separator))
                .bg(rgb(t.surface_inset))
                .cursor_pointer()
                .whitespace_nowrap()
                .child(ADD_BUS)
                .child(scope.control("button", ADD_BUS, true))
                .on_click(move |_, _, cx| {
                    adding.with(cx, |m, d| m.add_bus(d));
                }),
        )
        .into_any_element()
}

/// A part of a strip at (`x`, `y`) inside it, `w` × `h`.
fn at(x: f64, y: f64, w: f64, h: f64) -> gpui::Div {
    div()
        .absolute()
        .left(px(x as f32))
        .top(px(y as f32))
        .w(px(w as f32))
        .h(px(h as f32))
}

/// One strip, placed in the scrolled area.
#[allow(clippy::too_many_arguments)]
fn strip(
    view: &Entity<MixerView>,
    s: StripParts,
    g: &StripGrid,
    divider_before: Option<usize>,
    scroll: (f64, f64),
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let t = tokens();
    let mixer = view.read(cx).mixer.clone();
    let left = PADDING + StripGrid::x(s.index, divider_before) - scroll.0;
    let top = PADDING - scroll.1;
    let side = StripLayout::SIDE_PADDING;
    let inner = StripLayout::inner();
    let row = |k: StripRowKind| (g.tops.get(&k).copied(), g.rows.get(&k).copied());
    let mut panel_fill = rgb(t.surface_raised);
    if s.dimmed {
        panel_fill.a = 0.5;
    }
    let mut out = div()
        .absolute()
        .left(px(left as f32))
        .top(px(top as f32))
        .w(px(StripLayout::WIDTH as f32))
        .h(px(g.total as f32))
        .child(
            div()
                .absolute()
                .size_full()
                .rounded(px(PANEL_RADIUS))
                .bg(panel_fill),
        );
    // The header.
    if let (Some(y), Some(h)) = row(StripRowKind::Header) {
        out = out.child(header(view, &s, (side, y, inner, h), scope, window, cx));
    }
    // The output menu.
    if let (Some((face, choices)), (Some(y), Some(h))) = (&s.output, row(StripRowKind::Output)) {
        let label = format!("Output of {}", s.name);
        let open = view.read(cx).menu == Some((s.id, StripMenu::Output));
        let w = view.downgrade();
        let id = s.id;
        let face_el = controls::menu_face(&label, face, None, true, scope, move |_, cx| {
            w.update(cx, |v, cx| v.toggle_menu(id, StripMenu::Output, cx))
                .ok();
        });
        let mut holder = at(side, y, inner, h.min(controls::menu_face::HEIGHT)).child(face_el);
        if open {
            let items = choices
                .iter()
                .map(|(title, bus)| {
                    let (mx, bus, w) = (mixer.clone(), *bus, view.downgrade());
                    MenuChoice {
                        title: title.clone(),
                        selected: false,
                        pick: Box::new(move |cx: &mut App| {
                            w.update(cx, |v, cx| v.close_menu(cx)).ok();
                            mx.with(cx, |m, d| m.set_output(d, id, bus));
                        }),
                    }
                })
                .collect();
            holder = holder.children(popup(view, "mixer-output-menu", items, scope, window));
        }
        out = out.child(holder);
    }
    // The pan.
    if let (Some((knob, text)), (Some(y), Some(h))) = (&s.pan, row(StripRowKind::Pan)) {
        let nb = view.update(cx, |v, cx| v.number_box((s.id, BoxKind::Pan), window, cx));
        let knob_el = controls::knob(
            knob,
            KnobSize::Medium,
            t.knob(KnobFunction::Pan),
            0.5,
            scope,
            window,
            cx,
        );
        let box_el = controls::number_box(&nb, text.clone(), scope, cx);
        out = out.child(
            at(side, y, inner, h)
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(knob_el)
                .child(box_el),
        );
    }
    // The fader, its scale and the meter, centred.
    let throw = g.throw;
    let fader_el = controls::fader(&s.fader, throw, s.cap, scope, window, cx);
    let meter_el = controls::meter(s.meter, throw + 2. * StripLayout::METER_OVERHANG, window);
    out = out.child(
        at(side, g.fader_top, inner, throw + StripLayout::FADER_CAP)
            .flex()
            .flex_row()
            .justify_center()
            .items_start()
            .gap(px(4.))
            .child(fader_el)
            .child(
                div()
                    .pt(px((fader::CAP.1 / 2. - StripLayout::METER_OVERHANG) as f32))
                    .child(meter_el),
            ),
    );
    // The level.
    let level_box = view.update(cx, |v, cx| v.number_box((s.id, BoxKind::Level), window, cx));
    let level_el = controls::number_box(&level_box, s.level.clone(), scope, cx);
    out = out.child(
        at(side, g.level_top, inner, StripLayout::LEVEL_BOX)
            .flex()
            .justify_center()
            .child(level_el),
    );
    // M S R I; master keeps the solo's place blank.
    let strip_id = s.id;
    let mut buttons = at(side, g.buttons_top, inner, StripLayout::TOGGLE_ROW)
        .flex()
        .flex_row()
        .justify_center()
        .gap(px(4.))
        .child(controls::toggle(
            s.mute.clone(),
            scope,
            controls::toggle::toggle_mute(&mixer, strip_id),
        ));
    buttons = match &s.solo {
        Some(solo) => buttons.child(controls::toggle(
            solo.clone(),
            scope,
            controls::toggle::toggle_solo(&mixer, strip_id),
        )),
        None => buttons.child(
            div()
                .flex_none()
                .w(px(controls::toggle::SIDE as f32))
                .h(px(controls::toggle::SIDE as f32)),
        ),
    };
    let (armed, monitored) = (mixer.clone(), mixer.clone());
    buttons = buttons
        .child(controls::toggle(s.arm.clone(), scope, move |_, cx| {
            armed.with(cx, |m, d| m.toggle_arm(d, strip_id));
        }))
        .child(controls::toggle(s.monitor.clone(), scope, move |_, cx| {
            monitored.with(cx, |m, d| m.cycle_monitor(d, strip_id));
        }));
    out = out.child(buttons);
    // The automation mode.
    let mode_label = format!("Automation of {}", s.name);
    let open = view.read(cx).menu == Some((s.id, StripMenu::Automation));
    let w = view.downgrade();
    let face = controls::menu_face(
        &mode_label,
        AutomationModes::title(s.mode),
        Some(t.automation(s.mode)),
        true,
        scope,
        move |_, cx| {
            w.update(cx, |v, cx| {
                v.toggle_menu(strip_id, StripMenu::Automation, cx)
            })
            .ok();
        },
    );
    let mut mode = at(side, g.mode_top, inner, StripLayout::MODE_MENU).child(face);
    if open {
        let items = AutomationModes::ALL
            .iter()
            .map(|&m| {
                let (mx, w) = (mixer.clone(), view.downgrade());
                MenuChoice {
                    title: AutomationModes::title(m).to_owned(),
                    selected: m == s.mode,
                    pick: Box::new(move |cx: &mut App| {
                        w.update(cx, |v, cx| v.close_menu(cx)).ok();
                        mx.with(cx, |mm, d| mm.set_mode(d, strip_id, m));
                    }),
                }
            })
            .collect();
        mode = mode.children(popup(view, "mixer-mode-menu", items, scope, window));
    }
    out = out.child(mode);
    // The edge, over every row.
    let (edge, width) = if s.selected {
        (t.accent, 2.)
    } else {
        (t.surface_separator, 1.)
    };
    out.child(
        div()
            .absolute()
            .size_full()
            .rounded(px(PANEL_RADIUS))
            .border(px(width))
            .border_color(rgb(edge)),
    )
    .into_any_element()
}

/// The header: the colour bar, the name (or its rename field) and the
/// kind. A click on the name selects the strip (⌘ or ⇧ adds or removes
/// it); a double-click renames it.
fn header(
    view: &Entity<MixerView>,
    s: &StripParts,
    (x, y, w, h): (f64, f64, f64, f64),
    scope: &mut RegistryScope,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let t = tokens();
    let field = view
        .read(cx)
        .rename
        .as_ref()
        .filter(|r| r.strip == s.id)
        .map(|r| r.field.clone());
    let name: AnyElement = match field {
        Some(field) => {
            let (text, focused) = {
                let f = field.read(cx);
                (f.text().to_owned(), true)
            };
            div()
                .relative()
                .size_full()
                .child(field)
                .child(scope.control_with("textfield", NAME, true, Some(&text), focused))
                .into_any_element()
        }
        None => {
            let id = s.id;
            let weak: WeakEntity<MixerView> = view.downgrade();
            div()
                .id(gpui::ElementId::Name(
                    format!("mixer-strip-name:{id}").into(),
                ))
                .relative()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_primary))
                .child(s.name.clone())
                .child(scope.control("text", &s.name, true))
                .on_click(move |e: &ClickEvent, window, cx| {
                    let Some(v) = weak.upgrade() else { return };
                    if e.click_count() >= 2 {
                        v.update(cx, |v, cx| v.begin_rename(id, window, cx));
                        return;
                    }
                    let mods = e.modifiers();
                    let adding = mods.secondary(cx) || mods.shift;
                    let mixer = v.read(cx).mixer.clone();
                    mixer.with(cx, |m, d| m.select(d, id, adding));
                })
                .into_any_element()
        }
    };
    at(x, y, w, h)
        .child(
            at(0., 0., w, StripLayout::COLOUR_BAR)
                .rounded(px(t.radius))
                .bg(s.bar),
        )
        .child(at(0., NAME_TOP, w, NAME_HEIGHT).child(name))
        .child(
            at(0., KIND_TOP, w, KIND_HEIGHT)
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(10.))
                .text_color(rgb(t.text_secondary))
                .child(s.kind_label.clone())
                .child(scope.control("text", &s.kind_label, true)),
        )
        .into_any_element()
}

/// One choice of a strip menu.
struct MenuChoice {
    title: String,
    /// The current one (checked).
    selected: bool,
    pick: Box<dyn Fn(&mut App)>,
}

/// A strip menu open under its face: a backdrop that closes it, and the
/// list of `items`, recorded as menu items.
fn popup(
    view: &Entity<MixerView>,
    id: &'static str,
    items: Vec<MenuChoice>,
    scope: &mut RegistryScope,
    window: &Window,
) -> Vec<AnyElement> {
    let size = window.viewport_size();
    let closing = view.downgrade();
    let backdrop = deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("mixer-menu-backdrop")
                .w(size.width)
                .h(size.height)
                .occlude()
                .on_any_mouse_down(move |_, _, cx| {
                    closing.update(cx, |v, cx| v.close_menu(cx)).ok();
                }),
        ),
    )
    .with_priority(1)
    .into_any_element();
    let mut list = crate::arrangement::menu_list(id);
    for (row, item) in items.into_iter().enumerate() {
        let MenuChoice {
            title,
            selected,
            pick,
        } = item;
        list = list.child(
            div()
                .id(("mixer-menu-item", row))
                .relative()
                .px_3()
                .py_1()
                .text_xs()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(crate::transport_bar::colors::BUTTON)))
                .child(format!(
                    "{}{}",
                    if selected { "\u{2713} " } else { "" },
                    title
                ))
                .child(scope.control_selected("menuitem", &title, true, None, selected))
                .on_click(move |_, _, cx| pick(cx)),
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
