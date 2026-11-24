//! File › Bounce… (Swift `BounceSheet`, 440 wide): the bold title; the
//! `Bounce` picker `Mix`/`Stems` (off while the bounce runs); for stems a
//! toggle per strip; the captions; `Time selection only` (off without
//! one); the Format, Bits, Rate (`Other…` opens the `Rate (Hz)` field) and
//! Dither rows; `Add to project` and `Also write the files`; the core's
//! warning, `Length <m:ss.t> + tail`, the error and, while the bounce
//! runs, its progress. `Cancel` and `Bounce` (off with stems and none
//! chosen); while it runs only `Cancel`, which cancels the job.
//!
//! Bounce asks where first: stems choose a folder (panel #9a), a mix a
//! file always named `Bounce.<wav|aif|caf>` (panel #8; the app adds the
//! extension and asks the replace question itself when the typed name
//! lacked it); `Add to project` without the files asks nothing. The
//! check of the choices runs on the background executor; only its newest
//! answer lands ([`vizdaw_app::DocumentModel::land_bounce_check`]). The
//! sheet closes when the job ends; a failure is the alert "Bounce Failed"
//! (it waits for the sheet), what it wrote is the status line. Typing in
//! the `Rate (Hz)` field turns the document's keys off (the sheet host's
//! per-window owner). Never under the control channel.

use std::panic::AssertUnwindSafe;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use vizdaw_app::DocId;
use vizdaw_app::audio_format::AudioFormatChoices;
use vizdaw_app::bounce::{
    ADD_TO_PROJECT, ALSO_WRITE_FILES, BOUNCE, BounceChoices, CANCEL, CAPTION, FAILED, MIX, STEMS,
    STEMS_CAPTION, Source, TIME_SELECTION_ONLY, TITLE, extension,
};
use vizdaw_app::panels;
use vizdaw_ffi::StripRow;

use super::audio_format::{EditFormat, FormatRows, progress_bar, row, text};
use crate::app_state::AppState;
use crate::file_menu::choose_at;
use crate::file_panels::PanelPlace;
use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton, sheet_is};
use crate::shell::DocumentWindow;
use crate::transport_bar::colors;

/// The card's width (Swift `.frame(width: 440)`).
const WIDTH: f32 = 440.;
/// The stems list's height (Swift `.frame(height: 120)`).
const STEMS_HEIGHT: f32 = 120.;
/// The accessibility label of the sheet's progress bar.
pub const PROGRESS_LABEL: &str = "Bounce progress";
/// The warning glyph's colour.
const WARNING_TEXT: u32 = 0xd8a040;
/// The error's colour.
const ERROR_TEXT: u32 = 0xe06060;

/// What the sheet shows of the document, read once per frame.
struct View {
    choices: BounceChoices,
    strips: Vec<StripRow>,
    warning: Option<String>,
    error: Option<String>,
    length: String,
    job: bool,
    progress: Option<f32>,
    shown: bool,
    has_selection: bool,
}

/// The Bounce sheet of document `doc`.
pub struct BounceSheet {
    doc: DocId,
    /// The Format, Bits, Rate and Dither rows.
    format: FormatRows,
}

impl BounceSheet {
    fn new(doc: DocId, rate: u32, cx: &mut App) -> Self {
        BounceSheet {
            doc,
            format: FormatRows::new(rate, cx),
        }
    }

    fn view(&self, cx: &App) -> Option<View> {
        let app = AppState::global(cx).read(cx);
        let d = app.store.document(self.doc)?;
        let b = d.bounce();
        Some(View {
            choices: b.choices().clone(),
            strips: b.strips().to_vec(),
            warning: b.warning().map(str::to_owned),
            error: b.error().map(str::to_owned),
            length: b.length().to_owned(),
            job: b.job().is_some(),
            progress: d.bounce_progress(),
            shown: b.shown(),
            has_selection: d.bounce_has_selection(),
        })
    }
}

/// Edits `doc`'s choices (a change is checked again).
fn edit(doc: DocId, window: &mut Window, cx: &mut App, f: impl FnOnce(&mut BounceChoices)) {
    AppState::global(cx).update(cx, |s, cx| {
        if let Some(d) = s.store.document_mut(doc) {
            d.edit_bounce(f);
        }
        cx.notify();
    });
    window.refresh();
}

/// Runs `doc`'s waiting check on the background executor; its answer
/// lands on the main thread (only the newest one is kept).
fn run_check(doc: DocId, cx: &mut App) {
    let app = AppState::global(cx);
    let taken = app.update(cx, |s, _| {
        let d = s.store.document_mut(doc)?;
        let check = d.take_bounce_check()?;
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
                    d.land_bounce_check(landed);
                    cx.notify();
                }
            });
        });
    })
    .detach();
}

