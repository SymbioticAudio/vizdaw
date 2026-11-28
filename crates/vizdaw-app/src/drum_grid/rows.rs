//! The row and track edits of the Drum Grid (`M/DrumGridModel.swift:272-370`):
//! Add Row, Remove Row, Split Row to Own Track, Show Velocity, Rename, a
//! row header's drop, and the track menu's play quantize and nudge. Each
//! is one edit (one undo step). The row edits act on the selected row by
//! track and row ID, read from the shown sequence's drum tracks, never on
//! the row now at some index. The rules are the core's (Ruling 1).

use std::sync::OnceLock;

use vizdaw_ffi::drum::{
    DrumEdit, DrumLaneRow, DrumRowRow, QuantizeChoiceRow, drum_nudged, drum_row_drop,
};
use vizdaw_ffi::edit::{EditCommand, LaneEdit, QuantizeRow};

use super::{DrumRowKey, NO_ROW, NO_TRACK_TO_ADD, every_note_taken};
use crate::document::DocumentModel;

/// The track menu's play quantize choices, built once
/// (`M/DrumGridModel.swift:351`).
pub fn drum_quantize_choices() -> &'static [QuantizeChoiceRow] {
    static CHOICES: OnceLock<Vec<QuantizeChoiceRow>> = OnceLock::new();
    CHOICES.get_or_init(|| vizdaw_ffi::drum::drum_quantize_choices().unwrap_or_default())
}

/// The title of `lane`'s play quantize; an error reads "Custom"
/// (`M/DrumGridModel.swift:353-355`).
pub fn drum_quantize_title(lane: &DrumLaneRow) -> String {
    vizdaw_ffi::drum::drum_quantize_title(lane.play_quantize.clone())
        .unwrap_or_else(|_| "Custom".into())
}

impl DocumentModel {
    fn drum_edit(&mut self, edit: DrumEdit) -> bool {
        self.edit(EditCommand::Drum { edit })
    }

    /// The shown sequence and the selected row while it exists; else the
    /// status says to select one.
    fn drum_selected_or_say(&mut self) -> Option<(u64, DrumLaneRow, DrumRowRow)> {
        match (self.drum_seq(), self.drum_selected()) {
            (Some(seq), Some((lane, row))) => Some((seq, lane, row)),
            _ => {
                self.status = Some(NO_ROW.into());
                None
            }
        }
    }

    /// A row for the next free kit note in the current track; it becomes
    /// the selected row (`M/DrumGridModel.swift:279-294`).
    pub fn drum_add_row(&mut self) -> bool {
        let (Some(seq), Some(lane)) = (self.drum_seq(), self.drum_current_lane()) else {
            self.status = Some(NO_TRACK_TO_ADD.into());
            return false;
        };
        let used = lane.rows.iter().map(|r| r.note).collect();
        let Some(new) = vizdaw_ffi::drum::drum_next_row(used).ok().flatten() else {
            self.status = Some(every_note_taken(&lane.name));
            return false;
        };
        let note = new.note;
        if !self.drum_edit(DrumEdit::AddRow {
            seq,
            lane: lane.lane,
            name: new.name,
            note,
            dest: None,
        }) {
            return false;
        }
        let lanes = self.drum_lanes();
        if let Some(r) = lanes
            .iter()
            .find(|l| l.lane == lane.lane)
            .and_then(|l| l.rows.iter().find(|r| r.note == note && r.dest.is_none()))
        {
            self.drum_grid.selected = Some(DrumRowKey {
                lane: lane.lane,
                row: r.id,
            });
        }
        true
    }

    /// Removes the selected row and its notes (`:298-304`).
    pub fn drum_remove_selected_row(&mut self) -> bool {
        let Some((seq, lane, row)) = self.drum_selected_or_say() else {
            return false;
        };
        self.drum_edit(DrumEdit::RemoveRow {
            seq,
            lane: lane.lane,
            row: row.id,
        })
    }

