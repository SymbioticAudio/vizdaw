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
use vizdaw_ffi::host::{force_offline, reset_host, set_fake_input, set_fake_microphone_denied};
use vizdaw_ffi::record::{set_free_bytes, set_hold_finish, set_ring_frames};
use vizdaw_ffi::{
    EventKindRow, EventRow, MediaOriginRow, RecordSpecRow, Session, TickRange, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

/// One test at a time, with its own folder standing for the Trash.
struct Serial {
    _g: MutexGuard<'static, ()>,
    dir: tempfile::TempDir,
}

fn serial(input: Option<FakeInput>) -> Serial {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(input);
    set_free_bytes(None);
    set_ring_frames(None);
    set_hold_finish(false);
    set_fake_microphone_denied(false);
    vizdaw_ffi::host::inject_stop_recording_fault(false);
    reset_host();
    Serial {
        _g: g,
        dir: tempfile::tempdir().unwrap(),
    }
}

/// An untitled session whose Trash is the test's folder: closing it moves
/// its recordings there (pre-flight I2 keeps them in the scratch package
/// when the Trash refuses, which would leave them in `$TMPDIR`).
fn untitled(g: &Serial, doc: Document) -> Arc<Session> {
    let s = Session::from_document(doc).unwrap();
    s.debug_set_trash_dir(Some(g.dir.path().join("Trash").display().to_string()));
    s
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
            return_of: None,
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
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
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
    let g = serial(Some(FakeInput::Signals));
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
    let s = untitled(&g, doc);
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
    let g = serial(Some(FakeInput::Loopback(1000)));
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
    let s = untitled(&g, b.finish());
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

/// Final review I1: a graph latency change mid-pass (a latent plugin's
/// latency switched on) leaves what was recorded after it aligned. A
/// looped-back click before and after the change each land within two
/// frames of where they played, and MIDI played after the change lands
/// where the transport was heard then, not the latency later.
#[test]
fn a_latency_change_mid_pass_keeps_what_follows_aligned() {
    const LATENCY: i64 = 12_000;
    let g = serial(Some(FakeInput::Loopback(1000)));
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    let d = doc(8, true, true);
    // A region on "Src" from beat 1 with clicks 12 000 and 144 000 frames
    // in: they play at timeline frames 36 000 and 168 000.
    let mut b = Builder::from_doc(d.doc.clone());
    let media = b.media("Media/clicks.caf", 192_000, 1).unwrap();
    b.region(
        d.seq,
        d.src,
        Q,
        media,
        0,
        192_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let s = untitled(&g, b.finish());
    {
        let path = s.media_file(media.0).unwrap();
        let path = Path::new(&path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut v = vec![0.0f32; 192_000];
        for at in [12_000, 144_000] {
            for x in v.iter_mut().skip(at).take(8) {
                *x = 0.9;
            }
        }
        let mut w = CafWriter::create(path, 48_000, 1, BitDepth::Int24, WriteMode::Normal).unwrap();
        w.write(&v).unwrap();
        w.finish().unwrap();
    }
    s.recheck_media().unwrap();
    // A Test Gain on the master, with no latency yet.
    let master = s.mixer().unwrap().strips.last().unwrap().id;
    s.add_insert(
        master,
        None,
        vizdaw_ffi::PluginRefRow {
            format: vizdaw_ffi::PluginFormatTag::Clap,
            id: "vizdaw.test.gain".into(),
            name: "Test Gain".into(),
            vendor: "VizDAW".into(),
            version: "1.0.0".into(),
        },
    )
    .unwrap();
    let slot = s.plugin_slots().unwrap()[0].id;
    wait_until("the gain to load", || {
        s.idle().unwrap();
        s.plugin_slots().unwrap()[0].status == vizdaw_ffi::SlotStatus::Live
    });
    s.record(d.seq.0, spec()).unwrap();
    let tick = |s: &Session| s.transport().unwrap().tick;
    // Plays a quiet note now: `(tick before, tick after)` as heard.
    let play = |s: &Session, pitch: u8| {
        let before = tick(s);
        assert!(s.inject_midi("Keys".into(), vec![0x90, pitch, 10]).unwrap());
        let after = tick(s);
        (before, after)
    };
    wait_until("the first click to come back", || {
        s.idle().unwrap();
        frames(&s) >= 48_000
    });
    let early = play(&s, 60);
    wait_until("a beat on", || frames(&s) >= 72_000);
    assert!(s.inject_midi("Keys".into(), vec![0x80, 60, 0]).unwrap());
    // The latency switches on.
    s.set_param(slot, 1, LATENCY as f64).unwrap();
    wait_until("the latency to apply", || {
        s.idle().unwrap();
        s.levels().unwrap().latency_frames == LATENCY as u64
    });
    let changed_at = frames(&s);
    // Before the second click plays (rendered at run 168 000).
    assert!(
        changed_at < 140_000,
        "the latency changed late: {changed_at}"
    );
    wait_until("the new graph to be heard", || {
        s.idle().unwrap();
        frames(&s) >= changed_at + 2 * LATENCY as u64
    });
    let late = play(&s, 62);
    wait_until("a beat on", || {
        frames(&s) >= changed_at + 2 * LATENCY as u64 + 24_000
    });
    assert!(s.inject_midi("Keys".into(), vec![0x80, 62, 0]).unwrap());
    wait_until("the second click to come back", || {
        s.idle().unwrap();
        frames(&s) >= 168_000 + LATENCY as u64 + 1000 + 4800
    });
    s.stop().unwrap();
    // Audio: each click lands where it played.
    let r = regions(&s, d.seq.0, d.mic.0);
    assert!(!r.is_empty());
    let (_, data) = read(&s.media_file(r[0].media.unwrap()).unwrap());
    let mut clicks = Vec::new();
    for (k, x) in data.iter().enumerate() {
        if *x > 0.5 && clicks.last().is_none_or(|c: &usize| k > c + 100) {
            clicks.push(k);
        }
    }
    assert_eq!(clicks.len(), 2, "{clicks:?}");
    let mut landed = Vec::new();
    for k in clicks {
        let k = k as i64;
        let region = r
            .iter()
            .find(|e| {
                let from = e.source_offset as i64;
                from <= k && k < from + e.frames as i64
            })
            .unwrap_or_else(|| panic!("no region holds file frame {k}: {r:?}"));
        landed.push(region.tick * BEAT / Q + (k - region.source_offset as i64));
    }
    for (at, want) in landed.iter().zip([36_000i64, 168_000]) {
        assert!(
            (at - want).abs() <= 2,
            "a click landed at {at}, not {want}: {r:?}"
        );
    }
    // MIDI: each note where the transport was heard as it was played
    // (within a block or two either way; the latency is 480 ticks).
    let got = notes(&s, d.seq.0, d.keys.0);
    let slack = 2048 * Q / BEAT;
    for (pitch, (before, after)) in [(60u8, early), (62, late)] {
        let (t, _) = *got
            .iter()
            .find(|(_, p)| *p == pitch)
            .unwrap_or_else(|| panic!("note {pitch} not recorded: {got:?}"));
        assert!(
            before - slack <= t && t <= after + slack,
            "note {pitch} landed at tick {t}, played between {before} and {after}"
        );
    }
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
    let g = serial(Some(FakeInput::Signals));
    // One bar at 120 BPM: a pass every two seconds (pre-flight M14: wide
    // enough that a keep or discard lands in the pass it was meant for).
    let d = doc(1, false, true);
    let s = untitled(&g, d.doc.clone());
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
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, true, false);
    let s = untitled(&g, d.doc.clone());
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
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
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
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, false);
    let s = untitled(&g, d.doc.clone());
    let model = |r: Result<(), VizError>| matches!(r, Err(VizError::Model { .. }));
    assert!(model(s.record(d.seq.0, spec())), "nothing armed");
    assert!(model(s.record(999, spec())), "no such sequence");
    s.close().unwrap();
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    for bad in [
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
                return_of: None,
            }),
        }),
    )
    .unwrap()
    .doc;
    let s = untitled(&g, doc);
    assert!(model(s.record(d.seq.0, spec())));
    assert!(!s.record_state().unwrap().recording);
    assert!(s.media().unwrap().is_empty());
    s.close().unwrap();
}

