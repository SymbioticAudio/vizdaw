//! The MIDI Learn Maps sheet (Swift `S/LearnMapsSheet.swift:10-75`; plan
//! 5f-1 Task 11, Rulings 6, 7, 18), opened from "MIDI Learn Maps…" in a
//! strip name's menu: each map of the project, its source over what it
//! drives (or "(missing)"), its mode, its pick-up and Remove. Every
//! change is one undo step of the project; a refusal is a line under the
//! list. Done (Return or Esc) closes it. The state is the document's
//! [`vizdaw_app::learn::LearnModel`], which polls while the sheet is open.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, px, rgb,
};
use vizdaw_app::DocId;
use vizdaw_app::learn::{LearnModel, MODES, NO_MAPS, PICKUP_HELP, mode_title};
use vizdaw_ffi::surfaces::{LearnMapRow, LearnModeRow, LearnSourceRow};

use crate::app_state::AppState;
use crate::controls::menu_face;
use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton, sheet_is, with_help};
use crate::shell::DocumentWindow;
use crate::transport_bar::colors;

/// The sheet's title (`S/LearnMapsSheet.swift:16`).
pub const TITLE: &str = "MIDI Learn Maps";
/// The strip name menu's item (`S/ChannelStrip.swift:402`).
pub const ITEM: &str = "MIDI Learn Maps\u{2026}";
pub const DONE: &str = "Done";
pub const PICKUP: &str = "Pick-up";
pub const REMOVE: &str = "Remove";
/// `S/LearnMapsSheet.swift:36`.
const WIDTH: f32 = 640.;
/// The mode picker (`:63`).
const MODE_WIDTH: f32 = 190.;
/// The list's least height (`:27`), and the empty line's (`:20`).
const LIST_HEIGHT: f32 = 160.;
const EMPTY_HEIGHT: f32 = 60.;
/// The list scrolls past this.
const LIST_MAX: f32 = 360.;

/// The MIDI Learn Maps sheet of document `doc`.
pub struct LearnMapsSheet {
    doc: DocId,
    /// The source whose mode picker is open.
    open: Entity<Option<LearnSourceRow>>,
}

impl LearnMapsSheet {
    pub fn new(doc: DocId, cx: &mut App) -> Self {
        LearnMapsSheet {
            doc,
            open: cx.new(|_| None),
        }
    }
}

/// Runs `f` on document `doc` and redraws.
fn edit(
    doc: DocId,
    window: &mut Window,
    cx: &mut App,
    f: impl FnOnce(&mut vizdaw_app::DocumentModel),
) {
    AppState::global(cx).update(cx, |s, cx| {
        s.store.with_document(doc, |d, _| f(d));
        cx.notify();
    });
    window.refresh();
}

/// A line of text recorded in the tree.
fn text(t: &str, colour: u32, scope: &mut RegistryScope) -> gpui::Div {
    div()
        .relative()
        .text_color(rgb(colour))
        .child(t.to_owned())
        .child(scope.control("text", t, true))
}

impl LearnMapsSheet {
    /// Map `m`'s row (`:47-73`): source over target, the mode picker, Pick-up,
    /// Remove.
    fn row(
        &self,
        i: usize,
        m: &LearnMapRow,
        target: &str,
        scope: &mut RegistryScope,
        cx: &App,
    ) -> AnyElement {
        let doc = self.doc;
        let open = self.open.read(cx).as_ref() == Some(&m.source);
        let names = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(text(&m.text, colors::text(), scope))
            .child(text(target, colors::dim(), scope).text_xs());
        let toggling = self.open.clone();
        let source = m.source.clone();
        let face = menu_face(
            &format!("Mode of {}", m.text),
            mode_title(m.mode),
            None,
            true,
            scope,
            move |window, cx| {
                toggling.update(cx, |o, _| {
                    *o = (o.as_ref() != Some(&source)).then(|| source.clone());
                });
                window.refresh();
            },
        );
        let picker = div()
            .relative()
            .flex_none()
            .w(px(MODE_WIDTH))
            .child(face)
            .when(open, |d| d.child(self.modes(m, scope)));
        let on = m.picks_up && m.pickup;
        let flip = m.clone();
        let pickup = with_help(
            div()
                .id(("learn-pickup", i))
                .relative()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .text_color(rgb(if m.picks_up {
                    colors::text()
                } else {
                    colors::disabled()
                }))
                .child(if on { "\u{2611}" } else { "\u{2610}" })
                .child(PICKUP)
                .child(scope.control_selected(
                    "checkbox",
                    &format!("{PICKUP} of {}", m.text),
                    m.picks_up,
                    Some(PICKUP_HELP),
                    on,
                )),
            PICKUP_HELP,
        )
        .when(m.picks_up, |d| {
            d.cursor_pointer().on_click(move |_, window, cx| {
                edit(doc, window, cx, |d| d.set_learn_pickup(&flip, !on));
                cx.stop_propagation();
            })
        });
        let gone = m.clone();
        let remove = div()
            .id(("learn-remove", i))
            .relative()
            .flex_none()
            .h(px(24.))
            .px_3()
            .flex()
            .items_center()
            .rounded(px(4.))
            .bg(rgb(colors::button()))
            .text_color(rgb(colors::text()))
            .cursor_pointer()
            .child(REMOVE)
            .child(scope.control("button", &format!("{REMOVE} {}", m.text), true))
            .on_click(move |_, window, cx| {
                edit(doc, window, cx, |d| d.remove_learn(&gone));
                cx.stop_propagation();
            });
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .py_1()
            .child(names)
            .child(picker)
            .child(pickup)
            .child(remove)
            .into_any_element()
    }

