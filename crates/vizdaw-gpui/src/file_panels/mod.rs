//! The native file panels (slice 7b Task 1; inventory §2.5): one
//! [`PanelSpec`] from `vizdaw_app::panels` becomes a macOS
//! `NSOpenPanel`/`NSSavePanel` (shown with a completion handler; the app
//! refuses menus and other panels while it is up, for Swift's app-modal
//! `runModal`) or a Linux xdg-desktop-portal file chooser with the spec's
//! filter, accept label and folder. GPUI's own path prompts take no types, messages or
//! save prompts, and on Linux cannot choose files and folders together.
//!
//! Each request is built as a plain struct first ([`MacPanel`],
//! [`PortalRequest`]) so tests can check it without showing a panel.
//! Panels never run inside an entity update: [`choose`] shows them from a
//! deferred call. One panel is up at a time: a request while one is up is
//! refused quietly (Swift's app-modal panels allowed no second one).
//!
//! The test platform has no native panels: until [`use_native`] is called
//! (by [`crate::run`]), [`choose`] uses GPUI's path prompts, which tests
//! answer with `simulate_new_path_selection`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use futures::channel::oneshot;

use gpui::{AnyWindowHandle, App, Global, Keystroke, PathPromptOptions, Task};
use vizdaw_app::panels::{FileType, PanelKind, PanelSpec, replace_question, save_target};

/// What the caller fills in for one showing of a panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelPlace {
    /// The folder the panel starts in (a spec's `Given` or
    /// `CoProducerCopies` start, resolved by the caller).
    pub directory: Option<PathBuf>,
    /// The `{name}` of the spec's message and default name.
    pub name: String,
    /// The `{ext}` of a spec with a given extension (Bounce, MIDI export).
    pub ext: String,
}

/// A macOS type the panel allows: a declared identifier (skipped when the
/// system does not know it) or a file name extension's type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MacType {
    Identifier(String),
    Extension(String),
}

/// An `NSOpenPanel` (or `NSSavePanel` when `save`) as Swift configured it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacPanel {
    pub save: bool,
    pub choose_files: bool,
    pub choose_folders: bool,
    pub multiple: bool,
    pub can_create_folders: bool,
    /// `treatsFilePackagesAsDirectories` (always false: a project is
    /// chosen whole).
    pub packages_as_folders: bool,
    pub prompt: String,
    pub title: Option<String>,
    pub message: Option<String>,
    /// `allowedContentTypes`; empty allows any.
    pub types: Vec<MacType>,
    pub directory: Option<PathBuf>,
    /// `nameFieldStringValue` (save panels).
    pub name: Option<String>,
}

/// The macOS panel for `spec` at `place`.
pub fn mac_panel(spec: &PanelSpec, place: &PanelPlace) -> MacPanel {
    let mut types: Vec<MacType> = Vec::new();
    let mut add = |t: MacType| {
        if !types.contains(&t) {
            types.push(t);
        }
    };
    for t in spec.types {
        match t {
            FileType::GivenExtension => add(MacType::Extension(place.ext.clone())),
            _ => {
                for id in t.identifiers() {
                    add(MacType::Identifier((*id).to_owned()));
                }
                // The GPUI bundle declares no project type yet, so a
                // `.vizdaw` is a folder: allow folders where projects are.
                if *t == FileType::Project {
                    add(MacType::Identifier("public.folder".to_owned()));
                }
                for ext in t.extensions() {
                    add(MacType::Extension((*ext).to_owned()));
                }
            }
        }
    }
    MacPanel {
        save: spec.kind == PanelKind::Save,
        choose_files: spec.chooses_files(),
        choose_folders: spec.chooses_folders(),
        multiple: spec.multiple,
        can_create_folders: spec.can_create_folders,
        packages_as_folders: false,
        prompt: spec.prompt.to_owned(),
        title: spec.title.map(str::to_owned),
        message: spec.message_for(&place.name),
        types,
        directory: place.directory.clone(),
        name: spec.default_name_for(&place.name, &place.ext),
    }
}

