//! The merge against what it will meet (plan 6e Task 7, spec §12):
//!
//! - **edit scripts** (proptest): random documents from 6d's round-trip
//!   generators, a random script of edits applied to theirs (in the file)
//!   and another to ours (as commands), returned with a random [`Strip`].
//!   The merge never panics, validates, previews what it decides, keeps
//!   VizDAW's changes and VizDAW-only state, is one undo step, and merging
//!   the same file again adds nothing;
//! - **hostile records**: a malformed `record.json`, snapshot IDs naming a
//!   track ours deleted, IDs of the wrong kind, two IDs naming one track,
//!   and a huge ID map at lowered limits;
//! - **byte damage** (flip, insert, delete, truncate) on three returned
//!   packages and on their snapshots (`VIZDAW_FUZZ_CASES`, default 256):
//!   each case is refused with a sentence or merges, never panics, and
//!   stays under 2 s and 256 MiB.

mod common;

use common::generators::{Spec, build, spec};
use common::merge::{
    Except, Strip, Trip, fixture, played_sequences, return_file, shared_fixture, stripped_package,
    theirs_of, trip, trip_with, vizdaw_only_except,
};
use common::package::{TestPlist, package_bytes};
use common::{CountingAlloc, cases, measure};
use core_engine::fixtures::Q;
use core_io::IoError;
use core_io::dawproject::export::RegionGain;
use core_io::dawproject::import::{ImportEnv, MediaSource, TAGS_IGNORED, map};
use core_io::dawproject::merge::apply::{LABEL, apply_merge};
use core_io::dawproject::merge::decide::{MergePlan, Outcome, plan};
use core_io::dawproject::merge::matching::notes as pair_notes;
use core_io::dawproject::merge::text;
use core_io::dawproject::merge::theirs::{Theirs, theirs};
use core_io::dawproject::merge::{FRAMES, LEVEL_DB, NOTE_TICKS, PAN, normalised};
use core_io::dawproject::package::{RECORD_UNREADABLE, open};
use core_io::dawproject::record::{
    Identity, ObjectRef, PRegion, PStrip, PTrack, Projection, SNAPSHOT_DAMAGED, Snapshot,
    SnapshotLimits, projection_with_identity, read_snapshot, read_snapshot_with, returned,
};
use core_io::dawproject::{
    DpChannel, DpClip, DpMarker, DpNamed, DpNote, DpProject, DpStructureItem, DpTimeline, DpTrack,
};
use core_io::limits::PackageLimits;
use core_io::report::{ItemKind, Report};
use core_model::command::{Command, EventCmd, LaneCmd, MixerCmd, SequenceCmd};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::file::to_json;
use core_model::history::History;
use core_model::ids::{EventId, LaneId, SeqId, StripId, TakeId};
use core_model::mixer::PluginFormat;
use core_model::rules::validate;
use core_model::selection::ViewFilter;
use core_model::sequence::Marker;
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::Tick;
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const DATE: &str = "2026-11-13";
const MIB: usize = 1024 * 1024;

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

// ---------------------------------------------------------------- theirs' edits (in the file)

/// One edit the other DAW makes, on the file's project. Indices pick among
/// what is there (modulo its count); times are in quarter beats.
#[derive(Debug, Clone)]
enum TheirEdit {
    MoveNote(u16, i8),
    AddNote(u16, i8, u8),
    DeleteNote(u16),
    MoveClip(u16, i8),
    AddClip(u16, i8),
    DeleteClip(u16),
    Volume(u16, i8),
    Pan(u16, i8),
    Mute(u16, bool),
    Rename(u16, String),
    AddMarker(u8, String),
    MoveMarker(u16, i8),
    Tempo(u16),
    DeleteTrack(u16),
    /// A copy of an audio track (its crossfaded pairs too), as a new track
    /// without IDs or tags (Task 7 fix round 3).
    AddTrack(u16),
}

fn name() -> impl Strategy<Value = String> {
    "[A-Z][a-z]{1,6}"
}

fn their_edit() -> impl Strategy<Value = TheirEdit> {
    prop_oneof![
        (any::<u16>(), -8i8..=8).prop_map(|(i, d)| TheirEdit::MoveNote(i, d)),
        (any::<u16>(), 0i8..=16, 24u8..=96).prop_map(|(i, t, k)| TheirEdit::AddNote(i, t, k)),
        any::<u16>().prop_map(TheirEdit::DeleteNote),
        (any::<u16>(), -8i8..=8).prop_map(|(i, d)| TheirEdit::MoveClip(i, d)),
        (any::<u16>(), 1i8..=32).prop_map(|(i, d)| TheirEdit::AddClip(i, d)),
        any::<u16>().prop_map(TheirEdit::DeleteClip),
        (any::<u16>(), -60i8..=6).prop_map(|(i, db)| TheirEdit::Volume(i, db)),
        (any::<u16>(), -16i8..=16).prop_map(|(i, p)| TheirEdit::Pan(i, p)),
        (any::<u16>(), any::<bool>()).prop_map(|(i, m)| TheirEdit::Mute(i, m)),
        (any::<u16>(), name()).prop_map(|(i, n)| TheirEdit::Rename(i, n)),
        (0u8..=128, name()).prop_map(|(t, n)| TheirEdit::AddMarker(t, n)),
        (any::<u16>(), -8i8..=8).prop_map(|(i, d)| TheirEdit::MoveMarker(i, d)),
        (40u16..=240).prop_map(TheirEdit::Tempo),
        any::<u16>().prop_map(TheirEdit::DeleteTrack),
        any::<u16>().prop_map(TheirEdit::AddTrack),
    ]
}

/// `items[i % len]`, if there are any.
fn pick<T>(items: &mut [T], i: u16) -> Option<&mut T> {
    let n = items.len();
    (n > 0).then(|| items.get_mut(usize::from(i) % n)).flatten()
}

/// Every notes list of `t` and what it holds (clips' content included).
fn note_lists<'a>(t: &'a mut DpTimeline, out: &mut Vec<&'a mut Vec<DpNote>>) {
    match t {
        DpTimeline::Lanes(l) => {
            for i in &mut l.items {
                note_lists(i, out);
            }
        }
        DpTimeline::Clips(c) => {
            for clip in &mut c.clips {
                if let Some(inner) = clip.content.as_deref_mut() {
                    note_lists(inner, out);
                }
            }
        }
        DpTimeline::Notes(n) => out.push(&mut n.notes),
        _ => {}
    }
}

/// The clip lists on the arrangement's tracks (not inside clips).
fn clip_lists<'a>(t: &'a mut DpTimeline, out: &mut Vec<&'a mut Vec<DpClip>>) {
    match t {
        DpTimeline::Lanes(l) => {
            for i in &mut l.items {
                clip_lists(i, out);
            }
        }
        DpTimeline::Clips(c) => out.push(&mut c.clips),
        _ => {}
    }
}

/// The arrangement's top timelines.
fn top_items(p: &mut DpProject) -> Vec<&mut DpTimeline> {
    match p.arrangement.as_mut().and_then(|a| a.lanes.as_mut()) {
        Some(l) => l.items.iter_mut().collect(),
        None => Vec::new(),
    }
}

fn all_notes(p: &mut DpProject) -> Vec<&mut DpNote> {
    let mut lists = Vec::new();
    for t in top_items(p) {
        note_lists(t, &mut lists);
    }
    lists.into_iter().flat_map(|l| l.iter_mut()).collect()
}

fn all_clip_lists(p: &mut DpProject) -> Vec<&mut Vec<DpClip>> {
    let mut lists = Vec::new();
    for t in top_items(p) {
        clip_lists(t, &mut lists);
    }
    lists
}

fn channels_of<'a>(t: &'a mut DpTrack, out: &mut Vec<&'a mut DpChannel>) {
    if let Some(c) = t.channel.as_deref_mut() {
        out.push(c);
    }
    for child in &mut t.children {
        channels_of(child, out);
    }
}

fn all_channels(p: &mut DpProject) -> Vec<&mut DpChannel> {
    let mut out = Vec::new();
    for i in &mut p.structure {
        match i {
            DpStructureItem::Track(t) => channels_of(t, &mut out),
            DpStructureItem::Channel(c) => out.push(c),
        }
    }
    out
}

/// The top-level tracks that are not the master.
fn plain_tracks(p: &mut DpProject) -> Vec<&mut DpTrack> {
    p.structure
        .iter_mut()
        .filter_map(|i| match i {
            DpStructureItem::Track(t) => Some(t),
            DpStructureItem::Channel(_) => None,
        })
        .filter(|t| {
            t.channel
                .as_ref()
                .is_none_or(|c| c.role != Some(core_io::dawproject::DpMixerRole::Master))
        })
        .collect()
}

