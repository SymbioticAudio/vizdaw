//! The bottom pane and its detail editor (Swift `PaneFrame`, `DetailArea`
//! and `DetailPane`): under the arrangement, a picker of the editor kinds
//! (the List only, this slice), then the open detail's title and its close
//! button over the editor, or the empty state when nothing is open. Its
//! height follows the layout's bottom pane, fitted to the window as
//! `PaneGeometry.swift` fits it.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Size,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::editor::{DetailEditorKind, PaneSide, PaneState};

use crate::list_editor::{self, ListEditor, ListState};
use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::transport_bar::{self, colors};

/// The pane's group in the tree, and its picker's label (Swift
/// `PaneSide.bottom.title`).
pub const PANE: &str = "Bottom Pane";
/// The close button's label (Swift's help text).
pub const CLOSE: &str = "Close Detail Editor";
/// The empty state (Swift `DetailArea`).
pub const EMPTY_TITLE: &str = "No Editor Open";
pub const EMPTY_HELP: &str = "Double-click a block, or press \u{2318}1 to \u{2318}4.";

/// The line between the arrangement and the pane.
pub const DIVIDER: f32 = 1.;
/// What the arrangement keeps, at least (Swift `centreMinimum`).
pub const CENTRE_MINIMUM: (f32, f32) = (240., 160.);
/// The narrowest the pane shows (Swift `bottomMinimumWidth`).
pub const BOTTOM_MINIMUM_WIDTH: f32 = 240.;

/// The kinds the picker lists this slice (the others come with their
/// editors).
const KINDS: [DetailEditorKind; 1] = [DetailEditorKind::List];

/// The pane's height in a regions area of `area` (under the transport bar,
/// above the status line), or `None` when it does not show: hidden in the
/// layout, or no room for it beside the arrangement's minimum this frame
/// (the layout keeps `shown`). Its size is clamped to its range, then
/// shrunk to leave the arrangement 160 points and the divider (Swift
/// `PaneGeometry.fit` with the bottom pane alone).
pub fn bottom_height(area: Size<Pixels>, pane: &PaneState) -> Option<Pixels> {
    if !pane.shown {
        return None;
    }
    let (w, h) = (f32::from(area.width), f32::from(area.height));
    let range = PaneSide::Bottom.size_range();
    let (lo, hi) = (*range.start() as f32, *range.end() as f32);
    let size = if pane.size.is_finite() {
        (pane.size as f32).clamp(lo, hi)
    } else {
        lo
    };
    let fitted = size.min(h - DIVIDER - CENTRE_MINIMUM.1).max(lo);
    let fits = h - DIVIDER - fitted >= CENTRE_MINIMUM.1
        && w >= CENTRE_MINIMUM.0
        && w >= BOTTOM_MINIMUM_WIDTH;
    fits.then(|| px(fitted))
}

/// What the pane draws of the document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DetailPaneState {
    /// The picker's choice: the open detail's kind, else the last used.
    pub choice: DetailEditorKind,
    /// The open detail's kind and title.
    pub detail: Option<(DetailEditorKind, String)>,
}

impl DetailPaneState {
    pub fn of(d: &DocumentModel) -> Self {
        let l = d.layout();
        DetailPaneState {
            choice: l.detail.as_ref().map_or(l.last_kind, |x| x.kind),
            detail: l.detail.as_ref().map(|x| (x.kind, d.detail_title())),
        }
    }
}

/// The pane: `height` when shown; `None` keeps it mounted at zero height,
/// out of the tree and the pointer's way (Swift keeps the hidden pane's
/// editor mounted). `list` is the List editor's, when one is open.
/// `scope` is the pane's own container.
pub(crate) fn render(
    state: &DetailPaneState,
    height: Option<Pixels>,
    menu_open: bool,
    list: Option<(&ListEditor, &ListState)>,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let shown = height.is_some();
    let picker = picker(state, menu_open && shown, scope, window, cx);
    let content = match &state.detail {
        Some((kind, title)) => detail(*kind, title, list, scope, window, cx),
        None => empty_state(scope),
    };
    div()
        .id("bottom-pane")
        .relative()
        .flex()
        .flex_col()
        .flex_none()
        .w_full()
        .h(height.unwrap_or(px(0.)))
        .overflow_hidden()
        .when(!shown, |d| d.invisible())
        .bg(rgb(colors::READOUT))
        .child(picker)
        .child(divider())
        .child(content)
        .when(shown, |d| d.child(scope.this("group", PANE, true)))
        .into_any_element()
}

