//! Tracks (plan tracks Rulings 4 and 6): a lane's kind from its routing,
//! and the command lists that make a new audio, instrument or MIDI track
//! inside the caller's open transaction.

use crate::error::VizError;
use crate::events::bad;
use core_model::command::lane::LaneCmd;
use core_model::command::routing::RoutingCmd;
use core_model::command::{Command, Report};
use core_model::document::{DestKind, Document};
use core_model::event::EventBody;
use core_model::history::History;
use core_model::ids::{AnyId, DestId, LaneId, OutputPathId, PluginInstanceId, SeqId};
use core_model::lane::{InputChannels, LaneInput, MonitorMode};
use core_model::mixer::{BUILTIN_SYNTH_REF, PluginRef, PluginSlot};
use core_model::selection::ViewFilter;

/// What a track is for, as the track header's icon shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum TrackKindRow {
    /// Records and plays audio.
    Audio,
    /// Plays a built-in synth or a plugin instrument.
    Instrument,
    /// Plays an external MIDI destination, or nothing yet.
    Midi,
}

/// The kind of lane `lane` of `seq`; the first rule that matches wins:
/// an audio input is Audio; a built-in synth or plugin instrument
/// destination is Instrument; an external destination is Midi; an output
/// path or audio regions in the active take are Audio; otherwise Midi.
pub(crate) fn track_kind(
    doc: &Document,
    seq: SeqId,
    lane: LaneId,
) -> Result<TrackKindRow, VizError> {
    let l = doc.lane(seq, lane)?;
    if matches!(l.input, Some(LaneInput::Audio { .. })) {
        return Ok(TrackKindRow::Audio);
    }
    if let Some(d) = l.midi_dest.and_then(|d| doc.instruments().get(&d)) {
        match &d.kind {
            DestKind::PluginInstrument { .. } => return Ok(TrackKindRow::Instrument),
            k if k.is_builtin_synth() => return Ok(TrackKindRow::Instrument),
            k if k.is_external() => return Ok(TrackKindRow::Midi),
            _ => {}
        }
    }
    let regions = l.active().is_some_and(|t| {
        t.events
            .values()
            .any(|e| matches!(e.body, EventBody::AudioRegion { .. }))
    });
    if l.audio_out.is_some() || regions {
        return Ok(TrackKindRow::Audio);
    }
    Ok(TrackKindRow::Midi)
}

/// The history label of a new track of `kind`.
pub(crate) fn new_track_label(kind: TrackKindRow) -> &'static str {
    match kind {
        TrackKindRow::Audio => "New Audio Track",
        TrackKindRow::Instrument => "New Instrument Track",
        TrackKindRow::Midi => "New MIDI Track",
    }
}

/// Where a new track goes: just below `after` when that names a track of
/// `seq`, else the end (`None`).
pub(crate) fn index_below(doc: &Document, seq: SeqId, after: Option<u64>) -> Option<usize> {
    let after = LaneId(after?);
    let i = doc.sequence(seq).ok()?.lane_index(after)?;
    Some(i.saturating_add(1))
}

/// The name prefix of a new track of `kind`: "Audio", "Inst" or "MIDI"
/// ("Inst", not "Instrument", so the name fits the header's name field).
fn name_prefix(kind: TrackKindRow) -> &'static str {
    match kind {
        TrackKindRow::Audio => "Audio",
        TrackKindRow::Instrument => "Inst",
        TrackKindRow::Midi => "MIDI",
    }
}

/// "<prefix> N" with the smallest N no track of `seq` has, counted per
/// prefix, so "Audio 1", "Inst 1" and "MIDI 1" can coexist.
fn next_name(doc: &Document, seq: SeqId, kind: TrackKindRow) -> Result<String, VizError> {
    let s = doc.sequence(seq)?;
    let taken: std::collections::BTreeSet<&str> = s.lanes.iter().map(|l| l.name.as_str()).collect();
    let prefix = name_prefix(kind);
    let mut n = 1usize;
    loop {
        let name = format!("{prefix} {n}");
        if !taken.contains(name.as_str()) {
            debug_assert!(core_model::lane::valid_name(&name).is_some());
            return Ok(name);
        }
        n = n.saturating_add(1);
    }
}

/// Runs `cmd`; its report's warning lines are added to `notes`.
fn step(history: &mut History, notes: &mut Vec<String>, cmd: Command) -> Result<Report, VizError> {
    let r = history
        .execute(&cmd, &ViewFilter::default())
        .map_err(VizError::from)?;
    notes.extend(r.warnings.iter().cloned());
    Ok(r)
}

fn created_lane(r: &Report) -> Result<LaneId, VizError> {
    r.created
        .iter()
        .find_map(|id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })
        .ok_or_else(|| bad("no track was added"))
}

