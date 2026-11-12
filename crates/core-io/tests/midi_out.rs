//! MIDI export: the document → files (plan 6a Task 6).

use core_engine::input_fx::{CENTER, up7, up32};
use core_engine::midi1::Midi1Msg;
use core_engine::mpe::{bend_out, configuration};
use core_io::IoError;
use core_io::clip::{self, ClipEvent};
use core_io::midi_out::{ClipOptions, SmfOptions, to_clip, to_smf};
use core_io::report::{ItemKind, Report};
use core_io::smf::{self, SmfEvent, SmfFile};
use core_io::ump::{self, Decoded};
use core_model::command::{
    Command, DrumCmd, EventCmd, LaneCmd, RoutingCmd, SequenceCmd, SettingsCmd, apply as run,
};
use core_model::document::{DestKind, Document, MidiMix, Patch};
use core_model::event::{
    EventBody, EventSpec, LoopCount, NoteAttribute, PerNoteCtl, PerNoteKind, TextKind,
};
use core_model::ids::{AnyId, DestId, LaneId, SeqId};
use core_model::lane::QuantizeSpec;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::mixer::BUILTIN_SYNTH_REF;
use core_model::mpe::MpeConfig;
use core_model::query::flatten;
use core_model::selection::ViewFilter;
use core_model::sequence::{Marker, SeqLength};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::Tick;
use std::path::PathBuf;
use std::sync::Arc;

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

// ---- building documents ----

/// A document with one sequence, built by commands.
struct Doc {
    doc: Document,
    seq: SeqId,
}

impl Doc {
    /// A document whose one sequence `name` has no tracks.
    fn new(name: &str) -> Doc {
        let a = run(
            &Document::new(),
            &Command::Sequence(SequenceCmd::New { name: name.into() }),
        )
        .unwrap();
        let seq = a
            .report
            .created
            .iter()
            .find_map(|id| match id {
                AnyId::Seq(s) => Some(*s),
                _ => None,
            })
            .unwrap();
        let mut d = Doc { doc: a.doc, seq };
        let lane = d.doc.sequence(seq).unwrap().lanes.front().unwrap().id;
        d.exec(Command::Lane(LaneCmd::Remove { seq, lane }));
        d
    }

    fn exec(&mut self, cmd: Command) -> Vec<AnyId> {
        let a = run(&self.doc, &cmd).unwrap();
        self.doc = a.doc;
        a.report.created
    }

    /// Another sequence `name` with one track; returns both.
    fn sequence(&mut self, name: &str) -> (SeqId, LaneId) {
        let created = self.exec(Command::Sequence(SequenceCmd::New { name: name.into() }));
        let seq = created
            .iter()
            .find_map(|id| match id {
                AnyId::Seq(s) => Some(*s),
                _ => None,
            })
            .unwrap();
        let lane = self.doc.sequence(seq).unwrap().lanes.front().unwrap().id;
        (seq, lane)
    }

