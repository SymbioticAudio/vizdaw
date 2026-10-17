//! Select-and-modify (parent spec §9, plan 4c): rules, modifications and
//! templates as flat records. A rule is a list of conditions joined by
//! "all" or "any", each optionally negated (UniFFI enums cannot hold
//! themselves, Plan Ruling 4).

use crate::error::VizError;
use crate::events::{EventKindRow, bad, kind_of};
use core_model::command::SettingsCmd;
use core_model::document::Document;
use core_model::event::EventKind;
use core_model::ids::DestId;
use core_model::modify::{
    Amount, ChangeOp, Curve, Density, DurationOp, Modify, ModifyTemplate, Quantize, Substitute,
    Transpose, ValueOp, ValueTarget, Wave,
};
use core_model::rule::{Rule, SelectMode};
use core_model::time::Tick;

/// One test of a selection rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ConditionTest {
    /// Start in `[start, end)`.
    TimeRange {
        /// First tick.
        start: i64,
        /// End (exclusive, after `start`).
        end: i64,
    },
    /// Kind is one of these.
    Kinds {
        /// Accepted kinds.
        kinds: Vec<EventKindRow>,
    },
    /// Note or poly-pressure pitch in range.
    Pitch {
        /// Lowest.
        low: u8,
        /// Highest.
        high: u8,
    },
    /// Note velocity (16-bit) in range.
    Velocity {
        /// Lowest.
        low: u16,
        /// Highest.
        high: u16,
    },
    /// Note release velocity (16-bit) in range.
    ReleaseVelocity {
        /// Lowest.
        low: u16,
        /// Highest.
        high: u16,
    },
    /// Note length in range.
    Duration {
        /// Shortest.
        min: i64,
        /// Longest.
        max: i64,
    },
    /// CC, pitch-bend and pressure value, or program number, in range.
    Value {
        /// Lowest.
        low: u32,
        /// Highest.
        high: u32,
    },
    /// CC controller number in range.
    Controller {
        /// Lowest.
        low: u8,
        /// Highest.
        high: u8,
    },
    /// Resolved destination (None: unresolved).
    Dest {
        /// Destination.
        dest: Option<u64>,
    },
    /// Within `tolerance` of a grid line.
    Metric {
        /// Grid (> 0).
        grid: i64,
        /// Grid offset.
        offset: i64,
        /// Allowed distance.
        tolerance: i64,
    },
    /// Every `n`th candidate from `phase`.
    EveryNth {
        /// Period (>= 1).
        n: u32,
        /// First match.
        phase: u32,
    },
    /// Starts inside a note of this pitch range in its lane.
    BracketedBy {
        /// Lowest pitch.
        low: u8,
        /// Highest pitch.
        high: u8,
    },
}

/// A condition, optionally negated ("not equal", "outside").
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ConditionRow {
    /// The test.
    pub test: ConditionTest,
    /// Match when the test fails.
    pub negate: bool,
}

/// A selection rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct RuleRow {
    /// The conditions.
    pub conditions: Vec<ConditionRow>,
    /// All must match (else any).
    pub match_all: bool,
}

/// How a rule's matches combine with the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SelectModeRow {
    /// Replace the selected events.
    Select,
    /// Add to them.
    Add,
    /// Keep the selected ones that match.
    Refine,
}

/// Which value a change edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ValueTargetRow {
    /// Note velocity.
    Velocity,
    /// Note release velocity.
    ReleaseVelocity,
    /// Note length.
    Duration,
    /// CC, pitch-bend and pressure values.
    Value,
}

/// Line shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum CurveRow {
    /// Straight.
    Straight,
    /// Slow, then fast.
    EaseIn,
    /// Fast, then slow.
    EaseOut,
    /// Slow at both ends.
    SCurve,
}

/// Oscillator waves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum WaveRow {
    /// Sine.
    Sine,
    /// Square.
    Square,
    /// Triangle.
    Triangle,
    /// Saw.
    Saw,
}

