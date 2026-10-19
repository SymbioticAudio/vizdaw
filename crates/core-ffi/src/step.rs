//! Step entry over the FFI (plan 4f Rulings 11–15): `core_editor::step`'s
//! state and inputs as records. Swift keeps a copy of the state and hands
//! it to `Session.step` with each input; the session applies the effect as
//! one undo step and returns the next state.

use crate::error::VizError;
use crate::guard::ffi_guard;
use crate::snapshot::DocSnapshot;
use core_editor::step::{NoteValue, StepInput, StepKind, StepRecord, StepState};
use core_model::ids::{EventId, SeqId};
use core_model::time::Tick;

/// A note value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum NoteValueRow {
    /// 1/1
    Whole,
    /// 1/2
    Half,
    /// 1/4
    Quarter,
    /// 1/8
    Eighth,
    /// 1/16
    Sixteenth,
    /// 1/32
    ThirtySecond,
}

/// A note of a step: pitch and 16-bit velocity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct StepNoteRow {
    /// 0–127.
    pub pitch: u8,
    /// 16-bit velocity.
    pub velocity: u16,
}

/// What a past step did.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum StepKindRow {
    /// Entered these notes.
    Notes {
        /// Their IDs.
        events: Vec<u64>,
    },
    /// Moved on.
    Rest,
    /// Lengthened notes.
    Tie {
        /// Their IDs.
        events: Vec<u64>,
        /// By how much.
        by: i64,
    },
    /// Entered a reference.
    Reference {
        /// Its ID.
        event: u64,
    },
}

/// One past step.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct StepRecordRow {
    /// Where it began.
    pub tick: i64,
    /// How far it moved.
    pub length: i64,
    /// What it did.
    pub kind: StepKindRow,
}

/// Step entry's state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct StepStateRow {
    /// Where the next step goes.
    pub tick: i64,
    /// The chosen value.
    pub value: NoteValueRow,
    /// Dotted.
    pub dotted: bool,
    /// Triplet.
    pub triplet: bool,
    /// MIDI keys down.
    pub held: Vec<u8>,
    /// The chord being played.
    pub chord: Vec<StepNoteRow>,
    /// Past steps, oldest first.
    pub history: Vec<StepRecordRow>,
}

/// An input to step entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum StepInputRow {
    /// A note value key.
    Value {
        /// The value.
        value: NoteValueRow,
    },
    /// Toggles dotted.
    Dot,
    /// Toggles triplet.
    Triplet,
    /// Lengthens the last step's notes.
    Tie,
    /// Moves on.
    Rest,
    /// Takes the last step back.
    DeleteLast,
    /// A MIDI key went down.
    NoteOn {
        /// Its note.
        note: StepNoteRow,
    },
    /// A MIDI key went up.
    NoteOff {
        /// Its pitch.
        pitch: u8,
    },
    /// A whole chord at once.
    Notes {
        /// The notes.
        notes: Vec<StepNoteRow>,
    },
    /// A keyboard slot: a reference to `seq`.
    Reference {
        /// The sequence.
        seq: u64,
    },
}

/// What a step input came to.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct StepResultRow {
    /// The next state.
    pub state: StepStateRow,
    /// The document after the step, when it changed.
    pub snapshot: Option<DocSnapshot>,
    /// The position moved (a step was taken or taken back).
    pub moved: bool,
}

/// The core's value.
pub fn value(v: NoteValueRow) -> NoteValue {
    match v {
        NoteValueRow::Whole => NoteValue::Whole,
        NoteValueRow::Half => NoteValue::Half,
        NoteValueRow::Quarter => NoteValue::Quarter,
        NoteValueRow::Eighth => NoteValue::Eighth,
        NoteValueRow::Sixteenth => NoteValue::Sixteenth,
        NoteValueRow::ThirtySecond => NoteValue::ThirtySecond,
    }
}

fn value_row(v: NoteValue) -> NoteValueRow {
    match v {
        NoteValue::Whole => NoteValueRow::Whole,
        NoteValue::Half => NoteValueRow::Half,
        NoteValue::Quarter => NoteValueRow::Quarter,
        NoteValue::Eighth => NoteValueRow::Eighth,
        NoteValue::Sixteenth => NoteValueRow::Sixteenth,
        NoteValue::ThirtySecond => NoteValueRow::ThirtySecond,
    }
}

