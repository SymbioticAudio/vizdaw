//! MIDI import: file → tracks → the document (plan 6a Task 5).

use core_engine::input_fx::{up7, up32};
use core_engine::midi1::Midi1Msg;
use core_engine::mpe::bend_in;
use core_io::IoError;
use core_io::clip::{self, ClipEvent, ClipFile};
use core_io::gm::drum_name;
use core_io::limits::{Limits, MAX_MIDI_FILE_BYTES};
use core_io::midi_in::{
    DestChoice, ImportOptions, MidiFileKind, MidiImport, Placement, apply, channels, parse,
    parse_file, suggested_dest,
};
use core_io::report::{ItemKind, Report};
use core_io::smf::{self, Division, SmfEvent, SmfFile, SmfTrack};
use core_io::ump::{self, NoteAttribute as UmpAttribute};
use core_model::command::event::note_name;
use core_model::command::{Command, EventCmd, LaneCmd, RoutingCmd, SequenceCmd, apply as run};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec, NoteAttribute, PerNoteCtl, PerNoteKind, TextKind};
use core_model::history::History;
use core_model::ids::{AnyId, DestId, LaneId, SeqId};
use core_model::meter::{KeySig, Meter, MeterEntry};
use core_model::mixer::BUILTIN_SYNTH_REF;
use core_model::mpe::{MpeConfig, MpeZone};
use core_model::rules::{MAX_TICKS, check_info};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

// ---- building files ----

fn ch3(status: u8, d1: u8, d2: u8) -> SmfEvent {
    SmfEvent::Channel(Midi1Msg::three(status, d1, d2))
}

fn ch2(status: u8, d1: u8) -> SmfEvent {
    SmfEvent::Channel(Midi1Msg::two(status, d1))
}

fn on(c: u8, pitch: u8, vel: u8) -> SmfEvent {
    ch3(0x90 | c, pitch, vel)
}

fn off(c: u8, pitch: u8, vel: u8) -> SmfEvent {
    ch3(0x80 | c, pitch, vel)
}

fn cc(c: u8, index: u8, v: u8) -> SmfEvent {
    ch3(0xB0 | c, index, v)
}

fn meta(kind: u8, data: &[u8]) -> SmfEvent {
    SmfEvent::Meta {
        kind,
        data: data.to_vec(),
    }
}

fn text(kind: u8, s: &str) -> SmfEvent {
    meta(kind, s.as_bytes())
}

/// An SMF; each track gets an End of Track at its last tick.
fn smf_of(format: u16, division: Division, tracks: Vec<Vec<(u64, SmfEvent)>>) -> Vec<u8> {
    let tracks = tracks
        .into_iter()
        .map(|mut events| {
            let end = events.last().map_or(0, |(t, _)| *t);
            events.push((end, meta(0x2F, &[])));
            SmfTrack { events }
        })
        .collect();
    smf::write(&SmfFile {
        format,
        division,
        tracks,
    })
    .unwrap()
}

fn type1(ppq: u16, tracks: Vec<Vec<(u64, SmfEvent)>>) -> Vec<u8> {
    smf_of(1, Division::Ppq(ppq), tracks)
}

fn type0(ppq: u16, events: Vec<(u64, SmfEvent)>) -> Vec<u8> {
    smf_of(0, Division::Ppq(ppq), vec![events])
}

fn clip_of(tpq: u16, header: Vec<ClipEvent>, events: Vec<(u64, ClipEvent)>, end: u64) -> Vec<u8> {
    clip::write(&ClipFile {
        tpq,
        header,
        events,
        end,
    })
}

fn pk(p: ump::Packet) -> ClipEvent {
    ClipEvent::Packet(p)
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

fn import_with(bytes: &[u8], opts: ImportOptions) -> MidiImport {
    parse(bytes, &opts, &Limits::default(), &no_cancel()).unwrap()
}

fn import(bytes: &[u8]) -> MidiImport {
    import_with(bytes, ImportOptions::default())
}

// ---- looking at results ----

/// The count of the item of `kind` whose text contains `needle`.
#[track_caller]
fn has(r: &Report, kind: ItemKind, needle: &str) -> u64 {
    match r
        .items
        .iter()
        .find(|i| i.kind == kind && i.what.contains(needle))
    {
        Some(i) => i.count,
        None => panic!("no {kind:?} item containing {needle:?} in {:?}", r.items),
    }
}

#[track_caller]
fn lacks(r: &Report, needle: &str) {
    assert!(
        !r.items.iter().any(|i| i.what.contains(needle)),
        "unexpected item containing {needle:?} in {:?}",
        r.items
    );
}

/// `(tick, pitch, duration)` of every note.
fn notes(events: &[EventSpec]) -> Vec<(i64, u8, i64)> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Note {
                pitch, duration, ..
            } => Some((e.tick.0, *pitch, duration.0)),
            _ => None,
        })
        .collect()
}

fn note_with(
    pitch: u8,
    velocity: u16,
    release: u16,
    duration: i64,
    attribute: Option<NoteAttribute>,
    per_note: Vec<PerNoteCtl>,
) -> EventBody {
    EventBody::Note {
        pitch,
        velocity,
        release_velocity: release,
        duration: Tick(duration),
        attribute,
        per_note: per_note.into_iter().collect(),
    }
}

fn spec(tick: i64, gc: Option<(u8, u8)>, body: EventBody) -> EventSpec {
    EventSpec {
        tick: Tick(tick),
        dest: None,
        group_channel: gc,
        mute: false,
        body,
    }
}

// ---- documents ----

fn created_seq(created: &[AnyId]) -> SeqId {
    created
        .iter()
        .find_map(|id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        })
        .unwrap()
}

fn created_dest(created: &[AnyId]) -> DestId {
    created
        .iter()
        .find_map(|id| match id {
            AnyId::Dest(d) => Some(*d),
            _ => None,
        })
        .unwrap()
}

/// A document with one sequence "Main" holding one empty track.
fn doc_with_track() -> (Document, SeqId, LaneId) {
    let a = run(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New {
            name: "Main".into(),
        }),
    )
    .unwrap();
    let seq = created_seq(&a.report.created);
    let lane = a.doc.sequence(seq).unwrap().lanes.front().unwrap().id;
    (a.doc, seq, lane)
}

/// A document with one sequence and no tracks.
fn empty_doc() -> (Document, SeqId) {
    let (d, seq, lane) = doc_with_track();
    let d = run(&d, &Command::Lane(LaneCmd::Remove { seq, lane }))
        .unwrap()
        .doc;
    (d, seq)
}

fn add_dest(
    d: &Document,
    name: &str,
    endpoint: &str,
    group: u8,
    channel: u8,
) -> (Document, DestId) {
    let a = run(
        d,
        &Command::Routing(RoutingCmd::AddDestination {
            name: name.into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: endpoint.into(),
                group,
                channel,
            },
            default_patch: None,
        }),
    )
    .unwrap();
    let id = created_dest(&a.report.created);
    (a.doc, id)
}

fn place(seq: SeqId) -> Placement {
    Placement {
        seq,
        lane: None,
        tick: Tick(0),
        channel_map: BTreeMap::new(),
        use_tempo: false,
        new_project: false,
    }
}

