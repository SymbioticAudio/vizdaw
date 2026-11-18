//! Co-Producer's note chores and markers through the session (plan 8c
//! Task 2; Rulings 2–5): each builds the command the plan names, its
//! preview lists exactly what its apply does, every one needs the
//! composer's yes, a plan of marker changes runs in order, and every
//! input is hostile.

use core_control::registry::ArgValue;
use core_model::command::{Command, SequenceCmd};
use core_model::document::Document;
use core_model::ids::{LaneId, SeqId};
use core_model::modify::{Modify, Quantize, ValueOp};
use core_model::sequence::Marker;
use core_model::time::Tick;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::chores::{CHORE_IDS, command_for};
use vizdaw_ffi::co_producer::{CoProducerApproval, PlannedRow};
use vizdaw_ffi::control::{ControlArg, ControlResult, ControlValue, result_json};
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneEdit, SequenceEdit};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::input::upscale_velocity;
use vizdaw_ffi::record::set_free_bytes;
use vizdaw_ffi::tracks::TrackKindRow;
use vizdaw_ffi::{EventKindRow, EventRow, MarkerRow, Session, VizError};

const STAMP: &str = "2026-11-17 10-00";
const CHANGED: &str = "The song changed since the preview; preview again.";

static SERIAL: Mutex<()> = Mutex::new(());

/// Test builds keep the support folder in a per-process temporary folder
/// (`recovery::support_dir`), so nothing here reaches the real one.
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

fn lane_ids(s: &Session) -> Vec<u64> {
    s.lanes(first_seq(s))
        .unwrap()
        .into_iter()
        .map(|l| l.id)
        .collect()
}

fn add_track(s: &Session) -> u64 {
    let before = lane_ids(s);
    s.new_track(first_seq(s), TrackKindRow::Midi, None).unwrap();
    lane_ids(s)
        .into_iter()
        .find(|id| !before.contains(id))
        .unwrap()
}

/// Four notes off the sixteenth grid.
fn add_notes(s: &Session, lane: u64) {
    let events = [100, 4000, 7600, 11600]
        .iter()
        .map(|t| {
            let mut row = EventRow::blank(0, *t, EventKindRow::Note);
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

fn markers(s: &Session) -> Vec<(i64, String)> {
    s.sequence_info(first_seq(s))
        .unwrap()
        .markers
        .into_iter()
        .map(|m| (m.tick, m.name))
        .collect()
}

/// A session saved as `<dir>/P.vizdaw`: two MIDI tracks of four notes,
/// markers "Verse"@0, "Chorus"@15360 and "Outro"@30720.
fn saved(dir: &Path) -> (Arc<Session>, PathBuf, [u64; 2]) {
    let s = Session::new_empty().unwrap();
    let a = add_track(&s);
    let b = add_track(&s);
    add_notes(&s, a);
    add_notes(&s, b);
    s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::SetMarkers {
                seq: first_seq(&s),
                markers: [(0, "Verse"), (15360, "Chorus"), (30720, "Outro")]
                    .iter()
                    .map(|(tick, name)| MarkerRow {
                        tick: *tick,
                        name: (*name).into(),
                    })
                    .collect(),
            },
        },
        None,
    )
    .unwrap();
    let pkg = dir.join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    (s, pkg, [a, b])
}

fn doc(s: &Session) -> Document {
    core_model::file::from_json(&s.debug_document_json()).unwrap()
}

fn int(name: &str, value: i64) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Int { value },
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

/// `args` as the registry hands them on.
fn arg_values(args: &[ControlArg]) -> Vec<(String, ArgValue)> {
    args.iter()
        .map(|a| {
            let v = match &a.value {
                ControlValue::Text { value } => ArgValue::Text(value.clone()),
                ControlValue::Flag { value } => ArgValue::Flag(*value),
                ControlValue::Int { value } => ArgValue::Int(*value),
            };
            (a.name.clone(), v)
        })
        .collect()
}

fn notes_cmd(seq: u64, lane: u64, modify: Modify) -> Command {
    Command::ModifyNotes {
        seq: SeqId(seq),
        lanes: vec![LaneId(lane)],
        range: None,
        modify,
    }
}

#[test]
fn chores_build_the_right_commands() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _pkg, [a, _]) = saved(dir.path());
    let seq = first_seq(&s);
    let d = doc(&s);
    let lanes = a.to_string();
    let build = |id: &str, extra: Vec<ControlArg>| {
        let mut args = vec![int("seq", seq as i64), text("lanes", &lanes)];
        args.extend(extra);
        command_for(&d, id, &arg_values(&args)).unwrap()
    };
    assert_eq!(
        build("modify.quantize_notes", vec![text("grid", "1/16t")]),
        notes_cmd(
            seq,
            a,
            Modify::Quantize(Quantize::Grid {
                grid: Tick(2560),
                strength: 100,
            })
        )
    );
    assert_eq!(
        build(
            "modify.velocity_notes",
            vec![text("op", "set"), int("amount", 100)]
        ),
        notes_cmd(
            seq,
            a,
            Modify::Velocity(ValueOp::Set(u32::from(upscale_velocity(100))))
        )
    );
    assert_eq!(
        build(
            "modify.velocity_notes",
            vec![text("op", "add"), int("amount", -10)]
        ),
        notes_cmd(seq, a, Modify::Velocity(ValueOp::Add(-5120)))
    );
    let ranged = command_for(
        &d,
        "modify.transpose_notes",
        &arg_values(&[
            int("seq", seq as i64),
            text("lanes", &lanes),
            int("start", 0),
            int("end", 7680),
            int("semitones", -3),
        ]),
    )
    .unwrap();
    assert_eq!(
        ranged,
        Command::ModifyNotes {
            seq: SeqId(seq),
            lanes: vec![LaneId(a)],
            range: Some((Tick(0), Tick(7680))),
            modify: Modify::Transpose(core_model::modify::Transpose::Interval { semitones: -3 }),
        }
    );
    let add = command_for(
        &d,
        "marker.add",
        &arg_values(&[
            int("seq", seq as i64),
            int("tick", 7680),
            text("name", "Bridge"),
        ]),
    )
    .unwrap();
    let m = |tick: i64, name: &str| Marker {
        tick: Tick(tick),
        name: name.into(),
    };
    assert_eq!(
        add,
        Command::Sequence(SequenceCmd::SetMarkers {
            seq: SeqId(seq),
            markers: [
                m(0, "Verse"),
                m(7680, "Bridge"),
                m(15360, "Chorus"),
                m(30720, "Outro")
            ]
            .into_iter()
            .collect(),
        })
    );
    // A marker with the same tick and name is refused.
    let twice = command_for(
        &d,
        "marker.add",
        &arg_values(&[
            int("seq", seq as i64),
            int("tick", 0),
            text("name", "Verse"),
        ]),
    )
    .unwrap_err();
    assert!(
        twice
            .to_string()
            .contains("There is already a marker \"Verse\" there."),
        "{twice}"
    );
}

