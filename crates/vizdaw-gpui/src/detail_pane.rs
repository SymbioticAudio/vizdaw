//! The bottom pane's editor (Swift `DetailArea` and `DetailPane`): the
//! open detail's title and its close button over the editor, or the empty
//! state when nothing is open. The pane host (`panes.rs`) frames it with
//! the bottom pane's picker and keeps it mounted while the pane is hidden
//! or shows another view.

use gpui::{
    AnyElement, Context, IntoElement, ParentElement, StatefulInteractiveElement, Styled, Window,
    div, px, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::editor::DetailEditorKind;

use crate::drum_grid::{self, DrumGridState, DrumGridView};
use crate::list_editor::{self, ListEditor, ListState};
use crate::piano_roll::{self, PianoRoll, RollState};
use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::strip_chart::{self, StripChart, StripChartState};
use crate::transport_bar::{self, colors};

/// The pane's group in the tree, and its picker's label (Swift
/// `PaneSide.bottom.title`).
pub const PANE: &str = "Bottom Pane";
/// The close button's label (Swift's help text).
pub const CLOSE: &str = "Close Detail Editor";
/// The empty state (Swift `DetailArea`).
pub const EMPTY_TITLE: &str = "No Editor Open";
pub const EMPTY_HELP: &str = "Double-click a block, or press \u{2318}1 to \u{2318}4.";

/// The line between two parts.
pub const DIVIDER: f32 = crate::panes::DIVIDER as f32;

/// What the pane draws of the document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DetailPaneState {
    /// The open detail's kind and title.
    pub detail: Option<(DetailEditorKind, String)>,
}

impl DetailPaneState {
    pub fn of(d: &DocumentModel) -> Self {
        let l = d.layout();
        DetailPaneState {
            detail: l.detail.as_ref().map(|x| (x.kind, d.detail_title())),
        }
    }
}

/// The editor area: the open detail, or the empty state. `list` is the
/// List editor's, when one is open, `roll` the Piano Roll's, `chart`
/// the Strip Chart's and `drum` the Drum Grid's. `scope` is the area's
/// own container.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    state: &DetailPaneState,
    list: Option<(&ListEditor, &ListState)>,
    roll: Option<(&PianoRoll, RollState)>,
    chart: Option<(&StripChart, StripChartState)>,
    drum: Option<(&DrumGridView, DrumGridState)>,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    match &state.detail {
        Some((kind, title)) => detail(*kind, title, list, roll, chart, drum, scope, window, cx),
        None => empty_state(scope),
    }
}

/// The 1-point line between two parts.
pub(crate) fn divider() -> impl IntoElement {
    div().flex_none().h(px(DIVIDER)).bg(rgb(colors::border()))
}

/// The open detail (Swift `DetailPane`): its title and close button, a
/// divider, then the editor.
#[allow(clippy::too_many_arguments)]
fn detail(
    kind: DetailEditorKind,
    title: &str,
    list: Option<(&ListEditor, &ListState)>,
    roll: Option<(&PianoRoll, RollState)>,
    chart: Option<(&StripChart, StripChartState)>,
    drum: Option<(&DrumGridView, DrumGridState)>,
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
                .text_color(rgb(colors::text()))
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
                colors::bar(),
                scope,
            )
            .h(px(22.))
            .on_click(cx.listener(|this, _, _, cx| {
                this.with_model(cx, DocumentModel::close_detail);
            })),
        );
    let editor = match (kind, list, roll, chart, drum) {
        (DetailEditorKind::PianoRoll, _, Some((roll, state)), _, _) => div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(piano_roll::render(
                roll,
                state,
                &mut scope.container(),
                window,
                cx,
            )),
        (DetailEditorKind::List, Some((editor, state)), _, _, _) => div()
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
        (DetailEditorKind::StripChart, _, _, Some((chart, state)), _) => div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(strip_chart::render(
                chart,
                state,
                &mut scope.container(),
                window,
                cx,
            )),
        (DetailEditorKind::DrumGrid, _, _, _, Some((view, state))) => div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(drum_grid::render(
                view,
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
                .text_color(rgb(colors::text()))
                .child(EMPTY_TITLE)
                .child(scope.control("text", EMPTY_TITLE, true)),
        )
        .child(
            div()
                .relative()
                .text_xs()
                .text_color(rgb(colors::dim()))
                .child(EMPTY_HELP)
                .child(scope.control("text", EMPTY_HELP, true)),
        )
        .into_any_element()
}
