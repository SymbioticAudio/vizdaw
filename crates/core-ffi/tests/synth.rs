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
use core_model::mixer::{PluginFormat, PluginRef, StripKind};
use core_model::sequence::SeqLength;
use core_model::time::Tick;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{
    force_offline, midi_destination_targets, reset_host, set_fake_input, set_fake_synth,
};
use vizdaw_ffi::{
    EditCommand, EventRow, LaneEdit, LaneInputRow, RecordSpecRow, SequenceEdit, Session,
    SlotStatus, VizError,
};

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
    // A test that set a trim must not shift the next (5c-2 final review).
    vizdaw_ffi::set_record_latency_trim(0);
    // A test that failed holding a measurement must not hang the next.
    vizdaw_ffi::host::debug_hold_measure(false);
    vizdaw_ffi::host::debug_measure_end_delay(Duration::ZERO);
    vizdaw_ffi::host::debug_input_open_delay(None);
    vizdaw_ffi::host::set_fake_microphone_prompt(None);
    vizdaw_ffi::host::debug_hold_microphone_prompt(false);
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
/// it after it had answered Ok. Task 10 review I1: the refusal is `Busy`,
/// as Measure's is (a state that passes, not bad input).
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
    assert!(
        matches!(&e, VizError::Busy { message } if message.contains("open edit")),
        "{e:?}"
    );
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

fn return_latency(s: &Session, dest: DestId) -> f32 {
    s.destinations()
        .unwrap()
        .into_iter()
        .find(|x| x.id == dest.0)
        .unwrap()
        .return_latency_ms
}

/// Points lane `rec`'s return at `synth` on device input `first`.
fn listen(s: &Session, d: &Doc, first: u16) {
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq: d.seq.0,
                lane: d.rec.0,
                input: Some(LaneInputRow::Audio {
                    first,
                    stereo: false,
                    return_of: Some(d.synth.0),
                }),
            },
        },
        None,
    )
    .unwrap();
}

/// Spec §6.7, §12.3: Measure finds the fake delay within ±1 ms with the
/// transport stopped, and sets it as one undo step.
#[test]
fn measure_finds_the_fake_delay_and_sets_it() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let m = s.measure_return(d.synth.0).unwrap();
    assert!(
        (m.return_latency_ms - DELAY_MS as f32).abs() <= 1.0,
        "{m:?}"
    );
    assert_eq!(m.onsets, 5);
    assert_eq!(m.snapshot.undo_label.as_deref(), Some("Set Return Latency"));
    assert_eq!(return_latency(&s, d.synth), m.return_latency_ms);
    assert!(
        !s.transport().unwrap().playing,
        "the transport stayed stopped"
    );
    assert!(!vizdaw_ffi::host::measuring());
    s.undo().unwrap();
    assert_eq!(return_latency(&s, d.synth), 0.0);
    // Measured, then recorded: the clicks land on their notes.
    s.redo().unwrap();
    record_to_the_third_note(&s, d.seq);
    for (at, note) in clicks(&s, d.seq.0, d.rec.0)
        .iter()
        .zip([BEAT, 2 * BEAT, 3 * BEAT])
    {
        assert!(
            (at - note).abs() <= MS,
            "a click at {at}, its note at {note}"
        );
    }
    s.close().unwrap();
}

/// Spec §6.7, Review Focus 1: a return input with no clean onset (here
/// fake input 1, a sine that never falls quiet) fails "No signal on the
/// synth's return input" and changes nothing; nothing is left installed.
#[test]
fn measure_fails_cleanly_without_a_clean_onset() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    listen(&s, &d, 0);
    let v = s.snapshot().unwrap().version;
    match s.measure_return(d.synth.0) {
        Err(VizError::Model { message }) => assert!(
            message.contains("No signal on the synth's return input"),
            "{message}"
        ),
        other => panic!("{other:?}"),
    }
    assert_eq!(s.snapshot().unwrap().version, v);
    assert_eq!(return_latency(&s, d.synth), 0.0);
    assert!(!vizdaw_ffi::host::measuring());
    s.play(d.seq.0).unwrap();
    s.stop().unwrap();
    s.close().unwrap();
}

