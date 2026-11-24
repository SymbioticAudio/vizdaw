//! The VizDAW GPUI application.

pub mod alerts;
pub mod app_state;
pub mod arrangement;
pub mod banners;
mod channel_strip_pane;
pub mod control;
pub mod controls;
pub mod detail_pane;
pub mod file_menu;
pub mod file_panels;
mod fit;
pub mod format_panel;
#[doc(hidden)]
pub mod frame_stats;
pub mod list_editor;
pub mod menu_bar;
pub mod menus;
mod midi_strip;
pub mod mixer_view;
pub mod panes;
pub mod piano_roll;
pub mod registry;
pub mod screenshot;
pub mod sheet_host;
pub mod shell;
pub mod strip_palette;
pub mod text_input;
pub mod track_headers;
pub mod transport_bar;
pub mod transport_fields;
mod vca_strip;

use std::path::Path;

use gpui::{App, PromptLevel, WindowHandle};
use vizdaw_app::alerts::AlertItem;
use vizdaw_app::store::Opened;
use vizdaw_app::{DocId, DocumentStore};

pub use app_state::{AppState, WindowKind};
pub use menus::{
    AppFileCommand, CloseWindow, FileCommand, NewDemo, NewDocument, OpenDocument, PlayStop, Quit,
    RunCommand, Save, SaveAs, StopAndReturn,
};
pub use shell::DocumentWindow;

/// Creates the app state over `store`, binds the keys, sets the menus and
/// handles the app-level actions (New, the demos, Open, Import Project,
/// Quit).
pub fn install(store: DocumentStore, cx: &mut App) -> gpui::Entity<AppState> {
    let state = app_state::create(store, cx);
    // No window key acts while a file panel is up.
    file_panels::install(cx);
    // A window's keys stay in its sheet while one is up.
    sheet_host::install(cx);
    // Alerts and questions show as prompts when the state changes.
    alerts::install(&state, cx);
    // The store's file reads (DAWproject and AAF opens) run off the UI
    // thread from now on.
    control::files::install_spawner(cx);
    let keymap = state.read(cx).keymap.clone();
    menus::install(&keymap, cx);
    // File › New asks for the format first.
    cx.on_action(|_: &NewDocument, cx| file_menu::new_project(cx));
    file_menu::install(cx);
    cx.on_action(|_: &NewDemo, cx| {
        new_demo(cx);
    });
    cx.on_action(|_: &OpenDocument, cx| prompt_open(cx));
    cx.on_action(|_: &Quit, cx| quit(cx));
    state
}

/// An empty document in a new window (the one the app opens at launch;
/// File › New asks for its format first, [`file_menu::new_project`]).
pub fn new_document(cx: &mut App) -> Option<WindowHandle<DocumentWindow>> {
    let app = AppState::global(cx);
    let doc = app.update(cx, |s, cx| {
        cx.notify();
        s.store.new_empty()
    });
    show(doc, cx)
}

/// File › Create Demo Project.
pub fn new_demo(cx: &mut App) -> Option<WindowHandle<DocumentWindow>> {
    let app = AppState::global(cx);
    let doc = app.update(cx, |s, cx| {
        cx.notify();
        s.store.new_demo()
    });
    show(doc, cx)
}

/// Opens the project at `path` (File › Open once the dialog answers). A
/// project already open brings its window to the front. A refusal or
/// failure is an alert, and `None`; a project that asks a recovery
/// question first opens once it is answered ([`alerts`]).
pub fn open_path(path: &Path, cx: &mut App) -> Option<WindowHandle<DocumentWindow>> {
    let app = AppState::global(cx);
    // Shift held while opening asks the crash question (Swift).
    let shift = cx
        .active_window()
        .and_then(|w| w.update(cx, |_, window, _| window.modifiers().shift).ok())
        .unwrap_or(false);
    let opened = app.update(cx, |s, cx| {
        cx.notify();
        s.store.set_shift_held(Box::new(move || shift));
        let opened = s.store.open(path);
        s.store.set_shift_held(Box::new(|| false));
        match opened {
            Ok(Opened::Doc(d)) => Some(d),
            // The question shows first ([`alerts::present`]); the file is
            // read first, off the UI thread, and its project's window
            // opens when the read lands (`control::files::settle`).
            Ok(Opened::Asking) | Ok(Opened::Reading) | Err(_) => None,
        }
    });
    let handle = opened.and_then(|doc| show_or_front(doc, cx));
    alerts::present(cx);
    handle
}

/// `doc`'s window brought to the front, or opened.
pub(crate) fn show_or_front(doc: DocId, cx: &mut App) -> Option<WindowHandle<DocumentWindow>> {
    let app = AppState::global(cx);
    if let Some(entry) = app.read(cx).window_of(doc).cloned() {
        entry
            .handle
            .update(cx, |_, window, _| window.activate_window())
            .ok();
        return Some(entry.handle);
    }
    show(Some(doc), cx)
}

