//! The VizDAW GPUI application.

pub mod alerts;
pub mod app_state;
pub mod appearance;
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
pub mod input_picker;
pub mod learn_banner;
pub mod list_editor;
pub mod menu_bar;
pub mod menus;
mod midi_strip;
pub mod mixer_bar_menus;
pub mod mixer_view;
pub mod panes;
pub mod piano_roll;
pub mod plugin_browser;
pub mod plugin_windows;
pub mod plugins_pane;
pub mod record_popover;
pub mod region_drawing;
pub mod registry;
pub mod screenshot;
pub mod settings_window;
pub mod sheet_host;
pub mod sheets;
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
pub use appearance::current_theme;
pub use menus::{
    AppFileCommand, CloseWindow, FileCommand, NewDemo, NewDocument, OpenDocument, OpenSettings,
    PlayStop, Quit, RunCommand, Save, SaveAs, StopAndReturn,
};
pub use shell::DocumentWindow;

/// Creates the app state over `store`, binds the keys, sets the menus and
/// handles the app-level actions (New, the demos, Open, Import Project,
/// Quit).
pub fn install(store: DocumentStore, cx: &mut App) -> gpui::Entity<AppState> {
    let state = app_state::create(store, cx);
    // No window key acts while a file panel is up.
    file_panels::install(cx);
    // Esc cancels a waiting MIDI learn from any window. Before the sheet
    // host's: GPUI runs every interceptor in turn, so this one must see a
    // sheet still up (its Esc is the sheet's) before the sheet's Esc
    // closes it.
    learn_banner::install(cx);
    // A window's keys stay in its sheet while one is up.
    sheet_host::install(cx);
    // Alerts and questions show as prompts when the state changes.
    alerts::install(&state, cx);
    // Any quit closes every plug-in editor before a session drops.
    plugin_windows::install(cx);
    // The store's file reads (DAWproject and AAF opens) run off the UI
    // thread from now on.
    control::files::install_spawner(cx);
    let keymap = state.read(cx).keys.keymap().clone();
    menus::install(&keymap, cx);
    // File › New asks for the format first.
    cx.on_action(|_: &NewDocument, cx| file_menu::new_project(cx));
    file_menu::install(cx);
    cx.on_action(|_: &NewDemo, cx| {
        new_demo(cx);
    });
    cx.on_action(|_: &OpenDocument, cx| prompt_open(cx));
    cx.on_action(|_: &Quit, cx| quit(cx));
    // On a later turn: never inside the key's or the menu's own dispatch.
    cx.on_action(|_: &OpenSettings, cx| {
        cx.defer(|cx| settings_window::open_settings(None, cx));
    });
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
    if let Some(handle) = app
        .read(cx)
        .window_of(doc)
        .and_then(|w| w.document_window())
    {
        handle
            .update(cx, |_, window, _| window.activate_window())
            .ok();
        return Some(handle);
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
    let (dirty, front, waiting) = {
        let s = app.read(cx);
        let dirty = s.windows().iter().any(|w| s.has_unsaved_changes(w.doc));
        // The songs whose Save waits for their recording pass: Discard
        // Changes still saves them (their close lands the pass).
        let mut waiting: Vec<String> = vec![];
        for w in s.windows() {
            if let Some(d) = s.store.document(w.doc)
                && s.has_unsaved_changes(w.doc)
                && d.save_after_pass_pending()
                && !waiting.contains(&d.snapshot().title)
            {
                waiting.push(d.snapshot().title.clone());
            }
        }
        let front = s
            .store
            .front()
            .and_then(|d| s.window_of(d))
            .or_else(|| s.document_windows().next())
            .map(|w| w.handle);
        (dirty, front, waiting)
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
        front,
        sheet_host::Ask::with_labels(
            PromptLevel::Warning,
            "Quit without saving?",
            Some(&vizdaw_app::close_prompts::quit_detail(&waiting)),
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
        .filter_map(|w| w.document_window())
        .collect();
    for handle in handles {
        handle
            .update(cx, |v, window, cx| v.commit_rename(window, cx))
            .ok();
    }
}

/// Closes every plug-in editor, then every document (stopping its
/// transport, dropping it so no poll reads a closed session), and quits.
pub fn quit_now(cx: &mut App) {
    let app = AppState::global(cx);
    app.update(cx, |s, cx| {
        // Before any session drops: no plug-in outlives its parent view.
        s.close_plugin_windows();
        s.close_all();
        cx.notify();
    });
    cx.quit();
}

/// How often the plug-ins' run loop is pumped: plug-in timers are often
/// 16-30 ms, so faster than the 50 ms document idle.
pub const RUN_LOOP_EVERY: std::time::Duration = std::time::Duration::from_millis(10);

/// The plug-ins' main-thread run loop (CLAP timers and descriptors, VST3
/// `Linux::IRunLoop`): one app-wide loop on the main thread, where every
/// document's plug-ins and bounce copies load (the loop's table is per
/// thread, so one pump serves every document). Each pass polls without
/// waiting, under the plug-in registry's busy mark (a nested loop a
/// plug-in callback runs gets `Busy` from every registry call); nothing in
/// it calls back into the app.
pub fn pump_plugin_run_loop(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(RUN_LOOP_EVERY).await;
            vizdaw_ffi::editor::run_loop_pump();
        }
    })
    .detach();
}

