//! Ruling 8 (plan 6e Task 6b): theirs' edits inside reference uses, for a
//! format-2 snapshot (a format-1 one counts a use's notes as its track's,
//! Ruling 3).
//!
//! **Attribution.** A change theirs makes to what plays through one of
//! ours' (unmuted) uses belongs to that use: ours' side when its event
//! plays through the use; theirs' side when its clip is tagged `vz:e<use>`,
//! else when it lies in the span of theirs' clip so tagged, else in the
//! use's own span. A tag naming another of ours' events is not a use's.
//!
//! **Each use's edit** is made relative: ticks from the start of its pass
//! (less the inner track's play shift), keys with its transpose taken
//! off. A looped use's passes must all have the same edit; otherwise, or
//! when an edit cannot be written into the sequence so that it plays the
//! same (a nested use, an inner track with its own loop, length or play
//! quantize, a key out of range, an event outside the pass's window, a
//! crossfaded or muted region), the use is replaced by what theirs plays
//! (`unroll`, [`text::LOOPS_UNROLLED`]).
//!
//! **Each sequence:** when every exported use of it has the same edit
//! (and the export plays it inside no use of another sequence), the edit
//! goes into the sequence once ([`text::SHARED_ELSEWHERE`] when
//! it is also used outside the merged sequence). Otherwise each use with
//! an edit gets `ReferenceCmd::MakePrivateCopy`, then its edit
//! ([`text::PRIVATE_COPIES`]). A paired event is edited in place
//! (`EventCmd::Replace`), so it keeps its event.
//!
//! [`text::LOOPS_UNROLLED`]: super::text::LOOPS_UNROLLED
//! [`text::SHARED_ELSEWHERE`]: super::text::SHARED_ELSEWHERE
//! [`text::PRIVATE_COPIES`]: super::text::PRIVATE_COPIES

use super::{Cx, LaneEdit, lane_rows, run};
use crate::dawproject::merge::decide::{same_note, same_region, ticks_near, used_elsewhere};
use crate::dawproject::record::{EventAt, PNote, PRegion, UseAt};
use core_model::command::{Command, EventCmd, ReferenceCmd};
use core_model::document::Document;
use core_model::error::ModelError;
use core_model::event::{EventBody, EventSpec, LoopCount};
use core_model::history::History;
use core_model::ids::{AnyId, EventId, SeqId};
use core_model::lane::Lane;
use core_model::query::{reference_span, seq_length};
use core_model::sequence::Sequence;
use core_model::time::Tick;
use std::collections::{BTreeMap, BTreeSet};
use std::mem::discriminant;

/// What a changed played event is, to compare edits.
#[derive(Debug, Clone)]
pub(super) enum Key {
    /// A note, as theirs plays it.
    Note(PNote),
    /// Another MIDI event (compared by its spec's body).
    Midi,
    /// A region, as theirs plays it.
    Region(PRegion),
}

/// Ours' played event a change takes away.
#[derive(Debug, Clone, Copy)]
pub(super) struct Lost {
    /// Who it is.
    pub at: EventAt,
    /// Where it plays (ours' frame).
    pub tick: i64,
}

/// Theirs' played event a change puts in.
#[derive(Debug, Clone)]
pub(super) struct Gained {
    /// Where it plays, in ours' frame.
    pub tick: i64,
    /// What it is.
    pub key: Key,
    /// It as a top-level event of the track.
    pub spec: EventSpec,
}

/// One change inside use `use_ev`.
#[derive(Debug, Clone)]
pub(super) struct Change {
    /// Ours' use.
    pub use_ev: u64,
    /// Ours' event it removes.
    pub ours: Option<Lost>,
    /// Theirs' event it adds.
    pub theirs: Option<Gained>,
}

// ---------------------------------------------------------------- owners

/// Which use of a track a played event belongs to.
pub(super) struct Owners {
    /// The track's unmuted uses, by tick.
    uses: Vec<UseAt>,
    /// Each use's span from theirs' clips tagged with it.
    spans: BTreeMap<u64, (i64, i64)>,
    /// The sequence the track is in.
    seq: SeqId,
}

