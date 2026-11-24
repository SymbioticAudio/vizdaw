//! VST3 editors (host track H2 Task 1), driven through the test VST3's
//! headless view (Test View; it never touches its parent, so no display is
//! needed). `PluginEditor` sizes are logical points everywhere: on Linux
//! the host sends the window's scale before `attached` and converts the
//! view's physical pixels; macOS views work in points and get no scale.
//! The view's counters (`Vst3Probe::view`) say what the host did, in
//! order.

use core_host::{ParentView, PluginInstance};
use std::ffi::c_void;
use test_plugins::{
    BASE_SIZE, MAX_SIZE, VST3_GAIN_ID, VST3_SYNTH_ID, VST3_VIEW_ID, vst3_platform, vst3_view_flags,
};

mod common;

use common::vst3::{Private, load_vst3, private, sample, sdk};

/// The window scale the tests pass in.
const SCALE: f64 = 2.0;

/// `kResultTrue`.
const TRUE: i64 = 0;

/// Physical pixels per point the view works in: the scale on Linux, 1 on
/// macOS (points).
fn api_scale(scale: f64) -> f64 {
    if cfg!(target_os = "macos") {
        1.0
    } else {
        scale
    }
}

fn px(points: (u32, u32), scale: f64) -> (u32, u32) {
    let f = |v: u32| (f64::from(v) * api_scale(scale)).round() as u32;
    (f(points.0), f(points.1))
}

const PARENT: usize = 0x1000;

fn parent() -> ParentView {
    // SAFETY: the headless test view never uses its parent.
    unsafe { ParentView::new(PARENT as *mut c_void) }
}

/// Test View from a private copy, views made with `flags` (and `ask` for
/// `RESIZE_IN_ATTACHED`).
fn view_plugin(flags: u32, ask: (u32, u32)) -> (Private, Box<dyn PluginInstance>) {
    let pv = private();
    pv.probe.set_view(flags, ask);
    let p = load_vst3(&pv.path, VST3_VIEW_ID);
    (pv, p)
}

/// …with its editor opened at `scale`; returns the size it opened at.
fn opened(
    flags: u32,
    ask: (u32, u32),
    scale: f64,
) -> (Private, Box<dyn PluginInstance>, (u32, u32)) {
    let (pv, mut p) = view_plugin(flags, ask);
    let ed = p.editor().expect("Test View has an editor");
    ed.set_scale(scale);
    let size = ed.open(parent()).unwrap();
    (pv, p, size)
}

#[test]
fn only_test_view_has_an_editor_and_asking_keeps_no_view_alive() {
    let (pv, mut p) = view_plugin(0, (0, 0));
    for _ in 0..3 {
        assert!(p.editor().is_some());
    }
    let v = pv.probe.view();
    assert!(v.created <= 1, "the answer is cached: {} views", v.created);
    assert_eq!(v.releases, v.created, "no view is kept between opens");
    assert_eq!(v.attached, 0);
    for id in [VST3_GAIN_ID, VST3_SYNTH_ID] {
        let mut q = load_vst3(&pv.path, id);
        assert!(q.editor().is_none(), "{id}: no view");
    }
}

#[test]
fn a_view_that_supports_no_platform_type_is_no_editor() {
    let (pv, mut p) = view_plugin(vst3_view_flags::NO_PLATFORM, (0, 0));
    assert!(p.editor().is_none());
    let v = pv.probe.view();
    assert!(v.platform_asks >= 1);
    assert_eq!(v.releases, v.created);
}

