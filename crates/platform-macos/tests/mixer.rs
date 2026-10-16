#![cfg(target_os = "macos")]

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, demo_document};
use core_engine::offline::OfflineBackend;
use core_model::command::MixerCmd;
use core_model::ids::StripId;
use core_model::mixer::StripKind;
use platform_macos::rt::MacRtPlatform;
use platform_macos::sinks::MacSinks;
use std::sync::Arc;

fn rms_dbfs(x: &[f32]) -> f64 {
    let sum: f64 = x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
    20.0 * (sum / x.len().max(1) as f64).sqrt().max(1e-12).log10()
}

#[test]
fn the_demo_plays_through_the_builtin_strip_into_master() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.workers = 2;
    let (mut engine, r) =
        Engine::with_platform(cfg, Box::new(MacSinks::new()), Arc::new(MacRtPlatform)).unwrap();
    let report = engine.load(&doc, song).unwrap();
    assert!(report.graph.error.is_none() && report.graph.warnings.is_empty());
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(r).unwrap();
    engine.play().unwrap();
    let mut out = Vec::new();
    for _ in 0..(2 * 48_000 / 512) {
        b.block(512);
        out.extend_from_slice(b.output(512).0);
    }
    let level = rms_dbfs(&out);
    assert!(
        level > -40.0,
        "the demo is too quiet through the mixer: {level:.1} dBFS"
    );
    let meters = engine.meters();
    let synth = meters.iter().find(|m| m.strip == StripId(1)).unwrap();
    assert!(synth.peak_l > 0.0, "{meters:?}");

    // Muting the built-in synth's strip silences master from the next block.
    let mut bld = Builder::from_doc(doc);
    let strip = bld.strip_of(StripKind::BuiltinSynth).unwrap();
    bld.mixer(MixerCmd::SetMute { strip, mute: true }).unwrap();
    engine.load(bld.doc(), song).unwrap();
    b.block(512);
    assert!(b.output(512).0.iter().all(|x| *x == 0.0));
    assert!(!engine.faults().contains(core_engine::render::Faults::XRUN));
}