impl Owners {
    /// Ours' lane `lane`'s uses, against theirs track `t`.
    pub(super) fn new(doc: &Document, cx: &Cx<'_>, lane: u64, t: usize) -> Owners {
        let mut uses: Vec<UseAt> = lane_rows(cx, lane)
            .into_iter()
            .filter_map(|o| cx.ids.tracks.get(o))
            .flat_map(|i| i.uses.iter().copied())
            .filter(|u| doc.event(EventId(u.event)).is_ok_and(|(e, _)| !e.mute))
            .collect();
        uses.sort_by_key(|u| (u.tick, u.event));
        uses.dedup_by_key(|u| u.event);
        let mut o = Owners {
            uses,
            spans: BTreeMap::new(),
            seq: cx.seq,
        };
        let (Some(tt), Some(ti)) = (cx.th.projection.tracks.get(t), cx.th.identity.tracks.get(t))
        else {
            return o;
        };
        let mut tops = BTreeSet::new();
        let mut grow = |o: &mut Owners, at: &EventAt, lo: i64, hi: i64| {
            let Some(u) = o.tagged(cx, at) else { return };
            tops.insert(at.top);
            let s = o.spans.entry(u).or_insert((lo, hi));
            s.0 = s.0.min(lo);
            s.1 = s.1.max(hi);
        };
        let raw = |t: i64| t.saturating_add(cx.origin);
        for (n, at) in tt.notes_played.iter().zip(&ti.notes) {
            let lo = raw(n.tick);
            grow(&mut o, at, lo, lo.saturating_add(n.duration.max(1)));
        }
        for (m, at) in tt.midi.iter().zip(&ti.midi) {
            let lo = raw(m.0);
            grow(&mut o, at, lo, lo.saturating_add(1));
        }
        for (r, at) in tt.regions.iter().zip(&ti.regions) {
            let lo = raw(r.tick);
            grow(&mut o, at, lo, lo.saturating_add(1));
        }
        // A tagged clip the import kept as a use spans that use.
        for top in tops {
            let Ok((e, loc)) = cx.th.doc.event(EventId(top)) else {
                continue;
            };
            let EventBody::Reference {
                seq,
                length,
                loop_count,
                ..
            } = e.body
            else {
                continue;
            };
            let Some(&u) = cx.th.origins.events.get(&EventId(top)) else {
                continue;
            };
            let shift = cx
                .th
                .doc
                .lane(loc.seq, loc.lane)
                .map_or(0, |l| l.play_shift.0);
            let b = seq_length(&cx.th.doc, seq).unwrap_or(Tick::ZERO);
            let lo = raw(e.tick.0.saturating_add(shift));
            let hi = lo.saturating_add(reference_span(b, length, loop_count).0);
            let s = o.spans.entry(u).or_insert((lo, hi));
            s.0 = s.0.min(lo);
            s.1 = s.1.max(hi);
        }
        o
    }

    /// The use theirs' event `at` (the scratch project's) is tagged with.
    fn tagged(&self, cx: &Cx<'_>, at: &EventAt) -> Option<u64> {
        let tag = *cx.th.origins.events.get(&EventId(at.top))?;
        self.uses.iter().any(|u| u.event == tag).then_some(tag)
    }

    /// The use ours' played event `at` plays through.
    pub(super) fn of_ours(&self, at: &EventAt) -> Option<u64> {
        (at.event != at.top && self.uses.iter().any(|u| u.event == at.top)).then_some(at.top)
    }

    /// The use theirs' played event `at` (the scratch project's), playing
    /// at `tick` in ours' frame, belongs to.
    pub(super) fn of_theirs(
        &self,
        doc: &Document,
        cx: &Cx<'_>,
        at: &EventAt,
        tick: i64,
    ) -> Option<u64> {
        if let Some(u) = self.tagged(cx, at) {
            return Some(u);
        }
        // A tag naming another of ours' events in the merged sequence.
        if let Some(tag) = cx.th.origins.events.get(&EventId(at.top))
            && doc
                .event(EventId(*tag))
                .is_ok_and(|(_, loc)| loc.seq == self.seq)
        {
            return None;
        }
        self.uses.iter().find_map(|u| {
            let (lo, hi) = self.spans.get(&u.event).copied().unwrap_or((u.tick, u.end));
            (lo <= tick && tick < hi).then_some(u.event)
        })
    }
}

// ---------------------------------------------------------------- edits

/// One event an edit adds to the sequence.
#[derive(Debug, Clone)]
struct Rel {
    /// The sequence's track (its index).
    lane: usize,
    /// The sequence's tick.
    tick: i64,
    key: Key,
    /// It as the sequence's event.
    spec: EventSpec,
    /// The sequence's event it edits in place.
    replaces: Option<u64>,
}

