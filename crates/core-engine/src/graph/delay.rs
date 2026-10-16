//! Fixed stereo delay lines for PDC edges and bypassed inserts: allocated
//! once on the control thread, processed allocation-free.

use crate::backend::StereoBuf;

/// A stereo delay of a fixed number of frames.
#[derive(Debug, Clone)]
pub struct DelayLine {
    left: Box<[f32]>,
    right: Box<[f32]>,
    pos: usize,
}

impl DelayLine {
    /// A silent delay of `delay` frames (0 passes audio through).
    pub fn new(delay: u32) -> DelayLine {
        let n = delay as usize;
        DelayLine {
            left: vec![0.0; n].into_boxed_slice(),
            right: vec![0.0; n].into_boxed_slice(),
            pos: 0,
        }
    }

    /// The delay in frames.
    pub fn delay(&self) -> u32 {
        u32::try_from(self.left.len()).unwrap_or(u32::MAX)
    }

    /// Replaces `left`/`right` (same length) with the input delayed by
    /// `delay()` frames. // RT
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let d = self.left.len().min(self.right.len());
        if d == 0 {
            return;
        }
        for (a, b) in left.iter_mut().zip(right.iter_mut()) {
            let (Some(x), Some(y)) = (self.left.get_mut(self.pos), self.right.get_mut(self.pos))
            else {
                return;
            };
            std::mem::swap(a, x);
            std::mem::swap(b, y);
            self.pos += 1;
            if self.pos == d {
                self.pos = 0;
            }
        }
    }

    /// [`DelayLine::process`] on the first `frames` frames of `buf`. // RT
    pub fn process_buf(&mut self, buf: &mut StereoBuf, frames: u32) {
        let n = (frames as usize).min(buf.left.len()).min(buf.right.len());
        if let (Some(l), Some(r)) = (buf.left.get_mut(..n), buf.right.get_mut(..n)) {
            self.process(l, r);
        }
    }

    /// Silences the delay memory. // RT
    pub fn clear(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.pos = 0;
    }
}
