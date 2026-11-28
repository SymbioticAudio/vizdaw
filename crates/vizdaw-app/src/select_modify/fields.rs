//! The panel's numeric fields: which a condition or modification shows,
//! how each reads and parses its number, and the note map and groove
//! offset text (Swift `M/PanelFields.swift`).

use vizdaw_ffi::events::MeterEntryRow;
use vizdaw_ffi::modify::{
    AmountRow, ChangeOpRow, ConditionTest, ModifyRow, NoteMapEntry, SubstituteRow, ValueTargetRow,
};
use vizdaw_ffi::strip::{LineOpRow, LinePointRow, LineRow, LineShapeRow, LineTargetRow, PeriodRow};

use crate::arrangement::PPQ;
use crate::list::{Bbu, DurationText, PitchName, Value7, Velocity7, parse_position, trim_ws};
use crate::position::swift_int;

/// The Note Map field's placeholder (`S/ModifyPanel.swift:323`).
pub const NOTE_MAP_PLACEHOLDER: &str = "C2>D2, E2>F#2";

/// The Quantize groove field's placeholder (`S/ModifyPanel.swift:329`).
pub const GROOVE_LABEL: &str = "Groove offsets";

/// The Quantize groove field's help (`S/ModifyPanel.swift:333`).
pub const GROOVE_HELP: &str = "Groove offsets in ticks, one per grid line; empty for a plain grid";

/// Which end of a 7-bit step a typed value stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    /// A range's low end: the step's first 16- or 32-bit value.
    Low,
    /// A range's high end: the step's last value.
    High,
    /// A signed change: steps of 512 (or 2^25).
    Delta,
    /// One value: the MIDI 2.0 upscale.
    Exact,
}

/// How a panel field shows and reads its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    Count,
    /// A whole number limited to a range (semitones, controller numbers).
    Bounded(i64, i64),
    Percent,
    Pitch,
    /// A 16-bit velocity as 0-127.
    Velocity(Edge),
    /// A 32-bit value as 0-127.
    Value(Edge),
    /// Ticks as signed beats.units of a quarter note.
    Length,
    /// A tick as bar.beat.unit.
    Position,
    /// Milli-BPM as beats per minute ("120.00"; a change may be negative).
    Bpm,
}

/// One numeric field of a condition or modification; the label is also
/// its accessibility label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Field {
    pub label: &'static str,
    pub kind: FieldKind,
}

const fn field(label: &'static str, kind: FieldKind) -> Field {
    Field { label, kind }
}

/// `n` as the field shows it; positions in `meter` (`PanelFields.swift:47-60`).
pub fn field_text(n: i64, k: FieldKind, meter: &[MeterEntryRow]) -> String {
    match k {
        FieldKind::Count | FieldKind::Bounded(..) => n.to_string(),
        FieldKind::Percent => format!("{n}%"),
        FieldKind::Pitch => PitchName::name(n.clamp(0, 127) as u8),
        FieldKind::Velocity(Edge::Delta) => (n / 512).to_string(),
        FieldKind::Velocity(_) => (n >> 9).to_string(),
        FieldKind::Value(Edge::Delta) => (n / 33_554_432).to_string(),
        FieldKind::Value(_) => (n >> 25).to_string(),
        FieldKind::Length => {
            let m = n.checked_abs().unwrap_or(i64::MAX);
            format!(
                "{}{}",
                if n < 0 { "-" } else { "" },
                DurationText::text(m, PPQ)
            )
        }
        FieldKind::Position => Bbu::of(n, meter).text(),
        FieldKind::Bpm => format!("{:.2}", n as f64 / 1000.0),
    }
}

