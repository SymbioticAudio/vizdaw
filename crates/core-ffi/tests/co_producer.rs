//! Co-Producer requests, previews and one-time tokens through the session
//! (plan 8a Task 4; Rulings 3, 6, 7 and 9): a request is one undo step
//! with one saved copy, a preview changes nothing and lists exactly what
//! will change, and a token is taken once, for the same song and the same
//! commands only. Every input is hostile.

// `im` maps nest deeply: sending a `Session` to a thread needs more depth.
#![recursion_limit = "256"]

use core_model::mpe::{MpeConfig, MpeZone};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;
use vizdaw_ffi::co_producer::{ChangeItemRow, CoProducerApproval, PlannedRow, PreviewRow};
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneEdit};
use vizdaw_ffi::host::{force_offline, hosts_created, reset_host};
use vizdaw_ffi::record::set_free_bytes;
use vizdaw_ffi::tracks::TrackKindRow;
use vizdaw_ffi::{EventKindRow, EventRow, Session, VizError};

const STAMP: &str = "2026-11-14 15-02";
const OPEN_FIRST: &str = "Open a request first (begin_request).";
const USED: &str = "This preview was already used or replaced.";
const CHANGED: &str = "The song changed since the preview; preview again.";
const NOT_PREVIEWED: &str = "These are not the previewed commands.";
const ENTER_FIRST: &str = "Take the preview inside the Co-Producer call: co_producer_enter first.";

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    set_free_bytes(None);
    g
}

fn first_seq(s: &Session) -> u64 {
    s.snapshot().unwrap().sequences[0].id
}

fn lanes(s: &Session) -> Vec<(u64, String)> {
    s.lanes(first_seq(s))
        .unwrap()
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect()
}

fn undo_label(s: &Session) -> Option<String> {
    s.snapshot().unwrap().undo_label
}

fn add_track(s: &Session, kind: TrackKindRow) -> u64 {
    let before: Vec<u64> = lanes(s).into_iter().map(|l| l.0).collect();
    s.new_track(first_seq(s), kind, None).unwrap();
    lanes(s)
        .into_iter()
        .map(|l| l.0)
        .find(|id| !before.contains(id))
        .unwrap()
}

fn add_notes(s: &Session, lane: u64, n: i64) {
    let events = (0..n)
        .map(|i| {
            let mut row = EventRow::blank(0, i * 960, EventKindRow::Note);
            row.pitch = 60;
            row.velocity = 0x8000;
            row.duration = 480;
            row
        })
        .collect();
    s.edit(
        EditCommand::Event {
            edit: EventEdit::Insert {
                seq: first_seq(s),
                lane,
                events,
            },
        },
        None,
    )
    .unwrap();
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

fn int(name: &str, value: u64) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Int {
            value: value as i64,
        },
    }
}

fn text(name: &str, value: &str) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Text {
            value: value.into(),
        },
    }
}

fn planned(id: &str, args: Vec<ControlArg>) -> PlannedRow {
    PlannedRow {
        id: id.into(),
        args,
    }
}

fn rename(seq: u64, lane: u64, name: &str) -> PlannedRow {
    planned(
        "track.rename",
        vec![int("seq", seq), int("lane", lane), text("name", name)],
    )
}

fn delete(seq: u64, lane: u64) -> PlannedRow {
    planned("track.delete", vec![int("seq", seq), int("lane", lane)])
}

fn undo() -> PlannedRow {
    planned("edit.undo", vec![])
}

fn redo() -> PlannedRow {
    planned("edit.redo", vec![])
}

fn message(e: VizError) -> String {
    e.to_string()
}

/// A session saved as `<dir>/P.vizdaw` with two audio tracks.
fn saved(dir: &Path) -> (Arc<Session>, PathBuf) {
    let s = Session::new_empty().unwrap();
    add_track(&s, TrackKindRow::Audio);
    add_track(&s, TrackKindRow::Audio);
    let pkg = dir.join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    (s, pkg)
}

/// Every file under `dir` with its length and modification time.
fn disk(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let m = std::fs::symlink_metadata(e.path()).unwrap();
            if m.is_dir() {
                todo.push(e.path());
            } else {
                out.push((e.path(), m.len(), m.modified().unwrap()));
            }
        }
    }
    out.sort();
    out
}

/// One Co-Producer step through the session: begin, enter, `work`, leave,
/// end. Returns the label it made.
fn co_producer(s: &Session, name: &str, work: impl FnOnce(&Session)) -> Option<String> {
    s.co_producer_begin(name.into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    work(s);
    s.co_producer_leave();
    s.co_producer_end().unwrap()
}

#[test]
fn a_request_through_the_session_is_one_step_with_a_copy() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_empty().unwrap();
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let r = s.co_producer_begin("add a track".into()).unwrap();
    assert_eq!(r.label, "Co-Producer: Add a Track");
    assert!(!r.group_open && !r.continued);
    let copy = s.co_producer_enter(STAMP.into()).unwrap().expect("a copy");
    assert!(Path::new(&copy).join("project.json").is_file(), "{copy}");
    assert_eq!(
        PathBuf::from(&copy),
        pkg.join("Co-Producer/Copies/2026-11-14 15-02 before Add a Track.vizdaw")
    );
    s.new_track(first_seq(&s), TrackKindRow::Audio, None)
        .unwrap();
    s.co_producer_leave();
    assert_eq!(s.co_producer_enter(STAMP.into()).unwrap(), None);
    s.co_producer_leave();
    assert_eq!(core_model::file::list_copies(&pkg).unwrap().len(), 1);
    assert_eq!(
        s.co_producer_end().unwrap().as_deref(),
        Some("Co-Producer: Add a Track")
    );
    assert_eq!(s.co_producer_request().map(|r| r.name), None);
    assert_eq!(undo_label(&s).as_deref(), Some("Co-Producer: Add a Track"));
    s.undo().unwrap();
    assert!(lanes(&s).is_empty());
    s.close().unwrap();
}

#[test]
fn enter_needs_a_request() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path());
    assert_eq!(
        message(s.co_producer_enter(STAMP.into()).unwrap_err()),
        OPEN_FIRST
    );
    assert!(core_model::file::list_copies(&pkg).unwrap().is_empty());
    // A hostile stamp is refused before anything is written.
    s.co_producer_begin("tidy".into()).unwrap();
    for stamp in ["", "../x", "2026-11-14 15-02\0", &"9".repeat(70_000)] {
        assert!(s.co_producer_enter(stamp.into()).is_err());
    }
    assert!(core_model::file::list_copies(&pkg).unwrap().is_empty());
    s.close().unwrap();
}

