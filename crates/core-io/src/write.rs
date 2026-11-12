//! Partial-file writes: a temp file beside the destination, then a rename;
//! and the same for a whole folder.

use crate::{IoError, io_err};
use core_media::export::{display_name, partial_path};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A fully written partial file waiting to be moved to its destination.
/// Dropping it uncommitted removes the partial.
#[derive(Debug)]
pub struct PendingFile {
    partial: PathBuf,
    dest: PathBuf,
    replace: bool,
    committed: bool,
}

/// Writes `bytes` to the partial beside `dest`. Without `replace`, an
/// existing `dest` is refused before anything is written. A partial left by
/// a crashed run is refused, never overwritten.
pub fn write_partial(dest: &Path, bytes: &[u8], replace: bool) -> Result<PendingFile, IoError> {
    if !replace && std::fs::symlink_metadata(dest).is_ok() {
        return Err(IoError::Refused(format!(
            "{} exists; choose another name or replace it",
            display_name(dest)
        )));
    }
    let partial = partial_path(dest);
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(IoError::Refused(format!(
                "{} is left from an earlier export; remove it or choose another name",
                display_name(&partial)
            )));
        }
        Err(e) => return Err(io_err(dest, "written", &e)),
    };
    // From here the partial is ours: the guard removes it on any failure.
    let pending = PendingFile {
        partial,
        dest: dest.to_path_buf(),
        replace,
        committed: false,
    };
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| io_err(dest, "written", &e))?;
    Ok(pending)
}

impl PendingFile {
    /// Moves the partial to the destination: over an existing file with
    /// `replace`, else only if the name is still free.
    pub fn commit(mut self) -> Result<PathBuf, IoError> {
        let r = if self.replace {
            std::fs::rename(&self.partial, &self.dest)
        } else {
            core_media::import::rename_excl(&self.partial, &self.dest)
        };
        match r {
            Ok(()) => {
                self.committed = true;
                // The copy fallback of `rename_excl` can succeed with the
                // source still there: never leave a partial that would block
                // the next export of this name.
                let _ = std::fs::remove_file(&self.partial);
                sync_parent(&self.dest);
                Ok(std::mem::take(&mut self.dest))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(IoError::Refused(format!(
                    "{} exists; choose another name or replace it",
                    display_name(&self.dest)
                )))
            }
            Err(e) => Err(io_err(&self.dest, "written", &e)),
        }
    }
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.partial);
        }
    }
}

/// Whether `p` is one plain name: not empty, no separator, not `.` or
/// `..`, not absolute.
fn one_component(p: &Path) -> bool {
    let mut c = p.components();
    matches!(
        (c.next(), c.next()),
        (Some(std::path::Component::Normal(_)), None)
    ) && !p.as_os_str().to_string_lossy().contains('/')
}

fn not_a_name(name: &str) -> IoError {
    IoError::Refused(format!("{name} is not a folder name; choose another name"))
}

/// The most numbered names [`free_folder`] tries.
const MAX_FOLDER_NUMBER: u32 = 9999;

/// Whether nothing is at `p`, by `symlink_metadata` (lstat): a dangling
/// link is something.
fn absent(p: &Path) -> Result<bool, IoError> {
    match std::fs::symlink_metadata(p) {
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(io_err(p, "checked", &e)),
    }
}

/// The first of `name`, `name 2` … `name 9999` in `parent` where neither
/// the path nor its partial (`.<name>.partial`) exists (lstat). A stale
/// partial is passed over, never removed. Refused when all are taken, and
/// when `name` is not one plain name (a separator, `.`, `..` or empty).
pub fn free_folder(parent: &Path, name: &str) -> Result<PathBuf, IoError> {
    if !one_component(Path::new(name)) {
        return Err(not_a_name(name));
    }
    for n in 1..=MAX_FOLDER_NUMBER {
        let p = if n == 1 {
            parent.join(name)
        } else {
            parent.join(format!("{name} {n}"))
        };
        if absent(&p)? && absent(&partial_path(&p))? {
            return Ok(p);
        }
    }
    Err(IoError::Refused(format!(
        "Every name from {name} to {name} {MAX_FOLDER_NUMBER} is taken; choose another folder"
    )))
}

/// A staging folder (`.<name>.partial` beside its destination) being
/// filled. Dropping it uncommitted removes it and everything in it.
#[derive(Debug)]
pub struct PendingFolder {
    partial: PathBuf,
    dest: PathBuf,
    committed: bool,
}

/// Makes the staging folder of `dest`. One left from an earlier export is
/// refused, never reused or removed; so is a `dest` that does not end in
/// a plain name.
pub fn stage_folder(dest: &Path) -> Result<PendingFolder, IoError> {
    if !dest
        .file_name()
        .is_some_and(|n| one_component(Path::new(n)))
    {
        return Err(not_a_name(&dest.display().to_string()));
    }
    let partial = partial_path(dest);
    match std::fs::create_dir(&partial) {
        Ok(()) => Ok(PendingFolder {
            partial,
            dest: dest.to_path_buf(),
            committed: false,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(IoError::Refused(format!(
            "{} is left from an earlier export; remove it or choose another name",
            display_name(&partial)
        ))),
        Err(e) => Err(io_err(dest, "written", &e)),
    }
}

impl PendingFolder {
    /// The staging folder, to write into.
    pub fn path(&self) -> &Path {
        &self.partial
    }

    /// Moves the staging folder to its destination, only if that name is
    /// still free. Refused when it is taken; the staging folder is then
    /// removed.
    pub fn commit(mut self) -> Result<PathBuf, IoError> {
        match core_media::import::rename_excl(&self.partial, &self.dest) {
            Ok(()) => {
                self.committed = true;
                sync_parent(&self.dest);
                Ok(std::mem::take(&mut self.dest))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(IoError::Refused(
                format!("{} exists; choose another name", display_name(&self.dest)),
            )),
            Err(e) => Err(io_err(&self.dest, "written", &e)),
        }
    }
}

impl Drop for PendingFolder {
    fn drop(&mut self) {
        if !self.committed {
            // This job made it: remove it whole (a link in it is removed,
            // never followed).
            let _ = std::fs::remove_dir_all(&self.partial);
        }
    }
}

/// Syncs `p`'s folder, so a rename in it is on the disk (best effort).
fn sync_parent(p: &Path) {
    if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::File::open(dir).and_then(|d| d.sync_all());
    }
}
