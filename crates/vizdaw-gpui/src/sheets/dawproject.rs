//! File › Export › DAWproject… (Swift `DawprojectSheet`, 480 wide): the
//! bold title and `Most complete: tracks, mix, plug-ins, automation`; the
//! `Region gain` picker `Apply to the audio` / `Leave out` (off while the
//! export waits or runs); `What won't transfer` from the core's check and
//! the error; while waiting `Waiting for the running job to finish…`;
//! the same buttons as Export for Another DAW (`Export…` off without a
//! check or the core's file name).
//!
//! The check runs on the background executor; only the newest answer
//! lands. Export… asks for the file (panel #13, the core's name; the app
//! adds `.dawproject` and asks the replace question itself when the
//! typed name lacked it, Cancel the default). `replace` is sent only when
//! the path was taken when the panel answered (one lstat, as Swift's
//! `confirmed`), so a file that appears while the export waits is the
//! core's refusal, never replaced unasked. The sheet closes when the job
//! ends; a refusal or a failed job brings it back with the message.
//! Never under the control channel.

use std::panic::AssertUnwindSafe;
use std::path::Path;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext, Entity, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled, Window, anchored, deferred, div, px, rgb,
};
use vizdaw_app::dawproject::{
    APPLY, EXPORT_FAILED, LEAVE_OUT, REGION_GAIN, SUBTITLE, TITLE, WAITING_TEXT,
};
use vizdaw_app::for_daw::PROGRESS_LABEL;
use vizdaw_app::interchange::WHAT_WONT_TRANSFER;
use vizdaw_app::{DocId, DocumentModel, panels};
use vizdaw_ffi::{RegionGainRow, ReportRow};

use super::audio_format::{progress_bar, row, text};
use super::export_midi::wont_transfer;
use super::for_daw::{error_row, export_buttons, present_sheet, waiting_row};
use crate::app_state::AppState;
use crate::file_panels::PanelPlace;
use crate::registry::RegistryScope;
use crate::sheet_host::{Sheet, SheetButton, sheet_is};
use crate::shell::DocumentWindow;
use crate::transport_bar::colors;

/// The card's width (Swift `.frame(width: 480)`).
const WIDTH: f32 = 480.;
/// The region gain picker's choices.
const GAINS: [(RegionGainRow, &str); 2] = [
    (RegionGainRow::Apply, APPLY),
    (RegionGainRow::LeaveOut, LEAVE_OUT),
];

/// What the sheet shows of the document, read once per frame.
struct View {
    region_gain: RegionGainRow,
    wont_transfer: Option<ReportRow>,
    error: Option<String>,
    checking: bool,
    default_name: String,
    waiting: bool,
    running: bool,
    progress: Option<f32>,
}

/// The Export DAWproject sheet of document `doc`.
pub struct DawprojectSheet {
    doc: DocId,
    /// The region gain picker is open.
    open: Entity<bool>,
}

impl DawprojectSheet {
    fn new(doc: DocId, cx: &mut App) -> Self {
        DawprojectSheet {
            doc,
            open: cx.new(|_| false),
        }
    }

    fn view(&self, cx: &App) -> Option<View> {
        let app = AppState::global(cx).read(cx);
        let d = app.store.document(self.doc)?;
        let e = d.dawproject();
        Some(View {
            region_gain: e.region_gain(),
            wont_transfer: e.wont_transfer().cloned(),
            error: e.error().map(str::to_owned),
            checking: e.checking(),
            default_name: e.default_name().to_owned(),
            waiting: e.waiting(),
            running: d.dawproject_running(),
            progress: d.dawproject_progress(),
        })
    }
}

/// Runs `f` on `doc` (a closed document does nothing).
fn with_doc(doc: DocId, cx: &mut App, f: impl FnOnce(&mut DocumentModel)) {
    AppState::global(cx).update(cx, |s, cx| {
        if let Some(d) = s.store.document_mut(doc) {
            f(d);
        }
        cx.notify();
    });
}

/// Runs `doc`'s waiting check on the background executor; only the
/// newest answer lands, and a closed document drops it.
fn run_check(doc: DocId, cx: &mut App) {
    let taken = AppState::global(cx).update(cx, |s, _| {
        let d = s.store.document_mut(doc)?;
        let check = d.take_dawproject_check()?;
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
        cx.update(|cx| with_doc(doc, cx, |d| d.land_dawproject_check(landed)));
    })
    .detach();
}

