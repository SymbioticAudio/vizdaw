//! Automation recording through a session (plan 5e-1 Tasks 8–10;
//! sub-project 5 spec §7, §12.3): gestures on the static path and while
//! writing, each mode's landing at Stop as one undo step, writes joining
//! a recording, Off and suspend, hostile gestures and settings, a device
//! rebuild mid-pass, the idle before the roll, a Locate mid-pass, two
//! static gestures at once, a touch at the heard value, a target deleted
//! mid-pass, the preview and the state, the channel's spelling.
//! Offline only: no audio device is opened and no microphone asked.

#![recursion_limit = "256"]

mod common;

use core_engine::backend::BackendEvent;
use core_model::automation::{fader_db, fader_norm, pan_of};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::automation::{
    AutomationModeRow as Mode, AutomationSettingsRow, GesturePhaseRow as Phase, ThinningRow,
    reset_automation_settings, set_automation_settings, set_automation_suspended,
};
use vizdaw_ffi::host::{
    debug_hold_offline, fader_override, force_offline, reset_host, simulate_event,
};
use vizdaw_ffi::{AutomationTargetRow as Target, EventRow, Session, VizError};

const Q: i64 = 15_360;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    reset_automation_settings();
    set_automation_suspended(false);
    debug_hold_offline(false);
    g
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Paces gesture samples past the 10 ms sampler.
fn pace(s: &Session) {
    std::thread::sleep(Duration::from_millis(12));
    s.idle().unwrap();
}

fn demo() -> (Arc<Session>, u64) {
    let s = Session::new_demo().unwrap();
    let song = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id;
    (s, song)
}

fn strip(s: &Session, name: &str) -> u64 {
    s.mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.name == name)
        .unwrap()
        .id
}

fn tick(s: &Session) -> i64 {
    s.transport().unwrap().tick
}

fn lane_named(s: &Session, seq: u64, name: &str) -> Option<u64> {
    s.lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == name)
        .map(|l| l.id)
}

/// `(tick, value)` of a lane's automation points, by tick.
fn points(s: &Session, seq: u64, lane: u64) -> Vec<(i64, f64)> {
    let mut v: Vec<(i64, f64)> = s
        .lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .filter_map(|e: &EventRow| e.automation.map(|a| (e.tick, a.value)))
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

fn stop_and_land(s: &Session) {
    s.stop().unwrap();
    wait_until("the transport to stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
}

/// Spec §7.3 Touch and §7.4: a fader moved while playing writes into a
/// new lane "<strip> Fader" at the end of the playing sequence, glides
/// back to the static value, and lands at Stop as one undo step "Write
/// Automation"; the static value is unchanged.
#[test]
fn touch_writes_a_fader_while_playing_as_one_undo_step() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    let gain = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.id == synth)
        .unwrap()
        .gain_db;
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Automation Mode")
    );
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    assert!(s.automation_state().unwrap().touched.contains(&t));
    for v in [0.55, 0.6, 0.65, 0.7] {
        pace(&s);
        s.gesture(t, Phase::Value { value: v }).unwrap();
    }
    pace(&s);
    assert!(
        !s.automation_preview(t).unwrap().is_empty(),
        "the write in progress"
    );
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("the glide to pass", || tick(&s) > at + Q);
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("a new lane");
    let p = points(&s, song, lane);
    assert!(p.len() >= 4, "{p:?}");
    let values: Vec<f64> = p.iter().map(|x| x.1).collect();
    assert!(
        values.iter().any(|v| (v - 0.5).abs() < 1e-9)
            && values.iter().any(|v| (v - 0.7).abs() < 1e-9),
        "{p:?}"
    );
    // The edges and the glide's end hold the static value.
    let fixed = fader_norm(gain);
    assert!(
        (p.first().unwrap().1 - fixed).abs() < 1e-9 && (p.last().unwrap().1 - fixed).abs() < 1e-9,
        "{p:?}"
    );
    let g2 = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.id == synth)
        .unwrap()
        .gain_db;
    assert_eq!(g2, gain, "writing leaves the static value");
    s.undo().unwrap();
    assert_eq!(
        lane_named(&s, song, "Built-in Synth Fader"),
        None,
        "one step"
    );
    assert!(s.automation_preview(t).unwrap().is_empty());
    s.close().unwrap();
}

/// Spec §7.2: in Read (and while stopped) a gesture changes the static
/// value as one undo step named after the control; the engine hears it
/// while touched.
#[test]
fn read_gestures_change_the_static_value_as_one_step() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    for v in [0.4, 0.3] {
        pace(&s);
        s.gesture(t, Phase::Value { value: v }).unwrap();
    }
    s.gesture(t, Phase::Release).unwrap();
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Set Fader"));
    let gain = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.id == synth)
        .unwrap()
        .gain_db;
    assert!((gain - fader_db(0.3)).abs() < 1e-9, "{gain}");
    s.undo().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label,
        None,
        "one step for the gesture"
    );
    // While playing in Read, a touch is heard over the static value.
    s.play(song).unwrap();
    wait_until("playing", || tick(&s) > 0);
    s.gesture(
        t,
        Phase::Touch {
            value: Some(fader_norm(-20.0)),
        },
    )
    .unwrap();
    wait_until("the override", || {
        fader_override(synth).is_some_and(|d| (d + 20.0).abs() < 1e-6)
    });
    s.gesture(t, Phase::Release).unwrap();
    stop_and_land(&s);
    s.close().unwrap();
}

/// Spec §7.3 Write and Latch; Ruling 9: an untouched Write strip writes
/// its controls from the roll at their values; at landing it becomes the
/// After Write mode (Latch) in the same undo step; Latch holds a touched
/// pan until Stop.
#[test]
fn write_and_latch_land_at_stop_and_write_becomes_latch() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Write).unwrap();
    s.play(song).unwrap();
    wait_until("two beats", || tick(&s) > 2 * Q);
    // Pre-flight M8: Write holds are not touches (5e-2's rings).
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "{:?}",
        s.automation_state().unwrap().touched
    );
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    for name in [
        "Built-in Synth Fader",
        "Built-in Synth Pan",
        "Built-in Synth Mute",
    ] {
        let lane = lane_named(&s, song, name).unwrap_or_else(|| panic!("{name}"));
        assert!(points(&s, song, lane).len() >= 2, "{name}");
    }
    let state = s.automation_state().unwrap();
    assert!(
        state
            .modes
            .iter()
            .any(|m| m.strip == synth && m.mode == Mode::Latch),
        "After Write"
    );
    s.undo().unwrap();
    let state = s.automation_state().unwrap();
    assert!(
        state
            .modes
            .iter()
            .any(|m| m.strip == synth && m.mode == Mode::Write),
        "one step"
    );
    assert_eq!(lane_named(&s, song, "Built-in Synth Pan"), None);
    // Latch: a pan touched and let go holds to Stop.
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let pan = Target::StripPan { strip: synth };
    s.gesture(pan, Phase::Touch { value: Some(0.2) }).unwrap();
    pace(&s);
    s.gesture(pan, Phase::Release).unwrap();
    let released = tick(&s);
    wait_until("a beat after the release", || tick(&s) > released + Q);
    stop_and_land(&s);
    let lane = lane_named(&s, song, "Built-in Synth Pan").unwrap();
    let p = points(&s, song, lane);
    let held: Vec<_> = p.iter().filter(|x| (x.1 - 0.2).abs() < 1e-9).collect();
    assert!(
        held.last().unwrap().0 > released + Q / 2,
        "held past the release: {p:?}"
    );
    s.close().unwrap();
}

