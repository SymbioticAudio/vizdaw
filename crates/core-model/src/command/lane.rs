//! Lane and take commands.

use super::Report;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, DestId, LaneId, OutputPathId, SeqId, TakeId};
use crate::lane::{Lane, LaneInput, MonitorMode, QuantizeSpec, Take};
use crate::rules::check_positive;
use crate::time::Tick;

/// Lane-level edits.
#[derive(Debug, Clone, PartialEq)]
pub enum LaneCmd {
    /// Add a lane (with one empty take) at `index`, or at the end.
    Add {
        /// Sequence.
        seq: SeqId,
        /// Lane name.
        name: String,
        /// Position; clamped to the lane count.
        index: Option<usize>,
    },
    /// Remove a lane and all its takes and events.
    Remove {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
    },
    /// Move a lane to position `to` (clamped).
    Reorder {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// New index.
        to: usize,
    },
    /// Rename a lane.
    Rename {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// New name.
        name: String,
    },
    /// Set the lane's MIDI destination.
    SetDest {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Destination (must exist).
        dest: Option<DestId>,
    },
    /// Set the lane's audio output path.
    SetOutput {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Output (must exist).
        output: Option<OutputPathId>,
    },
    /// Set mute/solo/arm; `None` leaves a flag unchanged.
    SetFlags {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Mute.
        mute: Option<bool>,
        /// Solo.
        solo: Option<bool>,
        /// Record arm.
        arm: Option<bool>,
    },
    /// Set the lane's own length, loop flag and length lock.
    SetLength {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Own length (> 0) or none.
        length: Option<Tick>,
        /// Loop `[0, length)`.
        looped: bool,
        /// Lock the length against recording.
        locked: bool,
    },
    /// Set non-destructive play quantize.
    SetPlayQuantize {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Quantize (grid > 0, strength <= 100) or none.
        quantize: Option<QuantizeSpec>,
    },
    /// Set non-destructive play shift.
    SetPlayShift {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Shift in ticks.
        shift: Tick,
    },
    /// Add an empty take and make it active.
    AddTake {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Take name.
        name: String,
    },
    /// Choose which take plays.
    SetActiveTake {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Take (must exist in the lane).
        take: TakeId,
    },
    /// Set what the lane records (sub-project 5 spec §4.5).
    SetInput {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// The input (checked by [`LaneInput::check`]) or none.
        input: Option<LaneInput>,
    },
    /// Set when the lane's input is heard (spec §6.6).
    SetMonitor {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// The mode.
        monitor: MonitorMode,
    },
}

impl LaneCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            LaneCmd::Add { .. } => "Add Lane",
            LaneCmd::Remove { .. } => "Remove Lane",
            LaneCmd::Reorder { .. } => "Reorder Lane",
            LaneCmd::Rename { .. } => "Rename Lane",
            LaneCmd::SetDest { .. } => "Set Lane Destination",
            LaneCmd::SetOutput { .. } => "Set Lane Output",
            LaneCmd::SetFlags { .. } => "Set Lane Mute/Solo/Arm",
            LaneCmd::SetLength { .. } => "Set Lane Length",
            LaneCmd::SetPlayQuantize { .. } => "Set Play Quantize",
            LaneCmd::SetPlayShift { .. } => "Set Play Shift",
            LaneCmd::AddTake { .. } => "Add Take",
            LaneCmd::SetActiveTake { .. } => "Set Active Take",
            LaneCmd::SetInput { .. } => "Set Lane Input",
            LaneCmd::SetMonitor { .. } => "Set Lane Monitor",
        }
    }
}

