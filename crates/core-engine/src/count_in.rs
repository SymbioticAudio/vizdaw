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

/// Sample `offset` (frames since the click started) of a click at `rate`
/// Hz: the first beat of a bar (`k_in_bar` 0) is the accent, any other
/// beat the plain click; `gain` scales the peak. Zero from
/// [`CLICK_SECONDS`] on. The count-in and the metronome both sound
/// through it, so their clicks are one sound (groups-vcas-transport
/// Ruling 19). // RT
pub fn click_sample(k_in_bar: u32, offset: u64, rate: f64, gain: f32) -> f32 {
    let rate = rate.max(1.0);
    let t = offset as f64 / rate;
    if t >= CLICK_SECONDS {
        return 0.0;
    }
    let (hz, level) = if k_in_bar == 0 {
        (ACCENT_HZ, ACCENT_LEVEL)
    } else {
        (BEAT_HZ, BEAT_LEVEL)
    };
    (f64::from(level)
        * f64::from(gain)
        * (std::f64::consts::TAU * hz * t).sin()
        * (-t / DECAY_SECONDS).exp()) as f32
}

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
    /// Clicks played before a device rebuild (plan release-fixes Task 1):
    /// this count-in starts at click `first`.
    first: u32,
    /// The first click not started yet: a latency change moves only these
    /// (Task 5 parked N1).
    next: u32,
    /// The click started last and where it started, so its tail plays on
    /// where it began whatever the latency does next.
    last: Option<(u32, u64)>,
}

impl Counting {
    /// A count-in for `c` at `rate` Hz with graph latency `latency`, for a
    /// Play that stops at `until`. `done`: clicks already played before a
    /// rebuild; clicks 0..done are skipped and click `done` sounds first,
    /// at the start. Accents still follow the click's index k
    /// (k % per_bar == 0).
    pub fn new(c: CountIn, until: Option<u64>, rate: f64, latency: u64, done: u32) -> Counting {
        let beats = c.beats.max(1);
        let first = done.min(beats);
        Counting {
            c: CountIn {
                beats,
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
            first,
            next: first,
            last: None,
        }
    }

    /// Follows the graph latency as it changes during the count-in: the
    /// clicks not started yet move; one already sounding plays on (Task 5
    /// parked N1). // RT
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

    /// Clicks started so far, counting those played before a rebuild.
    pub fn started(&self) -> u32 {
        self.next
    }

    /// Frames from the first click to the roll.
    fn total(&self) -> u64 {
        (f64::from(self.c.beats - self.first) * self.c.beat_frames).round() as u64
    }

    /// Where click `k` starts, in frames since the count-in started.
    fn click_at(&self, k: u32) -> u64 {
        let pad = self.pad.unwrap_or(0);
        pad.saturating_add(self.delay).saturating_add(
            (f64::from(k.saturating_sub(self.first)) * self.c.beat_frames).round() as u64,
        )
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
    /// A click that has started keeps its start; one not started yet is
    /// placed by the current latency, never before this block (a fall in
    /// latency makes it late rather than dropping it; Task 5 parked N1),
    /// and never less than a beat after the click before it (a fall of
    /// more than a beat plays one click per beat, not two at once; plan
    /// release-fixes Task 1). // RT
    pub fn render(&mut self, out: &mut [&mut [f32]], frames: u32) {
        let from = self.elapsed;
        let to = from.saturating_add(u64::from(frames));
        if let Some((k, at)) = self.last {
            self.click(out, k, at, from, to);
        }
        while self.next < self.c.beats {
            let k = self.next;
            let beat = self.c.beat_frames.round() as u64;
            let floor = self.last.map_or(0, |(_, at)| at.saturating_add(beat));
            let at = self.click_at(k).max(from).max(floor);
            if at >= to {
                break;
            }
            self.click(out, k, at, from, to);
            self.last = Some((k, at));
            self.next = k.saturating_add(1);
        }
        self.elapsed = to;
    }

    /// Adds the part of click `k` (starting at `at`) inside `[from, to)`.
    /// // RT
    fn click(&self, out: &mut [&mut [f32]], k: u32, at: u64, from: u64, to: u64) {
        let a = at.max(from);
        let b = at.saturating_add(self.click_len()).min(to);
        for f in a..b {
            let v = click_sample(k % self.c.per_bar, f - at, self.rate, 1.0);
            let i = (f - from) as usize;
            for ch in out.iter_mut().take(2) {
                if let Some(x) = ch.get_mut(i) {
                    *x += v;
                }
            }
        }
    }

    /// Whether it rolled and every click has sounded.
    pub fn done(&self) -> bool {
        self.rolled
            && self.next >= self.c.beats
            && self
                .last
                .is_none_or(|(_, at)| self.elapsed >= at.saturating_add(self.click_len()))
    }
}