/// A new output path named `name` (`audio:1-2`, the values Track Synth
/// uses), with its own strip routed to master: each new audio track gets
/// its own mixer channel (owner, 2026-11-17; plan 8e Task 6c).
fn new_output_path(
    history: &mut History,
    notes: &mut Vec<String>,
    name: &str,
) -> Result<OutputPathId, VizError> {
    let r = step(
        history,
        notes,
        Command::Routing(RoutingCmd::AddOutput {
            name: name.into(),
            reference: "audio:1-2".into(),
        }),
    )?;
    r.created
        .iter()
        .find_map(|id| match id {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .ok_or_else(|| bad("no output path was added"))
}

/// The first built-in synth destination in id order, else a new "Built-in
/// Synth".
fn synth_dest(history: &mut History, notes: &mut Vec<String>) -> Result<DestId, VizError> {
    let found = history
        .doc()
        .instruments()
        .values()
        .find(|d| d.kind.is_builtin_synth())
        .map(|d| d.id);
    if let Some(d) = found {
        return Ok(d);
    }
    let r = step(
        history,
        notes,
        Command::Routing(RoutingCmd::AddDestination {
            name: "Built-in Synth".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: BUILTIN_SYNTH_REF.into(),
                group: 0,
                channel: 0,
            },
            default_patch: None,
        }),
    )?;
    r.created
        .iter()
        .find_map(|id| match id {
            AnyId::Dest(d) => Some(*d),
            _ => None,
        })
        .ok_or_else(|| bad("no destination was added"))
}

/// Makes a track of `kind` in `seq` (at `index`, else last) from the
/// existing commands, inside the caller's open transaction (the caller
/// aborts it on error). Every kind gets the monitor `monitor`; Audio the first input
/// channel and a new output path of its own, named after it, whose strip
/// goes to master; Instrument the built-in synth; MIDI the
/// first external destination, if any. Returns the new lane; the steps'
/// warning lines (a default EQ's fallback note, plan 6g-2 Ruling 4) are
/// added to `notes`.
pub(crate) fn build_track(
    history: &mut History,
    seq: SeqId,
    kind: TrackKindRow,
    index: Option<usize>,
    monitor: MonitorMode,
    notes: &mut Vec<String>,
) -> Result<LaneId, VizError> {
    build(history, seq, kind, index, monitor, None, notes)
}

/// The longest name a new plug-in instrument destination gets (plan 8d
/// Ruling 1).
const MAX_DEST_NAME: usize = 64;

/// `plugin`'s name as a destination's: cut to [`MAX_DEST_NAME`]
/// characters, with what the preview would not show as a space, trimmed;
/// "Plug-in Instrument" when nothing is left.
fn dest_name(plugin: &str) -> String {
    let clean: String = plugin
        .chars()
        .take(MAX_DEST_NAME)
        .map(|c| {
            let mut b = [0u8; 4];
            let one: &str = c.encode_utf8(&mut b);
            if core_model::diff::shown_name(one) == one {
                c
            } else {
                ' '
            }
        })
        .collect();
    match clean.trim() {
        "" => "Plug-in Instrument".into(),
        t => t.into(),
    }
}

/// `track.new_plugin_instrument` (plan 8d Ruling 1): a plug-in instrument
/// destination playing `plugin`, named after it (and its instrument
/// strip), then an instrument track playing it, as [`build_track`] makes
/// an instrument track; inside the caller's open transaction.
pub(crate) fn build_plugin_instrument_track(
    history: &mut History,
    seq: SeqId,
    plugin: PluginRef,
    index: Option<usize>,
    monitor: MonitorMode,
    notes: &mut Vec<String>,
) -> Result<LaneId, VizError> {
    let name = dest_name(&plugin.name);
    let r = step(
        history,
        notes,
        Command::Routing(RoutingCmd::AddDestination {
            name,
            kind: DestKind::PluginInstrument {
                slot: PluginSlot::new(PluginInstanceId(0), plugin),
            },
            default_patch: None,
        }),
    )?;
    let dest = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Dest(d) => Some(*d),
            _ => None,
        })
        .ok_or_else(|| bad("no destination was added"))?;
    build(
        history,
        seq,
        TrackKindRow::Instrument,
        index,
        monitor,
        Some(dest),
        notes,
    )
}

/// [`build_track`], with an instrument track playing `dest` when given
/// (else the built-in synth).
fn build(
    history: &mut History,
    seq: SeqId,
    kind: TrackKindRow,
    index: Option<usize>,
    monitor: MonitorMode,
    dest: Option<DestId>,
    notes: &mut Vec<String>,
) -> Result<LaneId, VizError> {
    let name = next_name(history.doc(), seq, kind)?;
    let lane = created_lane(&step(
        history,
        notes,
        Command::Lane(LaneCmd::Add {
            seq,
            name: name.clone(),
            index,
        }),
    )?)?;
    // Every kind takes the "Monitoring for new tracks" default.
    step(
        history,
        notes,
        Command::Lane(LaneCmd::SetMonitor { seq, lane, monitor }),
    )?;
    match kind {
        TrackKindRow::Audio => {
            step(
                history,
                notes,
                Command::Lane(LaneCmd::SetInput {
                    seq,
                    lane,
                    input: Some(LaneInput::Audio {
                        channels: InputChannels::Mono(0),
                        return_of: None,
                    }),
                }),
            )?;
            let output = Some(new_output_path(history, notes, &name)?);
            step(
                history,
                notes,
                Command::Lane(LaneCmd::SetOutput { seq, lane, output }),
            )?;
        }
        TrackKindRow::Instrument => {
            let dest = match dest {
                Some(d) => Some(d),
                None => Some(synth_dest(history, notes)?),
            };
            step(
                history,
                notes,
                Command::Lane(LaneCmd::SetDest { seq, lane, dest }),
            )?;
        }
        TrackKindRow::Midi => {
            let dest = history
                .doc()
                .instruments()
                .values()
                .find(|d| d.kind.is_external())
                .map(|d| d.id);
            if dest.is_some() {
                step(
                    history,
                    notes,
                    Command::Lane(LaneCmd::SetDest { seq, lane, dest }),
                )?;
            }
        }
    }
    if kind != TrackKindRow::Audio {
        step(
            history,
            notes,
            Command::Lane(LaneCmd::SetInput {
                seq,
                lane,
                input: Some(LaneInput::Midi {
                    port: None,
                    channel: None,
                }),
            }),
        )?;
    }
    Ok(lane)
}
