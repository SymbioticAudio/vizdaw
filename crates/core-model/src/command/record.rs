//! Recording passes (sub-project 5 spec §4.7, §6.4, §6.5): everything one
//! Record pass adds, applied atomically as `Command::Record` at Stop. The
//! pass is built outside the model (plan 5c Task 7); here it is checked
//! and applied in one step, so it applies whole or not at all.

use super::Report;
use crate::automation::{AutomationTarget, check_value, target_exists};
use crate::document::{Document, MediaItem, MediaOrigin};
use crate::error::{ModelError, Result};
use crate::event::{EventBody, EventSpec, Fade};
use crate::ids::{AnyId, LaneId, MediaId, SeqId, TakeId};
use crate::lane::{Lane, Take};
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

/// One target's automation a pass wrote (spec §7.3, §7.4; plan 5e-1
/// Ruling 4).
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationWrite {
    /// The top-level sequence that played.
    pub seq: SeqId,
    /// What it drives.
    pub target: AutomationTarget,
    /// First tick of the replaced range (included).
    pub start: Tick,
    /// Last tick of the replaced range (included).
    pub end: Tick,
    /// The new points, inside `[start, end]`, by tick.
    pub points: Vec<(Tick, f64)>,
    /// The name of the lane made when no playing lane of `seq` holds the
    /// target's automation.
    pub lane_name: String,
}

/// The most points one [`AutomationWrite`] may hold.
pub const MAX_WRITE_POINTS: usize = 100_000;

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
    /// Automation written while it played (spec §7.4), applied after the
    /// lanes, in order (plan 5e-1 Ruling 4).
    pub automation: Vec<AutomationWrite>,
}

impl RecordPass {
    /// The undo label: "Record", "Record (with gaps)", or "Write
    /// Automation" for a pass that wrote only automation (spec §7.4).
    pub fn name(&self) -> &'static str {
        if self.media.is_empty() && self.lanes.is_empty() && !self.automation.is_empty() {
            "Write Automation"
        } else if self.with_gaps {
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
    for w in &pass.automation {
        write_automation(d, w, r)?;
    }
    Ok(())
}

/// Checks `w` against the document before anything changes.
fn check_write(d: &Document, w: &AutomationWrite) -> Result<()> {
    d.sequence(w.seq)?;
    if w.lane_name.trim().is_empty() {
        return Err(ModelError::invalid("an automation write needs a lane name"));
    }
    check_ticks("automation.start", w.start.0)?;
    check_ticks("automation.end", w.end.0)?;
    if w.start > w.end {
        return Err(ModelError::range("automation.end", w.end.0));
    }
    if w.points.len() > MAX_WRITE_POINTS {
        return Err(ModelError::range("automation.points", w.points.len()));
    }
    for (t, v) in &w.points {
        if !check_value(*v) {
            return Err(ModelError::range("automation.value", v));
        }
        if *t < w.start || *t > w.end {
            return Err(ModelError::range("automation.tick", t.0));
        }
    }
    if !target_exists(d, w.target) {
        return Err(ModelError::invalid(format!(
            "automation target {:?} does not exist",
            w.target
        )));
    }
    Ok(())
}

/// Places one write (plan 5e-1 Ruling 4; final review I1): the target's
/// points in `[start, end]` leave the active take of every lane of `seq`
/// that plays and holds the target's automation, and the new points go
/// into the first such lane, else into a new lane at the end named
/// `lane_name` (none for a write with no points). Of points at one tick
/// the last is kept. A lane plays as `query::flatten` plays it, which is what
/// the writer read: not muted, and soloed when any lane is; a muted or
/// solo-silenced lane keeps its points. A new lane is soloed when another
/// lane is, so that it plays. Automation inside referenced sequences is
/// left: their lanes cannot record (spec Ruling 15).
fn write_automation(d: &mut Document, w: &AutomationWrite, r: &mut Report) -> Result<()> {
    check_write(d, w)?;
    let is_target =
        |b: &EventBody| matches!(b, EventBody::Automation { target, .. } if *target == w.target);
    let mut first: Option<LaneId> = None;
    let mut gone = Vec::new();
    let seq = d.sequence(w.seq)?;
    let any_solo = seq.lanes.iter().any(|l| l.solo);
    for lane in &seq.lanes {
        if lane.mute || (any_solo && !lane.solo) {
            continue;
        }
        let Some(take) = lane.active() else { continue };
        if !take.events.values().any(|e| is_target(&e.body)) {
            continue;
        }
        first.get_or_insert(lane.id);
        gone.extend(
            take.events
                .values()
                .filter(|e| is_target(&e.body) && e.tick >= w.start && e.tick <= w.end)
                .map(|e| e.id),
        );
    }
    for id in gone {
        d.remove_event(id)?;
        r.removed.push(AnyId::Event(id));
    }
    let lane = match first {
        Some(l) => l,
        // Nothing to put in a new lane (Task 3 review minor 2).
        None if w.points.is_empty() => return Ok(()),
        None => {
            let c = d.counters_mut();
            let (lane, take) = (c.alloc_lane(), c.alloc_take());
            let mut new = Lane::new(lane, take, w.lane_name.clone());
            new.solo = any_solo;
            d.seq_mut(w.seq)?.lanes.push_back(new);
            r.created.extend([AnyId::Lane(lane), AnyId::Take(take)]);
            lane
        }
    };
    let mut points = w.points.clone();
    points.sort_by_key(|p| p.0);
    // Of equal ticks the last given wins (the sort is stable), so what
    // plays does not depend on event IDs (Task 3 review minor 3).
    points.reverse();
    points.dedup_by_key(|p| p.0);
    points.reverse();
    for (tick, value) in points {
        let spec = EventSpec::new(
            tick,
            EventBody::Automation {
                target: w.target,
                value,
            },
        );
        check_event(d, w.seq, &spec)?;
        let id = d.counters_mut().alloc_event();
        d.insert_event(w.seq, lane, None, spec.with_id(id))?;
        r.created.push(AnyId::Event(id));
    }
    Ok(())
}
