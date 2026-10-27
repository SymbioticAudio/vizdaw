//! The control handle (non-RT): compiles documents and mixer graphs, sends
//! commands to the renderer and reads what it publishes.

use crate::backend::MidiPortInfo;
use crate::error::{BackendError, EngineError};
use crate::graph::mixer::mixer_spec;
use crate::graph::{
    EffectNode, Graph, GraphReport, InsertFactory, InsertInit, InstrumentFeed, NoInserts, NodeSpec,
    PlanError, plan,
};
use crate::midi1::{Midi1Msg, note_on_velocity, release_velocity};
use crate::pool::{Pool, RtEnv, RtPlatform, StdPlatform};
use crate::render::{
    CMD_CAPACITY, Counters, DspLoad, EngineCmd, Faults, RETIRE_CAPACITY, RenderConfig, Renderer,
    Retired, Shared, Sink,
};
use crate::schedule::targets::kinds_for;
use crate::schedule::{
    BuiltinRoute, CompileOptions, CompileReport, MAX_TARGETS, TargetIdx, TargetKind, TargetTable,
    TempoTable, compile,
};
use crate::transport::PlayState;
use core_media::stream::{DiskMode, MediaFile, StreamConfig, StreamWaiter, Streamer};
use core_model::document::{DestKind, Document};
use core_model::ids::{DestId, LaneId, MediaId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{PluginFormat, PluginSlot};
use core_model::time::Tick;
use rtrb::{Producer, RingBuffer};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// Creates sinks for new targets (control thread).
pub trait SinkFactory: Send {
    /// MIDI destinations that exist now.
    fn ports(&self) -> Vec<MidiPortInfo>;
    /// Creates the sink of target `target` (the index the engine gives
    /// it) at the stream's rate and block size.
    fn make(
        &mut self,
        target: TargetIdx,
        kind: &TargetKind,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Sink, BackendError>;
}

/// Engine settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineConfig {
    /// Stream sample rate.
    pub sample_rate: f64,
    /// Largest block the renderer accepts.
    pub max_frames: u32,
    /// Added to MIDI-out timestamps (CLI `--midi-offset-ms`).
    pub midi_offset_ns: i64,
    /// Where `builtin:synth` destinations play.
    pub route: BuiltinRoute,
    /// Render worker threads besides the callback thread (0–15; Plan
    /// Ruling 20).
    pub workers: usize,
    /// Fraction of a block the graph may take (Plan Ruling 18); `None`
    /// waits (offline).
    pub deadline: Option<f64>,
    /// Who runs disk passes (tests: Manual, serviced between blocks).
    pub disk: DiskMode,
    /// Streaming sizes (spec §5.5).
    pub stream: StreamConfig,
}

/// How long Play waits for the disk at most (spec §5.5).
pub const LOCATE_WAIT: Duration = Duration::from_millis(250);

impl EngineConfig {
    /// Defaults: 4096-frame blocks, no MIDI offset, built-in routing, no
    /// workers, no deadline.
    pub fn new(sample_rate: f64) -> EngineConfig {
        EngineConfig {
            sample_rate,
            max_frames: crate::backend::MAX_FRAMES,
            midi_offset_ns: 0,
            route: BuiltinRoute::Builtin,
            workers: 0,
            deadline: None,
            disk: DiskMode::Thread,
            stream: StreamConfig::default(),
        }
    }
}

/// A Play between [`Engine::begin_play`] and [`Engine::finish_play`].
#[derive(Debug)]
#[must_use = "a Play is sent by `Engine::finish_play`"]
pub struct PendingPlay {
    /// Where to wait for the disk (`None`: no wait).
    wait: Option<(StreamWaiter, u64)>,
    until: Option<Tick>,
}

impl PendingPlay {
    /// Waits up to [`LOCATE_WAIT`] for the disk at the frame Play starts
    /// from (spec §5.5); after that, playing rolls anyway. Needs no lock.
    pub fn wait(&self) {
        if let Some((w, at)) = &self.wait {
            let _ = w.wait_ready(*at, LOCATE_WAIT);
        }
    }

    /// True when [`PendingPlay::wait`] may wait at all.
    pub fn waits(&self) -> bool {
        self.wait.is_some()
    }
}

/// One strip's peaks since the last read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripMeter {
    /// The strip.
    pub strip: StripId,
    /// Left peak (linear).
    pub peak_l: f32,
    /// Right peak (linear).
    pub peak_r: f32,
}

/// An insert instance in the last published graph. An instance is reused
/// only for a slot with the same ID and the same plugin (format + id).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Live {
    index: usize,
    latency: u32,
    format: PluginFormat,
    plugin: String,
    /// The block size it was made for.
    max_frames: u32,
}

impl Live {
    /// True when this instance may keep serving `slot` with blocks of up
    /// to `max_frames`.
    fn serves(&self, slot: &PluginSlot, max_frames: u32) -> bool {
        self.format == slot.plugin.format
            && self.plugin == slot.plugin.id
            && self.max_frames >= max_frames
    }
}

/// What the engine knows about a target's sink.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SinkInfo {
    /// Instrument latency (`None`: not an instrument sink).
    latency: Option<u32>,
    /// Instrument outputs.
    outputs: u16,
    /// The block size the sink was made for.
    max_frames: u32,
}

