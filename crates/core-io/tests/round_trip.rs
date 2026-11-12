//! Export → import round trips (plan 6a Task 6): what each route carries
//! comes back.

use core_engine::input_fx::{CENTER, up7, up32};
use core_engine::mpe::{bend_in, bend_out};
use core_io::clip;
use core_io::limits::Limits;
use core_io::midi_in::{ImportOptions, MidiImport, Placement, apply, parse};
use core_io::midi_out::{ClipOptions, SmfOptions, to_clip, to_smf};
use core_io::report::{ItemKind, Report};
use core_io::smf;
use core_model::command::{
    Command, DrumCmd, EventCmd, LaneCmd, RoutingCmd, SequenceCmd, apply as run,
};
use core_model::document::{DestKind, Document};
use core_model::event::{
    EventBody, EventSpec, LoopCount, NoteAttribute, PerNoteCtl, PerNoteKind, TextKind,
};
use core_model::history::History;
use core_model::ids::{AnyId, DestId, LaneId, SeqId};
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::mixer::BUILTIN_SYNTH_REF;
use core_model::mpe::MpeConfig;
use core_model::query::flatten;
use core_model::selection::ViewFilter;
use core_model::sequence::{Marker, SeqLength};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

const Q: i64 = PPQ;
const BAR: i64 = 4 * Q;
/// The sequences' locked length: past every generated event.
const LENGTH: i64 = 64 * Q;

// ---- building documents ----

struct Doc {
    doc: Document,
    seq: SeqId,
}

fn created<T>(ids: &[AnyId], f: impl Fn(&AnyId) -> Option<T>) -> T {
    ids.iter().find_map(f).unwrap()
}

impl Doc {
    fn new(name: &str) -> Doc {
        let a = run(
            &Document::new(),
            &Command::Sequence(SequenceCmd::New { name: name.into() }),
        )
        .unwrap();
        let seq = created(&a.report.created, |id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        });
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

    fn sequence(&mut self, name: &str) -> (SeqId, LaneId) {
        let ids = self.exec(Command::Sequence(SequenceCmd::New { name: name.into() }));
        let seq = created(&ids, |id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        });
        let lane = self.doc.sequence(seq).unwrap().lanes.front().unwrap().id;
        (seq, lane)
    }

    fn builtin(&mut self, group: u8, channel: u8) -> DestId {
        let ids = self.exec(Command::Routing(RoutingCmd::AddDestination {
            name: format!("Built-in Synth (ch {})", channel + 1),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: BUILTIN_SYNTH_REF.into(),
                group,
                channel,
            },
            default_patch: None,
        }));
        created(&ids, |id| match id {
            AnyId::Dest(d) => Some(*d),
            _ => None,
        })
    }

    fn track(&mut self, name: &str, dest: DestId) -> LaneId {
        let seq = self.seq;
        let ids = self.exec(Command::Lane(LaneCmd::Add {
            seq,
            name: name.into(),
            index: None,
        }));
        let lane = created(&ids, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        });
        self.exec(Command::Lane(LaneCmd::SetDest {
            seq,
            lane,
            dest: Some(dest),
        }));
        lane
    }

    fn put_in(&mut self, seq: SeqId, lane: LaneId, events: Vec<EventSpec>) {
        if !events.is_empty() {
            self.exec(Command::Event(EventCmd::Insert { seq, lane, events }));
        }
    }

    fn put(&mut self, lane: LaneId, events: Vec<EventSpec>) {
        self.put_in(self.seq, lane, events);
    }

    fn smf(&self, ppq: u16, mpe: bool) -> (Vec<u8>, Report) {
        let (f, r) = to_smf(
            &self.doc,
            &SmfOptions {
                seq: self.seq,
                lanes: vec![],
                type0: false,
                ppq,
                mpe,
            },
        )
        .unwrap();
        (smf::write(&f).unwrap(), r)
    }
}

fn note(tick: i64, pitch: u8, velocity: u16, duration: i64) -> EventSpec {
    EventSpec::new(Tick(tick), EventBody::note(pitch, velocity, Tick(duration)))
}

fn ev(tick: i64, body: EventBody) -> EventSpec {
    EventSpec::new(Tick(tick), body)
}

fn import(bytes: &[u8]) -> MidiImport {
    parse(
        bytes,
        &ImportOptions::default(),
        &Limits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
}

/// `bytes` imported as a new project; the history and its tracks.
fn as_new_project(bytes: &[u8]) -> (History, SeqId, Vec<LaneId>, Report) {
    let imp = import(bytes);
    let d = Doc::new("Imported");
    let mut h = History::new(d.doc);
    let mut report = Report::default();
    h.begin("Import MIDI File").unwrap();
    let lanes = apply(
        &mut h,
        &imp,
        &Placement {
            seq: d.seq,
            lane: None,
            tick: Tick(0),
            channel_map: BTreeMap::new(),
            use_tempo: true,
            new_project: true,
        },
        &mut report,
    )
    .unwrap();
    h.commit().unwrap();
    (h, d.seq, lanes, report)
}

/// Every event of the active take as `(tick, body)`, sorted.
fn events_of(doc: &Document, seq: SeqId, lane: LaneId) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = doc
        .lane(seq, lane)
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .map(|e| (e.tick.0, format!("{:?}", e.body)))
        .collect();
    out.sort();
    out
}

