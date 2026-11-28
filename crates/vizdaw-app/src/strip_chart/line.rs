//! Strip Chart strokes as lines and editor commands (Swift
//! `StripLineBuilder` and `StripCommands`, `M/StripChart.swift:149-232`;
//! Plan 4d Rulings 15, 17, 22).

use std::collections::BTreeMap;

use vizdaw_ffi::edit::EditCommand;
use vizdaw_ffi::modify::{ConditionRow, ConditionTest, ModifyRow, RuleRow, SelectModeRow};
use vizdaw_ffi::strip::{LineOpRow, LinePointRow, LineRow, LineShapeRow};

use super::lane::{StripLane, after};
use super::scale::StripMode;
use super::settings::{ShapeKind, StripChartSettings};
use crate::select_modify::line_title;

/// A pointer position in a row: a tick and a value on the row's scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StripPoint {
    pub tick: i64,
    pub value: i64,
}

/// A free line's points: ascending ticks, the later point winning on a
/// tick (`M/StripChart.swift:160-166`).
pub fn free_points(stroke: &[StripPoint]) -> Vec<LinePointRow> {
    let by_tick: BTreeMap<i64, i64> = stroke.iter().map(|p| (p.tick, p.value)).collect();
    by_tick
        .into_iter()
        .map(|(tick, value)| LinePointRow { tick, value })
        .collect()
}

/// The line a drag from `a` to `b` draws (`M/StripChart.swift:168-187`):
/// its ends in time order (a right-to-left drag is turned around), Flat
/// holding `a`'s value, a Free line through `stroke` (`[a, b]` when it is
/// empty).
pub fn stroke_line(
    a: StripPoint,
    b: StripPoint,
    stroke: &[StripPoint],
    s: &StripChartSettings,
    seed: u64,
) -> LineRow {
    let step = s.step.max(1);
    if s.shape == ShapeKind::Free {
        let points = if stroke.is_empty() {
            free_points(&[a, b])
        } else {
            free_points(stroke)
        };
        if let (Some(f), Some(l)) = (points.first().copied(), points.last().copied()) {
            return LineRow {
                from: f.tick,
                to: l.tick,
                start: f.value,
                end: l.value,
                step,
                shape: LineShapeRow::Free { points },
            };
        }
    }
    let (lo, hi) = if a.tick <= b.tick { (a, b) } else { (b, a) };
    if s.shape == ShapeKind::Flat {
        return LineRow {
            from: lo.tick,
            to: hi.tick,
            start: a.value,
            end: a.value,
            step,
            shape: LineShapeRow::Flat,
        };
    }
    LineRow {
        from: lo.tick,
        to: hi.tick,
        start: lo.value,
        end: hi.value,
        step,
        shape: s.shape_row(seed),
    }
}

/// The operation of a Draw or Modify line: Draw writes a series and sets a
/// note value; Modify uses the bar's operation
/// (`M/StripChart.swift:191-196`).
pub fn stroke_op(lane: StripLane, s: &StripChartSettings) -> LineOpRow {
    if s.mode != StripMode::Modify {
        return if lane.is_note_value() {
            LineOpRow::Set
        } else {
            LineOpRow::Draw
        };
    }
    s.op.row()
}

/// A line over `[from, to]` of the shown track; `selected_only` (⌥ at the
/// press) limits it to the selected events (`M/StripChart.swift:198-209`).
pub fn line_command(
    lane: StripLane,
    op: LineOpRow,
    line: LineRow,
    selected_only: bool,
) -> EditCommand {
    EditCommand::SelectAndModify {
        rule: Some(RuleRow {
            conditions: vec![ConditionRow {
                test: ConditionTest::TimeRange {
                    start: line.from,
                    end: after(line.to),
                },
                negate: false,
            }],
            match_all: true,
        }),
        mode: if selected_only {
            SelectModeRow::Refine
        } else {
            SelectModeRow::Select
        },
        modify: Some(ModifyRow::Line {
            target: lane.line_target(),
            op,
            line,
        }),
        to_copy: false,
    }
}

/// A Compress rectangle: the values in its time range onto its bottom
/// (`start`) and top (`end`), ticks and values both ordered
/// (`M/StripChart.swift:211-220`).
pub fn compress_command(
    lane: StripLane,
    a: StripPoint,
    b: StripPoint,
    step: i64,
    selected_only: bool,
) -> EditCommand {
    let rect = LineRow {
        from: a.tick.min(b.tick),
        to: a.tick.max(b.tick),
        start: a.value.min(b.value),
        end: a.value.max(b.value),
        step: step.max(1),
        shape: LineShapeRow::Flat,
    };
    line_command(lane, LineOpRow::Compress, rect, selected_only)
}

/// The eraser over `[from, to]`: the row's series goes; `None` for note
/// values and tempo (`M/StripChart.swift:222-226`).
pub fn erase_command(lane: StripLane, from: i64, to: i64) -> Option<EditCommand> {
    lane.erase_rule(from, to)
        .map(|rule| EditCommand::SelectAndModify {
            rule: Some(rule),
            mode: SelectModeRow::Select,
            modify: Some(ModifyRow::Delete),
            to_copy: false,
        })
}

/// The undo step's name (the core's, `M/StripChart.swift:228-231`).
pub fn stroke_label(lane: StripLane, op: LineOpRow) -> &'static str {
    line_title(&lane.line_target(), op)
}