/// The number `s` means, or `None` when it is not one or is out of the
/// field's range; positions in `meter` (`PanelFields.swift:62-105`).
pub fn parse_field(s: &str, k: FieldKind, meter: &[MeterEntryRow]) -> Option<i64> {
    let t = trim_ws(s);
    let seven = |v: i64| (0..=127).contains(&v);
    match k {
        FieldKind::Count => swift_int(t).filter(|v| v.unsigned_abs() <= 1_000_000),
        FieldKind::Bounded(lo, hi) => swift_int(t).filter(|v| (lo..=hi).contains(v)),
        FieldKind::Percent => {
            swift_int(trim_ws(&t.replace('%', ""))).filter(|v| (0..=10_000).contains(v))
        }
        FieldKind::Pitch => PitchName::parse(t).map(i64::from),
        FieldKind::Velocity(e) => {
            let v = swift_int(t)?;
            match e {
                Edge::Delta => (-127..=127).contains(&v).then_some(v * 512),
                Edge::Low => seven(v).then_some(v << 9),
                Edge::High => seven(v).then_some((v << 9) | 0x1FF),
                Edge::Exact => seven(v).then(|| i64::from(Velocity7::up(v))),
            }
        }
        FieldKind::Value(e) => {
            let v = swift_int(t)?;
            match e {
                Edge::Delta => (-127..=127).contains(&v).then_some(v << 25),
                Edge::Low => seven(v).then_some(v << 25),
                Edge::High => seven(v).then_some((v << 25) | 0x1FF_FFFF),
                Edge::Exact => seven(v).then(|| i64::from(Value7::up(v))),
            }
        }
        FieldKind::Length => {
            let (negative, rest) = match t.strip_prefix('-') {
                Some(r) => (true, r),
                None => (false, t),
            };
            let d = DurationText::parse(rest, 0, PPQ)?;
            Some(if negative { -d } else { d })
        }
        FieldKind::Position => {
            let start = Bbu {
                bar: 1,
                beat: 1,
                unit: 0,
            };
            parse_position(t, start).map(|p| p.tick(meter))
        }
        FieldKind::Bpm => {
            let lower = t.to_lowercase().replace("bpm", "");
            let v: f64 = trim_ws(&lower).parse().ok()?;
            (v.is_finite() && v.abs() <= 999.0).then(|| (v * 1000.0).round() as i64)
        }
    }
}

/// A condition's fields (`PanelFields.swift:109-121`).
pub fn condition_fields(t: &ConditionTest) -> Vec<Field> {
    use FieldKind as K;
    match t {
        ConditionTest::TimeRange { .. } => {
            vec![field("from", K::Position), field("to", K::Position)]
        }
        ConditionTest::Pitch { .. } | ConditionTest::BracketedBy { .. } => {
            vec![field("from", K::Pitch), field("to", K::Pitch)]
        }
        ConditionTest::Velocity { .. } | ConditionTest::ReleaseVelocity { .. } => vec![
            field("from", K::Velocity(Edge::Low)),
            field("to", K::Velocity(Edge::High)),
        ],
        ConditionTest::Duration { .. } => vec![field("from", K::Length), field("to", K::Length)],
        ConditionTest::Value { .. } => vec![
            field("from", K::Value(Edge::Low)),
            field("to", K::Value(Edge::High)),
        ],
        ConditionTest::Controller { .. } => vec![
            field("from", K::Bounded(0, 127)),
            field("to", K::Bounded(0, 127)),
        ],
        ConditionTest::Metric { .. } => vec![
            field("grid", K::Length),
            field("offset", K::Length),
            field("within", K::Length),
        ],
        ConditionTest::EveryNth { .. } => vec![
            field("every", K::Bounded(1, 1_000_000)),
            field("starting at", K::Count),
        ],
        ConditionTest::Kinds { .. }
        | ConditionTest::Dest { .. }
        | ConditionTest::Automation { .. } => Vec::new(),
    }
}

/// A condition's numbers, one per field (`PanelFields.swift:123-135`).
pub fn condition_numbers(t: &ConditionTest) -> Vec<i64> {
    match *t {
        ConditionTest::TimeRange { start, end } => vec![start, end],
        ConditionTest::Pitch { low, high }
        | ConditionTest::BracketedBy { low, high }
        | ConditionTest::Controller { low, high } => vec![low.into(), high.into()],
        ConditionTest::Velocity { low, high } | ConditionTest::ReleaseVelocity { low, high } => {
            vec![low.into(), high.into()]
        }
        ConditionTest::Duration { min, max } => vec![min, max],
        ConditionTest::Value { low, high } => vec![low.into(), high.into()],
        ConditionTest::Metric {
            grid,
            offset,
            tolerance,
        } => vec![grid, offset, tolerance],
        ConditionTest::EveryNth { n, phase } => vec![n.into(), phase.into()],
        ConditionTest::Kinds { .. }
        | ConditionTest::Dest { .. }
        | ConditionTest::Automation { .. } => Vec::new(),
    }
}

fn pitch7(v: i64) -> u8 {
    v.clamp(0, 127) as u8
}

fn u16_of(v: i64) -> u16 {
    v.clamp(0, i64::from(u16::MAX)) as u16
}

