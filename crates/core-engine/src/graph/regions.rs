//! The Regions node's run context and render (sub-project 5 spec §5.4,
//! §6.6; plan 5a Ruling 11, plan 5d-1 Rulings 3–4). // RT
//!
//! A `Regions` node with lead `a` (`Plan.leads`) renders, while the
//! renderer renders frame `f`, the regions at frame `f − a`: ahead of what
//! is heard by its path's own latency, so no compensation delay follows
//! it and a monitored input mixed into it is heard after its own path's
//! plugins only. [`RegionRun::map`] turns a read frame into the frame read:
//! nothing before the roll's start, the loop's end before the loop's start
//! once the loop wrapped, nothing past the end of playing.

use crate::backend::StereoBuf;
use crate::input::MonitorRoute;
use crate::schedule::AudioSchedule;
use core_media::stream::{Read, StreamReader};
use core_model::ids::OutputPathId;

/// The device input `Regions` nodes monitor (spec §6.6): the renderer's
/// copy of this block's input channels, so a run that outlives the
/// device's call still reads valid memory.
#[derive(Debug)]
pub struct InputCopy {
    chans: Box<[Box<[f32]>]>,
}

impl InputCopy {
    /// `channels` channels of `frames` frames, zeroed. Allocates.
    pub fn new(channels: usize, frames: u32) -> InputCopy {
        InputCopy {
            chans: (0..channels)
                .map(|_| vec![0.0; frames as usize].into_boxed_slice())
                .collect(),
        }
    }

    /// Copies the first `frames` frames of the channels `want` names from
    /// `input` (missing channels read 0). // RT
    pub fn fill(&mut self, input: &[&[f32]], frames: u32, want: impl Fn(usize) -> bool) {
        let n = frames as usize;
        for (c, dst) in self.chans.iter_mut().enumerate() {
            if !want(c) {
                continue;
            }
            let Some(dst) = dst.get_mut(..n.min(dst.len())) else {
                continue;
            };
            match input.get(c) {
                Some(src) => {
                    for (k, d) in dst.iter_mut().enumerate() {
                        *d = src.get(k).copied().unwrap_or(0.0);
                    }
                }
                None => dst.fill(0.0),
            }
        }
    }

    /// Channel `c`'s frames `[at, at + n)`. // RT
    fn get(&self, c: u16, at: usize, n: usize) -> &[f32] {
        self.chans
            .get(usize::from(c))
            .and_then(|ch| ch.get(at..at + n))
            .unwrap_or(&[])
    }
}

/// What `Regions` nodes monitor during a run.
#[derive(Debug, Clone, Copy)]
struct Monitor {
    input: *const InputCopy,
    routes: *const [MonitorRoute],
    /// The run's first frame within the copied block.
    offset: u32,
    /// The transport plays.
    playing: bool,
    /// The record range while a recording rolls (`None`: not recording).
    rec: Option<(u64, u64)>,
}

/// What `Regions` nodes read during one run: the playing schedule's audio,
/// the stream reader, where the run is, and the monitored input. Built by
/// the renderer (or a test `Runner`) before each run.
#[derive(Debug, Clone, Copy)]
pub struct RegionRun {
    audio: *const AudioSchedule,
    reader: *const StreamReader,
    monitor: Option<Monitor>,
    /// Schedule frame the renderer renders at the run's first frame (a
    /// node with lead `a` reads `frame − a`).
    pub frame: u64,
    /// The transport plays through the whole run (or plays out the end).
    pub playing: bool,
    /// Misses count as late starts (spec §5.5, Ruling 10).
    pub late: bool,
    /// The device runs at another rate than the project (Ruling 20).
    pub muted: bool,
    /// Reads before this frame are silent (where the transport rolled
    /// from), unless the loop wrapped.
    pub floor: u64,
    /// Reads at or after this frame are silent (where playing ended;
    /// `u64::MAX` while it plays).
    pub ceiling: u64,
    /// The loop `[start, end)` once the transport wrapped it since it
    /// rolled: a read before its start comes from its end.
    pub wrapped: Option<Wrapped>,
    /// Frames played since the transport rolled, at the run's first
    /// frame: a node with lead `a` reads silence until `a` frames have
    /// played, whatever the loop did (plan 5d-1 review I1).
    pub played: u64,
}

