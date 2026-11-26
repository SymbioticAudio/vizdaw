//! File › Export › For Another DAW… (Swift `ForDawSheet`, 480 wide): the
//! bold title and `Audio stems and MIDI files for any DAW`; `Stems`, a
//! toggle per strip but the master (`Stem for <strip>`); the format rows
//! shared with Bounce ([`super::audio_format`]); `Add the stems to this
//! project`; from the core's check where every file starts, `Length
//! <m:ss.t> + tail` and its warning; `What won't transfer` and the error.
//! While the export waits for a running job, `Waiting for the running
//! import, export or bounce to finish…` and `Cancel` (`Cancel the
//! waiting export`); while it runs its progress (`Export progress`) and
//! `Cancel` (`Cancel the export`); else `Cancel` and `Export…` (`Export,
//! then choose where`; off with no check). The choices are off while the
//! export waits or runs.
//!
//! Every change asks the check again on the background executor; only
//! the newest answer lands. Export… asks for a folder (panel #9c); the
//! core writes a new, numbered folder in it, so nothing there is written
//! over. The sheet closes when the job ends (the report sheet then shows
//! what was written); a refusal or a failed job brings it back with the
//! message ([`present`]), never an alert. Never under the control
//! channel.

use std::panic::AssertUnwindSafe;
use std::rc::Rc;

use gpui::{
    AnyElement, AnyWindowHandle, App, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use vizdaw_app::DocId;
use vizdaw_app::audio_format::AudioFormatChoices;
use vizdaw_app::for_daw::{
    ADD_STEMS, CANCEL, CANCEL_EXPORT_LABEL, CANCEL_WAITING_LABEL, EXPORT_LABEL, ForDawChoices,
    PROGRESS_LABEL, STEMS, SUBTITLE, TITLE, WAITING_LABEL, WAITING_TEXT, stem_label,
};
use vizdaw_app::interchange::{EXPORT, WHAT_WONT_TRANSFER};
use vizdaw_app::panels;
use vizdaw_ffi::{ForDawCheckRow, StripRow};

use super::audio_format::{EditFormat, FormatRows, progress_bar, text, toggle};
use super::export_midi::wont_transfer;
use crate::app_state::AppState;
use crate::file_menu::choose_at;
use crate::file_panels::PanelPlace;
use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton, sheet_is};
use crate::shell::DocumentWindow;
use crate::transport_bar::colors;

/// The card's width (Swift `.frame(width: 480)`).
const WIDTH: f32 = 480.;
/// The stems list's height (Swift `.frame(height: 120)`).
const STEMS_HEIGHT: f32 = 120.;
/// What the sheet shows of the document, read once per frame.
struct View {
    choices: ForDawChoices,
    strips: Vec<StripRow>,
    check: Option<ForDawCheckRow>,
    length: String,
    error: Option<String>,
    checking: bool,
    waiting: bool,
    running: bool,
    progress: Option<f32>,
}

/// The Export for Another DAW sheet of document `doc`.
pub struct ForDawSheet {
    doc: DocId,
    format: FormatRows,
}

impl ForDawSheet {
    fn new(doc: DocId, rate: u32, cx: &mut App) -> Self {
        ForDawSheet {
            doc,
            format: FormatRows::new(rate, cx),
        }
    }

    fn view(&self, cx: &App) -> Option<View> {
        let app = AppState::global(cx).read(cx);
        let d = app.store.document(self.doc)?;
        let e = d.for_daw();
        Some(View {
            choices: e.choices().clone(),
            strips: e.strips().to_vec(),
            check: e.check().cloned(),
            length: e.length(),
            error: e.error().map(str::to_owned),
            checking: e.checking(),
            waiting: e.waiting(),
            running: d.for_daw_running(),
            progress: d.for_daw_progress(),
        })
    }
}

/// Runs `f` on `doc` (a closed document does nothing).
fn with_doc(doc: DocId, cx: &mut App, f: impl FnOnce(&mut vizdaw_app::DocumentModel)) {
    AppState::global(cx).update(cx, |s, cx| {
        if let Some(d) = s.store.document_mut(doc) {
            f(d);
        }
        cx.notify();
    });
}

/// Edits `doc`'s choices (a change is checked again).
fn edit(doc: DocId, window: &mut Window, cx: &mut App, f: impl FnOnce(&mut ForDawChoices)) {
    with_doc(doc, cx, |d| d.edit_for_daw(f));
    window.refresh();
}

/// Runs `doc`'s waiting check on the background executor; its answer
/// lands on the main thread (only the newest one is kept; a closed
/// document drops it).
fn run_check(doc: DocId, cx: &mut App) {
    let taken = AppState::global(cx).update(cx, |s, _| {
        let d = s.store.document_mut(doc)?;
        let check = d.take_for_daw_check()?;
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
        cx.update(|cx| with_doc(doc, cx, |d| d.land_for_daw_check(landed)));
    })
    .detach();
}

