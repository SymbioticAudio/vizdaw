//! Relink Media… (Swift `RelinkSheet`, 540 wide): the bold title, the line
//! under it, then each missing or changed item — a warning glyph, its
//! name, `<reason> · <path>` and `Locate…` (panel #5, its accessibility
//! label `Locate <name>`) — then `Search Folder…` (panel #6) and `Leave
//! Offline` (Esc). Each relink is a `Relink` job; its banner shows on the
//! window and the sheet stays up until nothing is left to relink or Leave
//! Offline. While a DAWproject merge is held Locate… and Search Folder…
//! are off, with Swift's help.
//!
//! The sheet is offered by itself ([`present`]): when the project opens
//! with such items and when idle or a recheck finds new ones (each item
//! once); never under the control channel.

use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, Window, div, px, rgb,
};
use vizdaw_app::DocId;
use vizdaw_app::panels;
use vizdaw_app::relink::{
    HELD_HELP, LEAVE_OFFLINE, LOCATE, MESSAGE, SEARCH_FOLDER, TITLE, detail, locate_label,
};
use vizdaw_ffi::MediaRow;

use crate::app_state::AppState;
use crate::file_menu::choose_at;
use crate::file_panels::PanelPlace;
use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton, button_face};
use crate::shell::DocumentWindow;
use crate::transport_bar::colors;

/// The card's width (Swift `.frame(width: 540)`).
const WIDTH: f32 = 540.;
/// The list's least height (Swift `.frame(minHeight: 160)`).
const LIST_MIN_HEIGHT: f32 = 160.;
/// How tall the list may grow before it scrolls.
const LIST_MAX_HEIGHT: f32 = 320.;
/// The warning glyph's colour (the theme's offline text).
const OFFLINE_TEXT: u32 = 0xd8a040;

/// The Relink sheet of document `doc`.
pub struct RelinkSheet {
    doc: DocId,
}

impl RelinkSheet {
    pub fn new(doc: DocId) -> Self {
        RelinkSheet { doc }
    }

    /// The items to relink and whether a held merge blocks relinking.
    fn state(&self, cx: &App) -> (Vec<MediaRow>, bool) {
        let app = AppState::global(cx).read(cx);
        app.store
            .document(self.doc)
            .map(|d| (d.relinkable(), d.relink_blocked()))
            .unwrap_or_default()
    }
}

/// Locate…: panel #5 for `row`, then the relink job.
fn locate(doc: DocId, row: &MediaRow, window: &Window, cx: &mut App) {
    let media = row.id;
    let place = PanelPlace {
        name: row.name.clone(),
        ..PanelPlace::default()
    };
    choose_at(&panels::RELINK, place, window, cx, move |paths, cx| {
        let Some(path) = paths.first() else { return };
        let path = path.to_string_lossy().into_owned();
        AppState::global(cx).update(cx, |s, cx| {
            s.store.with_document(doc, |d, _| d.relink(media, &path));
            cx.notify();
        });
    });
}

/// Search Folder…: panel #6, then the search job.
fn search(doc: DocId, window: &Window, cx: &mut App) {
    choose_at(
        &panels::SEARCH_FOLDER,
        PanelPlace::default(),
        window,
        cx,
        move |paths, cx| {
            let Some(folder) = paths.first() else { return };
            let folder = folder.to_string_lossy().into_owned();
            AppState::global(cx).update(cx, |s, cx| {
                s.store.with_document(doc, |d, _| d.search_folder(&folder));
                cx.notify();
            });
        },
    );
}

/// A held merge's button: off, with Swift's help.
fn held(b: SheetButton, blocked: bool) -> SheetButton {
    if blocked {
        b.disabled().with_help(HELD_HELP)
    } else {
        b
    }
}

/// One item's row: the glyph, the name over `<reason> · <path>`, and
/// Locate….
fn item_row(
    doc: DocId,
    i: usize,
    r: &MediaRow,
    blocked: bool,
    scope: &mut RegistryScope,
) -> AnyElement {
    let label = locate_label(&r.name);
    let b = held(SheetButton::new(LOCATE, ButtonRole::Plain), blocked);
    let mut face = button_face(i, &b).child(scope.control_with(
        "button",
        &label,
        b.enabled,
        b.help.as_deref(),
        false,
    ));
    if b.enabled {
        let row = r.clone();
        face = face.on_click(move |_, window, cx| {
            locate(doc, &row, window, cx);
            cx.stop_propagation();
        });
    }
    let name = r.name.clone();
    let line = detail(r);
    div()
        .id(("relink-row", i))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .py_1()
        .child(
            div()
                .flex_none()
                .text_color(rgb(OFFLINE_TEXT))
                .child("\u{26A0}"),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .relative()
                        .child(name.clone())
                        .child(scope.control("text", &name, true)),
                )
                .child(
                    div()
                        .relative()
                        .text_xs()
                        .text_color(rgb(colors::DIM))
                        .child(line.clone())
                        .child(scope.control("text", &line, true)),
                ),
        )
        .child(face)
        .into_any_element()
}

impl Sheet for RelinkSheet {
    fn label(&self) -> String {
        TITLE.to_owned()
    }

    fn width(&self) -> f32 {
        WIDTH
    }

