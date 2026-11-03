//! The Kontrol S-Series MK3 in the core (plan 5f-4 Tasks 1, 4, 5; spec
//! §8.4). The fake port transport only: no CoreMIDI client is created.

mod common;

use core_surface::kontrol::sysex;
use std::sync::{Mutex, MutexGuard};
use vizdaw_ffi::surfaces::{
    SurfaceKindRow as Kind, debug_add_ports, debug_advance, debug_reset, debug_sent,
    debug_support_file, kontrol_ports, surface_add, surface_list, surface_remove, surfaces_load,
    surfaces_state,
};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    vizdaw_ffi::host::force_offline();
    vizdaw_ffi::host::set_fake_input(None);
    vizdaw_ffi::host::reset_host();
    debug_reset();
    let _ = std::fs::remove_file(debug_support_file());
    g
}

const DAW: &str = "KONTROL S88 MK3 DAW";
const MAIN: &str = "KONTROL S88 MK3 Main";
/// The host's hello and the keyboard's answer: protocol version 4.
const V4: [u8; 3] = [0xBF, 0x01, 0x04];

/// A Kontrol on the DAW port, focused on `s`, answered and fed once.
fn kontrol(s: &vizdaw_ffi::Session) -> u32 {
    let id = surface_add(Kind::KontrolMk3, DAW.into(), DAW.into(), None).unwrap();
    s.focus_surfaces().unwrap();
    s.inject_surface(id, V4.to_vec()).unwrap();
    debug_advance(30);
    s.idle().unwrap();
    id
}

fn row(id: u32) -> vizdaw_ffi::surfaces::SurfaceRow {
    surface_list().into_iter().find(|r| r.id == id).unwrap()
}

fn song(s: &vizdaw_ffi::Session) -> u64 {
    s.snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id
}

#[test]
fn a_kontrol_is_added_greeted_and_identified() {
    let _g = serial();
    let id = surface_add(Kind::KontrolMk3, DAW.into(), DAW.into(), None).unwrap();
    assert_eq!(debug_sent(DAW), vec![V4.to_vec()], "the hello asks for 4");
    assert!(row(id).connected && !row(id).online);
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    s.inject_surface(id, V4.to_vec()).unwrap();
    assert!(row(id).online);
    debug_advance(30);
    s.idle().unwrap();
    let sent = debug_sent(DAW);
    assert_eq!(
        sent.first(),
        Some(&sysex(0x07, 1, 0, b"Generic")),
        "{sent:?}"
    );
    let saved = std::fs::read_to_string(debug_support_file()).unwrap();
    assert!(saved.contains("\"kontrol_mk3\""), "{saved}");
}

#[test]
fn a_kontrol_takes_position_0_only() {
    let _g = serial();
    let e = surface_add(Kind::KontrolMk3, DAW.into(), DAW.into(), Some(1)).unwrap_err();
    assert!(e.to_string().contains("bank of its own"), "{e}");
    let k = surface_add(Kind::KontrolMk3, DAW.into(), DAW.into(), Some(0)).unwrap();
    let s49 = "KONTROL S49 MK3 DAW";
    let k2 = surface_add(Kind::KontrolMk3, s49.into(), s49.into(), None).unwrap();
    surface_add(Kind::Mcu, "M In".into(), "M Out".into(), None).unwrap();
    assert_eq!((row(k).position, row(k2).position), (0, 0));
}

#[test]
fn a_wrong_protocol_version_is_named_in_the_problems() {
    let _g = serial();
    let id = surface_add(Kind::KontrolMk3, DAW.into(), DAW.into(), None).unwrap();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    s.inject_surface(id, vec![0xBF, 0x01, 0x02]).unwrap();
    assert!(!row(id).online);
    let problems = surfaces_state().problems;
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with(&format!("surface {id}: ")) && p.contains("version 2")),
        "{problems:?}"
    );
    let before = row(id).ignored;
    let mut cut = sysex(0x48, 0, 0, b"x");
    cut.pop();
    s.inject_surface(id, cut).unwrap();
    s.inject_surface(id, vec![0x90, 0x3C, 0x7F]).unwrap();
    assert_eq!(row(id).ignored, before + 2, "a cut SysEx and a note");
}