fn u32_of(v: i64) -> u32 {
    v.clamp(0, i64::from(u32::MAX)) as u32
}

fn i8_of(v: i64) -> i8 {
    v.clamp(i64::from(i8::MIN), i64::from(i8::MAX)) as i8
}

/// The condition with its numbers replaced, each clamped to its type
/// (`PanelFields.swift:137-155`); `None` unless `n` has one number per
/// field.
pub fn condition_with(t: &ConditionTest, n: &[i64]) -> Option<ConditionTest> {
    if n.len() != condition_fields(t).len() {
        return None;
    }
    let at = |i: usize| n[i];
    Some(match t {
        ConditionTest::TimeRange { .. } => ConditionTest::TimeRange {
            start: at(0),
            end: at(1),
        },
        ConditionTest::Pitch { .. } => ConditionTest::Pitch {
            low: pitch7(at(0)),
            high: pitch7(at(1)),
        },
        ConditionTest::BracketedBy { .. } => ConditionTest::BracketedBy {
            low: pitch7(at(0)),
            high: pitch7(at(1)),
        },
        ConditionTest::Velocity { .. } => ConditionTest::Velocity {
            low: u16_of(at(0)),
            high: u16_of(at(1)),
        },
        ConditionTest::ReleaseVelocity { .. } => ConditionTest::ReleaseVelocity {
            low: u16_of(at(0)),
            high: u16_of(at(1)),
        },
        ConditionTest::Duration { .. } => ConditionTest::Duration {
            min: at(0),
            max: at(1),
        },
        ConditionTest::Value { .. } => ConditionTest::Value {
            low: u32_of(at(0)),
            high: u32_of(at(1)),
        },
        ConditionTest::Controller { .. } => ConditionTest::Controller {
            low: pitch7(at(0)),
            high: pitch7(at(1)),
        },
        ConditionTest::Metric { .. } => ConditionTest::Metric {
            grid: at(0).max(1),
            offset: at(1),
            tolerance: at(2).max(0),
        },
        ConditionTest::EveryNth { .. } => ConditionTest::EveryNth {
            n: u32_of(at(0).max(1)),
            phase: u32_of(at(1)),
        },
        ConditionTest::Kinds { .. }
        | ConditionTest::Dest { .. }
        | ConditionTest::Automation { .. } => t.clone(),
    })
}

/// What a change of `t` is typed in, at edge `e` (`PanelFields.swift:159-165`).
fn unit(t: ValueTargetRow, e: Edge) -> FieldKind {
    match t {
        ValueTargetRow::Velocity | ValueTargetRow::ReleaseVelocity => FieldKind::Velocity(e),
        ValueTargetRow::Duration => FieldKind::Length,
        ValueTargetRow::Value => FieldKind::Value(e),
    }
}

/// Change's fields (`PanelFields.swift:167-181`).
fn change_fields(t: ValueTargetRow, op: &ChangeOpRow, a: &AmountRow) -> Vec<Field> {
    let k = match op {
        ChangeOpRow::Limit { .. } => {
            return vec![
                field("lowest", unit(t, Edge::Low)),
                field("highest", unit(t, Edge::High)),
            ];
        }
        ChangeOpRow::Compress { .. } => {
            return vec![
                field("lowest", unit(t, Edge::Exact)),
                field("highest", unit(t, Edge::Exact)),
            ];
        }
        ChangeOpRow::Legato => return Vec::new(),
        ChangeOpRow::Scale => FieldKind::Percent,
        ChangeOpRow::Add => unit(t, Edge::Delta),
        ChangeOpRow::Set => unit(t, Edge::Exact),
    };
    match a {
        AmountRow::Constant { .. } => vec![field("amount", k)],
        AmountRow::Line { .. } => vec![field("from", k), field("to", k)],
        AmountRow::Oscillator { .. } => vec![
            field("low", k),
            field("high", k),
            field("period", FieldKind::Length),
            field("phase", FieldKind::Length),
        ],
        AmountRow::Random { .. } => vec![field("low", k), field("high", k)],
    }
}

