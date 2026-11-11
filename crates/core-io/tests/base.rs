use core_io::IoError;
use core_io::limits::Limits;
use core_io::read::read_regular;
use core_io::report::{ItemKind, Report};
use core_io::text::sanitize;
use core_io::write::write_partial;
use proptest::prelude::*;
use std::time::{Duration, Instant};

fn refused_fast(r: Result<Vec<u8>, IoError>, what: &str, t: Instant) {
    assert!(
        matches!(r, Err(IoError::Refused(_) | IoError::Io(_))),
        "{what}: {r:?}"
    );
    assert!(t.elapsed() < Duration::from_secs(1), "{what} took too long");
}

#[test]
fn read_regular_refuses_non_files() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    refused_fast(read_regular(p, 100), "directory", Instant::now());

    let fifo = p.join("fifo");
    match std::process::Command::new("mkfifo").arg(&fifo).status() {
        Ok(s) if s.success() => refused_fast(read_regular(&fifo, 100), "fifo", Instant::now()),
        _ => println!("mkfifo is missing: FIFO case skipped"),
    }

    refused_fast(
        read_regular(std::path::Path::new("/dev/zero"), 100),
        "/dev/zero",
        Instant::now(),
    );

    #[cfg(unix)]
    {
        let (a, b) = (p.join("a"), p.join("b"));
        std::os::unix::fs::symlink(&b, &a).unwrap();
        std::os::unix::fs::symlink(&a, &b).unwrap();
        refused_fast(read_regular(&a, 100), "symlink loop", Instant::now());
    }

    refused_fast(
        read_regular(&p.join("missing"), 100),
        "missing",
        Instant::now(),
    );

    let big = p.join("big");
    std::fs::write(&big, vec![7u8; 101]).unwrap();
    refused_fast(read_regular(&big, 100), "max + 1", Instant::now());

    let exact = p.join("exact");
    std::fs::write(&exact, vec![7u8; 100]).unwrap();
    assert_eq!(read_regular(&exact, 100).unwrap(), vec![7u8; 100]);

    #[cfg(unix)]
    {
        let link = p.join("link");
        std::os::unix::fs::symlink(&exact, &link).unwrap();
        assert_eq!(read_regular(&link, 100).unwrap(), vec![7u8; 100]);
    }
}

#[test]
fn partial_files_are_removed_unless_committed() {
    let d = tempfile::tempdir().unwrap();
    let dest = d.path().join("x.mid");
    let partial = d.path().join(".x.mid.partial");

    drop(write_partial(&dest, b"abc", false).unwrap());
    assert!(!partial.exists() && !dest.exists());

    let pending = write_partial(&dest, b"abc", false).unwrap();
    assert!(partial.exists() && !dest.exists());
    assert_eq!(pending.commit().unwrap(), dest);
    assert!(!partial.exists());
    assert_eq!(std::fs::read(&dest).unwrap(), b"abc");

    // Taken without replace: refused before anything is written.
    let e = write_partial(&dest, b"new", false).unwrap_err();
    assert!(
        matches!(&e, IoError::Refused(m) if m == "x.mid exists; choose another name or replace it"),
        "{e:?}"
    );
    assert!(!partial.exists());
    assert_eq!(std::fs::read(&dest).unwrap(), b"abc");

    // With replace it is overwritten.
    write_partial(&dest, b"new", true)
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"new");

    // It appeared between the write and the commit: refused, partial removed.
    let other = d.path().join("y.mid");
    let pending = write_partial(&other, b"1", false).unwrap();
    std::fs::write(&other, b"theirs").unwrap();
    assert!(matches!(pending.commit(), Err(IoError::Refused(_))));
    assert_eq!(std::fs::read(&other).unwrap(), b"theirs");
    assert!(!d.path().join(".y.mid.partial").exists());

    // A stale partial is refused by name and left alone.
    std::fs::remove_file(&dest).unwrap();
    std::fs::write(&partial, b"stale").unwrap();
    let e = write_partial(&dest, b"abc", false).unwrap_err();
    assert!(
        matches!(&e, IoError::Refused(m) if m == ".x.mid.partial is left from an earlier export; remove it or choose another name"),
        "{e:?}"
    );
    assert_eq!(std::fs::read(&partial).unwrap(), b"stale");
}