    fn dest(&mut self, name: &str, endpoint: &str, channel: u8, patch: Option<Patch>) -> DestId {
        let created = self.exec(Command::Routing(RoutingCmd::AddDestination {
            name: name.into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: endpoint.into(),
                group: 0,
                channel,
            },
            default_patch: patch,
        }));
        created
            .iter()
            .find_map(|id| match id {
                AnyId::Dest(d) => Some(*d),
                _ => None,
            })
            .unwrap()
    }

    fn builtin(&mut self, channel: u8) -> DestId {
        let name = format!("Built-in Synth (ch {})", channel + 1);
        self.dest(&name, BUILTIN_SYNTH_REF, channel, None)
    }

    fn track(&mut self, name: &str, dest: Option<DestId>) -> LaneId {
        let seq = self.seq;
        let created = self.exec(Command::Lane(LaneCmd::Add {
            seq,
            name: name.into(),
            index: None,
        }));
        let lane = created
            .iter()
            .find_map(|id| match id {
                AnyId::Lane(l) => Some(*l),
                _ => None,
            })
            .unwrap();
        if dest.is_some() {
            self.exec(Command::Lane(LaneCmd::SetDest { seq, lane, dest }));
        }
        lane
    }

    fn put_in(&mut self, seq: SeqId, lane: LaneId, events: Vec<EventSpec>) {
        self.exec(Command::Event(EventCmd::Insert { seq, lane, events }));
    }

    fn put(&mut self, lane: LaneId, events: Vec<EventSpec>) {
        self.put_in(self.seq, lane, events);
    }

    fn tempo(&mut self, points: &[(i64, u32, bool)]) {
        let tempo = TempoMap::new(
            points
                .iter()
                .map(|&(t, bpm, ramp)| TempoPoint {
                    tick: Tick(t),
                    bpm: Bpm::whole(bpm),
                    ramp,
                })
                .collect(),
        )
        .unwrap();
        let seq = self.seq;
        self.exec(Command::Sequence(SequenceCmd::SetTempo { seq, tempo }));
    }

    fn meter(&mut self, entries: &[(i64, u8, u8, i8)]) {
        let meter = MeterMap::new(
            entries
                .iter()
                .map(|&(t, num, den, sharps)| MeterEntry {
                    tick: Tick(t),
                    meter: Meter { num, den },
                    key: KeySig {
                        sharps,
                        minor: false,
                    },
                })
                .collect(),
        )
        .unwrap();
        let seq = self.seq;
        self.exec(Command::Sequence(SequenceCmd::SetMeter { seq, meter }));
    }

    fn markers(&mut self, markers: &[(i64, &str)]) {
        let markers = markers
            .iter()
            .map(|&(t, n)| Marker {
                tick: Tick(t),
                name: n.into(),
            })
            .collect();
        let seq = self.seq;
        self.exec(Command::Sequence(SequenceCmd::SetMarkers { seq, markers }));
    }

    fn flags(&mut self, lane: LaneId, mute: Option<bool>, solo: Option<bool>) {
        let seq = self.seq;
        self.exec(Command::Lane(LaneCmd::SetFlags {
            seq,
            lane,
            mute,
            solo,
            arm: None,
        }));
    }

    fn smf(&self, ppq: u16) -> (SmfFile, Report) {
        self.smf_with(ppq, false, false)
    }

    fn smf_with(&self, ppq: u16, type0: bool, mpe: bool) -> (SmfFile, Report) {
        to_smf(
            &self.doc,
            &SmfOptions {
                seq: self.seq,
                lanes: vec![],
                type0,
                ppq,
                mpe,
            },
        )
        .unwrap()
    }
}

fn note(tick: i64, pitch: u8, velocity: u16, duration: i64) -> EventSpec {
    EventSpec::new(Tick(tick), EventBody::note(pitch, velocity, Tick(duration)))
}

fn ev(tick: i64, body: EventBody) -> EventSpec {
    EventSpec::new(Tick(tick), body)
}

fn text(tick: i64, kind: TextKind, s: &str) -> EventSpec {
    ev(
        tick,
        EventBody::Text {
            kind,
            text: s.into(),
        },
    )
}

/// The document of the two hand-written goldens: "Song" at 120 BPM in 4/4
/// and C major; track "Bass" on a built-in synth on channel 1; one note.
fn small_doc() -> (Doc, LaneId) {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0);
    let bass = d.track("Bass", Some(dest));
    d.put(bass, vec![note(0, 60, up7(100), Q)]);
    (d, bass)
}

