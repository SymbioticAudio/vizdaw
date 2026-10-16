use core_model::command::{Command, EventCmd, SequenceCmd, SettingsCmd};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{EventId, LaneId, SeqId};
use core_model::modify::{Modify, Transpose};
use core_model::rule::{Rule, SelectMode};
use core_model::selection::{Selection, ViewFilter};
use core_model::time::{PPQ, Tick};

fn f() -> ViewFilter {
    ViewFilter::default()
}

fn new_seq(name: &str) -> Command {
    Command::Sequence(SequenceCmd::New { name: name.into() })
}

fn rename(name: &str) -> Command {
    Command::Sequence(SequenceCmd::Rename {
        seq: SeqId(0),
        name: name.into(),
    })
}

fn insert_note(pitch: u8) -> Command {
    Command::Event(EventCmd::Insert {
        seq: SeqId(0),
        lane: LaneId(0),
        events: vec![EventSpec::new(
            Tick(0),
            EventBody::note(pitch, 100, Tick(PPQ)),
        )],
    })
}

fn name(h: &History) -> String {
    h.doc().sequence(SeqId(0)).unwrap().name.clone()
}

/// History whose present has seq0 "S" and nothing to undo.
fn hist() -> History {
    let mut h = History::new(Document::new());
    h.execute(&new_seq("S"), &f()).unwrap();
    History::new(h.doc().clone())
}

#[test]
fn undo_and_redo_walk_versions_with_labels() {
    let mut h = hist();
    let original = h.doc().clone();
    h.execute(&rename("A"), &f()).unwrap();
    h.execute(&rename("B"), &f()).unwrap();
    assert_eq!(h.undo_label(), Some("Rename Sequence"));
    h.undo().unwrap();
    assert_eq!(name(&h), "A");
    assert_eq!(h.redo_label(), Some("Rename Sequence"));
    h.undo().unwrap();
    assert_eq!(h.doc(), &original);
    assert!(h.undo().is_err());
    h.redo().unwrap();
    h.redo().unwrap();
    assert_eq!(name(&h), "B");
    assert!(h.redo().is_err());
}

#[test]
fn new_command_clears_redo() {
    let mut h = hist();
    h.execute(&rename("A"), &f()).unwrap();
    h.undo().unwrap();
    h.execute(&rename("C"), &f()).unwrap();
    assert!(h.redo().is_err());
}

#[test]
fn failed_command_changes_nothing() {
    let mut h = hist();
    let before = h.doc().clone();
    assert!(h.execute(&insert_note(200), &f()).is_err());
    assert_eq!(h.doc(), &before);
    assert_eq!(h.undo_depth(), 0);
}

#[test]
fn depth_limit_drops_oldest() {
    let mut h = hist();
    h.execute(
        &Command::Settings(SettingsCmd::SetHistoryDepth { depth: 3 }),
        &f(),
    )
    .unwrap();
    for i in 0..5 {
        h.execute(&rename(&format!("n{i}")), &f()).unwrap();
    }
    assert_eq!(h.undo_depth(), 3);
    for _ in 0..3 {
        h.undo().unwrap();
    }
    assert_eq!(name(&h), "n1");
    assert!(h.undo().is_err());
}

#[test]
fn transaction_is_one_undo_step() {
    let mut h = hist();
    let before = h.doc().clone();
    h.begin("Record").unwrap();
    h.execute(&insert_note(60), &f()).unwrap();
    h.execute(&insert_note(62), &f()).unwrap();
    // Inside the pass, steps undo individually.
    h.undo().unwrap();
    h.redo().unwrap();
    h.commit().unwrap();
    assert_eq!(h.undo_label(), Some("Record"));
    let after = h.doc().clone();
    h.undo().unwrap();
    assert_eq!(h.doc(), &before);
    h.redo().unwrap();
    assert_eq!(h.doc(), &after);
}

// Review focus #5: undo after a failed command inside a transaction.
#[test]
fn undo_after_failed_command_in_transaction() {
    let mut h = hist();
    let before = h.doc().clone();
    h.begin("Drag").unwrap();
    h.execute(&insert_note(60), &f()).unwrap();
    let mid = h.doc().clone();
    assert!(h.execute(&insert_note(255), &f()).is_err());
    assert_eq!(h.doc(), &mid);
    h.commit().unwrap();
    assert_eq!(h.undo_depth(), 1);
    h.undo().unwrap();
    assert_eq!(h.doc(), &before);
    h.redo().unwrap();
    assert_eq!(h.doc(), &mid);
}