fn apply_their_edit(p: &mut DpProject, e: &TheirEdit) {
    let quarter = |q: i8| f64::from(q) / 4.0;
    match e {
        TheirEdit::MoveNote(i, d) => {
            if let Some(n) = pick(&mut all_notes(p), *i) {
                n.time = (n.time + quarter(*d)).max(0.0);
            }
        }
        TheirEdit::AddNote(i, t, k) => {
            let mut lists = Vec::new();
            for x in top_items(p) {
                note_lists(x, &mut lists);
            }
            if let Some(l) = pick(&mut lists, *i) {
                l.push(DpNote {
                    time: quarter(*t),
                    duration: 0.5,
                    channel: 0,
                    key: i32::from(*k),
                    vel: Some(0.5),
                    rel: None,
                    content: None,
                });
            }
        }
        TheirEdit::DeleteNote(i) => {
            let mut lists = Vec::new();
            for x in top_items(p) {
                note_lists(x, &mut lists);
            }
            let mut lists: Vec<_> = lists.into_iter().filter(|l| !l.is_empty()).collect();
            if let Some(l) = pick(&mut lists, *i) {
                let at = usize::from(*i) % l.len();
                l.remove(at);
            }
        }
        TheirEdit::MoveClip(i, d) => {
            let mut clips: Vec<&mut DpClip> = all_clip_lists(p)
                .into_iter()
                .flat_map(|l| l.iter_mut())
                .collect();
            if let Some(c) = pick(&mut clips, *i) {
                c.time = (c.time + quarter(*d)).max(0.0);
            }
        }
        TheirEdit::AddClip(i, d) => {
            let mut lists: Vec<_> = all_clip_lists(p)
                .into_iter()
                .filter(|l| !l.is_empty())
                .collect();
            if let Some(l) = pick(&mut lists, *i) {
                let at = usize::from(*i) % l.len();
                if let Some(c) = l.get(at) {
                    let mut copy = c.clone();
                    copy.time += quarter(*d);
                    copy.named.id = None;
                    l.push(copy);
                }
            }
        }
        TheirEdit::DeleteClip(i) => {
            let mut lists: Vec<_> = all_clip_lists(p)
                .into_iter()
                .filter(|l| !l.is_empty())
                .collect();
            if let Some(l) = pick(&mut lists, *i) {
                let at = usize::from(*i) % l.len();
                l.remove(at);
            }
        }
        TheirEdit::Volume(i, db) => {
            if let Some(v) = pick(&mut all_channels(p), *i).and_then(|c| c.volume.as_deref_mut()) {
                v.value = Some(10f64.powf(f64::from(*db) / 20.0));
            }
        }
        TheirEdit::Pan(i, x) => {
            if let Some(v) = pick(&mut all_channels(p), *i).and_then(|c| c.pan.as_deref_mut()) {
                v.value = Some((f64::from(*x) + 16.0) / 32.0);
            }
        }
        TheirEdit::Mute(i, m) => {
            if let Some(v) = pick(&mut all_channels(p), *i).and_then(|c| c.mute.as_deref_mut()) {
                v.value = Some(*m);
            }
        }
        TheirEdit::Rename(i, n) => {
            if let Some(t) = pick(&mut plain_tracks(p), *i) {
                t.named.name = Some(n.clone());
            }
        }
        TheirEdit::AddMarker(t, n) => {
            if let Some(a) = p.arrangement.as_mut() {
                a.markers.push(DpMarker {
                    named: DpNamed {
                        name: Some(n.clone()),
                        ..DpNamed::default()
                    },
                    time: quarter(i8::try_from(*t / 2).unwrap_or(0)) * 2.0,
                });
            }
        }
        TheirEdit::MoveMarker(i, d) => {
            if let Some(a) = p.arrangement.as_mut()
                && let Some(m) = pick(&mut a.markers, *i)
            {
                m.time = (m.time + quarter(*d)).max(0.0);
            }
        }
        TheirEdit::Tempo(bpm) => {
            if let Some(t) = p.tempo.as_mut() {
                t.value = Some(f64::from(*bpm));
            }
        }
        TheirEdit::AddTrack(i) => add_track(p, *i),
        TheirEdit::DeleteTrack(i) => {
            let id = pick(&mut plain_tracks(p), *i).and_then(|t| t.named.id.clone());
            if let Some(id) = id {
                p.structure.retain(|x| {
                    !matches!(x, DpStructureItem::Track(t) if t.named.id.as_deref() == Some(&id))
                });
                if let Some(l) = p.arrangement.as_mut().and_then(|a| a.lanes.as_mut()) {
                    l.items.retain(|x| {
                        !matches!(x, DpTimeline::Lanes(l) if l.head.track.as_deref() == Some(&id))
                    });
                }
            }
        }
    }
}

/// `t` and what it holds without IDs or comments (so without tags) and
/// without its automation (which names the source track's parameters), its
/// track references to `old` made `new`.
fn reid(t: &mut DpTimeline, old: &str, new: &str) {
    let head = |h: &mut core_io::dawproject::DpHead| {
        h.named.id = None;
        h.named.comment = None;
        if h.track.as_deref() == Some(old) {
            h.track = Some(new.to_owned());
        }
    };
    match t {
        DpTimeline::Lanes(l) => {
            head(&mut l.head);
            // Automation names the source's parameters: not copied.
            l.items.retain(|i| !matches!(i, DpTimeline::Points(_)));
            for i in &mut l.items {
                reid(i, old, new);
            }
        }
        DpTimeline::Clips(c) => {
            head(&mut c.head);
            for clip in &mut c.clips {
                clip.named.id = None;
                clip.named.comment = None;
                if let Some(inner) = clip.content.as_deref_mut() {
                    reid(inner, old, new);
                }
            }
        }
        DpTimeline::Notes(n) => head(&mut n.head),
        DpTimeline::Points(x) => head(&mut x.head),
        DpTimeline::Audio(a) => head(&mut a.head),
        DpTimeline::Warps(w) => {
            head(&mut w.head);
            reid(&mut w.content, old, new);
        }
        DpTimeline::Markers(h, _) | DpTimeline::ClipSlot(h, _) | DpTimeline::Video(h) => head(h),
        DpTimeline::Unknown(_) => {}
    }
}

/// [`TheirEdit::AddTrack`]: a copy of the `i`th audio track of VizDAW's
/// (`t<lane>`), named "<name> copy", with no devices or sends, placed after
/// the file's last such track, and its arrangement copied with it.
fn add_track(p: &mut DpProject, i: u16) {
    let mut audio: Vec<DpTrack> = plain_tracks(p)
        .into_iter()
        .filter(|t| {
            t.named.id.as_deref().is_some_and(|x| x.starts_with('t'))
                && t.children.is_empty()
                && t.content
                    .contains(&core_io::dawproject::DpContentType::Audio)
        })
        .map(|t| t.clone())
        .collect();
    let Some(src) = pick(&mut audio, i) else {
        return;
    };
    let mut copy = src.clone();
    let old = copy.named.id.clone().unwrap_or_default();
    let taken = top_tracks(p);
    let new = (0usize..)
        .map(|k| format!("x{k}{old}"))
        .find(|id| !taken.contains_key(id))
        .unwrap_or_default();
    copy.named.id = Some(new.clone());
    copy.named.comment = None;
    copy.named.name = Some(format!("{} copy", copy.named.name.unwrap_or_default()));
    if let Some(c) = copy.channel.as_deref_mut() {
        c.named.id = Some(format!("{new}c"));
        c.named.comment = None;
        c.devices.clear();
        c.sends.clear();
        if let Some(v) = c.volume.as_deref_mut() {
            v.named.id = None;
        }
        if let Some(v) = c.pan.as_deref_mut() {
            v.named.id = None;
        }
        if let Some(v) = c.mute.as_deref_mut() {
            v.named.id = None;
        }
    }
    let lanes: Vec<DpTimeline> = top_items(p)
        .into_iter()
        .filter(
            |t| matches!(t, DpTimeline::Lanes(l) if l.head.track.as_deref() == Some(old.as_str())),
        )
        .map(|t| {
            let mut t = t.clone();
            reid(&mut t, &old, &new);
            t
        })
        .collect();
    let at = p
        .structure
        .iter()
        .rposition(|x| {
            matches!(x, DpStructureItem::Track(t)
                if t.named.id.as_deref().is_some_and(|i| i.starts_with('t') || i.starts_with('x')))
        })
        .map_or(p.structure.len(), |k| k + 1);
    p.structure.insert(at, DpStructureItem::Track(copy));
    if let Some(l) = p.arrangement.as_mut().and_then(|a| a.lanes.as_mut()) {
        l.items.extend(lanes);
    }
}

// ---------------------------------------------------------------- ours' edits (commands)

/// One edit made here after the export, as a command. Indices pick among
/// what is there; a command the model refuses is skipped.
#[derive(Debug, Clone)]
enum OurEdit {
    MoveEvent(u16, i8),
    AddNote(u16, i8, u8),
    DeleteEvent(u16),
    Gain(u16, i8),
    Pan(u16, i8),
    Mute(u16, bool),
    Rename(u16, String),
    AddMarker(u8, String),
    Tempo(u16),
    RemoveTrack(u16),
}

