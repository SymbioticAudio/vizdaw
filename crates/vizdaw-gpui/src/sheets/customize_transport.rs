//! View › Customize Transport Bar… (Swift `TransportCustomizeSheet`): the
//! draft bar as chips in the bar's own faces, an inspector under it (a
//! readout's size, a command button's command and label) and the palette
//! of what is not on the bar. Chips drag onto the bar, along it and back
//! to the palette; a palette chip also adds by a double click or its
//! "Add to Bar" button, and the selected chip has Move Left, Move Right
//! and Remove. Done writes the bar (`Prefs::set_transport_bar`, and every
//! window draws it); Cancel and Esc write nothing; Reset to Default only
//! changes the draft. The sheet's state is [`Customizer`] (all the rules)
//! in an entity the handlers update.

use crate::appearance::ThemedCorners as _;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, Stateful, StatefulInteractiveElement, Styled, Window, div,
    px, rgb,
};
use vizdaw_app::bar_items::STALE_TOOLTIP;
use vizdaw_app::transport_bar::{Size, TransportItem};
use vizdaw_app::transport_customize::{Customizer, DragSource, LABEL_TOO_LONG};

use crate::app_state::AppState;
use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton};
use crate::text_input::TextInput;
use crate::transport_bar::{Piece, button, colors, face};

pub const TITLE: &str = "Customize Transport Bar";
pub const CAPTION: &str =
    "Drag items onto the bar, along it, or back to the palette to remove them.";
pub const BAR_SECTION: &str = "Bar";
pub const PALETTE_SECTION: &str = "Palette";
pub const BAR_GROUP: &str = "Transport Bar Items";
pub const NOTHING_SELECTED: &str = "Select an item on the bar to set its size or its command.";
pub const SEARCH: &str = "Search Commands";
pub const LABEL_FIELD: &str = "Button Label";
pub const LABEL_PLACEHOLDER: &str = "Label (the command's title when empty)";
pub const SET_LABEL: &str = "Set Label";
pub const COMMANDS_LIST: &str = "Commands";
pub const ADD: &str = "Add to Bar";
pub const REMOVE: &str = "Remove";
pub const LEFT: &str = "Move Left";
pub const RIGHT: &str = "Move Right";
pub const RESET: &str = "Reset to Default";
pub const CANCEL: &str = "Cancel";
pub const DONE: &str = "Done";
/// The card's least height (Swift's `minHeight`).
const MIN_HEIGHT: f32 = 480.;
const WIDTH: f32 = 760.;
/// A palette cell: at least 140 wide (Swift's adaptive grid).
const CELL: f32 = 170.;
const LIST_HEIGHT: f32 = 160.;

/// What a drag carries: where the chip came from.
#[derive(Clone, Copy)]
struct Dragged(DragSource);

/// The chip that follows the pointer.
struct DragChip {
    title: String,
    fill: (u32, u32),
}

impl Render for DragChip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        face("drag-chip", &self.title, true, self.fill.0)
            .px_3()
            .text_color(rgb(self.fill.1))
            .border_2()
            .border_color(rgb(colors::accent()))
    }
}

/// The sheet's state.
struct State {
    c: Customizer,
    query: Entity<TextInput>,
    label: Entity<TextInput>,
    /// What the label field was last set from: the selection and that
    /// item's label.
    synced: Option<(Option<usize>, Option<String>)>,
    /// Why Done did not write.
    problem: Option<String>,
}

/// The Customize Transport Bar sheet.
pub struct CustomizeTransportSheet {
    state: Entity<State>,
}

impl CustomizeTransportSheet {
    /// The sheet over the bar now stored.
    pub fn new(cx: &mut App) -> Self {
        let layout = AppState::global(cx).read(cx).prefs.transport_bar();
        let query = cx.new(|cx| TextInput::new(SEARCH, cx).compact());
        let label = cx.new(|cx| TextInput::new(LABEL_PLACEHOLDER, cx).compact());
        let state = cx.new(|_| State {
            c: Customizer::new(&layout),
            query,
            label,
            synced: None,
            problem: None,
        });
        CustomizeTransportSheet { state }
    }
}

/// The title of `item` on a chip (Swift `TransportCustomizeEntries.title`):
/// a command button shows its label, else its command's title, else its
/// id.
fn title(item: &TransportItem) -> String {
    match item {
        TransportItem::Command { id, label } => label
            .clone()
            .or_else(|| core_control::registry::find(id).map(|c| c.title.to_owned()))
            .unwrap_or_else(|| id.clone()),
        other => {
            let t = Piece::of(other).map_or("", Piece::title);
            match other {
                TransportItem::Bar(Size::Small)
                | TransportItem::Time(Size::Small)
                | TransportItem::Tempo(Size::Small) => format!("{t} (Small)"),
                _ => t.to_owned(),
            }
        }
    }
}

