//! Recording through a session (plan 5c Task 9; sub-project 5 spec
//! §6.2–§6.5, §12.3): audio from the fake input lands as one undo step
//! "Record" in the project format; the loopback device lands a click
//! within two frames of where it played; loop passes become takes with
//! keep and discard; MIDI recorded in a punch replaces only that range;
//! edits made while recording join the step; a full disk, a lost device
//! and Stop end a pass cleanly and keep what was recorded; a save while
//! recording never takes the file being written. Offline only: no audio
//! device is opened and no microphone permission asked.

#![recursion_limit = "256"]

mod common;

use core_engine::fixtures::{Builder, Q};
use core_engine::offline::FakeInput;
use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::format::BitDepth;
use core_model::ids::{AnyId, LaneId, SeqId};
use core_model::lane::{InputChannels, LaneInput, MonitorMode};
use core_model::sequence::SeqLength;
use core_model::time::Tick;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host, set_fake_input};
use vizdaw_ffi::record::set_free_bytes;
use vizdaw_ffi::{
    EventKindRow, EventRow, MediaOriginRow, RecordSpecRow, Session, TickRange, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial(input: Option<FakeInput>) -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(input);
    set_free_bytes(None);
    reset_host();
    g
}

/// 120 BPM at 48 kHz: one beat (Q ticks) is 24 000 frames.
const BEAT: i64 = 24_000;

struct Doc {
    doc: Document,
    seq: SeqId,
    keys: LaneId,
    mic: LaneId,
    src: LaneId,
}

/// Sequence "S" (`bars` bars locked): lane "Keys" on the built-in synth
/// with notes at beats 1 and 5, MIDI input from "Keys", armed; lane "Mic"
/// recording fake input 1, armed, monitor Off; lane "Src" (an output
/// path, for regions); one output "Audio 1".
fn doc(bars: i64, arm_keys: bool, arm_mic: bool) -> Doc {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("S", "Keys", Some(synth)).unwrap();
    b.notes(
        seq,
        keys,
        &[(Q, 40, 0x8000, Q / 2), (5 * Q, 41, 0x8000, Q / 2)],
    )
    .unwrap();
    let mic = b.lane(seq, "Mic", None).unwrap();
    let src = b.lane(seq, "Src", None).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: "Audio 1".into(),
            reference: "audio:1-2".into(),
        }))
        .unwrap();
    let out = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .unwrap();
    for lane in [mic, src] {
        b.run(Command::Lane(LaneCmd::SetOutput {
            seq,
            lane,
            output: Some(out),
        }))
        .unwrap();
    }
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: keys,
        input: Some(LaneInput::Midi {
            port: Some("Keys".into()),
            channel: None,
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: mic,
        input: Some(LaneInput::Audio {
            channels: InputChannels::Mono(0),
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetMonitor {
        seq,
        lane: mic,
        monitor: MonitorMode::Off,
    }))
    .unwrap();
    for (lane, on) in [(keys, arm_keys), (mic, arm_mic)] {
        b.run(Command::Lane(LaneCmd::SetFlags {
            seq,
            lane,
            mute: None,
            solo: None,
            arm: Some(on),
        }))
        .unwrap();
    }
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(bars * 4 * Q)),
    }))
    .unwrap();
    Doc {
        doc: b.finish(),
        seq,
        keys,
        mic,
        src,
    }
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

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn frames(s: &Session) -> u64 {
    s.record_state().unwrap().frames
}

fn regions(s: &Session, seq: u64, lane: u64) -> Vec<EventRow> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .into_iter()
        .filter(|e| e.media.is_some())
        .collect()
}

fn read(path: &str) -> (core_media::caf::CafInfo, Vec<f32>) {
    let r = CafReader::open(Path::new(path)).unwrap();
    let info = r.info().clone();
    let mut out = vec![0.0; info.frames as usize * usize::from(info.channels)];
    r.read(0, &mut out, &mut Vec::new()).unwrap();
    (info, out)
}

