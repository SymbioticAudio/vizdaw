//! The drum grid's FFI (editors spec §4e): drum lanes with their rows and
//! hits, the `Drum` edit group, and `core-editor`'s grid logic as pure
//! functions of records, so Swift only draws (Plan 4e Ruling 1).

use crate::edit::{EditCommand, EventEdit, QuantizeRow, quantize_of, quantize_row};
use crate::error::VizError;
use crate::guard::ffi_guard;
use core_editor::drum as grid;
use core_model::command::DrumCmd;
use core_model::command::drum::DrumHit;
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::{DestId, DrumRowId, LaneId, SeqId};
use core_model::time::Tick;

/// One note of a drum row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumHitRow {
    /// Event ID.
    pub id: u64,
    /// Start.
    pub tick: i64,
    /// Length.
    pub duration: i64,
    /// 16-bit velocity.
    pub velocity: u16,
    /// Muted.
    pub mute: bool,
}

/// One row of a drum lane.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumRowRow {
    /// Row ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// Note 0–127.
    pub note: u8,
    /// The row's destination override.
    pub dest: Option<u64>,
    /// Name of the destination its notes play on (the row's, else the
    /// lane's).
    pub destination: Option<String>,
    /// The grid shows its velocity lane.
    pub show_velocity: bool,
    /// Its notes in the active take, by start.
    pub hits: Vec<DrumHitRow>,
}

/// One drum lane of a sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumLaneRow {
    /// Lane ID.
    pub lane: u64,
    /// Name.
    pub name: String,
    /// Name of the lane's MIDI destination.
    pub destination: Option<String>,
    /// Muted.
    pub mute: bool,
    /// Soloed.
    pub solo: bool,
    /// Play quantize.
    pub play_quantize: Option<QuantizeRow>,
    /// Play shift (nudge) in ticks.
    pub play_shift: i64,
    /// Rows in display order.
    pub rows: Vec<DrumRowRow>,
}

/// The drum lanes of sequence `seq`, in lane order.
pub fn drum_lanes(doc: &Document, seq: u64) -> Result<Vec<DrumLaneRow>, VizError> {
    let s = doc.sequence(crate::snapshot::seq_id(doc, seq)?)?;
    let name =
        |d: Option<DestId>| d.and_then(|d| doc.instruments().get(&d).map(|x| x.name.clone()));
    Ok(s.lanes
        .iter()
        .filter_map(|l| {
            let layout = l.drum.as_ref()?;
            let notes: Vec<_> = l
                .active()
                .map(|t| t.events.values().collect())
                .unwrap_or_default();
            let rows = layout
                .rows
                .iter()
                .map(|r| DrumRowRow {
                    id: r.id.0,
                    name: r.name.clone(),
                    note: r.note,
                    dest: r.dest.map(|d| d.0),
                    destination: name(r.dest.or(l.midi_dest)),
                    show_velocity: r.show_velocity,
                    hits: notes
                        .iter()
                        .filter_map(|e| match e.body {
                            EventBody::Note {
                                pitch,
                                velocity,
                                duration,
                                ..
                            } if r.matches(pitch, e.dest) => Some(DrumHitRow {
                                id: e.id.0,
                                tick: e.tick.0,
                                duration: duration.0,
                                velocity,
                                mute: e.mute,
                            }),
                            _ => None,
                        })
                        .collect(),
                })
                .collect();
            Some(DrumLaneRow {
                lane: l.id.0,
                name: l.name.clone(),
                destination: name(l.midi_dest),
                mute: l.mute,
                solo: l.solo,
                play_quantize: l.play_quantize.as_ref().map(quantize_row),
                play_shift: l.play_shift.0,
                rows,
            })
        })
        .collect())
}

/// One hit a paint adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumPaintHitRow {
    /// Start.
    pub tick: i64,
    /// 16-bit velocity (> 0).
    pub velocity: u16,
}

