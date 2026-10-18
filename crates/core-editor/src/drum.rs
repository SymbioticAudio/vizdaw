//! The drum grid (editors spec §4e, parent spec §9 "Drum grid"): drum
//! lanes stacked as bands (a header, then one band per row, a row with
//! its velocity lane shown taller), a step matrix of evenly spaced cells
//! or a timeline of true starts and lengths, a paint tool that toggles a
//! hit on click and paints or erases a run on drag, and rows dragged to a
//! new place, another drum lane or a new lane.

use core_model::command::drum::MAX_PAINT_HITS;
use core_model::command::event::note_name;
use core_model::lane::QuantizeSpec;
use core_model::rules::MAX_TICKS;
use core_model::time::{PPQ, Tick};

/// Height of a lane's header band (its name, play quantize and nudge).
pub const HEADER_HEIGHT: f64 = 22.0;
/// Height of the band under the last lane that a row is dropped on to
/// split it into a lane of its own.
pub const NEW_LANE_HEIGHT: f64 = 28.0;
/// The most cells [`cells`] returns.
pub const MAX_CELLS: usize = 4096;
/// One nudge step (a 128th of a quarter: about 3.9 ms at 120 BPM).
pub const NUDGE_STEP: i64 = PPQ / 128;
/// A lane's nudge stays within a sixteenth either way.
pub const MAX_NUDGE: i64 = PPQ / 4;

/// How hits are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    /// Evenly spaced cells; a hit fills the cell its start falls in.
    Matrix,
    /// True start times and durations.
    Timeline,
}

/// What the paint tool is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    /// Toggles: a press on a hit erases, anywhere else paints.
    Paint,
    /// Always erases.
    Erase,
}

/// The grid's settings (saved with the project by the front end).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Matrix or timeline.
    pub view: View,
    /// A matrix cell in ticks.
    pub step: i64,
    /// Distance between a stroke's hits (in the matrix, whole cells).
    pub spacing: i64,
    /// Length of a painted hit.
    pub duration: i64,
    /// Velocity (1–127) of a painted hit when its row's velocity lane is
    /// hidden.
    pub velocity: u8,
    /// Height of a row's cells.
    pub row_height: f64,
    /// Height of a shown velocity lane under its row's cells.
    pub velocity_height: f64,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            view: View::Matrix,
            step: PPQ / 4,
            spacing: PPQ / 4,
            duration: PPQ / 8,
            velocity: 100,
            row_height: 20.0,
            velocity_height: 28.0,
        }
    }
}

fn clamp_f(v: f64, lo: f64, hi: f64, fallback: f64) -> f64 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

impl Settings {
    /// Every field within its range: a step of a 128th to a whole note
    /// (else a sixteenth), spacing 1 tick to four bars (else the step),
    /// duration 1 tick to four bars (else a 32nd), velocity 1–127, row
    /// heights 12–64 and 16–96 points (else the defaults).
    pub fn sane(&self) -> Settings {
        let d = Settings::default();
        let step = if self.step > 0 {
            self.step.clamp(PPQ / 32, PPQ * 4)
        } else {
            d.step
        };
        Settings {
            view: self.view,
            step,
            spacing: if self.spacing > 0 {
                self.spacing.min(PPQ * 16)
            } else {
                step
            },
            duration: if self.duration > 0 {
                self.duration.min(PPQ * 16)
            } else {
                d.duration
            },
            velocity: self.velocity.clamp(1, 127),
            row_height: clamp_f(self.row_height, 12.0, 64.0, d.row_height),
            velocity_height: clamp_f(self.velocity_height, 16.0, 96.0, d.velocity_height),
        }
    }

    /// Distance between a stroke's hits: the spacing, in the matrix
    /// rounded to whole cells (at least one).
    pub fn stride(&self) -> i64 {
        let s = self.sane();
        match s.view {
            View::Timeline => s.spacing,
            View::Matrix => {
                let cells = (s.spacing + s.step / 2) / s.step;
                cells.max(1) * s.step
            }
        }
    }
}

/// A drum lane's rows for layout: whether each shows its velocity lane.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaneShape {
    /// One entry per row, in display order.
    pub rows: Vec<bool>,
}

