//! The package ZIP reader and writer (plan 6d Task 1): what the writer makes
//! reads back byte-equal and deterministic, and hostile archives (names that
//! leave the package, duplicates, links, too many entries, bombs, encrypted
//! or unknown methods, ZIP64 lies, damaged and random bytes) are refused with
//! bounded memory, never a panic.

mod common;

use common::rawzip_builder::{RawEntry, raw_zip, raw_zip_claiming};
use common::{CountingAlloc, cases, measure};
use core_io::IoError;
use core_io::limits::PackageLimits;
use core_io::zip::{EntryMethod, TOO_LARGE, ZipReader, ZipWriter, check_name};
use proptest::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const MIB: u64 = 1024 * 1024;

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

/// `bytes` written to `name` in `dir`.
fn put(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Opens `bytes` as a package with the default limits.
fn open_bytes(bytes: &[u8]) -> Result<ZipReader, IoError> {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "p.dawproject", bytes);
    ZipReader::open(&path, &PackageLimits::default())
}

/// The refusal's sentence, or a failure naming what came instead.
fn refusal<T>(r: Result<T, IoError>) -> String {
    match r {
        Err(IoError::Refused(s)) => s,
        Err(e) => panic!("expected a refusal, got {e:?}"),
        Ok(_) => panic!("expected a refusal, got Ok"),
    }
}

fn damaged(why: &str) -> String {
    format!("This DAWproject file is damaged: {why}.")
}

/// 100 KiB of XML-like text.
fn text_100k() -> Vec<u8> {
    let line = b"<Note time=\"1.5\" duration=\"0.25\" key=\"60\"/>\n";
    line.iter().copied().cycle().take(100 * 1024).collect()
}

fn sample_zip() -> Vec<u8> {
    let mut w = ZipWriter::new(Vec::new());
    w.add("a.txt", EntryMethod::Stored, b"hi").unwrap();
    w.add("dir/b.xml", EntryMethod::Deflated, &text_100k())
        .unwrap();
    w.add("audio/001.wav", EntryMethod::Stored, &[1, 2, 3])
        .unwrap();
    w.finish().unwrap()
}

#[test]
fn writer_output_reads_back() {
    let bytes = sample_zip();
    assert_eq!(bytes, sample_zip(), "two writes give equal bytes");
    let zip = open_bytes(&bytes).unwrap();
    let names: Vec<_> = zip.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["a.txt", "dir/b.xml", "audio/001.wav"]);

    let cancel = no_cancel();
    let want: [(&str, Vec<u8>, EntryMethod); 3] = [
        ("a.txt", b"hi".to_vec(), EntryMethod::Stored),
        ("dir/b.xml", text_100k(), EntryMethod::Deflated),
        ("audio/001.wav", vec![1, 2, 3], EntryMethod::Stored),
    ];
    for (name, data, method) in want {
        let e = zip.find(name).unwrap();
        assert_eq!(e.method, Some(method), "{name}");
        assert!(!e.encrypted, "{name}");
        assert_eq!(e.size, data.len() as u64, "{name}");
        match method {
            EntryMethod::Stored => assert_eq!(e.compressed, e.size, "{name}"),
            EntryMethod::Deflated => assert!(e.compressed < e.size / 4, "{name}"),
        }
        assert_eq!(
            zip.read(e, u64::MAX, None, &cancel).unwrap(),
            data,
            "{name}"
        );
    }
    assert!(zip.find("missing").is_none());
}

#[test]
fn names_that_leave_the_package_are_refused() {
    let long = "n".repeat(1025);
    for name in [
        "../x",
        "/abs",
        "C:x",
        "a/../../b",
        "a\\..\\b",
        "x\0y",
        "a//b",
        "./x",
        &long,
    ] {
        let bytes = raw_zip(&[RawEntry::stored(name, b"x")]);
        assert_eq!(
            refusal(open_bytes(&bytes)),
            damaged("an entry name leaves the package"),
            "{name:?}"
        );
        assert!(check_name(name).is_err(), "{name:?}");
        let mut w = ZipWriter::new(Vec::new());
        assert!(w.add(name, EntryMethod::Stored, b"x").is_err(), "{name:?}");
    }
    assert_eq!(check_name("audio/001.wav"), Ok(()));
    assert_eq!(check_name("plugins/001.aupreset"), Ok(()));
    assert_eq!(check_name(&"n".repeat(1024)), Ok(()));
    // A directory entry's own name is checked too.
    let bytes = raw_zip(&[RawEntry::stored("../", b"")]);
    assert_eq!(
        refusal(open_bytes(&bytes)),
        damaged("an entry name leaves the package")
    );
    // Names that are not UTF-8 cannot be named safely.
    let mut bad = RawEntry::stored("x", b"x");
    bad.name = vec![b'a', 0xFF];
    assert_eq!(
        refusal(open_bytes(&raw_zip(&[bad]))),
        damaged("an entry name leaves the package")
    );
}

