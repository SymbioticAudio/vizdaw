//! Matching the rows of the three sides (plan 6e Ruling 4): which base,
//! theirs and ours track, strip and marker are one thing, and within
//! matched tracks, which notes and regions are.
//!
//! - **Tracks.** Theirs↔base: by trusted XML ID or `vz:t` tag (the
//!   origin's lane plus the part's audio flag), else by name and content
//!   type among the unmatched, else, when no track was matched by ID or
//!   tag (the file lost its tags), by order among the unmatched of the
//!   same type: in a file that kept its tags an untagged track is new.
//!   Base↔ours: by lane (the snapshot's identity, format 2), else the
//!   same name and order rules.
//! - **Strips.** With their track, else the master with the master, else
//!   buses by ID or `vz:s` tag (theirs) or strip (ours), else by name.
//! - **Markers.** By `vz:m` index, else by name and tick (±[`NOTE_TICKS`]).
//! - **Notes** ([`notes`]) and **regions** ([`regions`]), within a matched
//!   track part.
//!
//! Each row appears in exactly one [`Matched`]: every pairing below takes
//! a row of each side at most once, first come.

use super::theirs::Theirs;
use super::{FRAMES, NOTE_TICKS, normalised};
use crate::IoError;
use crate::dawproject::import::Origins;
use crate::dawproject::record::{Identity, PTrack, Projection, Snapshot, TrackIds};
use core_model::ids::{EventId, LaneId, StripId};
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};

/// How two rows were found to be one thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchedBy {
    /// Theirs by a trusted XML ID (the record names the snapshot's export,
    /// Ruling 2).
    Id,
    /// Theirs by a `vz:` tag.
    Tag,
    /// By name (and content type, for tracks).
    Name,
    /// By order among the unmatched of the same type.
    Order,
    /// Ours by its lane, strip or place in the project (the snapshot's
    /// identity).
    Lane,
}

impl MatchedBy {
    /// How much less certain than a lane: the [`Matched`] row carries the
    /// least certain of its two pairings.
    fn doubt(self) -> u8 {
        match self {
            MatchedBy::Lane => 0,
            MatchedBy::Id => 1,
            MatchedBy::Tag => 2,
            MatchedBy::Name => 3,
            MatchedBy::Order => 4,
        }
    }
}

/// Indices into the three projections' rows (`None`: not on that side).
///
/// `by` is the less certain of the base↔theirs and base↔ours pairings
/// (a [`MatchedBy::Lane`] pairing never lowers it). A row matched to
/// nothing has `by: Id`, which then means nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Matched {
    /// The base row.
    pub base: Option<usize>,
    /// The theirs row.
    pub theirs: Option<usize>,
    /// The ours row.
    pub ours: Option<usize>,
    /// How they were matched.
    pub by: MatchedBy,
}

/// Every row of the three sides, matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matches {
    /// Rows of [`Projection::tracks`].
    pub tracks: Vec<Matched>,
    /// Rows of [`Projection::strips`].
    pub strips: Vec<Matched>,
    /// Rows of [`Projection::markers`].
    pub markers: Vec<Matched>,
    /// Some track was matched by name or order (`text::BY_NAME`).
    pub heuristic: bool,
}

/// One side's pairing with base: per base row, its partner and how; per
/// row of the other side, whether it is taken.
struct Pairs {
    of_base: Vec<Option<(usize, MatchedBy)>>,
    taken: Vec<bool>,
}

impl Pairs {
    fn new(base: usize, other: usize) -> Pairs {
        Pairs {
            of_base: vec![None; base],
            taken: vec![false; other],
        }
    }

    fn base_free(&self, b: usize) -> bool {
        matches!(self.of_base.get(b), Some(None))
    }

    fn other_free(&self, o: usize) -> bool {
        matches!(self.taken.get(o), Some(false))
    }

