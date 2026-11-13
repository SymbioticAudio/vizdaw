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
//!   Only strips of one role (a track's, a bus, the master) are matched.
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
use crate::dawproject::record::{EventAt, Identity, PTrack, Projection, Snapshot, TrackIds};
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
    /// Not matched: the row is on one side only (or a base row on neither
    /// other side).
    Unmatched,
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
            MatchedBy::Unmatched => 5,
        }
    }
}

/// Indices into the three projections' rows (`None`: not on that side).
///
/// `by` is the less certain of the base↔theirs and base↔ours pairings
/// (a [`MatchedBy::Lane`] pairing never lowers it). A row matched to
/// nothing has `by: Unmatched`.
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
    /// The file lost its track tags (no track matched by ID or tag), and
    /// some track was matched by name or order (`text::BY_NAME`).
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
            .unwrap_or(MatchedBy::Unmatched);
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
                by: MatchedBy::Unmatched,
            });
        }
    }
    for (i, taken) in ours.taken.iter().enumerate() {
        if !taken {
            out.push(Matched {
                base: None,
                theirs: None,
                ours: Some(i),
                by: MatchedBy::Unmatched,
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

/// Tracks of both pairings, and whether the file lost its tags (no track
/// matched by ID or tag).
fn match_tracks(
    base: &Projection,
    base_ids: Option<&Identity>,
    theirs: &Theirs,
    trusted: MatchedBy,
    ours: &Projection,
    ours_ids: &Identity,
) -> (Pairs, Pairs, bool) {
    let mut tb = Pairs::new(base.tracks.len(), theirs.projection.tracks.len());
    if let Some(ids) = base_ids {
        let rows = rows_by_lane(base, ids);
        // Rows whose origin names their own part first, so a hostile
        // origin naming the other part never wins over the real one.
        for exact in [true, false] {
            for (o, (t, id)) in theirs
                .projection
                .tracks
                .iter()
                .zip(&theirs.identity.tracks)
                .enumerate()
            {
                let Some((lane, audio)) = theirs.origins.lanes.get(&LaneId(id.lane)) else {
                    continue;
                };
                if (*audio == t.audio) != exact {
                    continue;
                }
                if let Some(&b) = rows.get(&(*lane, t.audio)) {
                    tb.pair(b, o, trusted);
                }
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
    (tb, ob, lost)
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

/// Strips a track of `p` plays through.
fn track_strips(p: &Projection) -> BTreeSet<usize> {
    p.tracks.iter().filter_map(|t| t.strip).collect()
}

/// What a strip is for: only strips of one role are ever one thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Track,
    Bus,
    Master,
}

/// Each strip's [`Role`] in `p`.
fn roles(p: &Projection) -> Vec<Role> {
    let tracks = track_strips(p);
    p.strips
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if s.role == "master" {
                Role::Master
            } else if tracks.contains(&i) {
                Role::Track
            } else {
                Role::Bus
            }
        })
        .collect()
}

/// Buses by their strip: `origin(o)` names the base strip ID of `other`'s
/// strip `o`. Only a bus pairs with a bus here: track strips pair with
/// their tracks and the master with the master, so a bus that names a
/// track's strip (or a track's channel that names a bus) stays unmatched,
/// and is new there.
fn buses_by_id(
    base: &Projection,
    ids: &Identity,
    other: &Projection,
    origin: impl Fn(usize) -> Option<u64>,
    by: MatchedBy,
    strips: &mut Pairs,
) {
    let rows = strips_by_id(ids);
    let (br, or) = (roles(base), roles(other));
    for (o, role) in or.iter().enumerate() {
        if *role != Role::Bus {
            continue;
        }
        if let Some(&b) = origin(o).and_then(|s| rows.get(&s))
            && br.get(b) == Some(&Role::Bus)
        {
            strips.pair(b, o, by);
        }
    }
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
        let origin = |o: usize| {
            let id = theirs.identity.strips.get(o)?.strip?;
            theirs.origins.strips.get(&StripId(id)).copied()
        };
        buses_by_id(base, ids, tp, origin, trusted, &mut tb);
    }
    buses_by_name(base, tp, &mut tb);

    let mut ob = Pairs::new(base.strips.len(), ours.strips.len());
    strips_with_tracks(base, ours, tracks.1, &mut ob);
    match base_ids {
        Some(ids) => {
            masters(base, ours, MatchedBy::Lane, &mut ob);
            let origin = |o: usize| ours_ids.strips.get(o)?.strip;
            buses_by_id(base, ids, ours, origin, MatchedBy::Lane, &mut ob);
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
    // Ours' markers have no identity; with a format-2 snapshot they are
    // labelled `Lane` (the project's own) only so that they never make a
    // row's `by` less certain than theirs' pairing.
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
    let (tt, ot, lost) = match_tracks(&b, base_ids, theirs, trusted, &o, &ours.1);
    stop(cancel)?;
    let (ts, os) = match_strips(&b, base_ids, theirs, trusted, &o, &ours.1, (&tt, &ot));
    stop(cancel)?;
    let (tm, om) = match_markers(&b, base_ids.is_some(), theirs, trusted, &o);
    stop(cancel)?;
    // In a file that kept its tags, a new track that took a deleted one's
    // name is not the file losing them (`BY_NAME`).
    let heuristic = lost && tt.any(|by| matches!(by, MatchedBy::Name | MatchedBy::Order));
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

/// Free rows of one side keyed by (tick, length, index), for lookups
/// within the tolerances in O(log n).
#[derive(Default)]
struct Near(BTreeSet<(i64, u64, usize)>);

impl Near {
    fn insert(&mut self, tick: i64, len: u64, i: usize) {
        self.0.insert((tick, len, i));
    }

    fn remove(&mut self, tick: i64, len: u64, i: usize) {
        self.0.remove(&(tick, len, i));
    }

    /// The first free row at `tick` (the tick itself first, then one
    /// before, then one after; within [`NOTE_TICKS`]) whose length is
    /// is `len`, else (only when none is) within `len_tol` of `len`, the
    /// lowest length then index first. At most `2 * (2 * NOTE_TICKS + 1)`
    /// range lookups.
    fn find(&self, tick: i64, len: u64, len_tol: u64) -> Option<(i64, u64, usize)> {
        let mut ticks = vec![tick];
        for d in 1..=NOTE_TICKS {
            ticks.push(tick.saturating_sub(d));
            ticks.push(tick.saturating_add(d));
        }
        let lo = len.saturating_sub(len_tol);
        let hi = len.saturating_add(len_tol);
        // The exact length first (at any of the ticks), so a row never
        // takes a near one that an exact row of the other side needs.
        let first = |lo: u64, hi: u64| {
            ticks.iter().find_map(|&t| {
                self.0
                    .range((t, lo, 0)..=(t, hi, usize::MAX))
                    .next()
                    .copied()
            })
        };
        first(len, len).or_else(|| first(lo, hi))
    }
}

/// A note's length as a key (a negative length, which no note has, is 0).
fn note_len(d: i64) -> u64 {
    u64::try_from(d).unwrap_or(0)
}

/// Within matched rows: pairs of (base or ours, theirs) note indices.
///
/// Two notes are one when their pitch and channel are equal and their
/// starts and lengths are within [`NOTE_TICKS`]. Greedy in (tick, pitch)
/// order: each of `a`'s notes takes an unpaired note of `b` within the
/// tolerances, at its own tick first. `b`'s notes are kept per (pitch,
/// channel) in a set sorted by (tick, length), so each lookup is a few
/// range queries: O(n log n) whatever the notes, never O(n²).
pub fn notes(a: &PTrack, b: &PTrack) -> Vec<(Option<usize>, Option<usize>)> {
    let tol = NOTE_TICKS.unsigned_abs();
    let mut free: HashMap<(u8, u8), Near> = HashMap::new();
    for (j, n) in b.notes_played.iter().enumerate() {
        free.entry((n.pitch, n.channel))
            .or_default()
            .insert(n.tick, note_len(n.duration), j);
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
        if let Some((t, l, j)) = set.find(n.tick, note_len(n.duration), tol) {
            set.remove(t, l, j);
            if let Some(p) = partner.get_mut(i) {
                *p = Some(j);
            }
        }
    }
    pair_list(&partner, b.notes_played.len())
}

/// What a region's media is known by: its content hash, else its media's
/// name (the stem of the file it came from).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MediaKey {
    Hash(String),
    Stem(String),
}

/// Within matched rows: pairs of (base or ours, theirs) region indices.
///
/// In three passes, each taking a row of each side at most once:
///
/// 1. by tag and position: `b`'s region whose clip's `vz:e` (its top
///    event's origin in `b`'s [`Origins`]) is the top event of `a`'s
///    region, or, for a region `a` plays through a reference use, its own
///    event (the export tags such a clip with the region inside the use,
///    the use giving the position), with its tick within [`NOTE_TICKS`]
///    and its length within [`FRAMES`];
/// 2. by tag alone, in order (a moved clip);
/// 3. by media and position (Ruling 4): the same content hash when both
///    have one, else the same media stem when both have one (an `Apply`
///    copy with gain, or offline media, has no hash); a region with
///    neither in common is never paired by position. Then the tick within
///    [`NOTE_TICKS`] and the length within [`FRAMES`].
///
/// Every pass looks rows up in sorted sets: O(n log n), never O(n²).
pub fn regions(
    a: (&PTrack, &TrackIds),
    b: (&PTrack, &TrackIds, &Origins),
) -> Vec<(Option<usize>, Option<usize>)> {
    let (at, aids) = a;
    let (bt, bids, origins) = b;
    let mut partner: Vec<Option<usize>> = vec![None; at.regions.len()];
    let mut taken = vec![false; bt.regions.len()];
    let take = |partner: &mut [Option<usize>], taken: &mut [bool], i: usize, j: usize| {
        if let Some(p) = partner.get_mut(i) {
            *p = Some(j);
        }
        if let Some(t) = taken.get_mut(j) {
            *t = true;
        }
    };

    // 1 and 2: `a`'s free regions per tag they answer to (the top event,
    // and for a region inside a use also its own), by position and in
    // order.
    let keys = |id: &EventAt| -> Vec<u64> {
        if id.event == id.top {
            vec![id.top]
        } else {
            vec![id.top, id.event]
        }
    };
    let mut near_top: HashMap<u64, Near> = HashMap::new();
    let mut order_top: HashMap<u64, BTreeSet<usize>> = HashMap::new();
    for (i, (r, id)) in at.regions.iter().zip(&aids.regions).enumerate() {
        for k in keys(id) {
            near_top.entry(k).or_default().insert(r.tick, r.length, i);
            order_top.entry(k).or_default().insert(i);
        }
    }
    let tag_of = |j: usize| -> Option<u64> {
        let top = bids.regions.get(j)?.top;
        origins.events.get(&EventId(top)).copied()
    };
    let drop_a = |near_top: &mut HashMap<u64, Near>,
                  order_top: &mut HashMap<u64, BTreeSet<usize>>,
                  i: usize| {
        let (Some(r), Some(id)) = (at.regions.get(i), aids.regions.get(i)) else {
            return;
        };
        for k in keys(id) {
            if let Some(n) = near_top.get_mut(&k) {
                n.remove(r.tick, r.length, i);
            }
            if let Some(o) = order_top.get_mut(&k) {
                o.remove(&i);
            }
        }
    };
    for (j, y) in bt.regions.iter().enumerate() {
        let Some(tag) = tag_of(j) else { continue };
        let found = near_top
            .get(&tag)
            .and_then(|n| n.find(y.tick, y.length, FRAMES));
        if let Some((_, _, i)) = found {
            drop_a(&mut near_top, &mut order_top, i);
            take(&mut partner, &mut taken, i, j);
        }
    }
    for j in 0..bt.regions.len() {
        if taken.get(j).copied().unwrap_or(true) {
            continue;
        }
        let Some(tag) = tag_of(j) else { continue };
        let found = order_top.get(&tag).and_then(|o| o.first().copied());
        if let Some(i) = found {
            drop_a(&mut near_top, &mut order_top, i);
            take(&mut partner, &mut taken, i, j);
        }
    }

    // 3: theirs' free regions per media key. A region with a hash is
    // under its hash and its stem; `Stem` buckets of hashed regions are
    // only for an `a` region with no hash.
    let mut by_key: HashMap<MediaKey, Near> = HashMap::new();
    let mut by_bare_stem: HashMap<&str, Near> = HashMap::new();
    for (j, y) in bt.regions.iter().enumerate() {
        if taken.get(j).copied().unwrap_or(true) {
            continue;
        }
        if let Some(h) = &y.media_hash {
            by_key
                .entry(MediaKey::Hash(h.clone()))
                .or_default()
                .insert(y.tick, y.length, j);
        }
        if let Some(st) = &y.stem {
            by_key
                .entry(MediaKey::Stem(st.clone()))
                .or_default()
                .insert(y.tick, y.length, j);
            if y.media_hash.is_none() {
                by_bare_stem
                    .entry(st.as_str())
                    .or_default()
                    .insert(y.tick, y.length, j);
            }
        }
    }
    for (i, r) in at.regions.iter().enumerate() {
        if !matches!(partner.get(i), Some(None)) {
            continue;
        }
        let found = match (&r.media_hash, &r.stem) {
            (Some(h), stem) => by_key
                .get(&MediaKey::Hash(h.clone()))
                .and_then(|n| n.find(r.tick, r.length, FRAMES))
                .or_else(|| {
                    let st = stem.as_deref()?;
                    by_bare_stem.get(st)?.find(r.tick, r.length, FRAMES)
                }),
            (None, Some(st)) => by_key
                .get(&MediaKey::Stem(st.clone()))
                .and_then(|n| n.find(r.tick, r.length, FRAMES)),
            (None, None) => None,
        };
        let Some((_, _, j)) = found else { continue };
        let Some(y) = bt.regions.get(j) else { continue };
        if let Some(h) = &y.media_hash
            && let Some(n) = by_key.get_mut(&MediaKey::Hash(h.clone()))
        {
            n.remove(y.tick, y.length, j);
        }
        if let Some(st) = &y.stem {
            if let Some(n) = by_key.get_mut(&MediaKey::Stem(st.clone())) {
                n.remove(y.tick, y.length, j);
            }
            if let Some(n) = by_bare_stem.get_mut(st.as_str()) {
                n.remove(y.tick, y.length, j);
            }
        }
        take(&mut partner, &mut taken, i, j);
    }
    pair_list(&partner, bt.regions.len())
}