/// An amount (Plan Ruling 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AmountRow {
    /// The same for every event.
    Constant {
        /// Amount.
        value: i64,
    },
    /// Along a line from the first to the last event.
    Line {
        /// At the first.
        from: i64,
        /// At the last.
        to: i64,
        /// Shape.
        curve: CurveRow,
    },
    /// A wave.
    Oscillator {
        /// Wave.
        wave: WaveRow,
        /// Lowest.
        low: i64,
        /// Highest.
        high: i64,
        /// Period in ticks (> 0).
        period: i64,
        /// Shift in ticks.
        phase: i64,
    },
    /// Random per event, fixed by the seed.
    Random {
        /// Lowest.
        low: i64,
        /// Highest.
        high: i64,
        /// Seed.
        seed: u64,
    },
}

/// How a change combines its amount with each value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ChangeOpRow {
    /// The amount.
    Set,
    /// Times the amount in percent.
    Scale,
    /// Plus the amount.
    Add,
    /// Kept inside `min..=max`.
    Limit {
        /// Lowest.
        min: i64,
        /// Highest.
        max: i64,
    },
    /// The range mapped onto `low..=high`.
    Compress {
        /// New lowest.
        low: i64,
        /// New highest.
        high: i64,
    },
    /// To the next note (lengths only).
    Legato,
}

/// One pitch of a note map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct NoteMapEntry {
    /// Pitch replaced.
    pub from: u8,
    /// Its replacement.
    pub to: u8,
}

/// What Substitute turns events into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SubstituteRow {
    /// CC `index`.
    Controller {
        /// Controller number.
        index: u8,
    },
    /// Pitch bend.
    PitchBend,
    /// Channel pressure.
    ChannelPressure,
    /// One pitch for notes.
    Pitch {
        /// The pitch.
        pitch: u8,
    },
}

/// A modification (parent spec §9 modify half).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ModifyRow {
    /// By semitones.
    TransposeInterval {
        /// Semitones.
        semitones: i8,
    },
    /// By scale degrees.
    TransposeDiatonic {
        /// Degrees.
        steps: i8,
        /// Root 0–11.
        root: u8,
        /// Scale mask.
        mask: u16,
    },
    /// Snapped to a scale.
    TransposeToScale {
        /// Root 0–11.
        root: u8,
        /// Scale mask.
        mask: u16,
    },
    /// Mirrored around a pitch.
    TransposeInvert {
        /// The pitch that stays.
        axis: u8,
    },
    /// By a note map.
    NoteMap {
        /// The map.
        map: Vec<NoteMapEntry>,
    },
    /// Quantize to a grid (no offsets) or a groove.
    Quantize {
        /// Grid in ticks.
        grid: i64,
        /// Strength 0–100.
        strength: u8,
        /// Groove offsets per grid line.
        offsets: Vec<i64>,
    },
    /// Change a value.
    Change {
        /// Which value.
        target: ValueTargetRow,
        /// How.
        op: ChangeOpRow,
        /// Amount.
        amount: AmountRow,
    },
    /// Move in time.
    Move {
        /// Offset.
        delta: i64,
    },
    /// Scale time around an anchor.
    ScaleTime {
        /// Numerator.
        num: u32,
        /// Denominator.
        den: u32,
        /// Fixed point.
        anchor: i64,
    },
    /// Move note starts, keeping ends.
    TrimStart {
        /// Offset.
        delta: i64,
    },
    /// Set the destination override.
    SetDest {
        /// Destination (None: the lane's).
        dest: Option<u64>,
    },
    /// Substitute kinds or pitches.
    Substitute {
        /// Into what.
        to: SubstituteRow,
    },
    /// Thin out controllers.
    Thin {
        /// Minimum distance.
        spacing: i64,
    },
    /// Thicken controllers.
    Thicken {
        /// Distance between added events.
        spacing: i64,
    },
    /// Delete.
    Delete,
}

/// A named configuration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ModifyTemplateRow {
    /// Name (not blank).
    pub name: String,
    /// Selection rule.
    pub rule: Option<RuleRow>,
    /// Selection mode.
    pub mode: SelectModeRow,
    /// Modification.
    pub modify: Option<ModifyRow>,
    /// Modify copies.
    pub to_copy: bool,
}

