//! Meter maps (time signatures and key signatures) and bar·beat·unit
//! conversion.

use crate::error::{ModelError, Result};
use crate::time::{BarBeatUnit, PPQ, Tick};
use im::Vector;
use serde::{Deserialize, Serialize};

/// A time signature. `den` must be a power of two from 1 to 64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meter {
    /// Beats per bar (1–255).
    pub num: u8,
    /// Beat unit as a note value (4 = quarter note).
    pub den: u8,
}

impl Meter {
    /// Ticks per beat.
    pub fn beat_ticks(self) -> i64 {
        (PPQ * 4) / i64::from(self.den.max(1))
    }

    /// Ticks per bar.
    pub fn bar_ticks(self) -> i64 {
        self.beat_ticks() * i64::from(self.num.max(1))
    }

    /// True when `num >= 1` and `den` is a power of two in 1..=64.
    pub fn is_valid(self) -> bool {
        self.num >= 1 && self.den.is_power_of_two() && self.den <= 64
    }
}

/// A key signature: `sharps` in -7..=7 (negative = flats).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeySig {
    /// Number of sharps (negative for flats).
    pub sharps: i8,
    /// Minor mode.
    pub minor: bool,
}

/// A meter (and key) change. Changes fall on barlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeterEntry {
    /// Where the change takes effect (a barline).
    pub tick: Tick,
    /// The new time signature.
    pub meter: Meter,
    /// The key signature from here on.
    pub key: KeySig,
}

/// Ordered meter entries. The first is at tick 0 (bar 1) and also governs
/// the pickup bars before it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeterMap {
    entries: Vector<MeterEntry>,
}

const FOUR_FOUR: MeterEntry = MeterEntry {
    tick: Tick::ZERO,
    meter: Meter { num: 4, den: 4 },
    key: KeySig {
        sharps: 0,
        minor: false,
    },
};

impl Default for MeterMap {
    fn default() -> Self {
        MeterMap {
            entries: Vector::unit(FOUR_FOUR),
        }
    }
}

impl MeterMap {
    /// Builds a validated map.
    pub fn new(entries: Vec<MeterEntry>) -> Result<MeterMap> {
        let m = MeterMap {
            entries: entries.into_iter().collect(),
        };
        m.check()?;
        Ok(m)
    }

    /// The entries, in order.
    pub fn entries(&self) -> &Vector<MeterEntry> {
        &self.entries
    }

    /// Returns the first rule violation: empty map, first entry not at tick
    /// 0, invalid meter, key outside -7..=7, unordered ticks, or a change off
    /// the previous meter's barlines ([`ModelError::NotOnBarline`]).
    pub fn check(&self) -> Result<()> {
        let first = self
            .entries
            .front()
            .ok_or_else(|| ModelError::invalid("meter map is empty"))?;
        if first.tick != Tick::ZERO {
            return Err(ModelError::range("meter.first_tick", first.tick.0));
        }
        let mut prev: Option<&MeterEntry> = None;
        for e in &self.entries {
            if !e.meter.is_valid() {
                return Err(ModelError::range(
                    "meter.signature",
                    format!("{}/{}", e.meter.num, e.meter.den),
                ));
            }
            if !(-7..=7).contains(&e.key.sharps) {
                return Err(ModelError::range("meter.key", e.key.sharps));
            }
            if let Some(p) = prev {
                if e.tick <= p.tick {
                    return Err(ModelError::range("meter.tick_order", e.tick.0));
                }
                if (e.tick.0 - p.tick.0) % p.meter.bar_ticks() != 0 {
                    return Err(ModelError::NotOnBarline { tick: e.tick.0 });
                }
            }
            prev = Some(e);
        }
        Ok(())
    }

