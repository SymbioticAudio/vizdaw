//! The merge applied (plan 6e Task 6, Rulings 5–7 and 9–11): each test
//! exports `common::merge::fixture()` (its "Bass" strip at −6 dB), makes
//! theirs (the returned file, edited in the file or as commands on a copy
//! of the exported document and exported again) and ours (`History`
//! commands on a copy of the exported document), plans, then applies the
//! plan inside `begin(LABEL)` … `commit()` on a `History` of ours.

mod common;

use common::merge::{Strip, Trip, fixture, return_file, theirs_of, trip, vizdaw_only};
use common::package::{TestPlist, media_of, package_bytes};
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
    AudioCmd, Command, CrossfadeLength, EventCmd, LaneCmd, MixerCmd, RoutingCmd, SequenceCmd,
    SettingsCmd,
};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::file::to_json;
use core_model::history::History;
use core_model::ids::{AnyId, EventId, LaneId, MediaId, PluginInstanceId, SeqId, StripId};
use core_model::input_fx::{InputFx, InputFxKind};
use core_model::lane::{Lane, LaneInput, MonitorMode, QuantizeSpec};
use core_model::learn::{LearnKind, LearnMap, LearnMode, LearnSource};
use core_model::mixer::{AuxSend, PluginSlot, StripKind};
use core_model::mpe::MpeConfig;
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
    let path = return_file(&t, Strip::Nothing, |p, _| third_note(p, "Loop").key = 64);
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
    let path = return_file(&t, Strip::Nothing, |p, _| {
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
    let path = return_file(&t, Strip::Nothing, |p, _| {
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

/// I5: theirs moves region A only. B and the use of "Clip" keep their
/// events (the use is still a use), and nothing is unrolled.
#[test]
fn untouched_regions_and_uses_keep_their_events() {
    let (doc, seq, [a, b, u]) = regions_fixture();
    let t = trip(&doc, seq);
    let p = plan_doc(&t, &moved(&t, &[a], BAR), &t.base_doc);
    assert!(
        p.decisions
            .iter()
            .any(|d| d.unit == Unit::Regions && d.outcome == Outcome::Theirs),
        "{:#?}",
        p.decisions
    );
    let before_b = region_at(&t.base_doc, b);
    let (h, report) = apply_on(&t.base_doc, &p);
    assert_eq!(region_at(h.doc(), b), before_b, "B keeps its event");
    let keys = lane(h.doc(), seq, "Keys").active().unwrap();
    let use_ = keys.events.values().find(|e| e.id == u).expect("the use");
    assert!(matches!(use_.body, EventBody::Reference { .. }));
    assert!(h.doc().event(a).is_err(), "A was replaced");
    let ticks: Vec<i64> = keys.events.values().map(|e| e.tick.0).collect();
    assert_eq!(ticks, vec![BAR, 2 * BAR, 4 * BAR]);
    assert!(!has(&report, text::LOOPS_UNROLLED));
}

/// `doc` returned as `theirs_doc` does, its audio "resampled" by the other
/// DAW: every converted file has another content hash and name, so only
/// the `vz:e` tags pair its regions with ours'.
fn theirs_resampled(t: &Trip, doc: &Document) -> Theirs {
    let x = trip(doc, t.seq).export;
    let n = DOCS.fetch_add(1, Ordering::Relaxed);
    let path = t.dir.path().join(format!("Doc {n}.dawproject"));
    std::fs::write(&path, package_bytes(&x).expect("packages")).expect("writes");
    let pkg = open(&path, &PackageLimits::default(), &never()).expect("opens");
    let mut media = media_of(&pkg, &x, doc);
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

/// I5: after a merge that moved A, a second file from the same export
/// (A and B moved, its audio resampled) still pairs B by its original
/// event: theirs' version (the conflict take) plays B with ours' media
/// and gain (Ruling 11).
#[test]
fn second_merge_pairs_regions_by_their_original_events() {
    let (doc, seq, [a, b, _]) = regions_fixture();
    let t = trip(&doc, seq);
    let (h, _) = apply_on(
        &t.base_doc,
        &plan_doc(&t, &moved(&t, &[a], BAR), &t.base_doc),
    );
    let once = h.doc().clone();
    let (_, ours_media, ours_gain) = region_at(&once, b);
    assert_eq!(ours_gain, -4.0);

    let second = theirs_resampled(&t, &moved(&t, &[a, b], BAR));
    let p = plan(t.snapshot.clone(), second, &once, false, &never()).expect("plans");
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
    let path = return_file(&t, Strip::Nothing, |p, _| owners_check(p));
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
    let x = trip(doc, t.seq).export;
    let n = DOCS.fetch_add(1, Ordering::Relaxed);
    let path = t.dir.path().join(format!("Doc {n}.dawproject"));
    std::fs::write(&path, package_bytes(&x).expect("packages")).expect("writes");
    let pkg = open(&path, &PackageLimits::default(), &never()).expect("opens");
    let mut media = media_of(&pkg, &x, doc);
    for m in media.values_mut() {
        *m = MediaSource::Offline {
            why: "unreadable".into(),
        };
    }
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
        false,
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
        false,
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
