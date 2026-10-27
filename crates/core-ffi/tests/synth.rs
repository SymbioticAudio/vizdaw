//! Tracking a hardware synth through a session (plan 5c-2; sub-project 5
//! spec §6.7, §12.3 "Synth tracking"): the offline device's fake synth
//! answers notes sent to "Fake Synth" on fake input channel 3, a
//! `return_of` lane's recording lands on the notes that played it, Track
//! Synth sets up in one undo step, a loop pass makes MIDI and audio takes
//! together, and Measure finds the fake delay with the transport stopped.
//! Offline only: no audio device is opened and no microphone asked.

#![recursion_limit = "256"]

mod common;

use core_engine::fixtures::{self, Builder, Q};
use core_engine::offline::{FAKE_SYNTH_PORT, FakeInput};
use core_media::caf::CafReader;
use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd};
use core_model::document::Document;
use core_model::ids::{AnyId, DestId, LaneId, SeqId};
use core_model::lane::{InputChannels, LaneInput, MonitorMode};
use core_model::sequence::SeqLength;
use core_model::time::Tick;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host, set_fake_input, set_fake_synth};
use vizdaw_ffi::{EventRow, RecordSpecRow, Session};

static SERIAL: Mutex<()> = Mutex::new(());

/// One test at a time, with its own folder standing for the Trash.
struct Serial {
    _g: MutexGuard<'static, ()>,
    dir: tempfile::TempDir,
}

/// Offline, with the fake synth answering `delay_ms` late (`None`: no
/// synth) and the fake input `input` (`None`: the signals input the synth
/// brings).
fn serial(delay_ms: Option<f64>, input: Option<FakeInput>) -> Serial {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(input);
    set_fake_synth(delay_ms);
    vizdaw_ffi::record::set_free_bytes(None);
    reset_host();
    Serial {
        _g: g,
        dir: tempfile::tempdir().unwrap(),
    }
}

fn untitled(g: &Serial, doc: Document) -> Arc<Session> {
    let s = Session::from_document(doc).unwrap();
    s.debug_set_trash_dir(Some(g.dir.path().join("Trash").display().to_string()));
    s
}

/// 120 BPM at 48 kHz: one beat (Q ticks) is 24 000 frames; 1 ms is 48.
const BEAT: i64 = 24_000;
const MS: i64 = 48;
/// The fake synth's delay in these tests.
const DELAY_MS: f64 = 25.0;

#[allow(dead_code)]
struct Doc {
    doc: Document,
    seq: SeqId,
    part: LaneId,
    rec: LaneId,
    synth: DestId,
}

/// Sequence "S" (`bars` bars locked, 120 BPM): lane "Part" plays the
/// external "Fake Synth" with notes at beats 1, 2 and 3 (an eighth each)
/// and listens to MIDI from "Keys", unarmed; lane "Rec" records fake
/// input 3 (as the synth's return when `return_of`), armed, monitor Off,
/// on output "Audio 1".
fn doc(return_of: bool, bars: i64) -> Doc {
    let mut b = Builder::new();
    let synth = b.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (seq, part) = b.sequence("S", "Part", Some(synth)).unwrap();
    b.notes(
        seq,
        part,
        &[
            (Q, 60, 0x8000, Q / 2),
            (2 * Q, 62, 0x8000, Q / 2),
            (3 * Q, 64, 0x8000, Q / 2),
        ],
    )
    .unwrap();
    let rec = b.lane(seq, "Rec", None).unwrap();
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
    for c in [
        LaneCmd::SetInput {
            seq,
            lane: part,
            input: Some(LaneInput::Midi {
                port: Some("Keys".into()),
                channel: None,
            }),
        },
        LaneCmd::SetOutput {
            seq,
            lane: rec,
            output: Some(out),
        },
        LaneCmd::SetInput {
            seq,
            lane: rec,
            input: Some(LaneInput::Audio {
                channels: InputChannels::Mono(2),
                return_of: return_of.then_some(synth),
            }),
        },
        LaneCmd::SetMonitor {
            seq,
            lane: rec,
            monitor: MonitorMode::Off,
        },
        LaneCmd::SetFlags {
            seq,
            lane: rec,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    ] {
        b.run(Command::Lane(c)).unwrap();
    }
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(bars * 4 * Q)),
    }))
    .unwrap();
    Doc {
        doc: b.finish(),
        seq,
        part,
        rec,
        synth,
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

fn frames(s: &Session) -> i64 {
    s.record_state().unwrap().frames as i64
}

fn regions(s: &Session, seq: u64, lane: u64) -> Vec<EventRow> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .into_iter()
        .filter(|e| e.media.is_some())
        .collect()
}

/// Where each click in `lane`'s one region landed on the timeline, in
/// frames: a sample above 0.3 after at least 5 ms at or below it (the
/// synth's tone stays under 0.1).
fn clicks(s: &Session, seq: u64, lane: u64) -> Vec<i64> {
    let r = regions(s, seq, lane);
    assert_eq!(r.len(), 1, "{r:?}");
    let path = s.media_file(r[0].media.unwrap()).unwrap();
    let reader = CafReader::open(Path::new(&path)).unwrap();
    let info = reader.info().clone();
    assert_eq!(info.channels, 1);
    let mut data = vec![0.0f32; info.frames as usize];
    reader.read(0, &mut data, &mut Vec::new()).unwrap();
    let from = r[0].source_offset as usize;
    fixtures::clicks(&data, from, 0.3, (5 * MS) as usize)
        .into_iter()
        .map(|k| r[0].tick * BEAT / Q + k as i64)
        .collect()
}

fn record_to_the_third_note(s: &Session, seq: SeqId) {
    s.record(seq.0, spec()).unwrap();
    wait_until("the third note's answer", || {
        frames(s) >= 3 * BEAT + 60 * MS
    });
    s.stop().unwrap();
}

/// Spec §12.1: the notes the sequence plays to "Fake Synth" come back on
/// fake input channel 3 the synth's delay after they were heard: a lane
/// recording channel 3 that is not linked to the synth takes the clicks
/// 25 ms late.
#[test]
fn the_fake_synth_answers_on_channel_3_its_delay_late() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(false, 2);
    let s = untitled(&g, d.doc.clone());
    record_to_the_third_note(&s, d.seq);
    let got = clicks(&s, d.seq.0, d.rec.0);
    assert_eq!(got.len(), 3, "{got:?}");
    for (at, note) in got.iter().zip([BEAT, 2 * BEAT, 3 * BEAT]) {
        let want = note + 25 * MS;
        assert!((at - want).abs() <= 2, "a click landed at {at}, not {want}");
    }
    s.close().unwrap();
}

/// Plan 5c-2 Ruling 7: with the loopback input the synth stays silent.
#[test]
fn with_the_loopback_input_the_fake_synth_is_silent() {
    let g = serial(Some(DELAY_MS), Some(FakeInput::Loopback(1000)));
    let d = doc(false, 2);
    let s = untitled(&g, d.doc.clone());
    record_to_the_third_note(&s, d.seq);
    assert!(clicks(&s, d.seq.0, d.rec.0).is_empty());
    s.close().unwrap();
}

/// Plan 5c-2 Ruling 10: the host names the return lanes' synths at every
/// load.
#[test]
fn the_return_lanes_synths_are_named_at_load() {
    let d = doc(true, 2);
    assert_eq!(vizdaw_ffi::host::return_dests(&d.doc, d.seq), vec![d.synth]);
    let plain = doc(false, 2);
    assert!(vizdaw_ffi::host::return_dests(&plain.doc, plain.seq).is_empty());
}
