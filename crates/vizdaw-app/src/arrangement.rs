//! The arrangement's maths, no GPUI (Swift: `TimeAxis`, `Scrolling`,
//! `Grid`, `MeterMath`, `ArrangementGeometry`, `ArrangementA11y`): tick and
//! point conversions, rows and block rectangles, the grid, scrolling and
//! the blocks' accessibility labels. Every conversion clamps instead of
//! trapping.

use std::ops::RangeInclusive;

use vizdaw_ffi::blocks::{BlockKind, BlockRow, BlockRule};
use vizdaw_ffi::events::{EventKindRow, EventRow, MeterEntryRow};
use vizdaw_ffi::selection::TickRange;

mod block_drag;
mod tool_machine;

pub use block_drag::{ArrangementFrame, ArrangementHit, BlockDrag, BlockDragState, hit_test};
pub use tool_machine::{MachineEvent, Modifiers, Phase, ToolMachine};

/// Ticks per quarter note.
pub const PPQ: i64 = 15_360;

/// How the arrangement groups a track's events into blocks: a new block
/// after four beats of silence (Swift `EditorLayout.blockRule` default
/// `silenceGap(beats: 4)`).
pub const BLOCK_RULE: BlockRule = BlockRule::SilenceGap { ticks: 4 * PPQ };

/// The ruler's height above the rows.
pub const RULER_HEIGHT: f32 = 22.;

/// How far a press must move before it drags (Swift `ToolMachine.deadZone`).
pub const DEAD_ZONE: f32 = 3.;

/// What one zoom button press multiplies or divides the zoom by.
pub const ZOOM_STEP: f64 = 1.5;

// MARK: Bars

/// One bar (Swift `MeterMath.Bar`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub number: i64,
    pub start: i64,
    pub length: i64,
    pub meter: MeterEntryRow,
}

const FOUR_FOUR: MeterEntryRow = MeterEntryRow {
    tick: 0,
    num: 4,
    den: 4,
    sharps: 0,
    minor: false,
};

pub fn beat_ticks(m: &MeterEntryRow) -> i64 {
    PPQ * 4 / i64::from(m.den.max(1))
}

pub fn bar_length(m: &MeterEntryRow) -> i64 {
    beat_ticks(m) * i64::from(m.num.max(1))
}

/// A meter map with the number of the bar each entry starts (an empty
/// map is 4/4), built once and asked for any number of bars.
#[derive(Debug, Clone, PartialEq)]
pub struct MeterBars(Vec<(MeterEntryRow, i64)>);

impl MeterBars {
    pub fn new(meter: &[MeterEntryRow]) -> Self {
        let entries: &[MeterEntryRow] = if meter.is_empty() {
            std::slice::from_ref(&FOUR_FOUR)
        } else {
            meter
        };
        let mut out: Vec<(MeterEntryRow, i64)> = Vec::with_capacity(entries.len());
        for e in entries {
            let bar = match out.last() {
                Some((prev, n)) => n + (e.tick - prev.tick) / bar_length(prev),
                None => 1,
            };
            out.push((*e, bar));
        }
        MeterBars(out)
    }

    /// The first entry (4/4 for an empty map).
    pub fn first(&self) -> MeterEntryRow {
        self.0[0].0
    }

    /// The bar containing `tick` (bar 1 starts at tick 0; pickup bars
    /// before it count down from 0).
    pub fn bar_at(&self, tick: i64) -> Bar {
        let (e, first) = self
            .0
            .iter()
            .rev()
            .find(|(e, _)| e.tick <= tick)
            .copied()
            .unwrap_or(self.0[0]);
        let length = bar_length(&e);
        let k = (tick - e.tick).div_euclid(length);
        Bar {
            number: first + k,
            start: e.tick + k * length,
            length,
            meter: e,
        }
    }
}

/// The bar containing `tick` (builds the table each call: for one-offs).
pub fn bar_at(tick: i64, meter: &[MeterEntryRow]) -> Bar {
    MeterBars::new(meter).bar_at(tick)
}

// MARK: TimeAxis

/// Tick ↔ x (Swift `TimeAxis`): `ticks_per_point` is the zoom, `origin`
/// the tick at x = 0 of the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeAxis {
    ticks_per_point: f64,
    origin: i64,
}

impl Default for TimeAxis {
    fn default() -> Self {
        TimeAxis::new(Self::DEFAULT_TICKS_PER_POINT, 0)
    }
}

