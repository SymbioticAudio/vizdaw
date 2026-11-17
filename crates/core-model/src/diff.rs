//! What a change does, in plain items (plan 8a Ruling 6): the list
//! Co-Producer's preview shows the composer before they say yes.
//!
//! Every name shown passes through one helper, [`shown_name`], so a name
//! Claude gave a track, sequence or bus can never add or hide lines in
//! that list.

use crate::document::Document;
use crate::event::{Event, EventBody};
use crate::ids::{EventId, LaneId, SeqId, StripId};
use crate::lane::Lane;
use crate::mixer::Swatch;
use crate::sequence::{Marker, Sequence};
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

/// The track items of one sequence present before and after.
fn tracks(before: &Sequence, after: &Sequence, out: &mut Vec<ChangeItem>) {
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
                    tracks(b, a, out);
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

fn strips(before: &Document, after: &Document, out: &mut Vec<ChangeItem>) {
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
    }
}

/// Plain items for what differs from `before` to `after` (Ruling 6): by
/// sequence then track order, then strips; `[]` when they are equal, and
/// one "The song" / "other changes" when they differ in nothing listed.
/// "Track N" is the track's 1-based position in its sequence, before the
/// change for a removal and after it otherwise.
pub fn describe(before: &Document, after: &Document) -> Vec<ChangeItem> {
    if before == after {
        return Vec::new();
    }
    let mut out = Vec::new();
    sequences(before, after, &mut out);
    strips(before, after, &mut out);
    if out.is_empty() {
        out.push(item("The song".into(), "other changes".into()));
    }
    out
}
