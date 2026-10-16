//! Event commands, plus the shared edit helper and drum-row auto-creation.

use super::Report;
use crate::document::Document;
use crate::error::Result;
use crate::event::{Event, EventBody, EventSpec};
use crate::ids::{AnyId, DestId, EventId, LaneId, SeqId};
use crate::lane::DrumRow;
use crate::rules::{check_event, check_event_fields};
use crate::time::Tick;
use std::collections::BTreeSet;

/// Event-level edits. Commands that take several events apply to each;
/// fields that do not apply to an event's kind leave it unchanged.
#[derive(Debug, Clone, PartialEq)]
pub enum EventCmd {
    /// Insert events into a lane's active take.
    Insert {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// The events.
        events: Vec<EventSpec>,
    },
    /// Delete events.
    Delete {
        /// The events.
        events: Vec<EventId>,
    },
    /// Move in time and optionally to another lane's active take.
    Move {
        /// The events.
        events: Vec<EventId>,
        /// Time offset.
        delta: Tick,
        /// Destination lane.
        to_lane: Option<(SeqId, LaneId)>,
    },
    /// Set note / poly-pressure pitch.
    SetPitch {
        /// The events.
        events: Vec<EventId>,
        /// Pitch 0–127.
        pitch: u8,
    },
    /// Set note velocity.
    SetVelocity {
        /// The events.
        events: Vec<EventId>,
        /// 16-bit velocity.
        velocity: u16,
    },
    /// Set note duration.
    SetDuration {
        /// The events.
        events: Vec<EventId>,
        /// Duration (>= 0).
        duration: Tick,
    },
    /// Set the 32-bit value of CC, pitch bend and pressure events.
    SetValue {
        /// The events.
        events: Vec<EventId>,
        /// Value.
        value: u32,
    },
    /// Set or clear the destination override.
    SetDest {
        /// The events.
        events: Vec<EventId>,
        /// Destination (must exist).
        dest: Option<DestId>,
    },
    /// Set or clear the group/channel override.
    SetGroupChannel {
        /// The events.
        events: Vec<EventId>,
        /// `(group, channel)`, each 0–15.
        group_channel: Option<(u8, u8)>,
    },
    /// Mute or unmute.
    SetMute {
        /// The events.
        events: Vec<EventId>,
        /// Mute flag.
        mute: bool,
    },
    /// Nudge in time and pitch (pitch clamps at 0 and 127).
    Nudge {
        /// The events.
        events: Vec<EventId>,
        /// Time offset.
        ticks: Tick,
        /// Pitch offset.
        semitones: i8,
    },
}

impl EventCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            EventCmd::Insert { .. } => "Insert Events",
            EventCmd::Delete { .. } => "Delete Events",
            EventCmd::Move { .. } => "Move Events",
            EventCmd::SetPitch { .. } => "Set Pitch",
            EventCmd::SetVelocity { .. } => "Set Velocity",
            EventCmd::SetDuration { .. } => "Set Duration",
            EventCmd::SetValue { .. } => "Set Value",
            EventCmd::SetDest { .. } => "Set Event Destination",
            EventCmd::SetGroupChannel { .. } => "Set Group/Channel",
            EventCmd::SetMute { .. } => "Mute Events",
            EventCmd::Nudge { .. } => "Nudge Events",
        }
    }
}

/// Note name such as `C2` for 36 (C-1 is note 0, middle C 60 is `C4`).
pub(crate) fn note_name(n: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let name = NAMES.get(usize::from(n % 12)).copied().unwrap_or("?");
    format!("{name}{}", i32::from(n / 12) - 1)
}

/// Adds a pitch-shifted copy of `p`, clamped to 0..=127.
pub(crate) fn shift_pitch(p: u8, by: i32) -> u8 {
    u8::try_from((i32::from(p) + by).clamp(0, 127)).unwrap_or(127)
}

/// If `id` is a note on a drum lane with no matching row, adds a row.
pub(crate) fn ensure_drum_row(d: &mut Document, id: EventId, r: &mut Report) -> Result<()> {
    let (ev, loc) = d.event(id)?;
    let EventBody::Note { pitch, .. } = ev.body else {
        return Ok(());
    };
    let dest = ev.dest;
    let lane = d.lane(loc.seq, loc.lane)?;
    let Some(drum) = &lane.drum else {
        return Ok(());
    };
    if drum.rows.iter().any(|row| row.matches(pitch, dest)) {
        return Ok(());
    }
    let row_id = d.counters_mut().alloc_drum_row();
    if let Some(drum) = d.lane_mut(loc.seq, loc.lane)?.drum.as_mut() {
        drum.rows.push_back(DrumRow {
            id: row_id,
            name: note_name(pitch),
            note: pitch,
            dest,
            show_velocity: false,
        });
    }
    r.created.push(AnyId::DrumRow(row_id));
    Ok(())
}