/// Plan 5c-2 Rulings 2 and 9 (and Task 5: the engine leaves these checks
/// to its caller): Measure refuses a destination that is not an external
/// synth (the built-in synth included), one no lane records the return
/// of, an unknown one, a return channel the input lacks, and anything
/// while the transport plays or records; afterwards it still works.
#[test]
fn measure_refuses_what_it_cannot_measure() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let mut b = Builder::from_doc(d.doc.clone());
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let juno = b.dest("Juno", "Juno", 0).unwrap();
    let s = untitled(&g, b.finish());
    let model = |r: Result<vizdaw_ffi::synth::MeasureRow, VizError>, needle: &str| match r {
        Err(VizError::Model { message }) => {
            assert!(message.contains(needle), "{needle}: {message}")
        }
        other => panic!("{needle}: {other:?}"),
    };
    model(
        s.measure_return(piano.0),
        "not an external MIDI destination",
    );
    model(
        s.measure_return(juno.0),
        "no lane records the return of Juno",
    );
    model(s.measure_return(999), "unknown id dest999");
    assert!(!vizdaw_ffi::host::measuring());
    listen(&s, &d, 8);
    model(
        s.measure_return(d.synth.0),
        "the audio input has 3 channel(s)",
    );
    assert!(!vizdaw_ffi::host::measuring());
    listen(&s, &d, 2);
    s.play(d.seq.0).unwrap();
    assert!(matches!(
        s.measure_return(d.synth.0),
        Err(VizError::Busy { .. })
    ));
    s.stop().unwrap();
    wait_until("the transport to stop", || !s.transport().unwrap().playing);
    s.record(d.seq.0, spec()).unwrap();
    assert!(matches!(
        s.measure_return(d.synth.0),
        Err(VizError::Busy { .. })
    ));
    s.stop().unwrap();
    wait_until("the transport to stop", || !s.transport().unwrap().playing);
    assert!(s.measure_return(d.synth.0).is_ok());
    s.close().unwrap();
}

/// Plan 5c-2 Ruling 9: while a measurement runs, Play, Record and another
/// window's Measure are Busy; the measurement then finishes. The
/// measurement is held past its end (pre-flight M5) so the calls cannot
/// miss its window.
#[test]
fn play_and_record_wait_for_a_measurement() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let other = untitled(&g, d.doc.clone());
    let s2 = Arc::clone(&s);
    let dest = d.synth.0;
    vizdaw_ffi::host::debug_hold_measure(true);
    let t = std::thread::spawn(move || s2.measure_return(dest));
    wait_until("the measurement to start", vizdaw_ffi::host::measuring);
    let busy = (
        s.play(d.seq.0),
        s.record(d.seq.0, spec()),
        other.measure_return(dest),
        other.play(d.seq.0),
    );
    vizdaw_ffi::host::debug_hold_measure(false);
    let m = t.join().unwrap().unwrap();
    assert!(matches!(busy.0, Err(VizError::Busy { .. })), "{:?}", busy.0);
    assert!(matches!(busy.1, Err(VizError::Busy { .. })), "{:?}", busy.1);
    assert!(matches!(busy.2, Err(VizError::Busy { .. })), "{:?}", busy.2);
    assert!(matches!(busy.3, Err(VizError::Busy { .. })), "{:?}", busy.3);
    assert_eq!(m.onsets, 5);
    assert!(!vizdaw_ffi::host::measuring());
    other.close().unwrap();
    s.close().unwrap();
}

/// Pre-flight I1: a measurement never outlives its sequence. Deleting
/// the measured sequence mid-measure ends the measurement (it fails),
/// and every session can play again.
#[test]
fn a_measurement_never_outlives_its_sequence() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let mut b = Builder::from_doc(d.doc.clone());
    b.sequence("T", "Lane", None).unwrap();
    let s = untitled(&g, b.finish());
    let other = untitled(&g, d.doc.clone());
    let s2 = Arc::clone(&s);
    let dest = d.synth.0;
    // Held, so the delete always lands mid-measure (review M5).
    vizdaw_ffi::host::debug_hold_measure(true);
    let t = std::thread::spawn(move || s2.measure_return(dest));
    wait_until("the measurement to start", vizdaw_ffi::host::measuring);
    s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::Delete {
                seq: d.seq.0,
                delete_references: false,
            },
        },
        None,
    )
    .unwrap();
    wait_until("the input to close", || {
        vizdaw_ffi::host::debug_host_input_open() == Some(false)
    });
    vizdaw_ffi::host::debug_hold_measure(false);
    let r = t.join().unwrap();
    assert!(matches!(r, Err(VizError::Engine { .. })), "{r:?}");
    assert!(!vizdaw_ffi::host::measuring());
    other.play(d.seq.0).unwrap();
    other.stop().unwrap();
    other.close().unwrap();
    s.close().unwrap();
}

