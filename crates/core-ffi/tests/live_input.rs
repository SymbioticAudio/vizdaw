//! Live input through a session (plan 5c Task 8; sub-project 5 spec
//! §4.5, §6.6, §12.1): lane inputs and monitor modes as undoable lane
//! edits shown in the lane rows; MIDI injected as if a source sent it
//! plays thru to an armed lane's destination; hostile injections are
//! refused; the offline device's fake input is reported and a monitored
//! input is heard through its path's strip. The engine host is
//! process-wide: one test at a time, all offline (no audio device, no
//! microphone permission).

#![recursion_limit = "256"]

mod common;

use core_engine::offline::FakeInput;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{drain_offline_log, force_offline, reset_host, set_fake_input};
use vizdaw_ffi::{
    EditCommand, LaneEdit, LaneInputRow, LaneRow, MonitorModeRow, Session, StripKindRow, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial(input: Option<FakeInput>) -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(input);
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

fn arm(s: &Session, seq: u64, lane: u64) {
    edit(
        s,
        LaneEdit::SetFlags {
            seq,
            lane,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    )
    .unwrap();
}

/// Plays and stops, so the session holds the engine.
fn take_engine(s: &Session, seq: u64) {
    s.play(seq).unwrap();
    let until = Instant::now() + common::wait_limit();
    while !s.transport().unwrap().playing {
        assert!(Instant::now() < until, "never played");
        std::thread::sleep(Duration::from_millis(2));
    }
    s.stop().unwrap();
}

#[test]
fn inputs_and_monitor_modes_are_lane_edits() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords");
    assert_eq!(
        (chords.input.clone(), chords.monitor),
        (None, MonitorModeRow::Auto)
    );
    let midi = LaneInputRow::Midi {
        port: Some("Keys".into()),
        channel: None,
    };
    edit(
        &s,
        LaneEdit::SetInput {
            seq,
            lane: chords.id,
            input: Some(midi.clone()),
        },
    )
    .unwrap();
    edit(
        &s,
        LaneEdit::SetMonitor {
            seq,
            lane: chords.id,
            monitor: MonitorModeRow::Off,
        },
    )
    .unwrap();
    let row = lane(&s, seq, "Chords");
    assert_eq!((row.input, row.monitor), (Some(midi), MonitorModeRow::Off));
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Lane Monitor")
    );
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(lane(&s, seq, "Chords").input, None);
    // Out-of-range channels and MIDI channels are refused, nothing changes.
    for bad in [
        LaneInputRow::Audio {
            first: 300,
            stereo: false,
        },
        LaneInputRow::Audio {
            first: 255,
            stereo: true,
        },
        LaneInputRow::Midi {
            port: None,
            channel: Some(16),
        },
        LaneInputRow::Midi {
            port: Some(String::new()),
            channel: None,
        },
    ] {
        let r = edit(
            &s,
            LaneEdit::SetInput {
                seq,
                lane: chords.id,
                input: Some(bad.clone()),
            },
        );
        assert!(matches!(r, Err(VizError::Model { .. })), "{bad:?}: {r:?}");
    }
    assert_eq!(lane(&s, seq, "Chords").input, None);
    s.close().unwrap();
}

#[test]
fn injected_midi_plays_thru_to_an_armed_lanes_destination() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords").id;
    // No engine yet: nothing to send to.
    assert!(!s.inject_midi("Keys".into(), vec![0x90, 70, 99]).unwrap());
    edit(
        &s,
        LaneEdit::SetInput {
            seq,
            lane: chords,
            input: Some(LaneInputRow::Midi {
                port: Some("Keys".into()),
                channel: None,
            }),
        },
    )
    .unwrap();
    arm(&s, seq, chords);
    take_engine(&s, seq);
    std::thread::sleep(Duration::from_millis(50));
    drain_offline_log();
    // Another source: not the lane's.
    assert!(s.inject_midi("Pads".into(), vec![0x90, 71, 99]).unwrap());
    // Played on channel 2: thru plays it on the piano's channel 1.
    assert!(
        s.inject_midi("Keys".into(), vec![0x91, 70, 99, 0x81, 70, 0])
            .unwrap()
    );
    let until = Instant::now() + common::wait_limit();
    let mut log = Vec::new();
    while !log
        .iter()
        .any(|(_, b): &(u64, Vec<u8>)| b == &vec![0x80, 70, 0])
    {
        assert!(Instant::now() < until, "thru never arrived: {log:?}");
        std::thread::sleep(Duration::from_millis(5));
        log.extend(drain_offline_log());
    }
    assert!(log.iter().any(|(_, b)| b == &vec![0x90, 70, 99]), "{log:?}");
    assert!(!log.iter().any(|(_, b)| b.get(1) == Some(&71)), "{log:?}");
    s.close().unwrap();
}

