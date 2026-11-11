//! MIDI input effects (spec §4.5, §6.8; plan 5g-1 Rulings 12 and 14): the
//! chain of up to eight built-in effects a MIDI or instrument lane runs on
//! live input. This module is the model only: shapes, defaults and range
//! checks. The real-time chain lives in `core-engine`.

use crate::error::{ModelError, Result};
use crate::time::{PPQ, Tick};
use serde::{Deserialize, Serialize};

/// The most effects a lane's chain holds (spec §4.5, §6.8).
pub const MAX_INPUT_FX: usize = 8;
/// The most notes a Chord adds (spec §6.8).
pub const MAX_CHORD_NOTES: usize = 6;
/// The largest transpose or chord interval, in semitones.
pub const MAX_SHIFT: i8 = 48;

fn is_false(b: &bool) -> bool {
    !*b
}

/// One effect in a lane's chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputFx {
    /// What the effect is and its settings.
    pub kind: InputFxKind,
    /// A bypassed effect passes input through untouched.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
}

/// The five built-in effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum InputFxKind {
    /// Arpeggiator.
    Arpeggiator(Arp),
    /// Transpose by whole semitones.
    Transpose {
        /// Semitones, at most [`MAX_SHIFT`] either way.
        semitones: i8,
    },
    /// Velocity scale, offset, limits or fixed value.
    Velocity(VelocityFx),
    /// Adds notes at the given intervals above or below each note.
    Chord {
        /// Distinct non-zero semitone intervals, at most [`MAX_CHORD_NOTES`].
        intervals: Vec<i8>,
    },
    /// Quantises recorded note starts (never thru).
    InputQuantize {
        /// One of [`input_quantize_grids`].
        grid: Tick,
        /// Percent, 0–100.
        strength: u8,
    },
}

/// The arpeggiator's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arp {
    /// Playing order.
    pub mode: ArpMode,
    /// Step length.
    pub rate: NoteValue,
    /// Octaves spanned, 1–4.
    pub octaves: u8,
    /// Gate in percent of a step, 5–200.
    pub gate: u16,
    /// None: As Played; Some: Fixed 1–127.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<u8>,
    /// Keep the held set after the keys are released.
    #[serde(default, skip_serializing_if = "is_false")]
    pub latch: bool,
}

/// The arpeggiator's playing orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArpMode {
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

/// A note value: a division of the whole note and a feel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteValue {
    /// 1, 2, 4, 8, 16, 32 or 64.
    pub division: u8,
    /// Straight, dotted or triplet.
    #[serde(default, skip_serializing_if = "Feel::is_straight")]
    pub feel: Feel,
}

/// A note value's feel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feel {
    /// As written.
    #[default]
    Straight,
    /// Times 3/2.
    Dotted,
    /// Times 2/3.
    Triplet,
}

/// The Velocity effect's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VelocityFx {
    /// Percent, 0–400.
    pub scale: u16,
    /// 7-bit steps added after scaling, -127–127.
    pub offset: i16,
    /// Lowest result, 1–127.
    pub min: u8,
    /// Highest result, `min`–127.
    pub max: u8,
    /// Some: every note gets this velocity, 1–127.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<u8>,
}

impl Default for Arp {
    fn default() -> Self {
        Arp {
            mode: ArpMode::Up,
            rate: NoteValue::SIXTEENTH,
            octaves: 1,
            gate: 50,
            velocity: None,
            latch: false,
        }
    }
}

impl Default for VelocityFx {
    fn default() -> Self {
        VelocityFx {
            scale: 100,
            offset: 0,
            min: 1,
            max: 127,
            fixed: None,
        }
    }
}

impl Feel {
    /// True for [`Feel::Straight`] (the serde skip test).
    pub fn is_straight(&self) -> bool {
        matches!(self, Feel::Straight)
    }
}

impl NoteValue {
    /// 1/16 straight.
    pub const SIXTEENTH: NoteValue = NoteValue {
        division: 16,
        feel: Feel::Straight,
    };

    /// `4 × PPQ / division`, × 3/2 dotted, × 2/3 triplet (Ruling 10). A
    /// division of 0 gives 0 ticks (it fails [`InputFxKind::check`]).
    pub fn ticks(self) -> Tick {
        let base = (4 * PPQ).checked_div(i64::from(self.division)).unwrap_or(0);
        Tick(match self.feel {
            Feel::Straight => base,
            Feel::Dotted => base * 3 / 2,
            Feel::Triplet => base * 2 / 3,
        })
    }
}

impl InputFx {
    /// An effect that is not bypassed.
    pub fn new(kind: InputFxKind) -> InputFx {
        InputFx {
            kind,
            bypass: false,
        }
    }
}