#[test]
fn enter_is_busy_during_a_hand_edit() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path());
    s.co_producer_begin("tidy".into()).unwrap();
    s.begin_edit("Move".into()).unwrap();
    match s.co_producer_enter(STAMP.into()) {
        Err(VizError::Busy { message }) => assert_eq!(message, "Finish the drag first."),
        r => panic!("{r:?}"),
    }
    assert!(core_model::file::list_copies(&pkg).unwrap().is_empty());
    s.end_edit().unwrap();
    s.close().unwrap();
}

#[test]
fn a_failed_copy_changes_nothing() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path());
    std::fs::write(pkg.join("Co-Producer"), b"not a folder").unwrap();
    s.co_producer_begin("tidy".into()).unwrap();
    let e = message(s.co_producer_enter(STAMP.into()).unwrap_err());
    assert!(
        e.starts_with("The saved copy could not be written (")
            && e.ends_with("), so Co-Producer changed nothing."),
        "{e}"
    );
    // Acting stayed off: the next hand edit is an ordinary step.
    let lane = lanes(&s)[0].0;
    hand_rename(&s, lane, "Kick");
    assert_eq!(undo_label(&s).as_deref(), Some("Rename Track"));
    assert_eq!(s.co_producer_end().unwrap(), None);
    s.close().unwrap();
}

#[test]
fn a_busy_capture_refuses_enter() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    // Untitled: no project ID is stamped.
    let s = Session::new_empty().unwrap();
    s.co_producer_begin("tidy".into()).unwrap();
    s.debug_capture_busy(true);
    assert!(matches!(
        s.co_producer_enter(STAMP.into()),
        Err(VizError::Busy { .. })
    ));
    assert_eq!(s.project_id().unwrap(), None);
    s.debug_capture_busy(false);
    s.close().unwrap();
    // Titled: no copy, and acting stays off.
    let (s, pkg) = saved(dir.path());
    s.co_producer_begin("tidy".into()).unwrap();
    s.debug_capture_busy(true);
    assert!(matches!(
        s.co_producer_enter(STAMP.into()),
        Err(VizError::Busy { .. })
    ));
    s.debug_capture_busy(false);
    assert!(core_model::file::list_copies(&pkg).unwrap().is_empty());
    hand_rename(&s, lanes(&s)[0].0, "Kick");
    assert_eq!(undo_label(&s).as_deref(), Some("Rename Track"));
    s.close().unwrap();
}

#[test]
fn work_owed_to_the_composer_lands_before_co_producer_acts() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = saved(dir.path());
    let lane = lanes(&s)[0].0;
    let mpe = MpeConfig {
        zone: MpeZone::Lower,
        members: 15,
        bend_range: 48,
    };
    s.debug_owe_mpe(lane, Some(mpe));
    let label = co_producer(&s, "add a track", |s| {
        s.new_track(first_seq(s), TrackKindRow::Audio, None)
            .unwrap();
    });
    assert_eq!(label.as_deref(), Some("Co-Producer: Add a Track"));
    s.undo().unwrap();
    // The composer's MPE setting is its own step, not Co-Producer's.
    assert_eq!(undo_label(&s).as_deref(), Some("Set Track MPE"));
    s.close().unwrap();
}

#[test]
fn idle_ends_the_request() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = saved(dir.path());
    s.co_producer_begin("add a track".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    s.new_track(first_seq(&s), TrackKindRow::Audio, None)
        .unwrap();
    // Not while Co-Producer is acting.
    assert_eq!(s.co_producer_idle(0), None);
    s.co_producer_leave();
    assert_eq!(
        s.co_producer_idle(0).as_deref(),
        Some("Co-Producer: Add a Track")
    );
    assert!(s.co_producer_request().is_none());
    assert_eq!(undo_label(&s).as_deref(), Some("Co-Producer: Add a Track"));
    s.co_producer_begin("tidy".into()).unwrap();
    assert_eq!(s.co_producer_idle(300_000), None);
    assert!(s.co_producer_request().is_some());
    s.close().unwrap();
}

/// What a preview must leave as it was.
#[derive(Debug, PartialEq)]
struct Untouched {
    doc: String,
    undo: Option<String>,
    version: u64,
    disk: Vec<(PathBuf, u64, SystemTime)>,
    hosts: u64,
}

fn untouched(s: &Session, pkg: &Path) -> Untouched {
    let snap = s.snapshot().unwrap();
    Untouched {
        doc: s.debug_document_json(),
        undo: snap.undo_label,
        version: snap.version,
        disk: disk(pkg),
        hosts: hosts_created(),
    }
}

