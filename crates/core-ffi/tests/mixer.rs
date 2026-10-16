//! The mixer through the FFI (spec §11): the read model, edits as undo
//! steps, drags as one step, routing refusals, solo, levels, and plugin
//! state captured before a plugin goes. The engine host is process-wide,
//! so these tests run one at a time.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    PluginFormatTag, PluginRefRow, SendRow, Session, SlotStatus, StripKindRow, StripRow, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    g
}

fn test_plugin(id: &str, name: &str) -> PluginRefRow {
    PluginRefRow {
        format: PluginFormatTag::Clap,
        id: id.into(),
        name: name.into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

fn strip(s: &Session, name: &str) -> StripRow {
    s.mixer()
        .unwrap()
        .strips
        .into_iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no strip {name}"))
}

fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let start = Instant::now();
    while !done(s) {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn demo() -> Arc<Session> {
    Session::new_demo().unwrap()
}

/// The state blob slot `id` has in the document (None: never captured).
fn saved_state(s: &Session, id: u64) -> Option<u64> {
    s.debug_slot_state(id).unwrap()
}

#[test]
fn the_demo_mixer_lists_its_strips_master_last() {
    let _g = serial();
    let s = demo();
    let m = s.mixer().unwrap();
    let names: Vec<&str> = m.strips.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Built-in Synth", "Master"]);
    assert_eq!(m.strips[0].kind, StripKindRow::BuiltinSynth);
    assert_eq!(m.strips[0].kind_label, "Built-in Synth");
    assert_eq!(m.strips[1].kind, StripKindRow::Master);
    assert_eq!(m.strips[0].output, None);
    assert!(m.buses.is_empty());
    assert!(
        m.strips
            .iter()
            .all(|r| !r.solo_muted && r.inserts.is_empty())
    );
}

#[test]
fn mixer_edits_are_undo_steps() {
    let _g = serial();
    let s = demo();
    let snap = s.add_bus("Reverb".into()).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Add Bus"));
    let m = s.mixer().unwrap();
    assert_eq!(m.version, snap.version);
    let bus = m.buses[0].id;
    assert_eq!(m.buses[0].name, "Reverb");
    let synth = strip(&s, "Built-in Synth").id;
    s.add_send(synth, bus).unwrap();
    s.edit_send(
        synth,
        0,
        SendRow {
            target: bus,
            level_db: -6.0,
            pre_fader: true,
            enabled: true,
        },
    )
    .unwrap();
    s.set_output(synth, Some(bus)).unwrap();
    let row = strip(&s, "Built-in Synth");
    assert_eq!(row.output, Some(bus));
    assert_eq!(row.sends.len(), 1);
    assert_eq!(
        (row.sends[0].level_db, row.sends[0].pre_fader),
        (-6.0, true)
    );
    s.set_pan(synth, -0.5).unwrap();
    s.rename_strip(bus, "Hall".into()).unwrap();
    assert_eq!(strip(&s, "Hall").kind_label, "Bus");
    for _ in 0..5 {
        s.undo().unwrap();
    }
    let row = strip(&s, "Built-in Synth");
    assert!(row.sends.is_empty() && row.output.is_none() && row.pan == 0.0);
    // Removing a bus retargets what used it, in one step.
    s.redo().unwrap();
    s.redo().unwrap();
    s.redo().unwrap();
    s.remove_bus(bus).unwrap();
    let row = strip(&s, "Built-in Synth");
    assert!(row.output.is_none() && row.sends.is_empty());
}

#[test]
fn a_routing_cycle_is_refused_and_changes_nothing() {
    let _g = serial();
    let s = demo();
    s.add_bus("A".into()).unwrap();
    s.add_bus("B".into()).unwrap();
    let (a, b) = (strip(&s, "A").id, strip(&s, "B").id);
    s.set_output(a, Some(b)).unwrap();
    let before = s.mixer().unwrap();
    let e = s.set_output(b, Some(a)).unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
    assert!(s.add_send(b, a).is_err());
    assert_eq!(s.mixer().unwrap(), before);
}

#[test]
fn a_drag_is_one_undo_step() {
    let _g = serial();
    let s = demo();
    let synth = strip(&s, "Built-in Synth").id;
    s.begin_edit("Fader".into()).unwrap();
    for db in [-1.0, -3.0, -9.5] {
        s.set_gain(synth, db).unwrap();
        assert_eq!(strip(&s, "Built-in Synth").gain_db, db, "heard at once");
    }
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Fader"));
    s.undo().unwrap();
    assert_eq!(strip(&s, "Built-in Synth").gain_db, 0.0);
    assert!(s.snapshot().unwrap().undo_label.is_none());
    // An end without a begin changes nothing.
    s.end_edit().unwrap();
    assert!(s.set_gain(synth, 13.0).is_err(), "above +12 dB");
}

#[test]
fn solo_mutes_the_strips_it_does_not_hear() {
    let _g = serial();
    let s = demo();
    s.add_bus("A".into()).unwrap();
    s.add_bus("B".into()).unwrap();
    s.set_strip_solo(strip(&s, "A").id, true).unwrap();
    assert!(!strip(&s, "A").solo_muted);
    assert!(strip(&s, "B").solo_muted);
    assert!(strip(&s, "Built-in Synth").solo_muted);
    assert!(!strip(&s, "Master").solo_muted, "downstream of A");
    s.set_strip_mute(strip(&s, "B").id, true).unwrap();
    assert!(strip(&s, "B").mute);
}

#[test]
fn levels_are_for_the_session_that_plays() {
    let _g = serial();
    let a = demo();
    let b = demo();
    let song = a.snapshot().unwrap().sequences[2].id;
    assert!(a.levels().unwrap().meters.is_empty());
    a.play(song).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    let l = a.levels().unwrap();
    let strips: Vec<u64> = l.meters.iter().map(|m| m.strip).collect();
    assert!(strips.contains(&strip(&a, "Built-in Synth").id), "{l:?}");
    assert!(strips.contains(&strip(&a, "Master").id), "{l:?}");
    assert!(l.dsp_peak >= 0.0 && l.faulted.is_empty());
    assert!(b.levels().unwrap().meters.is_empty(), "b does not play");
}

#[test]
fn a_removed_plugin_keeps_its_state_for_undo_and_redo() {
    let _g = serial();
    let s = demo();
    let synth = strip(&s, "Built-in Synth").id;
    s.add_insert(synth, None, test_plugin("vizdaw.test.gain", "Test Gain"))
        .unwrap();
    let id = strip(&s, "Built-in Synth").inserts[0].id;
    idle_until(&s, |s| {
        strip(s, "Built-in Synth").inserts[0].status == SlotStatus::Live
    });
    assert_eq!(saved_state(&s, id), None, "never captured yet");
    s.remove_insert(id).unwrap();
    assert!(strip(&s, "Built-in Synth").inserts.is_empty());
    s.undo().unwrap();
    let back = strip(&s, "Built-in Synth").inserts[0].clone();
    assert_eq!(back.id, id);
    assert!(saved_state(&s, id).is_some(), "captured before the removal");
    idle_until(&s, |s| {
        strip(s, "Built-in Synth").inserts[0].status == SlotStatus::Live
    });
    // Undoing the add removes it too: captured for the redo.
    s.undo().unwrap();
    assert!(strip(&s, "Built-in Synth").inserts.is_empty());
    s.redo().unwrap();
    assert!(saved_state(&s, id).is_some());
}

#[test]
fn plugin_instruments_bring_strips_and_free_outputs() {
    let _g = serial();
    let s = demo();
    s.add_plugin_instrument(
        "Keys".into(),
        test_plugin("vizdaw.test.synth4", "Test Synth 4-Out"),
    )
    .unwrap();
    let keys = strip(&s, "Keys");
    let StripKindRow::Instrument { dest } = keys.kind else {
        panic!("{:?}", keys.kind)
    };
    assert_eq!(keys.kind_label, "Keys");
    assert!(keys.instrument.as_ref().unwrap().instrument);
    idle_until(&s, |s| {
        strip(s, "Keys").instrument.unwrap().status == SlotStatus::Live
    });
    let keys = strip(&s, "Keys");
    assert_eq!(keys.instrument.as_ref().unwrap().outputs, 4);
    assert_eq!(keys.free_outputs, vec![1, 2, 3]);
    s.add_instrument_output(dest, 1, "Keys 2".into()).unwrap();
    let out = strip(&s, "Keys 2");
    assert_eq!(out.kind, StripKindRow::InstrumentOutput { dest, index: 1 });
    assert_eq!(out.kind_label, "Keys out 2");
    assert_eq!(strip(&s, "Keys").free_outputs, vec![2, 3]);
}

#[test]
fn inserts_reorder_bypass_and_take_a_sidechain() {
    let _g = serial();
    let s = demo();
    s.add_bus("Kick".into()).unwrap();
    let synth = strip(&s, "Built-in Synth").id;
    s.add_insert(synth, None, test_plugin("vizdaw.test.gain", "Test Gain"))
        .unwrap();
    s.add_insert(
        synth,
        None,
        test_plugin("vizdaw.test.ducker", "Test Ducker"),
    )
    .unwrap();
    idle_until(&s, |s| {
        strip(s, "Built-in Synth")
            .inserts
            .iter()
            .all(|i| i.status == SlotStatus::Live)
    });
    let ins = strip(&s, "Built-in Synth").inserts;
    let (gain, ducker) = (ins[0].clone(), ins[1].clone());
    assert!(!gain.has_sidechain_input);
    assert!(
        ducker.has_sidechain_input,
        "the ducker has a sidechain port"
    );
    s.set_sidechain(ducker.id, Some(strip(&s, "Kick").id))
        .unwrap();
    s.set_bypass(gain.id, true).unwrap();
    s.move_insert(ducker.id, synth, 0).unwrap();
    let ins = strip(&s, "Built-in Synth").inserts;
    assert_eq!(
        ins.iter().map(|i| i.id).collect::<Vec<_>>(),
        [ducker.id, gain.id]
    );
    assert_eq!(ins[0].sidechain, Some(strip(&s, "Kick").id));
    assert!(ins[1].bypass);
    // Moving keeps the running instance.
    assert_eq!(ins[0].status, SlotStatus::Live);
}

#[test]
fn a_destination_can_play_a_plugin_instrument() {
    let _g = serial();
    let s = demo();
    let dests = s.destinations().unwrap();
    let piano = dests
        .iter()
        .find(|d| d.name == "Built-in Piano")
        .unwrap()
        .clone();
    assert!(piano.plugin.is_none(), "a MIDI destination");
    let snap = s
        .set_destination_plugin(
            piano.id,
            test_plugin("vizdaw.test.synth4", "Test Synth 4-Out"),
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Edit Destination"));
    let now = s.destinations().unwrap();
    let piano = now.iter().find(|d| d.id == piano.id).unwrap();
    assert_eq!(piano.plugin.as_ref().unwrap().id, "vizdaw.test.synth4");
    // Its instrument strip appears and the plugin loads.
    assert_eq!(
        strip(&s, "Built-in Piano").kind,
        StripKindRow::Instrument { dest: piano.id }
    );
    idle_until(&s, |s| {
        strip(s, "Built-in Piano").instrument.unwrap().status == SlotStatus::Live
    });
    assert!(
        s.set_destination_plugin(999, test_plugin("x", "x"))
            .is_err()
    );
}
