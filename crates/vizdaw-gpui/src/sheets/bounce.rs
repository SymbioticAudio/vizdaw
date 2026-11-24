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

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled, Window, anchored, deferred, div, px,
    relative, rgb,
};
use vizdaw_app::DocId;
use vizdaw_app::bounce::{
    ADD_TO_PROJECT, ALSO_WRITE_FILES, BITS, BOUNCE, BounceChoices, CANCEL, CAPTION, CONTAINERS,
    DEPTHS, DITHER, FAILED, FORMAT, MIX, OTHER, OTHER_LABEL, OTHER_PLACEHOLDER, RATE, RATES, STEMS,
    STEMS_CAPTION, Source, TIME_SELECTION_ONLY, TITLE, extension, rate_title,
};
use vizdaw_app::panels;
use vizdaw_ffi::{BitDepthRow, StripRow};

use crate::app_state::AppState;
use crate::file_menu::choose_at;
use crate::file_panels::PanelPlace;
use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton, sheet_is};
use crate::shell::DocumentWindow;
use crate::text_input::TextInput;
use crate::transport_bar::colors;

/// The card's width (Swift `.frame(width: 440)`).
const WIDTH: f32 = 440.;
/// The width of a row's label column.
const LABEL_WIDTH: f32 = 90.;
/// The stems list's height (Swift `.frame(height: 120)`).
const STEMS_HEIGHT: f32 = 120.;
/// The accessibility label of the sheet's progress bar.
pub const PROGRESS_LABEL: &str = "Bounce progress";
/// The warning glyph's colour.
const WARNING_TEXT: u32 = 0xd8a040;
/// The error's colour.
const ERROR_TEXT: u32 = 0xe06060;

/// Which picker is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Format,
    Bits,
    Rate,
}

/// What the sheet's controls share with their click handlers.
struct BounceUi {
    open: Option<Pick>,
}

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
    ui: Entity<BounceUi>,
    /// The `Rate (Hz)` field.
    other: Entity<TextInput>,
}

