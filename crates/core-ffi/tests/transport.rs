//! Transport through the offline backend in real time. The engine host is
//! process-wide, so these tests run one at a time.

#![recursion_limit = "256"]

use core_engine::backend::BackendEvent;
use std::sync::{Mutex, MutexGuard};
use std::thread::sleep;
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{
    ReloadFault, drain_offline_log, engine_active, force_offline, hosts_created,
    inject_reload_fault, reset_host, simulate_event,
};
use vizdaw_ffi::{Session, TransportState};

const PPQ: i64 = 15_360;
const BAR: i64 = 4 * PPQ;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    inject_reload_fault(None);
    reset_host();
    g
}

fn ms(n: u64) {
    sleep(Duration::from_millis(n));
}

fn demo() -> (std::sync::Arc<Session>, u64) {
    let s = Session::new_demo().unwrap();
    let song = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id;
    (s, song)
}

fn t(s: &Session) -> TransportState {
    s.transport().unwrap()
}

/// Note-ons with velocity > 0 on MIDI channel 1 (the chords).
fn chord_note_ons() -> usize {
    drain_offline_log()
        .iter()
        .filter(|(_, b)| b.len() == 3 && b[0] == 0x90 && b[2] > 0)
        .count()
}

#[test]
fn before_play_the_transport_is_idle() {
    let _g = serial();
    let (s, _) = demo();
    let st = t(&s);
    assert!(!st.playing);
    assert_eq!(st.tick, 0);
    assert_eq!(st.seq, None);
    assert_eq!(st.position_bbu, "1.1.0");
    assert_eq!(st.device, "");
}

#[test]
fn position_advances_monotonically() {
    let _g = serial();
    let (s, song) = demo();
    s.play(song).unwrap();
    let mut last = -1;
    for _ in 0..6 {
        ms(40);
        let st = t(&s);
        assert!(st.playing);
        assert!(st.tick >= last, "{} < {last}", st.tick);
        last = st.tick;
    }
    let st = t(&s);
    assert!(st.tick > 0);
    assert_eq!(st.seq, Some(song));
    assert_eq!(st.device, "Offline");
    assert_eq!(st.sample_rate, 48_000.0);
    assert!(st.position_bbu.starts_with("1."), "{}", st.position_bbu);
    assert!(st.seconds > 0.1 && st.seconds < 2.0, "{}", st.seconds);
    assert!(st.faults.is_empty(), "{:?}", st.faults);
}

#[test]
fn stop_holds_the_position_and_play_resumes() {
    let _g = serial();
    let (s, song) = demo();
    s.play(song).unwrap();
    ms(150);
    s.stop().unwrap();
    ms(30);
    let held = t(&s);
    assert!(!held.playing);
    assert!(held.tick > 0);
    ms(100);
    assert_eq!(t(&s).tick, held.tick);
    s.play(song).unwrap();
    ms(60);
    assert!(t(&s).tick > held.tick);
}

#[test]
fn locate_moves_while_playing_and_before_play() {
    let _g = serial();
    let (s, song) = demo();
    s.play(song).unwrap();
    ms(30);
    s.locate(4 * BAR).unwrap();
    ms(60);
    let st = t(&s);
    assert!(st.tick >= 4 * BAR && st.tick < 5 * BAR, "{}", st.tick);
    assert!(st.position_bbu.starts_with("5.1."), "{}", st.position_bbu);

    // A fresh session that does not hold the engine keeps the position
    // for its first Play.
    let (other, song2) = demo();
    other.locate(2 * BAR).unwrap();
    assert_eq!(t(&other).tick, 2 * BAR);
    other.play(song2).unwrap();
    ms(30);
    let st = t(&other);
    assert!(st.tick >= 2 * BAR && st.tick < 3 * BAR, "{}", st.tick);
}

#[test]
fn loop_wraps_to_bar_one() {
    let _g = serial();
    let (s, song) = demo();
    let len = 12 * BAR;
    s.set_loop(true).unwrap();
    s.locate(len - PPQ / 4).unwrap();
    s.play(song).unwrap();
    ms(400);
    let st = t(&s);
    assert!(st.playing);
    assert!(st.tick < BAR, "did not wrap: {}", st.tick);
}