impl InputFxKind {
    /// "Arpeggiator", "Transpose", "Velocity", "Chord", "Input Quantize".
    pub fn name(&self) -> &'static str {
        match self {
            InputFxKind::Arpeggiator(_) => "Arpeggiator",
            InputFxKind::Transpose { .. } => "Transpose",
            InputFxKind::Velocity(_) => "Velocity",
            InputFxKind::Chord { .. } => "Chord",
            InputFxKind::InputQuantize { .. } => "Input Quantize",
        }
    }

    /// Range checks (Ruling 14; spec §6.8's ranges).
    pub fn check(&self) -> Result<()> {
        match self {
            InputFxKind::Arpeggiator(a) => {
                if !matches!(a.rate.division, 1 | 2 | 4 | 8 | 16 | 32 | 64) {
                    return Err(ModelError::range("arpeggiator.rate", a.rate.division));
                }
                if !(1..=4).contains(&a.octaves) {
                    return Err(ModelError::range("arpeggiator.octaves", a.octaves));
                }
                if !(5..=200).contains(&a.gate) {
                    return Err(ModelError::range("arpeggiator.gate", a.gate));
                }
                if let Some(v) = a.velocity
                    && !(1..=127).contains(&v)
                {
                    return Err(ModelError::range("arpeggiator.velocity", v));
                }
            }
            InputFxKind::Transpose { semitones } => {
                if semitones.unsigned_abs() > MAX_SHIFT.unsigned_abs() {
                    return Err(ModelError::range("transpose.semitones", semitones));
                }
            }
            InputFxKind::Velocity(v) => {
                if v.scale > 400 {
                    return Err(ModelError::range("velocity.scale", v.scale));
                }
                if v.offset.unsigned_abs() > 127 {
                    return Err(ModelError::range("velocity.offset", v.offset));
                }
                if !(1..=127).contains(&v.min) {
                    return Err(ModelError::range("velocity.min", v.min));
                }
                if !(1..=127).contains(&v.max) || v.max < v.min {
                    return Err(ModelError::range("velocity.max", v.max));
                }
                if let Some(f) = v.fixed
                    && !(1..=127).contains(&f)
                {
                    return Err(ModelError::range("velocity.fixed", f));
                }
            }
            InputFxKind::Chord { intervals } => {
                let bad = intervals.is_empty()
                    || intervals.len() > MAX_CHORD_NOTES
                    || intervals
                        .iter()
                        .any(|i| *i == 0 || i.unsigned_abs() > MAX_SHIFT.unsigned_abs())
                    || intervals
                        .iter()
                        .enumerate()
                        .any(|(n, i)| intervals.iter().skip(n + 1).any(|j| j == i));
                if bad {
                    return Err(ModelError::range(
                        "chord.intervals",
                        format!("{intervals:?}"),
                    ));
                }
            }
            InputFxKind::InputQuantize { grid, strength } => {
                if !input_quantize_grids().contains(grid) {
                    return Err(ModelError::range("input_quantize.grid", grid.0));
                }
                if *strength > 100 {
                    return Err(ModelError::range("input_quantize.strength", strength));
                }
            }
        }
        Ok(())
    }
}

/// The 15 Input Quantize grids (Ruling 12): for s in 2q, q, q/2, q/4, q/8
/// (q = PPQ): s, s × 3/2, s × 2/3, in that order.
pub fn input_quantize_grids() -> [Tick; 15] {
    let mut out = [Tick(0); 15];
    let bases = [2 * PPQ, PPQ, PPQ / 2, PPQ / 4, PPQ / 8];
    for (n, s) in bases.iter().enumerate() {
        for (k, t) in [*s, s * 3 / 2, s * 2 / 3].into_iter().enumerate() {
            if let Some(slot) = out.get_mut(n * 3 + k) {
                *slot = Tick(t);
            }
        }
    }
    out
}

/// Every effect's check, at most [`MAX_INPUT_FX`], at most one Input
/// Quantize.
pub fn check_chain<'a>(chain: impl IntoIterator<Item = &'a InputFx>) -> Result<()> {
    let mut n = 0usize;
    let mut quantizers = 0usize;
    for fx in chain {
        n += 1;
        if n > MAX_INPUT_FX {
            return Err(ModelError::range("input_fx.len", n));
        }
        fx.kind.check()?;
        if matches!(fx.kind, InputFxKind::InputQuantize { .. }) {
            quantizers += 1;
        }
    }
    if quantizers > 1 {
        return Err(ModelError::invalid("a chain holds one Input Quantize"));
    }
    Ok(())
}
