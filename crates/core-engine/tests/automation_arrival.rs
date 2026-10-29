//! Automation at each strip's arrival (plan 5e-1 Ruling 5; 5d-1
//! controller ruling C2): a strip's controls are read at the frame its
//! audio is at, so a point at bar 3 changes what is heard where bar 3 is
//! heard, on a led (read-ahead) path as on a delayed one.

use core_engine::backend::{AudioBackend, StereoBuf};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::mixer::mixer_spec;
use core_engine::graph::{
    EffectNode, Graph, InsertFactory, InsertInit, InstrumentFeed, ProcessStatus, TestInserts, plan,
};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::schedule::TargetIdx;
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile, StreamConfig};
use core_model::automation::AutomationTarget;
use core_model::document::Document;
use core_model::event::{EventBody, Fade};
use core_model::format::BitDepth;
use core_model::ids::{MediaId, SeqId};
use core_model::mixer::{ParamId, PluginSlot, StripKind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const QF: u64 = 24_000;
const FILE_FRAMES: u64 = 480_000;
const BLOCK: u32 = 512;

fn caf(path: &Path) {
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![0.25f32; 2 * FILE_FRAMES as usize]).unwrap();
    w.finish().unwrap();
}

/// A region from beat 1 to beat 7 on lane "Audio"; a latent bus (latency
/// `l`) so the audio path leads by `l`; the audio path's strip unmuted at
/// 0 and muted from beat 3.
fn doc(l: u32) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FILE_FRAMES, 2).unwrap();
    b.region(
        seq,
        lane,
        Q,
        m,
        0,
        6 * QF,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let bus = b.bus("Latent").unwrap();
    b.insert(bus, TestInserts::plugin(&format!("latency={l}")))
        .unwrap();
    let out = b.doc().lane(seq, lane).unwrap().audio_out.unwrap();
    let path = b.strip_of(StripKind::AudioPath(out)).unwrap();
    let mute = AutomationTarget::StripMute(path);
    b.event(
        seq,
        lane,
        0,
        EventBody::Automation {
            target: mute,
            value: 0.0,
        },
    )
    .unwrap();
    b.event(
        seq,
        lane,
        3 * Q,
        EventBody::Automation {
            target: mute,
            value: 1.0,
        },
    )
    .unwrap();
    (b.finish(), seq)
}

/// The left channel of `blocks` blocks from bar 1, with or without leads.
fn render(doc: &Document, seq: SeqId, leads: bool, blocks: usize) -> Vec<f32> {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"));
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = DiskMode::Manual;
    cfg.stream = StreamConfig {
        cache_file_bytes: 0,
        ..StreamConfig::default()
    };
    cfg.region_leads = leads;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_media(vec![MediaFile {
        id: MediaId(0),
        path: dir.path().join("a.caf"),
    }]);
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    engine.play().unwrap();
    let mut left = Vec::new();
    for _ in 0..blocks {
        engine.streamer().service();
        out.block(BLOCK);
        left.extend_from_slice(out.output(BLOCK).0);
    }
    left
}

/// Review Focus 1: the mute at beat 3 silences the path where beat 3 of
/// its audio is heard, `l` frames after beat 3, whether the path leads or
/// is delayed (before plan 5e-1 it fell `l` frames early on the led
/// path, whose strip's audio arrives `l` behind the renderer).
#[test]
fn a_led_paths_mute_lands_on_its_audio() {
    for l in [512u32, 4800] {
        let (d, seq) = doc(l);
        for leads in [true, false] {
            let left = render(&d, seq, leads, 400);
            let at = (3 * QF + u64::from(l)) as usize;
            assert!(
                left[at - 1] != 0.0,
                "heard up to beat 3: l {l}, leads {leads}"
            );
            assert!(
                left[at..at + 4800].iter().all(|x| *x == 0.0),
                "silent from beat 3 as heard: l {l}, leads {leads}"
            );
            assert!(
                left[(QF + u64::from(l)) as usize + 10] != 0.0,
                "the region plays before"
            );
        }
    }
}

