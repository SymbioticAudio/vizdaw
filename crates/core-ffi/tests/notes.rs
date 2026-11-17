//! The song's notes file, Co-Producer's one entry per request, dated
//! export folders, an untitled song's notes and copies moved in on its
//! first save, and the copies offer's list and removal (plan 8b Task 3;
//! Rulings 4–8). Every file Co-Producer reads is hostile input; nothing
//! here touches the real `~/Library`.

// `im` maps nest deeply: sending a `Session` around needs more depth.
#![recursion_limit = "256"]

mod common;

use core_control::request::ExportWhat;
use core_media::caf::{CafWriter, WriteMode};
use core_model::diff::ChangeItem;
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::jobs::ImportTargetRow;
use vizdaw_ffi::notes::{self, MAX_READ, NOTES_FILE, RequestLog};
use vizdaw_ffi::record::set_free_bytes;
use vizdaw_ffi::tracks::TrackKindRow;
use vizdaw_ffi::{ExportWhatRow, JobStateRow, Session};

const NOT_PLAIN: &str = "Notes.md is not a plain file, so Co-Producer leaves it alone.";
const ASK: &str = "Ask the composer where exports should go, then call export_folder with parent.";
const OPEN_FIRST: &str = "Open a request first (begin_request).";
const STAMP: &str = "2026-11-16 10-05";
const DATE: &str = "2026-11-16";

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    set_free_bytes(None);
    g
}

fn item(thing: &str, change: &str) -> ChangeItem {
    ChangeItem {
        thing: thing.into(),
        change: change.into(),
        co_producers: false,
    }
}

