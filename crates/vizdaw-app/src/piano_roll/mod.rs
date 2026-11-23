//! The Piano Roll detail editor's logic (Swift `PianoRollSettings`,
//! `PianoRollModel`, `EditorModel+Pencil`): saved settings, shown tracks,
//! the edit track, notes and selection. No GPUI; the canvas is the GPUI
//! crate's.

mod model;
mod settings;

pub use model::{LaneNotes, PianoRollModel, PianoRollReveal};
pub use settings::PianoRollSettings;
