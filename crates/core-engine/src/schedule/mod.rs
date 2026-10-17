//! Immutable playback schedules and their compiler.

pub mod automation;
pub mod compile;
pub mod targets;
pub mod tempo;

pub use automation::{
    AutoDest, AutoLane, AutoPoint, Automation, MAX_AUTO_LANES, MIN_RUN, StripParam,
};
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
    /// Controller state to chase after a locate, swap or wrap.
    pub chase: ChaseIndex,
    /// Automation lanes (4d).
    pub automation: Automation,
}

/// One chased controller value: `msg` takes effect at `frame`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChasePoint {
    /// Frame from the sequence origin.
    pub frame: u64,
    /// The message to resend.
    pub msg: Midi1Msg,
}

/// The chased controllers of one `(target, channel)`, in send order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChaseKind {
    /// Bank select MSB (CC 0).
    BankMsb = 0,
    /// Bank select LSB (CC 32).
    BankLsb = 1,
    /// Program change.
    Program = 2,
    /// Channel volume (CC 7).
    Volume = 3,
    /// Pan (CC 10).
    Pan = 4,
    /// Pitch bend.
    Bend = 5,
}

impl ChaseKind {
    /// The kind a MIDI 1.0 message updates, if it is chased.
    pub fn of(msg: &Midi1Msg) -> Option<ChaseKind> {
        let [status, d1, _] = msg.bytes;
        match (status & 0xF0, d1) {
            (0xB0, 0) => Some(ChaseKind::BankMsb),
            (0xB0, 32) => Some(ChaseKind::BankLsb),
            (0xB0, 7) => Some(ChaseKind::Volume),
            (0xB0, 10) => Some(ChaseKind::Pan),
            (0xC0, _) => Some(ChaseKind::Program),
            (0xE0, _) => Some(ChaseKind::Bend),
            _ => None,
        }
    }
}

/// Checkpoints of one `(target, channel, kind)`: `points[start..end]`,
/// sorted by frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChaseLane {
    /// Where the messages go.
    pub target: TargetIdx,
    /// First point.
    pub start: u32,
    /// One past the last point.
    pub end: u32,
}

/// Per `(target, channel)`, the bank/program, CC 7, CC 10 and pitch-bend
/// values over time, for looking up the state at any position in
/// O(lanes × log n) without allocating.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChaseIndex {
    lanes: Box<[ChaseLane]>,
    points: Box<[ChasePoint]>,
}

impl ChaseIndex {
    /// Builds the index from schedule entries sorted by frame (later
    /// entries on a frame win).
    pub fn build(entries: &[Entry]) -> ChaseIndex {
        let mut keyed: Vec<((u8, u8, ChaseKind), ChasePoint)> = entries
            .iter()
            .filter_map(|e| {
                let WireMsg::Midi1(msg) = e.msg else {
                    return None;
                };
                let kind = ChaseKind::of(&msg)?;
                Some((
                    (e.target.0, msg.bytes[0] & 0x0F, kind),
                    ChasePoint {
                        frame: e.frame,
                        msg,
                    },
                ))
            })
            .collect();
        // Stable: frame order (and order within a frame) is kept per lane.
        keyed.sort_by_key(|(k, _)| *k);
        let mut lanes: Vec<ChaseLane> = Vec::new();
        let mut last: Option<(u8, u8, ChaseKind)> = None;
        for (i, (k, _)) in keyed.iter().enumerate() {
            let i = u32::try_from(i).unwrap_or(u32::MAX);
            if last == Some(*k) {
                if let Some(l) = lanes.last_mut() {
                    l.end = i.saturating_add(1);
                }
            } else {
                lanes.push(ChaseLane {
                    target: TargetIdx(k.0),
                    start: i,
                    end: i.saturating_add(1),
                });
                last = Some(*k);
            }
        }
        ChaseIndex {
            lanes: lanes.into_boxed_slice(),
            points: keyed.into_iter().map(|(_, p)| p).collect(),
        }
    }

    /// The lanes, ordered by target, channel and [`ChaseKind`].
    pub fn lanes(&self) -> &[ChaseLane] {
        &self.lanes
    }

    /// Calls `f(target, msg)` with the last value of every lane strictly
    /// before `frame` (entries at `frame` itself play normally), in lane
    /// order: bank, program, volume, pan, bend per channel. // RT
    pub fn state_before(&self, frame: u64, mut f: impl FnMut(TargetIdx, &Midi1Msg)) {
        for lane in self.lanes.iter() {
            let pts = self
                .points
                .get(lane.start as usize..lane.end as usize)
                .unwrap_or(&[]);
            let n = pts.partition_point(|p| p.frame < frame);
            if let Some(p) = n.checked_sub(1).and_then(|i| pts.get(i)) {
                f(lane.target, &p.msg);
            }
        }
    }
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
