//! Crossfades (plan 6c): which region a flagged region crosses, and how
//! much room the crossfade has.
//!
//! A crossfade is a flag on the *later* region ([`EventBody::AudioRegion`]'s
//! `crossfade`): its `fade_in` is the crossfade. The earlier region is not
//! stored. It is found by rule, the one the engine uses to decide what
//! plays: the partner is the region on the same take with the latest
//! `(tick, id)` that starts strictly earlier in ticks and is still sounding
//! at the later region's start, in frames.

use crate::command::Report;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{Event, EventBody, FadeShape};
use crate::ids::{EventId, LaneId, SeqId, TakeId};
use crate::lane::Take;
use crate::time::Tick;
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

/// Whether `e` is a region with the crossfade flag.
fn is_flagged(e: &Event) -> bool {
    matches!(
        e.body,
        EventBody::AudioRegion {
            crossfade: true,
            ..
        }
    )
}

/// The refusal of a split inside a crossfade (plan 6c Ruling 7).
const SPLIT_INSIDE: &str = "Split outside the crossfade, or remove the crossfade first.";

/// Refuses a split of region `event` at `at` strictly inside a crossfade
/// span `(R.start, R.start + fade_in)`, where `R` is `event` itself or a
/// flagged region whose partner is `event` (plan 6c Ruling 7).
pub(crate) fn check_split(doc: &Document, event: EventId, at: Tick) -> Result<()> {
    let (_, loc) = doc.event(event)?;
    let sequence = doc.sequence(loc.seq)?;
    let Some(take) = doc.lane(loc.seq, loc.lane)?.take(loc.take) else {
        return Ok(());
    };
    if !take.events.values().any(is_flagged) {
        return Ok(());
    }
    let tempo = &sequence.tempo;
    let found = overlaps(doc, loc.seq, take);
    for r in take.events.values().filter(|e| is_flagged(e)) {
        let partner = found.get(&r.id).map(|o| o.with);
        if r.id != event && partner != Some(event) {
            continue;
        }
        let EventBody::AudioRegion { media, fade_in, .. } = &r.body else {
            continue;
        };
        let Some(item) = doc.media().get(media) else {
            continue;
        };
        let rate = f64::from(item.sample_rate.max(1));
        let pos = ((tempo.tick_to_seconds(at) - tempo.tick_to_seconds(r.tick)) * rate).round();
        if pos > 0.0 && pos < fade_in.length_frames as f64 {
            return Err(ModelError::invalid(SPLIT_INSIDE));
        }
    }
    Ok(())
}

/// A region's name for a warning: its own, else its media's.
fn region_name(doc: &Document, e: &Event) -> String {
    match &e.body {
        EventBody::AudioRegion { name: Some(n), .. } => n.clone(),
        EventBody::AudioRegion { media, .. } => doc
            .media()
            .get(media)
            .map(|m| m.name.clone())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Where a take is.
type TakeKey = (SeqId, LaneId, TakeId);

/// The overlaps of the take at `key` in `doc`, computed once per settle.
fn overlaps_at<'a>(
    cache: &'a mut Vec<(TakeKey, BTreeMap<EventId, Overlap>)>,
    doc: &Document,
    key: TakeKey,
) -> Option<&'a BTreeMap<EventId, Overlap>> {
    let i = match cache.iter().position(|(k, _)| *k == key) {
        Some(i) => i,
        None => {
            let take = doc.lane(key.0, key.1).ok()?.take(key.2)?;
            cache.push((key, overlaps(doc, key.0, take)));
            cache.len() - 1
        }
    };
    cache.get(i).map(|(_, m)| m)
}

/// Whether `new_p` is the second part of a split of `old_p` (plan 6c F2):
/// it is new in this command, `old_p` still exists and ends exactly where
/// `new_p` starts, with the same media and the source continuing.
fn is_split_partner(old: &Document, new: &Document, old_p: EventId, new_p: EventId) -> bool {
    if old.locate(new_p).is_ok() {
        return false;
    }
    let (Ok((first, lf)), Ok((second, ls))) = (new.event(old_p), new.event(new_p)) else {
        return false;
    };
    if (lf.seq, lf.lane, lf.take) != (ls.seq, ls.lane, ls.take) || second.tick <= first.tick {
        return false;
    }
    let (
        EventBody::AudioRegion {
            media: m1,
            source_offset: o1,
            length: l1,
            ..
        },
        EventBody::AudioRegion {
            media: m2,
            source_offset: o2,
            ..
        },
    ) = (&first.body, &second.body)
    else {
        return false;
    };
    let (Some(item), Ok(sequence)) = (new.media().get(m1), new.sequence(lf.seq)) else {
        return false;
    };
    let rate = f64::from(item.sample_rate.max(1));
    let tempo = &sequence.tempo;
    let gap = frames_at(
        tempo.tick_to_seconds(second.tick) - tempo.tick_to_seconds(first.tick),
        rate,
    );
    m1 == m2 && o1.checked_add(*l1) == Some(*o2) && gap == i128::from(*l1)
}

