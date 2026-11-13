//! The merge's decisions (plan 6e Task 5, Rulings 5, 7, 10, 11 and 15):
//! the rule table per unit, the preview's counts and the report's lines.
//!
//! Each test exports `common::merge::fixture()` (its "Bass" strip at
//! −6 dB), edits theirs in the returned file (`return_file`) and ours with
//! `History` commands on a copy of the exported document, then plans.

mod common;

use common::merge::{Strip, Trip, exported, fixture, return_file, theirs_of, trip, trip_with};
use common::package::{TestPlist, media_of};
use core_engine::fixtures::BAR;
use core_io::IoError;
use core_io::dawproject::export::RegionGain;
use core_io::dawproject::import::{ImportEnv, MediaSource, map};
use core_io::dawproject::merge::decide::{
    Decision, MergePlan, MergePreview, Outcome, StripField, Unit, plan,
};
use core_io::dawproject::merge::text::{
    self, BY_NAME, CONFLICT_SETTING, CONFLICT_TAKE, DELETED_HERE, DELETED_THERE, KEPT_EMPTY, LINES,
    LOOPS_UNROLLED, MERGED, NOTHING, OLD_EXPORT, PRIVATE_COPIES, SEQUENCE_GONE, SHARED_ELSEWHERE,
    SNAPSHOT_CHOSEN, TAKE_PARTS, VST_KEPT, summary,
};
use core_io::dawproject::merge::theirs::{Theirs, theirs};
use core_io::dawproject::package::open;
use core_io::dawproject::{
    DpChannel, DpClip, DpDeviceKind, DpMarker, DpNamed, DpNote, DpProject, DpStructureItem,
    DpTimeline, DpTrack,
};
use core_io::limits::PackageLimits;
use core_io::report::{ItemKind, Report};
use core_model::command::{
    AudioCmd, Command, EventCmd, LaneCmd, MixerCmd, SequenceCmd, SettingsCmd,
};
use core_model::document::{Document, SongInfo};
use core_model::history::History;
use core_model::ids::{EventId, LaneId, SeqId, StripId};
use core_model::lane::MonitorMode;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::mixer::PluginFormat;
use core_model::selection::ViewFilter;
use core_model::sequence::Marker;
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

// ---------------------------------------------------------------- helpers

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

/// Beats in a bar of 4/4.
const BAR_BEATS: f64 = 4.0;

/// Who the fixture's parts are (from the snapshot's identity).
struct Ids {
    seq: SeqId,
    bass: LaneId,
    keys: LaneId,
    bass_strip: StripId,
    keys_strip: StripId,
    reverb: StripId,
    /// Bass's first note (tick 0, key 36).
    bass_note: EventId,
    /// Keys audio's first region.
    keys_region: EventId,
}

fn ids(t: &Trip) -> Ids {
    let i = t.snapshot.identity.as_ref().expect("format 2");
    let strip = |n: usize| StripId(i.strips[n].strip.expect("a strip"));
    Ids {
        seq: t.seq,
        bass: LaneId(i.tracks[0].lane),
        keys: LaneId(i.tracks[1].lane),
        bass_strip: strip(0),
        keys_strip: strip(1),
        reverb: strip(3),
        bass_note: EventId(i.tracks[0].notes[0].event),
        keys_region: EventId(i.tracks[2].regions[0].event),
    }
}

/// An edit ours makes.
type OursEdit = dyn Fn(&mut History, &Ids);

fn run(h: &mut History, cmd: Command) {
    h.execute(&cmd, &ViewFilter::default()).expect("runs");
}

/// Ours: the exported document, edited by `edit`.
fn ours(t: &Trip, edit: impl FnOnce(&mut History, &Ids)) -> Document {
    let ids = ids(t);
    let mut h = History::new(t.base_doc.clone());
    edit(&mut h, &ids);
    h.doc().clone()
}

/// The plan for the file at `path` against `ours`.
fn plan_with(t: &Trip, path: &Path, ours: &Document, chosen: bool) -> MergePlan {
    let th = theirs_of(t, path);
    plan(t.snapshot.clone(), th, ours, chosen, &never()).expect("plans")
}

/// The plan for the file at `path` against `ours`, the snapshot not
/// chosen.
fn plan_of(t: &Trip, path: &Path, ours: &Document) -> MergePlan {
    plan_with(t, path, ours, false)
}

/// Each decision's unit, outcome and `what`, in order.
fn decided(p: &MergePlan) -> Vec<(Unit, Outcome, String)> {
    p.decisions
        .iter()
        .map(|d| (d.unit.clone(), d.outcome, d.what.clone()))
        .collect()
}

fn of(unit: Unit, outcome: Outcome, what: &str) -> (Unit, Outcome, String) {
    (unit, outcome, what.to_owned())
}

/// The one decision on `unit`.
#[track_caller]
fn decision<'a>(p: &'a MergePlan, unit: &Unit) -> &'a Decision {
    let found: Vec<_> = p.decisions.iter().filter(|d| &d.unit == unit).collect();
    assert_eq!(found.len(), 1, "{unit:?}: {:?}", decided(p));
    found[0]
}

/// The report item `what` (under `kind`).
#[track_caller]
fn item<'a>(r: &'a Report, kind: ItemKind, what: &str) -> &'a core_io::report::Item {
    r.items
        .iter()
        .find(|i| i.kind == kind && i.what == what)
        .unwrap_or_else(|| panic!("no {kind:?} '{what}' in {:#?}", r.items))
}

fn has(r: &Report, what: &str) -> bool {
    r.items.iter().any(|i| i.what == what)
}

// ------------------------------------------------- theirs: the file's parts

fn tracks(p: &mut DpProject) -> impl Iterator<Item = &mut DpTrack> {
    p.structure.iter_mut().filter_map(|i| match i {
        DpStructureItem::Track(t) => Some(t),
        DpStructureItem::Channel(_) => None,
    })
}