#[test]
fn sanitize_cuts_and_cleans() {
    let l = Limits::default();
    assert_eq!(
        sanitize("A\u{0007}B\nC".as_bytes(), &l),
        ("AB\nC".to_string(), false)
    );
    let (s, cut) = sanitize(&[b'a', 0xFF, b'b'], &l);
    assert_eq!((s.as_str(), cut), ("a\u{FFFD}b", false));
    let l4 = Limits {
        max_text_bytes: 4,
        ..Limits::default()
    };
    assert_eq!(sanitize(b"abcd", &l4), ("abcd".to_string(), false));
    assert_eq!(sanitize(b"abcde", &l4), ("abcd".to_string(), true));
    // Never inside a character: "ab" + 3-byte char needs 5 bytes.
    assert_eq!(
        sanitize("ab\u{20AC}".as_bytes(), &l4),
        ("ab".to_string(), true)
    );
}

#[test]
fn report_groups_and_caps_details() {
    let mut r = Report::default();
    assert!(r.is_empty());
    for k in 0..120 {
        r.add(ItemKind::LeftOut, "Muted notes", Some(format!("bar {k}")));
    }
    r.add(ItemKind::Note, "Muted notes", None);
    assert_eq!(r.items.len(), 2);
    assert_eq!((r.items[0].count, r.items[0].details.len()), (120, 50));
    assert_eq!(r.items[0].details[0], "bar 0");
    assert!(!r.is_empty());

    let mut s = Report::default();
    s.summary.tracks = 12;
    s.summary.notes = 4210;
    s.summary.markers = 8;
    s.summary.tempo_map = true;
    assert_eq!(
        s.summary_line("Imported"),
        "Imported 12 tracks, 4 210 notes, the tempo map and 8 markers"
    );
    let mut one = Report::default();
    one.summary.tracks = 1;
    one.summary.notes = 1;
    assert_eq!(one.summary_line("Exported"), "Exported 1 track and 1 note");
    one.summary.notes = 0;
    assert_eq!(one.summary_line("Exported"), "Exported 1 track");
    assert_eq!(
        Report::default().summary_line("Imported"),
        "Imported nothing"
    );

    let mut m = Report::default();
    m.add(ItemKind::Note, "Muted notes", Some("x".into()));
    m.merge(r);
    assert_eq!(m.items.len(), 2);
    assert_eq!((m.items[0].kind, m.items[0].count), (ItemKind::Note, 2));
    assert_eq!(m.items[1].count, 120);
}

#[test]
fn limits_are_the_spec_values() {
    use core_io::limits::*;
    assert_eq!(MAX_MIDI_FILE_BYTES, 67_108_864);
    assert_eq!(
        (MAX_VLQ_BYTES, MAX_EVENTS, MAX_SYSEX_BYTES),
        (4, 10_000_000, 1_048_576)
    );
    assert_eq!(
        (MAX_TRACKS, MAX_TEXT_BYTES, MAX_DETAILS),
        (4096, 65_536, 50)
    );
    assert_eq!(
        Limits::default(),
        Limits {
            max_events: 10_000_000,
            max_sysex_bytes: 1_048_576,
            max_tracks: 4096,
            max_text_bytes: 65_536
        }
    );
}

#[test]
fn details_cap_at_the_edges() {
    for (n, kept) in [(50, 50), (51, 50)] {
        let mut r = Report::default();
        for k in 0..n {
            r.add(ItemKind::Changed, "x", Some(k.to_string()));
        }
        assert_eq!(
            (r.items[0].count, r.items[0].details.len()),
            (n as u64, kept)
        );
    }
}

#[test]
fn merge_caps_details_of_new_and_existing_items() {
    let fill = |what: &str, n: usize| {
        let mut r = Report::default();
        for k in 0..n {
            r.add(ItemKind::LeftOut, what, Some(k.to_string()));
        }
        r
    };
    let mut a = fill("same", 40);
    a.merge(fill("same", 40));
    assert_eq!((a.items[0].count, a.items[0].details.len()), (80, 50));
    // An item that is new to `a` arrives over the cap (public fields).
    let mut big = fill("new", 10);
    big.items[0].details = (0..70).map(|k| k.to_string()).collect();
    a.merge(big);
    assert_eq!(a.items[1].details.len(), 50);
    // An over-full item on our side does not underflow.
    a.items[0].details = (0..70).map(|k| k.to_string()).collect();
    a.merge(fill("same", 5));
    assert_eq!(a.items[0].details.len(), 70);
}

