//! Audition: Swift `AuditionTests`, the audition cases of
//! `PianoRollModelTests` and `KeyboardTests`, and closing a document.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use support::{FakeSession, roll_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::audition::{AUDITION_HINT, AuditionNote, NOTE_OFF_WINDOW, STUCK_NOTE_HINT};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget};
use vizdaw_app::piano_roll::AuditionRequest;
use vizdaw_app::store::DocumentStore;
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::LaneRef;

const C4: AuditionNote = AuditionNote { dest: 5, pitch: 60 };
const D4: AuditionNote = AuditionNote { dest: 5, pitch: 62 };

fn busy() -> VizError {
    VizError::Busy {
        message: "busy".into(),
    }
}

fn make() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(roll_fake());
    let m = DocumentModel::new(f.clone()).unwrap();
    (f, m)
}

fn with_roll(lane: u64) -> (Arc<FakeSession>, DocumentModel) {
    let (f, mut m) = make();
    m.open_detail(
        EditorTarget::Lane { seq: 3, lane },
        Some(DetailEditorKind::PianoRoll),
    );
    (f, m)
}

fn log(f: &FakeSession) -> Vec<String> {
    f.auditions()
        .iter()
        .map(|(on, p, _, _)| format!("{on} {p}"))
        .collect()
}

#[test]
fn a_note_sounds_and_ends() {
    let (f, mut m) = make();
    m.audition_on(C4, 0x8000);
    m.audition_off(C4);
    m.audition_off(C4);
    assert_eq!(log(&f), ["on 60", "off 60"], "a second off sends nothing");
    assert_eq!(m.status(), None);
    assert_eq!(
        f.auditions(),
        [
            ("on".to_string(), 60, 5, 0x8000),
            ("off".to_string(), 60, 5, 0)
        ]
    );
}

#[test]
fn a_busy_note_off_is_retried_on_tick() {
    let (f, mut m) = make();
    m.audition_on(C4, 0x8000);
    f.fail_next("audition", busy());
    f.fail_next("audition", busy());
    m.audition_off(C4);
    assert!(m.has_pending_note_offs());
    m.tick();
    assert!(m.has_pending_note_offs(), "still busy");
    m.tick();
    assert!(!m.has_pending_note_offs());
    assert_eq!(log(&f), ["on 60", "off 60"]);
    assert_eq!(m.status(), None, "Busy is silent");
}

#[test]
fn the_idle_poll_retries_note_offs() {
    let (f, mut m) = make();
    m.audition_on(C4, 0x8000);
    f.fail_next("audition", busy());
    m.audition_off(C4);
    assert!(m.idle(), "a waiting note-off asks for another poll");
    assert_eq!(log(&f), ["on 60", "off 60"]);
}

#[test]
fn a_note_off_that_keeps_failing_is_given_up_after_two_seconds() {
    let (f, mut m) = make();
    let t0 = Instant::now();
    assert_eq!(NOTE_OFF_WINDOW, Duration::from_secs(2));
    m.audition_on_at(C4, 0x8000, t0);
    f.set_call_error("audition", Some(busy()));
    m.audition_off_at(C4, t0);
    // However often the poll runs, only time counts.
    for ms in (0..1900).step_by(10) {
        m.tick_at(t0 + Duration::from_millis(ms));
    }
    m.tick_at(t0 + Duration::from_millis(1900));
    assert!(m.has_pending_note_offs(), "still pending at 1.9 s");
    assert_eq!(m.status(), None);
    m.tick_at(t0 + Duration::from_secs(2));
    assert!(!m.has_pending_note_offs());
    assert_eq!(log(&f), ["on 60"]);
    assert_eq!(m.status(), Some(STUCK_NOTE_HINT));
    assert_eq!(
        STUCK_NOTE_HINT,
        "A note may still be sounding — press Stop."
    );
}

#[test]
fn a_slow_poll_still_gives_up_on_time() {
    let (f, mut m) = make();
    let t0 = Instant::now();
    m.audition_on_at(C4, 0x8000, t0);
    f.set_call_error("audition", Some(busy()));
    m.audition_off_at(C4, t0);
    m.tick_at(t0 + Duration::from_millis(250));
    assert_eq!(m.status(), None);
    m.tick_at(t0 + Duration::from_millis(2100));
    assert_eq!(m.status(), Some(STUCK_NOTE_HINT), "one slow tick is enough");
}