#[test]
fn hostile_injections_are_refused() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    take_engine(&s, song(&s));
    let long = vec![0x90u8; 1025];
    for (port, bytes) in [
        ("Keys", vec![]),
        ("Keys", long),
        ("Keys", vec![0xF0, 0x7E, 0xF7]),
        ("Keys", vec![0xF8]),
        ("Keys", vec![0x90, 60]),
        ("Keys", vec![0x90, 60, 200]),
        ("Keys", vec![60, 100]),
        ("", vec![0x90, 60, 100]),
        ("a\u{0}b", vec![0x90, 60, 100]),
    ] {
        let r = s.inject_midi(port.into(), bytes.clone());
        assert!(
            matches!(r, Err(VizError::Model { .. })),
            "{port:?} {bytes:?}: {r:?}"
        );
    }
    s.close().unwrap();
}

#[test]
fn the_fake_device_reports_its_input() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    assert_eq!(s.input_info().unwrap().channels, 0, "no engine yet");
    take_engine(&s, song(&s));
    let i = s.input_info().unwrap();
    assert_eq!(i.device, "Offline");
    assert_eq!(i.channels, 3);
    assert_eq!(i.names, vec!["Fake In 1", "Fake In 2", "Fake In 3"]);
    assert_eq!((i.input_latency_frames, i.output_latency_frames), (0, 0));
    s.close().unwrap();
    // A loopback device splits its delay between the directions.
    set_fake_input(Some(FakeInput::Loopback(1024)));
    reset_host();
    let s = Session::new_demo().unwrap();
    take_engine(&s, song(&s));
    let i = s.input_info().unwrap();
    assert_eq!(
        (i.input_latency_frames, i.output_latency_frames),
        (512, 512)
    );
    s.close().unwrap();
    // No fake input: none.
    set_fake_input(None);
    reset_host();
    let s = Session::new_demo().unwrap();
    take_engine(&s, song(&s));
    assert_eq!(s.input_info().unwrap().channels, 0);
    s.close().unwrap();
}

/// The audio path strip's peak over about 100 ms.
fn path_peak(s: &Session, strip: u64) -> f32 {
    let _ = s.levels().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    s.levels()
        .unwrap()
        .meters
        .iter()
        .filter(|m| m.strip == strip)
        .map(|m| m.peak_l.max(m.peak_r))
        .fold(0.0, f32::max)
}

#[test]
fn a_monitored_input_is_heard_through_its_path() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = lane(&s, seq, "Loop").id;
    let strip = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| matches!(x.kind, StripKindRow::AudioPath { .. }))
        .unwrap()
        .id;
    edit(
        &s,
        LaneEdit::SetInput {
            seq,
            lane: lp,
            input: Some(LaneInputRow::Audio {
                first: 0,
                stereo: false,
            }),
        },
    )
    .unwrap();
    edit(
        &s,
        LaneEdit::SetMonitor {
            seq,
            lane: lp,
            monitor: MonitorModeRow::Input,
        },
    )
    .unwrap();
    take_engine(&s, seq);
    // The fake sine (−12 dBFS) through the strip.
    let until = Instant::now() + common::wait_limit();
    while path_peak(&s, strip) < 0.05 {
        assert!(Instant::now() < until, "the input was never heard");
    }
    edit(
        &s,
        LaneEdit::SetMonitor {
            seq,
            lane: lp,
            monitor: MonitorModeRow::Off,
        },
    )
    .unwrap();
    let until = Instant::now() + common::wait_limit();
    while path_peak(&s, strip) > 0.001 {
        assert!(Instant::now() < until, "Off still heard the input");
    }
    s.close().unwrap();
}