/// A window for `doc`; with none, the store's alert shows in a dialog
/// window of its own ([`alerts`]).
pub(crate) fn show(doc: Option<DocId>, cx: &mut App) -> Option<WindowHandle<DocumentWindow>> {
    let app = AppState::global(cx);
    let handle = doc.and_then(|d| DocumentWindow::open(&app, d, cx).ok());
    alerts::present(cx);
    handle
}

/// File › Open…: Swift's open panel (one project, MIDI, DAWproject or
/// AAF file; on Linux a project is chosen by its `project.json`).
pub fn prompt_open(cx: &mut App) {
    choose_and_open(&vizdaw_app::panels::OPEN_PROJECT, cx);
}

/// `spec`'s panel over the active window, then each chosen path opened
/// ([`open_path`]): File › Open… and Import Project's items.
pub(crate) fn choose_and_open(spec: &vizdaw_app::panels::PanelSpec, cx: &mut App) {
    let window = cx.active_window();
    let chosen = file_panels::choose(spec, file_panels::PanelPlace::default(), window, cx);
    cx.spawn(async move |cx| {
        let answer = chosen.await;
        cx.update(|cx| match answer {
            Ok(Some(paths)) => {
                for p in paths {
                    open_path(&p, cx);
                }
            }
            Err(e) => panel_failed(e, cx),
            Ok(None) => {}
        });
    })
    .detach();
}

/// A panel that could not show: the alert "Open Failed".
pub(crate) fn panel_failed(message: String, cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| {
        s.store.post_alert(AlertItem::new("Open Failed", message));
        cx.notify();
    });
}

/// Quit: with unsaved changes, asks first in the front window. Deferred:
/// Quit arrives from a key or menu while the front window is being
/// updated, and a window cannot be updated from inside its own update.
pub fn quit(cx: &mut App) {
    cx.defer(quit_or_ask);
}

fn quit_or_ask(cx: &mut App) {
    commit_renames(cx);
    let app = AppState::global(cx);
    let (dirty, front) = {
        let s = app.read(cx);
        let dirty = s.windows().iter().any(|w| s.has_unsaved_changes(w.doc));
        let front = s
            .store
            .front()
            .and_then(|d| s.window_of(d))
            .or_else(|| s.windows().first())
            .map(|w| w.handle);
        (dirty, front)
    };
    if !dirty {
        quit_now(cx);
        return;
    }
    // Unsaved changes are never discarded without an answer: with no
    // window to ask in, Quit does nothing.
    let Some(front) = front else {
        eprintln!("Quit: unsaved changes and no window to ask in");
        return;
    };
    let answer = alerts::ask(
        front.into(),
        sheet_host::Ask::with_labels(
            PromptLevel::Warning,
            "Quit without saving?",
            Some("Some documents have unsaved changes."),
            &["Discard Changes and Quit", "Cancel"],
        ),
        cx,
    );
    cx.spawn(async move |cx| {
        if let Ok(0) = answer.await {
            cx.update(quit_now);
        }
    })
    .detach();
}

/// Commits the open track rename of every window, before Quit checks for
/// unsaved changes. Call it outside any window's update (a window being
/// updated is skipped).
pub fn commit_renames(cx: &mut App) {
    let handles: Vec<_> = AppState::global(cx)
        .read(cx)
        .windows()
        .iter()
        .map(|w| w.handle)
        .collect();
    for handle in handles {
        handle
            .update(cx, |v, window, cx| v.commit_rename(window, cx))
            .ok();
    }
}

/// Closes every document (stopping its transport, dropping it so no poll
/// reads a closed session) and quits.
pub fn quit_now(cx: &mut App) {
    let app = AppState::global(cx);
    app.update(cx, |s, cx| {
        s.close_all();
        cx.notify();
    });
    cx.quit();
}

/// Starts the app with one new document window (the demo with
/// `--demo`), and the control channel with `--control PATH` (or
/// `VIZDAW_CONTROL`). A control socket that cannot be made exits with
/// status 2, so a launching agent learns at once.
pub fn run() {
    let options = control::LaunchOptions::from_env();
    gpui_platform::application().run(move |cx: &mut App| {
        let state = install(DocumentStore::live(), cx);
        file_panels::use_native(cx);
        // Off macOS the platform has no prompt: GPUI's prompts are drawn
        // as the sheet card (with Return and Esc).
        if cfg!(not(target_os = "macos")) {
            sheet_host::use_sheet_prompts(cx);
        }
        // The preferences under the launch home (a throwaway e2e home
        // keeps its own).
        state.update(cx, |s, _| s.set_prefs(vizdaw_app::Prefs::open()));
        // Before any document opens.
        if let Some(path) = &options.control
            && let Err(e) = control::start(path, cx)
        {
            eprintln!("{e}");
            std::process::exit(2);
        }
        cx.activate(true);
        if options.demo {
            new_demo(cx);
        } else {
            new_document(cx);
        }
    });
}
