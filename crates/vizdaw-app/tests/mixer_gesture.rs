//! The mixer's moves are gestures on the core: Swift `MixerGestureTests`,
//! the drag tracker (`ControlDrag.Tracker`, `ControlGestureModel`), typed
//! values from `MixerStripTests`, and the slice's own drag rules (a bare
//! press, a Busy release, the last placement).

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use core_model::automation::{fader_db, fader_norm, pan_norm};
use support::{FakeSession, mixer_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::mixer::gesture::{ControlGesture, DRAG_TRAVEL, FINE_DIVISOR, SETTLE, Tracker};
use vizdaw_app::mixer::{MixerMath, MixerModel, MixerPlacement, RELEASE_WINDOW, StripEntry};
use vizdaw_app::store::DocumentStore;
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::{AutomationModeRow, AutomationStateRow, GesturePhaseRow};
use vizdaw_ffi::events::AutomationTargetRow as T;

const FADER: T = T::StripFader { strip: 1 };
const PAN: T = T::StripPan { strip: 1 };
const THROW: f64 = 160.0;

fn make() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(mixer_fake());
    let mut d = DocumentModel::new(f.clone()).unwrap();
    assert!(d.ensure_mixer());
    (f, d)
}

fn mx<R>(d: &mut DocumentModel, f: impl FnOnce(&mut MixerModel, &mut DocumentModel) -> R) -> R {
    d.with_mixer(f).expect("a mixer")
}

fn phases(f: &FakeSession) -> Vec<GesturePhaseRow> {
    f.gestures().iter().map(|g| g.1).collect()
}

fn releases(f: &FakeSession) -> Vec<T> {
    f.gestures()
        .iter()
        .filter(|g| g.1 == GesturePhaseRow::Release)
        .map(|g| g.0)
        .collect()
}

fn model_err(m: &str) -> VizError {
    VizError::Model { message: m.into() }
}

fn busy(m: &str) -> VizError {
    VizError::Busy { message: m.into() }
}

fn touch(v: Option<f64>) -> GesturePhaseRow {
    GesturePhaseRow::Touch { value: v }
}

fn value(v: f64) -> GesturePhaseRow {
    GesturePhaseRow::Value { value: v }
}

const RELEASE: GesturePhaseRow = GesturePhaseRow::Release;

fn gain(d: &DocumentModel, strip: u64) -> f64 {
    let m = d.mixer().unwrap();
    m.gain_db(m.strip(strip).unwrap())
}

// The drag tracker (Swift `GeometryTests`).

#[test]
fn a_fine_toggle_mid_drag_does_not_jump() {
    let mut t = Tracker::new(0.5, 200.0);
    let before = t.value(-50.0, false);
    assert!((before - 0.75).abs() < 1e-9);
    let after = t.value(-50.0, true);
    assert!((after - before).abs() < 1e-9);
    let further = t.value(-70.0, true);
    assert!((further - (before + 20.0 / 200.0 / FINE_DIVISOR)).abs() < 1e-9);
    let coarse = t.value(-70.0, false);
    assert!((coarse - further).abs() < 1e-9);
    assert!((t.value(-90.0, false) - (further + 0.1)).abs() < 1e-9);
}

#[test]
fn a_cancelled_drag_still_ends_once() {
    let now = Instant::now();
    let mut g = ControlGesture::new(DRAG_TRAVEL);
    assert!(!g.cancelled(now));
    let c = g.changed(-10.0, false, || 0.5).unwrap();
    assert!(c.began && c.value == 0.5);
    assert!(g.cancelled(now));
    assert!(!g.is_dragging());
    assert!(!g.ended(now));
    assert!(!g.cancelled(now));
}

