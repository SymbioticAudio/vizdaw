//! Settings › Keys (Swift `KeysSettingsView.swift`, 720×520): the number-
//! row switch, the message line (with Replace / Cancel for a key another
//! command has), every command by menu with its keys, a recorder and
//! Reset, and Save… / Load… / Reset All. Learn MIDI is not part of this
//! version.
//!
//! A change goes to the app's bindings ([`vizdaw_app::key_prefs`], saved in
//! the preferences) and then [`menus::rebind`] binds the window keys and
//! menus again. The recorder (see [`key_recorder`]) listens through one
//! keystroke interceptor the window installs when it opens, so a ⌘ key
//! reaches it before the bindings and menus do.

use core_control::bindings::{Chord, default_chords};
use core_control::registry::KeyContext;
use gpui::{
    AnyElement, App, Context, InteractiveElement, IntoElement, Keystroke, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use vizdaw_app::Prefs;
use vizdaw_app::key_prefs::KeyBindings;
use vizdaw_app::panels::{FileType, PanelKind, PanelSpec, StartFolder};

use super::{SettingsWindow, caption, switch, text};
use crate::app_state::AppState;
use crate::controls::key_recorder::{self, Take, key_recorder};
use crate::file_panels::{self, PanelPlace};
use crate::menus;
use crate::registry::RegistryScope;
use crate::strip_palette::tokens;

/// The Keys tab's content height (Swift's frame, 720×520).
pub const KEYS_HEIGHT: f32 = 520.;
/// Its width.
pub const KEYS_WIDTH: f32 = 720.;

/// The number-row switch's label (Swift `KeysSettingsView.swift:36`).
pub const RESERVE_LABEL: &str = "Number keys always go to locators (step entry uses the keypad)";

/// Swift's `NSSavePanel`: the name suggested, no message, no type.
pub const SAVE_KEYS: PanelSpec = PanelSpec {
    row: "keys-save",
    kind: PanelKind::Save,
    prompt: "Save",
    message: None,
    types: &[],
    multiple: false,
    can_create_folders: true,
    start: StartFolder::System,
    default_name: Some("Key Bindings.vizkeys"),
    save_extension: None,
};

/// Swift's `NSOpenPanel`: one file, no folders.
pub const LOAD_KEYS: PanelSpec = PanelSpec {
    row: "keys-load",
    kind: PanelKind::Open,
    prompt: "Open",
    message: None,
    types: &[] as &[FileType],
    multiple: false,
    can_create_folders: false,
    start: StartFolder::System,
    default_name: None,
    save_extension: None,
};

/// One command's row.
struct Row {
    id: &'static str,
    title: &'static str,
    step_entry: bool,
    keys: Vec<String>,
    changed: bool,
}

/// The rows by menu, the menus in the order they first appear.
fn groups(keymap: &core_control::bindings::Keymap) -> Vec<(&'static str, Vec<Row>)> {
    let mut out: Vec<(&'static str, Vec<Row>)> = vec![];
    for c in keymap.table() {
        let keys: Vec<String> = keymap.chords(c.id).iter().map(Chord::text).collect();
        let defaults: Vec<String> = default_chords(c).iter().map(Chord::text).collect();
        let row = Row {
            id: c.id,
            title: c.title,
            step_entry: c.context == KeyContext::StepEntry,
            changed: keys != defaults,
            keys,
        };
        match out.iter_mut().find(|(m, _)| *m == c.menu) {
            Some((_, rows)) => rows.push(row),
            None => out.push((c.menu, vec![row])),
        }
    }
    out
}

/// Changes the bindings, then binds the keys and menus again (after the
/// update, from a deferred call: never inside the one that changed them).
fn edit(cx: &mut App, f: impl FnOnce(&mut KeyBindings, &mut Prefs)) {
    AppState::global(cx).update(cx, |s, cx| {
        f(&mut s.keys, &mut s.prefs);
        cx.notify();
    });
    cx.defer(menus::rebind);
}

/// A push button: `button` in the tree; off, it takes no clicks.
pub(super) fn button(
    label: &str,
    enabled: bool,
    scope: &mut RegistryScope,
    press: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let t = tokens();
    let mut face = div()
        .id(format!("keys-button:{label}"))
        .px_2()
        .py(px(2.))
        .rounded(px(t.radius))
        .border_1()
        .border_color(rgb(t.surface_separator))
        .bg(rgb(t.surface_inset))
        .text_color(rgb(t.text_primary))
        .opacity(if enabled { 1. } else { 0.45 })
        .child(label.to_owned());
    if enabled {
        face = face
            .cursor_pointer()
            .on_click(move |_, window, cx| press(window, cx));
    }
    div()
        .relative()
        .flex_none()
        .child(face)
        .child(scope.control("button", label, enabled))
        .into_any_element()
}

/// A text line with a value in the tree and a colour of its own.
fn tagged(line: &str, value: Option<&str>, colour: u32, scope: &mut RegistryScope) -> AnyElement {
    div()
        .relative()
        .text_color(rgb(colour))
        .child(line.to_owned())
        .child(scope.control_with("text", line, true, value, false))
        .into_any_element()
}

impl SettingsWindow {
    /// Listens for the recorder's keys for as long as the window lives
    /// (called once, as it opens).
    pub(super) fn watch_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let me = cx.entity().downgrade();
        let id = window.window_handle().window_id();
        self.capture = Some(cx.intercept_keystrokes(move |e, window, cx| {
            if window.window_handle().window_id() != id {
                return;
            }
            let Some(this) = me.upgrade() else { return };
            if this.read(cx).recording.is_none() {
                return;
            }
            // Before any binding or menu: the key is the recorder's.
            cx.stop_propagation();
            let key = e.keystroke.clone();
            this.update(cx, |t, cx| t.take_key(&key, cx));
        }));
        // Losing the keyboard stops recording (Swift `resignFirstResponder`).
        self.activation = Some(cx.observe_window_activation(window, |t, window, cx| {
            if !window.is_window_active() {
                t.stop_recording(cx);
            }
        }));
    }

    pub(super) fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording.take().is_some() {
            cx.notify();
        }
    }

    /// What recording does with a key pressed while it is on.
    fn take_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        match key_recorder::take(key) {
            Take::Ignore => {}
            Take::Cancel => self.stop_recording(cx),
            Take::Key(text) => {
                if let Some(id) = self.recording.take() {
                    edit(cx, move |k, p| k.record(id, &text, p));
                }
                cx.notify();
            }
        }
    }

    /// Save…: the bindings go to the file chosen; a failure is the message
    /// line.
    fn save_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = file_panels::choose_save(
            &SAVE_KEYS,
            PanelPlace::default(),
            window.window_handle(),
            cx,
        );
        cx.spawn(async move |_, cx| {
            let answer = chosen.await;
            cx.update(|cx| match answer {
                Ok(Some(path)) => AppState::global(cx).update(cx, |s, cx| {
                    if let Err(e) = s.keys.save(&path) {
                        s.keys.set_message(e);
                    }
                    cx.notify();
                }),
                Ok(None) => {}
                Err(e) => AppState::global(cx).update(cx, |s, cx| {
                    s.keys.set_message(e);
                    cx.notify();
                }),
            });
        })
        .detach();
    }

    /// Load…: the bindings come from the file chosen; what it skipped, or
    /// why it failed, is the message line.
    fn load_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = file_panels::choose(
            &LOAD_KEYS,
            PanelPlace::default(),
            Some(window.window_handle()),
            cx,
        );
        cx.spawn(async move |_, cx| {
            let answer = chosen.await;
            cx.update(|cx| match answer {
                Ok(Some(paths)) => {
                    if let Some(path) = paths.into_iter().next() {
                        edit(cx, move |k, p| {
                            // A refusal is already the message.
                            let _ = k.load_file(&path, p);
                        });
                    }
                }
                Ok(None) => {}
                Err(e) => AppState::global(cx).update(cx, |s, cx| {
                    s.keys.set_message(e);
                    cx.notify();
                }),
            });
        })
        .detach();
    }

    pub(super) fn keys_tab(
        &mut self,
        scope: &mut RegistryScope,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = tokens();
        let (groups, message, pending, reserve) = {
            let s = AppState::global(cx).read(cx);
            (
                groups(s.keys.keymap()),
                s.keys.message().map(str::to_owned),
                s.keys.pending().is_some(),
                s.keys.reserve_number_row(&s.prefs),
            )
        };
        let me = cx.entity().downgrade();

        let toggle = switch(
            "settings-reserve-keys",
            RESERVE_LABEL,
            reserve,
            true,
            scope,
            {
                move |_, cx| {
                    AppState::global(cx).update(cx, |s, cx| {
                        s.keys.set_reserve_number_row(!reserve, &mut s.prefs);
                        cx.notify();
                    });
                }
            },
        );

        // The message line, with Replace and Cancel for a key another
        // command has.
        let message_line = message.map(|m| {
            let mut line = div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().child(caption(&m, scope)));
            if pending {
                line = line
                    .child(button("Replace", true, scope, |_, cx| {
                        edit(cx, |k, p| k.confirm_replace(p));
                    }))
                    .child(button("Cancel", true, scope, |_, cx| {
                        edit(cx, |k, _| k.cancel_replace());
                    }));
            }
            line.into_any_element()
        });

        // The list.
        let mut list = scope.container();
        let mut items: Vec<AnyElement> = vec![];
        for (menu, rows) in groups {
            items.push(
                div()
                    .mt_2()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(tagged(menu, Some("section"), t.text_primary, &mut list))
                    .into_any_element(),
            );
            for row in rows {
                let recording = self.recording == Some(row.id);
                let id = row.id;
                let starter = me.clone();
                let recorder = key_recorder(row.title, recording, &mut list, move |_, cx| {
                    starter
                        .update(cx, |t, cx| {
                            t.recording = Some(id);
                            cx.notify();
                        })
                        .ok();
                });
                let reset = button("Reset", row.changed, &mut list, move |_, cx| {
                    edit(cx, move |k, p| k.reset(id, p));
                });
                let mut name = div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(text(row.title, &mut list));
                if row.step_entry {
                    name = name.child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(t.text_secondary))
                            .child(tagged(
                                "step entry",
                                Some("step"),
                                t.text_secondary,
                                &mut list,
                            )),
                    );
                }
                let keys = row.keys.join("  ");
                let colour = if row.changed {
                    t.accent
                } else {
                    t.text_primary
                };
                items.push(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(name)
                        .child(tagged(
                            &keys,
                            row.changed.then_some("changed"),
                            colour,
                            &mut list,
                        ))
                        .child(recorder)
                        .child(reset)
                        .into_any_element(),
                );
            }
        }
        let list_el = div()
            .id("settings-keys-list")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .border_1()
            .border_color(rgb(t.surface_separator))
            .bg(rgb(t.surface_inset))
            .child(div().p_2().flex().flex_col().gap(px(4.)).children(items))
            .child(list.clip());

        let save_me = me.clone();
        let load_me = me.clone();
        let bottom = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(button("Save…", true, scope, move |window, cx| {
                save_me.update(cx, |t, cx| t.save_keys(window, cx)).ok();
            }))
            .child(button("Load…", true, scope, move |window, cx| {
                load_me.update(cx, |t, cx| t.load_keys(window, cx)).ok();
            }))
            .child(div().flex_1())
            .child(button("Reset All", true, scope, |_, cx| {
                edit(cx, |k, p| k.reset_all(p));
            }));

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p_4()
            .child(toggle)
            .children(message_line)
            .child(list_el)
            .child(bottom)
            .into_any_element()
    }
}
