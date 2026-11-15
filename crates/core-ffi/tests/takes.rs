//! Takes in the lane snapshot, and choosing, renaming and deleting them
//! through `edit` (plan 5c-2 takes Task 1, Ruling 4). Offline only: no
//! audio device or MIDI port opens.

use core_engine::fixtures::{Builder, Q};
use core_model::command::{Command, LaneCmd};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::ids::{LaneId, SeqId, TakeId};
use serde_json::json;
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::control::{ControlResult, result_json};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{EditCommand, LaneEdit, LaneRow, Session, TakeRow};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

struct Fixture {
    doc: Document,
    s: SeqId,
    l: LaneId,
    t1: TakeId,
    t2: TakeId,
    t3: TakeId,
}

fn add_take(b: &mut Builder, s: SeqId, l: LaneId, name: &str) -> TakeId {
    b.run(Command::Lane(LaneCmd::AddTake {
        seq: s,
        lane: l,
        name: name.into(),
    }))
    .unwrap();
    b.doc().lane(s, l).unwrap().active_take
}

/// "Keys" with two notes in Take 1, one in Take 2 (or, with `region`, one
/// audio region over a pool item), and an empty, active Take 3.
fn fixture(region: bool) -> Fixture {
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (s, l) = b.sequence("S", "Keys", Some(piano)).unwrap();
    let t1 = b.doc().lane(s, l).unwrap().active_take;
    b.notes(s, l, &[(0, 60, 0x8000, Q), (Q, 62, 0x8000, Q)])
        .unwrap();
    let t2 = add_take(&mut b, s, l, "Take 2");
    if region {
        let media = b.media("Media/a.wav", 48_000, 2).unwrap();
        b.region(
            s,
            l,
            0,
            media,
            0,
            48_000,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    } else {
        b.notes(s, l, &[(2 * Q, 64, 0x8000, Q)]).unwrap();
    }
    let t3 = add_take(&mut b, s, l, "Take 3");
    Fixture {
        doc: b.finish(),
        s,
        l,
        t1,
        t2,
        t3,
    }
}

fn keys(s: &Session, f: &Fixture) -> LaneRow {
    s.lanes(f.s.0)
        .unwrap()
        .into_iter()
        .find(|r| r.id == f.l.0)
        .unwrap()
}

fn lane_edit(edit: LaneEdit) -> EditCommand {
    EditCommand::Lane { edit }
}

fn take(id: TakeId, name: &str, midi: u32, regions: u32) -> TakeRow {
    TakeRow {
        id: id.0,
        name: name.into(),
        midi_event_count: midi,
        region_count: regions,
    }
}

fn session(f: &Fixture) -> Arc<Session> {
    Session::from_document(f.doc.clone()).unwrap()
}

#[test]
fn lane_rows_list_takes() {
    let _g = serial();
    let f = fixture(false);
    let s = session(&f);
    let row = keys(&s, &f);
    assert_eq!(
        row.takes,
        vec![
            take(f.t1, "Take 1", 2, 0),
            take(f.t2, "Take 2", 1, 0),
            take(f.t3, "Take 3", 0, 0),
        ]
    );
    assert_eq!(row.active_take_id, f.t3.0);
    assert_eq!(row.take_count, 3);
    assert_eq!(row.active_take, "Take 3");

    let v = result_json(&ControlResult::Document {
        doc: "D".into(),
        path: None,
        snapshot: s.snapshot().unwrap(),
        selected_seq: Some(f.s.0),
        lanes: s.lanes(f.s.0).unwrap(),
        destinations: vec![],
        alert: None,
    });
    let l = v["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == json!(f.l.0))
        .unwrap();
    assert_eq!(
        l["take_list"][2],
        json!({"id": f.t3.0, "name": "Take 3", "active": true})
    );
    assert_eq!(
        l["take_list"][0],
        json!({"id": f.t1.0, "name": "Take 1", "active": false})
    );
    assert_eq!(l["takes"], json!(3));
    assert_eq!(l["active_take"], json!("Take 3"));
}

#[test]
fn choosing_renaming_and_deleting_takes_through_edit() {
    let _g = serial();
    let f = fixture(false);
    let s = session(&f);
    let (seq, lane) = (f.s.0, f.l.0);

    let snap = s
        .edit(
            lane_edit(LaneEdit::SetActiveTake {
                seq,
                lane,
                take: f.t1.0,
            }),
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Set Active Take"));
    assert!(snap.dirty);
    let row = keys(&s, &f);
    assert_eq!(row.midi_event_count, 2);
    assert_eq!(row.active_take_id, f.t1.0);

    let snap = s
        .edit(
            lane_edit(LaneEdit::RenameTake {
                seq,
                lane,
                take: f.t1.0,
                name: "Verse".into(),
            }),
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Rename Take"));
    let row = keys(&s, &f);
    assert_eq!(row.takes[0].name, "Verse");
    assert_eq!(row.active_take, "Verse");

    let snap = s
        .edit(
            lane_edit(LaneEdit::RemoveTake {
                seq,
                lane,
                take: f.t2.0,
            }),
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Delete Take"));
    let ids: Vec<u64> = keys(&s, &f).takes.iter().map(|t| t.id).collect();
    assert_eq!(ids, vec![f.t1.0, f.t3.0]);

    s.undo().unwrap();
    let row = keys(&s, &f);
    let ids: Vec<u64> = row.takes.iter().map(|t| t.id).collect();
    assert_eq!(ids, vec![f.t1.0, f.t2.0, f.t3.0]);
    assert_eq!(row.takes[1], take(f.t2, "Take 2", 1, 0));

    let before = s.snapshot().unwrap().undo_label;
    assert!(
        s.edit(
            lane_edit(LaneEdit::RemoveTake {
                seq,
                lane,
                take: u64::MAX,
            }),
            None,
        )
        .is_err()
    );
    assert_eq!(s.snapshot().unwrap().undo_label, before);
    assert_eq!(keys(&s, &f).takes.len(), 3);
}

#[test]
fn deleting_a_take_keeps_its_media() {
    let _g = serial();
    let f = fixture(true);
    let s = session(&f);
    let row = keys(&s, &f);
    assert_eq!(row.takes[1], take(f.t2, "Take 2", 0, 1));
    let media: Vec<u64> = s.media().unwrap().iter().map(|m| m.id).collect();
    assert_eq!(media.len(), 1);

    s.edit(
        lane_edit(LaneEdit::RemoveTake {
            seq: f.s.0,
            lane: f.l.0,
            take: f.t2.0,
        }),
        None,
    )
    .unwrap();
    let after: Vec<u64> = s.media().unwrap().iter().map(|m| m.id).collect();
    assert_eq!(after, media);

    s.undo().unwrap();
    let row = keys(&s, &f);
    assert_eq!(row.takes[1].id, f.t2.0);
    assert_eq!(row.takes[1].region_count, 1);
}