#[test]
fn a_click_without_movement_sends_no_gesture() {
    let t0 = Instant::now();
    let mut g = ControlGesture::new(DRAG_TRAVEL);
    assert_eq!(
        g.changed(0.0, false, || 0.5),
        None,
        "a press that never moves"
    );
    assert!(!g.ended(t0));
    assert!(g.double_tapped(t0), "a double tap after it resets");
    let c = g.changed(-10.0, false, || 0.5).unwrap();
    assert!(c.began);
    assert!(!g.double_tapped(t0), "not while dragging");
    assert!(g.ended(t0));
    assert!(
        !g.double_tapped(t0 + Duration::from_millis(100)),
        "the tap that closes a drag"
    );
    assert!(g.double_tapped(t0 + SETTLE + Duration::from_millis(1)));
    let c = g.changed(-10.0, false, || 0.5).unwrap();
    assert!(c.began, "a new drag begins again");
}

// Swift `MixerGestureTests`.

#[test]
fn a_fader_drag_is_a_gesture_on_the_core() {
    let (f, mut d) = make();
    let now = Instant::now();
    mx(&mut d, |m, d| {
        m.touch(d, FADER, false);
        m.move_to(d, FADER, 0.5, now);
        m.move_to(d, FADER, 0.4, now);
        m.release(d, FADER, now);
    });
    assert_eq!(
        phases(&f),
        [touch(None), value(0.5), value(0.4), RELEASE],
        "mouse-down sends no value: the core starts from the value heard now"
    );
    assert!(f.gestures().iter().all(|g| g.0 == FADER && !g.2));
    assert!(f.calls().is_empty(), "no direct edit");
}

#[test]
fn clicks_are_touch_value_and_release() {
    let (f, mut d) = make();
    let now = Instant::now();
    mx(&mut d, |m, d| m.toggle_mute(d, 1, now));
    assert_eq!(phases(&f), [touch(Some(1.0)), value(1.0), RELEASE]);
    assert!(
        f.gestures()
            .iter()
            .all(|g| g.0 == T::StripMute { strip: 1 })
    );
    f.clear_gestures();
    mx(&mut d, |m, d| m.toggle_mute(d, 1, now));
    assert_eq!(
        phases(&f)[0],
        touch(Some(0.0)),
        "muted now: the next click unmutes"
    );
    f.clear_gestures();
    mx(&mut d, |m, d| m.toggle_solo(d, 1, now));
    assert_eq!(f.gestures()[0].0, T::StripSolo { strip: 1 });
}

#[test]
fn a_touched_control_shows_its_live_value() {
    let (_f, mut d) = make();
    let now = Instant::now();
    assert_eq!(gain(&d, 1), 0.0);
    mx(&mut d, |m, d| m.touch(d, FADER, false));
    assert!(d.mixer().unwrap().is_touched(FADER));
    assert_eq!(gain(&d, 1), 0.0, "touched, still showing what it showed");
    mx(&mut d, |m, d| m.move_to(d, FADER, fader_norm(-6.0), now));
    assert!((gain(&d, 1) + 6.0).abs() < 1e-9, "the hand's value");
    mx(&mut d, |m, d| m.release(d, FADER, now));
    assert!(!d.mixer().unwrap().is_touched(FADER));
    let m = d.mixer().unwrap();
    assert_eq!(
        gain(&d, 1),
        m.strip(1).unwrap().gain_db,
        "the snapshot's again"
    );
    mx(&mut d, |m, d| {
        m.touch(d, PAN, false);
        m.move_to(d, PAN, 0.25, now);
    });
    let m = d.mixer().unwrap();
    assert_eq!(m.pan(m.strip(1).unwrap()), -0.5);
}

