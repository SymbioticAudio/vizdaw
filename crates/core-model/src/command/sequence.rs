//! Sequence commands.

use super::Report;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, SeqId};
use crate::lane::{DrumLayout, Lane, Take};
use crate::meter::MeterMap;
use crate::rules::references_to;
use crate::sequence::{KeySlot, Marker, SeqKind, SeqLength, Sequence};
use crate::tempo::TempoMap;
use crate::time::Tick;
use im::{OrdMap, Vector};

/// Sequence-level edits.
#[derive(Debug, Clone, PartialEq)]
pub enum SequenceCmd {
    /// A new normal sequence with one empty lane.
    New {
        /// Name of the new sequence.
        name: String,
    },
    /// A deep copy with fresh IDs (no keyboard slot).
    Duplicate {
        /// The sequence to copy.
        seq: SeqId,
    },
    /// Rename.
    Rename {
        /// Target.
        seq: SeqId,
        /// New name.
        name: String,
    },
    /// Delete. A referenced sequence needs `delete_references: true`, which
    /// also removes every reference to it (listed in the report).
    Delete {
        /// Target.
        seq: SeqId,
        /// Confirmation to remove references.
        delete_references: bool,
    },
    /// Normal or generated.
    SetKind {
        /// Target.
        seq: SeqId,
        /// New kind.
        kind: SeqKind,
    },
    /// Assign or clear the keyboard slot (unique in the project).
    SetKeyboardSlot {
        /// Target.
        seq: SeqId,
        /// New slot.
        slot: Option<KeySlot>,
    },
    /// Replace the tempo map.
    SetTempo {
        /// Target.
        seq: SeqId,
        /// New map.
        tempo: TempoMap,
    },
    /// Replace the meter map.
    SetMeter {
        /// Target.
        seq: SeqId,
        /// New map.
        meter: MeterMap,
    },
    /// Replace the markers.
    SetMarkers {
        /// Target.
        seq: SeqId,
        /// New markers.
        markers: Vector<Marker>,
    },
    /// Set the pickup length (>= 0).
    SetStartOffset {
        /// Target.
        seq: SeqId,
        /// New offset.
        offset: Tick,
    },
    /// Derive or lock the length.
    SetLength {
        /// Target.
        seq: SeqId,
        /// New length mode.
        length: SeqLength,
    },
    /// Set the comment.
    SetComment {
        /// Target.
        seq: SeqId,
        /// New comment.
        comment: String,
    },
}

impl SequenceCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            SequenceCmd::New { .. } => "New Sequence",
            SequenceCmd::Duplicate { .. } => "Duplicate Sequence",
            SequenceCmd::Rename { .. } => "Rename Sequence",
            SequenceCmd::Delete { .. } => "Delete Sequence",
            SequenceCmd::SetKind { .. } => "Set Sequence Kind",
            SequenceCmd::SetKeyboardSlot { .. } => "Set Keyboard Slot",
            SequenceCmd::SetTempo { .. } => "Set Tempo",
            SequenceCmd::SetMeter { .. } => "Set Meter",
            SequenceCmd::SetMarkers { .. } => "Set Markers",
            SequenceCmd::SetStartOffset { .. } => "Set Start Offset",
            SequenceCmd::SetLength { .. } => "Set Sequence Length",
            SequenceCmd::SetComment { .. } => "Set Comment",
        }
    }
}