/// One Co-Producer call that takes `token` for `plan` and runs it.
fn take_and_run(s: &Session, token: String, plan: &[PlannedRow]) -> Result<(), VizError> {
    s.co_producer_enter(STAMP.into()).unwrap();
    let r = s
        .co_producer_take(token, plan.to_vec(), CoProducerApproval::Sheet)
        .and_then(|()| {
            for p in plan {
                s.run_chore(p.id.clone(), p.args.clone())?;
            }
            Ok(())
        });
    s.co_producer_leave();
    r
}

#[test]
fn previews_match_what_the_apply_does() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _pkg, [a, _]) = saved(dir.path());
    let seq = first_seq(&s) as i64;
    // A track Co-Producer added, with notes it wrote.
    s.co_producer_begin("tidy the notes".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    let ours = add_track(&s);
    add_notes(&s, ours);
    s.co_producer_leave();
    assert!(
        s.lanes(first_seq(&s))
            .unwrap()
            .iter()
            .any(|l| l.id == ours && l.co_producer)
    );
    let both = format!("{a},{ours}");
    let chores = vec![
        planned(
            "modify.quantize_notes",
            vec![int("seq", seq), text("lanes", &both), text("grid", "1/16")],
        ),
        planned(
            "modify.transpose_notes",
            vec![
                int("seq", seq),
                text("lanes", &ours.to_string()),
                int("semitones", 2),
            ],
        ),
        planned(
            "modify.velocity_notes",
            vec![
                int("seq", seq),
                text("lanes", &ours.to_string()),
                int("start", 0),
                int("end", 5000),
                text("op", "scale"),
                int("amount", 50),
            ],
        ),
        planned(
            "marker.rename",
            vec![int("seq", seq), int("index", 2), text("name", "Hook")],
        ),
        planned("marker.remove", vec![int("seq", seq), int("index", 3)]),
    ];
    for chore in chores {
        let plan = vec![chore];
        let p = s.co_producer_preview(plan.clone()).unwrap();
        assert!(p.needs_yes, "{}", plan[0].id);
        assert!(!p.items.is_empty(), "{}", plan[0].id);
        assert!(p.items.iter().all(|i| i.composers_work), "{p:?}");
        let before = doc(&s);
        take_and_run(&s, p.token, &plan).unwrap();
        let said: Vec<(String, String)> = core_model::diff::describe(&before, &doc(&s))
            .into_iter()
            .map(|i| (i.thing, i.change))
            .collect();
        let shown: Vec<(String, String)> =
            p.items.into_iter().map(|i| (i.thing, i.change)).collect();
        assert_eq!(said, shown, "{}", plan[0].id);
    }
    // A hand edit between the preview and the take.
    let plan = vec![planned(
        "marker.rename",
        vec![int("seq", seq), int("index", 1), text("name", "Intro")],
    )];
    let p = s.co_producer_preview(plan.clone()).unwrap();
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Rename {
                seq: first_seq(&s),
                lane: a,
                name: "Keys".into(),
            },
        },
        None,
    )
    .unwrap();
    let before = s.debug_document_json();
    let e = take_and_run(&s, p.token, &plan).unwrap_err();
    assert_eq!(e.to_string(), CHANGED);
    assert_eq!(s.debug_document_json(), before);
    s.co_producer_end().unwrap();
}

