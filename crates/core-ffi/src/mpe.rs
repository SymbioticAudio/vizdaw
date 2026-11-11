//! MPE settings over the FFI (plan 5g-2 Rulings 3, 7): a track's MPE
//! input and a destination's MPE output, as one zone of member channels.
//! The model checks the ranges ([`MpeConfig::check`]).

use core_model::mpe::{MpeConfig, MpeZone};

/// Which end of the channel range the zone's manager sits at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MpeZoneRow {
    /// Manager on channel 1, members counting up from channel 2.
    Lower,
    /// Manager on channel 16, members counting down from channel 15.
    Upper,
}

/// One MPE zone: its side, member channels (1–15) and member pitch-bend
/// range in semitones (1–96).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct MpeRow {
    /// The zone's side.
    pub zone: MpeZoneRow,
    /// Member channels, 1–15.
    pub members: u8,
    /// Member pitch-bend range in semitones, 1–96.
    pub bend_range: u8,
}

impl From<MpeConfig> for MpeRow {
    fn from(c: MpeConfig) -> Self {
        MpeRow {
            zone: match c.zone {
                MpeZone::Lower => MpeZoneRow::Lower,
                MpeZone::Upper => MpeZoneRow::Upper,
            },
            members: c.members,
            bend_range: c.bend_range,
        }
    }
}

impl From<MpeRow> for MpeConfig {
    fn from(r: MpeRow) -> Self {
        MpeConfig {
            zone: match r.zone {
                MpeZoneRow::Lower => MpeZone::Lower,
                MpeZoneRow::Upper => MpeZone::Upper,
            },
            members: r.members,
            bend_range: r.bend_range,
        }
    }
}