#[test]
fn the_daw_port_is_the_surfaces_and_the_main_port_the_lanes() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let id = kontrol(&s);
    assert!(
        s.inject_midi(DAW.into(), vec![0x90, 0x3C, 0x64]).unwrap(),
        "the surface takes its port's traffic"
    );
    assert!(
        s.inject_midi(DAW.into(), vec![0xBF, 0x50, 0x01]).unwrap(),
        "knobs too"
    );
    // The session holds the engine (pre-flight M3): the keys reach it, and
    // the surface hears none of them. Without the engine this assertion
    // could not fail.
    s.play(song(&s)).unwrap();
    let before = row(id).ignored;
    assert!(
        s.inject_midi(MAIN.into(), vec![0x90, 0x3C, 0x64]).unwrap(),
        "the keys port is a keyboard like any other: the engine takes it"
    );
    assert!(s.inject_midi(MAIN.into(), vec![0x80, 0x3C, 0x00]).unwrap());
    assert_eq!(row(id).ignored, before, "the surface heard nothing of them");
    s.stop().unwrap();
}

#[test]
fn kontrol_ports_lists_only_daw_ports() {
    let _g = serial();
    debug_add_ports(vec![
        DAW.into(),
        MAIN.into(),
        "KONTROL S88 MK3 MIDI 2.0".into(),
        "Native Instruments - KONTROL S61 MK3 - DAW".into(),
        "IAC Bus 1".into(),
    ]);
    let p = kontrol_ports();
    assert_eq!(
        p.sources,
        vec![
            DAW.to_string(),
            "Native Instruments - KONTROL S61 MK3 - DAW".to_string()
        ]
    );
    assert_eq!(p.destinations, p.sources);
    assert!(
        vizdaw_ffi::surfaces::surface_ports()
            .sources
            .contains(&MAIN.to_string())
    );
}

#[test]
fn removing_a_kontrol_and_quitting_say_goodbye() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let id = kontrol(&s);
    let _ = debug_sent(DAW);
    surface_remove(id).unwrap();
    assert_eq!(debug_sent(DAW).last(), Some(&vec![0xBF, 0x02, 0x00]));
    let id = kontrol(&s);
    let _ = debug_sent(DAW);
    vizdaw_ffi::shutdown().unwrap();
    assert_eq!(
        debug_sent(DAW).last(),
        Some(&vec![0xBF, 0x02, 0x00]),
        "quitting lets it go"
    );
    assert!(!row(id).online);
    // Pre-flight M2: the registry has quit; no hello pulls it back.
    debug_advance(2_100);
    let _ = s.idle();
    assert!(
        debug_sent(DAW).is_empty(),
        "nothing is sent after the goodbye"
    );
}