#[test]
fn the_editor_opens_at_its_size_in_points_with_the_scale_before_attached_on_linux_only() {
    let (pv, mut p, size) = opened(0, (0, 0), SCALE);
    assert_eq!(size, BASE_SIZE, "points");
    let v = pv.probe.view();
    assert_eq!(v.attached, 1);
    assert_eq!(v.frame_sets, 1, "a frame before attached");
    assert_eq!(v.parent, PARENT as u64);
    assert_eq!(v.size, px(BASE_SIZE, SCALE));
    if cfg!(target_os = "macos") {
        assert_eq!(v.platform, vst3_platform::NS_VIEW);
        assert_eq!(v.scales, 0, "points: no content scale");
        assert_eq!(v.scaled_first, 0);
    } else {
        assert_eq!(v.platform, vst3_platform::X11);
        assert_eq!(v.scale, SCALE);
        assert_eq!(v.scaled_first, 1, "the scale came before attached");
    }
    assert!(!p.editor().unwrap().can_resize());
    assert_eq!(pv.probe.view().created, v.created, "an open view is reused");
}

#[test]
fn without_a_scale_and_with_bad_scales_the_editor_opens_at_scale_one() {
    for bad in [None, Some(0.0), Some(-2.0), Some(f64::NAN)] {
        let (pv, mut p) = view_plugin(0, (0, 0));
        let ed = p.editor().unwrap();
        if let Some(s) = bad {
            ed.set_scale(s);
        }
        assert_eq!(ed.open(parent()).unwrap(), BASE_SIZE, "{bad:?}");
        let v = pv.probe.view();
        assert_eq!(v.size, BASE_SIZE, "{bad:?}");
        if !cfg!(target_os = "macos") {
            assert_eq!(v.scale, 1.0, "{bad:?}");
        }
    }
}

#[test]
fn resize_goes_through_check_size_constraint_and_on_size_in_points() {
    let (pv, mut p, _) = opened(vst3_view_flags::RESIZABLE, (0, 0), SCALE);
    let ed = p.editor().unwrap();
    assert!(ed.can_resize());
    assert_eq!(ed.resize(500, 400).unwrap(), (500, 400));
    let v = pv.probe.view();
    assert_eq!(v.checks, 1);
    assert_eq!(v.on_sizes, 1);
    assert_eq!(v.on_size, px((500, 400), SCALE));
    // The view's constraint wins, converted back to points.
    let max = (
        (f64::from(MAX_SIZE.0) / api_scale(SCALE)).ceil() as u32,
        (f64::from(MAX_SIZE.1) / api_scale(SCALE)).ceil() as u32,
    );
    assert_eq!(ed.resize(10_000, 10_000).unwrap(), max);
    assert_eq!(pv.probe.view().on_size, MAX_SIZE);
}

#[test]
fn a_fixed_size_view_keeps_its_size_and_gets_no_on_size_from_a_user_resize() {
    let (pv, mut p, _) = opened(0, (0, 0), SCALE);
    let ed = p.editor().unwrap();
    assert_eq!(ed.resize(500, 400).unwrap(), BASE_SIZE);
    assert_eq!(pv.probe.view().on_sizes, 0);
}

#[test]
fn a_resize_view_inside_attached_is_reported_at_the_next_idle_and_answered_with_on_size() {
    let ask = px((360, 240), SCALE);
    let (pv, mut p, size) = opened(vst3_view_flags::RESIZE_IN_ATTACHED, ask, SCALE);
    assert_eq!(size, BASE_SIZE, "the view takes the size at onSize");
    let v = pv.probe.view();
    assert_eq!(v.attach_resize, TRUE, "accepted from a fixed-size view");
    assert_eq!(v.on_sizes, 0, "not answered inside attached");
    assert_eq!(p.idle().editor_resize, Some((360, 240)));
    let v = pv.probe.view();
    assert_eq!(v.on_sizes, 1);
    assert_eq!(v.on_size, ask);
    assert_eq!(p.idle().editor_resize, None, "reported once");
}