#[test]
fn a_playing_fader_shows_its_heard_value() {
    let (f, mut d) = make();
    let now = Instant::now();
    d.ensure_mixer();
    f.set_heard([(FADER, -12.0)].into());
    f.set_playing(true);
    let _ = d.poll_transport();
    mx(&mut d, |m, d| m.poll_automation(d));
    assert_eq!(gain(&d, 1), -12.0, "the heard value");
    assert!(f.heard_calls().last().unwrap().contains(&FADER));
    mx(&mut d, |m, d| {
        m.touch(d, FADER, false);
        m.move_to(d, FADER, fader_norm(-3.0), now);
    });
    assert!((gain(&d, 1) + 3.0).abs() < 1e-9, "a live touch wins");
    mx(&mut d, |m, d| m.release(d, FADER, now));
    assert_eq!(gain(&d, 1), -12.0);
    f.set_playing(false);
    let _ = d.poll_transport();
    let calls = f.heard_calls().len();
    mx(&mut d, |m, d| m.poll_automation(d));
    let m = d.mixer().unwrap();
    assert_eq!(
        gain(&d, 1),
        m.strip(1).unwrap().gain_db,
        "the snapshot's value again"
    );
    assert_eq!(
        f.heard_calls().len(),
        calls,
        "nothing is read while stopped"
    );
}

fn touched(targets: Vec<T>, values: Vec<f64>) -> AutomationStateRow {
    AutomationStateRow {
        modes: vec![],
        touched: targets,
        touched_values: values,
        suspended: false,
        writing: false,
    }
}

#[test]
fn rings_follow_the_cores_touched_targets() {
    let (f, mut d) = make();
    let pan2 = T::StripPan { strip: 2 };
    f.set_automation_state(touched(vec![pan2], vec![0.5]));
    mx(&mut d, |m, d| m.poll_automation(d));
    assert!(
        d.mixer().unwrap().is_touched(pan2),
        "a channel or plugin touch has a ring"
    );
    f.set_automation_state(touched(vec![], vec![]));
    mx(&mut d, |m, d| m.poll_automation(d));
    assert!(!d.mixer().unwrap().is_touched(pan2));
}

#[test]
fn followers_show_the_cores_values() {
    let (f, mut d) = make();
    let fader2 = T::StripFader { strip: 2 };
    f.set_automation_state(touched(vec![fader2], vec![fader_norm(-9.0)]));
    mx(&mut d, |m, d| m.poll_automation(d));
    assert!(
        (gain(&d, 2) + 9.0).abs() < 1e-9,
        "a group drag's follower moves"
    );
}

#[test]
fn polling_keeps_the_last_touched_set_when_the_core_is_busy() {
    let (f, mut d) = make();
    let pan2 = T::StripPan { strip: 2 };
    f.set_automation_state(touched(vec![pan2], vec![0.5]));
    mx(&mut d, |m, d| m.poll_automation(d));
    f.set_call_error(
        "automation_state",
        Some(busy("Finish the open edit first.")),
    );
    f.set_automation_state(touched(vec![], vec![]));
    mx(&mut d, |m, d| m.poll_automation(d));
    assert!(
        d.mixer().unwrap().is_touched(pan2),
        "Busy keeps the last set"
    );
    f.set_call_error("automation_state", None);
    mx(&mut d, |m, d| m.poll_automation(d));
    assert!(!d.mixer().unwrap().is_touched(pan2));
}

#[test]
fn a_click_whose_value_fails_still_releases_the_touch() {
    let (f, mut d) = make();
    f.set_call_error("gesture_value", Some(model_err("boom")));
    mx(&mut d, |m, d| m.toggle_mute(d, 1, Instant::now()));
    assert_eq!(
        phases(&f),
        [touch(Some(1.0)), RELEASE],
        "the touch is closed"
    );
    assert_eq!(d.alert.as_ref().unwrap().message, "boom");
    assert_eq!(d.alert.as_ref().unwrap().title, "Mute Failed");
}

#[test]
fn a_refused_touch_is_not_released() {
    let (f, mut d) = make();
    f.set_call_error("gesture_touch", Some(model_err("no")));
    mx(&mut d, |m, d| m.toggle_mute(d, 1, Instant::now()));
    assert!(f.gestures().is_empty());
    assert_eq!(d.alert.as_ref().unwrap().message, "no");
}