fn text_of(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

fn write(p: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

// ---- notes.rs on temp folders ----

#[test]
fn a_new_song_gets_a_heading() {
    let dir = tempfile::tempdir().unwrap();
    let p = notes::ensure(dir.path(), "Night Drive").unwrap();
    assert_eq!(p, dir.path().join(NOTES_FILE));
    assert_eq!(text_of(&p), "# Night Drive\n");
    std::fs::write(&p, "# Night Drive\nmine").unwrap();
    notes::ensure(dir.path(), "Other").unwrap();
    assert_eq!(text_of(&p), "# Night Drive\nmine", "never replaced");
}

#[test]
fn appending_never_rewrites() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join(NOTES_FILE);
    std::fs::write(&p, "ideas").unwrap();
    notes::append(dir.path(), "Song", "## entry one\n- Nothing changed.\n").unwrap();
    assert_eq!(text_of(&p), "ideas\n\n## entry one\n- Nothing changed.\n");
    // Another editor writes in between.
    let mut t = text_of(&p);
    t.push_str("more\n");
    std::fs::write(&p, &t).unwrap();
    notes::append(dir.path(), "Song", "## entry two\n").unwrap();
    assert_eq!(
        text_of(&p),
        "ideas\n\n## entry one\n- Nothing changed.\nmore\n\n## entry two\n"
    );
    std::fs::remove_file(&p).unwrap();
    notes::append(dir.path(), "Song", "## entry three\n").unwrap();
    assert_eq!(text_of(&p), "# Song\n\n## entry three\n");
}

#[cfg(unix)]
#[test]
fn hostile_notes_files() {
    let dir = tempfile::tempdir().unwrap();
    // A link named Notes.md.
    let home = dir.path().join("linked");
    std::fs::create_dir(&home).unwrap();
    let target = dir.path().join("target.txt");
    std::fs::write(&target, "target\n").unwrap();
    std::os::unix::fs::symlink(&target, home.join(NOTES_FILE)).unwrap();
    assert_eq!(notes::read(&home, "S").unwrap_err().to_string(), NOT_PLAIN);
    assert_eq!(
        notes::append(&home, "S", "## x\n").unwrap_err().to_string(),
        NOT_PLAIN
    );
    assert_eq!(text_of(&target), "target\n");
    // A folder named Notes.md.
    let home = dir.path().join("folder");
    std::fs::create_dir_all(home.join(NOTES_FILE)).unwrap();
    assert_eq!(notes::read(&home, "S").unwrap_err().to_string(), NOT_PLAIN);
    assert_eq!(
        notes::append(&home, "S", "## x\n").unwrap_err().to_string(),
        NOT_PLAIN
    );
    // 5 MiB reads 1 MiB.
    let home = dir.path().join("big");
    write(&home.join(NOTES_FILE), &vec![b'a'; 5 << 20]);
    let n = notes::read(&home, "S").unwrap();
    assert!(n.truncated);
    assert_eq!(n.text.len(), MAX_READ);
    assert_eq!(n.path, home.join(NOTES_FILE));
    // Invalid UTF-8 reads lossily.
    let home = dir.path().join("bytes");
    write(&home.join(NOTES_FILE), &[0xff, 0xfe, b'a', b'\n']);
    let n = notes::read(&home, "S").unwrap();
    assert!(!n.truncated);
    assert_eq!(n.text, "\u{FFFD}\u{FFFD}a\n");
}

const EXAMPLE: &str = "## 2026-11-14 15:02 — Set Up a Drum Bus
- Asked: \"put my drums on a bus with some parallel compression\"
- Added: Strip Drums (Co-Producer): added; Strip Drum Crush (Co-Producer): added
- Changed (yes in chat): Track 1, Kick: renamed to \"Kick In\"
- Deleted (yes in chat): Track 3, Old Hats: deleted, with 4 notes
- Changed: Strip Drums (Co-Producer): colour none → blue
- Files: /Users/x/Exports/Night Drive — Stems — 2026-11-14
- Export folder: /Users/x/Exports
- Note: the composer wants the drums dry in the verse
- Undo step: \"Co-Producer: Set Up a Drum Bus\"
- Saved copy: Co-Producer/Copies/2026-11-14 15-02 before Set Up a Drum Bus.vizdaw
";

fn stamped() -> RequestLog {
    RequestLog {
        stamp: Some("2026-11-14 15-02".into()),
        ..RequestLog::default()
    }
}

#[test]
fn the_entry_is_exact() {
    let log = RequestLog {
        stamp: Some("2026-11-14 15-02".into()),
        asked: Some("put my drums on a bus with some parallel compression".into()),
        items: vec![
            (item("Strip Drums (Co-Producer)", "added"), false),
            (item("Strip Drum Crush (Co-Producer)", "added"), false),
            (item("Track 1, Kick", "renamed to \"Kick In\""), true),
            (item("Track 3, Old Hats", "deleted, with 4 notes"), true),
            (
                item("Strip Drums (Co-Producer)", "colour none → blue"),
                false,
            ),
        ],
        labels: vec!["Co-Producer: Set Up a Drum Bus".into()],
        files: vec!["/Users/x/Exports/Night Drive — Stems — 2026-11-14".into()],
        notes: vec!["the composer wants the drums dry in the verse".into()],
        export_folder: Some("/Users/x/Exports".into()),
        copy: Some("Co-Producer/Copies/2026-11-14 15-02 before Set Up a Drum Bus.vizdaw".into()),
    };
    assert_eq!(
        notes::entry(&log, "Set Up a Drum Bus").as_deref(),
        Some(EXAMPLE)
    );

    let mut many = stamped();
    many.items = (1..=25)
        .map(|i| (item(&format!("Strip S{i}"), "added"), false))
        .collect();
    let e = notes::entry(&many, "Many").unwrap();
    let added = e.lines().find(|l| l.starts_with("- Added: ")).unwrap();
    assert_eq!(added.matches(": added").count(), 20);
    assert!(added.ends_with("Strip S20: added; and 5 more"), "{added}");

    assert_eq!(notes::entry(&RequestLog::default(), "Never"), None);
    assert_eq!(
        notes::entry(&stamped(), "Look").as_deref(),
        Some("## 2026-11-14 15:02 — Look\n- Nothing changed.\n")
    );
}

#[test]
fn notes_cannot_forge_an_entry() {
    let mut log = stamped();
    log.notes = vec![
        "## 2026-01-01 00:00 — Fake".into(),
        "two\n## 2026-01-02 00:00 — Fake\nlines".into(),
    ];
    log.asked = Some("x\n# heading".into());
    log.files = vec!["/tmp/a\n## b".into()];
    let e = notes::entry(&log, "Real").unwrap();
    assert!(e.contains("\n- Note: ## 2026-01-01 00:00 — Fake\n"), "{e}");
    let hashed: Vec<&str> = e.lines().filter(|l| l.starts_with('#')).collect();
    assert_eq!(hashed, vec!["## 2026-11-14 15:02 — Real"], "{e}");
}

#[test]
fn export_folders_are_new_and_dated() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    let a = notes::make_export_folder(p, "Night Drive", ExportWhat::Stems, "2026-11-14").unwrap();
    assert_eq!(a, p.join("Night Drive — Stems — 2026-11-14"));
    assert!(a.is_dir());
    let b = notes::make_export_folder(p, "Night Drive", ExportWhat::Stems, "2026-11-14").unwrap();
    assert_eq!(b, p.join("Night Drive — Stems — 2026-11-14 2"));
    let c = notes::make_export_folder(p, "a/b:.c", ExportWhat::OtherDaw, "2026-11-14").unwrap();
    assert_eq!(c, p.join("a-b-.c — Other DAW — 2026-11-14"));
    let d = notes::make_export_folder(p, ".x", ExportWhat::Midi, "2026-11-14").unwrap();
    assert_eq!(d, p.join("x — MIDI — 2026-11-14"));
    let before = std::fs::read_dir(p).unwrap().count();
    for bad in ["2026-11-14/..", "14-11-2026", "x", ""] {
        assert!(
            notes::make_export_folder(p, "S", ExportWhat::Mix, bad).is_err(),
            "{bad}"
        );
    }
    assert_eq!(std::fs::read_dir(p).unwrap().count(), before);
}

