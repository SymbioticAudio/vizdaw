//! The real-time renderer and the state it publishes.

use crate::backend::{BlockTime, InstrumentNode, MidiOut, StereoBuf};
use crate::graph::{MAX_INSERTS, MAX_NODES};
use crate::notes::ActiveNotes;
use crate::schedule::{MAX_TARGETS, Schedule, TargetIdx};
use crate::transport::{PlayState, Step, Transport};
use core_model::ids::PluginInstanceId;
use rtrb::{Consumer, Producer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

/// Real-time fault bits, cleared when read by [`crate::engine::Engine::faults`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Faults(pub u32);

impl Faults {
    /// A block was larger than the prepared size (rendered as silence).
    pub const XRUN: Faults = Faults(1);
    /// The master bus was clamped at ±1.0.
    pub const CLIPPED: Faults = Faults(2);
    /// A MIDI output queue was full; a message was dropped.
    pub const MIDI_DROPPED: Faults = Faults(4);
    /// A schedule swap waited a block because the retire ring was full.
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
    /// Blocks in which a schedule swap was deferred.
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
    state: AtomicU8,
    faults: AtomicU32,
    ended: AtomicBool,
    midi_dropped: AtomicU64,
    swaps_deferred: AtomicU64,
    xruns: AtomicU64,
    deadline_misses: AtomicU64,
    busy_blocks: AtomicU64,
    plugin_faults: AtomicU64,
    load_last: AtomicU32,
    load_peak: AtomicU32,
    meters: Box<[MeterCell]>,
    board: Box<[AtomicU64]>,
}

impl Default for Shared {
    fn default() -> Shared {
        Shared {
            position: AtomicU64::new(0),
            state: AtomicU8::new(0),
            faults: AtomicU32::new(0),
            ended: AtomicBool::new(false),
            midi_dropped: AtomicU64::new(0),
            swaps_deferred: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            deadline_misses: AtomicU64::new(0),
            busy_blocks: AtomicU64::new(0),
            plugin_faults: AtomicU64::new(0),
            load_last: AtomicU32::new(0),
            load_peak: AtomicU32::new(0),
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

    /// Schedule frame of the transport.
    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    /// Transport state.
    pub fn state(&self) -> PlayState {
        PlayState::from_u8(self.state.load(Ordering::Relaxed))
    }

    /// True once after playback reached the end and the tail finished.
    pub fn take_ended(&self) -> bool {
        self.ended.swap(false, Ordering::Relaxed)
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
        }
    }

    /// The meter of graph node `i`. // RT
    pub fn meter(&self, i: usize) -> Option<&MeterCell> {
        self.meters.get(i)
    }

    /// Records that the insert at board index `slot` was bypassed for
    /// faults. // RT
    pub fn report_plugin_fault(&self, slot: usize, instance: Option<PluginInstanceId>) {
        self.raise(Faults::PLUGIN_FAULT);
        self.plugin_faults.fetch_add(1, Ordering::Relaxed);
        if let (Some(cell), Some(id)) = (self.board.get(slot), instance) {
            cell.store(id.0.saturating_add(1), Ordering::Relaxed);
        }
    }

    /// Instances bypassed for faults since the last call (read and
    /// cleared).
    pub fn take_plugin_faults(&self) -> Vec<PluginInstanceId> {
        self.board
            .iter()
            .filter_map(|c| {
                let v = c.swap(0, Ordering::Relaxed);
                (v != 0).then(|| PluginInstanceId(v - 1))
            })
            .collect()
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
    /// An instrument rendered into the mix.
    Instrument(Box<dyn InstrumentNode>),
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
    /// Start playing.
    Play,
    /// Stop (note-offs are sent).
    Stop,
    /// Move to a schedule frame.
    Locate(u64),
    /// Set or clear the loop range.
    SetLoop(Option<(u64, u64)>),
    /// Install the sink of a new target (targets are only appended).
    AddSink(TargetIdx, Sink),
}

/// Capacity of the command ring.
pub const CMD_CAPACITY: usize = 64;
/// Capacity of the retire ring.
pub const RETIRE_CAPACITY: usize = 8;

/// Renderer settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderConfig {
    /// Stream sample rate.
    pub sample_rate: f64,
    /// Largest block (≤ [`crate::backend::MAX_FRAMES`] is typical).
    pub max_frames: u32,
    /// Added to every MIDI-out timestamp.
    pub midi_offset_ns: i64,
}

/// The real-time object. Built by [`crate::engine::Engine::new`], moved to
/// the audio thread by a backend and handed back on stop.
pub struct Renderer {
    cfg: RenderConfig,
    cmds: Consumer<EngineCmd>,
    retire: Producer<Box<Schedule>>,
    shared: Arc<Shared>,
    schedule: Option<Box<Schedule>>,
    cursor: usize,
    transport: Transport,
    notes: ActiveNotes,
    sinks: [Option<Sink>; MAX_TARGETS],
    bufs: Vec<StereoBuf>,
    block_ns: u64,
    /// Controllers were reset (stop, locate, wrap or swap): the next played
    /// frames start with the chased state.
    chase_pending: bool,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("cfg", &self.cfg)
            .field("transport", &self.transport)
            .finish_non_exhaustive()
    }
}

/// Everything `send` needs, borrowed apart from the note tracker.
struct Out<'a> {
    sinks: &'a mut [Option<Sink>; MAX_TARGETS],
    shared: &'a Shared,
    block_ns: u64,
    ns_per_frame: f64,
    midi_offset_ns: i64,
}

