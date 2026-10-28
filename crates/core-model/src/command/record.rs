//! Recording passes (sub-project 5 spec §4.7, §6.4, §6.5): everything one
//! Record pass adds, applied atomically as `Command::Record` at Stop. The
//! pass is built outside the model (plan 5c Task 7); here it is checked
//! and applied in one step, so it applies whole or not at all.

use super::Report;
use crate::document::{Document, MediaItem, MediaOrigin};
use crate::error::{ModelError, Result};
use crate::event::{EventBody, EventSpec, Fade};
use crate::ids::{AnyId, LaneId, MediaId, SeqId, TakeId};
use crate::lane::Take;
use crate::rules::{check_event, check_media_path, check_ticks};
use crate::time::Tick;

/// A file the pass recorded, added to the media pool (origin
/// `Recorded { pass }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedMedia {
    /// Package-relative path (`Media/Recorded/…`).
    pub path: String,
    /// Sample rate (> 0).
    pub sample_rate: u32,
    /// Channels (1 or 2).
    pub channels: u16,
    /// Length in frames (> 0).
    pub length_frames: u64,
    /// BLAKE3 hex of the sample bytes.
    pub content_hash: String,
    /// Display name.
    pub name: String,
    /// The pass number.
    pub pass: u32,
}

/// A region over a slice of one of the pass's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedRegion {
    /// Index into [`RecordPass::media`].
    pub media: usize,
    /// Start tick.
    pub tick: Tick,
    /// First frame of the file it plays.
    pub source_offset: u64,
    /// Frames (≥ 1, within the file).
    pub length: u64,
}

/// What a pass puts into one take.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordedContent {
    /// MIDI events.
    pub events: Vec<EventSpec>,
    /// Audio regions.
    pub regions: Vec<RecordedRegion>,
}

impl RecordedContent {
    /// Nothing recorded.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.regions.is_empty()
    }
}

/// One lane's part of a pass.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneRecord {
    /// Sequence.
    pub seq: SeqId,
    /// Lane.
    pub lane: LaneId,
    /// Replace mode: the active take's MIDI events (not regions,
    /// references, text or automation) that start in one of these
    /// `[start, end)` ranges are removed first (spec §6.4).
    pub clear: Vec<(Tick, Tick)>,
    /// Into the active take.
    pub active: RecordedContent,
    /// New takes, in order, each with its name; the last becomes active.
    pub takes: Vec<(String, RecordedContent)>,
}

/// A finished recording pass (spec §4.7).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordPass {
    /// The input ring overflowed or blocks were lost: the lost frames are
    /// silence, and the undo label says so (spec §6.2).
    pub with_gaps: bool,
    /// The files recorded.
    pub media: Vec<RecordedMedia>,
    /// Each lane's part.
    pub lanes: Vec<LaneRecord>,
}

impl RecordPass {
    /// The undo label: "Record", or "Record (with gaps)".
    pub fn name(&self) -> &'static str {
        if self.with_gaps {
            "Record (with gaps)"
        } else {
            "Record"
        }
    }
}

/// The most takes a pass may add to one lane.
pub const MAX_PASS_TAKES: usize = 256;

fn check_media(m: &RecordedMedia) -> Result<()> {
    check_media_path(&m.path)?;
    if m.sample_rate == 0 || !(1..=2).contains(&m.channels) || m.length_frames == 0 {
        return Err(ModelError::range("record.media", &m.path));
    }
    if m.content_hash.is_empty() || m.name.is_empty() {
        return Err(ModelError::invalid(format!(
            "recorded file {} has no hash or name",
            m.path
        )));
    }
    Ok(())
}

/// The pass's files with the media IDs the pool gave them.
struct Files<'a> {
    pass: &'a RecordPass,
    ids: &'a [MediaId],
}

