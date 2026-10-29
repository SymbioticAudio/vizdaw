//! The renderer's record tap and the fake input (plan 5c Task 5;
//! sub-project 5 spec §6.2–§6.4, §12.1): blocks of input are stamped with
//! continuous run frames from the first block that plays, marks say which
//! schedule frame plays at the roll and at every loop wrap (and after a
//! block the transport did not advance through), a full ring loses whole
//! blocks and counts them, and the tap reads closed once it is removed or
//! its renderer goes.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{FakeInput, OfflineBackend, OfflineSinks, fake_signal};
use core_engine::record::{Mark, RecordHandles, RecordRequest};
use core_model::document::Document;
use core_model::ids::SeqId;

const SR: f64 = 48_000.0;

/// One bar of notes on the built-in synth (2 s at 120 BPM).
fn one_bar() -> (Document, SeqId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Bar", "Notes", Some(synth)).unwrap();
    b.notes(
        seq,
        lane,
        &[
            (0, 60, 0x8000, Q),
            (Q, 64, 0x8000, Q),
            (2 * Q, 67, 0x8000, Q),
            (3 * Q, 72, 0x8000, Q),
        ],
    )
    .unwrap();
    (b.finish(), seq)
}

fn setup(input: FakeInput, block: u32, sinks: OfflineSinks) -> (Engine, OfflineBackend) {
    let (doc, seq) = one_bar();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(SR, block).with_input(input);
    b.start(renderer).unwrap();
    (engine, b)
}

fn request(channels: Vec<u16>, ring_frames: usize) -> RecordRequest {
    RecordRequest {
        channels,
        range: None,
        ring_frames,
    }
}

/// Every block in the ring as `(run, frames, samples)`.
fn drain(h: &mut RecordHandles) -> Vec<(u64, u32, Vec<f32>)> {
    let mut out = Vec::new();
    let mut buf = Vec::new();
    while let Some(b) = h.ring.pop(&mut buf) {
        out.push((b.run, b.frames, buf.clone()));
    }
    out
}

fn marks(h: &mut RecordHandles) -> Vec<(u64, u64)> {
    full_marks(h).iter().map(|m| (m.run, m.frame)).collect()
}

fn full_marks(h: &mut RecordHandles) -> Vec<Mark> {
    let mut v = Vec::new();
    while let Ok(m) = h.marks.pop() {
        v.push(m);
    }
    v
}

#[test]
fn input_is_stamped_from_the_roll_with_continuous_runs() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let info = b.open(&Default::default()).unwrap();
    assert_eq!(info.input_channels, 3);
    assert_eq!(info.input_channel_names[0], "Fake In 1");
    assert_eq!(
        (info.input_latency_frames, info.output_latency_frames),
        (0, 0)
    );
    b.run_frames(1024);
    // Device channels 1 then 0 into the ring.
    let mut h = engine
        .start_recording(&request(vec![1, 0], 48_000))
        .unwrap();
    // Installed, but nothing plays yet: nothing is captured.
    b.run_frames(512);
    assert!(drain(&mut h).is_empty());
    engine.locate_frame(1000).unwrap();
    engine.play().unwrap();
    // Play is applied at the next block, absolute frame 1536.
    b.run_frames(2560);
    engine.stop_recording().unwrap();
    b.run_frames(256);
    assert!(h.status.closed());
    assert_eq!(h.status.run(), 2560);
    assert_eq!(h.status.delay(), Some(0));
    assert_eq!(h.status.lost_blocks(), 0);
    assert_eq!(marks(&mut h), vec![(0, 1000)]);
    let blocks = drain(&mut h);
    let mut next = 0u64;
    for (run, frames, s) in &blocks {
        assert_eq!(*run, next, "contiguous");
        for f in 0..*frames as usize {
            let abs = 1536 + run + f as u64;
            assert_eq!(s[2 * f], fake_signal(1, abs, SR));
            assert_eq!(s[2 * f + 1], fake_signal(0, abs, SR));
        }
        next += u64::from(*frames);
    }
    assert_eq!(next, 2560);
}

#[test]
fn every_loop_wrap_is_marked() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    engine.set_loop_range(Some((1000, 1700))).unwrap();
    engine.locate_frame(1000).unwrap();
    let mut h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.play().unwrap();
    b.run_frames(256 * 12);
    let m = full_marks(&mut h);
    assert_eq!(
        m.iter().map(|m| (m.run, m.frame)).collect::<Vec<_>>(),
        vec![
            (0, 1000),
            (700, 1000),
            (1400, 1000),
            (2100, 1000),
            (2800, 1000)
        ]
    );
    // The roll is no wrap; every later one is.
    assert_eq!(
        m.iter().map(|m| m.wrap).collect::<Vec<_>>(),
        vec![false, true, true, true, true]
    );
    // MIDI-only recordings have no ring channels and still mark.
    engine.stop().unwrap();
    engine.stop_recording().unwrap();
    b.run_frames(256);
    let mut h = engine.start_recording(&request(vec![], 48_000)).unwrap();
    engine.locate_frame(1200).unwrap();
    engine.play().unwrap();
    b.run_frames(256 * 4);
    assert_eq!(marks(&mut h), vec![(0, 1200), (500, 1000)]);
    assert!(drain(&mut h).is_empty());
}