impl TimeAxis {
    pub const DEFAULT_TICKS_PER_POINT: f64 = 384.;
    pub const MIN_TICKS_PER_POINT: f64 = 2.;
    /// One 4/4 bar per point.
    pub const MAX_TICKS_PER_POINT: f64 = 61_440.;
    /// The ticks the axis reaches (2^50, about two years at 120 BPM).
    pub const LIMIT: i64 = 1 << 50;

    pub fn new(ticks_per_point: f64, origin: i64) -> Self {
        TimeAxis {
            ticks_per_point: Self::clamp_zoom(ticks_per_point),
            origin: origin.clamp(-Self::LIMIT, Self::LIMIT),
        }
    }

    pub fn ticks_per_point(&self) -> f64 {
        self.ticks_per_point
    }

    pub fn origin(&self) -> i64 {
        self.origin
    }

    fn clamp_zoom(v: f64) -> f64 {
        if v.is_nan() {
            return Self::MIN_TICKS_PER_POINT;
        }
        v.clamp(Self::MIN_TICKS_PER_POINT, Self::MAX_TICKS_PER_POINT)
    }

    fn clamp_origin(v: f64) -> i64 {
        if v.is_nan() {
            return 0;
        }
        let limit = Self::LIMIT as f64;
        v.clamp(-limit, limit).round() as i64
    }

    /// A tick from a double: NaN is 0; values beyond the limit clamp.
    pub fn clamp_tick(d: f64) -> i64 {
        if d.is_nan() {
            return 0;
        }
        let limit = Self::LIMIT as f64;
        if d >= limit {
            return Self::LIMIT;
        }
        if d <= -limit {
            return -Self::LIMIT;
        }
        d.floor() as i64
    }

    pub fn x(&self, tick: i64) -> f32 {
        let t = tick.clamp(-Self::LIMIT, Self::LIMIT);
        ((t - self.origin) as f64 / self.ticks_per_point) as f32
    }

    /// The tick at `x` (floored).
    pub fn tick_at(&self, x: f32) -> i64 {
        Self::clamp_tick(self.origin as f64 + f64::from(x) * self.ticks_per_point)
    }

    pub fn width_of_ticks(&self, ticks: i64) -> f32 {
        (ticks.clamp(-Self::LIMIT, Self::LIMIT) as f64 / self.ticks_per_point) as f32
    }

    pub fn visible_range(&self, width: f32) -> RangeInclusive<i64> {
        let w = if width.is_nan() { 0. } else { width.max(0.) };
        self.tick_at(0.)..=self.tick_at(w)
    }

    /// Zoomed in by `factor` (> 1 zooms in) keeping the tick under `x`;
    /// never left of tick 0.
    pub fn zoom_about(&self, factor: f64, x: f32) -> TimeAxis {
        if !factor.is_finite() || factor <= 0. {
            return *self;
        }
        let x = f64::from(x);
        let anchor = self.origin as f64 + x * self.ticks_per_point;
        let tpp = Self::clamp_zoom(self.ticks_per_point / factor);
        TimeAxis {
            ticks_per_point: tpp,
            origin: Self::clamp_origin((anchor - x * tpp).max(0.)),
        }
    }

    /// The zoom set to `ticks_per_point` (clamped), the origin kept (the
    /// zoom buttons).
    pub fn with_zoom(&self, ticks_per_point: f64) -> TimeAxis {
        TimeAxis::new(ticks_per_point, self.origin)
    }

    /// Scrolled by the user: never left of tick 0, or of where a count-in
    /// left it. Following the playhead ([`scrolled_to`](Self::scrolled_to))
    /// may go there.
    pub fn scrolled_by(&self, points: f32) -> TimeAxis {
        let to = self.origin as f64 + f64::from(points) * self.ticks_per_point;
        if to.is_nan() {
            return TimeAxis::new(self.ticks_per_point, 0);
        }
        let origin = if points >= 0. {
            to
        } else {
            to.max(self.origin.min(0) as f64)
        };
        TimeAxis {
            ticks_per_point: self.ticks_per_point,
            origin: Self::clamp_origin(origin),
        }
    }

    pub fn scrolled_to(&self, tick: f64) -> TimeAxis {
        TimeAxis {
            ticks_per_point: self.ticks_per_point,
            origin: Self::clamp_origin(tick),
        }
    }
}