/// An item's name apart from its size or command ("Position in Bars").
fn base_title(item: &TransportItem) -> &'static str {
    Piece::of(item).map_or("", Piece::title)
}

/// A chip's fill and the text on it, in the bar's own faces: a readout
/// on the readout fill, anything else a button.
fn fill(item: &TransportItem) -> (u32, u32) {
    match item {
        TransportItem::Bar(_)
        | TransportItem::Time(_)
        | TransportItem::Tempo(_)
        | TransportItem::Signature => (colors::readout(), colors::readout_text()),
        _ => (colors::button(), colors::text()),
    }
}

fn heading(text: &str, scope: &mut RegistryScope) -> AnyElement {
    div()
        .relative()
        .text_color(rgb(colors::dim()))
        .font_weight(gpui::FontWeight::BOLD)
        .child(text.to_owned())
        .child(scope.control("text", text, true))
        .into_any_element()
}

fn caption(text: &str, colour: u32, scope: &mut RegistryScope) -> AnyElement {
    div()
        .relative()
        .text_xs()
        .text_color(rgb(colour))
        .child(text.to_owned())
        .child(scope.control("text", text, true))
        .into_any_element()
}

/// One chip: its face and, selected, the accent's border.
fn chip(
    id: impl Into<gpui::ElementId>,
    text: &str,
    colours: (u32, u32),
    selected: bool,
) -> Stateful<Div> {
    face(id, text, true, colours.0)
        .px_3()
        .text_color(rgb(colours.1))
        .whitespace_nowrap()
        .border_2()
        .border_color(rgb(if selected {
            colors::accent()
        } else {
            colors::border()
        }))
}

impl State {
    /// Runs `f` on the draft and draws the window again.
    fn edit(this: &Entity<State>, window: &mut Window, cx: &mut App, f: impl FnOnce(&mut State)) {
        this.update(cx, |s, _| {
            f(s);
            s.problem = None;
        });
        window.refresh();
    }

    /// The label field's text, trimmed, as longer than a label may be.
    fn too_long(&self, cx: &App) -> bool {
        self.label.read(cx).text().trim().chars().count() > vizdaw_app::transport_bar::MAX_LABEL
    }

    /// The selected command button takes the label field's text.
    fn commit_label(&mut self, text: &str) -> Result<(), String> {
        let Some(i) = self.c.selected() else {
            return Ok(());
        };
        if !matches!(self.c.items().get(i), Some(TransportItem::Command { .. })) {
            return Ok(());
        }
        self.c.set_label(i, text)
    }
}

/// The bar's chips.
fn bar_chips(
    state: &Entity<State>,
    items: &[TransportItem],
    selected: Option<usize>,
    scope: &mut RegistryScope,
) -> Vec<AnyElement> {
    let count = items.len();
    let mut out: Vec<AnyElement> = items
        .iter()
        .enumerate()
        .map(|(n, item)| {
            let t = title(item);
            let label = match item {
                TransportItem::Command { .. } => format!("Command Button: {t}"),
                _ => t.clone(),
            };
            let colours = fill(item);
            let is = selected == Some(n);
            let (click, drop) = (state.clone(), state.clone());
            let drag_title = t.clone();
            chip(("bar-chip", n), &t, colours, is)
                .child(scope.control_selected("chip", &label, true, None, is))
                .on_click(move |_, window, cx| {
                    State::edit(&click, window, cx, |s| {
                        s.c.select((s.c.selected() != Some(n)).then_some(n))
                    });
                })
                .on_drag(Dragged(DragSource::Bar(n)), move |_, _, _, cx| {
                    let (title, fill) = (drag_title.clone(), colours);
                    cx.new(|_| DragChip { title, fill })
                })
                .on_drop::<Dragged>(move |d, window, cx| {
                    State::edit(&drop, window, cx, |s| {
                        s.c.drop_on_bar(d.0, n);
                    });
                })
                .into_any_element()
        })
        .collect();
    // The end of the bar: a drop here appends.
    let end = state.clone();
    out.push(
        div()
            .id("bar-end")
            .flex_none()
            .w(px(40.))
            .h(px(28.))
            .on_drop::<Dragged>(move |d, window, cx| {
                State::edit(&end, window, cx, |s| {
                    s.c.drop_on_bar(d.0, count);
                });
            })
            .into_any_element(),
    );
    out
}

