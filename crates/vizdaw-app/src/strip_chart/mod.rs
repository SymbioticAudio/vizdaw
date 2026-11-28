//! The Strip Chart's logic (Swift `StripLane.swift`, `AutomationText.swift`,
//! Plan 4d Rulings 15-26): what a row shows, its names and value text, and
//! value ↔ y inside a row; the bar's settings, strokes as lines and
//! commands; the mixer's automation targets and what each chart shows.
//! No GPUI; the view is the GPUI crate's.

mod frame;
mod lane;
mod line;
mod model;
mod scale;
mod settings;
mod targets;

pub use frame::{StripFrame, TITLE_HEIGHT, TITLE_WIDTH};
pub use lane::{
    StripLane, StripNames, StripStyle, StripValue, after, automation_text, is_switch, normalized,
    parse_percent, percent, plain_text, short_title, v32,
};
pub use line::{
    StripPoint, compress_command, erase_command, free_points, line_command, stroke_label,
    stroke_line, stroke_op,
};
pub(crate) use model::StripCharts;
pub use model::{ChartSlot, ERASE_NOTES, StripChartModel, StripStroke, only_target, opening};
pub use scale::{StripMode, StripOp, StripScale};
pub use settings::{
    LineCurve, MAX_ROWS, PeriodKind, ShapeKind, StripBarChoices, StripChartSettings,
};

pub use targets::{AutomationTargets, StripMenu};

pub use crate::editor::controllers_key;
