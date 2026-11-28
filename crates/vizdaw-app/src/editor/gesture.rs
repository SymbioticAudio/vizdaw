//! Drags as one undo step (Swift `EditorModel+Gestures`): a gesture opens
//! an edit once the pointer leaves the dead zone, each move re-runs the
//! drag's full target inside it, and the mouse-up closes it as one undo
//! step, or none. Busy is waited out: an intermediate target is dropped,
//! a final one or an abort is retried by [`DocumentModel::tick`].

use std::collections::BTreeSet;

use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::EditCommand;

use crate::DocumentModel;
use crate::select_modify::model::ModifyRequest;
use crate::select_modify::{GestureOrigin, exact_request};

/// The status when a drag's track went away under it.
pub const LANE_REMOVED: &str = "The track was removed; the drag was cancelled.";

/// A drag in progress: one undo step named `label`.
#[derive(Debug, Clone, PartialEq)]
pub struct Gesture {
    pub label: String,
    /// The gesture that opened it; only note and block drags go exact.
    pub origin: GestureOrigin,
    /// The `(seq, lane)` tracks it edits; it is cancelled when one
    /// disappears.
    pub lanes: BTreeSet<(u64, u64)>,
    /// The target the document shows now (`None`: the start state).
    pub applied: Option<EditCommand>,
    /// The mouse-up target, until it has been applied and the edit closed.
    pub pending_final: Option<EditCommand>,
    /// Mouse-up came; `tick` retries what is left.
    pub finishing: bool,
    /// An intermediate edit failed, so `applied` was forgotten: the next
    /// target is always sent, even one equal to the forgotten one, and the
    /// document may no longer be at the start state.
    pub(crate) uncertain: bool,
    /// The abort met Busy: `tick` retries it, and nothing else touches the
    /// gesture meanwhile.
    pub(crate) reverting: bool,
    /// Which gesture this is: each one begun gets the next number, so a
    /// view can tell its own drag from another's.
    pub serial: u64,
    /// Exact Mode: the panel opens once the revert has succeeded.
    pub(crate) then_exact: Option<ModifyRequest>,
}

impl Gesture {
    fn new(label: &str, lanes: Vec<(u64, u64)>, origin: GestureOrigin, serial: u64) -> Self {
        Gesture {
            serial,
            label: label.to_owned(),
            origin,
            then_exact: None,
            lanes: lanes.into_iter().collect(),
            applied: None,
            pending_final: None,
            finishing: false,
            uncertain: false,
            reverting: false,
        }
    }
}

impl DocumentModel {
    /// The open drag, if any.
    pub fn gesture(&self) -> Option<&Gesture> {
        self.editor.gesture()
    }

    /// The open drag's serial (see [`Gesture::serial`]).
    pub fn gesture_serial(&self) -> Option<u64> {
        self.editor.gesture().map(|g| g.serial)
    }

    /// Opens a gesture once the pointer left the dead zone, touching the
    /// `(seq, lane)` tracks `lanes`. False when one is already open or the
    /// session refused (the refusal is the status).
    pub fn begin_gesture(&mut self, label: &str, lanes: Vec<(u64, u64)>) -> bool {
        self.begin_gesture_from(label, lanes, GestureOrigin::Other)
    }

    /// [`begin_gesture`](Self::begin_gesture) for a drag from `origin`:
    /// in Exact Mode, note and block drags open the panel instead of
    /// applying.
    pub fn begin_gesture_from(
        &mut self,
        label: &str,
        lanes: Vec<(u64, u64)>,
        origin: GestureOrigin,
    ) -> bool {
        if self.editor.gesture.is_some() {
            return false;
        }
        match self.session.begin_edit(label) {
            Ok(()) => {
                self.editor.gestures_begun += 1;
                let serial = self.editor.gestures_begun;
                self.editor.gesture = Some(Gesture::new(label, lanes, origin, serial));
                true
            }
            Err(e) => {
                self.report(&e);
                false
            }
        }
    }

