//! MIDI note input (plan 4c, Plan Ruling 11): note-ons queue with MIDI 2.0
//! velocities, note-offs (plan 4f) without; other messages do not.

use vizdaw_ffi::input::{CAPACITY, upscale_velocity};
use vizdaw_ffi::{NoteInRow, NoteInput};

#[test]
fn notes_queue_in_order_and_nothing_else_does() {
    let n = NoteInput::new();
    n.inject(vec![0x90, 60, 127]).unwrap();
    n.inject(vec![0x80, 60, 40]).unwrap();
    n.inject(vec![0x91, 62, 0]).unwrap();
    n.inject(vec![0xB0, 7, 100]).unwrap();
    n.inject(vec![0x9F, 64, 1]).unwrap();
    n.inject(vec![0x90, 60]).unwrap();
    let note = |pitch, velocity, channel, on| NoteInRow {
        pitch,
        velocity,
        channel,
        on,
    };
    assert_eq!(
        n.take().unwrap(),
        vec![
            note(60, 0xFFFF, 0, true),
            note(60, 0, 0, false),
            note(62, 0, 1, false),
            note(64, 1 << 9, 15, true),
        ]
    );
    assert!(n.take().unwrap().is_empty());
}

#[test]
fn the_queue_keeps_the_newest_notes() {
    let n = NoteInput::new();
    for i in 0..300u32 {
        n.inject(vec![0x90, (i % 128) as u8, 100]).unwrap();
    }
    let got = n.take().unwrap();
    assert_eq!(got.len(), CAPACITY);
    assert_eq!(got[0].pitch, 44, "the oldest 44 were dropped");
}

#[test]
fn velocities_upscale_by_the_midi_2_rule() {
    assert_eq!(upscale_velocity(0), 0);
    assert_eq!(upscale_velocity(64), 0x8000);
    assert_eq!(upscale_velocity(100), 0xC924);
    assert_eq!(upscale_velocity(127), 0xFFFF);
    assert_eq!(
        upscale_velocity(100) >> 9,
        100,
        "the 7-bit value reads back"
    );
}

#[test]
fn starting_again_and_stopping_never_fail() {
    let n = NoteInput::new();
    n.start().unwrap();
    n.start().unwrap();
    n.stop().unwrap();
    n.stop().unwrap();
}
