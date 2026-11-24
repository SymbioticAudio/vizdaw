//! Real VST3 X11 editors on Linux (host track H2 Task 1): the test view's
//! X11 variant and the SDK's `again` and `host-checker` (VSTGUI) editors
//! open into an `x11rb` parent window, make a child window there, and run
//! their timers and descriptors through the frame's `Linux::IRunLoop` and
//! the host's run loop, pumped as the app pumps it (poll the descriptors,
//! tick, idle).
//!
//! They need an X server: without `DISPLAY` each test says so and passes
//! (skipped), unless `VIZDAW_REQUIRE_DISPLAY=1`, which makes a missing
//! display a failure (CI runs them under Xvfb). The SDK samples are
//! skipped the same way unless `VIZDAW_REQUIRE_VST3_SAMPLES=1`.
#![cfg(target_os = "linux")]

use core_host::{
    FdFlags, InstanceEvents, ParentView, PluginInstance, run_loop_calls, run_loop_fds,
    run_loop_tick,
};
use std::ffi::c_void;
use std::time::{Duration, Instant};
use test_plugins::{BASE_SIZE, GUI_COLOR, VST3_VIEW_ID, vst3_view_flags};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ConnectionExt as _, CreateWindowAux, ImageFormat, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

mod common;

use common::vst3::{load_vst3, private, sample, sdk};

/// The window scale the test view is opened at.
const SCALE: f64 = 2.0;

/// How long the run loop gets to do something.
const WAIT: Duration = Duration::from_secs(5);

/// The test's own X connection, or `None` (skipped) without a display.
fn display() -> Option<RustConnection> {
    if std::env::var_os("DISPLAY").is_none() {
        let required = std::env::var("VIZDAW_REQUIRE_DISPLAY").is_ok_and(|v| v == "1");
        assert!(!required, "VIZDAW_REQUIRE_DISPLAY=1 but DISPLAY is not set");
        eprintln!("skipped: no DISPLAY (run under an X server, e.g. xvfb-run)");
        return None;
    }
    let (conn, _) = RustConnection::connect(None).expect("DISPLAY is set but X is unreachable");
    Some(conn)
}

/// A mapped top-level window, as the app's plug-in window would be.
fn parent_window(conn: &RustConnection) -> Window {
    let screen = &conn.setup().roots[0];
    let w = conn.generate_id().unwrap();
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        w,
        screen.root,
        0,
        0,
        800,
        600,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new().background_pixel(screen.white_pixel),
    )
    .unwrap()
    .check()
    .unwrap();
    conn.map_window(w).unwrap();
    conn.sync().unwrap();
    w
}

fn as_parent(w: Window) -> ParentView {
    // SAFETY: the window lives until the test destroys it, after the
    // editor is closed.
    unsafe { ParentView::new(w as usize as *mut c_void) }
}

/// One pass of the app's main loop: waits up to 10 ms for a registered
/// descriptor, runs the plug-ins' callbacks, then the instance's idle.
fn pump(p: &mut dyn PluginInstance) -> InstanceEvents {
    let fds = run_loop_fds();
    let mut polled: Vec<libc::pollfd> = fds
        .iter()
        .map(|(fd, flags)| libc::pollfd {
            fd: *fd,
            events: if flags.contains(FdFlags::READ) {
                libc::POLLIN
            } else {
                0
            } | if flags.contains(FdFlags::WRITE) {
                libc::POLLOUT
            } else {
                0
            },
            revents: 0,
        })
        .collect();
    // SAFETY: `polled` is a live array of `polled.len()` pollfd records.
    unsafe { libc::poll(polled.as_mut_ptr(), polled.len() as libc::nfds_t, 10) };
    let ready: Vec<(i32, FdFlags)> = polled
        .iter()
        .filter_map(|q| {
            let mut f = FdFlags::empty();
            if q.revents & libc::POLLIN != 0 {
                f |= FdFlags::READ;
            }
            if q.revents & libc::POLLOUT != 0 {
                f |= FdFlags::WRITE;
            }
            if q.revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                f |= FdFlags::ERROR;
            }
            (!f.is_empty()).then_some((q.fd, f))
        })
        .collect();
    run_loop_tick(Instant::now(), &ready);
    p.idle()
}

/// Pumps until `done` holds (true) or [`WAIT`] passes (false).
fn pump_until(p: &mut dyn PluginInstance, mut done: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < WAIT {
        pump(p);
        if done() {
            return true;
        }
    }
    false
}

fn children(conn: &RustConnection, w: Window) -> Vec<Window> {
    conn.query_tree(w).unwrap().reply().unwrap().children
}

fn geometry(conn: &RustConnection, w: Window) -> (u32, u32) {
    let g = conn.get_geometry(w).unwrap().reply().unwrap();
    (u32::from(g.width), u32::from(g.height))
}

