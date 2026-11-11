//! Partial-file writes: a temp file beside the destination, then a rename.

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

/// Syncs `p`'s folder, so a rename in it is on the disk (best effort).
fn sync_parent(p: &Path) {
    if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::File::open(dir).and_then(|d| d.sync_all());
    }
}
