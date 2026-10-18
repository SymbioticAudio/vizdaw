//! The insert seam: `EffectNode` processors made by an `InsertFactory`
//! (Plan Ruling 12). 3a ships only test nodes; 3b wraps plugin instances.

use crate::backend::{BlockInfo, InstrumentNode, StereoBuf};
use crate::graph::delay::DelayLine;
use crate::graph::dsp;
use core_model::ids::PluginInstanceId;
use core_model::mixer::{ParamId, PluginFormat, PluginRef, PluginSlot};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// What a processor reported for one block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    /// Processed normally.
    Ok,
    /// The processor failed; counted like a faulty block (spec §7).
    Error,
}

/// An insert processor. `latency` is read on the control thread when the
/// node is created; the other methods run on render threads.
pub trait EffectNode: Send {
    /// Processing latency in frames.
    fn latency(&self) -> u32;
    /// Processes the first `frames` frames of `io` in place; `sidechain` is
    /// the aligned sidechain input when one is routed. // RT
    fn process(
        &mut self,
        frames: u32,
        io: &mut StereoBuf,
        sidechain: Option<&StereoBuf>,
    ) -> ProcessStatus;
    /// Clears internal state (delay memory, envelopes). // RT
    fn reset(&mut self);

    /// Automation's value for parameter `param`, normalised 0–1, at `frame`
    /// of the next block; plugin processors map it to the parameter's range
    /// (Plan 4d Ruling 12). // RT
    fn automate(&mut self, frame: u32, param: ParamId, normalized: f64) {
        let _ = (frame, param, normalized);
    }

    /// The block went by without this insert running (bypassed or faulted):
    /// drops automation queued for it, so stale values are not applied
    /// when it returns. // RT
    fn skipped(&mut self) {}

    /// [`EffectNode::process`] with the block's transport facts; plugin
    /// processors override this. // RT
    fn process_block(
        &mut self,
        frames: u32,
        io: &mut StereoBuf,
        sidechain: Option<&StereoBuf>,
        info: &BlockInfo,
    ) -> ProcessStatus {
        let _ = info;
        self.process(frames, io, sidechain)
    }
}