    /// The tracks the open gesture touches now (a drag that changed its
    /// target track); it is cancelled when one of them disappears.
    pub fn set_gesture_lanes(&mut self, lanes: Vec<(u64, u64)>) {
        if let Some(g) = &mut self.editor.gesture {
            g.lanes = lanes.into_iter().collect();
        }
    }

    /// The pointer moved; `target` is the full change from the drag's
    /// start (`None`: none, which rewinds). Busy is dropped (the next move
    /// resends, even the same target); a refused target leaves the start
    /// state on screen, silently.
    pub fn update_gesture(&mut self, target: Option<EditCommand>) {
        let Some(mut g) = self.editor.gesture.clone() else {
            return;
        };
        if g.finishing || g.reverting || (target == g.applied && !g.uncertain) {
            return;
        }
        let Some(target) = target else {
            self.rewind(&mut g);
            self.keep(g);
            return;
        };
        let session = self.session.clone();
        match session.edit_gesture(target.clone()) {
            Ok(s) => {
                self.apply(s);
                g.applied = Some(target);
                g.uncertain = false;
            }
            Err(VizError::Busy { .. }) => {
                g.applied = None;
                g.uncertain = true;
            }
            Err(_) => {
                // Refused: what the document shows is unknown; read it.
                g.applied = None;
                g.uncertain = true;
                if let Ok(s) = session.snapshot() {
                    self.apply(s);
                }
            }
        }
        self.keep(g);
    }

    /// Mouse-up: `final_` becomes the gesture's content and the edit closes
    /// as one undo step; Busy leaves it pending for [`tick`](Self::tick).
    /// A refused final cancels the drag and its message is the status.
    /// In Exact Mode a note or block drag is put back instead, and once
    /// the revert succeeded (at once, or on a later tick after Busy) the
    /// panel opens with its amounts; a failed revert opens none.
    pub fn end_gesture(&mut self, final_: Option<EditCommand>) {
        let exact = self.exact_mode();
        let Some(g) = &mut self.editor.gesture else {
            return;
        };
        if g.reverting {
            return;
        }
        if exact
            && !g.finishing
            && let Some(r) = final_
                .as_ref()
                .and_then(|f| exact_request(f, &g.lanes, g.origin))
        {
            // The drag was a preview: put it back (before the panel takes
            // its baseline), then ask instead of applying.
            g.then_exact = Some(r);
            self.revert_gesture();
            return;
        }
        g.finishing = true;
        g.pending_final = final_;
        self.finish();
    }

    /// Escape, or the gesture's tracks vanished: the start state comes back
    /// and the edit closes without an undo step. Busy keeps the gesture
    /// reverting for [`tick`](Self::tick) to retry, so an open session
    /// edit never outlives its gesture. Whether the start state is back.
    /// An Exact Mode drag cancelled so opens no panel, even one whose
    /// revert still waits on Busy.
    pub fn cancel_gesture(&mut self) -> bool {
        if let Some(g) = &mut self.editor.gesture {
            g.then_exact = None;
        }
        self.revert_gesture()
    }

    /// Puts the start state back and closes the edit; then, when the
    /// gesture holds an Exact Mode request, opens the panel for it. Busy
    /// keeps the gesture (and its request) reverting for the tick.
    fn revert_gesture(&mut self) -> bool {
        let Some(mut g) = self.editor.gesture.clone() else {
            return false;
        };
        let session = self.session.clone();
        match session.abort_edit() {
            Ok(s) => {
                self.editor.gesture = None;
                self.apply(s);
                self.refresh_selection();
                if let Some(r) = g.then_exact.take() {
                    self.request_exact(r);
                }
                true
            }
            Err(VizError::Busy { .. }) => {
                g.reverting = true;
                self.editor.gesture = Some(g);
                false
            }
            Err(e) => {
                self.editor.gesture = None;
                self.report(&e);
                self.refresh_selection();
                false
            }
        }
    }

