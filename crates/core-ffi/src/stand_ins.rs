//! The stand-in folders of songs never saved (backlog row "Untitled songs
//! closed unsaved leave a stand-in folder"): `<support>/Co-Producer/Untitled/<id>/`
//! keeps the notes and copies of an untitled song. Settings lists them and
//! clears them. Only real folders named like a project ID are touched, no
//! link is followed, and a song open right now is never listed or deleted.

use crate::error::VizError;
use crate::guard::ffi_guard;
use crate::recovery;
use core_model::document::check_project_id;
use core_model::file;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// One stand-in folder, for Settings.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StandIn {
    /// The folder's name: the song's project ID.
    pub id: String,
    /// The bytes of the regular files in it.
    pub bytes: u64,
    /// When it last changed, in seconds since 1970 (0 if unknown).
    pub modified: u64,
}

fn untitled_root(support: &Path) -> PathBuf {
    support.join("Co-Producer").join("Untitled")
}

/// The folder's bytes: regular files only, no link followed.
fn folder_bytes(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0u64;
    for e in rd.flatten() {
        let Ok(m) = std::fs::symlink_metadata(e.path()) else {
            continue;
        };
        if m.is_dir() {
            total = total.saturating_add(folder_bytes(&e.path()));
        } else if m.is_file() {
            total = total.saturating_add(m.len());
        }
    }
    total
}

/// The real folders under `<support>/Co-Producer/Untitled/` whose names are
/// project IDs, sorted by ID. Nothing when `Co-Producer` or `Untitled` is
/// a link or not a folder.
fn entries(support: &Path) -> Result<Vec<(String, PathBuf)>, VizError> {
    let rel = Path::new("Co-Producer").join("Untitled");
    file::refuse_linked_folders(support, &rel)?;
    let Ok(rd) = std::fs::read_dir(untitled_root(support)) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let Ok(name) = e.file_name().into_string() else {
            continue;
        };
        if !check_project_id(&name) {
            continue;
        }
        let path = e.path();
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => out.push((name, path)),
            _ => {}
        }
    }
    out.sort();
    Ok(out)
}

pub(crate) fn list_in(support: &Path) -> Result<Vec<StandIn>, VizError> {
    Ok(entries(support)?
        .into_iter()
        .map(|(id, path)| {
            let modified = std::fs::symlink_metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            StandIn {
                bytes: folder_bytes(&path),
                id,
                modified,
            }
        })
        .collect())
}

pub(crate) fn delete_in(support: &Path, open_ids: &[String]) -> Result<u32, VizError> {
    let open: HashSet<&str> = open_ids.iter().map(String::as_str).collect();
    let mut gone = 0u32;
    for (id, path) in entries(support)? {
        if open.contains(id.as_str()) {
            continue;
        }
        // Looked at again right before the delete.
        if !matches!(std::fs::symlink_metadata(&path), Ok(m) if m.is_dir()) {
            continue;
        }
        if std::fs::remove_dir_all(&path).is_ok() {
            gone += 1;
        }
    }
    Ok(gone)
}

/// The stand-in folders of songs never saved, for Settings › Co-Producer.
#[uniffi::export]
pub fn list_stand_ins() -> Result<Vec<StandIn>, VizError> {
    ffi_guard(|| list_in(&recovery::support_dir()))
}

/// Deletes the stand-in folders except those of `open_ids` (the songs open
/// right now); how many went.
#[uniffi::export]
pub fn delete_stand_ins(open_ids: Vec<String>) -> Result<u32, VizError> {
    ffi_guard(|| delete_in(&recovery::support_dir(), &open_ids))
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    const B: &str = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    const C: &str = "cccccccc-cccc-cccc-cccc-cccccccccccc";

    fn make(support: &Path, id: &str, bytes: usize) -> PathBuf {
        let d = untitled_root(support).join(id);
        std::fs::create_dir_all(d.join("Copies")).unwrap();
        std::fs::write(d.join("Notes.md"), vec![b'x'; bytes]).unwrap();
        d
    }

    #[test]
    fn lists_only_untitled_stand_ins() {
        let t = tempfile::tempdir().unwrap();
        let s = t.path();
        make(s, B, 10);
        make(s, A, 5);
        // Not under Untitled, and not named like a project ID.
        std::fs::create_dir_all(s.join("Co-Producer").join("Other").join(C)).unwrap();
        std::fs::create_dir_all(untitled_root(s).join("not-an-id")).unwrap();
        let rows = list_in(s).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].id.as_str(), rows[0].bytes), (A, 5));
        assert_eq!((rows[1].id.as_str(), rows[1].bytes), (B, 10));
        assert!(rows[0].modified > 0);
        // Nothing made yet.
        assert!(list_in(&s.join("none")).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn delete_skips_open_songs_and_symlinks() {
        let t = tempfile::tempdir().unwrap();
        let s = t.path();
        let a = make(s, A, 5);
        let b = make(s, B, 5);
        let victim = t.path().join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("keep.txt"), "keep").unwrap();
        std::os::unix::fs::symlink(&victim, untitled_root(s).join(C)).unwrap();
        let n = delete_in(s, &[A.to_string()]).unwrap();
        assert_eq!(n, 1);
        assert!(a.exists(), "an open song's folder stays");
        assert!(!b.exists());
        assert!(victim.join("keep.txt").exists(), "a link is never followed");
        assert!(std::fs::symlink_metadata(untitled_root(s).join(C)).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn hostile_entries_are_skipped() {
        let t = tempfile::tempdir().unwrap();
        let s = t.path();
        let home = t.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("precious"), "x").unwrap();
        let root = untitled_root(s);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(A), "a file").unwrap();
        std::os::unix::fs::symlink(&home, root.join(B)).unwrap();
        let fifo = root.join(C);
        let ok = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        assert!(list_in(s).unwrap().is_empty());
        assert_eq!(delete_in(s, &[]).unwrap(), 0);
        assert!(root.join(A).exists() && home.join("precious").exists());
        assert!(std::fs::symlink_metadata(&fifo).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_untitled_folder_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let s = t.path();
        let elsewhere = t.path().join("elsewhere");
        make(&elsewhere, A, 1);
        std::fs::create_dir_all(s.join("Co-Producer")).unwrap();
        std::os::unix::fs::symlink(untitled_root(&elsewhere), untitled_root(s)).unwrap();
        assert!(list_in(s).is_err());
        assert!(delete_in(s, &[]).is_err());
        assert!(untitled_root(&elsewhere).join(A).exists());
    }
}