fn our_edit() -> impl Strategy<Value = OurEdit> {
    prop_oneof![
        (any::<u16>(), -8i8..=8).prop_map(|(i, d)| OurEdit::MoveEvent(i, d)),
        (any::<u16>(), 0i8..=16, 24u8..=96).prop_map(|(i, t, k)| OurEdit::AddNote(i, t, k)),
        any::<u16>().prop_map(OurEdit::DeleteEvent),
        (any::<u16>(), -60i8..=6).prop_map(|(i, db)| OurEdit::Gain(i, db)),
        (any::<u16>(), -16i8..=16).prop_map(|(i, p)| OurEdit::Pan(i, p)),
        (any::<u16>(), any::<bool>()).prop_map(|(i, m)| OurEdit::Mute(i, m)),
        (any::<u16>(), name()).prop_map(|(i, n)| OurEdit::Rename(i, n)),
        (0u8..=128, name()).prop_map(|(t, n)| OurEdit::AddMarker(t, n)),
        // Below about 20 BPM the generator's crossfaded pairs (a beat
        // apart, three seconds long) no longer overlap: ours' tempo then
        // settles ours' own crossfades (Task 7 fix round 4, N1).
        prop_oneof![40u16..=240, 8u16..=24].prop_map(OurEdit::Tempo),
        any::<u16>().prop_map(OurEdit::RemoveTrack),
    ]
}

fn lanes_of(doc: &Document, seq: SeqId) -> Vec<LaneId> {
    doc.sequence(seq)
        .map(|s| s.lanes.iter().map(|l| l.id).collect())
        .unwrap_or_default()
}

fn events_of(doc: &Document, seq: SeqId) -> Vec<EventId> {
    doc.sequence(seq)
        .map(|s| {
            s.lanes
                .iter()
                .filter_map(|l| l.active())
                .flat_map(|t| t.events.values().map(|e| e.id))
                .collect()
        })
        .unwrap_or_default()
}

fn strips_of(doc: &Document) -> Vec<StripId> {
    doc.mixer().strips().keys().copied().collect()
}

fn command_of(doc: &Document, seq: SeqId, e: &OurEdit) -> Option<Command> {
    let q = |x: i8| i64::from(x) * Q / 4;
    Some(match e {
        OurEdit::MoveEvent(i, d) => Command::Event(EventCmd::Move {
            events: vec![*pick(&mut events_of(doc, seq), *i)?],
            delta: Tick(q(*d)),
            to_lane: None,
        }),
        OurEdit::AddNote(i, t, k) => Command::Event(EventCmd::Insert {
            seq,
            lane: *pick(&mut lanes_of(doc, seq), *i)?,
            events: vec![EventSpec::new(
                Tick(q(*t)),
                EventBody::Note {
                    pitch: *k,
                    velocity: 0x8000,
                    release_velocity: 0x8000,
                    duration: Tick(Q),
                    attribute: None,
                    per_note: Default::default(),
                },
            )],
        }),
        OurEdit::DeleteEvent(i) => Command::Event(EventCmd::Delete {
            events: vec![*pick(&mut events_of(doc, seq), *i)?],
        }),
        OurEdit::Gain(i, db) => Command::Mixer(MixerCmd::SetGain {
            strip: *pick(&mut strips_of(doc), *i)?,
            gain_db: f64::from(*db),
        }),
        OurEdit::Pan(i, x) => Command::Mixer(MixerCmd::SetPan {
            strip: *pick(&mut strips_of(doc), *i)?,
            pan: f64::from(*x) / 16.0,
        }),
        OurEdit::Mute(i, m) => Command::Mixer(MixerCmd::SetMute {
            strip: *pick(&mut strips_of(doc), *i)?,
            mute: *m,
        }),
        OurEdit::Rename(i, n) => Command::Lane(LaneCmd::Rename {
            seq,
            lane: *pick(&mut lanes_of(doc, seq), *i)?,
            name: n.clone(),
        }),
        OurEdit::AddMarker(t, n) => {
            let mut markers: Vec<Marker> =
                doc.sequence(seq).ok()?.markers.iter().cloned().collect();
            markers.push(Marker {
                tick: Tick(i64::from(*t) * Q / 2),
                name: n.clone(),
            });
            Command::Sequence(SequenceCmd::SetMarkers {
                seq,
                markers: markers.into_iter().collect(),
            })
        }
        OurEdit::Tempo(bpm) => Command::Sequence(SequenceCmd::SetTempo {
            seq,
            tempo: TempoMap::new(vec![TempoPoint {
                tick: Tick(0),
                bpm: Bpm(u32::from(*bpm) * 1000),
                ramp: false,
            }])
            .ok()?,
        }),
        OurEdit::RemoveTrack(i) => Command::Lane(LaneCmd::Remove {
            seq,
            lane: *pick(&mut lanes_of(doc, seq), *i)?,
        }),
    })
}

/// `doc` after `script`, each edit a command (the refused ones skipped).
fn ours_after(doc: &Document, seq: SeqId, script: &[OurEdit]) -> Document {
    let mut h = History::new(doc.clone());
    for e in script {
        if let Some(cmd) = command_of(h.doc(), seq, e) {
            let _ = h.execute(&cmd, &ViewFilter::default());
        }
    }
    h.doc().clone()
}

// ---------------------------------------------------------------- the properties

/// The preview's counts are the decisions' (Ruling 15), with one conflict
/// line per conflict.
#[track_caller]
fn counts_are_the_decisions(p: &MergePlan) {
    let (mut applied, mut kept, mut added, mut removed, mut conflicts) = (0, 0, 0, 0, 0);
    for d in &p.decisions {
        match d.outcome {
            Outcome::Theirs | Outcome::Same => applied += 1,
            Outcome::Ours => kept += 1,
            Outcome::Added => added += 1,
            Outcome::Removed => removed += 1,
            Outcome::Conflict
            | Outcome::KeptEmpty
            | Outcome::DeletedThere
            | Outcome::DeletedHere => conflicts += 1,
        }
    }
    let v = &p.preview;
    assert_eq!(
        (v.applied, v.kept, v.added, v.removed, v.conflicts),
        (applied, kept, added, removed, conflicts),
        "{:#?}",
        p.decisions
    );
    assert_eq!(v.conflict_lines.len(), v.conflicts as usize, "{v:#?}");
}

// ---------------------------------------------------------------- the owner rules

fn level_eq(a: f64, b: f64) -> bool {
    if a < -120.0 && b < -120.0 {
        return true;
    }
    if !a.is_finite() || !b.is_finite() {
        return a == b;
    }
    (a - b).abs() <= LEVEL_DB
}

fn pan_eq(a: f64, b: f64) -> bool {
    if !a.is_finite() || !b.is_finite() {
        return a == b;
    }
    (a - b).abs() <= PAN
}

fn ticks_near(a: i64, b: i64) -> bool {
    a.abs_diff(b) <= NOTE_TICKS.unsigned_abs()
}

fn frames_near(a: u64, b: u64) -> bool {
    a.abs_diff(b) <= FRAMES
}

/// The notes and MIDI events of two track parts are the same set, within
/// the tolerances (Ruling 4).
fn notes_same(a: &PTrack, b: &PTrack) -> bool {
    if a.notes_played.len() != b.notes_played.len() || a.midi.len() != b.midi.len() {
        return false;
    }
    let sorted = |v: &[(i64, String)]| {
        let mut v: Vec<(i64, String)> = v.to_vec();
        v.sort_by(|x, y| (&x.1, x.0).cmp(&(&y.1, y.0)));
        v
    };
    let midi = sorted(&a.midi)
        .iter()
        .zip(sorted(&b.midi))
        .all(|(x, y)| x.1 == y.1 && ticks_near(x.0, y.0));
    midi && pair_notes(a, b).iter().all(|p| match *p {
        (Some(i), Some(j)) => match (a.notes_played.get(i), b.notes_played.get(j)) {
            (Some(x), Some(y)) => {
                x.velocity == y.velocity && x.release == y.release && x.expression == y.expression
            }
            _ => false,
        },
        _ => false,
    })
}

fn diff(a: &PTrack, b: &PTrack) -> String {
    let pairs = pair_notes(a, b);
    let mut out = String::new();
    for p in pairs {
        match p {
            (Some(i), None) => out += &format!("only merged: {:?}\n", a.notes_played[i]),
            (None, Some(j)) => out += &format!("only ours: {:?}\n", b.notes_played[j]),
            (Some(i), Some(j)) if a.notes_played[i] != b.notes_played[j] => {
                out += &format!(
                    "differ: {:?} / {:?}\n",
                    a.notes_played[i], b.notes_played[j]
                )
            }
            _ => {}
        }
    }
    if !regions_same(a, b) {
        out += &format!("regions {:#?}\n vs {:#?}\n", a.regions, b.regions);
    }
    if a.midi != b.midi {
        out += &format!("midi {:?}\n vs {:?}\n", a.midi, b.midi);
    }
    out
}

/// Two regions are the same without their media hash, gain and file stem
/// (Ruling 11).
fn region_same(a: &PRegion, b: &PRegion) -> bool {
    ticks_near(a.tick, b.tick)
        && frames_near(a.offset, b.offset)
        && frames_near(a.length, b.length)
        && frames_near(a.fade_in, b.fade_in)
        && frames_near(a.fade_out, b.fade_out)
        && a.muted == b.muted
        && a.name == b.name
        && a.crossfade == b.crossfade
}