pub(crate) fn apply(d: &mut Document, cmd: &LaneCmd, r: &mut Report) -> Result<()> {
    match cmd {
        LaneCmd::Add { seq, name, index } => {
            d.sequence(*seq)?;
            let c = d.counters_mut();
            let (lane, take) = (c.alloc_lane(), c.alloc_take());
            let s = d.seq_mut(*seq)?;
            let at = index.unwrap_or(s.lanes.len()).min(s.lanes.len());
            s.lanes.insert(at, Lane::new(lane, take, name.clone()));
            r.created.extend([AnyId::Lane(lane), AnyId::Take(take)]);
        }
        LaneCmd::Remove { seq, lane } => {
            let l = d.remove_lane(*seq, *lane)?;
            for t in &l.takes {
                r.removed
                    .extend(t.events.keys().map(|(_, id)| AnyId::Event(*id)));
            }
            r.removed.push(AnyId::Lane(*lane));
        }
        LaneCmd::Reorder { seq, lane, to } => {
            let s = d.seq_mut(*seq)?;
            let i = s
                .lane_index(*lane)
                .ok_or(ModelError::UnknownId(AnyId::Lane(*lane)))?;
            let l = s.lanes.remove(i);
            let at = (*to).min(s.lanes.len());
            s.lanes.insert(at, l);
        }
        LaneCmd::Rename { seq, lane, name } => d.lane_mut(*seq, *lane)?.name = name.clone(),
        LaneCmd::SetDest { seq, lane, dest } => {
            if let Some(id) = dest {
                if !d.instruments().contains_key(id) {
                    return Err(ModelError::UnknownId(AnyId::Dest(*id)));
                }
            }
            d.lane_mut(*seq, *lane)?.midi_dest = *dest;
        }
        LaneCmd::SetOutput { seq, lane, output } => {
            if let Some(id) = output {
                if !d.outputs().contains_key(id) {
                    return Err(ModelError::UnknownId(AnyId::Output(*id)));
                }
            }
            d.lane_mut(*seq, *lane)?.audio_out = *output;
        }
        LaneCmd::SetFlags {
            seq,
            lane,
            mute,
            solo,
            arm,
        } => {
            let l = d.lane_mut(*seq, *lane)?;
            if let Some(v) = mute {
                l.mute = *v;
            }
            if let Some(v) = solo {
                l.solo = *v;
            }
            if let Some(v) = arm {
                l.arm = *v;
            }
        }
        LaneCmd::SetLength {
            seq,
            lane,
            length,
            looped,
            locked,
        } => {
            if let Some(t) = length {
                check_positive("lane.length", *t)?;
                crate::rules::check_ticks("lane.length", t.0)?;
            }
            let l = d.lane_mut(*seq, *lane)?;
            l.length = *length;
            l.looped = *looped;
            l.length_locked = *locked;
        }
        LaneCmd::SetPlayQuantize {
            seq,
            lane,
            quantize,
        } => {
            if let Some(q) = quantize {
                q.check()?;
            }
            d.lane_mut(*seq, *lane)?.play_quantize = quantize.clone();
        }
        LaneCmd::SetPlayShift { seq, lane, shift } => {
            d.lane_mut(*seq, *lane)?.play_shift = *shift;
        }
        LaneCmd::AddTake { seq, lane, name } => {
            d.lane(*seq, *lane)?;
            let take = d.counters_mut().alloc_take();
            let l = d.lane_mut(*seq, *lane)?;
            l.takes.push_back(Take::new(take, name.clone()));
            l.active_take = take;
            r.created.push(AnyId::Take(take));
        }
        LaneCmd::SetActiveTake { seq, lane, take } => {
            let l = d.lane_mut(*seq, *lane)?;
            if l.take(*take).is_none() {
                return Err(ModelError::UnknownId(AnyId::Take(*take)));
            }
            l.active_take = *take;
        }
        LaneCmd::SetInput { seq, lane, input } => {
            if let Some(i) = input {
                i.check()?;
                // A synth's return names an external destination (plan
                // 5c-2 Ruling 3).
                if let Some(dest) = i.return_of() {
                    super::routing::check_external(d, dest)?;
                }
            }
            d.lane_mut(*seq, *lane)?.input = input.clone();
        }
        LaneCmd::SetMonitor { seq, lane, monitor } => {
            d.lane_mut(*seq, *lane)?.monitor = *monitor;
        }
    }
    Ok(())
}