#[test]
fn summary_line_covers_every_count() {
    let mut r = Report::default();
    r.summary.regions = 2;
    r.summary.automation_points = 1_000_000;
    r.summary.plugins = 1;
    assert_eq!(
        r.summary_line("Exported"),
        "Exported 2 regions, 1 000 000 automation points and 1 plug-in"
    );
    r.summary.plugins = 3;
    r.summary.regions = 1;
    r.summary.automation_points = 1;
    assert_eq!(
        r.summary_line("Exported"),
        "Exported 1 region, 1 automation point and 3 plug-ins"
    );
    for (n, text) in [
        (999, "999 notes"),
        (1000, "1 000 notes"),
        (1_234_567, "1 234 567 notes"),
    ] {
        let mut r = Report::default();
        r.summary.notes = n;
        assert_eq!(r.summary_line("Read"), format!("Read {text}"));
    }
}

#[test]
fn sanitize_strips_bidi_and_tabs() {
    let l = Limits::default();
    assert_eq!(
        sanitize("a\u{202E}b\u{2066}c\u{2069}d\u{202A}".as_bytes(), &l),
        ("abcd".to_string(), false)
    );
    assert_eq!(sanitize(b"a\tb\r\nc", &l), ("ab\nc".to_string(), false));
}

#[test]
fn sanitize_hostile_input_is_bounded() {
    let l = Limits::default();
    let t = Instant::now();
    let (s, cut) = sanitize(&vec![0xFFu8; 8 * 1024 * 1024], &l);
    assert!(cut && s.len() <= l.max_text_bytes);
    assert!(t.elapsed() < Duration::from_secs(2));
}

fn bad(c: char) -> bool {
    (c.is_control() && c != '\n') || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

proptest! {
    #[test]
    fn sanitize_properties(bytes in proptest::collection::vec(any::<u8>(), 0..300), max in 0usize..40) {
        let l = Limits { max_text_bytes: max, ..Limits::default() };
        let (out, cut) = sanitize(&bytes, &l);
        prop_assert!(out.len() <= max);
        prop_assert!(!out.chars().any(bad));
        let full = sanitize(&bytes, &Limits { max_text_bytes: usize::MAX, ..Limits::default() });
        prop_assert!(!full.1);
        prop_assert_eq!(cut, full.0.len() > max);
        prop_assert!(full.0.starts_with(&out));
    }
}

#[cfg(unix)]
#[test]
fn symlink_destinations() {
    let d = tempfile::tempdir().unwrap();
    let target = d.path().join("target.mid");
    std::fs::write(&target, b"old").unwrap();
    let link = d.path().join("link.mid");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    // Replace swaps the link itself and leaves what it pointed at.
    write_partial(&link, b"new", true)
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(std::fs::read(&link).unwrap(), b"new");
    assert!(
        !std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"old");
    // A dangling link counts as taken without replace.
    let dangling = d.path().join("dangling.mid");
    std::os::unix::fs::symlink(d.path().join("nowhere"), &dangling).unwrap();
    let e = write_partial(&dangling, b"x", false).unwrap_err();
    assert!(
        matches!(&e, IoError::Refused(m) if m.contains("exists")),
        "{e:?}"
    );
    assert!(!d.path().join(".dangling.mid.partial").exists());
}

#[test]
fn io_errors_name_the_file() {
    let d = tempfile::tempdir().unwrap();
    let e = read_regular(&d.path().join("gone.mid"), 10).unwrap_err();
    assert!(
        matches!(&e, IoError::Io(m) if m.contains("gone.mid")),
        "{e:?}"
    );
    let e = write_partial(&d.path().join("no/such/dir/x.mid"), b"", false).unwrap_err();
    assert!(matches!(&e, IoError::Io(m) if m.contains("x.mid")), "{e:?}");
}
