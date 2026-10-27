//! Regions read ahead instead of being delayed (plan 5d-1 Rulings 2–3;
//! sub-project 5 spec §6.6, Ruling 33): with a latent plugin elsewhere in
//! the mix, a led audio path plays exactly what the compensation delay
//! would have played, through loop wraps, Play Selection and the end of
//! the sequence; after a locate it starts silent, then aligned.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile, StreamConfig};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::format::BitDepth;
use core_model::ids::{MediaId, SeqId};
use core_model::time::Tick;
use std::path::Path;

const QF: u64 = 24_000;
const FILE_FRAMES: u64 = 480_000;
const BLOCK: u32 = 512;

fn value(f: u64) -> f32 {
    ((f % 997) as f32 + 1.0) / 2000.0
}

fn caf(path: &Path) {
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    let data: Vec<f32> = (0..FILE_FRAMES)
        .flat_map(|f| [value(f), -value(f)])
        .collect();
    w.write(&data).unwrap();
    w.finish().unwrap();
}

/// One region (beat 1, offset 100, six beats) on lane "Audio"; with
/// `latency` > 0 a bus with a test insert of that latency feeds master,
/// so the graph latency is `latency` and the audio path leads by it.
fn doc(latency: u32) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FILE_FRAMES, 2).unwrap();
    b.region(
        seq,
        lane,
        Q,
        m,
        100,
        6 * QF,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    if latency > 0 {
        let bus = b.bus("Latent").unwrap();
        b.insert(bus, TestInserts::plugin(&format!("latency={latency}")))
            .unwrap();
    }
    (b.finish(), seq)
}

struct Rig {
    engine: Engine,
    out: OfflineBackend,
    _dir: tempfile::TempDir,
}

/// Every region streams (no RAM cache), so the streamer's read head is
/// exercised too.
fn rig(doc: &Document, seq: SeqId, leads: bool) -> Rig {
    rig_with(doc, seq, leads, StreamConfig::default().voice_seconds)
}

/// [`rig`] with a streaming voice of `voice_seconds`.
fn rig_with(doc: &Document, seq: SeqId, leads: bool, voice_seconds: f64) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"));
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = DiskMode::Manual;
    cfg.stream = StreamConfig {
        cache_file_bytes: 0,
        voice_seconds,
        ..StreamConfig::default()
    };
    if voice_seconds < 2.0 {
        // The window ahead shrinks with the ring.
        cfg.stream.ahead_seconds = voice_seconds / 2.0;
        cfg.stream.seam_seconds = voice_seconds / 4.0;
        cfg.stream.ready_seconds = 0.01;
        cfg.stream.min_read_bytes = 4096;
    }
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
    Rig {
        engine,
        out,
        _dir: dir,
    }
}

impl Rig {
    /// Renders `blocks` blocks, servicing the disk first; both channels.
    fn render(&mut self, blocks: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (Vec::new(), Vec::new());
        for _ in 0..blocks {
            self.engine.streamer().service();
            self.out.block(BLOCK);
            let (a, b) = self.out.output(BLOCK);
            l.extend_from_slice(a);
            r.extend_from_slice(b);
        }
        (l, r)
    }
}

/// What `scenario` renders with and without leads, for graph latency `l`.
fn both(l: u32, scenario: impl Fn(&mut Rig) -> (Vec<f32>, Vec<f32>)) -> [(Vec<f32>, Vec<f32>); 2] {
    let (d, seq) = doc(l);
    [true, false].map(|leads| {
        let mut r = rig(&d, seq, leads);
        assert_eq!(r.engine.latency_frames(), u64::from(l));
        let got = scenario(&mut r);
        let f = r.engine.faults();
        assert!(
            !f.contains(Faults::STREAM_UNDERRUN) && !f.contains(Faults::LATE_START),
            "leads {leads}: {:?}",
            f.names()
        );
        got
    })
}

