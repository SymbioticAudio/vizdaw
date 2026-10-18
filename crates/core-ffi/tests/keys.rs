//! The transport keys' reads and edits (plan 4f Task 3): what Play
//! Selection plays, marker steps, record-arm moves.

use vizdaw_ffi::{
    DocSnapshot, EditCommand, LaneEdit, MarkerRow, SelectionRow, SequenceEdit, Session, TickRange,
    VizError,
};

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

fn id_of(s: &DocSnapshot, name: &str) -> u64 {
    s.sequences.iter().find(|r| r.name == name).unwrap().id
}

fn song() -> (std::sync::Arc<Session>, u64) {
    let s = Session::new_demo().unwrap();
    let song = id_of(&s.snapshot().unwrap(), "Song");
    (s, song)
}

fn select(s: &Session, range: Option<TickRange>, events: Vec<u64>) {
    s.set_selection(SelectionRow {
        time_range: range,
        lanes: vec![],
        events,
    })
    .unwrap();
}

#[test]
fn play_selection_plays_the_range_else_the_selected_events() {
    let (s, song) = song();
    assert_eq!(s.selection_span(song).unwrap(), None, "nothing selected");
    let range = TickRange {
        start: BAR,
        end: 2 * BAR,
    };
    select(&s, Some(range), vec![]);
    assert_eq!(s.selection_span(song).unwrap(), Some(range));
    let chords = s.lanes(song).unwrap()[1].id;
    let notes = s.lane_events(song, chords).unwrap().events;
    let (a, b) = (&notes[0], &notes[notes.len() - 1]);
    select(&s, None, vec![b.id, a.id]);
    let end = notes
        .iter()
        .filter(|e| e.id == a.id || e.id == b.id)
        .map(|e| e.tick + e.duration.max(1))
        .max()
        .unwrap();
    assert_eq!(
        s.selection_span(song).unwrap(),
        Some(TickRange { start: a.tick, end }),
        "from the first start to the last end"
    );
    // Events of another sequence do not count.
    let beat = id_of(&s.snapshot().unwrap(), "Beat");
    assert_eq!(s.selection_span(beat).unwrap(), None);
    assert!(matches!(
        s.selection_span(9_999),
        Err(VizError::Model { .. })
    ));
}

#[test]
fn markers_step_from_a_position() {
    let (s, song) = song();
    s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::SetMarkers {
                seq: song,
                markers: vec![
                    MarkerRow {
                        tick: 4 * BAR,
                        name: "Verse".into(),
                    },
                    MarkerRow {
                        tick: BAR,
                        name: "Intro".into(),
                    },
                ],
            },
        },
        None,
    )
    .unwrap();
    assert_eq!(s.marker_step(song, 0, true).unwrap(), Some(BAR));
    assert_eq!(s.marker_step(song, BAR, true).unwrap(), Some(4 * BAR));
    assert_eq!(s.marker_step(song, 4 * BAR, true).unwrap(), None);
    assert_eq!(s.marker_step(song, 4 * BAR, false).unwrap(), Some(BAR));
    assert_eq!(s.marker_step(song, BAR, false).unwrap(), None);
    assert!(s.marker_step(9_999, 0, true).is_err());
}

#[test]
fn record_arm_moves_between_lanes_as_one_undo_step() {
    let (s, song) = song();
    let lanes =
        |s: &Session| -> Vec<bool> { s.lanes(song).unwrap().iter().map(|l| l.arm).collect() };
    assert!(lanes(&s).len() >= 2, "the demo song has lanes");
    assert!(lanes(&s).iter().all(|a| !a));
    let snap = s.move_record_arm(song, true).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Move Record Arm"));
    assert!(lanes(&s)[0], "none armed: the first lane");
    s.move_record_arm(song, true).unwrap();
    let after = lanes(&s);
    assert!(!after[0] && after[1], "one lane on, the old one off");
    s.move_record_arm(song, false).unwrap();
    assert!(lanes(&s)[0]);
    let e = s.move_record_arm(song, false).unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
    s.undo().unwrap();
    let back = lanes(&s);
    assert!(!back[0] && back[1], "one undo step per move");
    // Two armed lanes, a third after them: moving arms the third alone.
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Add {
                seq: song,
                name: "Third".into(),
                index: Some(2),
            },
        },
        None,
    )
    .unwrap();
    let first = s.lanes(song).unwrap()[0].id;
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: song,
                lane: first,
                mute: None,
                solo: None,
                arm: Some(true),
            },
        },
        None,
    )
    .unwrap();
    assert_eq!(&lanes(&s)[..3], &[true, true, false]);
    s.move_record_arm(song, true).unwrap();
    assert_eq!(&lanes(&s)[..3], &[false, false, true]);
}