#[test]
fn directory_entries_are_checked_then_skipped() {
    let bytes = raw_zip(&[
        RawEntry::stored("audio/", b""),
        RawEntry::stored("audio/001.wav", b"abc"),
    ]);
    let zip = open_bytes(&bytes).unwrap();
    let names: Vec<_> = zip.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["audio/001.wav"]);
}

#[test]
fn duplicate_names_are_refused() {
    let bytes = raw_zip(&[
        RawEntry::stored("a.txt", b"one"),
        RawEntry::stored("a.txt", b"two"),
    ]);
    assert_eq!(
        refusal(open_bytes(&bytes)),
        damaged("two entries are named 'a.txt'")
    );
    let mut w = ZipWriter::new(Vec::new());
    w.add("a.txt", EntryMethod::Stored, b"one").unwrap();
    assert!(w.add("a.txt", EntryMethod::Stored, b"two").is_err());
}

#[test]
fn symlink_entries_are_refused() {
    let mut link = RawEntry::stored("audio/001.wav", b"/etc/passwd");
    link.unix_mode = Some(0o120_777);
    assert_eq!(
        refusal(open_bytes(&raw_zip(&[link]))),
        damaged("it holds a link")
    );
    // A plain Unix file mode is not a link.
    let mut file = RawEntry::stored("audio/001.wav", b"abc");
    file.unix_mode = Some(0o100_644);
    assert!(open_bytes(&raw_zip(&[file])).is_ok());
}

#[test]
fn overlapping_entries_are_refused() {
    // Two central records pointing at one local header and its data.
    let mut bytes = raw_zip(&[
        RawEntry::stored("a.xml", b"same bytes"),
        RawEntry::stored("b.xml", b"same bytes"),
    ]);
    // b's local header starts after a's: its offset field is the last four
    // bytes before b's name in the central directory. Point it at a.
    let cd = bytes
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x01, 0x02])
        .unwrap();
    bytes
        .get_mut(cd + 42..cd + 46)
        .unwrap()
        .copy_from_slice(&[0; 4]);
    assert_eq!(
        refusal(open_bytes(&bytes)),
        damaged("two entries share their data")
    );
}

#[test]
fn a_compressed_size_past_the_directory_is_refused() {
    // A 10 MiB deflate bomb whose central record claims ~4 GiB packed, so
    // that "size / compressed" looks harmless; DEFLATE stops at its own end
    // marker, so the over-long range would never be read.
    let zeros = vec![0u8; 10 * MIB as usize];
    let mut bytes = raw_zip(&[RawEntry::deflated("project.xml", &zeros)]);
    let cd = bytes
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x01, 0x02])
        .unwrap();
    bytes
        .get_mut(cd + 20..cd + 24)
        .unwrap()
        .copy_from_slice(&0xFFFF_FF00u32.to_le_bytes());
    let cancel = no_cancel();
    let r = open_bytes(&bytes).and_then(|zip| {
        let e = zip.find("project.xml").unwrap();
        zip.read(e, 64 * MIB, Some(200), &cancel)
    });
    assert_eq!(
        refusal(r),
        damaged("project.xml runs past the end of its data")
    );
}

fn many(n: usize) -> Vec<RawEntry> {
    (0..n)
        .map(|i| RawEntry::stored(&format!("f/{i}"), b""))
        .collect()
}

#[test]
fn entry_count_limits() {
    let zip = open_bytes(&raw_zip(&many(10_000))).unwrap();
    assert_eq!(zip.entries().len(), 10_000);

    let too_many = damaged("it holds more than 10 000 files");
    assert_eq!(refusal(open_bytes(&raw_zip(&many(10_001)))), too_many);

    // The end record claims 65 535 over a 2-entry directory: refused from
    // the claim alone, before the directory is walked.
    let dir = tempfile::tempdir().unwrap();
    let path = put(
        dir.path(),
        "claim.dawproject",
        &raw_zip_claiming(&many(2), u16::MAX),
    );
    let (r, seen) = measure(|| ZipReader::open(&path, &PackageLimits::default()));
    assert_eq!(refusal(r), too_many);
    assert!(seen.total < MIB as usize, "open allocated {seen:?}");

    // A lowered limit counts while walking: the claim says 2, there are 5.
    let lowered = PackageLimits {
        max_entries: 3,
        ..PackageLimits::default()
    };
    let path = put(dir.path(), "lie.dawproject", &raw_zip_claiming(&many(5), 2));
    assert_eq!(
        refusal(ZipReader::open(&path, &lowered)),
        damaged("it holds more than 3 files")
    );
}