#[test]
fn the_poll_runs_fast_while_a_note_off_waits() {
    let (f, mut m) = make();
    assert!(!m.wants_fast_poll());
    m.audition_on(C4, 0x8000);
    f.fail_next("audition", busy());
    m.audition_off(C4);
    assert!(m.wants_fast_poll(), "a note-off waits");
    m.tick();
    assert!(!m.wants_fast_poll());
}

#[test]
fn the_poll_runs_fast_while_a_drag_final_waits() {
    let (f, mut m) = with_roll(31);
    assert!(m.begin_gesture("Move Notes", vec![(3, 31)]));
    assert!(!m.wants_fast_poll(), "an open drag is not waiting");
    f.fail_next("end_edit", busy());
    m.end_gesture(None);
    assert!(m.wants_fast_poll());
    m.tick();
    assert!(!m.wants_fast_poll());
}

#[test]
fn a_non_busy_note_off_error_is_reported_at_once() {
    let (f, mut m) = make();
    m.audition_on(C4, 0x8000);
    f.fail_next(
        "audition",
        VizError::Engine {
            message: "queue full".into(),
        },
    );
    m.audition_off(C4);
    assert!(!m.has_pending_note_offs(), "not retried");
    assert_eq!(m.status(), Some("queue full"));
    assert_eq!(log(&f), ["on 60"]);
}

#[test]
fn off_all_meeting_busy_queues_retries() {
    let (f, mut m) = make();
    m.audition_on(C4, 0x8000);
    m.audition_on(D4, 0x8000);
    f.fail_next("audition", busy());
    f.fail_next("audition", busy());
    m.audition_off_all();
    assert!(m.has_pending_note_offs());
    m.tick();
    assert!(!m.has_pending_note_offs());
    let mut tail: Vec<String> = log(&f)[2..].to_vec();
    tail.sort();
    assert_eq!(tail, ["off 60", "off 62"]);
}

#[test]
fn a_note_on_waits_for_its_pending_note_off() {
    let (f, mut m) = make();
    m.audition_on(C4, 0x8000);
    f.fail_next("audition", busy());
    f.fail_next("audition", busy());
    m.audition_off(C4);
    // The retry inside the note-on meets Busy again: the note is not
    // started (the late note-off would end it).
    m.audition_on(C4, 0x8000);
    assert_eq!(log(&f), ["on 60"]);
    m.audition_on(C4, 0x8000);
    assert_eq!(log(&f), ["on 60", "off 60", "on 60"]);
}

#[test]
fn a_busy_note_on_is_dropped_silently() {
    let (f, mut m) = make();
    f.fail_next("audition", busy());
    m.audition_on(C4, 0x8000);
    m.audition_off(C4);
    assert!(f.auditions().is_empty(), "never started, so never ended");
    assert_eq!(m.status(), None);
}

#[test]
fn no_engine_shows_one_hint() {
    let (f, mut m) = make();
    f.set_audition_sent(false);
    m.audition_on(C4, 0x8000);
    assert_eq!(m.status(), Some(AUDITION_HINT));
    assert_eq!(
        AUDITION_HINT,
        "Notes sound once the audio engine runs: press Play once."
    );
    m.set_status(None);
    m.audition_on(D4, 0x8000);
    assert_eq!(m.status(), None, "once per document");
    m.audition_off(D4);
    assert!(f.auditions().is_empty(), "nothing held, nothing to end");
}

#[test]
fn off_all_ends_every_sounding_note() {
    let (f, mut m) = make();
    m.audition_on(C4, 0x8000);
    m.audition_on(D4, 0x8000);
    m.audition_off_all();
    let mut tail: Vec<String> = log(&f)[2..].to_vec();
    tail.sort();
    assert_eq!(tail, ["off 60", "off 62"]);
}

