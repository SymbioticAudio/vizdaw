//! Step entry (editors spec §4f, plan 4f Rulings 11–15): note values
//! from keys, dot and triplet, tie, rest, delete-last-step, pitches from
//! MIDI input (a chord is every key pressed before the last one is
//! released) and keyboard slots. A pure state machine: [`press`] turns an
//! input into an [`Effect`] on the document; once the document applied
//! it, [`commit`] records the step and moves on. A refused effect is
//! never committed, so the state stays where it was.

use core_model::ids::{EventId, SeqId};
use core_model::time::{PPQ, Tick};

/// The most notes one step enters.
pub const MAX_CHORD: usize = 16;
/// The most past steps Delete Last Step can take back.
pub const MAX_HISTORY: usize = 256;
/// The most characters of a typed pitch list.
pub const MAX_PITCH_TEXT: usize = 256;

/// A note value chosen by a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteValue {
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

impl NoteValue {
    /// Its plain length.
    pub fn ticks(self) -> Tick {
        let whole = 4 * PPQ;
        Tick(match self {
            NoteValue::Whole => whole,
            NoteValue::Half => whole / 2,
            NoteValue::Quarter => whole / 4,
            NoteValue::Eighth => whole / 8,
            NoteValue::Sixteenth => whole / 16,
            NoteValue::ThirtySecond => whole / 32,
        })
    }
}

/// A step's length: the value, times 3/2 when dotted, times 2/3 as a
/// triplet (both: the value).
pub fn step_length(value: NoteValue, dotted: bool, triplet: bool) -> Tick {
    let dot = if dotted { 3 } else { 2 };
    let three = if triplet { 2 } else { 3 };
    Tick(value.ticks().0 * dot * three / 6)
}

/// One past step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    /// Where it began.
    pub tick: Tick,
    /// How far it moved the position.
    pub length: Tick,
    /// What it did.
    pub kind: StepKind,
}

/// What a past step did to the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    /// Entered these notes.
    Notes(Vec<EventId>),
    /// Entered nothing.
    Rest,
    /// Lengthened these notes by `by`.
    Tie {
        /// The notes.
        notes: Vec<EventId>,
        /// How much.
        by: Tick,
    },
    /// Entered this reference (a keyboard slot).
    Reference(EventId),
}

/// Step entry's state; the app keeps a copy and hands it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepState {
    /// Where the next step goes.
    pub tick: Tick,
    /// The chosen value.
    pub value: NoteValue,
    /// Dotted (sticky until toggled).
    pub dotted: bool,
    /// Triplet (sticky until toggled).
    pub triplet: bool,
    /// MIDI keys down now.
    pub held: Vec<u8>,
    /// The chord being played: every key pressed since the first went
    /// down, with its velocity.
    pub chord: Vec<(u8, u16)>,
    /// Past steps, oldest first (at most [`MAX_HISTORY`]).
    pub history: Vec<StepRecord>,
}

impl StepState {
    /// Step entry starting at `tick` with quarter notes.
    pub fn new(tick: Tick) -> StepState {
        StepState {
            tick,
            value: NoteValue::Quarter,
            dotted: false,
            triplet: false,
            held: Vec::new(),
            chord: Vec::new(),
            history: Vec::new(),
        }
    }

    /// The length a step has now.
    pub fn length(&self) -> Tick {
        step_length(self.value, self.dotted, self.triplet)
    }

    /// The state with every list within its bound (a copy the app built
    /// from stale or hostile data).
    pub fn bounded(mut self) -> StepState {
        self.held.truncate(MAX_CHORD);
        self.chord.truncate(MAX_CHORD);
        let n = self.history.len();
        if n > MAX_HISTORY {
            self.history.drain(..n - MAX_HISTORY);
        }
        self
    }
}

/// An input to step entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepInput {
    /// A note value key.
    Value(NoteValue),
    /// The dot key: toggles dotted.
    Dot,
    /// The triplet key: toggles triplet.
    Triplet,
    /// Lengthens the last step's notes by a step.
    Tie,
    /// Moves on by a step.
    Rest,
    /// Takes the last step back.
    DeleteLast,
    /// A MIDI key went down.
    NoteOn {
        /// Note number (masked to 0–127).
        pitch: u8,
        /// 16-bit velocity.
        velocity: u16,
    },
    /// A MIDI key went up.
    NoteOff {
        /// Note number.
        pitch: u8,
    },
    /// A whole chord at once (typed pitches, the control channel).
    Notes(Vec<(u8, u16)>),
    /// A keyboard slot: a reference to `seq`, `length` long.
    Reference {
        /// The referenced sequence.
        seq: SeqId,
        /// Its length.
        length: Tick,
    },
}