// MARK: Geometry

/// A point in canvas points (y = 0 is the top of the first row).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// A rectangle in canvas points (y = 0 is the top of the first row).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn max_x(&self) -> f32 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f32 {
        self.y + self.height
    }

    /// `p` is inside: the left and top edges count, the right and bottom
    /// do not (as `CGRect.contains`).
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.max_x() && p.y >= self.y && p.y < self.max_y()
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.max_x() && o.x < self.max_x() && self.y < o.max_y() && o.y < self.max_y()
    }

    /// The part inside `o`, if any.
    pub fn intersection(&self, o: &Rect) -> Option<Rect> {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let w = self.max_x().min(o.max_x()) - x;
        let h = self.max_y().min(o.max_y()) - y;
        (w > 0. && h > 0.).then_some(Rect {
            x,
            y,
            width: w,
            height: h,
        })
    }
}

/// Rows and block rectangles (Swift `ArrangementGeometry`), in body
/// coordinates: y = 0 is the top of the first row before scrolling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub lane_height: f32,
    pub vertical_offset: f32,
}

impl Default for Geometry {
    fn default() -> Self {
        Geometry {
            lane_height: 48.,
            vertical_offset: 0.,
        }
    }
}

impl Geometry {
    pub const BLOCK_INSET: f32 = 2.;
    pub const MIN_BLOCK_WIDTH: f32 = 4.;

    pub fn row_top(&self, i: usize) -> f32 {
        i as f32 * self.lane_height - self.vertical_offset
    }

    pub fn row_at(&self, y: f32, count: usize) -> Option<usize> {
        if self.lane_height <= 0. {
            return None;
        }
        let i = ((y + self.vertical_offset) / self.lane_height).floor();
        (i >= 0. && (i as usize) < count).then_some(i as usize)
    }

    pub fn block_rect(&self, b: &BlockRow, row: usize, axis: &TimeAxis) -> Rect {
        let x0 = axis.x(b.start);
        let x1 = axis.x(b.end);
        Rect {
            x: x0,
            y: self.row_top(row) + Self::BLOCK_INSET,
            width: (x1 - x0).max(Self::MIN_BLOCK_WIDTH),
            height: self.lane_height - 2. * Self::BLOCK_INSET,
        }
    }

    /// The tracks plus one spare row (the headers' "Add Track" row).
    pub fn content_height(&self, rows: usize) -> f32 {
        (rows + 1) as f32 * self.lane_height
    }

    /// `y` as a vertical offset that keeps the content in view, for a
    /// body `visible` high.
    pub fn clamp_vertical(&self, y: f32, rows: usize, visible: f32) -> f32 {
        if y.is_nan() {
            return 0.;
        }
        let max = (self.content_height(rows) - visible).max(0.);
        y.clamp(0., max)
    }
}

/// The mini notes of an events block (Swift `ArrangementView.sketch`):
/// one 2-point bar per note between the block's start and end, from the
/// lowest pitch 5 points above the bottom to the highest 5 below the top.
/// `events` are the track's rows in time order.
pub fn sketch(b: &BlockRow, r: &Rect, events: &[EventRow], axis: &TimeAxis) -> Vec<Rect> {
    let from = events.partition_point(|e| e.tick < b.start);
    let notes: Vec<&EventRow> = events[from..]
        .iter()
        .take_while(|e| e.tick <= b.end)
        .filter(|e| e.kind == EventKindRow::Note)
        .collect();
    let (Some(lo), Some(hi)) = (
        notes.iter().map(|n| n.pitch).min(),
        notes.iter().map(|n| n.pitch).max(),
    ) else {
        return vec![];
    };
    let span = f32::from(hi.saturating_sub(lo).max(1));
    notes
        .iter()
        .filter_map(|n| {
            let y = r.max_y() - 5. - f32::from(n.pitch - lo) / span * (r.height - 10.);
            Rect {
                x: axis.x(n.tick),
                y: y - 1.,
                width: axis.width_of_ticks(n.duration).max(1.),
                height: 2.,
            }
            .intersection(r)
        })
        .collect()
}

// MARK: Grid

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridKind {
    /// A bar line with its bar number.
    Bar(i64),
    Beat,
}

/// One line of the grid and the ruler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridLine {
    pub tick: i64,
    pub kind: GridKind,
}

