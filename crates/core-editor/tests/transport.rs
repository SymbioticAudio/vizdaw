//! The transport keys' logic (plan 4f Task 3): markers and record arm.

use core_editor::transport::{
    ArmMove, WIND_LIMIT, arm_move, bar_step, bar_step_from, marker_step, wind_target,
};
use core_model::ids::SeqId;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::sequence::{Marker, Sequence};
use core_model::time::Tick;
use proptest::prelude::*;

#[test]
fn markers_step_to_the_nearest_one_either_way() {
    let mut s = Sequence::new(SeqId(1), "Song", Default::default());
    assert_eq!(marker_step(&s, Tick(0), true), None, "no markers");
    // Stored out of order: the nearest counts, not the next in the list.
    for t in [61_440, 0, 15_360] {
        s.markers.push_back(Marker {
            tick: Tick(t),
            name: format!("m{t}"),
        });
    }
    assert_eq!(
        marker_step(&s, Tick(0), true),
        Some(Tick(15_360)),
        "strictly after"
    );
    assert_eq!(marker_step(&s, Tick(15_360), true), Some(Tick(61_440)));
    assert_eq!(marker_step(&s, Tick(61_440), true), None);
    assert_eq!(marker_step(&s, Tick(20_000), false), Some(Tick(15_360)));
    assert_eq!(
        marker_step(&s, Tick(15_360), false),
        Some(Tick(0)),
        "strictly before"
    );
    assert_eq!(marker_step(&s, Tick(0), false), None);
    assert_eq!(marker_step(&s, Tick(i64::MIN), true), Some(Tick(0)));
    assert_eq!(marker_step(&s, Tick(i64::MAX), false), Some(Tick(61_440)));
}

#[test]
fn record_arm_moves_one_lane_and_leaves_one_armed() {
    let m = |disarm: Vec<usize>, arm| Some(ArmMove { disarm, arm });
    assert_eq!(
        arm_move(&[false, false, false], true),
        m(vec![], 0),
        "none armed: the first"
    );
    assert_eq!(
        arm_move(&[false, false, false], false),
        m(vec![], 2),
        "or the last"
    );
    assert_eq!(arm_move(&[false, true, false], true), m(vec![1], 2));
    assert_eq!(arm_move(&[false, true, false], false), m(vec![1], 0));
    assert_eq!(arm_move(&[false, false, true], true), None, "at the end");
    assert_eq!(arm_move(&[true, false, false], false), None, "at the start");
    assert_eq!(
        arm_move(&[true, false, true, false], true),
        m(vec![0, 2], 3),
        "from the last armed, all others disarmed"
    );
    assert_eq!(
        arm_move(&[true, false, true, false], false),
        None,
        "from the first"
    );
    assert_eq!(arm_move(&[], true), None);
    assert_eq!(arm_move(&[], false), None);
}

proptest! {
    /// A move arms one lane in range next to the armed ones and disarms
    /// every armed lane; None only at an end or without lanes.
    #[test]
    fn arm_moves_stay_in_range(armed in prop::collection::vec(any::<bool>(), 0..12), forward in any::<bool>()) {
        match arm_move(&armed, forward) {
            Some(m) => {
                prop_assert!(m.arm < armed.len());
                let on: Vec<usize> = (0..armed.len()).filter(|i| armed[*i]).collect();
                prop_assert_eq!(&m.disarm, &on);
                match (forward, on.first(), on.last()) {
                    (true, _, Some(l)) => prop_assert_eq!(m.arm, l + 1),
                    (false, Some(f), _) => prop_assert_eq!(m.arm + 1, *f),
                    (true, _, None) => prop_assert_eq!(m.arm, 0),
                    (false, None, _) => prop_assert_eq!(m.arm, armed.len() - 1),
                }
            }
            None => {
                let edge = if forward { armed.last() } else { armed.first() };
                prop_assert!(armed.is_empty() || edge == Some(&true));
            }
        }
    }
}

/// 4/4 from 0, 3/4 from bar 3 (122 880).
fn four_then_three() -> MeterMap {
    let entry = |tick, num| MeterEntry {
        tick: Tick(tick),
        meter: Meter { num, den: 4 },
        key: KeySig::default(),
    };
    MeterMap::new(vec![entry(0, 4), entry(122_880, 3)]).unwrap()
}

