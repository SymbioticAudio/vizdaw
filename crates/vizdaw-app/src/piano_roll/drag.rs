//! Note drags, the pencil and the eraser (Swift `NoteDrag`, `Pencil`,
//! `Eraser`), and the roll's canvas glue (Swift `PianoRollView`'s
//! gesture part): the tool machine's events become selections, one
//! gesture per drag, and audition requests the canvas passes on. As
//! `document/block_drag.rs` does for the arrangement's blocks.

use std::collections::HashSet;

use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
use vizdaw_ffi::events::{EventKindRow, EventRow, MeterEntryRow};

use super::LaneNotes;
use super::geometry::{CursorKind, NoteZone, PitchAxis, RollGeometry, RollHit, band_rect};
use crate::arrangement::{
    MachineEvent, MeterBars, Modifiers, PPQ, Point, Rect, TimeAxis, ToolMachine, bar_at,
};
use crate::document::DocumentModel;
use crate::editor::{GESTURE_OPEN, Snap, Tool};

/// Ticks held within the time axis's reach, so sums cannot overflow.
fn clamp_tick(t: i64) -> i64 {
    t.clamp(-TimeAxis::LIMIT, TimeAxis::LIMIT)
}

// MARK: NoteDrag

/// A note drag: what it carries and its full target for a pointer moved
/// by `raw_ticks` and `dy` from the press.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteDrag {
    pub zone: NoteZone,
    pub pressed: EventRow,
    pub lane: LaneRef,
    /// The dragged events, the pressed one first.
    pub events: Vec<u64>,
    /// The lowest and highest pitch among them (the transpose limits).
    pub lowest: u8,
    pub highest: u8,
    /// ⌥ when the drag began (start and middle zones only).
    pub copy: bool,
}

impl NoteDrag {
    pub fn new(
        zone: NoteZone,
        pressed: EventRow,
        lane: LaneRef,
        events: Vec<u64>,
        lowest: u8,
        highest: u8,
        copy: bool,
    ) -> Self {
        NoteDrag {
            zone,
            pressed,
            lane,
            events,
            lowest,
            highest,
            copy: copy && zone != NoteZone::End,
        }
    }

    /// The drag's undo step.
    pub fn label(&self) -> &'static str {
        match (self.zone, self.copy) {
            (NoteZone::End, _) => "Resize Notes",
            (_, true) => "Copy Notes",
            (NoteZone::Start, false) => "Move Notes",
            (NoteZone::Middle, false) => "Transpose Notes",
        }
    }

    /// Every selected note of the shown tracks when the pressed one is
    /// selected, the pressed one first; else the pressed one alone.
    pub fn events(pressed: u64, selected: &HashSet<u64>, lanes: &[LaneNotes]) -> Vec<u64> {
        if !selected.contains(&pressed) {
            return vec![pressed];
        }
        let others = lanes
            .iter()
            .flat_map(|l| l.notes().iter().map(|n| n.id))
            .filter(|id| *id != pressed && selected.contains(id));
        std::iter::once(pressed).chain(others).collect()
    }

    /// The time offset that puts the pressed note's start on the grid.
    pub fn time_delta(raw_ticks: i64, start: i64, snap: &Snap, meter: &[MeterEntryRow]) -> i64 {
        let start = clamp_tick(start);
        let raw = clamp_tick(raw_ticks);
        if !snap.enabled {
            return raw;
        }
        snap.apply(clamp_tick(start + raw), meter) - start
    }

    /// Whole rows for a vertical move of `dy` points (up positive).
    pub fn raw_semitones(dy: f32, row_height: f32) -> i64 {
        if !dy.is_finite() || row_height <= 0. || row_height.is_nan() {
            return 0;
        }
        (-dy / row_height).round().clamp(-1_000., 1_000.) as i64
    }

    /// `raw` clamped so every dragged note stays inside 0–127.
    pub fn semitones(&self, raw: i64) -> i8 {
        // Malformed bounds (lowest > highest, pitches above 127) still
        // land in i8's range.
        let s = raw
            .max(-i64::from(self.lowest))
            .min(127 - i64::from(self.highest));
        s.clamp(-127, 127) as i8
    }

    /// The duration change that puts the pressed note's end on the grid;
    /// the note keeps at least one tick.
    pub fn duration_delta(&self, raw_ticks: i64, snap: &Snap, meter: &[MeterEntryRow]) -> i64 {
        let tick = clamp_tick(self.pressed.tick);
        let end = clamp_tick(tick + clamp_tick(self.pressed.duration));
        let mut new_end = clamp_tick(end + clamp_tick(raw_ticks));
        if snap.enabled {
            new_end = snap.apply(new_end, meter);
        }
        new_end = new_end.max(tick + 1);
        new_end - end
    }

    /// The full target (every move sends it whole); `None` when nothing
    /// would change.
    pub fn command(
        &self,
        raw_ticks: i64,
        dy: f32,
        row_height: f32,
        snap: &Snap,
        meter: &[MeterEntryRow],
    ) -> Option<EditCommand> {
        let events = self.events.clone();
        let edit = match self.zone {
            NoteZone::Start => {
                let d = Self::time_delta(raw_ticks, self.pressed.tick, snap, meter);
                if d == 0 {
                    return None;
                }
                if self.copy {
                    EventEdit::Duplicate {
                        events,
                        ticks: d,
                        semitones: 0,
                    }
                } else {
                    EventEdit::Move {
                        events,
                        delta: d,
                        to_lane: None,
                    }
                }
            }
            NoteZone::Middle => {
                let s = self.semitones(Self::raw_semitones(dy, row_height));
                if s == 0 {
                    return None;
                }
                if self.copy {
                    EventEdit::Duplicate {
                        events,
                        ticks: 0,
                        semitones: s,
                    }
                } else {
                    EventEdit::Nudge {
                        events,
                        ticks: 0,
                        semitones: s,
                    }
                }
            }
            NoteZone::End => {
                let d = self.duration_delta(raw_ticks, snap, meter);
                if d == 0 {
                    return None;
                }
                EventEdit::Resize { events, delta: d }
            }
        };
        Some(EditCommand::Event { edit })
    }

    /// The pitch the pressed note sounds at: transposed in the middle
    /// zone, its own elsewhere.
    pub fn sounding_pitch(&self, dy: f32, row_height: f32) -> u8 {
        if self.zone != NoteZone::Middle {
            return self.pressed.pitch;
        }
        let s = self.semitones(Self::raw_semitones(dy, row_height));
        (i64::from(self.pressed.pitch) + i64::from(s)).clamp(0, 127) as u8
    }
}

