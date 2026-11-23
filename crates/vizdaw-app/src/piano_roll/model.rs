//! One piano roll (Swift `PianoRollModel`, `EditorModel+Pencil`): which
//! tracks it shows, which one new notes go to, track colours, the notes
//! and the selection it makes. The state lives in the document's editor
//! model (one roll: the detail pane's); the operations are
//! `DocumentModel`'s, since they read and edit the document. The shown
//! tracks' notes are built from the document's cached reads, once per
//! snapshot version, target and shown-track list.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
use vizdaw_ffi::events::{EventKindRow, EventRow};
use vizdaw_ffi::selection::{SelectionRow, TickRange};
use vizdaw_ffi::snapshot::LaneRow;
use vizdaw_ffi::tracks::TrackKindRow;

use crate::arrangement::bar_at;
use crate::audition::AuditionNote;
use crate::document::DocumentModel;
use crate::editor::{DetailEditorKind, EditorTarget, Tool};
use crate::piano_roll::AuditionRequest;

/// A request that the roll showing `target` scroll to `tick`; a new
/// `serial` is a new request, so asking again scrolls again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PianoRollReveal {
    pub target: EditorTarget,
    pub tick: i64,
    pub serial: u64,
}

/// One shown track and its notes, in time order, with the longest
/// note's duration (the hit tests' search bound).
#[derive(Debug, Clone, PartialEq)]
pub struct LaneNotes {
    pub lane: LaneRef,
    notes: Arc<[EventRow]>,
    longest: i64,
}

impl LaneNotes {
    /// `notes` must be in time order (as the core gives them).
    pub fn new(lane: LaneRef, notes: Arc<[EventRow]>) -> Self {
        debug_assert!(
            notes.windows(2).all(|w| w[0].tick <= w[1].tick),
            "a track's notes must be in time order"
        );
        let longest = notes.iter().map(|n| n.duration.max(0)).max().unwrap_or(0);
        LaneNotes {
            lane,
            notes,
            longest,
        }
    }

    pub fn notes(&self) -> &Arc<[EventRow]> {
        &self.notes
    }

    /// The longest note's duration in ticks (0 for none).
    pub fn longest(&self) -> i64 {
        self.longest
    }
}

#[derive(Clone, PartialEq)]
struct RollKey {
    version: u64,
    target: EditorTarget,
    lanes: Option<Vec<u64>>,
}

/// The roll's own state: the chosen track, the pending reveal and the
/// cached shown tracks. Not saved.
pub struct PianoRollModel {
    /// The track of the last pressed, selected or chosen note.
    pub(crate) chosen_lane: Option<LaneRef>,
    pub(crate) reveal: Option<PianoRollReveal>,
    reveal_serial: u64,
    /// The note sounding under the pointer: the one source of truth.
    pub(crate) sounding: Option<AuditionNote>,
    built: Mutex<Option<(RollKey, Arc<[LaneNotes]>)>>,
    builds: Mutex<u64>,
}

impl Default for PianoRollModel {
    fn default() -> Self {
        PianoRollModel {
            chosen_lane: None,
            reveal: None,
            reveal_serial: 0,
            sounding: None,
            built: Mutex::new(None),
            builds: Mutex::new(0),
        }
    }
}

impl Clone for PianoRollModel {
    /// The chosen track and the reveal; the cache starts empty.
    fn clone(&self) -> Self {
        PianoRollModel {
            chosen_lane: self.chosen_lane,
            reveal: self.reveal,
            reveal_serial: self.reveal_serial,
            ..PianoRollModel::default()
        }
    }
}

impl fmt::Debug for PianoRollModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PianoRollModel")
            .field("chosen_lane", &self.chosen_lane)
            .field("reveal", &self.reveal)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl PianoRollModel {
    /// The track last chosen (by a press, a selection or Insert Into).
    pub fn chosen_lane(&self) -> Option<LaneRef> {
        self.chosen_lane
    }

    /// The note sounding now, if any.
    pub fn sounding(&self) -> Option<AuditionNote> {
        self.sounding
    }

    /// The scroll the Pencil in the arrangement asked for, if any.
    pub fn reveal(&self) -> Option<PianoRollReveal> {
        self.reveal
    }

    /// How many times the shown tracks were built (the cache's tests).
    #[doc(hidden)]
    pub fn builds(&self) -> u64 {
        *lock(&self.builds)
    }
}

/// Whether a track shows when the detail names none: it has an
/// instrument, notes or regions.
fn shown_by_default(row: &LaneRow) -> bool {
    row.midi_dest.is_some() || row.midi_event_count > 0 || row.region_count > 0
}

fn notes_of(events: &[EventRow]) -> Arc<[EventRow]> {
    events
        .iter()
        .filter(|e| e.kind == EventKindRow::Note)
        .cloned()
        .collect()
}

impl DocumentModel {
    /// The piano roll's state (chosen track, reveal).
    pub fn piano_roll(&self) -> &PianoRollModel {
        &self.editor.roll
    }

