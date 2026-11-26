//! Merge DAWproject (Swift `MergeSheet`, 520 wide), the return prompt's
//! second page on the window of the project it changes: the bold title
//! and the file's name; while the merge waits `Waiting for the running
//! job to finish…`; while the Hold job reads the file `Reading the file…`
//! with its progress (`Merge progress`); when the file lost its export
//! ID `Which export did this file come from?` and the project's exports
//! (`Export of <when>, N tracks`, list `Exports of this project`); then
//! the core's counts line and `No conflicts.` or `Conflicts (VizDAW's
//! version is kept)` with its lines (`Conflicts, N`); the core's refusal.
//! Buttons: `Cancel` (Esc; `Cancel the merge; nothing changes` while
//! held), then `Preview` while choosing, else `Preview Again` (when the
//! core let the hold go) and `Merge` (Return; `Merge into this project`).
//!
//! The snapshot listing runs on the background executor; only the newest
//! answer lands. The sheet shows by itself ([`present`]) while its
//! document's merge shows, after any other sheet of the window (the
//! report of the merge before it). It is taken down without a press
//! under the control channel, which cancels nothing. Merge and Cancel run
//! once: the model ends the job and the hold before anything else.

use crate::appearance::ThemedCorners as _;
use std::panic::AssertUnwindSafe;
use std::path::Path;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AnyWindowHandle, App, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use vizdaw_app::merge::{
    CANCEL, CANCEL_HELD_LABEL, CONFLICTS, EXPORTS_LABEL, MERGE, MERGE_LABEL, NO_CONFLICTS, PREVIEW,
    PREVIEW_AGAIN, PREVIEW_AGAIN_LABEL, PREVIEW_LABEL, PROGRESS_LABEL, READING, TITLE,
    WAITING_TEXT, WHICH_EXPORT, conflicts_label, snapshot_label, tracks_text, when,
};
use vizdaw_app::{DocId, DocumentModel};
use vizdaw_ffi::{MergePreviewRow, SnapshotRow};

use super::audio_format::{progress_bar, text};
use super::for_daw::{error_row, present_sheet, waiting_row};
use crate::app_state::AppState;
use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton};
use crate::transport_bar::colors;

/// The card's width (Swift `.frame(width: 520)`).
const WIDTH: f32 = 520.;
/// The snapshot list's most height (Swift `maxHeight: 220`).
const LIST_MAX_HEIGHT: f32 = 220.;
/// The conflict lines' most height (Swift `maxHeight: 240`).
const CONFLICTS_MAX_HEIGHT: f32 = 240.;
/// What the sheet shows of the document, read once per frame.
struct View {
    path: String,
    waiting: bool,
    running: bool,
    progress: Option<f32>,
    choosing: bool,
    snapshots: Vec<SnapshotRow>,
    selected: Option<String>,
    preview: Option<MergePreviewRow>,
    error: Option<String>,
    held: bool,
    can_merge: bool,
    can_preview_again: bool,
}

/// The Merge DAWproject sheet of document `doc`.
pub struct MergeSheet {
    doc: DocId,
}

impl MergeSheet {
    fn view(&self, cx: &App) -> Option<View> {
        let d = AppState::global(cx).read(cx).store.document(self.doc)?;
        let g = d.merge();
        Some(View {
            path: g.path().to_owned(),
            waiting: g.waiting(),
            running: d.merge_running(),
            progress: d.merge_progress(),
            choosing: g.choosing(),
            snapshots: g.snapshots().to_vec(),
            selected: g.selected_snapshot().map(str::to_owned),
            preview: g.preview().cloned(),
            error: g.error().map(str::to_owned),
            held: g.held(),
            can_merge: g.can_merge(),
            can_preview_again: g.can_preview_again(),
        })
    }
}

