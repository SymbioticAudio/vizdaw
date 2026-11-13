//! Merging a returning DAWproject into the project it came from (plan 6e,
//! spec §7.8).
//!
//! **The three sides** are projections ([`Projection`]) of one sequence:
//!
//! - **base**, the snapshot's (`Interchange/<export id>.json`): what was
//!   exported;
//! - **theirs**, the returning file imported into a scratch project of its
//!   own ([`theirs::theirs`]);
//! - **ours**, the live document now.
//!
//! Before any comparison all three are put in the file's frame (Ruling 1):
//! base and ours through [`as_returned`] (with the snapshot's origin and
//! region-gain option, [`normalised`]), theirs through
//! [`returned`](super::record::returned), which [`theirs::theirs`] applies.
//! [`matching`] then pairs the rows of the three sides.
//!
//! **The tolerances** (Ruling 4) within which two values are the same:
//! [`NOTE_TICKS`] for note starts and lengths, marker and region ticks and
//! automation points; [`FRAMES`] for region lengths; [`LEVEL_DB`] for
//! levels and send levels (−∞ equals anything below −120 dB); [`PAN`] for
//! pan; [`VALUE`] for normalised automation values.
//!
//! [`Projection`]: super::record::Projection

pub mod apply;
pub mod decide;
pub mod matching;
pub mod text;
pub mod theirs;

use super::export::RegionGain;
use super::record::{Projection, Snapshot, as_returned};

/// Note starts and lengths, marker and region ticks, and automation point
/// ticks: within 1 tick (at 15 360 PPQ).
pub const NOTE_TICKS: i64 = 1;
/// Region lengths: within 1 frame.
pub const FRAMES: u64 = 1;
/// Levels and send levels: within 0.05 dB.
pub const LEVEL_DB: f64 = 0.05;
/// Pan (−1 … +1): within 0.005.
pub const PAN: f64 = 0.005;
/// Normalised automation values: within 1e-4.
pub const VALUE: f64 = 1e-4;

/// The snapshot's region-gain option: "leave_out" is
/// [`RegionGain::LeaveOut`]; "apply", or no option (a format-1 snapshot,
/// Ruling 3), is [`RegionGain::Apply`].
pub fn region_gain(base: &Snapshot) -> RegionGain {
    match base.region_gain.as_deref() {
        Some("leave_out") => RegionGain::LeaveOut,
        _ => RegionGain::Apply,
    }
}

/// `p` (base's or ours' projection) in the file's frame (Ruling 1):
/// [`as_returned`] with `base`'s origin and [`region_gain`].
pub fn normalised(p: &Projection, base: &Snapshot) -> Projection {
    as_returned(p, base.origin, region_gain(base))
}