/// The channel with XML ID `id`.
fn channel<'a>(p: &'a mut DpProject, id: &str) -> &'a mut DpChannel {
    tracks(p)
        .filter_map(|t| t.channel.as_deref_mut())
        .find(|c| c.named.id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("no channel {id}"))
}

/// Sets channel `id`'s volume to `db`.
fn set_db(p: &mut DpProject, id: &str, db: f64) {
    let v = channel(p, id).volume.as_deref_mut().expect("a volume");
    v.value = Some(10f64.powf(db / 20.0));
}

/// The clips on the track with XML ID `id`.
fn clips<'a>(p: &'a mut DpProject, id: &str) -> &'a mut Vec<DpClip> {
    let lanes = p.arrangement.as_mut().unwrap().lanes.as_mut().unwrap();
    for item in &mut lanes.items {
        if let DpTimeline::Lanes(l) = item
            && l.head.track.as_deref() == Some(id)
        {
            for inner in &mut l.items {
                if let DpTimeline::Clips(c) = inner {
                    return &mut c.clips;
                }
            }
        }
    }
    panic!("no clips on {id}")
}

/// Bass's first note (in its first clip, which is no sequence use).
fn bass_note(p: &mut DpProject) -> &mut DpNote {
    let clip = &mut clips(p, "t0")[0];
    match clip.content.as_deref_mut() {
        Some(DpTimeline::Notes(n)) => &mut n.notes[0],
        other => panic!("not notes: {other:?}"),
    }
}

fn markers(p: &mut DpProject) -> &mut Vec<DpMarker> {
    &mut p.arrangement.as_mut().unwrap().markers
}

/// Moves marker `name` to `beats`.
fn move_marker(p: &mut DpProject, name: &str, beats: f64) {
    let m = markers(p)
        .iter_mut()
        .find(|m| m.named.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no marker {name}"));
    m.time = beats;
}

/// Takes marker `name` out of the file (the other markers keep their
/// tags).
fn remove_marker(p: &mut DpProject, name: &str) {
    markers(p).retain(|m| m.named.name.as_deref() != Some(name));
}

/// Takes the track with XML ID `id`, and its timeline, out of the file.
fn remove_track(p: &mut DpProject, id: &str) {
    p.structure
        .retain(|i| !matches!(i, DpStructureItem::Track(t) if t.named.id.as_deref() == Some(id)));
    let lanes = p.arrangement.as_mut().unwrap().lanes.as_mut().unwrap();
    lanes
        .items
        .retain(|i| !matches!(i, DpTimeline::Lanes(l) if l.head.track.as_deref() == Some(id)));
}

/// The other DAW's new parts: a notes track "New" (a copy of Keys' first
/// clip, with no channel), a bus "Hall" and a marker "Bridge" at bar 9,
/// none of them tagged.
fn add_new_parts(p: &mut DpProject) {
    let mut new = tracks(p)
        .find(|t| t.named.id.as_deref() == Some("t1"))
        .unwrap()
        .clone();
    new.named = DpNamed {
        id: Some("tnew".into()),
        name: Some("New".into()),
        ..DpNamed::default()
    };
    new.channel = None;
    let mut bus = tracks(p)
        .find(|t| t.named.id.as_deref() == Some("b4"))
        .unwrap()
        .clone();
    bus.named = DpNamed {
        id: Some("bnew".into()),
        name: Some("Hall".into()),
        ..DpNamed::default()
    };
    let c = bus.channel.as_deref_mut().unwrap();
    c.named = DpNamed {
        id: Some("cnew".into()),
        name: Some("Hall".into()),
        ..DpNamed::default()
    };
    c.mute.as_deref_mut().unwrap().named.id = Some("cnewm".into());
    c.pan.as_deref_mut().unwrap().named.id = Some("cnewp".into());
    c.volume.as_deref_mut().unwrap().named.id = Some("cnewv".into());
    let at = p.structure.len() - 1;
    p.structure.insert(at, DpStructureItem::Track(bus));
    p.structure.insert(at, DpStructureItem::Track(new));
    let lanes = p.arrangement.as_mut().unwrap().lanes.as_mut().unwrap();
    let mut timeline = lanes
        .items
        .iter()
        .find_map(|i| match i {
            DpTimeline::Lanes(l) if l.head.track.as_deref() == Some("t1") => Some(l.clone()),
            _ => None,
        })
        .unwrap();
    timeline.head.track = Some("tnew".into());
    timeline.head.named = DpNamed::default();
    lanes.items.push(DpTimeline::Lanes(timeline));
    markers(p).push(DpMarker {
        named: DpNamed {
            name: Some("Bridge".into()),
            ..DpNamed::default()
        },
        time: 8.0 * BAR_BEATS,
    });
}

/// `doc`'s tempo map, written as the export writes it, in place of the
/// file's.
fn tempo_of(p: &mut DpProject, doc: &Document, seq: SeqId) {
    let (other, _) = exported(doc, seq);
    p.tempo = other.tempo.clone();
    p.arrangement.as_mut().unwrap().tempo_automation =
        other.arrangement.as_ref().unwrap().tempo_automation.clone();
}

/// `doc`'s meter map, as [`tempo_of`].
fn meter_of(p: &mut DpProject, doc: &Document, seq: SeqId) {
    let (other, _) = exported(doc, seq);
    p.time_signature = other.time_signature.clone();
    p.arrangement.as_mut().unwrap().time_signature_automation = other
        .arrangement
        .as_ref()
        .unwrap()
        .time_signature_automation
        .clone();
}

// ------------------------------------------------- ours: commands

fn set_gain(h: &mut History, strip: StripId, gain_db: f64) {
    run(h, Command::Mixer(MixerCmd::SetGain { strip, gain_db }));
}

fn set_pitch(h: &mut History, note: EventId, pitch: u8) {
    run(
        h,
        Command::Event(EventCmd::SetPitch {
            events: vec![note],
            pitch,
        }),
    );
}

fn set_markers(h: &mut History, seq: SeqId, markers: &[(&str, i64)]) {
    run(
        h,
        Command::Sequence(SequenceCmd::SetMarkers {
            seq,
            markers: markers
                .iter()
                .map(|(name, tick)| Marker {
                    tick: Tick(*tick),
                    name: (*name).into(),
                })
                .collect(),
        }),
    );
}

fn tempo(points: &[(i64, u32)]) -> TempoMap {
    TempoMap::new(
        points
            .iter()
            .map(|&(tick, bpm)| TempoPoint {
                tick: Tick(tick),
                bpm: Bpm(bpm * 1_000),
                ramp: false,
            })
            .collect(),
    )
    .unwrap()
}

fn set_tempo(h: &mut History, seq: SeqId, points: &[(i64, u32)]) {
    run(
        h,
        Command::Sequence(SequenceCmd::SetTempo {
            seq,
            tempo: tempo(points),
        }),
    );
}

fn set_info(h: &mut History, edit: impl FnOnce(&mut SongInfo)) {
    let mut info = h.doc().settings().info.clone();
    edit(&mut info);
    run(h, Command::Settings(SettingsCmd::SetInfo { info }));
}

/// A tick `bars` bars after bar 1 (4/4).
fn bar(bars: i64) -> i64 {
    bars * BAR
}

/// The three edits of the rule-table tests: the Bass strip's level, Bass's
/// first note's key and the Chorus marker's bar.
#[derive(Clone, Copy)]
struct Edit {
    db: f64,
    key: u8,
    chorus_bar: i64,
}

/// Theirs makes `e` in the file.
fn theirs_edit(e: Edit) -> impl FnOnce(&mut DpProject, &mut core_io::dawproject::DpMetaData) {
    move |p, _| {
        set_db(p, "c1", e.db);
        bass_note(p).key = i32::from(e.key);
        move_marker(p, "Chorus", (e.chorus_bar - 1) as f64 * BAR_BEATS);
    }
}

/// Ours makes `e` with commands.
fn ours_edit(e: Edit) -> impl FnOnce(&mut History, &Ids) {
    move |h, ids| {
        set_gain(h, ids.bass_strip, e.db);
        set_pitch(h, ids.bass_note, e.key);
        set_markers(
            h,
            ids.seq,
            &[("Verse", 0), ("Chorus", bar(e.chorus_bar - 1))],
        );
    }
}

/// The decisions on the three edited units, in decision order: Bass's
/// notes, the Bass strip's level, the Chorus marker.
fn three(outcome: Outcome) -> Vec<(Unit, Outcome, String)> {
    vec![
        of(Unit::Notes, outcome, "Track 'Bass': notes"),
        of(
            Unit::Strip(StripField::Level),
            outcome,
            "Strip 'Bass': level",
        ),
        of(Unit::Marker, outcome, "Marker 'Chorus'"),
    ]
}

/// The file with Bass (track and timeline), the Reverb bus and the
/// Chorus marker deleted (and `then`).
fn deleted_there(t: &Trip, then: impl FnOnce(&mut DpProject)) -> PathBuf {
    return_file(t, Strip::Nothing, |p, _| {
        remove_track(p, "t0");
        p.structure.retain(
            |i| !matches!(i, DpStructureItem::Track(t) if t.named.id.as_deref() == Some("b4")),
        );
        remove_marker(p, "Chorus");
        then(p);
    })
}

/// The decisions on the three deleted parts.
fn three_deleted(outcome: Outcome) -> Vec<(Unit, Outcome, String)> {
    vec![
        of(Unit::Track, outcome, "Track 'Bass'"),
        of(Unit::Bus, outcome, "Bus 'Reverb'"),
        of(Unit::Marker, outcome, "Marker 'Chorus'"),
    ]
}

// ---------------------------------------------------------------- the rule table

#[test]
fn theirs_changed_ours_unchanged_is_theirs() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let e = Edit {
        db: -3.0,
        key: 38,
        chorus_bar: 4,
    };
    let p = plan_of(
        &t,
        &return_file(&t, Strip::Nothing, theirs_edit(e)),
        &t.base_doc,
    );
    assert_eq!(decided(&p), three(Outcome::Theirs));
    let level = decision(&p, &Unit::Strip(StripField::Level));
    assert_eq!(level.ours.as_deref(), Some("\u{2212}6.0 dB"));
    assert_eq!(level.theirs.as_deref(), Some("\u{2212}3.0 dB"));
    assert_eq!(p.preview.applied, 3);
}

