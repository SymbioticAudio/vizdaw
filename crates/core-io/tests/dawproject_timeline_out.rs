//! DAWproject export of the arrangement: note clips, per-note expression,
//! controller points, reference uses, audio clips, fades, crossfades and
//! automation (plan 6d Task 5).

mod common;

use common::xmllint::schema_check;
use core_engine::fixtures::{BAR, Builder, Q};
use core_io::IoError;
use core_io::dawproject::export::{
    AudioFile, CANCEL_EVERY, CROSSFADES, ClipRule, DpExport, EVENTS_OUTSIDE, ExportEnv,
    ExportOptions, FADE_SHAPES, MIDI_GROUPS, MUTED_NOTES, NESTED_AUDIO, NESTED_MUTED,
    NOTE_ATTRIBUTES, OTHER_PER_NOTE, OVERLAPS, REGION_GAIN, RegionGain, SOLO_AUTOMATION, SYSEX,
    TEXTS, TRANSPOSED, UNPLACED_AUTOMATION, VELOCITY_SCALE_AUTOMATION, check, debug_cancel_after,
    debug_items_after_cancel, export,
};
use core_io::dawproject::plugins::{NoPlist, PlistCodec};
use core_io::dawproject::record::{Snapshot, projection};
use core_io::dawproject::schema::*;
use core_io::limits::PackageLimits;
use core_io::report::{ItemKind, Report};
use core_io::xml::{Tree, double};
use core_model::automation::AutomationTarget;
use core_model::automation::SendIndex;
use core_model::command::MixerCmd;
use core_model::command::{
    AudioCmd, Command, CrossfadeLength, EventCmd, MediaCmd, RoutingCmd, SequenceCmd,
};
use core_model::document::{DestKind, Document, MidiMix, Patch};
use core_model::event::{
    EventBody, EventSpec, Fade, FadeShape, NoteAttribute, PerNoteCtl, PerNoteKind, TextKind,
};
use core_model::ids::{AnyId, DestId, EventId, LaneId, MediaId, OutputPathId, SeqId, StripId};
use core_model::mixer::{AuxSend, ParamId, PluginFormat, PluginRef};
use core_model::mixer::{BUILTIN_SYNTH_REF, StripKind};
use core_model::sequence::SeqLength;
use core_model::time::Tick;
use std::sync::atomic::{AtomicBool, Ordering};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/dawproject");

// ---------------------------------------------------------------- helpers

fn env(plist: &dyn PlistCodec) -> ExportEnv<'_> {
    ExportEnv {
        plist,
        app_version: "1.2.3",
        project_id: "00000000-0000-4000-8000-000000000001",
        export_id: "E1",
        exported_at: 1_700_000_000,
    }
}

fn opts(seq: SeqId, clips: ClipRule, region_gain: RegionGain) -> ExportOptions {
    ExportOptions {
        seq,
        region_gain,
        clips,
        left_out: Default::default(),
    }
}

const GAP: ClipRule = ClipRule::SilenceGap(Tick(4 * Q));

fn run_opts(doc: &Document, o: ExportOptions) -> DpExport {
    export(doc, &o, &env(&NoPlist), &AtomicBool::new(false)).expect("exports")
}

fn run(doc: &Document, seq: SeqId) -> DpExport {
    run_opts(doc, opts(seq, GAP, RegionGain::Apply))
}

/// The never-blessed golden reader of 6a: a hand-written file, byte-exact.
#[track_caller]
fn hand_golden(name: &str, got: &[u8]) {
    let want = std::fs::read(format!("{DIR}/{name}"))
        .unwrap_or_else(|e| panic!("hand-written golden {name} is missing: {e}"));
    assert_eq!(
        String::from_utf8_lossy(got),
        String::from_utf8_lossy(&want),
        "{name}"
    );
}

fn read_back(x: &DpExport) -> DpProject {
    let tree = Tree::parse(
        x.project_xml.as_bytes(),
        &PackageLimits::default(),
        &AtomicBool::new(false),
    )
    .expect("parses");
    let mut r = Report::default();
    let p = read_project(&tree, &mut r).expect("reads back");
    assert!(r.is_empty(), "reading back: {:?}", r.items);
    p
}

fn validates(x: &DpExport) {
    schema_check(&format!("{DIR}/Project.xsd"), &x.project_xml);
    let p = read_back(x);
    let mut r = Report::default();
    check_ids(&p, &mut r).expect("IDs are unique and resolve");
    assert!(r.is_empty(), "ID check: {:?}", r.items);
}

/// The arrangement's `Lanes` of track `id`.
fn lanes<'a>(p: &'a DpProject, id: &str) -> &'a DpLanes {
    let root = p
        .arrangement
        .as_ref()
        .and_then(|a| a.lanes.as_ref())
        .expect("arrangement lanes");
    root.items
        .iter()
        .find_map(|t| match t {
            DpTimeline::Lanes(l) if l.head.track.as_deref() == Some(id) => Some(l),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no lanes for track {id}"))
}

/// The clips of track `id`.
fn clips<'a>(p: &'a DpProject, id: &str) -> &'a [DpClip] {
    lanes(p, id)
        .items
        .iter()
        .find_map(|t| match t {
            DpTimeline::Clips(c) => Some(c.clips.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

/// The track-level points of track `id`.
fn track_points<'a>(p: &'a DpProject, id: &str) -> Vec<&'a DpPoints> {
    lanes(p, id)
        .items
        .iter()
        .filter_map(|t| match t {
            DpTimeline::Points(x) => Some(x),
            _ => None,
        })
        .collect()
}

/// The note clips the clip rule made (reference uses are named).
fn rule_clips<'a>(p: &'a DpProject, id: &str) -> Vec<&'a DpClip> {
    clips(p, id)
        .iter()
        .filter(|c| c.named.name.is_none() && c.content_time_unit.is_none())
        .collect()
}

/// Every timeline in `t`, depth first.
fn walk<'a>(t: &'a DpTimeline, out: &mut Vec<&'a DpTimeline>) {
    out.push(t);
    match t {
        DpTimeline::Lanes(l) => l.items.iter().for_each(|i| walk(i, out)),
        DpTimeline::Clips(c) => c.clips.iter().for_each(|c| {
            if let Some(x) = &c.content {
                walk(x, out);
            }
        }),
        DpTimeline::Notes(n) => n.notes.iter().for_each(|n| {
            if let Some(x) = &n.content {
                walk(x, out);
            }
        }),
        _ => {}
    }
}