/// A loop `[start, end)` the transport wrapped since it rolled, with the
/// stretches the heard timeline ran before the wrap (review N2): going
/// back from the wrap the transport played `last` frames up to `end`,
/// before that `wraps - 2` full passes of the loop, before that `first`
/// frames up to `end` (the stretch from the roll to the first wrap).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wrapped {
    /// Loop start.
    pub start: u64,
    /// Loop end.
    pub end: u64,
    /// Frames played just before the last wrap.
    pub last: u64,
    /// Frames played before the first wrap.
    pub first: u64,
    /// Wraps since the roll.
    pub wraps: u64,
}

// SAFETY: the pointees are `Sync` (`AudioSchedule`, `InputCopy` and the
// monitor routes are plain data, `StreamReader` only touches atomics);
// the callers of `RegionRun::new` and `with_monitor` keep them alive while
// any thread renders the run.
unsafe impl Send for RegionRun {}
// SAFETY: as for `Send`.
unsafe impl Sync for RegionRun {}

impl RegionRun {
    /// No audio: Regions nodes render silence.
    pub const NONE: RegionRun = RegionRun {
        audio: std::ptr::null(),
        reader: std::ptr::null(),
        monitor: None,
        frame: 0,
        playing: false,
        late: false,
        muted: false,
        floor: 0,
        ceiling: u64::MAX,
        wrapped: None,
        played: 0,
    };

    /// A run reading `audio` through `reader` from schedule frame `frame`.
    ///
    /// # Safety
    /// `audio` and `reader` must stay alive and unchanged until the runs
    /// given this value have finished (`Exec::try_finish` returned true).
    pub unsafe fn new(
        audio: &AudioSchedule,
        reader: &StreamReader,
        frame: u64,
        playing: bool,
        late: bool,
        muted: bool,
    ) -> RegionRun {
        RegionRun {
            audio,
            reader,
            frame,
            playing,
            late,
            muted,
            ..RegionRun::NONE
        }
    }

    /// A run with no audio at frame `frame` (monitoring only).
    pub fn transport(frame: u64, playing: bool) -> RegionRun {
        RegionRun {
            frame,
            playing,
            ..RegionRun::NONE
        }
    }

    /// The run with the roll's start `floor`, the end of playing `ceiling`
    /// the loop it wrapped and the frames `played` since the roll (see the
    /// fields).
    pub fn with_bounds(
        self,
        floor: u64,
        ceiling: u64,
        wrapped: Option<Wrapped>,
        played: u64,
    ) -> RegionRun {
        RegionRun {
            floor,
            ceiling,
            wrapped: wrapped.filter(|w| w.end > w.start),
            played,
            ..self
        }
    }

    /// The run also mixes the monitor routes into their paths, from frame
    /// `offset` of `input` (spec §6.6): each route as its mode says, with
    /// the transport `playing` and, while a recording rolls, its record
    /// range `rec`.
    ///
    /// # Safety
    /// `input` and `routes` must stay alive and unchanged until the runs
    /// given the result have finished (`Exec::try_finish` returned true).
    pub(crate) unsafe fn with_monitor(
        self,
        input: &InputCopy,
        routes: &[MonitorRoute],
        offset: u32,
        playing: bool,
        rec: Option<(u64, u64)>,
    ) -> RegionRun {
        RegionRun {
            monitor: (!routes.is_empty()).then_some(Monitor {
                input,
                routes,
                offset,
                playing,
                rec,
            }),
            ..self
        }
    }

    /// Where read frame `x` (`frame − lead`, signed) reads from, and for
    /// how many of the next `left` (≥ 1) frames that holds: `None` is
    /// silence. Never 0 frames. // RT
    fn map(&self, x: i128, lead: u64, k: u32, left: u64) -> (Option<u64>, u64) {
        let left = i128::from(left.max(1));
        // Nothing was played to read before `lead` frames have (I1).
        let quiet = i128::from(lead) - i128::from(self.played) - i128::from(k);
        if quiet > 0 {
            return (None, left.min(quiet).max(1) as u64);
        }
        if let Some(w) = self.wrapped {
            let (s, e) = (w.start, w.end);
            let (s, e) = (i128::from(s), i128::from(e));
            if x < s {
                // The previous pass of the loop: its end.
                // Within the stretch just before the wrap the heard
                // frames are the ones before the loop end; further back
                // the loop repeats (N2).
                let d = s - x;
                let l = e - s;
                let last = i128::from(w.last);
                let d1 = d - last;
                let mid = i128::from(w.wraps.saturating_sub(2)) * l;
                let y = if d <= last {
                    e - d
                } else if d1 <= mid {
                    e - ((d1 - 1) % l + 1)
                } else {
                    e - (d1 - mid)
                };
                let n = left.min(e - y).min(d).max(1);
                return (u64::try_from(y).ok(), n as u64);
            }
        } else {
            let floor = i128::from(self.floor);
            if x < floor {
                return (None, left.min(floor - x).max(1) as u64);
            }
        }
        let ceiling = i128::from(self.ceiling);
        if x >= ceiling {
            return (None, left as u64);
        }
        (u64::try_from(x).ok(), left.min(ceiling - x).max(1) as u64)
    }

