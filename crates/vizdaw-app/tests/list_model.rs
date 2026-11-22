//! Swift `ListEditorModelTests` (without MIDI input, references and torn
//! windows), `ListEditorViewTests`' Return/Space rule and Insert Event.

mod support;

use std::sync::Arc;

use core_control::ErrorCode;
use support::{FakeSession, four_four, roll_fake};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget};
use vizdaw_app::list::{ListColumn, ListColumns, ListFilter, ListKind, ListText};
use vizdaw_app::{DocumentModel, Prefs, commands};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef, SequenceEdit};
use vizdaw_ffi::events::{EventRow, MarkerRow};

const Q: i64 = 15_360;

fn lane_target() -> EditorTarget {
    EditorTarget::Lane { seq: 3, lane: 31 }
}

fn list_model(target: EditorTarget) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(roll_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(target, Some(DetailEditorKind::List));
    (f, m)
}

fn ids(m: &DocumentModel) -> Vec<String> {
    m.list_rows().iter().map(|r| r.id().to_owned()).collect()
}

fn s(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|i| (*i).to_owned()).collect()
}

fn refusal(message: &str) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// The rows the last edit inserted.
fn inserted(f: &FakeSession) -> Vec<EventRow> {
    match f.edits().last() {
        Some(EditCommand::Event {
            edit: EventEdit::Insert { events, .. },
        }) => events.clone(),
        other => panic!("no insert: {other:?}"),
    }
}

#[test]
fn rows_follow_the_target() {
    let (_f, mut m) = list_model(lane_target());
    assert_eq!(ids(&m), ["e1", "e2"]);
    use ListColumn::*;
    assert_eq!(
        m.list_columns(),
        [Position, Duration, Kind, Dest, Data1, Data2, Mute]
    );
    m.open_detail(
        EditorTarget::Sequence { seq: 3 },
        Some(DetailEditorKind::List),
    );
    assert_eq!(
        ids(&m),
        ["t0", "m0", "e1", "e3", "e2"],
        "map rows, then lanes in order"
    );
    assert!(m.list_columns().contains(&Lane));
    m.open_detail(
        EditorTarget::Block {
            seq: 3,
            lane: 31,
            start: 0,
            end: Q,
        },
        Some(DetailEditorKind::List),
    );
    assert_eq!(ids(&m), ["e1"]);
    assert_eq!(
        m.list_context().dest_names.get(&5).map(String::as_str),
        Some("Piano")
    );
}

#[test]
fn rows_are_built_once_per_version_target_filter_and_columns() {
    let (_f, mut m) = list_model(lane_target());
    let a = m.list_rows();
    let b = m.list_rows();
    let _ = m.list_columns();
    let _ = m.list_context();
    assert!(Arc::ptr_eq(&a, &b), "the same rows, not a copy");
    assert_eq!(m.list().builds(), 1);
    m.list_toggle_kind(ListKind::Cc);
    let _ = m.list_rows();
    assert_eq!(m.list().builds(), 2, "the filter changed");
    m.set_list_prefs(vizdaw_app::list::ListPrefs {
        columns: ListColumns {
            end: true,
            ..ListColumns::default()
        },
        ..Default::default()
    });
    let _ = m.list_rows();
    assert_eq!(m.list().builds(), 3, "the columns changed");
    assert!(m.list_commit("3", "e1", ListColumn::Position));
    let _ = m.list_rows();
    let _ = m.list_rows();
    assert_eq!(m.list().builds(), 4, "a new snapshot");
}

#[test]
fn the_cursor_follows_a_moved_event() {
    let (f, mut m) = list_model(lane_target());
    assert!(m.list_commit("3", "e1", ListColumn::Position));
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Event {
            edit: EventEdit::Move {
                events: vec![1],
                delta: 2 * 61_440,
                to_lane: None
            }
        })
    );
    assert_eq!(m.list().cursor(), Some("e1"));
    let sel = m.selection().clone();
    assert!(
        sel.events == [1] && sel.lanes == [LaneRef { seq: 3, lane: 31 }],
        "the document selection follows"
    );
    assert_eq!(sel.time_range, None, "a list selection clears the range");
}

#[test]
fn a_refused_commit_leaves_the_document_and_says_why() {
    let (f, mut m) = list_model(lane_target());
    assert!(!m.list_commit("abc", "e1", ListColumn::Position));
    assert!(f.edits().is_empty());
    assert!(m.status().is_some_and(|s| s.contains("not a position")));
    assert!(
        m.list_commit("1.1.0", "e1", ListColumn::Position),
        "the same position changes nothing"
    );
    assert!(f.edits().is_empty());
}

