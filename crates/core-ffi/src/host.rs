//! The process-wide engine host: one engine, one output, one playing
//! session at a time. Created lazily on the first Play. The engine plays
//! the inserts and plugin instruments of the session that played last
//! (its plugin host is the engine's insert factory, Plan Ruling 2).

use crate::error::VizError;
use crate::guard::lock;
use crate::plugins;
use core_engine::backend::{AudioBackend, BackendEvent};
use core_engine::engine::{Engine, EngineConfig, PendingPlay};
use core_engine::graph::NoInserts;
use core_engine::input::{InputSpec, MidiInputSpec, MidiSender, MonitorRoute, port_key};
use core_engine::offline::{EventLog, FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::{RecordHandles, RecordRequest};
use core_engine::render::Renderer;
use core_engine::schedule::CompileReport;
use core_engine::transport::PlayState;
use core_host::{IdleReport, PluginHost};
use core_model::document::Document;
use core_model::ids::{DestId, SeqId};
use core_model::lane::{LaneInput, MonitorMode};
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A rate offline hosts run at instead of the project's (f64 bits; 0:
/// follow the project, 5a Ruling 20).
static OFFLINE_RATE_BITS: AtomicU64 = AtomicU64::new(0);

/// The offline device's rate for a project at `project` Hz.
fn offline_rate(project: f64) -> f64 {
    let r = f64::from_bits(OFFLINE_RATE_BITS.load(Ordering::Relaxed));
    if r > 0.0 { r } else { project }
}

/// Makes offline hosts created from now on run at `rate` whatever the
/// project's rate (test hook: a device that refused it); 0 follows the
/// project again.
#[cfg(feature = "test-hooks")]
pub fn set_offline_rate(rate: f64) {
    OFFLINE_RATE_BITS.store(rate.to_bits(), Ordering::Relaxed);
}

/// Two rates are the same within half a hertz.
fn same_rate(a: f64, b: f64) -> bool {
    (a - b).abs() <= 0.5
}

/// The rate a host for `doc` asks the device for: the project rate.
fn project_rate(doc: &Document) -> f64 {
    f64::from(doc.settings().format.sample_rate)
}
/// Offline block size.
pub const OFFLINE_BLOCK: u32 = 512;
/// Device label of the offline backend.
pub const OFFLINE_DEVICE: &str = "Offline";

static HOST: Mutex<Option<Host>> = Mutex::new(None);
static FORCE_OFFLINE: AtomicBool = AtomicBool::new(false);
/// Engine loads made by the host (test hook counter).
static LOADS: AtomicU64 = AtomicU64::new(0);
static HOSTS_CREATED: AtomicU64 = AtomicU64::new(0);
/// Hands out [`Host::epoch`] values (process-wide, so a new host never
/// reuses an old one's).
static EPOCHS: AtomicU64 = AtomicU64::new(1);
/// New hosts' disks stall (the `stall_disk` test hook).
static STALL_DISK: AtomicBool = AtomicBool::new(false);
/// The fake input offline hosts get: `Some(choice)` set by the
/// `set_fake_input` test hook, `None` read from the environment.
static FAKE_INPUT: Mutex<Option<Option<FakeInput>>> = Mutex::new(None);

/// The fake input of offline hosts created from now on (spec §12.1):
/// `VIZDAW_AUDIO_LOOPBACK=<frames>` loops the output back, else
/// `VIZDAW_AUDIO_INPUT=fake` gives the test signals; neither is no input.
/// Tests choose with the `set_fake_input` hook instead. Never a device:
/// an offline host opens no audio hardware, so no microphone permission
/// is asked (plan 5c Global Constraints).
pub fn fake_input() -> Option<FakeInput> {
    if let Some(v) = *lock(&FAKE_INPUT) {
        return v;
    }
    if let Some(n) = std::env::var("VIZDAW_AUDIO_LOOPBACK")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
    {
        return Some(FakeInput::Loopback(n));
    }
    std::env::var("VIZDAW_AUDIO_INPUT")
        .is_ok_and(|v| v == "fake")
        .then_some(FakeInput::Signals)
}

/// Makes offline hosts created from now on use `input` (test hook; it
/// wins over the environment).
#[cfg(feature = "test-hooks")]
pub fn set_fake_input(input: Option<FakeInput>) {
    *lock(&FAKE_INPUT) = Some(input);
}

/// What a host's audio input offers (spec §6.1, §12.1 `state.get input`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputInfo {
    /// Input channels (0: no input).
    pub channels: u32,
    /// Their names.
    pub names: Vec<String>,
    /// Input latency in frames.
    pub input_latency_frames: u32,
    /// Output latency in frames.
    pub output_latency_frames: u32,
}

