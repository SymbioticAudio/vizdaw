//! Co-Producer requests as one named undo step (plan 8a Task 2, Rulings 3
//! and 4): only Co-Producer's own steps go in a request's group, a hand
//! edit closes it, and undoing a Co-Producer step never removes the
//! composer's edits.

use core_model::ModelError;
use core_model::command::{Command, LaneCmd, SequenceCmd};
use core_model::document::Document;
use core_model::history::{History, request_label};
use core_model::ids::{LaneId, SeqId};
use core_model::selection::ViewFilter;
use proptest::prelude::*;

fn f() -> ViewFilter {
    ViewFilter::default()
}

fn add_lane(name: &str) -> Command {
    Command::Lane(LaneCmd::Add {
        seq: SeqId(0),
        name: name.into(),
        index: None,
    })
}

fn rename_lane(lane: LaneId, name: &str) -> Command {
    Command::Lane(LaneCmd::Rename {
        seq: SeqId(0),
        lane,
        name: name.into(),
    })
}

fn remove_lane(lane: LaneId) -> Command {
    Command::Lane(LaneCmd::Remove {
        seq: SeqId(0),
        lane,
    })
}

fn lanes(h: &History) -> Vec<LaneId> {
    h.doc()
        .sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .map(|l| l.id)
        .collect()
}

fn lane_name(h: &History, lane: LaneId) -> String {
    h.doc()
        .sequence(SeqId(0))
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.id == lane)
        .unwrap()
        .name
        .clone()
}

/// History whose present has seq0 "S" (with lane0) and nothing to undo.
fn hist() -> History {
    let mut h = History::new(Document::new());
    h.execute(
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
        &f(),
    )
    .unwrap();
    History::new(h.doc().clone())
}

/// Runs `cmd` as Co-Producer.
fn acting(h: &mut History, cmd: &Command) {
    h.set_acting(true).unwrap();
    h.execute(cmd, &f()).unwrap();
    h.set_acting(false).unwrap();
}