#[test]
fn loopback_input_is_the_output_later_and_reports_its_delay() {
    let sinks = OfflineSinks::new(10_000).with_tone();
    let (mut engine, mut b) = setup(FakeInput::Loopback(1000), 256, sinks);
    let info = b.open(&Default::default()).unwrap();
    assert_eq!(
        (info.output_latency_frames, info.input_latency_frames),
        (500, 500)
    );
    let mut h = engine.start_recording(&request(vec![0], 96_000)).unwrap();
    engine.play().unwrap();
    let mut out: Vec<f32> = Vec::new();
    for _ in 0..40 {
        b.block(256);
        out.extend_from_slice(b.output(256).0);
    }
    assert_eq!(h.status.delay(), Some(1000));
    let blocks = drain(&mut h);
    assert!(out.iter().any(|x| x.abs() > 0.001), "the tone plays");
    // Play applied at the first block: run r is absolute frame r.
    let mut checked = 0;
    for (run, frames, s) in &blocks {
        assert_eq!(s.len(), *frames as usize);
        for (f, x) in s.iter().enumerate() {
            let abs = (*run as usize) + f;
            let want = abs.checked_sub(1000).map_or(0.0, |k| out[k]);
            assert_eq!(*x, want, "frame {abs}");
            checked += 1;
        }
    }
    assert_eq!(checked, 40 * 256);
}

#[test]
fn a_full_ring_loses_whole_blocks_and_counts_them() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    // Room for one block of 256.
    let mut h = engine.start_recording(&request(vec![0, 1], 300)).unwrap();
    engine.play().unwrap();
    b.run_frames(256 * 5);
    assert!(h.status.lost_blocks() >= 3, "{}", h.status.lost_blocks());
    let first = drain(&mut h);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, 0);
    b.run_frames(256);
    let later = drain(&mut h);
    // The next block that fits shows the gap in its run.
    assert_eq!(later.len(), 1);
    assert_eq!(later[0].0, 256 * 5);
}

#[test]
fn a_block_the_transport_skipped_is_captured_and_marked_after() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let mut h = engine.start_recording(&request(vec![0], 96_000)).unwrap();
    engine.locate_frame(500).unwrap();
    engine.play().unwrap();
    // 8192 frames is more than the renderer was prepared for: an xrun,
    // rendered as silence, the transport does not move.
    b.run_blocks([256, 8192, 256]);
    assert_eq!(h.status.run(), 256 + 8192 + 256);
    assert_eq!(marks(&mut h), vec![(0, 500), (256 + 8192, 756)]);
    let runs: Vec<u64> = drain(&mut h).iter().map(|b| b.0).collect();
    assert_eq!(runs, vec![0, 256, 256 + 8192]);
}

#[test]
fn a_schedule_swap_while_recording_resyncs() {
    let (doc, seq) = one_bar();
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let mut h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.locate_frame(100).unwrap();
    engine.play().unwrap();
    b.run_frames(512);
    // An edit recompiles: the next block marks where the transport is.
    engine.load(&doc, seq).unwrap();
    b.run_frames(256);
    let m = full_marks(&mut h);
    assert_eq!(m.len(), 2, "{m:?}");
    assert_eq!((m[0].run, m[0].frame, m[0].wrap), (0, 100, false));
    assert_eq!((m[1].run, m[1].wrap), (512, false));
    // The swap maps the position through ticks (within a frame): the mark
    // says where the transport really is.
    assert!(m[1].frame.abs_diff(612) <= 1, "{m:?}");
}

#[test]
fn the_tap_reads_closed_when_its_renderer_goes() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let h = engine.start_recording(&request(vec![0], 4800)).unwrap();
    engine.play().unwrap();
    b.run_frames(512);
    assert!(!h.status.closed());
    drop(b.stop().unwrap());
    assert!(
        h.status.closed(),
        "an output that stopped ends the recording"
    );
    assert!(h.ring.closed());
}

