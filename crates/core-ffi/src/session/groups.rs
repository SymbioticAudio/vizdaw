//! Fader groups on the gesture path (groups-vcas-transport Task 2, Rulings
//! 2 and 5): the one path of the mouse, the channel, the surfaces and
//! learn fans a gesture out to the strip's group, unless it is alone.
//!
//! - A fader touch of a grouped strip whose base is finite also touches
//!   each other member's fader, with the same hand and no value. Members
//!   at −∞, and members any hand holds already (Review Focus 2), stay out.
//! - Each leader value moves every member by the same clamped dB from the
//!   bases taken at the touch ([`group_delta`]), the leader included, so
//!   the leader stops where the group stops (Review Focus 1).
//! - Releasing the leader releases its followers; the shared static edit
//!   is "Move Group ‹name›".
//! - A mute or solo gesture goes to every linked member, with the same
//!   value.
//!
//! Each member takes the path its own strip gives (Ruling 5): a writing
//! member writes its own lane, a static one joins the static edit.

use super::{Inner, Session};
use crate::automation::TouchHolder;
use crate::error::VizError;
use core_model::automation::{AutomationTarget, fader_db, fader_norm};
use core_model::groups::{Link, group_delta, group_of, linked};
use core_model::ids::{GroupId, StripId};
use core_model::mixer::GAIN_FLOOR_DB;
use core_surface::Phase;

/// A group drag in progress, kept under its leader's fader target.
#[derive(Debug, Clone)]
pub(crate) struct GroupDrag {
    /// The group dragged.
    group: GroupId,
    /// The leader's fader in dB at the touch.
    leader_base_db: f64,
    /// The other members' faders and their dB at the touch (F12: each
    /// member's own fader, as heard).
    followers: Vec<(AutomationTarget, f64)>,
    /// The hand that touched the followers, which releases them.
    holder: TouchHolder,
}

/// True for a fader level a group move measures from (Task 1's rule):
/// finite and above −∞.
fn finite_db(db: f64) -> bool {
    db.is_finite() && db > GAIN_FLOOR_DB
}

impl Session {
    /// A gesture phase on `t` from holder `h`, fanned out to `t`'s group
    /// unless `alone` (see the module doc); else as
    /// [`Session::gesture_one`].
    pub(crate) fn gesture_held(
        &self,
        inner: &mut Inner,
        t: AutomationTarget,
        phase: Phase,
        h: TouchHolder,
        host_ns: Option<u64>,
        alone: bool,
    ) -> Result<(), VizError> {
        if alone {
            return self.gesture_one(inner, t, phase, h, host_ns);
        }
        match t {
            AutomationTarget::StripFader(strip) => {
                self.fader_gesture(inner, strip, phase, h, host_ns)
            }
            AutomationTarget::StripMute(strip) => {
                self.linked_gesture(inner, strip, Link::Mute, phase, h, host_ns)
            }
            AutomationTarget::StripSolo(strip) => {
                self.linked_gesture(inner, strip, Link::Solo, phase, h, host_ns)
            }
            _ => self.gesture_one(inner, t, phase, h, host_ns),
        }
    }

