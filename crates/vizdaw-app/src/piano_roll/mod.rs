//! The Piano Roll detail editor's logic (Swift `PianoRollSettings`,
//! `PianoRollModel`, `EditorModel+Pencil`, `PitchAxis`,
//! `PianoRollGeometry`, `PianoTools` and `PianoRollView`'s gestures):
//! saved settings, shown tracks, the edit track, notes and selection,
//! the geometry, and the note drags the canvas feeds in. No GPUI; the
//! canvas is the GPUI crate's.

mod drag;
mod geometry;
mod model;
mod settings;

pub use drag::{
    AuditionRequest, Eraser, NoteDrag, Pencil, RollDragState, RollFrame, RollOutcome, RollPointer,
};
pub use geometry::{
    CursorKind, NoteHit, NoteZone, PitchAxis, RollGeometry, RollHit, a11y_label, band_rect,
    is_black, key_label,
};
pub use model::{LaneNotes, PianoRollModel, PianoRollReveal};
pub use settings::PianoRollSettings;