/// Pre-flight M6 (plan 5c Task 11): with the microphone refused (a fake
/// state: the real permission is never read), Record refuses with why, not
/// with a channel count, every time: the reason outlives the faults Play
/// clears.
#[test]
fn a_refused_microphone_is_why_record_refuses() {
    let g = serial(None);
    set_fake_microphone_denied(true);
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    for _ in 0..2 {
        match s.record(d.seq.0, spec()) {
            Err(VizError::Model { message }) => assert!(
                message.starts_with("VizDAW may not use audio input"),
                "{message}"
            ),
            r => panic!("{r:?}"),
        }
        assert!(!s.record_state().unwrap().recording);
    }
    assert!(s.media().unwrap().is_empty());
    s.close().unwrap();
    // Allowed (and no input): the channel count is the reason.
    set_fake_microphone_denied(false);
    let s = untitled(&g, d.doc.clone());
    match s.record(d.seq.0, spec()) {
        Err(VizError::Model { message }) => {
            assert!(message.contains("0 channel(s)"), "{message}")
        }
        r => panic!("{r:?}"),
    }
    s.close().unwrap();
}

/// Task 11 review I1: an edit during a pass that no longer wants audio
/// input (the only audio lane disarmed) neither closes nor restarts the
/// input under the pass; the change applies once the pass ends.
#[test]
fn input_stays_open_until_the_pass_ends() {
    use vizdaw_ffi::edit::{EditCommand, LaneEdit};
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.record(d.seq.0, spec()).unwrap();
    assert_eq!(s.debug_input_open(), Some(true));
    let changes = s.debug_input_changes().unwrap();
    wait_until("half a second of audio", || frames(&s) >= 24_000);
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: d.seq.0,
                lane: d.mic.0,
                mute: None,
                solo: None,
                arm: Some(false),
            },
        },
        None,
    )
    .unwrap();
    assert_eq!(s.debug_input_open(), Some(true), "closed under the pass");
    assert_eq!(s.debug_input_changes(), Some(changes), "restarted mid-pass");
    wait_until("a second of audio", || frames(&s) >= 48_000);
    assert!(s.record_state().unwrap().recording);
    s.stop().unwrap();
    assert!(!s.record_state().unwrap().recording);
    assert_eq!(
        s.debug_input_open(),
        Some(false),
        "the change applies after"
    );
    // The take is whole: the fake sine runs to its last frame.
    let media = s.media().unwrap();
    assert_eq!(media.len(), 1);
    let (_, data) = read(&s.media_file(media[0].id).unwrap());
    assert!(data.len() >= 48_000);
    let tail = &data[data.len() - 4_800..];
    let peak = tail.iter().fold(0.0f32, |a, x| a.max(x.abs()));
    assert!(peak > 0.2, "silent tail: {peak}");
    s.close().unwrap();
}

