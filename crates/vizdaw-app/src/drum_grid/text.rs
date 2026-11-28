//! The Drum Grid's words: its tree labels, header text, captions and
//! messages, each copied from the Swift file:line beside it.

use vizdaw_ffi::drum::{DrumLaneRow, DrumRowRow};
use vizdaw_ffi::events::MeterEntryRow;

use crate::arrangement::{bar_at, beat_ticks};
use crate::list::PitchName;

/// The most cells and hits the grid lists to the accessibility tree and
/// the control channel (the visible ones, `E/DrumGridView.swift:27`).
pub const MAX_TREE_ITEMS: usize = 2_048;

/// The undo name of a painting stroke (`M/DrumGridModel.swift:47`).
pub const PAINT_LABEL: &str = "Paint Drum Hits";
/// The undo name of an erasing stroke (`M/DrumGridModel.swift:48`).
pub const ERASE_LABEL: &str = "Erase Drum Hits";
/// `M/DrumGridModel.swift:49`.
pub const NO_ROW: &str = "Select a drum row first.";
/// `M/DrumCommands.swift:26`.
pub const NO_GRID: &str = "Open a drum grid first.";
/// `M/DrumCommands.swift:57`.
pub const NO_DRUM_TRACK: &str = "The drum grid shows no drum track.";
/// `M/DrumGridModel.swift:281`.
pub const NO_TRACK_TO_ADD: &str = "There is no drum track to add a row to.";
/// `M/DrumCommands.swift:72`.
pub const REFUSED: &str = "The drum command was refused.";
/// The band below the last track (`E/DrumGridView.swift:173`).
pub const NEW_TRACK_BAND: &str = "Drop a row here to give it a track of its own";

/// The empty state with no MIDI or Instrument track
/// (`S/DrumGridPane.swift:37-38`).
pub const NO_TRACKS_TITLE: &str = "No MIDI or Instrument Tracks";
/// `S/DrumGridPane.swift:38`.
pub const NO_TRACKS_TEXT: &str = "Add a MIDI or Instrument track to edit it as drums.";
/// The empty state with tracks but no drum track
/// (`S/DrumGridPane.swift:42-43`).
pub const CHOOSE_TITLE: &str = "Choose a Track";
/// `S/DrumGridPane.swift:43`.
pub const CHOOSE_TEXT: &str = "The Drum Grid edits a MIDI or Instrument track as drum rows.";

/// "Every note already has a row in ‹track›." (`M/DrumGridModel.swift:286`).
pub fn every_note_taken(track: &str) -> String {
    format!("Every note already has a row in {track}.")
}

/// A cell's or hit's label: "Kick, bar 2 beat 3 tick 1920, on, velocity
/// 100" (`E/DrumGridView.swift:15-23`).
pub fn cell_label(
    row: &str,
    tick: i64,
    hit: bool,
    velocity: u16,
    meter: &[MeterEntryRow],
) -> String {
    let bar = bar_at(tick, meter);
    let beat = beat_ticks(&bar.meter).max(1);
    let into = tick - bar.start;
    let mut s = format!("{row}, bar {} beat {}", bar.number, into / beat + 1);
    if into % beat != 0 {
        s += &format!(" tick {}", into % beat);
    }
    if hit {
        s += &format!(", on, velocity {}", velocity >> 9);
    } else {
        s += ", off";
    }
    s
}

/// A row header's label: "‹name›, ‹note›, ‹track›, ‹destination›"
/// (`E/DrumRowHeaderView.swift:311-312`).
pub fn row_label(lane: &DrumLaneRow, row: &DrumRowRow) -> String {
    format!(
        "{}, {}, {}, {}",
        row.name,
        PitchName::name(row.note),
        lane.name,
        row.destination.as_deref().unwrap_or("no destination")
    )
}

/// A track's header band: "‹track› · ‹destination›"
/// (`E/DrumRowHeaderView.swift:88`).
pub fn header_text(lane: &DrumLaneRow) -> String {
    format!(
        "{} · {}",
        lane.name,
        lane.destination.as_deref().unwrap_or("No Destination")
    )
}

/// A row header's text: its name, the destination its notes play on, and
/// whether that is the track's rather than the row's own
/// (`E/DrumRowHeaderView.swift:140-142`).
pub fn caption(row: &DrumRowRow) -> (String, Option<String>, bool) {
    (
        row.name.clone(),
        row.destination.clone(),
        row.dest.is_none(),
    )
}

/// The track menu's nudge section: "Nudge: +360 ticks"
/// (`S/DrumGridPane.swift:151`).
pub fn nudge_title(shift: i64) -> String {
    let sign = if shift > 0 { "+" } else { "" };
    format!("Nudge: {sign}{shift} ticks")
}
