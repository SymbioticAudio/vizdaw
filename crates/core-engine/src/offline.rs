//! The offline backend: drives a [`Renderer`] on the calling thread with
//! chosen block sizes, and records every event its sinks receive.

use crate::backend::{
    AudioBackend, AudioConfig, BackendEvent, BlockTime, InstrumentNode, MidiOut, MidiPortInfo,
    StereoBuf, StreamInfo,
};
use crate::engine::SinkFactory;
use crate::error::BackendError;
use crate::render::{Renderer, Sink};
use crate::schedule::{TargetIdx, TargetKind};
use rtrb::{Consumer, Producer, RingBuffer};
use std::fmt::Write as _;
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

    fn add(&self, target: TargetIdx, capacity: usize) -> Producer<LogRecord> {
        let (tx, rx) = RingBuffer::new(capacity);
        if let Ok(mut readers) = self.readers.lock() {
            readers.push((target, rx));
        }
        tx
    }
}

/// An instrument that records what it receives and renders silence.
pub struct RecordingNode {
    tx: Producer<LogRecord>,
    target: TargetIdx,
    rendered: u64,
}

impl InstrumentNode for RecordingNode {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        let _ = self.tx.push(LogRecord::new(
            self.rendered + u64::from(offset),
            self.target,
            msg,
        ));
    }

    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        out.clear(frames);
        self.rendered += u64::from(frames);
    }

    fn reset(&mut self) {}
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
}

impl OfflineSinks {
    /// Sinks whose logs hold `capacity` records each.
    pub fn new(capacity: usize) -> OfflineSinks {
        OfflineSinks {
            log: EventLog::default(),
            capacity,
            ports: Vec::new(),
            midi_accepts: true,
        }
    }

    /// Pretends these MIDI destinations exist.
    pub fn with_ports(mut self, ports: Vec<MidiPortInfo>) -> OfflineSinks {
        self.ports = ports;
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
        kind: &TargetKind,
        sample_rate: f64,
        _max_frames: u32,
    ) -> Result<Sink, BackendError> {
        // The index this target will get is the number of sinks made so far.
        let target = TargetIdx(
            self.log
                .readers
                .lock()
                .map(|r| u8::try_from(r.len()).unwrap_or(u8::MAX))
                .unwrap_or(0),
        );
        let tx = self.log.add(target, self.capacity);
        Ok(match kind {
            TargetKind::BuiltinSynth => Sink::Instrument(Box::new(RecordingNode {
                tx,
                target,
                rendered: 0,
            })),
            TargetKind::MidiVirtualSource | TargetKind::MidiDestination(_) => {
                Sink::Midi(Box::new(RecordingMidiOut {
                    tx,
                    target,
                    sample_rate,
                    accept: self.midi_accepts,
                }))
            }
        })
    }
}

/// Drives a renderer without hardware. Block `n` starts at host time
/// `frames_so_far × 1e9 / sample_rate` ns.
#[derive(Debug)]
pub struct OfflineBackend {
    sample_rate: f64,
    block: u32,
    channels: usize,
    renderer: Option<Renderer>,
    frames: u64,
    left: Vec<f32>,
    right: Vec<f32>,
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
    }

    /// Renders one block of `frames` frames; returns the peak absolute
    /// sample. Call [`OfflineBackend::reserve`] first for blocks larger than
    /// any seen before.
    pub fn block(&mut self, frames: u32) -> f32 {
        let Some(r) = self.renderer.as_mut() else {
            return 0.0;
        };
        let n = (frames as usize).min(self.left.len());
        let time = BlockTime {
            host_time_ns: (self.frames as f64 * 1e9 / self.sample_rate).round() as u64,
            sample_rate: self.sample_rate,
        };
        let (Some(l), Some(rt)) = (self.left.get_mut(..n), self.right.get_mut(..n)) else {
            return 0.0;
        };
        let mut chans: [&mut [f32]; 2] = [l, rt];
        r.process(&mut chans, frames, time);
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

    /// Mutable access to the running renderer (not RT; for tests).
    pub fn renderer_mut(&mut self) -> Option<&mut Renderer> {
        self.renderer.as_mut()
    }
}

impl AudioBackend for OfflineBackend {
    fn open(&mut self, _cfg: &AudioConfig) -> Result<StreamInfo, BackendError> {
        Ok(StreamInfo {
            device_name: "offline".into(),
            sample_rate: self.sample_rate,
            buffer_frames: self.block,
            channels: u32::try_from(self.channels).unwrap_or(2),
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