/// Controller ruling K-0, pre-flight C1: an entry of a kind this build does
/// not know is kept verbatim, not driven, and keeps its ID and ports; the
/// rest of the file loads, and nothing is set aside.
#[test]
fn a_surfaces_file_with_an_unknown_kind_keeps_the_rest() {
    let _g = serial();
    let f = debug_support_file();
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    let future = serde_json::json!({
        "id": 3, "kind": "future_box", "input": "X", "output": "X", "position": 0, "extra": true
    });
    let text = serde_json::json!({
        "version": 1,
        "surfaces": [
            {"id": 0, "kind": "mcu", "input": "M In", "output": "M Out", "position": 0},
            future.clone(),
        ],
        "commands": [],
    });
    std::fs::write(&f, text.to_string()).unwrap();
    for pass in 0..2 {
        surfaces_load().unwrap();
        let list: Vec<_> = surface_list().into_iter().map(|r| (r.id, r.kind)).collect();
        assert_eq!(
            list,
            vec![(0, Kind::Mcu)],
            "pass {pass}: the MCU only is driven"
        );
        assert!(
            !f.with_extension("json.bad").exists(),
            "nothing is set aside"
        );
        let problems = surfaces_state().problems;
        assert!(
            problems.iter().any(|p| p.starts_with("surface 3: ")
                && p.contains("future_box")
                && p.contains("newer VizDAW")),
            "{problems:?}"
        );
    }
    let e = surface_add(Kind::Hui, "X".into(), "H Out".into(), None).unwrap_err();
    assert!(
        e.to_string().contains("surface 3"),
        "its ports stay its own: {e}"
    );
    let hui = surface_add(Kind::Hui, "H In".into(), "H Out".into(), None).unwrap();
    assert_eq!(hui, 1, "the lowest free ID; 3 stays the kept entry's");
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    let ids: Vec<u64> = saved["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![0, 1, 3], "in ID order");
    let kept = saved["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == 3)
        .unwrap();
    assert_eq!(kept, &future, "written back as it was");
    surfaces_load().unwrap();
    assert_eq!(surface_list().len(), 2, "read back: the MCU and the HUI");
    assert!(
        surface_remove(3).is_err(),
        "not listed, so not removable here"
    );
}

fn synth(s: &vizdaw_ffi::Session) -> (u64, usize) {
    let m = s.mixer().unwrap();
    let i = m
        .strips
        .iter()
        .position(|x| x.name == "Built-in Synth")
        .unwrap();
    (m.strips[i].id, i)
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

/// Feedback is due: 30 ms on, one idle.
fn feed(s: &vizdaw_ffi::Session) -> Vec<Vec<u8>> {
    debug_advance(30);
    s.idle().unwrap();
    debug_sent(DAW)
}

#[track_caller]
fn has(sent: &[Vec<u8>], m: &[u8]) {
    assert!(sent.iter().any(|x| x == m), "{m:02x?} not in {sent:02x?}");
}

/// Inserts `plugins` (CLAP test ids and names) on the synth, plays until
/// they run, focuses a Kontrol in Plug-in mode on the synth: (session,
/// kontrol, synth, slot ids in chain order).
fn plugin_mode(
    plugins: &[(&str, &str)],
) -> (std::sync::Arc<vizdaw_ffi::Session>, u32, u64, Vec<u64>) {
    use vizdaw_ffi::{PluginFormatTag, PluginRefRow, SlotStatus};
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, _) = synth(&s);
    for (id, name) in plugins {
        s.add_insert(
            synth,
            None,
            PluginRefRow {
                format: PluginFormatTag::Clap,
                id: (*id).into(),
                name: (*name).into(),
                vendor: "VizDAW".into(),
                version: "1.0.0".into(),
            },
        )
        .unwrap();
    }
    s.play(song(&s)).unwrap();
    wait_until("the plugins to run", || {
        s.idle().unwrap();
        s.plugin_slots()
            .unwrap()
            .iter()
            .all(|x| x.status == SlotStatus::Live)
    });
    let slots = s
        .mixer()
        .unwrap()
        .strips
        .into_iter()
        .find(|x| x.id == synth)
        .unwrap()
        .inserts
        .into_iter()
        .map(|x| x.id)
        .collect();
    let id = kontrol(&s);
    s.set_surface_selection(vec![synth]).unwrap();
    s.inject_surface(id, vec![0xBF, 0x05, 0x01]).unwrap();
    (s, id, synth, slots)
}

#[test]
fn a_volume_knob_ride_is_one_set_fader_step() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = kontrol(&s);
    let before = s.mixer().unwrap().strips[k].gain_db;
    s.inject_surface(id, vec![0xBF, 0x50 + k as u8, 0x05])
        .unwrap();
    assert_eq!(
        s.automation_state().unwrap().touched,
        vec![vizdaw_ffi::AutomationTargetRow::StripFader { strip: synth }]
    );
    debug_advance(600);
    s.idle().unwrap();
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "released half a second after the last turn"
    );
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    assert!(s.mixer().unwrap().strips[k].gain_db > before);
}

