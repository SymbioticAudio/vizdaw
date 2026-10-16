//! The process-wide engine host: one engine, one output, one playing
//! session at a time. Created lazily on the first Play.

use crate::error::VizError;
use crate::guard::lock;
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::offline::{EventLog, OfflineBackend, OfflineSinks};
use core_engine::render::Renderer;
use core_engine::schedule::CompileReport;
use core_engine::transport::PlayState;
use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::time::Tick;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Offline sample rate.
pub const OFFLINE_RATE: f64 = 48_000.0;
/// Offline block size.
pub const OFFLINE_BLOCK: u32 = 512;
/// Device label of the offline backend.
pub const OFFLINE_DEVICE: &str = "Offline";

static HOST: Mutex<Option<Host>> = Mutex::new(None);
static FORCE_OFFLINE: AtomicBool = AtomicBool::new(false);

/// True when the offline backend is selected (`VIZDAW_AUDIO=offline`, or
/// the `force_offline` test hook).
pub fn offline_mode() -> bool {
    FORCE_OFFLINE.load(Ordering::Relaxed)
        || std::env::var("VIZDAW_AUDIO").is_ok_and(|v| v == "offline")
}

/// Selects the offline backend for this process (test hook).
#[cfg(feature = "test-hooks")]
pub fn force_offline() {
    FORCE_OFFLINE.store(true, Ordering::Relaxed);
}

/// Drops the host (test hook): the next Play creates a new one.
#[cfg(feature = "test-hooks")]
pub fn reset_host() {
    reset();
}

/// Drains the offline event log as `(frame, bytes)` (test hook).
#[cfg(feature = "test-hooks")]
pub fn drain_offline_log() -> Vec<(u64, Vec<u8>)> {
    let host = lock(&HOST);
    host.as_ref()
        .and_then(|h| h.log.as_ref())
        .map(|log| {
            log.drain()
                .iter()
                .map(|r| (r.frame, r.bytes().to_vec()))
                .collect()
        })
        .unwrap_or_default()
}

/// What the next `reload` should do instead of recompiling (test hook).
#[cfg(feature = "test-hooks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadFault {
    /// Return an engine error.
    Error,
    /// Panic while holding the host lock.
    Panic,
}

#[cfg(feature = "test-hooks")]
static RELOAD_FAULT: Mutex<Option<ReloadFault>> = Mutex::new(None);

/// Makes the next `reload` that reaches the engine fail (test hook).
#[cfg(feature = "test-hooks")]
pub fn inject_reload_fault(f: Option<ReloadFault>) {
    *lock(&RELOAD_FAULT) = f;
}

/// True when a host exists and its engine is not stopped (test hook).
#[cfg(feature = "test-hooks")]
pub fn engine_active() -> bool {
    lock(&HOST)
        .as_ref()
        .is_some_and(|h| h.engine.state() != PlayState::Stopped)
}

/// Drops the host, stopping its output.
pub fn reset() {
    let old = lock(&HOST).take();
    drop(old);
}

