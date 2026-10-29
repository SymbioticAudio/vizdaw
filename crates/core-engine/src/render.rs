//! The real-time renderer and the state it publishes.

use crate::backend::{BlockInfo, BlockTime, InputBlock, MidiOut, StereoBuf};
use crate::count_in::{CountIn, Counting};
use crate::gesture::GestureTap;
use crate::graph::exec::{Exec, RunResult, SinkTable};
use crate::graph::regions::InputCopy;
use crate::graph::{Graph, MAX_INSERTS, MAX_NODES};
use crate::input::{InputRoutes, MIDI_IN_PER_BLOCK, MidiIn, on_channel};
use crate::measure::MeasureTap;
use crate::midi1::Midi1Msg;
use crate::notes::ActiveNotes;
use crate::pool::PoolShared;
use crate::record::RecordTap;
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
    /// A region's frames were not streamed in time: they played as
    /// silence (5a).
    pub const STREAM_UNDERRUN: Faults = Faults(2048);
    /// Right after Play or Locate (within [`LATE_SECONDS`]) a region was
    /// not ready yet and started late (5a).
    pub const LATE_START: Faults = Faults(4096);

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
            (Faults::STREAM_UNDERRUN, "stream-underrun"),
            (Faults::LATE_START, "late-start"),
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
    /// Regions play silence (the device refused the project rate, 5a).
    regions_muted: AtomicBool,
    /// A count-in is playing (plan 5d-1 Ruling 7).
    counting: AtomicBool,
    position: AtomicU64,
    transport_pos: AtomicU64,
    state: AtomicU8,
    faults: AtomicU32,
    ended: AtomicBool,
    midi_dropped: AtomicU64,
    /// Gestures dropped by the renderer (a touch with no free live entry).
    gestures_dropped: AtomicU64,
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
    /// Per device input channel, the peak since the last read (`f32` bits;
    /// plan 5d-1 Ruling 27).
    input_peaks: Box<[AtomicU32]>,
}

/// The peak of the first `n` samples of `ch`; a non-finite sample counts
/// as a clip (1.0). // RT
pub fn block_peak(ch: &[f32], n: usize) -> f32 {
    ch.iter().take(n).fold(
        0f32,
        |m, x| if x.is_finite() { m.max(x.abs()) } else { 1.0 },
    )
}

impl Default for Shared {
    fn default() -> Shared {
        Shared {
            regions_muted: AtomicBool::new(false),
            counting: AtomicBool::new(false),
            position: AtomicU64::new(0),
            transport_pos: AtomicU64::new(0),
            state: AtomicU8::new(0),
            faults: AtomicU32::new(0),
            ended: AtomicBool::new(false),
            midi_dropped: AtomicU64::new(0),
            gestures_dropped: AtomicU64::new(0),
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
            input_peaks: (0..crate::backend::MAX_INPUT_CHANNELS)
                .map(|_| AtomicU32::new(0))
                .collect(),
        }
    }
}

impl Shared {
    /// Mutes or unmutes every region (5a Ruling 20).
    pub fn set_regions_muted(&self, on: bool) {
        self.regions_muted.store(on, Ordering::Relaxed);
    }

    /// Whether regions are muted. // RT
    pub fn regions_muted(&self) -> bool {
        self.regions_muted.load(Ordering::Relaxed)
    }

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

