//! Audio regions in the schedule (sub-project 5 spec §5.3; plan 5a Ruling
//! 13). Flattened regions are grouped by lane instance (the reference
//! steps of their path plus their lane); within one, the parent overlap
//! rule decides who plays: at any frame the covering region with the
//! latest start (ties: the later in flatten order), so an earlier, longer
//! region resumes after a later one ends. Each piece is a [`Segment`].
//! Fades are evaluated against the region as written, so a cut piece
//! keeps its curve and the cut itself is hard.
//!
//! A crossfade (plan 6c) is a flag on the later region: under the span of
//! its fade in, the earlier region (its partner in the model, by event ID)
//! plays on as an extra segment on the complementary curve.

use super::compile::CompileReport;
use core_media::stream::Need;
use core_model::crossfade::Overlap;
use core_model::document::Document;
use core_model::event::{EventBody, Fade, FadeShape};
use core_model::ids::{EventId, LaneId, MediaId, OutputPathId, SeqId, TakeId};
use core_model::mixer::db_to_gain;
use core_model::query::ResolvedEvent;
use core_model::time::Tick;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

/// One stretch of a region an output path plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// First schedule frame.
    pub start: u64,
    /// Frames (≥ 1).
    pub frames: u64,
    /// The media.
    pub media: MediaId,
    /// Media frame at `start`.
    pub file_frame: u64,
    /// The region's gain, linear.
    pub gain: f32,
    /// Media frame where the region as written starts (its source
    /// offset).
    pub region_offset: u64,
    /// The region's written length.
    pub region_frames: u64,
    /// Fade in.
    pub fade_in: Fade,
    /// Fade out.
    pub fade_out: Fade,
    /// The media is readable (else silence, nothing counted).
    pub online: bool,
    /// The lane of the playing sequence that holds the region, directly
    /// or through references: while that lane records with its input
    /// heard, the segment is silent (plan 5d-1 Ruling 6).
    pub top_lane: LaneId,
    /// While a later region crossfades over this one: the complement of its
    /// fade in, from media frame `xfade_from` (length 0: none; plan 6c).
    pub xfade: Fade,
    /// Media frame where the complement starts.
    pub xfade_from: u64,
}

impl Segment {
    /// One past the last schedule frame.
    pub fn end(&self) -> u64 {
        self.start.saturating_add(self.frames)
    }

    /// Gain × fade in × fade out (× the crossfade's complement) at media
    /// frame `f`. // RT
    pub fn gain_at(&self, f: u64) -> f32 {
        let r = f.saturating_sub(self.region_offset);
        let mut g = self.gain;
        let fi = self.fade_in.length_frames;
        if fi > 0 && r < fi {
            g *= fade_curve(self.fade_in.shape, r as f32 / fi as f32);
        }
        let fo = self.fade_out.length_frames;
        let left = self.region_frames.saturating_sub(r);
        if fo > 0 && left <= fo {
            g *= fade_curve(self.fade_out.shape, left as f32 / fo as f32);
        }
        let xf = self.xfade.length_frames;
        if xf > 0 && f >= self.xfade_from {
            let x = ((f - self.xfade_from) as f32 / xf as f32).min(1.0);
            g *= fade_curve(self.xfade.shape, 1.0 - x);
        }
        g
    }
}

/// A fade-in curve at `x` (0 silent … 1 full, clamped): linear;
/// equal-power `sin`; exponential over a 60 dB range (normalised to reach
/// 0 at 0); S-curve as equal-power of a smoothstep. A fade out uses the
/// same curve on the frames left. // RT
pub fn fade_curve(shape: FadeShape, x: f32) -> f32 {
    use std::f32::consts::FRAC_PI_2;
    let x = if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) };
    match shape {
        FadeShape::Linear => x,
        FadeShape::EqualPower => (x * FRAC_PI_2).sin(),
        FadeShape::Exponential => {
            const FLOOR: f32 = 0.001;
            ((10f32.powf(3.0 * (x - 1.0)) - FLOOR) / (1.0 - FLOOR)).max(0.0)
        }
        FadeShape::SCurve => {
            let s = x * x * (3.0 - 2.0 * x);
            (s * FRAC_PI_2).sin()
        }
    }
}

/// One output path's segments, sorted by start, with the longest length
/// (to find overlaps by binary search).
#[derive(Debug, Clone, PartialEq)]
struct PathSegments {
    path: OutputPathId,
    segments: Box<[Segment]>,
    longest: u64,
}

/// Every output path's segments.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioSchedule {
    paths: Box<[PathSegments]>,
}

impl AudioSchedule {
    /// No segments at all.
    pub fn is_empty(&self) -> bool {
        self.paths.iter().all(|p| p.segments.is_empty())
    }

    /// Segments in every path.
    pub fn segment_count(&self) -> usize {
        self.paths.iter().map(|p| p.segments.len()).sum()
    }