/// Task 11 re-review m1: a pass whose engine stop fails still lets go of
/// the input, so the next lane edit reaches it at once.
#[test]
fn a_failed_stop_releases_the_input() {
    use vizdaw_ffi::edit::{EditCommand, LaneEdit};
    use vizdaw_ffi::host::inject_stop_recording_fault;
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.record(d.seq.0, spec()).unwrap();
    assert_eq!(s.debug_input_open(), Some(true));
    inject_stop_recording_fault(true);
    s.stop().unwrap();
    inject_stop_recording_fault(false);
    assert!(!s.record_state().unwrap().recording);
    let arm = |on: bool| {
        s.edit(
            EditCommand::Lane {
                edit: LaneEdit::SetFlags {
                    seq: d.seq.0,
                    lane: d.mic.0,
                    mute: None,
                    solo: None,
                    arm: Some(on),
                },
            },
            None,
        )
        .unwrap();
    };
    // Not held back: each change reaches the input (on its own thread,
    // plan 5d-1 Ruling 13).
    arm(false);
    wait_until("the disarm to close the input", || {
        s.debug_input_open() == Some(false)
    });
    arm(true);
    wait_until("the arm to open the input", || {
        s.debug_input_open() == Some(true)
    });
    s.close().unwrap();
}

#[test]
fn a_full_disk_ends_the_pass_and_keeps_what_was_recorded() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
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
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
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
    // Pre-flight M11: the transport says the input was lost.
    let faults = s.transport().unwrap().faults;
    assert!(faults.iter().any(|f| f == "input-lost"), "{faults:?}");
    s.close().unwrap();
}

