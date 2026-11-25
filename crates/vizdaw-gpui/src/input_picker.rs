//! A track's input picker (Swift `LaneHeaderView.inputMenu`,
//! `StripInputView`): an anchored menu of the model's sections (the
//! untitled "No Input", then "Audio", "MIDI Input", "MIDI Channel" as the
//! track records), a separator between sections and a ✓ on the track's
//! input. The track header and the mixer strip share it; each owns its
//! open state. A backdrop under the list closes it on a click outside,
//! so one picker is open per window; Escape and a choice close it too.
//!
//! In the tree: each section title is a disabled `menuitem`, each choice
//! a `menuitem` (selected: the current input).

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, Window, anchored, deferred, div, point, px, rgb,
};
use vizdaw_app::lane_inputs::{InputChoice, InputGroup};
use vizdaw_ffi::edit::LaneInputRow;

use crate::arrangement::{menu_list, menu_separator};
use crate::registry::RegistryScope;
use crate::transport_bar::colors;

/// One row of the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerRow {
    /// A section's title (not chosen).
    Title(&'static str),
    Separator,
    Choice {
        choice: InputChoice,
        /// The track's input now.
        checked: bool,
    },
}

/// The picker's rows for `groups` with `current` checked (Swift: a
/// separator before every section but the first, a disabled title for a
/// titled one).
pub fn rows(groups: Vec<InputGroup>, current: Option<&LaneInputRow>) -> Vec<PickerRow> {
    let mut out = vec![];
    for (i, g) in groups.into_iter().enumerate() {
        if i > 0 {
            out.push(PickerRow::Separator);
        }
        if !g.title.is_empty() {
            out.push(PickerRow::Title(g.title));
        }
        for c in g.choices {
            let checked = c.input.as_ref() == current;
            out.push(PickerRow::Choice { choice: c, checked });
        }
    }
    out
}

/// What a choice does (the picker has closed).
pub type Choose = Rc<dyn Fn(&InputChoice, &mut Window, &mut App)>;
/// What closes the picker.
pub type Close = Rc<dyn Fn(&mut Window, &mut App)>;

/// The open picker: the backdrop over the window (a press on it closes
/// the picker) and the list anchored under its owner, which must be
/// `.relative()`. `id` names the list.
pub fn popup(
    id: &'static str,
    rows: Vec<PickerRow>,
    scope: &mut RegistryScope,
    window: &Window,
    close: Close,
    choose: Choose,
) -> Vec<AnyElement> {
    let size = window.viewport_size();
    let closing = close.clone();
    let backdrop = deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("input-picker-backdrop")
                .w(size.width)
                .h(size.height)
                .occlude()
                .on_any_mouse_down(move |_, window, cx| closing(window, cx)),
        ),
    )
    .with_priority(1)
    .into_any_element();
    let list = list(id, rows, &mut scope.container(), close, choose);
    let list = div()
        .absolute()
        .top_full()
        .left_0()
        .child(deferred(anchored().snap_to_window().child(list)).with_priority(2))
        .into_any_element();
    vec![backdrop, list]
}

fn list(
    id: &'static str,
    rows: Vec<PickerRow>,
    scope: &mut RegistryScope,
    close: Close,
    choose: Choose,
) -> AnyElement {
    let mut list = menu_list(id).min_w(px(180.));
    for (n, row) in rows.into_iter().enumerate() {
        list = list.child(match row {
            PickerRow::Separator => menu_separator(),
            PickerRow::Title(title) => div()
                .id(("input-picker-title", n))
                .relative()
                .px_3()
                .pt_1()
                .text_xs()
                .text_color(rgb(colors::DIM))
                .child(title)
                .child(scope.control("menuitem", title, false))
                .into_any_element(),
            PickerRow::Choice { choice, checked } => {
                let (close, choose) = (close.clone(), choose.clone());
                let title = choice.title.clone();
                div()
                    .id(("input-picker-item", n))
                    .relative()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .px_3()
                    .py_1()
                    .text_xs()
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors::BUTTON)))
                    .child(
                        div()
                            .w(px(10.))
                            .flex_none()
                            .when(checked, |d| d.child("\u{2713}")),
                    )
                    .child(title.clone())
                    .child(scope.control_selected("menuitem", &title, true, None, checked))
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        close(window, cx);
                        choose(&choice, window, cx);
                    })
                    .into_any_element()
            }
        });
    }
    list.into_any_element()
}