/// Plan 7d Ruling 3: a click on rewind or fast forward moves one bar of
/// the meter in effect, never below 0.
#[test]
fn bar_step_moves_one_bar_across_meter_changes() {
    let m = four_then_three();
    for (from, to) in [
        (0, 61_440),
        (61_440, 122_880),
        (122_880, 168_960),
        (130_000, 168_960),
        (-100, 0),
    ] {
        assert_eq!(
            bar_step(&m, Tick(from), true),
            Tick(to),
            "forward from {from}"
        );
    }
    for (from, to) in [
        (168_960, 122_880),
        (130_000, 122_880),
        (122_880, 61_440),
        (1, 0),
        (0, 0),
        (-100, 0),
    ] {
        assert_eq!(
            bar_step(&m, Tick(from), false),
            Tick(to),
            "back from {from}"
        );
    }
}

/// Plan 7d Ruling 5: steps and winds stay inside 0…=WIND_LIMIT, and a
/// non-finite wind is refused.
#[test]
fn bar_step_and_wind_target_clamp() {
    let m = four_then_three();
    assert_eq!(WIND_LIMIT, 1 << 50);
    assert_eq!(bar_step(&m, Tick(WIND_LIMIT), true), Tick(WIND_LIMIT));
    assert_eq!(bar_step(&m, Tick(i64::MAX), true), Tick(WIND_LIMIT));
    for q in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(wind_target(0, q), None, "{q}");
    }
    assert_eq!(wind_target(0, 1.0), Some(15_360));
    assert_eq!(wind_target(0, 1e300), Some(WIND_LIMIT));
    assert_eq!(wind_target(100, -1e300), Some(0));
    assert_eq!(wind_target(i64::MIN, 0.0), Some(0));
    assert_eq!(wind_target(30_720, -0.5), Some(23_040));
}

/// Plan 7d final review I-1 (controller ruling): while playing, a back
/// step within one beat after the current bar's barline goes to the
/// previous bar's start; otherwise to the current bar's start. Stopped,
/// and forward, are `bar_step`.
#[test]
fn a_back_step_while_playing_just_after_a_barline_goes_to_the_one_before() {
    const BAR: i64 = 61_440;
    const BEAT: i64 = 15_360;
    let m = four_then_three();
    // Bar 5 of 4/4 starts at 4 × BAR in a plain 4/4 map.
    let four = MeterMap::default();
    let bar5 = 4 * BAR;
    assert_eq!(
        bar_step_from(&four, Tick(bar5 + BEAT + 1), false, true),
        Tick(bar5)
    );
    assert_eq!(
        bar_step_from(&four, Tick(bar5 + BEAT - 1), false, true),
        Tick(bar5 - BAR)
    );
    assert_eq!(
        bar_step_from(&four, Tick(bar5), false, true),
        Tick(bar5 - BAR)
    );
    assert_eq!(
        bar_step_from(&four, Tick(bar5 + 1), false, true),
        Tick(bar5 - BAR)
    );
    // Stopped is unchanged: the last barline before.
    for from in [bar5 + BEAT + 1, bar5 + BEAT - 1, bar5 + 1, bar5] {
        assert_eq!(
            bar_step_from(&four, Tick(from), false, false),
            bar_step(&four, Tick(from), false),
            "stopped from {from}"
        );
    }
    // Forward is the same playing or not.
    assert_eq!(
        bar_step_from(&four, Tick(bar5 + 1), true, true),
        Tick(bar5 + BAR)
    );
    // A beat of the meter in effect: 3/4 beats are the same 15 360 here,
    // and the previous bar is a 3/4 one.
    assert_eq!(bar_step_from(&m, Tick(130_000), false, true), Tick(61_440));
    assert_eq!(
        bar_step_from(&m, Tick(122_880 + BEAT + 1), false, true),
        Tick(122_880)
    );
    // The first bar, and below it, stay at 0; hostile ticks clamp.
    assert_eq!(bar_step_from(&four, Tick(5), false, true), Tick(0));
    assert_eq!(bar_step_from(&four, Tick(0), false, true), Tick(0));
    assert_eq!(bar_step_from(&four, Tick(-9), false, true), Tick(0));
    assert!(bar_step_from(&four, Tick(i64::MAX), false, true).0 <= WIND_LIMIT);
    assert_eq!(bar_step_from(&four, Tick(i64::MIN), false, true), Tick(0));
}
