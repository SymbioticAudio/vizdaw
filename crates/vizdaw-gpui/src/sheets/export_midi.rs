//! File › Export › MIDI File… and MIDI Clip File… (Swift
//! `MidiExportSheet`, 480 wide): the bold title and its subtitle; for a
//! Standard MIDI File the `Type` and `Resolution` pickers; `Tracks`, a
//! toggle per track of the shown sequence (`Export track <name>`); `Write
//! per-note expression as MPE`, or for clips `Time selection only` (off
//! without one) and, for several tracks, the folder caption; `What won't
//! transfer` from the core's dry run (its groups, `Everything
//! transfers.`, or the spinner while it runs) and the error. `Cancel` and
//! `Export…` (off with no track).
//!
//! Every change asks the dry run again on the background executor; only
//! the newest answer lands, and a closed document drops it. Export… asks
//! where: a `<song>.mid` or `<track>.midi2` save panel (#10; the app adds
//! the extension and asks the replace question itself when the typed name
//! lacked it), or for several clip tracks a folder (#9b): the core then
//! names the files off the UI thread and, when any is there, the replace
//! question (`1 file already exists in <folder>. Replace it?`, `Replace`,
//! `Cancel`) is asked before anything is written. The sheet goes when the
//! job starts; a refusal or a failed job brings it back with the message
//! ([`present`]), never an alert. Never under the control channel.

use std::panic::AssertUnwindSafe;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext, Entity, InteractiveElement, IntoElement,
    ParentElement, PromptLevel, StatefulInteractiveElement, Styled, Window, anchored, deferred,
    div, px, rgb,
};
use vizdaw_app::DocId;
use vizdaw_app::interchange::{
    CANCEL, CHECKING, EACH_TRACK_ONE_FILE, EVERYTHING_TRANSFERS, EXPORT, ExportDestination,
    ExportKind, MIDI_EXPORT_FAILED, MidiExportChoices, REPLACE_ANSWERS, RESOLUTION,
    RESOLUTION_LABEL, RESOLUTIONS, TIME_SELECTION_ONLY, TRACKS, TYPE, TYPE_0, TYPE_1, TYPE_LABEL,
    WHAT_WONT_TRANSFER, WRITE_MPE, export_track_label,
};
use vizdaw_app::{panels, report_text};
use vizdaw_ffi::ReportRow;

use crate::app_state::AppState;
use crate::file_menu::choose_at;
use crate::file_panels::PanelPlace;
use crate::registry::RegistryScope;
use crate::sheet_host::{Ask, ButtonRole, Sheet, SheetButton, sheet_is};
use crate::shell::DocumentWindow;
use crate::transport_bar::colors;

/// The card's width (Swift `.frame(width: 480)`).
const WIDTH: f32 = 480.;
/// The track list's height (Swift `.frame(height: 120)`).
const TRACKS_HEIGHT: f32 = 120.;
/// What won't transfer's most height (Swift `maxHeight: 120`).
const WONT_HEIGHT: f32 = 120.;
/// The width of a picker row's label column.
const LABEL_WIDTH: f32 = 90.;
/// The error's colour.
const ERROR_TEXT: u32 = 0xe06060;

/// Which picker is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Type,
    Resolution,
}

/// What the sheet's controls share with their click handlers.
struct ExportUi {
    open: Option<Pick>,
}

/// What the sheet shows of the document, read once per frame.
struct View {
    choices: MidiExportChoices,
    wont_transfer: Option<ReportRow>,
    error: Option<String>,
    checking: bool,
    /// The shown sequence's tracks (ID, name).
    lanes: Vec<(u64, String)>,
    has_selection: bool,
}

/// The export sheet of document `doc`, for `kind`.
pub struct ExportMidiSheet {
    doc: DocId,
    kind: ExportKind,
    ui: Entity<ExportUi>,
}

impl ExportMidiSheet {
    fn new(doc: DocId, kind: ExportKind, cx: &mut App) -> Self {
        ExportMidiSheet {
            doc,
            kind,
            ui: cx.new(|_| ExportUi { open: None }),
        }
    }

    fn view(&self, cx: &App) -> Option<View> {
        let app = AppState::global(cx).read(cx);
        let d = app.store.document(self.doc)?;
        let e = d.midi_export();
        Some(View {
            choices: e.choices().clone(),
            wont_transfer: e.wont_transfer().cloned(),
            error: e.error().map(str::to_owned),
            checking: e.checking(),
            lanes: d.lanes().iter().map(|l| (l.id, l.name.clone())).collect(),
            has_selection: d.midi_export_has_selection(),
        })
    }
}