/// The regression goldens' document.
fn golden_doc() -> (Doc, LaneId, LaneId) {
    let mut d = Doc::new("Song");
    d.tempo(&[(0, 120, false), (BAR, 120, true), (2 * BAR, 140, false)]);
    d.meter(&[(0, 4, 4, 2), (2 * BAR, 3, 4, 2)]);
    d.markers(&[(0, "Intro"), (2 * BAR, "Verse")]);
    let mut info = d.doc.settings().info.clone();
    info.copyright = Some("Copyright 2026 the VizDAW tests".into());
    d.exec(Command::Settings(SettingsCmd::SetInfo { info }));
    // A MIDI mix is only kept for an external destination.
    let bass_dest = d.dest(
        "Bass Synth",
        "ext:bass",
        0,
        Some(Patch {
            program: 33,
            bank: Some((0, 1)),
        }),
    );
    d.exec(Command::Routing(RoutingCmd::SetMidiMix {
        dest: bass_dest,
        mix: MidiMix {
            volume: Some(100),
            pan: None,
            mute: false,
        },
    }));
    let bass = d.track("Bass", Some(bass_dest));
    let mut muted = note(Q, 45, up7(100), Q);
    muted.mute = true;
    d.put(
        bass,
        vec![
            note(0, 40, up7(90), Q / 2),
            text(0, TextKind::Lyric, "Down"),
            ev(
                0,
                EventBody::SysEx {
                    data: Arc::from(&[0x7E, 0x7F, 0x09, 0x01][..]),
                    eight_bit: false,
                },
            ),
            note(Q / 2, 43, up7(100), Q / 2),
            muted,
            ev(
                2 * Q,
                EventBody::Cc {
                    index: 1,
                    value: up32(64, 7),
                },
            ),
            ev(
                3 * Q,
                EventBody::PitchBend {
                    value: up32(8192 + 100, 14),
                },
            ),
            note(2 * BAR, 40, up7(80), Q),
            note(2 * BAR + 2 * Q, 43, up7(80), Q),
        ],
    );
    let drum_dest = d.builtin(9);
    let drums = d.track("Drums", Some(drum_dest));
    d.put(
        drums,
        vec![
            note(0, 36, up7(110), Q / 4),
            note(0, 42, up7(70), Q / 4),
            note(Q / 2, 42, up7(70), Q / 4),
            note(Q, 38, up7(100), Q / 4),
            note(BAR, 36, up7(110), Q / 4),
        ],
    );
    let seq = d.seq;
    d.exec(Command::Drum(DrumCmd::MarkDrumLane {
        seq,
        lane: drums,
        drum: true,
    }));
    (d, bass, drums)
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

/// Channel messages of track `i` as `(tick, bytes)`.
fn channel_msgs(f: &SmfFile, i: usize) -> Vec<(u64, Vec<u8>)> {
    f.tracks[i]
        .events
        .iter()
        .filter_map(|(t, e)| match e {
            SmfEvent::Channel(m) => Some((*t, m.as_slice().to_vec())),
            _ => None,
        })
        .collect()
}

/// Metas of `kind` on track `i` as `(tick, data)`.
fn metas(f: &SmfFile, i: usize, kind: u8) -> Vec<(u64, Vec<u8>)> {
    f.tracks[i]
        .events
        .iter()
        .filter_map(|(t, e)| match e {
            SmfEvent::Meta { kind: k, data } if *k == kind => Some((*t, data.clone())),
            _ => None,
        })
        .collect()
}

/// `(tick, pitch, velocity, duration)` of the notes on track `i`, paired
/// first-on first-off.
fn smf_notes(f: &SmfFile, i: usize) -> Vec<(u64, u8, u8, u64)> {
    let mut open: Vec<(u8, u8, u64, u8)> = Vec::new();
    let mut out = Vec::new();
    for (t, m) in channel_msgs(f, i) {
        let (st, d1, d2) = (m[0], m[1], m.get(2).copied().unwrap_or(0));
        match st & 0xF0 {
            0x90 if d2 > 0 => open.push((st & 0x0F, d1, t, d2)),
            0x80 | 0x90 => {
                let k = open
                    .iter()
                    .position(|o| o.0 == st & 0x0F && o.1 == d1)
                    .unwrap();
                let (_, p, s, v) = open.remove(k);
                out.push((s, p, v, t - s));
            }
            _ => {}
        }
    }
    assert!(open.is_empty(), "notes with no end: {open:?}");
    out.sort();
    out
}

fn data_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data"))
}

