//! Event rows (spec §3.1): a lane's active take as flat records the app
//! copies, and rows back into event specs for inserts (Plan Rulings 3, 4).
//!
//! `value` / `value2` by kind:
//! - `Cc`: `value` = 32-bit value, `value2` = controller number;
//! - `PitchBend`, `ChannelPressure`, `PolyPressure`: `value` = 32-bit value;
//! - `Program`: `value` = program, `value2` = bank as msb × 128 + lsb, or
//!   -1 for none;
//! - every other kind: 0.
//!
//! `duration` is the note length, a reference's span, or a region's length
//! in ticks through the sequence's tempo map; inserts ignore it for
//! references and regions (they use `ref_*` and `frames`).

use crate::error::VizError;
use crate::snapshot::KeySlotRow;
use core_model::automation::{AutomationTarget, SendIndex};
use core_model::document::Document;
use core_model::event::{
    Event, EventBody, EventSpec, Fade, FadeShape, LoopCount, NoteAttribute, PerNoteCtl,
    PerNoteKind, TextKind,
};
use core_model::ids::{DestId, LaneId, MediaId, SeqId};
use core_model::ids::{PluginInstanceId, StripId};
use core_model::mixer::ParamId;
use core_model::query::length::{reference_span, seq_length};
use core_model::sequence::SeqKind;
use core_model::tempo::TempoMap;
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Event kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum EventKindRow {
    /// A note.
    Note,
    /// A control change.
    Cc,
    /// Pitch bend.
    PitchBend,
    /// Channel pressure.
    ChannelPressure,
    /// Polyphonic pressure.
    PolyPressure,
    /// Program change.
    Program,
    /// System exclusive.
    SysEx,
    /// A text meta-event.
    Text,
    /// A reference to another sequence.
    Reference,
    /// An audio region.
    AudioRegion,
    /// An automation point (4d).
    Automation,
}

/// What an automation event drives (core `AutomationTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AutomationTargetRow {
    /// A strip's fader.
    StripFader {
        /// Strip ID.
        strip: u64,
    },
    /// A strip's pan.
    StripPan {
        /// Strip ID.
        strip: u64,
    },
    /// A strip's mute.
    StripMute {
        /// Strip ID.
        strip: u64,
    },
    /// A strip's solo.
    StripSolo {
        /// Strip ID.
        strip: u64,
    },
    /// A send's level.
    SendLevel {
        /// Strip ID.
        strip: u64,
        /// The send's position in the strip's sends.
        send: u16,
    },
    /// A plugin parameter.
    PluginParam {
        /// Plugin instance (slot) ID.
        slot: u64,
        /// Stable parameter ID.
        param: u64,
    },
    /// A destination's velocity scale.
    VelocityScale {
        /// Destination ID.
        dest: u64,
    },
}

/// An automation point's target and normalised value (0–1).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct AutomationRow {
    /// What it drives.
    pub target: AutomationTargetRow,
    /// Normalised value.
    pub value: f64,
}

/// The row of a core target.
pub fn automation_target_row(t: AutomationTarget) -> AutomationTargetRow {
    match t {
        AutomationTarget::StripFader(s) => AutomationTargetRow::StripFader { strip: s.0 },
        AutomationTarget::StripPan(s) => AutomationTargetRow::StripPan { strip: s.0 },
        AutomationTarget::StripMute(s) => AutomationTargetRow::StripMute { strip: s.0 },
        AutomationTarget::StripSolo(s) => AutomationTargetRow::StripSolo { strip: s.0 },
        AutomationTarget::SendLevel(s, i) => AutomationTargetRow::SendLevel {
            strip: s.0,
            send: i.0,
        },
        AutomationTarget::PluginParam(p, id) => AutomationTargetRow::PluginParam {
            slot: p.0,
            param: id.0,
        },
        AutomationTarget::VelocityScale(d) => AutomationTargetRow::VelocityScale { dest: d.0 },
    }
}