    /// The open mode picker's items, under its face.
    fn modes(&self, m: &LearnMapRow, scope: &mut RegistryScope) -> AnyElement {
        let doc = self.doc;
        let items = MODES.iter().enumerate().map(|(i, &mode)| {
            let (open, map) = (self.open.clone(), m.clone());
            let title = mode_title(mode);
            div()
                .id(("learn-mode", i))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::button())))
                .child(title)
                .child(scope.control_selected("menuitem", title, true, None, mode == m.mode))
                .on_click(move |_, window, cx| {
                    open.update(cx, |o, _| *o = None);
                    if mode != map.mode {
                        set_mode(doc, &map, mode, window, cx);
                    } else {
                        window.refresh();
                    }
                    cx.stop_propagation();
                })
        });
        div()
            .absolute()
            .top_full()
            .left_0()
            .child(
                deferred(
                    anchored().snap_to_window().child(
                        div()
                            .id("learn-modes")
                            .occlude()
                            .flex()
                            .flex_col()
                            .w(px(MODE_WIDTH))
                            .py_1()
                            .bg(rgb(colors::list()))
                            .border_1()
                            .border_color(rgb(colors::border()))
                            .rounded(px(6.))
                            .children(items),
                    ),
                )
                .with_priority(3),
            )
            .into_any_element()
    }
}

fn set_mode(doc: DocId, m: &LearnMapRow, mode: LearnModeRow, window: &mut Window, cx: &mut App) {
    edit(doc, window, cx, |d| d.set_learn_mode(m, mode));
}

impl Sheet for LearnMapsSheet {
    fn label(&self) -> String {
        TITLE.to_owned()
    }

    fn render(
        &mut self,
        scope: &mut RegistryScope,
        _window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let (maps, problem) = {
            let state = AppState::global(cx).read(cx);
            match state.store.document(self.doc) {
                Some(d) => (
                    d.learn
                        .maps()
                        .iter()
                        .map(|m| (m.clone(), LearnModel::target_text(m, d)))
                        .collect::<Vec<_>>(),
                    d.learn.problem().map(str::to_owned),
                ),
                None => (vec![], None),
            }
        };
        // A map gone (removed, undone) takes its open picker with it.
        let open = self.open.read(cx).clone();
        if open.is_some_and(|s| !maps.iter().any(|(m, _)| m.source == s)) {
            self.open.update(cx, |o, _| *o = None);
        }
        let title = text(TITLE, colors::text(), scope).text_base();
        let body = if maps.is_empty() {
            div()
                .min_h(px(EMPTY_HEIGHT))
                .child(text(NO_MAPS, colors::dim(), scope))
                .into_any_element()
        } else {
            let mut list = scope.container();
            let rows: Vec<AnyElement> = maps
                .iter()
                .enumerate()
                .map(|(i, (m, target))| self.row(i, m, target, &mut list, cx))
                .collect();
            div()
                .id("learn-maps-list")
                .min_h(px(LIST_HEIGHT))
                .max_h(px(LIST_MAX))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .border_1()
                .border_color(rgb(colors::border()))
                .px_2()
                .children(rows)
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(title)
            .child(body)
            .children(problem.map(|p| text(&p, colors::error(), scope)))
            .into_any_element()
    }

    fn buttons(&self, _cx: &App) -> Vec<SheetButton> {
        vec![SheetButton::new(DONE, ButtonRole::DefaultCancel)]
    }

    fn press(&mut self, _index: usize, _window: &mut Window, cx: &mut App) -> bool {
        let doc = self.doc;
        AppState::global(cx).update(cx, |s, cx| {
            s.store.with_document(doc, |d, _| d.learn.close_maps());
            cx.notify();
        });
        true
    }

    fn width(&self) -> f32 {
        WIDTH
    }
}

/// "MIDI Learn Maps…": the sheet on `view`'s window, the document's or
/// the mixer's. One is open at a time: none while this window shows
/// another sheet, while another window of the document shows this one,
/// or under the control channel.
pub fn show(view: &mut DocumentWindow, window: &mut Window, cx: &mut Context<DocumentWindow>) {
    if crate::control::channel_on(cx) || view.sheet_label().is_some() {
        return;
    }
    let (doc, token) = (view.doc(), view.token());
    let app = AppState::global(cx);
    let elsewhere = app
        .read(cx)
        .windows()
        .iter()
        .filter(|w| w.doc == doc && w.token != token)
        .any(|w| sheet_is(w.handle, TITLE, cx));
    if elsewhere {
        return;
    }
    app.update(cx, |s, cx| {
        s.store.with_document(doc, |d, _| {
            // A sheet whose window went without Done left it open.
            d.learn.close_maps();
            d.learn.open_maps();
        });
        cx.notify();
    });
    let sheet = LearnMapsSheet::new(doc, cx);
    if view.show_sheet(Box::new(sheet), window, cx).is_err() {
        app.update(cx, |s, _| {
            s.store.with_document(doc, |d, _| d.learn.close_maps());
        });
    }
}
