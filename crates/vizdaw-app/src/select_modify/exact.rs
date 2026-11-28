//! Exact Mode (Swift `M/ExactMode.swift`): a note or block drag's final
//! command as the Select and Modify panel, pre-filled with its amounts, and
//! a piano roll rubber band as a Select by Rule.

use std::collections::BTreeSet;

use vizdaw_ffi::edit::{EditCommand, EventEdit};
use vizdaw_ffi::events::EventKindRow;
use vizdaw_ffi::modify::{
    AmountRow, ChangeOpRow, ConditionRow, ConditionTest, ModifyRow, SelectModeRow, ValueTargetRow,
};

use super::model::{DroppedAxis, ModifyRequest};
use super::{ModifyConfig, PanelTask};
use crate::arrangement::PPQ;

/// Which gesture a drag came from; only note and block drags go exact
/// (`M/ExactMode.swift:6-11`). A Strip Chart stroke applies as usual
/// until its Exact Mode request is ported (slice 10c Task 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GestureOrigin {
    Notes,
    Blocks,
    RegionStrip,
    Expression,
    StripChart,
    #[default]
    Other,
}

/// The panel for a drag from `origin` over the `(seq, lane)` tracks
/// `lanes` that ended with `cmd`; `None` when that drag applies as usual
/// (region moves, pencil, eraser, expression strokes, cross-track drags).
/// `M/ExactMode.swift:54-105`.
pub fn exact_request(
    cmd: &EditCommand,
    lanes: &BTreeSet<(u64, u64)>,
    origin: GestureOrigin,
) -> Option<ModifyRequest> {
    let lanes: Vec<(u64, u64)> = lanes.iter().copied().collect();
    // The piano roll's scale-time drag acts on the selection.
    if origin == GestureOrigin::Notes
        && let EditCommand::SelectAndModify {
            rule,
            mode,
            modify: Some(op @ ModifyRow::ScaleTime { .. }),
            ..
        } = cmd
    {
        let mut c = ModifyConfig::preset(PanelTask::Change, None);
        c.modify.op = op.clone();
        if let Some(rule) = rule {
            c.select.enabled = true;
            c.select.mode = *mode;
            c.select.conditions = rule.conditions.clone();
        }
        return Some(ModifyRequest {
            task: PanelTask::Change,
            config: c,
            events: None,
            lanes,
            range: None,
            dropped: None,
        });
    }
    if !matches!(origin, GestureOrigin::Notes | GestureOrigin::Blocks) {
        return None;
    }
    let EditCommand::Event { edit } = cmd else {
        return None;
    };
    let make = |task: PanelTask, events: &[u64], op: ModifyRow, copy, dropped| {
        let mut c = ModifyConfig::preset(task, None);
        c.modify.op = op;
        c.modify.to_copy = copy;
        ModifyRequest {
            task,
            config: c,
            events: Some(events.to_vec()),
            lanes: lanes.clone(),
            range: None,
            dropped,
        }
    };
    // A drag in both axes keeps the stronger one: sixteenths moved
    // against semitones, time winning ties.
    let shifted = |events: &[u64], ticks: i64, semitones: i8, copy: bool| {
        let time_wins = semitones == 0
            || (ticks != 0
                && ticks.unsigned_abs() as f64 / (PPQ / 4) as f64
                    >= f64::from(semitones.unsigned_abs()));
        if time_wins {
            let dropped = (semitones != 0).then_some(DroppedAxis::Vertical);
            make(
                PanelTask::Move,
                events,
                ModifyRow::Move { delta: ticks },
                copy,
                dropped,
            )
        } else {
            let dropped = (ticks != 0).then_some(DroppedAxis::Horizontal);
            make(
                PanelTask::Transpose,
                events,
                ModifyRow::TransposeInterval { semitones },
                copy,
                dropped,
            )
        }
    };
    match edit {
        EventEdit::Move {
            events,
            delta,
            to_lane: None,
        } => Some(make(
            PanelTask::Move,
            events,
            ModifyRow::Move { delta: *delta },
            false,
            None,
        )),
        EventEdit::Duplicate {
            events,
            ticks,
            semitones,
        } => Some(shifted(events, *ticks, *semitones, true)),
        EventEdit::Nudge {
            events,
            ticks,
            semitones,
        } => Some(shifted(events, *ticks, *semitones, false)),
        EventEdit::Resize { events, delta } => Some(make(
            PanelTask::Change,
            events,
            ModifyRow::Change {
                target: ValueTargetRow::Duration,
                op: ChangeOpRow::Add,
                amount: AmountRow::Constant { value: *delta },
            },
            false,
            None,
        )),
        _ => None,
    }
}

/// A rubber band as Select by Rule: notes starting in `range` with pitches
/// in `pitches` (low, high), in the shown tracks `lanes`; `adding` (⇧)
/// adds to the selection (`M/ExactMode.swift:124-137`).
pub fn select_request(
    range: (i64, i64),
    pitches: (u8, u8),
    lanes: Vec<(u64, u64)>,
    adding: bool,
) -> ModifyRequest {
    let mut c = ModifyConfig::preset(PanelTask::SelectByRule, None);
    c.select.mode = if adding {
        SelectModeRow::Add
    } else {
        SelectModeRow::Select
    };
    let row = |test| ConditionRow {
        test,
        negate: false,
    };
    c.select.conditions = vec![
        row(ConditionTest::Kinds {
            kinds: vec![EventKindRow::Note],
        }),
        row(ConditionTest::TimeRange {
            start: range.0,
            end: range.1,
        }),
        row(ConditionTest::Pitch {
            low: pitches.0,
            high: pitches.1,
        }),
    ];
    ModifyRequest {
        task: PanelTask::SelectByRule,
        config: c,
        events: None,
        lanes,
        range: Some(range),
        dropped: None,
    }
}
