//! The import, export and merge report (Swift `ReportSheet`, 520 wide):
//! the title (`Export Report`, `Merge Report`, `Import Report` or
//! `Report`, [`report_text::title`]), the summary, then Conflicts (with
//! the warning glyph), Changed, Left out and Notes as disclosure rows
//! `<title> (<count>)` whose lines open onto their places (`and N more`),
//! or `Everything transferred.`. `Copy` puts the plain text on the
//! clipboard; `Save as Text…` writes it through panel #17 (`MIDI
//! Report.txt`, always); `Show in Finder` (Linux: `Show in Folder`) shows
//! what the export wrote, and only then; `Done` (Esc) closes it.
//!
//! The sheet is shown by itself ([`present`]) while the document's
//! report waits to be shown: after an import or export this document's
//! UI ran, and from File › Last Import or Export Report…. Never under the
//! control channel.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AnyWindowHandle, App, ClipboardItem, Global, InteractiveElement, IntoElement,
    ParentElement, PromptLevel, StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use vizdaw_app::report_text::{
    self, COPY, COPY_LABEL, DONE, EVERYTHING_TRANSFERRED, REPORT, SAVE_AS_TEXT, SAVE_LABEL,
    SHOW_FILES, SHOW_FILES_LABEL, TITLES,
};
use vizdaw_app::{DocId, panels};
use vizdaw_ffi::{ReportItemRow, ReportRow};

use crate::app_state::AppState;
use crate::file_panels::PanelPlace;
use crate::registry::RegistryScope;
use crate::sheet_host::{Ask, ButtonRole, Sheet, SheetButton};
use crate::shell::DocumentWindow;
use crate::transport_bar::colors;

/// The card's width (Swift `.frame(width: 520)`).
const WIDTH: f32 = 520.;
/// The groups' least and most height (Swift `minHeight: 120,
/// maxHeight: 360`).
const GROUPS_MIN_HEIGHT: f32 = 120.;
const GROUPS_MAX_HEIGHT: f32 = 360.;
/// Which disclosure rows are open: groups by title, lines by group and
/// index. Swift's start closed.
#[derive(Default)]
struct Open {
    groups: BTreeSet<&'static str>,
    items: BTreeSet<(&'static str, usize)>,
}

/// The report sheet of document `doc`.
pub struct ReportSheet {
    doc: DocId,
    /// Its title, as last drawn (the report's, [`report_text::title`]).
    title: &'static str,
    open: Rc<RefCell<Open>>,
}

impl ReportSheet {
    pub fn new(doc: DocId, cx: &App) -> Self {
        let mut sheet = ReportSheet {
            doc,
            title: REPORT,
            open: Rc::default(),
        };
        if let Some((r, w)) = sheet.state(cx) {
            sheet.title = report_text::title(&r, &w);
        }
        sheet
    }