fn preview(s: &Session, pkg: &Path, plan: Vec<PlannedRow>) -> PreviewRow {
    let before = untouched(s, pkg);
    let p = s.co_producer_preview(plan).unwrap();
    assert_eq!(untouched(s, pkg), before, "the preview changed something");
    assert_eq!(p.token.len(), 32);
    assert!(
        p.token
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    p
}

fn row(thing: &str, change: &str, composers_work: bool) -> ChangeItemRow {
    ChangeItemRow {
        thing: thing.into(),
        change: change.into(),
        composers_work,
    }
}

#[test]
fn previews_list_exactly_what_changes() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path());
    let seq = first_seq(&s);
    let (vox, old) = lanes(&s)[1].clone();
    add_notes(&s, vox, 4);

    let p = preview(&s, &pkg, vec![rename(seq, vox, "Vox")]);
    assert_eq!(
        p.items,
        vec![row(&format!("Track 2, {old}"), "renamed to \"Vox\"", true)]
    );
    assert!(p.needs_yes);

    let p = preview(&s, &pkg, vec![delete(seq, vox)]);
    assert_eq!(p.items[0].thing, format!("Track 2, {old}"));
    assert_eq!(p.items[0].change, "deleted, with 4 notes");
    assert!(p.needs_yes);

    let p = preview(&s, &pkg, vec![planned("file.save", vec![])]);
    assert_eq!(
        p.items,
        vec![row("P.vizdaw", "saved over the file on disk", true)]
    );

    // A combined plan works through the clone in order.
    let p = preview(&s, &pkg, vec![rename(seq, vox, "Vox"), undo()]);
    assert_eq!(p.items.len(), 3);
    assert_eq!(p.items[1], row("Undo", "\"Rename Track\"", true));

    // An Undo of Co-Producer's own step needs no yes.
    let label = co_producer(&s, "add a track", |s| {
        s.new_track(first_seq(s), TrackKindRow::Audio, None)
            .unwrap();
    });
    assert_eq!(label.as_deref(), Some("Co-Producer: Add a Track"));
    let p = preview(&s, &pkg, vec![undo()]);
    assert!(!p.needs_yes, "{p:?}");
    assert_eq!(
        p.items[0],
        row("Undo", "\"Co-Producer: Add a Track\"", false)
    );
    assert!(p.items.iter().any(|i| i.change == "deleted"), "{p:?}");

    // Its Redo adds only: no yes either.
    s.undo().unwrap();
    let p = preview(&s, &pkg, vec![redo()]);
    assert!(!p.needs_yes, "{p:?}");
    assert!(p.items.iter().skip(1).all(|i| i.change == "added"), "{p:?}");

    // An Undo of the composer's own step needs a yes.
    hand_rename(&s, vox, "Lead");
    let p = preview(&s, &pkg, vec![undo()]);
    assert!(p.needs_yes);
    assert_eq!(p.items[0], row("Undo", "\"Rename Track\"", true));

    // A Redo of a Co-Producer step that deleted a track needs a yes.
    co_producer(&s, "tidy up", |s| {
        s.edit(
            EditCommand::Lane {
                edit: LaneEdit::Remove { seq, lane: vox },
            },
            None,
        )
        .unwrap();
    });
    s.undo().unwrap();
    let p = preview(&s, &pkg, vec![redo()]);
    assert!(p.needs_yes, "{p:?}");

    // Nothing to undo, an unknown track and a non-previewable command are
    // refused, changing nothing.
    let before = untouched(&s, &pkg);
    assert!(s.co_producer_preview(vec![delete(seq, 999_999)]).is_err());
    assert!(
        s.co_producer_preview(vec![planned("track.new_audio", vec![])])
            .is_err()
    );
    assert!(s.co_producer_preview(vec![]).is_err());
    assert_eq!(untouched(&s, &pkg), before);
    s.close().unwrap();

    // Save on an untitled song is refused.
    let u = Session::new_empty().unwrap();
    let e = u
        .co_producer_preview(vec![planned("file.save", vec![])])
        .unwrap_err();
    assert_eq!(
        message(e),
        "The song has no file yet: ask the composer to save it (File › Save)."
    );
    let e = u.co_producer_preview(vec![undo()]).unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
    u.close().unwrap();
}

/// A take as the app makes it: inside the Co-Producer call (Task 4 fix
/// round 1: enter, take, the plan, leave).
fn take(s: &Session, token: String, plan: Vec<PlannedRow>) -> Result<(), VizError> {
    s.co_producer_enter(STAMP.into()).unwrap();
    let r = s.co_producer_take(token, plan, CoProducerApproval::Sheet);
    s.co_producer_leave();
    r
}

#[test]
fn take_needs_enter() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = saved(dir.path());
    let plan = || vec![rename(first_seq(&s), lanes(&s)[0].0, "Vox")];
    s.co_producer_begin("tidy up".into()).unwrap();
    let p = s.co_producer_preview(plan()).unwrap();
    assert_eq!(
        message(
            s.co_producer_take(p.token, plan(), CoProducerApproval::Sheet)
                .unwrap_err()
        ),
        ENTER_FIRST
    );
    // The token went with it.
    let p = s.co_producer_preview(plan()).unwrap();
    take(&s, p.token, plan()).unwrap();
    s.close().unwrap();
}

/// A plug-in gesture the composer finishes while Co-Producer runs an
/// approved Undo stays the composer's own step, and the Undo is refused
/// (Task 4 fix round 1, review I1).
#[test]
fn a_gesture_during_co_producers_undo_stays_the_composers() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = saved(dir.path());
    let master = s.mixer().unwrap().strips.last().unwrap().id;
    s.add_insert(
        master,
        None,
        vizdaw_ffi::PluginRefRow {
            format: vizdaw_ffi::PluginFormatTag::Clap,
            id: "test.gain".into(),
            name: "Gain".into(),
            vendor: "VizDAW".into(),
            version: "1.0.0".into(),
        },
    )
    .unwrap();
    let slot = s.plugin_slots().unwrap()[0].id;
    let tracks = lanes(&s).len();
    co_producer(&s, "add a track", |s| {
        s.new_track(first_seq(s), TrackKindRow::Audio, None)
            .unwrap();
    });
    let p = s.co_producer_preview(vec![undo()]).unwrap();
    assert!(!p.needs_yes);
    s.co_producer_begin("undo that".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    s.co_producer_take(p.token, vec![undo()], CoProducerApproval::Sheet)
        .unwrap();
    // The gesture arrives after the take, before the Undo runs.
    s.debug_owe_gesture(slot, 0, 0.25);
    assert_eq!(message(s.undo().unwrap_err()), CHANGED);
    s.co_producer_leave();
    assert_eq!(s.co_producer_end().unwrap(), None);
    // Co-Producer's track is still there, and the gesture is the
    // composer's own step on top.
    assert_eq!(lanes(&s).len(), tracks + 1);
    assert_eq!(undo_label(&s).as_deref(), Some("Change Plugin Parameter"));
    s.undo().unwrap();
    assert_eq!(undo_label(&s).as_deref(), Some("Co-Producer: Add a Track"));
    s.close().unwrap();
}

