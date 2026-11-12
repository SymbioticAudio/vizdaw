//! The return record and the snapshot (plan 6d Ruling 16, spec §7.8): what
//! an export keeps so that plan 6e can merge a returning file.
//!
//! The record (`vizdaw/record.json` in the package) holds the whole
//! document; the snapshot (`Interchange/<export id>.json` in the project)
//! holds the [`Projection`] of what was exported. Both map every XML ID to
//! the VizDAW object it came from.

use super::export::{Layout, Ticker, layout};
use super::units::swatch_hex;
use crate::IoError;
use core_model::automation::AutomationTarget;
use core_model::document::Document;
use core_model::event::{EventBody, PerNoteKind};
use core_model::ids::SeqId;
use core_model::mixer::PluginFormat;
use core_model::query::ResolvedEvent;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;

/// The record's package entry.
pub const RECORD_ENTRY: &str = "vizdaw/record.json";
/// The project folder holding the snapshots.
pub const SNAPSHOT_DIR: &str = "Interchange";
/// How many snapshots a project keeps.
pub const KEEP_SNAPSHOTS: usize = 10;
/// The record's and the snapshot's format.
pub const FORMAT: u32 = 1;

/// The VizDAW object an XML ID stands for. Every ID the export writes has
/// one. `kind` and what `id` (and `slot`, `index`) name:
///
/// - `track`: a track's `Track` (`t<lane>`, `t<lane>a`); `id` is the lane;
/// - `strip`: a strip's channel or its copy (`c<strip>`, `c<strip>t<lane>`),
///   a bus track (`b<strip>`) or the master track; `id` is the strip;
/// - `channel`: a plain channel (`k<lane>`, `k<lane>a`); `id` is its track;
/// - `volume`, `pan`, `mute`: a strip channel's parameter (`id` the strip);
///   `channel_volume`, `channel_pan`, `channel_mute`: a plain channel's
///   (`id` its track);
/// - `send`, `send_volume`, `send_enable`: a send and its parameters; `id`
///   is the strip, `index` the send's position;
/// - `device`, `device_enable`: a plug-in and its on/off; `id` is the
///   instance;
/// - `parameter`: a plug-in parameter; `id` is the parameter, `slot` its
///   instance;
/// - `tempo`, `meter`: the transport's tempo and time signature; `id` is the
///   sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRef {
    /// What it is (see above).
    pub kind: String,
    /// Its ID's number.
    pub id: u64,
    /// The plug-in instance of a `parameter`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    /// The position of a send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u64>,
}

/// `vizdaw/record.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// [`FORMAT`].
    pub format: u32,
    /// The project's ID.
    pub project_id: String,
    /// The export's ID.
    pub export_id: String,
    /// When, in Unix seconds.
    pub exported_at: u64,
    /// VizDAW's version.
    pub app_version: String,
    /// The exported sequence's ID.
    pub seq: u64,
    /// The tick at file time 0.
    pub origin: i64,
    /// XML ID → VizDAW object.
    pub ids: BTreeMap<String, ObjectRef>,
    /// The whole `project.json`, as JSON.
    pub document: serde_json::Value,
}

/// The part of a record an import reads (plan 6d).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecordHead {
    /// [`FORMAT`].
    pub format: u32,
    /// The project's ID.
    pub project_id: String,
    /// The export's ID.
    pub export_id: String,
}

/// `Interchange/<export id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// [`FORMAT`].
    pub format: u32,
    /// The project's ID.
    pub project_id: String,
    /// The export's ID.
    pub export_id: String,
    /// When, in Unix seconds.
    pub exported_at: u64,
    /// The exported sequence's ID.
    pub seq: u64,
    /// The tick at file time 0.
    pub origin: i64,
    /// XML ID → VizDAW object.
    pub ids: BTreeMap<String, ObjectRef>,
    /// What was exported.
    pub projection: Projection,
}

/// What the route carries (§7.1), ID-free: tracks in order, strips by index.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct Projection {
    /// The exported tracks (a split track is two).
    pub tracks: Vec<PTrack>,
    /// The exported channels, in the structure's order: the tracks', then
    /// buses and submixes, then the master.
    pub strips: Vec<PStrip>,
    /// The tempo map: tick, milli-BPM, ramp.
    pub tempo: Vec<(i64, u32, bool)>,
    /// The meter map: tick, numerator, denominator.
    pub meter: Vec<(i64, u8, u8)>,
    /// The markers: tick, name.
    pub markers: Vec<(i64, String)>,
    /// The 13 song-info fields.
    pub info: Vec<Option<String>>,
    /// The tick at file time 0.
    pub origin: i64,
}