/// What a band is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BandKind {
    /// Lane `lane`'s header.
    Header {
        /// Lane index.
        lane: usize,
    },
    /// Row `row` of lane `lane`.
    Row {
        /// Lane index.
        lane: usize,
        /// Row index in the lane.
        row: usize,
    },
    /// The drop target for a new lane, under the last lane.
    NewLane,
}

/// One horizontal band of the grid, in content coordinates (y down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    /// What it is.
    pub kind: BandKind,
    /// Top edge.
    pub top: f64,
    /// Height.
    pub height: f64,
    /// Height of its cells (less than `height` when a row shows its
    /// velocity lane underneath).
    pub cells: f64,
}

impl Band {
    /// A row with its velocity lane shown.
    pub fn shows_velocity(&self) -> bool {
        matches!(self.kind, BandKind::Row { .. }) && self.height > self.cells
    }
}

/// The grid's bands, top to bottom.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    /// Headers, rows and the new-lane band.
    pub bands: Vec<Band>,
    /// Total height.
    pub height: f64,
}

/// Lanes stacked top to bottom: each a header and its rows, then the
/// new-lane band.
pub fn layout(lanes: &[LaneShape], settings: &Settings) -> Layout {
    let s = settings.sane();
    let mut bands = Vec::new();
    let mut y = 0.0;
    let mut push = |kind, height: f64, cells: f64, y: &mut f64| {
        bands.push(Band {
            kind,
            top: *y,
            height,
            cells,
        });
        *y += height;
    };
    for (lane, shape) in lanes.iter().enumerate() {
        push(
            BandKind::Header { lane },
            HEADER_HEIGHT,
            HEADER_HEIGHT,
            &mut y,
        );
        for (row, &velocity) in shape.rows.iter().enumerate() {
            let h = s.row_height + if velocity { s.velocity_height } else { 0.0 };
            push(BandKind::Row { lane, row }, h, s.row_height, &mut y);
        }
    }
    push(BandKind::NewLane, NEW_LANE_HEIGHT, NEW_LANE_HEIGHT, &mut y);
    Layout { bands, height: y }
}

/// What a height in the grid is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Spot {
    /// A lane's header.
    Header {
        /// Lane index.
        lane: usize,
    },
    /// A row; `velocity` (1–127, from the height) when its velocity lane
    /// is shown.
    Row {
        /// Lane index.
        lane: usize,
        /// Row index.
        row: usize,
        /// The velocity the height means.
        velocity: Option<u8>,
    },
    /// The new-lane band, or anywhere below it.
    NewLane,
    /// Above the grid, or not a number.
    Nothing,
}

/// The band `y` is in.
pub fn band_at(l: &Layout, y: f64) -> Option<&Band> {
    if !y.is_finite() || y < 0.0 {
        return None;
    }
    let i = l.bands.partition_point(|b| b.top + b.height <= y);
    l.bands.get(i).or_else(|| l.bands.last())
}

/// The velocity (1–127) a height in a row means: its bottom edge 1, its
/// top edge 127, linear in between (the whole row, cells and velocity
/// lane, so a stroke can sweep it; Plan 4e Ruling 6).
pub fn velocity_at(band: &Band, y: f64) -> u8 {
    if !y.is_finite() || band.height <= 0.0 || !band.height.is_finite() {
        return 64;
    }
    let f = ((band.top + band.height - y) / band.height).clamp(0.0, 1.0);
    // In 1..=127 by the clamp, so the cast is exact.
    (1.0 + f * 126.0).round().clamp(1.0, 127.0) as u8
}

/// What `y` is over.
pub fn spot(l: &Layout, y: f64) -> Spot {
    let Some(b) = band_at(l, y) else {
        return Spot::Nothing;
    };
    match b.kind {
        BandKind::Header { lane } => Spot::Header { lane },
        BandKind::Row { lane, row } => Spot::Row {
            lane,
            row,
            velocity: b.shows_velocity().then(|| velocity_at(b, y)),
        },
        BandKind::NewLane => Spot::NewLane,
    }
}

/// The band of row `row` of lane `lane`.
pub fn row_band(l: &Layout, lane: usize, row: usize) -> Option<&Band> {
    l.bands
        .iter()
        .find(|b| b.kind == BandKind::Row { lane, row })
}

