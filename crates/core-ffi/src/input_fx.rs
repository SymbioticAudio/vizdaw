//! MIDI input effects over the FFI (plan 5g-1 Task 6; spec §4.5, §6.8,
//! §12.1): the rows the app edits a lane's chain with, and the JSON the
//! control channel spells it in (Ruling 14). Range checks belong to the
//! model's edit (`LaneCmd::SetInputFx`); the JSON reader is a parser only.

use crate::error::VizError;
use crate::guard::ffi_guard;
use core_model::input_fx::{Arp, ArpMode, Feel, InputFx, InputFxKind, NoteValue, VelocityFx};
use core_model::time::Tick;

/// The most bytes [`input_fx_from_json`] reads.
pub const MAX_CHAIN_JSON: usize = 65_536;

/// The arpeggiator's playing orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ArpModeRow {
    /// Lowest first.
    Up,
    /// Highest first.
    Down,
    /// Up then down without doubling the ends.
    UpDown,
    /// Down then up without doubling the ends.
    DownUp,
    /// Press order.
    AsPlayed,
    /// A seeded random pick each step.
    Random,
    /// All held notes at each step.
    Chord,
}

/// A note value's feel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FeelRow {
    /// As written.
    Straight,
    /// Times 3/2.
    Dotted,
    /// Times 2/3.
    Triplet,
}

/// One effect's kind and settings.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum InputFxKindRow {
    /// Arpeggiator.
    Arpeggiator {
        /// Playing order.
        mode: ArpModeRow,
        /// Rate: 1, 2, 4, 8, 16, 32 or 64.
        division: u8,
        /// Rate feel.
        feel: FeelRow,
        /// Octaves spanned, 1–4.
        octaves: u8,
        /// Gate in percent of a step, 5–200.
        gate: u16,
        /// None: As Played; Some: Fixed 1–127.
        velocity: Option<u8>,
        /// Keep the held set after the keys are released.
        latch: bool,
    },
    /// Transpose by whole semitones.
    Transpose {
        /// Semitones, at most 48 either way.
        semitones: i8,
    },
    /// Velocity scale, offset, limits or fixed value.
    Velocity {
        /// Percent, 0–400.
        scale: u16,
        /// 7-bit steps added after scaling, -127–127.
        offset: i16,
        /// Lowest result, 1–127.
        min: u8,
        /// Highest result, `min`–127.
        max: u8,
        /// Some: every note gets this velocity, 1–127.
        fixed: Option<u8>,
    },
    /// Adds notes at the given intervals.
    Chord {
        /// Distinct non-zero semitone intervals, at most 6.
        intervals: Vec<i8>,
    },
    /// Quantises recorded note starts (never thru).
    InputQuantize {
        /// A snap value in ticks (`input_quantize_grids`).
        grid: i64,
        /// Percent, 0–100.
        strength: u8,
    },
}

/// One effect in a lane's chain.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InputFxRow {
    /// What the effect is and its settings.
    pub kind: InputFxKindRow,
    /// A bypassed effect passes input through untouched.
    pub bypass: bool,
}

impl From<ArpMode> for ArpModeRow {
    fn from(m: ArpMode) -> Self {
        match m {
            ArpMode::Up => ArpModeRow::Up,
            ArpMode::Down => ArpModeRow::Down,
            ArpMode::UpDown => ArpModeRow::UpDown,
            ArpMode::DownUp => ArpModeRow::DownUp,
            ArpMode::AsPlayed => ArpModeRow::AsPlayed,
            ArpMode::Random => ArpModeRow::Random,
            ArpMode::Chord => ArpModeRow::Chord,
        }
    }
}

impl From<ArpModeRow> for ArpMode {
    fn from(m: ArpModeRow) -> Self {
        match m {
            ArpModeRow::Up => ArpMode::Up,
            ArpModeRow::Down => ArpMode::Down,
            ArpModeRow::UpDown => ArpMode::UpDown,
            ArpModeRow::DownUp => ArpMode::DownUp,
            ArpModeRow::AsPlayed => ArpMode::AsPlayed,
            ArpModeRow::Random => ArpMode::Random,
            ArpModeRow::Chord => ArpMode::Chord,
        }
    }
}

impl From<Feel> for FeelRow {
    fn from(f: Feel) -> Self {
        match f {
            Feel::Straight => FeelRow::Straight,
            Feel::Dotted => FeelRow::Dotted,
            Feel::Triplet => FeelRow::Triplet,
        }
    }
}

