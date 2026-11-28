//! The Strip Chart's logic (Swift `StripLane.swift`, `AutomationText.swift`,
//! Plan 4d Rulings 15-26): what a row shows, its names and value text, and
//! value ↔ y inside a row. No GPUI; the view is the GPUI crate's.

mod lane;
mod scale;

pub use lane::{
    StripLane, StripNames, StripStyle, StripValue, after, automation_text, is_switch, normalized,
    parse_percent, percent, plain_text, short_title, v32,
};
pub use scale::{StripMode, StripOp, StripScale};