#[test]
fn marker_plans_run_in_order() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _pkg, _) = saved(dir.path());
    let seq = first_seq(&s) as i64;
    s.co_producer_begin("tidy the markers".into()).unwrap();
    let plan = vec![
        planned(
            "marker.rename",
            vec![int("seq", seq), int("index", 2), text("name", "Hook")],
        ),
        planned("marker.remove", vec![int("seq", seq), int("index", 1)]),
    ];
    let p = s.co_producer_preview(plan.clone()).unwrap();
    let shown: Vec<(String, String)> = p
        .items
        .iter()
        .map(|i| (i.thing.clone(), i.change.clone()))
        .collect();
    assert_eq!(
        shown,
        vec![
            ("Marker 2, Chorus".into(), "renamed to \"Hook\"".into()),
            ("Marker 1, Verse".into(), "deleted".into()),
        ]
    );
    take_and_run(&s, p.token, &plan).unwrap();
    assert_eq!(
        markers(&s),
        vec![(15360, "Hook".to_string()), (30720, "Outro".to_string())]
    );
    // An index past the end fails at preview and changes nothing.
    let before = s.debug_document_json();
    let e = s
        .co_producer_preview(vec![planned(
            "marker.remove",
            vec![int("seq", seq), int("index", 9)],
        )])
        .unwrap_err();
    assert_eq!(e.to_string(), format!("Sequence {seq} has 2 markers."));
    assert_eq!(s.debug_document_json(), before);
    s.co_producer_end().unwrap();
    // Three markers, as the plan names it, outside the request.
    let dir = tempfile::tempdir().unwrap();
    let (s, _pkg, _) = saved(dir.path());
    let e = s
        .co_producer_preview(vec![planned(
            "marker.remove",
            vec![int("seq", seq), int("index", 9)],
        )])
        .unwrap_err();
    assert_eq!(e.to_string(), format!("Sequence {seq} has 3 markers."));
}

#[test]
fn markers_reach_the_state() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _pkg, _) = saved(dir.path());
    let seq = first_seq(&s);
    let v = result_json(&ControlResult::Document {
        doc: "D".into(),
        path: None,
        snapshot: s.snapshot().unwrap(),
        selected_seq: Some(seq),
        lanes: s.lanes(seq).unwrap(),
        markers: s.sequence_info(seq).unwrap().markers,
        destinations: vec![],
        alert: None,
    });
    assert_eq!(
        v["markers"],
        serde_json::json!([
            {"index": 1, "tick": 0, "name": "Verse"},
            {"index": 2, "tick": 15360, "name": "Chorus"},
            {"index": 3, "tick": 30720, "name": "Outro"},
        ])
    );
}

#[test]
fn run_chore_refuses_hostile_input() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _pkg, [a, _]) = saved(dir.path());
    let seq = first_seq(&s) as i64;
    let lanes = a.to_string();
    let before = s.debug_document_json();
    let cases: Vec<(&str, Vec<ControlArg>)> = vec![
        ("modify.nope", vec![int("seq", seq)]),
        ("track.delete", vec![int("seq", seq), int("lane", a as i64)]),
        (
            "modify.transpose_notes",
            vec![int("seq", seq), text("lanes", &lanes)],
        ),
        (
            "modify.transpose_notes",
            vec![
                int("seq", seq),
                text("lanes", &"1".repeat(70_000)),
                int("semitones", 2),
            ],
        ),
        (
            "modify.transpose_notes",
            vec![
                int("seq", u64::MAX as i64),
                text("lanes", &lanes),
                int("semitones", 2),
            ],
        ),
        (
            "modify.transpose_notes",
            vec![
                int("seq", seq),
                text("lanes", &lanes),
                text("semitones", "2"),
            ],
        ),
        (
            "modify.transpose_notes",
            vec![
                int("seq", seq),
                text("lanes", &lanes),
                int("semitones", 2),
                int("extra", 1),
            ],
        ),
        (
            "modify.transpose_notes",
            vec![int("seq", seq), text("lanes", "1,\0"), int("semitones", 2)],
        ),
        (
            "modify.transpose_notes",
            vec![int("seq", 999), text("lanes", &lanes), int("semitones", 2)],
        ),
        (
            "modify.transpose_notes",
            vec![int("seq", seq), text("lanes", "999"), int("semitones", 2)],
        ),
        (
            "marker.rename",
            vec![int("seq", seq), int("index", 4), text("name", "Hook")],
        ),
        (
            "marker.rename",
            vec![
                int("seq", seq),
                int("index", 1),
                text("name", &"x".repeat(70_000)),
            ],
        ),
        (
            "marker.add",
            vec![int("seq", 999), int("tick", 0), text("name", "Intro")],
        ),
        (
            "marker.remove",
            vec![int("seq", seq), int("index", i64::MAX)],
        ),
    ];
    for (id, args) in cases {
        let r = s.run_chore(id.into(), args.clone());
        assert!(r.is_err(), "{id} {args:?}");
        assert_eq!(s.debug_document_json(), before, "{id}");
    }
    assert!(CHORE_IDS.contains(&"marker.add"));
    // A good one outside a request runs as a hand edit.
    s.run_chore(
        "marker.add".into(),
        vec![int("seq", seq), int("tick", 7680), text("name", "Bridge")],
    )
    .unwrap();
    assert_eq!(markers(&s)[1], (7680, "Bridge".to_string()));
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Add Marker")
    );
}

// ---- Plan 8c Task 3: the export plan (Ruling 1), and jobs that land in
// their request (Ruling 6). ----

const DATE: &str = "2026-11-16";
const OPEN_FIRST: &str = "Open a request first (begin_request).";
/// One bar at 120 BPM.
const BAR: i64 = 4 * 15_360;
const FRAMES: u64 = 96_000;