#[test]
fn take_refuses_every_stale_or_altered_yes() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = saved(dir.path());
    let seq = first_seq(&s);
    let (a, b) = (lanes(&s)[0].0, lanes(&s)[1].0);
    let plan = || vec![rename(seq, a, "Vox"), delete(seq, b)];
    s.co_producer_begin("tidy up".into()).unwrap();
    let refused = |s: &Session, token: String, plan: Vec<PlannedRow>, why: &str| {
        let doc = s.debug_document_json();
        assert_eq!(message(take(s, token, plan).unwrap_err()), why);
        assert_eq!(s.debug_document_json(), doc);
    };

    // After a hand edit.
    let p = s.co_producer_preview(plan()).unwrap();
    hand_rename(&s, a, "Kick");
    refused(&s, p.token, plan(), CHANGED);

    // A second take of the same token.
    let p = s.co_producer_preview(plan()).unwrap();
    take(&s, p.token.clone(), plan()).unwrap();
    refused(&s, p.token, plan(), USED);

    // The plan's name changed.
    let p = s.co_producer_preview(plan()).unwrap();
    refused(
        &s,
        p.token,
        vec![rename(seq, a, "Bass"), delete(seq, b)],
        NOT_PREVIEWED,
    );

    // The commands swapped.
    let p = s.co_producer_preview(plan()).unwrap();
    refused(
        &s,
        p.token,
        vec![delete(seq, b), rename(seq, a, "Vox")],
        NOT_PREVIEWED,
    );

    // The first token after a second preview.
    let first = s.co_producer_preview(plan()).unwrap();
    let _second = s.co_producer_preview(plan()).unwrap();
    refused(&s, first.token, plan(), USED);

    // Another session's token.
    let other = Session::new_empty().unwrap();
    add_track(&other, TrackKindRow::Audio);
    let theirs = other
        .co_producer_preview(vec![rename(first_seq(&other), lanes(&other)[0].0, "Vox")])
        .unwrap();
    let _ours = s.co_producer_preview(plan()).unwrap();
    refused(&s, theirs.token, plan(), USED);
    other.close().unwrap();

    // Hostile tokens and plans.
    let p = s.co_producer_preview(plan()).unwrap();
    refused(&s, "x".repeat(70_000), plan(), USED);
    let p2 = s.co_producer_preview(plan()).unwrap();
    assert_ne!(p.token, p2.token);
    refused(&s, format!("{}\0", p2.token), plan(), USED);
    let p = s.co_producer_preview(plan()).unwrap();
    refused(
        &s,
        p.token,
        vec![planned("track.rename", vec![int("seq", seq)])],
        NOT_PREVIEWED,
    );

    // A fresh preview, then a take with the same plan, succeeds once.
    let p = s.co_producer_preview(plan()).unwrap();
    take(&s, p.token.clone(), plan()).unwrap();
    refused(&s, p.token, plan(), USED);
    s.close().unwrap();
}

#[test]
fn hostile_request_names_are_refused() {
    let _g = serial();
    let s = Session::new_empty().unwrap();
    for name in [
        String::new(),
        "  ".into(),
        "a\0b".into(),
        "a\u{202E}b".into(),
        "a\u{2028}b".into(),
        "x".repeat(70_000),
    ] {
        assert!(s.co_producer_begin(name.clone()).is_err(), "{name:?}");
        assert!(s.co_producer_request().is_none());
    }
    // One request at a time.
    s.co_producer_begin("tidy".into()).unwrap();
    assert!(s.co_producer_begin("more".into()).is_err());
    assert_eq!(s.co_producer_request().unwrap().name, "tidy");
    s.close().unwrap();
}

#[test]
fn hostile_plans_are_refused() {
    let _g = serial();
    let s = Session::new_empty().unwrap();
    add_track(&s, TrackKindRow::Audio);
    let seq = first_seq(&s);
    let lane = lanes(&s)[0].0;
    let doc = s.debug_document_json();
    let bad: Vec<Vec<PlannedRow>> = vec![
        vec![],
        (0..33).map(|_| undo()).collect(),
        vec![planned("no.such", vec![])],
        vec![planned(
            "track.rename",
            vec![int("seq", seq), int("lane", lane)],
        )],
        vec![planned(
            "track.rename",
            vec![int("seq", seq), int("lane", lane), int("name", 3)],
        )],
        vec![planned(
            "track.rename",
            vec![
                int("seq", seq),
                int("lane", lane),
                text("name", "a"),
                text("name", "b"),
            ],
        )],
        vec![planned(
            "track.rename",
            vec![
                int("seq", seq),
                int("lane", lane),
                text("name", "a"),
                text("x", "y"),
            ],
        )],
        vec![planned("track.delete", vec![int("lane", lane)])],
        vec![planned(
            "track.delete",
            vec![
                int("seq", seq),
                ControlArg {
                    name: "lane".into(),
                    value: ControlValue::Int { value: -1 },
                },
            ],
        )],
        vec![rename(seq, lane, "a\0b")],
        vec![rename(seq, lane, &"x".repeat(70_000))],
        vec![planned("file.save", vec![text("path", "/tmp/x.vizdaw")])],
        vec![planned(
            "track.delete",
            vec![int("seq", seq), int("lane", u64::MAX)],
        )],
        vec![planned("file.save_as", vec![text("path", "/tmp/x.vizdaw")])],
        vec![planned("track.new_audio", vec![])],
    ];
    for plan in bad {
        let e = s.co_producer_preview(plan.clone());
        assert!(e.is_err(), "{plan:?}: {e:?}");
        assert_eq!(s.debug_document_json(), doc);
    }
    s.close().unwrap();
}

#[test]
fn another_threads_edit_is_a_hand_edit() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = saved(dir.path());
    let lane = lanes(&s)[0].0;
    s.co_producer_begin("add a track".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    // While Co-Producer acts on this thread, an edit from another is the
    // composer's own: never part of the "Co-Producer: …" step.
    let other = Arc::clone(&s);
    std::thread::spawn(move || hand_rename(&other, lane, "Kick"))
        .join()
        .unwrap();
    s.new_track(first_seq(&s), TrackKindRow::Audio, None)
        .unwrap();
    s.co_producer_leave();
    assert_eq!(
        s.co_producer_end().unwrap().as_deref(),
        Some("Co-Producer: Add a Track")
    );
    s.undo().unwrap();
    assert_eq!(undo_label(&s).as_deref(), Some("Rename Track"));
    assert_eq!(lanes(&s)[0].1, "Kick");
    s.close().unwrap();
}

/// Plan 8b Ruling 11's sentence: the Rust backstop.
const NEEDS_VIZDAW: &str = "VizDAW's own approval is needed for this change.";

/// A take as the app makes it, with `approval`.
fn take_with(
    s: &Session,
    token: String,
    plan: Vec<PlannedRow>,
    approval: CoProducerApproval,
) -> Result<(), VizError> {
    s.co_producer_enter(STAMP.into()).unwrap();
    let r = s.co_producer_take(token, plan, approval);
    s.co_producer_leave();
    r
}