/// Pre-flight M1: measuring holds the input open. With the return lane
/// disarmed (so no lane wants input), an edit mid-measure does not close
/// it; it closes once the measurement ends.
#[test]
fn measuring_holds_the_input_open() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let mut b = Builder::from_doc(d.doc.clone());
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq: d.seq,
        lane: d.rec,
        mute: None,
        solo: None,
        arm: Some(false),
    }))
    .unwrap();
    let s = untitled(&g, b.finish());
    let s2 = Arc::clone(&s);
    let dest = d.synth.0;
    vizdaw_ffi::host::debug_hold_measure(true);
    let t = std::thread::spawn(move || s2.measure_return(dest));
    wait_until("the measurement to start", vizdaw_ffi::host::measuring);
    assert_eq!(s.debug_input_open(), Some(true));
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Rename {
                seq: d.seq.0,
                lane: d.part.0,
                name: "Synth Part".into(),
            },
        },
        None,
    )
    .unwrap();
    let held = s.debug_input_open();
    vizdaw_ffi::host::debug_hold_measure(false);
    let m = t.join().unwrap();
    assert_eq!(held, Some(true), "the edit closed the input mid-measure");
    assert_eq!(m.unwrap().onsets, 5);
    wait_until("the input to close", || s.debug_input_open() == Some(false));
    s.close().unwrap();
}

/// Arms (or disarms) lane `lane` of `seq`.
fn arm(s: &Session, seq: SeqId, lane: LaneId, on: bool) {
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: seq.0,
                lane: lane.0,
                mute: None,
                solo: None,
                arm: Some(on),
            },
        },
        None,
    )
    .unwrap();
}

/// Measures with `rec` armed `armed`, flipping its arm mid-measure;
/// returns the input's state after the measurement and how many times it
/// changed from the measurement's start to then.
fn flip_arm_mid_measure(armed: bool) -> (Option<bool>, u64) {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let mut b = Builder::from_doc(d.doc.clone());
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq: d.seq,
        lane: d.rec,
        mute: None,
        solo: None,
        arm: Some(armed),
    }))
    .unwrap();
    let s = untitled(&g, b.finish());
    let s2 = Arc::clone(&s);
    let dest = d.synth.0;
    vizdaw_ffi::host::debug_hold_measure(true);
    let t = std::thread::spawn(move || s2.measure_return(dest));
    wait_until("the measurement to start", vizdaw_ffi::host::measuring);
    let before = s.debug_input_changes().unwrap();
    arm(&s, d.seq, d.rec, !armed);
    vizdaw_ffi::host::debug_hold_measure(false);
    let m = t.join().unwrap();
    assert_eq!(m.unwrap().onsets, 5);
    wait_until("the input to settle", || !s.input_info().unwrap().opening);
    let out = (
        s.debug_input_open(),
        s.debug_input_changes().unwrap() - before,
    );
    s.close().unwrap();
    out
}

/// Review I2: a lane armed mid-measure keeps the input open once the
/// measurement ends, without a single input change.
#[test]
fn a_lane_armed_mid_measure_keeps_the_input_open() {
    assert_eq!(flip_arm_mid_measure(false), (Some(true), 0));
}

/// Review I2: a lane disarmed mid-measure closes the input once the
/// measurement ends, in one input change.
#[test]
fn a_lane_disarmed_mid_measure_closes_the_input_once() {
    assert_eq!(flip_arm_mid_measure(true), (Some(false), 1));
}