/// The core target of a row.
pub fn automation_target_of(t: AutomationTargetRow) -> AutomationTarget {
    match t {
        AutomationTargetRow::StripFader { strip } => AutomationTarget::StripFader(StripId(strip)),
        AutomationTargetRow::StripPan { strip } => AutomationTarget::StripPan(StripId(strip)),
        AutomationTargetRow::StripMute { strip } => AutomationTarget::StripMute(StripId(strip)),
        AutomationTargetRow::StripSolo { strip } => AutomationTarget::StripSolo(StripId(strip)),
        AutomationTargetRow::SendLevel { strip, send } => {
            AutomationTarget::SendLevel(StripId(strip), SendIndex(send))
        }
        AutomationTargetRow::PluginParam { slot, param } => {
            AutomationTarget::PluginParam(PluginInstanceId(slot), ParamId(param))
        }
        AutomationTargetRow::VelocityScale { dest } => {
            AutomationTarget::VelocityScale(DestId(dest))
        }
    }
}

/// Text meta-event kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum TextKindRow {
    /// Generic text.
    Text,
    /// Lyric.
    Lyric,
    /// Cue point.
    Cue,
    /// Marker.
    Marker,
    /// Copyright notice.
    Copyright,
}

/// Fade curve shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FadeShapeRow {
    /// Linear gain.
    Linear,
    /// Equal power.
    EqualPower,
    /// Exponential.
    Exponential,
    /// S-curve.
    SCurve,
}

/// A fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct FadeRow {
    /// Length in ticks.
    pub length: i64,
    /// Shape.
    pub shape: FadeShapeRow,
}

/// A UMP group and channel override, each 0–15.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct GroupChannel {
    /// UMP group.
    pub group: u8,
    /// Channel.
    pub channel: u8,
}

/// A MIDI 2.0 note attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct NoteAttributeRow {
    /// Attribute type.
    pub kind: u8,
    /// Attribute data.
    pub data: u16,
}

/// Per-note controller kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum PerNoteKindRow {
    /// Registered per-note controller.
    Registered {
        /// Controller number.
        index: u8,
    },
    /// Assignable per-note controller.
    Assignable {
        /// Controller number.
        index: u8,
    },
    /// Per-note pitch bend.
    PitchBend,
}

/// One per-note controller value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct PerNotePoint {
    /// Which controller.
    pub kind: PerNoteKindRow,
    /// Offset from the note start in ticks.
    pub offset: i64,
    /// 32-bit value.
    pub value: u32,
}

/// A per-note value at an absolute tick, as editors write them
/// (`EventEdit::SetPerNote`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct PerNoteValueRow {
    /// Absolute tick.
    pub tick: i64,
    /// 32-bit value.
    pub value: u32,
}

/// One event as a flat record; fields a kind does not use are zero,
/// empty or `None` (see the module documentation).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct EventRow {
    /// Event ID (ignored by inserts).
    pub id: u64,
    /// Start tick.
    pub tick: i64,
    /// Kind.
    pub kind: EventKindRow,
    /// Muted.
    pub mute: bool,
    /// Destination override.
    pub dest: Option<u64>,
    /// Group/channel override.
    pub group_channel: Option<GroupChannel>,
    /// Note or poly-pressure pitch.
    pub pitch: u8,
    /// 16-bit note velocity.
    pub velocity: u16,
    /// 16-bit release velocity.
    pub release_velocity: u16,
    /// Length in ticks (note length, reference span, region length).
    pub duration: i64,
    /// Kind-specific value.
    pub value: i64,
    /// Second kind-specific value.
    pub value2: i64,
    /// Note attribute.
    pub attribute: Option<NoteAttributeRow>,
    /// Per-note controllers.
    pub per_note: Vec<PerNotePoint>,
    /// Referenced sequence.
    pub ref_seq: Option<u64>,
    /// Per-use length of a reference (`None`: the sequence's length).
    pub ref_length: Option<i64>,
    /// Reference repeats; 0 means Fill.
    pub ref_loops: u32,
    /// Reference transpose in semitones.
    pub ref_transpose: i8,
    /// Region media item.
    pub media: Option<u64>,
    /// Region start within the media, in frames.
    pub source_offset: u64,
    /// Region length in frames.
    pub frames: u64,
    /// Region gain in dB.
    pub gain_db: f32,
    /// Region fade in.
    pub fade_in: FadeRow,
    /// Region fade out.
    pub fade_out: FadeRow,
    /// Text of a text event.
    pub text: Option<String>,
    /// Kind of a text event.
    pub text_kind: Option<TextKindRow>,
    /// SysEx payload (without F0/F7).
    pub data: Vec<u8>,
    /// SysEx may hold 8-bit data.
    pub eight_bit: bool,
    /// An automation point's target and value (4d; `value` then holds the
    /// value on the 32-bit scale, Plan 4d Ruling 25).
    #[uniffi(default = None)]
    pub automation: Option<AutomationRow>,
}