/// Edits `doc`'s choices (a change is checked again).
fn edit(doc: DocId, window: &mut Window, cx: &mut App, f: impl FnOnce(&mut MidiExportChoices)) {
    AppState::global(cx).update(cx, |s, cx| {
        if let Some(d) = s.store.document_mut(doc) {
            d.edit_midi_export(f);
        }
        cx.notify();
    });
    window.refresh();
}

/// Runs `doc`'s waiting dry run on the background executor; its answer
/// lands on the main thread (only the newest one is kept; a closed
/// document drops it).
fn run_check(doc: DocId, cx: &mut App) {
    let taken = AppState::global(cx).update(cx, |s, _| {
        let d = s.store.document_mut(doc)?;
        let check = d.take_midi_export_check()?;
        Some((check, d.session().clone()))
    });
    let Some((check, session)) = taken else {
        return;
    };
    let task = cx.background_executor().spawn(async move {
        std::panic::catch_unwind(AssertUnwindSafe(|| check.run(session.as_ref()))).ok()
    });
    cx.spawn(async move |cx| {
        let Some(landed) = task.await else { return };
        cx.update(|cx| {
            AppState::global(cx).update(cx, |s, cx| {
                if let Some(d) = s.store.document_mut(doc) {
                    d.land_midi_export_check(landed);
                    cx.notify();
                }
            });
        });
    })
    .detach();
}

/// Asks the core which clip files `doc`'s folder export writes, on the
/// background executor; when some are there already, the replace
/// question is asked on `asking` before anything is written.
fn run_names(doc: DocId, asking: AnyWindowHandle, cx: &mut App) {
    let taken = AppState::global(cx).update(cx, |s, _| {
        let d = s.store.document_mut(doc)?;
        let ask = d.take_clip_names()?;
        Some((ask, d.session().clone()))
    });
    let Some((ask, session)) = taken else {
        return;
    };
    let task = cx.background_executor().spawn(async move {
        std::panic::catch_unwind(AssertUnwindSafe(|| ask.run(session.as_ref()))).ok()
    });
    cx.spawn(async move |cx| {
        let Some(landed) = task.await else { return };
        cx.update(|cx| {
            let question = AppState::global(cx).update(cx, |s, cx| {
                cx.notify();
                let d = s.store.document_mut(doc)?;
                d.land_clip_names(landed);
                d.midi_export().replace_question().map(|q| q.message())
            });
            if let Some(message) = question {
                ask_replace(doc, asking, &message, cx);
            }
        });
    })
    .detach();
}

/// The replace question (title only): Replace exports, replacing them;
/// Cancel (or the window closing first) writes nothing.
fn ask_replace(doc: DocId, asking: AnyWindowHandle, message: &str, cx: &mut App) {
    let answer = crate::alerts::ask(
        asking,
        Ask::with_labels(PromptLevel::Warning, message, None, &REPLACE_ANSWERS),
        cx,
    );
    cx.spawn(async move |cx| {
        let replace = matches!(answer.await, Ok(0));
        cx.update(|cx| {
            AppState::global(cx).update(cx, |s, cx| {
                if let Some(d) = s.store.document_mut(doc) {
                    d.answer_midi_export_replace(replace);
                }
                cx.notify();
            });
        });
    })
    .detach();
}

/// Starts `doc`'s export at `path`, replacing as the panel confirmed.
fn start(doc: DocId, path: String, cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| {
        if let Some(d) = s.store.document_mut(doc) {
            d.start_midi_export(&path, true);
        }
        cx.notify();
    });
}

/// Export…: where to (a file, or a folder for several clip tracks), then
/// the job. A panel answered after the sheet went starts nothing.
fn begin(doc: DocId, label: &'static str, window: &Window, cx: &mut App) {
    let dest = AppState::global(cx)
        .read(cx)
        .store
        .document(doc)
        .map(|d| d.midi_export_destination());
    let asking = window.window_handle();
    match dest {
        Some(ExportDestination::File { name, ext }) => {
            let place = PanelPlace {
                name,
                ext: ext.to_owned(),
                ..PanelPlace::default()
            };
            let chosen =
                crate::file_panels::choose_save(&panels::MIDI_EXPORT_FILE, place, asking, cx);
            cx.spawn(async move |cx| {
                let answer = chosen.await;
                cx.update(|cx| match answer {
                    Ok(Some(_)) if !sheet_is(asking, label, cx) => {}
                    // The panel (or the app's own question) confirmed a replace.
                    Ok(Some(path)) => start(doc, path.to_string_lossy().into_owned(), cx),
                    Ok(None) => {}
                    Err(e) => AppState::global(cx).update(cx, |s, cx| {
                        s.store
                            .post_alert(vizdaw_app::AlertItem::new(MIDI_EXPORT_FAILED, e));
                        cx.notify();
                    }),
                });
            })
            .detach();
        }
        Some(ExportDestination::Folder) => choose_at(
            &panels::MIDI_EXPORT_FOLDER,
            PanelPlace::default(),
            window,
            cx,
            move |paths, cx| {
                let Some(folder) = paths.first() else { return };
                if !sheet_is(asking, label, cx) {
                    return;
                }
                let folder = folder.to_string_lossy().into_owned();
                AppState::global(cx).update(cx, |s, cx| {
                    if let Some(d) = s.store.document_mut(doc) {
                        d.export_midi_to_folder(&folder);
                    }
                    cx.notify();
                });
                run_names(doc, asking, cx);
            },
        ),
        None => {}
    }
}