fn invalid_reason(e: ModelError) -> String {
    match e {
        ModelError::Invalid { reason } => reason,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn labels_are_title_case_and_short() {
    assert_eq!(
        request_label("set up a drum bus").unwrap(),
        (
            "Set Up a Drum Bus".to_string(),
            "Co-Producer: Set Up a Drum Bus".to_string()
        )
    );
    assert_eq!(request_label("  the   kick  ").unwrap().0, "The Kick");
    let long = "x".repeat(60);
    let (title, label) = request_label(&long).unwrap();
    assert_eq!(title.chars().count(), 27);
    assert!(title.ends_with('…'));
    assert_eq!(label.chars().count(), 40);
    for bad in [
        "",
        "   ",
        "a\u{0}b",
        "a\nb",
        "a\u{202E}b",
        "a\u{200F}b",
        "a\u{2028}b",
    ] {
        let e = request_label(bad).unwrap_err();
        assert!(matches!(e, ModelError::Invalid { .. }), "{bad:?}: {e:?}");
    }
    assert!(matches!(
        request_label(&"y".repeat(201)),
        Err(ModelError::Invalid { .. })
    ));
    assert!(request_label(&"y".repeat(200)).is_ok());
}

#[test]
fn a_request_is_one_undo_step() {
    let mut h = hist();
    let original = h.doc().clone();
    h.begin_request("add two tracks").unwrap();
    h.set_acting(true).unwrap();
    h.execute(&add_lane("a"), &f()).unwrap();
    h.execute(&add_lane("b"), &f()).unwrap();
    h.set_acting(false).unwrap();
    assert_eq!(
        h.end_request().unwrap().as_deref(),
        Some("Co-Producer: Add Two Tracks")
    );
    assert!(h.request().is_none());
    assert_eq!(h.undo_label(), Some("Co-Producer: Add Two Tracks"));
    assert_eq!(h.undo_depth(), 1);
    let after = h.doc().clone();
    h.undo().unwrap();
    assert_eq!(h.doc(), &original);
    h.redo().unwrap();
    assert_eq!(h.doc(), &after);
    assert_eq!(lanes(&h).len(), 3);
}

#[test]
fn a_request_that_made_nothing_adds_no_step() {
    let mut h = hist();
    h.execute(&rename_lane(LaneId(0), "x"), &f()).unwrap();
    h.undo().unwrap();
    let (depth, redo) = (h.undo_depth(), h.redo_label().map(str::to_string));
    h.begin_request("nothing").unwrap();
    h.set_acting(true).unwrap();
    // Renaming to its own name changes nothing.
    let same = lane_name(&h, LaneId(0));
    h.execute(&rename_lane(LaneId(0), &same), &f()).unwrap();
    h.set_acting(false).unwrap();
    assert_eq!(h.end_request().unwrap(), None);
    assert_eq!(h.undo_depth(), depth);
    assert_eq!(h.redo_label().map(str::to_string), redo);
}

#[test]
fn a_hand_edit_closes_the_request_and_the_next_step_continues() {
    let mut h = hist();
    h.begin_request("add two tracks").unwrap();
    acting(&mut h, &add_lane("a"));
    assert!(h.request().unwrap().group_open);
    let rename = rename_lane(LaneId(0), "Mine");
    h.execute(&rename, &f()).unwrap();
    let info = h.request().unwrap();
    assert!(!info.group_open);
    assert!(info.continued);
    acting(&mut h, &add_lane("b"));
    assert_eq!(
        h.end_request().unwrap().as_deref(),
        Some("Co-Producer: Add Two Tracks (continued)")
    );
    assert_eq!(h.undo_depth(), 3);
    let mut labels = Vec::new();
    let mut probe = h.clone();
    while let Some(l) = probe.undo_label().map(str::to_string) {
        labels.push(l);
        probe.undo().unwrap();
    }
    assert_eq!(
        labels,
        vec![
            "Co-Producer: Add Two Tracks (continued)".to_string(),
            rename.name().to_string(),
            "Co-Producer: Add Two Tracks".to_string(),
        ]
    );
    h.undo().unwrap();
    assert_eq!(lane_name(&h, LaneId(0)), "Mine");
    assert_eq!(lanes(&h).len(), 2);
}

#[test]
fn undo_and_redo_close_the_request_group_first() {
    for acting_undo in [false, true] {
        let mut h = hist();
        let original = h.doc().clone();
        h.begin_request("two tracks").unwrap();
        h.set_acting(true).unwrap();
        h.execute(&add_lane("a"), &f()).unwrap();
        h.execute(&add_lane("b"), &f()).unwrap();
        if !acting_undo {
            h.set_acting(false).unwrap();
        }
        h.undo().unwrap();
        assert_eq!(h.doc(), &original, "acting undo: {acting_undo}");
        assert!(!h.request().unwrap().group_open);
        assert_eq!(h.redo_label(), Some("Co-Producer: Two Tracks"));
        h.redo().unwrap();
        assert_eq!(lanes(&h).len(), 3);
        h.set_acting(false).unwrap();
        h.end_request().unwrap();
    }
    // Redo closes it too.
    let mut h = hist();
    h.execute(&add_lane("mine"), &f()).unwrap();
    h.undo().unwrap();
    h.begin_request("rename").unwrap();
    acting(&mut h, &rename_lane(LaneId(0), "Theirs"));
    // The step cleared the redo stack; the redo closes the group first.
    assert!(h.redo().is_err());
    assert!(!h.request().unwrap().group_open);
    assert_eq!(h.undo_label(), Some("Co-Producer: Rename"));
}

#[test]
fn recording_closes_the_request_group() {
    let mut h = hist();
    let original = h.doc().clone();
    h.begin_request("two tracks").unwrap();
    h.set_acting(true).unwrap();
    h.execute(&add_lane("a"), &f()).unwrap();
    h.execute(&add_lane("b"), &f()).unwrap();
    h.set_acting(false).unwrap();
    h.begin_group().unwrap();
    assert!(h.in_group());
    assert!(!h.request().unwrap().group_open);
    h.execute(&rename_lane(LaneId(0), "Take"), &f()).unwrap();
    h.end_group("Record").unwrap();
    assert_eq!(h.undo_label(), Some("Record"));
    h.undo().unwrap();
    assert_eq!(h.undo_label(), Some("Co-Producer: Two Tracks"));
    h.undo().unwrap();
    assert_eq!(h.doc(), &original);
}

#[test]
fn acting_is_busy_during_a_drag_or_a_recording() {
    let mut h = hist();
    let original = h.doc().clone();
    h.begin_request("busy").unwrap();
    assert_eq!(h.acting_refusal(), None);

    h.begin("Drag").unwrap();
    assert_eq!(h.acting_refusal(), Some("Finish the drag first."));
    let e = h.set_acting(true).unwrap_err();
    assert_eq!(invalid_reason(e), "Finish the drag first.");
    assert!(!h.acting());
    h.commit().unwrap();

    h.begin_group().unwrap();
    assert_eq!(h.acting_refusal(), Some("Stop recording first."));
    let e = h.set_acting(true).unwrap_err();
    assert_eq!(invalid_reason(e), "Stop recording first.");
    assert!(!h.acting());
    h.end_group("Record").unwrap();

    assert_eq!(h.doc(), &original);
    h.set_acting(true).unwrap();
    assert!(h.acting());
    h.set_acting(false).unwrap();
    assert!(!h.acting());
}

#[test]
fn only_one_request_at_a_time() {
    let mut h = hist();
    let info = h.begin_request("set up a drum bus").unwrap();
    assert_eq!(info.name, "set up a drum bus");
    assert_eq!(info.title, "Set Up a Drum Bus");
    assert_eq!(info.label, "Co-Producer: Set Up a Drum Bus");
    assert!(!info.group_open);
    assert!(!info.continued);
    assert_eq!(h.request(), Some(info));
    let e = h.begin_request("another").unwrap_err();
    assert!(
        invalid_reason(e).contains("Co-Producer request \"Co-Producer: Set Up a Drum Bus\""),
        "names the open request"
    );
    assert_eq!(h.end_request().unwrap(), None);
    assert!(h.request().is_none());
    assert!(h.begin_request("another").is_ok());
    assert!(h.begin_request("").is_err());
}

#[derive(Debug, Clone)]
enum Op {
    Add,
    Rename(usize, usize),
    Delete(usize),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        Just(Op::Add),
        (any::<usize>(), 0usize..3).prop_map(|(i, n)| Op::Rename(i, n)),
        any::<usize>().prop_map(Op::Delete),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn prop_request_then_undo_is_the_original(ops in prop::collection::vec(op(), 1..=12)) {
        let mut h = hist();
        h.execute(&add_lane("l"), &f()).unwrap();
        let mut h = History::new(h.doc().clone());
        let original = h.doc().clone();
        let depth = h.undo_depth();
        h.begin_request("random steps").unwrap();
        h.set_acting(true).unwrap();
        for o in &ops {
            let ids = lanes(&h);
            let cmd = match *o {
                Op::Add => Some(add_lane("l")),
                Op::Rename(i, n) if !ids.is_empty() => {
                    Some(rename_lane(ids[i % ids.len()], ["l", "Lane 1", "m"][n]))
                }
                Op::Delete(i) if !ids.is_empty() => Some(remove_lane(ids[i % ids.len()])),
                _ => None,
            };
            if let Some(cmd) = cmd {
                let _ = h.execute(&cmd, &f());
            }
        }
        h.set_acting(false).unwrap();
        match h.end_request().unwrap() {
            Some(label) => {
                prop_assert_eq!(label.as_str(), "Co-Producer: Random Steps");
                prop_assert_eq!(h.undo_depth(), depth + 1);
                h.undo().unwrap();
                prop_assert_eq!(h.doc(), &original);
            }
            None => {
                prop_assert_eq!(h.doc(), &original);
                prop_assert_eq!(h.undo_depth(), depth);
            }
        }
    }
}

/// Review I1: a transaction Co-Producer leaves open when acting turns off
/// is aborted, so a later hand edit never joins Co-Producer's step.
#[test]
fn turning_acting_off_aborts_a_co_producer_transaction() {
    let mut h = hist();
    let original = h.doc().clone();
    h.begin_request("two").unwrap();
    h.set_acting(true).unwrap();
    h.begin("Drag").unwrap();
    h.execute(&add_lane("a"), &f()).unwrap();
    h.set_acting(false).unwrap();
    assert!(!h.in_transaction(), "the acting transaction is aborted");
    assert_eq!(h.doc(), &original);
    h.execute(&rename_lane(LaneId(0), "Mine"), &f()).unwrap();
    assert!(h.commit().is_err());
    assert_eq!(h.end_request().unwrap(), None);
    h.undo().unwrap();
    assert_eq!(h.doc(), &original);
    assert!(h.undo().is_err(), "the rename was its own step");
    h.redo().unwrap();
    assert_eq!(lane_name(&h, LaneId(0)), "Mine");

    // A hand transaction is not touched by turning acting off.
    let mut h = hist();
    h.begin("Drag").unwrap();
    h.set_acting(false).unwrap();
    assert!(h.in_transaction());
}

/// Review I2: between calls, the undo label, depth and peek describe the
/// one step `undo` will close the open request group into.
#[test]
fn undo_label_depth_and_peek_see_the_closed_group() {
    let mut h = hist();
    h.execute(&rename_lane(LaneId(0), "Mine"), &f()).unwrap();
    let before = h.doc().clone();
    h.begin_request("add two tracks").unwrap();
    acting(&mut h, &add_lane("a"));
    acting(&mut h, &add_lane("b"));
    assert!(h.request().unwrap().group_open);
    assert_eq!(h.undo_label(), Some("Co-Producer: Add Two Tracks"));
    assert_eq!(h.undo_depth(), 2);
    assert_eq!(h.peek_undo(), Some(&before));
    assert_eq!(h.peek_redo(), None);
    let mut probe = h.clone();
    probe.undo().unwrap();
    assert_eq!(probe.doc(), &before);
    assert_eq!(probe.undo_depth(), 1);

    // A hand edit, then a continued group.
    h.execute(&rename_lane(LaneId(0), "Again"), &f()).unwrap();
    let before = h.doc().clone();
    acting(&mut h, &add_lane("c"));
    assert_eq!(
        h.undo_label(),
        Some("Co-Producer: Add Two Tracks (continued)")
    );
    assert_eq!(h.undo_depth(), 4);
    assert_eq!(h.peek_undo(), Some(&before));

    // An open group that made no step changes nothing.
    let mut h = hist();
    h.execute(&rename_lane(LaneId(0), "Mine"), &f()).unwrap();
    h.begin_request("nothing").unwrap();
    assert_eq!(h.undo_label(), Some(rename_lane(LaneId(0), "x").name()));
    assert_eq!(h.undo_depth(), 1);
}
