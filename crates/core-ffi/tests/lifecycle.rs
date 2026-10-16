use vizdaw_ffi::{DocSnapshot, SeqRow, Session, VizError};

fn row<'a>(s: &'a DocSnapshot, name: &str) -> &'a SeqRow {
    s.sequences.iter().find(|r| r.name == name).unwrap()
}

#[test]
fn empty_session_is_untitled_and_clean() {
    let s = Session::new_empty().unwrap();
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.title, "Untitled");
    assert!(!snap.dirty);
    assert_eq!(snap.version, 0);
    assert_eq!(snap.undo_label, None);
    assert_eq!(snap.redo_label, None);
    assert!(snap.warnings.is_empty());
    assert_eq!(s.path().unwrap(), None);
}

#[test]
fn demo_has_song_groove_and_beat() {
    let s = Session::new_demo().unwrap();
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.sequences.len(), 3);
    assert!(!snap.dirty);
    let top: Vec<&str> = snap
        .sequences
        .iter()
        .filter(|r| r.top_level)
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(top, ["Song"]);
    assert_eq!(row(&snap, "Groove").use_count, 2);
    assert_eq!(row(&snap, "Beat").use_count, 1);
    assert_eq!(row(&snap, "Song").use_count, 0);
    assert_eq!(row(&snap, "Song").lane_count, 2);
    // 12 bars of 4/4 at 15360 ticks per quarter.
    assert_eq!(row(&snap, "Song").length_ticks, 12 * 4 * 15_360);
}

#[test]
fn demo_lanes_describe_destinations_and_contents() {
    let s = Session::new_demo().unwrap();
    let snap = s.snapshot().unwrap();
    let song = row(&snap, "Song").id;
    let lanes = s.lanes(song).unwrap();
    let names: Vec<&str> = lanes.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Groove", "Chords"]);
    let groove = &lanes[0];
    assert_eq!(groove.reference_count, 2);
    assert_eq!(groove.midi_event_count, 0);
    assert_eq!(groove.destination, None);
    let chords = &lanes[1];
    assert_eq!(chords.destination.as_deref(), Some("Built-in Piano"));
    assert!(chords.midi_event_count > 12);
    assert_eq!(chords.reference_count, 0);
    assert_eq!(chords.take_count, 1);
    assert_eq!(chords.active_take, "Take 1");
    assert!(!chords.mute && !chords.solo && !chords.looped && !chords.is_drum);
    let beat = row(&snap, "Beat").id;
    assert!(s.lanes(beat).unwrap()[0].is_drum);
}

#[test]
fn unknown_sequence_is_a_model_error() {
    let s = Session::new_demo().unwrap();
    let e = s.lanes(9_999).unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
}