    /// `path`'s segments, sorted by start (empty when it has none). // RT
    pub fn segments(&self, path: OutputPathId) -> &[Segment] {
        match self.paths.binary_search_by_key(&path, |p| p.path) {
            Ok(i) => self.paths.get(i).map_or(&[], |p| &p.segments),
            Err(_) => &[],
        }
    }

    /// Every path with its segments.
    pub fn paths(&self) -> impl Iterator<Item = (OutputPathId, &[Segment])> + '_ {
        self.paths.iter().map(|p| (p.path, &*p.segments))
    }

    /// `path`'s segments that overlap schedule frames `[from, to)`. // RT
    pub fn overlapping(
        &self,
        path: OutputPathId,
        from: u64,
        to: u64,
    ) -> impl Iterator<Item = &Segment> + '_ {
        let (segs, longest) = match self.paths.binary_search_by_key(&path, |p| p.path) {
            Ok(i) => self
                .paths
                .get(i)
                .map_or((&[][..], 0), |p| (&*p.segments, p.longest)),
            Err(_) => (&[][..], 0),
        };
        let first = segs.partition_point(|s| s.start < from.saturating_sub(longest));
        segs.get(first..)
            .unwrap_or(&[])
            .iter()
            .take_while(move |s| s.start < to)
            .filter(move |s| s.end() > from)
    }

    /// What the streamer must have ready: one need per online segment.
    pub fn needs(&self) -> Vec<Need> {
        self.paths
            .iter()
            .flat_map(|p| p.segments.iter())
            .filter(|s| s.online)
            .map(|s| Need {
                media: s.media,
                start: s.start,
                frames: s.frames,
                file_frame: s.file_frame,
            })
            .collect()
    }
}

/// A flattened region on its lane instance, in schedule frames.
struct Use {
    order: usize,
    /// The region event (the last step of its path).
    event: EventId,
    /// The signed frame before the cut at 0.
    raw_start: i64,
    start: u64,
    frames: u64,
    file_frame: u64,
    /// The region crossfades with its partner over `fade_in`.
    crossfade: bool,
    fade_in: Fade,
    seg: Segment,
}

impl Use {
    /// One past the last schedule frame, signed.
    fn raw_end(&self) -> i64 {
        i64::try_from(self.start.saturating_add(self.frames)).unwrap_or(i64::MAX)
    }
}

/// Where a take is.
type TakeKey = (SeqId, LaneId, TakeId);

/// The model's partner of region `event` (plan 6c F9), with each take's
/// overlaps computed once per build.
fn partner_of(
    doc: &Document,
    event: EventId,
    memo: &mut BTreeMap<TakeKey, BTreeMap<EventId, Overlap>>,
) -> Option<EventId> {
    let (_, loc) = doc.event(event).ok()?;
    let key = (loc.seq, loc.lane, loc.take);
    let found = match memo.entry(key) {
        Entry::Occupied(o) => o.into_mut(),
        Entry::Vacant(v) => {
            let take = doc.lane(key.0, key.1).ok()?.take(key.2)?;
            v.insert(core_model::crossfade::overlaps(doc, key.0, take))
        }
    };
    found.get(&event).map(|o| o.with)
}

/// The earlier region's extra segments under the crossfades of `uses`'
/// winning `pieces` (plan 6c): each piece of a crossfading use `u`,
/// within `[u.raw_start, u.raw_start + fade_in)`, plays the partner's use
/// that covers `u`'s start, up to that use's end, on the complement.
fn complements(
    doc: &Document,
    uses: &[Use],
    pieces: &[(usize, u64, u64)],
    memo: &mut BTreeMap<TakeKey, BTreeMap<EventId, Overlap>>,
    out: &mut Vec<Segment>,
) {
    for &(i, a, b) in pieces {
        let Some(u) = uses.get(i) else { continue };
        let len = u.fade_in.length_frames;
        if !u.crossfade || len == 0 {
            continue;
        }
        let span_end = u
            .raw_start
            .saturating_add(i64::try_from(len).unwrap_or(i64::MAX));
        let lo = i64::try_from(a).unwrap_or(i64::MAX).max(u.raw_start);
        let hi = i64::try_from(b).unwrap_or(i64::MAX).min(span_end);
        if lo >= hi {
            continue;
        }
        let Some(partner) = partner_of(doc, u.event, memo) else {
            continue;
        };
        let Some(p) = uses
            .iter()
            .filter(|p| {
                p.event == partner && p.raw_start < u.raw_start && u.raw_start < p.raw_end()
            })
            .max_by_key(|p| (p.raw_start, p.order))
        else {
            continue;
        };
        let lo = lo.max(i64::try_from(p.start).unwrap_or(i64::MAX));
        let hi = hi.min(p.raw_end());
        if lo >= hi {
            continue;
        }
        // lo ≥ p.start ≥ 0 here.
        let (lo, hi) = (lo.unsigned_abs(), hi.unsigned_abs());
        let mut s = p.seg;
        s.start = lo;
        s.frames = hi - lo;
        s.file_frame = p.file_frame + (lo - p.start);
        s.xfade = u.fade_in;
        s.xfade_from = p.seg.region_offset + u.raw_start.abs_diff(p.raw_start);
        out.push(s);
    }
}

