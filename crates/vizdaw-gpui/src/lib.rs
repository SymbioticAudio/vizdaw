//! The VizDAW GPUI application.

pub mod app_state;
pub mod menus;
pub mod registry;
pub mod shell;
pub mod text_input;
pub mod transport_bar;

use std::path::Path;

use gpui::{App, PathPromptOptions, PromptLevel, WindowHandle};
use vizdaw_app::alerts::AlertItem;
use vizdaw_app::{DocId, DocumentStore};

pub use app_state::AppState;
pub use menus::{
    CloseWindow, NewDemo, NewDocument, OpenDocument, PlayStop, Quit, Save, SaveAs, StopAndReturn,
};
pub use shell::DocumentWindow;

/// Creates the app state over `store`, binds the keys, sets the menus and
/// handles the app-level actions (New, New Demo, Open, Quit).
pub fn install(store: DocumentStore, cx: &mut App) -> gpui::Entity<AppState> {
    let state = app_state::create(store, cx);
    menus::install(cx);
    cx.on_action(|_: &NewDocument, cx| {
        new_document(cx);
    });
    cx.on_action(|_: &NewDemo, cx| {
        new_demo(cx);
    });
    cx.on_action(|_: &OpenDocument, cx| prompt_open(cx));
    cx.on_action(|_: &Quit, cx| quit(cx));
    state
}

/// File › New: an empty document in a new window.
pub fn new_document(cx: &mut App) -> Option<WindowHandle<DocumentWindow>> {
    let app = AppState::global(cx);
    let doc = app.update(cx, |s, cx| {
        cx.notify();
        s.store.new_empty()
    });
    show(doc, cx)
}

/// File › New Demo Project.
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
/// failure is an alert, and `None`.
pub fn open_path(path: &Path, cx: &mut App) -> Option<WindowHandle<DocumentWindow>> {
    let app = AppState::global(cx);
    let opened = app.update(cx, |s, cx| {
        cx.notify();
        s.store.open(path)
    });
    let doc = opened.ok()?;
    if let Some(entry) = app.read(cx).window_of(doc).cloned() {
        entry
            .handle
            .update(cx, |_, window, _| window.activate_window())
            .ok();
        return Some(entry.handle);
    }
    show(Some(doc), cx)
}

/// A window for `doc`; with none, the store's alert is all there is.
fn show(doc: Option<DocId>, cx: &mut App) -> Option<WindowHandle<DocumentWindow>> {
    let app = AppState::global(cx);
    let handle = doc.and_then(|d| DocumentWindow::open(&app, d, cx).ok());
    if handle.is_none() && app.read(cx).windows().is_empty() {
        // No window to show the alert in (slice 2: an app-level alert).
        if let Some(a) = app.read(cx).store.alert() {
            eprintln!("{}: {}", a.title, a.message);
        }
    }
    handle
}

/// File › Open…: GPUI's path prompt (a `.vizdaw` project is a folder).
pub fn prompt_open(cx: &mut App) {
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: true,
        multiple: true,
        prompt: Some("Open".into()),
    });
    cx.spawn(async move |cx| {
        let answer = chosen.await;
        cx.update(|cx| match answer {
            Ok(Ok(Some(paths))) => {
                for p in paths {
                    open_path(&p, cx);
                }
            }
            Ok(Err(e)) => {
                let app = AppState::global(cx);
                app.update(cx, |s, cx| {
                    s.store
                        .post_alert(AlertItem::new("Open Failed", e.to_string()));
                    cx.notify();
                });
            }
            _ => {}
        });
    })
    .detach();
}

/// Quit: with unsaved changes, asks first in the front window. Deferred:
/// Quit arrives from a key or menu while the front window is being
/// updated, and a window cannot be updated from inside its own update.
pub fn quit(cx: &mut App) {
    cx.defer(quit_or_ask);
}

fn quit_or_ask(cx: &mut App) {
    let app = AppState::global(cx);
    let (dirty, front) = {
        let s = app.read(cx);
        let dirty = s.windows().iter().any(|w| s.is_dirty(w.doc));
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
    let asked = front.update(cx, |_, window, cx| {
        window.prompt(
            PromptLevel::Warning,
            "Quit without saving?",
            Some("Some documents have unsaved changes."),
            &["Discard Changes and Quit", "Cancel"],
            cx,
        )
    });
    let Ok(answer) = asked else {
        eprintln!("Quit: the front window could not ask about unsaved changes");
        return;
    };
    cx.spawn(async move |cx| {
        if let Ok(0) = answer.await {
            cx.update(quit_now);
        }
    })
    .detach();
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

/// Starts the app with one new document window.
pub fn run() {
    gpui_platform::application().run(|cx: &mut App| {
        install(DocumentStore::live(), cx);
        cx.activate(true);
        new_document(cx);
    });
}