/// Starts the bounce on `doc` at `path`.
fn start(doc: DocId, path: Option<String>, replace: bool, cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| {
        if let Some(d) = s.store.document_mut(doc) {
            d.start_bounce(path.as_deref(), replace);
        }
        cx.notify();
    });
}

/// Bounce: where to (a folder for stems, `Bounce.<ext>` for a mix,
/// nothing for Add to project alone), then the job.
fn begin(doc: DocId, c: &BounceChoices, window: &Window, cx: &mut App) {
    if !c.needs_destination() {
        start(doc, None, false, cx);
        return;
    }
    // The panel answers after its sheet may have gone (its window closed,
    // or the sheet was dismissed): then nothing starts.
    let asking = window.window_handle();
    if c.source == Source::Stems {
        choose_at(
            &panels::BOUNCE_FOLDER,
            PanelPlace::default(),
            window,
            cx,
            move |paths, cx| {
                let Some(folder) = paths.first() else { return };
                if !sheet_is(asking, TITLE, cx) {
                    return;
                }
                start(doc, Some(folder.to_string_lossy().into_owned()), false, cx);
            },
        );
        return;
    }
    let place = PanelPlace {
        ext: extension(c.container).to_owned(),
        ..PanelPlace::default()
    };
    let chosen = crate::file_panels::choose_save(&panels::BOUNCE_FILE, place, asking, cx);
    cx.spawn(async move |cx| {
        let answer = chosen.await;
        cx.update(|cx| match answer {
            Ok(Some(_)) if !sheet_is(asking, TITLE, cx) => {}
            // The panel (or the app's own question) confirmed a replace.
            Ok(Some(path)) => start(doc, Some(path.to_string_lossy().into_owned()), true, cx),
            Ok(None) => {}
            Err(e) => AppState::global(cx).update(cx, |s, cx| {
                s.store.post_alert(vizdaw_app::AlertItem::new(FAILED, e));
                cx.notify();
            }),
        });
    })
    .detach();
}

