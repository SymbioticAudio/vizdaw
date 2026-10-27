//! The input opens on its own thread (plan 5d-1 Rulings 13–15; 5c carries
//! T11 M2, M3 and M6): an arm returns at once while the device opens, the
//! transport and the meters stay answerable meanwhile, Record waits for
//! the input (a bounded while), a denial at the first microphone prompt is
//! reported when arming, and an input that failed to open is tried again
//! when a lane's input or arm changes. The offline device's test hooks
//! stand in for the device's slowness, the prompt and a busy interface:
//! no device is opened and no microphone asked.

#![recursion_limit = "256"]

mod common;

use core_engine::offline::FakeInput;
use platform_macos::permission::DENIED_MESSAGE;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{
    debug_fail_input_opens, debug_hold_microphone_prompt, debug_input_open_delay, force_offline,
    reset_host, set_fake_input, set_fake_microphone_denied, set_fake_microphone_prompt,
};
use vizdaw_ffi::{EditCommand, LaneEdit, LaneInputRow, LaneRow, RecordSpecRow, Session, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(Some(FakeInput::Signals));
    debug_input_open_delay(None);
    set_fake_microphone_prompt(None);
    debug_hold_microphone_prompt(false);
    set_fake_microphone_denied(false);
    debug_fail_input_opens(0);
    reset_host();
    g
}

fn song(s: &Session) -> u64 {
    s.snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id
}

fn lane(s: &Session, seq: u64, name: &str) -> LaneRow {
    s.lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == name)
        .unwrap()
}

fn edit(s: &Session, e: LaneEdit) -> Result<(), VizError> {
    s.edit(EditCommand::Lane { edit: e }, None).map(|_| ())
}

fn arm(s: &Session, seq: u64, lane: u64, on: bool) {
    edit(
        s,
        LaneEdit::SetFlags {
            seq,
            lane,
            mute: None,
            solo: None,
            arm: Some(on),
        },
    )
    .unwrap();
}

