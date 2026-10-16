//! Snapshot records (copied to Swift) and the queries that build them.

use crate::error::VizError;
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::SeqId;
use core_model::query::length::seq_length;
use core_model::query::usage::{unresolved_events, use_counts};
use core_model::time::Tick;

/// Display units per beat in `TransportState.position_bbu` ("5.2.480" is
/// halfway through beat 2 of bar 5).
pub const UNITS_PER_BEAT: u32 = 960;

/// The document at sequence level.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DocSnapshot {
    /// Increases with every applied command, undo and redo; never saved.
    pub version: u64,
    /// File name without `.vizdaw`, or "Untitled".
    pub title: String,
    /// Differs from the last opened or saved document.
    pub dirty: bool,
    /// Label of the step Undo would revert.
    pub undo_label: Option<String>,
    /// Label of the step Redo would reapply.
    pub redo_label: Option<String>,
    /// Every sequence, in ID order.
    pub sequences: Vec<SeqRow>,
    /// Problems worth showing (unresolved references).
    pub warnings: Vec<String>,
}

/// One sequence.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SeqRow {
    /// Sequence ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// Number of reference events that use it.
    pub use_count: u32,
    /// Not used by any reference.
    pub top_level: bool,
    /// Length in ticks.
    pub length_ticks: i64,
    /// Number of lanes.
    pub lane_count: u32,
    /// Comment.
    pub comment: String,
}

/// One lane of a sequence.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LaneRow {
    /// Lane ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// Name of the lane's MIDI destination.
    pub destination: Option<String>,
    /// Events in the active take other than references and audio regions.
    pub midi_event_count: u32,
    /// Audio regions in the active take.
    pub region_count: u32,
    /// References in the active take.
    pub reference_count: u32,
    /// Muted.
    pub mute: bool,
    /// Soloed.
    pub solo: bool,
    /// Loops to the sequence end.
    pub looped: bool,
    /// Number of takes.
    pub take_count: u32,
    /// Name of the active take.
    pub active_take: String,
    /// Has a drum layout.
    pub is_drum: bool,
}

/// What the transport is doing.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TransportState {
    /// Playing (including the release tail after the end).
    pub playing: bool,
    /// The sequence loaded in the engine for this session.
    pub seq: Option<u64>,
    /// Position in ticks.
    pub tick: i64,
    /// Position as bar.beat.unit, [`UNITS_PER_BEAT`] units per beat.
    pub position_bbu: String,
    /// Position in seconds from bar 1 (negative in a pickup).
    pub seconds: f64,
    /// Stream sample rate (0 before the first Play).
    pub sample_rate: f64,
    /// Output device name, "Offline", or "" before the first Play.
    pub device: String,
    /// Real-time faults since the last Play.
    pub faults: Vec<String>,
    /// Messages of the last compile report.
    pub report: Vec<String>,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Converts an FFI ID back to a sequence ID, rejecting unknown ones.
pub fn seq_id(doc: &Document, seq: u64) -> Result<SeqId, VizError> {
    let id = SeqId(seq);
    doc.sequence(id)?;
    Ok(id)
}

/// The sequence rows of `doc`.
pub fn seq_rows(doc: &Document) -> Result<Vec<SeqRow>, VizError> {
    let uses = use_counts(doc);
    doc.sequences()
        .values()
        .map(|s| {
            let use_count = uses.get(&s.id).copied().unwrap_or(0);
            Ok(SeqRow {
                id: s.id.0,
                name: s.name.clone(),
                use_count: count(use_count),
                top_level: use_count == 0,
                length_ticks: seq_length(doc, s.id)?.0,
                lane_count: count(s.lanes.len()),
                comment: s.comment.clone(),
            })
        })
        .collect()
}

/// The lane rows of sequence `seq`.
pub fn lane_rows(doc: &Document, seq: u64) -> Result<Vec<LaneRow>, VizError> {
    let s = doc.sequence(seq_id(doc, seq)?)?;
    Ok(s.lanes
        .iter()
        .map(|l| {
            let (mut midi, mut regions, mut refs) = (0usize, 0usize, 0usize);
            if let Some(take) = l.active() {
                for ev in take.events.values() {
                    match ev.body {
                        EventBody::Reference { .. } => refs += 1,
                        EventBody::AudioRegion { .. } => regions += 1,
                        _ => midi += 1,
                    }
                }
            }
            LaneRow {
                id: l.id.0,
                name: l.name.clone(),
                destination: l
                    .midi_dest
                    .and_then(|d| doc.instruments().get(&d).map(|d| d.name.clone())),
                midi_event_count: count(midi),
                region_count: count(regions),
                reference_count: count(refs),
                mute: l.mute,
                solo: l.solo,
                looped: l.looped,
                take_count: count(l.takes.len()),
                active_take: l.active().map(|t| t.name.clone()).unwrap_or_default(),
                is_drum: l.drum.is_some(),
            }
        })
        .collect())
}

/// Problems worth showing for `doc`.
pub fn warnings(doc: &Document) -> Vec<String> {
    let n = unresolved_events(doc).len();
    if n == 0 {
        Vec::new()
    } else {
        vec![format!("{n} reference(s) point to missing sequences")]
    }
}

/// `tick` in `seq` as "bar.beat.unit".
pub fn position_bbu(doc: &Document, seq: SeqId, tick: Tick) -> String {
    doc.sequence(seq).map_or_else(
        |_| String::new(),
        |s| {
            let p = s.meter.tick_to_bbu(tick, UNITS_PER_BEAT);
            format!("{}.{}.{}", p.bar, p.beat, p.unit)
        },
    )
}

/// `tick` in `seq` as seconds from bar 1, through the sequence's tempo map.
pub fn seconds(doc: &Document, seq: SeqId, tick: Tick) -> f64 {
    doc.sequence(seq)
        .map_or(0.0, |s| s.tempo.tick_to_seconds(tick))
}
