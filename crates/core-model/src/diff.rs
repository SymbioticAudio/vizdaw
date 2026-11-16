//! What a change does, in plain items (plan 8a Ruling 6): the list
//! Co-Producer's preview shows the composer before they say yes.
//!
//! Every name shown passes through one helper, [`shown_name`], so a name
//! Claude gave a track, sequence or bus can never add or hide lines in
//! that list.

use crate::document::Document;
use crate::event::EventBody;
use crate::ids::{LaneId, SeqId, StripId};
use crate::lane::Lane;
use crate::sequence::Sequence;
use std::collections::{BTreeMap, BTreeSet};

/// One thing a change does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeItem {
    /// What it acts on ("Track 3, Lead Vox", "Strip Drums", "The song").
    pub thing: String,
    /// What happens to it ("deleted, with 4 notes", "renamed to \"Vox\"").
    pub change: String,
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
    ChangeItem { thing, change }
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
        out.push(item(track(i, &l.name), change));
    }
    for (i, l) in after.lanes.iter().enumerate() {
        let Some((_, old)) = b.get(&l.id) else {
            out.push(item(track(i, &l.name), "added".into()));
            continue;
        };
        if old.name != l.name {
            out.push(item(track(i, &old.name), renamed(&l.name)));
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
        if !parts.is_empty() {
            out.push(item(track(i, &old.name), parts.join(", ")));
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

fn strips(before: &Document, after: &Document, out: &mut Vec<ChangeItem>) {
    let (b, a) = (before.mixer().strips(), after.mixer().strips());
    for id in strip_order(before) {
        if let (Some(s), None) = (b.get(&id), a.get(&id)) {
            out.push(item(strip(&s.name), "deleted".into()));
        }
    }
    for id in strip_order(after) {
        let Some(s) = a.get(&id) else { continue };
        match b.get(&id) {
            None => out.push(item(strip(&s.name), "added".into())),
            Some(old) if old.name != s.name => {
                out.push(item(strip(&old.name), renamed(&s.name)));
            }
            Some(_) => {}
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
