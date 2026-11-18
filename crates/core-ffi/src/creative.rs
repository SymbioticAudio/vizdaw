//! Co-Producer's creative commands and reads (plan 8e Task 3; Rulings
//! 1–4): the core-model commands `automation.write` builds
//! ([`commands_for`]), the target's own units to the stored 0–1
//! ([`to_stored`]) and back, and the reads of a track's notes and of a
//! target's automation. The preview (`co_producer::preview_items`) and
//! the apply (`Session::run_creative`) both build through
//! [`commands_for`], so what the composer says yes to is what runs. Every
//! argument is hostile: each is read by name and checked again here.
//!
//! No command here writes notes (owner, 2026-11-16): notes are only read.

use crate::automation::parse_automation_target;
use crate::error::VizError;
use crate::events::{automation_target_of, automation_target_row};
use crate::mixer_chores::PluginLookup;
use crate::tracks::{TrackKindRow, track_kind};
use core_control::registry::{self, ArgValue, CREATIVE_IDS};
use core_model::automation::{
    AutomationTarget, fader_db, fader_norm, pan_norm, pan_of, param_plain, strip_of, switch_on,
    target_exists,
};
use core_model::command::{AutomationWrite, Command};
use core_model::diff::{bar_of, shown_name};
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::{LaneId, SeqId};
use core_model::mixer::{GAIN_FLOOR_DB, GAIN_MAX_DB};
use core_model::time::Tick;

/// The most notes or points one read gives (plan 8e Rulings 3 and 7).
pub const MAX_READ: usize = 4096;

/// The longest name a new automation track gets (plan 8e Ruling 2).
const MAX_LANE_NAME: usize = 64;

/// One note of a read, in ticks at 15 360 per quarter note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct NoteRow {
    /// Its start tick.
    pub start: u64,
    /// Note number 0–127.
    pub pitch: u8,
    /// Its length in ticks.
    pub length: u64,
    /// Its attack velocity, 7-bit.
    pub velocity: u8,
}

/// A track's notes for Co-Producer (plan 8e Ruling 3).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoProducerNotesRow {
    /// The sequence.
    pub seq: u64,
    /// The track.
    pub lane: u64,
    /// The track's name, safe to show.
    pub name: String,
    /// The active take's notes, by start then pitch, at most [`MAX_READ`].
    pub notes: Vec<NoteRow>,
    /// More notes were in range than listed.
    pub truncated: bool,
}

/// One automation point of a read, in the target's own units.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct CurvePointRow {
    /// Its tick.
    pub tick: u64,
    /// Its value in the target's units.
    pub value: f64,
}

/// A target's automation for Co-Producer (plan 8e Ruling 7).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CoProducerCurveRow {
    /// The target as given.
    pub target: String,
    /// "<strip name> <fader|pan|mute|solo|send n|param name>", safe to
    /// show.
    pub target_name: String,
    /// "dB", "%", "on/off", or "" for a parameter.
    pub unit: String,
    /// The lowest value in the unit.
    pub min: f64,
    /// The highest value in the unit.
    pub max: f64,
    /// The bar of `start`, from 1.
    pub start_bar: u32,
    /// The bar of `end`, from 1.
    pub end_bar: u32,
    /// The points in `start..=end`, by tick, at most [`MAX_READ`].
    pub points: Vec<CurvePointRow>,
    /// More points were in range than listed.
    pub truncated: bool,
}