/// What an input asks of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Nothing (a setting changed, or a chord is still being played).
    Nothing,
    /// Move on without touching the document.
    Rest {
        /// How far.
        length: Tick,
    },
    /// Enter notes at `tick`, each `length` long.
    Insert {
        /// Where.
        tick: Tick,
        /// How long (also how far the position moves).
        length: Tick,
        /// Pitch and velocity of each, by pitch.
        notes: Vec<(u8, u16)>,
    },
    /// Enter a reference to `seq` at `tick`.
    InsertReference {
        /// Where.
        tick: Tick,
        /// The sequence.
        seq: SeqId,
        /// How far the position moves.
        length: Tick,
    },
    /// Lengthen `notes` by `by`.
    Extend {
        /// The notes.
        notes: Vec<EventId>,
        /// How much (also how far the position moves).
        by: Tick,
    },
    /// Take the last step back: delete `delete`, shorten `shorten` by `by`,
    /// and go back to `to`.
    TakeBack {
        /// Events to delete (a step's notes or reference).
        delete: Vec<EventId>,
        /// Notes a tie lengthened.
        shorten: Vec<EventId>,
        /// How much they were lengthened.
        by: Tick,
        /// Where the step began.
        to: Tick,
    },
}

fn sorted_chord(mut notes: Vec<(u8, u16)>) -> Vec<(u8, u16)> {
    let mut out: Vec<(u8, u16)> = Vec::new();
    for (p, v) in notes.drain(..) {
        let p = p & 0x7F;
        if !out.iter().any(|(q, _)| *q == p) && out.len() < MAX_CHORD {
            out.push((p, v));
        }
    }
    out.sort_by_key(|(p, _)| *p);
    out
}

/// The notes the last step left sounding (its own or the ones it tied).
fn last_notes(state: &StepState) -> Option<Vec<EventId>> {
    match &state.history.last()?.kind {
        StepKind::Notes(ids) if !ids.is_empty() => Some(ids.clone()),
        StepKind::Tie { notes, .. } if !notes.is_empty() => Some(notes.clone()),
        _ => None,
    }
}

/// What `input` does in `state`: the state with any setting changed (not
/// yet moved on) and the effect to apply before [`commit`].
pub fn press(state: &StepState, input: StepInput) -> (StepState, Effect) {
    let mut s = state.clone().bounded();
    let length = s.length();
    let effect = match input {
        StepInput::Value(v) => {
            s.value = v;
            Effect::Nothing
        }
        StepInput::Dot => {
            s.dotted = !s.dotted;
            Effect::Nothing
        }
        StepInput::Triplet => {
            s.triplet = !s.triplet;
            Effect::Nothing
        }
        StepInput::Rest => Effect::Rest { length },
        StepInput::Tie => match last_notes(&s) {
            Some(notes) => Effect::Extend { notes, by: length },
            None => Effect::Rest { length },
        },
        StepInput::DeleteLast => match s.history.last() {
            None => Effect::Nothing,
            Some(r) => {
                let (delete, shorten, by) = match &r.kind {
                    StepKind::Notes(ids) => (ids.clone(), Vec::new(), Tick::ZERO),
                    StepKind::Rest => (Vec::new(), Vec::new(), Tick::ZERO),
                    StepKind::Tie { notes, by } => (Vec::new(), notes.clone(), *by),
                    StepKind::Reference(id) => (vec![*id], Vec::new(), Tick::ZERO),
                };
                Effect::TakeBack {
                    delete,
                    shorten,
                    by,
                    to: r.tick,
                }
            }
        },
        StepInput::NoteOn { pitch, velocity } => {
            let p = pitch & 0x7F;
            if !s.held.contains(&p) && s.held.len() < MAX_CHORD {
                s.held.push(p);
            }
            if !s.chord.iter().any(|(q, _)| *q == p) && s.chord.len() < MAX_CHORD {
                s.chord.push((p, velocity));
            }
            Effect::Nothing
        }
        StepInput::NoteOff { pitch } => {
            let p = pitch & 0x7F;
            let was_held = s.held.contains(&p);
            s.held.retain(|q| *q != p);
            if was_held && s.held.is_empty() && !s.chord.is_empty() {
                let notes = sorted_chord(std::mem::take(&mut s.chord));
                Effect::Insert {
                    tick: s.tick,
                    length,
                    notes,
                }
            } else {
                Effect::Nothing
            }
        }
        StepInput::Notes(notes) => {
            let notes = sorted_chord(notes);
            if notes.is_empty() {
                Effect::Nothing
            } else {
                Effect::Insert {
                    tick: s.tick,
                    length,
                    notes,
                }
            }
        }
        StepInput::Reference { seq, length } => Effect::InsertReference {
            tick: s.tick,
            seq,
            length: length.max(Tick::ZERO),
        },
    };
    (s, effect)
}