#[test]
fn remembered_folder_takes_the_last_line() {
    let t = "# S\n- Export folder: /one\n\n## e\n- Export folder: /two\n- Export folder: rel/x\n  - Export folder: /indented\n";
    assert_eq!(notes::remembered_folder(t), Some(PathBuf::from("/two")));
    assert_eq!(notes::remembered_folder("# S\n"), None);
    assert_eq!(notes::remembered_folder("- Export folder: rel"), None);
}

// ---- move-in ----

fn caf(path: &Path, frames: usize) -> PathBuf {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![0.25; frames * 2]).unwrap();
    w.finish().unwrap();
    path.to_path_buf()
}

fn first_seq(s: &Session) -> u64 {
    s.snapshot().unwrap().sequences[0].id
}

fn import(s: &Arc<Session>, path: &Path) {
    let id = s
        .import_audio(
            vec![path.display().to_string()],
            Some(ImportTargetRow {
                seq: first_seq(s),
                lane: None,
                tick: 0,
            }),
        )
        .unwrap();
    let until = Instant::now() + common::wait_limit();
    loop {
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            assert_eq!(j.state, JobStateRow::Done, "{j:?}");
            return;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn stand_in_of(s: &Session) -> PathBuf {
    let id = s.project_id().unwrap().expect("a project ID");
    vizdaw_ffi::recovery::support_dir()
        .join("Co-Producer")
        .join("Untitled")
        .join(id)
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn first_save_moves_notes_and_copies() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    // An untitled song's stand-in: one copy with its own media.
    let s = Session::new_empty().unwrap();
    import(&s, &caf(&dir.path().join("src/kick.caf"), 200));
    let copy = s.debug_write_co_producer_copy(STAMP, "Tidy").unwrap();
    let stand_in = stand_in_of(&s);
    let name = copy.file_name().unwrap().to_owned();
    write(&stand_in.join(NOTES_FILE), b"# Untitled\nidea\n");
    write(&stand_in.join("PluginState/00000000000000aa.bin"), b"blob");
    s.close().unwrap();
    drop(s);

    let pkg = dir.path().join("P.vizdaw");
    std::fs::create_dir(&pkg).unwrap();
    let lines = notes::move_in(&stand_in, &pkg);
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(text_of(&pkg.join(NOTES_FILE)), "# Untitled\nidea\n");
    assert_eq!(
        std::fs::read(pkg.join("PluginState/00000000000000aa.bin")).unwrap(),
        b"blob"
    );
    let moved = pkg.join("Co-Producer/Copies").join(&name);
    let doc = core_model::file::load_copy(&moved).unwrap();
    let media: Vec<_> = doc.media().values().map(|m| m.path.clone()).collect();
    assert_eq!(media.len(), 1);
    assert!(pkg.join(&media[0]).is_file(), "{}", media[0]);
    assert!(!stand_in.exists(), "the empty stand-in is removed");

    // A package that already has notes keeps them first.
    let stand_in = dir.path().join("stand-in");
    write(&stand_in.join(NOTES_FILE), b"# Untitled\nbefore\n");
    let pkg = dir.path().join("Q.vizdaw");
    write(&pkg.join(NOTES_FILE), b"mine\n");
    let lines = notes::move_in(&stand_in, &pkg);
    assert!(lines.is_empty(), "{lines:?}");
    let t = text_of(&pkg.join(NOTES_FILE));
    assert!(t.starts_with("mine\n"), "{t}");
    let at = t
        .find("## Notes from before the first save")
        .expect("the block");
    let old = t.find("# Untitled\nbefore\n").expect("the untitled text");
    assert!(at < old, "{t}");
    assert!(!stand_in.exists());
}

fn hand_copy(home: &Path, name: &str, body: &str) -> PathBuf {
    let c = home.join("Co-Producer/Copies").join(name);
    write(&c.join("project.json"), body.as_bytes());
    c
}

#[test]
fn a_move_that_cannot_finish_loses_nothing() {
    let dir = tempfile::tempdir().unwrap();
    // A copy of the same name is already in the package.
    let stand_in = dir.path().join("s1");
    let pkg = dir.path().join("P1.vizdaw");
    let mine = hand_copy(&stand_in, "X.vizdaw", "stand-in");
    let theirs = hand_copy(&pkg, "X.vizdaw", "package");
    let lines = notes::move_in(&stand_in, &pkg);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0]
            .starts_with("Co-Producer's copy \"X.vizdaw\" from before the first save stayed in "),
        "{}",
        lines[0]
    );
    assert_eq!(text_of(&theirs.join("project.json")), "package");
    assert_eq!(text_of(&mine.join("project.json")), "stand-in");

    // A package blob with other bytes keeps every copy in the stand-in.
    let stand_in = dir.path().join("s2");
    let pkg = dir.path().join("P2.vizdaw");
    write(&stand_in.join("PluginState/b.bin"), b"one");
    write(&pkg.join("PluginState/b.bin"), b"two");
    let a = hand_copy(&stand_in, "A.vizdaw", "a");
    let b = hand_copy(&stand_in, "B.vizdaw", "b");
    let lines = notes::move_in(&stand_in, &pkg);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("Co-Producer's copies from before the first save stayed in "),
        "{}",
        lines[0]
    );
    assert!(a.join("project.json").is_file() && b.join("project.json").is_file());
    assert!(!pkg.join("Co-Producer/Copies/A.vizdaw").exists());
    assert_eq!(
        std::fs::read(pkg.join("PluginState/b.bin")).unwrap(),
        b"two"
    );
    assert_eq!(
        std::fs::read(stand_in.join("PluginState/b.bin")).unwrap(),
        b"one"
    );
}

