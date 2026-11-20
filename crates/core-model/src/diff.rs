//! What a change does, in plain items (plan 8a Ruling 6): the list
//! Co-Producer's preview shows the composer before they say yes.
//!
//! Every name shown passes through one helper, [`shown_name`], so a name
//! Claude gave a track, sequence or bus can never add or hide lines in
//! that list.

use crate::automation::{AutomationMode, AutomationTarget, strip_of};
use crate::document::{DestKind, Document};
use crate::event::{Event, EventBody, FadeShape};
use crate::groups::{heard_db, vca_muted, vca_offset_db};
use crate::ids::{DestId, EventId, LaneId, OutputPathId, PluginInstanceId, SeqId, StripId, VcaId};
use crate::lane::Lane;
use crate::mixer::{AuxSend, GAIN_FLOOR_DB, Output, ParamId, PluginSlot, Strip, StripKind, Swatch};
use crate::sequence::{Marker, Sequence};
use crate::time::Tick;
use std::collections::{BTreeMap, BTreeSet};

/// One thing a change does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeItem {
    /// What it acts on ("Track 3, Lead Vox", "Strip Drums", "The song").
    pub thing: String,
    /// What happens to it ("deleted, with 4 notes", "renamed to \"Vox\"").
    pub change: String,
    /// The track or strip it names was Co-Producer's (marked before the
    /// change), and the change takes away none of its regions or notes
    /// (plan 8b Ruling 2): Co-Producer may do it without a yes.
    pub co_producers: bool,
}

/// Names longer than this are cut to one less plus "…".
const MAX_SHOWN: usize = 60;

/// Whether `c` could add, hide or reorder lines of the list: a C0 or C1
/// control, a bidi control or mark, or a line or paragraph separator.
fn unsafe_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}'
                | '\u{200E}'
                | '\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{2028}'
                | '\u{2029}'
        )
}

/// A name as the list shows it: controls, bidi controls and marks, and
/// line and paragraph separators become a space, and a name over 60
/// characters is cut to 59 plus "…".
pub fn shown_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if unsafe_char(c) { ' ' } else { c })
        .collect();
    if clean.chars().count() > MAX_SHOWN {
        let mut cut: String = clean.chars().take(MAX_SHOWN - 1).collect();
        cut.push('…');
        cut
    } else {
        clean
    }
}

fn item(thing: String, change: String) -> ChangeItem {
    ChangeItem {
        thing,
        change,
        co_producers: false,
    }
}

/// An item about a thing that was Co-Producer's when `ours`.
fn item_of(thing: String, change: String, ours: bool) -> ChangeItem {
    ChangeItem {
        co_producers: ours,
        ..item(thing, change)
    }
}

fn renamed(to: &str) -> String {
    format!("renamed to \"{}\"", shown_name(to))
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A lane's audio regions and notes, over all its takes (deleting the
/// track removes every take).
fn held(l: &Lane) -> (usize, usize) {
    l.takes
        .iter()
        .flat_map(|t| t.events.values())
        .fold((0, 0), |(r, n), e| match e.body {
            EventBody::AudioRegion { .. } => (r + 1, n),
            EventBody::Note { .. } => (r, n + 1),
            _ => (r, n),
        })
}

fn track(i: usize, name: &str) -> String {
    format!("Track {}, {}", i + 1, shown_name(name))
}

fn lanes_of(s: &Sequence) -> BTreeMap<LaneId, (usize, &Lane)> {
    s.lanes
        .iter()
        .enumerate()
        .map(|(i, l)| (l.id, (i, l)))
        .collect()
}

/// A fade shape as the Region menu titles it, in the list's lower case.
fn shape(s: FadeShape) -> &'static str {
    match s {
        FadeShape::Linear => "linear",
        FadeShape::EqualPower => "equal power",
        FadeShape::Exponential => "exponential",
        FadeShape::SCurve => "s-curve",
    }
}

/// A lane's audio regions' fades over all its takes, by ID.
fn fades(l: &Lane) -> BTreeMap<EventId, (FadeShape, FadeShape)> {
    l.takes
        .iter()
        .flat_map(|t| t.events.values())
        .filter_map(|e| match &e.body {
            EventBody::AudioRegion {
                fade_in, fade_out, ..
            } => Some((e.id, (fade_in.shape, fade_out.shape))),
            _ => None,
        })
        .collect()
}

/// "fade in linear → s-curve" for each region kept whose fade shapes
/// changed (plan 8e Task 6b).
fn changed_fades(before: &Lane, after: &Lane) -> Vec<String> {
    let old = fades(before);
    let mut parts = Vec::new();
    for (id, (fin, fout)) in fades(after) {
        let Some((oin, oout)) = old.get(&id) else {
            continue;
        };
        for (edge, was, now) in [("in", *oin, fin), ("out", *oout, fout)] {
            if was != now {
                parts.push(format!("fade {edge} {} → {}", shape(was), shape(now)));
            }
        }
    }
    parts
}

/// An output path's name as the list shows it; "none" for no output.
fn path_name(doc: &Document, o: Option<OutputPathId>) -> String {
    match o {
        None => "none".into(),
        Some(id) => doc
            .outputs()
            .get(&id)
            .map_or_else(|| format!("output {}", id.0), |p| shown_name(&p.name)),
    }
}