/// The velocity height `y` means for row `row` of lane `lane` while a
/// stroke on it is dragged anywhere (clamped to the row), when the row
/// shows its velocity lane.
pub fn row_velocity(l: &Layout, lane: usize, row: usize, y: f64) -> Option<u8> {
    row_band(l, lane, row)
        .filter(|b| b.shows_velocity())
        .map(|b| velocity_at(b, y))
}

/// One note of a row, as the grid needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hit {
    /// Event ID.
    pub id: u64,
    /// Start.
    pub tick: i64,
    /// Length.
    pub duration: i64,
    /// 16-bit velocity.
    pub velocity: u16,
}

/// Start of the cell `tick` falls in (cells of `step` ticks from tick 0;
/// the step is taken within 1–2^40).
pub fn cell_start(tick: i64, step: i64) -> i64 {
    let step = step.clamp(1, MAX_TICKS);
    tick.div_euclid(step).saturating_mul(step)
}

/// One matrix cell.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Cell {
    /// Its start.
    pub tick: i64,
    /// The row's hits that start in it, by start.
    pub hits: Vec<u64>,
    /// The loudest of their velocities (0 when empty).
    pub velocity: u16,
}

/// The cells of `step` ticks that overlap `[from, to)`, at most
/// [`MAX_CELLS`], each with the hits that start in it.
pub fn cells(hits: &[Hit], from: i64, to: i64, step: i64) -> Vec<Cell> {
    let step = step.clamp(1, MAX_TICKS);
    let mut sorted: Vec<&Hit> = hits.iter().collect();
    sorted.sort_by_key(|h| (h.tick, h.id));
    let mut out = Vec::new();
    let mut c = cell_start(from, step);
    while c < to && out.len() < MAX_CELLS {
        let end = c.saturating_add(step);
        let lo = sorted.partition_point(|h| h.tick < c);
        let hi = sorted.partition_point(|h| h.tick < end);
        let inside = sorted.get(lo..hi).unwrap_or(&[]);
        out.push(Cell {
            tick: c,
            hits: inside.iter().map(|h| h.id).collect(),
            velocity: inside.iter().map(|h| h.velocity).max().unwrap_or(0),
        });
        if end == c {
            break;
        }
        c = end;
    }
    out
}

/// The hits under `tick` in the timeline: those whose
/// `[start, start + max(duration, grab))` holds it (`grab`: the ticks a
/// hit is wide at least on screen).
pub fn hits_at(hits: &[Hit], tick: i64, grab: i64) -> Vec<u64> {
    hits.iter()
        .filter(|h| h.tick <= tick && tick < h.tick.saturating_add(h.duration.max(grab).max(1)))
        .map(|h| h.id)
        .collect()
}

/// A paint or erase stroke on one row (Plan 4e Rulings 5–7).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Stroke {
    /// It erases (the tool, or a paint press on a hit).
    pub erase: bool,
    /// The pointer's tick at the press.
    pub anchor: i64,
    /// The first hit's tick (a cell start in the matrix, the snapped
    /// press in the timeline).
    pub start: i64,
    /// The pointer's tick now.
    pub to: i64,
    /// Painting: `(tick, 16-bit velocity)` of each hit, from `start`
    /// towards the pointer every stride.
    pub hits: Vec<(i64, u16)>,
    /// Erasing: the IDs of the row's hits the run covers.
    pub erased: Vec<u64>,
}

fn clamp_tick(t: i64) -> i64 {
    t.clamp(-MAX_TICKS, MAX_TICKS)
}

/// A 7-bit velocity as the model's 16-bit one.
pub fn velocity16(v: u8) -> u16 {
    u16::from(v.clamp(1, 127)) << 9
}

