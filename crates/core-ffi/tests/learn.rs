//! MIDI learn in the core (plan 5f-1 Task 8; spec §4.8, §8.2; Rulings 7,
//! 8, 11, 18).

mod common;

use std::sync::{Mutex, MutexGuard};
use vizdaw_ffi::surfaces::{
    LearnModeRow as Mode, cancel_learn, command_maps, debug_advance, debug_reset, learn_command,
    learn_state, remove_command_map,
};
use vizdaw_ffi::{AutomationTargetRow as Target, Session};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    vizdaw_ffi::host::force_offline();
    vizdaw_ffi::host::reset_host();
    debug_reset();
    g
}

fn synth_fader(s: &Session) -> Target {
    let id = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.name == "Built-in Synth")
        .unwrap()
        .id;
    Target::StripFader { strip: id }
}

#[test]
fn a_learnt_cc_rides_its_target_and_is_one_undo_step() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    let t = synth_fader(&s);
    s.learn_target(t).unwrap();
    assert!(learn_state().waiting.is_some());
    assert!(
        s.inject_midi("Knobs".into(), vec![0xB0, 7, 100]).unwrap(),
        "learn took it"
    );
    debug_advance(301);
    s.idle().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("MIDI Learn")
    );
    let maps = s.learn_maps().unwrap();
    assert_eq!(maps.len(), 1);
    assert_eq!(maps[0].text, "Knobs · CC 7 ch 1");
    assert!(
        maps[0].pickup && maps[0].picks_up,
        "pick-up is on for a new absolute map (Ruling 18)"
    );
    assert!(learn_state().waiting.is_none());
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 0]).unwrap();
    s.idle().unwrap();
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "far from the fader's value: it waits to pick up"
    );
    s.set_learn_pickup(maps[0].source.clone(), false).unwrap();
    assert!(!s.learn_maps().unwrap()[0].pickup);
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 0]).unwrap();
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.contains(&t));
    debug_advance(501);
    s.idle().unwrap();
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "released 500 ms after"
    );
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    s.set_learn_mode(maps[0].source.clone(), Mode::RelativeBinOffset)
        .unwrap();
    assert_eq!(s.learn_maps().unwrap()[0].mode, Mode::RelativeBinOffset);
    assert!(
        !s.learn_maps().unwrap()[0].picks_up,
        "relative maps do not pick up"
    );
    s.unlearn_target(t).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Remove MIDI Learn")
    );
    assert!(s.learn_maps().unwrap().is_empty());
    assert!(
        s.remove_learn(maps[0].source.clone()).is_err(),
        "nothing left to remove"
    );
    assert!(s.unlearn_target(t).is_err());
}

#[test]
fn a_command_map_is_learnt_into_the_app_preferences() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    for pad in [36, 37] {
        learn_command("transport.play_stop".into()).unwrap();
        s.inject_midi("Pads".into(), vec![0x99, pad, 100]).unwrap();
        debug_advance(301);
        s.idle().unwrap();
    }
    assert_eq!(command_maps().len(), 2);
    assert_eq!(command_maps()[0].command, "transport.play_stop");
    s.inject_midi("Pads".into(), vec![0x99, 36, 100]).unwrap();
    s.idle().unwrap();
    use vizdaw_ffi::surfaces::SurfaceEventRow as E;
    assert_eq!(
        s.take_surface_events().unwrap(),
        vec![E::Command {
            id: "transport.play_stop".into(),
            programmatic: true
        }]
    );
    remove_command_map("transport.play_stop".into()).unwrap();
    assert!(command_maps().is_empty(), "every map for the command");
}