/// Review I1: an edit opened while Measure waits does not absorb its
/// step: Measure is refused, naming what it measured, and the edit's
/// undo step holds only the edit.
#[test]
fn an_edit_opened_mid_measure_refuses_the_measured_step() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let s2 = Arc::clone(&s);
    let dest = d.synth.0;
    vizdaw_ffi::host::debug_hold_measure(true);
    let t = std::thread::spawn(move || s2.measure_return(dest));
    wait_until("the measurement to start", vizdaw_ffi::host::measuring);
    s.begin_edit("Drag".into()).unwrap();
    s.set_lane_mute(d.seq.0, d.part.0, true).unwrap();
    vizdaw_ffi::host::debug_hold_measure(false);
    let r = t.join().unwrap();
    match r {
        Err(VizError::Busy { message }) => assert!(
            message.contains("open edit") && message.contains("(measured 25.0 ms)"),
            "{message}"
        ),
        other => panic!("{other:?}"),
    }
    assert!(!vizdaw_ffi::host::measuring());
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Drag"));
    assert_eq!(return_latency(&s, d.synth), 0.0);
    s.undo().unwrap();
    assert!(!lane_named(&s, d.seq, "Part").mute);
    assert_eq!(return_latency(&s, d.synth), 0.0);
    assert_eq!(s.snapshot().unwrap().undo_label, None);
    s.close().unwrap();
}

/// Review M4 (pre-flight I1): a window closed mid-measure ends its
/// measurement; every other session can play again.
#[test]
fn closing_a_window_ends_its_measurement() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let other = untitled(&g, d.doc.clone());
    let s2 = Arc::clone(&s);
    let dest = d.synth.0;
    vizdaw_ffi::host::debug_hold_measure(true);
    let t = std::thread::spawn(move || s2.measure_return(dest));
    wait_until("the measurement to start", vizdaw_ffi::host::measuring);
    s.close().unwrap();
    assert!(!vizdaw_ffi::host::measuring());
    // No one holds the engine: the input Measure opened is closed again
    // (re-review minor).
    wait_until("the input to close", || {
        vizdaw_ffi::host::debug_host_input_open() == Some(false)
    });
    vizdaw_ffi::host::debug_hold_measure(false);
    let r = t.join().unwrap();
    assert!(matches!(r, Err(VizError::Engine { .. })), "{r:?}");
    assert!(!vizdaw_ffi::host::measuring());
    other.play(d.seq.0).unwrap();
    other.stop().unwrap();
    other.close().unwrap();
}

/// Re-review Important: a measurement that finished is not taken for one
/// stopped early because its own removal landed before the result was
/// read (a delay hook stands for a descheduled thread).
#[test]
fn a_finished_measurement_survives_its_own_removal() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    vizdaw_ffi::host::debug_measure_end_delay(Duration::from_millis(200));
    let m = s.measure_return(d.synth.0);
    vizdaw_ffi::host::debug_measure_end_delay(Duration::ZERO);
    assert_eq!(m.unwrap().onsets, 5);
    s.close().unwrap();
}

/// 5c-2 final review carry (plan 5d-1 Task 6): the latency trim set for
/// the whole app reaches the engine: a measurement made with a trim of 96
/// frames (2 ms) reads 2 ms more. `serial` puts the trim back to 0.
#[test]
fn the_latency_trim_reaches_the_engine() {
    let g = serial(Some(DELAY_MS), None);
    vizdaw_ffi::set_record_latency_trim(96);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let m = s.measure_return(d.synth.0);
    vizdaw_ffi::set_record_latency_trim(0);
    let m = m.unwrap();
    assert!(
        (m.return_latency_ms - (DELAY_MS as f32 + 2.0)).abs() <= 0.25,
        "{m:?}"
    );
    s.close().unwrap();
}

/// Pre-flight M4: the I/O buffer size waits for a measurement too, with
/// its own words, and keeps the old size.
#[test]
fn the_buffer_size_waits_for_a_measurement() {
    let g = serial(Some(DELAY_MS), None);
    vizdaw_ffi::audio::reset_audio_settings();
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let s2 = Arc::clone(&s);
    let dest = d.synth.0;
    vizdaw_ffi::host::debug_hold_measure(true);
    let t = std::thread::spawn(move || s2.measure_return(dest));
    wait_until("the measurement to start", vizdaw_ffi::host::measuring);
    let r = vizdaw_ffi::audio::set_io_buffer_frames(256);
    let kept = vizdaw_ffi::audio::audio_settings().buffer_frames;
    vizdaw_ffi::host::debug_hold_measure(false);
    t.join().unwrap().unwrap();
    assert!(
        matches!(&r, Err(VizError::Busy { message }) if message == "Wait for the measurement to finish."),
        "{r:?}"
    );
    assert_eq!(kept, 128, "unchanged");
    s.close().unwrap();
}

