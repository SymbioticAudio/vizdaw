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
use core_io::dawproject::merge::decide::{MergePlan, Outcome, StripField, Unit, plan};
use core_io::dawproject::merge::text;
use core_io::dawproject::merge::theirs::theirs;
use core_io::dawproject::package::{RECORD_UNREADABLE, open};
use core_io::dawproject::record::{
    ObjectRef, SNAPSHOT_DAMAGED, Snapshot, SnapshotLimits, projection_with_identity, read_snapshot,
    read_snapshot_with,
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
use core_model::ids::{EventId, LaneId, SeqId, StripId};
use core_model::mixer::PluginFormat;
use core_model::rules::validate;
use core_model::selection::ViewFilter;
use core_model::sequence::Marker;
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::Tick;
use proptest::prelude::*;
use std::collections::BTreeMap;
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
        (40u16..=240).prop_map(OurEdit::Tempo),
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

/// What ours holds that the merge must keep where VizDAW wins: each
/// strip's level, pan and mute, and each track's name, by identity, for
/// the rows a decision kept as ours (or in conflict).
fn ours_wins(p: &MergePlan, ours: &Document) -> Vec<(String, String)> {
    let seq = SeqId(p.base.seq);
    let Ok((_, ids)) = projection_with_identity(ours, seq) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for d in &p.decisions {
        if !matches!(d.outcome, Outcome::Ours | Outcome::Conflict) {
            continue;
        }
        match &d.unit {
            Unit::Strip(f @ (StripField::Level | StripField::Pan | StripField::Mute)) => {
                let Some(id) = p
                    .matches
                    .strips
                    .get(d.row)
                    .and_then(|r| r.ours)
                    .and_then(|o| ids.strips.get(o))
                    .and_then(|s| s.strip)
                else {
                    continue;
                };
                if let Ok(s) = ours.mixer().strip(StripId(id)) {
                    let v = match f {
                        StripField::Level => s.gain_db.to_string(),
                        StripField::Pan => s.pan.to_string(),
                        _ => s.mute.to_string(),
                    };
                    out.push((format!("{f:?} {id}"), v));
                }
            }
            Unit::TrackName => {
                let Some(lane) = p
                    .matches
                    .tracks
                    .get(d.row)
                    .and_then(|r| r.ours)
                    .and_then(|o| ids.tracks.get(o))
                    .map(|t| t.lane)
                else {
                    continue;
                };
                if let Ok(l) = ours.lane(seq, LaneId(lane)) {
                    out.push((format!("name {lane}"), l.name.clone()));
                }
            }
            _ => {}
        }
    }
    out
}

/// [`ours_wins`]'s values as they are in `doc`.
fn values_now(wins: &[(String, String)], doc: &Document, seq: SeqId) -> Vec<(String, String)> {
    wins.iter()
        .map(|(k, _)| {
            let mut w = k.split(' ');
            let (what, id) = (w.next().unwrap_or(""), w.next().unwrap_or("0"));
            let id: u64 = id.parse().unwrap_or(0);
            let v = match what {
                "name" => doc
                    .lane(seq, LaneId(id))
                    .map(|l| l.name.clone())
                    .unwrap_or_default(),
                f => doc
                    .mixer()
                    .strip(StripId(id))
                    .map(|s| match f {
                        "Level" => s.gain_db.to_string(),
                        "Pan" => s.pan.to_string(),
                        _ => s.mute.to_string(),
                    })
                    .unwrap_or_default(),
            };
            (k.clone(), v)
        })
        .collect()
}

/// Spec §7.8's keep list is byte-equal in `merged` and `ours`, apart from
/// what a merge changes by design: the sequences played through the
/// exported one's uses (Ruling 8 edits them or makes private copies) and
/// the conflict takes it adds (Ruling 6).
#[track_caller]
fn vizdaw_only_kept(ours: &Document, merged: &Document, seq: SeqId) {
    let mut seqs = played_sequences(ours, seq);
    seqs.extend(played_sequences(merged, seq));
    let e = Except {
        seqs,
        returned_takes: true,
    };
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
    let path = return_file(&t, strip, |p, _| {
        for e in theirs_script {
            apply_their_edit(p, e);
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
    let wins = ours_wins(&p, &ours);
    let before = to_json(&ours).unwrap();

    let (mut h, report) = merged(&ours, &p);
    let problems = validate(h.doc());
    assert!(problems.is_empty(), "{problems:?}");
    vizdaw_only_kept(&ours, h.doc(), seq);
    assert_eq!(
        values_now(&wins, h.doc(), seq),
        wins,
        "VizDAW's changes win"
    );
    if p.preview.conflicts > 0 {
        assert!(
            report.items.iter().any(|i| i.kind == ItemKind::Conflict),
            "every conflict is listed: {:#?}",
            report.items
        );
    }
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
    vizdaw_only_kept(ours, h.doc(), SeqId(p.base.seq));
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
    let h = merges(&p, &t.base_doc);
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