/// Pre-flight I10: the map lands in the project that asked, not the
/// focused one.
#[test]
fn a_learn_lands_in_the_session_that_asked() {
    let _g = serial();
    let a = Session::new_demo().unwrap();
    let b = Session::new_demo().unwrap();
    a.focus_surfaces().unwrap();
    a.learn_target(synth_fader(&a)).unwrap();
    b.focus_surfaces().unwrap();
    b.inject_midi("Knobs".into(), vec![0xB0, 7, 100]).unwrap();
    debug_advance(301);
    b.idle().unwrap();
    a.idle().unwrap();
    assert_eq!(
        a.learn_maps().unwrap().len(),
        1,
        "in the project that asked"
    );
    assert!(b.learn_maps().unwrap().is_empty(), "not in the focused one");
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = std::time::Instant::now() + common::wait_limit();
    while !ok() {
        assert!(
            std::time::Instant::now() < until,
            "timed out waiting for {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn song(s: &Session) -> u64 {
    s.snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id
}

/// Knobs CC 7 learned to the synth's fader, pick-up off (the jump).
fn learned_fader(s: &Session) -> Target {
    let t = synth_fader(s);
    s.learn_target(t).unwrap();
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 64]).unwrap();
    debug_advance(301);
    s.idle().unwrap();
    let source = s.learn_maps().unwrap()[0].source.clone();
    s.set_learn_pickup(source, false).unwrap();
    t
}

/// The owner's rule (2026-11-02), first half: the learned control drives
/// its target as a gesture (holder Learn), so the strip's mode writes it.
#[test]
fn a_learned_cc_writes_its_targets_automation_in_touch() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    let t = learned_fader(&s);
    let Target::StripFader { strip } = t else {
        unreachable!()
    };
    let seq = song(&s);
    s.set_automation_mode(
        vec![strip],
        vizdaw_ffi::automation::AutomationModeRow::Touch,
    )
    .unwrap();
    s.play(seq).unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick > 15_360);
    for v in [20u8, 40, 60] {
        std::thread::sleep(std::time::Duration::from_millis(12));
        assert!(
            s.inject_midi("Knobs".into(), vec![0xB0, 7, v]).unwrap(),
            "the map takes it"
        );
        s.idle().unwrap();
    }
    assert!(
        s.automation_state().unwrap().touched.contains(&t),
        "held by Learn"
    );
    debug_advance(501);
    s.idle().unwrap();
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "released 500 ms after its last value"
    );
    // The hook's 500 ms passed at once: let the engine play past the ride,
    // as it would have in those 500 ms, before Stop ends the pass (a stamp
    // after the pass's end is not written).
    let after = s.transport().unwrap().tick + 15_360 / 4;
    wait_until("the ride to be played", || {
        s.transport().unwrap().tick > after
    });
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Write Automation")
    );
    let lane = s
        .lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Built-in Synth Fader")
        .expect("the fader's lane")
        .id;
    let written: Vec<f64> = s
        .lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .filter_map(|e| e.automation.map(|a| a.value))
        .collect();
    assert!(
        written.iter().any(|v| (v - 60.0 / 127.0).abs() < 1e-9),
        "the learned ride is the lane's: {written:?}"
    );
}