/// Document-settings edits (templates).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SettingsEdit {
    /// Store a template under its name (replacing one of that name).
    SaveTemplate {
        /// The template.
        template: ModifyTemplateRow,
    },
    /// Remove a template.
    DeleteTemplate {
        /// Its name.
        name: String,
    },
}

fn kind_row(k: EventKind) -> EventKindRow {
    match k {
        EventKind::Note => EventKindRow::Note,
        EventKind::Cc => EventKindRow::Cc,
        EventKind::PitchBend => EventKindRow::PitchBend,
        EventKind::ChannelPressure => EventKindRow::ChannelPressure,
        EventKind::PolyPressure => EventKindRow::PolyPressure,
        EventKind::Program => EventKindRow::Program,
        EventKind::SysEx => EventKindRow::SysEx,
        EventKind::Text => EventKindRow::Text,
        EventKind::Reference => EventKindRow::Reference,
        EventKind::AudioRegion => EventKindRow::AudioRegion,
    }
}

/// The core mode.
pub fn mode_of(m: SelectModeRow) -> SelectMode {
    match m {
        SelectModeRow::Select => SelectMode::Select,
        SelectModeRow::Add => SelectMode::Add,
        SelectModeRow::Refine => SelectMode::Refine,
    }
}

fn mode_row(m: SelectMode) -> SelectModeRow {
    match m {
        SelectMode::Select => SelectModeRow::Select,
        SelectMode::Add => SelectModeRow::Add,
        SelectMode::Refine => SelectModeRow::Refine,
    }
}

fn leaf(t: &ConditionTest) -> Result<Rule, VizError> {
    Ok(match t {
        ConditionTest::TimeRange { start, end } => {
            if end <= start {
                return Err(bad("the time range is empty"));
            }
            Rule::TimeRange {
                start: Tick(*start),
                end: Tick(*end),
            }
        }
        ConditionTest::Kinds { kinds } => Rule::Kind {
            kinds: kinds.iter().map(|k| kind_of(*k)).collect(),
        },
        ConditionTest::Pitch { low, high } => Rule::PitchRange {
            low: *low,
            high: *high,
        },
        ConditionTest::Velocity { low, high } => Rule::VelocityRange {
            low: *low,
            high: *high,
        },
        ConditionTest::ReleaseVelocity { low, high } => Rule::ReleaseVelocityRange {
            low: *low,
            high: *high,
        },
        ConditionTest::Duration { min, max } => Rule::DurationRange {
            min: Tick(*min),
            max: Tick(*max),
        },
        ConditionTest::Value { low, high } => Rule::ValueRange {
            low: *low,
            high: *high,
        },
        ConditionTest::Controller { low, high } => Rule::ControllerRange {
            low: *low,
            high: *high,
        },
        ConditionTest::Dest { dest } => Rule::Dest {
            dest: dest.map(DestId),
        },
        ConditionTest::Metric {
            grid,
            offset,
            tolerance,
        } => {
            if *grid <= 0 {
                return Err(bad("the metric grid must be longer than 0 ticks"));
            }
            Rule::MetricPosition {
                grid: Tick(*grid),
                offset: Tick(*offset),
                tolerance: Tick(*tolerance),
            }
        }
        ConditionTest::EveryNth { n, phase } => {
            if *n == 0 {
                return Err(bad("every nth needs n of 1 or more"));
            }
            Rule::EveryNth {
                n: *n,
                phase: *phase,
            }
        }
        ConditionTest::BracketedBy { low, high } => Rule::Bracketed {
            by: Box::new(Rule::PitchRange {
                low: *low,
                high: *high,
            }),
        },
    })
}

/// The core rule of a row.
pub fn rule(r: &RuleRow) -> Result<Rule, VizError> {
    let mut parts = Vec::with_capacity(r.conditions.len());
    for c in &r.conditions {
        let l = leaf(&c.test)?;
        parts.push(if c.negate { Rule::Not(Box::new(l)) } else { l });
    }
    Ok(if r.match_all {
        Rule::All(parts)
    } else {
        Rule::Any(parts)
    })
}