fn cmd() -> vizdaw_ffi::ModifiersRow {
    vizdaw_ffi::ModifiersRow {
        command: true,
        ..Default::default()
    }
}

#[test]
fn the_keymap_lists_every_command_with_its_keys() {
    let k = vizdaw_ffi::Keymap::new();
    let rows = k.rows();
    assert_eq!(rows.len(), core_control::registry::COMMANDS.len());
    let save = rows.iter().find(|r| r.id == "file.save").unwrap();
    assert_eq!(save.keys, vec!["⌘S"]);
    assert_eq!(save.defaults, save.keys);
    assert_eq!(save.menu_key.as_deref(), Some("s"));
    assert_eq!(save.menu_modifiers, cmd());
    assert!(save.menu_item && !save.step_entry);
    let close = rows
        .iter()
        .find(|r| r.id == "editors.close_detail")
        .unwrap();
    assert!(close.keys.is_empty() && close.menu_key.is_none());
    assert_eq!(
        k.lookup(49, Default::default(), false, false).as_deref(),
        Some("transport.play_stop"),
        "Space"
    );
    assert_eq!(k.lookup(1, cmd(), true, true).as_deref(), Some("file.save"));
    assert_eq!(k.lookup(0xFFFF, cmd(), false, false), None, "no such key");
    assert_eq!(k.slot_clash("s".into(), 8).as_deref(), Some("file.save"));
    assert_eq!(k.slot_clash("q".into(), 2), None);
    assert_eq!(
        k.slot_clash("nope".into(), 0),
        None,
        "not a key: never pressed"
    );
}

#[test]
fn bindings_change_save_load_and_reset() {
    let k = vizdaw_ffi::Keymap::new();
    k.bind("file.save".into(), vec!["⌘K".into()], false)
        .unwrap();
    let e = k
        .bind("edit.undo".into(), vec!["⌘K".into()], false)
        .unwrap_err();
    assert_eq!(
        e,
        VizError::Model {
            message: "⌘K is the key of “Save”".into()
        }
    );
    assert!(matches!(
        k.bind("file.save".into(), vec!["⌘⌘".into()], false),
        Err(VizError::Model { .. })
    ));
    assert_eq!(
        k.lookup(40, cmd(), false, false).as_deref(),
        Some("file.save")
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join("mine.vizkeys")
        .to_string_lossy()
        .into_owned();
    k.save_file(path.clone()).unwrap();
    k.reset();
    assert_eq!(k.lookup(40, cmd(), false, false), None);
    assert!(k.load_file(path).unwrap().is_empty());
    assert_eq!(
        k.lookup(40, cmd(), false, false).as_deref(),
        Some("file.save")
    );
    k.reset_one("file.save".into());
    assert_eq!(
        k.lookup(1, cmd(), false, false).as_deref(),
        Some("file.save")
    );
    // Damaged and hostile files: refused whole, the bindings unchanged.
    let before = k.to_json();
    let big = dir.path().join("big.vizkeys");
    std::fs::write(&big, vec![b' '; 300 * 1024]).unwrap();
    assert!(matches!(
        k.load_file(big.to_string_lossy().into_owned()),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        k.load_file(dir.path().join("none").to_string_lossy().into_owned()),
        Err(VizError::Io { .. })
    ));
    assert!(matches!(
        k.load_json("garbage".into()),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        k.save_file("/nonexistent/dir/k.vizkeys".into()),
        Err(VizError::Io { .. })
    ));
    assert_eq!(k.to_json(), before);
    let skipped = k
        .load_json(r#"{"vizdaw_keys": 1, "bindings": {"no.such": ["F1"]}}"#.into())
        .unwrap();
    assert_eq!(skipped, vec!["no command \"no.such\"".to_string()]);
}
