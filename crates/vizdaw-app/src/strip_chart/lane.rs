//! What a Strip Chart row shows, its names and its value text (Swift
//! `M/StripLane.swift`, `M/AutomationText.swift`; Plan 4d Rulings 3, 4, 8,
//! 18, 21, 26). Every law is the core's (`core_model::automation`); only
//! the text is here.

use std::collections::BTreeMap;

use core_model::automation::{
    fader_db, from_u32, pan_of, param_plain, switch_on, to_u32, unit, velocity_scale_percent,
};
use core_model::mixer::GAIN_FLOOR_DB;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use vizdaw_ffi::events::{AutomationTargetRow, EventKindRow, EventRow, TempoPointRow};
use vizdaw_ffi::modify::{ConditionRow, ConditionTest, RuleRow, ValueTargetRow};
use vizdaw_ffi::snapshot::ParamRow;
use vizdaw_ffi::strip::{LineTargetRow, SeriesRow};

use crate::arrangement::PPQ;
use crate::list::{Bend14, DurationText, PitchName, Value7, Velocity7};

/// Mute and solo hold each value until the next point (Plan 4d Ruling 8,
/// `StripLane.swift:43-49`).
pub fn is_switch(t: &AutomationTargetRow) -> bool {
    matches!(
        t,
        AutomationTargetRow::StripMute { .. } | AutomationTargetRow::StripSolo { .. }
    )
}

/// The tick after `t`, for an inclusive end as an exclusive one; the last
/// tick stays (no overflow, `StripLane.swift:101`).
pub fn after(t: i64) -> i64 {
    t.saturating_add(1)
}

/// A normalised value on the 32-bit scale.
pub fn v32(v: f64) -> i64 {
    i64::from(to_u32(v))
}

/// A 32-bit-scale value as a normalised value.
pub fn normalized(v32: i64) -> f64 {
    from_u32(v32.clamp(0, i64::from(u32::MAX)) as u32)
}

/// A plain parameter value, never in exponent form: precision follows
/// magnitude and trailing zeros go (`StripLane.swift:87-99`).
pub fn plain_text(x: f64) -> String {
    if !x.is_finite() {
        return "0".into();
    }
    let a = x.abs();
    let places = if a >= 1000. {
        0
    } else if a >= 100. {
        1
    } else if a >= 10. {
        2
    } else if a >= 1. {
        3
    } else if a >= 0.01 {
        4
    } else {
        6
    };
    let mut t = format!("{x:.places$}");
    if t.contains('.') {
        while t.ends_with('0') {
            t.pop();
        }
        if t.ends_with('.') {
            t.pop();
        }
    }
    if t == "-0" { "0".into() } else { t }
}

/// A normalised value as 0.0–100.0 % (`StripLane.swift:104`).
pub fn percent(v: f64) -> String {
    format!("{:.1}%", unit(v) * 100.)
}

/// Typed "0"…"100" (an optional "%") as a normalised value
/// (`AutomationText.swift:13-17`).
pub fn parse_percent(s: &str) -> Option<f64> {
    let t = s.replace('%', "");
    let p: f64 = crate::list::trim_ws(&t).parse().ok()?;
    (p.is_finite() && (0.0..=100.).contains(&p)).then_some(p / 100.)
}

/// A target's name where no mixer names are at hand (the List editor,
/// Plan 4d Ruling 26, `AutomationText.swift:7`).
pub fn short_title(t: &AutomationTargetRow) -> String {
    StripNames::default().title(t)
}

/// A value as the Strip Chart shows it (`StripLane.swift:106-126`).
pub fn automation_text(t: &AutomationTargetRow, v: f64, param: Option<&ParamRow>) -> String {
    use AutomationTargetRow as T;
    match t {
        T::StripFader { .. } | T::SendLevel { .. } | T::VcaFader { .. } => {
            let db = fader_db(v);
            // A level that would read "-144.0" is the floor: "-∞".
            if db < GAIN_FLOOR_DB + 0.05 {
                "-∞ dB".into()
            } else {
                format!("{db:.1} dB")
            }
        }
        T::StripPan { .. } => {
            let p = (pan_of(v) * 100.).round() as i64;
            match p {
                0 => "C".into(),
                p if p < 0 => format!("L{}", -p),
                p => format!("R{p}"),
            }
        }
        T::StripMute { .. } | T::StripSolo { .. } => if switch_on(v) { "On" } else { "Off" }.into(),
        T::PluginParam { .. } => match param {
            // `plain_text` never traps, whatever the range (non-finite, huge).
            Some(p) => plain_text(param_plain(v, p.min, p.max, p.stepped)),
            None => percent(v),
        },
        T::VelocityScale { .. } => format!("{:.0}%", velocity_scale_percent(v)),
    }
}

