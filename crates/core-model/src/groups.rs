//! Fader groups (plan groups-vcas Rulings 1–4): a named, coloured set of
//! strips whose faders move together by the same dB, with linked mute and
//! solo switches. And VCAs (spec §2): a named, coloured fader whose level
//! is an offset on each member's heard level, with its own mute and solo.
//! Membership lives in the group or VCA, not on the strip. Edits are in
//! `crate::command::groups`.

use crate::automation::AutomationMode;
use crate::error::{ModelError, Result};
use crate::ids::{GroupId, StripId, VcaId};
use crate::mixer::{GAIN_FLOOR_DB, GAIN_MAX_DB, Mixer, Swatch};
use im::Vector;
use serde::{Deserialize, Serialize};

/// Most members a group (or VCA) holds (Ruling 4): bounds a gesture's
/// fan-out well inside the engine's live-gesture limit.
pub const MAX_MEMBERS: usize = 128;
/// Most VCAs a song holds (the controller's cap, 2026-11-18): each of a
/// strip's VCA links costs audio-thread time every block. A new VCA past
/// it is refused; a loaded file keeps the first, by ID, and says so.
pub const MAX_VCAS: usize = 64;
/// Longest group or VCA name, in characters, after trimming (Ruling 4).
pub const NAME_MAX: usize = 64;
/// The fader's lowest finite step (Ruling 2), as the mixer view draws it:
/// a group move down stops when any member would pass it.
pub const FADER_FLOOR_DB: f64 = -60.0;

/// A fader group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    /// Group ID.
    pub id: GroupId,
    /// Display name ([`check_name`]).
    pub name: String,
    /// Its colour, shown as a tag on each member.
    pub colour: Swatch,
    /// Members in the order they joined; never the master, and a strip is
    /// in at most one group (Ruling 3).
    pub members: Vector<StripId>,
    /// Muting one member mutes them all.
    pub link_mute: bool,
    /// Soloing one member solos them all.
    pub link_solo: bool,
    /// Co-Producer made it (owner, 2026-11-19): only then, and with only
    /// its own strips, may it change it without asking; absent in files
    /// when not, so a file without it is the composer's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub co_producer: bool,
}

/// A VCA (spec §2): its level, mute and solo act on its members' heard
/// sound without ever rewriting their strips. A strip may be in any number
/// of VCAs, and in a group as well.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vca {
    /// VCA ID.
    pub id: VcaId,
    /// Display name ([`check_name`]).
    pub name: String,
    /// Its colour.
    pub colour: Swatch,
    /// Members in the order they were assigned; never the master, each
    /// once.
    pub members: Vector<StripId>,
    /// Its level in dB, added to each member's own fader
    /// ([`vca_offset_db`]); at or below [`GAIN_FLOOR_DB`] it silences them.
    pub gain_db: f64,
    /// Mutes every member, leaving each strip's own switch as it is.
    pub mute: bool,
    /// Solos every member, leaving each strip's own switch as it is.
    pub solo: bool,
    /// Its fader's automation mode, one of [`VCA_MODES`] (Ruling 6); Read
    /// is not written.
    #[serde(default, skip_serializing_if = "AutomationMode::is_read")]
    pub automation: AutomationMode,
    /// Co-Producer made it (owner, 2026-11-19): only then, and with only
    /// its own strips, may it change it without asking; absent in files
    /// when not, so a file without it is the composer's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub co_producer: bool,
}

/// A VCA's automation modes (Ruling 6), in menu order.
pub const VCA_MODES: [AutomationMode; 4] = [
    AutomationMode::Off,
    AutomationMode::Read,
    AutomationMode::Touch,
    AutomationMode::Latch,
];

/// The VCAs `strip` is in, by VCA ID.
pub fn vcas_of(m: &Mixer, strip: StripId) -> impl Iterator<Item = &Vca> + '_ {
    m.vcas()
        .values()
        .filter(move |v| v.members.contains(&strip))
}

/// True for a level that sounds: above [`GAIN_FLOOR_DB`] (NaN does not).
fn sounds(db: f64) -> bool {
    db > GAIN_FLOOR_DB
}

/// Σ of `strip`'s VCAs' levels; GAIN_FLOOR_DB when any is at or below it; 0 for none.
///
/// Also [`GAIN_FLOOR_DB`] when the sum is at or below it. The top is not
/// capped here: [`heard_db`] caps the total.
pub fn vca_offset_db(m: &Mixer, strip: StripId) -> f64 {
    vca_sum(vcas_of(m, strip).map(|v| v.gain_db))
}

/// The offset of VCA levels `levels`, by [`vca_offset_db`]'s rule: Σ;
/// GAIN_FLOOR_DB when any level or the sum is at or below it; 0 for none.
///
/// The engine sums a strip's VCAs with it (live, bounce and render alike);
/// it neither allocates nor panics.
pub fn vca_sum(levels: impl IntoIterator<Item = f64>) -> f64 {
    let mut sum = 0.0;
    for db in levels {
        if !sounds(db) {
            return GAIN_FLOOR_DB;
        }
        sum += db;
    }
    if sounds(sum) { sum } else { GAIN_FLOOR_DB }
}