fn notes_of(c: &DpClip) -> Vec<&DpNote> {
    let mut all = Vec::new();
    if let Some(t) = &c.content {
        walk(t, &mut all);
    }
    all.into_iter()
        .filter_map(|t| match t {
            DpTimeline::Notes(n) => Some(n.notes.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn points_of(c: &DpClip) -> Vec<&DpPoints> {
    let mut all = Vec::new();
    if let Some(t) = &c.content {
        walk(t, &mut all);
    }
    all.into_iter()
        .filter_map(|t| match t {
            DpTimeline::Points(p) => Some(p),
            _ => None,
        })
        .collect()
}

fn count(r: &Report, kind: ItemKind, what: &str) -> usize {
    r.items
        .iter()
        .filter(|i| i.kind == kind && i.what == what)
        .count()
}

fn item<'a>(r: &'a Report, kind: ItemKind, what: &str) -> &'a core_io::report::Item {
    r.items
        .iter()
        .find(|i| i.kind == kind && i.what == what)
        .unwrap_or_else(|| panic!("no {kind:?} '{what}' in {:#?}", r.items))
}

fn output(b: &mut Builder, name: &str) -> OutputPathId {
    let r = b
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: name.into(),
            reference: format!("hw:{name}"),
        }))
        .unwrap();
    r.created
        .iter()
        .find_map(|i| match i {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .unwrap()
}

fn route(b: &mut Builder, seq: SeqId, lane: LaneId, out: OutputPathId) {
    b.run(Command::Lane(core_model::command::LaneCmd::SetOutput {
        seq,
        lane,
        output: Some(out),
    }))
    .unwrap();
}

fn mute(b: &mut Builder, event: EventId) {
    b.run(Command::Event(EventCmd::SetMute {
        events: vec![event],
        mute: true,
    }))
    .unwrap();
}

fn spec(tick: i64, body: EventBody) -> EventSpec {
    EventSpec::new(Tick(tick), body)
}

fn note_with(pitch: u8, duration: i64, per_note: Vec<PerNoteCtl>) -> EventBody {
    EventBody::Note {
        pitch,
        velocity: 0x8000,
        release_velocity: 0,
        duration: Tick(duration),
        attribute: None,
        per_note: per_note.into_iter().collect(),
    }
}

fn automation(target: AutomationTarget, value: f64) -> EventBody {
    EventBody::Automation { target, value }
}

fn plain_region(b: &mut Builder, seq: SeqId, lane: LaneId, tick: i64, media: MediaId) -> EventId {
    b.region(
        seq,
        lane,
        tick,
        media,
        0,
        48_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap()
}

fn length(b: &mut Builder, seq: SeqId, ticks: i64) {
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(ticks)),
    }))
    .unwrap();
}

/// The per-note pitch bend of +2 semitones (as near as 32 bits get).
const BEND_2: u32 = 0x8000_0000 + 89_478_485;

/// IDs of the timeline fixture.
struct Timeline {
    doc: Document,
    seq: SeqId,
    keys: LaneId,
    drums: LaneId,
    drum_strip: StripId,
    synth_strip: StripId,
    a: EventId,
    b: EventId,
    looped: EventId,
    muted: EventId,
}