/// Renders an offline backend in real time on its own thread.
struct OfflineDriver {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OfflineDriver {
    fn start(renderer: Renderer) -> Result<OfflineDriver, VizError> {
        let mut backend = OfflineBackend::new(OFFLINE_RATE, OFFLINE_BLOCK);
        backend.start(renderer)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("vizdaw-offline".into())
            .spawn(move || {
                let t0 = Instant::now();
                while !flag.load(Ordering::Relaxed) {
                    let due = (t0.elapsed().as_secs_f64() * OFFLINE_RATE) as u64;
                    if due > backend.frames() {
                        backend.run_frames(due - backend.frames());
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                let _ = backend.stop();
            })
            .map_err(|e| VizError::Engine {
                message: format!("offline thread: {e}"),
            })?;
        Ok(OfflineDriver {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for OfflineDriver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

enum Output {
    /// Held for its `Drop`, which stops the render thread.
    Offline(#[allow(dead_code)] OfflineDriver),
    #[cfg(target_os = "macos")]
    Hal(platform_macos::hal::CoreAudioBackend),
}

/// The engine and its output.
struct Host {
    // Field order matters: the output stops (and frees the renderer)
    // before the engine's collector thread is joined.
    output: Output,
    engine: Engine,
    owner: u64,
    seq: SeqId,
    device: String,
    sample_rate: f64,
    report: Vec<String>,
    faults: Vec<String>,
    /// Read only by the `drain_offline_log` test hook.
    #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
    log: Option<EventLog>,
}

impl Host {
    fn create() -> Result<Host, VizError> {
        if offline_mode() {
            let sinks = OfflineSinks::new(1 << 16);
            let log = sinks.log();
            let (engine, renderer) = Engine::new(EngineConfig::new(OFFLINE_RATE), Box::new(sinks))?;
            let output = Output::Offline(OfflineDriver::start(renderer)?);
            return Ok(Host::new(
                engine,
                output,
                OFFLINE_DEVICE,
                OFFLINE_RATE,
                Some(log),
            ));
        }
        Host::create_live()
    }

    #[cfg(target_os = "macos")]
    fn create_live() -> Result<Host, VizError> {
        use core_engine::backend::AudioConfig;
        use platform_macos::hal::CoreAudioBackend;
        use platform_macos::sinks::MacSinks;
        let mut backend = CoreAudioBackend::new();
        let info = backend.open(&AudioConfig::default())?;
        let (engine, renderer) = Engine::new(
            EngineConfig::new(info.sample_rate),
            Box::new(MacSinks::new()),
        )?;
        backend.start(renderer)?;
        Ok(Host::new(
            engine,
            Output::Hal(backend),
            &info.device_name,
            info.sample_rate,
            None,
        ))
    }

    #[cfg(not(target_os = "macos"))]
    fn create_live() -> Result<Host, VizError> {
        Err(VizError::NoDevice {
            message: "live audio is macOS-only; set VIZDAW_AUDIO=offline".into(),
        })
    }

    fn new(
        engine: Engine,
        output: Output,
        device: &str,
        sample_rate: f64,
        log: Option<EventLog>,
    ) -> Host {
        Host {
            output,
            engine,
            owner: 0,
            seq: SeqId(0),
            device: device.to_string(),
            sample_rate,
            report: Vec::new(),
            faults: Vec::new(),
            log,
        }
    }

    fn load(&mut self, doc: &Document, seq: SeqId, looped: bool) -> Result<(), VizError> {
        let report = self.engine.load(doc, seq)?;
        self.report = report_lines(&report);
        self.engine.set_loop(looped)?;
        Ok(())
    }

    /// Reads the fault bits into the sticky list and reacts to device
    /// events. Returns false when the output is gone.
    fn refresh(&mut self) -> bool {
        for name in self.engine.faults().names() {
            if !self.faults.iter().any(|f| f == name) {
                self.faults.push(name.to_string());
            }
        }
        let _ = self.engine.take_ended();
        match &mut self.output {
            Output::Offline(_) => true,
            #[cfg(target_os = "macos")]
            Output::Hal(b) => b.take_events().is_empty(),
        }
    }
}

fn report_lines(r: &CompileReport) -> Vec<String> {
    let mut lines = r.warnings.clone();
    lines.extend(r.unroutable.iter().map(|u| u.reason.clone()));
    lines
}

/// Where a session's transport stands, as the host sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct HostState {
    /// Playing or in the release tail.
    pub playing: bool,
    /// The loaded sequence.
    pub seq: SeqId,
    /// Position.
    pub tick: Tick,
    /// Stream rate.
    pub sample_rate: f64,
    /// Device label.
    pub device: String,
    /// Sticky fault names.
    pub faults: Vec<String>,
    /// Last compile report.
    pub report: Vec<String>,
}

/// Plays `seq` of `doc` for session `owner`. A different owner, sequence
/// or an explicit `start` reloads (stopping whoever played); otherwise the
/// engine resumes where it stopped.
pub fn play(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    start: Option<Tick>,
    looped: bool,
) -> Result<(), VizError> {
    let mut slot = lock(&HOST);
    if slot.is_none() {
        *slot = Some(Host::create()?);
    }
    let host = slot.as_mut().ok_or_else(|| VizError::Internal {
        message: "engine host missing".into(),
    })?;
    if host.owner != owner || host.seq != seq || start.is_some() {
        host.engine.stop()?;
        host.load(doc, seq, looped)?;
        host.engine.locate_tick(start.unwrap_or(Tick::ZERO))?;
        host.owner = owner;
        host.seq = seq;
    }
    host.faults.clear();
    host.engine.play()?;
    Ok(())
}

/// Stops if `owner` is playing.
pub fn stop(owner: u64) -> Result<(), VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => Ok(h.engine.stop()?),
        _ => Ok(()),
    }
}

/// Moves `owner`'s transport. Returns false when `owner` does not hold
/// the engine (the caller keeps the position for its next Play).
pub fn locate(owner: u64, tick: Tick) -> Result<bool, VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            h.engine.locate_tick(tick)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Turns looping on or off if `owner` holds the engine.
pub fn set_loop(owner: u64, on: bool) -> Result<(), VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => Ok(h.engine.set_loop(on)?),
        _ => Ok(()),
    }
}

/// Recompiles after an edit if `owner` holds the engine.
pub fn reload(owner: u64, doc: &Document, looped: bool) -> Result<(), VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            #[cfg(feature = "test-hooks")]
            match lock(&RELOAD_FAULT).take() {
                Some(ReloadFault::Error) => {
                    return Err(VizError::Engine {
                        message: "injected reload fault".into(),
                    });
                }
                #[allow(clippy::panic)]
                Some(ReloadFault::Panic) => panic!("injected reload panic"),
                None => {}
            }
            let seq = h.seq;
            // The loaded sequence may have been deleted by the edit.
            if doc.sequence(seq).is_err() {
                h.engine.stop()?;
                h.owner = 0;
                return Ok(());
            }
            h.load(doc, seq, looped)
        }
        _ => Ok(()),
    }
}

/// `owner`'s view of the transport, or `None` when it does not hold the
/// engine. Drops the host if its output device went away.
pub fn state(owner: u64) -> Option<HostState> {
    let mut slot = lock(&HOST);
    let h = slot.as_mut()?;
    if !h.refresh() {
        *slot = None;
        return None;
    }
    if h.owner != owner {
        return None;
    }
    Some(HostState {
        playing: h.engine.state() != PlayState::Stopped,
        seq: h.seq,
        tick: h.engine.position_tick(),
        sample_rate: h.sample_rate,
        device: h.device.clone(),
        faults: h.faults.clone(),
        report: h.report.clone(),
    })
}

/// Releases the engine if `owner` holds it (a session is closing).
pub fn release(owner: u64) {
    let mut slot = lock(&HOST);
    if let Some(h) = slot.as_mut() {
        if h.owner == owner {
            let _ = h.engine.stop();
            h.owner = 0;
        }
    }
}
