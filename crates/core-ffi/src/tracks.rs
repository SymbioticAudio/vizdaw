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
use core_model::ids::{AnyId, DestId, LaneId, OutputPathId, SeqId};
use core_model::lane::{InputChannels, LaneInput, MonitorMode};
use core_model::mixer::BUILTIN_SYNTH_REF;
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

/// "Track N" with the smallest N no track of `seq` has.
fn next_name(doc: &Document, seq: SeqId) -> Result<String, VizError> {
    let s = doc.sequence(seq)?;
    let taken: std::collections::BTreeSet<&str> = s.lanes.iter().map(|l| l.name.as_str()).collect();
    let mut n = 1usize;
    loop {
        let name = format!("Track {n}");
        if !taken.contains(name.as_str()) {
            return Ok(name);
        }
        n = n.saturating_add(1);
    }
}

fn step(history: &mut History, cmd: Command) -> Result<Report, VizError> {
    history
        .execute(&cmd, &ViewFilter::default())
        .map_err(VizError::from)
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

/// The first output path in id order, else a new "Audio 1" (`audio:1-2`,
/// the values Track Synth uses).
fn output_path(history: &mut History) -> Result<OutputPathId, VizError> {
    if let Some(o) = history.doc().outputs().keys().next() {
        return Ok(*o);
    }
    let r = step(
        history,
        Command::Routing(RoutingCmd::AddOutput {
            name: "Audio 1".into(),
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
fn synth_dest(history: &mut History) -> Result<DestId, VizError> {
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
/// aborts it on error). Audio gets the monitor `monitor`, the first input
/// channel and an output path; Instrument the built-in synth; MIDI the
/// first external destination, if any. Returns the new lane.
pub(crate) fn build_track(
    history: &mut History,
    seq: SeqId,
    kind: TrackKindRow,
    index: Option<usize>,
    monitor: MonitorMode,
) -> Result<LaneId, VizError> {
    let name = next_name(history.doc(), seq)?;
    let lane = created_lane(&step(
        history,
        Command::Lane(LaneCmd::Add { seq, name, index }),
    )?)?;
    match kind {
        TrackKindRow::Audio => {
            step(
                history,
                Command::Lane(LaneCmd::SetMonitor { seq, lane, monitor }),
            )?;
            step(
                history,
                Command::Lane(LaneCmd::SetInput {
                    seq,
                    lane,
                    input: Some(LaneInput::Audio {
                        channels: InputChannels::Mono(0),
                        return_of: None,
                    }),
                }),
            )?;
            let output = Some(output_path(history)?);
            step(
                history,
                Command::Lane(LaneCmd::SetOutput { seq, lane, output }),
            )?;
        }
        TrackKindRow::Instrument => {
            let dest = Some(synth_dest(history)?);
            step(history, Command::Lane(LaneCmd::SetDest { seq, lane, dest }))?;
        }
        TrackKindRow::Midi => {
            let dest = history
                .doc()
                .instruments()
                .values()
                .find(|d| d.kind.is_external())
                .map(|d| d.id);
            if dest.is_some() {
                step(history, Command::Lane(LaneCmd::SetDest { seq, lane, dest }))?;
            }
        }
    }
    if kind != TrackKindRow::Audio {
        step(
            history,
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