/// Names for automation targets, from the mixer (Plan 4d Ruling 21,
/// `StripLane.swift:144-174`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StripNames {
    pub strips: BTreeMap<u64, String>,
    /// Per strip, its sends' target names in send order.
    pub sends: BTreeMap<u64, Vec<String>>,
    pub slots: BTreeMap<u64, String>,
    pub params: BTreeMap<u64, Vec<ParamRow>>,
    pub dests: BTreeMap<u64, String>,
    pub vcas: BTreeMap<u64, String>,
}

impl StripNames {
    pub fn param(&self, slot: u64, id: u64) -> Option<&ParamRow> {
        self.params.get(&slot)?.iter().find(|p| p.id == id)
    }

    /// Whether `t`'s strip, plugin slot, destination or VCA still exists
    /// (names read from a mixer with no strips know nothing: every target
    /// passes).
    pub fn knows(&self, t: &AutomationTargetRow) -> bool {
        use AutomationTargetRow as T;
        if self.strips.is_empty() {
            return true;
        }
        match *t {
            T::StripFader { strip }
            | T::StripPan { strip }
            | T::StripMute { strip }
            | T::StripSolo { strip }
            | T::SendLevel { strip, .. } => self.strips.contains_key(&strip),
            T::PluginParam { slot, .. } => self.slots.contains_key(&slot),
            T::VelocityScale { dest } => self.dests.contains_key(&dest),
            T::VcaFader { vca } => self.vcas.contains_key(&vca),
        }
    }

    fn strip(&self, s: u64) -> String {
        self.strips
            .get(&s)
            .cloned()
            .unwrap_or_else(|| format!("Strip {s}"))
    }

    pub fn title(&self, t: &AutomationTargetRow) -> String {
        use AutomationTargetRow as T;
        match *t {
            T::StripFader { strip } => format!("Fader · {}", self.strip(strip)),
            T::StripPan { strip } => format!("Pan · {}", self.strip(strip)),
            T::StripMute { strip } => format!("Mute · {}", self.strip(strip)),
            T::StripSolo { strip } => format!("Solo · {}", self.strip(strip)),
            T::SendLevel { strip, send } => {
                let to = self
                    .sends
                    .get(&strip)
                    .and_then(|s| s.get(usize::from(send)))
                    .map(|n| format!(" to {n}"))
                    .unwrap_or_default();
                format!("Send {}{to} · {}", u32::from(send) + 1, self.strip(strip))
            }
            T::PluginParam { slot, param } => {
                let name = self
                    .param(slot, param)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| format!("Parameter {param}"));
                let plugin = self
                    .slots
                    .get(&slot)
                    .cloned()
                    .unwrap_or_else(|| format!("Plugin {slot}"));
                format!("{name} · {plugin}")
            }
            T::VelocityScale { dest } => {
                let d = self
                    .dests
                    .get(&dest)
                    .cloned()
                    .unwrap_or_else(|| format!("Destination {dest}"));
                format!("Velocity Scale · {d}")
            }
            // As the core names the VCA's new track ("VCA ‹name› Fader").
            T::VcaFader { vca } => {
                let n = self
                    .vcas
                    .get(&vca)
                    .cloned()
                    .unwrap_or_else(|| vca.to_string());
                format!("VCA {n} Fader")
            }
        }
    }
}

/// One value a Strip Chart row draws (`StripLane.swift:177-196`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripValue {
    /// The event (`None` for a tempo point).
    pub id: Option<u64>,
    pub tick: i64,
    /// A note's length (bars are this wide); 0 for points.
    pub length: i64,
    pub value: i64,
    /// The value ramps to the next point (continuous automation, a ramping
    /// tempo point).
    pub ramp: bool,
}

/// How a row's values are drawn: bars at note starts, held steps, or
/// ramps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StripStyle {
    Bars,
    Steps,
    Ramps,
}

/// What a Strip Chart row shows (Plan 4d Ruling 21,
/// `StripLane.swift:199-376`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StripLane {
    Velocity,
    ReleaseVelocity,
    Duration,
    Controller(u8),
    PitchBend,
    ChannelPressure,
    PolyPressure(u8),
    Program,
    Tempo,
    Automation(AutomationTargetRow),
}