/// The track items of one sequence present before and after.
fn tracks(
    docs: (&Document, &Document),
    before: &Sequence,
    after: &Sequence,
    out: &mut Vec<ChangeItem>,
) {
    let (b, a) = (lanes_of(before), lanes_of(after));
    for (i, l) in before.lanes.iter().enumerate() {
        if a.contains_key(&l.id) {
            continue;
        }
        let (r, n) = held(l);
        let parts: Vec<String> = [
            (r > 0).then(|| count(r, "region", "regions")),
            (n > 0).then(|| count(n, "note", "notes")),
        ]
        .into_iter()
        .flatten()
        .collect();
        let change = if parts.is_empty() {
            "deleted".to_string()
        } else {
            format!("deleted, with {}", parts.join(" and "))
        };
        out.push(item_of(
            track(i, &l.name),
            change,
            l.co_producer && parts.is_empty(),
        ));
    }
    for (i, l) in after.lanes.iter().enumerate() {
        let Some((_, old)) = b.get(&l.id) else {
            out.push(item(track(i, &l.name), "added".into()));
            continue;
        };
        if old.name != l.name {
            out.push(item_of(
                track(i, &old.name),
                renamed(&l.name),
                old.co_producer,
            ));
        }
        let ((r0, n0), (r1, n1)) = (held(old), held(l));
        let mut parts = Vec::new();
        if r0 != r1 {
            parts.push(format!(
                "{} → {}",
                count(r0, "region", "regions"),
                count(r1, "region", "regions")
            ));
        }
        if n0 != n1 {
            parts.push(format!(
                "{} → {}",
                count(n0, "note", "notes"),
                count(n1, "note", "notes")
            ));
        }
        let changed = changed_notes(old, l);
        let kept = changed.is_empty();
        if !kept {
            parts.push(changed);
        }
        // Neither a fade shape nor an output takes work away.
        parts.extend(changed_fades(old, l));
        if old.audio_out != l.audio_out {
            parts.push(format!(
                "output {} → {}",
                path_name(docs.0, old.audio_out),
                path_name(docs.1, l.audio_out)
            ));
        }
        if !parts.is_empty() {
            // A falling count, or a changed note, takes the composer's work
            // away.
            let ours = old.co_producer && r1 >= r0 && n1 >= n0 && kept;
            out.push(item_of(track(i, &old.name), parts.join(", "), ours));
        }
    }
}

/// A lane's notes over all its takes, by ID.
fn notes(l: &Lane) -> BTreeMap<EventId, &Event> {
    l.takes
        .iter()
        .flat_map(|t| t.events.values())
        .filter(|e| matches!(e.body, EventBody::Note { .. }))
        .map(|e| (e.id, e))
        .collect()
}

/// Ruling 5: "N notes changed (start, pitch, velocity, length)" for the
/// notes of both versions whose values differ, naming only the values
/// that do; empty when none differ.
fn changed_notes(before: &Lane, after: &Lane) -> String {
    let after_notes = notes(after);
    let mut n = 0;
    let mut what = [false; 4];
    for (id, b) in notes(before) {
        let Some(a) = after_notes.get(&id) else {
            continue;
        };
        let (
            EventBody::Note {
                pitch: p0,
                velocity: v0,
                duration: d0,
                ..
            },
            EventBody::Note {
                pitch: p1,
                velocity: v1,
                duration: d1,
                ..
            },
        ) = (&b.body, &a.body)
        else {
            continue;
        };
        let diffs = [b.tick != a.tick, p0 != p1, v0 != v1, d0 != d1];
        if diffs.iter().any(|d| *d) {
            n += 1;
            for (w, d) in what.iter_mut().zip(diffs) {
                *w |= d;
            }
        }
    }
    if n == 0 {
        return String::new();
    }
    let names: Vec<&str> = ["start", "pitch", "velocity", "length"]
        .into_iter()
        .zip(what)
        .filter_map(|(name, d)| d.then_some(name))
        .collect();
    format!(
        "{} changed ({})",
        count(n, "note", "notes"),
        names.join(", ")
    )
}

fn marker(i: usize, m: &Marker) -> String {
    format!("Marker {}, {}", i + 1, shown_name(&m.name))
}

/// Ruling 5's marker items of one sequence: markers equal in tick and
/// name match first; of the rest, a pair at the same index and tick is a
/// rename, and at the same index and name a move; the others are
/// deletions (before's position) and additions (after's).
fn markers(before: &Sequence, after: &Sequence, out: &mut Vec<ChangeItem>) {
    let (b, a) = (&before.markers, &after.markers);
    let mut b_left = vec![true; b.len()];
    let mut a_left = vec![true; a.len()];
    for (i, m) in b.iter().enumerate() {
        let found = a
            .iter()
            .enumerate()
            .find(|(j, n)| a_left.get(*j).copied().unwrap_or(false) && *n == m);
        if let Some((j, _)) = found {
            if let (Some(x), Some(y)) = (b_left.get_mut(i), a_left.get_mut(j)) {
                *x = false;
                *y = false;
            }
        }
    }
    for (i, m) in b.iter().enumerate() {
        if !b_left.get(i).copied().unwrap_or(false) {
            continue;
        }
        let other = a.get(i).filter(|_| a_left.get(i).copied().unwrap_or(false));
        let change = match other {
            Some(n) if n.tick == m.tick => renamed(&n.name),
            Some(n) if n.name == m.name => "moved".to_string(),
            _ => {
                out.push(item(marker(i, m), "deleted".into()));
                continue;
            }
        };
        if let Some(y) = a_left.get_mut(i) {
            *y = false;
        }
        out.push(item(marker(i, m), change));
    }
    for (j, n) in a.iter().enumerate() {
        if a_left.get(j).copied().unwrap_or(false) {
            out.push(item(marker(j, n), "added".into()));
        }
    }
}

fn sequence(i: usize, name: &str) -> String {
    format!("Sequence {}, {}", i + 1, shown_name(name))
}

