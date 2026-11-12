//! Hostile MIDI Clip Files never crash the reader (plan 6a Task 4): byte
//! damage to every fixture in `tests/data/*.midi2` and a "UMP soup" of
//! random packets either read or are refused, quickly, and whatever reads
//! writes and reads back equal.

use core_io::IoError;
use core_io::clip::{read, write};
use core_io::limits::Limits;
use proptest::prelude::*;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// Cases per property: `VIZDAW_FUZZ_CASES` (a long local run), else `n`.
fn cases(n: u32) -> u32 {
    std::env::var("VIZDAW_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(n)
}

/// Every `.midi2` fixture, sorted by name.
fn fixtures() -> Vec<Vec<u8>> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data");
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "midi2"))
        .collect();
    paths.sort();
    paths.iter().map(|p| std::fs::read(p).unwrap()).collect()
}

fn limits() -> Limits {
    Limits {
        max_events: 100_000,
        ..Limits::default()
    }
}

/// Reads `bytes`: `Ok` or `Refused` within 2 s, and an `Ok` result writes
/// and reads back equal.
fn check(bytes: &[u8]) -> Result<(), TestCaseError> {
    let cancel = AtomicBool::new(false);
    let t = Instant::now();
    let r = read(bytes, &limits(), &cancel);
    prop_assert!(t.elapsed() < Duration::from_secs(2), "read took too long");
    match r {
        Ok((c, _)) => {
            let (back, _) = read(&write(&c), &limits(), &cancel)
                .map_err(|e| TestCaseError::fail(format!("read back: {e}")))?;
            prop_assert_eq!(back, c);
        }
        Err(IoError::Refused(_)) => {}
        Err(e) => prop_assert!(false, "unexpected error {e:?}"),
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum Damage {
    Flip(usize, u8),
    Insert(usize, u8),
    Delete(usize),
    Truncate(usize),
}

fn damage() -> impl Strategy<Value = Damage> {
    prop_oneof![
        (any::<usize>(), 0u8..8).prop_map(|(i, b)| Damage::Flip(i, b)),
        (any::<usize>(), any::<u8>()).prop_map(|(i, b)| Damage::Insert(i, b)),
        any::<usize>().prop_map(Damage::Delete),
        any::<usize>().prop_map(Damage::Truncate),
    ]
}

fn apply(bytes: &mut Vec<u8>, d: &Damage) {
    let n = bytes.len();
    match *d {
        Damage::Flip(i, b) if n > 0 => bytes[i % n] ^= 1 << b,
        Damage::Insert(i, b) => bytes.insert(i % (n + 1), b),
        Damage::Delete(i) if n > 0 => {
            bytes.remove(i % n);
        }
        Damage::Truncate(i) => bytes.truncate(i % (n + 1)),
        _ => {}
    }
}

/// Word 0 of a packet: a message type and status bits that make parsing
/// interesting (clip markers, DCS, SysEx and Flex Data forms), or anything.
fn word0() -> impl Strategy<Value = u32> {
    let known = prop::sample::select(vec![
        0x0000_0000u32,
        0x0040_0000,
        0x004F_FFFF,
        0x0030_03C0,
        0x0030_0000,
        0xF020_0000,
        0xF021_0000,
        0x2090_3C64,
        0x4090_3C00,
        0x3000_0000,
        0x3010_0000,
        0x3020_0000,
        0x3030_0000,
        0x5000_0000,
        0x5010_0000,
        0x5020_0000,
        0x5030_0000,
        0xD010_0000,
        0xD010_0103,
        0xD050_0201,
        0xD090_0201,
        0xD0D0_0201,
    ]);
    // Keep the type and status, randomise the low bits now and then.
    let mixed = (known.clone(), any::<u32>(), 0u32..4).prop_map(|(k, r, how)| match how {
        0 => k,
        1 => (k & 0xFFF0_0000) | (r & 0x000F_FFFF),
        2 => (k & 0xFF00_0000) | (r & 0x00FF_FFFF),
        _ => (k & 0xF000_0000) | (r & 0x0FFF_FFFF),
    });
    prop_oneof![4 => mixed, 1 => any::<u32>()]
}

/// One packet: word 0 and as many random words as its type takes, or
/// (now and then) one word too few or too many.
fn soup_packet() -> impl Strategy<Value = Vec<u8>> {
    (word0(), prop::collection::vec(any::<u32>(), 3), 0u8..12).prop_map(|(w0, rest, size)| {
        let n = core_io::ump::packet_words((w0 >> 28) as u8);
        let n = match size {
            0 => n.saturating_sub(1),
            1 => n + 1,
            _ => n,
        };
        let mut words = vec![w0];
        words.extend(rest.into_iter().cycle().take(n.saturating_sub(1)));
        words.truncate(n);
        words.iter().flat_map(|w| w.to_be_bytes()).collect()
    })
}

/// A plausible header (magic, DCS 0, DCTPQ, DCS 0, Start of Clip; parts
/// dropped now and then) and packet soup.
fn soup() -> impl Strategy<Value = Vec<u8>> {
    (
        prop::collection::vec(any::<bool>(), 5),
        prop::collection::vec(soup_packet(), 0..48),
    )
        .prop_map(|(keep, packets)| {
            let parts: [&[u8]; 5] = [
                b"SMF2CLIP",
                &[0x00, 0x40, 0x00, 0x00, 0x00, 0x30, 0x03, 0xC0],
                &[0x00, 0x40, 0x00, 0x00],
                &[0xF0, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                &[0x00, 0x40, 0x00, 0x01],
            ];
            let mut v = Vec::new();
            for (i, p) in parts.iter().enumerate() {
                // The magic and DCTPQ are always there; the rest half the time.
                if keep[i] || i < 2 {
                    v.extend_from_slice(p);
                }
            }
            v.extend(packets.concat());
            v
        })
}

#[test]
fn fixtures_read_cleanly() {
    let all = fixtures();
    assert!(all.len() >= 2, "fixtures are missing");
    let cancel = AtomicBool::new(false);
    for bytes in all {
        let (c, report) = read(&bytes, &limits(), &cancel).unwrap();
        assert!(report.is_empty(), "{report:?}");
        assert_eq!(write(&c), bytes);
    }
}

#[test]
fn every_kind_reads_as_written() {
    use core_io::clip::ClipEvent;
    let bytes = include_bytes!("data/every_kind.midi2");
    let (c, _) = read(bytes, &limits(), &AtomicBool::new(false)).unwrap();
    assert_eq!((c.tpq, c.header.len(), c.end), (960, 2, 960 + 0x10_0000));
    let texts: Vec<&str> = c
        .events
        .iter()
        .filter_map(|(_, e)| match e {
            ClipEvent::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["Song", "Bass", "Hello, VizDAW!!"]);
    let sysex: Vec<&ClipEvent> = c
        .events
        .iter()
        .map(|(_, e)| e)
        .filter(|e| matches!(e, ClipEvent::SysEx7 { .. } | ClipEvent::SysEx8 { .. }))
        .collect();
    assert_eq!(
        sysex,
        vec![
            &ClipEvent::SysEx7 {
                group: 0,
                data: vec![0x7E, 0x7F, 0x09, 0x01]
            },
            &ClipEvent::SysEx7 {
                group: 0,
                data: vec![0x43, 0x10, 0x4C, 0, 0, 0x7E, 1, 2, 3, 4]
            },
            &ClipEvent::SysEx8 {
                group: 0,
                data: vec![0x7E, 0x7F, 0x0D, 0x70]
            },
        ]
    );
    assert_eq!(c.events.len(), 20);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn clip_fuzz_never_panics(
        which in any::<usize>(),
        damages in prop::collection::vec(damage(), 1..8),
        soup in soup(),
    ) {
        let all = fixtures();
        let mut bytes = all[which % all.len()].clone();
        for d in &damages {
            apply(&mut bytes, d);
        }
        check(&bytes)?;
        check(&soup)?;
    }
}