/// The control handle.
pub struct Engine {
    cfg: EngineConfig,
    cmds: Producer<EngineCmd>,
    shared: Arc<Shared>,
    targets: TargetTable,
    factory: Box<dyn SinkFactory>,
    inserts: Box<dyn InsertFactory>,
    tempo: Option<TempoTable>,
    length_frames: u64,
    loop_range: Option<(u64, u64)>,
    /// The default loop (bar 1 to the end) is on: loads re-send it.
    default_loop: bool,
    /// Each target's sink.
    sinks: [SinkInfo; MAX_TARGETS],
    /// Instances to recreate at the next load (`Engine::restart_instance`).
    restart: BTreeSet<PluginInstanceId>,
    /// Targets were released and no load has published a schedule and a
    /// graph since: the published ones may still name them.
    release_unpublished: bool,
    /// Insert instances in the last published graph, by ID.
    live: BTreeMap<PluginInstanceId, Live>,
    /// Inserts made for a graph that could not be planned: kept for the
    /// next load instead of being dropped, so a refused graph does not send
    /// every new plugin back to its host (which would activate it again and
    /// ask for the same load).
    spare: BTreeMap<PluginInstanceId, (Box<dyn EffectNode>, Live)>,
    /// Meter cell of every strip of the last published graph, by strip
    /// (cells are keyed by strip, not node, so a structural edit never
    /// moves a strip's peaks to another strip).
    meter_slots: BTreeMap<StripId, usize>,
    /// The automation overrides of the last published graph.
    overrides: Option<Arc<crate::graph::OverrideSnapshot>>,
    /// Overrides another engine held, for the next published graph.
    seed: Option<Arc<crate::graph::OverrideSnapshot>>,
    /// Latency of the last published graph.
    latency: u64,
    collector_stop: Arc<AtomicBool>,
    collector_paused: Arc<AtomicBool>,
    collector: Option<JoinHandle<()>>,
    pool: Option<Pool>,
    /// The disk streamer (5a).
    streamer: Streamer,
    /// Media the streamer may read; the rest compile as offline.
    online: BTreeSet<MediaId>,
    /// Where the last Locate put the transport (Play waits for the disk
    /// there; the published position may not have caught up).
    located: Option<u64>,
    /// The loop the renderer plays outside Play Selection, or `None`.
    active_loop: Option<(u64, u64)>,
    /// Play Selection is playing (as commanded: set by `play_until`,
    /// cleared by `play` and `stop`): the transport ignores the loop, so
    /// the streamer gets none (see [`Engine::sync_stream_loop`]).
    selection: bool,
    /// The last transport command left playing (`play`/`play_until`, and
    /// no `stop` since), as this engine commanded it, not as the renderer
    /// has published yet.
    commanded_playing: bool,
    /// A Stop or Locate was sent since the last Play: the next Play waits
    /// for the disk.
    moved_since_play: bool,
    /// The streamer's (underruns, late) counters at the last `faults`.
    stream_seen: (AtomicU64, AtomicU64),
    /// What lanes listen to (spec §6.6); resolved at every load.
    input_spec: crate::input::InputSpec,
    /// Feeds the renderer's MIDI input queue.
    midi_tx: crate::input::MidiSender,
    /// The routes last sent to the renderer: a load sends them only when
    /// they change, so a load still retires just a schedule and a graph.
    inputs_sent: crate::input::InputRoutes,
    /// The destinations whose return a lane records (spec §6.7): interned
    /// at every load, so a measurement can play them (plan 5c-2 Ruling 10).
    return_dests: Vec<DestId>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("cfg", &self.cfg)
            .field("targets", &self.targets)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Creates the control handle and its renderer, starts the collector
    /// thread that frees retired schedules and graphs every 10 ms and, with
    /// `cfg.workers > 0`, the portable worker pool. Fails when a thread
    /// cannot be started (ruling O24).
    pub fn new(
        cfg: EngineConfig,
        factory: Box<dyn SinkFactory>,
    ) -> Result<(Engine, Renderer), EngineError> {
        Engine::with_platform(cfg, factory, Arc::new(StdPlatform))
    }

    /// [`Engine::new`] with platform wakers and real-time promotion for the
    /// workers (`platform-macos` passes `MacRtPlatform`).
    pub fn with_platform(
        cfg: EngineConfig,
        factory: Box<dyn SinkFactory>,
        platform: Arc<dyn RtPlatform>,
    ) -> Result<(Engine, Renderer), EngineError> {
        Engine::with_spawner(cfg, factory, platform, |b, f| b.spawn(f))
    }

    /// [`Engine::with_platform`] with the collector's thread spawn injected
    /// (tests make it fail).
    fn with_spawner(
        cfg: EngineConfig,
        factory: Box<dyn SinkFactory>,
        platform: Arc<dyn RtPlatform>,
        spawn: impl FnOnce(
            std::thread::Builder,
            Box<dyn FnOnce() + Send + 'static>,
        ) -> std::io::Result<JoinHandle<()>>,
    ) -> Result<(Engine, Renderer), EngineError> {
        let (cmd_tx, cmd_rx) = RingBuffer::<EngineCmd>::new(CMD_CAPACITY);
        let (retire_tx, mut retire_rx) = RingBuffer::<Retired>::new(RETIRE_CAPACITY);
        let shared = Arc::new(Shared::default());
        let collector_stop = Arc::new(AtomicBool::new(false));
        let collector_paused = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&collector_stop);
        let paused = Arc::clone(&collector_paused);
        const COLLECTOR: &str = "vizdaw-collector";
        let collector = spawn(
            std::thread::Builder::new().name(COLLECTOR.into()),
            Box::new(move || {
                loop {
                    while !paused.load(Ordering::Relaxed) {
                        let Ok(old) = retire_rx.pop() else {
                            break;
                        };
                        drop(old);
                    }
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }),
        )
        .map_err(|e| EngineError::Thread {
            name: COLLECTOR,
            reason: e.to_string(),
        })?;
        let workers = cfg.workers.min(MAX_WORKERS);
        let pool = if workers > 0 {
            Some(Pool::new(workers, platform, Arc::clone(&shared))?)
        } else {
            None
        };
        let (midi_tx, midi_rx) = crate::input::midi_queue();
        let (streamer, reader) =
            Streamer::new(cfg.sample_rate, cfg.stream, cfg.disk).map_err(|e| {
                EngineError::Thread {
                    name: "vizdaw-disk",
                    reason: e.to_string(),
                }
            })?;
        let renderer = Renderer::new(
            RenderConfig {
                sample_rate: cfg.sample_rate,
                max_frames: cfg.max_frames,
                midi_offset_ns: cfg.midi_offset_ns,
                deadline: cfg.deadline,
            },
            cmd_rx,
            retire_tx,
            Arc::clone(&shared),
            pool.as_ref().map(Pool::handle),
            reader,
            midi_rx,
        );
        let engine = Engine {
            cfg,
            cmds: cmd_tx,
            shared,
            targets: TargetTable::new(),
            factory,
            inserts: Box::new(NoInserts),
            tempo: None,
            length_frames: 0,
            loop_range: None,
            default_loop: false,
            sinks: [SinkInfo::default(); MAX_TARGETS],
            restart: BTreeSet::new(),
            release_unpublished: false,
            live: BTreeMap::new(),
            spare: BTreeMap::new(),
            meter_slots: BTreeMap::new(),
            overrides: None,
            seed: None,
            latency: 0,
            collector_stop,
            collector_paused,
            collector: Some(collector),
            pool,
            streamer,
            online: BTreeSet::new(),
            located: None,
            active_loop: None,
            selection: false,
            commanded_playing: false,
            moved_since_play: false,
            stream_seen: (AtomicU64::new(0), AtomicU64::new(0)),
            input_spec: crate::input::InputSpec::default(),
            midi_tx,
            inputs_sent: crate::input::InputRoutes::default(),
            return_dests: Vec::new(),
        };
        Ok((engine, renderer))
    }