fn ids(v: &[u64]) -> Vec<EventId> {
    v.iter().copied().map(EventId).collect()
}

fn id_rows(v: &[EventId]) -> Vec<u64> {
    v.iter().map(|e| e.0).collect()
}

/// The core's state (bounded: a stale or hostile copy is cut to size).
pub fn state(r: &StepStateRow) -> StepState {
    StepState {
        tick: Tick(r.tick),
        value: value(r.value),
        dotted: r.dotted,
        triplet: r.triplet,
        held: r.held.iter().map(|p| p & 0x7F).collect(),
        chord: r
            .chord
            .iter()
            .map(|n| (n.pitch & 0x7F, n.velocity))
            .collect(),
        history: r
            .history
            .iter()
            .map(|h| StepRecord {
                tick: Tick(h.tick),
                length: Tick(h.length),
                kind: match &h.kind {
                    StepKindRow::Notes { events } => StepKind::Notes(ids(events)),
                    StepKindRow::Rest => StepKind::Rest,
                    StepKindRow::Tie { events, by } => StepKind::Tie {
                        notes: ids(events),
                        by: Tick(*by),
                    },
                    StepKindRow::Reference { event } => StepKind::Reference(EventId(*event)),
                },
            })
            .collect(),
    }
    .bounded()
}

/// The state as a row.
pub fn state_row(s: &StepState) -> StepStateRow {
    StepStateRow {
        tick: s.tick.0,
        value: value_row(s.value),
        dotted: s.dotted,
        triplet: s.triplet,
        held: s.held.clone(),
        chord: s
            .chord
            .iter()
            .map(|(pitch, velocity)| StepNoteRow {
                pitch: *pitch,
                velocity: *velocity,
            })
            .collect(),
        history: s
            .history
            .iter()
            .map(|h| StepRecordRow {
                tick: h.tick.0,
                length: h.length.0,
                kind: match &h.kind {
                    StepKind::Notes(e) => StepKindRow::Notes { events: id_rows(e) },
                    StepKind::Rest => StepKindRow::Rest,
                    StepKind::Tie { notes, by } => StepKindRow::Tie {
                        events: id_rows(notes),
                        by: by.0,
                    },
                    StepKind::Reference(e) => StepKindRow::Reference { event: e.0 },
                },
            })
            .collect(),
    }
}

/// The core's input; a reference's length is the session's to fill in.
pub fn input(r: &StepInputRow, reference_length: impl FnOnce(SeqId) -> Tick) -> StepInput {
    let notes = |v: &[StepNoteRow]| v.iter().map(|n| (n.pitch, n.velocity)).collect();
    match r {
        StepInputRow::Value { value: v } => StepInput::Value(value(*v)),
        StepInputRow::Dot => StepInput::Dot,
        StepInputRow::Triplet => StepInput::Triplet,
        StepInputRow::Tie => StepInput::Tie,
        StepInputRow::Rest => StepInput::Rest,
        StepInputRow::DeleteLast => StepInput::DeleteLast,
        StepInputRow::NoteOn { note } => StepInput::NoteOn {
            pitch: note.pitch,
            velocity: note.velocity,
        },
        StepInputRow::NoteOff { pitch } => StepInput::NoteOff { pitch: *pitch },
        StepInputRow::Notes { notes: n } => StepInput::Notes(notes(n)),
        StepInputRow::Reference { seq } => StepInput::Reference {
            seq: SeqId(*seq),
            length: reference_length(SeqId(*seq)),
        },
    }
}

/// Step entry starting at `tick` with quarter notes.
#[uniffi::export]
pub fn step_state_new(tick: i64) -> StepStateRow {
    state_row(&StepState::new(Tick(tick)))
}

/// A step's length in ticks.
#[uniffi::export]
pub fn step_length(value: NoteValueRow, dotted: bool, triplet: bool) -> i64 {
    core_editor::step::step_length(self::value(value), dotted, triplet).0
}

/// Typed pitches (`60 64 67`, `C4 E4 G4`; plan 4f Ruling 14).
#[uniffi::export]
pub fn step_parse_pitches(text: String) -> Result<Vec<u8>, VizError> {
    ffi_guard(|| {
        core_editor::step::parse_pitches(&text).map_err(|message| VizError::Model { message })
    })
}