fn sequences(before: &Document, after: &Document, out: &mut Vec<ChangeItem>) {
    let pos = |d: &Document| -> BTreeMap<SeqId, usize> {
        d.sequences()
            .keys()
            .enumerate()
            .map(|(i, k)| (*k, i))
            .collect()
    };
    let (bp, ap) = (pos(before), pos(after));
    let ids: BTreeSet<SeqId> = bp.keys().chain(ap.keys()).copied().collect();
    for id in ids {
        match (before.sequence(id), after.sequence(id)) {
            (Ok(b), Err(_)) => {
                let i = bp.get(&id).copied().unwrap_or_default();
                out.push(item(sequence(i, &b.name), "deleted".into()));
            }
            (Err(_), Ok(a)) => {
                let i = ap.get(&id).copied().unwrap_or_default();
                out.push(item(sequence(i, &a.name), "added".into()));
            }
            (Ok(b), Ok(a)) => {
                if b.name != a.name {
                    let i = ap.get(&id).copied().unwrap_or_default();
                    out.push(item(sequence(i, &b.name), renamed(&a.name)));
                }
                if b != a {
                    tracks((before, after), b, a, out);
                    markers(b, a, out);
                }
            }
            (Err(_), Err(_)) => {}
        }
    }
}

/// Strips in display order: master first, then the display order, then
/// any other (none in a valid document).
fn strip_order(d: &Document) -> Vec<StripId> {
    let m = d.mixer();
    let mut ids = vec![m.master()];
    ids.extend(m.order().iter().copied());
    for id in m.strips().keys() {
        if !ids.contains(id) {
            ids.push(*id);
        }
    }
    ids
}

fn strip(name: &str) -> String {
    format!("Strip {}", shown_name(name))
}

/// A strip colour as the list says it (plan 8b Ruling 3).
fn colour(c: Option<Swatch>) -> &'static str {
    match c {
        None => "none",
        Some(Swatch::Red) => "red",
        Some(Swatch::Orange) => "orange",
        Some(Swatch::Yellow) => "yellow",
        Some(Swatch::Green) => "green",
        Some(Swatch::Teal) => "teal",
        Some(Swatch::Blue) => "blue",
        Some(Swatch::Purple) => "purple",
        Some(Swatch::Pink) => "pink",
    }
}

fn strips(
    before: &Document,
    after: &Document,
    automation: &mut BTreeMap<Owner, (Vec<String>, bool)>,
    params: ParamLookup,
    out: &mut Vec<ChangeItem>,
) {
    let (b, a) = (before.mixer().strips(), after.mixer().strips());
    for id in strip_order(before) {
        if let (Some(s), None) = (b.get(&id), a.get(&id)) {
            out.push(item_of(strip(&s.name), "deleted".into(), s.co_producer));
        }
    }
    for id in strip_order(after) {
        let Some(s) = a.get(&id) else { continue };
        let Some(old) = b.get(&id) else {
            out.push(item(strip(&s.name), "added".into()));
            continue;
        };
        if old.name != s.name {
            out.push(item_of(strip(&old.name), renamed(&s.name), old.co_producer));
        }
        if old.colour != s.colour {
            out.push(item_of(
                strip(&old.name),
                format!("colour {} → {}", colour(old.colour), colour(s.colour)),
                old.co_producer,
            ));
        }
        let mut parts = mixer_parts(before, after, old, s, params);
        parts.extend(vca_parts(before, after, old, s));
        let (auto, theirs) = automation.remove(&Owner::Strip(id)).unwrap_or_default();
        parts.extend(auto);
        let parts = capped(parts);
        if !parts.is_empty() {
            let ours = old.co_producer && !theirs;
            out.push(item_of(strip(&old.name), parts.join(", "), ours));
        }
    }
}

/// What the VCAs do to a strip present before (`old`) and after (`new`)
/// (groups-vcas-transport Task 7): `heard <a> → <b>` when the level it is
/// heard at changed while its own fader did not, and `muted by a VCA` or
/// `no longer muted by a VCA`.
fn vca_parts(before: &Document, after: &Document, old: &Strip, new: &Strip) -> Vec<String> {
    let mut parts = Vec::new();
    let heard = |d: &Document, s: &Strip| heard_db(s.gain_db, vca_offset_db(d.mixer(), s.id));
    let (was, now) = (heard(before, old), heard(after, new));
    if old.gain_db == new.gain_db && was != now {
        parts.push(format!("heard {} → {}", db(was), db(now)));
    }
    let (m0, m1) = (
        vca_muted(before.mixer(), old.id),
        vca_muted(after.mixer(), new.id),
    );
    if m0 != m1 {
        parts.push(
            if m1 {
                "muted by a VCA"
            } else {
                "no longer muted by a VCA"
            }
            .into(),
        );
    }
    parts
}

/// Whether every one of `members` is a strip of `doc` marked Co-Producer's.
fn all_ours(doc: &Document, members: &im::Vector<StripId>) -> bool {
    members
        .iter()
        .all(|id| doc.mixer().strip(*id).is_ok_and(|s| s.co_producer))
}

/// `<strip> <added>` for each of `new` not in `old`, then `<strip>
/// <removed>` for each of `old` not in `new`, each in its list's order;
/// strips named as the strip items name them, by the name before.
fn member_parts(
    before: &Document,
    after: &Document,
    (old, new): (&im::Vector<StripId>, &im::Vector<StripId>),
    (added, removed): (&str, &str),
    parts: &mut Vec<String>,
) {
    for id in new.iter().filter(|id| !old.contains(id)) {
        parts.push(format!("{} {added}", either_name(before, after, *id)));
    }
    for id in old.iter().filter(|id| !new.contains(id)) {
        parts.push(format!("{} {removed}", either_name(before, after, *id)));
    }
}