/// own + offset, clamped to GAIN_MAX_DB; GAIN_FLOOR_DB when either is at or below it or the sum is.
///
/// The level a strip is heard at (preflight F12: a group move measures
/// from the own fader, never from this). NaN anywhere gives the floor.
pub fn heard_db(own_db: f64, offset_db: f64) -> f64 {
    let sum = own_db + offset_db;
    if sounds(own_db) && sounds(offset_db) && sounds(sum) {
        sum.min(GAIN_MAX_DB)
    } else {
        GAIN_FLOOR_DB
    }
}

/// True when any of `strip`'s VCAs is muted.
pub fn vca_muted(m: &Mixer, strip: StripId) -> bool {
    vcas_of(m, strip).any(|v| v.mute)
}

/// True when any of `strip`'s VCAs is soloed.
pub fn vca_soloed(m: &Mixer, strip: StripId) -> bool {
    vcas_of(m, strip).any(|v| v.solo)
}

/// A group's link switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Link {
    /// Mute.
    Mute,
    /// Solo.
    Solo,
}

/// True for a level a group move measures from: finite and above
/// [`GAIN_FLOOR_DB`] (−∞). A member at −∞ takes no part in a move
/// (preflight F2).
fn moves(db: f64) -> bool {
    db.is_finite() && db > GAIN_FLOOR_DB
}

/// The clamped delta (Ruling 2): ≤ `GAIN_MAX_DB` − max finite base, ≥
/// min(0, `FADER_FLOOR_DB` − min finite base). A base is finite when it is
/// above `GAIN_FLOOR_DB`; bases at or below it (−∞), NaN and ±∞ count for
/// neither bound.
///
/// 0 for no finite base or a non-finite `wanted`, and 0 when no move fits:
/// the bounds cross, or the upper one is below 0 (both only happen with a
/// base above `GAIN_MAX_DB`, which a valid document never has; preflight
/// F3). So the result is always finite and between 0 and `wanted`.
pub fn group_delta(bases_db: &[f64], wanted: f64) -> f64 {
    if !wanted.is_finite() {
        return 0.0;
    }
    let mut finite = bases_db.iter().copied().filter(|b| moves(*b));
    let Some(first) = finite.next() else {
        return 0.0;
    };
    let (low, high) = finite.fold((first, first), |(lo, hi), b| (lo.min(b), hi.max(b)));
    let upper = GAIN_MAX_DB - high;
    let lower = (FADER_FLOOR_DB - low).min(0.0);
    // `lower` ≤ 0, so crossed bounds also have `upper` < 0.
    if upper < 0.0 {
        return 0.0;
    }
    wanted.clamp(lower, upper)
}

/// `strip`'s fader to `gain_db` with its group following, leader first; just `[(strip, gain_db)]` when
/// ungrouped, alone, or the leader is at −∞. Members at −∞ are left out.
///
/// Bases are each member's own fader in the document (preflight F12).
/// "Alone" means no other member would follow: the strip is the group's
/// only member, or every other member is at −∞. The others follow in the
/// group's member order; an unknown member is skipped.
pub fn group_gains(m: &Mixer, strip: StripId, gain_db: f64) -> Vec<(StripId, f64)> {
    let alone = vec![(strip, gain_db)];
    let Some(group) = group_of(m, strip) else {
        return alone;
    };
    let Some(base) = m.strip(strip).ok().map(|s| s.gain_db).filter(|b| moves(*b)) else {
        return alone;
    };
    let followers: Vec<(StripId, f64)> = group
        .members
        .iter()
        .filter(|id| **id != strip)
        .filter_map(|id| m.strip(*id).ok())
        .filter(|s| moves(s.gain_db))
        .map(|s| (s.id, s.gain_db))
        .collect();
    if followers.is_empty() {
        return alone;
    }
    let bases: Vec<f64> = std::iter::once(base)
        .chain(followers.iter().map(|(_, b)| *b))
        .collect();
    let delta = group_delta(&bases, gain_db - base);
    // Rounding never takes a member past the top, or a member that was
    // above the floor below it.
    let to = |b: f64| {
        let v = (b + delta).min(GAIN_MAX_DB);
        if b >= FADER_FLOOR_DB {
            v.max(FADER_FLOOR_DB)
        } else {
            v
        }
    };
    std::iter::once((strip, to(base)))
        .chain(followers.into_iter().map(|(id, b)| (id, to(b))))
        .collect()
}

/// `strip` and, when its group links `link`, the other members.
pub fn linked(m: &Mixer, strip: StripId, link: Link) -> Vec<StripId> {
    let mut out = vec![strip];
    if let Some(g) = group_of(m, strip) {
        let on = match link {
            Link::Mute => g.link_mute,
            Link::Solo => g.link_solo,
        };
        if on {
            out.extend(g.members.iter().copied().filter(|id| *id != strip));
        }
    }
    out
}

/// The group `strip` is in, if any.
pub fn group_of(m: &Mixer, strip: StripId) -> Option<&Group> {
    m.groups().values().find(|g| g.members.contains(&strip))
}

/// Trimmed, 1–NAME_MAX chars, no control chars; else ModelError::invalid naming the rule.
///
/// `what` names the thing in the message, as in "A group's name".
pub fn check_name(what: &str, name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().nth(NAME_MAX).is_some() {
        return Err(ModelError::invalid(format!(
            "{what} is 1 to {NAME_MAX} characters."
        )));
    }
    if n.chars().any(char::is_control) {
        return Err(ModelError::invalid(format!(
            "{what} cannot hold control characters."
        )));
    }
    Ok(n.to_string())
}