/// `<dir>/<name>.vizdaw` built by `build` (on a sequence "Song" whose
/// first track is "Bass" with no destination) and saved, with
/// `Media/a.caf` written (FRAMES stereo frames). Opened, with a private
/// Trash.
fn song_with(
    dir: &Path,
    name: &str,
    build: impl FnOnce(&mut core_engine::fixtures::Builder, SeqId, LaneId),
) -> (Arc<Session>, PathBuf) {
    use core_media::caf::{CafWriter, WriteMode};
    use core_model::format::BitDepth;
    let pkg = dir.join(format!("{name}.vizdaw"));
    let mut b = core_engine::fixtures::Builder::new();
    let (seq, bass) = b.sequence("Song", "Bass", None).unwrap();
    build(&mut b, seq, bass);
    core_model::file::save(&b.finish(), &pkg).unwrap();
    let media = pkg.join("Media");
    std::fs::create_dir_all(&media).unwrap();
    let mut w = CafWriter::create(
        &media.join("a.caf"),
        48_000,
        2,
        BitDepth::Float32,
        WriteMode::Normal,
    )
    .unwrap();
    w.write(&vec![0.25f32; FRAMES as usize * 2]).unwrap();
    w.finish().unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.join("Trash").display().to_string()));
    (s, pkg)
}

/// Two audio tracks, "Bass" and "Drums", each playing `Media/a.caf` from
/// tick 0.
fn audio_song(dir: &Path, name: &str) -> (Arc<Session>, PathBuf) {
    use core_model::event::Fade;
    song_with(dir, name, |b, seq, bass| {
        let drums = b.lane(seq, "Drums", None).unwrap();
        let m = b.media("Media/a.caf", FRAMES, 2).unwrap();
        for lane in [bass, drums] {
            b.region(
                seq,
                lane,
                0,
                m,
                0,
                FRAMES,
                0.0,
                Fade::default(),
                Fade::default(),
            )
            .unwrap();
        }
    })
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn arg<'a>(args: &'a [ControlArg], name: &str) -> Option<&'a ControlValue> {
    args.iter().find(|a| a.name == name).map(|a| &a.value)
}

fn text_arg(args: &[ControlArg], name: &str) -> Option<String> {
    match arg(args, name) {
        Some(ControlValue::Text { value }) => Some(value.clone()),
        Some(other) => panic!("{name}: {other:?}"),
        None => None,
    }
}

fn shown(p: &Path) -> String {
    p.display().to_string()
}

fn plan(
    s: &Session,
    what: vizdaw_ffi::ExportWhatRow,
    parent: &Path,
    strips: Option<Vec<u64>>,
) -> Result<vizdaw_ffi::chores::ExportPlanRow, VizError> {
    s.co_producer_export_plan(what, DATE.into(), Some(shown(parent)), strips)
}

/// Each planned path passes the Co-Producer output check, as the app
/// runs it.
fn passes_the_check(p: &vizdaw_ffi::chores::ExportPlanRow) {
    use core_control::co_producer::{check_output, output};
    let args = arg_values(&p.args);
    let rule = output(&p.id, &args).unwrap_or_else(|| panic!("{} has no output", p.id));
    let path = text_arg(&p.args, "path").unwrap();
    check_output(Path::new(&path), rule).unwrap_or_else(|e| panic!("{path}: {}", e.message));
    for never in ["replace", "dry_run", "add_to_project"] {
        assert!(arg(&p.args, never).is_none(), "{} sets {never}", p.id);
    }
}

