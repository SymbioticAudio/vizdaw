//! `--offline`: render without audio hardware and write the event log.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::schedule::{BuiltinRoute, CompileReport};
use core_model::document::Document;
use core_model::ids::SeqId;

/// Offline sample rate.
pub const RATE: f64 = 48_000.0;
/// Offline block size.
pub const BLOCK: u32 = 512;

/// Plays `seq` to its end and returns the compile report and the log text.
/// When `looped`, the loop is switched off during the third pass, so three
/// passes play followed by the tail.
pub fn render(
    doc: &Document,
    seq: SeqId,
    route: BuiltinRoute,
    looped: bool,
) -> Result<(CompileReport, String), String> {
    let sinks = OfflineSinks::new(1 << 16);
    let log = sinks.log();
    let mut cfg = EngineConfig::new(RATE);
    cfg.route = route;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(sinks));
    let report = engine.load(doc, seq).map_err(|e| e.to_string())?;
    let mut backend = OfflineBackend::new(RATE, BLOCK);
    backend.start(renderer).map_err(|e| e.to_string())?;
    engine.set_loop(looped).map_err(|e| e.to_string())?;
    engine.play().map_err(|e| e.to_string())?;
    let mut text = String::new();
    let length = engine.length_frames();
    let mut looping = looped;
    // Bounded: three passes plus the tail, with margin.
    let limit = 4 * length + 10 * RATE as u64;
    while backend.frames() < limit {
        backend.run_frames(u64::from(BLOCK));
        text.push_str(&log.drain_text());
        if looping && backend.frames() >= 2 * length {
            engine.set_loop(false).map_err(|e| e.to_string())?;
            looping = false;
        }
        if engine.take_ended() {
            break;
        }
    }
    let _ = backend.stop();
    Ok((report, text))
}
