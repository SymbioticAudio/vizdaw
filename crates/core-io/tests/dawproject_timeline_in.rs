//! DAWproject import of the arrangement: notes, expression, controller
//! points, re-sharing, audio, fades, crossfades, automation, seconds,
//! warps and scenes (plan 6d Task 7). Most tests read one track of the
//! hand-written `hand_timeline` fixture, named after the test.

use core_io::IoError;
use core_io::dawproject::import::{
    AUDIO_PAST_FILE, AUDIO_TOO_SHORT, AUTOMATION_TRACK, BAD_NOTES, CROSSFADES_PLAIN, DpImport,
    EXPANSION, EXPANSION_FLOOR, EXPRESSION_PAST_END, FORMANT, HELD_STEPS, ImportEnv, MAIN_CHANNEL,
    MIDI_RAMPS, MediaSource, NAMES_CUT, NESTED_TOO_DEEP, NOTES_CUT, OFFLINE, PAST_END,
    SHARED_AUTOMATION, SHARED_SPLIT, SLOTS, TIME_STRETCH, TOO_MUCH, TRANSPOSE_CLAMPED, UNROLLED,
    VEL_CLAMPED, VIDEO, WIDE_AUDIO, apply, map,
};
use core_io::dawproject::import::{
    CANCEL_EVERY, MAX_NEW_SEQUENCES, MAX_SOUNDING, debug_items_after_pause, debug_items_seen,
    debug_pause_at, debug_paused,
};
use core_io::dawproject::package::{DpPackage, open};
use core_io::dawproject::plugins::PlistCodec;
use core_io::dawproject::schema::UNREADABLE_VALUES;
use core_io::dawproject::units::db_of_linear;
use core_io::limits::PackageLimits;
use core_io::report::{ItemKind, Report};
use core_io::zip::{EntryMethod, ZipWriter};
use core_media::import::Imported;
use core_model::automation::{AutomationTarget, SendIndex, fader_norm, to_u32};
use core_model::command::{Command, LaneCmd, SequenceCmd};
use core_model::document::Document;
use core_model::event::{Event, EventBody, FadeShape, LoopCount, PerNoteCtl, PerNoteKind};
use core_model::history::History;
use core_model::ids::{AnyId, SeqId};
use core_model::lane::Lane;
use core_model::mixer::{ParamId, PluginFormat, StripKind};
use core_model::rules;
use core_model::selection::ViewFilter;
use core_model::sequence::{SeqLength, Sequence};
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/dawproject");
const BEAT: i64 = 15_360;

// ---------------------------------------------------------------- helpers

struct NoCodec;

impl PlistCodec for NoCodec {
    fn to_xml(&self, _: &[u8]) -> Result<Vec<u8>, String> {
        Err("not used".into())
    }

    fn to_binary(&self, _: &[u8]) -> Result<Vec<u8>, String> {
        Err("not used".into())
    }
}

fn yes(_: PluginFormat, _: &str) -> bool {
    true
}

/// A package of `project` alone, opened.
fn package(dir: &Path, project: &str) -> DpPackage {
    let path = dir.join("Timeline.dawproject");
    let mut z = ZipWriter::new(std::fs::File::create(&path).unwrap());
    z.add("project.xml", EntryMethod::Deflated, project.as_bytes())
        .unwrap();
    z.finish().unwrap();
    open(&path, &PackageLimits::default(), &AtomicBool::new(false)).expect("opens")
}

/// Every audio file of `pkg` converted: one item at 48 kHz, as long as the
/// file says (at least a frame).
fn converted(pkg: &DpPackage) -> BTreeMap<usize, MediaSource> {
    pkg.audio()
        .iter()
        .map(|a| {
            let item = Imported {
                path: format!("Media/{}.caf", a.name),
                name: a.name.clone(),
                original_name: format!("{}.wav", a.name),
                channels: a.channels.clamp(1, 2),
                sample_rate: 48_000,
                length_frames: a.frames.max(1),
                content_hash: "h".into(),
                clipped: 0,
            };
            (a.key, MediaSource::Online(vec![item]))
        })
        .collect()
}

fn mapped(pkg: &DpPackage, media: &BTreeMap<usize, MediaSource>) -> DpImport {
    let env = ImportEnv {
        installed: &yes,
        plist: &NoCodec,
        rate: 48_000,
    };
    map(pkg, media, &env, &AtomicBool::new(false)).expect("maps")
}

/// A document like a new project: the master and one sequence with no
/// tracks.
fn new_doc() -> (Document, SeqId) {
    let mut h = History::new(Document::new());
    let r = h
        .execute(
            &Command::Sequence(SequenceCmd::New {
                name: "Sequence 1".into(),
            }),
            &ViewFilter::default(),
        )
        .unwrap();
    let seq = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        })
        .unwrap();
    let lane = h.doc().sequence(seq).unwrap().lanes.front().unwrap().id;
    h.execute(
        &Command::Lane(LaneCmd::Remove { seq, lane }),
        &ViewFilter::default(),
    )
    .unwrap();
    (h.doc().clone(), seq)
}

struct Applied {
    h: History,
    seq: SeqId,
    report: Report,
}

fn apply_to_new(imp: &DpImport) -> Applied {
    let (doc, seq) = new_doc();
    let mut h = History::new(doc);
    let mut report = Report::default();
    h.begin("Import DAWproject").unwrap();
    apply(&mut h, imp, seq, &mut report).expect("applies");
    h.commit().unwrap();
    Applied { h, seq, report }
}

/// `project` imported with every audio file converted.
fn imported(project: &str) -> Applied {
    let dir = tempfile::tempdir().unwrap();
    let pkg = package(dir.path(), project);
    let media = converted(&pkg);
    apply_to_new(&mapped(&pkg, &media))
}

fn hand_timeline() -> Applied {
    let xml = std::fs::read_to_string(format!("{DIR}/hand_timeline.project.xml"))
        .expect("the hand-written fixture hand_timeline.project.xml");
    imported(&xml)
}

fn lane(a: &Applied, name: &str) -> Lane {
    a.h.doc()
        .sequence(a.seq)
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.name == name)
        .cloned()
        .unwrap_or_else(|| panic!("no track {name}"))
}

fn events_of(l: &Lane) -> Vec<Event> {
    l.active()
        .map(|t| t.events.values().cloned().collect())
        .unwrap_or_default()
}

fn events(a: &Applied, name: &str) -> Vec<Event> {
    events_of(&lane(a, name))
}

/// `(tick, pitch, duration)` of each note.
fn notes_of(evs: &[Event]) -> Vec<(i64, u8, i64)> {
    evs.iter()
        .filter_map(|e| match &e.body {
            EventBody::Note {
                pitch, duration, ..
            } => Some((e.tick.0, *pitch, duration.0)),
            _ => None,
        })
        .collect()
}

/// `(tick, seq, length, loop count, mute)` of each use.
fn uses_of(evs: &[Event]) -> Vec<(i64, SeqId, Option<Tick>, LoopCount, bool)> {
    evs.iter()
        .filter_map(|e| match &e.body {
            EventBody::Reference {
                seq,
                length,
                loop_count,
                ..
            } => Some((e.tick.0, *seq, *length, *loop_count, e.mute)),
            _ => None,
        })
        .collect()
}

/// The region fields: `(tick, source_offset, length, fade_in, fade_out,
/// crossfade, mute)`.
type RegionRow = (i64, u64, u64, u64, u64, bool, bool);

fn regions_of(evs: &[Event]) -> Vec<RegionRow> {
    evs.iter()
        .filter_map(|e| match &e.body {
            EventBody::AudioRegion {
                source_offset,
                length,
                fade_in,
                fade_out,
                crossfade,
                ..
            } => Some((
                e.tick.0,
                *source_offset,
                *length,
                fade_in.length_frames,
                fade_out.length_frames,
                *crossfade,
                e.mute,
            )),
            _ => None,
        })
        .collect()
}