/// `(tick, pitch, duration)` of the notes `flatten` plays on `lane`.
fn played_notes(doc: &Document, seq: SeqId, lane: LaneId) -> Vec<(i64, u8, i64)> {
    let mut out: Vec<(i64, u8, i64)> = flatten(doc, seq, None, &ViewFilter::default())
        .unwrap()
        .filter(|r| r.path[0].lane == lane)
        .filter_map(|r| match r.body {
            EventBody::Note {
                pitch, duration, ..
            } => Some((r.tick.0, pitch, duration.0)),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

fn notes_of(doc: &Document, seq: SeqId, lane: LaneId) -> Vec<(i64, u8, i64)> {
    let mut out: Vec<(i64, u8, i64)> = doc
        .lane(seq, lane)
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Note {
                pitch, duration, ..
            } => Some((e.tick.0, pitch, duration.0)),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

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

fn cases(n: u32) -> u32 {
    std::env::var("VIZDAW_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(n)
}

// ---- the generator ----

#[derive(Debug, Clone)]
enum Kind {
    Note { pitch: u8, vel: u8, dur: i64 },
    Cc { index: u8, value: u8 },
    Bend(u16),
    Pressure(u8),
    Poly { pitch: u8, value: u8 },
    Program { program: u8, bank: Option<(u8, u8)> },
    SysEx(Vec<u8>),
    Text(TextKind, String),
}

#[derive(Debug, Clone)]
struct TrackSpec {
    name: String,
    channel: u8,
    events: Vec<(i64, Kind)>,
}

#[derive(Debug, Clone)]
struct Spec {
    meter: (u8, u8, i8, bool),
    change: Option<(i64, u8, u8)>,
    tempo: Vec<(i64, u32, bool)>,
    pickup_beats: u8,
    markers: Vec<(i64, String)>,
    tracks: Vec<TrackSpec>,
}

/// Printable words that pass `valid_name`.
fn word() -> impl Strategy<Value = String> {
    "[A-Z][a-z]{1,6}( [a-z]{1,6}){0,2}"
}

fn kind() -> impl Strategy<Value = Kind> {
    // No free CC 0 or 32 (banks come only with programs, F7) and no RPN
    // or data entry (an MPE Configuration Message would decode as MPE).
    let index =
        (1u8..120).prop_filter("plain controller", |i| !matches!(i, 6 | 32 | 38 | 98..=101));
    prop_oneof![
        4 => (0u8..128, 1u8..128, 1i64..4 * Q).prop_map(|(pitch, vel, dur)| Kind::Note { pitch, vel, dur }),
        1 => (index, 0u8..128).prop_map(|(index, value)| Kind::Cc { index, value }),
        1 => (0u16..16_384).prop_map(Kind::Bend),
        1 => (0u8..128).prop_map(Kind::Pressure),
        1 => (0u8..128, 0u8..128).prop_map(|(pitch, value)| Kind::Poly { pitch, value }),
        1 => (0u8..128, proptest::option::of((0u8..128, 0u8..128)))
            .prop_map(|(program, bank)| Kind::Program { program, bank }),
        1 => proptest::collection::vec(0u8..128, 1..8).prop_map(Kind::SysEx),
        1 => (
            prop_oneof![
                Just(TextKind::Text),
                Just(TextKind::Lyric),
                Just(TextKind::Cue),
                Just(TextKind::Copyright)
            ],
            word()
        )
            .prop_map(|(k, s)| Kind::Text(k, s)),
    ]
}

fn spec() -> impl Strategy<Value = Spec> {
    let meter = (
        2u8..8,
        prop_oneof![Just(4u8), Just(8u8)],
        -7i8..=7,
        any::<bool>(),
    );
    (
        meter,
        proptest::option::of((1i64..5, 2u8..8, prop_oneof![Just(4u8), Just(8u8)])),
        (
            30u32..=240,
            proptest::collection::vec((1i64..32 * Q, 30u32..=240, any::<bool>()), 0..4),
        ),
        any::<bool>(),
        0u8..8,
        proptest::collection::vec((0i64..24 * Q, word()), 0..3),
        proptest::sample::subsequence((0u8..16).collect::<Vec<_>>(), 1..=4).prop_shuffle(),
        proptest::collection::vec(
            (
                word(),
                proptest::collection::vec((0i64..24 * Q, kind()), 1..24),
            ),
            4,
        ),
    )
        .prop_map(
            |(meter, change, (bpm0, points), first_ramp, pickup, markers, channels, tracks)| {
                let pickup_beats = pickup % meter.0;
                let beat = 4 * Q / i64::from(meter.1);
                let pickup_ticks = i64::from(pickup_beats) * beat;
                let bar = beat * i64::from(meter.0);
                // A longer bar 2 after bar 1 (no pickup) reads back as a
                // pickup (Ruling 8); the property checks that shift.
                let change = change
                    .filter(|(_, num, den)| (*num, *den) != (meter.0, meter.1))
                    .map(|(k, num, den)| (k * bar, num, den));
                let mut tempo = vec![(0, bpm0, first_ramp)];
                let mut points = points;
                points.sort_by_key(|p| p.0);
                points.dedup_by_key(|p| p.0);
                tempo.extend(points);
                let mut markers: Vec<(i64, String)> = markers
                    .into_iter()
                    .map(|(t, n)| (t - pickup_ticks, n))
                    .collect();
                markers.sort_by_key(|m| m.0);
                markers.dedup_by_key(|m| m.0);
                let tracks = channels
                    .into_iter()
                    .zip(tracks)
                    .enumerate()
                    .map(|(i, (channel, (name, events)))| {
                        let mut events: Vec<(i64, Kind)> = events
                            .into_iter()
                            .map(|(t, k)| (t - pickup_ticks, k))
                            .collect();
                        // At least one note; no two notes of a pitch overlap.
                        events.push((
                            0,
                            Kind::Note {
                                pitch: 60,
                                vel: 100,
                                dur: Q,
                            },
                        ));
                        events.sort_by_key(|e| e.0);
                        let mut free: BTreeMap<u8, i64> = BTreeMap::new();
                        events.retain(|(t, k)| match k {
                            Kind::Note { pitch, dur, .. } => {
                                if free.get(pitch).is_some_and(|f| *f > *t) {
                                    return false;
                                }
                                free.insert(*pitch, t + dur);
                                true
                            }
                            _ => true,
                        });
                        TrackSpec {
                            name: format!("{name} {i}"),
                            channel,
                            events,
                        }
                    })
                    .collect();
                Spec {
                    meter,
                    change,
                    tempo,
                    pickup_beats,
                    markers,
                    tracks,
                }
            },
        )
}

fn body(k: &Kind) -> EventBody {
    match k {
        Kind::Note { pitch, vel, dur } => EventBody::note(*pitch, up7(*vel), Tick(*dur)),
        Kind::Cc { index, value } => EventBody::Cc {
            index: *index,
            value: up32(u32::from(*value), 7),
        },
        Kind::Bend(v) => EventBody::PitchBend {
            value: up32(u32::from(*v), 14),
        },
        Kind::Pressure(v) => EventBody::ChannelPressure {
            value: up32(u32::from(*v), 7),
        },
        Kind::Poly { pitch, value } => EventBody::PolyPressure {
            pitch: *pitch,
            value: up32(u32::from(*value), 7),
        },
        Kind::Program { program, bank } => EventBody::Program {
            program: *program,
            bank: *bank,
        },
        Kind::SysEx(data) => EventBody::SysEx {
            data: Arc::from(data.as_slice()),
            eight_bit: false,
        },
        Kind::Text(kind, text) => EventBody::Text {
            kind: *kind,
            text: text.clone(),
        },
    }
}

fn build(s: &Spec) -> (Doc, Vec<LaneId>) {
    let mut d = Doc::new("Song");
    let seq = d.seq;
    let (num, den, sharps, minor) = s.meter;
    let key = KeySig { sharps, minor };
    let mut entries = vec![MeterEntry {
        tick: Tick(0),
        meter: Meter { num, den },
        key,
    }];
    if let Some((t, num, den)) = s.change {
        entries.push(MeterEntry {
            tick: Tick(t),
            meter: Meter { num, den },
            key,
        });
    }
    let meter = MeterMap::new(entries).unwrap();
    let beat = meter.entries()[0].meter.beat_ticks();
    d.exec(Command::Sequence(SequenceCmd::SetMeter { seq, meter }));
    let tempo = TempoMap::new(
        s.tempo
            .iter()
            .map(|&(t, bpm, ramp)| TempoPoint {
                tick: Tick(t),
                bpm: Bpm::whole(bpm),
                ramp,
            })
            .collect(),
    )
    .unwrap();
    d.exec(Command::Sequence(SequenceCmd::SetTempo { seq, tempo }));
    d.exec(Command::Sequence(SequenceCmd::SetStartOffset {
        seq,
        offset: Tick(i64::from(s.pickup_beats) * beat),
    }));
    d.exec(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(LENGTH)),
    }));
    if !s.markers.is_empty() {
        let markers = s
            .markers
            .iter()
            .map(|(t, n)| Marker {
                tick: Tick(*t),
                name: n.clone(),
            })
            .collect();
        d.exec(Command::Sequence(SequenceCmd::SetMarkers { seq, markers }));
    }
    let mut lanes = Vec::new();
    for t in &s.tracks {
        let dest = d.builtin(0, t.channel);
        let lane = d.track(&t.name, dest);
        d.put(
            lane,
            t.events
                .iter()
                .map(|(tick, k)| ev(*tick, body(k)))
                .collect(),
        );
        lanes.push(lane);
    }
    (d, lanes)
}

/// Bar 1's length when it is shorter than bar 2 and there is no pickup.
fn short_first_bar(s: &core_model::sequence::Sequence) -> Option<i64> {
    let e = s.meter.entries();
    let (a, b) = (e.front()?, e.get(1)?);
    let bar = a.meter.bar_ticks();
    (s.start_offset.0 == 0 && b.tick.0 == bar && bar < b.meter.bar_ticks()).then_some(bar)
}

/// `points` (by tick) moved back by `by`, the last at or before 0 becoming
/// the one at 0, as import removes a pickup.
fn collapse<T: Copy>(points: Vec<(i64, T)>, by: i64, at: impl Fn(T, i64) -> T) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for (t, p) in points {
        let t = t - by;
        if t <= 0 {
            out.clear();
            out.push(at(p, 0));
        } else {
            out.push(at(p, t));
        }
    }
    out
}

/// The original tempo map with each ramp replaced by its steps (one every
/// 3 840 ticks, each as long in time as the ramp's stretch), as
/// `(tick, bpm, is_step)`.
fn steps_of(map: &TempoMap) -> Vec<(i64, f64, bool)> {
    let pts: Vec<TempoPoint> = map.points().iter().copied().collect();
    let mut out = Vec::new();
    for (i, p) in pts.iter().enumerate() {
        match pts.get(i + 1) {
            Some(next) if p.ramp => {
                let mut t = p.tick.0;
                while t < next.tick.0 {
                    let end = (t + 3_840).min(next.tick.0);
                    let secs = map.tick_to_seconds(Tick(end)) - map.tick_to_seconds(Tick(t));
                    let bpm = 60.0 * (end - t) as f64 / (PPQ as f64 * secs);
                    out.push((t, bpm, true));
                    t = end;
                }
            }
            _ => out.push((p.tick.0, p.bpm.as_f64(), false)),
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(64)))]

    #[test]
    fn smf_round_trip_keeps_what_the_route_carries(s in spec()) {
        let (d, lanes) = build(&s);
        let original = d.doc.sequence(d.seq).unwrap().clone();

        let (bytes, report) = d.smf(15_360, false);
        prop_assert!(
            !report.items.iter().any(|i| i.kind == ItemKind::LeftOut),
            "{:?}", report.items
        );
        // A short bar 1 without a pickup reads back as one (Ruling 8): the
        // export says so, and everything moves back by bar 1.
        let misread = short_first_bar(&original);
        prop_assert_eq!(
            report.items.iter().any(|i| i.what.contains("may read it as a pickup")),
            misread.is_some()
        );
        let by = misread.unwrap_or(0);
        let (h, seq, back, r) = as_new_project(&bytes);
        prop_assert_eq!(
            r.items.iter().any(|i| i.what == "First bar read as a pickup"),
            by > 0 || original.start_offset.0 > 0
        );
        let got = h.doc().sequence(seq).unwrap();
        prop_assert_eq!(back.len(), lanes.len());
        for (a, b) in lanes.iter().zip(&back) {
            let want: Vec<(i64, String)> = events_of(&d.doc, d.seq, *a)
                .into_iter()
                .map(|(t, b)| (t - by, b))
                .collect();
            prop_assert_eq!(want, events_of(h.doc(), seq, *b));
        }
        let want_meter = MeterMap::new(collapse(
            original.meter.entries().iter().map(|e| (e.tick.0, *e)).collect(),
            by,
            |e, t| MeterEntry { tick: Tick(t), ..e },
        ))
        .unwrap();
        prop_assert_eq!(&got.meter, &want_meter);
        let want_markers: Vec<Marker> = original
            .markers
            .iter()
            .map(|m| Marker { tick: Tick(m.tick.0 - by), name: m.name.clone() })
            .collect();
        prop_assert_eq!(got.markers.iter().cloned().collect::<Vec<_>>(), want_markers);
        prop_assert_eq!(got.start_offset.0, original.start_offset.0 + by);
        let want = collapse(
            steps_of(&original.tempo).into_iter().map(|p| (p.0, p)).collect(),
            by,
            |p, t| (t, p.1, p.2),
        );
        let have: Vec<TempoPoint> = got.tempo.points().iter().copied().collect();
        prop_assert_eq!(have.len(), want.len(), "{:?} vs {:?}", have, want);
        for (h, (t, bpm, step)) in have.iter().zip(&want) {
            prop_assert_eq!(h.tick.0, *t);
            prop_assert!(!h.ramp);
            if *step {
                prop_assert!((h.bpm.as_f64() - bpm).abs() <= bpm * 0.001, "{:?} vs {}", h, bpm);
            } else {
                prop_assert_eq!(h.bpm.as_f64(), *bpm);
            }
        }

        // At 960 per quarter every position is within half the grid step,
        // and the report says so exactly when one moved.
        let shift = original.start_offset.0;
        let mut positions: Vec<i64> = s.markers.iter().map(|m| m.0).collect();
        positions.extend(s.tempo.iter().map(|p| p.0));
        for t in &s.tracks {
            for (tick, k) in &t.events {
                positions.push(*tick);
                if let Kind::Note { dur, .. } = k {
                    positions.push(tick + dur);
                }
            }
        }
        let moved = positions.iter().any(|p| (p + shift) % 16 != 0);
        let (bytes, report) = d.smf(960, false);
        let rounded = report.items.iter().any(|i| {
            i.kind == ItemKind::Changed && i.what == "Positions rounded to 960 ticks per quarter"
        });
        prop_assert_eq!(rounded, moved);
        let (h, seq, back, _) = as_new_project(&bytes);
        for (a, b) in lanes.iter().zip(&back) {
            let mut want: Vec<(i64, u8, i64)> = notes_of(&d.doc, d.seq, *a)
                .into_iter()
                .map(|n| (n.0 - by, n.1, n.2))
                .collect();
            let mut have = notes_of(h.doc(), seq, *b);
            want.sort_by_key(|n| (n.1, n.0));
            have.sort_by_key(|n| (n.1, n.0));
            prop_assert_eq!(want.len(), have.len());
            for (w, g) in want.iter().zip(&have) {
                prop_assert_eq!(w.1, g.1);
                prop_assert!((w.0 - g.0).abs() <= 8, "{:?} vs {:?}", w, g);
                prop_assert!(((w.0 + w.2) - (g.0 + g.2)).abs() <= 8, "{:?} vs {:?}", w, g);
            }
        }
    }

    #[test]
    fn clip_round_trip_is_exact(
        group in 0u8..16,
        channel in 0u8..16,
        events in proptest::collection::vec((0i64..16 * Q, clip_kind()), 1..32),
    ) {
        let mut d = Doc::new("Song");
        let dest = d.builtin(group, channel);
        let lane = d.track("Lead", dest);
        let seq = d.seq;
        d.exec(Command::Sequence(SequenceCmd::SetLength {
            seq,
            length: SeqLength::Locked(Tick(LENGTH)),
        }));
        let mut events = events;
        events.sort_by_key(|e| e.0);
        let mut free: BTreeMap<u8, i64> = BTreeMap::new();
        events.retain(|(t, b)| match b {
            EventBody::Note { pitch, duration, .. } => {
                if free.get(pitch).is_some_and(|f| *f > *t) {
                    return false;
                }
                free.insert(*pitch, t + duration.0);
                true
            }
            // A copyright at tick 0 before any note is the song's.
            EventBody::Text { kind: TextKind::Copyright, .. } => *t > 0,
            _ => true,
        });
        d.put(lane, events.into_iter().map(|(t, b)| ev(t, b)).collect());
        let (c, report) = to_clip(&d.doc, &ClipOptions { seq: d.seq, lane, range: None }).unwrap();
        prop_assert!(report.is_empty(), "{:?}", report.items);
        let imp = import(&clip::write(&c));
        prop_assert_eq!(imp.tracks.len(), 1);
        let mut have: Vec<(i64, String)> = imp.tracks[0]
            .events
            .iter()
            .map(|e| (e.tick.0, format!("{:?}", e.body)))
            .collect();
        have.sort();
        prop_assert_eq!(events_of(&d.doc, d.seq, lane), have);
        let routed = imp.tracks[0]
            .events
            .iter()
            .filter(|e| e.body.is_midi() && !matches!(e.body, EventBody::SysEx { .. }))
            .all(|e| e.group_channel == Some((group, channel)));
        prop_assert!(routed);
    }
}

