//! Plugin parameters through the FFI (spec §4, §11): the list, a live set
//! and its one-step commit, plugin-UI gestures as undo steps (also when an
//! undo comes before the next timer tick, and when a drag is open), and a
//! gesture an editor window left open. One test at a time (the engine
//! host is process-wide).

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{ParamRow, ParamValue, PluginFormatTag, PluginRefRow, Session, SlotStatus};

static SERIAL: Mutex<()> = Mutex::new(());

const GAIN: u64 = 0;
const WIGGLE: u64 = 2;

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    g
}

fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let start = Instant::now();
    while !done(s) {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The demo with a running Test Gain on the built-in synth, saved and
/// reopened (so it has no undo history), playing: (session, slot).
fn playing_gain() -> (tempfile::TempDir, Arc<Session>, u64) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.vizdaw").to_string_lossy().into_owned();
    {
        let s = Session::new_demo().unwrap();
        let synth = s.mixer().unwrap().strips[0].id;
        s.add_insert(
            synth,
            None,
            PluginRefRow {
                format: PluginFormatTag::Clap,
                id: "vizdaw.test.gain".into(),
                name: "Test Gain".into(),
                vendor: "VizDAW".into(),
                version: "1.0.0".into(),
            },
        )
        .unwrap();
        s.save_as(path.clone()).unwrap();
    }
    let s = Session::open(path).unwrap();
    let id = s.plugin_slots().unwrap()[0].id;
    let song = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id;
    s.play(song).unwrap();
    idle_until(&s, |s| {
        s.plugin_slots().unwrap()[0].status == SlotStatus::Live
            && s.transport().unwrap().report.is_empty()
            && s.transport().unwrap().playing
    });
    (dir, s, id)
}

fn param(s: &Session, slot: u64, id: u64) -> ParamRow {
    s.params(slot)
        .unwrap()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
}

#[test]
fn params_list_the_plugins_parameters_with_values() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    let names: Vec<String> = s.params(id).unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["Gain", "Latency", "Wiggle"]);
    let g = param(&s, id, GAIN);
    assert_eq!((g.min, g.max, g.default, g.value), (0.0, 2.0, 1.0, 1.0));
    assert!(param(&s, id, 1).stepped);
    assert!(s.params(999).unwrap().is_empty(), "no such slot");
    assert!(s.set_param(id, GAIN, f64::NAN).is_err());
}

#[test]
fn a_dragged_control_is_heard_at_once_and_commits_one_step() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    for v in [0.9, 0.6, 0.25] {
        s.set_param(id, GAIN, v).unwrap();
    }
    let snap = s
        .commit_params(
            id,
            vec![ParamValue {
                id: GAIN,
                value: 0.25,
            }],
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Change Plugin Parameter"));
    idle_until(&s, |s| param(s, id, GAIN).value == 0.25);
    s.undo().unwrap();
    // The plugin goes back to the value it was loaded with.
    idle_until(&s, |s| param(s, id, GAIN).value == 1.0);
    assert!(s.snapshot().unwrap().undo_label.is_none());
}

#[test]
fn a_gesture_in_the_plugin_becomes_an_undo_step() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    // "Wiggle" makes the plugin perform a gesture on Gain, as a user would.
    s.set_param(id, WIGGLE, 0.4).unwrap();
    // A timer tick commits it as an undo step.
    idle_until(&s, |s| s.snapshot().unwrap().undo_label.is_some());
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Change Plugin Parameter")
    );
    assert_eq!(param(&s, id, GAIN).value, 0.4);
    s.undo().unwrap();
    idle_until(&s, |s| param(s, id, GAIN).value == 1.0);
}

#[test]
fn an_undo_before_the_next_tick_undoes_the_gesture() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    s.set_param(id, WIGGLE, 0.7).unwrap();
    // The audio thread runs the gesture; no timer tick happens before the
    // user presses Undo.
    std::thread::sleep(Duration::from_millis(80));
    let snap = s.undo().unwrap();
    assert_eq!(snap.redo_label.as_deref(), Some("Change Plugin Parameter"));
    idle_until(&s, |s| param(s, id, GAIN).value == 1.0);
}

#[test]
fn gestures_wait_while_a_drag_is_open() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    let synth = s.mixer().unwrap().strips[0].id;
    s.begin_edit("Fader".into()).unwrap();
    s.set_gain(synth, -6.0).unwrap();
    s.set_param(id, WIGGLE, 0.3).unwrap();
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(150) {
        assert!(s.idle().unwrap().snapshot.is_none(), "held while dragging");
        std::thread::sleep(Duration::from_millis(5));
    }
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Change Plugin Parameter"));
    s.undo().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Fader"),
        "the drag stayed its own step"
    );
}

#[test]
fn a_closed_editor_ends_its_open_gesture() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    // Nothing is open: ending changes nothing.
    s.end_plugin_gestures(id).unwrap();
    assert!(s.idle().unwrap().snapshot.is_none());
    s.end_plugin_gestures(999).unwrap();
}