    /// Replaces the insert factory (default [`NoInserts`]; tests pass
    /// `TestInserts`, 3b the plugin hosts). Affects inserts created by later
    /// loads.
    pub fn set_insert_factory(&mut self, f: Box<dyn InsertFactory>) {
        self.inserts = f;
        // Spares came from the old factory.
        self.spare.clear();
    }

    /// Hands the render workers the device's period and workgroup (Plan
    /// Ruling 22); no-op without workers.
    pub fn set_rt_env(&self, env: RtEnv) {
        if let Some(p) = &self.pool {
            p.set_env(env);
        }
    }

    /// Whether the callback thread renders graph nodes too (diagnostic
    /// hook, Plan Ruling 19).
    pub fn set_callback_helps(&self, on: bool) {
        if let Some(p) = &self.pool {
            p.set_callback_helps(on);
        }
    }

    /// Graph nodes rendered by worker threads so far (diagnostic hook:
    /// tests use it to prove workers rendered).
    pub fn worker_steps(&self) -> u64 {
        self.pool.as_ref().map_or(0, |p| p.handle().worker_steps())
    }

    /// Graphs the renderer has swapped in so far (diagnostic hook).
    pub fn graphs_applied(&self) -> u64 {
        self.shared.graphs_applied()
    }

    /// Pauses or resumes freeing retired schedules and graphs (a diagnostic
    /// hook: tests use it to fill the retire ring).
    pub fn pause_collector(&self, paused: bool) {
        self.collector_paused.store(paused, Ordering::Relaxed);
    }

    /// Settings.
    pub fn config(&self) -> EngineConfig {
        self.cfg
    }

    /// Targets created so far.
    pub fn targets(&self) -> &TargetTable {
        &self.targets
    }

