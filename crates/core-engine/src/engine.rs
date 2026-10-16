//! The control handle (non-RT): compiles documents and mixer graphs, sends
//! commands to the renderer and reads what it publishes.

use crate::backend::MidiPortInfo;
use crate::error::{BackendError, EngineError};
use crate::graph::mixer::mixer_spec;
use crate::graph::{
    EffectNode, Graph, GraphReport, InsertFactory, InsertInit, NoInserts, PlanError, plan,
};
use crate::pool::{Pool, RtEnv, RtPlatform, StdPlatform};
use crate::render::{
    CMD_CAPACITY, Counters, DspLoad, EngineCmd, Faults, RETIRE_CAPACITY, RenderConfig, Renderer,
    Retired, Shared, Sink,
};
use crate::schedule::{
    BuiltinRoute, CompileOptions, CompileReport, MAX_TARGETS, TargetIdx, TargetKind, TargetTable,
    TempoTable, compile,
};
use crate::transport::PlayState;
use core_model::document::Document;
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::{PluginFormat, PluginSlot};
use core_model::time::Tick;
use rtrb::{Producer, RingBuffer};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
}

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
        }
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
}

impl Live {
    fn serves(&self, slot: &PluginSlot) -> bool {
        self.format == slot.plugin.format && self.plugin == slot.plugin.id
    }
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
    /// Latency of each target's instrument sink.
    instrument_latency: [Option<u32>; MAX_TARGETS],
    /// Insert instances in the last published graph, by ID.
    live: BTreeMap<PluginInstanceId, Live>,
    /// `(node, strip)` of the last published graph.
    strip_nodes: Vec<(usize, StripId)>,
    /// Latency of the last published graph.
    latency: u64,
    collector_stop: Arc<AtomicBool>,
    collector_paused: Arc<AtomicBool>,
    collector: Option<JoinHandle<()>>,
    pool: Option<Pool>,
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
        let pool = if cfg.workers > 0 {
            Some(Pool::new(cfg.workers, platform, Arc::clone(&shared))?)
        } else {
            None
        };
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
            instrument_latency: [None; MAX_TARGETS],
            live: BTreeMap::new(),
            strip_nodes: Vec::new(),
            latency: 0,
            collector_stop,
            collector_paused,
            collector: Some(collector),
            pool,
        };
        Ok((engine, renderer))
    }

    /// Replaces the insert factory (default [`NoInserts`]; tests pass
    /// `TestInserts`, 3b the plugin hosts). Affects inserts created by later
    /// loads.
    pub fn set_insert_factory(&mut self, f: Box<dyn InsertFactory>) {
        self.inserts = f;
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
        // Room for every possible new sink plus the graph, the schedule and
        // the loop, so no push below can fail after targets were interned.
        if self.cmds.slots() < MAX_TARGETS + 3 {
            return Err(EngineError::Busy);
        }
        let opts = CompileOptions {
            sample_rate: self.cfg.sample_rate,
            route: self.cfg.route,
            ports: self.factory.ports(),
        };
        let (sr, max) = (self.cfg.sample_rate, self.block_frames());
        let before = self.targets.len();
        let mut new_sinks: Vec<(TargetIdx, Sink)> = Vec::new();
        let targets = &mut self.targets;
        let factory = &mut self.factory;
        let mut resolve = |kind: &TargetKind| -> Result<Result<TargetIdx, String>, EngineError> {
            if let Some(i) = targets.find(kind) {
                return Ok(Ok(i));
            }
            if targets.len() >= MAX_TARGETS {
                return Err(EngineError::TooManyTargets);
            }
            // `intern` appends, so the new target gets the next index.
            let next = u8::try_from(targets.len()).map_err(|_| EngineError::TooManyTargets)?;
            match factory.make(TargetIdx(next), kind, sr, max) {
                Ok(sink) => {
                    let i = targets.intern(kind.clone())?;
                    new_sinks.push((i, sink));
                    Ok(Ok(i))
                }
                Err(e) => Ok(Err(e.to_string())),
            }
        };
        let compiled = compile(doc, seq, &opts, &mut resolve);
        let (schedule, mut report) = match compiled {
            Ok(c) => c,
            Err(e) => {
                // Targets interned by this load have no sinks yet; forget
                // them (their sinks drop with `new_sinks`).
                self.targets.truncate(before);
                return Err(e);
            }
        };
        for (i, sink) in new_sinks {
            if let (Sink::Instrument(node), Some(l)) =
                (&sink, self.instrument_latency.get_mut(usize::from(i.0)))
            {
                *l = Some(node.latency());
            }
            self.push(EngineCmd::AddSink(i, sink))?;
        }
        report.graph = self.compile_graph(doc)?;
        self.tempo = Some(schedule.tempo.clone());
        self.length_frames = schedule.length_frames;
        self.loop_range = schedule.loop_range;
        self.push(EngineCmd::SetSchedule(Box::new(schedule)))?;
        if self.default_loop {
            // The length may have changed; the mapped old range would not
            // follow it.
            self.push(EngineCmd::SetLoop(self.loop_range))?;
        }
        Ok(report)
    }

    /// Builds and publishes the mixer graph. A graph that cannot be planned
    /// is reported (and its fault raised) and the previous graph keeps
    /// playing (spec §6).
    fn compile_graph(&mut self, doc: &Document) -> Result<GraphReport, EngineError> {
        let (sr, max) = (self.cfg.sample_rate, self.block_frames());
        let instruments: Vec<(TargetIdx, u32)> = self
            .targets
            .iter()
            .filter(|(_, k)| **k == TargetKind::BuiltinSynth)
            .filter_map(|(i, _)| {
                let lat = self
                    .instrument_latency
                    .get(usize::from(i.0))
                    .copied()
                    .flatten()?;
                Some((i, lat))
            })
            .collect();
        let mut fresh: BTreeMap<PluginInstanceId, (Box<dyn EffectNode>, Live)> = BTreeMap::new();
        let mut adopted: BTreeMap<PluginInstanceId, Live> = BTreeMap::new();
        let mut placeholders: Vec<(PluginInstanceId, String)> = Vec::new();
        let live = &self.live;
        let inserts = &mut self.inserts;
        let mut latency = |slot: &PluginSlot| -> u32 {
            if let Some(l) = live.get(&slot.id).filter(|l| l.serves(slot)) {
                adopted.insert(slot.id, l.clone());
                return l.latency;
            }
            match inserts.make(slot, sr, max) {
                Ok(node) => {
                    let lat = node.latency();
                    let info = Live {
                        index: 0,
                        latency: lat,
                        format: slot.plugin.format,
                        plugin: slot.plugin.id.clone(),
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
        let mg = mixer_spec(doc, &instruments, &mut latency);
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
                return Ok(report);
            }
        };
        let mut next_live: BTreeMap<PluginInstanceId, Live> = BTreeMap::new();
        let graph = Graph::build(&mg.spec, &p, max, |idx, ins| {
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
        report.latency_frames = graph.latency();
        self.push(EngineCmd::SetGraph(Box::new(graph)))?;
        self.live = next_live;
        self.latency = report.latency_frames;
        self.strip_nodes = mg.strips;
        Ok(report)
    }

    /// The block size graphs, inserts and sinks are built for: the
    /// configured size, or the renderer's prepared size when a host grew it
    /// (`Renderer::prepare`).
    fn block_frames(&self) -> u32 {
        self.cfg.max_frames.max(self.shared.prepared_frames())
    }

    fn push(&mut self, cmd: EngineCmd) -> Result<(), EngineError> {
        self.cmds.push(cmd).map_err(|_| EngineError::Busy)
    }

    /// Starts playback (from the start when at or past the end).
    pub fn play(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Play)
    }

    /// Stops playback; sounding notes get note-offs.
    pub fn stop(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Stop)
    }

    /// Moves to a schedule frame.
    pub fn locate_frame(&mut self, frame: u64) -> Result<(), EngineError> {
        self.push(EngineCmd::Locate(frame))
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
        Ok(())
    }

    /// Loops an explicit frame range, or stops looping.
    pub fn set_loop_range(&mut self, range: Option<(u64, u64)>) -> Result<(), EngineError> {
        self.push(EngineCmd::SetLoop(range))?;
        self.default_loop = false;
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
    pub fn faults(&self) -> Faults {
        self.shared.take_faults()
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
        self.strip_nodes
            .iter()
            .filter_map(|(node, strip)| {
                let (l, r) = self.shared.meter(*node)?.take();
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
    fn new_succeeds_normally() {
        let (engine, _r) =
            Engine::new(EngineConfig::new(48_000.0), Box::new(OfflineSinks::new(4))).unwrap();
        assert!(engine.targets().is_empty());
    }
}