#[cfg(unix)]
#[test]
fn a_read_only_package_keeps_the_copies_in_the_stand_in() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let stand_in = dir.path().join("s");
    let pkg = dir.path().join("P.vizdaw");
    let z = hand_copy(&stand_in, "Z.vizdaw", "z");
    let cp = pkg.join("Co-Producer");
    std::fs::create_dir_all(&cp).unwrap();
    std::fs::set_permissions(&cp, std::fs::Permissions::from_mode(0o555)).unwrap();
    let lines = notes::move_in(&stand_in, &pkg);
    std::fs::set_permissions(&cp, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(z.join("project.json").is_file());
    assert!(names_in(&cp).iter().all(|n| !n.contains("moving")));
    assert!(
        names_in(&cp.join("Copies"))
            .iter()
            .all(|n| !n.ends_with(".moving")),
        "nothing half-moved"
    );
    let lines = notes::move_in(&stand_in, &pkg);
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(text_of(&cp.join("Copies/Z.vizdaw/project.json")), "z");
    assert!(!stand_in.exists());
}

// ---- through the session ----

fn lanes(s: &Session) -> Vec<(u64, String)> {
    s.lanes(first_seq(s))
        .unwrap()
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect()
}

fn add_track(s: &Session) -> u64 {
    add_track_of(s, TrackKindRow::Audio)
}

/// A new track of `kind`; an instrument track gets its own strip.
fn add_track_of(s: &Session, kind: TrackKindRow) -> u64 {
    let before: Vec<u64> = lanes(s).into_iter().map(|l| l.0).collect();
    s.new_track(first_seq(s), kind, None).unwrap();
    lanes(s)
        .into_iter()
        .map(|l| l.0)
        .find(|id| !before.contains(id))
        .unwrap()
}

fn hand_rename(s: &Session, lane: u64, name: &str) {
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Rename {
                seq: first_seq(s),
                lane,
                name: name.into(),
            },
        },
        None,
    )
    .unwrap();
}