    /// Compiles `seq` and the mixer graph and sends them (and sinks for new
    /// targets) to the renderer. Call again after every committed edit.
    pub fn load(&mut self, doc: &Document, seq: SeqId) -> Result<CompileReport, EngineError> {
        // Room for removing and adding every possible sink plus the graph,
        // the schedule and the loop, so no push below can fail after
        // targets were interned.
        if self.cmds.slots() < 2 * MAX_TARGETS + 4 {
            return Err(EngineError::Busy);
        }
        let opts = CompileOptions {
            sample_rate: self.cfg.sample_rate,
            route: self.cfg.route,
            ports: self.factory.ports(),
            offline_media: doc
                .media()
                .keys()
                .filter(|m| !self.online.contains(m))
                .copied()
                .collect(),
        };
        let (sr, max) = (self.cfg.sample_rate, self.block_frames());
        // Indices released by an earlier load are free only once a load
        // published both a schedule and a graph after the release.
        if !self.release_unpublished {
            self.targets.recycle();
        }
        self.release_stale_targets(doc, max)?;
        let mut new_sinks: Vec<(TargetIdx, Sink)> = Vec::new();
        let targets = &mut self.targets;
        let factory = &mut self.factory;
        let inserts = &mut self.inserts;
        let mut resolve = |kind: &TargetKind| -> Result<Result<TargetIdx, String>, EngineError> {
            if let Some(i) = targets.find(kind) {
                return Ok(Ok(i));
            }
            let next = targets.next_index().ok_or(EngineError::TooManyTargets)?;
            let made = match kind {
                TargetKind::PluginInstrument(id) => match doc.plugin_slot(*id) {
                    Some(slot) => inserts.make_instrument(slot, sr, max).map(Sink::Instrument),
                    None => Err(format!("plugin instance {id} has no slot")),
                },
                other => factory
                    .make(next, other, sr, max)
                    .map_err(|e| e.to_string()),
            };
            match made {
                Ok(sink) => {
                    let i = targets.intern(kind.clone())?;
                    new_sinks.push((i, sink));
                    Ok(Ok(i))
                }
                Err(e) => Ok(Err(e)),
            }
        };
        let compiled = compile(doc, seq, &opts, &mut resolve);
        // The targets live input plays on (spec §6.6): interned like the
        // schedule's, so thru reaches a destination the sequence never
        // plays.
        let spec = self.input_spec.clone();
        let mut midi_routes: Vec<crate::input::MidiRoute> = Vec::with_capacity(spec.midi.len());
        // Thru that cannot play is reported, not dropped (fix round 1, M1).
        let mut thru_unroutable: Vec<(LaneId, String)> = Vec::new();
        if compiled.is_ok() {
            for m in &spec.midi {
                let d = m.dest.and_then(|d| doc.instruments().get(&d));
                let kinds = match (m.dest, d) {
                    (None, _) => Vec::new(),
                    (Some(id), None) => {
                        if m.thru {
                            thru_unroutable
                                .push((m.lane, format!("destination {id} is not in the project")));
                        }
                        Vec::new()
                    }
                    (Some(_), Some(d)) => match kinds_for(&d.kind, opts.route, &opts.ports) {
                        Ok(k) => k,
                        Err(e) => {
                            if m.thru {
                                thru_unroutable.push((m.lane, e));
                            }
                            Vec::new()
                        }
                    },
                };
                let mut targets = [None, None];
                for (slot, k) in targets.iter_mut().zip(&kinds) {
                    *slot = match resolve(k) {
                        Ok(Ok(t)) => Some(t),
                        Ok(Err(e)) => {
                            if m.thru {
                                thru_unroutable.push((m.lane, e));
                            }
                            None
                        }
                        Err(e) => {
                            if m.thru {
                                thru_unroutable.push((m.lane, e.to_string()));
                            }
                            None
                        }
                    };
                }
                let dest_channel = match d.map(|d| &d.kind) {
                    Some(DestKind::MidiEndpoint { channel, .. }) => channel & 0x0F,
                    _ => 0,
                };
                midi_routes.push(crate::input::MidiRoute {
                    spec: m.clone(),
                    targets,
                    dest_channel,
                    chain: crate::input::InputChain,
                });
            }
            // The synths lanes record the return of (spec §6.7): interned
            // like thru's, so Measure can play one the sequence never plays
            // (plan 5c-2 Ruling 10). One that cannot play is left out;
            // Measure says why.
            for id in &self.return_dests {
                let Some(d) = doc.instruments().get(id) else {
                    continue;
                };
                if let Ok(kinds) = kinds_for(&d.kind, opts.route, &opts.ports) {
                    for k in &kinds {
                        let _ = resolve(k);
                    }
                }
            }
        }
        let (mut schedule, mut report) = match compiled {
            Ok(c) => c,
            Err(e) => {
                // Targets interned by this load have no sinks yet; forget
                // them (their sinks drop with `new_sinks`).
                for (i, _) in &new_sinks {
                    self.targets.forget(*i);
                }
                return Err(e);
            }
        };
        for (i, sink) in new_sinks {
            if let Some(info) = self.sinks.get_mut(usize::from(i.0)) {
                *info = SinkInfo {
                    latency: match &sink {
                        Sink::Instrument(node) => Some(node.latency()),
                        Sink::Midi(_) => None,
                    },
                    outputs: match &sink {
                        Sink::Instrument(node) => node.outputs().clamp(1, 16),
                        Sink::Midi(_) => 0,
                    },
                    max_frames: max,
                };
            }
            self.push(EngineCmd::AddSink(i, sink))?;
        }
        // Plugin-parameter lanes: the target of a plugin instrument (by
        // the slot's owner, not by routing, Plan 4d P10) and the
        // parameters the plugin declares stepped (Rulings 8, 12).
        report.automation_instrument_lanes = schedule.automation.bind_plugins(doc, &self.targets);
        let inserts = &self.inserts;
        schedule.automation.mark_stepped(|d| match d {
            crate::schedule::AutoDest::Plugin(id, p) => inserts.param_stepped(id, p),
            _ => false,
        });
        for (lane, why) in &thru_unroutable {
            report
                .warnings
                .push(format!("MIDI thru of lane {lane} not played: {why}"));
        }
        report.thru_unroutable = thru_unroutable;
        report.graph = self.compile_graph(doc)?;
        if report.graph.error.is_none() {
            // Schedule and graph both go out below: nothing published
            // still names a released index.
            self.release_unpublished = false;
            self.restart.clear();
        }
        self.streamer.set_plan(Arc::from(schedule.audio.needs()));
        // The renderer maps an explicit loop through the tempo change
        // (`EngineCmd::SetSchedule`); the streamer gets the same range.
        let old_tempo = self.tempo.replace(schedule.tempo.clone());
        if let (Some(old), Some((a, b))) = (old_tempo.as_ref(), self.active_loop) {
            let map = |f: u64| schedule.tempo.tick_to_frame(old.frame_to_tick(f));
            self.active_loop = Some((map(a), map(b)));
        }
        self.length_frames = schedule.length_frames;
        self.loop_range = schedule.loop_range;
        self.push(EngineCmd::SetSchedule(Box::new(schedule)))?;
        let monitor: Box<[crate::input::MonitorRoute]> = spec
            .monitor
            .iter()
            .take(crate::input::MAX_MONITOR_ROUTES)
            .copied()
            .collect();
        let routes = crate::input::InputRoutes {
            midi: midi_routes.into_boxed_slice(),
            monitor,
        };
        if routes != self.inputs_sent {
            self.inputs_sent = routes.clone();
            self.push(EngineCmd::SetInputs(Box::new(routes)))?;
        }
        if self.default_loop {
            // The length may have changed; the mapped old range would not
            // follow it.
            self.active_loop = self.loop_range;
            self.push(EngineCmd::SetLoop(self.loop_range))?;
        }
        self.sync_stream_loop();
        Ok(report)
    }

