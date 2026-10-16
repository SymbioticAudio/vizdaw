//! Events: MIDI 2.0 channel-voice data, text, references and audio regions.

use crate::ids::{DestId, EventId, MediaId, SeqId};
use crate::time::Tick;
use im::Vector;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// One event on a lane's take.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    /// Document-unique ID.
    pub id: EventId,
    /// Start position.
    pub tick: Tick,
    /// Destination override (else the lane's MIDI destination).
    pub dest: Option<DestId>,
    /// `(group, channel)` override, each 0–15.
    pub group_channel: Option<(u8, u8)>,
    /// Per-event mute; on a reference it mutes that use.
    pub mute: bool,
    /// What the event is.
    pub body: EventBody,
}

/// An event without an ID, as passed to insert commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSpec {
    /// Start position.
    pub tick: Tick,
    /// Destination override.
    pub dest: Option<DestId>,
    /// `(group, channel)` override.
    pub group_channel: Option<(u8, u8)>,
    /// Mute flag.
    pub mute: bool,
    /// What the event is.
    pub body: EventBody,
}

impl EventSpec {
    /// A spec with no overrides and no mute.
    pub fn new(tick: Tick, body: EventBody) -> EventSpec {
        EventSpec {
            tick,
            dest: None,
            group_channel: None,
            mute: false,
            body,
        }
    }

    /// Gives the spec an ID.
    pub fn with_id(self, id: EventId) -> Event {
        Event {
            id,
            tick: self.tick,
            dest: self.dest,
            group_channel: self.group_channel,
            mute: self.mute,
            body: self.body,
        }
    }
}

impl Event {
    /// The event without its ID.
    pub fn spec(&self) -> EventSpec {
        EventSpec {
            tick: self.tick,
            dest: self.dest,
            group_channel: self.group_channel,
            mute: self.mute,
            body: self.body.clone(),
        }
    }

    /// The take key `(tick, id)` this event is stored under.
    pub fn key(&self) -> (Tick, EventId) {
        (self.tick, self.id)
    }
}

/// The payload of an event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
pub enum EventBody {
    /// A note with MIDI 2.0 velocities.
    Note {
        /// Note number 0–127.
        pitch: u8,
        /// 16-bit attack velocity.
        velocity: u16,
        /// 16-bit release velocity.
        release_velocity: u16,
        /// Length in ticks (>= 0).
        duration: Tick,
        /// Optional MIDI 2.0 note attribute.
        attribute: Option<NoteAttribute>,
        /// Per-note controllers and pitch bend.
        per_note: Vector<PerNoteCtl>,
    },
    /// Control change with a 32-bit value.
    Cc {
        /// Controller number 0–127.
        index: u8,
        /// 32-bit value.
        value: u32,
    },
    /// 32-bit pitch bend (0x8000_0000 is centre).
    PitchBend {
        /// 32-bit value.
        value: u32,
    },
    /// 32-bit channel pressure.
    ChannelPressure {
        /// 32-bit value.
        value: u32,
    },
    /// 32-bit polyphonic pressure.
    PolyPressure {
        /// Note number 0–127.
        pitch: u8,
        /// 32-bit value.
        value: u32,
    },
    /// Program change with optional bank select.
    Program {
        /// Program 0–127.
        program: u8,
        /// `(msb, lsb)`, each 0–127.
        bank: Option<(u8, u8)>,
    },
    /// System exclusive data (without F0/F7).
    SysEx {
        /// The payload bytes.
        data: Arc<[u8]>,
        /// 8-bit data allowed; otherwise every byte must be < 0x80.
        eight_bit: bool,
    },
    /// A text-type meta event.
    Text {
        /// Which kind of text.
        kind: TextKind,
        /// The text.
        text: String,
    },
    /// A live reference to another sequence.
    Reference {
        /// The referenced sequence.
        seq: SeqId,
        /// Per-use length (defaults to the sequence's length).
        length: Option<Tick>,
        /// How many times the use repeats.
        loop_count: LoopCount,
        /// Transpose in semitones, applied to notes and poly pressure.
        transpose: i8,
    },
    /// An audio region on the lane's audio output path.
    AudioRegion {
        /// The media item.
        media: MediaId,
        /// Start within the media, in sample frames.
        source_offset: u64,
        /// Length in sample frames (>= 1).
        length: u64,
        /// Gain in dB, finite, -144..=24.
        gain_db: f32,
        /// Fade in.
        fade_in: Fade,
        /// Fade out.
        fade_out: Fade,
    },
}

