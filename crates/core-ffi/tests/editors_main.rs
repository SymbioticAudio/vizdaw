//! Plugin editors opened on the process's main thread, as the app opens
//! them (`harness = false`: `main` runs on the main thread, which
//! `open_editor` insists on). The editor gets the caller's window scale, and
//! a size its plug-in asks for comes back through `idle` in points. On
//! Linux with an X server (`DISPLAY`; CI runs it under Xvfb) the GUI test
//! plug-in's real X11 editor is driven through the run loop the app pumps;
//! without one that part says so and is skipped.

use std::sync::Arc;
use std::time::{Duration, Instant};
use test_plugins::{BASE_SIZE, PARAM_GUI_SCALE};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{EditorSize, PluginFormatTag, PluginRefRow, Session, SlotStatus};

fn main() {
    // `cargo test -- --list` and friends: this binary has no test list.
    if std::env::args().any(|a| a == "--list") {
        return;
    }
    force_offline();
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    the_editor_opens_at_the_callers_scale();
    #[cfg(target_os = "linux")]
    x11::a_real_editor_reports_its_own_resize_through_idle();
    println!("editors_main: ok");
}

/// The demo with test plug-in `id` on the built-in synth, running:
/// (session, slot).
fn with_insert(id: &str) -> (Arc<Session>, u64) {
    let s = Session::new_demo().unwrap();
    let synth = s.mixer().unwrap().strips[0].id;
    let p = PluginRefRow {
        format: PluginFormatTag::Clap,
        id: id.into(),
        name: id.into(),
        vendor: String::new(),
        version: String::new(),
    };
    s.add_insert(synth, None, p).unwrap();
    let slot = s.plugin_slots().unwrap()[0].id;
    let t = Instant::now();
    while s.plugin_slots().unwrap()[0].status != SlotStatus::Live {
        assert!(
            t.elapsed() < Duration::from_secs(60),
            "{id} never went live"
        );
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    (s, slot)
}

/// The timer kind's headless editor (it never uses its parent).
fn the_editor_opens_at_the_callers_scale() {
    let (s, slot) = with_insert("vizdaw.test.timer");
    let size = s.open_editor_scaled(slot, 0x1000, 2.0).unwrap();
    assert_eq!(
        size,
        EditorSize {
            width: BASE_SIZE.0,
            height: BASE_SIZE.1,
            resizable: false,
        },
        "points, whatever the scale"
    );
    let scale = s
        .debug_plugin_param(slot, u64::from(PARAM_GUI_SCALE))
        .unwrap();
    // Cocoa sizes are points already: no `set_scale` there.
    let want = if cfg!(target_os = "macos") { 0.0 } else { 2.0 };
    assert_eq!(scale, want, "the caller's scale reached the plug-in");
    assert!(s.idle().unwrap().editor_resize.is_empty());
    s.close_editor(slot).unwrap();
    // The Swift app's call: scale 1.
    s.open_editor(slot, 0x1000).unwrap();
    let scale = s
        .debug_plugin_param(slot, u64::from(PARAM_GUI_SCALE))
        .unwrap();
    let want = if cfg!(target_os = "macos") { 0.0 } else { 1.0 };
    assert_eq!(scale, want, "open_editor opens at scale 1");
    s.close_editor(slot).unwrap();
    println!("the_editor_opens_at_the_callers_scale: ok");
}

#[cfg(target_os = "linux")]
mod x11 {
    use super::*;
    use test_plugins::GUI_GROW;
    use vizdaw_ffi::EditorResize;
    use vizdaw_ffi::editor::{run_loop_fds, run_loop_pump};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    pub fn a_real_editor_reports_its_own_resize_through_idle() {
        if std::env::var_os("DISPLAY").is_none() {
            println!("a_real_editor_reports_its_own_resize_through_idle: skipped (no DISPLAY)");
            return;
        }
        let (conn, _) = RustConnection::connect(None).expect("DISPLAY is set but X is unreachable");
        let screen = &conn.setup().roots[0];
        let parent = conn.generate_id().unwrap();
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            parent,
            screen.root,
            0,
            0,
            800,
            600,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .unwrap()
        .check()
        .unwrap();
        conn.map_window(parent).unwrap();
        conn.sync().unwrap();

        let (s, slot) = with_insert("vizdaw.test.gui");
        assert!(s.plugin_slots().unwrap()[0].has_editor);
        let size = s.open_editor_scaled(slot, u64::from(parent), 2.0).unwrap();
        assert_eq!((size.width, size.height), BASE_SIZE);
        assert!(!size.resizable);
        let children = conn.query_tree(parent).unwrap().reply().unwrap().children;
        assert_eq!(
            children.len(),
            1,
            "the editor's window is the parent's child"
        );
        assert_eq!(
            run_loop_fds().len(),
            1,
            "its X connection, registered in create"
        );
        let want = EditorResize {
            slot,
            width: BASE_SIZE.0 + GUI_GROW.0,
            height: BASE_SIZE.1 + GUI_GROW.1,
        };
        let t = Instant::now();
        let mut seen = Vec::new();
        while t.elapsed() < Duration::from_secs(5) && !seen.contains(&want) {
            run_loop_pump();
            seen.extend(s.idle().unwrap().editor_resize);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(seen, [want], "reported once, in points");
        s.close_editor(slot).unwrap();
        assert!(
            run_loop_fds().is_empty(),
            "unregistered when the editor closed"
        );
        conn.destroy_window(parent).unwrap();
        conn.sync().unwrap();
        println!("a_real_editor_reports_its_own_resize_through_idle: ok");
    }
}
