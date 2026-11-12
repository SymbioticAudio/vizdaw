//! Crossfades (plan 6c): which region a flagged region crosses, and how
//! much room the crossfade has.
//!
//! A crossfade is a flag on the *later* region ([`EventBody::AudioRegion`]'s
//! `crossfade`): its `fade_in` is the crossfade. The earlier region is not
//! stored. It is found by rule, the one the engine uses to decide what
//! plays: the partner is the region on the same take with the latest
//! `(tick, id)` that starts strictly earlier in ticks and is still sounding
//! at the later region's start, in frames.

use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{EventBody, FadeShape};
use crate::ids::{EventId, SeqId};
use crate::lane::Take;
use std::collections::BTreeMap;

/// The shape of a crossfade made without one (plan 6c Ruling 4): equal
/// power keeps the level of two unrelated sounds steady.
pub const DEFAULT_SHAPE: FadeShape = FadeShape::EqualPower;

/// A region's overlap with the region it starts inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overlap {
    /// The earlier region (the partner).
    pub with: EventId,
    /// How many frames of the later region's media rate the earlier region
    /// still sounds after the later one starts (at least 1).
    pub frames: u64,
}

/// A region seen by the sweep.
struct Seen {
    id: EventId,
    tick: i64,
    /// The region's start in seconds.
    secs: f64,
    /// The region's length in seconds.
    len_secs: f64,
}

/// `secs` at `rate` as whole frames (saturating, so hostile values never
/// wrap).
fn frames_at(secs: f64, rate: f64) -> i128 {
    (secs * rate).round() as i128
}

/// Each region of `take` that starts inside another, by the rule in the
/// module doc. A region that starts inside nothing is absent. `seq` supplies
/// the tempo map.
pub fn overlaps(doc: &Document, seq: SeqId, take: &Take) -> BTreeMap<EventId, Overlap> {
    let mut out = BTreeMap::new();
    let Ok(sequence) = doc.sequence(seq) else {
        return out;
    };
    let tempo = &sequence.tempo;
    // The regions still sounding at the current start, in take order.
    let mut sounding: Vec<Seen> = Vec::new();
    for ev in take.events.values() {
        let EventBody::AudioRegion { media, length, .. } = &ev.body else {
            continue;
        };
        let Some(item) = doc.media().get(media) else {
            continue;
        };
        let rate = f64::from(item.sample_rate.max(1));
        let secs = tempo.tick_to_seconds(ev.tick);
        let start = frames_at(secs, rate);
        sounding.retain(|p| frames_at(p.secs + p.len_secs, rate) > start);
        if let Some(p) = sounding.iter().rev().find(|p| p.tick < ev.tick.0) {
            let end = frames_at(p.secs + p.len_secs, rate);
            let frames = u64::try_from(end.saturating_sub(start)).unwrap_or(u64::MAX);
            out.insert(ev.id, Overlap { with: p.id, frames });
        }
        sounding.push(Seen {
            id: ev.id,
            tick: ev.tick.0,
            secs,
            len_secs: *length as f64 / rate,
        });
    }
    out
}

/// The overlap of region `event` (`Err` for an unknown event or a
/// non-region).
pub fn overlap(doc: &Document, event: EventId) -> Result<Option<Overlap>> {
    let (ev, loc) = doc.event(event)?;
    if !matches!(ev.body, EventBody::AudioRegion { .. }) {
        return Err(ModelError::invalid(format!(
            "{event} is not an audio region"
        )));
    }
    let take = doc
        .lane(loc.seq, loc.lane)?
        .take(loc.take)
        .ok_or(ModelError::UnknownId(crate::ids::AnyId::Take(loc.take)))?;
    Ok(overlaps(doc, loc.seq, take).remove(&event))
}

/// `min(overlap, length − fade_out)`; 0 without an overlap.
pub fn room(doc: &Document, event: EventId) -> Result<u64> {
    let Some(o) = overlap(doc, event)? else {
        return Ok(0);
    };
    Ok(o.frames.min(free_length(doc, event)?))
}

/// The region's length less its fade out: the most a fade in can take.
pub(crate) fn free_length(doc: &Document, event: EventId) -> Result<u64> {
    match &doc.event(event)?.0.body {
        EventBody::AudioRegion {
            length, fade_out, ..
        } => Ok(length.saturating_sub(fade_out.length_frames)),
        _ => Err(ModelError::invalid(format!(
            "{event} is not an audio region"
        ))),
    }
}

/// One line per crossfade that breaks the rules (for `validate`).
pub(crate) fn problems(doc: &Document) -> Vec<String> {
    let mut p = Vec::new();
    for s in doc.sequences().values() {
        for l in &s.lanes {
            for t in &l.takes {
                let flagged = |e: &&crate::event::Event| {
                    matches!(
                        e.body,
                        EventBody::AudioRegion {
                            crossfade: true,
                            ..
                        }
                    )
                };
                // Most takes have no crossfade: skip the sweep for them.
                if !t.events.values().any(|e| flagged(&e)) {
                    continue;
                }
                let found = overlaps(doc, s.id, t);
                for e in t.events.values().filter(flagged) {
                    let EventBody::AudioRegion {
                        length,
                        fade_in,
                        fade_out,
                        ..
                    } = &e.body
                    else {
                        continue;
                    };
                    // A missing media item is reported by the event's own
                    // check; a partner cannot be found without it.
                    if !matches!(&e.body, EventBody::AudioRegion { media, .. } if doc.media().contains_key(media))
                    {
                        continue;
                    }
                    let Some(o) = found.get(&e.id) else {
                        p.push(format!(
                            "crossfade {}: the region does not start inside another region",
                            e.id
                        ));
                        continue;
                    };
                    let room = o.frames.min(length.saturating_sub(fade_out.length_frames));
                    let n = fade_in.length_frames;
                    if room == 0 {
                        p.push(format!(
                            "crossfade {}: the region has no room for a crossfade",
                            e.id
                        ));
                    } else if n == 0 || n > room {
                        p.push(format!(
                            "crossfade {}: length {n} frames is not between 1 and {room}",
                            e.id
                        ));
                    }
                }
            }
        }
    }
    p
}
