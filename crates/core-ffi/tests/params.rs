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

#[test]
fn an_undo_while_the_registry_is_busy_is_refused_and_changes_nothing() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    let synth = s.mixer().unwrap().strips[0].id;
    s.set_gain(synth, -6.0).unwrap();
    let before = s.snapshot().unwrap();
    assert!(before.undo_label.is_some() && before.redo_label.is_none());
    // The plugin finishes a gesture; no timer tick runs before Cmd-Z.
    s.set_param(id, WIGGLE, 0.7).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    // Cmd-Z inside a nested run loop (an AU v3 editor opening).
    let inner = vizdaw_ffi::plugins::with(0, |_| {
        assert!(matches!(s.undo(), Err(vizdaw_ffi::VizError::Busy { .. })));
        assert!(matches!(s.redo(), Err(vizdaw_ffi::VizError::Busy { .. })));
        let now = s.snapshot().unwrap();
        assert_eq!(now.undo_label, before.undo_label);
        assert_eq!(now.redo_label, None);
        assert_eq!(now.version, before.version);
    });
    assert!(inner.is_ok());
    // Afterwards the gesture is committed first, then undone.
    let snap = s.undo().unwrap();
    assert_eq!(snap.redo_label.as_deref(), Some("Change Plugin Parameter"));
    assert_eq!(
        snap.undo_label, before.undo_label,
        "the fader step survives"
    );
    idle_until(&s, |s| param(s, id, GAIN).value == 1.0);
}

/// Spec §7.1, plan 5e-1 Ruling 15: a gesture in a CLAP plugin's own
/// window while its strip writes (Touch) becomes automation of that
/// parameter, normalised by its range (Gain 0–2: 0.4 is 0.2), in a lane
/// named after it, and no "Change Plugin Parameter" step; in Read it is
/// still one.
#[test]
fn a_plugin_windows_gesture_writes_its_parameter() {
    use vizdaw_ffi::automation::AutomationModeRow;
    let _g = serial();
    let (_d, s, id) = playing_gain();
    let synth = s.mixer().unwrap().strips[0].id;
    let song = s.transport().unwrap().seq.unwrap();
    s.set_automation_mode(vec![synth], AutomationModeRow::Touch)
        .unwrap();
    s.set_param(id, WIGGLE, 0.4).unwrap();
    let t0 = Instant::now();
    idle_until(&s, |_| t0.elapsed() > Duration::from_millis(400));
    s.stop().unwrap();
    idle_until(&s, |s| {
        s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation")
    });
    let lane = s
        .lanes(song)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Built-in Synth Gain")
        .expect("a lane named after the parameter");
    let values: Vec<f64> = s
        .lane_events(song, lane.id)
        .unwrap()
        .events
        .iter()
        .filter_map(|e| e.automation.map(|a| a.value))
        .collect();
    assert!(values.iter().any(|v| (v - 0.2).abs() < 1e-9), "{values:?}");
    s.undo().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Automation Mode"),
        "no parameter step under the write"
    );
    s.set_automation_mode(vec![synth], AutomationModeRow::Read)
        .unwrap();
    s.play(song).unwrap();
    s.set_param(id, WIGGLE, 0.8).unwrap();
    idle_until(&s, |s| {
        s.snapshot().unwrap().undo_label.as_deref() == Some("Change Plugin Parameter")
    });
    s.stop().unwrap();
}

/// Pre-flight I9, Ruling 15 (and M13, M14): a gesture from the app on a
/// plugin parameter (plan 5e-2's sliders) reaches the plugin on every
/// path. While writing it is automation and no parameter step, and after
/// landing the plugin plays the landed value (the glide back: its value
/// before); stopped, it is one "Change Plugin Parameter" step. Gain is
/// 0–2, so 0.3 is 0.6.
#[test]
fn a_ui_gesture_on_a_plugin_parameter_reaches_the_plugin() {
    use vizdaw_ffi::AutomationTargetRow as Target;
    use vizdaw_ffi::automation::{AutomationModeRow, GesturePhaseRow as Phase};
    let _g = serial();
    let (_d, s, id) = playing_gain();
    let synth = s.mixer().unwrap().strips[0].id;
    let before = param(&s, id, GAIN).value;
    s.set_automation_mode(vec![synth], AutomationModeRow::Touch)
        .unwrap();
    let t = Target::PluginParam {
        slot: id,
        param: GAIN,
    };
    s.gesture(t, Phase::Touch { value: Some(0.25) }).unwrap();
    std::thread::sleep(Duration::from_millis(15));
    s.gesture(t, Phase::Value { value: 0.3 }).unwrap();
    idle_until(&s, |s| (param(s, id, GAIN).value - 0.6).abs() < 1e-6);
    s.gesture(t, Phase::Release).unwrap();
    let t0 = Instant::now();
    idle_until(&s, |_| t0.elapsed() > Duration::from_millis(400));
    s.stop().unwrap();
    idle_until(&s, |s| {
        s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation")
    });
    idle_until(&s, |s| (param(s, id, GAIN).value - before).abs() < 1e-6);
    s.undo().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Automation Mode"),
        "no parameter step"
    );
    // Stopped: the static path, one step.
    s.gesture(t, Phase::Touch { value: Some(0.25) }).unwrap();
    std::thread::sleep(Duration::from_millis(15));
    s.gesture(t, Phase::Value { value: 0.25 }).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Change Plugin Parameter")
    );
    idle_until(&s, |s| (param(s, id, GAIN).value - 0.5).abs() < 1e-6);
    s.undo().unwrap();
    idle_until(&s, |s| (param(s, id, GAIN).value - before).abs() < 1e-6);
}

