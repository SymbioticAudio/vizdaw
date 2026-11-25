//! The take that grows while it is written (Swift `GrowingTakeTests`;
//! plan 5d-2 Task 9; spec §6.2): where it sits, which file frames fill
//! it, the record poll reading the writer's live peaks for its visible
//! pieces, and the rows going at Stop. The fake's tempo maps 30 720 ticks
//! to a second; the project is 48 kHz.

mod support;

use std::sync::Arc;

use support::{FakeSession, demo_fake, lane_row};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{Rect, TimeAxis};
use vizdaw_app::growing_take::{GrowingTake, MAX_COLUMNS, MAX_PIECES, TakeView};
use vizdaw_ffi::edit::LaneInputRow;
use vizdaw_ffi::record::RecordLaneRow;
use vizdaw_ffi::selection::TickRange;
use vizdaw_ffi::{OverviewRows, OverviewStateRow, VizError};

fn armed(id: u64, name: &str, audio: bool) -> vizdaw_ffi::snapshot::LaneRow {
    let mut l = lane_row(id, name, false);
    l.arm = true;
    l.input = Some(if audio {
        LaneInputRow::Audio {
            first: 0,
            stereo: false,
            return_of: None,
        }
    } else {
        LaneInputRow::Midi {
            port: None,
            channel: None,
        }
    });
    l
}

fn lane(id: u64, audio: bool, return_frames: i64) -> RecordLaneRow {
    RecordLaneRow {
        lane: id,
        audio,
        path: audio.then(|| "Media/Recorded/G.caf".to_owned()),
        clipped: 0,
        peak: 0.,
        return_frames,
    }
}

/// Song recording: the pass started at 1 s (file frame 48 000), the
/// record range at 2 s; 4 s of file written; Groove (30) records audio,
/// Chords (31) MIDI.
fn recording(punch_end: Option<i64>) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(demo_fake());
    f.set_lanes(
        3,
        vec![armed(30, "Groove", true), armed(31, "Chords", false)],
    );
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.select(Some(3));
    m.refresh_project_info();
    m.toggle_record().unwrap();
    f.with_pass(|r| {
        r.lanes = vec![lane(30, true, 0), lane(31, false, 0)];
        if let Some(end) = punch_end
            && let Some(spec) = r.spec.as_mut()
        {
            spec.punch = Some(TickRange { start: 61_440, end });
        }
        r.pass_start_tick = Some(30_720);
        r.pass_start_file_frame = 48_000;
        r.range_start_tick = 61_440;
        r.frames = 4 * 48_000;
    });
    m.refresh_record(false);
    (f, m)
}

/// One record poll, its pieces' read run on another thread and landed
/// (the store's part).
fn poll(m: &mut DocumentModel) {
    m.poll_record();
    if let Some(r) = m.take_growing_read() {
        let reply = std::thread::spawn(move || r.run()).join().unwrap();
        m.land_growing_read(reply);
    }
}

fn take(start_tick: i64, end_tick: i64, from_frame: u64, to_frame: u64) -> GrowingTake {
    GrowingTake {
        lane: 30,
        start_tick,
        end_tick,
        from_frame,
        to_frame,
    }
}

fn rect(x: f32, width: f32) -> Rect {
    Rect {
        x,
        y: 0.,
        width,
        height: 40.,
    }
}

#[test]
fn the_take_runs_from_the_range_start_to_what_is_written() {
    let (_f, m) = recording(None);
    assert_eq!(
        m.growing_takes(),
        vec![take(61_440, 122_880, 96_000, 192_000)]
    );
}

#[test]
fn the_punch_out_ends_it() {
    let (_f, m) = recording(Some(92_160));
    assert_eq!(
        m.growing_takes(),
        vec![take(61_440, 92_160, 96_000, 144_000)]
    );
}

#[test]
fn nothing_grows_before_the_roll_or_in_another_sequence() {
    let (f, mut m) = recording(None);
    f.with_pass(|r| r.pass_start_tick = None);
    m.refresh_record(false);
    assert!(m.growing_takes().is_empty());
    f.with_pass(|r| {
        r.pass_start_tick = Some(30_720);
        r.frames = 96_000;
    });
    m.refresh_record(false);
    assert!(
        m.growing_takes().is_empty(),
        "nothing written inside the range yet"
    );
    f.with_pass(|r| r.frames = 192_000);
    m.refresh_record(false);
    assert!(!m.growing_takes().is_empty());
    m.select(Some(2));
    assert!(m.growing_takes().is_empty());
}

