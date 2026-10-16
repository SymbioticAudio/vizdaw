//! Reference commands: insert, make from selection, re-point, private copy,
//! flatten and per-use parameters.

use super::event::{ensure_drum_row, insert_specs};
use super::sequence::clone_sequence;
use super::{Context, Report};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{EventBody, EventSpec, LoopCount};
use crate::ids::{AnyId, EventId, LaneId, SeqId};
use crate::lane::Lane;
use crate::query::flatten::Walk;
use crate::query::seq_length;
use crate::rules::{check_event_fields, check_reference};
use crate::selection::ViewFilter;
use crate::sequence::{SeqLength, Sequence};
use crate::time::Tick;
use im::Vector;

/// Reference edits.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceCmd {
    /// Insert a reference to `target` into a lane's active take.
    Insert {
        /// Owning sequence.
        seq: SeqId,
        /// Owning lane.
        lane: LaneId,
        /// Position.
        tick: Tick,
        /// Referenced sequence.
        target: SeqId,
        /// Per-use length.
        length: Option<Tick>,
        /// Repeats.
        loop_count: LoopCount,
        /// Transpose in semitones.
        transpose: i8,
    },
    /// Move the selected lanes' active-take events inside the selection's
    /// time range into new sequence(s) and put reference(s) in their place.
    FromSelection {
        /// The sequence the selection is in.
        seq: SeqId,
        /// One reference per lane instead of one multi-lane reference.
        per_lane: bool,
        /// Widen the range to barlines.
        round_to_bars: bool,
        /// Loop count of the new reference(s).
        loop_count: LoopCount,
    },
    /// Point a reference at another sequence.
    RePoint {
        /// The reference event.
        event: EventId,
        /// New target.
        target: SeqId,
    },
    /// Clone the referenced sequence and point this use at the clone.
    MakePrivateCopy {
        /// The reference event.
        event: EventId,
    },
    /// Replace the reference with its events, inlined in place (length,
    /// loop, transpose, mute and nested references applied; destinations made
    /// explicit).
    Flatten {
        /// The reference event.
        event: EventId,
    },
    /// Set per-use length, loop count and transpose.
    SetParams {
        /// The reference event.
        event: EventId,
        /// Per-use length.
        length: Option<Tick>,
        /// Repeats.
        loop_count: LoopCount,
        /// Transpose.
        transpose: i8,
    },
}

impl ReferenceCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            ReferenceCmd::Insert { .. } => "Insert Reference",
            ReferenceCmd::FromSelection { .. } => "Make Reference from Selection",
            ReferenceCmd::RePoint { .. } => "Re-point Reference",
            ReferenceCmd::MakePrivateCopy { .. } => "Make Private Copy",
            ReferenceCmd::Flatten { .. } => "Flatten Reference",
            ReferenceCmd::SetParams { .. } => "Set Reference Parameters",
        }
    }
}

/// The target of a reference event, or `Invalid` if it is not one.
fn target_of(d: &Document, event: EventId) -> Result<SeqId> {
    match d.event(event)?.0.body {
        EventBody::Reference { seq, .. } => Ok(seq),
        _ => Err(ModelError::invalid(format!("{event} is not a reference"))),
    }
}

fn set_target(d: &mut Document, event: EventId, target: SeqId) -> Result<()> {
    let (ev, loc) = d.event(event)?;
    check_reference(d, loc.seq, target)?;
    let mut ev = ev.clone();
    if let EventBody::Reference { seq, .. } = &mut ev.body {
        *seq = target;
    }
    d.replace_event(ev)?;
    Ok(())
}

fn flatten_ref(d: &mut Document, event: EventId, r: &mut Report) -> Result<()> {
    target_of(d, event)?;
    let (ev, loc) = d.event(event)?;
    let ev = ev.clone();
    let owner = d.sequence(loc.seq)?.clone();
    let filter = ViewFilter::default();
    let resolved = {
        let mut w = Walk::new(d, &owner.tempo, &filter, (Tick::MIN, Tick::MAX));
        let mut path = Vec::new();
        w.reference(
            &ev,
            ev.tick,
            Tick::ZERO,
            (Tick::MIN, Tick::MAX),
            0,
            &mut path,
        );
        w.out
    };
    let mut specs = Vec::with_capacity(resolved.len());
    for x in resolved {
        let source_gc = x
            .path
            .last()
            .and_then(|p| d.event(p.event).ok())
            .and_then(|(e, _)| e.group_channel);
        specs.push(EventSpec {
            tick: x.tick,
            dest: x.dest,
            group_channel: source_gc,
            mute: false,
            body: x.body,
        });
    }
    let unresolved = specs
        .iter()
        .filter(|s| s.body.is_midi() && s.dest.is_none())
        .count();
    if unresolved > 0 {
        r.warnings.push(format!(
            "{unresolved} flattened events had no destination and now follow this lane's"
        ));
    }
    let old_len = seq_length(d, loc.seq)?;
    d.remove_event(event)?;
    r.removed.push(AnyId::Event(event));
    for spec in specs {
        check_event_fields(d, &spec)?;
        let id = d.counters_mut().alloc_event();
        d.insert_event(loc.seq, loc.lane, Some(loc.take), spec.with_id(id))?;
        r.created.push(AnyId::Event(id));
        ensure_drum_row(d, id, r)?;
    }
    // The reference's span may have been longer than its events; keep the
    // owner's playing length so playback is unchanged.
    if owner.length == SeqLength::Derived && seq_length(d, loc.seq)? < old_len {
        d.seq_mut(loc.seq)?.length = SeqLength::Locked(old_len);
        r.warnings
            .push("sequence length locked to keep the flattened span".to_string());
    }
    Ok(())
}