/// What the lanes of `seq` listen to (spec §6.6): armed or Input-monitored
/// lanes with an input. MIDI lanes play thru unless their monitor is Off;
/// audio lanes are heard through their output path (the document's first
/// when they have none) unless it is Off.
pub fn input_spec(doc: &Document, seq: SeqId) -> InputSpec {
    let mut spec = InputSpec::default();
    let Ok(s) = doc.sequence(seq) else {
        return spec;
    };
    let first_out = doc.outputs().keys().next().copied();
    for l in &s.lanes {
        if !(l.arm || l.monitor == MonitorMode::Input) {
            continue;
        }
        match &l.input {
            Some(LaneInput::Midi { port, channel }) => spec.midi.push(MidiInputSpec {
                lane: l.id,
                port: port.as_deref().map(port_key),
                channel: *channel,
                dest: l.midi_dest,
                thru: l.monitor != MonitorMode::Off,
                armed: l.arm,
            }),
            Some(LaneInput::Audio { channels }) if l.monitor != MonitorMode::Off => {
                if let Some(path) = l.audio_out.or(first_out) {
                    spec.monitor.push(MonitorRoute {
                        path,
                        left: channels.first(),
                        right: channels.last().unwrap_or(channels.first()),
                        mode: l.monitor,
                        armed: l.arm,
                    });
                }
            }
            _ => {}
        }
    }
    spec
}

fn next_epoch() -> u64 {
    EPOCHS.fetch_add(1, Ordering::Relaxed)
}

/// Each session's online media, handed to the engine before every load
/// (5a Task 10).
static MEDIA: Mutex<BTreeMap<u64, Vec<core_media::stream::MediaFile>>> =
    Mutex::new(BTreeMap::new());

/// Sets session `owner`'s online media; reloads use it.
pub fn set_media(owner: u64, files: Vec<core_media::stream::MediaFile>) {
    lock(&MEDIA).insert(owner, files);
}

/// Forgets session `owner`'s media (it closed).
pub fn forget_media(owner: u64) {
    lock(&MEDIA).remove(&owner);
}

/// Session `owner`'s media table as `(media, path)` (test hook).
#[cfg(feature = "test-hooks")]
pub fn debug_media(owner: u64) -> Vec<(u64, String)> {
    media_of(owner)
        .into_iter()
        .map(|f| (f.id.0, f.path.to_string_lossy().into_owned()))
        .collect()
}

fn media_of(owner: u64) -> Vec<core_media::stream::MediaFile> {
    lock(&MEDIA).get(&owner).cloned().unwrap_or_default()
}

/// Media whose reads failed while `owner` played (each once).
pub fn take_stream_errors(owner: u64) -> Vec<(core_model::ids::MediaId, String)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.take_stream_errors())
        .unwrap_or_default()
}

/// Stalls the disk of the current host and of hosts created from now on
/// (`true`), as a disk that never answers: a Play waits its full
/// `LOCATE_WAIT` (test hook).
#[cfg(feature = "test-hooks")]
pub fn stall_disk(on: bool) {
    STALL_DISK.store(on, Ordering::Relaxed);
    if let Some(h) = lock(&HOST).as_ref() {
        h.engine.streamer().set_stalled(on);
    }
}

/// How long a still-running device gets to play the note-offs before a
/// rebuild (as in `vizdaw-play`'s live loop).
const DRAIN: Duration = Duration::from_millis(300);

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
    lock(&RETIRED_LOG).clear();
}

/// Queues a device event as if the output reported it (test hook); the
/// owner's next `state` call handles it.
#[cfg(feature = "test-hooks")]
pub fn simulate_event(ev: BackendEvent) {
    if let Some(h) = lock(&HOST).as_mut() {
        h.injected.push(ev);
    }
}

/// Number of hosts created so far in this process (test hook).
#[cfg(feature = "test-hooks")]
pub fn hosts_created() -> u64 {
    HOSTS_CREATED.load(Ordering::Relaxed)
}

/// The engine's automation override of `strip`'s fader in dB, if it holds
/// one (test hook).
#[cfg(feature = "test-hooks")]
pub fn fader_override(strip: u64) -> Option<f64> {
    lock(&HOST)
        .as_ref()?
        .engine
        .overrides()?
        .fader_db(core_model::ids::StripId(strip))
}

/// Offline events of hosts retired for another session's document (their
/// logs go with them), until the next `drain_offline_log`.
#[cfg(feature = "test-hooks")]
static RETIRED_LOG: Mutex<Vec<(u64, Vec<u8>)>> = Mutex::new(Vec::new());