/// Review Focus 2, Ruling 20: a Stop while a fader is held lands what
/// was written; the touch stays until its release, which writes nothing
/// more and adds no undo step.
#[test]
fn a_stop_while_touched_lands_the_write_and_keeps_the_touch() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    let gain = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.id == synth)
        .unwrap()
        .gain_db;
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.4) }).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    assert!(s.automation_state().unwrap().touched.contains(&t));
    let version = s.snapshot().unwrap().version;
    s.gesture(t, Phase::Release).unwrap();
    s.idle().unwrap();
    assert!(!s.automation_state().unwrap().touched.contains(&t));
    assert_eq!(
        s.snapshot().unwrap().version,
        version,
        "no step for the late release"
    );
    // Pre-flight I2: the late release sent Release, so nothing is held
    // through the next Play. The landed lane goes first, so nothing drives
    // the fader; no Locate (a Locate would end a wrong hold by itself).
    s.undo().unwrap();
    let from = tick(&s);
    s.play(song).unwrap();
    wait_until("a quarter beat on", || tick(&s) > from + Q / 4);
    let heard = fader_override(synth);
    assert!(
        heard.is_none_or(|d| (d - fader_db(0.4)).abs() > 1e-3),
        "nothing held from the late release: {heard:?}"
    );
    assert!(heard.is_none_or(|d| (d - gain).abs() < 1e-6), "{heard:?}");
    stop_and_land(&s);
    s.close().unwrap();
}

/// §4.7, §7.4: with MIDI recording too, the writes are part of the one
/// "Record" step.
#[test]
fn writes_join_the_record_pass() {
    use core_engine::fixtures::{Builder, Q as FQ};
    use core_model::command::{Command, LaneCmd, SequenceCmd};
    use core_model::lane::LaneInput;
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let _g = serial();
    let mut b = Builder::new();
    let synth_dest = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("S", "Keys", Some(synth_dest)).unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: keys,
        input: Some(LaneInput::Midi {
            port: Some("Keys".into()),
            channel: None,
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq,
        lane: keys,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(32 * FQ)),
    }))
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    s.debug_set_trash_dir(Some(dir.path().display().to_string()));
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    s.record(
        seq.0,
        vizdaw_ffi::RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    assert!(s.inject_midi("Keys".into(), vec![0x90, 60, 100]).unwrap());
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.6 }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    assert!(s.inject_midi("Keys".into(), vec![0x80, 60, 0]).unwrap());
    let at = tick(&s);
    wait_until("a beat on", || tick(&s) > at + Q);
    stop_and_land(&s);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    assert!(lane_named(&s, seq.0, "Built-in Synth Fader").is_some());
    s.undo().unwrap();
    assert_eq!(
        lane_named(&s, seq.0, "Built-in Synth Fader"),
        None,
        "one Record step"
    );
    s.close().unwrap();
}

/// §4.6, §7.5, Ruling 3: an Off strip plays no automation; suspended, it
/// does (the holder reloads at its next idle).
#[test]
fn off_ignores_automation_and_suspend_plays_it() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(
        t,
        Phase::Touch {
            value: Some(fader_norm(-18.0)),
        },
    )
    .unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    stop_and_land(&s);
    s.set_automation_mode(vec![synth], Mode::Off).unwrap();
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(300) {
        s.idle().unwrap();
        assert!(fader_override(synth).is_none(), "Off plays no automation");
        std::thread::sleep(Duration::from_millis(10));
    }
    set_automation_suspended(true);
    wait_until("suspended: the automation plays", || {
        s.idle().unwrap();
        fader_override(synth).is_some()
    });
    set_automation_suspended(false);
    stop_and_land(&s);
    s.close().unwrap();
}

/// Ruling 16, Review Focus 5: what cannot be gestured is refused and
/// changes nothing; a release with no touch does nothing.
#[test]
fn hostile_gestures_are_refused() {
    let _g = serial();
    let (s, _) = demo();
    let synth = strip(&s, "Built-in Synth");
    let version = s.snapshot().unwrap().version;
    let model =
        |r: Result<(), VizError>| assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}");
    model(s.gesture(
        Target::StripFader { strip: 999 },
        Phase::Touch { value: Some(0.5) },
    ));
    model(s.gesture(
        Target::SendLevel {
            strip: synth,
            send: 7,
        },
        Phase::Touch { value: Some(0.5) },
    ));
    model(s.gesture(
        Target::PluginParam {
            slot: 999,
            param: 0,
        },
        Phase::Touch { value: Some(0.5) },
    ));
    model(s.gesture(
        Target::VelocityScale { dest: 0 },
        Phase::Touch { value: Some(0.5) },
    ));
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        model(s.gesture(
            Target::StripFader { strip: synth },
            Phase::Touch { value: Some(v) },
        ));
        model(s.gesture(
            Target::StripFader { strip: synth },
            Phase::Value { value: v },
        ));
    }
    s.gesture(Target::StripPan { strip: synth }, Phase::Release)
        .unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
    assert!(s.set_automation_mode(vec![], Mode::Touch).is_err());
    assert!(
        s.set_automation_mode(vec![synth, 999], Mode::Touch)
            .is_err()
    );
    assert_eq!(s.snapshot().unwrap().version, version, "nothing changed");
    s.close().unwrap();
}

/// Ruling 19: settings out of range are refused and change nothing.
#[test]
fn hostile_automation_settings_are_refused() {
    let _g = serial();
    let good = vizdaw_ffi::automation::automation_settings();
    assert_eq!(
        good,
        AutomationSettingsRow {
            after_write: Mode::Latch,
            return_ms: 250,
            thinning: ThinningRow::Medium
        }
    );
    for bad in [
        AutomationSettingsRow {
            return_ms: 2001,
            ..good
        },
        AutomationSettingsRow {
            return_ms: u32::MAX,
            ..good
        },
        AutomationSettingsRow {
            after_write: Mode::Write,
            ..good
        },
    ] {
        assert!(
            matches!(set_automation_settings(bad), Err(VizError::Model { .. })),
            "{bad:?}"
        );
        assert_eq!(vizdaw_ffi::automation::automation_settings(), good);
    }
    let ok = AutomationSettingsRow {
        after_write: Mode::Touch,
        return_ms: 0,
        thinning: ThinningRow::High,
    };
    set_automation_settings(ok).unwrap();
    assert_eq!(vizdaw_ffi::automation::automation_settings(), ok);
}

/// Review Focus 2, Ruling 21: a device change mid-pass rebuilds the
/// engine; what was written up to then lands.
#[test]
fn a_device_rebuild_mid_pass_keeps_what_was_written() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.45) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    simulate_event(BackendEvent::SampleRateChanged(44_100.0));
    wait_until("the write to land", || {
        let _ = s.transport();
        s.idle().unwrap();
        s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation")
    });
    let lane = lane_named(&s, song, "Built-in Synth Fader").unwrap();
    assert!(
        points(&s, song, lane)
            .iter()
            .any(|p| (p.1 - 0.45).abs() < 1e-9)
    );
    let _ = s.stop();
    s.close().unwrap();
}

/// Task 10 review M2: a mode change while a static touch is open is
/// refused instead of joining the touch's "Set Fader" step; after the
/// release it is its own undo step.
#[test]
fn a_mode_change_waits_for_a_static_touch() {
    let _g = serial();
    let (s, _song) = demo();
    let synth = strip(&s, "Built-in Synth");
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    assert!(
        matches!(
            s.set_automation_mode(vec![synth], Mode::Latch),
            Err(VizError::Busy { .. })
        ),
        "refused while the touch is open"
    );
    s.gesture(t, Phase::Release).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Automation Mode")
    );
    s.close().unwrap();
}

/// Final review m4: a Play that ends at once (a range past the sequence's
/// end) never rolls; its pass lands instead of sending touches down the
/// write path while the transport stands still.
#[test]
fn a_play_that_ends_at_once_does_not_keep_writing() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    s.play_from(song, 10_000 * Q, Some(10_001 * Q)).unwrap();
    wait_until("the pass to end", || {
        s.idle().unwrap();
        !s.automation_state().unwrap().writing
    });
    assert!(!s.transport().unwrap().playing);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader"),
        "a touch while stopped is a static edit"
    );
    s.close().unwrap();
}

