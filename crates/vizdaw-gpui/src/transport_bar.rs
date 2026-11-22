//! The transport bar: Play/Stop, Stop and Return, and the bar and time
//! readouts (tempo and signature come with sequence info in slice 2).

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Div, ElementId, InteractiveElement, IntoElement, ParentElement, Rgba, Stateful,
    StatefulInteractiveElement, Styled, div, px, rgb,
};
use vizdaw_app::transport::{ReadoutBox, TransportReadout};

use crate::menus::{PlayStop, StopAndReturn};
use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;

/// What the bar draws.
pub struct BarState {
    pub readout: TransportReadout,
    pub can_play: bool,
}

pub(crate) mod colors {
    pub const BAR: u32 = 0x26262e;
    pub const BUTTON: u32 = 0x3a3f4f;
    pub const PLAYING: u32 = 0x2f6f4f;
    pub const READOUT: u32 = 0x15151a;
    pub const TEXT: u32 = 0xe8e8ec;
    pub const DISABLED: u32 = 0x77777f;
    pub const DIM: u32 = 0x9a9aa6;
}

fn color(enabled: bool) -> Rgba {
    rgb(if enabled {
        colors::TEXT
    } else {
        colors::DISABLED
    })
}

/// A push button that records itself in `scope`. Disabled buttons take no
/// clicks; the caller adds `on_click` to enabled ones.
pub(crate) fn button(
    id: impl Into<ElementId>,
    label: &str,
    enabled: bool,
    background: u32,
    scope: &mut RegistryScope,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .h(px(28.))
        .min_w(px(96.))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(rgb(background))
        .text_color(color(enabled))
        .when(enabled, |b| b.cursor_pointer())
        .child(label.to_owned())
        .child(scope.control("button", label, enabled))
}

fn readout(which: ReadoutBox, text: &str, scope: &mut RegistryScope) -> Div {
    div()
        .relative()
        .w(px(132.))
        .h(px(36.))
        .px_2()
        .flex()
        .flex_col()
        .justify_center()
        .rounded_md()
        .bg(rgb(colors::READOUT))
        .child(
            div()
                .text_xs()
                .text_color(rgb(colors::DIM))
                .child(which.title()),
        )
        .child(div().text_color(rgb(colors::TEXT)).child(text.to_owned()))
        .child(scope.control("text", which.accessibility_label(), true))
}

pub fn render(
    state: &BarState,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> impl IntoElement {
    let playing = state.readout.playing;
    let play_label = if playing { "Stop" } else { "Play" };
    let play_enabled = state.can_play;
    let play = button(
        "play-stop",
        play_label,
        play_enabled,
        if playing {
            colors::PLAYING
        } else {
            colors::BUTTON
        },
        scope,
    )
    .when(play_enabled, |b| {
        b.on_click(cx.listener(|this, _, window, cx| this.play_stop(&PlayStop, window, cx)))
    });
    let stop = button(
        "stop-and-return",
        "Stop and Return",
        playing,
        colors::BUTTON,
        scope,
    )
    .when(playing, |b| {
        b.on_click(
            cx.listener(|this, _, window, cx| this.stop_and_return(&StopAndReturn, window, cx)),
        )
    });
    let texts = &state.readout.texts;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .bg(rgb(colors::BAR))
        .child(play)
        .child(stop)
        .child(readout(ReadoutBox::Bar, &texts.bar, scope))
        .child(readout(ReadoutBox::Time, &texts.time, scope))
}
