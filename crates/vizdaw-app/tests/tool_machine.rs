//! Swift `ToolMachineTests`: press, drag past the dead zone, release and
//! Escape.

use vizdaw_app::arrangement::{MachineEvent, Modifiers, Phase, Point, ToolMachine};

type M = ToolMachine<&'static str>;
type E = MachineEvent<&'static str>;

fn p(x: f32, y: f32) -> Point {
    Point { x, y }
}

const NONE: Modifiers = Modifiers::NONE;

#[test]
fn a_click_without_movement_is_a_click() {
    let mut m = M::new();
    assert!(m.down("a", p(10., 10.), 2, Modifiers::SHIFT).is_empty());
    assert!(m.drag(p(12., 11.), NONE).is_empty(), "inside the dead zone");
    assert_eq!(
        m.up(p(12., 11.), NONE),
        vec![E::Click {
            hit: "a",
            at: p(10., 10.),
            clicks: 2,
            modifiers: Modifiers::SHIFT
        }]
    );
    assert_eq!(*m.phase(), Phase::Idle);
}

#[test]
fn moving_past_three_points_begins_a_drag() {
    let mut m = M::new();
    let _ = m.down("a", p(0., 0.), 1, NONE);
    assert!(
        m.drag(p(3., 0.), NONE).is_empty(),
        "exactly 3 is still inside"
    );
    assert_eq!(
        m.drag(p(4., 0.), Modifiers::OPTION),
        vec![
            E::Begin {
                hit: "a",
                from: p(0., 0.),
                modifiers: Modifiers::OPTION
            },
            E::Move {
                hit: "a",
                from: p(0., 0.),
                to: p(4., 0.)
            },
        ]
    );
    assert_eq!(
        m.drag(p(9., 2.), NONE),
        vec![E::Move {
            hit: "a",
            from: p(0., 0.),
            to: p(9., 2.)
        }]
    );
    assert_eq!(
        m.up(p(9., 2.), NONE),
        vec![E::End {
            hit: "a",
            from: p(0., 0.),
            to: p(9., 2.),
            modifiers: NONE
        }]
    );
}

#[test]
fn escape_cancels_a_drag_and_ignores_the_rest() {
    let mut m = M::new();
    let _ = m.down("a", p(0., 0.), 1, NONE);
    let _ = m.drag(p(10., 0.), NONE);
    assert_eq!(m.cancel(), vec![E::Cancel]);
    assert!(m.drag(p(20., 0.), NONE).is_empty());
    assert!(m.up(p(20., 0.), NONE).is_empty());
}

#[test]
fn escape_before_the_dead_zone_does_nothing() {
    let mut m = M::new();
    let _ = m.down("a", p(0., 0.), 1, NONE);
    assert!(m.cancel().is_empty());
    assert!(m.up(p(0., 0.), NONE).is_empty());
}

#[test]
fn a_down_during_a_drag_cancels_the_lost_one() {
    let mut m = M::new();
    let _ = m.down("a", p(0., 0.), 1, NONE);
    let _ = m.drag(p(10., 0.), NONE);
    assert_eq!(m.down("b", p(0., 0.), 1, NONE), vec![E::Cancel]);
}