/// Pre-flight C1, Ruling 24: the idles that run right after Play, before
/// the renderer has rolled, do not end the pass; what is then written
/// lands at Stop.
#[test]
fn writing_survives_the_idle_before_the_roll() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    // The renderer is held, so these idles certainly run before the roll
    // (pre-flight check I-1).
    debug_hold_offline(true);
    s.play(song).unwrap();
    assert!(!s.transport().unwrap().playing, "not rolled yet");
    for _ in 0..3 {
        s.idle().unwrap();
    }
    assert!(
        s.automation_state().unwrap().writing,
        "still writing after the idles before the roll"
    );
    debug_hold_offline(false);
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    for v in [0.6, 0.7] {
        pace(&s);
        s.gesture(t, Phase::Value { value: v }).unwrap();
    }
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("the write landed");
    assert!(
        points(&s, song, lane)
            .iter()
            .any(|p| (p.1 - 0.7).abs() < 1e-9)
    );
    s.close().unwrap();
}

/// Pre-flight C2, Ruling 23: a Locate while writing lands what was
/// written so far as its own step; writing goes on from the new position
/// as a new pass; each pass undoes on its own.
#[test]
fn a_locate_while_writing_lands_and_starts_a_new_pass() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    let t = Target::StripFader { strip: synth };
    s.play(song).unwrap();
    wait_until("beat 2", || tick(&s) > 2 * Q);
    s.gesture(t, Phase::Touch { value: Some(0.4) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.5 }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("a beat on", || tick(&s) > at + Q);
    s.locate(0).unwrap();
    s.idle().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("the first pass landed");
    let first = points(&s, song, lane);
    assert!(
        first.iter().all(|p| p.0 >= Q),
        "nothing is written across the jump: {first:?}"
    );
    assert!(
        s.automation_state().unwrap().writing,
        "a new pass from the new position"
    );
    wait_until("the jump back", || tick(&s) < Q);
    s.gesture(t, Phase::Touch { value: Some(0.8) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    assert!(
        points(&s, song, lane)
            .iter()
            .any(|p| (p.1 - 0.8).abs() < 1e-9)
    );
    s.undo().unwrap();
    assert_eq!(points(&s, song, lane), first, "one step per pass");
    s.undo().unwrap();
    assert_eq!(
        lane_named(&s, song, "Built-in Synth Fader"),
        None,
        "both passes undone"
    );
    s.close().unwrap();
}

/// Pre-flight check I-2, Ruling 23: a Locate that arrives after Play
/// returned but before the renderer rolled (a ruler click queued during
/// the disk wait) starts the new pass all the same.
#[test]
fn a_locate_before_the_roll_keeps_writing() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    debug_hold_offline(true);
    s.play(song).unwrap();
    s.locate(Q).unwrap();
    s.idle().unwrap();
    debug_hold_offline(false);
    wait_until("playing", || s.transport().unwrap().playing);
    assert!(
        s.automation_state().unwrap().writing,
        "a pass after the Locate"
    );
    let t = Target::StripFader { strip: synth };
    wait_until("past beat 1", || tick(&s) > Q + Q / 4);
    s.gesture(t, Phase::Touch { value: Some(0.4) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("the write landed");
    assert!(
        points(&s, song, lane)
            .iter()
            .any(|p| (p.1 - 0.4).abs() < 1e-9)
    );
    s.close().unwrap();
}

/// Pre-flight C2 (its third failure), Ruling 23: Play pressed again while
/// writing lands what was written (the new tap would drop it) and writes
/// on as a new pass; each pass undoes on its own.
#[test]
fn a_play_while_writing_lands_and_starts_a_new_pass() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    let t = Target::StripFader { strip: synth };
    s.play(song).unwrap();
    wait_until("beat 2", || tick(&s) > 2 * Q);
    s.gesture(t, Phase::Touch { value: Some(0.4) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("a beat on", || tick(&s) > at + Q);
    s.play(song).unwrap();
    s.idle().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("the first pass landed");
    let first = points(&s, song, lane);
    assert!(first.iter().any(|p| (p.1 - 0.4).abs() < 1e-9), "{first:?}");
    assert!(s.automation_state().unwrap().writing, "a new pass");
    wait_until("playing", || s.transport().unwrap().playing);
    s.gesture(t, Phase::Touch { value: Some(0.8) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    assert!(
        points(&s, song, lane)
            .iter()
            .any(|p| (p.1 - 0.8).abs() < 1e-9)
    );
    s.undo().unwrap();
    assert_eq!(points(&s, song, lane), first, "one step per pass");
    s.undo().unwrap();
    assert_eq!(
        lane_named(&s, song, "Built-in Synth Fader"),
        None,
        "both passes undone"
    );
    s.close().unwrap();
}

/// Pre-flight M2, Ruling 20 as amended: a touch held over Stop has its
/// path picked again at the next Play: its strip set to Read meanwhile,
/// its next value writes nothing and changes no static value.
#[test]
fn a_touch_held_over_stop_takes_its_strips_mode_at_the_next_play() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    let gain = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.id == synth)
        .unwrap()
        .gain_db;
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    let t = Target::StripFader { strip: synth };
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    s.gesture(t, Phase::Touch { value: Some(0.4) }).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let lane = lane_named(&s, song, "Built-in Synth Fader").unwrap();
    let landed = points(&s, song, lane);
    s.set_automation_mode(vec![synth], Mode::Read).unwrap();
    s.play(song).unwrap();
    wait_until("a beat on", || s.transport().unwrap().playing);
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.6 }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Automation Mode"),
        "no step for the held touch"
    );
    assert_eq!(points(&s, song, lane), landed, "nothing more written");
    let g2 = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.id == synth)
        .unwrap()
        .gain_db;
    assert_eq!(g2, gain, "and no static change");
    s.close().unwrap();
}

/// Pre-flight M4, Ruling 19: a setting changed mid-pass applies from the
/// next Play; the landing equals the preview taken before the change.
#[test]
fn settings_changed_mid_pass_apply_from_the_next_play() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    let t = Target::StripFader { strip: synth };
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    // Steps of 0.15 dB: kept at Medium (0.1 dB), thinned away at High (0.2 dB).
    s.gesture(
        t,
        Phase::Touch {
            value: Some(fader_norm(-6.0)),
        },
    )
    .unwrap();
    for k in 1..=6 {
        pace(&s);
        s.gesture(
            t,
            Phase::Value {
                value: fader_norm(-6.0 + 0.15 * f64::from(k % 2)),
            },
        )
        .unwrap();
    }
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("the glide to pass", || tick(&s) > at + Q);
    let preview = s.automation_preview(t).unwrap();
    assert!(preview.len() >= 6, "{preview:?}");
    let high = AutomationSettingsRow {
        thinning: ThinningRow::High,
        ..vizdaw_ffi::automation::automation_settings()
    };
    set_automation_settings(high).unwrap();
    stop_and_land(&s);
    let lane = lane_named(&s, song, "Built-in Synth Fader").unwrap();
    let landed = points(&s, song, lane);
    let want: Vec<(i64, f64)> = preview.iter().map(|p| (p.tick, p.value)).collect();
    assert_eq!(landed, want, "landed with the thinning read at Play");
    s.close().unwrap();
}

/// Pre-flight I1, Ruling 14: two static gestures at once (a fader and a
/// pan, stopped) share one edit: one undo step, named after the first
/// touch, and one Undo puts both back.
#[test]
fn two_static_gestures_at_once_are_one_step() {
    let _g = serial();
    let (s, _) = demo();
    let synth = strip(&s, "Built-in Synth");
    let row = |s: &Session| {
        s.mixer()
            .unwrap()
            .strips
            .into_iter()
            .find(|x| x.id == synth)
            .unwrap()
    };
    let before = row(&s);
    let (fader, pan) = (
        Target::StripFader { strip: synth },
        Target::StripPan { strip: synth },
    );
    s.gesture(fader, Phase::Touch { value: Some(0.5) }).unwrap();
    s.gesture(pan, Phase::Touch { value: Some(0.2) }).unwrap();
    pace(&s);
    s.gesture(fader, Phase::Value { value: 0.4 }).unwrap();
    s.gesture(pan, Phase::Value { value: 0.1 }).unwrap();
    s.gesture(pan, Phase::Release).unwrap();
    s.gesture(fader, Phase::Release).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    let after = row(&s);
    assert!((after.gain_db - fader_db(0.4)).abs() < 1e-9, "{after:?}");
    assert!((after.pan - pan_of(0.1)).abs() < 1e-9, "{after:?}");
    s.undo().unwrap();
    let undone = row(&s);
    assert_eq!(
        (undone.gain_db, undone.pan),
        (before.gain_db, before.pan),
        "one Undo restores both"
    );
    assert_eq!(s.snapshot().unwrap().undo_label, None, "one step");
    s.close().unwrap();
}