fn test_of(r: &Rule) -> Option<ConditionTest> {
    Some(match r {
        Rule::TimeRange { start, end } => ConditionTest::TimeRange {
            start: start.0,
            end: end.0,
        },
        Rule::Kind { kinds } => ConditionTest::Kinds {
            kinds: kinds.iter().map(|k| kind_row(*k)).collect(),
        },
        Rule::PitchRange { low, high } => ConditionTest::Pitch {
            low: *low,
            high: *high,
        },
        Rule::VelocityRange { low, high } => ConditionTest::Velocity {
            low: *low,
            high: *high,
        },
        Rule::ReleaseVelocityRange { low, high } => ConditionTest::ReleaseVelocity {
            low: *low,
            high: *high,
        },
        Rule::DurationRange { min, max } => ConditionTest::Duration {
            min: min.0,
            max: max.0,
        },
        Rule::ValueRange { low, high } => ConditionTest::Value {
            low: *low,
            high: *high,
        },
        Rule::ControllerRange { low, high } => ConditionTest::Controller {
            low: *low,
            high: *high,
        },
        Rule::Dest { dest } => ConditionTest::Dest {
            dest: dest.map(|d| d.0),
        },
        Rule::MetricPosition {
            grid,
            offset,
            tolerance,
        } => ConditionTest::Metric {
            grid: grid.0,
            offset: offset.0,
            tolerance: tolerance.0,
        },
        Rule::EveryNth { n, phase } => ConditionTest::EveryNth {
            n: *n,
            phase: *phase,
        },
        Rule::Bracketed { by } => match by.as_ref() {
            Rule::PitchRange { low, high } => ConditionTest::BracketedBy {
                low: *low,
                high: *high,
            },
            _ => return None,
        },
        Rule::All(_) | Rule::Any(_) | Rule::Not(_) => return None,
    })
}

fn condition_of(r: &Rule) -> Option<ConditionRow> {
    match r {
        Rule::Not(inner) => test_of(inner).map(|test| ConditionRow { test, negate: true }),
        other => test_of(other).map(|test| ConditionRow {
            test,
            negate: false,
        }),
    }
}

/// The row of a core rule; None when it is not a flat list of conditions.
pub fn rule_row(r: &Rule) -> Option<RuleRow> {
    let (parts, match_all) = match r {
        Rule::All(p) => (p.as_slice(), true),
        Rule::Any(p) => (p.as_slice(), false),
        other => (std::slice::from_ref(other), true),
    };
    let conditions = parts.iter().map(condition_of).collect::<Option<Vec<_>>>()?;
    Some(RuleRow {
        conditions,
        match_all,
    })
}

fn target_of(t: ValueTargetRow) -> ValueTarget {
    match t {
        ValueTargetRow::Velocity => ValueTarget::Velocity,
        ValueTargetRow::ReleaseVelocity => ValueTarget::ReleaseVelocity,
        ValueTargetRow::Duration => ValueTarget::Duration,
        ValueTargetRow::Value => ValueTarget::Value,
    }
}

fn target_row(t: ValueTarget) -> ValueTargetRow {
    match t {
        ValueTarget::Velocity => ValueTargetRow::Velocity,
        ValueTarget::ReleaseVelocity => ValueTargetRow::ReleaseVelocity,
        ValueTarget::Duration => ValueTargetRow::Duration,
        ValueTarget::Value => ValueTargetRow::Value,
    }
}

fn op_of(o: ChangeOpRow) -> ChangeOp {
    match o {
        ChangeOpRow::Set => ChangeOp::Set,
        ChangeOpRow::Scale => ChangeOp::Scale,
        ChangeOpRow::Add => ChangeOp::Add,
        ChangeOpRow::Limit { min, max } => ChangeOp::Limit { min, max },
        ChangeOpRow::Compress { low, high } => ChangeOp::Compress { low, high },
        ChangeOpRow::Legato => ChangeOp::Legato,
    }
}