#[test]
fn sounding_a_new_pitch_ends_the_old_one() {
    let (f, mut m) = with_roll(31);
    m.roll_sound(60, 0x8000, Some(5));
    m.roll_sound(60, 0x8000, Some(5));
    m.roll_sound(62, 0x8000, Some(5));
    m.roll_silence();
    assert_eq!(log(&f), ["on 60", "off 60", "on 62", "off 62"]);
    assert_eq!(m.piano_roll().sounding(), None);
}

#[test]
fn a_lane_without_destination_is_silent() {
    let (f, mut m) = with_roll(30);
    let groove = LaneRef { seq: 3, lane: 30 };
    let mut row = support::note_row(9, 0, 60);
    assert_eq!(m.roll_dest(&row, groove), None);
    m.roll_sound(60, 0x8000, m.roll_dest(&row, groove));
    m.roll_silence();
    assert!(f.auditions().is_empty());
    assert_eq!(m.status(), None);
    row.dest = Some(9);
    assert_eq!(m.roll_dest(&row, groove), Some(9), "the note's own wins");
}

#[test]
fn sounding_without_a_destination_ends_the_old_note() {
    let (f, mut m) = with_roll(31);
    m.roll_sound(60, 0x8000, Some(5));
    m.roll_sound(60, 0x8000, None);
    assert_eq!(log(&f), ["on 60", "off 60"]);
}

#[test]
fn the_canvas_requests_drive_the_roll_audition() {
    let (f, mut m) = with_roll(31);
    let sound = |pitch| AuditionRequest::Sound {
        pitch,
        velocity: 0x8000,
        dest: Some(5),
    };
    m.roll_audition(&[
        sound(60),
        sound(60),
        sound(62),
        AuditionRequest::Silence,
        AuditionRequest::Silence,
    ]);
    assert_eq!(log(&f), ["on 60", "off 60", "on 62", "off 62"]);
}

#[test]
fn a_key_click_auditions_through_the_edit_track() {
    let (f, mut m) = with_roll(31);
    m.roll_key_sound(60);
    m.roll_key_release();
    assert_eq!(
        f.auditions(),
        [
            ("on".to_string(), 60, 5, 100 << 9),
            ("off".to_string(), 60, 5, 0)
        ],
        "Chords' destination at the Pencil's velocity"
    );
    // Insert Into another track: its destination.
    f.clear_calls();
    m.roll_set_edit_lane(LaneRef { seq: 3, lane: 32 });
    m.roll_toggle_shown(32);
    m.roll_key_sound(40);
    m.roll_key_release();
    assert_eq!(f.auditions()[2], ("on".to_string(), 40, 6, 100 << 9));
}

#[test]
fn a_key_is_silent_without_an_edit_track_destination() {
    let (f, mut m) = with_roll(30);
    m.roll_key_sound(60);
    m.roll_key_release();
    assert!(f.auditions().is_empty(), "Groove has no destination");
    let (f, mut m) = make();
    m.roll_key_sound(60);
    assert!(f.auditions().is_empty(), "no roll, no edit track");
    assert_eq!(m.status(), None);
}

#[test]
fn dragging_across_keys_plays_each_in_turn() {
    let (f, mut m) = with_roll(31);
    for p in [60, 60, 62] {
        m.roll_key_sound(p);
    }
    m.roll_key_release();
    assert_eq!(log(&f), ["on 60", "off 60", "on 62", "off 62"]);
}

#[test]
fn closing_a_document_silences_every_held_note() {
    let f = Arc::new(roll_fake());
    let shared = f.clone();
    let mut store = DocumentStore::new(
        Box::new(move || Ok(shared.clone())),
        Box::new(|| Ok(Arc::new(roll_fake()))),
        Box::new(|_| Ok(Arc::new(roll_fake()))),
    );
    let id = store.new_empty().unwrap();
    {
        let m = store.document_mut(id).unwrap();
        m.roll_sound(60, 0x8000, Some(5));
        m.audition_on(D4, 0x8000);
    }
    store.close(id);
    let mut tail: Vec<String> = log(&f)[2..].to_vec();
    tail.sort();
    assert_eq!(tail, ["off 60", "off 62"]);
}