fn sequence(a: &Applied, id: SeqId) -> Sequence {
    a.h.doc().sequence(id).unwrap().clone()
}

fn named_sequences(a: &Applied, name: &str) -> Vec<Sequence> {
    a.h.doc()
        .sequences()
        .values()
        .filter(|s| s.name == name)
        .cloned()
        .collect()
}

/// The events of a sequence's only track.
fn seq_events(s: &Sequence) -> Vec<Event> {
    assert_eq!(s.lanes.len(), 1, "one track in '{}'", s.name);
    events_of(s.lanes.front().unwrap())
}

fn item<'a>(r: &'a Report, kind: ItemKind, what: &str) -> Option<&'a core_io::report::Item> {
    r.items.iter().find(|i| i.kind == kind && i.what == what)
}

#[track_caller]
fn has(r: &Report, kind: ItemKind, what: &str) {
    assert!(
        item(r, kind, what).is_some(),
        "no {kind:?} '{what}' in {:#?}",
        r.items
    );
}

#[track_caller]
fn lacks(r: &Report, what: &str) {
    assert!(
        r.items.iter().all(|i| i.what != what),
        "unexpected '{what}' in {:#?}",
        r.items
    );
}

/// A project of `structure` (beside a master "Master", channel `mc`) and
/// `arrangement` at 120 BPM.
fn project_xml(structure: &str, arrangement: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Fixture" version="1"/>
  <Transport><Tempo id="tempo" unit="bpm" value="120"/></Transport>
  <Structure>
    <Track id="m" name="Master" contentType="audio notes"><Channel id="mc" role="master"/></Track>
    {structure}
  </Structure>
  <Arrangement>{arrangement}</Arrangement>
</Project>"#
    )
}

fn note_track(id: &str, name: &str) -> String {
    format!(
        r#"<Track id="{id}" name="{name}" contentType="notes"><Channel id="{id}c" role="regular" destination="mc"/></Track>"#
    )
}

// ---------------------------------------------------------------- tests

#[test]
fn notes_land_where_the_clips_play() {
    let a = hand_timeline();
    let evs = events(&a, "notes_land_where_the_clips_play");
    assert_eq!(
        notes_of(&evs),
        vec![(4 * BEAT, 61, BEAT), (5 * BEAT + BEAT / 2, 62, BEAT / 2)],
        "the notes at 0.5 and 3 are outside [1, 3); the 2.5 note is cut"
    );
    has(&a.report, ItemKind::Changed, NOTES_CUT);
    for e in &evs {
        assert_eq!(e.group_channel, None, "channel 1 is the track's own");
        assert!(!e.mute);
    }
    let EventBody::Note {
        velocity,
        release_velocity,
        ..
    } = &evs[0].body
    else {
        panic!("a note")
    };
    assert_eq!(*velocity, 32_768, "0.5 × 65 535, rounded");
    assert_eq!(*release_velocity, 0, "no rel reads as 0");

    // A note with no vel plays at MIDI's 100.
    let a = imported(&project_xml(
        &note_track("t", "NoVel"),
        r#"<Lanes timeUnit="beats"><Lanes track="t"><Clips><Clip time="0" duration="1" playStart="0"><Notes><Note time="0" duration="1" channel="0" key="60"/></Notes></Clip></Clips></Lanes></Lanes>"#,
    ));
    let evs = events(&a, "NoVel");
    let EventBody::Note { velocity, .. } = &evs[0].body else {
        panic!("a note")
    };
    assert_eq!(*velocity, (100.0 / 127.0 * 65_535.0_f64).round() as u16);
}

#[test]
fn looped_clip_becomes_a_looped_use() {
    let a = hand_timeline();
    let evs = events(&a, "looped_clip_becomes_a_looped_use");
    let uses = uses_of(&evs);
    assert_eq!(uses.len(), 2, "{uses:?}");

    let (tick, id, length, count, mute) = uses[0];
    assert_eq!(
        (tick, length, count, mute),
        (0, None, LoopCount::Count(3), false)
    );
    let s = sequence(&a, id);
    assert_eq!(s.name, "Loop A");
    assert_eq!(
        s.length,
        SeqLength::Locked(Tick(2 * BEAT)),
        "loopEnd − loopStart"
    );
    assert_eq!(notes_of(&seq_events(&s)), vec![(0, 60, BEAT / 2)]);

    let (tick, id, length, count, _) = uses[1];
    assert_eq!(
        (tick, length, count),
        (8 * BEAT, Some(Tick(5 * BEAT)), LoopCount::Fill)
    );
    let s = sequence(&a, id);
    assert_eq!(s.name, "Loop B");
    assert_eq!(s.length, SeqLength::Locked(Tick(2 * BEAT)));

    // Loop C starts at beat 1 of its content: unrolled onto the track.
    assert_eq!(
        notes_of(&evs),
        vec![
            (16 * BEAT, 64, BEAT / 2),
            (17 * BEAT, 62, BEAT / 2),
            (18 * BEAT, 64, BEAT / 2),
            (19 * BEAT, 62, BEAT / 2),
        ]
    );
    assert!(named_sequences(&a, "Loop C").is_empty());
    has(&a.report, ItemKind::Changed, UNROLLED);
}

#[test]
fn alias_clips_share_one_sequence() {
    let a = hand_timeline();
    let uses = uses_of(&events(&a, "alias_clips_share_one_sequence"));
    assert_eq!(uses.len(), 2);
    assert_eq!((uses[0].0, uses[1].0), (0, 4 * BEAT));
    assert_eq!(uses[0].1, uses[1].1, "one sequence");
    let s = sequence(&a, uses[0].1);
    assert_eq!(s.name, "alias_clips_share_one_sequence clip 1");
    assert_eq!(
        notes_of(&seq_events(&s)),
        vec![(0, 60, BEAT), (BEAT, 62, BEAT)]
    );
    assert_eq!(uses[0].3, LoopCount::Count(1));
    assert_eq!(
        s.lanes.front().unwrap().midi_dest,
        lane(&a, "alias_clips_share_one_sequence").midi_dest
    );
    assert!(
        notes_of(&events(&a, "alias_clips_share_one_sequence")).is_empty(),
        "the notes are in the sequence"
    );

    // On two tracks with different instruments: one sequence each.
    let two = lane(&a, "alias_clips_share_one_sequence 2");
    let three = lane(&a, "alias_clips_share_one_sequence 3");
    let u2 = uses_of(&events_of(&two));
    let u3 = uses_of(&events_of(&three));
    assert_eq!(u2.len(), 2);
    assert_eq!(u2[0].1, u2[1].1);
    assert_eq!(u3.len(), 1);
    assert_ne!(u2[0].1, u3[0].1, "separate sequences");
    let (s2, s3) = (sequence(&a, u2[0].1), sequence(&a, u3[0].1));
    assert_eq!((s2.name.as_str(), s3.name.as_str()), ("Shared", "Shared"));
    assert_eq!(s2.lanes.front().unwrap().midi_dest, two.midi_dest);
    assert_eq!(s3.lanes.front().unwrap().midi_dest, three.midi_dest);
    assert_ne!(two.midi_dest, three.midi_dest);
    has(&a.report, ItemKind::Note, SHARED_SPLIT);
}

