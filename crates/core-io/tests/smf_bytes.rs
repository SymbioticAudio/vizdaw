//! Standard MIDI File bytes: the golden, running status, SysEx packets,
//! unknown chunks and hostile files (plan 6a Task 3).

use core_engine::midi1::Midi1Msg;
use core_io::IoError;
use core_io::limits::{Limits, MAX_EVENTS};
use core_io::report::{ItemKind, Report};
use core_io::smf::{Division, SmfEvent, SmfFile, SmfTrack, read, write};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// Bytes from hex digits; spaces are ignored.
fn hex(s: &str) -> Vec<u8> {
    let d: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    d.chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}

fn mthd(format: u16, ntrks: u16, division: u16) -> Vec<u8> {
    let mut v = b"MThd\0\0\0\x06".to_vec();
    for x in [format, ntrks, division] {
        v.extend_from_slice(&x.to_be_bytes());
    }
    v
}

fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = kind.to_vec();
    v.extend_from_slice(&(body.len() as u32).to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn mtrk(body: &[u8]) -> Vec<u8> {
    chunk(b"MTrk", body)
}

/// A type 1 file at 960 PPQ with one track per body.
fn file_of(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut v = mthd(1, bodies.len() as u16, 960);
    for b in bodies {
        v.extend(mtrk(b));
    }
    v
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

fn rd(bytes: &[u8]) -> Result<(SmfFile, Report), IoError> {
    read(bytes, &Limits::default(), &no_cancel())
}

fn rd_with(bytes: &[u8], limits: &Limits) -> Result<(SmfFile, Report), IoError> {
    read(bytes, limits, &no_cancel())
}

#[track_caller]
fn refused(r: Result<(SmfFile, Report), IoError>, needle: &str) {
    match r {
        Err(IoError::Refused(m)) => assert!(m.contains(needle), "{m:?} lacks {needle:?}"),
        other => panic!("expected a refusal containing {needle:?}, got {other:?}"),
    }
}

#[track_caller]
fn has(r: &Report, kind: ItemKind, needle: &str) {
    assert!(
        r.items
            .iter()
            .any(|i| i.kind == kind && i.what.contains(needle)),
        "no {kind:?} item containing {needle:?} in {:?}",
        r.items
    );
}

fn meta(kind: u8, data: &[u8]) -> SmfEvent {
    SmfEvent::Meta {
        kind,
        data: data.to_vec(),
    }
}

fn three(s: u8, a: u8, b: u8) -> SmfEvent {
    SmfEvent::Channel(Midi1Msg::three(s, a, b))
}

fn eot() -> SmfEvent {
    meta(0x2F, &[])
}

/// The events of a single-track file with `body`.
fn events(body: &str) -> Vec<(u64, SmfEvent)> {
    let (f, _) = rd(&file_of(&[hex(body)])).unwrap();
    f.tracks.into_iter().next().unwrap().events
}

fn golden_file() -> SmfFile {
    SmfFile {
        format: 1,
        division: Division::Ppq(960),
        tracks: vec![
            SmfTrack {
                events: vec![
                    (0, meta(0x03, b"Song")),
                    (0, meta(0x51, &[0x07, 0xA1, 0x20])),
                    (0, meta(0x58, &[0x04, 0x02, 0x18, 0x08])),
                    (0, meta(0x59, &[0x00, 0x00])),
                    (0, eot()),
                ],
            },
            SmfTrack {
                events: vec![
                    (0, meta(0x03, b"Bass")),
                    (0, three(0x90, 0x3C, 0x64)),
                    (960, three(0x80, 0x3C, 0x00)),
                    (960, eot()),
                ],
            },
        ],
    }
}

const GOLDEN: &str = "4D546864 00000006 0001 0002 03C0
    4D54726B 00000021 00FF0304536F6E67 00FF510307A120 00FF580404021808 00FF59020000 00FF2F00
    4D54726B 00000015 00FF030442617373 00903C64 8740803C00 00FF2F00";

#[test]
fn reads_and_writes_the_golden() {
    let bytes = write(&golden_file()).unwrap();
    assert_eq!(bytes, hex(GOLDEN));
    assert_eq!(bytes, include_bytes!("data/golden_type1.mid").to_vec());
    let (f, report) = rd(&bytes).unwrap();
    assert_eq!(f, golden_file());
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn running_status_and_note_on_zero() {
    assert_eq!(
        events("00 90 3C 64 00 3E 64 00 3C 00 00 FF 2F 00"),
        vec![
            (0, three(0x90, 0x3C, 0x64)),
            (0, three(0x90, 0x3E, 0x64)),
            (0, three(0x90, 0x3C, 0x00)),
            (0, eot()),
        ]
    );
    // Two-byte messages run too.
    assert_eq!(
        events("00 C0 05 10 06 00 FF 2F 00"),
        vec![
            (0, SmfEvent::Channel(Midi1Msg::two(0xC0, 5))),
            (16, SmfEvent::Channel(Midi1Msg::two(0xC0, 6))),
            (16, eot()),
        ]
    );
    // A meta event, a SysEx or an escape between cancels running status.
    for between in ["FF 01 00", "F0 01 F7", "F7 00"] {
        let body = hex(&format!("00 90 3C 64 00 {between} 00 3C 64 00 FF 2F 00"));
        refused(rd(&file_of(&[body])), "track 1");
    }
}

#[test]
fn sysex_packets_reassemble() {
    assert_eq!(
        events("00 F0 03 01 02 03 10 F7 02 04 F7 00 FF 2F 00"),
        vec![(0, SmfEvent::SysEx(vec![1, 2, 3, 4])), (16, eot())]
    );
    // Other events may sit between the packets; the SysEx keeps its start.
    assert_eq!(
        events("00 F0 02 01 02 05 90 3C 64 00 F7 02 03 F7 00 FF 2F 00"),
        vec![
            (0, SmfEvent::SysEx(vec![1, 2, 3])),
            (5, three(0x90, 0x3C, 0x64)),
            (5, eot()),
        ]
    );
    assert_eq!(
        events("00 F7 02 90 3C 00 FF 2F 00"),
        vec![(0, SmfEvent::Escape(vec![0x90, 0x3C])), (0, eot())]
    );
    assert_eq!(
        events("00 F0 03 7E 7F F7 00 FF 2F 00"),
        vec![(0, SmfEvent::SysEx(vec![0x7E, 0x7F])), (0, eot())]
    );
}

#[test]
fn unknown_chunks_are_skipped() {
    let t = hex("00 90 3C 64 00 FF 2F 00");
    let mut bytes = mthd(1, 2, 960);
    bytes.extend(mtrk(&t));
    bytes.extend(chunk(b"XFIH", &[1, 2, 3, 4]));
    bytes.extend(mtrk(&t));
    let (f, r) = rd(&bytes).unwrap();
    assert_eq!(f.tracks.len(), 2);
    has(&r, ItemKind::Note, "Unknown file sections skipped");
}

#[test]
fn hostile_smf_is_refused_or_reported() {
    let ok = hex("00 90 3C 64 00 FF 2F 00");
    let not_midi = "This isn't a MIDI file.";

    // Headers.
    refused(rd(&mthd(1, 1, 960)[..13]), not_midi);
    refused(rd(b""), not_midi);
    let mut magic = file_of(std::slice::from_ref(&ok));
    magic[0] = b'X';
    refused(rd(&magic), not_midi);
    let mut short = b"MThd\0\0\0\x05\0\x01\0\x01\x03\xC0\0".to_vec();
    short.extend(mtrk(&ok));
    refused(rd(&short), not_midi);
    let mut huge = b"MThd\xFF\xFF\xFF\xFF\0\x01\0\x01\x03\xC0".to_vec();
    huge.extend(mtrk(&ok));
    refused(rd(&huge), not_midi);
    // A longer header is read, its extra bytes skipped.
    let mut long = b"MThd\0\0\0\x08\0\x01\0\x01\x03\xC0\xAA\xBB".to_vec();
    long.extend(mtrk(&ok));
    assert_eq!(rd(&long).unwrap().0.tracks.len(), 1);

    let mut t2 = mthd(2, 1, 960);
    t2.extend(mtrk(&ok));
    refused(
        rd(&t2),
        "This is a type 2 MIDI file; VizDAW reads types 0 and 1.",
    );
    let mut t3 = mthd(3, 1, 960);
    t3.extend(mtrk(&ok));
    refused(rd(&t3), not_midi);

    // A chunk length past the end names the 1-based track.
    let mut past = file_of(&[ok.clone(), ok.clone()]);
    past.truncate(past.len() - 2);
    refused(
        rd(&past),
        "This MIDI file can't be read: track 2 ends in the middle of an event.",
    );

    // Damage inside a chunk.
    refused(
        rd(&file_of(&[hex("00 90 3C")])),
        "track 1 ends in the middle",
    );
    refused(
        rd(&file_of(&[ok.clone(), hex("00 FF 03 05 41")])),
        "track 2",
    );
    refused(rd(&file_of(&[hex("FF FF FF FF 7F 90 3C 64")])), "track 1");
    refused(rd(&file_of(&[hex("00 3C 64 00 FF 2F 00")])), "track 1");
    refused(rd(&file_of(&[hex("00 F1 00 00 FF 2F 00")])), "track 1");
    refused(rd(&file_of(&[hex("00 90 3C 90 00 FF 2F 00")])), "track 1");
    assert_eq!(
        events("FF FF FF 7F 90 3C 64 00 FF 2F 00"),
        vec![(0x0FFF_FFFF, three(0x90, 0x3C, 0x64)), (0x0FFF_FFFF, eot())]
    );

    // A SysEx declaring 2^30 bytes (a 5-byte length) or the most a 4-byte
    // length holds (2^28 - 1): refused without allocating them.
    let t = Instant::now();
    refused(
        rd(&file_of(&[hex("00 F0 84 80 80 80 00 01 02 03")])),
        "track 1",
    );
    refused(
        rd(&file_of(&[hex("00 F0 FF FF FF 7F 01 02 03")])),
        "track 1 ends in the middle of an event.",
    );
    refused(
        rd(&file_of(&[hex("00 FF 01 FF FF FF 7F 41")])),
        "track 1 ends in the middle of an event.",
    );
    assert!(t.elapsed() < Duration::from_secs(1));

    // Limits, at the limit and one past it.
    let small = Limits {
        max_sysex_bytes: 16,
        max_tracks: 8,
        max_events: 100,
        ..Limits::default()
    };
    let sysex = |n: usize| {
        let mut b = vec![0x00, 0xF0, (n + 1) as u8];
        b.extend(std::iter::repeat_n(0x11, n));
        b.extend([0xF7, 0x00, 0xFF, 0x2F, 0x00]);
        file_of(&[b])
    };
    let (f, r) = rd_with(&sysex(16), &small).unwrap();
    assert_eq!(f.tracks[0].events[0], (0, SmfEvent::SysEx(vec![0x11; 16])));
    assert!(r.is_empty(), "{r:?}");
    let (f, r) = rd_with(&sysex(17), &small).unwrap();
    assert_eq!(f.tracks[0].events, vec![(0, eot())]);
    has(&r, ItemKind::LeftOut, "SysEx messages over 1 MiB");
    // Over the limit across packets too.
    let split = file_of(&[hex(&format!(
        "00 F0 08 {} 00 F7 0A {} F7 00 FF 2F 00",
        "11 ".repeat(8),
        "11 ".repeat(9)
    ))]);
    let (f, r) = rd_with(&split, &small).unwrap();
    assert_eq!(f.tracks[0].events, vec![(0, eot())]);
    has(&r, ItemKind::LeftOut, "SysEx messages over 1 MiB");

    let tracks = |n: usize| file_of(&vec![ok.clone(); n]);
    assert_eq!(rd_with(&tracks(8), &small).unwrap().0.tracks.len(), 8);
    refused(rd_with(&tracks(9), &small), "more than 8 tracks.");

    let notes = |n: usize| {
        let mut b = hex("00 90 3C 64");
        for _ in 1..n - 1 {
            b.extend(hex("00 3C 64"));
        }
        b.extend(hex("00 FF 2F 00"));
        file_of(&[b])
    };
    let (f, _) = rd_with(&notes(100), &small).unwrap();
    assert_eq!(f.tracks[0].events.len(), 100);
    refused(rd_with(&notes(101), &small), "more than 100 events.");
    // Events count across tracks.
    let half = {
        let mut b = hex("00 90 3C 64");
        for _ in 1..50 {
            b.extend(hex("00 3C 64"));
        }
        b
    };
    assert!(rd_with(&file_of(&[half.clone(), half.clone()]), &small).is_ok());
    let mut more = half.clone();
    more.extend(hex("00 3C 64"));
    refused(
        rd_with(&file_of(&[half.clone(), more]), &small),
        "more than 100 events.",
    );

    // Texts are cut at the text limit.
    let tiny_text = Limits {
        max_text_bytes: 8,
        ..Limits::default()
    };
    let text = |s: &str| {
        let mut b = vec![0x00, 0xFF, 0x01, s.len() as u8];
        b.extend(s.bytes());
        b.extend(hex("00 FF 2F 00"));
        file_of(&[b])
    };
    let (f, r) = rd_with(&text("12345678"), &tiny_text).unwrap();
    assert_eq!(f.tracks[0].events[0], (0, meta(0x01, b"12345678")));
    assert!(r.is_empty(), "{r:?}");
    let (f, r) = rd_with(&text("123456789"), &tiny_text).unwrap();
    assert_eq!(f.tracks[0].events[0], (0, meta(0x01, b"12345678")));
    has(&r, ItemKind::Changed, "Texts over 64 KiB cut");

    // An unfinished SysEx: at the end of the track, or cut off by a new one.
    for body in [
        "00 F0 02 01 02 00 FF 2F 00",
        "00 F0 02 01 02 00 F7 01 03 00 FF 2F 00",
        "00 F0 02 01 02",
    ] {
        let (f, r) = rd(&file_of(&[hex(body)])).unwrap();
        assert!(
            !f.tracks[0]
                .events
                .iter()
                .any(|(_, e)| matches!(e, SmfEvent::SysEx(_))),
            "{body}"
        );
        has(&r, ItemKind::LeftOut, "Unfinished SysEx messages");
    }
    let (f, r) = rd(&file_of(&[hex(
        "00 F0 02 01 02 00 F0 02 03 F7 00 FF 2F 00",
    )]))
    .unwrap();
    assert_eq!(
        f.tracks[0].events,
        vec![(0, SmfEvent::SysEx(vec![3])), (0, eot())]
    );
    has(&r, ItemKind::LeftOut, "Unfinished SysEx messages");

    // Structure that reads, with a report.
    let mut type0 = mthd(0, 2, 960);
    type0.extend(mtrk(&ok));
    type0.extend(mtrk(&ok));
    let (f, r) = rd(&type0).unwrap();
    assert_eq!((f.format, f.tracks.len()), (0, 2));
    has(&r, ItemKind::Changed, "Type 0 file with 2 tracks");

    let mut declares = mthd(1, 5, 960);
    declares.extend(mtrk(&ok));
    declares.extend(mtrk(&ok));
    let (f, r) = rd(&declares).unwrap();
    assert_eq!(f.tracks.len(), 2);
    has(
        &r,
        ItemKind::Changed,
        "The file declares 5 tracks and holds 2",
    );

    let (f, r) = rd(&file_of(&[hex("00 90 3C 64")])).unwrap();
    assert_eq!(f.tracks[0].events, vec![(0, three(0x90, 0x3C, 0x64))]);
    has(&r, ItemKind::Note, "End of Track");

    let (f, r) = rd(&file_of(&[hex("00 FF 2F 00 00 90 3C 64")])).unwrap();
    assert_eq!(f.tracks[0].events, vec![(0, eot())]);
    has(&r, ItemKind::Note, "after the End of Track");
}

#[test]
#[ignore = "builds a 30 MB file of 10 000 001 events; run by hand"]
fn ten_million_events_are_refused_at_the_real_limit() {
    let mut b = hex("00 90 3C 64");
    b.reserve(MAX_EVENTS * 3);
    for _ in 0..MAX_EVENTS {
        b.extend_from_slice(&[0x00, 0x3C, 0x64]);
    }
    refused(rd(&file_of(&[b])), "more than 10 000 000 events.");
}

#[test]
fn zero_division_is_refused() {
    let timing = "This MIDI file's timing can't be read.";
    let ok = hex("00 90 3C 64 00 FF 2F 00");
    let with = |division: u16| {
        let mut v = mthd(1, 1, division);
        v.extend(mtrk(&ok));
        v
    };
    refused(rd(&with(0)), timing);
    // SMPTE: the high byte is minus the frame rate.
    refused(rd(&with(0xE800)), timing); // -24 fps, 0 ticks per frame
    refused(rd(&with(0xEA28)), timing); // -22 fps
    refused(rd(&with(0x8028)), timing); // -128 fps
    for (fps, hi) in [(24u8, 0xE8u16), (25, 0xE7), (29, 0xE3), (30, 0xE2)] {
        let (f, _) = rd(&with((hi << 8) | 40)).unwrap();
        assert_eq!(
            f.division,
            Division::Smpte {
                fps,
                ticks_per_frame: 40
            }
        );
        assert_eq!(write(&f).unwrap(), with((hi << 8) | 40));
    }
}

#[test]
fn cancel_stops_the_reader() {
    let bytes = hex(GOLDEN);
    assert_eq!(
        read(&bytes, &Limits::default(), &AtomicBool::new(true)),
        Err(IoError::Cancelled)
    );
}

#[test]
fn write_refuses_what_a_file_cannot_hold() {
    let one = |events: Vec<(u64, SmfEvent)>| SmfFile {
        format: 1,
        division: Division::Ppq(960),
        tracks: vec![SmfTrack { events }],
    };
    let r = write(&one(vec![(10, eot()), (9, eot())]));
    assert!(matches!(r, Err(IoError::Refused(_))), "{r:?}");
    let r = write(&one(vec![(0x1000_0000, eot())]));
    assert!(matches!(r, Err(IoError::Refused(_))), "{r:?}");
    let max = write(&one(vec![(0x0FFF_FFFF, eot())])).unwrap();
    assert!(max.ends_with(&hex("FF FF FF 7F FF 2F 00")), "{max:x?}");
    // Running status is never written; SysEx and escapes as one packet.
    let f = one(vec![
        (0, three(0x90, 0x3C, 0x64)),
        (0, three(0x90, 0x3E, 0x64)),
        (0, SmfEvent::SysEx(vec![1, 2])),
        (0, SmfEvent::Escape(vec![0xF8])),
        (0, eot()),
    ]);
    let bytes = write(&f).unwrap();
    assert!(
        bytes.ends_with(&hex(
            "00 90 3C 64 00 90 3E 64 00 F0 03 01 02 F7 00 F7 01 F8 00 FF 2F 00"
        )),
        "{bytes:x?}"
    );
    assert_eq!(rd(&bytes).unwrap().0, f);
}
