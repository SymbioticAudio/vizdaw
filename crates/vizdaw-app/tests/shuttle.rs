mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use core_control::ErrorCode;
use support::{FakeSession, demo_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::transport::Shuttle;
use vizdaw_ffi::VizError;

const BAR: i64 = 61_440;
const TEXT: &str = "A text field has the keyboard.";

fn demo() -> (Arc<FakeSession>, DocumentModel, Shuttle, Instant) {
    let f = Arc::new(demo_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.select(Some(1));
    (f, m, Shuttle::new(), Instant::now())
}

fn s(t0: Instant, secs: f64) -> Instant {
    t0 + Duration::from_secs_f64(secs)
}

fn tick(f: &FakeSession) -> i64 {
    use vizdaw_app::DocSession;
    f.transport().unwrap().tick
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_click_steps_one_bar() {
    let (f, mut m, mut sh, t0) = demo();
    sh.press(true, false, t0, &mut m);
    assert!(sh.holding());
    sh.release(true, s(t0, 0.2), &mut m);
    assert_eq!(f.step_bars(), vec![true]);
    assert!(f.winds().is_empty());
    assert_eq!(tick(&f), BAR);
    assert!(!sh.holding());
    assert_eq!(
        f.hold_ends(),
        0,
        "a click is one locate, with no hold to end"
    );
}

#[test]
fn a_shift_click_goes_to_the_marker() {
    let (f, mut m, mut sh, t0) = demo();
    f.set_marker_ticks(vec![30_720]);
    sh.press(true, true, t0, &mut m);
    sh.tick(s(t0, 0.5), &mut m);
    sh.tick(s(t0, 1.0), &mut m);
    sh.release(true, s(t0, 1.1), &mut m);
    assert!(f.winds().is_empty(), "a shift press never winds");
    assert!(f.step_bars().is_empty());
    assert_eq!(tick(&f), 30_720);
    sh.press(true, true, s(t0, 2.0), &mut m);
    sh.release(true, s(t0, 2.1), &mut m);
    assert_eq!(m.status(), Some("No marker after the position."));
    assert_eq!(tick(&f), 30_720);
}

#[test]
fn a_hold_winds_faster_the_longer_it_is_held() {
    let (f, mut m, mut sh, t0) = demo();
    sh.press(true, false, t0, &mut m);
    for t in [0.1, 0.2, 0.3] {
        sh.tick(s(t0, t), &mut m);
    }
    assert!(f.winds().is_empty(), "nothing below 0.4 s");
    sh.tick(s(t0, 0.5), &mut m);
    assert_eq!(f.winds(), vec![0.4]);
    sh.tick(s(t0, 1.5), &mut m);
    assert!(close(*f.winds().last().unwrap(), 4.4), "0.9 x 4 + 0.1 x 8");
    sh.tick(s(t0, 3.5), &mut m);
    assert!(
        close(*f.winds().last().unwrap(), 26.4),
        "0.9 x 8 + 1.0 x 16 + 0.1 x 32"
    );
    assert_eq!(f.winds().len(), 3);
    assert!(f.step_bars().is_empty());
    assert!(sh.holding());
}

#[test]
fn a_release_after_winding_steps_no_bar() {
    let (f, mut m, mut sh, t0) = demo();
    f.set_transport(|t| t.tick = 200_000);
    sh.press(false, false, t0, &mut m);
    sh.tick(s(t0, 0.5), &mut m);
    sh.release(true, s(t0, 0.55), &mut m);
    let w = f.winds();
    assert!(
        w.len() == 2 && close(w[0], -0.4) && close(w[1], -0.2),
        "{w:?}"
    );
    assert!(f.step_bars().is_empty());
    assert_eq!(f.hold_ends(), 1);
    assert!(!sh.holding());
}

#[test]
fn cancel_stops_without_a_move() {
    let (f, mut m, mut sh, t0) = demo();
    sh.press(true, false, t0, &mut m);
    sh.tick(s(t0, 0.5), &mut m);
    sh.cancel(&mut m);
    sh.tick(s(t0, 1.0), &mut m);
    sh.release(true, s(t0, 1.1), &mut m);
    assert_eq!(f.winds(), vec![0.4]);
    assert!(f.step_bars().is_empty());
    assert_eq!(
        f.hold_ends(),
        1,
        "the open pass lands where the wind got to"
    );
    assert!(!sh.holding());
    // A release outside the button with no wind yet moves nothing.
    sh.press(true, false, s(t0, 2.0), &mut m);
    sh.release(false, s(t0, 2.2), &mut m);
    assert_eq!(f.winds(), vec![0.4]);
    assert!(f.step_bars().is_empty() && f.count("locate(0)") == 0);
    assert_eq!(
        f.calls().iter().filter(|c| c.starts_with("locate")).count(),
        0
    );
    assert_eq!(f.hold_ends(), 1);
    assert!(!sh.holding());
    // A time earlier than the press counts as the press.
    sh.press(true, false, s(t0, 5.0), &mut m);
    sh.tick(t0, &mut m);
    assert_eq!(f.winds(), vec![0.4]);
    sh.cancel(&mut m);
    assert_eq!(f.hold_ends(), 1, "no wind, no hold to land");
}

#[test]
fn a_hold_waits_for_a_text_field_as_a_click_does() {
    let (f, mut m, mut sh, t0) = demo();
    m.begin_text_editing("track-rename");
    sh.press(true, false, t0, &mut m);
    assert!(!sh.holding());
    sh.tick(s(t0, 0.5), &mut m);
    sh.tick(s(t0, 1.0), &mut m);
    sh.release(true, s(t0, 1.1), &mut m);
    assert!(f.winds().is_empty() && f.step_bars().is_empty());
    assert_eq!(f.wind_attempts(), 0);
    assert_eq!(m.status(), Some(TEXT));
}

#[test]
fn a_text_field_taking_the_keyboard_mid_hold_stops_the_hold() {
    let (f, mut m, mut sh, t0) = demo();
    sh.press(true, false, t0, &mut m);
    sh.tick(s(t0, 0.5), &mut m);
    assert!(f.winds() == vec![0.4] && sh.holding());
    m.begin_text_editing("track-rename");
    sh.tick(s(t0, 0.6), &mut m);
    assert!(!sh.holding());
    assert_eq!(m.status(), Some(TEXT));
    assert_eq!(f.hold_ends(), 1, "wound, so the pass lands where it got to");
    sh.tick(s(t0, 1.5), &mut m);
    sh.release(true, s(t0, 1.6), &mut m);
    assert!(f.winds() == vec![0.4] && f.hold_ends() == 1 && f.step_bars().is_empty());
    m.end_text_editing("track-rename");
    // Before any wind: stopped, with nothing to land.
    m.set_status(None);
    sh.press(false, false, s(t0, 2.0), &mut m);
    m.begin_text_editing("track-rename");
    sh.tick(s(t0, 2.2), &mut m);
    assert!(!sh.holding());
    assert_eq!(m.status(), Some(TEXT));
    assert!(f.hold_ends() == 1 && f.winds() == vec![0.4]);
}

#[test]
fn a_refused_wind_stops_the_hold_and_says_why() {
    let (f, mut m, mut sh, t0) = demo();
    f.set_jump_error(Some(VizError::Busy {
        message: "Stop recording first.".into(),
    }));
    sh.press(true, false, t0, &mut m);
    sh.tick(s(t0, 0.5), &mut m);
    assert!(!sh.holding());
    sh.tick(s(t0, 0.6), &mut m);
    assert_eq!(f.wind_attempts(), 1);
    assert_eq!(m.status(), Some("Stop recording first."));
    assert!(m.alert.is_none());
    assert_eq!(f.hold_ends(), 0, "nothing wound, nothing to land");
    // A wind that throws after earlier winds still ends the hold once.
    f.set_jump_error(None);
    m.set_status(None);
    sh.press(true, false, s(t0, 1.0), &mut m);
    sh.tick(s(t0, 1.5), &mut m);
    assert_eq!(f.winds(), vec![0.4]);
    f.set_jump_error(Some(VizError::Busy {
        message: "Stop recording first.".into(),
    }));
    sh.tick(s(t0, 1.6), &mut m);
    assert!(!sh.holding());
    assert_eq!(f.hold_ends(), 1);
    sh.release(true, s(t0, 1.7), &mut m);
    assert!(f.hold_ends() == 1 && f.wind_attempts() == 3);
    assert_eq!(m.status(), Some("Stop recording first."));
    assert!(m.alert.is_none());
}

#[test]
fn a_held_key_is_one_hold_landed_on_key_up() {
    let (f, mut m, mut sh, _t0) = demo();
    for _ in 0..3 {
        sh.key_down(true, &mut m).unwrap();
    }
    assert_eq!(f.key_steps(), vec![true, true, true]);
    assert!(f.step_bars().is_empty());
    assert_eq!(f.hold_ends(), 0);
    assert_eq!(tick(&f), 3 * BAR);
    sh.key_up(&mut m);
    assert_eq!(f.hold_ends(), 1);
    sh.key_up(&mut m);
    assert_eq!(f.hold_ends(), 1, "one landing per hold");
    m.begin_text_editing("track-rename");
    let e = sh.key_down(false, &mut m).unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (ErrorCode::Unavailable, TEXT));
    assert_eq!(f.key_steps(), vec![true, true, true]);
    assert_eq!(m.status(), Some(TEXT));
    sh.key_up(&mut m);
    assert_eq!(f.hold_ends(), 1);
}
