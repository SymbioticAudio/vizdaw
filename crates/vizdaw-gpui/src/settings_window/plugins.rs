//! Settings › Plug-ins (Swift `PluginSettingsView.swift`, 500 wide): "Put
//! an EQ on new channels", the picker of which EQ that is, and "Install
//! Multi-Q for other apps". Everything lives in the app's
//! [`PluginPrefs`]; this view only reads it. The installed effects are
//! read off the UI thread when the tab shows, and the install (a bundle
//! copy) runs off it too; something already in the way asks "Replace the
//! … in your Components folder?" before replacing it.
//!
//! [`PluginPrefs`]: vizdaw_app::plugin_prefs::PluginPrefs

use std::rc::Rc;

use gpui::{
    AnyElement, AnyWindowHandle, App, Context, IntoElement, ParentElement, Styled, Window, div, px,
    rgb,
};
use vizdaw_app::plugin_prefs::{InstallOutcome, NO_BUNDLED_EQ, PluginPrefs};
use vizdaw_app::session::CoreCalls;
use vizdaw_ffi::bundled::DefaultEqRow;
use vizdaw_ffi::snapshot::PluginFormatTag;

use super::keys::button;
use super::{
    AudioMenu, Choice, Pick, SettingsWindow, caption, form_row, read_off_thread, switch, text,
};
use crate::app_state::AppState;
use crate::registry::RegistryScope;
use crate::sheet_host::Ask;
use crate::strip_palette::tokens;

/// The switch's label (and its accessibility label).
const SWITCH: &str = "Put an EQ on new channels";
/// The picker's label.
const PICKER: &str = "EQ for new channels";
/// What the replace question says under its title.
const REPLACE_DETAIL: &str = "Something is already installed under that name.";

fn format_label(f: PluginFormatTag) -> &'static str {
    match f {
        PluginFormatTag::Au => "AU",
        PluginFormatTag::Clap => "CLAP",
        PluginFormatTag::Vst3 => "VST3",
    }
}

/// Changes the plug-in settings with `f` (core first, saved after).
fn edit(cx: &mut App, f: impl FnOnce(&mut PluginPrefs, &mut vizdaw_app::Prefs, &dyn CoreCalls)) {
    AppState::global(cx).update(cx, |s, cx| {
        let core = s.store.core();
        f(&mut s.plugins, &mut s.prefs, core.as_ref());
        cx.notify();
    });
}

fn pick_eq(choice: Option<DefaultEqRow>) -> Pick {
    Rc::new(move |_, cx| {
        let choice = choice.clone();
        edit(cx, move |p, prefs, core| {
            let _ = p.set_default_eq(choice, prefs, core);
        });
    })
}

/// The picker's items: the built-in EQ, each installed effect, and the
/// chosen one when it is missing.
fn eq_items(p: &PluginPrefs, read: bool) -> Vec<Choice> {
    let chosen = p.default_eq();
    let mut items = vec![];
    if let Some(name) = p.bundled_eq_name() {
        items.push(Choice {
            title: format!("{name} (built in)"),
            checked: chosen.is_none(),
            pick: pick_eq(None),
        });
    }
    for c in p.eq_choices() {
        items.push(Choice {
            title: format!("{} ({})", c.name, format_label(c.format)),
            checked: chosen.is_some_and(|d| d.format == c.format && d.id == c.id),
            pick: pick_eq(Some(c.clone())),
        });
    }
    // Before the first read the choice is shown as itself; after it, a
    // choice nobody offers is not available.
    if let Some(c) = chosen
        && !p
            .eq_choices()
            .iter()
            .any(|r| r.format == c.format && r.id == c.id)
    {
        let tail = if read {
            "not available"
        } else {
            format_label(c.format)
        };
        items.push(Choice {
            title: format!("{} ({tail})", c.name),
            checked: true,
            pick: pick_eq(Some(c.clone())),
        });
    }
    items
}

impl SettingsWindow {
    /// The tab shows: the installed effects are read again, off the UI
    /// thread; only the newest read lands.
    pub(super) fn read_eqs(&mut self, cx: &mut Context<Self>) {
        self.eq_gen += 1;
        let generation = self.eq_gen;
        let core = AppState::global(cx).read(cx).store.core();
        read_off_thread(
            cx,
            move || PluginPrefs::read(core.as_ref()),
            move |t, read, cx| {
                let Some(read) = read else { return };
                if t.eq_gen != generation {
                    return;
                }
                t.eqs_read = true;
                AppState::global(cx).update(cx, |s, cx| {
                    s.plugins.apply_read(read);
                    cx.notify();
                });
                cx.notify();
            },
        );
    }