/// Pre-flight I1 (Ruling 4): a knob held when the surface is removed is
/// released once, before the goodbye, which neither forgets nor answers it.
#[test]
fn removing_a_kontrol_while_a_knob_is_held_releases_it_once() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = kontrol(&s);
    s.inject_surface(id, vec![0xBF, 0x50 + k as u8, 0x05])
        .unwrap();
    assert_eq!(
        s.automation_state().unwrap().touched,
        vec![vizdaw_ffi::AutomationTargetRow::StripFader { strip: synth }]
    );
    let _ = debug_sent(DAW);
    surface_remove(id).unwrap();
    assert_eq!(
        debug_sent(DAW).last(),
        Some(&vec![0xBF, 0x02, 0x00]),
        "it is let go of"
    );
    s.idle().unwrap();
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "released at the removal, not left held"
    );
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    debug_advance(600);
    s.idle().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader"),
        "released once"
    );
}

/// Ruling 4 (owner, 2026-11-03): a keyboard switched out of DAW mode (its
/// goodbye) is asked again 2 s later and, once it answers, gets its name
/// and the full state through the core.
#[test]
fn a_keyboard_switched_out_of_daw_mode_is_pulled_back_with_a_full_refresh() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let id = kontrol(&s);
    let first = s.mixer().unwrap().strips[0].name.clone();
    s.inject_surface(id, vec![0xBF, 0x02, 0x00]).unwrap();
    assert!(!row(id).online);
    let _ = debug_sent(DAW);
    debug_advance(2_000);
    s.idle().unwrap();
    assert!(
        debug_sent(DAW).contains(&V4.to_vec()),
        "VizDAW asks again: it pulls the keyboard back"
    );
    s.inject_surface(id, V4.to_vec()).unwrap();
    assert!(row(id).online);
    let sent = feed(&s);
    assert_eq!(
        sent.first(),
        Some(&sysex(0x07, 1, 0, b"Generic")),
        "{sent:02x?}"
    );
    has(
        &sent,
        &sysex(0x48, 0, 0, &core_surface::kontrol::ascii(&first)),
    );
}

#[test]
fn kontrol_buttons_become_events_for_the_app() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = kontrol(&s);
    let _ = s.take_surface_events().unwrap();
    let bytes = [
        vec![0xBF, 0x10, 0x01, 0xBF, 0x11, 0x01, 0xBF, 0x14, 0x01],
        sysex(0x42, 1, k as u8, &[]),
    ]
    .concat();
    s.inject_surface(id, bytes).unwrap();
    use vizdaw_ffi::surfaces::SurfaceEventRow as E;
    let c = |id: &str| E::Command {
        id: id.into(),
        programmatic: true,
    };
    assert_eq!(
        s.take_surface_events().unwrap(),
        vec![
            c("transport.play"),
            c("transport.return_to_start"),
            c("transport.play"),
            c("transport.stop"),
            E::Select { strip: synth },
        ]
    );
}

#[test]
fn track_navigation_moves_the_kontrols_own_bank_with_the_selection() {
    use vizdaw_ffi::surfaces::SurfaceEventRow as E;
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    for i in 0..12 {
        s.add_bus(format!("Extra {i}")).unwrap();
    }
    let mcu = surface_add(Kind::Mcu, "M In".into(), "M Out".into(), None).unwrap();
    let id = kontrol(&s);
    let strips = s.mixer().unwrap().strips;
    s.set_surface_selection(vec![strips[7].id]).unwrap();
    let _ = feed(&s);
    let _ = s.take_surface_events().unwrap();
    s.inject_surface(id, vec![0xBF, 0x30, 0x01]).unwrap();
    assert_eq!(
        s.take_surface_events().unwrap(),
        vec![E::Select {
            strip: strips[8].id
        }]
    );
    let sent = feed(&s);
    let name = core_surface::kontrol::ascii(&strips[1].name);
    has(&sent, &sysex(0x48, 0, 0, &name));
    s.inject_surface(mcu, vec![0x90, 0x18, 0x7F]).unwrap();
    assert_eq!(
        s.take_surface_events().unwrap(),
        vec![E::Select {
            strip: strips[0].id
        }],
        "the MCU group keeps its bank"
    );
}

