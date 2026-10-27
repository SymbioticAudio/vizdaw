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
use vizdaw_ffi::host::{
    force_offline, midi_destination_targets, reset_host, set_fake_input, set_fake_synth,
};
use vizdaw_ffi::{EditCommand, EventRow, LaneEdit, LaneInputRow, RecordSpecRow, Session};

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

/// Task 7 review I1: with the loopback input nothing plays the fake
/// synth, so the host has no "Fake Synth" port (a lane playing it is
/// unroutable) and notes sent its way never fill a queue nothing drains:
/// thousands of them raise no `midi-dropped` fault.
#[test]
fn with_the_loopback_input_there_is_no_fake_synth_port_to_fill() {
    let g = serial(Some(DELAY_MS), Some(FakeInput::Loopback(1000)));
    let d = doc(false, 8);
    let mut b = Builder::from_doc(d.doc.clone());
    b.run(Command::Lane(LaneCmd::SetMonitor {
        seq: d.seq,
        lane: d.part,
        monitor: MonitorMode::Input,
    }))
    .unwrap();
    let s = untitled(&g, b.finish());
    s.play(d.seq.0).unwrap();
    wait_until("the transport", || s.transport().unwrap().playing);
    // 4200 notes (8400 events, over the synth's 4096), 50 at a time.
    for _ in 0..84 {
        let mut bytes = Vec::new();
        for _ in 0..50 {
            bytes.extend_from_slice(&[0x90, 60, 100, 0x80, 60, 0]);
        }
        wait_until("room in the MIDI input queue", || {
            s.inject_midi("Keys".into(), bytes.clone()).is_ok()
        });
        std::thread::sleep(Duration::from_millis(5));
    }
    let from = s.transport().unwrap().tick;
    wait_until("the last notes to play", || {
        s.transport().unwrap().tick >= from + Q / 4
    });
    let t = s.transport().unwrap();
    assert!(
        !t.faults.iter().any(|f| f == "midi-dropped"),
        "{:?}",
        t.faults
    );
    assert!(
        t.report.iter().any(|l| l.contains(FAKE_SYNTH_PORT)),
        "the lane playing Fake Synth is reported unroutable (the port is absent): {:?}",
        t.report
    );
    assert!(midi_destination_targets().is_empty());
    s.stop().unwrap();
    s.close().unwrap();
}

/// Task 7 review M3: every load hands the engine the return lanes'
/// synths, so it interns one the sequence never plays (Measure plays it;
/// plan 5c-2 Ruling 10), once however many lanes return it.
#[test]
fn a_load_interns_the_return_synth_the_sequence_never_plays() {
    let g = serial(Some(DELAY_MS), None);
    let mut b = Builder::new();
    let synth = b.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (seq, _part) = b.sequence("S", "Part", None).unwrap();
    for name in ["Rec", "Rec 2"] {
        let lane = b.lane(seq, name, None).unwrap();
        b.run(Command::Lane(LaneCmd::SetInput {
            seq,
            lane,
            input: Some(LaneInput::Audio {
                channels: InputChannels::Mono(2),
                return_of: Some(synth),
            }),
        }))
        .unwrap();
    }
    let doc = b.finish();
    assert_eq!(vizdaw_ffi::host::return_dests(&doc, seq), vec![synth]);
    let s = untitled(&g, doc);
    s.play(seq.0).unwrap();
    assert_eq!(
        midi_destination_targets(),
        vec![FAKE_SYNTH_PORT.to_string()]
    );
    s.stop().unwrap();
    s.close().unwrap();
}

/// Spec §12.3: one pass with an armed `return_of` lane — the clicks land
/// on the MIDI note starts within ±1 ms, with `return_latency_ms` set to
/// the fake delay.
#[test]
fn a_return_lane_lands_on_the_notes_that_played_it() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    s.set_return_latency(d.synth.0, DELAY_MS as f32).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Return Latency")
    );
    let dest = s
        .destinations()
        .unwrap()
        .into_iter()
        .find(|x| x.id == d.synth.0)
        .unwrap();
    assert!(dest.external);
    assert_eq!(dest.return_latency_ms, DELAY_MS as f32);
    record_to_the_third_note(&s, d.seq);
    let got = clicks(&s, d.seq.0, d.rec.0);
    assert_eq!(got.len(), 3, "{got:?}");
    for (at, note) in got.iter().zip([BEAT, 2 * BEAT, 3 * BEAT]) {
        assert!(
            (at - note).abs() <= MS,
            "a click landed at {at}, its note at {note}"
        );
    }
    // The region starts where recording did, its file offset taking the
    // synth's latency.
    let r = regions(&s, d.seq.0, d.rec.0);
    assert_eq!((r[0].tick, r[0].source_offset), (0, 25 * MS as u64));
    s.close().unwrap();
}