/// Step 1's fixture, at 120 BPM:
/// - "Keys" on the built-in synth: C4 at beats 0 and 1 (a beat each), a C4
///   at beat 12 (two beats) with per-note pitch bend +2 semitones at offset 0
///   and `Registered(74)` 0.5 at offset 1 beat; CC 1 = 0.25 at beat 0.5;
///   `Program { 5, bank (0, 1) }` at beat 12; "Riff" (one bar, a D4 on its
///   first beat) looped twice at bar 5 and a muted use of it at bar 9.
/// - "Drums": region A at tick 0 (48 000 frames, a 480-frame Linear fade
///   in), region B "Hit" at tick 15 360 (frame 24 000) on the same media,
///   48 000 frames at −6 dB, crossfading into A over 4 800 frames (Linear);
///   fader automation 0.0, 0.631 and 1.0 at beats 0, 1 and 2 and mute on at
///   beat 3.
fn timeline_fixture() -> Timeline {
    let mut b = Builder::new();
    let synth = b.dest("Synth", BUILTIN_SYNTH_REF, 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    let drums = b.lane(seq, "Drums", None).unwrap();
    let out = output(&mut b, "Drums");
    route(&mut b, seq, drums, out);
    let (riff, riff_lane) = b.sequence("Riff", "Riff", Some(synth)).unwrap();
    b.notes(riff, riff_lane, &[(0, 62, 0x8000, Q)]).unwrap();
    length(&mut b, riff, BAR);

    b.notes(seq, keys, &[(0, 60, 0x8000, Q), (Q, 60, 0x8000, Q)])
        .unwrap();
    b.events(
        seq,
        keys,
        vec![
            spec(
                12 * Q,
                note_with(
                    60,
                    2 * Q,
                    vec![
                        PerNoteCtl {
                            kind: PerNoteKind::PitchBend,
                            offset: Tick(0),
                            value: BEND_2,
                        },
                        PerNoteCtl {
                            kind: PerNoteKind::Registered(74),
                            offset: Tick(Q),
                            value: 0x8000_0000,
                        },
                    ],
                ),
            ),
            spec(
                Q / 2,
                EventBody::Cc {
                    index: 1,
                    value: 0x4000_0000,
                },
            ),
            spec(
                12 * Q,
                EventBody::Program {
                    program: 5,
                    bank: Some((0, 1)),
                },
            ),
        ],
    )
    .unwrap();
    let looped = b.reference(seq, keys, 4 * BAR, riff, 2, 0).unwrap();
    let muted = b.reference(seq, keys, 8 * BAR, riff, 1, 0).unwrap();
    mute(&mut b, muted);

    let media = b.media("Media/a.caf", 48_000, 2).unwrap();
    let a = b
        .region(
            seq,
            drums,
            0,
            media,
            0,
            48_000,
            0.0,
            Fade {
                length_frames: 480,
                shape: FadeShape::Linear,
            },
            Fade::default(),
        )
        .unwrap();
    let rb = b
        .region(
            seq,
            drums,
            Q,
            media,
            0,
            48_000,
            -6.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    b.run(Command::Audio(AudioCmd::SetName {
        event: rb,
        name: Some("Hit".into()),
    }))
    .unwrap();
    b.run(Command::Audio(AudioCmd::Crossfade {
        event: rb,
        length: CrossfadeLength::Frames(4_800),
        shape: FadeShape::Linear,
    }))
    .unwrap();
    let drum_strip = b.strip_of(StripKind::AudioPath(out)).unwrap();
    let synth_strip = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.events(
        seq,
        drums,
        vec![
            spec(0, automation(AutomationTarget::StripFader(drum_strip), 0.0)),
            spec(
                Q,
                automation(AutomationTarget::StripFader(drum_strip), 0.631),
            ),
            spec(
                2 * Q,
                automation(AutomationTarget::StripFader(drum_strip), 1.0),
            ),
            spec(
                3 * Q,
                automation(AutomationTarget::StripMute(drum_strip), 1.0),
            ),
        ],
    )
    .unwrap();
    Timeline {
        doc: b.finish(),
        seq,
        keys,
        drums,
        drum_strip,
        synth_strip,
        a,
        b: rb,
        looped,
        muted,
    }
}

// ---------------------------------------------------------------- tests

#[test]
fn timeline_matches_the_hand_golden() {
    let f = timeline_fixture();
    let x = run(&f.doc, f.seq);
    hand_golden("export_timeline.project.xml", x.project_xml.as_bytes());
    // The fixture's IDs are the ones the golden names.
    assert_eq!(
        (
            f.keys.0,
            f.drums.0,
            f.synth_strip.0,
            f.drum_strip.0,
            f.a.0,
            f.b.0,
            f.looped.0,
            f.muted.0
        ),
        (0, 1, 1, 2, 8, 9, 6, 7)
    );
    assert_eq!(x.audio.len(), 2);
    assert_eq!(x.report.summary.notes, 5);
    assert_eq!(x.report.summary.regions, 2);
    assert_eq!(x.report.summary.audio_files, 2);
    assert_eq!(x.report.summary.automation_points, 4);
    let what: Vec<(ItemKind, &str)> = x
        .report
        .items
        .iter()
        .map(|i| (i.kind, i.what.as_str()))
        .collect();
    assert_eq!(
        what,
        vec![
            (ItemKind::Note, "Track 'Keys' plays the built-in synth here"),
            (ItemKind::Changed, CROSSFADES),
        ]
    );
}

#[test]
fn exports_pass_the_vendored_schema() {
    let f = timeline_fixture();
    for rule in [GAP, ClipRule::FixedBars(1), ClipRule::WholeTrack] {
        for gain in [RegionGain::Apply, RegionGain::LeaveOut] {
            validates(&run_opts(&f.doc, opts(f.seq, rule, gain)));
        }
    }
    let (doc, seq) = listed_fixture();
    validates(&run_opts(&doc, opts(seq, GAP, RegionGain::LeaveOut)));
    let (doc, seq, _) = loops_fixture(None);
    validates(&run(&doc, seq));
    let (doc, seq, _) = loops_fixture(Some(6 * BAR));
    validates(&run(&doc, seq));
    let (doc, seq, _) = patch_fixture();
    validates(&run(&doc, seq));
    let (doc, seq, _) = numbers_fixture();
    validates(&run(&doc, seq));
}

#[test]
fn clip_rules_group_as_the_arrangement_shows() {
    let f = timeline_fixture();
    let keys = format!("t{}", f.keys.0);
    let get = |rule| read_back(&run_opts(&f.doc, opts(f.seq, rule, RegionGain::Apply)));

    let p = get(GAP);
    let c = rule_clips(&p, &keys);
    assert_eq!(c.len(), 2, "{c:#?}");
    assert_eq!((c[0].time, c[0].duration), (0.0, Some(2.0)));
    assert_eq!((c[1].time, c[1].duration), (12.0, Some(2.0)));

    let p = get(ClipRule::FixedBars(1));
    let c = rule_clips(&p, &keys);
    assert_eq!(c.len(), 2, "{c:#?}");
    assert_eq!((c[0].time, c[0].duration), (0.0, Some(4.0)));
    assert_eq!((c[1].time, c[1].duration), (12.0, Some(4.0)));
    assert!(
        points_of(c[1])
            .iter()
            .any(|p| p.target.expression == Some(DpExpression::ProgramChange)),
        "bar 4 holds the program"
    );
    assert_eq!(notes_of(c[0]).len(), 2);
    assert_eq!(notes_of(c[1]).len(), 1);

    let p = get(ClipRule::WholeTrack);
    let c = rule_clips(&p, &keys);
    assert_eq!(c.len(), 1, "{c:#?}");
    assert_eq!((c[0].time, c[0].duration), (0.0, Some(14.0)));
    assert_eq!(notes_of(c[0]).len(), 3);

    // The two uses of "Riff" are clips of their own under every rule.
    for rule in [GAP, ClipRule::FixedBars(1), ClipRule::WholeTrack] {
        let p = get(rule);
        let uses: Vec<_> = clips(&p, &keys)
            .iter()
            .filter(|c| c.named.name.as_deref() == Some("Riff"))
            .collect();
        assert_eq!(uses.len(), 2);
        assert_eq!(
            uses[0].named.comment.as_deref(),
            Some(format!("vz:e{}", f.looped.0).as_str())
        );
        assert_eq!(uses[1].enable, Some(false), "the muted use");
        assert_eq!(
            uses[1].named.comment.as_deref(),
            Some(format!("vz:e{}", f.muted.0).as_str())
        );
    }
}

/// "Song" with "Keys" on the built-in synth and a use of "Riff" (one bar:
/// notes at beats 0 and 2) looped three times at bar 5; with `cut`, the
/// song's length is locked there.
fn loops_fixture(cut: Option<i64>) -> (Document, SeqId, LaneId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", BUILTIN_SYNTH_REF, 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    let (riff, riff_lane) = b.sequence("Riff", "Riff", Some(synth)).unwrap();
    b.notes(
        riff,
        riff_lane,
        &[(0, 62, 0x8000, Q), (2 * Q, 64, 0x8000, Q)],
    )
    .unwrap();
    length(&mut b, riff, BAR);
    b.reference(seq, keys, 4 * BAR, riff, 3, 0).unwrap();
    if let Some(t) = cut {
        length(&mut b, seq, t);
    }
    (b.finish(), seq, keys)
}

#[test]
fn looped_and_unrolled_uses() {
    // Three identical passes: one looped clip.
    let (doc, seq, keys) = loops_fixture(None);
    let p = read_back(&run(&doc, seq));
    let c = clips(&p, &format!("t{}", keys.0));
    assert_eq!(c.len(), 1, "{c:#?}");
    let c = &c[0];
    assert_eq!(c.named.name.as_deref(), Some("Riff"));
    assert_eq!(c.time, 16.0);
    assert_eq!(c.duration, Some(12.0));
    assert_eq!(c.play_start, Some(0.0));
    assert_eq!(c.loop_start, Some(0.0));
    assert_eq!(c.loop_end, Some(4.0));
    let n = notes_of(c);
    assert_eq!(
        n.iter().map(|n| (n.time, n.key)).collect::<Vec<_>>(),
        vec![(0.0, 62), (2.0, 64)]
    );

    // The song ends two bars into the use: the third pass does not play,
    // so the clip is unrolled and holds every played note.
    let (doc, seq, keys) = loops_fixture(Some(6 * BAR));
    let p = read_back(&run(&doc, seq));
    let c = clips(&p, &format!("t{}", keys.0));
    assert_eq!(c.len(), 1, "{c:#?}");
    let c = &c[0];
    assert_eq!(c.time, 16.0);
    assert_eq!(c.duration, Some(8.0));
    assert_eq!((c.loop_start, c.loop_end), (None, None));
    let n = notes_of(c);
    assert_eq!(
        n.iter().map(|n| (n.time, n.key)).collect::<Vec<_>>(),
        vec![(0.0, 62), (2.0, 64), (4.0, 62), (6.0, 64)]
    );
}

const VELOCITIES: [u16; 4] = [0, 1, 32_767, 65_535];
const BENDS: [u32; 4] = [0, 0x8000_0000, 0x8000_0000 + 0x8000_0000 / 48, u32::MAX];
const OFFSETS: [u64; 3] = [0, 1, (1 << 40) - 1];

/// Notes at the edge velocities, per-note bends at the edge values and
/// regions at the edge offsets of a 44.1 kHz file.
fn numbers_fixture() -> (Document, SeqId, [LaneId; 2]) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", BUILTIN_SYNTH_REF, 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    let audio = b.lane(seq, "Audio", None).unwrap();
    let out = output(&mut b, "Audio");
    route(&mut b, seq, audio, out);
    let mut specs: Vec<EventSpec> = VELOCITIES
        .iter()
        .enumerate()
        .map(|(i, v)| {
            spec(
                i as i64 * Q,
                EventBody::Note {
                    pitch: 60,
                    velocity: *v,
                    release_velocity: *v,
                    duration: Tick(Q),
                    attribute: None,
                    per_note: Default::default(),
                },
            )
        })
        .collect();
    specs.push(spec(
        8 * Q,
        note_with(
            62,
            8 * Q,
            BENDS
                .iter()
                .enumerate()
                .map(|(i, v)| PerNoteCtl {
                    kind: PerNoteKind::PitchBend,
                    offset: Tick(i as i64 * Q),
                    value: *v,
                })
                .collect(),
        ),
    ));
    b.events(seq, keys, specs).unwrap();
    let r = b
        .run(Command::Media(MediaCmd::Add {
            path: "Media/long.caf".into(),
            sample_rate: 44_100,
            channels: 1,
            length_frames: 1 << 41,
            content_hash: None,
            name: None,
            origin: None,
        }))
        .unwrap();
    let media = r
        .created
        .iter()
        .find_map(|i| match i {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .unwrap();
    for (i, off) in OFFSETS.iter().enumerate() {
        b.region(
            seq,
            audio,
            i as i64 * 4 * BAR,
            media,
            *off,
            44_100,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    }
    (b.finish(), seq, [keys, audio])
}

#[test]
fn numbers_come_back_exactly() {
    let (doc, seq, [keys, audio]) = numbers_fixture();
    let x = run(&doc, seq);
    let p = read_back(&x);
    // The text as written, through the strict reader.
    let back = |v: f64| double(&core_io::xml::fmt_double(v)).expect("a double");
    let c = clips(&p, &format!("t{}", keys.0));
    let notes: Vec<&DpNote> = c.iter().flat_map(notes_of).collect();
    let vels: Vec<u16> = notes
        .iter()
        .take(4)
        .map(|n| (back(n.vel.unwrap_or(0.0)) * 65_535.0).round() as u16)
        .collect();
    assert_eq!(vels, VELOCITIES);
    let rels: Vec<u16> = notes
        .iter()
        .take(4)
        .map(|n| (back(n.rel.unwrap_or(0.0)) * 65_535.0).round() as u16)
        .collect();
    assert_eq!(rels, VELOCITIES);

    let bent = notes.get(4).expect("the bent note");
    let mut tl = Vec::new();
    walk(bent.content.as_deref().expect("expression"), &mut tl);
    let pts = tl
        .iter()
        .find_map(|t| match t {
            DpTimeline::Points(p) if p.target.expression == Some(DpExpression::Transpose) => {
                Some(p)
            }
            _ => None,
        })
        .expect("transpose points");
    assert_eq!(pts.unit, Some(DpUnit::Semitones));
    let values: Vec<u32> = pts
        .points
        .iter()
        .map(|p| match p.value {
            DpPointValue::Real(s, _) => {
                (back(s) * 2_147_483_648.0 / 48.0 + 2_147_483_648.0).round() as u32
            }
            _ => panic!("a real point"),
        })
        .collect();
    assert_eq!(values, BENDS);
    let times: Vec<f64> = pts.points.iter().map(|p| p.time).collect();
    assert_eq!(times, vec![0.0, 1.0, 2.0, 3.0], "relative to the note");

    let offsets: Vec<u64> = clips(&p, &format!("t{}", audio.0))
        .iter()
        .map(|c| (back(c.play_start.unwrap_or(-1.0)) * 44_100.0).round() as u64)
        .collect();
    assert_eq!(offsets, OFFSETS);
    let stops: Vec<u64> = clips(&p, &format!("t{}", audio.0))
        .iter()
        .map(|c| (back(c.play_stop.unwrap_or(-1.0)) * 44_100.0).round() as u64)
        .collect();
    assert_eq!(stops, OFFSETS.map(|o| o + 44_100));
}

/// A song with each timeline item the file cannot hold as it is, exported
/// with region gain left out.
fn listed_fixture() -> (Document, SeqId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", BUILTIN_SYNTH_REF, 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    let drums = b.lane(seq, "Drums", None).unwrap();
    let out = output(&mut b, "Drums");
    route(&mut b, seq, drums, out);
    let (inner, inner_lane) = b.sequence("Inner", "Inner", Some(synth)).unwrap();
    b.notes(inner, inner_lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let inner_audio = b.lane(inner, "Inner audio", None).unwrap();
    route(&mut b, inner, inner_audio, out);
    let media = b.media("Media/a.caf", 48_000, 2).unwrap();
    plain_region(&mut b, inner, inner_audio, 0, media);

    let mut muted = spec(0, EventBody::note(60, 0x8000, Tick(Q)));
    muted.mute = true;
    let mut grouped = spec(Q, EventBody::note(61, 0x8000, Tick(Q)));
    grouped.group_channel = Some((1, 0));
    b.events(
        seq,
        keys,
        vec![
            muted,
            grouped,
            spec(
                2 * Q,
                EventBody::SysEx {
                    data: vec![0x7E, 0x01].into(),
                    eight_bit: false,
                },
            ),
            spec(
                2 * Q,
                EventBody::Text {
                    kind: TextKind::Lyric,
                    text: "la".into(),
                },
            ),
            spec(
                3 * Q,
                EventBody::Note {
                    pitch: 64,
                    velocity: 0x8000,
                    release_velocity: 0,
                    duration: Tick(Q),
                    attribute: Some(NoteAttribute {
                        kind: 3,
                        data: 0x1234,
                    }),
                    per_note: vec![PerNoteCtl {
                        kind: PerNoteKind::Registered(1),
                        offset: Tick(0),
                        value: 7,
                    }]
                    .into_iter()
                    .collect(),
                },
            ),
            spec(0, automation(AutomationTarget::VelocityScale(synth), 0.5)),
        ],
    )
    .unwrap();
    b.reference(seq, keys, 2 * BAR, inner, 1, 3).unwrap();
    let drum_strip = b.strip_of(StripKind::AudioPath(out)).unwrap();
    b.event(
        seq,
        drums,
        0,
        automation(AutomationTarget::StripSolo(drum_strip), 1.0),
    )
    .unwrap();
    let r = b
        .region(
            seq,
            drums,
            0,
            media,
            0,
            48_000,
            -6.0,
            Fade {
                length_frames: 480,
                shape: FadeShape::EqualPower,
            },
            Fade::default(),
        )
        .unwrap();
    b.run(Command::Audio(AudioCmd::SetName {
        event: r,
        name: Some("Hit".into()),
    }))
    .unwrap();
    // A second region inside the first, not crossfading.
    plain_region(&mut b, seq, drums, Q, media);
    // Automation of a send the strip lacks, and of a strip no track plays.
    let spare = output(&mut b, "Spare");
    let spare = b.strip_of(StripKind::AudioPath(spare)).unwrap();
    b.events(
        seq,
        drums,
        vec![
            spec(
                0,
                automation(AutomationTarget::SendLevel(drum_strip, SendIndex(3)), 0.5),
            ),
            spec(0, automation(AutomationTarget::StripFader(spare), 0.5)),
        ],
    )
    .unwrap();
    // A note past the end of the sequence.
    length(&mut b, seq, 4 * BAR);
    b.notes(seq, keys, &[(5 * BAR, 60, 0x8000, Q)]).unwrap();
    (b.finish(), seq)
}

#[test]
fn what_is_left_out_on_the_timeline() {
    let (doc, seq) = listed_fixture();
    let x = run_opts(&doc, opts(seq, GAP, RegionGain::LeaveOut));
    let r = &x.report;
    for what in [
        MUTED_NOTES,
        SYSEX,
        TEXTS,
        NOTE_ATTRIBUTES,
        OTHER_PER_NOTE,
        SOLO_AUTOMATION,
        VELOCITY_SCALE_AUTOMATION,
        REGION_GAIN,
        EVENTS_OUTSIDE,
        UNPLACED_AUTOMATION,
    ] {
        assert_eq!(
            count(r, ItemKind::LeftOut, what),
            1,
            "{what}: {:#?}",
            r.items
        );
    }
    assert_eq!(item(r, ItemKind::LeftOut, UNPLACED_AUTOMATION).count, 2);
    for what in [FADE_SHAPES, MIDI_GROUPS, NESTED_AUDIO] {
        assert_eq!(
            count(r, ItemKind::Changed, what),
            1,
            "{what}: {:#?}",
            r.items
        );
    }
    for what in [OVERLAPS, TRANSPOSED] {
        assert_eq!(count(r, ItemKind::Note, what), 1, "{what}: {:#?}", r.items);
    }
    assert_eq!(item(r, ItemKind::LeftOut, MUTED_NOTES).count, 1);
    assert_eq!(count(r, ItemKind::Changed, CROSSFADES), 0);

    // The golden's song has none of them.
    let f = timeline_fixture();
    let r = run(&f.doc, f.seq).report;
    for what in [
        MUTED_NOTES,
        SYSEX,
        TEXTS,
        NOTE_ATTRIBUTES,
        OTHER_PER_NOTE,
        SOLO_AUTOMATION,
        VELOCITY_SCALE_AUTOMATION,
        REGION_GAIN,
        EVENTS_OUTSIDE,
        FADE_SHAPES,
        MIDI_GROUPS,
        NESTED_AUDIO,
        OVERLAPS,
        TRANSPOSED,
        UNPLACED_AUTOMATION,
        NESTED_MUTED,
    ] {
        assert!(
            r.items.iter().all(|i| i.what != what),
            "{what}: {:#?}",
            r.items
        );
    }
}

#[test]
fn region_gain_entries() {
    let f = timeline_fixture();
    let media = f
        .doc
        .media()
        .keys()
        .next()
        .copied()
        .expect("one media item");
    let x = run_opts(&f.doc, opts(f.seq, GAP, RegionGain::Apply));
    assert_eq!(
        x.audio,
        vec![
            AudioFile {
                entry: "audio/001.wav".into(),
                media,
                gain_db: 0.0,
                name: "a".into(),
            },
            AudioFile {
                entry: "audio/002.wav".into(),
                media,
                gain_db: -6.0,
                name: "a".into(),
            },
        ]
    );
    assert_eq!(count(&x.report, ItemKind::LeftOut, REGION_GAIN), 0);

    let x = run_opts(&f.doc, opts(f.seq, GAP, RegionGain::LeaveOut));
    assert_eq!(
        x.audio,
        vec![AudioFile {
            entry: "audio/001.wav".into(),
            media,
            gain_db: 0.0,
            name: "a".into(),
        }]
    );
    let it = item(&x.report, ItemKind::LeftOut, REGION_GAIN);
    assert_eq!(it.details, vec!["Hit".to_string()]);
    let p = read_back(&x);
    let files: Vec<String> = clips(&p, &format!("t{}", f.drums.0))
        .iter()
        .map(|c| match c.content.as_deref() {
            Some(DpTimeline::Audio(a)) => a.file.path.clone(),
            other => panic!("audio content, not {other:?}"),
        })
        .collect();
    assert_eq!(files, vec!["audio/001.wav", "audio/001.wav"]);
}

/// S9 (plan 6d Task 9): media the caller leaves out gets no entry, one
/// Left out line with its name, and clips with no content, so
/// `project.xml` names no file the package lacks and still passes the
/// schema.
#[test]
fn left_out_media_is_written_without_audio() {
    let f = timeline_fixture();
    let media = f
        .doc
        .media()
        .keys()
        .next()
        .copied()
        .expect("one media item");
    let with_audio = run(&f.doc, f.seq);
    assert!(with_audio.project_xml.contains("audio/001.wav"));
    let regions = clips(&read_back(&with_audio), &format!("t{}", f.drums.0)).len();
    for gain in [RegionGain::Apply, RegionGain::LeaveOut] {
        let mut o = opts(f.seq, GAP, gain);
        o.left_out.insert(media, "Gone audio".to_owned());
        let x = run_opts(&f.doc, o);
        assert!(x.audio.is_empty(), "{:?}", x.audio);
        assert_eq!(x.report.summary.audio_files, 0);
        let it = item(&x.report, ItemKind::LeftOut, "Gone audio");
        assert_eq!((it.count, it.details.clone()), (1, vec!["a".to_string()]));
        assert_eq!(
            count(&x.report, ItemKind::LeftOut, REGION_GAIN),
            0,
            "no gain to leave out without the audio"
        );
        assert!(!x.project_xml.contains("audio/"), "{}", x.project_xml);
        assert!(!x.project_xml.contains("<Audio"), "{}", x.project_xml);
        validates(&x);
        let p = read_back(&x);
        let cs = clips(&p, &format!("t{}", f.drums.0));
        assert_eq!(cs.len(), regions, "every region is still a clip");
        for c in cs {
            assert!(c.content.is_none(), "{c:?}");
            assert_eq!((c.play_start, c.play_stop), (None, None));
        }
        // The dry run lists it too.
        let mut o = opts(f.seq, GAP, gain);
        o.left_out.insert(media, "Gone audio".to_owned());
        let dry = check(&f.doc, &o, &NoPlist, &AtomicBool::new(false)).unwrap();
        assert_eq!(dry.report, x.report);
        assert_eq!(dry.slots, x.slots);
    }
}

#[test]
fn crossfade_writes_matching_fades() {
    let f = timeline_fixture();
    let x = run(&f.doc, f.seq);
    let p = read_back(&x);
    let c = clips(&p, &format!("t{}", f.drums.0));
    assert_eq!(c.len(), 2);
    let (a, b) = (&c[0], &c[1]);
    assert_eq!(a.named.comment.as_deref(), Some("vz:e8"));
    assert_eq!(a.fade_time_unit, Some(DpTimeUnit::Seconds));
    assert_eq!(a.fade_in, Some(0.01));
    assert_eq!(a.fade_out, Some(0.5), "24 000 frames of overlap");
    assert_eq!(b.fade_in, Some(0.1));
    assert_eq!(b.fade_out, None);
    assert_eq!((a.time, b.time), (0.0, 1.0));
    assert!(b.time < a.time + a.duration.unwrap_or(0.0), "they overlap");
    let it = item(&x.report, ItemKind::Changed, CROSSFADES);
    assert_eq!(
        it.what,
        "Crossfades were written as overlapping clips; the earlier clip's fade-out covers the overlap"
    );
    assert_eq!(it.count, 1);
}

/// "Keys" on an external destination (channel 3) with a default patch and
/// a MIDI mix, playing one note.
fn patch_fixture() -> (Document, SeqId, LaneId) {
    let mut b = Builder::new();
    let ext: DestId = b
        .dest_kind(
            "Ext",
            DestKind::MidiEndpoint {
                endpoint_ref: "hw:ext".into(),
                group: 0,
                channel: 3,
            },
            Some(Patch {
                program: 10,
                bank: None,
            }),
        )
        .unwrap();
    b.run(Command::Routing(RoutingCmd::SetMidiMix {
        dest: ext,
        mix: MidiMix {
            volume: Some(100),
            pan: Some(64),
            mute: false,
        },
    }))
    .unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(ext)).unwrap();
    b.notes(seq, keys, &[(0, 60, 0x8000, Q)]).unwrap();
    (b.finish(), seq, keys)
}

#[test]
fn default_patch_and_midi_mix_at_time_zero() {
    let (doc, seq, keys) = patch_fixture();
    let p = read_back(&run(&doc, seq));
    let pts = track_points(&p, &format!("t{}", keys.0));
    let find = |e: DpExpression, controller: Option<i32>| {
        pts.iter()
            .find(|p| p.target.expression == Some(e) && p.target.controller == controller)
            .unwrap_or_else(|| panic!("no {e:?} {controller:?} in {pts:#?}"))
    };
    let prog = find(DpExpression::ProgramChange, None);
    assert_eq!(prog.target.channel, Some(3));
    assert_eq!(
        prog.points,
        vec![DpPoint {
            time: 0.0,
            value: DpPointValue::Integer(10),
        }]
    );
    for (cc, v) in [(7, 100.0 / 127.0), (10, 64.0 / 127.0)] {
        let x = find(DpExpression::ChannelController, Some(cc));
        assert_eq!(x.target.channel, Some(3));
        assert_eq!(x.unit, Some(DpUnit::Normalized));
        assert_eq!(
            x.points,
            vec![DpPoint {
                time: 0.0,
                value: DpPointValue::Real(v, Some(DpInterpolation::Hold)),
            }]
        );
    }
    // No bank: no CC 0 or 32.
    assert!(
        pts.iter()
            .all(|p| !matches!(p.target.controller, Some(0 | 32)))
    );
}

/// A song of `notes` short notes on one track.
fn big_song(notes: i64) -> (Document, SeqId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", BUILTIN_SYNTH_REF, 0).unwrap();
    let (seq, lane) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    let list: Vec<(i64, u8, u16, i64)> = (0..notes)
        .map(|i| (i * Q / 8, 60 + (i % 12) as u8, 0x8000, Q / 16))
        .collect();
    b.notes(seq, lane, &list).unwrap();
    (b.finish(), seq)
}

#[test]
fn timeline_export_stops_when_cancelled() {
    // The layout looks at each of the 200 000 events once; the flag goes up
    // 50 000 events into the timeline, which must see it within one
    // interval. The dry run writes no projection, so only the timeline's
    // own checks can stop it.
    let n = 200_000;
    let (doc, seq) = big_song(n);
    let o = opts(seq, GAP, RegionGain::Apply);
    let flag = AtomicBool::new(false);
    debug_cancel_after(Some(n as usize + 50_000));
    let got = check(&doc, &o, &NoPlist, &flag);
    let after = debug_items_after_cancel();
    debug_cancel_after(None);
    assert!(flag.load(Ordering::Relaxed), "the hook raised the flag");
    assert_eq!(got, Err(IoError::Cancelled));
    assert!(after <= CANCEL_EVERY, "{after} events after the flag");
}

/// "Keys" (notes on the built-in synth, audio on "Keys out") using "Inner"
/// at bar 1, muted when `muted`. "Inner" holds a note, a region on "Keys
/// out", a fader point for "Keys out" and a lyric; with `inner_muted` also a
/// muted region.
fn muted_use_fixture(muted: bool, inner_muted: bool) -> (Document, SeqId, LaneId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", BUILTIN_SYNTH_REF, 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    let out = output(&mut b, "Keys out");
    route(&mut b, seq, keys, out);
    let strip = b.strip_of(StripKind::AudioPath(out)).unwrap();
    let (inner, inner_lane) = b.sequence("Inner", "Inner", Some(synth)).unwrap();
    b.notes(inner, inner_lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let inner_audio = b.lane(inner, "Inner audio", None).unwrap();
    route(&mut b, inner, inner_audio, out);
    let media = b.media("Media/a.caf", 48_000, 2).unwrap();
    plain_region(&mut b, inner, inner_audio, 0, media);
    if inner_muted {
        let r = plain_region(&mut b, inner, inner_audio, 2 * Q, media);
        mute(&mut b, r);
    }
    b.events(
        inner,
        inner_lane,
        vec![
            spec(0, automation(AutomationTarget::StripFader(strip), 0.5)),
            spec(
                0,
                EventBody::Text {
                    kind: TextKind::Lyric,
                    text: "la".into(),
                },
            ),
        ],
    )
    .unwrap();
    let u = b.reference(seq, keys, 0, inner, 1, 0).unwrap();
    if muted {
        mute(&mut b, u);
    }
    (b.finish(), seq, keys)
}

#[test]
fn a_muted_use_stays_silent() {
    let (doc, seq, keys) = muted_use_fixture(true, false);
    let x = run(&doc, seq);
    validates(&x);
    let p = read_back(&x);
    let (notes, audio) = (format!("t{}", keys.0), format!("t{}a", keys.0));
    let c = clips(&p, &notes);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].enable, Some(false), "the use's MIDI");
    let c = clips(&p, &audio);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].enable, Some(false), "the region inside the muted use");
    assert!(track_points(&p, &notes).is_empty(), "no automation from it");
    assert!(track_points(&p, &audio).is_empty(), "no automation from it");
    for what in [TEXTS, MUTED_NOTES] {
        assert_eq!(count(&x.report, ItemKind::LeftOut, what), 0, "{what}");
    }
    // The record's projection is the document's: nothing of the muted use
    // plays.
    let snap: Snapshot = serde_json::from_str(&x.snapshot_json).unwrap();
    let want = projection(&doc, seq).unwrap();
    assert_eq!(snap.projection, want);
    assert!(want.tracks.iter().all(|t| t.regions.is_empty()));
    assert!(want.tracks.iter().all(|t| t.automation.is_empty()));
    assert!(want.tracks.iter().all(|t| t.notes_played.is_empty()));

    // Unmuted, the same use plays all of it.
    let (doc, seq, keys) = muted_use_fixture(false, false);
    let x = run(&doc, seq);
    let p = read_back(&x);
    assert_eq!(clips(&p, &format!("t{}a", keys.0))[0].enable, None);
    assert_eq!(track_points(&p, &format!("t{}", keys.0)).len(), 1);
    assert_eq!(count(&x.report, ItemKind::LeftOut, TEXTS), 1);
}

#[test]
fn muted_parts_inside_a_use_are_noted() {
    let (doc, seq, keys) = muted_use_fixture(false, true);
    let x = run(&doc, seq);
    let it = item(&x.report, ItemKind::Note, NESTED_MUTED);
    assert_eq!(it.count, 1);
    assert_eq!(it.details, vec!["Inner".to_string()]);
    // The muted region does not play and is not written.
    let p = read_back(&x);
    assert_eq!(clips(&p, &format!("t{}a", keys.0)).len(), 1);
}

#[test]
fn a_muted_region_is_disabled_and_overlaps_nothing() {
    let mut b = Builder::new();
    let (seq, drums) = b.sequence("Song", "Drums", None).unwrap();
    let out = output(&mut b, "Drums");
    route(&mut b, seq, drums, out);
    let media = b.media("Media/a.caf", 48_000, 2).unwrap();
    let a = plain_region(&mut b, seq, drums, 0, media);
    mute(&mut b, a);
    plain_region(&mut b, seq, drums, Q, media);
    let doc = b.finish();
    let x = run(&doc, seq);
    validates(&x);
    let p = read_back(&x);
    let c = clips(&p, &format!("t{}", drums.0));
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].enable, Some(false));
    assert_eq!(c[1].enable, None);
    assert_eq!(
        count(&x.report, ItemKind::Note, OVERLAPS),
        0,
        "{:#?}",
        x.report.items
    );
}

