//! Counts allocations made on the render path: the counting allocator is
//! armed only around `OfflineBackend::block`, which only calls
//! `Renderer::process`.

use core_engine::backend::AudioBackend;
use core_engine::backend::{BlockInfo, InstrumentNode, StereoBuf};
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::error::EngineError;
use core_engine::fixtures::{Builder, demo_document};
use core_engine::graph::{EffectNode, InsertFactory, ProcessStatus, TestInserts, TestInstrument};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::{Faults, RETIRE_CAPACITY};
use core_engine::rt_check::{self, CountingAlloc};
use core_engine::schedule::BuiltinRoute;
use core_engine::transport::PlayState;
use core_model::automation::AutomationTarget;
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::SeqId;
use core_model::mixer::{ParamId, PluginSlot, StripKind};
use core_model::time::Tick;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

/// Counts the automated values a plugin processor is given.
struct Hooked<T: ?Sized>(Arc<AtomicU64>, Box<T>);

impl EffectNode for Hooked<dyn EffectNode> {
    fn latency(&self) -> u32 {
        self.1.latency()
    }
    fn process(
        &mut self,
        frames: u32,
        io: &mut StereoBuf,
        sidechain: Option<&StereoBuf>,
    ) -> ProcessStatus {
        self.1.process(frames, io, sidechain)
    }
    fn reset(&mut self) {
        self.1.reset();
    }
    fn automate(&mut self, frame: u32, param: ParamId, normalized: f64) -> bool {
        let _ = (frame, param, normalized);
        self.0.fetch_add(1, Ordering::Relaxed);
        true
    }
}

impl InstrumentNode for Hooked<dyn InstrumentNode> {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        self.1.midi(offset, msg);
    }
    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        self.1.render(frames, out);
    }
    fn reset(&mut self) {
        self.1.reset();
    }
    fn render_outputs(&mut self, frames: u32, outs: &mut [StereoBuf], info: &BlockInfo) {
        self.1.render_outputs(frames, outs, info);
    }
    fn automate(&mut self, frame: u32, param: ParamId, normalized: f64) -> bool {
        let _ = (frame, param, normalized);
        self.0.fetch_add(1, Ordering::Relaxed);
        true
    }
}

/// Test plugins whose `automate` hooks count into `inserts` and
/// `instruments`.
struct HookedPlugins {
    inner: TestInserts,
    inserts: Arc<AtomicU64>,
    instruments: Arc<AtomicU64>,
}