    /// Whether a count-in plays before the transport rolls.
    pub fn counting_in(&self) -> bool {
        self.counting.load(Ordering::Relaxed)
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

    /// Raises input channel `c`'s held peak to `peak` (non-negative floats
    /// order like their bits). // RT
    pub fn hold_input(&self, c: usize, peak: f32) {
        if let Some(cell) = self.input_peaks.get(c) {
            if peak.is_finite() && peak > 0.0 {
                cell.fetch_max(peak.to_bits(), Ordering::Relaxed);
            }
        }
    }

    /// Clears every held input peak (control side, when the input config
    /// changes).
    pub fn clear_input_peaks(&self) {
        for c in self.input_peaks.iter() {
            c.store(0, Ordering::Relaxed);
        }
    }

    /// The first `channels` input channels' peaks since the last read,
    /// cleared.
    pub fn take_input_peaks(&self, channels: usize) -> Vec<f32> {
        self.input_peaks
            .iter()
            .take(channels)
            .map(|c| f32::from_bits(c.swap(0, Ordering::Relaxed)))
            .collect()
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

    /// Counts a gesture the live table refused (plan 5e-1 Ruling 6). // RT
    pub(crate) fn count_gesture_drop(&self) {
        self.gestures_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Gestures the live table refused so far.
    pub fn gestures_dropped(&self) -> u64 {
        self.gestures_dropped.load(Ordering::Relaxed)
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
    /// Start playing; `until` (a schedule frame) stops it there instead
    /// of at the end, ignoring the loop (Play Selection, plan 4f). With
    /// `count_in`, a count-in plays first and the transport rolls when it
    /// ends (plan 5d-1 Ruling 7).
    Play {
        /// The frame playing stops at.
        until: Option<u64>,
        /// The count-in to play before rolling.
        count_in: Option<CountIn>,
    },
    /// Stop (note-offs are sent).
    Stop,
    /// Move to a schedule frame.
    Locate(u64),
    /// Set or clear the loop range.
    SetLoop(Option<(u64, u64)>),
    /// Install the sink of a new target (a released index is reused only
    /// after its `RemoveSink` and every schedule and graph naming it).
    AddSink(TargetIdx, Sink),
    /// Remove a released target's sink, ending the notes still sounding
    /// on it first (plan 5d-1 Ruling 23); it goes back through the retire
    /// ring (a plugin instrument is deactivated off the audio thread).
    RemoveSink(TargetIdx),
    /// Send a note on or off to a target now (editor audition). Tracked
    /// apart from played notes: Stop, Locate and removing the target
    /// release it; a schedule swap (every edit recompiles), a loop wrap and
    /// the end do not, so a note dragged in an editor keeps sounding until
    /// the editor ends it (4b Plan Ruling 1).
    Audition(TargetIdx, Midi1Msg),
    /// Install a record tap, or remove the one installed (it goes back
    /// through the retire ring; dropping it tells the control side the
    /// recording's input ended). Spec §6.2.
    Record(Option<Box<RecordTap>>),
    /// Replace the live input routes (spec §6.6); notes sounding through
    /// thru end first.
    SetInputs(Box<InputRoutes>),
    /// Install a return-latency measurement (spec §6.7), or remove the one
    /// installed (its sounding note ends; it goes back through the retire
    /// ring).
    Measure(Option<Box<MeasureTap>>),
    /// Install a gesture tap (plan 5e-1 Ruling 7), or remove the one
    /// installed (it goes back through the retire ring; its status reads
    /// closed).
    Gestures(Option<Box<GestureTap>>),
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
    /// A removed record tap.
    Tap(Box<RecordTap>),
    /// Replaced input routes.
    Inputs(Box<InputRoutes>),
    /// A removed measurement.
    Measure(Box<MeasureTap>),
    /// A removed gesture tap.
    Gestures(Box<GestureTap>),
}

/// Capacity of the command ring.
pub const CMD_CAPACITY: usize = 256;
/// How long after Play or Locate a miss counts as a late start (spec
/// §5.5: the transport waits this long for the disk at most).
pub const LATE_SECONDS: f64 = 0.25;
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
    /// The streamer's real-time side (boxed: the executor's job points at
    /// it while a run is in flight).
    reader: Box<core_media::stream::StreamReader>,
    /// Frames played since the last Play or Locate: misses while it is
    /// under [`LATE_SECONDS`] count as late starts (5a Ruling 10). Counted
    /// in frames played, not schedule frames, so a loop wrap back below
    /// where playing started does not reopen the window (F16).
    since_start: u64,
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
    /// The recording's tap (spec §6.2), while one is installed.
    tap: Option<Box<RecordTap>>,
    /// The gesture tap (plan 5e-1 Ruling 7), while one is installed.
    gestures: Option<Box<GestureTap>>,
    /// MIDI from sources (spec §6.6).
    midi_in: Consumer<MidiIn>,
    /// Gestures from the control side (spec §7.1; plan 5e-1 Ruling 6).
    gesture_in: Consumer<crate::gesture::GestureMsg>,
    /// The touched targets (boxed: the renderer moves between threads by
    /// value).
    live: Box<crate::gesture::LiveTable>,
    /// The live input routes.
    inputs: Option<Box<InputRoutes>>,
    /// Notes sounding through thru (boxed: the renderer moves between
    /// threads by value).
    thru: Box<ActiveNotes>,
    /// This block's input, for monitoring.
    input_copy: Box<InputCopy>,
    /// A return-latency measurement (spec §6.7), while one is installed.
    measure: Option<Box<MeasureTap>>,
    /// Where the transport last rolled from (a Play while not playing, a
    /// Locate): led regions read nothing before it (plan 5d-1 Ruling 3).
    roll_floor: u64,
    /// The loop `[start, end)` the transport last wrapped since it rolled
    /// (kept when the loop is edited: it is what was heard).
    wrapped: Option<crate::graph::Wrapped>,
    /// Frames played (`since_start`) when the transport last rolled or
    /// wrapped.
    seg_mark: u64,
    /// Frames rendered since playing reached its end: led regions play
    /// out their last frames meanwhile.
    after_end: u64,
    /// The count-in playing before the transport rolls, and its last
    /// clicks after (Ruling 7).
    counting: Option<Counting>,
}

/// What a wrap now, after `last` frames played since the roll or the
/// last wrap, makes of the remembered wraps: the active loop (else
/// the one remembered) with the stretches counted on. // RT
fn wrapped_at(
    transport: &Transport,
    prev: Option<crate::graph::Wrapped>,
    last: u64,
) -> Option<crate::graph::Wrapped> {
    let (start, end) = transport.active_loop().or(prev.map(|w| (w.start, w.end)))?;
    Some(crate::graph::Wrapped {
        start,
        end,
        last,
        first: prev.map_or(last, |w| w.first),
        wraps: prev.map_or(0, |w| w.wraps) + 1,
    })
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
    // One argument per queue and shared part the engine hands over.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        cfg: RenderConfig,
        cmds: Consumer<EngineCmd>,
        retire: Producer<Retired>,
        shared: Arc<Shared>,
        pool: Option<Arc<PoolShared>>,
        reader: core_media::stream::StreamReader,
        midi_in: Consumer<MidiIn>,
        gesture_in: Consumer<crate::gesture::GestureMsg>,
    ) -> Renderer {
        let mut r = Renderer {
            cfg,
            cmds,
            retire,
            shared,
            reader: Box::new(reader),
            since_start: 0,
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
            tap: None,
            gestures: None,
            midi_in,
            gesture_in,
            live: Box::new(crate::gesture::LiveTable::new()),
            inputs: None,
            thru: Box::default(),
            input_copy: Box::new(InputCopy::new(0, 0)),
            measure: None,
            roll_floor: 0,
            wrapped: None,
            seg_mark: 0,
            after_end: 0,
            counting: None,
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
        *self.input_copy = InputCopy::new(crate::backend::MAX_INPUT_CHANNELS, max_frames);
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

    /// Clears every held input peak ([`Engine::clear_input_peaks`]): a
    /// host reopening the device calls it while the renderer is stopped,
    /// so no peak of the old config survives and none of the new one is
    /// lost (plan 5d-1 final review m3).
    ///
    /// [`Engine::clear_input_peaks`]: crate::engine::Engine::clear_input_peaks
    pub fn clear_input_peaks(&self) {
        self.shared.clear_input_peaks();
    }

    /// Renders one block of `frames` frames into planar `out` channels.
    /// Channels 0–1 get master, the rest zeros. // RT
    pub fn process(&mut self, out: &mut [&mut [f32]], frames: u32, time: BlockTime) {
        self.process_input(out, frames, time, InputBlock::NONE);
    }

    /// [`Renderer::process`] with this block's device input (spec §6.1):
    /// a record tap takes it, and monitored lanes hear it. // RT
    pub fn process_input(
        &mut self,
        out: &mut [&mut [f32]],
        frames: u32,
        time: BlockTime,
        input: InputBlock<'_>,
    ) {
        #[cfg(feature = "rt-check")]
        crate::rt_check::arm();
        self.process_inner(out, frames, time, &input);
        #[cfg(feature = "rt-check")]
        crate::rt_check::disarm();
    }

    /// The taps' end of a block (see [`RecordTap::end_block`] and
    /// [`GestureTap::end_block`]), and the measurement's (it watches every
    /// block's input, played or not: [`MeasureTap::capture`]). // RT
    fn end_tap(&mut self, input: &InputBlock<'_>, frames: u32, advanced: bool) {
        if let Some(t) = self.tap.as_deref_mut() {
            t.end_block(input.channels, frames, advanced);
        }
        if let Some(t) = self.gestures.as_deref_mut() {
            t.end_block(frames, advanced);
        }
        if let Some(m) = self.measure.as_deref_mut() {
            m.capture(input.channels, frames, input.delay_frames);
        }
    }

    fn latency_ns(&self) -> i64 {
        (self.latency as f64 * 1e9 / self.cfg.sample_rate.max(1.0)).round() as i64
    }

    fn process_inner(
        &mut self,
        out: &mut [&mut [f32]],
        frames: u32,
        time: BlockTime,
        input: &InputBlock<'_>,
    ) {
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
        // The input meters see every block, rendered or not.
        for (c, ch) in input.channels.iter().enumerate() {
            self.shared.hold_input(c, block_peak(ch, n));
        }
        if !self.exec.try_finish(deadline) {
            // Nodes of a missed block are still running: touch nothing they
            // use (Plan Ruling 18).
            self.shared.count_busy_block();
            self.pending_skip = self.pending_skip.saturating_add(u64::from(frames));
            // The block was lost to the graph: report it as full load.
            self.shared.set_load(1.0);
            // Its input is still recorded; the transport did not move.
            self.end_tap(input, frames, false);
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
            self.end_tap(input, frames, false);
            return;
        }
        self.block_ns = time.host_time_ns;
        self.drain_commands();
        // A count-in that ends at this block's start rolls the transport.
        let roll = self
            .counting
            .as_mut()
            .and_then(|c| c.rolls(frames).then(|| c.until()));
        if let Some(until) = roll {
            self.roll(until);
        }
        let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
        if let Some(t) = self.tap.as_deref_mut() {
            match (self.transport.state, self.transport.play_pos(length)) {
                (PlayState::Playing, Some(at)) => t.begin_block(at, input.delay_frames),
                // At the end already: the block's `Step::End` stops the tap.
                (PlayState::Playing, None) => {}
                // Stopped or in the tail: the run ends here (pre-flight M2).
                _ => t.stop(0, self.transport.pos),
            }
        }
        // The gesture tap's clock follows the record tap's (Ruling 7).
        if let Some(t) = self.gestures.as_deref_mut() {
            match (self.transport.state, self.transport.play_pos(length)) {
                (PlayState::Playing, Some(at)) => t.begin_block(at),
                (PlayState::Playing, None) => {}
                _ => t.stop(0, self.transport.pos),
            }
        }
        self.live_midi();
        self.live_gestures();
        self.measure_sends();
        let monitored = self.copy_monitored(input, frames);
        // No run is in flight (`try_finish` succeeded above): the streamer
        // may count a new epoch (5a Ruling 9) and plan around the playhead
        // as this block's commands left it (a Locate's new position, never
        // the stale one: commands are applied only here).
        // Led regions read up to the largest lead behind the transport:
        // the streamer plans from the earliest frame read (Ruling 3).
        let lead = self.graph.as_deref().map_or(0, Graph::max_lead);
        let first = self.audio_run(length);
        self.reader.begin_block_seam(
            first.head(lead).unwrap_or(self.transport.pos),
            first.seam(lead),
        );
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
            // Built before the run's length: automation is read at each
            // strip's arrival through it (plan 5e-1 Ruling 5).
            let regions = self.region_run(length, monitored, offset);
            let run = self.next_run(length, frames - offset, &regions);
            let end = offset + run;
            let run_start = offset;
            // A run that starts at or past the end of playing plays out
            // the led regions' last frames (Ruling 3).
            let past_end = self.transport.play_pos(length).is_none()
                && matches!(self.transport.state, PlayState::Playing | PlayState::Tail);
            self.automate(length, run, &regions);
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
                        if let Some(t) = self.tap.as_deref_mut() {
                            t.wrap(offset, to);
                        }
                        if let Some(t) = self.gestures.as_deref_mut() {
                            t.wrap(offset, to);
                        }
                        // A wrap ends held values (plan 5e-1 Ruling 10).
                        self.live.end_holds();
                        // Led regions read the loop's end before its start.
                        let now = self.since_start + u64::from(offset - run_start);
                        self.wrapped = wrapped_at(
                            &self.transport,
                            self.wrapped,
                            now.saturating_sub(self.seg_mark),
                        );
                        self.seg_mark = now;
                        release_all(&mut self.notes, &mut o, offset);
                        if let Some(s) = self.schedule.as_deref() {
                            reset_controllers(s, &mut o, offset);
                        }
                        self.chase_pending = true;
                        self.cursor = self.schedule.as_ref().map_or(0, |s| s.cursor_at(to));
                    }
                    Step::End { offset } => {
                        // The run ends where the sequence does (pre-flight
                        // M2).
                        if let Some(t) = self.tap.as_deref_mut() {
                            t.stop(offset, self.transport.pos);
                        }
                        if let Some(t) = self.gestures.as_deref_mut() {
                            t.stop(offset, self.transport.pos);
                        }
                        release_all(&mut self.notes, &mut o, offset);
                    }
                    Step::Idle { .. } => {}
                }
                at += step.frames();
            }
            let lost = self.render_graph(out, offset, run, info, regions, deadline);
            if regions.playing {
                self.since_start = self.since_start.saturating_add(u64::from(run));
            }
            if past_end {
                self.after_end = self.after_end.saturating_add(u64::from(run));
            }
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
        if let Some(c) = self.counting.as_mut() {
            c.set_latency(self.latency);
            c.render(out, frames);
            clamp_master(out, 0, frames, &self.shared);
            if c.done() {
                self.counting = None;
            }
        }
        self.end_tap(input, frames, !missed);
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
    /// schedule with automation or audio plays (Plan 4d Ruling 10, 5a
    /// Ruling 12). Cut at each lane's next point at its own arrival
    /// (pre-flight I13), and where that lane's read jumps; past the end,
    /// only the lanes still reading (`auto_pos`). // RT
    fn next_run(&self, length: u64, left: u32, regions: &crate::graph::RegionRun) -> u32 {
        let Some(s) = self.schedule.as_deref() else {
            return left;
        };
        if s.automation.is_empty() && s.audio.is_empty() {
            return left;
        }
        let Some((pos, past)) = self.auto_pos(length, regions) else {
            return left;
        };
        let mut run = u64::from(left);
        let g = self.graph.as_deref();
        let auto = &s.automation;
        for lane in auto.lanes() {
            let a = lane_arrival(g, lane);
            if a > 0 {
                // Where this lane's read jumps (its loop seam, the roll's
                // floor, the end), so no run reads across it (Task 4
                // review I1).
                run = run.min(regions.read_span(a, run).1);
            }
            let Some(r) = lane_frame(regions, pos, a, past) else {
                continue;
            };
            let pts = auto.points(lane);
            let i = pts.partition_point(|p| p.frame <= r);
            if let Some(p) = pts.get(i) {
                run = run.min(p.frame.saturating_sub(r).max(u64::from(MIN_RUN)));
            }
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
    /// 16)` frames, each only when it changed. Each lane is read at its
    /// strip's arrival (`auto_frame`, plan 5e-1 Ruling 5).
    ///
    /// Live gestures (plan 5e-1 Rulings 6, 10, 11): a live target's value
    /// replaces its lane's in both commits; live targets no lane set
    /// (`apply_live`) join the same two commits, so neither undoes the
    /// other's ramp; an ended entry's override goes before them; a touched
    /// or held plugin parameter gets no automation; then glides move on
    /// and ended plugin entries have their lanes sent again (pre-flight
    /// I10). // RT
    fn automate(&mut self, length: u64, run: u32, regions: &crate::graph::RegionRun) {
        let at = self.auto_pos(length, regions);
        let auto = self
            .schedule
            .as_deref()
            .map(|s| &s.automation)
            .filter(|a| !a.is_empty());
        let live = &mut *self.live;
        // A mute or solo point inside the run (one shorter than `MIN_RUN`
        // stretched over it) switches at the run boundary nearest to it:
        // the run's start when at most half way in, else the next run's.
        // A fader, pan or send jump inside such a run (Ruling 11's "two
        // points on one frame") is not switched hard: it fades over the
        // run, at most `MIN_RUN` frames (under 1 ms), which avoids a click.
        if let Some(g) = self.graph.as_deref_mut() {
            // An entry ended: its override goes, unless a lane or another
            // entry drives the control.
            let released = live.take_released();
            if released {
                g.release_overrides(|d| auto.is_some_and(|a| a.drives(d)) || live.has(d));
            }
            for ramp in [false, true] {
                let mut any = released;
                let into = if ramp { run } else { 0 };
                if let (Some((pos, past)), Some(auto)) = (at, auto) {
                    for lane in auto.lanes() {
                        let a = lane_arrival(Some(g), lane);
                        let Some(f0) = lane_frame(regions, pos, a, past) else {
                            continue;
                        };
                        let mid = f0.saturating_add(u64::from(run / 2));
                        let end = f0.saturating_add(u64::from(run));
                        let v = match (ramp, lane.dest) {
                            (false, _) => auto.value(lane, f0, false),
                            (true, AutoDest::Strip(_, StripParam::Mute | StripParam::Solo)) => {
                                auto.value(lane, mid, false)
                            }
                            (true, _) => auto.value(lane, end, true),
                        };
                        let under = v;
                        let v = match live.value(lane.dest, v, into) {
                            Some(lv) => {
                                // The next run starts here; offsets and
                                // glides play over this when the lane
                                // reads nothing (Task 5 review I1, I2).
                                if ramp {
                                    live.note(lane.dest, lv, Some(under));
                                }
                                lv
                            }
                            None => v,
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
                }
                any |= apply_live(g, live, auto, regions, at, into, ramp);
                if any {
                    g.commit_controls(ramp);
                }
            }
        }
        if let (Some((pos, past)), Some(auto)) = (at, auto) {
            let every = (run / 16).max(MIN_RUN);
            for (k, lane) in auto.lanes().iter().enumerate() {
                let AutoDest::Plugin(id, param) = lane.dest else {
                    continue;
                };
                // Touched or held: the plugin keeps the user's value (Ruling 6).
                if live.holds(lane.dest) {
                    continue;
                }
                let a = lane_arrival(self.graph.as_deref(), lane);
                let Some(f0) = lane_frame(regions, pos, a, past) else {
                    continue;
                };
                let Some(last) = self.auto_last.get_mut(k) else {
                    break;
                };
                let mut f = 0u32;
                loop {
                    let v = auto.value(lane, f0.saturating_add(u64::from(f)), false);
                    // An offset or a glide plays over the automation.
                    let v = live.value(lane.dest, v, f).unwrap_or(v);
                    // Recorded as sent only once the plugin's queue took it, so
                    // a refused value is sent again on the next run (Plan 4d
                    // final review I1).
                    if v.to_bits() != last.to_bits()
                        && send_param(
                            self.graph.as_deref_mut(),
                            &mut self.sinks,
                            lane.instrument,
                            id,
                            f,
                            param,
                            v,
                        )
                    {
                        *last = v;
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
        live.advance(run);
        // A release ramped back over this run (or a glide ended): its
        // gains already stand at the value under it, so its override is
        // dropped and mirrored now; the next run's commits write the gains
        // (and drop it again, idempotently).
        if live.released() {
            if let Some(g) = self.graph.as_deref_mut() {
                g.release_overrides(|d| auto.is_some_and(|a| a.drives(d)) || live.has(d));
                g.publish_overrides();
            }
        }
        // A plugin parameter let go gets its automation again, even a flat
        // or stepped lane whose value was last sent before the touch
        // (pre-flight I10).
        let last = &mut self.auto_last;
        let all = live.take_ended_plugins(|d| {
            let k = auto.and_then(|a| a.lanes().binary_search_by_key(&d, |l| l.dest).ok());
            if let Some(x) = k.and_then(|k| last.get_mut(k)) {
                *x = f64::NAN;
            }
        });
        if all {
            last.fill(f64::NAN);
        }
    }

    /// Takes the queued gestures into the live table (plan 5e-1 Ruling 6)
    /// and stamps each on the gesture tap (Ruling 7), offset by its host
    /// time as `live_midi` offsets MIDI, and heard the graph latency less
    /// its control's arrival after the renderer: the live value acts at
    /// once at a control that renders the timeline its arrival behind
    /// (Task 5 review M3). // RT
    fn live_gestures(&mut self) {
        let sr = self.cfg.sample_rate;
        let block_ns = self.block_ns;
        let latency = i64::try_from(self.latency).unwrap_or(i64::MAX);
        for _ in 0..crate::gesture::GESTURES_PER_BLOCK {
            let Ok(m) = self.gesture_in.pop() else {
                break;
            };
            if !self.live.apply(m.dest, m.phase) {
                self.shared.count_gesture_drop();
            }
            if let Some(t) = self.gestures.as_deref_mut() {
                let offset = if m.host_ns == 0 {
                    0
                } else {
                    let ns = i128::from(m.host_ns) - i128::from(block_ns);
                    (ns as f64 * sr / 1e9).round() as i64
                };
                let arrival = dest_arrival(self.graph.as_deref(), m.dest);
                let ahead = latency.saturating_sub(i64::try_from(arrival).unwrap_or(i64::MAX));
                t.stamp(m.dest, offset, ahead, m.phase, m.tag);
            }
        }
    }

    /// Where automation reads in the run starting now: the transport's
    /// play position, or (`true`) past the end while the end plays out,
    /// the renderer's frame, where only lanes behind latency still read
    /// (`lane_frame`; Task 4 review M3). `None` while not playing. // RT
    fn auto_pos(&self, length: u64, regions: &crate::graph::RegionRun) -> Option<(u64, bool)> {
        if let Some(p) = self.transport.play_pos(length) {
            return Some((p, false));
        }
        let ending = matches!(self.transport.state, PlayState::Playing | PlayState::Tail);
        (ending && regions.playing).then_some((regions.frame, true))
    }

    /// Drops the overrides of the controls the schedule no longer
    /// automates and no live gesture holds (Plan 4d Ruling 13). // RT
    fn forget_unautomated(&mut self) {
        let Some(g) = self.graph.as_deref_mut() else {
            return;
        };
        let auto = self.schedule.as_deref().map(|s| &s.automation);
        let live = &*self.live;
        g.release_overrides(|d| auto.is_some_and(|a| a.drives(d)) || live.has(d));
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
        regions: crate::graph::RegionRun,
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
        match self.exec.run(
            g,
            &self.sinks,
            &self.shared,
            frames,
            info,
            regions,
            deadline,
        ) {
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
                Ok(EngineCmd::Record(_)) => self.tap.is_some() && self.retire.slots() == 0,
                Ok(EngineCmd::Gestures(_)) => self.gestures.is_some() && self.retire.slots() == 0,
                Ok(EngineCmd::SetInputs(_)) => self.inputs.is_some() && self.retire.slots() == 0,
                Ok(EngineCmd::Measure(_)) => self.measure.is_some() && self.retire.slots() == 0,
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
                    self.transport.end = self
                        .transport
                        .end
                        .map(|e| new.tempo.tick_to_frame(old.tempo.frame_to_tick(e)));
                    let remap = |f: u64| new.tempo.tick_to_frame(old.tempo.frame_to_tick(f));
                    self.roll_floor = remap(self.roll_floor);
                    self.wrapped = self.wrapped.map(|w| crate::graph::Wrapped {
                        start: remap(w.start),
                        end: remap(w.end),
                        ..w
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
                if let Some(t) = self.tap.as_deref_mut() {
                    t.resync();
                }
                if let Some(t) = self.gestures.as_deref_mut() {
                    t.resync();
                }
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
                // What a recording captures from here on is heard with the
                // new latency (final review I1).
                if let Some(t) = self.tap.as_deref_mut() {
                    t.set_latency(self.latency);
                }
                if let Some(t) = self.gestures.as_deref_mut() {
                    t.set_latency(self.latency);
                }
                self.graph = Some(new);
                self.shared.graphs_applied.fetch_add(1, Ordering::Relaxed);
                // New inserts need their automated values.
                self.auto_last.fill(f64::NAN);
            }
            EngineCmd::Play { until, count_in } => {
                match count_in {
                    // Counted in: the transport rolls when it ends.
                    Some(c) if self.transport.state != PlayState::Playing => {
                        self.counting =
                            Some(Counting::new(c, until, self.cfg.sample_rate, self.latency));
                    }
                    // A plain Play during a count-in takes over: the clicks stop.
                    _ => {
                        self.counting = None;
                        self.roll(until);
                    }
                }
            }
            EngineCmd::Stop => {
                // The stop is marked where playback stopped, before any
                // later command of this block moves the transport (review
                // fix M2).
                if self.transport.state == PlayState::Playing {
                    if let Some(t) = self.tap.as_deref_mut() {
                        t.stop(0, self.transport.pos);
                    }
                    if let Some(t) = self.gestures.as_deref_mut() {
                        t.stop(0, self.transport.pos);
                    }
                }
                self.release_now(true);
                self.transport.stop();
                // Held values end at a Stop (plan 5e-1 Ruling 10).
                self.live.end_holds();
                // A count-in still playing never rolls.
                self.counting = None;
            }
            EngineCmd::Locate(frame) => {
                self.release_now(true);
                self.transport.locate(frame);
                // Held values do not play on over the new position (plan
                // 5e-1 Ruling 23).
                self.live.end_holds();
                self.roll_floor = frame;
                self.wrapped = None;
                self.seg_mark = 0;
                self.after_end = 0;
                // A jump while recording is a cut: the next block marks
                // where the transport landed (review fix M1).
                if let Some(t) = self.tap.as_deref_mut() {
                    t.resync();
                }
                if let Some(t) = self.gestures.as_deref_mut() {
                    t.resync();
                }
                self.since_start = 0;
                self.seek();
                self.auto_last.fill(f64::NAN);
            }
            EngineCmd::SetLoop(range) => {
                // The loop wrapped so far stays what led regions read
                // across (m1): it is what was heard.
                self.transport.loop_range = range;
            }
            EngineCmd::RemoveSink(idx) => {
                // End its sounding notes (played, auditioned, played thru)
                // before the sink goes, as Stop does: an external synth
                // would hold them forever (plan 5d-1 Ruling 23; Task 11
                // review I1).
                let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
                let mut o = Out {
                    sinks: &mut self.sinks,
                    shared: &self.shared,
                    block_ns: self.block_ns,
                    ns_per_frame: 0.0,
                    late_ns,
                    base: 0,
                };
                self.notes.release(idx, |m| o.send(idx, 0, m.as_slice()));
                self.audition.release(idx, |m| o.send(idx, 0, m.as_slice()));
                self.thru.release(idx, |m| o.send(idx, 0, m.as_slice()));
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
            EngineCmd::Record(mut new) => {
                // The latency the graph has now, whatever it was when the
                // tap was made (final review I1).
                if let Some(t) = new.as_deref_mut() {
                    t.set_latency(self.latency);
                }
                if let Some(old) = std::mem::replace(&mut self.tap, new) {
                    old.close();
                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Tap(old)) {
                        // Unreachable: room was checked in `drain_commands`.
                        std::mem::forget(old);
                    }
                }
            }
            EngineCmd::Gestures(mut new) => {
                // The latency the graph has now (as `Record`).
                if let Some(t) = new.as_deref_mut() {
                    t.set_latency(self.latency);
                }
                if let Some(old) = std::mem::replace(&mut self.gestures, new) {
                    old.close();
                    if let Err(rtrb::PushError::Full(old)) =
                        self.retire.push(Retired::Gestures(old))
                    {
                        // Unreachable: room was checked in `drain_commands`.
                        std::mem::forget(old);
                    }
                }
            }
            EngineCmd::SetInputs(new) => {
                // Notes thru started end: a lane disarmed or re-routed
                // must not leave one hanging.
                let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
                let mut o = Out {
                    sinks: &mut self.sinks,
                    shared: &self.shared,
                    block_ns: self.block_ns,
                    ns_per_frame: 0.0,
                    late_ns,
                    base: 0,
                };
                release_all(&mut self.thru, &mut o, 0);
                if let Some(old) = self.inputs.replace(new) {
                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Inputs(old))
                    {
                        // Unreachable: room was checked in `drain_commands`.
                        std::mem::forget(old);
                    }
                }
            }
            EngineCmd::Measure(new) => {
                if let Some(mut old) = std::mem::replace(&mut self.measure, new) {
                    // Its note must not hang.
                    let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
                    let mut o = Out {
                        sinks: &mut self.sinks,
                        shared: &self.shared,
                        block_ns: self.block_ns,
                        ns_per_frame: 0.0,
                        late_ns,
                        base: 0,
                    };
                    let target = old.target();
                    old.release(|m| o.send(target, 0, m.as_slice()));
                    old.close();
                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Measure(old))
                    {
                        // Unreachable: room was checked in `drain_commands`.
                        std::mem::forget(old);
                    }
                }
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

    /// The `Regions` nodes' context for the run starting now: the
    /// schedule's audio from the playhead (led nodes read behind it, plan
    /// 5d-1 Ruling 3), late within `LATE_SECONDS` of playing after a Play
    /// or Locate, muted when the device refused the rate, and the
    /// monitored input with the record range while a recording rolls
    /// (each node decides Auto at its read frame, Ruling 4). // RT
    fn region_run(&self, length: u64, monitored: bool, offset: u32) -> crate::graph::RegionRun {
        let base = self.audio_run(length);
        if !monitored {
            return base;
        }
        let Some(routes) = self.inputs.as_deref() else {
            return base;
        };
        let playing = self.transport.state == PlayState::Playing;
        let rec = self
            .tap
            .as_deref()
            .filter(|t| t.rolling())
            .map(RecordTap::range_or_all);
        // SAFETY: the routes (boxed) and the input copy (boxed) change only
        // in `drain_commands`, `copy_monitored` or `prepare`, after
        // `try_finish` succeeded, so they outlive every run given this.
        unsafe { base.with_monitor(&self.input_copy, &routes.monitor, offset, playing, rec) }
    }

    /// The schedule's audio from the playhead, or past the end while
    /// playing plays out (none while stopped). // RT
    fn audio_run(&self, length: u64) -> crate::graph::RegionRun {
        let Some(s) = self.schedule.as_deref() else {
            return crate::graph::RegionRun::NONE;
        };
        let (frame, ceiling) = match self.transport.play_pos(length) {
            Some(p) => (p, u64::MAX),
            // At or past the end: led nodes read on to it (Ruling 3).
            None if matches!(self.transport.state, PlayState::Playing | PlayState::Tail) => (
                self.transport.pos.saturating_add(self.after_end),
                self.transport.pos,
            ),
            None => return crate::graph::RegionRun::NONE,
        };
        // A wrap due at this run's start (the transport sits at the loop
        // end) counts: the run plays from the loop start.
        let lp = self.transport.active_loop();
        let due = lp.is_some_and(|(_, e)| self.transport.pos == e);
        let wrapped = if due {
            wrapped_at(
                &self.transport,
                self.wrapped,
                self.since_start.saturating_sub(self.seg_mark),
            )
        } else {
            self.wrapped
        };
        let run = if s.audio.is_empty() {
            crate::graph::RegionRun::transport(frame, true)
        } else {
            // SAFETY: the schedule (boxed) and the reader (boxed) change
            // only in `drain_commands` or `Drop`, after `try_finish`
            // succeeded, so they outlive every run given this value.
            unsafe {
                crate::graph::RegionRun::new(
                    &s.audio,
                    &self.reader,
                    frame,
                    true,
                    self.since_start < self.late_frames(),
                    self.shared.regions_muted(),
                )
            }
        };
        run.with_bounds(self.roll_floor, ceiling, wrapped, self.since_start)
    }

    /// Copies the input channels monitor routes name; true when there is
    /// one. // RT
    fn copy_monitored(&mut self, input: &InputBlock<'_>, frames: u32) -> bool {
        let Some(routes) = self.inputs.as_deref() else {
            return false;
        };
        if routes.monitor.is_empty() {
            return false;
        }
        let r = &routes.monitor;
        self.input_copy.fill(input.channels, frames, |c| {
            r.iter()
                .any(|m| usize::from(m.left) == c || usize::from(m.right) == c)
        });
        true
    }

    /// Takes the queued MIDI input: each message a lane listens to goes
    /// through its input chain to thru (its destination, now) and, while a
    /// tap rolls and the lane is armed, to the recorder, stamped at the
    /// run frame heard when it arrived (spec §6.3, §6.6). // RT
    fn live_midi(&mut self) {
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let sr = self.cfg.sample_rate;
        let block_ns = self.block_ns;
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns,
            ns_per_frame: 0.0,
            late_ns,
            base: 0,
        };
        let thru = &mut self.thru;
        let mut tap = self.tap.as_deref_mut();
        let mut routes = self.inputs.as_deref_mut();
        for _ in 0..MIDI_IN_PER_BLOCK {
            let Ok(ev) = self.midi_in.pop() else {
                break;
            };
            let Some(routes) = routes.as_deref_mut() else {
                continue;
            };
            let offset = if ev.host_ns == 0 {
                0
            } else {
                let ns = i128::from(ev.host_ns) - i128::from(block_ns);
                (ns as f64 * sr / 1e9).round() as i64
            };
            for r in routes.midi.iter_mut() {
                if !r.matches(&ev) {
                    continue;
                }
                let (spec, targets, ch) = (&r.spec, r.targets, r.dest_channel);
                r.chain.process(ev.msg, &mut |m| {
                    if spec.thru {
                        let m = on_channel(m, ch);
                        for t in targets.iter().flatten() {
                            o.send(*t, 0, m.as_slice());
                            thru.observe(*t, m.as_slice());
                        }
                    }
                    if spec.armed {
                        if let Some(t) = tap.as_deref_mut() {
                            t.midi(spec.lane, offset, m);
                        }
                    }
                });
            }
        }
    }

    /// Sends what a measurement has due (spec §6.7) at block offset 0,
    /// heard the graph latency and the MIDI offset after the block's first
    /// frame. // RT
    fn measure_sends(&mut self) {
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        // Signed: a negative MIDI offset makes a note heard before the
        // block's first frame (fix round 1).
        let late = (late_ns as f64 * self.cfg.sample_rate / 1e9).round() as i64;
        let Some(m) = self.measure.as_deref_mut() else {
            return;
        };
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns: self.block_ns,
            ns_per_frame: 0.0,
            late_ns,
            base: 0,
        };
        let target = m.target();
        m.sends(late, |msg| o.send(target, 0, msg.as_slice()));
    }

    /// [`LATE_SECONDS`] in frames.
    fn late_frames(&self) -> u64 {
        (LATE_SECONDS * self.cfg.sample_rate.max(0.0)) as u64
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

    /// Starts playing (a Play, or the end of its count-in). // RT
    fn roll(&mut self, until: Option<u64>) {
        let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
        self.transport.end = until;
        let rolls = self.transport.state != PlayState::Playing;
        if rolls {
            self.since_start = 0;
        }
        self.transport.play(length);
        if rolls {
            self.roll_floor = self.transport.pos;
            self.wrapped = None;
            self.seg_mark = 0;
            self.after_end = 0;
        }
        self.seek();
        self.forget_unautomated();
        self.auto_last.fill(f64::NAN);
    }

    fn publish(&mut self) {
        let s = &self.shared;
        s.counting.store(
            self.counting.is_some_and(|c| !c.rolled()),
            Ordering::Relaxed,
        );
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

/// The frame automation is read at for a control whose audio is `arrival`
/// frames behind the renderer (plan 5e-1 Ruling 5): where a led region
/// with that lead reads, else that far back. // RT
fn auto_frame(run: &crate::graph::RegionRun, pos: u64, arrival: u64) -> u64 {
    if arrival == 0 {
        return pos;
    }
    run.read_frame(arrival)
        .unwrap_or(pos.saturating_sub(arrival))
}

/// The frame a lane with arrival `arrival` reads at in the run starting
/// at `pos`: `auto_frame`; past the end (`past`), only a lane behind
/// latency reads, and only while its audio still plays (Task 4 review
/// M3). // RT
fn lane_frame(run: &crate::graph::RegionRun, pos: u64, arrival: u64, past: bool) -> Option<u64> {
    if !past {
        return Some(auto_frame(run, pos, arrival));
    }
    if arrival == 0 {
        return None;
    }
    run.read_frame(arrival)
}

/// A lane's arrival (plan 5e-1 Ruling 5): a strip's fader and pan at its
/// strip's (they scale the strip's output); its mute and solo at its main
/// output's edge and a send's level at that send's edge (they are edge
/// gains, applied after the edge's delay line); an insert's parameter at
/// the insert's; a plugin instrument's parameter (and every lane without a
/// graph) at 0. // RT
fn lane_arrival(graph: Option<&Graph>, lane: &crate::schedule::AutoLane) -> u64 {
    if matches!(lane.dest, AutoDest::Plugin(..)) && lane.instrument.is_some() {
        return 0;
    }
    dest_arrival(graph, lane.dest)
}

/// `dest`'s arrival, as [`lane_arrival`] reads its lane: a plugin
/// instrument's parameter is no insert's, so it is 0 here too. // RT
fn dest_arrival(graph: Option<&Graph>, dest: AutoDest) -> u64 {
    let Some(g) = graph else {
        return 0;
    };
    match dest {
        AutoDest::Strip(s, StripParam::Fader | StripParam::Pan) => g.strip_arrival(s),
        AutoDest::Strip(s, StripParam::Mute | StripParam::Solo) => g.feed_arrival(s, None),
        AutoDest::Send(s, i) => g.feed_arrival(s, Some(i)),
        AutoDest::Plugin(id, _) => g.insert_arrival(id),
    }
}

/// Sets, for the next commit, the value of every live strip and send
/// target this run's lanes did not set (no lane drives it, its lane reads
/// nothing past the end, or the transport is not playing: `at` is `None`);
/// true when one was set. Its value plays over the static value, or, for a
/// target a lane drives, over what was last heard under it (Task 5 review
/// I1). The ramped commit's value is noted as the next run's start (I2).
/// // RT
fn apply_live(
    g: &mut Graph,
    live: &mut crate::gesture::LiveTable,
    auto: Option<&crate::schedule::Automation>,
    regions: &crate::graph::RegionRun,
    at: Option<(u64, bool)>,
    into: u32,
    ramp: bool,
) -> bool {
    let mut any = false;
    for i in 0..live.len() {
        let Some(dest) = live.dest_at(i) else {
            break;
        };
        if matches!(dest, AutoDest::Plugin(..)) {
            continue;
        }
        let lane = auto.and_then(|a| {
            let lanes = a.lanes();
            lanes
                .binary_search_by_key(&dest, |l| l.dest)
                .ok()
                .and_then(|i| lanes.get(i))
        });
        if let (Some((pos, past)), Some(l)) = (at, lane) {
            if lane_frame(regions, pos, lane_arrival(Some(g), l), past).is_some() {
                continue;
            }
        }
        let under = if lane.is_some() {
            match live.base(dest) {
                Some(b) => Some(b),
                None => {
                    let h = g.heard_value(dest);
                    if let Some(h) = h {
                        live.set_base(dest, h);
                    }
                    h
                }
            }
        } else {
            g.static_value(dest)
        };
        let Some(v) = under.and_then(|under| live.value(dest, under, into)) else {
            continue;
        };
        match dest {
            AutoDest::Strip(id, p) => g.automate_strip(id, p, v),
            AutoDest::Send(id, i) => g.automate_send(id, i, v),
            AutoDest::Plugin(..) => continue,
        }
        if ramp {
            live.note(dest, v, None);
        }
        any = true;
    }
    any
}

/// Sends a plugin parameter's automated value: to its plugin
/// instrument's sink when it has one, else to its insert in the graph.
/// False when the plugin refused it (its queue is full); a missing target
/// counts as sent.
/// // RT
fn send_param(
    graph: Option<&mut Graph>,
    sinks: &mut SinkTable,
    instrument: Option<TargetIdx>,
    id: PluginInstanceId,
    frame: u32,
    param: ParamId,
    v: f64,
) -> bool {
    if let Some(t) = instrument {
        if let Some(Some(Sink::Instrument(node))) = sinks.get_mut(usize::from(t.0)) {
            return node.automate(frame, param, v);
        }
        return true;
    }
    match graph.and_then(|g| g.insert_mut(id)) {
        Some(node) => node.automate(frame, param, v),
        None => true,
    }
}

/// Note-offs (and pedal-ups) for every target at block offset `offset`. // RT
fn release_all(notes: &mut ActiveNotes, o: &mut Out<'_>, offset: u32) {
    for t in 0..MAX_TARGETS {
        let target = TargetIdx(t as u8);
        notes.release(target, |m| o.send(target, offset, m.as_slice()));
    }
}
