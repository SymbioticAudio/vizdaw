//! The merge applied (plan 6e Task 6, Rulings 5–7 and 9–11): each test
//! exports `common::merge::fixture()` (its "Bass" strip at −6 dB), makes
//! theirs (the returned file, edited in the file or as commands on a copy
//! of the exported document and exported again) and ours (`History`
//! commands on a copy of the exported document), plans, then applies the
//! plan inside `begin(LABEL)` … `commit()` on a `History` of ours.

mod common;

use common::merge::{
    Strip, Trip, fixture, return_file, stripped_package, theirs_of, trip, vizdaw_only,
};
use common::package::{TestPlist, media_of};
use core_engine::fixtures::{BAR, Builder, Q};
use core_io::dawproject::import::{FROM_VIZDAW, ImportEnv, MediaSource, map};
use core_io::dawproject::merge::apply::{LABEL, apply_merge};
use core_io::dawproject::merge::decide::{Decision, MergePlan, MergePreview, Outcome, Unit, plan};
use core_io::dawproject::merge::text::{
    self, CONFLICT_SETTING, CONFLICT_TAKE, DELETED_HERE, KEPT_EMPTY, NOT_APPLIED, NOTHING, summary,
};
use core_io::dawproject::merge::theirs::{Theirs, theirs};
use core_io::dawproject::package::open;
use core_io::dawproject::record::{PStrip, PTrack, Projection, projection};
use core_io::dawproject::{DpClip, DpMarker, DpNamed, DpNote, DpProject, DpTimeline};
use core_io::limits::PackageLimits;
use core_io::report::{ItemKind, Report};
use core_model::automation::{AutomationMode, AutomationTarget};
use core_model::command::{
    AudioCmd, Command, CrossfadeLength, EventCmd, LaneCmd, MixerCmd, ReferenceCmd, RoutingCmd,
    SequenceCmd, SettingsCmd,
};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec, Fade, FadeShape, LoopCount};
use core_model::file::to_json;
use core_model::history::History;
use core_model::ids::{AnyId, EventId, LaneId, MediaId, PluginInstanceId, SeqId, StripId};
use core_model::input_fx::{InputFx, InputFxKind};
use core_model::lane::{Lane, LaneInput, MonitorMode, QuantizeSpec};
use core_model::learn::{LearnKind, LearnMap, LearnMode, LearnSource};
use core_model::mixer::{AuxSend, PluginSlot, StripKind};
use core_model::mpe::MpeConfig;
use core_model::rules;
use core_model::selection::ViewFilter;
use core_model::sequence::{KeySlot, Marker, SeqLength};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::Tick;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// ---------------------------------------------------------------- helpers

/// The date the tests merge on.
const DATE: &str = "2026-11-13";

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

/// Who the fixture's parts are (from the snapshot's identity).
#[allow(dead_code)]
struct Ids {
    seq: SeqId,
    bass: LaneId,
    keys: LaneId,
    bass_strip: StripId,
    reverb: StripId,
    /// Bass's first note (tick 0, key 36).
    bass_note: EventId,
    /// Keys' note (tick 0, key 60).
    keys_note: EventId,
    /// The Comp insert on Bass's strip.
    comp: PluginInstanceId,
}

fn ids(t: &Trip) -> Ids {
    let i = t.snapshot.identity.as_ref().expect("format 2");
    let strip = |n: usize| StripId(i.strips[n].strip.expect("a strip"));
    let bass_strip = strip(0);
    let comp = t
        .base_doc
        .mixer()
        .strip(bass_strip)
        .unwrap()
        .inserts
        .front()
        .unwrap()
        .id;
    Ids {
        seq: t.seq,
        bass: LaneId(i.tracks[0].lane),
        keys: LaneId(i.tracks[1].lane),
        bass_strip,
        reverb: strip(3),
        bass_note: EventId(i.tracks[0].notes[0].event),
        keys_note: EventId(i.tracks[1].notes[0].event),
        comp,
    }
}

fn run(h: &mut History, cmd: Command) {
    h.execute(&cmd, &ViewFilter::default()).expect("runs");
}

/// `doc` edited by `edit` (with the fixture's IDs from `t`).
fn edited(t: &Trip, doc: &Document, edit: impl FnOnce(&mut History, &Ids)) -> Document {
    let ids = ids(t);
    let mut h = History::new(doc.clone());
    edit(&mut h, &ids);
    h.doc().clone()
}

/// Ours: the exported document, edited by `edit`.
fn ours(t: &Trip, edit: impl FnOnce(&mut History, &Ids)) -> Document {
    edited(t, &t.base_doc, edit)
}

/// The plan for the file at `path` against `ours`.
fn plan_of(t: &Trip, path: &Path, ours: &Document) -> MergePlan {
    plan(
        t.snapshot.clone(),
        theirs_of(t, path),
        ours,
        chosen(),
        &never(),
    )
    .expect("plans")
}

static DOCS: AtomicUsize = AtomicUsize::new(0);

/// Theirs as the other DAW would return `doc` (an edited copy of
/// `t.base_doc`): `doc` exported with the harness's IDs, packaged as the
/// test's [`strip()`] says (with its record, which names `t`'s export,
/// unless stripped), opened and mapped with the snapshot's IDs trusted when
/// the record is there.
fn theirs_doc(t: &Trip, doc: &Document) -> Theirs {
    theirs_with(t, doc, |_| {})
}

/// [`theirs_doc`], with the media a conversion made changed by `media`.
fn theirs_with(
    t: &Trip,
    doc: &Document,
    tweak: impl FnOnce(&mut std::collections::BTreeMap<usize, MediaSource>),
) -> Theirs {
    let x = trip(doc, t.seq).export;
    let n = DOCS.fetch_add(1, Ordering::Relaxed);
    let path = t.dir.path().join(format!("Doc {n}.dawproject"));
    std::fs::write(&path, stripped_package(&x, strip())).expect("writes");
    let pkg = open(&path, &PackageLimits::default(), &never()).expect("opens");
    let mut media = media_of(&pkg, &x, doc);
    tweak(&mut media);
    let installed = |_: core_model::mixer::PluginFormat, _: &str| true;
    let format = t.base_doc.settings().format;
    let trusted = pkg.ret.record.is_some();
    let imp = map(
        &pkg,
        &media,
        &ImportEnv {
            installed: &installed,
            plist: &TestPlist,
            rate: format.sample_rate,
            ids: trusted.then_some(&t.snapshot.ids),
        },
        &never(),
    )
    .expect("maps");
    theirs(imp, format, t.snapshot.origin, &never()).expect("theirs")
}

/// The plan for theirs `doc` (an edited copy of `t.base_doc`) against
/// `ours`.
fn plan_doc(t: &Trip, doc: &Document, ours: &Document) -> MergePlan {
    plan(
        t.snapshot.clone(),
        theirs_doc(t, doc),
        ours,
        chosen(),
        &never(),
    )
    .expect("plans")
}

/// `plan` applied on a `History` of `ours` in one transaction labelled
/// [`LABEL`], merged on [`DATE`].
fn apply_on(ours: &Document, plan: &MergePlan) -> (History, Report) {
    apply_history(History::new(ours.clone()), plan, DATE)
}

fn apply_history(mut h: History, plan: &MergePlan, date: &str) -> (History, Report) {
    let mut report = Report::default();
    h.begin(LABEL).expect("begins");
    apply_merge(&mut h, plan, date, &mut report).expect("applies");
    h.commit().expect("commits");
    if chosen() {
        // The heuristic path, with the snapshot chosen (Task 7).
        assert!(has(&report, text::SNAPSHOT_CHOSEN), "{:#?}", report.items);
        assert!(
            has(&report, text::BY_NAME) || has(&report, text::OLD_EXPORT),
            "{:#?}",
            report.items
        );
    }
    (h, report)
}

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

/// The lane named `name` in `seq`.
#[track_caller]
fn lane<'a>(doc: &'a Document, seq: SeqId, name: &str) -> &'a Lane {
    doc.sequence(seq)
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.name == name)
        .unwrap_or_else(|| panic!("no track {name}"))
}

fn has_lane(doc: &Document, seq: SeqId, name: &str) -> bool {
    doc.sequence(seq)
        .unwrap()
        .lanes
        .iter()
        .any(|l| l.name == name)
}

/// The notes of `lane`'s active take: (tick, pitch).
fn notes_of(lane: &Lane) -> Vec<(i64, u8)> {
    lane.active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Note { pitch, .. } => Some((e.tick.0, pitch)),
            _ => None,
        })
        .collect()
}

/// The take named `name` on `lane`.
#[track_caller]
fn take<'a>(lane: &'a Lane, name: &str) -> &'a core_model::lane::Take {
    lane.takes
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| {
            panic!(
                "no take {name} in {:?}",
                lane.takes.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        })
}

/// A projection with levels rounded to 1e-6 dB, so a level that went
/// through the file's linear form compares equal.
fn rounded(mut p: Projection) -> Projection {
    let r = |x: f64| (x * 1e6).round() / 1e6;
    for s in &mut p.strips {
        s.gain_db = r(s.gain_db);
        s.pan = r(s.pan);
        for send in &mut s.sends {
            send.1 = r(send.1);
        }
    }
    p
}

fn gain(doc: &Document, strip: StripId) -> f64 {
    doc.mixer().strip(strip).unwrap().gain_db
}

fn markers_of(doc: &Document, seq: SeqId) -> Vec<(i64, String)> {
    doc.sequence(seq)
        .unwrap()
        .markers
        .iter()
        .map(|m| (m.tick.0, m.name.clone()))
        .collect()
}

// ------------------------------------------------- theirs: the file's parts

fn tracks(p: &mut DpProject) -> impl Iterator<Item = &mut core_io::dawproject::DpTrack> {
    p.structure.iter_mut().filter_map(|i| match i {
        core_io::dawproject::DpStructureItem::Track(t) => Some(t),
        core_io::dawproject::DpStructureItem::Channel(_) => None,
    })
}

/// Sets channel `id`'s volume to `db`.
fn set_db(p: &mut DpProject, id: &str, db: f64) {
    let c = tracks(p)
        .filter_map(|t| t.channel.as_deref_mut())
        .find(|c| c.named.id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("no channel {id}"));
    let v = c.volume.as_deref_mut().expect("a volume");
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

/// The first note of the first clip on track `id` (Bass's first clip is
/// no sequence use).
fn first_note<'a>(p: &'a mut DpProject, id: &str) -> &'a mut DpNote {
    let clip = &mut clips(p, id)[0];
    match clip.content.as_deref_mut() {
        Some(DpTimeline::Notes(n)) => &mut n.notes[0],
        other => panic!("not notes: {other:?}"),
    }
}

/// Moves Bass's first note `beats` later, its clip long enough to hold
/// it.
fn move_bass_note(p: &mut DpProject, beats: f64) {
    let n = first_note(p, "t0");
    n.time += beats;
    let end = n.time + n.duration;
    let clip = &mut clips(p, "t0")[0];
    clip.duration = Some(clip.duration.unwrap_or(0.0).max(end));
    clip.play_stop = None;
}

fn add_marker(p: &mut DpProject, name: &str, beats: f64) {
    p.arrangement.as_mut().unwrap().markers.push(DpMarker {
        named: DpNamed {
            name: Some(name.into()),
            ..DpNamed::default()
        },
        time: beats,
    });
}

/// Takes the track with XML ID `id`, and its timeline, out of the file.
fn remove_track(p: &mut DpProject, id: &str) {
    use core_io::dawproject::DpStructureItem;
    p.structure
        .retain(|i| !matches!(i, DpStructureItem::Track(t) if t.named.id.as_deref() == Some(id)));
    let lanes = p.arrangement.as_mut().unwrap().lanes.as_mut().unwrap();
    lanes
        .items
        .retain(|i| !matches!(i, DpTimeline::Lanes(l) if l.head.track.as_deref() == Some(id)));
}

/// The owner's check: Bass's first note one beat later, the Bass strip
/// at −3 dB and a marker "Bridge" at bar 9.
fn owners_check(p: &mut DpProject) {
    move_bass_note(p, 1.0);
    set_db(p, "c1", -3.0);
    add_marker(p, "Bridge", 32.0);
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

fn add_note(h: &mut History, seq: SeqId, lane: LaneId, tick: i64, pitch: u8) {
    run(
        h,
        Command::Event(EventCmd::Insert {
            seq,
            lane,
            events: vec![EventSpec::new(
                Tick(tick),
                EventBody::Note {
                    pitch,
                    velocity: 0x8000,
                    release_velocity: 0x8000,
                    duration: Tick(Q),
                    attribute: None,
                    per_note: Default::default(),
                },
            )],
        }),
    );
}

fn created<T>(r: &core_model::command::Report, f: impl Fn(&AnyId) -> Option<T>) -> T {
    r.created.iter().find_map(f).expect("created")
}

// ---------------------------------------------------------------- the tests

#[test]
fn theirs_only_edits_are_applied() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let i = ids(&t);
    let path = return_file(&t, strip(), |p, _| owners_check(p));
    let p = plan_of(&t, &path, &t.base_doc);
    assert_eq!(
        p.preview,
        MergePreview {
            applied: 2,
            added: 1,
            ..MergePreview::default()
        }
    );
    let (h, report) = apply_on(&t.base_doc, &p);

    let bass = lane(h.doc(), seq, "Bass");
    assert!(notes_of(bass).contains(&(Q, 36)), "{:?}", notes_of(bass));
    assert!(!notes_of(bass).contains(&(0, 36)));
    assert!((gain(h.doc(), i.bass_strip) - -3.0).abs() < 1e-6);
    assert!(markers_of(h.doc(), seq).contains(&(8 * BAR, "Bridge".to_owned())));
    assert_eq!(h.undo_label(), Some("Merge DAWproject"));

    // Nothing else changed: the merged project is the exported one with
    // the three edits made as commands.
    let expected = ours(&t, |h, i| {
        run(
            h,
            Command::Event(EventCmd::Move {
                events: vec![i.bass_note],
                delta: Tick(Q),
                to_lane: None,
            }),
        );
        set_gain(h, i.bass_strip, -3.0);
        run(
            h,
            Command::Sequence(SequenceCmd::SetMarkers {
                seq: i.seq,
                markers: [("Verse", 0), ("Chorus", 2 * BAR), ("Bridge", 8 * BAR)]
                    .into_iter()
                    .map(|(n, tick)| Marker {
                        tick: Tick(tick),
                        name: n.into(),
                    })
                    .collect(),
            }),
        );
    });
    assert_eq!(
        rounded(projection(h.doc(), seq).unwrap()),
        rounded(projection(&expected, seq).unwrap())
    );
    assert_eq!(vizdaw_only(h.doc(), seq), vizdaw_only(&t.base_doc, seq));
    assert!(has(&report, &text::merged(t.snapshot.exported_at)));
    assert!(!has(&report, FROM_VIZDAW));
}

#[test]
fn ours_only_edits_are_kept() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let i = ids(&t);
    let path = return_file(&t, strip(), |p, _| set_db(p, "c1", -3.0));
    let o = ours(&t, |h, i| {
        run(
            h,
            Command::Lane(LaneCmd::Rename {
                seq: i.seq,
                lane: i.bass,
                name: "Bass 2".into(),
            }),
        );
        add_note(h, i.seq, i.bass, 2 * Q, 50);
    });
    let p = plan_of(&t, &path, &o);
    let (h, _) = apply_on(&o, &p);
    let bass = h.doc().lane(seq, i.bass).unwrap();
    assert_eq!(bass.name, "Bass 2");
    assert!(notes_of(bass).contains(&(2 * Q, 50)));
    assert!(notes_of(bass).contains(&(0, 36)));
    assert!((gain(h.doc(), i.bass_strip) - -3.0).abs() < 1e-6);
}

