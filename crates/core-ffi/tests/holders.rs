//! One target, several hands (plan 5f-1 Task 6; Ruling 2; 5e-2 carry m8):
//! each holder releases its own touch, stopped and while writing; a
//! resting touch opens no edit. Offline only.

mod common;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::automation::{
    AutomationModeRow as Mode, GestureHolderRow as H, GesturePhaseRow as Phase,
};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{AutomationTargetRow as Target, Session, VizError};

const Q: i64 = 15_360;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Paces gesture samples past the 10 ms sampler.
fn pace(s: &Session) {
    std::thread::sleep(Duration::from_millis(12));
    s.idle().unwrap();
}

fn demo() -> (Arc<Session>, u64) {
    let s = Session::new_demo().unwrap();
    let synth = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.name == "Built-in Synth")
        .unwrap()
        .id;
    (s, synth)
}

fn song(s: &Session) -> u64 {
    s.snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id
}

fn gain(s: &Session, strip: u64) -> f64 {
    s.mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.id == strip)
        .unwrap()
        .gain_db
}

#[test]
fn a_surface_and_the_mouse_hold_one_target_until_both_let_go() {
    let _g = serial();
    let (s, synth) = demo();
    let t = Target::StripFader { strip: synth };
    let before = gain(&s, synth);
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel)
        .unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert!(
        s.automation_state().unwrap().touched.contains(&t),
        "the channel still holds it"
    );
    // Inside the open static edit `undo_label` already names its step,
    // so "no step yet" is read as: the edit is still open.
    assert!(
        s.begin_edit("Probe".into()).is_err(),
        "no step yet: the static edit is still open"
    );
    s.gesture_from(t, Phase::Value { value: 0.25 }, H::Channel)
        .unwrap();
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    let db = gain(&s, synth);
    s.undo().unwrap();
    assert!(
        (gain(&s, synth) - db).abs() > 0.1,
        "one step undoes both hands' moves"
    );
    // Review M2: one Undo goes back to before the first hand's touch.
    assert!(
        (gain(&s, synth) - before).abs() < 1e-9,
        "one step, not one per hand"
    );
    assert_eq!(s.snapshot().unwrap().undo_label, None);
}

#[test]
fn each_holder_releases_only_its_own_touch() {
    let _g = serial();
    let (s, synth) = demo();
    let t = Target::StripPan { strip: synth };
    s.gesture_from(t, Phase::Touch { value: Some(0.3) }, H::Channel)
        .unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert!(
        s.automation_state().unwrap().touched.contains(&t),
        "the mouse never held it"
    );
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
}

#[test]
fn a_holder_touching_twice_holds_once() {
    let _g = serial();
    let (s, synth) = demo();
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    s.gesture(t, Phase::Touch { value: Some(0.6) }).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "one release ends it"
    );
}

/// Pre-flight C2: a touch with no value holds no edit open; its first
/// value opens its own step.
#[test]
fn a_resting_touch_does_not_open_an_edit() {
    let _g = serial();
    let (s, synth) = demo();
    let t = Target::StripFader { strip: synth };
    let before = gain(&s, synth);
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel)
        .unwrap();
    s.begin_edit("Drag".into()).unwrap();
    assert!(
        matches!(
            s.gesture_from(t, Phase::Value { value: 0.4 }, H::Channel),
            Err(VizError::Busy { .. })
        ),
        "a first value waits for the open drag"
    );
    assert!(
        matches!(
            s.gesture(t, Phase::Touch { value: Some(0.4) }),
            Err(VizError::Busy { .. })
        ),
        "so does a second hand's touch at a value"
    );
    s.end_edit().unwrap();
    s.add_bus("Rest".into()).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Add Bus"),
        "its own step while the touch rests"
    );
    s.gesture_from(t, Phase::Value { value: 0.25 }, H::Channel)
        .unwrap();
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "the refused mouse touch never joined"
    );
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    s.undo().unwrap();
    assert!(
        (gain(&s, synth) - before).abs() < 1e-9,
        "Undo restores only the fader"
    );
    assert!(
        s.mixer().unwrap().strips.iter().any(|x| x.name == "Rest"),
        "and keeps the bus"
    );
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel)
        .unwrap();
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Add Bus"),
        "a touch with no value commits nothing"
    );
}