/// Arms one MIDI lane, `lane`, in `s` (an edit that claims an idle engine,
/// plan 5d-1 Ruling 16).
fn arm_lane(s: &Session, seq: SeqId, lane: LaneId) {
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: seq.0,
                lane: lane.0,
                mute: None,
                solo: None,
                arm: Some(true),
            },
        },
        None,
    )
    .unwrap();
}

/// Plan 5d-1 Ruling 16 (Task 8 review m4): an arm in another window never
/// takes a measuring engine.
#[test]
fn an_arm_never_takes_a_measuring_engine() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let other = untitled(&g, d.doc.clone());
    let s2 = Arc::clone(&s);
    let dest = d.synth.0;
    vizdaw_ffi::host::debug_hold_measure(true);
    let t = std::thread::spawn(move || s2.measure_return(dest));
    wait_until("the measurement to start", vizdaw_ffi::host::measuring);
    let owner = vizdaw_ffi::host::holder();
    arm_lane(&other, d.seq, d.part);
    let kept = (vizdaw_ffi::host::holder(), vizdaw_ffi::host::measuring());
    vizdaw_ffi::host::debug_hold_measure(false);
    let m = t.join().unwrap().unwrap();
    assert_eq!(kept, (owner, true), "a measuring engine is not taken");
    assert_eq!(m.onsets, 5);
    other.close().unwrap();
    s.close().unwrap();
}

/// Task 8 review m1: while Measure waits for its input to open (no
/// measurement installed yet), an arm in another window does not take the
/// engine, so the measurement hears its input.
#[test]
fn an_arm_never_takes_the_engine_during_measures_input_wait() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let other = untitled(&g, d.doc.clone());
    let s2 = Arc::clone(&s);
    let dest = d.synth.0;
    vizdaw_ffi::host::debug_input_open_delay(Some(Duration::from_millis(500)));
    let t = std::thread::spawn(move || s2.measure_return(dest));
    wait_until("the input to be opening", vizdaw_ffi::host::debug_input_job);
    let owner = vizdaw_ffi::host::holder();
    arm_lane(&other, d.seq, d.part);
    let kept = vizdaw_ffi::host::holder();
    let m = t.join().unwrap();
    vizdaw_ffi::host::debug_input_open_delay(None);
    assert_eq!(kept, owner, "the engine Measure waits on is not taken");
    assert_eq!(m.unwrap().onsets, 5);
    other.close().unwrap();
    s.close().unwrap();
}

/// Task 7 parked r1: a Measure refused because its input is still
/// opening, on an idle engine another window holds, leaves that window's
/// input as it asked (here closed: nothing of it is armed), not open for
/// nobody.
#[test]
fn a_measure_refused_while_opening_leaves_the_holders_input_alone() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let holder = untitled(&g, d.doc.clone());
    arm(&holder, d.seq, d.rec, false);
    holder.play(d.seq.0).unwrap();
    wait_until("playing", || holder.transport().unwrap().playing);
    holder.stop().unwrap();
    wait_until("stopped", || !holder.transport().unwrap().playing);
    wait_until("the holder's input to settle", || {
        !vizdaw_ffi::host::debug_input_job()
    });
    assert_eq!(holder.debug_input_open(), Some(false));
    let s = untitled(&g, d.doc.clone());
    // An open that cannot finish within Measure's wait.
    vizdaw_ffi::host::set_fake_microphone_prompt(Some(true));
    vizdaw_ffi::host::debug_hold_microphone_prompt(true);
    let r = s.measure_return(d.synth.0);
    vizdaw_ffi::host::debug_hold_microphone_prompt(false);
    vizdaw_ffi::host::set_fake_microphone_prompt(None);
    assert!(
        matches!(&r, Err(VizError::Busy { message }) if message.contains("still opening")),
        "{r:?}"
    );
    wait_until("the input to settle", || {
        !vizdaw_ffi::host::debug_input_job()
    });
    assert_eq!(
        holder.debug_input_open(),
        Some(false),
        "left open for nobody"
    );
    s.close().unwrap();
    holder.close().unwrap();
}