    /// Entries paired with the bar number they start at.
    fn with_bars(&self) -> Vec<(MeterEntry, i64)> {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut bar = 1i64;
        let mut prev: Option<MeterEntry> = None;
        for e in &self.entries {
            if let Some(p) = prev {
                bar += (e.tick.0 - p.tick.0) / p.meter.bar_ticks();
            }
            out.push((*e, bar));
            prev = Some(*e);
        }
        if out.is_empty() {
            out.push((FOUR_FOUR, 1));
        }
        out
    }

    /// The entry in effect at `t` (the first entry for negative ticks).
    pub fn entry_at(&self, t: Tick) -> MeterEntry {
        self.entries
            .iter()
            .take_while(|e| e.tick <= t)
            .last()
            .or_else(|| self.entries.front())
            .copied()
            .unwrap_or(FOUR_FOUR)
    }

    /// Converts a tick to bar·beat·unit with `units_per_beat` display units
    /// per beat (0 is treated as 1).
    pub fn tick_to_bbu(&self, t: Tick, units_per_beat: u32) -> BarBeatUnit {
        let upb = i128::from(units_per_beat.max(1));
        let bars = self.with_bars();
        let (e, bar0) = bars
            .iter()
            .take_while(|(e, _)| e.tick <= t)
            .last()
            .or_else(|| bars.first())
            .copied()
            .unwrap_or((FOUR_FOUR, 1));
        let bar_t = e.meter.bar_ticks();
        let beat_t = e.meter.beat_ticks();
        let rel = t.0.saturating_sub(e.tick.0);
        let bar = bar0.saturating_add(rel.div_euclid(bar_t));
        let in_bar = rel.rem_euclid(bar_t);
        let beat = in_bar / beat_t;
        let within = in_bar % beat_t;
        let unit = i128::from(within) * upb / i128::from(beat_t);
        BarBeatUnit {
            bar: i32::try_from(bar).unwrap_or(if bar < 0 { i32::MIN } else { i32::MAX }),
            beat: u16::try_from(beat + 1).unwrap_or(u16::MAX),
            unit: u32::try_from(unit).unwrap_or(u32::MAX),
        }
    }

    /// Converts bar·beat·unit back to the first tick of that unit. Beat 0 is treated as
    /// beat 1; beats past the bar run into the following bars.
    pub fn bbu_to_tick(&self, p: BarBeatUnit, units_per_beat: u32) -> Tick {
        let upb = i128::from(units_per_beat.max(1));
        let bars = self.with_bars();
        let target = i64::from(p.bar);
        let (e, bar0) = bars
            .iter()
            .take_while(|(_, b)| *b <= target)
            .last()
            .or_else(|| bars.first())
            .copied()
            .unwrap_or((FOUR_FOUR, 1));
        let beat_t = e.meter.beat_ticks();
        let bars_in = (target - bar0).saturating_mul(e.meter.bar_ticks());
        let beats_in = i64::from(p.beat.saturating_sub(1)).saturating_mul(beat_t);
        // Ceiling division: the first tick whose display unit is `p.unit`.
        let units_in = (i128::from(p.unit) * i128::from(beat_t) + upb - 1) / upb;
        let units_in = i64::try_from(units_in).unwrap_or(i64::MAX);
        Tick(
            e.tick
                .0
                .saturating_add(bars_in)
                .saturating_add(beats_in)
                .saturating_add(units_in),
        )
    }

    /// The first barline at or after `t`.
    pub fn barline_at_or_after(&self, t: Tick) -> Tick {
        let b = self.barline_at_or_before(t);
        if b == t {
            b
        } else {
            b + Tick(self.entry_at(t).meter.bar_ticks())
        }
    }

    /// The last barline at or before `t`.
    pub fn barline_at_or_before(&self, t: Tick) -> Tick {
        let e = self.entry_at(t);
        let bar_t = e.meter.bar_ticks();
        let rel = t.0.saturating_sub(e.tick.0);
        Tick(e.tick.0.saturating_add(rel.div_euclid(bar_t) * bar_t))
    }
}