#[test]
fn committing_a_row_that_vanished_does_nothing() {
    let (f, mut m) = list_model(lane_target());
    assert!(!m.list_commit("3", "e99", ListColumn::Position));
    assert!(f.edits().is_empty() && m.status().is_none());
}

#[test]
fn insert_repeats_the_last_kind() {
    let (f, mut m) = list_model(lane_target());
    assert_eq!(m.list().last_insert_kind(), ListKind::Note);
    m.list_insert(Some(ListKind::Cc));
    let Some(EditCommand::Event {
        edit:
            EventEdit::Insert {
                seq: 3,
                lane: 31,
                events,
            },
    }) = f.edits().last().cloned()
    else {
        panic!("no insert");
    };
    assert!(events[0].kind == vizdaw_ffi::events::EventKindRow::Cc && events[0].tick == 0);
    assert_eq!(m.list().last_insert_kind(), ListKind::Cc);
    m.list_insert(None);
    assert_eq!(f.edits().len(), 2);
    assert_eq!(
        inserted(&f)[0].kind,
        vizdaw_ffi::events::EventKindRow::Cc,
        "insert repeats the last inserted kind"
    );
    m.list_insert(Some(ListKind::Reference));
    assert_eq!(f.edits().len(), 2);
    assert_eq!(
        m.status(),
        Some("References and regions are made in the arrangement.")
    );
    m.list_insert(Some(ListKind::Tempo));
    assert_eq!(f.edits().len(), 2, "a lane target lists no tempo rows");
    assert_eq!(
        m.status(),
        Some("Open the whole sequence to add tempo, meter or marker rows.")
    );
    assert!(!m.list_insert_kinds().contains(&ListKind::Tempo));
    m.list_set_cursor(Some("e2".into()));
    m.list_insert(Some(ListKind::Note));
    assert_eq!(inserted(&f)[0].tick, Q, "at the cursor row");
}

#[test]
fn filters_apply_to_all_lists_or_become_the_default() {
    let (_f, mut m) = list_model(lane_target());
    let mut prefs = Prefs::in_memory();
    m.list_toggle_kind(ListKind::Note);
    assert!(m.list_rows().is_empty());
    prefs.set_list_default_filter(&m.list_filter()).unwrap();
    assert_eq!(
        prefs.list().default_filter.hidden_kinds,
        [ListKind::Note].into()
    );
    m.set_list_prefs(prefs.list());
    m.close_detail();
    m.open_detail(lane_target(), Some(DetailEditorKind::List));
    assert!(
        m.list_rows().is_empty(),
        "a new list starts from the default"
    );
    m.list_set_filter(ListFilter::default());
    assert_eq!(m.list_rows().len(), 2);
    m.list_toggle_dest(5);
    assert_eq!(m.list_filter().shown_dests, Some([6].into()));
    assert!(!m.list_shows_dest(5));
    assert!(m.list_rows().is_empty(), "Chords plays on destination 5");
    m.open_detail(
        EditorTarget::Lane { seq: 3, lane: 32 },
        Some(DetailEditorKind::List),
    );
    assert_eq!(
        m.list_filter().shown_dests,
        Some([6].into()),
        "the filter stays with the list"
    );
    prefs.set_list_default_filter(&m.list_filter()).unwrap();
    assert_eq!(
        prefs.list().default_filter.shown_dests,
        None,
        "destinations are not saved as a default"
    );
    m.list_toggle_dest(5);
    assert_eq!(
        m.list_filter().shown_dests,
        None,
        "every destination: no filter"
    );
}

#[test]
fn deleting_the_first_tempo_says_it_stays() {
    let (f, mut m) = list_model(EditorTarget::Sequence { seq: 3 });
    m.list_delete(&s(&["t0"]));
    assert!(f.edits().is_empty() && f.edit_alls().is_empty());
    assert_eq!(m.status(), Some("The first tempo change stays."));
    m.list_delete(&s(&["m0"]));
    assert_eq!(m.status(), Some("The first meter change stays."));
}

