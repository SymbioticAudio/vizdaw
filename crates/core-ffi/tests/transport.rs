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

/// Polls the transport until `ok` holds or 5 s pass (slow CI); returns
/// whether it held. The engine applies Play and Stop a block late.
fn wait_for(s: &Session, ok: impl Fn(&TransportState) -> bool) -> bool {
    let start = Instant::now();
    loop {
        if ok(&t(s)) {
            return true;
        }
        if start.elapsed() > Duration::from_secs(5) {
            return false;
        }
        ms(2);
    }
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
    assert!(
        wait_for(&s, |t| t.playing),
        "playback resumed after the rebuild"
    );
}

#[test]
fn a_rebuild_right_after_play_still_resumes_playback() {
    // The engine applies Play on its next audio block, so a device event
    // handled before that must not read "stopped" and drop the Play. The
    // slower the machine, the wider the window; repeat to hit it here.
    let _g = serial();
    let (s, song) = demo();
    for round in 0..15 {
        s.stop().unwrap();
        ms(10);
        s.play(song).unwrap();
        simulate_event(BackendEvent::DeviceLost);
        s.transport().unwrap();
        assert!(
            wait_for(&s, |t| t.playing),
            "round {round}: playback lost in the rebuild"
        );
    }
}

#[test]
fn a_rebuild_during_the_release_tail_does_not_restart_the_song() {
    let _g = serial();
    let (s, song) = demo();
    let len = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.id == song)
        .unwrap()
        .length_ticks;
    s.locate(len - 200).unwrap();
    s.play(song).unwrap();
    // The end is a few ms away; the tail then rings out for about a second.
    ms(200);
    assert!(t(&s).playing, "in the tail");
    simulate_event(BackendEvent::DeviceLost);
    s.transport().unwrap();
    ms(60);
    let st = t(&s);
    assert!(!st.playing, "the song was restarted");
    assert!(st.tick > len / 2, "back at {}", st.tick);
}

#[test]
fn a_stop_during_the_rebuild_drain_is_not_lost() {
    let _g = serial();
    let (s, song) = demo();
    s.play(song).unwrap();
    assert!(wait_for(&s, |t| t.playing));
    simulate_event(BackendEvent::DefaultDeviceChanged);
    let poller = std::sync::Arc::clone(&s);
    let poll = std::thread::spawn(move || poller.transport().map(|t| t.playing));
    ms(100);
    s.stop().unwrap();
    assert!(poll.join().unwrap().is_ok());
    // The engine applies Stop a block late: wait for it (generously, slow
    // CI) instead of a fixed sleep; a lost Stop keeps playing and fails.
    assert!(
        wait_for(&s, |t| !t.playing),
        "the Stop was lost to the rebuild"
    );
    assert!(!t(&s).playing, "the rebuild did not start playing again");
}

#[test]
fn a_device_rebuild_while_stopped_keeps_the_automated_fader() {
    // Plan 4d Ruling 13: after Stop the last automated value stays until
    // the user moves the fader, a device change included (final review
    // Minor 1).
    use core_engine::fixtures::Builder;
    use core_model::automation::{AutomationTarget, fader_norm};
    use core_model::event::{EventBody, EventSpec};
    use core_model::mixer::StripKind;
    use core_model::time::Tick;
    use vizdaw_ffi::host::fader_override;
    let _g = serial();
    let mut b = Builder::new();
    let dest = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(dest)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0xFFFF, 16 * BAR)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let auto = b.lane(seq, "Auto", None).unwrap();
    let target = AutomationTarget::StripFader(synth);
    b.events(
        seq,
        auto,
        vec![EventSpec::new(
            Tick(0),
            EventBody::Automation {
                target,
                value: fader_norm(-12.0),
            },
        )],
    )
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.play(seq.0).unwrap();
    let start = Instant::now();
    while fader_override(synth.0).is_none() {
        assert!(start.elapsed() < Duration::from_secs(5), "never automated");
        ms(2);
    }
    s.stop().unwrap();
    assert!(wait_for(&s, |t| !t.playing));
    let held = fader_override(synth.0).expect("Stop keeps the override");
    assert!((held + 12.0).abs() < 1e-6, "{held}");
    let created = hosts_created();
    simulate_event(BackendEvent::SampleRateChanged(44_100.0));
    t(&s);
    assert_eq!(hosts_created(), created + 1, "no rebuild");
    assert!(!t(&s).playing);
    let after = fader_override(synth.0);
    assert!(
        after.is_some_and(|v| (v + 12.0).abs() < 1e-6),
        "the rebuilt engine dropped the override: {after:?}"
    );
}

/// Play Selection (plan 4f Ruling 9): playing from a tick up to another
/// stops there by itself, the loop on or not; a plain Play then goes on.
#[test]
fn play_from_stops_at_the_end_of_its_range() {
    let _g = serial();
    let (s, song) = demo();
    s.set_loop(true).unwrap();
    s.play_from(song, BAR, Some(BAR + PPQ / 2)).unwrap();
    assert!(wait_for(&s, |st| st.playing && st.tick >= BAR));
    assert!(wait_for(&s, |st| !st.playing), "it stops by itself");
    let st = t(&s);
    assert!(st.tick >= BAR && st.tick <= BAR + PPQ, "{}", st.tick);
    s.play(song).unwrap();
    assert!(
        wait_for(&s, |st| st.tick > BAR + PPQ),
        "a plain Play goes on"
    );
    s.stop().unwrap();
    for (from, to) in [
        (BAR, Some(BAR)),
        (BAR, Some(0)),
        (1 << 41, None),
        (0, Some(1 << 41)),
    ] {
        assert!(
            matches!(
                s.play_from(song, from, to),
                Err(vizdaw_ffi::VizError::Model { .. })
            ),
            "{from} {to:?}"
        );
    }
    assert!(s.play_from(9_999, 0, None).is_err(), "no such sequence");
}

