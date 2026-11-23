//! The main-thread run loop behind CLAP `timer-support` and
//! `posix-fd-support`, driven through the timer test plug-in with a
//! deterministic clock. Each test runs on its own thread, and the run loop
//! belongs to the thread that creates the instances, so tests never see
//! each other's registrations.

use core_host::{FdFlags, PluginInstance, run_loop_fds, run_loop_tick};
use core_model::mixer::ParamId;
use std::time::{Duration, Instant};
use test_plugins::{
    PARAM_FD_CALLS, PARAM_FD_FLAGS, PARAM_FD_MODIFY, PARAM_FD_REGISTER, PARAM_FD_SELF_UNREGISTER,
    PARAM_FD_UNREGISTER, PARAM_LAST_RESULT, PARAM_TIMER_FIRES, PARAM_TIMER_REGISTER,
    PARAM_TIMER_SELF_UNREGISTER, PARAM_TIMER_SPAWN, PARAM_TIMER_UNREGISTER,
};

mod common;

const TIMER: &str = "vizdaw.test.timer";
const R: FdFlags = FdFlags::READ;
const W: FdFlags = FdFlags::WRITE;
const E: FdFlags = FdFlags::ERROR;

fn read(p: &mut dyn PluginInstance, id: u32) -> f64 {
    p.param_value(ParamId(u64::from(id))).unwrap()
}

/// Sends one switch to the plug-in and returns its result readout.
fn cmd(p: &mut dyn PluginInstance, id: u32, value: f64) -> f64 {
    p.set_params(&[(ParamId(u64::from(id)), value)]).unwrap();
    read(p, PARAM_LAST_RESULT)
}

fn register_timer(p: &mut dyn PluginInstance, period_ms: u32) -> u32 {
    let id = cmd(p, PARAM_TIMER_REGISTER, f64::from(period_ms));
    assert!(id >= 0.0, "the host refused a {period_ms} ms timer");
    id as u32
}

fn fires(p: &mut dyn PluginInstance, id: u32) -> u32 {
    read(p, PARAM_TIMER_FIRES + id) as u32
}

fn fd_value(fd: i32, flags: FdFlags) -> f64 {
    f64::from(fd as u32 * 8 + flags.bits())
}

fn fd_calls(p: &mut dyn PluginInstance, fd: i32) -> u32 {
    read(p, PARAM_FD_CALLS + fd as u32) as u32
}

fn fd_flags(p: &mut dyn PluginInstance, fd: i32) -> FdFlags {
    FdFlags::from_bits_truncate(read(p, PARAM_FD_FLAGS + fd as u32) as u32)
}

fn at(t0: Instant, ms: u64) -> Instant {
    t0 + Duration::from_millis(ms)
}

#[test]
fn timers_fire_once_per_period_until_unregistered() {
    let mut p = common::load(TIMER);
    let p = p.as_mut();
    let a = register_timer(p, 10);
    let b = register_timer(p, 25);
    assert_ne!(a, b, "ids are unique within an instance");
    let t0 = Instant::now();
    // The first tick arms new timers; they fire a period later.
    run_loop_tick(t0, &[]);
    run_loop_tick(at(t0, 9), &[]);
    assert_eq!((fires(p, a), fires(p, b)), (0, 0));
    run_loop_tick(at(t0, 10), &[]);
    assert_eq!((fires(p, a), fires(p, b)), (1, 0));
    run_loop_tick(at(t0, 10), &[]);
    run_loop_tick(at(t0, 19), &[]);
    assert_eq!(fires(p, a), 1, "once per period");
    run_loop_tick(at(t0, 20), &[]);
    run_loop_tick(at(t0, 25), &[]);
    assert_eq!((fires(p, a), fires(p, b)), (2, 1));
    // A late tick fires each due timer once; missed periods are dropped.
    run_loop_tick(at(t0, 1000), &[]);
    assert_eq!((fires(p, a), fires(p, b)), (3, 2));
    run_loop_tick(at(t0, 1005), &[]);
    assert_eq!(fires(p, a), 3, "rescheduled from the late tick");
    run_loop_tick(at(t0, 1010), &[]);
    assert_eq!(fires(p, a), 4);
    // Unregistered: never again; a second unregister (or an unknown id)
    // fails.
    assert_eq!(cmd(p, PARAM_TIMER_UNREGISTER, f64::from(a)), 1.0);
    assert_eq!(cmd(p, PARAM_TIMER_UNREGISTER, f64::from(a)), 0.0);
    assert_eq!(cmd(p, PARAM_TIMER_UNREGISTER, 999.0), 0.0);
    run_loop_tick(at(t0, 5000), &[]);
    assert_eq!((fires(p, a), fires(p, b)), (4, 3));
}

