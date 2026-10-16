use core_engine::midi1::{
    Class, Midi1Msg, When, bend14, note_on_velocity, release_velocity, translate, value7,
};
use core_model::event::{EventBody, NoteAttribute, PerNoteCtl, PerNoteKind, TextKind};
use core_model::time::Tick;
use im::Vector;
use std::sync::Arc;

#[test]
fn velocity_table() {
    // (16-bit velocity, 7-bit note-on velocity)
    let table = [
        (0x0000, 1),
        (0x0001, 1),
        (0x01FF, 1),
        (0x0200, 1),
        (0x0400, 2),
        (0x8000, 64),
        (0xFFFF, 127),
    ];
    for (v16, v7) in table {
        assert_eq!(note_on_velocity(v16), v7, "velocity {v16:#06x}");
    }
    assert_eq!(release_velocity(0), 0);
    assert_eq!(release_velocity(0xFFFF), 127);
}

#[test]
fn controller_and_bend_tables() {
    assert_eq!(value7(0), 0);
    assert_eq!(value7(0xFFFF_FFFF), 127);
    assert_eq!(value7(100 << 25), 100);
    assert_eq!(bend14(0x8000_0000), (0x00, 0x40)); // centre 0x2000
    assert_eq!(bend14(0x0000_0000), (0x00, 0x00));
    assert_eq!(bend14(0xFFFF_FFFF), (0x7F, 0x7F));
}

#[test]
fn note_becomes_on_and_off() {
    let body = EventBody::Note {
        pitch: 60,
        velocity: 0x8000,
        release_velocity: 0x4000,
        duration: Tick(100),
        attribute: None,
        per_note: Vector::new(),
    };
    let t = translate(&body, 3);
    let msgs = t.msgs();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].when, When::Start);
    assert_eq!(msgs[0].class, Class::NoteOn);
    assert_eq!(msgs[0].msg, Midi1Msg::three(0x93, 60, 64));
    assert_eq!(msgs[1].when, When::End);
    assert_eq!(msgs[1].class, Class::NoteOff);
    assert_eq!(msgs[1].msg, Midi1Msg::three(0x83, 60, 32));
}

#[test]
fn bank_then_program_in_patch_class() {
    let body = EventBody::Program {
        program: 33,
        bank: Some((1, 2)),
    };
    let t = translate(&body, 0);
    let got: Vec<(Class, &[u8])> = t
        .msgs()
        .iter()
        .map(|o| (o.class, o.msg.as_slice()))
        .collect();
    assert_eq!(
        got,
        vec![
            (Class::Patch, &[0xB0, 0, 1][..]),
            (Class::Patch, &[0xB0, 32, 2][..]),
            (Class::Patch, &[0xC0, 33][..]),
        ]
    );
}

#[test]
fn controllers_and_pressure() {
    let cc = translate(
        &EventBody::Cc {
            index: 7,
            value: 100 << 25,
        },
        1,
    );
    assert_eq!(cc.msgs()[0].msg.as_slice(), &[0xB1, 7, 100]);
    let pb = translate(&EventBody::PitchBend { value: 0x8000_0000 }, 1);
    assert_eq!(pb.msgs()[0].msg.as_slice(), &[0xE1, 0x00, 0x40]);
    let cp = translate(&EventBody::ChannelPressure { value: 0xFFFF_FFFF }, 2);
    assert_eq!(cp.msgs()[0].msg.as_slice(), &[0xD2, 127]);
    let pp = translate(
        &EventBody::PolyPressure {
            pitch: 61,
            value: 64 << 25,
        },
        2,
    );
    assert_eq!(pp.msgs()[0].msg.as_slice(), &[0xA2, 61, 64]);
    assert!(pp.msgs().iter().all(|o| o.class == Class::Control));
}

#[test]
fn midi2_only_data_is_counted() {
    let body = EventBody::Note {
        pitch: 60,
        velocity: 0x8000,
        release_velocity: 0,
        duration: Tick(10),
        attribute: Some(NoteAttribute {
            kind: 3,
            data: 0x1234,
        }),
        per_note: Vector::from(vec![
            PerNoteCtl {
                kind: PerNoteKind::PitchBend,
                offset: Tick(0),
                value: 1,
            },
            PerNoteCtl {
                kind: PerNoteKind::Registered(7),
                offset: Tick(1),
                value: 2,
            },
        ]),
    };
    let t = translate(&body, 0);
    assert_eq!(t.drops.per_note, 2);
    assert!(t.drops.attribute);
    assert_eq!(t.msgs().len(), 2, "the note itself still plays");

    let sysex8 = EventBody::SysEx {
        data: Arc::from(&[0x7E, 0x80][..]),
        eight_bit: true,
    };
    let t = translate(&sysex8, 0);
    assert!(t.drops.sysex8);
    assert_eq!(t.sysex, None);
}

#[test]
fn sysex7_passes_and_text_is_silent() {
    let body = EventBody::SysEx {
        data: Arc::from(&[0x7E, 0x7F, 0x09, 0x01][..]),
        eight_bit: false,
    };
    let t = translate(&body, 0);
    assert_eq!(t.sysex, Some(&[0x7E, 0x7F, 0x09, 0x01][..]));
    assert!(t.msgs().is_empty());
    let text = EventBody::Text {
        kind: TextKind::Lyric,
        text: "la".into(),
    };
    let t = translate(&text, 0);
    assert!(t.msgs().is_empty() && t.sysex.is_none());
}

#[test]
fn class_order() {
    assert!(Class::NoteOff < Class::Patch);
    assert!(Class::Patch < Class::Control);
    assert!(Class::Control < Class::NoteOn);
    assert_eq!(Class::of(0x95), Class::NoteOn);
    assert_eq!(Class::of(0x85), Class::NoteOff);
    assert_eq!(Class::of(0xC5), Class::Patch);
    assert_eq!(Class::of(0xB5), Class::Control);
}

#[test]
fn raw_bank_select_ccs_sort_as_patch() {
    // (CC index, expected sort class) — ruling O19.
    let table = [
        (0u8, Class::Patch),
        (32, Class::Patch),
        (1, Class::Control),
        (7, Class::Control),
        (31, Class::Control),
        (33, Class::Control),
        (64, Class::Control),
        (127, Class::Control),
    ];
    for (index, class) in table {
        let body = EventBody::Cc { index, value: 0 };
        let t = translate(&body, 0);
        assert_eq!(t.msgs()[0].class, class, "CC {index}");
    }
}
