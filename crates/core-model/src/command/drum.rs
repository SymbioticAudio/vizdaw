//! Drum-lane commands. Rows are a view over the lane's notes grouped by
//! `(pitch, dest override)`; membership is derived, never stored.

use super::Report;
use super::event::note_name;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::EventBody;
use crate::ids::{AnyId, DestId, DrumRowId, EventId, LaneId, SeqId};
use crate::lane::{DrumLayout, DrumRow, Lane};
use std::collections::BTreeSet;

/// Drum-lane edits.
#[derive(Debug, Clone, PartialEq)]
pub enum DrumCmd {
    /// Make a lane a drum lane (rows are created for its existing notes) or
    /// a normal lane again.
    MarkDrumLane {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// True to make it a drum lane.
        drum: bool,
    },
    /// Add a row; `(note, dest)` must be new in the lane.
    AddRow {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Row name.
        name: String,
        /// Note 0–127.
        note: u8,
        /// Destination override for the row's notes.
        dest: Option<DestId>,
    },
    /// Rename a row.
    RenameRow {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Row.
        row: DrumRowId,
        /// New name.
        name: String,
    },
    /// Move a row to display position `to` (clamped).
    ReorderRow {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Row.
        row: DrumRowId,
        /// New index.
        to: usize,
    },
    /// Remove a row and delete its notes from every take.
    RemoveRow {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Row.
        row: DrumRowId,
    },
    /// Move a row and its active-take notes to another lane of the same
    /// sequence, keeping their sound.
    MoveRowToLane {
        /// Sequence.
        seq: SeqId,
        /// Source lane.
        lane: LaneId,
        /// Row.
        row: DrumRowId,
        /// Target lane.
        to_lane: LaneId,
    },
    /// Move a row to a new lane of its own, placed after the source lane.
    SplitRowToLane {
        /// Sequence.
        seq: SeqId,
        /// Source lane.
        lane: LaneId,
        /// Row.
        row: DrumRowId,
    },
}

impl DrumCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            DrumCmd::MarkDrumLane { .. } => "Mark as Drum Lane",
            DrumCmd::AddRow { .. } => "Add Drum Row",
            DrumCmd::RenameRow { .. } => "Rename Drum Row",
            DrumCmd::ReorderRow { .. } => "Reorder Drum Row",
            DrumCmd::RemoveRow { .. } => "Remove Drum Row",
            DrumCmd::MoveRowToLane { .. } => "Move Drum Row to Lane",
            DrumCmd::SplitRowToLane { .. } => "Split Drum Row to Lane",
        }
    }
}

fn layout_mut(d: &mut Document, seq: SeqId, lane: LaneId) -> Result<&mut DrumLayout> {
    d.lane_mut(seq, lane)?
        .drum
        .as_mut()
        .ok_or_else(|| ModelError::invalid(format!("{lane} is not a drum lane")))
}

fn find_row(d: &Document, seq: SeqId, lane: LaneId, row: DrumRowId) -> Result<DrumRow> {
    d.lane(seq, lane)?
        .drum
        .as_ref()
        .and_then(|l| l.rows.iter().find(|r| r.id == row))
        .cloned()
        .ok_or(ModelError::UnknownId(AnyId::DrumRow(row)))
}

/// Notes belonging to `row` in one lane; `active_only` limits to the
/// active take.
pub(crate) fn row_events(lane: &Lane, row: &DrumRow, active_only: bool) -> Vec<EventId> {
    lane.takes
        .iter()
        .filter(|t| !active_only || t.id == lane.active_take)
        .flat_map(|t| t.events.values())
        .filter(|e| matches!(e.body, EventBody::Note { pitch, .. } if row.matches(pitch, e.dest)))
        .map(|e| e.id)
        .collect()
}

