//! The offline backend: drives a [`Renderer`] on the calling thread with
//! chosen block sizes, and records every event its sinks receive.

use crate::backend::{
    AudioBackend, AudioConfig, BackendEvent, BlockTime, InputBlock, InstrumentNode, MidiOut,
    MidiPortInfo, StereoBuf, StreamInfo,
};
use crate::engine::SinkFactory;
use crate::error::BackendError;
use crate::render::{Renderer, Sink};
use crate::schedule::{TargetIdx, TargetKind};
use rtrb::{Consumer, Producer, RingBuffer};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Bytes kept per logged message (longer SysEx is truncated in the log).
pub const LOG_BYTES: usize = 16;

/// One logged message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogRecord {
    /// Absolute output frame.
    pub frame: u64,
    /// Target.
    pub target: TargetIdx,
    /// Full message length.
    pub len: u16,
    /// The first [`LOG_BYTES`] bytes.
    pub data: [u8; LOG_BYTES],
}

impl LogRecord {
    fn new(frame: u64, target: TargetIdx, msg: &[u8]) -> LogRecord {
        let mut data = [0u8; LOG_BYTES];
        for (d, s) in data.iter_mut().zip(msg) {
            *d = *s;
        }
        LogRecord {
            frame,
            target,
            len: u16::try_from(msg.len()).unwrap_or(u16::MAX),
            data,
        }
    }

    /// The kept bytes.
    pub fn bytes(&self) -> &[u8] {
        let n = usize::from(self.len).min(LOG_BYTES);
        self.data.get(..n).unwrap_or(&[])
    }

    /// `frame tN hex bytes`, e.g. `24000 t0 90 3c 64`.
    pub fn line(&self) -> String {
        let mut s = format!("{} t{}", self.frame, self.target.0);
        for b in self.bytes() {
            let _ = write!(s, " {b:02x}");
        }
        if usize::from(self.len) > LOG_BYTES {
            s.push_str(" …");
        }
        s
    }
}

type Readers = Vec<(TargetIdx, Consumer<LogRecord>)>;

/// Collects the records of every recording sink (control side).
#[derive(Clone, Default)]
pub struct EventLog {
    readers: Arc<Mutex<Readers>>,
    dropped: Arc<AtomicU64>,
}

impl std::fmt::Debug for EventLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EventLog")
    }
}

impl EventLog {
    /// Removes and returns everything recorded so far, ordered by
    /// `(frame, target, arrival)`.
    pub fn drain(&self) -> Vec<LogRecord> {
        let mut out = Vec::new();
        if let Ok(mut readers) = self.readers.lock() {
            for (_, rx) in readers.iter_mut() {
                while let Ok(r) = rx.pop() {
                    out.push(r);
                }
            }
        }
        out.sort_by_key(|r| (r.frame, r.target));
        out
    }

    /// [`EventLog::drain`] as text, one [`LogRecord::line`] per line.
    pub fn drain_text(&self) -> String {
        let mut s = String::new();
        for r in self.drain() {
            s.push_str(&r.line());
            s.push('\n');
        }
        s
    }

    /// Records instrument sinks could not keep because their log was full.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// A new log for `target`, replacing any earlier one (whose sink was
    /// dropped with a failed load, or removed before its index was reused;
    /// unread records of the old target are discarded).
    fn add(&self, target: TargetIdx, capacity: usize) -> Producer<LogRecord> {
        let (tx, rx) = RingBuffer::new(capacity);
        if let Ok(mut readers) = self.readers.lock() {
            readers.retain(|(t, _)| *t != target);
            readers.push((target, rx));
        }
        tx
    }
}

/// A deterministic, sample-accurate test voice (Plan Ruling 27): every
/// held note adds a quiet sawtooth whose period depends on its pitch;
/// events apply at their frame offsets.
struct Tone {
    on: [u128; 16],
    events: [(u32, [u8; 3]); 64],
    len: usize,
    t: u64,
}

impl Tone {
    fn new() -> Tone {
        Tone {
            on: [0; 16],
            events: [(0, [0; 3]); 64],
            len: 0,
            t: 0,
        }
    }