/// Applies inside one transaction; returns the history, lanes and report.
fn apply_to(doc: Document, imp: &MidiImport, p: &Placement) -> (History, Vec<LaneId>, Report) {
    let mut h = History::new(doc);
    let mut report = Report::default();
    h.begin("Import MIDI File").unwrap();
    let lanes = apply(&mut h, imp, p, &mut report).unwrap();
    // `apply` merges the import's own report.
    for i in &imp.report.items {
        assert!(
            report
                .items
                .iter()
                .any(|r| r.kind == i.kind && r.what == i.what)
        );
    }
    h.commit().unwrap();
    (h, lanes, report)
}

fn lane_events(h: &History, seq: SeqId, lane: LaneId) -> Vec<EventSpec> {
    let l = h.doc().lane(seq, lane).unwrap();
    l.active()
        .unwrap()
        .events
        .values()
        .map(|e| e.spec())
        .collect()
}

// ---- tests ----

#[test]
fn positions_scale_exactly_or_round() {
    for ppq in [960u16, 480] {
        let f = type1(
            ppq,
            vec![vec![
                (u64::from(ppq), on(0, 60, 100)),
                (u64::from(ppq) * 2, off(0, 60, 0)),
            ]],
        );
        let imp = import(&f);
        assert_eq!(
            notes(&imp.tracks[0].events),
            vec![(15_360, 60, 15_360)],
            "ppq {ppq}"
        );
        lacks(&imp.report, "Positions rounded");
    }

    let note = |t: u64, len: u64| {
        vec![
            (t, pk(ump::note_on(0, 0, 60, 0x8000, None))),
            (t + len, pk(ump::note_off(0, 0, 60, 0, None))),
        ]
    };
    let imp = import(&clip_of(480, vec![], note(480, 480), 960));
    assert_eq!(imp.kind, MidiFileKind::Clip);
    assert_eq!(notes(&imp.tracks[0].events), vec![(15_360, 60, 15_360)]);
    lacks(&imp.report, "Positions rounded");
    let imp = import(&clip_of(7, vec![], note(1, 6), 7));
    assert_eq!(notes(&imp.tracks[0].events), vec![(2_194, 60, 13_166)]);
    assert!(has(&imp.report, ItemKind::Changed, "Positions rounded") >= 1);

    // 1000 is not a divisor of 15 360: exact positions report nothing.
    let imp = import(&type1(
        1000,
        vec![vec![(1000, on(0, 60, 100)), (2000, off(0, 60, 0))]],
    ));
    assert_eq!(notes(&imp.tracks[0].events), vec![(15_360, 60, 15_360)]);
    lacks(&imp.report, "Positions rounded");
    // 3 → 46.08 → 46; 7 → 107.52 → 108.
    let imp = import(&type1(
        1000,
        vec![vec![(3, on(0, 60, 100)), (7, off(0, 60, 0))]],
    ));
    assert_eq!(notes(&imp.tracks[0].events), vec![(46, 60, 62)]);
    has(
        &imp.report,
        ItemKind::Changed,
        "Positions rounded to VizDAW's resolution",
    );

    // 25 fps × 40 ticks: one second is two quarters at 120 BPM.
    let f = smf_of(
        1,
        Division::Smpte {
            fps: 25,
            ticks_per_frame: 40,
        },
        vec![vec![(1000, on(0, 60, 100)), (2000, off(0, 60, 0))]],
    );
    let imp = import(&f);
    assert_eq!(notes(&imp.tracks[0].events), vec![(30_720, 60, 30_720)]);
    has(
        &imp.report,
        ItemKind::Changed,
        "Timecode-based file placed at 120 BPM",
    );
}

#[test]
fn overlapping_notes_pair_first_on_first_off() {
    let f = type1(
        960,
        vec![vec![
            (0, on(0, 60, 100)),
            (0, on(0, 62, 90)),
            (0, on(0, 64, 80)),
            (100, off(0, 65, 0)),
            (480, on(0, 62, 0)),
            (960, on(0, 60, 101)),
            (1920, off(0, 60, 64)),
            (2880, off(0, 60, 0)),
            (3840, cc(0, 1, 0)),
        ]],
    );
    let imp = import(&f);
    let t = &imp.tracks[0].events;
    assert_eq!(
        notes(t),
        vec![
            (0, 60, 30_720),
            (0, 62, 7_680),
            (0, 64, 61_440),
            (15_360, 60, 30_720),
        ]
    );
    // The first off (velocity 64) ends the first on.
    match &t[0].body {
        EventBody::Note {
            velocity,
            release_velocity,
            ..
        } => {
            assert_eq!(*velocity, up7(100));
            assert_eq!(*release_velocity, up7(64));
        }
        b => panic!("{b:?}"),
    }
    // A note-on of velocity 0 makes no note of velocity 0.
    assert!(
        t.iter()
            .all(|e| !matches!(e.body, EventBody::Note { velocity: 0, .. }))
    );
    assert_eq!(
        has(
            &imp.report,
            ItemKind::Changed,
            "Notes with no end were ended at the end of their track"
        ),
        1
    );
    assert_eq!(
        has(&imp.report, ItemKind::LeftOut, "Note-offs with no note"),
        1
    );
}

#[test]
fn values_scale_by_midi2_rules() {
    let f = type1(
        960,
        vec![vec![
            (0, on(0, 60, 100)),
            (0, cc(0, 7, 90)),
            (0, ch3(0xE0, 0x00, 0x40)),
            (0, ch2(0xD0, 50)),
            (0, ch3(0xA0, 60, 70)),
            (960, cc(0, 0, 1)),
            (960, cc(0, 32, 2)),
            (960, ch2(0xC0, 5)),
            (1920, cc(0, 101, 0)),
            (1920, cc(0, 100, 0)),
            (1920, cc(0, 6, 2)),
            (2880, ch2(0xC0, 9)),
            (3840, off(0, 60, 0)),
        ]],
    );
    let imp = import(&f);
    let gc = Some((0, 0));
    let expected = vec![
        spec(0, gc, note_with(60, up7(100), 0, 61_440, None, vec![])),
        spec(
            0,
            gc,
            EventBody::Cc {
                index: 7,
                value: up32(90, 7),
            },
        ),
        spec(0, gc, EventBody::PitchBend { value: 0x8000_0000 }),
        spec(0, gc, EventBody::ChannelPressure { value: up32(50, 7) }),
        spec(
            0,
            gc,
            EventBody::PolyPressure {
                pitch: 60,
                value: up32(70, 7),
            },
        ),
        spec(
            15_360,
            gc,
            EventBody::Program {
                program: 5,
                bank: Some((1, 2)),
            },
        ),
        spec(
            30_720,
            gc,
            EventBody::Cc {
                index: 101,
                value: 0,
            },
        ),
        spec(
            30_720,
            gc,
            EventBody::Cc {
                index: 100,
                value: 0,
            },
        ),
        spec(
            30_720,
            gc,
            EventBody::Cc {
                index: 6,
                value: up32(2, 7),
            },
        ),
        spec(
            46_080,
            gc,
            EventBody::Program {
                program: 9,
                bank: None,
            },
        ),
    ];
    assert_eq!(imp.tracks[0].events, expected);
}

