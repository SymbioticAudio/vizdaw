//! Seconds and ticks through the imported tempo map: the model's own
//! indexed lookup ([`TempoClock`]), the one its crossfade partners use, so
//! the crossfades placed here are the ones the model finds.

use super::*;
use core_model::tempo::TempoClock;

pub(super) struct Clock(TempoClock);

impl Clock {
    pub(super) fn new(map: &TempoMap) -> Clock {
        Clock(map.clock())
    }

    /// Seconds from tick 0 to `t`.
    pub(super) fn secs(&self, t: i64) -> f64 {
        self.0.tick_to_seconds(Tick(t))
    }

    /// The tick at `s` seconds, rounded (saturating).
    pub(super) fn tick(&self, s: f64) -> i64 {
        self.0.seconds_to_tick(s).0
    }
}