    /// The frame a node with lead `lead` reads at the run's first frame,
    /// or `None` when it reads silence there. // RT
    pub fn read_frame(&self, lead: u64) -> Option<u64> {
        self.map(i128::from(self.frame) - i128::from(lead), lead, 0, 1)
            .0
    }

    /// The frame a node with lead `lead` reads at the run's first frame
    /// (`None`: silence) and for how many of the next `left` (≥ 1) frames
    /// its read goes on continuously from there (to the loop's seam, the
    /// roll's floor or the ceiling). Never 0 frames. // RT
    pub fn read_span(&self, lead: u64, left: u64) -> (Option<u64>, u64) {
        self.map(i128::from(self.frame) - i128::from(lead), lead, 0, left)
    }

    /// The earliest frame a node with lead `lead` will read from the run's
    /// first frame on: the frame it reads, else the roll's start while it
    /// is silent before it; `None` once it reads nothing more (the
    /// streamer's read head, Ruling 3). // RT
    pub fn head(&self, lead: u64) -> Option<u64> {
        if !self.playing {
            return None;
        }
        let x = i128::from(self.frame) - i128::from(lead);
        match self.map(x, lead, 0, 1) {
            (Some(f), _) => Some(f),
            (None, _) if x < i128::from(self.floor) => Some(self.floor),
            (None, _) if lead > self.played => Some(self.floor),
            (None, _) => None,
        }
    }

    /// The frame the earliest read (lead `lead`) jumps to after the
    /// remembered loop's seam, while it still reads before it: the
    /// streamer plans it alongside the head, so a loop turned off or
    /// edited meanwhile does not starve it. // RT
    pub fn seam(&self, lead: u64) -> Option<u64> {
        let s = self.wrapped?.start;
        let x = i128::from(self.frame) - i128::from(lead);
        (self.playing && x < i128::from(s)).then_some(s)
    }

    /// Adds the heard monitor routes of `path` to `out`, deciding Auto at
    /// the read frame (`in_range`) (Ruling 4). // RT
    fn add_monitor(&self, path: OutputPathId, in_range: bool, frames: u32, out: &mut StereoBuf) {
        let Some(m) = self.monitor else {
            return;
        };
        // SAFETY: `with_monitor`'s contract keeps both alive for this run.
        let (Some(input), Some(routes)) =
            (unsafe { m.input.as_ref() }, unsafe { m.routes.as_ref() })
        else {
            return;
        };
        let n = frames as usize;
        let at = m.offset as usize;
        for r in routes.iter().take(64) {
            if r.path != path || !r.heard(m.playing, in_range) {
                continue;
            }
            for (dst, ch) in [(&mut out.left, r.left), (&mut out.right, r.right)] {
                let src = input.get(ch, at, n);
                for (d, x) in dst.iter_mut().zip(src) {
                    *d += *x;
                }
            }
        }
    }

    /// Whether read frame `read` lies in the record range. The first
    /// `lead` frames after a roll have no read frame: they count as out of
    /// range, which is what is heard (nothing of the new position has
    /// reached the output yet). Input and region silence share this one
    /// decision. // RT
    fn in_record_range(&self, read: Option<u64>) -> bool {
        self.monitor.is_some_and(|m| {
            m.rec
                .is_some_and(|(a, b)| read.is_some_and(|x| x >= a && x < b))
        })
    }

