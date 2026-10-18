//! The real-time renderer and the state it publishes.

use crate::backend::{BlockInfo, BlockTime, MidiOut, StereoBuf};
use crate::graph::exec::{Exec, RunResult, SinkTable};
use crate::graph::{Graph, MAX_INSERTS, MAX_NODES};
use crate::midi1::Midi1Msg;
use crate::notes::ActiveNotes;
use crate::pool::PoolShared;
use crate::schedule::automation::StripParam;
use crate::schedule::{AutoDest, MAX_AUTO_LANES, MAX_TARGETS, MIN_RUN, Schedule, TargetIdx};
use crate::transport::{PlayState, Step, Transport};
use core_model::ids::PluginInstanceId;
use core_model::mixer::ParamId;
use rtrb::{Consumer, Producer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Real-time fault bits, cleared when read by [`crate::engine::Engine::faults`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Faults(pub u32);

impl Faults {
    /// A block was larger than the prepared size, missed its deadline or
    /// found stragglers running (rendered as silence).
    pub const XRUN: Faults = Faults(1);
    /// The master bus was clamped at ±1.0.
    pub const CLIPPED: Faults = Faults(2);
    /// A MIDI output queue was full; a message was dropped.
    pub const MIDI_DROPPED: Faults = Faults(4);
    /// A schedule or graph swap waited a block because the retire ring was
    /// full.
    pub const SWAP_DEFERRED: Faults = Faults(8);
    /// The audio callback panicked; output is silent.
    pub const PANIC: Faults = Faults(16);
    /// A recompiled graph exceeded 1024 nodes or 4096 inserts; the
    /// previous graph keeps playing.
    pub const GRAPH_TOO_LARGE: Faults = Faults(32);
    /// A recompiled graph needs more than 2 s of delay on an edge; the
    /// previous graph keeps playing.
    pub const LATENCY_TOO_LARGE: Faults = Faults(64);
    /// An insert produced 16 faulty blocks in a row and was bypassed
    /// (`Engine::plugin_faults` names it).
    pub const PLUGIN_FAULT: Faults = Faults(128);
    /// A render worker could not join the device's workgroup.
    pub const WORKGROUP_JOIN_FAILED: Faults = Faults(256);
    /// A render worker could not get real-time scheduling.
    pub const THREAD_POLICY_FAILED: Faults = Faults(512);
    /// A recompiled graph was invalid (cycle or bad index); the previous
    /// graph keeps playing.
    pub const GRAPH_INVALID: Faults = Faults(1024);

    /// True when every bit of `other` is set.
    pub fn contains(self, other: Faults) -> bool {
        self.0 & other.0 == other.0
    }

    /// True when no bit is set.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Names of the set bits.
    pub fn names(self) -> Vec<&'static str> {
        [
            (Faults::XRUN, "xrun"),
            (Faults::CLIPPED, "clipped"),
            (Faults::MIDI_DROPPED, "midi-dropped"),
            (Faults::SWAP_DEFERRED, "swap-deferred"),
            (Faults::PANIC, "panic"),
            (Faults::GRAPH_TOO_LARGE, "graph-too-large"),
            (Faults::LATENCY_TOO_LARGE, "latency-too-large"),
            (Faults::PLUGIN_FAULT, "plugin-fault"),
            (Faults::WORKGROUP_JOIN_FAILED, "workgroup-join-failed"),
            (Faults::THREAD_POLICY_FAILED, "thread-policy-failed"),
            (Faults::GRAPH_INVALID, "graph-invalid"),
        ]
        .into_iter()
        .filter(|(f, _)| self.contains(*f))
        .map(|(_, n)| n)
        .collect()
    }
}

/// Counters published by the renderer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    /// MIDI messages dropped because an output queue was full.
    pub midi_dropped: u64,
    /// Blocks in which a schedule or graph swap was deferred.
    pub swaps_deferred: u64,
    /// Blocks rendered as silence because they were too large.
    pub xruns: u64,
    /// Blocks rendered as silence because the graph missed its deadline.
    pub deadline_misses: u64,
    /// Blocks rendered as silence because nodes of a missed block were
    /// still running.
    pub busy_blocks: u64,
    /// Inserts bypassed for faults.
    pub plugin_faults: u64,
    /// Graph runs: one per block unless automation splits it (Plan 4d
    /// Ruling 10).
    pub runs: u64,
}

/// Peak meter of one graph node: the largest |sample| per channel since
/// the last read, as `f32` bits (non-negative floats order like their
/// bits, so `fetch_max` works).
#[derive(Debug, Default)]
pub struct MeterCell {
    l: AtomicU32,
    r: AtomicU32,
}

impl MeterCell {
    /// Raises the held peaks. // RT
    pub fn hold(&self, l: f32, r: f32) {
        if l.is_finite() && l > 0.0 {
            self.l.fetch_max(l.to_bits(), Ordering::Relaxed);
        }
        if r.is_finite() && r > 0.0 {
            self.r.fetch_max(r.to_bits(), Ordering::Relaxed);
        }
    }