fn hex(b: &[u8]) -> String {
    b.chunks(16)
        .map(|c| c.iter().map(|x| format!("{x:02X}")).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A hand-written golden: never blessed.
#[track_caller]
fn hand_golden(name: &str, got: &[u8]) {
    let want = std::fs::read(data_dir().join(name))
        .unwrap_or_else(|e| panic!("hand-written golden {name} is missing: {e}"));
    assert_eq!(hex(got), hex(&want), "{name}");
}

/// A regression golden: `VIZDAW_BLESS=1` (re)writes it; a missing one fails.
#[track_caller]
fn golden(name: &str, got: &[u8]) {
    let path = data_dir().join(name);
    if std::env::var("VIZDAW_BLESS").is_ok_and(|v| v == "1") {
        std::fs::write(&path, got).unwrap();
    }
    let want = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("golden {name} is missing (bless with VIZDAW_BLESS=1): {e}"));
    assert_eq!(hex(got), hex(&want), "{name}");
}

// ---- tests ----

#[test]
fn small_goldens_match() {
    let (d, bass) = small_doc();
    let (f, r) = d.smf(960);
    hand_golden("export_small_type1_960.mid", &smf::write(&f).unwrap());
    assert!(r.is_empty(), "{:?}", r.items);
    let (c, r) = to_clip(
        &d.doc,
        &ClipOptions {
            seq: d.seq,
            lane: bass,
            range: None,
        },
    )
    .unwrap();
    hand_golden("export_small_clip.midi2", &clip::write(&c));
    assert!(r.is_empty(), "{:?}", r.items);
}

#[test]
fn golden_options() {
    let (d, _, _) = golden_doc();
    for (name, ppq, type0) in [
        ("export_type1_960.mid", 960, false),
        ("export_type0_960.mid", 960, true),
        ("export_type1_480.mid", 480, false),
        ("export_type1_15360.mid", 15360, false),
    ] {
        let (f, _) = d.smf_with(ppq, type0, false);
        assert_eq!(f.format, u16::from(!type0));
        golden(name, &smf::write(&f).unwrap());
    }
}

#[test]
fn refusals() {
    let (mut d, bass) = small_doc();
    let (other, other_lane) = d.sequence("Other");
    let refused = |r: Result<(SmfFile, Report), IoError>| match r {
        Err(IoError::Refused(m)) => {
            assert!(!m.to_lowercase().contains("lane"), "{m}");
            m
        }
        other => panic!("{other:?}"),
    };
    let opts = |seq, lanes: Vec<LaneId>, ppq| SmfOptions {
        seq,
        lanes,
        type0: false,
        ppq,
        mpe: false,
    };
    let unknown = SeqId(9_999);
    refused(to_smf(&d.doc, &opts(unknown, vec![], 960)));
    refused(to_smf(&d.doc, &opts(d.seq, vec![LaneId(9_999)], 960)));
    refused(to_smf(&d.doc, &opts(d.seq, vec![other_lane], 960)));
    for ppq in [0, 96, 1000, 15_359] {
        refused(to_smf(&d.doc, &opts(d.seq, vec![], ppq)));
    }
    for ppq in [480, 960, 15_360] {
        assert!(to_smf(&d.doc, &opts(d.seq, vec![bass], ppq)).is_ok());
    }
    let clip = |seq, lane, range| to_clip(&d.doc, &ClipOptions { seq, lane, range });
    for r in [
        clip(unknown, bass, None),
        clip(d.seq, LaneId(9_999), None),
        clip(d.seq, other_lane, None),
        clip(other, bass, None),
        clip(d.seq, bass, Some((Tick(Q), Tick(Q)))),
        clip(d.seq, bass, Some((Tick(Q), Tick(0)))),
    ] {
        match r {
            Err(IoError::Refused(m)) => assert!(!m.to_lowercase().contains("lane"), "{m}"),
            other => panic!("{other:?}"),
        }
    }
    assert!(clip(d.seq, bass, Some((Tick(0), Tick(Q)))).is_ok());
}

#[test]
fn what_is_written_is_what_plays() {
    let mut d = Doc::new("Song");
    let (riff, riff_lane) = d.sequence("Riff");
    d.put_in(
        riff,
        riff_lane,
        vec![note(0, 60, up7(100), Q), note(Q, 62, up7(90), Q / 2)],
    );
    let seq = d.seq;
    d.exec(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(3 * BAR)),
    }));
    let dest = d.builtin(0);
    let refs = d.track("Refs", Some(dest));
    let reference = |tick| {
        ev(
            tick,
            EventBody::Reference {
                seq: riff,
                length: None,
                loop_count: LoopCount::Count(1),
                transpose: 2,
            },
        )
    };
    d.put(refs, vec![reference(0), reference(BAR)]);
    let dest2 = d.builtin(1);
    let looped = d.track("Loop", Some(dest2));
    d.put(
        looped,
        vec![
            note(100, 48, up7(100), Q / 2),
            note(Q + 3_000, 50, up7(100), Q / 2),
            note(2 * Q + 7_000, 52, up7(100), Q / 2),
        ],
    );
    d.exec(Command::Lane(LaneCmd::SetLength {
        seq,
        lane: looped,
        length: Some(Tick(BAR)),
        looped: true,
        locked: false,
    }));
    d.exec(Command::Lane(LaneCmd::SetPlayQuantize {
        seq,
        lane: looped,
        quantize: Some(QuantizeSpec::grid(Tick(Q / 2), 100)),
    }));
    d.exec(Command::Lane(LaneCmd::SetPlayShift {
        seq,
        lane: looped,
        shift: Tick(10),
    }));
    let dest3 = d.builtin(2);
    let takes = d.track("Takes", Some(dest3));
    d.put(takes, vec![note(0, 70, up7(100), Q)]);
    d.exec(Command::Lane(LaneCmd::AddTake {
        seq,
        lane: takes,
        name: "Take 2".into(),
    }));

    let (f, _) = d.smf(15_360);
    assert_eq!(f.tracks.len(), 4);
    let played: Vec<_> = flatten(&d.doc, seq, None, &ViewFilter::default())
        .unwrap()
        .collect();
    for (i, lane) in [(1, refs), (2, looped)] {
        let mut want: Vec<(u64, u8, u8, u64)> = played
            .iter()
            .filter(|r| r.path[0].lane == lane)
            .filter_map(|r| match &r.body {
                EventBody::Note {
                    pitch,
                    velocity,
                    duration,
                    ..
                } => Some((
                    r.tick.0 as u64,
                    *pitch,
                    (velocity >> 9) as u8,
                    duration.0 as u64,
                )),
                _ => None,
            })
            .collect();
        want.sort();
        assert!(!want.is_empty());
        assert_eq!(smf_notes(&f, i), want, "track {i}");
    }
    // The reference plays transposed, twice; the loop fills three bars.
    assert_eq!(smf_notes(&f, 1).len(), 4);
    assert!(smf_notes(&f, 1).iter().all(|n| n.1 == 62 || n.1 == 64));
    assert_eq!(smf_notes(&f, 2).len(), 9);
    assert_eq!(smf_notes(&f, 2)[0].0, 10);
    // The inactive take writes nothing.
    assert!(smf_notes(&f, 3).is_empty());
}

