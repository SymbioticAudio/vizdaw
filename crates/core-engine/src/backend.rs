//! Platform-neutral backend traits. `platform-macos` implements them with
//! CoreAudio/CoreMIDI; the `offline` module implements them for tests.

use crate::error::BackendError;
use crate::render::Renderer;

/// Largest block any node or renderer is prepared for.
pub const MAX_FRAMES: u32 = 4096;

/// Timing of one block: its first frame reaches the output at
/// `host_time_ns`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockTime {
    /// Host time (nanoseconds) at which the block's first frame is heard.
    pub host_time_ns: u64,
    /// Stream sample rate.
    pub sample_rate: f64,
}

/// What the caller wants from an audio device.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioConfig {
    /// Device name; `None` follows the system default output.
    pub device: Option<String>,
    /// Requested buffer size in frames (32–2048); `None` keeps the device's.
    pub buffer_frames: Option<u32>,
}

/// What a backend opened.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamInfo {
    /// Device display name.
    pub device_name: String,
    /// Nominal sample rate.
    pub sample_rate: f64,
    /// IO buffer size in frames.
    pub buffer_frames: u32,
    /// Output channel count.
    pub channels: u32,
}

/// Something that happened to the device, reported by
/// [`AudioBackend::take_events`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackendEvent {
    /// The device disappeared.
    DeviceLost,
    /// The system default output changed (only when following it).
    DefaultDeviceChanged,
    /// The nominal sample rate changed.
    SampleRateChanged(f64),
    /// The IO buffer size changed.
    BufferSizeChanged(u32),
}

/// An audio output device driving a [`Renderer`].
pub trait AudioBackend {
    /// Opens the device (not yet running).
    fn open(&mut self, cfg: &AudioConfig) -> Result<StreamInfo, BackendError>;
    /// Moves the renderer to the real-time thread and starts it.
    fn start(&mut self, renderer: Renderer) -> Result<(), BackendError>;
    /// Stops the device and hands the renderer back (never dropped on the
    /// real-time thread).
    fn stop(&mut self) -> Result<Renderer, BackendError>;
    /// Device events since the last call.
    fn take_events(&mut self) -> Vec<BackendEvent>;
}

/// Planar stereo scratch buffers, allocated once.
#[derive(Debug, Clone)]
pub struct StereoBuf {
    /// Left channel.
    pub left: Box<[f32]>,
    /// Right channel.
    pub right: Box<[f32]>,
}

impl StereoBuf {
    /// Zeroed buffers of `frames` samples per channel.
    pub fn new(frames: u32) -> StereoBuf {
        let n = frames as usize;
        StereoBuf {
            left: vec![0.0; n].into_boxed_slice(),
            right: vec![0.0; n].into_boxed_slice(),
        }
    }

    /// Capacity in frames.
    pub fn capacity(&self) -> u32 {
        u32::try_from(self.left.len().min(self.right.len())).unwrap_or(u32::MAX)
    }

    /// Zeroes the first `frames` samples of both channels. // RT
    pub fn clear(&mut self, frames: u32) {
        let n = (frames as usize).min(self.left.len()).min(self.right.len());
        if let (Some(l), Some(r)) = (self.left.get_mut(..n), self.right.get_mut(..n)) {
            l.fill(0.0);
            r.fill(0.0);
        }
    }
}

/// Transport facts for one block, as of its first frame (plugins read
/// tempo and position from it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockInfo {
    /// The transport is playing.
    pub playing: bool,
    /// Schedule frame of the block's first frame.
    pub frame: u64,
    /// Position in quarter notes (ticks ÷ PPQ; negative in a pickup).
    pub beats: f64,
    /// Tempo in beats per minute.
    pub tempo_bpm: f64,
}

impl Default for BlockInfo {
    fn default() -> BlockInfo {
        BlockInfo {
            playing: false,
            frame: 0,
            beats: 0.0,
            tempo_bpm: 120.0,
        }
    }
}

/// An instrument hosted inside the engine (the built-in synth, plugin
/// instruments). Every method but `latency` and `outputs` is called on the
/// real-time thread.
pub trait InstrumentNode: Send {
    /// Queues one MIDI 1.0 message (or a whole `F0 … F7` SysEx) at a frame
    /// offset within the next `render` call. // RT
    fn midi(&mut self, offset: u32, msg: &[u8]);
    /// Renders `frames` frames into `out`, replacing its contents. // RT
    fn render(&mut self, frames: u32, out: &mut StereoBuf);
    /// All notes off and controllers reset. // RT
    fn reset(&mut self);
    /// A block of `frames` frames was not rendered (an xrun); nodes that
    /// track time advance by it. // RT
    fn skip(&mut self, frames: u32) {
        let _ = frames;
    }

    /// The plugin instance this node has just bypassed for faults (spec
    /// §7), once; the renderer reports it under that instance. Nodes that
    /// are not plugins never fault. // RT
    fn take_fault(&mut self) -> Option<core_model::ids::PluginInstanceId> {
        None
    }

    /// Automation's value for parameter `param`, normalised 0–1, at `frame`
    /// of the next render; plugin instruments map it to the parameter's
    /// range (Plan 4d Ruling 12). // RT
    fn automate(&mut self, frame: u32, param: core_model::mixer::ParamId, normalized: f64) {
        let _ = (frame, param, normalized);
    }

    /// Output latency in frames, read on the control thread when the sink
    /// is created (the DLS synth: 0).
    fn latency(&self) -> u32 {
        0
    }

    /// Number of stereo outputs (1 … 16), read on the control thread when
    /// the sink is created. Output 0 feeds the destination's `Instrument`
    /// strip, output `k` its `InstrumentOutput(_, k)` strip.
    fn outputs(&self) -> u16 {
        1
    }

    /// Renders every output: `outs[k]` is output `k` (`outs.len()` is
    /// [`InstrumentNode::outputs`]). The default renders output 0 with
    /// `render` and silences the rest. // RT
    fn render_outputs(&mut self, frames: u32, outs: &mut [StereoBuf], info: &BlockInfo) {
        let _ = info;
        if let Some((first, rest)) = outs.split_first_mut() {
            self.render(frames, first);
            for o in rest {
                o.clear(frames);
            }
        }
    }
}

/// A timestamped MIDI output. `send` must be wait-free.
pub trait MidiOut: Send {
    /// Queues `msg` for delivery at `host_time_ns`; `false` means it was
    /// dropped because the queue is full. // RT
    fn send(&mut self, host_time_ns: u64, msg: &[u8]) -> bool;
}

/// A MIDI destination visible to the system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MidiPortInfo {
    /// Display name.
    pub name: String,
    /// CoreMIDI unique ID, when known.
    pub unique_id: Option<i32>,
}

/// The system MIDI service.
pub trait MidiBackend {
    /// Destinations that can be opened.
    fn destinations(&self) -> Vec<MidiPortInfo>;
    /// Creates VizDAW's own virtual source.
    fn open_virtual_source(&mut self, name: &str) -> Result<Box<dyn MidiOut>, BackendError>;
    /// Opens an existing destination.
    fn open_destination(&mut self, port: &MidiPortInfo) -> Result<Box<dyn MidiOut>, BackendError>;
}