/// An edit to a sequence: its events deleted (or replaced), events added.
#[derive(Debug, Clone, Default)]
struct Edit {
    dels: BTreeSet<u64>,
    adds: Vec<Rel>,
}

fn near(a: &Rel, b: &Rel) -> bool {
    if a.lane != b.lane || !ticks_near(a.tick, b.tick) {
        return false;
    }
    match (&a.key, &b.key) {
        (Key::Note(x), Key::Note(y)) => {
            x.pitch == y.pitch
                && x.channel == y.channel
                && ticks_near(x.duration, y.duration)
                && same_note(x, y)
        }
        (Key::Region(x), Key::Region(y)) => same_region(x, y),
        (Key::Midi, Key::Midi) => {
            a.spec.body == b.spec.body && a.spec.group_channel == b.spec.group_channel
        }
        _ => false,
    }
}

impl Edit {
    /// The same edit, within the tolerances.
    fn same(&self, other: &Edit) -> bool {
        if self.dels != other.dels || self.adds.len() != other.adds.len() {
            return false;
        }
        let mut free: BTreeMap<(usize, i64), Vec<usize>> = BTreeMap::new();
        for (i, r) in other.adds.iter().enumerate() {
            free.entry((r.lane, r.tick)).or_default().push(i);
        }
        for a in &self.adds {
            let lo = (
                a.lane,
                a.tick.saturating_sub(crate::dawproject::merge::NOTE_TICKS),
            );
            let hi = (
                a.lane,
                a.tick.saturating_add(crate::dawproject::merge::NOTE_TICKS),
            );
            let found = free.range_mut(lo..=hi).find_map(|(_, v)| {
                let p = v
                    .iter()
                    .position(|&i| other.adds.get(i).is_some_and(|b| near(a, b)))?;
                Some(v.remove(p))
            });
            if found.is_none() {
                return false;
            }
        }
        true
    }
}

/// How one use takes theirs' changes.
enum Took {
    /// One edit, the same on every pass.
    Edit(Edit),
    /// Replaced by what theirs plays.
    Unroll,
}

/// Whether a sequence's track plays its events where they are.
fn plain(l: &Lane) -> bool {
    !l.looped && l.length.is_none() && l.play_quantize.is_none()
}

/// `p` played through a use transposed by `by` on track `l`, untransposed.
fn untransposed(p: u8, by: i32, l: &Lane) -> Option<u8> {
    if l.drum.is_some() {
        return Some(p);
    }
    u8::try_from(i32::from(p).checked_sub(by)?)
        .ok()
        .filter(|k| *k <= 127)
}

/// Ours' event `at` in sequence `target` (directly, not through a use of
/// another sequence): its track's index, its body and its track.
fn in_seq<'d>(
    doc: &'d Document,
    target: &'d Sequence,
    at: &EventAt,
) -> Option<(usize, &'d EventBody, &'d Lane)> {
    let (ev, loc) = doc.event(EventId(at.event)).ok()?;
    if loc.seq != target.id {
        return None;
    }
    let i = target.lanes.iter().position(|l| l.id == loc.lane)?;
    Some((i, &ev.body, target.lanes.get(i)?))
}