/// The regions of two track parts are the same set.
fn regions_same(a: &PTrack, b: &PTrack) -> bool {
    if a.regions.len() != b.regions.len() {
        return false;
    }
    let mut free: Vec<&PRegion> = b.regions.iter().collect();
    a.regions
        .iter()
        .all(|x| match free.iter().position(|y| region_same(x, y)) {
            Some(i) => {
                free.swap_remove(i);
                true
            }
            None => false,
        })
}

fn tempo_same(a: &[(i64, u32, bool)], b: &[(i64, u32, bool)]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| ticks_near(x.0, y.0) && x.1 == y.1 && x.2 == y.2)
}

/// A side's projection of the merged sequence, with who each row is, in
/// the file's frame (Ruling 1).
struct Side {
    p: Projection,
    ids: Identity,
}

impl Side {
    fn of(doc: &Document, snap: &Snapshot) -> Option<Side> {
        let (p, ids) = projection_with_identity(doc, SeqId(snap.seq)).ok()?;
        Some(Side {
            p: normalised(&p, snap),
            ids,
        })
    }

    /// Track `lane`'s part (its audio part when `audio`).
    fn part(&self, (lane, audio): (u64, bool)) -> Option<&PTrack> {
        self.p
            .tracks
            .iter()
            .zip(&self.ids.tracks)
            .find(|(t, i)| i.lane == lane && t.audio == audio)
            .map(|(t, _)| t)
    }

    fn strip(&self, i: Option<usize>) -> Option<&PStrip> {
        self.p.strips.get(i?)
    }

    /// The one strip of `role`s, if there is exactly one.
    fn only(&self, roles: &[&str]) -> Option<&PStrip> {
        let mut it = self
            .p
            .strips
            .iter()
            .filter(|s| roles.contains(&s.role.as_str()));
        let first = it.next()?;
        it.next().is_none().then_some(first)
    }
}

/// What the owner rules expect the merge's report to list, one line or
/// detail per item.
#[derive(Default)]
struct Expected {
    /// Settings changed on both sides, differently: the end of each one's
    /// [`text::CONFLICT_SETTING`] detail (": level: here …, there …").
    settings: Vec<String>,
    /// Tracks whose notes or regions became a conflict take.
    takes: BTreeSet<u64>,
    /// Track parts theirs changed and ours deleted.
    deleted_here: u64,
    /// Track parts theirs deleted that the merge kept.
    kept_there: u64,
}

/// One setting under the rule table: theirs' change lands when ours made
/// none; otherwise ours' value stays, and a change on both sides that
/// differs is listed.
#[track_caller]
fn setting<V: PartialEq + std::fmt::Debug>(
    e: &mut Expected,
    what: &str,
    [b, t, o, m]: [&V; 4],
    eq: impl Fn(&V, &V) -> bool,
    text: impl Fn(&V) -> String,
) {
    let (bt, bo) = (eq(b, t), eq(b, o));
    if !bt && bo {
        assert!(
            eq(m, t),
            "{what}: theirs' change lands (base {b:?}, theirs {t:?}, merged {m:?})"
        );
    } else {
        assert_eq!(m, o, "{what}: ours stays (base {b:?}, theirs {t:?})");
        if !bt && !bo && !eq(o, t) {
            let field = what.rsplit(' ').next().unwrap_or(what);
            e.settings
                .push(format!(": {field}: here {}, there {}", text(o), text(t)));
        }
    }
}

/// A strip's level, pan and mute under [`setting`].
#[track_caller]
fn strip_settings(e: &mut Expected, what: &str, s: [&PStrip; 4]) {
    let [b, t, o, m] = s;
    setting(
        e,
        &format!("{what} level"),
        [&b.gain_db, &t.gain_db, &o.gain_db, &m.gain_db],
        |x, y| level_eq(*x, *y),
        |x| text::level(*x),
    );
    setting(
        e,
        &format!("{what} pan"),
        [&b.pan, &t.pan, &o.pan, &m.pan],
        |x, y| pan_eq(*x, *y),
        |x| text::pan(*x),
    );
    setting(
        e,
        &format!("{what} mute"),
        [&b.mute, &t.mute, &o.mute, &m.mute],
        |x, y| x == y,
        |x| text::on_off(*x).to_owned(),
    );
}

/// What theirs' script did to the file's top-level tracks, by XML ID.
#[derive(Debug, Default)]
struct TheirsDid {
    deleted: BTreeSet<String>,
    /// Tracks theirs added (copies, [`TheirEdit::AddTrack`]).
    added: BTreeSet<String>,
    /// Each renamed track's new name.
    renamed: BTreeMap<String, String>,
}

fn top_tracks(p: &DpProject) -> BTreeMap<String, String> {
    p.structure
        .iter()
        .filter_map(|i| match i {
            DpStructureItem::Track(t) => Some((
                t.named.id.clone().unwrap_or_default(),
                t.named.name.clone().unwrap_or_default(),
            )),
            DpStructureItem::Channel(_) => None,
        })
        .collect()
}

/// A track part's content or settings differ between `a` and `b`, as far
/// as the scripts can change them.
fn part_differs(a: &PTrack, sa: &Side, b: &PTrack, sb: &Side, strips: bool) -> bool {
    let strip = || match (sa.strip(a.strip), sb.strip(b.strip)) {
        (Some(x), Some(y)) => {
            !level_eq(x.gain_db, y.gain_db) || !pan_eq(x.pan, y.pan) || x.mute != y.mute
        }
        (None, None) => false,
        _ => true,
    };
    a.name != b.name || !notes_same(a, b) || !regions_same(a, b) || (strips && strip())
}

/// Theirs' regions as the model holds them under `merged`'s tempo map:
/// theirs' scratch project set to that map (in theirs' frame, ticks less
/// `origin`), then given its own back, projected. A crossfaded pair that
/// map cannot hold loses its flag or has its fade in shortened (plan 6c);
/// everything else is as imported.
fn held_in(theirs: &Theirs, merged: &Document, seq: SeqId, origin: i64) -> Projection {
    let map = &merged.sequence(seq).expect("merged sequence").tempo;
    let mut points: Vec<TempoPoint> = map
        .points()
        .iter()
        .filter(|p| p.tick.0 > origin)
        .map(|p| TempoPoint {
            tick: Tick(p.tick.0 - origin),
            ..*p
        })
        .collect();
    let at = map.points().iter().rev().find(|p| p.tick.0 <= origin);
    points.insert(
        0,
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm((map.bpm_at(Tick(origin)) * 1000.0).round() as u32),
            ramp: at.is_some_and(|p| p.ramp),
        },
    );
    let to = TempoMap::new(points).expect("a map");
    let back = theirs
        .doc
        .sequence(theirs.seq)
        .expect("theirs")
        .tempo
        .clone();
    let mut h = History::new(theirs.doc.clone());
    for tempo in [to, back] {
        let _ = h.execute(
            &Command::Sequence(SequenceCmd::SetTempo {
                seq: theirs.seq,
                tempo,
            }),
            &ViewFilter::default(),
        );
    }
    let (p, _) = projection_with_identity(h.doc(), theirs.seq).expect("projects");
    returned(&p, origin)
}

