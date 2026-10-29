//! Automation targets and how a normalised value drives each (editors
//! spec §4d; Plan 4d Rulings 2–5). Values are `f64` in `0.0..=1.0`.

use crate::document::{Document, SlotOwner};
use crate::ids::{DestId, PluginInstanceId, StripId};
use crate::mixer::{GAIN_FLOOR_DB, GAIN_MAX_DB, ParamId, StripKind};
use serde::{Deserialize, Serialize};

/// A send's position in its strip's send list (Plan 4d Ruling 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SendIndex(pub u16);

/// What an automation event drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTarget {
    /// A strip's fader.
    StripFader(StripId),
    /// A strip's pan (ignored on master).
    StripPan(StripId),
    /// A strip's mute (on at 0.5 and above).
    StripMute(StripId),
    /// A strip's solo (on at 0.5 and above).
    StripSolo(StripId),
    /// The level of one of a strip's sends.
    SendLevel(StripId, SendIndex),
    /// A plugin parameter, by its stable ID.
    PluginParam(PluginInstanceId, ParamId),
    /// A destination's velocity scale (Plan 4d Ruling 2), applied to note
    /// velocities when the sequence is compiled.
    VelocityScale(DestId),
}

impl AutomationTarget {
    /// Mute and solo hold each value until the next point; every other
    /// target ramps (a parameter the plugin declares stepped is the
    /// engine's to add, Plan 4d Ruling 8).
    pub fn is_stepped(self) -> bool {
        matches!(
            self,
            AutomationTarget::StripMute(_) | AutomationTarget::StripSolo(_)
        )
    }

    /// A short description for messages ("strip3 fader").
    pub fn describe(self) -> String {
        match self {
            AutomationTarget::StripFader(s) => format!("{s} fader"),
            AutomationTarget::StripPan(s) => format!("{s} pan"),
            AutomationTarget::StripMute(s) => format!("{s} mute"),
            AutomationTarget::StripSolo(s) => format!("{s} solo"),
            AutomationTarget::SendLevel(s, i) => {
                format!("{s} send {} level", u32::from(i.0) + 1)
            }
            AutomationTarget::PluginParam(p, id) => format!("{p} parameter {}", id.0),
            AutomationTarget::VelocityScale(d) => format!("{d} velocity scale"),
        }
    }
}

/// A strip's automation mode (sub-project 5 spec §4.6: "`Off` plays the
/// static values and ignores automation. `Read` plays automation").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationMode {
    /// Plays the static values and ignores automation.
    Off,
    /// Plays automation.
    #[default]
    Read,
    /// Writes from touch to release, then returns to the automation.
    Touch,
    /// Writes from touch until Stop or the next loop wrap.
    Latch,
    /// Writes every target from play start to Stop or the next wrap.
    Write,
    /// Adds the touch's offset to the existing automation.
    Trim,
}

impl AutomationMode {
    /// Every mode, in menu order.
    pub const ALL: [AutomationMode; 6] = [
        AutomationMode::Off,
        AutomationMode::Read,
        AutomationMode::Touch,
        AutomationMode::Latch,
        AutomationMode::Write,
        AutomationMode::Trim,
    ];

    /// True for Read, the mode that is not saved (plan 5e-1 Ruling 2).
    pub fn is_read(&self) -> bool {
        *self == AutomationMode::Read
    }

    /// True for the modes that write automation (Touch, Latch, Write,
    /// Trim).
    pub fn writes(self) -> bool {
        matches!(
            self,
            AutomationMode::Touch
                | AutomationMode::Latch
                | AutomationMode::Write
                | AutomationMode::Trim
        )
    }

    /// The mode's name as menus show it ("Off" … "Trim").
    pub fn name(self) -> &'static str {
        match self {
            AutomationMode::Off => "Off",
            AutomationMode::Read => "Read",
            AutomationMode::Touch => "Touch",
            AutomationMode::Latch => "Latch",
            AutomationMode::Write => "Write",
            AutomationMode::Trim => "Trim",
        }
    }
}

