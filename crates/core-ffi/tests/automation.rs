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

/// Task 5 (forwarded): in Read while stopped, a release returns the
/// control to its automation's last value (the static value the gesture
/// set is not what plays over the automation).
///
/// Ignored: it fails. The static path's `SetGain` reloads the engine, and
/// the new graph adopts an override only where the static value did not
/// change (Plan 4d Ruling 13), so the automation's last value (−12 dB) is
/// dropped and the release returns to the new static value (−3 dB) until
/// the next roll. Making it pass changes Plan 4d Ruling 13 in the engine
/// (a controller ruling; Task 8 report).
#[test]
#[ignore = "Plan 4d Ruling 13 drops the held automation value at the static edit; needs a ruling"]
fn a_read_release_while_stopped_returns_to_the_automation() {
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
    wait_until("back to the automation", || {
        fader_override(bus).is_some_and(|d| (d + 12.0).abs() < 1e-6)
    });
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    s.close().unwrap();
}
