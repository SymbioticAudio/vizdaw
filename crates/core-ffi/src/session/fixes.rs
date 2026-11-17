//! The 26.0.0 release fixes' session methods (plan release-fixes Tasks 4,
//! 6–8), kept out of `session.rs`: that file gains only `mod fixes;` and
//! one-line hooks.

use super::{Inner, Session};
use crate::automation::TouchHolder;
use crate::error::VizError;
use crate::events::{AutomationTargetRow, automation_target_of};
use crate::guard::ffi_guard;
use core_model::automation::AutomationTarget;

/// The most targets one [`Session::heard_values`] call reads.
const MAX_HEARD_TARGETS: usize = 4096;

impl Session {
    /// False for a PluginParam its plug-in marks non-automatable: the
    /// touch then takes the static/live path, never Write (no lane).
    pub(super) fn may_write(&self, _inner: &Inner, t: AutomationTarget) -> bool {
        if !matches!(t, AutomationTarget::PluginParam(..)) {
            return true;
        }
        // A parameter the host does not list now (or a busy registry)
        // keeps the old behaviour.
        crate::automation::param_info(self.id, t).is_none_or(|p| p.automatable)
    }
}

/// `v` (normalised) in the units the mixer shows `t` in: dB for a fader
/// or a send, −1 to 1 for a pan, else as it is (0 or 1 for mute and solo,
/// 0–1 for a plug-in parameter).
fn shown(t: AutomationTarget, v: f64) -> f64 {
    use core_model::automation::{fader_db, pan_of};
    match t {
        AutomationTarget::StripFader(_) | AutomationTarget::SendLevel(..) => fader_db(v),
        AutomationTarget::StripPan(_) => pan_of(v),
        _ => v,
    }
}

/// Release fixes (plan release-fixes Task 4).
#[uniffi::export]
impl Session {
    /// Releases every touch the control channel holds (Decision 2).
    /// Returns how many were released. Ends an open static edit as a
    /// release does, so the step commits.
    pub fn release_channel_touches(&self) -> Result<u32, VizError> {
        ffi_guard(|| {
            let mut inner = self.lock_inner();
            let held: Vec<AutomationTarget> = inner
                .touches
                .iter()
                .filter(|(_, x)| x.holders.has(TouchHolder::Channel))
                .map(|(t, _)| *t)
                .collect();
            for t in &held {
                self.release_from(&mut inner, *t, TouchHolder::Channel, None);
            }
            Ok(u32::try_from(held.len()).unwrap_or(u32::MAX))
        })
    }

    /// While playing: each target's heard value (surface_existing +
    /// surface_heard, Ruling 26 of plan 5e-1), in the units the mixer
    /// shows it in (dB for a fader or send, −1 to 1 for a pan, else
    /// normalised). Stopped: all None. An unknown target is None; more
    /// than 4096 targets is Err(Invalid).
    pub fn heard_values(
        &self,
        targets: Vec<AutomationTargetRow>,
    ) -> Result<Vec<Option<f64>>, VizError> {
        ffi_guard(|| {
            if targets.len() > MAX_HEARD_TARGETS {
                return Err(crate::events::bad(&format!(
                    "at most {MAX_HEARD_TARGETS} targets"
                )));
            }
            let mut inner = self.lock_inner();
            let Some(cached) = self.surface_existing(&mut inner) else {
                return Ok(vec![None; targets.len()]);
            };
            let inner: &Inner = &inner;
            Ok(targets
                .into_iter()
                .map(|row| {
                    let t = automation_target_of(row);
                    let known = !matches!(t, AutomationTarget::VelocityScale(_))
                        && core_model::automation::target_exists(inner.doc(), t);
                    known.then(|| shown(t, self.surface_heard(inner, t, Some(&cached))))
                })
                .collect())
        })
    }
}

/// Release fixes (plan release-fixes Task 6).
#[uniffi::export]
impl Session {
    /// The document's output paths in id order, for an audio track's
    /// output picker.
    pub fn output_paths(&self) -> Result<Vec<crate::mixer::OutputPathRow>, VizError> {
        ffi_guard(|| {
            let inner = self.lock_inner();
            Ok(inner
                .doc()
                .outputs()
                .values()
                .map(|p| crate::mixer::OutputPathRow {
                    id: p.id.0,
                    name: p.name.clone(),
                })
                .collect())
        })
    }
}
