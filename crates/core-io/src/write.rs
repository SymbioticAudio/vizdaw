//! Partial-file writes: a temp file beside the destination, then a rename.

use crate::IoError;
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
        Err(e) => return Err(e.into()),
    };
    // From here the partial is ours: the guard removes it on any failure.
    let pending = PendingFile {
        partial,
        dest: dest.to_path_buf(),
        replace,
        committed: false,
    };
    file.write_all(bytes)?;
    file.sync_all()?;
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
                Ok(std::mem::take(&mut self.dest))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(IoError::Refused(format!(
                    "{} exists; choose another name or replace it",
                    display_name(&self.dest)
                )))
            }
            Err(e) => Err(e.into()),
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