/// Theirs moves Bass's first note a beat; ours raises it to key 38.
fn notes_conflict(t: &Trip) -> (std::path::PathBuf, Document) {
    let path = return_file(t, strip(), |p, _| move_bass_note(p, 1.0));
    let o = ours(t, |h, i| set_pitch(h, i.bass_note, 38));
    (path, o)
}

#[test]
fn both_changed_notes_become_an_alternate_take() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let i = ids(&t);
    let (path, o) = notes_conflict(&t);
    let p = plan_of(&t, &path, &o);
    let before = o.lane(seq, i.bass).unwrap().clone();
    let (h, report) = apply_on(&o, &p);

    let bass = h.doc().lane(seq, i.bass).unwrap();
    assert_eq!(bass.takes.len(), 2);
    // Ours' take is still active and unchanged.
    assert_eq!(bass.active_take, before.active_take);
    assert_eq!(bass.active().unwrap(), before.active().unwrap());
    // The new take holds theirs' notes as they play: the moved note and
    // the riff's six.
    let returned = take(bass, "Returned 2026-11-13");
    let mut notes: Vec<(i64, u8)> = returned
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Note { pitch, .. } => Some((e.tick.0, pitch)),
            _ => None,
        })
        .collect();
    notes.sort();
    assert_eq!(notes.len(), 7, "{notes:?}");
    assert!(notes.contains(&(Q, 36)));
    assert!(!notes.iter().any(|n| n.1 == 38));
    assert!(
        returned
            .events
            .values()
            .all(|e| !matches!(e.body, EventBody::Reference { .. }))
    );
    let line = item(&report, ItemKind::Conflict, CONFLICT_TAKE);
    assert_eq!(
        line.details,
        vec!["Bass: take \u{201c}Returned 2026-11-13\u{201d}".to_owned()]
    );
    assert_eq!(h.undo_label(), Some(LABEL));
}

#[test]
fn take_names_count_up_on_the_same_day() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let i = ids(&t);
    let (first, o) = notes_conflict(&t);
    let (h, _) = apply_on(&o, &plan_of(&t, &first, &o));
    let once = h.doc().clone();

    // Other conflicting notes, the same day.
    let second = return_file(&t, strip(), |p, _| first_note(p, "t0").key = 40);
    let (h, report) = apply_history(h, &plan_of(&t, &second, &once), DATE);
    let bass = h.doc().lane(seq, i.bass).unwrap();
    assert_eq!(bass.takes.len(), 3);
    take(bass, "Returned 2026-11-13 (2)");
    assert_eq!(
        item(&report, ItemKind::Conflict, CONFLICT_TAKE).details,
        vec!["Bass: take \u{201c}Returned 2026-11-13 (2)\u{201d}".to_owned()]
    );

    // The first file again: its take already exists.
    let twice = h.doc().clone();
    let label = h.undo_label().map(str::to_owned);
    let (h, report) = apply_history(h, &plan_of(&t, &first, &twice), DATE);
    assert_eq!(h.doc().lane(seq, i.bass).unwrap().takes.len(), 3);
    assert_eq!(h.undo_label().map(str::to_owned), label);
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&twice).unwrap());
    assert_eq!(
        item(&report, ItemKind::Conflict, CONFLICT_TAKE).details,
        vec!["Bass: take \u{201c}Returned 2026-11-13\u{201d}".to_owned()]
    );
}

#[test]
fn both_changed_settings_keep_ours_and_list_theirs() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let i = ids(&t);
    let path = return_file(&t, strip(), |p, _| set_db(p, "c1", -3.0));
    let o = ours(&t, |h, i| set_gain(h, i.bass_strip, -9.0));
    let p = plan_of(&t, &path, &o);
    let h0 = History::new(o.clone());
    assert_eq!(h0.undo_label(), None);
    let (h, report) = apply_on(&o, &p);
    assert_eq!(gain(h.doc(), i.bass_strip), -9.0);
    assert_eq!(
        item(&report, ItemKind::Conflict, CONFLICT_SETTING).details,
        vec!["Strip 'Bass': level: here \u{2212}9.0 dB, there \u{2212}3.0 dB".to_owned()]
    );
    // The only decision is the conflict: no step.
    assert_eq!(h.undo_label(), None);
    assert_eq!(h.undo_depth(), 0);
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&o).unwrap());
    let _ = seq;
}

#[test]
fn deletions_follow_the_table() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let i = ids(&t);
    let keys_gone = |p: &mut DpProject| {
        remove_track(p, "t1");
        remove_track(p, "t1a");
    };

    // Deleted there, untouched here: gone.
    let path = return_file(&t, strip(), |p, _| keys_gone(p));
    let p = plan_of(&t, &path, &t.base_doc);
    let (h, _) = apply_on(&t.base_doc, &p);
    assert!(!has_lane(h.doc(), seq, "Keys"));
    assert!(has_lane(h.doc(), seq, "Bass"));

    // Deleted there, edited here: kept.
    let o = ours(&t, |h, i| set_pitch(h, i.keys_note, 62));
    let p = plan_of(&t, &path, &o);
    let (h, _) = apply_on(&o, &p);
    let keys = lane(h.doc(), seq, "Keys");
    assert!(notes_of(keys).contains(&(0, 62)));

    // Deleted here, edited there: still deleted.
    let path = return_file(&t, strip(), |p, _| first_note(p, "t1").key = 64);
    let o = ours(&t, |h, i| {
        run(
            h,
            Command::Lane(LaneCmd::Remove {
                seq: i.seq,
                lane: i.keys,
            }),
        )
    });
    let p = plan_of(&t, &path, &o);
    let (h, _) = apply_on(&o, &p);
    assert!(!has_lane(h.doc(), seq, "Keys"));
    assert!(h.doc().lane(seq, i.keys).is_err());
}

#[test]
fn deleted_track_with_vizdaw_only_state_is_kept_empty() {
    let (doc, seq) = fixture();
    // Bass gets an inactive take with a note, and input effects.
    let doc = {
        let t0 = trip(&doc, seq);
        edited(&t0, &doc, |h, i| {
            let first = h.doc().lane(i.seq, i.bass).unwrap().active_take;
            run(
                h,
                Command::Lane(LaneCmd::AddTake {
                    seq: i.seq,
                    lane: i.bass,
                    name: "Take 2".into(),
                }),
            );
            add_note(h, i.seq, i.bass, 0, 70);
            run(
                h,
                Command::Lane(LaneCmd::SetActiveTake {
                    seq: i.seq,
                    lane: i.bass,
                    take: first,
                }),
            );
            run(
                h,
                Command::Lane(LaneCmd::SetInputFx {
                    seq: i.seq,
                    lane: i.bass,
                    chain: vec![InputFx {
                        kind: InputFxKind::Transpose { semitones: 2 },
                        bypass: false,
                    }],
                }),
            );
        })
    };
    let t = trip(&doc, seq);
    let i = ids(&t);
    let path = return_file(&t, strip(), |p, _| remove_track(p, "t0"));
    let p = plan_of(&t, &path, &t.base_doc);
    let before = t.base_doc.lane(seq, i.bass).unwrap().clone();
    let (h, report) = apply_on(&t.base_doc, &p);

    let bass = h.doc().lane(seq, i.bass).unwrap();
    assert!(bass.active().unwrap().events.is_empty());
    assert_eq!(take(bass, "Take 2"), take(&before, "Take 2"));
    assert_eq!(bass.input_fx, before.input_fx);
    assert_eq!(
        item(&report, ItemKind::Conflict, KEPT_EMPTY).details,
        vec!["Track 'Bass'".to_owned()]
    );
}

/// The fixture with every kind of state the file cannot carry: a keyboard
/// slot on "Riff", an inactive take on Keys, input effects and MPE on
/// Bass, a MIDI input and monitoring on Keys, drum rows on Keys, an
/// automation mode on the master and a MIDI-learn map.
fn with_vizdaw_only(doc: &Document, seq: SeqId) -> Document {
    let t0 = trip(doc, seq);
    edited(&t0, doc, |h, i| {
        let riff = h
            .doc()
            .sequences()
            .values()
            .find(|s| s.name == "Riff")
            .unwrap()
            .id;
        run(
            h,
            Command::Sequence(SequenceCmd::SetKeyboardSlot {
                seq: riff,
                slot: Some(KeySlot {
                    key: "a".into(),
                    modifiers: 0,
                }),
            }),
        );
        let first = h.doc().lane(i.seq, i.keys).unwrap().active_take;
        run(
            h,
            Command::Lane(LaneCmd::AddTake {
                seq: i.seq,
                lane: i.keys,
                name: "Take 2".into(),
            }),
        );
        add_note(h, i.seq, i.keys, 0, 72);
        run(
            h,
            Command::Lane(LaneCmd::SetActiveTake {
                seq: i.seq,
                lane: i.keys,
                take: first,
            }),
        );
        run(
            h,
            Command::Lane(LaneCmd::SetInputFx {
                seq: i.seq,
                lane: i.bass,
                chain: vec![InputFx {
                    kind: InputFxKind::Transpose { semitones: 2 },
                    bypass: false,
                }],
            }),
        );
        run(
            h,
            Command::Lane(LaneCmd::SetMpe {
                seq: i.seq,
                lane: i.bass,
                mpe: Some(MpeConfig::default()),
            }),
        );
        run(
            h,
            Command::Lane(LaneCmd::SetInput {
                seq: i.seq,
                lane: i.keys,
                input: Some(LaneInput::Midi {
                    port: None,
                    channel: None,
                }),
            }),
        );
        run(
            h,
            Command::Lane(LaneCmd::SetMonitor {
                seq: i.seq,
                lane: i.keys,
                monitor: MonitorMode::Off,
            }),
        );
        run(
            h,
            Command::Drum(core_model::command::DrumCmd::MarkDrumLane {
                seq: i.seq,
                lane: i.keys,
                drum: true,
            }),
        );
        let master = h.doc().mixer().master();
        run(
            h,
            Command::Mixer(MixerCmd::SetAutomationMode {
                strips: vec![master],
                mode: AutomationMode::Touch,
            }),
        );
        run(
            h,
            Command::Settings(SettingsCmd::SetLearn {
                map: LearnMap {
                    source: LearnSource {
                        port: "Knobs".into(),
                        kind: LearnKind::Cc7 { channel: 0, cc: 7 },
                    },
                    target: AutomationTarget::StripFader(master),
                    mode: LearnMode::Absolute,
                    pickup: true,
                },
            }),
        );
    })
}

/// Theirs changes every carried thing: Bass's first note, the Bass
/// strip's level, pan, mute, colour and send, a Keys region, the markers,
/// the tempo and meter maps and the title, and adds a track.
fn change_everything(t: &Trip, doc: &Document) -> Document {
    edited(t, doc, |h, i| {
        run(
            h,
            Command::Event(EventCmd::Move {
                events: vec![i.bass_note],
                delta: Tick(Q),
                to_lane: None,
            }),
        );
        set_gain(h, i.bass_strip, -3.0);
        run(
            h,
            Command::Mixer(MixerCmd::SetPan {
                strip: i.bass_strip,
                pan: 0.5,
            }),
        );
        run(
            h,
            Command::Mixer(MixerCmd::SetMute {
                strip: i.bass_strip,
                mute: true,
            }),
        );
        run(
            h,
            Command::Mixer(MixerCmd::SetColour {
                strip: i.bass_strip,
                colour: Some(core_model::mixer::Swatch::Red),
            }),
        );
        run(
            h,
            Command::Mixer(MixerCmd::EditSend {
                strip: i.bass_strip,
                index: 0,
                send: AuxSend {
                    target: i.reverb,
                    level_db: -4.0,
                    pre_fader: false,
                    enabled: true,
                },
            }),
        );
        let region = h
            .doc()
            .lane(i.seq, i.keys)
            .unwrap()
            .active()
            .unwrap()
            .events
            .values()
            .find(|e| matches!(e.body, EventBody::AudioRegion { .. }))
            .unwrap()
            .id;
        run(
            h,
            Command::Event(EventCmd::Move {
                events: vec![region],
                delta: Tick(8 * BAR),
                to_lane: None,
            }),
        );
        run(
            h,
            Command::Sequence(SequenceCmd::SetMarkers {
                seq: i.seq,
                markers: [("Verse", 0), ("Chorus", 3 * BAR), ("Bridge", 8 * BAR)]
                    .into_iter()
                    .map(|(n, tick)| Marker {
                        tick: Tick(tick),
                        name: n.into(),
                    })
                    .collect(),
            }),
        );
        run(
            h,
            Command::Sequence(SequenceCmd::SetTempo {
                seq: i.seq,
                tempo: TempoMap::new(vec![
                    TempoPoint {
                        tick: Tick(0),
                        bpm: Bpm(120_000),
                        ramp: false,
                    },
                    TempoPoint {
                        tick: Tick(4 * BAR),
                        bpm: Bpm(140_000),
                        ramp: false,
                    },
                ])
                .unwrap(),
            }),
        );
        let mut meter = h.doc().sequence(i.seq).unwrap().meter.clone();
        let mut entries: Vec<_> = meter.entries().iter().copied().collect();
        let mut e = entries[0];
        e.tick = Tick(4 * BAR);
        e.meter = core_model::meter::Meter { num: 3, den: 4 };
        entries.push(e);
        meter = core_model::meter::MeterMap::new(entries).unwrap();
        run(
            h,
            Command::Sequence(SequenceCmd::SetMeter { seq: i.seq, meter }),
        );
        let mut info = h.doc().settings().info.clone();
        info.title = Some("Returned".into());
        run(h, Command::Settings(SettingsCmd::SetInfo { info }));
        let r = h
            .execute(
                &Command::Lane(LaneCmd::Add {
                    seq: i.seq,
                    name: "Extra".into(),
                    index: None,
                }),
                &ViewFilter::default(),
            )
            .unwrap();
        let extra = created(&r, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        });
        let dest = h.doc().lane(i.seq, i.bass).unwrap().midi_dest;
        run(
            h,
            Command::Lane(LaneCmd::SetDest {
                seq: i.seq,
                lane: extra,
                dest,
            }),
        );
        add_note(h, i.seq, extra, 0, 48);
    })
}

#[test]
fn vizdaw_only_state_is_byte_equal() {
    let (doc, seq) = fixture();
    let doc = with_vizdaw_only(&doc, seq);
    let t = trip(&doc, seq);
    let theirs = change_everything(&t, &t.base_doc);
    let p = plan_doc(&t, &theirs, &t.base_doc);
    // Without tags a marker moved by more than a tick ("Chorus") is new
    // plus deleted (Ruling 4: by tag, else name and tick).
    assert!(
        p.decisions.iter().all(|d| match d.outcome {
            Outcome::Theirs | Outcome::Added => true,
            Outcome::Removed => !tagged() && d.unit == Unit::Marker,
            _ => false,
        }),
        "{:#?}",
        p.decisions
    );
    for unit in [
        Unit::Notes,
        Unit::Regions,
        Unit::Marker,
        Unit::Tempo,
        Unit::Meter,
    ] {
        assert!(p.decisions.iter().any(|d| d.unit == unit), "{unit:?}");
    }
    let before = vizdaw_only(&t.base_doc, seq);
    let (h, _) = apply_on(&t.base_doc, &p);
    assert_eq!(vizdaw_only(h.doc(), seq), before);
    assert!(has_lane(h.doc(), seq, "Extra"));
    assert_eq!(
        h.doc().sequence(seq).unwrap().meter.entries().len(),
        2,
        "the meter map is theirs"
    );
}