/// Ruling 5's arrivals: a strip's controls at its node's output time, an
/// insert's parameter at its strip's input time plus the inserts before
/// it, 0 for strips without latency.
#[test]
fn arrivals_follow_the_plan() {
    let mut b = Builder::new();
    b.dest("Synth", "builtin:synth", 0).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let bus = b.bus("Bus").unwrap();
    let first = b.insert(bus, TestInserts::plugin("latency=100")).unwrap();
    let second = b.insert(bus, TestInserts::plugin("latency=50")).unwrap();
    let master = b.doc().mixer().master();
    let d = b.finish();
    let mut latency = |s: &PluginSlot| {
        if s.plugin.id.ends_with("100") {
            100
        } else {
            50
        }
    };
    let mg = mixer_spec(
        &d,
        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
        &mut latency,
    );
    let p = plan(&mg.spec, 96_000).unwrap();
    let g = Graph::build(&mg.spec, &p, 512, |_, _| InsertInit::Placeholder);
    assert_eq!(g.strip_arrival(synth), 0);
    assert_eq!(g.strip_arrival(bus), 150);
    assert_eq!(g.insert_arrival(first), 0);
    assert_eq!(g.insert_arrival(second), 100);
    assert_eq!(
        g.strip_arrival(master),
        150,
        "master's inputs arrive aligned to the bus"
    );
    assert_eq!(g.arrivals(), &[0, 100, 150]);
    assert_eq!(g.strip_arrival(core_model::ids::StripId(999)), 0);
}

/// An insert that counts its `process` calls: one per run the renderer
/// makes.
struct Counted(Arc<AtomicU64>, Box<dyn EffectNode>);

impl EffectNode for Counted {
    fn latency(&self) -> u32 {
        self.1.latency()
    }
    fn process(
        &mut self,
        frames: u32,
        io: &mut StereoBuf,
        sc: Option<&StereoBuf>,
    ) -> ProcessStatus {
        self.0.fetch_add(1, Ordering::Relaxed);
        self.1.process(frames, io, sc)
    }
    fn reset(&mut self) {
        self.1.reset();
    }
    fn automate(&mut self, frame: u32, param: ParamId, v: f64) -> bool {
        self.1.automate(frame, param, v)
    }
}

struct Counting(TestInserts, Arc<AtomicU64>);

impl InsertFactory for Counting {
    fn make(
        &mut self,
        slot: &PluginSlot,
        sr: f64,
        max: u32,
    ) -> Result<Box<dyn EffectNode>, String> {
        Ok(Box::new(Counted(
            Arc::clone(&self.1),
            self.0.make(slot, sr, max)?,
        )))
    }
}

/// Pre-flight I13 (Ruling 5): each automation point cuts a run once,
/// however many distinct arrivals the graph has, so a latent session is
/// not rendered in slices around every point. Two inserts on a bus count
/// the runs; with latencies 100 and 150 (arrivals 0, 100, 250) the runs
/// are the same as with latencies 0 and 0 (arrival 0 only).
#[test]
fn each_point_cuts_once_on_a_latent_session() {
    let runs = |latencies: [u32; 2]| -> u64 {
        let mut b = Builder::new();
        let d = b.dest("Synth", "builtin:synth", 0).unwrap();
        let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
        b.notes(seq, notes, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
        let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
        let bus = b.bus("A").unwrap();
        for l in latencies {
            b.insert(bus, TestInserts::plugin(&format!("latency={l}")))
                .unwrap();
        }
        let fader = AutomationTarget::StripFader(synth);
        for (t, v) in [(0, 0.2), (Q / 4, 0.8), (Q / 2, 0.3), (3 * Q / 4, 0.6)] {
            b.event(
                seq,
                notes,
                t,
                EventBody::Automation {
                    target: fader,
                    value: v,
                },
            )
            .unwrap();
        }
        let doc = b.finish();
        let count = Arc::new(AtomicU64::new(0));
        let (mut engine, renderer) =
            Engine::new(EngineConfig::new(48_000.0), Box::new(OfflineSinks::new(64))).unwrap();
        engine.set_insert_factory(Box::new(Counting(TestInserts::new(), Arc::clone(&count))));
        engine.load(&doc, seq).unwrap();
        let mut out = OfflineBackend::new(48_000.0, BLOCK);
        out.start(renderer).unwrap();
        out.reserve(4096);
        engine.play().unwrap();
        out.block(BLOCK);
        let before = count.load(Ordering::Relaxed);
        for _ in 0..60 {
            out.block(BLOCK);
        }
        count.load(Ordering::Relaxed) - before
    };
    let plain = runs([0, 0]);
    assert!(plain > 2 * 60, "the points cut some runs: {plain}");
    assert_eq!(
        runs([100, 150]),
        plain,
        "a latent session cuts at the same points"
    );
}

/// The `doc` shape with the path muted from beat 3.5 on (unmuted from 0).
fn seam_doc(l: u32) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FILE_FRAMES, 2).unwrap();
    b.region(
        seq,
        lane,
        Q,
        m,
        0,
        6 * QF,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let bus = b.bus("Latent").unwrap();
    b.insert(bus, TestInserts::plugin(&format!("latency={l}")))
        .unwrap();
    let out = b.doc().lane(seq, lane).unwrap().audio_out.unwrap();
    let path = b.strip_of(StripKind::AudioPath(out)).unwrap();
    let mute = AutomationTarget::StripMute(path);
    b.event(
        seq,
        lane,
        0,
        EventBody::Automation {
            target: mute,
            value: 0.0,
        },
    )
    .unwrap();
    b.event(
        seq,
        lane,
        7 * Q / 2,
        EventBody::Automation {
            target: mute,
            value: 1.0,
        },
    )
    .unwrap();
    (b.finish(), seq)
}