/// The region event of `r`, checked against the file it slices.
fn region_spec(r: &RecordedRegion, files: &Files<'_>) -> Result<EventSpec> {
    let (Some(m), Some(id)) = (files.pass.media.get(r.media), files.ids.get(r.media)) else {
        return Err(ModelError::range("record.region.media", r.media));
    };
    let end = r.source_offset.checked_add(r.length);
    if r.length == 0 || end.is_none_or(|e| e > m.length_frames) {
        return Err(ModelError::range("record.region.length", r.length));
    }
    Ok(EventSpec::new(
        r.tick,
        EventBody::AudioRegion {
            media: *id,
            source_offset: r.source_offset,
            length: r.length,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        },
    ))
}

/// Inserts `content` into take `take` (the active one when `None`).
fn insert_content(
    d: &mut Document,
    seq: SeqId,
    lane: LaneId,
    take: Option<TakeId>,
    content: &RecordedContent,
    files: &Files<'_>,
    r: &mut Report,
) -> Result<()> {
    let mut specs: Vec<EventSpec> = content.events.clone();
    for reg in &content.regions {
        specs.push(region_spec(reg, files)?);
    }
    if !content.regions.is_empty() && d.lane(seq, lane)?.audio_out.is_none() {
        // A region plays on its lane's output path: a lane without one
        // takes the first, or a new "Audio 1" (5a Ruling 6).
        let out = match d.outputs().keys().next().copied() {
            Some(o) => o,
            None => super::routing::add_output(d, "Audio 1", "audio:1-2", r),
        };
        d.lane_mut(seq, lane)?.audio_out = Some(out);
    }
    for spec in specs {
        check_event(d, seq, &spec)?;
        let id = d.counters_mut().alloc_event();
        d.insert_event(seq, lane, take, spec.with_id(id))?;
        r.created.push(AnyId::Event(id));
        super::event::ensure_drum_row(d, id, r)?;
    }
    Ok(())
}

pub(crate) fn apply(d: &mut Document, pass: &RecordPass, r: &mut Report) -> Result<()> {
    let mut ids = Vec::with_capacity(pass.media.len());
    for m in &pass.media {
        check_media(m)?;
        // Media paths are write-once: a path names one audio.
        if d.media().values().any(|i| i.path == m.path) {
            return Err(ModelError::invalid(format!(
                "media path {} is already in the pool",
                m.path
            )));
        }
        let id = d.counters_mut().alloc_media();
        d.media_mut().insert(
            id,
            MediaItem {
                id,
                path: m.path.clone(),
                sample_rate: m.sample_rate,
                channels: m.channels,
                length_frames: m.length_frames,
                content_hash: Some(m.content_hash.clone()),
                name: m.name.clone(),
                origin: MediaOrigin::Recorded { pass: m.pass },
            },
        );
        r.created.push(AnyId::Media(id));
        ids.push(id);
    }
    let files = Files { pass, ids: &ids };
    for l in &pass.lanes {
        d.lane(l.seq, l.lane)?;
        if l.takes.len() > MAX_PASS_TAKES {
            return Err(ModelError::range("record.takes", l.takes.len()));
        }
        for (a, b) in &l.clear {
            check_ticks("record.clear", a.0)?;
            check_ticks("record.clear", b.0)?;
            if a >= b {
                return Err(ModelError::range("record.clear", b.0));
            }
        }
        if !l.clear.is_empty() {
            let gone: Vec<_> = d
                .lane(l.seq, l.lane)?
                .active()
                .map(|t| {
                    t.events
                        .values()
                        .filter(|e| {
                            e.body.is_midi()
                                && l.clear.iter().any(|(a, b)| e.tick >= *a && e.tick < *b)
                        })
                        .map(|e| e.id)
                        .collect()
                })
                .unwrap_or_default();
            for id in gone {
                d.remove_event(id)?;
                r.removed.push(AnyId::Event(id));
            }
        }
        insert_content(d, l.seq, l.lane, None, &l.active, &files, r)?;
        for (name, content) in &l.takes {
            let take = d.counters_mut().alloc_take();
            let lane = d.lane_mut(l.seq, l.lane)?;
            lane.takes.push_back(Take::new(take, name.clone()));
            lane.active_take = take;
            r.created.push(AnyId::Take(take));
            insert_content(d, l.seq, l.lane, Some(take), content, &files, r)?;
        }
    }
    Ok(())
}