/// A session saved as `<dir>/<name>.vizdaw` with one hand track.
fn saved(dir: &Path, name: &str) -> (Arc<Session>, PathBuf) {
    let s = Session::new_empty().unwrap();
    add_track(&s);
    let pkg = dir.join(format!("{name}.vizdaw"));
    s.save_as(pkg.display().to_string()).unwrap();
    (s, pkg)
}

/// The last entry of the notes at `pkg`.
fn last_entry(pkg: &Path) -> String {
    let t = text_of(&pkg.join(NOTES_FILE));
    let at = t.rfind("\n## ").expect("an entry");
    t[at + 1..].to_string()
}

fn headings(pkg: &Path) -> usize {
    std::fs::read_to_string(pkg.join(NOTES_FILE))
        .map(|t| t.lines().filter(|l| l.starts_with("## ")).count())
        .unwrap_or(0)
}

#[test]
fn a_request_writes_one_entry() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path(), "Song");
    s.co_producer_begin("add a bus".into()).unwrap();
    s.co_producer_set_asked("give me a reverb bus".into())
        .unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    add_track_of(&s, TrackKindRow::Instrument);
    s.co_producer_leave();
    s.co_producer_note("keep it dark".into()).unwrap();
    s.co_producer_file("/tmp/x.mid".into()).unwrap();
    s.co_producer_end().unwrap();
    assert!(text_of(&pkg.join(NOTES_FILE)).starts_with("# Song\n\n## "));
    let e = last_entry(&pkg);
    let lines: Vec<&str> = e.lines().collect();
    assert_eq!(lines.len(), 7, "{e}");
    assert_eq!(lines[0], "## 2026-11-16 10:05 — Add a Bus");
    assert_eq!(lines[1], "- Asked: \"give me a reverb bus\"");
    let added: Vec<&str> = lines[2]
        .strip_prefix("- Added: ")
        .expect("an Added line")
        .split("; ")
        .collect();
    assert_eq!(added.len(), 2, "{e}");
    assert!(
        added[0].starts_with("Track 2, ") && added[0].ends_with(" (Co-Producer): added"),
        "{e}"
    );
    assert!(
        added[1].starts_with("Strip ") && added[1].ends_with(": added"),
        "{e}"
    );
    assert_eq!(lines[3], "- Files: /tmp/x.mid");
    assert_eq!(lines[4], "- Note: keep it dark");
    assert_eq!(lines[5], "- Undo step: \"Co-Producer: Add a Bus\"");
    assert_eq!(
        lines[6],
        "- Saved copy: Co-Producer/Copies/2026-11-16 10-05 before Add a Bus.vizdaw"
    );
    assert_eq!(headings(&pkg), 1);
    s.close().unwrap();
}

#[test]
fn hand_edits_are_not_logged() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path(), "Song");
    let hand = lanes(&s)[0].0;
    s.co_producer_begin("tidy".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    add_track(&s);
    s.co_producer_leave();
    hand_rename(&s, hand, "Hand Name");
    s.co_producer_enter(STAMP.into()).unwrap();
    add_track(&s);
    s.co_producer_leave();
    s.co_producer_end().unwrap();
    let e = last_entry(&pkg);
    assert!(!e.contains("Hand Name"), "{e}");
    assert!(e.contains("- Added: "), "{e}");
    assert!(e.contains("\"Co-Producer: Tidy\""), "{e}");
    assert!(e.contains("\"Co-Producer: Tidy (continued)\""), "{e}");
    s.close().unwrap();
}

#[test]
fn idle_and_read_only_requests() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path(), "Song");
    s.co_producer_begin("add a track".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    add_track(&s);
    s.co_producer_leave();
    assert!(s.co_producer_idle(0).is_some());
    assert_eq!(headings(&pkg), 1);
    assert!(last_entry(&pkg).starts_with("## 2026-11-16 10:05 — Add a Track\n"));
    // Only read: no entry, by end or idle.
    s.co_producer_begin("look".into()).unwrap();
    s.co_producer_end().unwrap();
    s.co_producer_begin("look again".into()).unwrap();
    assert_eq!(s.co_producer_idle(0), None);
    assert!(s.co_producer_request().is_none());
    assert_eq!(headings(&pkg), 1);
    s.close().unwrap();
}