/// Mirrors `core_model::command::DrumCmd`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum DrumEdit {
    /// Make a lane a drum lane (rows for its notes) or a normal lane.
    MarkLane {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Drum lane or not.
        drum: bool,
    },
    /// Add a row; `(note, dest)` must be new in the lane.
    AddRow {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Name.
        name: String,
        /// Note 0–127.
        note: u8,
        /// Destination override.
        dest: Option<u64>,
    },
    /// Rename a row.
    RenameRow {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
        /// New name.
        name: String,
    },
    /// Show or hide a row's velocity lane.
    ShowVelocity {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
        /// Shown.
        show: bool,
    },
    /// Move a row to index `to` of its lane.
    ReorderRow {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
        /// New index (clamped).
        to: u32,
    },
    /// Remove a row and its notes.
    RemoveRow {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
    },
    /// Move a row and its notes to another lane of the sequence.
    MoveRowToLane {
        /// Sequence.
        seq: u64,
        /// Source lane.
        lane: u64,
        /// Row.
        row: u64,
        /// Target lane.
        to_lane: u64,
    },
    /// Move a row to a new lane of its own after its lane.
    SplitRow {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
    },
    /// Paint hits into a row (`DrumCmd::Paint`).
    Paint {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
        /// The hits.
        hits: Vec<DrumPaintHitRow>,
        /// A hit is skipped when the row has a note starting this close
        /// after it (> 0).
        window: i64,
        /// Length of each new note (> 0).
        duration: i64,
    },
}

/// The core command of a drum edit.
pub fn drum_cmd(e: &DrumEdit) -> DrumCmd {
    let s = |seq: &u64| SeqId(*seq);
    match e {
        DrumEdit::MarkLane { seq, lane, drum } => DrumCmd::MarkDrumLane {
            seq: s(seq),
            lane: LaneId(*lane),
            drum: *drum,
        },
        DrumEdit::AddRow {
            seq,
            lane,
            name,
            note,
            dest,
        } => DrumCmd::AddRow {
            seq: s(seq),
            lane: LaneId(*lane),
            name: name.clone(),
            note: *note,
            dest: dest.map(DestId),
        },
        DrumEdit::RenameRow {
            seq,
            lane,
            row,
            name,
        } => DrumCmd::RenameRow {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
            name: name.clone(),
        },
        DrumEdit::ShowVelocity {
            seq,
            lane,
            row,
            show,
        } => DrumCmd::SetRowShowVelocity {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
            show: *show,
        },
        DrumEdit::ReorderRow { seq, lane, row, to } => DrumCmd::ReorderRow {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
            to: usize::try_from(*to).unwrap_or(usize::MAX),
        },
        DrumEdit::RemoveRow { seq, lane, row } => DrumCmd::RemoveRow {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
        },
        DrumEdit::MoveRowToLane {
            seq,
            lane,
            row,
            to_lane,
        } => DrumCmd::MoveRowToLane {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
            to_lane: LaneId(*to_lane),
        },
        DrumEdit::SplitRow { seq, lane, row } => DrumCmd::SplitRowToLane {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
        },
        DrumEdit::Paint {
            seq,
            lane,
            row,
            hits,
            window,
            duration,
        } => DrumCmd::Paint {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
            hits: hits
                .iter()
                .map(|h| DrumHit {
                    tick: Tick(h.tick),
                    velocity: h.velocity,
                })
                .collect(),
            window: Tick(*window),
            duration: Tick(*duration),
        },
    }
}

/// How the grid shows hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum DrumViewRow {
    /// Evenly spaced cells.
    Matrix,
    /// True starts and lengths.
    Timeline,
}

/// The grid's settings (`core_editor::drum::Settings`).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct DrumGridSettingsRow {
    /// Matrix or timeline.
    pub view: DrumViewRow,
    /// A matrix cell in ticks.
    pub step: i64,
    /// Distance between a stroke's hits.
    pub spacing: i64,
    /// Length of a painted hit.
    pub duration: i64,
    /// Velocity (1–127) of a painted hit while its row's velocity lane is
    /// hidden.
    pub velocity: u8,
    /// Height of a row's cells.
    pub row_height: f64,
    /// Height of a shown velocity lane.
    pub velocity_height: f64,
}

fn settings(s: &DrumGridSettingsRow) -> grid::Settings {
    grid::Settings {
        view: match s.view {
            DrumViewRow::Matrix => grid::View::Matrix,
            DrumViewRow::Timeline => grid::View::Timeline,
        },
        step: s.step,
        spacing: s.spacing,
        duration: s.duration,
        velocity: s.velocity,
        row_height: s.row_height,
        velocity_height: s.velocity_height,
    }
}

fn row_of(s: &grid::Settings) -> DrumGridSettingsRow {
    DrumGridSettingsRow {
        view: match s.view {
            grid::View::Matrix => DrumViewRow::Matrix,
            grid::View::Timeline => DrumViewRow::Timeline,
        },
        step: s.step,
        spacing: s.spacing,
        duration: s.duration,
        velocity: s.velocity,
        row_height: s.row_height,
        velocity_height: s.velocity_height,
    }
}