#[test]
fn the_conductor_carries_the_maps() {
    let (d, _, _) = golden_doc();
    let (f, r) = d.smf(15_360);
    assert_eq!(f.tracks[0].events[0], (0, meta(0x03, b"Song")));
    assert_eq!(
        metas(&f, 0, 0x02),
        vec![(0, b"Copyright 2026 the VizDAW tests".to_vec())]
    );
    let tempos = metas(&f, 0, 0x51);
    let ticks: Vec<u64> = tempos.iter().map(|(t, _)| *t).collect();
    let mut want = vec![0u64];
    want.extend((0..16).map(|k| (BAR + k * 3_840) as u64));
    want.push(2 * BAR as u64);
    assert_eq!(ticks, want);
    let us: Vec<u32> = tempos
        .iter()
        .map(|(_, d)| u32::from_be_bytes([0, d[0], d[1], d[2]]))
        .collect();
    assert_eq!(us[0], 500_000);
    assert_eq!(*us.last().unwrap(), 428_571);
    assert!(us.windows(2).skip(1).all(|w| w[0] > w[1]), "{us:?}");
    assert_eq!(
        has(
            &r,
            ItemKind::Changed,
            "Tempo ramp at bar 2 written as 16 steps"
        ),
        1
    );
    assert_eq!(
        metas(&f, 0, 0x58),
        vec![(0, vec![4, 2, 24, 8]), (2 * BAR as u64, vec![3, 2, 24, 8])]
    );
    assert_eq!(metas(&f, 0, 0x59), vec![(0, vec![2, 0])]);
    assert_eq!(
        metas(&f, 0, 0x06),
        vec![(0, b"Intro".to_vec()), (2 * BAR as u64, b"Verse".to_vec())]
    );
    // Patch and mix at file position 0, before the track's own events.
    let msgs = channel_msgs(&f, 1);
    assert_eq!(
        msgs[..4].to_vec(),
        vec![
            (0, vec![0xB0, 0, 0]),
            (0, vec![0xB0, 32, 1]),
            (0, vec![0xC0, 33]),
            (0, vec![0xB0, 7, 100]),
        ]
    );
    assert_eq!(f.tracks[1].events[0], (0, meta(0x03, b"Bass")));
    // The muted note is not written.
    assert!(smf_notes(&f, 1).iter().all(|n| n.1 != 45));
    has(&r, ItemKind::LeftOut, "Muted notes");
}

fn meta(kind: u8, data: &[u8]) -> SmfEvent {
    SmfEvent::Meta {
        kind,
        data: data.to_vec(),
    }
}

#[test]
fn ruling_10_orders_each_tick() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0);
    let lane = d.track("Bass", Some(dest));
    d.put(
        lane,
        vec![
            note(0, 60, up7(100), Q),
            ev(
                Q,
                EventBody::SysEx {
                    data: Arc::from(&[1u8, 2][..]),
                    eight_bit: false,
                },
            ),
            note(Q, 62, up7(100), Q),
            ev(
                Q,
                EventBody::Cc {
                    index: 7,
                    value: up32(90, 7),
                },
            ),
            ev(
                Q,
                EventBody::Program {
                    program: 5,
                    bank: Some((1, 2)),
                },
            ),
            text(Q, TextKind::Text, "here"),
        ],
    );
    let (f, _) = d.smf(960);
    let at_q: Vec<&SmfEvent> = f.tracks[1]
        .events
        .iter()
        .filter(|(t, _)| *t == 960)
        .map(|(_, e)| e)
        .collect();
    let ch = |b: &[u8]| SmfEvent::Channel(Midi1Msg::three(b[0], b[1], b[2]));
    assert_eq!(
        at_q,
        vec![
            &meta(0x01, b"here"),
            &ch(&[0x80, 60, 0]),
            &ch(&[0xB0, 0, 1]),
            &ch(&[0xB0, 32, 2]),
            &SmfEvent::Channel(Midi1Msg::two(0xC0, 5)),
            &ch(&[0xB0, 7, 90]),
            &SmfEvent::SysEx(vec![1, 2]),
            &ch(&[0x90, 62, 100]),
        ]
    );
}