    /// Releases targets whose sink must go before this load: plugin
    /// instruments no longer in `doc` or marked for restart, and instrument
    /// sinks made for blocks smaller than `max` (a later
    /// `Renderer::prepare` grew the block size; Plan Ruling 5).
    fn release_stale_targets(&mut self, doc: &Document, max: u32) -> Result<(), EngineError> {
        let stale: Vec<TargetIdx> = self
            .targets
            .iter()
            .filter(|(i, kind)| {
                let info = self.sinks.get(usize::from(i.0)).copied().unwrap_or_default();
                let small = info.latency.is_some() && info.max_frames < max;
                match kind {
                    TargetKind::PluginInstrument(id) => {
                        small
                            || self.restart.contains(id)
                            || !doc.instruments().values().any(|d| {
                                matches!(&d.kind, DestKind::PluginInstrument { slot } if slot.id == *id)
                            })
                    }
                    TargetKind::BuiltinSynth => small,
                    TargetKind::MidiVirtualSource | TargetKind::MidiDestination(_) => false,
                }
            })
            .map(|(i, _)| i)
            .collect();
        for i in stale {
            self.release_unpublished = true;
            self.push(EngineCmd::RemoveSink(i))?;
            self.targets.release(i);
            if let Some(info) = self.sinks.get_mut(usize::from(i.0)) {
                *info = SinkInfo::default();
            }
        }
        Ok(())
    }

    /// Makes the next load recreate instance `id` (an insert or a plugin
    /// instrument) instead of keeping the running one: its old processor
    /// is retired with the graph or sink it lived in. The plugin host calls
    /// this when a plugin asks to be restarted (a latency change).
    pub fn restart_instance(&mut self, id: PluginInstanceId) {
        self.restart.insert(id);
    }

    /// Meter cells for `strips`: a strip keeps its cell; a new strip gets
    /// the lowest free one, cleared first (Plan Ruling 13).
    fn assign_meters(&self, strips: &[(usize, StripId)]) -> BTreeMap<StripId, usize> {
        let mut out = BTreeMap::new();
        for (_, id) in strips {
            if let Some(c) = self.meter_slots.get(id) {
                out.insert(*id, *c);
            }
        }
        let mut used: std::collections::BTreeSet<usize> = out.values().copied().collect();
        let mut next = 0usize;
        for (_, id) in strips {
            if out.contains_key(id) {
                continue;
            }
            while used.contains(&next) {
                next += 1;
            }
            if let Some(m) = self.shared.meter(next) {
                m.take();
                out.insert(*id, next);
                used.insert(next);
            }
        }
        out
    }

    /// Builds and publishes the mixer graph. A graph that cannot be planned
    /// is reported (and its fault raised) and the previous graph keeps
    /// playing (spec §6).
    fn compile_graph(&mut self, doc: &Document) -> Result<GraphReport, EngineError> {
        let (sr, max) = (self.cfg.sample_rate, self.block_frames());
        let instruments: Vec<InstrumentFeed> = self
            .targets
            .iter()
            .filter_map(|(i, k)| {
                let info = self.sinks.get(usize::from(i.0))?;
                let lat = info.latency?;
                match k {
                    TargetKind::BuiltinSynth => Some(InstrumentFeed::builtin(i, lat)),
                    TargetKind::PluginInstrument(id) => {
                        let dest = doc.instruments().values().find(|d| {
                            matches!(&d.kind, DestKind::PluginInstrument { slot } if slot.id == *id)
                        })?;
                        Some(InstrumentFeed::plugin(i, lat, info.outputs, dest.id))
                    }
                    _ => None,
                }
            })
            .collect();
        let mut fresh: BTreeMap<PluginInstanceId, (Box<dyn EffectNode>, Live)> = BTreeMap::new();
        let mut adopted: BTreeMap<PluginInstanceId, Live> = BTreeMap::new();
        let mut placeholders: Vec<(PluginInstanceId, String)> = Vec::new();
        let live = &self.live;
        let restart = &self.restart;
        let inserts = &mut self.inserts;
        // Spares of a refused graph that still fit are used first; the
        // others (and those whose slot is gone) drop below.
        let mut spare = std::mem::take(&mut self.spare);
        let mut latency = |slot: &PluginSlot| -> u32 {
            if let Some(l) = live
                .get(&slot.id)
                .filter(|l| l.serves(slot, max) && !restart.contains(&slot.id))
            {
                adopted.insert(slot.id, l.clone());
                return l.latency;
            }
            if let Some((node, info)) = spare.remove(&slot.id) {
                if info.serves(slot, max) && !restart.contains(&slot.id) {
                    let lat = node.latency();
                    fresh.insert(
                        slot.id,
                        (
                            node,
                            Live {
                                latency: lat,
                                ..info
                            },
                        ),
                    );
                    return lat;
                }
            }
            match inserts.make(slot, sr, max) {
                Ok(node) => {
                    let lat = node.latency();
                    let info = Live {
                        index: 0,
                        latency: lat,
                        format: slot.plugin.format,
                        plugin: slot.plugin.id.clone(),
                        max_frames: max,
                    };
                    fresh.insert(slot.id, (node, info));
                    lat
                }
                Err(why) => {
                    placeholders.push((slot.id, why));
                    0
                }
            }
        };
        let mut mg = mixer_spec(doc, &instruments, &mut latency);
        drop(spare);
        let slots = self.assign_meters(&mg.strips);
        for (node, strip) in &mg.strips {
            if let (Some(NodeSpec::Strip(s)), Some(slot)) =
                (mg.spec.nodes.get_mut(*node), slots.get(strip))
            {
                s.meter = Some(*slot);
            }
        }
        let mut report = GraphReport {
            nodes: mg.spec.nodes.len(),
            latency_frames: 0,
            placeholders,
            warnings: mg.warnings,
            error: None,
        };
        let max_delay = (2.0 * sr).round().max(0.0) as u64;
        let p = match plan(&mg.spec, max_delay) {
            Ok(p) => p,
            Err(e) => {
                self.shared.raise(match e {
                    PlanError::TooLarge { .. } => Faults::GRAPH_TOO_LARGE,
                    PlanError::LatencyTooLarge { .. } => Faults::LATENCY_TOO_LARGE,
                    PlanError::Cycle | PlanError::BadIndex { .. } => Faults::GRAPH_INVALID,
                });
                // The planner knows only `max_delay`; name the engine's
                // limit (spec §6).
                report.error = Some(match e {
                    PlanError::LatencyTooLarge { frames } => format!(
                        "a latency or compensation delay of {frames} frames exceeds the 2 s limit ({max_delay} frames)"
                    ),
                    other => other.to_string(),
                });
                // Nothing was published: keep the new inserts for the next
                // load.
                self.spare = fresh;
                return Ok(report);
            }
        };
        let mut next_live: BTreeMap<PluginInstanceId, Live> = BTreeMap::new();
        let mut graph = Graph::build(&mg.spec, &p, max, |idx, ins| {
            let Some(id) = ins.instance else {
                return InsertInit::Placeholder;
            };
            if let Some(l) = adopted.remove(&id) {
                let old = l.index;
                next_live.insert(id, Live { index: idx, ..l });
                return InsertInit::Adopt(old);
            }
            match fresh.remove(&id) {
                Some((node, info)) => {
                    next_live.insert(id, Live { index: idx, ..info });
                    InsertInit::Fresh(node)
                }
                None => InsertInit::Placeholder,
            }
        });
        graph.set_controls(&mg.controls);
        if let Some(seed) = &self.seed {
            graph.seed_overrides(seed);
        }
        let overrides = graph.override_snapshot();
        report.latency_frames = graph.latency();
        self.push(EngineCmd::SetGraph(Box::new(graph)))?;
        self.seed = None;
        self.overrides = Some(overrides);
        self.live = next_live;
        self.latency = report.latency_frames;
        self.meter_slots = slots;
        Ok(report)
    }

