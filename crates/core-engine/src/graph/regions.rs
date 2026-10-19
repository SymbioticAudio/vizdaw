//! The Regions node's run context and render (sub-project 5 spec §5.4;
//! plan 5a Ruling 11). // RT

use crate::backend::StereoBuf;
use crate::schedule::AudioSchedule;
use core_media::stream::{Read, StreamReader};
use core_model::ids::OutputPathId;

/// What `Regions` nodes read during one run: the playing schedule's audio,
/// the stream reader, and where the run is. Built by the renderer (or a
/// test `Runner`) before each run.
#[derive(Debug, Clone, Copy)]
pub struct RegionRun {
    audio: *const AudioSchedule,
    reader: *const StreamReader,
    /// Schedule frame of the run's first frame.
    pub frame: u64,
    /// The transport plays through the whole run.
    pub playing: bool,
    /// Misses count as late starts (spec §5.5, Ruling 10).
    pub late: bool,
    /// The device runs at another rate than the project (Ruling 20).
    pub muted: bool,
}

// SAFETY: the pointees are `Sync` (`AudioSchedule` is plain data,
// `StreamReader` only touches atomics); `RegionRun::new`'s caller keeps
// them alive while any thread renders the run.
unsafe impl Send for RegionRun {}
// SAFETY: as for `Send`.
unsafe impl Sync for RegionRun {}

impl RegionRun {
    /// No audio: Regions nodes render silence.
    pub const NONE: RegionRun = RegionRun {
        audio: std::ptr::null(),
        reader: std::ptr::null(),
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
            frame,
            playing,
            late,
            muted,
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