#[test]
fn groups_become_ports_and_tracks() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0);
    let bass = d.track("Bass", Some(dest));
    let mut high = note(Q, 62, up7(100), Q);
    high.group_channel = Some((2, 0));
    d.put(bass, vec![note(0, 60, up7(100), Q), high]);
    let (f, _) = d.smf(960);
    assert_eq!(f.tracks.len(), 3);
    assert_eq!(f.tracks[1].events[0], (0, meta(0x03, b"Bass")));
    assert!(metas(&f, 1, 0x21).is_empty());
    assert_eq!(f.tracks[2].events[0], (0, meta(0x21, &[2])));
    assert_eq!(f.tracks[2].events[1], (0, meta(0x03, b"Bass (group 3)")));
    assert_eq!(smf_notes(&f, 1), vec![(0, 60, 100, 960)]);
    assert_eq!(smf_notes(&f, 2), vec![(960, 62, 100, 960)]);
}

#[test]
fn mpe_tracks_use_the_encoder() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0);
    let cfg = MpeConfig::default();
    d.exec(Command::Routing(RoutingCmd::SetMpe {
        dest,
        mpe: Some(cfg),
    }));
    let lead = d.track("Lead", Some(dest));
    let bend = CENTER + 0x0100_0000;
    let with_bend = |tick: i64, pitch: u8| {
        let mut n = note(tick, pitch, up7(100), 2 * Q);
        if let EventBody::Note { per_note, .. } = &mut n.body {
            per_note.push_back(PerNoteCtl {
                kind: PerNoteKind::PitchBend,
                offset: Tick(0),
                value: bend,
            });
        }
        n
    };
    d.put(
        lead,
        vec![
            with_bend(0, 60),
            with_bend(0, 64),
            with_bend(0, 67),
            with_bend(Q, 72),
        ],
    );

    let (f, r) = d.smf_with(15_360, false, true);
    let msgs = channel_msgs(&f, 1);
    let config: Vec<(u64, Vec<u8>)> = configuration(&cfg)
        .iter()
        .map(|m| (0, m.as_slice().to_vec()))
        .collect();
    assert_eq!(msgs[..config.len()].to_vec(), config);
    let ons: Vec<(usize, u8)> = msgs
        .iter()
        .enumerate()
        .filter(|(_, (_, m))| m[0] & 0xF0 == 0x90)
        .map(|(i, (_, m))| (i, m[0] & 0x0F))
        .collect();
    assert_eq!(ons.len(), 4);
    let mut chans: Vec<u8> = ons.iter().map(|o| o.1).collect();
    chans.sort();
    chans.dedup();
    assert_eq!(chans.len(), 4, "{ons:?}");
    assert!(chans.iter().all(|c| cfg.is_member(*c)));
    let (lsb, msb) = {
        let v = bend_out(bend, cfg.bend_range);
        ((v & 0x7F) as u8, (v >> 7) as u8)
    };
    for (i, ch) in ons {
        assert!(
            msgs[i..]
                .iter()
                .any(|(_, m)| *m == vec![0xE0 | ch, lsb, msb]),
            "no bend on channel {ch}"
        );
    }
    lacks(&r, "Per-note expression");

    let (f, r) = d.smf_with(15_360, false, false);
    assert_eq!(
        has(&r, ItemKind::LeftOut, "Per-note expression on 4 notes"),
        1
    );
    let msgs = channel_msgs(&f, 1);
    assert!(msgs.iter().all(|(_, m)| m[0] & 0x0F == 0));
    assert!(msgs.iter().all(|(_, m)| m[0] & 0xF0 != 0xE0));
}