#[test]
fn ours_changed_theirs_unchanged_is_ours() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let e = Edit {
        db: -9.0,
        key: 38,
        chorus_bar: 4,
    };
    let o = ours(&t, ours_edit(e));
    let p = plan_of(&t, &return_file(&t, Strip::Nothing, |_, _| {}), &o);
    assert_eq!(decided(&p), three(Outcome::Ours));
    assert_eq!(p.preview.kept, 3);
}

#[test]
fn both_changed_the_same_way_is_applied_once() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let e = Edit {
        db: -3.0,
        key: 38,
        chorus_bar: 4,
    };
    let o = ours(&t, ours_edit(e));
    let p = plan_of(&t, &return_file(&t, Strip::Nothing, theirs_edit(e)), &o);
    assert_eq!(decided(&p), three(Outcome::Same));
    assert_eq!(
        p.preview,
        MergePreview {
            applied: 3,
            ..MergePreview::default()
        }
    );
}

#[test]
fn both_changed_differently_is_a_conflict() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let o = ours(
        &t,
        ours_edit(Edit {
            db: -9.0,
            key: 38,
            chorus_bar: 4,
        }),
    );
    let path = return_file(
        &t,
        Strip::Nothing,
        theirs_edit(Edit {
            db: -3.0,
            key: 41,
            chorus_bar: 5,
        }),
    );
    let p = plan_of(&t, &path, &o);
    assert_eq!(decided(&p), three(Outcome::Conflict));
    assert_eq!(
        decision(&p, &Unit::Notes).what,
        "Track 'Bass': notes",
        "the notes conflict as a whole"
    );
    let level = decision(&p, &Unit::Strip(StripField::Level));
    assert_eq!(level.ours.as_deref(), Some("\u{2212}9.0 dB"));
    assert_eq!(level.theirs.as_deref(), Some("\u{2212}3.0 dB"));
    let marker = decision(&p, &Unit::Marker);
    assert_eq!(marker.ours.as_deref(), Some("Chorus at bar 4"));
    assert_eq!(marker.theirs.as_deref(), Some("Chorus at bar 5"));

    // The settings are listed with both values; the notes become a take
    // when the merge is applied (Task 6), so the plan lists them in the
    // preview only.
    let settings = item(&p.report, ItemKind::Conflict, CONFLICT_SETTING);
    assert_eq!(
        settings.details,
        vec![
            "Strip 'Bass': level: here \u{2212}9.0 dB, there \u{2212}3.0 dB".to_owned(),
            "Marker 'Chorus': here Chorus at bar 4, there Chorus at bar 5".to_owned(),
        ]
    );
    assert!(!has(&p.report, CONFLICT_TAKE));
    assert_eq!(p.preview.conflicts, 3);
    assert_eq!(
        p.preview.conflict_lines,
        vec![
            "Track 'Bass': notes".to_owned(),
            "Strip 'Bass': level: here \u{2212}9.0 dB, there \u{2212}3.0 dB".to_owned(),
            "Marker 'Chorus': here Chorus at bar 4, there Chorus at bar 5".to_owned(),
        ]
    );
}