/// Use `u`'s changes as one edit of its sequence, or `None` when it must
/// be unrolled.
fn took(doc: &Document, cx: &Cx<'_>, u: &UseAt, changes: &[Change]) -> Option<Edit> {
    let (e, _) = doc.event(EventId(u.event)).ok()?;
    let EventBody::Reference {
        seq: s,
        length,
        loop_count,
        transpose,
    } = e.body
    else {
        return None;
    };
    let target = doc.sequence(s).ok()?;
    let b = seq_length(doc, s).ok()?.0.max(1);
    let use_len = length.map_or(b, |t| t.0).max(1);
    let iter = match loop_count {
        LoopCount::Count(_) => use_len,
        LoopCount::Fill => b,
    };
    let song = seq_length(doc, cx.seq).ok()?.0;
    let cut = u.end.min(song);
    // The passes that play.
    let span = i128::from(cut) - i128::from(u.tick);
    if span <= 0 {
        return None;
    }
    let played = ((span + i128::from(iter) - 1) / i128::from(iter)).min(i128::from(u.passes));
    let played = i64::try_from(played).ok()?;
    if played <= 0 {
        return None;
    }
    let pass_of = |tick: i64| {
        tick.saturating_sub(u.tick)
            .div_euclid(iter)
            .clamp(0, played.saturating_sub(1))
    };
    let start_of = |k: i64| u.tick.saturating_add(iter.saturating_mul(k));
    let index_of = |at: &EventAt| in_seq(doc, target, at);
    // The sequence's track a new event of each kind goes on: the first
    // that plays one through this use.
    let first_lane = |region: bool| -> usize {
        cx.ids
            .tracks
            .iter()
            .flat_map(|t| {
                let v: &[EventAt] = if region { &t.regions } else { &t.notes };
                v.iter()
            })
            .filter(|at| at.top == u.event)
            .find_map(|at| index_of(at).map(|x| x.0))
            .unwrap_or(0)
    };
    let (note_lane, region_lane) = (first_lane(false), first_lane(true));
    let mut passes: BTreeMap<i64, Edit> = BTreeMap::new();
    for c in changes {
        let mut ours_lane = None;
        if let Some(l) = &c.ours {
            let (i, body, lane) = index_of(&l.at)?;
            if !plain(lane)
                || matches!(
                    body,
                    EventBody::AudioRegion {
                        crossfade: true,
                        ..
                    }
                )
            {
                return None;
            }
            passes
                .entry(pass_of(l.tick))
                .or_default()
                .dels
                .insert(l.at.event);
            ours_lane = Some((i, pass_of(l.tick), body.clone()));
        }
        let Some(g) = &c.theirs else { continue };
        let k = pass_of(g.tick);
        let start = start_of(k);
        let lo = if k == 0 {
            target.start_offset.0.saturating_neg()
        } else {
            0
        };
        let hi = iter.min(cut.saturating_sub(start));
        let x = g.tick.saturating_sub(start);
        if x < lo || x >= hi {
            return None;
        }
        let paired = ours_lane.as_ref().filter(|(_, pk, _)| *pk == k);
        let li = match paired {
            Some((i, _, _)) => *i,
            None if matches!(g.key, Key::Region(_)) => region_lane,
            None => note_lane,
        };
        let lane = target.lanes.get(li)?;
        if !plain(lane) {
            return None;
        }
        let tick = x.saturating_sub(lane.play_shift.0);
        let mut spec = g.spec.clone();
        spec.tick = Tick(tick);
        let by = i32::from(transpose);
        let mut key = g.key.clone();
        match (&mut spec.body, &mut key) {
            (EventBody::Note { pitch, .. }, Key::Note(n)) => {
                *pitch = untransposed(*pitch, by, lane)?;
                n.pitch = *pitch;
                n.tick = tick;
            }
            (EventBody::PolyPressure { pitch, .. }, Key::Midi) => {
                *pitch = untransposed(*pitch, by, lane)?;
            }
            (EventBody::AudioRegion { crossfade, .. }, Key::Region(r)) => {
                if *crossfade || r.crossfade || r.muted || spec.mute {
                    return None;
                }
                r.tick = tick;
            }
            (_, Key::Midi) => {}
            _ => return None,
        }
        let replaces = paired
            .filter(|(_, _, body)| discriminant(body) == discriminant(&spec.body))
            .and_then(|_| c.ours.map(|l| l.at.event));
        if let Some(r) = replaces
            && let Ok((ev, _)) = doc.event(EventId(r))
        {
            spec.dest = ev.dest;
        }
        passes.entry(k).or_default().adds.push(Rel {
            lane: li,
            tick,
            key,
            spec,
            replaces,
        });
    }
    if i64::try_from(passes.len()).ok()? != played {
        return None;
    }
    let mut each = passes.into_values();
    let mut first = each.next()?;
    if !each.all(|p| p.same(&first)) {
        return None;
    }
    moves(doc, target, &mut first);
    Some(first)
}