#[test]
fn meta_events_map() {
    let conductor = vec![
        (0, meta(0x51, &[0x07, 0xA1, 0x20])),
        (0, meta(0x58, &[3, 2, 24, 8])),
        (0, meta(0x59, &[2, 0])),
        (0, text(0x03, "Song")),
        (0, text(0x02, "(c) Someone")),
        (100, meta(0x51, &[0, 0, 0])),
        (100, meta(0x58, &[4, 100, 24, 8])),
        (200, meta(0x51, &[0x07, 0xA1])),
        (300, meta(0x51, &[0x00, 0xEA, 0x60])),
        (400, meta(0x51, &[0xE4, 0xE1, 0xC0])),
        (3000, meta(0x58, &[4, 2, 24, 8])),
    ];
    let track = vec![
        (0, meta(0x21, &[3])),
        (0, text(0x03, "  Bass  ")),
        (0, text(0x01, "hello")),
        (0, text(0x02, "mine")),
        (0, meta(0x54, &[0, 0, 0, 0, 0])),
        (0, meta(0x7F, &[0, 0, 0x41])),
        (0, on(0, 40, 100)),
        (960, text(0x05, "la")),
        (960, text(0x06, "Verse")),
        (960, text(0x07, "go")),
        (1920, off(0, 40, 0)),
    ];
    let imp = import(&type1(960, vec![conductor, track]));
    assert_eq!(imp.kind, MidiFileKind::Smf1);
    assert_eq!(imp.title.as_deref(), Some("Song"));
    assert_eq!(imp.copyright.as_deref(), Some("(c) Someone"));
    assert!(imp.has_tempo);
    let points: Vec<(i64, u32)> = imp
        .tempo
        .points()
        .iter()
        .map(|p| (p.tick.0, p.bpm.0))
        .collect();
    assert_eq!(points, vec![(0, 120_000), (4_800, 999_000), (6_400, 5_000)]);
    assert_eq!(
        has(
            &imp.report,
            ItemKind::Changed,
            "Tempos outside 5 to 999 BPM"
        ),
        2
    );
    assert_eq!(
        has(&imp.report, ItemKind::LeftOut, "Unreadable tempo changes"),
        2
    );
    assert_eq!(
        has(&imp.report, ItemKind::LeftOut, "Unreadable time signatures"),
        1
    );
    // 3/4 at 0 (two sharps); the 4/4 at 3000 moves to the barline at 5760.
    let key = KeySig {
        sharps: 2,
        minor: false,
    };
    let entries: Vec<MeterEntry> = imp.meter.entries().iter().copied().collect();
    assert_eq!(
        entries,
        vec![
            MeterEntry {
                tick: Tick(0),
                meter: Meter { num: 3, den: 4 },
                key
            },
            MeterEntry {
                tick: Tick(92_160),
                meter: Meter { num: 4, den: 4 },
                key
            },
        ]
    );
    has(&imp.report, ItemKind::Changed, "moved to the next barline");
    assert_eq!(imp.start_offset, Tick(0));
    assert_eq!(imp.markers.len(), 1);
    assert_eq!(
        (imp.markers[0].tick, imp.markers[0].name.as_str()),
        (Tick(15_360), "Verse")
    );
    has(&imp.report, ItemKind::LeftOut, "SMPTE offsets");
    has(&imp.report, ItemKind::LeftOut, "Sequencer-specific data");

    assert_eq!(imp.tracks.len(), 1);
    let t = &imp.tracks[0];
    assert_eq!(t.name, "Bass");
    assert_eq!(t.main, Some((3, 0)));
    let txt = |kind: TextKind, s: &str| EventBody::Text {
        kind,
        text: s.into(),
    };
    let expected = vec![
        spec(0, None, txt(TextKind::Text, "hello")),
        spec(0, None, txt(TextKind::Copyright, "mine")),
        spec(
            0,
            Some((3, 0)),
            note_with(40, up7(100), 0, 30_720, None, vec![]),
        ),
        spec(15_360, None, txt(TextKind::Lyric, "la")),
        spec(15_360, None, txt(TextKind::Cue, "go")),
    ];
    assert_eq!(t.events, expected);
}

#[test]
fn key_signatures_and_odd_metas() {
    let imp = import(&type1(
        960,
        vec![vec![
            (0, meta(0x59, &[0xFD, 1])), // 3 flats, minor
            (0, meta(0x59, &[9, 0])),    // unreadable
            (0, meta(0x21, &[20])),      // port past group 16
            (0, on(0, 60, 100)),
            (960, off(0, 60, 0)),
        ]],
    ));
    let e = imp.meter.entries()[0];
    assert_eq!(
        e.key,
        KeySig {
            sharps: -3,
            minor: true
        }
    );
    has(&imp.report, ItemKind::LeftOut, "Unreadable key signatures");
    has(&imp.report, ItemKind::LeftOut, "Port changes past group 16");
    assert_eq!(imp.tracks[0].main, Some((0, 0)));
}

#[test]
fn meta_only_tracks_make_no_track() {
    let conductor = vec![
        (0, meta(0x51, &[0x07, 0xA1, 0x20])),
        (0, meta(0x58, &[4, 2, 24, 8])),
        (0, text(0x03, "Song")),
        (0, text(0x06, "Intro")),
    ];
    let lyrics = vec![(0, text(0x05, "la")), (960, text(0x05, "di"))];
    let notes_track = vec![(0, on(0, 60, 100)), (960, off(0, 60, 0))];
    let imp = import(&type1(960, vec![conductor, lyrics, notes_track]));
    assert_eq!(imp.title.as_deref(), Some("Song"));
    assert_eq!(imp.tracks.len(), 2);
    assert_eq!(imp.tracks[0].main, None);
    assert_eq!(imp.tracks[0].name, "Track 2");
    assert_eq!(imp.tracks[0].events.len(), 2);
    assert_eq!(imp.markers.len(), 1);
}

