//! The merge applied (plan 6e Task 6, Rulings 5–7 and 9–11): each test
//! exports `common::merge::fixture()` (its "Bass" strip at −6 dB), makes
//! theirs (the returned file, edited in the file or as commands on a copy
//! of the exported document and exported again) and ours (`History`
//! commands on a copy of the exported document), plans, then applies the
//! plan inside `begin(LABEL)` … `commit()` on a `History` of ours.

mod common;

use common::merge::{Strip, Trip, fixture, return_file, theirs_of, trip, vizdaw_only};
use common::package::{TestPlist, media_of, package_bytes};
use core_engine::fixtures::{BAR, Q};
use core_io::dawproject::import::{FROM_VIZDAW, ImportEnv, map};
use core_io::dawproject::merge::apply::{LABEL, apply_merge};
use core_io::dawproject::merge::decide::{MergePlan, MergePreview, Outcome, Unit, plan};
use core_io::dawproject::merge::text::{
    self, CONFLICT_SETTING, CONFLICT_TAKE, KEPT_EMPTY, NOTHING, summary,
};
use core_io::dawproject::merge::theirs::{Theirs, theirs};
use core_io::dawproject::package::open;
use core_io::dawproject::record::{PStrip, PTrack, Projection, projection};
use core_io::dawproject::{DpClip, DpMarker, DpNamed, DpNote, DpProject, DpTimeline};
use core_io::limits::PackageLimits;
use core_io::report::{ItemKind, Report};
use core_model::automation::{AutomationMode, AutomationTarget};
use core_model::command::{
    AudioCmd, Command, CrossfadeLength, EventCmd, LaneCmd, MixerCmd, RoutingCmd, SequenceCmd,
    SettingsCmd,
};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::file::to_json;
use core_model::history::History;
use core_model::ids::{AnyId, EventId, LaneId, MediaId, PluginInstanceId, SeqId, StripId};
use core_model::input_fx::{InputFx, InputFxKind};
use core_model::lane::{Lane, LaneInput, MonitorMode};
use core_model::learn::{LearnKind, LearnMap, LearnMode, LearnSource};
use core_model::mixer::{AuxSend, PluginSlot, StripKind};
use core_model::mpe::MpeConfig;
use core_model::selection::ViewFilter;
use core_model::sequence::{KeySlot, Marker};
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
        false,
        &never(),
    )
    .expect("plans")
}

static DOCS: AtomicUsize = AtomicUsize::new(0);

/// Theirs as the other DAW would return `doc` (an edited copy of
/// `t.base_doc`): `doc` exported with the harness's IDs, packaged with its
/// record (which names `t`'s export), opened and mapped with the
/// snapshot's IDs trusted.
fn theirs_doc(t: &Trip, doc: &Document) -> Theirs {
    let x = trip(doc, t.seq).export;
    let n = DOCS.fetch_add(1, Ordering::Relaxed);
    let path = t.dir.path().join(format!("Doc {n}.dawproject"));
    std::fs::write(&path, package_bytes(&x).expect("packages")).expect("writes");
    let pkg = open(&path, &PackageLimits::default(), &never()).expect("opens");
    let media = media_of(&pkg, &x, doc);
    let installed = |_: core_model::mixer::PluginFormat, _: &str| true;
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

/// The plan for theirs `doc` (an edited copy of `t.base_doc`) against
/// `ours`.
fn plan_doc(t: &Trip, doc: &Document, ours: &Document) -> MergePlan {
    plan(
        t.snapshot.clone(),
        theirs_doc(t, doc),
        ours,
        false,
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
    let path = return_file(&t, Strip::Nothing, |p, _| owners_check(p));
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
    let path = return_file(&t, Strip::Nothing, |p, _| set_db(p, "c1", -3.0));
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
    let path = return_file(t, Strip::Nothing, |p, _| move_bass_note(p, 1.0));
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
    let second = return_file(&t, Strip::Nothing, |p, _| first_note(p, "t0").key = 40);
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
    let path = return_file(&t, Strip::Nothing, |p, _| set_db(p, "c1", -3.0));
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
    let path = return_file(&t, Strip::Nothing, |p, _| keys_gone(p));
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
    let path = return_file(&t, Strip::Nothing, |p, _| first_note(p, "t1").key = 64);
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
    let path = return_file(&t, Strip::Nothing, |p, _| remove_track(p, "t0"));
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
    assert!(
        p.decisions
            .iter()
            .all(|d| matches!(d.outcome, Outcome::Theirs | Outcome::Added)),
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
    let path = return_file(&t, Strip::Nothing, |p, _| owners_check(p));
    let p = plan_of(&t, &path, &t.base_doc);
    let (mut h, _) = apply_on(&t.base_doc, &p);
    h.undo().unwrap();
    assert_eq!(to_json(h.doc()).unwrap(), to_json(&t.base_doc).unwrap());

    // A notes conflict, a setting conflict and theirs-only edits.
    let path = return_file(&t, Strip::Nothing, |p, _| {
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
    let p = plan(t.snapshot.clone(), th, &t.base_doc, false, &never()).expect("plans");
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
    let path = return_file(&t, Strip::Nothing, |_, _| {});
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
    let path = return_file(&t, Strip::Nothing, |p, _| owners_check(p));
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

/// Until Task 6b (Ruling 8), a use's notes count as its track's: theirs'
/// edit inside a use plays on ours exactly as on theirs.
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
    let p = plan(t.snapshot.clone(), th, &t.base_doc, false, &never()).expect("plans");
    let (h, _) = apply_on(&t.base_doc, &p);
    assert_eq!(played(&projection(h.doc(), seq).unwrap()), want);
    // Keys is untouched.
    assert_eq!(
        projection(h.doc(), seq).unwrap().tracks[1..],
        projection(&t.base_doc, seq).unwrap().tracks[1..]
    );
}
