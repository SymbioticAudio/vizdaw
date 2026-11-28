//! The Strip Chart bar's settings and picker choices (Swift
//! `M/StripChart.swift:1-147,234-275`; Plan 4d Rulings 17, 21), shared by
//! every Strip Chart and saved with the project under `stripChart`.

use std::ops::RangeInclusive;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use vizdaw_ffi::modify::CurveRow;
use vizdaw_ffi::strip::{LineShapeRow, PeriodRow};

use super::scale::{StripMode, StripOp};
use crate::arrangement::PPQ;
use crate::editor::EditorLayout;
use crate::select_modify::WaveKind;

/// The most rows a chart shows (`M/StripChartModel.swift:104`).
pub const MAX_ROWS: usize = 4;

/// Line shapes (Plan 4d Ruling 17, `M/StripChart.swift:4-18`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShapeKind {
    Straight,
    Curved,
    Flat,
    Free,
    Random,
    Oscillator,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 6] = [
        Self::Straight,
        Self::Curved,
        Self::Flat,
        Self::Free,
        Self::Random,
        Self::Oscillator,
    ];

    /// `M/StripChart.swift:8-17`.
    pub fn title(self) -> &'static str {
        match self {
            Self::Straight => "Straight",
            Self::Curved => "Curved",
            Self::Flat => "Flat",
            Self::Free => "Free",
            Self::Random => "Random",
            Self::Oscillator => "Oscillator",
        }
    }

    /// The kind of a line's shape.
    pub fn of(shape: &LineShapeRow) -> Self {
        match shape {
            LineShapeRow::Straight => Self::Straight,
            LineShapeRow::Curved { .. } => Self::Curved,
            LineShapeRow::Flat => Self::Flat,
            LineShapeRow::Free { .. } => Self::Free,
            LineShapeRow::Random { .. } => Self::Random,
            LineShapeRow::Oscillator { .. } => Self::Oscillator,
        }
    }
}

/// Curves of a curved line (Swift `CurveKind`, `M/StripChart.swift:20-39`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LineCurve {
    EaseIn,
    EaseOut,
    SCurve,
}

impl LineCurve {
    pub const ALL: [LineCurve; 3] = [Self::EaseIn, Self::EaseOut, Self::SCurve];

    /// `M/StripChart.swift:24-30`.
    pub fn title(self) -> &'static str {
        match self {
            Self::EaseIn => "Ease In",
            Self::EaseOut => "Ease Out",
            Self::SCurve => "S-Curve",
        }
    }

    pub fn row(self) -> CurveRow {
        match self {
            Self::EaseIn => CurveRow::EaseIn,
            Self::EaseOut => CurveRow::EaseOut,
            Self::SCurve => CurveRow::SCurve,
        }
    }
}

/// How an oscillator's period is measured (Plan 4d Ruling 17,
/// `M/StripChart.swift:65-75`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeriodKind {
    Musical,
    Clock,
    Bars,
}

impl PeriodKind {
    pub const ALL: [PeriodKind; 3] = [Self::Musical, Self::Clock, Self::Bars];

    /// `M/StripChart.swift:68-74`.
    pub fn title(self) -> &'static str {
        match self {
            Self::Musical => "Musical",
            Self::Clock => "Clock",
            Self::Bars => "Bars (follow meter)",
        }
    }

    /// The kind of a period.
    pub fn of(period: &PeriodRow) -> Self {
        match period {
            PeriodRow::Ticks { .. } => Self::Musical,
            PeriodRow::Millis { .. } => Self::Clock,
            PeriodRow::Bars { .. } => Self::Bars,
        }
    }
}

