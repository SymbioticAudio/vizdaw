//! A measurement hook for the meters' cost (slice 6a Decision M1), off
//! unless `VIZDAW_FRAME_STATS` is set; nothing of it is in the user's UI.
//!
//! - `VIZDAW_FRAME_STATS=1`: every window times its frames (from the start
//!   of its render to the end of its paint, and from one paint's end to
//!   the next), and `state.get windows` adds a `frames` object to each
//!   window's row with what was timed since the last read (the read
//!   starts the next span).
//! - `VIZDAW_FRAME_STATS=meters`: the same, and every strip's meter is
//!   driven by a synthetic signal on top of what `levels()` reads (still
//!   read at 30 Hz), so N strips meter without audio.
//!
//! A frame interval over [`DROPPED_MS`] counts as dropped (the spike's
//! rule); one of [`IDLE_MS`] or more is an idle gap (nothing asked for a
//! frame), counted apart and left out of the interval figures.

use std::sync::OnceLock;
use std::time::Instant;

use serde_json::{Value, json};
use vizdaw_ffi::mixer::{MeterRow, MixerLevels};

/// The environment variable that turns the hook on.
pub const ENV: &str = "VIZDAW_FRAME_STATS";
/// An interval longer than this is a dropped frame (at 60 Hz).
pub const DROPPED_MS: f64 = 25.;
/// An interval this long or longer is an idle gap, not a frame.
pub const IDLE_MS: f64 = 250.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    Frames,
    Meters,
}

fn mode() -> Mode {
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| match std::env::var(ENV).ok().as_deref() {
        None | Some("") | Some("0") => Mode::Off,
        Some("meters") => Mode::Meters,
        Some(_) => Mode::Frames,
    })
}

/// Whether windows time their frames.
pub fn enabled() -> bool {
    mode() != Mode::Off
}

/// Whether the meters take the synthetic signal.
pub fn synthetic_meters() -> bool {
    mode() == Mode::Meters
}

/// A read of every one of `strips` at `now`: a slow sine per strip, each
/// a little behind the last, peaking near 0 dBFS.
pub fn synthetic_levels(
    strips: impl Iterator<Item = u64>,
    started: Instant,
    now: Instant,
) -> MixerLevels {
    let t = now.saturating_duration_since(started).as_secs_f32();
    let meters = strips
        .enumerate()
        .map(|(i, strip)| {
            let phase = t * std::f32::consts::TAU * 0.5 + i as f32 * 0.4;
            let peak = 0.05 + 0.9 * (0.5 + 0.5 * phase.sin());
            MeterRow {
                strip,
                peak_l: peak,
                peak_r: peak * 0.9,
            }
        })
        .collect();
    MixerLevels {
        meters,
        dsp_load: 0.,
        dsp_peak: 0.,
        latency_frames: 0,
        faulted: vec![],
        midi: vec![],
    }
}

/// One window's frames since the last read.
#[derive(Debug, Default)]
pub struct FrameStats {
    since: Option<Instant>,
    render_started: Option<Instant>,
    last_paint: Option<Instant>,
    /// Paint end to paint end, in ms (idle gaps left out).
    intervals: Vec<f64>,
    /// Render start to paint end, in ms.
    builds: Vec<f64>,
    idle: u64,
    /// The strips the window's placements drew, at the last frame.
    pub strips: usize,
}

impl FrameStats {
    /// The window's render starts.
    pub fn render_started(&mut self, now: Instant) {
        self.since.get_or_insert(now);
        self.render_started = Some(now);
    }

    /// The window's paint ends.
    pub fn painted(&mut self, now: Instant) {
        if let Some(start) = self.render_started.take() {
            self.builds.push(ms(now.saturating_duration_since(start)));
        }
        if let Some(last) = self.last_paint {
            let gap = ms(now.saturating_duration_since(last));
            if gap >= IDLE_MS {
                self.idle += 1;
            } else {
                self.intervals.push(gap);
            }
        }
        self.last_paint = Some(now);
    }

    /// What was timed since the last read, as JSON; the next span starts.
    pub fn take_json(&mut self, now: Instant) -> Value {
        let span = self
            .since
            .map_or(0., |s| ms(now.saturating_duration_since(s)));
        let intervals = std::mem::take(&mut self.intervals);
        let builds = std::mem::take(&mut self.builds);
        let dropped = intervals.iter().filter(|i| **i > DROPPED_MS).count();
        let idle = std::mem::take(&mut self.idle);
        self.since = Some(now);
        json!({
            "span_ms": span,
            "frames": builds.len(),
            "dropped": dropped,
            "idle_gaps": idle,
            "fps": if span > 0. { builds.len() as f64 * 1000. / span } else { 0. },
            "interval_ms": summary(intervals),
            "build_ms": summary(builds),
            "strips": self.strips,
            "synthetic_meters": synthetic_meters(),
        })
    }
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.
}

/// p50, p99 and max of `v`.
fn summary(mut v: Vec<f64>) -> Value {
    if v.is_empty() {
        return json!({"p50": null, "p99": null, "max": null});
    }
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    json!({"p50": at(0.5), "p99": at(0.99), "max": v[v.len() - 1]})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn frames_count_dropped_and_idle_apart() {
        let t0 = Instant::now();
        let mut f = FrameStats::default();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        for (start, end) in [(0, 4), (12, 20), (30, 37), (70, 75), (600, 604)] {
            f.render_started(at(start));
            f.painted(at(end));
        }
        let j = f.take_json(at(1000));
        assert_eq!(j["frames"], 5);
        // 16, 17, 38 (dropped); 529 is an idle gap.
        assert_eq!(j["dropped"], 1);
        assert_eq!(j["idle_gaps"], 1);
        assert_eq!(j["interval_ms"]["max"], 38.);
        assert_eq!(j["build_ms"]["max"], 8.);
        let next = f.take_json(at(1100));
        assert_eq!(next["frames"], 0);
        assert_eq!(next["span_ms"], 100.);
    }

    #[test]
    fn the_synthetic_signal_covers_every_strip() {
        let t0 = Instant::now();
        let l = synthetic_levels([1, 2, 3].into_iter(), t0, t0 + Duration::from_millis(300));
        assert_eq!(
            l.meters.iter().map(|m| m.strip).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(l.meters.iter().all(|m| m.peak_l > 0.04 && m.peak_l < 1.));
    }
}