impl InsertFactory for HookedPlugins {
    fn make(
        &mut self,
        slot: &PluginSlot,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn EffectNode>, String> {
        let n = self.inner.make(slot, sample_rate, max_frames)?;
        Ok(Box::new(Hooked(Arc::clone(&self.inserts), n)))
    }
    fn make_instrument(
        &mut self,
        slot: &PluginSlot,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn InstrumentNode>, String> {
        let n = self.inner.make_instrument(slot, sample_rate, max_frames)?;
        Ok(Box::new(Hooked(Arc::clone(&self.instruments), n)))
    }
}

/// The demo with a lane "Auto" in `song`: a fader ramp every 64 ticks over
/// eight bars, a mute toggled every beat and a pan point every 8 ticks for
/// a bar (runs of 32 frames); plus a test insert on the synth strip and a
/// test plugin instrument, each with a parameter ramping over eight bars.
fn automation_document(doc: &Document, song: SeqId) -> Document {
    let mut b = Builder::from_doc(doc.clone());
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let insert = b.insert(synth, TestInserts::plugin("gain=1")).unwrap();
    let (_, instrument) = b
        .plugin_dest("Test synth", TestInstrument::plugin("synth"))
        .unwrap();
    let lane = b.lane(song, "Auto", None).unwrap();
    let q = 15_360i64;
    let mut events = Vec::new();
    for k in 0..(32 * q / 64) {
        events.push(EventSpec::new(
            Tick(k * 64),
            EventBody::Automation {
                target: AutomationTarget::StripFader(synth),
                value: (k % 100) as f64 / 100.0,
            },
        ));
    }
    for k in 0..32 {
        events.push(EventSpec::new(
            Tick(k * q),
            EventBody::Automation {
                target: AutomationTarget::StripMute(synth),
                value: (k % 2) as f64,
            },
        ));
    }
    for k in 0..(4 * q / 8) {
        events.push(EventSpec::new(
            Tick(k * 8),
            EventBody::Automation {
                target: AutomationTarget::StripPan(synth),
                value: (k % 3) as f64 / 2.0,
            },
        ));
    }
    for (k, id) in [(0u64, insert), (1, instrument)] {
        for (t, v) in [(0, 0.0), (32 * q, 1.0)] {
            events.push(EventSpec::new(
                Tick(t),
                EventBody::Automation {
                    target: AutomationTarget::PluginParam(id, ParamId(k)),
                    value: v,
                },
            ));
        }
    }
    b.events(song, lane, events).unwrap();
    b.finish()
}

/// One test, so no other test thread allocates while the global counter
/// is compared.
#[test]
fn render_path_never_allocates() {
    // The counter works: one allocation and one free.
    let before = rt_check::count();
    rt_check::arm();
    let v: Vec<u8> = Vec::with_capacity(10);
    drop(v);
    rt_check::disarm();
    assert_eq!(rt_check::count() - before, 2);

    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let before = rt_check::count();

    // 10k blocks of play, stop, locate and loop wraps on the built-in
    // synth plus a MIDI output (route both).
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.route = BuiltinRoute::Both;
    let sinks = OfflineSinks::new(1 << 20);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(cfg, Box::new(sinks)).unwrap();
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop(true).unwrap();
    engine.play().unwrap();
    // What the phases exercised (read outside the armed blocks).
    let (mut wraps, mut stops, mut locates, mut tails, mut ended) = (0, 0, 0, 0, 0);
    for i in 0..10_000u32 {
        let mut moved = false;
        match i % 1000 {
            300 => {
                engine.stop().unwrap();
                stops += 1;
            }
            310 => engine.play().unwrap(),
            500 => {
                engine.locate_tick(Tick(15_360 * 40)).unwrap();
                locates += 1;
                moved = true;
            }
            700 => engine.set_loop(false).unwrap(),
            900 => engine.set_loop(true).unwrap(),
            _ => {}
        }
        let at = engine.position_frames();
        armed_block(&mut b, 1 + (i * 37) % 1024);
        if !moved && engine.position_frames() < at {
            wraps += 1;
        }
        if i % 100 == 0 {
            log.drain();
        }
    }
    assert_eq!(rt_check::count() - before, 0, "allocations while playing");

    // Loop off: play past the end into the tail until the end is reported.
    engine.set_loop(false).unwrap();
    engine
        .locate_frame(engine.length_frames() - 10_000)
        .unwrap();
    locates += 1;
    engine.play().unwrap();
    for i in 0..1_000u32 {
        armed_block(&mut b, 1 + (i * 53) % 1024);
        if engine.state() == PlayState::Tail {
            tails += 1;
        }
        if engine.take_ended() {
            ended += 1;
            break;
        }
    }
    log.drain();
    assert_eq!(engine.state(), PlayState::Stopped);
    assert_eq!(rt_check::count() - before, 0, "allocations into the tail");
    assert!(wraps >= 1, "no loop wrap was rendered");
    assert!(
        stops >= 1 && locates >= 1,
        "stops {stops}, locates {locates}"
    );
    assert!(tails >= 1, "the tail was never rendered");
    assert_eq!(ended, 1, "the end was never reported");

    // 100 schedule swaps; with the collector paused the retire ring
    // (`RETIRE_CAPACITY`, two entries per load) fills and a swap is
    // deferred.
    engine.pause_collector(true);
    for i in 0..100 {
        if i == RETIRE_CAPACITY / 2 + 4 {
            engine.pause_collector(false);
        }
        // Once the collector resumes it frees the retire ring on its own
        // schedule; until then deferred commands can fill the command
        // ring and `load` reports `Busy`. Keep rendering until it fits
        // rather than guessing how long the collector takes to run.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match engine.load(&doc, song) {
                Ok(_) => break,
                Err(EngineError::Busy) if std::time::Instant::now() < deadline => {
                    armed_block(&mut b, 256);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(e) => panic!("load failed: {e:?}"),
            }
        }
        armed_block(&mut b, 256);
    }
    assert!(engine.counters().swaps_deferred >= 1);
    assert_eq!(rt_check::count() - before, 0, "allocations during swaps");

    // Blocks larger than the prepared size.
    armed_block(&mut b, 8192);
    assert!(engine.faults().contains(Faults::XRUN));
    assert_eq!(rt_check::count() - before, 0, "allocations on xrun");
    drop(b.stop().unwrap());
    drop(engine);

    // A full MIDI queue drops instead of allocating.
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.route = BuiltinRoute::Virtual;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(8))).unwrap();
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    engine.play().unwrap();
    for _ in 0..2_000 {
        armed_block(&mut b, 512);
    }
    assert!(engine.counters().midi_dropped > 0);
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations with a full MIDI queue"
    );
    drop(b.stop().unwrap());
    drop(engine);

    // Automation: runs split at points, ramps, mutes, plugin parameters, a
    // stop, a locate and a recompile while playing (Plan 4d Rulings 10–13).
    let auto = automation_document(&doc, song);
    let (mut engine, renderer) = Engine::new(
        EngineConfig::new(48_000.0),
        Box::new(OfflineSinks::new(1 << 20).with_tone()),
    )
    .unwrap();
    let inserts = Arc::new(AtomicU64::new(0));
    let instruments = Arc::new(AtomicU64::new(0));
    engine.set_insert_factory(Box::new(HookedPlugins {
        inner: TestInserts::new(),
        inserts: Arc::clone(&inserts),
        instruments: Arc::clone(&instruments),
    }));
    let report = engine.load(&auto, song).unwrap();
    assert_eq!(report.automation_instrument_lanes, 1, "{report:?}");
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop(true).unwrap();
    engine.play().unwrap();
    for i in 0..3_000u32 {
        match i {
            1_000 => engine.stop().unwrap(),
            1_010 => engine.play().unwrap(),
            1_500 => {
                engine.load(&auto, song).unwrap();
            }
            2_000 => engine.locate_tick(Tick(15_360 * 8)).unwrap(),
            _ => {}
        }
        armed_block(&mut b, 1 + (i * 37) % 1024);
    }
    assert!(engine.counters().runs > 3_000, "automation split blocks");
    assert!(
        inserts.load(Ordering::Relaxed) > 100 && instruments.load(Ordering::Relaxed) > 100,
        "plugin automate hooks: inserts {}, instruments {}",
        inserts.load(Ordering::Relaxed),
        instruments.load(Ordering::Relaxed)
    );
    assert_eq!(rt_check::count() - before, 0, "allocations with automation");
    drop(b.stop().unwrap());
    drop(engine);

    // Regions (5a): streamed from disk with a loop and locates; the disk
    // is serviced outside the armed blocks. The output must carry the
    // regions' audio, so this phase cannot pass on silence.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.caf");
    {
        let mut w = core_media::caf::CafWriter::create(
            &path,
            48_000,
            2,
            core_model::format::BitDepth::Float32,
            core_media::caf::WriteMode::Normal,
        )
        .unwrap();
        w.write(&vec![0.25f32; 2 * 480_000]).unwrap();
        w.finish().unwrap();
    }
    let mut bld = Builder::new();
    let (seq, lane) = bld.sequence("Song", "Audio", None).unwrap();
    let m = bld.media("Media/a.caf", 480_000, 2).unwrap();
    for beat in [0i64, 3, 9] {
        bld.region(
            seq,
            lane,
            beat * 15_360,
            m,
            beat as u64 * 1000,
            100_000,
            -3.0,
            core_model::event::Fade {
                length_frames: 500,
                shape: core_model::event::FadeShape::SCurve,
            },
            core_model::event::Fade::default(),
        )
        .unwrap();
    }
    // A latent bus (plan 5d-1 Task 2): the audio path's regions read 4800
    // frames ahead of the renderer, across the loop seam and the locates.
    let bus = bld.bus("Latent").unwrap();
    bld.insert(bus, TestInserts::plugin("latency=4800"))
        .unwrap();
    let audio_doc = bld.finish();
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = core_media::stream::DiskMode::Manual;
    cfg.stream = core_media::stream::StreamConfig {
        cache_file_bytes: 0,
        ..Default::default()
    };
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_media(vec![core_media::stream::MediaFile {
        id: m,
        path: path.clone(),
    }]);
    let report = engine.load(&audio_doc, seq).unwrap();
    assert_eq!(engine.latency_frames(), 4800);
    assert!(
        report.audio_segments >= 3 && report.audio_offline == 0,
        "{report:?}"
    );
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop_range(Some((10_000, 200_000))).unwrap();
    engine.play().unwrap();
    let (mut sounding, mut peak) = (0u32, 0f32);
    for i in 0..2_000u32 {
        if i % 500 == 250 {
            engine.locate_frame(u64::from(i) * 97).unwrap();
        }
        engine.streamer().service();
        let frames = 1 + (i * 37) % 1024;
        armed_block(&mut b, frames);
        let (l, r) = b.output(frames);
        let p = l.iter().chain(r).fold(0f32, |m, x| m.max(x.abs()));
        if p > 0.0 {
            sounding += 1;
        }
        peak = peak.max(p);
    }
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations while playing regions"
    );
    assert_eq!(engine.state(), PlayState::Playing);
    assert!(
        sounding > 1_000 && peak > 0.1,
        "regions were not heard: {sounding} sounding block(s), peak {peak}"
    );
    assert!(
        engine.streamer().counters().stream_voices > 0,
        "nothing streamed"
    );
    let f = engine.faults();
    assert!(!f.contains(Faults::STREAM_UNDERRUN), "{:?}", f.names());
}
