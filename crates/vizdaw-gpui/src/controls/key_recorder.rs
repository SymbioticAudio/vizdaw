//! The key recorder (Swift `KeyRecorderView`,
//! `TransportFieldsView.swift:437-520`): a 96×20 button that reads
//! "Record"; pressed, it reads "Press a key…" and takes the next key.
//! Escape alone cancels; a key VizDAW cannot bind is ignored and
//! recording goes on. `button` in the tree, labelled "Record Key for
//! <title>", its value the face's text.
//!
//! The button only starts a recording. The window that owns it listens
//! for keys through `App::intercept_keystrokes`, which runs before any
//! key binding or menu (GPUI matches bindings only after the
//! interceptors, and stops there when one stops propagation), so ⌘ keys
//! are taken too, as Swift's `performKeyEquivalent` took them before the
//! menus. [`take`] turns what was pressed into what to do with it.

use core_control::bindings::{self, COMMAND, CONTROL, Chord, OPTION, SHIFT};
use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, Keystroke, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::registry::RegistryScope;
use crate::strip_palette::tokens;

/// The face's text while idle.
pub const IDLE: &str = "Record";
/// The face's text while a key is awaited.
pub const RECORDING: &str = "Press a key…";
/// The face's width and height (Swift `.frame(width: 96, height: 20)`).
pub const WIDTH: f32 = 96.;
pub const HEIGHT: f32 = 20.;

/// What a recording does with a keystroke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Take {
    /// Escape alone: recording ends, nothing is bound.
    Cancel,
    /// A key to bind, as chord text (`⇧⌘K`).
    Key(String),
    /// A key VizDAW cannot bind: recording goes on.
    Ignore,
}

/// What recording does with `key` on this platform.
pub fn take(key: &Keystroke) -> Take {
    take_for(key, cfg!(target_os = "macos"))
}

/// [`take`] for macOS (`mac`) or elsewhere. Elsewhere ⌘ is Ctrl (as
/// `secondary!` and `menus::keystroke` make it) and the Super key is
/// the desktop's, so a chord with it is ignored.
pub fn take_for(key: &Keystroke, mac: bool) -> Take {
    let m = key.modifiers;
    if key.key == "escape" && !(m.shift || m.control || m.alt || m.platform) {
        return Take::Cancel;
    }
    let mut mods = 0u8;
    if m.shift {
        mods |= SHIFT;
    }
    if m.alt {
        mods |= OPTION;
    }
    if mac {
        if m.control {
            mods |= CONTROL;
        }
        if m.platform {
            mods |= COMMAND;
        }
    } else {
        if m.control {
            mods |= COMMAND;
        }
        if m.platform {
            return Take::Ignore;
        }
    }
    let Some(name) = registry_key(&key.key) else {
        return Take::Ignore;
    };
    match Chord::of_slot(&name, mods) {
        Some(c) => Take::Key(c.text()),
        None => Take::Ignore,
    }
}

/// The registry's name of a GPUI key (the inverse of
/// `menus::gpui_key`): the main-keys Return, never the keypad's Enter.
fn registry_key(key: &str) -> Option<String> {
    let name = match key {
        "enter" => "Return",
        "backspace" => "Delete",
        "delete" => "ForwardDelete",
        // Modifier names reach the interceptor when one is let go alone.
        "shift" | "control" | "alt" | "platform" | "function" => return None,
        k => k,
    };
    bindings::Chord::of_slot(name, 0).map(|_| name.to_owned())
}

/// The recorder button of the command titled `title`: `recording` shows
/// "Press a key…"; a press calls `start` (the owner stops the recording
/// and listens for the key).
pub fn key_recorder(
    title: &str,
    recording: bool,
    scope: &mut RegistryScope,
    start: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let t = tokens();
    let label = format!("Record Key for {title}");
    let face = if recording { RECORDING } else { IDLE };
    let id = ElementId::Name(format!("key-recorder:{title}").into());
    div()
        .relative()
        .flex_none()
        .w(px(WIDTH))
        .h(px(HEIGHT))
        .child(
            div()
                .id(id)
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .rounded(px(t.radius))
                .border_1()
                .border_color(rgb(if recording {
                    t.state_record
                } else {
                    t.surface_separator
                }))
                .text_size(px(11.))
                .text_color(rgb(t.text_primary))
                .child(face)
                .on_click(move |_, window, cx| start(window, cx)),
        )
        .child(scope.control_with("button", &label, true, Some(face), false))
        .into_any_element()
}
