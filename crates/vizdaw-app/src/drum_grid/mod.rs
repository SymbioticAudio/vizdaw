//! The Drum Grid's logic (Swift `DrumGrid.swift`, `DrumGridModel.swift`,
//! `DrumCommands.swift`; Plan 4e): the bar's settings and length choices,
//! the grid's labels and messages, and what the pane's grid shows. The grid's records and rules are
//! the core's (`vizdaw_ffi::drum`). No GPUI; the view is the GPUI crate's.

mod commands;
mod model;
mod rows;
mod settings;
mod stroke;
mod text;

pub use model::{DrumGridModel, DrumRowKey, drum_grid_offers};

pub use stroke::DrumExtend;

pub use commands::{DRUM_COMMANDS, drum_enabled, drum_is_on, run_drum};
pub use rows::{drum_quantize_choices, drum_quantize_title};

pub use settings::{DrumGridSettings, DrumView, LENGTHS, lengths_including, stepped_velocity};
pub use text::{
    CHOOSE_TEXT, CHOOSE_TITLE, ERASE_LABEL, MAX_TREE_ITEMS, NEW_TRACK_BAND, NO_DRUM_TRACK, NO_GRID,
    NO_ROW, NO_TRACK_TO_ADD, NO_TRACKS_TEXT, NO_TRACKS_TITLE, PAINT_LABEL, REFUSED, caption,
    cell_label, every_note_taken, header_text, nudge_title, row_label,
};
