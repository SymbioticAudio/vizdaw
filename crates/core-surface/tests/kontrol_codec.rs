//! The Kontrol S-Series MK3 codec, byte by byte (plan 5f-4 Task 1;
//! spec §8.4). Protocol facts from the public DrivenByMoss sources
//! (LGPL-3.0), cross-checked against ReaKontrol and flmidi; no code copied.

use core_surface::TrackKind;
use core_surface::kontrol::{
    HEADER, MAX_TEXT, ascii, color, is_daw_port, is_meter, parse_sysex, relative, sysex, tempo_bpm,
    tempo_bytes, track_type, vu,
};

#[test]
fn sysex_frames_the_header_command_value_index_and_info() {
    assert_eq!(
        HEADER,
        [0xF0, 0x00, 0x21, 0x09, 0x00, 0x00, 0x44, 0x43, 0x01, 0x00]
    );
    assert_eq!(
        sysex(0x48, 0, 3, b"Bass"),
        vec![
            0xF0, 0x00, 0x21, 0x09, 0x00, 0x00, 0x44, 0x43, 0x01, 0x00, 0x48, 0x00, 0x03, b'B',
            b'a', b's', b's', 0xF7
        ]
    );
    assert_eq!(sysex(0x40, 0, 7, &[]).len(), 14);
}

#[test]
fn parse_sysex_refuses_other_headers_cut_frames_and_high_bytes() {
    let m = sysex(0x48, 0, 3, b"Bass");
    assert_eq!(parse_sysex(&m), Some((0x48, 0, 3, &b"Bass"[..])));
    assert_eq!(
        parse_sysex(&sysex(0x42, 1, 2, &[])),
        Some((0x42, 1, 2, &[][..]))
    );
    let mut other = m.clone();
    other[3] = 0x0A;
    assert_eq!(parse_sysex(&other), None, "another manufacturer");
    let mut v2 = m.clone();
    v2[8] = 0x02;
    assert_eq!(parse_sysex(&v2), None, "another header version");
    assert_eq!(parse_sysex(&m[..m.len() - 1]), None, "no F7");
    let mut high = m.clone();
    high[13] = 0x80;
    assert_eq!(parse_sysex(&high), None, "a byte above 0x7F");
    let mut short = HEADER.to_vec();
    short.extend([0x42, 0x00, 0xF7]);
    assert_eq!(parse_sysex(&short), None, "no index");
    assert_eq!(parse_sysex(&[0xF0, 0xF7]), None);
    assert_eq!(parse_sysex(&[]), None);
    assert_eq!(parse_sysex(&[0xBF, 0x10, 0x01]), None);
}

/// Pre-flight I3: a value or index above 127 is sent as 127, so a frame
/// never holds a data byte above 0x7F.
#[test]
fn sysex_keeps_value_and_index_seven_bit() {
    let m = sysex(0x74, 0x80, 0xFF, b"");
    assert_eq!(
        (m[11], m[12]),
        (0x7F, 0x7F),
        "a value above 127 is sent as 127"
    );
    assert!(m[1..m.len() - 1].iter().all(|b| *b <= 0x7F), "{m:02x?}");
    assert_eq!(parse_sysex(&m), Some((0x74, 0x7F, 0x7F, &[][..])));
    assert_eq!(sysex(0x48, 0x7F, 0x7F, b"a")[11..13], [0x7F, 0x7F]);
}

#[test]
fn display_text_is_printable_ascii_and_at_most_32_bytes() {
    assert_eq!(MAX_TEXT, 32);
    assert_eq!(ascii("Bass"), b"Bass".to_vec());
    assert_eq!(ascii("Café"), b"Caf?".to_vec());
    assert_eq!(ascii("a\tb\u{7f}"), b"a?b?".to_vec());
    assert_eq!(ascii("🎹 keys"), b"? keys".to_vec());
    assert_eq!(ascii(&"x".repeat(40)).len(), 32);
    assert_eq!(ascii(&"Ü".repeat(40)), vec![b'?'; 32]);
    assert_eq!(ascii(""), Vec::<u8>::new());
}

#[test]
fn tempo_is_a_quarter_note_in_10_ns_steps_seven_bits_at_a_time() {
    assert_eq!(tempo_bytes(120.0), Some([0x00, 0x61, 0x6B, 0x17, 0x00]));
    assert_eq!(tempo_bytes(90.0), Some([0x2B, 0x01, 0x65, 0x1F, 0x00]));
    let back = tempo_bpm([0x00, 0x61, 0x6B, 0x17, 0x00]).unwrap();
    assert!((back - 120.0).abs() < 1e-9, "{back}");
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 0.1] {
        assert_eq!(tempo_bytes(bad), None, "{bad}");
    }
    assert!(tempo_bytes(5.0).is_some(), "slow tempos fit 35 bits");
    assert_eq!(tempo_bpm([0; 5]), None);
    assert_eq!(tempo_bpm([0x80, 0, 0, 0, 0]), None);
}

#[test]
fn relative_values_are_twos_complement_on_seven_bits() {
    assert_eq!(
        [1, 63, 127, 65, 0, 64].map(relative),
        [1, 63, -1, -63, 0, 0]
    );
}

#[test]
fn daw_ports_are_found_by_kontrol_mk3_and_daw() {
    for yes in [
        "KONTROL S88 MK3 DAW",
        "KONTROL S49 MK3 DAW",
        "Native Instruments - KONTROL S61 MK3 - DAW",
        "kontrol s88 mk3 daw",
    ] {
        assert!(is_daw_port(yes), "{yes}");
    }
    for no in [
        "KONTROL S88 MK3 Main",
        "KONTROL S88 MK3 MIDI 2.0",
        "Komplete Kontrol DAW - 1",
        "KONTROL S88 MK3",
        "MIDIIN2 (KONTROL S88 MK3)",
        "DAWKONTROL MK3",
        "",
    ] {
        assert!(!is_daw_port(no), "{no}");
    }
}

#[test]
fn vu_maps_60_db_onto_0_to_127() {
    assert_eq!(
        [1.0, 2.0, 0.5, 0.25, 0.001, 0.0, f32::NAN, -1.0].map(vu),
        [127, 127, 114, 102, 0, 0, 0, 0]
    );
}

#[test]
fn track_types_and_colours_follow_the_strip_kind() {
    assert_eq!(track_type(None), 0);
    assert_eq!(
        [
            TrackKind::Instrument,
            TrackKind::Audio,
            TrackKind::Bus,
            TrackKind::Master
        ]
        .map(|k| (track_type(Some(k)), color(k))),
        [
            (2, "#3C8CE6"),
            (3, "#46B45A"),
            (5, "#E6963C"),
            (6, "#B4B4B4")
        ]
    );
}

#[test]
fn the_vu_message_is_a_meter() {
    assert!(is_meter(&sysex(0x49, 2, 0, &[0; 16])));
    assert!(!is_meter(&sysex(0x48, 0, 0, b"VU")));
    assert!(!is_meter(&[0xD0, 0x10]));
}
