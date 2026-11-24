//! Step entry for one document (Swift `StepEntryModel` and
//! `StepCommands`): on or off, the track steps go into and the state the
//! core's state machine hands back. The step position is the
//! transport's: steps locate it as they go, and moving the transport
//! moves where the next step goes. One implementation for the Step menu,
//! the keys and the control channel.

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};
use vizdaw_ffi::NoteInRow;
use vizdaw_ffi::VizError;
use vizdaw_ffi::control::StepEntryStateRow;
use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::step::{NoteValueRow, StepInputRow, StepNoteRow, StepStateRow};

use crate::alerts::error_message;
use crate::commands::{Ran, TEXT_FIELD_HAS_KEYBOARD, check_names, on_flag, ran};
use crate::document::DocumentModel;

/// The velocity of typed notes (a played note keeps its own): 100 as 16
/// bits.
pub const TYPED_VELOCITY: u16 = 100 << 9;
/// Step entry cannot start, or take a step, while playing.
pub const PLAYING: &str = "Stop playing to enter steps.";
/// A step key while step entry is off.
pub const OFF: &str = "Step entry is off (Step > Step Entry).";
/// Step entry ended because another sequence is shown.
pub const SEQUENCE_CHANGED: &str = "Step entry ended: the sequence changed.";
/// Step entry ended because its track was deleted.
pub const TRACK_GONE: &str = "Step entry ended: its track is gone.";
/// Step entry has no track to start on.
pub const NO_TRACK: &str = "Select a sequence with a track first.";

/// `step.entry`: on (`on` sets it, absent flips it).
pub const ENTRY: &str = "step.entry";
/// `step.notes`: a typed chord (the channel's alone).
pub const NOTES: &str = "step.notes";

/// Every step command, in the Step menu's order.
pub const STEP_COMMANDS: &[&str] = &[
    ENTRY,
    "step.whole",
    "step.half",
    "step.quarter",
    "step.eighth",
    "step.sixteenth",
    "step.thirty_second",
    "step.dot",
    "step.triplet",
    "step.tie",
    "step.rest",
    "step.delete_last",
    NOTES,
];

/// Whether a held key runs `id` again at the key-repeat rate: rest, tie
/// and Delete Last Step; every other step key runs once per press.
pub fn repeats(id: &str) -> bool {
    matches!(id, "step.rest" | "step.tie" | "step.delete_last")
}

/// The note value command `id` chooses.
fn value_of(id: &str) -> Option<NoteValueRow> {
    Some(match id {
        "step.whole" => NoteValueRow::Whole,
        "step.half" => NoteValueRow::Half,
        "step.quarter" => NoteValueRow::Quarter,
        "step.eighth" => NoteValueRow::Eighth,
        "step.sixteenth" => NoteValueRow::Sixteenth,
        "step.thirty_second" => NoteValueRow::ThirtySecond,
        _ => return None,
    })
}

/// The input command `id` sends (a value, a toggle, a step).
fn input_of(id: &str) -> Option<StepInputRow> {
    if let Some(value) = value_of(id) {
        return Some(StepInputRow::Value { value });
    }
    Some(match id {
        "step.dot" => StepInputRow::Dot,
        "step.triplet" => StepInputRow::Triplet,
        "step.tie" => StepInputRow::Tie,
        "step.rest" => StepInputRow::Rest,
        "step.delete_last" => StepInputRow::DeleteLast,
        _ => return None,
    })
}

/// The value's snake_case name on the control channel.
pub fn wire_name(v: NoteValueRow) -> &'static str {
    match v {
        NoteValueRow::Whole => "whole",
        NoteValueRow::Half => "half",
        NoteValueRow::Quarter => "quarter",
        NoteValueRow::Eighth => "eighth",
        NoteValueRow::Sixteenth => "sixteenth",
        NoteValueRow::ThirtySecond => "thirty_second",
    }
}

/// The value as the summary shows it (`1/8`).
pub fn title(v: NoteValueRow) -> &'static str {
    match v {
        NoteValueRow::Whole => "1/1",
        NoteValueRow::Half => "1/2",
        NoteValueRow::Quarter => "1/4",
        NoteValueRow::Eighth => "1/8",
        NoteValueRow::Sixteenth => "1/16",
        NoteValueRow::ThirtySecond => "1/32",
    }
}

/// Step entry's state and target; both `None` while it is off.
#[derive(Debug, Clone, Default)]
pub struct StepEntryModel {
    state: Option<StepStateRow>,
    target: Option<LaneRef>,
}

impl StepEntryModel {
    /// On. The document ends it the moment its target is gone, so the
    /// summary, the editor state, the menu and the keys agree.
    pub fn is_on(&self) -> bool {
        self.state.is_some()
    }