/// Pre-flight I11, Ruling 26: a touch with no value starts at the value
/// heard now (here the bus fader's flat automation, 0.3), not at the
/// static value, so grabbing an automated fader writes no jump.
/// A session whose bus fader is automated flat at `v` in lane "Auto":
/// `(session, sequence, bus strip, lane)`.
fn automated_bus(v: f64) -> (Arc<Session>, u64, u64, u64) {
    use core_engine::fixtures::{Builder, Q as FQ};
    use core_model::automation::AutomationTarget;
    use core_model::event::EventBody;
    let mut b = Builder::new();
    let synth_dest = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(synth_dest)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 64 * FQ)]).unwrap();
    let bus = b.bus("Bus").unwrap();
    let auto = b.lane(seq, "Auto", None).unwrap();
    b.event(
        seq,
        auto,
        0,
        EventBody::Automation {
            target: AutomationTarget::StripFader(bus),
            value: v,
        },
    )
    .unwrap();
    (
        Session::from_document(b.finish()).unwrap(),
        seq.0,
        bus.0,
        auto.0,
    )
}

#[test]
fn a_touch_without_a_value_starts_at_the_heard_value() {
    let _g = serial();
    let (s, seq, bus, auto) = automated_bus(0.3);
    let t = Target::StripFader { strip: bus };
    assert!(
        (fader_norm(0.0) - 0.3).abs() > 0.1,
        "the static value is not the automation's"
    );
    s.set_automation_mode(vec![bus], Mode::Touch).unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    s.gesture(t, Phase::Touch { value: None }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.6 }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("the glide to pass", || tick(&s) > at + Q);
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let p = points(&s, seq, auto);
    let moved = p
        .iter()
        .position(|x| (x.1 - 0.6).abs() < 1e-9)
        .expect("the move is written");
    assert!(
        p[..moved].iter().all(|x| (x.1 - 0.3).abs() < 1e-6),
        "no jump before the move: {p:?}"
    );
    s.close().unwrap();
}

/// Pre-flight I12, Ruling 11: a Trim grab is sent as an offset of 0, so
/// the automation is heard unchanged until the hand moves (an absolute
/// touch value would play for a block first); a move of +3 dB adds 3 dB.
#[test]
fn a_trim_grab_does_not_jump() {
    let _g = serial();
    let (s, seq, bus, _) = automated_bus(fader_norm(-12.0));
    let t = Target::StripFader { strip: bus };
    s.set_automation_mode(vec![bus], Mode::Trim).unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    s.gesture(
        t,
        Phase::Touch {
            value: Some(fader_norm(0.0)),
        },
    )
    .unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(50) {
        let d = fader_override(bus);
        assert!(
            d.is_some_and(|d| (d + 12.0).abs() < 1e-6),
            "the grab is heard as the automation: {d:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    pace(&s);
    s.gesture(
        t,
        Phase::Value {
            value: fader_norm(3.0),
        },
    )
    .unwrap();
    wait_until("+3 dB on the automation", || {
        fader_override(bus).is_some_and(|d| (d + 9.0).abs() < 1e-6)
    });
    s.gesture(t, Phase::Release).unwrap();
    stop_and_land(&s);
    s.close().unwrap();
}

/// Pre-flight I6, Ruling 25: a target deleted mid-pass drops its own
/// write with a problem line; the rest of the pass lands.
#[test]
fn writes_to_a_target_deleted_mid_pass_are_dropped_and_the_rest_lands() {
    let _g = serial();
    let (s, song) = demo();
    s.add_bus("Gone".into()).unwrap();
    let synth = strip(&s, "Built-in Synth");
    let bus = strip(&s, "Gone");
    // The bus writes (Write) and becomes After Write at landing, unless
    // it is gone (Ruling 25: After Write leaves out deleted strips).
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    s.set_automation_mode(vec![bus], Mode::Write).unwrap();
    let targets = [
        Target::StripFader { strip: synth },
        Target::StripFader { strip: bus },
    ];
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    for t in targets {
        s.gesture(t, Phase::Touch { value: Some(0.45) }).unwrap();
    }
    pace(&s);
    s.gesture(targets[0], Phase::Release).unwrap();
    pace(&s);
    s.remove_bus(bus).unwrap();
    // Pre-flight M9: the bus fader's touch goes with its strip.
    assert!(!s.automation_state().unwrap().touched.contains(&targets[1]));
    pace(&s);
    s.stop().unwrap();
    wait_until("the transport to stop", || !s.transport().unwrap().playing);
    let news = s.idle().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let lane = lane_named(&s, song, "Built-in Synth Fader").expect("the synth's write landed");
    assert!(
        points(&s, song, lane)
            .iter()
            .any(|p| (p.1 - 0.45).abs() < 1e-9)
    );
    assert_eq!(lane_named(&s, song, "Gone Fader"), None);
    assert!(
        news.problems
            .iter()
            .any(|p| p.contains("was not written: it no longer exists")),
        "{:?}",
        news.problems
    );
    let modes = s.automation_state().unwrap().modes;
    assert!(
        modes
            .iter()
            .any(|m| m.strip == synth && m.mode == Mode::Latch),
        "{modes:?}"
    );
    assert!(modes.iter().all(|m| m.strip != bus));
    s.close().unwrap();
}

/// Ruling 25, Ruling 28 (pre-flight I6, M16): a recording that records
/// nothing (its punch range is never reached) still lands the automation
/// written meanwhile, as "Write Automation", outside the punch range.
#[test]
fn writes_land_when_the_recording_records_nothing() {
    use core_engine::fixtures::{Builder, Q as FQ};
    use core_model::command::{Command, LaneCmd, SequenceCmd};
    use core_model::lane::LaneInput;
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let _g = serial();
    let mut b = Builder::new();
    let synth_dest = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("S", "Keys", Some(synth_dest)).unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: keys,
        input: Some(LaneInput::Midi {
            port: Some("Keys".into()),
            channel: None,
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq,
        lane: keys,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(32 * FQ)),
    }))
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    s.debug_set_trash_dir(Some(dir.path().display().to_string()));
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Touch).unwrap();
    let punch = vizdaw_ffi::selection::TickRange {
        start: 16 * Q,
        end: 17 * Q,
    };
    s.record(
        seq.0,
        vizdaw_ffi::RecordSpecRow {
            replace: true,
            punch: Some(punch),
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.6 }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    let at = tick(&s);
    wait_until("a beat on", || tick(&s) > at + Q);
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let lane = lane_named(&s, seq.0, "Built-in Synth Fader").expect("the automation landed");
    let p = points(&s, seq.0, lane);
    assert!(p.iter().any(|x| (x.1 - 0.6).abs() < 1e-9), "{p:?}");
    assert!(
        p.iter().all(|x| x.0 < punch.start),
        "written outside the punch range: {p:?}"
    );
    s.close().unwrap();
}

/// Task 2 review M5: every engine built after a device change gets the
/// process-wide suspend before its first load, so a rebuild while
/// suspended stays suspended.
#[test]
fn a_device_rebuild_while_suspended_stays_suspended() {
    use vizdaw_ffi::host::{debug_automation_suspended, hosts_created};
    let _g = serial();
    let (s, song) = demo();
    set_automation_suspended(true);
    s.play(song).unwrap();
    wait_until("playing", || s.transport().unwrap().playing);
    // The suspend's reload is taken here, so no idle reloads after the
    // rebuild below.
    s.idle().unwrap();
    assert_eq!(debug_automation_suspended(), Some(true));
    let built = hosts_created();
    simulate_event(BackendEvent::SampleRateChanged(44_100.0));
    wait_until("the rebuild", || {
        s.transport().unwrap().playing && hosts_created() > built
    });
    assert_eq!(
        debug_automation_suspended(),
        Some(true),
        "the rebuilt engine is suspended from its first load"
    );
    set_automation_suspended(false);
    stop_and_land(&s);
    s.close().unwrap();
}

