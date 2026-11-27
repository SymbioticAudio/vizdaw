//! A waiting MIDI learn in the windows (Swift `LearnBanner`, `LearnItem`
//! and `KeyMonitor`'s Esc; slice 9d Task 5): the banner with its Cancel
//! at the top of the mixer (and in Settings › Keys), the "MIDI Learn" item
//! of a control's menu, and Esc, which cancels the waiting learn from a
//! document, mixer, plug-in or Settings window.

use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, Window, div, px, rgb,
};
use vizdaw_app::DocId;

use crate::app_state::AppState;
use crate::arrangement::with_alpha;
use crate::plugin_windows::PluginWindowView;
use crate::registry::RegistryScope;
use crate::settings_window::SettingsWindow;
use crate::shell::DocumentWindow;
use crate::strip_palette::tokens;

/// A control menu's item (`S/MixerView.swift:349`).
pub const MIDI_LEARN: &str = "MIDI Learn";
/// The banner's button (`S/LearnMapsSheet.swift:93`).
pub const CANCEL: &str = "Cancel";
/// The banner's tint: the accent at 15% (`S/LearnMapsSheet.swift:98`).
const TINT: f32 = 0.15;

/// The banner's text for `doc`, naming its project when two or more are
/// open (`S/LearnMapsSheet.swift:105-108`); `None` unless its learn waits.
pub fn banner_text(doc: DocId, cx: &App) -> Option<String> {
    let store = &AppState::global(cx).read(cx).store;
    let d = store.document(doc)?;
    let project = (store.count() >= 2).then(|| d.title());
    d.learn.banner_text(project.as_deref())
}

/// "Waiting for MIDI… move a control (Esc cancels)" and a small Cancel
/// while `doc`'s learn waits (`S/LearnMapsSheet.swift:81-101`); `None`
/// otherwise, when nothing is drawn or recorded.
pub fn learn_banner(doc: DocId, scope: &mut RegistryScope, cx: &App) -> Option<AnyElement> {
    let text = banner_text(doc, cx)?;
    let t = tokens();
    Some(
        div()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(8.))
            .py(px(4.))
            .bg(with_alpha(t.accent, TINT))
            .text_xs()
            .text_color(rgb(t.text_primary))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(text.clone())
                    .child(scope.control("text", &text, true)),
            )
            .child(cancel_button(doc, scope))
            .into_any_element(),
    )
}

/// The banner's small Cancel: ends `doc`'s learn.
pub(crate) fn cancel_button(doc: DocId, scope: &mut RegistryScope) -> AnyElement {
    let t = tokens();
    div()
        .id("learn-cancel")
        .relative()
        .flex_none()
        .h(px(20.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(t.radius))
        .border_1()
        .border_color(rgb(t.surface_separator))
        .bg(rgb(t.surface_inset))
        .text_color(rgb(t.text_primary))
        .cursor_pointer()
        .whitespace_nowrap()
        .child(CANCEL)
        .child(scope.control("button", CANCEL, true))
        .on_click(move |_, _, cx| cancel(Some(doc), cx))
        .into_any_element()
}

/// Ends the waiting learn: `doc`'s, or (`None`) whichever document's
/// waits. The shared owner is cleared through the document that owns it,
/// so a cancel from any window leaves no learn recorded as waiting.
pub fn cancel(doc: Option<DocId>, cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| {
        let Some(owner) = s.store.learning_document() else {
            return;
        };
        if doc.is_some_and(|d| d != owner) {
            return;
        }
        let core = s.store.core();
        s.store
            .with_document(owner, |d, _| d.cancel_learn(core.as_ref()));
        cx.notify();
    });
}

/// Whether Esc in `window` cancels a waiting learn
/// (`S/KeyMonitor.swift:155-164`): a bare Esc in a document, mixer,
/// plug-in or Settings window with no sheet or file panel up, no text
/// field typed in and the Keys recorder not recording, while a learn
/// waits.
fn esc_cancels(key: &gpui::Keystroke, window: &Window, cx: &App) -> bool {
    if key.key != "escape" || key.modifiers.modified() || crate::file_panels::is_up(cx) {
        return false;
    }
    // An alert or question up in the window has the key (review M1).
    let handle = window.window_handle();
    if window.has_active_prompt() || crate::alerts::showing(cx).iter().any(|(w, _)| *w == handle) {
        return false;
    }
    let free = if let Some(Some(root)) = window.root::<DocumentWindow>() {
        let v = root.read(cx);
        let doc = v.doc();
        v.sheet_label().is_none()
            && AppState::global(cx)
                .read(cx)
                .store
                .document(doc)
                .is_none_or(|d| d.transport_keys_enabled())
    } else if let Some(Some(root)) = window.root::<SettingsWindow>() {
        // The recorder's Esc is its own (it cancels recording).
        let v = root.read(cx);
        v.sheet_label().is_none() && !v.is_recording()
    } else {
        matches!(window.root::<PluginWindowView>(), Some(Some(_)))
    };
    free && AppState::global(cx)
        .read(cx)
        .store
        .learning_document()
        .is_some()
}

/// Esc cancels a waiting learn ([`esc_cancels`]), before any binding or
/// key handler sees it, once no menu or popover is left for it to close;
/// otherwise it goes on as before.
pub fn install(cx: &mut App) {
    cx.intercept_keystrokes(|e, window, cx| {
        if !esc_cancels(&e.keystroke, window, cx) {
            return;
        }
        cx.stop_propagation();
        // An open menu or popover, or a held roll key, takes this Esc
        // first (review M2); the next one cancels the learn.
        if let Some(Some(root)) = window.root::<DocumentWindow>()
            && root.update(cx, |v, cx| v.escape_menus(window, cx))
        {
            return;
        }
        cancel(None, cx);
    })
    .detach();
}
