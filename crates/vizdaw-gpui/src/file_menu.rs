//! What the File menu's items do (slice 7b Task 3; Swift `AppCommands`'s
//! File groups):
//!
//! - New: the New Project format panel ([`crate::format_panel`]) first, as
//!   a sheet on the front window (a dialog window of its own when no
//!   window is open, or the front window already shows a sheet); Create
//!   remembers the format as the default and makes the project. Under
//!   the control channel no panel shows: the project is made in the
//!   default format.
//! - Create Demo Project with Audio: the demo with audio.
//! - Import Project › DAWproject… and AAF…: their panel, then the store's
//!   open (a DAWproject or AAF file opens as a new project).
//! - Import Audio…: its panel (several files), then the import job on the
//!   document at Swift's import target (one file: the selected track of
//!   the shown sequence, else new tracks; at the transport's tick).
//! - Project Settings…: its sheet ([`crate::sheets::project_settings`]).
//! - Import MIDI File…: its panel, then the import sheet's read of the
//!   file (Swift `beginImport`; a file that cannot be read is the alert
//!   "Import Failed"; the sheet itself is slice 7c's).
//! - Relink Media…: the Relink sheet ([`crate::sheets::relink`]).
//! - Open Co-Producer Copy…: panel #2 in the project's copies folder, then
//!   the store's `open_copy` (a new untitled project).
//! - The items whose sheets come later do nothing yet
//!   ([`LATER`]).
//!
//! New, Open…, Create Demo Project, Close, Save and Save As… keep their
//! slice 1 actions ([`crate::menus::action_for`]).

use std::path::PathBuf;

use gpui::{App, Context, Global, Window};
use vizdaw_app::panels::{self, PanelSpec};
use vizdaw_ffi::ProjectFormatRow;

use crate::app_state::AppState;
use crate::file_panels::{self, PanelPlace};
use crate::format_panel::FormatSheet;
use crate::menus::{AppFileCommand, FileCommand};
use crate::sheet_host::{Sheet, SheetDialog};
use crate::shell::DocumentWindow;

/// The File items whose sheet or flow lands later; their menu item does
/// nothing yet: Bounce… (Task 6), the Export items and the report (slice
/// 7c).
pub const LATER: &[&str] = &[
    "file.bounce",
    "file.export_dawproject",
    "file.export_aaf",
    "file.export_midi",
    "file.export_midi_clip",
    "file.export_for_daw",
    "file.interchange_report",
];

/// Handles the File items that need no document.
pub fn install(cx: &mut App) {
    cx.on_action(|a: &AppFileCommand, cx| run_app(&a.id, cx));
}

fn run_app(id: &str, cx: &mut App) {
    match id {
        "file.create_demo_audio" => {
            let doc = AppState::global(cx).update(cx, |s, cx| {
                cx.notify();
                s.store.new_demo_audio()
            });
            crate::show(doc, cx);
        }
        "file.import_dawproject" => crate::choose_and_open(&panels::IMPORT_DAWPROJECT, cx),
        // With a project open Swift's Import AAF sheet asks where it goes
        // (slice 7c); until then it opens as a new project, the sheet's
        // first choice and what Swift does with no project open.
        "file.import_aaf" => crate::choose_and_open(&panels::IMPORT_AAF, cx),
        _ => {}
    }
}

/// File item `action` on `view`'s document (the window it came from).
pub(crate) fn run_in_window(
    action: &FileCommand,
    view: &mut DocumentWindow,
    window: &mut Window,
    cx: &mut Context<DocumentWindow>,
) {
    let doc = view.doc();
    match action.id.as_ref() {
        "file.project_settings" => crate::sheets::project_settings::show(view, window, cx),
        "media.relink" => AppState::global(cx).update(cx, |s, cx| {
            // The presenter shows the sheet once the window is free.
            s.store.with_document(doc, |d, _| d.show_relink());
            cx.notify();
        }),
        "file.open_copy" => open_copy(doc, window, cx),
        "media.import" => choose(&panels::IMPORT_AUDIO, window, cx, move |paths, cx| {
            let paths = paths
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            AppState::global(cx).update(cx, |s, cx| {
                s.store.with_document(doc, |d, _| d.import_audio(paths));
                cx.notify();
            });
        }),
        "file.import_midi" => choose(&panels::IMPORT_MIDI, window, cx, move |paths, cx| {
            let Some(path) = paths.first() else { return };
            let path = path.to_string_lossy().into_owned();
            AppState::global(cx).update(cx, |s, cx| {
                s.store.begin_midi_import(doc, &path);
                cx.notify();
            });
        }),
        _ => {}
    }
}