/// "renamed <old> → <new>" and "colour <old> → <new>" of a group or VCA.
fn name_parts((n0, n1): (&str, &str), (c0, c1): (Swatch, Swatch), parts: &mut Vec<String>) {
    if n0 != n1 {
        parts.push(format!("renamed {} → {}", shown_name(n0), shown_name(n1)));
    }
    if c0 != c1 {
        parts.push(format!(
            "colour {} → {}",
            colour(Some(c0)),
            colour(Some(c1))
        ));
    }
}

/// An automation mode as the strip's menu titles it.
fn mode(m: AutomationMode) -> &'static str {
    match m {
        AutomationMode::Off => "Off",
        AutomationMode::Read => "Read",
        AutomationMode::Touch => "Touch",
        AutomationMode::Latch => "Latch",
        AutomationMode::Write => "Write",
        AutomationMode::Trim => "Trim",
    }
}

/// The "Group <name>" items, by group ID (groups-vcas-transport Task 7):
/// `added`, `deleted`, `renamed`, `colour`, `<strip> added`, `<strip>
/// removed`, `mute link on`/`off` and `solo link on`/`off`. A group is
/// Co-Producer's only when, before and after, it carries Co-Producer's
/// mark and every member is a strip marked Co-Producer's (Ruling 10;
/// owner, 2026-11-19): the composer's group asks, even an empty one.
fn groups(before: &Document, after: &Document, out: &mut Vec<ChangeItem>) {
    let (b, a) = (before.mixer().groups(), after.mixer().groups());
    let ids: BTreeSet<_> = b.keys().chain(a.keys()).copied().collect();
    let none = im::Vector::new();
    for id in ids {
        let (old, new) = (b.get(&id), a.get(&id));
        let (Some(named), true) = (old.or(new), old != new) else {
            continue;
        };
        let mut parts = Vec::new();
        match (old, new) {
            (None, Some(g)) => {
                parts.push("added".to_string());
                member_parts(
                    before,
                    after,
                    (&none, &g.members),
                    ("added", "removed"),
                    &mut parts,
                );
            }
            (Some(_), None) => parts.push("deleted".to_string()),
            (Some(o), Some(g)) => {
                name_parts((&o.name, &g.name), (o.colour, g.colour), &mut parts);
                member_parts(
                    before,
                    after,
                    (&o.members, &g.members),
                    ("added", "removed"),
                    &mut parts,
                );
                for (link, was, now) in [
                    ("mute", o.link_mute, g.link_mute),
                    ("solo", o.link_solo, g.link_solo),
                ] {
                    if was != now {
                        parts.push(format!("{link} link {}", if now { "on" } else { "off" }));
                    }
                }
            }
            (None, None) => {}
        }
        let parts = capped(parts);
        if parts.is_empty() {
            continue;
        }
        let ours = old.is_none_or(|g| g.co_producer && all_ours(before, &g.members))
            && new.is_none_or(|g| g.co_producer && all_ours(after, &g.members));
        out.push(item_of(
            format!("Group {}", shown_name(&named.name)),
            parts.join(", "),
            ours,
        ));
    }
}

/// The points of `doc` that play on VCA `vca`'s fader: their count, the
/// first and last tick, and whether any is on a track not Co-Producer's.
fn vca_points(doc: &Document, vca: VcaId) -> Option<(usize, SeqId, Tick, Tick, bool)> {
    let points = automation_points(doc);
    let mut found: Option<(usize, SeqId, Tick, Tick, bool)> = None;
    for ((target, seq), ps) in &points {
        if *target != AutomationTarget::VcaFader(vca) {
            continue;
        }
        for (t, _, ours) in ps.values() {
            let f = found.get_or_insert((0, *seq, *t, *t, false));
            f.0 += 1;
            f.2 = f.2.min(*t);
            f.3 = f.3.max(*t);
            f.4 |= !ours;
        }
    }
    found
}

/// The "VCA <name>" items, by VCA ID (groups-vcas-transport Task 7):
/// `added`, `deleted`, `renamed`, `colour`, `level <a> → <b>`,
/// `muted`/`unmuted`, `soloed`/`unsoloed`, `mode <a> → <b>`, `<strip>
/// assigned`, `<strip> unassigned`, then its fader automation's parts. A
/// deleted VCA's automation stops playing, so its item says so. A VCA is
/// Co-Producer's as a group is ([`groups`]), and not when the change takes
/// away or changes points on a track not Co-Producer's.
fn vcas(
    before: &Document,
    after: &Document,
    automation: &mut BTreeMap<Owner, (Vec<String>, bool)>,
    out: &mut Vec<ChangeItem>,
) {
    let (b, a) = (before.mixer().vcas(), after.mixer().vcas());
    let ids: BTreeSet<_> = b.keys().chain(a.keys()).copied().collect();
    let none = im::Vector::new();
    for id in ids {
        let (old, new) = (b.get(&id), a.get(&id));
        let Some(named) = old.or(new) else { continue };
        let (auto, mut theirs) = automation.remove(&Owner::Vca(id)).unwrap_or_default();
        let mut parts = Vec::new();
        match (old, new) {
            (None, Some(v)) => {
                parts.push("added".to_string());
                member_parts(
                    before,
                    after,
                    (&none, &v.members),
                    ("assigned", "unassigned"),
                    &mut parts,
                );
            }
            (Some(_), None) => {
                parts.push("deleted".to_string());
                if let Some((n, seq, lo, hi, any_theirs)) = vca_points(before, id) {
                    parts.push(format!(
                        "fader automation no longer plays ({}, bars {}–{})",
                        count(n, "point", "points"),
                        bar_of(before, seq, lo),
                        bar_of(before, seq, hi)
                    ));
                    theirs |= any_theirs;
                }
            }
            (Some(o), Some(v)) => {
                name_parts((&o.name, &v.name), (o.colour, v.colour), &mut parts);
                if o.gain_db != v.gain_db {
                    parts.push(format!("level {} → {}", db(o.gain_db), db(v.gain_db)));
                }
                if o.mute != v.mute {
                    parts.push(if v.mute { "muted" } else { "unmuted" }.into());
                }
                if o.solo != v.solo {
                    parts.push(if v.solo { "soloed" } else { "unsoloed" }.into());
                }
                if o.automation != v.automation {
                    parts.push(format!(
                        "mode {} → {}",
                        mode(o.automation),
                        mode(v.automation)
                    ));
                }
                member_parts(
                    before,
                    after,
                    (&o.members, &v.members),
                    ("assigned", "unassigned"),
                    &mut parts,
                );
            }
            (None, None) => {}
        }
        parts.extend(auto);
        let parts = capped(parts);
        if parts.is_empty() {
            continue;
        }
        let ours = !theirs
            && old.is_none_or(|v| v.co_producer && all_ours(before, &v.members))
            && new.is_none_or(|v| v.co_producer && all_ours(after, &v.members));
        out.push(item_of(
            crate::groups::vca_label(&shown_name(&named.name)),
            parts.join(", "),
            ours,
        ));
    }
}

