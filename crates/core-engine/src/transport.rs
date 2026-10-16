//! The transport state machine. It splits each block into steps at loop
//! ends and the sequence end; it never allocates. // RT

/// Transport state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayState {
    /// Not moving.
    Stopped = 0,
    /// Playing the schedule.
    Playing = 1,
    /// Past the sequence end, letting sounds ring out.
    Tail = 2,
}

impl PlayState {
    /// From the published `u8`.
    pub fn from_u8(v: u8) -> PlayState {
        match v {
            1 => PlayState::Playing,
            2 => PlayState::Tail,
            _ => PlayState::Stopped,
        }
    }
}

/// One piece of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Emit entries with `from <= frame < to`; block offset of `from` is
    /// `offset`; covers `to - from` frames.
    Play {
        /// First schedule frame.
        from: u64,
        /// End schedule frame (exclusive).
        to: u64,
        /// Block offset of `from`.
        offset: u32,
    },
    /// Loop end reached at `offset`: release notes, continue at the loop
    /// start. Covers no frames.
    Wrap {
        /// Block offset.
        offset: u32,
        /// Schedule frame playback continues from.
        to: u64,
    },
    /// Sequence end reached at `offset`: release notes, play the tail.
    /// Covers no frames.
    End {
        /// Block offset.
        offset: u32,
    },
    /// Nothing to emit for `len` frames (stopped or in the tail).
    Idle {
        /// Frames covered.
        len: u32,
    },
}

impl Step {
    /// Frames this step covers.
    pub fn frames(&self) -> u32 {
        match self {
            Step::Play { from, to, .. } => u32::try_from(to - from).unwrap_or(u32::MAX),
            Step::Wrap { .. } | Step::End { .. } => 0,
            Step::Idle { len } => *len,
        }
    }
}

/// Transport position and mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transport {
    /// State.
    pub state: PlayState,
    /// Schedule frame of the next frame to play.
    pub pos: u64,
    /// Loop range `[start, end)`; ignored unless `end > start`.
    pub loop_range: Option<(u64, u64)>,
    /// Tail length in frames.
    pub tail_frames: u64,
    tail_left: u64,
    ended: bool,
}

impl Transport {
    /// A stopped transport at frame 0.
    pub fn new(tail_frames: u64) -> Transport {
        Transport {
            state: PlayState::Stopped,
            pos: 0,
            loop_range: None,
            tail_frames,
            tail_left: 0,
            ended: false,
        }
    }

    fn active_loop(&self) -> Option<(u64, u64)> {
        self.loop_range.filter(|(s, e)| e > s)
    }

    /// Starts playing from `pos` (from 0 when at or past `length`).
    pub fn play(&mut self, length: u64) {
        if self.state == PlayState::Playing {
            return;
        }
        if self.pos >= length {
            self.pos = 0;
        }
        self.state = PlayState::Playing;
    }

    /// Stops; the position is kept.
    pub fn stop(&mut self) {
        self.state = PlayState::Stopped;
    }

    /// Moves to `frame`; a transport in its tail plays again.
    pub fn locate(&mut self, frame: u64) {
        self.pos = frame;
        if self.state == PlayState::Tail {
            self.state = PlayState::Playing;
        }
    }

    /// True once, after the tail finished. // RT
    pub fn take_ended(&mut self) -> bool {
        std::mem::take(&mut self.ended)
    }

    /// The next step of a block at `offset` with `remaining > 0` frames
    /// left, for a schedule `length` frames long. // RT
    pub fn next(&mut self, length: u64, offset: u32, remaining: u32) -> Step {
        match self.state {
            PlayState::Stopped => Step::Idle { len: remaining },
            PlayState::Tail => {
                let n = u64::from(remaining).min(self.tail_left);
                self.tail_left -= n;
                if self.tail_left == 0 {
                    self.state = PlayState::Stopped;
                    self.ended = true;
                }
                Step::Idle {
                    len: u32::try_from(n).unwrap_or(remaining),
                }
            }
            PlayState::Playing => {
                let lp = self.active_loop();
                if let Some((s, e)) = lp {
                    if self.pos == e {
                        self.pos = s;
                        return Step::Wrap { offset, to: s };
                    }
                }
                let end = match lp {
                    Some((_, e)) if self.pos < e => e,
                    _ => length,
                };
                if self.pos >= end {
                    self.state = PlayState::Tail;
                    self.tail_left = self.tail_frames;
                    if self.tail_left == 0 {
                        self.state = PlayState::Stopped;
                        self.ended = true;
                    }
                    return Step::End { offset };
                }
                let n = (end - self.pos).min(u64::from(remaining));
                let from = self.pos;
                self.pos += n;
                Step::Play {
                    from,
                    to: self.pos,
                    offset,
                }
            }
        }
    }
}