#[test]
fn type_0_splits_by_channel_and_type_1_does_not() {
    let events = vec![
        (0, text(0x03, "Song")),
        (0, on(0, 60, 100)),
        (0, on(9, 36, 100)),
        (480, off(9, 36, 0)),
        (960, off(0, 60, 0)),
    ];
    let imp = import(&type0(960, events.clone()));
    assert_eq!(imp.kind, MidiFileKind::Smf0);
    assert_eq!(imp.title.as_deref(), Some("Song"));
    assert_eq!(imp.tracks.len(), 2);
    let drums = imp.tracks.iter().find(|t| t.main == Some((0, 9))).unwrap();
    assert!(drums.drum);
    assert_eq!(drums.name, "Drums");
    assert_eq!(notes(&drums.events), vec![(0, 36, 7_680)]);
    let keys = imp.tracks.iter().find(|t| t.main == Some((0, 0))).unwrap();
    assert!(!keys.drum);
    assert_eq!(notes(&keys.events), vec![(0, 60, 15_360)]);
    let one = import_with(
        &type0(960, events),
        ImportOptions {
            split_channels: Some(false),
            mpe: false,
        },
    );
    assert_eq!(one.tracks.len(), 1);

    let track = vec![
        (0, text(0x03, "Keys")),
        (0, on(0, 60, 100)),
        (0, on(1, 64, 100)),
        (480, on(0, 62, 100)),
        (960, off(0, 60, 0)),
        (960, off(0, 62, 0)),
        (960, off(1, 64, 0)),
    ];
    let imp = import(&type1(960, vec![track.clone()]));
    assert_eq!(imp.tracks.len(), 1);
    assert_eq!(imp.tracks[0].main, Some((0, 0)));
    assert_eq!(imp.tracks[0].name, "Keys");
    let split = import_with(
        &type1(960, vec![track]),
        ImportOptions {
            split_channels: Some(true),
            mpe: false,
        },
    );
    assert_eq!(split.tracks.len(), 2);
    let mains: Vec<_> = split.tracks.iter().map(|t| t.main).collect();
    assert_eq!(mains, vec![Some((0, 0)), Some((0, 1))]);
}

/// Member channel 2 (index 1): a note with bend, CC 74 and pressure.
fn mpe_body() -> Vec<(u64, SmfEvent)> {
    vec![
        (0, on(1, 60, 100)),
        (480, ch3(0xE1, 0x00, 0x60)),
        (480, cc(1, 74, 0x40)),
        (600, ch2(0xD1, 0x30)),
        (960, off(1, 60, 0)),
    ]
}

#[test]
fn mpe_is_decoded() {
    let mut events = vec![(0, cc(0, 101, 0)), (0, cc(0, 100, 6)), (0, cc(0, 6, 15))];
    events.extend(mpe_body());
    let decoded = |imp: &MidiImport, cfg: MpeConfig| {
        let t = &imp.tracks[0];
        assert_eq!(t.mpe, Some(cfg));
        let expected = vec![
            spec(
                0,
                Some((0, 0)),
                note_with(
                    60,
                    up7(100),
                    0,
                    15_360,
                    None,
                    vec![
                        PerNoteCtl {
                            kind: PerNoteKind::PitchBend,
                            offset: Tick(7_680),
                            value: bend_in(0x60 << 7, cfg.bend_range),
                        },
                        PerNoteCtl {
                            kind: PerNoteKind::Registered(74),
                            offset: Tick(7_680),
                            value: up32(0x40, 7),
                        },
                    ],
                ),
            ),
            spec(
                9_600,
                Some((0, 0)),
                EventBody::PolyPressure {
                    pitch: 60,
                    value: up32(0x30, 7),
                },
            ),
        ];
        assert_eq!(t.events, expected);
    };
    let lower15 = MpeConfig {
        zone: MpeZone::Lower,
        members: 15,
        bend_range: 48,
    };
    decoded(&import(&type1(960, vec![events])), lower15);

    // The same notes without an MCM are plain MIDI…
    let plain = import(&type1(960, vec![mpe_body()]));
    assert_eq!(plain.tracks[0].mpe, None);
    assert!(
        plain.tracks[0]
            .events
            .iter()
            .any(|e| matches!(e.body, EventBody::PitchBend { .. }))
    );
    // …unless the sheet says MPE.
    let forced = import_with(
        &type1(960, vec![mpe_body()]),
        ImportOptions {
            split_channels: None,
            mpe: true,
        },
    );
    decoded(&forced, MpeConfig::default());
}

#[test]
fn clip_maps_everything_it_carries() {
    let attr = Some(UmpAttribute {
        kind: 3,
        data: 0x0100,
    });
    let events = vec![
        (
            0,
            ClipEvent::Text {
                group: 0,
                bank: ump::BANK_METADATA,
                status: 0x03,
                text: "Lead".into(),
            },
        ),
        (0, pk(ump::note_on(0, 0, 60, 0x1234, attr))),
        (100, pk(ump::per_note_pitch_bend(0, 0, 60, 0x9000_0000))),
        (200, pk(ump::per_note_registered(0, 0, 60, 74, 0x7000_0000))),
        (300, pk(ump::control_change(0, 0, 7, 0xDEAD_BEEF))),
        (400, pk(ump::program_change(0, 0, 5, Some((1, 2))))),
        (
            500,
            ClipEvent::SysEx8 {
                group: 0,
                data: vec![1, 2, 0x80, 0xFF],
            },
        ),
        (1000, pk(ump::note_off(0, 0, 60, 0x4321, None))),
        (1100, pk(ump::midi1(0, 0x91, 62, 100))),
        (1200, pk(ump::midi1(0, 0x81, 62, 0))),
        (1300, pk(ump::per_note_pitch_bend(0, 0, 64, 1))),
    ];
    let imp = import(&clip_of(15_360, vec![], events, 2000));
    assert_eq!(imp.tracks.len(), 1);
    let t = &imp.tracks[0];
    assert_eq!(t.name, "Lead");
    let gc = Some((0, 0));
    let expected = vec![
        spec(
            0,
            gc,
            note_with(
                60,
                0x1234,
                0x4321,
                1000,
                Some(NoteAttribute {
                    kind: 3,
                    data: 0x0100,
                }),
                vec![
                    PerNoteCtl {
                        kind: PerNoteKind::PitchBend,
                        offset: Tick(100),
                        value: 0x9000_0000,
                    },
                    PerNoteCtl {
                        kind: PerNoteKind::Registered(74),
                        offset: Tick(200),
                        value: 0x7000_0000,
                    },
                ],
            ),
        ),
        spec(
            300,
            gc,
            EventBody::Cc {
                index: 7,
                value: 0xDEAD_BEEF,
            },
        ),
        spec(
            400,
            gc,
            EventBody::Program {
                program: 5,
                bank: Some((1, 2)),
            },
        ),
        spec(
            500,
            gc,
            EventBody::SysEx {
                data: Arc::from(vec![1u8, 2, 0x80, 0xFF]),
                eight_bit: true,
            },
        ),
        spec(
            1100,
            Some((0, 1)),
            note_with(62, up7(100), 0, 100, None, vec![]),
        ),
    ];
    assert_eq!(t.events, expected);
    assert_eq!(
        has(
            &imp.report,
            ItemKind::LeftOut,
            "Per-note messages with no note"
        ),
        1
    );
}