/// What a line's values are typed in (Plan 4d Ruling 18,
/// `PanelFields.swift:183-193`).
pub fn line_unit(t: &LineTargetRow, op: LineOpRow) -> FieldKind {
    let e = if op == LineOpRow::Add {
        Edge::Delta
    } else {
        Edge::Exact
    };
    if op == LineOpRow::Scale {
        return FieldKind::Percent;
    }
    match t {
        LineTargetRow::Tempo => FieldKind::Bpm,
        LineTargetRow::Value {
            target: ValueTargetRow::Velocity | ValueTargetRow::ReleaseVelocity,
        } => FieldKind::Velocity(e),
        LineTargetRow::Value {
            target: ValueTargetRow::Duration,
        } => FieldKind::Length,
        _ => FieldKind::Value(e),
    }
}

/// A line's fields (`PanelFields.swift:195-208`).
fn line_fields(t: &LineTargetRow, op: LineOpRow, l: &LineRow) -> Vec<Field> {
    use FieldKind as K;
    let u = line_unit(t, op);
    let mut f = vec![
        field("from", K::Position),
        field("to", K::Position),
        field("start", u),
        field("end", u),
        field("step", K::Length),
    ];
    if let LineShapeRow::Oscillator { period, .. } = &l.shape {
        f.push(match period {
            PeriodRow::Ticks { .. } => field("period", K::Length),
            PeriodRow::Millis { .. } => field("period (ms)", K::Bounded(1, 600_000)),
            PeriodRow::Bars { .. } => field("period (bars)", K::Bounded(1, 1_000)),
        });
        f.push(field("phase (\u{2030})", K::Bounded(0, 999)));
    }
    f
}

/// `t` moved from range `a` to range `b` in proportion (a zero-width `a`
/// maps to `b`'s start), inside `b` (`PanelFields.swift:210-218`).
pub fn remap(t: i64, a: (i64, i64), b: (i64, i64)) -> i64 {
    let span = a.1 as f64 - a.0 as f64;
    if span <= 0. {
        return b.0;
    }
    let x = (t as f64 - a.0 as f64) / span;
    let v = (b.0 as f64 + x * (b.1 as f64 - b.0 as f64)).round();
    // Swift's `min(max(v, b.0), b.1)`; `as` saturates.
    v.max(b.0 as f64).min(b.1 as f64) as i64
}

/// A line's numbers (`PanelFields.swift:220-231`).
fn line_numbers(l: &LineRow) -> Vec<i64> {
    let mut n = vec![l.from, l.to, l.start, l.end, l.step];
    if let LineShapeRow::Oscillator { period, phase, .. } = &l.shape {
        n.push(match *period {
            PeriodRow::Ticks { ticks } => ticks,
            PeriodRow::Millis { ms } => ms.into(),
            PeriodRow::Bars { bars } => bars.into(),
        });
        n.push((*phase).into());
    }
    n
}

/// The line with numbers `n` (`PanelFields.swift:233-256`): "to" at least
/// "from", "step" and the period at least 1, the phase 0–999; a Free
/// line's points move with its range.
fn line_with(l: &LineRow, n: &[i64]) -> LineRow {
    let mut o = l.clone();
    o.from = n[0];
    o.to = n[1].max(n[0]);
    if let LineShapeRow::Free { points } = &l.shape
        && (o.from != l.from || o.to != l.to)
    {
        o.shape = LineShapeRow::Free {
            points: points
                .iter()
                .map(|p| LinePointRow {
                    tick: remap(p.tick, (l.from, l.to), (o.from, o.to)),
                    value: p.value,
                })
                .collect(),
        };
    }
    o.start = n[2];
    o.end = n[3];
    o.step = n[4].max(1);
    if let LineShapeRow::Oscillator { wave, period, .. } = &l.shape {
        let v = n[5].max(1);
        let period = match period {
            PeriodRow::Ticks { .. } => PeriodRow::Ticks { ticks: v },
            PeriodRow::Millis { .. } => PeriodRow::Millis { ms: u32_of(v) },
            PeriodRow::Bars { .. } => PeriodRow::Bars { bars: u32_of(v) },
        };
        o.shape = LineShapeRow::Oscillator {
            wave: *wave,
            period,
            phase: n[6].clamp(0, 999) as u16,
        };
    }
    o
}