/// The least spacing of drawn lines, in points.
const MIN_SPACING: f64 = 6.;
const MAX_LINES: usize = 4_000;

/// The lines across `width` points of `axis`.
pub fn grid_lines(axis: &TimeAxis, width: f32, meter: &[MeterEntryRow]) -> Vec<GridLine> {
    let r = axis.visible_range(width);
    grid_lines_between(meter, *r.start(), *r.end(), axis.ticks_per_point())
}

/// As [`grid_lines_between`], over a table already built.
pub fn grid_lines_in(bars: &MeterBars, from: i64, to: i64, ticks_per_point: f64) -> Vec<GridLine> {
    let mut out = Vec::new();
    let mut bar = bars.bar_at(from);
    let mut visits = 0;
    while bar.start <= to && out.len() < MAX_LINES && visits < 200_000 {
        visits += 1;
        let bar_points = bar.length as f64 / ticks_per_point;
        let mut every: i64 = 1;
        while (every as f64) * bar_points < MIN_SPACING && every < (1 << 20) {
            every *= 2;
        }
        if (bar.number - 1).div_euclid(every) * every == bar.number - 1 {
            out.push(GridLine {
                tick: bar.start,
                kind: GridKind::Bar(bar.number),
            });
        }
        let beat = beat_ticks(&bar.meter);
        if beat as f64 / ticks_per_point >= MIN_SPACING {
            let mut t = bar.start + beat;
            while t < bar.start + bar.length && out.len() < MAX_LINES {
                if t >= from && t <= to {
                    out.push(GridLine {
                        tick: t,
                        kind: GridKind::Beat,
                    });
                }
                t += beat;
            }
        }
        bar = bars.bar_at(bar.start + bar.length);
    }
    out
}

/// Lines from the bar containing `from` up to `to` (Swift `Grid.lines`).
/// Beat lines only when beats are at least 6 points apart; bar lines
/// thinned to every 2^n-th bar (counted from bar 1) when bars are closer;
/// at most 4000 lines.
pub fn grid_lines_between(
    meter: &[MeterEntryRow],
    from: i64,
    to: i64,
    ticks_per_point: f64,
) -> Vec<GridLine> {
    grid_lines_in(&MeterBars::new(meter), from, to, ticks_per_point)
}

/// Label every n-th bar so labels are at least 28 points apart (n a power
/// of two).
pub fn label_every(bar_ticks: i64, ticks_per_point: f64) -> i64 {
    let mut every: i64 = 1;
    while (every as f64) * (bar_ticks as f64) / ticks_per_point < 28. && every < (1 << 20) {
        every *= 2;
    }
    every
}

// MARK: Scrolling

/// Scrolling while a drag passes an edge.
pub struct AutoScroll;

impl AutoScroll {
    /// Distance from an edge where scrolling starts.
    pub const EDGE: f32 = 24.;
    /// Points per second for each point of overshoot.
    pub const RATE: f32 = 20.;
    pub const MAX_SPEED: f32 = 1_200.;

    /// Horizontal speed in points per second (negative: towards earlier
    /// time) for a pointer at `x` in a view `width` wide.
    pub fn speed(x: f32, width: f32) -> f32 {
        if x < Self::EDGE {
            return -(Self::MAX_SPEED.min((Self::EDGE - x) * Self::RATE));
        }
        if x > width - Self::EDGE {
            return Self::MAX_SPEED.min((x - (width - Self::EDGE)) * Self::RATE);
        }
        0.
    }
}

/// Following playback in page mode.
pub struct PageFollow;

impl PageFollow {
    pub const MARGIN: f32 = 16.;

    /// The new origin tick when the playhead at `tick` is outside the
    /// visible width, else `None`.
    pub fn origin(tick: f64, axis: &TimeAxis, width: f32) -> Option<f64> {
        let x = (tick - axis.origin() as f64) / axis.ticks_per_point();
        (x < 0. || x > f64::from(width))
            .then(|| tick - f64::from(Self::MARGIN) * axis.ticks_per_point())
    }
}

/// A ruler drag's time range.
pub struct RulerSelection;

impl RulerSelection {
    /// The range between the press (`anchor`, a tick, so it survives
    /// auto-scroll) and the pointer at `x`; `None` when empty. Slice 3 has
    /// no Snap setting: the ticks are raw.
    pub fn range(anchor: i64, x: f32, axis: &TimeAxis) -> Option<TickRange> {
        let other = axis.tick_at(x);
        let (a, b) = (anchor.min(other), anchor.max(other));
        (b > a).then_some(TickRange { start: a, end: b })
    }
}