/// Drains the offline event log as `(frame, bytes)` (test hook): the
/// events of hosts retired since the last drain first, then the current
/// host's.
#[cfg(feature = "test-hooks")]
pub fn drain_offline_log() -> Vec<(u64, Vec<u8>)> {
    let mut events = std::mem::take(&mut *lock(&RETIRED_LOG));
    let host = lock(&HOST);
    if let Some(log) = host.as_ref().and_then(|h| h.log.as_ref()) {
        events.extend(log.drain().iter().map(|r| (r.frame, r.bytes().to_vec())));
    }
    events
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

/// Closed sessions' plugin hosts still waiting for processors (test hook).
#[cfg(feature = "test-hooks")]
pub fn closing_hosts_waiting() -> usize {
    plugins::closing_hosts()
}

/// How many engine loads the host made (test hook).
#[cfg(feature = "test-hooks")]
pub fn loads() -> u64 {
    LOADS.load(Ordering::Relaxed)
}

/// The live engine's worker count (test hook; `None` without a host).
#[cfg(feature = "test-hooks")]
pub fn engine_workers() -> Option<usize> {
    lock(&HOST).as_ref().map(|h| h.engine.config().workers)
}

/// True when a host exists and its engine is not stopped (test hook).
#[cfg(feature = "test-hooks")]
pub fn engine_active() -> bool {
    lock(&HOST)
        .as_ref()
        .is_some_and(|h| h.engine.state() != PlayState::Stopped)
}

/// Lets a stopped host's still-running output play its note-offs, then
/// drops it (output first, then the engine). Call without the host lock.
fn retire(old: Option<Host>) {
    let Some(old) = old else {
        return;
    };
    std::thread::sleep(DRAIN);
    #[cfg(feature = "test-hooks")]
    if let Some(log) = &old.log {
        lock(&RETIRED_LOG).extend(log.drain().iter().map(|r| (r.frame, r.bytes().to_vec())));
    }
    drop(old);
}

/// Drops the host: its engine stops first, so played and auditioned
/// notes end (their note-offs reach external MIDI destinations), then the
/// output plays them and the host goes, without the host lock (as in
/// `play`'s handover).
pub fn reset() {
    let mut old = lock(&HOST).take();
    if let Some(h) = old.as_mut() {
        let _ = h.engine.stop();
    }
    retire(old);
}

/// Renders an offline backend in real time on its own thread.
struct OfflineDriver {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OfflineDriver {
    fn start(
        renderer: Renderer,
        rate: f64,
        input: Option<FakeInput>,
    ) -> Result<(OfflineDriver, InputInfo), VizError> {
        let mut backend = OfflineBackend::new(rate, OFFLINE_BLOCK);
        if let Some(k) = input {
            backend = backend.with_input(k);
        }
        let info = backend.open(&core_engine::backend::AudioConfig::default())?;
        let inputs = InputInfo {
            channels: info.input_channels,
            names: info.input_channel_names,
            input_latency_frames: info.input_latency_frames,
            output_latency_frames: info.output_latency_frames,
        };
        backend.start(renderer)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("vizdaw-offline".into())
            .spawn(move || {
                let t0 = Instant::now();
                while !flag.load(Ordering::Relaxed) {
                    let due = (t0.elapsed().as_secs_f64() * rate) as u64;
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
        Ok((
            OfflineDriver {
                stop,
                thread: Some(thread),
            },
            inputs,
        ))
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
    /// The session whose plugin host is the engine's insert factory (0:
    /// none).
    factory_owner: u64,
    /// The engine loaded a document with plugin slots since it was made:
    /// its instances and plugin targets are keyed by that document's
    /// instance IDs, so another session must not inherit them (Plan
    /// Ruling 2).
    plugins_in_engine: bool,
    seq: SeqId,
    looped: bool,
    /// Play was asked and not stopped since. The engine applies Play on
    /// its next audio block, so `engine.state()` can still say Stopped
    /// right after `play`; a rebuild must resume from this too.
    want_play: bool,
    /// Where the asked Play stops (Play Selection, plan 4f): a rebuild
    /// resumes with it.
    until: Option<Tick>,
    /// Changed by every Play, Stop and release: a Play that waited for
    /// the disk without the lock goes on only if nothing came meanwhile
    /// ([`finish_play`]).
    epoch: u64,
    device: String,
    sample_rate: f64,
    /// The project rate this host was opened for (the device was asked
    /// for it; Ruling 20).
    project_rate: f64,
    /// The device runs at another rate than `project_rate`: regions are
    /// muted.
    rate_mismatch: bool,
    report: Vec<String>,
    faults: Vec<String>,
    /// Instances the engine bypassed for faults (sticky for this engine,
    /// 3a Plan Ruling 23).
    faulted: std::collections::BTreeSet<u64>,
    /// Read only by the `drain_offline_log` test hook.
    #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
    log: Option<EventLog>,
    /// Events queued by the `simulate_event` test hook.
    #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
    injected: Vec<BackendEvent>,
    /// What the output's input offers.
    inputs: InputInfo,
}

impl Host {
    /// A host for a project at `project` Hz: the device is asked for that
    /// rate; when it runs at another, regions are muted (Ruling 20).
    fn create(project: f64) -> Result<Host, VizError> {
        let mut h = Host::create_any(project)?;
        if STALL_DISK.load(Ordering::Relaxed) {
            h.engine.streamer().set_stalled(true);
        }
        h.project_rate = project;
        h.rate_mismatch = !same_rate(h.sample_rate, project);
        h.engine.set_regions_muted(h.rate_mismatch);
        Ok(h)
    }

    fn create_any(project: f64) -> Result<Host, VizError> {
        HOSTS_CREATED.fetch_add(1, Ordering::Relaxed);
        if offline_mode() {
            let sinks = OfflineSinks::new(1 << 16);
            let log = sinks.log();
            let rate = offline_rate(project);
            let (engine, renderer) = Engine::new(EngineConfig::new(rate), Box::new(sinks))?;
            let (driver, inputs) = OfflineDriver::start(renderer, rate, fake_input())?;
            let mut h = Host::new(
                engine,
                Output::Offline(driver),
                OFFLINE_DEVICE,
                rate,
                Some(log),
            );
            h.inputs = inputs;
            return Ok(h);
        }
        Host::create_live(project)
    }

    #[cfg(target_os = "macos")]
    fn create_live(project: f64) -> Result<Host, VizError> {
        use core_engine::backend::AudioConfig;
        use platform_macos::hal::CoreAudioBackend;
        use platform_macos::rt::MacRtPlatform;
        use platform_macos::sinks::MacSinks;
        use std::sync::Arc;
        let mut backend = CoreAudioBackend::new();
        let info = backend.open(&AudioConfig {
            sample_rate: Some(project),
            ..AudioConfig::default()
        })?;
        let (engine, renderer) = Engine::with_platform(
            live_config(info.sample_rate),
            Box::new(MacSinks::new()),
            Arc::new(MacRtPlatform),
        )?;
        engine.set_rt_env(rt_env(&backend, info.buffer_frames, info.sample_rate));
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
    fn create_live(_project: f64) -> Result<Host, VizError> {
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
            factory_owner: 0,
            plugins_in_engine: false,
            seq: SeqId(0),
            looped: false,
            want_play: false,
            until: None,
            epoch: next_epoch(),
            device: device.to_string(),
            sample_rate,
            project_rate: sample_rate,
            rate_mismatch: false,
            report: Vec::new(),
            faults: Vec::new(),
            faulted: std::collections::BTreeSet::new(),
            log,
            injected: Vec::new(),
            inputs: InputInfo::default(),
        }
    }

    /// Makes `owner`'s plugin host the engine's insert factory (if it is
    /// not already) and activates its plugins for this engine's rate and
    /// block size.
    fn attach(&mut self, owner: u64, plugins: Option<&mut PluginHost>) -> bool {
        let mut restarted = false;
        let block = self.engine.block_frames();
        if self.factory_owner != owner {
            match plugins.as_deref() {
                Some(p) => self.engine.set_insert_factory(Box::new(p.factory())),
                None => self.engine.set_insert_factory(Box::new(NoInserts)),
            }
            self.factory_owner = owner;
        }
        if let Some(p) = plugins {
            // Instances unloaded since (a plugin turned off): the engine
            // must not keep playing them.
            for id in p.take_evicted() {
                self.engine.restart_instance(id);
                restarted = true;
            }
            let (rate, max) = p.config();
            if rate != self.sample_rate || max < block {
                // Processors still in the engine are made for the old
                // configuration: the engine drops them (they come back and
                // are activated again; the host then owes a load).
                for id in p.reconfigure(self.sample_rate, block.max(max)) {
                    self.engine.restart_instance(id);
                    restarted = true;
                }
            }
        }
        plugins::set_config(self.sample_rate, block);
        restarted
    }

    fn load(
        &mut self,
        owner: u64,
        doc: &Document,
        seq: SeqId,
        looped: bool,
    ) -> Result<(), VizError> {
        LOADS.fetch_add(1, Ordering::Relaxed);
        self.plugins_in_engine |= has_slots(doc);
        self.engine.set_media(media_of(owner));
        self.engine.set_inputs(input_spec(doc, seq));
        let report = self.engine.load(doc, seq)?;
        self.report = report_lines(&report);
        self.engine.set_loop(looped)?;
        self.looped = looped;
        Ok(())
    }

    /// Reads the fault bits into the sticky list.
    fn collect_faults(&mut self) {
        for name in self.engine.faults().names() {
            if !self.faults.iter().any(|f| f == name) {
                self.faults.push(name.to_string());
            }
        }
        if self.engine.take_ended() {
            self.want_play = false;
        }
    }

    /// Device events reported since the last call (plus injected ones).
    fn take_events(&mut self) -> Vec<BackendEvent> {
        #[allow(unused_mut)]
        let mut events = std::mem::take(&mut self.injected);
        match &mut self.output {
            Output::Offline(_) => {}
            #[cfg(target_os = "macos")]
            Output::Hal(b) => events.extend(b.take_events()),
        }
        events
    }

    /// Restarts the device, preparing a larger buffer if needed (as the
    /// CLI does).
    fn restart_output(&mut self, frames: u32) -> Result<(), VizError> {
        match &mut self.output {
            Output::Offline(_) => {
                let _ = frames;
                Ok(())
            }
            #[cfg(target_os = "macos")]
            Output::Hal(b) => {
                let mut r = b.stop()?;
                if frames > r.config().max_frames {
                    r.prepare(frames);
                }
                b.start(r)?;
                // The workers follow the new block period.
                self.engine.set_rt_env(rt_env(b, frames, self.sample_rate));
                Ok(())
            }
        }
    }
}

/// Live blocks may use 80 % of their duration (3a Plan Ruling 18).
pub const DEADLINE: f64 = 0.8;

/// The live engine's settings: the spec's default worker count
/// `min(performance cores − 1, 7)` (3a Plan Ruling 20) and the deadline.
/// The offline backend keeps 0 workers and no deadline.
#[cfg(target_os = "macos")]
pub fn live_config(sample_rate: f64) -> EngineConfig {
    let mut cfg = EngineConfig::new(sample_rate);
    cfg.workers = platform_macos::rt::default_workers();
    cfg.deadline = Some(DEADLINE);
    cfg
}

/// The device's block period and workgroup, for the workers.
#[cfg(target_os = "macos")]
fn rt_env(
    backend: &platform_macos::hal::CoreAudioBackend,
    frames: u32,
    sample_rate: f64,
) -> core_engine::pool::RtEnv {
    core_engine::pool::RtEnv {
        period_ns: (f64::from(frames) * 1e9 / sample_rate.max(1.0)).round() as u64,
        workgroup: backend.workgroup(),
    }
}

/// A rebuild `handle_events` started: the old host is out of the slot;
/// [`rebuild`] finishes it without the host lock.
struct Rebuild {
    old: Host,
    at: Tick,
    playing: bool,
    /// The device still renders: its note-offs get `DRAIN` to play.
    drain: bool,
}

/// Handles device events for the owner (mirrors `vizdaw-play`'s live
/// loop): a buffer-size change restarts the output at once; a lost device,
/// a new default device or a rate change take the host out of `slot` and
/// return the rebuild, which the caller runs with [`rebuild`] after
/// dropping the host lock (Plan 4d Ruling 1: the drain sleeps 300 ms).
fn handle_events(slot: &mut Option<Host>) -> Result<Option<Rebuild>, VizError> {
    let Some(h) = slot.as_mut() else {
        return Ok(None);
    };
    for ev in h.take_events() {
        match ev {
            BackendEvent::BufferSizeChanged(n) => {
                if let Some(h) = slot.as_mut() {
                    h.restart_output(n)?;
                }
            }
            BackendEvent::DeviceLost
            | BackendEvent::DefaultDeviceChanged
            | BackendEvent::SampleRateChanged(_) => {
                let Some(mut old) = slot.take() else {
                    return Ok(None);
                };
                let at = old.engine.transport_tick();
                // Playback that ended by itself is not resumed.
                if old.engine.take_ended() {
                    old.want_play = false;
                }
                // `want_play` is the user's intent (the engine applies Play
                // and Stop a block late, so its state can lag it). A song in
                // its release tail is past its end: a Play would restart it.
                let playing = old.want_play && old.engine.state() != PlayState::Tail;
                let drain = !matches!(ev, BackendEvent::DeviceLost);
                if drain {
                    let _ = old.engine.stop();
                }
                // The rebuilt host replaces any later queued events.
                return Ok(Some(Rebuild {
                    old,
                    at,
                    playing,
                    drain,
                }));
            }
        }
    }
    Ok(None)
}

/// Finishes a rebuild without the host lock: lets a still-running output
/// play its note-offs, drops it (output first, then the engine, whose
/// processors go back to the plugin host), makes a new host for the
/// owner, recompiles and resumes at the same tick. The owner's plugins
/// follow the new engine's rate (`attach`). A failed rebuild leaves no
/// host; a host another call made meanwhile wins.
fn rebuild(r: Rebuild, doc: &Document, plugins: Option<&mut PluginHost>) -> Result<(), VizError> {
    let Rebuild {
        old,
        at,
        playing,
        drain,
    } = r;
    let (owner, seq, looped, until) = (old.owner, old.seq, old.looped, old.until);
    if drain {
        std::thread::sleep(DRAIN);
    }
    // Read once the old output has stopped (dropped first, below).
    let overrides = old.engine.overrides();
    drop(old);
    let mut new = Host::create(project_rate(doc))?;
    new.attach(owner, plugins);
    // Automated values held while stopped survive the rebuild (Plan 4d
    // Ruling 13, final review Minor 1).
    if let Some(o) = overrides {
        new.engine.seed_overrides(o);
    }
    new.load(owner, doc, seq, looped)?;
    new.engine.locate_tick(at)?;
    if playing {
        match until {
            Some(t) => new.engine.play_until(t)?,
            None => new.engine.play()?,
        }
        new.want_play = true;
        new.until = until;
    }
    new.owner = owner;
    new.seq = seq;
    let mut slot = lock(&HOST);
    let spare = if slot.is_none() {
        *slot = Some(new);
        None
    } else {
        Some(new)
    };
    drop(slot);
    drop(spare);
    Ok(())
}

/// The compile report as messages: warnings, unroutable events, then the
/// mixer graph's (placeholders as "plugin N: why", other warnings, and why
/// a graph was not used).
fn report_lines(r: &CompileReport) -> Vec<String> {
    let mut lines = r.warnings.clone();
    lines.extend(r.unroutable.iter().map(|u| u.reason.clone()));
    let g = &r.graph;
    lines.extend(
        g.placeholders
            .iter()
            .map(|(id, why)| format!("{id}: {why}")),
    );
    lines.extend(g.warnings.iter().cloned());
    lines.extend(g.error.iter().map(|e| format!("mixer graph not used: {e}")));
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
    /// Host time (ns) stamped right after `tick` was read.
    pub host_time_ns: u64,
    /// Stream rate.
    pub sample_rate: f64,
    /// Device label.
    pub device: String,
    /// Sticky fault names.
    pub faults: Vec<String>,
    /// Last compile report.
    pub report: Vec<String>,
    /// The device runs at another rate than the project: regions are
    /// muted (Ruling 20).
    pub rate_mismatch: bool,
}

/// A Play [`play`] prepared; [`finish_play`] waits for the disk and
/// sends it.
#[derive(Debug)]
#[must_use = "the Play is sent by `finish_play`"]
pub struct HostPlay {
    owner: u64,
    epoch: u64,
    pending: PendingPlay,
}

/// Plays `seq` of `doc` for session `owner`, up to the disk wait (spec
/// §5.5): the caller drops every lock and calls [`finish_play`]. A
/// different owner, sequence or an explicit `start` reloads (stopping
/// whoever played); otherwise the engine resumes where it stopped.
/// `until` stops it there (the loop ignored; plan 4f Ruling 9).
pub fn play(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    start: Option<Tick>,
    until: Option<Tick>,
    looped: bool,
    mut plugins: Option<&mut PluginHost>,
) -> Result<HostPlay, VizError> {
    let project = project_rate(doc);
    let mut slot = loop {
        let mut slot = lock(&HOST);
        // Device events nobody polled (the host had no owner, or another
        // one): a rebuild-class event drops the host so a fresh one opens
        // below.
        if let Some(h) = slot.as_mut().filter(|h| h.owner != owner) {
            for ev in h.take_events() {
                match ev {
                    BackendEvent::BufferSizeChanged(n) => h.restart_output(n)?,
                    _ => {
                        *slot = None;
                        break;
                    }
                }
            }
        } else if let Some(r) = handle_events(&mut slot)? {
            // The drain and the rebuild run without the host lock; then
            // start over (another call may have changed the host).
            drop(slot);
            rebuild(r, doc, plugins.as_deref_mut())?;
            continue;
        }
        // Another session's plugins played here, or the host was opened for
        // another project rate: a fresh engine (for plugins: so none of its
        // instances is taken for this document's same-numbered slots; for
        // rates: so the device is asked for this project's, Ruling 20).
        if slot.as_ref().is_some_and(|h| {
            (h.factory_owner != owner && h.plugins_in_engine) || !same_rate(h.project_rate, project)
        }) {
            let mut old = slot.take();
            if let Some(h) = old.as_mut() {
                // Its played and auditioned notes end before the engine
                // goes (4b Plan Ruling 1).
                let _ = h.engine.stop();
            }
            // The note-offs play and the old host is dropped without the
            // lock, so no other host call (the UI's timers) waits on them.
            // Then start over: another call may have changed the host
            // meanwhile.
            drop(slot);
            retire(old);
            continue;
        }
        break slot;
    };
    if slot.is_none() {
        *slot = Some(Host::create(project)?);
    }
    let host = slot.as_mut().ok_or_else(|| VizError::Internal {
        message: "engine host missing".into(),
    })?;
    let restarted = host.attach(owner, plugins.as_deref_mut());
    if let Some(p) = plugins {
        // Slots still loading play as placeholders; `service` loads again
        // once they are in.
        crate::recovery::before_sync();
        p.sync(doc);
    }
    if host.owner != owner || host.seq != seq || start.is_some() {
        host.engine.stop()?;
        if host.owner != owner {
            // Another session's recording ends here: its tap must not
            // capture this one's playback.
            host.engine.stop_recording()?;
        }
        host.load(owner, doc, seq, looped)?;
        host.engine.locate_tick(start.unwrap_or(Tick::ZERO))?;
        host.owner = owner;
        host.seq = seq;
    } else if restarted {
        // Resumed with restarted instances: compile their placeholders.
        host.load(owner, doc, seq, looped)?;
    }
    host.faults.clear();
    // The intent is Play from here on: a rebuild during the wait resumes
    // playing, and a Stop during it cancels this Play.
    let pending = host.engine.begin_play(until);
    host.want_play = true;
    host.until = until;
    host.epoch = next_epoch();
    Ok(HostPlay {
        owner,
        epoch: host.epoch,
        pending,
    })
}

/// Waits for the disk (up to `LOCATE_WAIT`, without any lock: the UI's
/// polls and every other host call go on meanwhile), then sends the
/// Play, unless a Stop, another Play, a release or a new engine came in
/// the meantime (they win; a rebuilt engine resumes on its own).
pub fn finish_play(p: HostPlay) -> Result<(), VizError> {
    p.pending.wait();
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == p.owner && h.epoch == p.epoch => {
            Ok(h.engine.finish_play(p.pending)?)
        }
        _ => Ok(()),
    }
}

/// Stops if `owner` is playing.
pub fn stop(owner: u64) -> Result<(), VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            h.want_play = false;
            h.until = None;
            h.epoch = next_epoch();
            Ok(h.engine.stop()?)
        }
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
        Some(h) if h.owner == owner => {
            h.engine.set_loop(on)?;
            h.looped = on;
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Sends an audition note through `owner`'s engine (Plan Ruling 9); false
/// when `owner` does not hold the engine or it has no target for `dest`.
pub fn audition(
    owner: u64,
    doc: &Document,
    dest: DestId,
    pitch: u8,
    velocity: u16,
    on: bool,
) -> Result<bool, VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => Ok(h.engine.audition(doc, dest, pitch, velocity, on)?),
        _ => Ok(false),
    }
}

/// After an edit: starts loading the plugins `doc` gained (and drops the
/// ones it lost), then recompiles if `owner` holds the engine and no
/// plugin is loading (otherwise `service` loads once they are in).
pub fn reload(
    owner: u64,
    doc: &Document,
    looped: bool,
    mut plugins: Option<&mut PluginHost>,
) -> Result<(), VizError> {
    let ready = plugins.as_deref_mut().is_none_or(|p| {
        crate::recovery::before_sync();
        p.sync(doc)
    });
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
                h.epoch = next_epoch();
                h.want_play = false;
                h.owner = 0;
                return Ok(());
            }
            if h.factory_owner == owner {
                // Instances unloaded since (a plugin turned off): dropped
                // by this load (or the one `service` runs once loads are
                // done).
                for id in plugins.map(PluginHost::take_evicted).unwrap_or_default() {
                    h.engine.restart_instance(id);
                }
            }
            if !ready {
                return Ok(());
            }
            h.load(owner, doc, seq, looped)
        }
        _ => Ok(()),
    }
}

