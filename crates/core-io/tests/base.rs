use core_io::IoError;
use core_io::limits::Limits;
use core_io::read::read_regular;
use core_io::report::{ItemKind, Report};
use core_io::text::sanitize;
use core_io::write::{free_folder, stage_folder, write_partial};
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

#[test]
fn report_plain_matches_the_sheet() {
    assert_eq!(
        Report::default().plain("Exported"),
        "Exported nothing\nEverything transferred."
    );
    let mut r = Report::default();
    r.summary.notes = 3;
    for k in 1..=120 {
        r.add(ItemKind::LeftOut, "Muted notes", Some(format!("bar {k}")));
    }
    // Kept without details: the count alone, no "and N more".
    r.add(ItemKind::Note, "Once", None);
    r.add(ItemKind::Changed, "Twice", None);
    r.add(ItemKind::Changed, "Twice", None);
    let mut want = vec![
        "Exported 3 notes".to_string(),
        "Changed".into(),
        "  Twice (2)".into(),
        "Left out".into(),
        "  Muted notes (120)".into(),
    ];
    want.extend((1..=50).map(|k| format!("    bar {k}")));
    want.push("    and 70 more".into());
    want.push("Notes".into());
    want.push("  Once".into());
    assert_eq!(r.plain("Exported"), want.join("\n"));
}

#[test]
fn summary_line_counts_audio_files_after_tracks() {
    let mut r = Report::default();
    r.summary.tracks = 2;
    r.summary.audio_files = 3;
    r.summary.notes = 1200;
    r.summary.tempo_map = true;
    r.summary.markers = 4;
    assert_eq!(
        r.summary_line("Exported"),
        "Exported 2 tracks, 3 audio files, 1 200 notes, the tempo map and 4 markers"
    );
    let mut r = Report::default();
    r.summary.audio_files = 1;
    assert_eq!(r.summary_line("Exported"), "Exported 1 audio file");
    let mut sum = Report::default();
    sum.merge(r.clone());
    sum.merge(r);
    assert_eq!(sum.summary.audio_files, 2);
}

#[test]
fn free_folder_numbers_past_anything_taken() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    let name = "Song for another DAW";
    assert_eq!(free_folder(p, name).unwrap(), p.join(name));
    // An undotted "<name>.partial" is someone else's: it does not count.
    std::fs::create_dir(p.join(format!("{name}.partial"))).unwrap();
    assert_eq!(free_folder(p, name).unwrap(), p.join(name));

    let taken: [&dyn Fn(&std::path::Path); 4] = [
        &|q| std::fs::create_dir(q.join(name)).unwrap(),
        &|q| std::fs::write(q.join(name), b"x").unwrap(),
        &|q| std::os::unix::fs::symlink(q.join("nowhere"), q.join(name)).unwrap(),
        &|q| {
            let stale = core_media::export::partial_path(&q.join(name));
            std::fs::create_dir(stale).unwrap();
        },
    ];
    for (i, take) in taken.iter().enumerate() {
        let e = tempfile::tempdir().unwrap();
        take(e.path());
        assert_eq!(
            free_folder(e.path(), name).unwrap(),
            e.path().join(format!("{name} 2")),
            "case {i}"
        );
        if i == 3 {
            // The stale partial is passed over, never removed.
            let stale = core_media::export::partial_path(&e.path().join(name));
            assert!(std::fs::symlink_metadata(stale).unwrap().is_dir());
        }
    }

    // " 2" taken too: " 3"; every number up to 9999 taken: refused.
    let e = tempfile::tempdir().unwrap();
    std::fs::create_dir(e.path().join("A")).unwrap();
    std::fs::create_dir(e.path().join("A 2")).unwrap();
    assert_eq!(free_folder(e.path(), "A").unwrap(), e.path().join("A 3"));
    for n in 3..=9999 {
        std::fs::write(e.path().join(format!("A {n}")), b"").unwrap();
    }
    assert!(matches!(
        free_folder(e.path(), "A"),
        Err(IoError::Refused(_))
    ));
}

#[test]
fn staged_folders_commit_or_vanish() {
    let d = tempfile::tempdir().unwrap();
    let dest = d.path().join("Song for another DAW");
    let partial = core_media::export::partial_path(&dest);

    // Dropped uncommitted: nothing is left, contents included.
    let f = stage_folder(&dest).unwrap();
    assert_eq!(f.path(), partial.as_path());
    std::fs::create_dir(f.path().join("MIDI")).unwrap();
    std::fs::write(f.path().join("MIDI/01 Bass.mid"), b"x").unwrap();
    drop(f);
    assert!(std::fs::symlink_metadata(&partial).is_err());
    assert!(std::fs::symlink_metadata(&dest).is_err());

    // Committed: the folder, with its contents.
    let f = stage_folder(&dest).unwrap();
    std::fs::write(f.path().join("About these files.txt"), b"hi").unwrap();
    assert_eq!(f.commit().unwrap(), dest);
    assert_eq!(
        std::fs::read(dest.join("About these files.txt")).unwrap(),
        b"hi"
    );
    assert!(std::fs::symlink_metadata(&partial).is_err());

    // A stale staging folder is refused by name and left alone.
    let other = d.path().join("Other");
    let stale = core_media::export::partial_path(&other);
    std::fs::create_dir(&stale).unwrap();
    let e = stage_folder(&other).unwrap_err();
    assert!(
        matches!(&e, IoError::Refused(m) if m == ".Other.partial is left from an earlier export; remove it or choose another name"),
        "{e:?}"
    );
    assert!(stale.is_dir());
    std::fs::remove_dir(&stale).unwrap();

    // The destination appears before the commit: refused, theirs kept,
    // the staging folder gone.
    let f = stage_folder(&other).unwrap();
    std::fs::write(f.path().join("mine"), b"1").unwrap();
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("theirs"), b"2").unwrap();
    let e = f.commit().unwrap_err();
    assert!(
        matches!(&e, IoError::Refused(m) if m == "Other exists; choose another name"),
        "{e:?}"
    );
    assert_eq!(std::fs::read(other.join("theirs")).unwrap(), b"2");
    assert!(std::fs::symlink_metadata(other.join("mine")).is_err());
    assert!(std::fs::symlink_metadata(&stale).is_err());
}