/// Spec §6.7, §12.3: Track Synth sets up and arms in one undo step; run
/// again it reuses the lane; it refuses what it cannot set up and
/// changes nothing then.
#[test]
fn track_synth_sets_up_and_arms_in_one_undo_step() {
    let g = serial(Some(DELAY_MS), None);
    let mut b = Builder::new();
    let synth = b.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (seq, part) = b.sequence("S", "Part", Some(synth)).unwrap();
    let keys = b.lane(seq, "Keys", Some(piano)).unwrap();
    let s = untitled(&g, b.finish());
    s.track_synth(seq.0, part.0, 2, false, false, false)
        .unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Track Synth")
    );
    let lanes = s.lanes(seq.0).unwrap();
    let names: Vec<&str> = lanes.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Part", "Fake Synth audio", "Keys"]);
    assert_eq!(
        lanes[1].input,
        Some(LaneInputRow::Audio {
            first: 2,
            stereo: false,
            return_of: Some(synth.0),
        })
    );
    assert!(lanes[1].arm && !lanes[0].arm);
    assert!(lanes[1].audio_out.is_some());
    s.track_synth(seq.0, part.0, 4, true, true, false).unwrap();
    let lanes = s.lanes(seq.0).unwrap();
    assert_eq!(lanes.len(), 3, "the same lane, set up again");
    assert_eq!(
        lanes[1].input,
        Some(LaneInputRow::Audio {
            first: 4,
            stereo: true,
            return_of: Some(synth.0),
        })
    );
    assert!(lanes[0].arm);
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(s.lanes(seq.0).unwrap().len(), 2);
    let v = s.snapshot().unwrap().version;
    for (lane, first) in [(keys.0, 0u16), (part.0, 300), (999, 0)] {
        assert!(
            s.track_synth(seq.0, lane, first, false, false, false)
                .is_err(),
            "lane {lane}, channel {first}"
        );
    }
    assert!(s.track_synth(999, part.0, 0, false, false, false).is_err());
    assert_eq!(s.snapshot().unwrap().version, v, "nothing changed");
    s.close().unwrap();
}

/// Spec §6.7, §12.3: with the MIDI lane armed too, a loop pass makes MIDI
/// and audio takes together; "Mute the MIDI lane after recording" mutes
/// it in the same "Record" step (plan 5c-2 Ruling 6).
#[test]
fn a_loop_pass_makes_midi_and_audio_takes_together_and_mutes_after() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 1);
    let s = untitled(&g, d.doc.clone());
    s.set_return_latency(d.synth.0, DELAY_MS as f32).unwrap();
    s.track_synth(d.seq.0, d.part.0, 2, false, true, true)
        .unwrap();
    let mut spec = spec();
    spec.looped = true;
    s.record(d.seq.0, spec).unwrap();
    // One bar is 96 000 frames: a note in pass 1, then a beat and a half
    // into pass 2.
    wait_until("a little of pass 1", || frames(&s) >= 12_000);
    assert!(s.inject_midi("Keys".into(), vec![0x90, 70, 100]).unwrap());
    wait_until("an eighth on", || frames(&s) >= 24_000);
    assert!(s.inject_midi("Keys".into(), vec![0x80, 70, 0]).unwrap());
    wait_until("pass 2", || {
        let r = s.record_state().unwrap();
        r.pass >= 2 && frames(&s) >= 96_000 + 36_000
    });
    s.stop().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    let lane = |id: LaneId| {
        s.lanes(d.seq.0)
            .unwrap()
            .into_iter()
            .find(|l| l.id == id.0)
            .unwrap()
    };
    let (part, rec) = (lane(d.part), lane(d.rec));
    assert_eq!(part.take_count, rec.take_count, "matching takes");
    assert_eq!(rec.take_count, 3, "one take per pass, besides the first");
    assert!(part.mute, "muted after recording");
    s.undo().unwrap();
    let (part, rec) = (lane(d.part), lane(d.rec));
    assert!(!part.mute, "the mute is part of the Record step");
    assert_eq!((part.take_count, rec.take_count), (1, 1));
    s.close().unwrap();
}

/// Plan 5c-2 Ruling 2 over the FFI: return latencies outside 0–500 ms,
/// not finite, or for a destination that is not an external synth are
/// refused and change nothing.
#[test]
fn hostile_return_latencies_are_refused() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let mut b = Builder::from_doc(d.doc.clone());
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let s = untitled(&g, b.finish());
    let v = s.snapshot().unwrap().version;
    for (dest, ms) in [
        (d.synth.0, -1.0f32),
        (d.synth.0, 500.5),
        (d.synth.0, f32::NAN),
        (d.synth.0, f32::INFINITY),
        (piano.0, 10.0),
        (999, 10.0),
    ] {
        assert!(s.set_return_latency(dest, ms).is_err(), "{dest} {ms}");
    }
    assert_eq!(s.snapshot().unwrap().version, v);
    let dests = s.destinations().unwrap();
    assert!(dests.iter().all(|x| x.return_latency_ms == 0.0));
    assert!(dests.iter().any(|x| x.id == piano.0 && !x.external));
    s.close().unwrap();
}

fn lane_named(s: &Session, seq: SeqId, name: &str) -> vizdaw_ffi::LaneRow {
    s.lanes(seq.0)
        .unwrap()
        .into_iter()
        .find(|l| l.name == name)
        .unwrap()
}

