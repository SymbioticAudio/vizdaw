//! Live playback through CoreAudio, the mixer (with the project's AU and
//! CLAP plugins) and CoreMIDI (macOS), rendered in parallel on worker
//! threads joined to the device's workgroup.

use crate::args::Args;
use crate::{plugins, project, signal};
use core_engine::backend::{AudioBackend, AudioConfig, BackendEvent, StreamInfo};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::pool::RtEnv;
use core_engine::render::Renderer;
use core_engine::transport::PlayState;
use core_host::PluginHost;
use core_host::catalog::Catalog;
use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::time::Tick;
use platform_macos::hal::CoreAudioBackend;
use platform_macos::rt::{MacRtPlatform, default_workers};
use platform_macos::sinks::MacSinks;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

/// Live blocks may use 80 % of their duration (Plan Ruling 18).
const DEADLINE: f64 = 0.8;

struct Session {
    backend: CoreAudioBackend,
    engine: Engine,
    /// Dropped after the engine: processors come back through it.
    host: PluginHost,
    info: StreamInfo,
}

fn engine_config(args: &Args, rate: f64) -> EngineConfig {
    let mut cfg = EngineConfig::new(rate);
    cfg.route = args.to.into();
    cfg.midi_offset_ns = (args.midi_offset_ms * 1e6).round() as i64;
    cfg.workers = args.workers.map_or_else(default_workers, |w| w as usize);
    cfg.deadline = Some(DEADLINE);
    cfg
}

/// The device's block period and workgroup, for the workers. The workgroup
/// is fetched from the backend as it is now, so a reopened device hands the
/// workers its own.
fn rt_env(backend: &CoreAudioBackend, info: &StreamInfo) -> RtEnv {
    RtEnv {
        period_ns: (f64::from(info.buffer_frames) * 1e9 / info.sample_rate.max(1.0)).round() as u64,
        workgroup: backend.workgroup(),
    }
}

/// Opens the device, builds the engine at its rate, loads and starts.
fn open(
    args: &Args,
    doc: &Document,
    seq: SeqId,
    catalog: &mut Catalog,
    at: Tick,
    playing: bool,
) -> Result<Session, String> {
    let mut backend = CoreAudioBackend::new();
    let cfg = AudioConfig {
        device: args.device.clone(),
        buffer_frames: args.buffer,
        sample_rate: None,
    };
    let info = backend
        .open(&cfg)
        .map_err(|e| format!("audio device: {e}"))?;
    let ecfg = engine_config(args, info.sample_rate);
    let (mut engine, renderer) =
        Engine::with_platform(ecfg, Box::new(MacSinks::new()), Arc::new(MacRtPlatform))
            .map_err(|e| e.to_string())?;
    let env = rt_env(&backend, &info);
    let joined = env.workgroup.is_some();
    engine.set_rt_env(env);
    // Plugins load (and fail, reported) before the first graph.
    let host = plugins::host(doc, catalog, info.sample_rate, engine.block_frames())?;
    engine.set_insert_factory(Box::new(host.factory()));
    if let Some(package) = &args.project {
        engine.set_media(project::media_files(doc, package));
    }
    let report = engine.load(doc, seq).map_err(|e| e.to_string())?;
    for w in report.warnings.iter().chain(&report.graph.warnings) {
        eprintln!("warning: {w}");
    }
    for (id, why) in &report.graph.placeholders {
        eprintln!("warning: {id}: {why}");
    }
    if let Some(e) = &report.graph.error {
        eprintln!("warning: mixer graph not used: {e}");
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
        "playing on {} at {} Hz, {} frames, {} event(s), compiled in {:.1} ms, \
         {} worker(s){}, latency {} frame(s)",
        info.device_name,
        info.sample_rate,
        info.buffer_frames,
        report.events,
        report.compile_time.as_secs_f64() * 1e3,
        ecfg.workers,
        if joined {
            " in the device workgroup"
        } else {
            ""
        },
        report.graph.latency_frames
    );
    Ok(Session {
        backend,
        engine,
        host,
        info,
    })
}

/// Stops the device, frees the renderer on this thread, then drops the
/// engine (its MIDI sender drains and sends All Notes Off to every MIDI
/// output it owned before the next session opens any), then the plugin
/// host, which deactivates the processors the engine gave back.
fn close(mut s: Session) {
    let r: Option<Renderer> = s.backend.stop().ok();
    drop(r);
    drop(s.engine);
    drop(s.host);
}

/// Stops the transport and gives a live device a few blocks to play the
/// note-offs (and release tails).
fn stop_and_wait(s: &mut Session) {
    let _ = s.engine.stop();
    std::thread::sleep(Duration::from_millis(300));
}

/// Plays until the end (exit 0) or Ctrl-C.
pub fn run(args: &Args, doc: &Document, seq: SeqId, catalog: &mut Catalog) -> Result<(), String> {
    signal::install();
    let meter = doc.sequence(seq).map_err(|e| e.to_string())?.meter.clone();
    let mut s = open(args, doc, seq, catalog, Tick::ZERO, true)?;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        // `host_idle`: plugin callbacks, restarts (a latency change
        // recompiles), processors coming back.
        // An error is reported and playback goes on: the host keeps a
        // failed load and retries it.
        match s.host.service(&mut s.engine, doc, seq) {
            Ok(r) => {
                if let Some(c) = r.compiled {
                    eprintln!("\nrecompiled: latency {} frame(s)", c.graph.latency_frames);
                }
            }
            Err(e) => eprintln!("\nwarning: plugin host: {e}"),
        }
        let pos = meter.tick_to_bbu(s.engine.position_tick(), 4);
        let load = s.engine.dsp_load();
        print!(
            "\r{:>4}.{}.{}  load {:>3.0}%  ",
            pos.bar,
            pos.beat,
            pos.unit + 1,
            load.peak * 100.0
        );
        let _ = std::io::stdout().flush();
        let faults = s.engine.faults();
        if !faults.is_empty() {
            let c = s.engine.counters();
            eprintln!(
                "\nfault: {} (midi dropped {}, swaps deferred {}, xruns {}, deadline misses {}, \
                 busy blocks {}, plugin faults {})",
                faults.names().join(", "),
                c.midi_dropped,
                c.swaps_deferred,
                c.xruns,
                c.deadline_misses,
                c.busy_blocks,
                c.plugin_faults
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
                    s.info.buffer_frames = n;
                    s.engine.set_rt_env(rt_env(&s.backend, &s.info));
                }
                BackendEvent::DeviceLost
                | BackendEvent::DefaultDeviceChanged
                | BackendEvent::SampleRateChanged(_) => {
                    eprintln!("\n{ev:?}: reopening the output");
                    let at = s.engine.transport_tick();
                    let playing = s.engine.state() == PlayState::Playing;
                    if !matches!(ev, BackendEvent::DeviceLost) {
                        // The device still renders: let the engine send its
                        // note-offs to MIDI outputs before the rebuild.
                        stop_and_wait(&mut s);
                    }
                    let old_rate = s.info.sample_rate;
                    close(s);
                    s = open(args, doc, seq, catalog, at, playing)?;
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