#[test]
fn a_resize_view_after_open_is_reported_and_the_latest_wins() {
    let (pv, mut p, _) = opened(0, (0, 0), SCALE);
    assert_eq!(i64::from(pv.probe.view_resize(10, 10)), TRUE);
    let want = px((320, 220), SCALE);
    assert_eq!(i64::from(pv.probe.view_resize(want.0, want.1)), TRUE);
    assert_eq!(p.idle().editor_resize, Some((320, 220)));
    assert_eq!(pv.probe.view().on_sizes, 1, "one answer, the latest size");
    assert_eq!(pv.probe.view().on_size, want);
    // Then the user's resize of a fixed view keeps the new size.
    assert_eq!(p.editor().unwrap().resize(1, 1).unwrap(), (320, 220));
    // Empty sizes are refused.
    assert_ne!(i64::from(pv.probe.view_resize(0, 10)), TRUE);
    assert_eq!(p.idle().editor_resize, None);
}

#[test]
fn a_resize_asked_before_close_is_not_reported_after_it() {
    let (pv, mut p, _) = opened(0, (0, 0), SCALE);
    assert_eq!(i64::from(pv.probe.view_resize(400, 400)), TRUE);
    p.editor().unwrap().close();
    assert_eq!(p.idle().editor_resize, None);
    assert_eq!(pv.probe.view().on_sizes, 0);
}

#[test]
fn close_removes_clears_the_frame_and_releases_the_view_in_order_and_reopens() {
    let (pv, mut p, _) = opened(0, (0, 0), SCALE);
    let ed = p.editor().unwrap();
    ed.close();
    let v = pv.probe.view();
    assert_eq!(v.removed, 1);
    assert_eq!(v.frame_clears, 1);
    assert_eq!(v.releases, v.created, "released");
    assert!(
        v.last_removed < v.last_frame_clear,
        "removed, then setFrame(null)"
    );
    assert!(v.last_frame_clear < v.last_release, "then the release");
    ed.close();
    assert_eq!(pv.probe.view().removed, 1, "a second close does nothing");
    // A new open makes a new view.
    assert_eq!(ed.open(parent()).unwrap(), BASE_SIZE);
    let w = pv.probe.view();
    assert_eq!(w.attached, 2);
    assert_eq!(w.created, v.created + 1);
}

#[test]
fn opening_an_open_editor_again_closes_the_first_view() {
    let (pv, mut p, _) = opened(0, (0, 0), SCALE);
    let ed = p.editor().unwrap();
    ed.open(parent()).unwrap();
    let v = pv.probe.view();
    assert_eq!(v.attached, 2);
    assert_eq!(v.removed, 1);
    assert_eq!(v.releases + 1, v.created, "only the new view lives");
}

#[test]
fn a_failed_attached_leaves_no_view_and_no_frame() {
    let (pv, mut p) = view_plugin(0, (0, 0));
    let ed = p.editor().unwrap();
    // SAFETY: a null parent; the view refuses it.
    let null = unsafe { ParentView::new(std::ptr::null_mut()) };
    assert!(ed.open(null).is_err());
    let v = pv.probe.view();
    assert_eq!(v.attached, 0);
    assert_eq!(v.frame_clears, 1, "the frame is taken back");
    assert_eq!(v.releases, v.created);
    assert_eq!(p.idle().editor_resize, None);
    assert!(
        p.editor().unwrap().open(parent()).is_ok(),
        "and it opens later"
    );
}

#[test]
fn dropping_an_open_editor_releases_the_view_before_the_controller_terminates() {
    let (pv, p, _) = opened(0, (0, 0), SCALE);
    drop(p);
    let v = pv.probe.view();
    let c = pv.probe.counters();
    assert_eq!(v.removed, 1);
    assert_eq!(v.frame_clears, 1);
    assert_eq!(v.releases, v.created);
    assert!(c.last_controller_term > 0);
    assert!(
        v.last_release < c.last_controller_term,
        "view released ({}) before the controller's terminate ({})",
        v.last_release,
        c.last_controller_term
    );
}

