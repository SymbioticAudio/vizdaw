//! The Kontrol S-Series MK3 in the core (plan 5f-4 Tasks 1, 4, 5; spec
//! §8.4). The fake port transport only: no CoreMIDI client is created.

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