#[test]
fn export_plans_are_new_and_checked() {
    use vizdaw_ffi::ExportWhatRow as W;
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = audio_song(dir.path(), "Night Drive");
    let parent = dir.path().join("Exports");
    std::fs::create_dir(&parent).unwrap();
    s.co_producer_begin("export the song".into()).unwrap();

    let p = plan(&s, W::Dawproject, &parent, None).unwrap();
    let folder = parent.join("Night Drive — DAWproject — 2026-11-16");
    assert_eq!(p.id, "file.export_dawproject");
    assert_eq!(p.folder, shown(&folder));
    assert!(folder.is_dir());
    assert!(names_in(&folder).is_empty(), "the folder starts empty");
    assert_eq!(p.args.len(), 1, "{:?}", p.args);
    assert_eq!(
        text_arg(&p.args, "path"),
        Some(shown(&folder.join("Night Drive.dawproject")))
    );
    passes_the_check(&p);

    // Stems with no strips: For Another DAW's defaults.
    let defaults: Vec<String> = core_io::for_daw::default_strips(&doc(&s), SeqId(first_seq(&s)))
        .unwrap()
        .into_iter()
        .map(|id| id.0.to_string())
        .collect();
    assert!(
        !defaults.is_empty(),
        "the audio tracks play through a strip"
    );
    let p = plan(&s, W::Stems, &parent, None).unwrap();
    let folder = parent.join("Night Drive — Stems — 2026-11-16");
    assert_eq!(p.id, "file.bounce");
    assert_eq!(p.folder, shown(&folder));
    assert_eq!(text_arg(&p.args, "source").as_deref(), Some("stems"));
    assert_eq!(text_arg(&p.args, "strips"), Some(defaults.join(",")));
    assert_eq!(text_arg(&p.args, "path"), Some(shown(&folder)));
    assert_eq!(p.args.len(), 3, "{:?}", p.args);
    passes_the_check(&p);
    let again = plan(&s, W::Stems, &parent, None).unwrap();
    assert_eq!(
        again.folder,
        shown(&parent.join("Night Drive — Stems — 2026-11-16 2"))
    );
    passes_the_check(&again);

    // Ruling 1's table.
    let p = plan(&s, W::Mix, &parent, None).unwrap();
    let folder = parent.join("Night Drive — Mix — 2026-11-16");
    assert_eq!(p.id, "file.bounce");
    assert_eq!(text_arg(&p.args, "source").as_deref(), Some("mix"));
    assert_eq!(
        text_arg(&p.args, "path"),
        Some(shown(&folder.join("Night Drive.wav")))
    );
    assert_eq!(p.args.len(), 2, "{:?}", p.args);
    passes_the_check(&p);

    let p = plan(&s, W::Midi, &parent, None).unwrap();
    let folder = parent.join("Night Drive — MIDI — 2026-11-16");
    assert_eq!(p.id, "file.export_midi");
    assert_eq!(
        text_arg(&p.args, "path"),
        Some(shown(&folder.join("Night Drive.mid")))
    );
    assert_eq!(p.args.len(), 1, "{:?}", p.args);
    passes_the_check(&p);

    let p = plan(&s, W::Aaf, &parent, None).unwrap();
    let folder = parent.join("Night Drive — AAF — 2026-11-16");
    assert_eq!(p.id, "file.export_aaf");
    assert_eq!(text_arg(&p.args, "path"), Some(shown(&folder)));
    assert_eq!(p.args.len(), 1, "{:?}", p.args);
    passes_the_check(&p);

    let p = plan(&s, W::OtherDaw, &parent, None).unwrap();
    let folder = parent.join("Night Drive — Other DAW — 2026-11-16");
    assert_eq!(p.id, "file.export_for_daw");
    assert_eq!(p.folder, shown(&folder));
    assert_eq!(text_arg(&p.args, "path"), Some(shown(&folder)));
    assert_eq!(p.args.len(), 1, "no strips unless given: {:?}", p.args);
    passes_the_check(&p);
    let first: u64 = defaults[0].parse().unwrap();
    let p = plan(&s, W::OtherDaw, &parent, Some(vec![first])).unwrap();
    assert_eq!(text_arg(&p.args, "strips"), Some(first.to_string()));
    passes_the_check(&p);
    let p = plan(&s, W::Stems, &parent, Some(vec![first])).unwrap();
    assert_eq!(text_arg(&p.args, "strips"), Some(first.to_string()));

    s.co_producer_end().unwrap();
    s.close().unwrap();
}