/// A modification's fields (`PanelFields.swift:258-273`).
pub fn modify_fields(r: &ModifyRow) -> Vec<Field> {
    use FieldKind as K;
    match r {
        ModifyRow::TransposeInterval { .. } => vec![field("semitones", K::Bounded(-127, 127))],
        ModifyRow::TransposeDiatonic { .. } => vec![field("steps", K::Bounded(-127, 127))],
        ModifyRow::TransposeInvert { .. } => vec![field("around", K::Pitch)],
        ModifyRow::Quantize { .. } => vec![field("grid", K::Length), field("strength", K::Percent)],
        ModifyRow::Change { target, op, amount } => change_fields(*target, op, amount),
        ModifyRow::Move { .. } | ModifyRow::TrimStart { .. } => vec![field("by", K::Length)],
        ModifyRow::ScaleTime { .. } => vec![
            field("times", K::Bounded(1, 1_000_000)),
            field("divided by", K::Bounded(1, 1_000_000)),
            field("around", K::Position),
        ],
        ModifyRow::Substitute {
            to: SubstituteRow::Controller { .. },
        } => vec![field("controller", K::Bounded(0, 127))],
        ModifyRow::Substitute {
            to: SubstituteRow::Pitch { .. },
        } => vec![field("pitch", K::Pitch)],
        ModifyRow::Thin { .. } | ModifyRow::Thicken { .. } => vec![field("spacing", K::Length)],
        ModifyRow::Line { target, op, line } => line_fields(target, *op, line),
        ModifyRow::TransposeToScale { .. }
        | ModifyRow::NoteMap { .. }
        | ModifyRow::SetDest { .. }
        | ModifyRow::Substitute { .. }
        | ModifyRow::Delete => Vec::new(),
    }
}

/// A modification's numbers, one per field (`PanelFields.swift:276-302`).
pub fn modify_numbers(r: &ModifyRow) -> Vec<i64> {
    match r {
        ModifyRow::TransposeInterval { semitones } => vec![(*semitones).into()],
        ModifyRow::TransposeDiatonic { steps, .. } => vec![(*steps).into()],
        ModifyRow::TransposeInvert { axis } => vec![(*axis).into()],
        ModifyRow::Quantize { grid, strength, .. } => vec![*grid, (*strength).into()],
        ModifyRow::Change { op, amount, .. } => match *op {
            ChangeOpRow::Limit { min: a, max: b } | ChangeOpRow::Compress { low: a, high: b } => {
                vec![a, b]
            }
            ChangeOpRow::Legato => Vec::new(),
            ChangeOpRow::Set | ChangeOpRow::Scale | ChangeOpRow::Add => match *amount {
                AmountRow::Constant { value } => vec![value],
                AmountRow::Line { from, to, .. } => vec![from, to],
                AmountRow::Oscillator {
                    low,
                    high,
                    period,
                    phase,
                    ..
                } => vec![low, high, period, phase],
                AmountRow::Random { low, high, .. } => vec![low, high],
            },
        },
        ModifyRow::Move { delta } | ModifyRow::TrimStart { delta } => vec![*delta],
        ModifyRow::ScaleTime { num, den, anchor } => vec![(*num).into(), (*den).into(), *anchor],
        ModifyRow::Substitute {
            to: SubstituteRow::Controller { index },
        } => vec![(*index).into()],
        ModifyRow::Substitute {
            to: SubstituteRow::Pitch { pitch },
        } => vec![(*pitch).into()],
        ModifyRow::Thin { spacing } | ModifyRow::Thicken { spacing } => vec![*spacing],
        ModifyRow::Line { line, .. } => line_numbers(line),
        ModifyRow::TransposeToScale { .. }
        | ModifyRow::NoteMap { .. }
        | ModifyRow::SetDest { .. }
        | ModifyRow::Substitute { .. }
        | ModifyRow::Delete => Vec::new(),
    }
}

