//! A stepper (Swift `Stepper`): its value as text, then a minus and a plus
//! button. The value is clamped to its range and moves by its step; the
//! button at each end is disabled. In the tree: the value is a `text`, the
//! buttons `button` named "Decrease <name>" and "Increase <name>".

use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::registry::RegistryScope;
use crate::strip_palette::tokens;

/// A button's side.
pub const BUTTON: f32 = 22.;

/// A stepper's range and step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub min: i64,
    pub max: i64,
    pub step: i64,
}

impl Range {
    /// `value` one step down (`dir` negative) or up, clamped; `None` when it
    /// is already at that end.
    pub fn stepped(self, value: i64, dir: i64) -> Option<i64> {
        let v = (value + dir.signum() * self.step).clamp(self.min, self.max);
        (v != value.clamp(self.min, self.max)).then_some(v)
    }

    pub fn can_decrease(self, value: i64) -> bool {
        self.stepped(value, -1).is_some()
    }

    pub fn can_increase(self, value: i64) -> bool {
        self.stepped(value, 1).is_some()
    }
}

/// One of the pair's buttons.
fn button(
    glyph: &'static str,
    name: String,
    enabled: bool,
    scope: &mut RegistryScope,
    on_press: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let k = tokens();
    let mut face = div()
        .id(ElementId::Name(format!("stepper:{name}").into()))
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(k.radius))
        .border_1()
        .border_color(rgb(k.surface_separator))
        .bg(rgb(k.surface_inset))
        .text_color(rgb(k.text_primary))
        .opacity(if enabled { 1. } else { 0.45 })
        .child(glyph);
    if enabled {
        face = face
            .cursor_pointer()
            .on_click(move |_, window, cx| on_press(window, cx));
    }
    div()
        .relative()
        .flex_none()
        .w(px(BUTTON))
        .h(px(BUTTON))
        .child(face)
        .child(scope.control("button", &name, enabled))
        .into_any_element()
}

/// The minus and plus buttons of the stepper `name`, for the range's ends
/// at `value`.
pub fn stepper_buttons(
    name: &str,
    range: Range,
    value: i64,
    scope: &mut RegistryScope,
    on_step: impl Fn(i64, &mut Window, &mut App) + Clone + 'static,
) -> AnyElement {
    let down = {
        let on = on_step.clone();
        button(
            "\u{2212}",
            format!("Decrease {name}"),
            range.can_decrease(value),
            scope,
            move |w, cx| {
                if let Some(v) = range.stepped(value, -1) {
                    on(v, w, cx);
                }
            },
        )
    };
    let up = button(
        "+",
        format!("Increase {name}"),
        range.can_increase(value),
        scope,
        move |w, cx| {
            if let Some(v) = range.stepped(value, 1) {
                on_step(v, w, cx);
            }
        },
    );
    div()
        .flex()
        .flex_row()
        .gap(px(2.))
        .child(down)
        .child(up)
        .into_any_element()
}