#[test]
fn clip_texts_and_maps() {
    let meta_text = |status: u8, s: &str| ClipEvent::Text {
        group: 0,
        bank: ump::BANK_METADATA,
        status,
        text: s.into(),
    };
    let header = vec![pk(ump::set_tempo(60_000_000)), pk(ump::time_sig(3, 2, 8))];
    let events = vec![
        (0, meta_text(0x02, "Song")),
        (0, meta_text(0x04, "(c) Me")),
        (0, pk(ump::key_sig(-2, false))),
        (0, pk(ump::note_on(0, 0, 60, 0x8000, None))),
        (10, meta_text(0x00, "marker: Verse")),
        (20, meta_text(0x00, "cue: go")),
        (30, meta_text(0x00, "plain")),
        (40, meta_text(0x04, "late")),
        (
            50,
            ClipEvent::Text {
                group: 0,
                bank: ump::BANK_PERFORMANCE,
                status: 0x01,
                text: "la".into(),
            },
        ),
        (100, pk(ump::note_off(0, 0, 60, 0, None))),
    ];
    let imp = import(&clip_of(15_360, header, events, 100));
    assert_eq!(imp.title.as_deref(), Some("Song"));
    assert_eq!(imp.copyright.as_deref(), Some("(c) Me"));
    assert!(imp.has_tempo);
    assert_eq!(imp.tempo.points()[0].bpm, Bpm(100_000));
    let e = imp.meter.entries()[0];
    assert_eq!(e.meter, Meter { num: 3, den: 4 });
    assert_eq!(
        e.key,
        KeySig {
            sharps: -2,
            minor: false
        }
    );
    assert_eq!(imp.markers.len(), 1);
    assert_eq!(
        (imp.markers[0].tick, imp.markers[0].name.as_str()),
        (Tick(10), "Verse")
    );
    let texts: Vec<(i64, TextKind, String)> = imp.tracks[0]
        .events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Text { kind, text } => Some((e.tick.0, *kind, text.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            (20, TextKind::Cue, "go".into()),
            (30, TextKind::Text, "plain".into()),
            (40, TextKind::Copyright, "late".into()),
            (50, TextKind::Lyric, "la".into()),
        ]
    );
}

#[test]
fn huge_deltas_are_left_out_not_wrapped() {
    let d = 0x0FFF_FFFFu64;
    let f = type1(
        1,
        vec![vec![
            (0, cc(0, 1, 1)),
            (d, cc(0, 1, 2)),
            (2 * d, cc(0, 1, 3)),
        ]],
    );
    let imp = import(&f);
    assert_eq!(imp.tracks[0].events.len(), 1);
    assert_eq!(
        has(
            &imp.report,
            ItemKind::LeftOut,
            "Events past the end of the timeline"
        ),
        2
    );

    // 71 582 788 × 15 360 is just under 2^40; one bar later is past it.
    let f = type1(1, vec![vec![(0, cc(0, 1, 1)), (71_582_788, cc(0, 1, 2))]]);
    let imp = import(&f);
    assert_eq!(imp.tracks[0].events.len(), 2);
    assert!(
        imp.tracks[0]
            .events
            .iter()
            .all(|e| (0..=MAX_TICKS).contains(&e.tick.0))
    );
    let (doc, seq, _) = doc_with_track();
    let mut p = place(seq);
    p.tick = Tick(61_440);
    let (h, lanes, report) = apply_to(doc, &imp, &p);
    let placed = lane_events(&h, seq, lanes[0]);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].tick, Tick(61_440));
    assert_eq!(
        has(
            &report,
            ItemKind::LeftOut,
            "Events past the end of the timeline"
        ),
        1
    );
}

#[test]
fn channels_reuse_existing_destinations() {
    let (doc, seq, _) = doc_with_track();
    let (doc, synth2) = add_dest(&doc, "Synth 2", BUILTIN_SYNTH_REF, 0, 1);
    assert_eq!(suggested_dest(&doc, 0, 1), DestChoice::Existing(synth2));
    assert_eq!(suggested_dest(&doc, 0, 0), DestChoice::NewBuiltin);
    assert_eq!(suggested_dest(&doc, 0, 9), DestChoice::NewBuiltin);
    assert_eq!(suggested_dest(&doc, 1, 1), DestChoice::NewBuiltin);

    let keys = vec![
        (0, text(0x03, "Keys")),
        (0, on(0, 60, 100)),
        (0, on(1, 64, 100)),
        (0, cc(2, 7, 100)),
        (480, on(0, 62, 100)),
        (960, off(0, 60, 0)),
        (960, off(0, 62, 0)),
        (960, off(1, 64, 0)),
    ];
    let drums = vec![
        (0, text(0x03, "Beat")),
        (0, on(9, 36, 100)),
        (480, off(9, 36, 0)),
    ];
    let imp = import(&type1(960, vec![keys, drums]));
    let used: Vec<((u8, u8), u64)> = channels(&imp).into_iter().collect();
    assert_eq!(
        used,
        vec![((0, 0), 2), ((0, 1), 1), ((0, 2), 0), ((0, 9), 1)]
    );

    let (h, lanes, report) = apply_to(doc, &imp, &place(seq));
    let d = h.doc();
    let named = |n: &str| {
        let (id, dest) = d.instruments().iter().find(|(_, x)| x.name == n).unwrap();
        (*id, dest.kind.clone())
    };
    let (ch1, k1) = named("Built-in Synth (ch 1)");
    assert_eq!(
        k1,
        DestKind::MidiEndpoint {
            endpoint_ref: BUILTIN_SYNTH_REF.into(),
            group: 0,
            channel: 0
        }
    );
    let (ch10, _) = named("Built-in Synth (ch 10)");
    let (ch3_, _) = named("Built-in Synth (ch 3)");
    assert_eq!(d.instruments().len(), 4);
    assert_eq!(lanes.len(), 2);
    let keys_lane = d.lane(seq, lanes[0]).unwrap();
    assert_eq!(keys_lane.name, "Keys");
    assert_eq!(keys_lane.midi_dest, Some(ch1));
    assert_eq!(d.lane(seq, lanes[1]).unwrap().midi_dest, Some(ch10));
    for e in lane_events(&h, seq, lanes[0]) {
        let want = match e.group_channel {
            Some((0, 0)) => None,
            Some((0, 1)) => Some(synth2),
            Some((0, 2)) => Some(ch3_),
            gc => panic!("{gc:?}"),
        };
        assert_eq!(e.dest, want, "{e:?}");
    }
    assert_eq!(report.summary.tracks, 2);
    assert_eq!(report.summary.notes, 4);
}

#[test]
fn drum_tracks_get_gm_rows() {
    assert_eq!(drum_name(35), Some("Acoustic Bass Drum"));
    assert_eq!(drum_name(36), Some("Bass Drum 1"));
    assert_eq!(drum_name(81), Some("Open Triangle"));
    assert_eq!(drum_name(34), None);
    assert_eq!(drum_name(82), None);
    let names: Vec<_> = (35..=81).map(|n| drum_name(n).unwrap()).collect();
    assert_eq!(names.len(), 47);

    let f = type0(
        960,
        vec![
            (0, on(0, 60, 100)),
            (0, on(9, 36, 100)),
            (0, on(9, 38, 100)),
            (0, on(9, 42, 100)),
            (0, on(9, 20, 100)),
            (480, off(9, 36, 0)),
            (480, off(9, 38, 0)),
            (480, off(9, 42, 0)),
            (480, off(9, 20, 0)),
            (960, off(0, 60, 0)),
        ],
    );
    let imp = import(&f);
    let (doc, seq) = empty_doc();
    let (h, lanes, _) = apply_to(doc, &imp, &place(seq));
    let d = h.doc();
    let drum = lanes
        .iter()
        .map(|l| d.lane(seq, *l).unwrap())
        .find(|l| l.name == "Drums")
        .unwrap();
    let rows: BTreeMap<u8, String> = drum
        .drum
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .map(|r| (r.note, r.name.clone()))
        .collect();
    assert_eq!(rows[&36], "Bass Drum 1");
    assert_eq!(rows[&38], "Acoustic Snare");
    assert_eq!(rows[&42], "Closed Hi-Hat");
    assert_eq!(rows[&20], note_name(20));
    let other = lanes
        .iter()
        .map(|l| d.lane(seq, *l).unwrap())
        .find(|l| l.name != "Drums")
        .unwrap();
    assert!(other.drum.is_none());
}

