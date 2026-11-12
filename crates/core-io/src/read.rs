//! Guarded file reads: regular files only, bounded size.

use crate::{IoError, io_err};
use std::io::Read;
use std::path::Path;

/// `path` opened as a regular file (one symlink hop; FIFO, device, directory refused), checked again on the handle.
/// The kind is checked before opening, so a FIFO never blocks the open; a
/// symlink loop is the operating system's error.
pub fn open_regular(path: &Path) -> Result<std::fs::File, IoError> {
    let not_file = || {
        IoError::Refused(format!(
            "{} is not a regular file",
            core_media::export::display_name(path)
        ))
    };
    let meta = std::fs::metadata(path).map_err(|e| io_err(path, "read", &e))?;
    if !meta.is_file() {
        return Err(not_file());
    }
    let file = std::fs::File::open(path).map_err(|e| io_err(path, "opened", &e))?;
    // The path may have been swapped since the check above.
    if !file
        .metadata()
        .map_err(|e| io_err(path, "read", &e))?
        .is_file()
    {
        return Err(not_file());
    }
    Ok(file)
}

/// Reads the regular file at `path` (see [`open_regular`]). A file over
/// `max` bytes is refused, also if it grows while read.
pub fn read_regular(path: &Path, max: u64) -> Result<Vec<u8>, IoError> {
    let too_big = || {
        IoError::Refused(format!(
            "{} is too large to open",
            core_media::export::display_name(path)
        ))
    };
    let file = open_regular(path)?;
    if file.metadata().map_err(|e| io_err(path, "read", &e))?.len() > max {
        return Err(too_big());
    }
    // A limit of u64::MAX cannot be exceeded, so it reads without the probe.
    let probe = max.checked_add(1).unwrap_or(max);
    let mut out = Vec::new();
    file.take(probe)
        .read_to_end(&mut out)
        .map_err(|e| io_err(path, "read", &e))?;
    if out.len() as u64 > max {
        return Err(too_big());
    }
    Ok(out)
}
