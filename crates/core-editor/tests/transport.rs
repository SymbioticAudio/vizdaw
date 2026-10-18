//! The transport keys' logic (plan 4f Task 3): markers and record arm.

use core_editor::transport::{ArmMove, arm_move, marker_step};
use core_model::ids::SeqId;
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