#[test]
fn same_name_same_content_is_reshared() {
    let a = hand_timeline();
    let evs = events(&a, "same_name_same_content_is_reshared");
    let uses = uses_of(&evs);
    assert_eq!(uses.len(), 2, "{uses:?}");
    assert_eq!((uses[0].0, uses[1].0), (0, 4 * BEAT));
    assert_eq!(uses[0].1, uses[1].1);
    assert_eq!(sequence(&a, uses[0].1).name, "A");
    assert_eq!(named_sequences(&a, "A").len(), 1);
    assert!(named_sequences(&a, "B").is_empty());
    // "A" with other notes, and "B", go straight onto the track.
    assert_eq!(
        notes_of(&evs),
        vec![(8 * BEAT, 62, BEAT), (12 * BEAT, 60, BEAT)]
    );
}

#[test]
fn note_expressions_map_to_per_note_expression() {
    let a = hand_timeline();
    let evs = events(&a, "note_expressions_map_to_per_note_expression");
    let per_note = |tick: i64| -> Vec<PerNoteCtl> {
        let e = evs
            .iter()
            .find(|e| e.tick.0 == tick && matches!(e.body, EventBody::Note { .. }))
            .expect("the note");
        let EventBody::Note { per_note, .. } = &e.body else {
            unreachable!()
        };
        let mut v: Vec<PerNoteCtl> = per_note.iter().copied().collect();
        v.sort_by_key(|c| (c.offset, format!("{:?}", c.kind)));
        v
    };
    let bend_2 = (2_147_483_648.0f64 + 2.0 * 2_147_483_648.0 / 48.0).round() as u32;
    let ctl = |kind, offset, value| PerNoteCtl {
        kind,
        offset: Tick(offset),
        value,
    };
    let mut want = vec![
        ctl(PerNoteKind::PitchBend, 0, bend_2),
        ctl(PerNoteKind::Registered(10), 0, to_u32(0.25)),
        ctl(PerNoteKind::Registered(7), 0, to_u32(0.5)),
        ctl(PerNoteKind::Registered(74), 3_840, to_u32(0.5)),
    ];
    want.sort_by_key(|c| (c.offset, format!("{:?}", c.kind)));
    assert_eq!(per_note(0), want);
    let pressure: Vec<(i64, u8, u32)> = evs
        .iter()
        .filter_map(|e| match e.body {
            EventBody::PolyPressure { pitch, value } => Some((e.tick.0, pitch, value)),
            _ => None,
        })
        .collect();
    assert_eq!(pressure, vec![(BEAT / 2, 60, u32::MAX)]);
    has(&a.report, ItemKind::LeftOut, FORMANT);

    // Transpose 60 is clamped to 48.
    assert_eq!(
        per_note(2 * BEAT),
        vec![ctl(PerNoteKind::PitchBend, 0, u32::MAX)]
    );
    has(&a.report, ItemKind::Changed, TRANSPOSE_CLAMPED);
}

#[test]
fn midi_points_become_events() {
    let a = hand_timeline();
    let got: Vec<(i64, EventBody)> = events(&a, "midi_points_become_events")
        .into_iter()
        .map(|e| (e.tick.0, e.body))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                0,
                EventBody::Cc {
                    index: 1,
                    value: to_u32(0.5)
                }
            ),
            (BEAT, EventBody::PitchBend { value: 0x8000_0000 }),
            (
                2 * BEAT,
                EventBody::Program {
                    program: 5,
                    bank: Some((0, 1))
                }
            ),
            (
                3 * BEAT,
                EventBody::Cc {
                    index: 7,
                    value: to_u32(0.25)
                }
            ),
            (
                3 * BEAT + BEAT / 2,
                EventBody::Cc {
                    index: 7,
                    value: to_u32(0.75)
                }
            ),
        ],
        "the bank's CC 0 and 32 join the program"
    );
    has(&a.report, ItemKind::Changed, MIDI_RAMPS);
}

#[test]
fn audio_clips_map_to_regions() {
    let a = hand_timeline();
    let evs = events(&a, "audio_clips_map_to_regions");
    assert_eq!(
        regions_of(&evs),
        vec![
            (30_720, 24_000, 48_000, 480, 0, false, false),
            (8 * BEAT, 24_000, 48_000, 0, 6_000, false, true),
        ]
    );
    let names: Vec<Option<String>> = evs
        .iter()
        .map(|e| match &e.body {
            EventBody::AudioRegion {
                name,
                fade_in,
                fade_out,
                ..
            } => {
                assert_eq!(fade_in.shape, FadeShape::Linear);
                assert_eq!(fade_out.shape, FadeShape::Linear);
                name.clone()
            }
            _ => None,
        })
        .collect();
    assert_eq!(names, vec![Some("Region".into()), Some("Muted".into())]);
    let l = lane(&a, "audio_clips_map_to_regions");
    assert!(l.audio_out.is_some());
    // One media item for the one file.
    let doc = a.h.doc();
    let media: Vec<_> = doc.media().values().collect();
    assert_eq!(media.len(), 1, "audio/a.wav once");
    assert_eq!(media[0].path, "Media/a.caf");
}

#[test]
fn negative_fade_in_starts_the_clip_earlier() {
    let a = hand_timeline();
    let regions = regions_of(&events(&a, "negative_fade_in_starts_the_clip_earlier"));
    assert_eq!(
        regions,
        vec![
            // 0.25 s earlier, from 0.75 s of the file.
            (3 * BEAT + BEAT / 2, 36_000, 60_000, 12_000, 0, false, false),
            (10 * BEAT, 0, 48_000, 0, 0, false, false),
            // Starting inside the region before it: a crossfade.
            (11 * BEAT + BEAT / 2, 36_000, 60_000, 12_000, 0, true, false),
        ]
    );
    assert!(rules::validate(a.h.doc()).is_empty());
}

#[test]
fn nested_clips_compose_their_offsets() {
    let a = hand_timeline();
    assert_eq!(
        regions_of(&events(&a, "nested_clips_compose_their_offsets")),
        vec![
            (8 * BEAT, 0, 48_000, 0, 0, false, false),
            (10 * BEAT, 0, 24_000, 0, 0, false, false),
        ],
        "the second is cut at the outer clip's end (beat 11)"
    );
}

#[test]
fn identity_warps_are_not_reported() {
    let a = hand_timeline();
    for name in [
        "identity_warps_are_not_reported",
        "identity_warps_are_not_reported 2",
    ] {
        assert_eq!(
            regions_of(&events(&a, name)),
            vec![(0, 0, 192_000, 0, 0, false, false)],
            "{name}: 8 beats unstretched"
        );
    }
    let i = item(&a.report, ItemKind::LeftOut, TIME_STRETCH).expect("the time-stretching line");
    assert_eq!(i.count, 1);
    assert_eq!(
        i.details,
        vec!["identity_warps_are_not_reported 2".to_owned()]
    );
}

#[test]
fn overlapping_clips_with_covering_fades_become_crossfades() {
    let a = hand_timeline();
    let evs = events(
        &a,
        "overlapping_clips_with_covering_fades_become_crossfades",
    );
    assert_eq!(
        regions_of(&evs),
        vec![
            (0, 0, 48_000, 0, 0, false, false),
            (BEAT, 0, 48_000, 24_000, 0, true, false),
        ],
        "A's fade-out covered the overlap: it is cleared"
    );
    let EventBody::AudioRegion { fade_in, .. } = &evs[1].body else {
        panic!("a region")
    };
    assert_eq!(fade_in.shape, FadeShape::Linear);
    assert_eq!(rules::validate(a.h.doc()), Vec::<String>::new());

    let evs = events(
        &a,
        "overlapping_clips_with_covering_fades_become_crossfades 2",
    );
    assert_eq!(
        regions_of(&evs),
        vec![
            (0, 0, 48_000, 0, 0, false, false),
            (BEAT, 0, 48_000, 4_800, 0, false, false),
        ],
        "a plain overlap"
    );
}

