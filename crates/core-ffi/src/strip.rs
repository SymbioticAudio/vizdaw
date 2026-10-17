//! Strip Chart lines as FFI records (Plan 4d Rulings 15–20, 25): the parts
//! of `ModifyRow::Line`. Values are on the target's scale (velocities
//! 16-bit, lengths in ticks, everything else on the 32-bit scale).

use crate::events::{AutomationTargetRow, automation_target_of, automation_target_row};
use crate::modify::{
    CurveRow, ValueTargetRow, WaveRow, curve_of, curve_row, target_of, target_row, wave_of,
    wave_row,
};
use core_model::strip::{Line, LineChange, LineOp, LineShape, LineTarget, Period, Series};
use core_model::time::Tick;

/// A controller-like series (core `Series`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SeriesRow {
    /// Control change `index`.
    Controller {
        /// Controller number 0–127.
        index: u8,
    },
    /// Pitch bend.
    PitchBend,
    /// Channel pressure.
    ChannelPressure,
    /// Polyphonic pressure on one pitch.
    PolyPressure {
        /// Pitch 0–127.
        pitch: u8,
    },
    /// Program changes.
    Program,
    /// Automation of one target.
    Automation {
        /// The target.
        target: AutomationTargetRow,
    },
}

/// What a line acts on (core `LineTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum LineTargetRow {
    /// A value of the events in range.
    Value {
        /// Which value.
        target: ValueTargetRow,
    },
    /// One series of the events in range (drawing writes it).
    Series {
        /// The series.
        series: SeriesRow,
    },
}

/// What a line does (core `LineOp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum LineOpRow {
    /// Write the line.
    Draw,
    /// Set each value.
    Set,
    /// Scale each value (percent).
    Scale,
    /// Add to each value.
    Add,
    /// Keep each value at most the line's.
    LimitMax,
    /// Keep each value at least the line's.
    LimitMin,
    /// Map the range's lowest and highest onto `start` and `end`.
    Compress,
    /// Thin out to `step`.
    Thin,
    /// Thicken every `step`.
    Thicken,
    /// Lengths to the next note.
    Legato,
}

/// An oscillator's period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum PeriodRow {
    /// Musical: ticks.
    Ticks {
        /// Ticks (> 0).
        ticks: i64,
    },
    /// Clock: milliseconds.
    Millis {
        /// Milliseconds (> 0).
        ms: u32,
    },
    /// Bars that follow the meter.
    Bars {
        /// Bars (> 0).
        bars: u32,
    },
}

/// One point of a free line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct LinePointRow {
    /// Tick.
    pub tick: i64,
    /// Value.
    pub value: i64,
}

/// A line's shape (core `LineShape`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum LineShapeRow {
    /// Straight.
    Straight,
    /// Along a curve.
    Curved {
        /// The curve.
        curve: CurveRow,
    },
    /// The start value all along.
    Flat,
    /// Through drawn points (ascending ticks).
    Free {
        /// The points.
        points: Vec<LinePointRow>,
    },
    /// Random per tick between start and end.
    Random {
        /// Seed.
        seed: u64,
    },
    /// A wave from start (low) to end (high).
    Oscillator {
        /// Wave.
        wave: WaveRow,
        /// Period.
        period: PeriodRow,
        /// Phase in thousandths of a period (0–999).
        phase: u16,
    },
}

/// A Strip Chart line (core `Line`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct LineRow {
    /// First tick.
    pub from: i64,
    /// Last tick.
    pub to: i64,
    /// Value at `from`.
    pub start: i64,
    /// Value at `to`.
    pub end: i64,
    /// Sample spacing (and Thin/Thicken spacing), ticks > 0.
    pub step: i64,
    /// Shape.
    pub shape: LineShapeRow,
}

fn series_of(s: SeriesRow) -> Series {
    match s {
        SeriesRow::Controller { index } => Series::Controller(index),
        SeriesRow::PitchBend => Series::PitchBend,
        SeriesRow::ChannelPressure => Series::ChannelPressure,
        SeriesRow::PolyPressure { pitch } => Series::PolyPressure(pitch),
        SeriesRow::Program => Series::Program,
        SeriesRow::Automation { target } => Series::Automation(automation_target_of(target)),
    }
}

fn series_row(s: Series) -> SeriesRow {
    match s {
        Series::Controller(index) => SeriesRow::Controller { index },
        Series::PitchBend => SeriesRow::PitchBend,
        Series::ChannelPressure => SeriesRow::ChannelPressure,
        Series::PolyPressure(pitch) => SeriesRow::PolyPressure { pitch },
        Series::Program => SeriesRow::Program,
        Series::Automation(t) => SeriesRow::Automation {
            target: automation_target_row(t),
        },
    }
}