    /// The Install button (and the Replace answer): the copy runs off the
    /// UI thread; something in the way asks before replacing it.
    fn install_multiq(&mut self, replace: bool, window: AnyWindowHandle, cx: &mut Context<Self>) {
        if self.installing {
            return;
        }
        let (dest, core, name) = {
            let s = AppState::global(cx).read(cx);
            if !s.plugins.can_install() {
                return;
            }
            (
                s.plugins.install_destination().map(str::to_owned),
                s.store.core(),
                s.plugins.bundled_eq_name().map(str::to_owned),
            )
        };
        self.installing = true;
        read_off_thread(
            cx,
            move || core.install_multiq(dest, replace),
            move |t, result, cx| {
                t.installing = false;
                let Some(result) = result else { return };
                let outcome = AppState::global(cx).update(cx, |s, cx| {
                    let outcome = s.plugins.finish_install(result);
                    cx.notify();
                    outcome
                });
                cx.notify();
                if outcome == InstallOutcome::AskReplace {
                    t.ask_replace(name.as_deref().unwrap_or("the bundled EQ"), window, cx);
                }
            },
        );
    }

    /// "Replace the … in your Components folder?"
    fn ask_replace(&mut self, name: &str, window: AnyWindowHandle, cx: &mut Context<Self>) {
        let ask = Ask::replace(
            &format!("Replace the {name} in your Components folder?"),
            Some(REPLACE_DETAIL),
        );
        let answer = crate::alerts::ask(window, ask.clone(), cx);
        cx.spawn(async move |this, cx| {
            if let Ok(i) = answer.await
                && ask.replaces(i)
            {
                this.update(cx, |t, cx| t.install_multiq(true, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub(super) fn plugins_tab(
        &mut self,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (
            items,
            face,
            on,
            switch_enabled,
            bundled,
            can_install,
            note,
            caption_text,
            status,
            problem,
        ) = {
            let p = &AppState::global(cx).read(cx).plugins;
            let items = eq_items(p, self.eqs_read);
            let face = items
                .iter()
                .find(|c| c.checked)
                .map_or_else(|| "Multi-Q".to_owned(), |c| c.title.clone());
            (
                items,
                face,
                p.new_channel_eq(),
                p.bundled_eq_name().is_some() || !p.eq_choices().is_empty(),
                p.bundled_eq_name().map(str::to_owned),
                p.can_install(),
                p.note().map(str::to_owned),
                format!(
                    "New channels, new buses and the master of new projects start with {} in their EQ slot. Projects you already have are not changed.",
                    p.default_eq_name()
                ),
                p.status().map(str::to_owned),
                p.problem().map(str::to_owned),
            )
        };
        let t = tokens();
        let eq_name = bundled.as_deref().unwrap_or("the bundled EQ");

        let toggle = switch(
            "settings-new-channel-eq",
            SWITCH,
            on,
            switch_enabled,
            scope,
            move |_, cx| {
                edit(cx, move |p, prefs, core| {
                    let _ = p.set_new_channel_eq(!on, prefs, core);
                });
            },
        );
        let picker =
            self.picker_enabled(PICKER, &face, AudioMenu::Eq, items, on, scope, window, cx);

        let me = cx.entity().downgrade();
        let handle = window.window_handle();
        let install = button(
            &format!("Install {eq_name} for other apps"),
            can_install && !self.installing,
            scope,
            move |_, cx| {
                me.update(cx, |t, cx| t.install_multiq(false, handle, cx))
                    .ok();
            },
        );

        let mut body = div().flex().flex_col().gap(px(6.)).p_4();
        body = body.child(toggle).child(form_row(PICKER, picker, scope));
        if let Some(n) = note {
            body = body.child(
                div()
                    .text_color(rgb(t.state_warning))
                    .child(text(&n, scope)),
            );
        }
        body = body
            .child(caption(&caption_text, scope))
            .child(div().flex().flex_row().child(install))
            .child(caption(
                "Copies the Audio Unit to your Components folder, so projects you take to other apps keep their EQ settings.",
                scope,
            ));
        if bundled.is_none() {
            body = body.child(
                div()
                    .text_color(rgb(t.state_warning))
                    .child(text(NO_BUNDLED_EQ, scope)),
            );
        }
        if let Some(s) = status {
            body = body.child(div().text_color(rgb(t.text_primary)).child(text(&s, scope)));
        }
        if let Some(p) = Self::problem_line(problem, scope) {
            body = body.child(p);
        }
        body.into_any_element()
    }
}