/// Whether something is at `path` (lstat: a dangling link counts as
/// taken).
fn taken(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Export…: the save panel with the core's name, then the export (or its
/// wait). A panel answered after the sheet went starts nothing.
fn begin(doc: DocId, name: String, window: &Window, cx: &mut App) {
    let asking = window.window_handle();
    let place = PanelPlace {
        name,
        ..PanelPlace::default()
    };
    let chosen =
        crate::file_panels::choose_save(&panels::DAWPROJECT_EXPORT_FILE, place, asking, cx);
    cx.spawn(async move |cx| {
        let answer = chosen.await;
        cx.update(|cx| match answer {
            Ok(Some(_)) if !sheet_is(asking, TITLE, cx) => {}
            // Confirmed: replace only what is there now.
            Ok(Some(path)) => {
                let replace = taken(&path);
                let path = path.to_string_lossy().into_owned();
                with_doc(doc, cx, |d| d.start_dawproject(&path, replace));
            }
            Ok(None) => {}
            Err(e) => AppState::global(cx).update(cx, |s, cx| {
                s.store
                    .post_alert(vizdaw_app::AlertItem::new(EXPORT_FAILED, e));
                cx.notify();
            }),
        });
    })
    .detach();
}

/// The `Region gain` picker: its face (`menu`), its items (`menuitem`)
/// while open.
fn picker(
    doc: DocId,
    gain: RegionGainRow,
    enabled: bool,
    open: &Entity<bool>,
    scope: &mut RegistryScope,
    cx: &App,
) -> AnyElement {
    let chosen = GAINS
        .iter()
        .find(|(g, _)| *g == gain)
        .map_or(APPLY, |(_, t)| t);
    let is_open = enabled && *open.read(cx);
    let flip = open.clone();
    let face = div()
        .id("dawproject-gain")
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
        .text_color(rgb(if enabled {
            colors::TEXT
        } else {
            colors::DISABLED
        }))
        .child(chosen)
        .child(div().text_color(rgb(colors::DIM)).child("\u{25BE}"))
        .child(scope.control_with("menu", REGION_GAIN, enabled, Some(chosen), false))
        .when(enabled, |d| {
            d.cursor_pointer().on_click(move |_, window, cx| {
                flip.update(cx, |o, _| *o = !*o);
                window.refresh();
            })
        });
    face.when(is_open, |d| {
        let rows = GAINS.iter().enumerate().map(|(i, (g, title))| {
            let open = open.clone();
            let g = *g;
            div()
                .id(("dawproject-gain-item", i))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::BUTTON)))
                .child(*title)
                .child(scope.control_selected("menuitem", title, true, None, g == gain))
                .on_click(move |_, window, cx| {
                    open.update(cx, |o, _| *o = false);
                    with_doc(doc, cx, |d| d.set_dawproject_region_gain(g));
                    window.refresh();
                    cx.stop_propagation();
                })
        });
        d.child(
            div().absolute().top_full().left_0().child(
                deferred(
                    anchored().snap_to_window().child(
                        div()
                            .id("dawproject-gain-list")
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

impl Sheet for DawprojectSheet {
    fn label(&self) -> String {
        TITLE.to_owned()
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
        let open = !v.waiting && !v.running;
        if !open {
            self.open.update(cx, |o, _| *o = false);
        }
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(text(TITLE, scope).font_weight(gpui::FontWeight::BOLD))
            .child(text(SUBTITLE, scope).text_xs().text_color(rgb(colors::DIM)))
            .child(row(
                REGION_GAIN,
                picker(doc, v.region_gain, open, &self.open, scope, cx),
            ))
            .child(text(WHAT_WONT_TRANSFER, scope));
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
        self.open.update(cx, |o, _| *o = false);
        let Some(v) = self.view(cx) else {
            return true;
        };
        if v.waiting {
            with_doc(doc, cx, |d| d.cancel_dawproject_waiting());
            return false;
        }
        if v.running {
            with_doc(doc, cx, |d| d.cancel_dawproject());
            return false;
        }
        if index == 0 {
            with_doc(doc, cx, |d| d.hide_dawproject());
            return true;
        }
        begin(doc, v.default_name, window, cx);
        false
    }
}

/// File › Export › DAWproject…: the sheet on `view`'s window (none while
/// another sheet is up, or under the control channel).
pub fn show(
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
        if d.dawproject().shown() {
            return None;
        }
        d.show_dawproject();
        Some(())
    });
    if shown.is_none() {
        return;
    }
    let sheet = DawprojectSheet::new(doc, cx);
    if view.show_sheet(Box::new(sheet), window, cx).is_err() {
        with_doc(doc, cx, |d| d.hide_dawproject());
    }
}

/// Shows each document's sheet again when it asks (a refusal or a failed
/// job), takes it down once hidden; see [`super::for_daw::present`].
pub(crate) fn present(cx: &mut App, free: impl Fn(AnyWindowHandle, &mut App) -> bool) {
    present_sheet(
        TITLE,
        |d| d.dawproject().shown(),
        |doc, cx| Box::new(DawprojectSheet::new(doc, cx)),
        cx,
        free,
    );
}