/// Creates plugin processors on the control thread: inserts, and the
/// instruments of plugin instrument destinations.
pub trait InsertFactory: Send {
    /// The processor for `slot` at the stream's rate and largest block.
    /// `Err` leaves the slot a placeholder (audio passes, no latency) and is
    /// reported in `GraphReport::placeholders`.
    fn make(
        &mut self,
        slot: &PluginSlot,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn EffectNode>, String>;

    /// The instrument for a plugin instrument destination's `slot`. `Err`
    /// leaves the destination unroutable (reported in the compile
    /// warnings) and its strip silent.
    fn make_instrument(
        &mut self,
        slot: &PluginSlot,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn InstrumentNode>, String> {
        let _ = (sample_rate, max_frames);
        Err(format!(
            "{} cannot load until plugin hosting exists",
            slot.plugin.name
        ))
    }

    /// Whether parameter `param` of instance `slot` is stepped: its
    /// automation holds instead of ramping (Plan 4d Ruling 8). Unknown
    /// parameters are not.
    fn param_stepped(&self, slot: PluginInstanceId, param: ParamId) -> bool {
        let _ = (slot, param);
        false
    }
}

/// The factory before plugin hosting: every slot is a placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoInserts;

impl InsertFactory for NoInserts {
    fn make(&mut self, slot: &PluginSlot, _: f64, _: u32) -> Result<Box<dyn EffectNode>, String> {
        Err(format!(
            "{} cannot load until plugin hosting exists",
            slot.plugin.name
        ))
    }
}

/// What a test node does.
#[derive(Debug, Clone)]
pub enum TestMode {
    /// `out = in × gain`.
    Gain(f32),
    /// `out = in + level × noise` (xorshift32 from `seed`, per sample).
    Noise {
        /// Generator seed.
        seed: u32,
        /// Amplitude.
        level: f32,
    },
    /// `out = in × (1 − min(1, peak(sidechain)))` per block (a crude ducker).
    Duck,
    /// `out = in + sidechain` (makes sidechain alignment visible).
    AddSidechain,
    /// Writes NaN into the first left sample.
    Nan,
    /// Passes audio through but reports `ProcessStatus::Error`.
    Error,
    /// Spins until the flag is set (at most 5 s), then passes audio through.
    Stall(Arc<AtomicBool>),
    /// Adds a unit impulse at the very first frame it processes.
    Impulse,
}

/// A test stand-in for a plugin: a [`TestMode`] followed by a fixed
/// latency.
#[derive(Debug, Clone)]
pub struct TestNode {
    mode: TestMode,
    latency: u32,
    delay: DelayLine,
    rng: u32,
    fired: bool,
}

impl TestNode {
    /// A node of `mode` delayed by `latency` frames.
    pub fn new(mode: TestMode, latency: u32) -> TestNode {
        let rng = match &mode {
            TestMode::Noise { seed, .. } => *seed | 1,
            _ => 1,
        };
        TestNode {
            mode,
            latency,
            delay: DelayLine::new(latency),
            rng,
            fired: false,
        }
    }
}

/// xorshift32 → [-1, 1). // RT
fn noise(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

impl EffectNode for TestNode {
    fn latency(&self) -> u32 {
        self.latency
    }

    fn process(
        &mut self,
        frames: u32,
        io: &mut StereoBuf,
        sidechain: Option<&StereoBuf>,
    ) -> ProcessStatus {
        let n = (frames as usize).min(io.left.len()).min(io.right.len());
        let mut status = ProcessStatus::Ok;
        let duck = match (&self.mode, sidechain) {
            (TestMode::Duck, Some(sc)) => 1.0 - dsp::peak(sc, frames).min(1.0),
            _ => 1.0,
        };
        let (Some(l), Some(r)) = (io.left.get_mut(..n), io.right.get_mut(..n)) else {
            return status;
        };
        match &self.mode {
            TestMode::Gain(g) => {
                for x in l.iter_mut().chain(r.iter_mut()) {
                    *x *= *g;
                }
            }
            TestMode::Noise { level, .. } => {
                for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                    let v = noise(&mut self.rng) * *level;
                    *a += v;
                    *b += v;
                }
            }
            TestMode::Duck => {
                for x in l.iter_mut().chain(r.iter_mut()) {
                    *x *= duck;
                }
            }
            TestMode::AddSidechain => {
                if let Some(sc) = sidechain {
                    let (sl, sr) = (
                        sc.left.get(..n).unwrap_or(&[]),
                        sc.right.get(..n).unwrap_or(&[]),
                    );
                    for (a, s) in l.iter_mut().zip(sl) {
                        *a += *s;
                    }
                    for (b, s) in r.iter_mut().zip(sr) {
                        *b += *s;
                    }
                }
            }
            TestMode::Nan => {
                if let Some(x) = l.first_mut() {
                    *x = f32::NAN;
                }
            }
            TestMode::Error => status = ProcessStatus::Error,
            TestMode::Stall(flag) => {
                let start = Instant::now();
                while !flag.load(Ordering::Acquire) && start.elapsed() < Duration::from_secs(5) {
                    std::hint::spin_loop();
                }
            }
            TestMode::Impulse => {
                if !self.fired {
                    self.fired = true;
                    if let (Some(a), Some(b)) = (l.first_mut(), r.first_mut()) {
                        *a += 1.0;
                        *b += 1.0;
                    }
                }
            }
        }
        self.delay.process(l, r);
        status
    }

    fn reset(&mut self) {
        self.delay.clear();
    }
}

/// Builds [`TestNode`]s from CLAP ids `vizdaw.test:<key>[=<value>],…`
/// (Plan Ruling 12). Clones share the `made` counter.
#[derive(Debug, Clone, Default)]
pub struct TestInserts {
    stall: Option<Arc<AtomicBool>>,
    made: Arc<AtomicUsize>,
}

impl TestInserts {
    /// The id prefix of test plugins.
    pub const PREFIX: &'static str = "vizdaw.test:";

    /// A factory without a stall flag (`stall` ids are refused).
    pub fn new() -> TestInserts {
        TestInserts::default()
    }

    /// A factory whose `stall` nodes wait for `flag`.
    pub fn with_stall(flag: Arc<AtomicBool>) -> TestInserts {
        TestInserts {
            stall: Some(flag),
            made: Arc::default(),
        }
    }

    /// The reference of test plugin `spec`, e.g. `"gain=0.5,latency=64"`.
    pub fn plugin(spec: &str) -> PluginRef {
        PluginRef {
            format: PluginFormat::Clap,
            id: format!("{}{spec}", Self::PREFIX),
            name: format!("Test {spec}"),
            vendor: "VizDAW".into(),
            version: "1".into(),
        }
    }

    /// Nodes created so far by this factory and its clones.
    pub fn made(&self) -> usize {
        self.made.load(Ordering::Relaxed)
    }

    /// Parses a test plugin id (default mode `gain=1`, latency 0).
    pub fn parse(&self, id: &str) -> Result<TestNode, String> {
        let spec = id
            .strip_prefix(Self::PREFIX)
            .ok_or_else(|| format!("{id:?} is not a test plugin"))?;
        let mut mode = TestMode::Gain(1.0);
        let mut latency = 0u32;
        for part in spec.split(',').filter(|p| !p.is_empty()) {
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            let bad = || format!("bad value {v:?} for {k:?}");
            match k {
                "gain" => mode = TestMode::Gain(v.parse().map_err(|_| bad())?),
                "latency" => latency = v.parse().map_err(|_| bad())?,
                "noise" => {
                    mode = TestMode::Noise {
                        seed: v.parse().map_err(|_| bad())?,
                        level: 0.25,
                    }
                }
                "duck" => mode = TestMode::Duck,
                "sc" => mode = TestMode::AddSidechain,
                "nan" => mode = TestMode::Nan,
                "error" => mode = TestMode::Error,
                "impulse" => mode = TestMode::Impulse,
                "stall" => {
                    let flag = self.stall.clone().ok_or("this factory has no stall flag")?;
                    mode = TestMode::Stall(flag);
                }
                other => return Err(format!("unknown test plugin key {other:?}")),
            }
        }
        Ok(TestNode::new(mode, latency))
    }
}

impl InsertFactory for TestInserts {
    fn make(&mut self, slot: &PluginSlot, _: f64, _: u32) -> Result<Box<dyn EffectNode>, String> {
        let n = self.parse(&slot.plugin.id)?;
        self.made.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(n))
    }

    fn make_instrument(
        &mut self,
        slot: &PluginSlot,
        _: f64,
        max_frames: u32,
    ) -> Result<Box<dyn InstrumentNode>, String> {
        let n = TestInstrument::parse(&slot.plugin.id, max_frames)?;
        self.made.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(n))
    }
}