fn clip_kind() -> impl Strategy<Value = EventBody> {
    let per_note = (
        prop_oneof![
            (0u8..128).prop_map(PerNoteKind::Registered),
            (0u8..128).prop_map(PerNoteKind::Assignable),
            Just(PerNoteKind::PitchBend),
        ],
        0i64..1_000,
        any::<u32>(),
    );
    let note = (
        0u8..128,
        1u16..=u16::MAX,
        any::<u16>(),
        1_000i64..4 * Q,
        proptest::option::of((1u8..4, any::<u16>())),
        proptest::collection::vec(per_note, 0..4),
    )
        .prop_map(|(pitch, velocity, release, dur, attr, mut pn)| {
            pn.sort_by_key(|p| p.1);
            EventBody::Note {
                pitch,
                velocity,
                release_velocity: release,
                duration: Tick(dur),
                attribute: attr.map(|(kind, data)| NoteAttribute { kind, data }),
                per_note: pn
                    .into_iter()
                    .map(|(kind, offset, value)| PerNoteCtl {
                        kind,
                        offset: Tick(offset),
                        value,
                    })
                    .collect(),
            }
        });
    prop_oneof![
        4 => note,
        1 => (0u8..128, any::<u32>()).prop_map(|(index, value)| EventBody::Cc { index, value }),
        1 => any::<u32>().prop_map(|value| EventBody::PitchBend { value }),
        1 => any::<u32>().prop_map(|value| EventBody::ChannelPressure { value }),
        1 => (0u8..128, any::<u32>()).prop_map(|(pitch, value)| EventBody::PolyPressure { pitch, value }),
        1 => (0u8..128, proptest::option::of((0u8..128, 0u8..128)))
            .prop_map(|(program, bank)| EventBody::Program { program, bank }),
        1 => proptest::collection::vec(0u8..128, 1..20).prop_map(|d| EventBody::SysEx {
            data: Arc::from(d.as_slice()),
            eight_bit: false,
        }),
        1 => proptest::collection::vec(any::<u8>(), 1..30).prop_map(|d| EventBody::SysEx {
            data: Arc::from(d.as_slice()),
            eight_bit: true,
        }),
        1 => (
            prop_oneof![
                Just(TextKind::Text),
                Just(TextKind::Lyric),
                Just(TextKind::Cue),
                Just(TextKind::Copyright)
            ],
            word()
        )
            .prop_map(|(kind, text)| EventBody::Text { kind, text }),
    ]
}

