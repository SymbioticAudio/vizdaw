//! File › Import MIDI File… (Swift `MidiImportSheet`, 480 wide): the bold
//! title; `<file>: <kind>, N track(s)`; the `Import into` choice (`This
//! project, at the playhead` / `A new project`); the channel list — each
//! channel's name, its notes and its `Destination` picker (`New built-in
//! synth` and the project's destinations; off for a new project, whose
//! heading says every channel plays a new built-in synth) — or `The file
//! plays no notes on any channel.`; `Split by channel`, `Treat as MPE` and
//! `Use the file's tempo map` (on and off-limits for a sequence without
//! events or a new project, with Swift's help). `Cancel` and `Import`.
//!
//! The panel's file is read on the background executor
//! ([`vizdaw_app::DocumentStore::begin_midi_import`]); the sheet shows
//! once the read landed ([`present`]), on one window of the document. Import
//! into this project starts the `ImportMidi` job; into a new project the
//! store makes the project and its window opens. Never under the control
//! channel.

use crate::appearance::ThemedCorners as _;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, px, rgb,
};
use vizdaw_app::DocId;
use vizdaw_app::interchange::{
    A_NEW_PROJECT, CANCEL, ChannelChoice, IMPORT, IMPORT_INTO, IMPORT_TITLE, MPE_HELP, MidiImport,
    NEW_BUILT_IN_SYNTH, NEW_PROJECT_CHANNELS, NO_NOTES, SPLIT_BY_CHANNEL, SPLIT_LABEL,
    THIS_PROJECT, TREAT_AS_MPE, TempoBox, USE_TEMPO, WHERE_EACH_CHANNEL_PLAYS, channel_name,
    describe, destination_label, notes_text,
};
use vizdaw_ffi::mixer::DestRow;

use crate::app_state::AppState;
use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton, with_help};
use crate::shell::DocumentWindow;
use crate::transport_bar::colors;

/// The card's width (Swift `.frame(width: 480)`).
const WIDTH: f32 = 480.;
/// The channel list's height (Swift `.frame(height: 160)`).
const LIST_HEIGHT: f32 = 160.;
/// A channel name's least width (Swift `minWidth: 120`).
const NAME_WIDTH: f32 = 120.;
/// The destination picker's width (Swift `.frame(width: 200)`).
const PICKER_WIDTH: f32 = 200.;

/// What the sheet's controls share with their click handlers.
struct ImportUi {
    /// The row whose destination picker is open.
    open: Option<usize>,
}

/// What the sheet shows of the document, read once per frame.
struct View {
    sheet: MidiImport,
    tempo: TempoBox,
}

/// The Import MIDI File sheet of document `doc`.
pub struct ImportMidiSheet {
    doc: DocId,
    ui: Entity<ImportUi>,
    /// The project's destinations a channel can play to (as the sheet
    /// opened, Swift's `destinations`).
    destinations: Vec<DestRow>,
}

impl ImportMidiSheet {
    fn new(doc: DocId, destinations: Vec<DestRow>, cx: &mut App) -> Self {
        ImportMidiSheet {
            doc,
            ui: cx.new(|_| ImportUi { open: None }),
            destinations,
        }
    }

    fn view(&self, cx: &App) -> Option<View> {
        let app = AppState::global(cx).read(cx);
        let d = app.store.document(self.doc)?;
        Some(View {
            sheet: d.midi_import()?.clone(),
            tempo: d.midi_import_tempo_box(),
        })
    }
}

/// Edits `doc`'s sheet.
fn edit(doc: DocId, window: &mut Window, cx: &mut App, f: impl FnOnce(&mut MidiImport)) {
    AppState::global(cx).update(cx, |s, cx| {
        if let Some(d) = s.store.document_mut(doc) {
            d.edit_midi_import(f);
        }
        cx.notify();
    });
    window.refresh();
}

/// A text line recorded in the tree.
fn text(t: &str, scope: &mut RegistryScope) -> gpui::Div {
    div()
        .relative()
        .child(t.to_owned())
        .child(scope.control("text", t, true))
}