/// The bar's settings (`M/StripChart.swift:79-147`), under Swift's keys;
/// all thirteen are always written. Decoding is tolerant.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StripChartSettings {
    pub mode: StripMode,
    pub op: StripOp,
    pub shape: ShapeKind,
    pub curve: LineCurve,
    pub wave: WaveKind,
    pub period: PeriodKind,
    pub period_ticks: i64,
    pub period_ms: u32,
    pub period_bars: u32,
    /// Thousandths of a period (0–999).
    pub phase: u16,
    /// Sample spacing of drawn lines, and the spacing of Thin and Thicken.
    pub step: i64,
    pub row_height: f64,
    /// The chart's own zoom (a 4/4 bar is 640 pt at 96).
    pub ticks_per_point: f64,
}

impl Default for StripChartSettings {
    fn default() -> Self {
        StripChartSettings {
            mode: StripMode::Draw,
            op: StripOp::Set,
            shape: ShapeKind::Straight,
            curve: LineCurve::EaseOut,
            wave: WaveKind::Sine,
            period: PeriodKind::Musical,
            period_ticks: PPQ,
            period_ms: 500,
            period_bars: 1,
            phase: 0,
            step: PPQ / 8,
            row_height: 96.,
            ticks_per_point: 96.,
        }
    }
}

/// `obj[key]` as a `T`, or `fallback` when missing or malformed.
fn field<T: DeserializeOwned>(obj: &Map<String, Value>, key: &str, fallback: T) -> T {
    obj.get(key)
        .and_then(|v| T::deserialize(v).ok())
        .unwrap_or(fallback)
}

impl StripChartSettings {
    /// `M/StripChart.swift:97`.
    pub const ROW_HEIGHT_RANGE: RangeInclusive<f64> = 40.0..=400.;

    /// Tolerant, field by field (`M/StripChart.swift:106-123`): one that is
    /// missing or malformed keeps its default; periods and step are at
    /// least 1, the phase at most 999; anything but an object is the
    /// default.
    pub fn decode(v: &Value) -> StripChartSettings {
        let d = StripChartSettings::default();
        let Some(obj) = v.as_object() else {
            return d;
        };
        StripChartSettings {
            mode: field(obj, "mode", d.mode),
            op: field(obj, "op", d.op),
            shape: field(obj, "shape", d.shape),
            curve: field(obj, "curve", d.curve),
            wave: field(obj, "wave", d.wave),
            period: field(obj, "period", d.period),
            period_ticks: field(obj, "periodTicks", d.period_ticks).max(1),
            period_ms: field(obj, "periodMs", d.period_ms).max(1),
            period_bars: field(obj, "periodBars", d.period_bars).max(1),
            phase: field(obj, "phase", d.phase).min(999),
            step: field(obj, "step", d.step).max(1),
            row_height: EditorLayout::sane(
                field(obj, "rowHeight", d.row_height),
                &Self::ROW_HEIGHT_RANGE,
                d.row_height,
            ),
            ticks_per_point: EditorLayout::sane(
                field(obj, "ticksPerPoint", d.ticks_per_point),
                &EditorLayout::TICKS_PER_POINT_RANGE,
                d.ticks_per_point,
            ),
        }
    }

    /// The oscillator's period (`M/StripChart.swift:125-131`).
    pub fn period_row(&self) -> PeriodRow {
        match self.period {
            PeriodKind::Musical => PeriodRow::Ticks {
                ticks: self.period_ticks.max(1),
            },
            PeriodKind::Clock => PeriodRow::Millis {
                ms: self.period_ms.max(1),
            },
            PeriodKind::Bars => PeriodRow::Bars {
                bars: self.period_bars.max(1),
            },
        }
    }

    /// The shape of a line drawn with these settings; a free line's points
    /// are added by `stroke_line` (`M/StripChart.swift:133-146`).
    pub fn shape_row(&self, seed: u64) -> LineShapeRow {
        match self.shape {
            ShapeKind::Straight => LineShapeRow::Straight,
            ShapeKind::Curved => LineShapeRow::Curved {
                curve: self.curve.row(),
            },
            ShapeKind::Flat => LineShapeRow::Flat,
            ShapeKind::Free => LineShapeRow::Free { points: Vec::new() },
            ShapeKind::Random => LineShapeRow::Random { seed },
            ShapeKind::Oscillator => LineShapeRow::Oscillator {
                wave: self.wave.row(),
                period: self.period_row(),
                phase: self.phase.min(999),
            },
        }
    }
}

