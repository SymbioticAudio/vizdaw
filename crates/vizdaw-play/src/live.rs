//! Live playback through CoreAudio, the DLS synth and CoreMIDI (macOS).

use crate::args::Args;
use crate::signal;
use core_engine::backend::{AudioBackend, AudioConfig, BackendEvent, StreamInfo};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::render::Renderer;
use core_engine::transport::PlayState;
use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::time::Tick;
use platform_macos::hal::CoreAudioBackend;
use platform_macos::sinks::MacSinks;
use std::io::Write;
use std::time::Duration;

struct Session {
    backend: CoreAudioBackend,
    engine: Engine,
    info: StreamInfo,
}

fn engine_config(args: &Args, rate: f64) -> EngineConfig {
    let mut cfg = EngineConfig::new(rate);
    cfg.route = args.to.into();
    cfg.midi_offset_ns = (args.midi_offset_ms * 1e6).round() as i64;
    cfg
}

/// Opens the device, builds the engine at its rate, loads and starts.
fn open(
    args: &Args,
    doc: &Document,
    seq: SeqId,
    at: Tick,
    playing: bool,
) -> Result<Session, String> {
    let mut backend = CoreAudioBackend::new();
    let cfg = AudioConfig {
        device: args.device.clone(),
        buffer_frames: args.buffer,
    };
    let info = backend
        .open(&cfg)
        .map_err(|e| format!("audio device: {e}"))?;
    let (mut engine, renderer) = Engine::new(
        engine_config(args, info.sample_rate),
        Box::new(MacSinks::new()),
    )
    .map_err(|e| e.to_string())?;
    let report = engine.load(doc, seq).map_err(|e| e.to_string())?;
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    engine.set_loop(args.looped).map_err(|e| e.to_string())?;
    if at != Tick::ZERO {
        engine.locate_tick(at).map_err(|e| e.to_string())?;
    }
    if playing {
        engine.play().map_err(|e| e.to_string())?;
    }
    backend
        .start(renderer)
        .map_err(|e| format!("audio start: {e}"))?;
    eprintln!(
        "playing on {} at {} Hz, {} frames, {} event(s), compiled in {:.1} ms",
        info.device_name,
        info.sample_rate,
        info.buffer_frames,
        report.events,
        report.compile_time.as_secs_f64() * 1e3
    );
    Ok(Session {
        backend,
        engine,
        info,
    })
}

/// Stops the device, frees the renderer on this thread, then drops the
/// engine: its MIDI sender drains and sends All Notes Off to every MIDI
/// output it owned before the next session opens any.
fn close(mut s: Session) {
    let r: Option<Renderer> = s.backend.stop().ok();
    drop(r);
    drop(s.engine);
}

/// Stops the transport and gives a live device a few blocks to play the
/// note-offs (and release tails).
fn stop_and_wait(s: &mut Session) {
    let _ = s.engine.stop();
    std::thread::sleep(Duration::from_millis(300));
}

/// Plays until the end (exit 0) or Ctrl-C.
pub fn run(args: &Args, doc: &Document, seq: SeqId) -> Result<(), String> {
    signal::install();
    let meter = doc.sequence(seq).map_err(|e| e.to_string())?.meter.clone();
    let mut s = open(args, doc, seq, Tick::ZERO, true)?;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let pos = meter.tick_to_bbu(s.engine.position_tick(), 4);
        print!("\r{:>4}.{}.{}  ", pos.bar, pos.beat, pos.unit + 1);
        let _ = std::io::stdout().flush();
        let faults = s.engine.faults();
        if !faults.is_empty() {
            let c = s.engine.counters();
            eprintln!(
                "\nfault: {} (midi dropped {}, swaps deferred {}, xruns {})",
                faults.names().join(", "),
                c.midi_dropped,
                c.swaps_deferred,
                c.xruns
            );
        }
        if s.engine.take_ended() {
            println!();
            break;
        }
        if signal::interrupted() {
            // Stop sends note-offs; give the device a few blocks to play
            // them and the release tails.
            stop_and_wait(&mut s);
            println!();
            break;
        }
        for ev in s.backend.take_events() {
            match ev {
                BackendEvent::BufferSizeChanged(n) => {
                    eprintln!("\nbuffer size changed to {n}");
                    let mut r = s.backend.stop().map_err(|e| e.to_string())?;
                    if n > r.config().max_frames {
                        r.prepare(n);
                    }
                    s.backend.start(r).map_err(|e| e.to_string())?;
                }
                BackendEvent::DeviceLost
                | BackendEvent::DefaultDeviceChanged
                | BackendEvent::SampleRateChanged(_) => {
                    eprintln!("\n{ev:?}: reopening the output");
                    let at = s.engine.position_tick();
                    let playing = s.engine.state() == PlayState::Playing;
                    if !matches!(ev, BackendEvent::DeviceLost) {
                        // The device still renders: let the engine send its
                        // note-offs to MIDI outputs before the rebuild.
                        stop_and_wait(&mut s);
                    }
                    let old_rate = s.info.sample_rate;
                    close(s);
                    s = open(args, doc, seq, at, playing)?;
                    if s.info.sample_rate != old_rate {
                        eprintln!(
                            "sample rate {old_rate} -> {}: recompiled",
                            s.info.sample_rate
                        );
                    }
                    break;
                }
            }
        }
    }
    close(s);
    Ok(())
}