    /// The automation overrides the last published graph held at its last
    /// commit (Plan 4d Ruling 13); stable once the engine's output stopped.
    pub fn overrides(&self) -> Option<Arc<crate::graph::OverrideSnapshot>> {
        self.overrides.clone()
    }

    /// Starts the next published graph from `snap`, another engine's
    /// overrides (a device rebuild), for the controls whose static value
    /// is unchanged.
    pub fn seed_overrides(&mut self, snap: Arc<crate::graph::OverrideSnapshot>) {
        self.seed = Some(snap);
    }

    /// The block size graphs, inserts and sinks are built for: the
    /// configured size, or the renderer's prepared size when a host grew it
    /// (`Renderer::prepare`). Plugin hosts activate instances for this
    /// size.
    pub fn block_frames(&self) -> u32 {
        self.cfg.max_frames.max(self.shared.prepared_frames())
    }

    fn push(&mut self, cmd: EngineCmd) -> Result<(), EngineError> {
        self.cmds.push(cmd).map_err(|_| EngineError::Busy)
    }

    /// Sends a note on (`on`) or off for `pitch` straight to the targets
    /// `dest` plays on, without touching the document (editor audition,
    /// spec §3.2). Returns false when this engine has none of them (the
    /// loaded sequence does not use that target) or `dest` is unknown.
    pub fn audition(
        &mut self,
        doc: &Document,
        dest: DestId,
        pitch: u8,
        velocity: u16,
        on: bool,
    ) -> Result<bool, EngineError> {
        let Some(d) = doc.instruments().get(&dest) else {
            return Ok(false);
        };
        let channel = match &d.kind {
            DestKind::MidiEndpoint { channel, .. } => channel & 0x0F,
            DestKind::PluginInstrument { .. } => 0,
        };
        let Ok(kinds) = kinds_for(&d.kind, self.cfg.route, &self.factory.ports()) else {
            return Ok(false);
        };
        let pitch = pitch & 0x7F;
        let msg = if on {
            Midi1Msg::three(0x90 | channel, pitch, note_on_velocity(velocity))
        } else {
            Midi1Msg::three(0x80 | channel, pitch, release_velocity(velocity))
        };
        let mut sent = false;
        for k in &kinds {
            if let Some(t) = self.targets.find(k) {
                self.push(EngineCmd::Audition(t, msg))?;
                sent = true;
            }
        }
        Ok(sent)
    }

    /// Tells the streamer the loop the transport plays: none during Play
    /// Selection (the transport ignores the loop then), else the stored
    /// one. Every path that changes the loop or the selection goes
    /// through here (fix round 1, I1).
    fn sync_stream_loop(&self) {
        self.streamer.set_loop(if self.selection {
            None
        } else {
            self.active_loop
        });
    }

    /// Where Play's disk wait (spec §5.5) must look: the frame Play
    /// starts from, or `None` when the wait is skipped: only while
    /// playing on as this engine commanded it (a Play, no Stop or Locate
    /// since) and the renderer agrees; the published state alone lags the
    /// commands just sent (fix round 1, I2).
    fn prime_at(&mut self, until: Option<u64>) -> Option<u64> {
        let located = self.located.take();
        if self.commanded_playing && !self.moved_since_play && self.state() == PlayState::Playing {
            return None;
        }
        let at = located.unwrap_or_else(|| self.transport_frames());
        Some(if at >= self.length_frames && until.is_none() {
            0
        } else {
            at
        })
    }

    /// Starts playback (from the start when at or past the end), waiting
    /// up to [`LOCATE_WAIT`] for the disk first.
    pub fn play(&mut self) -> Result<(), EngineError> {
        let p = self.begin_play(None);
        p.wait();
        self.finish_play(p)
    }

    /// Starts playback that stops at `tick` of the loaded sequence, the
    /// loop ignored (Play Selection, plan 4f Ruling 9).
    pub fn play_until(&mut self, tick: Tick) -> Result<(), EngineError> {
        let p = self.begin_play(Some(tick));
        p.wait();
        self.finish_play(p)
    }