#[test]
fn deleting_mixed_rows_is_one_undo_step_all_or_nothing() {
    let f = Arc::new(roll_fake());
    let mut info = four_four(3);
    info.markers = vec![
        MarkerRow {
            tick: 0,
            name: "A".into(),
        },
        MarkerRow {
            tick: Q,
            name: "B".into(),
        },
    ];
    f.set_info(3, info);
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(
        EditorTarget::Sequence { seq: 3 },
        Some(DetailEditorKind::List),
    );
    m.list_set_cursor(Some("e1".into()));
    f.set_call_error("edit_all", Some(refusal("refused")));
    m.list_delete(&s(&["e1", "k0"]));
    assert!(
        f.edits().is_empty() && f.edit_alls().is_empty(),
        "nothing sent one by one"
    );
    assert_eq!(m.status(), Some("refused"));
    assert_eq!(
        m.list().cursor(),
        Some("e1"),
        "a refused delete keeps the cursor"
    );
    f.set_call_error("edit_all", None);
    m.list_delete(&s(&["e1", "k0"]));
    let alls = f.edit_alls();
    assert_eq!(alls.len(), 1);
    assert_eq!(alls[0].0, "Delete");
    assert_eq!(
        alls[0].1,
        [
            EditCommand::Event {
                edit: EventEdit::Delete { events: vec![1] }
            },
            EditCommand::Sequence {
                edit: SequenceEdit::SetMarkers {
                    seq: 3,
                    markers: vec![MarkerRow {
                        tick: Q,
                        name: "B".into()
                    }]
                }
            },
        ]
    );
    assert_eq!(m.list().cursor(), None);
    // One kind of row is a plain edit, named after itself.
    m.list_delete(&s(&["e2"]));
    assert_eq!(f.edit_alls().len(), 1);
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Event {
            edit: EventEdit::Delete { events: vec![2] }
        })
    );
}

#[test]
fn insert_uses_the_transport_only_on_its_own_sequence() {
    let (f, mut m) = list_model(lane_target());
    f.set_transport(|t| {
        t.seq = Some(4);
        t.tick = 30_720;
    });
    m.poll_transport().unwrap();
    m.list_insert(Some(ListKind::Cc));
    assert_eq!(
        inserted(&f)[0].tick,
        0,
        "the transport is on another sequence"
    );
    f.set_transport(|t| t.seq = Some(3));
    m.poll_transport().unwrap();
    m.list_insert(Some(ListKind::Cc));
    assert_eq!(inserted(&f)[0].tick, 30_720);
}

#[test]
fn insert_ignores_a_playing_transport() {
    let (f, mut m) = list_model(lane_target());
    f.set_transport(|t| {
        t.playing = true;
        t.seq = Some(3);
        t.tick = 30_720;
    });
    m.poll_transport().unwrap();
    m.list_insert(Some(ListKind::Cc));
    assert_eq!(inserted(&f)[0].tick, 0);
}

#[test]
fn a_core_error_from_an_edit_reaches_the_status_line() {
    let (f, mut m) = list_model(lane_target());
    f.set_edit_error(Some(refusal("lane is locked")));
    assert!(!m.list_commit("3", "e1", ListColumn::Position));
    assert_eq!(m.status(), Some("lane is locked"));
}

#[test]
fn columns_and_smpte_come_from_the_preferences() {
    let (_f, mut m) = list_model(lane_target());
    let mut prefs = Prefs::in_memory();
    prefs
        .set_list_columns(ListColumns {
            smpte: true,
            fps: 25,
            ..ListColumns::default()
        })
        .unwrap();
    m.set_list_prefs(prefs.list());
    assert!(m.list_columns().contains(&ListColumn::Smpte));
    let row = m.list_row("e2").unwrap();
    assert_eq!(
        ListText::cell(&row, ListColumn::Smpte, &m.list_context()),
        "00:00:00:12",
        "a quarter at 120 BPM is half a second"
    );
    assert_eq!(prefs.list().columns.fps, 25);
}

#[test]
fn list_prefs_are_stored_under_the_list_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = Prefs::file_in(dir.path());
    let mut p = Prefs::at(&path);
    assert_eq!(p.list(), Default::default());
    p.set_list_columns(ListColumns {
        release: true,
        fps: 24,
        ..ListColumns::default()
    })
    .unwrap();
    p.set_list_default_filter(&ListFilter::new([ListKind::Cc], Some([5].into())))
        .unwrap();
    let back = Prefs::at(&path);
    assert_eq!(
        back.raw("list"),
        Some(&serde_json::json!({
            "defaultFilter": {"hiddenKinds": ["cc"]},
            "columns": {"end": false, "duration": true, "release": true, "smpte": false, "fps": 24}
        }))
    );
    assert_eq!(back.list().columns.fps, 24);
    // A damaged value is the defaults.
    std::fs::write(&path, r#"{"list": 7}"#).unwrap();
    assert_eq!(Prefs::at(&path).list(), Default::default());
}