/// Open Co-Producer Copy…: panel #2 in `doc`'s `Co-Producer/Copies`
/// folder, then the copy as a new untitled project (anything else is the
/// alert "Open Failed" / "That is not a Co-Producer copy."). On Linux a
/// project is chosen by its `project.json`: that is its package (owner
/// ruling 2026-11-23). No panel under the control channel.
fn open_copy(doc: vizdaw_app::DocId, window: &Window, cx: &mut App) {
    if crate::control::channel_on(cx) {
        return;
    }
    let package = AppState::global(cx)
        .read(cx)
        .store
        .document(doc)
        .and_then(|d| d.path());
    let Some(package) = package else { return };
    let place = PanelPlace {
        directory: Some(panels::co_producer_copies(&package)),
        ..PanelPlace::default()
    };
    choose_at(&panels::OPEN_COPY, place, window, cx, |paths, cx| {
        let Some(path) = paths.first() else { return };
        let path = vizdaw_app::store::project_path(path);
        let opened = AppState::global(cx).update(cx, |s, cx| {
            cx.notify();
            s.store.open_copy(&path).ok()
        });
        crate::show(opened, cx);
    });
}

/// Shows `spec`'s panel over `window`; `then` takes the chosen paths. A
/// panel that cannot show is the alert "Open Failed".
fn choose(
    spec: &PanelSpec,
    window: &Window,
    cx: &mut App,
    then: impl FnOnce(Vec<PathBuf>, &mut App) + 'static,
) {
    choose_at(spec, PanelPlace::default(), window, cx, then);
}

/// [`choose`] with the panel's folder and name filled in.
pub(crate) fn choose_at(
    spec: &PanelSpec,
    place: PanelPlace,
    window: &Window,
    cx: &mut App,
    then: impl FnOnce(Vec<PathBuf>, &mut App) + 'static,
) {
    let chosen = file_panels::choose(spec, place, Some(window.window_handle()), cx);
    cx.spawn(async move |cx| {
        let answer = chosen.await;
        cx.update(|cx| match answer {
            Ok(Some(paths)) => then(paths, cx),
            Ok(None) => {}
            Err(e) => crate::panel_failed(e, cx),
        });
    })
    .detach();
}

// ---------------------------------------------------------------------
// New

/// The format panel is up (a second New waits for it to be answered).
#[derive(Default)]
struct FormatPanelUp(bool);

impl Global for FormatPanelUp {}

/// Whether the New Project format panel is up.
pub fn format_panel_up(cx: &App) -> bool {
    cx.try_global::<FormatPanelUp>().is_some_and(|u| u.0)
}

/// File › New: the format panel, then the project (deferred: New comes
/// from a key or menu while a window is being updated).
pub fn new_project(cx: &mut App) {
    cx.defer(ask_format);
}

fn ask_format(cx: &mut App) {
    if format_panel_up(cx) {
        return;
    }
    let app = AppState::global(cx);
    let (initial, rates) = {
        let s = app.read(cx);
        (s.store.default_format(), s.store.core().project_rates())
    };
    // Under the channel no panel shows (`file.new` gives the format).
    if crate::control::channel_on(cx) {
        make(initial, cx);
        return;
    }
    let (sheet, _, answer) = FormatSheet::new(rates, initial, cx);
    let mut slot: Option<Box<dyn Sheet>> = Some(Box::new(sheet));
    let front = crate::alerts::front_window(cx).and_then(|w| w.downcast::<DocumentWindow>());
    if let Some(front) = front {
        front
            .update(cx, |v, window, cx| {
                if let Some(sheet) = slot.take()
                    && let Err(back) = v.show_sheet(sheet, window, cx)
                {
                    slot = Some(back);
                }
            })
            .ok();
    }
    if let Some(sheet) = slot.take()
        && SheetDialog::open(sheet, cx).is_none()
    {
        return;
    }
    cx.set_global(FormatPanelUp(true));
    crate::menus::refresh(cx);
    cx.spawn(async move |cx| {
        let chosen = answer.await;
        cx.update(|cx| {
            cx.set_global(FormatPanelUp(false));
            crate::menus::refresh(cx);
            if let Ok(format) = chosen {
                make(format, cx);
            }
        });
    })
    .detach();
}

/// The format becomes the default (remembered in the preferences), and a
/// new project in it opens in a window.
fn make(format: ProjectFormatRow, cx: &mut App) {
    let doc = AppState::global(cx).update(cx, |s, cx| {
        cx.notify();
        s.remember_default_format(format);
        s.store.new_project(format)
    });
    crate::show(doc, cx);
}