#[test]
fn two_targets_touched_at_once_are_independent() {
    let (f, mut d) = make();
    let other = T::StripFader { strip: 2 };
    mx(&mut d, |m, d| {
        m.touch(d, FADER, false);
        m.touch(d, other, false);
        m.release(d, FADER, Instant::now());
    });
    let m = d.mixer().unwrap();
    assert!(m.is_touched(other) && !m.is_touched(FADER));
    assert_eq!(releases(&f), [FADER]);
}

#[test]
fn touches_are_released_once_when_the_window_closes_or_the_strip_goes() {
    let (f, mut d) = make();
    let now = Instant::now();
    mx(&mut d, |m, d| {
        m.touch(d, FADER, false);
        m.end_open_drag(d, now);
        m.end_open_drag(d, now);
        m.release(d, FADER, now);
    });
    assert_eq!(releases(&f).len(), 1);
    f.clear_gestures();
    let other = T::StripFader { strip: 2 };
    mx(&mut d, |m, d| m.touch(d, other, false));
    f.edit_mixer(|m| m.strips.retain(|s| s.id != 2));
    // An edit refetches the mixer: strip 2 is gone.
    mx(&mut d, |m, d| m.set_output(d, 1, None));
    assert_eq!(phases(&f), [touch(None), RELEASE]);
    mx(&mut d, |m, d| m.release(d, other, now));
    assert_eq!(releases(&f).len(), 1);
}

#[test]
fn a_refused_touch_sends_no_release() {
    let (f, mut d) = make();
    let now = Instant::now();
    f.set_call_error("gesture", Some(busy("Finish the open edit first.")));
    mx(&mut d, |m, d| m.touch(d, FADER, false));
    assert_eq!(
        d.alert.as_ref().unwrap().message,
        "Finish the open edit first."
    );
    f.set_call_error("gesture", None);
    mx(&mut d, |m, d| {
        m.move_to(d, FADER, 0.3, now);
        m.release(d, FADER, now);
    });
    assert!(f.gestures().is_empty(), "nothing after a refused touch");
}

#[test]
fn a_refused_value_ends_the_drag() {
    let (f, mut d) = make();
    let now = Instant::now();
    let before = gain(&d, 1);
    mx(&mut d, |m, d| m.touch(d, FADER, false));
    f.set_call_error("gesture_value", Some(busy("Finish the open edit first.")));
    mx(&mut d, |m, d| m.move_to(d, FADER, 0.2, now));
    assert_eq!(
        d.alert.as_ref().unwrap().message,
        "Finish the open edit first."
    );
    assert_eq!(phases(&f), [touch(None), RELEASE], "released at once");
    assert!(!d.mixer().unwrap().is_touched(FADER));
    assert_eq!(gain(&d, 1), before, "the hand's value is not shown");
    f.set_call_error("gesture_value", None);
    mx(&mut d, |m, d| {
        m.move_to(d, FADER, 0.3, now);
        m.release(d, FADER, now);
    });
    assert_eq!(f.gestures().len(), 2, "nothing more from that drag");
}

#[test]
fn a_value_with_no_editing_span_is_a_click() {
    let (f, mut d) = make();
    let now = Instant::now();
    mx(&mut d, |m, d| m.move_to(d, FADER, 0.3, now));
    assert_eq!(phases(&f), [touch(Some(0.3)), value(0.3), RELEASE]);
    assert!(f.gestures().iter().all(|g| g.0 == FADER));
    assert!(!d.mixer().unwrap().is_touched(FADER), "no touch left open");
    mx(&mut d, |m, d| {
        m.release(d, FADER, now);
        m.end_open_drag(d, now);
    });
    assert_eq!(f.gestures().len(), 3, "nothing left to release");
    // Inside a drag whose touch the core refused, values still send
    // nothing (no click per move).
    f.set_call_error("gesture", Some(busy("busy")));
    mx(&mut d, |m, d| m.touch(d, FADER, false));
    f.set_call_error("gesture", None);
    mx(&mut d, |m, d| {
        m.move_to(d, FADER, 0.4, now);
        m.release(d, FADER, now);
    });
    assert_eq!(f.gestures().len(), 3);
    mx(&mut d, |m, d| m.move_to(d, FADER, 0.5, now));
    assert_eq!(
        f.gestures().len(),
        6,
        "after that drag ends, a lone value clicks again"
    );
}