/// The left channel of `blocks` blocks from bar 1, looping `range`.
fn render_looped(
    doc: &Document,
    seq: SeqId,
    leads: bool,
    range: (u64, u64),
    blocks: usize,
) -> Vec<f32> {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"));
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = DiskMode::Manual;
    cfg.stream = StreamConfig {
        cache_file_bytes: 0,
        ..StreamConfig::default()
    };
    cfg.region_leads = leads;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_media(vec![MediaFile {
        id: MediaId(0),
        path: dir.path().join("a.caf"),
    }]);
    engine.load(doc, seq).unwrap();
    engine.set_loop_range(Some(range)).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    engine.play().unwrap();
    let mut left = Vec::new();
    for _ in 0..blocks {
        engine.streamer().service();
        out.block(BLOCK);
        left.extend_from_slice(out.output(BLOCK).0);
    }
    left
}

/// Review I1 (Task 4 fix round 1): at a loop wrap, a strip behind latency
/// reads its automation from the loop's start as soon as its audio does,
/// not past the loop's end for the rest of the run: muted at the loop's
/// end and unmuted at its start, the second pass is heard from its first
/// frame, led or delayed.
#[test]
fn a_latent_strips_automation_follows_the_loop_seam() {
    let l = 100u32;
    let (d, seq) = seam_doc(l);
    let (s, e) = (2 * QF, 4 * QF + 300);
    for leads in [true, false] {
        let left = render_looped(&d, seq, leads, (s, e), 220);
        // The second pass's loop start is heard at renderer frame `e`
        // plus the latency.
        let at = (e + u64::from(l)) as usize;
        assert_eq!(left[at - 1], 0.0, "the loop's end is muted: leads {leads}");
        let lost = left[at..at + 4800].iter().filter(|x| **x == 0.0).count();
        assert_eq!(
            lost, 0,
            "the second pass is heard from its start: leads {leads}"
        );
    }
}

/// Review M3 (Task 4 fix round 1): a point in the last `l` frames of the
/// sequence still applies to a strip behind latency `l`, while its audio
/// plays out past the end: muted 4000 frames before the region's (and
/// the sequence's, bar 3) end, the path is silent from there as heard,
/// led or delayed.
#[test]
fn a_point_near_the_end_applies_while_the_end_plays_out() {
    let l = 4800u32;
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FILE_FRAMES, 2).unwrap();
    b.region(
        seq,
        lane,
        Q,
        m,
        0,
        7 * QF,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let bus = b.bus("Latent").unwrap();
    b.insert(bus, TestInserts::plugin(&format!("latency={l}")))
        .unwrap();
    let out = b.doc().lane(seq, lane).unwrap().audio_out.unwrap();
    let path = b.strip_of(StripKind::AudioPath(out)).unwrap();
    let mute = AutomationTarget::StripMute(path);
    // 2560 ticks are 4000 frames at 120 bpm.
    b.event(
        seq,
        lane,
        0,
        EventBody::Automation {
            target: mute,
            value: 0.0,
        },
    )
    .unwrap();
    b.event(
        seq,
        lane,
        8 * Q - 2560,
        EventBody::Automation {
            target: mute,
            value: 1.0,
        },
    )
    .unwrap();
    let d = b.finish();
    for leads in [true, false] {
        let left = render(&d, seq, leads, 400);
        let at = (8 * QF - 4000 + u64::from(l)) as usize;
        assert!(left[at - 10] != 0.0, "heard up to the point: leads {leads}");
        let heard = left[at..at + 3900].iter().filter(|x| **x != 0.0).count();
        assert_eq!(heard, 0, "silent from the point as heard: leads {leads}");
    }
}