/// The palette's chips: double click or the "+" adds.
fn palette_chips(
    state: &Entity<State>,
    items: &[TransportItem],
    scope: &mut RegistryScope,
) -> Vec<AnyElement> {
    items
        .iter()
        .enumerate()
        .map(|(n, item)| {
            let t = if matches!(item, TransportItem::Command { .. }) {
                vizdaw_app::bar_items::COMMAND_BUTTON.to_owned()
            } else {
                title(item)
            };
            let label = format!("{t}, not on the bar");
            let colours = fill(item);
            let (double, plus) = (state.clone(), state.clone());
            let drag_title = t.clone();
            let add = move |state: &Entity<State>, window: &mut Window, cx: &mut App| {
                State::edit(state, window, cx, |s| {
                    let at = s.c.items().len();
                    s.c.drop_on_bar(DragSource::Palette(n), at);
                });
            };
            let add_again = add;
            chip(("palette-chip", n), &t, colours, false)
                .w(px(CELL))
                .min_w(px(140.))
                .justify_between()
                .child(scope.control("chip", &label, true))
                .child(
                    div()
                        .id(("palette-add", n))
                        .relative()
                        .flex_none()
                        .w(px(22.))
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .round_md()
                        .cursor_pointer()
                        .text_color(rgb(colors::accent()))
                        .child("+")
                        .child(scope.control("button", ADD, true))
                        .on_click(move |_, window, cx| {
                            add_again(&plus, window, cx);
                            cx.stop_propagation();
                        }),
                )
                .on_click(move |e, window, cx| {
                    if e.click_count() >= 2 {
                        add(&double, window, cx);
                    }
                })
                .on_drag(Dragged(DragSource::Palette(n)), move |_, _, _, cx| {
                    let (title, fill) = (drag_title.clone(), colours);
                    cx.new(|_| DragChip { title, fill })
                })
                .into_any_element()
        })
        .collect()
}

/// A button of the inspector, pressed through `act`.
fn action(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    scope: &mut RegistryScope,
    state: &Entity<State>,
    act: impl Fn(&mut State) + 'static,
) -> AnyElement {
    let state = state.clone();
    let b = button(id, label, enabled, colors::button(), scope);
    if enabled {
        b.on_click(move |_, window, cx| State::edit(&state, window, cx, |s| act(s)))
            .into_any_element()
    } else {
        b.into_any_element()
    }
}

/// The inspector: the selected item's actions and settings.
fn inspector(
    state: &Entity<State>,
    selected: Option<usize>,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let (items, query) = {
        let s = state.read(cx);
        (s.c.items().to_vec(), s.query.clone())
    };
    let Some((n, item)) = selected.and_then(|n| items.get(n).map(|i| (n, i.clone()))) else {
        return caption(NOTHING_SELECTED, colors::dim(), scope);
    };
    let mut col = div().flex().flex_col().gap_1();
    // Every chip's moves; the cluster is never removed.
    let last = items.len() - 1;
    col = col.child(
        div()
            .flex()
            .flex_row()
            .gap_1()
            .child(action("chip-left", LEFT, n > 0, scope, state, move |s| {
                s.c.move_left(n)
            }))
            .child(action(
                "chip-right",
                RIGHT,
                n < last,
                scope,
                state,
                move |s| s.c.move_right(n),
            ))
            .child(action(
                "chip-remove",
                REMOVE,
                item != TransportItem::Cluster,
                scope,
                state,
                move |s| {
                    s.c.remove(n);
                },
            )),
    );
    match &item {
        TransportItem::Bar(size) | TransportItem::Time(size) | TransportItem::Tempo(size) => {
            let label = format!("{} Size", base_title(&item));
            let mut row = div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(caption(&label, colors::text(), scope));
            for (id, name, which) in [
                ("size-small", "Small", Size::Small),
                ("size-large", "Large", Size::Large),
            ] {
                let on = *size == which;
                let st = state.clone();
                row = row.child(
                    face(
                        id,
                        name,
                        true,
                        if on { colors::on() } else { colors::button() },
                    )
                    .w(px(72.))
                    .child(scope.control_selected("button", name, true, None, on))
                    .on_click(move |_, window, cx| {
                        State::edit(&st, window, cx, |s| s.c.set_size(n, which));
                    }),
                );
            }
            col.child(row).into_any_element()
        }
        TransportItem::Command { id, .. } => col
            .child(command_editor(state, n, id, &query, scope, window, cx))
            .into_any_element(),
        other => col
            .child(caption(
                &format!("{} has no settings.", base_title(other)),
                colors::dim(),
                scope,
            ))
            .into_any_element(),
    }
}

