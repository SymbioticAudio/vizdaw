//! Arming claims an idle engine (plan 5d-1 Rulings 16–17; 5c carries I2
//! and T12 M5): with the transport stopped and no Play yet, an armed or
//! monitored lane is heard and MIDI thru plays; a playing, recording or
//! measuring engine is never taken; a MIDI lane's arm reconnects the MIDI
//! sources. Offline only: no device is opened and no microphone asked.

#![recursion_limit = "256"]

mod common;

use core_engine::offline::FakeInput;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{
    debug_pretend_live_midi, drain_offline_log, force_offline, holder, midi_source_connects,
    reset_host, set_fake_input,
};
use vizdaw_ffi::{EditCommand, LaneEdit, LaneInputRow, LaneRow, MonitorModeRow, Session, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(Some(FakeInput::Signals));
    debug_pretend_live_midi(false);
    vizdaw_ffi::host::set_midi_connect_failure(None);
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

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The demo's "Chords" listens to MIDI source "Keys"; "Loop" (demo audio)
/// records fake input 1.
fn inputs(s: &Session, seq: u64, audio: bool) -> u64 {
    let (name, input) = if audio {
        (
            "Loop",
            LaneInputRow::Audio {
                first: 0,
                stereo: false,
                return_of: None,
            },
        )
    } else {
        (
            "Chords",
            LaneInputRow::Midi {
                port: Some("Keys".into()),
                channel: None,
            },
        )
    };
    let id = lane(s, seq, name).id;
    edit(
        s,
        LaneEdit::SetInput {
            seq,
            lane: id,
            input: Some(input),
        },
    )
    .unwrap();
    id
}

/// Ruling 16 (Review Focus 2): before any Play, arming an audio lane
/// claims the engine and opens its input (monitored Auto, armed, stopped:
/// heard); arming a MIDI lane makes thru play.
#[test]
fn arming_claims_an_idle_engine_and_hears_the_input() {
    let _g = serial();
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    assert_eq!(holder(), None);
    let lp = inputs(&s, seq, true);
    assert_eq!(holder(), None, "an input alone wants nothing");
    arm(&s, seq, lp, true);
    wait_until("the input to open", || s.debug_input_open() == Some(true));
    let routes = s.debug_engine_inputs().unwrap();
    assert_eq!(routes.monitor.len(), 1, "monitored Auto, armed");
    assert!(!s.transport().unwrap().playing, "nothing played");
    assert_eq!(s.input_info().unwrap().channels, 3);
    s.close().unwrap();
    // MIDI thru with no Play.
    reset_host();
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = inputs(&s, seq, false);
    arm(&s, seq, chords, true);
    std::thread::sleep(Duration::from_millis(50));
    drain_offline_log();
    assert!(s.inject_midi("Keys".into(), vec![0x90, 70, 99]).unwrap());
    let mut log = Vec::new();
    wait_until("thru", || {
        log.extend(drain_offline_log());
        log.iter()
            .any(|(_, b): &(u64, Vec<u8>)| b == &vec![0x90, 70, 99])
    });
    s.close().unwrap();
}

/// Ruling 16 (Review Focus 2): a playing engine stays with its window;
/// once it stands idle, arming in the other window takes it. A monitor
/// edit claims too.
#[test]
fn arming_never_takes_a_busy_engine() {
    let _g = serial();
    let a = Session::new_demo().unwrap();
    let b = Session::new_demo_audio().unwrap();
    let (sa, sb) = (song(&a), song(&b));
    a.play(sa).unwrap();
    wait_until("a plays", || a.transport().unwrap().playing);
    let owner = holder();
    let lp = inputs(&b, sb, true);
    arm(&b, sb, lp, true);
    assert_eq!(holder(), owner, "a playing engine is not taken");
    assert!(a.transport().unwrap().playing, "a plays on");
    assert!(b.debug_engine_inputs().is_none());
    a.stop().unwrap();
    wait_until("a stops", || !a.transport().unwrap().playing);
    edit(
        &b,
        LaneEdit::SetMonitor {
            seq: sb,
            lane: lp,
            monitor: MonitorModeRow::Input,
        },
    )
    .unwrap();
    wait_until("b's input to open", || b.debug_input_open() == Some(true));
    assert_ne!(holder(), owner, "an idle engine is taken");
    a.close().unwrap();
    b.close().unwrap();
}

/// Ruling 17 (5c carry T12 M5; Review Focus 2): arming a MIDI lane
/// reconnects the MIDI sources, so a keyboard plugged in after launch is
/// heard (offline hosts connect nothing real; the hook counts the tries).
#[test]
fn arming_a_midi_lane_listens_to_new_sources() {
    let _g = serial();
    debug_pretend_live_midi(true);
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = inputs(&s, seq, false);
    let before = midi_source_connects();
    arm(&s, seq, chords, true);
    wait_until("a connect", || midi_source_connects() > before);
    let once = midi_source_connects();
    arm(&s, seq, chords, false);
    arm(&s, seq, chords, true);
    wait_until("another connect", || midi_source_connects() > once);
    debug_pretend_live_midi(false);
    s.close().unwrap();
}
