//! Relinking (sub-project 5 spec §10.3; plan 5b Rulings 9, 10, 12): the
//! job bodies for "Locate…" (one chosen file) and "Search folder…" (every
//! missing or changed item, by content hash, then name and duration).
//! Run on job threads; nothing here takes the session lock.

use crate::jobs::{RelinkFile, Work};
use core_media::MediaError;
use core_media::caf::{CafReader, hash_file};
use core_media::import::{Imported, MediaImporter, import_reserved};
use core_model::document::MediaItem;
use core_model::format::AudioFormat;
use core_model::ids::MediaId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// Folder levels a search descends.
const DEPTH: u32 = 8;
/// Files a search looks at.
const MAX_FILES: usize = 20_000;
/// Folders a search opens (final review I2): a wide tree of near-empty
/// folders never reaches the file cap.
const MAX_DIRS: usize = 20_000;

/// An item a search looks for.
#[derive(Debug, Clone, PartialEq)]
pub struct Lost {
    /// The item.
    pub media: MediaId,
    /// Its display name.
    pub name: String,
    /// Its file's stem.
    pub stem: String,
    /// Its content hash.
    pub hash: Option<String>,
    /// Its duration in seconds.
    pub seconds: f64,
    /// Its own file (absolute), never a match for it (final review I1).
    pub file: PathBuf,
    /// True when it is offline because its file's content changed: then
    /// only a content-hash match counts, since a file of the same name
    /// and duration is exactly what made it changed (final review I1).
    pub changed: bool,
}

/// `Media/<name>` when `src` is a file directly in `root`'s `Media/` (not
/// hidden).
fn in_media(root: &Path, src: &Path) -> Option<String> {
    let media = std::fs::canonicalize(root.join("Media")).ok()?;
    let f = std::fs::canonicalize(src).ok()?;
    let name = f.file_name()?.to_str()?.to_string();
    (f.parent()? == media && !name.starts_with('.')).then(|| format!("Media/{name}"))
}

/// `Ok` when `path` (symlinks followed) is a regular file: never a FIFO,
/// socket, device or folder (Task 6 review m2).
pub fn regular_file(path: &Path) -> Result<(), MediaError> {
    let m = std::fs::metadata(path)?;
    if m.is_file() {
        Ok(())
    } else {
        Err(MediaError::Unsupported(format!(
            "{} is not a regular file",
            path.display()
        )))
    }
}

/// The error for a file without audio (Task 1 carry: never the model's
/// region-length refusal).
fn no_audio() -> MediaError {
    MediaError::Unsupported("the file holds no audio".into())
}

/// The error for a source of more than two channels.
fn too_many_channels() -> MediaError {
    MediaError::Unsupported(
        "the file has more than two channels; relink each channel's item to a mono file".into(),
    )
}

/// True when no history state names `rel` (compared without case, as
/// macOS volumes resolve names; preflight I2) for other audio than
/// `hash` (preflight M7): a path in the history always names one audio.
fn free_for(named: &BTreeMap<String, Option<String>>, rel: &str, hash: &str) -> bool {
    let rel = rel.to_lowercase();
    named
        .iter()
        .filter(|(p, _)| p.to_lowercase() == rel)
        .all(|(_, h)| h.as_deref() == Some(hash))
}

