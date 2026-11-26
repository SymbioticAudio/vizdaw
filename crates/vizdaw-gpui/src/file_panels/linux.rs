//! Linux: the xdg-desktop-portal file chooser through `ashpd` (the version
//! GPUI uses), with the spec's filter, accept label and folder, modal to
//! the window that asked.

use std::path::PathBuf;

use ashpd::WindowIdentifier;
use ashpd::desktop::file_chooser::{FileFilter, OpenFileRequest, SaveFileRequest, SelectedFiles};
use gpui::{AnyWindowHandle, App, Task};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use super::{Chosen, PortalFilter, PortalRequest};

/// GPUI's sentence for a missing portal.
const PORTAL_MISSING: &str =
    "Couldn't open file picker due to missing xdg-desktop-portal implementation.";

pub(super) fn run(
    request: PortalRequest,
    window: Option<AnyWindowHandle>,
    cx: &mut App,
) -> Task<Chosen> {
    cx.spawn(async move |cx| {
        let handles = window.and_then(|w| {
            cx.update(|cx| {
                w.update(cx, |_, window, _| {
                    let win = HasWindowHandle::window_handle(&*window).ok()?.as_raw();
                    let display = HasDisplayHandle::display_handle(&*window)
                        .ok()
                        .map(|d| d.as_raw());
                    Some((win, display))
                })
                .ok()
                .flatten()
            })
        });
        let identifier = match &handles {
            Some((win, display)) => WindowIdentifier::from_raw_handle(win, display.as_ref()).await,
            None => None,
        };
        let sent = if request.save {
            let mut r = SaveFileRequest::default()
                .identifier(identifier)
                .modal(true)
                .title(request.title.as_str())
                .accept_label(request.accept_label.as_str())
                .current_name(request.current_name.as_deref());
            if let Some(dir) = &request.current_folder {
                r = r.current_folder(dir).map_err(|e| e.to_string())?;
            }
            if let Some(f) = &request.filter {
                r = r.filter(filter(f));
            }
            r.send().await
        } else {
            let mut r = OpenFileRequest::default()
                .identifier(identifier)
                .modal(true)
                .title(request.title.as_str())
                .accept_label(request.accept_label.as_str())
                .multiple(request.multiple)
                .directory(request.directory);
            if let Some(dir) = &request.current_folder {
                r = r.current_folder(dir).map_err(|e| e.to_string())?;
            }
            if let Some(f) = &request.filter {
                r = r.filter(filter(f));
            }
            r.send().await
        };
        let request = match sent {
            Ok(r) => r,
            Err(ashpd::Error::PortalNotFound(_)) => return Err(PORTAL_MISSING.to_owned()),
            Err(e) => return Err(e.to_string()),
        };
        match request.response() {
            Ok(files) => Ok(Some(paths(&files))),
            Err(ashpd::Error::Response(_)) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })
}

fn filter(f: &PortalFilter) -> FileFilter {
    let mut out = FileFilter::new(&f.label);
    for m in &f.mime_types {
        out = out.mimetype(m);
    }
    for g in &f.globs {
        out = out.glob(g);
    }
    out
}

fn paths(files: &SelectedFiles) -> Vec<PathBuf> {
    files
        .uris()
        .iter()
        .filter_map(|u| url::Url::parse(u.as_str()).ok())
        .filter_map(|u| u.to_file_path().ok())
        .collect()
}
