//! Crossfades in the engine (plan 6c Task 3): under a crossfade's span the
//! earlier region plays on the complement of the later one's fade in.
//! The goldens are hand-written (spec §7.0), never blessed.

// The tables are hand-written literals, not the library's constants.
#![allow(clippy::approx_constant)]

use core_engine::fixtures::Builder;
use core_engine::graph::exec::{Graph, Runner};
use core_engine::graph::{GraphSpec, InputSpec, NodeSpec, StripSpec, Tap, plan};
use core_engine::schedule::{AudioSchedule, CompileOptions, TargetKind, TargetTable, compile};
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile, StreamConfig, Streamer};
use core_model::command::{AudioCmd, Command, CrossfadeLength};
use core_model::event::{Fade, FadeShape};
use core_model::format::BitDepth;
use core_model::ids::{EventId, MediaId, OutputPathId};
use std::path::Path;
use std::sync::Arc;

use FadeShape::{EqualPower, Exponential, Linear, SCurve};

/// The fade in at x = 0, ¼, ½, ¾ (hand-computed).
const IN: [(FadeShape, [f32; 4]); 4] = [
    (Linear, [0.0, 0.25, 0.5, 0.75]),
    (EqualPower, [0.0, 0.3826834, 0.7071068, 0.9238795]),
    (Exponential, [0.0, 0.004628, 0.0306534, 0.1770049]),
    (SCurve, [0.0, 0.2429802, 0.7071068, 0.9700313]),
];
/// Its complement at the same points.
const OUT: [(FadeShape, [f32; 4]); 4] = [
    (Linear, [1.0, 0.75, 0.5, 0.25]),
    (EqualPower, [1.0, 0.9238795, 0.7071068, 0.3826834]),
    (Exponential, [1.0, 0.1770049, 0.0306534, 0.004628]),
    (SCurve, [1.0, 0.9700313, 0.7071068, 0.2429802]),
];

/// Writes a CAF whose frame f is `(f + 1) / 1e6` (mono).
fn ramp(path: &Path, frames: u64) {
    let mut w = CafWriter::create(path, 48_000, 1, BitDepth::Float32, WriteMode::Normal).unwrap();
    let data: Vec<f32> = (0..frames).map(value).collect();
    w.write(&data).unwrap();
    w.finish().unwrap();
}

fn value(f: u64) -> f32 {
    (f + 1) as f32 / 1e6
}

/// Writes a mono 48 kHz float CAF of `frames` frames at `level`.
fn dc(path: &Path, frames: u64, level: f32) {
    let mut w = CafWriter::create(path, 48_000, 1, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![level; frames as usize]).unwrap();
    w.finish().unwrap();
}

/// A Regions node for path 0 → a strip → master.
fn graph() -> Graph {
    let mut path_strip = StripSpec::new(None);
    path_strip.inputs.push(InputSpec {
        from: 0,
        tap: Tap::Post,
        gain: 1.0,
    });
    let mut master = StripSpec::new(None);
    master.inputs.push(InputSpec {
        from: 1,
        tap: Tap::Post,
        gain: 1.0,
    });
    let spec = GraphSpec {
        nodes: vec![
            NodeSpec::Regions {
                path: OutputPathId(0),
            },
            NodeSpec::Strip(path_strip),
            NodeSpec::Strip(master),
        ],
        master: 2,
    };
    let p = plan(&spec, 96_000).unwrap();
    assert_eq!(p.latency, 0);
    Graph::build(&spec, &p, 1024, |_, _| unreachable!("no inserts"))
}

/// The document's audio schedule (its first sequence).
fn schedule(b: &Builder) -> AudioSchedule {
    let seq = *b.doc().sequences().keys().next().unwrap();
    let mut table = TargetTable::new();
    let (s, _) = compile(
        b.doc(),
        seq,
        &CompileOptions::new(48_000.0),
        &mut |k: &TargetKind| table.intern(k.clone()).map(Ok),
    )
    .unwrap();
    s.audio
}

fn streamer(dir: &Path, items: &[(u64, &str)]) -> (Streamer, core_media::stream::StreamReader) {
    let (s, r) = Streamer::new(48_000.0, StreamConfig::default(), DiskMode::Manual).unwrap();
    s.set_media(
        items
            .iter()
            .map(|(id, name)| MediaFile {
                id: MediaId(*id),
                path: dir.join(name),
            })
            .collect(),
    );
    (s, r)
}