fn from_selection(
    d: &mut Document,
    ctx: Context<'_>,
    seq: SeqId,
    per_lane: bool,
    round_to_bars: bool,
    loop_count: LoopCount,
    r: &mut Report,
) -> Result<()> {
    let parent = d.sequence(seq)?.clone();
    let (mut a, mut b) = ctx
        .selection
        .time_range
        .ok_or_else(|| ModelError::invalid("selection has no time range"))?;
    if round_to_bars {
        a = parent.meter.barline_at_or_before(a);
        b = parent.meter.barline_at_or_after(b);
    }
    if b <= a {
        return Err(ModelError::range(
            "selection.time_range",
            format!("{}..{}", a.0, b.0),
        ));
    }
    let lanes: Vec<Lane> = parent
        .lanes
        .iter()
        .filter(|l| ctx.selection.lanes.contains(&(seq, l.id)))
        .cloned()
        .collect();
    if lanes.is_empty() {
        return Err(ModelError::invalid(
            "no lanes of that sequence are selected",
        ));
    }
    let groups: Vec<Vec<Lane>> = if per_lane {
        lanes.into_iter().map(|l| vec![l]).collect()
    } else {
        vec![lanes]
    };
    for group in groups {
        let c = d.counters_mut();
        let new_seq = c.alloc_seq();
        let mut new_lanes = Vector::new();
        let mut moves = Vec::new();
        for l in &group {
            let (nl_id, take) = (c.alloc_lane(), c.alloc_take());
            let mut nl = Lane::new(nl_id, take, l.name.clone());
            nl.midi_dest = l.midi_dest;
            nl.audio_out = l.audio_out;
            new_lanes.push_back(nl);
            if let Some(t) = l.active() {
                for e in t.events.values().filter(|e| e.tick >= a && e.tick < b) {
                    moves.push((e.id, nl_id));
                }
            }
        }
        let mut s = Sequence::new(
            new_seq,
            format!("{} part {}", parent.name, new_seq.0),
            new_lanes,
        );
        s.tempo = parent.tempo.clone();
        s.meter = parent.meter.clone();
        s.length = SeqLength::Locked(b - a);
        d.put_sequence(s);
        r.created.push(AnyId::Seq(new_seq));
        for (id, nl) in moves {
            let (mut ev, _) = d.remove_event(id)?;
            ev.tick = ev.tick - a;
            d.insert_event(new_seq, nl, None, ev)?;
        }
        let first = group
            .first()
            .map(|l| l.id)
            .ok_or_else(|| ModelError::invalid("empty lane group"))?;
        let spec = EventSpec::new(
            a,
            EventBody::Reference {
                seq: new_seq,
                length: None,
                loop_count,
                transpose: 0,
            },
        );
        insert_specs(d, seq, first, &[spec], r)?;
    }
    Ok(())
}

pub(crate) fn apply(
    d: &mut Document,
    ctx: Context<'_>,
    cmd: &ReferenceCmd,
    r: &mut Report,
) -> Result<()> {
    match cmd {
        ReferenceCmd::Insert {
            seq,
            lane,
            tick,
            target,
            length,
            loop_count,
            transpose,
        } => {
            let spec = EventSpec::new(
                *tick,
                EventBody::Reference {
                    seq: *target,
                    length: *length,
                    loop_count: *loop_count,
                    transpose: *transpose,
                },
            );
            insert_specs(d, *seq, *lane, &[spec], r)?;
        }
        ReferenceCmd::FromSelection {
            seq,
            per_lane,
            round_to_bars,
            loop_count,
        } => from_selection(d, ctx, *seq, *per_lane, *round_to_bars, *loop_count, r)?,
        ReferenceCmd::RePoint { event, target } => {
            target_of(d, *event)?;
            set_target(d, *event, *target)?;
        }
        ReferenceCmd::MakePrivateCopy { event } => {
            let target = target_of(d, *event)?;
            let name = format!("{} (copy)", d.sequence(target)?.name);
            let copy = clone_sequence(d, target, name)?;
            r.created.push(AnyId::Seq(copy));
            set_target(d, *event, copy)?;
        }
        ReferenceCmd::Flatten { event } => flatten_ref(d, *event, r)?,
        ReferenceCmd::SetParams {
            event,
            length,
            loop_count,
            transpose,
        } => {
            target_of(d, *event)?;
            let mut ev = d.event(*event)?.0.clone();
            if let EventBody::Reference {
                length: l,
                loop_count: lc,
                transpose: t,
                ..
            } = &mut ev.body
            {
                *l = *length;
                *lc = *loop_count;
                *t = *transpose;
            }
            check_event_fields(d, &ev.spec())?;
            d.replace_event(ev)?;
        }
    }
    Ok(())
}