/// A device change while a selection plays resumes with its end (plan 4f
/// Ruling 9): it still stops there, not at the sequence end.
#[test]
fn a_device_change_keeps_play_selections_end() {
    let _g = serial();
    let (s, song) = demo();
    s.play_from(song, BAR, Some(BAR + 2 * PPQ)).unwrap();
    assert!(wait_for(&s, |st| st.playing && st.tick >= BAR));
    let created = hosts_created();
    simulate_event(BackendEvent::DeviceLost);
    t(&s);
    assert_eq!(hosts_created(), created + 1, "rebuilt");
    // The new engine applies Play on its first block.
    assert!(wait_for(&s, |st| st.playing), "resumed");
    assert!(wait_for(&s, |st| !st.playing), "it still stops at its end");
    let st = t(&s);
    assert!(st.tick >= BAR && st.tick <= BAR + 3 * PPQ, "{}", st.tick);
}

#[test]
fn loop_wraps_in_a_new_project() {
    let _g = serial();
    let s = Session::new_empty().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    s.set_loop(true).unwrap();
    s.play(seq).unwrap();
    // One empty bar lasts 2 s at 120 bpm; the loop keeps it playing.
    assert!(wait_for(&s, |st| st.playing));
    ms(2600);
    let st = t(&s);
    assert!(st.playing, "the loop ended: {st:?}");
    assert!(st.tick < BAR, "{}", st.tick);
}

#[test]
fn loop_wraps_with_an_audio_track() {
    let _g = serial();
    let s = Session::new_demo_audio().unwrap();
    let song = s.snapshot().unwrap().sequences[0].id;
    let len = s.snapshot().unwrap().sequences[0].length_ticks;
    s.set_loop(true).unwrap();
    s.locate(len - PPQ / 4).unwrap();
    s.play(song).unwrap();
    ms(500);
    let st = t(&s);
    assert!(st.playing, "{st:?}");
    assert!(st.tick < len - PPQ, "did not wrap: {}", st.tick);
}

/// Sequence "S", 8 bars at 120 BPM: track "Keys" on the built-in synth,
/// MIDI input from "Keys", armed.
fn armed_keys() -> (std::sync::Arc<Session>, u64) {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, SequenceCmd};
    use core_model::lane::LaneInput;
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("S", "Keys", Some(synth)).unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: keys,
        input: Some(LaneInput::Midi {
            port: Some("Keys".into()),
            channel: None,
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq,
        lane: keys,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(8 * 4 * Q)),
    }))
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    (s, seq.0)
}

/// Plan release-fixes Task 1 (owner decision 1): a device rebuild during
/// Record's count-in resumes it from the next beat: the timeline still
/// stands at the record start, and the transport rolls after the beats
/// left.
fn a_rebuild_during_the_count_in(event: BackendEvent) {
    use core_engine::fixtures::Q;
    let (s, seq) = armed_keys();
    s.locate(4 * Q).unwrap();
    s.record(
        seq,
        vizdaw_ffi::RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 1,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    // One 4/4 bar at 120 BPM: a click each 500 ms. The first has sounded.
    assert!(wait_for(&s, |st| st.counting_in), "no count-in");
    ms(100);
    let created = hosts_created();
    simulate_event(event);
    t(&s);
    assert_eq!(hosts_created(), created + 1, "no rebuild");
    let rebuilt = Instant::now();
    assert!(
        wait_for(&s, |st| st.counting_in),
        "the count-in did not resume"
    );
    let st = t(&s);
    assert!(!st.playing);
    assert_eq!(st.tick, 4 * Q, "the timeline moved");
    assert!(wait_for(&s, |st| st.playing), "it never rolled");
    // Three beats left: 1.5 s, give or take a block and the polling. The
    // margins hold on a loaded runner (Task 9): the new engine can start
    // counting before `rebuilt` is read, and the polling can see the roll
    // late. The low bound is halfway to two beats (1.0 s, a resume that
    // lost a beat); the high one catches a count-in that never ends or
    // counts its beats again.
    let took = rebuilt.elapsed();
    assert!(
        took >= Duration::from_millis(1250) && took < Duration::from_millis(3000),
        "rolled after {took:?}"
    );
    // A rebuild still ends the recording pass (backlog, 27.1.0): only the
    // count-in is asserted here.
    s.stop().unwrap();
    s.close().unwrap();
}

#[test]
fn a_rebuild_during_the_count_in_keeps_counting() {
    let _g = serial();
    a_rebuild_during_the_count_in(BackendEvent::DefaultDeviceChanged);
}

#[test]
fn a_rebuild_during_the_count_in_keeps_counting_after_a_rate_change() {
    let _g = serial();
    a_rebuild_during_the_count_in(BackendEvent::SampleRateChanged(48_000.0));
}