#[test]
fn saving_while_recording_never_trashes_the_file_being_written() {
    let g = serial(Some(FakeInput::Signals));
    let dir = tempfile::tempdir().unwrap();
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
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
    let g = serial(Some(FakeInput::Signals));
    let dir = tempfile::tempdir().unwrap();
    let d = doc(8, false, true);
    let s: Arc<Session> = untitled(&g, d.doc.clone());
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
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
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
    let g = serial(Some(FakeInput::Signals));
    vizdaw_ffi::host::set_offline_rate(44_100.0);
    reset_host();
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
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

// Pre-flight items (plan 5c pre-flight I1–I4, M5, M11).

fn trash_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Pre-flight I1: a save while Stop finishes the pass (its files closing
/// without the session lock) never sends the take to the Trash, and Save
/// As waits for it.
#[test]
fn a_save_while_the_pass_finishes_keeps_its_file() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    let pkg = g.dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    let rel = s.record_state().unwrap().lanes[0].path.clone().unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    set_hold_finish(true);
    let stopper = {
        let s = Arc::clone(&s);
        std::thread::spawn(move || s.stop())
    };
    wait_until("the pass to be finishing", || {
        !s.record_state().unwrap().recording
    });
    s.save().unwrap();
    assert!(pkg.join(&rel).is_file(), "the file being finished stays");
    let other = g.dir.path().join("Q.vizdaw");
    assert!(matches!(
        s.save_as(other.display().to_string()),
        Err(VizError::Busy { .. })
    ));
    set_hold_finish(false);
    stopper.join().unwrap().unwrap();
    assert!(pkg.join(&rel).is_file());
    let m = &s.media().unwrap()[0];
    assert_eq!(m.offline, None, "online after the pass landed");
    s.save().unwrap();
    assert!(pkg.join(&rel).is_file());
    assert!(trash_names(&g.dir.path().join("Trash")).is_empty());
    s.close().unwrap();
}

/// Pre-flight I1: Save As is refused while a finished pass waits for an
/// open edit to end (it would land on paths the new package never got).
#[test]
fn save_as_waits_for_a_pending_pass() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.begin_edit("Drag".into()).unwrap();
    s.stop().unwrap();
    let pkg = g.dir.path().join("P.vizdaw");
    assert!(matches!(
        s.save_as(pkg.display().to_string()),
        Err(VizError::Busy { .. })
    ));
    s.end_edit().unwrap();
    s.save_as(pkg.display().to_string()).unwrap();
    let m = &s.media().unwrap()[0];
    assert_eq!(m.offline, None);
    assert!(Path::new(&s.media_file(m.id).unwrap()).starts_with(&pkg));
    s.close().unwrap();
}

/// Pre-flight I2: closing an untitled project whose recordings cannot go
/// to the Trash (none in test builds) leaves its scratch package.
#[test]
fn an_untitled_close_that_cannot_trash_keeps_the_recordings() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.stop().unwrap();
    let file = std::path::PathBuf::from(s.media_file(s.media().unwrap()[0].id).unwrap());
    assert!(file.is_file());
    s.close().unwrap();
    let kept = file.is_file();
    // Media/Recorded/<file> in the scratch package.
    let package = file.ancestors().nth(3).unwrap().to_path_buf();
    assert!(
        package.extension().is_some_and(|e| e == "vizdaw"),
        "{package:?}"
    );
    let _ = std::fs::remove_dir_all(&package);
    assert!(kept, "never unlinked");
}

/// Pre-flight I3: a take whose lane was removed while recording is kept
/// (and said so), never trashed by the next save.
#[test]
fn a_take_whose_lane_was_removed_is_kept_and_reported() {
    use vizdaw_ffi::edit::{EditCommand, LaneEdit};
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    let pkg = g.dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    let rel = s.record_state().unwrap().lanes[0].path.clone().unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Remove {
                seq: d.seq.0,
                lane: d.mic.0,
            },
        },
        None,
    )
    .unwrap();
    s.stop().unwrap();
    let problems = s.idle().unwrap().problems;
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("Mic: ") && p.contains("its lane was removed")),
        "{problems:?}"
    );
    s.save().unwrap();
    assert!(pkg.join(&rel).is_file(), "kept for the recovery offer");
    s.close().unwrap();
    assert!(pkg.join(&rel).is_file());
    assert!(trash_names(&g.dir.path().join("Trash")).is_empty());
}

/// Pre-flight I4: input the ring could not hold is silence, the step says
/// "Record (with gaps)" and the user is told.
#[test]
fn lost_input_is_silence_and_the_step_says_with_gaps() {
    let g = serial(Some(FakeInput::Signals));
    // A one-frame ring (room for 61 frames with its headers): the offline
    // driver renders at least 2 ms (96 frames) a block, so blocks are
    // lost.
    set_ring_frames(Some(1));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.record(d.seq.0, spec()).unwrap();
    wait_until("a second", || frames(&s) >= 48_000);
    let st = s.record_state().unwrap();
    assert!(st.overflows > 0, "{st:?}");
    // Pre-flight M11: the transport says so while it records.
    wait_until("the record-overflow fault", || {
        s.idle().unwrap();
        s.transport()
            .unwrap()
            .faults
            .iter()
            .any(|f| f == "record-overflow")
    });
    let run = frames(&s);
    s.stop().unwrap();
    set_ring_frames(None);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Record (with gaps)")
    );
    let m = &s.media().unwrap()[0];
    // Padded to the run: as long as what played.
    assert!(m.frames >= run, "{} < {run}", m.frames);
    let (_, data) = read(&s.media_file(m.id).unwrap());
    // Lost blocks are silence (the writer pads them at the end).
    let zeros = data.iter().filter(|x| **x == 0.0).count();
    assert!(zeros * 100 >= data.len() * 99, "{zeros} of {}", data.len());
    let problems = s.idle().unwrap().problems;
    assert!(
        problems.iter().any(|p| p.starts_with("Recording: ")
            && p.ends_with("s of input could not be written in time and is silence.")),
        "{problems:?}"
    );
    s.close().unwrap();
}