#[test]
fn automation_points_become_events() {
    let a = hand_timeline();
    let doc = a.h.doc();
    let strip_of = |name: &str| {
        let l = lane(&a, name);
        doc.mixer()
            .find_kind(StripKind::AudioPath(l.audio_out.unwrap()))
            .unwrap()
    };
    let s = strip_of("automation_points_become_events");
    let slot = doc.mixer().strip(s).unwrap().inserts[0].id;
    let mut got: Vec<(AutomationTarget, i64, f64)> = events(&a, "automation_points_become_events")
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::Automation { target, value } => Some((target, e.tick.0, value)),
            _ => None,
        })
        .collect();
    got.sort_by_key(|x| (x.0, x.1));
    let level = |l: f64| fader_norm(db_of_linear(l));
    let mut want = vec![
        (AutomationTarget::StripFader(s), 0, level(0.0)),
        (AutomationTarget::StripFader(s), BEAT, level(1.0)),
        (AutomationTarget::StripFader(s), 2 * BEAT, level(2.0)),
        (AutomationTarget::StripPan(s), 0, 0.25),
        (AutomationTarget::StripPan(s), 4 * BEAT, 0.25),
        (AutomationTarget::StripPan(s), 5 * BEAT - 1, 0.25),
        (AutomationTarget::StripPan(s), 5 * BEAT, 0.75),
        (AutomationTarget::StripMute(s), 0, 0.0),
        (AutomationTarget::StripMute(s), BEAT, 1.0),
        (AutomationTarget::SendLevel(s, SendIndex(0)), 0, level(1.0)),
        (AutomationTarget::PluginParam(slot, ParamId(7)), 0, 0.5),
    ];
    want.sort_by_key(|x| (x.0, x.1));
    assert_eq!(got, want);
    has(&a.report, ItemKind::Changed, HELD_STEPS);

    // Points with no track go to the track whose channel they target.
    let s2 = strip_of("automation_points_become_events 2");
    let got: Vec<(AutomationTarget, i64, f64)> = events(&a, "automation_points_become_events 2")
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::Automation { target, value } => Some((target, e.tick.0, value)),
            _ => None,
        })
        .collect();
    assert_eq!(got, vec![(AutomationTarget::StripFader(s2), 0, level(1.0))]);

    // The master's go to a new track "Automation".
    let auto = lane(&a, "Automation");
    assert_eq!((auto.midi_dest, auto.audio_out), (None, None));
    let master = doc.mixer().master();
    let got: Vec<(AutomationTarget, i64)> = events_of(&auto)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::Automation { target, .. } => Some((target, e.tick.0)),
            _ => None,
        })
        .collect();
    assert_eq!(got, vec![(AutomationTarget::StripFader(master), 0)]);
    has(&a.report, ItemKind::Note, AUTOMATION_TRACK);

    // Two note tracks on the built-in synth share its strip: the first
    // (whose settings the strip took) drives its fader; the second's
    // automation, and the mute automation of either, is left out.
    let chan = |id: &str| {
        format!(
            r#"<Channel id="{id}c" role="regular" destination="mc"><Volume id="{id}v" unit="linear" value="1" min="0" max="4"/><Mute id="{id}m" value="false"/></Channel>"#
        )
    };
    let structure = format!(
        r#"<Track id="s1" name="Synth A" contentType="notes">{}</Track><Track id="s2" name="Synth B" contentType="notes">{}</Track>"#,
        chan("s1"),
        chan("s2")
    );
    let points = |track: &str, param: &str, value: &str| {
        format!(
            r#"<Lanes track="{track}"><Points unit="linear"><Target parameter="{param}"/><RealPoint time="0" value="{value}" interpolation="linear"/></Points></Lanes>"#
        )
    };
    let mute = |track: &str| {
        format!(
            r#"<Lanes track="{track}"><Points><Target parameter="{track}m"/><BoolPoint time="0" value="true"/></Points></Lanes>"#
        )
    };
    let arrangement = format!(
        r#"<Lanes timeUnit="beats">{}{}{}{}</Lanes>"#,
        points("s1", "s1v", "0.5"),
        mute("s1"),
        points("s2", "s2v", "2"),
        mute("s2")
    );
    let a = imported(&project_xml(&structure, &arrangement));
    let synth =
        a.h.doc()
            .mixer()
            .find_kind(StripKind::BuiltinSynth)
            .unwrap();
    let autos = |name: &str| -> Vec<AutomationTarget> {
        events(&a, name)
            .into_iter()
            .filter_map(|e| match e.body {
                EventBody::Automation { target, .. } => Some(target),
                _ => None,
            })
            .collect()
    };
    assert_eq!(autos("Synth A"), vec![AutomationTarget::StripFader(synth)]);
    assert_eq!(autos("Synth B"), Vec::<AutomationTarget>::new());
    let i = item(&a.report, ItemKind::LeftOut, SHARED_AUTOMATION).expect("the shared line");
    assert_eq!(i.details, vec!["Synth A".to_owned(), "Synth B".to_owned()]);
}

#[test]
fn seconds_timelines_map_through_the_tempo_map() {
    let a = hand_timeline();
    assert_eq!(
        notes_of(&events(&a, "seconds_timelines_map_through_the_tempo_map")),
        vec![(BEAT, 60, BEAT / 2)],
        "0.5 s at 120 BPM is beat 1; its 0.25 s is half a beat"
    );
    assert_eq!(
        notes_of(&events(&a, "seconds_timelines_map_through_the_tempo_map 2")),
        vec![(4 * BEAT, 60, BEAT)],
        "the beats timeline inside a seconds one is in beats"
    );

    // Under a ramp from 120 to 240 BPM over 4 beats.
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Fixture" version="1"/>
  <Transport><Tempo id="tempo" unit="bpm" value="120"/></Transport>
  <Structure>
    <Track id="m" name="Master" contentType="audio notes"><Channel id="mc" role="master"/></Track>
    {}
  </Structure>
  <Arrangement>
    <Lanes timeUnit="seconds">
      <Lanes track="r"><Clips><Clip time="1.5" duration="0.25" playStart="0"><Notes><Note time="0" duration="0.1" channel="0" key="60" vel="0.5"/></Notes></Clip><Clip time="1" duration="0.25" playStart="0"><Notes><Note time="0" duration="0.1" channel="0" key="61" vel="0.5"/></Notes></Clip></Clips></Lanes>
    </Lanes>
    <TempoAutomation unit="bpm" timeUnit="beats">
      <Target parameter="tempo"/>
      <RealPoint time="0" value="120" interpolation="linear"/>
      <RealPoint time="4" value="240" interpolation="hold"/>
    </TempoAutomation>
  </Arrangement>
</Project>"#,
        note_track("r", "Ramp")
    );
    let a = imported(&xml);
    let tempo = &a.h.doc().sequence(a.seq).unwrap().tempo;
    assert_eq!(tempo.points().len(), 2, "the ramp was imported");
    let got = notes_of(&events(&a, "Ramp"));
    let tick_of = |pitch: u8| got.iter().find(|n| n.1 == pitch).expect("the note").0;
    // 1.5 s is past the ramp's end (it lasts 4 × 60 × ln 2 / 120 s).
    let at = tempo.seconds_to_tick(1.5).0;
    assert_eq!(tick_of(60), at);
    assert!(at > 4 * BEAT && at < 5 * BEAT, "{at}");
    // 1 s is inside it.
    let at = tempo.seconds_to_tick(1.0).0;
    assert_eq!(tick_of(61), at);
    assert!(at > 2 * BEAT && at < 3 * BEAT, "{at}");
}