    /// The first half of [`Engine::play`] (`until`: [`Engine::play_until`])
    /// for a host that must not wait for the disk under its own lock
    /// (plan 5a Task 10): sets the loop the streamer plans for and says
    /// where to wait. The caller runs [`PendingPlay::wait`] without the
    /// lock, then [`Engine::finish_play`], unless a Stop, another Play or
    /// a new engine came meanwhile (then it drops it).
    pub fn begin_play(&mut self, until: Option<Tick>) -> PendingPlay {
        // Play Selection: the transport ignores the loop until the next
        // plain Play.
        self.selection = until.is_some();
        self.sync_stream_loop();
        let frame = until.map(|t| self.tempo.as_ref().map_or(0, |m| m.tick_to_frame(t)));
        let at = self.prime_at(frame);
        PendingPlay {
            wait: at.map(|a| (self.streamer.waiter(), a)),
            until,
        }
    }

    /// Sends the Play [`Engine::begin_play`] prepared.
    pub fn finish_play(&mut self, p: PendingPlay) -> Result<(), EngineError> {
        let until = p
            .until
            .map(|t| self.tempo.as_ref().map_or(0, |m| m.tick_to_frame(t)));
        self.push(EngineCmd::Play { until })?;
        self.commanded_playing = true;
        self.moved_since_play = false;
        Ok(())
    }

    /// Stops playback; sounding notes get note-offs.
    pub fn stop(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Stop)?;
        // `located` is kept until a Play consumes it: a Locate made while
        // playing was applied there, so priming there is harmless.
        self.commanded_playing = false;
        self.moved_since_play = true;
        self.selection = false;
        self.sync_stream_loop();
        Ok(())
    }

    /// Moves to a schedule frame (the streamer starts filling there).
    pub fn locate_frame(&mut self, frame: u64) -> Result<(), EngineError> {
        self.streamer.hint(frame);
        self.located = Some(frame);
        self.push(EngineCmd::Locate(frame))?;
        self.moved_since_play = true;
        Ok(())
    }

    /// Moves to a tick of the loaded sequence.
    pub fn locate_tick(&mut self, tick: Tick) -> Result<(), EngineError> {
        let frame = self.tempo.as_ref().map_or(0, |t| t.tick_to_frame(tick));
        self.locate_frame(frame)
    }

    /// Loops bar 1 to the end of the loaded sequence, or stops looping.
    pub fn set_loop(&mut self, on: bool) -> Result<(), EngineError> {
        let range = if on { self.loop_range } else { None };
        self.push(EngineCmd::SetLoop(range))?;
        self.default_loop = on;
        self.active_loop = range;
        self.sync_stream_loop();
        Ok(())
    }

    /// Loops an explicit frame range, or stops looping.
    pub fn set_loop_range(&mut self, range: Option<(u64, u64)>) -> Result<(), EngineError> {
        self.push(EngineCmd::SetLoop(range))?;
        self.default_loop = false;
        self.active_loop = range;
        self.sync_stream_loop();
        Ok(())
    }

    /// Frame of the transport as heard (schedule frame minus the graph
    /// latency).
    pub fn position_frames(&self) -> u64 {
        self.shared.position()
    }

    /// Tick of the transport as heard, through the last loaded schedule's
    /// tempo table.
    pub fn position_tick(&self) -> Tick {
        let f = self.shared.position();
        self.tempo
            .as_ref()
            .map_or(Tick::ZERO, |t| t.frame_to_tick(f))
    }

    /// Schedule frame of the transport, not reduced by the graph latency:
    /// where a rebuilt engine locates to continue (a device reopen).
    pub fn transport_frames(&self) -> u64 {
        self.shared.transport_position()
    }

    /// [`Engine::transport_frames`] as a tick of the loaded sequence.
    pub fn transport_tick(&self) -> Tick {
        let f = self.shared.transport_position();
        self.tempo
            .as_ref()
            .map_or(Tick::ZERO, |t| t.frame_to_tick(f))
    }

    /// Frame of the loaded sequence's end.
    pub fn length_frames(&self) -> u64 {
        self.length_frames
    }

    /// Latency `L` of the last published graph, in frames.
    pub fn latency_frames(&self) -> u64 {
        self.latency
    }

    /// Transport state.
    pub fn state(&self) -> PlayState {
        self.shared.state()
    }

    /// Reads and clears the fault bits.
    /// The streamer's counters add `STREAM_UNDERRUN` / `LATE_START` when
    /// they grew since the last call (lock-free: never waits for a disk
    /// pass).
    pub fn faults(&self) -> Faults {
        let mut f = self.shared.take_faults();
        let c = self.streamer.counters();
        let (u, l) = (&self.stream_seen.0, &self.stream_seen.1);
        if c.underruns > u.swap(c.underruns, Ordering::Relaxed) {
            f = Faults(f.0 | Faults::STREAM_UNDERRUN.0);
        }
        if c.late > l.swap(c.late, Ordering::Relaxed) {
            f = Faults(f.0 | Faults::LATE_START.0);
        }
        f
    }

    /// The media the streamer may read (online items with their CAF
    /// paths); the others compile as offline at the next `load`.
    pub fn set_media(&mut self, files: Vec<MediaFile>) {
        self.online = files.iter().map(|f| f.id).collect();
        self.streamer.set_media(files);
    }

    /// The streamer (tests service it in Manual mode).
    pub fn streamer(&self) -> &Streamer {
        &self.streamer
    }

    /// Media whose reads failed since the last call (each once).
    pub fn take_stream_errors(&self) -> Vec<(MediaId, String)> {
        self.streamer.take_errors()
    }

    /// Mutes every region (the device refused the project rate) or
    /// unmutes them.
    pub fn set_regions_muted(&self, on: bool) {
        self.shared.set_regions_muted(on);
    }

    /// Current counters.
    pub fn counters(&self) -> Counters {
        self.shared.counters()
    }

    /// True once after playback reached the end and the tail finished.
    pub fn take_ended(&self) -> bool {
        self.shared.take_ended()
    }

    /// Peak meters of every strip of the last published graph since the
    /// last call (read and cleared).
    pub fn meters(&self) -> Vec<StripMeter> {
        self.meter_slots
            .iter()
            .filter_map(|(strip, slot)| {
                let (l, r) = self.shared.meter(*slot)?.take();
                Some(StripMeter {
                    strip: *strip,
                    peak_l: l,
                    peak_r: r,
                })
            })
            .collect()
    }

    /// DSP load of the last block and the peak since the last call.
    pub fn dsp_load(&self) -> DspLoad {
        self.shared.take_load()
    }

    /// Instances bypassed for faults since the last call (read and
    /// cleared).
    pub fn plugin_faults(&self) -> Vec<PluginInstanceId> {
        self.shared.take_plugin_faults()
    }

    /// Installs a record tap (spec §6.2): from the next block that plays,
    /// the renderer captures input and MIDI for it. Replaces a tap already
    /// installed. Returns the control side.
    pub fn start_recording(
        &mut self,
        req: &crate::record::RecordRequest,
    ) -> Result<crate::record::RecordHandles, EngineError> {
        let (tap, handles) = crate::record::tap(req, self.latency);
        self.push(EngineCmd::Record(Some(Box::new(tap))))?;
        Ok(handles)
    }

    /// Removes the record tap: the block that applies this captures no
    /// more, and the tap's status reads closed.
    pub fn stop_recording(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Record(None))
    }

    /// What lanes listen to (spec §6.6); the next [`Engine::load`]
    /// resolves and publishes it.
    pub fn set_inputs(&mut self, spec: crate::input::InputSpec) {
        self.input_spec = spec;
    }

    /// What lanes listen to, as last set.
    pub fn inputs(&self) -> &crate::input::InputSpec {
        &self.input_spec
    }

    /// Feeds this engine's MIDI input queue (CoreMIDI sources,
    /// `midi.inject`).
    pub fn midi_sender(&self) -> crate::input::MidiSender {
        self.midi_tx.clone()
    }

    /// The destinations whose return the loaded sequence's lanes record
    /// (spec §6.7); the next [`Engine::load`] interns their targets.
    pub fn set_return_dests(&mut self, dests: Vec<DestId>) {
        self.return_dests = dests;
    }

    /// Starts measuring `dest`'s return latency (spec §6.7) on device
    /// input channels `inputs` (its return lane's): from the next block
    /// the renderer plays the notes to the synth and watches the input,
    /// whether or not the transport plays. Replaces a measurement already
    /// installed. `Ok(Err(why))` when `dest` cannot play here: it is not
    /// in `doc`, no MIDI destination has its name, or no load interned it
    /// (no lane of the loaded sequence records its return). It does not
    /// refuse a destination that is not external (the built-in synth, a
    /// plugin instrument) or input channels the device lacks: the caller
    /// checks those (plan 5c-2 Ruling 2; Task 9). `trim` is the recording
    /// latency trim in frames: each note's window starts its heard run
    /// plus the input delay less `trim` later (Ruling 8; final review I1).
    pub fn start_measure(
        &mut self,
        doc: &Document,
        dest: DestId,
        inputs: Vec<u16>,
        trim: i64,
    ) -> Result<Result<Arc<crate::measure::MeasureStatus>, String>, EngineError> {
        let Some(d) = doc.instruments().get(&dest) else {
            return Ok(Err(format!("destination {dest} is not in the project")));
        };
        let channel = match &d.kind {
            DestKind::MidiEndpoint { channel, .. } => channel & 0x0F,
            DestKind::PluginInstrument { .. } => 0,
        };
        let kinds = match kinds_for(&d.kind, self.cfg.route, &self.factory.ports()) {
            Ok(k) => k,
            Err(e) => return Ok(Err(e)),
        };
        let Some(target) = kinds.iter().find_map(|k| self.targets.find(k)) else {
            return Ok(Err(format!(
                "{} is not loaded: no lane of the loaded sequence records its return",
                d.name
            )));
        };
        let req = crate::measure::MeasureRequest {
            target,
            channel,
            inputs,
            trim,
        };
        let (tap, status) = crate::measure::tap(&req, self.cfg.sample_rate);
        self.push(EngineCmd::Measure(Some(Box::new(tap))))?;
        Ok(Ok(status))
    }

    /// Removes the measurement: the block that applies this sends its
    /// sounding note's off, and its status reads closed.
    pub fn stop_measure(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Measure(None))
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.collector_stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.collector.take() {
            let _ = h.join();
        }
    }
}