fn op_row(o: ChangeOp) -> ChangeOpRow {
    match o {
        ChangeOp::Set => ChangeOpRow::Set,
        ChangeOp::Scale => ChangeOpRow::Scale,
        ChangeOp::Add => ChangeOpRow::Add,
        ChangeOp::Limit { min, max } => ChangeOpRow::Limit { min, max },
        ChangeOp::Compress { low, high } => ChangeOpRow::Compress { low, high },
        ChangeOp::Legato => ChangeOpRow::Legato,
    }
}

fn curve_of(c: CurveRow) -> Curve {
    match c {
        CurveRow::Straight => Curve::Straight,
        CurveRow::EaseIn => Curve::EaseIn,
        CurveRow::EaseOut => Curve::EaseOut,
        CurveRow::SCurve => Curve::SCurve,
    }
}

fn curve_row(c: Curve) -> CurveRow {
    match c {
        Curve::Straight => CurveRow::Straight,
        Curve::EaseIn => CurveRow::EaseIn,
        Curve::EaseOut => CurveRow::EaseOut,
        Curve::SCurve => CurveRow::SCurve,
    }
}

fn wave_of(w: WaveRow) -> Wave {
    match w {
        WaveRow::Sine => Wave::Sine,
        WaveRow::Square => Wave::Square,
        WaveRow::Triangle => Wave::Triangle,
        WaveRow::Saw => Wave::Saw,
    }
}

fn wave_row(w: Wave) -> WaveRow {
    match w {
        Wave::Sine => WaveRow::Sine,
        Wave::Square => WaveRow::Square,
        Wave::Triangle => WaveRow::Triangle,
        Wave::Saw => WaveRow::Saw,
    }
}

fn amount_of(a: AmountRow) -> Amount {
    match a {
        AmountRow::Constant { value } => Amount::Constant { value },
        AmountRow::Line { from, to, curve } => Amount::Line {
            from,
            to,
            curve: curve_of(curve),
        },
        AmountRow::Oscillator {
            wave,
            low,
            high,
            period,
            phase,
        } => Amount::Oscillator {
            wave: wave_of(wave),
            low,
            high,
            period: Tick(period),
            phase: Tick(phase),
        },
        AmountRow::Random { low, high, seed } => Amount::Random { low, high, seed },
    }
}

fn amount_row(a: Amount) -> AmountRow {
    match a {
        Amount::Constant { value } => AmountRow::Constant { value },
        Amount::Line { from, to, curve } => AmountRow::Line {
            from,
            to,
            curve: curve_row(curve),
        },
        Amount::Oscillator {
            wave,
            low,
            high,
            period,
            phase,
        } => AmountRow::Oscillator {
            wave: wave_row(wave),
            low,
            high,
            period: period.0,
            phase: phase.0,
        },
        Amount::Random { low, high, seed } => AmountRow::Random { low, high, seed },
    }
}

fn subst_of(s: SubstituteRow) -> Substitute {
    match s {
        SubstituteRow::Controller { index } => Substitute::Controller { index },
        SubstituteRow::PitchBend => Substitute::PitchBend,
        SubstituteRow::ChannelPressure => Substitute::ChannelPressure,
        SubstituteRow::Pitch { pitch } => Substitute::Pitch { pitch },
    }
}

fn subst_row(s: Substitute) -> SubstituteRow {
    match s {
        Substitute::Controller { index } => SubstituteRow::Controller { index },
        Substitute::PitchBend => SubstituteRow::PitchBend,
        Substitute::ChannelPressure => SubstituteRow::ChannelPressure,
        Substitute::Pitch { pitch } => SubstituteRow::Pitch { pitch },
    }
}