#[test]
fn undo_cannot_cross_transaction_start_and_abort_restores() {
    let mut h = hist();
    h.execute(&rename("A"), &f()).unwrap();
    let before = h.doc().clone();
    h.begin("Gesture").unwrap();
    assert!(h.begin("Nested").is_err());
    assert!(h.undo().is_err());
    h.execute(&rename("B"), &f()).unwrap();
    h.abort().unwrap();
    assert_eq!(h.doc(), &before);
    assert_eq!(h.undo_label(), Some("Rename Sequence"));
    assert!(h.commit().is_err());
    // An empty transaction adds no step.
    h.begin("Empty").unwrap();
    h.commit().unwrap();
    assert_eq!(h.undo_depth(), 1);
}

#[test]
fn selection_changes_are_not_undo_steps_and_modify_again_replays() {
    let mut h = hist();
    h.execute(&insert_note(60), &f()).unwrap();
    let depth = h.undo_depth();
    h.set_selection(Selection {
        time_range: None,
        lanes: [(SeqId(0), LaneId(0))].into_iter().collect(),
        events: Default::default(),
    });
    h.execute(
        &Command::SelectAndModify {
            rule: Some(Rule::All(vec![])),
            mode: SelectMode::Select,
            modify: None,
            to_copy: false,
        },
        &f(),
    )
    .unwrap();
    assert_eq!(h.undo_depth(), depth);
    assert_eq!(h.selection().events.len(), 1);
    let up = Modify::Transpose(Transpose::Interval { semitones: 1 });
    h.execute(
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(up.clone()),
            to_copy: false,
        },
        &f(),
    )
    .unwrap();
    assert_eq!(h.last_modify(), Some(&up));
    h.modify_again(&f()).unwrap();
    assert_eq!(
        h.doc().event(EventId(0)).unwrap().0.body,
        EventBody::note(62, 100, Tick(PPQ))
    );
    assert_eq!(h.undo_label(), Some("Transpose"));
    // Undoing past the insert drops the vanished event from the selection.
    h.undo().unwrap();
    h.undo().unwrap();
    h.undo().unwrap();
    assert!(h.selection().events.is_empty());
}

#[test]
fn modify_again_without_history_is_invalid() {
    let mut h = hist();
    assert!(h.modify_again(&f()).is_err());
}

#[test]
fn undo_inside_a_transaction_then_commit_leaves_nothing_to_redo() {
    let mut h = hist();
    h.begin("Gesture").unwrap();
    h.execute(&rename("A"), &f()).unwrap();
    h.execute(&rename("B"), &f()).unwrap();
    h.undo().unwrap();
    h.commit().unwrap();
    assert_eq!(name(&h), "A");
    // B was an intra-transaction state; it must not be reachable by redo.
    assert!(h.redo().is_err());
    h.undo().unwrap();
    assert_eq!(name(&h), "S");
    h.redo().unwrap();
    assert_eq!(name(&h), "A");
}

#[test]
fn empty_transaction_keeps_the_redo_stack() {
    let mut h = hist();
    h.execute(&rename("A"), &f()).unwrap();
    h.undo().unwrap();
    h.begin("Nothing").unwrap();
    h.commit().unwrap();
    h.redo().unwrap();
    assert_eq!(name(&h), "A");
}

#[test]
fn no_op_command_adds_no_undo_step() {
    let mut h = hist();
    h.execute(&insert_note(60), &f()).unwrap();
    h.execute(&rename("A"), &f()).unwrap();
    h.undo().unwrap();
    let depth = h.undo_depth();
    // Nothing is selected, so the transpose changes nothing.
    let up = Modify::Transpose(Transpose::Interval { semitones: 1 });
    h.execute(
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(up.clone()),
            to_copy: false,
        },
        &f(),
    )
    .unwrap();
    assert_eq!(h.undo_depth(), depth);
    assert_eq!(h.last_modify(), Some(&up));
    // A no-op does not discard the redo stack either.
    h.redo().unwrap();
    assert_eq!(name(&h), "A");
}