    /// Pairs `b` and `o` when both are free; whether it did.
    fn pair(&mut self, b: usize, o: usize, by: MatchedBy) -> bool {
        if !self.base_free(b) || !self.other_free(o) {
            return false;
        }
        if let (Some(slot), Some(t)) = (self.of_base.get_mut(b), self.taken.get_mut(o)) {
            *slot = Some((o, by));
            *t = true;
            true
        } else {
            false
        }
    }

    fn any(&self, f: impl Fn(MatchedBy) -> bool) -> bool {
        self.of_base.iter().flatten().any(|(_, by)| f(*by))
    }
}

/// The rows of the three sides from both pairings: one per base row, then
/// theirs' and ours' unpaired rows.
fn combine(theirs: &Pairs, ours: &Pairs) -> Vec<Matched> {
    let mut out = Vec::new();
    for (b, (t, o)) in theirs.of_base.iter().zip(&ours.of_base).enumerate() {
        let by = [t.map(|x| x.1), o.map(|x| x.1)]
            .into_iter()
            .flatten()
            .max_by_key(|by| by.doubt())
            .unwrap_or(MatchedBy::Id);
        out.push(Matched {
            base: Some(b),
            theirs: t.map(|x| x.0),
            ours: o.map(|x| x.0),
            by,
        });
    }
    for (i, taken) in theirs.taken.iter().enumerate() {
        if !taken {
            out.push(Matched {
                base: None,
                theirs: Some(i),
                ours: None,
                by: MatchedBy::Id,
            });
        }
    }
    for (i, taken) in ours.taken.iter().enumerate() {
        if !taken {
            out.push(Matched {
                base: None,
                theirs: None,
                ours: Some(i),
                by: MatchedBy::Id,
            });
        }
    }
    out
}

fn stop(cancel: &AtomicBool) -> Result<(), IoError> {
    if cancel.load(Ordering::Relaxed) {
        Err(IoError::Cancelled)
    } else {
        Ok(())
    }
}

/// Tracks by name and content type among the unmatched, then, when
/// `order`, by order among the unmatched of the same type.
fn by_name_then_order(base: &[PTrack], other: &[PTrack], order: bool, pairs: &mut Pairs) {
    let mut free: HashMap<(&str, bool), Vec<usize>> = HashMap::new();
    for (b, t) in base.iter().enumerate().rev() {
        if pairs.base_free(b) {
            free.entry((t.name.as_str(), t.audio)).or_default().push(b);
        }
    }
    for (o, t) in other.iter().enumerate() {
        if !pairs.other_free(o) {
            continue;
        }
        if let Some(b) = free.get_mut(&(t.name.as_str(), t.audio)).and_then(Vec::pop) {
            pairs.pair(b, o, MatchedBy::Name);
        }
    }
    if !order {
        return;
    }
    for audio in [false, true] {
        let bs = (0..base.len())
            .filter(|&b| pairs.base_free(b) && base.get(b).is_some_and(|t| t.audio == audio));
        let bs: Vec<usize> = bs.collect();
        let os: Vec<usize> = (0..other.len())
            .filter(|&o| pairs.other_free(o) && other.get(o).is_some_and(|t| t.audio == audio))
            .collect();
        for (b, o) in bs.into_iter().zip(os) {
            pairs.pair(b, o, MatchedBy::Order);
        }
    }
}

/// Base row index by (lane, audio part).
fn rows_by_lane(p: &Projection, ids: &Identity) -> HashMap<(u64, bool), usize> {
    let mut out = HashMap::new();
    for (i, (t, id)) in p.tracks.iter().zip(&ids.tracks).enumerate() {
        out.entry((id.lane, t.audio)).or_insert(i);
    }
    out
}