/// A toggle: its box and title, recorded as a `checkbox` labelled
/// `label` (selected when on, its help as its value); a click flips it.
#[allow(clippy::too_many_arguments)]
fn toggle(
    i: usize,
    title: &str,
    label: &str,
    help: Option<&'static str>,
    on: bool,
    enabled: bool,
    scope: &mut RegistryScope,
    flip: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(("midi-import-toggle", i))
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .text_color(rgb(if enabled {
            colors::text()
        } else {
            colors::disabled()
        }))
        .child(if on { "\u{2611}" } else { "\u{2610}" })
        .child(title.to_owned())
        .child(scope.control_selected("checkbox", label, enabled, help, on))
        .when_some(help, with_help)
        .when(enabled, |d| {
            d.cursor_pointer().on_click(move |_, window, cx| {
                flip(window, cx);
                cx.stop_propagation();
            })
        })
        .into_any_element()
}

/// The `Import into` choice: two radio buttons, the chosen one selected.
fn where_to(doc: DocId, fresh: bool, scope: &mut RegistryScope) -> AnyElement {
    let radio = |i: usize, title: &str, value: bool, scope: &mut RegistryScope| {
        let on = fresh == value;
        div()
            .id(("midi-import-into", i))
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .text_color(rgb(colors::text()))
            .child(if on { "\u{25C9}" } else { "\u{25CB}" })
            .child(title.to_owned())
            .child(scope.control_selected("radio", title, true, None, on))
            .on_click(move |_, window, cx| {
                edit(doc, window, cx, |m| m.into_new_project = value);
                cx.stop_propagation();
            })
    };
    div()
        .flex()
        .flex_row()
        .gap_2()
        .child(
            div()
                .relative()
                .flex_none()
                .child(IMPORT_INTO)
                .child(scope.control("text", IMPORT_INTO, true)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(radio(0, THIS_PROJECT, false, scope))
                .child(radio(1, A_NEW_PROJECT, true, scope)),
        )
        .into_any_element()
}

/// Row `row`'s destination picker: its face (`menu`, labelled
/// `Destination for channel N`, its value the choice), and while open its
/// items (`menuitem`).
#[allow(clippy::too_many_arguments)]
fn picker(
    doc: DocId,
    row: usize,
    c: &ChannelChoice,
    destinations: &[DestRow],
    enabled: bool,
    ui: &Entity<ImportUi>,
    scope: &mut RegistryScope,
    cx: &App,
) -> AnyElement {
    let mut all: Vec<(Option<u64>, String)> = vec![(None, NEW_BUILT_IN_SYNTH.to_owned())];
    all.extend(destinations.iter().map(|d| (Some(d.id), d.name.clone())));
    // A suggestion the list lacks still names itself (as its ID).
    let chosen = all.iter().find(|(id, _)| *id == c.dest).map_or_else(
        || c.dest.map(|d| d.to_string()).unwrap_or_default(),
        |(_, t)| t.clone(),
    );
    let open = enabled && ui.read(cx).open == Some(row);
    let label = destination_label(c);
    let toggle_ui = ui.clone();
    let face = div()
        .id(("midi-import-picker", row))
        .relative()
        .flex_none()
        .w(px(PICKER_WIDTH))
        .h(px(24.))
        .px_2()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .round_md()
        .border_1()
        .border_color(rgb(colors::border()))
        .bg(rgb(colors::button()))
        .text_color(rgb(if enabled {
            colors::text()
        } else {
            colors::disabled()
        }))
        .child(div().overflow_hidden().child(chosen.clone()))
        .child(div().text_color(rgb(colors::dim())).child("\u{25BE}"))
        .child(scope.control_with("menu", &label, enabled, Some(&chosen), false))
        .when(enabled, |d| {
            d.cursor_pointer().on_click(move |_, window, cx| {
                toggle_ui.update(cx, |u, _| {
                    u.open = (u.open != Some(row)).then_some(row);
                });
                window.refresh();
                cx.stop_propagation();
            })
        });
    face.when(open, |d| {
        let rows = all.iter().enumerate().map(|(i, (dest, title))| {
            let ui = ui.clone();
            let dest = *dest;
            div()
                .id(("midi-import-item", i))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::button())))
                .child(title.clone())
                .child(scope.control_selected("menuitem", title, true, None, dest == c.dest))
                .on_click(move |_, window, cx| {
                    ui.update(cx, |u, _| u.open = None);
                    edit(doc, window, cx, |m| {
                        if let Some(c) = m.choices.get_mut(row) {
                            c.dest = dest;
                        }
                    });
                    cx.stop_propagation();
                })
        });
        d.child(
            div().absolute().top_full().left_0().child(
                deferred(
                    anchored().snap_to_window().child(
                        div()
                            .id("midi-import-list")
                            .occlude()
                            .flex()
                            .flex_col()
                            .min_w(px(PICKER_WIDTH))
                            .py_1()
                            .bg(rgb(colors::list()))
                            .border_1()
                            .border_color(rgb(colors::border()))
                            .round_md()
                            .children(rows),
                    ),
                )
                .with_priority(3),
            ),
        )
    })
    .into_any_element()
}

