//! The count-in click (sub-project 5 spec §6.4: "Count-in plays the click
//! before the start point without moving the timeline"; plan 5d-1 Ruling
//! 7). The renderer runs a [`Counting`] instead of rolling: it plays one
//! click per beat, the first of each bar accented, then rolls the
//! transport at the block that starts where the count-in ends. The clicks
//! are delayed by the graph latency, so the downbeat is heard one beat
//! after the last click. // RT

/// What Record asks the renderer to count in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CountIn {
    /// Clicks: bars × beats per bar (at least 1).
    pub beats: u32,
    /// Beats per bar: the first of each is accented.
    pub per_bar: u32,
    /// Frames per beat at the tempo of the roll point.
    pub beat_frames: f64,
}

/// A click's length.
pub const CLICK_SECONDS: f64 = 0.025;
/// A click's decay time constant.
pub const DECAY_SECONDS: f64 = 0.006;
/// The accented (first) beat's pitch.
pub const ACCENT_HZ: f64 = 1000.0;
/// The accented beat's peak.
pub const ACCENT_LEVEL: f32 = 0.5;
/// The other beats' pitch.
pub const BEAT_HZ: f64 = 800.0;
/// The other beats' peak.
pub const BEAT_LEVEL: f32 = 0.35;

/// A count-in in progress on the real-time side. `Copy`, so starting one
/// never allocates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Counting {
    c: CountIn,
    /// Where playing stops (the Play's `until`).
    until: Option<u64>,
    rate: f64,
    /// Frames between rendering a click and hearing it (the graph latency
    /// when the count-in started).
    delay: u64,
    /// Silence before the first click, chosen at the first block so the
    /// roll falls on a block boundary; `None` before it.
    pad: Option<u64>,
    /// Frames rendered since it started.
    elapsed: u64,
    rolled: bool,
}

impl Counting {
    /// A count-in for `c` at `rate` Hz with graph latency `latency`, for a
    /// Play that stops at `until`.
    pub fn new(c: CountIn, until: Option<u64>, rate: f64, latency: u64) -> Counting {
        Counting {
            c: CountIn {
                beats: c.beats.max(1),
                per_bar: c.per_bar.max(1),
                beat_frames: if c.beat_frames.is_finite() {
                    c.beat_frames.max(1.0)
                } else {
                    1.0
                },
            },
            until,
            rate: rate.max(1.0),
            delay: latency,
            pad: None,
            elapsed: 0,
            rolled: false,
        }
    }

    /// Follows the graph latency as it changes during the count-in. // RT
    pub fn set_latency(&mut self, latency: u64) {
        self.delay = latency;
    }

    /// The Play's `until`.
    pub fn until(&self) -> Option<u64> {
        self.until
    }

    /// Whether the transport has rolled.
    pub fn rolled(&self) -> bool {
        self.rolled
    }

    /// Frames from the first click to the roll.
    fn total(&self) -> u64 {
        (f64::from(self.c.beats) * self.c.beat_frames).round() as u64
    }

    /// Where click `k` starts, in frames since the count-in started.
    fn click_at(&self, k: u32) -> u64 {
        let pad = self.pad.unwrap_or(0);
        pad.saturating_add(self.delay)
            .saturating_add((f64::from(k) * self.c.beat_frames).round() as u64)
    }

    fn click_len(&self) -> u64 {
        (CLICK_SECONDS * self.rate).round() as u64
    }

    /// At the start of a block of `frames` frames: whether the transport
    /// rolls now. The first block chooses the pad, so that at a constant
    /// block size the roll is exactly where the count-in ends. // RT
    pub fn rolls(&mut self, frames: u32) -> bool {
        let n = u64::from(frames.max(1));
        let pad = *self.pad.get_or_insert((n - self.total() % n) % n);
        if !self.rolled && self.elapsed >= pad.saturating_add(self.total()) {
            self.rolled = true;
            return true;
        }
        false
    }

    /// Adds this block's clicks to channels 0–1 of `out` and advances.
    /// // RT
    pub fn render(&mut self, out: &mut [&mut [f32]], frames: u32) {
        let from = self.elapsed;
        let to = from.saturating_add(u64::from(frames));
        let len = self.click_len();
        for k in 0..self.c.beats {
            let at = self.click_at(k);
            if at >= to || at.saturating_add(len) <= from {
                continue;
            }
            let (hz, level) = if k % self.c.per_bar == 0 {
                (ACCENT_HZ, ACCENT_LEVEL)
            } else {
                (BEAT_HZ, BEAT_LEVEL)
            };
            let a = at.max(from);
            let b = at.saturating_add(len).min(to);
            for f in a..b {
                let t = (f - at) as f64 / self.rate;
                let v = (f64::from(level)
                    * (std::f64::consts::TAU * hz * t).sin()
                    * (-t / DECAY_SECONDS).exp()) as f32;
                let i = (f - from) as usize;
                for ch in out.iter_mut().take(2) {
                    if let Some(x) = ch.get_mut(i) {
                        *x += v;
                    }
                }
            }
        }
        self.elapsed = to;
    }

    /// Whether it rolled and every click has sounded.
    pub fn done(&self) -> bool {
        self.rolled
            && self.elapsed
                >= self
                    .click_at(self.c.beats.saturating_sub(1))
                    .saturating_add(self.click_len())
    }
}
