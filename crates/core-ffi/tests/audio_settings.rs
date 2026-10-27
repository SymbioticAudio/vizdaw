//! The process-wide audio settings (plan 5d-1 Rulings 8–12; sub-project 5
//! spec §6.6, §6.9): Software Monitoring takes effect at once and leaves
//! thru alone; the I/O buffer size is refused while recording; the stream
//! sizes wait for an idle engine; Add Lane takes the default monitor mode
//! in one step; the round trip is reported; hostile values change
//! nothing. Offline only: no device is opened and no microphone asked.

#![recursion_limit = "256"]

mod common;

use core_engine::offline::FakeInput;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::audio::{
    audio_settings, reset_audio_settings, set_default_monitor_mode, set_io_buffer_frames,
    set_software_monitoring, set_stream_settings,
};
use vizdaw_ffi::host::{force_offline, hosts_created, reset_host, set_fake_input};
use vizdaw_ffi::{
    EditCommand, LaneEdit, LaneInputRow, LaneRow, MonitorModeRow, RecordSpecRow, Session, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial(input: Option<FakeInput>) -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(input);
    reset_audio_settings();
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

/// Plays and stops, so the session holds the (idle) engine.
fn take_engine(s: &Session, seq: u64) {
    s.play(seq).unwrap();
    let until = Instant::now() + common::wait_limit();
    while !s.transport().unwrap().playing {
        assert!(Instant::now() < until, "never played");
        std::thread::sleep(Duration::from_millis(2));
    }
    s.stop().unwrap();
    while s.transport().unwrap().playing {
        assert!(Instant::now() < until, "never stopped");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The demo's audio lane "Loop" records fake input 1 and is monitored
/// Input; "Chords" plays thru from MIDI source "Keys", armed.
fn monitored(s: &Session, seq: u64) {
    let lp = lane(s, seq, "Loop").id;
    let chords = lane(s, seq, "Chords").id;
    for e in [
        LaneEdit::SetInput {
            seq,
            lane: lp,
            input: Some(LaneInputRow::Audio {
                first: 0,
                stereo: false,
                return_of: None,
            }),
        },
        LaneEdit::SetMonitor {
            seq,
            lane: lp,
            monitor: MonitorModeRow::Input,
        },
        LaneEdit::SetInput {
            seq,
            lane: chords,
            input: Some(LaneInputRow::Midi {
                port: Some("Keys".into()),
                channel: None,
            }),
        },
        LaneEdit::SetFlags {
            seq,
            lane: chords,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    ] {
        edit(s, e).unwrap();
    }
}

/// Spec §6.6, Ruling 8 (Review Focus 4): Software Monitoring off drops
/// the monitor routes at the holder's next idle, without a Play; MIDI
/// thru stays; an unarmed lane monitored Input no longer needs the input.
#[test]
fn software_monitoring_off_drops_monitoring_at_once_but_keeps_thru() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    monitored(&s, seq);
    take_engine(&s, seq);
    let inputs = s.debug_engine_inputs().unwrap();
    assert_eq!((inputs.monitor.len(), inputs.midi.len()), (1, 1));
    assert_eq!(s.debug_input_open(), Some(true));
    set_software_monitoring(false);
    assert!(!audio_settings().software_monitoring);
    s.idle().unwrap();
    wait_until("the monitor routes to go", || {
        s.debug_engine_inputs().unwrap().monitor.is_empty()
    });
    let inputs = s.debug_engine_inputs().unwrap();
    assert_eq!(inputs.midi.len(), 1, "thru stays");
    assert!(inputs.midi[0].thru);
    assert_eq!(s.debug_input_open(), Some(false), "nothing needs input");
    set_software_monitoring(true);
    s.idle().unwrap();
    wait_until("the monitor routes to come back", || {
        s.debug_engine_inputs().unwrap().monitor.len() == 1
    });
    s.close().unwrap();
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

/// Spec §6.6, Ruling 9 (Review Focus 4): the I/O buffer size is one of
/// six, and refused while recording.
#[test]
fn the_buffer_size_is_refused_while_recording() {
    let _g = serial(Some(FakeInput::Signals));
    assert_eq!(audio_settings().buffer_frames, 128, "the default");
    set_io_buffer_frames(256).unwrap();
    assert_eq!(audio_settings().buffer_frames, 256);
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = lane(&s, seq, "Loop").id;
    edit(
        &s,
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
    edit(
        &s,
        LaneEdit::SetFlags {
            seq,
            lane: lp,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    )
    .unwrap();
    s.record(seq, spec()).unwrap();
    let r = set_io_buffer_frames(512);
    assert!(
        matches!(&r, Err(VizError::Busy { message }) if message.contains("Stop recording")),
        "{r:?}"
    );
    assert_eq!(audio_settings().buffer_frames, 256, "unchanged");
    s.stop().unwrap();
    set_io_buffer_frames(512).unwrap();
    assert_eq!(audio_settings().buffer_frames, 512);
    s.close().unwrap();
}

/// Ruling 10 (Review Focus 4): new stream sizes drop an idle engine (the
/// next Play makes one with them) and leave a playing one alone.
#[test]
fn stream_settings_wait_for_an_idle_engine() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    take_engine(&s, seq);
    let made = hosts_created();
    set_stream_settings(256, 4).unwrap();
    let a = audio_settings();
    assert_eq!((a.cache_mb, a.read_ahead_seconds), (256, 4));
    assert_eq!(vizdaw_ffi::host::holder(), None, "the idle engine went");
    take_engine(&s, seq);
    assert_eq!(hosts_created(), made + 1, "the next Play made one");
    s.play(seq).unwrap();
    wait_until("playing", || s.transport().unwrap().playing);
    set_stream_settings(512, 3).unwrap();
    assert!(vizdaw_ffi::host::holder().is_some(), "the engine stays");
    assert!(s.transport().unwrap().playing, "playing goes on");
    s.stop().unwrap();
    s.close().unwrap();
}

/// Ruling 11: Add Lane gives a new lane the default monitor mode in its
/// one undo step "Add Lane"; Auto is a plain add.
#[test]
fn added_lanes_take_the_default_monitor_mode_in_one_step() {
    let _g = serial(None);
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    set_default_monitor_mode(MonitorModeRow::Off);
    assert_eq!(audio_settings().default_monitor, MonitorModeRow::Off);
    let before = s.lanes(seq).unwrap().len();
    edit(
        &s,
        LaneEdit::Add {
            seq,
            name: "New".into(),
            index: None,
        },
    )
    .unwrap();
    assert_eq!(lane(&s, seq, "New").monitor, MonitorModeRow::Off);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Add Lane")
    );
    s.undo().unwrap();
    assert_eq!(s.lanes(seq).unwrap().len(), before, "one step");
    set_default_monitor_mode(MonitorModeRow::Auto);
    edit(
        &s,
        LaneEdit::Add {
            seq,
            name: "Plain".into(),
            index: None,
        },
    )
    .unwrap();
    assert_eq!(lane(&s, seq, "Plain").monitor, MonitorModeRow::Auto);
    s.close().unwrap();
}

/// Ruling 9: the round trip is both latencies and a buffer each way (the
/// offline device's buffer is its 512-frame block).
#[test]
fn the_round_trip_is_both_latencies_and_two_buffers() {
    let _g = serial(Some(FakeInput::Loopback(1024)));
    let s = Session::new_demo().unwrap();
    take_engine(&s, song(&s));
    let i = s.input_info().unwrap();
    assert_eq!(i.buffer_frames, 512);
    assert_eq!(i.round_trip_frames, 512 + 512 + 2 * 512);
    s.close().unwrap();
}

/// Review Focus 4: hostile values are `Model` and change nothing.
#[test]
fn hostile_audio_settings_are_refused() {
    let _g = serial(None);
    let before = audio_settings();
    for frames in [0, 1, 31, 33, 100, 127, 2048, u32::MAX] {
        assert!(
            matches!(set_io_buffer_frames(frames), Err(VizError::Model { .. })),
            "{frames}"
        );
    }
    for (mb, s) in [
        (0, 3),
        (63, 3),
        (8193, 3),
        (u32::MAX, 3),
        (512, 0),
        (512, 1),
        (512, 9),
    ] {
        assert!(
            matches!(set_stream_settings(mb, s), Err(VizError::Model { .. })),
            "{mb} MiB, {s} s"
        );
    }
    assert_eq!(audio_settings(), before);
}

/// Pre-flight I2: new stream sizes leave alone an engine that only
/// monitors (an armed lane's input and route): no Play, no re-arm, and
/// the monitoring goes on.
#[test]
fn stream_settings_leave_a_monitoring_engine_alone() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = lane(&s, seq, "Loop").id;
    edit(
        &s,
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
    edit(
        &s,
        LaneEdit::SetFlags {
            seq,
            lane: lp,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    )
    .unwrap();
    take_engine(&s, seq);
    wait_until("the armed lane to monitor", || {
        s.debug_input_open() == Some(true) && s.debug_engine_inputs().unwrap().monitor.len() == 1
    });
    set_stream_settings(256, 4).unwrap();
    assert!(vizdaw_ffi::host::holder().is_some(), "the engine stays");
    s.idle().unwrap();
    wait_until("the monitoring to go on", || {
        s.debug_input_open() == Some(true) && s.debug_engine_inputs().unwrap().monitor.len() == 1
    });
    let a = audio_settings();
    assert_eq!((a.cache_mb, a.read_ahead_seconds), (256, 4));
    s.close().unwrap();
}
