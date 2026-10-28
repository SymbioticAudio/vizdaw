//! Audio regions in the schedule (plan 5a Task 7): segments per output
//! path, the overlap rule, loops, references, fades and routing.

use core_engine::fixtures::{BAR, Builder, Q};
use core_engine::schedule::{
    AudioSchedule, CompileOptions, CompileReport, Schedule, Segment, TargetKind, TargetTable,
    compile, fade_curve,
};
use core_model::command::{Command, LaneCmd};
use core_model::document::Document;
use core_model::event::{Fade, FadeShape};
use core_model::ids::{LaneId, MediaId, OutputPathId, SeqId};
use core_model::time::Tick;
use std::collections::BTreeSet;

/// 120 BPM at 48 kHz: a quarter is 24 000 frames.
const QF: u64 = 24_000;

fn none() -> Fade {
    Fade::default()
}

fn fade(n: u64, shape: FadeShape) -> Fade {
    Fade {
        length_frames: n,
        shape,
    }
}

fn run(doc: &Document, seq: SeqId, offline: &[u64]) -> (Schedule, CompileReport) {
    let mut opts = CompileOptions::new(48_000.0);
    opts.offline_media = offline.iter().map(|m| MediaId(*m)).collect::<BTreeSet<_>>();
    let mut table = TargetTable::new();
    compile(doc, seq, &opts, &mut |k: &TargetKind| {
        table.intern(k.clone()).map(Ok)
    })
    .unwrap()
}

/// `(start, frames, media, file_frame)` of `path`'s segments.
fn segs(a: &AudioSchedule, path: u64) -> Vec<(u64, u64, u64, u64)> {
    a.segments(OutputPathId(path))
        .iter()
        .map(|s| (s.start, s.frames, s.media.0, s.file_frame))
        .collect()
}

/// One sequence with one lane and two 10 s media items (0 and 1).
fn base() -> (Builder, SeqId, LaneId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    b.media("Media/a.caf", 480_000, 2).unwrap();
    b.media("Media/b.caf", 480_000, 1).unwrap();
    (b, seq, lane)
}

#[test]
fn a_region_becomes_one_segment_on_its_output_path() {
    let (mut b, seq, lane) = base();
    b.region(seq, lane, Q, MediaId(0), 1000, 48_000, -6.0, none(), none())
        .unwrap();
    let (s, r) = run(b.doc(), seq, &[]);
    assert_eq!(segs(&s.audio, 0), vec![(QF, 48_000, 0, 1000)]);
    let g = s.audio.segments(OutputPathId(0))[0].gain;
    assert!((g - 0.501_187).abs() < 1e-5, "{g}");
    assert_eq!(
        (r.audio_regions, r.audio_segments, r.audio_unrouted),
        (1, 1, 0)
    );
    assert!(
        r.warnings.iter().all(|w| !w.contains("audio")),
        "{:?}",
        r.warnings
    );
    let needs = s.audio.needs();
    assert_eq!(needs.len(), 1);
    assert_eq!(
        (needs[0].start, needs[0].frames, needs[0].file_frame),
        (QF, 48_000, 1000)
    );
}

#[test]
fn later_start_takes_over_and_earlier_resumes() {
    let (mut b, seq, lane) = base();
    // A: beat 0, 4 beats; B: beat 1, 1 beat. A plays 0-1, B 1-2, A again 2-4.
    b.region(seq, lane, 0, MediaId(0), 0, 4 * QF, 0.0, none(), none())
        .unwrap();
    b.region(seq, lane, Q, MediaId(1), 500, QF, 0.0, none(), none())
        .unwrap();
    let (s, _) = run(b.doc(), seq, &[]);
    assert_eq!(
        segs(&s.audio, 0),
        vec![(0, QF, 0, 0), (QF, QF, 1, 500), (2 * QF, 2 * QF, 0, 2 * QF)]
    );
}

#[test]
fn equal_starts_go_to_the_later_region() {
    let (mut b, seq, lane) = base();
    b.region(seq, lane, 0, MediaId(0), 0, QF, 0.0, none(), none())
        .unwrap();
    b.region(seq, lane, 0, MediaId(1), 0, QF / 2, 0.0, none(), none())
        .unwrap();
    let (s, _) = run(b.doc(), seq, &[]);
    assert_eq!(
        segs(&s.audio, 0),
        vec![(0, QF / 2, 1, 0), (QF / 2, QF / 2, 0, QF / 2)]
    );
}

