//! The mixer's bar (Swift `MixerBar`, `S/MixerView.swift:133-188`): the
//! load, "Add Bus", the "Add Track" menu (New Audio, Instrument and MIDI
//! Track, each its track command; disabled with no sequence), and the
//! "Instruments" menu: "New Plugin Instrument…" opens the instrument
//! browser for a new instrument, then one item per destination opens it
//! for that destination. The browser hangs under "Instruments"; its pick
//! opens no window (Swift's opened none).

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use vizdaw_app::mixer::BrowserTarget;
use vizdaw_ffi::mixer::DestRow;

use crate::menus::RunCommand;
use crate::mixer_view::{ADD_BUS, BAR_HEIGHT, MenuEntry, MixerView, browser_popover, popup};
use crate::registry::RegistryScope;
use crate::strip_palette::tokens;
use crate::track_headers::ADD_TRACK_ITEMS;

/// The Add Track menu's button.
pub const ADD_TRACK: &str = "Add Track";
/// The Instruments menu's button.
pub const INSTRUMENTS: &str = "Instruments";
/// The Instruments menu's first item.
pub const NEW_PLUGIN_INSTRUMENT: &str = "New Plugin Instrument\u{2026}";
/// The Add Track button's help with a sequence, and without one.
pub const ADD_TRACK_HELP: &str = "Add a track";
pub const NO_SEQUENCE_HELP: &str = "Select a sequence first";
/// Add Bus's help.
pub const ADD_BUS_HELP: &str = "Add a bus";

/// A menu of the bar, while open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarMenu {
    AddTrack,
    Instruments,
}

/// The Add Track button's help: why it is off with no sequence.
pub fn add_track_help(has_sequence: bool) -> &'static str {
    if has_sequence {
        ADD_TRACK_HELP
    } else {
        NO_SEQUENCE_HELP
    }
}

/// A destination's item in the Instruments menu: "‹name›: ‹its plug-in,
/// or "MIDI"› → Plugin…".
pub fn destination_item(d: &DestRow) -> String {
    let plays = d.plugin.as_ref().map_or("MIDI", |p| p.name.as_str());
    format!("{}: {plays} \u{2192} Plugin\u{2026}", d.name)
}

/// The bar: the load, then Add Bus, Add Track and Instruments.
pub(crate) fn bar(
    view: &Entity<MixerView>,
    load: &str,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let t = tokens();
    let v = view.read(cx);
    let mixer = v.mixer.clone();
    let open = v.bar_menu();
    let (has_sequence, dests) = mixer
        .read(cx, |m, d| {
            (d.selected_seq().is_some(), m.destinations().to_vec())
        })
        .unwrap_or_default();
    let adding = mixer.clone();
    let add_bus = button("mixer-add-bus", ADD_BUS, false, true, scope).on_click(move |_, _, cx| {
        adding.with(cx, |m, d| m.add_bus(d));
    });
    let add_bus = crate::sheet_host::with_help(add_bus, ADD_BUS_HELP);

    let weak = view.downgrade();
    let add_track =
        button("mixer-add-track", ADD_TRACK, true, has_sequence, scope).when(has_sequence, |b| {
            b.on_click(move |_, _, cx| {
                weak.update(cx, |v, cx| v.toggle_bar_menu(BarMenu::AddTrack, cx))
                    .ok();
            })
        });
    let add_track = crate::sheet_host::with_help(add_track, add_track_help(has_sequence));
    let mut add_track = div().relative().child(add_track);
    if has_sequence && open == Some(BarMenu::AddTrack) {
        add_track = add_track.children(popup(
            view,
            "mixer-add-track-menu",
            track_items(),
            scope,
            window,
            cx,
        ));
    }

    let weak = view.downgrade();
    let instruments =
        button("mixer-instruments", INSTRUMENTS, true, true, scope).on_click(move |_, _, cx| {
            weak.update(cx, |v, cx| {
                // Its browser open: the button closes it.
                if v.browser_target().is_some_and(|t| t.wants_instrument()) {
                    v.close_browser(cx);
                } else {
                    v.toggle_bar_menu(BarMenu::Instruments, cx);
                }
            })
            .ok();
        });
    let instrument_browser = v.browser_target().filter(BrowserTarget::wants_instrument);
    let mut instruments = div().relative().child(instruments);
    if open == Some(BarMenu::Instruments) {
        instruments = instruments.children(popup(
            view,
            "mixer-instruments-menu",
            instrument_items(view, &dests),
            scope,
            window,
            cx,
        ));
    }
    if let Some(target) = instrument_browser {
        instruments = instruments.children(browser_popover(view, target, scope, window, cx));
    }

    div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .justify_end()
        .gap(px(12.))
        .h(px(BAR_HEIGHT as f32))
        .px(px(8.))
        .text_xs()
        .child(
            div()
                .relative()
                .text_color(rgb(t.text_secondary))
                .whitespace_nowrap()
                .child(load.to_owned())
                .child(scope.control("text", load, true)),
        )
        .child(add_bus)
        .child(add_track)
        .child(instruments)
        .into_any_element()
}

/// A bar button: `label`, a menu's chevron after it when `menu`, dimmed
/// when not `enabled`.
fn button(
    id: &'static str,
    label: &'static str,
    menu: bool,
    enabled: bool,
    scope: &mut RegistryScope,
) -> gpui::Stateful<gpui::Div> {
    let t = tokens();
    div()
        .id(id)
        .relative()
        .h(px(22.))
        .px(px(8.))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .rounded(px(t.radius))
        .border_1()
        .border_color(rgb(t.surface_separator))
        .bg(rgb(t.surface_inset))
        .whitespace_nowrap()
        .when(enabled, |d| d.cursor_pointer())
        .when(!enabled, |d| d.opacity(0.45))
        .child(label)
        .when(menu, |d| {
            d.child(div().text_color(rgb(t.text_secondary)).child("\u{25BE}"))
        })
        .child(scope.control("button", label, enabled))
}

/// The Add Track menu: each item runs its track command on the window's
/// document, as the track headers' menu does.
fn track_items() -> Vec<MenuEntry> {
    ADD_TRACK_ITEMS
        .into_iter()
        .map(|(title, id)| {
            MenuEntry::item(title, move |window, cx| {
                window.dispatch_action(Box::new(RunCommand::new(id)), cx);
            })
        })
        .collect()
}

/// The Instruments menu: New Plugin Instrument…, a divider, then each
/// destination's item; each opens the instrument browser for its target.
fn instrument_items(view: &Entity<MixerView>, dests: &[DestRow]) -> Vec<MenuEntry> {
    let opens = |target: BrowserTarget| {
        let weak = view.downgrade();
        move |window: &mut Window, cx: &mut App| {
            weak.update(cx, |v, cx| v.open_browser(target, window, cx))
                .ok();
        }
    };
    let mut out = vec![
        MenuEntry::item(NEW_PLUGIN_INSTRUMENT, opens(BrowserTarget::Instrument)),
        MenuEntry::Separator,
    ];
    out.extend(
        dests
            .iter()
            .map(|d| MenuEntry::item(destination_item(d), opens(BrowserTarget::Destination(d.id)))),
    );
    out
}
