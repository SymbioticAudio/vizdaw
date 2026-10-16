//! What one `AudioProcessor::process` call gets (spec §5). Every buffer is
//! allocated before processing starts; nothing here allocates. // RT

use core_engine::backend::BlockInfo;
use core_model::mixer::ParamId;

/// Most MIDI events passed in one block (more are dropped and counted).
pub const MAX_EVENTS: usize = 512;
/// Most parameter events in or out of one block.
pub const MAX_PARAM_EVENTS: usize = 256;

/// A MIDI 1.0 channel message at a frame of the block. SysEx is not passed
/// to plugins in 3b (Plan Ruling 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MidiEvent {
    /// Frame offset in the block.
    pub frame: u32,
    /// Status and data bytes; `len` of them are used.
    pub bytes: [u8; 3],
    /// 1 … 3.
    pub len: u8,
}

impl MidiEvent {
    /// A message from its bytes (at most 3 are kept).
    pub fn new(frame: u32, msg: &[u8]) -> MidiEvent {
        let mut bytes = [0u8; 3];
        for (d, s) in bytes.iter_mut().zip(msg) {
            *d = *s;
        }
        MidiEvent {
            frame,
            bytes,
            len: msg.len().min(3) as u8,
        }
    }

    /// The used bytes.
    pub fn as_slice(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or(&[])
    }
}

/// A parameter value set by the host for this block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamChange {
    /// Frame offset in the block.
    pub frame: u32,
    /// Parameter.
    pub id: ParamId,
    /// Plain value.
    pub value: f64,
}

impl Default for ParamChange {
    fn default() -> ParamChange {
        ParamChange {
            frame: 0,
            id: ParamId(0),
            value: 0.0,
        }
    }
}

/// A parameter event from the plugin (its own UI moved a control).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamEvent {
    /// The user grabbed the control.
    Begin(ParamId),
    /// The value changed.
    Value(ParamId, f64),
    /// The user let go.
    End(ParamId),
}

/// A fixed-capacity list of parameter events the plugin produced. // RT
#[derive(Debug, Clone)]
pub struct ParamOut {
    events: [ParamEvent; MAX_PARAM_EVENTS],
    len: usize,
}

impl Default for ParamOut {
    fn default() -> ParamOut {
        ParamOut {
            events: [ParamEvent::End(ParamId(0)); MAX_PARAM_EVENTS],
            len: 0,
        }
    }
}

impl ParamOut {
    /// Appends an event; false when full (the event is dropped). // RT
    pub fn push(&mut self, e: ParamEvent) -> bool {
        match self.events.get_mut(self.len) {
            Some(slot) => {
                *slot = e;
                self.len += 1;
                true
            }
            None => false,
        }
    }

    /// The events so far.
    pub fn as_slice(&self) -> &[ParamEvent] {
        self.events.get(..self.len).unwrap_or(&[])
    }

    /// Forgets every event. // RT
    pub fn clear(&mut self) {
        self.len = 0;
    }
}

/// One block for a processor. Channel buffers hold at least `frames`
/// samples; `outputs` holds every output port's channels, port after port
/// in [`crate::PluginInfo::outputs`] order.
pub struct ProcessCtx<'a> {
    /// Frames to process.
    pub frames: u32,
    /// Main input channels (empty for instruments). Mutable so formats
    /// that take the host's buffers (AU) can lend them; a plugin must not
    /// rely on what it finds there afterwards.
    pub input: &'a mut [Box<[f32]>],
    /// Sidechain input channels, when the plugin has a sidechain port
    /// (silent when nothing is routed to it).
    pub sidechain: &'a mut [Box<[f32]>],
    /// Output channels of every port.
    pub outputs: &'a mut [Box<[f32]>],
    /// MIDI events sorted by frame.
    pub events: &'a [MidiEvent],
    /// Parameter changes from the host, sorted by frame.
    pub params_in: &'a [ParamChange],
    /// Parameter events from the plugin.
    pub params_out: &'a mut ParamOut,
    /// Transport facts at the block's first frame.
    pub transport: &'a BlockInfo,
}
