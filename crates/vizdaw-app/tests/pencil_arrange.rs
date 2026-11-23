//! A Pencil click on a track in the arrangement opens its piano roll at
//! that bar (Swift `PencilArrangeTests`, the model's part; the view's
//! scroll and the arrangement click are the GPUI crate's).

mod support;

use std::sync::Arc;

use support::{FakeSession, lane_row, roll_fake};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, Tool};
use vizdaw_app::{DocSession, DocumentModel};
use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::tracks::TrackKindRow;

const BAR2: i64 = 61_440;

fn make() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(roll_fake());
    let mut rows = DocSession::lanes(f.as_ref(), 3).unwrap();
    let mut vocals = lane_row(90, "Vocals", false);
    vocals.kind = TrackKindRow::Audio;
    let mut synth = lane_row(91, "Synth", false);
    synth.kind = TrackKindRow::Instrument;
    rows.extend([vocals, synth]);
    f.set_lanes(3, rows);
    let m = DocumentModel::new(f.clone()).unwrap();
    (f, m)
}

#[test]
fn a_midi_track_opens_its_piano_roll_at_the_clicked_bar_with_the_pencil() {
    let (_, mut m) = make();
    assert!(m.pencil_opens_piano_roll(3, 31, BAR2 + 5_000));
    let d = m.layout().detail.clone().unwrap();
    assert_eq!(d.kind, DetailEditorKind::PianoRoll);
    assert_eq!(d.target, EditorTarget::Lane { seq: 3, lane: 31 });
    assert_eq!(m.tool(), Tool::Pencil);
    assert_eq!(m.selection().lanes, vec![LaneRef { seq: 3, lane: 31 }]);
    assert!(m.selection().events.is_empty());
    assert_eq!(m.selection().time_range, None);
    let r = m.piano_roll().reveal().unwrap();
    assert_eq!(r.tick, BAR2);
    assert_eq!(r.target, EditorTarget::Lane { seq: 3, lane: 31 });
    assert_eq!(m.layout().last_kind, DetailEditorKind::PianoRoll);
}

#[test]
fn an_instrument_track_opens_and_each_request_is_new() {
    let (_, mut m) = make();
    assert!(m.pencil_opens_piano_roll(3, 91, BAR2 + 1));
    let first = m.piano_roll().reveal().unwrap();
    assert!(m.pencil_opens_piano_roll(3, 91, BAR2 + 1));
    let second = m.piano_roll().reveal().unwrap();
    assert_eq!(first.tick, second.tick);
    assert_ne!(first.serial, second.serial, "a new request scrolls again");
}

#[test]
fn bar_one_stays_at_the_start_and_negative_ticks_clamp() {
    let (_, mut m) = make();
    assert!(m.pencil_opens_piano_roll(3, 31, -50));
    assert_eq!(m.piano_roll().reveal().unwrap().tick, 0);
}

#[test]
fn the_bar_follows_the_meter() {
    let (f, mut m) = make();
    let mut info = support::four_four(3);
    info.meter[0].num = 3;
    f.set_info(3, info);
    m.rename_track(31, "Kit");
    assert!(m.pencil_opens_piano_roll(3, 31, 4 * 15_360));
    assert_eq!(
        m.piano_roll().reveal().unwrap().tick,
        3 * 15_360,
        "bar 2 of 3/4"
    );
}

#[test]
fn an_audio_track_or_an_unknown_one_does_nothing() {
    let (f, mut m) = make();
    assert!(!m.pencil_opens_piano_roll(3, 90, BAR2));
    assert!(!m.pencil_opens_piano_roll(3, 12_345, BAR2));
    assert!(!m.pencil_opens_piano_roll(999, 1, BAR2));
    assert!(m.layout().detail.is_none());
    assert!(m.piano_roll().reveal().is_none());
    assert_eq!(m.tool(), Tool::Arrow);
    assert!(m.selection().lanes.is_empty());
    assert!(f.edits().is_empty());
}

#[test]
fn opening_another_editor_drops_the_request() {
    let (_, mut m) = make();
    m.pencil_opens_piano_roll(3, 31, BAR2);
    m.open_detail(
        EditorTarget::Lane { seq: 3, lane: 31 },
        Some(DetailEditorKind::List),
    );
    assert!(m.piano_roll().reveal().is_none());
}