// MARK: Pencil

/// The pencil: new notes on the grid.
pub struct Pencil;

impl Pencil {
    /// MIDI 1.0 velocity 100 as a 16-bit velocity.
    pub const VELOCITY: u16 = 100 << 9;
    /// The undo step of a pencil click or drag.
    pub const LABEL: &'static str = "Insert Note";

    /// One grid step at `tick`; a quarter note with snap off.
    pub fn step(tick: i64, snap: &Snap, meter: &[MeterEntryRow]) -> i64 {
        if !snap.enabled {
            return PPQ;
        }
        snap.step(&bar_at(tick, meter).meter)
    }

    /// A new note's start: the grid line at or before `tick`.
    pub fn start(tick: i64, snap: &Snap, meter: &[MeterEntryRow]) -> i64 {
        snap.floor(clamp_tick(tick), meter)
    }

    /// The length painted from `start` to the pointer at `tick`: snapped
    /// and at least one step; with snap off the raw length, at least a
    /// 32nd.
    pub fn length(start: i64, tick: i64, snap: &Snap, meter: &[MeterEntryRow]) -> i64 {
        let start = clamp_tick(start);
        let tick = clamp_tick(tick);
        let step = Self::step(start, snap, meter);
        if !snap.enabled {
            return if tick > start {
                (tick - start).max(PPQ / 8)
            } else {
                PPQ / 8
            };
        }
        if tick <= start {
            return step;
        }
        (snap.apply(tick, meter) - start).max(step)
    }

    pub fn note(tick: i64, pitch: u8, length: i64) -> EventRow {
        EventRow {
            pitch,
            velocity: Self::VELOCITY,
            duration: length,
            ..EventRow::blank(0, tick, EventKindRow::Note)
        }
    }

    pub fn command(lane: LaneRef, tick: i64, pitch: u8, length: i64) -> EditCommand {
        EditCommand::Event {
            edit: EventEdit::Insert {
                seq: lane.seq,
                lane: lane.lane,
                events: vec![Self::note(tick, pitch, length)],
            },
        }
    }
}

// MARK: Eraser

/// The eraser: every note it lands on or crosses, each once.
pub struct Eraser;

impl Eraser {
    pub const LABEL: &'static str = "Erase Notes";