    /// Reads and clears the held peaks.
    pub fn take(&self) -> (f32, f32) {
        (
            f32::from_bits(self.l.swap(0, Ordering::Relaxed)),
            f32::from_bits(self.r.swap(0, Ordering::Relaxed)),
        )
    }
}

/// DSP load: render time ÷ block duration.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DspLoad {
    /// The last block.
    pub last: f32,
    /// The highest since the last read.
    pub peak: f32,
}

/// State shared between the renderer, its workers and the control side
/// (atomics only).
#[derive(Debug)]
pub struct Shared {
    position: AtomicU64,
    transport_pos: AtomicU64,
    state: AtomicU8,
    faults: AtomicU32,
    ended: AtomicBool,
    midi_dropped: AtomicU64,
    swaps_deferred: AtomicU64,
    graphs_applied: AtomicU64,
    xruns: AtomicU64,
    deadline_misses: AtomicU64,
    busy_blocks: AtomicU64,
    plugin_faults: AtomicU64,
    runs: AtomicU64,
    load_last: AtomicU32,
    load_peak: AtomicU32,
    /// Block size the renderer was last prepared for.
    prepared: AtomicU32,
    meters: Box<[MeterCell]>,
    board: Box<[AtomicU64]>,
}

impl Default for Shared {
    fn default() -> Shared {
        Shared {
            position: AtomicU64::new(0),
            transport_pos: AtomicU64::new(0),
            state: AtomicU8::new(0),
            faults: AtomicU32::new(0),
            ended: AtomicBool::new(false),
            midi_dropped: AtomicU64::new(0),
            swaps_deferred: AtomicU64::new(0),
            graphs_applied: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            deadline_misses: AtomicU64::new(0),
            busy_blocks: AtomicU64::new(0),
            plugin_faults: AtomicU64::new(0),
            runs: AtomicU64::new(0),
            load_last: AtomicU32::new(0),
            load_peak: AtomicU32::new(0),
            prepared: AtomicU32::new(0),
            meters: (0..MAX_NODES).map(|_| MeterCell::default()).collect(),
            board: (0..MAX_INSERTS).map(|_| AtomicU64::new(0)).collect(),
        }
    }
}

impl Shared {
    /// Sets fault bits. // RT
    pub fn raise(&self, f: Faults) {
        self.faults.fetch_or(f.0, Ordering::Relaxed);
    }

    /// Reads and clears the fault bits.
    pub fn take_faults(&self) -> Faults {
        Faults(self.faults.swap(0, Ordering::Relaxed))
    }

    /// Schedule frame of the transport, minus the graph latency (Plan
    /// Ruling 17).
    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    /// Schedule frame of the transport (not reduced by the graph latency):
    /// where a rebuilt engine must locate to continue seamlessly.
    pub fn transport_position(&self) -> u64 {
        self.transport_pos.load(Ordering::Relaxed)
    }

    /// Transport state.
    pub fn state(&self) -> PlayState {
        PlayState::from_u8(self.state.load(Ordering::Relaxed))
    }

    /// True once after playback reached the end and the tail finished.
    pub fn take_ended(&self) -> bool {
        self.ended.swap(false, Ordering::Relaxed)
    }

    /// Graphs the renderer has swapped in so far (diagnostic hook).
    pub fn graphs_applied(&self) -> u64 {
        self.graphs_applied.load(Ordering::Relaxed)
    }

    /// Current counters.
    pub fn counters(&self) -> Counters {
        Counters {
            midi_dropped: self.midi_dropped.load(Ordering::Relaxed),
            swaps_deferred: self.swaps_deferred.load(Ordering::Relaxed),
            xruns: self.xruns.load(Ordering::Relaxed),
            deadline_misses: self.deadline_misses.load(Ordering::Relaxed),
            busy_blocks: self.busy_blocks.load(Ordering::Relaxed),
            plugin_faults: self.plugin_faults.load(Ordering::Relaxed),
            runs: self.runs.load(Ordering::Relaxed),
        }
    }

    /// The meter of graph node `i`. // RT
    pub fn meter(&self, i: usize) -> Option<&MeterCell> {
        self.meters.get(i)
    }

    /// Records that `instance` was bypassed for faults. The board is an
    /// open-addressed set keyed by instance ID, so an ID survives structural
    /// edits until it is read. // RT
    pub fn report_plugin_fault(&self, instance: Option<PluginInstanceId>) {
        self.raise(Faults::PLUGIN_FAULT);
        self.plugin_faults.fetch_add(1, Ordering::Relaxed);
        let Some(id) = instance else {
            return;
        };
        let v = id.0.saturating_add(1);
        let n = self.board.len().max(1);
        let start = (id.0 % n as u64) as usize;
        for k in 0..n {
            let Some(cell) = self.board.get((start + k) % n) else {
                return;
            };
            match cell.compare_exchange(0, v, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return,
                Err(x) if x == v => return,
                Err(_) => {}
            }
        }
    }

    /// Instances bypassed for faults since the last call (read and
    /// cleared), in ascending ID order.
    pub fn take_plugin_faults(&self) -> Vec<PluginInstanceId> {
        let mut v: Vec<PluginInstanceId> = self
            .board
            .iter()
            .filter_map(|c| {
                let v = c.swap(0, Ordering::Relaxed);
                (v != 0).then(|| PluginInstanceId(v - 1))
            })
            .collect();
        v.sort();
        v
    }