fn line_target_of(t: LineTargetRow) -> LineTarget {
    match t {
        LineTargetRow::Value { target } => LineTarget::Value(target_of(target)),
        LineTargetRow::Series { series } => LineTarget::Series(series_of(series)),
    }
}

fn line_target_row(t: LineTarget) -> LineTargetRow {
    match t {
        LineTarget::Value(v) => LineTargetRow::Value {
            target: target_row(v),
        },
        LineTarget::Series(s) => LineTargetRow::Series {
            series: series_row(s),
        },
    }
}

fn op_of(o: LineOpRow) -> LineOp {
    match o {
        LineOpRow::Draw => LineOp::Draw,
        LineOpRow::Set => LineOp::Set,
        LineOpRow::Scale => LineOp::Scale,
        LineOpRow::Add => LineOp::Add,
        LineOpRow::LimitMax => LineOp::LimitMax,
        LineOpRow::LimitMin => LineOp::LimitMin,
        LineOpRow::Compress => LineOp::Compress,
        LineOpRow::Thin => LineOp::Thin,
        LineOpRow::Thicken => LineOp::Thicken,
        LineOpRow::Legato => LineOp::Legato,
    }
}

fn op_row(o: LineOp) -> LineOpRow {
    match o {
        LineOp::Draw => LineOpRow::Draw,
        LineOp::Set => LineOpRow::Set,
        LineOp::Scale => LineOpRow::Scale,
        LineOp::Add => LineOpRow::Add,
        LineOp::LimitMax => LineOpRow::LimitMax,
        LineOp::LimitMin => LineOpRow::LimitMin,
        LineOp::Compress => LineOpRow::Compress,
        LineOp::Thin => LineOpRow::Thin,
        LineOp::Thicken => LineOpRow::Thicken,
        LineOp::Legato => LineOpRow::Legato,
    }
}

fn shape_of(s: &LineShapeRow) -> LineShape {
    match s {
        LineShapeRow::Straight => LineShape::Straight,
        LineShapeRow::Curved { curve } => LineShape::Curved {
            curve: curve_of(*curve),
        },
        LineShapeRow::Flat => LineShape::Flat,
        LineShapeRow::Free { points } => LineShape::Free {
            points: points.iter().map(|p| (Tick(p.tick), p.value)).collect(),
        },
        LineShapeRow::Random { seed } => LineShape::Random { seed: *seed },
        LineShapeRow::Oscillator {
            wave,
            period,
            phase,
        } => LineShape::Oscillator {
            wave: wave_of(*wave),
            period: match period {
                PeriodRow::Ticks { ticks } => Period::Ticks(Tick(*ticks)),
                PeriodRow::Millis { ms } => Period::Millis(*ms),
                PeriodRow::Bars { bars } => Period::Bars(*bars),
            },
            phase: *phase,
        },
    }
}

fn shape_row(s: &LineShape) -> LineShapeRow {
    match s {
        LineShape::Straight => LineShapeRow::Straight,
        LineShape::Curved { curve } => LineShapeRow::Curved {
            curve: curve_row(*curve),
        },
        LineShape::Flat => LineShapeRow::Flat,
        LineShape::Free { points } => LineShapeRow::Free {
            points: points
                .iter()
                .map(|(t, v)| LinePointRow {
                    tick: t.0,
                    value: *v,
                })
                .collect(),
        },
        LineShape::Random { seed } => LineShapeRow::Random { seed: *seed },
        LineShape::Oscillator {
            wave,
            period,
            phase,
        } => LineShapeRow::Oscillator {
            wave: wave_row(*wave),
            period: match period {
                Period::Ticks(t) => PeriodRow::Ticks { ticks: t.0 },
                Period::Millis(ms) => PeriodRow::Millis { ms: *ms },
                Period::Bars(b) => PeriodRow::Bars { bars: *b },
            },
            phase: *phase,
        },
    }
}

/// The core change of a `ModifyRow::Line` (checked when it runs).
pub fn line_change(target: LineTargetRow, op: LineOpRow, line: &LineRow) -> LineChange {
    LineChange {
        target: line_target_of(target),
        op: op_of(op),
        line: Line {
            from: Tick(line.from),
            to: Tick(line.to),
            start: line.start,
            end: line.end,
            step: Tick(line.step),
            shape: shape_of(&line.shape),
        },
    }
}

/// The rows of a core change.
pub fn line_change_row(c: &LineChange) -> (LineTargetRow, LineOpRow, LineRow) {
    (
        line_target_row(c.target),
        op_row(c.op),
        LineRow {
            from: c.line.from.0,
            to: c.line.to.0,
            start: c.line.start,
            end: c.line.end,
            step: c.line.step.0,
            shape: shape_row(&c.line.shape),
        },
    )
}