#[test]
fn untitled_notes_live_in_the_stand_in_and_move_on_save() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_empty().unwrap();
    s.co_producer_begin("add a track".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    add_track(&s);
    s.co_producer_leave();
    s.co_producer_end().unwrap();
    let stand_in = stand_in_of(&s);
    assert_eq!(
        s.notes_path().unwrap(),
        stand_in.join(NOTES_FILE).display().to_string()
    );
    assert!(
        text_of(&stand_in.join(NOTES_FILE))
            .starts_with("# Untitled\n\n## 2026-11-16 10:05 — Add a Track\n")
    );
    assert_eq!(names_in(&stand_in.join("Co-Producer/Copies")).len(), 1);
    let pkg = dir.path().join("Song.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let problems = s.idle().unwrap().problems;
    assert!(problems.is_empty(), "{problems:?}");
    assert!(!stand_in.exists(), "the stand-in is gone");
    assert!(text_of(&pkg.join(NOTES_FILE)).contains("## 2026-11-16 10:05 — Add a Track\n"));
    let copies = s.co_producer_copies().unwrap();
    assert_eq!(copies.len(), 1);
    assert!(PathBuf::from(&copies[0].path).starts_with(&pkg));
    assert_eq!(
        s.notes_path().unwrap(),
        pkg.join(NOTES_FILE).display().to_string()
    );
    s.close().unwrap();
}

#[test]
fn export_folder_uses_parent_then_memory_then_the_song() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path(), "A");
    let parent = dir.path().join("Exports");
    std::fs::create_dir(&parent).unwrap();
    s.co_producer_begin("export stems".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    let p = s
        .co_producer_export_folder(
            ExportWhatRow::Stems,
            DATE.into(),
            Some(parent.display().to_string()),
        )
        .unwrap();
    assert_eq!(PathBuf::from(&p), parent.join("A — Stems — 2026-11-16"));
    assert!(PathBuf::from(&p).is_dir());
    s.co_producer_leave();
    s.co_producer_end().unwrap();
    let e = last_entry(&pkg);
    assert!(
        e.contains(&format!("\n- Export folder: {}\n", parent.display())),
        "{e}"
    );

    // The next request remembers it.
    s.co_producer_begin("export a mix".into()).unwrap();
    let p = s
        .co_producer_export_folder(ExportWhatRow::Mix, DATE.into(), None)
        .unwrap();
    assert_eq!(PathBuf::from(&p), parent.join("A — Mix — 2026-11-16"));
    s.co_producer_end().unwrap();
    s.close().unwrap();

    // Another saved song, no memory: the folder its package is in.
    let songs = dir.path().join("songs");
    std::fs::create_dir(&songs).unwrap();
    let (b, _) = saved(&songs, "B");
    b.co_producer_begin("export stems".into()).unwrap();
    let p = b
        .co_producer_export_folder(ExportWhatRow::Stems, DATE.into(), None)
        .unwrap();
    assert_eq!(PathBuf::from(&p), songs.join("B — Stems — 2026-11-16"));
    b.co_producer_end().unwrap();
    b.close().unwrap();

    // Untitled, no memory: ask.
    let u = Session::new_empty().unwrap();
    u.co_producer_begin("export stems".into()).unwrap();
    let e = u
        .co_producer_export_folder(ExportWhatRow::Stems, DATE.into(), None)
        .unwrap_err();
    assert_eq!(e.to_string(), ASK);
    u.co_producer_end().unwrap();
    // No request.
    let e = u
        .co_producer_export_folder(
            ExportWhatRow::Stems,
            DATE.into(),
            Some(parent.display().to_string()),
        )
        .unwrap_err();
    assert_eq!(e.to_string(), OPEN_FIRST);
    u.close().unwrap();
}

#[cfg(unix)]
#[test]
fn remembered_folders_are_checked_again() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path(), "A");
    let notes_file = PathBuf::from(s.notes_path().unwrap());
    let before = names_in(&pkg);
    std::fs::write(
        &notes_file,
        format!("# A\n- Export folder: {}\n", pkg.display()),
    )
    .unwrap();
    s.co_producer_begin("export".into()).unwrap();
    let e = s
        .co_producer_export_folder(ExportWhatRow::Stems, DATE.into(), None)
        .unwrap_err();
    assert!(
        e.to_string().contains("it is inside a project package"),
        "{e}"
    );
    assert_eq!(names_in(&pkg), before);

    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let mut t = text_of(&notes_file);
    t.push_str(&format!("- Export folder: {}\n", link.display()));
    std::fs::write(&notes_file, t).unwrap();
    let e = s
        .co_producer_export_folder(ExportWhatRow::Stems, DATE.into(), None)
        .unwrap_err();
    assert!(e.to_string().contains("its folder is a link"), "{e}");
    assert!(names_in(&real).is_empty());
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