#[test]
fn scenes_become_unplaced_sequences() {
    let a = hand_timeline();
    let bass = lane(&a, "Bass");
    for (name, pitch) in [("Scene 1 – Bass", 36), ("Scene 2 – Bass", 38)] {
        let found = named_sequences(&a, name);
        assert_eq!(found.len(), 1, "{name}");
        let s = &found[0];
        assert_eq!(notes_of(&seq_events(s)), vec![(0, pitch, BEAT)]);
        assert_eq!(s.lanes.front().unwrap().midi_dest, bass.midi_dest);
        let used =
            a.h.doc()
                .all_events()
                .any(|(_, e)| matches!(e.body, EventBody::Reference { seq, .. } if seq == s.id));
        assert!(!used, "{name} is not placed");
    }
    let i = item(&a.report, ItemKind::Changed, SLOTS).expect("the slots line");
    assert_eq!(i.count, 2);
    has(&a.report, ItemKind::LeftOut, VIDEO);
    assert!(events(&a, "Bass").is_empty());
}

#[test]
fn hostile_numbers_leave_items_out() {
    let a = hand_timeline();
    let evs = events(&a, "hostile_numbers_leave_items_out");
    assert_eq!(
        notes_of(&evs),
        vec![(BEAT, 64, BEAT), (2 * BEAT, 62, BEAT)],
        "only the readable notes"
    );
    let vel = evs
        .iter()
        .find_map(|e| match e.body {
            EventBody::Note { velocity, .. } if e.tick.0 == BEAT => Some(velocity),
            _ => None,
        })
        .unwrap();
    assert_eq!(vel, 65_535, "2.0 clamped to 1");
    has(&a.report, ItemKind::Changed, VEL_CLAMPED);
    assert!(
        evs.iter().any(|e| e.tick.0 == BEAT
            && e.body
                == EventBody::Cc {
                    index: 1,
                    value: to_u32(0.5)
                }),
        "the readable point"
    );
    assert!(regions_of(&evs).is_empty(), "the sampleRate=\"0\" clip");
    has(&a.report, ItemKind::LeftOut, PAST_END);
    let bad = item(&a.report, ItemKind::LeftOut, BAD_NOTES).expect("bad notes");
    assert_eq!(bad.count, 3, "duration −1, key 200, channel 99");
    let values = item(&a.report, ItemKind::LeftOut, UNREADABLE_VALUES).expect("values");
    for d in ["Audio@sampleRate", "Clip@duration"] {
        assert!(
            values.details.iter().any(|x| x == d),
            "{d} in {:?}",
            values.details
        );
    }
    assert_eq!(rules::validate(a.h.doc()), Vec::<String>::new());
}

#[test]
fn exported_files_come_back() {
    // Task 5's export of its timeline fixture, read back.
    let xml = std::fs::read_to_string(format!("{DIR}/export_timeline.project.xml")).unwrap();
    let a = imported(&xml);
    let keys = events(&a, "Keys");
    assert_eq!(
        notes_of(&keys),
        vec![(0, 60, BEAT), (BEAT, 60, BEAT), (12 * BEAT, 60, 2 * BEAT)]
    );
    let bodies: Vec<(i64, EventBody)> = keys
        .iter()
        .filter(|e| !matches!(e.body, EventBody::Note { .. } | EventBody::Reference { .. }))
        .map(|e| (e.tick.0, e.body.clone()))
        .collect();
    assert_eq!(
        bodies,
        vec![
            (
                BEAT / 2,
                EventBody::Cc {
                    index: 1,
                    value: 0x4000_0000
                }
            ),
            (
                12 * BEAT,
                EventBody::Program {
                    program: 5,
                    bank: Some((0, 1))
                }
            ),
        ]
    );
    let bend = keys
        .iter()
        .find_map(|e| match &e.body {
            EventBody::Note { per_note, .. } if e.tick.0 == 12 * BEAT => Some(per_note.clone()),
            _ => None,
        })
        .unwrap();
    let mut bend: Vec<PerNoteCtl> = bend.into_iter().collect();
    bend.sort_by_key(|c| c.offset);
    assert_eq!(
        bend,
        vec![
            PerNoteCtl {
                kind: PerNoteKind::PitchBend,
                offset: Tick(0),
                value: 0x8000_0000 + 89_478_485
            },
            PerNoteCtl {
                kind: PerNoteKind::Registered(74),
                offset: Tick(BEAT),
                value: 0x8000_0000
            },
        ]
    );
    // The looped use and the muted one share "Riff" again.
    let uses = uses_of(&keys);
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].1, uses[1].1);
    assert_eq!(
        (uses[0].0, uses[0].3, uses[0].4),
        (16 * BEAT, LoopCount::Count(2), false)
    );
    assert_eq!(
        (uses[1].0, uses[1].3, uses[1].4),
        (32 * BEAT, LoopCount::Count(1), true)
    );
    let riff = sequence(&a, uses[0].1);
    assert_eq!(riff.name, "Riff");
    assert_eq!(riff.length, SeqLength::Locked(Tick(4 * BEAT)));
    assert_eq!(notes_of(&seq_events(&riff)), vec![(0, 62, BEAT)]);

    // The crossfade comes back; the earlier region's fade-out was the
    // overlap, so it is cleared. The second keeps its name.
    let drums = events(&a, "Drums");
    let names: Vec<Option<String>> = drums
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::AudioRegion { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec![None, Some("Hit".to_owned())]);
    assert_eq!(
        regions_of(&drums),
        vec![
            (0, 0, 48_000, 480, 0, false, false),
            (BEAT, 0, 48_000, 4_800, 0, true, false),
        ]
    );
    let autos: Vec<(i64, f64)> = drums
        .iter()
        .filter_map(|e| match e.body {
            EventBody::Automation {
                target: AutomationTarget::StripFader(_),
                value,
            } => Some((e.tick.0, value)),
            _ => None,
        })
        .collect();
    assert_eq!(autos.len(), 3);
    assert!((autos[1].1 - 0.631).abs() < 1e-9 && (autos[2].1 - 1.0).abs() < 1e-9);
    assert!(drums.iter().any(|e| e.tick.0 == 3 * BEAT
        && matches!(
            e.body,
            EventBody::Automation {
                target: AutomationTarget::StripMute(_),
                value
            } if value == 1.0
        )));
    assert_eq!(rules::validate(a.h.doc()), Vec::<String>::new());
}