/// Runs `f` on `doc` (a closed document does nothing).
fn with_doc<R>(doc: DocId, cx: &mut App, f: impl FnOnce(&mut DocumentModel) -> R) -> Option<R> {
    AppState::global(cx).update(cx, |s, cx| {
        cx.notify();
        s.store.document_mut(doc).map(f)
    })
}

/// Runs each document's waiting snapshot listing on the background
/// executor; only the newest answer lands, and a closed document drops
/// it.
fn run_reads(cx: &mut App) {
    let taken: Vec<_> = AppState::global(cx).update(cx, |s, _| {
        let docs: Vec<DocId> = s.document_windows().map(|w| w.doc).collect();
        docs.into_iter()
            .filter_map(|doc| {
                let d = s.store.document_mut(doc)?;
                let read = d.take_merge_snapshots()?;
                Some((doc, read, d.session().clone()))
            })
            .collect()
    });
    for (doc, read, session) in taken {
        let task = cx.background_executor().spawn(async move {
            std::panic::catch_unwind(AssertUnwindSafe(|| read.run(session.as_ref()))).ok()
        });
        cx.spawn(async move |cx| {
            let Some(listed) = task.await else { return };
            cx.update(|cx| {
                with_doc(doc, cx, |d| d.land_merge_snapshots(listed));
            });
        })
        .detach();
    }
}

/// The file's name (Swift `lastPathComponent`).
fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// "Which export did this file come from?" and the exports, newest
/// first; a click selects one.
fn snapshot_list(
    doc: DocId,
    rows: &[SnapshotRow],
    selected: Option<&str>,
    scope: &mut RegistryScope,
) -> AnyElement {
    let mut list = scope.container();
    let items: Vec<AnyElement> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let on = selected == Some(r.export_id.as_str());
            let id = r.export_id.clone();
            div()
                .id(("merge-snapshot", i))
                .relative()
                .flex()
                .flex_row()
                .justify_between()
                .px_2()
                .py_1()
                .round_sm()
                .cursor_pointer()
                .when(on, |d| d.bg(rgb(colors::on())))
                .child(when(r.exported_at))
                .child(
                    div()
                        .text_color(rgb(colors::dim()))
                        .child(tracks_text(r.tracks)),
                )
                .child(list.control_selected("row", &snapshot_label(r), true, None, on))
                .on_click(move |_, window, cx| {
                    with_doc(doc, cx, |d| d.select_merge_snapshot(Some(&id)));
                    window.refresh();
                })
                .into_any_element()
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(text(WHICH_EXPORT, scope))
        .child(
            div()
                .id("merge-snapshots")
                .relative()
                .flex()
                .flex_col()
                .max_h(px(LIST_MAX_HEIGHT))
                .overflow_y_scroll()
                .border_1()
                .border_color(rgb(colors::border()))
                .round_md()
                .bg(rgb(colors::list()))
                .children(items)
                .child(list.this("list", EXPORTS_LABEL, true))
                .child(list.clip()),
        )
        .into_any_element()
}

/// The core's counts line, then its conflicts.
fn preview_view(p: &MergePreviewRow, scope: &mut RegistryScope) -> AnyElement {
    let mut body = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(text(&p.summary, scope));
    if p.conflict_lines.is_empty() {
        return body
            .child(text(NO_CONFLICTS, scope).text_color(rgb(colors::dim())))
            .into_any_element();
    }
    body = body.child(
        text(CONFLICTS, scope)
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(colors::warning())),
    );
    let mut lines = scope.container();
    let items: Vec<AnyElement> = p
        .conflict_lines
        .iter()
        .map(|l| {
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
                .child(text(l, &mut lines).flex_1())
                .into_any_element()
        })
        .collect();
    body.child(
        div()
            .id("merge-conflicts")
            .relative()
            .flex()
            .flex_col()
            .gap_1()
            .max_h(px(CONFLICTS_MAX_HEIGHT))
            .overflow_y_scroll()
            .children(items)
            .child(lines.this("group", &conflicts_label(p.conflict_lines.len()), true))
            .child(lines.clip()),
    )
    .into_any_element()
}

