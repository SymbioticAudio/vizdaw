//! The record, step and click calls on the session seam (slice 8a Task
//! 1): round trips on a real session (the offline backend with the fake
//! input) and the fake's scripted passes.

use std::sync::{Arc, Mutex, MutexGuard, Once};
use std::time::{Duration, Instant};

use vizdaw_app::testing::{FakeCore, FakeSession, demo_fake, lane_row};
use vizdaw_app::{CoreCalls, DocSession, LiveCore};
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::edit::{EditCommand, LaneEdit, LaneInputRow};
use vizdaw_ffi::live::InputChannelsRow;
use vizdaw_ffi::step::{NoteValueRow, StepInputRow, StepNoteRow, StepResultRow};
use vizdaw_ffi::tracks::TrackKindRow;
use vizdaw_ffi::{
    ClickModeRow, ClickRow, InputInfoRow, OverviewRows, OverviewStateRow, RecordSpecRow, VizError,
    step_length, step_state_new,
};

/// The offline backend with the fake input, set once before any session
/// of this binary exists, and one real session at a time (the engine is
/// the process's).
fn offline() -> MutexGuard<'static, ()> {
    static ENV: Once = Once::new();
    static ONE: Mutex<()> = Mutex::new(());
    ENV.call_once(|| {
        // SAFETY: every test of this binary calls this before touching
        // the core, and the `Once` holds the others back until it is done,
        // so nothing reads the environment while it is written.
        unsafe {
            std::env::set_var("VIZDAW_AUDIO", "offline");
            std::env::set_var("VIZDAW_AUDIO_INPUT", "fake");
        }
    });
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

fn spec(looped: bool, count_in_bars: u8) -> RecordSpecRow {
    RecordSpecRow {
        replace: true,
        punch: None,
        looped,
        count_in_bars,
        pre_roll_bars: 0,
        passes_as_takes: true,
    }
}

fn model(m: &str) -> VizError {
    VizError::Model { message: m.into() }
}

fn busy(m: &str) -> VizError {
    VizError::Busy { message: m.into() }
}

fn song(s: &dyn DocSession) -> u64 {
    s.snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id
}

fn lane_id(s: &dyn DocSession, seq: u64, name: &str) -> u64 {
    s.lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == name)
        .unwrap()
        .id
}

fn arm_input(s: &dyn DocSession, seq: u64, lane: u64) {
    s.edit(EditCommand::Lane {
        edit: LaneEdit::SetInput {
            seq,
            lane,
            input: Some(LaneInputRow::Audio {
                first: 0,
                stereo: false,
                return_of: None,
            }),
        },
    })
    .unwrap();
    s.edit(EditCommand::Lane {
        edit: LaneEdit::SetFlags {
            seq,
            lane,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    })
    .unwrap();
}

fn text(name: &str, v: &str) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Text { value: v.into() },
    }
}

fn int(name: &str, v: i64) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Int { value: v },
    }
}

// ---- the real session -------------------------------------------------

