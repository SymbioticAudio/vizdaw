//! The List editor's logic (Swift `ListFormat`, `ListRows`, `ListEdit`,
//! `ListEditorModel`, `ListPreferences`): cell text and typing, rows,
//! inserts and deletes. No GPUI; the table is the GPUI crate's.

mod edit;
mod format;
mod model;
mod rows;

pub use edit::{ListChange, ListDelete, ListEdit, ListEditError, ListInsert};
pub(crate) use format::trim_ws;
pub use format::{
    Bbu, Bend14, DurationText, KeyName, PitchName, Smpte, UNITS_PER_BEAT, Value7, Velocity7,
    parse_position,
};
pub use model::{
    FIRST_METER_STAYS, FIRST_TEMPO_STAYS, INSERT_MAP_NEEDS_SEQUENCE, INSERT_METER_TAKEN,
    INSERT_NO_TRACK, INSERT_REFERENCE, ListModel, ListPrefs,
};
pub use rows::{
    ListColumn, ListColumns, ListContext, ListFilter, ListInput, ListKind, ListLane, ListRow,
    ListSource, ListText, TickAtSeconds, build_rows,
};