/// Keeps, shortens or drops each crossfade a command disturbed (plan 6c
/// Ruling 6), in the same undo step: `old` is the document before the
/// command, `new` after it. It visits each take whose events changed, every
/// take of a sequence whose tempo changed or that is new, and every take
/// when the media pool changed. Each change gets a line in `r.warnings`,
/// except a lone copy losing its flag.
pub(crate) fn settle(old: &Document, new: &mut Document, r: &mut Report) -> Result<()> {
    // No media, no regions.
    if new.media().is_empty() {
        return Ok(());
    }
    let media_changed = !old.media().ptr_eq(new.media());
    let mut visit: Vec<TakeKey> = Vec::new();
    for s in new.sequences().values() {
        let old_s = old.sequences().get(&s.id);
        let all = media_changed || old_s.is_none_or(|o| o.tempo != s.tempo);
        for l in &s.lanes {
            let old_l = old_s.and_then(|o| o.lane(l.id));
            for t in &l.takes {
                let changed = all
                    || old_l
                        .and_then(|ol| ol.take(t.id))
                        .is_none_or(|ot| !ot.events.ptr_eq(&t.events));
                // Most takes have no crossfade: skip the sweep for them.
                if changed && t.events.values().any(is_flagged) {
                    visit.push((s.id, l.id, t.id));
                }
            }
        }
    }
    if visit.is_empty() {
        return Ok(());
    }
    let mut old_cache = Vec::new();
    let mut fixes: Vec<Event> = Vec::new();
    for key in visit {
        let doc: &Document = new;
        let Some(take) = doc.lane(key.0, key.1).ok().and_then(|l| l.take(key.2)) else {
            continue;
        };
        let found = overlaps(doc, key.0, take);
        for ev in take.events.values().filter(|e| is_flagged(e)) {
            let EventBody::AudioRegion { fade_in, .. } = &ev.body else {
                continue;
            };
            let name = region_name(doc, ev);
            let partner = found.get(&ev.id);
            // The region before the command, if it was flagged then.
            let was = old.event(ev.id).ok();
            let mut fixed = ev.clone();
            let clear = |e: &mut Event| {
                if let EventBody::AudioRegion { crossfade, .. } = &mut e.body {
                    *crossfade = false;
                }
            };
            let Some(o) = partner.filter(|_| fade_in.length_frames > 0) else {
                clear(&mut fixed);
                r.warnings.push(format!(
                    "Crossfade removed: region '{name}' no longer starts inside another region on its track."
                ));
                fixes.push(fixed);
                continue;
            };
            match was {
                Some((old_ev, loc)) if is_flagged(old_ev) => {
                    let old_partner =
                        overlaps_at(&mut old_cache, old, (loc.seq, loc.lane, loc.take))
                            .and_then(|m| m.get(&ev.id))
                            .map(|p| p.with);
                    let same = old_partner == Some(o.with)
                        || old_partner.is_some_and(|p| is_split_partner(old, doc, p, o.with));
                    if !same {
                        clear(&mut fixed);
                        r.warnings.push(format!(
                            "Crossfade removed: region '{name}' now starts inside a different region."
                        ));
                        fixes.push(fixed);
                        continue;
                    }
                }
                Some(_) => {}
                None => {
                    // A lone copy never joins an old region.
                    if old.locate(o.with).is_ok() {
                        clear(&mut fixed);
                        fixes.push(fixed);
                        continue;
                    }
                }
            }
            let room = o.frames.min(free_length(doc, ev.id)?);
            if fade_in.length_frames > room {
                if room == 0 {
                    clear(&mut fixed);
                    r.warnings.push(format!(
                        "Crossfade removed: region '{name}' no longer starts inside another region on its track."
                    ));
                } else {
                    if let EventBody::AudioRegion { fade_in, .. } = &mut fixed.body {
                        fade_in.length_frames = room;
                    }
                    r.warnings.push(format!(
                        "Crossfade on region '{name}' shortened to fit the overlap."
                    ));
                }
                fixes.push(fixed);
            }
        }
    }
    for e in fixes {
        new.replace_event(e)?;
    }
    Ok(())
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