// ---- fixed cases ----

#[test]
fn nested_references_play_as_written() {
    let mut d = Doc::new("Song");
    let (riff, riff_lane) = d.sequence("Riff");
    d.put_in(
        riff,
        riff_lane,
        vec![note(0, 60, up7(100), Q), note(Q, 64, up7(100), Q)],
    );
    let (part, part_lane) = d.sequence("Part");
    let reference = |seq, tick, transpose| {
        ev(
            tick,
            EventBody::Reference {
                seq,
                length: None,
                loop_count: LoopCount::Count(2),
                transpose,
            },
        )
    };
    d.put_in(part, part_lane, vec![reference(riff, 0, 3)]);
    let dest = d.builtin(0, 0);
    let lane = d.track("Bass", dest);
    d.put(lane, vec![reference(part, BAR, -2)]);
    let (bytes, _) = d.smf(15_360, false);
    let (h, seq, back, _) = as_new_project(&bytes);
    let want = played_notes(&d.doc, d.seq, lane);
    assert_eq!(want.len(), 8);
    assert!(want.iter().all(|n| n.1 == 61 || n.1 == 65));
    assert_eq!(notes_of(h.doc(), seq, back[0]), want);
}

#[test]
fn only_the_active_take_is_written() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0, 0);
    let lane = d.track("Bass", dest);
    d.put(lane, vec![note(0, 60, up7(100), Q)]);
    let seq = d.seq;
    d.exec(Command::Lane(LaneCmd::AddTake {
        seq,
        lane,
        name: "Take 2".into(),
    }));
    d.put(lane, vec![note(Q, 62, up7(100), Q)]);
    let (bytes, _) = d.smf(960, false);
    let (h, seq, back, _) = as_new_project(&bytes);
    assert_eq!(notes_of(h.doc(), seq, back[0]), vec![(Q, 62, Q)]);
}