/// Pre-flight M2: once the transport leaves Playing, the run stops growing
/// and a non-wrap mark says where it stopped; playing again resumes the run
/// with a mark.
#[test]
fn a_stop_ends_the_run_with_a_mark() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let mut h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.locate_frame(1000).unwrap();
    engine.play().unwrap();
    b.run_frames(1024);
    engine.stop().unwrap();
    b.run_frames(1024);
    assert_eq!(h.status.run(), 1024);
    b.run_frames(512);
    assert_eq!(h.status.run(), 1024, "the run stops growing");
    assert!(!h.status.closed(), "stopping is not removing");
    let m = full_marks(&mut h);
    assert_eq!(
        m.iter()
            .map(|m| (m.run, m.frame, m.wrap))
            .collect::<Vec<_>>(),
        vec![(0, 1000, false), (1024, 2024, false)]
    );
    let blocks = drain(&mut h);
    let total: u64 = blocks.iter().map(|b| u64::from(b.1)).sum();
    assert_eq!(total, 1024, "nothing is captured once stopped");
    // Playing again resumes the run where it stopped.
    engine.play().unwrap();
    b.run_frames(256);
    assert_eq!(h.status.run(), 1280);
    assert_eq!(marks(&mut h), vec![(1024, 2024)]);
}

/// Pre-flight M2: the sequence end stops the run inside the block, at the
/// frame the end is reached, and marks it.
#[test]
fn the_sequence_end_ends_the_run_with_a_mark() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let length = engine.length_frames();
    let start = length - 300;
    let mut h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.locate_frame(start).unwrap();
    engine.play().unwrap();
    b.run_frames(256 * 4);
    assert_eq!(h.status.run(), 300);
    b.run_frames(256 * 4);
    assert_eq!(h.status.run(), 300, "the tail is not captured");
    assert_eq!(marks(&mut h), vec![(0, start), (300, length)]);
    let blocks = drain(&mut h);
    assert_eq!(
        blocks.iter().map(|b| (b.0, b.1)).collect::<Vec<_>>(),
        vec![(0, 256), (256, 44)]
    );
}

/// Review fix M1: a Locate applied while recording is a cut, marked where
/// the transport lands, even though the session refuses it (Ruling 17).
#[test]
fn a_locate_while_recording_is_marked() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let mut h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.locate_frame(1000).unwrap();
    engine.play().unwrap();
    b.run_frames(512);
    engine.locate_frame(5000).unwrap();
    b.run_frames(256);
    assert_eq!(marks(&mut h), vec![(0, 1000), (512, 5000)]);
    assert_eq!(h.status.run(), 768);
}

/// Review fixes M1 and M2: Stop, Locate and Play drained in one block mark
/// the stop where playback stopped, then the located frame.
#[test]
fn stop_locate_play_in_one_block_marks_the_stop_and_the_jump() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let mut h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.locate_frame(1000).unwrap();
    engine.play().unwrap();
    b.run_frames(512);
    engine.stop().unwrap();
    engine.locate_frame(5000).unwrap();
    engine.play().unwrap();
    b.run_frames(256);
    assert_eq!(marks(&mut h), vec![(0, 1000), (512, 1512), (512, 5000)]);
    assert_eq!(h.status.run(), 768);
}

/// Review fix M2: a stop with a return to start in the same block marks
/// where playback stopped, not the located frame.
#[test]
fn a_stop_mark_is_where_playback_stopped() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let mut h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.locate_frame(1000).unwrap();
    engine.play().unwrap();
    b.run_frames(512);
    engine.stop().unwrap();
    engine.locate_frame(0).unwrap();
    b.run_frames(512);
    assert_eq!(marks(&mut h), vec![(0, 1000), (512, 1512)]);
    assert_eq!(h.status.run(), 512);
}

/// Review fix M3: rolling exactly at the loop end marks the roll once, not
/// a roll and a wrap at run 0.
#[test]
fn rolling_at_the_loop_end_marks_once() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    engine.set_loop_range(Some((1000, 1700))).unwrap();
    engine.locate_frame(1700).unwrap();
    let mut h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.play().unwrap();
    b.run_frames(256 * 4);
    let m = full_marks(&mut h);
    assert_eq!(
        m.iter()
            .map(|m| (m.run, m.frame, m.wrap))
            .collect::<Vec<_>>(),
        vec![(0, 1000, false), (700, 1000, true)]
    );
}

/// Review fix M4: playing again after a stop takes the device's input delay
/// as it is then.
#[test]
fn a_resume_refreshes_the_input_delay() {
    let (mut t, h) = core_engine::record::tap(&request(vec![0], 4800), 0);
    let ch = [0.0f32; 256];
    let input: [&[f32]; 1] = [&ch];
    t.begin_block(100, 7);
    t.end_block(&input, 256, true);
    assert_eq!(h.status.delay(), Some(7));
    t.stop(0, 356);
    t.end_block(&input, 256, true);
    t.begin_block(356, 9);
    assert_eq!(h.status.delay(), Some(9));
}

