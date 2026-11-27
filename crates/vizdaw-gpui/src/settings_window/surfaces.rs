//! Settings › Surfaces (Swift `SurfacesSettingsView.swift`, 560 wide):
//! the configured units with their connection state and why a silent one
//! is not answering, Remove, the lines that name no unit, "Add Surface…"
//! (the [`AddSurfaceSheet`]), and the commands learned to MIDI controls.
//! Everything lives in the app's [`SurfaceSettings`]; the tab reads the
//! core every 500 ms (`:71-77`) off the UI thread, one read at a time, and
//! redraws only when the read changed something. A refusal is an alert on
//! the Settings window (`:83-90`).
//!
//! [`SurfaceSettings`]: vizdaw_app::surface_settings::SurfaceSettings

use std::time::Duration;

use gpui::{
    AnyElement, AnyWindowHandle, App, Context, ElementId, InteractiveElement, IntoElement,
    ParentElement, PromptLevel, StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use vizdaw_app::AlertItem;
use vizdaw_app::surface_settings::{NO_COMMAND_MAPS, NO_SURFACES, SurfaceRead, row_title, status};

use super::{SettingsWindow, caption, read_off_thread, section, text};
use crate::app_state::AppState;
use crate::registry::RegistryScope;
use crate::sheet_host::Ask;
use crate::sheets::add_surface::AddSurfaceSheet;
use crate::strip_palette::tokens;

/// The tab's width (`S/SurfacesSettingsView.swift:69`).
pub const SURFACES_WIDTH: f32 = 560.;
/// Its content height: the list scrolls past it.
pub const SURFACES_HEIGHT: f32 = 420.;
/// The poll's pace (`:71-77`).
const SURFACES_EVERY: Duration = Duration::from_millis(500);
/// The button under the list (`:51`).
const ADD: &str = "Add Surface\u{2026}";
const REMOVE: &str = "Remove";

/// A small push button: `button` in the tree, `label` its label.
fn button(
    id: ElementId,
    label: &str,
    scope: &mut RegistryScope,
    press: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let t = tokens();
    div()
        .relative()
        .flex_none()
        .child(
            div()
                .id(id)
                .px_2()
                .py(px(2.))
                .rounded(px(t.radius))
                .border_1()
                .border_color(rgb(t.surface_separator))
                .bg(rgb(t.surface_inset))
                .text_color(rgb(t.text_primary))
                .cursor_pointer()
                .child(label.to_owned())
                .on_click(move |_, window, cx| press(window, cx)),
        )
        .child(scope.control("button", label, true))
        .into_any_element()
}

/// A line in `colour`, in the tree as a `text`.
fn line(t: &str, colour: u32, scope: &mut RegistryScope) -> AnyElement {
    div()
        .text_color(rgb(colour))
        .child(text(t, scope))
        .into_any_element()
}

/// One unit's row as the tab draws it.
struct UnitRow {
    id: u32,
    title: String,
    ports: String,
    status: &'static str,
    answering: bool,
    problem: Option<String>,
}

impl SettingsWindow {
    /// Starts the 2 Hz poll (the tab shows); dropping it stops it.
    pub(super) fn poll_surfaces(&mut self, cx: &mut Context<Self>) {
        self.surfaces_poll = Some(cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |t, cx| t.read_surfaces(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(SURFACES_EVERY).await;
            }
        }));
    }

    /// Reads the core off the UI thread (the port list is a CoreMIDI
    /// listing on a live Mac); a read still out holds off the next. An
    /// unchanged read redraws nothing.
    fn read_surfaces(&mut self, cx: &mut Context<Self>) {
        if self.surfaces_reading {
            return;
        }
        self.surfaces_reading = true;
        let generation = self.surfaces_gen;
        let core = AppState::global(cx).read(cx).store.core();
        read_off_thread(
            cx,
            move || SurfaceRead::read(core.as_ref()),
            move |t, read, cx| {
                // Whatever the generation, the read is over.
                t.surfaces_reading = false;
                if t.surfaces_gen != generation {
                    return;
                }
                // A read that panicked leaves what was shown.
                let Some(read) = read else { return };
                let changed = AppState::global(cx).update(cx, |s, _| s.surfaces.apply_read(read));
                if changed {
                    cx.notify();
                }
            },
        );
    }

    /// "Add Surface…": the sheet, with the first ports picked.
    fn begin_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sheet.is_up() {
            return;
        }
        AppState::global(cx).update(cx, |s, _| {
            let core = s.store.core();
            s.surfaces.begin_add(core.as_ref());
        });
        let sheet = AddSurfaceSheet::new(cx);
        if self.show_sheet(Box::new(sheet), window, cx).is_err() {
            AppState::global(cx).update(cx, |s, _| s.surfaces.cancel_add());
        }
        cx.notify();
    }

    /// Removes unit `id`; the pickers mark their ports again and the
    /// surface tick follows (as the channel's `surface.remove`).
    fn remove_surface(&mut self, id: u32, window: AnyWindowHandle, cx: &mut Context<Self>) {
        let alert = AppState::global(cx).update(cx, |s, cx| {
            let core = s.store.core();
            s.surfaces.remove(id, core.as_ref());
            let alert = s.surfaces.take_alert();
            if alert.is_none() {
                s.store.refresh_claimed_ports();
            }
            cx.notify();
            alert
        });
        self.alert(alert, window, cx);
    }

    /// Removes the MIDI controls learned to `command`.
    fn remove_command_map(
        &mut self,
        command: &str,
        window: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        let alert = AppState::global(cx).update(cx, |s, cx| {
            let core = s.store.core();
            s.surfaces.remove_command_map(command, core.as_ref());
            cx.notify();
            s.surfaces.take_alert()
        });
        self.alert(alert, window, cx);
    }

    /// A refusal: its title, its message and OK, on this window.
    pub(super) fn alert(
        &mut self,
        alert: Option<AlertItem>,
        window: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
        let Some(a) = alert else { return };
        let answer = crate::alerts::ask(
            window,
            Ask::alert(PromptLevel::Warning, &a.title, &a.message),
            cx,
        );
        // Held until answered (a dropped answer takes the ask away).
        cx.spawn(async move |_, _| {
            let _ = answer.await;
        })
        .detach();
    }

    pub(super) fn surfaces_tab(
        &mut self,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (units, others, maps) = {
            let st = &AppState::global(cx).read(cx).surfaces;
            let units: Vec<UnitRow> = st
                .rows()
                .iter()
                .map(|r| UnitRow {
                    id: r.id,
                    title: row_title(r),
                    ports: format!("{} \u{2192} {}", r.input, r.output),
                    status: status(r),
                    answering: r.connected && r.online,
                    problem: st.problem_for(r.id).map(str::to_owned),
                })
                .collect();
            let others: Vec<String> = st.other_problems().into_iter().map(str::to_owned).collect();
            let maps: Vec<(String, String)> = st
                .command_maps()
                .iter()
                .map(|m| (m.text.clone(), m.command.clone()))
                .collect();
            (units, others, maps)
        };
        let t = tokens();
        let me = cx.entity().downgrade();
        let handle = window.window_handle();

        let mut body = div()
            .id("settings-surfaces")
            .flex()
            .flex_col()
            .gap(px(6.))
            .p_4()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(section("Surfaces", scope));
        if units.is_empty() {
            body = body.child(line(NO_SURFACES, t.text_secondary, scope));
        }
        for u in units {
            let mut left = scope.container();
            let names = div()
                .flex()
                .flex_col()
                .child(text(&u.title, &mut left))
                .child(caption(&u.ports, &mut left));
            let mut right = scope.container();
            let colour = if u.answering {
                t.text_primary
            } else {
                t.text_secondary
            };
            let mut state = div()
                .flex()
                .flex_col()
                .items_end()
                .child(line(u.status, colour, &mut right));
            if let Some(why) = &u.problem {
                state = state.child(div().text_size(px(11.)).text_right().child(line(
                    why,
                    t.state_warning,
                    &mut right,
                )));
            }
            let (me, id) = (me.clone(), u.id);
            let remove = button(
                ElementId::NamedInteger("surfaces-remove".into(), u64::from(u.id)),
                REMOVE,
                scope,
                move |_, cx| {
                    me.update(cx, |t, cx| t.remove_surface(id, handle, cx)).ok();
                },
            );
            body = body.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(names)
                    .child(div().flex_1())
                    .child(state)
                    .child(remove),
            );
        }
        for l in &others {
            body = body.child(
                div()
                    .text_size(px(11.))
                    .child(line(l, t.state_warning, scope)),
            );
        }
        let adding = me.clone();
        body = body.child(div().flex().flex_row().child(button(
            "surfaces-add".into(),
            ADD,
            scope,
            move |window, cx| {
                adding.update(cx, |t, cx| t.begin_add(window, cx)).ok();
            },
        )));

        body = body.child(section("Commands Learned to MIDI", scope));
        if maps.is_empty() {
            body = body.child(line(NO_COMMAND_MAPS, t.text_secondary, scope));
        }
        for (i, (source, command)) in maps.into_iter().enumerate() {
            // Its title in the registry, or its id (`:61`).
            let title = core_control::registry::find(&command)
                .map_or_else(|| command.clone(), |c| c.title.to_owned());
            let me = me.clone();
            let remove = button(
                ElementId::NamedInteger("surfaces-unlearn".into(), i as u64),
                REMOVE,
                scope,
                move |_, cx| {
                    me.update(cx, |t, cx| t.remove_command_map(&command, handle, cx))
                        .ok();
                },
            );
            body = body.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(text(&source, scope))
                    .child(div().flex_1())
                    .child(line(&title, t.text_secondary, scope))
                    .child(remove),
            );
        }
        body.into_any_element()
    }
}
