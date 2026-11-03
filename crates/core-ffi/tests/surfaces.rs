//! Surfaces in the core (plan 5f-1 Tasks 7–8; spec §8, §12.1–§12.3).
//! The fake port transport only: no CoreMIDI client is created.

mod common;

use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::surfaces::{
    SurfaceKindRow as Kind, debug_advance, debug_reset, debug_sent, debug_set_missing,
    debug_stale_sources, debug_support_file, surface_add, surface_list, surface_remove,
    surfaces_load,
};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    vizdaw_ffi::host::force_offline();
    vizdaw_ffi::host::reset_host();
    debug_reset();
    let _ = std::fs::remove_file(debug_support_file());
    g
}

#[test]
fn surfaces_are_added_listed_saved_and_removed() {
    let _g = serial();
    let a = surface_add(Kind::Mcu, "MCU In".into(), "MCU Out".into(), None).unwrap();
    let b = surface_add(Kind::McuXt, "XT In".into(), "XT Out".into(), None).unwrap();
    let list = surface_list();
    assert_eq!(list.len(), 2);
    assert_eq!((list[0].id, list[0].position, list[1].position), (a, 0, 1));
    assert!(list.iter().all(|s| s.connected), "fake ports are present");
    assert_eq!(
        debug_sent("MCU Out").first(),
        Some(&vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x00, 0xF7]),
        "the device query on connect"
    );
    let saved = std::fs::read_to_string(debug_support_file()).unwrap();
    assert!(
        saved.contains("\"version\": 1") || saved.contains("\"version\":1"),
        "{saved}"
    );
    debug_reset();
    surfaces_load().unwrap();
    assert_eq!(surface_list().len(), 2, "read back");
    surface_remove(b).unwrap();
    assert_eq!(surface_list().len(), 1);
    assert!(surface_remove(b).is_err());
    let c = surface_add(Kind::Hui, "H In".into(), "H Out".into(), None).unwrap();
    assert_eq!((a, b, c), (0, 1, 1), "IDs are the lowest free ones");
}

#[test]
fn a_fifth_unit_or_a_second_main_is_refused() {
    let _g = serial();
    surface_add(Kind::Mcu, "A".into(), "A".into(), None).unwrap();
    assert!(surface_add(Kind::Mcu, "B".into(), "B".into(), None).is_err());
    surface_add(Kind::McuXt, "X0".into(), "X0".into(), Some(2)).unwrap();
    let e = surface_add(Kind::McuXt, "X1".into(), "X1".into(), Some(2)).unwrap_err();
    assert!(e.to_string().contains("position 2 is taken"), "{e}");
    for n in 1..3 {
        surface_add(Kind::McuXt, format!("X{n}"), format!("X{n}"), None).unwrap();
    }
    let e = surface_add(Kind::McuXt, "X4".into(), "X4".into(), None).unwrap_err();
    assert!(e.to_string().contains("up to three extenders"), "{e}");
    surface_add(Kind::Hui, "H".into(), "H".into(), None).unwrap();
}

#[test]
fn hostile_surface_calls_are_refused() {
    let _g = serial();
    for (i, o) in [
        ("", "x"),
        ("x", ""),
        (&*"p".repeat(256), "x"),
        ("a\nb", "x"),
    ] {
        assert!(
            surface_add(Kind::Hui, i.to_string(), o.to_string(), None).is_err(),
            "{i:?} {o:?}"
        );
    }
    assert!(
        surface_add(Kind::McuXt, "x".into(), "x".into(), Some(9)).is_err(),
        "positions 0–3"
    );
    assert!(surface_remove(u32::MAX).is_err());
    assert!(surface_list().is_empty());
    surface_add(Kind::Hui, "P In".into(), "P Out".into(), None).unwrap();
    let e = surface_add(Kind::Hui, "P In".into(), "Other Out".into(), None).unwrap_err();
    assert!(e.to_string().contains("is used by surface"), "{e}");
    assert!(
        surface_add(Kind::Hui, "Other In".into(), "P Out".into(), None).is_err(),
        "outputs too"
    );
    for n in 1..16 {
        surface_add(Kind::Hui, format!("H{n} In"), format!("H{n} Out"), None).unwrap();
    }
    let e = surface_add(Kind::Hui, "H16 In".into(), "H16 Out".into(), None).unwrap_err();
    assert!(e.to_string().contains("at most 16 surfaces"), "{e}");
    assert!(surface_list().iter().all(|s| s.id < 16));
}

#[test]
fn a_vanished_port_disconnects() {
    let _g = serial();
    let id = surface_add(Kind::Mcu, "U In".into(), "U Out".into(), None).unwrap();
    debug_set_missing(vec!["U In".into()]);
    debug_advance(2_100);
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    s.idle().unwrap();
    let row = surface_list().into_iter().find(|r| r.id == id).unwrap();
    assert!(!row.connected);
    // The touch half is Task 8's `a_surface_touch_is_released_when_its_port_goes`.
}

/// Pre-flight I2: CoreMIDI's source list changes only at a refresh, so
/// the refresh must run while every unit is connected.
#[test]
fn a_vanished_port_is_seen_while_connected() {
    let _g = serial();
    debug_stale_sources(true);
    let id = surface_add(Kind::Mcu, "V In".into(), "V Out".into(), None).unwrap();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    debug_set_missing(vec!["V In".into()]);
    debug_advance(100);
    s.idle().unwrap();
    let connected = || {
        surface_list()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap()
            .connected
    };
    assert!(connected(), "the stale source list still has it");
    debug_advance(2_100);
    s.idle().unwrap();
    assert!(
        !connected(),
        "the 2 s refresh runs while connected and sees it go"
    );
}

#[test]
fn a_damaged_surfaces_file_is_kept_aside() {
    let _g = serial();
    let f = debug_support_file();
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, "{ not json").unwrap();
    surfaces_load().unwrap();
    assert!(surface_list().is_empty());
    assert!(f.with_extension("json.bad").exists());
    std::fs::write(&f, r#"{"version": 99, "surfaces": [], "commands": []}"#).unwrap();
    surfaces_load().unwrap();
    assert!(
        std::fs::read_to_string(f.with_extension("json.bad"))
            .unwrap()
            .contains("99")
    );
    for (bad, why) in [
        (
            r#"{"version": 1, "surfaces": [{"id": 1000, "kind": "hui", "input": "a", "output": "b", "position": 0}], "commands": []}"#,
            "an ID above 15",
        ),
        (
            r#"{"version": 1, "surfaces": [{"id": 0, "kind": "mcu", "input": "a", "output": "b", "position": 0}, {"id": 1, "kind": "mcu", "input": "c", "output": "d", "position": 1}], "commands": []}"#,
            "two main units",
        ),
        (
            r#"{"version": 1, "surfaces": [{"id": 0, "kind": "hui", "input": "a", "output": "b", "position": 0}, {"id": 1, "kind": "hui", "input": "a", "output": "d", "position": 0}], "commands": []}"#,
            "a shared port",
        ),
    ] {
        let _ = std::fs::remove_file(f.with_extension("json.bad"));
        std::fs::write(&f, bad).unwrap();
        surfaces_load().unwrap();
        assert!(surface_list().is_empty(), "{why}");
        assert!(f.with_extension("json.bad").exists(), "{why}");
    }
}

pub fn keep(_: Arc<vizdaw_ffi::Session>) {
    let _ = common::wait_limit();
}
