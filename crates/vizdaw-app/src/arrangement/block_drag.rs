//! Hit testing and block drags (Swift `ArrangementHitTest`, `BlockDrag`
//! and the drag part of `ArrangementView`): moving, ⌥-copying and merging
//! blocks. Pure: the document model sends what these compute.

use std::collections::HashSet;

use vizdaw_ffi::blocks::BlockRow;
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
use vizdaw_ffi::events::{EventRow, MeterEntryRow};

use super::{Geometry, Point, TimeAxis};
use crate::editor::Snap;

/// What a point of the arrangement body lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrangementHit {
    /// Block `index` of track `lane`'s blocks.
    Block { lane: u64, index: usize },
    /// No block: the track of the row (none below the last), and the tick.
    Empty { lane: Option<u64>, tick: i64 },
}

/// What `point` (body coordinates) lands on; among overlapping blocks the
/// last drawn wins. `rows` are the shown tracks in order, with their
/// blocks.
pub fn hit_test<B: AsRef<[BlockRow]>>(
    geometry: &Geometry,
    axis: &TimeAxis,
    rows: &[(u64, B)],
    point: Point,
) -> ArrangementHit {
    let tick = axis.tick_at(point.x);
    let Some(row) = geometry.row_at(point.y, rows.len()) else {
        return ArrangementHit::Empty { lane: None, tick };
    };
    let (lane, blocks) = &rows[row];
    let found = blocks
        .as_ref()
        .iter()
        .enumerate()
        .rev()
        .find(|(_, b)| geometry.block_rect(b, row, axis).contains(point));
    match found {
        Some((index, _)) => ArrangementHit::Block { lane: *lane, index },
        None => ArrangementHit::Empty {
            lane: Some(*lane),
            tick,
        },
    }
}

/// Moving, copying and merging blocks (Swift `BlockDrag`).
pub struct BlockDrag;

impl BlockDrag {
    /// The dragged distance from `start_tick` (the press) to the pointer
    /// at `to_x`, snapped so the pressed block's start lands on the grid.
    pub fn delta(
        axis: &TimeAxis,
        start_tick: i64,
        to_x: f32,
        block_start: i64,
        snap: &Snap,
        meter: &[MeterEntryRow],
    ) -> i64 {
        let raw = axis.tick_at(to_x).saturating_sub(start_tick);
        if !snap.enabled {
            return raw;
        }
        snap.apply(block_start.saturating_add(raw), meter)
            .saturating_sub(block_start)
    }

    /// The events a drag carries: the pressed block's, plus those of every
    /// other fully selected block of the lane when the pressed one is
    /// selected.
    pub fn events(
        pressed: &BlockRow,
        lane_blocks: &[BlockRow],
        selected: &HashSet<u64>,
    ) -> Vec<u64> {
        let is_selected =
            |b: &BlockRow| !b.events.is_empty() && b.events.iter().all(|e| selected.contains(e));
        if !is_selected(pressed) {
            return pressed.events.clone();
        }
        lane_blocks
            .iter()
            .filter(|b| is_selected(b))
            .flat_map(|b| b.events.iter().copied())
            .collect()
    }

    /// The full target of a drag of `events` from lane `from` by `delta`
    /// into lane `to`: a move (merging into `to`), or with `copy` shifted
    /// copies of the `rows` — a duplicate in their own lane, an insert
    /// into another. `None` when nothing would change.
    pub fn command(
        events: &[u64],
        rows: &[EventRow],
        from: LaneRef,
        to: LaneRef,
        delta: i64,
        copy: bool,
    ) -> Option<EditCommand> {
        if delta == 0 && from == to {
            return None;
        }
        let edit = if copy && from == to {
            let present: HashSet<u64> = rows.iter().map(|r| r.id).collect();
            let copied: Vec<u64> = events
                .iter()
                .copied()
                .filter(|e| present.contains(e))
                .collect();
            if copied.is_empty() {
                return None;
            }
            EventEdit::Duplicate {
                events: copied,
                ticks: delta,
                semitones: 0,
            }
        } else if copy {
            let ids: HashSet<u64> = events.iter().copied().collect();
            let copies: Vec<EventRow> = rows
                .iter()
                .filter(|r| ids.contains(&r.id))
                .map(|r| EventRow {
                    tick: r.tick.saturating_add(delta),
                    ..r.clone()
                })
                .collect();
            if copies.is_empty() {
                return None;
            }
            EventEdit::Insert {
                seq: to.seq,
                lane: to.lane,
                events: copies,
            }
        } else {
            EventEdit::Move {
                events: events.to_vec(),
                delta,
                to_lane: (from != to).then_some(to),
            }
        };
        Some(EditCommand::Event { edit })
    }

    /// The drag's undo step.
    pub fn label(copy: bool) -> &'static str {
        if copy { "Copy Block" } else { "Move Block" }
    }
}

/// How the arrangement body is drawn now: rows and the time axis (which
/// auto-scroll moves during a drag).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ArrangementFrame {
    pub geometry: Geometry,
    pub axis: TimeAxis,
}

/// A block drag in progress (Swift `ArrangementView.Drag`): what was
/// pressed and what the drag carries, fixed when it began.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockDragState {
    /// The pressed block's track.
    pub source: LaneRef,
    /// The pressed block's start (the tick that snaps).
    pub block_start: i64,
    pub events: Vec<u64>,
    /// The source track's rows, for a copy (empty for a move).
    pub rows: Vec<EventRow>,
    /// ⌥ was held when the drag began (pressing it later changes nothing).
    pub copy: bool,
    /// The tick under the press.
    pub start_tick: i64,
}

impl BlockDragState {
    /// The track under `to`; the source track outside every row. `lanes`
    /// are the shown tracks in row order.
    pub fn target_lane(&self, frame: &ArrangementFrame, lanes: &[u64], to: Point) -> LaneRef {
        match frame.geometry.row_at(to.y, lanes.len()) {
            Some(i) => LaneRef {
                seq: self.source.seq,
                lane: lanes[i],
            },
            None => self.source,
        }
    }

    /// The tracks the drag edits with the pointer at `to` (source and
    /// target), as gesture lanes.
    pub fn touched(&self, frame: &ArrangementFrame, lanes: &[u64], to: Point) -> Vec<(u64, u64)> {
        let t = self.target_lane(frame, lanes, to);
        vec![(self.source.seq, self.source.lane), (t.seq, t.lane)]
    }

    /// The drag's full target with the pointer at `to`.
    pub fn target(
        &self,
        frame: &ArrangementFrame,
        lanes: &[u64],
        to: Point,
        snap: &Snap,
        meter: &[MeterEntryRow],
    ) -> Option<EditCommand> {
        let delta = BlockDrag::delta(
            &frame.axis,
            self.start_tick,
            to.x,
            self.block_start,
            snap,
            meter,
        );
        BlockDrag::command(
            &self.events,
            &self.rows,
            self.source,
            self.target_lane(frame, lanes, to),
            delta,
            self.copy,
        )
    }
}
