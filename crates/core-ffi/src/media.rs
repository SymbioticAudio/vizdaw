//! Media in a session (sub-project 5 spec §4.3, §5.1; plan 5a Rulings 14,
//! 19, 22): where the files live, which are offline and why, the rows the
//! app shows, and copying files into a package on save.

use crate::error::VizError;
use core_media::caf::{CafReader, hash_file};
use core_media::import::{CafImporter, MediaImporter};
use core_media::stream::MediaFile;
use core_model::document::{Document, MediaOrigin};
use core_model::event::EventBody;
use core_model::ids::MediaId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Why a media item cannot play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum OfflineReasonRow {
    /// The file is not there.
    Missing,
    /// The file is not a readable VizDAW CAF.
    Unreadable,
    /// The file is in another rate or depth than the project.
    Format,
    /// The file's length differs from the item's.
    Length,
}

/// Where a media item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MediaOriginRow {
    /// Imported from a file.
    Imported,
    /// Recorded.
    Recorded,
    /// A bounce.
    Bounced,
    /// Returned by the external editor.
    ExternalEdit,
}

/// One media-pool item.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MediaRow {
    /// Media ID.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Package-relative path.
    pub path: String,
    /// Channels.
    pub channels: u16,
    /// Length in frames.
    pub frames: u64,
    /// Sample rate.
    pub sample_rate: u32,
    /// Origin.
    pub origin: MediaOriginRow,
    /// The imported file's name.
    pub original_name: Option<String>,
    /// Why it is offline (`None`: online).
    pub offline: Option<OfflineReasonRow>,
    /// Regions using it (any take).
    pub uses: u32,
}

/// Where an untitled session keeps its media (Ruling 14).
pub fn scratch_root(session: u64) -> PathBuf {
    std::env::temp_dir()
        .join("VizDAW-untitled")
        .join(format!("{}-{session}.vizdaw", std::process::id()))
}

/// The offline items of `doc` under `root`, with why (Ruling 22; the
/// content hash is not re-checked here, Ruling 4).
pub fn check(doc: &Document, root: &Path) -> BTreeMap<MediaId, OfflineReasonRow> {
    let f = doc.settings().format;
    let mut out = BTreeMap::new();
    for m in doc.media().values() {
        let p = root.join(&m.path);
        let reason = if !p.is_file() {
            Some(OfflineReasonRow::Missing)
        } else {
            match CafReader::open(&p) {
                Err(_) => Some(OfflineReasonRow::Unreadable),
                Ok(r) => {
                    let i = r.info();
                    if i.sample_rate != f.sample_rate || i.depth != f.depth {
                        Some(OfflineReasonRow::Format)
                    } else if i.frames != m.length_frames || i.channels != m.channels {
                        Some(OfflineReasonRow::Length)
                    } else {
                        None
                    }
                }
            }
        };
        if let Some(r) = reason {
            out.insert(m.id, r);
        }
    }
    out
}

/// The online items as the engine's media table.
pub fn files(
    doc: &Document,
    root: &Path,
    offline: &BTreeMap<MediaId, OfflineReasonRow>,
) -> Vec<MediaFile> {
    doc.media()
        .values()
        .filter(|m| !offline.contains_key(&m.id))
        .map(|m| MediaFile {
            id: m.id,
            path: root.join(&m.path),
        })
        .collect()
}

/// The media pool as rows.
pub fn rows(doc: &Document, offline: &BTreeMap<MediaId, OfflineReasonRow>) -> Vec<MediaRow> {
    let mut uses: BTreeMap<MediaId, u32> = BTreeMap::new();
    for (_, e) in doc.all_events() {
        if let EventBody::AudioRegion { media, .. } = e.body {
            *uses.entry(media).or_default() += 1;
        }
    }
    doc.media()
        .values()
        .map(|m| {
            let (origin, original_name) = match &m.origin {
                MediaOrigin::Imported { original_name } => (
                    MediaOriginRow::Imported,
                    Some(original_name.clone()).filter(|n| !n.is_empty()),
                ),
                MediaOrigin::Recorded { .. } => (MediaOriginRow::Recorded, None),
                MediaOrigin::Bounced => (MediaOriginRow::Bounced, None),
                MediaOrigin::ExternalEdit { .. } => (MediaOriginRow::ExternalEdit, None),
            };
            MediaRow {
                id: m.id.0,
                name: m.name.clone(),
                path: m.path.clone(),
                channels: m.channels,
                frames: m.length_frames,
                sample_rate: m.sample_rate,
                origin,
                original_name,
                offline: offline.get(&m.id).copied(),
                uses: uses.get(&m.id).copied().unwrap_or(0),
            }
        })
        .collect()
}

/// Copies every media file of `doc` from `from` into `to` (Ruling 14)
/// unless `to` already holds the same audio. A same-named file with other
/// audio (Save As replacing an older package, review I1) is replaced
/// atomically: copied to a temp file beside it, then renamed over it.
/// Files of `to` the document does not reference are left alone. Missing
/// sources are skipped (they stay offline).
pub fn copy_media(doc: &Document, from: &Path, to: &Path) -> Result<(), VizError> {
    if from == to {
        return Ok(());
    }
    for m in doc.media().values() {
        let (src, dst) = (from.join(&m.path), to.join(&m.path));
        if !src.is_file() {
            continue;
        }
        if dst.exists() && same_audio(&src, &dst, m.content_hash.as_deref()) {
            continue;
        }
        let Some(dir) = dst.parent() else {
            return Err(VizError::Io {
                message: format!("no folder for {}", dst.display()),
            });
        };
        std::fs::create_dir_all(dir).map_err(io)?;
        let name = dst
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tmp = dir.join(format!(".{name}.{}.vizdaw-tmp", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        // A hard link when both are on one volume; else a copy.
        let placed = std::fs::hard_link(&src, &tmp)
            .or_else(|_| std::fs::copy(&src, &tmp).map(|_| ()))
            .and_then(|()| std::fs::rename(&tmp, &dst));
        if let Err(e) = placed {
            let _ = std::fs::remove_file(&tmp);
            return Err(io(e));
        }
    }
    Ok(())
}

/// True when `dst` holds the audio of `src`: by the item's content hash
/// when known, else by hashing both. An unreadable file never matches.
fn same_audio(src: &Path, dst: &Path, hash: Option<&str>) -> bool {
    let Ok(have) = hash_file(dst) else {
        return false;
    };
    match hash {
        Some(h) => have == h,
        None => hash_file(src).is_ok_and(|want| want == have),
    }
}

fn io(e: std::io::Error) -> VizError {
    VizError::Io {
        message: e.to_string(),
    }
}

/// The platform importer: ExtAudioFile on macOS, CAF only elsewhere.
pub fn importer() -> Box<dyn MediaImporter> {
    #[cfg(target_os = "macos")]
    {
        Box::new(platform_macos::media::ExtAudioImporter)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(CafImporter)
    }
}

/// The CAF importer, for depth-only conversions on any platform.
pub fn caf_importer() -> Box<dyn MediaImporter> {
    Box::new(CafImporter)
}