/// A text line recorded in the tree.
fn text(t: &str, scope: &mut RegistryScope) -> gpui::Div {
    div()
        .relative()
        .child(t.to_owned())
        .child(scope.control("text", t, true))
}

/// A toggle: its box and title, recorded as a `checkbox` labelled
/// `label` (selected when on); a click flips it through `flip`.
fn toggle(
    id: (&'static str, usize),
    title: &str,
    label: &str,
    on: bool,
    enabled: bool,
    scope: &mut RegistryScope,
    flip: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .text_color(rgb(if enabled {
            colors::TEXT
        } else {
            colors::DISABLED
        }))
        .child(if on { "\u{2611}" } else { "\u{2610}" })
        .child(title.to_owned())
        .child(scope.control_selected("checkbox", label, enabled, None, on))
        .when(enabled, |d| {
            d.cursor_pointer().on_click(move |_, window, cx| {
                flip(window, cx);
                cx.stop_propagation();
            })
        })
        .into_any_element()
}

/// Picker `which`'s items and the chosen one's index.
fn items(which: Pick, c: &MidiExportChoices) -> (Vec<&'static str>, usize) {
    match which {
        Pick::Type => (vec![TYPE_1, TYPE_0], usize::from(c.type0)),
        Pick::Resolution => (
            RESOLUTIONS.iter().map(|(_, t)| *t).collect(),
            RESOLUTIONS
                .iter()
                .position(|(p, _)| *p == c.ppq)
                .unwrap_or(1),
        ),
    }
}

/// Item `i` of picker `which`, chosen.
fn choose(which: Pick, i: usize, c: &mut MidiExportChoices) {
    match which {
        Pick::Type => c.type0 = i == 1,
        Pick::Resolution => c.ppq = RESOLUTIONS[i].0,
    }
}

/// A labelled row: the label column, then `control`.
fn row(label: &str, control: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_none()
                .text_color(rgb(colors::TEXT))
                .child(label.to_owned()),
        )
        .child(control)
}

/// Picker `which` labelled `label`: its face (`menu`, its value the
/// choice), and its items (`menuitem`) while open.
fn picker(
    doc: DocId,
    which: Pick,
    label: &str,
    c: &MidiExportChoices,
    ui: &Entity<ExportUi>,
    scope: &mut RegistryScope,
    cx: &App,
) -> AnyElement {
    let (all, at) = items(which, c);
    let chosen = all[at].to_owned();
    let open = ui.read(cx).open == Some(which);
    let toggle_ui = ui.clone();
    let face = div()
        .id(("midi-export-picker", which as usize))
        .relative()
        .h(px(24.))
        .min_w(px(160.))
        .px_2()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(colors::BORDER))
        .bg(rgb(colors::BUTTON))
        .cursor_pointer()
        .child(chosen.clone())
        .child(div().text_color(rgb(colors::DIM)).child("\u{25BE}"))
        .child(scope.control_with("menu", label, true, Some(&chosen), false))
        .on_click(move |_, window, cx| {
            toggle_ui.update(cx, |u, _| {
                u.open = (u.open != Some(which)).then_some(which);
            });
            window.refresh();
        });
    face.when(open, |d| {
        let rows = all.iter().enumerate().map(|(i, title)| {
            let ui = ui.clone();
            div()
                .id(("midi-export-item", i))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::BUTTON)))
                .child(*title)
                .child(scope.control_selected("menuitem", title, true, None, i == at))
                .on_click(move |_, window, cx| {
                    ui.update(cx, |u, _| u.open = None);
                    edit(doc, window, cx, |c| choose(which, i, c));
                    cx.stop_propagation();
                })
        });
        d.child(
            div().absolute().top_full().left_0().child(
                deferred(
                    anchored().snap_to_window().child(
                        div()
                            .id("midi-export-list")
                            .occlude()
                            .flex()
                            .flex_col()
                            .min_w(px(160.))
                            .py_1()
                            .bg(rgb(colors::LIST))
                            .border_1()
                            .border_color(rgb(colors::BORDER))
                            .rounded_md()
                            .children(rows),
                    ),
                )
                .with_priority(3),
            ),
        )
    })
    .into_any_element()
}