/// Export…: the folder panel, then the export (or its wait for a running
/// job). A panel answered after the sheet went starts nothing.
fn begin(doc: DocId, window: &Window, cx: &mut App) {
    let asking = window.window_handle();
    choose_at(
        &panels::FOR_DAW_FOLDER,
        PanelPlace::default(),
        window,
        cx,
        move |paths, cx| {
            let Some(folder) = paths.first() else { return };
            if !sheet_is(asking, TITLE, cx) {
                return;
            }
            let folder = folder.to_string_lossy().into_owned();
            with_doc(doc, cx, |d| d.start_for_daw(&folder));
        },
    );
}

/// The spinner and text of an export waiting for a running job.
pub(crate) fn waiting_row(t: &str, scope: &mut RegistryScope) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .gap_2()
        .child(
            div()
                .relative()
                .text_color(rgb(colors::dim()))
                .child("\u{22EF}")
                .child(scope.control("progress", WAITING_LABEL, true)),
        )
        .child(text(t, scope).text_xs().text_color(rgb(colors::dim())))
        .into_any_element()
}

/// The error line.
pub(crate) fn error_row(e: &str, scope: &mut RegistryScope) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .gap_2()
        .text_color(rgb(colors::error()))
        .child(div().flex_none().child("\u{2716}"))
        .child(text(e, scope).flex_1())
        .into_any_element()
}

/// The buttons of an export sheet: waiting, Cancel the wait; running,
/// Cancel the job; else Cancel and Export… (off unless `ready`).
pub(crate) fn export_buttons(waiting: bool, running: bool, ready: bool) -> Vec<SheetButton> {
    if waiting {
        return vec![SheetButton::new(CANCEL, ButtonRole::Cancel).with_label(CANCEL_WAITING_LABEL)];
    }
    if running {
        return vec![SheetButton::new(CANCEL, ButtonRole::Cancel).with_label(CANCEL_EXPORT_LABEL)];
    }
    let export = SheetButton::new(EXPORT, ButtonRole::Default).with_label(EXPORT_LABEL);
    vec![
        SheetButton::new(CANCEL, ButtonRole::Cancel).with_label(CANCEL),
        if ready { export } else { export.disabled() },
    ]
}

impl Sheet for ForDawSheet {
    fn label(&self) -> String {
        TITLE.to_owned()
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
        let Some(v) = self.view(cx) else {
            return div().into_any_element();
        };
        run_check(doc, cx);
        // A kept or running export has its choices already.
        let open = !v.waiting && !v.running;
        let c = &v.choices;
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(text(TITLE, scope).font_weight(gpui::FontWeight::BOLD))
            .child(
                text(SUBTITLE, scope)
                    .text_xs()
                    .text_color(rgb(colors::dim())),
            )
            .child(text(STEMS, scope));
        let mut list = scope.container();
        let clip = list.clip();
        let rows: Vec<AnyElement> = v
            .strips
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let id = s.id;
                toggle(
                    ("for-daw-stem", i),
                    &s.name,
                    &stem_label(&s.name),
                    c.stems.contains(&id),
                    open,
                    &mut list,
                    move |window, cx| {
                        edit(doc, window, cx, |c| {
                            if !c.stems.remove(&id) {
                                c.stems.insert(id);
                            }
                        })
                    },
                )
            })
            .collect();
        body = body.child(
            div()
                .id("for-daw-stems")
                .relative()
                .h(px(STEMS_HEIGHT))
                .overflow_y_scroll()
                .border_1()
                .border_color(rgb(colors::border()))
                .rounded_md()
                .px_2()
                .py_1()
                .child(div().flex().flex_col().gap_1().children(rows))
                .child(clip),
        );
        let edit_format: EditFormat = Rc::new(move |window, cx, f| {
            edit(doc, window, cx, |c| c.edit_audio_format(f));
        });
        body = body.children(self.format.render(
            &c.audio_format(),
            open,
            &edit_format,
            scope,
            window,
            cx,
        ));
        body = body.child(toggle(
            ("for-daw-add", 0),
            ADD_STEMS,
            ADD_STEMS,
            c.add_to_project,
            open,
            scope,
            move |window, cx| edit(doc, window, cx, |c| c.add_to_project = !c.add_to_project),
        ));
        if let Some(check) = &v.check {
            body = body
                .child(
                    text(&check.start, scope)
                        .text_xs()
                        .text_color(rgb(colors::dim())),
                )
                .child(
                    text(&format!("Length {}", v.length), scope)
                        .text_xs()
                        .text_color(rgb(colors::dim())),
                );
            if let Some(w) = &check.warning {
                body = body.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            div()
                                .flex_none()
                                .text_color(rgb(colors::warning()))
                                .child("\u{26A0}"),
                        )
                        .child(text(w, scope).flex_1()),
                );
            }
        }
        body = body.child(text(WHAT_WONT_TRANSFER, scope));
        if let Some(w) = wont_transfer(v.check.as_ref().map(|c| &c.report), v.checking, scope) {
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
            Some(v) => export_buttons(v.waiting, v.running, v.check.is_some()),
            None => export_buttons(false, false, false),
        }
    }

    fn focus_order(&self, cx: &App) -> Vec<FocusHandle> {
        match self.view(cx) {
            Some(v) if !v.waiting && !v.running => {
                self.format.focus_order(&v.choices.audio_format(), cx)
            }
            _ => vec![],
        }
    }

    fn press(&mut self, index: usize, window: &mut Window, cx: &mut App) -> bool {
        let doc = self.doc;
        self.format.close(cx);
        let Some(v) = self.view(cx) else {
            return true;
        };
        if v.waiting {
            with_doc(doc, cx, |d| d.cancel_for_daw_waiting());
            return false;
        }
        if v.running {
            // The sheet closes once the job ended.
            with_doc(doc, cx, |d| d.cancel_for_daw());
            return false;
        }
        if index == 0 {
            with_doc(doc, cx, |d| d.hide_for_daw());
            return true;
        }
        begin(doc, window, cx);
        false
    }
}

