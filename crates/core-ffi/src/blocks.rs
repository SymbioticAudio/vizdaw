//! Arrangement blocks (spec §3.1, Plan Ruling 5): a lane's active take
//! grouped for the arrangement. A reference event is always its own block;
//! other events (audio regions included) group by the rule.

use crate::error::VizError;
use crate::events::{EventKindRow, EventRow, lane_events};
use core_model::document::Document;
use core_model::ids::{LaneId, SeqId};
use core_model::meter::MeterMap;
use core_model::time::{BarBeatUnit, Tick};

/// How a lane's events group into blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum BlockRule {
    /// A new block starts after at least `ticks` (minimum 1) of silence.
    SilenceGap {
        /// Gap size in ticks.
        ticks: i64,
    },
    /// Blocks of `bars` bars, counted from bar 1.
    FixedBars {
        /// Bars per block (minimum 1).
        bars: u32,
    },
    /// One block for all events.
    WholeLane,
}

/// What a block holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum BlockKind {
    /// Ordinary events (and audio regions).
    Events,
    /// One reference event.
    Reference,
}

/// One arrangement block.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct BlockRow {
    /// First tick.
    pub start: i64,
    /// End tick (exclusive; equal to `start` for zero-length content).
    pub end: i64,
    /// Kind.
    pub kind: BlockKind,
    /// The events in the block, in time order.
    pub events: Vec<u64>,
    /// The referenced sequence's name, for reference blocks.
    pub label: Option<String>,
    /// The referenced sequence, for reference blocks.
    pub ref_seq: Option<u64>,
    /// Every event in the block is muted.
    pub muted: bool,
}

struct Group {
    start: i64,
    end: i64,
    events: Vec<u64>,
    muted: bool,
    /// The bar span `[start, end)` of a fixed-bars group.
    bars: Option<(i64, i64)>,
}

impl Group {
    fn finish(self) -> BlockRow {
        let (start, end) = self.bars.unwrap_or((self.start, self.end));
        BlockRow {
            start,
            end,
            kind: BlockKind::Events,
            events: self.events,
            label: None,
            ref_seq: None,
            muted: self.muted,
        }
    }
}

/// The span `[start, end)` of the `bars`-bar group holding `tick`.
fn bar_span(meter: &MeterMap, tick: i64, bars: u32) -> (i64, i64) {
    let n = i32::try_from(bars.max(1)).unwrap_or(i32::MAX);
    let bar = meter.tick_to_bbu(Tick(tick), 1).bar;
    let first = (bar - 1).div_euclid(n).saturating_mul(n).saturating_add(1);
    let at = |bar: i32| {
        meter
            .bbu_to_tick(
                BarBeatUnit {
                    bar,
                    beat: 1,
                    unit: 0,
                },
                1,
            )
            .0
    };
    (at(first), at(first.saturating_add(n)))
}

fn reference_block(doc: &Document, r: &EventRow) -> BlockRow {
    let label = r
        .ref_seq
        .and_then(|s| doc.sequence(SeqId(s)).ok())
        .map_or_else(|| "Missing sequence".to_string(), |s| s.name.clone());
    BlockRow {
        start: r.tick,
        end: r.tick.saturating_add(r.duration.max(0)),
        kind: BlockKind::Reference,
        events: vec![r.id],
        label: Some(label),
        ref_seq: r.ref_seq,
        muted: r.mute,
    }
}

/// The blocks of `lane` under `rule`, sorted by start, then end.
pub fn blocks(
    doc: &Document,
    seq: SeqId,
    lane: LaneId,
    rule: BlockRule,
) -> Result<Vec<BlockRow>, VizError> {
    let meter = &doc.sequence(seq)?.meter;
    let rows = lane_events(doc, seq, lane, 0)?.events;
    let mut out = Vec::new();
    let mut group: Option<Group> = None;
    for r in &rows {
        if r.kind == EventKindRow::Reference {
            out.push(reference_block(doc, r));
            continue;
        }
        let end = r.tick.saturating_add(r.duration.max(0));
        let bars = match rule {
            BlockRule::FixedBars { bars } => Some(bar_span(meter, r.tick, bars)),
            _ => None,
        };
        let joins = match (&group, rule) {
            (None, _) => false,
            (Some(g), BlockRule::SilenceGap { ticks }) => {
                r.tick.saturating_sub(g.end) < ticks.max(1)
            }
            (Some(g), BlockRule::FixedBars { .. }) => g.bars == bars,
            (Some(_), BlockRule::WholeLane) => true,
        };
        match group.as_mut() {
            Some(g) if joins => {
                g.end = g.end.max(end);
                g.events.push(r.id);
                g.muted &= r.mute;
            }
            _ => {
                if let Some(g) = group.take() {
                    out.push(g.finish());
                }
                group = Some(Group {
                    start: r.tick,
                    end,
                    events: vec![r.id],
                    muted: r.mute,
                    bars,
                });
            }
        }
    }
    if let Some(g) = group {
        out.push(g.finish());
    }
    out.sort_by_key(|b| (b.start, b.end));
    Ok(out)
}