    /// The shown tracks with their notes, in sequence order (none with no
    /// detail open). A track target always shows its own track and the
    /// detail's extra ones; a sequence target shows exactly the detail's,
    /// else every track with an instrument, notes or regions.
    pub fn roll_lane_notes(&self) -> Arc<[LaneNotes]> {
        let Some(d) = &self.layout().detail else {
            return Arc::from([]);
        };
        let key = RollKey {
            version: self.snapshot().version,
            target: d.target,
            lanes: d.lanes(),
        };
        if let Some((k, built)) = &*lock(&self.editor.roll.built)
            && *k == key
        {
            return built.clone();
        }
        let (built, fresh) = self.build_roll(&key);
        *lock(&self.editor.roll.builds) += 1;
        // A build from a busy core's older reads is not this version's:
        // it is built again next time.
        if fresh {
            *lock(&self.editor.roll.built) = Some((key, built.clone()));
        }
        built
    }

    /// The shown tracks' notes, and whether every read was fresh.
    fn build_roll(&self, key: &RollKey) -> (Arc<[LaneNotes]>, bool) {
        let seq = key.target.seq();
        let extra = key.lanes.as_deref();
        let wanted = |row: &LaneRow| match (key.target.lane(), extra) {
            (Some(lane), _) => row.id == lane || extra.is_some_and(|e| e.contains(&row.id)),
            (None, Some(exact)) => exact.contains(&row.id),
            (None, None) => shown_by_default(row),
        };
        let rows = self.lane_rows_read(seq);
        let mut fresh = rows.fresh;
        let built = rows
            .rows
            .iter()
            .filter(|r| wanted(r))
            .map(|r| {
                let events = self.lane_events_read(seq, r.id);
                fresh &= events.fresh;
                LaneNotes::new(LaneRef { seq, lane: r.id }, notes_of(&events.rows))
            })
            .collect();
        (built, fresh)
    }

    /// The shown tracks, in sequence order.
    pub fn roll_shown_lanes(&self) -> Vec<LaneRef> {
        self.roll_lane_notes().iter().map(|l| l.lane).collect()
    }

    pub fn roll_is_shown(&self, lane: u64) -> bool {
        self.roll_lane_notes().iter().any(|l| l.lane.lane == lane)
    }

    /// Shows or hides `lane` (the "Tracks" menu); the target's own track
    /// always shows. Written to the detail's `lanes`: a track target with
    /// no extras writes none.
    pub fn roll_toggle_shown(&mut self, lane: u64) {
        let Some(d) = &self.layout().detail else {
            return;
        };
        let target = d.target;
        if target.lane() == Some(lane) {
            return;
        }
        let mut set: HashSet<u64> = match target.lane() {
            None => self.roll_shown_lanes().iter().map(|l| l.lane).collect(),
            Some(_) => d.lanes().unwrap_or_default().into_iter().collect(),
        };
        if !set.remove(&lane) {
            set.insert(lane);
        }
        let ordered: Vec<u64> = self
            .lane_rows(target.seq())
            .iter()
            .map(|r| r.id)
            .filter(|id| set.contains(id))
            .collect();
        let lanes = (!ordered.is_empty() || target.lane().is_none()).then_some(ordered);
        self.update_layout(|l| {
            if let Some(d) = l.detail.as_mut() {
                d.set_lanes(lanes);
            }
        });
    }

    /// The track the Pencil inserts into: the chosen one while it shows,
    /// else the target's own, else the first shown.
    pub fn roll_edit_lane(&self) -> Option<LaneRef> {
        let target = self.layout().detail.as_ref()?.target;
        let shown = self.roll_shown_lanes();
        if let Some(c) = self.editor.roll.chosen_lane
            && shown.contains(&c)
        {
            return Some(c);
        }
        if let Some(lane) = target.lane() {
            return Some(LaneRef {
                seq: target.seq(),
                lane,
            });
        }
        shown.first().copied()
    }

    /// "Insert Into": new notes go to `lane` while it shows.
    pub fn roll_set_edit_lane(&mut self, lane: LaneRef) {
        self.editor.roll.chosen_lane = Some(lane);
    }

    /// The track's colour index: its position in its sequence (0 when
    /// missing). The palette cycles every 8.
    pub fn roll_color_index(&self, key: LaneRef) -> usize {
        self.lane_rows(key.seq)
            .iter()
            .position(|r| r.id == key.lane)
            .unwrap_or(0)
    }

    pub fn roll_row(&self, key: LaneRef) -> Option<LaneRow> {
        self.lane_rows(key.seq)
            .iter()
            .find(|r| r.id == key.lane)
            .cloned()
    }

    /// The track's notes, in time order.
    pub fn roll_notes(&self, key: LaneRef) -> Arc<[EventRow]> {
        if let Some(l) = self.roll_lane_notes().iter().find(|l| l.lane == key) {
            return l.notes.clone();
        }
        notes_of(&self.lane_events(key.seq, key.lane))
    }

