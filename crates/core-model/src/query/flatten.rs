//! Flattening: resolve a sequence and everything it references into
//! absolute, routed events.
//!
//! Nested timing rule: ticks inside a reference are offset into the playing
//! sequence's tick space, and `seconds` always comes from the playing
//! (top-level) sequence's tempo map. A referenced sequence's own tempo and
//! meter are never consulted here.

use super::length::Lengths;
use super::routing::resolve;
use crate::command::event::shift_pitch;
use crate::document::Document;
use crate::error::Result;
use crate::event::{Event, EventBody, LoopCount};
use crate::ids::{DestId, EventId, LaneId, OutputPathId, SeqId};
use crate::lane::Lane;
use crate::selection::ViewFilter;
use crate::sequence::Sequence;
use crate::tempo::TempoMap;
use crate::time::Tick;

/// Upper bound on loop passes of one lane or one reference use, so a tiny
/// period inside a huge window can't make flattening pathologically slow.
const MAX_PASSES: i64 = 1_000_000;

/// One step of the path from the flattened sequence to a source event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathStep {
    /// Sequence holding the event.
    pub seq: SeqId,
    /// Lane holding the event.
    pub lane: LaneId,
    /// The event (a reference for every step but the last).
    pub event: EventId,
}

/// A playable event with absolute time and resolved routing.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEvent {
    /// Absolute tick in the flattened sequence.
    pub tick: Tick,
    /// Seconds from tick 0 under the flattened sequence's tempo map.
    pub seconds: f64,
    /// Resolved MIDI destination (None for audio or unresolved events).
    pub dest: Option<DestId>,
    /// Resolved `(group, channel)`.
    pub group_channel: Option<(u8, u8)>,
    /// Audio output path for audio regions.
    pub audio_out: Option<OutputPathId>,
    /// The body after transpose and duration clipping.
    pub body: EventBody,
    /// Path from the flattened sequence down to the source event.
    pub path: Vec<PathStep>,
}

pub(crate) struct Walk<'a> {
    doc: &'a Document,
    tempo: &'a TempoMap,
    filter: &'a ViewFilter,
    range: (Tick, Tick),
    lengths: Lengths,
    stack: Vec<SeqId>,
    pub(crate) out: Vec<ResolvedEvent>,
}