#[test]
fn modes_are_set_per_strip_and_for_all() {
    let (f, mut d) = make();
    mx(&mut d, |m, d| m.set_mode(d, 2, AutomationModeRow::Touch));
    assert_eq!(
        f.mode_calls().last().unwrap(),
        &(vec![2], AutomationModeRow::Touch)
    );
    mx(&mut d, |m, d| m.set_all_modes(d, AutomationModeRow::Latch));
    assert_eq!(
        f.mode_calls().last().unwrap(),
        &(vec![1, 2, 0], AutomationModeRow::Latch)
    );
    assert_eq!(
        d.mixer().unwrap().strip(2).unwrap().automation,
        AutomationModeRow::Latch,
        "the mixer is read again"
    );
    f.set_call_error(
        "set_automation_mode",
        Some(busy("Release the touched control first.")),
    );
    mx(&mut d, |m, d| m.set_mode(d, 2, AutomationModeRow::Off));
    assert_eq!(d.alert.as_ref().unwrap().title, "Automation Mode Failed");
}

#[test]
fn the_selection_forgets_strips_that_go() {
    let (f, mut d) = make();
    mx(&mut d, |m, d| {
        m.select(d, 1, false);
        m.select(d, 2, true);
    });
    assert_eq!(
        d.selected_strips().iter().copied().collect::<Vec<_>>(),
        [1, 2]
    );
    mx(&mut d, |m, d| m.select(d, 2, true));
    assert_eq!(
        d.selected_strips().iter().copied().collect::<Vec<_>>(),
        [1],
        "⌘-click removes"
    );
    mx(&mut d, |m, d| m.select(d, 0, false));
    assert_eq!(
        d.selected_strips().iter().copied().collect::<Vec<_>>(),
        [0],
        "master can be selected"
    );
    mx(&mut d, |m, d| m.select(d, 2, true));
    f.edit_mixer(|m| m.strips.retain(|s| s.id != 2));
    mx(&mut d, |m, d| m.set_output(d, 1, None));
    assert_eq!(d.selected_strips().iter().copied().collect::<Vec<_>>(), [0]);
    assert_eq!(
        f.surface_selections(),
        [vec![1], vec![1, 2], vec![1], vec![0], vec![0, 2], vec![0]],
        "every change reaches the core, sorted"
    );
}

// Typed values (Swift `MixerStripTests`).

#[test]
fn typing_a_level_is_one_click_and_a_refusal_changes_nothing() {
    let (f, mut d) = make();
    let now = Instant::now();
    assert!(mx(&mut d, |m, d| m.type_level(d, 1, "-6", now)));
    let n = fader_norm(-6.0);
    assert_eq!(phases(&f), [touch(Some(n)), value(n), RELEASE]);
    f.clear_gestures();
    assert!(!mx(&mut d, |m, d| m.type_level(d, 1, "inf", now)));
    assert_eq!(d.status(), Some(StripEntry::LEVEL_HINT));
    assert!(f.gestures().is_empty());
    mx(&mut d, |m, d| m.touch(d, FADER, false));
    f.clear_gestures();
    assert!(!mx(&mut d, |m, d| m.type_level(d, 1, "-3", now)));
    assert_eq!(d.status(), Some(StripEntry::TOUCHED_HINT));
    assert!(f.gestures().is_empty());
}

