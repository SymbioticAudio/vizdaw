//! Plugin windows through the FFI (spec §9): editors refuse what they
//! cannot show (no GUI, no parent view, not the main thread), and the open
//! windows are saved with the project and offered again on open. Opening a
//! real editor needs AppKit on the main thread: platform-macos's
//! `au_editor` test and the manual checklist cover that.

mod common;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{PluginFormatTag, PluginRefRow, Session, SlotStatus, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    g
}

fn plugin(format: PluginFormatTag, id: &str) -> PluginRefRow {
    PluginRefRow {
        format,
        id: id.into(),
        name: id.into(),
        vendor: String::new(),
        version: String::new(),
    }
}

/// The demo with `p` on the built-in synth, running: (session, slot).
fn with_insert(p: PluginRefRow) -> (Arc<Session>, u64) {
    common::assert_exe_dir_small();
    let s = Session::new_demo().unwrap();
    let synth = s.mixer().unwrap().strips[0].id;
    s.add_insert(synth, None, p).unwrap();
    let id = s.plugin_slots().unwrap()[0].id;
    let t = Instant::now();
    while s.plugin_slots().unwrap()[0].status != SlotStatus::Live {
        assert!(t.elapsed() < common::au_wait_limit());
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    (s, id)
}

fn unavailable(r: Result<vizdaw_ffi::EditorSize, VizError>) -> String {
    match r {
        Err(VizError::Engine { message }) if message.starts_with("editor unavailable") => message,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_plugin_without_a_gui_has_no_editor() {
    let _g = serial();
    let (s, id) = with_insert(plugin(PluginFormatTag::Clap, "vizdaw.test.gain"));
    assert!(!s.plugin_slots().unwrap()[0].has_editor);
    unavailable(s.open_editor(id, 0x1000));
    unavailable(s.open_editor(id, 0));
    unavailable(s.open_editor(999, 0x1000));
    unavailable(s.resize_editor(id, 100, 100));
    // Closing what is not open is fine.
    s.close_editor(id).unwrap();
    s.close_editor(999).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn an_au_editor_opens_only_on_the_main_thread() {
    let _g = serial();
    let (s, id) = with_insert(plugin(PluginFormatTag::Au, "aufx/dely/appl"));
    assert!(s.plugin_slots().unwrap()[0].has_editor);
    // Test threads are not the main thread: refused before AppKit is used.
    let m = unavailable(s.open_editor(id, 0x1000));
    assert!(m.contains("main thread"), "{m}");
}

#[test]
fn open_windows_are_saved_with_the_project() {
    let _g = serial();
    let (s, id) = with_insert(plugin(PluginFormatTag::Clap, "vizdaw.test.gain"));
    s.set_window_open(id, true).unwrap();
    s.set_window_open(4242, true).unwrap();
    assert_eq!(s.open_windows().unwrap(), vec![id], "only existing slots");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("w.vizdaw");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    assert!(path.join("ui.json").exists());
    // One session per package (Task 9 review I-2): the first lets go.
    s.close().unwrap();
    let again = Session::open(path.to_string_lossy().into_owned()).unwrap();
    assert_eq!(again.open_windows().unwrap(), vec![id]);
    again.set_window_open(id, false).unwrap();
    again.save().unwrap();
    again.close().unwrap();
    let third = Session::open(path.to_string_lossy().into_owned()).unwrap();
    assert!(third.open_windows().unwrap().is_empty());
    third.close().unwrap();
    // A project without the file (older, or written elsewhere) opens none.
    std::fs::remove_file(path.join("ui.json")).unwrap();
    let fourth = Session::open(path.to_string_lossy().into_owned()).unwrap();
    assert!(fourth.open_windows().unwrap().is_empty());
}