/// Plan 5d-1 Ruling 25 ("New MIDI Destination…", 5c-2 carry): the ports
/// a new destination may play (offline: the fake synth's), and the
/// destination as one undo step.
#[test]
fn a_new_midi_destination_names_a_port_in_one_step() {
    let g = serial(Some(DELAY_MS), None);
    assert_eq!(
        vizdaw_ffi::host::midi_destination_ports(),
        vec![FAKE_SYNTH_PORT.to_string()]
    );
    let s = untitled(&g, Builder::new().finish());
    let before = s.destinations().unwrap().len();
    let snap = s
        .add_midi_destination("Synth A".into(), FAKE_SYNTH_PORT.into(), 2)
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Add Destination"));
    let d = s
        .destinations()
        .unwrap()
        .into_iter()
        .find(|d| d.name == "Synth A")
        .unwrap();
    assert!(d.external && d.plugin.is_none());
    s.undo().unwrap();
    assert_eq!(s.destinations().unwrap().len(), before);
    s.close().unwrap();
    set_fake_synth(None);
    assert!(vizdaw_ffi::host::midi_destination_ports().is_empty());
}

/// Review Focus 4: hostile names, ports and channels are `Model` and add
/// nothing. A port must be one `midi_destination_ports` lists: an unknown
/// one, or a reserved reference (the built-in synth, the virtual output),
/// is refused.
#[test]
fn hostile_midi_destinations_are_refused() {
    let g = serial(Some(DELAY_MS), None);
    let s = untitled(&g, Builder::new().finish());
    let long = "x".repeat(256);
    for (name, port, channel) in [
        ("", FAKE_SYNTH_PORT, 0u8),
        ("   ", FAKE_SYNTH_PORT, 0),
        (long.as_str(), FAKE_SYNTH_PORT, 0),
        ("a\nb", FAKE_SYNTH_PORT, 0),
        ("Synth", "", 0),
        ("Synth", long.as_str(), 0),
        ("Synth", "Port\u{7}", 0),
        ("Synth", "No Such Port", 0),
        ("Synth", "builtin:synth", 0),
        ("Synth", "virtual:out", 0),
        ("Synth", "   ", 0),
        // Matched as given: no listed port has edge spaces.
        ("Synth", " Fake Synth ", 0),
        ("Synth", FAKE_SYNTH_PORT, 16),
        ("Synth", FAKE_SYNTH_PORT, 255),
    ] {
        assert!(
            matches!(
                s.add_midi_destination(name.into(), port.into(), channel),
                Err(VizError::Model { .. })
            ),
            "{name:?} {port:?} {channel}"
        );
    }
    assert!(s.destinations().unwrap().is_empty());
    assert!(s.snapshot().unwrap().undo_label.is_none());
    // The channel's refusal names both numberings (preflight M10).
    match s.add_midi_destination("Synth".into(), FAKE_SYNTH_PORT.into(), 16) {
        Err(VizError::Model { message }) => assert!(
            message.contains("the MIDI channel is 0 to 15 (MIDI channels 1 to 16)"),
            "{message}"
        ),
        other => panic!("{other:?}"),
    }
    // With the fake synth off and no host running (nothing here played),
    // its port is unknown too.
    set_fake_synth(None);
    assert!(matches!(
        s.add_midi_destination("Synth".into(), FAKE_SYNTH_PORT.into(), 0),
        Err(VizError::Model { .. })
    ));
    assert!(s.destinations().unwrap().is_empty());
    s.close().unwrap();
}

/// Plan 5d-1 Ruling 25, Review Focus 4: the same name, port and channel
/// twice make two destinations, one undo step each; they share the port's
/// one target, which stays while either names the port (Ruling 23,
/// preflight M16).
#[test]
fn duplicate_midi_destinations_share_one_target() {
    let g = serial(Some(DELAY_MS), None);
    let mut b = Builder::new();
    let (seq, rec) = b.sequence("S", "Rec", None).unwrap();
    let s = untitled(&g, b.finish());
    for _ in 0..2 {
        let snap = s
            .add_midi_destination("Synth".into(), FAKE_SYNTH_PORT.into(), 0)
            .unwrap();
        assert_eq!(snap.undo_label.as_deref(), Some("Add Destination"));
    }
    let ids: Vec<u64> = s.destinations().unwrap().iter().map(|d| d.id).collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    set_input(
        &s,
        seq,
        rec.0,
        Some(LaneInputRow::Audio {
            first: 2,
            stereo: false,
            return_of: Some(ids[0]),
        }),
    );
    s.play(seq.0).unwrap();
    assert_eq!(
        midi_destination_targets(),
        vec![FAKE_SYNTH_PORT.to_string()]
    );
    s.stop().unwrap();
    // The lane's return goes, then one of the two: the other still names
    // the port.
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(s.destinations().unwrap().len(), 1);
    assert_eq!(
        midi_destination_targets(),
        vec![FAKE_SYNTH_PORT.to_string()]
    );
    s.undo().unwrap();
    assert!(s.destinations().unwrap().is_empty());
    assert!(midi_destination_targets().is_empty());
    s.close().unwrap();
}