const NO_FADE: FadeRow = FadeRow {
    length: 0,
    shape: FadeShapeRow::Linear,
};

impl EventRow {
    /// A row of `kind` at `tick` with every other field zero, empty or
    /// `None`.
    pub fn blank(id: u64, tick: i64, kind: EventKindRow) -> EventRow {
        EventRow {
            id,
            tick,
            kind,
            mute: false,
            dest: None,
            group_channel: None,
            pitch: 0,
            velocity: 0,
            release_velocity: 0,
            duration: 0,
            value: 0,
            value2: 0,
            attribute: None,
            per_note: Vec::new(),
            ref_seq: None,
            ref_length: None,
            ref_loops: 0,
            ref_transpose: 0,
            media: None,
            source_offset: 0,
            frames: 0,
            gain_db: 0.0,
            fade_in: NO_FADE,
            fade_out: NO_FADE,
            text: None,
            text_kind: None,
            data: Vec::new(),
            eight_bit: false,
            automation: None,
        }
    }
}

/// A lane's active take as rows, in time order.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LaneEvents {
    /// Sequence ID.
    pub seq: u64,
    /// Lane ID.
    pub lane: u64,
    /// The lane's revision these rows belong to.
    pub rev: u64,
    /// The rows.
    pub events: Vec<EventRow>,
}

/// One tempo point.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct TempoPointRow {
    /// Where it starts.
    pub tick: i64,
    /// Beats per minute.
    pub bpm: f64,
    /// Ramps linearly to the next point.
    pub ramp: bool,
}

/// One meter (and key) change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct MeterEntryRow {
    /// Where it takes effect (a barline).
    pub tick: i64,
    /// Beats per bar.
    pub num: u8,
    /// Beat unit as a note value (4 = quarter).
    pub den: u8,
    /// Key: sharps (negative for flats).
    pub sharps: i8,
    /// Key: minor.
    pub minor: bool,
}

/// A named position.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct MarkerRow {
    /// Position.
    pub tick: i64,
    /// Name.
    pub name: String,
}

/// What rulers and headers need about a sequence.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SequenceInfo {
    /// Sequence ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// Comment.
    pub comment: String,
    /// Keyboard slot.
    pub keyboard_slot: Option<KeySlotRow>,
    /// Pickup length before bar 1.
    pub start_offset: i64,
    /// Playing length in ticks.
    pub length_ticks: i64,
    /// Tempo map.
    pub tempo: Vec<TempoPointRow>,
    /// Meter map (the first entry is at tick 0).
    pub meter: Vec<MeterEntryRow>,
    /// Markers.
    pub markers: Vec<MarkerRow>,
    /// A generated sequence.
    pub generated: bool,
}

/// A `VizError::Model` with `message`.
pub fn bad(message: &str) -> VizError {
    VizError::Model {
        message: message.to_string(),
    }
}

/// Reference repeats from the FFI convention (0 = Fill).
pub fn loops(n: u32) -> LoopCount {
    if n == 0 {
        LoopCount::Fill
    } else {
        LoopCount::Count(n)
    }
}

fn narrow<T: TryFrom<i64>>(v: i64, what: &str) -> Result<T, VizError> {
    T::try_from(v).map_err(|_| bad(&format!("{what} {v} is out of range")))
}