#[test]
fn parameter_and_send_automation_targets_resolve() {
    let mut b = Builder::new();
    let (seq, drums) = b.sequence("Song", "Drums", None).unwrap();
    let out = output(&mut b, "Drums");
    route(&mut b, seq, drums, out);
    let strip = b.strip_of(StripKind::AudioPath(out)).unwrap();
    let verb = b.bus("Verb").unwrap();
    b.mixer(MixerCmd::AddSend {
        strip,
        send: AuxSend::new(verb),
    })
    .unwrap();
    let slot = b
        .insert(
            strip,
            PluginRef {
                format: PluginFormat::Clap,
                id: "org.example.delay".into(),
                name: "Delay".into(),
                vendor: String::new(),
                version: String::new(),
            },
        )
        .unwrap();
    b.events(
        seq,
        drums,
        vec![
            spec(
                0,
                automation(AutomationTarget::SendLevel(strip, SendIndex(0)), 0.5),
            ),
            spec(
                Q,
                automation(AutomationTarget::PluginParam(slot, ParamId(5)), 0.25),
            ),
            spec(Q, automation(AutomationTarget::StripPan(strip), 0.75)),
        ],
    )
    .unwrap();
    let doc = b.finish();
    let x = run(&doc, seq);
    // `validates` runs the ID check: every `Points@parameter` resolves.
    validates(&x);
    let p = read_back(&x);
    let params: Vec<(Option<String>, Option<DpUnit>)> = track_points(&p, &format!("t{}", drums.0))
        .iter()
        .map(|p| (p.target.parameter.clone(), p.unit))
        .collect();
    let s = strip.0;
    assert_eq!(
        params,
        vec![
            (Some(format!("c{s}s0v")), Some(DpUnit::Linear)),
            (Some(format!("c{s}d{}q5", slot.0)), Some(DpUnit::Normalized)),
            (Some(format!("c{s}p")), Some(DpUnit::Normalized)),
        ]
    );
}