/// The core modification of a row (values are checked when it runs).
pub fn modify(m: &ModifyRow) -> Result<Modify, VizError> {
    Ok(match m {
        ModifyRow::TransposeInterval { semitones } => Modify::Transpose(Transpose::Interval {
            semitones: *semitones,
        }),
        ModifyRow::TransposeDiatonic { steps, root, mask } => {
            Modify::Transpose(Transpose::Diatonic {
                steps: *steps,
                root: *root,
                mask: *mask,
            })
        }
        ModifyRow::TransposeToScale { root, mask } => Modify::Transpose(Transpose::ToScale {
            root: *root,
            mask: *mask,
        }),
        ModifyRow::TransposeInvert { axis } => Modify::Transpose(Transpose::Invert { axis: *axis }),
        ModifyRow::NoteMap { map } => Modify::NoteMap {
            map: map.iter().map(|e| (e.from, e.to)).collect(),
        },
        ModifyRow::Quantize {
            grid,
            strength,
            offsets,
        } => Modify::Quantize(if offsets.is_empty() {
            Quantize::Grid {
                grid: Tick(*grid),
                strength: *strength,
            }
        } else {
            Quantize::Groove {
                grid: Tick(*grid),
                offsets: offsets.iter().copied().map(Tick).collect(),
                strength: *strength,
            }
        }),
        ModifyRow::Change { target, op, amount } => Modify::Change {
            target: target_of(*target),
            op: op_of(*op),
            amount: amount_of(*amount),
        },
        ModifyRow::Move { delta } => Modify::Move {
            delta: Tick(*delta),
        },
        ModifyRow::ScaleTime { num, den, anchor } => Modify::ScaleTime {
            num: *num,
            den: *den,
            anchor: Tick(*anchor),
        },
        ModifyRow::TrimStart { delta } => Modify::TrimStart {
            delta: Tick(*delta),
        },
        ModifyRow::SetDest { dest } => Modify::ReassignDest {
            dest: dest.map(DestId),
        },
        ModifyRow::Substitute { to } => Modify::Substitute(subst_of(*to)),
        ModifyRow::Thin { spacing } => Modify::Density(Density::Thin {
            spacing: Tick(*spacing),
        }),
        ModifyRow::Thicken { spacing } => Modify::Density(Density::Thicken {
            spacing: Tick(*spacing),
        }),
        ModifyRow::Delete => Modify::Delete,
    })
}

fn constant(target: ValueTargetRow, op: ChangeOpRow, value: i64) -> ModifyRow {
    ModifyRow::Change {
        target,
        op,
        amount: AmountRow::Constant { value },
    }
}

fn legacy(target: ValueTargetRow, op: ValueOp) -> ModifyRow {
    match op {
        ValueOp::Set(v) => constant(target, ChangeOpRow::Set, i64::from(v)),
        ValueOp::ScalePercent(p) => constant(target, ChangeOpRow::Scale, i64::from(p)),
        ValueOp::Add(a) => constant(target, ChangeOpRow::Add, a),
    }
}

