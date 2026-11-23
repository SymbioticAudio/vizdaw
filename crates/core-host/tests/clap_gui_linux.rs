//! A real CLAP X11 editor on Linux: the GUI test plug-in registers a timer
//! and its X connection's descriptor in `gui.create`, draws a child window
//! under the parent the host passes, and asks to grow from its timer. The
//! tests drive the run loop as the app does (poll the descriptors, tick,
//! idle) and check the X window tree.
//!
//! They need an X server: without `DISPLAY` each test says so and passes
//! (skipped). CI runs them under Xvfb.
#![cfg(target_os = "linux")]

use core_host::{FdFlags, InstanceEvents, ParentView, PluginInstance, run_loop_fds, run_loop_tick};
use core_model::mixer::ParamId;
use std::ffi::c_void;
use std::time::{Duration, Instant};
use test_plugins::{
    BASE_SIZE, GUI_COLOR, GUI_GROW, PARAM_GUI_CREATE_FD, PARAM_GUI_CREATE_TIMER, PARAM_GUI_DRAWS,
    PARAM_GUI_FD_CALLS, PARAM_GUI_OPEN, PARAM_GUI_PARENT, PARAM_GUI_RESIZE_ASKED, PARAM_GUI_SCALE,
    PARAM_GUI_TIMER_CALLS, PARAM_GUI_WINDOW,
};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ConnectionExt as _, CreateWindowAux, ImageFormat, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

mod common;

const GUI: &str = "vizdaw.test.gui";

/// The window scale the tests pass in.
const SCALE: f64 = 2.0;

/// How long the run loop gets to do something.
const WAIT: Duration = Duration::from_secs(5);

/// The test's own X connection, or `None` (skipped) without a display.
fn display() -> Option<RustConnection> {
    if std::env::var_os("DISPLAY").is_none() {
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

fn read(p: &mut dyn PluginInstance, id: u32) -> f64 {
    p.param_value(ParamId(u64::from(id))).unwrap()
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
fn pump_until(
    p: &mut dyn PluginInstance,
    mut done: impl FnMut(&mut dyn PluginInstance) -> bool,
) -> bool {
    let t = Instant::now();
    while t.elapsed() < WAIT {
        pump(p);
        if done(p) {
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

/// Loads the GUI kind and opens its editor in `parent` at [`SCALE`].
fn opened(parent: Window) -> Box<dyn PluginInstance> {
    let mut p = common::load(GUI);
    let ed = p.editor().expect("the GUI kind has an X11 editor on Linux");
    ed.set_scale(SCALE);
    assert_eq!(ed.open(as_parent(parent)).unwrap(), BASE_SIZE, "points");
    p
}

#[test]
fn the_editor_registers_in_create_draws_a_child_of_the_parent_and_follows_its_own_resize() {
    let Some(conn) = display() else {
        return;
    };
    let parent = parent_window(&conn);
    let mut p = opened(parent);
    let p = p.as_mut();
    // The run loop's table was there for `gui.create`.
    assert!(
        read(p, PARAM_GUI_CREATE_TIMER) >= 0.0,
        "timer refused in create"
    );
    assert_eq!(
        read(p, PARAM_GUI_CREATE_FD),
        1.0,
        "descriptor refused in create"
    );
    assert_eq!(run_loop_fds().len(), 1, "the X connection's descriptor");
    // Scale before parent: physical pixels.
    assert_eq!(read(p, PARAM_GUI_SCALE), SCALE);
    // Its window is a child of the parent we passed.
    assert_eq!(read(p, PARAM_GUI_PARENT), f64::from(parent));
    let win = read(p, PARAM_GUI_WINDOW) as Window;
    assert_ne!(win, 0);
    assert_eq!(
        conn.query_tree(win).unwrap().reply().unwrap().parent,
        parent
    );
    assert_eq!(children(&conn, parent), [win]);
    assert_eq!(geometry(&conn, win), (px(BASE_SIZE.0), px(BASE_SIZE.1)));
    // Its timer asks to grow; idle reports the new size in points once.
    let want = (BASE_SIZE.0 + GUI_GROW.0, BASE_SIZE.1 + GUI_GROW.1);
    let mut got = None;
    let t = Instant::now();
    while got.is_none() && t.elapsed() < WAIT {
        got = pump(p).editor_resize;
    }
    assert_eq!(got, Some(want), "editor_resize in points");
    assert_eq!(read(p, PARAM_GUI_RESIZE_ASKED), 1.0, "the host accepted");
    let grown = (px(want.0), px(want.1));
    assert!(
        pump_until(p, |_| geometry(&conn, win) == grown),
        "the window took the size it asked for: {:?}",
        geometry(&conn, win)
    );
    assert_eq!(pump(p).editor_resize, None, "reported once");
    // It draws, from its timer and from its descriptor's X events.
    let corner = (grown.0 as i16 - 2, grown.1 as i16 - 2);
    assert!(
        pump_until(p, |_| pixel(&conn, win, corner.0, corner.1)
            == Some(GUI_COLOR)),
        "drawn: {:x?}",
        pixel(&conn, win, corner.0, corner.1)
    );
    assert!(read(p, PARAM_GUI_DRAWS) > 0.0);
    assert!(read(p, PARAM_GUI_TIMER_CALLS) > 0.0);
    assert!(
        read(p, PARAM_GUI_FD_CALLS) > 0.0,
        "X events came through the descriptor"
    );
    p.editor().unwrap().close();
    conn.destroy_window(parent).unwrap();
    conn.sync().unwrap();
}

#[test]
fn closing_the_editor_destroys_its_window_and_its_registrations_and_it_reopens() {
    let Some(conn) = display() else {
        return;
    };
    let parent = parent_window(&conn);
    let mut b = opened(parent);
    let p = b.as_mut();
    assert_eq!(children(&conn, parent).len(), 1);
    let first_timer = read(p, PARAM_GUI_CREATE_TIMER);
    p.editor().unwrap().close();
    assert_eq!(read(p, PARAM_GUI_OPEN), 0.0);
    assert!(
        run_loop_fds().is_empty(),
        "descriptor unregistered in gui.destroy"
    );
    assert!(
        pump_until(p, |_| children(&conn, parent).is_empty()),
        "window destroyed"
    );
    // Open again: a fresh create registers again.
    let ed = p.editor().unwrap();
    ed.set_scale(SCALE);
    ed.open(as_parent(parent)).unwrap();
    assert!(read(p, PARAM_GUI_CREATE_TIMER) > first_timer, "a new timer");
    assert_eq!(read(p, PARAM_GUI_CREATE_FD), 1.0);
    assert_eq!(run_loop_fds().len(), 1);
    assert!(pump_until(p, |_| children(&conn, parent).len() == 1));
    // Dropped with its editor open: its registrations and window go too.
    drop(b);
    assert!(run_loop_fds().is_empty(), "nothing left in the run loop");
    let t = Instant::now();
    while !children(&conn, parent).is_empty() && t.elapsed() < WAIT {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        children(&conn, parent).is_empty(),
        "window gone with the plug-in"
    );
    conn.destroy_window(parent).unwrap();
    conn.sync().unwrap();
}