fn kind_row(b: &EventBody) -> EventKindRow {
    match b {
        EventBody::Note { .. } => EventKindRow::Note,
        EventBody::Cc { .. } => EventKindRow::Cc,
        EventBody::PitchBend { .. } => EventKindRow::PitchBend,
        EventBody::ChannelPressure { .. } => EventKindRow::ChannelPressure,
        EventBody::PolyPressure { .. } => EventKindRow::PolyPressure,
        EventBody::Program { .. } => EventKindRow::Program,
        EventBody::SysEx { .. } => EventKindRow::SysEx,
        EventBody::Text { .. } => EventKindRow::Text,
        EventBody::Reference { .. } => EventKindRow::Reference,
        EventBody::AudioRegion { .. } => EventKindRow::AudioRegion,
        EventBody::Automation { .. } => EventKindRow::Automation,
    }
}

/// The core kind of an FFI kind.
pub fn kind_of(k: EventKindRow) -> core_model::event::EventKind {
    use core_model::event::EventKind as K;
    match k {
        EventKindRow::Note => K::Note,
        EventKindRow::Cc => K::Cc,
        EventKindRow::PitchBend => K::PitchBend,
        EventKindRow::ChannelPressure => K::ChannelPressure,
        EventKindRow::PolyPressure => K::PolyPressure,
        EventKindRow::Program => K::Program,
        EventKindRow::SysEx => K::SysEx,
        EventKindRow::Text => K::Text,
        EventKindRow::Reference => K::Reference,
        EventKindRow::AudioRegion => K::AudioRegion,
        EventKindRow::Automation => K::Automation,
    }
}

fn text_kind_row(k: TextKind) -> TextKindRow {
    match k {
        TextKind::Text => TextKindRow::Text,
        TextKind::Lyric => TextKindRow::Lyric,
        TextKind::Cue => TextKindRow::Cue,
        TextKind::Marker => TextKindRow::Marker,
        TextKind::Copyright => TextKindRow::Copyright,
    }
}

fn text_kind_of(k: TextKindRow) -> TextKind {
    match k {
        TextKindRow::Text => TextKind::Text,
        TextKindRow::Lyric => TextKind::Lyric,
        TextKindRow::Cue => TextKind::Cue,
        TextKindRow::Marker => TextKind::Marker,
        TextKindRow::Copyright => TextKind::Copyright,
    }
}

/// A fade as a row.
pub fn fade_row(f: Fade) -> FadeRow {
    FadeRow {
        length: f.length.0,
        shape: match f.shape {
            FadeShape::Linear => FadeShapeRow::Linear,
            FadeShape::EqualPower => FadeShapeRow::EqualPower,
            FadeShape::Exponential => FadeShapeRow::Exponential,
            FadeShape::SCurve => FadeShapeRow::SCurve,
        },
    }
}

/// A fade from a row.
pub fn fade_of(f: &FadeRow) -> Fade {
    Fade {
        length: Tick(f.length),
        shape: match f.shape {
            FadeShapeRow::Linear => FadeShape::Linear,
            FadeShapeRow::EqualPower => FadeShape::EqualPower,
            FadeShapeRow::Exponential => FadeShape::Exponential,
            FadeShapeRow::SCurve => FadeShape::SCurve,
        },
    }
}

fn per_note_row(p: &PerNoteCtl) -> PerNotePoint {
    PerNotePoint {
        kind: match p.kind {
            PerNoteKind::Registered(index) => PerNoteKindRow::Registered { index },
            PerNoteKind::Assignable(index) => PerNoteKindRow::Assignable { index },
            PerNoteKind::PitchBend => PerNoteKindRow::PitchBend,
        },
        offset: p.offset.0,
        value: p.value,
    }
}

/// The core per-note kind of a row kind.
pub(crate) fn per_note_kind(k: PerNoteKindRow) -> PerNoteKind {
    match k {
        PerNoteKindRow::Registered { index } => PerNoteKind::Registered(index),
        PerNoteKindRow::Assignable { index } => PerNoteKind::Assignable(index),
        PerNoteKindRow::PitchBend => PerNoteKind::PitchBend,
    }
}

fn per_note_of(p: &PerNotePoint) -> PerNoteCtl {
    PerNoteCtl {
        kind: per_note_kind(p.kind),
        offset: Tick(p.offset),
        value: p.value,
    }
}

