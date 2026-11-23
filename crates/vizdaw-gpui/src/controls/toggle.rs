//! One of M S R I (Swift `StripToggle`): 24 by 24, lit in its state's
//! colour, `button` in the tree with the value `on` or `off`. Mute and
//! solo are clicks on the core (one gesture each); record arm and input
//! monitor act on the strip's input track, which the strip wires.

use std::time::Instant;

use gpui::{
    AnyElement, App, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use vizdaw_app::mixer::MixerModel;
use vizdaw_ffi::events::AutomationTargetRow as Target;

use super::MixerRef;
use crate::registry::RegistryScope;
use crate::strip_palette::tokens;

/// The toggle's side (Swift `ControlMetrics.hitSize(.toggle)`).
pub const SIDE: f64 = 24.;

/// What a toggle shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Toggle {
    /// "M", "S", "R", "I".
    pub letter: &'static str,
    pub on: bool,
    /// Its lit colour (0xRRGGBB).
    pub colour: u32,
    /// "Mute Synth".
    pub label: String,
    pub enabled: bool,
    /// Touched here or in the core: the 2-point ring.
    pub touched: bool,
}

impl Toggle {
    /// Strip `strip`'s mute, as `m` shows it.
    pub fn mute(m: &MixerModel, strip: u64) -> Option<Toggle> {
        let s = m.strip(strip)?;
        Some(Toggle {
            letter: "M",
            on: s.mute,
            colour: tokens().state_mute,
            label: format!("Mute {}", s.name),
            enabled: true,
            touched: m.is_touched(Target::StripMute { strip }),
        })
    }

    /// Strip `strip`'s solo (the master has none).
    pub fn solo(m: &MixerModel, strip: u64) -> Option<Toggle> {
        let s = m.strip(strip)?;
        Some(Toggle {
            letter: "S",
            on: s.solo,
            colour: tokens().state_solo,
            label: format!("Solo {}", s.name),
            enabled: true,
            touched: m.is_touched(Target::StripSolo { strip }),
        })
    }
}

/// Mute's press: a click on the core.
pub fn toggle_mute(mixer: &MixerRef, strip: u64) -> impl Fn(&mut Window, &mut App) + 'static {
    let mixer = mixer.clone();
    move |_, cx| {
        mixer.with(cx, |m, d| m.toggle_mute(d, strip, Instant::now()));
    }
}

/// Solo's press: a click on the core.
pub fn toggle_solo(mixer: &MixerRef, strip: u64) -> impl Fn(&mut Window, &mut App) + 'static {
    let mixer = mixer.clone();
    move |_, cx| {
        mixer.with(cx, |m, d| m.toggle_solo(d, strip, Instant::now()));
    }
}

/// Toggle `t`; a click calls `on_press` (not while disabled).
pub fn toggle(
    t: Toggle,
    scope: &mut RegistryScope,
    on_press: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let k = tokens();
    let fill = if t.on { t.colour } else { k.surface_inset };
    let legend = if t.on {
        k.legend_on(fill)
    } else {
        rgb(k.text_secondary)
    };
    let mut face = div()
        .id(ElementId::Name(format!("mixer-toggle:{}", t.label).into()))
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(k.radius))
        .border_1()
        .border_color(rgb(k.surface_separator))
        .bg(rgb(fill))
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .text_color(legend)
        .opacity(if t.enabled { 1. } else { 0.45 })
        .child(t.letter);
    if t.touched {
        face = face.border_2().border_color(rgb(k.focus_ring));
    }
    if t.enabled {
        face = face
            .cursor_pointer()
            .on_click(move |_, window, cx| on_press(window, cx));
    }
    // The record covers the whole toggle, border included.
    div()
        .relative()
        .flex_none()
        .w(px(SIDE as f32))
        .h(px(SIDE as f32))
        .child(face)
        .child(scope.control_with(
            "button",
            &t.label,
            t.enabled,
            Some(if t.on { "on" } else { "off" }),
            false,
        ))
        .into_any_element()
}
