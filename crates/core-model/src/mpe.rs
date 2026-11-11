//! MPE settings (plan 5g-2 Rulings 2, 3): a lane decodes MPE input and a
//! destination encodes MPE output through one zone of member channels.
//! This module is the model only: shape, defaults, range checks and the
//! zone's channel arithmetic.

use crate::error::{ModelError, Result};
use serde::{Deserialize, Serialize};

/// The most member channels a zone holds.
pub const MAX_MEMBERS: u8 = 15;
/// The default pitch-bend range of member channels, in semitones.
pub const DEFAULT_BEND_RANGE: u8 = 48;
/// The largest pitch-bend range, in semitones.
pub const MAX_BEND_RANGE: u8 = 96;

/// Which end of the channel range a zone's manager sits at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MpeZone {
    /// Manager on channel 1 (index 0), members counting up from channel 2.
    Lower,
    /// Manager on channel 16 (index 15), members counting down from channel 15.
    Upper,
}

/// One MPE zone: its side, member-channel count and member bend range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MpeConfig {
    /// The zone's side.
    pub zone: MpeZone,
    /// Member channels, 1–[`MAX_MEMBERS`].
    pub members: u8,
    /// Member pitch-bend range in semitones, 1–[`MAX_BEND_RANGE`].
    pub bend_range: u8,
}

impl Default for MpeConfig {
    fn default() -> Self {
        MpeConfig {
            zone: MpeZone::Lower,
            members: MAX_MEMBERS,
            bend_range: DEFAULT_BEND_RANGE,
        }
    }
}

impl MpeConfig {
    /// Members 1–15 (`mpe.members`), bend range 1–96 (`mpe.bend_range`).
    pub fn check(&self) -> Result<()> {
        if !(1..=MAX_MEMBERS).contains(&self.members) {
            return Err(ModelError::range("mpe.members", self.members));
        }
        if !(1..=MAX_BEND_RANGE).contains(&self.bend_range) {
            return Err(ModelError::range("mpe.bend_range", self.bend_range));
        }
        Ok(())
    }

    /// The manager channel (0-based): 0 (lower) or 15 (upper).
    pub fn manager(&self) -> u8 {
        match self.zone {
            MpeZone::Lower => 0,
            MpeZone::Upper => 15,
        }
    }

    /// The `k`th member channel in rotation order: lower 1, 2, …, members;
    /// upper 14, 13, …, 15 − members. `None` past the last member.
    pub fn member(&self, k: u8) -> Option<u8> {
        if k >= self.members {
            return None;
        }
        match self.zone {
            MpeZone::Lower => k.checked_add(1),
            MpeZone::Upper => 14u8.checked_sub(k),
        }
    }

    /// Whether `ch` is one of this zone's member channels.
    pub fn is_member(&self, ch: u8) -> bool {
        match self.zone {
            MpeZone::Lower => (1..=self.members).contains(&ch),
            MpeZone::Upper => ch <= 14 && ch >= 15u8.saturating_sub(self.members),
        }
    }
}
