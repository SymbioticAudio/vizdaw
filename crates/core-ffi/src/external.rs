//! The external sample editor's copies (sub-project 5 spec §11; plan 5f-3
//! Rulings 2, 3): their names, rows and runtime record, and the file work
//! the session does for them. The session keeps the live copies
//! (`Inner.edits`) and runs the copy and the import (`session.rs`).

use crate::error::VizError;
use core_media::export::ExportFormat;
use core_model::document::{Document, MediaOrigin};
use core_model::event::EventBody;
use core_model::ids::{EventId, MediaId};
use std::path::{Component, Path, PathBuf};

/// Where edit copies go, package-relative.
pub const EDITS_DIR: &str = "Media/Edits";
/// The longest stem, in UTF-8 bytes, so `.<stem> edit 9999.wav.partial`
/// stays within 255 bytes (pre-flight M3).
pub const MAX_STEM_BYTES: usize = 200;
/// The longest path `apply_external_edit` takes, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// Open on a region whose media is offline (Ruling 2).
pub const OFFLINE: &str = "The region's audio is offline; relink it first.";
/// Reload or apply on a region without a copy (pre-flight I10).
pub const NO_EDIT: &str = "This region has no external edit; use Open in External Editor.";
/// A copy that is not on disk, or not in this session (Ruling 3).
pub const GONE: &str = "The edit copy is gone; Open in External Editor again.";
/// An edit of a region that is gone (Ruling 6).
pub const REGION_DELETED: &str = "The region was deleted; the edit was not applied.";

/// A region's copy in this session (Ruling 3; runtime only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditCopy {
    /// The copy, absolute (pre-flight I1): Save As leaves it where it is.
    pub(crate) abs: PathBuf,
    /// The media item the edit started from.
    pub(crate) from: MediaId,
}

/// A region's edit copy as the app sees it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ExternalEditRow {
    /// The region.
    pub event: u64,
    /// The copy's absolute path.
    pub path: String,
    /// The media item the edit started from.
    pub from: u64,
}

/// What Reload finds for a region (Ruling 7; pre-flight I10).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ExternalEditStateRow {
    /// Its copy in this session, on disk.
    Copy {
        /// The copy.
        row: ExternalEditRow,
    },
    /// Its copy is gone: not on disk, or not in this session while its
    /// media came from an external edit (the project was reopened).
    Gone,
    /// It has no external edit.
    NoEdit,
}

/// `name` as a file stem (Review Focus 4; pre-flight M3): `/`, `\`, `:`
/// and control characters become `-`, spaces at the ends go, a leading
/// `.` becomes `-` (a dot-name is hidden, and retention never sees it),
/// the result is cut to [`MAX_STEM_BYTES`] at a character boundary, and an
/// empty stem is "Audio".
pub fn safe_stem(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let mut s = mapped.trim().to_string();
    if s.starts_with('.') {
        s.replace_range(..1, "-");
    }
    let mut cut = s.len().min(MAX_STEM_BYTES);
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    let s = s.trim_end();
    if s.is_empty() {
        "Audio".to_string()
    } else {
        s.to_string()
    }
}

/// The copy's file name for media `name` and number `n`, through the one
/// pattern [`core_media::export::write_span`] writes (final review M11).
pub fn copy_name(name: &str, n: u32) -> String {
    core_media::export::edit_copy_name(&safe_stem(name), n)
}

/// Whether `p`, a path the app sent, names `abs`, a region's copy (final
/// review I1): the same path, or, with no `..` in it, the same regular
/// file on disk (the same package spelled another way after Save As: a
/// symlinked folder, another case). Any other file is not the copy.
pub(crate) fn names_copy(abs: &Path, p: &Path) -> bool {
    if abs == p {
        return true;
    }
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        return false;
    }
    same_file(abs, p)
}

/// Whether `a` and `b` are one regular file (its device and inode; a
/// symlink in the last place is not followed).
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
        (Ok(x), Ok(y)) => x.is_file() && y.is_file() && x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

/// Whether `a` and `b` are one regular file (their canonical paths).
#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    let file = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file());
    file(a) && file(b) && matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// The name a copy of `media` is called after (final review M10): the