/// A test stand-in for a plugin instrument, from CLAP ids
/// `vizdaw.test:synth[,outputs=N][,latency=L]`: while a note on MIDI
/// channel `c` is held, output `c % N` carries the constant
/// `0.1 × (c % N + 1)` (sample-exact from the note's offset), delayed by
/// `latency` frames.
#[derive(Debug, Clone)]
pub struct TestInstrument {
    outputs: u16,
    latency: u32,
    /// Held notes per output.
    held: [u32; 16],
    /// `(offset, output, on)` of this block's notes, in arrival order.
    queue: Vec<(u32, u16, bool)>,
    delay: Vec<DelayLine>,
    max_frames: u32,
}

impl TestInstrument {
    /// Parses a `vizdaw.test:synth…` id.
    pub fn parse(id: &str, max_frames: u32) -> Result<TestInstrument, String> {
        let spec = id
            .strip_prefix(TestInserts::PREFIX)
            .ok_or_else(|| format!("{id:?} is not a test plugin"))?;
        let mut outputs = 1u16;
        let mut latency = 0u32;
        let mut synth = false;
        for part in spec.split(',').filter(|p| !p.is_empty()) {
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            let bad = || format!("bad value {v:?} for {k:?}");
            match k {
                "synth" => synth = true,
                "outputs" => outputs = v.parse().map_err(|_| bad())?,
                "latency" => latency = v.parse().map_err(|_| bad())?,
                other => return Err(format!("unknown test instrument key {other:?}")),
            }
        }
        if !synth {
            return Err(format!("{id:?} is not a test instrument"));
        }
        let outputs = outputs.clamp(1, 16);
        Ok(TestInstrument {
            outputs,
            latency,
            held: [0; 16],
            queue: Vec::with_capacity(256),
            delay: (0..outputs).map(|_| DelayLine::new(latency)).collect(),
            max_frames,
        })
    }

