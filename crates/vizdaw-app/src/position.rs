//! Positions as bar.beat.unit (Swift: `Bbu` and `PositionInput` in
//! `ListFormat.swift`): 960 units per beat as the transport shows them,
//! converted by the core's meter map (`tick_to_bbu`, `bbu_to_tick`).

use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::time::{BarBeatUnit, Tick};
use vizdaw_ffi::events::MeterEntryRow;

/// Display units per beat.
pub const UNITS_PER_BEAT: u32 = 960;

/// A position: bar (0 is the pickup bar, negative before it), beat from 1,
/// unit from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bbu {
    pub bar: i64,
    pub beat: i64,
    pub unit: i64,
}

/// The bars, beats and units a typed position may name.
const BARS: std::ops::RangeInclusive<i64> = -1_000_000..=1_000_000;
const BEATS: std::ops::RangeInclusive<i64> = 1..=999;
const UNITS: std::ops::RangeInclusive<i64> = 0..=959;

/// `meter` as the core's map; an empty or damaged list is 4/4 from bar 1.
pub fn meter_map(meter: &[MeterEntryRow]) -> MeterMap {
    let entries = meter
        .iter()
        .map(|m| MeterEntry {
            tick: Tick(m.tick),
            meter: Meter {
                num: m.num,
                den: m.den,
            },
            key: KeySig {
                sharps: m.sharps,
                minor: m.minor,
            },
        })
        .collect();
    MeterMap::new(entries).unwrap_or_default()
}

/// One bar's length at `tick`.
pub fn bar_ticks(tick: i64, meter: &[MeterEntryRow]) -> i64 {
    meter_map(meter).entry_at(Tick(tick)).meter.bar_ticks()
}

impl Bbu {
    /// The position of `tick`.
    pub fn of(tick: i64, meter: &[MeterEntryRow]) -> Self {
        let p = meter_map(meter).tick_to_bbu(Tick(tick), UNITS_PER_BEAT);
        Bbu {
            bar: p.bar.into(),
            beat: p.beat.into(),
            unit: p.unit.into(),
        }
    }

    /// The first tick of this position.
    pub fn tick(&self, meter: &[MeterEntryRow]) -> i64 {
        let p = BarBeatUnit {
            bar: i32::try_from(self.bar).unwrap_or(if self.bar < 0 { i32::MIN } else { i32::MAX }),
            beat: u16::try_from(self.beat.max(1)).unwrap_or(u16::MAX),
            unit: u32::try_from(self.unit.max(0)).unwrap_or(u32::MAX),
        };
        meter_map(meter).bbu_to_tick(p, UNITS_PER_BEAT).0
    }

    pub fn text(&self) -> String {
        format!("{}.{}.{}", self.bar, self.beat, self.unit)
    }
}

/// Typed positions: "5" is bar 5's downbeat, "5.3" and "5.3.480" name
/// more; an empty part keeps that part of `current` (".3" the beat only,
/// "..480" the unit only, "5.." the bar only).
pub fn parse_position(text: &str, current: Bbu) -> Option<Bbu> {
    let s = text.trim();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 3 {
        return None;
    }
    let sub_field = parts[0].is_empty();
    let mut out = if sub_field {
        current
    } else {
        Bbu {
            bar: current.bar,
            beat: 1,
            unit: 0,
        }
    };
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() {
            if !sub_field && i == 1 {
                out.beat = current.beat;
            }
            if !sub_field && i == 2 {
                out.unit = current.unit;
            }
            continue;
        }
        // Swift's `Int64(_:)`: an optional sign and digits only.
        let digits = p.strip_prefix(['+', '-']).unwrap_or(p);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let v: i64 = p.parse().ok()?;
        match i {
            0 => out.bar = v,
            1 => out.beat = v,
            _ => out.unit = v,
        }
    }
    (BARS.contains(&out.bar) && BEATS.contains(&out.beat) && UNITS.contains(&out.unit))
        .then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: i64 = 61_440;
    const BEAT: i64 = 15_360;

    fn four_four() -> Vec<MeterEntryRow> {
        vec![]
    }

    #[test]
    fn positions_read_and_write_as_the_core_does() {
        let m = four_four();
        assert_eq!(Bbu::of(BAR + BEAT, &m).text(), "2.2.0");
        assert_eq!(Bbu::of(-BAR, &m).text(), "0.1.0");
        let p = Bbu {
            bar: 4,
            beat: 2,
            unit: 480,
        };
        assert_eq!(p.tick(&m), 3 * BAR + BEAT + BEAT / 2);
    }

    #[test]
    fn an_empty_part_keeps_the_current_one() {
        let cur = Bbu {
            bar: 4,
            beat: 2,
            unit: 480,
        };
        let p = |t| parse_position(t, cur);
        assert_eq!(
            p("3"),
            Some(Bbu {
                bar: 3,
                beat: 1,
                unit: 0
            })
        );
        assert_eq!(
            p(".3"),
            Some(Bbu {
                bar: 4,
                beat: 3,
                unit: 480
            })
        );
        assert_eq!(
            p("5.."),
            Some(Bbu {
                bar: 5,
                beat: 2,
                unit: 480
            })
        );
        assert_eq!(p("x.y"), None);
        assert_eq!(p(""), None);
        assert_eq!(p("1.2.3.4"), None);
        assert_eq!(p("1.0"), None);
        assert_eq!(p("1.1.960"), None);
        assert_eq!(p("99999999999999999999"), None);
    }
}