/// A synth return track's file runs one return latency behind (plan
/// 5d-1 Task 10 review I1): it is drawn where Stop places it.
#[test]
fn a_return_lane_is_placed_by_its_return_latency() {
    let (f, mut m) = recording(None);
    f.with_pass(|r| r.lanes[0] = lane(30, true, 4_800));
    m.refresh_record(false);
    assert_eq!(
        m.growing_takes(),
        vec![take(61_440, 119_808, 100_800, 192_000)]
    );
}

/// A large positive latency trim can put the pass's anchor before the
/// file's start: the take starts at the file's first frame, never traps.
#[test]
fn a_negative_anchor_starts_later() {
    let (f, mut m) = recording(None);
    f.with_pass(|r| {
        r.pass_start_file_frame = -96_000;
        r.range_start_tick = 30_720;
    });
    m.refresh_record(false);
    assert_eq!(m.growing_takes(), vec![take(92_160, 215_040, 0, 192_000)]);
}

/// The core saturates the pass's file frame at `i64::MAX` (5d-2 final
/// review I2): an anchor past the frame range draws no take, never traps.
#[test]
fn a_saturated_file_frame_draws_nothing_and_never_traps() {
    for punch in [None, Some(92_160)] {
        let (f, mut m) = recording(punch);
        f.with_pass(|r| {
            r.pass_start_file_frame = i64::MAX - 1;
            r.lanes[0] = lane(30, true, 4_800);
        });
        m.refresh_record(false);
        assert!(m.growing_takes().is_empty());
        f.with_pass(|r| {
            r.pass_start_file_frame = i64::MIN + 1;
            r.lanes[0] = lane(30, true, -4_800);
        });
        m.refresh_record(false);
        assert!(m.growing_takes().is_empty());
        f.with_pass(|r| r.frames = u64::MAX);
        m.refresh_record(false);
        m.set_take_view(TakeView {
            axis: TimeAxis::default(),
            width: 800.,
        });
        poll(&mut m);
        assert!(m.held_takes().is_empty());
    }
}

/// The record poll reads the visible pieces' rows (Swift: the view drew
/// from the live peaks; here the poll reads, render only draws).
#[test]
fn the_arrangement_draws_it_from_the_live_peaks() {
    let (f, mut m) = recording(None);
    m.set_take_view(TakeView {
        axis: TimeAxis::default(),
        width: 800.,
    });
    poll(&mut m);
    let calls = f.recording_waveform_asks();
    assert!(!calls.is_empty() && calls.iter().all(|c| c.0 == 30 && c.3 > 0));
    assert_eq!(calls.iter().map(|c| c.1).min(), Some(96_000));
    assert_eq!(
        calls.iter().map(|c| c.2).max(),
        Some(192_000),
        "the slices cover the take"
    );
    let held = m.held_takes();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].take, take(61_440, 122_880, 96_000, 192_000));
    assert_eq!(held[0].pieces.len(), calls.len());
    assert_eq!(held[0].pieces.first().unwrap().from_tick, 61_440);
    assert_eq!(held[0].pieces.last().unwrap().to_tick, 122_880);
}

/// A four-bar take over a tempo change: 120 BPM for two bars (4 s), 60
/// after (8 s).
#[test]
fn a_growing_take_follows_a_tempo_change() {
    let (f, m) = recording(None);
    f.set_seconds_at_map(Some(Arc::new(|t| {
        if t <= 122_880 {
            t as f64 / 30_720.
        } else {
            4. + (t - 122_880) as f64 / 15_360.
        }
    })));
    let t = take(0, 245_760, 0, 576_000);
    let r = rect(0., 512.);
    let slices = m.growing_slices(&t, r, r, |x| (x * 480.).round() as i64);
    assert!(!slices.is_empty() && slices.len() <= MAX_PIECES);
    assert_eq!(slices.len(), 8, "eight 64-point pieces");
    let mid = slices.iter().find(|s| s.min_x == 256.).unwrap();
    assert_eq!(mid.from, 192_000, "not 288 000, as a linear map gives");
    assert_eq!(slices.first().unwrap().from, 0);
    assert_eq!(slices.last().unwrap().to, 576_000);
    for (a, b) in slices.iter().zip(&slices[1..]) {
        assert!(a.to == b.from && a.max_x == b.min_x && a.to_tick == b.from_tick);
    }
    assert!(slices.iter().map(|s| s.columns).sum::<u32>() <= MAX_COLUMNS);
    let far = rect(0., 100_000.);
    let many = m.growing_slices(&t, far, far, |x| x as i64);
    assert!(many.len() <= MAX_PIECES);
    assert!(many.iter().map(|s| s.columns).sum::<u32>() <= MAX_COLUMNS);
}