#[test]
fn xml_bombs_are_refused() {
    let zeros = vec![0u8; 50 * MIB as usize];
    let dir = tempfile::tempdir().unwrap();
    let cancel = no_cancel();

    let mut w = ZipWriter::new(Vec::new());
    w.add("project.xml", EntryMethod::Deflated, &zeros).unwrap();
    let honest = put(dir.path(), "bomb.dawproject", &w.finish().unwrap());
    let zip = ZipReader::open(&honest, &PackageLimits::default()).unwrap();
    let e = zip.find("project.xml").unwrap();
    assert_eq!(
        refusal(zip.read(e, 64 * MIB, Some(200), &cancel)),
        damaged("project.xml unpacks to more than 200 times its size")
    );
    // Past `max`, TOO_LARGE (S12).
    assert_eq!(refusal(zip.read(e, MIB, None, &cancel)), TOO_LARGE);
    // With no ratio and room enough, it reads.
    assert_eq!(zip.read(e, 64 * MIB, None, &cancel).unwrap(), zeros);

    // The same data declared as 1 KiB: refused once the 1 KiB is passed,
    // with no more than 1 KiB, one 64 KiB buffer and the inflater held.
    let mut liar = RawEntry::deflated("project.xml", &zeros);
    liar.declared_size = 1024;
    let path = put(dir.path(), "liar.dawproject", &raw_zip(&[liar]));
    let zip = ZipReader::open(&path, &PackageLimits::default()).unwrap();
    let e = zip.find("project.xml").unwrap().clone();
    let (r, seen) = measure(|| zip.read(&e, 64 * MIB, Some(200), &cancel));
    assert_eq!(refusal(r), damaged("project.xml is larger than it says"));
    // About 140 KiB in practice: the 64 KiB chunk and the inflater's state.
    assert!(seen.peak < 1024 + 64 * 1024 + 96 * 1024, "held {seen:?}");
}

#[test]
fn encrypted_and_unknown_methods_are_unreadable() {
    let mut secret = RawEntry::stored("secret.xml", b"<x/>");
    secret.flags = 1;
    let mut odd = RawEntry::stored("odd.wav", b"abcd");
    odd.method = 12;
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "p.dawproject", &raw_zip(&[secret, odd]));
    let zip = ZipReader::open(&path, &PackageLimits::default()).unwrap();
    let cancel = no_cancel();

    let e = zip.find("secret.xml").unwrap();
    assert!(e.encrypted);
    let encrypted = "secret.xml is encrypted.";
    assert_eq!(refusal(zip.read(e, MIB, None, &cancel)), encrypted);
    let to = dir.path().join("secret.out");
    assert_eq!(refusal(zip.extract(e, &to, &cancel)), encrypted);
    assert!(std::fs::symlink_metadata(&to).is_err());

    let e = zip.find("odd.wav").unwrap();
    assert_eq!(e.method, None);
    let unknown = "odd.wav uses a compression VizDAW can't read.";
    assert_eq!(refusal(zip.read(e, MIB, None, &cancel)), unknown);
    let to = dir.path().join("odd.out");
    assert_eq!(refusal(zip.extract(e, &to, &cancel)), unknown);
    assert!(std::fs::symlink_metadata(&to).is_err());
}

/// Opens `bytes` and reads and extracts every entry; every outcome is `Ok`
/// or `Refused`, never a panic or an OS error.
fn try_all(bytes: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "p.dawproject", bytes);
    let zip = match ZipReader::open(&path, &PackageLimits::default()) {
        Ok(z) => z,
        Err(IoError::Refused(_)) => return,
        Err(e) => panic!("open gave {e:?}"),
    };
    let cancel = no_cancel();
    for (i, e) in zip.entries().iter().enumerate() {
        match zip.read(e, MIB, Some(200), &cancel) {
            Ok(_) | Err(IoError::Refused(_)) => {}
            Err(e) => panic!("read gave {e:?}"),
        }
        let to = dir.path().join(format!("out{i}"));
        match zip.extract(e, &to, &cancel) {
            Ok(n) => assert_eq!(std::fs::metadata(&to).unwrap().len(), n),
            Err(IoError::Refused(_)) => assert!(std::fs::symlink_metadata(&to).is_err()),
            Err(e) => panic!("extract gave {e:?}"),
        }
    }
}

