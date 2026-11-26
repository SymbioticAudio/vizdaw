//! File › Export › AAF… and File › Import Project › AAF… (Swift
//! `AafExportSheet`, `AafImportSheet`; both 480 wide).
//!
//! Export AAF: the bold title and `Audio, fades and volume; MIDI as MIDI
//! files beside it`; `Embed audio in the AAF file`; the `Bit depth`,
//! `Stereo tracks`, `Start` and `Frame rate` pickers; `Clip gain` and
//! `Track volume and pan` (all off while the export waits or runs);
//! `Writes the folder “<name>”.` with the core's name; `What won't
//! transfer` and the error; while waiting `Waiting for the running job
//! to finish…`; the same buttons as the other export sheets (`Export…`
//! off without a check or the core's name). Every change asks the check
//! again on the background executor; only the newest answer lands.
//! Export… asks for the folder to put the export in (panel #9d, its
//! message naming the core's folder). The core writes a new folder
//! there, numbered when the name is taken, so nothing is ever replaced
//! and nothing is asked. The sheet closes when the job ends (the report
//! follows); a refusal or a failed job brings it back with the message.
//!
//! Import AAF: shown once the chosen file was read off the UI thread
//! ([`vizdaw_app::DocumentStore::begin_aaf_import`]): `<file>: <rate> Hz,
//! N tracks, N audio files[, N missing]`; `Import into` (`Open as a new
//! project`, the default, or `Add to this project`); the `MIDI file
//! beside it` field (filled from the core's find; while it edits, the
//! document's bare transport keys are off through the window's sheet
//! owner) and its `Choose…` (panel #15); `Look for missing audio in…`
//! with the folder's name or `Nowhere else`, `Choose…` (panel #16) and
//! `Clear`; `Join left and right tracks into stereo`; `Cancel` and
//! `Import`. A new project is made at the file's rate and its window
//! opens. Never under the control channel.

use std::panic::AssertUnwindSafe;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled, Window,
    anchored, deferred, div, px, rgb,
};
use vizdaw_app::aaf::{
    ADD_TO_THIS, AUTOMATION, AafExportChoices, AafImport, BIT_DEPTH, CANCEL, CHOOSE,
    CHOOSE_FOLDER_LABEL, CHOOSE_MIDI_LABEL, CLEAR, CLEAR_LABEL, CLIP_GAIN, DEPTHS, EMBED,
    EXPORT_SUBTITLE, EXPORT_TITLE, FPS, FRAME_RATE, IMPORT, IMPORT_INTO, IMPORT_TITLE, JOIN_PAIRS,
    LOOK_FOR_MISSING, MIDI_BESIDE, NOWHERE_ELSE, OPEN_AS_NEW, START, STARTS, STEREO, STEREO_TRACKS,
    WAITING_TEXT, describe, folder_name, writes_folder,
};
use vizdaw_app::for_daw::PROGRESS_LABEL;
use vizdaw_app::interchange::WHAT_WONT_TRANSFER;
use vizdaw_app::{DocId, DocumentModel, panels};
use vizdaw_ffi::ReportRow;

use super::audio_format::{progress_bar, row, text, toggle};
use super::export_midi::wont_transfer;
use super::for_daw::{error_row, export_buttons, present_sheet, waiting_row};
use crate::app_state::AppState;
use crate::file_menu::choose_at;
use crate::file_panels::PanelPlace;
use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton, button_face, sheet_is, with_help};
use crate::shell::DocumentWindow;
use crate::text_input::TextInput;
use crate::transport_bar::colors;

/// The cards' width (Swift `.frame(width: 480)`).
const WIDTH: f32 = 480.;

/// Runs `f` on `doc` (a closed document does nothing).
fn with_doc(doc: DocId, cx: &mut App, f: impl FnOnce(&mut DocumentModel)) {
    AppState::global(cx).update(cx, |s, cx| {
        if let Some(d) = s.store.document_mut(doc) {
            f(d);
        }
        cx.notify();
    });
}

