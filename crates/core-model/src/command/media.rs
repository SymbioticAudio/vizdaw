//! Media-pool commands (sub-project 5 spec §4.3; plan 5a Ruling 5).
//! Removing an item in use is refused unless a replacement is given;
//! converting gives items new files and rescales their regions.

use super::Report;
use super::audio::clamp_fades;
use super::routing::require_replacement;
use crate::document::{Document, MediaItem, MediaOrigin};
use crate::error::{ModelError, Result};
use crate::event::EventBody;
use crate::ids::{AnyId, EventId, MediaId};
use crate::rules::{check_event_fields, check_media_path};
use std::collections::BTreeSet;

/// A media item's new file (a conversion): the same item, so its regions
/// keep it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSwap {
    /// The item.
    pub media: MediaId,
    /// The new file, package-relative.
    pub path: String,
    /// The new file's length in frames (> 0).
    pub length_frames: u64,
    /// The new file's content hash.
    pub content_hash: Option<String>,
}

/// Media-pool edits.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaCmd {
    /// Add a media item.
    Add {
        /// Package-relative path.
        path: String,
        /// Sample rate (> 0).
        sample_rate: u32,
        /// Channels (> 0).
        channels: u16,
        /// Length in frames.
        length_frames: u64,
        /// Content hash.
        content_hash: Option<String>,
        /// Display name; `None`: the file stem.
        name: Option<String>,
        /// Origin; `None`: imported from a file of this name.
        origin: Option<MediaOrigin>,
    },
    /// Remove a media item, re-pointing regions to `replacement`.
    Remove {
        /// Target.
        media: MediaId,
        /// Replacement (required when in use; must be long enough).
        replacement: Option<MediaId>,
    },
    /// Rename a media item (display name only).
    Rename {
        /// Target.
        media: MediaId,
        /// New name.
        name: String,
    },
    /// Point a media item at another file; refused while a region uses
    /// frames beyond the new file's end.
    Relink {
        /// Target.
        media: MediaId,
        /// New package-relative path.
        path: String,
        /// The new file's content hash.
        content_hash: Option<String>,
        /// The new file's length in frames.
        length_frames: u64,
        /// The new file's channels (> 0).
        channels: u16,
        /// The new file's sample rate (> 0).
        sample_rate: u32,
    },
    /// Remove every item no region uses.
    Compact,
    /// New files for some items, at the project's rate (the banner's
    /// "Convert" for media in another format).
    Convert {
        /// The conversions (at least one).
        swaps: Vec<MediaSwap>,
    },
}

impl MediaCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            MediaCmd::Add { .. } => "Add Media",
            MediaCmd::Remove { .. } => "Remove Media",
            MediaCmd::Rename { .. } => "Rename Media",
            MediaCmd::Relink { .. } => "Relink Media",
            MediaCmd::Compact => "Compact Media",
            MediaCmd::Convert { .. } => "Convert Media",
        }
    }
}

/// The regions (in any take) that use `media`.
pub fn regions_using(d: &Document, media: MediaId) -> Vec<EventId> {
    d.all_events()
        .filter(|(_, e)| matches!(e.body, EventBody::AudioRegion { media: m, .. } if m == media))
        .map(|(_, e)| e.id)
        .collect()
}

/// Every media item some region uses.
pub fn used_media(d: &Document) -> BTreeSet<MediaId> {
    d.all_events()
        .filter_map(|(_, e)| match e.body {
            EventBody::AudioRegion { media, .. } => Some(media),
            _ => None,
        })
        .collect()
}

fn item(d: &Document, media: MediaId) -> Result<&MediaItem> {
    d.media()
        .get(&media)
        .ok_or(ModelError::UnknownId(AnyId::Media(media)))
}