fn match_tracks(
    base: &Projection,
    base_ids: Option<&Identity>,
    theirs: &Theirs,
    trusted: MatchedBy,
    ours: &Projection,
    ours_ids: &Identity,
) -> (Pairs, Pairs) {
    let mut tb = Pairs::new(base.tracks.len(), theirs.projection.tracks.len());
    if let Some(ids) = base_ids {
        let rows = rows_by_lane(base, ids);
        for (o, (t, id)) in theirs
            .projection
            .tracks
            .iter()
            .zip(&theirs.identity.tracks)
            .enumerate()
        {
            let Some((lane, _)) = theirs.origins.lanes.get(&LaneId(id.lane)) else {
                continue;
            };
            if let Some(&b) = rows.get(&(*lane, t.audio)) {
                tb.pair(b, o, trusted);
            }
        }
    }
    // A file that kept its tags names every track it kept: an untagged
    // track is new, so order matches only a file that lost them.
    let lost = !tb.any(|_| true);
    by_name_then_order(&base.tracks, &theirs.projection.tracks, lost, &mut tb);

    let mut ob = Pairs::new(base.tracks.len(), ours.tracks.len());
    match base_ids {
        Some(ids) => {
            let rows = rows_by_lane(base, ids);
            for (o, (t, id)) in ours.tracks.iter().zip(&ours_ids.tracks).enumerate() {
                if let Some(&b) = rows.get(&(id.lane, t.audio)) {
                    ob.pair(b, o, MatchedBy::Lane);
                }
            }
        }
        None => by_name_then_order(&base.tracks, &ours.tracks, true, &mut ob),
    }
    (tb, ob)
}

/// The strips of the tracks `tracks` paired, paired with the tracks' `by`.
fn strips_with_tracks(base: &Projection, other: &Projection, tracks: &Pairs, strips: &mut Pairs) {
    for (b, pair) in tracks.of_base.iter().enumerate() {
        let Some((o, by)) = *pair else { continue };
        let bs = base.tracks.get(b).and_then(|t| t.strip);
        let os = other.tracks.get(o).and_then(|t| t.strip);
        if let (Some(bs), Some(os)) = (bs, os) {
            strips.pair(bs, os, by);
        }
    }
}

/// Strips no track of `p` plays through.
fn track_strips(p: &Projection) -> BTreeSet<usize> {
    p.tracks.iter().filter_map(|t| t.strip).collect()
}

/// The master with the master.
fn masters(base: &Projection, other: &Projection, by: MatchedBy, strips: &mut Pairs) {
    let master = |p: &Projection| p.strips.iter().position(|s| s.role == "master");
    if let (Some(b), Some(o)) = (master(base), master(other)) {
        strips.pair(b, o, by);
    }
}

/// Buses (strips no track plays through, other than the master) by name
/// among the unmatched.
fn buses_by_name(base: &Projection, other: &Projection, strips: &mut Pairs) {
    let bt = track_strips(base);
    let ot = track_strips(other);
    let mut free: HashMap<&str, Vec<usize>> = HashMap::new();
    for (b, s) in base.strips.iter().enumerate().rev() {
        if strips.base_free(b) && !bt.contains(&b) && s.role != "master" {
            free.entry(s.name.as_str()).or_default().push(b);
        }
    }
    for (o, s) in other.strips.iter().enumerate() {
        if strips.other_free(o) && !ot.contains(&o) && s.role != "master" {
            if let Some(b) = free.get_mut(s.name.as_str()).and_then(Vec::pop) {
                strips.pair(b, o, MatchedBy::Name);
            }
        }
    }
}