/// A small valid 3-entry package.
fn small_zip() -> Vec<u8> {
    let mut w = ZipWriter::new(Vec::new());
    w.add("a.txt", EntryMethod::Stored, b"hi").unwrap();
    w.add("b.xml", EntryMethod::Deflated, &text_100k()[..2000])
        .unwrap();
    w.add("c.wav", EntryMethod::Stored, &[1, 2, 3]).unwrap();
    w.finish().unwrap()
}

#[test]
fn damaged_bytes_never_panic() {
    let good = small_zip();
    for n in 0..good.len() {
        try_all(good.get(..n).unwrap());
    }
    for i in 0..good.len() {
        let mut flipped = good.clone();
        *flipped.get_mut(i).unwrap() ^= 0xFF;
        try_all(&flipped);
    }
    // 4 KiB of xorshift noise, with and without an end record after it.
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let noise: Vec<u8> = (0..4096)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x.to_le_bytes()[0]
        })
        .collect();
    try_all(&noise);
    let mut tailed = noise.clone();
    tailed.extend_from_slice(&good[good.len() - 22..]);
    try_all(&tailed);
}

#[test]
fn zip64_edge_sizes_never_panic() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = no_cancel();

    // Sizes 0xFFFF_FFFF with a ZIP64 extra claiming 2^63.
    let mut huge = RawEntry::stored("project.xml", b"tiny");
    huge.zip64_size = Some(1 << 63);
    let path = put(dir.path(), "huge.dawproject", &raw_zip(&[huge]));
    let (r, seen) = measure(|| {
        let zip = ZipReader::open(&path, &PackageLimits::default())?;
        let e = zip.find("project.xml").unwrap().clone();
        assert_eq!(e.size, 1 << 63);
        zip.read(&e, 64 * MIB, Some(200), &cancel)
    });
    assert!(matches!(r, Err(IoError::Refused(_))), "{r:?}");
    assert!(seen.total < 2 * MIB as usize, "allocated {seen:?}");

    // Declared 0 over 1 MiB of data.
    let mut zero = RawEntry::stored("audio/001.wav", &vec![7u8; MIB as usize]);
    zero.declared_size = 0;
    let path = put(dir.path(), "zero.dawproject", &raw_zip(&[zero]));
    let zip = ZipReader::open(&path, &PackageLimits::default()).unwrap();
    let e = zip.find("audio/001.wav").unwrap().clone();
    let (r, seen) = measure(|| zip.read(&e, 64 * MIB, None, &cancel));
    assert_eq!(refusal(r), damaged("audio/001.wav is larger than it says"));
    assert!(seen.total < 2 * MIB as usize, "allocated {seen:?}");
    let to = dir.path().join("zero.wav");
    assert_eq!(
        refusal(zip.extract(&e, &to, &cancel)),
        damaged("audio/001.wav is larger than it says")
    );
    assert!(std::fs::symlink_metadata(&to).is_err());
}