/// Record, the pass readouts, Busy and the not-looping refusals, the
/// growing take, the input, the save-after-pass flags, and Stop landing
/// the take as "Record".
#[test]
fn a_pass_round_trips_on_a_real_session() {
    let _g = offline();
    let dir = tempfile::tempdir().unwrap();
    let s = LiveCore.new_demo_audio().unwrap();
    s.save_as(dir.path().join("Take.vizdaw").display().to_string())
        .unwrap();
    let seq = song(&*s);
    let lp = lane_id(&*s, seq, "Loop");
    let idle = s.record_state().unwrap();
    assert!(!idle.recording);
    assert_eq!(idle.pass, 0);
    assert!(s.recovered_recordings().unwrap().is_empty());
    assert_eq!(s.save_as_refusal().unwrap(), None);
    assert!(!s.saves_after_pass().unwrap());

    arm_input(&*s, seq, lp);
    s.record(seq, spec(false, 0)).unwrap();
    let r = s.record_state().unwrap();
    assert!(r.recording);
    assert_eq!(r.seq, Some(seq));
    assert_eq!(r.spec, Some(spec(false, 0)));
    assert_eq!(r.lanes.iter().map(|l| l.lane).collect::<Vec<_>>(), [lp]);
    assert_eq!(
        s.record(seq, spec(false, 0)).unwrap_err(),
        busy("A recording is already running.")
    );
    let not_looping = model("keep and discard act on loop passes; this recording does not loop");
    assert_eq!(s.keep_pass().unwrap_err(), not_looping);
    assert_eq!(s.discard_pass().unwrap_err(), not_looping);
    assert_eq!(s.play(seq).unwrap_err(), busy("Stop recording first."));
    assert_eq!(
        s.move_record_arm(seq, false).unwrap_err(),
        busy(vizdaw_ffi::session::RECORDING_LANES)
    );

    let until = Instant::now() + Duration::from_secs(10);
    let mut frames = 0;
    while frames == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
        frames = s.record_state().unwrap().frames;
    }
    assert!(frames > 0, "the fake input records");
    let w = s.recording_waveform(lp, 0, frames, 4).unwrap();
    assert_eq!(w.columns, 4);
    assert_eq!(
        s.recording_waveform(lp + 1000, 0, frames, 4).unwrap_err(),
        model(&format!(
            "track {} records no audio in this pass",
            lp + 1000
        ))
    );

    let input = s.input_info().unwrap();
    assert_eq!(input.device, "Offline");
    assert!(input.channels > 0);
    let levels = s.input_levels().unwrap();
    assert!(levels.len() <= input.channels as usize, "{levels:?}");
    s.save().unwrap();
    assert!(
        s.saves_after_pass().unwrap(),
        "a Save while recording is owed"
    );
    s.cancel_save_after_pass().unwrap();
    assert!(!s.saves_after_pass().unwrap());

    s.stop().unwrap();
    assert!(!s.record_state().unwrap().recording);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    assert_eq!(s.keep_pass().unwrap_err(), model("nothing is recording"));
    s.close().unwrap();
}

/// Move Record Arm, the click, step entry, a marker chore, MIDI
/// injection and the free functions.
#[test]
fn the_other_calls_round_trip_on_a_real_session() {
    let _g = offline();
    let s = LiveCore.new_demo_audio().unwrap();
    let seq = song(&*s);
    let lanes = s.lanes(seq).unwrap();

    s.move_record_arm(seq, true).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Move Record Arm")
    );
    assert!(s.lanes(seq).unwrap()[0].arm);
    assert_eq!(
        s.move_record_arm(seq, false).unwrap_err(),
        model("there is no track before the armed one")
    );

    let was = s.click().unwrap();
    s.run_click("transport.click".into(), vec![]).unwrap();
    assert_eq!(s.click().unwrap().on, !was.on);
    s.run_click(
        "transport.set_click_mode".into(),
        vec![text("mode", "record_only")],
    )
    .unwrap();
    assert_eq!(s.click().unwrap().mode, ClickModeRow::RecordOnly);
    assert!(matches!(
        s.run_click("transport.play".into(), vec![]),
        Err(VizError::Model { .. })
    ));

    let midi = lanes
        .iter()
        .find(|l| l.kind == TrackKindRow::Midi)
        .unwrap()
        .id;
    let state = LiveCore.step_state_new(0);
    assert_eq!(state.tick, 0);
    let note = StepNoteRow {
        pitch: 60,
        velocity: 40_000,
    };
    let r = s
        .step_entry(seq, midi, state, StepInputRow::Notes { notes: vec![note] })
        .unwrap();
    assert!(r.moved);
    assert_eq!(
        r.state.tick,
        step_length(NoteValueRow::Quarter, false, false)
    );
    assert_eq!(
        r.snapshot.and_then(|s| s.undo_label).as_deref(),
        Some("Step Entry")
    );
    assert_eq!(
        LiveCore.step_parse_pitches("60 64 67".into()).unwrap(),
        [60, 64, 67]
    );
    assert!(matches!(
        LiveCore.step_parse_pitches("not a pitch".into()),
        Err(VizError::Model { .. })
    ));

    s.run_chore(
        "marker.add".into(),
        vec![
            int("seq", seq as i64),
            int("tick", 7680),
            text("name", "Bridge"),
        ],
    )
    .unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Add Marker")
    );
    assert!(s.markers(seq).unwrap().iter().any(|m| m.name == "Bridge"));

    s.inject_midi("Keys".into(), vec![0x90, 60, 100]).unwrap();
    assert_eq!(
        s.inject_midi("Keys".into(), vec![0xF8]).unwrap_err(),
        model("byte 0xf8 does not start a channel message")
    );
    assert_eq!(
        LiveCore.audio_settings(),
        vizdaw_ffi::audio::audio_settings()
    );
    s.close().unwrap();
}