    /// The routes whose lane's own regions are silent now (bit `k`: route
    /// `k`; Ruling 6): armed, heard and recording inside the record range
    /// at the read frame (`in_range`), whatever path they feed (the lane's regions
    /// may play to other paths through references). // RT
    fn silenced_mask(&self, in_range: bool) -> u64 {
        let Some(m) = self.monitor else {
            return 0;
        };
        // SAFETY: `with_monitor`'s contract keeps the routes alive.
        let Some(routes) = (unsafe { m.routes.as_ref() }) else {
            return 0;
        };
        if !in_range {
            return 0;
        }
        let mut mask = 0u64;
        for (k, r) in routes.iter().enumerate().take(64) {
            if r.armed && r.heard(m.playing, in_range) {
                mask |= 1u64 << k;
            }
        }
        mask
    }

    /// Whether `lane`'s regions are silent in this run: a route in `mask`
    /// is that lane's. // RT
    fn silenced(&self, mask: u64, lane: core_model::ids::LaneId) -> bool {
        if mask == 0 {
            return false;
        }
        let Some(m) = self.monitor else {
            return false;
        };
        // SAFETY: `with_monitor`'s contract keeps the routes alive.
        let Some(routes) = (unsafe { m.routes.as_ref() }) else {
            return false;
        };
        routes
            .iter()
            .enumerate()
            .take(64)
            .any(|(k, r)| mask & (1u64 << k) != 0 && r.lane == lane)
    }

    /// Renders path `path`, whose node leads by `lead`, for `frames`
    /// frames into `out` (cleared first), using `scratch` (at least
    /// `frames` long). // RT
    pub(crate) fn render(
        &self,
        path: OutputPathId,
        lead: u64,
        frames: u32,
        out: &mut StereoBuf,
        scratch: &mut StereoBuf,
    ) {
        out.clear(frames);
        let read = self.read_frame(lead);
        let in_range = self.in_record_range(read);
        self.add_monitor(path, in_range, frames, out);
        let mask = self.silenced_mask(in_range);
        if !self.playing || self.muted {
            return;
        }
        // SAFETY: `new`'s contract keeps both alive for this run.
        let (Some(audio), Some(reader)) = (unsafe { self.audio.as_ref() }, unsafe {
            self.reader.as_ref()
        }) else {
            return;
        };
        let mut off = 0u32;
        let mut x = i128::from(self.frame) - i128::from(lead);
        while off < frames {
            let (src, n) = self.map(x, lead, off, u64::from(frames - off));
            let n = u32::try_from(n)
                .unwrap_or(frames - off)
                .clamp(1, frames - off);
            if let Some(from) = src {
                let skip = |lane| self.silenced(mask, lane);
                render_piece(
                    audio, reader, self.late, path, from, off, n, &skip, out, scratch,
                );
            }
            off += n;
            x += i128::from(n);
        }
    }
}

/// Adds path `path`'s segments over schedule frames `[from, from + n)` to
/// frames `[off, off + n)` of `out`, but none whose lane `skip` names. // RT
#[allow(clippy::too_many_arguments)]
fn render_piece(
    audio: &AudioSchedule,
    reader: &StreamReader,
    late: bool,
    path: OutputPathId,
    from: u64,
    off: u32,
    n: u32,
    skip: &dyn Fn(core_model::ids::LaneId) -> bool,
    out: &mut StereoBuf,
    scratch: &mut StereoBuf,
) {
    let to = from.saturating_add(u64::from(n));
    for seg in audio.overlapping(path, from, to) {
        if !seg.online || skip(seg.top_lane) {
            continue;
        }
        let a = from.max(seg.start);
        let b = to.min(seg.end());
        let k = (b - a) as usize;
        let at = off as usize + (a - from) as usize;
        let file = seg.file_frame + (a - seg.start);
        let (Some(sl), Some(sr)) = (scratch.left.get_mut(..k), scratch.right.get_mut(..k)) else {
            continue;
        };
        match reader.read(seg.media, seg.start, seg.file_frame, file, sl, sr) {
            Read::Done => {
                let (Some(ol), Some(or)) =
                    (out.left.get_mut(at..at + k), out.right.get_mut(at..at + k))
                else {
                    continue;
                };
                for i in 0..k {
                    let g = seg.gain_at(file + i as u64);
                    if let (Some(o), Some(x)) = (ol.get_mut(i), sl.get(i)) {
                        *o += x * g;
                    }
                    if let (Some(o), Some(x)) = (or.get_mut(i), sr.get(i)) {
                        *o += x * g;
                    }
                }
            }
            Read::Miss => reader.count_miss(late),
            Read::Offline => {}
        }
    }
}
