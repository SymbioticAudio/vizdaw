//! The piano roll's maths, no GPUI (Swift `PitchAxis`, `PianoRollGeometry`,
//! `RubberBand`, `PianoRollCursor`, `PianoRollA11y`, `KeyboardLabels`):
//! pitch ↔ y, note rectangles, zones and hit tests, the eraser's segment
//! crossing, the rubber band, cursors and labels. Everything clamps
//! instead of trapping. Hit tests binary-search the time-ordered notes,
//! so a long track costs only the notes near the pointer.

use std::ops::RangeInclusive;

use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::events::EventRow;

use super::LaneNotes;
use crate::arrangement::{MeterBars, Point, Rect, TimeAxis, beat_ticks};
use crate::editor::Tool;
use crate::list::PitchName;

// MARK: PitchAxis

/// Pitch ↔ y: pitch 127 on top, one row of `row_height` points per pitch;
/// `offset` is the vertical scroll (points from the top of pitch 127).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchAxis {
    row_height: f32,
    offset: f32,
}

impl Default for PitchAxis {
    fn default() -> Self {
        PitchAxis::new(Self::DEFAULT_ROW_HEIGHT, 0.)
    }
}

impl PitchAxis {
    pub const MIN_ROW_HEIGHT: f32 = 4.;
    pub const MAX_ROW_HEIGHT: f32 = 40.;
    pub const DEFAULT_ROW_HEIGHT: f32 = 10.;
    pub const PITCHES: usize = 128;

    pub fn new(row_height: f32, offset: f32) -> Self {
        PitchAxis {
            row_height: Self::clamp_row(row_height),
            offset: if offset.is_finite() { offset } else { 0. },
        }
    }

    fn clamp_row(h: f32) -> f32 {
        if !h.is_finite() {
            return Self::DEFAULT_ROW_HEIGHT;
        }
        h.clamp(Self::MIN_ROW_HEIGHT, Self::MAX_ROW_HEIGHT)
    }

    pub fn row_height(&self) -> f32 {
        self.row_height
    }

    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// The vertical scroll; a non-finite one is 0.
    pub fn set_offset(&mut self, offset: f32) {
        self.offset = if offset.is_finite() { offset } else { 0. };
    }

    pub fn content_height(&self) -> f32 {
        Self::PITCHES as f32 * self.row_height
    }

    pub fn top(&self, pitch: u8) -> f32 {
        f32::from(127 - pitch.min(127)) * self.row_height - self.offset
    }

    pub fn center(&self, pitch: u8) -> f32 {
        self.top(pitch) + self.row_height / 2.
    }

    /// The unclamped row under `y` (0 is pitch 127's row); a non-finite
    /// `y` is pitch 60's row.
    pub fn row_at(&self, y: f32) -> i64 {
        if !y.is_finite() {
            return 67;
        }
        let r = ((y + self.offset) / self.row_height).floor();
        r.clamp(-1_000_000., 1_000_000.) as i64
    }

    pub fn pitch_at(&self, y: f32) -> u8 {
        (127 - self.row_at(y).clamp(0, 127)) as u8
    }

    /// Whole rows from `from_y` to `to_y`, up positive, unclamped.
    pub fn semitones(&self, from_y: f32, to_y: f32) -> i64 {
        self.row_at(from_y) - self.row_at(to_y)
    }

    /// Another row height, keeping the pitch position under `y`.
    pub fn with_row_height(&self, h: f32, keeping_y: f32) -> PitchAxis {
        let new = Self::clamp_row(h);
        let anchor = (keeping_y + self.offset) / self.row_height;
        PitchAxis::new(new, anchor * new - keeping_y)
    }

    /// `pitch` centred in a view `height` tall, clamped to the content.
    pub fn centering(&self, pitch: u8, height: f32) -> PitchAxis {
        let h = if height.is_finite() { height } else { 0. };
        let mut a = *self;
        a.set_offset(
            f32::from(127 - pitch.min(127)) * self.row_height + self.row_height / 2. - h / 2.,
        );
        a.clamped(h)
    }

    /// The offset kept inside the content for a view `height` tall.
    pub fn clamped(&self, height: f32) -> PitchAxis {
        let h = if height.is_finite() { height } else { 0. };
        let mut a = *self;
        a.offset = self.offset.clamp(0., (self.content_height() - h).max(0.));
        a
    }