#[test]
fn deleted_there_unchanged_here_is_removed() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let p = plan_of(&t, &deleted_there(&t, |_| {}), &t.base_doc);
    assert_eq!(decided(&p), three_deleted(Outcome::Removed));
    assert_eq!(
        p.preview,
        MergePreview {
            removed: 3,
            ..MergePreview::default()
        }
    );
}

#[test]
fn deleted_there_changed_here_is_kept() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let o = ours(&t, |h, ids| {
        set_pitch(h, ids.bass_note, 38);
        set_gain(h, ids.reverb, -4.0);
        set_markers(h, ids.seq, &[("Verse", 0), ("Chorus", bar(3))]);
    });
    let p = plan_of(&t, &deleted_there(&t, |_| {}), &o);
    assert_eq!(decided(&p), three_deleted(Outcome::DeletedThere));
    let kept = item(&p.report, ItemKind::Conflict, DELETED_THERE);
    assert_eq!(
        kept.details,
        vec!["Track 'Bass'", "Bus 'Reverb'", "Marker 'Chorus'"]
    );
    assert_eq!(p.preview.conflicts, 3);
}

#[test]
fn ours_deleted_theirs_changed_stays_deleted() {
    // Review Focus 4: VizDAW's deletion wins, and is listed.
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let o = ours(&t, |h, ids| {
        run(
            h,
            Command::Lane(LaneCmd::Remove {
                seq: ids.seq,
                lane: ids.bass,
            }),
        );
        run(h, Command::Mixer(MixerCmd::RemoveBus { strip: ids.reverb }));
        set_markers(h, ids.seq, &[("Verse", 0)]);
    });
    let path = return_file(&t, Strip::Nothing, |p, _| {
        bass_note(p).key = 38;
        set_db(p, "c4", -4.0);
        move_marker(p, "Chorus", 3.0 * BAR_BEATS);
    });
    let p = plan_of(&t, &path, &o);
    assert_eq!(decided(&p), three_deleted(Outcome::DeletedHere));
    let left = item(&p.report, ItemKind::Conflict, DELETED_HERE);
    assert_eq!(
        left.details,
        vec!["Track 'Bass'", "Bus 'Reverb'", "Marker 'Chorus'"]
    );

    // Theirs unchanged: ours' deletions are ours' changes, kept.
    let p = plan_of(&t, &return_file(&t, Strip::Nothing, |_, _| {}), &o);
    assert_eq!(decided(&p), three_deleted(Outcome::Ours));
}

#[test]
fn a_split_track_deleted_here_and_unchanged_there_is_ours() {
    // Both parts of Keys, its audio part's strip named after the track in
    // theirs (the file does not carry that name).
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let o = ours(&t, |h, ids| {
        run(
            h,
            Command::Lane(LaneCmd::Remove {
                seq: ids.seq,
                lane: ids.keys,
            }),
        )
    });
    let p = plan_of(&t, &return_file(&t, Strip::Nothing, |_, _| {}), &o);
    assert_eq!(
        decided(&p),
        vec![
            of(Unit::Track, Outcome::Ours, "Track 'Keys'"),
            of(Unit::Track, Outcome::Ours, "Track 'Keys audio'"),
        ]
    );
}

#[test]
fn new_there_is_added() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, Strip::Nothing, |p, _| add_new_parts(p));
    let p = plan_of(&t, &path, &t.base_doc);
    assert_eq!(
        decided(&p),
        vec![
            of(Unit::Track, Outcome::Added, "Track 'New'"),
            of(Unit::Bus, Outcome::Added, "Bus 'Hall'"),
            of(Unit::Marker, Outcome::Added, "Marker 'Bridge'"),
        ]
    );
    assert_eq!(p.preview.added, 3);
}

#[test]
fn what_both_sides_added_is_added_once() {
    // Review Focus 2: after a merge ours holds theirs' new parts, so the
    // same file merged again adds nothing.
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, Strip::Nothing, |p, _| {
        add_new_parts(p);
        remove_track(p, "tnew");
    });
    let o = ours(&t, |h, ids| {
        run(
            h,
            Command::Mixer(MixerCmd::AddBus {
                name: "Hall".into(),
                index: None,
            }),
        );
        set_markers(
            h,
            ids.seq,
            &[("Verse", 0), ("Chorus", bar(2)), ("Bridge", bar(8))],
        );
    });
    let p = plan_of(&t, &path, &o);
    let added: Vec<_> = p
        .decisions
        .iter()
        .filter(|d| d.outcome == Outcome::Added)
        .collect();
    assert!(added.is_empty(), "{:?}", decided(&p));
    let marker = decision(&p, &Unit::Marker);
    assert_eq!(marker.outcome, Outcome::Same, "{:?}", decided(&p));
    let bus = decision(&p, &Unit::Bus);
    assert_eq!(bus.outcome, Outcome::Same, "{:?}", decided(&p));
}