/// Applies `f` to each event (deduplicated), re-validates it, stores it and
/// auto-creates drum rows.
pub(crate) fn edit_events(
    d: &mut Document,
    ids: &[EventId],
    r: &mut Report,
    mut f: impl FnMut(&mut Event) -> Result<()>,
) -> Result<()> {
    let unique: BTreeSet<EventId> = ids.iter().copied().collect();
    for id in unique {
        let (ev, _) = d.event(id)?;
        let mut ev = ev.clone();
        f(&mut ev)?;
        check_event_fields(d, &ev.spec())?;
        d.replace_event(ev)?;
        ensure_drum_row(d, id, r)?;
    }
    Ok(())
}

/// Inserts specs into the active take of a lane; returns the new IDs.
pub(crate) fn insert_specs(
    d: &mut Document,
    seq: SeqId,
    lane: LaneId,
    specs: &[EventSpec],
    r: &mut Report,
) -> Result<Vec<EventId>> {
    d.lane(seq, lane)?;
    let mut ids = Vec::with_capacity(specs.len());
    for spec in specs {
        check_event(d, seq, spec)?;
        let id = d.counters_mut().alloc_event();
        d.insert_event(seq, lane, None, spec.clone().with_id(id))?;
        r.created.push(AnyId::Event(id));
        ensure_drum_row(d, id, r)?;
        ids.push(id);
    }
    Ok(ids)
}

pub(crate) fn apply(d: &mut Document, cmd: &EventCmd, r: &mut Report) -> Result<()> {
    match cmd {
        EventCmd::Insert { seq, lane, events } => {
            insert_specs(d, *seq, *lane, events, r)?;
        }
        EventCmd::Delete { events } => {
            let unique: BTreeSet<EventId> = events.iter().copied().collect();
            for id in unique {
                d.remove_event(id)?;
                r.removed.push(AnyId::Event(id));
            }
        }
        EventCmd::Move {
            events,
            delta,
            to_lane,
        } => match to_lane {
            None => edit_events(d, events, r, |e| {
                e.tick = e.tick + *delta;
                Ok(())
            })?,
            Some((seq, lane)) => {
                d.lane(*seq, *lane)?;
                let unique: BTreeSet<EventId> = events.iter().copied().collect();
                for id in unique {
                    let (mut ev, _) = d.remove_event(id)?;
                    ev.tick = ev.tick + *delta;
                    check_event(d, *seq, &ev.spec())?;
                    d.insert_event(*seq, *lane, None, ev)?;
                    ensure_drum_row(d, id, r)?;
                }
            }
        },
        EventCmd::SetPitch { events, pitch } => edit_events(d, events, r, |e| {
            match &mut e.body {
                EventBody::Note { pitch: p, .. } | EventBody::PolyPressure { pitch: p, .. } => {
                    *p = *pitch
                }
                _ => {}
            }
            Ok(())
        })?,
        EventCmd::SetVelocity { events, velocity } => edit_events(d, events, r, |e| {
            if let EventBody::Note { velocity: v, .. } = &mut e.body {
                *v = *velocity;
            }
            Ok(())
        })?,
        EventCmd::SetDuration { events, duration } => edit_events(d, events, r, |e| {
            if let EventBody::Note { duration: v, .. } = &mut e.body {
                *v = *duration;
            }
            Ok(())
        })?,
        EventCmd::SetValue { events, value } => edit_events(d, events, r, |e| {
            match &mut e.body {
                EventBody::Cc { value: v, .. }
                | EventBody::PitchBend { value: v }
                | EventBody::ChannelPressure { value: v }
                | EventBody::PolyPressure { value: v, .. } => *v = *value,
                _ => {}
            }
            Ok(())
        })?,
        EventCmd::SetDest { events, dest } => edit_events(d, events, r, |e| {
            e.dest = *dest;
            Ok(())
        })?,
        EventCmd::SetGroupChannel {
            events,
            group_channel,
        } => edit_events(d, events, r, |e| {
            e.group_channel = *group_channel;
            Ok(())
        })?,
        EventCmd::SetMute { events, mute } => edit_events(d, events, r, |e| {
            e.mute = *mute;
            Ok(())
        })?,
        EventCmd::Nudge {
            events,
            ticks,
            semitones,
        } => edit_events(d, events, r, |e| {
            e.tick = e.tick + *ticks;
            match &mut e.body {
                EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                    *pitch = shift_pitch(*pitch, i32::from(*semitones))
                }
                _ => {}
            }
            Ok(())
        })?,
    }
    Ok(())
}