    /// The pitches a view `height` tall shows, lowest first.
    pub fn visible(&self, height: f32) -> RangeInclusive<u8> {
        let h = if height.is_finite() {
            height.max(0.)
        } else {
            0.
        };
        self.pitch_at((h - 0.001).max(0.))..=self.pitch_at(0.)
    }
}

// MARK: Keys

/// A black key's pitch (rows shaded in the roll).
pub fn is_black(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

/// The keyboard strip's label: only C keys are labelled.
pub fn key_label(pitch: u8) -> Option<String> {
    (pitch % 12 == 0).then(|| PitchName::name(pitch))
}

// MARK: Notes

/// Where over a note the pointer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteZone {
    Start,
    Middle,
    End,
}

/// A note under the pointer.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteHit {
    pub lane: LaneRef,
    pub row: EventRow,
    pub zone: NoteZone,
}

/// What a point of the note area lands on.
#[derive(Debug, Clone, PartialEq)]
pub enum RollHit {
    /// Boxed: a hit is made per press or hover, and an event row is large.
    Note(Box<NoteHit>),
    Empty {
        tick: i64,
        pitch: u8,
    },
}

/// Note rectangles and hit tests as pure functions of rows and axes.
pub struct RollGeometry;

impl RollGeometry {
    pub const MIN_NOTE_WIDTH: f32 = 3.;
    /// The widest a start or end zone gets.
    pub const MAX_EDGE: f32 = 8.;

    pub fn rect(n: &EventRow, time: &TimeAxis, pitch: &PitchAxis) -> Rect {
        Rect {
            x: time.x(n.tick),
            y: pitch.top(n.pitch),
            width: time.width_of_ticks(n.duration).max(Self::MIN_NOTE_WIDTH),
            height: pitch.row_height(),
        }
    }

    /// Edge zones are `min(MAX_EDGE, width / 3)` wide.
    pub fn zone(x: f32, r: &Rect) -> NoteZone {
        let edge = Self::MAX_EDGE.min(r.width / 3.);
        if x < r.x + edge {
            NoteZone::Start
        } else if x >= r.max_x() - edge {
            NoteZone::End
        } else {
            NoteZone::Middle
        }
    }