impl Out<'_> {
    /// Sends `bytes` to `target` at block offset `offset`. // RT
    fn send(&mut self, target: TargetIdx, offset: u32, bytes: &[u8]) {
        match self.sinks.get_mut(usize::from(target.0)) {
            Some(Some(Sink::Instrument(node))) => node.midi(offset, bytes),
            Some(Some(Sink::Midi(out))) => {
                let at = self.block_ns as i64
                    + (f64::from(offset) * self.ns_per_frame).round() as i64
                    + self.midi_offset_ns;
                if !out.send(at.max(0) as u64, bytes) {
                    self.shared.raise(Faults::MIDI_DROPPED);
                    self.shared.midi_dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }
}

impl Renderer {
    pub(crate) fn new(
        cfg: RenderConfig,
        cmds: Consumer<EngineCmd>,
        retire: Producer<Box<Schedule>>,
        shared: Arc<Shared>,
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
            sinks: std::array::from_fn(|_| None),
            bufs: Vec::new(),
            block_ns: 0,
            chase_pending: false,
        };
        r.prepare(cfg.max_frames);
        r
    }

    /// (Re)allocates scratch buffers for blocks of up to `max_frames`.
    /// Not real-time safe: call only while stopped.
    pub fn prepare(&mut self, max_frames: u32) {
        self.cfg.max_frames = max_frames;
        self.bufs = (0..MAX_TARGETS)
            .map(|_| StereoBuf::new(max_frames))
            .collect();
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
    /// Channels 0–1 get the mix, the rest zeros. // RT
    pub fn process(&mut self, out: &mut [&mut [f32]], frames: u32, time: BlockTime) {
        #[cfg(feature = "rt-check")]
        crate::rt_check::arm();
        self.process_inner(out, frames, time);
        #[cfg(feature = "rt-check")]
        crate::rt_check::disarm();
    }

    fn process_inner(&mut self, out: &mut [&mut [f32]], frames: u32, time: BlockTime) {
        let n = frames as usize;
        if frames > self.cfg.max_frames || out.iter().any(|c| c.len() < n) {
            for ch in out.iter_mut() {
                ch.fill(0.0);
            }
            for sink in self.sinks.iter_mut().flatten() {
                if let Sink::Instrument(node) = sink {
                    node.skip(frames);
                }
            }
            self.shared.raise(Faults::XRUN);
            self.shared.xruns.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.block_ns = time.host_time_ns;
        self.drain_commands();
        let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns: time.host_time_ns,
            ns_per_frame: 1e9 / self.cfg.sample_rate.max(1.0),
            midi_offset_ns: self.cfg.midi_offset_ns,
        };
        let mut offset = 0u32;
        while offset < frames {
            let step = self.transport.next(length, offset, frames - offset);
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
                            let off =
                                offset + u32::try_from(e.frame.saturating_sub(from)).unwrap_or(0);
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
            offset += step.frames();
        }
        self.mix(out, frames);
        self.publish();
    }

    /// Applies queued commands. A `SetSchedule` waits when the retire ring
    /// is full (and everything after it waits too, to keep order). // RT
    fn drain_commands(&mut self) {
        loop {
            let deferred = match self.cmds.peek() {
                Ok(EngineCmd::SetSchedule(_)) => {
                    self.schedule.is_some() && self.retire.slots() == 0
                }
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

    /// Note-offs, then a controller reset on every channel of the current
    /// schedule, at block offset 0; the next played frames chase the
    /// controller state (ruling O23). // RT
    fn release_now(&mut self) {
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns: self.block_ns,
            ns_per_frame: 0.0,
            midi_offset_ns: self.cfg.midi_offset_ns,
        };
        release_all(&mut self.notes, &mut o, 0);
        if let Some(s) = self.schedule.as_deref() {
            reset_controllers(s, &mut o, 0);
        }
        self.chase_pending = true;
    }

    fn apply(&mut self, cmd: EngineCmd) {
        match cmd {
            EngineCmd::SetSchedule(new) => {
                self.release_now();
                if let Some(old) = self.schedule.take() {
                    let tick = old.tempo.frame_to_tick(self.transport.pos);
                    self.transport.pos = new.tempo.tick_to_frame(tick);
                    self.transport.loop_range = self.transport.loop_range.map(|(s, e)| {
                        (
                            new.tempo.tick_to_frame(old.tempo.frame_to_tick(s)),
                            new.tempo.tick_to_frame(old.tempo.frame_to_tick(e)),
                        )
                    });
                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(old) {
                        // Unreachable: room was checked in `drain_commands`.
                        // Leaking is the only RT-safe choice.
                        std::mem::forget(old);
                    }
                }
                self.cursor = new.cursor_at(self.transport.pos);
                self.schedule = Some(new);
            }
            EngineCmd::Play => {
                let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
                self.transport.play(length);
                self.seek();
            }
            EngineCmd::Stop => {
                self.release_now();
                self.transport.stop();
            }
            EngineCmd::Locate(frame) => {
                self.release_now();
                self.transport.locate(frame);
                self.seek();
            }
            EngineCmd::SetLoop(range) => self.transport.loop_range = range,
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

    fn seek(&mut self) {
        self.cursor = self
            .schedule
            .as_ref()
            .map_or(0, |s| s.cursor_at(self.transport.pos));
    }

    /// Renders instrument sinks and sums them into channels 0–1. // RT
    fn mix(&mut self, out: &mut [&mut [f32]], frames: u32) {
        let n = frames as usize;
        for ch in out.iter_mut() {
            if let Some(c) = ch.get_mut(..n) {
                c.fill(0.0);
            }
        }
        let mut clipped = false;
        for (sink, buf) in self.sinks.iter_mut().zip(self.bufs.iter_mut()) {
            let Some(Sink::Instrument(node)) = sink else {
                continue;
            };
            node.render(frames, buf);
            for (i, src) in [&buf.left, &buf.right].into_iter().enumerate() {
                let (Some(dst), Some(src)) =
                    (out.get_mut(i).and_then(|c| c.get_mut(..n)), src.get(..n))
                else {
                    continue;
                };
                for (d, s) in dst.iter_mut().zip(src) {
                    *d += *s;
                }
            }
        }
        for ch in out.iter_mut().take(2) {
            if let Some(c) = ch.get_mut(..n) {
                for x in c {
                    // NaN fails both comparisons below; silence any
                    // non-finite sample and report it as clipping.
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
            self.shared.raise(Faults::CLIPPED);
        }
    }

    fn publish(&mut self) {
        let s = &self.shared;
        s.position.store(self.transport.pos, Ordering::Relaxed);
        s.state.store(self.transport.state as u8, Ordering::Relaxed);
        if self.transport.take_ended() {
            s.ended.store(true, Ordering::Relaxed);
        }
    }

    /// Resets every instrument (all notes off). Call while stopped.
    pub fn reset_instruments(&mut self) {
        for sink in self.sinks.iter_mut().flatten() {
            if let Sink::Instrument(node) = sink {
                node.reset();
            }
        }
    }

    /// Mutable access to a target's sink (for backends and tests, not RT).
    pub fn sink_mut(&mut self, idx: TargetIdx) -> Option<&mut Sink> {
        self.sinks
            .get_mut(usize::from(idx.0))
            .and_then(Option::as_mut)
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

/// Note-offs (and pedal-ups) for every target at block offset `offset`. // RT
fn release_all(notes: &mut ActiveNotes, o: &mut Out<'_>, offset: u32) {
    for t in 0..MAX_TARGETS {
        let target = TargetIdx(t as u8);
        notes.release(target, |m| o.send(target, offset, m.as_slice()));
    }
}
