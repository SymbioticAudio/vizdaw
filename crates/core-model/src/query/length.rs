//! Derived sequence lengths and event extents.

use crate::document::Document;
use crate::error::Result;
use crate::event::{Event, EventBody, LoopCount};
use crate::ids::SeqId;
use crate::sequence::{SeqLength, Sequence};
use crate::time::Tick;
use std::collections::{BTreeMap, BTreeSet};

/// Total ticks a reference use occupies, given the referenced sequence's
/// length `seq_len`: `Count(n)` plays the per-use length `n` times; `Fill`
/// plays the per-use length once (filling it with repeats of the sequence).
pub fn reference_span(seq_len: Tick, length: Option<Tick>, loop_count: LoopCount) -> Tick {
    let use_len = length.unwrap_or(seq_len);
    match loop_count {
        LoopCount::Count(n) => use_len.times(i64::from(n.max(1))),
        LoopCount::Fill => use_len,
    }
}

/// Memoising length calculator (guards against cycles in corrupt input).
#[derive(Default)]
pub(crate) struct Lengths {
    memo: BTreeMap<SeqId, Tick>,
    visiting: BTreeSet<SeqId>,
}

impl Lengths {
    /// The playing length of `seq`: locked, or the end of the longest lane
    /// rounded up to a barline (at least one bar).
    pub(crate) fn of(&mut self, doc: &Document, seq: SeqId) -> Tick {
        if let Some(t) = self.memo.get(&seq) {
            return *t;
        }
        let Ok(s) = doc.sequence(seq) else {
            return Tick::ZERO;
        };
        if let SeqLength::Locked(t) = s.length {
            return t;
        }
        if !self.visiting.insert(seq) {
            return Tick::ZERO;
        }
        let mut end = Tick::ZERO;
        for lane in &s.lanes {
            let lane_end = match lane.length {
                Some(l) => l,
                None => lane
                    .active()
                    .map(|t| {
                        t.events
                            .values()
                            .map(|e| self.event_end(doc, s, e))
                            .max()
                            .unwrap_or(Tick::ZERO)
                    })
                    .unwrap_or(Tick::ZERO),
            };
            end = end.max(lane_end);
        }
        self.visiting.remove(&seq);
        let len = s.meter.barline_at_or_after(end.max(Tick(1)));
        self.memo.insert(seq, len);
        len
    }

    /// Where an event in `owner` ends.
    pub(crate) fn event_end(&mut self, doc: &Document, owner: &Sequence, e: &Event) -> Tick {
        match &e.body {
            EventBody::Note { duration, .. } => e.tick + *duration,
            EventBody::Reference {
                seq,
                length,
                loop_count,
                ..
            } => {
                let b = self.of(doc, *seq);
                e.tick + reference_span(b, *length, *loop_count)
            }
            EventBody::AudioRegion { media, length, .. } => {
                let rate = doc
                    .media()
                    .get(media)
                    .map_or(48_000, |m| m.sample_rate.max(1));
                let start = owner.tempo.tick_to_seconds(e.tick);
                owner
                    .tempo
                    .seconds_to_tick(start + *length as f64 / f64::from(rate))
            }
            _ => e.tick,
        }
    }
}

/// The playing length of a sequence (derived lengths are computed, never
/// stored).
pub fn seq_length(doc: &Document, seq: SeqId) -> Result<Tick> {
    doc.sequence(seq)?;
    Ok(Lengths::default().of(doc, seq))
}
