//! Swift `PitchAxisTests` (the value axis belongs to the expression pane,
//! which is out of this slice).

use vizdaw_app::piano_roll::PitchAxis;

#[test]
fn rows_run_from_the_top() {
    let a = PitchAxis::new(10., 0.);
    assert_eq!(a.top(127), 0.);
    assert_eq!(a.top(0), 1_270.);
    assert_eq!(a.center(60), 675.);
    assert_eq!(a.content_height(), 1_280.);
}

#[test]
fn pitch_at_y_clamps_and_follows_the_offset() {
    let a = PitchAxis::new(10., 600.);
    assert_eq!(a.pitch_at(75.), 60);
    assert_eq!(a.pitch_at(-1_000.), 127);
    assert_eq!(a.pitch_at(9_999.), 0);
    assert_eq!(a.pitch_at(f32::NAN), 60);
}

#[test]
fn semitones_are_whole_rows_up_positive() {
    let a = PitchAxis::new(10., 3.);
    assert_eq!(a.semitones(100., 81.), 2);
    assert_eq!(a.semitones(100., 104.), 0);
    assert_eq!(a.semitones(100., 135.), -3);
    assert!(
        a.semitones(0., -5_000.) > 127,
        "unclamped: the drag clamps the group"
    );
}

#[test]
fn zoom_keeps_the_pitch_under_the_anchor() {
    let a = PitchAxis::new(10., 600.);
    let z = a.with_row_height(20., 75.);
    assert_eq!(z.row_height(), 20.);
    assert_eq!(z.pitch_at(75.), 60);
    assert_eq!(
        PitchAxis::new(999., 0.).row_height(),
        PitchAxis::MAX_ROW_HEIGHT
    );
    assert_eq!(
        PitchAxis::new(1., 0.).row_height(),
        PitchAxis::MIN_ROW_HEIGHT
    );
    assert_eq!(PitchAxis::new(f32::NAN, 0.).row_height(), 10.);
}

#[test]
fn a_row_height_change_keeps_the_middle_pitch() {
    // The roll keeps the pitch at the view's vertical middle.
    let a = PitchAxis::new(10., 600.);
    let middle = 200.;
    let before = a.pitch_at(middle);
    for h in [4., 12., 22., 40.] {
        assert_eq!(a.with_row_height(h, middle).pitch_at(middle), before);
    }
}

#[test]
fn centering_clamps_to_the_content() {
    let a = PitchAxis::new(10., 0.);
    let c = a.centering(60, 200.);
    assert!((c.center(60) - 100.).abs() < 0.001);
    assert_eq!(a.centering(127, 200.).offset(), 0.);
    assert_eq!(a.centering(0, 200.).offset(), 1_080.);
    assert_eq!(a.clamped(2_000.).offset(), 0., "taller than the content");
}

#[test]
fn visible_pitches() {
    let a = PitchAxis::new(10., 600.);
    assert_eq!(a.visible(100.), 58..=67);
}

#[test]
fn non_finite_inputs_never_trap() {
    let c = PitchAxis::new(10., 0.).centering(60, f32::NAN);
    assert!(c.offset().is_finite());
    let _ = c.pitch_at(5.);
    let mut a = PitchAxis::new(10., 600.);
    a.set_offset(f32::NAN);
    assert_eq!(a.offset(), 0.);
    let _ = a.row_at(5.);
    assert!(a.clamped(f32::INFINITY).offset().is_finite());
    assert_eq!(PitchAxis::new(10., f32::INFINITY).offset(), 0.);
    let _ = a.with_row_height(f32::NAN, f32::NAN);
    let _ = a.visible(f32::NAN);
}