    /// Stores one block's DSP load. // RT
    pub fn set_load(&self, load: f32) {
        // Not `load.max(0.0)`: that may keep -0.0, whose bits beat every load.
        let bits = if load > 0.0 { load.to_bits() } else { 0 };
        self.load_last.store(bits, Ordering::Relaxed);
        self.load_peak.fetch_max(bits, Ordering::Relaxed);
    }

    /// The last block's load and the peak since the last call (cleared).
    pub fn take_load(&self) -> DspLoad {
        DspLoad {
            last: f32::from_bits(self.load_last.load(Ordering::Relaxed)),
            peak: f32::from_bits(self.load_peak.swap(0, Ordering::Relaxed)),
        }
    }

    /// The block size the renderer was last prepared for (0 before it
    /// was); the engine builds graphs, inserts and sinks for at least this.
    pub fn prepared_frames(&self) -> u32 {
        self.prepared.load(Ordering::Acquire)
    }

    /// Counts a block rendered as silence because it was larger than the
    /// prepared size or the graph's buffers. // RT
    pub fn count_xrun(&self) {
        self.raise(Faults::XRUN);
        self.xruns.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts a block that missed its deadline. // RT
    pub fn count_deadline_miss(&self) {
        self.raise(Faults::XRUN);
        self.deadline_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts a block skipped because stragglers were running. // RT
    pub fn count_busy_block(&self) {
        self.raise(Faults::XRUN);
        self.busy_blocks.fetch_add(1, Ordering::Relaxed);
    }
}

/// Where a target's events go.
pub enum Sink {
    /// An instrument rendered into the graph.
    Instrument(Box<dyn crate::backend::InstrumentNode>),
    /// A timestamped MIDI output.
    Midi(Box<dyn MidiOut>),
}

impl std::fmt::Debug for Sink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Sink::Instrument(_) => "Sink::Instrument",
            Sink::Midi(_) => "Sink::Midi",
        })
    }
}

/// Commands from the control side, applied at the start of a block.
#[derive(Debug)]
pub enum EngineCmd {
    /// Replace the schedule, keeping the musical position.
    SetSchedule(Box<Schedule>),
    /// Replace the render graph; it adopts the running graph's inserts and,
    /// with the same shape, its node state.
    SetGraph(Box<Graph>),
    /// Start playing.
    Play,
    /// Stop (note-offs are sent).
    Stop,
    /// Move to a schedule frame.
    Locate(u64),
    /// Set or clear the loop range.
    SetLoop(Option<(u64, u64)>),
    /// Install the sink of a new target (a released index is reused only
    /// after its `RemoveSink` and every schedule and graph naming it).
    AddSink(TargetIdx, Sink),
    /// Remove a released target's sink; it goes back through the retire
    /// ring (a plugin instrument is deactivated off the audio thread).
    RemoveSink(TargetIdx),
    /// Send a note on or off to a target now (editor audition). Tracked
    /// apart from played notes: Stop, Locate and removing the target
    /// release it; a schedule swap (every edit recompiles), a loop wrap and
    /// the end do not, so a note dragged in an editor keeps sounding until
    /// the editor ends it (4b Plan Ruling 1).
    Audition(TargetIdx, Midi1Msg),
}

/// Something the renderer hands back for freeing off the audio thread.
#[derive(Debug)]
pub enum Retired {
    /// A replaced schedule.
    Schedule(Box<Schedule>),
    /// A replaced graph (with the inserts it did not hand on).
    Graph(Box<Graph>),
    /// A removed sink.
    Sink(Sink),
}

/// Capacity of the command ring.
pub const CMD_CAPACITY: usize = 256;
/// Capacity of the retire ring: room for a schedule, a graph and a
/// removed sink per target (3a Plan Ruling 30; 3b Plan Ruling 4).
pub const RETIRE_CAPACITY: usize = 64;

/// Renderer settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderConfig {
    /// Stream sample rate.
    pub sample_rate: f64,
    /// Largest block (≤ [`crate::backend::MAX_FRAMES`] is typical).
    pub max_frames: u32,
    /// Added to every MIDI-out timestamp.
    pub midi_offset_ns: i64,
    /// Fraction of the block duration the graph may take before the block
    /// is given up (Plan Ruling 18); `None` waits for the graph.
    pub deadline: Option<f64>,
}