/// One exported track.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct PTrack {
    /// Its name.
    pub name: String,
    /// Whether it holds notes.
    pub notes: bool,
    /// Whether it holds audio.
    pub audio: bool,
    /// Its channel in [`Projection::strips`].
    pub strip: Option<usize>,
    /// The notes it plays.
    pub notes_played: Vec<PNote>,
    /// Its other MIDI events: tick, "<kind> <channel> <number> <value>".
    pub midi: Vec<(i64, String)>,
    /// Its regions.
    pub regions: Vec<PRegion>,
    /// Its automation: target, then (tick, value) points.
    pub automation: Vec<(String, Vec<(i64, f64)>)>,
}

/// One played note.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct PNote {
    /// Where it starts.
    pub tick: i64,
    /// Its key.
    pub pitch: u8,
    /// Its MIDI channel.
    pub channel: u8,
    /// Its velocity.
    pub velocity: u16,
    /// Its release velocity.
    pub release: u16,
    /// Its length.
    pub duration: i64,
    /// Its per-note values the route carries: kind, offset, value.
    pub expression: Vec<(String, i64, u32)>,
}

/// One region.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct PRegion {
    /// Where it starts.
    pub tick: i64,
    /// Its media's content hash.
    pub media_hash: Option<String>,
    /// Its start in the media, in frames.
    pub offset: u64,
    /// Its length, in frames.
    pub length: u64,
    /// Its fade-in length, in frames.
    pub fade_in: u64,
    /// Its fade-out length, in frames.
    pub fade_out: u64,
    /// Whether it is muted.
    pub muted: bool,
    /// Its own name.
    pub name: Option<String>,
    /// Whether its fade-in is a crossfade.
    pub crossfade: bool,
    /// Its gain, in dB.
    pub gain_db: f32,
}

/// One exported channel.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct PStrip {
    /// Its name.
    pub name: String,
    /// "regular", "effect", "submix" or "master".
    pub role: String,
    /// Its colour, `#rrggbb`.
    pub colour: Option<String>,
    /// Its fader, in dB.
    pub gain_db: f64,
    /// Its pan, −1 … +1.
    pub pan: f64,
    /// Its mute.
    pub mute: bool,
    /// Its solo.
    pub solo: bool,
    /// The strip it plays into.
    pub output: Option<usize>,
    /// Its sends: target strip, level in dB, pre-fader, enabled.
    pub sends: Vec<(usize, f64, bool, bool)>,
    /// Its devices: format, plug-in ID, bypass, state hash.
    pub devices: Vec<(String, String, bool, Option<u64>)>,
}

/// The projection of sequence `seq` as `export` writes it.
pub fn projection(doc: &Document, seq: SeqId) -> Result<Projection, IoError> {
    let never = AtomicBool::new(false);
    let mut scratch = crate::report::Report::default();
    let l = layout(doc, seq, &mut scratch, &never)?;
    project(&l, &never)
}

fn role_text(r: super::schema::DpMixerRole) -> &'static str {
    use super::schema::DpMixerRole::*;
    match r {
        Regular => "regular",
        Master => "master",
        Effect => "effect",
        Submix => "submix",
        Vca => "vca",
    }
}

fn per_note_kind(k: PerNoteKind) -> Option<String> {
    match k {
        PerNoteKind::PitchBend => Some("pitch_bend".to_owned()),
        PerNoteKind::Registered(n @ (7 | 10 | 74)) => Some(format!("registered {n}")),
        PerNoteKind::Registered(_) | PerNoteKind::Assignable(_) => None,
    }
}

/// A MIDI event other than a note, as the projection spells it; `None` for
/// what the route does not carry.
fn midi_text(r: &ResolvedEvent) -> Option<String> {
    let ch = r.group_channel.map_or(0, |g| g.1);
    Some(match &r.body {
        EventBody::Cc { index, value } => format!("cc {ch} {index} {value}"),
        EventBody::PitchBend { value } => format!("pitch_bend {ch} 0 {value}"),
        EventBody::ChannelPressure { value } => format!("channel_pressure {ch} 0 {value}"),
        EventBody::PolyPressure { pitch, value } => format!("poly_pressure {ch} {pitch} {value}"),
        EventBody::Program { program, bank } => match bank {
            Some((msb, lsb)) => format!(
                "program {ch} {program} {}",
                u32::from(*msb) * 128 + u32::from(*lsb)
            ),
            None => format!("program {ch} {program} none"),
        },
        _ => return None,
    })
}