/// Event kinds, for rules and view filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// [`EventBody::Note`].
    Note,
    /// [`EventBody::Cc`].
    Cc,
    /// [`EventBody::PitchBend`].
    PitchBend,
    /// [`EventBody::ChannelPressure`].
    ChannelPressure,
    /// [`EventBody::PolyPressure`].
    PolyPressure,
    /// [`EventBody::Program`].
    Program,
    /// [`EventBody::SysEx`].
    SysEx,
    /// [`EventBody::Text`].
    Text,
    /// [`EventBody::Reference`].
    Reference,
    /// [`EventBody::AudioRegion`].
    AudioRegion,
}

impl EventBody {
    /// The kind of this body.
    pub fn kind(&self) -> EventKind {
        match self {
            EventBody::Note { .. } => EventKind::Note,
            EventBody::Cc { .. } => EventKind::Cc,
            EventBody::PitchBend { .. } => EventKind::PitchBend,
            EventBody::ChannelPressure { .. } => EventKind::ChannelPressure,
            EventBody::PolyPressure { .. } => EventKind::PolyPressure,
            EventBody::Program { .. } => EventKind::Program,
            EventBody::SysEx { .. } => EventKind::SysEx,
            EventBody::Text { .. } => EventKind::Text,
            EventBody::Reference { .. } => EventKind::Reference,
            EventBody::AudioRegion { .. } => EventKind::AudioRegion,
        }
    }

    /// A plain note with no attribute or per-note controllers.
    pub fn note(pitch: u8, velocity: u16, duration: Tick) -> EventBody {
        EventBody::Note {
            pitch,
            velocity,
            release_velocity: 0,
            duration,
            attribute: None,
            per_note: Vector::new(),
        }
    }

    /// A reference used once at the sequence's own length.
    pub fn reference(seq: SeqId) -> EventBody {
        EventBody::Reference {
            seq,
            length: None,
            loop_count: LoopCount::Count(1),
            transpose: 0,
        }
    }

    /// Whether this body is routed to a MIDI destination (everything except
    /// references, audio regions and text meta-events, which the engine
    /// ignores for routing).
    pub fn is_midi(&self) -> bool {
        !matches!(
            self,
            EventBody::Reference { .. } | EventBody::AudioRegion { .. } | EventBody::Text { .. }
        )
    }
}

/// A MIDI 2.0 note attribute (type and 16-bit data).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteAttribute {
    /// Attribute type (e.g. 3 = pitch 7.9).
    pub kind: u8,
    /// Attribute data.
    pub data: u16,
}

/// A per-note controller value at an offset within the note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerNoteCtl {
    /// Which controller.
    pub kind: PerNoteKind,
    /// Offset from the note start.
    pub offset: Tick,
    /// 32-bit value.
    pub value: u32,
}

/// Per-note controller kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerNoteKind {
    /// Registered per-note controller number.
    Registered(u8),
    /// Assignable per-note controller number.
    Assignable(u8),
    /// Per-note pitch bend.
    PitchBend,
}

/// Text meta-event kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
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

/// How a reference repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopCount {
    /// Play the per-use length this many times (>= 1).
    Count(u32),
    /// Repeat the sequence's own length until the per-use length is filled.
    Fill,
}

/// A re-editable fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fade {
    /// Fade length in ticks (>= 0).
    pub length: Tick,
    /// Curve shape.
    pub shape: FadeShape,
}

impl Default for Fade {
    fn default() -> Self {
        Fade {
            length: Tick::ZERO,
            shape: FadeShape::Linear,
        }
    }
}

/// Fade curve shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FadeShape {
    /// Linear gain.
    Linear,
    /// Equal-power (sine) curve.
    EqualPower,
    /// Exponential curve.
    Exponential,
    /// S-curve.
    SCurve,
}