impl BounceSheet {
    fn new(doc: DocId, rate: u32, cx: &mut App) -> Self {
        let text = rate.to_string();
        BounceSheet {
            doc,
            ui: cx.new(|_| BounceUi { open: None }),
            other: cx.new(|cx| {
                TextInput::new(OTHER_PLACEHOLDER, cx)
                    .compact()
                    .with_text(&text)
            }),
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

/// A text line recorded in the tree.
fn text(t: &str, scope: &mut RegistryScope) -> gpui::Div {
    div()
        .relative()
        .child(t.to_owned())
        .child(scope.control("text", t, true))
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

/// A toggle: its box and label (`checkbox` in the tree, selected when
/// on); a click flips it through `flip`.
fn toggle(
    id: (&'static str, usize),
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
        .child(label.to_owned())
        .child(scope.control_selected("checkbox", label, enabled, None, on))
        .when(enabled, |d| {
            d.cursor_pointer().on_click(move |_, window, cx| {
                flip(window, cx);
                cx.stop_propagation();
            })
        })
        .into_any_element()
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

/// Picker `which`'s items and the chosen one's title.
fn items(which: Pick, c: &BounceChoices) -> (Vec<String>, String) {
    match which {
        Pick::Format => (
            CONTAINERS.iter().map(|(_, t)| (*t).to_owned()).collect(),
            CONTAINERS
                .iter()
                .find(|(k, _)| *k == c.container)
                .map_or("", |(_, t)| t)
                .to_owned(),
        ),
        Pick::Bits => (
            DEPTHS.iter().map(|(_, t)| (*t).to_owned()).collect(),
            DEPTHS
                .iter()
                .find(|(k, _)| *k == c.depth)
                .map_or("", |(_, t)| t)
                .to_owned(),
        ),
        Pick::Rate => {
            let mut all: Vec<String> = RATES.iter().map(|r| rate_title(*r)).collect();
            all.push(OTHER.to_owned());
            let chosen = if c.other_rate {
                OTHER.to_owned()
            } else {
                rate_title(c.rate)
            };
            (all, chosen)
        }
    }
}

/// Item `i` of picker `which`, chosen.
fn choose(which: Pick, i: usize, c: &mut BounceChoices) {
    match which {
        Pick::Format => c.container = CONTAINERS[i].0,
        Pick::Bits => c.depth = DEPTHS[i].0,
        Pick::Rate => c.set_rate_choice(RATES.get(i).copied().unwrap_or(0)),
    }
}

/// Picker `which` labelled `label`: its face (`menu`, its value the
/// choice), and its items (`menuitem`) while open.
fn picker(
    doc: DocId,
    which: Pick,
    label: &str,
    c: &BounceChoices,
    ui: &Entity<BounceUi>,
    scope: &mut RegistryScope,
    cx: &App,
) -> AnyElement {
    let (all, chosen) = items(which, c);
    let open = ui.read(cx).open == Some(which);
    let toggle_ui = ui.clone();
    let face = div()
        .id(("bounce-picker", which as usize))
        .relative()
        .h(px(24.))
        .min_w(px(120.))
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
                .id(("bounce-item", i))
                .relative()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors::BUTTON)))
                .child(title.clone())
                .child(scope.control_selected("menuitem", title, true, None, *title == chosen))
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
                            .id("bounce-list")
                            .occlude()
                            .flex()
                            .flex_col()
                            .min_w(px(120.))
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

/// The progress bar (0–1), its value the progress.
fn progress_bar(p: f32, scope: &mut RegistryScope) -> AnyElement {
    let p = p.clamp(0., 1.);
    let value = format!("{p:.2}");
    div()
        .relative()
        .w_full()
        .h(px(6.))
        .rounded_sm()
        .bg(rgb(colors::READOUT))
        .child(
            div()
                .h_full()
                .w(relative(p))
                .rounded_sm()
                .bg(rgb(colors::ON)),
        )
        .child(scope.control_with("progress", PROGRESS_LABEL, true, Some(&value), false))
        .into_any_element()
}

impl BounceSheet {
    /// The `Rate (Hz)` field follows the choice: what is typed sets the
    /// rate (clamped to 8,000–384,000); left, it shows the rate.
    fn sync_other(&self, c: &BounceChoices, window: &mut Window, cx: &mut App) {
        let focused = self.other.focus_handle(cx).is_focused(window);
        let typed = self.other.read(cx).text().trim().to_owned();
        if focused {
            if let Ok(n) = typed.parse::<i64>() {
                let mut next = c.clone();
                next.set_other_rate(n);
                if next.rate != c.rate {
                    edit(self.doc, window, cx, |c| c.set_other_rate(n));
                }
            }
        } else if typed != c.rate.to_string() {
            let rate = c.rate.to_string();
            self.other.update(cx, |f, cx| f.set_text(&rate, cx));
        }
    }
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
        if c.other_rate {
            self.sync_other(c, window, cx);
        }
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
        let ui = self.ui.clone();
        body = body
            .child(row(
                FORMAT,
                picker(doc, Pick::Format, FORMAT, c, &ui, scope, cx),
            ))
            .child(row(BITS, picker(doc, Pick::Bits, BITS, c, &ui, scope, cx)))
            .child(row(RATE, picker(doc, Pick::Rate, RATE, c, &ui, scope, cx)));
        if c.other_rate {
            let typed = self.other.read(cx).text().to_owned();
            let focused = self.other.focus_handle(cx).is_focused(window);
            body = body.child(row(
                "",
                div()
                    .relative()
                    .w(px(120.))
                    .child(self.other.clone())
                    .child(scope.control_with(
                        "textfield",
                        OTHER_LABEL,
                        true,
                        Some(&typed),
                        focused,
                    )),
            ));
        }
        body = body.child(toggle(
            ("bounce-toggle", 1),
            DITHER,
            c.dither,
            c.depth != BitDepthRow::Float32,
            scope,
            move |window, cx| edit(doc, window, cx, |c| c.dither = !c.dither),
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
            body = body.child(progress_bar(p, scope));
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
            Some(v) if v.choices.other_rate => vec![self.other.focus_handle(cx)],
            _ => vec![],
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
        self.ui.update(cx, |u, _| u.open = None);
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
