//! CLAP editor sizes, scale and resize requests, driven through the timer
//! test plug-in's headless editor (it never touches its parent, so no
//! display is needed). `PluginEditor` sizes are logical points everywhere:
//! on X11 the host sets the window's scale before the parent (CLAP gui
//! order) and converts the plug-in's physical pixels; Cocoa sizes are
//! already points, so `set_scale` is never called there.

use core_host::{ParentView, PluginInstance};
use core_model::mixer::ParamId;
use std::ffi::c_void;
use test_plugins::{
    BASE_SIZE, MAX_SIZE, PARAM_GUI_HEIGHT, PARAM_GUI_OPEN, PARAM_GUI_REFUSE_SET_SIZE,
    PARAM_GUI_REQUEST_RESIZE, PARAM_GUI_RESIZABLE, PARAM_GUI_SCALE, PARAM_GUI_SCALED_FIRST,
    PARAM_GUI_SET_SIZES, PARAM_GUI_WIDTH, PARAM_LAST_RESULT,
};

mod common;

const TIMER: &str = "vizdaw.test.timer";

/// The window scale the tests pass in.
const SCALE: f64 = 2.0;

/// Physical pixels per point the plug-in works in: the scale on X11, 1 on
/// Cocoa (logical sizes).
fn api_scale(scale: f64) -> f64 {
    if cfg!(target_os = "macos") {
        1.0
    } else {
        scale
    }
}

fn parent() -> ParentView {
    // SAFETY: the headless test editor never uses its parent.
    unsafe { ParentView::new(0x1000 as *mut c_void) }
}

fn read(p: &mut dyn PluginInstance, id: u32) -> f64 {
    p.param_value(ParamId(u64::from(id))).unwrap()
}

fn cmd(p: &mut dyn PluginInstance, id: u32, value: f64) -> f64 {
    p.set_params(&[(ParamId(u64::from(id)), value)]).unwrap();
    read(p, PARAM_LAST_RESULT)
}

/// The plug-in's own size, in its API's units.
fn plugin_size(p: &mut dyn PluginInstance) -> (u32, u32) {
    (
        read(p, PARAM_GUI_WIDTH) as u32,
        read(p, PARAM_GUI_HEIGHT) as u32,
    )
}

fn request(p: &mut dyn PluginInstance, w: u32, h: u32) -> f64 {
    cmd(p, PARAM_GUI_REQUEST_RESIZE, f64::from(w << 16 | h))
}

/// Loads the plug-in, optionally resizable, and opens its editor at `scale`.
fn opened(resizable: bool, scale: f64) -> (Box<dyn PluginInstance>, (u32, u32)) {
    let mut p = common::load(TIMER);
    cmd(
        p.as_mut(),
        PARAM_GUI_RESIZABLE,
        f64::from(u8::from(resizable)),
    );
    let ed = p.editor().expect("the timer kind has an editor");
    ed.set_scale(scale);
    let size = ed.open(parent()).unwrap();
    (p, size)
}

fn px(points: u32, scale: f64) -> u32 {
    (f64::from(points) * api_scale(scale)).round() as u32
}

#[test]
fn the_editor_opens_at_its_size_in_points_with_the_scale_set_before_the_parent() {
    let (mut p, size) = opened(false, SCALE);
    let p = p.as_mut();
    assert_eq!(size, BASE_SIZE, "points, whatever the scale");
    assert_eq!(read(p, PARAM_GUI_OPEN), 1.0);
    assert_eq!(
        plugin_size(p),
        (px(BASE_SIZE.0, SCALE), px(BASE_SIZE.1, SCALE))
    );
    if cfg!(target_os = "macos") {
        assert_eq!(read(p, PARAM_GUI_SCALE), 0.0, "Cocoa gets no set_scale");
        assert_eq!(read(p, PARAM_GUI_SCALED_FIRST), 0.0);
    } else {
        assert_eq!(read(p, PARAM_GUI_SCALE), SCALE);
        assert_eq!(read(p, PARAM_GUI_SCALED_FIRST), 1.0, "scale before parent");
    }
}