    /// `erased` plus the note `hit` landed on, if new.
    pub fn adding_hit(hit: &RollHit, erased: &[u64]) -> Vec<u64> {
        match hit {
            RollHit::Note(n) if !erased.contains(&n.row.id) => {
                erased.iter().copied().chain([n.row.id]).collect()
            }
            _ => erased.to_vec(),
        }
    }

    /// `erased` plus the notes of `ids` it lacks, in order.
    pub fn adding(ids: &[u64], erased: &[u64]) -> Vec<u64> {
        let mut out = erased.to_vec();
        for id in ids {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        out
    }

    /// The notes the segment `from`–`to` crosses, in the order met.
    pub fn crossed(
        from: Point,
        to: Point,
        lanes: &[LaneNotes],
        time: &TimeAxis,
        pitch: &PitchAxis,
    ) -> Vec<u64> {
        RollGeometry::crossed(from, to, lanes, time, pitch)
    }

    pub fn command(erased: &[u64]) -> Option<EditCommand> {
        (!erased.is_empty()).then(|| EditCommand::Event {
            edit: EventEdit::Delete {
                events: erased.to_vec(),
            },
        })
    }
}

// MARK: The canvas glue

/// How the note area is drawn now: the axes (auto-scroll and the
/// vertical scroll move them during a drag), its size and the meter's
/// bars. Points are content coordinates: y = 0 is the top of the note
/// area, below the ruler.
#[derive(Debug, Clone, PartialEq)]
pub struct RollFrame {
    pub axis: TimeAxis,
    pub pitch: PitchAxis,
    pub width: f32,
    pub height: f32,
    pub bars: MeterBars,
}

impl RollFrame {
    pub fn new(
        axis: TimeAxis,
        pitch: PitchAxis,
        width: f32,
        height: f32,
        meter: &[MeterEntryRow],
    ) -> Self {
        RollFrame {
            axis,
            pitch,
            width,
            height,
            bars: MeterBars::new(meter),
        }
    }

    /// The note area's bounds.
    pub fn bounds(&self) -> Rect {
        Rect {
            x: 0.,
            y: 0.,
            width: self.width.max(0.),
            height: self.height.max(0.),
        }
    }
}

/// A roll drag in progress (Swift `PianoRollView.Drag`).
#[derive(Debug, Clone, PartialEq)]
pub enum RollDragState {
    /// Moving, copying, transposing or resizing notes; the tick and y
    /// under the press.
    Notes {
        drag: Box<NoteDrag>,
        start_tick: i64,
        start_y: f32,
    },
    /// Painting a new note's length.
    Pencil {
        lane: LaneRef,
        start: i64,
        pitch: u8,
    },
    /// The notes erased so far and the last pointer position.
    Erase { erased: Vec<u64>, last: Point },
    /// A rubber band from the press to the pointer; ⇧ adds.
    Band {
        from: Point,
        to: Point,
        adding: bool,
    },
}

impl RollDragState {
    /// The rubber band to draw, if this is one.
    pub fn band(&self) -> Option<Rect> {
        match self {
            RollDragState::Band { from, to, .. } => Some(band_rect(*from, *to)),
            _ => None,
        }
    }
}

/// A sound the canvas asks the audition for, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditionRequest {
    /// Sound `pitch` at `velocity` through `dest` (none: silence only).
    Sound {
        pitch: u8,
        velocity: u16,
        dest: Option<u64>,
    },
    /// Stop the sounding note.
    Silence,
}

/// What the canvas must do after a pointer event.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RollOutcome {
    /// Sounds to start or stop, in order.
    pub audition: Vec<AuditionRequest>,
    /// The rubber band to draw now (none: none).
    pub band: Option<Rect>,
}

/// The roll canvas's pointer state: the tool machine and the drag in
/// progress.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RollPointer {
    pub machine: ToolMachine<RollHit>,
    pub drag: Option<RollDragState>,
}

impl DocumentModel {
    /// The meter of the roll's sequence (empty: 4/4, or nothing open).
    fn roll_meter(&self) -> Vec<MeterEntryRow> {
        self.layout()
            .detail
            .as_ref()
            .and_then(|d| self.sequence_info(d.target.seq()))
            .map(|i| i.meter.clone())
            .unwrap_or_default()
    }

    /// The frame for axes `axis` and `pitch` over a note area `width` ×
    /// `height`, with the roll's meter.
    pub fn roll_frame(
        &self,
        axis: TimeAxis,
        pitch: PitchAxis,
        width: f32,
        height: f32,
    ) -> RollFrame {
        RollFrame::new(axis, pitch, width, height, &self.roll_meter())
    }