#[test]
fn recording_the_fake_input_is_one_undo_step_in_the_project_format() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    s.locate(4 * Q).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    let st = s.record_state().unwrap();
    assert!(st.recording);
    assert_eq!(st.lanes.len(), 1);
    assert!(st.lanes[0].audio);
    assert!(
        st.lanes[0]
            .path
            .as_deref()
            .unwrap()
            .starts_with("Media/Recorded/Mic ")
    );
    wait_until("a second of audio", || frames(&s) >= 48_000);
    s.stop().unwrap();
    assert!(!s.record_state().unwrap().recording);
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Record"));
    let media = s.media().unwrap();
    assert_eq!(media.len(), 1);
    let m = &media[0];
    assert_eq!(m.origin, MediaOriginRow::Recorded);
    assert_eq!((m.sample_rate, m.channels, m.offline), (48_000, 1, None));
    let r = regions(&s, d.seq.0, d.mic.0);
    assert_eq!(r.len(), 1);
    // Where recording started, from the file's first frame (no latency).
    assert_eq!((r[0].tick, r[0].source_offset), (4 * Q, 0));
    assert_eq!(r[0].frames, m.frames);
    // The sine at −12 dBFS, 24-bit as the project is.
    let (info, data) = read(&s.media_file(m.id).unwrap());
    assert_eq!(info.depth, BitDepth::Int24);
    let peak = data.iter().fold(0.0f32, |a, x| a.max(x.abs()));
    assert!((peak - 0.2512).abs() < 0.01, "{peak}");
    s.undo().unwrap();
    assert!(s.media().unwrap().is_empty());
    assert!(regions(&s, d.seq.0, d.mic.0).is_empty());
    s.redo().unwrap();
    assert_eq!(regions(&s, d.seq.0, d.mic.0).len(), 1);
    s.close().unwrap();
}

#[test]
fn files_follow_the_project_format() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let mut doc = d.doc.clone();
    doc = core_model::command::apply(
        &doc,
        &Command::Settings(core_model::command::SettingsCmd::SetFormat {
            format: core_model::format::AudioFormat {
                sample_rate: 44_100,
                depth: BitDepth::Int16,
            },
            swaps: Vec::new(),
        }),
    )
    .unwrap()
    .doc;
    let s = Session::from_document(doc).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4410);
    s.stop().unwrap();
    let m = &s.media().unwrap()[0];
    let (info, _) = read(&s.media_file(m.id).unwrap());
    assert_eq!((info.sample_rate, info.depth), (44_100, BitDepth::Int16));
    assert_eq!(m.sample_rate, 44_100);
    s.close().unwrap();
}

/// A mono CAF of `frames` frames, silent but for 8 frames of 0.9 at `at`.
fn click(path: &Path, frames: usize, at: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut v = vec![0.0f32; frames];
    for x in v.iter_mut().skip(at).take(8) {
        *x = 0.9;
    }
    let mut w = CafWriter::create(path, 48_000, 1, BitDepth::Int24, WriteMode::Normal).unwrap();
    w.write(&v).unwrap();
    w.finish().unwrap();
}

#[test]
fn a_looped_back_click_lands_within_two_frames_of_where_it_played() {
    let _g = serial(Some(FakeInput::Loopback(1000)));
    let d = doc(8, false, true);
    // A region on "Src" from beat 1 whose click is 12 000 frames in: it
    // plays at timeline frame 36 000.
    let mut b = Builder::from_doc(d.doc.clone());
    let media = b.media("Media/click.caf", 48_000, 1).unwrap();
    b.region(
        d.seq,
        d.src,
        Q,
        media,
        0,
        48_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    click(Path::new(&s.media_file(media.0).unwrap()), 48_000, 12_000);
    s.recheck_media().unwrap();
    assert_eq!(s.media().unwrap()[0].offline, None);
    s.record(d.seq.0, spec()).unwrap();
    wait_until("the click to come back", || frames(&s) >= 48_000 + 1000);
    s.stop().unwrap();
    let r = regions(&s, d.seq.0, d.mic.0);
    assert_eq!(r.len(), 1, "{r:?}");
    // The device reported 1000 frames of round trip: they come off.
    assert_eq!(r[0].source_offset, 1000);
    let (_, data) = read(&s.media_file(r[0].media.unwrap()).unwrap());
    let k = data.iter().position(|x| x.abs() > 0.2).unwrap() as i64;
    let at = r[0].tick * BEAT / Q + (k - r[0].source_offset as i64);
    assert!((at - 36_000).abs() <= 2, "the click landed at {at}");
    s.close().unwrap();
}

/// Takes of `lane` as `(name, region source offsets)`.
fn takes(s: &Session, seq: u64, lane: u64) -> (u32, String) {
    let row = s
        .lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.id == lane)
        .unwrap();
    (row.take_count, row.active_take)
}