#[test]
fn one_undo_restores_the_project_exactly() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    // The owner's check.
    let path = return_file(&t, strip(), |p, _| owners_check(p));
    let p = plan_of(&t, &path, &t.base_doc);
    let (mut h, _) = apply_on(&t.base_doc, &p);
    h.undo().unwrap();
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&t.base_doc).unwrap());

    // A notes conflict, a setting conflict and theirs-only edits.
    let path = return_file(&t, strip(), |p, _| {
        owners_check(p);
        set_db(p, "c1", -3.0);
    });
    let o = ours(&t, |h, i| {
        set_pitch(h, i.bass_note, 38);
        set_gain(h, i.bass_strip, -9.0);
    });
    let p = plan_of(&t, &path, &o);
    assert!(p.preview.conflicts >= 2, "{:?}", p.preview);
    let (mut h, _) = apply_on(&o, &p);
    assert_eq!(h.undo_label(), Some(LABEL));
    h.undo().unwrap();
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&o).unwrap());
    let _ = seq;
}

#[test]
fn new_tracks_and_buses_are_added_as_an_import_would() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let theirs_doc_ = edited(&t, &t.base_doc, |h, i| {
        let r = h
            .execute(
                &Command::Routing(RoutingCmd::AddDestination {
                    name: "Lead Synth".into(),
                    kind: DestKind::PluginInstrument {
                        slot: PluginSlot::new(
                            PluginInstanceId(0),
                            common::merge::clap("org.example.Lead"),
                        ),
                    },
                    default_patch: None,
                }),
                &ViewFilter::default(),
            )
            .unwrap();
        let dest = created(&r, |id| match id {
            AnyId::Dest(d) => Some(*d),
            _ => None,
        });
        let r = h
            .execute(
                &Command::Lane(LaneCmd::Add {
                    seq: i.seq,
                    name: "Lead".into(),
                    index: None,
                }),
                &ViewFilter::default(),
            )
            .unwrap();
        let lead = created(&r, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        });
        run(
            h,
            Command::Lane(LaneCmd::SetDest {
                seq: i.seq,
                lane: lead,
                dest: Some(dest),
            }),
        );
        add_note(h, i.seq, lead, 0, 72);
        add_note(h, i.seq, lead, Q, 74);
        let r = h
            .execute(
                &Command::Mixer(MixerCmd::AddBus {
                    name: "Hall".into(),
                    index: None,
                }),
                &ViewFilter::default(),
            )
            .unwrap();
        let hall = created(&r, |id| match id {
            AnyId::Strip(s) => Some(*s),
            _ => None,
        });
        set_gain(h, hall, -2.0);
        let strip = h
            .doc()
            .mixer()
            .find_kind(StripKind::Instrument(dest))
            .unwrap();
        set_gain(h, strip, -5.0);
        run(
            h,
            Command::Mixer(MixerCmd::AddSend {
                strip,
                send: AuxSend {
                    target: hall,
                    level_db: -8.0,
                    pre_fader: false,
                    enabled: true,
                },
            }),
        );
    });
    let th = theirs_doc(&t, &theirs_doc_);
    let import = th.projection.clone();
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    assert_eq!(p.preview.added, 2, "{:#?}", p.decisions);
    let (h, _) = apply_on(&t.base_doc, &p);
    let merged = rounded(projection(h.doc(), seq).unwrap());
    let import = rounded(import);

    // The track as the import made it.
    let find = |p: &Projection, name: &str| -> PTrack {
        p.tracks.iter().find(|t| t.name == name).unwrap().clone()
    };
    let strip_of = |p: &Projection, tr: &PTrack| -> PStrip { p.strips[tr.strip.unwrap()].clone() };
    let (ours_lead, their_lead) = (find(&merged, "Lead"), find(&import, "Lead"));
    assert_eq!(
        PTrack {
            strip: None,
            ..ours_lead.clone()
        },
        PTrack {
            strip: None,
            ..their_lead.clone()
        }
    );
    let (os, ts) = (
        strip_of(&merged, &ours_lead),
        strip_of(&import, &their_lead),
    );
    let named_sends = |p: &Projection, s: &PStrip| -> Vec<(String, f64)> {
        s.sends
            .iter()
            .map(|x| (p.strips[x.0].name.clone(), x.1))
            .collect()
    };
    assert_eq!(os.gain_db, ts.gain_db);
    assert_eq!(os.devices, ts.devices);
    assert_eq!(named_sends(&merged, &os), named_sends(&import, &ts));
    assert_eq!(named_sends(&merged, &os), vec![("Hall".to_owned(), -8.0)]);
    let hall = |p: &Projection| p.strips.iter().find(|s| s.name == "Hall").cloned().unwrap();
    assert_eq!(hall(&merged).gain_db, hall(&import).gain_db);
    assert_eq!(hall(&merged).role, "effect");

    // The same file again adds nothing (Review Focus 2).
    let once = h.doc().clone();
    let again = plan_doc(&t, &theirs_doc_, &once);
    assert_eq!(again.preview.added, 0, "{:#?}", again.decisions);
    let label = h.undo_label().map(str::to_owned);
    let (h, _) = apply_history(h, &again, DATE);
    assert_eq!(h.undo_label().map(str::to_owned), label);
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&once).unwrap());
}

/// A region of media `media` at `tick`, one second long.
fn region(media: MediaId, tick: i64) -> EventSpec {
    let no = Fade {
        length_frames: 0,
        shape: FadeShape::Linear,
    };
    EventSpec::new(
        Tick(tick),
        EventBody::AudioRegion {
            media,
            source_offset: 0,
            length: 48_000,
            gain_db: 0.0,
            fade_in: no,
            fade_out: no,
            name: None,
            crossfade: false,
        },
    )
}

#[test]
fn crossfaded_pair_added_in_one_insert() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let theirs_doc_ = edited(&t, &t.base_doc, |h, i| {
        let media = *h.doc().media().keys().next().unwrap();
        let r = h
            .execute(
                &Command::Event(EventCmd::Insert {
                    seq: i.seq,
                    lane: i.keys,
                    events: vec![region(media, 8 * BAR), region(media, 8 * BAR + Q)],
                }),
                &ViewFilter::default(),
            )
            .unwrap();
        let second = r
            .created
            .iter()
            .filter_map(|id| match id {
                AnyId::Event(e) => Some(*e),
                _ => None,
            })
            .nth(1)
            .unwrap();
        run(
            h,
            Command::Audio(AudioCmd::Crossfade {
                event: second,
                length: CrossfadeLength::Frames(4_800),
                shape: FadeShape::EqualPower,
            }),
        );
    });
    let p = plan_doc(&t, &theirs_doc_, &t.base_doc);
    assert!(
        p.decisions
            .iter()
            .any(|d| d.unit == Unit::Regions && d.outcome == Outcome::Theirs),
        "{:#?}",
        p.decisions
    );
    let (h, report) = apply_on(&t.base_doc, &p);
    let keys = h.doc().lane(seq, ids(&t).keys).unwrap();
    let regions: Vec<(i64, bool)> = keys
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::AudioRegion { crossfade, .. } => Some((e.tick.0, crossfade)),
            _ => None,
        })
        .collect();
    assert_eq!(
        regions,
        vec![(0, false), (Q, true), (8 * BAR, false), (8 * BAR + Q, true)]
    );
    // The model changed no crossfade the merge made.
    assert!(!has(&report, core_io::dawproject::import::CROSSFADES_PLAIN));
}

#[test]
fn plugin_state_from_theirs_is_one_step() {
    let (doc, seq) = fixture();
    let comp_of = |doc: &Document| {
        let t0 = trip(doc, seq);
        ids(&t0).comp
    };
    let comp = comp_of(&doc);
    let mut h = History::new(doc);
    h.capture_state(comp, b"ours").unwrap();
    let doc = h.doc().clone();
    let t = trip(&doc, seq);
    let mut h = History::new(doc.clone());
    h.capture_state(comp, b"theirs").unwrap();
    let theirs_doc_ = h.doc().clone();

    let p = plan_doc(&t, &theirs_doc_, &t.base_doc);
    assert!(
        p.decisions
            .iter()
            .any(|d| matches!(d.unit, Unit::State(_)) && d.outcome == Outcome::Theirs),
        "{:#?}",
        p.decisions
    );
    let blob = |doc: &Document| -> Vec<u8> {
        let slot = doc.plugin_slot(comp).unwrap();
        doc.state_blob(slot.state.unwrap()).unwrap().to_vec()
    };
    let (mut h, _) = apply_on(&t.base_doc, &p);
    assert_eq!(blob(h.doc()), b"theirs");
    assert_eq!(h.undo_label(), Some(LABEL));
    h.undo().unwrap();
    assert_eq!(blob(h.doc()), b"ours");
    assert_eq!(h.undo_depth(), 0);
}

#[test]
fn nothing_to_merge_adds_no_step() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, strip(), |_, _| {});
    // Ours has a step of its own before the merge.
    let mut h = History::new(t.base_doc.clone());
    set_gain(&mut h, ids(&t).bass_strip, -6.0);
    let label = h.undo_label().map(str::to_owned);
    let o = h.doc().clone();
    let p = plan_of(&t, &path, &o);
    assert!(p.decisions.is_empty(), "{:#?}", p.decisions);
    assert_eq!(summary(&p.preview), NOTHING);
    let (h, _) = apply_history(h, &p, DATE);
    assert_eq!(h.undo_label().map(str::to_owned), label);
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&o).unwrap());
    let _ = seq;
}

#[test]
fn merging_the_same_file_twice_changes_nothing_the_second_time() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let i = ids(&t);
    // The owner's check, and the notes conflict on Bass.
    let path = return_file(&t, strip(), |p, _| owners_check(p));
    let o = ours(&t, |h, i| set_pitch(h, i.bass_note, 38));
    let (h, _) = apply_on(&o, &plan_of(&t, &path, &o));
    let once = h.doc().clone();
    let label = h.undo_label().map(str::to_owned);
    assert_eq!(once.lane(seq, i.bass).unwrap().takes.len(), 2);

    let again = plan_of(&t, &path, &once);
    for d in &again.decisions {
        match d.outcome {
            Outcome::Same | Outcome::Ours => {}
            Outcome::Conflict => assert_eq!(d.unit, Unit::Notes, "{d:?}"),
            _ => panic!("{d:?}"),
        }
    }
    let (h, _) = apply_history(h, &again, DATE);
    assert_eq!(h.undo_label().map(str::to_owned), label);
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&once).unwrap());
    let bass = h.doc().lane(seq, i.bass).unwrap();
    assert_eq!(bass.takes.len(), 2);
    assert_eq!(
        markers_of(h.doc(), seq)
            .iter()
            .filter(|m| m.1 == "Bridge")
            .count(),
        1
    );
}

/// Theirs' edit inside Riff, played through both of its uses, plays on
/// ours exactly as on theirs (since Task 6b through an edit of Riff,
/// Ruling 8).
#[test]
fn edits_inside_a_use_play_as_theirs() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let theirs_doc_ = edited(&t, &t.base_doc, |h, _| {
        let riff = h
            .doc()
            .sequences()
            .values()
            .find(|s| s.name == "Riff")
            .unwrap();
        let first = riff.lanes[0]
            .active()
            .unwrap()
            .events
            .values()
            .next()
            .unwrap()
            .id;
        set_pitch(h, first, 41);
    });
    let th = theirs_doc(&t, &theirs_doc_);
    let played = |p: &Projection| {
        let mut n: Vec<(i64, u8)> = p.tracks[0]
            .notes_played
            .iter()
            .map(|n| (n.tick, n.pitch))
            .collect();
        n.sort();
        n
    };
    let want = played(&th.projection);
    // The riff plays once at bar 2 and twice from bar 4.
    assert_eq!(want.iter().filter(|n| n.1 == 41).count(), 3);
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    let (h, _) = apply_on(&t.base_doc, &p);
    assert_eq!(played(&projection(h.doc(), seq).unwrap()), want);
    // Keys is untouched.
    assert_eq!(
        projection(h.doc(), seq).unwrap().tracks[1..],
        projection(&t.base_doc, seq).unwrap().tracks[1..]
    );
}

// ------------------------------------------------- fix round 1

/// The fixture, its sequence locked to eight bars, with a track "Loop" on
/// CLAP instrument "Loop Synth" (`org.example.Loop`) holding `notes`
/// (tick, key), then shaped by `shape` (its own loop, length or play
/// quantize).
fn with_loop_track(
    notes: &[(i64, u8)],
    shape: impl FnOnce(&mut History, SeqId, LaneId),
) -> (Document, SeqId) {
    let (doc, seq) = fixture();
    let mut h = History::new(doc);
    let r = h
        .execute(
            &Command::Routing(RoutingCmd::AddDestination {
                name: "Loop Synth".into(),
                kind: DestKind::PluginInstrument {
                    slot: PluginSlot::new(
                        PluginInstanceId(0),
                        common::merge::clap("org.example.Loop"),
                    ),
                },
                default_patch: None,
            }),
            &ViewFilter::default(),
        )
        .unwrap();
    let dest = created(&r, |id| match id {
        AnyId::Dest(d) => Some(*d),
        _ => None,
    });
    let r = h
        .execute(
            &Command::Lane(LaneCmd::Add {
                seq,
                name: "Loop".into(),
                index: None,
            }),
            &ViewFilter::default(),
        )
        .unwrap();
    let lp = created(&r, |id| match id {
        AnyId::Lane(l) => Some(*l),
        _ => None,
    });
    run(
        &mut h,
        Command::Lane(LaneCmd::SetDest {
            seq,
            lane: lp,
            dest: Some(dest),
        }),
    );
    for &(tick, key) in notes {
        add_note(&mut h, seq, lp, tick, key);
    }
    run(
        &mut h,
        Command::Sequence(SequenceCmd::SetLength {
            seq,
            length: SeqLength::Locked(Tick(8 * BAR)),
        }),
    );
    shape(&mut h, seq, lp);
    (h.doc().clone(), seq)
}

/// The XML ID of the track named `name`.
fn track_id(p: &mut DpProject, name: &str) -> String {
    tracks(p)
        .find(|t| t.named.name.as_deref() == Some(name))
        .and_then(|t| t.named.id.clone())
        .unwrap_or_else(|| panic!("no track {name}"))
}

/// The third note (by time) of the file's track `name`.
fn third_note<'a>(p: &'a mut DpProject, name: &str) -> &'a mut DpNote {
    let id = track_id(p, name);
    let mut at: Vec<(f64, usize, usize)> = Vec::new();
    for (c, clip) in clips(p, &id).iter().enumerate() {
        if let Some(DpTimeline::Notes(n)) = clip.content.as_deref() {
            for (k, note) in n.notes.iter().enumerate() {
                at.push((clip.time + note.time, c, k));
            }
        }
    }
    at.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (_, c, k) = at[2];
    match clips(p, &id)[c].content.as_deref_mut() {
        Some(DpTimeline::Notes(n)) => &mut n.notes[k],
        _ => unreachable!(),
    }
}