#[test]
fn two_lanes_overlap_freely() {
    let (mut b, seq, lane) = base();
    let other = b.lane(seq, "Two", None).unwrap();
    b.region(seq, lane, 0, MediaId(0), 0, 2 * QF, 0.0, none(), none())
        .unwrap();
    b.region(seq, other, 0, MediaId(1), 0, 2 * QF, 0.0, none(), none())
        .unwrap();
    let (s, _) = run(b.doc(), seq, &[]);
    // Both lanes took output path 0: two overlapping segments there.
    assert_eq!(
        segs(&s.audio, 0),
        vec![(0, 2 * QF, 0, 0), (0, 2 * QF, 1, 0)]
    );
    let hits: Vec<u64> = s
        .audio
        .overlapping(OutputPathId(0), QF, QF + 10)
        .map(|g| g.media.0)
        .collect();
    assert_eq!(hits, vec![0, 1]);
    assert_eq!(
        s.audio.overlapping(OutputPathId(0), 2 * QF, 3 * QF).count(),
        0
    );
}

#[test]
fn looped_lane_repeats_regions_and_cuts_at_its_length() {
    let (mut b, seq, lane) = base();
    // A 3-beat region in a 2-beat looped lane, in a 4-bar sequence.
    b.region(seq, lane, 0, MediaId(0), 0, 3 * QF, 0.0, none(), none())
        .unwrap();
    b.run(Command::Lane(LaneCmd::SetLength {
        seq,
        lane,
        length: Some(Tick(2 * Q)),
        looped: true,
        locked: false,
    }))
    .unwrap();
    let mut b2 = b.clone();
    let filler = b2.lane(seq, "Len", None).unwrap();
    b2.notes(seq, filler, &[(4 * BAR - Q, 60, 100, Q)]).unwrap();
    let (s, _) = run(b2.doc(), seq, &[]);
    let got = segs(&s.audio, 0);
    assert_eq!(got.len(), 8, "{got:?}");
    for (k, g) in got.iter().enumerate() {
        assert_eq!(*g, (k as u64 * 2 * QF, 2 * QF, 0, 0));
    }
}

#[test]
fn references_place_their_regions_in_the_parent_tempo() {
    let mut b = Builder::new();
    let (child, cl) = b.sequence("Loop", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", 480_000, 2).unwrap();
    b.region(child, cl, 0, m, 0, QF, 0.0, none(), none())
        .unwrap();
    let (song, sl) = b.sequence("Song", "Refs", None).unwrap();
    b.reference(song, sl, BAR, child, 2, 0).unwrap();
    let (s, _) = run(b.doc(), song, &[]);
    // The child's length rounds up to a barline (4 QF); two uses from bar 2.
    assert_eq!(
        segs(&s.audio, 0),
        vec![(4 * QF, QF, 0, 0), (8 * QF, QF, 0, 0)]
    );
    // Plan 5d-1 Ruling 6: they belong to the song's lane holding the
    // reference, the lane that records in the song.
    assert!(
        s.audio
            .segments(OutputPathId(0))
            .iter()
            .all(|g| g.top_lane == sl)
    );
    // A region directly on a lane belongs to that lane.
    let (c, _) = run(b.doc(), child, &[]);
    assert!(
        c.audio
            .segments(OutputPathId(0))
            .iter()
            .all(|g| g.top_lane == cl)
    );
}

#[test]
fn lanes_without_an_output_and_offline_media() {
    let (mut b, seq, lane) = base();
    b.region(seq, lane, 0, MediaId(0), 0, QF, 0.0, none(), none())
        .unwrap();
    b.region(seq, lane, Q, MediaId(1), 0, QF, 0.0, none(), none())
        .unwrap();
    let (s, r) = run(b.doc(), seq, &[1]);
    let online: Vec<bool> = s
        .audio
        .segments(OutputPathId(0))
        .iter()
        .map(|g| g.online)
        .collect();
    assert_eq!(online, vec![true, false]);
    assert_eq!(r.audio_offline, 1);
    assert_eq!(s.audio.needs().len(), 1);
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("1 audio region(s) play silence"))
    );
    // Clear the lane's output: nothing plays, and it says so.
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane,
        output: None,
    }))
    .unwrap();
    let (s, r) = run(b.doc(), seq, &[]);
    assert!(s.audio.is_empty());
    assert_eq!(r.audio_unrouted, 2);
    assert!(r.warnings.iter().any(|w| w.contains("no output path")));
}