/// Base strip index by its strip ID (first wins).
fn strips_by_id(ids: &Identity) -> HashMap<u64, usize> {
    let mut out = HashMap::new();
    for (i, s) in ids.strips.iter().enumerate() {
        if let Some(id) = s.strip {
            out.entry(id).or_insert(i);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn match_strips(
    base: &Projection,
    base_ids: Option<&Identity>,
    theirs: &Theirs,
    trusted: MatchedBy,
    ours: &Projection,
    ours_ids: &Identity,
    tracks: (&Pairs, &Pairs),
) -> (Pairs, Pairs) {
    let tp = &theirs.projection;
    let mut tb = Pairs::new(base.strips.len(), tp.strips.len());
    strips_with_tracks(base, tp, tracks.0, &mut tb);
    let master_by = if trusted == MatchedBy::Id {
        MatchedBy::Id
    } else {
        MatchedBy::Name
    };
    masters(base, tp, master_by, &mut tb);
    if let Some(ids) = base_ids {
        let rows = strips_by_id(ids);
        for (o, id) in theirs.identity.strips.iter().enumerate() {
            let origin = id
                .strip
                .and_then(|s| theirs.origins.strips.get(&StripId(s)));
            if let Some(b) = origin.and_then(|s| rows.get(s)) {
                tb.pair(*b, o, trusted);
            }
        }
    }
    buses_by_name(base, tp, &mut tb);

    let mut ob = Pairs::new(base.strips.len(), ours.strips.len());
    strips_with_tracks(base, ours, tracks.1, &mut ob);
    match base_ids {
        Some(ids) => {
            let rows = strips_by_id(ids);
            for (o, id) in ours_ids.strips.iter().enumerate() {
                if let Some(b) = id.strip.and_then(|s| rows.get(&s)) {
                    ob.pair(*b, o, MatchedBy::Lane);
                }
            }
        }
        None => {
            masters(base, ours, MatchedBy::Name, &mut ob);
            buses_by_name(base, ours, &mut ob);
        }
    }
    (tb, ob)
}

/// Markers by name and tick (±[`NOTE_TICKS`]) among the unmatched, each
/// base marker taking the first; then, when `moved`, by name alone, in
/// order.
fn markers_by_name(
    base: &[(i64, String)],
    other: &[(i64, String)],
    by: MatchedBy,
    moved: bool,
    pairs: &mut Pairs,
) {
    let mut free: HashMap<&str, Vec<usize>> = HashMap::new();
    for (o, (_, name)) in other.iter().enumerate() {
        if pairs.other_free(o) {
            free.entry(name.as_str()).or_default().push(o);
        }
    }
    for (b, (tick, name)) in base.iter().enumerate() {
        if !pairs.base_free(b) {
            continue;
        }
        let Some(cands) = free.get_mut(name.as_str()) else {
            continue;
        };
        let near = cands.iter().position(|&o| {
            other
                .get(o)
                .is_some_and(|(t, _)| t.abs_diff(*tick) <= NOTE_TICKS.unsigned_abs())
        });
        if let Some(i) = near {
            let o = cands.remove(i);
            pairs.pair(b, o, by);
        }
    }
    if moved {
        for (b, (_, name)) in base.iter().enumerate() {
            if !pairs.base_free(b) {
                continue;
            }
            if let Some(cands) = free.get_mut(name.as_str())
                && !cands.is_empty()
            {
                let o = cands.remove(0);
                pairs.pair(b, o, by);
            }
        }
    }
}

fn match_markers(
    base: &Projection,
    format_two: bool,
    theirs: &Theirs,
    trusted: MatchedBy,
    ours: &Projection,
) -> (Pairs, Pairs) {
    let tp = &theirs.projection;
    let mut tb = Pairs::new(base.markers.len(), tp.markers.len());
    for (o, tag) in theirs.origins.markers.iter().enumerate() {
        if o >= tp.markers.len() {
            break;
        }
        if let Some(b) = tag.and_then(|t| usize::try_from(t).ok()) {
            tb.pair(b, o, trusted);
        }
    }
    markers_by_name(&base.markers, &tp.markers, MatchedBy::Name, false, &mut tb);

    let mut ob = Pairs::new(base.markers.len(), ours.markers.len());
    let by = if format_two {
        MatchedBy::Lane
    } else {
        MatchedBy::Name
    };
    markers_by_name(&base.markers, &ours.markers, by, true, &mut ob);
    (tb, ob)
}

/// Matches the rows of the three sides (Ruling 4), base and ours in the
/// file's frame ([`normalised`]), theirs as [`super::theirs::theirs`]
/// gives it. Base↔ours uses the snapshot's identity (format 2), else the
/// name and order rules; theirs↔base uses the import's origins (reported
/// as [`MatchedBy::Id`] when the import trusted the file's IDs, else
/// [`MatchedBy::Tag`]), then names and order. A format-1 snapshot, which
/// has no identity, matches tracks and buses by name and order alone.
pub fn match_all(
    base: &Snapshot,
    theirs: &Theirs,
    ours: &(Projection, Identity),
    cancel: &AtomicBool,
) -> Result<Matches, IoError> {
    stop(cancel)?;
    let b = normalised(&base.projection, base);
    let o = normalised(&ours.0, base);
    let base_ids = base.identity.as_ref();
    let trusted = if theirs.imp.ids_trusted() {
        MatchedBy::Id
    } else {
        MatchedBy::Tag
    };
    let (tt, ot) = match_tracks(&b, base_ids, theirs, trusted, &o, &ours.1);
    stop(cancel)?;
    let (ts, os) = match_strips(&b, base_ids, theirs, trusted, &o, &ours.1, (&tt, &ot));
    stop(cancel)?;
    let (tm, om) = match_markers(&b, base_ids.is_some(), theirs, trusted, &o);
    stop(cancel)?;
    let heuristic = tt.any(|by| matches!(by, MatchedBy::Name | MatchedBy::Order));
    Ok(Matches {
        tracks: combine(&tt, &ot),
        strips: combine(&ts, &os),
        markers: combine(&tm, &om),
        heuristic,
    })
}

/// The pairs of a greedy pairing: every row of `a` in order, with its
/// partner or `None`, then `b`'s unpaired rows.
fn pair_list(partner: &[Option<usize>], b_len: usize) -> Vec<(Option<usize>, Option<usize>)> {
    let mut taken = vec![false; b_len];
    let mut out: Vec<(Option<usize>, Option<usize>)> = Vec::with_capacity(partner.len() + b_len);
    for (i, p) in partner.iter().enumerate() {
        if let Some(t) = p.and_then(|j| taken.get_mut(j)) {
            *t = true;
        }
        out.push((Some(i), *p));
    }
    for (j, t) in taken.iter().enumerate() {
        if !t {
            out.push((None, Some(j)));
        }
    }
    out
}

/// Within matched rows: pairs of (base or ours, theirs) note indices.
///
/// Two notes are one when their pitch and channel are equal and their
/// starts and lengths are within [`NOTE_TICKS`]. Greedy in (tick, pitch)
/// order: each of `a`'s notes takes `b`'s first unpaired note (by tick)
/// within the tolerances. O(n log n): `b`'s notes are kept per (pitch,
/// channel) in a sorted set and looked up by tick range.
pub fn notes(a: &PTrack, b: &PTrack) -> Vec<(Option<usize>, Option<usize>)> {
    let mut free: HashMap<(u8, u8), BTreeSet<(i64, usize)>> = HashMap::new();
    for (j, n) in b.notes_played.iter().enumerate() {
        free.entry((n.pitch, n.channel))
            .or_default()
            .insert((n.tick, j));
    }
    let mut order: Vec<usize> = (0..a.notes_played.len()).collect();
    order.sort_by_key(|&i| a.notes_played.get(i).map(|n| (n.tick, n.pitch, i)));
    let mut partner = vec![None; a.notes_played.len()];
    for i in order {
        let Some(n) = a.notes_played.get(i) else {
            continue;
        };
        let Some(set) = free.get_mut(&(n.pitch, n.channel)) else {
            continue;
        };
        let lo = (n.tick.saturating_sub(NOTE_TICKS), 0);
        let hi = (n.tick.saturating_add(NOTE_TICKS), usize::MAX);
        let found = set
            .range(lo..=hi)
            .find(|(_, j)| {
                b.notes_played
                    .get(*j)
                    .is_some_and(|m| m.duration.abs_diff(n.duration) <= NOTE_TICKS.unsigned_abs())
            })
            .copied();
        if let Some(key) = found {
            set.remove(&key);
            if let Some(p) = partner.get_mut(i) {
                *p = Some(key.1);
            }
        }
    }
    pair_list(&partner, b.notes_played.len())
}

/// Within matched rows: pairs of (base or ours, theirs) region indices.
///
/// In three passes, each taking a row of each side at most once:
///
/// 1. by tag and position: `b`'s region whose clip's `vz:e` (its top
///    event's origin in `b`'s [`Origins`]) is the top event of `a`'s
///    region, with its tick within [`NOTE_TICKS`] and its length within
///    [`FRAMES`];
/// 2. by tag alone, in order (a moved clip);
/// 3. by media and position: the same content hash (or either side has
///    none: offline, or a copy with gain applied), the tick within
///    [`NOTE_TICKS`] and the length within [`FRAMES`].
pub fn regions(
    a: (&PTrack, &TrackIds),
    b: (&PTrack, &TrackIds, &Origins),
) -> Vec<(Option<usize>, Option<usize>)> {
    let (at, aids) = a;
    let (bt, bids, origins) = b;
    let near = |i: usize, j: usize| -> bool {
        match (at.regions.get(i), bt.regions.get(j)) {
            (Some(x), Some(y)) => {
                x.tick.abs_diff(y.tick) <= NOTE_TICKS.unsigned_abs()
                    && x.length.abs_diff(y.length) <= FRAMES
            }
            _ => false,
        }
    };
    let mut partner: Vec<Option<usize>> = vec![None; at.regions.len()];
    let mut taken = vec![false; bt.regions.len()];

    // `a`'s regions by their top event.
    let mut by_top: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, id) in aids.regions.iter().enumerate().take(at.regions.len()) {
        by_top.entry(id.top).or_default().push(i);
    }
    let tag_of = |j: usize| -> Option<u64> {
        let top = bids.regions.get(j)?.top;
        origins.events.get(&EventId(top)).copied()
    };
    for exact in [true, false] {
        for j in 0..bt.regions.len() {
            if taken.get(j).copied().unwrap_or(true) {
                continue;
            }
            let Some(cands) = tag_of(j).and_then(|t| by_top.get(&t)) else {
                continue;
            };
            let found = cands
                .iter()
                .copied()
                .find(|&i| matches!(partner.get(i), Some(None)) && (!exact || near(i, j)));
            if let Some(i) = found {
                if let Some(p) = partner.get_mut(i) {
                    *p = Some(j);
                }
                if let Some(t) = taken.get_mut(j) {
                    *t = true;
                }
            }
        }
    }

    // By media and position.
    let mut free: BTreeSet<(i64, usize)> = BTreeSet::new();
    for (j, r) in bt.regions.iter().enumerate() {
        if !taken.get(j).copied().unwrap_or(true) {
            free.insert((r.tick, j));
        }
    }
    for (i, r) in at.regions.iter().enumerate() {
        if !matches!(partner.get(i), Some(None)) {
            continue;
        }
        let lo = (r.tick.saturating_sub(NOTE_TICKS), 0);
        let hi = (r.tick.saturating_add(NOTE_TICKS), usize::MAX);
        let found = free
            .range(lo..=hi)
            .find(|(_, j)| {
                bt.regions.get(*j).is_some_and(|y| {
                    let media = match (&r.media_hash, &y.media_hash) {
                        (Some(x), Some(y)) => x == y,
                        _ => true,
                    };
                    media && near(i, *j)
                })
            })
            .copied();
        if let Some(key) = found {
            free.remove(&key);
            if let Some(p) = partner.get_mut(i) {
                *p = Some(key.1);
            }
        }
    }
    pair_list(&partner, bt.regions.len())
}
