//! The Strip Chart's logic (Swift `StripLane.swift`, `AutomationText.swift`,
//! Plan 4d Rulings 15-26): what a row shows, its names and value text, and
//! value ↔ y inside a row; the bar's settings, strokes as lines and
//! commands. No GPUI; the view is the GPUI crate's.

mod lane;
mod line;
mod scale;
mod settings;

pub use lane::{
    StripLane, StripNames, StripStyle, StripValue, after, automation_text, is_switch, normalized,
    parse_percent, percent, plain_text, short_title, v32,
};
pub use line::{
    StripPoint, compress_command, erase_command, free_points, line_command, stroke_label,
    stroke_line, stroke_op,
};
pub use scale::{StripMode, StripOp, StripScale};
pub use settings::{
    LineCurve, MAX_ROWS, PeriodKind, ShapeKind, StripBarChoices, StripChartSettings,
};

pub use crate::editor::controllers_key;