/// Plan 8b Task 6 (Ruling 11): a take whose preview needs the composer's
/// yes needs VizDAW's own approval too, and a refused one uses the token.
#[test]
fn take_needs_vizdaw_approval_for_a_yes() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = saved(dir.path());
    let seq = first_seq(&s);
    let (hand, name) = lanes(&s)[0].clone();
    let plan = || vec![rename(seq, hand, "Kick")];
    s.co_producer_begin("tidy up".into()).unwrap();
    let p = s.co_producer_preview(plan()).unwrap();
    assert!(p.needs_yes);
    let doc = s.debug_document_json();
    assert_eq!(
        message(take_with(&s, p.token.clone(), plan(), CoProducerApproval::None).unwrap_err()),
        NEEDS_VIZDAW
    );
    assert_eq!(s.debug_document_json(), doc, "nothing changed");
    assert_eq!(lanes(&s)[0].1, name);
    // That refusal used the token up.
    assert_eq!(
        message(take_with(&s, p.token, plan(), CoProducerApproval::Sheet).unwrap_err()),
        USED
    );
    // A fresh preview with the sheet's Allow, and one with a standing
    // approval, both take.
    let p = s.co_producer_preview(plan()).unwrap();
    take_with(&s, p.token, plan(), CoProducerApproval::Sheet).unwrap();
    let back = || vec![rename(seq, hand, "Snare")];
    let p = s.co_producer_preview(back()).unwrap();
    assert!(p.needs_yes);
    take_with(&s, p.token, back(), CoProducerApproval::Standing).unwrap();
    s.co_producer_end().unwrap();

    // Co-Producer's own track needs no yes, so no approval either.
    let keys = co_producer_track(&s);
    let own = || vec![rename(seq, keys, "Pad")];
    let p = s.co_producer_preview(own()).unwrap();
    assert!(!p.needs_yes, "{p:?}");
    take_with(&s, p.token, own(), CoProducerApproval::None).unwrap();
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

/// Ruling 11: `co_producer_pending` checks a token as a take does, but
/// takes nothing, and answers the preview's row.
#[test]
fn pending_checks_without_taking() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = saved(dir.path());
    let seq = first_seq(&s);
    let (a, b) = (lanes(&s)[0].0, lanes(&s)[1].0);
    let plan = || vec![rename(seq, a, "Vox"), delete(seq, b)];
    s.co_producer_begin("tidy up".into()).unwrap();
    let p = s.co_producer_preview(plan()).unwrap();
    let doc = s.debug_document_json();
    assert_eq!(s.co_producer_pending(p.token.clone(), plan()).unwrap(), p);
    assert_eq!(s.co_producer_pending(p.token.clone(), plan()).unwrap(), p);
    assert_eq!(s.debug_document_json(), doc, "a peek changes nothing");

    // Refusals, none of which uses the good token up.
    let wrong = "0".repeat(32);
    assert_eq!(
        message(s.co_producer_pending(wrong, plan()).unwrap_err()),
        USED
    );
    assert_eq!(
        message(
            s.co_producer_pending("x".repeat(70_000), plan())
                .unwrap_err()
        ),
        USED
    );
    assert_eq!(
        message(
            s.co_producer_pending(
                p.token.clone(),
                vec![rename(seq, a, "Bass"), delete(seq, b)]
            )
            .unwrap_err()
        ),
        NOT_PREVIEWED
    );
    assert_eq!(
        message(s.co_producer_pending(p.token.clone(), vec![]).unwrap_err()),
        NOT_PREVIEWED
    );
    take_with(&s, p.token.clone(), plan(), CoProducerApproval::Sheet).unwrap();
    // A used token.
    assert_eq!(
        message(s.co_producer_pending(p.token, plan()).unwrap_err()),
        USED
    );

    // A hand edit in between.
    let c = add_track(&s, TrackKindRow::Audio);
    let again = || vec![rename(seq, c, "Bass")];
    let p = s.co_producer_preview(again()).unwrap();
    hand_rename(&s, a, "Kick");
    assert_eq!(
        message(s.co_producer_pending(p.token.clone(), again()).unwrap_err()),
        CHANGED
    );
    // A good token after the refusals still takes.
    let p = s.co_producer_preview(again()).unwrap();
    let _ = s.co_producer_pending("1".repeat(32), again());
    let _ = s.co_producer_pending(p.token.clone(), vec![rename(seq, c, "Other")]);
    s.co_producer_pending(p.token.clone(), again()).unwrap();
    take_with(&s, p.token, again(), CoProducerApproval::Sheet).unwrap();

    // VizDAW's no drops the token (Ruling 11); another token drops nothing.
    let p = s.co_producer_preview(again()).unwrap();
    s.co_producer_forget("2".repeat(32));
    s.co_producer_forget("x".repeat(70_000));
    s.co_producer_pending(p.token.clone(), again()).unwrap();
    s.co_producer_forget(p.token.clone());
    assert_eq!(
        message(s.co_producer_pending(p.token.clone(), again()).unwrap_err()),
        USED
    );
    assert_eq!(
        message(take_with(&s, p.token, again(), CoProducerApproval::Sheet).unwrap_err()),
        USED
    );
    s.co_producer_end().unwrap();

    // With no open request there is nothing to wait for.
    let p = s.co_producer_preview(again()).unwrap();
    assert_eq!(
        message(s.co_producer_pending(p.token, again()).unwrap_err()),
        OPEN_FIRST
    );
    s.close().unwrap();
}

mod control {
    //! The five `co_producer.*` calls through the FFI (plan 8a Task 4):
    //! typed requests in, typed results out as the reply's JSON, and the
    //! Co-Producer socket's own server.

    use super::{rename, row};
    use core_control::client::{CallError, Client};
    use core_control::co_producer::Planned;
    use core_control::params::WindowRef;
    use core_control::registry::ArgValue;
    use core_control::request::{Command, ExportWhat};
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    use vizdaw_ffi::co_producer::{CoProducerRequestRow, PreviewRow};
    use vizdaw_ffi::control::{request_of, result_json};
    use vizdaw_ffi::{
        ControlErrorCode, ControlFailure, ControlRequest, ControlResult, ControlServer,
        ExportWhatRow, WindowRefRow,
    };

    fn plan() -> Vec<Planned> {
        vec![Planned {
            id: "track.rename".into(),
            args: vec![
                ("seq".into(), ArgValue::Int(1)),
                ("lane".into(), ArgValue::Int(2)),
                ("name".into(), ArgValue::Text("Vox".into())),
            ],
        }]
    }

    fn of(c: Command) -> ControlRequest {
        request_of(c, || unreachable!()).expect("a request for the app")
    }