/// A press at tick `at` on a row with `hits` (`snapped`: `at` on the
/// shared edit grid, used by the timeline; `grab`: see [`hits_at`];
/// `velocity`: the row's velocity lane under the pointer, if shown).
pub fn begin(
    hits: &[Hit],
    at: i64,
    snapped: i64,
    grab: i64,
    velocity: Option<u8>,
    tool: Tool,
    settings: &Settings,
) -> Stroke {
    let s = settings.sane();
    let at = clamp_tick(at);
    let (start, occupied) = match s.view {
        View::Matrix => {
            let c = cell_start(at, s.step);
            (c, hits.iter().any(|h| cell_start(h.tick, s.step) == c))
        }
        View::Timeline => (clamp_tick(snapped), !hits_at(hits, at, grab).is_empty()),
    };
    let first = Stroke {
        erase: tool == Tool::Erase || occupied,
        anchor: at,
        start,
        to: at,
        hits: Vec::new(),
        erased: Vec::new(),
    };
    extend(&first, hits, at, grab, velocity, &s)
}

/// The stroke with the pointer at tick `at`: the run from its start to
/// the pointer (shrinking when the pointer goes back), each painted hit
/// keeping the velocity it got when the run first reached it.
pub fn extend(
    stroke: &Stroke,
    hits: &[Hit],
    at: i64,
    grab: i64,
    velocity: Option<u8>,
    settings: &Settings,
) -> Stroke {
    let s = settings.sane();
    let stride = s.stride();
    let at = clamp_tick(at);
    let end = match s.view {
        View::Matrix => cell_start(at, s.step),
        View::Timeline => at,
    };
    let span = end.saturating_sub(stroke.start);
    let n = usize::try_from((span / stride).unsigned_abs()).unwrap_or(usize::MAX);
    let dir = if span < 0 { -stride } else { stride };
    let positions: Vec<i64> = (0..=n.min(MAX_PAINT_HITS - 1))
        .map(|k| {
            let k = i64::try_from(k).unwrap_or(0);
            stroke.start.saturating_add(dir.saturating_mul(k))
        })
        .filter(|t| (-MAX_TICKS..=MAX_TICKS).contains(t))
        .collect();
    let mut out = Stroke {
        to: at,
        hits: Vec::new(),
        erased: Vec::new(),
        ..stroke.clone()
    };
    if stroke.erase {
        let mut ids: Vec<u64> = match s.view {
            View::Matrix => hits
                .iter()
                .filter(|h| positions.contains(&cell_start(h.tick, s.step)))
                .map(|h| h.id)
                .collect(),
            View::Timeline => {
                let (lo, hi) = (stroke.anchor.min(at), stroke.anchor.max(at));
                hits.iter()
                    .filter(|h| {
                        h.tick <= hi && h.tick.saturating_add(h.duration.max(grab).max(1)) > lo
                    })
                    .map(|h| h.id)
                    .collect()
            }
        };
        ids.sort_unstable();
        ids.dedup();
        out.erased = ids;
    } else {
        let now = velocity16(velocity.unwrap_or(s.velocity));
        out.hits = positions
            .iter()
            .map(|&t| {
                let kept = stroke.hits.iter().find(|(p, _)| *p == t).map(|(_, v)| *v);
                (t, kept.unwrap_or(now))
            })
            .collect();
    }
    out
}

/// The window a painted hit checks for an existing one: a cell in the
/// matrix, one tick (the same start) in the timeline.
pub fn paint_window(settings: &Settings) -> i64 {
    let s = settings.sane();
    match s.view {
        View::Matrix => s.step,
        View::Timeline => 1,
    }
}

/// Where a row dragged from `(lane, row)` lands when released at `y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Drop {
    /// To index `to` in its own lane.
    Reorder {
        /// New index.
        to: usize,
    },
    /// Into another drum lane (appended).
    ToLane {
        /// Lane index.
        lane: usize,
    },
    /// Into a new lane of its own.
    NewLane,
    /// Nowhere (its own place, or above the grid).
    Nothing,
}

/// [`Drop`] for a row of lane `lane` at index `row` released at `y`.
pub fn drop_row(l: &Layout, lane: usize, row: usize, y: f64) -> Drop {
    match spot(l, y) {
        Spot::Row {
            lane: to, row: r, ..
        } if to == lane => {
            if r == row {
                Drop::Nothing
            } else {
                Drop::Reorder { to: r }
            }
        }
        Spot::Header { lane: to } if to == lane => {
            if row == 0 {
                Drop::Nothing
            } else {
                Drop::Reorder { to: 0 }
            }
        }
        Spot::Row { lane: to, .. } | Spot::Header { lane: to } => Drop::ToLane { lane: to },
        Spot::NewLane => Drop::NewLane,
        Spot::Nothing => Drop::Nothing,
    }
}