/// Bytes of every regular file under `dir`.
fn tree_bytes(dir: &Path) -> u64 {
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let m = std::fs::symlink_metadata(e.path()).unwrap();
        if m.is_dir() {
            n += tree_bytes(&e.path());
        } else if m.is_file() {
            n += m.len();
        }
    }
    n
}

/// A saved `P.vizdaw` with two imported tracks, kick and snare, and a
/// copy ("Tidy") written while both were there; then kick's track is
/// deleted, compacted and saved, so only the copy names kick. Reopened.
/// Answers the session, the package and kick's and snare's paths.
fn kick_only_in_a_copy(dir: &Path) -> (Arc<Session>, PathBuf, String, String) {
    let s = Session::new_empty().unwrap();
    import(&s, &caf(&dir.join("src/kick.caf"), 4000));
    import(&s, &caf(&dir.join("src/snare.caf"), 4000));
    let pkg = dir.join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let media = s.media().unwrap();
    let kick = media
        .iter()
        .find(|m| m.path.contains("kick"))
        .unwrap()
        .path
        .clone();
    let snare = media
        .iter()
        .find(|m| m.path.contains("snare"))
        .unwrap()
        .path
        .clone();
    s.debug_write_co_producer_copy("2026-11-14 15-02", "Tidy")
        .unwrap();
    let seq = first_seq(&s);
    // Kick was imported first, onto the first track.
    let lane = s.lanes(seq).unwrap()[0].id;
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Remove { seq, lane },
        },
        None,
    )
    .unwrap();
    s.compact_media().unwrap();
    s.save().unwrap();
    s.close().unwrap();
    drop(s);
    let s = Session::open(pkg.display().to_string()).unwrap();
    (s, pkg, kick, snare)
}

#[test]
fn copies_are_listed_with_sizes() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path(), "Song");
    s.debug_write_co_producer_copy("2026-11-15 09-00", "Two")
        .unwrap();
    s.debug_write_co_producer_copy("2026-11-14 15-02", "One")
        .unwrap();
    let copies = s.co_producer_copies().unwrap();
    let names: Vec<&str> = copies.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "2026-11-14 15-02 before One.vizdaw",
            "2026-11-15 09-00 before Two.vizdaw"
        ]
    );
    for c in &copies {
        let p = PathBuf::from(&c.path);
        assert_eq!(p, pkg.join("Co-Producer/Copies").join(&c.name));
        assert_eq!(c.bytes, tree_bytes(&p), "names no media");
    }
    s.close().unwrap();
    drop(s);

    // A copy that alone names a media file counts it; one whose media
    // the present song uses does not.
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, kick, snare) = kick_only_in_a_copy(dir.path());
    let copies = s.co_producer_copies().unwrap();
    assert_eq!(copies.len(), 1);
    let own = tree_bytes(Path::new(&copies[0].path));
    let kick_bytes = std::fs::metadata(pkg.join(&kick)).unwrap().len();
    assert!(kick_bytes >= 1000);
    assert_eq!(
        copies[0].bytes,
        own + kick_bytes,
        "snare ({snare}) is the song's"
    );
    s.close().unwrap();
}

#[test]
fn copies_are_removed_only_by_name_and_to_the_trash() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, kick, snare) = kick_only_in_a_copy(dir.path());
    let trash = dir.path().join("Trash");
    s.debug_set_trash_dir(Some(trash.display().to_string()));
    s.debug_write_co_producer_copy("2026-11-15 10-00", "Keep")
        .unwrap();
    let copies = s.co_producer_copies().unwrap();
    assert_eq!(copies.len(), 2);
    let (first, keep) = (copies[0].name.clone(), copies[1].name.clone());

    let r = s.remove_co_producer_copies(vec![first.clone()]).unwrap();
    assert_eq!(r.removed, 1);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert!(trash.join(&first).join("project.json").is_file());
    assert!(!pkg.join("Co-Producer/Copies").join(&first).exists());

    let hostile = vec![
        "../Song.vizdaw".to_string(),
        "x/y".to_string(),
        "missing.vizdaw".to_string(),
        first.clone(),
        String::new(),
    ];
    let r = s.remove_co_producer_copies(hostile.clone()).unwrap();
    assert_eq!(r.removed, 0);
    assert_eq!(r.problems.len(), hostile.len(), "{:?}", r.problems);
    for (p, n) in r.problems.iter().zip(&hostile) {
        assert_eq!(p, &format!("Not a copy of this song: {n}."));
    }
    let r = s
        .remove_co_producer_copies(vec![keep.clone(), keep.clone()])
        .unwrap();
    assert_eq!(r.removed, 1);
    assert_eq!(
        r.problems,
        vec![format!("Not a copy of this song: {keep}.")]
    );
    assert!(s.co_producer_copies().unwrap().is_empty());
    assert!(pkg.is_dir() && pkg.join("project.json").is_file());

    // The next Save trashes the media only the removed copy used.
    s.save().unwrap();
    let kick_name = Path::new(&kick).file_name().unwrap();
    assert!(trash.join(kick_name).is_file(), "{:?}", names_in(&trash));
    assert!(!pkg.join(&kick).exists());
    assert!(pkg.join(&snare).is_file());
    s.close().unwrap();
}