/// Scroll-wheel deltas: precise deltas are points; a notched wheel's
/// lines scale to points and to a ~10% zoom step per notch.
pub struct WheelScroll;

impl WheelScroll {
    /// Points per wheel line.
    pub const LINE_HEIGHT: f32 = 12.;
    /// Zoom factor per point of precise vertical delta.
    pub const ZOOM_PER_POINT: f64 = 1.01;
    /// Zoom factor per wheel line (one notch).
    pub const ZOOM_PER_LINE: f64 = 1.1;

    pub fn points(dx: f32, dy: f32, precise: bool) -> (f32, f32) {
        if precise {
            (dx, dy)
        } else {
            (dx * Self::LINE_HEIGHT, dy * Self::LINE_HEIGHT)
        }
    }

    /// The zoom factor for a vertical delta with the secondary modifier.
    pub fn zoom_factor(dy: f32, precise: bool) -> f64 {
        let base = if precise {
            Self::ZOOM_PER_POINT
        } else {
            Self::ZOOM_PER_LINE
        };
        base.powf(f64::from(dy))
    }
}

// MARK: Accessibility

/// A block's label (Swift `ArrangementA11y.label`).
pub fn a11y_label(b: &BlockRow, lane_name: &str, meter: &[MeterEntryRow]) -> String {
    a11y_label_in(b, lane_name, &MeterBars::new(meter))
}

/// As [`a11y_label`], over a table already built (one per frame).
pub fn a11y_label_in(b: &BlockRow, lane_name: &str, bars: &MeterBars) -> String {
    let first = bars.bar_at(b.start).number;
    let last = bars.bar_at((b.end - 1).max(b.start)).number;
    let bars = if first == last {
        format!("bar {first}")
    } else {
        format!("bars {first} to {last}")
    };
    let muted = if b.muted { ", muted" } else { "" };
    match b.kind {
        BlockKind::Reference => format!(
            "Reference to {} on {lane_name}, {bars}{muted}",
            b.label.as_deref().unwrap_or("a sequence")
        ),
        BlockKind::Events => {
            let n = b.events.len();
            let s = if n == 1 { "" } else { "s" };
            format!("Block on {lane_name}, {bars}, {n} event{s}{muted}")
        }
    }
}

/// What a region is called (Swift `RegionA11y.name`): its own name, else
/// its file's (`media_name`), else "audio".
pub fn region_name(r: &EventRow, media_name: Option<&str>) -> String {
    match r.region_name.as_deref() {
        Some(n) if !n.is_empty() => n.to_owned(),
        _ => media_name.unwrap_or("audio").to_owned(),
    }
}

/// A region gain's label (Swift `RegionHandles.gainLabel`): "+3.0 dB",
/// "0.0 dB", "-6.5 dB".
pub fn region_gain_label(db: f32) -> String {
    if db.abs() < 0.05 {
        "0.0 dB".into()
    } else {
        format!("{db:+.1} dB")
    }
}

/// An audio region's label (Swift `RegionA11y.label`): "Region <name> on
/// <track>, bar(s) …, gain …[, crossfade][, fades][, offline][, muted]".
pub fn region_a11y_label(
    r: &EventRow,
    name: &str,
    lane_name: &str,
    bars: &MeterBars,
    offline: bool,
) -> String {
    let len = r.duration.max(1);
    let end = r.tick.checked_add(len - 1).unwrap_or(i64::MAX);
    let first = bars.bar_at(r.tick).number;
    let last = bars.bar_at(end.max(r.tick)).number;
    let span = if first == last {
        format!("bar {first}")
    } else {
        format!("bars {first} to {last}")
    };
    let mut s = format!(
        "Region {name} on {lane_name}, {span}, gain {}",
        region_gain_label(r.gain_db)
    );
    // A crossfade says so; its fade in is not one of the fades.
    if r.crossfade {
        s.push_str(", crossfade");
    }
    if (!r.crossfade && r.fade_in.length > 0) || r.fade_out.length > 0 {
        s.push_str(", fades");
    }
    if offline {
        s.push_str(", offline");
    }
    if r.mute {
        s.push_str(", muted");
    }
    s
}
