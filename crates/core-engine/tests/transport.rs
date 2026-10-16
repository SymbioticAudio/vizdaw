use core_engine::midi1::Midi1Msg;
use core_engine::notes::ActiveNotes;
use core_engine::schedule::TargetIdx;
use core_engine::transport::{PlayState, Step, Transport};

/// Runs one block through the transport and returns its steps.
fn block(t: &mut Transport, length: u64, frames: u32) -> Vec<Step> {
    let mut steps = vec![];
    let mut offset = 0;
    while offset < frames {
        let s = t.next(length, offset, frames - offset);
        offset += s.frames();
        steps.push(s);
    }
    steps
}

#[test]
fn stopped_is_one_idle_step() {
    let mut t = Transport::new(100);
    assert_eq!(block(&mut t, 1000, 64), vec![Step::Idle { len: 64 }]);
}

#[test]
fn block_splits_exactly_at_loop_end() {
    let mut t = Transport::new(100);
    t.loop_range = Some((10, 50));
    t.locate(40);
    t.play(1000);
    assert_eq!(
        block(&mut t, 1000, 30),
        vec![
            Step::Play {
                from: 40,
                to: 50,
                offset: 0
            },
            Step::Wrap { offset: 10, to: 10 },
            Step::Play {
                from: 10,
                to: 30,
                offset: 10
            },
        ]
    );
    // A loop shorter than the block wraps several times.
    t.locate(10);
    let steps = block(&mut t, 1000, 100);
    assert_eq!(
        steps
            .iter()
            .filter(|s| matches!(s, Step::Wrap { .. }))
            .count(),
        2
    );
    assert_eq!(t.pos, 30);
}

#[test]
fn loop_end_on_a_block_boundary_wraps_in_the_next_block() {
    let mut t = Transport::new(100);
    t.loop_range = Some((0, 64));
    t.play(1000);
    assert_eq!(
        block(&mut t, 1000, 64),
        vec![Step::Play {
            from: 0,
            to: 64,
            offset: 0
        }]
    );
    assert_eq!(
        block(&mut t, 1000, 64),
        vec![
            Step::Wrap { offset: 0, to: 0 },
            Step::Play {
                from: 0,
                to: 64,
                offset: 0
            }
        ]
    );
}

#[test]
fn end_plays_the_tail_then_stops_once() {
    let mut t = Transport::new(100);
    t.play(150);
    assert_eq!(
        block(&mut t, 150, 128),
        vec![Step::Play {
            from: 0,
            to: 128,
            offset: 0
        }]
    );
    assert_eq!(
        block(&mut t, 150, 128),
        vec![
            Step::Play {
                from: 128,
                to: 150,
                offset: 0
            },
            Step::End { offset: 22 },
            Step::Idle { len: 100 },
            Step::Idle { len: 6 },
        ]
    );
    assert_eq!(t.state, PlayState::Stopped);
    assert!(t.take_ended());
    assert!(!t.take_ended());
    // Play at the end starts over.
    t.play(150);
    assert_eq!(t.pos, 0);
}

#[test]
fn loop_past_its_end_plays_to_the_sequence_end() {
    let mut t = Transport::new(0);
    t.loop_range = Some((0, 50));
    t.locate(60);
    t.play(100);
    assert_eq!(
        block(&mut t, 100, 64),
        vec![
            Step::Play {
                from: 60,
                to: 100,
                offset: 0
            },
            Step::End { offset: 40 },
            Step::Idle { len: 24 }
        ]
    );
}

#[test]
fn empty_or_inverted_loops_are_ignored() {
    let mut t = Transport::new(0);
    t.loop_range = Some((50, 50));
    t.play(60);
    let steps = block(&mut t, 60, 64);
    assert!(!steps.iter().any(|s| matches!(s, Step::Wrap { .. })));
}

#[test]
fn notes_and_pedals_are_released() {
    let mut n = ActiveNotes::default();
    let t = TargetIdx(2);
    n.observe(t, &[0x90, 60, 100]);
    n.observe(t, &[0x91, 61, 100]);
    n.observe(t, &[0x90, 62, 100]);
    n.observe(t, &[0x80, 62, 0]);
    n.observe(t, &[0x90, 63, 0]); // velocity 0 = off
    n.observe(t, &[0xB1, 64, 127]);
    assert!(n.any());
    let mut sent = vec![];
    n.release(t, |m: Midi1Msg| sent.push(m.as_slice().to_vec()));
    assert_eq!(
        sent,
        vec![vec![0x80, 60, 0], vec![0x81, 61, 0], vec![0xB1, 64, 0]]
    );
    assert!(!n.any());
    let mut again = 0;
    n.release(t, |_| again += 1);
    assert_eq!(again, 0);
}