/// The real-time object. Built by [`crate::engine::Engine::new`], moved to
/// the audio thread by a backend and handed back on stop.
pub struct Renderer {
    cfg: RenderConfig,
    cmds: Consumer<EngineCmd>,
    retire: Producer<Retired>,
    shared: Arc<Shared>,
    schedule: Option<Box<Schedule>>,
    cursor: usize,
    transport: Transport,
    notes: ActiveNotes,
    /// Notes sounding because an editor auditions them.
    audition: ActiveNotes,
    sinks: Box<SinkTable>,
    bufs: Vec<StereoBuf>,
    block_ns: u64,
    /// Controllers were reset (stop, locate, wrap or swap): the next played
    /// frames start with the chased state.
    chase_pending: bool,
    graph: Option<Box<Graph>>,
    exec: Box<Exec>,
    /// Graph latency `L` in frames (Plan Ruling 17).
    latency: u64,
    /// Frames of busy blocks the sinks have not been told about.
    pending_skip: u64,
    /// Per automation lane, the plugin value last sent (NaN: none yet).
    auto_last: Box<[f64; MAX_AUTO_LANES]>,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("cfg", &self.cfg)
            .field("transport", &self.transport)
            .field("graph", &self.graph)
            .finish_non_exhaustive()
    }
}

/// Everything `send` needs, borrowed apart from the note tracker.
struct Out<'a> {
    sinks: &'a mut SinkTable,
    shared: &'a Shared,
    block_ns: u64,
    ns_per_frame: f64,
    /// MIDI offset plus the graph latency, in ns.
    late_ns: i64,
    /// Block offset of the run being rendered: instrument sinks get offsets
    /// relative to it (Plan 4d Ruling 10).
    base: u32,
}