/// After the document applied `effect` (`created`: the events it made, in
/// order): records the step and moves the position on (or back).
pub fn commit(state: &StepState, effect: &Effect, created: &[EventId]) -> StepState {
    let mut s = state.clone().bounded();
    let record = |tick: Tick, length: Tick, kind: StepKind| StepRecord { tick, length, kind };
    match effect {
        Effect::Nothing => return s,
        Effect::Rest { length } => {
            let r = record(s.tick, *length, StepKind::Rest);
            s.history.push(r);
            s.tick = s.tick + *length;
        }
        Effect::Insert { tick, length, .. } => {
            s.history
                .push(record(*tick, *length, StepKind::Notes(created.to_vec())));
            s.tick = *tick + *length;
        }
        Effect::InsertReference { tick, length, .. } => {
            let kind = match created.first() {
                Some(id) => StepKind::Reference(*id),
                None => StepKind::Rest,
            };
            s.history.push(record(*tick, *length, kind));
            s.tick = *tick + *length;
        }
        Effect::Extend { notes, by } => {
            let r = record(
                s.tick,
                *by,
                StepKind::Tie {
                    notes: notes.clone(),
                    by: *by,
                },
            );
            s.history.push(r);
            s.tick = s.tick + *by;
        }
        Effect::TakeBack { to, .. } => {
            s.history.pop();
            s.tick = *to;
        }
    }
    s.bounded()
}

/// Reads typed pitches (plan 4f Ruling 14): note numbers (`60`) or names
/// (`C4`, `c#4`, `Eb3`, `B-1`; middle C is C4), separated by spaces or
/// commas; at most [`MAX_CHORD`] of them.
pub fn parse_pitches(text: &str) -> Result<Vec<u8>, String> {
    if text.chars().count() > MAX_PITCH_TEXT {
        return Err(format!("at most {MAX_PITCH_TEXT} characters of pitches"));
    }
    let mut out = Vec::new();
    for token in text.split([' ', ',', '\t', '\n']).filter(|t| !t.is_empty()) {
        if out.len() == MAX_CHORD {
            return Err(format!("at most {MAX_CHORD} pitches"));
        }
        out.push(parse_pitch(token)?);
    }
    if out.is_empty() {
        return Err("no pitches".into());
    }
    Ok(out)
}

fn parse_pitch(token: &str) -> Result<u8, String> {
    let shown = || token.chars().take(16).collect::<String>();
    if let Ok(n) = token.parse::<u8>() {
        return if n <= 127 {
            Ok(n)
        } else {
            Err(format!("{:?} is above 127", shown()))
        };
    }
    let mut chars = token.chars();
    let class: i32 = match chars.next().map(|c| c.to_ascii_uppercase()) {
        Some('C') => 0,
        Some('D') => 2,
        Some('E') => 4,
        Some('F') => 5,
        Some('G') => 7,
        Some('A') => 9,
        Some('B') => 11,
        _ => return Err(format!("{:?} is not a pitch", shown())),
    };
    let rest = chars.as_str();
    let (shift, octave) = match rest.chars().next() {
        Some('#') => (1, rest.get(1..).unwrap_or("")),
        Some('b') => (-1, rest.get(1..).unwrap_or("")),
        _ => (0, rest),
    };
    let octave: i32 = octave
        .parse()
        .map_err(|_| format!("{:?} is not a pitch", shown()))?;
    let n = octave
        .checked_add(1)
        .and_then(|o| o.checked_mul(12))
        .and_then(|o| o.checked_add(class + shift))
        .filter(|n| (0..=127).contains(n))
        .ok_or_else(|| format!("{:?} is out of range", shown()))?;
    u8::try_from(n).map_err(|_| format!("{:?} is out of range", shown()))
}