/// Ticks covered by `frames` of `media` starting at `tick`.
fn region_ticks(doc: &Document, tempo: &TempoMap, tick: Tick, media: MediaId, frames: u64) -> i64 {
    let rate = doc
        .media()
        .get(&media)
        .map_or(48_000, |m| m.sample_rate.max(1));
    let start = tempo.tick_to_seconds(tick);
    let end = tempo.seconds_to_tick(start + frames as f64 / f64::from(rate));
    (end - tick).0.max(0)
}

/// One event as a row. `spans` memoises referenced sequences' lengths.
pub fn row(
    doc: &Document,
    tempo: &TempoMap,
    spans: &mut BTreeMap<SeqId, Tick>,
    ev: &Event,
) -> EventRow {
    let mut r = EventRow::blank(ev.id.0, ev.tick.0, kind_row(&ev.body));
    r.mute = ev.mute;
    r.dest = ev.dest.map(|d| d.0);
    r.group_channel = ev
        .group_channel
        .map(|(group, channel)| GroupChannel { group, channel });
    match &ev.body {
        EventBody::Note {
            pitch,
            velocity,
            release_velocity,
            duration,
            attribute,
            per_note,
        } => {
            r.pitch = *pitch;
            r.velocity = *velocity;
            r.release_velocity = *release_velocity;
            r.duration = duration.0;
            r.attribute = attribute.map(|a| NoteAttributeRow {
                kind: a.kind,
                data: a.data,
            });
            r.per_note = per_note.iter().map(per_note_row).collect();
        }
        EventBody::Cc { index, value } => {
            r.value = i64::from(*value);
            r.value2 = i64::from(*index);
        }
        EventBody::PitchBend { value } | EventBody::ChannelPressure { value } => {
            r.value = i64::from(*value);
        }
        EventBody::PolyPressure { pitch, value } => {
            r.pitch = *pitch;
            r.value = i64::from(*value);
        }
        EventBody::Program { program, bank } => {
            r.value = i64::from(*program);
            r.value2 = bank.map_or(-1, |(msb, lsb)| i64::from(msb) * 128 + i64::from(lsb));
        }
        EventBody::SysEx { data, eight_bit } => {
            r.data = data.to_vec();
            r.eight_bit = *eight_bit;
        }
        EventBody::Text { kind, text } => {
            r.text_kind = Some(text_kind_row(*kind));
            r.text = Some(text.clone());
        }
        EventBody::Reference {
            seq,
            length,
            loop_count,
            transpose,
        } => {
            r.ref_seq = Some(seq.0);
            r.ref_length = length.map(|t| t.0);
            r.ref_loops = match loop_count {
                LoopCount::Count(n) => (*n).max(1),
                LoopCount::Fill => 0,
            };
            r.ref_transpose = *transpose;
            let seq_len = *spans
                .entry(*seq)
                .or_insert_with(|| seq_length(doc, *seq).unwrap_or(Tick::ZERO));
            r.duration = reference_span(seq_len, *length, *loop_count).0;
        }
        EventBody::AudioRegion {
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
        } => {
            r.media = Some(media.0);
            r.source_offset = *source_offset;
            r.frames = *length;
            r.gain_db = *gain_db;
            r.fade_in = fade_row(*fade_in);
            r.fade_out = fade_row(*fade_out);
            r.duration = region_ticks(doc, tempo, ev.tick, *media, *length);
        }
        EventBody::Automation { target, value } => {
            r.automation = Some(AutomationRow {
                target: automation_target_row(*target),
                value: *value,
            });
            r.value = i64::from(core_model::automation::to_u32(*value));
        }
    }
    r
}

