//! The Add Surface sheet (Swift `SurfacesSettingsView.swift:105-155`, 420
//! wide), over the Settings window: the Type, Input and Output pickers,
//! the keyboard's note and help, the XT's position stepper and help, and
//! the core's refusal, which keeps the sheet open. Cancel (Esc) and Add
//! (Return); Add is off while the input or the output is empty. The draft
//! lives in the app's [`SurfaceSettings`].
//!
//! [`SurfaceSettings`]: vizdaw_app::surface_settings::SurfaceSettings

use gpui::{
    AnyElement, App, AppContext, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, px, rgb,
};
use vizdaw_app::surface_settings::{
    KONTROL_HELP, NO_DESTINATIONS, NO_SOURCES, SurfaceDraft, XT_HELP, kind_title,
};
use vizdaw_ffi::surfaces::SurfaceKindRow;

use crate::app_state::AppState;
use crate::controls::menu_face;
use crate::controls::stepper::{Range, stepper_buttons};
use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton};
use crate::strip_palette::tokens;

/// The sheet's title (`:107`).
pub const TITLE: &str = "Add Surface";
pub const CANCEL: &str = "Cancel";
pub const ADD: &str = "Add";
/// `:154`.
const WIDTH: f32 = 420.;
/// A picker's face.
const PICKER_WIDTH: f32 = 260.;
/// The four types, in Swift's order (`:14`).
const KINDS: [SurfaceKindRow; 4] = [
    SurfaceKindRow::Mcu,
    SurfaceKindRow::McuXt,
    SurfaceKindRow::Hui,
    SurfaceKindRow::KontrolMk3,
];
/// The XT's position range (`:132`).
const POSITIONS: Range = Range {
    min: 0,
    max: 3,
    step: 1,
};

/// The sheet's pickers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Picker {
    Type,
    Input,
    Output,
}

impl Picker {
    fn label(self) -> &'static str {
        match self {
            Picker::Type => "Type",
            Picker::Input => "Input",
            Picker::Output => "Output",
        }
    }
}

/// One entry of an open picker.
struct Item {
    title: String,
    checked: bool,
    pick: Box<dyn Fn(&mut App)>,
}

/// The Add Surface sheet.
pub struct AddSurfaceSheet {
    /// The picker that is open.
    open: Entity<Option<Picker>>,
}

impl AddSurfaceSheet {
    pub fn new(cx: &mut App) -> Self {
        AddSurfaceSheet {
            open: cx.new(|_| None),
        }
    }
}

/// Changes the draft with `f`.
fn edit(
    cx: &mut App,
    f: impl FnOnce(&mut vizdaw_app::surface_settings::SurfaceSettings, &dyn vizdaw_app::CoreCalls),
) {
    AppState::global(cx).update(cx, |s, cx| {
        let core = s.store.core();
        f(&mut s.surfaces, core.as_ref());
        cx.notify();
    });
}

/// A line in `colour`, in the tree as a `text`.
fn line(t: &str, colour: u32, scope: &mut RegistryScope) -> gpui::Div {
    div()
        .relative()
        .text_color(rgb(colour))
        .child(t.to_owned())
        .child(scope.control("text", t, true))
}

/// A port picker's entries: the ports, or the one empty line (`:115-122`).
fn port_items(
    ports: &[String],
    chosen: &str,
    empty: &str,
    set: fn(&mut vizdaw_app::surface_settings::SurfaceSettings, String),
) -> Vec<Item> {
    let mut items: Vec<Item> = vec![];
    if ports.is_empty() {
        items.push(Item {
            title: empty.to_owned(),
            checked: chosen.is_empty(),
            pick: Box::new(move |cx| edit(cx, |s, _| set(s, String::new()))),
        });
    }
    for p in ports {
        let port = p.clone();
        items.push(Item {
            title: p.clone(),
            checked: *p == chosen,
            pick: Box::new(move |cx| {
                let port = port.clone();
                edit(cx, move |s, _| set(s, port));
            }),
        });
    }
    items
}

impl AddSurfaceSheet {
    /// A labelled picker: its face, and its list under it while open.
    fn picker(
        &self,
        which: Picker,
        face: &str,
        items: Vec<Item>,
        scope: &mut RegistryScope,
        cx: &App,
    ) -> AnyElement {
        let open = *self.open.read(cx) == Some(which);
        let toggling = self.open.clone();
        let label = which.label();
        let face_el = menu_face(label, face, None, true, scope, move |window, cx| {
            toggling.update(cx, |o, _| {
                *o = (*o != Some(which)).then_some(which);
            });
            window.refresh();
        });
        let mut holder = div()
            .relative()
            .flex_none()
            .w(px(PICKER_WIDTH))
            .child(face_el);
        if open {
            let t = tokens();
            let rows = items.into_iter().enumerate().map(|(i, item)| {
                let closing = self.open.clone();
                let Item {
                    title,
                    checked,
                    pick,
                } = item;
                div()
                    .id(("add-surface-item", i))
                    .relative()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .px_3()
                    .py_1()
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(t.surface_inset)))
                    .child(
                        div()
                            .w(px(10.))
                            .flex_none()
                            .children(checked.then_some("\u{2713}")),
                    )
                    .child(title.clone())
                    .child(scope.control_selected("menuitem", &title, true, None, checked))
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        closing.update(cx, |o, _| *o = None);
                        pick(cx);
                        window.refresh();
                    })
            });
            holder = holder.child(
                div().absolute().top_full().left_0().child(
                    deferred(
                        anchored().snap_to_window().child(
                            div()
                                .id("add-surface-menu")
                                .occlude()
                                .flex()
                                .flex_col()
                                .min_w(px(PICKER_WIDTH))
                                .py_1()
                                .bg(rgb(t.surface_raised))
                                .border_1()
                                .border_color(rgb(t.surface_separator))
                                .rounded(px(t.radius))
                                .text_color(rgb(t.text_primary))
                                .children(rows),
                        ),
                    )
                    .with_priority(3),
                ),
            );
        }
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .child(line(label, tokens().text_primary, scope)),
            )
            .child(holder)
            .into_any_element()
    }
}