/// The owner rules (spec §7.8, §15), from base, theirs' file and ours
/// alone, never from the plan's decisions: an edit made only by theirs
/// lands (a take on a transformed track); where both changed a setting,
/// ours' value stays and the setting is listed; where both changed a
/// track's notes or regions, ours stays and a "Returned <date>" take
/// holds theirs'; every item theirs changed that the merge did not take
/// has its own line.
///
/// `theirs` is the import as it came, never the plan's view of it (fix
/// round 4): what theirs changed is judged on it. `held` is theirs'
/// regions as the merged tempo map holds them ([`held_in`]): what lands,
/// and what a take holds, is compared with these.
#[allow(clippy::too_many_arguments)]
fn owner_rules(
    t: &Trip,
    did: &TheirsDid,
    strip: Strip,
    [theirs, held]: [&Projection; 2],
    ours: &Document,
    merged: &Document,
    report: &Report,
) {
    let snap = &t.snapshot;
    let seq = SeqId(snap.seq);
    let base = Side {
        p: normalised(&snap.projection, snap),
        ids: snap.identity.clone().expect("format 2"),
    };
    let (Some(o), Some(m)) = (Side::of(ours, snap), Side::of(merged, snap)) else {
        panic!("projections");
    };
    let kind = |id: &str| snap.ids.get(id).map(|r| (r.kind.as_str(), r.id));
    let deleted: BTreeSet<u64> = did
        .deleted
        .iter()
        .filter_map(|id| kind(id).filter(|k| k.0 == "track").map(|k| k.1))
        .collect();
    let tracks_renamed: Vec<&String> = did
        .renamed
        .iter()
        .filter(|(id, _)| kind(id).is_some_and(|k| k.0 == "track"))
        .map(|(_, n)| n)
        .collect();
    let bus_touched = did
        .deleted
        .iter()
        .chain(did.renamed.keys())
        .any(|id| id.starts_with('b') && kind(id).is_some_and(|k| k.0 == "strip"));
    // Theirs' tracks are base's, in order, without those theirs deleted.
    let keys: Vec<(u64, bool)> = base
        .p
        .tracks
        .iter()
        .zip(&base.ids.tracks)
        .map(|(tr, i)| (i.lane, tr.audio))
        .collect();
    let kept: Vec<(u64, bool)> = keys
        .iter()
        .copied()
        .filter(|k| !deleted.contains(&k.0))
        .collect();
    // Theirs' tracks are base's kept ones, then the ones theirs added
    // (placed after them, [`add_track`]).
    assert_eq!(
        kept.len() + did.added.len(),
        theirs.tracks.len(),
        "theirs' tracks are base's, then its own"
    );
    let theirs_part: BTreeMap<(u64, bool), &PTrack> =
        kept.iter().copied().zip(&theirs.tracks).collect();
    let held_part: BTreeMap<(u64, bool), &PTrack> =
        kept.iter().copied().zip(&held.tracks).collect();
    // Without the record and tags, tracks pair by name, then order (Task
    // 4 ruling): a script that renames a track and deletes another, or
    // names that repeat, pair by order, not identity. Those rows are left
    // to the plan's own tests.
    let names: Vec<&String> = base.p.tracks.iter().map(|t| &t.name).collect();
    let repeats = names.iter().collect::<BTreeSet<_>>().len() != names.len();
    // A file whose every exported track theirs deleted has no track tags
    // left: theirs' new tracks pair by order too (Task 4 ruling).
    let untagged = kept.is_empty() && !did.added.is_empty();
    let heuristic = untagged
        || strip == Strip::Both
            && (repeats
                || (!did.added.is_empty() && !deleted.is_empty())
                || (!tracks_renamed.is_empty()
                    && (!deleted.is_empty() || tracks_renamed.iter().any(|n| names.contains(n)))));
    let shared =
        |i: Option<usize>| i.is_some() && base.p.tracks.iter().filter(|t| t.strip == i).count() > 1;

    let mut e = Expected::default();
    let mut takes: Vec<(u64, bool, bool, PTrack)> = Vec::new();
    for (bt, key) in base.p.tracks.iter().zip(&keys) {
        let (lane, _) = *key;
        let what = format!("track {lane}");
        if heuristic {
            continue;
        }
        match (theirs_part.get(key).copied(), o.part(*key), m.part(*key)) {
            (Some(tt), Some(ot), mt) => {
                assert!(
                    merged.lane(seq, LaneId(lane)).is_ok(),
                    "{what}: kept on both sides, it stays"
                );
                // A split track's part with nothing left is no part.
                let gone = mt.is_none();
                let empty = PTrack {
                    notes_played: Vec::new(),
                    midi: Vec::new(),
                    regions: Vec::new(),
                    automation: Vec::new(),
                    ..ot.clone()
                };
                let mt = mt.unwrap_or(&empty);
                if !gone {
                    setting(
                        &mut e,
                        &format!("{what} name"),
                        [&bt.name, &tt.name, &ot.name, &mt.name],
                        |x, y| x == y,
                        |x| x.clone(),
                    );
                }
                let l = ours.lane(seq, LaneId(lane)).expect("ours' lane");
                let transformed = l.looped || l.play_quantize.is_some() || l.length.is_some();
                for (regions, same) in [
                    (false, notes_same as fn(&PTrack, &PTrack) -> bool),
                    (true, regions_same),
                ] {
                    let w = format!("{what} {}", if regions { "regions" } else { "notes" });
                    // Ours stays: the track's events of that kind are as
                    // they were (a transformed track's notes play through
                    // the merged tempo map, so compare what it holds).
                    let stays = || {
                        assert_eq!(
                            events_json(merged, seq, lane, regions),
                            events_json(ours, seq, lane, regions),
                            "{w}: ours stays"
                        );
                    };
                    // Changed by theirs: judged as imported. What lands
                    // is theirs as the merged tempo map holds it.
                    let ht = held_part.get(key).copied().unwrap_or(tt);
                    if same(bt, tt) || (!same(bt, ot) && same(ot, ht)) {
                        stays();
                    } else if same(bt, ot) && !transformed {
                        let lands = if regions {
                            regions_same(mt, ht)
                        } else {
                            same(mt, ht)
                        };
                        assert!(lands, "{w}: theirs' change lands\n{}", diff(mt, ht));
                    } else {
                        stays();
                        e.takes.insert(lane);
                        takes.push((lane, key.1, regions, ht.clone()));
                    }
                }
                if !shared(bt.strip)
                    && !gone
                    && let (Some(bs), Some(ts), Some(os), Some(ms)) = (
                        base.strip(bt.strip),
                        Side::strip_of(theirs, tt.strip),
                        o.strip(ot.strip),
                        m.strip(mt.strip),
                    )
                {
                    strip_settings(&mut e, &format!("{what}'s strip"), [bs, ts, os, ms]);
                }
            }
            (None, Some(ot), mt) => {
                // Theirs deleted it: removed, emptied (Ruling 7), or kept
                // when ours changed it, listed.
                if let Some(mt) = mt {
                    e.kept_there += 1;
                    let emptied =
                        mt.notes_played.is_empty() && mt.midi.is_empty() && mt.regions.is_empty();
                    assert!(
                        emptied || (notes_same(mt, ot) && regions_same(mt, ot)),
                        "{what}: deleted there, kept as ours or emptied"
                    );
                } else {
                    assert!(
                        !part_differs(bt, &base, ot, &o, !shared(bt.strip)),
                        "{what}: deleted there and changed here, it is kept"
                    );
                }
            }
            (Some(tt), None, mt) => {
                assert!(mt.is_none(), "{what}: deleted here, it stays deleted");
                let ts = Side {
                    p: theirs.clone(),
                    ids: Identity::default(),
                };
                if part_differs(bt, &base, tt, &ts, !shared(bt.strip)) {
                    e.deleted_here += 1;
                }
            }
            (None, None, mt) => assert!(mt.is_none(), "{what}: deleted on both sides"),
        }
    }
    // The master, and the one bus (unless theirs renamed or deleted it:
    // without the record its name is what pairs it).
    let all = [&base, &o, &m];
    let ts = Side {
        p: theirs.clone(),
        ids: Identity::default(),
    };
    let master = |s: &Side| s.only(&["master"]).cloned();
    if let (Some(b), Some(t), Some(o), Some(m)) =
        (master(all[0]), master(&ts), master(all[1]), master(all[2]))
    {
        strip_settings(&mut e, "the master", [&b, &t, &o, &m]);
    }
    let bus = |s: &Side| s.only(&["effect", "submix"]).cloned();
    if !bus_touched
        && let (Some(b), Some(t), Some(o), Some(m)) =
            (bus(all[0]), bus(&ts), bus(all[1]), bus(all[2]))
    {
        strip_settings(&mut e, "the bus", [&b, &t, &o, &m]);
    }
    setting(
        &mut e,
        "the tempo map",
        [&base.p.tempo, &theirs.tempo, &o.p.tempo, &m.p.tempo],
        |x, y| tempo_same(x, y),
        |x| format!("{x:?}"),
    );
    e.settings.retain(|s| !s.starts_with(": map:"));

    // Each conflict take holds theirs' notes or regions.
    for (lane, audio, regions, tt) in &takes {
        let new: Vec<_> = returned_takes(ours, merged, seq, *lane);
        assert_eq!(new.len(), 1, "track {lane}: one conflict take");
        let take = take_side(merged, snap, LaneId(*lane), new[0]);
        let part = take.part((*lane, *audio)).expect("the take's part");
        if *regions {
            assert!(
                regions_same(part, tt),
                "track {lane}: the take holds theirs' regions"
            );
        } else {
            assert!(
                notes_same(part, tt),
                "track {lane}: the take holds theirs' notes"
            );
        }
    }

    // One line or detail per item.
    let count = |what: &str| {
        report
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Conflict && i.what == what)
            .map(|i| i.count)
            .sum::<u64>()
    };
    if !heuristic {
        assert_eq!(
            count(text::CONFLICT_TAKE),
            e.takes.len() as u64,
            "one take line per track: {:#?}",
            report.items
        );
    }
    let mut details: Vec<&String> = report
        .items
        .iter()
        .filter(|i| i.kind == ItemKind::Conflict && i.what == text::CONFLICT_SETTING)
        .flat_map(|i| &i.details)
        .collect();
    for want in &e.settings {
        let at = details.iter().position(|d| d.ends_with(want.as_str()));
        assert!(at.is_some(), "a line for '{want}': {:#?}", report.items);
        if let Some(at) = at {
            details.swap_remove(at);
        }
    }
    assert!(
        count(text::DELETED_HERE) >= e.deleted_here,
        "{} deleted here: {:#?}",
        e.deleted_here,
        report.items
    );
    assert!(
        count(text::DELETED_THERE) + count(text::KEPT_EMPTY) >= e.kept_there,
        "{} kept: {:#?}",
        e.kept_there,
        report.items
    );
}

impl Side {
    fn strip_of(p: &Projection, i: Option<usize>) -> Option<&PStrip> {
        p.strips.get(i?)
    }
}

