//! Guarded file reads: regular files only, bounded size.

use crate::IoError;
use std::io::Read;
use std::path::Path;

/// Reads the regular file at `path` (a symlink is followed once by the OS;
/// a loop is its error). A directory, FIFO or device is refused before it is
/// opened; a file over `max` bytes is refused, also if it grows while read.
pub fn read_regular(path: &Path, max: u64) -> Result<Vec<u8>, IoError> {
    let name = core_media::export::display_name(path);
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(IoError::Refused(format!("{name} is not a regular file")));
    }
    let too_big = || IoError::Refused(format!("{name} is too large to open"));
    if meta.len() > max {
        return Err(too_big());
    }
    let mut out = Vec::new();
    std::fs::File::open(path)?
        .take(max.saturating_add(1))
        .read_to_end(&mut out)?;
    if out.len() as u64 > max {
        return Err(too_big());
    }
    Ok(out)
}