    /// Queues a message for the next render (at most 64 per block). // RT
    fn queue(&mut self, offset: u32, msg: &[u8]) {
        let (Some(&s), Some(slot)) = (msg.first(), self.events.get_mut(self.len)) else {
            return;
        };
        *slot = (
            offset,
            [
                s,
                msg.get(1).copied().unwrap_or(0),
                msg.get(2).copied().unwrap_or(0),
            ],
        );
        self.len += 1;
    }

    fn apply(&mut self, [s, d1, d2]: [u8; 3]) {
        let Some(mask) = self.on.get_mut(usize::from(s & 0x0F)) else {
            return;
        };
        let bit = 1u128 << (d1 & 0x7F);
        match s & 0xF0 {
            0x90 if d2 > 0 => *mask |= bit,
            0x80 | 0x90 => *mask &= !bit,
            0xB0 if d1 == 123 => *mask = 0,
            _ => {}
        }
    }

    /// Renders `frames` frames, applying queued events at their offsets. // RT
    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        let n = (frames as usize).min(out.left.len()).min(out.right.len());
        let mut next = 0;
        for f in 0..n {
            while next < self.len {
                let Some(&(off, m)) = self.events.get(next) else {
                    break;
                };
                if off as usize > f {
                    break;
                }
                self.apply(m);
                next += 1;
            }
            let mut v = 0.0f32;
            for mask in self.on {
                let mut m = mask;
                while m != 0 {
                    let p = u64::from(m.trailing_zeros());
                    m &= m - 1;
                    v += ((self.t.wrapping_mul(p + 1) % 512) as f32 / 512.0 - 0.5) * 0.02;
                }
            }
            if let (Some(l), Some(r)) = (out.left.get_mut(f), out.right.get_mut(f)) {
                *l = v;
                *r = v;
            }
            self.t += 1;
        }
        self.flush(next);
    }

    /// Applies the queued events from index `from` on and empties the
    /// queue. // RT
    fn flush(&mut self, from: usize) {
        for i in from..self.len {
            if let Some(&(_, m)) = self.events.get(i) {
                self.apply(m);
            }
        }
        self.len = 0;
    }

    /// A block passed unrendered. // RT
    fn skip(&mut self, frames: u32) {
        self.flush(0);
        self.t += u64::from(frames);
    }
}

/// An instrument that records what it receives and renders silence (or,
/// from [`OfflineSinks::with_tone`], a test tone).
pub struct RecordingNode {
    tx: Producer<LogRecord>,
    target: TargetIdx,
    rendered: u64,
    dropped: Arc<AtomicU64>,
    tone: Option<Box<Tone>>,
}

impl InstrumentNode for RecordingNode {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        let r = LogRecord::new(self.rendered + u64::from(offset), self.target, msg);
        if self.tx.push(r).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(t) = self.tone.as_mut() {
            t.queue(offset, msg);
        }
    }

    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        match self.tone.as_mut() {
            Some(t) => t.render(frames, out),
            None => out.clear(frames),
        }
        self.rendered += u64::from(frames);
    }

    fn reset(&mut self) {}

    fn skip(&mut self, frames: u32) {
        if let Some(t) = self.tone.as_mut() {
            t.skip(frames);
        }
        self.rendered += u64::from(frames);
    }
}

/// A MIDI output that records what it receives, converting host time back
/// to frames.
pub struct RecordingMidiOut {
    tx: Producer<LogRecord>,
    target: TargetIdx,
    sample_rate: f64,
    accept: bool,
}

impl MidiOut for RecordingMidiOut {
    fn send(&mut self, host_time_ns: u64, msg: &[u8]) -> bool {
        if !self.accept {
            return false;
        }
        let frame = (host_time_ns as f64 * self.sample_rate / 1e9).round() as u64;
        self.tx
            .push(LogRecord::new(frame, self.target, msg))
            .is_ok()
    }
}