/// Pre-flight M5: another window playing ends the pass, and says so.
#[test]
fn another_window_playing_ends_the_pass_and_says_so() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    let other = untitled(&g, d.doc.clone());
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    other.play(d.seq.0).unwrap();
    let mut problems = Vec::new();
    wait_until("the pass to end", || {
        problems.extend(s.idle().unwrap().problems);
        !s.record_state().unwrap().recording
    });
    assert!(
        problems
            .iter()
            .any(|p| p.contains("another window started playing")),
        "{problems:?}"
    );
    assert_eq!(regions(&s, d.seq.0, d.mic.0).len(), 1, "kept");
    other.stop().unwrap();
    other.close().unwrap();
    s.close().unwrap();
}

/// Pre-flight M11: the streamer's underruns and late starts while
/// recording are in the record state.
#[test]
fn stream_misses_while_recording_are_counted() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let mut b = Builder::from_doc(d.doc.clone());
    let media = b.media("Media/click.caf", 480_000, 1).unwrap();
    b.region(
        d.seq,
        d.src,
        0,
        media,
        0,
        480_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let s = untitled(&g, b.finish());
    click(Path::new(&s.media_file(media.0).unwrap()), 480_000, 12_000);
    s.recheck_media().unwrap();
    let st = s.record_state().unwrap();
    assert_eq!((st.underruns, st.late_starts), (0, 0));
    vizdaw_ffi::host::stall_disk(true);
    let r = s.record(d.seq.0, spec());
    let mut st = s.record_state().unwrap();
    if r.is_ok() {
        wait_until("a stream miss", || {
            st = s.record_state().unwrap();
            st.underruns + st.late_starts > 0
        });
    }
    vizdaw_ffi::host::stall_disk(false);
    r.unwrap();
    assert!(st.underruns + st.late_starts > 0, "{st:?}");
    s.stop().unwrap();
    s.close().unwrap();
    reset_host();
}

/// Pre-flight I3: a kept take (its lane was removed) of an untitled
/// project goes with it into the package Save As makes.
#[test]
fn a_kept_take_follows_save_as() {
    use vizdaw_ffi::edit::{EditCommand, LaneEdit};
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.record(d.seq.0, spec()).unwrap();
    let rel = s.record_state().unwrap().lanes[0].path.clone().unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Remove {
                seq: d.seq.0,
                lane: d.mic.0,
            },
        },
        None,
    )
    .unwrap();
    s.stop().unwrap();
    let pkg = g.dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    assert!(pkg.join(&rel).is_file(), "copied with the project");
    s.close().unwrap();
    assert!(pkg.join(&rel).is_file());
}

// Task 9 review fix round 1.

fn format_44k() -> vizdaw_ffi::ProjectFormatRow {
    vizdaw_ffi::ProjectFormatRow {
        sample_rate: 44_100,
        depth: vizdaw_ffi::BitDepthRow::Int24,
    }
}

/// Review I-1: the project format does not change under a recording (the
/// take would be planned at the new rate), while it finishes, or while
/// its pass waits for an edit to end.
#[test]
fn the_format_does_not_change_while_recording() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    let busy = |r: Result<u64, VizError>| matches!(r, Err(VizError::Busy { .. }));
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    assert!(busy(s.set_project_format(format_44k())), "recording");
    // Finishing (held, on another thread).
    set_hold_finish(true);
    let stopper = {
        let s = Arc::clone(&s);
        std::thread::spawn(move || s.stop())
    };
    wait_until("the pass to be finishing", || {
        !s.record_state().unwrap().recording
    });
    assert!(busy(s.set_project_format(format_44k())), "finishing");
    set_hold_finish(false);
    stopper.join().unwrap().unwrap();
    // Pending behind a drag.
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.begin_edit("Drag".into()).unwrap();
    s.stop().unwrap();
    assert!(busy(s.set_project_format(format_44k())), "pending");
    s.end_edit().unwrap();
    assert_eq!(s.project_info().unwrap().format.sample_rate, 48_000);
    s.close().unwrap();
}