// ---------------------------------------------------------------- further rules

#[test]
fn nothing_changed_decides_nothing() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    for strip in [Strip::Nothing, Strip::Record, Strip::Tags, Strip::Both] {
        let p = plan_of(&t, &return_file(&t, strip, |_, _| {}), &t.base_doc);
        assert_eq!(decided(&p), vec![], "{strip:?}");
        assert_eq!(p.preview, MergePreview::default(), "{strip:?}");
        assert_eq!(summary(&p.preview), NOTHING, "{strip:?}");
        let merged = item(&p.report, ItemKind::Note, &text::merged(1_700_000_000));
        assert_eq!(merged.count, 1);
    }
}

#[test]
fn a_deleted_sequence_is_refused() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let o = ours(&t, |h, ids| {
        run(
            h,
            Command::Sequence(SequenceCmd::Delete {
                seq: ids.seq,
                delete_references: true,
            }),
        );
    });
    let th = theirs_of(&t, &return_file(&t, Strip::Nothing, |_, _| {}));
    match plan(t.snapshot.clone(), th, &o, false, &never()) {
        Err(IoError::Refused(why)) => assert_eq!(why, SEQUENCE_GONE),
        Err(e) => panic!("{e:?}"),
        Ok(p) => panic!("planned: {:?}", decided(&p)),
    }
}

#[test]
fn a_cancelled_plan_stops() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let th = theirs_of(&t, &return_file(&t, Strip::Nothing, |_, _| {}));
    let cancel = AtomicBool::new(true);
    assert!(matches!(
        plan(t.snapshot.clone(), th, &t.base_doc, false, &cancel),
        Err(IoError::Cancelled)
    ));
}

#[test]
fn levels_within_tolerance_are_unchanged() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let near = return_file(&t, Strip::Nothing, |p, _| set_db(p, "c1", -6.0 + 0.04));
    let p = plan_of(&t, &near, &t.base_doc);
    assert_eq!(decided(&p), vec![], "+0.04 dB is no change");

    let far = return_file(&t, Strip::Nothing, |p, _| set_db(p, "c1", -6.0 + 0.06));
    let p = plan_of(&t, &far, &t.base_doc);
    assert_eq!(
        decided(&p),
        vec![of(
            Unit::Strip(StripField::Level),
            Outcome::Theirs,
            "Strip 'Bass': level"
        )]
    );
}

/// The fixture with Keys audio's first region at −6 dB.
fn with_region_gain() -> (Document, SeqId) {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let ids = ids(&t);
    let mut h = History::new(doc);
    run(
        &mut h,
        Command::Audio(AudioCmd::SetGain {
            event: ids.keys_region,
            gain_db: -6.0,
        }),
    );
    (h.doc().clone(), seq)
}

/// The file at `path` as theirs, every audio file offline (its regions
/// have no content hash).
fn theirs_offline(t: &Trip, path: &Path) -> Theirs {
    let pkg = open(path, &PackageLimits::default(), &never()).expect("opens");
    let media: std::collections::BTreeMap<usize, MediaSource> =
        media_of(&pkg, &t.export, &t.base_doc)
            .into_keys()
            .map(|k| {
                (
                    k,
                    MediaSource::Offline {
                        why: "unreadable".into(),
                    },
                )
            })
            .collect();
    let installed = |_: PluginFormat, _: &str| true;
    let format = t.base_doc.settings().format;
    let imp = map(
        &pkg,
        &media,
        &ImportEnv {
            installed: &installed,
            plist: &TestPlist,
            rate: format.sample_rate,
            ids: Some(&t.snapshot.ids),
        },
        &never(),
    )
    .expect("maps");
    theirs(imp, format, t.snapshot.origin, &never()).expect("theirs")
}

#[test]
fn region_gain_is_never_theirs() {
    // Ruling 11: the file has no clip gain, so its 0 dB is no change.
    let (doc, seq) = with_region_gain();
    for gain in [RegionGain::Apply, RegionGain::LeaveOut] {
        let t = trip_with(&doc, seq, gain);
        let p = plan_of(&t, &return_file(&t, Strip::Nothing, |_, _| {}), &t.base_doc);
        assert_eq!(decided(&p), vec![], "{gain:?}");

        // A format-1 snapshot has no gain option: Apply.
        let mut old = trip_with(&doc, seq, gain);
        old.snapshot.region_gain = None;
        if gain == RegionGain::Apply {
            let p = plan_of(
                &old,
                &return_file(&old, Strip::Nothing, |_, _| {}),
                &old.base_doc,
            );
            assert!(
                p.decisions.iter().all(|d| d.unit != Unit::Regions),
                "{:?}",
                decided(&p)
            );
        }
    }

    // Regions whose audio is offline in the scratch import have no hash.
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let th = theirs_offline(&t, &return_file(&t, Strip::Nothing, |_, _| {}));
    assert!(
        th.projection
            .tracks
            .iter()
            .flat_map(|x| &x.regions)
            .all(|r| r.media_hash.is_none()),
        "offline"
    );
    let p = plan(t.snapshot.clone(), th, &t.base_doc, false, &never()).expect("plans");
    assert!(
        p.decisions.iter().all(|d| d.unit != Unit::Regions),
        "{:?}",
        decided(&p)
    );
}

#[test]
fn regions_theirs_moved_are_theirs() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, Strip::Nothing, |p, _| {
        clips(p, "t1a")[1].time += 1.0;
    });
    let p = plan_of(&t, &path, &t.base_doc);
    assert_eq!(
        decided(&p),
        vec![of(
            Unit::Regions,
            Outcome::Theirs,
            "Track 'Keys audio': regions"
        )]
    );
}

