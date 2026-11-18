//! The private file (plan 8f Ruling 5): `<support>/Metrics/time.json`,
//! a 0700 folder of this user's that is not a link, a 0600 file written
//! through a temp file, a damaged file set aside and never replaced, and
//! delete-all that never follows links. Only `std::fs` and
//! `std::os::unix::fs`.

use super::store::Store;
use super::{MAX_STORE_BYTES, METRICS_DIR, STORE};
use crate::error::VizError;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The temp file's name before its pid.
const TEMP_PREFIX: &str = "time.json.tmp-";
/// How many "time.damaged <n>.json" names are tried.
const MAX_DAMAGED: u32 = 1000;

fn io(message: String) -> VizError {
    VizError::Io { message }
}

fn not_ours(what: &str) -> VizError {
    io(format!("The Metrics folder is not VizDAW's ({what})."))
}

/// `support` as a path: absolute, and an existing folder.
pub fn support_dir(support: &str) -> Result<PathBuf, VizError> {
    let p = PathBuf::from(support);
    if support.contains('\0') || !p.is_absolute() {
        return Err(io(
            "The support folder must be an absolute path.".to_string()
        ));
    }
    match fs::metadata(&p) {
        Ok(m) if m.is_dir() => Ok(p),
        Ok(_) => Err(io("The support folder is not a folder.".to_string())),
        Err(e) => Err(io(format!("The support folder cannot be used: {e}."))),
    }
}

/// `<support>/Metrics` after Ruling 5's checks: `None` when it is missing
/// and `create` is false. With `fix`, an existing folder is set to 0700.
pub fn metrics_dir(support: &Path, create: bool, fix: bool) -> Result<Option<PathBuf>, VizError> {
    let dir = support.join(METRICS_DIR);
    match fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == ErrorKind::NotFound => {
            if !create {
                return Ok(None);
            }
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&dir)
                .map_err(|e| io(format!("The Metrics folder could not be made: {e}.")))?;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|e| {
                io(format!(
                    "The Metrics folder could not be made private: {e}."
                ))
            })?;
            Ok(Some(dir))
        }
        Err(e) => Err(io(format!("The Metrics folder cannot be read: {e}."))),
        Ok(m) if m.file_type().is_symlink() => Err(not_ours("a link")),
        Ok(m) if !m.is_dir() => Err(not_ours("not a folder")),
        Ok(m) => {
            let owner = fs::metadata(support)
                .map_err(|e| io(format!("The support folder cannot be used: {e}.")))?
                .uid();
            if m.uid() != owner {
                return Err(not_ours("another user's"));
            }
            if fix && m.permissions().mode() & 0o777 != 0o700 {
                fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|e| {
                    io(format!(
                        "The Metrics folder could not be made private: {e}."
                    ))
                })?;
            }
            Ok(Some(dir))
        }
    }
}

/// Removes temp files a crash left behind.
pub fn remove_strays(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        if e.file_name().to_string_lossy().starts_with(TEMP_PREFIX)
            && e.file_type().is_ok_and(|t| !t.is_dir())
        {
            let _ = fs::remove_file(e.path());
        }
    }
}

/// What reading `time.json` found.
pub enum Found {
    /// No file: an empty store.
    Missing,
    /// A good store.
    Store(Store),
    /// A file that is damaged (Ruling 2's bounds).
    Damaged,
    /// A link or something other than a file: never read or written.
    NotAFile,
}

/// Reads `<dir>/time.json`.
pub fn read(dir: &Path) -> Result<Found, VizError> {
    let path = dir.join(STORE);
    let m = match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Found::Missing),
        Err(e) => return Err(io(format!("The time records cannot be read: {e}."))),
        Ok(m) => m,
    };
    if !m.file_type().is_file() {
        return Ok(Found::NotAFile);
    }
    if m.len() > MAX_STORE_BYTES {
        return Ok(Found::Damaged);
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .and_then(|f| f.take(MAX_STORE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| io(format!("The time records cannot be read: {e}.")))?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Ok(Found::Damaged);
    }
    Ok(match std::str::from_utf8(&bytes).map(Store::parse) {
        Ok(Ok(s)) => Found::Store(s),
        _ => Found::Damaged,
    })
}

/// Renames a damaged `time.json` to the first free "time.damaged.json",
/// "time.damaged 2.json", …; answers the name used.
pub fn set_aside(dir: &Path) -> Result<String, VizError> {
    for n in 1..=MAX_DAMAGED {
        let name = if n == 1 {
            "time.damaged.json".to_string()
        } else {
            format!("time.damaged {n}.json")
        };
        let to = dir.join(&name);
        if fs::symlink_metadata(&to).is_err_and(|e| e.kind() == ErrorKind::NotFound) {
            fs::rename(dir.join(STORE), &to).map_err(|e| {
                io(format!(
                    "The damaged time records could not be set aside: {e}."
                ))
            })?;
            return Ok(name);
        }
    }
    Err(io(
        "The damaged time records could not be set aside: too many are kept.".to_string(),
    ))
}

/// The sentence for a store set aside as `name`.
pub fn set_aside_problem(name: &str) -> String {
    format!(
        "The time records could not be read, so they were set aside as {name} and counting \
         started afresh."
    )
}

/// Writes `store` to `<dir>/time.json` through a 0600 temp file. A
/// `time.json` that is a link or not a file refuses the write.
pub fn write(dir: &Path, store: &Store) -> Result<(), VizError> {
    let path = dir.join(STORE);
    match fs::symlink_metadata(&path) {
        Ok(m) if m.file_type().is_symlink() => {
            return Err(io("The time records file is not VizDAW's (a link).".into()));
        }
        Ok(m) if !m.file_type().is_file() => {
            return Err(io(
                "The time records file is not VizDAW's (not a file).".into()
            ));
        }
        _ => {}
    }
    let bytes = serde_json::to_vec(store)
        .map_err(|e| io(format!("The time records could not be written: {e}.")))?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err(io("The time records are full.".to_string()));
    }
    let temp = dir.join(format!("{TEMP_PREFIX}{}", std::process::id()));
    let _ = fs::remove_file(&temp);
    let result = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .and_then(|mut f| {
            f.write_all(&bytes)?;
            f.sync_all()
        })
        .and_then(|()| fs::rename(&temp, &path));
    result.map_err(|e| {
        let _ = fs::remove_file(&temp);
        io(format!("The time records could not be written: {e}."))
    })
}

/// Removes `<support>/Metrics`: a real folder with everything in it, a
/// link as a link (its target untouched). Missing is `Ok`.
pub fn delete_all(support: &Path) -> Result<(), VizError> {
    let dir = support.join(METRICS_DIR);
    let failed = |e: std::io::Error| io(format!("The time records could not be deleted: {e}."));
    match fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(failed(e)),
        Ok(m) if m.file_type().is_symlink() => fs::remove_file(&dir).map_err(failed),
        Ok(m) if m.is_dir() => fs::remove_dir_all(&dir).map_err(failed),
        Ok(_) => Err(not_ours("not a folder")),
    }
}