#[test]
fn loop_passes_with_keep_and_discard_become_takes() {
    let _g = serial(Some(FakeInput::Signals));
    // One bar at 120 BPM: a pass every two seconds (pre-flight M14: wide
    // enough that a keep or discard lands in the pass it was meant for).
    let d = doc(1, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    s.record(
        d.seq.0,
        RecordSpecRow {
            looped: true,
            ..spec()
        },
    )
    .unwrap();
    assert!(s.keep_pass().is_ok(), "nothing completed yet: no effect");
    wait_until("pass 2", || s.record_state().unwrap().pass >= 2);
    s.keep_pass().unwrap();
    wait_until("pass 3", || s.record_state().unwrap().pass >= 3);
    s.discard_pass().unwrap();
    let st = s.record_state().unwrap();
    assert_eq!((st.kept, st.discarded), (1, 1));
    // More than a beat into pass 3: a shorter last pass is no take (plan
    // 5c Task 7, pre-flight M3).
    // Counted in run frames (the transport's tick can still read the end
    // of pass 2 right after the wrap): two passes of 4 beats, then 2.
    wait_until("two beats into pass 3", || frames(&s) as i64 >= 10 * BEAT);
    s.stop().unwrap();
    // Pass 1 (kept) and pass 3 (playing at Stop) became takes; pass 2 went.
    assert_eq!(takes(&s, d.seq.0, d.mic.0), (3, "Take 3".into()));
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    s.undo().unwrap();
    assert_eq!(takes(&s, d.seq.0, d.mic.0), (1, "Take 1".into()));
    s.close().unwrap();
}

fn notes(s: &Session, seq: u64, lane: u64) -> Vec<(i64, u8)> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .filter(|e| e.kind == EventKindRow::Note)
        .map(|e| (e.tick, e.pitch))
        .collect()
}

#[test]
fn midi_recorded_in_a_punch_replaces_only_that_range() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, true, false);
    let s = Session::from_document(d.doc.clone()).unwrap();
    // Punch beats 4–8: the old note at beat 5 goes, the one at beat 1 stays.
    s.record(
        d.seq.0,
        RecordSpecRow {
            punch: Some(TickRange {
                start: 4 * Q,
                end: 8 * Q,
            }),
            ..spec()
        },
    )
    .unwrap();
    // Before the punch: not recorded.
    wait_until("beat 1", || s.transport().unwrap().tick >= Q);
    assert!(s.inject_midi("Keys".into(), vec![0x90, 60, 100]).unwrap());
    assert!(s.inject_midi("Keys".into(), vec![0x80, 60, 0]).unwrap());
    wait_until("the punch", || s.transport().unwrap().tick >= 5 * Q);
    assert!(s.inject_midi("Keys".into(), vec![0x90, 64, 100]).unwrap());
    wait_until("a beat on", || s.transport().unwrap().tick >= 6 * Q);
    assert!(s.inject_midi("Keys".into(), vec![0x80, 64, 0]).unwrap());
    // Another source: not this lane's.
    assert!(s.inject_midi("Pads".into(), vec![0x90, 65, 100]).unwrap());
    wait_until("the punch out", || {
        s.transport().unwrap().tick >= 8 * Q + Q / 2
    });
    s.stop().unwrap();
    let got = notes(&s, d.seq.0, d.keys.0);
    let pitches: Vec<u8> = got.iter().map(|(_, p)| *p).collect();
    assert_eq!(pitches, vec![40, 64], "{got:?}");
    // Played after beat 5, within a block or two.
    let t = got[1].0;
    assert!((5 * Q..6 * Q).contains(&t), "{t}");
    s.close().unwrap();
}

