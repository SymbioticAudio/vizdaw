//! Guarded file reads: regular files only, bounded size.

use crate::{IoError, io_err};
use std::io::Read;
use std::path::Path;

/// `path` opened as a regular file (the whole symlink chain is followed; a
/// loop is the operating system's error). A FIFO, device or directory is
/// refused before opening, and the kind is checked again on the handle.
/// A path swapped for a FIFO between the check and the open can still
/// block the open: `std` offers no non-blocking open.
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

/// True for text that names a URL (`scheme://…`) rather than a file
/// (6d Ruling 19). Files are read only from paths.
pub fn is_url(s: &str) -> bool {
    s.contains("://")
}

/// 6d Ruling 19: the file an external path names, when it may be read:
/// not a URL ([`is_url`]), and a regular file, following one link at
/// most. Never written. The error is the reason, as a sentence fragment.
pub fn external_file(p: &Path) -> Result<std::path::PathBuf, String> {
    if is_url(&p.to_string_lossy()) {
        return Err(URL_NOT_FILE.into());
    }
    let why = |e: std::io::Error| e.to_string();
    let m = std::fs::symlink_metadata(p).map_err(why)?;
    let (real, m) = if m.file_type().is_symlink() {
        let to = std::fs::read_link(p).map_err(why)?;
        let to = match p.parent() {
            Some(dir) if to.is_relative() => dir.join(to),
            _ => to,
        };
        let m = std::fs::symlink_metadata(&to).map_err(why)?;
        (to, m)
    } else {
        (p.to_path_buf(), m)
    };
    if m.is_file() {
        Ok(real)
    } else {
        Err("it is not a regular file".into())
    }
}

/// Why a URL is not read.
pub const URL_NOT_FILE: &str = "it is a URL, not a file";