/// The file in `root`'s `Media/` to relink to `src`: `src` itself when it
/// is a VizDAW CAF in `format` directly in `Media/` whose path no history
/// state names for other audio, else a new file imported (converted) from
/// it under a free name no history state names. `named` is every
/// package-relative path the history references, with its content hash
/// (`History::referenced_media_anywhere`). `unapplied` counts the
/// session's unapplied jobs, this one included: while another is
/// unapplied, the file is always copied, since it may be one that job
/// wrote and still removes when it fails or is cancelled (Task 6 review
/// m1). Anything but a regular file is refused before it is opened (a
/// FIFO would block; review m2), as is a file without audio.
#[allow(clippy::too_many_arguments)]
pub fn relink_file(
    importer: &dyn MediaImporter,
    root: &Path,
    src: &Path,
    format: AudioFormat,
    named: &BTreeMap<String, Option<String>>,
    unapplied: &AtomicUsize,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<RelinkFile, MediaError> {
    regular_file(src)?;
    let alone = unapplied.load(Ordering::SeqCst) <= 1;
    if let Some(rel) = in_media(root, src).filter(|_| alone) {
        if let Ok(r) = CafReader::open(src) {
            let i = r.info().clone();
            if i.sample_rate == format.sample_rate && i.depth == format.depth {
                if i.frames == 0 {
                    return Err(no_audio());
                }
                let hash = hash_file(src)?;
                if free_for(named, &rel, &hash) {
                    let (file, stem) = MediaItem::file_parts(&rel);
                    return Ok(RelinkFile {
                        file: Imported {
                            path: rel,
                            name: stem,
                            original_name: file,
                            channels: i.channels,
                            sample_rate: i.sample_rate,
                            length_frames: i.frames,
                            content_hash: hash,
                            clipped: 0,
                        },
                        made: false,
                    });
                }
            }
        }
    }
    // More than two channels is refused before any conversion (Task 6
    // review m3): the probe only reads the header.
    if importer.probe(src)?.channels > 2 {
        return Err(too_many_channels());
    }
    let reserved: BTreeSet<String> = named.keys().cloned().collect();
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut made = import_reserved(
        importer, src, root, format, &stem, &reserved, progress, cancel,
    )?;
    if made.iter().any(|i| i.length_frames == 0) {
        for i in &made {
            let _ = std::fs::remove_file(root.join(&i.path));
        }
        return Err(no_audio());
    }
    if made.len() != 1 {
        for i in &made {
            let _ = std::fs::remove_file(root.join(&i.path));
        }
        return Err(too_many_channels());
    }
    Ok(RelinkFile {
        file: made.remove(0),
        made: true,
    })
}

/// What a walk may still visit.
struct Budget {
    /// Folders it may still open.
    dirs: usize,
    /// Files it may still collect.
    files: usize,
}

/// Files under `dir`, depth first in name order, at most `DEPTH` levels,
/// `MAX_DIRS` folders and `MAX_FILES` files; hidden entries and symlinks
/// are skipped. A folder's entries are read one by one up to what the
/// budget leaves (never a whole huge folder first), and `cancel` is
/// checked per entry (final review I2): `false` when it was set.
fn walk(
    dir: &Path,
    depth: u32,
    budget: &mut Budget,
    out: &mut Vec<PathBuf>,
    cancel: &AtomicBool,
) -> bool {
    if cancel.load(Ordering::Relaxed) {
        return false;
    }
    if depth == 0 || budget.files == 0 || budget.dirs == 0 {
        return true;
    }
    budget.dirs -= 1;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return true;
    };
    let room = budget.files.saturating_add(budget.dirs);
    let mut entries = Vec::new();
    for e in rd.flatten() {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        if entries.len() >= room {
            break;
        }
        if !e.file_name().to_string_lossy().starts_with('.') {
            entries.push(e);
        }
    }
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        if budget.files == 0 {
            return true;
        }
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            if !walk(&e.path(), depth - 1, budget, out, cancel) {
                return false;
            }
        } else if t.is_file() {
            budget.files -= 1;
            out.push(e.path());
        }
    }
    true
}

/// The `(device, inode)` of each lost item's own file, when it exists.
fn own_files(lost: &[Lost]) -> Vec<Option<(u64, u64)>> {
    lost.iter()
        .map(|l| crate::retention::file_id(&l.file))
        .collect()
}