impl Sheet for ImportMidiSheet {
    fn label(&self) -> String {
        IMPORT_TITLE.to_owned()
    }

    fn width(&self) -> f32 {
        WIDTH
    }

    fn render(&mut self, scope: &mut RegistryScope, _: &mut Window, cx: &mut App) -> AnyElement {
        let doc = self.doc;
        let Some(v) = self.view(cx) else {
            return div().into_any_element();
        };
        let m = &v.sheet;
        let fresh = m.into_new_project;
        let summary = describe(m.info(), m.path());
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(text(IMPORT_TITLE, scope).font_weight(gpui::FontWeight::BOLD))
            .child(
                text(&summary, scope)
                    .text_xs()
                    .text_color(rgb(colors::dim())),
            )
            .child(where_to(doc, fresh, scope));
        if m.choices.is_empty() {
            body = body.child(text(NO_NOTES, scope).text_color(rgb(colors::dim())));
        } else {
            let heading = if fresh {
                NEW_PROJECT_CHANNELS
            } else {
                WHERE_EACH_CHANNEL_PLAYS
            };
            body = body.child(text(heading, scope));
            let mut list = scope.container();
            let clip = list.clip();
            let rows: Vec<AnyElement> = m
                .choices
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let name = channel_name(c);
                    let notes = notes_text(c.notes);
                    let mut row = list.container();
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .py_1()
                        .child(text(&name, &mut row).min_w(px(NAME_WIDTH)))
                        .child(
                            text(&notes, &mut row)
                                .text_xs()
                                .text_color(rgb(colors::dim())),
                        )
                        .child(div().flex_1())
                        .child(picker(
                            doc,
                            i,
                            c,
                            &self.destinations,
                            !fresh,
                            &self.ui,
                            &mut row,
                            cx,
                        ))
                        .into_any_element()
                })
                .collect();
            body = body.child(
                div()
                    .id("midi-import-channels")
                    .relative()
                    .h(px(LIST_HEIGHT))
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(rgb(colors::border()))
                    .round_md()
                    .px_2()
                    .child(div().flex().flex_col().children(rows))
                    .child(clip),
            );
        }
        body = body
            .child(toggle(
                0,
                SPLIT_BY_CHANNEL,
                SPLIT_LABEL,
                None,
                m.split_channels,
                true,
                scope,
                move |window, cx| edit(doc, window, cx, |m| m.split_channels = !m.split_channels),
            ))
            .child(toggle(
                1,
                TREAT_AS_MPE,
                TREAT_AS_MPE,
                Some(MPE_HELP),
                m.mpe,
                true,
                scope,
                move |window, cx| edit(doc, window, cx, |m| m.mpe = !m.mpe),
            ));
        let tempo = v.tempo;
        body = body.child(toggle(
            2,
            USE_TEMPO,
            USE_TEMPO,
            Some(tempo.help),
            tempo.on,
            tempo.enabled,
            scope,
            move |window, cx| {
                AppState::global(cx).update(cx, |s, cx| {
                    if let Some(d) = s.store.document_mut(doc) {
                        d.set_midi_import_use_tempo(!tempo.on);
                    }
                    cx.notify();
                });
                window.refresh();
            },
        ));
        body.into_any_element()
    }

    fn buttons(&self, _: &App) -> Vec<SheetButton> {
        vec![
            SheetButton::new(CANCEL, ButtonRole::Cancel),
            SheetButton::new(IMPORT, ButtonRole::Default),
        ]
    }

    fn press(&mut self, index: usize, _: &mut Window, cx: &mut App) -> bool {
        let doc = self.doc;
        self.ui.update(cx, |u, _| u.open = None);
        let fresh = self.view(cx).is_some_and(|v| v.sheet.into_new_project);
        if index == 1 && fresh {
            // The new project's window opens outside this window's update.
            cx.defer(move |cx| {
                let made = AppState::global(cx).update(cx, |s, cx| {
                    cx.notify();
                    s.store.import_midi_as_new_project(doc)
                });
                crate::show(made, cx);
            });
            return true;
        }
        AppState::global(cx).update(cx, |s, cx| {
            if let Some(d) = s.store.document_mut(doc) {
                if index == 1 {
                    d.start_midi_import();
                } else {
                    d.hide_midi_import();
                }
            }
            cx.notify();
        });
        true
    }
}