/// Track `lane`'s events on its active take: its audio regions when
/// `regions`, else its notes and MIDI events (not its automation).
fn events_json(doc: &Document, seq: SeqId, lane: u64, regions: bool) -> String {
    let events: Vec<_> = doc
        .lane(seq, LaneId(lane))
        .ok()
        .and_then(|l| l.active())
        .map(|t| {
            t.events
                .values()
                .filter(|e| match e.body {
                    EventBody::AudioRegion { .. } => regions,
                    EventBody::Automation { .. } => false,
                    _ => !regions,
                })
                .collect()
        })
        .unwrap_or_default();
    serde_json::to_string(&events).expect("serialises")
}

/// The takes of track `lane` that `merged` has and `ours` has not, named
/// "Returned …" (Ruling 6).
fn returned_takes(ours: &Document, merged: &Document, seq: SeqId, lane: u64) -> Vec<TakeId> {
    let had: BTreeSet<TakeId> = ours
        .lane(seq, LaneId(lane))
        .map(|l| l.takes.iter().map(|t| t.id).collect())
        .unwrap_or_default();
    merged
        .lane(seq, LaneId(lane))
        .map(|l| {
            l.takes
                .iter()
                .filter(|t| !had.contains(&t.id) && t.name.starts_with("Returned "))
                .map(|t| t.id)
                .collect()
        })
        .unwrap_or_default()
}

/// `merged`'s projection with `take` the active take of track `lane`, as
/// the take holds it: without the track's own loop, length and play
/// quantize (the take holds theirs' notes as they played, Ruling 6).
fn take_side(merged: &Document, snap: &Snapshot, lane: LaneId, take: TakeId) -> Side {
    let seq = SeqId(snap.seq);
    let mut h = History::new(merged.clone());
    for cmd in [
        LaneCmd::SetActiveTake { seq, lane, take },
        LaneCmd::SetLength {
            seq,
            lane,
            length: None,
            looped: false,
            locked: false,
        },
        LaneCmd::SetPlayQuantize {
            seq,
            lane,
            quantize: None,
        },
    ] {
        // A command that changes nothing is refused; that is fine.
        let _ = h.execute(&Command::Lane(cmd), &ViewFilter::default());
    }
    assert!(
        h.doc().lane(seq, lane).is_ok_and(|l| l.active_take == take),
        "activates the take"
    );
    Side::of(h.doc(), snap).expect("a projection")
}

/// The sequences track `lane` of `seq` plays through its uses, nested
/// uses included.
fn played_by(doc: &Document, seq: SeqId, lane: LaneId) -> BTreeSet<SeqId> {
    let mut out = BTreeSet::new();
    let Ok(l) = doc.lane(seq, lane) else {
        return out;
    };
    let mut todo: Vec<SeqId> = l
        .active()
        .into_iter()
        .flat_map(|t| t.events.values())
        .filter_map(|e| match e.body {
            EventBody::Reference { seq, .. } => Some(seq),
            _ => None,
        })
        .collect();
    while let Some(s) = todo.pop() {
        if out.insert(s) {
            todo.extend(played_sequences(doc, s));
        }
    }
    out
}

/// Spec §7.8's keep list is byte-equal in `merged` and `ours`, apart from
/// what a merge changes by design, each checked against theirs instead:
///
/// - a sequence played through a use that the merge edited (or made as a
///   private copy, Ruling 8): every track playing it plays theirs' notes;
/// - a conflict take the merge added (Ruling 6): each part it holds is
///   theirs'.
#[track_caller]
fn vizdaw_only_kept(p: &MergePlan, ours: &Document, merged: &Document) {
    let snap = &p.base;
    let seq = SeqId(snap.seq);
    let json = |d: &Document, s: SeqId| d.sequence(s).ok().map(|x| serde_json::to_string(x).ok());
    let played = played_sequences(merged, seq);
    let seqs: BTreeSet<SeqId> = merged
        .sequences()
        .keys()
        .copied()
        .filter(|s| *s != seq && json(merged, *s) != json(ours, *s))
        .filter(|s| played.contains(s))
        .collect();
    let (Some(o), Some(m)) = (Side::of(ours, snap), Side::of(merged, snap)) else {
        panic!("projections");
    };
    // Theirs' part for ours' part `key`, through the plan's matching.
    let theirs_of = |key: (u64, bool)| -> Option<&PTrack> {
        let oi =
            o.p.tracks
                .iter()
                .zip(&o.ids.tracks)
                .position(|(t, i)| i.lane == key.0 && t.audio == key.1)?;
        let row = p.matches.tracks.iter().find(|r| r.ours == Some(oi))?;
        p.theirs.projection.tracks.get(row.theirs?)
    };
    let lanes: Vec<LaneId> = merged
        .sequence(seq)
        .map(|s| s.lanes.iter().map(|l| l.id).collect())
        .unwrap_or_default();
    for s in &seqs {
        for lane in lanes
            .iter()
            .filter(|l| played_by(merged, seq, **l).contains(s))
        {
            let key = (lane.0, false);
            let (Some(mt), Some(tt)) = (m.part(key), theirs_of(key)) else {
                continue;
            };
            assert!(
                notes_same(mt, tt),
                "track {}: an edited sequence {s:?} plays theirs' notes",
                lane.0
            );
        }
    }
    let mut takes = BTreeSet::new();
    for lane in &lanes {
        for take in returned_takes(ours, merged, seq, lane.0) {
            takes.insert(take);
            let tk = take_side(merged, snap, *lane, take);
            for audio in [false, true] {
                let (Some(part), Some(tt)) = (tk.part((lane.0, audio)), theirs_of((lane.0, audio)))
                else {
                    continue;
                };
                if !part.notes_played.is_empty() || !part.midi.is_empty() {
                    assert!(notes_same(part, tt), "track {}: the take's notes", lane.0);
                }
                if !part.regions.is_empty() {
                    assert!(
                        regions_same(part, tt),
                        "track {}: the take's regions",
                        lane.0
                    );
                }
            }
        }
    }
    let e = Except { seqs, takes };
    assert_eq!(
        vizdaw_only_except(merged, seq, &e),
        vizdaw_only_except(ours, seq, &e),
        "VizDAW-only state"
    );
}

/// `plan` applied in one transaction on a history of `doc`.
fn merged(doc: &Document, p: &MergePlan) -> (History, Report) {
    let mut h = History::new(doc.clone());
    let mut report = Report::default();
    h.begin(LABEL).expect("begins");
    apply_merge(&mut h, p, DATE, &mut report).expect("applies");
    h.commit().expect("commits");
    (h, report)
}

fn check_script(s: &Spec, theirs_script: &[TheirEdit], ours_script: &[OurEdit], strip: Strip) {
    let (doc, seq) = build(s);
    let gain = if s.gain {
        RegionGain::Apply
    } else {
        RegionGain::LeaveOut
    };
    let t = trip_with(&doc, seq, gain);
    let mut did = TheirsDid::default();
    let path = return_file(&t, strip, |p, _| {
        let before = top_tracks(p);
        for e in theirs_script {
            apply_their_edit(p, e);
        }
        let after = top_tracks(p);
        did.added = after
            .keys()
            .filter(|id| !before.contains_key(*id))
            .cloned()
            .collect();
        for (id, name) in &before {
            match after.get(id) {
                None => {
                    did.deleted.insert(id.clone());
                }
                Some(n) if n != name => {
                    did.renamed.insert(id.clone(), n.clone());
                }
                Some(_) => {}
            }
        }
    });
    let ours = ours_after(&doc, seq, ours_script);
    let chosen = strip == Strip::Both;
    let p = plan(
        t.snapshot.clone(),
        theirs_of(&t, &path),
        &ours,
        chosen,
        &never(),
    )
    .expect("plans");
    counts_are_the_decisions(&p);
    let before = to_json(&ours).unwrap();

    let (mut h, report) = merged(&ours, &p);
    let problems = validate(h.doc());
    assert!(problems.is_empty(), "{problems:?}");
    vizdaw_only_kept(&p, &ours, h.doc());
    // Theirs as imported, never the plan's own view of it (fix round 4).
    let raw = theirs_of(&t, &path);
    let held = held_in(&raw, h.doc(), seq, t.snapshot.origin);
    owner_rules(
        &t,
        &did,
        strip,
        [&raw.projection, &held],
        &ours,
        h.doc(),
        &report,
    );
    let after = to_json(h.doc()).unwrap();

    // Merging the same file again adds nothing.
    let again = plan(
        t.snapshot.clone(),
        theirs_of(&t, &path),
        h.doc(),
        chosen,
        &never(),
    )
    .expect("plans again");
    let (h2, _) = merged(h.doc(), &again);
    assert_eq!(h2.undo_depth(), 0, "{:#?}", again.decisions);
    assert_eq!(to_json(h2.doc()).unwrap(), after, "{:#?}", again.decisions);

    // One undo gives ours back exactly.
    if after != before {
        assert_eq!(h.undo_label(), Some(LABEL));
        assert_eq!(h.undo_depth(), 1);
        h.undo().expect("undoes");
        assert_eq!(to_json(h.doc()).unwrap(), before, "one undo restores ours");
    } else {
        assert_eq!(
            h.undo_depth(),
            0,
            "a merge that changes nothing adds no step"
        );
    }
}