    /// A fader gesture: a group drag starts at the leader's touch, moves
    /// with its values and ends with its release ([`Session::end_group_drag`]).
    fn fader_gesture(
        &self,
        inner: &mut Inner,
        strip: StripId,
        phase: Phase,
        h: TouchHolder,
        host_ns: Option<u64>,
    ) -> Result<(), VizError> {
        let t = AutomationTarget::StripFader(strip);
        let value = match phase {
            Phase::Release => return self.gesture_one(inner, t, phase, h, host_ns),
            Phase::Touch(v) => v,
            Phase::Value(v) => Some(v),
        };
        // A refused target or value is refused as a single gesture's.
        if value.is_some_and(|v| !v.is_finite()) || self.check_gesture(inner, t).is_err() {
            return self.gesture_one(inner, t, phase, h, host_ns);
        }
        if inner.touches.contains_key(&t) {
            let (Some(v), Some(drag)) = (value, inner.group_drags.get(&t).cloned()) else {
                return self.gesture_one(inner, t, phase, h, host_ns);
            };
            return self.move_group(inner, t, &drag, v, phase, h, host_ns);
        }
        let Some(drag) = self.new_drag(inner, strip, h) else {
            return self.gesture_one(inner, t, phase, h, host_ns);
        };
        // Kept first, so the leader's first value opens "Move Group".
        inner.group_drags.insert(t, drag.clone());
        let (leader_phase, delta) = match (phase, value) {
            (_, None) => (phase, 0.0),
            (_, Some(v)) => {
                let delta = Self::drag_delta(&drag, v);
                let to = fader_norm(drag.leader_base_db + delta);
                let p = match phase {
                    Phase::Value(_) => Phase::Value(to),
                    _ => Phase::Touch(Some(to)),
                };
                (p, delta)
            }
        };
        if let Err(e) = self.gesture_one(inner, t, leader_phase, h, host_ns) {
            inner.group_drags.remove(&t);
            return Err(e);
        }
        let mut kept = Vec::with_capacity(drag.followers.len());
        for (f, base) in &drag.followers {
            if self
                .gesture_one(inner, *f, Phase::Touch(None), h, host_ns)
                .is_ok()
            {
                kept.push((*f, *base));
            }
        }
        if let Some(d) = inner.group_drags.get_mut(&t) {
            d.followers = kept.clone();
        }
        if value.is_some() {
            for (f, base) in kept {
                let _ =
                    self.gesture_one(inner, f, Phase::Value(fader_norm(base + delta)), h, host_ns);
            }
        }
        Ok(())
    }

    /// The drag a touch of `strip`'s fader starts, if it is grouped, its
    /// base is finite and another member would follow: one at a finite
    /// level that no hand holds (Review Focus 2).
    fn new_drag(&self, inner: &Inner, strip: StripId, h: TouchHolder) -> Option<GroupDrag> {
        let doc = inner.doc();
        let group = group_of(doc.mixer(), strip)?;
        let db = |s: StripId| fader_db(self.heard_value(inner, AutomationTarget::StripFader(s)));
        let leader_base_db = Some(db(strip)).filter(|b| finite_db(*b))?;
        let followers: Vec<(AutomationTarget, f64)> = group
            .members
            .iter()
            .copied()
            .filter(|m| *m != strip && doc.mixer().strip(*m).is_ok())
            .filter(|m| {
                !inner
                    .touches
                    .contains_key(&AutomationTarget::StripFader(*m))
            })
            .map(|m| (AutomationTarget::StripFader(m), db(m)))
            .filter(|(_, b)| finite_db(*b))
            .collect();
        if followers.is_empty() {
            return None;
        }
        Some(GroupDrag {
            group: group.id,
            leader_base_db,
            followers,
            holder: h,
        })
    }

    /// The clamped dB the group moves by when its leader is at
    /// normalised `v`, measured from the bases at the touch.
    fn drag_delta(drag: &GroupDrag, v: f64) -> f64 {
        let bases: Vec<f64> = std::iter::once(drag.leader_base_db)
            .chain(drag.followers.iter().map(|(_, b)| *b))
            .collect();
        group_delta(&bases, fader_db(v) - drag.leader_base_db)
    }