/// A portal file filter: a label and its patterns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalFilter {
    pub label: String,
    pub globs: Vec<String>,
    pub mime_types: Vec<String>,
}

/// An xdg-desktop-portal `OpenFile` (or `SaveFile` when `save`) request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalRequest {
    pub save: bool,
    /// The dialog title: Swift's message (the portal shows no message
    /// line), else the prompt.
    pub title: String,
    pub accept_label: String,
    pub multiple: bool,
    /// A folder chooser (the portal chooses files or folders, never both).
    pub directory: bool,
    pub filter: Option<PortalFilter>,
    pub current_folder: Option<PathBuf>,
    pub current_name: Option<String>,
}

/// The Linux portal request for `spec` at `place`. An open panel that
/// allows projects chooses files: a project is chosen by its
/// `project.json`, which the store maps to the `.vizdaw` folder (owner
/// ruling 2026-11-23); plain folders are not offered.
pub fn portal_request(spec: &PanelSpec, place: &PanelPlace) -> PortalRequest {
    let directory = spec.kind == PanelKind::Folder;
    let mut globs: Vec<String> = Vec::new();
    let mut mime_types: Vec<String> = Vec::new();
    let mut labels: Vec<&str> = Vec::new();
    for t in spec.types {
        let patterns: Vec<String> = match t {
            FileType::Folder => continue,
            FileType::Project => vec!["project.json".to_owned()],
            FileType::GivenExtension => vec![format!("*.{}", place.ext)],
            _ => t.extensions().iter().map(|e| format!("*.{e}")).collect(),
        };
        match t {
            FileType::Audio => mime_types.push("audio/*".to_owned()),
            FileType::PlainText => mime_types.push("text/plain".to_owned()),
            FileType::Midi => {
                mime_types.push("audio/midi".to_owned());
                mime_types.push("audio/x-midi".to_owned());
            }
            _ => {}
        }
        labels.push(t.label());
        for p in patterns {
            if !globs.contains(&p) {
                globs.push(p);
            }
        }
    }
    let filter =
        (!directory && !(globs.is_empty() && mime_types.is_empty())).then(|| PortalFilter {
            label: sentence_list(&labels),
            globs,
            mime_types,
        });
    PortalRequest {
        save: spec.kind == PanelKind::Save,
        title: spec
            .title
            .map(str::to_owned)
            .or_else(|| spec.message_for(&place.name))
            .unwrap_or_else(|| spec.prompt.to_owned()),
        accept_label: spec.prompt.to_owned(),
        multiple: spec.multiple,
        directory,
        filter,
        current_folder: place.directory.clone(),
        current_name: spec.default_name_for(&place.name, &place.ext),
    }
}

/// "a", "a or b", "a, b or c", with the first letter capitalised.
fn sentence_list(items: &[&str]) -> String {
    let text = match items {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    };
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => text,
    }
}

/// What a panel answered: the chosen paths, `None` when cancelled or
/// refused (another panel was up), an error when it could not be shown.
pub type Chosen = Result<Option<Vec<PathBuf>>, String>;

/// Shows panels. `show` returns at once; the answer is sent on `done`
/// once the panel closes (a dropped sender reads as cancelled).
pub trait PanelBackend {
    fn show(
        &self,
        spec: &PanelSpec,
        place: &PanelPlace,
        window: Option<AnyWindowHandle>,
        done: oneshot::Sender<Chosen>,
        cx: &mut App,
    );

    /// The panel's parent `window` is closing while the panel is up: end
    /// it (as cancelled) before the window goes. The panel layer reads the
    /// panel as cancelled either way.
    fn end(&self, _window: AnyWindowHandle, _cx: &mut App) {}
}

/// GPUI's path prompts (the test platform's, answered by tests).
struct GpuiPrompts;

/// The platform's own panels.
struct NativePanels;

struct PanelState {
    backend: Rc<dyn PanelBackend>,
    up: bool,
}

