//! Selection round trip, reference-from-selection with a view filter,
//! sequence templates and the editor layout in ui.json.

use vizdaw_ffi::{
    DocSnapshot, EditCommand, EventKindRow, LaneRef, ReferenceEdit, SelectionRow, SequenceTemplate,
    Session, TickRange, ViewFilterRow, VizError,
};

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

fn id_of(s: &DocSnapshot, name: &str) -> u64 {
    s.sequences.iter().find(|r| r.name == name).unwrap().id
}

fn song() -> (std::sync::Arc<Session>, u64, u64) {
    let s = Session::new_demo().unwrap();
    let song = id_of(&s.snapshot().unwrap(), "Song");
    let chords = s.lanes(song).unwrap()[1].id;
    (s, song, chords)
}

#[test]
fn selection_round_trips_and_drops_unknown_ids() {
    let (s, song, chords) = song();
    let first = s.lane_events(song, chords).unwrap().events[0].id;
    s.set_selection(SelectionRow {
        time_range: Some(TickRange { start: 0, end: BAR }),
        lanes: vec![
            LaneRef {
                seq: song,
                lane: chords,
            },
            LaneRef {
                seq: song,
                lane: 9_999,
            },
        ],
        events: vec![first, 9_999],
    })
    .unwrap();
    let sel = s.selection().unwrap();
    assert_eq!(sel.time_range, Some(TickRange { start: 0, end: BAR }));
    assert_eq!(
        sel.lanes,
        vec![LaneRef {
            seq: song,
            lane: chords
        }]
    );
    assert_eq!(sel.events, vec![first]);
}

#[test]
fn an_empty_or_inverted_range_is_dropped() {
    let (s, _, _) = song();
    for (start, end) in [(BAR, BAR), (BAR, 0)] {
        s.set_selection(SelectionRow {
            time_range: Some(TickRange { start, end }),
            lanes: vec![],
            events: vec![],
        })
        .unwrap();
        assert_eq!(s.selection().unwrap().time_range, None);
    }
}

#[test]
fn selecting_is_not_an_undo_step() {
    let (s, song, chords) = song();
    // Give the history a step, so "unchanged" is not just "empty".
    s.new_sequence_from_template("Plain".into(), SequenceTemplate::Empty)
        .unwrap();
    let before = s.snapshot().unwrap();
    assert_eq!(
        before.undo_label.as_deref(),
        Some("New Sequence from Template")
    );
    s.set_selection(SelectionRow {
        time_range: Some(TickRange { start: 0, end: BAR }),
        lanes: vec![LaneRef {
            seq: song,
            lane: chords,
        }],
        events: vec![],
    })
    .unwrap();
    let after = s.snapshot().unwrap();
    assert_eq!(after.undo_label, before.undo_label);
    assert_eq!(after.redo_label, before.redo_label);
    assert_eq!(after.version, before.version);
    s.undo().unwrap();
    assert_eq!(
        s.snapshot().unwrap().sequences.len(),
        before.sequences.len() - 1,
        "the one undo step is the template, not the selection"
    );
}

fn from_selection(song: u64) -> EditCommand {
    EditCommand::Reference {
        edit: ReferenceEdit::FromSelection {
            seq: song,
            per_lane: false,
            round_to_bars: true,
            loop_count: 1,
        },
    }
}

#[test]
fn a_reference_is_made_from_the_selection() {
    let (s, song, chords) = song();
    let sequences = s.snapshot().unwrap().sequences.len();
    s.set_selection(SelectionRow {
        time_range: Some(TickRange {
            start: 10,
            end: 4 * BAR - 10,
        }),
        lanes: vec![LaneRef {
            seq: song,
            lane: chords,
        }],
        events: vec![],
    })
    .unwrap();
    let snap = s.edit(from_selection(song), None).unwrap();
    assert_eq!(snap.sequences.len(), sequences + 1);
    let rows = s.lane_events(song, chords).unwrap().events;
    let r = rows
        .iter()
        .find(|r| r.kind == EventKindRow::Reference)
        .unwrap();
    assert_eq!((r.tick, r.duration), (0, 4 * BAR), "rounded to bars");
    assert!(
        !rows
            .iter()
            .any(|r| r.kind == EventKindRow::Note && r.tick < 4 * BAR)
    );
    s.undo().unwrap();
    assert_eq!(s.snapshot().unwrap().sequences.len(), sequences);
}

