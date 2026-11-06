//! The mixer through the FFI (spec §11): the read model, edits as undo
//! steps, drags as one step, routing refusals, solo, levels, and plugin
//! state captured before a plugin goes. The engine host is process-wide,
//! so these tests run one at a time.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    OpenOptions, PluginFormatTag, PluginRefRow, SendRow, Session, SlotStatus, StripColourRow,
    StripKindRow, StripRow, SwatchRow, TrackKindRow, VizError,
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
    // A fresh insert, never captured: undoing its add removes it too, and
    // that step must capture it for the redo (the document it comes back
    // from has no state of its own).
    s.add_insert(synth, None, test_plugin("vizdaw.test.gain", "Test Gain"))
        .unwrap();
    let id2 = strip(&s, "Built-in Synth").inserts[1].id;
    assert_ne!(id2, id);
    idle_until(&s, |s| {
        strip(s, "Built-in Synth").inserts[1].status == SlotStatus::Live
    });
    // Parameters reach the plugin while the engine plays.
    s.debug_set_slot_param(id2, 0, 0.5).unwrap();
    let song = s.snapshot().unwrap().sequences[2].id;
    s.play(song).unwrap();
    for _ in 0..30 {
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    s.stop().unwrap();
    assert_eq!(saved_state(&s, id2), None, "never captured yet");
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(strip(&s, "Built-in Synth").inserts.len(), 1);
    s.redo().unwrap();
    assert!(
        saved_state(&s, id2).is_some(),
        "captured when the undo removed it"
    );
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

/// The synth's EQ, once it runs.
fn eq_live(s: &Session) -> bool {
    strip(s, "Built-in Synth")
        .eq
        .is_some_and(|e| e.status == SlotStatus::Live)
}

/// Plan 7b Ruling 7: the EQ slot loads, stays put while the inserts
/// reorder, is saved and reopened, and comes back with its state on redo.
#[test]
fn the_eq_slot_loads_and_survives_reordering_saving_and_undo() {
    let _g = serial();
    let s = demo();
    let synth = strip(&s, "Built-in Synth").id;
    let snap = s
        .set_eq(synth, Some(test_plugin("vizdaw.test.gain", "Test Gain")))
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Load EQ"));
    let eq = strip(&s, "Built-in Synth").eq.unwrap();
    assert_eq!(eq.plugin.name, "Test Gain");
    assert!(!eq.instrument);
    assert!(
        strip(&s, "Built-in Synth").inserts.is_empty(),
        "not an insert"
    );
    idle_until(&s, eq_live);
    assert!(
        s.plugin_slots().unwrap().iter().any(|r| r.id == eq.id),
        "the plug-in list shows the EQ"
    );
    s.add_insert(synth, None, test_plugin("vizdaw.test.gain", "Test Gain"))
        .unwrap();
    s.add_insert(synth, None, test_plugin("vizdaw.test.bank", "Test Bank"))
        .unwrap();
    let ins = strip(&s, "Built-in Synth").inserts;
    let (a, b) = (ins[0].id, ins[1].id);
    s.move_insert(b, synth, 0).unwrap();
    let row = strip(&s, "Built-in Synth");
    assert_eq!(row.eq.as_ref().unwrap().id, eq.id, "the EQ stays first");
    assert_eq!(row.inserts.iter().map(|i| i.id).collect::<Vec<_>>(), [b, a]);
    // Saving captures the EQ's state.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("eq.vizdaw").to_string_lossy().into_owned();
    s.save_as(path.clone()).unwrap();
    let state = saved_state(&s, eq.id);
    assert!(state.is_some(), "saved with its state");
    // Undo until the EQ goes, then redo it all.
    for _ in 0..4 {
        s.undo().unwrap();
    }
    assert!(strip(&s, "Built-in Synth").eq.is_none());
    for _ in 0..4 {
        s.redo().unwrap();
    }
    let row = strip(&s, "Built-in Synth");
    assert_eq!(row.eq.as_ref().unwrap().id, eq.id);
    assert_eq!(saved_state(&s, eq.id), state, "the same state is back");
    s.close().unwrap();
    drop(s);
    let r = Session::open(path).unwrap();
    let row = strip(&r, "Built-in Synth");
    let back = row.eq.unwrap();
    assert_eq!((back.id, back.plugin.name.as_str()), (eq.id, "Test Gain"));
    assert_eq!(row.inserts.iter().map(|i| i.id).collect::<Vec<_>>(), [b, a]);
    assert_eq!(saved_state(&r, eq.id), state);
}

/// Review carry (B1 M1): replacing the EQ captures the old one first, so
/// Undo brings it back with its state.
#[test]
fn replacing_the_eq_keeps_the_old_state_for_undo() {
    let _g = serial();
    let s = demo();
    let synth = strip(&s, "Built-in Synth").id;
    s.set_eq(synth, Some(test_plugin("vizdaw.test.gain", "Test Gain")))
        .unwrap();
    let a = strip(&s, "Built-in Synth").eq.unwrap().id;
    idle_until(&s, eq_live);
    s.debug_set_slot_param(a, 0, 0.25).unwrap();
    assert_eq!(saved_state(&s, a), None, "never captured yet");
    s.set_eq(synth, Some(test_plugin("vizdaw.test.bank", "Test Bank")))
        .unwrap();
    let b = strip(&s, "Built-in Synth").eq.unwrap();
    assert_ne!(b.id, a);
    assert_eq!(b.plugin.name, "Test Bank");
    s.undo().unwrap();
    let back = strip(&s, "Built-in Synth").eq.unwrap();
    assert_eq!((back.id, back.plugin.name.as_str()), (a, "Test Gain"));
    assert!(saved_state(&s, a).is_some(), "captured before the replace");
}

/// Review carry (B1 M1): a project opened with plug-ins off holds its
/// EQ, as it holds every other slot.
#[test]
fn opening_with_plugins_off_holds_the_eq() {
    let _g = serial();
    let s = demo();
    let synth = strip(&s, "Built-in Synth").id;
    s.set_eq(synth, Some(test_plugin("vizdaw.test.gain", "Test Gain")))
        .unwrap();
    let eq = strip(&s, "Built-in Synth").eq.unwrap().id;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("off.vizdaw").to_string_lossy().into_owned();
    s.save_as(path.clone()).unwrap();
    s.close().unwrap();
    drop(s);
    let r = Session::open_with(
        path,
        OpenOptions {
            plugins_off: true,
            autosave: None,
        },
    )
    .unwrap();
    for _ in 0..20 {
        r.idle().unwrap();
    }
    assert_eq!(r.held_slots().unwrap(), vec![eq]);
    assert_eq!(
        strip(&r, "Built-in Synth").eq.unwrap().status,
        SlotStatus::Placeholder {
            reason: "Plugins off (safe mode)".into()
        }
    );
}

/// Plan 7b Ruling 7: the EQ is not an insert, an unknown strip has none,
/// and an empty slot cannot be emptied; each refusal changes nothing.
#[test]
fn hostile_eq_calls_are_refused() {
    let _g = serial();
    let s = demo();
    let synth = strip(&s, "Built-in Synth").id;
    let v = s.snapshot().unwrap().version;
    assert!(
        s.set_eq(999_999, Some(test_plugin("vizdaw.test.gain", "Test Gain")))
            .is_err()
    );
    assert!(s.set_eq(u64::MAX, None).is_err());
    let e = s.set_eq(synth, None).unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
    assert_eq!(s.snapshot().unwrap().version, v);
    s.set_eq(synth, Some(test_plugin("vizdaw.test.gain", "Test Gain")))
        .unwrap();
    let eq = strip(&s, "Built-in Synth").eq.unwrap().id;
    let v = s.snapshot().unwrap().version;
    assert!(s.remove_insert(eq).is_err());
    assert!(s.move_insert(eq, synth, 0).is_err());
    assert_eq!(s.snapshot().unwrap().version, v);
    assert_eq!(strip(&s, "Built-in Synth").eq.unwrap().id, eq);
    assert!(strip(&s, "Built-in Synth").inserts.is_empty());
}

/// Controller ruling C-1: an unknown strip takes no colour; every swatch
/// reads back as itself, and each choice is one undo step.
#[test]
fn hostile_strip_colour_calls_are_refused() {
    let _g = serial();
    let s = demo();
    let synth = strip(&s, "Built-in Synth").id;
    let v = s.snapshot().unwrap().version;
    assert!(s.set_strip_colour(999_999, Some(SwatchRow::Red)).is_err());
    assert!(s.set_strip_colour(u64::MAX, None).is_err());
    assert_eq!(s.snapshot().unwrap().version, v);
    assert_eq!(strip(&s, "Built-in Synth").colour, None);
    let snap = s.set_strip_colour(synth, Some(SwatchRow::Blue)).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Set Strip Colour"));
    assert_eq!(strip(&s, "Built-in Synth").colour, Some(SwatchRow::Blue));
    s.undo().unwrap();
    assert_eq!(strip(&s, "Built-in Synth").colour, None);
    for c in [
        SwatchRow::Red,
        SwatchRow::Orange,
        SwatchRow::Yellow,
        SwatchRow::Green,
        SwatchRow::Teal,
        SwatchRow::Blue,
        SwatchRow::Purple,
        SwatchRow::Pink,
    ] {
        s.set_strip_colour(synth, Some(c)).unwrap();
        assert_eq!(strip(&s, "Built-in Synth").colour, Some(c));
    }
    s.set_strip_colour(synth, None).unwrap();
    assert_eq!(strip(&s, "Built-in Synth").colour, None);
    // Fader caps go by kind (owner, G1): a colour leaves the kind alone.
    s.set_strip_colour(synth, Some(SwatchRow::Green)).unwrap();
    assert_eq!(
        strip(&s, "Built-in Synth").kind_colour,
        StripColourRow::Instrument
    );
}

/// Plan 7b Ruling 10 (spec §2 item 8): strips are coloured by kind; a
/// bus fed only by sends is an aux return, any other bus a bus.
#[test]
fn strips_are_coloured_by_kind() {
    let _g = serial();
    let s = Session::new_demo_audio().unwrap();
    let m = s.mixer().unwrap();
    for r in &m.strips {
        let want = match r.kind {
            StripKindRow::BuiltinSynth
            | StripKindRow::Instrument { .. }
            | StripKindRow::InstrumentOutput { .. } => StripColourRow::Instrument,
            StripKindRow::AudioPath { .. } => StripColourRow::Channel,
            StripKindRow::Master => StripColourRow::Master,
            StripKindRow::Bus => continue,
        };
        assert_eq!(r.kind_colour, want, "{}", r.name);
    }
    assert_eq!(
        strip(&s, "Built-in Synth").kind_colour,
        StripColourRow::Instrument
    );
    assert_eq!(strip(&s, "Master").kind_colour, StripColourRow::Master);
    let audio = m
        .strips
        .iter()
        .find(|r| matches!(r.kind, StripKindRow::AudioPath { .. }))
        .expect("the audio demo has an audio path")
        .clone();
    assert_eq!(audio.kind_colour, StripColourRow::Channel);
    s.add_bus("FX".into()).unwrap();
    let fx = strip(&s, "FX").id;
    assert_eq!(strip(&s, "FX").kind_colour, StripColourRow::Bus, "no feed");
    let synth = strip(&s, "Built-in Synth").id;
    s.add_send(synth, fx).unwrap();
    assert_eq!(strip(&s, "FX").kind_colour, StripColourRow::AuxReturn);
    s.set_output(audio.id, Some(fx)).unwrap();
    assert_eq!(
        strip(&s, "FX").kind_colour,
        StripColourRow::Bus,
        "fed by an output"
    );
    s.undo().unwrap();
    assert_eq!(strip(&s, "FX").kind_colour, StripColourRow::AuxReturn);
}

/// The destination (and its strip) is named for the plugin it plays,
/// unless the user renamed it.
#[test]
fn replacing_the_instrument_renames_the_destination_and_strip() {
    let _g = serial();
    let s = Session::new_empty().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    s.new_track(seq, TrackKindRow::Instrument, None).unwrap();
    let dests = s.destinations().unwrap();
    let synth = dests.iter().find(|d| d.name == "Built-in Synth").unwrap().clone();
    assert_eq!(s.lanes(seq).unwrap()[0].destination.as_deref(), Some("Built-in Synth"));
    s.set_destination_plugin(synth.id, test_plugin("vizdaw.test.synth4", "Test Synth 4-Out"))
        .unwrap();
    let now = s.destinations().unwrap();
    let d = now.iter().find(|d| d.id == synth.id).unwrap();
    assert_eq!(d.name, "Test Synth 4-Out");
    assert_eq!(
        strip(&s, "Test Synth 4-Out").kind,
        StripKindRow::Instrument { dest: synth.id }
    );
    // Another plugin: still named for the plugin, so it follows.
    s.set_destination_plugin(synth.id, test_plugin("vizdaw.test.other", "Other"))
        .unwrap();
    assert_eq!(strip(&s, "Other").kind, StripKindRow::Instrument { dest: synth.id });
    assert_eq!(s.lanes(seq).unwrap()[0].destination.as_deref(), Some("Other"));
}