/// The row of a core modification; the legacy value variants read as
/// `Change` rows (Plan Ruling 2).
pub fn modify_row(m: &Modify) -> ModifyRow {
    match m {
        Modify::Transpose(Transpose::Interval { semitones }) => ModifyRow::TransposeInterval {
            semitones: *semitones,
        },
        Modify::Transpose(Transpose::ToScale { root, mask }) => ModifyRow::TransposeToScale {
            root: *root,
            mask: *mask,
        },
        Modify::Transpose(Transpose::Diatonic { steps, root, mask }) => {
            ModifyRow::TransposeDiatonic {
                steps: *steps,
                root: *root,
                mask: *mask,
            }
        }
        Modify::Transpose(Transpose::Invert { axis }) => ModifyRow::TransposeInvert { axis: *axis },
        Modify::NoteMap { map } => ModifyRow::NoteMap {
            map: map
                .iter()
                .map(|(from, to)| NoteMapEntry {
                    from: *from,
                    to: *to,
                })
                .collect(),
        },
        Modify::Quantize(Quantize::Grid { grid, strength }) => ModifyRow::Quantize {
            grid: grid.0,
            strength: *strength,
            offsets: Vec::new(),
        },
        Modify::Quantize(Quantize::Groove {
            grid,
            offsets,
            strength,
        }) => ModifyRow::Quantize {
            grid: grid.0,
            strength: *strength,
            offsets: offsets.iter().map(|o| o.0).collect(),
        },
        Modify::Velocity(op) => legacy(ValueTargetRow::Velocity, *op),
        Modify::ControllerValue(op) => legacy(ValueTargetRow::Value, *op),
        Modify::Duration(DurationOp::Set(t)) => {
            constant(ValueTargetRow::Duration, ChangeOpRow::Set, t.0)
        }
        Modify::Duration(DurationOp::ScalePercent(p)) => {
            constant(ValueTargetRow::Duration, ChangeOpRow::Scale, i64::from(*p))
        }
        Modify::Legato => constant(ValueTargetRow::Duration, ChangeOpRow::Legato, 0),
        Modify::Change { target, op, amount } => ModifyRow::Change {
            target: target_row(*target),
            op: op_row(*op),
            amount: amount_row(*amount),
        },
        Modify::ReassignDest { dest } => ModifyRow::SetDest {
            dest: dest.map(|d| d.0),
        },
        Modify::ScaleTime { num, den, anchor } => ModifyRow::ScaleTime {
            num: *num,
            den: *den,
            anchor: anchor.0,
        },
        Modify::Move { delta } => ModifyRow::Move { delta: delta.0 },
        Modify::TrimStart { delta } => ModifyRow::TrimStart { delta: delta.0 },
        Modify::Substitute(s) => ModifyRow::Substitute { to: subst_row(*s) },
        Modify::Density(Density::Thin { spacing }) => ModifyRow::Thin { spacing: spacing.0 },
        Modify::Density(Density::Thicken { spacing }) => ModifyRow::Thicken { spacing: spacing.0 },
        Modify::Delete => ModifyRow::Delete,
    }
}

/// The core template of a row; a blank name is refused.
pub fn template(t: &ModifyTemplateRow) -> Result<ModifyTemplate, VizError> {
    if t.name.trim().is_empty() {
        return Err(bad("a template needs a name"));
    }
    Ok(ModifyTemplate {
        rule: t.rule.as_ref().map(rule).transpose()?,
        mode: mode_of(t.mode),
        modify: t.modify.as_ref().map(modify).transpose()?,
        to_copy: t.to_copy,
    })
}

/// The row of a stored template; None when its rule is not flat.
pub fn template_row(name: &str, t: &ModifyTemplate) -> Option<ModifyTemplateRow> {
    let rule = match &t.rule {
        Some(r) => Some(rule_row(r)?),
        None => None,
    };
    Some(ModifyTemplateRow {
        name: name.to_string(),
        rule,
        mode: mode_row(t.mode),
        modify: t.modify.as_ref().map(modify_row),
        to_copy: t.to_copy,
    })
}

/// Every template of `doc` that has a row, by name.
pub fn template_rows(doc: &Document) -> Vec<ModifyTemplateRow> {
    doc.settings()
        .templates
        .iter()
        .filter_map(|(n, t)| template_row(n, t))
        .collect()
}

/// Why Save refuses to overwrite a stored template the panel can't show.
pub const HIDDEN_TEMPLATE_MESSAGE: &str =
    "A template with that name exists but can't be shown here.";

/// Refuses a Save that would silently overwrite a template of the same
/// name whose rule has no row (it is not listed, so the user can't see
/// what they would replace).
pub fn check_settings_edit(doc: &Document, e: &SettingsEdit) -> Result<(), VizError> {
    if let SettingsEdit::SaveTemplate { template: t } = e {
        let name = t.name.trim();
        if let Some(old) = doc.settings().templates.get(name) {
            if template_row(name, old).is_none() {
                return Err(bad(HIDDEN_TEMPLATE_MESSAGE));
            }
        }
    }
    Ok(())
}

/// The core settings command of a settings edit.
pub fn settings_cmd(e: &SettingsEdit) -> Result<SettingsCmd, VizError> {
    Ok(match e {
        SettingsEdit::SaveTemplate { template: t } => SettingsCmd::SetTemplate {
            name: t.name.trim().to_string(),
            template: Some(template(t)?),
        },
        SettingsEdit::DeleteTemplate { name } => SettingsCmd::SetTemplate {
            name: name.clone(),
            template: None,
        },
    })
}