/// Makes recording sinks that feed one [`EventLog`].
#[derive(Debug, Clone)]
pub struct OfflineSinks {
    log: EventLog,
    capacity: usize,
    ports: Vec<MidiPortInfo>,
    /// When false, every MIDI output refuses messages (tests the
    /// midi-dropped fault).
    pub midi_accepts: bool,
    tone: bool,
}

impl OfflineSinks {
    /// Sinks whose logs hold `capacity` records each.
    pub fn new(capacity: usize) -> OfflineSinks {
        OfflineSinks {
            log: EventLog::default(),
            capacity,
            ports: Vec::new(),
            midi_accepts: true,
            tone: false,
        }
    }

    /// Pretends these MIDI destinations exist.
    pub fn with_ports(mut self, ports: Vec<MidiPortInfo>) -> OfflineSinks {
        self.ports = ports;
        self
    }

    /// Built-in synth sinks also render a deterministic test tone (Plan
    /// Ruling 27).
    pub fn with_tone(mut self) -> OfflineSinks {
        self.tone = true;
        self
    }

    /// The shared log.
    pub fn log(&self) -> EventLog {
        self.log.clone()
    }
}

impl SinkFactory for OfflineSinks {
    fn ports(&self) -> Vec<MidiPortInfo> {
        self.ports.clone()
    }

    fn make(
        &mut self,
        target: TargetIdx,
        kind: &TargetKind,
        sample_rate: f64,
        _max_frames: u32,
    ) -> Result<Sink, BackendError> {
        if let TargetKind::PluginInstrument(id) = kind {
            return Err(BackendError::NotSupported(format!("plugin instance {id}")));
        }
        let tx = self.log.add(target, self.capacity);
        Ok(match kind {
            TargetKind::BuiltinSynth => Sink::Instrument(Box::new(RecordingNode {
                tx,
                target,
                rendered: 0,
                dropped: Arc::clone(&self.log.dropped),
                tone: self.tone.then(|| Box::new(Tone::new())),
            })),
            TargetKind::MidiVirtualSource
            | TargetKind::MidiDestination(_)
            | TargetKind::PluginInstrument(_) => Sink::Midi(Box::new(RecordingMidiOut {
                tx,
                target,
                sample_rate,
                accept: self.midi_accepts,
            })),
        })
    }
}

/// A fake audio input (sub-project 5 spec §12.1): no device and no
/// microphone permission, for tests and the e2e.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeInput {
    /// Channel 1 carries a 440 Hz sine at −12 dBFS, channel 2 an impulse
    /// every 0.5 s (from frame 0), channel 3 silence.
    Signals,
    /// Channels 1 and 2 are the output's left and right this many frames
    /// later (at least one block, plan 5c Ruling 8); channel 3 silence.
    /// The device reports half the delay as output latency and the rest
    /// as input latency, as a real loopback cable would show.
    Loopback(u32),
}

/// Channels of the fake input.
pub const FAKE_INPUT_CHANNELS: usize = 3;
/// The fake sine's frequency.
pub const FAKE_SINE_HZ: f64 = 440.0;
/// The fake sine's amplitude: −12 dBFS.
pub const FAKE_SINE_AMPLITUDE: f32 = 0.251_188_64;

/// Sample `frame` of fake input channel `c` for [`FakeInput::Signals`].
pub fn fake_signal(c: usize, frame: u64, sample_rate: f64) -> f32 {
    match c {
        0 => {
            let t = frame as f64 / sample_rate.max(1.0);
            (FAKE_SINE_AMPLITUDE as f64 * (std::f64::consts::TAU * FAKE_SINE_HZ * t).sin()) as f32
        }
        1 => {
            let every = (sample_rate / 2.0).round().max(1.0) as u64;
            if frame % every == 0 { 1.0 } else { 0.0 }
        }
        _ => 0.0,
    }
}

/// The fake input's buffers.
#[derive(Debug)]
struct Fake {
    kind: FakeInput,
    /// The loopback delay in frames (0 for signals).
    delay: u32,
    chans: [Vec<f32>; FAKE_INPUT_CHANNELS],
    /// The output's left and right, by absolute frame modulo the length.
    history: [Vec<f32>; 2],
}

