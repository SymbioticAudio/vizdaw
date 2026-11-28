//! The Drum menu (Plan 4e Ruling 13, `M/DrumCommands.swift`): one
//! implementation for the menu items, keys and the control channel's
//! `command.run`. It acts on the pane's Drum Grid while it shows
//! (Ruling 2: there is no torn-off grid yet).

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};

use super::{DrumView, NO_DRUM_TRACK, NO_GRID, NO_ROW, REFUSED};
use crate::commands::{Ran, check_names, on_flag, ran};
use crate::document::DocumentModel;

/// The Drum menu's registry IDs (`M/DrumCommands.swift:18-24`).
pub const DRUM_COMMANDS: [&str; 5] = [
    "drum.add_row",
    "drum.show_velocity",
    "drum.split_row",
    "drum.remove_row",
    "drum.timeline",
];

/// Whether `id` can run on `doc` now (its menu item is enabled,
/// `M/DrumCommands.swift:29-36`).
pub fn drum_enabled(id: &str, doc: &DocumentModel) -> bool {
    match id {
        "drum.timeline" => true,
        "drum.add_row" => doc.drum_showing() && doc.drum_current_lane().is_some(),
        "drum.show_velocity" | "drum.split_row" | "drum.remove_row" => {
            doc.drum_showing() && doc.drum_selected().is_some()
        }
        _ => false,
    }
}

/// A toggle's state, its menu item's check mark
/// (`M/DrumCommands.swift:39-45`).
pub fn drum_is_on(id: &str, doc: &DocumentModel) -> bool {
    match id {
        "drum.show_velocity" => {
            doc.drum_showing() && doc.drum_selected().is_some_and(|(_, r)| r.show_velocity)
        }
        "drum.timeline" => doc.drum_settings().view == DrumView::Timeline,
        _ => false,
    }
}

fn unavailable(message: &str) -> ControlError {
    ControlError::new(ErrorCode::Unavailable, message)
}

/// Runs `id` on `doc`; `on` sets a toggle (absent flips it)
/// (`M/DrumCommands.swift:49-73`). A refused edit is `failed` with the
/// status line's message.
pub fn run_drum(
    id: &str,
    args: &[(String, ArgValue)],
    doc: &mut DocumentModel,
) -> Result<Ran, ControlError> {
    check_names(id, args)?;
    let on = on_flag(id, args)?;
    if id == "drum.timeline" {
        let want = on.unwrap_or(!drum_is_on(id, doc));
        doc.update_drum_settings(|s| {
            s.view = if want {
                DrumView::Timeline
            } else {
                DrumView::Matrix
            }
        });
        return Ok(ran(doc));
    }
    if !DRUM_COMMANDS.contains(&id) {
        return Err(ControlError::new(
            ErrorCode::UnknownId,
            format!("There is no command {id}."),
        ));
    }
    if !doc.drum_showing() {
        return Err(unavailable(NO_GRID));
    }
    let ok = if id == "drum.add_row" {
        if doc.drum_current_lane().is_none() {
            return Err(unavailable(NO_DRUM_TRACK));
        }
        doc.drum_add_row()
    } else {
        if doc.drum_selected().is_none() {
            return Err(unavailable(NO_ROW));
        }
        match id {
            "drum.show_velocity" => doc.drum_show_velocity(on),
            "drum.split_row" => doc.drum_split_selected_row(),
            _ => doc.drum_remove_selected_row(),
        }
    };
    if !ok {
        let why = doc.status().filter(|s| !s.is_empty()).unwrap_or(REFUSED);
        return Err(ControlError::new(ErrorCode::Failed, why));
    }
    Ok(ran(doc))
}