/// A file at 90 BPM in 3/4 with a title, a copyright, a marker and a note.
fn song_file() -> Vec<u8> {
    type1(
        960,
        vec![
            vec![
                (0, text(0x03, "Song")),
                (0, text(0x02, "(c) Me")),
                (0, meta(0x51, &[0x0A, 0x2C, 0x2B])), // 666 667 µs: 90 BPM
                (0, meta(0x58, &[3, 2, 24, 8])),
                (960, text(0x06, "Verse")),
            ],
            vec![(0, on(0, 60, 100)), (960, off(0, 60, 0))],
        ],
    )
}

#[test]
fn importing_into_a_project_keeps_its_tempo_map() {
    let (doc, seq, lane) = doc_with_track();
    let ramp = TempoMap::new(vec![
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(120),
            ramp: true,
        },
        TempoPoint {
            tick: Tick(61_440),
            bpm: Bpm::whole(140),
            ramp: false,
        },
    ])
    .unwrap();
    let doc = run(
        &doc,
        &Command::Sequence(SequenceCmd::SetTempo {
            seq,
            tempo: ramp.clone(),
        }),
    )
    .unwrap()
    .doc;
    let doc = run(
        &doc,
        &Command::Event(EventCmd::Insert {
            seq,
            lane,
            events: vec![EventSpec::new(
                Tick(0),
                EventBody::note(48, 0x8000, Tick(100)),
            )],
        }),
    )
    .unwrap()
    .doc;
    let meter_before = doc.sequence(seq).unwrap().meter.clone();
    let imp = import(&song_file());
    let bar9 = Tick(8 * 61_440);

    let mut p = place(seq);
    p.tick = bar9;
    let (h, lanes, report) = apply_to(doc.clone(), &imp, &p);
    let s = h.doc().sequence(seq).unwrap();
    assert_eq!(s.tempo, ramp);
    assert_eq!(s.meter, meter_before);
    assert_eq!(s.start_offset, Tick(0));
    has(&report, ItemKind::Note, "The file's tempo map was not used");
    has(&report, ItemKind::Note, "title");
    has(&report, ItemKind::Note, "copyright");
    assert_eq!(s.name, "Main");
    assert!(h.doc().settings().info.copyright.is_none());
    let markers: Vec<_> = s.markers.iter().map(|m| (m.tick, m.name.clone())).collect();
    assert_eq!(markers, vec![(bar9 + Tick(15_360), "Verse".to_string())]);
    assert_eq!(report.summary.markers, 1);
    assert_eq!(
        notes(&lane_events(&h, seq, lanes[0])),
        vec![(bar9.0, 60, 15_360)]
    );

    // With the file's maps at bar 9: bars 1-8 keep the project's ramp and
    // 4/4; the file's 90 BPM and 3/4 start at bar 9, with its first note.
    p.use_tempo = true;
    let (h, lanes, report) = apply_to(doc.clone(), &imp, &p);
    let s = h.doc().sequence(seq).unwrap();
    let points: Vec<(Tick, u32, bool)> = s
        .tempo
        .points()
        .iter()
        .map(|p| (p.tick, p.bpm.0, p.ramp))
        .collect();
    assert_eq!(
        points,
        vec![
            (Tick(0), 120_000, true),
            (Tick(61_440), 140_000, false),
            (bar9, 90_000, false),
        ]
    );
    let entries: Vec<(Tick, Meter)> = s
        .meter
        .entries()
        .iter()
        .map(|e| (e.tick, e.meter))
        .collect();
    assert_eq!(
        entries,
        vec![
            (Tick(0), Meter { num: 4, den: 4 }),
            (bar9, Meter { num: 3, den: 4 }),
        ]
    );
    let first = lane_events(&h, seq, lanes[0])[0].tick;
    let at = s.meter.tick_to_bbu(first, 1);
    assert_eq!((at.bar, at.beat, at.unit), (9, 1, 0));
    assert_eq!(has(&report, ItemKind::Changed, "replaced from bar 9"), 1);
    lacks(&report, "moved to the start of bar");
    assert!(report.summary.tempo_map);

    // Off a barline, the drop moves to the start of its bar (Changed).
    p.tick = bar9 + Tick(100);
    let (h, lanes, report) = apply_to(doc.clone(), &imp, &p);
    assert_eq!(lane_events(&h, seq, lanes[0])[0].tick, bar9);
    has(&report, ItemKind::Changed, "moved to the start of bar 9");

    // Maps that already match the file change nothing and say nothing.
    let (h, _, _) = apply_to(doc.clone(), &imp, &place_at_zero_with_maps(seq));
    let same = h.doc().clone();
    let mut h2 = History::new(same);
    let mut report = Report::default();
    h2.begin("Import MIDI File").unwrap();
    apply(&mut h2, &imp, &place_at_zero_with_maps(seq), &mut report).unwrap();
    h2.commit().unwrap();
    lacks(&report, "replaced from bar");
}

fn place_at_zero_with_maps(seq: SeqId) -> Placement {
    let mut p = place(seq);
    p.use_tempo = true;
    p
}

#[test]
fn a_new_project_takes_the_file_whole() {
    // A one-beat pickup (1/4), then 4/4 from bar 1, at 100 BPM.
    let f = type1(
        960,
        vec![
            vec![
                (0, text(0x03, "Song")),
                (0, text(0x02, "(c) Me")),
                (0, meta(0x51, &[0x09, 0x27, 0xC0])), // 600 000 µs
                (0, meta(0x58, &[1, 2, 24, 8])),
                (960, meta(0x58, &[4, 2, 24, 8])),
                (960, text(0x06, "Bar 1")),
            ],
            vec![
                (0, on(0, 55, 100)),
                (960, off(0, 55, 0)),
                (960, on(0, 60, 100)),
                (1920, off(0, 60, 0)),
            ],
        ],
    );
    let imp = import(&f);
    assert_eq!(imp.start_offset, Tick(15_360));

    let (doc, seq) = empty_doc();
    let mut p = place(seq);
    p.new_project = true;
    let (h, lanes, report) = apply_to(doc, &imp, &p);
    let s = h.doc().sequence(seq).unwrap();
    assert_eq!(s.name, "Song");
    assert_eq!(h.doc().settings().info.copyright.as_deref(), Some("(c) Me"));
    assert_eq!(s.start_offset, Tick(15_360));
    assert_eq!(s.tempo.points()[0].bpm, Bpm(100_000));
    let entries: Vec<_> = s
        .meter
        .entries()
        .iter()
        .map(|e| (e.tick, e.meter))
        .collect();
    assert_eq!(entries, vec![(Tick(0), Meter { num: 4, den: 4 })]);
    assert_eq!(
        notes(&lane_events(&h, seq, lanes[0])),
        vec![(-15_360, 55, 15_360), (0, 60, 15_360)]
    );
    let markers: Vec<_> = s.markers.iter().map(|m| m.tick).collect();
    assert_eq!(markers, vec![Tick(0)]);
    assert!(report.summary.tempo_map);
    lacks(&report, "title was not used");

    let (doc, seq, _) = doc_with_track();
    let mut h = History::new(doc);
    let mut report = Report::default();
    h.begin("Import MIDI File").unwrap();
    let mut p = place(seq);
    p.new_project = true;
    let err = apply(&mut h, &imp, &p, &mut report).unwrap_err();
    assert!(
        err.to_string()
            .contains("A new-project import needs an empty project"),
        "{err}"
    );
    h.abort().unwrap();
}