/// C1: theirs changes one played note of "Loop" (its third, on the third
/// pass when it loops). The export's tick transform has no inverse, so
/// ours' notes stay as they are on every pass and theirs' version becomes
/// the take "Returned 2026-11-13", listed under `CONFLICT_TAKE`.
fn transformed_track_keeps_ours(doc: Document, seq: SeqId) {
    let t = trip(&doc, seq);
    let path = return_file(&t, strip(), |p, _| third_note(p, "Loop").key = 64);
    let p = plan_of(&t, &path, &t.base_doc);
    let notes = p
        .decisions
        .iter()
        .find(|d| d.unit == Unit::Notes)
        .unwrap_or_else(|| panic!("{:#?}", p.decisions));
    assert_eq!(notes.outcome, Outcome::Conflict, "{notes:?}");
    assert_eq!(notes.what, "Track 'Loop': notes");
    assert_eq!(p.preview.applied, 0, "{:?}", p.preview);

    let loop_of = |doc: &Document| lane(doc, seq, "Loop").clone();
    let played = |doc: &Document| -> Vec<(i64, u8)> {
        let pr = projection(doc, seq).unwrap();
        let tr = pr.tracks.iter().find(|t| t.name == "Loop").unwrap();
        tr.notes_played.iter().map(|n| (n.tick, n.pitch)).collect()
    };
    let before = loop_of(&t.base_doc);
    let (h, report) = apply_on(&t.base_doc, &p);
    let after = loop_of(h.doc());
    assert_eq!(after.active_take, before.active_take);
    assert_eq!(after.active(), before.active());
    assert_eq!(played(h.doc()), played(&t.base_doc), "every pass is ours'");
    assert!(!played(h.doc()).iter().any(|n| n.1 == 64));
    let returned = take(&after, "Returned 2026-11-13");
    assert!(
        returned
            .events
            .values()
            .any(|e| matches!(e.body, EventBody::Note { pitch: 64, .. })),
        "{returned:?}"
    );
    assert_eq!(
        item(&report, ItemKind::Conflict, CONFLICT_TAKE).details,
        vec!["Loop: take \u{201c}Returned 2026-11-13\u{201d}".to_owned()]
    );
    assert_eq!(h.undo_label(), Some(LABEL));
}

#[test]
fn looped_track_edit_becomes_a_take() {
    // One note, looped every bar: it plays on eight passes.
    let (doc, seq) = with_loop_track(&[(0, 60)], |h, seq, lane| {
        run(
            h,
            Command::Lane(LaneCmd::SetLength {
                seq,
                lane,
                length: Some(Tick(BAR)),
                looped: true,
                locked: false,
            }),
        )
    });
    transformed_track_keeps_ours(doc, seq);
}

#[test]
fn own_length_track_edit_becomes_a_take() {
    let notes = [(0, 60), (BAR, 60), (2 * BAR, 60), (3 * BAR, 60)];
    let (doc, seq) = with_loop_track(&notes, |h, seq, lane| {
        run(
            h,
            Command::Lane(LaneCmd::SetLength {
                seq,
                lane,
                length: Some(Tick(6 * BAR)),
                looped: false,
                locked: false,
            }),
        )
    });
    transformed_track_keeps_ours(doc, seq);
}

#[test]
fn play_quantized_track_edit_becomes_a_take() {
    let notes = [
        (0, 60),
        (BAR + 100, 60),
        (2 * BAR + 200, 60),
        (3 * BAR + 300, 60),
    ];
    let (doc, seq) = with_loop_track(&notes, |h, seq, lane| {
        run(
            h,
            Command::Lane(LaneCmd::SetPlayQuantize {
                seq,
                lane,
                quantize: Some(QuantizeSpec::grid(Tick(Q), 50)),
            }),
        )
    });
    transformed_track_keeps_ours(doc, seq);
}

/// The fixture with inserts `org.example.Gate` and `org.example.Lim`
/// after Bass's Comp; the slots of Comp, Gate and Lim.
fn with_three_inserts() -> (Document, SeqId, [PluginInstanceId; 3]) {
    let (doc, seq) = fixture();
    let t0 = trip(&doc, seq);
    let strip = ids(&t0).bass_strip;
    let mut h = History::new(doc);
    for id in ["org.example.Gate", "org.example.Lim"] {
        run(
            &mut h,
            Command::Mixer(MixerCmd::AddInsert {
                strip,
                index: None,
                plugin: common::merge::clap(id),
            }),
        );
    }
    let ins: Vec<PluginInstanceId> = h
        .doc()
        .mixer()
        .strip(strip)
        .unwrap()
        .inserts
        .iter()
        .map(|s| s.id)
        .collect();
    (h.doc().clone(), seq, [ins[0], ins[1], ins[2]])
}

/// I2: theirs replaces Gate (a `Devices` change from the second insert)
/// and changes Comp's state (before it) and Lim's bypass (after it).
#[test]
fn device_list_and_slot_changes_on_one_strip_all_land() {
    let (doc, seq, [comp, gate, lim]) = with_three_inserts();
    let t = trip(&doc, seq);
    let strip = ids(&t).bass_strip;
    let theirs_doc_ = edited(&t, &t.base_doc, |h, _| {
        h.capture_state(comp, b"comp theirs").unwrap();
        run(h, Command::Mixer(MixerCmd::RemoveInsert { slot: gate }));
        run(
            h,
            Command::Mixer(MixerCmd::AddInsert {
                strip,
                index: Some(1),
                plugin: common::merge::clap("org.example.Gate2"),
            }),
        );
        run(
            h,
            Command::Mixer(MixerCmd::SetBypass {
                slot: lim,
                bypass: true,
            }),
        );
    });
    let p = plan_doc(&t, &theirs_doc_, &t.base_doc);
    for unit in [Unit::Devices, Unit::State(1), Unit::Bypass(3)] {
        assert!(
            p.decisions
                .iter()
                .any(|d| d.unit == unit && d.outcome == Outcome::Theirs),
            "{unit:?}: {:#?}",
            p.decisions
        );
    }
    let (h, _) = apply_on(&t.base_doc, &p);
    let s = h.doc().mixer().strip(strip).unwrap();
    let names: Vec<&str> = s.inserts.iter().map(|x| x.plugin.id.as_str()).collect();
    assert_eq!(
        names,
        ["org.example.Comp", "org.example.Gate2", "org.example.Lim"]
    );
    let comp_now = &s.inserts[0];
    assert_eq!(comp_now.id, comp, "Comp is kept");
    let blob = h
        .doc()
        .state_blob(comp_now.state.unwrap())
        .unwrap()
        .to_vec();
    assert_eq!(blob, b"comp theirs");
    assert!(s.inserts[2].bypass, "Lim's bypass is theirs");
    assert!(!s.inserts[1].bypass);
    assert_eq!(h.undo_label(), Some(LABEL));
}

/// The channel with XML ID `id`.
fn channel<'a>(p: &'a mut DpProject, id: &str) -> &'a mut core_io::dawproject::DpChannel {
    tracks(p)
        .filter_map(|t| t.channel.as_deref_mut())
        .find(|c| c.named.id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("no channel {id}"))
}

/// I3: theirs swaps Bass's instrument. VizDAW's is kept, listed under
/// `NOT_APPLIED` with both, and nothing counts as applied.
#[test]
fn instrument_swap_is_kept_and_listed() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, strip(), |p, _| {
        let d = channel(p, "c1")
            .devices
            .iter_mut()
            .find(|d| d.device_id.as_deref() == Some("org.example.Bass"))
            .expect("the instrument");
        d.device_id = Some("org.example.Other".into());
        d.device_name = "Other Synth".into();
        d.named.comment = None;
    });
    let p = plan_of(&t, &path, &t.base_doc);
    let d = p
        .decisions
        .iter()
        .find(|d| d.unit == Unit::Instrument)
        .unwrap_or_else(|| panic!("{:#?}", p.decisions));
    assert_eq!(d.outcome, Outcome::Conflict);
    assert!(
        p.decisions.iter().all(|d| d.unit != Unit::Devices),
        "{:#?}",
        p.decisions
    );
    assert_eq!(p.preview.applied, 0, "{:?}", p.preview);
    assert_eq!(
        item(&p.report, ItemKind::Conflict, NOT_APPLIED).details,
        vec!["Strip 'Bass': instrument: here org.example.Bass, there org.example.Other".to_owned()]
    );
    let (h, report) = apply_on(&t.base_doc, &p);
    assert!(has(&report, NOT_APPLIED));
    assert_eq!(h.undo_depth(), 0);
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&t.base_doc).unwrap());
}

/// Task 7 carry-over (Task 5 hand-off): theirs swaps Bass's instrument for
/// a VST3 one. VizDAW hosts no VST3: ours keeps its instrument, the swap is
/// listed under `NOT_APPLIED`, and the merge makes no step.
#[test]
fn instrument_swapped_for_a_vst3_is_kept_and_listed() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, strip(), |p, _| {
        let d = channel(p, "c1")
            .devices
            .iter_mut()
            .find(|d| d.device_id.as_deref() == Some("org.example.Bass"))
            .expect("the instrument");
        d.kind = core_io::dawproject::DpDeviceKind::Vst3;
        d.device_id = Some("ABCDEF0123456789ABCDEF0123456789".into());
        d.device_name = "Other Synth".into();
        d.named.comment = None;
        d.state = None;
    });
    let p = plan_of(&t, &path, &t.base_doc);
    assert_eq!(p.preview.applied, 0, "{:#?} {:#?}", p.decisions, p.report);
    let i = item(&p.report, ItemKind::Conflict, NOT_APPLIED);
    assert_eq!(i.details.len(), 1, "{:?}", i.details);
    assert!(
        i.details[0].starts_with("Strip 'Bass': instrument: here org.example.Bass, there "),
        "{:?}",
        i.details
    );
    let (h, report) = apply_on(&t.base_doc, &p);
    assert!(has(&report, NOT_APPLIED));
    assert_eq!(h.undo_depth(), 0);
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&t.base_doc).unwrap());
}

/// I3: theirs drops Comp's state. No command clears one: ours stays,
/// listed under `NOT_APPLIED`, and nothing counts as applied.
#[test]
fn dropped_state_is_kept_and_listed() {
    let (doc, seq) = fixture();
    let comp = ids(&trip(&doc, seq)).comp;
    let mut h = History::new(doc);
    h.capture_state(comp, b"ours").unwrap();
    let doc = h.doc().clone();
    let t = trip(&doc, seq);
    let path = return_file(&t, strip(), |p, _| {
        let d = channel(p, "c1")
            .devices
            .iter_mut()
            .find(|d| d.device_id.as_deref() == Some("org.example.Comp"))
            .expect("Comp");
        d.state = None;
    });
    let p = plan_of(&t, &path, &t.base_doc);
    let d = p
        .decisions
        .iter()
        .find(|d| matches!(d.unit, Unit::State(_)))
        .unwrap_or_else(|| panic!("{:#?}", p.decisions));
    assert_eq!(d.outcome, Outcome::Conflict);
    assert_eq!(p.preview.applied, 0, "{:?}", p.preview);
    let hash = t.snapshot.projection.strips[0].devices[1]
        .3
        .expect("a state");
    assert_eq!(
        item(&p.report, ItemKind::Conflict, NOT_APPLIED).details,
        vec![format!(
            "Strip 'Bass': settings of org.example.Comp: here settings {hash:016x}, there none"
        )]
    );
    let (h, _) = apply_on(&t.base_doc, &p);
    assert_eq!(h.undo_depth(), 0);
    let slot = h.doc().plugin_slot(comp).unwrap();
    assert_eq!(h.doc().state_blob(slot.state.unwrap()).unwrap(), b"ours");
}

/// Ours: the exported document with bus "Reverb" deleted.
fn reverb_deleted(t: &Trip) -> Document {
    ours(t, |h, i| {
        run(h, Command::Mixer(MixerCmd::RemoveBus { strip: i.reverb }))
    })
}

fn keys_strip(doc: &Document, seq: SeqId) -> StripId {
    let dest = lane(doc, seq, "Keys").midi_dest.unwrap();
    doc.mixer().find_kind(StripKind::Instrument(dest)).unwrap()
}

fn has_bus(doc: &Document, name: &str) -> bool {
    doc.mixer()
        .strips()
        .values()
        .any(|s| s.kind == StripKind::Bus && s.name == name)
}

/// I3: theirs gives Keys a send to Reverb and plays it into Reverb, which
/// ours deleted: no send is made, Keys plays into the master, and both are
/// listed under `DELETED_HERE`.
#[test]
fn send_and_output_to_a_bus_deleted_here_are_listed() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let theirs_doc_ = edited(&t, &t.base_doc, |h, i| {
        let keys = keys_strip(h.doc(), i.seq);
        run(
            h,
            Command::Mixer(MixerCmd::AddSend {
                strip: keys,
                send: AuxSend {
                    target: i.reverb,
                    level_db: -12.0,
                    pre_fader: false,
                    enabled: true,
                },
            }),
        );
        run(
            h,
            Command::Mixer(MixerCmd::SetOutput {
                strip: keys,
                output: core_model::mixer::Output::Bus(i.reverb),
            }),
        );
    });
    let o = reverb_deleted(&t);
    let p = plan_doc(&t, &theirs_doc_, &o);
    assert_eq!(p.preview.applied, 0, "{:#?}", p.decisions);
    let mut details = item(&p.report, ItemKind::Conflict, DELETED_HERE)
        .details
        .clone();
    details.sort();
    assert_eq!(
        details,
        vec![
            "Strip 'Keys': output to 'Reverb'".to_owned(),
            "Strip 'Keys': send to 'Reverb'".to_owned(),
        ]
    );
    let (h, _) = apply_on(&o, &p);
    let keys = h.doc().mixer().strip(keys_strip(h.doc(), seq)).unwrap();
    assert!(keys.sends.is_empty());
    assert_eq!(keys.output, core_model::mixer::Output::Master);
    assert!(!has_bus(h.doc(), "Reverb"));
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&o).unwrap());
}

/// I4: theirs adds a track that plays into Reverb and sends to it; ours
/// deleted Reverb. The bus is not added back: the track plays into the
/// master with no send, listed under `DELETED_HERE`.
#[test]
fn new_track_needing_a_bus_deleted_here_plays_into_the_master() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let theirs_doc_ = edited(&t, &t.base_doc, |h, i| {
        let r = h
            .execute(
                &Command::Routing(RoutingCmd::AddDestination {
                    name: "Lead Synth".into(),
                    kind: DestKind::PluginInstrument {
                        slot: PluginSlot::new(
                            PluginInstanceId(0),
                            common::merge::clap("org.example.Lead"),
                        ),
                    },
                    default_patch: None,
                }),
                &ViewFilter::default(),
            )
            .unwrap();
        let dest = created(&r, |id| match id {
            AnyId::Dest(d) => Some(*d),
            _ => None,
        });
        let r = h
            .execute(
                &Command::Lane(LaneCmd::Add {
                    seq: i.seq,
                    name: "Lead".into(),
                    index: None,
                }),
                &ViewFilter::default(),
            )
            .unwrap();
        let lead = created(&r, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        });
        run(
            h,
            Command::Lane(LaneCmd::SetDest {
                seq: i.seq,
                lane: lead,
                dest: Some(dest),
            }),
        );
        add_note(h, i.seq, lead, 0, 72);
        let strip = h
            .doc()
            .mixer()
            .find_kind(StripKind::Instrument(dest))
            .unwrap();
        run(
            h,
            Command::Mixer(MixerCmd::AddSend {
                strip,
                send: AuxSend {
                    target: i.reverb,
                    level_db: -8.0,
                    pre_fader: false,
                    enabled: true,
                },
            }),
        );
        run(
            h,
            Command::Mixer(MixerCmd::SetOutput {
                strip,
                output: core_model::mixer::Output::Bus(i.reverb),
            }),
        );
    });
    let o = reverb_deleted(&t);
    let p = plan_doc(&t, &theirs_doc_, &o);
    assert_eq!(p.preview.added, 1, "{:#?}", p.decisions);
    let mut details = item(&p.report, ItemKind::Conflict, DELETED_HERE)
        .details
        .clone();
    details.sort();
    assert_eq!(
        details,
        vec![
            "Strip 'Lead': output to 'Reverb'".to_owned(),
            "Strip 'Lead': send to 'Reverb'".to_owned(),
        ]
    );
    let (h, _) = apply_on(&o, &p);
    assert!(has_lane(h.doc(), seq, "Lead"));
    assert!(
        !has_bus(h.doc(), "Reverb"),
        "a bus deleted here stays deleted"
    );
    let dest = lane(h.doc(), seq, "Lead").midi_dest.unwrap();
    let lead = h
        .doc()
        .mixer()
        .strip(
            h.doc()
                .mixer()
                .find_kind(StripKind::Instrument(dest))
                .unwrap(),
        )
        .unwrap();
    assert!(lead.sends.is_empty());
    assert_eq!(lead.output, core_model::mixer::Output::Master);
}