#[test]
fn export_lists_what_it_leaves_out() {
    let mut d = Doc::new("Song");
    let seq = d.seq;
    let (riff, riff_lane) = d.sequence("Riff");
    d.put_in(
        riff,
        riff_lane,
        vec![note(0, 60, up7(100), Q), note(Q, 62, up7(100), Q)],
    );
    d.exec(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(2 * BAR)),
    }));
    let mut info = d.doc.settings().info.clone();
    info.artist = Some("Someone".into());
    info.composer = Some("Someone Else".into());
    d.exec(Command::Settings(SettingsCmd::SetInfo { info }));
    let dest = d.builtin(0);
    let keys = d.track("Keys", Some(dest));
    let mut muted = note(Q, 64, up7(100), Q);
    muted.mute = true;
    let mut attributed = note(2 * Q, 65, up7(64), Q);
    if let EventBody::Note { attribute, .. } = &mut attributed.body {
        *attribute = Some(NoteAttribute {
            kind: 3,
            data: 0x7800,
        });
    }
    d.put(
        keys,
        vec![
            note(5, 60, 0x1234, Q),
            muted,
            attributed,
            ev(
                3 * Q,
                EventBody::SysEx {
                    data: Arc::from(&[0x80u8, 1][..]),
                    eight_bit: true,
                },
            ),
            note(-100, 61, up7(100), 50),
        ],
    );
    d.flags(keys, Some(true), None);
    let pad = d.track("Pad", Some(dest));
    let mut muted_ref = ev(BAR, EventBody::reference(riff));
    muted_ref.mute = true;
    d.put(pad, vec![muted_ref, note(3 * BAR, 67, up7(100), Q)]);
    let kit_dest = d.builtin(9);
    let kit = d.track("Kit", Some(kit_dest));
    d.put(
        kit,
        vec![note(0, 36, up7(100), Q / 4), note(Q, 38, up7(100), Q / 4)],
    );
    d.exec(Command::Drum(DrumCmd::MarkDrumLane {
        seq,
        lane: kit,
        drum: true,
    }));

    let (f, r) = d.smf(960);
    assert_eq!(f.tracks.len(), 4, "the muted track is still written");
    assert_eq!(
        has(
            &r,
            ItemKind::Changed,
            "Velocities rounded to 7 bits (MIDI file)"
        ),
        1
    );
    assert!(
        has(
            &r,
            ItemKind::Changed,
            "Positions rounded to 960 ticks per quarter"
        ) >= 1
    );
    assert_eq!(has(&r, ItemKind::LeftOut, "Muted notes"), 3);
    assert_eq!(has(&r, ItemKind::LeftOut, "Events outside the sequence"), 2);
    assert_eq!(has(&r, ItemKind::LeftOut, "Note attributes"), 1);
    assert_eq!(has(&r, ItemKind::LeftOut, "8-bit SysEx"), 1);
    assert_eq!(has(&r, ItemKind::LeftOut, "Drum row names"), 2);
    assert_eq!(
        has(
            &r,
            ItemKind::LeftOut,
            "Song info other than title and copyright"
        ),
        2
    );
    assert_eq!(
        has(&r, ItemKind::Note, "Track 'Keys' is muted in the project"),
        1
    );
    for i in &r.items {
        assert!(!i.what.to_lowercase().contains("lane"), "{}", i.what);
    }
    assert_eq!(r.summary.tracks, 3);
    assert!(r.summary.tempo_map);

    let (_, r) = d.smf(15_360);
    lacks(&r, "Positions rounded");
    has(&r, ItemKind::Changed, "Velocities rounded");

    // A soloed track silences the others in the project; all are written.
    d.flags(keys, Some(false), None);
    d.flags(pad, None, Some(true));
    let (f, r) = d.smf(960);
    assert_eq!(f.tracks.len(), 4);
    has(&r, ItemKind::Note, "Track 'Keys'");
    has(&r, ItemKind::Note, "Track 'Kit'");
    lacks(&r, "Track 'Pad'");
    assert!(!smf_notes(&f, 1).is_empty());
}

#[test]
fn chosen_tracks_only() {
    let (d, _, drums) = golden_doc();
    let (f, r) = to_smf(
        &d.doc,
        &SmfOptions {
            seq: d.seq,
            lanes: vec![drums],
            type0: false,
            ppq: 960,
            mpe: false,
        },
    )
    .unwrap();
    assert_eq!(f.tracks.len(), 2);
    assert_eq!(f.tracks[1].events[0], (0, meta(0x03, b"Drums")));
    assert_eq!(r.summary.tracks, 1);
    assert_eq!(r.summary.notes, 5);
}