/// Review I-1, the other way: no recording starts while a Format job has
/// not been applied.
#[test]
fn no_recording_starts_while_the_format_changes() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.debug_hold_jobs(true).unwrap();
    let job = s.set_project_format(format_44k()).unwrap();
    let r = s.record(d.seq.0, spec());
    s.debug_hold_jobs(false).unwrap();
    match r {
        Err(VizError::Busy { message }) => assert!(message.contains("format"), "{message}"),
        other => panic!("{other:?}"),
    }
    assert!(!s.record_state().unwrap().recording);
    wait_until("the format job", || {
        s.idle().unwrap();
        s.job(job).unwrap().state == vizdaw_ffi::JobStateRow::Done
    });
    s.close().unwrap();
}

/// Review I-2: one package is open in one session at a time (a second
/// session's save would trash the first one's recording).
#[test]
fn a_package_opens_in_one_session_at_a_time() {
    let g = serial(None);
    let d = doc(8, false, true);
    let a = untitled(&g, d.doc.clone());
    let p = g.dir.path().join("P.vizdaw");
    a.save_as(p.display().to_string()).unwrap();
    let already = |r: Result<Arc<Session>, VizError>| match r {
        Err(VizError::Busy { message }) => message.contains("already open"),
        _ => false,
    };
    assert!(already(Session::open(p.display().to_string())));
    // Any spelling of the same package.
    let other_case = g.dir.path().join("p.VIZDAW");
    if other_case.exists() {
        assert!(already(Session::open(other_case.display().to_string())));
    }
    // Save As onto it from another session.
    let b = untitled(&g, d.doc.clone());
    assert!(matches!(
        b.save_as(p.display().to_string()),
        Err(VizError::Busy { .. })
    ));
    let q = g.dir.path().join("Q.vizdaw");
    b.save_as(q.display().to_string()).unwrap();
    assert!(already(Session::open(q.display().to_string())));
    a.close().unwrap();
    // Closed: it opens again; Save As moved b off its untitled state.
    let a2 = Session::open(p.display().to_string()).unwrap();
    b.close().unwrap();
    drop(b);
    let b2 = Session::open(q.display().to_string()).unwrap();
    // Dropped without close: released too.
    drop(a2);
    let a3 = Session::open(p.display().to_string()).unwrap();
    // Save As away from P releases it.
    a3.save_as(g.dir.path().join("R.vizdaw").display().to_string())
        .unwrap();
    Session::open(p.display().to_string())
        .unwrap()
        .close()
        .unwrap();
    a3.close().unwrap();
    b2.close().unwrap();
}

/// Review minor (b): an unreadable `Media/Recorded` at an untitled close
/// may hold recordings: the scratch package stays.
#[test]
fn an_unreadable_recorded_folder_is_never_removed() {
    use std::os::unix::fs::PermissionsExt;
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.stop().unwrap();
    let file = std::path::PathBuf::from(s.media_file(s.media().unwrap()[0].id).unwrap());
    let folder = file.parent().unwrap().to_path_buf();
    let package = file.ancestors().nth(3).unwrap().to_path_buf();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o000)).unwrap();
    s.close().unwrap();
    let _ = std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755));
    let kept = file.is_file();
    let pkg_left = package.exists();
    let _ = std::fs::remove_dir_all(&package);
    assert!(kept, "the recording stays");
    assert!(pkg_left, "the scratch package stays");
}

// Task 9 re-review fix round 2: the package claim is airtight.

/// Two opens of one package at the same moment: exactly one gets it.
#[test]
fn concurrent_opens_of_one_package_give_it_to_one() {
    let g = serial(None);
    let d = doc(8, false, true);
    let p = g.dir.path().join("P.vizdaw");
    let first = untitled(&g, d.doc.clone());
    first.save_as(p.display().to_string()).unwrap();
    first.close().unwrap();
    drop(first);
    for round in 0..20 {
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let opens: Vec<_> = (0..2)
            .map(|_| {
                let (b, path) = (Arc::clone(&barrier), p.display().to_string());
                std::thread::spawn(move || {
                    b.wait();
                    Session::open(path)
                })
            })
            .collect();
        let got: Vec<_> = opens.into_iter().map(|t| t.join().unwrap()).collect();
        let ok: Vec<&Arc<Session>> = got.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert_eq!(ok.len(), 1, "round {round}: {got:?}");
        assert!(
            got.iter().any(
                |r| matches!(r, Err(VizError::Busy { message }) if message.contains("already open"))
            ),
            "round {round}"
        );
        ok[0].close().unwrap();
    }
}