#[test]
fn a_single_track_lands_on_the_drop_track() {
    let (doc, seq, drop) = doc_with_track();
    let (doc, x) = add_dest(&doc, "X", "ext", 0, 5);
    let one = import(&type1(
        960,
        vec![vec![
            (0, on(0, 60, 100)),
            (960, off(0, 60, 0)),
            (960, on(1, 62, 100)),
            (1920, off(1, 62, 0)),
        ]],
    ));
    let mut p = place(seq);
    p.lane = Some(drop);
    p.tick = Tick(-500);
    p.channel_map.insert((0, 0), DestChoice::Existing(x));
    let (h, lanes, _) = apply_to(doc.clone(), &one, &p);
    assert_eq!(lanes, vec![drop]);
    let d = h.doc();
    assert_eq!(d.sequence(seq).unwrap().lanes.len(), 1);
    // (0, 1) was not in the map: it took the suggestion, a new built-in synth.
    let (ch2_, _) = d
        .instruments()
        .iter()
        .find(|(_, x)| x.name == "Built-in Synth (ch 2)")
        .unwrap();
    let placed = lane_events(&h, seq, drop);
    assert_eq!(notes(&placed), vec![(0, 60, 15_360), (15_360, 62, 15_360)]);
    // The drop track has no destination, so every event names its own.
    assert_eq!(placed[0].dest, Some(x));
    assert_eq!(placed[1].dest, Some(*ch2_));

    let two = import(&type1(
        960,
        vec![
            vec![(0, on(0, 60, 100)), (960, off(0, 60, 0))],
            vec![(0, on(1, 62, 100)), (960, off(1, 62, 0))],
        ],
    ));
    let (doc, _) = add_dest(&doc, "after", "e", 3, 3);
    let doc = run(
        &doc,
        &Command::Lane(LaneCmd::Add {
            seq,
            name: "Last".into(),
            index: None,
        }),
    )
    .unwrap()
    .doc;
    let mut p = place(seq);
    p.lane = Some(drop);
    let (h, lanes, _) = apply_to(doc, &two, &p);
    assert_eq!(lanes.len(), 2);
    let order: Vec<LaneId> = h
        .doc()
        .sequence(seq)
        .unwrap()
        .lanes
        .iter()
        .map(|l| l.id)
        .collect();
    assert_eq!(order[0], drop);
    assert_eq!(&order[1..3], &lanes[..]);
    assert_eq!(order.len(), 4);
    assert!(lane_events(&h, seq, drop).is_empty());
}

#[test]
fn long_texts_are_cut_and_reported_once() {
    let limits = Limits {
        max_text_bytes: 10,
        ..Limits::default()
    };
    let f = type1(
        960,
        vec![vec![
            (0, text(0x01, "abcdefghijklmnopqrst")),
            (0, meta(0x01, &[0xFF, 0xFF, 0xFF, 0xFF])),
            (0, on(0, 60, 100)),
            (960, off(0, 60, 0)),
        ]],
    );
    let imp = parse(&f, &ImportOptions::default(), &limits, &no_cancel()).unwrap();
    // Cut once by the reader, once (after U+FFFD made it longer) by mapping.
    assert_eq!(has(&imp.report, ItemKind::Changed, "Texts over"), 2);
    let texts: Vec<String> = imp.tracks[0]
        .events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            "abcdefghij".to_string(),
            "\u{FFFD}\u{FFFD}\u{FFFD}".to_string()
        ]
    );

    // Song info fields are cut to fit `check_info`.
    let long = "x".repeat(5000);
    let f = type1(
        960,
        vec![vec![(0, text(0x02, &long)), (0, text(0x03, &long))]],
    );
    let imp = import(&f);
    let c = imp.copyright.clone().unwrap();
    assert_eq!(c.len(), core_model::document::MAX_INFO_BYTES);
    assert_eq!(
        imp.title.as_ref().unwrap().len(),
        core_model::document::MAX_INFO_BYTES
    );
    assert_eq!(has(&imp.report, ItemKind::Changed, "Song info cut"), 2);
    let info = core_model::document::SongInfo {
        copyright: Some(c),
        ..Default::default()
    };
    check_info(&info).unwrap();
}

#[test]
fn files_are_sniffed_and_size_checked() {
    let opts = ImportOptions::default();
    let limits = Limits::default();
    match parse(b"RIFF....", &opts, &limits, &no_cancel()) {
        Err(IoError::Refused(m)) => assert_eq!(m, "This isn't a MIDI file."),
        other => panic!("{other:?}"),
    }
    let cancel = AtomicBool::new(true);
    assert_eq!(
        parse(&song_file(), &opts, &limits, &cancel),
        Err(IoError::Cancelled)
    );

    let dir = tempfile::tempdir().unwrap();
    let ok = dir.path().join("song.mid");
    std::fs::write(&ok, song_file()).unwrap();
    let imp = parse_file(&ok, &opts, &limits, &no_cancel()).unwrap();
    assert_eq!(imp.tracks.len(), 1);
    // At the cap the size check passes (and the bytes are not MIDI);
    // one byte past it is refused before reading.
    let at = dir.path().join("at.mid");
    std::fs::File::create(&at)
        .unwrap()
        .set_len(MAX_MIDI_FILE_BYTES)
        .unwrap();
    match parse_file(&at, &opts, &limits, &no_cancel()) {
        Err(IoError::Refused(m)) => assert_eq!(m, "This isn't a MIDI file."),
        other => panic!("{other:?}"),
    }
    let big = dir.path().join("big.mid");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(MAX_MIDI_FILE_BYTES + 1)
        .unwrap();
    match parse_file(&big, &opts, &limits, &no_cancel()) {
        Err(IoError::Refused(m)) => assert!(m.contains("too large"), "{m}"),
        other => panic!("{other:?}"),
    }
    match parse_file(dir.path(), &opts, &limits, &no_cancel()) {
        Err(IoError::Refused(m)) => assert!(m.contains("not a regular file"), "{m}"),
        other => panic!("{other:?}"),
    }
}