/// A field that records itself as a text field.
fn field(
    input: &Entity<TextInput>,
    label: &str,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &App,
) -> Div {
    let text = input.read(cx).text().to_owned();
    let focused = input.focus_handle(cx).is_focused(window);
    div()
        .relative()
        .child(input.clone())
        .child(scope.control_with("textfield", label, true, Some(&text), focused))
}

fn command_editor(
    state: &Entity<State>,
    n: usize,
    current: &str,
    query: &Entity<TextInput>,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let (label_input, rows, too_long, stale) = {
        let s = state.read(cx);
        (
            s.label.clone(),
            s.c.commands(query.read(cx).text()),
            s.too_long(cx),
            s.c.stale(n),
        )
    };
    let mut col = div().flex().flex_col().gap_1();
    let search = field(query, SEARCH, scope, window, cx).w(px(260.));
    let label = field(&label_input, LABEL_FIELD, scope, window, cx).flex_1();
    let set = {
        let st = state.clone();
        let b = button("set-label", SET_LABEL, !too_long, colors::button(), scope);
        if too_long {
            b
        } else {
            b.on_click(move |_, window, cx| {
                let text = st.read(cx).label.read(cx).text().to_owned();
                State::edit(&st, window, cx, |s| {
                    let _ = s.commit_label(&text);
                });
            })
        }
    };
    col = col.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(search)
            .child(label)
            .child(set),
    );
    if too_long {
        col = col.child(caption(LABEL_TOO_LONG, colors::error(), scope));
    }
    let mut list_scope = scope.container();
    let clip = list_scope.clip();
    let list = div()
        .id("commands")
        .relative()
        .h(px(LIST_HEIGHT))
        .flex_none()
        .overflow_y_scroll()
        .bg(rgb(colors::list()))
        .round_md()
        .child(
            div()
                .flex()
                .flex_col()
                .children(rows.into_iter().enumerate().map(|(i, (id, name, menu))| {
                    let on = id == current;
                    let text = format!("{name}, {menu}");
                    let st = state.clone();
                    div()
                        .id(("command-row", i))
                        .relative()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .h(px(22.))
                        .flex_none()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors::button())))
                        .child(div().text_color(rgb(colors::text())).child(name))
                        .child(div().text_color(rgb(colors::dim())).child(menu))
                        .child(div().flex_1())
                        .when(on, |d| {
                            d.child(div().text_color(rgb(colors::accent())).child("\u{2713}"))
                        })
                        .child(list_scope.control_selected("button", &text, true, None, on))
                        .on_click(move |_, window, cx| {
                            State::edit(&st, window, cx, |s| {
                                s.c.set_command(n, id);
                            });
                        })
                })),
        )
        .child(clip)
        .child(list_scope.this("group", COMMANDS_LIST, true));
    col = col.child(list);
    if stale.is_some() {
        col = col.child(caption(
            &format!("{current}: {STALE_TOOLTIP}."),
            colors::warning(),
            scope,
        ));
    }
    col.into_any_element()
}

impl Sheet for CustomizeTransportSheet {
    fn label(&self) -> String {
        TITLE.to_owned()
    }