fn bad(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// The target's units (Ruling 1).
enum Scale {
    /// A fader or send level, dB −144..=12.
    Level,
    /// Pan, percent −100..=100.
    Pan,
    /// Mute or solo, 0 or 1.
    Switch,
    /// A plug-in parameter in its own range.
    Param {
        name: String,
        min: f64,
        max: f64,
        stepped: bool,
    },
}

/// The text `t` is spelled with (`fader:<strip>` and so on).
fn target_text(t: AutomationTarget) -> String {
    crate::automation::automation_target_text(automation_target_row(t))
}

fn no_target(text: &str) -> VizError {
    let shown: String = text.chars().take(64).collect();
    bad(format!("This song has no {}.", shown_name(&shown)))
}

/// `text` as a target of this song: "This song has no <target>."
/// otherwise.
fn target_of(doc: &Document, text: &str) -> Result<AutomationTarget, VizError> {
    let t = parse_automation_target(text.to_string())
        .map(automation_target_of)
        .ok_or_else(|| no_target(text))?;
    if !target_exists(doc, t) || matches!(t, AutomationTarget::VelocityScale(_)) {
        return Err(no_target(text));
    }
    Ok(t)
}

/// `seq`, a top-level sequence of `doc`: "This song has no sequence
/// <seq>." otherwise.
fn top_seq(doc: &Document, seq: u64) -> Result<SeqId, VizError> {
    let id = SeqId(seq);
    let top = doc.sequence(id).is_ok()
        && core_model::query::use_counts(doc)
            .get(&id)
            .is_some_and(|n| *n == 0);
    if top {
        Ok(id)
    } else {
        Err(bad(format!("This song has no sequence {seq}.")))
    }
}

/// The units of `t`, which exists in `doc`; a parameter needs its
/// plug-in running (plan 8d's sentence otherwise).
fn scale_of(
    doc: &Document,
    t: AutomationTarget,
    host: &dyn PluginLookup,
) -> Result<Scale, VizError> {
    Ok(match t {
        AutomationTarget::StripFader(_) | AutomationTarget::SendLevel(..) => Scale::Level,
        AutomationTarget::StripPan(_) => Scale::Pan,
        AutomationTarget::StripMute(_) | AutomationTarget::StripSolo(_) => Scale::Switch,
        AutomationTarget::PluginParam(slot, param) => {
            let rows = host.params(slot).ok_or_else(|| {
                bad(format!(
                    "The plug-in in slot {} is not running yet; read plugin_params again in a moment.",
                    slot.0
                ))
            })?;
            let row = rows
                .into_iter()
                .find(|r| r.id == param.0 && !r.hidden)
                .ok_or_else(|| {
                    bad(format!("Slot {} has no parameter {}.", slot.0, param.0))
                })?;
            Scale::Param {
                name: shown_name(&row.name),
                min: row.min,
                max: row.max,
                stepped: row.stepped,
            }
        }
        AutomationTarget::VelocityScale(_) => return Err(no_target(&target_text(t))),
    })
    .and_then(|s| {
        // `t` was checked by the caller; this keeps `to_stored` safe alone.
        if target_exists(doc, t) {
            Ok(s)
        } else {
            Err(no_target(&target_text(t)))
        }
    })
}

/// A whole value as text: at most 6 decimals, trailing zeros dropped.
fn shown_value(v: f64) -> String {
    let t = format!("{v:.6}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" { "0".into() } else { t.into() }
}

/// What `t` is called after its strip's name: `fader`, `pan`, `mute`,
/// `solo`, `send <n>` or the parameter's name.
fn kind_name(t: AutomationTarget, scale: &Scale) -> String {
    match (t, scale) {
        (_, Scale::Param { name, .. }) => name.clone(),
        (AutomationTarget::StripFader(_), _) => "fader".into(),
        (AutomationTarget::StripPan(_), _) => "pan".into(),
        (AutomationTarget::StripMute(_), _) => "mute".into(),
        (AutomationTarget::StripSolo(_), _) => "solo".into(),
        (AutomationTarget::SendLevel(_, i), _) => format!("send {}", u32::from(i.0) + 1),
        _ => "automation".into(),
    }
}

/// "<strip name> <kind>", safe to show.
fn target_name(doc: &Document, t: AutomationTarget, scale: &Scale) -> String {
    let strip = strip_of(doc, t)
        .and_then(|s| doc.mixer().strip(s).ok())
        .map(|s| s.name.clone())
        .unwrap_or_default();
    shown_name(format!("{strip} {}", kind_name(t, scale)).trim())
}

/// Ruling 2's name of a new automation track: "<strip name> <kind>
/// Automation", cut to 64 characters.
fn lane_name(doc: &Document, t: AutomationTarget, scale: &Scale) -> String {
    let strip = strip_of(doc, t)
        .and_then(|s| doc.mixer().strip(s).ok())
        .map(|s| s.name.clone())
        .unwrap_or_default();
    let full = format!("{strip} {} Automation", kind_name(t, scale));
    let cut: String = full.trim().chars().take(MAX_LANE_NAME).collect();
    match cut.trim_end() {
        "" => "Automation".into(),
        c => c.into(),
    }
}

fn convert(t: AutomationTarget, scale: &Scale, value: f64) -> Result<f64, VizError> {
    let label = match t {
        AutomationTarget::StripFader(_) => "fader",
        AutomationTarget::SendLevel(..) => "send",
        AutomationTarget::StripPan(_) => "pan",
        AutomationTarget::StripMute(_) => "mute",
        _ => "solo",
    };
    match scale {
        Scale::Level => {
            if !(GAIN_FLOOR_DB..=GAIN_MAX_DB).contains(&value) {
                return Err(bad(format!("{label} is \u{2212}144 to 12.")));
            }
            Ok(fader_norm(value))
        }
        Scale::Pan => {
            if !(-100.0..=100.0).contains(&value) {
                return Err(bad(format!("{label} is \u{2212}100 to 100.")));
            }
            Ok(pan_norm(value / 100.0))
        }
        Scale::Switch => {
            if value != 0.0 && value != 1.0 {
                return Err(bad(format!("{label} is 0 to 1.")));
            }
            Ok(value)
        }
        Scale::Param {
            name,
            min,
            max,
            stepped,
        } => {
            if !(min.is_finite() && max.is_finite() && (*min..=*max).contains(&value)) {
                return Err(bad(format!(
                    "{name} is {} to {}.",
                    shown_value(*min),
                    shown_value(*max)
                )));
            }
            if *stepped && value.fract() != 0.0 {
                return Err(bad(format!("{name} takes whole values.")));
            }
            if max > min {
                Ok(((value - min) / (max - min)).clamp(0.0, 1.0))
            } else {
                Ok(0.0)
            }
        }
    }
}

/// A value in the target's units to the stored 0–1, or Ruling 1's refusal.
pub fn to_stored(
    doc: &Document,
    target: AutomationTarget,
    value: f64,
    host: &dyn PluginLookup,
) -> Result<f64, VizError> {
    let scale = scale_of(doc, target, host)?;
    convert(target, &scale, value)
}

/// A stored 0–1 in the target's units.
fn from_stored(scale: &Scale, v: f64) -> f64 {
    match scale {
        Scale::Level => fader_db(v),
        Scale::Pan => pan_of(v) * 100.0,
        Scale::Switch => {
            if switch_on(v) {
                1.0
            } else {
                0.0
            }
        }
        Scale::Param {
            min, max, stepped, ..
        } => param_plain(v, *min, *max, *stepped),
    }
}

fn arg<'a>(args: &'a [(String, ArgValue)], name: &str) -> Option<&'a ArgValue> {
    args.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

fn need_int(id: &str, args: &[(String, ArgValue)], name: &str) -> Result<u64, VizError> {
    match arg(args, name) {
        Some(ArgValue::Int(i)) => {
            u64::try_from(*i).map_err(|_| bad(format!("{id}: `{name}` is out of range")))
        }
        Some(_) => Err(bad(format!("{id}: `{name}` must be a whole number"))),
        None => Err(bad(format!("{id}: give `{name}`"))),
    }
}

fn need_text<'a>(
    id: &str,
    args: &'a [(String, ArgValue)],
    name: &str,
) -> Result<&'a str, VizError> {
    match arg(args, name) {
        Some(ArgValue::Text(t)) => Ok(t),
        _ => Err(bad(format!("{id}: give `{name}` as text"))),
    }
}

