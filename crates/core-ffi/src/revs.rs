//! Lane revisions (spec §3.1). A lane's revision changes whenever anything
//! in it changes: the events of any take, the active take, its flags, its
//! routing, its drum layout. They are computed here by comparing the
//! document the revisions were last computed for with the current one:
//! `im` shares the event maps of unchanged lanes, so a pointer comparison
//! finds them, and `core-model` keeps no bookkeeping (Plan Ruling 1).
//! What a lane's rows are computed from counts too: a sequence's tempo map
//! (region durations) and meter map (fixed-bar blocks) bump all its lanes,
//! and the media pool (region durations) bumps every lane holding a region.

use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::{LaneId, SeqId};
use core_model::lane::Lane;
use std::collections::BTreeMap;

/// One lane's revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct LaneRev {
    /// Sequence ID.
    pub seq: u64,
    /// Lane ID.
    pub lane: u64,
    /// Changes whenever the lane changes; never reused within a session.
    pub rev: u64,
}

/// The revisions of every lane of one session's document.
#[derive(Debug, Clone)]
pub struct Revisions {
    /// The document `revs` describe.
    base: Document,
    revs: BTreeMap<(SeqId, LaneId), u64>,
    next: u64,
}

impl Revisions {
    /// Revisions 1, 2, … for every lane of `doc`.
    pub fn new(doc: &Document) -> Revisions {
        let mut r = Revisions {
            base: doc.clone(),
            revs: BTreeMap::new(),
            next: 1,
        };
        for s in doc.sequences().values() {
            for l in &s.lanes {
                let rev = r.bump();
                r.revs.insert((s.id, l.id), rev);
            }
        }
        r
    }

    fn bump(&mut self) -> u64 {
        let r = self.next;
        self.next = self.next.saturating_add(1);
        r
    }

    /// Brings the revisions up to `doc`: lanes that changed since the last
    /// update, and new lanes, get a fresh revision; lanes that are gone
    /// are dropped. O(1) when no sequence and no media item changed (a
    /// mixer edit).
    pub fn update(&mut self, doc: &Document) {
        let same_media = self.base.media().ptr_eq(doc.media()) || self.base.media() == doc.media();
        if self.base.sequences().ptr_eq(doc.sequences()) && same_media {
            return;
        }
        let mut revs = BTreeMap::new();
        let mut next = self.next;
        for s in doc.sequences().values() {
            // Tempo and meter shape the rows of every lane of the sequence.
            let before = self
                .base
                .sequences()
                .get(&s.id)
                .filter(|b| b.tempo == s.tempo && b.meter == s.meter);
            for l in &s.lanes {
                let key = (s.id, l.id);
                let kept = before
                    .and_then(|b| b.lane(l.id))
                    .filter(|old| same_lane(old, l))
                    .filter(|_| same_media || !holds_regions(l))
                    .and_then(|_| self.revs.get(&key).copied());
                let rev = kept.unwrap_or_else(|| {
                    let fresh = next;
                    next = next.saturating_add(1);
                    fresh
                });
                revs.insert(key, rev);
            }
        }
        self.next = next;
        self.revs = revs;
        self.base = doc.clone();
    }

    /// A lane's revision (`None` when the lane does not exist).
    pub fn rev(&self, seq: SeqId, lane: LaneId) -> Option<u64> {
        self.revs.get(&(seq, lane)).copied()
    }

    /// Every lane's revision, by sequence ID then lane ID.
    pub fn rows(&self) -> Vec<LaneRev> {
        self.revs
            .iter()
            .map(|((s, l), r)| LaneRev {
                seq: s.0,
                lane: l.0,
                rev: *r,
            })
            .collect()
    }
}

/// True when nothing an editor shows can differ between `a` and `b`.
/// Takes are compared one by one (`im::Vector::ptr_eq` is always false for
/// the inline vectors of one or two takes); event maps by pointer, so a
/// lane rebuilt with equal content counts as changed (harmless).
pub fn same_lane(a: &Lane, b: &Lane) -> bool {
    a.id == b.id
        && a.name == b.name
        && a.midi_dest == b.midi_dest
        && a.audio_out == b.audio_out
        && a.mute == b.mute
        && a.solo == b.solo
        && a.arm == b.arm
        && a.length == b.length
        && a.looped == b.looped
        && a.length_locked == b.length_locked
        && a.play_quantize == b.play_quantize
        && a.play_shift == b.play_shift
        && a.drum == b.drum
        && a.active_take == b.active_take
        && a.takes.len() == b.takes.len()
        && a.takes
            .iter()
            .zip(b.takes.iter())
            .all(|(x, y)| x.id == y.id && x.name == y.name && x.events.ptr_eq(&y.events))
}

/// True when any take of `lane` holds an audio region (its duration depends
/// on the media pool).
fn holds_regions(lane: &Lane) -> bool {
    lane.takes.iter().any(|t| {
        t.events
            .values()
            .any(|e| matches!(e.body, EventBody::AudioRegion { .. }))
    })
}