#[test]
fn an_export_plan_is_logged_like_an_export_folder() {
    use vizdaw_ffi::ExportWhatRow as W;
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = audio_song(dir.path(), "Night Drive");
    let parent = dir.path().join("Exports");
    std::fs::create_dir(&parent).unwrap();
    s.co_producer_begin("export the song".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    let p = plan(&s, W::Mix, &parent, None).unwrap();
    s.co_producer_leave();
    s.co_producer_end().unwrap();
    let notes = std::fs::read_to_string(pkg.join("Notes.md")).unwrap();
    assert!(
        notes.contains(&format!("\n- Export folder: {}\n", parent.display())),
        "{notes}"
    );
    assert!(notes.contains(&p.folder), "{notes}");
    // The next request remembers the parent.
    s.co_producer_begin("export again".into()).unwrap();
    let p = s
        .co_producer_export_plan(W::Midi, DATE.into(), None, None)
        .unwrap();
    assert!(Path::new(&p.folder).starts_with(&parent), "{}", p.folder);
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

#[test]
fn a_refused_plan_makes_no_folder() {
    use vizdaw_ffi::ExportWhatRow as W;
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = audio_song(dir.path(), "Night Drive");
    let parent = dir.path().join("Exports");
    std::fs::create_dir(&parent).unwrap();
    s.co_producer_begin("export the song".into()).unwrap();
    for what in [W::Stems, W::OtherDaw] {
        let e = plan(&s, what, &parent, Some(vec![999])).unwrap_err();
        assert!(e.to_string().contains("999"), "{e}");
    }
    let e = plan(&s, W::Selection, &parent, None).unwrap_err();
    assert!(e.to_string().contains("export_folder"), "{e}");
    let e = plan(&s, W::Mix, &parent, Some(vec![1])).unwrap_err();
    assert!(e.to_string().contains("strips"), "{e}");
    let e = s
        .co_producer_export_plan(W::Mix, "16 Nov".into(), Some(shown(&parent)), None)
        .unwrap_err();
    assert!(e.to_string().contains("YYYY-MM-DD"), "{e}");
    assert!(names_in(&parent).is_empty(), "{:?}", names_in(&parent));

    // A remembered parent inside the package.
    let before = names_in(&pkg);
    std::fs::write(
        pkg.join("Notes.md"),
        format!("# Night Drive\n- Export folder: {}\n", pkg.display()),
    )
    .unwrap();
    let e = s
        .co_producer_export_plan(W::Mix, DATE.into(), None, None)
        .unwrap_err();
    assert!(
        e.to_string().contains("it is inside a project package"),
        "{e}"
    );
    let mut after = names_in(&pkg);
    after.retain(|n| n != "Notes.md");
    let mut was = before.clone();
    was.retain(|n| n != "Notes.md");
    assert_eq!(after, was);
    s.co_producer_end().unwrap();
    s.close().unwrap();

    // Only external MIDI plays: no stems.
    let (s, _) = song_with(dir.path(), "Outboard", |b, seq, bass| {
        let ext = b.dest("Ext", "port:Outboard", 0).unwrap();
        b.run(Command::Lane(core_model::command::LaneCmd::SetDest {
            seq,
            lane: bass,
            dest: Some(ext),
        }))
        .unwrap();
        b.notes(seq, bass, &[(0, 64, 0x8000, 15_360)]).unwrap();
    });
    s.co_producer_begin("export the stems".into()).unwrap();
    let e = plan(&s, W::Stems, &parent, None).unwrap_err();
    assert_eq!(
        e.to_string(),
        "There is nothing to bounce: no strip plays audio."
    );
    assert!(names_in(&parent).is_empty(), "{:?}", names_in(&parent));
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

/// A ":" in a file name is how the Finder shows a "/"; a leading dot is
/// dropped (8b Ruling 6's cleaning).
/// Plan 8c final review, Important 1: an export the app would refuse to
/// start is refused before its folder is made, so no empty folder and no
/// notes line are left, and the retry makes the first-numbered folder.
#[test]
fn a_refused_export_leaves_no_folder_and_no_notes_line() {
    use vizdaw_ffi::ExportWhatRow as W;
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = audio_song(dir.path(), "Night Drive");
    let parent = dir.path().join("Exports");
    let out = dir.path().join("out");
    std::fs::create_dir(&parent).unwrap();
    std::fs::create_dir(&out).unwrap();
    s.co_producer_begin("export the stems".into()).unwrap();
    // A bounce is running.
    s.debug_hold_jobs(true).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    let running = stems_onto_tracks(&s, &out);
    let e = plan(&s, W::Stems, &parent, None).unwrap_err();
    s.co_producer_leave();
    assert!(matches!(e, VizError::Busy { .. }), "{e:?}");
    assert!(names_in(&parent).is_empty(), "{:?}", names_in(&parent));
    s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&s, running).state, vizdaw_ffi::JobStateRow::Done);
    s.co_producer_end().unwrap();
    let notes = std::fs::read_to_string(pkg.join("Notes.md")).unwrap();
    assert!(!notes.contains("Night Drive — Stems"), "{notes}");

    // The retry makes the first-numbered folder, not "… 2".
    s.co_producer_begin("export the stems again".into())
        .unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    let p = plan(&s, W::Stems, &parent, None).unwrap();
    s.co_producer_leave();
    let first = parent.join("Night Drive — Stems — 2026-11-16");
    assert_eq!(p.folder, shown(&first));
    assert_eq!(names_in(&parent), ["Night Drive — Stems — 2026-11-16"]);
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

/// A planned export that still did not start: its folder goes while it is
/// empty, with its line; a folder with anything in it, or one the entry
/// does not name, stays.
#[test]
fn a_planned_folder_is_dropped_only_while_empty() {
    use vizdaw_ffi::ExportWhatRow as W;
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg) = audio_song(dir.path(), "Night Drive");
    let parent = dir.path().join("Exports");
    std::fs::create_dir(&parent).unwrap();
    s.co_producer_begin("export".into()).unwrap();
    let mix = plan(&s, W::Mix, &parent, None).unwrap();
    let midi = plan(&s, W::Midi, &parent, None).unwrap();
    std::fs::write(Path::new(&midi.folder).join("kept.txt"), "x").unwrap();
    assert_eq!(s.co_producer_drop_export(mix.folder.clone()), Ok(true));
    assert!(!Path::new(&mix.folder).exists());
    assert_eq!(s.co_producer_drop_export(midi.folder.clone()), Ok(false));
    assert!(Path::new(&midi.folder).join("kept.txt").is_file());
    // Not the entry's: nothing happens.
    let other = parent.join("Other");
    std::fs::create_dir(&other).unwrap();
    assert_eq!(s.co_producer_drop_export(shown(&other)), Ok(false));
    assert!(other.is_dir());
    s.co_producer_end().unwrap();
    let notes = std::fs::read_to_string(pkg.join("Notes.md")).unwrap();
    assert!(!notes.contains(&mix.folder), "{notes}");
    assert!(notes.contains(&midi.folder), "{notes}");
    // No open request.
    assert!(matches!(
        s.co_producer_drop_export(midi.folder.clone()),
        Err(VizError::Model { .. })
    ));
    s.close().unwrap();
}

#[test]
fn odd_song_names_are_cleaned() {
    use vizdaw_ffi::ExportWhatRow as W;
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("Exports");
    std::fs::create_dir(&parent).unwrap();
    for (name, song) in [("..:Mix", "-Mix"), ("a:b", "a-b"), (".hidden", "hidden")] {
        let (s, _) = audio_song(dir.path(), name);
        s.co_producer_begin("export the song".into()).unwrap();
        let p = plan(&s, W::Dawproject, &parent, None).unwrap();
        let folder = parent.join(format!("{song} — DAWproject — 2026-11-16"));
        assert_eq!(p.folder, shown(&folder), "{name}");
        assert_eq!(
            text_arg(&p.args, "path"),
            Some(shown(&folder.join(format!("{song}.dawproject")))),
            "{name}"
        );
        passes_the_check(&p);
        s.co_producer_end().unwrap();
        s.close().unwrap();
    }
}

#[test]
fn export_needs_a_request() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = audio_song(dir.path(), "Night Drive");
    let parent = dir.path().join("Exports");
    std::fs::create_dir(&parent).unwrap();
    let e = plan(&s, vizdaw_ffi::ExportWhatRow::Mix, &parent, None).unwrap_err();
    assert_eq!(e.to_string(), OPEN_FIRST);
    assert!(names_in(&parent).is_empty());
    s.close().unwrap();
}

