//! macOS: an `NSOpenPanel` or `NSSavePanel` with Swift's settings, shown
//! with a completion handler: as a sheet on the window that asked, else
//! (no window) modeless at the modal-panel level, so no click into a
//! document window can bring it in front of the panel (keys and menus are
//! off while a panel is up: a hidden panel would look like a frozen app).
//! Not `runModal`: GPUI runs its tasks from main-queue blocks, and a
//! nested modal loop entered inside one never drains the main queue, so
//! every GPUI task, timer, frame and control reply would stop while the
//! panel is up.

use std::cell::RefCell;
use std::path::PathBuf;

use block2::RcBlock;
use futures::channel::oneshot;
use gpui::{AnyWindowHandle, App};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::{
    NSModalPanelWindowLevel, NSModalResponse, NSModalResponseCancel, NSModalResponseOK,
    NSOpenPanel, NSSavePanel, NSView, NSWindow,
};
use objc2_foundation::{NSArray, NSString, NSURL};
use objc2_uniform_type_identifiers::UTType;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{Chosen, MacPanel, MacType};

thread_local! {
    /// The panel shown as a sheet now, and the window it is on (one panel
    /// at a time): [`end`] ends it when that window closes.
    static SHEET: RefCell<Option<(Retained<NSWindow>, Retained<NSSavePanel>)>> =
        const { RefCell::new(None) };
}

/// Ends the panel's sheet with Cancel (its completion answers `None`):
/// its window is closing. GPUI ends only its own sheets when it drops a
/// window, and a sheet left on a closed window never answers.
pub(super) fn end() {
    // Taken first: the completion may run inside `endSheet` and clears
    // the slot itself.
    let Some((parent, panel)) = SHEET.with(|s| s.borrow_mut().take()) else {
        return;
    };
    parent.endSheet_returnCode(&panel, NSModalResponseCancel);
}

/// Off the main thread no panel can be shown.
const NOT_MAIN: &str = "A file panel can only be shown from the main thread.";

/// The AppKit window of GPUI window `window` (`None` once it closed, or
/// while it is being updated).
pub(super) fn ns_window(window: AnyWindowHandle, cx: &mut App) -> Option<Retained<NSWindow>> {
    window
        .update(cx, |_, window, _| {
            let RawWindowHandle::AppKit(h) =
                HasWindowHandle::window_handle(&*window).ok()?.as_raw()
            else {
                return None;
            };
            // SAFETY: GPUI's AppKit handle is its window's live content
            // view, on the main thread (we are inside the window's update).
            let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
            view.window()
        })
        .ok()
        .flatten()
}

/// Shows `panel` (as a sheet on `parent` when given) and returns at once;
/// the answer goes to `done` when it closes (the chosen paths, `None` when
/// cancelled).
pub(super) fn present(
    panel: &MacPanel,
    parent: Option<Retained<NSWindow>>,
    done: oneshot::Sender<Chosen>,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        done.send(Err(NOT_MAIN.to_owned())).ok();
        return;
    };
    let types: Vec<Retained<UTType>> = panel.types.iter().filter_map(ut_type).collect();
    let types = NSArray::from_retained_slice(&types);
    let directory = panel.directory.as_ref().map(|d| {
        NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&d.to_string_lossy()), true)
    });
    let done = RefCell::new(Some(done));
    let answer = move |chosen: Option<Vec<PathBuf>>| {
        SHEET.with(|s| s.borrow_mut().take());
        if let Some(tx) = done.borrow_mut().take() {
            tx.send(Ok(chosen)).ok();
        }
    };
    if panel.save {
        let p = NSSavePanel::savePanel(mtm);
        p.setCanCreateDirectories(panel.can_create_folders);
        p.setTreatsFilePackagesAsDirectories(panel.packages_as_folders);
        p.setPrompt(Some(&NSString::from_str(&panel.prompt)));
        if let Some(t) = &panel.title {
            p.setTitle(Some(&NSString::from_str(t)));
        }
        if let Some(m) = &panel.message {
            p.setMessage(Some(&NSString::from_str(m)));
        }
        if !types.is_empty() {
            p.setAllowedContentTypes(&types);
        }
        if let Some(d) = &directory {
            p.setDirectoryURL(Some(d));
        }
        if let Some(n) = &panel.name {
            p.setNameFieldStringValue(&NSString::from_str(n));
        }
        let shown = p.clone();
        let handler = RcBlock::new(move |response: NSModalResponse| {
            answer(
                (response == NSModalResponseOK)
                    .then(|| shown.URL().and_then(|u| path(&u)).into_iter().collect()),
            );
        });
        begin(&p, parent.as_deref(), &handler);
        return;
    }
    let p = NSOpenPanel::openPanel(mtm);
    p.setCanChooseFiles(panel.choose_files);
    p.setCanChooseDirectories(panel.choose_folders);
    p.setAllowsMultipleSelection(panel.multiple);
    p.setCanCreateDirectories(panel.can_create_folders);
    p.setTreatsFilePackagesAsDirectories(panel.packages_as_folders);
    p.setPrompt(Some(&NSString::from_str(&panel.prompt)));
    if let Some(t) = &panel.title {
        p.setTitle(Some(&NSString::from_str(t)));
    }
    if let Some(m) = &panel.message {
        p.setMessage(Some(&NSString::from_str(m)));
    }
    if !types.is_empty() {
        p.setAllowedContentTypes(&types);
    }
    if let Some(d) = &directory {
        p.setDirectoryURL(Some(d));
    }
    let shown = p.clone();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        answer(
            (response == NSModalResponseOK)
                .then(|| shown.URLs().iter().filter_map(|u| path(&u)).collect()),
        );
    });
    begin(&p, parent.as_deref(), &handler);
}

/// A sheet on `parent`; with none, modeless but raised to the modal-panel
/// level and ordered front (a document window cannot cover it).
fn begin(
    panel: &NSSavePanel,
    parent: Option<&NSWindow>,
    handler: &block2::DynBlock<dyn Fn(NSModalResponse)>,
) {
    match parent {
        Some(w) => {
            SHEET.with(|s| *s.borrow_mut() = Some((w.retain(), panel.retain())));
            panel.beginSheetModalForWindow_completionHandler(w, handler);
        }
        None => {
            panel.beginWithCompletionHandler(handler);
            panel.setLevel(NSModalPanelWindowLevel);
            panel.orderFrontRegardless();
        }
    }
}

fn ut_type(t: &MacType) -> Option<Retained<UTType>> {
    match t {
        MacType::Identifier(id) => UTType::typeWithIdentifier(&NSString::from_str(id)),
        MacType::Extension(ext) => UTType::typeWithFilenameExtension(&NSString::from_str(ext)),
    }
}

fn path(url: &NSURL) -> Option<PathBuf> {
    url.path().map(|p| PathBuf::from(p.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test threads are not the main thread: the panel refuses with an
    /// error rather than a silent cancel.
    #[test]
    fn off_the_main_thread_a_panel_answers_an_error() {
        let (tx, mut rx) = oneshot::channel();
        let panel = super::super::mac_panel(
            &vizdaw_app::panels::OPEN_PROJECT,
            &super::super::PanelPlace::default(),
        );
        present(&panel, None, tx);
        assert_eq!(rx.try_recv(), Ok(Some(Err(NOT_MAIN.to_owned()))));
    }
}