#[test]
fn an_mpe_track_round_trips() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0, 0);
    d.exec(Command::Routing(RoutingCmd::SetMpe {
        dest,
        mpe: Some(MpeConfig::default()),
    }));
    let lane = d.track("Lead", dest);
    let expressive = |tick: i64, pitch: u8, bend: u32| {
        let mut n = note(tick, pitch, up7(100), 2 * Q);
        if let EventBody::Note { per_note, .. } = &mut n.body {
            per_note.push_back(PerNoteCtl {
                kind: PerNoteKind::PitchBend,
                offset: Tick(0),
                value: bend,
            });
            per_note.push_back(PerNoteCtl {
                kind: PerNoteKind::Registered(74),
                offset: Tick(Q),
                value: up32(100, 7),
            });
        }
        n
    };
    // A value the 14-bit member bend carries exactly.
    let bend = bend_in(bend_out(CENTER + 0x0200_0000, 48), 48);
    d.put(
        lane,
        vec![
            expressive(0, 60, bend),
            expressive(0, 64, bend),
            expressive(Q, 67, bend),
        ],
    );
    let (bytes, report) = d.smf(15_360, true);
    assert!(report.is_empty(), "{:?}", report.items);
    let (h, seq, back, _) = as_new_project(&bytes);
    assert_eq!(
        h.doc().lane(seq, back[0]).unwrap().mpe,
        Some(MpeConfig::default())
    );
    let mut got: Vec<(i64, u8, Vec<PerNoteCtl>)> = h
        .doc()
        .lane(seq, back[0])
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match &e.body {
            EventBody::Note {
                pitch, per_note, ..
            } => Some((e.tick.0, *pitch, per_note.iter().copied().collect())),
            _ => None,
        })
        .collect();
    got.sort_by_key(|n| (n.0, n.1));
    assert_eq!(got.len(), 3);
    for (_, _, per_note) in &got {
        assert_eq!(
            per_note,
            &vec![
                PerNoteCtl {
                    kind: PerNoteKind::PitchBend,
                    offset: Tick(0),
                    value: bend
                },
                PerNoteCtl {
                    kind: PerNoteKind::Registered(74),
                    offset: Tick(Q),
                    value: up32(100, 7)
                },
            ]
        );
    }
}

