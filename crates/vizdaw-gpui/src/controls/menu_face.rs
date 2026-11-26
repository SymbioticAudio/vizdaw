//! A strip menu's face (Swift `StripMenuFace`): the current choice, an
//! optional colour dot before it (the automation mode's), and the menu
//! arrow; 24 points high, as wide as it is given. `menu` in the tree, its
//! value the face's text. The menu itself is the strip's to open.

use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement, Rgba,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::registry::RegistryScope;
use crate::strip_palette::tokens;

/// The face's height (Swift `ControlMetrics.minimumHit`).
pub const HEIGHT: f64 = 24.;

/// The face of menu `label` showing `text`; a click calls `on_click` (not
/// while disabled).
pub fn menu_face(
    label: &str,
    text: &str,
    dot: Option<Rgba>,
    enabled: bool,
    scope: &mut RegistryScope,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let k = tokens();
    let mut face = div()
        .id(ElementId::Name(format!("mixer-menu:{label}").into()))
        .size_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .px(px(4.))
        .rounded(px(k.radius))
        .border_1()
        .border_color(rgb(k.surface_separator))
        .bg(rgb(k.surface_inset))
        .text_size(px(10.))
        .opacity(if enabled { 1. } else { 0.45 })
        .children(dot.map(|c| div().flex_none().size(px(7.)).rounded_full().bg(c)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(rgb(k.text_primary))
                .child(text.to_owned()),
        )
        .child(
            div()
                .flex_none()
                .text_color(rgb(k.text_secondary))
                .child("\u{25BE}"),
        );
    if enabled {
        face = face
            .cursor_pointer()
            .on_click(move |_, window, cx| on_click(window, cx));
    }
    div()
        .relative()
        .w_full()
        .h(px(HEIGHT as f32))
        .child(face)
        .child(scope.control_with("menu", label, enabled, Some(text), false))
        .into_any_element()
}