/// Pairs the notes `edit` deletes with the ones it adds of the same key on
/// the same track (nearest first), so a moved note is edited in place and
/// keeps its event.
fn moves(doc: &Document, target: &Sequence, edit: &mut Edit) {
    let taken: BTreeSet<u64> = edit.adds.iter().filter_map(|r| r.replaces).collect();
    let mut free: BTreeMap<(usize, u8), BTreeSet<(i64, u64)>> = BTreeMap::new();
    for &e in edit.dels.iter().filter(|e| !taken.contains(e)) {
        let Some((i, body, _)) = in_seq(doc, target, &EventAt { event: e, top: e }) else {
            continue;
        };
        if let (EventBody::Note { pitch, .. }, Ok((ev, _))) = (body, doc.event(EventId(e))) {
            free.entry((i, *pitch)).or_default().insert((ev.tick.0, e));
        }
    }
    for r in edit.adds.iter_mut().filter(|r| r.replaces.is_none()) {
        let EventBody::Note { pitch, .. } = r.spec.body else {
            continue;
        };
        let Some(set) = free.get_mut(&(r.lane, pitch)) else {
            continue;
        };
        let after = set.range((r.tick, 0)..).next().copied();
        let before = set.range(..(r.tick, 0)).next_back().copied();
        let pick = match (before, after) {
            (Some(b), Some(a)) => {
                if r.tick.saturating_sub(b.0) <= a.0.saturating_sub(r.tick) {
                    b
                } else {
                    a
                }
            }
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => continue,
        };
        set.remove(&pick);
        r.replaces = Some(pick.1);
        if let Ok((ev, _)) = doc.event(EventId(pick.1)) {
            r.spec.dest = ev.dest;
        }
    }
}

// ---------------------------------------------------------------- the plan

/// What goes into a sequence.
enum SeqEdit {
    /// The sequence itself.
    Direct(SeqId, Edit),
    /// A private copy of it for use `.0`.
    Copy(u64, SeqId, Edit),
}

/// Ruling 8's sequence edits and lines.
#[derive(Default)]
pub(super) struct Shared {
    edits: Vec<SeqEdit>,
    /// [`text::PRIVATE_COPIES`] details.
    ///
    /// [`text::PRIVATE_COPIES`]: super::text::PRIVATE_COPIES
    pub copies: Vec<String>,
    /// [`text::SHARED_ELSEWHERE`] details.
    ///
    /// [`text::SHARED_ELSEWHERE`]: super::text::SHARED_ELSEWHERE
    pub elsewhere: Vec<String>,
}

/// "Sequence '<name>'".
fn seq_detail(doc: &Document, s: SeqId) -> String {
    let name = doc.sequence(s).map(|x| x.name.clone()).unwrap_or_default();
    format!("Sequence '{name}'")
}

/// The sequences the export plays inside a use of another sequence.
fn nested(doc: &Document, cx: &Cx<'_>, uses: &BTreeMap<u64, UseAt>) -> BTreeSet<SeqId> {
    let mut out = BTreeSet::new();
    for t in &cx.ids.tracks {
        let played = t
            .notes
            .iter()
            .chain(&t.midi)
            .chain(&t.regions)
            .chain(t.automation.iter().flatten());
        for at in played.filter(|at| at.event != at.top) {
            let Some(u) = uses.get(&at.top) else { continue };
            if let Ok((_, loc)) = doc.event(EventId(at.event))
                && loc.seq.0 != u.seq
            {
                out.insert(loc.seq);
            }
        }
    }
    out
}

/// Takes every lane edit's changes inside uses and decides each use: a
/// use to unroll goes back into its lane's edit (ours' events deleted,
/// theirs' inserted); the others become sequence edits, for [`write`].
pub(super) fn resolve(doc: &Document, cx: &Cx<'_>, edits: &mut [(u64, LaneEdit)]) -> Shared {
    let mut changed: BTreeMap<u64, (usize, Vec<Change>)> = BTreeMap::new();
    for (k, (_, e)) in edits.iter_mut().enumerate() {
        for c in std::mem::take(&mut e.inside) {
            changed.entry(c.use_ev).or_insert((k, Vec::new())).1.push(c);
        }
    }
    let mut out = Shared::default();
    if changed.is_empty() {
        return out;
    }
    // Every exported use, by sequence.
    let mut by_seq: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    let mut all: BTreeMap<u64, UseAt> = BTreeMap::new();
    for t in &cx.ids.tracks {
        for u in &t.uses {
            by_seq.entry(u.seq).or_default().insert(u.event);
            all.insert(u.event, *u);
        }
    }
    let mut took_: BTreeMap<u64, Took> = BTreeMap::new();
    for (u, (_, changes)) in &changed {
        let t = all
            .get(u)
            .and_then(|at| took(doc, cx, at, changes))
            .map_or(Took::Unroll, Took::Edit);
        took_.insert(*u, t);
    }
    let elsewhere = used_elsewhere(doc, cx.seq);
    let nested = nested(doc, cx, &all);
    for (s, uses) in &by_seq {
        if !uses.iter().any(|u| changed.contains_key(u)) {
            continue;
        }
        let s = SeqId(*s);
        let each: Vec<Option<&Edit>> = uses
            .iter()
            .map(|u| match took_.get(u) {
                Some(Took::Edit(e)) => Some(e),
                _ => None,
            })
            .collect();
        let first = each.first().copied().flatten();
        // A sequence that also plays inside another exported use would
        // change what that use plays: never edited itself.
        let uniform = !nested.contains(&s)
            && first.is_some_and(|f| each.iter().all(|e| e.is_some_and(|e| e.same(f))));
        match first {
            Some(f) if uniform => {
                out.edits.push(SeqEdit::Direct(s, f.clone()));
                if elsewhere.contains(&s) {
                    out.elsewhere.push(seq_detail(doc, s));
                }
            }
            _ => {
                let mut any = false;
                for u in uses {
                    if let Some(Took::Edit(e)) = took_.get(u) {
                        out.edits.push(SeqEdit::Copy(*u, s, e.clone()));
                        any = true;
                    }
                }
                if any {
                    out.copies.push(seq_detail(doc, s));
                }
            }
        }
    }
    // The uses to unroll go back to their lanes.
    for (u, (k, changes)) in changed {
        if !matches!(took_.get(&u), Some(Took::Unroll)) {
            continue;
        }
        let Some((_, e)) = edits.get_mut(k) else {
            continue;
        };
        e.unroll.insert(u);
        for c in changes {
            if let Some(l) = c.ours {
                e.delete(&l.at, l.tick);
            }
            if let Some(g) = c.theirs {
                e.insert.push(g.spec);
            }
        }
    }
    out
}