/// Controller ruling (1) on the Task 5 forward (Plan 4d Ruling 13 kept):
/// in Read while stopped, a released gesture on an automated fader stays
/// at the value the user left (the new static value); at the next Play the
/// automation takes over.
#[test]
fn a_read_release_while_stopped_stays_until_play() {
    let _g = serial();
    let (s, seq, bus, _) = automated_bus(fader_norm(-12.0));
    let t = Target::StripFader { strip: bus };
    s.play(seq).unwrap();
    wait_until("the automation", || {
        fader_override(bus).is_some_and(|d| (d + 12.0).abs() < 1e-6)
    });
    stop_and_land(&s);
    s.gesture(
        t,
        Phase::Touch {
            value: Some(fader_norm(-3.0)),
        },
    )
    .unwrap();
    wait_until("the touch heard", || {
        fader_override(bus).is_some_and(|d| (d + 3.0).abs() < 1e-6)
    });
    s.gesture(t, Phase::Release).unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(200) {
        s.idle().unwrap();
        let d = fader_override(bus);
        assert!(
            d.is_none_or(|d| (d + 3.0).abs() < 1e-6),
            "stays at the user's value while stopped: {d:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let gain = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.id == bus)
        .unwrap()
        .gain_db;
    assert!((gain + 3.0).abs() < 1e-9, "{gain}");
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    s.play(seq).unwrap();
    wait_until("the automation at Play", || {
        fader_override(bus).is_some_and(|d| (d + 12.0).abs() < 1e-6)
    });
    stop_and_land(&s);
    s.close().unwrap();
}

/// Review I-1 (Ruling 11): a Trim touch held over a Locate stays an offset
/// over the automation, from its own grab: no jump to the hand's absolute
/// position.
#[test]
fn a_trim_touch_held_over_a_locate_stays_an_offset() {
    let _g = serial();
    let (s, seq, bus, _) = automated_bus(fader_norm(-12.0));
    let t = Target::StripFader { strip: bus };
    s.set_automation_mode(vec![bus], Mode::Trim).unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    s.gesture(
        t,
        Phase::Touch {
            value: Some(fader_norm(0.0)),
        },
    )
    .unwrap();
    pace(&s);
    s.gesture(
        t,
        Phase::Value {
            value: fader_norm(3.0),
        },
    )
    .unwrap();
    wait_until("+3 dB on the automation", || {
        fader_override(bus).is_some_and(|d| (d + 9.0).abs() < 1e-6)
    });
    s.locate(4 * Q).unwrap();
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().writing);
    wait_until("past the jump", || tick(&s) > 4 * Q + Q / 4);
    pace(&s);
    s.gesture(
        t,
        Phase::Value {
            value: fader_norm(4.0),
        },
    )
    .unwrap();
    wait_until("+4 dB on the automation", || {
        let d = fader_override(bus);
        assert!(
            d.is_none_or(|d| d < 0.0),
            "no jump to the hand's +4 dB: {d:?}"
        );
        d.is_some_and(|d| (d + 8.0).abs() < 1e-6)
    });
    s.gesture(t, Phase::Release).unwrap();
    stop_and_land(&s);
    s.close().unwrap();
}

/// A session whose bus fader (lane "Auto" of "S") ramps from 0.2 at tick
/// 0 to 0.8 at `32 Q`, and an empty sequence "Other" first:
/// `(session, S, Other, bus, lane)`.
fn ramped_bus() -> (Arc<Session>, u64, u64, u64, u64) {
    use core_engine::fixtures::{Builder, Q as FQ};
    use core_model::automation::AutomationTarget;
    use core_model::event::EventBody;
    let mut b = Builder::new();
    let synth_dest = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (other, onotes) = b.sequence("Other", "Notes", Some(synth_dest)).unwrap();
    b.notes(other, onotes, &[(0, 60, 0x8000, 64 * FQ)]).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(synth_dest)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 64 * FQ)]).unwrap();
    let bus = b.bus("Bus").unwrap();
    let auto = b.lane(seq, "Auto", None).unwrap();
    for (tick, v) in [(0, 0.2), (32 * FQ, 0.8)] {
        b.event(
            seq,
            auto,
            tick,
            EventBody::Automation {
                target: AutomationTarget::StripFader(bus),
                value: v,
            },
        )
        .unwrap();
    }
    (
        Session::from_document(b.finish()).unwrap(),
        seq.0,
        other.0,
        bus.0,
        auto.0,
    )
}

/// The ramp's value at `tick`.
fn ramp_at(tick: i64) -> f64 {
    0.2 + 0.6 * tick as f64 / (32 * Q) as f64
}

/// The first point at or after `from` holds `v` (the held value written
/// from the pass's start).
fn held_from(p: &[(i64, f64)], from: i64, v: f64) {
    let first = p.iter().find(|x| x.0 >= from);
    assert!(
        first.is_some_and(|x| (x.1 - v).abs() < 1e-3),
        "held at {v} from {from}: {p:?}"
    );
}

/// Review I-2 (Ruling 9): Play from a tick elsewhere holds a Write strip
/// at its value there, not where the transport stood before.
#[test]
fn write_holds_take_the_play_from_tick() {
    let _g = serial();
    let (s, seq, _, bus, lane) = ramped_bus();
    s.play(seq).unwrap();
    wait_until("beat 2", || tick(&s) > 2 * Q);
    stop_and_land(&s);
    s.set_automation_mode(vec![bus], Mode::Write).unwrap();
    s.play_from(seq, 16 * Q, None).unwrap();
    wait_until("a beat on", || tick(&s) > 17 * Q);
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    held_from(&points(&s, seq, lane), 16 * Q, ramp_at(16 * Q));
    s.close().unwrap();
}

/// Review I-2 with check I-2: a Locate before the roll starts the pass
/// again from the new position, and its Write holds take the values
/// there, not where the held renderer last published. (A mid-pass Locate
/// takes the same path: its landing's After Write is put off while an
/// edit is open, and a strip switched to Write mid-pass is not in the
/// pass's Write strips.)
#[test]
fn write_holds_after_a_locate_take_the_new_tick() {
    let _g = serial();
    let (s, seq, _, bus, lane) = ramped_bus();
    s.set_automation_mode(vec![bus], Mode::Write).unwrap();
    debug_hold_offline(true);
    s.play(seq).unwrap();
    s.locate(16 * Q).unwrap();
    s.idle().unwrap();
    debug_hold_offline(false);
    assert!(s.automation_state().unwrap().writing);
    wait_until("a beat on", || tick(&s) > 17 * Q);
    stop_and_land(&s);
    held_from(&points(&s, seq, lane), 16 * Q, ramp_at(16 * Q));
    s.close().unwrap();
}

/// Review I-2: the first Play of a sequence after another played holds
/// Write strips at the new sequence's start, not at the old position.
#[test]
fn write_holds_after_switching_sequence_take_its_start() {
    let _g = serial();
    let (s, seq, other, bus, lane) = ramped_bus();
    s.play(other).unwrap();
    wait_until("eight beats", || tick(&s) > 8 * Q);
    stop_and_land(&s);
    s.set_automation_mode(vec![bus], Mode::Write).unwrap();
    s.play(seq).unwrap();
    wait_until("a beat on", || {
        s.transport().unwrap().seq == Some(seq) && tick(&s) > Q
    });
    stop_and_land(&s);
    held_from(&points(&s, seq, lane), 0, ramp_at(0));
    s.close().unwrap();
}

/// Review I-3: clicking an automated Read fader (a touch with no value)
/// and letting go changes no static value and makes no undo step.
#[test]
fn a_click_on_an_automated_read_fader_changes_nothing() {
    let _g = serial();
    let (s, seq, bus, _) = automated_bus(fader_norm(-12.0));
    let t = Target::StripFader { strip: bus };
    let gain = |s: &Session| {
        s.mixer()
            .unwrap()
            .strips
            .iter()
            .find(|x| x.id == bus)
            .unwrap()
            .gain_db
    };
    let before = gain(&s);
    s.play(seq).unwrap();
    wait_until("the automation", || {
        fader_override(bus).is_some_and(|d| (d + 12.0).abs() < 1e-6)
    });
    s.gesture(t, Phase::Touch { value: None }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    stop_and_land(&s);
    s.gesture(t, Phase::Touch { value: None }).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label, None, "no step");
    assert_eq!(gain(&s), before, "no static change");
    s.close().unwrap();
}

