//! Audio regions in the schedule (sub-project 5 spec §5.3; plan 5a Ruling
//! 13). Flattened regions are grouped by lane instance (the reference
//! steps of their path plus their lane); within one, the parent overlap
//! rule decides who plays: at any frame the covering region with the
//! latest start (ties: the later in flatten order), so an earlier, longer
//! region resumes after a later one ends. Each piece is a [`Segment`].
//! Fades are evaluated against the region as written, so a cut piece
//! keeps its curve and the cut itself is hard.

use super::compile::CompileReport;
use core_media::stream::Need;
use core_model::document::Document;
use core_model::event::{EventBody, Fade, FadeShape};
use core_model::ids::{EventId, LaneId, MediaId, OutputPathId, SeqId};
use core_model::mixer::db_to_gain;
use core_model::query::ResolvedEvent;
use core_model::time::Tick;
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
}

impl Segment {
    /// One past the last schedule frame.
    pub fn end(&self) -> u64 {
        self.start.saturating_add(self.frames)
    }

    /// Gain × fade in × fade out at media frame `f`. // RT
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
    start: u64,
    frames: u64,
    file_frame: u64,
    seg: Segment,
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
        let start = frame_of(r.tick);
        let (start, cut) = if start < 0 {
            (0u64, start.unsigned_abs())
        } else {
            (start as u64, 0)
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
            start,
            frames: length - cut,
            file_frame: source_offset + cut,
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
            },
        });
    }
    let mut per_path: BTreeMap<OutputPathId, Vec<Segment>> = BTreeMap::new();
    for (_, (path, uses)) in groups {
        let out = per_path.entry(path).or_default();
        let mut bounds: Vec<u64> = uses
            .iter()
            .flat_map(|u| [u.start, u.start + u.frames])
            .collect();
        bounds.sort_unstable();
        bounds.dedup();
        let mut last: Option<(usize, u64, u64)> = None;
        let flush = |piece: Option<(usize, u64, u64)>, out: &mut Vec<Segment>| {
            if let Some((i, a, b)) = piece {
                if let Some(u) = uses.get(i) {
                    let mut s = u.seg;
                    s.start = a;
                    s.frames = b - a;
                    s.file_frame = u.file_frame + (a - u.start);
                    out.push(s);
                }
            }
        };
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
            match (winner, last) {
                (Some(i), Some((j, s, e))) if i == j && e == a => last = Some((j, s, b)),
                (Some(i), prev) => {
                    flush(prev, out);
                    last = Some((i, a, b));
                }
                (None, prev) => {
                    flush(prev, out);
                    last = None;
                }
            }
        }
        flush(last, out);
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