#[test]
fn pressure_at_its_notes_start_is_the_notes() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", BUILTIN_SYNTH_REF, 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    // The pressure is made first, so it plays before the note at tick 0.
    b.event(
        seq,
        keys,
        0,
        EventBody::PolyPressure {
            pitch: 60,
            value: 0x4000_0000,
        },
    )
    .unwrap();
    b.notes(seq, keys, &[(0, 60, 0x8000, Q)]).unwrap();
    let doc = b.finish();
    let p = read_back(&run(&doc, seq));
    let c = clips(&p, &format!("t{}", keys.0));
    assert_eq!(c.len(), 1);
    let pts = points_of(&c[0]);
    assert!(
        pts.iter()
            .any(|x| x.target.expression == Some(DpExpression::Pressure)),
        "{:#?}",
        c[0]
    );
    assert!(
        pts.iter()
            .all(|x| x.target.expression != Some(DpExpression::PolyPressure)),
        "{:#?}",
        c[0]
    );
}

#[test]
fn a_last_point_is_inside_its_clip() {
    // Found by the round-trip property (plan 6d Task 8): a clip is
    // `[time, time + duration)`, so a controller point on the clip's last
    // tick was cut off by the import. The clip reaches one tick past it.
    let mut b = Builder::new();
    let synth = b.dest("Synth", BUILTIN_SYNTH_REF, 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    b.notes(seq, keys, &[(0, 60, 0x8000, Q)]).unwrap();
    b.event(
        seq,
        keys,
        2 * Q,
        EventBody::Cc {
            index: 1,
            value: 0x4000_0000,
        },
    )
    .unwrap();
    let doc = b.finish();
    let p = read_back(&run(&doc, seq));
    let c = rule_clips(&p, &format!("t{}", keys.0));
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].duration, Some(2.0 + 1.0 / 15_360.0));
    // A clip that a silence gap ends, its last event a point, is no
    // different: the next clip starts after the gap.
    let mut b = Builder::new();
    let synth = b.dest("Synth", BUILTIN_SYNTH_REF, 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    b.notes(seq, keys, &[(0, 60, 0x8000, Q), (6 * Q, 62, 0x8000, Q)])
        .unwrap();
    b.event(
        seq,
        keys,
        2 * Q,
        EventBody::Cc {
            index: 1,
            value: 0x4000_0000,
        },
    )
    .unwrap();
    let doc = b.finish();
    let p = read_back(&run(&doc, seq));
    let c = rule_clips(&p, &format!("t{}", keys.0));
    assert_eq!(
        c.iter().map(|c| (c.time, c.duration)).collect::<Vec<_>>(),
        vec![(0.0, Some(2.0 + 1.0 / 15_360.0)), (6.0, Some(1.0))]
    );
}

