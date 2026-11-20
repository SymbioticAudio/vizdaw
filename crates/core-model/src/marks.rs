//! Co-Producer's marks (plan 8b Ruling 1; spec §7.2): tracks, strips,
//! sends and plug-in slots Co-Producer made carry `co_producer` until the
//! composer takes them over. So do fader groups and VCAs (owner,
//! 2026-11-19), though their names stay as given.
//!
//! [`settle`] runs after every step of [`crate::history::History`]: an
//! acting step marks what it made and names its marked tracks and strips
//! "… (Co-Producer)"; a hand step never makes a marked thing, and a hand
//! rename of a marked track, strip, group or VCA takes it over. Make Mine
//! ([`crate::command::LaneCmd::MakeMine`],
//! [`crate::command::MixerCmd::MakeMine`]) takes things over by hand.

use crate::document::{DestKind, Document};
use crate::error::Result;
use crate::ids::{DestId, GroupId, LaneId, PluginInstanceId, SeqId, StripId, VcaId};
use crate::lane::Lane;
use crate::mixer::{PluginSlot, Strip, StripKind};
use std::collections::{BTreeMap, BTreeSet};

/// What a marked track's or strip's name ends with.
pub const SUFFIX: &str = " (Co-Producer)";

/// The strip `lane` plays through, as the app's StripFocus finds it: its audio path's
/// strip; else, for a plug-in destination, its Instrument strip; none for an external
/// destination; else the built-in synth's strip.
pub fn lane_strip(doc: &Document, lane: LaneId) -> Option<StripId> {
    let l = doc.sequences().values().find_map(|s| s.lane(lane))?;
    let m = doc.mixer();
    if let Some(p) = l.audio_out {
        return m.find_kind(StripKind::AudioPath(p));
    }
    let dest = l.midi_dest?;
    let d = doc.instruments().get(&dest)?;
    match &d.kind {
        DestKind::PluginInstrument { .. } => m.find_kind(StripKind::Instrument(dest)),
        k if k.is_external() => None,
        _ => m.find_kind(StripKind::BuiltinSynth),
    }
}

/// What a lane or strip becomes: its mark, and its name when that changes.
fn settled(
    old: Option<(&str, bool)>,
    name: &str,
    marked: bool,
    acting: bool,
) -> (bool, Option<String>) {
    let marked = match old {
        // New: marked exactly when Co-Producer made it.
        None => acting,
        // A hand rename of a marked thing takes it over.
        Some((old_name, true)) if !acting && old_name != name => false,
        Some(_) => marked,
    };
    let name = (acting && marked && !name.ends_with(SUFFIX)).then(|| format!("{name}{SUFFIX}"));
    (marked, name)
}

/// A lane by ID in any sequence.
fn find_lane(doc: &Document, lane: LaneId) -> Option<&Lane> {
    doc.sequences().values().find_map(|s| s.lane(lane))
}

fn settle_lanes(before: &Document, after: &mut Document, acting: bool) {
    let seqs: Vec<SeqId> = after
        .sequences()
        .changed_keys(before.sequences())
        .copied()
        .collect();
    let mut changes = Vec::new();
    for seq in seqs {
        let Ok(s) = after.sequence(seq) else { continue };
        // The sequence's lanes before, by ID: one pass, so a step on a
        // song with many tracks stays linear.
        let old: BTreeMap<LaneId, &Lane> = before
            .sequence(seq)
            .map(|b| b.lanes.iter().map(|l| (l.id, l)).collect())
            .unwrap_or_default();
        for l in &s.lanes {
            // A lane moved in from another sequence is not new.
            let o = old
                .get(&l.id)
                .copied()
                .or_else(|| find_lane(before, l.id))
                .map(|o| (o.name.as_str(), o.co_producer));
            let (marked, name) = settled(o, &l.name, l.co_producer, acting);
            if marked != l.co_producer || name.is_some() {
                changes.push((seq, l.id, marked, name));
            }
        }
    }
    for (seq, lane, marked, name) in changes {
        if let Ok(l) = after.lane_mut(seq, lane) {
            l.co_producer = marked;
            if let Some(n) = name {
                l.name = n;
            }
        }
    }
}