impl<'a> Walk<'a> {
    pub(crate) fn new(
        doc: &'a Document,
        tempo: &'a TempoMap,
        filter: &'a ViewFilter,
        range: (Tick, Tick),
    ) -> Walk<'a> {
        Walk {
            doc,
            tempo,
            filter,
            range,
            lengths: Lengths::default(),
            stack: Vec::new(),
            out: Vec::new(),
        }
    }

    /// Walks sequence `s` placed at `offset`, playing its local window
    /// `win`, with accumulated `transpose`.
    fn sequence(
        &mut self,
        s: &Sequence,
        offset: Tick,
        win: (Tick, Tick),
        transpose: i32,
        path: &mut Vec<PathStep>,
    ) {
        if win.0 >= win.1 || self.stack.contains(&s.id) {
            return;
        }
        self.stack.push(s.id);
        let any_solo = s.lanes.iter().any(|l| l.solo);
        for lane in &s.lanes {
            if lane.mute || (any_solo && !lane.solo) || self.filter.hidden_lanes.contains(&lane.id)
            {
                continue;
            }
            self.lane(s.id, lane, offset, win, transpose, path);
        }
        self.stack.pop();
    }

    fn lane(
        &mut self,
        seq: SeqId,
        lane: &Lane,
        offset: Tick,
        win: (Tick, Tick),
        transpose: i32,
        path: &mut Vec<PathStep>,
    ) {
        let Some(take) = lane.active() else {
            return;
        };
        let (period, limit) = match lane.length {
            Some(l) if lane.looped && l.0 > 0 => (Some(l), l),
            Some(l) => (None, l),
            None => (None, Tick::MAX),
        };
        let mut k = 0i64;
        loop {
            let base = period.map_or(Tick::ZERO, |p| p.times(k));
            if k > 0 && (base + lane.play_shift >= win.1 || k >= MAX_PASSES) {
                break;
            }
            // A pass spans [base, base + limit) (shifted); playback of the
            // lane stops at its own length. Negative-tick (pickup) events
            // play only before the first pass: later passes start at their
            // loop point.
            let pass_start = if k > 0 {
                base + lane.play_shift
            } else {
                Tick::MIN
            };
            let lw = (
                win.0.max(pass_start),
                win.1.min(base + limit + lane.play_shift),
            );
            // Events at or after the loop limit never play, so skip them
            // without scanning.
            for (_, e) in take.events.range(..(limit, EventId(0))) {
                if e.mute || !self.filter.allows(lane.id, e.body.kind()) {
                    continue;
                }
                let mut lt = e.tick + base;
                if let (Some(q), EventBody::Note { .. }) = (lane.play_quantize, &e.body) {
                    lt = q.apply(lt);
                }
                lt = lt + lane.play_shift;
                // A reference that starts before the window still plays the
                // part of it that falls inside.
                let is_ref = matches!(e.body, EventBody::Reference { .. });
                if (lt < lw.0 && !is_ref) || lt >= lw.1 {
                    continue;
                }
                path.push(PathStep {
                    seq,
                    lane: lane.id,
                    event: e.id,
                });
                if is_ref {
                    self.reference(e, lt, offset, lw, transpose, path);
                } else {
                    self.emit(lane, e, lt, offset, lw.1, transpose, path);
                }
                path.pop();
            }
            if period.is_none() {
                break;
            }
            k += 1;
        }
    }

    /// Expands reference `e` placed at local tick `lt` of a sequence playing
    /// its local window `win`.
    pub(crate) fn reference(
        &mut self,
        e: &Event,
        lt: Tick,
        offset: Tick,
        win: (Tick, Tick),
        transpose: i32,
        path: &mut Vec<PathStep>,
    ) {
        let EventBody::Reference {
            seq,
            length,
            loop_count,
            transpose: tp,
        } = e.body
        else {
            return;
        };
        let doc = self.doc;
        let Ok(target) = doc.sequence(seq) else {
            return;
        };
        let b = self.lengths.of(doc, seq);
        if b.0 <= 0 {
            return;
        }
        let use_len = length.unwrap_or(b);
        let (iter_len, count) = match loop_count {
            LoopCount::Count(n) => (use_len, i64::from(n.max(1))),
            // Ceiling division without overflow (b > 0).
            LoopCount::Fill => (b, use_len.0.saturating_add(b.0 - 1) / b.0),
        };
        let count = count.min(MAX_PASSES);
        let total = match loop_count {
            LoopCount::Count(_) => iter_len.times(count),
            LoopCount::Fill => use_len,
        };
        let end = (lt + total).min(win.1);
        for i in 0..count {
            let start = lt + iter_len.times(i);
            if start >= end {
                break;
            }
            let lo = if i == 0 {
                -target.start_offset
            } else {
                Tick::ZERO
            }
            .max(win.0 - start);
            let hi = iter_len.min(end - start);
            let abs_lo = offset + start + lo;
            let abs_hi = offset + start + hi;
            if i > 0 && abs_lo >= self.range.1 {
                break;
            }
            if abs_hi <= self.range.0 || abs_lo >= self.range.1 {
                continue;
            }
            self.sequence(
                target,
                offset + start,
                (lo, hi),
                transpose + i32::from(tp),
                path,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        lane: &Lane,
        e: &Event,
        lt: Tick,
        offset: Tick,
        win_end: Tick,
        transpose: i32,
        path: &[PathStep],
    ) {
        let abs = offset + lt;
        if abs < self.range.0 || abs >= self.range.1 {
            return;
        }
        let mut body = e.body.clone();
        match &mut body {
            EventBody::Note {
                pitch, duration, ..
            } => {
                *pitch = shift_pitch(*pitch, transpose);
                *duration = (*duration).min(win_end - lt);
            }
            EventBody::PolyPressure { pitch, .. } => *pitch = shift_pitch(*pitch, transpose),
            EventBody::AudioRegion { media, length, .. } => {
                // Clip at the window end, measured in the playing tempo.
                let rate = self
                    .doc
                    .media()
                    .get(media)
                    .map_or(48_000, |m| m.sample_rate.max(1));
                let secs =
                    self.tempo.tick_to_seconds(offset + win_end) - self.tempo.tick_to_seconds(abs);
                // A tiny epsilon keeps float error from losing a whole frame.
                let avail = (secs * f64::from(rate) + 1e-6).floor();
                if avail < 1.0 {
                    return;
                }
                // Float-to-int `as` saturates, so a huge window is safe.
                *length = (*length).min(avail as u64);
            }
            _ => {}
        }
        let (dest, group_channel, audio_out) = match body {
            _ if body.is_midi() => {
                let r = resolve(self.doc, lane, e);
                (r.dest, r.group_channel, None)
            }
            EventBody::AudioRegion { .. } => (None, None, lane.audio_out),
            // Text meta-events carry no routing; the engine ignores them.
            _ => (None, None, None),
        };
        self.out.push(ResolvedEvent {
            tick: abs,
            seconds: self.tempo.tick_to_seconds(abs),
            dest,
            group_channel,
            audio_out,
            body,
            path: path.to_vec(),
        });
    }
}

/// Flattens `seq` over its playing window `[-start_offset, length)`,
/// keeping events whose absolute tick is in `range` (all when `None`) and
/// visible through `filter`. Events are ordered by tick (stable).
pub fn flatten(
    doc: &Document,
    seq: SeqId,
    range: Option<(Tick, Tick)>,
    filter: &ViewFilter,
) -> Result<std::vec::IntoIter<ResolvedEvent>> {
    let s = doc.sequence(seq)?;
    let len = Lengths::default().of(doc, seq);
    let mut w = Walk::new(
        doc,
        &s.tempo,
        filter,
        range.unwrap_or((Tick::MIN, Tick::MAX)),
    );
    let mut path = Vec::new();
    w.sequence(s, Tick::ZERO, (-s.start_offset, len), 0, &mut path);
    let mut out = w.out;
    out.sort_by_key(|r| r.tick);
    Ok(out.into_iter())
}