/// Task 9 review I-1 (plan 5e-1 Ruling 15: "the static value stays"): a
/// ride on a Latch strip, from the plugin's own window or from the app,
/// leaves the plugin at the ride's value while its lane plays; undoing
/// "Write Automation" puts the plugin back at its static value (Gain's
/// 1.0), a redo plays the lane's value again, and a save after the undo
/// keeps the static value in the plugin's state.
#[test]
fn undoing_a_plugin_parameter_write_puts_back_its_static_value() {
    use vizdaw_ffi::AutomationTargetRow as Target;
    use vizdaw_ffi::automation::{AutomationModeRow, GesturePhaseRow as Phase};
    let _g = serial();
    for from_plugin in [true, false] {
        let (d, s, id) = playing_gain();
        let synth = s.mixer().unwrap().strips[0].id;
        let before = param(&s, id, GAIN).value;
        assert_eq!(before, 1.0);
        s.set_automation_mode(vec![synth], AutomationModeRow::Latch)
            .unwrap();
        let ridden = if from_plugin {
            s.set_param(id, WIGGLE, 0.4).unwrap();
            0.4
        } else {
            let t = Target::PluginParam {
                slot: id,
                param: GAIN,
            };
            s.gesture(t, Phase::Touch { value: Some(0.25) }).unwrap();
            std::thread::sleep(Duration::from_millis(15));
            s.gesture(t, Phase::Value { value: 0.3 }).unwrap();
            s.gesture(t, Phase::Release).unwrap();
            0.6
        };
        let t0 = Instant::now();
        idle_until(&s, |_| t0.elapsed() > Duration::from_millis(400));
        s.stop().unwrap();
        idle_until(&s, |s| {
            s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation")
        });
        idle_until(&s, |s| (param(s, id, GAIN).value - ridden).abs() < 1e-6);
        // Re-review N1: what plays is what counts. A muted lane, or a new
        // (empty) take made active, plays no automation: the plugin goes
        // back to its static value; the undo plays the ride again.
        let song = s.transport().unwrap().seq.unwrap();
        let lane = s
            .lanes(song)
            .unwrap()
            .into_iter()
            .find(|l| l.name == "Built-in Synth Gain")
            .expect("the written lane")
            .id;
        s.set_lane_mute(song, lane, true).unwrap();
        idle_until(&s, |s| (param(s, id, GAIN).value - before).abs() < 1e-6);
        s.undo().unwrap();
        idle_until(&s, |s| (param(s, id, GAIN).value - ridden).abs() < 1e-6);
        s.edit(
            vizdaw_ffi::EditCommand::Lane {
                edit: vizdaw_ffi::LaneEdit::AddTake {
                    seq: song,
                    lane,
                    name: "Take 2".into(),
                },
            },
            None,
        )
        .unwrap();
        idle_until(&s, |s| (param(s, id, GAIN).value - before).abs() < 1e-6);
        s.undo().unwrap();
        idle_until(&s, |s| (param(s, id, GAIN).value - ridden).abs() < 1e-6);
        assert_eq!(
            s.snapshot().unwrap().undo_label.as_deref(),
            Some("Write Automation")
        );
        s.undo().unwrap();
        idle_until(&s, |s| (param(s, id, GAIN).value - before).abs() < 1e-6);
        s.redo().unwrap();
        idle_until(&s, |s| (param(s, id, GAIN).value - ridden).abs() < 1e-6);
        s.undo().unwrap();
        idle_until(&s, |s| (param(s, id, GAIN).value - before).abs() < 1e-6);
        s.save().unwrap();
        let path = d.path().join("p.vizdaw").to_string_lossy().into_owned();
        drop(s);
        let s = Session::open(path).unwrap();
        idle_until(&s, |s| {
            s.plugin_slots().unwrap()[0].status == SlotStatus::Live
        });
        assert_eq!(
            param(&s, id, GAIN).value,
            before,
            "from the plugin: {from_plugin}"
        );
    }
}