fn wait_job(s: &Session, id: u64) -> vizdaw_ffi::JobRow {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        s.idle().unwrap();
        let j = s.job(id).unwrap();
        if j.state != vizdaw_ffi::JobStateRow::Running {
            s.idle().unwrap();
            return j;
        }
        assert!(std::time::Instant::now() < until, "job {id} still running");
        std::thread::yield_now();
    }
}

/// Stems of every default strip, one bar, onto new tracks and into `out`.
fn stems_onto_tracks(s: &Session, out: &Path) -> u64 {
    use vizdaw_ffi::{
        BitDepthRow, BounceRangeRow, BounceRequestRow, BounceSourceRow, ContainerRow,
    };
    let strips = core_io::for_daw::default_strips(&doc(s), SeqId(first_seq(s)))
        .unwrap()
        .into_iter()
        .map(|id| id.0)
        .collect();
    s.bounce(BounceRequestRow {
        seq: first_seq(s),
        source: BounceSourceRow::Stems { strips },
        range: BounceRangeRow::Ticks { start: 0, end: BAR },
        container: ContainerRow::Wav,
        depth: BitDepthRow::Int24,
        rate: 48_000,
        dither: false,
        path: Some(shown(out)),
        add_to_project: true,
        replace: false,
    })
    .unwrap()
}

fn lane_names(s: &Session) -> Vec<String> {
    s.lanes(first_seq(s))
        .unwrap()
        .into_iter()
        .map(|l| l.name)
        .collect()
}

fn undo_label(s: &Session) -> Option<String> {
    s.snapshot().unwrap().undo_label
}

#[test]
fn jobs_land_inside_their_request() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = audio_song(dir.path(), "Night Drive");
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let label = s
        .co_producer_begin("bounce the stems".into())
        .unwrap()
        .label;
    s.co_producer_enter(STAMP.into()).unwrap();
    let id = stems_onto_tracks(&s, &out);
    s.co_producer_leave();
    let j = wait_job(&s, id);
    assert_eq!(j.state, vizdaw_ffi::JobStateRow::Done, "{j:?}");
    assert_eq!(
        s.co_producer_end().unwrap().as_deref(),
        Some(label.as_str())
    );
    assert_eq!(undo_label(&s).as_deref(), Some(label.as_str()));
    let names = lane_names(&s);
    assert!(names.len() > 2, "{names:?}");
    for n in &names[2..] {
        assert!(n.ends_with(" (Co-Producer)"), "{names:?}");
    }
    assert!(!names[0].ends_with(" (Co-Producer)"), "{names:?}");
    s.undo().unwrap();
    assert_eq!(lane_names(&s), ["Bass", "Drums"], "one undo removes them");

    // A hand rename between the leave and the landing: "(continued)".
    let label = s.co_producer_begin("mark and bounce".into()).unwrap().label;
    s.co_producer_enter(STAMP.into()).unwrap();
    s.run_chore(
        "marker.add".into(),
        vec![
            int("seq", first_seq(&s) as i64),
            int("tick", 0),
            text("name", "Top"),
        ],
    )
    .unwrap();
    s.debug_hold_jobs(true).unwrap();
    let id = stems_onto_tracks(&s, &out);
    s.co_producer_leave();
    let bass = lane_ids(&s)[0];
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Rename {
                seq: first_seq(&s),
                lane: bass,
                name: "Low End".into(),
            },
        },
        None,
    )
    .unwrap();
    s.debug_hold_jobs(false).unwrap();
    let j = wait_job(&s, id);
    assert_eq!(j.state, vizdaw_ffi::JobStateRow::Done, "{j:?}");
    let continued = format!("{label} (continued)");
    assert_eq!(
        s.co_producer_end().unwrap().as_deref(),
        Some(continued.as_str())
    );
    assert_eq!(undo_label(&s).as_deref(), Some(continued.as_str()));
    let names = lane_names(&s);
    assert!(names.len() > 2, "{names:?}");
    assert!(
        names[2..].iter().all(|n| n.ends_with(" (Co-Producer)")),
        "{names:?}"
    );
    s.undo().unwrap();
    assert_eq!(lane_names(&s), ["Low End", "Drums"]);
    s.undo().unwrap();
    assert_eq!(lane_names(&s), ["Bass", "Drums"]);
    assert_eq!(undo_label(&s).as_deref(), Some(label.as_str()));
    s.close().unwrap();
}