/// Controller ruling (3): a device rebuild mid-pass while the transport
/// keeps playing lands the pass and starts a new one, so writing strips
/// keep writing; each pass undoes on its own.
#[test]
fn a_device_rebuild_mid_pass_starts_a_new_pass() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    let t = Target::StripFader { strip: synth };
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    s.gesture(t, Phase::Touch { value: Some(0.45) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    simulate_event(BackendEvent::SampleRateChanged(44_100.0));
    wait_until("the first pass to land", || {
        let _ = s.transport();
        s.idle().unwrap();
        s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation")
    });
    wait_until("a new pass", || {
        s.idle().unwrap();
        s.automation_state().unwrap().writing && s.transport().unwrap().playing
    });
    let lane = lane_named(&s, song, "Built-in Synth Fader").unwrap();
    let first = points(&s, song, lane);
    let at = tick(&s);
    wait_until("on", || tick(&s) > at + Q / 4);
    s.gesture(t, Phase::Touch { value: Some(0.8) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    stop_and_land(&s);
    assert!(
        points(&s, song, lane)
            .iter()
            .any(|p| (p.1 - 0.8).abs() < 1e-9),
        "the new pass wrote"
    );
    s.undo().unwrap();
    assert_eq!(points(&s, song, lane), first, "one step per pass");
    s.close().unwrap();
}

/// Controller ruling (3), the other half (M17): another session taking the
/// engine lands the pass and starts none.
#[test]
fn another_session_taking_the_engine_starts_no_pass() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    let t = Target::StripFader { strip: synth };
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    s.gesture(t, Phase::Touch { value: Some(0.45) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Release).unwrap();
    pace(&s);
    let (other, other_song) = demo();
    other.play(other_song).unwrap();
    let mut problems = Vec::new();
    wait_until("the pass to land", || {
        problems.extend(s.idle().unwrap().problems);
        s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation")
    });
    for _ in 0..5 {
        std::thread::sleep(Duration::from_millis(12));
        problems.extend(s.idle().unwrap().problems);
        assert!(
            !s.automation_state().unwrap().writing,
            "no new pass in another session's engine"
        );
    }
    // Not even tried: a pass in another session's engine cannot start.
    assert!(
        !problems.iter().any(|p| p.contains("not being written")),
        "{problems:?}"
    );
    other.stop().unwrap();
    other.close().unwrap();
    s.close().unwrap();
}

/// Review m2 (Ruling 23): two passes that wait for the same open static
/// edit land as two undo steps, not one.
#[test]
fn passes_waiting_for_an_open_edit_land_as_their_own_steps() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    let master = s.mixer().unwrap().strips.last().unwrap().id;
    s.set_automation_mode(vec![synth], Mode::Latch).unwrap();
    let t = Target::StripFader { strip: synth };
    s.play(song).unwrap();
    wait_until("beat 1", || tick(&s) > Q);
    // The master is Read: its touch opens the static edit and keeps it.
    let m = Target::StripFader { strip: master };
    s.gesture(m, Phase::Touch { value: Some(0.5) }).unwrap();
    for v in [0.4, 0.7] {
        s.gesture(t, Phase::Touch { value: Some(v) }).unwrap();
        pace(&s);
        s.gesture(t, Phase::Release).unwrap();
        pace(&s);
        s.locate(Q).unwrap();
        s.idle().unwrap();
        wait_until("playing on", || tick(&s) > Q + Q / 4);
    }
    s.gesture(m, Phase::Release).unwrap();
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    s.undo().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation"),
        "two steps"
    );
    s.undo().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    s.close().unwrap();
}

/// Review m3: a static gesture's step reaches the app through `idle`'s
/// snapshot.
#[test]
fn a_static_gesture_sends_a_snapshot() {
    let _g = serial();
    let (s, _) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.idle().unwrap();
    let t = Target::StripFader { strip: synth };
    s.gesture(t, Phase::Touch { value: Some(0.5) }).unwrap();
    pace(&s);
    s.gesture(t, Phase::Value { value: 0.4 }).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    let snap = s.idle().unwrap().snapshot;
    assert_eq!(
        snap.and_then(|x| x.undo_label).as_deref(),
        Some("Set Fader")
    );
    s.close().unwrap();
}

/// Re-review N1 (controller ruling 3, Ruling 9): a device rebuild during a
/// Write pass played from 16 Q lands the pass first (the strip becomes
/// After Write, so the new pass holds nothing of it) and never writes the
/// fader's bar-1 value after the rebuild.
#[test]
fn a_rebuild_during_a_write_pass_lands_before_the_new_pass() {
    let _g = serial();
    let (s, seq, _, bus, lane) = ramped_bus();
    s.set_automation_mode(vec![bus], Mode::Write).unwrap();
    s.play_from(seq, 16 * Q, None).unwrap();
    wait_until("20 Q", || tick(&s) > 20 * Q);
    simulate_event(BackendEvent::SampleRateChanged(44_100.0));
    wait_until("the first pass to land and a new one", || {
        let _ = s.transport();
        s.idle().unwrap();
        s.snapshot().unwrap().undo_label.as_deref() == Some("Write Automation")
            && s.automation_state().unwrap().writing
    });
    let modes = s.automation_state().unwrap().modes;
    assert!(
        modes
            .iter()
            .any(|m| m.strip == bus && m.mode == Mode::Latch),
        "{modes:?}"
    );
    let at = tick(&s);
    wait_until("a beat on", || tick(&s) > at + Q);
    stop_and_land(&s);
    let p = points(&s, seq, lane);
    // The new pass held nothing of the strip (it was After Write by then):
    // nothing is written past the first pass's end.
    assert!(
        // The first pass's last point and its end edge only.
        p.iter().filter(|x| x.0 > 20 * Q && x.0 < 32 * Q).count() <= 2,
        "no hold from the new pass: {p:?}"
    );
    assert!(
        p.iter()
            .filter(|x| x.0 > 20 * Q)
            .all(|x| (x.1 - ramp_at(0)).abs() > 1e-3),
        "no bar-1 hold after the rebuild: {p:?}"
    );
    s.close().unwrap();
}

/// Re-review N1: with the landing put off by an open edit, the strip is
/// still Write when the new pass starts, and its holds take the value at
/// the tick the rebuilt engine resumed from, not bar 1.
#[test]
fn a_rebuild_restart_holds_at_the_resume_tick() {
    let _g = serial();
    let (s, seq, _, bus, lane) = ramped_bus();
    let master = s.mixer().unwrap().strips.last().unwrap().id;
    s.set_automation_mode(vec![bus], Mode::Write).unwrap();
    s.play_from(seq, 16 * Q, None).unwrap();
    wait_until("20 Q", || tick(&s) > 20 * Q);
    // The master is Read: its held touch keeps the static edit open.
    let m = Target::StripFader { strip: master };
    s.gesture(m, Phase::Touch { value: Some(0.5) }).unwrap();
    let reb = tick(&s);
    simulate_event(BackendEvent::SampleRateChanged(44_100.0));
    let mut restarted = false;
    wait_until("a new pass after the rebuild", || {
        let before = s.automation_state().unwrap().writing;
        let _ = s.transport();
        s.idle().unwrap();
        restarted |= !before;
        tick(&s) > 21 * Q && s.automation_state().unwrap().writing
    });
    let _ = restarted;
    let at = tick(&s);
    wait_until("a beat on", || tick(&s) > at + Q);
    s.gesture(m, Phase::Release).unwrap();
    stop_and_land(&s);
    let p = points(&s, seq, lane);
    let after: Vec<_> = p.iter().filter(|x| x.0 > 20 * Q).collect();
    assert!(
        after.iter().all(|x| (x.1 - ramp_at(0)).abs() > 1e-3),
        "no bar-1 hold: {p:?}"
    );
    let want = ramp_at(reb);
    assert!(
        after.iter().any(|x| x.1 > want - 1e-3 && x.1 < want + 0.02),
        "held at the resume tick (about {want}): {p:?}"
    );
    s.close().unwrap();
}

/// Re-review m4: the Write holds re-sent at the roll (so a Locate applied
/// after them cannot end them) are stamped again at the held values and
/// add no point: an untouched Write pass lands each held control as one
/// flat stretch, its two ends inside the two edges.
#[test]
fn a_resent_hold_adds_no_point() {
    let _g = serial();
    let (s, song) = demo();
    let synth = strip(&s, "Built-in Synth");
    s.set_automation_mode(vec![synth], Mode::Write).unwrap();
    s.play(song).unwrap();
    // Idles run meanwhile: one of them sees the roll and re-sends.
    wait_until("two beats", || {
        s.idle().unwrap();
        tick(&s) > 2 * Q
    });
    stop_and_land(&s);
    for name in [
        "Built-in Synth Fader",
        "Built-in Synth Pan",
        "Built-in Synth Mute",
    ] {
        let lane = lane_named(&s, song, name).unwrap();
        let p = points(&s, song, lane);
        assert_eq!(p.len(), 4, "{name}: {p:?}");
        assert!(
            p.windows(2).all(|w| (w[0].1 - w[1].1).abs() < 1e-9),
            "{name}: {p:?}"
        );
    }
    s.close().unwrap();
}

/// Re-review 2 N4: a Write touch held over a pre-roll Locate into a pass
/// that still holds its strip, and released before the first idle sees
/// the roll, keeps its released value: the holds re-sent at the roll
/// (m4) spare it.
#[test]
fn a_write_touch_held_over_the_pass_start_is_not_re_held() {
    let _g = serial();
    let (s, seq, _, bus, lane) = ramped_bus();
    s.set_automation_mode(vec![bus], Mode::Write).unwrap();
    let t = Target::StripFader { strip: bus };
    debug_hold_offline(true);
    s.play(seq).unwrap();
    s.gesture(t, Phase::Touch { value: Some(0.7) }).unwrap();
    s.locate(16 * Q).unwrap();
    s.idle().unwrap();
    s.gesture(t, Phase::Release).unwrap();
    debug_hold_offline(false);
    wait_until("a beat on", || {
        s.idle().unwrap();
        tick(&s) > 17 * Q
    });
    stop_and_land(&s);
    let p = points(&s, seq, lane);
    let after: Vec<_> = p.iter().filter(|x| x.0 >= 16 * Q && x.0 < 17 * Q).collect();
    assert!(
        !after.is_empty() && after.iter().all(|x| (x.1 - 0.7).abs() < 1e-9),
        "the released 0.7 stays: {p:?}"
    );
    s.close().unwrap();
}

/// Re-review 2 N4, moved after the roll: the same touch moves to 0.9 and
/// is released once the transport rolled, before any idle sees the roll;
/// the holds re-sent then do not take it back to its earlier value.
#[test]
fn a_write_touch_held_over_the_pass_start_and_moved_after_the_roll_keeps_its_value() {
    let _g = serial();
    let (s, seq, _, bus, lane) = ramped_bus();
    s.set_automation_mode(vec![bus], Mode::Write).unwrap();
    let t = Target::StripFader { strip: bus };
    debug_hold_offline(true);
    s.play(seq).unwrap();
    s.gesture(t, Phase::Touch { value: Some(0.7) }).unwrap();
    s.locate(16 * Q).unwrap();
    s.idle().unwrap();
    debug_hold_offline(false);
    wait_until("the roll", || tick(&s) > 16 * Q);
    s.gesture(t, Phase::Value { value: 0.9 }).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    wait_until("a beat on", || {
        s.idle().unwrap();
        tick(&s) > 17 * Q
    });
    stop_and_land(&s);
    let p = points(&s, seq, lane);
    // Held at 0.7 from the pass's start, then the move to 0.9 holds to
    // the end: no re-sent hold takes it back.
    let after: Vec<_> = p.iter().filter(|x| x.0 >= 16 * Q && x.0 < 17 * Q).collect();
    let moved = after.iter().position(|x| (x.1 - 0.9).abs() < 1e-9);
    assert!(
        moved.is_some_and(|m| after[..m].iter().all(|x| (x.1 - 0.7).abs() < 1e-9)
            && after[m..].iter().all(|x| (x.1 - 0.9).abs() < 1e-9)),
        "0.7, then the released 0.9 stays: {p:?}"
    );
    s.close().unwrap();
}

/// Re-review 3 n1 (spec §7.3 Write; controller ruling, fix round 4): a
/// fader held on a Write strip while Play is pressed becomes a writing
/// touch for that pass. Every move after Play is written as it is heard,
/// and its release holds the last value to Stop; the landed lane is what
/// was heard.
#[test]
fn a_write_fader_held_over_play_writes_its_ride() {
    let _g = serial();
    let (s, seq, _, bus, lane) = ramped_bus();
    s.set_automation_mode(vec![bus], Mode::Write).unwrap();
    let t = Target::StripFader { strip: bus };
    // Held while stopped: the static path.
    s.gesture(t, Phase::Touch { value: Some(0.7) }).unwrap();
    pace(&s);
    s.play(seq).unwrap();
    wait_until("the roll", || {
        s.idle().unwrap();
        tick(&s) > Q / 4
    });
    for (v, until) in [(0.9, 2 * Q), (0.5, 4 * Q)] {
        s.gesture(t, Phase::Value { value: v }).unwrap();
        wait_until("the move heard", || {
            s.idle().unwrap();
            fader_override(bus).is_some_and(|d| (d - fader_db(v)).abs() < 1e-6)
        });
        wait_until("playing on", || {
            s.idle().unwrap();
            tick(&s) > until
        });
    }
    s.gesture(t, Phase::Release).unwrap();
    let released = tick(&s);
    wait_until("a beat after the release", || {
        s.idle().unwrap();
        tick(&s) > released + Q
    });
    // Write keeps the held value to Stop.
    let heard = fader_override(bus);
    assert!(
        heard.is_some_and(|d| (d - fader_db(0.5)).abs() < 1e-6),
        "the released 0.5 is held: {heard:?}"
    );
    stop_and_land(&s);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let p = points(&s, seq, lane);
    // From the pass's start to its end edge (where the ramp resumes after
    // the Stop): held at 0.7, then 0.9, then 0.5 to the end; nothing of
    // the ramp, and the lane ends where it was heard.
    let ride = |x: &&(i64, f64)| [0.7, 0.9, 0.5].iter().any(|v| (x.1 - v).abs() < 1e-9);
    let pass: Vec<_> = p.iter().filter(|x| x.0 >= 0).take_while(ride).collect();
    let at = |v: f64| pass.iter().position(|x| (x.1 - v).abs() < 1e-9);
    assert!(
        pass.first().is_some_and(|x| x.0 == 0)
            && at(0.7) == Some(0)
            && at(0.7) < at(0.9)
            && at(0.9) < at(0.5),
        "0.7 from the start, then 0.9, then 0.5: {p:?}"
    );
    assert!(
        pass.last()
            .is_some_and(|x| (x.1 - 0.5).abs() < 1e-9 && x.0 > released + Q / 2),
        "0.5 held to the Stop: {p:?}"
    );
    s.close().unwrap();
}

/// Re-review 3 m1: a Write touch held over a pass's start stays touched
/// in the engine (`Touch` only, no `Hold`), so a Stop's end of the holds
/// leaves it: held over a pre-roll Locate, not released over Stop, it is
/// still heard at the hand's value when the strip then plays in Read.
#[test]
fn a_write_touch_held_over_the_pass_start_is_heard_after_stop() {
    let _g = serial();
    let (s, seq, _, bus, _) = ramped_bus();
    s.set_automation_mode(vec![bus], Mode::Write).unwrap();
    let t = Target::StripFader { strip: bus };
    debug_hold_offline(true);
    s.play(seq).unwrap();
    s.gesture(t, Phase::Touch { value: Some(0.7) }).unwrap();
    s.locate(16 * Q).unwrap();
    s.idle().unwrap();
    debug_hold_offline(false);
    wait_until("a beat on", || {
        s.idle().unwrap();
        tick(&s) > 17 * Q
    });
    stop_and_land(&s);
    s.set_automation_mode(vec![bus], Mode::Read).unwrap();
    s.play(seq).unwrap();
    wait_until("playing", || {
        s.idle().unwrap();
        tick(&s) > Q
    });
    wait_until("the fader heard", || {
        s.idle().unwrap();
        fader_override(bus).is_some()
    });
    pace(&s);
    let heard = fader_override(bus);
    assert!(
        heard.is_some_and(|d| (d - fader_db(0.7)).abs() < 1e-6),
        "the hand's 0.7 is heard: {heard:?}"
    );
    s.gesture(t, Phase::Release).unwrap();
    stop_and_land(&s);
    s.close().unwrap();
}

/// Rides a Trim fader held over Play (re-review 4 N1): the ramped bus in
/// Trim is touched with `touch` while stopped (then moved to `stopped`,
/// when given, still stopped), Play, at about 1 Q moved to `to`, held to
/// 4 Q and released. Answers the offsets heard over the
/// lane at about 2 Q and 3.5 Q, and those landed between the move and the
/// release, in dB (heard or landed minus the ramp).
fn trim_ride_over_play(
    touch: Option<f64>,
    stopped: Option<f64>,
    to: f64,
) -> (Vec<f64>, Vec<(i64, f64)>) {
    let (s, seq, _, bus, lane) = ramped_bus();
    s.set_automation_mode(vec![bus], Mode::Trim).unwrap();
    let t = Target::StripFader { strip: bus };
    s.gesture(t, Phase::Touch { value: touch }).unwrap();
    pace(&s);
    if let Some(v) = stopped {
        s.gesture(t, Phase::Value { value: v }).unwrap();
        pace(&s);
    }
    s.play(seq).unwrap();
    wait_until("beat 1", || {
        s.idle().unwrap();
        tick(&s) > Q
    });
    let before = fader_override(bus);
    let lane_db = fader_db(ramp_at(tick(&s)));
    assert!(
        before.is_some_and(|d| (d - lane_db).abs() < 0.5),
        "the lane is heard until the hand moves: {before:?}, lane {lane_db}"
    );
    s.gesture(t, Phase::Value { value: to }).unwrap();
    let moved = tick(&s);
    let mut heard = Vec::new();
    for at in [2 * Q, 7 * Q / 2] {
        wait_until("playing on", || {
            s.idle().unwrap();
            tick(&s) > at
        });
        let d = fader_override(bus).expect("the bus is heard");
        heard.push(d - fader_db(ramp_at(tick(&s))));
    }
    wait_until("4 Q", || {
        s.idle().unwrap();
        tick(&s) > 4 * Q
    });
    s.gesture(t, Phase::Release).unwrap();
    let released = tick(&s);
    stop_and_land(&s);
    // The landed lane inside the ride, away from both edges (where the
    // move and the release land depends on the block that took them;
    // Task 9b re-review m1): read every Q/4 from the move plus Q/8 to the
    // release minus Q/8, as the lane plays (linear between points).
    let p = points(&s, seq, lane);
    let mut landed = Vec::new();
    let mut k = moved + Q / 8;
    while k <= released - Q / 8 {
        let v = lane_at(&p, k).expect("the lane plays inside the ride");
        landed.push((k, fader_db(v) - fader_db(ramp_at(k))));
        k += Q / 4;
    }
    s.close().unwrap();
    (heard, landed)
}

/// The value lane points `p` (by tick) play at tick `k`: linear between
/// the points around it, the last one at the same tick winning; `None`
/// before the first point.
fn lane_at(p: &[(i64, f64)], k: i64) -> Option<f64> {
    let after = p.iter().position(|x| x.0 > k);
    let before = match after {
        Some(0) => return None,
        Some(a) => p.get(a - 1)?,
        None => return p.last().map(|x| x.1),
    };
    let next = p.get(after?)?;
    let span = (next.0 - before.0) as f64;
    Some(before.1 + (next.1 - before.1) * (k - before.0) as f64 / span)
}

/// The landed offsets are one offset (the ramp's shape kept, no flat
/// absolute line), spread over the ride.
fn assert_shape_kept(landed: &[(i64, f64)], off: f64) {
    assert!(
        landed.len() >= 2 && landed.last().unwrap().0 - landed[0].0 > Q,
        "points over the ride: {landed:?}"
    );
    assert!(
        landed.iter().all(|x| (x.1 - off).abs() < 0.3),
        "the lane plus {off} dB, its shape kept: {landed:?}"
    );
}

/// Re-review 4 N1 (Ruling 11, spec §7.3 Trim): a Trim fader held over
/// Play becomes an offset from the hand's value at the pass's start; a
/// push of 3 dB is heard and written as the lane plus 3 dB.
#[test]
fn a_trim_fader_held_over_play_rides_as_an_offset() {
    let _g = serial();
    let (heard, landed) = trim_ride_over_play(Some(0.5), None, fader_norm(fader_db(0.5) + 3.0));
    assert!(
        heard.iter().all(|d| (d - 3.0).abs() < 0.5),
        "heard as the lane plus 3 dB: {heard:?}"
    );
    assert_shape_kept(&landed, 3.0);
}

/// Task 8 re-review 5 m1: a Trim fader moved while stopped (static edits)
/// and held over Play grabs from where the hand is at the pass's start, so
/// a push of 3 dB after Play is heard as the lane plus 3 dB, not plus the
/// stopped move too.
#[test]
fn a_trim_fader_moved_while_stopped_then_held_over_play_rides_from_the_hand() {
    let _g = serial();
    let (heard, landed) =
        trim_ride_over_play(Some(0.5), Some(0.6), fader_norm(fader_db(0.6) + 3.0));
    assert!(
        heard.iter().all(|d| (d - 3.0).abs() < 0.5),
        "heard as the lane plus 3 dB: {heard:?}"
    );
    assert_shape_kept(&landed, 3.0);
}

/// The same with a touch at no value: whatever the hand's start, the ride
/// is one offset over the lane, heard and written.
#[test]
fn a_trim_fader_held_at_no_value_over_play_rides_as_an_offset() {
    let _g = serial();
    let (heard, landed) = trim_ride_over_play(None, None, 0.5);
    assert!(
        (heard[0] - heard[1]).abs() < 0.5,
        "one offset heard: {heard:?}"
    );
    assert_shape_kept(&landed, (heard[0] + heard[1]) / 2.0);
}

/// Ruling 17, Review Focus 5: targets are spelt one way; anything else
/// is refused (signs, spaces, zero sends, overflow, velocity scale, long
/// or non-ASCII text).
#[test]
fn targets_parse_only_in_their_own_spelling() {
    use vizdaw_ffi::automation::{automation_target_text, parse_automation_target as parse};
    for t in [
        Target::StripFader { strip: 3 },
        Target::StripPan { strip: 0 },
        Target::StripMute { strip: 12 },
        Target::StripSolo { strip: 4 },
        Target::SendLevel { strip: 3, send: 0 },
        Target::SendLevel {
            strip: u64::MAX,
            send: u16::MAX,
        },
        Target::PluginParam {
            slot: 9,
            param: 4_294_967_295,
        },
    ] {
        let text = automation_target_text(t);
        assert_eq!(parse(text.clone()), Some(t), "{text}");
    }
    assert_eq!(
        automation_target_text(Target::SendLevel { strip: 3, send: 0 }),
        "send:3:1"
    );
    assert_eq!(
        automation_target_text(Target::VelocityScale { dest: 1 }),
        ""
    );
    for bad in [
        "",
        "fader",
        "fader:",
        "fader:-1",
        "fader:+1",
        "fader: 3",
        "fader:3 ",
        "Fader:3",
        "fader:3:1",
        "send:3",
        "send:3:0",
        "send:3:65537",
        "param:9",
        "velocity:1",
        "fader:99999999999999999999999",
        "fader:３",
        "pan:1\u{0}",
        &"9".repeat(100),
    ] {
        assert_eq!(parse(bad.to_string()), None, "{bad:?}");
    }
}
