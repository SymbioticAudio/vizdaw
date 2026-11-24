//! macOS: an `NSOpenPanel` or `NSSavePanel` run app-modal (`runModal`), as
//! Swift's `Panels` ran them.

use std::path::PathBuf;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel, NSSavePanel};
use objc2_foundation::{NSArray, NSString, NSURL};
use objc2_uniform_type_identifiers::UTType;

use super::{MacPanel, MacType};

/// Shows `panel` and waits for it: the chosen paths, `None` when
/// cancelled. Must run on the main thread outside any GPUI update.
pub(super) fn run(panel: &MacPanel) -> Option<Vec<PathBuf>> {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("file panel: not on the main thread");
        return None;
    };
    let types: Vec<Retained<UTType>> = panel.types.iter().filter_map(ut_type).collect();
    let types = NSArray::from_retained_slice(&types);
    let directory = panel.directory.as_ref().map(|d| {
        NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&d.to_string_lossy()), true)
    });
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
        if p.runModal() != NSModalResponseOK {
            return None;
        }
        return p.URL().and_then(|u| path(&u)).map(|p| vec![p]);
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
    if p.runModal() != NSModalResponseOK {
        return None;
    }
    Some(p.URLs().iter().filter_map(|u| path(&u)).collect())
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