/// Each sequence's events, by take, in order (`MakePrivateCopy` copies
/// them in that order).
fn events_of(s: &Sequence) -> Vec<u64> {
    s.lanes
        .iter()
        .flat_map(|l| l.takes.iter())
        .flat_map(|t| t.events.values().map(|e| e.id.0))
        .collect()
}

/// Carries out [`resolve`]'s sequence edits (after the lanes' edits, so an
/// unrolled use has played the sequence as it was).
pub(super) fn write(h: &mut History, shared: &Shared) -> Result<(), ModelError> {
    for e in &shared.edits {
        match e {
            SeqEdit::Direct(s, edit) => edit_seq(h, *s, edit, None)?,
            SeqEdit::Copy(u, s, edit) => {
                let before = events_of(h.doc().sequence(*s)?);
                let r = run(
                    h,
                    Command::Reference(ReferenceCmd::MakePrivateCopy { event: EventId(*u) }),
                )?;
                let copy = r
                    .created
                    .iter()
                    .find_map(|id| match id {
                        AnyId::Seq(s) => Some(*s),
                        _ => None,
                    })
                    .ok_or_else(|| ModelError::invalid("no private copy was made"))?;
                let after = events_of(h.doc().sequence(copy)?);
                let map: BTreeMap<u64, u64> = before.into_iter().zip(after).collect();
                edit_seq(h, copy, edit, Some(&map))?;
            }
        }
    }
    Ok(())
}

/// `edit` written into sequence `s`, its events named through `map` (a
/// private copy's).
fn edit_seq(
    h: &mut History,
    s: SeqId,
    edit: &Edit,
    map: Option<&BTreeMap<u64, u64>>,
) -> Result<(), ModelError> {
    let id = |e: u64| map.map_or(Some(e), |m| m.get(&e).copied()).map(EventId);
    let replaced: BTreeSet<u64> = edit.adds.iter().filter_map(|r| r.replaces).collect();
    let dels: Vec<EventId> = edit
        .dels
        .iter()
        .filter(|e| !replaced.contains(e))
        .filter_map(|e| id(*e))
        .collect();
    if !dels.is_empty() {
        run(h, Command::Event(EventCmd::Delete { events: dels }))?;
    }
    let mut inserts: BTreeMap<usize, Vec<EventSpec>> = BTreeMap::new();
    for r in &edit.adds {
        match r.replaces.and_then(id) {
            Some(event) => {
                run(
                    h,
                    Command::Event(EventCmd::Replace {
                        event,
                        spec: r.spec.clone(),
                    }),
                )?;
            }
            None => inserts.entry(r.lane).or_default().push(r.spec.clone()),
        }
    }
    for (li, events) in inserts {
        let Some(lane) = h.doc().sequence(s)?.lanes.get(li).map(|l| l.id) else {
            continue;
        };
        run(
            h,
            Command::Event(EventCmd::Insert {
                seq: s,
                lane,
                events,
            }),
        )?;
    }
    Ok(())
}
