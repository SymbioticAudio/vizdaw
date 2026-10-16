//! Immutable playback schedules and their compiler.

pub mod compile;
pub mod targets;
pub mod tempo;

pub use compile::{CompileOptions, CompileReport, Unroutable, compile};
pub use targets::{BuiltinRoute, MAX_TARGETS, TargetIdx, TargetKind, TargetTable};
pub use tempo::TempoTable;

use crate::midi1::Midi1Msg;

/// A message on the wire: a MIDI 1.0 channel message or a SysEx stored in
/// the schedule's arena. A `Ump64` variant can be added for MIDI 2.0
/// destinations without touching sorting or scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireMsg {
    /// A MIDI 1.0 channel message.
    Midi1(Midi1Msg),
    /// `F0 … F7` bytes at `sysex[start..start + len]`.
    SysEx {
        /// Arena offset.
        start: u32,
        /// Length including `F0` and `F7`.
        len: u32,
    },
}

/// One scheduled message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Frame from the sequence origin.
    pub frame: u64,
    /// Where it goes.
    pub target: TargetIdx,
    /// What is sent.
    pub msg: WireMsg,
}

/// A compiled sequence: everything the audio thread needs, immutable.
#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    /// Sample rate the frames were computed at.
    pub sample_rate: f64,
    /// Entries sorted by `(frame, class, source order)`.
    pub entries: Box<[Entry]>,
    /// SysEx arena.
    pub sysex: Box<[u8]>,
    /// Frame ↔ tick map.
    pub tempo: TempoTable,
    /// Frame of the sequence end.
    pub length_frames: u64,
    /// Default loop range: bar 1 (tick 0) to the end.
    pub loop_range: Option<(u64, u64)>,
    /// Per target, a bitmask of the channels the schedule uses.
    pub used_channels: [u16; MAX_TARGETS],
}

impl Schedule {
    /// The bytes of `msg`. // RT
    pub fn bytes<'a>(&'a self, msg: &'a WireMsg) -> &'a [u8] {
        match msg {
            WireMsg::Midi1(m) => m.as_slice(),
            WireMsg::SysEx { start, len } => {
                let s = *start as usize;
                let e = s.saturating_add(*len as usize);
                self.sysex.get(s..e).unwrap_or(&[])
            }
        }
    }

    /// Index of the first entry at or after `frame`. // RT
    pub fn cursor_at(&self, frame: u64) -> usize {
        self.entries.partition_point(|e| e.frame < frame)
    }
}