impl Sheet for AddSurfaceSheet {
    fn label(&self) -> String {
        TITLE.to_owned()
    }

    fn render(&mut self, scope: &mut RegistryScope, _: &mut Window, cx: &mut App) -> AnyElement {
        let (draft, sources, destinations, note, problem) = {
            let s = &AppState::global(cx).read(cx).surfaces;
            (
                s.draft().cloned().unwrap_or_default(),
                s.ports().sources.clone(),
                s.ports().destinations.clone(),
                s.draft_note().map(str::to_owned),
                s.draft_problem().map(str::to_owned),
            )
        };
        let SurfaceDraft {
            kind,
            input,
            output,
            position,
        } = draft;
        let t = tokens();

        let kinds = KINDS
            .iter()
            .map(|&k| Item {
                title: kind_title(k).to_owned(),
                checked: k == kind,
                pick: Box::new(move |cx| edit(cx, move |s, core| s.set_draft_kind(k, core))),
            })
            .collect();
        let input_face = if input.is_empty() && sources.is_empty() {
            NO_SOURCES.to_owned()
        } else {
            input.clone()
        };
        let output_face = if output.is_empty() && destinations.is_empty() {
            NO_DESTINATIONS.to_owned()
        } else {
            output.clone()
        };
        let mut form = scope.container();
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                line(TITLE, t.text_primary, scope)
                    .text_base()
                    .font_weight(gpui::FontWeight::BOLD),
            )
            .child(self.picker(Picker::Type, kind_title(kind), kinds, &mut form, cx))
            .child(self.picker(
                Picker::Input,
                &input_face,
                port_items(&sources, &input, NO_SOURCES, |s, p| s.set_draft_input(p)),
                &mut form,
                cx,
            ))
            .child(self.picker(
                Picker::Output,
                &output_face,
                port_items(&destinations, &output, NO_DESTINATIONS, |s, p| {
                    s.set_draft_output(p)
                }),
                &mut form,
                cx,
            ));
        if kind == SurfaceKindRow::KontrolMk3 {
            let mut help = div().flex().flex_col().gap(px(2.)).text_size(px(11.));
            if let Some(n) = &note {
                help = help.child(line(n, t.state_warning, &mut form));
            }
            body = body.child(help.child(line(KONTROL_HELP, t.text_secondary, &mut form)));
        }
        if kind == SurfaceKindRow::McuXt {
            let label = format!("Position: {position}");
            let buttons = stepper_buttons(
                "Position",
                POSITIONS,
                i64::from(position),
                &mut form,
                |v, window, cx| {
                    edit(cx, move |s, _| s.set_draft_position(v as u8));
                    window.refresh();
                },
            );
            body = body
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .child(line(&label, t.text_primary, &mut form)),
                        )
                        .child(buttons),
                )
                .child(line(XT_HELP, t.text_secondary, &mut form).text_size(px(11.)));
        }
        if let Some(p) = &problem {
            body = body.child(line(p, t.state_error, scope));
        }
        body.into_any_element()
    }

    fn buttons(&self, cx: &App) -> Vec<SheetButton> {
        let s = &AppState::global(cx).read(cx).surfaces;
        let ready = s
            .draft()
            .is_some_and(|d| !d.input.is_empty() && !d.output.is_empty());
        let add = SheetButton::new(ADD, ButtonRole::Default);
        vec![
            SheetButton::new(CANCEL, ButtonRole::Cancel),
            if ready { add } else { add.disabled() },
        ]
    }

    fn press(&mut self, index: usize, window: &mut Window, cx: &mut App) -> bool {
        self.open.update(cx, |o, _| *o = None);
        let closed = AppState::global(cx).update(cx, |s, cx| {
            cx.notify();
            if index == 0 {
                s.surfaces.cancel_add();
                return true;
            }
            let core = s.store.core();
            s.surfaces.commit_add(core.as_ref());
            let added = s.surfaces.draft().is_none();
            // The pickers mark the new port and the surface tick runs (as
            // the channel's `surface.add`).
            if added {
                s.store.refresh_claimed_ports();
            }
            added
        });
        window.refresh();
        closed
    }

    fn width(&self) -> f32 {
        WIDTH
    }
}