    /// The state the core last handed back.
    pub fn state(&self) -> Option<&StepStateRow> {
        self.state.as_ref()
    }

    /// The sequence and track steps go into.
    pub fn target(&self) -> Option<LaneRef> {
        self.target
    }

    fn stop(&mut self) {
        self.state = None;
        self.target = None;
    }
}

/// A step refusal that is not the core's: unavailable passes a key on,
/// failed uses it up.
fn unavailable(message: &str) -> ControlError {
    ControlError::new(ErrorCode::Unavailable, message)
}

fn refused(message: &str) -> ControlError {
    ControlError::new(ErrorCode::Failed, message)
}

/// Forgets the MIDI keys held and the chord being played.
fn let_go(s: &mut StepStateRow) {
    s.held.clear();
    s.chord.clear();
}

impl DocumentModel {
    /// Step entry's state and target.
    pub fn step_entry(&self) -> &StepEntryModel {
        &self.step
    }

    /// Step entry is on (Record is refused, the step keys win).
    pub fn step_entry_on(&self) -> bool {
        self.step.is_on()
    }

    /// The track step entry starts on: the selection's first track of the
    /// shown sequence, else its first track.
    pub fn step_lane(&self) -> Option<LaneRef> {
        let seq = self.selected_seq()?;
        let lane = self
            .selected_lane()
            .or_else(|| self.lanes().first().map(|l| l.id))?;
        Some(LaneRef { seq, lane })
    }

    /// Turns step entry on at the transport's position (read first).
    pub fn start_step_entry(&mut self) -> Result<(), ControlError> {
        let _ = self.poll_transport();
        if self.is_playing() {
            return Err(unavailable(PLAYING));
        }
        let Some(target) = self.step_lane() else {
            return Err(unavailable(NO_TRACK));
        };
        let tick = self.last_transport().tick;
        self.step = StepEntryModel {
            state: Some(vizdaw_ffi::step_state_new(tick)),
            target: Some(target),
        };
        Ok(())
    }

    /// Turns step entry off.
    pub fn stop_step_entry(&mut self) {
        self.step.stop();
    }

    /// Why the target is no longer where steps can go (`None` while it is).
    fn step_end_reason(&self) -> Option<&'static str> {
        let t = self.step.target?;
        if self.selected_seq() != Some(t.seq) {
            return Some(SEQUENCE_CHANGED);
        }
        if !self.lanes().iter().any(|l| l.id == t.lane) {
            return Some(TRACK_GONE);
        }
        None
    }

    /// Ends step entry, saying why in the status line, once its target is
    /// gone (the shown sequence changed, its track was deleted). The
    /// document calls it whenever either changes.
    pub(crate) fn check_step_target(&mut self) {
        if let Some(why) = self.step_end_reason() {
            self.step.stop();
            self.set_status(Some(why.to_owned()));
        }
    }

    /// One input; a step moves the transport to the next position. While
    /// playing it is refused (a key is used up, so Delete never reaches
    /// the editor) and the held keys are forgotten: a key let go while
    /// playing never arrives.
    pub fn step_input(&mut self, input: StepInputRow) -> Result<(), ControlError> {
        let (Some(mut s), Some(t)) = (self.step.state.clone(), self.step.target) else {
            return Err(unavailable(OFF));
        };
        if let Some(why) = self.step_end_reason() {
            self.step.stop();
            self.set_status(Some(why.to_owned()));
            return Err(unavailable(why));
        }
        let _ = self.poll_transport();
        if self.is_playing() {
            let_go(&mut s);
            self.step.state = Some(s);
            return Err(refused(PLAYING));
        }
        s.tick = self.last_transport().tick;
        let note = matches!(
            input,
            StepInputRow::NoteOn { .. } | StepInputRow::NoteOff { .. }
        );
        let result = self
            .session()
            .step_entry(t.seq, t.lane, s.clone(), input)
            .and_then(|r| {
                self.step.state = Some(r.state.clone());
                if let Some(snap) = r.snapshot {
                    self.apply(snap);
                    self.refresh_selection();
                }
                if r.moved {
                    self.move_to(r.state.tick)?;
                }
                Ok(())
            });
        result.map_err(|e: VizError| {
            // A refused key event may never be matched: forget the held
            // keys.
            if note && self.step.is_on() {
                let mut cleared = s;
                let_go(&mut cleared);
                self.step.state = Some(cleared);
            }
            let message = error_message(&e);
            self.set_status(Some(message.clone()));
            let code = if matches!(e, VizError::Busy { .. }) {
                ErrorCode::Busy
            } else {
                ErrorCode::Failed
            };
            ControlError::new(code, message)
        })
    }

    /// Notes played on a MIDI source: each key down and up in order.
    pub fn receive_step_notes(&mut self, notes: &[NoteInRow]) {
        for n in notes {
            let input = if n.on {
                StepInputRow::NoteOn {
                    note: StepNoteRow {
                        pitch: n.pitch,
                        velocity: n.velocity,
                    },
                }
            } else {
                StepInputRow::NoteOff { pitch: n.pitch }
            };
            let _ = self.step_input(input);
        }
    }

    /// Typed pitches as one chord at the typed velocity.
    pub fn enter_step_pitches(&mut self, pitches: &[u8]) -> Result<(), ControlError> {
        let notes = pitches
            .iter()
            .map(|&pitch| StepNoteRow {
                pitch,
                velocity: TYPED_VELOCITY,
            })
            .collect();
        self.step_input(StepInputRow::Notes { notes })
    }

    /// The step bar's line: `Step Entry · Chords · 1/8 dotted`; `None`
    /// while off or ended.
    pub fn step_summary(&self) -> Option<String> {
        let (s, t) = (self.step.state.as_ref()?, self.step.target?);
        if self.step_end_reason().is_some() {
            return None;
        }
        let track = self
            .lanes()
            .iter()
            .find(|l| l.id == t.lane)
            .map_or_else(|| format!("Track {}", t.lane), |l| l.name.clone());
        let mut v = title(s.value).to_owned();
        if s.dotted {
            v.push_str(" dotted");
        }
        if s.triplet {
            v.push_str(" triplet");
        }
        Some(format!("Step Entry · {track} · {v}"))
    }

    /// Step entry for `state.get editor`: `None` while off or ended; the
    /// position is the transport's (read now).
    pub fn step_state_row(&self) -> Option<StepEntryStateRow> {
        let (s, t) = (self.step.state.as_ref()?, self.step.target?);
        if self.step_end_reason().is_some() {
            return None;
        }
        let _ = self.poll_transport();
        Some(StepEntryStateRow {
            seq: t.seq,
            lane: t.lane,
            tick: self.last_transport().tick,
            value: wire_name(s.value).to_owned(),
            dotted: s.dotted,
            triplet: s.triplet,
            steps: u32::try_from(s.history.len()).unwrap_or(u32::MAX),
        })
    }
}