/// The settings as every grid function uses them (`Settings::sane`), so a
/// front end draws with the step, spacing and duration Rust paints with.
#[uniffi::export]
pub fn drum_grid_settings_sane(
    settings: DrumGridSettingsRow,
) -> Result<DrumGridSettingsRow, VizError> {
    ffi_guard(|| Ok(row_of(&self::settings(&settings).sane())))
}

/// What a band of the grid is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum DrumBandKindRow {
    /// A lane's header.
    Header {
        /// Lane index.
        lane: u32,
    },
    /// A row.
    Row {
        /// Lane index.
        lane: u32,
        /// Row index.
        row: u32,
    },
    /// The new-lane band.
    NewLane,
}

/// One band (content coordinates, y down).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct DrumBandRow {
    /// What it is.
    pub kind: DrumBandKindRow,
    /// Top.
    pub top: f64,
    /// Height.
    pub height: f64,
    /// Height of its cells.
    pub cells: f64,
}

/// The grid's bands.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DrumGridLayoutRow {
    /// Bands, top to bottom.
    pub bands: Vec<DrumBandRow>,
    /// Total height.
    pub height: f64,
}

/// What a height is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum DrumSpotRow {
    /// A lane's header.
    Header {
        /// Lane index.
        lane: u32,
    },
    /// A row (with the velocity the height means when its velocity lane
    /// is shown).
    Row {
        /// Lane index.
        lane: u32,
        /// Row index.
        row: u32,
        /// 1–127.
        velocity: Option<u8>,
    },
    /// The new-lane band or below.
    NewLane,
    /// Nothing.
    Nothing,
}

/// One matrix cell.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumCellRow {
    /// Start.
    pub tick: i64,
    /// The hits starting in it.
    pub hits: Vec<u64>,
    /// The loudest velocity (0 when empty).
    pub velocity: u16,
}

/// A paint or erase stroke (`core_editor::drum::Stroke`) on row `row` of
/// lane `lane` (indexes in the layout).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumStrokeRow {
    /// Lane index.
    pub lane: u32,
    /// Row index.
    pub row: u32,
    /// It erases.
    pub erase: bool,
    /// The pointer's tick at the press.
    pub anchor: i64,
    /// The first hit's tick.
    pub start: i64,
    /// The pointer's tick now.
    pub to: i64,
    /// Painting: the hits.
    pub hits: Vec<DrumPaintHitRow>,
    /// Erasing: the IDs of the hits it covers.
    pub erased: Vec<u64>,
}

/// One entry of the play quantize menu.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct QuantizeChoiceRow {
    /// Menu title.
    pub title: String,
    /// The quantize (`None`: off).
    pub quantize: Option<QuantizeRow>,
}

/// The row Add Row makes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumNewRow {
    /// Note.
    pub note: u8,
    /// Name.
    pub name: String,
}

fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

fn hits_of(h: &[DrumHitRow]) -> Vec<grid::Hit> {
    h.iter()
        .map(|h| grid::Hit {
            id: h.id,
            tick: h.tick,
            duration: h.duration,
            velocity: h.velocity,
        })
        .collect()
}

fn layout_of(l: &DrumGridLayoutRow) -> grid::Layout {
    grid::Layout {
        bands: l
            .bands
            .iter()
            .map(|b| grid::Band {
                kind: match b.kind {
                    DrumBandKindRow::Header { lane } => grid::BandKind::Header {
                        lane: usize::try_from(lane).unwrap_or(usize::MAX),
                    },
                    DrumBandKindRow::Row { lane, row } => grid::BandKind::Row {
                        lane: usize::try_from(lane).unwrap_or(usize::MAX),
                        row: usize::try_from(row).unwrap_or(usize::MAX),
                    },
                    DrumBandKindRow::NewLane => grid::BandKind::NewLane,
                },
                top: b.top,
                height: b.height,
                cells: b.cells,
            })
            .collect(),
        height: l.height,
    }
}

fn stroke_of(s: &DrumStrokeRow) -> grid::Stroke {
    grid::Stroke {
        erase: s.erase,
        anchor: s.anchor,
        start: s.start,
        to: s.to,
        hits: s.hits.iter().map(|h| (h.tick, h.velocity)).collect(),
        erased: s.erased.clone(),
    }
}

fn stroke_row(lane: u32, row: u32, s: grid::Stroke) -> DrumStrokeRow {
    DrumStrokeRow {
        lane,
        row,
        erase: s.erase,
        anchor: s.anchor,
        start: s.start,
        to: s.to,
        hits: s
            .hits
            .into_iter()
            .map(|(tick, velocity)| DrumPaintHitRow { tick, velocity })
            .collect(),
        erased: s.erased,
    }
}