/// Final review M1: the package stays claimed until `close` has finished
/// writing it (the keep file, the tidy): an open meanwhile is refused.
#[test]
fn close_holds_the_package_until_it_is_tidied() {
    let g = serial(None);
    let d = doc(8, false, true);
    let p = g.dir.path().join("P.vizdaw");
    let a = untitled(&g, d.doc.clone());
    a.save_as(p.display().to_string()).unwrap();
    type Opened = Option<Result<Arc<Session>, VizError>>;
    let during: Arc<Mutex<Opened>> = Arc::default();
    {
        let during = during.clone();
        let at = p.display().to_string();
        vizdaw_ffi::session::set_before_close_tidy_hook(Some(Box::new(move || {
            *during.lock().unwrap() = Some(Session::open(at));
        })));
    }
    a.close().unwrap();
    vizdaw_ffi::session::set_before_close_tidy_hook(None);
    let got = during.lock().unwrap().take().expect("the hook ran");
    let refused =
        matches!(&got, Err(VizError::Busy { message }) if message.contains("already open"));
    if let Ok(b) = &got {
        b.close().unwrap();
    }
    assert!(refused, "an open during close's tidy: {got:?}");
    // Closed: it opens again.
    Session::open(p.display().to_string())
        .unwrap()
        .close()
        .unwrap();
}

/// Final review M2: an open refused because the package is open elsewhere
/// leaves that session's import temp files alone.
#[test]
fn a_refused_open_leaves_the_temp_files() {
    let g = serial(None);
    let d = doc(8, false, true);
    let p = g.dir.path().join("P.vizdaw");
    let a = untitled(&g, d.doc.clone());
    a.save_as(p.display().to_string()).unwrap();
    let media = p.join("Media");
    std::fs::create_dir_all(&media).unwrap();
    let tmp = media.join(".take.wav.1.vizdaw-tmp");
    std::fs::write(&tmp, b"in flight").unwrap();
    assert!(matches!(
        Session::open(p.display().to_string()),
        Err(VizError::Busy { .. })
    ));
    assert!(
        tmp.is_file(),
        "the refused open swept the other session's temp file"
    );
    a.close().unwrap();
    // The open that claims it sweeps it.
    let b = Session::open(p.display().to_string()).unwrap();
    assert!(!tmp.exists());
    b.close().unwrap();
}

/// A panic while opening (after the package was claimed, before the
/// session exists) does not leave the package claimed.
#[test]
fn a_panic_while_opening_releases_the_package() {
    let g = serial(None);
    let d = doc(8, false, true);
    let p = g.dir.path().join("P.vizdaw");
    let first = untitled(&g, d.doc.clone());
    first.save_as(p.display().to_string()).unwrap();
    first.close().unwrap();
    drop(first);
    fn boom() {
        panic!("injected panic before the open's sync");
    }
    vizdaw_ffi::recovery::set_before_sync_hook(Some(boom));
    let r = Session::open(p.display().to_string());
    vizdaw_ffi::recovery::set_before_sync_hook(None);
    assert!(matches!(r, Err(VizError::Internal { .. })), "{r:?}");
    let s = Session::open(p.display().to_string()).expect("not left claimed");
    s.close().unwrap();
}

/// Save As claims its target before writing anything: onto an open
/// package nothing is written; a closed session cannot Save As.
#[test]
fn save_as_claims_before_it_writes() {
    let g = serial(None);
    let d = doc(8, false, true);
    let p = g.dir.path().join("P.vizdaw");
    let a = untitled(&g, d.doc.clone());
    a.save_as(p.display().to_string()).unwrap();
    // A package that is open but not on disk yet stands for "claimed":
    // the target exists only as a claim here, so check nothing is
    // written into an open package.
    let marker = p.join("project.json");
    let before = std::fs::metadata(&marker).unwrap().modified().unwrap();
    let b = untitled(&g, d.doc.clone());
    b.rename_sequence(d.seq.0, "B".into()).unwrap();
    assert!(matches!(
        b.save_as(p.display().to_string()),
        Err(VizError::Busy { .. })
    ));
    assert_eq!(
        std::fs::metadata(&marker).unwrap().modified().unwrap(),
        before
    );
    let text = std::fs::read_to_string(&marker).unwrap();
    assert!(!text.contains("\"B\""), "nothing written");
    // Closed: no Save As (it would claim a package for a closed window).
    b.close().unwrap();
    let q = g.dir.path().join("Q.vizdaw");
    assert!(b.save_as(q.display().to_string()).is_err());
    assert!(!q.exists());
    a.close().unwrap();
}

