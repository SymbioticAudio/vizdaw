//! Musical time: ticks and bar·beat·unit positions.

use serde::{Deserialize, Serialize};
use std::ops::{Add, Neg, Sub};

/// Ticks per quarter note.
pub const PPQ: i64 = 15_360;

/// Musical time in ticks at [`PPQ`] ticks per quarter note.
///
/// Arithmetic saturates instead of overflowing, so no input can panic.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Tick(pub i64);

impl Tick {
    /// Tick zero: the downbeat of bar 1.
    pub const ZERO: Tick = Tick(0);
    /// The largest tick.
    pub const MAX: Tick = Tick(i64::MAX);
    /// The smallest tick.
    pub const MIN: Tick = Tick(i64::MIN);

    /// `n` quarter notes.
    pub fn quarters(n: i64) -> Tick {
        Tick(n.saturating_mul(PPQ))
    }

    /// Saturating multiplication by an integer.
    pub fn times(self, n: i64) -> Tick {
        Tick(self.0.saturating_mul(n))
    }
}

impl Add for Tick {
    type Output = Tick;
    fn add(self, o: Tick) -> Tick {
        Tick(self.0.saturating_add(o.0))
    }
}

impl Sub for Tick {
    type Output = Tick;
    fn sub(self, o: Tick) -> Tick {
        Tick(self.0.saturating_sub(o.0))
    }
}

impl Neg for Tick {
    type Output = Tick;
    fn neg(self) -> Tick {
        Tick(self.0.saturating_neg())
    }
}

/// A display and entry position. Bar 1 beat 1 unit 0 is tick 0; bar 0 is the
/// pickup bar before it. `beat` is 1-based. `unit` counts display units within
/// the beat at a resolution chosen by the caller (`units_per_beat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarBeatUnit {
    /// Bar number (0 is the pickup bar, may be negative before it).
    pub bar: i32,
    /// Beat within the bar, starting at 1.
    pub beat: u16,
    /// Units within the beat.
    pub unit: u32,
}