#[test]
fn vst_replacement_is_kept_and_listed() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, Strip::Nothing, |p, _| {
        let c = channel(p, "c1");
        let d = c
            .devices
            .iter_mut()
            .find(|d| d.device_id.as_deref() == Some("org.example.Comp"))
            .unwrap();
        d.kind = DpDeviceKind::Vst3;
        d.device_id = Some("ABCDEF0123456789".into());
        d.device_name = "Fancy Comp".into();
        d.named.comment = None;
    });
    let p = plan_of(&t, &path, &t.base_doc);
    assert_eq!(
        decided(&p),
        vec![of(Unit::Devices, Outcome::Ours, "Strip 'Bass': plug-ins")]
    );
    let kept = item(&p.report, ItemKind::LeftOut, VST_KEPT);
    assert_eq!(kept.details, vec!["Fancy Comp"]);
    assert_eq!(p.preview.kept, 1);
}

#[test]
fn song_info_merges_field_by_field() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, Strip::Nothing, |_, m| {
        m.fields[0] = Some("Their Title".into());
    });
    let o = ours(&t, |h, _| {
        set_info(h, |i| i.artist = Some("Our Artist".into()))
    });
    let p = plan_of(&t, &path, &o);
    assert_eq!(
        decided(&p),
        vec![
            of(Unit::Info(0), Outcome::Theirs, "Song info: title"),
            of(Unit::Info(1), Outcome::Ours, "Song info: artist"),
        ]
    );
    assert_eq!(p.preview.conflicts, 0);
}

#[test]
fn tempo_and_meter_are_whole_maps() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let o = ours(&t, |h, ids| {
        set_tempo(h, ids.seq, &[(0, 120), (bar(16), 140)])
    });
    let mut there = t.base_doc.clone();
    {
        let mut h = History::new(there);
        set_tempo(&mut h, seq, &[(0, 120), (bar(8), 100)]);
        run(
            &mut h,
            Command::Sequence(SequenceCmd::SetMeter {
                seq,
                meter: MeterMap::new(vec![
                    MeterEntry {
                        tick: Tick(0),
                        meter: Meter { num: 4, den: 4 },
                        key: KeySig::default(),
                    },
                    MeterEntry {
                        tick: Tick(bar(12)),
                        meter: Meter { num: 3, den: 4 },
                        key: KeySig::default(),
                    },
                ])
                .unwrap(),
            }),
        );
        there = h.doc().clone();
    }
    let path = return_file(&t, Strip::Nothing, |p, _| {
        tempo_of(p, &there, seq);
        meter_of(p, &there, seq);
    });
    let p = plan_of(&t, &path, &o);
    assert_eq!(
        decided(&p),
        vec![
            of(Unit::Tempo, Outcome::Conflict, "Tempo map"),
            of(Unit::Meter, Outcome::Theirs, "Meter map"),
        ]
    );
    let tempo = decision(&p, &Unit::Tempo);
    assert_eq!(
        tempo.ours.as_deref(),
        Some("120 BPM at bar 1; 140 BPM at bar 17")
    );
    assert_eq!(
        tempo.theirs.as_deref(),
        Some("120 BPM at bar 1; 100 BPM at bar 9")
    );
}

#[test]
fn deleted_tracks_holding_what_the_file_cannot_carry_are_kept_empty() {
    // Ruling 7: the keep rule reads the lane in ours.
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, Strip::Nothing, |p, _| remove_track(p, "t1a"));
    let keeps: [(&str, Box<OursEdit>); 4] = [
        (
            "monitoring",
            Box::new(|h, ids| {
                run(
                    h,
                    Command::Lane(LaneCmd::SetMonitor {
                        seq: ids.seq,
                        lane: ids.keys,
                        monitor: MonitorMode::Input,
                    }),
                )
            }),
        ),
        (
            "an inactive take",
            Box::new(|h, ids| {
                let first = h.doc().lane(ids.seq, ids.keys).unwrap().active_take;
                run(
                    h,
                    Command::Lane(LaneCmd::AddTake {
                        seq: ids.seq,
                        lane: ids.keys,
                        name: "Take 2".into(),
                    }),
                );
                run(
                    h,
                    Command::Lane(LaneCmd::SetActiveTake {
                        seq: ids.seq,
                        lane: ids.keys,
                        take: first,
                    }),
                );
            }),
        ),
        (
            "its own length",
            Box::new(|h, ids| {
                run(
                    h,
                    Command::Lane(LaneCmd::SetLength {
                        seq: ids.seq,
                        lane: ids.keys,
                        length: Some(Tick(bar(8))),
                        looped: false,
                        locked: false,
                    }),
                )
            }),
        ),
        ("nothing", Box::new(|_, _| {})),
    ];
    for (why, edit) in keeps {
        let o = ours(&t, |h, ids| edit(h, ids));
        let p = plan_of(&t, &path, &o);
        let want = match why {
            "monitoring" | "an inactive take" | "its own length" => Outcome::KeptEmpty,
            _ => Outcome::Removed,
        };
        assert_eq!(
            decided(&p),
            vec![of(Unit::Track, want, "Track 'Keys audio'")],
            "{why}"
        );
        if want == Outcome::KeptEmpty {
            let kept = item(&p.report, ItemKind::Conflict, KEPT_EMPTY);
            assert_eq!(kept.details, vec!["Track 'Keys audio'"], "{why}");
        }
    }
}

// ---------------------------------------------------------------- preview and report

/// The fixture with four more markers, M1 to M4 at bars 5 to 8.
fn with_markers() -> (Document, SeqId) {
    let (doc, seq) = fixture();
    let mut h = History::new(doc);
    set_markers(
        &mut h,
        seq,
        &[
            ("Verse", 0),
            ("Chorus", bar(2)),
            ("M1", bar(4)),
            ("M2", bar(5)),
            ("M3", bar(6)),
            ("M4", bar(7)),
        ],
    );
    (h.doc().clone(), seq)
}