/// The modification with its numbers replaced, each clamped to its type
/// (`PanelFields.swift:304-346`); `None` unless `n` has one number per
/// field.
pub fn modify_with(r: &ModifyRow, n: &[i64]) -> Option<ModifyRow> {
    if n.len() != modify_fields(r).len() {
        return None;
    }
    let at = |i: usize| n[i];
    Some(match r {
        ModifyRow::TransposeInterval { .. } => ModifyRow::TransposeInterval {
            semitones: i8_of(at(0)),
        },
        ModifyRow::TransposeDiatonic { root, mask, .. } => ModifyRow::TransposeDiatonic {
            steps: i8_of(at(0)),
            root: *root,
            mask: *mask,
        },
        ModifyRow::TransposeInvert { .. } => ModifyRow::TransposeInvert {
            axis: pitch7(at(0)),
        },
        ModifyRow::Quantize { offsets, .. } => ModifyRow::Quantize {
            grid: at(0).max(1),
            strength: at(1).clamp(0, 100) as u8,
            offsets: offsets.clone(),
        },
        ModifyRow::Change { target, op, amount } => {
            let (op, amount) = match *op {
                ChangeOpRow::Limit { .. } => (
                    ChangeOpRow::Limit {
                        min: at(0),
                        max: at(1),
                    },
                    *amount,
                ),
                ChangeOpRow::Compress { .. } => (
                    ChangeOpRow::Compress {
                        low: at(0),
                        high: at(1),
                    },
                    *amount,
                ),
                ChangeOpRow::Legato => return Some(r.clone()),
                ChangeOpRow::Set | ChangeOpRow::Scale | ChangeOpRow::Add => {
                    let a = match *amount {
                        AmountRow::Constant { .. } => AmountRow::Constant { value: at(0) },
                        AmountRow::Line { curve, .. } => AmountRow::Line {
                            from: at(0),
                            to: at(1),
                            curve,
                        },
                        AmountRow::Oscillator { wave, .. } => AmountRow::Oscillator {
                            wave,
                            low: at(0),
                            high: at(1),
                            period: at(2).max(1),
                            phase: at(3),
                        },
                        AmountRow::Random { seed, .. } => AmountRow::Random {
                            low: at(0),
                            high: at(1),
                            seed,
                        },
                    };
                    (*op, a)
                }
            };
            ModifyRow::Change {
                target: *target,
                op,
                amount,
            }
        }
        ModifyRow::Move { .. } => ModifyRow::Move { delta: at(0) },
        ModifyRow::TrimStart { .. } => ModifyRow::TrimStart { delta: at(0) },
        ModifyRow::ScaleTime { .. } => ModifyRow::ScaleTime {
            num: u32_of(at(0).max(1)),
            den: u32_of(at(1).max(1)),
            anchor: at(2),
        },
        ModifyRow::Substitute {
            to: SubstituteRow::Controller { .. },
        } => ModifyRow::Substitute {
            to: SubstituteRow::Controller {
                index: pitch7(at(0)),
            },
        },
        ModifyRow::Substitute {
            to: SubstituteRow::Pitch { .. },
        } => ModifyRow::Substitute {
            to: SubstituteRow::Pitch {
                pitch: pitch7(at(0)),
            },
        },
        ModifyRow::Thin { .. } => ModifyRow::Thin {
            spacing: at(0).max(1),
        },
        ModifyRow::Thicken { .. } => ModifyRow::Thicken {
            spacing: at(0).max(1),
        },
        ModifyRow::Line { target, op, line } => ModifyRow::Line {
            target: *target,
            op: *op,
            line: line_with(line, n),
        },
        ModifyRow::TransposeToScale { .. }
        | ModifyRow::NoteMap { .. }
        | ModifyRow::SetDest { .. }
        | ModifyRow::Substitute { .. }
        | ModifyRow::Delete => r.clone(),
    })
}

/// A note map as "C2>D2, E2>F#2" (`PanelFields.swift:350-353`).
pub fn note_map_text(m: &[NoteMapEntry]) -> String {
    m.iter()
        .map(|e| format!("{}>{}", PitchName::name(e.from), PitchName::name(e.to)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The note map `s` writes; empty is no map, anything unreadable `None`
/// (`PanelFields.swift:355-365`).
pub fn parse_note_map(s: &str) -> Option<Vec<NoteMapEntry>> {
    s.split(',')
        .filter(|item| !item.is_empty())
        .map(|item| {
            let p: Vec<&str> = item.split('>').filter(|p| !p.is_empty()).collect();
            match p.as_slice() {
                [a, b] => Some(NoteMapEntry {
                    from: PitchName::parse(a)?,
                    to: PitchName::parse(b)?,
                }),
                _ => None,
            }
        })
        .collect()
}

/// Groove offsets in ticks as "0 120 -60" (`PanelFields.swift:370`).
pub fn offsets_text(o: &[i64]) -> String {
    o.iter().map(i64::to_string).collect::<Vec<_>>().join(" ")
}

/// The offsets `s` writes; empty or blank is a plain grid
/// (`PanelFields.swift:372-379`).
pub fn parse_offsets(s: &str) -> Option<Vec<i64>> {
    s.split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| swift_int(w).filter(|v| v.unsigned_abs() <= 1_000_000))
        .collect()
}