impl Out<'_> {
    /// Sends `bytes` to `target` at block offset `offset`. // RT
    fn send(&mut self, target: TargetIdx, offset: u32, bytes: &[u8]) {
        match self.sinks.get_mut(usize::from(target.0)) {
            Some(Some(Sink::Instrument(node))) => {
                node.midi(offset.saturating_sub(self.base), bytes)
            }
            Some(Some(Sink::Midi(out))) => {
                let at = self.block_ns as i64
                    + (f64::from(offset) * self.ns_per_frame).round() as i64
                    + self.late_ns;
                if !out.send(at.max(0) as u64, bytes) {
                    self.shared.raise(Faults::MIDI_DROPPED);
                    self.shared.midi_dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }
}

/// Tells every instrument sink not in `except` (a target bitmask) that
/// `frames` frames passed unrendered. // RT
fn skip_sinks(sinks: &mut SinkTable, frames: u64, except: u64) {
    for (t, s) in sinks.iter_mut().enumerate() {
        let bit = u32::try_from(t)
            .ok()
            .and_then(|t| 1u64.checked_shl(t))
            .unwrap_or(0);
        if except & bit != 0 {
            continue;
        }
        if let Some(Sink::Instrument(node)) = s {
            let mut left = frames;
            while left > 0 {
                let k = u32::try_from(left).unwrap_or(u32::MAX);
                node.skip(k);
                left -= u64::from(k);
            }
        }
    }
}

/// Hard clamp of frames `offset..offset + frames` of channels 0–1 at ±1.0;
/// non-finite samples become 0. Raises `CLIPPED` when anything was changed
/// (engine ruling). // RT
fn clamp_master(out: &mut [&mut [f32]], offset: usize, frames: u32, shared: &Shared) {
    let n = frames as usize;
    let mut clipped = false;
    for ch in out.iter_mut().take(2) {
        if let Some(c) = ch.get_mut(offset..offset + n) {
            for x in c {
                if !x.is_finite() {
                    clipped = true;
                    *x = 0.0;
                } else if *x > 1.0 || *x < -1.0 {
                    clipped = true;
                    *x = x.clamp(-1.0, 1.0);
                }
            }
        }
    }
    if clipped {
        shared.raise(Faults::CLIPPED);
    }
}

impl Renderer {
    pub(crate) fn new(
        cfg: RenderConfig,
        cmds: Consumer<EngineCmd>,
        retire: Producer<Retired>,
        shared: Arc<Shared>,
        pool: Option<Arc<PoolShared>>,
    ) -> Renderer {
        let mut r = Renderer {
            cfg,
            cmds,
            retire,
            shared,
            schedule: None,
            cursor: 0,
            transport: Transport::new(cfg.sample_rate.max(0.0).round() as u64),
            notes: ActiveNotes::default(),
            audition: ActiveNotes::default(),
            sinks: Box::default(),
            bufs: Vec::new(),
            block_ns: 0,
            chase_pending: false,
            graph: None,
            exec: Exec::new(pool),
            latency: 0,
            pending_skip: 0,
            auto_last: Box::new([f64::NAN; MAX_AUTO_LANES]),
        };
        r.prepare(cfg.max_frames);
        r
    }

    /// (Re)allocates scratch and graph buffers for blocks of up to
    /// `max_frames`. Not real-time safe: call only while stopped.
    pub fn prepare(&mut self, max_frames: u32) {
        self.exec.wait_idle();
        self.cfg.max_frames = max_frames;
        self.shared.prepared.store(max_frames, Ordering::Release);
        self.bufs = (0..MAX_TARGETS)
            .map(|_| StereoBuf::new(max_frames))
            .collect();
        if let Some(g) = self.graph.as_mut() {
            g.prepare(max_frames);
        }
    }

    /// Settings.
    pub fn config(&self) -> RenderConfig {
        self.cfg
    }

    /// Sets fault bits (used by backends, e.g. after a caught panic). // RT
    pub fn raise(&self, f: Faults) {
        self.shared.raise(f);
    }

    /// Renders one block of `frames` frames into planar `out` channels.
    /// Channels 0–1 get master, the rest zeros. // RT
    pub fn process(&mut self, out: &mut [&mut [f32]], frames: u32, time: BlockTime) {
        #[cfg(feature = "rt-check")]
        crate::rt_check::arm();
        self.process_inner(out, frames, time);
        #[cfg(feature = "rt-check")]
        crate::rt_check::disarm();
    }

    fn latency_ns(&self) -> i64 {
        (self.latency as f64 * 1e9 / self.cfg.sample_rate.max(1.0)).round() as i64
    }

    fn process_inner(&mut self, out: &mut [&mut [f32]], frames: u32, time: BlockTime) {
        let started = Instant::now();
        let block_s = f64::from(frames) / self.cfg.sample_rate.max(1.0);
        let deadline = self
            .cfg
            .deadline
            .and_then(|f| Duration::try_from_secs_f64(f * block_s).ok())
            .and_then(|d| started.checked_add(d));
        let n = frames as usize;
        for ch in out.iter_mut() {
            ch.fill(0.0);
        }
        if !self.exec.try_finish(deadline) {
            // Nodes of a missed block are still running: touch nothing they
            // use (Plan Ruling 18).
            self.shared.count_busy_block();
            self.pending_skip = self.pending_skip.saturating_add(u64::from(frames));
            // The block was lost to the graph: report it as full load.
            self.shared.set_load(1.0);
            return;
        }
        // No run is in flight: the last run's ramps have arrived (Plan 4d
        // P11).
        if let Some(g) = self.graph.as_deref_mut() {
            g.settle();
        }
        if self.pending_skip > 0 {
            let skip = std::mem::take(&mut self.pending_skip);
            skip_sinks(&mut self.sinks, skip, 0);
        }
        // A graph smaller than the block is caught in `render_graph`, after
        // the commands: checking it here would also hold back the graph
        // that replaces it.
        if frames > self.cfg.max_frames || out.iter().any(|c| c.len() < n) {
            skip_sinks(&mut self.sinks, u64::from(frames), 0);
            self.shared.count_xrun();
            return;
        }
        self.block_ns = time.host_time_ns;
        self.drain_commands();
        let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let ns_per_frame = 1e9 / self.cfg.sample_rate.max(1.0);
        let mut missed = false;
        let mut offset = 0u32;
        while offset < frames {
            // Between runs no worker may still hold the graph or the sinks
            // (the first run was checked above); then the previous run's
            // ramps have arrived (Plan 4d P11).
            if offset > 0 {
                if !self.exec.try_finish(deadline) {
                    self.shared.count_deadline_miss();
                    missed = true;
                    break;
                }
                if let Some(g) = self.graph.as_deref_mut() {
                    g.settle();
                }
            }
            let run = self.next_run(length, frames - offset);
            let end = offset + run;
            self.automate(length, run);
            let info = self.block_info();
            let mut o = Out {
                sinks: &mut self.sinks,
                shared: &self.shared,
                block_ns: time.host_time_ns,
                ns_per_frame,
                late_ns,
                base: offset,
            };
            let mut at = offset;
            while at < end {
                let step = self.transport.next(length, at, end - at);
                match step {
                    Step::Play { from, to, offset } => {
                        if let Some(s) = self.schedule.as_deref() {
                            if std::mem::take(&mut self.chase_pending) {
                                s.chase
                                    .state_before(from, |t, m| o.send(t, offset, m.as_slice()));
                            }
                            while let Some(e) = s.entries.get(self.cursor) {
                                if e.frame >= to {
                                    break;
                                }
                                let off = offset
                                    + u32::try_from(e.frame.saturating_sub(from)).unwrap_or(0);
                                let bytes = s.bytes(&e.msg);
                                o.send(e.target, off, bytes);
                                self.notes.observe(e.target, bytes);
                                self.cursor += 1;
                            }
                        }
                    }
                    Step::Wrap { offset, to } => {
                        release_all(&mut self.notes, &mut o, offset);
                        if let Some(s) = self.schedule.as_deref() {
                            reset_controllers(s, &mut o, offset);
                        }
                        self.chase_pending = true;
                        self.cursor = self.schedule.as_ref().map_or(0, |s| s.cursor_at(to));
                    }
                    Step::End { offset } => release_all(&mut self.notes, &mut o, offset),
                    Step::Idle { .. } => {}
                }
                at += step.frames();
            }
            let lost = self.render_graph(out, offset, run, info, deadline);
            offset = end;
            if lost {
                missed = true;
                break;
            }
        }
        if missed && offset < frames {
            // Plan 4d Ruling 10 as built (P11): the run that missed its
            // deadline was already played by the transport (its MIDI went
            // out and its frames advanced); only the later runs of the block
            // are skipped. The transport does not advance over them, and,
            // as for a busy block, the sinks are told they passed once the
            // stragglers are done.
            self.pending_skip = self.pending_skip.saturating_add(u64::from(frames - offset));
        }
        self.publish();
        if block_s > 0.0 {
            let load = (started.elapsed().as_secs_f64() / block_s) as f32;
            // A missed deadline lost the block: at least full load.
            self.shared
                .set_load(if missed { load.max(1.0) } else { load });
        }
    }

    /// The next run's length (1 ..= `left`): the rest of the block, cut at
    /// the next automation point (never under `MIN_RUN` frames after the
    /// run's start) and at the loop end or the sequence end while a
    /// schedule with automation plays (Plan 4d Ruling 10). // RT
    fn next_run(&self, length: u64, left: u32) -> u32 {
        let Some(s) = self.schedule.as_deref() else {
            return left;
        };
        if s.automation.is_empty() {
            return left;
        }
        let Some(pos) = self.transport.play_pos(length) else {
            return left;
        };
        let mut run = u64::from(left);
        if let Some(c) = s.automation.next_cut(pos) {
            run = run.min((c - pos).max(u64::from(MIN_RUN)));
        }
        if let Some(b) = self.transport.boundary(length) {
            if b > pos {
                run = run.min(b - pos);
            }
        }
        u32::try_from(run).unwrap_or(left).clamp(1, left.max(1))
    }

    /// Applies automation to the next `run` frames while playing (Plan 4d
    /// Rulings 11, 12): strip and send controls are committed at the run's
    /// start values, then ramp to its end values (mute and solo switch hard,
    /// at the run boundary nearest their point); a plugin parameter gets
    /// its start value and, on a ramp, a value every `max(MIN_RUN, run /
    /// 16)` frames, each only when it changed. // RT
    fn automate(&mut self, length: u64, run: u32) {
        let Some(s) = self.schedule.as_deref() else {
            return;
        };
        let auto = &s.automation;
        if auto.is_empty() {
            return;
        }
        let Some(pos) = self.transport.play_pos(length) else {
            return;
        };
        let end = pos.saturating_add(u64::from(run));
        // A mute or solo point inside the run (one shorter than `MIN_RUN`
        // stretched over it) switches at the run boundary nearest to it:
        // the run's start when at most half way in, else the next run's.
        // A fader, pan or send jump inside such a run (Ruling 11's "two
        // points on one frame") is not switched hard: it fades over the
        // run, at most `MIN_RUN` frames (under 1 ms), which avoids a click.
        let mid = pos.saturating_add(u64::from(run / 2));
        if let Some(g) = self.graph.as_deref_mut() {
            for ramp in [false, true] {
                let mut any = false;
                for lane in auto.lanes() {
                    let v = match (ramp, lane.dest) {
                        (false, _) => auto.value(lane, pos, false),
                        (true, AutoDest::Strip(_, StripParam::Mute | StripParam::Solo)) => {
                            auto.value(lane, mid, false)
                        }
                        (true, _) => auto.value(lane, end, true),
                    };
                    match lane.dest {
                        AutoDest::Strip(id, p) => {
                            g.automate_strip(id, p, v);
                            any = true;
                        }
                        AutoDest::Send(id, i) => {
                            g.automate_send(id, i, v);
                            any = true;
                        }
                        AutoDest::Plugin(..) => {}
                    }
                }
                if any {
                    g.commit_controls(ramp);
                }
            }
        }
        let every = (run / 16).max(MIN_RUN);
        for (k, lane) in auto.lanes().iter().enumerate() {
            let AutoDest::Plugin(id, param) = lane.dest else {
                continue;
            };
            let Some(last) = self.auto_last.get_mut(k) else {
                break;
            };
            let mut f = 0u32;
            loop {
                let v = auto.value(lane, pos.saturating_add(u64::from(f)), false);
                if v.to_bits() != last.to_bits() {
                    *last = v;
                    send_param(
                        self.graph.as_deref_mut(),
                        &mut self.sinks,
                        lane.instrument,
                        id,
                        f,
                        param,
                        v,
                    );
                }
                if lane.stepped {
                    break;
                }
                f = f.saturating_add(every);
                if f >= run {
                    break;
                }
            }
        }
    }

    /// Drops the overrides of the controls the schedule no longer
    /// automates (Plan 4d Ruling 13). // RT
    fn forget_unautomated(&mut self) {
        let Some(g) = self.graph.as_deref_mut() else {
            return;
        };
        let auto = self.schedule.as_deref().map(|s| &s.automation);
        g.release_overrides(|d| auto.is_some_and(|a| a.drives(d)));
        g.commit_controls(false);
    }

    /// Renders the graph (or, before the first graph arrives, sums the
    /// instrument sinks) into frames `offset..offset + frames` of channels
    /// 0–1 and clamps; true when the graph missed its deadline. // RT
    fn render_graph(
        &mut self,
        out: &mut [&mut [f32]],
        offset: u32,
        frames: u32,
        info: BlockInfo,
        deadline: Option<Instant>,
    ) -> bool {
        let at = offset as usize;
        let Some(g) = self.graph.as_deref() else {
            self.mix_direct(out, at, frames);
            clamp_master(out, at, frames, &self.shared);
            return false;
        };
        if g.max_frames() < frames {
            // A graph with smaller buffers than the block (built before a
            // `prepare` and still queued then): never run it past them.
            skip_sinks(&mut self.sinks, u64::from(frames), 0);
            self.shared.count_xrun();
            return false;
        }
        self.shared.runs.fetch_add(1, Ordering::Relaxed);
        match self
            .exec
            .run(g, &self.sinks, &self.shared, frames, info, deadline)
        {
            RunResult::Done => {
                g.write_master(out, at, frames);
                let covered = g.instruments();
                skip_sinks(&mut self.sinks, u64::from(frames), covered);
                clamp_master(out, at, frames, &self.shared);
                false
            }
            RunResult::Missed => {
                self.shared.count_deadline_miss();
                true
            }
        }
    }

    /// Applies queued commands. A `SetSchedule` or `SetGraph` waits when
    /// the retire ring is full (and everything after it waits too, to keep
    /// order). // RT
    fn drain_commands(&mut self) {
        loop {
            let deferred = match self.cmds.peek() {
                Ok(EngineCmd::SetSchedule(_)) => {
                    self.schedule.is_some() && self.retire.slots() == 0
                }
                Ok(EngineCmd::SetGraph(_)) => self.graph.is_some() && self.retire.slots() == 0,
                Ok(EngineCmd::RemoveSink(_)) => self.retire.slots() == 0,
                Ok(_) => false,
                Err(_) => break,
            };
            if deferred {
                self.shared.raise(Faults::SWAP_DEFERRED);
                self.shared.swaps_deferred.fetch_add(1, Ordering::Relaxed);
                break;
            }
            let Ok(cmd) = self.cmds.pop() else {
                break;
            };
            self.apply(cmd);
        }
    }

    /// Note-offs (of audition notes too when `auditions`), then a
    /// controller reset on every channel of the current schedule, at block
    /// offset 0; the next played frames chase the controller state (ruling
    /// O23). // RT
    fn release_now(&mut self, auditions: bool) {
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns: self.block_ns,
            ns_per_frame: 0.0,
            late_ns,
            base: 0,
        };
        release_all(&mut self.notes, &mut o, 0);
        if auditions {
            release_all(&mut self.audition, &mut o, 0);
        }
        if let Some(s) = self.schedule.as_deref() {
            reset_controllers(s, &mut o, 0);
        }
        self.chase_pending = true;
    }

    fn apply(&mut self, cmd: EngineCmd) {
        match cmd {
            EngineCmd::SetSchedule(new) => {
                self.release_now(false);
                if let Some(old) = self.schedule.take() {
                    let tick = old.tempo.frame_to_tick(self.transport.pos);
                    self.transport.pos = new.tempo.tick_to_frame(tick);
                    self.transport.loop_range = self.transport.loop_range.map(|(s, e)| {
                        (
                            new.tempo.tick_to_frame(old.tempo.frame_to_tick(s)),
                            new.tempo.tick_to_frame(old.tempo.frame_to_tick(e)),
                        )
                    });
                    if let Err(rtrb::PushError::Full(old)) =
                        self.retire.push(Retired::Schedule(old))
                    {
                        // Unreachable: room was checked in `drain_commands`.
                        // Leaking is the only RT-safe choice.
                        std::mem::forget(old);
                    }
                }
                self.cursor = new.cursor_at(self.transport.pos);
                self.schedule = Some(new);
                self.auto_last.fill(f64::NAN);
                if self.transport.state == PlayState::Playing {
                    self.forget_unautomated();
                }
            }
            EngineCmd::SetGraph(mut new) => {
                if let Some(mut old) = self.graph.take() {
                    new.adopt_from(&mut old);
                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Graph(old)) {
                        // Unreachable: room was checked in `drain_commands`.
                        std::mem::forget(old);
                    }
                }
                self.latency = new.latency();
                // The compensated audio of the last notes arrives L frames
                // late: the tail waits for it before `ended`.
                self.transport.latency = self.latency;
                self.graph = Some(new);
                self.shared.graphs_applied.fetch_add(1, Ordering::Relaxed);
                // New inserts need their automated values.
                self.auto_last.fill(f64::NAN);
            }
            EngineCmd::Play => {
                let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
                self.transport.play(length);
                self.seek();
                self.forget_unautomated();
                self.auto_last.fill(f64::NAN);
            }
            EngineCmd::Stop => {
                self.release_now(true);
                self.transport.stop();
            }
            EngineCmd::Locate(frame) => {
                self.release_now(true);
                self.transport.locate(frame);
                self.seek();
                self.auto_last.fill(f64::NAN);
            }
            EngineCmd::SetLoop(range) => self.transport.loop_range = range,
            EngineCmd::RemoveSink(idx) => {
                // Forget its sounding notes: nothing will play them now.
                self.notes.release(idx, |_| {});
                self.audition.release(idx, |_| {});
                let taken = self
                    .sinks
                    .get_mut(usize::from(idx.0))
                    .and_then(Option::take);
                if let Some(old) = taken {
                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Sink(old)) {
                        // Unreachable: room was checked in `drain_commands`.
                        std::mem::forget(old);
                    }
                }
            }
            EngineCmd::Audition(target, msg) => {
                let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
                let mut o = Out {
                    sinks: &mut self.sinks,
                    shared: &self.shared,
                    block_ns: self.block_ns,
                    ns_per_frame: 0.0,
                    late_ns,
                    base: 0,
                };
                o.send(target, 0, msg.as_slice());
                self.audition.observe(target, msg.as_slice());
            }
            EngineCmd::AddSink(idx, sink) => {
                if let Some(slot) = self.sinks.get_mut(usize::from(idx.0)) {
                    if let Some(old) = slot.replace(sink) {
                        // Unreachable: the engine only fills empty slots.
                        std::mem::forget(old);
                    }
                }
            }
        }
    }

    /// The transport facts of the block about to render. // RT
    fn block_info(&self) -> BlockInfo {
        let frame = self.transport.pos;
        let sr = self.cfg.sample_rate;
        let (beats, tempo_bpm) = self.schedule.as_ref().map_or((0.0, 120.0), |s| {
            (
                s.tempo.frame_to_tick(frame).0 as f64 / core_model::time::PPQ as f64,
                s.tempo.bpm_at(frame, sr),
            )
        });
        BlockInfo {
            playing: self.transport.state == PlayState::Playing,
            frame,
            beats,
            tempo_bpm,
        }
    }

    fn seek(&mut self) {
        self.cursor = self
            .schedule
            .as_ref()
            .map_or(0, |s| s.cursor_at(self.transport.pos));
    }

    /// Before the first graph: renders instrument sinks and sums them into
    /// frames `at..at + frames` of channels 0–1 (2b behaviour). // RT
    fn mix_direct(&mut self, out: &mut [&mut [f32]], at: usize, frames: u32) {
        let n = frames as usize;
        for (sink, buf) in self.sinks.iter_mut().zip(self.bufs.iter_mut()) {
            let Some(Sink::Instrument(node)) = sink else {
                continue;
            };
            node.render(frames, buf);
            for (i, src) in [&buf.left, &buf.right].into_iter().enumerate() {
                let (Some(dst), Some(src)) = (
                    out.get_mut(i).and_then(|c| c.get_mut(at..at + n)),
                    src.get(..n),
                ) else {
                    continue;
                };
                for (d, s) in dst.iter_mut().zip(src) {
                    *d += *s;
                }
            }
        }
    }

    fn publish(&mut self) {
        let s = &self.shared;
        s.position.store(
            self.transport.pos.saturating_sub(self.latency),
            Ordering::Relaxed,
        );
        s.transport_pos.store(self.transport.pos, Ordering::Relaxed);
        s.state.store(self.transport.state as u8, Ordering::Relaxed);
        if self.transport.take_ended() {
            s.ended.store(true, Ordering::Relaxed);
        }
    }

    /// Resets every instrument (all notes off). Call while stopped.
    pub fn reset_instruments(&mut self) {
        self.exec.wait_idle();
        for sink in self.sinks.iter_mut().flatten() {
            if let Sink::Instrument(node) = sink {
                node.reset();
            }
        }
    }

    /// Mutable access to a target's sink (for backends and tests, not RT).
    pub fn sink_mut(&mut self, idx: TargetIdx) -> Option<&mut Sink> {
        self.exec.wait_idle();
        self.sinks
            .get_mut(usize::from(idx.0))
            .and_then(Option::as_mut)
    }

    /// The graph latency `L` in frames.
    pub fn latency(&self) -> u64 {
        self.latency
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // Stragglers of a missed block may still use the graph and sinks.
        self.exec.wait_idle();
    }
}