#[test]
fn extract_is_bounded_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = no_cancel();

    // A wrong CRC: refused, and the copy is gone.
    let mut bad = RawEntry::stored("audio/001.wav", b"hello");
    bad.crc = Some(1);
    let path = put(dir.path(), "crc.dawproject", &raw_zip(&[bad]));
    let zip = ZipReader::open(&path, &PackageLimits::default()).unwrap();
    let e = zip.find("audio/001.wav").unwrap();
    let to = dir.path().join("crc.wav");
    let checksum = damaged("audio/001.wav fails its checksum");
    assert_eq!(refusal(zip.extract(e, &to, &cancel)), checksum);
    assert!(std::fs::symlink_metadata(&to).is_err());
    assert_eq!(refusal(zip.read(e, MIB, None, &cancel)), checksum);

    // An existing destination is refused and left as it was.
    let good = put(dir.path(), "good.dawproject", &small_zip());
    let zip = ZipReader::open(&good, &PackageLimits::default()).unwrap();
    let kept = put(dir.path(), "kept.wav", b"keep");
    let e = zip.find("c.wav").unwrap();
    assert!(matches!(
        zip.extract(e, &kept, &cancel),
        Err(IoError::Refused(_))
    ));
    assert_eq!(std::fs::read(&kept).unwrap(), b"keep");
    // And a fresh one gets the bytes.
    let fresh = dir.path().join("fresh.wav");
    assert_eq!(zip.extract(e, &fresh, &cancel).unwrap(), 3);
    assert_eq!(std::fs::read(&fresh).unwrap(), [1, 2, 3]);

    // A cancel arriving once the first 64 KiB chunk is written (the hook
    // sets the flag there): Cancelled, and the copy is gone.
    let big = dir.path().join("big.dawproject");
    let mut w = ZipWriter::new(std::fs::File::create(&big).unwrap());
    let mut src = std::io::Read::take(std::io::repeat(0), 4 * MIB);
    assert_eq!(
        w.add_from("audio/big.wav", EntryMethod::Deflated, &mut src, &cancel)
            .unwrap(),
        4 * MIB
    );
    w.finish().unwrap();
    let mut zip = ZipReader::open(&big, &PackageLimits::default()).unwrap();
    let e = zip.find("audio/big.wav").unwrap().clone();
    // Without the hook the whole entry extracts.
    let whole = dir.path().join("whole.wav");
    assert_eq!(zip.extract(&e, &whole, &cancel).unwrap(), 4 * MIB);
    zip.debug_cancel_at_chunk(Some(1));
    let to = dir.path().join("big.wav");
    let stop = AtomicBool::new(false);
    assert_eq!(zip.extract(&e, &to, &stop), Err(IoError::Cancelled));
    assert!(stop.load(Ordering::Relaxed), "the hook set the flag");
    assert!(std::fs::symlink_metadata(&to).is_err());
    // And a read stops the same way.
    let stop = AtomicBool::new(false);
    assert_eq!(zip.read(&e, 64 * MIB, None, &stop), Err(IoError::Cancelled));

    // A copy that cannot be removed after a failure is reported, naming
    // the file left behind.
    zip.debug_cancel_at_chunk(Some(1));
    zip.debug_fail_removal(true);
    let left = dir.path().join("left.wav");
    let stop = AtomicBool::new(false);
    match zip.extract(&e, &left, &stop) {
        Err(IoError::Io(why)) => {
            assert!(why.starts_with("left.wav was left behind"), "{why}");
            assert!(why.contains("could not be removed"), "{why}");
        }
        other => panic!("expected the leftover to be reported, got {other:?}"),
    }
    assert!(std::fs::metadata(&left).unwrap().len() > 0);

    // A writer cancelled mid-entry stops.
    let set = AtomicBool::new(true);
    let mut w = ZipWriter::new(Vec::new());
    let mut src = std::io::Read::take(std::io::repeat(0), MIB);
    assert_eq!(
        w.add_from("x.wav", EntryMethod::Stored, &mut src, &set),
        Err(IoError::Cancelled)
    );
}

#[test]
fn open_refuses_what_is_not_a_regular_file() {
    let dir = tempfile::tempdir().unwrap();
    let limits = PackageLimits::default();
    assert!(matches!(
        ZipReader::open(dir.path(), &limits),
        Err(IoError::Refused(_))
    ));

    let fifo = dir.path().join("fifo.dawproject");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    assert!(matches!(
        ZipReader::open(&fifo, &limits),
        Err(IoError::Refused(_))
    ));

    let (a, b) = (
        dir.path().join("a.dawproject"),
        dir.path().join("b.dawproject"),
    );
    std::os::unix::fs::symlink(&b, &a).unwrap();
    std::os::unix::fs::symlink(&a, &b).unwrap();
    assert!(ZipReader::open(&a, &limits).is_err());

    // Not a ZIP at all.
    let text = put(dir.path(), "text.dawproject", b"just some text");
    assert_eq!(
        refusal(ZipReader::open(&text, &limits)),
        core_io::zip::NOT_DAWPROJECT
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn random_bytes_never_panic(
        noise in proptest::collection::vec(any::<u8>(), 0..4096),
        hits in proptest::collection::vec((any::<usize>(), any::<u8>()), 0..8),
    ) {
        try_all(&noise);
        // The valid package with a few bytes replaced.
        let mut hit = small_zip();
        let len = hit.len();
        for (at, b) in hits {
            if let Some(x) = hit.get_mut(at % len) {
                *x = b;
            }
        }
        try_all(&hit);
    }
}
