#![cfg(target_os = "macos")]

use core_engine::backend::{AudioBackend, AudioConfig};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::Builder;
use core_engine::offline::OfflineSinks;
use platform_macos::hal::{CoreAudioBackend, output_devices};
use std::time::Duration;

#[test]
#[ignore = "needs an audio output device"]
fn hal_default_device_opens() {
    let mut backend = CoreAudioBackend::new();
    let info = backend.open(&AudioConfig::default()).unwrap();
    assert!(info.sample_rate > 0.0 && info.channels > 0 && info.buffer_frames > 0);
    assert!(output_devices().contains(&info.device_name));

    // An empty one-bar sequence: 2 s of silence at 120 BPM.
    let mut b = Builder::new();
    let (seq, _) = b.sequence("Silence", "L", None).unwrap();
    let doc = b.finish();
    let (mut engine, renderer) = Engine::new(
        EngineConfig::new(info.sample_rate),
        Box::new(OfflineSinks::new(16)),
    );
    engine.load(&doc, seq).unwrap();
    engine.play().unwrap();
    backend.start(renderer).unwrap();
    std::thread::sleep(Duration::from_secs(1));
    let pos = engine.position_frames() as f64;
    let renderer = backend.stop().unwrap();
    drop(renderer);
    assert!(
        (pos - info.sample_rate).abs() < info.sample_rate * 0.25,
        "advanced {pos} frames in 1 s at {}",
        info.sample_rate
    );
    assert!(engine.faults().is_empty());
}