// ---------------------------------------------------------------------------
// Task 9b (spec §7, plan 5e-1 Ruling 15): an automated parameter's static
// value survives every save and autosave. CLAP Test Gain only: Gain 0–2,
// static 1.0; a ride is Wiggle 0.4 from the plugin's window, or an app
// gesture to 0.3 (Gain 0.6).

fn settle(s: &Session, ms: u64) {
    let t0 = Instant::now();
    idle_until(s, |_| t0.elapsed() > Duration::from_millis(ms));
}

fn gain_is(s: &Session, id: u64, v: f64) {
    idle_until(s, |s| (param(s, id, GAIN).value - v).abs() < 1e-6);
}

/// Starts a ride of Gain while playing: from the plugin's own window, or
/// from the app. Returns the value the ride sets.
fn start_ride(s: &Session, id: u64, from_plugin: bool) -> f64 {
    use vizdaw_ffi::AutomationTargetRow as Target;
    use vizdaw_ffi::automation::GesturePhaseRow as Phase;
    if from_plugin {
        s.set_param(id, WIGGLE, 0.4).unwrap();
        0.4
    } else {
        let t = Target::PluginParam {
            slot: id,
            param: GAIN,
        };
        s.gesture(t, Phase::Touch { value: Some(0.25) }).unwrap();
        std::thread::sleep(Duration::from_millis(15));
        s.gesture(t, Phase::Value { value: 0.3 }).unwrap();
        s.gesture(t, Phase::Release).unwrap();
        0.6
    }
}

/// A Latch ride landed as "Write Automation", static Gain `static_v`
/// (committed to the overlay first when given): (dir, session, slot,
/// song, the written lane, the ride's value).
fn latch_ride(
    static_v: Option<f64>,
    from_plugin: bool,
) -> (tempfile::TempDir, Arc<Session>, u64, u64, u64, f64) {
    use vizdaw_ffi::automation::AutomationModeRow;
    let (d, s, id) = playing_gain();
    let song = s.transport().unwrap().seq.unwrap();
    if let Some(v) = static_v {
        s.stop().unwrap();
        // The stopped pass ends first.
        settle(&s, 150);
        s.commit_params(id, vec![ParamValue { id: GAIN, value: v }])
            .unwrap();
        gain_is(&s, id, v);
        s.play(song).unwrap();
        idle_until(&s, |s| {
            s.transport().unwrap().report.is_empty() && s.transport().unwrap().playing
        });
        settle(&s, 50);
    }
    let synth = s.mixer().unwrap().strips[0].id;
    s.set_automation_mode(vec![synth], AutomationModeRow::Latch)
        .unwrap();
    let ridden = start_ride(&s, id, from_plugin);
    settle(&s, 400);
    s.stop().unwrap();
    idle_until(&s, |s| {
        s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation")
    });
    gain_is(&s, id, ridden);
    let lane = s
        .lanes(song)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Built-in Synth Gain")
        .expect("the written lane")
        .id;
    (d, s, id, song, lane, ridden)
}

fn remove_lane(s: &Session, seq: u64, lane: u64) {
    s.edit(
        vizdaw_ffi::EditCommand::Lane {
            edit: vizdaw_ffi::LaneEdit::Remove { seq, lane },
        },
        None,
    )
    .unwrap();
}

/// Saves, closes and opens the project again, once its plugin runs.
fn reopen(d: &tempfile::TempDir, s: Arc<Session>) -> Arc<Session> {
    s.save().unwrap();
    let path = d.path().join("p.vizdaw").to_string_lossy().into_owned();
    drop(s);
    let s = Session::open(path).unwrap();
    idle_until(&s, |s| {
        s.plugin_slots().unwrap()[0].status == SlotStatus::Live
    });
    s
}