/// A row as an event spec, for inserts. The core validates the result
/// (ranges, destinations, reference cycles) when it is inserted.
pub fn spec(row: &EventRow) -> Result<EventSpec, VizError> {
    let body = match row.kind {
        EventKindRow::Note => EventBody::Note {
            pitch: row.pitch,
            velocity: row.velocity,
            release_velocity: row.release_velocity,
            duration: Tick(row.duration),
            attribute: row.attribute.map(|a| NoteAttribute {
                kind: a.kind,
                data: a.data,
            }),
            per_note: row.per_note.iter().map(per_note_of).collect(),
        },
        EventKindRow::Cc => EventBody::Cc {
            index: narrow(row.value2, "controller")?,
            value: narrow(row.value, "value")?,
        },
        EventKindRow::PitchBend => EventBody::PitchBend {
            value: narrow(row.value, "value")?,
        },
        EventKindRow::ChannelPressure => EventBody::ChannelPressure {
            value: narrow(row.value, "value")?,
        },
        EventKindRow::PolyPressure => EventBody::PolyPressure {
            pitch: row.pitch,
            value: narrow(row.value, "value")?,
        },
        EventKindRow::Program => EventBody::Program {
            program: narrow(row.value, "program")?,
            bank: if row.value2 < 0 {
                None
            } else {
                Some((
                    narrow(row.value2 / 128, "bank")?,
                    narrow(row.value2 % 128, "bank")?,
                ))
            },
        },
        EventKindRow::SysEx => EventBody::SysEx {
            data: Arc::from(row.data.as_slice()),
            eight_bit: row.eight_bit,
        },
        EventKindRow::Text => EventBody::Text {
            kind: text_kind_of(row.text_kind.unwrap_or(TextKindRow::Text)),
            text: row.text.clone().unwrap_or_default(),
        },
        EventKindRow::Reference => EventBody::Reference {
            seq: SeqId(
                row.ref_seq
                    .ok_or_else(|| bad("a reference row needs ref_seq"))?,
            ),
            length: row.ref_length.map(Tick),
            loop_count: loops(row.ref_loops),
            transpose: row.ref_transpose,
        },
        EventKindRow::AudioRegion => EventBody::AudioRegion {
            media: MediaId(row.media.ok_or_else(|| bad("a region row needs media"))?),
            source_offset: row.source_offset,
            length: row.frames,
            gain_db: row.gain_db,
            fade_in: fade_of(&row.fade_in),
            fade_out: fade_of(&row.fade_out),
        },
        EventKindRow::Automation => {
            let a = row
                .automation
                .ok_or_else(|| bad("an automation row needs its automation record"))?;
            EventBody::Automation {
                target: automation_target_of(a.target),
                value: a.value,
            }
        }
    };
    Ok(EventSpec {
        tick: Tick(row.tick),
        dest: row.dest.map(DestId),
        group_channel: row.group_channel.map(|g| (g.group, g.channel)),
        mute: row.mute,
        body,
    })
}

/// The active take of `lane` as rows, tagged with revision `rev`.
pub fn lane_events(
    doc: &Document,
    seq: SeqId,
    lane: LaneId,
    rev: u64,
) -> Result<LaneEvents, VizError> {
    let s = doc.sequence(seq)?;
    let l = doc.lane(seq, lane)?;
    let mut spans = BTreeMap::new();
    let events = l
        .active()
        .map(|t| {
            t.events
                .values()
                .map(|e| row(doc, &s.tempo, &mut spans, e))
                .collect()
        })
        .unwrap_or_default();
    Ok(LaneEvents {
        seq: seq.0,
        lane: lane.0,
        rev,
        events,
    })
}

/// What rulers need about `seq`.
pub fn sequence_info(doc: &Document, seq: SeqId) -> Result<SequenceInfo, VizError> {
    let s = doc.sequence(seq)?;
    Ok(SequenceInfo {
        id: s.id.0,
        name: s.name.clone(),
        comment: s.comment.clone(),
        keyboard_slot: s.keyboard_slot.as_ref().map(|k| KeySlotRow {
            key: k.key.clone(),
            modifiers: k.modifiers,
        }),
        start_offset: s.start_offset.0,
        length_ticks: seq_length(doc, seq)?.0,
        tempo: s
            .tempo
            .points()
            .iter()
            .map(|p| TempoPointRow {
                tick: p.tick.0,
                bpm: p.bpm.as_f64(),
                ramp: p.ramp,
            })
            .collect(),
        meter: s
            .meter
            .entries()
            .iter()
            .map(|e| MeterEntryRow {
                tick: e.tick.0,
                num: e.meter.num,
                den: e.meter.den,
                sharps: e.key.sharps,
                minor: e.key.minor,
            })
            .collect(),
        markers: s
            .markers
            .iter()
            .map(|m| MarkerRow {
                tick: m.tick.0,
                name: m.name.clone(),
            })
            .collect(),
        generated: matches!(s.kind, SeqKind::Generated { .. }),
    })
}