/// Finds a sequence by name.
pub fn find_sequence(doc: &Document, name: &str) -> Result<SeqId, EngineError> {
    doc.sequences()
        .values()
        .find(|s| s.name == name)
        .map(|s| s.id)
        .ok_or_else(|| EngineError::UnknownSequence(name.to_string()))
}

/// Most render workers an engine starts, whatever the config asks for.
const MAX_WORKERS: usize = 15;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offline::OfflineSinks;

    #[test]
    fn new_fails_when_the_collector_thread_cannot_start() {
        let r = Engine::with_spawner(
            EngineConfig::new(48_000.0),
            Box::new(OfflineSinks::new(4)),
            Arc::new(StdPlatform),
            |_, _| Err(std::io::Error::other("no threads")),
        );
        match r {
            Err(EngineError::Thread { name, .. }) => assert_eq!(name, "vizdaw-collector"),
            other => panic!("expected a thread error, got {other:?}"),
        }
    }

    #[test]
    fn workers_are_clamped() {
        let mut cfg = EngineConfig::new(48_000.0);
        cfg.workers = 1000;
        let (e, _r) = Engine::new(cfg, Box::new(OfflineSinks::new(4))).unwrap();
        assert_eq!(e.pool.as_ref().unwrap().handle().workers(), MAX_WORKERS);
    }

    #[test]
    fn new_succeeds_normally() {
        let (engine, _r) =
            Engine::new(EngineConfig::new(48_000.0), Box::new(OfflineSinks::new(4))).unwrap();
        assert!(engine.targets().is_empty());
    }
}