    /// What `p` lands on: the edit track's notes first.
    pub fn roll_hit(&self, frame: &RollFrame, p: Point) -> RollHit {
        RollGeometry::hit(
            p,
            &self.roll_lane_notes(),
            self.roll_edit_lane(),
            &frame.axis,
            &frame.pitch,
        )
    }

    /// The pointer's shape over `p` (an arrow above the note area).
    pub fn roll_cursor(&self, frame: &RollFrame, p: Point) -> CursorKind {
        if p.y < 0. {
            return CursorKind::Arrow;
        }
        CursorKind::of(&self.roll_hit(frame, p), self.tool())
    }

    /// A press at `p`: a drag whose release was lost is dropped, then the
    /// pressed note sounds (any tool but the eraser; its track becomes
    /// the chosen one), or the pencil's new pitch through the edit track.
    pub fn roll_mouse_down(
        &mut self,
        pointer: &mut RollPointer,
        frame: &RollFrame,
        p: Point,
        clicks: u32,
        modifiers: Modifiers,
    ) -> RollOutcome {
        let hit = self.roll_hit(frame, p);
        let events = pointer.machine.down(hit.clone(), p, clicks, modifiers);
        let mut out = self.roll_drag_events(frame, &mut pointer.drag, &events);
        match (self.tool(), &hit) {
            (Tool::Eraser, _) => {}
            (_, RollHit::Note(n)) => {
                self.editor.roll.chosen_lane = Some(n.lane);
                out.audition.push(AuditionRequest::Sound {
                    pitch: n.row.pitch,
                    velocity: n.row.velocity,
                    dest: self.roll_dest(&n.row, n.lane),
                });
            }
            (Tool::Pencil, RollHit::Empty { pitch, .. }) => {
                if let Some(lane) = self.roll_edit_lane() {
                    out.audition.push(AuditionRequest::Sound {
                        pitch: *pitch,
                        velocity: Pencil::VELOCITY,
                        dest: self.roll_row(lane).and_then(|r| r.midi_dest),
                    });
                }
            }
            (_, RollHit::Empty { .. }) => {}
        }
        out
    }

    pub fn roll_mouse_drag(
        &mut self,
        pointer: &mut RollPointer,
        frame: &RollFrame,
        p: Point,
        modifiers: Modifiers,
    ) -> RollOutcome {
        let events = pointer.machine.drag(p, modifiers);
        self.roll_drag_events(frame, &mut pointer.drag, &events)
    }

    /// The release: a click or the drag's end, then silence.
    pub fn roll_mouse_up(
        &mut self,
        pointer: &mut RollPointer,
        frame: &RollFrame,
        p: Point,
        modifiers: Modifiers,
    ) -> RollOutcome {
        let events = pointer.machine.up(p, modifiers);
        let mut out = self.roll_drag_events(frame, &mut pointer.drag, &events);
        out.audition.push(AuditionRequest::Silence);
        out
    }

    /// Escape on the canvas (Swift `PianoRollView.contentCancel`): the
    /// drag is dropped without an undo step, the sound stops, and a
    /// gesture still open is cancelled too, which also aborts a final
    /// edit still pending on Busy after the mouse-up.
    pub fn cancel_roll_drag(
        &mut self,
        pointer: &mut RollPointer,
        frame: &RollFrame,
    ) -> RollOutcome {
        let mut out = self.roll_window_lost(pointer, frame);
        if self.gesture_open() {
            self.cancel_gesture();
        }
        out.band = None;
        out
    }

    /// The canvas left its window mid-press (the release will not come):
    /// the drag is dropped without an undo step and the sound stops. A
    /// final edit already pending on Busy goes on.
    pub fn roll_window_lost(
        &mut self,
        pointer: &mut RollPointer,
        frame: &RollFrame,
    ) -> RollOutcome {
        let events = pointer.machine.cancel();
        let mut out = self.roll_drag_events(frame, &mut pointer.drag, &events);
        self.abandon_roll_drag(&mut pointer.drag);
        out.band = None;
        out.audition.push(AuditionRequest::Silence);
        out
    }