#[test]
fn a_drum_track_round_trips() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0, 9);
    let lane = d.track("Kit", dest);
    d.put(
        lane,
        vec![
            note(0, 36, up7(100), Q / 4),
            note(0, 42, up7(60), Q / 4),
            note(Q, 38, up7(100), Q / 4),
        ],
    );
    let seq = d.seq;
    d.exec(Command::Drum(DrumCmd::MarkDrumLane {
        seq,
        lane,
        drum: true,
    }));
    let (bytes, report) = d.smf(960, false);
    has(&report, ItemKind::LeftOut, "Drum row names");
    let (h, seq2, back, _) = as_new_project(&bytes);
    assert!(h.doc().lane(seq2, back[0]).unwrap().drum.is_some());
    assert_eq!(
        events_of(&d.doc, seq, lane),
        events_of(h.doc(), seq2, back[0])
    );
}

#[test]
fn a_ramp_round_trips_as_steps() {
    let mut d = Doc::new("Song");
    let seq = d.seq;
    let tempo = TempoMap::new(vec![
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(100),
            ramp: false,
        },
        TempoPoint {
            tick: Tick(BAR),
            bpm: Bpm::whole(100),
            ramp: true,
        },
        TempoPoint {
            tick: Tick(2 * BAR),
            bpm: Bpm::whole(150),
            ramp: false,
        },
    ])
    .unwrap();
    d.exec(Command::Sequence(SequenceCmd::SetTempo {
        seq,
        tempo: tempo.clone(),
    }));
    let dest = d.builtin(0, 0);
    let lane = d.track("Bass", dest);
    d.put(lane, vec![note(0, 60, up7(100), 3 * BAR)]);
    let (bytes, report) = d.smf(960, false);
    assert_eq!(
        has(
            &report,
            ItemKind::Changed,
            "Tempo ramp at bar 2 written as 16 steps"
        ),
        1
    );
    let (h, seq, _, _) = as_new_project(&bytes);
    let got = &h.doc().sequence(seq).unwrap().tempo;
    assert_eq!(got.points().len(), 18);
    // The steps keep the ramp's time: the end of bar 3 is where it was.
    let t = |m: &TempoMap| m.tick_to_seconds(Tick(3 * BAR));
    assert!(
        (t(got) - t(&tempo)).abs() < 1e-3,
        "{} vs {}",
        t(got),
        t(&tempo)
    );
}