/// Every plug-in instance ID in `doc`.
fn slot_ids(doc: &Document) -> BTreeSet<PluginInstanceId> {
    let insts = doc.instruments().values().filter_map(|d| match &d.kind {
        DestKind::PluginInstrument { slot } => Some(slot.id),
        DestKind::MidiEndpoint { .. } => None,
    });
    insts
        .chain(doc.mixer().slots().map(|(_, p)| p.id))
        .collect()
}

/// Marks a slot new since `before` as `acting` says; whether it changed.
fn settle_slot(slot: &mut PluginSlot, old: &BTreeSet<PluginInstanceId>, acting: bool) -> bool {
    if old.contains(&slot.id) || slot.co_producer == acting {
        return false;
    }
    slot.co_producer = acting;
    true
}

/// `s` settled against `old` (its version in `before`, if any); whether
/// anything changed.
fn settle_strip(
    s: &mut Strip,
    old: Option<&Strip>,
    old_slots: &BTreeSet<PluginInstanceId>,
    acting: bool,
) -> bool {
    let mut changed = false;
    let (marked, name) = settled(
        old.map(|o| (o.name.as_str(), o.co_producer)),
        &s.name,
        s.co_producer,
        acting,
    );
    if marked != s.co_producer {
        s.co_producer = marked;
        changed = true;
    }
    if let Some(n) = name {
        s.name = n;
        changed = true;
    }
    for send in s.sends.iter_mut() {
        let new = old.is_none_or(|o| !o.sends.iter().any(|x| x.target == send.target));
        if new && send.co_producer != acting {
            send.co_producer = acting;
            changed = true;
        }
    }
    for p in s.chain_mut() {
        changed |= settle_slot(p, old_slots, acting);
    }
    changed
}

fn settle_mixer(
    before: &Document,
    after: &mut Document,
    acting: bool,
    old_slots: &BTreeSet<PluginInstanceId>,
) {
    let ids: Vec<StripId> = after
        .mixer()
        .strips()
        .changed_keys(before.mixer().strips())
        .copied()
        .collect();
    for id in ids {
        let Ok(s) = after.mixer().strip(id) else {
            continue;
        };
        let old = before.mixer().strip(id).ok();
        let mut next = s.clone();
        if settle_strip(&mut next, old, old_slots, acting)
            && let Ok(s) = after.mixer_mut().strip_mut(id)
        {
            *s = next;
        }
    }
}

fn settle_instruments(
    before: &Document,
    after: &mut Document,
    acting: bool,
    old_slots: &BTreeSet<PluginInstanceId>,
) {
    let ids: Vec<DestId> = after
        .instruments()
        .changed_keys(before.instruments())
        .copied()
        .collect();
    for id in ids {
        let fresh = match after.instruments().get(&id).map(|d| &d.kind) {
            Some(DestKind::PluginInstrument { slot }) => {
                !old_slots.contains(&slot.id) && slot.co_producer != acting
            }
            _ => false,
        };
        if fresh
            && let Some(DestKind::PluginInstrument { slot }) =
                after.instruments_mut().get_mut(&id).map(|d| &mut d.kind)
        {
            slot.co_producer = acting;
        }
    }
}