#[test]
fn typing_pan_and_send_levels_uses_their_targets() {
    let (f, mut d) = make();
    let now = Instant::now();
    assert!(mx(&mut d, |m, d| m.type_pan(d, 1, "L 24", now)));
    assert_eq!(f.gestures()[0], (PAN, touch(Some(pan_norm(-0.24))), false));
    f.clear_gestures();
    assert!(!mx(&mut d, |m, d| m.type_pan(d, 1, "L", now)));
    assert_eq!(d.status(), Some(StripEntry::PAN_HINT));
    assert!(
        !mx(&mut d, |m, d| m.type_send(d, 1, 0, "-3", now)),
        "strip 1 has no send"
    );
    f.edit_mixer(|m| {
        m.strips[0].sends.push(vizdaw_ffi::mixer::SendRow {
            target: 2,
            level_db: 0.0,
            pre_fader: false,
            enabled: true,
        })
    });
    mx(&mut d, |m, d| m.set_output(d, 1, None));
    f.clear_gestures();
    assert!(mx(&mut d, |m, d| m.type_send(d, 1, 0, "-3", now)));
    assert_eq!(
        f.gestures()[0].0,
        T::SendLevel { strip: 1, send: 0 },
        "0-based"
    );
}

// The drag tracker in the model.

#[test]
fn a_bare_press_sends_nothing() {
    let (f, mut d) = make();
    let now = Instant::now();
    let v = MixerPlacement::View;
    mx(&mut d, |m, d| {
        m.press(d, v, FADER, THROW, false, now);
        m.drag(d, v, 1.0, 1.0, false, now);
        m.lift(d, v, now);
    });
    assert!(
        f.gestures().is_empty(),
        "a press under 2 points is no touch"
    );
    assert!(!d.mixer().unwrap().is_touched(FADER));
    // The double-click that follows resets to 0 dB, and leaves no touch.
    f.edit_mixer(|m| m.strips[0].gain_db = -10.0);
    mx(&mut d, |m, d| m.set_output(d, 1, None));
    mx(&mut d, |m, d| {
        m.press(d, v, FADER, THROW, false, now);
        m.double_click(d, v, FADER, now);
        m.lift(d, v, now);
    });
    let n = fader_norm(0.0);
    assert_eq!(phases(&f), [touch(Some(n)), value(n), RELEASE]);
    assert!(!d.mixer().unwrap().is_touched(FADER));
}

#[test]
fn a_fader_drag_through_the_tracker() {
    let (f, mut d) = make();
    let now = Instant::now();
    let v = MixerPlacement::View;
    let start = MixerMath::fader_position(0.0);
    mx(&mut d, |m, d| {
        m.press(d, v, FADER, THROW, false, now);
        m.drag(d, v, 0.0, -3.0, false, now);
        m.drag(d, v, 0.0, -19.0, false, now);
    });
    assert!(d.mixer().unwrap().is_dragging(v));
    let up = fader_norm(MixerMath::fader_db(start + 16.0 / THROW));
    assert_eq!(
        phases(&f),
        [
            touch(None),
            value(fader_norm(MixerMath::fader_db(start))),
            value(up)
        ],
        "the first move anchors; 16 points up is 16/160 of the throw"
    );
    assert!((gain(&d, 1) - fader_db(up)).abs() < 1e-9);
    mx(&mut d, |m, d| m.lift(d, v, now));
    assert_eq!(phases(&f).last(), Some(&RELEASE));
    assert!(!d.mixer().unwrap().is_dragging(v));
    assert!(
        (d.mixer().unwrap().strip(1).unwrap().gain_db - 7.2).abs() < 1e-9,
        "the step landed"
    );
}