fn set_input(s: &Session, seq: SeqId, lane: u64, input: Option<LaneInputRow>) {
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq: seq.0,
                lane,
                input,
            },
        },
        None,
    )
    .unwrap();
}

/// Task 2 carry (plan 5c-2 Task 8): a row's `return_of` is the lane's
/// link, so a `SetInput` without one leaves plain audio, unlinked.
#[test]
fn a_set_input_without_a_return_unlinks_the_lane() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let input = |s: &Session| lane_named(s, d.seq, "Rec").input;
    assert_eq!(
        input(&s),
        Some(LaneInputRow::Audio {
            first: 2,
            stereo: false,
            return_of: Some(d.synth.0),
        })
    );
    let plain = LaneInputRow::Audio {
        first: 3,
        stereo: true,
        return_of: None,
    };
    set_input(&s, d.seq, d.rec.0, Some(plain.clone()));
    assert_eq!(input(&s), Some(plain), "unlinked");
    let linked = LaneInputRow::Audio {
        first: 3,
        stereo: true,
        return_of: Some(d.synth.0),
    };
    set_input(&s, d.seq, d.rec.0, Some(linked.clone()));
    assert_eq!(input(&s), Some(linked), "linked again");
    s.close().unwrap();
}

/// Task 8 ruling M10: "Mute the MIDI lane after recording" does not
/// outlive an undo of the Track Synth that asked for it, even when a
/// plain lane later takes the undone return lane's ID and records.
#[test]
fn an_undone_track_synth_mutes_nothing_after_recording() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(false, 2);
    let s = untitled(&g, d.doc.clone());
    s.track_synth(d.seq.0, d.part.0, 2, false, false, true)
        .unwrap();
    let undone = lane_named(&s, d.seq, "Fake Synth audio").id;
    s.undo().unwrap();
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Add {
                seq: d.seq.0,
                name: "Again".into(),
                index: None,
            },
        },
        None,
    )
    .unwrap();
    let again = lane_named(&s, d.seq, "Again").id;
    assert_eq!(again, undone, "the undone lane's ID comes back");
    set_input(
        &s,
        d.seq,
        again,
        Some(LaneInputRow::Audio {
            first: 2,
            stereo: false,
            return_of: None,
        }),
    );
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: d.seq.0,
                lane: again,
                mute: None,
                solo: None,
                arm: Some(true),
            },
        },
        None,
    )
    .unwrap();
    record_to_the_third_note(&s, d.seq);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    assert_eq!(
        regions(&s, d.seq.0, again).len(),
        1,
        "the pass placed audio there"
    );
    assert!(
        !lane_named(&s, d.seq, "Part").mute,
        "the MIDI lane is not muted"
    );
    s.close().unwrap();
}

/// Task 8 review M1: with an edit open (a drag), Track Synth is refused
/// and changes nothing; the next gesture's restart would otherwise drop
/// it after it had answered Ok.
#[test]
fn track_synth_waits_for_the_open_edit() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(false, 2);
    let s = untitled(&g, d.doc.clone());
    s.begin_edit("Drag".into()).unwrap();
    let v = s.snapshot().unwrap().version;
    let e = s
        .track_synth(d.seq.0, d.part.0, 2, false, false, true)
        .unwrap_err();
    assert!(e.to_string().contains("open edit"), "{e}");
    assert_eq!(s.snapshot().unwrap().version, v, "nothing changed");
    s.end_edit().unwrap();
    assert_eq!(s.lanes(d.seq.0).unwrap().len(), 2, "no lane added");
    assert_ne!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Track Synth")
    );
    s.close().unwrap();
}

/// Task 8 review M3, plan 5c-2 Ruling 14: Track Synth is `Busy` while a
/// recording runs and changes nothing.
#[test]
fn track_synth_is_busy_while_recording() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    s.record(d.seq.0, spec()).unwrap();
    let before = s.lanes(d.seq.0).unwrap();
    let e = s
        .track_synth(d.seq.0, d.part.0, 4, true, true, true)
        .unwrap_err();
    assert!(matches!(e, vizdaw_ffi::VizError::Busy { .. }), "{e:?}");
    assert_eq!(s.lanes(d.seq.0).unwrap(), before, "nothing changed");
    s.stop().unwrap();
    assert!(!lane_named(&s, d.seq, "Part").mute, "no mute was asked for");
    s.close().unwrap();
}

/// Task 8 review M3: a redone Track Synth keeps its "mute after": the
/// next recording mutes the MIDI lane.
#[test]
fn a_redone_track_synth_mutes_after_recording() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(false, 2);
    let s = untitled(&g, d.doc.clone());
    s.track_synth(d.seq.0, d.part.0, 2, false, false, true)
        .unwrap();
    s.undo().unwrap();
    s.redo().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Track Synth")
    );
    let ret = lane_named(&s, d.seq, "Fake Synth audio").id;
    record_to_the_third_note(&s, d.seq);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    assert_eq!(
        regions(&s, d.seq.0, ret).len(),
        1,
        "the pass placed audio there"
    );
    assert!(lane_named(&s, d.seq, "Part").mute, "the MIDI lane is muted");
    s.close().unwrap();
}