/// Plan 5d-1 Ruling 23 (5c-2 final review finding 6): a MIDI-destination
/// target is released at the load after no destination names its port.
#[test]
fn unused_midi_destination_targets_are_released() {
    let g = serial(Some(DELAY_MS), None);
    let mut b = Builder::new();
    let (seq, rec) = b.sequence("S", "Rec", None).unwrap();
    let s = untitled(&g, b.finish());
    let snap = s
        .add_midi_destination("Synth".into(), FAKE_SYNTH_PORT.into(), 0)
        .unwrap();
    let dest = s
        .destinations()
        .unwrap()
        .into_iter()
        .find(|d| d.name == "Synth")
        .unwrap()
        .id;
    assert_eq!(snap.undo_label.as_deref(), Some("Add Destination"));
    // A lane records its return: every load interns its target.
    set_input(
        &s,
        seq,
        rec.0,
        Some(LaneInputRow::Audio {
            first: 2,
            stereo: false,
            return_of: Some(dest),
        }),
    );
    s.play(seq.0).unwrap();
    assert_eq!(
        midi_destination_targets(),
        vec![FAKE_SYNTH_PORT.to_string()]
    );
    s.stop().unwrap();
    s.undo().unwrap();
    s.undo().unwrap();
    assert!(s.destinations().unwrap().is_empty());
    assert!(
        midi_destination_targets().is_empty(),
        "nothing names the port: {:?}",
        midi_destination_targets()
    );
    s.close().unwrap();
}

/// Plan 5d-1 Ruling 22 (5c-2 final review finding 12): a measurement
/// waits for the input to open and applies its step. Since Task 7 the
/// input change after a measurement runs on the input thread, so its
/// failure is asynchronous (a host fault) and never takes the measurement
/// with it (preflight M8: this pins the wait and the step, not a failure).
#[test]
fn a_measurement_waits_for_the_input_to_open_and_applies_its_step() {
    let g = serial(Some(DELAY_MS), None);
    vizdaw_ffi::host::debug_input_open_delay(Some(Duration::from_millis(200)));
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let m = s.measure_return(d.synth.0);
    vizdaw_ffi::host::debug_input_open_delay(None);
    let m = m.unwrap();
    assert_eq!(m.snapshot.undo_label.as_deref(), Some("Set Return Latency"));
    assert!(
        (m.return_latency_ms - DELAY_MS as f32).abs() <= 1.0,
        "{m:?}"
    );
    // The return lane is armed: the input stays open, once settled.
    wait_until("the input to settle", || !s.input_info().unwrap().opening);
    assert_eq!(s.debug_input_open(), Some(true));
    s.close().unwrap();
}

/// Task 11 review m3: with a host running, the ports are the ones its
/// loads resolve against, whatever the fake-synth setting says now; with
/// none, the setting's.
#[test]
fn the_ports_are_the_running_hosts() {
    let g = serial(Some(DELAY_MS), None);
    let mut b = Builder::new();
    let (seq, _) = b.sequence("S", "L", None).unwrap();
    let s = untitled(&g, b.finish());
    s.play(seq.0).unwrap();
    set_fake_synth(None);
    assert_eq!(
        vizdaw_ffi::host::midi_destination_ports(),
        vec![FAKE_SYNTH_PORT.to_string()],
        "the running host still plays the fake synth"
    );
    s.add_midi_destination("Synth".into(), FAKE_SYNTH_PORT.into(), 0)
        .unwrap();
    s.stop().unwrap();
    s.close().unwrap();
    reset_host();
    assert!(vizdaw_ffi::host::midi_destination_ports().is_empty());
}