    /// Runs the tool machine's `events` on the roll; `drag` is the
    /// canvas's drag in progress.
    pub fn roll_drag_events(
        &mut self,
        frame: &RollFrame,
        drag: &mut Option<RollDragState>,
        events: &[MachineEvent<RollHit>],
    ) -> RollOutcome {
        let mut out = RollOutcome::default();
        for e in events {
            match e {
                MachineEvent::Click { hit, modifiers, .. } => {
                    self.roll_click(hit, *modifiers, &mut out);
                }
                MachineEvent::Begin {
                    hit,
                    from,
                    modifiers,
                } => {
                    *drag = self.roll_begin(frame, hit, *from, *modifiers, &mut out);
                }
                MachineEvent::Move { to, .. } => self.roll_move(frame, drag, *to, &mut out),
                MachineEvent::End { to, .. } => {
                    if let Some(d) = drag.take() {
                        self.roll_end(frame, d, *to);
                    }
                }
                MachineEvent::Cancel => self.abandon_roll_drag(drag),
            }
        }
        out.band = drag.as_ref().and_then(RollDragState::band);
        out
    }

    /// Drops the drag in progress; its gesture (a rubber band has none)
    /// goes without an undo step.
    fn abandon_roll_drag(&mut self, drag: &mut Option<RollDragState>) {
        match drag.take() {
            None | Some(RollDragState::Band { .. }) => {}
            Some(_) => {
                self.cancel_gesture();
            }
        }
    }

    fn roll_click(&mut self, hit: &RollHit, modifiers: Modifiers, out: &mut RollOutcome) {
        match (self.tool(), hit) {
            (Tool::Eraser, RollHit::Note(n)) => {
                self.edit(EditCommand::Event {
                    edit: EventEdit::Delete {
                        events: vec![n.row.id],
                    },
                });
            }
            (_, RollHit::Note(n)) => {
                if modifiers.shift {
                    self.roll_toggle(n.row.id, n.lane);
                } else {
                    self.roll_select(&[(n.lane, n.row.id)], false);
                }
            }
            (Tool::Pencil, RollHit::Empty { tick, pitch }) => {
                // A gesture, so the undo step reads "Insert Note".
                let Some(lane) = self.roll_edit_lane() else {
                    return;
                };
                if !self.open_roll_gesture(Pencil::LABEL, vec![(lane.seq, lane.lane)], out) {
                    return;
                }
                let snap = self.layout().snap;
                let meter = self.roll_meter();
                let start = Pencil::start(*tick, &snap, &meter);
                let length = Pencil::step(start, &snap, &meter);
                self.end_gesture(Some(Pencil::command(lane, start, *pitch, length)));
            }
            (_, RollHit::Empty { .. }) => {
                if !modifiers.shift {
                    self.clear_selection();
                }
            }
        }
    }

    fn roll_begin(
        &mut self,
        frame: &RollFrame,
        hit: &RollHit,
        from: Point,
        modifiers: Modifiers,
        out: &mut RollOutcome,
    ) -> Option<RollDragState> {
        match (self.tool(), hit) {
            (Tool::Eraser, _) => {
                let erased = Eraser::adding_hit(hit, &[]);
                let lanes = self
                    .roll_shown_lanes()
                    .iter()
                    .map(|l| (l.seq, l.lane))
                    .collect();
                if !self.open_roll_gesture(Eraser::LABEL, lanes, out) {
                    return None;
                }
                self.update_gesture(Eraser::command(&erased));
                Some(RollDragState::Erase { erased, last: from })
            }
            (_, RollHit::Note(n)) => {
                if !self.is_event_selected(n.row.id) {
                    self.roll_select(&[(n.lane, n.row.id)], false);
                }
                // ⌘ on the end zone is Scale Time: not in this slice, so
                // the drag does nothing (no gesture, no edit).
                if modifiers.command && n.zone == NoteZone::End {
                    return None;
                }
                let lanes = self.roll_lane_notes();
                let selected: HashSet<u64> = self.selection().events.iter().copied().collect();
                let ids = NoteDrag::events(n.row.id, &selected, &lanes);
                let wanted: HashSet<u64> = ids.iter().copied().collect();
                let mut touched = vec![];
                let (mut lowest, mut highest) = (n.row.pitch, n.row.pitch);
                for l in lanes.iter() {
                    let mut any = false;
                    for note in l.notes().iter().filter(|x| wanted.contains(&x.id)) {
                        any = true;
                        lowest = lowest.min(note.pitch);
                        highest = highest.max(note.pitch);
                    }
                    if any {
                        touched.push((l.lane.seq, l.lane.lane));
                    }
                }
                let d = NoteDrag::new(
                    n.zone,
                    n.row.clone(),
                    n.lane,
                    ids,
                    lowest,
                    highest,
                    modifiers.option,
                );
                if !self.open_roll_gesture(d.label(), touched, out) {
                    return None;
                }
                Some(RollDragState::Notes {
                    drag: Box::new(d),
                    start_tick: frame.axis.tick_at(from.x),
                    start_y: from.y,
                })
            }
            (Tool::Pencil, RollHit::Empty { tick, pitch }) => {
                let lane = self.roll_edit_lane()?;
                if !self.open_roll_gesture(Pencil::LABEL, vec![(lane.seq, lane.lane)], out) {
                    return None;
                }
                let start = Pencil::start(*tick, &self.layout().snap, &self.roll_meter());
                Some(RollDragState::Pencil {
                    lane,
                    start,
                    pitch: *pitch,
                })
            }
            (_, RollHit::Empty { .. }) => Some(RollDragState::Band {
                from,
                to: from,
                adding: modifiers.shift,
            }),
        }
    }

