//! Co-Producer's creative session methods (plan 8e Task 3), kept out of
//! `session.rs`: that file gains only `mod co_producer_creative;`.
//! `automation.write` runs as one step named its title, built as the
//! preview built it; the reads take the document as it stands and change
//! nothing. `co_producer_starter` (Task 4) builds a new song's sounds and
//! routing as one step. No method here writes notes (owner, 2026-11-16).

use super::co_producer_mixer::LiveLookup;
use super::{Session, edit_open_busy, records_busy};
use crate::control::ControlArg;
use crate::creative::{CoProducerCurveRow, CoProducerNotesRow, commands_for, curve_of, notes_of};
use crate::error::VizError;
use crate::guard::ffi_guard;
use crate::snapshot::DocSnapshot;
use crate::starter::{
    CoProducerStarterRow, NEEDS_A_NEW_SONG, answer, build, new_song_seq, resolve_with_eq,
};
use core_control::registry::{self, CREATIVE_IDS};
use core_model::diff::shown_name;
use core_model::history::request_label;
use core_model::selection::ViewFilter;
use std::time::Instant;

fn bad(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// Co-Producer's creative edits and reads (plan 8e Task 3).
#[uniffi::export]
impl Session {
    /// Runs one of [`CREATIVE_IDS`] as one step named its title (Ruling
    /// 1), as the preview built it ([`commands_for`]): `args` are
    /// validated by the registry and checked again; a refusal changes
    /// nothing. Inside a Co-Producer call the step is Co-Producer's, and
    /// joins the request's. `Busy` while an edit is open.
    pub fn run_creative(&self, id: String, args: Vec<ControlArg>) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            if !CREATIVE_IDS.contains(&id.as_str()) {
                let shown: String = id.chars().take(64).collect();
                return Err(bad(format!(
                    "{} is not a creative command.",
                    shown_name(&shown)
                )));
            }
            let title = registry::find(&id)
                .map(|spec| spec.title)
                .ok_or_else(|| bad(format!("{id} is not a creative command.")))?;
            let args = crate::co_producer::validated(&id, &args)?;
            let host = LiveLookup(self.id);
            let mut inner = self.lock_inner();
            if inner.edit_label.is_some() || inner.history.in_transaction() {
                return Err(edit_open_busy());
            }
            let cmds = commands_for(inner.doc(), &id, &args, &host)?;
            let before = inner.doc().clone();
            let filter = ViewFilter::default();
            let h = &mut inner.history;
            h.begin(title)?;
            let mut notes = Vec::new();
            for c in &cmds {
                match h.execute(c, &filter) {
                    Ok(r) => notes.extend(r.warnings),
                    Err(e) => {
                        let _ = h.abort();
                        return Err(e.into());
                    }
                }
            }
            h.commit()?;
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            self.release_vanished(&mut inner);
            self.resync_plugin_automation(&mut inner, Some(&before));
            self.send_mix_changes(&mut inner, &before);
            inner.snapshot_with(notes)
        })
    }

    /// Track `lane`'s notes in sequence `seq` (Ruling 3): the active
    /// take's, those starting in `[start, end)` when both are given, by
    /// start then pitch, 7-bit velocities, at most 4096. `Model` for an
    /// unknown or referenced sequence, a track not in it, or an audio
    /// track. Changes nothing.
    pub fn co_producer_midi_notes(
        &self,
        seq: u64,
        lane: u64,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<CoProducerNotesRow, VizError> {
        ffi_guard(|| {
            let mut inner = self.lock_inner();
            if inner.history.request().is_some() {
                inner.co_producer_seen = Instant::now();
            }
            notes_of(inner.doc(), seq, lane, start, end)
        })
    }

    /// `target`'s automation in sequence `seq` over `start..=end` (Ruling
    /// 7), in the target's own units, at most 4096 points. `Model` for an
    /// unknown sequence or target, `end` not after `start`, or a parameter
    /// whose plug-in is not running. Changes nothing.
    pub fn co_producer_automation(
        &self,
        seq: u64,
        target: String,
        start: u64,
        end: u64,
    ) -> Result<CoProducerCurveRow, VizError> {
        ffi_guard(|| {
            let host = LiveLookup(self.id);
            let mut inner = self.lock_inner();
            if inner.history.request().is_some() {
                inner.co_producer_seen = Instant::now();
            }
            curve_of(inner.doc(), seq, &target, start, end, &host)
        })
    }

    /// Ruling 5's build, as one acting step in this (new, untitled)
    /// document. `spec` is the JSON text, parsed again by
    /// `core_control::starter::parse`: text that is not JSON gets parse's
    /// answer for a non-object, and any refusal is `Model` with parse's
    /// sentence. Refuses ("A starter project needs a new song.") unless
    /// the document has no events, tracks or buses of its own beyond File ›
    /// New's. Every plug-in resolves by 8d Ruling 3 or falls back to a
    /// built-in ([`resolve`]), so no plug-in fails it. Any refusal of a
    /// step aborts the build and leaves the song as it was. The step is
    /// "Co-Producer: Start <Title>", or the open request's. `Busy` while
    /// an edit is open or recording.
    pub fn co_producer_starter(&self, spec: String) -> Result<CoProducerStarterRow, VizError> {
        ffi_guard(|| {
            let value = serde_json::from_str(&spec).unwrap_or(serde_json::Value::Null);
            let spec = core_control::starter::parse(&value).map_err(bad)?;
            let host = LiveLookup(self.id);
            let monitor = crate::audio::default_monitor();
            let label = request_label(&format!("start {}", spec.title))
                .map(|(_, label)| label)
                .unwrap_or_else(|_| format!("Co-Producer: Start {}", spec.title));
            let mut inner = self.lock_inner();
            if inner.edit_label.is_some() || inner.history.in_transaction() {
                return Err(edit_open_busy());
            }
            records_busy(&inner)?;
            let seq = new_song_seq(inner.doc()).ok_or_else(|| bad(NEEDS_A_NEW_SONG))?;
            if inner.history.request().is_some() {
                inner.co_producer_seen = Instant::now();
            }
            let before = inner.doc().clone();
            let mut notes = Vec::new();
            let h = &mut inner.history;
            crate::bundled::arm(h);
            // Resolved against the EQ the armed History gives new strips
            // (the catalog's lock is a leaf: it takes no session's).
            let resolved = resolve_with_eq(&spec, h.new_strip_eq(), &host);
            h.begin(&label)?;
            let built = match build(h, seq, &spec, &resolved, monitor, &mut notes) {
                Ok(b) => b,
                Err(e) => {
                    let _ = h.abort();
                    return Err(e);
                }
            };
            if let Err(e) = h.commit() {
                // An open transaction would answer Busy to every later edit.
                let _ = h.abort();
                return Err(e.into());
            }
            let undo_label = h.undo_label().unwrap_or(&label).to_string();
            inner.version += 1;
            crate::host::clear_input_failure_of(self.id);
            inner.reload_host(self.id);
            self.mark(&mut inner);
            self.send_mix_changes(&mut inner, &before);
            Ok(answer(
                inner.doc(),
                &spec,
                &resolved,
                &built,
                undo_label,
                &notes,
            ))
        })
    }
}