fn no_fade() -> Fade {
    Fade {
        length_frames: 0,
        shape: FadeShape::Linear,
    }
}

/// Sequence "Song" with an audio track "Keys": region A of media "loop"
/// at 0, region B of it at bar 3 with −4 dB of gain, and sequence "Clip"
/// (region C of media "hit") used at bar 5. The events of A, B and the
/// use.
fn regions_fixture() -> (Document, SeqId, [EventId; 3]) {
    let mut b = Builder::new();
    let (seq, keys) = b.sequence("Song", "Keys", None).unwrap();
    let lp = common::merge::media(&mut b, "loop", 48_000, 2);
    let hit = common::merge::media(&mut b, "hit", 24_000, 2);
    let a = b
        .region(seq, keys, 0, lp, 0, 48_000, 0.0, no_fade(), no_fade())
        .unwrap();
    let r = b
        .region(
            seq,
            keys,
            2 * BAR,
            lp,
            0,
            48_000,
            -4.0,
            no_fade(),
            no_fade(),
        )
        .unwrap();
    let (clip, cl) = b.sequence("Clip", "Clip", None).unwrap();
    b.region(clip, cl, 0, hit, 0, 24_000, 0.0, no_fade(), no_fade())
        .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq: clip,
        length: SeqLength::Locked(Tick(BAR)),
    }))
    .unwrap();
    let u = b.reference(seq, keys, 4 * BAR, clip, 1, 0).unwrap();
    (b.finish(), seq, [a, r, u])
}

/// `doc` edited by `edit` (for a project other than the fixture).
fn changed(doc: &Document, edit: impl FnOnce(&mut History)) -> Document {
    let mut h = History::new(doc.clone());
    edit(&mut h);
    h.doc().clone()
}

/// The doc with events `events` moved `delta` later.
fn moved(t: &Trip, events: &[EventId], delta: i64) -> Document {
    changed(&t.base_doc, |h| {
        run(
            h,
            Command::Event(EventCmd::Move {
                events: events.to_vec(),
                delta: Tick(delta),
                to_lane: None,
            }),
        )
    })
}

/// Region `e`'s (tick, media, gain) in `doc`.
fn region_at(doc: &Document, e: EventId) -> (i64, MediaId, f32) {
    let (ev, _) = doc.event(e).unwrap();
    match ev.body {
        EventBody::AudioRegion { media, gain_db, .. } => (ev.tick.0, media, gain_db),
        _ => panic!("not a region"),
    }
}

/// I5 (fix round 2, ruling 1): theirs moves region A only, and its audio
/// comes back resampled (other hashes and names), so only the `vz:e`
/// tags pair regions: the region inside the use by its own event. A is
/// edited in place; B and the use of "Clip" keep their events, the use is
/// still a use with ours' media, and nothing is unrolled.
#[test]
fn untouched_regions_and_uses_keep_their_events() {
    let (doc, seq, [a, b, u]) = regions_fixture();
    let t = trip(&doc, seq);
    if !tagged() {
        // Only the tags pair resampled, renamed audio.
        return same_resampled_file_twice(&t, &moved(&t, &[a], BAR), "Keys", &[]);
    }
    let th = theirs_resampled(&t, &moved(&t, &[a], BAR));
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    assert!(
        p.decisions
            .iter()
            .any(|d| d.unit == Unit::Regions && d.outcome == Outcome::Theirs),
        "{:#?}",
        p.decisions
    );
    let before_b = region_at(&t.base_doc, b);
    let media_before = t.base_doc.media().len();
    let (h, report) = apply_on(&t.base_doc, &p);
    assert_eq!(region_at(h.doc(), b), before_b, "B keeps its event");
    let keys = lane(h.doc(), seq, "Keys").active().unwrap();
    let use_ = keys.events.values().find(|e| e.id == u).expect("the use");
    assert!(matches!(use_.body, EventBody::Reference { .. }));
    let (tick_a, media_a, _) = region_at(h.doc(), a);
    assert_eq!(tick_a, BAR, "A is edited in place");
    assert_eq!(media_a, region_at(&t.base_doc, a).1, "ours' media");
    assert_eq!(h.doc().media().len(), media_before, "no media added");
    let ticks: Vec<i64> = keys.events.values().map(|e| e.tick.0).collect();
    assert_eq!(ticks, vec![BAR, 2 * BAR, 4 * BAR]);
    assert!(!has(&report, text::LOOPS_UNROLLED));
}

/// `doc` returned as `theirs_doc` does, its audio "resampled" by the other
/// DAW: every converted file has another content hash and name, so only
/// the `vz:e` tags pair its regions with ours'.
fn theirs_resampled(t: &Trip, doc: &Document) -> Theirs {
    theirs_with(t, doc, |media| {
        for m in media.values_mut() {
            if let MediaSource::Online(items) = m {
                for i in items {
                    i.content_hash = format!("{}-resampled", i.content_hash);
                    i.name = format!("{} resampled", i.name);
                    i.original_name = format!("{} resampled", i.original_name);
                    i.path = i.path.replace(".caf", " resampled.caf");
                }
            }
        }
    })
}

/// I5: after a merge that moved A, a second file from the same export
/// (A and B moved, its audio resampled) still pairs B by its original
/// event: theirs' version (the conflict take) plays B with ours' media
/// and gain (Ruling 11).
#[test]
fn second_merge_pairs_regions_by_their_original_events() {
    let (doc, seq, [a, b, _]) = regions_fixture();
    let t = trip(&doc, seq);
    if !tagged() {
        // Only the tags pair resampled, renamed audio.
        return same_resampled_file_twice(&t, &moved(&t, &[a, b], BAR), "Keys", &[]);
    }
    let (h, _) = apply_on(
        &t.base_doc,
        &plan_doc(&t, &moved(&t, &[a], BAR), &t.base_doc),
    );
    let once = h.doc().clone();
    let (_, ours_media, ours_gain) = region_at(&once, b);
    assert_eq!(ours_gain, -4.0);

    let second = theirs_resampled(&t, &moved(&t, &[a, b], BAR));
    let p = plan(t.snapshot.clone(), second, &once, chosen(), &never()).expect("plans");
    assert!(
        p.decisions
            .iter()
            .any(|d| d.unit == Unit::Regions && d.outcome == Outcome::Conflict),
        "{:#?}",
        p.decisions
    );
    let (h, _) = apply_history(h, &p, DATE);
    let returned = take(lane(h.doc(), seq, "Keys"), "Returned 2026-11-13");
    let b_there = returned
        .events
        .values()
        .find(|e| e.tick.0 == 3 * BAR)
        .unwrap_or_else(|| panic!("{returned:#?}"));
    match b_there.body {
        EventBody::AudioRegion { media, gain_db, .. } => {
            assert_eq!(media, ours_media, "ours' media");
            assert_eq!(gain_db, ours_gain, "ours' gain");
        }
        _ => panic!("not a region"),
    }
}

/// M2: an error midway through the merge (an invalid tempo map from
/// theirs, after the notes, strips and markers ran), then `abort`, leaves
/// the project exactly as it was.
#[test]
fn an_error_midway_then_abort_restores_the_project() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, strip(), |p, _| owners_check(p));
    let mut p = plan_of(&t, &path, &t.base_doc);
    p.theirs.projection.tempo.clear();
    p.decisions.push(Decision {
        unit: Unit::Tempo,
        row: 0,
        outcome: Outcome::Theirs,
        what: "Tempo map".into(),
        ours: None,
        theirs: None,
    });
    let mut h = History::new(t.base_doc.clone());
    h.begin(LABEL).unwrap();
    let r = apply_merge(&mut h, &p, DATE, &mut Report::default());
    assert!(r.is_err(), "{r:?}");
    assert_ne!(
        projection(h.doc(), seq).unwrap(),
        projection(&t.base_doc, seq).unwrap(),
        "the edits before the error ran"
    );
    h.abort().unwrap();
    assert_eq!(
        projection(h.doc(), seq).unwrap(),
        projection(&t.base_doc, seq).unwrap()
    );
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&t.base_doc).unwrap());
    assert_eq!(h.undo_depth(), 0);
}

/// `doc` returned as `theirs_doc` does, every audio file offline in the
/// scratch import (no content hash).
fn theirs_offline(t: &Trip, doc: &Document) -> Theirs {
    theirs_with(t, doc, |media| {
        for m in media.values_mut() {
            *m = MediaSource::Offline {
                why: "unreadable".into(),
            };
        }
    })
}

/// M1: a regions conflict whose new region's media is offline in the
/// scratch import, merged twice: the second merge finds the take the
/// first made and adds none.
#[test]
fn offline_regions_conflict_take_is_not_made_twice() {
    let (doc, seq, [a, b, _]) = regions_fixture();
    let t = trip(&doc, seq);
    // Theirs adds a region; ours moves B.
    let theirs_doc_ = changed(&t.base_doc, |h| {
        let (_, media, _) = region_at(h.doc(), a);
        let keys = lane(h.doc(), seq, "Keys").id;
        run(
            h,
            Command::Event(EventCmd::Insert {
                seq,
                lane: keys,
                events: vec![region(media, 6 * BAR)],
            }),
        );
    });
    let o = changed(&t.base_doc, |h| {
        run(
            h,
            Command::Event(EventCmd::Move {
                events: vec![b],
                delta: Tick(BAR),
                to_lane: None,
            }),
        )
    });
    let p = plan(
        t.snapshot.clone(),
        theirs_offline(&t, &theirs_doc_),
        &o,
        chosen(),
        &never(),
    )
    .expect("plans");
    assert!(
        p.decisions
            .iter()
            .any(|d| d.unit == Unit::Regions && d.outcome == Outcome::Conflict),
        "{:#?}",
        p.decisions
    );
    let (h, _) = apply_on(&o, &p);
    let once = h.doc().clone();
    assert_eq!(lane(&once, seq, "Keys").takes.len(), 2);
    let label = h.undo_label().map(str::to_owned);
    let again = plan(
        t.snapshot.clone(),
        theirs_offline(&t, &theirs_doc_),
        &once,
        chosen(),
        &never(),
    )
    .expect("plans");
    let (h, report) = apply_history(h, &again, DATE);
    assert_eq!(lane(h.doc(), seq, "Keys").takes.len(), 2);
    assert_eq!(h.undo_label().map(str::to_owned), label);
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&once).unwrap());
    assert_eq!(
        item(&report, ItemKind::Conflict, CONFLICT_TAKE).details,
        vec!["Keys: take \u{201c}Returned 2026-11-13\u{201d}".to_owned()]
    );
}

/// Coordinator ruling on fix round 1's concern 2: Keys plays into bus
/// "Sub" (base and ours); theirs moves it to Reverb, which ours deleted.
/// VizDAW wins: Keys still plays into Sub, listed under `DELETED_HERE`.
#[test]
fn matched_output_to_a_bus_deleted_here_keeps_ours() {
    let (doc, seq) = fixture();
    let doc = changed(&doc, |h| {
        let r = h
            .execute(
                &Command::Mixer(MixerCmd::AddBus {
                    name: "Sub".into(),
                    index: None,
                }),
                &ViewFilter::default(),
            )
            .unwrap();
        let sub = created(&r, |id| match id {
            AnyId::Strip(s) => Some(*s),
            _ => None,
        });
        let keys = keys_strip(h.doc(), seq);
        run(
            h,
            Command::Mixer(MixerCmd::SetOutput {
                strip: keys,
                output: core_model::mixer::Output::Bus(sub),
            }),
        );
    });
    let t = trip(&doc, seq);
    let theirs_doc_ = edited(&t, &t.base_doc, |h, i| {
        let keys = keys_strip(h.doc(), i.seq);
        run(
            h,
            Command::Mixer(MixerCmd::SetOutput {
                strip: keys,
                output: core_model::mixer::Output::Bus(i.reverb),
            }),
        );
    });
    let o = reverb_deleted(&t);
    let before = o.mixer().strip(keys_strip(&o, seq)).unwrap().output;
    assert!(matches!(before, core_model::mixer::Output::Bus(_)));
    let p = plan_doc(&t, &theirs_doc_, &o);
    assert_eq!(p.preview.applied, 0, "{:#?}", p.decisions);
    assert_eq!(
        item(&p.report, ItemKind::Conflict, DELETED_HERE).details,
        vec!["Strip 'Keys': output to 'Reverb'".to_owned()]
    );
    let (h, _) = apply_on(&o, &p);
    let keys = h.doc().mixer().strip(keys_strip(h.doc(), seq)).unwrap();
    assert_eq!(keys.output, before, "ours' output stays");
    assert!(!has_bus(h.doc(), "Reverb"));
    assert_eq!(h.undo_depth(), 0);
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&o).unwrap());
}

// ------------------------------------------------- fix round 2

/// What a second merge of the same file must leave alone: the document
/// (JSON), the undo label and depth, the takes and the media.
struct Kept {
    json: String,
    label: Option<String>,
    depth: usize,
    takes: usize,
    media: usize,
}

fn kept(h: &History, seq: SeqId, track: &str) -> Kept {
    Kept {
        json: to_json(h.doc()).unwrap(),
        label: h.undo_label().map(str::to_owned),
        depth: h.undo_depth(),
        takes: lane(h.doc(), seq, track).takes.len(),
        media: h.doc().media().len(),
    }
}

/// Ruling 2: `theirs_doc_` (an edited copy of `t.base_doc`), returned with
/// its audio resampled, merged into the export, then the same file merged
/// again: the second merge makes no step, no take and no media item, and
/// the project is byte-identical. The regions theirs changed keep their
/// events (`changed`).
///
/// Without tags (the stripped variants) nothing pairs resampled, renamed
/// audio (Ruling 4: by tag, else media plus position), so each of theirs'
/// regions is new plus deleted and keeps no event of ours; the second
/// merge still adds nothing.
fn same_resampled_file_twice(t: &Trip, theirs_doc_: &Document, track: &str, changed: &[EventId]) {
    let seq = t.seq;
    let p = plan(
        t.snapshot.clone(),
        theirs_resampled(t, theirs_doc_),
        &t.base_doc,
        chosen(),
        &never(),
    )
    .expect("plans");
    assert!(
        p.decisions
            .iter()
            .any(|d| d.unit == Unit::Regions && d.outcome == Outcome::Theirs),
        "{:#?}",
        p.decisions
    );
    let (h, _) = apply_on(&t.base_doc, &p);
    assert!(rules::validate(h.doc()).is_empty());
    for e in changed.iter().filter(|_| tagged()) {
        assert!(h.doc().event(*e).is_ok(), "{e:?} keeps its event");
    }
    let once = kept(&h, seq, track);
    let again = plan(
        t.snapshot.clone(),
        theirs_resampled(t, theirs_doc_),
        h.doc(),
        chosen(),
        &never(),
    )
    .expect("plans");
    assert!(
        again
            .decisions
            .iter()
            .all(|d| matches!(d.outcome, Outcome::Same | Outcome::Ours)),
        "{:#?}",
        again.decisions
    );
    let (h, _) = apply_history(h, &again, DATE);
    let twice = kept(&h, seq, track);
    assert_eq!(twice.label, once.label, "no step");
    assert_eq!(twice.depth, once.depth, "no step");
    assert_eq!(twice.takes, once.takes, "no take");
    assert_eq!(twice.media, once.media, "no media item");
    assert_eq!(twice.json, once.json, "byte-identical");
}

