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
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Folder levels a search descends.
const DEPTH: u32 = 8;
/// Files a search looks at.
const MAX_FILES: usize = 20_000;

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
}

/// `Media/<name>` when `src` is a file directly in `root`'s `Media/` (not
/// hidden).
fn in_media(root: &Path, src: &Path) -> Option<String> {
    let media = std::fs::canonicalize(root.join("Media")).ok()?;
    let f = std::fs::canonicalize(src).ok()?;
    let name = f.file_name()?.to_str()?.to_string();
    (f.parent()? == media && !name.starts_with('.')).then(|| format!("Media/{name}"))
}

/// The error for a file without audio (Task 1 carry: never the model's
/// region-length refusal).
fn no_audio() -> MediaError {
    MediaError::Unsupported("the file holds no audio".into())
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
/// (`History::referenced_media_anywhere`). A file without audio is
/// refused.
pub fn relink_file(
    importer: &dyn MediaImporter,
    root: &Path,
    src: &Path,
    format: AudioFormat,
    named: &BTreeMap<String, Option<String>>,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<RelinkFile, MediaError> {
    if let Some(rel) = in_media(root, src) {
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
        return Err(MediaError::Unsupported(
            "the file has more than two channels; relink each channel's item to a mono file".into(),
        ));
    }
    Ok(RelinkFile {
        file: made.remove(0),
        made: true,
    })
}

/// Files under `dir`, depth first in name order, at most `DEPTH` levels
/// and `MAX_FILES` files; hidden entries and symlinks are skipped.
fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth == 0 || out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<std::fs::DirEntry> = rd.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        if out.len() >= MAX_FILES {
            return;
        }
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            walk(&e.path(), depth - 1, out);
        } else if t.is_file() {
            out.push(e.path());
        }
    }
}

/// The "Search folder…" job (Ruling 10): matches `lost` items to files in
/// `folder` by content hash, then by stem or name with a duration within
/// 1 ms, and relinks each match; an item without one is a problem.
#[allow(clippy::too_many_arguments)]
pub fn search(
    importer: &dyn MediaImporter,
    root: &Path,
    folder: &Path,
    lost: Vec<Lost>,
    format: AudioFormat,
    named: &BTreeMap<String, Option<String>>,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Work {
    let mut files = Vec::new();
    walk(folder, DEPTH, &mut files);
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
            if found.get(k).is_some_and(Option::is_none) && l.hash.as_deref() == Some(h.as_str()) {
                if let Some(slot) = found.get_mut(k) {
                    *slot = Some(f.clone());
                }
                break;
            }
        }
    }
    // By name and duration.
    for f in &files {
        if found.iter().all(Option::is_some) {
            break;
        }
        let stem = f
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (k, l) in lost.iter().enumerate() {
            if !found.get(k).is_some_and(Option::is_none) || (stem != l.stem && stem != l.name) {
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
            &mut |p| progress.store(((base + p) / n).to_bits(), Ordering::Relaxed),
            cancel,
        );
        done += 1.0;
        match r {
            Err(MediaError::Cancelled) => {
                crate::session::discard(root, Work::Relink { files: out });
                return Work::Cancelled;
            }
            r => out.push((l.media, r.map_err(|e| format!("{}: {e}", l.name)))),
        }
    }
    Work::Relink { files: out }
}