/// `owner`'s view of the transport, or `None` when it does not hold the
/// engine. For the owner, device events are handled first (`doc` is the
/// owner's document, needed to recompile after a rebuild).
pub fn state(
    owner: u64,
    doc: &Document,
    plugins: Option<&mut PluginHost>,
) -> Result<Option<HostState>, VizError> {
    let mut slot = lock(&HOST);
    match slot.as_ref() {
        Some(h) if h.owner == owner => {}
        _ => return Ok(None),
    }
    if let Some(r) = handle_events(&mut slot)? {
        drop(slot);
        rebuild(r, doc, plugins)?;
        slot = lock(&HOST);
    }
    let Some(h) = slot.as_mut().filter(|h| h.owner == owner) else {
        return Ok(None);
    };
    h.collect_faults();
    let playing = h.engine.state() != PlayState::Stopped;
    // The stamp follows the tick read directly: the playhead extrapolates
    // from this pair (spec §3.4).
    let tick = h.engine.position_tick();
    let host_time_ns = crate::clock::now_ns();
    Ok(Some(HostState {
        playing,
        seq: h.seq,
        tick,
        host_time_ns,
        sample_rate: h.sample_rate,
        device: h.device.clone(),
        faults: h.faults.clone(),
        report: h.report.clone(),
        rate_mismatch: h.rate_mismatch,
    }))
}