/// Makes a lane a drum lane (if it is not one already), creating a row for
/// every `(pitch, dest)` among its active-take notes.
fn mark_drum(d: &mut Document, seq: SeqId, lane: LaneId, r: &mut Report) -> Result<()> {
    let l = d.lane(seq, lane)?;
    if l.drum.is_some() {
        return Ok(());
    }
    let keys: BTreeSet<(u8, Option<DestId>)> = l
        .active()
        .map(|t| {
            t.events
                .values()
                .filter_map(|e| match e.body {
                    EventBody::Note { pitch, .. } => Some((pitch, e.dest)),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let mut layout = DrumLayout::default();
    for (note, dest) in keys {
        let id = d.counters_mut().alloc_drum_row();
        layout.rows.push_back(DrumRow {
            id,
            name: note_name(note),
            note,
            dest,
            show_velocity: false,
        });
        r.created.push(AnyId::DrumRow(id));
    }
    d.lane_mut(seq, lane)?.drum = Some(layout);
    Ok(())
}

fn move_row(
    d: &mut Document,
    seq: SeqId,
    lane: LaneId,
    row: DrumRowId,
    to_lane: LaneId,
    r: &mut Report,
) -> Result<()> {
    let mut moved = find_row(d, seq, lane, row)?;
    let src = d.lane(seq, lane)?.clone();
    let dst = d.lane(seq, to_lane)?.clone();
    if lane == to_lane {
        return Ok(());
    }
    // A normal target lane becomes a drum lane with rows for its own notes
    // first, so none of them drop out of the drum view.
    mark_drum(d, seq, to_lane, r)?;
    let ids = row_events(&src, &moved, true);
    // Keep the sound: pin the source lane's destination when it differs.
    let pin = if moved.dest.is_none() && src.midi_dest != dst.midi_dest {
        src.midi_dest
    } else {
        None
    };
    if pin.is_some() {
        moved.dest = pin;
    }
    for id in ids {
        let (mut ev, _) = d.remove_event(id)?;
        if pin.is_some() {
            ev.dest = pin;
        }
        d.insert_event(seq, to_lane, None, ev)?;
    }
    let src_layout = layout_mut(d, seq, lane)?;
    src_layout.rows.retain(|x| x.id != row);
    let layout = layout_mut(d, seq, to_lane)?;
    if !layout
        .rows
        .iter()
        .any(|x| x.matches(moved.note, moved.dest))
    {
        layout.rows.push_back(moved);
    } else {
        r.warnings
            .push(format!("{row} merged into an existing row of {to_lane}"));
    }
    Ok(())
}

pub(crate) fn apply(d: &mut Document, cmd: &DrumCmd, r: &mut Report) -> Result<()> {
    match cmd {
        DrumCmd::MarkDrumLane { seq, lane, drum } => {
            if *drum {
                mark_drum(d, *seq, *lane, r)?;
            } else {
                d.lane_mut(*seq, *lane)?.drum = None;
            }
        }
        DrumCmd::AddRow {
            seq,
            lane,
            name,
            note,
            dest,
        } => {
            if *note > 127 {
                return Err(ModelError::range("drum_row.note", note));
            }
            if let Some(id) = dest {
                if !d.instruments().contains_key(id) {
                    return Err(ModelError::UnknownId(AnyId::Dest(*id)));
                }
            }
            let layout = layout_mut(d, *seq, *lane)?;
            if layout.rows.iter().any(|x| x.matches(*note, *dest)) {
                return Err(ModelError::invalid(
                    "a row for that note and destination exists",
                ));
            }
            let id = d.counters_mut().alloc_drum_row();
            layout_mut(d, *seq, *lane)?.rows.push_back(DrumRow {
                id,
                name: name.clone(),
                note: *note,
                dest: *dest,
                show_velocity: false,
            });
            r.created.push(AnyId::DrumRow(id));
        }
        DrumCmd::RenameRow {
            seq,
            lane,
            row,
            name,
        } => {
            let layout = layout_mut(d, *seq, *lane)?;
            let x = layout
                .rows
                .iter_mut()
                .find(|x| x.id == *row)
                .ok_or(ModelError::UnknownId(AnyId::DrumRow(*row)))?;
            x.name = name.clone();
        }
        DrumCmd::ReorderRow { seq, lane, row, to } => {
            let layout = layout_mut(d, *seq, *lane)?;
            let i = layout
                .rows
                .iter()
                .position(|x| x.id == *row)
                .ok_or(ModelError::UnknownId(AnyId::DrumRow(*row)))?;
            let x = layout.rows.remove(i);
            let at = (*to).min(layout.rows.len());
            layout.rows.insert(at, x);
        }
        DrumCmd::RemoveRow { seq, lane, row } => {
            let x = find_row(d, *seq, *lane, *row)?;
            let ids = row_events(d.lane(*seq, *lane)?, &x, false);
            for id in ids {
                d.remove_event(id)?;
                r.removed.push(AnyId::Event(id));
            }
            layout_mut(d, *seq, *lane)?.rows.retain(|y| y.id != *row);
            r.removed.push(AnyId::DrumRow(*row));
        }
        DrumCmd::MoveRowToLane {
            seq,
            lane,
            row,
            to_lane,
        } => move_row(d, *seq, *lane, *row, *to_lane, r)?,
        DrumCmd::SplitRowToLane { seq, lane, row } => {
            let x = find_row(d, *seq, *lane, *row)?;
            let src = d.lane(*seq, *lane)?.clone();
            let c = d.counters_mut();
            let (new_lane, take) = (c.alloc_lane(), c.alloc_take());
            let mut nl = Lane::new(new_lane, take, x.name.clone());
            nl.midi_dest = src.midi_dest;
            nl.drum = Some(DrumLayout::default());
            let s = d.seq_mut(*seq)?;
            let at = s.lane_index(*lane).map_or(s.lanes.len(), |i| i + 1);
            s.lanes.insert(at, nl);
            r.created.extend([AnyId::Lane(new_lane), AnyId::Take(take)]);
            move_row(d, *seq, *lane, *row, new_lane, r)?;
        }
    }
    Ok(())
}
