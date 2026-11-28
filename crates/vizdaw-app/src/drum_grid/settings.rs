//! The Drum Grid bar's settings and length choices (Swift
//! `M/DrumGrid.swift`; Plan 4e Ruling 11), shared by every drum grid and
//! saved with the project under `drumGrid`.

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use vizdaw_ffi::drum::{DrumGridSettingsRow, DrumViewRow, drum_grid_settings_sane};

use crate::arrangement::PPQ;
use crate::editor::{EditorLayout, field};

/// How the grid shows hits (spec §4e, `M/DrumGrid.swift:3-20`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DrumView {
    Matrix,
    Timeline,
}

impl DrumView {
    pub const ALL: [DrumView; 2] = [Self::Matrix, Self::Timeline];

    /// `M/DrumGrid.swift:7-12`.
    pub fn title(self) -> &'static str {
        match self {
            Self::Matrix => "Step Matrix",
            Self::Timeline => "Timeline",
        }
    }

    pub fn row(self) -> DrumViewRow {
        match self {
            Self::Matrix => DrumViewRow::Matrix,
            Self::Timeline => DrumViewRow::Timeline,
        }
    }
}

/// The bar's settings (`M/DrumGrid.swift:22-69`), under Swift's keys; all
/// eight are always written. Decoding is tolerant, and step, spacing and
/// duration are the lengths Rust paints with.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrumGridSettings {
    pub view: DrumView,
    /// A matrix cell.
    pub step: i64,
    /// Distance between a stroke's hits.
    pub spacing: i64,
    /// Length of a painted hit.
    pub duration: i64,
    /// Velocity (1–127) of a painted hit while its row's velocity lane is
    /// hidden.
    pub velocity: u8,
    pub row_height: f64,
    pub velocity_height: f64,
    /// The grid's own zoom (a 4/4 bar is 640 pt at 96).
    pub ticks_per_point: f64,
}

impl Default for DrumGridSettings {
    /// `M/DrumGrid.swift:27-39`.
    fn default() -> Self {
        DrumGridSettings {
            view: DrumView::Matrix,
            step: PPQ / 4,
            spacing: PPQ / 4,
            duration: PPQ / 8,
            velocity: 100,
            row_height: 20.,
            velocity_height: 28.,
            ticks_per_point: 96.,
        }
    }
}

/// Step, spacing and duration choices: title and ticks
/// (`M/DrumGrid.swift:79-83`).
pub const LENGTHS: [(&str, i64); 9] = [
    ("1 Bar", 4 * PPQ),
    ("1/2", 2 * PPQ),
    ("1/4", PPQ),
    ("1/8", PPQ / 2),
    ("1/8 Triplet", PPQ / 3),
    ("1/16", PPQ / 4),
    ("1/16 Triplet", PPQ / 6),
    ("1/32", PPQ / 8),
    ("1/64", PPQ / 16),
];

/// [`LENGTHS`], plus `current` ("‹n› ticks") when it is none of them (a
/// hand-edited `ui.json`), so a picker never shows blank
/// (`M/DrumGrid.swift:85-90`).
pub fn lengths_including(current: i64) -> Vec<(String, i64)> {
    let mut list: Vec<(String, i64)> = LENGTHS.iter().map(|&(s, v)| (s.to_owned(), v)).collect();
    if !LENGTHS.iter().any(|&(_, v)| v == current) {
        list.push((format!("{current} ticks"), current));
    }
    list
}

/// The bar's velocity stepper: up 4 to at most 127, down 4 to at least 1
/// (`S/DrumGridPane.swift:117-118`).
pub fn stepped_velocity(v: u8, up: bool) -> u8 {
    if up {
        v.saturating_add(4).min(127)
    } else {
        v.max(5) - 4
    }
}

impl DrumGridSettings {
    /// `M/DrumGrid.swift:55`.
    pub const ROW_HEIGHT_RANGE: RangeInclusive<f64> = 12.0..=64.;
    /// `M/DrumGrid.swift:56`.
    pub const VELOCITY_HEIGHT_RANGE: RangeInclusive<f64> = 16.0..=96.;

    /// Tolerant, field by field (`M/DrumGrid.swift:47-69`): one that is
    /// missing or malformed keeps its default; the velocity is 1–127, the
    /// heights and zoom are clamped, and step, spacing and duration are
    /// Rust's (`drum_grid_settings_sane`), else at least 1. Anything but an
    /// object is the default.
    pub fn decode(v: &Value) -> DrumGridSettings {
        let d = DrumGridSettings::default();
        let Some(obj) = v.as_object() else {
            return d;
        };
        let mut s = DrumGridSettings {
            view: field(obj, "view", d.view),
            step: field(obj, "step", d.step),
            spacing: field(obj, "spacing", d.spacing),
            duration: field(obj, "duration", d.duration),
            velocity: field(obj, "velocity", d.velocity).clamp(1, 127),
            row_height: EditorLayout::sane(
                field(obj, "rowHeight", d.row_height),
                &Self::ROW_HEIGHT_RANGE,
                d.row_height,
            ),
            velocity_height: EditorLayout::sane(
                field(obj, "velocityHeight", d.velocity_height),
                &Self::VELOCITY_HEIGHT_RANGE,
                d.velocity_height,
            ),
            ticks_per_point: EditorLayout::sane(
                field(obj, "ticksPerPoint", d.ticks_per_point),
                &EditorLayout::TICKS_PER_POINT_RANGE,
                d.ticks_per_point,
            ),
        };
        match drum_grid_settings_sane(s.row()) {
            Ok(sane) => {
                s.step = sane.step;
                s.spacing = sane.spacing;
                s.duration = sane.duration;
            }
            Err(_) => {
                s.step = s.step.max(1);
                s.spacing = s.spacing.max(1);
                s.duration = s.duration.max(1);
            }
        }
        s
    }

    /// The settings as Rust takes them (`M/DrumGrid.swift:72-77`).
    pub fn row(&self) -> DrumGridSettingsRow {
        DrumGridSettingsRow {
            view: self.view.row(),
            step: self.step,
            spacing: self.spacing,
            duration: self.duration,
            velocity: self.velocity,
            row_height: self.row_height,
            velocity_height: self.velocity_height,
        }
    }
}