#[test]
fn the_kontrol_sees_kinds_colours_tempo_and_solo() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    // The demo's bank shows one strip: a bus gives the solo another slot.
    s.add_bus("Extra".into()).unwrap();
    let (_, k) = synth(&s);
    let slot = k as u8;
    let id = kontrol(&s);
    let sent = debug_sent(DAW);
    has(&sent, &sysex(0x40, 2, slot, &[]));
    has(&sent, &sysex(0x4B, 0, slot, b"#3C8CE6"));
    assert!(
        sent.iter()
            .any(|m| m.get(10) == Some(&0x19) && m.len() == 19),
        "the tempo"
    );
    s.inject_surface(id, sysex(0x44, 1, slot, &[])).unwrap();
    let sent = feed(&s);
    has(&sent, &sysex(0x44, 1, slot, &[]));
    let other = if k == 0 { 1u8 } else { 0 };
    has(&sent, &sysex(0x4A, 1, other, &[]));
}

#[test]
fn kontrol_meters_are_counted_not_listed() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let id = kontrol(&s);
    let st = s.surface_state().unwrap();
    let f = st.feedback.iter().find(|f| f.surface == id).unwrap();
    assert!(f.meters_sent >= 1);
    let ni = "f0 00 21 09 00 00 44 43 01 00";
    let vu = "f0 00 21 09 00 00 44 43 01 00 49";
    assert!(
        f.messages.iter().all(|m| !m.starts_with(vu)),
        "{:?}",
        f.messages
    );
    // The log keeps the last 64; a full refresh holds fewer than 64 CCs.
    assert!(
        f.messages.iter().any(|m| m.starts_with(ni)),
        "SysEx is listed"
    );
}

#[test]
fn plugin_mode_pages_through_the_chain_and_a_knob_moves_the_parameter() {
    let _g = serial();
    let (s, id, _, slots) = plugin_mode(&[
        ("vizdaw.test.gain", "Test Gain"),
        ("vizdaw.test.bank", "Test Bank"),
    ]);
    // Pre-flight M15: the mode switch made the unit due at once; no time
    // passed since the Mixer-mode feedback.
    let sent = debug_sent(DAW);
    has(&sent, &sysex(0x71, 0, 0, b"Test Gain\0Test Bank\0\0"));
    has(&sent, &sysex(0x72, 0, 0, b"Gain"));
    has(&sent, &sysex(0x74, 1, 0, &[]));
    s.inject_surface(id, sysex(0x70, 0, 1, &[])).unwrap();
    let sent = feed(&s);
    has(&sent, &sysex(0x70, 0, 1, &[]));
    has(&sent, &sysex(0x74, 3, 0, &[]));
    has(&sent, &sysex(0x72, 0, 0, b"Bank"));
    s.inject_surface(id, vec![0xBF, 0x31, 0x01]).unwrap();
    let sent = feed(&s);
    has(&sent, &sysex(0x74, 3, 1, &[]));
    has(&sent, &sysex(0x75, 0, 0, b"Page 2"));
    let bank = slots[1];
    let value = |s: &vizdaw_ffi::Session| {
        s.params(bank)
            .unwrap()
            .into_iter()
            .find(|p| p.id == 108)
            .unwrap()
            .value
    };
    let before = value(&s);
    s.inject_surface(id, vec![0xBF, 0x70, 0x10]).unwrap();
    s.stop().unwrap();
    assert!(
        value(&s) > before,
        "knob 1 of page 2 moved the ninth parameter"
    );
}

#[test]
fn the_plugin_page_starts_over_when_the_selection_changes() {
    let _g = serial();
    let (s, id, synth, _) = plugin_mode(&[("vizdaw.test.bank", "Test Bank")]);
    s.inject_surface(id, vec![0xBF, 0x31, 0x01]).unwrap();
    has(&feed(&s), &sysex(0x74, 3, 1, &[]));
    let bus = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| x.id != synth)
        .unwrap()
        .id;
    s.set_surface_selection(vec![bus]).unwrap();
    has(&feed(&s), &sysex(0x71, 0, 0, b"\0"));
    s.set_surface_selection(vec![synth]).unwrap();
    has(&feed(&s), &sysex(0x74, 3, 0, &[]));
    s.stop().unwrap();
}