#[test]
fn fades_are_relative_to_the_region_as_written() {
    let (mut b, seq, lane) = base();
    b.region(
        seq,
        lane,
        0,
        MediaId(0),
        100,
        4 * QF,
        0.0,
        fade(1000, FadeShape::Linear),
        fade(2000, FadeShape::Linear),
    )
    .unwrap();
    // A later region cuts the first one in the middle; the resumed piece
    // still fades out at the region's own end.
    b.region(seq, lane, Q, MediaId(1), 0, QF, 0.0, none(), none())
        .unwrap();
    let (s, _) = run(b.doc(), seq, &[]);
    let all = s.audio.segments(OutputPathId(0));
    let (first, last) = (all[0], all[2]);
    assert_eq!(first.region_offset, 100);
    assert_eq!(first.gain_at(100), 0.0);
    assert!((first.gain_at(100 + 500) - 0.5).abs() < 1e-6);
    assert_eq!(first.gain_at(100 + 1000), 1.0);
    // The region's last frame is 100 + 4 QF - 1: 1/2000 of the way in.
    let end = 100 + 4 * QF;
    assert!((last.gain_at(end - 1) - 1.0 / 2000.0).abs() < 1e-6);
    assert!((last.gain_at(end - 1000) - 0.5).abs() < 1e-6);
    assert_eq!(last.gain_at(end - 2001), 1.0);
}

#[test]
fn fade_curves_at_their_ends_and_middle() {
    for shape in [
        FadeShape::Linear,
        FadeShape::EqualPower,
        FadeShape::Exponential,
        FadeShape::SCurve,
    ] {
        assert_eq!(fade_curve(shape, 0.0), 0.0, "{shape:?}");
        assert!((fade_curve(shape, 1.0) - 1.0).abs() < 1e-6, "{shape:?}");
        // Monotonic.
        let mut last = 0.0;
        for k in 1..=100 {
            let v = fade_curve(shape, k as f32 / 100.0);
            assert!(v >= last, "{shape:?} at {k}");
            last = v;
        }
    }
    assert!((fade_curve(FadeShape::Linear, 0.25) - 0.25).abs() < 1e-6);
    // Equal power: sin(π/4) at the middle.
    assert!(
        (fade_curve(FadeShape::EqualPower, 0.5) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6
    );
    // Exponential over 60 dB: -30 dB (minus the floor) at the middle.
    let mid = fade_curve(FadeShape::Exponential, 0.5);
    assert!((mid - (0.031_622_78 - 0.001) / 0.999).abs() < 1e-5, "{mid}");
    // The S-curve is symmetric about the middle.
    let a = fade_curve(FadeShape::SCurve, 0.3);
    let b = fade_curve(FadeShape::SCurve, 0.7);
    assert!((a * a + b * b - 1.0).abs() < 1e-5);
    // Out of range clamps.
    assert_eq!(fade_curve(FadeShape::Linear, -1.0), 0.0);
    assert_eq!(fade_curve(FadeShape::Linear, 2.0), 1.0);
}

#[test]
fn a_pickup_region_plays_from_frame_zero() {
    let (mut b, seq, lane) = base();
    b.run(Command::Sequence(
        core_model::command::SequenceCmd::SetStartOffset {
            seq,
            offset: Tick(Q),
        },
    ))
    .unwrap();
    // Schedule frame 0 is the pickup start, one beat before bar 1.
    b.region(seq, lane, -Q, MediaId(0), 0, 3 * QF, 0.0, none(), none())
        .unwrap();
    b.region(seq, lane, 2 * Q, MediaId(1), 0, QF, 0.0, none(), none())
        .unwrap();
    let (s, _) = run(b.doc(), seq, &[]);
    assert_eq!(
        segs(&s.audio, 0),
        vec![(0, 3 * QF, 0, 0), (3 * QF, QF, 1, 0)]
    );
    // A region starting before the playing window does not play (flatten
    // keeps only events inside it); `build`'s cut of negative frames is
    // the guard for rounding at the window's start.
    let mut b2 = b.clone();
    b2.region(
        seq,
        lane,
        -2 * Q,
        MediaId(1),
        0,
        4 * QF,
        0.0,
        none(),
        none(),
    )
    .unwrap();
    let (s2, _) = run(b2.doc(), seq, &[]);
    assert_eq!(segs(&s2.audio, 0), segs(&s.audio, 0));
}

#[test]
fn segment_gain_combines_fades() {
    let g = Segment {
        start: 0,
        frames: 10,
        media: MediaId(0),
        file_frame: 0,
        gain: 0.5,
        region_offset: 0,
        region_frames: 10,
        fade_in: fade(10, FadeShape::Linear),
        fade_out: fade(10, FadeShape::Linear),
        online: true,
        top_lane: LaneId(0),
    };
    // In at 0.5, out (5 frames left of 10) at 0.5: 0.5 × 0.5 × 0.5.
    assert!((g.gain_at(5) - 0.125).abs() < 1e-6);
}