fn strips() -> impl Strategy<Value = Strip> {
    prop_oneof![
        Just(Strip::Nothing),
        Just(Strip::Record),
        Just(Strip::Tags),
        Just(Strip::Both),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(common::cases(64)))]

    #[test]
    fn edit_scripts_never_panic_validate_and_undo_once(
        s in spec(),
        theirs_script in proptest::collection::vec(their_edit(), 0..=8),
        ours_script in proptest::collection::vec(our_edit(), 0..=8),
        strip in strips(),
    ) {
        check_script(&s, &theirs_script, &ours_script, strip);
    }
}

// ---------------------------------------------------------------- hostile records

/// The plan for package bytes `bytes` (a return of `t`) against `ours`,
/// with `snap` as base; `None` when the file is refused.
fn plan_bytes(
    t: &Trip,
    bytes: &[u8],
    snap: Snapshot,
    ours: &Document,
    chosen: bool,
) -> Result<MergePlan, IoError> {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("Return.dawproject");
    std::fs::write(&path, bytes).expect("writes");
    plan_path(t, &path, snap, ours, chosen)
}

/// Each of `pkg`'s audio files as the media `t`'s export made it, else
/// offline (a damaged file may name others).
fn media_or_offline(
    pkg: &core_io::dawproject::package::DpPackage,
    t: &Trip,
) -> BTreeMap<usize, MediaSource> {
    use core_io::dawproject::package::AudioSource;
    pkg.audio()
        .iter()
        .map(|a| {
            let found = match &a.source {
                AudioSource::Entry(entry) => t.export.audio.iter().find(|f| &f.entry == entry),
                _ => None,
            };
            let item = found.and_then(|f| t.base_doc.media().get(&f.media).map(|m| (f, m)));
            let source = match item {
                Some((f, m)) => MediaSource::Online(vec![core_media::import::Imported {
                    path: format!("Media/{}.caf", a.name),
                    name: f.name.clone(),
                    original_name: m.name.clone(),
                    channels: m.channels,
                    sample_rate: m.sample_rate,
                    length_frames: m.length_frames,
                    content_hash: m.content_hash.clone().unwrap_or_default(),
                    clipped: 0,
                }]),
                None => MediaSource::Offline {
                    why: "unreadable".into(),
                },
            };
            (a.key, source)
        })
        .collect()
}

/// Opens, maps (the snapshot's IDs trusted when the record names its
/// export), makes theirs and plans, as the merge job does; any stage may
/// refuse.
fn plan_path(
    t: &Trip,
    path: &Path,
    snap: Snapshot,
    ours: &Document,
    chosen: bool,
) -> Result<MergePlan, IoError> {
    let cancel = never();
    let pkg = open(path, &PackageLimits::default(), &cancel)?;
    let media = media_or_offline(&pkg, t);
    let trusted = pkg
        .ret
        .record
        .as_ref()
        .is_some_and(|r| r.export_id == snap.export_id && r.project_id == snap.project_id);
    let installed = |_: PluginFormat, _: &str| true;
    let format = ours.settings().format;
    let imp = map(
        &pkg,
        &media,
        &ImportEnv {
            installed: &installed,
            plist: &TestPlist,
            rate: format.sample_rate,
            ids: trusted.then_some(&snap.ids),
        },
        &cancel,
    )?;
    let th = theirs(imp, format, snap.origin, &cancel)?;
    plan(snap, th, ours, chosen, &cancel)
}

/// Theirs' file of the owner's check: Bass's first note a beat later,
/// Bass at −3 dB, a marker "Bridge".
fn owners_check(p: &mut DpProject) {
    if let Some(n) = all_notes(p).into_iter().next() {
        n.time += 1.0;
    }
    if let Some(v) = all_channels(p)
        .into_iter()
        .find(|c| c.named.id.as_deref() == Some("c1"))
        .and_then(|c| c.volume.as_deref_mut())
    {
        v.value = Some(10f64.powf(-3.0 / 20.0));
    }
    if let Some(a) = p.arrangement.as_mut() {
        a.markers.push(DpMarker {
            named: DpNamed {
                name: Some("Bridge".into()),
                ..DpNamed::default()
            },
            time: 32.0,
        });
    }
}

/// The merge applies and validates, and keeps ours' VizDAW-only state.
#[track_caller]
fn merges(p: &MergePlan, ours: &Document) -> History {
    counts_are_the_decisions(p);
    let (h, _) = merged(ours, p);
    let problems = validate(h.doc());
    assert!(problems.is_empty(), "{problems:?}");
    vizdaw_only_kept(p, ours, h.doc());
    h
}

fn has(r: &Report, what: &str) -> bool {
    r.items.iter().any(|i| i.what == what)
}

fn track_ref(lane: u64) -> ObjectRef {
    ObjectRef {
        kind: "track".into(),
        id: lane,
        slot: None,
        index: None,
    }
}

/// The XML ID the snapshot gives track `lane`.
fn xml_id_of(snap: &Snapshot, lane: u64) -> String {
    snap.ids
        .iter()
        .find(|(_, r)| r.kind == "track" && r.id == lane)
        .map(|(k, _)| k.clone())
        .expect("the track's ID")
}

#[test]
fn hostile_records() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let ident = t.snapshot.identity.clone().expect("format 2");
    let bass = ident.tracks[0].lane;
    let keys = ident.tracks[1].lane;
    let edited = |strip: Strip| {
        let path = return_file(&t, strip, |p, _| owners_check(p));
        std::fs::read(path).expect("reads")
    };

    // A malformed record: 6d's line, then the tags match.
    let mut x = t.export.clone();
    x.record_json = "{ \"format\": 1, not json".into();
    {
        let mut report = Report::default();
        let mut project = core_io::dawproject::read_project(
            &core_io::xml::Tree::parse(
                x.project_xml.as_bytes(),
                &PackageLimits::default(),
                &never(),
            )
            .unwrap(),
            &mut report,
        )
        .unwrap();
        owners_check(&mut project);
        x.project_xml = core_io::dawproject::write_project(&project);
    }
    let p = plan_bytes(
        &t,
        &package_bytes(&x).unwrap(),
        t.snapshot.clone(),
        &t.base_doc,
        false,
    )
    .expect("merges by its tags");
    assert!(has(&p.report, RECORD_UNREADABLE), "{:#?}", p.report.items);
    assert!(!has(&p.report, text::BY_NAME), "matched by tags");
    assert_eq!(p.preview.applied, 2, "{:#?}", p.decisions);
    assert_eq!(p.preview.added, 1, "{:#?}", p.decisions);
    merges(&p, &t.base_doc);

    // Snapshot IDs naming a track ours no longer has: deleted here.
    let ours = {
        let mut h = History::new(t.base_doc.clone());
        h.execute(
            &Command::Lane(LaneCmd::Remove {
                seq,
                lane: LaneId(bass),
            }),
            &ViewFilter::default(),
        )
        .unwrap();
        h.doc().clone()
    };
    let p = plan_bytes(
        &t,
        &edited(Strip::Nothing),
        t.snapshot.clone(),
        &ours,
        false,
    )
    .expect("merges");
    assert!(
        p.decisions
            .iter()
            .any(|d| d.what == "Track 'Bass'" && d.outcome == Outcome::DeletedHere),
        "{:#?}",
        p.decisions
    );
    let h = merges(&p, &ours);
    assert!(h.doc().lane(seq, LaneId(bass)).is_err(), "still deleted");

    // An ID of the wrong kind is ignored and counted; the tag matches.
    let mut snap = t.snapshot.clone();
    let bass_id = xml_id_of(&snap, bass);
    snap.ids.insert(
        bass_id.clone(),
        ObjectRef {
            kind: "strip".into(),
            ..track_ref(bass)
        },
    );
    let p = plan_bytes(&t, &edited(Strip::Nothing), snap, &t.base_doc, false).expect("merges");
    assert!(has(&p.report, TAGS_IGNORED), "{:#?}", p.report.items);
    assert_eq!(p.preview.applied, 2, "{:#?}", p.decisions);
    merges(&p, &t.base_doc);

    // Two IDs naming one track: the first (in the file) keeps it, the
    // second is ignored and counted; nothing is duplicated.
    let mut snap = t.snapshot.clone();
    let keys_id = xml_id_of(&snap, keys);
    snap.ids.insert(keys_id, track_ref(bass));
    let p = plan_bytes(&t, &edited(Strip::Nothing), snap, &t.base_doc, false).expect("merges");
    assert!(has(&p.report, TAGS_IGNORED), "{:#?}", p.report.items);
    // Which kept it (review Minor): Bass's own track, first in the file;
    // Keys pairs by its tag.
    let theirs_name = |base_row: usize| {
        let r = p
            .matches
            .tracks
            .iter()
            .find(|r| r.base == Some(base_row))
            .expect("a row");
        p.theirs.projection.tracks[r.theirs.expect("matched")]
            .name
            .clone()
    };
    assert_eq!(theirs_name(0), "Bass", "{:#?}", p.matches);
    assert_eq!(theirs_name(1), "Keys", "{:#?}", p.matches);
    let h = merges(&p, &t.base_doc);
    let notes = |doc: &Document, lane: u64| events_json(doc, seq, lane, false);
    assert_ne!(
        notes(h.doc(), bass),
        notes(&t.base_doc, bass),
        "theirs' edit is Bass's"
    );
    assert_eq!(
        notes(h.doc(), keys),
        notes(&t.base_doc, keys),
        "Keys is untouched"
    );
    let lanes: Vec<String> = h
        .doc()
        .sequence(seq)
        .unwrap()
        .lanes
        .iter()
        .map(|l| l.name.clone())
        .collect();
    assert_eq!(lanes, vec!["Bass".to_owned(), "Keys".to_owned()]);

    // A huge ID map, at lowered limits: 1 000 entries read and merge,
    // 1 001 are damaged. (The real limit is Task 1's asserted
    // `MAX_XML_NODES`.)
    let limits = SnapshotLimits {
        max_ids: 1_000,
        ..SnapshotLimits::default()
    };
    let padded = |n: usize| {
        let mut v: serde_json::Value = serde_json::from_str(&t.export.snapshot_json).unwrap();
        let ids = v["ids"].as_object_mut().unwrap();
        let mut i = 0usize;
        while ids.len() < n {
            ids.insert(
                format!("x{i}"),
                serde_json::to_value(track_ref(1_000_000 + i as u64)).unwrap(),
            );
            i += 1;
        }
        serde_json::to_vec(&v).unwrap()
    };
    let (r, seen) = measure(|| {
        let snap = read_snapshot_with(&padded(1_000), &limits).expect("1 000 IDs read");
        assert_eq!(snap.ids.len(), 1_000);
        let p = plan_bytes(&t, &edited(Strip::Nothing), snap, &t.base_doc, false).expect("merges");
        merges(&p, &t.base_doc);
    });
    let () = r;
    assert!(seen.peak < 256 * MIB, "peak {} bytes", seen.peak);
    assert_eq!(
        read_snapshot_with(&padded(1_001), &limits).map(|_| ()),
        Err(IoError::Refused(SNAPSHOT_DAMAGED.to_owned()))
    );
}