/// File › Export › For Another DAW…: the sheet on `view`'s window (none
/// while another sheet is up, or under the control channel). An export
/// waiting or running keeps its choices.
pub fn show(
    view: &mut DocumentWindow,
    window: &mut Window,
    cx: &mut gpui::Context<DocumentWindow>,
) {
    if crate::control::channel_on(cx) || view.sheet_label().is_some() {
        return;
    }
    let doc = view.doc();
    let rate = AppState::global(cx).update(cx, |s, cx| {
        cx.notify();
        let d = s.store.document_mut(doc)?;
        // Up on another window of the document already: it stays there.
        if d.for_daw().shown() {
            return None;
        }
        d.show_for_daw();
        Some(d.for_daw().choices().format.rate)
    });
    let Some(rate) = rate else { return };
    let sheet = ForDawSheet::new(doc, rate, cx);
    if view.show_sheet(Box::new(sheet), window, cx).is_err() {
        with_doc(doc, cx, |d| d.hide_for_daw());
    }
}

/// Shows each document's sheet again when the document asks for it (a
/// refusal or a failed job brings it back with the message), on one free
/// window (`free`) of the document, the active one first; takes it down
/// once the document hid it (the job ended). Under the control channel
/// nothing shows. Call it outside any update.
pub(crate) fn present(cx: &mut App, free: impl Fn(AnyWindowHandle, &mut App) -> bool) {
    present_sheet(
        TITLE,
        |d| d.for_daw().shown(),
        |doc, cx| {
            let rate = AppState::global(cx)
                .read(cx)
                .store
                .document(doc)
                .map_or(48_000, |d| d.for_daw().choices().format.rate);
            Box::new(ForDawSheet::new(doc, rate, cx))
        },
        cx,
        free,
    );
}

/// [`present`] for the sheet titled `title`, wanted while `wanted` and
/// made by `make`.
pub(crate) fn present_sheet(
    title: &str,
    wanted: impl Fn(&vizdaw_app::DocumentModel) -> bool,
    make: impl Fn(DocId, &mut App) -> Box<dyn Sheet>,
    cx: &mut App,
    free: impl Fn(AnyWindowHandle, &mut App) -> bool,
) {
    let channel = crate::control::channel_on(cx);
    let app = AppState::global(cx);
    let active = cx.active_window();
    let mut windows: Vec<(DocId, gpui::WindowHandle<DocumentWindow>)> = app
        .read(cx)
        .document_windows()
        .filter_map(|w| w.document_window().map(|h| (w.doc, h)))
        .collect();
    windows.sort_by_key(|(_, h)| Some(AnyWindowHandle::from(*h)) != active);
    let wants = |doc: DocId, cx: &App| {
        !channel
            && AppState::global(cx)
                .read(cx)
                .store
                .document(doc)
                .is_some_and(&wanted)
    };
    let mut placed: Vec<DocId> = vec![];
    for (doc, handle) in &windows {
        let label = handle.update(cx, |v, _, _| v.sheet_label()).ok().flatten();
        if label.as_deref() != Some(title) {
            continue;
        }
        if wants(*doc, cx) && !placed.contains(doc) {
            placed.push(*doc);
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
        let sheet = make(doc, cx);
        let shown = handle
            .update(cx, |v, window, cx| v.show_sheet(sheet, window, cx).is_ok())
            .unwrap_or(false);
        if shown {
            placed.push(doc);
        }
    }
}