#[test]
fn a_pickup_of_one_beat_round_trips() {
    let mut d = Doc::new("Song");
    let seq = d.seq;
    d.exec(Command::Sequence(SequenceCmd::SetStartOffset {
        seq,
        offset: Tick(Q),
    }));
    d.exec(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: vec![Marker {
            tick: Tick(-Q),
            name: "Pickup".into(),
        }]
        .into_iter()
        .collect(),
    }));
    let dest = d.builtin(0, 0);
    let lane = d.track("Bass", dest);
    d.put(
        lane,
        vec![note(-Q, 55, up7(100), Q), note(0, 60, up7(100), Q)],
    );
    let (bytes, report) = d.smf(960, false);
    assert!(report.is_empty(), "{:?}", report.items);
    let f = smf::read(&bytes, &Limits::default(), &AtomicBool::new(false))
        .unwrap()
        .0;
    // A first bar of one beat: 1/4.
    assert!(f.tracks[0].events.contains(&(
        0,
        smf::SmfEvent::Meta {
            kind: 0x58,
            data: vec![1, 2, 24, 8]
        }
    )));
    let (h, seq2, back, _) = as_new_project(&bytes);
    let got = h.doc().sequence(seq2).unwrap();
    assert_eq!(got.start_offset, Tick(Q));
    assert_eq!(got.meter, MeterMap::default());
    assert_eq!(got.markers[0].tick, Tick(-Q));
    assert_eq!(
        notes_of(h.doc(), seq2, back[0]),
        vec![(-Q, 55, Q), (0, 60, Q)]
    );
}

#[test]
fn a_thousand_tracks_finish_quickly() {
    let mut d = Doc::new("Song");
    let dests: Vec<DestId> = (0..16).map(|c| d.builtin(0, c)).collect();
    let seq = d.seq;
    let mut lanes = Vec::new();
    for i in 0..1_000usize {
        let ids = d.exec(Command::Lane(LaneCmd::Add {
            seq,
            name: format!("Track {i}"),
            index: None,
        }));
        let lane = created(&ids, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        });
        lanes.push(lane);
    }
    for (i, lane) in lanes.iter().enumerate() {
        d.exec(Command::Lane(LaneCmd::SetDest {
            seq,
            lane: *lane,
            dest: Some(dests[i % 16]),
        }));
        d.put(*lane, vec![note(i as i64 * 10, 60, up7(100), Q)]);
    }
    let start = std::time::Instant::now();
    let (bytes, _) = d.smf(960, false);
    let imp = import(&bytes);
    let elapsed = start.elapsed();
    assert_eq!(imp.tracks.len(), 1_000);
    assert!(elapsed.as_secs_f64() < 10.0, "{elapsed:?}");
}