/// A toggle titled and labelled `label`.
fn toggle(
    id: (&'static str, usize),
    label: &str,
    on: bool,
    enabled: bool,
    scope: &mut RegistryScope,
    flip: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    super::audio_format::toggle(id, label, label, on, enabled, scope, flip)
}

/// The `Mix`/`Stems` segments (`button`s, the chosen one selected).
fn segments(doc: DocId, source: Source, enabled: bool, scope: &mut RegistryScope) -> AnyElement {
    let seg = |i: usize, s: Source, title: &str, scope: &mut RegistryScope| {
        let on = source == s;
        div()
            .id(("bounce-source", i))
            .relative()
            .h(px(22.))
            .px_3()
            .flex()
            .items_center()
            .bg(rgb(if on { colors::ON } else { colors::BUTTON }))
            .text_color(rgb(if enabled {
                colors::TEXT
            } else {
                colors::DISABLED
            }))
            .child(title.to_owned())
            .child(scope.control_selected("button", title, enabled, None, on))
            .when(enabled, |d| {
                d.cursor_pointer().on_click(move |_, window, cx| {
                    edit(doc, window, cx, |c| c.source = s);
                    cx.stop_propagation();
                })
            })
    };
    div()
        .flex()
        .flex_row()
        .rounded_md()
        .overflow_hidden()
        .border_1()
        .border_color(rgb(colors::BORDER))
        .child(seg(0, Source::Mix, MIX, scope))
        .child(seg(1, Source::Stems, STEMS, scope))
        .into_any_element()
}

impl Sheet for BounceSheet {
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
        // The job ended (or the document asked the sheet away): it closes.
        if !v.shown {
            let handle = window.window_handle().downcast::<DocumentWindow>();
            cx.defer(move |cx| {
                if let Some(h) = handle {
                    h.update(cx, |w, window, cx| {
                        if w.sheet_label().as_deref() == Some(TITLE) {
                            w.close_sheet(window, cx);
                        }
                    })
                    .ok();
                }
            });
        }
        run_check(doc, cx);
        let c = &v.choices;
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(text(TITLE, scope).font_weight(gpui::FontWeight::BOLD))
            .child(row(TITLE, segments(doc, c.source, !v.job, scope)));
        if c.source == Source::Stems {
            let mut list = scope.container();
            let clip = list.clip();
            let rows: Vec<AnyElement> = v
                .strips
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let id = s.id;
                    toggle(
                        ("bounce-stem", i),
                        &s.name,
                        c.stems.contains(&id),
                        true,
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
                    .id("bounce-stems")
                    .relative()
                    .h(px(STEMS_HEIGHT))
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(rgb(colors::BORDER))
                    .rounded_md()
                    .px_2()
                    .py_1()
                    .child(div().flex().flex_col().gap_1().children(rows))
                    .child(clip),
            );
        }
        body = body.child(text(CAPTION, scope).text_xs().text_color(rgb(colors::DIM)));
        if c.source == Source::Stems {
            body = body.child(
                text(STEMS_CAPTION, scope)
                    .text_xs()
                    .text_color(rgb(colors::DIM)),
            );
        }
        body = body.child(toggle(
            ("bounce-toggle", 0),
            TIME_SELECTION_ONLY,
            c.use_selection,
            v.has_selection,
            scope,
            move |window, cx| edit(doc, window, cx, |c| c.use_selection = !c.use_selection),
        ));
        let edit_format: EditFormat = Rc::new(move |window, cx, f| {
            edit(doc, window, cx, |c| c.edit_audio_format(f));
        });
        body = body.children(self.format.render(
            &c.audio_format(),
            true,
            &edit_format,
            scope,
            window,
            cx,
        ));
        body = body.child(toggle(
            ("bounce-toggle", 2),
            ADD_TO_PROJECT,
            c.add_to_project,
            true,
            scope,
            move |window, cx| edit(doc, window, cx, |c| c.add_to_project = !c.add_to_project),
        ));
        if c.add_to_project {
            body = body.child(toggle(
                ("bounce-toggle", 3),
                ALSO_WRITE_FILES,
                c.write_files,
                true,
                scope,
                move |window, cx| edit(doc, window, cx, |c| c.write_files = !c.write_files),
            ));
        }
        if let Some(w) = &v.warning {
            body = body.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(WARNING_TEXT))
                            .child("\u{26A0}"),
                    )
                    .child(text(w, scope).flex_1()),
            );
        }
        if !v.length.is_empty() {
            let length = format!("Length {}", v.length);
            body = body.child(text(&length, scope).text_xs().text_color(rgb(colors::DIM)));
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
        if let Some(p) = v.progress {
            body = body.child(progress_bar(p, PROGRESS_LABEL, scope));
        }
        body.into_any_element()
    }

    fn buttons(&self, cx: &App) -> Vec<SheetButton> {
        let Some(v) = self.view(cx) else {
            return vec![SheetButton::new(CANCEL, ButtonRole::Cancel)];
        };
        if v.job {
            return vec![SheetButton::new(CANCEL, ButtonRole::Cancel)];
        }
        let bounce = SheetButton::new(BOUNCE, ButtonRole::Default);
        let none = v.choices.source == Source::Stems && v.choices.stems.is_empty();
        vec![
            SheetButton::new(CANCEL, ButtonRole::Cancel),
            if none { bounce.disabled() } else { bounce },
        ]
    }

    fn focus_order(&self, cx: &App) -> Vec<FocusHandle> {
        match self.view(cx) {
            Some(v) => self.format.focus_order(&v.choices.audio_format(), cx),
            None => vec![],
        }
    }

    fn press(&mut self, index: usize, window: &mut Window, cx: &mut App) -> bool {
        let doc = self.doc;
        let Some(v) = self.view(cx) else {
            return true;
        };
        let app = AppState::global(cx);
        if v.job {
            // Cancel stops the job; the sheet closes once it ended.
            app.update(cx, |s, cx| {
                if let Some(d) = s.store.document_mut(doc) {
                    d.cancel_bounce();
                }
                cx.notify();
            });
            return false;
        }
        if index == 0 {
            app.update(cx, |s, cx| {
                if let Some(d) = s.store.document_mut(doc) {
                    d.hide_bounce();
                }
                cx.notify();
            });
            return true;
        }
        self.format.close(cx);
        begin(doc, &v.choices, window, cx);
        false
    }
}

/// File › Bounce…: the sheet on `view`'s window, its choices reset to the
/// project's format (none while another sheet is up, or under the control
/// channel).
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
        d.show_bounce();
        Some(d.bounce().choices().rate)
    });
    let Some(rate) = rate else { return };
    let sheet = BounceSheet::new(doc, rate, cx);
    if view.show_sheet(Box::new(sheet), window, cx).is_err() {
        AppState::global(cx).update(cx, |s, _| {
            if let Some(d) = s.store.document_mut(doc) {
                d.hide_bounce();
            }
        });
    }
}