    /// A block target's range (at least one tick).
    pub fn roll_focus_range(&self) -> Option<TickRange> {
        match self.layout().detail.as_ref()?.target {
            EditorTarget::Block { start, end, .. } => Some(TickRange {
                start,
                end: end.max(start.saturating_add(1)),
            }),
            _ => None,
        }
    }

    /// Where a note sounds: its own destination, else its track's.
    pub fn roll_dest(&self, note: &EventRow, key: LaneRef) -> Option<u64> {
        note.dest
            .or_else(|| self.roll_row(key).and_then(|r| r.midi_dest))
    }

    /// Sounds `pitch` (ending the previous note); no destination is
    /// silence only. The same note again does nothing.
    pub fn roll_sound(&mut self, pitch: u8, velocity: u16, dest: Option<u64>) {
        let next = dest.map(|dest| AuditionNote { dest, pitch });
        if next == self.editor.roll.sounding {
            return;
        }
        self.roll_silence();
        let Some(next) = next else {
            return;
        };
        self.audition_on(next, velocity);
        self.editor.roll.sounding = Some(next);
    }

    /// Ends the sounding note, if any.
    pub fn roll_silence(&mut self) {
        if let Some(s) = self.editor.roll.sounding.take() {
            self.audition_off(s);
        }
    }

    /// Acts on what the canvas asked of the audition, in order.
    pub fn roll_audition(&mut self, requests: &[AuditionRequest]) {
        for r in requests {
            match *r {
                AuditionRequest::Sound {
                    pitch,
                    velocity,
                    dest,
                } => self.roll_sound(pitch, velocity, dest),
                AuditionRequest::Silence => self.roll_silence(),
            }
        }
    }

    /// Selects `notes` (with their tracks; no time range); `adding` keeps
    /// the selection. The last note's track becomes the chosen one.
    pub fn roll_select(&mut self, notes: &[(LaneRef, u64)], adding: bool) {
        let mut s = if adding {
            self.selection().clone()
        } else {
            SelectionRow {
                time_range: None,
                lanes: vec![],
                events: vec![],
            }
        };
        for (lane, id) in notes {
            if !s.events.contains(id) {
                s.events.push(*id);
            }
            if !s.lanes.contains(lane) {
                s.lanes.push(*lane);
            }
        }
        self.set_selection(s);
        if let Some((lane, _)) = notes.last() {
            self.editor.roll.chosen_lane = Some(*lane);
        }
    }

    /// Shift-click: adds note `id` (with its track, which becomes the
    /// chosen one) or removes it (its track stays selected).
    pub fn roll_toggle(&mut self, id: u64, lane: LaneRef) {
        let mut s = self.selection().clone();
        match s.events.iter().position(|e| *e == id) {
            Some(i) => {
                s.events.remove(i);
            }
            None => {
                s.events.push(id);
                if !s.lanes.contains(&lane) {
                    s.lanes.push(lane);
                }
                self.editor.roll.chosen_lane = Some(lane);
            }
        }
        self.set_selection(s);
    }

    /// Delete: the selected notes of the shown tracks, as one "Delete
    /// Events" step; nothing selected there sends nothing. Whether an
    /// edit applied.
    pub fn roll_delete_selected(&mut self) -> bool {
        let shown: HashSet<u64> = self
            .roll_lane_notes()
            .iter()
            .flat_map(|l| l.notes.iter().map(|n| n.id))
            .collect();
        let ids: Vec<u64> = self
            .selection()
            .events
            .iter()
            .copied()
            .filter(|id| shown.contains(id))
            .collect();
        if ids.is_empty() {
            return false;
        }
        self.edit(EditCommand::Event {
            edit: EventEdit::Delete { events: ids },
        })
    }

    /// A Pencil click in the arrangement on track `lane` of `seq` at
    /// `tick`: a MIDI or instrument track is selected and its piano roll
    /// opens with the Pencil, asked to show the clicked bar. An audio or
    /// unknown track changes nothing. Whether it opened.
    pub fn pencil_opens_piano_roll(&mut self, seq: u64, lane: u64, tick: i64) -> bool {
        let rows = self.lane_rows(seq);
        let Some(row) = rows.iter().find(|r| r.id == lane) else {
            return false;
        };
        if row.kind == TrackKindRow::Audio {
            return false;
        }
        let meter = self
            .sequence_info(seq)
            .map(|i| i.meter.clone())
            .unwrap_or_default();
        let start = bar_at(tick.max(0), &meter).start;
        self.set_tool(Tool::Pencil);
        self.set_selection(SelectionRow {
            time_range: None,
            lanes: vec![LaneRef { seq, lane }],
            events: vec![],
        });
        let target = EditorTarget::Lane { seq, lane };
        self.open_detail(target, Some(DetailEditorKind::PianoRoll));
        let roll = &mut self.editor.roll;
        roll.reveal_serial += 1;
        roll.reveal = Some(PianoRollReveal {
            target,
            tick: start,
            serial: roll.reveal_serial,
        });
        true
    }
}