/// `(rate_mismatch, device rate)` if `owner` holds the engine.
pub fn device(owner: u64) -> Option<(bool, f64)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| (h.rate_mismatch, h.sample_rate))
}

/// Drops the host if `owner` holds it and it was opened for another
/// project rate than `project` (a format change, or its undo; Ruling 20):
/// playback stops and the next Play asks the device for the new rate. The
/// note-offs drain without the host lock, as in [`reset`].
pub fn reset_if_owner(owner: u64, project: f64) {
    let mut slot = lock(&HOST);
    if !slot
        .as_ref()
        .is_some_and(|h| h.owner == owner && !same_rate(h.project_rate, project))
    {
        return;
    }
    let mut old = slot.take();
    drop(slot);
    if let Some(h) = old.as_mut() {
        let _ = h.engine.stop();
    }
    retire(old);
}

/// Releases the engine if `owner` holds it (a session is closing).
pub fn release(owner: u64) {
    let mut slot = lock(&HOST);
    if let Some(h) = slot.as_mut() {
        if h.owner == owner {
            let _ = h.engine.stop();
            h.want_play = false;
            h.epoch = next_epoch();
            h.owner = 0;
        }
    }
}

/// The main-thread timer's plugin step for session `owner` (spec §5,
/// 3b `PluginHost::service`): with the engine when `owner` plays through
/// it, so finished loads, restarts and edits that waited reach it; else
/// the host's own `idle`. An engine error is kept as a fault (the host
/// retries once something changes).
pub fn service(owner: u64, doc: &Document, plugins: &mut PluginHost) -> IdleReport {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner && h.factory_owner == owner => {
            h.plugins_in_engine |= has_slots(doc);
            match plugins.service(&mut h.engine, doc, h.seq) {
                Ok(r) => {
                    if let Some(c) = &r.compiled {
                        h.report = report_lines(c);
                    }
                    r
                }
                Err(e) => {
                    let f = format!("plugin load: {e}");
                    if !h.faults.contains(&f) {
                        h.faults.push(f);
                    }
                    IdleReport::default()
                }
            }
        }
        _ => {
            // Not playing: slots the catalog released (a plugin enabled
            // again) still load.
            crate::recovery::before_sync();
            plugins.sync(doc);
            plugins.idle()
        }
    }
}