/// The pixel at (`x`, `y`) of `w`, as `0xRRGGBB` (a 24- or 32-bit
/// TrueColor server, as Xvfb and XWayland are).
fn pixel(conn: &RustConnection, w: Window, x: i16, y: i16) -> Option<u32> {
    let r = conn
        .get_image(ImageFormat::Z_PIXMAP, w, x, y, 1, 1, !0)
        .ok()?
        .reply()
        .ok()?;
    let b: [u8; 4] = r.data.get(..4)?.try_into().ok()?;
    Some(u32::from_le_bytes(b) & 0xFF_FF_FF)
}

fn px(points: u32) -> u32 {
    (f64::from(points) * SCALE).round() as u32
}

#[test]
fn the_test_views_x11_child_is_drawn_through_the_run_loop_and_goes_at_close() {
    let Some(conn) = display() else {
        return;
    };
    let parent = parent_window(&conn);
    let pv = private();
    pv.probe.set_view(vst3_view_flags::X11, (0, 0));
    let mut p = load_vst3(&pv.path, VST3_VIEW_ID);
    let ed = p.editor().expect("Test View has an editor");
    ed.set_scale(SCALE);
    assert_eq!(ed.open(as_parent(parent)).unwrap(), BASE_SIZE, "points");
    let v = pv.probe.view();
    assert_eq!(v.timer_registered, 0, "timer through the frame's IRunLoop");
    assert_eq!(
        v.fd_registered, 0,
        "X descriptor through the frame's IRunLoop"
    );
    assert_eq!(run_loop_fds().len(), 1);
    let win = v.x_window as Window;
    assert_ne!(win, 0);
    assert_eq!(
        conn.query_tree(win).unwrap().reply().unwrap().parent,
        parent
    );
    assert_eq!(children(&conn, parent), [win]);
    assert_eq!(geometry(&conn, win), (px(BASE_SIZE.0), px(BASE_SIZE.1)));
    let corner = (px(BASE_SIZE.0) as i16 - 2, px(BASE_SIZE.1) as i16 - 2);
    assert!(
        pump_until(p.as_mut(), || pixel(&conn, win, corner.0, corner.1)
            == Some(GUI_COLOR)),
        "drawn: {:x?}",
        pixel(&conn, win, corner.0, corner.1)
    );
    // The draw can come through the X descriptor before the timer's
    // first tick: pump on until the timer has run too.
    assert!(
        pump_until(p.as_mut(), || pv.probe.view().timer_calls > 0),
        "{:?}",
        pv.probe.view()
    );
    let v = pv.probe.view();
    assert!(v.draws > 0 && v.timer_calls > 0, "{v:?}");
    // Closed: its window and registrations go.
    p.editor().unwrap().close();
    assert!(run_loop_fds().is_empty());
    conn.sync().unwrap();
    assert!(
        pump_until(p.as_mut(), || children(&conn, parent).is_empty()),
        "window destroyed"
    );
    conn.destroy_window(parent).unwrap();
    conn.sync().unwrap();
}

/// Opens SDK sample class `id` of bundle `name` into a parent and checks a
/// child window appears and the editor's timers fire through the pump.
fn sdk_editor_opens_and_ticks(name: &str, id: &str) {
    let Some(conn) = display() else {
        return;
    };
    let Some(path) = sample(name) else {
        return;
    };
    let parent = parent_window(&conn);
    let mut p = load_vst3(&path, id);
    let ed = p.editor().unwrap_or_else(|| panic!("{name} has an editor"));
    ed.set_scale(1.0);
    let (w, h) = ed.open(as_parent(parent)).unwrap();
    assert!(w > 0 && h > 0, "{name}: {w}×{h}");
    assert!(
        pump_until(p.as_mut(), || !children(&conn, parent).is_empty()),
        "{name}: a child window under the parent"
    );
    assert!(!run_loop_fds().is_empty(), "{name}: its X descriptor");
    let before = run_loop_calls();
    assert!(
        pump_until(p.as_mut(), || run_loop_calls().timers > before.timers),
        "{name}: its timers fire through the pump"
    );
    p.editor().unwrap().close();
    assert!(run_loop_fds().is_empty(), "{name}: nothing left registered");
    conn.sync().unwrap();
    assert!(
        pump_until(p.as_mut(), || children(&conn, parent).is_empty()),
        "{name}: its window goes at close"
    );
    drop(p);
    conn.destroy_window(parent).unwrap();
    conn.sync().unwrap();
}

#[test]
fn the_sdk_again_editor_opens_into_an_x11_parent_and_ticks() {
    sdk_editor_opens_and_ticks("again.vst3", sdk::AGAIN);
}

#[test]
fn the_sdk_host_checker_editor_opens_into_an_x11_parent_and_ticks() {
    sdk_editor_opens_and_ticks("host-checker.vst3", sdk::HOST_CHECKER);
}
