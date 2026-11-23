//! A knob's or fader's drag apart from drawing (Swift `ControlDrag` and
//! `ControlGestureModel`): relative vertical drag, ⌥ ten times finer, a
//! begin only on the first real movement and a double tap that resets only
//! when no drag is open or just ended.

use std::time::{Duration, Instant};

/// The least movement, in points, before a press becomes a drag (SwiftUI's
/// `DragGesture(minimumDistance: 2)`).
pub const MIN_DRAG_DISTANCE: f64 = 2.0;
/// Points of drag for a knob's whole range.
pub const DRAG_TRAVEL: f64 = 200.0;
/// ⌥ makes a drag this many times finer.
pub const FINE_DIVISOR: f64 = 10.0;
/// After a drag ends, a double tap this soon is part of it, not a reset.
pub const SETTLE: Duration = Duration::from_millis(250);

fn unit(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// `start` moved by a drag of `translation` points (down is positive, so
/// it is negated), `travel` points for the whole range; clamped to 0…1.
pub fn drag_value(start: f64, translation: f64, travel: f64, fine: bool) -> f64 {
    if travel.is_nan() || travel <= 0.0 || !translation.is_finite() {
        return unit(start);
    }
    let delta = -translation / travel / if fine { FINE_DIVISOR } else { 1.0 };
    unit(unit(start) + delta)
}

/// One drag's state. When `fine` changes mid-drag it re-anchors (start =
/// the current value, origin = the current translation), so toggling ⌥
/// never makes the value jump.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tracker {
    start: f64,
    origin: f64,
    fine: Option<bool>,
    pub travel: f64,
}

impl Tracker {
    pub fn new(start: f64, travel: f64) -> Self {
        Tracker {
            start: unit(start),
            origin: 0.0,
            fine: None,
            travel,
        }
    }

    pub fn value(&mut self, translation: f64, fine: bool) -> f64 {
        if let Some(was) = self.fine
            && was != fine
        {
            self.start = drag_value(self.start, translation - self.origin, self.travel, was);
            self.origin = translation;
        }
        self.fine = Some(fine);
        drag_value(self.start, translation - self.origin, self.travel, fine)
    }
}

/// What a change did: whether it began the drag, and the new position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Change {
    pub began: bool,
    pub value: f64,
}

/// What a knob or fader does with its gestures. A begin comes with the
/// first change that moves, an end only after a begin, and a reset on a
/// double tap only when no drag is under way or has just ended. A press
/// without movement does nothing, so a double-click never leaves a touch.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlGesture {
    pub travel: f64,
    tracker: Option<Tracker>,
    first_translation: f64,
    last_end: Option<Instant>,
}

impl ControlGesture {
    pub fn new(travel: f64) -> Self {
        ControlGesture {
            travel,
            tracker: None,
            first_translation: 0.0,
            last_end: None,
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.tracker.is_some()
    }

    /// The drag moved to `translation` (vertical points from the press,
    /// down positive). `start` is the control's position, read only when
    /// the drag begins. `None`: nothing to send.
    pub fn changed(
        &mut self,
        translation: f64,
        fine: bool,
        start: impl FnOnce() -> f64,
    ) -> Option<Change> {
        let mut began = false;
        if self.tracker.is_none() {
            if translation == 0.0 || !translation.is_finite() {
                return None;
            }
            self.tracker = Some(Tracker::new(start(), self.travel.max(1.0)));
            self.first_translation = translation;
            began = true;
        }
        let t = self.tracker.as_mut()?;
        let value = t.value(translation - self.first_translation, fine);
        Some(Change { began, value })
    }

    /// Mouse-up: true when a drag was under way (its end is to be sent).
    pub fn ended(&mut self, now: Instant) -> bool {
        if self.tracker.take().is_none() {
            return false;
        }
        self.last_end = Some(now);
        true
    }

    /// The drag was cancelled (deactivation, the control went): ends a
    /// drag under way, once.
    pub fn cancelled(&mut self, now: Instant) -> bool {
        self.ended(now)
    }

    /// A double tap: true when it is a reset.
    pub fn double_tapped(&self, now: Instant) -> bool {
        if self.tracker.is_some() {
            return false;
        }
        !self
            .last_end
            .is_some_and(|e| now.saturating_duration_since(e) < SETTLE)
    }
}