    /// A value `v` of leader `t` (a value, or a touch at a value): every
    /// touched member moves to its base plus the clamped delta.
    #[allow(clippy::too_many_arguments)]
    fn move_group(
        &self,
        inner: &mut Inner,
        t: AutomationTarget,
        drag: &GroupDrag,
        v: f64,
        phase: Phase,
        h: TouchHolder,
        host_ns: Option<u64>,
    ) -> Result<(), VizError> {
        let delta = Self::drag_delta(drag, v);
        let to = fader_norm(drag.leader_base_db + delta);
        let leader = match phase {
            Phase::Touch(_) => Phase::Touch(Some(to)),
            _ => Phase::Value(to),
        };
        self.gesture_one(inner, t, leader, h, host_ns)?;
        for (f, base) in &drag.followers {
            // Only while the drag's hand still holds it: a follower that
            // was let go (or went) and that another hand took since
            // follows that hand alone (Review Focus 2).
            if inner
                .touches
                .get(f)
                .is_some_and(|x| x.holders.has(drag.holder))
            {
                let _ = self.gesture_one(
                    inner,
                    *f,
                    Phase::Value(fader_norm(base + delta)),
                    drag.holder,
                    host_ns,
                );
            }
        }
        Ok(())
    }

    /// A mute or solo gesture: the same phase on `strip`'s target and on
    /// each member `link` joins to it. A release goes to every member, so
    /// a link switched off mid-touch leaves no member held.
    fn linked_gesture(
        &self,
        inner: &mut Inner,
        strip: StripId,
        link: Link,
        phase: Phase,
        h: TouchHolder,
        host_ns: Option<u64>,
    ) -> Result<(), VizError> {
        let target = |s: StripId| match link {
            Link::Mute => AutomationTarget::StripMute(s),
            Link::Solo => AutomationTarget::StripSolo(s),
        };
        let m = inner.doc().mixer();
        let others: Vec<StripId> = match phase {
            Phase::Release => group_of(m, strip)
                .map(|g| g.members.iter().copied().filter(|x| *x != strip).collect())
                .unwrap_or_default(),
            _ => linked(m, strip, link).into_iter().skip(1).collect(),
        };
        self.gesture_one(inner, target(strip), phase, h, host_ns)?;
        for s in others {
            let _ = self.gesture_one(inner, target(s), phase, h, host_ns);
        }
        Ok(())
    }

    /// The drag led by `t` ends (its touch was released by its last
    /// hand, or went): its followers are released by the hand that took
    /// them.
    pub(super) fn end_group_drag(
        &self,
        inner: &mut Inner,
        t: AutomationTarget,
        host_ns: Option<u64>,
    ) {
        let Some(drag) = inner.group_drags.remove(&t) else {
            return;
        };
        for (f, _) in drag.followers {
            self.release_from(inner, f, drag.holder, host_ns);
        }
    }

    /// The label of the static edit `t` opens: "Move Group ‹name›" for a
    /// member of a group drag, else the control's own.
    pub(super) fn static_label_of(&self, inner: &Inner, t: AutomationTarget) -> String {
        let group = inner.group_drags.get(&t).map(|d| d.group).or_else(|| {
            inner
                .group_drags
                .values()
                .find(|d| d.followers.iter().any(|(f, _)| *f == t))
                .map(|d| d.group)
        });
        match group.and_then(|g| inner.doc().mixer().group(g).ok()) {
            Some(g) => crate::mixer_chores::move_group_label(&g.name),
            None => crate::automation::static_label(t).to_string(),
        }
    }
}

#[cfg(feature = "test-hooks")]
impl Session {
    /// A gesture from control surface `surface` (below 16), as a surface's
    /// action reaches the gesture path (test hook; preflight F4: the
    /// surface's holder is internal). Not exported to Swift.
    pub fn debug_surface_gesture(
        &self,
        surface: u32,
        target: crate::events::AutomationTargetRow,
        phase: crate::automation::GesturePhaseRow,
        alone: bool,
    ) -> Result<(), VizError> {
        crate::guard::ffi_guard(|| {
            let id = crate::automation::SurfaceId::new(surface).ok_or_else(|| VizError::Model {
                message: format!("No surface {surface}."),
            })?;
            let mut inner = self.lock_inner();
            self.gesture_held(
                &mut inner,
                crate::events::automation_target_of(target),
                super::phase_of(phase),
                TouchHolder::Surface(id),
                None,
                alone,
            )
        })
    }
}