/// The bar's picker choices (spec §4d, `M/StripChart.swift:234-275`). A
/// saved value outside a list (an older or hand-edited `ui.json`) is added
/// to it, so its picker never shows blank (parked T14).
pub struct StripBarChoices;

impl StripBarChoices {
    /// `M/StripChart.swift:239-241`.
    pub const STEPS: [(&'static str, i64); 6] = [
        ("1/4", PPQ),
        ("1/8", PPQ / 2),
        ("1/16", PPQ / 4),
        ("1/32", PPQ / 8),
        ("1/64", PPQ / 16),
        ("1/128", PPQ / 32),
    ];
    /// `M/StripChart.swift:242-244`.
    pub const MUSICAL_PERIODS: [(&'static str, i64); 6] = [
        ("1 Bar of 4/4", 4 * PPQ),
        ("1/2", 2 * PPQ),
        ("1/4", PPQ),
        ("1/8", PPQ / 2),
        ("1/16", PPQ / 4),
        ("1/32", PPQ / 8),
    ];
    /// `M/StripChart.swift:245`.
    pub const CLOCK_PERIODS: [u32; 8] = [25, 50, 100, 250, 500, 1000, 2000, 4000];
    /// `M/StripChart.swift:246`.
    pub const BAR_PERIODS: [u32; 5] = [1, 2, 4, 8, 16];
    /// `M/StripChart.swift:247`.
    pub const PHASES: [u16; 5] = [0, 125, 250, 500, 750];

    /// `list`, plus `current` (labelled by `label`) when it is not in it
    /// (`M/StripChart.swift:249-252`).
    fn with<T: PartialEq + Copy>(
        list: Vec<(String, T)>,
        current: T,
        label: impl Fn(T) -> String,
    ) -> Vec<(String, T)> {
        let mut list = list;
        if !list.iter().any(|(_, v)| *v == current) {
            list.push((label(current), current));
        }
        list
    }

    fn named<T: Copy>(list: &[(&str, T)]) -> Vec<(String, T)> {
        list.iter().map(|&(s, v)| (s.to_owned(), v)).collect()
    }

    pub fn steps(current: i64) -> Vec<(String, i64)> {
        Self::with(Self::named(&Self::STEPS), current, |n| format!("{n} ticks"))
    }

    pub fn musical_periods(current: i64) -> Vec<(String, i64)> {
        Self::with(Self::named(&Self::MUSICAL_PERIODS), current, |n| {
            format!("{n} ticks")
        })
    }

    pub fn clock_periods(current: u32) -> Vec<(String, u32)> {
        let list = Self::CLOCK_PERIODS.map(|n| (format!("{n} ms"), n)).to_vec();
        Self::with(list, current, |n| format!("{n} ms"))
    }

    pub fn bar_periods(current: u32) -> Vec<(String, u32)> {
        let list = Self::BAR_PERIODS.map(|n| (bars(n), n)).to_vec();
        Self::with(list, current, bars)
    }

    pub fn phases(current: u16) -> Vec<(String, u16)> {
        let list = Self::PHASES.map(|p| (degrees(p), p)).to_vec();
        Self::with(list, current, degrees)
    }
}

/// "1 bar", "‹n› bars" (`M/StripChart.swift:268`).
fn bars(n: u32) -> String {
    format!("{n} bar{}", if n == 1 { "" } else { "s" })
}

/// A phase in ‰ of a period as degrees, one decimal when not whole
/// (`M/StripChart.swift:269-273`).
fn degrees(p: u16) -> String {
    let d = f64::from(p) * 0.36;
    if d == d.round() {
        format!("{}°", d as i64)
    } else {
        format!("{d:.1}°")
    }
}