    fn render(
        &mut self,
        scope: &mut RegistryScope,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let state = self.state.clone();
        // The label field shows the selected command button's label; it
        // is set again only when the selection or that label changes.
        let (items, selected, palette, now, label_input, problem) = {
            let s = state.read(cx);
            let sel = s.c.selected();
            let label = match sel.and_then(|i| s.c.items().get(i)) {
                Some(TransportItem::Command { label, .. }) => label.clone(),
                _ => None,
            };
            (
                s.c.items().to_vec(),
                sel,
                s.c.palette(),
                (sel, label),
                s.label.clone(),
                s.problem.clone(),
            )
        };
        if state.read(cx).synced.as_ref() != Some(&now) {
            let text = now.1.clone().unwrap_or_default();
            label_input.update(cx, |t, cx| t.set_text(&text, cx));
            state.update(cx, |s, _| s.synced = Some(now));
        }
        let mut bar_scope = scope.container();
        let bar_chips = bar_chips(&state, &items, selected, &mut bar_scope);
        let bar_group = bar_scope.this("group", BAR_GROUP, true);
        let mut inspector_scope = scope.container();
        let inspect = inspector(&state, selected, &mut inspector_scope, window, cx);
        let mut palette_scope = scope.container();
        let palette_chips = palette_chips(&state, &palette, &mut palette_scope);
        let palette_group = palette_scope.this("group", PALETTE_SECTION, true);
        let to_palette = state.clone();
        let to_bar = state.clone();
        let mut out = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .relative()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(TITLE)
                    .child(scope.control("text", TITLE, true)),
            )
            .child(caption(CAPTION, colors::dim(), scope))
            .child(heading(BAR_SECTION, scope))
            .child(
                div()
                    .id("bar")
                    .relative()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_1()
                    .p_2()
                    .bg(rgb(colors::list()))
                    .round_md()
                    .children(bar_chips)
                    .child(bar_group)
                    .on_drop::<Dragged>(move |d, window, cx| {
                        State::edit(&to_bar, window, cx, |s| {
                            let end = s.c.items().len();
                            s.c.drop_on_bar(d.0, end);
                        });
                    }),
            )
            .child(inspect)
            .child(heading(PALETTE_SECTION, scope))
            .child(
                div()
                    .id("palette")
                    .relative()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_1()
                    .p_2()
                    .bg(rgb(colors::list()))
                    .round_md()
                    .children(palette_chips)
                    .child(palette_group)
                    .on_drop::<Dragged>(move |d, window, cx| {
                        State::edit(&to_palette, window, cx, |s| {
                            s.c.drop_on_palette(d.0);
                        });
                    }),
            );
        if let Some(p) = problem {
            out = out.child(
                div()
                    .relative()
                    .text_color(rgb(colors::error()))
                    .child(p.clone())
                    .child(scope.control_with("text", &p, true, Some("problem"), false)),
            );
        }
        out.into_any_element()
    }

    fn buttons(&self, _: &App) -> Vec<SheetButton> {
        vec![
            SheetButton::new(RESET, ButtonRole::Plain),
            SheetButton::new(CANCEL, ButtonRole::Cancel),
            SheetButton::new(DONE, ButtonRole::Default),
        ]
    }

    fn focus_order(&self, cx: &App) -> Vec<FocusHandle> {
        let s = self.state.read(cx);
        match s.c.selected().and_then(|i| s.c.items().get(i)) {
            Some(TransportItem::Command { .. }) => {
                vec![s.query.focus_handle(cx), s.label.focus_handle(cx)]
            }
            _ => vec![],
        }
    }

    fn press(&mut self, index: usize, window: &mut Window, cx: &mut App) -> bool {
        match index {
            0 => {
                State::edit(&self.state, window, cx, |s| s.c.reset());
                false
            }
            1 => true,
            _ => self.done(window, cx),
        }
    }

    fn width(&self) -> f32 {
        WIDTH
    }

    fn min_height(&self) -> f32 {
        MIN_HEIGHT
    }
}

impl CustomizeTransportSheet {
    /// Done: the label being typed is set, then the bar is written and
    /// every window draws it. A label that is not one, or a failed write,
    /// leaves the sheet up with the reason.
    fn done(&mut self, window: &mut Window, cx: &mut App) -> bool {
        let state = self.state.clone();
        let text = state.read(cx).label.read(cx).text().to_owned();
        let layout = state.update(cx, |s, _| {
            if let Err(e) = s.commit_label(&text) {
                s.problem = Some(e);
                return None;
            }
            match s.c.finish() {
                Ok(l) => Some(l),
                Err(e) => {
                    s.problem = Some(e);
                    None
                }
            }
        });
        let Some(layout) = layout else {
            window.refresh();
            return false;
        };
        let wrote = AppState::global(cx).update(cx, |a, cx| {
            let r = a.prefs.set_transport_bar(&layout);
            // Every window draws its bar from the preferences.
            cx.notify();
            r
        });
        match wrote {
            Ok(()) => true,
            Err(e) => {
                state.update(cx, |s, _| {
                    s.problem = Some(format!("Could not save the bar: {e}"))
                });
                window.refresh();
                false
            }
        }
    }
}

/// View › Customize Transport Bar…: the sheet on `view`'s window, a
/// document's or the mixer's (none while another sheet is up, or under
/// the control channel).
pub fn show(
    view: &mut crate::shell::DocumentWindow,
    window: &mut Window,
    cx: &mut Context<crate::shell::DocumentWindow>,
) {
    if crate::control::channel_on(cx) || view.sheet_label().is_some() {
        return;
    }
    let sheet = CustomizeTransportSheet::new(cx);
    let _ = view.show_sheet(Box::new(sheet), window, cx);
}