/// A tick of a command or read: at most `i64::MAX`.
fn tick(id: &str, name: &str, v: u64) -> Result<Tick, VizError> {
    i64::try_from(v)
        .map(Tick)
        .map_err(|_| bad(format!("{id}: `{name}` is out of range")))
}

/// The core-model commands for `automation.write` (CREATIVE_IDS), in
/// order; Ruling 1's refusals, all found from `doc` (and `host`) before
/// any command is built. `args` are the registry's, read by name and
/// checked again.
pub fn commands_for(
    doc: &Document,
    id: &str,
    args: &[(String, ArgValue)],
    host: &dyn PluginLookup,
) -> Result<Vec<Command>, VizError> {
    if !CREATIVE_IDS.contains(&id) {
        let shown: String = id.chars().take(64).collect();
        return Err(bad(format!(
            "{} is not a creative command.",
            shown_name(&shown)
        )));
    }
    let seq = top_seq(doc, need_int(id, args, "seq")?)?;
    let target = target_of(doc, need_text(id, args, "target")?)?;
    let (start, end) = (need_int(id, args, "start")?, need_int(id, args, "end")?);
    if end <= start {
        return Err(bad(format!("{id}: `end` must be after `start`")));
    }
    let (start_tick, end_tick) = (tick(id, "start", start)?, tick(id, "end", end)?);
    let list = registry::point_list(need_text(id, args, "points")?, start, end).map_err(bad)?;
    let scale = scale_of(doc, target, host)?;
    let mut points = Vec::with_capacity(list.len());
    for (t, v) in list {
        points.push((tick(id, "points", t)?, convert(target, &scale, v)?));
    }
    Ok(vec![Command::WriteAutomation(AutomationWrite {
        seq,
        target,
        start: start_tick,
        end: end_tick,
        points,
        lane_name: lane_name(doc, target, &scale),
    })])
}