impl Default for PanelState {
    fn default() -> Self {
        PanelState {
            backend: Rc::new(GpuiPrompts),
            up: false,
        }
    }
}

impl Global for PanelState {}

/// Shows the platform's own panels from now on (the real app).
pub fn use_native(cx: &mut App) {
    set_backend(Rc::new(NativePanels), cx);
}

/// Shows panels through `backend` from now on (tests).
#[doc(hidden)]
pub fn set_backend(backend: Rc<dyn PanelBackend>, cx: &mut App) {
    cx.default_global::<PanelState>().backend = backend;
}

/// Whether a panel is up now.
pub fn is_up(cx: &App) -> bool {
    cx.try_global::<PanelState>().is_some_and(|s| s.up)
}

/// Whether `key` must not reach a window now: while a panel is up no
/// window key acts (Swift's panels were app-modal), apart from Esc and
/// Cmd-. , which belong to the panel (AppKit's cancel keys).
pub fn blocks_key(key: &Keystroke, cx: &App) -> bool {
    let cancel = key.key == "escape" || (key.modifiers.platform && key.key == ".");
    is_up(cx) && !cancel
}

/// Keeps keys from every window while a panel is up ([`blocks_key`]): an
/// interceptor stops the key before bindings match, and GPUI then calls
/// no key-down handler either (they run only while the event propagates).
pub fn install(cx: &mut App) {
    cx.intercept_keystrokes(|e, _, cx| {
        if blocks_key(&e.keystroke, cx) {
            cx.stop_propagation();
        }
    })
    .detach();
}

fn set_up(up: bool, cx: &mut App) {
    cx.default_global::<PanelState>().up = up;
    // The platform menus are off while a panel is up (Swift's panels were
    // app-modal).
    crate::menus::refresh(cx);
}

/// Shows `spec`'s panel, parented to `window` where the platform can. The
/// panel is shown from a deferred call (never inside the caller's
/// update) and answers through its completion, so GPUI keeps running
/// while it is up. While it is up a second request answers `Ok(None)` at
/// once. The panel-up flag is cleared when the panel answers, whether or
/// not the caller still holds the returned task. When `window` closes
/// first, the backend ends the panel and it reads as cancelled.
pub fn choose(
    spec: &PanelSpec,
    place: PanelPlace,
    window: Option<AnyWindowHandle>,
    cx: &mut App,
) -> Task<Chosen> {
    if is_up(cx) {
        return Task::ready(Ok(None));
    }
    set_up(true, cx);
    let backend = cx.default_global::<PanelState>().backend.clone();
    let (done, answered) = oneshot::channel::<Chosen>();
    let (out, answer) = oneshot::channel::<Chosen>();
    let spec = *spec;
    // Its window closing ends the panel and reads as cancelled, so the
    // panel-up flag clears even if the panel never answers.
    let (closed, gone) = match window {
        Some(w) => {
            let backend = backend.clone();
            let (tx, rx) = oneshot::channel::<()>();
            let mut tx = Some(tx);
            let sub = cx.on_window_closed(move |cx, id| {
                if id == w.window_id()
                    && let Some(tx) = tx.take()
                {
                    backend.end(w, cx);
                    tx.send(()).ok();
                }
            });
            (Some(sub), Some(rx))
        }
        None => (None, None),
    };
    cx.defer(move |cx| backend.show(&spec, &place, window, done, cx));
    cx.spawn(async move |cx| {
        let chosen = match gone {
            Some(gone) => match futures::future::select(answered, gone).await {
                futures::future::Either::Left((a, _)) => a.unwrap_or(Ok(None)),
                futures::future::Either::Right(_) => Ok(None),
            },
            None => answered.await.unwrap_or(Ok(None)),
        };
        drop(closed);
        cx.update(|cx| set_up(false, cx));
        out.send(chosen).ok();
    })
    .detach();
    cx.spawn(async move |_| answer.await.unwrap_or(Ok(None)))
}