/// The dry run's report (Swift `WontTransfer`): its groups and lines,
/// `Everything transfers.`, or the spinner while it runs.
fn wont_transfer(
    report: Option<&ReportRow>,
    checking: bool,
    scope: &mut RegistryScope,
) -> Option<AnyElement> {
    if let Some(r) = report {
        let groups = report_text::groups(r);
        if groups.is_empty() {
            return Some(
                text(EVERYTHING_TRANSFERS, scope)
                    .text_xs()
                    .text_color(rgb(colors::DIM))
                    .into_any_element(),
            );
        }
        let mut list = scope.container();
        let clip = list.clip();
        let mut col = div().flex().flex_col().gap_1();
        for (title, items) in groups {
            col = col.child(
                text(title, &mut list)
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD),
            );
            for item in items {
                col = col.child(text(&report_text::line(item), &mut list).text_xs());
            }
        }
        return Some(
            div()
                .id("midi-export-wont")
                .relative()
                .max_h(px(WONT_HEIGHT))
                .overflow_y_scroll()
                .child(col)
                .child(clip)
                .into_any_element(),
        );
    }
    checking.then(|| {
        div()
            .relative()
            .text_color(rgb(colors::DIM))
            .child("\u{22EF}")
            .child(scope.control("progress", CHECKING, true))
            .into_any_element()
    })
}

impl Sheet for ExportMidiSheet {
    fn label(&self) -> String {
        self.kind.title().to_owned()
    }

    fn width(&self) -> f32 {
        WIDTH
    }

    fn render(&mut self, scope: &mut RegistryScope, _: &mut Window, cx: &mut App) -> AnyElement {
        let doc = self.doc;
        let Some(v) = self.view(cx) else {
            return div().into_any_element();
        };
        run_check(doc, cx);
        let smf = self.kind == ExportKind::Smf;
        let c = &v.choices;
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(text(self.kind.title(), scope).font_weight(gpui::FontWeight::BOLD))
            .child(
                text(self.kind.subtitle(), scope)
                    .text_xs()
                    .text_color(rgb(colors::DIM)),
            );
        if smf {
            let ui = self.ui.clone();
            body = body
                .child(row(
                    TYPE,
                    picker(doc, Pick::Type, TYPE_LABEL, c, &ui, scope, cx),
                ))
                .child(row(
                    RESOLUTION,
                    picker(doc, Pick::Resolution, RESOLUTION_LABEL, c, &ui, scope, cx),
                ));
        }
        body = body.child(text(TRACKS, scope));
        let mut list = scope.container();
        let clip = list.clip();
        let rows: Vec<AnyElement> = v
            .lanes
            .iter()
            .enumerate()
            .map(|(i, (id, name))| {
                let id = *id;
                toggle(
                    ("midi-export-track", i),
                    name,
                    &export_track_label(name),
                    c.lanes.contains(&id),
                    true,
                    &mut list,
                    move |window, cx| {
                        edit(doc, window, cx, |c| {
                            if !c.lanes.remove(&id) {
                                c.lanes.insert(id);
                            }
                        })
                    },
                )
            })
            .collect();
        body = body.child(
            div()
                .id("midi-export-tracks")
                .relative()
                .h(px(TRACKS_HEIGHT))
                .overflow_y_scroll()
                .border_1()
                .border_color(rgb(colors::BORDER))
                .rounded_md()
                .px_2()
                .py_1()
                .child(div().flex().flex_col().gap_1().children(rows))
                .child(clip),
        );
        if smf {
            body = body.child(toggle(
                ("midi-export-toggle", 0),
                WRITE_MPE,
                WRITE_MPE,
                c.mpe,
                true,
                scope,
                move |window, cx| edit(doc, window, cx, |c| c.mpe = !c.mpe),
            ));
        } else {
            body = body.child(toggle(
                ("midi-export-toggle", 1),
                TIME_SELECTION_ONLY,
                TIME_SELECTION_ONLY,
                c.use_selection,
                v.has_selection,
                scope,
                move |window, cx| edit(doc, window, cx, |c| c.use_selection = !c.use_selection),
            ));
            if c.lanes.len() > 1 {
                body = body.child(
                    text(EACH_TRACK_ONE_FILE, scope)
                        .text_xs()
                        .text_color(rgb(colors::DIM)),
                );
            }
        }
        body = body.child(text(WHAT_WONT_TRANSFER, scope));
        if let Some(w) = wont_transfer(v.wont_transfer.as_ref(), v.checking, scope) {
            body = body.child(w);
        }
        if let Some(e) = &v.error {
            body = body.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .text_color(rgb(ERROR_TEXT))
                    .child(div().flex_none().child("\u{2716}"))
                    .child(text(e, scope).flex_1()),
            );
        }
        body.into_any_element()
    }

    fn buttons(&self, cx: &App) -> Vec<SheetButton> {
        let export = SheetButton::new(EXPORT, ButtonRole::Default);
        let none = self.view(cx).is_none_or(|v| v.choices.lanes.is_empty());
        vec![
            SheetButton::new(CANCEL, ButtonRole::Cancel),
            if none { export.disabled() } else { export },
        ]
    }

    fn press(&mut self, index: usize, window: &mut Window, cx: &mut App) -> bool {
        let doc = self.doc;
        self.ui.update(cx, |u, _| u.open = None);
        if index == 0 {
            AppState::global(cx).update(cx, |s, cx| {
                if let Some(d) = s.store.document_mut(doc) {
                    d.hide_midi_export();
                }
                cx.notify();
            });
            return true;
        }
        begin(doc, self.kind.title(), window, cx);
        false
    }
}