/// The projection of a layout.
pub(super) fn project(l: &Layout<'_>, cancel: &AtomicBool) -> Result<Projection, IoError> {
    let doc = l.doc;
    let s = l.seq;
    let by_id: BTreeMap<&str, usize> = l
        .chans
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id.as_str(), i))
        .collect();
    let index = |id: &str| by_id.get(id).copied();

    let strips = l
        .chans
        .iter()
        .map(|c| {
            let st = c.strip;
            PStrip {
                name: c.name.clone(),
                role: role_text(c.role).to_owned(),
                colour: st.and_then(|s| s.colour).map(|w| swatch_hex(w).to_owned()),
                gain_db: st.map_or(0.0, |s| s.gain_db),
                pan: st.map_or(0.0, |s| s.pan),
                mute: st.is_some_and(|s| s.mute),
                solo: st.is_some_and(|s| s.solo),
                output: c.to.as_deref().and_then(index),
                sends: st
                    .map(|s| {
                        s.sends
                            .iter()
                            .filter_map(|x| {
                                let to = index(&format!("c{}", x.target.0))?;
                                Some((to, x.level_db, x.pre_fader, x.enabled))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                devices: c
                    .slots()
                    .iter()
                    .map(|(slot, _, _)| {
                        let format = match slot.plugin.format {
                            PluginFormat::Au => "au",
                            PluginFormat::Clap => "clap",
                        };
                        let hash = slot
                            .state
                            .filter(|id| doc.state_blob(*id).is_some())
                            .map(|id| id.0);
                        (format.to_owned(), slot.plugin.id.clone(), slot.bypass, hash)
                    })
                    .collect(),
            }
        })
        .collect();

    let mut ticker = Ticker::new(cancel);
    let mut tracks = Vec::new();
    for (pi, p) in l.parts.iter().enumerate() {
        let mut t = PTrack {
            name: p.name(),
            notes: !p.audio,
            audio: p.audio,
            strip: Some(p.chan),
            notes_played: Vec::new(),
            midi: Vec::new(),
            regions: Vec::new(),
            automation: Vec::new(),
        };
        let mut automation: BTreeMap<String, Vec<(i64, f64)>> = BTreeMap::new();
        for r in l.played.get(&p.lane.id).map_or(&[][..], Vec::as_slice) {
            ticker.tick()?;
            // What a muted reference use holds does not play.
            if r.path.len() > 1 && r.path.first().is_some_and(|x| l.muted.contains(&x.event)) {
                continue;
            }
            let tick = r.tick.0;
            match &r.body {
                EventBody::AudioRegion {
                    media,
                    source_offset,
                    length,
                    gain_db,
                    fade_in,
                    fade_out,
                    name,
                    crossfade,
                } if p.audio => t.regions.push(PRegion {
                    tick,
                    media_hash: doc.media().get(media).and_then(|m| m.content_hash.clone()),
                    offset: *source_offset,
                    length: *length,
                    fade_in: fade_in.length_frames,
                    fade_out: fade_out.length_frames,
                    muted: r.path.len() == 1
                        && r.path.last().is_some_and(|x| l.muted.contains(&x.event)),
                    name: name.clone(),
                    crossfade: *crossfade,
                    gain_db: *gain_db,
                }),
                EventBody::AudioRegion { .. } => {}
                _ if p.audio && p.split => {}
                EventBody::Note {
                    pitch,
                    velocity,
                    release_velocity,
                    duration,
                    per_note,
                    ..
                } => t.notes_played.push(PNote {
                    tick,
                    pitch: *pitch,
                    channel: r.group_channel.map_or(0, |g| g.1),
                    velocity: *velocity,
                    release: *release_velocity,
                    duration: duration.0,
                    expression: per_note
                        .iter()
                        .filter_map(|c| Some((per_note_kind(c.kind)?, c.offset.0, c.value)))
                        .collect(),
                }),
                EventBody::Automation { target, value } => {
                    let Some((c, d)) = l.target(pi, *target) else {
                        continue;
                    };
                    let name = match (*target, d) {
                        (AutomationTarget::StripFader(_), _) => format!("fader {c}"),
                        (AutomationTarget::StripPan(_), _) => format!("pan {c}"),
                        (AutomationTarget::StripMute(_), _) => format!("mute {c}"),
                        (AutomationTarget::SendLevel(_, i), _) => format!("send {c} {}", i.0),
                        (AutomationTarget::PluginParam(_, q), Some(d)) => {
                            format!("param {c} {d} {}", q.0)
                        }
                        _ => continue,
                    };
                    automation.entry(name).or_default().push((tick, *value));
                }
                _ => t.midi.extend(midi_text(r).map(|m| (tick, m))),
            }
        }
        t.automation = automation.into_iter().collect();
        tracks.push(t);
    }
    Ok(Projection {
        tracks,
        strips,
        tempo: s
            .tempo
            .points()
            .iter()
            .map(|p| (p.tick.0, p.bpm.0, p.ramp))
            .collect(),
        meter: s
            .meter
            .entries()
            .iter()
            .map(|e| (e.tick.0, e.meter.num, e.meter.den))
            .collect(),
        markers: s
            .markers
            .iter()
            .map(|m| (m.tick.0, m.name.clone()))
            .collect(),
        info: doc.settings().info.fields().into_iter().cloned().collect(),
        origin: l.origin,
    })
}