/// A chain of `depth` reference hops from track "A" to a note, each
/// timeline holding `fan` clips that use the next.
fn chain(depth: usize, fan: usize) -> String {
    let mut defs = String::new();
    for k in 1..depth {
        let clips: String = (0..fan)
            .map(|_| {
                format!(
                    r#"<Clip time="0" duration="1" playStart="0" reference="t{}"/>"#,
                    k + 1
                )
            })
            .collect();
        defs.push_str(&format!(r#"<Clips id="t{k}">{clips}</Clips>"#));
    }
    defs.push_str(&format!(
        r#"<Notes id="t{depth}"><Note time="0" duration="1" channel="0" key="60" vel="0.5"/></Notes>"#
    ));
    project_xml(
        &note_track("a", "A"),
        &format!(
            r#"<Lanes timeUnit="beats"><Lanes track="a"><Clips><Clip time="0" duration="1" playStart="0" reference="t1"/></Clips></Lanes><Lanes>{defs}</Lanes></Lanes>"#
        ),
    )
}

#[test]
fn reference_chains_are_bounded() {
    let a = imported(&chain(8, 1));
    let s = sequence(&a, uses_of(&events(&a, "A"))[0].1);
    assert_eq!(notes_of(&seq_events(&s)), vec![(0, 60, BEAT)], "8 hops");
    lacks(&a.report, NESTED_TOO_DEEP);

    let a = imported(&chain(40, 1));
    has(&a.report, ItemKind::LeftOut, NESTED_TOO_DEEP);
    let all_notes =
        a.h.doc()
            .all_events()
            .filter(|(_, e)| matches!(e.body, EventBody::Note { .. }))
            .count();
    assert_eq!(all_notes, 0, "the note 40 hops down is left out");

    // Ten uses at each of 15 levels would be 10^14 notes from a file of
    // about 150 parts: the walk stops at 16 times the file's parts (plus
    // 65 536) and says so.
    let start = std::time::Instant::now();
    let a = imported(&chain(15, 10));
    has(&a.report, ItemKind::LeftOut, TOO_MUCH);
    let made =
        a.h.doc()
            .all_events()
            .filter(|(_, e)| matches!(e.body, EventBody::Note { .. }))
            .count();
    assert!(
        made > 0 && made < EXPANSION_FLOOR + EXPANSION * 200,
        "{made}"
    );
    assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
}

#[test]
fn offline_audio_keeps_its_regions() {
    let dir = tempfile::tempdir().unwrap();
    let structure = r#"<Track id="a" name="A" contentType="audio"><Channel id="ac" role="regular" destination="mc"/></Track>"#;
    let arrangement = r#"<Lanes timeUnit="beats"><Lanes track="a"><Clips><Clip time="0" duration="2" contentTimeUnit="seconds" playStart="0" playStop="1"><Audio sampleRate="44100" channels="1" duration="2"><File path="audio/gone.wav"/></Audio></Clip></Clips></Lanes></Lanes>"#;
    let pkg = package(dir.path(), &project_xml(structure, arrangement));
    let media = BTreeMap::from([(
        0usize,
        MediaSource::Offline {
            why: "it is damaged".into(),
        },
    )]);
    let a = apply_to_new(&mapped(&pkg, &media));
    let doc = a.h.doc();
    let items: Vec<_> = doc.media().values().collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].path, "Media/gone.caf");
    assert_eq!(
        (
            items[0].sample_rate,
            items[0].channels,
            items[0].length_frames
        ),
        (44_100, 1, 88_200)
    );
    assert_eq!(
        regions_of(&events(&a, "A")),
        vec![(0, 0, 44_100, 0, 0, false, false)]
    );
    let i = item(&a.report, ItemKind::Note, OFFLINE).expect("the offline line");
    assert_eq!(i.details, vec!["gone: it is damaged".to_owned()]);

    // An offline file placed before an online one of the same stem never
    // takes the online item's path, even in another case (as the disk
    // compares them).
    let structure = r#"<Track id="a" name="A" contentType="audio"><Channel id="ac" role="regular" destination="mc"/></Track>"#;
    let clip = |time: u32, path: &str| {
        format!(
            r#"<Clip time="{time}" duration="2" contentTimeUnit="seconds" playStart="0" playStop="1"><Audio sampleRate="48000" channels="1" duration="2"><File path="{path}"/></Audio></Clip>"#
        )
    };
    let arrangement = format!(
        r#"<Lanes timeUnit="beats"><Lanes track="a"><Clips>{}{}{}</Clips></Lanes></Lanes>"#,
        clip(0, "audio/kick.wav"),
        clip(4, "samples/kick.wav"),
        clip(8, "loops/KICK.aif")
    );
    let pkg = package(dir.path(), &project_xml(structure, &arrangement));
    let keys: Vec<(usize, String)> = pkg
        .audio()
        .iter()
        .map(|a| (a.key, a.name.clone()))
        .collect();
    assert_eq!(keys.len(), 3, "{keys:?}");
    let online = Imported {
        path: "Media/Kick.caf".into(),
        name: "kick".into(),
        original_name: "kick.wav".into(),
        channels: 1,
        sample_rate: 48_000,
        length_frames: 96_000,
        content_hash: "h".into(),
        clipped: 0,
    };
    let gone = || MediaSource::Offline {
        why: "it is missing".into(),
    };
    let media = BTreeMap::from([
        (keys[0].0, gone()),
        (keys[1].0, MediaSource::Online(vec![online])),
        (keys[2].0, gone()),
    ]);
    let a = apply_to_new(&mapped(&pkg, &media));
    let mut paths: Vec<String> = a.h.doc().media().values().map(|m| m.path.clone()).collect();
    paths.sort();
    assert_eq!(
        paths,
        ["Media/KICK 3.caf", "Media/Kick.caf", "Media/kick 2.caf"]
    );
    assert_eq!(regions_of(&events(&a, "A")).len(), 3);
}

