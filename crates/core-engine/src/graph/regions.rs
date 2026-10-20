//! The Regions node's run context and render (sub-project 5 spec §5.4;
//! plan 5a Ruling 11). // RT

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
    /// Bit `k`: route `k` is heard.
    heard: u64,
    /// The run's first frame within the copied block.
    offset: u32,
}

/// What `Regions` nodes read during one run: the playing schedule's audio,
/// the stream reader, where the run is, and the monitored input. Built by
/// the renderer (or a test `Runner`) before each run.
#[derive(Debug, Clone, Copy)]
pub struct RegionRun {
    audio: *const AudioSchedule,
    reader: *const StreamReader,
    monitor: Option<Monitor>,
    /// Schedule frame of the run's first frame.
    pub frame: u64,
    /// The transport plays through the whole run.
    pub playing: bool,
    /// Misses count as late starts (spec §5.5, Ruling 10).
    pub late: bool,
    /// The device runs at another rate than the project (Ruling 20).
    pub muted: bool,
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
            monitor: None,
            frame,
            playing,
            late,
            muted,
        }
    }

    /// The run also mixes the input routes `heard` names (bit `k`: route
    /// `k`) into their paths, from frame `offset` of `input` (spec §6.6).
    ///
    /// # Safety
    /// `input` and `routes` must stay alive and unchanged until the runs
    /// given the result have finished (`Exec::try_finish` returned true).
    pub(crate) unsafe fn with_monitor(
        self,
        input: &InputCopy,
        routes: &[MonitorRoute],
        heard: u64,
        offset: u32,
    ) -> RegionRun {
        RegionRun {
            monitor: (heard != 0).then_some(Monitor {
                input,
                routes,
                heard,
                offset,
            }),
            ..self
        }
    }

    /// Adds the heard input routes of `path` to `out`. // RT
    fn add_monitor(&self, path: OutputPathId, frames: u32, out: &mut StereoBuf) {
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
        for (k, r) in routes.iter().enumerate() {
            if r.path != path || k >= 64 || m.heard & (1u64 << k) == 0 {
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

    /// Renders path `path` for `frames` frames into `out` (cleared first),
    /// using `scratch` (at least `frames` long). // RT
    pub(crate) fn render(
        &self,
        path: OutputPathId,
        frames: u32,
        out: &mut StereoBuf,
        scratch: &mut StereoBuf,
    ) {
        out.clear(frames);
        self.add_monitor(path, frames, out);
        if !self.playing || self.muted {
            return;
        }
        // SAFETY: `new`'s contract keeps both alive for this run.
        let (Some(audio), Some(reader)) = (unsafe { self.audio.as_ref() }, unsafe {
            self.reader.as_ref()
        }) else {
            return;
        };
        let from = self.frame;
        let to = from.saturating_add(u64::from(frames));
        for seg in audio.overlapping(path, from, to) {
            if !seg.online {
                continue;
            }
            let a = from.max(seg.start);
            let b = to.min(seg.end());
            let n = (b - a) as usize;
            let off = (a - from) as usize;
            let file = seg.file_frame + (a - seg.start);
            let (Some(sl), Some(sr)) = (scratch.left.get_mut(..n), scratch.right.get_mut(..n))
            else {
                continue;
            };
            match reader.read(seg.media, seg.start, seg.file_frame, file, sl, sr) {
                Read::Done => {
                    let (Some(ol), Some(or)) = (
                        out.left.get_mut(off..off + n),
                        out.right.get_mut(off..off + n),
                    ) else {
                        continue;
                    };
                    for k in 0..n {
                        let g = seg.gain_at(file + k as u64);
                        if let (Some(o), Some(x)) = (ol.get_mut(k), sl.get(k)) {
                            *o += x * g;
                        }
                        if let (Some(o), Some(x)) = (or.get_mut(k), sr.get(k)) {
                            *o += x * g;
                        }
                    }
                }
                Read::Miss => reader.count_miss(self.late),
                Read::Offline => {}
            }
        }
    }
}
