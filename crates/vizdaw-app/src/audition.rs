//! Audition (Swift `EditorModel+Audition`): notes an editor sends straight
//! to a destination. A failed note-on is dropped; a failed note-off is
//! retried by every [`Audition::tick`] so no note sticks.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use vizdaw_ffi::VizError;

use crate::DocumentModel;
use crate::alerts::error_message;
use crate::session::DocSession;

/// How long a note-off that meets Busy is retried (Swift: 120 ticks of a
/// 60 Hz timer). Time-based, since the poll's rate changes: a stopped
/// document polls slowly.
pub const NOTE_OFF_WINDOW: Duration = Duration::from_secs(2);

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
    /// Each waiting note-off with the time it is given up at.
    pending: BTreeMap<AuditionNote, Instant>,
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
        now: Instant,
    ) -> Option<String> {
        let mut status = self.tick(session, now);
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
    pub fn off(
        &mut self,
        session: &dyn DocSession,
        note: AuditionNote,
        now: Instant,
    ) -> Option<String> {
        if !self.held.remove(&note) {
            return None;
        }
        self.send_off(session, note, now + NOTE_OFF_WINDOW, now)
    }

    /// Ends every note this document sounds.
    pub fn off_all(&mut self, session: &dyn DocSession, now: Instant) -> Option<String> {
        let mut status = None;
        for note in std::mem::take(&mut self.held) {
            merge(
                &mut status,
                self.send_off(session, note, now + NOTE_OFF_WINDOW, now),
            );
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
    pub fn tick(&mut self, session: &dyn DocSession, now: Instant) -> Option<String> {
        let mut status = None;
        for (note, deadline) in std::mem::take(&mut self.pending) {
            merge(&mut status, self.send_off(session, note, deadline, now));
        }
        status
    }

    fn send_off(
        &mut self,
        session: &dyn DocSession,
        note: AuditionNote,
        deadline: Instant,
        now: Instant,
    ) -> Option<String> {
        match session.audition(note.dest, note.pitch, 0, false) {
            Ok(_) => None,
            Err(VizError::Busy { .. }) => {
                if now < deadline {
                    self.pending.insert(note, deadline);
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
        self.audition_on_at(note, velocity, Instant::now());
    }

    pub fn audition_on_at(&mut self, note: AuditionNote, velocity: u16, now: Instant) {
        let s = self
            .editor
            .audition
            .on(self.session.as_ref(), note, velocity, now);
        self.audition_status(s);
    }

    /// Ends `note` if it sounds.
    pub fn audition_off(&mut self, note: AuditionNote) {
        self.audition_off_at(note, Instant::now());
    }

    pub fn audition_off_at(&mut self, note: AuditionNote, now: Instant) {
        let s = self.editor.audition.off(self.session.as_ref(), note, now);
        self.audition_status(s);
    }

    /// Ends every audition note, the roll's included.
    pub fn audition_off_all(&mut self) {
        self.editor.roll.sounding = None;
        let s = self
            .editor
            .audition
            .off_all(self.session.as_ref(), Instant::now());
        self.audition_status(s);
    }

    /// Note-offs wait for a retry.
    pub fn has_pending_note_offs(&self) -> bool {
        self.editor.audition.has_pending_note_offs()
    }

    /// The poll should run fast (about 16 to 33 ms): a note-off or a
    /// drag's mouse-up waits on Busy.
    pub fn wants_fast_poll(&self) -> bool {
        self.has_pending_note_offs() || self.gesture_pending()
    }

    /// One retry of every pending note-off, as of `now` (part of
    /// [`DocumentModel::tick_at`]).
    pub(crate) fn retry_note_offs(&mut self, now: Instant) {
        let s = self.editor.audition.tick(self.session.as_ref(), now);
        self.audition_status(s);
    }

    fn audition_status(&mut self, s: Option<String>) {
        if s.is_some() {
            self.status = s;
        }
    }
}
