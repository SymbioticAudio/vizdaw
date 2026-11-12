//! MIDI Clip File bytes: the golden, long gaps, text over several packets,
//! the header rule, hostile files and the UMP builders (plan 6a Task 4).

use core_io::IoError;
use core_io::clip::{ClipEvent, ClipFile, read, write};
use core_io::limits::Limits;
use core_io::report::{ItemKind, Report};
use core_io::ump::{self, Decoded, NoteAttribute, Packet};
use proptest::prelude::*;
use std::sync::atomic::AtomicBool;

/// Bytes from hex digits; spaces and newlines are ignored.
fn hex(s: &str) -> Vec<u8> {
    let d: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    d.chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}

/// The big-endian bytes of `words`.
fn be(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_be_bytes()).collect()
}

fn bytes_of(p: &Packet) -> Vec<u8> {
    be(p.words())
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

fn rd(bytes: &[u8]) -> Result<(ClipFile, Report), IoError> {
    read(bytes, &Limits::default(), &no_cancel())
}

fn rd_with(bytes: &[u8], limits: &Limits) -> Result<(ClipFile, Report), IoError> {
    read(bytes, limits, &no_cancel())
}

#[track_caller]
fn refused(r: Result<(ClipFile, Report), IoError>, needle: &str) {
    match r {
        Err(IoError::Refused(m)) => assert!(m.contains(needle), "{m:?} lacks {needle:?}"),
        other => panic!("expected a refusal containing {needle:?}, got {other:?}"),
    }
}

#[track_caller]
fn has(r: &Report, kind: ItemKind, needle: &str) -> u64 {
    r.items
        .iter()
        .find(|i| i.kind == kind && i.what.contains(needle))
        .unwrap_or_else(|| panic!("no {kind:?} item containing {needle:?} in {:?}", r.items))
        .count
}

const MAGIC: &str = "534D4632 434C4950";
const DCS0: &str = "00400000";
const START: &str = "F0200000 00000000 00000000 00000000";
const END: &str = "F0210000 00000000 00000000 00000000";

/// A clip at 960 ticks per quarter, no header messages, with `body` (hex)
/// between Start of Clip and End of Clip.
fn clip_hex(body: &str) -> Vec<u8> {
    hex(&format!(
        "{MAGIC} {DCS0} 003003C0 {DCS0} {START} {body} {DCS0} {END}"
    ))
}

fn note_on() -> Packet {
    ump::note_on(0, 0, 60, 0x8000, None)
}

fn note_hex() -> String {
    format!("{DCS0} 40903C00 80000000")
}

fn golden() -> ClipFile {
    ClipFile {
        tpq: 15360,
        header: vec![
            ClipEvent::Packet(ump::set_tempo(50_000_000)),
            ClipEvent::Packet(ump::time_sig(4, 2, 8)),
        ],
        events: vec![
            (0, ClipEvent::Packet(ump::note_on(0, 0, 60, 0x8000, None))),
            (15360, ClipEvent::Packet(ump::note_off(0, 0, 60, 0, None))),
        ],
        end: 15360,
    }
}

#[test]
fn writes_the_minimal_golden() {
    let mut want = hex(&format!("{MAGIC} 00400000 00303C00 00400000"));
    want.extend(bytes_of(&ump::set_tempo(50_000_000)));
    want.extend(hex("00400000"));
    want.extend(bytes_of(&ump::time_sig(4, 2, 8)));
    want.extend(hex(&format!("00400000 {START} 00400000")));
    want.extend(bytes_of(&ump::note_on(0, 0, 60, 0x8000, None)));
    want.extend(hex("00403C00"));
    want.extend(bytes_of(&ump::note_off(0, 0, 60, 0, None)));
    want.extend(hex(&format!("00400000 {END}")));
    // The builders give the checked layouts (M2-104-UM figures 70, 71, 46, 47).
    assert_eq!(
        bytes_of(&ump::set_tempo(50_000_000)),
        hex("D0100000 02FAF080 00000000 00000000")
    );
    assert_eq!(
        bytes_of(&ump::time_sig(4, 2, 8)),
        hex("D0100001 04020800 00000000 00000000")
    );
    assert_eq!(bytes_of(&note_on()), hex("40903C00 80000000"));

    let bytes = write(&golden());
    assert_eq!(bytes, want);
    assert_eq!(bytes, include_bytes!("data/golden_minimal.midi2").to_vec());
    let (c, report) = rd(&bytes).unwrap();
    assert_eq!(c, golden());
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn long_gaps_restart_with_noop() {
    let c = ClipFile {
        tpq: 960,
        header: vec![],
        events: vec![(2_100_000, ClipEvent::Packet(note_on()))],
        end: 2_100_000,
    };
    let bytes = write(&c);
    let gap = hex("004FFFFF 00000000 004FFFFF 00000000 00400B22 40903C00 80000000");
    assert!(
        bytes.windows(gap.len()).any(|w| w == gap.as_slice()),
        "no restarted gap in {bytes:02X?}"
    );
    let (back, report) = rd(&bytes).unwrap();
    assert_eq!(back, c);
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn text_spans_packets() {
    let name = "A clip name of exactly forty bytes long!";
    assert_eq!(name.len(), 40);
    let packets = ump::text(0x01, 0x03, 0, name);
    let forms: Vec<u32> = packets.iter().map(|p| (p.words()[0] >> 22) & 3).collect();
    assert_eq!(forms, vec![1, 2, 2, 3]);
    assert!(
        packets
            .iter()
            .all(|p| p.mt() == 0xD && p.words()[0] & 0xFFFF == 0x0103)
    );
    let text = ClipEvent::Text {
        group: 0,
        bank: 0x01,
        status: 0x03,
        text: name.to_string(),
    };
    let c = ClipFile {
        tpq: 960,
        header: vec![],
        events: vec![(0, text)],
        end: 0,
    };
    let (back, report) = rd(&write(&c)).unwrap();
    assert_eq!(back, c);
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn header_holds_at_most_tempo_and_meter() {
    let ts = |n: u8| bytes_of(&ump::time_sig(n, 2, 8));
    let mut b = hex(&format!("{MAGIC} {DCS0} 003003C0 {DCS0}"));
    b.extend(bytes_of(&ump::set_tempo(50_000_000)));
    b.extend(hex(DCS0));
    b.extend(ts(4));
    b.extend(hex(DCS0));
    b.extend(ts(3));
    b.extend(hex(&format!(
        "{DCS0} {START} 00400010 40903C00 80000000 {DCS0} {END}"
    )));
    let (c, report) = rd(&b).unwrap();
    assert_eq!(
        c.header,
        vec![
            ClipEvent::Packet(ump::set_tempo(50_000_000)),
            ClipEvent::Packet(ump::time_sig(4, 2, 8)),
        ]
    );
    assert_eq!(
        c.events,
        vec![
            (0, ClipEvent::Packet(ump::time_sig(3, 2, 8))),
            (16, ClipEvent::Packet(note_on())),
        ]
    );
    assert_eq!(c.end, 16);
    assert_eq!(
        has(&report, ItemKind::Changed, "moved to the start of the clip"),
        1
    );
    // A time signature before the tempo is put after it.
    let mut b = hex(&format!("{MAGIC} {DCS0} 003003C0 {DCS0}"));
    b.extend(ts(4));
    b.extend(hex(DCS0));
    b.extend(bytes_of(&ump::set_tempo(50_000_000)));
    b.extend(hex(&format!("{DCS0} {START} {DCS0} {END}")));
    let (c, _) = rd(&b).unwrap();
    assert_eq!(
        c.header,
        vec![
            ClipEvent::Packet(ump::set_tempo(50_000_000)),
            ClipEvent::Packet(ump::time_sig(4, 2, 8)),
        ]
    );
}

#[test]
fn hostile_clip_is_refused_or_reported() {
    refused(
        rd(&hex("534D4632 434C4951 00400000")),
        "This isn't a MIDI Clip File.",
    );
    refused(rd(b"SMF2"), "This isn't a MIDI Clip File.");
    refused(rd(b""), "This isn't a MIDI Clip File.");

    // No DCTPQ, DCTPQ 0.
    refused(
        rd(&hex(&format!("{MAGIC} {DCS0} {START} {DCS0} {END}"))),
        "no ticks per quarter note",
    );
    refused(rd(&hex(MAGIC)), "no ticks per quarter note");
    refused(
        rd(&hex(&format!(
            "{MAGIC} {DCS0} 00300000 {DCS0} {START} {DCS0} {END}"
        ))),
        "ticks per quarter note of 0",
    );
    // No Start of Clip; End of Clip before it.
    refused(
        rd(&hex(&format!("{MAGIC} {DCS0} 003003C0"))),
        "no Start of Clip",
    );
    refused(
        rd(&hex(&format!("{MAGIC} {DCS0} 003003C0 {DCS0} {END}"))),
        "no Start of Clip",
    );
    // A packet cut off at the end, in the header and in the sequence.
    refused(
        rd(&hex(&format!("{MAGIC} {DCS0} 003003C0 {DCS0} D0100000"))),
        "ends in the middle of a message",
    );
    refused(
        rd(&hex(&format!(
            "{MAGIC} {DCS0} 003003C0 {DCS0} {START} {DCS0} 40903C00"
        ))),
        "ends in the middle of a message",
    );
    refused(
        rd(&hex(&format!(
            "{MAGIC} {DCS0} 003003C0 {DCS0} {START} 0040"
        ))),
        "ends in the middle of a message",
    );
    // A second Start of Clip.
    refused(
        rd(&clip_hex(&format!("{DCS0} {START}"))),
        "second Start of Clip",
    );

    // Data after End of Clip, even a partial packet, is ignored.
    let mut b = clip_hex(&note_hex());
    b.extend(hex("40903C00 80000000 1234"));
    let (c, report) = rd(&b).unwrap();
    assert_eq!(c.events, vec![(0, ClipEvent::Packet(note_on()))]);
    has(
        &report,
        ItemKind::Changed,
        "Data after the end of the clip was ignored",
    );

    // No End of Clip: the clip ends at its last event.
    let b = hex(&format!(
        "{MAGIC} {DCS0} 003003C0 {DCS0} {START} 00400020 40903C00 80000000 00400100"
    ));
    let (c, report) = rd(&b).unwrap();
    assert_eq!(c.events, vec![(32, ClipEvent::Packet(note_on()))]);
    assert_eq!(c.end, 32);
    has(&report, ItemKind::Changed, "no End of Clip");

    // Reserved message types are skipped and counted, whatever their size.
    let unknown = [
        "60000000",
        "70000000",
        "80000000 00000000",
        "90000000 00000000",
        "A0000000 00000000",
        "B0000000 00000000 00000000",
        "C0000000 00000000 00000000",
        "E0000000 00000000 00000000 00000000",
    ]
    .map(|p| format!("{DCS0} {p}"))
    .join(" ");
    let (c, report) = rd(&clip_hex(&format!("{unknown} {}", note_hex()))).unwrap();
    assert_eq!(c.events, vec![(0, ClipEvent::Packet(note_on()))]);
    assert_eq!(has(&report, ItemKind::LeftOut, "Unknown MIDI messages"), 8);

    // Profile settings before DCTPQ are not used.
    let b = hex(&format!(
        "{MAGIC} 30167E7F 0D220000 30320102 00000000 {DCS0} 003003C0 {DCS0} {START} {DCS0} {END}"
    ));
    let (c, report) = rd(&b).unwrap();
    assert!(c.events.is_empty());
    has(
        &report,
        ItemKind::Note,
        "Profile settings in the clip were not used",
    );
}

#[test]
fn limits_apply_at_and_past_the_limit() {
    let lim = Limits {
        max_events: 100,
        max_sysex_bytes: 16,
        max_text_bytes: 8,
        ..Limits::default()
    };
    // 100 timed packets read, 101 are refused.
    let notes = |n: usize| clip_hex(&vec![note_hex(); n].join(" "));
    let (c, report) = rd_with(&notes(100), &lim).unwrap();
    assert_eq!(c.events.len(), 100);
    assert!(report.is_empty(), "{report:?}");
    refused(rd_with(&notes(101), &lim), "more than 100 events");

    // A SysEx 7 of 16 bytes reads; 17 is left out.
    let sysex = |n: u8| {
        let data: Vec<u8> = (0..n).collect();
        let c = ClipFile {
            tpq: 960,
            header: vec![],
            events: vec![(0, ClipEvent::SysEx7 { group: 2, data })],
            end: 0,
        };
        write(&c)
    };
    let (c, report) = rd_with(&sysex(16), &lim).unwrap();
    assert_eq!(
        c.events,
        vec![(
            0,
            ClipEvent::SysEx7 {
                group: 2,
                data: (0..16).collect()
            }
        )]
    );
    assert!(report.is_empty(), "{report:?}");
    let (c, report) = rd_with(&sysex(17), &lim).unwrap();
    assert!(c.events.is_empty());
    has(&report, ItemKind::LeftOut, "SysEx messages over");

    // The same for SysEx 8.
    let sysex8 = |n: u8| {
        let data: Vec<u8> = (0..n).collect();
        let c = ClipFile {
            tpq: 960,
            header: vec![],
            events: vec![(0, ClipEvent::SysEx8 { group: 1, data })],
            end: 0,
        };
        write(&c)
    };
    let (c, _) = rd_with(&sysex8(16), &lim).unwrap();
    assert_eq!(c.events.len(), 1);
    let (c, report) = rd_with(&sysex8(17), &lim).unwrap();
    assert!(c.events.is_empty());
    has(&report, ItemKind::LeftOut, "SysEx messages over");

    // A SysEx 7 start with no end is left out.
    let (c, report) = rd_with(&clip_hex(&format!("{DCS0} 30160102 03040506")), &lim).unwrap();
    assert!(c.events.is_empty());
    has(&report, ItemKind::LeftOut, "Unfinished SysEx messages");
    // A continue with no start is left out too.
    let (c, report) = rd_with(&clip_hex(&format!("{DCS0} 30320102 00000000")), &lim).unwrap();
    assert!(c.events.is_empty());
    has(&report, ItemKind::LeftOut, "Unfinished SysEx messages");
    // A SysEx 7 packet with more than 6 bytes is damaged.
    let (c, report) = rd_with(&clip_hex(&format!("{DCS0} 30070102 03040506")), &lim).unwrap();
    assert!(c.events.is_empty());
    has(&report, ItemKind::LeftOut, "Damaged SysEx messages");
    // A SysEx 8 aborted by its end packet is left out.
    let b = clip_hex(&format!(
        "{DCS0} 501E0001 02030405 06070809 0A0B0C0D {DCS0} 503F0000 00000000 00000000 00000000"
    ));
    let (c, report) = rd_with(&b, &lim).unwrap();
    assert!(c.events.is_empty());
    has(&report, ItemKind::LeftOut, "Unfinished SysEx messages");

    // A text of 8 bytes is kept; 9 is cut.
    let text = |s: &str| {
        let c = ClipFile {
            tpq: 960,
            header: vec![],
            events: vec![(
                0,
                ClipEvent::Text {
                    group: 0,
                    bank: 2,
                    status: 1,
                    text: s.to_string(),
                },
            )],
            end: 0,
        };
        write(&c)
    };
    let (c, report) = rd_with(&text("12345678"), &lim).unwrap();
    assert!(report.is_empty(), "{report:?}");
    assert!(matches!(&c.events[0].1, ClipEvent::Text { text, .. } if text == "12345678"));
    let (c, report) = rd_with(&text("123456789"), &lim).unwrap();
    assert!(matches!(&c.events[0].1, ClipEvent::Text { text, .. } if text == "12345678"));
    has(&report, ItemKind::Changed, "Texts over");
}

#[test]
fn packets_read_and_write() {
    let p = [ump::noop(), ump::dcs(5), note_on(), ump::start_of_clip()];
    let mut out = Vec::new();
    ump::write_packets(&p, &mut out);
    assert_eq!(out.len(), 4 * (1 + 1 + 2 + 4));
    assert_eq!(ump::read_packets(&out).unwrap(), p.to_vec());
    out.pop();
    assert!(matches!(ump::read_packets(&out), Err(IoError::Refused(_))));
    assert_eq!(ump::read_packets(&[]).unwrap(), vec![]);
    // Sizes by message type (M2-104-UM Table 4).
    let sizes: Vec<usize> = (0u8..16).map(ump::packet_words).collect();
    assert_eq!(sizes, vec![1, 1, 1, 2, 2, 4, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4]);
    assert!(Packet::new(&[0x4090_3C00]).is_none());
    assert!(Packet::new(&[]).is_none());
    assert_eq!(Packet::new(&[0x2390_3C64]).unwrap().group(), 3);
    assert_eq!(
        ump::decode(&Packet::new(&[0x6000_0000]).unwrap()),
        Decoded::Unknown
    );
}

#[test]
fn key_signature_tonics() {
    // C major, A minor, G major, E minor, F major, D minor, unknown.
    let cases = [
        (0, false, 3),
        (0, true, 1),
        (1, false, 7),
        (1, true, 5),
        (-1, false, 6),
        (-1, true, 4),
    ];
    for (sharps, minor, tonic) in cases {
        let p = ump::key_sig(sharps, minor);
        assert_eq!(ump::decode(&p), Decoded::KeySignature { sharps, tonic });
        assert_eq!(ump::is_minor(sharps, tonic), minor);
    }
    assert_eq!(
        bytes_of(&ump::key_sig(-3, false)),
        hex("D0100005 D5000000 00000000 00000000")
    );
}

fn group() -> impl Strategy<Value = u8> {
    0u8..16
}
fn ch() -> impl Strategy<Value = u8> {
    0u8..16
}
fn b7() -> impl Strategy<Value = u8> {
    0u8..128
}
fn attr() -> impl Strategy<Value = Option<NoteAttribute>> {
    prop::option::of(
        (1u8..=255, any::<u16>()).prop_map(|(kind, data)| NoteAttribute { kind, data }),
    )
}

proptest! {
    #[test]
    fn ump_round_trips(
        g in group(), c in ch(), n in b7(), i in b7(), v16 in any::<u16>(), v in any::<u32>(),
        a in attr(), bank in prop::option::of((b7(), b7())), st in 0x80u8..0xF0, d2 in b7(),
        ticks in 0u32..0x10_0000, tpq in any::<u16>(), num in any::<u8>(), den in any::<u8>(),
        n32 in any::<u8>(), sharps in -7i8..=7, minor in any::<bool>(),
        sx7 in prop::collection::vec(b7(), 0..=6), sx8 in prop::collection::vec(any::<u8>(), 0..=13),
        stream in any::<u8>(),
    ) {
        use Decoded as D;
        let cases = vec![
            (ump::noop(), D::Noop),
            (ump::dcs(ticks), D::Dcs(ticks)),
            (ump::dctpq(tpq), D::Dctpq(tpq)),
            (ump::midi1(g, st, n, d2), D::Midi1 { group: g, status: st, data1: n, data2: d2 }),
            (ump::note_on(g, c, n, v16, a), D::NoteOn { group: g, channel: c, note: n, velocity: v16, attribute: a }),
            (ump::note_off(g, c, n, v16, a), D::NoteOff { group: g, channel: c, note: n, velocity: v16, attribute: a }),
            (ump::poly_pressure(g, c, n, v), D::PolyPressure { group: g, channel: c, note: n, value: v }),
            (ump::control_change(g, c, i, v), D::ControlChange { group: g, channel: c, index: i, value: v }),
            (ump::program_change(g, c, n, bank), D::ProgramChange { group: g, channel: c, program: n, bank }),
            (ump::channel_pressure(g, c, v), D::ChannelPressure { group: g, channel: c, value: v }),
            (ump::pitch_bend(g, c, v), D::PitchBend { group: g, channel: c, value: v }),
            (ump::per_note_registered(g, c, n, i, v), D::PerNoteRegistered { group: g, channel: c, note: n, index: i, value: v }),
            (ump::per_note_assignable(g, c, n, i, v), D::PerNoteAssignable { group: g, channel: c, note: n, index: i, value: v }),
            (ump::per_note_pitch_bend(g, c, n, v), D::PerNotePitchBend { group: g, channel: c, note: n, value: v }),
            (ump::set_tempo(v), D::SetTempo(v)),
            (ump::time_sig(num, den, n32), D::TimeSignature { numerator: num, denominator: den, thirty_seconds: n32 }),
            (ump::key_sig(sharps, minor), D::KeySignature { sharps, tonic: ump::tonic(sharps, minor) }),
            (ump::start_of_clip(), D::StartOfClip),
            (ump::end_of_clip(), D::EndOfClip),
        ];
        for (p, want) in cases {
            prop_assert_eq!(ump::decode(&p), want);
            prop_assert_eq!(Packet::new(p.words()), Some(p));
        }
        prop_assert_eq!(ump::is_minor(sharps, ump::tonic(sharps, minor)), minor);
        // Single-packet SysEx and text decode to their bytes.
        let s7 = ump::sysex7(g, &sx7);
        prop_assert_eq!(s7.len(), 1);
        prop_assert_eq!(ump::decode(&s7[0]), D::SysEx7 { group: g, form: 0, data: sx7.clone() });
        let s8 = ump::sysex8(g, stream, &sx8);
        prop_assert_eq!(s8.len(), 1);
        prop_assert_eq!(ump::decode(&s8[0]), D::SysEx8 { group: g, form: 0, stream, data: sx8.clone() });
        let t = ump::text(1, 2, g, "Song");
        let mut data = [0u8; 12];
        data[..4].copy_from_slice(b"Song");
        prop_assert_eq!(ump::decode(&t[0]), D::Text { group: g, form: 0, bank: 1, status: 2, data });
    }
}