/// Settles each group's and VCA's mark against `before` as [`settled`]
/// does a track's: a new one is marked exactly when Co-Producer made it,
/// and a hand rename of a marked one takes it over. A group or VCA keeps
/// its name: no " (Co-Producer)" is added.
fn settle_groups(before: &Document, after: &mut Document, acting: bool) {
    let (b, a) = (before.mixer(), after.mixer());
    // Only the mark: the name `settled` would give is not used.
    let mark =
        |old: Option<(&str, bool)>, name: &str, marked: bool| settled(old, name, marked, acting).0;
    let groups: Vec<(GroupId, bool)> = a
        .groups()
        .iter()
        .filter_map(|(id, g)| {
            let old = b.groups().get(id).map(|o| (o.name.as_str(), o.co_producer));
            let m = mark(old, &g.name, g.co_producer);
            (m != g.co_producer).then_some((*id, m))
        })
        .collect();
    let vcas: Vec<(VcaId, bool)> = a
        .vcas()
        .iter()
        .filter_map(|(id, v)| {
            let old = b.vcas().get(id).map(|o| (o.name.as_str(), o.co_producer));
            let m = mark(old, &v.name, v.co_producer);
            (m != v.co_producer).then_some((*id, m))
        })
        .collect();
    for (id, m) in groups {
        if let Ok(g) = after.mixer_mut().group_mut(id) {
            g.co_producer = m;
        }
    }
    for (id, m) in vcas {
        if let Ok(v) = after.mixer_mut().vca_mut(id) {
            v.co_producer = m;
        }
    }
}

/// Whether [`settle`] has work for a step from `before`: always while
/// Co-Producer acts, else only when `before` may hold marks. A hand step
/// on a song with no marks makes none (whatever it adds is unmarked, and
/// nothing it copies is marked), so it skips the settling.
pub fn settles(before: &Document, acting: bool) -> bool {
    acting || before.may_have_marks()
}

/// Ruling 1's settling of `after` against `before`.
pub fn settle(before: &Document, after: &mut Document, acting: bool) {
    if !settles(before, acting) {
        return;
    }
    if acting {
        after.set_may_have_marks();
    }
    settle_lanes(before, after, acting);
    settle_groups(before, after, acting);
    let mixer = !after.mixer().strips().ptr_eq(before.mixer().strips());
    let insts = !after.instruments().ptr_eq(before.instruments());
    if !(mixer || insts) {
        return;
    }
    let old_slots = slot_ids(before);
    if mixer {
        settle_mixer(before, after, acting, &old_slots);
    }
    if insts {
        settle_instruments(before, after, acting, &old_slots);
    }
}

/// `name` without a trailing [`SUFFIX`], unless nothing would be left.
fn drop_suffix(name: &mut String) {
    if let Some(base) = name.strip_suffix(SUFFIX)
        && !base.trim().is_empty()
    {
        *name = base.to_string();
    }
}

/// Make Mine of a strip: it, its sends and its plug-in slots (and, for an
/// Instrument strip, its instrument's own slot) are the composer's.
pub(crate) fn make_strip_mine(d: &mut Document, id: StripId) -> Result<()> {
    let s = d.mixer().strip(id)?;
    let dest = match s.kind {
        StripKind::Instrument(dest) => Some(dest),
        _ => None,
    };
    let marked =
        s.co_producer || s.sends.iter().any(|x| x.co_producer) || s.chain().any(|p| p.co_producer);
    if marked {
        let s = d.mixer_mut().strip_mut(id)?;
        if s.co_producer {
            s.co_producer = false;
            drop_suffix(&mut s.name);
        }
        for x in s.sends.iter_mut() {
            x.co_producer = false;
        }
        for p in s.chain_mut() {
            p.co_producer = false;
        }
    }
    let inst_marked = dest.is_some_and(|dest| {
        matches!(
            d.instruments().get(&dest).map(|x| &x.kind),
            Some(DestKind::PluginInstrument { slot }) if slot.co_producer
        )
    });
    if inst_marked
        && let Some(dest) = dest
        && let Some(DestKind::PluginInstrument { slot }) =
            d.instruments_mut().get_mut(&dest).map(|x| &mut x.kind)
    {
        slot.co_producer = false;
    }
    Ok(())
}

/// Make Mine of a track: it and the strip it plays through
/// ([`lane_strip`]), with [`make_strip_mine`].
pub(crate) fn make_lane_mine(d: &mut Document, seq: SeqId, lane: LaneId) -> Result<()> {
    if d.lane(seq, lane)?.co_producer {
        let l = d.lane_mut(seq, lane)?;
        l.co_producer = false;
        drop_suffix(&mut l.name);
    }
    match lane_strip(d, lane) {
        Some(s) => make_strip_mine(d, s),
        None => Ok(()),
    }
}