    /// The notes of `lane` whose rectangles may reach into x `from..=to`
    /// (a contiguous run, in time order): found by binary search, so a
    /// long track costs only the notes near there.
    pub fn candidates<'a>(
        lane: &'a LaneNotes,
        time: &TimeAxis,
        from: f32,
        to: f32,
    ) -> &'a [EventRow] {
        let notes = &lane.notes()[..];
        if from.is_nan() || to.is_nan() {
            return &notes[..0];
        }
        let hi = notes.partition_point(|n| time.x(n.tick) <= to);
        // A note reaches at most its longest duration, or the minimum
        // width, right of its start.
        let min_ticks = (f64::from(Self::MIN_NOTE_WIDTH) * time.ticks_per_point()).ceil() as i64;
        let reach = lane.longest().max(min_ticks).saturating_add(1);
        let earliest = time.tick_at(from).saturating_sub(reach);
        let lo = notes[..hi].partition_point(|n| n.tick < earliest);
        &notes[lo..hi]
    }

    /// The note under `p`: the active lane's first, then the others in
    /// order; within a lane the latest-starting one whose pitch is the
    /// row's (notes are in time order).
    pub fn hit(
        p: Point,
        lanes: &[LaneNotes],
        active: Option<LaneRef>,
        time: &TimeAxis,
        pitch: &PitchAxis,
    ) -> RollHit {
        let target = pitch.pitch_at(p.y);
        let ordered = lanes
            .iter()
            .filter(|l| Some(l.lane) == active)
            .chain(lanes.iter().filter(|l| Some(l.lane) != active));
        for l in ordered {
            let under = Self::candidates(l, time, p.x, p.x)
                .iter()
                .rev()
                .find(|n| n.pitch == target && Self::rect(n, time, pitch).contains(p));
            if let Some(n) = under {
                let r = Self::rect(n, time, pitch);
                return RollHit::Note(Box::new(NoteHit {
                    lane: l.lane,
                    row: n.clone(),
                    zone: Self::zone(p.x, &r),
                }));
            }
        }
        RollHit::Empty {
            tick: time.tick_at(p.x),
            pitch: target,
        }
    }

    /// The shown tracks bottom to top: the others in reverse order, then
    /// the active one. [`hit`](Self::hit) tries the active track, then
    /// the others in order, so the note hit is the one drawn on top.
    pub fn paint_order(lanes: &[LaneNotes], active: Option<LaneRef>) -> Vec<&LaneNotes> {
        lanes
            .iter()
            .rev()
            .filter(|l| Some(l.lane) != active)
            .chain(lanes.iter().filter(|l| Some(l.lane) == active))
            .collect()
    }

    /// The notes the pointer crossed moving from `a` to `b`, in the order
    /// it met them (a fast eraser drag skips none).
    pub fn crossed(
        a: Point,
        b: Point,
        lanes: &[LaneNotes],
        time: &TimeAxis,
        pitch: &PitchAxis,
    ) -> Vec<u64> {
        let mut hits: Vec<(f32, usize, u64)> = vec![];
        for l in lanes {
            for n in Self::candidates(l, time, a.x.min(b.x), a.x.max(b.x)) {
                if let Some(t) = Self::entry(a, b, &Self::rect(n, time, pitch)) {
                    hits.push((t, hits.len(), n.id));
                }
            }
        }
        hits.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        hits.into_iter().map(|h| h.2).collect()
    }

    /// Where along `a`–`b` (0…1) the segment enters `r`; `None` when it
    /// misses (Liang–Barsky clipping).
    fn entry(a: Point, b: Point, r: &Rect) -> Option<f32> {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let mut t0: f32 = 0.;
        let mut t1: f32 = 1.;
        for (p, q) in [
            (-dx, a.x - r.x),
            (dx, r.max_x() - a.x),
            (-dy, a.y - r.y),
            (dy, r.max_y() - a.y),
        ] {
            if p == 0. {
                if q < 0. {
                    return None;
                }
                continue;
            }
            let t = q / p;
            if p < 0. {
                if t > t1 {
                    return None;
                }
                t0 = t0.max(t);
            } else {
                if t < t0 {
                    return None;
                }
                t1 = t1.min(t);
            }
        }
        Some(t0)
    }

    /// The notes a rubber band `r` touches, lane by lane.
    pub fn notes_in(
        r: &Rect,
        lanes: &[LaneNotes],
        time: &TimeAxis,
        pitch: &PitchAxis,
    ) -> Vec<(LaneRef, u64)> {
        lanes
            .iter()
            .flat_map(|l| {
                Self::candidates(l, time, r.x, r.max_x())
                    .iter()
                    .filter(|n| Self::rect(n, time, pitch).intersects(r))
                    .map(|n| (l.lane, n.id))
            })
            .collect()
    }
}

/// The rubber band between the press and the pointer.
pub fn band_rect(a: Point, b: Point) -> Rect {
    Rect {
        x: a.x.min(b.x),
        y: a.y.min(b.y),
        width: (b.x - a.x).abs(),
        height: (b.y - a.y).abs(),
    }
}

// MARK: Cursors

/// The pointer's shape over the note area; the canvas maps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorKind {
    Arrow,
    OpenHand,
    UpDown,
    ResizeRight,
    Crosshair,
    Eraser,
}

impl CursorKind {
    /// The pointer over `hit` with `tool`.
    pub fn of(hit: &RollHit, tool: Tool) -> CursorKind {
        if tool == Tool::Eraser {
            return CursorKind::Eraser;
        }
        match hit {
            RollHit::Note(n) => match n.zone {
                NoteZone::Start => CursorKind::OpenHand,
                NoteZone::Middle => CursorKind::UpDown,
                NoteZone::End => CursorKind::ResizeRight,
            },
            RollHit::Empty { .. } if tool == Tool::Pencil => CursorKind::Crosshair,
            RollHit::Empty { .. } => CursorKind::Arrow,
        }
    }
}

// MARK: Accessibility

/// A note's label: "Note C4, bar 1 beat 1, velocity 100", with " tick N"
/// off the beat, the track's name when more than one shows
/// (`lane_name`), and ", muted".
pub fn a11y_label(n: &EventRow, lane_name: Option<&str>, bars: &MeterBars) -> String {
    let bar = bars.bar_at(n.tick);
    let beat = beat_ticks(&bar.meter);
    let into = n.tick - bar.start;
    let mut s = format!(
        "Note {}, bar {} beat {}",
        PitchName::name(n.pitch),
        bar.number,
        into / beat + 1
    );
    if into % beat != 0 {
        s += &format!(" tick {}", into % beat);
    }
    s += &format!(", velocity {}", n.velocity >> 9);
    if let Some(name) = lane_name {
        s += &format!(", {name}");
    }
    if n.mute {
        s += ", muted";
    }
    s
}
