//! Plugin instrument destinations as engine targets: their sink comes from
//! the plugin factory, plays through their `Instrument` strip and is
//! released when the destination goes, restarts or is too small for the
//! renderer.

use core_engine::backend::{AudioBackend, InstrumentNode, StereoBuf};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::{EffectNode, InsertFactory, TestInserts, TestInstrument};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::schedule::{TargetIdx, TargetKind};
use core_model::command::{Command, RoutingCmd};
use core_model::document::Document;
use core_model::ids::{DestId, PluginInstanceId, SeqId};
use core_model::mixer::{PluginSlot, StripKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const SR: f64 = 48_000.0;

/// A test synth holding one long note on channel 0.
fn synth_doc() -> (Builder, SeqId, DestId, PluginInstanceId) {
    let mut b = Builder::new();
    let (d, id) = b
        .plugin_dest("Synth", TestInstrument::plugin("synth"))
        .unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0xFFFF, 16 * Q)]).unwrap();
    (b, seq, d, id)
}

fn engine(max_frames: u32) -> (Engine, OfflineBackend, TestInserts) {
    let mut cfg = EngineConfig::new(SR);
    cfg.max_frames = max_frames;
    let (mut engine, r) = Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16))).unwrap();
    let f = TestInserts::new();
    engine.set_insert_factory(Box::new(f.clone()));
    let mut b = OfflineBackend::new(SR, 256);
    b.start(r).unwrap();
    (engine, b, f)
}