#[test]
fn option_at_the_press_moves_the_fader_alone() {
    let (f, mut d) = make();
    let now = Instant::now();
    let v = MixerPlacement::View;
    mx(&mut d, |m, d| {
        m.press(d, v, FADER, THROW, true, now);
        m.drag(d, v, 0.0, -10.0, false, now);
        m.lift(d, v, now);
    });
    assert_eq!(f.gestures().len(), 3);
    assert!(f.gestures().iter().all(|g| g.2), "every phase alone");
    f.clear_gestures();
    mx(&mut d, |m, d| {
        m.press(d, v, PAN, DRAG_TRAVEL, true, now);
        m.drag(d, v, 0.0, -10.0, false, now);
        m.lift(d, v, now);
    });
    assert!(
        f.gestures().iter().all(|g| !g.2),
        "a knob has no group alone"
    );
}

#[test]
fn option_while_dragging_is_fine() {
    let (f, mut d) = make();
    let now = Instant::now();
    let v = MixerPlacement::Pane;
    mx(&mut d, |m, d| {
        m.press(d, v, PAN, DRAG_TRAVEL, false, now);
        m.drag(d, v, 0.0, -5.0, false, now);
        // ⌥ goes down: no jump, then ten times finer.
        m.drag(d, v, 0.0, -5.0, true, now);
        m.drag(d, v, 0.0, -25.0, true, now);
    });
    let last = match phases(&f).last() {
        Some(GesturePhaseRow::Value { value }) => *value,
        p => panic!("{p:?}"),
    };
    assert!((last - (0.5 + 20.0 / DRAG_TRAVEL / FINE_DIVISOR)).abs() < 1e-9);
}

#[test]
fn a_double_click_right_after_a_drag_is_not_a_reset() {
    let (f, mut d) = make();
    let t0 = Instant::now();
    let v = MixerPlacement::View;
    mx(&mut d, |m, d| {
        m.press(d, v, PAN, DRAG_TRAVEL, false, t0);
        m.drag(d, v, 0.0, -10.0, false, t0);
        m.lift(d, v, t0);
    });
    f.clear_gestures();
    mx(&mut d, |m, d| {
        m.double_click(d, v, PAN, t0 + Duration::from_millis(100))
    });
    assert!(f.gestures().is_empty(), "the tap that closes a drag");
    mx(&mut d, |m, d| {
        m.double_click(d, v, PAN, t0 + Duration::from_secs(1))
    });
    assert_eq!(
        phases(&f),
        [touch(Some(0.5)), value(0.5), RELEASE],
        "the centre"
    );
}

#[test]
fn a_key_nudge_is_one_gesture() {
    let (f, mut d) = make();
    let now = Instant::now();
    mx(&mut d, |m, d| m.nudge(d, PAN, 0.1, now));
    assert_eq!(f.gestures().len(), 3);
    match phases(&f)[1] {
        GesturePhaseRow::Value { value } => assert!((value - 0.6).abs() < 1e-9),
        p => panic!("{p:?}"),
    }
    assert!(!d.mixer().unwrap().is_touched(PAN));
}

#[test]
fn a_busy_release_is_retried() {
    let (f, mut d) = make();
    let t0 = Instant::now();
    f.fail_next("gesture_release", busy("busy"));
    mx(&mut d, |m, d| {
        m.touch(d, FADER, false);
        m.move_to(d, FADER, 0.5, t0);
        m.release(d, FADER, t0);
    });
    assert!(releases(&f).is_empty());
    assert!(d.alert.is_none(), "Busy is quiet");
    assert!(d.mixer_release_pending());
    assert!(d.wants_fast_poll(), "the poll runs fast while it waits");
    assert!(
        d.mixer().unwrap().is_touched(FADER),
        "the core still holds it"
    );
    d.tick_at(t0 + Duration::from_millis(16));
    assert_eq!(releases(&f), [FADER], "retried by the document's tick");
    assert!(!d.mixer_release_pending());
    assert!(!d.wants_fast_poll());
    assert!(!d.mixer().unwrap().is_touched(FADER));
}

