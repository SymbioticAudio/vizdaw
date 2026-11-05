//! The Rewind / Fast Forward shuttle (Ruling 5): a tap skips, a hold winds,
//! faster the longer it is held.

use crate::SurfaceAction;
use std::time::{Duration, Instant};

/// A press shorter than this is a tap (Ruling 5).
pub const SHUTTLE_HOLD: Duration = Duration::from_millis(400);

/// At most one Wind per this.
pub const WIND_EVERY: Duration = Duration::from_millis(100);

/// The rate steps: from this held time (ms) on, this many quarter notes a second.
const STEPS: [(u64, f64); 5] = [
    (0, 0.0),
    (400, 4.0),
    (1_400, 8.0),
    (2_400, 16.0),
    (3_400, 32.0),
];

/// Quarter notes per second after holding for `held` (from the press):
/// 0 below SHUTTLE_HOLD, then 4, 8 from 1.4 s, 16 from 2.4 s, 32 from 3.4 s on.
pub fn wind_rate(held: Duration) -> f64 {
    STEPS
        .iter()
        .rev()
        .find(|&&(from, _)| held >= Duration::from_millis(from))
        .map_or(0.0, |&(_, rate)| rate)
}

/// The quarter notes wound over held time `[a, b)` (both measured from
/// the press): the exact integral of [`wind_rate`], summed piece by
/// piece; 0 when `b` is not after `a`. The transport bar's hold winds by
/// it too (plan 7d Ruling 3), so a surface and the bar wind alike.
pub fn wound(a: Duration, b: Duration) -> f64 {
    let mut sum = 0.0;
    for (k, &(from, rate)) in STEPS.iter().enumerate() {
        let lo = Duration::from_millis(from).max(a);
        let hi = STEPS
            .get(k.saturating_add(1))
            .map_or(b, |&(to, _)| Duration::from_millis(to).min(b));
        if hi > lo {
            sum += hi.saturating_sub(lo).as_secs_f64() * rate;
        }
    }
    sum
}

/// A held Rewind or Fast Forward button.
#[derive(Debug, Clone, Copy)]
struct Press {
    forward: bool,
    at: Instant,
    /// When the last Wind was emitted (`None`: none yet).
    last: Option<Instant>,
}

/// The shuttle: which button is held, since when, and the last wind.
#[derive(Debug, Default)]
pub struct Shuttle {
    press: Option<Press>,
}

impl Press {
    /// The wind since the last one (or the press) up to `now`, signed.
    fn wind(&self, now: Instant) -> f64 {
        let from = self
            .last
            .unwrap_or(self.at)
            .saturating_duration_since(self.at);
        let to = now.saturating_duration_since(self.at);
        let q = wound(from, to);
        if self.forward { q } else { -q }
    }
}

impl Shuttle {
    /// A press of Fast Forward (`forward`) or Rewind at `now`.
    pub fn press(&mut self, forward: bool, now: Instant) {
        self.press = Some(Press {
            forward,
            at: now,
            last: None,
        });
    }

    /// `Skip(±1)` when no wind step was emitted (a tap, or a hold released
    /// before its first step); else the `Wind` since the last one (none when zero).
    pub fn release(&mut self, forward: bool, now: Instant) -> Vec<SurfaceAction> {
        let Some(p) = self.press.filter(|p| p.forward == forward) else {
            return Vec::new();
        };
        self.press = None;
        // A release never does nothing (Ruling T1-M4): before SHUTTLE_HOLD it
        // is a tap, and at or after it, with no wind step emitted yet, too.
        if p.last.is_none() {
            return vec![SurfaceAction::Skip(if forward { 1 } else { -1 })];
        }
        let q = p.wind(now);
        if q == 0.0 {
            Vec::new()
        } else {
            vec![SurfaceAction::Wind(q)]
        }
    }

    /// A `Wind` when held past SHUTTLE_HOLD and WIND_EVERY passed since the last.
    pub fn tick(&mut self, now: Instant) -> Vec<SurfaceAction> {
        let Some(p) = self.press.as_mut() else {
            return Vec::new();
        };
        let since = p
            .last
            .unwrap_or_else(|| p.at.checked_add(SHUTTLE_HOLD).unwrap_or(p.at));
        if now.saturating_duration_since(since) < WIND_EVERY {
            return Vec::new();
        }
        let q = p.wind(now);
        if q == 0.0 {
            return Vec::new();
        }
        // `last` is set only by an emitted step, so a release can tell a
        // hold that wound from one that did not.
        p.last = Some(now);
        vec![SurfaceAction::Wind(q)]
    }

    /// Forget a press (release_all).
    pub fn clear(&mut self) {
        self.press = None;
    }
}