/// Whether `doc` has a plugin slot (an insert or a plugin instrument).
fn has_slots(doc: &Document) -> bool {
    doc.mixer().inserts().next().is_some()
        || doc.instruments().values().any(|d| {
            matches!(
                d.kind,
                core_model::document::DestKind::PluginInstrument { .. }
            )
        })
}

/// The session whose plugin host feeds the engine, if any.
pub fn factory_owner() -> Option<u64> {
    lock(&HOST)
        .as_ref()
        .map(|h| h.factory_owner)
        .filter(|o| *o != 0)
}

/// `owner`'s meters, DSP load, latency and fault-bypassed instances, or
/// `None` when it does not hold the engine.
pub fn levels(owner: u64) -> Option<crate::mixer::MixerLevels> {
    let mut slot = lock(&HOST);
    let h = slot.as_mut().filter(|h| h.owner == owner)?;
    h.faulted
        .extend(h.engine.plugin_faults().into_iter().map(|i| i.0));
    let load = h.engine.dsp_load();
    Some(crate::mixer::MixerLevels {
        meters: h
            .engine
            .meters()
            .into_iter()
            .map(|m| crate::mixer::MeterRow {
                strip: m.strip.0,
                peak_l: m.peak_l,
                peak_r: m.peak_r,
            })
            .collect(),
        dsp_load: load.last,
        dsp_peak: load.peak,
        latency_frames: h.engine.latency_frames(),
        faulted: h.faulted.iter().copied().collect(),
    })
}