/// Spec §6.1: audio input is wanted only while an audio lane is armed or
/// monitors its input.
#[test]
fn audio_input_is_wanted_only_for_armed_or_monitored_audio_lanes() {
    use core_model::command::{Command, LaneCmd, apply};
    use core_model::lane::{InputChannels, LaneInput, MonitorMode};
    use vizdaw_ffi::host::wants_audio_input;
    let doc = core_engine::fixtures::demo_document().unwrap();
    let song = core_engine::engine::find_sequence(&doc, "Song").unwrap();
    let lane = doc.sequence(song).unwrap().lanes[0].id;
    assert!(!wants_audio_input(&doc, song));
    let run =
        |d: &core_model::document::Document, c: LaneCmd| apply(d, &Command::Lane(c)).unwrap().doc;
    let audio = run(
        &doc,
        LaneCmd::SetInput {
            seq: song,
            lane,
            input: Some(LaneInput::Audio {
                channels: InputChannels::Mono(0),
            }),
        },
    );
    assert!(!wants_audio_input(&audio, song), "an input alone");
    let monitored = run(
        &audio,
        LaneCmd::SetMonitor {
            seq: song,
            lane,
            monitor: MonitorMode::Input,
        },
    );
    assert!(wants_audio_input(&monitored, song));
    let armed = run(
        &audio,
        LaneCmd::SetFlags {
            seq: song,
            lane,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    );
    assert!(wants_audio_input(&armed, song));
    let midi = run(
        &armed,
        LaneCmd::SetInput {
            seq: song,
            lane,
            input: Some(LaneInput::Midi {
                port: None,
                channel: None,
            }),
        },
    );
    assert!(!wants_audio_input(&midi, song), "MIDI needs no audio input");
}

/// Plan 5c Task 12: what CoreMIDI sources send reaches the newest
/// engine, filtered by the lane's source.
#[test]
fn source_messages_reach_the_engine() {
    use vizdaw_ffi::host::debug_midi_from_source;
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords").id;
    edit(
        &s,
        LaneEdit::SetInput {
            seq,
            lane: chords,
            input: Some(LaneInputRow::Midi {
                port: Some("Keystation".into()),
                channel: Some(0),
            }),
        },
    )
    .unwrap();
    arm(&s, seq, chords);
    take_engine(&s, seq);
    std::thread::sleep(Duration::from_millis(50));
    drain_offline_log();
    // Another channel, another source: not this lane's; a SysEx: dropped.
    debug_midi_from_source("Keystation", 0, &[0x91, 72, 90]);
    debug_midi_from_source("Other", 0, &[0x90, 72, 90]);
    debug_midi_from_source("Keystation", 0, &[0xF0, 1, 0xF7]);
    debug_midi_from_source("Keystation", 0, &[0x90, 74, 90]);
    let until = Instant::now() + common::wait_limit();
    let mut log = Vec::new();
    while !log
        .iter()
        .any(|(_, b): &(u64, Vec<u8>)| b == &vec![0x90, 74, 90])
    {
        assert!(Instant::now() < until, "never arrived: {log:?}");
        std::thread::sleep(Duration::from_millis(5));
        log.extend(drain_offline_log());
    }
    assert!(!log.iter().any(|(_, b)| b.get(1) == Some(&72)), "{log:?}");
    s.close().unwrap();
}

#[test]
fn lane_edits_reach_the_engines_inputs_at_load() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords").id;
    take_engine(&s, seq);
    assert_eq!(s.debug_engine_inputs().unwrap().midi.len(), 0);
    edit(
        &s,
        LaneEdit::SetInput {
            seq,
            lane: chords,
            input: Some(LaneInputRow::Midi {
                port: Some("Keys".into()),
                channel: Some(3),
            }),
        },
    )
    .unwrap();
    // Not armed, monitor Auto: not listening yet.
    assert_eq!(s.debug_engine_inputs().unwrap().midi.len(), 0);
    arm(&s, seq, chords);
    let m = s.debug_engine_inputs().unwrap().midi;
    assert_eq!(m.len(), 1);
    assert_eq!((m[0].channel, m[0].thru, m[0].armed), (Some(3), true, true));
    edit(
        &s,
        LaneEdit::SetMonitor {
            seq,
            lane: chords,
            monitor: MonitorModeRow::Off,
        },
    )
    .unwrap();
    let m = s.debug_engine_inputs().unwrap().midi;
    assert_eq!(
        (m.len(), m[0].thru),
        (1, false),
        "armed, monitor Off: records, no thru"
    );
    s.close().unwrap();
}