/// Ruling 3's read: the notes of track `lane` of sequence `seq`'s active
/// take, those starting in `[start, end)` when a range is given, by start
/// then pitch, at most [`MAX_READ`]. Changes nothing.
pub fn notes_of(
    doc: &Document,
    seq: u64,
    lane: u64,
    start: Option<u64>,
    end: Option<u64>,
) -> Result<CoProducerNotesRow, VizError> {
    let id = "co_producer.midi_notes";
    let s = top_seq(doc, seq)?;
    let range = match (start, end) {
        (None, None) => None,
        (Some(a), Some(b)) if b > a => Some((tick(id, "start", a)?, tick(id, "end", b)?)),
        (Some(_), Some(_)) => return Err(bad(format!("{id}: `end` must be after `start`"))),
        _ => {
            return Err(bad(format!(
                "{id}: give both `start` and `end`, or neither"
            )));
        }
    };
    let l = doc
        .sequence(s)?
        .lane(LaneId(lane))
        .ok_or_else(|| bad(format!("Track {lane} is not in sequence {seq}.")))?;
    if track_kind(doc, s, l.id)? == TrackKindRow::Audio {
        return Err(bad(format!(
            "Track {} is not a MIDI or instrument track.",
            shown_name(&l.name)
        )));
    }
    let mut notes: Vec<NoteRow> = l
        .active()
        .map(|take| {
            take.events
                .values()
                .filter(|e| range.is_none_or(|(a, b)| e.tick >= a && e.tick < b))
                .filter_map(|e| match &e.body {
                    EventBody::Note {
                        pitch,
                        velocity,
                        duration,
                        ..
                    } => Some(NoteRow {
                        start: u64::try_from(e.tick.0).ok()?,
                        pitch: *pitch,
                        length: u64::try_from(duration.0).unwrap_or(0),
                        // The inverse of `input::upscale_velocity`.
                        velocity: u8::try_from(velocity >> 9).unwrap_or(127),
                    }),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    notes.sort_by_key(|n| (n.start, n.pitch));
    let truncated = notes.len() > MAX_READ;
    notes.truncate(MAX_READ);
    Ok(CoProducerNotesRow {
        seq,
        lane,
        name: shown_name(&l.name),
        notes,
        truncated,
    })
}

/// Ruling 7's read: `target`'s automation points in sequence `seq` (the
/// active take of every track), in `start..=end`, by tick, in the
/// target's units, at most [`MAX_READ`]. Changes nothing.
pub fn curve_of(
    doc: &Document,
    seq: u64,
    target: &str,
    start: u64,
    end: u64,
    host: &dyn PluginLookup,
) -> Result<CoProducerCurveRow, VizError> {
    let id = "co_producer.automation";
    let s = top_seq(doc, seq)?;
    let t = target_of(doc, target)?;
    if end <= start {
        return Err(bad(format!("{id}: `end` must be after `start`")));
    }
    let (a, b) = (tick(id, "start", start)?, tick(id, "end", end)?);
    let scale = scale_of(doc, t, host)?;
    let mut points: Vec<CurvePointRow> = Vec::new();
    for lane in &doc.sequence(s)?.lanes {
        let Some(take) = lane.active() else { continue };
        for e in take.events.values() {
            if let EventBody::Automation { target: et, value } = e.body
                && et == t
                && e.tick >= a
                && e.tick <= b
                && let Ok(tick) = u64::try_from(e.tick.0)
            {
                points.push(CurvePointRow {
                    tick,
                    value: from_stored(&scale, value),
                });
            }
        }
    }
    points.sort_by_key(|p| p.tick);
    let truncated = points.len() > MAX_READ;
    points.truncate(MAX_READ);
    let (unit, min, max) = match &scale {
        Scale::Level => ("dB", GAIN_FLOOR_DB, GAIN_MAX_DB),
        Scale::Pan => ("%", -100.0, 100.0),
        Scale::Switch => ("on/off", 0.0, 1.0),
        Scale::Param { min, max, .. } => ("", *min, *max),
    };
    Ok(CoProducerCurveRow {
        target: target_text(t),
        target_name: target_name(doc, t, &scale),
        unit: unit.into(),
        min,
        max,
        start_bar: bar_of(doc, s, a),
        end_bar: bar_of(doc, s, b),
        points,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `core-control`'s grammar check and the app's parser agree.
    #[test]
    fn the_two_target_grammars_agree() {
        let long = format!("fader:{}", "1".repeat(70));
        let cases = [
            "fader:1",
            "pan:0",
            "mute:22",
            "solo:3",
            "send:1:1",
            "send:1:0",
            "send:1:65536",
            "send:1:65537",
            "param:4:0",
            "param:4",
            "fader:1:2",
            "fader:",
            "fader:-1",
            "fader:+1",
            "fader: 1",
            "Fader:1",
            "velocity:1",
            "",
            ":",
            "fader:18446744073709551615",
            "fader:18446744073709551616",
            "pan:1\0",
            long.as_str(),
        ];
        for c in cases {
            assert_eq!(
                parse_automation_target(c.to_string()).is_some(),
                registry::automation_target(c).is_ok(),
                "{c:?}"
            );
        }
    }
}