// ---- the fake -----------------------------------------------------------

/// The demo fake with Song's tracks: Bass (MIDI), Voice (audio, an input)
/// and Keys.
fn rec_fake() -> FakeSession {
    let f = demo_fake();
    let mut voice = lane_row(21, "Voice", false);
    voice.kind = TrackKindRow::Audio;
    voice.input = Some(LaneInputRow::Audio {
        first: 0,
        stereo: false,
        return_of: None,
    });
    f.set_lanes(
        3,
        vec![
            lane_row(20, "Bass", false),
            voice,
            lane_row(22, "Keys", false),
        ],
    );
    f
}

fn arm(f: &FakeSession, lane: u64) {
    f.edit(EditCommand::Lane {
        edit: LaneEdit::SetFlags {
            seq: 3,
            lane,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    })
    .unwrap();
}

/// A loop pass: pass counters, keep and discard, Busy while recording,
/// and the pass ending by itself as the core's does (the take lands as
/// "Record" and the transport stops).
#[test]
fn the_fakes_pass_ends_by_itself() {
    let f = rec_fake();
    arm(&f, 21);
    f.record(3, spec(true, 0)).unwrap();
    let r = f.record_state().unwrap();
    assert!(r.recording);
    assert_eq!((r.seq, r.pass, r.kept, r.discarded), (Some(3), 1, 0, 0));
    assert_eq!(r.spec, Some(spec(true, 0)));
    assert_eq!(r.lanes.len(), 1);
    assert!(r.lanes[0].audio);
    assert!(f.transport().unwrap().playing);
    assert!(f.calls().contains(&"record(3)".to_string()));

    let stop_first = busy("Stop recording first.");
    assert_eq!(f.play(3).unwrap_err(), stop_first);
    assert_eq!(f.play_from(3, 0, None).unwrap_err(), stop_first);
    assert_eq!(f.locate(0).unwrap_err(), stop_first);
    assert_eq!(f.set_loop(true).unwrap_err(), stop_first);
    assert_eq!(
        f.record(3, spec(true, 0)).unwrap_err(),
        busy("A recording is already running.")
    );

    f.next_pass();
    f.keep_pass().unwrap();
    f.next_pass();
    f.next_pass();
    f.discard_pass().unwrap();
    let r = f.record_state().unwrap();
    assert_eq!((r.pass, r.kept, r.discarded), (4, 1, 2));
    // The pass still playing is not decided.
    f.keep_pass().unwrap();
    assert_eq!(f.record_state().unwrap().kept, 1);

    let before = f.snap().version;
    f.end_pass_by_itself();
    let r = f.record_state().unwrap();
    assert!(!r.recording);
    assert_eq!((r.pass, r.kept, r.discarded), (0, 0, 0));
    assert!(!f.transport().unwrap().playing);
    let snap = f.snap();
    assert!(snap.version > before);
    assert_eq!(snap.undo_label.as_deref(), Some("Record"));
    assert_eq!(f.keep_pass().unwrap_err(), model("nothing is recording"));
    f.locate(0).unwrap();
    f.record(3, spec(false, 0)).unwrap();
    assert_eq!(
        f.move_record_arm(3, true).unwrap_err(),
        busy(vizdaw_ffi::session::RECORDING_LANES)
    );
}

/// Stop finishes the pass; a pass that does not loop refuses keep and
/// discard; counting in reports `playing: false` until the roll.
#[test]
fn the_fake_counts_in_and_stop_lands_the_take() {
    let f = rec_fake();
    arm(&f, 21);
    f.record(3, spec(false, 2)).unwrap();
    let t = f.transport().unwrap();
    assert!(t.counting_in);
    assert!(!t.playing);
    assert_eq!(f.record_state().unwrap().pass, 0);
    let not_looping = model("keep and discard act on loop passes; this recording does not loop");
    assert_eq!(f.keep_pass().unwrap_err(), not_looping);
    assert_eq!(f.discard_pass().unwrap_err(), not_looping);

    f.count_in_done();
    let t = f.transport().unwrap();
    assert!(!t.counting_in);
    assert!(t.playing);
    assert_eq!(f.record_state().unwrap().pass, 1);

    f.stop().unwrap();
    assert!(!f.record_state().unwrap().recording);
    assert!(!f.transport().unwrap().playing);
    assert_eq!(f.snap().undo_label.as_deref(), Some("Record"));
}

/// The core's refusals: nothing armed with an input, an open edit, and a
/// scripted error.
#[test]
fn the_fake_refuses_record_as_the_core_does() {
    let f = rec_fake();
    assert_eq!(
        f.record(3, spec(false, 0)).unwrap_err(),
        model("arm a track that has an input to record")
    );
    arm(&f, 21);
    f.begin_edit("Drag").unwrap();
    assert_eq!(
        f.record(3, spec(false, 0)).unwrap_err(),
        busy("Finish the open edit first.")
    );
    f.abort_edit().unwrap();
    f.fail_next(
        "record",
        busy("The disk is almost full: free some space to record."),
    );
    assert!(f.record(3, spec(false, 0)).is_err());
    assert!(!f.record_state().unwrap().recording);
    f.record(3, spec(false, 0)).unwrap();
    assert!(f.record_state().unwrap().recording);
}

/// A Save while recording is owed again when the pass lands; a cancel
/// drops it; Save As's refusal is scripted.
#[test]
fn the_fake_saves_after_the_pass() {
    let f = rec_fake();
    f.set_path(Some("/tmp/Song.vizdaw"));
    arm(&f, 21);
    f.record(3, spec(false, 0)).unwrap();
    f.save().unwrap();
    assert!(f.saves_after_pass().unwrap());
    f.cancel_save_after_pass().unwrap();
    assert!(!f.saves_after_pass().unwrap());
    f.save().unwrap();
    f.clear_calls();
    f.end_pass_by_itself();
    assert!(!f.saves_after_pass().unwrap());
    assert_eq!(f.count("save"), 1, "the owed save: {:?}", f.calls());
    assert!(!f.snap().dirty);
    f.save().unwrap();
    assert!(!f.saves_after_pass().unwrap(), "not recording");

    assert_eq!(f.save_as_refusal().unwrap(), None);
    f.set_save_as_refusal(Some("Wait for the merge."));
    assert_eq!(
        f.save_as_refusal().unwrap().as_deref(),
        Some("Wait for the merge.")
    );
}

/// Move Record Arm follows the core: from the last armed track forward
/// or the first backward, one armed after, the core's texts at the ends.
#[test]
fn the_fake_moves_record_arm() {
    let f = rec_fake();
    let armed = |f: &FakeSession| -> Vec<u64> {
        f.lanes(3)
            .unwrap()
            .iter()
            .filter(|l| l.arm)
            .map(|l| l.id)
            .collect()
    };
    f.move_record_arm(3, true).unwrap();
    assert_eq!(armed(&f), [20]);
    assert_eq!(f.snap().undo_label.as_deref(), Some("Move Record Arm"));
    assert_eq!(
        f.move_record_arm(3, false).unwrap_err(),
        model("there is no track before the armed one")
    );
    f.move_record_arm(3, true).unwrap();
    f.move_record_arm(3, true).unwrap();
    assert_eq!(armed(&f), [22]);
    assert_eq!(
        f.move_record_arm(3, true).unwrap_err(),
        model("there is no track after the armed one")
    );
    arm(&f, 21);
    f.move_record_arm(3, false).unwrap();
    assert_eq!(armed(&f), [20], "every other armed track is disarmed");
}

/// The click row and its commands, input info and levels, the waveform,
/// recovered recordings, step entry, chores and injection are scripted
/// and logged.
#[test]
fn the_fake_scripts_click_input_and_steps() {
    let f = rec_fake();
    let off = ClickRow {
        on: false,
        level_db: -6.0,
        mode: ClickModeRow::PlayAndRecord,
    };
    assert_eq!(f.click().unwrap(), off, "the core's default");
    f.run_click("transport.click".into(), vec![]).unwrap();
    assert!(f.click().unwrap().on);
    f.run_click(
        "transport.set_click_mode".into(),
        vec![text("mode", "record_only")],
    )
    .unwrap();
    f.run_click(
        "transport.set_click_level".into(),
        vec![text("level_db", "-12")],
    )
    .unwrap();
    assert_eq!(
        f.click().unwrap(),
        ClickRow {
            on: true,
            level_db: -12.0,
            mode: ClickModeRow::RecordOnly
        }
    );
    assert_eq!(f.clicks_run().len(), 3);
    f.set_click_row(off);
    assert_eq!(f.click().unwrap(), off);

    assert_eq!(f.input_info().unwrap().channels, 0);
    let input = InputInfoRow {
        device: "Offline".into(),
        channels: 2,
        names: vec!["Input 1".into(), "Input 2".into()],
        input_latency_frames: 0,
        output_latency_frames: 0,
        buffer_frames: 512,
        round_trip_frames: 1024,
        opening: false,
        sample_rate: 48_000.0,
        midi_fx_dropped: 0,
        per_note_dropped: 0,
        mpe_stolen: 0,
    };
    f.set_input(input.clone());
    f.set_input_levels(vec![0.5, 0.25]);
    assert_eq!(f.input_info().unwrap(), input);
    assert_eq!(f.input_levels().unwrap(), [0.5, 0.25]);
    assert_eq!(
        f.input_levels().unwrap(),
        [0.0, 0.0],
        "peaks since the last read"
    );

    assert_eq!(
        f.recording_waveform(21, 0, 10, 4).unwrap_err(),
        model("nothing is recording")
    );
    arm(&f, 21);
    f.record(3, spec(false, 0)).unwrap();
    let w = f.recording_waveform(21, 0, 10, 4).unwrap();
    assert_eq!(
        (w.state, w.channels, w.columns),
        (OverviewStateRow::Ready, 1, 4)
    );
    let rows = OverviewRows {
        state: OverviewStateRow::Ready,
        channels: 1,
        columns: 2,
        min: vec![-0.5, -0.1],
        max: vec![0.5, 0.1],
    };
    f.set_recording_waveform(rows.clone());
    assert_eq!(f.recording_waveform(21, 0, 10, 2).unwrap(), rows);
    assert_eq!(
        f.recording_waveform(20, 0, 10, 2).unwrap_err(),
        model("track 20 records no audio in this pass")
    );
    f.stop().unwrap();

    assert!(f.recovered_recordings().unwrap().is_empty());
    f.set_recovered_files(vec![vizdaw_ffi::record::RecoveredFileRow {
        path: "Media/Recorded/a.caf".into(),
        frames: 10,
        size_known: false,
        readable: true,
    }]);
    assert_eq!(f.recovered_recordings().unwrap(), ["Media/Recorded/a.caf"]);

    let state = step_state_new(0);
    let r = f
        .step_entry(3, 20, state.clone(), StepInputRow::Rest)
        .unwrap();
    assert!(r.moved);
    assert_eq!(
        r.state.tick,
        step_length(NoteValueRow::Quarter, false, false)
    );
    let scripted = StepResultRow {
        state: step_state_new(99),
        snapshot: None,
        moved: false,
    };
    f.script_step(scripted.clone());
    assert_eq!(
        f.step_entry(3, 20, state.clone(), StepInputRow::Dot)
            .unwrap(),
        scripted
    );
    assert_eq!(
        f.step_inputs(),
        [(3, 20, StepInputRow::Rest), (3, 20, StepInputRow::Dot)]
    );

    f.run_chore("marker.add".into(), vec![int("tick", 0)])
        .unwrap();
    assert_eq!(
        f.chores_run(),
        [("marker.add".to_string(), vec![int("tick", 0)])]
    );

    assert!(!f.inject_midi("Keys".into(), vec![0x90, 60, 1]).unwrap());
    f.set_inject_taken(true);
    assert!(f.inject_midi("Keys".into(), vec![0x80, 60, 0]).unwrap());
    assert_eq!(f.injected().len(), 2);
}

/// The free functions through the fake core are the core's.
#[test]
fn the_fake_core_answers_the_free_functions() {
    let core = Arc::new(FakeCore::new());
    assert_eq!(core.step_state_new(15), step_state_new(15));
    assert_eq!(core.step_parse_pitches("C4".into()).unwrap(), [60]);
    assert_eq!(core.audio_settings(), vizdaw_ffi::audio::audio_settings());
}

// ---- the picker sources and Software Monitoring (slice 8c Task 1) ------

/// Offline with the fake input, the real core offers the fake input's
/// three channels, named as it names them, and no MIDI sources; the seam
/// answers what the core's free functions answer.
#[test]
fn the_real_core_answers_the_picker_sources_offline() {
    let _one = offline();
    let fake = InputChannelsRow {
        channels: 3,
        names: vec!["Fake In 1".into(), "Fake In 2".into(), "Fake In 3".into()],
    };
    assert_eq!(LiveCore.audio_input_channels(), Some(fake));
    assert_eq!(
        LiveCore.audio_input_channels(),
        vizdaw_ffi::host::audio_input_channels()
    );
    assert!(LiveCore.midi_source_ports().is_empty());
    assert_eq!(
        LiveCore.audio_settings().software_monitoring,
        vizdaw_ffi::audio::audio_settings().software_monitoring
    );
}

/// The fake core answers as off macOS until scripted, then its script,
/// and counts each read.
#[test]
fn the_fake_core_scripts_the_picker_sources_and_software_monitoring() {
    let core = FakeCore::new();
    assert_eq!(core.audio_input_channels(), None);
    assert!(core.midi_source_ports().is_empty());

    let two = InputChannelsRow {
        channels: 2,
        names: vec!["Mic".into(), "Line".into()],
    };
    core.set_input_channels(Some(two.clone()));
    core.set_midi_sources(vec!["Keys In".into(), "Pads In".into()]);
    assert_eq!(core.audio_input_channels(), Some(two));
    assert_eq!(core.midi_source_ports(), ["Keys In", "Pads In"]);

    let on = core.audio_settings().software_monitoring;
    core.set_software_monitoring(!on);
    assert_eq!(core.audio_settings().software_monitoring, !on);
    assert_eq!(
        core.audio_settings().buffer_frames,
        vizdaw_ffi::audio::audio_settings().buffer_frames,
        "the rest is the core's"
    );
    core.set_audio_settings(None);
    assert_eq!(core.audio_settings(), vizdaw_ffi::audio::audio_settings());

    assert_eq!(core.call_count("audio_input_channels"), 2);
    assert_eq!(core.call_count("midi_source_ports"), 2);
    assert_eq!(core.call_count("audio_settings"), 4);
}

/// A held read takes its answer when it starts and waits for its release
/// on its own thread, so a later read, of a newer script, finishes first.
#[test]
fn a_held_source_read_finishes_after_a_later_one() {
    let core = Arc::new(FakeCore::new());
    core.set_midi_sources(vec!["Old".into()]);
    let hold = core.hold("midi_source_ports");
    let reader = {
        let core = core.clone();
        std::thread::spawn(move || core.midi_source_ports())
    };
    assert!(hold.wait_started(Duration::from_secs(5)));
    core.set_midi_sources(vec!["New".into()]);
    assert_eq!(
        core.midi_source_ports(),
        ["New"],
        "the later read is not held"
    );
    std::thread::sleep(Duration::from_millis(20));
    assert!(!reader.is_finished(), "the held read waits for its release");
    hold.release();
    assert_eq!(reader.join().unwrap(), ["Old"]);

    let threads = core.read_threads("midi_source_ports");
    assert_eq!(threads.len(), 2);
    assert_ne!(threads[0], std::thread::current().id());
    assert_eq!(threads[1], std::thread::current().id());

    // A hold per call: one on the channels does not hold the sources.
    let ch = core.hold("audio_input_channels");
    assert_eq!(core.midi_source_ports(), ["New"]);
    assert!(!ch.started());
    ch.release();
    assert_eq!(
        core.audio_input_channels(),
        None,
        "a released hold waits not"
    );
}
