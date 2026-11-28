//! The Strip Chart canvas geometry (`E/StripChartView.swift:30-31,
//! 120-151`): rows ↔ y, content points ↔ row ticks and values, and the
//! stroke preview's track space. Content points are below the ruler; `y`
//! grows down. Everything clamps instead of trapping.

use vizdaw_ffi::events::MeterEntryRow;

use super::lane::StripLane;
use super::line::StripPoint;
use super::scale::{StripMode, StripOp, StripScale};
use super::settings::StripChartSettings;
use crate::arrangement::TimeAxis;
use crate::editor::Snap;

/// A press this close to a row's top, left of [`TITLE_WIDTH`], opens the
/// row's menu (`E/StripChartView.swift:30`).
pub const TITLE_HEIGHT: f32 = 18.;
/// `E/StripChartView.swift:31`.
pub const TITLE_WIDTH: f32 = 200.;

/// The canvas as it is scrolled: the time axis, each row's height and the
/// vertical scroll (`offset` points of the rows are above the top).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripFrame {
    pub axis: TimeAxis,
    pub row_height: f32,
    pub offset: f32,
}

impl StripFrame {
    fn height(&self) -> f32 {
        if self.row_height.is_finite() {
            self.row_height.max(0.)
        } else {
            0.
        }
    }

    fn off(&self) -> f32 {
        if self.offset.is_finite() {
            self.offset
        } else {
            0.
        }
    }

    /// Row `i`'s top in content points.
    pub fn row_top(&self, i: usize) -> f32 {
        i as f32 * self.height() - self.off()
    }

    /// The row at `y` of `rows`; `None` above the first, below the last,
    /// or for a non-finite `y`.
    pub fn row_at(&self, y: f32, rows: usize) -> Option<usize> {
        let h = self.height();
        if !y.is_finite() || h <= 0. {
            return None;
        }
        let f = ((y + self.off()) / h).floor();
        if !f.is_finite() || f < 0. {
            return None;
        }
        let i = f as usize;
        (i < rows).then_some(i)
    }

    /// `(x, y)` is on row `i`'s title (`E/StripChartView.swift:278`).
    pub fn in_title(&self, x: f32, y: f32, i: usize) -> bool {
        let dy = y - self.row_top(i);
        (0. ..TITLE_HEIGHT).contains(&dy) && x < TITLE_WIDTH
    }

    /// Value ↔ y in row `i` showing `lane`.
    pub fn scale(&self, i: usize, lane: StripLane) -> StripScale {
        StripScale::new(lane.range(), self.row_top(i), self.height())
    }

    /// A content point as row `i`'s tick and value: snapped to the grid
    /// when snap is on and at least 0; in Modify mode other than Compress
    /// the operation's amount (Plan 4d Ruling 18,
    /// `E/StripChartView.swift:142-151`).
    #[allow(clippy::too_many_arguments)]
    pub fn point(
        &self,
        x: f32,
        y: f32,
        i: usize,
        lane: StripLane,
        settings: &StripChartSettings,
        snap: &Snap,
        meter: &[MeterEntryRow],
    ) -> StripPoint {
        let raw = self.axis.tick_at(x).max(0);
        let tick = snap.apply(raw, meter).max(0);
        let s = self.scale(i, lane);
        let value = if settings.mode == StripMode::Modify && settings.op != StripOp::Compress {
            s.amount_at(y, settings.op)
        } else {
            s.value_at(y)
        };
        StripPoint { tick, value }
    }

    /// A content point in track space: its tick and its y in the rows
    /// (the stroke preview, `E/StripChartView.swift:124-125`).
    pub fn lane_point(&self, x: f32, y: f32) -> (i64, f32) {
        (self.axis.tick_at(x), y + self.off())
    }

    /// A track-space point where the view shows it now.
    pub fn content_point(&self, p: (i64, f32)) -> (f32, f32) {
        (self.axis.x(p.0), p.1 - self.off())
    }

    /// The rows' total height.
    pub fn content_height(&self, rows: usize) -> f32 {
        rows as f32 * self.height()
    }
}