    #[test]
    fn co_producer_calls_reach_the_app_as_rows() {
        let key = Some(WindowRefRow::Key);
        assert_eq!(
            of(Command::CoProducerBegin {
                name: "tidy".into(),
                asked: Some("tidy my drums".into()),
                window: Some(WindowRef::Key),
            }),
            ControlRequest::CoProducerBegin {
                name: "tidy".into(),
                asked: Some("tidy my drums".into()),
                window: key,
            }
        );
        assert_eq!(
            of(Command::CoProducerEnd { window: None }),
            ControlRequest::CoProducerEnd { window: None }
        );
        assert_eq!(
            of(Command::CoProducerStatus {
                window: Some(WindowRef::Number(7)),
            }),
            ControlRequest::CoProducerStatus {
                window: Some(WindowRefRow::Number { number: 7 }),
            }
        );
        assert_eq!(
            of(Command::CoProducerPreview {
                plan: plan(),
                window: None,
            }),
            ControlRequest::CoProducerPreview {
                plan: vec![rename(1, 2, "Vox")],
                window: None,
            }
        );
        assert_eq!(
            of(Command::CoProducerApply {
                token: "ab".into(),
                plan: plan(),
                window: Some(WindowRef::Main),
                client: Some("Helper Chat".into()),
            }),
            ControlRequest::CoProducerApply {
                token: "ab".into(),
                plan: vec![rename(1, 2, "Vox")],
                window: Some(WindowRefRow::Main),
                client: Some("Helper Chat".into()),
            }
        );
        // Plan 8b Ruling 9's three.
        assert_eq!(
            of(Command::CoProducerNotes { window: None }),
            ControlRequest::CoProducerNotes { window: None }
        );
        assert_eq!(
            of(Command::CoProducerNote {
                text: "dry verse".into(),
                window: Some(WindowRef::Key),
            }),
            ControlRequest::CoProducerNote {
                text: "dry verse".into(),
                window: key,
            }
        );
        for (what, row) in [
            (ExportWhat::Stems, ExportWhatRow::Stems),
            (ExportWhat::Mix, ExportWhatRow::Mix),
            (ExportWhat::Selection, ExportWhatRow::Selection),
            (ExportWhat::Midi, ExportWhatRow::Midi),
            (ExportWhat::Dawproject, ExportWhatRow::Dawproject),
            (ExportWhat::Aaf, ExportWhatRow::Aaf),
            (ExportWhat::OtherDaw, ExportWhatRow::OtherDaw),
        ] {
            assert_eq!(
                of(Command::CoProducerExportFolder {
                    what,
                    parent: Some("/Users/x/Exports".into()),
                    window: None,
                }),
                ControlRequest::CoProducerExportFolder {
                    what: row,
                    parent: Some("/Users/x/Exports".into()),
                    window: None,
                }
            );
        }
        assert_eq!(
            of(Command::CoProducerExportFolder {
                what: ExportWhat::Mix,
                parent: None,
                window: Some(WindowRef::Main),
            }),
            ControlRequest::CoProducerExportFolder {
                what: ExportWhatRow::Mix,
                parent: None,
                window: Some(WindowRefRow::Main),
            }
        );
        // Plan 8c Ruling 1: the one-call export.
        assert_eq!(
            of(Command::CoProducerExport {
                what: ExportWhat::Stems,
                parent: Some("/Users/x/Exports".into()),
                strips: Some(vec![2, 3]),
                window: Some(WindowRef::Key),
            }),
            ControlRequest::CoProducerExport {
                what: ExportWhatRow::Stems,
                parent: Some("/Users/x/Exports".into()),
                strips: Some(vec![2, 3]),
                window: key,
            }
        );
        assert_eq!(
            of(Command::CoProducerExport {
                what: ExportWhat::Dawproject,
                parent: None,
                strips: None,
                window: None,
            }),
            ControlRequest::CoProducerExport {
                what: ExportWhatRow::Dawproject,
                parent: None,
                strips: None,
                window: None,
            }
        );
        // Plan 8d Ruling 5: the plug-in reads.
        use core_control::request::PluginKindArg;
        use vizdaw_ffi::catalog::PluginKindTag;
        for (kind, tag) in [
            (None, None),
            (
                Some(PluginKindArg::Instrument),
                Some(PluginKindTag::Instrument),
            ),
            (Some(PluginKindArg::Effect), Some(PluginKindTag::Effect)),
        ] {
            assert_eq!(
                of(Command::CoProducerPlugins { kind }),
                ControlRequest::CoProducerPlugins { kind: tag }
            );
        }
        assert_eq!(
            of(Command::CoProducerParams {
                slot: 7,
                window: Some(WindowRef::Key),
            }),
            ControlRequest::CoProducerParams {
                slot: 7,
                window: key
            }
        );
    }

    /// Plan 8f Task 1: the time-record read reaches the app as a row.
    #[test]
    fn the_metrics_read_reaches_the_app_as_a_row() {
        assert_eq!(
            of(Command::CoProducerMetrics {
                days: None,
                window: None,
            }),
            ControlRequest::CoProducerMetrics {
                days: None,
                window: None,
            }
        );
        assert_eq!(
            of(Command::CoProducerMetrics {
                days: Some(30),
                window: Some(WindowRef::Number(2)),
            }),
            ControlRequest::CoProducerMetrics {
                days: Some(30),
                window: Some(WindowRefRow::Number { number: 2 }),
            }
        );
    }

    #[test]
    fn co_producer_results_are_json() {
        let r = ControlResult::CoProducerRequest {
            doc: "D1".into(),
            request: Some(CoProducerRequestRow {
                name: "tidy".into(),
                label: "Co-Producer: Tidy".into(),
                group_open: true,
                continued: false,
            }),
            undo_label: Some("Co-Producer: Tidy".into()),
        };
        assert_eq!(
            result_json(&r),
            json!({
                "doc": "D1",
                "request": {"name": "tidy", "label": "Co-Producer: Tidy", "group_open": true, "continued": false},
                "undo_label": "Co-Producer: Tidy",
            })
        );
        let none = ControlResult::CoProducerRequest {
            doc: "D1".into(),
            request: None,
            undo_label: None,
        };
        assert_eq!(
            result_json(&none),
            json!({"doc": "D1", "request": null, "undo_label": null})
        );
        let p = ControlResult::CoProducerPreview {
            doc: "D1".into(),
            preview: PreviewRow {
                token: "0f".into(),
                needs_yes: true,
                items: vec![row("Track 2, Vox", "deleted", true)],
            },
        };
        assert_eq!(
            result_json(&p),
            json!({
                "doc": "D1", "token": "0f", "needs_yes": true,
                "items": [{"thing": "Track 2, Vox", "change": "deleted", "composers_work": true}],
            })
        );
        let a = ControlResult::CoProducerApplied {
            doc: "D1".into(),
            results: vec!["track.delete: done".into()],
            undo_label: Some("Co-Producer: Tidy".into()),
        };
        assert_eq!(
            result_json(&a),
            json!({"doc": "D1", "results": ["track.delete: done"], "undo_label": "Co-Producer: Tidy"})
        );
        // Plan 8b Task 6: the test hook's answer.
        let h = ControlResult::CoProducerApprovalAnswered {
            app: "Helper Chat".into(),
            kind: "unverified".into(),
            label: Some("Helper Chat".into()),
            path: Some("/usr/local/bin/helper".into()),
            items: vec![row("Track 2, Vox", "deleted", true)],
        };
        assert_eq!(
            result_json(&h),
            json!({
                "app": "Helper Chat", "kind": "unverified", "label": "Helper Chat",
                "path": "/usr/local/bin/helper",
                "items": [{"thing": "Track 2, Vox", "change": "deleted", "composers_work": true}],
            })
        );
    }