#[test]
fn edits_made_while_recording_join_the_record_step() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    let before = s.lanes(d.seq.0).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    s.rename_sequence(d.seq.0, "Renamed".into()).unwrap();
    // Undoable on its own while recording…
    s.undo().unwrap();
    s.redo().unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.stop().unwrap();
    // …and then one step with the recording.
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    s.undo().unwrap();
    assert_eq!(s.snapshot().unwrap().sequences[0].name, "S");
    assert_eq!(s.lanes(d.seq.0).unwrap(), before);
    assert!(s.snapshot().unwrap().undo_label.is_none());
    s.close().unwrap();
}

#[test]
fn bad_requests_are_refused_and_recording_holds_the_transport() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, false);
    let s = Session::from_document(d.doc.clone()).unwrap();
    let model = |r: Result<(), VizError>| matches!(r, Err(VizError::Model { .. }));
    assert!(model(s.record(d.seq.0, spec())), "nothing armed");
    assert!(model(s.record(999, spec())), "no such sequence");
    s.close().unwrap();
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    for bad in [
        RecordSpecRow {
            count_in_bars: 1,
            ..spec()
        },
        RecordSpecRow {
            count_in_bars: 5,
            ..spec()
        },
        RecordSpecRow {
            pre_roll_bars: 9,
            ..spec()
        },
        RecordSpecRow {
            punch: Some(TickRange { start: Q, end: Q }),
            ..spec()
        },
        RecordSpecRow {
            punch: Some(TickRange {
                start: 0,
                end: i64::MAX,
            }),
            ..spec()
        },
    ] {
        assert!(model(s.record(d.seq.0, bad.clone())), "{bad:?}");
    }
    assert!(model(s.keep_pass()), "nothing is recording");
    s.record(d.seq.0, spec()).unwrap();
    for r in [
        s.record(d.seq.0, spec()),
        s.locate(0),
        s.play(d.seq.0),
        s.set_loop(true),
        s.save_as("/tmp/never.vizdaw".into()),
    ] {
        assert!(matches!(r, Err(VizError::Busy { .. })), "{r:?}");
    }
    assert!(model(s.keep_pass()), "not loop recording");
    s.stop().unwrap();
    s.close().unwrap();
    // An input channel the device does not have.
    let mut doc = d.doc.clone();
    doc = core_model::command::apply(
        &doc,
        &Command::Lane(LaneCmd::SetInput {
            seq: d.seq,
            lane: d.mic,
            input: Some(LaneInput::Audio {
                channels: InputChannels::Stereo(2),
            }),
        }),
    )
    .unwrap()
    .doc;
    let s = Session::from_document(doc).unwrap();
    assert!(model(s.record(d.seq.0, spec())));
    assert!(!s.record_state().unwrap().recording);
    assert!(s.media().unwrap().is_empty());
    s.close().unwrap();
}

#[test]
fn a_full_disk_ends_the_pass_and_keeps_what_was_recorded() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    set_free_bytes(Some(50 << 20));
    assert!(matches!(
        s.record(d.seq.0, spec()),
        Err(VizError::Busy { .. })
    ));
    set_free_bytes(Some(500 << 20));
    s.record(d.seq.0, spec()).unwrap();
    assert!(
        s.idle()
            .unwrap()
            .problems
            .iter()
            .any(|p| p.contains("less than 1 GB")),
        "a warning first"
    );
    wait_until("audio", || frames(&s) >= 4800);
    set_free_bytes(Some(50 << 20));
    let mut problems = Vec::new();
    wait_until("the pass to end", || {
        problems.extend(s.idle().unwrap().problems);
        !s.record_state().unwrap().recording
    });
    assert!(
        problems
            .iter()
            .any(|p| p == "Recording stopped: the disk is almost full."),
        "{problems:?}"
    );
    assert_eq!(regions(&s, d.seq.0, d.mic.0).len(), 1, "kept");
    set_free_bytes(None);
    s.close().unwrap();
}