/// Drives a renderer without hardware. Block `n` starts at host time
/// `(frames_so_far + output latency) × 1e9 / sample_rate` ns: the time
/// its first frame is heard (the fake device's output latency is 0
/// unless it loops back).
#[derive(Debug)]
pub struct OfflineBackend {
    sample_rate: f64,
    block: u32,
    channels: usize,
    renderer: Option<Renderer>,
    frames: u64,
    left: Vec<f32>,
    right: Vec<f32>,
    fake: Option<Fake>,
}

impl OfflineBackend {
    /// A backend at `sample_rate` whose default block is `block` frames.
    pub fn new(sample_rate: f64, block: u32) -> OfflineBackend {
        OfflineBackend {
            sample_rate,
            block,
            channels: 2,
            renderer: None,
            frames: 0,
            left: Vec::new(),
            right: Vec::new(),
            fake: None,
        }
    }

    /// The backend with a fake input (spec §12.1).
    pub fn with_input(mut self, kind: FakeInput) -> OfflineBackend {
        let delay = match kind {
            FakeInput::Signals => 0,
            FakeInput::Loopback(n) => n.max(self.block),
        };
        self.fake = Some(Fake {
            kind,
            delay,
            chans: Default::default(),
            history: Default::default(),
        });
        let n = self.left.len() as u32;
        self.reserve(n.max(self.block));
        self
    }

    /// `(output, input)` latency the fake device reports, in frames.
    fn latencies(&self) -> (u32, u32) {
        match &self.fake {
            Some(f) => (f.delay / 2, f.delay - f.delay / 2),
            None => (0, 0),
        }
    }

    /// Frames rendered so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Prepares output buffers for blocks up to `max` frames (non-RT).
    pub fn reserve(&mut self, max: u32) {
        let n = max as usize;
        if self.left.len() < n {
            self.left.resize(n, 0.0);
            self.right.resize(n, 0.0);
        }
        let n = self.left.len();
        if let Some(f) = self.fake.as_mut() {
            for c in f.chans.iter_mut() {
                if c.len() < n {
                    c.resize(n, 0.0);
                }
            }
            let h = f.delay as usize + n;
            for c in f.history.iter_mut() {
                if c.len() < h {
                    // Resizing loses the history: only before playing.
                    *c = vec![0.0; h];
                }
            }
        }
    }

    /// Renders one block of `frames` frames; returns the peak absolute
    /// sample. Call [`OfflineBackend::reserve`] first for blocks larger than
    /// any seen before.
    pub fn block(&mut self, frames: u32) -> f32 {
        let (out_latency, _) = self.latencies();
        let Some(r) = self.renderer.as_mut() else {
            return 0.0;
        };
        let n = (frames as usize).min(self.left.len());
        let time = BlockTime {
            host_time_ns: ((self.frames + u64::from(out_latency)) as f64 * 1e9 / self.sample_rate)
                .round() as u64,
            sample_rate: self.sample_rate,
        };
        let abs = self.frames;
        let rate = self.sample_rate;
        if let Some(f) = self.fake.as_mut() {
            fill_fake(f, abs, n, rate);
        }
        let (Some(l), Some(rt)) = (self.left.get_mut(..n), self.right.get_mut(..n)) else {
            return 0.0;
        };
        let mut chans: [&mut [f32]; 2] = [l, rt];
        match &self.fake {
            Some(f) => {
                let ins: [&[f32]; FAKE_INPUT_CHANNELS] = [
                    f.chans[0].get(..n).unwrap_or(&[]),
                    f.chans[1].get(..n).unwrap_or(&[]),
                    f.chans[2].get(..n).unwrap_or(&[]),
                ];
                let input = InputBlock {
                    channels: &ins,
                    delay_frames: i64::from(f.delay),
                };
                r.process_input(&mut chans, frames, time, input);
            }
            None => r.process(&mut chans, frames, time),
        }
        if let Some(f) = self.fake.as_mut() {
            if f.kind != FakeInput::Signals {
                for (h, src) in f.history.iter_mut().zip([&*chans[0], &*chans[1]]) {
                    let len = h.len().max(1);
                    for (k, x) in src.iter().enumerate() {
                        if let Some(d) = h.get_mut((abs as usize + k) % len) {
                            *d = *x;
                        }
                    }
                }
            }
        }
        self.frames += u64::from(frames);
        chans
            .iter()
            .flat_map(|c| c.iter())
            .fold(0.0f32, |m, x| m.max(x.abs()))
    }