/// Shows each document's Import MIDI File sheet once its read landed, on
/// one of its windows (the active one if it is the document's) that is
/// free (`free`: no prompt, ask or sheet there), and takes it down once
/// the document hid it. Under the control channel nothing shows. Call it
/// outside any update (the presenter's).
pub(crate) fn present(cx: &mut App, free: impl Fn(gpui::AnyWindowHandle, &mut App) -> bool) {
    let channel = crate::control::channel_on(cx);
    let app = AppState::global(cx);
    let active = cx.active_window();
    let mut windows: Vec<(DocId, gpui::WindowHandle<DocumentWindow>)> = app
        .read(cx)
        .document_windows()
        .filter_map(|w| w.document_window().map(|h| (w.doc, h)))
        .collect();
    // The active window first: it is the one that asked.
    windows.sort_by_key(|(_, h)| Some(gpui::AnyWindowHandle::from(*h)) != active);
    let mut placed: Vec<DocId> = vec![];
    // Windows showing the sheet, and whether their document still wants it.
    let mut up: Vec<(DocId, gpui::WindowHandle<DocumentWindow>)> = vec![];
    for (doc, handle) in &windows {
        if let Ok(true) = handle.update(cx, |v, _, _| {
            v.sheet_label().as_deref() == Some(IMPORT_TITLE)
        }) {
            up.push((*doc, *handle));
        }
    }
    let wants = |doc: DocId, cx: &App| {
        !channel
            && AppState::global(cx)
                .read(cx)
                .store
                .document(doc)
                .and_then(|d| d.midi_import())
                .is_some_and(MidiImport::shown)
    };
    for (doc, handle) in up {
        if wants(doc, cx) && !placed.contains(&doc) {
            placed.push(doc);
        } else {
            handle
                .update(cx, |v, window, cx| v.close_sheet(window, cx))
                .ok();
        }
    }
    for (doc, handle) in windows {
        if placed.contains(&doc) || !wants(doc, cx) || !free(handle.into(), cx) {
            continue;
        }
        let destinations = app
            .update(cx, |s, _| {
                s.store.mixer(doc).map(|m| m.destinations().to_vec())
            })
            .unwrap_or_default();
        let sheet = ImportMidiSheet::new(doc, destinations, cx);
        let shown = handle
            .update(cx, |v, window, cx| {
                v.show_sheet(Box::new(sheet), window, cx).is_ok()
            })
            .unwrap_or(false);
        if shown {
            placed.push(doc);
        }
    }
}