    /// The report shown and what its export wrote.
    fn state(&self, cx: &App) -> Option<(ReportRow, Vec<String>)> {
        let d = AppState::global(cx).read(cx).store.document(self.doc)?;
        Some((d.last_report()?.clone(), d.report_written().to_vec()))
    }
}

fn text(t: &str, scope: &mut RegistryScope) -> gpui::Div {
    div()
        .relative()
        .child(t.to_owned())
        .child(scope.control("text", t, true))
}

/// A disclosure row's face: its triangle and `label`, recorded as a
/// `disclosure` (selected when open); a click opens or closes it.
fn disclosure(
    id: (&'static str, usize),
    face: gpui::Div,
    label: &str,
    open: bool,
    flip: impl Fn(&mut Open) + 'static,
    state: &Rc<RefCell<Open>>,
    scope: &mut RegistryScope,
) -> AnyElement {
    let state = state.clone();
    div()
        .id(id)
        .relative()
        .flex()
        .flex_row()
        .gap_1()
        .cursor_pointer()
        .child(
            div()
                .flex_none()
                .w(px(12.))
                .text_color(rgb(colors::dim()))
                .child(if open { "\u{25BE}" } else { "\u{25B8}" }),
        )
        .child(face)
        .child(scope.control_selected("disclosure", label, true, None, open))
        .on_click(move |_, window, cx| {
            flip(&mut state.borrow_mut());
            window.refresh();
            cx.stop_propagation();
        })
        .into_any_element()
}

/// One report line: its places in a disclosure row when it has any.
fn item_row(
    group: &'static str,
    i: usize,
    item: &ReportItemRow,
    state: &Rc<RefCell<Open>>,
    scope: &mut RegistryScope,
) -> AnyElement {
    let line = report_text::line(item);
    let details = report_text::details(item);
    if details.is_empty() {
        return text(&line, scope).into_any_element();
    }
    let key = (group, i);
    let open = state.borrow().items.contains(&key);
    let mut col = div().flex().flex_col().child(disclosure(
        (group, i),
        div().child(line.clone()),
        &line,
        open,
        move |o| {
            if !o.items.remove(&key) {
                o.items.insert(key);
            }
        },
        state,
        scope,
    ));
    if open {
        let mut places = div().flex().flex_col().pl_4();
        for d in &details {
            places = places.child(text(d, scope).text_xs().text_color(rgb(colors::dim())));
        }
        col = col.child(places);
    }
    col.into_any_element()
}

/// The groups: each a disclosure row `<title> (<count>)` over its lines.
fn groups_list(r: &ReportRow, state: &Rc<RefCell<Open>>, scope: &mut RegistryScope) -> AnyElement {
    let mut list = scope.container();
    let clip = list.clip();
    let mut col = div().flex().flex_col().gap_2();
    for (title, items) in report_text::groups(r) {
        let heading = report_text::group_heading(title, &items);
        let label = report_text::group_label(title, &items);
        let conflicts = title == "Conflicts";
        let open = state.borrow().groups.contains(title);
        let face = div()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .when(conflicts, |d| d.text_color(rgb(colors::warning())))
            .child(if conflicts {
                format!("\u{26A0} {heading}")
            } else {
                heading
            });
        let mut group = div().flex().flex_col().gap_1().child(disclosure(
            ("report-group", col_index(title)),
            face,
            &label,
            open,
            move |o| {
                if !o.groups.remove(title) {
                    o.groups.insert(title);
                }
            },
            state,
            &mut list,
        ));
        if open {
            let mut lines = div().flex().flex_col().gap_1().pl_4();
            for (i, item) in items.iter().enumerate() {
                lines = lines.child(item_row(title, i, item, state, &mut list));
            }
            group = group.child(lines);
        }
        col = col.child(group);
    }
    div()
        .id("report-groups")
        .relative()
        .min_h(px(GROUPS_MIN_HEIGHT))
        .max_h(px(GROUPS_MAX_HEIGHT))
        .overflow_y_scroll()
        .child(col)
        .child(clip)
        .into_any_element()
}

/// A group's place in Swift's order (its element id).
fn col_index(title: &str) -> usize {
    ["Conflicts", "Changed", "Left out", "Notes"]
        .iter()
        .position(|t| *t == title)
        .unwrap_or(4)
}

impl Sheet for ReportSheet {
    fn label(&self) -> String {
        self.title.to_owned()
    }

    fn width(&self) -> f32 {
        WIDTH
    }

    fn render(&mut self, scope: &mut RegistryScope, _: &mut Window, cx: &mut App) -> AnyElement {
        let Some((report, written)) = self.state(cx) else {
            return div().into_any_element();
        };
        // The title follows the report shown (Swift's computed title).
        let title = report_text::title(&report, &written);
        self.title = title;
        let mut col = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(text(title, scope).font_weight(gpui::FontWeight::BOLD))
            .child(text(&report.summary, scope));
        if report_text::groups(&report).is_empty() {
            col = col.child(text(EVERYTHING_TRANSFERRED, scope).text_color(rgb(colors::dim())));
        } else {
            col = col.child(groups_list(&report, &self.open, scope));
        }
        col.into_any_element()
    }

    fn buttons(&self, cx: &App) -> Vec<SheetButton> {
        let written = self.state(cx).is_some_and(|(_, w)| !w.is_empty());
        let mut b = vec![
            SheetButton::new(COPY, ButtonRole::Plain).with_label(COPY_LABEL),
            SheetButton::new(SAVE_AS_TEXT, ButtonRole::Plain).with_label(SAVE_LABEL),
        ];
        if written {
            b.push(SheetButton::new(SHOW_FILES, ButtonRole::Plain).with_label(SHOW_FILES_LABEL));
        }
        // As the Track Synth sheet's Done: Escape closes it.
        b.push(SheetButton::new(DONE, ButtonRole::Cancel));
        b
    }

    fn press(&mut self, index: usize, window: &mut Window, cx: &mut App) -> bool {
        let Some(pressed) = self.buttons(cx).get(index).map(|b| b.label.clone()) else {
            return false;
        };
        let state = self.state(cx);
        let plain = state
            .as_ref()
            .map(|(r, _)| report_text::plain(r))
            .unwrap_or_default();
        match pressed.as_str() {
            COPY => {
                cx.write_to_clipboard(ClipboardItem::new_string(plain));
                false
            }
            SAVE_AS_TEXT => {
                save(plain, window, cx);
                false
            }
            SHOW_FILES => {
                if let Some((_, written)) = state {
                    show_files(&written, cx);
                }
                false
            }
            _ => {
                let doc = self.doc;
                AppState::global(cx).update(cx, |s, cx| {
                    s.store.with_document(doc, |d, _| d.hide_report());
                    cx.notify();
                });
                true
            }
        }
    }
}

/// Save as Text…: panel #17 (`MIDI Report.txt`; the app adds `.txt` and
/// asks before replacing), then the text is written off the UI thread. A
/// write that fails is an alert over the sheet (Swift `NSAlert(error:)`).
fn save(plain: String, window: &Window, cx: &mut App) {
    let asking = window.window_handle();
    let chosen =
        crate::file_panels::choose_save(&panels::REPORT_TEXT, PanelPlace::default(), asking, cx);
    cx.spawn(async move |cx| {
        let Ok(Some(path)) = chosen.await else {
            return;
        };
        let to = path.clone();
        let written = cx
            .background_executor()
            .spawn(async move { std::fs::write(&to, plain) })
            .await;
        if let Err(e) = written {
            cx.update(|cx| save_failed(asking, &path, &e, cx));
        }
    })
    .detach();
}

/// The alert of a report that could not be written to `path`.
fn save_failed(asking: AnyWindowHandle, path: &Path, e: &std::io::Error, cx: &mut App) {
    let ask = Ask::alert(
        PromptLevel::Warning,
        &report_text::save_failed_title(path),
        &e.to_string(),
    );
    let answer = crate::alerts::ask(asking, ask, cx);
    // The ask is dropped once nobody waits for its answer.
    cx.spawn(async move |_| {
        let _ = answer.await;
    })
    .detach();
}

/// Shows the files to `reveal` (GPUI's `reveal_path`; a test's own).
pub type Reveal = Rc<dyn Fn(&Path, &mut App)>;

struct Revealer(Reveal);

impl Global for Revealer {}

/// Shows written files with `reveal` instead of the file manager (tests).
pub fn set_revealer(reveal: Reveal, cx: &mut App) {
    cx.set_global(Revealer(reveal));
}

/// Show in Finder / Show in Folder: Swift selects every file written in
/// the file manager; `reveal_path` shows one, so each folder written into
/// shows once, with its first file selected.
fn show_files(written: &[String], cx: &mut App) {
    let reveal = cx.try_global::<Revealer>().map(|r| r.0.clone());
    for path in report_text::reveal_paths(written) {
        let path = PathBuf::from(path);
        match &reveal {
            Some(r) => r(&path, cx),
            None => cx.reveal_path(&path),
        }
    }
}

/// Shows or takes down each document's report sheet: it shows on one free
/// window of the document (the active one first; `free`: no prompt, ask
/// or sheet there) while the report waits to be shown, and goes once the
/// document hides it. Under the control channel nothing shows. Call it
/// outside any update (the presenter's).
pub(crate) fn present(cx: &mut App, free: impl Fn(AnyWindowHandle, &mut App) -> bool) {
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
                .is_some_and(|d| d.report_shown() && d.last_report().is_some())
    };
    let mut placed: Vec<DocId> = vec![];
    for (doc, handle) in &windows {
        let ours = handle
            .update(cx, |v, _, _| {
                v.sheet_label()
                    .is_some_and(|l| TITLES.contains(&l.as_str()))
            })
            .unwrap_or(false);
        if !ours {
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
        let shown = handle
            .update(cx, |v, window, cx| {
                let sheet = ReportSheet::new(doc, cx);
                v.show_sheet(Box::new(sheet), window, cx).is_ok()
            })
            .unwrap_or(false);
        if shown {
            placed.push(doc);
        }
    }
}

/// File › Last Import or Export Report…: the session's last report (the
/// sheet shows through [`present`]), or the status line saying there is
/// none.
pub fn show_last(doc: DocId, cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| {
        s.store.with_document(doc, |d, _| d.show_last_report());
        cx.notify();
    });
}