/// Task 12 review M3 (pre-flight M7): an offline host never connects the
/// machine's CoreMIDI sources, at Play or at a Record with an armed MIDI
/// lane. The counter counts every connect attempt, whatever the build
/// (the hook below proves it moves), so the gate pinned is the host's
/// own live check, not the test build's compiled-out listener.
#[test]
fn an_offline_host_never_connects_midi_sources() {
    use vizdaw_ffi::host::{debug_connect_midi_sources, midi_source_connects};
    let g = serial(None);
    let d = doc(2, true, false);
    let s = untitled(&g, d.doc);
    let before = midi_source_connects();
    s.play(d.seq.0).unwrap();
    s.stop().unwrap();
    s.record(d.seq.0, spec()).unwrap();
    wait_until("recording", || s.record_state().unwrap().recording);
    s.stop().unwrap();
    assert_eq!(midi_source_connects(), before, "an offline host connected");
    debug_connect_midi_sources();
    assert_eq!(midi_source_connects(), before + 1);
    s.close().unwrap();
}

/// Plan 5d-1 Ruling 7 (spec §6.4): a count-in plays before the transport
/// rolls, the timeline standing still: nothing is captured while it
/// counts, and the take starts where the recording was asked to.
#[test]
fn a_count_in_plays_before_the_pass_and_does_not_move_the_take() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.locate(4 * Q).unwrap();
    s.record(
        d.seq.0,
        RecordSpecRow {
            count_in_bars: 1,
            ..spec()
        },
    )
    .unwrap();
    // One 4/4 bar at 120 BPM: two seconds of clicks.
    wait_until("the count-in", || s.transport().unwrap().counting_in);
    assert_eq!(frames(&s), 0, "nothing is captured while it counts");
    assert!(!s.transport().unwrap().playing);
    assert!(s.record_state().unwrap().recording);
    wait_until("the roll", || {
        !s.transport().unwrap().counting_in && frames(&s) > 0
    });
    wait_until("a second of audio", || frames(&s) >= 48_000);
    s.stop().unwrap();
    let r = regions(&s, d.seq.0, d.mic.0);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].tick, r[0].source_offset), (4 * Q, 0));
    s.close().unwrap();
}

/// Plan 5d-1 M12: Stop during the count-in cancels the pass: nothing was
/// captured, so no region lands, recording ends, and the transport never
/// rolls.
#[test]
fn stop_during_a_count_in_records_nothing() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    // Saved, so the package (and its Media/Recorded) has a known path.
    let package = g.dir.path().join("Count.vizdaw");
    let s = untitled(&g, d.doc.clone());
    s.save_as(package.display().to_string()).unwrap();
    s.record(
        d.seq.0,
        RecordSpecRow {
            count_in_bars: 1,
            ..spec()
        },
    )
    .unwrap();
    wait_until("the count-in", || s.transport().unwrap().counting_in);
    let recorded = package.join("Media/Recorded");
    let files = |dir: &std::path::Path| -> Vec<std::path::PathBuf> {
        std::fs::read_dir(dir)
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    };
    let opened = files(&recorded);
    assert!(!opened.is_empty(), "the pass opened its take file");
    s.stop().unwrap();
    assert!(!s.record_state().unwrap().recording);
    assert!(!s.transport().unwrap().counting_in);
    assert!(!s.transport().unwrap().playing);
    assert_eq!(frames(&s), 0);
    assert!(
        regions(&s, d.seq.0, d.mic.0).is_empty(),
        "nothing was captured"
    );
    // The (empty) take file is left in place, never unlinked.
    for f in &opened {
        assert!(f.is_file(), "{f:?} was removed");
    }
    s.close().unwrap();
}

/// Review m5: with Software Monitoring off an armed lane still records.
#[test]
fn recording_goes_on_with_software_monitoring_off() {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            vizdaw_ffi::audio::set_software_monitoring(true);
        }
    }
    let g = serial(Some(FakeInput::Signals));
    let _r = Restore;
    vizdaw_ffi::audio::set_software_monitoring(false);
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.record(d.seq.0, spec()).unwrap();
    wait_until("a second of audio", || frames(&s) >= 48_000);
    s.stop().unwrap();
    let media = s.media().unwrap();
    assert_eq!(media.len(), 1);
    assert_eq!(regions(&s, d.seq.0, d.mic.0).len(), 1);
    let (_, data) = read(&s.media_file(media[0].id).unwrap());
    let peak = data.iter().fold(0.0f32, |a, x| a.max(x.abs()));
    assert!((peak - 0.2512).abs() < 0.01, "{peak}");
    s.close().unwrap();
}
