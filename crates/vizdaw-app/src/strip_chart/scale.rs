//! Value ↔ y inside one Strip Chart row, and the chart's modes and
//! operations (Swift `M/StripLane.swift:378-485`; Plan 4d Rulings 15, 18,
//! 19, 22).

use serde::{Deserialize, Serialize};
use vizdaw_ffi::strip::LineOpRow;

use super::lane::StripLane;

/// The Strip Chart's modes (Plan 4d Ruling 22, `StripLane.swift:379-389`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StripMode {
    Draw,
    Modify,
    Erase,
}

impl StripMode {
    pub const ALL: [StripMode; 3] = [Self::Draw, Self::Modify, Self::Erase];

    pub fn title(self) -> &'static str {
        match self {
            Self::Draw => "Draw",
            Self::Modify => "Modify",
            Self::Erase => "Erase",
        }
    }
}

/// What a Modify line does (Plan 4d Rulings 15, 19,
/// `StripLane.swift:392-430`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StripOp {
    Set,
    Scale,
    Add,
    LimitMax,
    LimitMin,
    Compress,
    Thin,
    Thicken,
    Legato,
}

impl StripOp {
    pub const ALL: [StripOp; 9] = [
        Self::Set,
        Self::Scale,
        Self::Add,
        Self::LimitMax,
        Self::LimitMin,
        Self::Compress,
        Self::Thin,
        Self::Thicken,
        Self::Legato,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Set => "Set",
            Self::Scale => "Scale",
            Self::Add => "Add",
            Self::LimitMax => "Max Limit",
            Self::LimitMin => "Min Limit",
            Self::Compress => "Compress",
            Self::Thin => "Thin",
            Self::Thicken => "Thicken",
            Self::Legato => "Legato",
        }
    }

    pub fn row(self) -> LineOpRow {
        match self {
            Self::Set => LineOpRow::Set,
            Self::Scale => LineOpRow::Scale,
            Self::Add => LineOpRow::Add,
            Self::LimitMax => LineOpRow::LimitMax,
            Self::LimitMin => LineOpRow::LimitMin,
            Self::Compress => LineOpRow::Compress,
            Self::Thin => LineOpRow::Thin,
            Self::Thicken => LineOpRow::Thicken,
            Self::Legato => LineOpRow::Legato,
        }
    }

    /// What a row offers: legato for lengths only; thin and thicken for
    /// series and tempo.
    pub fn choices(lane: StripLane) -> Vec<StripOp> {
        let mut ops = vec![
            Self::Set,
            Self::Scale,
            Self::Add,
            Self::LimitMax,
            Self::LimitMin,
            Self::Compress,
        ];
        if lane == StripLane::Duration {
            ops.push(Self::Legato);
        } else if !lane.is_note_value() {
            ops.extend([Self::Thin, Self::Thicken]);
        }
        ops
    }
}

/// Value ↔ y inside one row (`top` is the row's top in the view; `inset`
/// points stay free above and below; `StripLane.swift:433-485`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripScale {
    /// The values the row spans, bottom to top, inclusive.
    pub range: (i64, i64),
    pub top: f32,
    pub height: f32,
    pub inset: f32,
}

impl StripScale {
    pub const INSET: f32 = 3.;

    pub fn new(range: (i64, i64), top: f32, height: f32) -> Self {
        Self {
            range,
            top: if top.is_finite() { top } else { 0. },
            height: if height.is_finite() {
                height.max(0.)
            } else {
                0.
            },
            inset: Self::INSET,
        }
    }

    fn span(&self) -> f32 {
        (self.height - 2. * self.inset).max(1.)
    }

    pub fn y(&self, v: i64) -> f32 {
        let (lo, hi) = self.range;
        let c = if lo <= hi { v.clamp(lo, hi) } else { lo };
        let (lo, hi, c) = (lo as f64, hi as f64, c as f64);
        let f = if hi > lo { (c - lo) / (hi - lo) } else { 0. };
        self.top + self.inset + self.span() * (1. - f) as f32
    }

    /// 0 at the bottom of the row, 1 at its top; clamped.
    pub fn fraction(&self, y: f32) -> f64 {
        if !y.is_finite() {
            return 0.;
        }
        let f = 1. - f64::from((y - self.top - self.inset) / self.span());
        if f.is_finite() { f.clamp(0., 1.) } else { 0. }
    }

    pub fn value_at(&self, y: f32) -> i64 {
        let (lo, hi) = (self.range.0 as f64, self.range.1 as f64);
        (lo + self.fraction(y) * (hi - lo)).round() as i64
    }

    /// What a point at `y` stands for under `op` (Plan 4d Ruling 18): a
    /// percentage for Scale (0 at the bottom, 100 in the middle, 200 at the
    /// top), a signed change for Add (± half the range at the edges), else
    /// a value.
    pub fn amount_at(&self, y: f32, op: StripOp) -> i64 {
        match op {
            StripOp::Scale => (self.fraction(y) * 200.).round() as i64,
            StripOp::Add => {
                let span = (self.range.1 as f64) - (self.range.0 as f64);
                ((self.fraction(y) - 0.5) * span).round() as i64
            }
            _ => self.value_at(y),
        }
    }
}