#[test]
fn a_job_after_its_request_lands_on_its_own() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = audio_song(dir.path(), "Night Drive");
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    s.co_producer_begin("bounce the stems".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    s.debug_hold_jobs(true).unwrap();
    let id = stems_onto_tracks(&s, &out);
    s.co_producer_leave();
    assert_eq!(s.co_producer_end().unwrap(), None, "no step yet");
    // A later request does not take it either.
    s.co_producer_begin("something else".into()).unwrap();
    s.debug_hold_jobs(false).unwrap();
    let j = wait_job(&s, id);
    assert_eq!(j.state, vizdaw_ffi::JobStateRow::Done, "{j:?}");
    assert_eq!(s.co_producer_end().unwrap(), None);
    assert_eq!(undo_label(&s).as_deref(), Some("Bounce"));
    let names = lane_names(&s);
    assert!(names.len() > 2, "{names:?}");
    assert!(
        names.iter().all(|n| !n.ends_with(" (Co-Producer)")),
        "{names:?}"
    );
    s.undo().unwrap();
    assert_eq!(lane_names(&s), ["Bass", "Drums"]);
    s.redo().unwrap();
    assert_eq!(lane_names(&s), names, "no step is lost");
    s.close().unwrap();
}

/// 8a final review: a job the composer started lands as the composer's
/// own step, even while a request is open.
#[test]
fn a_composers_job_during_a_request_stays_the_composers() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = audio_song(dir.path(), "Night Drive");
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let label = s.co_producer_begin("add a marker".into()).unwrap().label;
    s.co_producer_enter(STAMP.into()).unwrap();
    s.run_chore(
        "marker.add".into(),
        vec![
            int("seq", first_seq(&s) as i64),
            int("tick", 0),
            text("name", "Top"),
        ],
    )
    .unwrap();
    s.co_producer_leave();
    let id = stems_onto_tracks(&s, &out);
    let j = wait_job(&s, id);
    assert_eq!(j.state, vizdaw_ffi::JobStateRow::Done, "{j:?}");
    assert_eq!(undo_label(&s).as_deref(), Some("Bounce"));
    let names = lane_names(&s);
    assert!(names.len() > 2, "{names:?}");
    assert!(
        names.iter().all(|n| !n.ends_with(" (Co-Producer)")),
        "{names:?}"
    );
    assert_eq!(
        s.co_producer_end().unwrap(),
        None,
        "the hand step closed it"
    );
    s.undo().unwrap();
    assert_eq!(undo_label(&s).as_deref(), Some(label.as_str()));
    s.close().unwrap();
}

#[test]
fn export_result_json() {
    let v = result_json(&ControlResult::CoProducerExported {
        doc: "d".into(),
        folder: "/x/Night Drive — Mix — 2026-11-16".into(),
        written: vec!["/x/Night Drive — Mix — 2026-11-16/Night Drive.wav".into()],
        summary: None,
    });
    assert_eq!(
        v,
        serde_json::json!({
            "doc": "d",
            "folder": "/x/Night Drive — Mix — 2026-11-16",
            "written": ["/x/Night Drive — Mix — 2026-11-16/Night Drive.wav"],
            "summary": null,
        })
    );
    let v = result_json(&ControlResult::CoProducerExported {
        doc: "d".into(),
        folder: "/x".into(),
        written: vec![],
        summary: Some("3 tracks".into()),
    });
    assert_eq!(v["summary"], "3 tracks");
}

/// What, date, parent and strips.
type HostileExport = (
    vizdaw_ffi::ExportWhatRow,
    String,
    Option<String>,
    Option<Vec<u64>>,
);

#[test]
fn export_plan_refuses_hostile_input() {
    use vizdaw_ffi::ExportWhatRow as W;
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = audio_song(dir.path(), "Night Drive");
    let parent = dir.path().join("Exports");
    std::fs::create_dir(&parent).unwrap();
    s.co_producer_begin("export the song".into()).unwrap();
    let good = shown(&parent);
    let cases: Vec<HostileExport> = vec![
        (W::Stems, DATE.into(), Some(good.clone()), Some(vec![])),
        (W::Stems, DATE.into(), Some(good.clone()), Some(vec![0])),
        (W::Stems, DATE.into(), Some(good.clone()), Some(vec![1, 1])),
        (
            W::Stems,
            DATE.into(),
            Some(good.clone()),
            Some(vec![u64::MAX]),
        ),
        (
            W::Stems,
            DATE.into(),
            Some(good.clone()),
            Some((1..=70_000).collect()),
        ),
        (W::Aaf, DATE.into(), Some(good.clone()), Some(vec![1])),
        (W::Mix, "x".repeat(70_000), Some(good.clone()), None),
        (W::Mix, "2026-11-1\0".into(), Some(good.clone()), None),
        (W::Mix, DATE.into(), Some("relative".into()), None),
        (
            W::Mix,
            DATE.into(),
            Some(format!("/{}", "x".repeat(70_000))),
            None,
        ),
        (W::Mix, DATE.into(), Some(format!("{good}\0")), None),
        (
            W::Mix,
            DATE.into(),
            Some(format!("{good}/../Exports")),
            None,
        ),
        (W::Mix, DATE.into(), Some(format!("{good}/")), None),
        (
            W::Mix,
            DATE.into(),
            Some(dir.path().join("missing").display().to_string()),
            None,
        ),
    ];
    for (what, date, parent_arg, strips) in cases {
        let r = s.co_producer_export_plan(what, date.clone(), parent_arg.clone(), strips.clone());
        assert!(
            matches!(r, Err(VizError::Model { .. })),
            "{what:?} {:?} {:?} {:?}: {r:?}",
            date.chars().take(20).collect::<String>(),
            parent_arg.map(|p| p.chars().take(40).collect::<String>()),
            strips.map(|v| v.len())
        );
    }
    assert!(names_in(&parent).is_empty(), "{:?}", names_in(&parent));
    s.co_producer_end().unwrap();
    s.close().unwrap();
}
