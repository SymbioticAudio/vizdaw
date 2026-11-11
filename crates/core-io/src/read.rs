//! Guarded file reads: regular files only, bounded size.

use crate::{IoError, io_err};
use std::io::Read;
use std::path::Path;

/// Reads the regular file at `path` (a symlink is followed once by the OS;
/// a loop is its error). A directory, FIFO or device is refused before it is
/// opened; a file over `max` bytes is refused, also if it grows while read.
pub fn read_regular(path: &Path, max: u64) -> Result<Vec<u8>, IoError> {
    let name = core_media::export::display_name(path);
    let not_file = || IoError::Refused(format!("{name} is not a regular file"));
    let too_big = || IoError::Refused(format!("{name} is too large to open"));
    let meta = std::fs::metadata(path).map_err(|e| io_err(path, "read", &e))?;
    if !meta.is_file() {
        return Err(not_file());
    }
    if meta.len() > max {
        return Err(too_big());
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