/// Cases per property: `VIZDAW_FUZZ_CASES` (a long local run), else `n`.
fn cases(n: u32) -> u32 {
    std::env::var("VIZDAW_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(n)
}

/// Every `.mid` and `.midi2` fixture.
fn fixtures() -> Vec<Vec<u8>> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data");
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "mid" || x == "midi2"))
        .collect();
    paths.sort();
    paths.iter().map(|p| std::fs::read(p).unwrap()).collect()
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(cases(256)))]

    /// Damaged files parse or are refused, and whatever parses applies
    /// cleanly into a project (at 0 and spliced in at an off-barline drop)
    /// and as a new project.
    #[test]
    fn damaged_files_parse_or_refuse_and_apply(
        pick in 0usize..16,
        flips in proptest::collection::vec((0usize..4096, proptest::prelude::any::<u8>()), 0..8),
        mpe in proptest::prelude::any::<bool>(),
        split in proptest::option::of(proptest::prelude::any::<bool>()),
    ) {
        let all = fixtures();
        let mut bytes = all[pick % all.len()].clone();
        for (at, b) in flips {
            let n = bytes.len();
            bytes[at % n] = b;
        }
        let opts = ImportOptions { split_channels: split, mpe };
        let limits = Limits { max_events: 100_000, ..Limits::default() };
        match parse(&bytes, &opts, &limits, &no_cancel()) {
            Ok(imp) => {
                for (new_project, at) in [(false, 0), (true, 0), (false, 3 * 61_440 + 77)] {
                    let (doc, seq) = empty_doc();
                    let mut p = place(seq);
                    p.new_project = new_project;
                    p.use_tempo = true;
                    p.tick = Tick(at);
                    let mut h = History::new(doc);
                    let mut report = Report::default();
                    h.begin("Import MIDI File").unwrap();
                    let r = apply(&mut h, &imp, &p, &mut report);
                    proptest::prop_assert!(r.is_ok(), "{r:?}");
                    h.commit().unwrap();
                    proptest::prop_assert!(core_model::rules::validate(h.doc()).is_empty());
                }
            }
            Err(IoError::Refused(_)) => {}
            Err(e) => proptest::prop_assert!(false, "unexpected {e:?}"),
        }
    }
}

#[test]
fn a_drop_onto_an_instrument_track_keeps_its_instrument() {
    let (doc, seq, drop) = doc_with_track();
    let a = run(
        &doc,
        &Command::Routing(RoutingCmd::AddDestination {
            name: "Piano".into(),
            kind: DestKind::plugin("Piano"),
            default_patch: None,
        }),
    )
    .unwrap();
    let piano = created_dest(&a.report.created);
    let doc = run(
        &a.doc,
        &Command::Lane(LaneCmd::SetDest {
            seq,
            lane: drop,
            dest: Some(piano),
        }),
    )
    .unwrap()
    .doc;
    // A drum file on channel 10 with one note on channel 2.
    let f = type1(
        960,
        vec![vec![
            (0, on(9, 36, 100)),
            (0, on(9, 38, 100)),
            (0, on(1, 60, 100)),
            (480, off(9, 36, 0)),
            (480, off(9, 38, 0)),
            (480, off(1, 60, 0)),
        ]],
    );
    let imp = import(&f);
    let mut p = place(seq);
    p.lane = Some(drop);
    let (h, lanes, report) = apply_to(doc, &imp, &p);
    assert_eq!(lanes, vec![drop]);
    let d = h.doc();
    let names: Vec<&str> = d.instruments().values().map(|x| x.name.as_str()).collect();
    assert_eq!(names, vec!["Piano", "Built-in Synth (ch 2)"]);
    let ch2_ = d
        .instruments()
        .iter()
        .find(|(_, x)| x.name == "Built-in Synth (ch 2)")
        .map(|(id, _)| *id);
    let lane = d.lane(seq, drop).unwrap();
    assert_eq!(lane.midi_dest, Some(piano));
    assert!(lane.drum.is_none());
    for e in lane_events(&h, seq, drop) {
        let want = if e.group_channel == Some((0, 1)) {
            ch2_
        } else {
            None
        };
        assert_eq!(e.dest, want, "{e:?}");
    }
    has(&report, ItemKind::Note, "was not made a drum track");

    // MPE that the drop track does not take is noted too.
    let mut events = vec![(0, cc(0, 101, 0)), (0, cc(0, 100, 6)), (0, cc(0, 6, 15))];
    events.extend(mpe_body());
    let imp = import(&type1(960, vec![events]));
    let (doc, seq, drop) = doc_with_track();
    let mut p = place(seq);
    p.lane = Some(drop);
    let (_, _, report) = apply_to(doc, &imp, &p);
    has(&report, ItemKind::Note, "was not set to MPE");
}

#[test]
fn other_texts_channel_prefixes_and_sysex_map() {
    let f = type1(
        960,
        vec![vec![
            (0, text(0x04, "Piano")),
            (0, text(0x08, "Grand")),
            (0, text(0x09, "Port A")),
            (0, text(0x0A, "ten")),
            (0, meta(0x20, &[3])),
            (0, SmfEvent::SysEx(vec![0x7E, 0x7F, 0x09, 0x01])),
            (0, SmfEvent::SysEx(vec![0x41, 0x80])),
            (0, on(3, 60, 100)),
            (960, off(3, 60, 0)),
        ]],
    );
    let imp = import(&f);
    let t = &imp.tracks[0];
    let txt = |s: &str| EventBody::Text {
        kind: TextKind::Text,
        text: s.into(),
    };
    let expected = vec![
        spec(0, None, txt("Instrument: Piano")),
        spec(0, None, txt("Program: Grand")),
        spec(0, None, txt("Device: Port A")),
        spec(0, None, txt("ten")),
        spec(
            0,
            Some((0, 3)),
            EventBody::SysEx {
                data: Arc::from(vec![0x7Eu8, 0x7F, 0x09, 0x01]),
                eight_bit: false,
            },
        ),
        spec(
            0,
            Some((0, 3)),
            note_with(60, up7(100), 0, 15_360, None, vec![]),
        ),
    ];
    assert_eq!(t.events, expected);
    assert_eq!(
        has(&imp.report, ItemKind::Changed, "names became text events"),
        3
    );
    has(&imp.report, ItemKind::Note, "Channel prefixes");
    assert_eq!(
        has(&imp.report, ItemKind::LeftOut, "Damaged SysEx messages"),
        1
    );
}

#[test]
fn nothing_is_made_for_events_all_past_the_end() {
    let (doc, seq, _) = doc_with_track();
    let imp = import(&type1(
        960,
        vec![vec![(0, on(0, 60, 100)), (960, off(0, 60, 0))]],
    ));
    let mut p = place(seq);
    p.tick = Tick(MAX_TICKS);
    p.tick = p.tick + Tick(1);
    let (h, lanes, report) = apply_to(doc, &imp, &p);
    assert!(lanes.is_empty());
    assert!(h.doc().instruments().is_empty());
    assert_eq!(h.doc().sequence(seq).unwrap().lanes.len(), 1);
    assert_eq!(
        has(
            &report,
            ItemKind::LeftOut,
            "Events past the end of the timeline"
        ),
        1
    );
    assert_eq!(report.summary.tracks, 0);
}
