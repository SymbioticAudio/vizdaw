//! Seconds and ticks through the imported tempo map.

use super::*;

/// One tempo segment: its start and end ticks, its tempos, and the seconds
/// at its start.
#[derive(Debug, Clone, Copy)]
struct Seg {
    start: i64,
    end: Option<i64>,
    b0: f64,
    b1: f64,
    acc: f64,
}

/// Seconds and ticks through a tempo map, as `TempoMap::tick_to_seconds`
/// and `seconds_to_tick` compute them (the same float operations in the
/// same order), found by binary search so a long map stays fast.
pub(super) struct Clock {
    seg: Vec<Seg>,
    first: f64,
}

fn spt(bpm: f64) -> f64 {
    60.0 / (bpm * PPQ as f64)
}

fn seg_seconds(dx: f64, len: f64, b0: f64, b1: f64) -> f64 {
    if (b1 - b0).abs() < 1e-9 || len <= 0.0 {
        return dx * spt(b0);
    }
    let k = (b1 - b0) / len;
    60.0 / (PPQ as f64 * k) * (k * dx / b0).ln_1p()
}

fn seg_ticks(s: f64, len: f64, b0: f64, b1: f64) -> f64 {
    if (b1 - b0).abs() < 1e-9 || len <= 0.0 {
        return s / spt(b0);
    }
    let k = (b1 - b0) / len;
    b0 * (s * PPQ as f64 * k / 60.0).exp_m1() / k
}

impl Clock {
    pub(super) fn new(map: &TempoMap) -> Clock {
        let pts: Vec<TempoPoint> = map.points().iter().copied().collect();
        let first = pts.first().map_or(120.0, |p| p.bpm.as_f64().max(1.0));
        let mut seg = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        for (i, p) in pts.iter().enumerate() {
            let next = pts.get(i + 1);
            let b0 = p.bpm.as_f64().max(1.0);
            let b1 = match next {
                Some(q) if p.ramp => q.bpm.as_f64().max(1.0),
                _ => b0,
            };
            let end = next.map(|q| q.tick.0);
            seg.push(Seg {
                start: p.tick.0,
                end,
                b0,
                b1,
                acc,
            });
            if let Some(e) = end {
                let len = (e - p.tick.0) as f64;
                acc += seg_seconds(len, len, b0, b1);
            }
        }
        Clock { seg, first }
    }

    /// Seconds from tick 0 to `t`.
    pub(super) fn secs(&self, t: i64) -> f64 {
        if t < 0 || self.seg.is_empty() {
            return t as f64 * spt(self.first);
        }
        let i = self.seg.partition_point(|s| s.start <= t);
        let Some(s) = i.checked_sub(1).and_then(|i| self.seg.get(i)) else {
            return 0.0;
        };
        let seg_end = s.end.unwrap_or(i64::MAX);
        let len = (seg_end - s.start) as f64;
        let dx = (t.min(seg_end) - s.start) as f64;
        s.acc + seg_seconds(dx, len, s.b0, s.b1)
    }

    /// The tick at `s` seconds, rounded (saturating).
    pub(super) fn tick(&self, s: f64) -> i64 {
        if s.is_nan() {
            return i64::MIN;
        }
        if s.is_infinite() {
            return if s > 0.0 { i64::MAX } else { i64::MIN };
        }
        if s < 0.0 || self.seg.is_empty() {
            return (s / spt(self.first)).round() as i64;
        }
        // The segments that end at or before `s`.
        let i = self
            .seg
            .get(1..)
            .map_or(0, |rest| rest.partition_point(|x| s >= x.acc));
        let Some(g) = self.seg.get(i) else {
            return 0;
        };
        let dx = match g.end {
            Some(e) => {
                let len = (e - g.start) as f64;
                seg_ticks(s - g.acc, len, g.b0, g.b1)
            }
            None => (s - g.acc) / spt(g.b0),
        };
        (g.start as f64 + dx).round() as i64
    }
}