#[test]
fn chain_and_page_buttons_with_nothing_to_show_are_counted() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let id = kontrol(&s);
    s.inject_surface(id, vec![0xBF, 0x05, 0x01]).unwrap();
    has(&feed(&s), &sysex(0x71, 0, 0, b"\0"));
    let before = row(id).ignored;
    s.inject_surface(id, vec![0xBF, 0x31, 0x01]).unwrap();
    assert_eq!(
        row(id).ignored,
        before + 1,
        "no selected strip: no page to turn"
    );
}

#[test]
fn preset_buttons_step_the_focused_plugins_presets() {
    use vizdaw_ffi::plugins::{debug_current_preset, debug_set_presets};
    let _g = serial();
    let (s, id, _, slots) = plugin_mode(&[("vizdaw.test.gain", "Test Gain")]);
    let slot = slots[0];
    debug_set_presets(
        slot,
        vec!["Init".into(), "Warm".into(), "Bright".into()],
        None,
    );
    s.inject_surface(id, vec![0xBF, 0x36, 0x01]).unwrap();
    assert_eq!(
        debug_current_preset(slot),
        Some(0),
        "from none, Next loads the first"
    );
    has(&feed(&s), &sysex(0x76, 0, 0, b"Init"));
    s.inject_surface(id, [0xBF, 0x36, 0x01].repeat(3)).unwrap();
    assert_eq!(debug_current_preset(slot), Some(2), "no wrap past the last");
    s.inject_surface(id, vec![0xBF, 0x36, 0x7F]).unwrap();
    assert_eq!(debug_current_preset(slot), Some(1));
    let sent = feed(&s);
    has(&sent, &sysex(0x76, 0, 0, b"Warm"));
    has(&sent, &[0xBF, 0x36, 0x03]);
    debug_set_presets(slot, vec!["A".into(), "B".into()], None);
    s.inject_surface(id, vec![0xBF, 0x36, 0x7F]).unwrap();
    assert_eq!(
        debug_current_preset(slot),
        Some(1),
        "from none, Previous loads the last"
    );
    s.stop().unwrap();
}

#[test]
fn a_preset_load_refreshes_the_parameters_and_is_no_undo_step() {
    use vizdaw_ffi::plugins::debug_set_presets;
    let _g = serial();
    let (s, id, _, slots) = plugin_mode(&[("vizdaw.test.gain", "Test Gain")]);
    let _ = s.idle().unwrap();
    let undo = s.snapshot().unwrap().undo_label;
    debug_set_presets(slots[0], vec!["Init".into()], None);
    s.inject_surface(id, vec![0xBF, 0x36, 0x01]).unwrap();
    let news = s.idle().unwrap();
    assert!(news.params_changed.contains(&slots[0]), "{news:?}");
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.undo_label, undo, "not an undo step");
    assert!(
        news.snapshot.is_some(),
        "the title hears Edited (doc_changed)"
    );
    s.stop().unwrap();
}

#[test]
fn a_plugin_without_presets_ignores_the_preset_buttons() {
    let _g = serial();
    let (s, id, _, _) = plugin_mode(&[("vizdaw.test.gain", "Test Gain")]);
    // Pre-flight M15: the mode switch in `plugin_mode` sent the page at once.
    let sent = debug_sent(DAW);
    has(&sent, &[0xBF, 0x36, 0x00]);
    has(&sent, &sysex(0x76, 0, 0, b""));
    let before = row(id).ignored;
    s.inject_surface(id, vec![0xBF, 0x36, 0x01, 0xBF, 0x36, 0x7F])
        .unwrap();
    assert_eq!(row(id).ignored, before, "nothing to step is not a fault");
    s.stop().unwrap();
}