/// A Built-in Synth's sound by its title (plan release-fixes Scope 1:
/// a General MIDI program and channel, with no bank), as its window names
/// it; "another sound" for any other patch.
fn synth_sound(d: &crate::document::Destination) -> &'static str {
    let DestKind::MidiEndpoint { channel, .. } = &d.kind else {
        return "another sound";
    };
    let program = match d.default_patch {
        Some(p) if p.bank.is_some() => return "another sound",
        p => p.map_or(0, |p| p.program),
    };
    match (program, *channel) {
        (0, 0) => "Piano",
        (33, 0) => "Bass",
        (89, 0) => "Pad",
        (80, 0) => "Lead",
        (0, 9) => "Drums",
        _ => "another sound",
    }
}

/// "Built-in Synth" / "sound Piano → Pad" for each Built-in Synth whose
/// sound changed (plan 8e Task 6b). Every track that plays it changes, so
/// it is never Co-Producer's alone.
fn synth_sounds(before: &Document, after: &Document, out: &mut Vec<ChangeItem>) {
    for (id, old) in before.instruments().iter() {
        let Some(new) = after.instruments().get(id) else {
            continue;
        };
        if !(old.kind.is_builtin_synth() && new.kind.is_builtin_synth()) {
            continue;
        }
        let (was, now) = (synth_sound(old), synth_sound(new));
        if was != now {
            out.push(item(
                "Built-in Synth".into(),
                format!("sound {was} → {now}"),
            ));
        }
    }
}

/// Name and current value of a running plug-in's parameter, if known.
pub type ParamLookup<'a> = &'a dyn Fn(PluginInstanceId, ParamId) -> Option<(String, f64)>;

/// The most parts one mixer item lists before "and N more changes"
/// (plan 8d Ruling 4).
pub const MAX_PARTS: usize = 12;

/// The strip an instrument's own slot belongs to: its destination's
/// `Instrument` strip with output 0.
pub fn instrument_strip(doc: &Document, slot: PluginInstanceId) -> Option<StripId> {
    let dest = doc
        .instruments()
        .values()
        .find(|d| matches!(&d.kind, DestKind::PluginInstrument { slot: s } if s.id == slot))?;
    doc.mixer().find_kind(StripKind::Instrument(dest.id))
}

/// A plug-in instrument destination's own slot.
fn own_slot(doc: &Document, dest: DestId) -> Option<&PluginSlot> {
    match &doc.instruments().get(&dest)?.kind {
        DestKind::PluginInstrument { slot } => Some(slot),
        DestKind::MidiEndpoint { .. } => None,
    }
}

/// `parts`, cut to [`MAX_PARTS`] plus "and N more changes".
fn capped(mut parts: Vec<String>) -> Vec<String> {
    if parts.len() > MAX_PARTS {
        let more = parts.len() - MAX_PARTS;
        parts.truncate(MAX_PARTS);
        parts.push(format!(
            "and {}",
            count(more, "more change", "more changes")
        ));
    }
    parts
}

/// `text` with U+2212 for a minus, unless it shows as zero.
fn signed(negative: bool, text: String) -> String {
    if negative && text.chars().any(|c| c.is_ascii_digit() && c != '0') {
        format!("−{text}")
    } else {
        text
    }
}

/// A level: one decimal, "−∞ dB" at the floor.
fn db(v: f64) -> String {
    if v <= GAIN_FLOOR_DB {
        return "−∞ dB".into();
    }
    format!("{} dB", signed(v < 0.0, format!("{:.1}", v.abs())))
}

/// A pan: "centre", "30% left" or "30% right".
fn pan(p: f64) -> String {
    let n = (p * 100.0).round() as i64;
    match n {
        0 => "centre".into(),
        n if n < 0 => format!("{}% left", n.unsigned_abs()),
        n => format!("{n}% right"),
    }
}

/// A parameter value: at most 3 decimals, trailing zeros dropped.
fn value(v: f64) -> String {
    if !v.is_finite() || v.abs() >= 1e12 {
        return format!("{v:.3e}");
    }
    let s = format!("{:.3}", v.abs());
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    signed(v < 0.0, s)
}

/// A strip's name as `doc` has it.
fn strip_name(doc: &Document, id: StripId) -> String {
    match doc.mixer().strip(id) {
        Ok(s) => shown_name(&s.name),
        Err(_) => format!("strip {id}"),
    }
}

fn output_name(doc: &Document, o: Output) -> String {
    match o {
        Output::Master => strip_name(doc, doc.mixer().master()),
        Output::Bus(id) => strip_name(doc, id),
    }
}