    /// Retries a note-off, a mouse-up or an Escape that met Busy (called
    /// every poll; calls no session when nothing waits).
    pub fn tick(&mut self) {
        self.tick_at(std::time::Instant::now());
    }

    /// [`tick`](Self::tick) as of `now` (the note-off deadlines).
    pub fn tick_at(&mut self, now: std::time::Instant) {
        self.retry_note_offs(now);
        self.retry_mixer_releases(now);
        let Some(g) = &self.editor.gesture else {
            return;
        };
        if g.reverting {
            self.revert_gesture();
        } else if g.finishing {
            self.finish();
        }
    }

    /// A mouse-up or Escape waits on Busy.
    pub(crate) fn gesture_pending(&self) -> bool {
        self.editor
            .gesture
            .as_ref()
            .is_some_and(|g| g.reverting || g.finishing)
    }

    /// A new snapshot: a gesture whose tracks left it is cancelled.
    pub(crate) fn after_sync(&mut self) {
        let Some(g) = &self.editor.gesture else {
            return;
        };
        let alive = |l: &(u64, u64)| {
            self.snapshot()
                .lane_revs
                .iter()
                .any(|r| (r.seq, r.lane) == *l)
        };
        if g.lanes.iter().all(alive) {
            return;
        }
        self.cancel_gesture();
        self.status = Some(LANE_REMOVED.into());
    }

    /// Stores `g` back, unless applying a snapshot cancelled the gesture.
    /// A cancel that met Busy while applying marked the stored gesture
    /// reverting: `g`, cloned before, keeps that mark.
    fn keep(&mut self, mut g: Gesture) {
        if let Some(stored) = &self.editor.gesture {
            g.reverting |= stored.reverting;
            self.editor.gesture = Some(g);
        }
    }

    /// Back to the start state: the edit is aborted and opened again.
    fn rewind(&mut self, g: &mut Gesture) {
        if g.applied.is_none() && !g.uncertain {
            return;
        }
        let session = self.session.clone();
        let result = session.abort_edit().and_then(|s| {
            self.apply(s);
            // Applying may have cancelled the gesture (its track vanished):
            // reopening the edit then would leave one open with no gesture.
            if self.editor.gesture.is_none() {
                return Ok(());
            }
            session.begin_edit(&g.label)?;
            g.applied = None;
            g.uncertain = false;
            Ok(())
        });
        if let Err(e) = result {
            self.report(&e);
        }
    }

    fn finish(&mut self) {
        let Some(mut g) = self.editor.gesture.clone() else {
            return;
        };
        if g.pending_final.is_none() && (g.applied.is_some() || g.uncertain) {
            self.cancel_gesture();
            return;
        }
        let session = self.session.clone();
        if let Some(cmd) = g.pending_final.clone()
            && (Some(&cmd) != g.applied.as_ref() || g.uncertain)
        {
            match session.edit_gesture(cmd.clone()) {
                Ok(s) => {
                    self.apply(s);
                    g.applied = Some(cmd);
                    g.uncertain = false;
                    // Applying cancelled the gesture (its track vanished),
                    // or is still cancelling it (the abort met Busy): it
                    // never commits.
                    match &self.editor.gesture {
                        None => return,
                        Some(stored) if stored.reverting => return,
                        Some(_) => {}
                    }
                    self.editor.gesture = Some(g);
                }
                Err(VizError::Busy { .. }) => return,
                Err(e) => {
                    self.cancel_gesture();
                    self.report(&e);
                    return;
                }
            }
        }
        match session.end_edit() {
            Ok(s) => {
                self.editor.gesture = None;
                self.apply(s);
                self.refresh_selection();
            }
            Err(VizError::Busy { .. }) => {}
            Err(e) => {
                self.editor.gesture = None;
                self.report(&e);
                if let Ok(s) = session.abort_edit() {
                    self.apply(s);
                }
            }
        }
    }
}