impl Sheet for MergeSheet {
    fn label(&self) -> String {
        TITLE.to_owned()
    }

    fn width(&self) -> f32 {
        WIDTH
    }

    fn render(&mut self, scope: &mut RegistryScope, _: &mut Window, cx: &mut App) -> AnyElement {
        run_reads(cx);
        let Some(v) = self.view(cx) else {
            return div().into_any_element();
        };
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(text(TITLE, scope).font_weight(gpui::FontWeight::BOLD))
            .child(
                text(&file_name(&v.path), scope)
                    .text_xs()
                    .text_color(rgb(colors::dim()))
                    .truncate(),
            );
        if v.waiting {
            body = body.child(waiting_row(WAITING_TEXT, scope));
        } else if v.running {
            body = body.child(text(READING, scope)).child(progress_bar(
                v.progress.unwrap_or(0.),
                PROGRESS_LABEL,
                scope,
            ));
        } else if v.choosing {
            body = body.child(snapshot_list(
                self.doc,
                &v.snapshots,
                v.selected.as_deref(),
                scope,
            ));
        } else if let Some(p) = &v.preview {
            body = body.child(preview_view(p, scope));
        }
        if let Some(e) = &v.error {
            body = body.child(error_row(e, scope));
        }
        body.into_any_element()
    }

    fn buttons(&self, cx: &App) -> Vec<SheetButton> {
        let Some(v) = self.view(cx) else {
            return vec![SheetButton::new(CANCEL, ButtonRole::Cancel)];
        };
        let mut out = vec![
            SheetButton::new(CANCEL, ButtonRole::Cancel).with_label(if v.held {
                CANCEL_HELD_LABEL
            } else {
                CANCEL
            }),
        ];
        if v.choosing {
            let preview = SheetButton::new(PREVIEW, ButtonRole::Default).with_label(PREVIEW_LABEL);
            out.push(if v.selected.is_some() {
                preview
            } else {
                preview.disabled()
            });
            return out;
        }
        if v.can_preview_again {
            out.push(
                SheetButton::new(PREVIEW_AGAIN, ButtonRole::Plain).with_label(PREVIEW_AGAIN_LABEL),
            );
        }
        let merge = SheetButton::new(MERGE, ButtonRole::Default).with_label(MERGE_LABEL);
        out.push(if v.can_merge { merge } else { merge.disabled() });
        out
    }

    fn press(&mut self, index: usize, _: &mut Window, cx: &mut App) -> bool {
        let doc = self.doc;
        let Some(label) = self.buttons(cx).get(index).map(|b| b.label.clone()) else {
            return false;
        };
        let closes = with_doc(doc, cx, |d| {
            match label.as_str() {
                PREVIEW => d.preview_selected_merge(),
                PREVIEW_AGAIN => d.preview_merge_again(),
                MERGE => d.confirm_merge(),
                _ => d.cancel_merge(),
            }
            // Gone, or the report of a confirmed merge comes first (a
            // merge queued behind it shows after).
            !d.merge().shown() || (label == MERGE && d.report_shown())
        });
        // The project changed: what waited for the merge may start, and
        // a queued merge begins; the sheets follow.
        cx.defer(crate::alerts::present);
        closes.unwrap_or(true)
    }
}

/// Shows each document's merge sheet while its merge shows and its
/// window is free (after the window's other sheets), takes it down once
/// the merge ends; see [`super::for_daw::present`]. The snapshot
/// listings run from here, whether the sheet is up yet or not.
pub(crate) fn present(cx: &mut App, free: impl Fn(AnyWindowHandle, &mut App) -> bool) {
    run_reads(cx);
    present_sheet(
        TITLE,
        |d| d.merge().shown(),
        |doc, _| Box::new(MergeSheet { doc }),
        cx,
        free,
    );
}