/// A strip named as `before` has it, else as `after` does.
fn either_name(before: &Document, after: &Document, id: StripId) -> String {
    if before.mixer().strip(id).is_ok() {
        strip_name(before, id)
    } else {
        strip_name(after, id)
    }
}

fn sidechain_name(doc: &Document, s: Option<StripId>) -> String {
    s.map_or_else(|| "none".into(), |id| strip_name(doc, id))
}

/// Ruling 4's parts for a strip present before (`old`) and after (`new`).
fn mixer_parts(
    before: &Document,
    after: &Document,
    old: &Strip,
    new: &Strip,
    params: ParamLookup,
) -> Vec<String> {
    let mut parts = Vec::new();
    if old.gain_db != new.gain_db {
        parts.push(format!("volume {} → {}", db(old.gain_db), db(new.gain_db)));
    }
    if old.pan != new.pan {
        parts.push(format!("pan {} → {}", pan(old.pan), pan(new.pan)));
    }
    if old.mute != new.mute {
        parts.push(if new.mute { "muted" } else { "unmuted" }.into());
    }
    if old.solo != new.solo {
        parts.push(if new.solo { "soloed" } else { "unsoloed" }.into());
    }
    if old.output != new.output {
        parts.push(format!(
            "output {} → {}",
            output_name(before, old.output),
            output_name(after, new.output)
        ));
    }
    sends(before, after, old, new, &mut parts);
    eq(before, after, old, new, &mut parts);
    inserts(before, after, old, new, &mut parts);
    // The instrument's own slot (`mixer.set_bypass` reaches it too).
    let own = match (old.kind, new.kind) {
        (StripKind::Instrument(d0), StripKind::Instrument(d1)) => {
            match (own_slot(before, d0), own_slot(after, d1)) {
                (Some(a), Some(b)) if a.id == b.id => Some((a, b)),
                _ => None,
            }
        }
        _ => None,
    };
    if let Some((a, b)) = own {
        let label = format!("instrument {}", shown_name(&a.plugin.name));
        slot_parts(before, after, &label, a, b, &mut parts);
    }
    // Parameters, in signal order: the instrument's own slot, the EQ,
    // then the inserts.
    if let Some((a, b)) = own {
        param_parts(a, b, params, &mut parts);
    }
    let old_slots: BTreeMap<PluginInstanceId, &PluginSlot> =
        old.chain().map(|p| (p.id, p)).collect();
    for b in new.chain() {
        if let Some(a) = old_slots.get(&b.id) {
            param_parts(a, b, params, &mut parts);
        }
    }
    parts
}

/// Sends, matched by target and then by order among sends to it.
fn sends(before: &Document, after: &Document, old: &Strip, new: &Strip, parts: &mut Vec<String>) {
    let group = |s: &Strip| {
        let mut m: BTreeMap<StripId, Vec<AuxSend>> = BTreeMap::new();
        for x in &s.sends {
            m.entry(x.target).or_default().push(*x);
        }
        m
    };
    let (b, a) = (group(old), group(new));
    let mut seen = BTreeSet::new();
    let targets: Vec<StripId> = old
        .sends
        .iter()
        .chain(new.sends.iter())
        .map(|x| x.target)
        .filter(|t| seen.insert(*t))
        .collect();
    let none = Vec::new();
    for t in targets {
        let name = format!("send to {}", either_name(before, after, t));
        let (bs, as_) = (b.get(&t).unwrap_or(&none), a.get(&t).unwrap_or(&none));
        for k in 0..bs.len().max(as_.len()) {
            match (bs.get(k), as_.get(k)) {
                (Some(x), Some(y)) => {
                    let mut what = Vec::new();
                    if x.level_db != y.level_db {
                        what.push(format!("{} → {}", db(x.level_db), db(y.level_db)));
                    }
                    if x.pre_fader != y.pre_fader {
                        what.push(
                            if y.pre_fader {
                                "now pre-fader"
                            } else {
                                "now post-fader"
                            }
                            .into(),
                        );
                    }
                    if x.enabled != y.enabled {
                        what.push(if y.enabled { "on" } else { "off" }.into());
                    }
                    if !what.is_empty() {
                        parts.push(format!("{name} {}", what.join(", ")));
                    }
                }
                (Some(_), None) => parts.push(format!("{name} deleted")),
                (None, Some(y)) => parts.push(format!(
                    "{name} added ({}, {}{})",
                    db(y.level_db),
                    if y.pre_fader {
                        "pre-fader"
                    } else {
                        "post-fader"
                    },
                    if y.enabled { "" } else { ", off" }
                )),
                (None, None) => {}
            }
        }
    }
}

/// Bypass and sidechain parts of a slot present before and after.
fn slot_parts(
    before: &Document,
    after: &Document,
    label: &str,
    a: &PluginSlot,
    b: &PluginSlot,
    parts: &mut Vec<String>,
) {
    if a.bypass != b.bypass {
        let what = if b.bypass {
            "bypassed"
        } else {
            "turned back on"
        };
        parts.push(format!("{label} {what}"));
    }
    if a.sidechain != b.sidechain {
        parts.push(format!(
            "{label} sidechain {} → {}",
            sidechain_name(before, a.sidechain),
            sidechain_name(after, b.sidechain)
        ));
    }
}

fn eq(before: &Document, after: &Document, old: &Strip, new: &Strip, parts: &mut Vec<String>) {
    match (&old.eq, &new.eq) {
        (None, Some(e)) => parts.push(format!("EQ {} added", shown_name(&e.plugin.name))),
        (Some(e), None) => parts.push(format!("EQ {} deleted", shown_name(&e.plugin.name))),
        (Some(a), Some(b)) if a.id != b.id => parts.push(format!(
            "EQ {} → {}",
            shown_name(&a.plugin.name),
            shown_name(&b.plugin.name)
        )),
        (Some(a), Some(b)) => {
            let label = format!("EQ {}", shown_name(&a.plugin.name));
            slot_parts(before, after, &label, a, b, parts);
        }
        (None, None) => {}
    }
}

