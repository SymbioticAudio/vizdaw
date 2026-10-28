//! The retention rule (sub-project 5 spec §6.5 and Ruling 13; plan 5b
//! Ruling 11): on save and on close, files under `Media/` that no undo or
//! redo state, no saved project and no autosave references go to the
//! Trash. They are moved, never unlinked. The history's references come
//! from `History::referenced_media_anywhere` (final review I3), never from
//! the present document alone.
//!
//! A candidate is kept when its spelling is a kept path, or when it is
//! the same file as a kept path that exists, by `(device, inode)`
//! (preflight I2): macOS volumes resolve names without case and across
//! Unicode normalizations, so `Media/Kick.caf` on disk may be the file a
//! document names `Media/kick.caf`.

use core_model::document::Document;
use core_model::file;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// What a pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Package-relative paths moved to the Trash.
    pub trashed: Vec<String>,
    /// Lines for the user.
    pub problems: Vec<String>,
}

/// The media paths and hashes the package's saved project and autosaves
/// name; `None` when any of them cannot be read or parsed (then nothing
/// may be trashed).
pub fn on_disk(package: &Path) -> Option<(BTreeSet<String>, BTreeSet<String>)> {
    let mut paths = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    let mut take = |d: &Document| {
        for m in d.media().values() {
            paths.insert(m.path.clone());
            if let Some(h) = &m.content_hash {
                hashes.insert(h.clone());
            }
        }
    };
    let pj = package.join(file::PROJECT_FILE);
    if pj.exists() {
        take(&file::from_json(&std::fs::read_to_string(&pj).ok()?).ok()?);
    }
    for a in file::list_autosaves(package).ok()? {
        take(&file::from_json(&std::fs::read_to_string(&a).ok()?).ok()?);
    }
    Some((paths, hashes))
}

/// Package-relative paths of the regular files under `package/Media/`
/// (subfolders too) whose name and folders do not start with `.` and are
/// UTF-8 (a media path is a string), sorted.
pub fn candidates(package: &Path) -> Vec<String> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let Ok(name) = e.file_name().into_string() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let Ok(t) = e.file_type() else { continue };
            let r = format!("{rel}/{name}");
            if t.is_dir() {
                walk(&e.path(), &r, out);
            } else if t.is_file() {
                out.push(r);
            }
        }
    }
    let mut out = Vec::new();
    walk(&package.join("Media"), "Media", &mut out);
    out.sort();
    out
}

/// One pass over `package`: every candidate that `history` (path → hash,
/// the history's references), the saved project and the autosaves leave
/// out goes to the Trash through `trash`; overviews of hashes none of
/// them has and leftovers in `Media/.incoming` are deleted (caches and
/// interrupted imports, not media).
pub fn run(
    history: &BTreeMap<String, Option<String>>,
    package: &Path,
    trash: &dyn Fn(&Path) -> Result<(), String>,
) -> Outcome {
    let mut out = Outcome::default();
    let Some((disk_paths, disk_hashes)) = on_disk(package) else {
        out.problems.push(
            "Unused media files were not moved to the Trash: the saved project or one of its autosaves could not be read."
                .into(),
        );
        return out;
    };
    let keep: BTreeSet<&str> = history
        .keys()
        .map(String::as_str)
        .chain(disk_paths.iter().map(String::as_str))
        .collect();
    let keep_ids: BTreeSet<(u64, u64)> = keep
        .iter()
        .filter_map(|p| file_id(&package.join(p)))
        .collect();
    for rel in candidates(package) {
        if keep.contains(rel.as_str()) {
            continue;
        }
        let path = package.join(&rel);
        if file_id(&path).is_none_or(|id| keep_ids.contains(&id)) {
            // The same file as a kept path under another spelling, or
            // not readable now: it stays.
            continue;
        }
        match trash(&path) {
            Ok(()) => out.trashed.push(rel),
            Err(e) => out
                .problems
                .push(format!("{rel}: not moved to the Trash ({e})")),
        }
    }
    let hashes: BTreeSet<&str> = history
        .values()
        .flatten()
        .map(String::as_str)
        .chain(disk_hashes.iter().map(String::as_str))
        .collect();
    if let Ok(rd) = std::fs::read_dir(package.join(core_media::overview::DIR)) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(h) = name.strip_suffix(".vzov") {
                if !hashes.contains(h) {
                    let _ = std::fs::remove_file(e.path());
                }
            } else if stale_tmp(&name, &e.path()) {
                // A write a crash interrupted: never renamed into place.
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    if let Ok(rd) = std::fs::read_dir(package.join(core_media::import::INCOMING_DIR)) {
        for e in rd.flatten() {
            if e.path().is_file() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    out
}

/// How old an overview's temporary file must be before retention deletes
/// it: a write in progress renames it within moments.
pub const STALE_TMP: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// True when `name` (at `path`) is an overview write's temporary file
/// (`.<name>.<pid>-<n>.tmp`) last modified more than [`STALE_TMP`] ago.
fn stale_tmp(name: &str, path: &Path) -> bool {
    name.starts_with('.')
        && name.ends_with(".tmp")
        && std::fs::symlink_metadata(path)
            .ok()
            .filter(std::fs::Metadata::is_file)
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > STALE_TMP)
}

/// The `(device, inode)` of the file at `path` (following links), `None`
/// when it cannot be read.
#[cfg(unix)]
pub(crate) fn file_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// Without inode numbers a file cannot be matched safely: every
/// candidate counts as unreadable and is kept (nothing is trashed).
#[cfg(not(unix))]
pub(crate) fn file_id(_path: &Path) -> Option<(u64, u64)> {
    None
}

/// The folder that stands for the Trash: a session's (tests), else
/// `$VIZDAW_TRASH_DIR` when absolute (the e2e), else none (the platform
/// Trash).
pub fn trash_dir(session: Option<&Path>) -> Option<PathBuf> {
    session.map(Path::to_path_buf).or_else(|| {
        std::env::var_os("VIZDAW_TRASH_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    })
}

/// Moves `path` into `dir` under its own name (or `n name` when taken).
pub fn move_to(dir: &Path, path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    for n in 1..=10_000u32 {
        let to = if n == 1 {
            dir.join(&name)
        } else {
            dir.join(format!("{n} {name}"))
        };
        if to.exists() {
            continue;
        }
        return match std::fs::rename(path, &to) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => std::fs::copy(path, &to)
                .and_then(|_| std::fs::remove_file(path))
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
    }
    Err("no free name in the trash folder".into())
}

/// Moves `path` to the Trash: into `dir` when given, else the platform's.
pub fn trash(dir: Option<&Path>, path: &Path) -> Result<(), String> {
    match dir {
        Some(d) => move_to(d, path),
        None => platform(path),
    }
}

/// With the test hooks on, the real Trash is never touched.
#[cfg(feature = "test-hooks")]
fn platform(_path: &Path) -> Result<(), String> {
    Err("no trash folder in tests".into())
}

#[cfg(all(not(feature = "test-hooks"), target_os = "macos"))]
fn platform(path: &Path) -> Result<(), String> {
    platform_macos::trash::move_to_trash(path)
}

#[cfg(all(not(feature = "test-hooks"), not(target_os = "macos")))]
fn platform(_path: &Path) -> Result<(), String> {
    Err("this platform has no Trash support yet".into())
}