/// The 1-point line between two parts.
pub(crate) fn divider() -> impl IntoElement {
    div().flex_none().h(px(DIVIDER)).bg(rgb(colors::BORDER))
}

/// The pane's header: its picker (Swift's menu-style `Picker`, labelled
/// "Bottom Pane"), showing the current choice.
fn picker(
    state: &DetailPaneState,
    menu_open: bool,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let title = state.choice.title();
    let mut button = div()
        .id("bottom-pane-picker")
        .relative()
        .h(px(24.))
        .flex()
        .flex_none()
        .items_center()
        .px_2()
        .gap_1()
        .rounded_md()
        .cursor_pointer()
        .text_xs()
        .bg(rgb(colors::BUTTON))
        .text_color(rgb(colors::TEXT))
        .child(format!("{title} \u{25be}"))
        .child(scope.control_with("button", PANE, true, Some(title), false))
        .on_click(cx.listener(|this, _, _, cx| {
            this.detail_menu_open = !this.detail_menu_open;
            cx.notify();
        }));
    if menu_open {
        button = button.children(kind_menu(state, &mut scope.container(), window, cx));
    }
    div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .px(px(8.))
        .py(px(4.))
        .bg(rgb(colors::BAR))
        .child(button)
        .child(div().flex_1())
        .into_any_element()
}

/// The picker's menu: the kinds this slice has. Choosing one shows it, on
/// the open detail's target or the selection.
fn kind_menu(
    state: &DetailPaneState,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> Vec<AnyElement> {
    let size = window.viewport_size();
    let backdrop = deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("bottom-pane-backdrop")
                .w(size.width)
                .h(size.height)
                .occlude()
                .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                    this.detail_menu_open = false;
                    cx.notify();
                })),
        ),
    )
    .with_priority(1)
    .into_any_element();
    let mut list = crate::arrangement::menu_list("bottom-pane-menu");
    for kind in KINDS {
        let current = state.choice == kind;
        list = list.child(
            div()
                .id(("bottom-pane-kind", kind as usize))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::BUTTON)))
                .child(format!(
                    "{}{}",
                    if current { "\u{2713} " } else { "" },
                    kind.title()
                ))
                .child(scope.control_selected("menuitem", kind.title(), true, None, current))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.detail_menu_open = false;
                    this.with_model(cx, |d| d.show_detail(kind));
                })),
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

/// The open detail (Swift `DetailPane`): its title and close button, a
/// divider, then the editor.
fn detail(
    kind: DetailEditorKind,
    title: &str,
    list: Option<(&ListEditor, &ListState)>,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let header = div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap_2()
        .px(px(8.))
        .py(px(4.))
        .child(
            div()
                .relative()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(rgb(colors::TEXT))
                .child(title.to_owned())
                .child(scope.control("text", title, true)),
        )
        .child(div().flex_1())
        .child(
            transport_bar::cluster_button(
                "close-detail",
                "\u{2715}",
                CLOSE,
                true,
                colors::BAR,
                scope,
            )
            .h(px(22.))
            .on_click(cx.listener(|this, _, _, cx| {
                this.with_model(cx, DocumentModel::close_detail);
            })),
        );
    let editor = match (kind, list) {
        (DetailEditorKind::List, Some((editor, state))) => div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(list_editor::render(
                editor,
                state,
                &mut scope.container(),
                window,
                cx,
            )),
        // No other kind opens this slice.
        _ => div().flex_1().min_h_0(),
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(header)
        .child(divider())
        .child(editor)
        .into_any_element()
}

/// Nothing open (Swift's `ContentUnavailableView`).
fn empty_state(scope: &mut RegistryScope) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .gap_1()
        .child(
            div()
                .relative()
                .text_color(rgb(colors::TEXT))
                .child(EMPTY_TITLE)
                .child(scope.control("text", EMPTY_TITLE, true)),
        )
        .child(
            div()
                .relative()
                .text_xs()
                .text_color(rgb(colors::DIM))
                .child(EMPTY_HELP)
                .child(scope.control("text", EMPTY_HELP, true)),
        )
        .into_any_element()
}