#[test]
fn without_a_scale_the_editor_opens_at_scale_one_and_bad_scales_count_as_one() {
    for scale in [
        None,
        Some(0.0),
        Some(-2.0),
        Some(f64::NAN),
        Some(f64::INFINITY),
    ] {
        let mut p = common::load(TIMER);
        let ed = p.editor().unwrap();
        if let Some(s) = scale {
            ed.set_scale(s);
        }
        assert_eq!(ed.open(parent()).unwrap(), BASE_SIZE, "{scale:?}");
        let want = if cfg!(target_os = "macos") { 0.0 } else { 1.0 };
        assert_eq!(read(p.as_mut(), PARAM_GUI_SCALE), want, "{scale:?}");
        assert_eq!(plugin_size(p.as_mut()), BASE_SIZE, "{scale:?}");
    }
}

#[test]
fn resize_takes_and_returns_points() {
    let (mut p, _) = opened(true, SCALE);
    let ed = p.editor().unwrap();
    assert_eq!(ed.resize(400, 250).unwrap(), (400, 250));
    assert_eq!(plugin_size(p.as_mut()), (px(400, SCALE), px(250, SCALE)));
    // Clamped by the plug-in's `adjust_size`, in its own units.
    let ed = p.editor().unwrap();
    let s = api_scale(SCALE);
    let max = (
        (f64::from(MAX_SIZE.0) / s) as u32,
        (f64::from(MAX_SIZE.1) / s) as u32,
    );
    assert_eq!(ed.resize(5000, 5000).unwrap(), max);
    assert_eq!(plugin_size(p.as_mut()), MAX_SIZE);
}

#[test]
fn a_new_scale_applies_at_the_next_open() {
    let (mut p, _) = opened(true, SCALE);
    let ed = p.editor().unwrap();
    ed.set_scale(1.0);
    // Still the scale it opened with.
    assert_eq!(ed.resize(400, 250).unwrap(), (400, 250));
    assert_eq!(plugin_size(p.as_mut()), (px(400, SCALE), px(250, SCALE)));
    let ed = p.editor().unwrap();
    ed.close();
    ed.open(parent()).unwrap();
    if !cfg!(target_os = "macos") {
        assert_eq!(read(p.as_mut(), PARAM_GUI_SCALE), 1.0);
    }
}

#[test]
fn a_resizable_editors_request_is_clamped_set_and_reported_once_in_points() {
    let (mut p, _) = opened(true, SCALE);
    let p = p.as_mut();
    assert_eq!(p.idle().editor_resize, None);
    // In the plug-in's units: odd physical sizes round up to whole points.
    let (w, h) = (px(450, SCALE) + 1, px(250, SCALE) + 1);
    assert_eq!(request(p, w, h), 1.0, "accepted");
    let want = if cfg!(target_os = "macos") {
        (w, h)
    } else {
        (451, 251)
    };
    assert_eq!(p.idle().editor_resize, Some(want));
    assert_eq!(plugin_size(p), (w, h), "the host set the size");
    assert_eq!(p.idle().editor_resize, None, "reported once");
    // Too big and too small: clamped through `adjust_size`.
    assert_eq!(request(p, 9000, 10), 1.0);
    let s = api_scale(SCALE);
    let want = (
        (f64::from(MAX_SIZE.0) / s).ceil() as u32,
        (100.0 / s).ceil() as u32,
    );
    assert_eq!(p.idle().editor_resize, Some(want));
    assert_eq!(plugin_size(p), (MAX_SIZE.0, 100));
}

fn set_sizes(p: &mut dyn PluginInstance) -> u32 {
    read(p, PARAM_GUI_SET_SIZES) as u32
}

/// CLAP lets a fixed-size editor ask too (a zoom menu, a panel shown):
/// the window follows the size it asked for; the host neither adjusts nor
/// sets it.
#[test]
fn a_fixed_editors_request_is_reported_as_asked_without_set_size() {
    let (mut p, _) = opened(false, SCALE);
    let p = p.as_mut();
    let (w, h) = (px(400, SCALE) + 1, px(300, SCALE));
    assert_eq!(request(p, w, h), 1.0, "accepted");
    let want = if cfg!(target_os = "macos") {
        (w, h)
    } else {
        (401, 300)
    };
    assert_eq!(p.idle().editor_resize, Some(want));
    assert_eq!(set_sizes(p), 0, "no set_size for a fixed editor");
    assert_eq!(plugin_size(p), (w, h));
    // Not clamped either: `adjust_size` is for resizable editors.
    assert_eq!(request(p, 9000, 10), 1.0);
    let s = api_scale(SCALE);
    let want = ((9000.0 / s).ceil() as u32, (10.0 / s).ceil() as u32);
    assert_eq!(p.idle().editor_resize, Some(want));
    assert_eq!(set_sizes(p), 0);
    assert_eq!(p.idle().editor_resize, None, "reported once");
}