    /// Renders `total` frames in blocks of the default size.
    pub fn run_frames(&mut self, total: u64) {
        self.reserve(self.block);
        let mut left = total;
        while left > 0 {
            let n = u32::try_from(left.min(u64::from(self.block))).unwrap_or(self.block);
            self.block(n);
            left -= u64::from(n);
        }
    }

    /// Renders blocks of the given sizes.
    pub fn run_blocks(&mut self, sizes: impl IntoIterator<Item = u32>) {
        for n in sizes {
            self.reserve(n);
            self.block(n);
        }
    }

    /// The last block's output: `frames` frames of the left and right
    /// channels.
    pub fn output(&self, frames: u32) -> (&[f32], &[f32]) {
        let n = frames as usize;
        (
            self.left.get(..n).unwrap_or(&[]),
            self.right.get(..n).unwrap_or(&[]),
        )
    }

    /// Mutable access to the running renderer (not RT; for tests).
    pub fn renderer_mut(&mut self) -> Option<&mut Renderer> {
        self.renderer.as_mut()
    }
}

impl AudioBackend for OfflineBackend {
    fn open(&mut self, _cfg: &AudioConfig) -> Result<StreamInfo, BackendError> {
        let (output_latency_frames, input_latency_frames) = self.latencies();
        let inputs = if self.fake.is_some() {
            FAKE_INPUT_CHANNELS
        } else {
            0
        };
        Ok(StreamInfo {
            device_name: "offline".into(),
            sample_rate: self.sample_rate,
            buffer_frames: self.block,
            channels: u32::try_from(self.channels).unwrap_or(2),
            input_channels: inputs as u32,
            input_channel_names: (1..=inputs).map(|k| format!("Fake In {k}")).collect(),
            input_latency_frames,
            output_latency_frames,
        })
    }

    fn start(&mut self, renderer: Renderer) -> Result<(), BackendError> {
        self.reserve(self.block.max(renderer.config().max_frames));
        self.renderer = Some(renderer);
        Ok(())
    }

    fn stop(&mut self) -> Result<Renderer, BackendError> {
        self.renderer.take().ok_or(BackendError::NoDevice)
    }

    fn take_events(&mut self) -> Vec<BackendEvent> {
        Vec::new()
    }
}

/// Fills the fake input's channels for `n` frames from absolute frame
/// `abs`. // RT
fn fill_fake(f: &mut Fake, abs: u64, n: usize, rate: f64) {
    let [c0, c1, c2] = &mut f.chans;
    let (Some(c0), Some(c1), Some(c2)) = (c0.get_mut(..n), c1.get_mut(..n), c2.get_mut(..n)) else {
        return;
    };
    c2.fill(0.0);
    match f.kind {
        FakeInput::Signals => {
            for k in 0..n {
                let at = abs + k as u64;
                if let (Some(a), Some(b)) = (c0.get_mut(k), c1.get_mut(k)) {
                    *a = fake_signal(0, at, rate);
                    *b = fake_signal(1, at, rate);
                }
            }
        }
        FakeInput::Loopback(_) => {
            let delay = u64::from(f.delay);
            for (c, h) in [c0, c1].into_iter().zip(f.history.iter()) {
                let len = h.len().max(1) as u64;
                for (k, x) in c.iter_mut().enumerate() {
                    let at = abs + k as u64;
                    // Frames the output has not played yet read 0 (a block
                    // longer than the delay).
                    *x = match at.checked_sub(delay) {
                        Some(src) if src < abs => {
                            h.get((src % len) as usize).copied().unwrap_or(0.0)
                        }
                        _ => 0.0,
                    };
                }
            }
        }
    }
}
