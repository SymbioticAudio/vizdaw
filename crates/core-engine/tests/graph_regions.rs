//! The Regions node (plan 5a Task 8): segments of one output path read
//! through the streamer, with gain and fades, into that path's strip.

use core_engine::fixtures::{Builder, Q};
use core_engine::graph::exec::{Graph, Runner};
use core_engine::graph::{GraphSpec, InputSpec, NodeSpec, StripSpec, Tap, mixer_spec, plan};
use core_engine::schedule::{AudioSchedule, CompileOptions, TargetKind, TargetTable, compile};
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile, StreamConfig, Streamer};
use core_model::event::{Fade, FadeShape};
use core_model::format::BitDepth;
use core_model::ids::{MediaId, OutputPathId};
use core_model::mixer::StripKind;
use std::path::Path;
use std::sync::Arc;

const QF: u64 = 24_000;

/// Writes a CAF whose frame f is `(f + 1) / 1e6` left and its negative
/// right (or mono).
fn caf(path: &Path, frames: u64, ch: u16) {
    let mut w = CafWriter::create(path, 48_000, ch, BitDepth::Float32, WriteMode::Normal).unwrap();
    let data: Vec<f32> = (0..frames)
        .flat_map(|f| {
            let x = (f + 1) as f32 / 1e6;
            if ch == 2 { vec![x, -x] } else { vec![x] }
        })
        .collect();
    w.write(&data).unwrap();
    w.finish().unwrap();
}

fn value(f: u64) -> f32 {
    (f + 1) as f32 / 1e6
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

/// The document's audio schedule (sequence "Song").
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

#[test]
fn a_region_plays_its_frames_with_gain_and_fades() {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"), 100_000, 2);
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", 100_000, 2).unwrap();
    b.region(
        seq,
        lane,
        Q,
        m,
        10,
        2000,
        -6.020_6,
        Fade {
            length_frames: 100,
            shape: FadeShape::Linear,
        },
        Fade::default(),
    )
    .unwrap();
    let audio = Arc::new(schedule(&b));
    let (s, r) = streamer(dir.path(), &[(m.0, "a.caf")]);
    s.set_plan(Arc::from(audio.needs()));
    s.hint(QF - 512);
    s.service();
    let mut run = Runner::serial(graph());
    run.set_regions(Arc::clone(&audio), r, QF - 512);
    let (l, rr) = run.render(3072, &[512]);
    // Silence before the region.
    assert!(l[..512].iter().all(|x| *x == 0.0));
    // Frame k of the region reads media frame 10 + k at half gain, faded in
    // over 100 frames.
    for k in [0usize, 50, 99, 100, 1999] {
        let want = value(10 + k as u64) * 0.5 * if k < 100 { k as f32 / 100.0 } else { 1.0 };
        assert!(
            (l[512 + k] - want).abs() < 1e-9,
            "left {k}: {} vs {want}",
            l[512 + k]
        );
        assert!((rr[512 + k] + want).abs() < 1e-9, "right {k}");
    }
    // Silence after it, and nothing missed.
    assert!(l[512 + 2000..].iter().all(|x| *x == 0.0));
    assert_eq!(s.counters().underruns, 0);
}

#[test]
fn mono_plays_on_both_channels_and_misses_count() {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("m.caf"), 50_000, 1);
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/m.caf", 50_000, 1).unwrap();
    b.region(
        seq,
        lane,
        0,
        m,
        0,
        40_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let audio = Arc::new(schedule(&b));
    let (s, r) = streamer(dir.path(), &[(m.0, "m.caf")]);
    s.set_plan(Arc::from(audio.needs()));
    let mut run = Runner::serial(graph());
    run.set_regions(Arc::clone(&audio), r.clone(), 0);
    // Not serviced: every block misses.
    let (l, _) = run.render(1024, &[512]);
    assert!(l.iter().all(|x| *x == 0.0));
    assert_eq!(s.counters().underruns, 2);
    s.service();
    run.set_regions(Arc::clone(&audio), r, 0);
    let (l, rr) = run.render(512, &[512]);
    assert_eq!(l[7], value(7));
    assert_eq!(rr[7], value(7));
}

#[test]
fn offline_segments_play_silence_and_count_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/gone.caf", 50_000, 2).unwrap();
    b.region(
        seq,
        lane,
        0,
        m,
        0,
        40_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let mut opts = CompileOptions::new(48_000.0);
    opts.offline_media.insert(m);
    let mut table = TargetTable::new();
    let (sch, _) = compile(b.doc(), seq, &opts, &mut |k: &TargetKind| {
        table.intern(k.clone()).map(Ok)
    })
    .unwrap();
    let (s, r) = streamer(dir.path(), &[]);
    let mut run = Runner::serial(graph());
    run.set_regions(Arc::new(sch.audio), r, 0);
    let (l, _) = run.render(2048, &[512]);
    assert!(l.iter().all(|x| *x == 0.0));
    assert_eq!(s.counters().underruns, 0);
}

#[test]
fn mixer_spec_feeds_each_audio_path_strip_from_a_regions_node() {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", 1000, 2).unwrap();
    b.region(
        seq,
        lane,
        0,
        m,
        0,
        100,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let doc = b.finish();
    let g = mixer_spec(&doc, &[], &mut |_| 0);
    let strip = doc
        .mixer()
        .find_kind(StripKind::AudioPath(OutputPathId(0)))
        .unwrap();
    let strip_node = g.strips.iter().find(|(_, id)| *id == strip).unwrap().0;
    let regions_node = g
        .spec
        .nodes
        .iter()
        .position(|n| matches!(n, NodeSpec::Regions { path } if *path == OutputPathId(0)))
        .unwrap();
    assert!(
        regions_node > strip_node,
        "regions nodes come after the strips"
    );
    let NodeSpec::Strip(s) = &g.spec.nodes[strip_node] else {
        panic!()
    };
    assert_eq!(s.inputs[0].from, regions_node);
    // Only one Regions node per path.
    let n = g
        .spec
        .nodes
        .iter()
        .filter(|n| matches!(n, NodeSpec::Regions { .. }))
        .count();
    assert_eq!(n, 1);
}