/// Copies sequence `src` under a new ID with fresh lane, take, event and
/// drum-row IDs. Returns the new ID.
pub(crate) fn clone_sequence(d: &mut Document, src: SeqId, name: String) -> Result<SeqId> {
    let s = d.sequence(src)?.clone();
    let c = d.counters_mut();
    let new_id = c.alloc_seq();
    let mut lanes = Vector::new();
    for l in &s.lanes {
        let mut nl = l.clone();
        nl.id = c.alloc_lane();
        let mut takes = Vector::new();
        for t in &l.takes {
            let tid = c.alloc_take();
            if t.id == l.active_take {
                nl.active_take = tid;
            }
            let mut events = OrdMap::new();
            for e in t.events.values() {
                let mut ne = e.clone();
                ne.id = c.alloc_event();
                events.insert(ne.key(), ne);
            }
            takes.push_back(Take {
                id: tid,
                name: t.name.clone(),
                events,
            });
        }
        nl.takes = takes;
        nl.drum = l.drum.as_ref().map(|dl| DrumLayout {
            rows: dl
                .rows
                .iter()
                .map(|r| {
                    let mut nr = r.clone();
                    nr.id = c.alloc_drum_row();
                    nr
                })
                .collect(),
        });
        lanes.push_back(nl);
    }
    let copy = Sequence {
        id: new_id,
        name,
        keyboard_slot: None,
        lanes,
        ..s
    };
    d.put_sequence(copy);
    Ok(new_id)
}

pub(crate) fn apply(d: &mut Document, cmd: &SequenceCmd, r: &mut Report) -> Result<()> {
    match cmd {
        SequenceCmd::New { name } => {
            let c = d.counters_mut();
            let id = c.alloc_seq();
            let lane = c.alloc_lane();
            let take = c.alloc_take();
            let s = Sequence::new(
                id,
                name.clone(),
                Vector::unit(Lane::new(lane, take, "Lane 1")),
            );
            d.put_sequence(s);
            r.created
                .extend([AnyId::Seq(id), AnyId::Lane(lane), AnyId::Take(take)]);
        }
        SequenceCmd::Duplicate { seq } => {
            let name = format!("{} copy", d.sequence(*seq)?.name);
            let id = clone_sequence(d, *seq, name)?;
            r.created.push(AnyId::Seq(id));
        }
        SequenceCmd::Rename { seq, name } => d.seq_mut(*seq)?.name = name.clone(),
        SequenceCmd::Delete {
            seq,
            delete_references,
        } => {
            d.sequence(*seq)?;
            let users = references_to(d, *seq);
            if !users.is_empty() && !delete_references {
                return Err(ModelError::InUse {
                    users: users.into_iter().map(AnyId::Event).collect(),
                });
            }
            for u in users {
                d.remove_event(u)?;
                r.removed.push(AnyId::Event(u));
            }
            d.remove_sequence(*seq)?;
            r.removed.push(AnyId::Seq(*seq));
        }
        SequenceCmd::SetKind { seq, kind } => d.seq_mut(*seq)?.kind = kind.clone(),
        SequenceCmd::SetKeyboardSlot { seq, slot } => {
            d.sequence(*seq)?;
            if let Some(k) = slot {
                if let Some(other) = d
                    .sequences()
                    .values()
                    .find(|s| s.id != *seq && s.keyboard_slot.as_ref() == Some(k))
                {
                    return Err(ModelError::invalid(format!(
                        "keyboard slot already used by {}",
                        other.id
                    )));
                }
            }
            d.seq_mut(*seq)?.keyboard_slot = slot.clone();
        }
        SequenceCmd::SetTempo { seq, tempo } => {
            tempo.check()?;
            d.seq_mut(*seq)?.tempo = tempo.clone();
        }
        SequenceCmd::SetMeter { seq, meter } => {
            meter.check()?;
            d.seq_mut(*seq)?.meter = meter.clone();
        }
        SequenceCmd::SetMarkers { seq, markers } => d.seq_mut(*seq)?.markers = markers.clone(),
        SequenceCmd::SetStartOffset { seq, offset } => {
            if offset.0 < 0 {
                return Err(ModelError::range("start_offset", offset.0));
            }
            d.seq_mut(*seq)?.start_offset = *offset;
        }
        SequenceCmd::SetLength { seq, length } => {
            if let SeqLength::Locked(t) = length {
                crate::rules::check_positive("length", *t)?;
            }
            d.seq_mut(*seq)?.length = *length;
        }
        SequenceCmd::SetComment { seq, comment } => d.seq_mut(*seq)?.comment = comment.clone(),
    }
    Ok(())
}
