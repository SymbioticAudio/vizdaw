//! Transport through the offline backend in real time. The engine host is
//! process-wide, so these tests run one at a time.

use std::sync::{Mutex, MutexGuard};
use std::thread::sleep;
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{drain_offline_log, force_offline, reset_host};
use vizdaw_ffi::{Session, TransportState};

const PPQ: i64 = 15_360;
const BAR: i64 = 4 * PPQ;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
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
    a.play(song).unwrap();
    ms(30);
    drop(a);
    let (b, _) = demo();
    assert!(!t(&b).playing);
}