/// Pitch-bend centre and CC 121 (reset all controllers) on every channel
/// `s` uses, at block offset `offset`. // RT
fn reset_controllers(s: &Schedule, o: &mut Out<'_>, offset: u32) {
    for (t, mask) in s.used_channels.iter().enumerate() {
        let target = TargetIdx(t as u8);
        for ch in 0..16u8 {
            if mask & (1 << ch) != 0 {
                o.send(target, offset, &[0xE0 | ch, 0x00, 0x40]);
                o.send(target, offset, &[0xB0 | ch, 121, 0x00]);
            }
        }
    }
}

/// Sends a plugin parameter's automated value: to its plugin
/// instrument's sink when it has one, else to its insert in the graph.
/// // RT
fn send_param(
    graph: Option<&mut Graph>,
    sinks: &mut SinkTable,
    instrument: Option<TargetIdx>,
    id: PluginInstanceId,
    frame: u32,
    param: ParamId,
    v: f64,
) {
    if let Some(t) = instrument {
        if let Some(Some(Sink::Instrument(node))) = sinks.get_mut(usize::from(t.0)) {
            node.automate(frame, param, v);
        }
        return;
    }
    if let Some(node) = graph.and_then(|g| g.insert_mut(id)) {
        node.automate(frame, param, v);
    }
}

/// Note-offs (and pedal-ups) for every target at block offset `offset`. // RT
fn release_all(notes: &mut ActiveNotes, o: &mut Out<'_>, offset: u32) {
    for t in 0..MAX_TARGETS {
        let target = TargetIdx(t as u8);
        notes.release(target, |m| o.send(target, offset, m.as_slice()));
    }
}
