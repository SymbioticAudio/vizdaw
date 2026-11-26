//! The shared edit grid (Swift `Snap.swift`): one setting for every editor,
//! toggled by one key. The grid is counted from the start of the bar a
//! tick is in, so a bar line always snaps.

use serde::{Deserialize, Serialize};
use vizdaw_ffi::events::MeterEntryRow;

use crate::arrangement::{PPQ, bar_at, bar_length};

/// The grid's note value. Written snake_case in `ui.json` and on the
/// control channel; the camel-case `thirtySecond` older builds wrote still
/// reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Division {
    Bar,
    Half,
    #[default]
    Quarter,
    Eighth,
    Sixteenth,
    #[serde(alias = "thirtySecond")]
    ThirtySecond,
}

impl Division {
    pub const ALL: [Division; 6] = [
        Division::Bar,
        Division::Half,
        Division::Quarter,
        Division::Eighth,
        Division::Sixteenth,
        Division::ThirtySecond,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Division::Bar => "Bar",
            Division::Half => "1/2",
            Division::Quarter => "1/4",
            Division::Eighth => "1/8",
            Division::Sixteenth => "1/16",
            Division::ThirtySecond => "1/32",
        }
    }

    /// The raw value (`ui.json`, the control channel).
    pub fn name(self) -> &'static str {
        match self {
            Division::Bar => "bar",
            Division::Half => "half",
            Division::Quarter => "quarter",
            Division::Eighth => "eighth",
            Division::Sixteenth => "sixteenth",
            Division::ThirtySecond => "thirty_second",
        }
    }
}

/// Snap to grid. Every field is required when decoded (as Swift's
/// synthesized decoding): a partial value falls back to the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snap {
    pub division: Division,
    pub triplet: bool,
    pub dotted: bool,
    pub enabled: bool,
}

impl Default for Snap {
    fn default() -> Self {
        Snap {
            division: Division::Quarter,
            triplet: false,
            dotted: false,
            enabled: true,
        }
    }
}

impl Snap {
    /// The grid step in ticks inside a bar of meter `m`.
    pub fn step(&self, m: &MeterEntryRow) -> i64 {
        let q = PPQ;
        let mut s = match self.division {
            Division::Bar => return bar_length(m),
            Division::Half => 2 * q,
            Division::Quarter => q,
            Division::Eighth => q / 2,
            Division::Sixteenth => q / 4,
            Division::ThirtySecond => q / 8,
        };
        if self.triplet {
            s = s * 2 / 3;
        }
        if self.dotted {
            s = s * 3 / 2;
        }
        s.max(1)
    }

    /// `tick` on the grid line at or before it, within its bar. Unchanged
    /// when snap is off.
    pub fn floor(&self, tick: i64, meter: &[MeterEntryRow]) -> i64 {
        if !self.enabled {
            return tick;
        }
        let bar = bar_at(tick, meter);
        let s = self.step(&bar.meter);
        bar.start + (tick - bar.start) / s * s
    }

    /// `tick` on the nearest grid line of its bar (ties go later); the
    /// bar's end counts as a grid line. Unchanged when snap is off.
    pub fn apply(&self, tick: i64, meter: &[MeterEntryRow]) -> i64 {
        if !self.enabled {
            return tick;
        }
        let bar = bar_at(tick, meter);
        let s = self.step(&bar.meter);
        let down = bar.start + (tick - bar.start) / s * s;
        let up = (down + s).min(bar.start + bar.length);
        if tick - down < up - tick { down } else { up }
    }
}