#[test]
fn disabled_clips_import_muted() {
    let clip = |time: u32, attrs: &str, inner: &str| {
        format!(r#"<Clip time="{time}" duration="2" playStart="0"{attrs}>{inner}</Clip>"#)
    };
    let note = |key: u8| {
        format!(r#"<Notes><Note time="0" duration="1" channel="0" key="{key}" vel="0.5"/></Notes>"#)
    };
    let clips = [
        clip(0, r#" enable="false""#, &note(60)),
        // A disabled clip holding a clip: what it holds stays muted.
        clip(
            4,
            r#" enable="false""#,
            &format!("<Clips>{}</Clips>", clip(0, "", &note(61))),
        ),
        clip(8, "", r#"<Notes id="shared"><Note time="0" duration="1" channel="0" key="62" vel="0.5"/></Notes>"#),
        clip(12, r#" enable="false" reference="shared""#, ""),
    ]
    .concat();
    let xml = project_xml(
        &note_track("t", "M"),
        &format!(
            r#"<Lanes timeUnit="beats"><Lanes track="t"><Clips>{clips}</Clips></Lanes></Lanes>"#
        ),
    );
    let a = imported(&xml);
    let evs = events(&a, "M");
    let muted: Vec<(i64, bool)> = evs
        .iter()
        .filter(|e| matches!(e.body, EventBody::Note { .. }))
        .map(|e| (e.tick.0, e.mute))
        .collect();
    assert_eq!(muted, vec![(0, true), (4 * BEAT, true)]);
    let uses = uses_of(&evs);
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].1, uses[1].1, "one sequence");
    assert_eq!(
        (uses[0].4, uses[1].4),
        (false, true),
        "the disabled use is muted"
    );
    let inside = seq_events(&sequence(&a, uses[0].1));
    assert!(inside.iter().all(|e| !e.mute), "the sequence itself plays");
}

#[test]
fn wide_audio_splits_into_mono_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let structure = r#"<Track id="a" name="Wide" contentType="audio"><Channel id="ac" role="regular" destination="mc"/></Track>"#;
    let arrangement = r#"<Lanes timeUnit="beats"><Lanes track="a"><Clips><Clip time="0" duration="2" contentTimeUnit="seconds" playStart="0" playStop="1"><Audio sampleRate="48000" channels="3" duration="2"><File path="audio/wide.wav"/></Audio></Clip></Clips></Lanes></Lanes>"#;
    let pkg = package(dir.path(), &project_xml(structure, arrangement));
    let mono = |n: u32| Imported {
        path: format!("Media/wide {n}.caf"),
        name: format!("wide {n}"),
        original_name: "wide.wav".into(),
        channels: 1,
        sample_rate: 48_000,
        length_frames: 96_000,
        content_hash: format!("h{n}"),
        clipped: 0,
    };
    let media = BTreeMap::from([(0usize, MediaSource::Online(vec![mono(1), mono(2), mono(3)]))]);
    let a = apply_to_new(&mapped(&pkg, &media));
    for name in ["Wide", "Wide ch 2", "Wide ch 3"] {
        let l = lane(&a, name);
        assert!(l.audio_out.is_some(), "{name} has an output path");
        assert_eq!(
            regions_of(&events_of(&l)),
            vec![(0, 0, 48_000, 0, 0, false, false)],
            "{name}"
        );
    }
    assert_eq!(a.h.doc().media().len(), 3);
    let i = item(&a.report, ItemKind::Changed, WIDE_AUDIO).expect("the split line");
    assert_eq!(i.count, 2);
}

#[test]
fn long_loops_stop_at_the_limit() {
    // A loop of one beat repeated for ten million beats, inside a clip (so
    // it is unrolled rather than used).
    let inner = r#"<Clip time="0" duration="10000000" playStart="0" loopStart="0" loopEnd="1"><Notes><Note time="0" duration="0.5" channel="0" key="60" vel="0.5"/></Notes></Clip>"#;
    let xml = project_xml(
        &note_track("t", "Long"),
        &format!(
            r#"<Lanes timeUnit="beats"><Lanes track="t"><Clips><Clip time="0" duration="10000000" playStart="0"><Clips>{inner}</Clips></Clip></Clips></Lanes></Lanes>"#
        ),
    );
    let start = std::time::Instant::now();
    let a = imported(&xml);
    has(&a.report, ItemKind::LeftOut, TOO_MUCH);
    let notes = notes_of(&events(&a, "Long"));
    assert!(
        notes.len() > 1_000 && notes.len() < EXPANSION_FLOOR + 1_000,
        "{}",
        notes.len()
    );
    assert_eq!(notes[1], (BEAT, 60, BEAT / 2), "one note per pass");
    assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());

    // The walk looks at the cancel flag as it unrolls: paused at its
    // 20 000th item, it stops within one interval of the flag.
    let dir = tempfile::tempdir().unwrap();
    let pkg = package(dir.path(), &xml);
    let flag = AtomicBool::new(false);
    debug_pause_at(&flag, Some(20_000));
    let env = ImportEnv {
        installed: &yes,
        plist: &NoCodec,
        rate: 48_000,
    };
    let got = std::thread::scope(|s| {
        s.spawn(|| {
            while !debug_paused() {
                std::thread::yield_now();
            }
            flag.store(true, Ordering::Relaxed);
        });
        map(&pkg, &BTreeMap::new(), &env, &flag).map(|_| ())
    });
    let after = debug_items_after_pause();
    debug_pause_at(&flag, None);
    assert_eq!(got, Err(IoError::Cancelled));
    assert!(after <= CANCEL_EVERY, "{after} items after the flag");

    // A loop shorter than a tick is no loop.
    let xml = project_xml(
        &note_track("t", "Tiny"),
        r#"<Lanes timeUnit="beats"><Lanes track="t"><Clips><Clip time="0" duration="4" playStart="0"><Clips><Clip time="0" duration="4" playStart="0" loopStart="0" loopEnd="0.00000001"><Notes><Note time="0" duration="1" channel="0" key="60" vel="0.5"/></Notes></Clip></Clips></Clip></Clips></Lanes></Lanes>"#,
    );
    let a = imported(&xml);
    assert_eq!(notes_of(&events(&a, "Tiny")), vec![(0, 60, BEAT)]);
    let values = item(&a.report, ItemKind::LeftOut, UNREADABLE_VALUES).expect("values");
    assert!(values.details.iter().any(|d| d == "Clip@loopEnd"));
}

#[test]
fn mapping_is_deterministic() {
    let xml = std::fs::read_to_string(format!("{DIR}/hand_timeline.project.xml")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pkg = package(dir.path(), &xml);
    let media = converted(&pkg);
    let first = mapped(&pkg, &media);
    assert_eq!(first, mapped(&pkg, &media));
    let (a, b) = (apply_to_new(&first), apply_to_new(&first));
    assert_eq!(a.h.doc(), b.h.doc());
    assert_eq!(a.report, b.report);
}

/// A track "Stack" whose one clip holds `outer` clips, each a use of 32
/// clips that crossfade (a negative `fadeInTime`) into one 600 s file:
/// each region starts 64 ticks after the one before and overlaps all the
/// others.
fn region_stack(outer: usize) -> String {
    let inner: String = (0..32)
        .map(|k| {
            format!(
                r#"<Clip time="{}" duration="1200" contentTimeUnit="seconds" playStart="0.5" fadeTimeUnit="seconds" fadeInTime="-0.001" reference="aud"/>"#,
                (64 * k) as f64 / 15_360.0
            )
        })
        .collect();
    let outer: String = (0..outer)
        .map(|i| {
            format!(
                r#"<Clip time="{}" duration="3000" playStart="0" reference="l"/>"#,
                (2048 * i) as f64 / 15_360.0
            )
        })
        .collect();
    project_xml(
        r#"<Track id="a" name="Stack" contentType="audio"><Channel id="ac" role="regular" destination="mc"/></Track>"#,
        &format!(
            r#"<Lanes timeUnit="beats"><Lanes track="a"><Clips><Clip time="0" duration="5000" playStart="0"><Clips>{outer}</Clips></Clip></Clips></Lanes><Lanes><Clips id="l">{inner}</Clips><Audio id="aud" sampleRate="48000" channels="1" duration="600"><File path="audio/s.wav"/></Audio></Lanes></Lanes>"#
        ),
    )
}

#[test]
fn overlapping_region_stacks_are_bounded() {
    // About 90 000 regions that all overlap, from a file of 8 000 clips:
    // at most MAX_SOUNDING of them sound at once, the rest are left out,
    // and the import stays fast.
    let xml = region_stack(8_000);
    let start = std::time::Instant::now();
    let a = imported(&xml);
    let regions = regions_of(&events(&a, "Stack"));
    assert_eq!(
        regions.len(),
        MAX_SOUNDING,
        "the regions that may sound at once"
    );
    assert!(
        regions[1..].iter().all(|r| r.5),
        "each crossfades into the one before"
    );
    has(&a.report, ItemKind::LeftOut, TOO_MUCH);
    assert_eq!(rules::validate(a.h.doc()), Vec::<String>::new());
    assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());

    // The passes after the walk look at the cancel flag: paused 70 000
    // items before the end (among the regions), the mapping stops within
    // one interval of the flag.
    let dir = tempfile::tempdir().unwrap();
    let pkg = package(dir.path(), &xml);
    let media = converted(&pkg);
    let env = ImportEnv {
        installed: &yes,
        plist: &NoCodec,
        rate: 48_000,
    };
    let flag = AtomicBool::new(false);
    debug_pause_at(&flag, Some(usize::MAX));
    map(&pkg, &media, &env, &flag).expect("maps");
    let total = debug_items_seen();
    debug_pause_at(&flag, Some(total - 70_000));
    let got = std::thread::scope(|s| {
        s.spawn(|| {
            while !debug_paused() {
                std::thread::yield_now();
            }
            flag.store(true, Ordering::Relaxed);
        });
        map(&pkg, &media, &env, &flag).map(|_| ())
    });
    let after = debug_items_after_pause();
    debug_pause_at(&flag, None);
    assert_eq!(got, Err(IoError::Cancelled));
    assert!(after <= CANCEL_EVERY, "{after} items after the flag");
}

#[test]
fn many_shared_clips_are_bounded() {
    // Twice MAX_NEW_SEQUENCES clips, each the only use of its own notes,
    // beside an audio clip (so the model looks at crossfades on every
    // command): the first MAX_NEW_SEQUENCES become sequences, the rest are
    // unrolled onto the track (loops repeated), and the import stays fast.
    let n = 2 * MAX_NEW_SEQUENCES;
    // The last clip loops its one beat three times.
    let clips: String = (0..n)
        .map(|k| match k + 1 == n {
            false => format!(r#"<Clip time="{k}" duration="1" playStart="0" reference="n{k}"/>"#),
            true => format!(
                r#"<Clip time="{k}" duration="3" playStart="0" loopStart="0" loopEnd="1" reference="n{k}"/>"#
            ),
        })
        .collect();
    let defs: String = (0..n)
        .map(|k| {
            format!(
                r#"<Notes id="n{k}"><Note time="0" duration="1" channel="0" key="{}" vel="0.5"/></Notes>"#,
                k % 128
            )
        })
        .collect();
    let structure = format!(
        r#"{}<Track id="a" name="Audio" contentType="audio"><Channel id="ac" role="regular" destination="mc"/></Track>"#,
        note_track("t", "Many")
    );
    let xml = project_xml(
        &structure,
        &format!(
            r#"<Lanes timeUnit="beats"><Lanes track="t"><Clips>{clips}</Clips></Lanes><Lanes track="a"><Clips><Clip time="0" duration="2" contentTimeUnit="seconds" playStart="0" playStop="1"><Audio sampleRate="48000" channels="1" duration="2"><File path="audio/x.wav"/></Audio></Clip></Clips></Lanes><Lanes>{defs}</Lanes></Lanes>"#
        ),
    );
    let start = std::time::Instant::now();
    let a = imported(&xml);
    let uses = uses_of(&events(&a, "Many"));
    assert_eq!(uses.len(), MAX_NEW_SEQUENCES);
    assert_eq!(a.h.doc().sequences().len(), MAX_NEW_SEQUENCES + 1);
    lacks(&a.report, TOO_MUCH);
    has(&a.report, ItemKind::Changed, UNROLLED);
    let notes = notes_of(&events(&a, "Many"));
    let k = MAX_NEW_SEQUENCES as i64;
    assert_eq!(
        notes.first(),
        Some(&(k * BEAT, 0, BEAT)),
        "the 1 025th clip's note"
    );
    let last = (n - 1) as i64;
    assert_eq!(
        notes[notes.len() - 3..],
        [
            (last * BEAT, (n - 1) as u8 % 128, BEAT),
            ((last + 1) * BEAT, (n - 1) as u8 % 128, BEAT),
            ((last + 2) * BEAT, (n - 1) as u8 % 128, BEAT),
        ],
        "the looped one repeated"
    );
    assert_eq!(notes.len(), n - MAX_NEW_SEQUENCES + 2);
    assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
}

#[test]
fn quiet_drops_are_listed() {
    // Expression past the end of a note that was not cut; notes whose main
    // channel is not the first; a clip name past the limit.
    let long = "n".repeat(5_000);
    let notes = format!(
        r#"<Lanes track="t"><Clips><Clip time="0" duration="4" playStart="0" name="{long}"><Notes><Note time="0" duration="1" channel="2" key="60" vel="0.5"><Lanes><Points unit="normalized"><Target expression="timbre"/><RealPoint time="2" value="0.5"/></Points></Lanes></Note></Notes></Clip></Clips></Lanes>"#
    );
    // A region shorter than a frame, one past its file's end, and a
    // negative fade-in where the file starts.
    let audio = |play: &str, fade: &str| {
        format!(
            r#"<Clip time="4" duration="2" contentTimeUnit="seconds" {play} fadeTimeUnit="seconds" {fade}><Audio sampleRate="48000" channels="1" duration="2"><File path="audio/x.wav"/></Audio></Clip>"#
        )
    };
    let clips = [
        audio(r#"playStart="0" playStop="0.000001""#, ""),
        audio(r#"playStart="5" playStop="6""#, ""),
        audio(r#"playStart="0" playStop="1""#, r#"fadeInTime="-0.5""#),
    ]
    .concat();
    let structure = format!(
        r#"{}<Track id="a" name="A" contentType="audio"><Channel id="ac" role="regular" destination="mc"/></Track>"#,
        note_track("t", "N")
    );
    let a = imported(&project_xml(
        &structure,
        &format!(
            r#"<Lanes timeUnit="beats">{notes}<Lanes track="a"><Clips>{clips}</Clips></Lanes></Lanes>"#
        ),
    ));
    has(&a.report, ItemKind::LeftOut, EXPRESSION_PAST_END);
    let i = item(&a.report, ItemKind::Changed, MAIN_CHANNEL).expect("the channel line");
    assert_eq!(i.details, vec!["N: channel 3".to_owned()]);
    has(&a.report, ItemKind::Changed, NAMES_CUT);
    has(&a.report, ItemKind::LeftOut, AUDIO_TOO_SHORT);
    has(&a.report, ItemKind::LeftOut, AUDIO_PAST_FILE);
    has(&a.report, ItemKind::Changed, CROSSFADES_PLAIN);
    assert_eq!(
        regions_of(&events(&a, "A")),
        vec![(4 * BEAT, 0, 48_000, 0, 0, false, false)],
        "the fade-in had no file before it"
    );
}

#[test]
fn exported_fills_and_channels_come_back() {
    // What Task 5's timeline fixture does not hold: a use that fills its
    // length, and a note on a channel other than its track's.
    use core_engine::fixtures::{BAR, Builder, Q};
    use core_io::dawproject::export::{ClipRule, ExportEnv, ExportOptions, RegionGain, export};
    use core_io::dawproject::plugins::NoPlist;
    use core_model::event::EventSpec;
    use core_model::mixer::BUILTIN_SYNTH_REF;

    let mut b = Builder::new();
    let synth = b.dest("Synth", BUILTIN_SYNTH_REF, 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    let (riff, riff_lane) = b.sequence("Riff", "Riff", Some(synth)).unwrap();
    b.notes(riff, riff_lane, &[(0, 62, 0x8000, Q)]).unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq: riff,
        length: SeqLength::Locked(Tick(BAR)),
    }))
    .unwrap();
    b.notes(
        seq,
        keys,
        &[(4 * BAR, 60, 0x8000, Q), (5 * BAR, 60, 0x8000, Q)],
    )
    .unwrap();
    let mut on_four = EventSpec::new(
        Tick(6 * BAR),
        EventBody::Note {
            pitch: 64,
            velocity: 0x8000,
            release_velocity: 0,
            duration: Tick(Q),
            attribute: None,
            per_note: Default::default(),
        },
    );
    on_four.group_channel = Some((0, 3));
    let fill = EventSpec::new(
        Tick(0),
        EventBody::Reference {
            seq: riff,
            length: Some(Tick(6 * Q)),
            loop_count: LoopCount::Fill,
            transpose: 0,
        },
    );
    b.events(seq, keys, vec![on_four, fill]).unwrap();
    let doc = b.finish();
    let x = export(
        &doc,
        &ExportOptions {
            seq,
            region_gain: RegionGain::Apply,
            clips: ClipRule::SilenceGap(Tick(4 * Q)),
        },
        &ExportEnv {
            plist: &NoPlist,
            app_version: "1.2.3",
            project_id: "00000000-0000-4000-8000-000000000001",
            export_id: "E1",
            exported_at: 1_700_000_000,
        },
        &AtomicBool::new(false),
    )
    .expect("exports");

    let a = imported(&x.project_xml);
    let evs = events(&a, "Keys");
    // Task 5 writes a filling use as its notes, unrolled, in a clip named
    // after the sequence (no loop): it comes back as those notes.
    assert_eq!(uses_of(&evs), vec![]);
    assert_eq!(
        notes_of(&evs),
        vec![
            (0, 62, Q),
            (BAR, 62, Q),
            (4 * BAR, 60, Q),
            (5 * BAR, 60, Q),
            (6 * BAR, 64, Q)
        ]
    );
    let channels: Vec<(i64, Option<(u8, u8)>)> = evs
        .iter()
        .filter(|e| matches!(e.body, EventBody::Note { .. }))
        .map(|e| (e.tick.0, e.group_channel))
        .collect();
    assert_eq!(
        channels,
        vec![
            (0, None),
            (BAR, None),
            (4 * BAR, None),
            (5 * BAR, None),
            (6 * BAR, Some((0, 3)))
        ]
    );
    assert_eq!(rules::validate(a.h.doc()), Vec::<String>::new());
}