    /// Moves the selected row to a track of its own (`:307-313`).
    pub fn drum_split_selected_row(&mut self) -> bool {
        let Some((seq, lane, row)) = self.drum_selected_or_say() else {
            return false;
        };
        self.drum_edit(DrumEdit::SplitRow {
            seq,
            lane: lane.lane,
            row: row.id,
        })
    }

    /// Shows or hides the selected row's velocity lane (`None` toggles).
    /// Asking for the state it is in sends nothing and is done (`:317-325`).
    pub fn drum_show_velocity(&mut self, on: Option<bool>) -> bool {
        let Some((seq, lane, row)) = self.drum_selected_or_say() else {
            return false;
        };
        let show = on.unwrap_or(!row.show_velocity);
        if show == row.show_velocity {
            return true;
        }
        self.drum_edit(DrumEdit::ShowVelocity {
            seq,
            lane: lane.lane,
            row: row.id,
            show,
        })
    }

    /// Renames row `key` to `name`, trimmed. An empty name, the current
    /// one or a row the grid does not show sends nothing (`:328-333`).
    pub fn drum_rename(&mut self, key: DrumRowKey, name: &str) -> bool {
        let name = name.trim();
        let Some(seq) = self.drum_seq() else {
            return false;
        };
        let lanes = self.drum_lanes();
        let Some(current) = lanes
            .iter()
            .find(|l| l.lane == key.lane)
            .and_then(|l| l.rows.iter().find(|r| r.id == key.row))
        else {
            return false;
        };
        if name.is_empty() || name == current.name {
            return false;
        }
        self.drum_edit(DrumEdit::RenameRow {
            seq,
            lane: key.lane,
            row: key.row,
            name: name.into(),
        })
    }

    /// Row `row` of shown track `lane` dragged by its header and released
    /// at height `y` (content coordinates): reordered, moved to another
    /// drum track or split into a new one (the core decides). Indexes that
    /// no longer exist, and a drop that changes nothing, send nothing
    /// (`:339-347`).
    pub fn drum_drop(&mut self, lane: usize, row: usize, y: f64) -> bool {
        let Some(seq) = self.drum_seq() else {
            return false;
        };
        let lanes = self.drum_lanes();
        let Some(r) = lanes.get(lane).and_then(|l| l.rows.get(row)) else {
            return false;
        };
        let (Ok(lane_index), Ok(row_index)) = (u32::try_from(lane), u32::try_from(row)) else {
            return false;
        };
        let cmd = drum_row_drop(
            (*self.drum_layout()).clone(),
            lanes.iter().map(|l| l.lane).collect(),
            seq,
            lane_index,
            r.id,
            row_index,
            y,
        );
        match cmd {
            Ok(Some(cmd)) => self.edit(cmd),
            _ => false,
        }
    }

    /// Sets track `lane`'s play quantize (`None`: off) (`:358-361`).
    pub fn drum_set_quantize(&mut self, lane: u64, q: Option<QuantizeRow>) -> bool {
        let Some(seq) = self.drum_seq() else {
            return false;
        };
        self.edit(EditCommand::Lane {
            edit: LaneEdit::SetPlayQuantize {
                seq,
                lane,
                quantize: q,
            },
        })
    }

    /// Moves shown track `lane`'s nudge (play shift) by `steps` steps; at
    /// the end of its range it sends nothing and is done (`:365-370`).
    pub fn drum_nudge(&mut self, lane: u64, steps: i64) -> bool {
        let Some(seq) = self.drum_seq() else {
            return false;
        };
        let Some(current) = self
            .drum_lanes()
            .iter()
            .find(|l| l.lane == lane)
            .map(|l| l.play_shift)
        else {
            return false;
        };
        let shift = drum_nudged(current, steps).unwrap_or(current);
        if shift == current {
            return true;
        }
        self.edit(EditCommand::Lane {
            edit: LaneEdit::SetPlayShift { seq, lane, shift },
        })
    }
}