#[test]
fn a_moved_region_merged_twice_from_resampled_audio_adds_nothing() {
    let (doc, seq, [a, _, _]) = regions_fixture();
    let t = trip(&doc, seq);
    same_resampled_file_twice(&t, &moved(&t, &[a], BAR), "Keys", &[a]);
}

/// The fixture's Keys regions: the earlier one, and the later one
/// crossfaded into it.
fn keys_regions(doc: &Document, seq: SeqId) -> [EventId; 2] {
    let ids: Vec<EventId> = lane(doc, seq, "Keys")
        .active()
        .unwrap()
        .events
        .values()
        .filter(|e| matches!(e.body, EventBody::AudioRegion { .. }))
        .map(|e| e.id)
        .collect();
    [ids[0], ids[1]]
}

/// `doc` with region `e` trimmed by 12 000 frames at its end (it still
/// overlaps its partner).
fn trimmed(doc: &Document, e: EventId) -> Document {
    changed(doc, |h| {
        run(
            h,
            Command::Audio(AudioCmd::Trim {
                event: e,
                front_frames: 0,
                back_frames: 12_000,
            }),
        )
    })
}

/// Whether `e` is still crossfaded in `doc`; without tags (where `e` came
/// back as a new region), whether some region of "Keys" is.
fn crossfaded(doc: &Document, seq: SeqId, e: EventId) -> bool {
    if tagged() {
        return crossfade_of(doc, e);
    }
    lane(doc, seq, "Keys")
        .active()
        .unwrap()
        .events
        .values()
        .any(|ev| crossfade_of(doc, ev.id))
}

fn crossfade_of(doc: &Document, e: EventId) -> bool {
    matches!(
        doc.event(e).unwrap().0.body,
        EventBody::AudioRegion {
            crossfade: true,
            ..
        }
    )
}

#[test]
fn crossfaded_pair_with_its_earlier_member_changed_merges_twice_to_nothing() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let [first, second] = keys_regions(&t.base_doc, seq);
    same_resampled_file_twice(&t, &trimmed(&t.base_doc, first), "Keys", &[first, second]);
    // The crossfade survived the merge.
    let p = plan(
        t.snapshot.clone(),
        theirs_resampled(&t, &trimmed(&t.base_doc, first)),
        &t.base_doc,
        chosen(),
        &never(),
    )
    .unwrap();
    let (h, report) = apply_on(&t.base_doc, &p);
    assert!(crossfaded(h.doc(), seq, second));
    assert!(!has(&report, core_io::dawproject::import::CROSSFADES_PLAIN));
}

#[test]
fn crossfaded_pair_with_its_later_member_changed_merges_twice_to_nothing() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let [first, second] = keys_regions(&t.base_doc, seq);
    same_resampled_file_twice(&t, &trimmed(&t.base_doc, second), "Keys", &[first, second]);
    let p = plan(
        t.snapshot.clone(),
        theirs_resampled(&t, &trimmed(&t.base_doc, second)),
        &t.base_doc,
        chosen(),
        &never(),
    )
    .unwrap();
    let (h, _) = apply_on(&t.base_doc, &p);
    assert!(crossfaded(h.doc(), seq, second));
}

/// Task 7 (found by the edit scripts): theirs moves the later region of
/// Keys' crossfaded pair a beat later, still crossfading. Without tags the
/// moved region is new (its old place deleted): inserted beside the pair's
/// earlier region, which stays, it keeps its crossfade.
#[test]
fn a_moved_crossfaded_region_keeps_its_crossfade() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let [_, second] = keys_regions(&t.base_doc, seq);
    let p = plan_doc(&t, &moved(&t, &[second], Q), &t.base_doc);
    let (h, _) = apply_on(&t.base_doc, &p);
    let regions: Vec<(i64, bool)> = lane(h.doc(), seq, "Keys")
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::AudioRegion { crossfade, .. } => Some((e.tick.0, crossfade)),
            _ => None,
        })
        .collect();
    assert_eq!(regions, vec![(0, false), (2 * Q, true)]);
    assert!(rules::validate(h.doc()).is_empty());
    merges_twice_to_nothing(&t, &moved(&t, &[second], Q), h);
}

/// Task 7 (found by the edit scripts): theirs sets the tempo to 600 BPM,
/// so Keys' regions (in beats in the file) come back shorter. They are
/// placed before the tempo map is: under ours' 120 BPM the pair no longer
/// overlaps, so its crossfade is settled away, then set again once the
/// tempo is theirs. Merged twice, nothing more happens.
#[test]
fn a_faster_tempo_keeps_theirs_crossfades() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, strip(), |p, _| {
        p.tempo.as_mut().expect("a tempo").value = Some(600.0);
    });
    let p = plan_of(&t, &path, &t.base_doc);
    let (h, _) = apply_on(&t.base_doc, &p);
    let flags: Vec<bool> = lane(h.doc(), seq, "Keys")
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::AudioRegion { crossfade, .. } => Some(crossfade),
            _ => None,
        })
        .collect();
    assert_eq!(flags, vec![false, true]);
    assert!(rules::validate(h.doc()).is_empty());
    let json = to_json(h.doc()).unwrap();
    let again = plan_of(&t, &path, h.doc());
    let (h2, _) = apply_on(h.doc(), &again);
    assert_eq!(h2.undo_depth(), 0, "{:#?}", again.decisions);
    assert_eq!(to_json(h2.doc()).unwrap(), json);
}

/// Task 7: as above, with ours' first Keys region trimmed, so the regions
/// conflict: theirs' pair goes on the take "Returned 2026-11-13", which
/// keeps its crossfade, so the same file merged again finds the take.
#[test]
fn a_faster_tempo_keeps_theirs_crossfades_on_the_take() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let [first, _] = keys_regions(&t.base_doc, seq);
    let ours = trimmed(&t.base_doc, first);
    let path = return_file(&t, strip(), |p, _| {
        p.tempo.as_mut().expect("a tempo").value = Some(600.0);
    });
    let p = plan_of(&t, &path, &ours);
    let (h, _) = apply_on(&ours, &p);
    let returned = take(lane(h.doc(), seq, "Keys"), "Returned 2026-11-13");
    let flags: Vec<bool> = returned
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::AudioRegion { crossfade, .. } => Some(crossfade),
            _ => None,
        })
        .collect();
    assert_eq!(flags, vec![false, true]);
    let json = to_json(h.doc()).unwrap();
    let again = plan_of(&t, &path, h.doc());
    let (h2, _) = apply_on(h.doc(), &again);
    assert_eq!(h2.undo_depth(), 0, "{:#?}", again.decisions);
    assert_eq!(to_json(h2.doc()).unwrap(), json);
}

/// Minor: theirs moves region A on a track with its own length. The
/// regions are a take conflict: ours' track is untouched and theirs' A is
/// on the take "Returned 2026-11-13".
#[test]
fn regions_on_a_track_with_its_own_length_become_a_take() {
    let (doc, seq, [a, _, _]) = regions_fixture();
    let doc = changed(&doc, |h| {
        let keys = lane(h.doc(), seq, "Keys").id;
        run(
            h,
            Command::Lane(LaneCmd::SetLength {
                seq,
                lane: keys,
                length: Some(Tick(8 * BAR)),
                looped: false,
                locked: false,
            }),
        )
    });
    let t = trip(&doc, seq);
    let p = plan_doc_any(&t, &moved(&t, &[a], BAR), &t.base_doc);
    let d = p
        .decisions
        .iter()
        .find(|d| d.unit == Unit::Regions)
        .unwrap_or_else(|| panic!("{:#?}", p.decisions));
    assert_eq!(d.outcome, Outcome::Conflict);
    assert_eq!(p.preview.applied, 0);
    let before = lane(&t.base_doc, seq, "Keys").clone();
    let (h, report) = apply_on(&t.base_doc, &p);
    let after = lane(h.doc(), seq, "Keys");
    assert_eq!(after.active(), before.active());
    let returned = take(after, "Returned 2026-11-13");
    assert!(
        returned
            .events
            .values()
            .any(|e| e.tick.0 == BAR && matches!(e.body, EventBody::AudioRegion { .. })),
        "{returned:#?}"
    );
    assert_eq!(
        item(&report, ItemKind::Conflict, CONFLICT_TAKE).details,
        vec!["Keys: take \u{201c}Returned 2026-11-13\u{201d}".to_owned()]
    );
}

/// `plan_doc` for a project other than the fixture.
fn plan_doc_any(t: &Trip, doc: &Document, ours: &Document) -> MergePlan {
    plan(
        t.snapshot.clone(),
        theirs_doc(t, doc),
        ours,
        chosen(),
        &never(),
    )
    .expect("plans")
}

/// Minor: theirs adds a level curve to "Loop", a track with its own
/// length. It is not applied: listed under `NOT_APPLIED` with the reason.
#[test]
fn automation_on_a_track_with_its_own_length_is_listed_with_the_reason() {
    let notes = [(0, 60), (BAR, 60)];
    let (doc, seq) = with_loop_track(&notes, |h, seq, lane| {
        run(
            h,
            Command::Lane(LaneCmd::SetLength {
                seq,
                lane,
                length: Some(Tick(6 * BAR)),
                looped: false,
                locked: false,
            }),
        )
    });
    let t = trip(&doc, seq);
    let theirs_doc_ = changed(&t.base_doc, |h| {
        let l = lane(h.doc(), seq, "Loop");
        let (id, dest) = (l.id, l.midi_dest.unwrap());
        let strip = h
            .doc()
            .mixer()
            .find_kind(StripKind::Instrument(dest))
            .unwrap();
        let point = |tick: i64, value: f64| {
            EventSpec::new(
                Tick(tick),
                EventBody::Automation {
                    target: AutomationTarget::StripFader(strip),
                    value,
                },
            )
        };
        run(
            h,
            Command::Event(EventCmd::Insert {
                seq,
                lane: id,
                events: vec![point(0, 0.5), point(BAR, 0.25)],
            }),
        );
    });
    let p = plan_doc_any(&t, &theirs_doc_, &t.base_doc);
    let d = p
        .decisions
        .iter()
        .find(|d| matches!(d.unit, Unit::Automation(_)))
        .unwrap_or_else(|| panic!("{:#?}", p.decisions));
    assert_eq!(d.outcome, Outcome::Conflict);
    assert_eq!(p.preview.applied, 0, "{:#?}", p.decisions);
    let detail = &item(&p.report, ItemKind::Conflict, NOT_APPLIED).details;
    assert_eq!(
        detail,
        &vec![
            "Track 'Loop': level automation (the track plays its own length): \
             here 0 points, there 2 points"
                .to_owned()
        ]
    );
    let (h, _) = apply_on(&t.base_doc, &p);
    assert_eq!(h.undo_depth(), 0);
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&t.base_doc).unwrap());
}

// ------------------------------------------------- Task 6b: shared sequences (Ruling 8)

/// The sequence named `name`.
#[track_caller]
fn seq_named(doc: &Document, name: &str) -> SeqId {
    doc.sequences()
        .values()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no sequence {name}"))
        .id
}

/// Sequence `s`'s first lane's active-take notes: (event, tick, key).
fn seq_notes(doc: &Document, s: SeqId) -> Vec<(EventId, i64, u8)> {
    doc.sequence(s).unwrap().lanes[0]
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Note { pitch, .. } => Some((e.id, e.tick.0, pitch)),
            _ => None,
        })
        .collect()
}

/// [`seq_notes`] without the events.
fn seq_keys(doc: &Document, s: SeqId) -> Vec<(i64, u8)> {
    seq_notes(doc, s)
        .into_iter()
        .map(|(_, t, k)| (t, k))
        .collect()
}

/// The reference uses on track `name` of `seq`: (event, tick, target).
fn uses_on(doc: &Document, seq: SeqId, name: &str) -> Vec<(EventId, i64, SeqId)> {
    lane(doc, seq, name)
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Reference { seq, .. } => Some((e.id, e.tick.0, seq)),
            _ => None,
        })
        .collect()
}

/// Track `track`'s played notes in `p`: (tick, key), sorted.
fn played_on(p: &Projection, track: usize) -> Vec<(i64, u8)> {
    let mut n: Vec<(i64, u8)> = p.tracks[track]
        .notes_played
        .iter()
        .map(|n| (n.tick, n.pitch))
        .collect();
    n.sort();
    n
}

/// The detail list of the report line `what` (kind `kind`).
#[track_caller]
fn details(r: &Report, kind: ItemKind, what: &str) -> Vec<String> {
    item(r, kind, what).details.clone()
}

/// Riff's second note (Q, key 43) moved `delta` later, in `doc`'s
/// sequence `riff`.
fn move_riff_note(h: &mut History, riff: SeqId, index: usize, delta: i64) {
    let e = seq_notes(h.doc(), riff)[index].0;
    run(
        h,
        Command::Event(EventCmd::Move {
            events: vec![e],
            delta: Tick(delta),
            to_lane: None,
        }),
    );
}

/// `doc` with Bass's second use of "Riff" (bar 4) set to `loops` passes
/// and `transpose`.
fn with_second_use(doc: Document, seq: SeqId, loops: u32, transpose: i8) -> Document {
    changed(&doc, |h| {
        let (u, _, _) = uses_on(h.doc(), seq, "Bass")[1];
        run(
            h,
            Command::Reference(ReferenceCmd::SetParams {
                event: u,
                length: None,
                loop_count: LoopCount::Count(loops),
                transpose,
            }),
        );
    })
}

/// The Ruling 8 lines of `r` (private copies, uses unrolled, edits that
/// play elsewhere), with their counts and details.
fn applied_lines(r: &Report) -> Vec<(ItemKind, String, u64, Vec<String>)> {
    r.items
        .iter()
        .filter(|i| {
            [
                text::PRIVATE_COPIES,
                text::LOOPS_UNROLLED,
                text::USES_AS_NOTES,
                text::SHARED_ELSEWHERE,
            ]
            .contains(&i.what.as_str())
        })
        .map(|i| (i.kind, i.what.clone(), i.count, i.details.clone()))
        .collect()
}

/// Fix round 1 (I1): the preview (the plan's report) lists the same
/// Ruling 8 lines as the merge's report, and at least one.
#[track_caller]
fn preview_lists_them(p: &MergePlan, report: &Report) {
    let planned = applied_lines(&p.report);
    assert!(!planned.is_empty(), "the preview lists the Ruling 8 lines");
    assert_eq!(planned, applied_lines(report));
}