/// The "Search folder…" job (Ruling 10): matches `lost` items to files in
/// `folder` by content hash, then by stem or name with a duration within
/// 1 ms, and relinks each match; an item without one is a problem. An
/// item's own file never matches it, and an item whose content changed
/// matches only by hash (final review I1). One file may serve every item
/// of its hash (Task 6 review m5).
#[allow(clippy::too_many_arguments)]
pub fn search(
    importer: &dyn MediaImporter,
    root: &Path,
    folder: &Path,
    lost: Vec<Lost>,
    format: AudioFormat,
    named: &BTreeMap<String, Option<String>>,
    unapplied: &AtomicUsize,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Work {
    let mut files = Vec::new();
    let mut budget = Budget {
        dirs: MAX_DIRS,
        files: MAX_FILES,
    };
    if !walk(folder, DEPTH, &mut budget, &mut files, cancel) {
        return Work::Cancelled;
    }
    let own = own_files(&lost);
    let is_own = |k: usize, f: &Path| {
        own.get(k)
            .copied()
            .flatten()
            .is_some_and(|id| crate::retention::file_id(f) == Some(id))
    };
    let mut found: Vec<Option<PathBuf>> = vec![None; lost.len()];
    // By content hash: only VizDAW CAF files have one to compare.
    for f in &files {
        if cancel.load(Ordering::Relaxed) {
            return Work::Cancelled;
        }
        if found.iter().all(Option::is_some) {
            break;
        }
        if !f.extension().is_some_and(|e| e.eq_ignore_ascii_case("caf")) {
            continue;
        }
        let Ok(h) = hash_file(f) else { continue };
        for (k, l) in lost.iter().enumerate() {
            if found.get(k).is_some_and(Option::is_none)
                && l.hash.as_deref() == Some(h.as_str())
                && !is_own(k, f)
            {
                if let Some(slot) = found.get_mut(k) {
                    *slot = Some(f.clone());
                }
            }
        }
    }
    // By name and duration.
    for f in &files {
        if cancel.load(Ordering::Relaxed) {
            return Work::Cancelled;
        }
        if found.iter().all(Option::is_some) {
            break;
        }
        let stem = f
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (k, l) in lost.iter().enumerate() {
            if l.changed
                || !found.get(k).is_some_and(Option::is_none)
                || (stem != l.stem && stem != l.name)
                || is_own(k, f)
            {
                continue;
            }
            let Ok(info) = importer.probe(f) else {
                continue;
            };
            let secs = info.frames as f64 / info.sample_rate.max(1.0);
            if (secs - l.seconds).abs() <= 0.001 {
                if let Some(slot) = found.get_mut(k) {
                    *slot = Some(f.clone());
                }
                break;
            }
        }
    }
    let n = found.iter().flatten().count().max(1) as f32;
    let mut done = 0f32;
    let mut out = Vec::with_capacity(lost.len());
    for (l, f) in lost.iter().zip(found) {
        let Some(f) = f else {
            out.push((
                l.media,
                Err(format!("{}: not found in {}", l.name, folder.display())),
            ));
            continue;
        };
        let base = done;
        let r = relink_file(
            importer,
            root,
            &f,
            format,
            named,
            unapplied,
            &mut |p| progress.store(((base + p) / n).to_bits(), Ordering::Relaxed),
            cancel,
        );
        done += 1.0;
        match r {
            Err(MediaError::Cancelled) => {
                crate::session::discard(root, Work::Relink { files: out }, &BTreeSet::new());
                return Work::Cancelled;
            }
            r => out.push((l.media, r.map_err(|e| format!("{}: {e}", l.name)))),
        }
    }
    Work::Relink { files: out }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` folders under `dir`, each holding `files` empty files.
    fn tree(dir: &Path, n: usize, files: usize) {
        for d in 0..n {
            let sub = dir.join(format!("d{d:05}"));
            std::fs::create_dir_all(&sub).unwrap();
            for f in 0..files {
                std::fs::write(sub.join(format!("f{f}.caf")), "").unwrap();
            }
        }
    }

    /// Final review I2: a set cancel flag stops the walk before it opens a
    /// folder.
    #[test]
    fn a_cancelled_walk_stops_at_once() {
        let dir = tempfile::tempdir().unwrap();
        tree(dir.path(), 50, 1);
        let cancel = AtomicBool::new(true);
        let mut budget = Budget {
            dirs: MAX_DIRS,
            files: MAX_FILES,
        };
        let mut out = Vec::new();
        assert!(!walk(dir.path(), DEPTH, &mut budget, &mut out, &cancel));
        assert!(out.is_empty());
        assert_eq!(budget.dirs, MAX_DIRS, "no folder opened");
    }

    /// Final review I2: folders are capped as well as files, and a
    /// folder's entries are read only up to what the budget leaves.
    #[test]
    fn the_walk_caps_folders_and_files() {
        let dir = tempfile::tempdir().unwrap();
        tree(dir.path(), 10, 2);
        let cancel = AtomicBool::new(false);
        // The root and two of its folders.
        let mut budget = Budget {
            dirs: 3,
            files: 100,
        };
        let mut out = Vec::new();
        assert!(walk(dir.path(), DEPTH, &mut budget, &mut out, &cancel));
        assert_eq!(budget.dirs, 0);
        assert_eq!(out.len(), 4, "{out:?}");
        assert!(out.iter().all(|p| p.starts_with(dir.path().join("d00000"))
            || p.starts_with(dir.path().join("d00001"))));
        // Three files at most, wherever they are.
        let mut budget = Budget {
            dirs: 100,
            files: 3,
        };
        let mut out = Vec::new();
        assert!(walk(dir.path(), DEPTH, &mut budget, &mut out, &cancel));
        assert_eq!(out.len(), 3, "{out:?}");
        // Depth: the root's files only at depth 1.
        std::fs::write(dir.path().join("top.caf"), "").unwrap();
        let mut budget = Budget {
            dirs: 100,
            files: 100,
        };
        let mut out = Vec::new();
        assert!(walk(dir.path(), 1, &mut budget, &mut out, &cancel));
        assert_eq!(out, vec![dir.path().join("top.caf")]);
    }
}