#[test]
fn a_use_cut_by_the_song_end_ends_there() {
    // A use's clip ends where the song does when the song cuts it: its
    // points past the end do not play and are not written, and the clip is
    // not made a tick longer for them.
    let points = |c: &DpClip| {
        let mut all = Vec::new();
        if let Some(t) = c.content.as_deref() {
            walk(t, &mut all);
        }
        all.iter()
            .map(|t| match t {
                DpTimeline::Points(p) => p.points.len(),
                _ => 0,
            })
            .sum::<usize>()
    };
    for (song_len, want_points) in [(BAR + 2 * Q, 1), (BAR + 2 * Q + 1, 2)] {
        let mut b = Builder::new();
        let synth = b.dest("Synth", BUILTIN_SYNTH_REF, 0).unwrap();
        let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
        let (riff, rl) = b.sequence("Riff", "Riff", Some(synth)).unwrap();
        b.notes(riff, rl, &[(0, 60, 0x8000, Q)]).unwrap();
        b.event(
            riff,
            rl,
            2 * Q,
            EventBody::Cc {
                index: 1,
                value: 0x4000_0000,
            },
        )
        .unwrap();
        length(&mut b, riff, BAR);
        b.reference(seq, keys, 0, riff, 2, 0).unwrap();
        length(&mut b, seq, song_len);
        let doc = b.finish();
        let p = read_back(&run(&doc, seq));
        let c = clips(&p, &format!("t{}", keys.0));
        assert_eq!(c.len(), 1, "{song_len}");
        assert_eq!(c[0].named.name.as_deref(), Some("Riff"));
        assert_eq!(c[0].loop_end, None, "cut, so not a loop");
        assert_eq!(
            c[0].duration,
            Some(song_len as f64 / Q as f64),
            "the song's end, {song_len}"
        );
        assert_eq!(points(&c[0]), want_points, "{song_len}");
    }
}