/// Which of `seq` (distinct numbers) stay put: one longest increasing
/// run, so the fewest slots read as moved.
fn kept(seq: &[usize]) -> Vec<bool> {
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; seq.len()];
    for (i, v) in seq.iter().enumerate() {
        let k = tails.partition_point(|t| seq.get(*t).is_some_and(|x| x < v));
        if let (Some(p), Some(t)) = (prev.get_mut(i), k.checked_sub(1)) {
            *p = tails.get(t).copied();
        }
        match tails.get_mut(k) {
            Some(t) => *t = i,
            None => tails.push(i),
        }
    }
    let mut keep = vec![false; seq.len()];
    let mut cur = tails.last().copied();
    while let Some(i) = cur {
        if let Some(k) = keep.get_mut(i) {
            *k = true;
        }
        cur = prev.get(i).copied().flatten();
    }
    keep
}

/// Inserts, matched by slot ID: added, deleted, moved (among the slots
/// present before and after), then bypass and sidechain.
fn inserts(before: &Document, after: &Document, old: &Strip, new: &Strip, parts: &mut Vec<String>) {
    let pos = |s: &Strip| -> BTreeMap<PluginInstanceId, usize> {
        s.inserts
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id, i))
            .collect()
    };
    let (bp, ap) = (pos(old), pos(new));
    let label = |p: &PluginSlot| format!("insert {}", shown_name(&p.plugin.name));
    for (j, p) in new.inserts.iter().enumerate() {
        if !bp.contains_key(&p.id) {
            parts.push(format!("{} added at {}", label(p), j + 1));
        }
    }
    for p in &old.inserts {
        if !ap.contains_key(&p.id) {
            parts.push(format!("{} deleted", label(p)));
        }
    }
    // (before position, after position, before slot, after slot) of the
    // slots in both, in after's order.
    let both: Vec<(usize, usize, &PluginSlot, &PluginSlot)> = new
        .inserts
        .iter()
        .enumerate()
        .filter_map(|(j, b)| {
            let i = *bp.get(&b.id)?;
            Some((i, j, old.inserts.get(i)?, b))
        })
        .collect();
    let order: Vec<usize> = both.iter().map(|x| x.0).collect();
    for ((i, j, a, _), stays) in both.iter().zip(kept(&order)) {
        if !stays {
            parts.push(format!("{} moved {} → {}", label(a), i + 1, j + 1));
        }
    }
    for (_, _, a, b) in &both {
        slot_parts(before, after, &label(a), a, b, parts);
    }
}

/// One part per overlay value of `b` that differs from `a`'s.
fn param_parts(a: &PluginSlot, b: &PluginSlot, params: ParamLookup, parts: &mut Vec<String>) {
    let slot = shown_name(&b.plugin.name);
    for (id, v) in b.params.iter() {
        let was = a.params.get(id).copied();
        if was == Some(*v) {
            continue;
        }
        let known = params(b.id, *id);
        let name = known
            .as_ref()
            .map_or_else(|| format!("parameter {}", id.0), |(n, _)| shown_name(n));
        match was.or(known.map(|(_, x)| x)) {
            Some(o) => parts.push(format!("{slot} {name} {} → {}", value(o), value(*v))),
            None => parts.push(format!("{slot} {name} set to {}", value(*v))),
        }
    }
}

/// The 1-based bar a tick falls in, from `seq`'s meter: bar 1 for a
/// tick before it, or a sequence `doc` does not have.
pub fn bar_of(doc: &Document, seq: SeqId, tick: Tick) -> u32 {
    let Ok(s) = doc.sequence(seq) else { return 1 };
    u32::try_from(s.meter.tick_to_bbu(tick, 1).bar)
        .unwrap_or(1)
        .max(1)
}

/// What an automation part joins: a strip's item, or a VCA's
/// (groups-vcas-transport Task 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Owner {
    Strip(StripId),
    Vca(VcaId),
}

/// Every automation point of `doc`, by target and sequence, then by ID:
/// its tick and value, and whether its lane is Co-Producer's. Points of
/// every take of every lane count.
type Points = BTreeMap<(AutomationTarget, SeqId), BTreeMap<EventId, Point>>;

/// One point: tick, value, and its lane's Co-Producer mark.
type Point = (Tick, f64, bool);

fn automation_points(doc: &Document) -> Points {
    let mut out: Points = BTreeMap::new();
    for (id, seq) in doc.sequences() {
        for (lane, e) in seq
            .lanes
            .iter()
            .flat_map(|l| l.takes.iter().map(move |t| (l, t)))
            .flat_map(|(l, t)| t.events.values().map(move |e| (l, e)))
        {
            if let EventBody::Automation { target, value } = e.body {
                out.entry((target, *id))
                    .or_default()
                    .insert(e.id, (e.tick, value, lane.co_producer));
            }
        }
    }
    out
}

