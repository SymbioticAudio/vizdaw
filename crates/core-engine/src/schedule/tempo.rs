//! `TempoTable`: a piecewise-linear frame ↔ tick map sampled from 2a's
//! `TempoMap::tick_to_seconds`, for real-time position lookups.

use core_model::tempo::TempoMap;
use core_model::time::{PPQ, Tick};

/// Largest knot spacing inside a ramp: one 1/64 note (960 ticks at PPQ 15360).
pub const RAMP_KNOT_TICKS: i64 = 960;
/// Inside ramps, knots are subdivided until linear interpolation is within
/// this many ticks (frame → tick) and frames (tick → frame) of 2a's
/// conversion.
const MAX_KNOT_ERROR: f64 = 0.25;

/// One knot: an exact `(frame, tick)` pair (frames are not rounded).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Knot {
    /// Frames from the sequence origin.
    pub frame: f64,
    /// Absolute tick.
    pub tick: i64,
}

/// Frame ↔ tick map. Lookups are a binary search plus one interpolation,
/// with no allocation. // RT (lookups)
#[derive(Debug, Clone, PartialEq)]
pub struct TempoTable {
    knots: Box<[Knot]>,
    /// Frames per tick before the first knot.
    head_slope: f64,
    /// Frames per tick after the last knot (the last tempo is constant).
    tail_slope: f64,
}

fn frames_per_tick(bpm: f64, sample_rate: f64) -> f64 {
    60.0 / (bpm.max(1.0) * PPQ as f64) * sample_rate
}

impl TempoTable {
    /// Samples `tempo` with the origin (frame 0) at tick `origin`.
    pub fn build(tempo: &TempoMap, origin: Tick, sample_rate: f64) -> TempoTable {
        let base = tempo.tick_to_seconds(origin);
        let frame_of = |t: i64| (tempo.tick_to_seconds(Tick(t)) - base) * sample_rate;
        let points = tempo.points();
        let mut ticks: Vec<i64> = vec![origin.0];
        for (i, p) in points.iter().enumerate() {
            if p.tick.0 > origin.0 {
                ticks.push(p.tick.0);
            }
            let next = points.get(i + 1);
            if let Some(q) = next.filter(|q| p.ramp && q.bpm != p.bpm) {
                let mut t = p.tick.0.max(origin.0);
                while t < q.tick.0 {
                    let end = (t + RAMP_KNOT_TICKS).min(q.tick.0);
                    subdivide(&frame_of, t, end, sample_rate, tempo, base, &mut ticks);
                    ticks.push(end);
                    t = end;
                }
            }
        }
        ticks.sort_unstable();
        ticks.dedup();
        let knots: Box<[Knot]> = ticks
            .iter()
            .map(|t| Knot {
                frame: frame_of(*t),
                tick: *t,
            })
            .collect();
        let first_bpm = points.front().map_or(120.0, |p| p.bpm.as_f64());
        let last_bpm = points.last().map_or(120.0, |p| p.bpm.as_f64());
        TempoTable {
            knots,
            head_slope: frames_per_tick(first_bpm, sample_rate),
            tail_slope: frames_per_tick(last_bpm, sample_rate),
        }
    }

    /// The knots, in order.
    pub fn knots(&self) -> &[Knot] {
        &self.knots
    }

    /// The tick at `frame`, rounded to the nearest tick. // RT
    pub fn frame_to_tick(&self, frame: u64) -> Tick {
        let f = frame as f64;
        let i = self.knots.partition_point(|k| k.frame <= f);
        let t = match (
            i.checked_sub(1).and_then(|j| self.knots.get(j)),
            self.knots.get(i),
        ) {
            (Some(a), Some(b)) => {
                let span = b.frame - a.frame;
                let x = if span > 0.0 {
                    (f - a.frame) / span
                } else {
                    0.0
                };
                a.tick as f64 + x * (b.tick - a.tick) as f64
            }
            (Some(a), None) => a.tick as f64 + (f - a.frame) / self.tail_slope,
            (None, Some(b)) => b.tick as f64 - (b.frame - f) / self.head_slope,
            (None, None) => 0.0,
        };
        Tick(t.round() as i64)
    }

    /// The frame at `tick`, rounded to the nearest frame (0 before the
    /// origin). // RT
    pub fn tick_to_frame(&self, tick: Tick) -> u64 {
        let t = tick.0;
        let i = self.knots.partition_point(|k| k.tick <= t);
        let f = match (
            i.checked_sub(1).and_then(|j| self.knots.get(j)),
            self.knots.get(i),
        ) {
            (Some(a), Some(b)) => {
                let span = (b.tick - a.tick) as f64;
                let x = if span > 0.0 {
                    (t - a.tick) as f64 / span
                } else {
                    0.0
                };
                a.frame + x * (b.frame - a.frame)
            }
            (Some(a), None) => a.frame + (t - a.tick) as f64 * self.tail_slope,
            (None, Some(b)) => b.frame - (b.tick - t) as f64 * self.head_slope,
            (None, None) => 0.0,
        };
        if f <= 0.0 { 0 } else { f.round() as u64 }
    }
}

/// Adds knots strictly inside `(a, b)` until linear interpolation between
/// neighbours is within [`MAX_KNOT_ERROR`] ticks and frames of 2a's
/// conversion in both directions.
fn subdivide(
    frame_of: &impl Fn(i64) -> f64,
    a: i64,
    b: i64,
    sample_rate: f64,
    tempo: &TempoMap,
    base: f64,
    out: &mut Vec<i64>,
) {
    if b - a < 2 {
        return;
    }
    let (fa, fb) = (frame_of(a), frame_of(b));
    // frame → tick at the frame midpoint.
    let fm = (fa + fb) / 2.0;
    let tick_linear = (a + b) as f64 / 2.0;
    let tick_exact = tick_at_seconds(tempo, base + fm / sample_rate);
    // tick → frame at the tick midpoint.
    let m = a + (b - a) / 2;
    let frame_linear = fa + (fb - fa) * (m - a) as f64 / (b - a) as f64;
    let frame_exact = frame_of(m);
    if (tick_exact - tick_linear).abs() <= MAX_KNOT_ERROR
        && (frame_exact - frame_linear).abs() <= MAX_KNOT_ERROR
    {
        return;
    }
    subdivide(frame_of, a, m, sample_rate, tempo, base, out);
    out.push(m);
    subdivide(frame_of, m, b, sample_rate, tempo, base, out);
}

/// Fractional tick at `secs`, refined from 2a's rounded inverse by one
/// Newton step on `tick_to_seconds`.
fn tick_at_seconds(tempo: &TempoMap, secs: f64) -> f64 {
    let t0 = tempo.seconds_to_tick(secs).0;
    let s0 = tempo.tick_to_seconds(Tick(t0));
    let s1 = tempo.tick_to_seconds(Tick(t0 + 1));
    let d = s1 - s0;
    if d > 0.0 {
        t0 as f64 + (secs - s0) / d
    } else {
        t0 as f64
    }
}