/// The bands of drum lanes whose rows show their velocity lanes as in
/// `shapes` (one list per lane, one flag per row).
#[uniffi::export]
pub fn drum_grid_layout(
    shapes: Vec<Vec<bool>>,
    settings: DrumGridSettingsRow,
) -> Result<DrumGridLayoutRow, VizError> {
    ffi_guard(|| {
        let lanes: Vec<grid::LaneShape> = shapes
            .into_iter()
            .map(|rows| grid::LaneShape { rows })
            .collect();
        let l = grid::layout(&lanes, &self::settings(&settings));
        Ok(DrumGridLayoutRow {
            bands: l
                .bands
                .iter()
                .map(|b| DrumBandRow {
                    kind: match b.kind {
                        grid::BandKind::Header { lane } => {
                            DrumBandKindRow::Header { lane: index(lane) }
                        }
                        grid::BandKind::Row { lane, row } => DrumBandKindRow::Row {
                            lane: index(lane),
                            row: index(row),
                        },
                        grid::BandKind::NewLane => DrumBandKindRow::NewLane,
                    },
                    top: b.top,
                    height: b.height,
                    cells: b.cells,
                })
                .collect(),
            height: l.height,
        })
    })
}

/// What height `y` of `layout` is over.
#[uniffi::export]
pub fn drum_grid_spot(layout: DrumGridLayoutRow, y: f64) -> Result<DrumSpotRow, VizError> {
    ffi_guard(|| {
        Ok(match grid::spot(&layout_of(&layout), y) {
            grid::Spot::Header { lane } => DrumSpotRow::Header { lane: index(lane) },
            grid::Spot::Row {
                lane,
                row,
                velocity,
            } => DrumSpotRow::Row {
                lane: index(lane),
                row: index(row),
                velocity,
            },
            grid::Spot::NewLane => DrumSpotRow::NewLane,
            grid::Spot::Nothing => DrumSpotRow::Nothing,
        })
    })
}

/// The velocity height `y` means for row `row` of lane `lane` (layout
/// indexes) while a stroke on it is dragged (clamped to the row); `None`
/// when the row hides its velocity lane.
#[uniffi::export]
pub fn drum_row_velocity(
    layout: DrumGridLayoutRow,
    lane: u32,
    row: u32,
    y: f64,
) -> Result<Option<u8>, VizError> {
    ffi_guard(|| {
        Ok(grid::row_velocity(
            &layout_of(&layout),
            usize::try_from(lane).unwrap_or(usize::MAX),
            usize::try_from(row).unwrap_or(usize::MAX),
            y,
        ))
    })
}

/// The matrix cells of `step` ticks overlapping `[from, to)` with the
/// hits that start in each (at most 4 096).
#[uniffi::export]
pub fn drum_grid_cells(
    hits: Vec<DrumHitRow>,
    from: i64,
    to: i64,
    step: i64,
) -> Result<Vec<DrumCellRow>, VizError> {
    ffi_guard(|| {
        Ok(grid::cells(&hits_of(&hits), from, to, step)
            .into_iter()
            .map(|c| DrumCellRow {
                tick: c.tick,
                hits: c.hits,
                velocity: c.velocity,
            })
            .collect())
    })
}

/// The hits under `tick` in the timeline (`grab`: a hit's least width in
/// ticks).
#[uniffi::export]
pub fn drum_grid_hits_at(
    hits: Vec<DrumHitRow>,
    tick: i64,
    grab: i64,
) -> Result<Vec<u64>, VizError> {
    ffi_guard(|| Ok(grid::hits_at(&hits_of(&hits), tick, grab)))
}

/// A press on row `row` of lane `lane` (layout indexes) whose hits are
/// `hits`, at tick `at` (`snapped`: on the shared edit grid); `velocity`
/// from a shown velocity lane; `erase_tool`: the eraser is chosen.
#[allow(clippy::too_many_arguments)]
#[uniffi::export]
pub fn drum_stroke_begin(
    lane: u32,
    row: u32,
    hits: Vec<DrumHitRow>,
    at: i64,
    snapped: i64,
    grab: i64,
    velocity: Option<u8>,
    erase_tool: bool,
    settings: DrumGridSettingsRow,
) -> Result<DrumStrokeRow, VizError> {
    ffi_guard(|| {
        let tool = if erase_tool {
            grid::Tool::Erase
        } else {
            grid::Tool::Paint
        };
        let s = grid::begin(
            &hits_of(&hits),
            at,
            snapped,
            grab,
            velocity,
            tool,
            &self::settings(&settings),
        );
        Ok(stroke_row(lane, row, s))
    })
}