impl StripLane {
    /// The controllers the row menu lists first (`StripLane.swift:211`).
    pub const CONTROLLERS: [u8; 8] = [1, 2, 4, 7, 10, 11, 64, 74];

    /// A listed controller's name (`StripLane.swift:212-215`).
    pub fn controller_name(i: u8) -> Option<&'static str> {
        Some(match i {
            1 => "Modulation",
            2 => "Breath",
            4 => "Foot",
            7 => "Volume",
            10 => "Pan",
            11 => "Expression",
            64 => "Sustain",
            74 => "Brightness",
            _ => return None,
        })
    }

    /// The row of a line target (the panel's target picker); `None` for
    /// a plain value.
    pub fn from_line_target(t: LineTargetRow) -> Option<StripLane> {
        Some(match t {
            LineTargetRow::Value { target } => match target {
                ValueTargetRow::Velocity => Self::Velocity,
                ValueTargetRow::ReleaseVelocity => Self::ReleaseVelocity,
                ValueTargetRow::Duration => Self::Duration,
                ValueTargetRow::Value => return None,
            },
            LineTargetRow::Series { series } => match series {
                SeriesRow::Controller { index } => Self::Controller(index),
                SeriesRow::PitchBend => Self::PitchBend,
                SeriesRow::ChannelPressure => Self::ChannelPressure,
                SeriesRow::PolyPressure { pitch } => Self::PolyPressure(pitch),
                SeriesRow::Program => Self::Program,
                SeriesRow::Automation { target } => Self::Automation(target),
            },
            LineTargetRow::Tempo => Self::Tempo,
        })
    }

    pub fn line_target(self) -> LineTargetRow {
        let value = |target| LineTargetRow::Value { target };
        let series = |series| LineTargetRow::Series { series };
        match self {
            Self::Velocity => value(ValueTargetRow::Velocity),
            Self::ReleaseVelocity => value(ValueTargetRow::ReleaseVelocity),
            Self::Duration => value(ValueTargetRow::Duration),
            Self::Controller(index) => series(SeriesRow::Controller { index }),
            Self::PitchBend => series(SeriesRow::PitchBend),
            Self::ChannelPressure => series(SeriesRow::ChannelPressure),
            Self::PolyPressure(pitch) => series(SeriesRow::PolyPressure { pitch }),
            Self::Program => series(SeriesRow::Program),
            Self::Tempo => LineTargetRow::Tempo,
            Self::Automation(target) => series(SeriesRow::Automation { target }),
        }
    }

    pub fn is_note_value(self) -> bool {
        matches!(
            self,
            Self::Velocity | Self::ReleaseVelocity | Self::Duration
        )
    }

    pub fn style(self) -> StripStyle {
        match self {
            Self::Velocity | Self::ReleaseVelocity | Self::Duration => StripStyle::Bars,
            Self::Tempo => StripStyle::Ramps,
            Self::Automation(t) if is_switch(&t) => StripStyle::Steps,
            Self::Automation(_) => StripStyle::Ramps,
            _ => StripStyle::Steps,
        }
    }

    /// The values a row spans, bottom to top, inclusive (Plan 4d Ruling 18).
    pub fn range(self) -> (i64, i64) {
        match self {
            Self::Velocity | Self::ReleaseVelocity => (0, 0xFFFF),
            Self::Duration => (0, 4 * PPQ),
            Self::Tempo => (30_000, 300_000),
            _ => (0, i64::from(u32::MAX)),
        }
    }

    /// A centre line (pitch bend, pan).
    pub fn center(self) -> Option<i64> {
        match self {
            Self::PitchBend | Self::Automation(AutomationTargetRow::StripPan { .. }) => {
                Some(0x8000_0000)
            }
            _ => None,
        }
    }

    /// The row's values among `events` (or the tempo map), in their order.
    pub fn values(self, events: &[EventRow], tempo: &[TempoPointRow]) -> Vec<StripValue> {
        match self {
            // A non-finite bpm is skipped rather than trapping.
            Self::Tempo => tempo
                .iter()
                .filter(|p| p.bpm.is_finite())
                .map(|p| StripValue {
                    id: None,
                    tick: p.tick,
                    length: 0,
                    value: (p.bpm * 1000.).round().clamp(0., 1e9) as i64,
                    ramp: p.ramp,
                })
                .collect(),
            Self::Velocity | Self::ReleaseVelocity | Self::Duration => events
                .iter()
                .filter(|e| e.kind == EventKindRow::Note)
                .map(|e| StripValue {
                    id: Some(e.id),
                    tick: e.tick,
                    length: e.duration.max(0),
                    value: match self {
                        Self::Velocity => i64::from(e.velocity),
                        Self::ReleaseVelocity => i64::from(e.release_velocity),
                        _ => e.duration,
                    },
                    ramp: false,
                })
                .collect(),
            _ => {
                let ramp = self.style() == StripStyle::Ramps;
                events
                    .iter()
                    .filter_map(|e| {
                        self.series_value(e).map(|value| StripValue {
                            id: Some(e.id),
                            tick: e.tick,
                            length: 0,
                            value,
                            ramp,
                        })
                    })
                    .collect()
            }
        }
    }

    /// `e`'s value when it belongs to this row's series (32-bit scale).
    fn series_value(self, e: &EventRow) -> Option<i64> {
        use EventKindRow as K;
        match self {
            Self::Controller(i) => (e.kind == K::Cc && e.value2 == i64::from(i)).then_some(e.value),
            Self::PitchBend => (e.kind == K::PitchBend).then_some(e.value),
            Self::ChannelPressure => (e.kind == K::ChannelPressure).then_some(e.value),
            Self::PolyPressure(p) => (e.kind == K::PolyPressure && e.pitch == p).then_some(e.value),
            Self::Program => (e.kind == K::Program).then_some(e.value.wrapping_shl(25)),
            Self::Automation(t) => match e.automation {
                Some(a) if e.kind == K::Automation && a.target == t => Some(v32(a.value)),
                _ => None,
            },
            _ => None,
        }
    }

    /// What the eraser deletes between two ticks (either order); `None` for
    /// note values and tempo, which are not erased here.
    pub fn erase_rule(self, from: i64, to: i64) -> Option<RuleRow> {
        let (lo, hi) = (from.min(to), from.max(to));
        let rule = |tests: Vec<ConditionTest>| RuleRow {
            conditions: std::iter::once(ConditionTest::TimeRange {
                start: lo,
                end: after(hi),
            })
            .chain(tests)
            .map(|test| ConditionRow {
                test,
                negate: false,
            })
            .collect(),
            match_all: true,
        };
        let kinds = |k: EventKindRow| ConditionTest::Kinds { kinds: vec![k] };
        Some(match self {
            Self::Velocity | Self::ReleaseVelocity | Self::Duration | Self::Tempo => return None,
            Self::Controller(i) => rule(vec![
                kinds(EventKindRow::Cc),
                ConditionTest::Controller { low: i, high: i },
            ]),
            Self::PitchBend => rule(vec![kinds(EventKindRow::PitchBend)]),
            Self::ChannelPressure => rule(vec![kinds(EventKindRow::ChannelPressure)]),
            Self::PolyPressure(p) => rule(vec![
                kinds(EventKindRow::PolyPressure),
                ConditionTest::Pitch { low: p, high: p },
            ]),
            Self::Program => rule(vec![kinds(EventKindRow::Program)]),
            Self::Automation(target) => rule(vec![ConditionTest::Automation { target }]),
        })
    }

    /// The row's title (`StripLane.swift:347-359`).
    pub fn title(self, names: &StripNames) -> String {
        match self {
            Self::Velocity => "Velocity".into(),
            Self::ReleaseVelocity => "Release Velocity".into(),
            Self::Duration => "Length".into(),
            Self::Controller(i) => match Self::controller_name(i) {
                Some(n) => format!("Controller {i} ({n})"),
                None => format!("Controller {i}"),
            },
            Self::PitchBend => "Pitch Bend".into(),
            Self::ChannelPressure => "Channel Pressure".into(),
            Self::PolyPressure(p) => format!("Poly Pressure {}", PitchName::name(p)),
            Self::Program => "Program".into(),
            Self::Tempo => "Tempo".into(),
            Self::Automation(t) => names.title(&t),
        }
    }

    /// A value as the row shows it (`StripLane.swift:362-375`).
    pub fn text(self, v: i64, names: &StripNames) -> String {
        match self {
            Self::Velocity | Self::ReleaseVelocity => {
                Velocity7::of(v.clamp(0, i64::from(u16::MAX)) as u16).to_string()
            }
            Self::Duration => DurationText::text(v.max(0), PPQ),
            Self::Controller(_) | Self::ChannelPressure | Self::PolyPressure(_) => {
                Value7::of(v).to_string()
            }
            Self::PitchBend => Bend14::of(v).to_string(),
            Self::Program => (v.clamp(0, i64::from(u32::MAX)) >> 25).to_string(),
            Self::Tempo => format!("{:.1} BPM", v as f64 / 1000.),
            Self::Automation(t) => {
                let p = match t {
                    AutomationTargetRow::PluginParam { slot, param } => names.param(slot, param),
                    _ => None,
                };
                automation_text(&t, normalized(v), p)
            }
        }
    }
}

