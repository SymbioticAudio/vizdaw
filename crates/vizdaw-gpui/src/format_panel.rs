//! The New Project format panel (Swift `FormatPanel`,
//! `ProjectPanels.swift`): "New Project", its message, a `Sample rate:`
//! pop-up of the core's project rates and a `Bit depth:` pop-up, `Create`
//! and `Cancel`. It starts at the default format; Create answers the
//! chosen format. A [`Sheet`]: on the front window, or in a dialog window
//! of its own when no window is open ([`crate::file_menu`]).
//!
//! Each pop-up is a face showing its choice (`menu` in the tree, its
//! value the choice); a click opens its items (`menuitem`, the chosen one
//! selected), and a click on one chooses it.

use futures::channel::oneshot;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, px, rgb,
};
use vizdaw_app::formats::{
    BIT_DEPTH_LABEL, FORMAT_CANCEL, FormatChoice, NEW_PROJECT_CREATE, NEW_PROJECT_MESSAGE,
    NEW_PROJECT_TITLE, SAMPLE_RATE_LABEL, depth_title, rate_title,
};
use vizdaw_ffi::ProjectFormatRow;

use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton};
use crate::transport_bar::colors;

/// Which pop-up is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popup {
    Rate,
    Depth,
}

/// The panel's state: the choice and the open pop-up.
pub struct FormatState {
    pub choice: FormatChoice,
    pub open: Option<Popup>,
}

/// The format panel as a sheet. Create answers the chosen format; Cancel
/// (or the sheet dropped unanswered) answers nothing.
pub struct FormatSheet {
    state: Entity<FormatState>,
    answer: Option<oneshot::Sender<ProjectFormatRow>>,
}

impl FormatSheet {
    /// The panel over `rates`, starting at `initial`: the sheet, its state
    /// and the answer.
    pub fn new(
        rates: Vec<u32>,
        initial: ProjectFormatRow,
        cx: &mut App,
    ) -> (
        FormatSheet,
        Entity<FormatState>,
        oneshot::Receiver<ProjectFormatRow>,
    ) {
        let (tx, rx) = oneshot::channel();
        let state = cx.new(|_| FormatState {
            choice: FormatChoice::new(rates, initial),
            open: None,
        });
        (
            FormatSheet {
                state: state.clone(),
                answer: Some(tx),
            },
            state,
            rx,
        )
    }
}

/// The pop-up `which`'s label in the tree (its visible label without the
/// colon).
fn tree_label(which: Popup) -> &'static str {
    let label = match which {
        Popup::Rate => SAMPLE_RATE_LABEL,
        Popup::Depth => BIT_DEPTH_LABEL,
    };
    label.trim_end_matches(':')
}

/// Pop-up `which`: its face, and its items while open.
fn popup(
    which: Popup,
    state: &Entity<FormatState>,
    scope: &mut RegistryScope,
    cx: &App,
) -> AnyElement {
    let s = state.read(cx);
    let (items, chosen): (Vec<String>, String) = match which {
        Popup::Rate => (
            s.choice.rate_titles(),
            rate_title(s.choice.format.sample_rate),
        ),
        Popup::Depth => (
            s.choice
                .depth_titles()
                .into_iter()
                .map(String::from)
                .collect(),
            depth_title(s.choice.format.depth).to_owned(),
        ),
    };
    let open = s.open == Some(which);
    let label = tree_label(which);
    let toggle = state.clone();
    let face = div()
        .id(("format-popup", which as usize))
        .relative()
        .h(px(24.))
        .min_w(px(110.))
        .px_2()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(colors::BORDER))
        .bg(rgb(colors::BUTTON))
        .cursor_pointer()
        .child(chosen.clone())
        .child(div().text_color(rgb(colors::DIM)).child("\u{25BE}"))
        .child(scope.control_with("menu", label, true, Some(&chosen), false))
        .on_click(move |_, window, cx| {
            toggle.update(cx, |s, _| {
                s.open = (s.open != Some(which)).then_some(which);
            });
            window.refresh();
        });
    face.when(open, |d| {
        let rows = items.iter().enumerate().map(|(i, title)| {
            let pick = state.clone();
            let title_owned = title.clone();
            div()
                .id(("format-item", i))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::BUTTON)))
                .child(title.clone())
                .child(scope.control_selected("menuitem", title, true, None, *title == chosen))
                .on_click(move |_, window, cx| {
                    pick.update(cx, |s, _| {
                        match which {
                            Popup::Rate => s.choice.choose_rate(&title_owned),
                            Popup::Depth => s.choice.choose_depth(&title_owned),
                        }
                        s.open = None;
                    });
                    cx.stop_propagation();
                    window.refresh();
                })
        });
        d.child(
            div().absolute().top_full().left_0().child(
                deferred(
                    anchored().snap_to_window().child(
                        div()
                            .id("format-list")
                            .occlude()
                            .flex()
                            .flex_col()
                            .min_w(px(110.))
                            .py_1()
                            .bg(rgb(0x2e2e38))
                            .border_1()
                            .border_color(rgb(colors::BORDER))
                            .rounded_md()
                            .children(rows),
                    ),
                )
                .with_priority(3),
            ),
        )
    })
    .into_any_element()
}

impl Sheet for FormatSheet {
    fn label(&self) -> String {
        NEW_PROJECT_TITLE.to_owned()
    }

    fn render(&mut self, scope: &mut RegistryScope, _: &mut Window, cx: &mut App) -> AnyElement {
        let text = |t: &str, scope: &mut RegistryScope| {
            div()
                .relative()
                .child(t.to_owned())
                .child(scope.control("text", t, true))
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(text(NEW_PROJECT_TITLE, scope).font_weight(gpui::FontWeight::BOLD))
            .child(text(NEW_PROJECT_MESSAGE, scope).text_color(rgb(colors::DIM)))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(SAMPLE_RATE_LABEL)
                    .child(popup(Popup::Rate, &self.state, scope, cx))
                    .child(BIT_DEPTH_LABEL)
                    .child(popup(Popup::Depth, &self.state, scope, cx)),
            )
            .into_any_element()
    }

    fn buttons(&self, _: &App) -> Vec<SheetButton> {
        vec![
            SheetButton::new(NEW_PROJECT_CREATE, ButtonRole::Default),
            SheetButton::new(FORMAT_CANCEL, ButtonRole::Cancel),
        ]
    }

    fn press(&mut self, index: usize, _: &mut Window, cx: &mut App) -> bool {
        if index == 0
            && let Some(tx) = self.answer.take()
        {
            tx.send(self.state.read(cx).choice.format).ok();
        }
        // Cancel drops the answer.
        self.answer = None;
        true
    }
}