/// `stroke` with the pointer at tick `at`.
#[uniffi::export]
pub fn drum_stroke_extend(
    stroke: DrumStrokeRow,
    hits: Vec<DrumHitRow>,
    at: i64,
    grab: i64,
    velocity: Option<u8>,
    settings: DrumGridSettingsRow,
) -> Result<DrumStrokeRow, VizError> {
    ffi_guard(|| {
        let s = grid::extend(
            &stroke_of(&stroke),
            &hits_of(&hits),
            at,
            grab,
            velocity,
            &self::settings(&settings),
        );
        Ok(stroke_row(stroke.lane, stroke.row, s))
    })
}

/// The edit a stroke on row `row` of lane `lane` of sequence `seq` makes:
/// a paint, a delete, or nothing when it covers nothing.
#[uniffi::export]
pub fn drum_stroke_command(
    stroke: DrumStrokeRow,
    seq: u64,
    lane: u64,
    row: u64,
    settings: DrumGridSettingsRow,
) -> Result<Option<EditCommand>, VizError> {
    ffi_guard(|| {
        let s = self::settings(&settings).sane();
        Ok(if stroke.erase {
            (!stroke.erased.is_empty()).then(|| EditCommand::Event {
                edit: EventEdit::Delete {
                    events: stroke.erased.clone(),
                },
            })
        } else {
            (!stroke.hits.is_empty()).then(|| EditCommand::Drum {
                edit: DrumEdit::Paint {
                    seq,
                    lane,
                    row,
                    hits: stroke.hits.clone(),
                    window: grid::paint_window(&s),
                    duration: s.duration,
                },
            })
        })
    })
}

/// The edit that drops row `row` (ID) at index `row_index` of lane
/// `lane_index` at height `y`; `lanes` are the shown lanes' IDs in
/// layout order. `None`: it lands where it is, or nowhere.
#[allow(clippy::too_many_arguments)]
#[uniffi::export]
pub fn drum_row_drop(
    layout: DrumGridLayoutRow,
    lanes: Vec<u64>,
    seq: u64,
    lane_index: u32,
    row: u64,
    row_index: u32,
    y: f64,
) -> Result<Option<EditCommand>, VizError> {
    ffi_guard(|| {
        let li = usize::try_from(lane_index).unwrap_or(usize::MAX);
        let Some(&lane) = lanes.get(li) else {
            return Ok(None);
        };
        let ri = usize::try_from(row_index).unwrap_or(usize::MAX);
        let edit = match grid::drop_row(&layout_of(&layout), li, ri, y) {
            grid::Drop::Reorder { to } => DrumEdit::ReorderRow {
                seq,
                lane,
                row,
                to: index(to),
            },
            grid::Drop::ToLane { lane: to } => match lanes.get(to) {
                Some(&to_lane) => DrumEdit::MoveRowToLane {
                    seq,
                    lane,
                    row,
                    to_lane,
                },
                None => return Ok(None),
            },
            grid::Drop::NewLane => DrumEdit::SplitRow { seq, lane, row },
            grid::Drop::Nothing => return Ok(None),
        };
        Ok(Some(EditCommand::Drum { edit }))
    })
}

/// The play quantize menu: Off, grids and swings.
#[uniffi::export]
pub fn drum_quantize_choices() -> Result<Vec<QuantizeChoiceRow>, VizError> {
    ffi_guard(|| {
        Ok(grid::quantize_choices()
            .into_iter()
            .map(|(title, q)| QuantizeChoiceRow {
                title,
                quantize: q.as_ref().map(quantize_row),
            })
            .collect())
    })
}

/// The menu title of a play quantize ("Custom" when it is no choice).
#[uniffi::export]
pub fn drum_quantize_title(quantize: Option<QuantizeRow>) -> Result<String, VizError> {
    ffi_guard(|| {
        Ok(grid::quantize_title(
            quantize.as_ref().map(quantize_of).as_ref(),
        ))
    })
}

/// A play shift moved by `steps` nudge steps (within a sixteenth).
#[uniffi::export]
pub fn drum_nudged(shift: i64, steps: i64) -> Result<i64, VizError> {
    ffi_guard(|| Ok(grid::nudged(shift, steps)))
}

/// The row Add Row makes in a lane whose rows use `used` notes.
#[uniffi::export]
pub fn drum_next_row(used: Vec<u8>) -> Result<Option<DrumNewRow>, VizError> {
    ffi_guard(|| Ok(grid::next_row(&used).map(|(note, name)| DrumNewRow { note, name })))
}
