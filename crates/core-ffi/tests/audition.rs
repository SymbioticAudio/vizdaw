//! Audition through the session and the transport's host-time stamp
//! (spec §3.2, §3.4). The engine host is process-wide: one test at a time.

use std::sync::{Mutex, MutexGuard};
use std::thread::sleep;
use std::time::Duration;
use vizdaw_ffi::Session;
use vizdaw_ffi::host::{drain_offline_log, force_offline, reset_host};

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

fn demo() -> (std::sync::Arc<Session>, u64, u64) {
    let s = Session::new_demo().unwrap();
    let song = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id;
    let piano = s
        .destinations()
        .unwrap()
        .iter()
        .find(|d| d.name == "Built-in Piano")
        .unwrap()
        .id;
    (s, song, piano)
}

#[test]
fn audition_needs_the_engine_and_then_sounds() {
    let _g = serial();
    let (s, song, piano) = demo();
    assert!(
        !s.audition(piano, 72, 0x8000, true).unwrap(),
        "no engine before the first Play (Plan Ruling 9)"
    );
    s.play(song).unwrap();
    sleep(Duration::from_millis(50));
    drain_offline_log();
    assert!(s.audition(piano, 72, 0x8000, true).unwrap());
    sleep(Duration::from_millis(100));
    assert!(s.audition(piano, 72, 0, false).unwrap());
    sleep(Duration::from_millis(100));
    s.stop().unwrap();
    let log = drain_offline_log();
    assert!(log.iter().any(|(_, b)| b == &vec![0x90, 72, 64]), "{log:?}");
    assert!(log.iter().any(|(_, b)| b == &vec![0x80, 72, 0]), "{log:?}");
    reset_host();
}

#[test]
fn the_transport_carries_host_time_and_tempo() {
    let _g = serial();
    let (s, _, _) = demo();
    let a = s.transport().unwrap();
    sleep(Duration::from_millis(2));
    let b = s.transport().unwrap();
    assert!(b.host_time_ns > a.host_time_ns);
    assert_eq!(a.ticks_per_second, 30_720.0, "120 BPM");
    // Song ramps from 120 to 90 BPM over its last two bars.
    s.locate(12 * BAR - Q).unwrap();
    let t = s.transport().unwrap();
    assert!(
        t.ticks_per_second < 30_720.0 && t.ticks_per_second > 23_000.0,
        "{}",
        t.ticks_per_second
    );
}