/// What a target's automation part calls it (plan 8e Ruling 4): `fader`,
/// `pan`, `mute`, `solo`, `send to <bus>` or `<slot name> <param name>`;
/// `None` for a target no strip shows.
fn automation_label(
    before: &Document,
    after: &Document,
    target: AutomationTarget,
    params: ParamLookup,
) -> Option<String> {
    Some(match target {
        AutomationTarget::StripFader(_) => "fader".into(),
        AutomationTarget::StripPan(_) => "pan".into(),
        AutomationTarget::StripMute(_) => "mute".into(),
        AutomationTarget::StripSolo(_) => "solo".into(),
        AutomationTarget::SendLevel(s, i) => {
            let send_to = |d: &Document| {
                d.mixer()
                    .strip(s)
                    .ok()
                    .and_then(|x| x.sends.get(usize::from(i.0)))
                    .map(|x| x.target)
            };
            match send_to(after).or_else(|| send_to(before)) {
                Some(t) => format!("send to {}", either_name(before, after, t)),
                None => format!("send {}", u32::from(i.0) + 1),
            }
        }
        AutomationTarget::PluginParam(p, id) => {
            let slot = after.plugin_slot(p).or_else(|| before.plugin_slot(p))?;
            let name = params(p, id)
                .map_or_else(|| format!("parameter {}", id.0), |(n, _)| shown_name(&n));
            format!("{} {name}", shown_name(&slot.plugin.name))
        }
        // A VCA's fader joins its VCA's item.
        AutomationTarget::VcaFader(_) => "fader".into(),
        AutomationTarget::VelocityScale(_) => return None,
    })
}

/// Ruling 4's automation parts, by the strip (or VCA) whose item they join: for
/// each target and sequence whose points differ, `<label> automation <n>
/// points → <m> points, bars <a>–<b>` (or `<label> automation cleared,
/// bars <a>–<b>` when none are left), over the ticks from the first to
/// the last point that went, came or changed. With each strip's parts,
/// whether they take away or change a point of a track that is not
/// Co-Producer's (8e Task 2 fix round 1): then the item needs a yes.
fn automation_parts(
    before: &Document,
    after: &Document,
    params: ParamLookup,
) -> BTreeMap<Owner, (Vec<String>, bool)> {
    let (b, a) = (automation_points(before), automation_points(after));
    let keys: BTreeSet<(AutomationTarget, SeqId)> = b.keys().chain(a.keys()).copied().collect();
    let none = BTreeMap::new();
    let mut out: BTreeMap<Owner, (Vec<String>, bool)> = BTreeMap::new();
    for key in keys {
        let (target, seq) = key;
        let (old, new) = (b.get(&key).unwrap_or(&none), a.get(&key).unwrap_or(&none));
        // The points of `x` that `y` does not have at the same tick
        // and value.
        let differ = |x: &BTreeMap<EventId, Point>, y: &BTreeMap<EventId, Point>| {
            x.iter()
                .filter(|(id, (t, v, _))| y.get(id).is_none_or(|(t1, v1, _)| (t, v) != (t1, v1)))
                .map(|(_, p)| *p)
                .collect::<Vec<Point>>()
        };
        let gone = differ(old, new);
        let theirs = gone.iter().any(|(_, _, ours)| !ours);
        let mut changed: Vec<Tick> = gone.iter().map(|p| p.0).collect();
        changed.extend(differ(new, old).iter().map(|p| p.0));
        let (Some(lo), Some(hi)) = (changed.iter().min().copied(), changed.iter().max().copied())
        else {
            continue;
        };
        let owner = match target {
            AutomationTarget::VcaFader(v) => Owner::Vca(v),
            _ => match strip_of(after, target).or_else(|| strip_of(before, target)) {
                Some(strip) => Owner::Strip(strip),
                None => continue,
            },
        };
        let Some(label) = automation_label(before, after, target, params) else {
            continue;
        };
        let within = |m: &BTreeMap<EventId, Point>| {
            m.values().filter(|(t, _, _)| (lo..=hi).contains(t)).count()
        };
        let (n, m) = (within(old), within(new));
        let doc = if after.sequence(seq).is_ok() {
            after
        } else {
            before
        };
        let bars = format!("bars {}–{}", bar_of(doc, seq, lo), bar_of(doc, seq, hi));
        let part = if m == 0 {
            format!("{label} automation cleared, {bars}")
        } else {
            format!(
                "{label} automation {} → {}, {bars}",
                count(n, "point", "points"),
                count(m, "point", "points")
            )
        };
        let entry = out.entry(owner).or_default();
        entry.0.push(part);
        entry.1 |= theirs;
    }
    out
}

/// Plain items for what differs from `before` to `after` (Ruling 6): by
/// sequence then track order, then strips; `[]` when they are equal, and
/// one "The song" / "other changes" when they differ in nothing listed.
/// "Track N" is the track's 1-based position in its sequence, before the
/// change for a removal and after it otherwise.
///
/// A strip present before and after gets one item for its mixer changes
/// (plan 8d Ruling 4), after its rename and colour items; its automation
/// changes (plan 8e Ruling 4) follow the parameters in that item, after
/// what its VCAs do to it (`heard …`, `muted by a VCA`). The "Group" and
/// "VCA" items follow the strips (groups-vcas-transport Task 7).
///
/// A track's item also lists its regions' fade shapes ("fade in linear →
/// s-curve") and its output ("output Audio 1 → none"); a Built-in Synth
/// whose sound changed is one "Built-in Synth" / "sound Piano → Pad" item
/// after the strips (plan 8e Task 6b).
pub fn describe(before: &Document, after: &Document) -> Vec<ChangeItem> {
    describe_with(before, after, &|_, _| None)
}

/// [`describe`], with parameter names and present values from `params`
/// (plan 8d Ruling 4).
pub fn describe_with(before: &Document, after: &Document, params: ParamLookup) -> Vec<ChangeItem> {
    if before == after {
        return Vec::new();
    }
    let mut out = Vec::new();
    sequences(before, after, &mut out);
    let mut automation = automation_parts(before, after, params);
    strips(before, after, &mut automation, params, &mut out);
    groups(before, after, &mut out);
    vcas(before, after, &mut automation, &mut out);
    synth_sounds(before, after, &mut out);
    if out.is_empty() {
        out.push(item("The song".into(), "other changes".into()));
    }
    out
}