#[test]
fn clip_export_is_exact() {
    let (mut d, _, _) = golden_doc();
    let dest = d.builtin(2);
    let lead = d.track("Lead", Some(dest));
    let attr = NoteAttribute {
        kind: 3,
        data: 0x7800,
    };
    let per_note: Vec<PerNoteCtl> = vec![
        PerNoteCtl {
            kind: PerNoteKind::Registered(74),
            offset: Tick(0),
            value: 0x9000_0000,
        },
        PerNoteCtl {
            kind: PerNoteKind::PitchBend,
            offset: Tick(500),
            value: 0x8100_0000,
        },
    ];
    let expressive = EventSpec::new(
        Tick(BAR + 100),
        EventBody::Note {
            pitch: 60,
            velocity: 0x1234,
            release_velocity: 0x0400,
            duration: Tick(1_000),
            attribute: Some(attr),
            per_note: per_note.into_iter().collect(),
        },
    );
    let bar4 = 2 * BAR + 3 * Q;
    d.put(
        lead,
        vec![
            note(0, 50, up7(100), Q),
            expressive,
            ev(
                BAR + 2_000,
                EventBody::Cc {
                    index: 74,
                    value: 0x1234_5678,
                },
            ),
            note(2 * BAR + Q, 62, 0xFFFF, Q),
            note(bar4, 64, up7(100), Q),
        ],
    );
    let (c, r) = to_clip(
        &d.doc,
        &ClipOptions {
            seq: d.seq,
            lane: lead,
            range: Some((Tick(BAR), Tick(bar4))),
        },
    )
    .unwrap();
    assert_eq!(c.tpq, 15_360);
    assert_eq!(c.end, (bar4 - BAR) as u64);
    // The header: the tempo (the ramp's first step) and meter at bar 2.
    assert_eq!(c.header.len(), 2);
    match &c.header[0] {
        ClipEvent::Packet(p) => match ump::decode(p) {
            Decoded::SetTempo(v) => assert!((49_500_000..50_000_000).contains(&v), "{v}"),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    assert_eq!(c.header[1], ClipEvent::Packet(ump::time_sig(4, 2, 8)));
    let at = |tick: i64| -> Vec<&ClipEvent> {
        c.events
            .iter()
            .filter(|(t, _)| *t == tick as u64)
            .map(|(_, e)| e)
            .collect()
    };
    let txt = |status: u8, s: &str| ClipEvent::Text {
        group: 0,
        bank: 1,
        status,
        text: s.into(),
    };
    let pk = ClipEvent::Packet;
    assert_eq!(
        at(0),
        vec![
            &txt(0x02, "Song"),
            &txt(0x03, "Lead"),
            &txt(0x04, "Copyright 2026 the VizDAW tests"),
            &pk(ump::key_sig(2, false)),
        ]
    );
    // Every tempo and meter change inside the range, at its offset.
    for k in 1..16 {
        assert!(matches!(
            at(k * 3_840)[..],
            [ClipEvent::Packet(p)] if matches!(ump::decode(p), Decoded::SetTempo(_))
        ));
    }
    assert_eq!(
        at(BAR),
        vec![
            &pk(ump::set_tempo(42_857_143)),
            &pk(ump::time_sig(3, 2, 8)),
            &txt(0x00, "marker: Verse"),
        ]
    );
    assert_eq!(
        at(100),
        vec![
            &pk(ump::note_on(
                0,
                2,
                60,
                0x1234,
                Some(ump::NoteAttribute {
                    kind: 3,
                    data: 0x7800
                })
            )),
            &pk(ump::per_note_registered(0, 2, 60, 74, 0x9000_0000)),
        ]
    );
    assert_eq!(
        at(600),
        vec![&pk(ump::per_note_pitch_bend(0, 2, 60, 0x8100_0000))]
    );
    assert_eq!(at(1_100), vec![&pk(ump::note_off(0, 2, 60, 0x0400, None))]);
    assert_eq!(
        at(2_000),
        vec![&pk(ump::control_change(0, 2, 74, 0x1234_5678))]
    );
    assert_eq!(at(BAR + Q), vec![&pk(ump::note_on(0, 2, 62, 0xFFFF, None))]);
    let note_ons = c
        .events
        .iter()
        .filter(|(_, e)| matches!(e, ClipEvent::Packet(p) if matches!(ump::decode(p), Decoded::NoteOn { .. })))
        .count();
    assert_eq!(note_ons, 2, "only events starting inside the range");
    lacks(&r, "rounded");
    lacks(&r, "Note attributes");
    golden("export_clip.midi2", &clip::write(&c));
}

#[test]
fn clip_texts_are_cut_to_384_bytes() {
    let (mut d, bass) = small_doc();
    let long = "é".repeat(300);
    d.put(bass, vec![text(Q, TextKind::Text, &long)]);
    let (c, r) = to_clip(
        &d.doc,
        &ClipOptions {
            seq: d.seq,
            lane: bass,
            range: None,
        },
    )
    .unwrap();
    let cut = c
        .events
        .iter()
        .find_map(|(_, e)| match e {
            ClipEvent::Text {
                status: 0, text, ..
            } => Some(text.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(cut.len(), 384);
    assert_eq!(cut, "é".repeat(192));
    assert_eq!(has(&r, ItemKind::Changed, "Texts over 384 bytes cut"), 1);
}

#[test]
fn far_apart_events_are_left_out() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0);
    let lane = d.track("Bass", Some(dest));
    // 0x0FFF_FFFF + 1 file ticks at 15 360 per quarter.
    let far = 0x1000_0000i64;
    d.put(
        lane,
        vec![
            note(0, 60, up7(100), Q),
            ev(
                far + Q,
                EventBody::Cc {
                    index: 1,
                    value: up32(1, 7),
                },
            ),
        ],
    );
    let (f, r) = d.smf(15_360);
    assert_eq!(
        has(
            &r,
            ItemKind::LeftOut,
            "Events too far apart for a MIDI file"
        ),
        1
    );
    smf::write(&f).unwrap();
    let (f, r) = d.smf(960);
    lacks(&r, "too far apart");
    assert_eq!(channel_msgs(&f, 1).len(), 3);
}