// ---------------------------------------------------------------- byte damage

/// One of the three returned packages: its trip, ours, whether the user
/// chose the snapshot, the package's bytes and the snapshot's.
struct Case {
    t: Trip,
    ours: Document,
    chosen: bool,
    package: Vec<u8>,
}

/// The three packages, built once: the owner's check on the fixture (as
/// VizDAW wrote it); the fixture with Keys deleted and Bass at −3 dB, the
/// record and tags stripped (the snapshot chosen); the shared fixture with
/// a note of the looped use moved, its record stripped.
fn build_cases() -> Vec<Case> {
    {
        let (doc, seq) = fixture();
        let one = {
            let t = trip(&doc, seq);
            let x = edited_export(&t, owners_check);
            Case {
                package: stripped_package(&x, Strip::Nothing),
                ours: t.base_doc.clone(),
                chosen: false,
                t,
            }
        };
        let two = {
            let t = trip(&doc, seq);
            let x = edited_export(&t, |p| {
                let id = plain_tracks(p)
                    .into_iter()
                    .find(|t| t.named.name.as_deref() == Some("Keys"))
                    .and_then(|t| t.named.id.clone());
                if let Some(id) = id {
                    p.structure.retain(|x| {
                        !matches!(x, DpStructureItem::Track(t) if t.named.id.as_deref() == Some(&id))
                    });
                }
                apply_their_edit(p, &TheirEdit::Volume(0, -3));
            });
            Case {
                package: stripped_package(&x, Strip::Both),
                ours: t.base_doc.clone(),
                chosen: true,
                t,
            }
        };
        let three = {
            let (doc, seq) = shared_fixture();
            let t = trip(&doc, seq);
            let x = edited_export(&t, |p| {
                apply_their_edit(p, &TheirEdit::MoveNote(3, 2));
            });
            Case {
                package: stripped_package(&x, Strip::Record),
                ours: t.base_doc.clone(),
                chosen: false,
                t,
            }
        };
        vec![one, two, three]
    }
}

thread_local! {
    /// [`build_cases`], once per test thread (a `Document` is not `Sync`).
    static CASES: Vec<Case> = build_cases();
}

/// `t`'s export with its project edited by `edit`.
fn edited_export(
    t: &Trip,
    edit: impl FnOnce(&mut DpProject),
) -> core_io::dawproject::export::DpExport {
    let mut report = Report::default();
    let mut project = core_io::dawproject::read_project(
        &core_io::xml::Tree::parse(
            t.export.project_xml.as_bytes(),
            &PackageLimits::default(),
            &never(),
        )
        .unwrap(),
        &mut report,
    )
    .unwrap();
    edit(&mut project);
    let mut x = t.export.clone();
    x.project_xml = core_io::dawproject::write_project(&project);
    x
}

#[derive(Debug, Clone)]
enum Damage {
    Flip(usize, u8),
    Insert(usize, Vec<u8>),
    Delete(usize, usize),
    Truncate(usize),
}

fn damage() -> impl Strategy<Value = Damage> {
    prop_oneof![
        (any::<usize>(), 1u8..=255).prop_map(|(at, x)| Damage::Flip(at, x)),
        (
            any::<usize>(),
            proptest::collection::vec(any::<u8>(), 1..16)
        )
            .prop_map(|(at, b)| Damage::Insert(at, b)),
        (any::<usize>(), 1usize..64).prop_map(|(at, n)| Damage::Delete(at, n)),
        any::<usize>().prop_map(Damage::Truncate),
    ]
}

fn damaged(bytes: &[u8], d: &Damage) -> Vec<u8> {
    let mut b = bytes.to_vec();
    let n = b.len().max(1);
    match d {
        Damage::Flip(at, x) => {
            if let Some(v) = b.get_mut(at % n) {
                *v ^= x;
            }
        }
        Damage::Insert(at, extra) => {
            let at = at % (b.len() + 1);
            b.splice(at..at, extra.iter().copied());
        }
        Damage::Delete(at, k) => {
            let at = at % n;
            let end = (at + k).min(b.len());
            b.drain(at.min(b.len())..end);
        }
        Damage::Truncate(at) => b.truncate(at % n),
    }
    b
}

/// One damaged case: refused with a sentence, or merged (applied and
/// valid, or refused by the model, the transaction aborted).
fn survive(case: &Case, package: &[u8], snapshot: &[u8]) {
    let t = &case.t;
    let snap = match read_snapshot(snapshot) {
        Ok(s) => s,
        Err(e) => {
            assert!(
                matches!(e, IoError::Refused(ref s) if !s.is_empty()),
                "{e:?}"
            );
            return;
        }
    };
    let p = match plan_bytes(t, package, snap, &case.ours, case.chosen) {
        Ok(p) => p,
        Err(e) => {
            assert!(
                matches!(&e, IoError::Refused(s) | IoError::Io(s) if !s.is_empty()),
                "{e:?}"
            );
            return;
        }
    };
    let mut h = History::new(case.ours.clone());
    h.begin(LABEL).expect("begins");
    match apply_merge(&mut h, &p, DATE, &mut Report::default()) {
        Ok(()) => {
            h.commit().expect("commits");
            let problems = validate(h.doc());
            assert!(problems.is_empty(), "{problems:?}");
        }
        Err(e) => {
            assert!(!e.to_string().is_empty());
            h.abort().expect("aborts");
            assert_eq!(
                to_json(h.doc()).unwrap(),
                to_json(&case.ours).unwrap(),
                "an aborted merge leaves ours"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn damaged_returns_and_snapshots_never_crash(
        which in 0usize..3,
        on_snapshot in any::<bool>(),
        d in damage(),
    ) {
        let (took, seen) = CASES.with(|cases| {
            let case = &cases[which];
            let snapshot = case.t.export.snapshot_json.as_bytes();
            let (package, snapshot) = if on_snapshot {
                (case.package.clone(), damaged(snapshot, &d))
            } else {
                (damaged(&case.package, &d), snapshot.to_vec())
            };
            let start = Instant::now();
            let ((), seen) = measure(|| survive(case, &package, &snapshot));
            (start.elapsed(), seen)
        });
        prop_assert!(took.as_secs_f64() < 2.0, "{took:?}");
        prop_assert!(seen.peak < 256 * MIB, "peak {} bytes", seen.peak);
    }
}

/// The three packages merge undamaged (so the damage tests start from
/// files that merge).
#[test]
fn the_fuzzed_returns_merge_undamaged() {
    for (i, case) in build_cases().iter().enumerate() {
        let snap = read_snapshot(case.t.export.snapshot_json.as_bytes()).unwrap();
        let p = plan_bytes(&case.t, &case.package, snap, &case.ours, case.chosen)
            .unwrap_or_else(|e| panic!("case {i}: {e}"));
        assert!(!p.decisions.is_empty(), "case {i} changes something");
        merges(&p, &case.ours);
    }
}