fn plugin_targets(e: &Engine) -> Vec<(TargetIdx, PluginInstanceId)> {
    e.targets()
        .iter()
        .filter_map(|(i, k)| match k {
            TargetKind::PluginInstrument(id) => Some((i, *id)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_plugin_instrument_plays_through_its_strip() {
    let (b, seq, _, id) = synth_doc();
    let (mut engine, mut be, made) = engine(4096);
    let report = engine.load(b.doc(), seq).unwrap();
    assert!(report.unroutable.is_empty(), "{:?}", report.warnings);
    assert!(
        report.graph.warnings.is_empty(),
        "{:?}",
        report.graph.warnings
    );
    assert_eq!(report.graph.nodes, 3, "instrument, master, its strip");
    assert_eq!(plugin_targets(&engine), vec![(TargetIdx(0), id)]);
    assert_eq!(made.made(), 1);
    engine.play().unwrap();
    be.run_frames(1024);
    // 0.1 through the strip's centred pan (−3 dB), master unpanned.
    let (l, r) = be.output(256);
    let want = 0.1 * std::f32::consts::FRAC_1_SQRT_2;
    assert!((l[100] - want).abs() < 1e-6 && (r[100] - want).abs() < 1e-6);
}

#[test]
fn without_a_plugin_factory_the_destination_is_unroutable() {
    let (b, seq, _, _) = synth_doc();
    let (mut engine, _r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    let report = engine.load(b.doc(), seq).unwrap();
    assert_eq!(report.unroutable.len(), 1);
    assert!(
        report.warnings[0].contains("cannot load until plugin hosting exists"),
        "{:?}",
        report.warnings
    );
    assert_eq!(
        report.graph.warnings,
        vec!["strip \"Synth\" has no instrument".to_string()]
    );
    assert!(engine.targets().is_empty());
}

fn replace(doc: &Document, dest: DestId, with: DestId) -> Document {
    core_model::command::apply(
        doc,
        &Command::Routing(RoutingCmd::RemoveDestination {
            dest,
            replacement: Some(with),
        }),
    )
    .unwrap()
    .doc
}

#[test]
fn released_targets_are_reused_only_from_the_next_load() {
    let (mut b, seq, d, first) = synth_doc();
    let (mut engine, mut be, _) = engine(4096);
    engine.load(b.doc(), seq).unwrap();
    be.run_frames(256);
    // Replace the synth with another one in a single edit.
    let (d2, second) = b
        .plugin_dest("Synth 2", TestInstrument::plugin("synth"))
        .unwrap();
    let mut b = Builder::from_doc(replace(b.doc(), d, d2));
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(
        plugin_targets(&engine),
        vec![(TargetIdx(1), second)],
        "index 0 is released, not reused in the same load"
    );
    assert_ne!(first, second);
    be.run_frames(256);
    let (d3, third) = b
        .plugin_dest("Synth 3", TestInstrument::plugin("synth"))
        .unwrap();
    let lane = b.lane(seq, "L3", Some(d3)).unwrap();
    b.notes(seq, lane, &[(0, 64, 0xFFFF, Q)]).unwrap();
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(
        plugin_targets(&engine),
        vec![(TargetIdx(0), third), (TargetIdx(1), second)]
    );
}

#[test]
fn restart_instance_recreates_inserts_and_instruments() {
    let (mut b, seq, _, id) = synth_doc();
    let strip = b.strip_of(StripKind::Instrument(DestId(0))).unwrap();
    let gain = b
        .insert(strip, TestInserts::plugin("gain=0.5,latency=16"))
        .unwrap();
    let (mut engine, mut be, made) = engine(4096);
    engine.load(b.doc(), seq).unwrap();
    be.run_frames(256);
    assert_eq!(made.made(), 2);
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(made.made(), 2, "an ordinary load keeps both");
    engine.restart_instance(gain);
    engine.restart_instance(id);
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(made.made(), 4, "both recreated");
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(made.made(), 4, "a restart applies to one load");
}

#[test]
fn a_larger_prepare_recreates_instrument_sinks_and_inserts() {
    let (mut b, seq, _, _) = synth_doc();
    let strip = b.strip_of(StripKind::Instrument(DestId(0))).unwrap();
    b.insert(strip, TestInserts::plugin("gain=0.5")).unwrap();
    let (mut engine, mut be, made) = engine(512);
    engine.load(b.doc(), seq).unwrap();
    engine.play().unwrap();
    be.run_blocks([256]);
    assert_eq!(made.made(), 2);
    let mut r = be.stop().unwrap();
    r.prepare(2048);
    be.start(r).unwrap();
    engine.load(b.doc(), seq).unwrap();
    assert_eq!(made.made(), 4, "made again for 2048-frame blocks");
    // The new synth missed the held note: play it again from the start.
    engine.locate_frame(0).unwrap();
    be.run_blocks([2048, 2048]);
    let (l, _) = be.output(2048);
    assert!(l.iter().all(|x| *x > 0.0), "plays at the new size");
    assert!(engine.faults().is_empty(), "{:?}", engine.faults().names());
}

/// A test instrument that counts its drops.
struct Counted(Box<dyn InstrumentNode>, Arc<AtomicUsize>);

impl InstrumentNode for Counted {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        self.0.midi(offset, msg);
    }
    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        self.0.render(frames, out);
    }
    fn reset(&mut self) {
        self.0.reset();
    }
    fn latency(&self) -> u32 {
        self.0.latency()
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

struct CountingInserts(TestInserts, Arc<AtomicUsize>);

impl InsertFactory for CountingInserts {
    fn make(
        &mut self,
        slot: &PluginSlot,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn EffectNode>, String> {
        self.0.make(slot, sample_rate, max_frames)
    }
    fn make_instrument(
        &mut self,
        slot: &PluginSlot,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn InstrumentNode>, String> {
        let n = self.0.make_instrument(slot, sample_rate, max_frames)?;
        Ok(Box::new(Counted(n, Arc::clone(&self.1))))
    }
}

#[test]
fn a_removed_sink_is_dropped_by_the_collector() {
    let (mut b, seq, d, _) = synth_doc();
    let (mut engine, mut be, _) = engine(4096);
    let drops = Arc::new(AtomicUsize::new(0));
    engine.set_insert_factory(Box::new(CountingInserts(
        TestInserts::new(),
        Arc::clone(&drops),
    )));
    engine.load(b.doc(), seq).unwrap();
    be.run_frames(256);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let (d2, _) = b
        .plugin_dest("Synth 2", TestInstrument::plugin("synth"))
        .unwrap();
    let b = Builder::from_doc(replace(b.doc(), d, d2));
    engine.load(b.doc(), seq).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while drops.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        be.run_frames(256);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(drops.load(Ordering::SeqCst), 1, "removed sink dropped once");
}

#[test]
fn a_released_index_is_not_reused_before_a_publish() {
    let (mut b, seq, d, _) = synth_doc();
    let (mut engine, mut be, _) = engine(4096);
    engine.load(b.doc(), seq).unwrap();
    be.run_frames(256);
    let (d2, second) = b
        .plugin_dest("Synth 2", TestInstrument::plugin("synth"))
        .unwrap();
    let mut ok = Builder::from_doc(replace(b.doc(), d, d2));
    let mut bad = Builder::from_doc(ok.doc().clone());
    let strip = bad.strip_of(StripKind::Instrument(d2)).unwrap();
    bad.insert(strip, TestInserts::plugin("gain=0.5,latency=200000"))
        .unwrap();
    // Index 0 is released, but the graph cannot be planned: the old graph
    // keeps playing and still names it.
    let report = engine.load(bad.doc(), seq).unwrap();
    assert!(report.graph.error.is_some(), "{:?}", report.graph);
    be.run_frames(256);
    let (d3, third) = bad
        .plugin_dest("Synth 3", TestInstrument::plugin("synth"))
        .unwrap();
    let lane = bad.lane(seq, "L3", Some(d3)).unwrap();
    bad.notes(seq, lane, &[(0, 64, 0xFFFF, Q)]).unwrap();
    let report = engine.load(bad.doc(), seq).unwrap();
    assert!(report.graph.error.is_some());
    assert_eq!(
        plugin_targets(&engine),
        vec![(TargetIdx(1), second), (TargetIdx(2), third)],
        "index 0 not reused while nothing was published"
    );
    be.run_frames(256);
    // A good load publishes; the next one may reuse index 0.
    let report = engine.load(ok.doc(), seq).unwrap();
    assert!(report.graph.error.is_none(), "{:?}", report.graph);
    be.run_frames(256);
    let (d4, fourth) = ok
        .plugin_dest("Synth 4", TestInstrument::plugin("synth"))
        .unwrap();
    let lane = ok.lane(seq, "L4", Some(d4)).unwrap();
    ok.notes(seq, lane, &[(0, 64, 0xFFFF, Q)]).unwrap();
    engine.load(ok.doc(), seq).unwrap();
    assert_eq!(
        plugin_targets(&engine),
        vec![(TargetIdx(0), fourth), (TargetIdx(1), second)]
    );
}