/// Builds the audio schedule from the flattened regions of `doc`
/// (`frame_of` maps a tick to a signed schedule frame). Counts into
/// `report`.
pub(crate) fn build(
    doc: &Document,
    regions: &[ResolvedEvent],
    frame_of: &dyn Fn(Tick) -> i64,
    offline: &BTreeSet<MediaId>,
    report: &mut CompileReport,
) -> AudioSchedule {
    type Key = (Vec<EventId>, SeqId, LaneId);
    let mut groups: BTreeMap<Key, (OutputPathId, Vec<Use>)> = BTreeMap::new();
    for (order, r) in regions.iter().enumerate() {
        let EventBody::AudioRegion {
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
            crossfade,
            ..
        } = r.body
        else {
            continue;
        };
        let Some(path) = r.audio_out else {
            report.audio_unrouted += 1;
            continue;
        };
        let Some(last) = r.path.last() else {
            continue;
        };
        // The region as written (flatten clipped `length` at its window).
        let written = match doc.event(last.event) {
            Ok((e, _)) => match e.body {
                EventBody::AudioRegion { length, .. } => length,
                _ => length,
            },
            Err(_) => length,
        };
        report.audio_regions += 1;
        let raw_start = frame_of(r.tick);
        let (start, cut) = if raw_start < 0 {
            (0u64, raw_start.unsigned_abs())
        } else {
            (raw_start.unsigned_abs(), 0)
        };
        if cut >= length {
            continue;
        }
        let refs: Vec<EventId> = r
            .path
            .iter()
            .take(r.path.len().saturating_sub(1))
            .map(|p| p.event)
            .collect();
        let entry = groups
            .entry((refs, last.seq, last.lane))
            .or_insert_with(|| (path, Vec::new()));
        entry.1.push(Use {
            order,
            event: last.event,
            raw_start,
            start,
            frames: length - cut,
            file_frame: source_offset + cut,
            crossfade,
            fade_in,
            seg: Segment {
                start,
                frames: length - cut,
                media,
                file_frame: source_offset + cut,
                gain: db_to_gain(f64::from(gain_db)) as f32,
                region_offset: source_offset,
                region_frames: written,
                fade_in,
                fade_out,
                online: !offline.contains(&media),
                top_lane: r.path.first().map_or(last.lane, |p| p.lane),
                xfade: Fade::default(),
                xfade_from: 0,
            },
        });
    }
    let mut per_path: BTreeMap<OutputPathId, Vec<Segment>> = BTreeMap::new();
    let mut memo = BTreeMap::new();
    for (_, (path, uses)) in groups {
        let out = per_path.entry(path).or_default();
        let mut bounds: Vec<u64> = uses
            .iter()
            .flat_map(|u| [u.start, u.start + u.frames])
            .collect();
        bounds.sort_unstable();
        bounds.dedup();
        // The winners' pieces `(use, start, end)`, merged where one use
        // wins adjacent windows.
        let mut pieces: Vec<(usize, u64, u64)> = Vec::new();
        for w in bounds.windows(2) {
            let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else {
                continue;
            };
            let winner = uses
                .iter()
                .enumerate()
                .filter(|(_, u)| u.start <= a && u.start + u.frames >= b)
                .max_by_key(|(_, u)| (u.start, u.order))
                .map(|(i, _)| i);
            match (winner, pieces.last_mut()) {
                (Some(i), Some((j, _, e))) if i == *j && *e == a => *e = b,
                (Some(i), _) => pieces.push((i, a, b)),
                (None, _) => {}
            }
        }
        for &(i, a, b) in &pieces {
            if let Some(u) = uses.get(i) {
                let mut s = u.seg;
                s.start = a;
                s.frames = b - a;
                s.file_frame = u.file_frame + (a - u.start);
                out.push(s);
            }
        }
        if uses.iter().any(|u| u.crossfade) {
            complements(doc, &uses, &pieces, &mut memo, out);
        }
    }
    let paths: Box<[PathSegments]> = per_path
        .into_iter()
        .map(|(path, mut segments)| {
            segments.sort_by_key(|s| s.start);
            let longest = segments.iter().map(|s| s.frames).max().unwrap_or(0);
            report.audio_segments += segments.len();
            report.audio_offline += segments.iter().filter(|s| !s.online).count();
            PathSegments {
                path,
                segments: segments.into_boxed_slice(),
                longest,
            }
        })
        .collect();
    AudioSchedule { paths }
}
