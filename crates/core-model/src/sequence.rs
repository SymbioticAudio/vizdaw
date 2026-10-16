//! Sequences: the main unit of work.

use crate::ids::{LaneId, SeqId};
use crate::lane::Lane;
use crate::meter::MeterMap;
use crate::tempo::TempoMap;
use crate::time::Tick;
use im::{OrdMap, Vector};
use serde::{Deserialize, Serialize};

/// A sequence with its own lanes, tempo and meter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    /// Sequence ID.
    pub id: SeqId,
    /// Display name.
    pub name: String,
    /// Normal or generated.
    pub kind: SeqKind,
    /// Optional key chord that auditions or inserts this sequence.
    pub keyboard_slot: Option<KeySlot>,
    /// Tempo map (used when played or viewed on its own).
    pub tempo: TempoMap,
    /// Meter map (used when played or viewed on its own).
    pub meter: MeterMap,
    /// Named positions.
    pub markers: Vector<Marker>,
    /// Free-text comment.
    pub comment: String,
    /// Pickup length before bar 1 (>= 0). Content plays from `-start_offset`.
    pub start_offset: Tick,
    /// Derived from the lanes or locked.
    pub length: SeqLength,
    /// Lanes in display order.
    pub lanes: Vector<Lane>,
}

impl Sequence {
    /// A normal sequence at 120 BPM in 4/4 with the given lanes.
    pub fn new(id: SeqId, name: impl Into<String>, lanes: Vector<Lane>) -> Sequence {
        Sequence {
            id,
            name: name.into(),
            kind: SeqKind::Normal,
            keyboard_slot: None,
            tempo: TempoMap::default(),
            meter: MeterMap::default(),
            markers: Vector::new(),
            comment: String::new(),
            start_offset: Tick::ZERO,
            length: SeqLength::Derived,
            lanes,
        }
    }

    /// A lane by ID.
    pub fn lane(&self, id: LaneId) -> Option<&Lane> {
        self.lanes.iter().find(|l| l.id == id)
    }

    /// Index of a lane by ID.
    pub fn lane_index(&self, id: LaneId) -> Option<usize> {
        self.lanes.iter().position(|l| l.id == id)
    }
}

/// Sequence kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeqKind {
    /// Holds ordinary event data.
    Normal,
    /// Produces events from rules; parameters are stored only (2a).
    Generated {
        /// The generator parameters.
        generator: GeneratorParams,
    },
}

/// Opaque generator parameters, stored and round-tripped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorParams {
    /// Generator name.
    pub generator: String,
    /// Parameter values as strings.
    pub params: OrdMap<String, String>,
}

/// A keyboard slot: one key plus a modifier bitmask
/// (1 = shift, 2 = control, 4 = option, 8 = command).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeySlot {
    /// The key, e.g. "a" or "F5".
    pub key: String,
    /// Modifier bitmask.
    pub modifiers: u8,
}

/// A named position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    /// Position.
    pub tick: Tick,
    /// Name.
    pub name: String,
}

/// How a sequence's length is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeqLength {
    /// Follows the longest lane, rounded up to a barline (never stored).
    Derived,
    /// Fixed length in ticks (> 0).
    Locked(Tick),
}