#[test]
fn insert_event_needs_a_list_with_the_keyboard() {
    let f = Arc::new(roll_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    const NEEDS: &str = "Insert Event needs a list editor that has the keyboard.";
    let e = commands::run("editors.insert_event", &[], &mut m).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::Unavailable, NEEDS)
    );
    assert!(!commands::enabled("editors.insert_event", &m));
    m.open_detail(lane_target(), Some(DetailEditorKind::PianoRoll));
    assert!(!commands::enabled("editors.insert_event", &m), "not a list");
    m.open_detail(lane_target(), Some(DetailEditorKind::List));
    assert!(commands::enabled("editors.insert_event", &m));
    m.begin_text_editing("list.cell");
    assert!(!commands::enabled("editors.insert_event", &m));
    let e = commands::perform_from_menu("editors.insert_event", &mut m).unwrap_err();
    assert_eq!(e.message, NEEDS);
    assert_eq!(m.status(), Some(NEEDS));
    assert!(f.edits().is_empty());
    m.end_text_editing("list.cell");
    commands::run("editors.insert_event", &[], &mut m).unwrap();
    assert_eq!(inserted(&f)[0].kind, vizdaw_ffi::events::EventKindRow::Note);
}

#[test]
fn a_focused_list_keeps_space_but_not_return() {
    let (_f, mut m) = list_model(lane_target());
    assert!(m.return_key_enabled());
    m.set_list_focused(true);
    assert!(m.list_focused());
    assert!(!m.return_key_enabled(), "Return no longer returns to start");
    assert!(m.transport_keys_enabled(), "Space still plays");
    assert!(commands::enabled("transport.play_stop", &m));
    assert!(!commands::enabled("transport.return_to_start", &m));
    m.set_list_focused(false);
    assert!(commands::enabled("transport.return_to_start", &m));
}

#[test]
fn hostile_commits_leave_at_most_one_edit() {
    let (f, mut m) = list_model(lane_target());
    for text in [
        "5..",
        "99999999999",
        "H9",
        "",
        "-",
        "1.1.99999",
        "\u{d7}",
        "::",
    ] {
        for col in ListColumn::ALL {
            let before = f.edits().len();
            m.list_commit(text, "e1", col);
            assert!(f.edits().len() <= before + 1, "{text:?} in {col:?}");
            m.list_set_cursor(None);
        }
    }
    assert!(f.edit_alls().is_empty(), "a commit is never a batch");
    let refused = [
        (
            "H9",
            ListColumn::Data1,
            "\"H9\" is not a pitch: type a name such as C4 or a number 0 to 127.",
        ),
        (
            "99999999999",
            ListColumn::Position,
            "\"99999999999\" is not a position: type a bar, bar.beat or bar.beat.unit.",
        ),
        (
            "1.1.99999",
            ListColumn::Position,
            "\"1.1.99999\" is not a position: type a bar, bar.beat or bar.beat.unit.",
        ),
    ];
    for (text, col, status) in refused {
        m.set_status(None);
        let before = f.edits().len();
        assert!(!m.list_commit(text, "e1", col), "{text}");
        assert_eq!(f.edits().len(), before, "{text}: nothing sent");
        assert_eq!(m.status(), Some(status));
    }
    // "5.." names the bar only: e2 at 1.2.0 goes to 5.2.0.
    let before = f.edits().len();
    assert!(m.list_commit("5..", "e2", ListColumn::Position));
    assert_eq!(f.edits().len(), before + 1);
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Event {
            edit: EventEdit::Move {
                events: vec![2],
                delta: 4 * 61_440,
                to_lane: None
            }
        })
    );
}

#[test]
fn selecting_and_deleting_many_rows_of_a_long_list_is_quick() {
    let f = Arc::new(roll_fake());
    let events: Vec<EventRow> = (0..100_000)
        .map(|i| support::note_row(i + 1, i as i64 * 240, 60))
        .collect();
    f.set_events(3, 31, events);
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(lane_target(), Some(DetailEditorKind::List));
    assert_eq!(m.list_rows().len(), 100_000);
    let chosen: Vec<String> = (0..10_000).map(|i| format!("e{}", i * 10 + 1)).collect();
    let start = std::time::Instant::now();
    m.list_select(&chosen);
    assert_eq!(m.selection().events.len(), 10_000);
    m.list_delete(&chosen);
    let elapsed = start.elapsed();
    let Some(EditCommand::Event {
        edit: EventEdit::Delete { events },
    }) = f.edits().last().cloned()
    else {
        panic!("no delete");
    };
    assert_eq!(events.len(), 10_000);
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "debug build bound: {elapsed:?}"
    );
}