/// A save panel's answer after the app's own checks: the extension added
/// when it was missing and, when that names a file already there, the
/// replace question asked in `window` ([`replace_question`]). `Ok(None)`
/// when cancelled at either step.
pub fn choose_save(
    spec: &PanelSpec,
    place: PanelPlace,
    window: AnyWindowHandle,
    cx: &mut App,
) -> Task<Result<Option<PathBuf>, String>> {
    let spec = *spec;
    let ext = place.ext.clone();
    let chosen = choose(&spec, place, Some(window), cx);
    cx.spawn(async move |cx| {
        let Some(path) = chosen.await?.and_then(|p| p.into_iter().next()) else {
            return Ok(None);
        };
        let target = save_target(&spec, &path, &ext, Path::exists);
        if !target.ask_replace {
            return Ok(Some(target.path));
        }
        let (title, detail) = replace_question(&spec, &target.path);
        // Cancel is the default (Return and Esc); Replace must be clicked.
        let ask = crate::sheet_host::Ask::replace(&title, Some(&detail));
        let answer = cx.update(|cx| crate::alerts::ask(window, ask.clone(), cx));
        let replace = answer.await.is_ok_and(|i| ask.replaces(i));
        Ok(replace.then_some(target.path))
    })
}

impl PanelBackend for GpuiPrompts {
    fn show(
        &self,
        spec: &PanelSpec,
        place: &PanelPlace,
        _window: Option<AnyWindowHandle>,
        done: oneshot::Sender<Chosen>,
        cx: &mut App,
    ) {
        if spec.kind == PanelKind::Save {
            let dir = place
                .directory
                .clone()
                .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("/"));
            let name = spec.default_name_for(&place.name, &place.ext);
            let rx = cx.prompt_for_new_path(&dir, name.as_deref());
            cx.spawn(async move |_| {
                let chosen = match rx.await {
                    Ok(Ok(path)) => Ok(path.map(|p| vec![p])),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Ok(None),
                };
                done.send(chosen).ok();
            })
            .detach();
            return;
        }
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: spec.chooses_files(),
            directories: spec.chooses_folders(),
            multiple: spec.multiple,
            prompt: Some(spec.prompt.into()),
        });
        cx.spawn(async move |_| {
            let chosen = match rx.await {
                Ok(Ok(paths)) => Ok(paths),
                Ok(Err(e)) => Err(e.to_string()),
                Err(_) => Ok(None),
            };
            done.send(chosen).ok();
        })
        .detach();
    }
}

impl PanelBackend for NativePanels {
    #[cfg(target_os = "macos")]
    fn show(
        &self,
        spec: &PanelSpec,
        place: &PanelPlace,
        window: Option<AnyWindowHandle>,
        done: oneshot::Sender<Chosen>,
        cx: &mut App,
    ) {
        // With a completion handler (a sheet on the asking window, else
        // raised and modeless): a `runModal` here would run inside GPUI's
        // main-queue drain and stop every GPUI task, timer and frame until
        // the panel closed.
        let parent = window.and_then(|w| macos::ns_window(w, cx));
        macos::present(&mac_panel(spec, place), parent, done);
    }

    /// A sheet whose window closes is ended here: GPUI ends only its own
    /// sheets when it drops a window.
    #[cfg(target_os = "macos")]
    fn end(&self, _window: AnyWindowHandle, _cx: &mut App) {
        macos::end();
    }

    #[cfg(target_os = "linux")]
    fn show(
        &self,
        spec: &PanelSpec,
        place: &PanelPlace,
        window: Option<AnyWindowHandle>,
        done: oneshot::Sender<Chosen>,
        cx: &mut App,
    ) {
        let task = linux::run(portal_request(spec, place), window, cx);
        cx.spawn(async move |_| {
            done.send(task.await).ok();
        })
        .detach();
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn show(
        &self,
        spec: &PanelSpec,
        place: &PanelPlace,
        window: Option<AnyWindowHandle>,
        done: oneshot::Sender<Chosen>,
        cx: &mut App,
    ) {
        GpuiPrompts.show(spec, place, window, done, cx);
    }
}