#[test]
fn preview_counts() {
    let (doc, seq) = with_markers();
    let t = trip(&doc, seq);
    let mut there = t.base_doc.clone();
    {
        let mut h = History::new(there);
        set_tempo(&mut h, seq, &[(0, 120), (bar(8), 100)]);
        there = h.doc().clone();
    }
    let path = return_file(&t, Strip::Nothing, |p, m| {
        set_db(p, "c1", -3.0); // Theirs
        m.fields[0] = Some("Their Title".into()); // Theirs
        bass_note(p).key = 38; // Same
        tempo_of(p, &there, seq); // Conflict
        remove_marker(p, "M1"); // Removed
        remove_track(p, "t1a"); // KeptEmpty
        remove_marker(p, "M2"); // DeletedThere
        move_marker(p, "M3", 6.0 * BAR_BEATS + 2.0); // DeletedHere
        markers(p).push(DpMarker {
            named: DpNamed {
                name: Some("Bridge".into()),
                ..DpNamed::default()
            },
            time: 8.0 * BAR_BEATS,
        }); // Added
    });
    let o = ours(&t, |h, ids| {
        run(
            h,
            Command::Mixer(MixerCmd::SetPan {
                strip: ids.keys_strip,
                pan: -0.5,
            }),
        ); // Ours
        set_info(h, |i| i.artist = Some("Our Artist".into())); // Ours
        set_pitch(h, ids.bass_note, 38); // Same
        set_tempo(h, ids.seq, &[(0, 120), (bar(16), 140)]); // Conflict
        run(
            h,
            Command::Lane(LaneCmd::SetMonitor {
                seq: ids.seq,
                lane: ids.keys,
                monitor: MonitorMode::Input,
            }),
        ); // KeptEmpty
        set_markers(
            h,
            ids.seq,
            &[
                ("Verse", 0),
                ("Chorus", bar(2)),
                ("M1", bar(4)),
                ("M2", bar(5) + PPQ * 2), // DeletedThere
                ("M4", bar(7)),           // M3: DeletedHere
            ],
        );
    });
    let p = plan_of(&t, &path, &o);
    assert_eq!(
        decided(&p),
        vec![
            of(Unit::Notes, Outcome::Same, "Track 'Bass': notes"),
            of(Unit::Track, Outcome::KeptEmpty, "Track 'Keys audio'"),
            of(
                Unit::Strip(StripField::Level),
                Outcome::Theirs,
                "Strip 'Bass': level"
            ),
            of(
                Unit::Strip(StripField::Pan),
                Outcome::Ours,
                "Strip 'Keys': pan"
            ),
            of(Unit::Marker, Outcome::Removed, "Marker 'M1'"),
            of(Unit::Marker, Outcome::DeletedThere, "Marker 'M2'"),
            of(Unit::Marker, Outcome::DeletedHere, "Marker 'M3'"),
            of(Unit::Marker, Outcome::Added, "Marker 'Bridge'"),
            of(Unit::Tempo, Outcome::Conflict, "Tempo map"),
            of(Unit::Info(0), Outcome::Theirs, "Song info: title"),
            of(Unit::Info(1), Outcome::Ours, "Song info: artist"),
        ]
    );
    let tempo = "Tempo map: here 120 BPM at bar 1; 140 BPM at bar 17, \
                 there 120 BPM at bar 1; 100 BPM at bar 9";
    assert_eq!(
        p.preview,
        MergePreview {
            applied: 3,
            kept: 2,
            added: 1,
            removed: 1,
            conflicts: 4,
            conflict_lines: vec![
                "Track 'Keys audio'".to_owned(),
                "Marker 'M2'".to_owned(),
                "Marker 'M3'".to_owned(),
                tempo.to_owned(),
            ],
        }
    );
    assert_eq!(
        summary(&p.preview),
        "Merged: 3 applied, 2 kept, 1 added, 1 removed, 4 in conflict"
    );
    assert_eq!(
        item(&p.report, ItemKind::Conflict, CONFLICT_SETTING).details,
        vec![tempo]
    );
}

