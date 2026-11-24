//! macOS: an `NSOpenPanel` or `NSSavePanel` with Swift's settings, shown
//! modeless with a completion handler. Not `runModal`: GPUI runs its
//! tasks from main-queue blocks, and a nested modal loop entered inside
//! one never drains the main queue, so every GPUI task, timer, frame and
//! control reply would stop while the panel is up.

use std::cell::RefCell;
use std::path::PathBuf;

use block2::RcBlock;
use futures::channel::oneshot;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSModalResponse, NSModalResponseOK, NSOpenPanel, NSSavePanel};
use objc2_foundation::{NSArray, NSString, NSURL};
use objc2_uniform_type_identifiers::UTType;

use super::{Chosen, MacPanel, MacType};

/// Off the main thread no panel can be shown.
const NOT_MAIN: &str = "A file panel can only be shown from the main thread.";

/// Shows `panel` and returns at once; the answer goes to `done` when it
/// closes (the chosen paths, `None` when cancelled).
pub(super) fn present(panel: &MacPanel, done: oneshot::Sender<Chosen>) {
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
        if let Some(tx) = done.borrow_mut().take() {
            tx.send(Ok(chosen)).ok();
        }
    };
    if panel.save {
        let p = NSSavePanel::savePanel(mtm);
        p.setCanCreateDirectories(panel.can_create_folders);
        p.setTreatsFilePackagesAsDirectories(panel.packages_as_folders);
        p.setPrompt(Some(&NSString::from_str(&panel.prompt)));
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
        p.beginWithCompletionHandler(&handler);
        return;
    }
    let p = NSOpenPanel::openPanel(mtm);
    p.setCanChooseFiles(panel.choose_files);
    p.setCanChooseDirectories(panel.choose_folders);
    p.setAllowsMultipleSelection(panel.multiple);
    p.setCanCreateDirectories(panel.can_create_folders);
    p.setTreatsFilePackagesAsDirectories(panel.packages_as_folders);
    p.setPrompt(Some(&NSString::from_str(&panel.prompt)));
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
    p.beginWithCompletionHandler(&handler);
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
        present(&panel, tx);
        assert_eq!(rx.try_recv(), Ok(Some(Err(NOT_MAIN.to_owned()))));
    }
}
