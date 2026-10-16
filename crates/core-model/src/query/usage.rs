//! Usage queries: users and use counts of sequences, unused sequences,
//! unresolved events, media in use, and the drum view.

use super::routing::resolve;
use crate::command::drum::row_events;
use crate::document::Document;
use crate::error::Result;
use crate::event::EventBody;
use crate::ids::{EventId, LaneId, MediaId, SeqId};
use crate::lane::DrumRow;
use crate::rules::references_to;
use std::collections::BTreeMap;

/// Reference events (in any take) that use `seq`, sorted by ID.
pub fn users_of(doc: &Document, seq: SeqId) -> Vec<EventId> {
    let mut v = references_to(doc, seq);
    v.sort();
    v
}

/// How many reference events use `seq`.
pub fn use_count(doc: &Document, seq: SeqId) -> usize {
    references_to(doc, seq).len()
}

/// Use counts for every sequence (zero included), in one pass.
pub fn use_counts(doc: &Document) -> BTreeMap<SeqId, usize> {
    let mut m: BTreeMap<SeqId, usize> = doc.sequences().keys().map(|k| (*k, 0)).collect();
    for (_, e) in doc.all_events() {
        if let EventBody::Reference { seq, .. } = e.body {
            *m.entry(seq).or_insert(0) += 1;
        }
    }
    m
}

/// Sequences that no reference uses (top-level songs included; callers
/// decide which to keep).
pub fn unused_sequences(doc: &Document) -> Vec<SeqId> {
    use_counts(doc)
        .into_iter()
        .filter(|(_, n)| *n == 0)
        .map(|(s, _)| s)
        .collect()
}

/// MIDI events (in any take) with no resolvable destination.
pub fn unresolved_events(doc: &Document) -> Vec<EventId> {
    let mut out = Vec::new();
    for s in doc.sequences().values() {
        for l in &s.lanes {
            for t in &l.takes {
                for e in t.events.values() {
                    if e.body.is_midi() && resolve(doc, l, e).dest.is_none() {
                        out.push(e.id);
                    }
                }
            }
        }
    }
    out
}

/// Each media item in use, with the regions using it.
pub fn media_in_use(doc: &Document) -> BTreeMap<MediaId, Vec<EventId>> {
    let mut m: BTreeMap<MediaId, Vec<EventId>> = BTreeMap::new();
    for (_, e) in doc.all_events() {
        if let EventBody::AudioRegion { media, .. } = e.body {
            m.entry(media).or_default().push(e.id);
        }
    }
    m
}

/// The rows of a drum lane with the active-take notes in each.
pub fn drum_view(doc: &Document, seq: SeqId, lane: LaneId) -> Result<Vec<(DrumRow, Vec<EventId>)>> {
    let l = doc.lane(seq, lane)?;
    Ok(l.drum
        .as_ref()
        .map(|layout| {
            layout
                .rows
                .iter()
                .map(|r| (r.clone(), row_events(l, r, true)))
                .collect()
        })
        .unwrap_or_default())
}