/// A lane's play shift moved by `steps` nudge steps, within
/// ±[`MAX_NUDGE`].
pub fn nudged(shift: i64, steps: i64) -> i64 {
    shift
        .saturating_add(steps.saturating_mul(NUDGE_STEP))
        .clamp(-MAX_NUDGE, MAX_NUDGE)
}

/// The play quantize menu (Plan 4e Ruling 10): Off, straight grids, and
/// swing on eighths and sixteenths.
pub fn quantize_choices() -> Vec<(String, Option<QuantizeSpec>)> {
    let mut out = vec![("Off".to_string(), None)];
    for (title, grid) in [
        ("1/4", PPQ),
        ("1/8", PPQ / 2),
        ("1/8 Triplet", PPQ / 3),
        ("1/16", PPQ / 4),
        ("1/16 Triplet", PPQ / 6),
        ("1/32", PPQ / 8),
    ] {
        out.push((title.to_string(), Some(QuantizeSpec::grid(Tick(grid), 100))));
    }
    for (name, grid) in [("1/8", PPQ / 2), ("1/16", PPQ / 4)] {
        for p in [54u8, 58, 62, 66] {
            out.push((
                format!("{name} Swing {p}%"),
                Some(QuantizeSpec::swing(Tick(grid), p)),
            ));
        }
    }
    out
}

/// The menu title of a lane's play quantize: its choice's, else
/// "Custom".
pub fn quantize_title(q: Option<&QuantizeSpec>) -> String {
    quantize_choices()
        .into_iter()
        .find(|(_, c)| c.as_ref() == q)
        .map_or_else(|| "Custom".to_string(), |(t, _)| t)
}

/// General MIDI percussion names, notes 35–81.
const GM: [&str; 47] = [
    "Acoustic Bass Drum",
    "Kick",
    "Side Stick",
    "Snare",
    "Hand Clap",
    "Electric Snare",
    "Low Floor Tom",
    "Closed Hat",
    "High Floor Tom",
    "Pedal Hat",
    "Low Tom",
    "Open Hat",
    "Low-Mid Tom",
    "Hi-Mid Tom",
    "Crash",
    "High Tom",
    "Ride",
    "Chinese Cymbal",
    "Ride Bell",
    "Tambourine",
    "Splash",
    "Cowbell",
    "Crash 2",
    "Vibraslap",
    "Ride 2",
    "Hi Bongo",
    "Low Bongo",
    "Mute Hi Conga",
    "Open Hi Conga",
    "Low Conga",
    "High Timbale",
    "Low Timbale",
    "High Agogo",
    "Low Agogo",
    "Cabasa",
    "Maracas",
    "Short Whistle",
    "Long Whistle",
    "Short Guiro",
    "Long Guiro",
    "Claves",
    "Hi Wood Block",
    "Low Wood Block",
    "Mute Cuica",
    "Open Cuica",
    "Mute Triangle",
    "Open Triangle",
];

/// The name a new row for `note` gets: its General MIDI percussion name,
/// else the note's name ("C4" is 60).
pub fn row_name(note: u8) -> String {
    match note.checked_sub(35).and_then(|i| GM.get(usize::from(i))) {
        Some(n) => (*n).to_string(),
        None => note_name(note),
    }
}

/// The order Add Row tries notes in: the usual kit first, then the rest
/// of General MIDI percussion, then every other note.
const PREFERRED: [u8; 12] = [36, 38, 42, 46, 39, 49, 51, 45, 47, 50, 37, 44];

/// The note and name of the row Add Row makes in a lane whose rows use
/// `used` notes (rows with their own destination count too); `None` when
/// all 128 are taken.
pub fn next_row(used: &[u8]) -> Option<(u8, String)> {
    PREFERRED
        .iter()
        .copied()
        .chain(35..=81)
        .chain(0..=127)
        .find(|n| !used.contains(n))
        .map(|n| (n, row_name(n)))
}