#[test]
fn new_session_calls_refuse_hostile_input() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path(), "Song");
    let before = names_in(dir.path());
    let pkg_before = names_in(&pkg);
    // No request yet.
    assert_eq!(
        s.co_producer_note("x".into()).unwrap_err().to_string(),
        OPEN_FIRST
    );
    assert_eq!(
        s.co_producer_file("/tmp/x".into()).unwrap_err().to_string(),
        OPEN_FIRST
    );
    assert_eq!(
        s.co_producer_set_asked("x".into()).unwrap_err().to_string(),
        OPEN_FIRST
    );
    s.co_producer_begin("look".into()).unwrap();
    let long = format!("/{}", "a".repeat(70_000));
    for bad in ["", "relative/x.mid", long.as_str(), "/tmp/a\0b.mid"] {
        assert!(s.co_producer_file(bad.into()).is_err(), "{}", bad.len());
    }
    assert!(s.co_producer_note("n".repeat(2001)).is_err());
    assert!(s.co_producer_note("   ".into()).is_err());
    assert!(s.co_producer_note("a\0b".into()).is_err());
    assert!(s.co_producer_set_asked("q".repeat(70_000)).is_err());
    assert!(
        s.co_producer_export_folder(ExportWhatRow::Stems, "x".into(), None)
            .is_err()
    );
    assert!(
        s.co_producer_export_folder(ExportWhatRow::Stems, DATE.into(), Some("relative".into()))
            .is_err()
    );
    assert!(
        s.remove_co_producer_copies(vec![long.clone(), "\0".into()])
            .is_ok()
    );
    s.co_producer_end().unwrap();
    assert_eq!(names_in(dir.path()), before);
    assert_eq!(names_in(&pkg), pkg_before, "no file written");
    s.close().unwrap();
}

#[test]
fn a_failed_entry_is_a_problem_line() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path(), "Song");
    std::fs::create_dir(pkg.join(NOTES_FILE)).unwrap();
    s.idle().unwrap();
    s.co_producer_begin("add a track".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    add_track(&s);
    s.co_producer_leave();
    assert_eq!(
        s.co_producer_end().unwrap().as_deref(),
        Some("Co-Producer: Add a Track")
    );
    assert!(s.co_producer_request().is_none(), "ended as usual");
    let problems = s.idle().unwrap().problems;
    assert_eq!(
        problems,
        vec![
            "Song notes not written: Notes.md is not a plain file, so Co-Producer leaves it alone."
        ]
    );
    assert!(pkg.join(NOTES_FILE).is_dir());
    s.close().unwrap();
}

#[test]
fn notes_and_folder_results_as_json() {
    use vizdaw_ffi::control::result_json;
    use vizdaw_ffi::{ControlResult, NotesRow};
    let r = ControlResult::CoProducerNotes {
        doc: "d1".into(),
        notes: NotesRow {
            text: "# S\n".into(),
            truncated: true,
            path: "/x/S.vizdaw/Notes.md".into(),
        },
    };
    assert_eq!(
        result_json(&r),
        serde_json::json!({"text": "# S\n", "truncated": true, "path": "/x/S.vizdaw/Notes.md"})
    );
    let r = ControlResult::CoProducerFolder {
        doc: "d1".into(),
        path: "/x/S — Stems — 2026-11-16".into(),
    };
    assert_eq!(
        result_json(&r),
        serde_json::json!({"path": "/x/S — Stems — 2026-11-16"})
    );
}