    fn render(&mut self, scope: &mut RegistryScope, _: &mut Window, cx: &mut App) -> AnyElement {
        let (rows, blocked) = self.state(cx);
        let title = div()
            .relative()
            .font_weight(gpui::FontWeight::BOLD)
            .child(TITLE)
            .child(scope.control("text", TITLE, true));
        let message = div()
            .relative()
            .text_color(rgb(colors::DIM))
            .child(MESSAGE)
            .child(scope.control("text", MESSAGE, true));
        let mut list_scope = scope.container();
        let clip = list_scope.clip();
        let items: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| item_row(self.doc, i, r, blocked, &mut list_scope.container()))
            .collect();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(title)
            .child(message)
            .child(
                div()
                    .id("relink-list")
                    .relative()
                    .min_h(px(LIST_MIN_HEIGHT))
                    .max_h(px(LIST_MAX_HEIGHT))
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(rgb(colors::BORDER))
                    .rounded_md()
                    .px_2()
                    .child(div().flex().flex_col().children(items))
                    .child(clip),
            )
            .into_any_element()
    }

    fn buttons(&self, cx: &App) -> Vec<SheetButton> {
        let (_, blocked) = self.state(cx);
        vec![
            held(SheetButton::new(SEARCH_FOLDER, ButtonRole::Plain), blocked),
            SheetButton::new(LEAVE_OFFLINE, ButtonRole::Cancel),
        ]
    }

    fn press(&mut self, index: usize, window: &mut Window, cx: &mut App) -> bool {
        let doc = self.doc;
        match index {
            0 => {
                search(doc, window, cx);
                false
            }
            _ => {
                AppState::global(cx).update(cx, |s, cx| {
                    s.store.with_document(doc, |d, _| d.leave_offline());
                    cx.notify();
                });
                true
            }
        }
    }
}

/// Shows or takes down each document window's Relink sheet: the offer is
/// made when due ([`vizdaw_app::DocumentModel::offer_relink_if_due`]);
/// the sheet shows once the window is free (`free`: no prompt, ask or
/// sheet there) and goes when nothing is left to relink. Under the control
/// channel nothing shows. Call it outside any update (the presenter's).
pub(crate) fn present(cx: &mut App, free: impl Fn(gpui::AnyWindowHandle, &mut App) -> bool) {
    let channel = crate::control::channel_on(cx);
    let app = AppState::global(cx);
    let windows: Vec<(DocId, gpui::WindowHandle<DocumentWindow>)> = app
        .read(cx)
        .document_windows()
        .map(|w| (w.doc, w.handle))
        .collect();
    for (doc, handle) in windows {
        let shows = app
            .update(cx, |s, _| {
                s.store.with_document(doc, |d, _| {
                    d.offer_relink_if_due(channel);
                    d.shows_relink_sheet(channel)
                })
            })
            .unwrap_or(false);
        // A window being updated is left for the next call.
        let Ok(up) = handle.update(cx, |v, _, _| v.sheet_label().as_deref() == Some(TITLE)) else {
            continue;
        };
        if up && !shows {
            handle
                .update(cx, |v, window, cx| v.close_sheet(window, cx))
                .ok();
        } else if shows && !up && free(handle.into(), cx) {
            handle
                .update(cx, |v, window, cx| {
                    let _ = v.show_sheet(Box::new(RelinkSheet::new(doc)), window, cx);
                })
                .ok();
        }
    }
}

/// When the app went to the background (no window of it active, and
/// none of its own panels or prompts holding the keyboard).
#[derive(Default)]
struct AppInactive(Option<std::time::Instant>);

impl gpui::Global for AppInactive {}

/// A return within this long of leaving is focus churn, not the app
/// coming back: no recheck (a recheck re-verifies every media file).
pub const RETURN_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(2);

/// One of the app's own panels or prompts has the keyboard: a window lost
/// it to the app itself, not to another app (Swift's
/// `applicationDidBecomeActive` never fires for in-app panels).
fn own_modal_up(cx: &App) -> bool {
    crate::file_panels::is_up(cx) || crate::alerts::prompting(cx)
}

/// A window lost the keyboard: once no window of the app is active and
/// none of its panels or prompts is up, the app is in the background
/// (GPUI has no app-activation event on the desktop).
pub(crate) fn window_deactivated(cx: &mut App) {
    if own_modal_up(cx) {
        return;
    }
    cx.defer(|cx| {
        if cx.active_window().is_none() && !own_modal_up(cx) {
            let now = cx.background_executor().now();
            cx.set_global(AppInactive(Some(now)));
        }
    });
}

/// A window became active: when the app was in the background for longer
/// than [`RETURN_DEBOUNCE`], files may have changed on disk meanwhile
/// ([`recheck_all`], Swift `applicationDidBecomeActive`).
pub(crate) fn window_activated(cx: &mut App) {
    let Some(since) = cx.try_global::<AppInactive>().and_then(|a| a.0) else {
        return;
    };
    cx.set_global(AppInactive(None));
    if cx.background_executor().now().duration_since(since) >= RETURN_DEBOUNCE {
        cx.defer(recheck_all);
    }
}

/// Every open project checks its media files again; one whose offline set
/// changed reads its media again and is offered the sheet for new items.
pub fn recheck_all(cx: &mut App) {
    let app = AppState::global(cx);
    // One recheck per document, however many windows it has.
    let docs: std::collections::BTreeSet<DocId> =
        app.read(cx).windows().iter().map(|w| w.doc).collect();
    app.update(cx, |s, cx| {
        let mut changed = false;
        for doc in docs {
            changed |= s
                .store
                .with_document(doc, |d, _| d.recheck_media())
                .unwrap_or(false);
        }
        if changed {
            cx.notify();
        }
    });
}