/// Pre-flight I12: Ruling 2 while writing. The first hand's release
/// neither glides nor holds; the write follows the second hand to its
/// release.
#[test]
fn two_holders_on_a_writing_strip_end_only_at_the_last_release() {
    let _g = serial();
    let (s, synth) = demo();
    let seq = song(&s);
    let t = Target::StripFader { strip: synth };
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick > Q);
    let t0 = s.transport().unwrap().tick;
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.45 }).unwrap();
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel)
        .unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    assert!(
        s.automation_state().unwrap().touched.contains(&t),
        "the channel still holds it"
    );
    for v in [0.4, 0.35, 0.3] {
        pace(&s);
        s.gesture_from(t, Phase::Value { value: v }, H::Channel)
            .unwrap();
    }
    pace(&s);
    let t1 = s.transport().unwrap().tick;
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    wait_until("the glide to pass", || s.transport().unwrap().tick > t1 + Q);
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let lane = s
        .lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Built-in Synth Fader")
        .expect("a new lane")
        .id;
    let mut p: Vec<(i64, f64)> = s
        .lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .filter_map(|e| e.automation.map(|a| (e.tick, a.value)))
        .collect();
    p.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for v in [0.5, 0.3] {
        assert!(
            p.iter().any(|x| (x.1 - v).abs() < 1e-9),
            "{v} from both hands: {p:?}"
        );
    }
    assert!(
        p.iter()
            .any(|x| x.0 > t0 && x.0 < t1 && x.1 < 0.5 - 1e-9 && x.1 > 0.4 - 1e-9),
        "the mouse's second value is in the ride (thinning may move it): {p:?}"
    );
    let ride: Vec<f64> = p
        .iter()
        .filter(|x| x.0 > t0 && x.0 < t1)
        .map(|x| x.1)
        .collect();
    assert!(
        ride.windows(2).all(|w| w[1] <= w[0] + 1e-9),
        "falling all the way: no glide back toward the static value at the first release: {p:?}"
    );
}

/// Pre-flight C2 held over Play (fold check C1): a hand resting on a Read
/// strip's fader when Play starts keeps the static path; its move is a
/// static edit.
#[test]
fn a_resting_touch_held_over_play_keeps_its_path() {
    let _g = serial();
    let (s, synth) = demo();
    let seq = song(&s);
    let t = Target::StripFader { strip: synth };
    let before = gain(&s, synth);
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel)
        .unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick > Q);
    s.gesture_from(t, Phase::Value { value: 0.25 }, H::Channel)
        .unwrap();
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader"),
        "a Read strip's move is a static edit"
    );
    assert!(
        (gain(&s, synth) - before).abs() > 0.1,
        "and it stays after the release"
    );
}

/// Review M3: a hand that let go releasing again does nothing while
/// another hand holds the target.
#[test]
fn a_second_release_from_one_hand_does_nothing_while_another_holds() {
    let _g = serial();
    let (s, synth) = demo();
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel)
        .unwrap();
    s.gesture(t, Phase::Release).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert!(
        s.automation_state().unwrap().touched.contains(&t),
        "the channel still holds it"
    );
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
}

/// Review M5: a value from a hand that does not hold a touched target
/// joins, as a value on an untouched target touches it: the other hand's
/// release does not end it.
#[test]
fn a_value_from_a_hand_that_does_not_hold_it_joins() {
    let _g = serial();
    let (s, synth) = demo();
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    s.gesture_from(t, Phase::Value { value: 0.4 }, H::Channel)
        .unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert!(
        s.automation_state().unwrap().touched.contains(&t),
        "the channel's value made it a holder"
    );
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
}

/// Review M3: at Play only a touch that shares the static edit leaves it.
/// A resting touch on a writing strip converts without closing the edit
/// a sharing touch on a Read strip still holds open.
#[test]
fn only_a_sharing_touch_leaves_the_static_edit_at_play() {
    let _g = serial();
    let (s, synth) = demo();
    let seq = song(&s);
    let other = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.id != synth)
        .unwrap()
        .id;
    let rest = Target::StripFader { strip: synth };
    let held = Target::StripFader { strip: other };
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    s.gesture_from(rest, Phase::Touch { value: None }, H::Channel)
        .unwrap();
    s.gesture(held, Phase::Touch { value: Some(0.5) }).unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick > Q);
    assert!(
        s.begin_edit("Probe".into()).is_err(),
        "the Read strip's touch still holds the static edit open"
    );
    for v in [0.4, 0.3] {
        pace(&s);
        s.gesture_from(rest, Phase::Value { value: v }, H::Channel)
            .unwrap();
    }
    pace(&s);
    s.gesture(held, Phase::Value { value: 0.25 }).unwrap();
    s.gesture(held, Phase::Release).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader"),
        "the shared edit closes at its own release"
    );
    s.gesture_from(rest, Phase::Release, H::Channel).unwrap();
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation"),
        "the resting touch wrote"
    );
}

/// Ruling T6-1 (review M1): a mode change while playing re-picks the path
/// of a touch that rests with no value, as if it were touched now.
#[test]
fn a_resting_touch_takes_the_mode_set_while_it_rests() {
    let _g = serial();
    let (s, synth) = demo();
    let seq = song(&s);
    let t = Target::StripFader { strip: synth };
    let before = gain(&s, synth);
    s.play(seq).unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick > Q);
    s.gesture_from(t, Phase::Touch { value: None }, H::Channel)
        .unwrap();
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    for v in [0.4, 0.3] {
        pace(&s);
        s.gesture_from(t, Phase::Value { value: v }, H::Channel)
            .unwrap();
    }
    pace(&s);
    s.gesture_from(t, Phase::Release, H::Channel).unwrap();
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation"),
        "the move is written"
    );
    assert!(
        (gain(&s, synth) - before).abs() < 1e-9,
        "and the static value is left alone"
    );
    let lane = s
        .lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Built-in Synth Fader")
        .expect("a new lane")
        .id;
    let values: Vec<f64> = s
        .lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .filter_map(|e| e.automation.map(|a| a.value))
        .collect();
    assert!(values.iter().any(|v| (v - 0.3).abs() < 1e-9), "{values:?}");
}
