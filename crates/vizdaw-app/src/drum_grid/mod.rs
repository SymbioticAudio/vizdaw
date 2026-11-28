//! The Drum Grid's logic (Swift `DrumGrid.swift`, `DrumGridModel.swift`,
//! `DrumCommands.swift`; Plan 4e): the bar's settings and length choices,
//! and the grid's labels and messages. The grid's records and rules are
//! the core's (`vizdaw_ffi::drum`). No GPUI; the view is the GPUI crate's.

mod settings;
mod text;

pub use settings::{DrumGridSettings, DrumView, LENGTHS, lengths_including, stepped_velocity};
pub use text::{
    CHOOSE_TEXT, CHOOSE_TITLE, ERASE_LABEL, MAX_TREE_ITEMS, NEW_TRACK_BAND, NO_DRUM_TRACK, NO_GRID,
    NO_ROW, NO_TRACK_TO_ADD, NO_TRACKS_TEXT, NO_TRACKS_TITLE, PAINT_LABEL, REFUSED, caption,
    cell_label, every_note_taken, header_text, nudge_title, row_label,
};
