//! Paint and erase strokes as one undo step each (`M/DrumGridModel.swift:
//! 182-270`, Plan 4e Rulings 5-7). A press keeps its target by sequence,
//! track and row ID; nothing opens until the pointer moves or a click has
//! something to change. The stroke keeps the serial of the gesture it
//! opened and touches only that gesture, so a stroke whose gesture went
//! under it cannot end another editor's (slice 10c Outcome 5).

use vizdaw_ffi::VizError;
use vizdaw_ffi::drum::{
    DrumGridSettingsRow, DrumHitRow, DrumStrokeRow, drum_stroke_begin, drum_stroke_command,
    drum_stroke_extend,
};
use vizdaw_ffi::edit::EditCommand;

use super::{ERASE_LABEL, PAINT_LABEL};
use crate::document::DocumentModel;
use crate::editor::{DetailState, GESTURE_OPEN};
use crate::select_modify::GestureOrigin;

/// Extends a stroke to the pointer (the core's `drum_stroke_extend`; a
/// test seam, `set_drum_extend_for_test`).
pub type DrumExtend = fn(
    DrumStrokeRow,
    Vec<DrumHitRow>,
    i64,
    i64,
    Option<u8>,
    DrumGridSettingsRow,
) -> Result<DrumStrokeRow, VizError>;

/// The stroke in progress and what it was pressed on.
pub(super) struct DrumStroke {
    stroke: DrumStrokeRow,
    seq: u64,
    lane: u64,
    row: u64,
    /// The row's hits at the press.
    hits: Vec<DrumHitRow>,
    /// The serial of the gesture it opened (`None`: none yet).
    serial: Option<u64>,
}

impl DocumentModel {
    /// The stroke in progress.
    pub fn drum_stroke(&self) -> Option<&DrumStrokeRow> {
        self.drum_grid.stroke.as_ref().map(|s| &s.stroke)
    }

    /// Every later extend calls `f` instead of the core's.
    pub fn set_drum_extend_for_test(&mut self, f: DrumExtend) {
        self.drum_grid.extend = Some(f);
    }

    /// The open gesture is `s`'s own.
    fn drum_owns(&self, s: &DrumStroke) -> bool {
        s.serial.is_some() && self.gesture_serial() == s.serial
    }