/// An accepted request the host cannot apply (the editor refused
/// `set_size`) puts the editor back at the window's size and reports
/// nothing.
#[test]
fn a_request_that_fails_at_idle_restores_the_windows_size() {
    let (mut p, opened_at) = opened(true, SCALE);
    let p = p.as_mut();
    let before = plugin_size(p);
    assert_eq!(before, (px(opened_at.0, SCALE), px(opened_at.1, SCALE)));
    cmd(p, PARAM_GUI_REFUSE_SET_SIZE, 1.0);
    assert_eq!(request(p, 900, 700), 1.0, "accepted");
    assert_eq!(plugin_size(p), (900, 700), "it grew itself");
    assert_eq!(p.idle().editor_resize, None, "not applied");
    assert_eq!(set_sizes(p), 2, "the refused set_size, then the restore");
    assert_eq!(plugin_size(p), before, "back at the window's size");
    // The window's size is the one a later resize or request changed.
    assert_eq!(p.editor().unwrap().resize(500, 400).unwrap(), (500, 400));
    cmd(p, PARAM_GUI_REFUSE_SET_SIZE, 1.0);
    assert_eq!(request(p, 900, 700), 1.0);
    assert_eq!(p.idle().editor_resize, None);
    assert_eq!(plugin_size(p), (px(500, SCALE), px(400, SCALE)));
}

#[test]
fn requests_without_an_open_editor_are_refused_or_dropped() {
    let mut p = common::load(TIMER);
    assert_eq!(request(p.as_mut(), 800, 600), 0.0, "not open yet");
    let ed = p.editor().unwrap();
    ed.open(parent()).unwrap();
    assert_eq!(request(p.as_mut(), 800, 600), 1.0);
    // Closed before the next idle: the request goes with the editor.
    p.editor().unwrap().close();
    assert_eq!(read(p.as_mut(), PARAM_GUI_OPEN), 0.0);
    assert_eq!(p.idle().editor_resize, None);
    assert_eq!(request(p.as_mut(), 800, 600), 0.0, "closed");
    // Closed and reopened before an idle: nothing stale is reported.
    p.editor().unwrap().open(parent()).unwrap();
    assert_eq!(request(p.as_mut(), 700, 500), 1.0);
    p.editor().unwrap().close();
    p.editor().unwrap().open(parent()).unwrap();
    assert_eq!(p.idle().editor_resize, None);
    // And a reopen without a close in between (open replaces the editor).
    assert_eq!(request(p.as_mut(), 700, 500), 1.0);
    p.editor().unwrap().open(parent()).unwrap();
    assert_eq!(p.idle().editor_resize, None);
}

/// The plug-in's own unregister calls while its editor and then the
/// plug-in are destroyed are accepted: the instance leaves the run loop
/// only after both (the host offers timers only off macOS).
#[cfg(not(target_os = "macos"))]
#[test]
fn unregister_calls_during_destroy_are_accepted() {
    use test_plugins::{
        PARAM_DESTROY_UNREGISTER, PARAM_GUI_DESTROY_UNREGISTER, PARAM_TIMER_REGISTER,
    };
    let dir = std::env::temp_dir().join(format!(
        "vizdaw-clap-editor-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join(test_plugins::bundle().file_name().unwrap());
    test_plugins::copy_bundle(&bundle);
    let mut p = common::load_from(&bundle, TIMER);
    let a = cmd(p.as_mut(), PARAM_TIMER_REGISTER, 10.0);
    let b = cmd(p.as_mut(), PARAM_TIMER_REGISTER, 20.0);
    assert!(a >= 0.0 && b >= 0.0, "timers registered");
    cmd(p.as_mut(), PARAM_GUI_DESTROY_UNREGISTER, a);
    cmd(p.as_mut(), PARAM_DESTROY_UNREGISTER, b);
    p.editor().unwrap().open(parent()).unwrap();
    // Active (its processor dropped without `deactivate`): the drop
    // deactivates it before destroying it.
    drop(p.activate(48_000.0, 512).unwrap());
    assert!(p.is_active());
    drop(p);
    let mut log_path = bundle.into_os_string();
    log_path.push(".log");
    let log = std::fs::read_to_string(&log_path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        log,
        format!(
            "gui.destroy unregister {}: 1\ndestroy unregister {}: 1\n",
            a as u32, b as u32
        )
    );
}
