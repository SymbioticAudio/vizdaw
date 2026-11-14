//! Hostile Standard MIDI Files never crash the reader (plan 6a Task 3):
//! byte damage to every fixture in `tests/data/*.mid` and a "chunk soup"
//! of random chunks either read or are refused, quickly, and whatever reads
//! writes and reads back equal.

use core_io::IoError;
use core_io::limits::Limits;
use core_io::smf::{read, write};
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

/// Every `.mid` fixture, sorted by name.
fn fixtures() -> Vec<Vec<u8>> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data");
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "mid"))
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
        Ok((f, _)) => {
            let again = write(&f).map_err(|e| TestCaseError::fail(format!("write: {e}")))?;
            let (g, _) = read(&again, &limits(), &cancel)
                .map_err(|e| TestCaseError::fail(format!("read back: {e}")))?;
            prop_assert_eq!(g, f);
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

/// Bytes that make event parsing interesting: statuses, VLQ continuation
/// bytes, meta kinds and small values, mixed with anything.
fn event_byte() -> impl Strategy<Value = u8> {
    prop_oneof![
        3 => any::<u8>(),
        2 => prop::sample::select(vec![
            0x00, 0x01, 0x2F, 0x3C, 0x51, 0x7F, 0x80, 0x81, 0x90, 0xB0, 0xC0, 0xE0, 0xF0,
            0xF7, 0xFF,
        ]),
    ]
}

/// One chunk: a kind, a declared length (true or random) and a body.
fn soup_chunk() -> impl Strategy<Value = Vec<u8>> {
    let kind = prop_oneof![
        4 => Just(*b"MTrk"),
        1 => Just(*b"MThd"),
        1 => any::<[u8; 4]>(),
    ];
    let body = prop::collection::vec(event_byte(), 0..96);
    let honest = prop_oneof![4 => Just(None), 1 => any::<u32>().prop_map(Some)];
    (kind, body, honest).prop_map(|(kind, body, len)| {
        let mut v = kind.to_vec();
        let len = len.unwrap_or(body.len() as u32);
        v.extend_from_slice(&len.to_be_bytes());
        v.extend(body);
        v
    })
}

/// A plausible header (or, now and then, a random one) and chunk soup.
fn soup() -> impl Strategy<Value = Vec<u8>> {
    let format = prop_oneof![Just(0u16), Just(1), Just(2), any::<u16>()];
    let ntrks = prop_oneof![1u16..6, any::<u16>()];
    let division = prop_oneof![Just(96u16), Just(960), Just(0), Just(0xE828), any::<u16>()];
    (
        format,
        ntrks,
        division,
        prop::collection::vec(soup_chunk(), 0..8),
    )
        .prop_map(|(format, ntrks, division, chunks)| {
            let mut v = b"MThd\0\0\0\x06".to_vec();
            for x in [format, ntrks, division] {
                v.extend_from_slice(&x.to_be_bytes());
            }
            v.extend(chunks.concat());
            v
        })
}

#[test]
fn fixtures_read_cleanly() {
    let all = fixtures();
    assert!(all.len() >= 3, "fixtures are missing");
    let cancel = AtomicBool::new(false);
    for bytes in all {
        let (f, report) = read(&bytes, &limits(), &cancel).unwrap();
        assert!(report.is_empty(), "{report:?}");
        let (g, _) = read(&write(&f).unwrap(), &limits(), &cancel).unwrap();
        assert_eq!(g, f);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn smf_fuzz_never_panics(
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