/// Renders frames 0..2048 of `b`'s path 0 (left channel).
fn render(b: &Builder, dir: &Path) -> Vec<f32> {
    let audio = Arc::new(schedule(b));
    let (s, r) = streamer(dir, &[(0, "a.caf"), (1, "b.caf")]);
    s.set_plan(Arc::from(audio.needs()));
    s.hint(0);
    s.service();
    let mut run = Runner::serial(graph());
    run.set_regions(Arc::clone(&audio), r, 0);
    let (l, _) = run.render(2048, &[512]);
    assert_eq!(s.counters().underruns, 0);
    l
}

/// A at tick 0 for 1 200 frames and B at tick 512 (frame 800 at 120 BPM)
/// for 1 000 frames; A fades out over `a_out`. Media 0 is A's, 1 is B's.
fn pair(a_out: Fade) -> (Builder, EventId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let ma = b.media("Media/a.caf", 4000, 1).unwrap();
    let mb = b.media("Media/b.caf", 4000, 1).unwrap();
    b.region(seq, lane, 0, ma, 0, 1200, 0.0, Fade::default(), a_out)
        .unwrap();
    let ev = b
        .region(
            seq,
            lane,
            512,
            mb,
            0,
            1000,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    (b, ev)
}

fn crossfade(b: &mut Builder, event: EventId, frames: u64, shape: FadeShape) {
    b.run(Command::Audio(AudioCmd::Crossfade {
        event,
        length: CrossfadeLength::Frames(frames),
        shape,
    }))
    .unwrap();
}

fn dc_media(dir: &Path) {
    dc(&dir.join("a.caf"), 4000, 0.5);
    dc(&dir.join("b.caf"), 4000, 0.25);
}

#[test]
fn golden_renders_for_each_shape() {
    let dir = tempfile::tempdir().unwrap();
    dc_media(dir.path());
    for k in 0..4 {
        let (shape, inn) = IN[k];
        let (out_shape, out) = OUT[k];
        assert_eq!(shape, out_shape);
        let (mut b, ev) = pair(Fade::default());
        crossfade(&mut b, ev, 400, shape);
        let l = render(&b, dir.path());
        for (j, f) in [800usize, 900, 1000, 1100].into_iter().enumerate() {
            let want = 0.5 * out[j] + 0.25 * inn[j];
            assert!(
                (l[f] - want).abs() < 1e-5,
                "{shape:?} at {f}: {} vs {want}",
                l[f]
            );
        }
        for (f, want) in [(0usize, 0.5f32), (799, 0.5), (1200, 0.25), (1799, 0.25)] {
            assert!(
                (l[f] - want).abs() < 1e-5,
                "{shape:?} at {f}: {} vs {want}",
                l[f]
            );
        }
        assert_eq!(l[1800], 0.0, "{shape:?}");
    }
}

#[test]
fn crossfade_beside_a_plain_fade_out() {
    let dir = tempfile::tempdir().unwrap();
    dc_media(dir.path());
    let (mut b, ev) = pair(Fade {
        length_frames: 200,
        shape: Linear,
    });
    crossfade(&mut b, ev, 400, Linear);
    let l = render(&b, dir.path());
    for (f, want) in [(1000usize, 0.375f32), (1100, 0.25), (1150, 0.234375)] {
        assert!((l[f] - want).abs() < 1e-6, "at {f}: {} vs {want}", l[f]);
    }
}

#[test]
fn null_test_matches_a_hand_mix() {
    let dir = tempfile::tempdir().unwrap();
    ramp(&dir.path().join("a.caf"), 4000);
    // B's media is the negative ramp.
    {
        let mut w = CafWriter::create(
            &dir.path().join("b.caf"),
            48_000,
            1,
            BitDepth::Float32,
            WriteMode::Normal,
        )
        .unwrap();
        let data: Vec<f32> = (0..4000).map(|f| -value(f)).collect();
        w.write(&data).unwrap();
        w.finish().unwrap();
    }
    let (mut b, ev) = pair(Fade::default());
    crossfade(&mut b, ev, 400, EqualPower);
    let l = render(&b, dir.path());
    let a = |f: u64| f64::from(value(f));
    let bb = |f: u64| -f64::from(value(f - 800));
    let mut worst = 0f64;
    for f in 0..=1800u64 {
        let want = if f < 800 {
            a(f)
        } else if f < 1200 {
            let x = (f - 800) as f64 / 400.0;
            let q = std::f64::consts::FRAC_PI_2 * x;
            a(f) * q.cos() + bb(f) * q.sin()
        } else if f < 1800 {
            bb(f)
        } else {
            0.0
        };
        worst = worst.max((f64::from(l[f as usize]) - want).abs());
    }
    assert!(worst < 1e-6, "largest difference {worst}");
}

#[test]
fn a_region_without_a_crossfade_renders_as_before() {
    let dir = tempfile::tempdir().unwrap();
    dc_media(dir.path());
    let (b, _) = pair(Fade::default());
    let l = render(&b, dir.path());
    assert!(l[..800].iter().all(|x| *x == 0.5));
    assert!(l[800..1800].iter().all(|x| *x == 0.25));
    assert!(l[1800..].iter().all(|x| *x == 0.0));
}

#[test]
fn a_version_8_pair_loads_and_renders_as_before() {
    let (b, _) = pair(Fade::default());
    let text = core_model::file::to_json(b.doc()).unwrap();
    assert_eq!(text.matches("\"version\": 9").count(), 1);
    let v8 = text.replace("\"version\": 9", "\"version\": 8");
    let doc = core_model::file::from_json(&v8).unwrap();
    let audio = schedule(&Builder::from_doc(doc));
    let segs = audio.segments(OutputPathId(0));
    let got: Vec<(u64, u64, u64)> = segs
        .iter()
        .map(|s| (s.start, s.frames, s.media.0))
        .collect();
    assert_eq!(got, vec![(0, 800, 0), (800, 1000, 1)]);
    assert!(segs.iter().all(|s| s.xfade.length_frames == 0));
}

/// Plan 6c Task 3 (controller): at a tempo that puts region boundaries on
/// fractional frames, the model's partner and overlap and the engine's
/// playing span agree exactly.
#[test]
fn model_and_engine_agree_on_fractional_frames() {
    let mut checked = 0;
    for bpm in [97u32, 120, 133] {
        for a_tick in [0i64, 3, 8, 701] {
            for gap in [1i64, 7, 256, 517, 619, 1000] {
                let mut b = Builder::new();
                let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
                b.tempo(seq, &[(0, bpm, false)]).unwrap();
                let ma = b.media("Media/a.caf", 4000, 1).unwrap();
                let mb = b.media("Media/b.caf", 4000, 1).unwrap();
                let a = b
                    .region(
                        seq,
                        lane,
                        a_tick,
                        ma,
                        0,
                        1201,
                        0.0,
                        Fade::default(),
                        Fade::default(),
                    )
                    .unwrap();
                let ev = b
                    .region(
                        seq,
                        lane,
                        a_tick + gap,
                        mb,
                        0,
                        2000,
                        0.0,
                        Fade::default(),
                        Fade::default(),
                    )
                    .unwrap();
                let o = core_model::crossfade::overlap(b.doc(), ev).unwrap();
                let audio = schedule(&b);
                let segs = audio.segments(OutputPathId(0));
                let b_seg = *segs.iter().find(|s| s.media == mb).unwrap();
                let a_end = segs
                    .iter()
                    .filter(|s| s.media == ma)
                    .map(|s| s.end())
                    .max()
                    .unwrap();
                let Some(o) = o else {
                    // No overlap in the model: the engine plays A to its
                    // end before B starts.
                    assert!(a_end <= b_seg.start, "{bpm} {a_tick} {gap}");
                    continue;
                };
                assert_eq!(o.with, a);
                b.run(Command::Audio(AudioCmd::Crossfade {
                    event: ev,
                    length: CrossfadeLength::UpTo(1_000_000),
                    shape: Linear,
                }))
                .unwrap();
                let audio = schedule(&b);
                let segs = audio.segments(OutputPathId(0));
                let extra: Vec<_> = segs.iter().filter(|s| s.xfade.length_frames > 0).collect();
                assert_eq!(extra.len(), 1, "{bpm} {a_tick} {gap}");
                let x = extra[0];
                assert_eq!(x.media, ma);
                assert_eq!(x.start, b_seg.start, "{bpm} {a_tick} {gap}");
                assert_eq!(x.frames, o.frames, "{bpm} {a_tick} {gap}");
                assert_eq!(x.xfade.length_frames, o.frames, "{bpm} {a_tick} {gap}");
                checked += 1;
            }
        }
    }
    assert!(checked >= 30, "{checked}");
}