/// As `run_step` decides: step entry can start with a track to enter
/// into while stopped, its inputs while it is on and stopped; nothing
/// while a text field has the keyboard.
pub fn step_enabled(id: &str, doc: &DocumentModel) -> bool {
    if !doc.transport_keys_enabled() {
        return false;
    }
    if id == ENTRY {
        return doc.step_entry_on() || (doc.step_lane().is_some() && !doc.is_playing());
    }
    doc.step_entry_on() && !doc.is_playing()
}

/// A Step menu item's check mark: step entry, the chosen value, dot,
/// triplet.
pub fn step_is_on(id: &str, doc: &DocumentModel) -> bool {
    let Some(s) = doc.step_entry().state() else {
        return false;
    };
    match id {
        ENTRY => true,
        "step.dot" => s.dotted,
        "step.triplet" => s.triplet,
        _ => value_of(id) == Some(s.value),
    }
}

/// Runs step command `id` (a menu item, a key, the channel). Every one
/// but `step.notes` (the channel's alone) waits while a text field has
/// the keyboard.
pub fn run_step(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    check_names(id, args)?;
    if id != NOTES && !doc.transport_keys_enabled() {
        return Err(unavailable(TEXT_FIELD_HAS_KEYBOARD));
    }
    if id == ENTRY {
        let want = on_flag(id, args)?.unwrap_or(!doc.step_entry_on());
        if want != doc.step_entry_on() {
            if want {
                doc.start_step_entry()?;
            } else {
                doc.stop_step_entry();
            }
        }
        return Ok(ran(doc));
    }
    if !doc.step_entry_on() {
        return Err(unavailable(OFF));
    }
    if id == NOTES {
        let text = match args.iter().find(|(n, _)| n == "pitches") {
            Some((_, ArgValue::Text(t))) => t.clone(),
            Some(_) => return Err(ControlError::params(id, "`pitches` is text")),
            None => String::new(),
        };
        // The core's pure parser (`CoreCalls::step_parse_pitches` is the
        // same function; the document holds no `CoreCalls`).
        let pitches =
            vizdaw_ffi::step_parse_pitches(text).map_err(|e| refused(&error_message(&e)))?;
        doc.enter_step_pitches(&pitches)?;
        return Ok(ran(doc));
    }
    let Some(input) = input_of(id) else {
        return Err(unavailable(&format!("VizDAW has no handler for {id}.")));
    };
    doc.step_input(input)?;
    Ok(ran(doc))
}