#[test]
fn nested_same_pitch_notes_re_pair_first_on_first_off() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0, 0);
    let lane = d.track("Bass", dest);
    d.put(
        lane,
        vec![note(0, 60, up7(100), 1_000), note(100, 60, up7(90), 500)],
    );
    let (bytes, report) = d.smf(15_360, false);
    assert!(report.is_empty(), "{:?}", report.items);
    let (h, seq, back, r) = as_new_project(&bytes);
    // On at 0, on at 100, off at 600, off at 1 000.
    assert_eq!(
        notes_of(h.doc(), seq, back[0]),
        vec![(0, 60, 600), (100, 60, 900)]
    );
    assert!(r.items.is_empty(), "{:?}", r.items);
}

#[test]
fn zero_length_notes_are_left_out() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0, 0);
    let lane = d.track("Bass", dest);
    d.put(
        lane,
        vec![note(0, 60, up7(100), 0), note(Q, 62, up7(100), Q)],
    );
    for ppq in [960, 15_360] {
        let (bytes, report) = d.smf(ppq, false);
        assert_eq!(has(&report, ItemKind::LeftOut, "Notes of zero length"), 1);
        let (h, seq, back, _) = as_new_project(&bytes);
        assert_eq!(notes_of(h.doc(), seq, back[0]), vec![(Q, 62, Q)]);
    }
    let (c, report) = to_clip(
        &d.doc,
        &ClipOptions {
            seq: d.seq,
            lane,
            range: None,
        },
    )
    .unwrap();
    assert_eq!(has(&report, ItemKind::LeftOut, "Notes of zero length"), 1);
    let imp = import(&clip::write(&c));
    assert_eq!(imp.tracks[0].events.len(), 1);
}

#[test]
fn free_bank_selects_merge_into_the_program() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0, 0);
    let lane = d.track("Bass", dest);
    d.put(
        lane,
        vec![
            ev(
                Q,
                EventBody::Cc {
                    index: 0,
                    value: up32(1, 7),
                },
            ),
            ev(
                Q,
                EventBody::Cc {
                    index: 32,
                    value: up32(2, 7),
                },
            ),
            ev(
                Q,
                EventBody::Program {
                    program: 5,
                    bank: None,
                },
            ),
            note(Q, 60, up7(100), Q),
        ],
    );
    let (bytes, report) = d.smf(960, false);
    assert_eq!(
        has(
            &report,
            ItemKind::Changed,
            "Bank selects merged into program changes"
        ),
        1
    );
    let (h, seq, back, _) = as_new_project(&bytes);
    let events = events_of(h.doc(), seq, back[0]);
    assert_eq!(
        events
            .iter()
            .filter(|e| e.1.starts_with("Program"))
            .cloned()
            .collect::<Vec<_>>(),
        vec![(
            Q,
            format!(
                "{:?}",
                EventBody::Program {
                    program: 5,
                    bank: Some((1, 2))
                }
            )
        )]
    );
    assert!(!events.iter().any(|e| e.1.starts_with("Cc")));
}

#[test]
fn a_short_first_bar_reads_back_as_a_pickup() {
    // 4/4 then 5/4 from bar 2 writes the same bytes as a one-bar pickup
    // into 5/4 (Ruling 8): both sides say so, and every event keeps its
    // time; only the bar numbers move.
    let mut d = Doc::new("Song");
    let seq = d.seq;
    let meter = MeterMap::new(vec![
        MeterEntry {
            tick: Tick(0),
            meter: Meter { num: 4, den: 4 },
            key: KeySig::default(),
        },
        MeterEntry {
            tick: Tick(BAR),
            meter: Meter { num: 5, den: 4 },
            key: KeySig::default(),
        },
    ])
    .unwrap();
    d.exec(Command::Sequence(SequenceCmd::SetMeter { seq, meter }));
    let dest = d.builtin(0, 0);
    let lane = d.track("Bass", dest);
    d.put(
        lane,
        vec![note(0, 60, up7(100), Q), note(BAR, 62, up7(100), Q)],
    );
    let (bytes, report) = d.smf(960, false);
    has(&report, ItemKind::Note, "Bar 1 is shorter than bar 2");
    let (h, seq2, back, r) = as_new_project(&bytes);
    assert_eq!(has(&r, ItemKind::Note, "First bar read as a pickup"), 1);
    let got = h.doc().sequence(seq2).unwrap();
    assert_eq!(got.start_offset, Tick(BAR));
    assert_eq!(
        got.meter
            .entries()
            .iter()
            .map(|e| (e.tick, e.meter))
            .collect::<Vec<_>>(),
        vec![(Tick(0), Meter { num: 5, den: 4 })]
    );
    // Both notes, a bar apart, the first now in the pickup.
    assert_eq!(
        notes_of(h.doc(), seq2, back[0]),
        vec![(-BAR, 60, Q), (0, 62, Q)]
    );
    let secs = |doc: &Document, s: SeqId, t: i64| {
        let seq = doc.sequence(s).unwrap();
        seq.tempo.tick_to_seconds(Tick(t)) - seq.tempo.tick_to_seconds(-seq.start_offset)
    };
    assert_eq!(secs(&d.doc, seq, BAR), secs(h.doc(), seq2, 0));
}