#[test]
fn playback_ends_after_the_last_bar() {
    let _g = serial();
    let (s, song) = demo();
    s.locate(12 * BAR - PPQ / 4).unwrap();
    s.play(song).unwrap();
    let t0 = Instant::now();
    while t(&s).playing {
        assert!(t0.elapsed() < Duration::from_secs(4), "never ended");
        ms(50);
    }
    assert!(!t(&s).playing);
}

#[test]
fn playing_a_second_session_stops_the_first() {
    let _g = serial();
    let (a, song_a) = demo();
    let (b, song_b) = demo();
    a.play(song_a).unwrap();
    ms(50);
    b.play(song_b).unwrap();
    ms(50);
    assert!(!t(&a).playing);
    assert!(t(&b).playing);
    // Stopping the first session does not touch the second.
    a.stop().unwrap();
    ms(30);
    assert!(t(&b).playing);
}

#[test]
fn muting_while_playing_recompiles() {
    let _g = serial();
    let (s, song) = demo();
    let chords = s.lanes(song).unwrap()[1].id;
    s.play(song).unwrap();
    ms(100);
    assert!(chord_note_ons() > 0, "bar 1 chord missing");

    s.set_lane_mute(song, chords, true).unwrap();
    drain_offline_log();
    s.locate(BAR).unwrap();
    ms(300);
    assert_eq!(chord_note_ons(), 0, "muted chords still play");

    s.undo().unwrap();
    drain_offline_log();
    s.locate(2 * BAR).unwrap();
    ms(300);
    assert!(chord_note_ons() > 0, "undo did not bring the chords back");
}

#[test]
fn dropping_the_playing_session_stops_the_engine() {
    let _g = serial();
    let (a, song) = demo();
    // Start half a beat before bar 2, where the chords change.
    a.locate(BAR - PPQ / 2).unwrap();
    a.play(song).unwrap();
    ms(20);
    assert!(engine_active());
    drop(a);
    // The release tail may still emit note-offs; no new note-ons may start.
    drain_offline_log();
    ms(600);
    assert_eq!(chord_note_ons(), 0, "audio kept playing after the drop");
}

#[test]
fn a_failed_reload_keeps_the_edit_and_shows_as_a_fault() {
    let _g = serial();
    let (s, song) = demo();
    let chords = s.lanes(song).unwrap()[1].id;
    s.play(song).unwrap();
    inject_reload_fault(Some(ReloadFault::Error));
    let snap = s.set_lane_mute(song, chords, true).unwrap();
    assert!(snap.undo_label.is_some());
    assert!(s.lanes(song).unwrap()[1].mute);
    let faults = t(&s).faults;
    assert!(
        faults.iter().any(|f| f.contains("reload failed")),
        "{faults:?}"
    );
    // The next good recompile clears it.
    s.undo().unwrap();
    assert!(t(&s).faults.is_empty());
}

#[test]
fn a_panic_during_reload_resets_the_host_and_keeps_the_edit() {
    let _g = serial();
    let (s, song) = demo();
    let chords = s.lanes(song).unwrap()[1].id;
    s.play(song).unwrap();
    inject_reload_fault(Some(ReloadFault::Panic));
    s.set_lane_mute(song, chords, true).unwrap();
    assert!(!engine_active(), "host not reset");
    assert!(s.lanes(song).unwrap()[1].mute);
    let st = t(&s);
    assert!(!st.playing);
    assert!(st.faults.iter().any(|f| f.contains("reload failed")));
    // A later Play starts clean.
    s.play(song).unwrap();
    ms(40);
    assert!(t(&s).playing);
}

#[test]
fn locate_without_the_engine_formats_against_a_sequence() {
    let _g = serial();
    let (s, song) = demo();
    s.locate(2 * BAR).unwrap();
    let st = t(&s);
    assert_eq!(st.seq, Some(song));
    assert!(st.position_bbu.starts_with("3.1."), "{}", st.position_bbu);
    assert!(st.seconds > 3.0, "{}", st.seconds);
}

