//! Touches for controls without touch sensing (spec §7.1, §8): touched by
//! their first value, released `IDLE_RELEASE` after their last (Ruling 2).

use crate::{Phase, SurfaceAction};
use core_model::automation::AutomationTarget;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// How long after its last value an untouched control is released.
pub const IDLE_RELEASE: Duration = Duration::from_millis(500);

/// The open idle touches: target → its last value and that value's time.
#[derive(Debug, Default)]
pub struct IdleTouches {
    open: BTreeMap<AutomationTarget, (f64, Instant)>,
}

impl IdleTouches {
    /// Touch(Some(v)) the first time, else Value(v).
    pub fn value(&mut self, t: AutomationTarget, v: f64, now: Instant) -> SurfaceAction {
        let phase = match self.open.insert(t, (v, now)) {
            None => Phase::Touch(Some(v)),
            Some(_) => Phase::Value(v),
        };
        SurfaceAction::Gesture(t, phase)
    }

    /// `IdleRelease(t, last value's time)` for every target idle for IDLE_RELEASE.
    pub fn tick(&mut self, now: Instant) -> Vec<SurfaceAction> {
        let mut out = Vec::new();
        self.open.retain(|&t, &mut (_, last)| {
            let idle = now.saturating_duration_since(last) >= IDLE_RELEASE;
            if idle {
                out.push(SurfaceAction::IdleRelease(t, last));
            }
            !idle
        });
        out
    }

    /// The last value sent for `t` while its idle touch is open.
    pub(crate) fn last(&self, t: AutomationTarget) -> Option<f64> {
        self.open.get(&t).map(|&(v, _)| v)
    }

    /// Closes `t`'s idle touch without a release: a real touch took it over.
    pub(crate) fn forget(&mut self, t: AutomationTarget) {
        self.open.remove(&t);
    }

    /// The targets held open.
    pub fn held(&self) -> Vec<AutomationTarget> {
        self.open.keys().copied().collect()
    }

    /// `held()`, then forgets them all.
    pub fn take_all(&mut self) -> Vec<AutomationTarget> {
        let held = self.held();
        self.open.clear();
        held
    }
}