/// Ruling 5 as refined by the controller (Task 4): mute, solo and send
/// gains act on the fed edge, after its delay line, so their arrival is
/// the fed node's input time; a strip without such an edge (master) or a
/// graph without controls answers the strip's own arrival.
#[test]
fn edge_gains_arrive_at_the_fed_node() {
    let mut b = Builder::new();
    b.dest("Synth", "builtin:synth", 0).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let bus = b.bus("Bus").unwrap();
    b.insert(bus, TestInserts::plugin("latency=150")).unwrap();
    let master = b.doc().mixer().master();
    let d = b.finish();
    let mut latency = |_: &PluginSlot| 150;
    let mg = mixer_spec(
        &d,
        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
        &mut latency,
    );
    let p = plan(&mg.spec, 96_000).unwrap();
    let mut g = Graph::build(&mg.spec, &p, 512, |_, _| InsertInit::Placeholder);
    assert_eq!(
        g.feed_arrival(synth, None),
        0,
        "no controls: the strip's own"
    );
    g.set_controls(&mg.controls);
    assert_eq!(g.strip_arrival(synth), 0, "its fader acts at its output");
    assert_eq!(
        g.feed_arrival(synth, None),
        150,
        "its mute acts after master's delay"
    );
    assert_eq!(g.feed_arrival(bus, None), 150);
    assert_eq!(g.feed_arrival(master, None), 150, "master: its own arrival");
    assert_eq!(g.arrivals(), &[0, 150]);
}

/// Final review m1 (Task 4 re-review N1): a lane that holds one value
/// across its loop seam does not cut the run there, so 16 buses behind
/// distinct latencies with flat fader lanes render a looping beat in as
/// many runs as with no latency, instead of one more run per bus at each
/// wrap.
#[test]
fn flat_lanes_do_not_cut_runs_at_the_loop_seam() {
    let runs = |latent: bool| -> u64 {
        let mut b = Builder::new();
        let d = b.dest("Synth", "builtin:synth", 0).unwrap();
        let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
        b.notes(seq, notes, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
        for i in 0..16u32 {
            let bus = b.bus(&format!("B{i}")).unwrap();
            let l = if latent { 1000 + i } else { 0 };
            b.insert(bus, TestInserts::plugin(&format!("latency={l}")))
                .unwrap();
            for t in [0, 4 * Q] {
                b.event(
                    seq,
                    notes,
                    t,
                    EventBody::Automation {
                        target: AutomationTarget::StripFader(bus),
                        value: 0.5,
                    },
                )
                .unwrap();
            }
        }
        let doc = b.finish();
        let count = Arc::new(AtomicU64::new(0));
        let (mut engine, renderer) =
            Engine::new(EngineConfig::new(48_000.0), Box::new(OfflineSinks::new(64))).unwrap();
        engine.set_insert_factory(Box::new(Counting(TestInserts::new(), Arc::clone(&count))));
        engine.load(&doc, seq).unwrap();
        engine.set_loop_range(Some((0, QF))).unwrap();
        let mut out = OfflineBackend::new(48_000.0, BLOCK);
        out.start(renderer).unwrap();
        out.reserve(4096);
        engine.play().unwrap();
        // Past the roll (each lane's read starts once at its floor).
        for _ in 0..8 {
            out.block(BLOCK);
        }
        let before = count.load(Ordering::Relaxed);
        // About four wraps.
        for _ in 0..200 {
            out.block(BLOCK);
        }
        (count.load(Ordering::Relaxed) - before) / 16
    };
    let plain = runs(false);
    assert!(plain >= 200, "{plain}");
    assert_eq!(runs(true), plain, "flat lanes add no runs at the seams");
}