#[test]
fn a_device_change_rebuilds_and_resumes_at_the_same_position() {
    let _g = serial();
    let (s, song) = demo();
    s.set_loop(true).unwrap();
    s.play(song).unwrap();
    s.locate(3 * BAR).unwrap();
    ms(60);
    let before = t(&s).tick;
    let created = hosts_created();
    simulate_event(BackendEvent::DefaultDeviceChanged);
    // The owner's poll handles the event: drain 300 ms, rebuild, resume.
    t(&s);
    assert_eq!(hosts_created(), created + 1, "no rebuild");
    // The new engine applies Play on its first block.
    ms(30);
    let st = t(&s);
    assert!(st.playing, "playback did not resume");
    assert_eq!(st.device, "Offline");
    // Resumed at the old position, not at bar 1.
    assert!(
        st.tick >= before && st.tick < 4 * BAR,
        "{} vs {before}",
        st.tick
    );
    // A lost device rebuilds without the drain wait.
    simulate_event(BackendEvent::DeviceLost);
    t(&s);
    ms(30);
    assert!(t(&s).playing);
    assert_eq!(hosts_created(), created + 2);
}

#[test]
fn a_buffer_size_change_keeps_the_engine() {
    let _g = serial();
    let (s, song) = demo();
    s.play(song).unwrap();
    ms(30);
    let created = hosts_created();
    simulate_event(BackendEvent::BufferSizeChanged(8192));
    t(&s);
    ms(30);
    assert!(t(&s).playing);
    assert_eq!(hosts_created(), created);
}

#[test]
fn switching_sequences_starts_the_new_one_at_bar_one() {
    let _g = serial();
    let (s, song) = demo();
    let groove = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Groove")
        .unwrap()
        .id;
    s.play(song).unwrap();
    s.locate(6 * BAR).unwrap();
    ms(30);
    s.play(groove).unwrap();
    ms(30);
    let st = t(&s);
    assert_eq!(st.seq, Some(groove));
    assert!(st.tick < BAR, "{}", st.tick);
}

#[test]
fn a_device_event_on_a_released_host_rebuilds_on_the_next_play() {
    let _g = serial();
    let (a, song) = demo();
    a.play(song).unwrap();
    ms(20);
    drop(a);
    let created = hosts_created();
    // Nobody owns the engine, so nobody polls the event.
    simulate_event(BackendEvent::DeviceLost);
    let (b, song_b) = demo();
    b.play(song_b).unwrap();
    assert_eq!(hosts_created(), created + 1, "played on the lost device");
    ms(30);
    assert!(t(&b).playing);
}

#[test]
fn shutdown_stops_and_drops_the_engine_host() {
    let _g = serial();
    let (s, song) = demo();
    s.play(song).unwrap();
    ms(20);
    assert!(engine_active());
    vizdaw_ffi::shutdown().unwrap();
    assert!(!engine_active());
    // Idempotent, and the session still closes cleanly.
    vizdaw_ffi::shutdown().unwrap();
    s.close().unwrap();
}

#[test]
fn a_device_rebuild_drains_without_holding_the_host_lock() {
    let _g = serial();
    let (s, song) = demo();
    s.play(song).unwrap();
    ms(40);
    simulate_event(BackendEvent::DefaultDeviceChanged);
    // The owner's poll handles the event on another thread: 300 ms of
    // drain, then the rebuild.
    let poller = std::sync::Arc::clone(&s);
    let poll = std::thread::spawn(move || poller.transport().map(|t| t.playing));
    ms(100);
    let t0 = Instant::now();
    let _ = vizdaw_ffi::host::factory_owner();
    let waited = t0.elapsed();
    assert!(
        waited < Duration::from_millis(150),
        "another host call waited {waited:?} for the drain"
    );
    assert!(poll.join().unwrap().is_ok());
    ms(30);
    assert!(t(&s).playing, "playback resumed after the rebuild");
}