    fn private_dir() -> tempfile::TempDir {
        let d = tempfile::Builder::new()
            .prefix("vzc")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        d
    }

    #[test]
    fn the_co_producer_server_admits_only_its_calls() {
        let d = private_dir();
        let path = d.path().join("c.sock");
        let s =
            ControlServer::start_co_producer(path.to_string_lossy().into_owned(), 60_000).unwrap();
        let long = Duration::from_secs(20);
        // Refused by the gate, never queued.
        let mut c = Client::connect(&path).unwrap();
        match c.call("app.quit", json!({}), long) {
            Err(CallError::Reply(e)) => assert_eq!(e.code.as_str(), "not_allowed"),
            other => panic!("{other:?}"),
        }
        // A co_producer call reaches the app; its failure keeps its code.
        let p = path.clone();
        let h = std::thread::spawn(move || {
            Client::connect(&p)
                .unwrap()
                .call("co_producer.begin", json!({"name": "tidy"}), long)
        });
        let deadline = Instant::now() + long;
        let call = loop {
            if let Some(c) = s.next_call().unwrap() {
                break c;
            }
            assert!(Instant::now() < deadline, "no call arrived");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(
            call.request,
            ControlRequest::CoProducerBegin {
                name: "tidy".into(),
                asked: None,
                window: None,
            }
        );
        // Plan 8b Task 6: the peer read at accept rides on the call.
        #[cfg(target_os = "macos")]
        {
            let peer = call
                .peer
                .clone()
                .expect("the Co-Producer socket knows its peer");
            assert_eq!(peer.audit_token.len(), 8);
            assert_eq!(peer.audit_token[5], std::process::id());
            assert!(peer.conn >= 1);
        }
        s.fail(
            call.token,
            ControlFailure {
                code: ControlErrorCode::NotAllowed,
                message: "no".into(),
                dialog: None,
            },
        )
        .unwrap();
        match h.join().unwrap() {
            Err(CallError::Reply(e)) => assert_eq!(e.code.as_str(), "not_allowed"),
            other => panic!("{other:?}"),
        }
        s.stop().unwrap();
    }

    /// Task 7: the app enters for every class but read, from the
    /// registry's own table (Ruling 5).
    #[test]
    fn the_app_knows_which_commands_only_read() {
        use vizdaw_ffi::control::control_command_reads as reads;
        assert!(reads("transport.stop".into()));
        assert!(reads("locator.go_3".into()));
        assert!(!reads("track.new_audio".into()));
        assert!(!reads("file.export_midi".into()));
        assert!(!reads("track.delete".into()));
        assert!(!reads("no.such".into()));
        assert!(!reads("a\0b".repeat(70_000)));
    }
}

/// A composer's job that finishes while Co-Producer acts lands as the
/// composer's own step, never inside "Co-Producer: …", wherever it is
/// applied from: here by Co-Producer's own `file.bounce` (plan 8a final
/// review I1).
#[test]
fn a_composers_finished_job_never_lands_in_a_co_producer_step() {
    use core_media::caf::{CafWriter, WriteMode};
    use vizdaw_ffi::{
        BitDepthRow, BounceRangeRow, BounceRequestRow, BounceSourceRow, ContainerRow,
        ImportTargetRow,
    };
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = saved(dir.path());
    let src = dir.path().join("kick.caf");
    let mut w = CafWriter::create(
        &src,
        48_000,
        2,
        core_model::format::BitDepth::Float32,
        WriteMode::Normal,
    )
    .unwrap();
    w.write(&vec![0.25; 400]).unwrap();
    w.finish().unwrap();
    let before = lanes(&s).len();
    // The composer's import, held until Co-Producer has entered.
    s.debug_hold_jobs(true).unwrap();
    s.import_audio(
        vec![src.display().to_string()],
        Some(ImportTargetRow {
            seq: first_seq(&s),
            lane: None,
            tick: 0,
        }),
    )
    .unwrap();
    s.co_producer_begin("add a track".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    s.new_track(first_seq(&s), TrackKindRow::Audio, None)
        .unwrap();
    s.debug_hold_jobs(false).unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while !s.debug_jobs_settled() {
        assert!(
            std::time::Instant::now() < until,
            "the import never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // Co-Producer's bounce applies finished jobs first.
    s.bounce(BounceRequestRow {
        seq: first_seq(&s),
        source: BounceSourceRow::Mix,
        range: BounceRangeRow::Ticks {
            start: 0,
            end: 15_360,
        },
        container: ContainerRow::Wav,
        depth: BitDepthRow::Int24,
        rate: 48_000,
        dither: false,
        path: Some(dir.path().join("mix.wav").display().to_string()),
        add_to_project: false,
        replace: false,
    })
    .unwrap();
    s.co_producer_leave();
    s.co_producer_end().unwrap();
    assert_eq!(lanes(&s).len(), before + 2, "both tracks are there");
    let top = undo_label(&s).unwrap();
    assert!(
        !top.starts_with("Co-Producer"),
        "the import is the composer's: {top}"
    );
    // Its Undo is the composer's work: it needs a yes.
    assert!(s.co_producer_preview(vec![undo()]).unwrap().needs_yes);
    s.undo().unwrap();
    assert_eq!(lanes(&s).len(), before + 1);
    // Co-Producer's own step holds only its track.
    assert_eq!(undo_label(&s).as_deref(), Some("Co-Producer: Add a Track"));
    s.redo().unwrap();
    let wait = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while !s.debug_jobs_settled() {
        assert!(
            std::time::Instant::now() < wait,
            "the bounce never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    s.idle().unwrap();
    s.close().unwrap();
}

fn colour(strip: u64, c: &str) -> PlannedRow {
    planned(
        "mixer.set_colour",
        vec![int("strip", strip), text("colour", c)],
    )
}

/// The strip of `kind` in the session's mixer.
fn strip_of(s: &Session, want: impl Fn(&vizdaw_ffi::StripKindRow) -> bool) -> vizdaw_ffi::StripRow {
    s.mixer()
        .unwrap()
        .strips
        .into_iter()
        .find(|x| want(&x.kind))
        .unwrap()
}

fn synth_strip(s: &Session) -> vizdaw_ffi::StripRow {
    strip_of(s, |k| matches!(k, vizdaw_ffi::StripKindRow::BuiltinSynth))
}

fn audio_strip(s: &Session) -> vizdaw_ffi::StripRow {
    strip_of(s, |k| {
        matches!(k, vizdaw_ffi::StripKindRow::AudioPath { .. })
    })
}

/// Inside an open request, Co-Producer adds an instrument track (with its
/// own new built-in synth strip); answers the track.
fn co_producer_track(s: &Session) -> u64 {
    s.co_producer_begin("add keys".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    let lane = add_track(s, TrackKindRow::Instrument);
    s.co_producer_leave();
    lane
}

/// Plan 8b Ruling 2: Co-Producer's own track and strip need no yes.
#[test]
fn previews_spare_co_producers_own_things() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path());
    let seq = first_seq(&s);
    let keys = co_producer_track(&s);
    assert_eq!(
        lanes(&s).last().unwrap(),
        &(keys, "Inst 1 (Co-Producer)".to_string())
    );

    let p = preview(&s, &pkg, vec![rename(seq, keys, "Kick")]);
    assert!(!p.needs_yes, "{p:?}");
    assert_eq!(
        p.items,
        vec![row(
            "Track 3, Inst 1 (Co-Producer)",
            "renamed to \"Kick (Co-Producer)\"",
            false
        )]
    );
    let synth = synth_strip(&s);
    assert!(synth.co_producer);
    let p = preview(&s, &pkg, vec![colour(synth.id, "blue")]);
    assert!(!p.needs_yes, "{p:?}");
    assert_eq!(
        p.items,
        vec![row(
            &format!("Strip {}", synth.name),
            "colour none → blue",
            false
        )]
    );
    // An empty Co-Producer track goes without a yes.
    let p = preview(&s, &pkg, vec![delete(seq, keys)]);
    assert!(!p.needs_yes, "{p:?}");

    // The same on the composer's track and strip need a yes.
    let (hand, name) = lanes(&s)[0].clone();
    let p = preview(&s, &pkg, vec![rename(seq, hand, "Kick")]);
    assert!(p.needs_yes);
    assert_eq!(
        p.items,
        vec![row(
            &format!("Track 1, {name}"),
            "renamed to \"Kick\"",
            true
        )]
    );
    let audio = audio_strip(&s);
    assert!(!audio.co_producer);
    let p = preview(&s, &pkg, vec![colour(audio.id, "blue")]);
    assert!(p.needs_yes);
    assert_eq!(
        p.items,
        vec![row(
            &format!("Strip {}", audio.name),
            "colour none → blue",
            true
        )]
    );
    // "none" on an uncoloured strip changes nothing; a missing strip is
    // refused.
    let p = preview(&s, &pkg, vec![colour(audio.id, "none")]);
    assert_eq!(p.items, vec![], "an uncoloured strip stays as it is");
    assert!(s.co_producer_preview(vec![colour(9_999, "red")]).is_err());
    s.co_producer_end().unwrap();
}

/// Ruling 2's stricter rule: a Co-Producer track holding the composer's
/// notes needs a yes to delete.
#[test]
fn deleting_a_filled_co_producer_track_needs_a_yes() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = saved(dir.path());
    let seq = first_seq(&s);
    let keys = co_producer_track(&s);
    add_notes(&s, keys, 1);
    let p = preview(&s, &pkg, vec![delete(seq, keys)]);
    assert!(p.needs_yes, "{p:?}");
    assert!(p.items[0].change.starts_with("deleted, with"), "{p:?}");
    assert!(p.items[0].composers_work);
}

/// The marks reach the rows and the control channel's state JSON.
#[test]
fn marks_reach_the_rows_and_state() {
    use vizdaw_ffi::ControlResult;
    use vizdaw_ffi::control::result_json;
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _pkg) = saved(dir.path());
    let seq = first_seq(&s);
    let keys = co_producer_track(&s);
    s.co_producer_end().unwrap();
    let rows = s.lanes(seq).unwrap();
    let row = rows.iter().find(|l| l.id == keys).unwrap();
    assert!(row.co_producer);
    assert!(rows.iter().filter(|l| l.id != keys).all(|l| !l.co_producer));
    assert!(synth_strip(&s).co_producer);
    assert!(!audio_strip(&s).co_producer);

    let v = result_json(&ControlResult::Document {
        doc: "D".into(),
        path: None,
        snapshot: s.snapshot().unwrap(),
        selected_seq: Some(seq),
        lanes: rows.clone(),
        markers: vec![],
        destinations: vec![],
        alert: None,
    });
    let l = v["lanes"].as_array().unwrap();
    assert_eq!(l[2]["co_producer"], true);
    assert_eq!(l[0]["co_producer"], false);
    let m = result_json(&ControlResult::Mixer {
        doc: "D".into(),
        mixer: s.mixer().unwrap(),
    });
    let strips = m["strips"].as_array().unwrap();
    let synth = synth_strip(&s).id;
    for st in strips {
        assert_eq!(st["co_producer"], st["id"] == synth, "{st}");
    }
}

/// Make Mine through the session: one undo step, and a hostile strip id
/// changes nothing.
#[test]
fn make_mine_through_the_session() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _pkg) = saved(dir.path());
    let seq = first_seq(&s);
    let keys = co_producer_track(&s);
    s.co_producer_end().unwrap();
    s.make_mine_track(seq, keys).unwrap();
    assert_eq!(undo_label(&s).as_deref(), Some("Make Mine"));
    let row = s
        .lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.id == keys)
        .unwrap();
    assert!(!row.co_producer);
    assert_eq!(row.name, "Inst 1");
    let synth = synth_strip(&s);
    assert!(!synth.co_producer);
    assert_eq!(synth.name, "Built-in Synth");

    s.undo().unwrap();
    let synth = synth_strip(&s);
    assert!(synth.co_producer);
    s.make_mine_strip(synth.id).unwrap();
    assert_eq!(undo_label(&s).as_deref(), Some("Make Mine"));
    assert!(!synth_strip(&s).co_producer);

    let before = s.debug_document_json();
    let version = s.snapshot().unwrap().version;
    for bad in [9_999, u64::MAX] {
        assert!(s.make_mine_strip(bad).is_err());
        assert!(s.make_mine_track(seq, bad).is_err());
        assert!(s.make_mine_track(bad, keys).is_err());
    }
    assert_eq!(s.debug_document_json(), before);
    assert_eq!(s.snapshot().unwrap().version, version);
}