#[test]
fn the_view_filter_keeps_hidden_kinds_in_place() {
    let (s, song, chords) = song();
    s.set_selection(SelectionRow {
        time_range: Some(TickRange {
            start: 0,
            end: 4 * BAR,
        }),
        lanes: vec![LaneRef {
            seq: song,
            lane: chords,
        }],
        events: vec![],
    })
    .unwrap();
    let filter = ViewFilterRow {
        hidden_kinds: vec![EventKindRow::Note],
        hidden_lanes: vec![],
    };
    s.edit(from_selection(song), Some(filter)).unwrap();
    let rows = s.lane_events(song, chords).unwrap().events;
    assert!(
        rows.iter()
            .any(|r| r.kind == EventKindRow::Note && r.tick < 4 * BAR),
        "hidden notes stay in the lane"
    );

    // Without the filter the same command takes the notes into the reference.
    s.undo().unwrap();
    s.edit(from_selection(song), None).unwrap();
    let rows = s.lane_events(song, chords).unwrap().events;
    assert!(
        !rows
            .iter()
            .any(|r| r.kind == EventKindRow::Note && r.tick < 4 * BAR),
        "unfiltered, the notes move"
    );
}

#[test]
fn templates_make_one_undo_step() {
    let (s, song, _) = song();
    let n = s.snapshot().unwrap().sequences.len();
    let snap = s
        .new_sequence_from_template("Plain".into(), SequenceTemplate::Empty)
        .unwrap();
    assert_eq!(
        snap.undo_label.as_deref(),
        Some("New Sequence from Template")
    );
    let plain = id_of(&snap, "Plain");
    assert_eq!(s.lanes(plain).unwrap().len(), 1);

    let snap = s
        .new_sequence_from_template("Kit".into(), SequenceTemplate::DrumKit)
        .unwrap();
    let kit = s.lanes(id_of(&snap, "Kit")).unwrap();
    assert_eq!(kit.len(), 1);
    assert!(kit[0].is_drum);
    assert_eq!(kit[0].name, "Drums");

    let snap = s
        .new_sequence_from_template("Shape".into(), SequenceTemplate::SameLanesAs { seq: song })
        .unwrap();
    let shape = id_of(&snap, "Shape");
    let (a, b) = (s.lanes(song).unwrap(), s.lanes(shape).unwrap());
    let names = |v: &[vizdaw_ffi::LaneRow]| v.iter().map(|l| l.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&a), names(&b));
    assert_eq!(
        a.iter().map(|l| l.midi_dest).collect::<Vec<_>>(),
        b.iter().map(|l| l.midi_dest).collect::<Vec<_>>()
    );
    assert!(
        b.iter()
            .all(|l| l.midi_event_count == 0 && l.reference_count == 0)
    );
    assert_eq!(
        s.sequence_info(song).unwrap().tempo,
        s.sequence_info(shape).unwrap().tempo
    );
    s.undo().unwrap();
    assert_eq!(s.snapshot().unwrap().sequences.len(), n + 2);
    assert!(matches!(
        s.new_sequence_from_template("X".into(), SequenceTemplate::SameLanesAs { seq: 9_999 }),
        Err(VizError::Model { .. })
    ));
    assert_eq!(
        s.snapshot().unwrap().sequences.len(),
        n + 2,
        "a failed template leaves nothing"
    );
}

#[test]
fn the_editor_layout_is_saved_in_ui_json() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_demo().unwrap();
    assert_eq!(s.ui_layout().unwrap(), None);
    s.set_ui_layout(r#"{"version":1,"sequenceOrder":[3,1,2]}"#.into())
        .unwrap();
    assert!(matches!(
        s.set_ui_layout("not json".into()),
        Err(VizError::Model { .. })
    ));
    let before = s.snapshot().unwrap();
    assert!(
        !before.dirty,
        "the layout does not mark the document edited"
    );
    let path = dir.path().join("layout.vizdaw");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    let text = std::fs::read_to_string(path.join("ui.json")).unwrap();
    assert!(text.contains("open_plugin_windows"), "{text}");
    let reopened = Session::open(path.to_string_lossy().into_owned()).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&reopened.ui_layout().unwrap().unwrap()).unwrap();
    assert_eq!(json["sequenceOrder"], serde_json::json!([3, 1, 2]));
}