/// File › Export › MIDI File… / MIDI Clip File…: the sheet on `view`'s
/// window with the choices reset (none while another sheet is up, or
/// under the control channel).
pub fn show(
    kind: ExportKind,
    view: &mut DocumentWindow,
    window: &mut Window,
    cx: &mut gpui::Context<DocumentWindow>,
) {
    if crate::control::channel_on(cx) || view.sheet_label().is_some() {
        return;
    }
    let doc = view.doc();
    let shown = AppState::global(cx).update(cx, |s, cx| {
        cx.notify();
        let d = s.store.document_mut(doc)?;
        // Up on another window of the document already: it stays there.
        if d.midi_export().shown() {
            return None;
        }
        d.show_midi_export(kind);
        Some(())
    });
    if shown.is_none() {
        return;
    }
    let sheet = ExportMidiSheet::new(doc, kind, cx);
    if view.show_sheet(Box::new(sheet), window, cx).is_err() {
        AppState::global(cx).update(cx, |s, _| {
            if let Some(d) = s.store.document_mut(doc) {
                d.hide_midi_export();
            }
        });
    }
}

/// Shows each document's export sheet again when the document asks for
/// it (a refused start or a failed job brings it back with the message),
/// on one free window (`free`) of the document, the active one first;
/// takes it down once the document hid it (the job started). Under the
/// control channel nothing shows. Call it outside any update.
pub(crate) fn present(cx: &mut App, free: impl Fn(AnyWindowHandle, &mut App) -> bool) {
    let channel = crate::control::channel_on(cx);
    let app = AppState::global(cx);
    let active = cx.active_window();
    let mut windows: Vec<(DocId, gpui::WindowHandle<DocumentWindow>)> = app
        .read(cx)
        .document_windows()
        .map(|w| (w.doc, w.handle))
        .collect();
    windows.sort_by_key(|(_, h)| Some(AnyWindowHandle::from(*h)) != active);
    let wanted = |doc: DocId, cx: &App| {
        if channel {
            return None;
        }
        let d = AppState::global(cx).read(cx).store.document(doc)?;
        let e = d.midi_export();
        e.shown().then(|| e.kind())
    };
    let mut placed: Vec<DocId> = vec![];
    for (doc, handle) in &windows {
        let label = handle.update(cx, |v, _, _| v.sheet_label()).ok().flatten();
        let Some(label) = label else { continue };
        let ours = [ExportKind::Smf, ExportKind::Clip]
            .iter()
            .find(|k| k.title() == label)
            .copied();
        let Some(kind) = ours else { continue };
        if wanted(*doc, cx) == Some(kind) && !placed.contains(doc) {
            placed.push(*doc);
        } else {
            handle
                .update(cx, |v, window, cx| v.close_sheet(window, cx))
                .ok();
        }
    }
    for (doc, handle) in windows {
        if placed.contains(&doc) || !free(handle.into(), cx) {
            continue;
        }
        let Some(kind) = wanted(doc, cx) else {
            continue;
        };
        let sheet = ExportMidiSheet::new(doc, kind, cx);
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