/// 5d-2 T9 M3: only the visible slice's frames are asked for, at most
/// 8192 columns.
#[test]
fn the_visible_slice_asks_for_its_frames_and_at_most_8192_columns() {
    let r = rect(0., 1000.);
    assert_eq!(GrowingTake::slice(r, r, 0, 48_000), Some((0, 48_000, 1000)));
    assert_eq!(
        GrowingTake::slice(r, rect(500., 600.), 0, 48_000),
        Some((24_000, 48_000, 500))
    );
    let wide = rect(0., 20_000.);
    assert_eq!(
        GrowingTake::slice(wide, wide, 0, 960_000).map(|s| s.2),
        Some(8192)
    );
    assert_eq!(GrowingTake::slice(r, rect(2000., 10.), 0, 48_000), None);
    assert_eq!(GrowingTake::slice(r, r, 5, 5), None);
}

fn peaks(v: f32) -> OverviewRows {
    OverviewRows {
        state: OverviewStateRow::Ready,
        channels: 1,
        columns: 4,
        min: vec![-v; 4],
        max: vec![v; 4],
    }
}

/// A failed read keeps the last rows; the rows go at Stop. Each poll
/// reads once per piece, and the pass never locates.
#[test]
fn a_failed_read_keeps_the_last_rows_and_stop_drops_them() {
    let (f, mut m) = recording(None);
    m.set_take_view(TakeView {
        axis: TimeAxis::default(),
        width: 800.,
    });
    f.set_recording_waveform(peaks(0.5));
    poll(&mut m);
    let first = m.held_takes()[0].pieces.clone();
    assert!(first.iter().all(|p| p.rows.max[0] == 0.5));
    let reads = f.reads("recording_waveform");
    assert_eq!(reads, first.len(), "one read per piece");
    f.fail_next(
        "recording_waveform",
        VizError::Model {
            message: "gone".into(),
        },
    );
    f.with_pass(|r| r.frames = 5 * 48_000);
    poll(&mut m);
    let held = m.held_takes()[0].clone();
    assert_eq!(held.take.to_frame, 240_000, "the take still grows");
    assert_eq!(held.pieces, first, "the last rows are kept");
    m.toggle_record().unwrap();
    assert!(!m.record().is_recording());
    assert!(m.held_takes().is_empty(), "the rows go at Stop");
    assert!(
        !f.calls().iter().any(|c| c.starts_with("locate")),
        "{:?}",
        f.calls()
    );
}

/// The poll asks; the store runs the read off the UI thread (no
/// `recording_waveform` in the poll), one read out at a time, and a read
/// from before Stop lands nowhere.
#[test]
fn the_pieces_are_read_off_the_ui_thread_one_read_at_a_time() {
    let (f, mut m) = recording(None);
    m.set_take_view(TakeView {
        axis: TimeAxis::default(),
        width: 800.,
    });
    m.poll_record();
    assert_eq!(f.reads("recording_waveform"), 0, "no read in the poll");
    let first = m.take_growing_read().expect("a read to run");
    m.poll_record();
    assert!(m.take_growing_read().is_none(), "one read out at a time");
    let reply = std::thread::spawn(move || first.run()).join().unwrap();
    assert!(m.land_growing_read(reply));
    assert!(!m.held_takes()[0].pieces.is_empty());
    m.poll_record();
    let late = m.take_growing_read().expect("the next read");
    m.toggle_record().unwrap();
    let reply = std::thread::spawn(move || late.run()).join().unwrap();
    assert!(!m.land_growing_read(reply), "the pass ended");
    assert!(m.held_takes().is_empty());
}

/// Through the store: a poll in `with_document` hands the read out, and
/// nothing reads in the call.
#[test]
fn the_store_runs_the_pieces_read() {
    use vizdaw_app::DocumentStore;
    let (f, m) = recording(None);
    drop(m);
    let shared = f.clone();
    let mut store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    let id = store.new_demo().unwrap();
    store.with_document(id, |d, _| {
        d.select(Some(3));
        d.refresh_project_info();
        d.refresh_record(false);
        d.set_take_view(TakeView {
            axis: TimeAxis::default(),
            width: 800.,
        });
        d.poll_record();
    });
    assert_eq!(f.reads("recording_waveform"), 0, "no read in the call");
    store.run_reads();
    let held = store.document(id).unwrap().held_takes();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].pieces.len(), f.reads("recording_waveform"));
}