    /// The reference of test instrument `spec`, e.g. `"synth,outputs=4"`.
    pub fn plugin(spec: &str) -> PluginRef {
        TestInserts::plugin(spec)
    }

    /// Output count.
    pub fn output_count(&self) -> u16 {
        self.outputs
    }

    /// Renders output `k` into `out` from the held state, applying this
    /// block's queued notes for that output at their offsets. // RT
    fn render_one(&mut self, k: u16, frames: u32, out: &mut StereoBuf) {
        let n = (frames as usize).min(out.left.len()).min(out.right.len());
        let level = 0.1 * f32::from(k + 1);
        let mut held = self.held.get(usize::from(k)).copied().unwrap_or(0);
        let mut at = 0usize;
        let queue = std::mem::take(&mut self.queue);
        for &(off, o, on) in queue.iter().filter(|(_, o, _)| *o == k) {
            let _ = o;
            let end = (off as usize).min(n);
            let v = if held > 0 { level } else { 0.0 };
            if let (Some(l), Some(r)) = (out.left.get_mut(at..end), out.right.get_mut(at..end)) {
                l.fill(v);
                r.fill(v);
            }
            at = at.max(end);
            held = if on { held + 1 } else { held.saturating_sub(1) };
        }
        self.queue = queue;
        let v = if held > 0 { level } else { 0.0 };
        if let (Some(l), Some(r)) = (out.left.get_mut(at..n), out.right.get_mut(at..n)) {
            l.fill(v);
            r.fill(v);
        }
        if let Some(h) = self.held.get_mut(usize::from(k)) {
            *h = held;
        }
        if let (Some(d), Some(l), Some(r)) = (
            self.delay.get_mut(usize::from(k)),
            out.left.get_mut(..n),
            out.right.get_mut(..n),
        ) {
            d.process(l, r);
        }
    }
}

impl InstrumentNode for TestInstrument {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        let (Some(&status), Some(&vel)) = (msg.first(), msg.get(2)) else {
            return;
        };
        let out = u16::from(status & 0x0F) % self.outputs;
        let on = match status & 0xF0 {
            0x90 if vel > 0 => true,
            0x90 | 0x80 => false,
            _ => return,
        };
        if self.queue.len() < self.queue.capacity() {
            self.queue.push((offset.min(self.max_frames), out, on));
        }
    }

    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        self.render_one(0, frames, out);
        // Outputs without a buffer still consume their notes.
        for k in 1..self.outputs {
            let mut held = self.held.get(usize::from(k)).copied().unwrap_or(0);
            for &(_, o, on) in self.queue.iter().filter(|(_, o, _)| *o == k) {
                let _ = o;
                held = if on { held + 1 } else { held.saturating_sub(1) };
            }
            if let Some(h) = self.held.get_mut(usize::from(k)) {
                *h = held;
            }
        }
        self.queue.clear();
    }

    fn reset(&mut self) {
        self.held = [0; 16];
        self.queue.clear();
        for d in &mut self.delay {
            d.clear();
        }
    }

    fn latency(&self) -> u32 {
        self.latency
    }

    fn outputs(&self) -> u16 {
        self.outputs
    }

    fn render_outputs(&mut self, frames: u32, outs: &mut [StereoBuf], _: &BlockInfo) {
        for k in 0..self.outputs {
            match outs.get_mut(usize::from(k)) {
                Some(o) => self.render_one(k, frames, o),
                None => {
                    let mut dummy = StereoBuf::new(0);
                    self.render_one(k, 0, &mut dummy);
                }
            }
        }
        self.queue.clear();
    }
}