/// Every line the plan lists, with its kind, across the cases that make
/// each one.
#[test]
fn report_lines_are_listed() {
    let kind_of = |what: &str| {
        LINES
            .iter()
            .find(|(w, _)| *w == what)
            .map(|(_, k)| *k)
            .unwrap_or_else(|| panic!("{what} is not in LINES"))
    };
    for (what, kind) in [
        (CONFLICT_TAKE, ItemKind::Conflict),
        (CONFLICT_SETTING, ItemKind::Conflict),
        (KEPT_EMPTY, ItemKind::Conflict),
        (DELETED_THERE, ItemKind::Conflict),
        (DELETED_HERE, ItemKind::Conflict),
        (PRIVATE_COPIES, ItemKind::Changed),
        (LOOPS_UNROLLED, ItemKind::Changed),
        (VST_KEPT, ItemKind::LeftOut),
        (MERGED, ItemKind::Note),
        (BY_NAME, ItemKind::Note),
        (SNAPSHOT_CHOSEN, ItemKind::Note),
        (OLD_EXPORT, ItemKind::Note),
        (SHARED_ELSEWHERE, ItemKind::Note),
        (TAKE_PARTS, ItemKind::Note),
    ] {
        assert_eq!(kind_of(what), kind, "{what}");
    }
    assert_eq!(LINES.len(), 14);

    let mut reports: Vec<Report> = Vec::new();
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);

    // Conflicts: a setting, a deletion each way, a kept-empty track.
    let o = ours(&t, |h, ids| {
        set_gain(h, ids.bass_strip, -9.0);
        set_gain(h, ids.reverb, -4.0);
        run(
            h,
            Command::Lane(LaneCmd::SetMonitor {
                seq: ids.seq,
                lane: ids.keys,
                monitor: MonitorMode::Input,
            }),
        );
        set_markers(h, ids.seq, &[("Chorus", bar(2))]);
    });
    let path = return_file(&t, Strip::Nothing, |p, _| {
        set_db(p, "c1", -3.0);
        p.structure.retain(
            |i| !matches!(i, DpStructureItem::Track(t) if t.named.id.as_deref() == Some("b4")),
        );
        remove_track(p, "t1a");
        move_marker(p, "Verse", 1.0);
    });
    reports.push(plan_of(&t, &path, &o).report);

    // Left out: a plug-in VizDAW cannot host.
    let vst = return_file(&t, Strip::Nothing, |p, _| {
        let d = &mut channel(p, "c1").devices[1];
        d.kind = DpDeviceKind::Vst3;
        d.device_name = "Fancy Comp".into();
    });
    reports.push(plan_of(&t, &vst, &t.base_doc).report);

    // Notes: tags lost; the snapshot chosen; an old export; a take of one
    // part of a split track.
    reports.push(plan_of(&t, &return_file(&t, Strip::Both, |_, _| {}), &t.base_doc).report);
    reports.push(
        plan_with(
            &t,
            &return_file(&t, Strip::Both, |_, _| {}),
            &t.base_doc,
            true,
        )
        .report,
    );
    let mut old = trip(&doc, seq);
    old.snapshot.format = 1;
    old.snapshot.identity = None;
    old.snapshot.region_gain = None;
    let p = plan_of(
        &old,
        &return_file(&old, Strip::Nothing, |_, _| {}),
        &old.base_doc,
    );
    assert!(has(&p.report, OLD_EXPORT));
    assert!(!has(&p.report, BY_NAME), "an old export says OLD_EXPORT");
    reports.push(p.report);
    let o = ours(&t, |h, _| {
        let keys_note = EventId(t.snapshot.identity.as_ref().unwrap().tracks[1].notes[0].event);
        set_pitch(h, keys_note, 62);
    });
    let path = return_file(&t, Strip::Nothing, |p, _| {
        let clip = &mut clips(p, "t1")[0];
        if let Some(DpTimeline::Notes(n)) = clip.content.as_deref_mut() {
            n.notes[0].key = 64;
        }
    });
    let p = plan_of(&t, &path, &o);
    assert_eq!(
        decided(&p),
        vec![of(Unit::Notes, Outcome::Conflict, "Track 'Keys': notes")]
    );
    assert_eq!(p.preview.conflict_lines, vec!["Track 'Keys': notes"]);
    reports.push(p.report);

    let merged = text::merged(t.snapshot.exported_at);
    assert_eq!(
        merged,
        "This file was merged into the project it came from (the export of 2023-11-14)"
    );
    for (what, kind, detail) in [
        (
            CONFLICT_SETTING,
            ItemKind::Conflict,
            Some("Strip 'Bass': level: here \u{2212}9.0 dB, there \u{2212}3.0 dB"),
        ),
        (KEPT_EMPTY, ItemKind::Conflict, Some("Track 'Keys audio'")),
        (DELETED_THERE, ItemKind::Conflict, Some("Bus 'Reverb'")),
        (DELETED_HERE, ItemKind::Conflict, Some("Marker 'Verse'")),
        (VST_KEPT, ItemKind::LeftOut, Some("Fancy Comp")),
        (merged.as_str(), ItemKind::Note, None),
        (BY_NAME, ItemKind::Note, None),
        (SNAPSHOT_CHOSEN, ItemKind::Note, None),
        (OLD_EXPORT, ItemKind::Note, None),
        (TAKE_PARTS, ItemKind::Note, Some("Keys")),
    ] {
        let found = reports
            .iter()
            .find_map(|r| r.items.iter().find(|i| i.what == what && i.kind == kind));
        let found = found.unwrap_or_else(|| panic!("no {kind:?} '{what}'"));
        if let Some(d) = detail {
            assert!(
                found.details.iter().any(|x| x == d),
                "{what}: {:?}",
                found.details
            );
        }
    }
    // The import's own note that the file came from VizDAW is replaced.
    for r in &reports {
        assert!(!has(r, core_io::dawproject::import::FROM_VIZDAW));
        // The take, the private copies, the unrolled loops and the shared
        // edits are listed when the merge is applied (Tasks 6 and 6b).
        for later in [
            CONFLICT_TAKE,
            PRIVATE_COPIES,
            LOOPS_UNROLLED,
            SHARED_ELSEWHERE,
        ] {
            assert!(!has(r, later), "{later}");
        }
    }
    assert_eq!(
        text::conflict_take("Bass", "Returned 2026-11-13"),
        "Bass: take \u{201c}Returned 2026-11-13\u{201d}"
    );
}

#[test]
fn values_are_written_as_the_app_shows_them() {
    assert_eq!(text::level(-6.0), "\u{2212}6.0 dB");
    assert_eq!(text::level(3.0), "+3.0 dB");
    assert_eq!(text::level(0.0), "0.0 dB");
    assert_eq!(text::level(-0.04), "0.0 dB");
    assert_eq!(text::level(-144.0), "\u{2212}\u{221e} dB");
    assert_eq!(text::level(f64::NEG_INFINITY), "\u{2212}\u{221e} dB");
    assert_eq!(text::level(f64::NAN), "\u{2212}\u{221e} dB");
    assert_eq!(text::pan(0.0), "C");
    assert_eq!(text::pan(-0.24), "L 24");
    assert_eq!(text::pan(0.1), "R 10");
    assert_eq!(text::on_off(true), "on");
    assert_eq!(text::on_off(false), "off");
    assert_eq!(text::bpm(120_000), "120 BPM");
    assert_eq!(text::bpm(120_500), "120.5 BPM");
    assert_eq!(text::points(12), "12 points");
    assert_eq!(text::points(1), "1 point");
    assert_eq!(text::utc_date(0), "1970-01-01");
    assert_eq!(text::utc_date(951_782_400), "2000-02-29");
    assert!(text::utc_date(u64::MAX).len() >= 10);
    assert_eq!(
        summary(&MergePreview {
            conflicts: 1,
            ..MergePreview::default()
        }),
        "Merged: 0 applied, 0 kept, 0 added, 0 removed, 1 in conflict"
    );
}
