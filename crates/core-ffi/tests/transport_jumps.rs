//! The transport bar's rewind and fast forward through a session (plan 7d
//! Rulings 3 and 5): a click steps one bar, a wind moves by quarter notes,
//! both remember where they went for the next one, a locate forgets it,
//! and hostile values are refused or clamped. No device is opened and no
//! transport is started: the session's cursor holds the position.

use vizdaw_ffi::surfaces::shuttle_quarters;
use vizdaw_ffi::{Session, VizError};

const BAR: i64 = 61_440;

/// The demo: its first sequence is the one a step moves, the cursor at 0.
fn demo() -> std::sync::Arc<Session> {
    let s = Session::new_demo().unwrap();
    assert_eq!(s.transport().unwrap().tick, 0);
    s
}

#[test]
fn step_bar_and_wind_move_the_cursor_and_remember_the_target() {
    let s = demo();
    assert_eq!(s.transport_step_bar(true).unwrap(), BAR);
    assert_eq!(s.transport().unwrap().tick, BAR);
    // With no engine the cursor holds the target too, so this does not
    // prove the memory; `a_locate_forgets_the_wind_memory` does.
    assert_eq!(s.transport_step_bar(true).unwrap(), 2 * BAR);
    assert_eq!(s.transport_step_bar(false).unwrap(), BAR);
    assert_eq!(s.transport_wind(1.5).unwrap(), 84_480);
    assert_eq!(s.transport().unwrap().tick, 84_480);
    assert_eq!(s.transport_wind(-1e6).unwrap(), 0);
    assert_eq!(s.transport_step_bar(false).unwrap(), 0);
    s.close().unwrap();
}

#[test]
fn hostile_steps_and_winds_are_refused_or_clamped() {
    let s = demo();
    s.transport_step_bar(true).unwrap();
    for q in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        match s.transport_wind(q) {
            Err(VizError::Model { message }) => {
                assert!(message.contains("quarters must be finite"), "{message}")
            }
            other => panic!("{q}: {other:?}"),
        }
        assert_eq!(s.transport().unwrap().tick, BAR, "{q} moved nothing");
    }
    assert_eq!(s.transport_wind(f64::MAX).unwrap(), 1 << 50);
    assert_eq!(s.transport_step_bar(true).unwrap(), 1 << 50);
    assert_eq!(s.transport_wind(-f64::MAX).unwrap(), 0);
    s.close().unwrap();

    // No sequence to move in.
    let empty = Session::new_empty().unwrap();
    assert!(empty.snapshot().unwrap().sequences.is_empty());
    assert!(matches!(
        empty.transport_step_bar(true),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        empty.transport_wind(1.0),
        Err(VizError::Model { .. })
    ));
    empty.close().unwrap();
}

#[test]
fn shuttle_quarters_is_the_surfaces_curve() {
    assert_eq!(shuttle_quarters(0, 400), 0.0);
    assert_eq!(shuttle_quarters(400, 1400), 4.0);
    assert_eq!(shuttle_quarters(0, 3400), 28.0);
    assert_eq!(shuttle_quarters(3400, 4400), 32.0);
    assert_eq!(shuttle_quarters(5000, 4000), 0.0, "backwards is none");
    let far = shuttle_quarters(0, u64::MAX);
    assert!(far.is_finite() && far > 0.0, "{far}");
}

/// Controller ruling I6: a locate (a ruler click, return to start, a
/// marker jump) just before a step is where the step starts.
#[test]
fn a_locate_forgets_the_wind_memory() {
    let s = demo();
    assert_eq!(s.transport_step_bar(true).unwrap(), BAR);
    s.locate(0).unwrap();
    assert_eq!(s.transport_step_bar(true).unwrap(), BAR, "not 2 bars");
    s.close().unwrap();
}