/// Runs the plugin timer until `done` (10 s at most).
fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let start = Instant::now();
    while !done(s) {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The engine's report lines for plugin slots that are placeholders.
fn placeholder_lines(s: &Session) -> Vec<String> {
    s.transport()
        .unwrap()
        .report
        .into_iter()
        .filter(|l| l.contains("loading") || l.contains("Test Gain"))
        .collect()
}

/// `d`'s document with a Test Gain insert on the master, and the insert.
fn with_gain(d: &Doc) -> (Document, core_model::ids::PluginInstanceId) {
    let mut b = Builder::from_doc(d.doc.clone());
    let master = b.strip_of(StripKind::Master).unwrap();
    let slot = b
        .insert(
            master,
            PluginRef {
                format: PluginFormat::Clap,
                id: "vizdaw.test.gain".into(),
                name: "Test Gain".into(),
                vendor: "VizDAW".into(),
                version: "1.0.0".into(),
            },
        )
        .unwrap();
    (b.finish(), slot)
}

/// Plan 5d-2 Task 8 review I1: Measure run off the thread that holds the
/// session's plugin host (the app's detached task) claims the engine with
/// no plugin host. It must not make the empty factory this document's:
/// after another window took the engine, the document's plugin plays
/// again at its next Play.
#[test]
fn a_measure_off_the_plugin_thread_keeps_the_documents_plugins() {
    let g = serial(Some(DELAY_MS), None);
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    let d = doc(true, 2);
    let (with_gain, slot) = with_gain(&d);
    let a = untitled(&g, with_gain);
    let live = |s: &Session| {
        s.plugin_slots()
            .unwrap()
            .iter()
            .any(|r| r.id == slot.0 && r.status == SlotStatus::Live)
    };
    let plays_its_plugin =
        |s: &Session| live(s) && placeholder_lines(s).is_empty() && s.transport().unwrap().playing;
    a.play(d.seq.0).unwrap();
    idle_until(&a, plays_its_plugin);
    a.stop().unwrap();
    let a_id = vizdaw_ffi::host::factory_owner().unwrap();
    // Another window (no plugins) takes the engine and its factory.
    let other = untitled(&g, d.doc.clone());
    other.play(d.seq.0).unwrap();
    other.stop().unwrap();
    assert_ne!(vizdaw_ffi::host::factory_owner(), Some(a_id));
    // A measures from a thread with no plugin host.
    let a2 = Arc::clone(&a);
    let dest = d.synth.0;
    let m = std::thread::spawn(move || a2.measure_return(dest))
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(m.onsets, 5);
    assert_ne!(
        vizdaw_ffi::host::factory_owner(),
        Some(a_id),
        "a claim with no plugin host does not take the factory"
    );
    // An idle claim with the plugin host (an arm, as monitoring needs)
    // brings the document's plugin back without a Play (re-review N1).
    arm(&a, d.seq, d.rec, false);
    arm(&a, d.seq, d.rec, true);
    assert_eq!(
        vizdaw_ffi::host::factory_owner(),
        Some(a_id),
        "the arm's claim installs A's factory"
    );
    idle_until(&a, |s| live(s) && placeholder_lines(s).is_empty());
    assert!(!a.transport().unwrap().playing);
    // A's next Play plays its plugin.
    a.play(d.seq.0).unwrap();
    idle_until(&a, plays_its_plugin);
    assert_eq!(vizdaw_ffi::host::factory_owner(), Some(a_id));
    a.stop().unwrap();
    other.close().unwrap();
    a.close().unwrap();
}

/// Plan 5d-2 Task 8 re-review N2: a claim with no plugin host keeps an
/// engine whose factory nobody owns, so Measures run off the plugin
/// thread one after another do not reopen the device each time.
#[test]
fn measures_off_the_plugin_thread_keep_the_engine() {
    let g = serial(Some(DELAY_MS), None);
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    let d = doc(true, 2);
    let (with_gain, _) = with_gain(&d);
    let a = untitled(&g, with_gain);
    let measure = || {
        let a2 = Arc::clone(&a);
        let dest = d.synth.0;
        let m = std::thread::spawn(move || a2.measure_return(dest))
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(m.onsets, 5);
    };
    measure();
    assert_eq!(vizdaw_ffi::host::factory_owner(), None);
    let created = vizdaw_ffi::host::hosts_created();
    measure();
    measure();
    assert_eq!(
        vizdaw_ffi::host::hosts_created(),
        created,
        "the same engine for each Measure"
    );
    a.close().unwrap();
}