/// Plays and stops, so the session holds the (idle) engine.
fn take_engine(s: &Session, seq: u64) {
    s.play(seq).unwrap();
    wait_until("playing", || s.transport().unwrap().playing);
    s.stop().unwrap();
    wait_until("stopped", || !s.transport().unwrap().playing);
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The demo's lane "Loop" records fake input 1 (not armed yet).
fn audio_lane(s: &Session, seq: u64) -> u64 {
    let lp = lane(s, seq, "Loop").id;
    edit(
        s,
        LaneEdit::SetInput {
            seq,
            lane: lp,
            input: Some(LaneInputRow::Audio {
                first: 0,
                stereo: false,
                return_of: None,
            }),
        },
    )
    .unwrap();
    lp
}

fn spec() -> RecordSpecRow {
    RecordSpecRow {
        replace: true,
        punch: None,
        looped: false,
        count_in_bars: 0,
        pre_roll_bars: 0,
        passes_as_takes: true,
    }
}

/// Ruling 13 (5c carry T11 M6; Review Focus 3): arming returns while the
/// input opens on its own thread, and the transport and the meters
/// answer meanwhile.
#[test]
fn an_input_change_runs_off_the_calling_thread() {
    let _g = serial();
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = audio_lane(&s, seq);
    take_engine(&s, seq);
    let delay = Duration::from_millis(600);
    debug_input_open_delay(Some(delay));
    let t = Instant::now();
    arm(&s, seq, lp, true);
    // Pre-flight M14: the arm came back before the open could have
    // finished, and the input is still opening (no fixed time bound).
    let opening = s.input_info().unwrap().opening;
    let elapsed = t.elapsed();
    assert!(
        opening && elapsed < delay,
        "opening {opening} after {elapsed:?}"
    );
    assert_eq!(s.debug_input_open(), Some(false));
    let _ = s.transport().unwrap();
    let _ = s.levels().unwrap();
    let _ = s.input_levels().unwrap();
    // The transport and the meters answered meanwhile.
    let opening = s.input_info().unwrap().opening;
    let elapsed = t.elapsed();
    assert!(
        opening && elapsed < delay,
        "opening {opening} after {elapsed:?}"
    );
    wait_until("the input to open", || !s.input_info().unwrap().opening);
    assert_eq!(s.debug_input_open(), Some(true));
    // Disarming while it opens: the input ends closed.
    arm(&s, seq, lp, false);
    arm(&s, seq, lp, true);
    arm(&s, seq, lp, false);
    wait_until("the input to settle", || !s.input_info().unwrap().opening);
    assert_eq!(s.debug_input_open(), Some(false), "the last asked wins");
    s.close().unwrap();
}

/// Ruling 13 (Review Focus 3): Record waits for an input that is opening,
/// and is Busy (recording nothing) when it takes longer than
/// `INPUT_WAIT`.
#[test]
fn record_waits_for_an_input_that_is_opening() {
    let _g = serial();
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = audio_lane(&s, seq);
    take_engine(&s, seq);
    debug_input_open_delay(Some(Duration::from_millis(500)));
    arm(&s, seq, lp, true);
    assert!(s.input_info().unwrap().opening);
    s.record(seq, spec()).unwrap();
    assert!(s.record_state().unwrap().recording);
    assert!(!s.input_info().unwrap().opening, "Record waited for it");
    s.stop().unwrap();
    // Longer than the wait: Busy, nothing records.
    arm(&s, seq, lp, false);
    wait_until("closed", || s.debug_input_open() == Some(false));
    debug_input_open_delay(Some(vizdaw_ffi::host::INPUT_WAIT + Duration::from_secs(2)));
    arm(&s, seq, lp, true);
    let r = s.record(seq, spec());
    assert!(
        matches!(&r, Err(VizError::Busy { message }) if message.contains("still opening")),
        "{r:?}"
    );
    assert!(!s.record_state().unwrap().recording);
    debug_input_open_delay(None);
    wait_until("the input to open", || s.debug_input_open() == Some(true));
    s.close().unwrap();
}

/// Ruling 14 (5c carry T11 M2; Review Focus 3): a "no" to the first
/// microphone prompt is reported when arming, and Record refuses with it,
/// instead of a pass recording silence.
#[test]
fn a_denied_first_prompt_is_reported_before_recording() {
    let _g = serial();
    // A device whose input macOS refused offers no input channels.
    set_fake_input(None);
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = audio_lane(&s, seq);
    take_engine(&s, seq);
    set_fake_microphone_prompt(Some(false));
    arm(&s, seq, lp, true);
    wait_until("the prompt's answer", || !s.input_info().unwrap().opening);
    assert!(
        s.transport()
            .unwrap()
            .faults
            .iter()
            .any(|f| f == DENIED_MESSAGE),
        "{:?}",
        s.transport().unwrap().faults
    );
    assert_eq!(s.debug_input_open(), Some(false));
    let r = s.record(seq, spec());
    assert!(
        matches!(&r, Err(VizError::Model { message }) if message.starts_with("VizDAW may not use audio input")),
        "{r:?}"
    );
    assert!(!s.record_state().unwrap().recording);
    s.close().unwrap();
}

/// Ruling 15 (5c carry T11 M3; Review Focus 3): an input that failed to
/// open is not retried by Play, but is by arming again or choosing the
/// input again.
#[test]
fn choosing_the_input_again_retries_a_failed_open() {
    let _g = serial();
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = audio_lane(&s, seq);
    take_engine(&s, seq);
    debug_fail_input_opens(1);
    arm(&s, seq, lp, true);
    let failed = |s: &Session| {
        s.transport()
            .unwrap()
            .faults
            .iter()
            .any(|f| f.starts_with("audio input could not open"))
    };
    assert!(failed(&s));
    assert_eq!(s.debug_input_open(), Some(false));
    // Play does not try again (each try restarts the device).
    take_engine(&s, seq);
    assert!(failed(&s), "said again after Play cleared the faults");
    assert_eq!(s.debug_input_open(), Some(false));
    // Choosing the input again does.
    audio_lane(&s, seq);
    wait_until("the input to open", || s.debug_input_open() == Some(true));
    // And so does arming, after another failure.
    arm(&s, seq, lp, false);
    debug_fail_input_opens(1);
    arm(&s, seq, lp, true);
    assert_eq!(s.debug_input_open(), Some(false));
    arm(&s, seq, lp, true);
    wait_until("the input to open again", || {
        s.debug_input_open() == Some(true)
    });
    // The input device picker clears it too.
    arm(&s, seq, lp, false);
    debug_fail_input_opens(1);
    arm(&s, seq, lp, true);
    assert_eq!(s.debug_input_open(), Some(false));
    vizdaw_ffi::host::set_audio_input_device(None);
    s.idle().unwrap();
    take_engine(&s, seq);
    wait_until("the input to open a third time", || {
        s.debug_input_open() == Some(true)
    });
    s.close().unwrap();
}

/// Pre-flight I1: a host dropped while its input thread waits at the
/// first microphone prompt does not wait for the answer (the thread
/// detaches, and touches nothing of the host once it is answered).
#[test]
fn a_host_goes_without_waiting_for_a_prompt_that_is_open() {
    let _g = serial();
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = audio_lane(&s, seq);
    take_engine(&s, seq);
    set_fake_microphone_prompt(Some(true));
    debug_hold_microphone_prompt(true);
    arm(&s, seq, lp, true);
    assert!(s.input_info().unwrap().opening, "the prompt is open");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        reset_host();
        let _ = tx.send(());
    });
    let gone = rx.recv_timeout(common::wait_limit());
    debug_hold_microphone_prompt(false);
    assert!(gone.is_ok(), "reset_host waited for the prompt");
    assert_eq!(s.debug_input_open(), None, "the host is gone");
    // The answered thread hands nothing to the next host.
    take_engine(&s, seq);
    wait_until("the new host's input", || {
        s.debug_input_open() == Some(true)
    });
    s.close().unwrap();
}