// Swift's `Codable` shapes for `StripLane` and `AutoTarget` (Ruling 5):
// `{"velocity":{}}`, `{"controller":{"_0":7}}`,
// `{"automation":{"_0":{"send":{"strip":5,"index":2}}}}`.

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum TargetRepr {
    Fader { strip: u64 },
    Pan { strip: u64 },
    Mute { strip: u64 },
    Solo { strip: u64 },
    Send { strip: u64, index: u16 },
    Param { slot: u64, param: u64 },
    VelocityScale { dest: u64 },
    VcaFader { vca: u64 },
}

impl From<AutomationTargetRow> for TargetRepr {
    fn from(t: AutomationTargetRow) -> Self {
        use AutomationTargetRow as T;
        match t {
            T::StripFader { strip } => Self::Fader { strip },
            T::StripPan { strip } => Self::Pan { strip },
            T::StripMute { strip } => Self::Mute { strip },
            T::StripSolo { strip } => Self::Solo { strip },
            T::SendLevel { strip, send } => Self::Send { strip, index: send },
            T::PluginParam { slot, param } => Self::Param { slot, param },
            T::VelocityScale { dest } => Self::VelocityScale { dest },
            T::VcaFader { vca } => Self::VcaFader { vca },
        }
    }
}

impl From<TargetRepr> for AutomationTargetRow {
    fn from(t: TargetRepr) -> Self {
        use TargetRepr as R;
        match t {
            R::Fader { strip } => Self::StripFader { strip },
            R::Pan { strip } => Self::StripPan { strip },
            R::Mute { strip } => Self::StripMute { strip },
            R::Solo { strip } => Self::StripSolo { strip },
            R::Send { strip, index } => Self::SendLevel { strip, send: index },
            R::Param { slot, param } => Self::PluginParam { slot, param },
            R::VelocityScale { dest } => Self::VelocityScale { dest },
            R::VcaFader { vca } => Self::VcaFader { vca },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum LaneRepr {
    Velocity {},
    ReleaseVelocity {},
    Duration {},
    Controller {
        #[serde(rename = "_0")]
        index: u8,
    },
    PitchBend {},
    ChannelPressure {},
    PolyPressure {
        #[serde(rename = "_0")]
        pitch: u8,
    },
    Program {},
    Tempo {},
    Automation {
        #[serde(rename = "_0")]
        target: TargetRepr,
    },
}

impl Serialize for StripLane {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match *self {
            Self::Velocity => LaneRepr::Velocity {},
            Self::ReleaseVelocity => LaneRepr::ReleaseVelocity {},
            Self::Duration => LaneRepr::Duration {},
            Self::Controller(index) => LaneRepr::Controller { index },
            Self::PitchBend => LaneRepr::PitchBend {},
            Self::ChannelPressure => LaneRepr::ChannelPressure {},
            Self::PolyPressure(pitch) => LaneRepr::PolyPressure { pitch },
            Self::Program => LaneRepr::Program {},
            Self::Tempo => LaneRepr::Tempo {},
            Self::Automation(t) => LaneRepr::Automation { target: t.into() },
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for StripLane {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match LaneRepr::deserialize(d)? {
            LaneRepr::Velocity {} => Self::Velocity,
            LaneRepr::ReleaseVelocity {} => Self::ReleaseVelocity,
            LaneRepr::Duration {} => Self::Duration,
            LaneRepr::Controller { index } => Self::Controller(index),
            LaneRepr::PitchBend {} => Self::PitchBend,
            LaneRepr::ChannelPressure {} => Self::ChannelPressure,
            LaneRepr::PolyPressure { pitch } => Self::PolyPressure(pitch),
            LaneRepr::Program {} => Self::Program,
            LaneRepr::Tempo {} => Self::Tempo,
            LaneRepr::Automation { target } => Self::Automation(target.into()),
        })
    }
}