/// item the edits started from, following `ExternalEdit { from }` back,
/// so names do not grow on every round trip; `media`'s own when that item
/// is gone.
pub(crate) fn root_name(doc: &Document, media: MediaId) -> Option<String> {
    let items = doc.media();
    let mut item = items.get(&media)?;
    for _ in 0..items.len() {
        match &item.origin {
            MediaOrigin::ExternalEdit { from } => match items.get(from) {
                Some(up) => item = up,
                None => break,
            },
            _ => break,
        }
    }
    Some(item.name.clone())
}

/// Region `event`'s media and span; `Model` for an unknown event or one
/// that is no audio region.
pub(crate) fn region_span(doc: &Document, event: EventId) -> Result<(MediaId, u64, u64), VizError> {
    let (ev, _) = doc.event(event)?;
    match &ev.body {
        EventBody::AudioRegion {
            media,
            source_offset,
            length,
            ..
        } => Ok((*media, *source_offset, *length)),
        _ => Err(VizError::Model {
            message: format!("{event} is not an audio region; select one audio region."),
        }),
    }
}

/// `abs` as a package-relative, `/`-joined path when it lies under
/// `package` (retention compares those; pre-flight I1).
pub(crate) fn package_relative(abs: &Path, package: &Path) -> Option<String> {
    let rel = abs.strip_prefix(package).ok()?;
    let parts: Option<Vec<&str>> = rel
        .components()
        .map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    Some(parts?.join("/"))
}

/// Writes the copy (Ruling 2): `dir` (the package's `Media/Edits`) is made
/// when missing and refused when something else is there, then
/// [`core_media::export::write_span`]. The copy's absolute path.
pub(crate) fn write_copy(
    source: &Path,
    start: u64,
    frames: u64,
    dir: &Path,
    stem: &str,
    fmt: &ExportFormat,
) -> Result<PathBuf, VizError> {
    if std::fs::symlink_metadata(dir).is_ok_and(|m| !m.is_dir()) {
        return Err(VizError::Io {
            message: format!("{EDITS_DIR} is a file, not a folder; move it away and try again."),
        });
    }
    std::fs::create_dir_all(dir).map_err(|e| VizError::Io {
        message: format!("{EDITS_DIR}: {e}"),
    })?;
    core_media::export::write_span(source, start, frames, dir, stem, fmt)
        .map(|e| e.path)
        .map_err(|e| VizError::Io {
            message: e.to_string(),
        })
}

/// Whether `name` in `Media/Edits` is a partial a copy is writing (or a
/// crash left): not media; it goes with the folder (fold check M8).
pub(crate) fn partial(name: &str) -> bool {
    name.starts_with('.')
}

/// Moves every regular file in `dir` to the Trash (`trash`: the folder
/// standing for it, as [`crate::retention::trash_dir`] resolved it, else
/// the platform's), except those whose file name `skip` takes; the count
/// left behind (`dir` unreadable, an entry not read, or a file not moved),
/// each named on stderr. `Media/Edits` skips dot-names ([`partial`]): only
/// a copy being written or a crash's partial is dot-named there.
/// `Media/Recorded` skips none at close (a take may be dot-named, and
/// recordings are never unlinked: spec §6.5; plan 5f-3 fix round 1, ruling
/// T3-1), and at an untitled Save As only those the project carried
/// (ruling F-1). A missing `dir` leaves none. Never unlinks (plan 5c
/// Ruling 15; plan 5f-3 owner answer 2026-11-03).
pub(crate) fn trash_files_in(
    dir: &Path,
    trash: Option<&Path>,
    skip: &dyn Fn(&str) -> bool,
) -> usize {
    let rd = match std::fs::read_dir(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return 0,
        // Unreadable: files may be in it (5c review minor b).
        Err(e) => {
            eprintln!("VizDAW: {}: not read ({e})", dir.display());
            return 1;
        }
        Ok(rd) => rd,
    };
    let mut left = 0;
    for e in rd {
        // An entry not read may be a file (review M3).
        let e = match e {
            Ok(e) => e,
            Err(why) => {
                eprintln!("VizDAW: {}: an entry was not read ({why})", dir.display());
                left += 1;
                continue;
            }
        };
        if skip(&e.file_name().to_string_lossy()) {
            continue;
        }
        if e.path().is_file() {
            if let Err(why) = crate::retention::trash(trash, &e.path()) {
                eprintln!(
                    "VizDAW: {}: not moved to the Trash ({why})",
                    e.path().display()
                );
                left += 1;
            }
        }
    }
    left
}