/// The same file merged a second time changes nothing and adds no step.
fn merges_twice_to_nothing(t: &Trip, theirs_doc_: &Document, h: History) {
    let label = h.undo_label().map(str::to_owned);
    let depth = h.undo_depth();
    let json = to_json(h.doc()).unwrap();
    let p = plan(
        t.snapshot.clone(),
        theirs_doc(t, theirs_doc_),
        h.doc(),
        chosen(),
        &never(),
    )
    .expect("plans");
    assert!(
        p.decisions
            .iter()
            .all(|d| matches!(d.outcome, Outcome::Same | Outcome::Ours)),
        "{:#?}",
        p.decisions
    );
    let (h, _) = apply_history(h, &p, DATE);
    assert_eq!(h.undo_label().map(str::to_owned), label);
    assert_eq!(h.undo_depth(), depth);
    assert_eq!(to_json(h.doc()).unwrap(), json);
}

/// Ruling 8: theirs moves Riff's second note in both of its uses (bar 2
/// once, bar 4 twice). The edit goes into Riff, once: the note keeps its
/// event, no sequence is made and both uses stay uses of Riff. Riff also
/// plays from a keyboard slot, so `SHARED_ELSEWHERE` names it.
#[test]
fn shared_sequence_edited_the_same_way_edits_the_sequence() {
    let (doc, seq) = fixture();
    let doc = changed(&doc, |h| {
        let riff = seq_named(h.doc(), "Riff");
        run(
            h,
            Command::Sequence(SequenceCmd::SetKeyboardSlot {
                seq: riff,
                slot: Some(KeySlot {
                    key: "a".into(),
                    modifiers: 0,
                }),
            }),
        );
    });
    let t = trip(&doc, seq);
    let riff = seq_named(&t.base_doc, "Riff");
    let theirs_doc_ = changed(&t.base_doc, |h| move_riff_note(h, riff, 1, Q / 2));
    let th = theirs_doc(&t, &theirs_doc_);
    let want = played_on(&th.projection, 0);
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    let (h, report) = apply_on(&t.base_doc, &p);
    preview_lists_them(&p, &report);
    assert_eq!(h.undo_depth(), 1, "one undo step");
    assert_eq!(h.undo_label(), Some(LABEL));

    assert_eq!(h.doc().sequences().len(), t.base_doc.sequences().len());
    let before = seq_notes(&t.base_doc, riff);
    let after = seq_notes(h.doc(), riff);
    assert_eq!(
        after.iter().map(|n| n.0).collect::<Vec<_>>(),
        before.iter().map(|n| n.0).collect::<Vec<_>>(),
        "one edit: the note keeps its event"
    );
    assert_eq!(seq_keys(h.doc(), riff), vec![(0, 40), (Q + Q / 2, 43)]);
    let uses = uses_on(h.doc(), seq, "Bass");
    assert_eq!(uses, uses_on(&t.base_doc, seq, "Bass"), "both uses stay");
    assert!(uses.iter().all(|u| u.2 == riff));
    assert_eq!(played_on(&projection(h.doc(), seq).unwrap(), 0), want);
    assert_eq!(
        details(&report, ItemKind::Note, text::SHARED_ELSEWHERE),
        vec!["Sequence 'Riff'".to_owned()]
    );
    assert!(!has(&report, text::PRIVATE_COPIES));
    assert!(!has(&report, text::LOOPS_UNROLLED));
    merges_twice_to_nothing(&t, &theirs_doc_, h);
}

/// Ruling 8: theirs edits only Riff's second use (bar 4, two passes), as
/// a private copy of its own. That use now plays a new sequence made by
/// `MakePrivateCopy` ("Riff (copy)") holding the edit; the first use and
/// Riff itself are unchanged, and `PRIVATE_COPIES` names Riff.
#[test]
fn shared_uses_edited_differently_become_private_copies() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let riff = seq_named(&t.base_doc, "Riff");
    let theirs_doc_ = changed(&t.base_doc, |h| {
        let (u, _, _) = uses_on(h.doc(), seq, "Bass")[1];
        run(
            h,
            Command::Reference(ReferenceCmd::MakePrivateCopy { event: u }),
        );
        let copy = seq_named(h.doc(), "Riff (copy)");
        move_riff_note(h, copy, 1, Q / 2);
    });
    let th = theirs_doc(&t, &theirs_doc_);
    let want = played_on(&th.projection, 0);
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    let (h, report) = apply_on(&t.base_doc, &p);
    preview_lists_them(&p, &report);

    let before = uses_on(&t.base_doc, seq, "Bass");
    let after = uses_on(h.doc(), seq, "Bass");
    assert_eq!(after.len(), 2);
    assert_eq!(after[0], before[0], "the first use is unchanged");
    assert_eq!((after[1].0, after[1].1), (before[1].0, before[1].1));
    let copy = after[1].2;
    assert_ne!(copy, riff, "the second use plays a private copy");
    assert_eq!(h.doc().sequence(copy).unwrap().name, "Riff (copy)");
    assert_eq!(seq_keys(h.doc(), copy), vec![(0, 40), (Q + Q / 2, 43)]);
    assert_eq!(
        seq_notes(h.doc(), riff),
        seq_notes(&t.base_doc, riff),
        "Riff is unchanged"
    );
    assert_eq!(played_on(&projection(h.doc(), seq).unwrap(), 0), want);
    assert_eq!(
        details(&report, ItemKind::Changed, text::PRIVATE_COPIES),
        vec!["Sequence 'Riff'".to_owned()]
    );
    assert!(!has(&report, text::SHARED_ELSEWHERE));
    assert!(!has(&report, text::LOOPS_UNROLLED));
    merges_twice_to_nothing(&t, &theirs_doc_, h);
}

/// Ruling 8: Riff's second use loops 4 times (bars 4–7) and theirs moves
/// its second note in pass 3 only. The use cannot stay a use: it is
/// replaced by theirs' notes on the track, `LOOPS_UNROLLED` names it, and
/// Bass plays exactly as theirs' projection. Riff is unchanged.
#[test]
fn looped_use_edited_on_one_pass_becomes_notes() {
    let (doc, seq) = fixture();
    let doc = with_second_use(doc, seq, 4, 0);
    let t = trip(&doc, seq);
    let riff = seq_named(&t.base_doc, "Riff");
    let (u, at, _) = uses_on(&t.base_doc, seq, "Bass")[1];
    let theirs_doc_ = changed(&t.base_doc, |h| {
        run(h, Command::Reference(ReferenceCmd::Flatten { event: u }));
        let pass3 = lane(h.doc(), seq, "Bass")
            .active()
            .unwrap()
            .events
            .values()
            .find(|e| e.tick.0 == at + 2 * BAR + Q)
            .expect("pass 3's second note")
            .id;
        run(
            h,
            Command::Event(EventCmd::Move {
                events: vec![pass3],
                delta: Tick(Q / 2),
                to_lane: None,
            }),
        );
    });
    let th = theirs_doc(&t, &theirs_doc_);
    let want = played_on(&th.projection, 0);
    assert!(want.contains(&(at + 2 * BAR + Q + Q / 2, 43)));
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    let (h, report) = apply_on(&t.base_doc, &p);
    preview_lists_them(&p, &report);
    assert!(!has(&report, text::USES_AS_NOTES));

    let after = uses_on(h.doc(), seq, "Bass");
    assert_eq!(
        after,
        uses_on(&t.base_doc, seq, "Bass")[..1],
        "the use is gone"
    );
    assert_eq!(played_on(&projection(h.doc(), seq).unwrap(), 0), want);
    assert_eq!(
        details(&report, ItemKind::Changed, text::LOOPS_UNROLLED),
        vec!["Track 'Bass': use of 'Riff'".to_owned()]
    );
    assert_eq!(seq_notes(h.doc(), riff), seq_notes(&t.base_doc, riff));
    assert_eq!(h.doc().sequences().len(), t.base_doc.sequences().len());
    assert!(!has(&report, text::PRIVATE_COPIES));
    merges_twice_to_nothing(&t, &theirs_doc_, h);
}

/// Ruling 8: Riff's only use is transposed +2 (bar 4, twice); theirs moves
/// its first note (played at key 42). The edit goes into Riff at its own,
/// untransposed key 40.
#[test]
fn a_transposed_use_takes_its_edit_untransposed() {
    let (doc, seq) = fixture();
    let doc = changed(&with_second_use(doc, seq, 2, 2), |h| {
        let (first, _, _) = uses_on(h.doc(), seq, "Bass")[0];
        run(
            h,
            Command::Event(EventCmd::Delete {
                events: vec![first],
            }),
        );
    });
    let t = trip(&doc, seq);
    let riff = seq_named(&t.base_doc, "Riff");
    assert!(played_on(&t.snapshot.projection, 0).contains(&(3 * BAR, 42)));
    let theirs_doc_ = changed(&t.base_doc, |h| move_riff_note(h, riff, 0, Q / 2));
    let th = theirs_doc(&t, &theirs_doc_);
    let want = played_on(&th.projection, 0);
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    let (h, report) = apply_on(&t.base_doc, &p);

    assert_eq!(seq_keys(h.doc(), riff), vec![(Q / 2, 40), (Q, 43)]);
    assert_eq!(
        seq_notes(h.doc(), riff)[0].0,
        seq_notes(&t.base_doc, riff)[0].0,
        "the note keeps its event"
    );
    assert_eq!(
        uses_on(h.doc(), seq, "Bass"),
        uses_on(&t.base_doc, seq, "Bass")
    );
    assert_eq!(played_on(&projection(h.doc(), seq).unwrap(), 0), want);
    assert!(!has(&report, text::SHARED_ELSEWHERE));
    assert!(!has(&report, text::PRIVATE_COPIES));
    assert!(!has(&report, text::LOOPS_UNROLLED));
    assert!(!has(&report, text::USES_AS_NOTES));
    merges_twice_to_nothing(&t, &theirs_doc_, h);
}

/// Ruling 3: with a format-1 snapshot, uses are not refined: the edit of
/// `shared_uses_edited_differently_become_private_copies` makes no
/// private copy. The edited use becomes notes (Task 6's behaviour) and
/// `OLD_EXPORT` is listed.
#[test]
fn format_one_snapshots_merge_uses_as_notes() {
    let (doc, seq) = fixture();
    let mut t = trip(&doc, seq);
    t.snapshot.format = 1;
    t.snapshot.identity = None;
    let riff = seq_named(&t.base_doc, "Riff");
    let theirs_doc_ = changed(&t.base_doc, |h| {
        let (u, _, _) = uses_on(h.doc(), seq, "Bass")[1];
        run(
            h,
            Command::Reference(ReferenceCmd::MakePrivateCopy { event: u }),
        );
        let copy = seq_named(h.doc(), "Riff (copy)");
        move_riff_note(h, copy, 1, Q / 2);
    });
    let th = theirs_doc(&t, &theirs_doc_);
    let want = played_on(&th.projection, 0);
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    assert!(has(&p.report, text::OLD_EXPORT));
    let (h, report) = apply_on(&t.base_doc, &p);

    assert!(has(&report, text::OLD_EXPORT));
    assert_eq!(h.doc().sequences().len(), t.base_doc.sequences().len());
    assert!(!has(&report, text::PRIVATE_COPIES));
    let before = uses_on(&t.base_doc, seq, "Bass");
    assert_eq!(uses_on(h.doc(), seq, "Bass"), before[..1]);
    assert_eq!(seq_notes(h.doc(), riff), seq_notes(&t.base_doc, riff));
    assert_eq!(played_on(&projection(h.doc(), seq).unwrap(), 0), want);
    // A use that plays twice, edited the same on both passes: not a loop
    // edited on some passes (fix round 1, I4).
    assert_eq!(
        details(&report, ItemKind::Changed, text::USES_AS_NOTES),
        vec!["Track 'Bass': use of 'Riff'".to_owned()]
    );
    assert!(!has(&report, text::LOOPS_UNROLLED));
    preview_lists_them(&p, &report);
    merges_twice_to_nothing(&t, &theirs_doc_, h);
}

/// Task 7 carry-over (Task 6b re-review): with more uses than a line
/// keeps details for, the preview counts them all, as the merge's report
/// does. A format-1 snapshot turns each of the 52 uses theirs edited into
/// notes.
#[test]
fn preview_counts_past_the_details_kept() {
    use core_io::limits::MAX_DETAILS;
    let uses = MAX_DETAILS + 2;
    let mut b = Builder::new();
    let (dest, _) = b
        .plugin_dest("Bass Synth", common::merge::clap("org.example.Bass"))
        .unwrap();
    let (seq, bass) = b.sequence("Song", "Bass", Some(dest)).unwrap();
    let (riff, rl) = b.sequence("Riff", "Riff", Some(dest)).unwrap();
    b.notes(riff, rl, &[(0, 40, 0x8000, Q), (Q, 43, 0x9000, Q)])
        .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq: riff,
        length: SeqLength::Locked(Tick(BAR)),
    }))
    .unwrap();
    for k in 0..uses {
        b.reference(seq, bass, i64::try_from(k).unwrap() * BAR, riff, 1, 0)
            .unwrap();
    }
    let doc = b.finish();
    let mut t = trip(&doc, seq);
    t.snapshot.format = 1;
    t.snapshot.identity = None;
    let theirs_doc_ = changed(&t.base_doc, |h| move_riff_note(h, riff, 1, Q / 2));
    let th = theirs_doc(&t, &theirs_doc_);
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    let (_, report) = apply_on(&t.base_doc, &p);
    let real = item(&report, ItemKind::Changed, text::USES_AS_NOTES);
    assert_eq!(real.count, u64::try_from(uses).unwrap());
    assert_eq!(real.details.len(), MAX_DETAILS);
    preview_lists_them(&p, &report);
}

/// Task 7: theirs renames "Bass" to "Low" and moves marker "Chorus" by a
/// beat. With its tags or trusted IDs the track is renamed and the marker
/// moved. Without either, a renamed track still pairs by order (the only
/// note track left of its kind) and is renamed, but the marker, matched by
/// name and tick, is new plus deleted (Ruling 4), which lands it at the
/// same place.
#[test]
fn a_renamed_track_and_a_moved_marker() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, strip(), |p, _| {
        tracks(p)
            .find(|x| x.named.name.as_deref() == Some("Bass"))
            .expect("Bass")
            .named
            .name = Some("Low".into());
        p.arrangement
            .as_mut()
            .expect("an arrangement")
            .markers
            .iter_mut()
            .find(|m| m.named.name.as_deref() == Some("Chorus"))
            .expect("Chorus")
            .time += 1.0;
    });
    let p = plan_of(&t, &path, &t.base_doc);
    let markers: Vec<Outcome> = p
        .decisions
        .iter()
        .filter(|d| d.unit == Unit::Marker)
        .map(|d| d.outcome)
        .collect();
    if tagged() {
        assert_eq!(markers, vec![Outcome::Theirs], "{:#?}", p.decisions);
    } else {
        assert_eq!(
            markers,
            vec![Outcome::Removed, Outcome::Added],
            "{:#?}",
            p.decisions
        );
    }
    let (h, _) = apply_on(&t.base_doc, &p);
    assert!(has_lane(h.doc(), seq, "Low"), "renamed");
    assert!(!has_lane(h.doc(), seq, "Bass"));
    assert_eq!(
        markers_of(h.doc(), seq),
        vec![(0, "Verse".to_owned()), (2 * BAR + Q, "Chorus".to_owned())]
    );
    assert!(rules::validate(h.doc()).is_empty());
}