#[test]
fn a_new_scale_applies_at_the_next_open() {
    let (pv, mut p, _) = opened(0, (0, 0), SCALE);
    let ed = p.editor().unwrap();
    ed.set_scale(1.5);
    assert_eq!(pv.probe.view().size, px(BASE_SIZE, SCALE), "not yet");
    assert_eq!(ed.open(parent()).unwrap(), BASE_SIZE);
    assert_eq!(pv.probe.view().size, px(BASE_SIZE, 1.5));
}

#[cfg(target_os = "macos")]
#[test]
fn on_macos_the_frame_offers_no_run_loop() {
    let (pv, _p, _) = opened(0, (0, 0), SCALE);
    let v = pv.probe.view();
    assert_eq!(v.timer_registered, -1, "no Linux::IRunLoop");
}

#[test]
fn the_host_context_offers_a_run_loop_on_linux_only() {
    // VSTGUI takes its Linux run loop from the factory's host context and
    // keeps it for the module's life; without one its first editor dies.
    let (pv, _p) = view_plugin(0, (0, 0));
    assert_eq!(pv.probe.view().host_run_loop, cfg!(target_os = "linux"));
}

#[test]
fn sdk_again_has_an_editor_and_the_note_expression_synth_has_none() {
    let Some(again) = sample("again.vst3") else {
        return;
    };
    let mut p = load_vst3(&again, sdk::AGAIN);
    assert!(p.editor().is_some(), "again has a VSTGUI editor");
    let Some(synth) = sample("note-expression-synth.vst3") else {
        return;
    };
    let mut q = load_vst3(&synth, sdk::NOTE_EXPRESSION_SYNTH);
    assert!(q.editor().is_none());
}

#[cfg(target_os = "linux")]
mod linux {
    //! The frame's `Linux::IRunLoop`: the view's timer and descriptor go
    //! into the host's run loop and leave it with the view.
    use super::*;
    use core_host::{run_loop_fds, run_loop_tick};
    use std::time::{Duration, Instant};

    /// Ticks the run loop with every registered descriptor ready.
    fn tick_all(at: Instant) {
        let ready: Vec<_> = run_loop_fds();
        run_loop_tick(at, &ready);
    }

    #[test]
    fn the_views_timer_and_descriptor_reach_the_run_loop() {
        let (pv, _p, _) = opened(0, (0, 0), SCALE);
        let v = pv.probe.view();
        assert_eq!(v.timer_registered, TRUE);
        assert_eq!(v.fd_registered, TRUE);
        assert_eq!(run_loop_fds().len(), 1, "the view's pipe");
        assert!(pv.probe.view_poke());
        let t0 = Instant::now();
        tick_all(t0);
        tick_all(t0 + Duration::from_millis(100));
        let v = pv.probe.view();
        assert!(v.timer_calls >= 1, "timer: {}", v.timer_calls);
        assert!(v.fd_calls >= 1, "descriptor: {}", v.fd_calls);
    }

    #[test]
    fn closing_unregisters_and_drops_what_the_view_left_behind() {
        for flags in [0, vst3_view_flags::KEEP_RUN_LOOP] {
            let (pv, mut p, _) = opened(flags, (0, 0), SCALE);
            assert_eq!(run_loop_fds().len(), 1);
            p.editor().unwrap().close();
            assert!(run_loop_fds().is_empty(), "flags {flags}");
            let before = pv.probe.view().timer_calls;
            let t0 = Instant::now();
            tick_all(t0);
            tick_all(t0 + Duration::from_millis(100));
            assert_eq!(pv.probe.view().timer_calls, before, "flags {flags}");
            let want = if flags == 0 { 2 } else { 0 };
            assert_eq!(pv.probe.view().unregisters, want, "flags {flags}");
        }
    }

    #[test]
    fn dropping_the_instance_drops_the_views_registrations() {
        let (_pv, p, _) = opened(vst3_view_flags::KEEP_RUN_LOOP, (0, 0), SCALE);
        assert_eq!(run_loop_fds().len(), 1);
        drop(p);
        assert!(run_loop_fds().is_empty());
    }
}