/// The owner's rule (2026-11-02), second half: the learned CC itself is
/// never recorded or played thru; an unlearned note from the same port is.
#[test]
fn a_learned_cc_is_neither_recorded_nor_played_thru() {
    use vizdaw_ffi::host::{debug_midi_from_source, drain_offline_log};
    use vizdaw_ffi::{EditCommand, EventKindRow, LaneEdit, LaneInputRow};
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    let seq = song(&s);
    let chords = s
        .lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Chords")
        .unwrap()
        .id;
    let edit = |e: LaneEdit| s.edit(EditCommand::Lane { edit: e }, None).unwrap();
    edit(LaneEdit::SetInput {
        seq,
        lane: chords,
        input: Some(LaneInputRow::Midi {
            port: None,
            channel: None,
        }),
    });
    edit(LaneEdit::SetFlags {
        seq,
        lane: chords,
        mute: None,
        solo: None,
        arm: Some(true),
    });
    let _ = learned_fader(&s);
    let kinds = |s: &Session| {
        let ev = s.lane_events(seq, chords).unwrap().events;
        (
            ev.iter().filter(|e| e.kind == EventKindRow::Cc).count(),
            ev.iter().filter(|e| e.kind == EventKindRow::Note).count(),
        )
    };
    let (ccs, notes) = kinds(&s);
    s.record(
        seq,
        vizdaw_ffi::record::RecordSpecRow {
            replace: false,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: false,
        },
    )
    .unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick > 15_360);
    drain_offline_log();
    for v in [20u8, 40, 60] {
        assert!(
            s.inject_midi("Knobs".into(), vec![0xB0, 7, v]).unwrap(),
            "the map takes it"
        );
        debug_midi_from_source("Knobs", 0, &[0xB0, 7, v + 1]);
        s.idle().unwrap();
    }
    assert!(s.inject_midi("Knobs".into(), vec![0x90, 62, 100]).unwrap());
    let mut log = Vec::new();
    wait_until("the unlearned note's thru", || {
        log.extend(drain_offline_log());
        log.iter()
            .any(|(_, b): &(u64, Vec<u8>)| b == &vec![0x90, 62, 100])
    });
    assert!(s.inject_midi("Knobs".into(), vec![0x80, 62, 0]).unwrap());
    wait_until("a beat on", || s.transport().unwrap().tick > 2 * 15_360);
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
    log.extend(drain_offline_log());
    // The demo's Chords lane has its own CC 7 = 100 at bar 1, which the
    // engine chases when the ride (a static edit on this Read strip)
    // recompiles the schedule: only the values played on the knob count.
    let played = [20u8, 21, 40, 41, 60, 61];
    assert!(
        !log.iter()
            .any(|(_, b)| b.first().is_some_and(|x| x & 0xF0 == 0xB0)
                && b.get(1) == Some(&7)
                && b.get(2).is_some_and(|v| played.contains(v))),
        "no CC 7 thru: {log:?}"
    );
    let (ccs_after, notes_after) = kinds(&s);
    assert_eq!(ccs_after, ccs, "the learned CC is not in the take");
    assert_eq!(notes_after, notes + 1, "the unlearned note is");
}

/// Fold check I5: Undo changes `Settings.learn` without the learn API; the
/// map stops driving its target and taking its messages.
#[test]
fn an_undone_learn_stops_driving_and_taking() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    let t = synth_fader(&s);
    s.learn_target(t).unwrap();
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 100]).unwrap();
    debug_advance(301);
    s.idle().unwrap();
    s.undo().unwrap();
    assert!(s.learn_maps().unwrap().is_empty());
    s.idle().unwrap();
    assert!(
        !s.inject_midi("Knobs".into(), vec![0xB0, 7, 0]).unwrap(),
        "no map takes it (and no engine yet)"
    );
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
}

#[test]
fn hostile_learn_calls_are_refused() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    assert!(
        s.learn_target(Target::StripFader { strip: 987_654 })
            .is_err()
    );
    let dest = s.destinations().unwrap()[0].id;
    assert!(
        s.learn_target(Target::VelocityScale { dest }).is_err(),
        "an existing destination: only the rule refuses it"
    );
    assert!(
        learn_command("no.such_command".into()).is_err(),
        "registry IDs only"
    );
    assert!(learn_command(String::new()).is_err());
    for id in ["automation.gesture", "surface.add"] {
        assert!(
            learn_command(id.into()).is_err(),
            "{id}: only commands a key can run"
        );
    }
    // Task 9 review M2: the surface commands take no required parameter,
    // but a pad that runs one without arguments does nothing.
    for id in ["surface.learn", "surface.unlearn"] {
        let e = learn_command(id.into()).unwrap_err();
        assert!(
            e.to_string().contains("cannot be learned"),
            "{id}: the channel's, not a pad's: {e}"
        );
    }
    assert!(remove_command_map("transport.play_stop".into()).is_err());
    let bad = vizdaw_ffi::surfaces::LearnSourceRow {
        port: "x".into(),
        kind: vizdaw_ffi::surfaces::LearnKindRow::Cc7,
        channel: 99,
        number: 7,
    };
    assert!(s.set_learn_mode(bad.clone(), Mode::Absolute).is_err());
    assert!(s.set_learn_pickup(bad.clone(), true).is_err());
    assert!(s.remove_learn(bad).is_err());
    cancel_learn();
    assert!(learn_state().waiting.is_none());
    let _ = common::wait_limit();
}