/// Task 7 carry-over (Task 6b re-review): the apply (and so the preview's
/// dry run of it) stops at its next track once cancelled; aborting then
/// restores the project.
#[test]
fn a_cancelled_apply_stops_and_aborts_cleanly() {
    use core_io::dawproject::merge::apply::{CANCELLED, apply_merge_until};
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let path = return_file(&t, strip(), |p, _| owners_check(p));
    let p = plan_of(&t, &path, &t.base_doc);
    let before = to_json(&t.base_doc).unwrap();
    let mut h = History::new(t.base_doc.clone());
    h.begin(LABEL).unwrap();
    let e = apply_merge_until(
        &mut h,
        &p,
        DATE,
        &mut Report::default(),
        &AtomicBool::new(true),
    )
    .expect_err("cancelled");
    assert!(e.to_string().contains(CANCELLED), "{e}");
    h.abort().unwrap();
    assert_eq!(to_json(h.doc()).unwrap(), before);

    // A plan cancelled from the start answers so.
    let r = plan(
        t.snapshot.clone(),
        theirs_of(&t, &path),
        &t.base_doc,
        chosen(),
        &AtomicBool::new(true),
    );
    assert!(matches!(r, Err(core_io::IoError::Cancelled)));
}

/// Task 6 carry-over: theirs moves region C inside "Clip" (its only use,
/// at bar 5). The region is edited in Clip, in place: it keeps its event
/// and ours' media, and the use stays a use (nothing unrolled).
#[test]
fn a_changed_region_inside_a_use_edits_its_sequence() {
    let (doc, seq, [_, _, u]) = regions_fixture();
    let t = trip(&doc, seq);
    let clip = seq_named(&t.base_doc, "Clip");
    let c = t.base_doc.sequence(clip).unwrap().lanes[0]
        .active()
        .unwrap()
        .events
        .values()
        .next()
        .unwrap()
        .id;
    if !tagged() {
        // Only the tags pair resampled, renamed audio: the use's region is
        // new plus deleted, so the use becomes theirs' region.
        return same_resampled_file_twice(&t, &moved(&t, &[c], Q), "Keys", &[]);
    }
    let th = theirs_resampled(&t, &moved(&t, &[c], Q));
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    let media_before = t.base_doc.media().len();
    let (h, report) = apply_on(&t.base_doc, &p);

    let keys = lane(h.doc(), seq, "Keys").active().unwrap();
    let use_ = keys.events.values().find(|e| e.id == u).expect("the use");
    assert!(matches!(use_.body, EventBody::Reference { seq: s, .. } if s == clip));
    let (tick, media, _) = region_at(h.doc(), c);
    assert_eq!(tick, Q, "C is edited in Clip, in place");
    assert_eq!(media, region_at(&t.base_doc, c).1, "ours' media");
    assert_eq!(h.doc().media().len(), media_before, "no media added");
    assert_eq!(h.doc().sequences().len(), t.base_doc.sequences().len());
    assert!(!has(&report, text::LOOPS_UNROLLED));
    assert!(!has(&report, text::PRIVATE_COPIES));
}

/// Ruling 8 with a nested use: sequence "Verse" uses Riff and is used on
/// Bass at bar 6, beside Riff's own two uses. Theirs edits Riff's second
/// note, so every play of it changes. Riff also plays inside Verse's use,
/// so it is not edited itself: its own uses get private copies, and
/// Verse's use (whose notes are Riff's, nested) becomes notes, under
/// `USES_AS_NOTES` (it plays once). Bass plays
/// exactly as theirs; Riff and Verse are unchanged.
#[test]
fn a_sequence_played_inside_another_use_is_not_edited_itself() {
    let (doc, seq) = fixture();
    let doc = changed(&doc, |h| {
        let riff = seq_named(h.doc(), "Riff");
        let r = h
            .execute(
                &Command::Sequence(SequenceCmd::New {
                    name: "Verse".into(),
                }),
                &ViewFilter::default(),
            )
            .unwrap();
        let verse = created(&r, |id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        });
        let r = h
            .execute(
                &Command::Lane(LaneCmd::Add {
                    seq: verse,
                    name: "Verse".into(),
                    index: None,
                }),
                &ViewFilter::default(),
            )
            .unwrap();
        let vl = created(&r, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        });
        for (s, l, tick, target) in [
            (verse, vl, 0, riff),
            (seq, lane(h.doc(), seq, "Bass").id, 5 * BAR, verse),
        ] {
            run(
                h,
                Command::Reference(ReferenceCmd::Insert {
                    seq: s,
                    lane: l,
                    tick: Tick(tick),
                    target,
                    length: None,
                    loop_count: LoopCount::Count(1),
                    transpose: 0,
                }),
            );
        }
    });
    let t = trip(&doc, seq);
    let riff = seq_named(&t.base_doc, "Riff");
    let verse = seq_named(&t.base_doc, "Verse");
    let theirs_doc_ = changed(&t.base_doc, |h| move_riff_note(h, riff, 1, Q / 2));
    let th = theirs_doc(&t, &theirs_doc_);
    let want = played_on(&th.projection, 0);
    assert!(want.contains(&(5 * BAR + Q + Q / 2, 43)), "{want:?}");
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    let (h, report) = apply_on(&t.base_doc, &p);

    assert_eq!(played_on(&projection(h.doc(), seq).unwrap(), 0), want);
    assert_eq!(seq_notes(h.doc(), riff), seq_notes(&t.base_doc, riff));
    let copies = h
        .doc()
        .sequences()
        .values()
        .filter(|s| s.name == "Riff (copy)")
        .count();
    assert_eq!(copies, 2, "each of Riff's own uses gets a private copy");
    assert_eq!(
        h.doc().sequence(verse).unwrap(),
        t.base_doc.sequence(verse).unwrap()
    );
    let uses = uses_on(h.doc(), seq, "Bass");
    assert_eq!(uses.len(), 2, "Verse's use became notes: {uses:?}");
    assert!(uses.iter().all(|u| u.2 != riff && u.2 != verse));
    assert_eq!(
        details(&report, ItemKind::Changed, text::PRIVATE_COPIES),
        vec!["Sequence 'Riff'".to_owned()]
    );
    assert_eq!(
        details(&report, ItemKind::Changed, text::USES_AS_NOTES),
        vec!["Track 'Bass': use of 'Verse'".to_owned()]
    );
    assert!(!has(&report, text::LOOPS_UNROLLED));
    // Riff is used by Verse, outside the exported uses, but it is not
    // edited itself, so nothing plays elsewhere.
    assert!(!has(&report, text::SHARED_ELSEWHERE));
    preview_lists_them(&p, &report);
    merges_twice_to_nothing(&t, &theirs_doc_, h);
}

/// Fix round 1 (I2): Bass's inactive take "Take 2" also uses Riff. Theirs
/// edits Riff the same way in both exported uses, so Riff is edited
/// itself, and that take plays the edit too: `SHARED_ELSEWHERE` names it.
#[test]
fn a_use_on_an_inactive_take_counts_as_elsewhere() {
    let (doc, seq) = fixture();
    let doc = changed(&doc, |h| {
        let riff = seq_named(h.doc(), "Riff");
        let bass = lane(h.doc(), seq, "Bass");
        let (id, first) = (bass.id, bass.active_take);
        run(
            h,
            Command::Lane(LaneCmd::AddTake {
                seq,
                lane: id,
                name: "Take 2".into(),
            }),
        );
        run(
            h,
            Command::Reference(ReferenceCmd::Insert {
                seq,
                lane: id,
                tick: Tick(0),
                target: riff,
                length: None,
                loop_count: LoopCount::Count(1),
                transpose: 0,
            }),
        );
        run(
            h,
            Command::Lane(LaneCmd::SetActiveTake {
                seq,
                lane: id,
                take: first,
            }),
        );
    });
    let t = trip(&doc, seq);
    let riff = seq_named(&t.base_doc, "Riff");
    let theirs_doc_ = changed(&t.base_doc, |h| move_riff_note(h, riff, 1, Q / 2));
    let th = theirs_doc(&t, &theirs_doc_);
    let want = played_on(&th.projection, 0);
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    let (h, report) = apply_on(&t.base_doc, &p);

    assert_eq!(seq_keys(h.doc(), riff), vec![(0, 40), (Q + Q / 2, 43)]);
    assert_eq!(h.doc().sequences().len(), t.base_doc.sequences().len());
    assert_eq!(played_on(&projection(h.doc(), seq).unwrap(), 0), want);
    assert_eq!(
        details(&report, ItemKind::Note, text::SHARED_ELSEWHERE),
        vec!["Sequence 'Riff'".to_owned()]
    );
    preview_lists_them(&p, &report);
}

/// Fix round 1 (I3): Riff's length is derived (two bars: its third note
/// is in bar 2). Theirs deletes that note in both uses, keeping their
/// two-bar passes. Editing Riff would shrink it to one bar and move every
/// pass, so Riff is left as it is: each use gets a private copy, locked to
/// the two bars its use played, and Bass plays exactly as theirs.
#[test]
fn an_edit_that_would_change_a_sequence_length_makes_private_copies() {
    let (doc, seq) = fixture();
    let doc = changed(&doc, |h| {
        let riff = seq_named(h.doc(), "Riff");
        let rl = h.doc().sequence(riff).unwrap().lanes[0].id;
        add_note(h, riff, rl, BAR + Q, 45);
        run(
            h,
            Command::Sequence(SequenceCmd::SetLength {
                seq: riff,
                length: SeqLength::Derived,
            }),
        );
    });
    let t = trip(&doc, seq);
    let riff = seq_named(&t.base_doc, "Riff");
    assert_eq!(
        core_model::query::seq_length(&t.base_doc, riff).unwrap(),
        Tick(2 * BAR)
    );
    let theirs_doc_ = changed(&t.base_doc, |h| {
        let third = seq_notes(h.doc(), riff)[2].0;
        run(
            h,
            Command::Sequence(SequenceCmd::SetLength {
                seq: riff,
                length: SeqLength::Locked(Tick(2 * BAR)),
            }),
        );
        run(
            h,
            Command::Event(EventCmd::Delete {
                events: vec![third],
            }),
        );
    });
    let th = theirs_doc(&t, &theirs_doc_);
    let want = played_on(&th.projection, 0);
    let p = plan(t.snapshot.clone(), th, &t.base_doc, chosen(), &never()).expect("plans");
    let (h, report) = apply_on(&t.base_doc, &p);

    assert_eq!(seq_notes(h.doc(), riff), seq_notes(&t.base_doc, riff));
    assert_eq!(h.doc().sequence(riff).unwrap().length, SeqLength::Derived);
    let uses = uses_on(h.doc(), seq, "Bass");
    assert_eq!(uses.len(), 2);
    for (_, _, s) in &uses {
        assert_ne!(*s, riff);
        let copy = h.doc().sequence(*s).unwrap();
        assert_eq!(copy.name, "Riff (copy)");
        assert_eq!(copy.length, SeqLength::Locked(Tick(2 * BAR)));
        assert_eq!(seq_keys(h.doc(), *s), vec![(0, 40), (Q, 43)]);
    }
    assert_eq!(played_on(&projection(h.doc(), seq).unwrap(), 0), want);
    assert_eq!(
        details(&report, ItemKind::Changed, text::PRIVATE_COPIES),
        vec!["Sequence 'Riff'".to_owned()]
    );
    assert!(!has(&report, text::SHARED_ELSEWHERE));
    preview_lists_them(&p, &report);
    merges_twice_to_nothing(&t, &theirs_doc_, h);
}

// ---------------------------------------------------------------- stripped variants (Task 7)

thread_local! {
    /// What this test's returned files lose ([`with_strip`]).
    static STRIP: std::cell::Cell<Strip> = const { std::cell::Cell::new(Strip::Nothing) };
}

/// What this test's returned files lose: [`Strip::Nothing`] for the plain
/// tests, else the variant's.
fn strip() -> Strip {
    STRIP.with(std::cell::Cell::get)
}

/// Whether the user chose the snapshot: the file lost its export ID
/// ([`Strip::Both`]).
fn chosen() -> bool {
    strip() == Strip::Both
}

/// Whether the file still has its `vz:` tags.
fn tagged() -> bool {
    matches!(strip(), Strip::Nothing | Strip::Record)
}

/// Runs `test` with its returned files stripped as `s` says.
fn with_strip(s: Strip, test: fn()) {
    STRIP.with(|c| c.set(s));
    test();
    // Only for checks after the test (its thread ends with it).
    STRIP.with(|c| c.set(Strip::Nothing));
}

/// Every test above, once per [`Strip`] other than `Nothing` (spec §12:
/// the same cases without the record, without the tags, and without both,
/// with the snapshot chosen). Where a case relies on a tag the test says
/// so, per case.
macro_rules! stripped {
    ($($name:ident),* $(,)?) => {
        mod without_record {
            $( #[test] fn $name() { super::with_strip(super::Strip::Record, super::$name) } )*
        }
        mod without_tags {
            $( #[test] fn $name() { super::with_strip(super::Strip::Tags, super::$name) } )*
        }
        mod without_either {
            $( #[test] fn $name() { super::with_strip(super::Strip::Both, super::$name) } )*
        }
    };
}

stripped!(
    a_faster_tempo_keeps_theirs_crossfades_on_the_take,
    a_faster_tempo_keeps_theirs_crossfades,
    a_moved_crossfaded_region_keeps_its_crossfade,
    a_renamed_track_and_a_moved_marker,
    theirs_only_edits_are_applied,
    ours_only_edits_are_kept,
    both_changed_notes_become_an_alternate_take,
    take_names_count_up_on_the_same_day,
    both_changed_settings_keep_ours_and_list_theirs,
    deletions_follow_the_table,
    deleted_track_with_vizdaw_only_state_is_kept_empty,
    vizdaw_only_state_is_byte_equal,
    one_undo_restores_the_project_exactly,
    new_tracks_and_buses_are_added_as_an_import_would,
    crossfaded_pair_added_in_one_insert,
    plugin_state_from_theirs_is_one_step,
    nothing_to_merge_adds_no_step,
    merging_the_same_file_twice_changes_nothing_the_second_time,
    edits_inside_a_use_play_as_theirs,
    looped_track_edit_becomes_a_take,
    own_length_track_edit_becomes_a_take,
    play_quantized_track_edit_becomes_a_take,
    device_list_and_slot_changes_on_one_strip_all_land,
    instrument_swap_is_kept_and_listed,
    instrument_swapped_for_a_vst3_is_kept_and_listed,
    dropped_state_is_kept_and_listed,
    send_and_output_to_a_bus_deleted_here_are_listed,
    new_track_needing_a_bus_deleted_here_plays_into_the_master,
    untouched_regions_and_uses_keep_their_events,
    second_merge_pairs_regions_by_their_original_events,
    an_error_midway_then_abort_restores_the_project,
    offline_regions_conflict_take_is_not_made_twice,
    matched_output_to_a_bus_deleted_here_keeps_ours,
    a_moved_region_merged_twice_from_resampled_audio_adds_nothing,
    crossfaded_pair_with_its_earlier_member_changed_merges_twice_to_nothing,
    crossfaded_pair_with_its_later_member_changed_merges_twice_to_nothing,
    regions_on_a_track_with_its_own_length_become_a_take,
    automation_on_a_track_with_its_own_length_is_listed_with_the_reason,
    shared_sequence_edited_the_same_way_edits_the_sequence,
    shared_uses_edited_differently_become_private_copies,
    looped_use_edited_on_one_pass_becomes_notes,
    a_transposed_use_takes_its_edit_untransposed,
    format_one_snapshots_merge_uses_as_notes,
    preview_counts_past_the_details_kept,
    a_cancelled_apply_stops_and_aborts_cleanly,
    a_changed_region_inside_a_use_edits_its_sequence,
    a_sequence_played_inside_another_use_is_not_edited_itself,
    a_use_on_an_inactive_take_counts_as_elsewhere,
    an_edit_that_would_change_a_sequence_length_makes_private_copies,
);