/// Task 6 review carry 4 (Task 10): a message that arrived while the
/// transport stood still but is taken by the resume block has a negative
/// offset, a run before the resume mark, where the frozen run already
/// played the timeline before the stop. It is dropped; one at or after
/// the resume mark is kept, and before the stop nothing changes.
#[test]
fn midi_from_before_a_resume_is_dropped() {
    use core_engine::midi1::Midi1Msg;
    use core_model::ids::LaneId;
    let (mut t, mut h) = core_engine::record::tap(&request(vec![0], 4800), 0);
    let ch = [0.0f32; 256];
    let input: [&[f32]; 1] = [&ch];
    let lane = LaneId(1);
    let msg = |p: u8| Midi1Msg::three(0x90, p, 100);
    t.begin_block(1000, 0);
    // Late in the first block: before it, still kept (pre-roll style).
    t.midi(lane, -20, msg(60));
    t.midi(lane, 10, msg(61));
    t.end_block(&input, 256, true);
    t.stop(100, 1356);
    t.midi(lane, 50, msg(62));
    t.end_block(&input, 256, true);
    // Resumed at run 356.
    t.begin_block(1356, 0);
    t.midi(lane, -30, msg(63));
    t.midi(lane, 0, msg(64));
    t.midi(lane, 5, msg(65));
    t.end_block(&input, 256, true);
    // A later block's late message reaching back before the resume.
    t.begin_block(1612, 0);
    t.midi(lane, -300, msg(66));
    t.midi(lane, -10, msg(67));
    let mut got = Vec::new();
    while let Ok(m) = h.midi.pop() {
        got.push((m.run, m.msg.bytes[1]));
    }
    assert_eq!(
        got,
        vec![
            (-20, 60),
            (10, 61),
            (306, 62),
            (356, 64),
            (361, 65),
            (602, 67)
        ]
    );
}

/// Final review I1: every mark carries the graph latency in effect from
/// it on. The renderer's latency at install is the roll's; a change while
/// rolling marks the next block with the new one; an unchanged latency
/// marks nothing; wraps carry the latency then.
#[test]
fn marks_carry_the_graph_latency_and_a_change_is_marked() {
    let (mut t, mut h) = core_engine::record::tap(&request(vec![0], 4800), 0);
    let ch = [0.0f32; 256];
    let input: [&[f32]; 1] = [&ch];
    // Installed into a renderer whose graph has 64 frames of latency.
    t.set_latency(64);
    t.begin_block(1000, 0);
    t.end_block(&input, 256, true);
    t.set_latency(64);
    t.begin_block(1256, 0);
    t.end_block(&input, 256, true);
    // A graph with 2048 frames of latency is swapped in.
    t.set_latency(2048);
    t.begin_block(1512, 0);
    t.wrap(100, 1000);
    t.end_block(&input, 256, true);
    let m = full_marks(&mut h);
    assert_eq!(
        m.iter()
            .map(|m| (m.run, m.frame, m.wrap, m.latency))
            .collect::<Vec<_>>(),
        vec![
            (0, 1000, false, 64),
            (512, 1512, false, 2048),
            (612, 1000, true, 2048)
        ]
    );
}

/// Task 9b re-review m5: the roll block's run reaches the control side
/// only once that block has published its transport state (the renderer
/// calls `published` right after `publish`), so a run seen implies the
/// Playing state is visible; later blocks store their run at their end.
#[test]
fn the_roll_blocks_run_waits_for_its_publish() {
    let (mut t, h) = core_engine::record::tap(&request(vec![0], 4800), 0);
    let ch = [0.0f32; 256];
    let input: [&[f32]; 1] = [&ch];
    t.begin_block(100, 0);
    t.end_block(&input, 256, true);
    assert_eq!(h.status.run(), 0, "not before the roll block publishes");
    t.published();
    assert_eq!(h.status.run(), 256);
    t.published();
    t.begin_block(356, 0);
    t.end_block(&input, 256, true);
    assert_eq!(h.status.run(), 512, "a later block as before");
}

/// Task 9b re-review 2 n3: the renderer calls `published` after the roll
/// block publishes, so the run is seen after that block, and a take whose
/// sequence ends inside the roll block ends at the frames it played
/// rather than at 0.
#[test]
fn the_renderer_publishes_the_roll_blocks_run() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.play().unwrap();
    b.run_frames(256);
    assert_eq!(h.status.run(), 256, "seen after the roll block");
    engine.stop().unwrap();
    let length = engine.length_frames();
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.locate_frame(length - 100).unwrap();
    engine.play().unwrap();
    b.run_frames(256 * 3);
    assert_eq!(h.status.run(), 100, "rolled and ended in one block");
}