/// A picker: its face (`menu`, labelled `label`, its value the choice)
/// and, while open, its items (`menuitem`); choosing item `i` calls
/// `pick(i)`.
#[allow(clippy::too_many_arguments)]
fn picker(
    which: usize,
    label: &'static str,
    titles: &[&'static str],
    chosen: usize,
    enabled: bool,
    open: &Entity<Option<usize>>,
    scope: &mut RegistryScope,
    cx: &App,
    pick: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
) -> AnyElement {
    let title = titles.get(chosen).copied().unwrap_or_default();
    let is_open = enabled && *open.read(cx) == Some(which);
    let flip = open.clone();
    let face = div()
        .id(("aaf-picker", which))
        .relative()
        .h(px(24.))
        .min_w(px(200.))
        .px_2()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(colors::border()))
        .bg(rgb(colors::button()))
        .text_color(rgb(if enabled {
            colors::text()
        } else {
            colors::disabled()
        }))
        .child(title)
        .child(div().text_color(rgb(colors::dim())).child("\u{25BE}"))
        .child(scope.control_with("menu", label, enabled, Some(title), false))
        .when(enabled, |d| {
            d.cursor_pointer().on_click(move |_, window, cx| {
                flip.update(cx, |o, _| *o = (*o != Some(which)).then_some(which));
                window.refresh();
            })
        });
    face.when(is_open, |d| {
        let rows = titles.iter().enumerate().map(|(i, t)| {
            let open = open.clone();
            let pick = pick.clone();
            div()
                .id(("aaf-picker-item", i))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::button())))
                .child(*t)
                .child(scope.control_selected("menuitem", t, true, None, i == chosen))
                .on_click(move |_, window, cx| {
                    open.update(cx, |o, _| *o = None);
                    pick(i, window, cx);
                    window.refresh();
                    cx.stop_propagation();
                })
        });
        d.child(
            div().absolute().top_full().left_0().child(
                deferred(
                    anchored().snap_to_window().child(
                        div()
                            .id("aaf-picker-list")
                            .occlude()
                            .flex()
                            .flex_col()
                            .min_w(px(200.))
                            .py_1()
                            .bg(rgb(colors::list()))
                            .border_1()
                            .border_color(rgb(colors::border()))
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

// ---------------------------------------------------------------------
// Export AAF

/// What the export sheet shows of the document, read once per frame.
struct ExportView {
    choices: AafExportChoices,
    wont_transfer: Option<ReportRow>,
    error: Option<String>,
    checking: bool,
    default_name: String,
    waiting: bool,
    running: bool,
    progress: Option<f32>,
}

/// The Export AAF sheet of document `doc`.
pub struct AafExportSheet {
    doc: DocId,
    /// The open picker.
    open: Entity<Option<usize>>,
}

impl AafExportSheet {
    fn new(doc: DocId, cx: &mut App) -> Self {
        AafExportSheet {
            doc,
            open: cx.new(|_| None),
        }
    }

    fn view(&self, cx: &App) -> Option<ExportView> {
        let app = AppState::global(cx).read(cx);
        let d = app.store.document(self.doc)?;
        let e = d.aaf_export();
        Some(ExportView {
            choices: *e.choices(),
            wont_transfer: e.wont_transfer().cloned(),
            error: e.error().map(str::to_owned),
            checking: e.checking(),
            default_name: e.default_name().to_owned(),
            waiting: e.waiting(),
            running: d.aaf_export_running(),
            progress: d.aaf_export_progress(),
        })
    }
}

/// Edits `doc`'s options (a change is checked again).
fn edit(doc: DocId, cx: &mut App, f: impl FnOnce(&mut AafExportChoices)) {
    with_doc(doc, cx, |d| d.edit_aaf_export(f));
}

/// Runs `doc`'s waiting check on the background executor; only the
/// newest answer lands, and a closed document drops it.
fn run_check(doc: DocId, cx: &mut App) {
    let taken = AppState::global(cx).update(cx, |s, _| {
        let d = s.store.document_mut(doc)?;
        let check = d.take_aaf_check()?;
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
        cx.update(|cx| with_doc(doc, cx, |d| d.land_aaf_check(landed)));
    })
    .detach();
}

/// Export…: the folder panel naming the core's folder, then the export
/// (or its wait). A panel answered after the sheet went starts nothing.
fn begin(doc: DocId, name: String, window: &Window, cx: &mut App) {
    let asking = window.window_handle();
    let place = PanelPlace {
        name,
        ..PanelPlace::default()
    };
    choose_at(
        &panels::AAF_EXPORT_FOLDER,
        place,
        window,
        cx,
        move |paths, cx| {
            let Some(folder) = paths.first() else { return };
            if !sheet_is(asking, EXPORT_TITLE, cx) {
                return;
            }
            let folder = folder.to_string_lossy().into_owned();
            with_doc(doc, cx, |d| d.start_aaf_export(&folder));
        },
    );
}

/// The index of `value` in `all` (0 when missing).
fn index_of<T: PartialEq>(all: &[(T, &str)], value: &T) -> usize {
    all.iter().position(|(v, _)| v == value).unwrap_or(0)
}

fn titles<T>(all: &[(T, &'static str)]) -> Vec<&'static str> {
    all.iter().map(|(_, t)| *t).collect()
}

impl Sheet for AafExportSheet {
    fn label(&self) -> String {
        EXPORT_TITLE.to_owned()
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
        // A kept or running export has its choices already.
        let enabled = !v.waiting && !v.running;
        if !enabled {
            self.open.update(cx, |o, _| *o = None);
        }
        let c = v.choices;
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(text(EXPORT_TITLE, scope).font_weight(gpui::FontWeight::BOLD))
            .child(
                text(EXPORT_SUBTITLE, scope)
                    .text_xs()
                    .text_color(rgb(colors::dim())),
            )
            .child(toggle(
                ("aaf-embed", 0),
                EMBED,
                EMBED,
                c.embed,
                enabled,
                scope,
                move |_, cx| edit(doc, cx, |c| c.embed = !c.embed),
            ))
            .child(row(
                BIT_DEPTH,
                picker(
                    0,
                    BIT_DEPTH,
                    &titles(&DEPTHS),
                    index_of(&DEPTHS, &c.depth),
                    enabled,
                    &self.open,
                    scope,
                    cx,
                    move |i, _, cx| edit(doc, cx, |c| c.depth = DEPTHS[i].0),
                ),
            ))
            .child(row(
                STEREO_TRACKS,
                picker(
                    1,
                    STEREO_TRACKS,
                    &titles(&STEREO),
                    index_of(&STEREO, &c.stereo),
                    enabled,
                    &self.open,
                    scope,
                    cx,
                    move |i, _, cx| edit(doc, cx, |c| c.stereo = STEREO[i].0),
                ),
            ))
            .child(row(
                START,
                picker(
                    2,
                    START,
                    &titles(&STARTS),
                    index_of(&STARTS, &c.hour_start),
                    enabled,
                    &self.open,
                    scope,
                    cx,
                    move |i, _, cx| edit(doc, cx, |c| c.hour_start = STARTS[i].0),
                ),
            ))
            .child(row(
                FRAME_RATE,
                picker(
                    3,
                    FRAME_RATE,
                    &titles(&FPS),
                    index_of(&FPS, &c.fps),
                    enabled,
                    &self.open,
                    scope,
                    cx,
                    move |i, _, cx| edit(doc, cx, |c| c.fps = FPS[i].0),
                ),
            ))
            .child(toggle(
                ("aaf-clip-gain", 0),
                CLIP_GAIN,
                CLIP_GAIN,
                c.clip_gain,
                enabled,
                scope,
                move |_, cx| edit(doc, cx, |c| c.clip_gain = !c.clip_gain),
            ))
            .child(toggle(
                ("aaf-automation", 0),
                AUTOMATION,
                AUTOMATION,
                c.automation,
                enabled,
                scope,
                move |_, cx| edit(doc, cx, |c| c.automation = !c.automation),
            ));
        if !v.default_name.is_empty() {
            body = body.child(
                text(&writes_folder(&v.default_name), scope)
                    .text_xs()
                    .text_color(rgb(colors::dim())),
            );
        }
        body = body.child(text(WHAT_WONT_TRANSFER, scope));
        if let Some(w) = wont_transfer(v.wont_transfer.as_ref(), v.checking, scope) {
            body = body.child(w);
        }
        if let Some(e) = &v.error {
            body = body.child(error_row(e, scope));
        }
        if v.waiting {
            body = body.child(waiting_row(WAITING_TEXT, scope));
        } else if v.running
            && let Some(p) = v.progress
        {
            body = body.child(progress_bar(p, PROGRESS_LABEL, scope));
        }
        body.into_any_element()
    }

    fn buttons(&self, cx: &App) -> Vec<SheetButton> {
        match self.view(cx) {
            Some(v) => export_buttons(
                v.waiting,
                v.running,
                v.wont_transfer.is_some() && !v.default_name.is_empty(),
            ),
            None => export_buttons(false, false, false),
        }
    }

    fn press(&mut self, index: usize, window: &mut Window, cx: &mut App) -> bool {
        let doc = self.doc;
        self.open.update(cx, |o, _| *o = None);
        let Some(v) = self.view(cx) else {
            return true;
        };
        if v.waiting {
            with_doc(doc, cx, |d| d.cancel_aaf_export_waiting());
            return false;
        }
        if v.running {
            with_doc(doc, cx, |d| d.cancel_aaf_export());
            return false;
        }
        if index == 0 {
            with_doc(doc, cx, |d| d.hide_aaf_export());
            return true;
        }
        begin(doc, v.default_name, window, cx);
        false
    }
}

/// File › Export › AAF…: the sheet on `view`'s window (none while another
/// sheet is up, or under the control channel).
pub fn show_export(
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
        if d.aaf_export().shown() {
            return None;
        }
        d.show_aaf_export();
        Some(())
    });
    if shown.is_none() {
        return;
    }
    let sheet = AafExportSheet::new(doc, cx);
    if view.show_sheet(Box::new(sheet), window, cx).is_err() {
        with_doc(doc, cx, |d| d.hide_aaf_export());
    }
}

// ---------------------------------------------------------------------
// Import AAF

/// The Import AAF sheet of document `doc`.
pub struct AafImportSheet {
    doc: DocId,
    /// The `MIDI file beside it` field.
    midi: Entity<TextInput>,
}

impl AafImportSheet {
    fn new(doc: DocId, cx: &mut App) -> Self {
        let midi_path = AppState::global(cx)
            .read(cx)
            .store
            .document(doc)
            .and_then(|d| d.aaf_import())
            .and_then(|a| a.midi_path.clone())
            .unwrap_or_default();
        AafImportSheet {
            doc,
            midi: cx.new(|cx| TextInput::new(MIDI_BESIDE, cx).with_text(&midi_path)),
        }
    }

    fn view(&self, cx: &App) -> Option<AafImport> {
        AppState::global(cx)
            .read(cx)
            .store
            .document(self.doc)?
            .aaf_import()
            .cloned()
    }

    /// The field and the choice agree: what is typed is the MIDI path
    /// (empty: none); left, the field shows the choice (a Choose… answer).
    fn sync_midi(&self, a: &AafImport, window: &Window, cx: &mut App) {
        let typed = self.midi.read(cx).text().to_owned();
        let chosen = a.midi_path.clone().unwrap_or_default();
        if typed == chosen {
            return;
        }
        if self.midi.focus_handle(cx).is_focused(window) {
            let path = (!typed.is_empty()).then_some(typed);
            with_doc(self.doc, cx, |d| d.edit_aaf_import(|a| a.midi_path = path));
        } else {
            self.midi.update(cx, |t, cx| t.set_text(&chosen, cx));
        }
    }
}

/// Edits `doc`'s import choices.
fn edit_import(doc: DocId, window: &mut Window, cx: &mut App, f: impl FnOnce(&mut AafImport)) {
    with_doc(doc, cx, |d| d.edit_aaf_import(f));
    window.refresh();
}

/// A button in the sheet's body, recorded as `button` labelled `a11y`.
fn body_button(
    id: usize,
    title: &str,
    a11y: &str,
    help: Option<String>,
    scope: &mut RegistryScope,
    click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let b = SheetButton::new(title, ButtonRole::Plain);
    button_face(id, &b)
        .child(scope.control("button", a11y, true))
        .when_some(help, with_help)
        .on_click(move |_, window, cx| {
            click(window, cx);
            cx.stop_propagation();
        })
        .into_any_element()
}

/// `spec`'s panel; its first path goes to `then` while the import sheet
/// is still up in the window that asked.
fn choose_for_sheet(
    spec: &'static vizdaw_app::panels::PanelSpec,
    window: &Window,
    cx: &mut App,
    then: impl FnOnce(String, &mut App) + 'static,
) {
    let asking = window.window_handle();
    choose_at(spec, PanelPlace::default(), window, cx, move |paths, cx| {
        let Some(p) = paths.first() else { return };
        if !sheet_is(asking, IMPORT_TITLE, cx) {
            return;
        }
        then(p.to_string_lossy().into_owned(), cx);
    });
}

/// The `Import into` choice: two radio buttons, the chosen one selected.
fn import_into(doc: DocId, new_project: bool, scope: &mut RegistryScope) -> AnyElement {
    let radio = |i: usize, title: &str, value: bool, scope: &mut RegistryScope| {
        let on = new_project == value;
        div()
            .id(("aaf-import-into", i))
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
                edit_import(doc, window, cx, |a| a.new_project = value);
                cx.stop_propagation();
            })
    };
    let mut group = scope.container();
    let radios = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(radio(0, OPEN_AS_NEW, true, &mut group))
        .child(radio(1, ADD_TO_THIS, false, &mut group));
    div()
        .relative()
        .flex()
        .flex_row()
        .gap_2()
        .child(div().flex_none().child(IMPORT_INTO))
        .child(radios)
        .child(group.this("radiogroup", IMPORT_INTO, true))
        .into_any_element()
}

impl Sheet for AafImportSheet {
    fn label(&self) -> String {
        IMPORT_TITLE.to_owned()
    }

    fn width(&self) -> f32 {
        WIDTH
    }

    fn render(
        &mut self,
        scope: &mut RegistryScope,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let doc = self.doc;
        let Some(a) = self.view(cx) else {
            return div().into_any_element();
        };
        self.sync_midi(&a, window, cx);
        let typed = self.midi.read(cx).text().to_owned();
        let focused = self.midi.focus_handle(cx).is_focused(window);
        let field = self.midi.clone();
        let midi_row =
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().relative().flex_1().child(self.midi.clone()).child(
                    scope.control_with("textfield", MIDI_BESIDE, true, Some(&typed), focused),
                ))
                .child(body_button(
                    0,
                    CHOOSE,
                    CHOOSE_MIDI_LABEL,
                    None,
                    scope,
                    move |window, cx| {
                        let field = field.clone();
                        choose_for_sheet(&panels::MIDI_BESIDE_AAF, window, cx, move |path, cx| {
                            field.update(cx, |t, cx| t.set_text(&path, cx));
                            with_doc(doc, cx, |d| d.edit_aaf_import(|a| a.midi_path = Some(path)));
                        });
                    },
                ));
        let folder_text = a
            .media_folder
            .as_deref()
            .map_or_else(|| NOWHERE_ELSE.to_owned(), folder_name);
        let mut folder_row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(text(LOOK_FOR_MISSING, scope).flex_none())
            .child(
                div()
                    .id("aaf-media-folder")
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(rgb(colors::dim()))
                    .child(text(&folder_text, scope))
                    .when_some(a.media_folder.clone(), with_help),
            )
            .child(body_button(
                1,
                CHOOSE,
                CHOOSE_FOLDER_LABEL,
                None,
                scope,
                move |window, cx| {
                    choose_for_sheet(&panels::AAF_MEDIA_FOLDER, window, cx, move |path, cx| {
                        with_doc(doc, cx, |d| {
                            d.edit_aaf_import(|a| a.media_folder = Some(path))
                        });
                    });
                },
            ));
        if a.media_folder.is_some() {
            folder_row = folder_row.child(body_button(
                2,
                CLEAR,
                CLEAR_LABEL,
                None,
                scope,
                move |window, cx| edit_import(doc, window, cx, |a| a.media_folder = None),
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(text(IMPORT_TITLE, scope).font_weight(gpui::FontWeight::BOLD))
            .child(
                text(&describe(a.info(), a.path()), scope)
                    .text_xs()
                    .text_color(rgb(colors::dim())),
            )
            .child(import_into(doc, a.new_project, scope))
            .child(midi_row)
            .child(folder_row)
            .child(toggle(
                ("aaf-join", 0),
                JOIN_PAIRS,
                JOIN_PAIRS,
                a.join_pairs,
                true,
                scope,
                move |window, cx| edit_import(doc, window, cx, |a| a.join_pairs = !a.join_pairs),
            ))
            .into_any_element()
    }

    fn buttons(&self, _: &App) -> Vec<SheetButton> {
        vec![
            SheetButton::new(CANCEL, ButtonRole::Cancel),
            SheetButton::new(IMPORT, ButtonRole::Default),
        ]
    }

    fn focus_order(&self, cx: &App) -> Vec<FocusHandle> {
        vec![self.midi.focus_handle(cx)]
    }

    fn press(&mut self, index: usize, window: &mut Window, cx: &mut App) -> bool {
        let doc = self.doc;
        if index == 0 {
            with_doc(doc, cx, |d| d.hide_aaf_import());
            return true;
        }
        // What is typed is what is sent, even if no frame drew since.
        if let Some(a) = self.view(cx) {
            self.sync_midi(&a, window, cx);
        }
        // A new project's window opens outside this window's update.
        cx.defer(move |cx| {
            let made = AppState::global(cx).update(cx, |s, cx| {
                cx.notify();
                s.store.start_aaf_import(doc)
            });
            crate::show(made, cx);
        });
        true
    }
}

/// File › Import Project › AAF… with `window` (a document's) in front:
/// panel #14, then the read of the file off the UI thread; the sheet
/// shows once it landed ([`present_import`]).
pub(crate) fn choose_import(doc: DocId, window: AnyWindowHandle, cx: &mut App) {
    let chosen =
        crate::file_panels::choose(&panels::IMPORT_AAF, PanelPlace::default(), Some(window), cx);
    cx.spawn(async move |cx| {
        let answer = chosen.await;
        cx.update(|cx| match answer {
            // The window that asked closed meanwhile: nothing starts.
            Ok(Some(_)) if !cx.windows().contains(&window) => {}
            Ok(Some(paths)) => {
                let Some(p) = paths.first() else { return };
                let path = p.to_string_lossy().into_owned();
                AppState::global(cx).update(cx, |s, cx| {
                    s.store.begin_aaf_import(doc, &path);
                    cx.notify();
                });
            }
            Ok(None) => {}
            Err(e) => crate::panel_failed(e, cx),
        });
    })
    .detach();
}

/// Shows each document's Export AAF sheet again when it asks (a refusal
/// or a failed job), takes it down once hidden; see
/// [`super::for_daw::present`].
pub(crate) fn present_export(cx: &mut App, free: impl Fn(AnyWindowHandle, &mut App) -> bool) {
    present_sheet(
        EXPORT_TITLE,
        |d| d.aaf_export().shown(),
        |doc, cx| Box::new(AafExportSheet::new(doc, cx)),
        cx,
        free,
    );
}

/// Shows each document's Import AAF sheet once its read landed, on one
/// free window of the document (the active one first), and takes it down
/// once hidden. Under the control channel nothing shows.
pub(crate) fn present_import(cx: &mut App, free: impl Fn(AnyWindowHandle, &mut App) -> bool) {
    present_sheet(
        IMPORT_TITLE,
        |d| d.aaf_import().is_some_and(AafImport::shown),
        |doc, cx| Box::new(AafImportSheet::new(doc, cx)),
        cx,
        free,
    );
}
