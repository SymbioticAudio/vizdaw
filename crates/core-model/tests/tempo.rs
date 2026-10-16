use core_model::ModelError;
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;

fn pt(tick: i64, bpm: u32, ramp: bool) -> TempoPoint {
    TempoPoint {
        tick: Tick(tick),
        bpm: Bpm::whole(bpm),
        ramp,
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn constant_tempo_converts_quarters_to_seconds() {
    let m = TempoMap::constant(Bpm::whole(120)).unwrap();
    assert!(close(m.tick_to_seconds(Tick(PPQ)), 0.5));
    assert!(close(m.tick_to_seconds(Tick(-PPQ)), -0.5));
    assert_eq!(m.seconds_to_tick(0.5), Tick(PPQ));
    assert_eq!(m.seconds_to_tick(-0.5), Tick(-PPQ));
}

#[test]
fn step_change_accumulates_segments() {
    let m = TempoMap::new(vec![pt(0, 120, false), pt(PPQ * 4, 60, false)]).unwrap();
    // 4 beats at 120 = 2 s, then 1 beat at 60 = 1 s.
    assert!(close(m.tick_to_seconds(Tick(PPQ * 5)), 3.0));
    assert_eq!(m.seconds_to_tick(3.0), Tick(PPQ * 5));
}

#[test]
fn linear_ramp_matches_closed_form() {
    // Ramp 60 -> 120 BPM over 4 beats: t = 60/(PPQ*k) * ln(b1/b0), k = 60/(4*PPQ).
    let m = TempoMap::new(vec![pt(0, 60, true), pt(PPQ * 4, 120, false)]).unwrap();
    let expected = 4.0 * std::f64::consts::LN_2;
    assert!(close(m.tick_to_seconds(Tick(PPQ * 4)), expected));
    assert_eq!(m.seconds_to_tick(expected), Tick(PPQ * 4));
    // After the ramp, constant 120.
    assert!(close(m.tick_to_seconds(Tick(PPQ * 5)), expected + 0.5));
}

#[test]
fn ramp_flag_on_last_point_is_ignored() {
    let m = TempoMap::new(vec![pt(0, 120, true)]).unwrap();
    assert!(close(m.tick_to_seconds(Tick(PPQ)), 0.5));
}

#[test]
fn validation_rejects_bad_maps() {
    assert!(matches!(
        TempoMap::new(vec![pt(0, 4, false)]),
        Err(ModelError::OutOfRange {
            field: "tempo.bpm",
            ..
        })
    ));
    assert!(matches!(
        TempoMap::new(vec![pt(0, 1000, false)]),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(matches!(
        TempoMap::new(vec![pt(10, 120, false)]),
        Err(ModelError::OutOfRange {
            field: "tempo.first_tick",
            ..
        })
    ));
    assert!(matches!(
        TempoMap::new(vec![pt(0, 120, false), pt(0, 90, false)]),
        Err(ModelError::OutOfRange {
            field: "tempo.tick_order",
            ..
        })
    ));
    assert!(TempoMap::new(vec![]).is_err());
}

#[test]
fn non_finite_seconds_do_not_panic() {
    let m = TempoMap::default();
    assert_eq!(m.seconds_to_tick(f64::INFINITY), Tick::MAX);
    assert_eq!(m.seconds_to_tick(f64::NEG_INFINITY), Tick::MIN);
    assert_eq!(m.seconds_to_tick(f64::NAN), Tick::MIN);
}

fn arb_map() -> impl Strategy<Value = TempoMap> {
    prop::collection::vec((1i64..PPQ * 16, 5u32..=999, any::<bool>()), 0..6).prop_map(|rest| {
        let mut pts = vec![pt(0, 120, true)];
        let mut t = 0;
        for (gap, bpm, ramp) in rest {
            t += gap;
            pts.push(pt(t, bpm, ramp));
        }
        TempoMap::new(pts).unwrap()
    })
}

proptest! {
    // Review focus #3: tick <-> seconds across linear tempo ramps.
    #[test]
    fn tick_seconds_round_trip(m in arb_map(), t in -PPQ * 8..PPQ * 2_000) {
        let s = m.tick_to_seconds(Tick(t));
        prop_assert_eq!(m.seconds_to_tick(s), Tick(t));
    }

    #[test]
    fn seconds_are_monotonic(m in arb_map(), t in 0i64..PPQ * 200) {
        prop_assert!(m.tick_to_seconds(Tick(t + 1)) > m.tick_to_seconds(Tick(t)));
    }
}