impl From<FeelRow> for Feel {
    fn from(f: FeelRow) -> Self {
        match f {
            FeelRow::Straight => Feel::Straight,
            FeelRow::Dotted => Feel::Dotted,
            FeelRow::Triplet => Feel::Triplet,
        }
    }
}

impl From<&InputFxKind> for InputFxKindRow {
    fn from(k: &InputFxKind) -> Self {
        match k {
            InputFxKind::Arpeggiator(a) => InputFxKindRow::Arpeggiator {
                mode: a.mode.into(),
                division: a.rate.division,
                feel: a.rate.feel.into(),
                octaves: a.octaves,
                gate: a.gate,
                velocity: a.velocity,
                latch: a.latch,
            },
            InputFxKind::Transpose { semitones } => InputFxKindRow::Transpose {
                semitones: *semitones,
            },
            InputFxKind::Velocity(v) => InputFxKindRow::Velocity {
                scale: v.scale,
                offset: v.offset,
                min: v.min,
                max: v.max,
                fixed: v.fixed,
            },
            InputFxKind::Chord { intervals } => InputFxKindRow::Chord {
                intervals: intervals.clone(),
            },
            InputFxKind::InputQuantize { grid, strength } => InputFxKindRow::InputQuantize {
                grid: grid.0,
                strength: *strength,
            },
        }
    }
}

impl From<&InputFxKindRow> for InputFxKind {
    fn from(k: &InputFxKindRow) -> Self {
        match k {
            InputFxKindRow::Arpeggiator {
                mode,
                division,
                feel,
                octaves,
                gate,
                velocity,
                latch,
            } => InputFxKind::Arpeggiator(Arp {
                mode: (*mode).into(),
                rate: NoteValue {
                    division: *division,
                    feel: (*feel).into(),
                },
                octaves: *octaves,
                gate: *gate,
                velocity: *velocity,
                latch: *latch,
            }),
            InputFxKindRow::Transpose { semitones } => InputFxKind::Transpose {
                semitones: *semitones,
            },
            InputFxKindRow::Velocity {
                scale,
                offset,
                min,
                max,
                fixed,
            } => InputFxKind::Velocity(VelocityFx {
                scale: *scale,
                offset: *offset,
                min: *min,
                max: *max,
                fixed: *fixed,
            }),
            InputFxKindRow::Chord { intervals } => InputFxKind::Chord {
                intervals: intervals.clone(),
            },
            InputFxKindRow::InputQuantize { grid, strength } => InputFxKind::InputQuantize {
                grid: Tick(*grid),
                strength: *strength,
            },
        }
    }
}

impl From<&InputFx> for InputFxRow {
    fn from(f: &InputFx) -> Self {
        InputFxRow {
            kind: (&f.kind).into(),
            bypass: f.bypass,
        }
    }
}

impl From<&InputFxRow> for InputFx {
    fn from(r: &InputFxRow) -> Self {
        InputFx {
            kind: (&r.kind).into(),
            bypass: r.bypass,
        }
    }
}

/// The chain in `text`, as `lane.set_input_fx`'s `chain` and `state.get`'s
/// per-lane `input_fx` spell it (Ruling 14). `Model` for anything serde
/// refuses or text over [`MAX_CHAIN_JSON`] bytes; the ranges are checked
/// when the chain is set on a lane.
#[uniffi::export]
pub fn input_fx_from_json(text: String) -> Result<Vec<InputFxRow>, VizError> {
    ffi_guard(|| {
        if text.len() > MAX_CHAIN_JSON {
            return Err(VizError::Model {
                message: format!("the chain is over {MAX_CHAIN_JSON} bytes"),
            });
        }
        let chain: Vec<InputFx> = serde_json::from_str(&text).map_err(|e| VizError::Model {
            message: format!("not an input effect chain: {e}"),
        })?;
        Ok(chain.iter().map(Into::into).collect())
    })
}

/// The JSON value of `chain` (for `control.rs`): an array of `InputFx` as
/// the model serialises it.
pub fn input_fx_json(chain: &[InputFxRow]) -> serde_json::Value {
    let model: Vec<InputFx> = chain.iter().map(Into::into).collect();
    serde_json::to_value(model).unwrap_or(serde_json::Value::Array(Vec::new()))
}
