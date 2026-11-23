//! The mixer (Swift `MixerModel`, `MixerMath`, `StripEntry`,
//! `ControlGestureModel`): one model per document, shared by every
//! placement that shows it (the Mix view, the bottom pane, the Mixer
//! window).

pub mod commands;
pub mod gesture;
pub mod layout;
pub mod math;
pub mod meters;
mod model;

use std::collections::BTreeSet;
use std::time::Instant;

use vizdaw_ffi::VizError;

pub use math::{AutomationModes, MixerMath, StripEntry};
pub use meters::{MeterLevel, Meters};
pub use model::{
    MAX_HEARD_TARGETS, MIXER_UNAVAILABLE, MixerColumn, MixerModel, MixerPlacement, RELEASE_WINDOW,
    failure_title, monitor_help, next_monitor, norm_of, reset_value, send_index,
};

use crate::alerts::AlertItem;
use crate::document::DocumentModel;

/// The document's mixer state (held by [`DocumentModel`]).
#[derive(Default)]
pub(crate) struct DocMixer {
    model: Option<Box<MixerModel>>,
    /// The model is lent out (`with_mixer`): none is made meanwhile.
    lent: bool,
    /// The document version a refused first read was at: not asked again
    /// until the document moves.
    failed_at: Option<u64>,
    /// "Mixer Unavailable" was shown for a first read.
    alerted: bool,
    /// The mixer's selected strips; the core hears every change.
    pub(crate) selected: BTreeSet<u64>,
}

impl DocumentModel {
    /// The mixer, if it was made (see [`DocumentModel::ensure_mixer`]).
    pub fn mixer(&self) -> Option<&MixerModel> {
        self.mixer.model.as_deref()
    }

    /// Makes the mixer when first asked. A busy core is asked again next
    /// time, quietly; another refusal is the alert "Mixer Unavailable"
    /// once, and is asked again once the document moves. True when the
    /// mixer exists.
    pub fn ensure_mixer(&mut self) -> bool {
        if self.mixer.model.is_some() {
            return true;
        }
        if self.mixer.lent {
            return false;
        }
        let version = self.snapshot().version;
        if self.mixer.failed_at == Some(version) {
            return false;
        }
        match MixerModel::new(self) {
            Ok(m) => {
                self.mixer.model = Some(Box::new(m));
                self.mixer.failed_at = None;
                true
            }
            Err(VizError::Busy { .. }) => false,
            Err(e) => {
                self.mixer.failed_at = Some(version);
                if !self.mixer.alerted {
                    self.mixer.alerted = true;
                    self.alert = Some(AlertItem::from_error(&e, MIXER_UNAVAILABLE));
                }
                false
            }
        }
    }

    /// Runs `f` with the mixer (made first if need be) and this document.
    /// `None` when there is no mixer.
    pub fn with_mixer<R>(
        &mut self,
        f: impl FnOnce(&mut MixerModel, &mut DocumentModel) -> R,
    ) -> Option<R> {
        if !self.ensure_mixer() {
            return None;
        }
        let mut m = self.mixer.model.take()?;
        self.mixer.lent = true;
        let r = f(&mut m, self);
        self.mixer.lent = false;
        self.mixer.model = Some(m);
        Some(r)
    }

    /// The mixer's selected strips.
    pub fn selected_strips(&self) -> &BTreeSet<u64> {
        &self.mixer.selected
    }

    /// Sets the mixer's selected strips; a change reaches the core (the
    /// surfaces show it).
    pub fn set_selected_strips(&mut self, strips: BTreeSet<u64>) {
        if strips == self.mixer.selected {
            return;
        }
        let _ = self
            .session()
            .set_surface_selection(strips.iter().copied().collect());
        self.mixer.selected = strips;
    }

    /// A mixer release waits on Busy.
    pub fn mixer_release_pending(&self) -> bool {
        self.mixer().is_some_and(MixerModel::release_pending)
    }

    /// One retry of the mixer's releases that met Busy (part of the
    /// document's tick).
    pub(crate) fn retry_mixer_releases(&mut self, now: Instant) {
        if self.mixer_release_pending() {
            self.with_mixer(|m, d| m.retry_releases(d, now));
        }
    }

    /// Ends every open mixer drag (the document closes).
    pub fn end_mixer_drags(&mut self, now: Instant) {
        if self.mixer().is_some() {
            self.with_mixer(|m, d| {
                m.end_open_drag(d, now);
                m.retry_releases(d, now);
            });
        }
    }
}