#[test]
fn fds_register_modify_unregister_and_get_their_ready_events() {
    let mut p = common::load(TIMER);
    let p = p.as_mut();
    assert!(run_loop_fds().is_empty());
    assert_eq!(cmd(p, PARAM_FD_REGISTER, fd_value(7, R | W)), 1.0);
    assert_eq!(cmd(p, PARAM_FD_REGISTER, fd_value(5, R)), 1.0);
    assert_eq!(
        cmd(p, PARAM_FD_REGISTER, fd_value(5, W)),
        0.0,
        "a registered descriptor changes through modify"
    );
    assert_eq!(run_loop_fds(), vec![(5, R), (7, R | W)]);
    assert_eq!(cmd(p, PARAM_FD_MODIFY, fd_value(5, W)), 1.0);
    assert_eq!(cmd(p, PARAM_FD_MODIFY, fd_value(9, W)), 0.0, "unknown");
    assert_eq!(run_loop_fds(), vec![(5, W), (7, R | W)]);

    let t0 = Instant::now();
    run_loop_tick(t0, &[(7, R)]);
    assert_eq!((fd_calls(p, 7), fd_flags(p, 7)), (1, R));
    // Only the events it asked for (errors always) reach the plug-in.
    run_loop_tick(t0, &[(5, R)]);
    assert_eq!(fd_calls(p, 5), 0);
    run_loop_tick(t0, &[(5, R | W | E), (11, R)]);
    assert_eq!((fd_calls(p, 5), fd_flags(p, 5)), (1, W | E));
    assert_eq!(fd_calls(p, 11), 0, "not registered");
    // Level-triggered: still ready, called again on the next tick.
    run_loop_tick(t0, &[(7, R)]);
    assert_eq!(fd_calls(p, 7), 2);

    assert_eq!(cmd(p, PARAM_FD_UNREGISTER, 7.0), 1.0);
    assert_eq!(cmd(p, PARAM_FD_UNREGISTER, 7.0), 0.0);
    assert_eq!(run_loop_fds(), vec![(5, W)]);
    run_loop_tick(t0, &[(7, R)]);
    assert_eq!(fd_calls(p, 7), 2);
}

#[test]
fn a_timer_or_fd_can_unregister_itself_inside_its_callback() {
    let mut p = common::load(TIMER);
    let p = p.as_mut();
    let t = register_timer(p, 0);
    let keep = register_timer(p, 0);
    cmd(p, PARAM_TIMER_SELF_UNREGISTER, f64::from(t));
    assert_eq!(cmd(p, PARAM_FD_REGISTER, fd_value(3, R)), 1.0);
    assert_eq!(cmd(p, PARAM_FD_REGISTER, fd_value(4, R)), 1.0);
    cmd(p, PARAM_FD_SELF_UNREGISTER, 3.0);
    let t0 = Instant::now();
    run_loop_tick(t0, &[(3, R), (4, R)]);
    assert_eq!((fires(p, t), fires(p, keep)), (1, 1));
    assert_eq!((fd_calls(p, 3), fd_calls(p, 4)), (1, 1));
    assert_eq!(run_loop_fds(), vec![(4, R)]);
    run_loop_tick(at(t0, 1), &[(3, R), (4, R)]);
    assert_eq!((fires(p, t), fires(p, keep)), (1, 2));
    assert_eq!((fd_calls(p, 3), fd_calls(p, 4)), (1, 2));
    assert_eq!(
        cmd(p, PARAM_TIMER_UNREGISTER, f64::from(t)),
        0.0,
        "already gone"
    );
}

#[test]
fn destroying_an_instance_leaves_nothing_behind() {
    let mut a = common::load(TIMER);
    let mut b = common::load(TIMER);
    let ta = register_timer(a.as_mut(), 0);
    let tb = register_timer(b.as_mut(), 0);
    assert_eq!(cmd(a.as_mut(), PARAM_FD_REGISTER, fd_value(4, R)), 1.0);
    assert_eq!(cmd(b.as_mut(), PARAM_FD_REGISTER, fd_value(6, R)), 1.0);
    assert_eq!(run_loop_fds(), vec![(4, R), (6, R)]);
    let t0 = Instant::now();
    run_loop_tick(t0, &[(4, R), (6, R)]);
    assert_eq!((fires(a.as_mut(), ta), fires(b.as_mut(), tb)), (1, 1));
    drop(a);
    assert_eq!(run_loop_fds(), vec![(6, R)]);
    run_loop_tick(at(t0, 1), &[(4, R), (6, R)]);
    assert_eq!(fires(b.as_mut(), tb), 2);
    assert_eq!(fd_calls(b.as_mut(), 6), 2);
    drop(b);
    assert!(run_loop_fds().is_empty());
    run_loop_tick(at(t0, 2), &[(4, R), (6, R)]);
}

#[test]
fn a_period_0_flood_fires_at_most_once_per_tick() {
    let mut p = common::load(TIMER);
    let p = p.as_mut();
    let zero = register_timer(p, 0);
    let tiny = register_timer(p, 1);
    let t0 = Instant::now();
    for _ in 0..5 {
        run_loop_tick(t0, &[]);
    }
    assert_eq!(fires(p, zero), 5, "once per tick, even at one instant");
    assert_eq!(fires(p, tiny), 0, "armed at t0, due at t0 + 1 ms");
    run_loop_tick(at(t0, 10_000), &[]);
    assert_eq!(fires(p, tiny), 1, "ten thousand missed periods fire once");
    // A callback that registers a new period-0 timer every time: the new
    // one waits for the next tick, so a tick's work is what was there when
    // it started.
    cmd(p, PARAM_TIMER_SPAWN, f64::from(zero));
    run_loop_tick(at(t0, 10_001), &[]);
    let first = read(p, PARAM_LAST_RESULT) as u32;
    assert_eq!(fires(p, first), 0);
    run_loop_tick(at(t0, 10_002), &[]);
    let second = read(p, PARAM_LAST_RESULT) as u32;
    assert_ne!(first, second);
    assert_eq!((fires(p, first), fires(p, second)), (1, 0));
    assert_eq!(fires(p, zero), 8);
}