/// Ruling 3: playing to the end, looping and Play Selection sound
/// bit-exactly as the compensation delay did, for latencies around the
/// block size and the 100 ms a look-ahead limiter may add.
#[test]
fn regions_read_ahead_play_exactly_what_a_delay_would() {
    for l in [1u32, 511, 512, 4800] {
        // From bar 1 through the region's end (beat 7) and the tail.
        let [led, delayed] = both(l, |r| {
            r.engine.play().unwrap();
            r.render(460)
        });
        assert_eq!(led, delayed, "to the end, latency {l}");
        // The region's last frame is heard L late, in both.
        let last = (7 * QF - 1 + u64::from(l)) as usize;
        assert!(led.0[last] != 0.0 && led.0[last + 1] == 0.0, "latency {l}");
        // A loop over beats 2–4, wrapping six times.
        let [led, delayed] = both(l, |r| {
            r.engine.set_loop_range(Some((2 * QF, 4 * QF))).unwrap();
            r.engine.locate_frame(2 * QF).unwrap();
            r.engine.play().unwrap();
            r.render(600)
        });
        assert_eq!(led, delayed, "looping, latency {l}");
        // Play Selection to beat 3 plays out its last frames.
        let [led, delayed] = both(l, |r| {
            r.engine.locate_frame(QF).unwrap();
            r.engine.play_until(Tick(3 * Q)).unwrap();
            r.render(300)
        });
        assert_eq!(led, delayed, "play selection, latency {l}");
    }
}

/// Ruling 3: after a locate a led path reads nothing from before the new
/// position: it is silent for its lead, then plays the new position as
/// it is heard.
#[test]
fn a_located_lead_path_starts_silent_and_aligned() {
    const L: u64 = 4800;
    let (d, seq) = doc(L as u32);
    let mut r = rig(&d, seq, true);
    r.engine.play().unwrap();
    let (before, _) = r.render(100);
    // Heard from beat 1 plus L: the pan law scales by a constant k.
    let k = before[(QF + L) as usize] / value(100);
    assert!(k > 0.5 && k <= 1.0, "{k}");
    r.engine.locate_frame(4 * QF).unwrap();
    let (after, _) = r.render(40);
    assert!(
        after[..L as usize].iter().all(|x| *x == 0.0),
        "nothing from before the locate is heard"
    );
    for i in 0..10_000u64 {
        let want = k * value(100 + 4 * QF - QF + i);
        let got = after[(L + i) as usize];
        assert!((got - want).abs() < 1e-6, "frame {i}: {got} for {want}");
    }
}

/// Plan 5d-1 M3: a lead never exceeds half a streaming voice. Path 1 has
/// no plugin and leads by 4800 frames over path 2, which carries a
/// latency-4800 insert; on a 0.15 s voice (7200 frames) the streamer must
/// hold the span between the two reads (the lead) plus its window ahead,
/// so the lead is capped at 3600 and nothing underruns. The sound is
/// still what a delay would have made.
#[test]
fn a_lead_is_capped_by_half_a_streaming_voice() {
    use core_model::mixer::StripKind;
    const L: u32 = 4800;
    let (d, seq) = doc(0);
    let mut b = Builder::from_doc(d);
    let lane = b.lane(seq, "Audio 2", None).unwrap();
    b.region(
        seq,
        lane,
        Q,
        MediaId(0),
        100,
        6 * QF,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let second = *b.doc().outputs().keys().next_back().unwrap();
    let strip = b.strip_of(StripKind::AudioPath(second)).unwrap();
    b.insert(strip, TestInserts::plugin(&format!("latency={L}")))
        .unwrap();
    let d = b.finish();
    let mut out = Vec::new();
    for leads in [true, false] {
        let mut r = rig_with(&d, seq, leads, 0.15);
        assert_eq!(r.engine.latency_frames(), u64::from(L));
        r.engine.play().unwrap();
        let got = r.render(460);
        let f = r.engine.faults();
        assert!(
            !f.contains(Faults::STREAM_UNDERRUN) && !f.contains(Faults::LATE_START),
            "leads {leads}: {:?}",
            f.names()
        );
        out.push(got);
    }
    assert_eq!(out[0], out[1]);
    assert!(out[0].0.iter().any(|x| *x != 0.0));
}