    /// Opens the gesture a drag or a pencil click needs. Refused while
    /// another one is still finishing: the status says so and the sound
    /// stops (as for any refusal).
    fn open_roll_gesture(
        &mut self,
        label: &str,
        lanes: Vec<(u64, u64)>,
        out: &mut RollOutcome,
    ) -> bool {
        let pending = self.gesture_open();
        if self.begin_gesture(label, lanes) {
            return true;
        }
        if pending {
            self.status = Some(GESTURE_OPEN.into());
        }
        out.audition.push(AuditionRequest::Silence);
        false
    }

    fn roll_move(
        &mut self,
        frame: &RollFrame,
        drag: &mut Option<RollDragState>,
        to: Point,
        out: &mut RollOutcome,
    ) {
        let Some(d) = drag.as_mut() else {
            return;
        };
        match d {
            RollDragState::Notes {
                drag: nd, start_y, ..
            } => {
                let sound = AuditionRequest::Sound {
                    pitch: nd.sounding_pitch(to.y - *start_y, frame.pitch.row_height()),
                    velocity: nd.pressed.velocity,
                    dest: self.roll_dest(&nd.pressed, nd.lane),
                };
                let target = self.roll_target(frame, d, to);
                self.update_gesture(target);
                out.audition.push(sound);
            }
            RollDragState::Pencil { .. } => {
                let target = self.roll_target(frame, d, to);
                self.update_gesture(target);
            }
            RollDragState::Erase { erased, last } => {
                let crossed = Eraser::crossed(
                    *last,
                    to,
                    &self.roll_lane_notes(),
                    &frame.axis,
                    &frame.pitch,
                );
                let next = Eraser::adding(&crossed, erased);
                *last = to;
                if next.len() != erased.len() {
                    *erased = next;
                    let cmd = Eraser::command(erased);
                    self.update_gesture(cmd);
                }
            }
            RollDragState::Band { to: band_to, .. } => *band_to = to,
        }
    }

    fn roll_end(&mut self, frame: &RollFrame, d: RollDragState, to: Point) {
        if let RollDragState::Band { from, adding, .. } = d {
            let rect = band_rect(from, to);
            let found =
                RollGeometry::notes_in(&rect, &self.roll_lane_notes(), &frame.axis, &frame.pitch);
            self.roll_select(&found, adding);
            return;
        }
        let target = self.roll_target(frame, &d, to);
        self.end_gesture(target);
    }

    /// The drag's full target with the pointer at `to`.
    fn roll_target(&self, frame: &RollFrame, d: &RollDragState, to: Point) -> Option<EditCommand> {
        let snap = self.layout().snap;
        let meter = self.roll_meter();
        match d {
            RollDragState::Notes {
                drag,
                start_tick,
                start_y,
            } => {
                let raw = frame.axis.tick_at(to.x).saturating_sub(*start_tick);
                // Without horizontal motion a resize changes nothing (a
                // zero-length note would otherwise gain a tick).
                if drag.zone == NoteZone::End && raw == 0 {
                    return None;
                }
                drag.command(raw, to.y - start_y, frame.pitch.row_height(), &snap, &meter)
            }
            RollDragState::Pencil { lane, start, pitch } => {
                let length = Pencil::length(*start, frame.axis.tick_at(to.x), &snap, &meter);
                Some(Pencil::command(*lane, *start, *pitch, length))
            }
            RollDragState::Erase { erased, last } => {
                let crossed = Eraser::crossed(
                    *last,
                    to,
                    &self.roll_lane_notes(),
                    &frame.axis,
                    &frame.pitch,
                );
                Eraser::command(&Eraser::adding(&crossed, erased))
            }
            RollDragState::Band { .. } => None,
        }
    }
}