#[test]
fn a_device_that_goes_away_ends_the_pass_cleanly() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    // The output (and its renderer, with the tap) goes.
    reset_host();
    let mut problems = Vec::new();
    wait_until("the pass to end", || {
        problems.extend(s.idle().unwrap().problems);
        !s.record_state().unwrap().recording
    });
    assert!(
        problems
            .iter()
            .any(|p| p.contains("audio device went away")),
        "{problems:?}"
    );
    assert_eq!(regions(&s, d.seq.0, d.mic.0).len(), 1);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    s.close().unwrap();
}

#[test]
fn saving_while_recording_never_trashes_the_file_being_written() {
    let _g = serial(Some(FakeInput::Signals));
    let dir = tempfile::tempdir().unwrap();
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    let rel = s.record_state().unwrap().lanes[0].path.clone().unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.save().unwrap();
    assert!(pkg.join(&rel).is_file(), "the file being written stays");
    s.stop().unwrap();
    s.save().unwrap();
    assert!(pkg.join(&rel).is_file());
    // Undone and saved: the history still names it.
    s.undo().unwrap();
    s.save().unwrap();
    assert!(pkg.join(&rel).is_file());
    s.close().unwrap();
    // Closed: still there, or in the Trash; never deleted.
    let name = Path::new(&rel).file_name().unwrap().to_owned();
    let trashed = std::fs::read_dir(dir.path().join("Trash"))
        .map(|rd| rd.flatten().any(|e| e.file_name() == name))
        .unwrap_or(false);
    assert!(pkg.join(&rel).is_file() || trashed);
}

#[test]
fn closing_an_untitled_recording_moves_it_to_the_trash() {
    let _g = serial(Some(FakeInput::Signals));
    let dir = tempfile::tempdir().unwrap();
    let d = doc(8, false, true);
    let s: Arc<Session> = Session::from_document(d.doc.clone()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    // Closing while recording finishes the pass first.
    s.close().unwrap();
    let names: Vec<String> = std::fs::read_dir(dir.path().join("Trash"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().any(|n| n.starts_with("Mic ")), "{names:?}");
}

/// Stop during a drag: the pass waits for the drag to end, then lands as
/// the one "Record" step.
#[test]
fn a_pass_finished_during_a_drag_lands_when_it_ends() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.begin_edit("Drag".into()).unwrap();
    s.stop().unwrap();
    assert!(!s.record_state().unwrap().recording);
    assert!(
        regions(&s, d.seq.0, d.mic.0).is_empty(),
        "not inside the drag"
    );
    s.end_edit().unwrap();
    assert_eq!(regions(&s, d.seq.0, d.mic.0).len(), 1);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    s.close().unwrap();
}

/// A device that runs at another rate than the project refuses recording
/// (no resampling: it would play back at the wrong speed).
#[test]
fn a_device_at_another_rate_refuses_recording() {
    let _g = serial(Some(FakeInput::Signals));
    vizdaw_ffi::host::set_offline_rate(44_100.0);
    reset_host();
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    let r = s.record(d.seq.0, spec());
    vizdaw_ffi::host::set_offline_rate(0.0);
    match r {
        Err(VizError::Model { message }) => assert!(message.contains("44100"), "{message}"),
        other => panic!("{other:?}"),
    }
    assert!(!s.record_state().unwrap().recording);
    assert!(s.snapshot().unwrap().undo_label.is_none());
    s.close().unwrap();
    reset_host();
}
