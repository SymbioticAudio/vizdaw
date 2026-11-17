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