/// The strip whose automation mode governs `t` (plan 5e-1 Ruling 3): a
/// strip's own controls and sends, an insert's strip, a plugin
/// instrument's Instrument strip; `None` for velocity scale or a target
/// whose strip or slot does not exist.
pub fn strip_of(doc: &Document, t: AutomationTarget) -> Option<StripId> {
    match t {
        AutomationTarget::StripFader(s)
        | AutomationTarget::StripPan(s)
        | AutomationTarget::StripMute(s)
        | AutomationTarget::StripSolo(s)
        | AutomationTarget::SendLevel(s, _) => doc.mixer().strip(s).ok().map(|x| x.id),
        AutomationTarget::PluginParam(p, _) => match doc.find_slot(p)? {
            SlotOwner::Insert(s, _) => Some(s),
            SlotOwner::Instrument(dest) => doc.mixer().find_kind(StripKind::Instrument(dest)),
        },
        AutomationTarget::VelocityScale(_) => None,
    }
}

/// Whether an automation target's strip, send, plugin or destination
/// exists.
pub fn target_exists(doc: &Document, t: AutomationTarget) -> bool {
    let m = doc.mixer();
    match t {
        AutomationTarget::StripFader(s)
        | AutomationTarget::StripPan(s)
        | AutomationTarget::StripMute(s)
        | AutomationTarget::StripSolo(s) => m.strip(s).is_ok(),
        AutomationTarget::SendLevel(s, i) => {
            m.strip(s).is_ok_and(|x| usize::from(i.0) < x.sends.len())
        }
        AutomationTarget::PluginParam(p, _) => doc.plugin_slot(p).is_some(),
        AutomationTarget::VelocityScale(d) => doc.instruments().contains_key(&d),
    }
}

/// The 32-bit scale controllers use, `2³² − 1` (Plan 4d Ruling 18).
pub const FULL_SCALE: f64 = u32::MAX as f64;

/// `v` as a usable normalised value: NaN is 0, the rest clamped to 0–1.
pub fn unit(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// True for a finite value in 0–1, what an automation event may hold
/// (Plan 4d Ruling 3).
pub fn check_value(v: f64) -> bool {
    v.is_finite() && (0.0..=1.0).contains(&v)
}

/// A normalised value on the 32-bit scale.
pub fn to_u32(v: f64) -> u32 {
    // Float-to-int `as` saturates.
    (unit(v) * FULL_SCALE).round() as u32
}

/// A 32-bit scale value as a normalised value.
pub fn from_u32(x: u32) -> f64 {
    f64::from(x) / FULL_SCALE
}

/// The fader or send level of normalised `v`: `12 + 60·log10(v)` dB, −∞
/// (`GAIN_FLOOR_DB`) once that reaches the floor; unity at v ≈ 0.631
/// (Plan 4d Ruling 4).
pub fn fader_db(v: f64) -> f64 {
    let v = unit(v);
    if v <= 0.0 {
        return GAIN_FLOOR_DB;
    }
    let db = GAIN_MAX_DB + 60.0 * v.log10();
    if db <= GAIN_FLOOR_DB {
        GAIN_FLOOR_DB
    } else {
        db
    }
}

/// The normalised value of a fader or send level (inverse of
/// [`fader_db`]).
pub fn fader_norm(db: f64) -> f64 {
    if db.is_nan() || db <= GAIN_FLOOR_DB {
        return 0.0;
    }
    10f64.powf((db.min(GAIN_MAX_DB) - GAIN_MAX_DB) / 60.0)
}

/// Pan −1 … +1 of normalised `v` (0.5 is the centre).
pub fn pan_of(v: f64) -> f64 {
    unit(v) * 2.0 - 1.0
}

/// The normalised value of pan `p`.
pub fn pan_norm(p: f64) -> f64 {
    if p.is_nan() {
        0.5
    } else {
        (p.clamp(-1.0, 1.0) + 1.0) / 2.0
    }
}

/// A switch (mute, solo) is on at 0.5 and above.
pub fn switch_on(v: f64) -> bool {
    unit(v) >= 0.5
}

/// The velocity-scale percentage of normalised `v`: 1–100 % over 0–0.5,
/// 100–250 % over 0.5–1.
pub fn velocity_scale_percent(v: f64) -> f64 {
    let v = unit(v);
    if v <= 0.5 {
        1.0 + 198.0 * v
    } else {
        100.0 + 300.0 * (v - 0.5)
    }
}

/// A plugin parameter's plain value of normalised `v` in `min..=max`,
/// rounded when the parameter is stepped.
pub fn param_plain(v: f64, min: f64, max: f64, stepped: bool) -> f64 {
    let p = min + unit(v) * (max - min);
    if stepped { p.round() } else { p }
}
