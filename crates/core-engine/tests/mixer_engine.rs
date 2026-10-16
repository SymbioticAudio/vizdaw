use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, Q, demo_document};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_engine::schedule::BuiltinRoute;
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::{AuxSend, PluginRef, StripKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const SR: f64 = 48_000.0;

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn song() -> (Document, SeqId) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    (doc, song)
}

/// Renders `total` frames in blocks cycling through `sizes`; returns the
/// left channel.
fn render(b: &mut OfflineBackend, total: usize, sizes: &[u32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(total);
    let mut k = 0;
    while out.len() < total {
        let n = (sizes[k % sizes.len()] as usize).min(total - out.len()) as u32;
        k += 1;
        b.reserve(n);
        b.block(n);
        out.extend_from_slice(b.output(n).0);
    }
    out
}

#[test]
fn the_demo_log_is_unchanged_with_and_without_workers() {
    for workers in [0, 4] {
        let (doc, song) = song();
        let mut cfg = EngineConfig::new(SR);
        cfg.workers = workers;
        let sinks = OfflineSinks::new(100_000);
        let log = sinks.log();
        let (mut engine, r) = Engine::new(cfg, Box::new(sinks)).unwrap();
        let report = engine.load(&doc, song).unwrap();
        assert_eq!(
            report.graph.nodes, 3,
            "built-in synth instrument, master, built-in strip"
        );
        assert_eq!(report.graph.latency_frames, 0);
        assert!(report.graph.error.is_none() && report.graph.warnings.is_empty());
        let mut b = OfflineBackend::new(SR, 512);
        b.start(r).unwrap();
        engine.play().unwrap();
        for _ in 0..100_000 {
            b.run_frames(512);
            if engine.take_ended() {
                break;
            }
        }
        let text = log.drain_text();
        assert_eq!(text.lines().count(), 335, "{workers} worker(s)");
        assert_eq!(
            format!("{:016x}", fnv(&text)),
            "1feb33c1bfd60916",
            "{workers} worker(s)"
        );
        assert!(engine.faults().is_empty());
    }
}

/// The demo plus a reverb-ish bus: synth → (gain, latency 37) → master and
/// a post send to "Verb" (noise, latency 100; then a sidechain adder fed by
/// the synth).
fn mixed() -> (Document, SeqId) {
    let (doc, song) = song();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let verb = b.bus("Verb").unwrap();
    b.insert(synth, TestInserts::plugin("gain=0.8,latency=37"))
        .unwrap();
    b.insert(verb, TestInserts::plugin("noise=5,latency=100"))
        .unwrap();
    let sc = b.insert(verb, TestInserts::plugin("sc")).unwrap();
    b.mixer(MixerCmd::SetSidechain {
        slot: sc,
        source: Some(synth),
    })
    .unwrap();
    let mut send = AuxSend::new(verb);
    send.level_db = -6.0;
    b.mixer(MixerCmd::AddSend { strip: synth, send }).unwrap();
    (b.finish(), song)
}

fn mixed_audio(workers: usize, sizes: &[u32]) -> Vec<f32> {
    let (doc, song) = mixed();
    let mut cfg = EngineConfig::new(SR);
    cfg.workers = workers;
    let (mut engine, r) =
        Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    let report = engine.load(&doc, song).unwrap();
    assert_eq!(report.graph.latency_frames, 137);
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    engine.play().unwrap();
    let out = render(&mut b, 3 * 48_000, sizes);
    assert!(engine.faults().is_empty(), "{:?}", engine.faults().names());
    out
}

#[test]
fn audio_is_identical_across_workers_and_block_sizes() {
    let reference = mixed_audio(0, &[512]);
    assert!(reference.iter().any(|x| *x != 0.0));
    let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    for (w, sizes) in [
        (1, &[64u32][..]),
        (3, &[333, 512][..]),
        (8, &[1024, 7, 4096][..]),
    ] {
        assert_eq!(
            bits(&mixed_audio(w, sizes)),
            bits(&reference),
            "{w} worker(s), {sizes:?}"
        );
    }
}

/// One note on the built-in synth through an insert of latency `latency`.
fn one_note(latency: u32) -> (Document, SeqId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Bar", "Notes", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q / 2)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.insert(synth, TestInserts::plugin(&format!("latency={latency}")))
        .unwrap();
    (b.finish(), seq)
}

#[test]
fn graph_latency_delays_midi_out_and_the_position() {
    let (doc, seq) = one_note(480);
    let mut cfg = EngineConfig::new(SR);
    cfg.route = BuiltinRoute::Both;
    let sinks = OfflineSinks::new(1000);
    let log = sinks.log();
    let (mut engine, r) = Engine::new(cfg, Box::new(sinks)).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    let report = engine.load(&doc, seq).unwrap();
    assert_eq!(report.graph.latency_frames, 480);
    assert_eq!(engine.latency_frames(), 480);
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    engine.play().unwrap();
    b.run_frames(24_000);
    let text = log.drain_text();
    for line in [
        "0 t0 90 3c 40",
        "480 t1 90 3c 40",
        "12000 t0 80 3c 00",
        "12480 t1 80 3c 00",
    ] {
        assert!(
            text.lines().any(|l| l == line),
            "missing {line:?} in\n{text}"
        );
    }
    assert_eq!(engine.position_frames(), 24_000 - 480);
}

#[test]
fn limits_keep_the_previous_graph() {
    let build = |latency: u32| {
        let mut b = Builder::new();
        let d = b.dest("Synth", "builtin:synth", 0).unwrap();
        let (seq, _) = b.sequence("S", "L", Some(d)).unwrap();
        let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
        let long = b.bus("Long").unwrap();
        b.mixer(MixerCmd::AddSend {
            strip: synth,
            send: AuxSend::new(long),
        })
        .unwrap();
        b.insert(long, TestInserts::plugin(&format!("latency={latency}")))
            .unwrap();
        (b.finish(), seq)
    };
    let (mut engine, _r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    let (ok, seq) = build(100);
    assert_eq!(engine.load(&ok, seq).unwrap().graph.latency_frames, 100);
    let (bad, seq) = build(96_001);
    let report = engine.load(&bad, seq).unwrap();
    assert!(
        report
            .graph
            .error
            .as_deref()
            .unwrap()
            .contains("exceeds the 2 s limit")
    );
    assert!(engine.faults().contains(Faults::LATENCY_TOO_LARGE));
    assert_eq!(
        engine.latency_frames(),
        100,
        "the previous graph keeps playing"
    );
}

#[test]
fn meters_and_load_are_published() {
    let (doc, song) = song();
    let (mut engine, r) = Engine::new(
        EngineConfig::new(SR),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    engine.play().unwrap();
    b.run_frames(48_000);
    let meters = engine.meters();
    let strips: Vec<StripId> = meters.iter().map(|m| m.strip).collect();
    assert_eq!(strips, vec![StripId(0), StripId(1)]);
    assert!(
        meters.iter().all(|m| m.peak_l > 0.0 && m.peak_r > 0.0),
        "{meters:?}"
    );
    assert!(
        engine.meters().iter().all(|m| m.peak_l == 0.0),
        "read and cleared"
    );
    let load = engine.dsp_load();
    assert!(load.last.is_finite() && load.last >= 0.0 && load.peak >= 0.0);
}

#[test]
fn plugin_fault_is_reported_by_instance() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, _) = b.sequence("S", "L", Some(d)).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let nan = b.insert(synth, TestInserts::plugin("nan")).unwrap();
    let doc = b.finish();
    let (mut engine, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(SR, 256);
    b.start(r).unwrap();
    b.run_frames(20 * 256);
    assert_eq!(engine.plugin_faults(), vec![nan]);
    assert!(engine.faults().contains(Faults::PLUGIN_FAULT));
    assert_eq!(engine.counters().plugin_faults, 1);
}

/// Noise on the synth strip, sent to "Wet" (×0.5, latency 64), so master's
/// input from the synth carries a 64-frame compensation delay.
fn wet(master_db: f64) -> (Document, SeqId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, _) = b.sequence("S", "L", Some(d)).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.insert(synth, TestInserts::plugin("noise=9")).unwrap();
    let wet = b.bus("Wet").unwrap();
    b.insert(wet, TestInserts::plugin("gain=0.5,latency=64"))
        .unwrap();
    b.mixer(MixerCmd::AddSend {
        strip: synth,
        send: AuxSend::new(wet),
    })
    .unwrap();
    b.mixer(MixerCmd::SetGain {
        strip: StripId(0),
        gain_db: master_db,
    })
    .unwrap();
    (b.finish(), seq)
}

#[test]
fn parameter_edits_reuse_instances_and_state() {
    let factory = TestInserts::new();
    let (doc1, seq) = wet(0.0);
    let (doc2, _) = wet(-6.0);
    let (mut engine, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    engine.set_insert_factory(Box::new(factory.clone()));
    engine.load(&doc1, seq).unwrap();
    assert_eq!(factory.made(), 2);
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    let _ = render(&mut b, 5120, &[512]);
    engine.load(&doc2, seq).unwrap();
    assert_eq!(factory.made(), 2, "a fader edit creates no instance");
    let edited = render(&mut b, 5120, &[512]);

    let (mut reference, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    reference.set_insert_factory(Box::new(TestInserts::new()));
    reference.load(&doc2, seq).unwrap();
    let mut rb = OfflineBackend::new(SR, 512);
    rb.start(r).unwrap();
    let whole = render(&mut rb, 10_240, &[512]);
    assert_eq!(
        edited,
        whole[5120..].to_vec(),
        "no glitch: delay memory and noise state kept"
    );

    // A structural edit adopts the instances too; a new insert is made.
    let mut b2 = Builder::from_doc(doc2);
    b2.bus("Extra").unwrap();
    engine.load(b2.doc(), seq).unwrap();
    assert_eq!(factory.made(), 2);
    b2.insert(StripId(0), TestInserts::plugin("gain=1"))
        .unwrap();
    engine.load(b2.doc(), seq).unwrap();
    assert_eq!(factory.made(), 3);
}

#[test]
fn placeholders_pass_audio_and_are_reported() {
    let (doc, song) = song();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.insert(synth, PluginRef::from_legacy("clap:missing"))
        .unwrap();
    let (mut engine, r) = Engine::new(
        EngineConfig::new(SR),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    let report = engine.load(b.doc(), song).unwrap();
    assert_eq!(report.graph.placeholders.len(), 1);
    assert_eq!(report.graph.placeholders[0].0, PluginInstanceId(0));
    assert!(report.graph.placeholders[0].1.contains("clap:missing"));
    assert_eq!(report.graph.latency_frames, 0);
    let mut ob = OfflineBackend::new(SR, 512);
    ob.start(r).unwrap();
    engine.play().unwrap();
    let out = render(&mut ob, 48_000, &[512]);
    assert!(
        out.iter().any(|x| *x != 0.0),
        "the placeholder passes the synth"
    );
}

#[test]
fn prepare_resizes_graph_buffers() {
    let (doc, song) = song();
    let mut cfg = EngineConfig::new(SR);
    cfg.max_frames = 512;
    let (mut engine, r) = Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16))).unwrap();
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    b.run_blocks([512, 2048]);
    assert!(
        engine.faults().contains(Faults::XRUN),
        "2048 > 512 before prepare"
    );
    let mut r = b.stop().unwrap();
    r.prepare(2048);
    b.start(r).unwrap();
    b.run_blocks([2048, 2048]);
    assert!(engine.faults().is_empty());
}

#[test]
fn a_stalled_node_misses_the_deadline_without_blocking() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut bld = Builder::new();
    let d = bld.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, _) = bld.sequence("S", "L", Some(d)).unwrap();
    let synth = bld.strip_of(StripKind::BuiltinSynth).unwrap();
    bld.insert(synth, TestInserts::plugin("stall")).unwrap();
    let doc = bld.finish();
    let mut cfg = EngineConfig::new(SR);
    cfg.workers = 1;
    cfg.deadline = Some(0.5);
    let (mut engine, r) = Engine::new(cfg, Box::new(OfflineSinks::new(10))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::with_stall(Arc::clone(&flag))));
    engine.set_callback_helps(false);
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    b.run_blocks([512, 512]);
    let c = engine.counters();
    assert_eq!((c.deadline_misses, c.busy_blocks), (1, 1));
    assert!(engine.faults().contains(Faults::XRUN));
    let load = engine.dsp_load();
    assert!(
        load.last >= 1.0 && load.peak >= 1.0,
        "a lost block is full load: {load:?}"
    );
    flag.store(true, Ordering::Release);
    // Run blocks until one neither misses nor finds the node still busy.
    let mut recovered = false;
    for _ in 0..2000 {
        let c = engine.counters();
        b.run_blocks([512]);
        let d = engine.counters();
        if d.deadline_misses == c.deadline_misses && d.busy_blocks == c.busy_blocks {
            recovered = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(recovered, "recovered");
    assert!(engine.counters().busy_blocks >= 1);
}

#[test]
fn a_load_after_a_larger_prepare_plays_at_the_new_size() {
    let (doc, song) = song();
    let mut cfg = EngineConfig::new(SR);
    cfg.max_frames = 512;
    let (mut engine, r) =
        Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    engine.play().unwrap();
    b.run_blocks([512]);
    let mut r = b.stop().unwrap();
    r.prepare(2048);
    b.start(r).unwrap();
    b.run_blocks([2048]);
    assert!(engine.faults().is_empty());
    // An edit after the device grew its buffer: the graph is built for the
    // prepared size, not the configured one.
    engine.load(&doc, song).unwrap();
    let out = render(&mut b, 48 * 2048, &[2048]);
    let faults = engine.faults();
    assert!(faults.is_empty(), "{:?}", faults.names());
    assert_eq!(engine.counters().xruns, 0);
    assert!(out.iter().any(|x| *x != 0.0));
}

#[test]
fn a_graph_queued_before_a_larger_prepare_is_not_run_past_its_buffers() {
    let (doc, song) = song();
    let mut cfg = EngineConfig::new(SR);
    cfg.max_frames = 512;
    let (mut engine, r) =
        Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
    engine.load(&doc, song).unwrap();
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    engine.play().unwrap();
    b.run_blocks([512]);
    // Still queued (built for 512 frames) when the renderer is prepared for
    // 2048: the guard renders the block as an xrun (the backstop).
    engine.load(&doc, song).unwrap();
    let mut r = b.stop().unwrap();
    r.prepare(2048);
    b.start(r).unwrap();
    b.run_blocks([2048]);
    assert!(engine.faults().contains(Faults::XRUN));
    assert!(b.output(2048).0.iter().all(|x| *x == 0.0));
    assert_eq!(engine.counters().xruns, 1);
    // The next load builds at the prepared size and heals it.
    engine.load(&doc, song).unwrap();
    let out = render(&mut b, 48 * 2048, &[2048]);
    let faults = engine.faults();
    assert!(faults.is_empty(), "{:?}", faults.names());
    assert_eq!(engine.counters().xruns, 1);
    assert!(out.iter().any(|x| *x != 0.0));
}
