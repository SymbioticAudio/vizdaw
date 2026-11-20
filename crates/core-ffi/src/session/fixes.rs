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
/// (a VCA's too) or a send, −1 to 1 for a pan, else as it is (0 or 1 for mute and solo,
/// 0–1 for a plug-in parameter).
fn shown(t: AutomationTarget, v: f64) -> f64 {
    use core_model::automation::{fader_db, pan_of};
    match t {
        AutomationTarget::StripFader(_)
        | AutomationTarget::SendLevel(..)
        | AutomationTarget::VcaFader(_) => fader_db(v),
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

/// The most frames each side [`Session::zero_crossing`] looks (a second
/// at 48 kHz).
const MAX_CROSSING_RADIUS: u64 = 48_000;

/// Release fixes (plan release-fixes Task 7).
#[uniffi::export]
impl Session {
    /// The frame of `media` nearest `frame` where the channel-summed signal
    /// changes sign (s[k-1] and s[k] differ in sign, or s[k] == 0), within
    /// `radius` frames each side; ties go to the earlier frame. None found,
    /// or the media offline: `frame`. `frame` past the end clamps to the
    /// length. Unknown media, or a radius over 48_000: `VizError::Model`.
    /// Reads with CafReader as Normalize does.
    pub fn zero_crossing(&self, media: u64, frame: u64, radius: u64) -> Result<u64, VizError> {
        ffi_guard(|| {
            use core_model::ids::{AnyId, MediaId};
            if radius > MAX_CROSSING_RADIUS {
                return Err(crate::events::bad(&format!(
                    "the radius must be at most {MAX_CROSSING_RADIUS} frames"
                )));
            }
            let id = MediaId(media);
            // Under the lock: where the file is. The read is without it.
            let (file, frames, offline) = {
                let inner = self.lock_inner();
                let m = inner
                    .doc()
                    .media()
                    .get(&id)
                    .ok_or(core_model::ModelError::UnknownId(AnyId::Media(id)))?;
                (
                    inner.root(self.id).join(&m.path),
                    m.length_frames,
                    inner.offline.contains_key(&id),
                )
            };
            let frame = frame.min(frames);
            if offline {
                return Ok(frame);
            }
            // A file gone since it was checked keeps the frame, as offline.
            let Ok(r) = core_media::caf::CafReader::open(&file) else {
                return Ok(frame);
            };
            Ok(nearest_crossing(&r, frame, radius).unwrap_or(frame))
        })
    }
}

/// The frame nearest `frame` (within `radius` each side, the earlier on a
/// tie) where `r`'s channel sum is 0 or has the opposite sign of the frame
/// before; None when there is none or the read fails.
fn nearest_crossing(r: &core_media::caf::CafReader, frame: u64, radius: u64) -> Option<u64> {
    let ch = usize::from(r.info().channels.max(1));
    let last = r.info().frames.checked_sub(1)?;
    let lo = frame.saturating_sub(radius);
    let hi = frame.saturating_add(radius).min(last);
    if lo > hi {
        return None;
    }
    // From one frame before `lo`, for its sign.
    let from = lo.saturating_sub(1);
    let n = usize::try_from(hi - from + 1).ok()?;
    let mut buf = vec![0f32; n.checked_mul(ch)?];
    let got = r.read(from, &mut buf, &mut Vec::new()).ok()?;
    let sums: Vec<f32> = buf
        .chunks_exact(ch)
        .take(got)
        .map(|c| c.iter().sum())
        .collect();
    let at = |k: u64| {
        k.checked_sub(from)
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| sums.get(i).copied())
    };
    let crossing = |k: u64| {
        let Some(s) = at(k) else {
            return false;
        };
        if s == 0.0 {
            return true;
        }
        k.checked_sub(1)
            .and_then(at)
            .is_some_and(|p| p != 0.0 && (p < 0.0) != (s < 0.0) && !p.is_nan() && !s.is_nan())
    };
    (0..=radius).find_map(|d| {
        let before = frame.checked_sub(d).filter(|k| *k >= lo && crossing(*k));
        before.or_else(|| frame.checked_add(d).filter(|k| *k <= hi && crossing(*k)))
    })
}

/// Release fixes (plan release-fixes Task 8).
#[uniffi::export]
impl Session {
    /// Sets Built-in Synth destination `dest`'s sound: one
    /// `RoutingCmd::EditDestination` giving its kind the sound's channel and
    /// `default_patch = Some(Patch { program, bank: None })`, one undo step
    /// "Set Built-in Synth Sound". Every track that plays it changes
    /// (Decision 3). Another destination: `Model` "Only a Built-in Synth
    /// has these sounds."; an unknown one: `Model` (unknown ID).
    pub fn set_builtin_sound(
        &self,
        dest: u64,
        sound: crate::builtin_sound::BuiltinSound,
    ) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = self.lock_inner();
            if inner.edit_label.is_some() || inner.history.in_transaction() {
                return Err(super::edit_open_busy());
            }
            let cmd = crate::builtin_sound::sound_command(
                inner.doc(),
                core_model::ids::DestId(dest),
                sound,
            )?;
            inner.history.begin("Set Built-in Synth Sound")?;
            match inner
                .history
                .execute(&cmd, &core_model::selection::ViewFilter::default())
            {
                Ok(_) => inner.history.commit()?,
                Err(e) => {
                    inner.history.abort()?;
                    return Err(e.into());
                }
            }
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            Ok(())
        })
    }
}
