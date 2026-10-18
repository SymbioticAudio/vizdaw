//! Lanes (untyped tracks), takes and drum layouts.

use crate::error::ModelError;
use crate::event::Event;
use crate::ids::{DestId, DrumRowId, EventId, LaneId, OutputPathId, TakeId};
use crate::time::Tick;
use im::{OrdMap, Vector};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A timeline lane holding MIDI events, audio regions or both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lane {
    /// Lane ID.
    pub id: LaneId,
    /// Display name.
    pub name: String,
    /// Default MIDI destination for the lane's events.
    pub midi_dest: Option<DestId>,
    /// Audio output path for the lane's regions.
    pub audio_out: Option<OutputPathId>,
    /// Lane mute.
    pub mute: bool,
    /// Lane solo (within its sequence).
    pub solo: bool,
    /// Record arm.
    pub arm: bool,
    /// Own length; playback of the lane stops there (events at or after
    /// it do not play, and longer notes and references are cut).
    pub length: Option<Tick>,
    /// Repeat `[0, length)` to fill the sequence.
    pub looped: bool,
    /// Recording may not change the length.
    pub length_locked: bool,
    /// Non-destructive play quantize for notes.
    pub play_quantize: Option<QuantizeSpec>,
    /// Non-destructive time shift.
    pub play_shift: Tick,
    /// Present when this is a drum lane.
    pub drum: Option<DrumLayout>,
    /// Alternate takes; never empty.
    pub takes: Vector<Take>,
    /// The take that plays.
    pub active_take: TakeId,
}

impl Lane {
    /// A new lane with one empty take.
    pub fn new(id: LaneId, take: TakeId, name: impl Into<String>) -> Lane {
        Lane {
            id,
            name: name.into(),
            midi_dest: None,
            audio_out: None,
            mute: false,
            solo: false,
            arm: false,
            length: None,
            looped: false,
            length_locked: false,
            play_quantize: None,
            play_shift: Tick::ZERO,
            drum: None,
            takes: Vector::unit(Take::new(take, "Take 1")),
            active_take: take,
        }
    }

    /// The take that plays, if it exists.
    pub fn active(&self) -> Option<&Take> {
        self.take(self.active_take)
    }

    /// A take by ID.
    pub fn take(&self, id: TakeId) -> Option<&Take> {
        self.takes.iter().find(|t| t.id == id)
    }

    /// Index of a take by ID.
    pub fn take_index(&self, id: TakeId) -> Option<usize> {
        self.takes.iter().position(|t| t.id == id)
    }
}

/// One version of a lane's content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Take {
    /// Take ID.
    pub id: TakeId,
    /// Display name.
    pub name: String,
    /// Events ordered by `(tick, id)`. Serialized as an array in that order.
    /// Mutate only through `Document` so the event index stays correct.
    #[serde(with = "events_as_array")]
    pub events: OrdMap<(Tick, EventId), Event>,
}

impl Take {
    /// An empty take.
    pub fn new(id: TakeId, name: impl Into<String>) -> Take {
        Take {
            id,
            name: name.into(),
            events: OrdMap::new(),
        }
    }
}

mod events_as_array {
    use super::*;

    pub fn serialize<S: Serializer>(
        m: &OrdMap<(Tick, EventId), Event>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.collect_seq(m.values())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<OrdMap<(Tick, EventId), Event>, D::Error> {
        let v: Vec<Event> = Vec::deserialize(d)?;
        let mut m = OrdMap::new();
        for e in v {
            if m.insert(e.key(), e.clone()).is_some() {
                return Err(serde::de::Error::custom(format!(
                    "duplicate event {}",
                    e.id
                )));
            }
        }
        Ok(m)
    }
}

/// The most offsets a play-quantize groove may have.
pub const MAX_GROOVE: usize = 64;

/// Non-destructive quantize: snap note starts toward a grid, or a groove
/// (a grid whose lines are moved by a repeating list of offsets).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantizeSpec {
    /// Grid size in ticks (> 0).
    pub grid: Tick,
    /// Strength 0–100 percent.
    pub strength: u8,
    /// Groove: grid line `k` (counted from tick 0) moves by
    /// `groove[k % len]` before the strength applies; each offset is
    /// shorter than the grid. Empty: a plain grid (and absent in files,
    /// so projects without a groove read and write as before).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groove: Vec<Tick>,
}

impl QuantizeSpec {
    /// A plain grid.
    pub fn grid(grid: Tick, strength: u8) -> QuantizeSpec {
        QuantizeSpec {
            grid,
            strength,
            groove: Vec::new(),
        }
    }

    /// Swing: every second line of `grid` moves late so that the pair
    /// splits `percent : 100 − percent` (50 is straight, 66 a triplet
    /// feel); `percent` is clamped to 50–75; full strength.
    pub fn swing(grid: Tick, percent: u8) -> QuantizeSpec {
        let p = i64::from(percent.clamp(50, 75));
        let late = grid.0.saturating_mul(2).saturating_mul(p - 50) / 100;
        QuantizeSpec {
            grid,
            strength: 100,
            groove: vec![Tick::ZERO, Tick(late)],
        }
    }

    /// Checks the grid (> 0, within ±2^40 ticks), the strength (≤ 100)
    /// and the groove (at most [`MAX_GROOVE`] offsets, each shorter than
    /// the grid).
    pub fn check(&self) -> crate::error::Result<()> {
        crate::rules::check_positive("play_quantize.grid", self.grid)?;
        crate::rules::check_ticks("play_quantize.grid", self.grid.0)?;
        if self.strength > 100 {
            return Err(ModelError::range("play_quantize.strength", self.strength));
        }
        if self.groove.len() > MAX_GROOVE {
            return Err(ModelError::range("play_quantize.groove", self.groove.len()));
        }
        if let Some(o) = self
            .groove
            .iter()
            .find(|o| o.0.unsigned_abs() >= self.grid.0.unsigned_abs())
        {
            return Err(ModelError::range("play_quantize.groove", o.0));
        }
        Ok(())
    }

    /// Applies the quantize to a tick.
    pub fn apply(&self, t: Tick) -> Tick {
        if self.grid.0 <= 0 {
            return t;
        }
        let g = self.grid.0;
        let k = t.0.saturating_add(g / 2).div_euclid(g);
        let mut target = k.saturating_mul(g);
        if !self.groove.is_empty() {
            let len = i64::try_from(self.groove.len()).unwrap_or(1);
            let i = usize::try_from(k.rem_euclid(len)).unwrap_or(0);
            target = target.saturating_add(self.groove.get(i).map_or(0, |o| o.0));
        }
        let delta =
            (i128::from(target) - i128::from(t.0)) * i128::from(self.strength.min(100)) / 100;
        Tick(t.0.saturating_add(i64::try_from(delta).unwrap_or(0)))
    }
}

/// The rows of a drum lane.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrumLayout {
    /// Rows in display order.
    pub rows: Vector<DrumRow>,
}

/// A drum row: a view over the lane's notes with this pitch and destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrumRow {
    /// Row ID.
    pub id: DrumRowId,
    /// Display name.
    pub name: String,
    /// Note number 0–127.
    pub note: u8,
    /// Destination the row's notes carry as their override (None = lane).
    pub dest: Option<DestId>,
    /// Show velocity in the drum grid.
    pub show_velocity: bool,
}

impl DrumRow {
    /// Whether an event with this pitch and destination override belongs to
    /// the row.
    pub fn matches(&self, pitch: u8, dest: Option<DestId>) -> bool {
        self.note == pitch && self.dest == dest
    }
}
