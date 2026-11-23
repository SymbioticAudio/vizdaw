//! Audition (Swift `EditorModel+Audition`): notes an editor sends straight
//! to a destination. A failed note-on is dropped; a failed note-off is
//! retried by every [`Audition::tick`] so no note sticks.

use std::collections::{BTreeMap, BTreeSet};

use vizdaw_ffi::VizError;

use crate::DocumentModel;
use crate::alerts::error_message;
use crate::session::DocSession;

/// Editor ticks a note-off is retried for. Swift's 60 Hz plugin timer made
/// this about two seconds; here it is as many ticks of the editor's tick
/// (`DocumentModel::idle`), so the time follows the poll interval.
pub const NOTE_OFF_RETRIES: u32 = 120;

/// The status when no audio engine takes the note.
pub const AUDITION_HINT: &str = "Notes sound once the audio engine runs: press Play once.";

/// The status when a note-off could not be sent for good.
pub const STUCK_NOTE_HINT: &str = "A note may still be sounding — press Stop.";

/// A note an editor sends straight to a destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AuditionNote {
    pub dest: u64,
    pub pitch: u8,
}

/// The notes a document sounds, the note-offs still to retry and whether
/// the "no engine" hint was shown.
#[derive(Debug, Clone, Default)]
pub struct Audition {
    held: BTreeSet<AuditionNote>,
    pending: BTreeMap<AuditionNote, u32>,
    hint_shown: bool,
}

/// Later statuses win, as in Swift where each assignment replaces the last.
fn merge(into: &mut Option<String>, s: Option<String>) {
    if s.is_some() {
        *into = s;
    }
}

impl Audition {
    /// Sounds `note`. Note-offs still pending are retried first; while the
    /// one for this very note cannot be sent, the note is not started.
    /// Returns the status to show, if any (Busy is silent).
    pub fn on(
        &mut self,
        session: &dyn DocSession,
        note: AuditionNote,
        velocity: u16,
    ) -> Option<String> {
        let mut status = self.tick(session);
        if self.pending.contains_key(&note) {
            return status;
        }
        match session.audition(note.dest, note.pitch, velocity, true) {
            Ok(true) => {
                self.held.insert(note);
            }
            Ok(false) => {
                if !self.hint_shown {
                    self.hint_shown = true;
                    status = Some(AUDITION_HINT.into());
                }
            }
            Err(VizError::Busy { .. }) => {}
            Err(e) => status = Some(error_message(&e)),
        }
        status
    }

    /// Ends `note` if it sounds; a failed note-off is retried by `tick`.
    pub fn off(&mut self, session: &dyn DocSession, note: AuditionNote) -> Option<String> {
        if !self.held.remove(&note) {
            return None;
        }
        self.send_off(session, note, NOTE_OFF_RETRIES)
    }

    /// Ends every note this document sounds.
    pub fn off_all(&mut self, session: &dyn DocSession) -> Option<String> {
        let mut status = None;
        for note in std::mem::take(&mut self.held) {
            merge(&mut status, self.send_off(session, note, NOTE_OFF_RETRIES));
        }
        status
    }

    pub fn has_pending_note_offs(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The notes sounding now.
    pub fn held(&self) -> impl Iterator<Item = AuditionNote> + '_ {
        self.held.iter().copied()
    }

    /// One retry of every pending note-off.
    pub fn tick(&mut self, session: &dyn DocSession) -> Option<String> {
        let mut status = None;
        for (note, left) in std::mem::take(&mut self.pending) {
            merge(&mut status, self.send_off(session, note, left - 1));
        }
        status
    }

    fn send_off(
        &mut self,
        session: &dyn DocSession,
        note: AuditionNote,
        attempts_left: u32,
    ) -> Option<String> {
        match session.audition(note.dest, note.pitch, 0, false) {
            Ok(_) => None,
            Err(VizError::Busy { .. }) => {
                if attempts_left > 0 {
                    self.pending.insert(note, attempts_left);
                    None
                } else {
                    Some(STUCK_NOTE_HINT.into())
                }
            }
            Err(e) => Some(error_message(&e)),
        }
    }
}

impl DocumentModel {
    /// Sounds `note` (a refused or failed one may set the status).
    pub fn audition_on(&mut self, note: AuditionNote, velocity: u16) {
        let s = self
            .editor
            .audition
            .on(self.session.as_ref(), note, velocity);
        self.audition_status(s);
    }

    /// Ends `note` if it sounds.
    pub fn audition_off(&mut self, note: AuditionNote) {
        let s = self.editor.audition.off(self.session.as_ref(), note);
        self.audition_status(s);
    }

    /// Ends every audition note, the roll's included.
    pub fn audition_off_all(&mut self) {
        self.editor.roll.sounding = None;
        let s = self.editor.audition.off_all(self.session.as_ref());
        self.audition_status(s);
    }

    /// Note-offs wait for a retry.
    pub fn has_pending_note_offs(&self) -> bool {
        self.editor.audition.has_pending_note_offs()
    }

    /// One retry of every pending note-off (part of [`DocumentModel::tick`]).
    pub(crate) fn retry_note_offs(&mut self) {
        let s = self.editor.audition.tick(self.session.as_ref());
        self.audition_status(s);
    }

    fn audition_status(&mut self, s: Option<String>) {
        if s.is_some() {
            self.status = s;
        }
    }
}
