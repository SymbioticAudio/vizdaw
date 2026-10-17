//! Host time and tempo for the playhead (spec §3.4, Plan Ruling 10).

use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::time::{PPQ, Tick};

/// Now on the host clock in nanoseconds. On macOS this is CoreAudio host
/// time, the clock of `CACurrentMediaTime` and `CADisplayLink.timestamp`;
/// elsewhere nanoseconds since the first call.
pub fn now_ns() -> u64 {
    #[cfg(target_os = "macos")]
    {
        platform_macos::midi::now_ns()
    }
    #[cfg(not(target_os = "macos"))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        u64::try_from(START.get_or_init(Instant::now).elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// Ticks per second at `tick` of `seq` through its tempo map, measured
/// over the next quarter note (30 720 at 120 BPM, also when `seq` is
/// unknown).
pub fn ticks_per_second(doc: &Document, seq: Option<SeqId>, tick: Tick) -> f64 {
    let default = 2.0 * PPQ as f64;
    let Some(s) = seq.and_then(|s| doc.sequence(s).ok()) else {
        return default;
    };
    let d = s.tempo.tick_to_seconds(tick + Tick(PPQ)) - s.tempo.tick_to_seconds(tick);
    if d.is_finite() && d > 0.0 {
        PPQ as f64 / d
    } else {
        default
    }
}