/// Gives each swapped item its new file at `rate` and rescales its
/// regions by `rate / old rate` (offset, length and fades rounded to the
/// nearest frame; the length kept ≥ 1 and the region clamped into the new
/// file; the fades clamped as in Ruling 1).
pub(crate) fn swap_media(d: &mut Document, swaps: &[MediaSwap], rate: u32) -> Result<()> {
    let mut seen = BTreeSet::new();
    for s in swaps {
        if !seen.insert(s.media) {
            return Err(ModelError::invalid(format!(
                "{} is converted twice",
                s.media
            )));
        }
        check_media_path(&s.path)?;
        if s.length_frames == 0 {
            return Err(ModelError::range("media.length_frames", 0));
        }
        let old_rate = item(d, s.media)?.sample_rate.max(1);
        let ratio = f64::from(rate) / f64::from(old_rate);
        if let Some(m) = d.media_mut().get_mut(&s.media) {
            m.path = s.path.clone();
            m.length_frames = s.length_frames;
            m.content_hash = s.content_hash.clone();
            m.sample_rate = rate;
        }
        // Float-to-int `as` saturates; ratio 1 maps every value to itself.
        let scale = |x: u64| (x as f64 * ratio).round() as u64;
        for id in regions_using(d, s.media) {
            let mut ev = d.event(id)?.0.clone();
            if let EventBody::AudioRegion {
                source_offset,
                length,
                fade_in,
                fade_out,
                ..
            } = &mut ev.body
            {
                *source_offset = scale(*source_offset).min(s.length_frames - 1);
                *length = scale(*length).max(1).min(s.length_frames - *source_offset);
                fade_in.length_frames = scale(fade_in.length_frames);
                fade_out.length_frames = scale(fade_out.length_frames);
            }
            clamp_fades(&mut ev);
            check_event_fields(d, &ev.spec())?;
            d.replace_event(ev)?;
        }
    }
    Ok(())
}

pub(crate) fn apply(d: &mut Document, cmd: &MediaCmd, r: &mut Report) -> Result<()> {
    match cmd {
        MediaCmd::Add {
            path,
            sample_rate,
            channels,
            length_frames,
            content_hash,
            name,
            origin,
        } => {
            if *sample_rate == 0 || *channels == 0 {
                return Err(ModelError::range("media.format", sample_rate));
            }
            check_media_path(path)?;
            let id = d.counters_mut().alloc_media();
            let (file, stem) = MediaItem::file_parts(path);
            d.media_mut().insert(
                id,
                MediaItem {
                    id,
                    path: path.clone(),
                    sample_rate: *sample_rate,
                    channels: *channels,
                    length_frames: *length_frames,
                    content_hash: content_hash.clone(),
                    name: name.clone().unwrap_or(stem),
                    origin: origin.clone().unwrap_or(MediaOrigin::Imported {
                        original_name: file,
                    }),
                },
            );
            r.created.push(AnyId::Media(id));
        }
        MediaCmd::Remove { media, replacement } => {
            item(d, *media)?;
            let regions = regions_using(d, *media);
            let users = regions.iter().map(|e| AnyId::Event(*e)).collect();
            let exists = replacement.is_some_and(|x| d.media().contains_key(&x));
            let rep = require_replacement(*media, *replacement, exists, users)?;
            if let Some(rep) = rep {
                for id in regions {
                    let mut ev = d.event(id)?.0.clone();
                    if let EventBody::AudioRegion { media: m, .. } = &mut ev.body {
                        *m = rep;
                    }
                    check_event_fields(d, &ev.spec())?;
                    d.replace_event(ev)?;
                }
            }
            d.media_mut().remove(media);
            r.removed.push(AnyId::Media(*media));
        }
        MediaCmd::Rename { media, name } => {
            item(d, *media)?;
            if let Some(m) = d.media_mut().get_mut(media) {
                m.name = name.clone();
            }
        }
        MediaCmd::Relink {
            media,
            path,
            content_hash,
            length_frames,
            channels,
            sample_rate,
        } => {
            item(d, *media)?;
            if *sample_rate == 0 || *channels == 0 {
                return Err(ModelError::range("media.format", sample_rate));
            }
            check_media_path(path)?;
            for id in regions_using(d, *media) {
                if let EventBody::AudioRegion {
                    source_offset,
                    length,
                    ..
                } = d.event(id)?.0.body
                {
                    let end = source_offset.saturating_add(length);
                    if end > *length_frames {
                        return Err(ModelError::range("relink.length_frames", length_frames));
                    }
                }
            }
            if let Some(m) = d.media_mut().get_mut(media) {
                m.path = path.clone();
                m.content_hash = content_hash.clone();
                m.length_frames = *length_frames;
                m.channels = *channels;
                m.sample_rate = *sample_rate;
            }
        }
        MediaCmd::Compact => {
            let used = used_media(d);
            let unused: Vec<MediaId> = d
                .media()
                .keys()
                .filter(|m| !used.contains(m))
                .copied()
                .collect();
            for m in unused {
                d.media_mut().remove(&m);
                r.removed.push(AnyId::Media(m));
            }
        }
        MediaCmd::Convert { swaps } => {
            if swaps.is_empty() {
                return Err(ModelError::invalid("nothing to convert"));
            }
            let rate = d.settings().format.sample_rate;
            swap_media(d, swaps, rate)?;
        }
    }
    Ok(())
}