/// Task 9b, the re-review's table: a Latch ride, landed, then a save (with
/// or without a reopen), then the lane removed: the plugin goes back to
/// the static value (1.0, or the 0.8 committed before the ride), never to
/// the ride's value. Both sources, both with and without a reopen.
#[test]
fn a_save_keeps_an_automated_parameters_static_value() {
    let _g = serial();
    for from_plugin in [true, false] {
        for static_v in [None, Some(0.8)] {
            for reopened in [false, true] {
                let case = format!("plugin {from_plugin}, static {static_v:?}, reopen {reopened}");
                let (d, s, id, song, lane, ridden) = latch_ride(static_v, from_plugin);
                let s = if reopened {
                    reopen(&d, s)
                } else {
                    s.save().unwrap();
                    // The plugin still plays the lane's value.
                    settle(&s, 50);
                    assert!(
                        (param(&s, id, GAIN).value - ridden).abs() < 1e-6,
                        "{case}: not snapped back"
                    );
                    s
                };
                remove_lane(&s, song, lane);
                let want = static_v.unwrap_or(1.0);
                let t0 = Instant::now();
                while (param(&s, id, GAIN).value - want).abs() > 1e-6 {
                    assert!(
                        t0.elapsed() < Duration::from_secs(10),
                        "{case}: Gain {} after removing the lane",
                        param(&s, id, GAIN).value
                    );
                    s.idle().unwrap();
                    std::thread::sleep(Duration::from_millis(2));
                }
                // And it stays so across another save and reopen.
                let s = reopen(&d, s);
                settle(&s, 50);
                assert!(
                    (param(&s, id, GAIN).value - want).abs() < 1e-6,
                    "{case}: Gain {} after the last reopen",
                    param(&s, id, GAIN).value
                );
            }
        }
    }
}

/// Task 9b: a save (the path every autosave takes) while a pass is open
/// and Gain is being ridden, before its lane exists: after Stop the lane
/// plays the ride, Undo of the landing puts back the static value, and a
/// save and reopen keep it. Both sources.
#[test]
fn a_save_in_the_middle_of_a_pass_keeps_the_static_value() {
    use vizdaw_ffi::automation::AutomationModeRow;
    let _g = serial();
    for from_plugin in [true, false] {
        let (d, s, id) = playing_gain();
        let synth = s.mixer().unwrap().strips[0].id;
        s.set_automation_mode(vec![synth], AutomationModeRow::Latch)
            .unwrap();
        let ridden = start_ride(&s, id, from_plugin);
        settle(&s, 150);
        s.save().unwrap();
        settle(&s, 50);
        assert!(
            (param(&s, id, GAIN).value - ridden).abs() < 1e-6,
            "plugin {from_plugin}: not snapped back by the save"
        );
        settle(&s, 200);
        s.stop().unwrap();
        idle_until(&s, |s| {
            s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation")
        });
        gain_is(&s, id, ridden);
        s.undo().unwrap();
        gain_is(&s, id, 1.0);
        let s = reopen(&d, s);
        settle(&s, 50);
        assert_eq!(param(&s, id, GAIN).value, 1.0, "plugin {from_plugin}");
    }
}

/// Task 9b: a parameter nobody automates is saved as the plugin holds it
/// (no change): a value sent to the plugin and never committed is in its
/// saved state after a reopen.
#[test]
fn a_save_captures_a_parameter_nobody_automates_as_the_plugin_holds_it() {
    let _g = serial();
    let (d, s, id) = playing_gain();
    s.set_param(id, GAIN, 0.7).unwrap();
    gain_is(&s, id, 0.7);
    let s = reopen(&d, s);
    settle(&s, 50);
    assert_eq!(param(&s, id, GAIN).value, 0.7);
}

/// Task 9b, item 4: a plugin that loads (at open, or again: here the
/// undo of its removal, whose capture kept Gain's static value) gets its
/// state, then its static value, and then the value its lane plays at
/// the position, not the static value. (At open no engine runs yet, so
/// that value reaches the plugin with its first block; here the engine
/// runs while stopped.)
#[test]
fn a_loaded_plugin_plays_its_automation_at_the_position() {
    let _g = serial();
    let (_d, s, id, _song, _lane, ridden) = latch_ride(None, true);
    s.remove_insert(id).unwrap();
    settle(&s, 300);
    assert!(s.plugin_slots().unwrap().is_empty());
    s.undo().unwrap();
    idle_until(&s, |s| {
        s.plugin_slots()
            .unwrap()
            .first()
            .is_some_and(|p| p.status == SlotStatus::Live)
    });
    gain_is(&s, id, ridden);
    // The removal's capture kept the static value: removing the lane
    // puts it back.
    let song = s.transport().unwrap().seq.unwrap();
    let lane = s
        .lanes(song)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Built-in Synth Gain")
        .expect("the written lane")
        .id;
    remove_lane(&s, song, lane);
    gain_is(&s, id, 1.0);
}