    /// A press on row `row` of shown track `lane` at tick `at` (`snapped`:
    /// on the shared edit grid; `grab`: a hit's least width in ticks;
    /// `velocity`: the row's velocity lane under the pointer; `erase`: the
    /// eraser). A stroke still held (its release was lost) is cancelled
    /// first; indexes that do not exist do nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn drum_stroke_began(
        &mut self,
        lane: usize,
        row: usize,
        at: i64,
        snapped: i64,
        grab: i64,
        velocity: Option<u8>,
        erase: bool,
    ) {
        if self.drum_grid.stroke.is_some() {
            self.drum_stroke_cancelled();
        }
        let Some(seq) = self.drum_seq() else { return };
        let lanes = self.drum_lanes();
        let Some(l) = lanes.get(lane) else { return };
        let Some(r) = l.rows.get(row) else { return };
        self.drum_select(lane, row);
        let Ok(stroke) = drum_stroke_begin(
            lane as u32,
            row as u32,
            r.hits.clone(),
            at,
            snapped,
            grab,
            velocity,
            erase,
            self.drum_settings().row(),
        ) else {
            return;
        };
        self.drum_grid.stroke = Some(DrumStroke {
            stroke,
            seq,
            lane: l.lane,
            row: r.id,
            hits: r.hits.clone(),
            serial: None,
        });
    }

    /// The pointer moved past the view's dead zone: the first move opens
    /// the gesture, and each move sends the stroke's full command.
    pub fn drum_stroke_moved(&mut self, at: i64, grab: i64, velocity: Option<u8>) {
        let Some(mut s) = self.drum_grid.stroke.take() else {
            return;
        };
        let Ok(next) = self.drum_extended(&s, at, grab, velocity) else {
            self.drum_grid.stroke = Some(s);
            return;
        };
        s.stroke = next;
        if s.serial.is_some() {
            // Its gesture went under it (its track was removed).
            if !self.drum_owns(&s) {
                return;
            }
        } else if !self.open_drum_stroke(&mut s) {
            return;
        }
        let cmd = self.drum_command(&s);
        self.drum_grid.stroke = Some(s);
        self.update_gesture(cmd);
    }

    /// The pointer went up: a click toggles one hit, a drag ends its run.
    /// A failed extend cancels; a click opens only when it changes
    /// something.
    pub fn drum_stroke_ended(&mut self, at: i64, grab: i64, velocity: Option<u8>) {
        let Some(mut s) = self.drum_grid.stroke.take() else {
            return;
        };
        let Ok(next) = self.drum_extended(&s, at, grab, velocity) else {
            // No gesture may stay open.
            if self.drum_owns(&s) {
                self.cancel_gesture();
            }
            return;
        };
        s.stroke = next;
        let final_ = self.drum_command(&s);
        if s.serial.is_some() {
            if !self.drum_owns(&s) {
                return;
            }
        } else if final_.is_none() || !self.open_drum_stroke(&mut s) {
            return;
        }
        match final_ {
            Some(cmd) => self.end_gesture(Some(cmd)),
            None => {
                self.cancel_gesture();
            }
        }
    }

    /// Escape, or the grid went away: no undo step. Only the stroke's own
    /// gesture is cancelled.
    pub fn drum_stroke_cancelled(&mut self) {
        if let Some(s) = self.drum_grid.stroke.take()
            && self.drum_owns(&s)
        {
            self.cancel_gesture();
        }
    }

    /// The pane's detail changed from `before`: a stroke is cancelled
    /// when the kind or target changed, or the detail closed (Review
    /// Focus 1).
    pub(super) fn drum_stroke_detail_changed(&mut self, before: Option<&DetailState>) {
        if self.drum_grid.stroke.is_none() {
            return;
        }
        let same = match (before, self.layout().detail.as_ref()) {
            (Some(b), Some(n)) => b.kind == n.kind && b.target == n.target,
            _ => false,
        };
        if !same || !self.drum_showing() {
            self.drum_stroke_cancelled();
        }
    }

    fn drum_extended(
        &self,
        s: &DrumStroke,
        at: i64,
        grab: i64,
        velocity: Option<u8>,
    ) -> Result<DrumStrokeRow, VizError> {
        let extend = self.drum_grid.extend.unwrap_or(drum_stroke_extend);
        extend(
            s.stroke.clone(),
            s.hits.clone(),
            at,
            grab,
            velocity,
            self.drum_settings().row(),
        )
    }

    /// The stroke's command, aimed at the row it was pressed on by ID,
    /// never the row now at its index (Review Focus 2).
    fn drum_command(&self, s: &DrumStroke) -> Option<EditCommand> {
        drum_stroke_command(
            s.stroke.clone(),
            s.seq,
            s.lane,
            s.row,
            self.drum_settings().row(),
        )
        .ok()
        .flatten()
    }

    /// Opens the stroke's gesture on its track (`M/DrumGridModel.swift:
    /// 258-270`); a refusal while another drag is open says so.
    fn open_drum_stroke(&mut self, s: &mut DrumStroke) -> bool {
        let label = if s.stroke.erase {
            ERASE_LABEL
        } else {
            PAINT_LABEL
        };
        let pending = self.gesture().is_some();
        if !self.begin_gesture_from(label, vec![(s.seq, s.lane)], GestureOrigin::Other) {
            if pending {
                self.status = Some(GESTURE_OPEN.into());
            }
            return false;
        }
        s.serial = self.gesture_serial();
        true
    }
}