#[test]
fn a_busy_release_past_its_window_alerts() {
    let (f, mut d) = make();
    let t0 = Instant::now();
    f.set_call_error("gesture_release", Some(busy("busy")));
    mx(&mut d, |m, d| {
        m.touch(d, FADER, false);
        m.release(d, FADER, t0);
    });
    d.tick_at(t0 + RELEASE_WINDOW / 2);
    assert!(d.mixer_release_pending() && d.alert.is_none());
    d.tick_at(t0 + RELEASE_WINDOW);
    assert!(!d.mixer_release_pending());
    assert_eq!(d.alert.as_ref().unwrap().title, "Fader Failed");
}

#[test]
fn a_new_drag_while_the_release_waits_carries_the_touch_on() {
    let (f, mut d) = make();
    let t0 = Instant::now();
    f.fail_next("gesture_release", busy("busy"));
    mx(&mut d, |m, d| {
        m.touch(d, FADER, false);
        m.release(d, FADER, t0);
        m.touch(d, FADER, false);
    });
    assert_eq!(
        phases(&f),
        [touch(None)],
        "the core still holds it: no second touch"
    );
    assert!(
        !d.mixer_release_pending(),
        "the new drag's release closes it"
    );
    mx(&mut d, |m, d| m.release(d, FADER, t0));
    assert_eq!(releases(&f), [FADER]);
}

#[test]
fn the_last_placement_releases_open_drags() {
    let (f, mut d) = make();
    let now = Instant::now();
    let (view, window) = (MixerPlacement::View, MixerPlacement::Window);
    mx(&mut d, |m, d| {
        m.appeared(view);
        m.appeared(window);
        m.press(d, view, FADER, THROW, false, now);
        m.drag(d, view, 0.0, -10.0, false, now);
        m.press(d, window, PAN, DRAG_TRAVEL, false, now);
        m.drag(d, window, 0.0, -10.0, false, now);
        // A touch no drag owns (a keyboard span) ends with the last one.
        m.touch(d, T::StripFader { strip: 2 }, false);
        m.disappeared(d, window, now);
    });
    assert_eq!(releases(&f), [PAN], "closing one keeps the other's drag");
    assert!(d.mixer().unwrap().is_dragging(view));
    mx(&mut d, |m, d| m.disappeared(d, view, now));
    let mut gone = releases(&f);
    gone.sort_by_key(|t| format!("{t:?}"));
    assert_eq!(gone.len(), 3);
    assert!(gone.contains(&FADER) && gone.contains(&T::StripFader { strip: 2 }));
    assert!(d.mixer().unwrap().placements().is_empty());
}

#[test]
fn closing_the_document_releases_open_drags() {
    let f = Arc::new(mixer_fake());
    let g = f.clone();
    let mut store = DocumentStore::new(
        Box::new(move || Ok(g.clone())),
        Box::new(|| Ok(Arc::new(mixer_fake()))),
        Box::new(|_| Ok(Arc::new(mixer_fake()))),
    );
    let id = store.new_empty().unwrap();
    store.with_document(id, |d, _| d.with_mixer(|m, d| m.touch(d, FADER, false)));
    store.close(id);
    assert_eq!(releases(&f), [FADER]);
}

#[test]
fn a_strip_that_goes_ends_its_drag() {
    let (f, mut d) = make();
    let now = Instant::now();
    let v = MixerPlacement::View;
    mx(&mut d, |m, d| {
        m.press(d, v, FADER, THROW, false, now);
        m.drag(d, v, 0.0, -10.0, false, now);
    });
    f.edit_mixer(|m| m.strips.retain(|s| s.id != 1));
    mx(&mut d, |m, d| {
        m.set_output(d, 2, None);
        m.drag(d, v, 0.0, -20.0, false, now);
        m.lift(d, v, now);
    });
    assert_eq!(releases(&f), [FADER], "released once, quietly");
    assert_eq!(f.gestures().len(), 3, "the later moves send nothing");
    assert!(d.alert.is_none());
}