/// Installs a record tap in `owner`'s engine (spec §6.2): the next block
/// that plays starts capturing. Fails only when `owner` does not hold the
/// engine or its command queue is full.
pub fn start_recording(owner: u64, req: &RecordRequest) -> Result<RecordHandles, VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => Ok(h.engine.start_recording(req)?),
        _ => Err(VizError::Engine {
            message: "this document does not hold the audio engine".into(),
        }),
    }
}

/// Stops `owner`'s transport and removes its record tap (nothing when
/// another session holds the engine: its tap went with the old engine,
/// or that session's Play removed it).
pub fn stop_recording(owner: u64) -> Result<(), VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            h.want_play = false;
            h.until = None;
            h.epoch = next_epoch();
            h.engine.stop()?;
            Ok(h.engine.stop_recording()?)
        }
        _ => Ok(()),
    }
}

/// Whether `owner`'s renderer has its transport stopped (the end of a
/// song without a loop), when it holds the engine. Right after a Play this
/// still reads stopped until the renderer applies it.
pub fn transport_stopped(owner: u64) -> Option<bool> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.state() == PlayState::Stopped)
}

/// Where `owner`'s transport is heard, when it holds the engine.
pub fn position(owner: u64) -> Option<(SeqId, Tick)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| (h.seq, h.engine.position_tick()))
}

/// `owner`'s input, when it holds the engine.
pub fn input_info(owner: u64) -> Option<(String, InputInfo)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| (h.device.clone(), h.inputs.clone()))
}

/// The MIDI input of `owner`'s engine, when it holds it.
pub fn midi_sender(owner: u64) -> Option<MidiSender> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.midi_sender())
}

/// The input routes `owner`'s engine was last given (test hook).
#[cfg(feature = "test-hooks")]
pub fn debug_inputs(owner: u64) -> Option<InputSpec> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.inputs().clone())
}