/// How often the app polls the plug-in catalog while a scan runs or the
/// Plugin Manager is open (Swift's every 15th 60 Hz tick,
/// `S/VizDAWApp.swift:576-610`).
pub const PLUGIN_POLL_EVERY: std::time::Duration = std::time::Duration::from_millis(250);

/// Starts the app's plug-in catalog over `catalog` (scanning the new or
/// changed plug-ins with `scan`; Swift `openCatalog` and `startScan`,
/// `S/VizDAWApp.swift:515-542`) and its poll, every
/// [`PLUGIN_POLL_EVERY`] on the main thread ([`plugins_tick`]).
pub fn start_plugins(
    catalog: std::sync::Arc<dyn vizdaw_app::plugins::PluginCatalogCalls>,
    scan: bool,
    cx: &mut App,
) {
    AppState::global(cx).update(cx, |s, _| {
        s.app_plugins = vizdaw_app::plugins::AppPlugins::new(catalog);
        s.app_plugins.start(scan);
    });
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(PLUGIN_POLL_EVERY).await;
            cx.update(plugins_tick);
        }
    })
    .detach();
}

/// One pass of the plug-in poll: while a scan runs or the Plugin Manager
/// is open, the catalog's progress is read (main thread: it applies the
/// scan's results to the hosts); a change notifies the app state, which
/// redraws the Plugin Manager. Then the catalog's alerts show.
pub fn plugins_tick(cx: &mut App) {
    let app = AppState::global(cx);
    app.update(cx, |s, cx| {
        let wanted = s.app_plugins.needs_poll() || s.plugin_manager_open;
        if wanted && s.app_plugins.poll() {
            cx.notify();
        }
    });
    alerts::post_catalog_alerts(cx);
}

/// The platform the app runs on. On Linux with `DISPLAY` set it is X11
/// (XWayland in a Wayland session; ruling D2), so every plug-in editor
/// embeds; native-Wayland editor windows are a 27.x item.
fn platform_application() -> gpui::Application {
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()) {
        return gpui_platform::linux(gpui::WindowingModes::X11);
    }
    gpui_platform::application()
}

/// Starts the app with one new document window (the demo with
/// `--demo`), and the control channel with `--control PATH` (or
/// `VIZDAW_CONTROL`). A control socket that cannot be made exits with
/// status 2, so a launching agent learns at once.
pub fn run() {
    let options = control::LaunchOptions::from_env();
    platform_application().run(move |cx: &mut App| {
        let state = install(DocumentStore::live(), cx);
        pump_plugin_run_loop(cx);
        file_panels::use_native(cx);
        // Off macOS the platform has no prompt: GPUI's prompts are drawn
        // as the sheet card (with Return and Esc).
        if cfg!(not(target_os = "macos")) {
            sheet_host::use_sheet_prompts(cx);
        }
        // The preferences under the launch home (a throwaway e2e home
        // keeps its own).
        let prefs = vizdaw_app::Prefs::open();
        launch(&state, prefs, options.demo, cx, |cx| {
            // Before any document opens, so their plug-ins load; the
            // scan runs in the background.
            start_plugins(
                vizdaw_app::plugins::live_plugin_catalog(),
                !options.no_scan,
                cx,
            );
            // Before any document opens.
            if let Some(path) = &options.control
                && let Err(e) = control::start(path, cx)
            {
                eprintln!("{e}");
                std::process::exit(2);
            }
            cx.activate(true);
        });
    });
}

/// The launch's steps over the installed `state`, in order: the
/// preferences; `surfaces.json` (after the preferences and before the
/// first document, as Swift read it after setting the support folder,
/// `S/VizDAWApp.swift:515-522`; the pickers then mark the surfaces'
/// ports); the saved key bindings; `before_documents` (the plug-in
/// catalog and the control channel); the pickers' sources, off the UI
/// thread; then the first window, the demo when `demo`.
pub fn launch(
    state: &gpui::Entity<AppState>,
    prefs: vizdaw_app::Prefs,
    demo: bool,
    cx: &mut App,
    before_documents: impl FnOnce(&mut App),
) -> Option<WindowHandle<DocumentWindow>> {
    state.update(cx, |s, _| s.set_prefs(prefs));
    state.update(cx, |s, _| s.load_surfaces());
    // The saved key bindings, in the keys and the menus.
    menus::rebind(cx);
    before_documents(cx);
    // The picker's sources, off the UI thread, for every document.
    state.update(cx, |s, _| s.warm_sources());
    if demo { new_demo(cx) } else { new_document(cx) }
}
