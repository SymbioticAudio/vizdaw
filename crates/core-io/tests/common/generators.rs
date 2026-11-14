//! 6d's round-trip document generator (plan 6d Task 8), shared with the
//! merge's edit-script properties (plan 6e Task 7): the building helpers,
//! the proptest strategies (`spec()` and the ones it uses) and [`build`],
//! which makes a document of a [`Spec`].

use core_engine::fixtures::{Builder, Q};
use core_model::command::{Command, LaneCmd, SequenceCmd};
use core_model::document::Document;
use core_model::ids::{AnyId, SeqId};
use core_model::lane::QuantizeSpec;
use core_model::mixer::PluginFormat;
use core_model::time::Tick;
use std::collections::BTreeMap;
// ---------------------------------------------------------------- building

use core_model::automation::{AutomationTarget, SendIndex};
use core_model::command::{AudioCmd, CrossfadeLength, EventCmd, MediaCmd, MixerCmd, RoutingCmd};
use core_model::document::{MediaOrigin, SongInfo};
use core_model::event::{EventBody, EventSpec, Fade, FadeShape, PerNoteCtl, PerNoteKind};
use core_model::ids::{DestId, LaneId, MediaId, OutputPathId, PluginInstanceId, StripId};
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::mixer::{AuxSend, ParamId, PluginRef, StripKind, Swatch};
use core_model::sequence::{Marker, SeqLength};

pub fn clap(id: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: id.into(),
        name: id.into(),
        vendor: "Example".into(),
        version: "1.0".into(),
    }
}

pub fn output(b: &mut Builder, name: &str) -> OutputPathId {
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

pub fn route(b: &mut Builder, seq: SeqId, lane: LaneId, out: OutputPathId) {
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane,
        output: Some(out),
    }))
    .unwrap();
}

pub fn spec_at(tick: i64, body: EventBody) -> EventSpec {
    EventSpec::new(Tick(tick), body)
}

/// The sequence of a new song with a note track "name" on its own CLAP
/// instrument "<name> Synth" (named apart from its track, so a trip shows
/// the import keeps the file's name). Returns the instrument's strip and
/// plug-in instance.
pub fn song(b: &mut Builder, name: &str) -> (SeqId, LaneId, DestId, PluginInstanceId) {
    let (dest, slot) = b
        .plugin_dest(
            &format!("{name} Synth"),
            clap(&format!("org.example.{name}")),
        )
        .unwrap();
    let (seq, lane) = b.sequence("Song", name, Some(dest)).unwrap();
    (seq, lane, dest, slot)
}

/// A further note track "name" on its own instrument "<name> Synth".
pub fn note_track(b: &mut Builder, seq: SeqId, name: &str) -> (LaneId, DestId, PluginInstanceId) {
    let (dest, slot) = b
        .plugin_dest(
            &format!("{name} Synth"),
            clap(&format!("org.example.{name}")),
        )
        .unwrap();
    let lane = b.lane(seq, name, Some(dest)).unwrap();
    (lane, dest, slot)
}

/// An audio track "name" on an output path of the same name; returns its
/// lane and strip.
pub fn audio_track(b: &mut Builder, seq: SeqId, name: &str) -> (LaneId, StripId) {
    let lane = b.lane(seq, name, None).unwrap();
    let out = output(b, name);
    route(b, seq, lane, out);
    (lane, b.strip_of(StripKind::AudioPath(out)).unwrap())
}

/// A media item with a content hash.
pub fn media(b: &mut Builder, name: &str, frames: u64, channels: u16) -> MediaId {
    let r = b
        .run(Command::Media(MediaCmd::Add {
            path: format!("Media/{name}.caf"),
            sample_rate: 48_000,
            channels,
            length_frames: frames,
            content_hash: Some(format!("hash-{name}")),
            name: Some(name.into()),
            origin: Some(MediaOrigin::Imported {
                original_name: format!("{name}.wav"),
            }),
        }))
        .unwrap();
    r.created
        .iter()
        .find_map(|i| match i {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .unwrap()
}

pub fn lock(b: &mut Builder, seq: SeqId, ticks: i64) {
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(ticks)),
    }))
    .unwrap();
}

pub fn capture(b: Builder, slot: PluginInstanceId, bytes: &[u8]) -> Builder {
    let (doc, _) = core_model::state::capture(b.doc(), slot, bytes).unwrap();
    Builder::from_doc(doc)
}

pub fn plain_fade(frames: u64) -> Fade {
    Fade {
        length_frames: frames,
        shape: FadeShape::Linear,
    }
}

// ---------------------------------------------------------------- the generator

use proptest::prelude::*;

/// A fader/pan/mute/send/parameter the generator automates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auto {
    Fader,
    Pan,
    Mute,
    Send,
    Param,
}

#[derive(Debug, Clone)]
pub struct StripSpec {
    /// Fader in half dB.
    pub gain: i32,
    /// Pan in sixteenths.
    pub pan: i32,
    pub mute: bool,
    pub solo: bool,
    pub colour: Option<usize>,
    /// Send to the bus: half dB, pre-fader, enabled.
    pub send: Option<(i32, bool, bool)>,
    /// An effect: its state, and whether it is bypassed.
    pub insert: Option<(Option<Vec<u8>>, bool)>,
    /// The instrument's state.
    pub state: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct NoteSpec {
    pub tick: i64,
    pub pitch: u8,
    pub velocity: u16,
    pub release: u16,
    pub duration: i64,
    pub channel: Option<(u8, u8)>,
    /// (kind 0..4, offset, value).
    pub expression: Vec<(u8, i64, u32)>,
}

#[derive(Debug, Clone)]
pub enum Midi {
    Cc(u8, u32),
    Bend(u32),
    Pressure(u32),
    Poly(u8, u32),
    Program(u8, Option<(u8, u8)>),
}

#[derive(Debug, Clone)]
pub struct NoteTrack {
    pub strip: StripSpec,
    /// Play quantize (grid, strength), final review I2.
    pub quantize: Option<QuantizeSpec>,
    pub notes: Vec<NoteSpec>,
    pub midi: Vec<(i64, Midi)>,
    pub automation: Vec<(Auto, Vec<(i64, f64)>)>,
}

#[derive(Debug, Clone)]
pub enum Slot {
    Plain {
        offset: u64,
        length: u64,
        gain: f32,
        fade_in: (u64, usize),
        fade_out: (u64, usize),
        name: Option<String>,
        muted: bool,
        wide: bool,
    },
    /// Two long regions of the stereo item, the second crossfading in.
    Pair { fade: u64, shape: usize },
}

#[derive(Debug, Clone)]
pub struct AudioTrack {
    pub strip: StripSpec,
    pub slots: Vec<Slot>,
    pub automation: Vec<(Auto, Vec<(i64, f64)>)>,
}

#[derive(Debug, Clone)]
pub struct Spec {
    pub notes: Vec<NoteTrack>,
    pub audio: Vec<AudioTrack>,
    pub bus: Option<StripSpec>,
    pub master_gain: i32,
    pub meter: (u8, u8),
    pub change: Option<(i64, u8, u8)>,
    pub tempo: Vec<(i64, u32, bool)>,
    pub pickup_beats: u8,
    pub markers: Vec<(i64, String)>,
    pub info: Vec<(usize, String)>,
    pub gain: bool,
}

pub const SWATCHES: [Swatch; 8] = [
    Swatch::Red,
    Swatch::Orange,
    Swatch::Yellow,
    Swatch::Green,
    Swatch::Teal,
    Swatch::Blue,
    Swatch::Purple,
    Swatch::Pink,
];
pub const SHAPES: [FadeShape; 4] = [
    FadeShape::Linear,
    FadeShape::EqualPower,
    FadeShape::Exponential,
    FadeShape::SCurve,
];

pub fn word() -> impl Strategy<Value = String> {
    "[A-Z][a-z]{1,6}( [a-z]{1,6}){0,2}"
}

pub fn strip_spec(with_state: bool, with_insert: bool) -> impl Strategy<Value = StripSpec> {
    (
        -120i32..=24,
        -16i32..=16,
        any::<bool>(),
        any::<bool>(),
        proptest::option::of(0usize..8),
        proptest::option::of((-120i32..=12, any::<bool>(), any::<bool>())),
        proptest::option::of((
            proptest::option::of(proptest::collection::vec(any::<u8>(), 0..32)),
            any::<bool>(),
        )),
        proptest::option::of(proptest::collection::vec(any::<u8>(), 0..32)),
    )
        .prop_map(
            move |(gain, pan, mute, solo, colour, send, insert, state)| StripSpec {
                gain,
                pan,
                mute,
                solo,
                colour,
                send,
                insert: if with_insert { insert } else { None },
                state: if with_state { state } else { None },
            },
        )
}

pub fn midi() -> impl Strategy<Value = Midi> {
    // No CC 0 or 32 (they are a program's bank) and no data entry.
    let index =
        (1u8..120).prop_filter("plain controller", |i| !matches!(i, 6 | 32 | 38 | 98..=101));
    prop_oneof![
        (index, any::<u32>()).prop_map(|(i, v)| Midi::Cc(i, v)),
        any::<u32>().prop_map(Midi::Bend),
        any::<u32>().prop_map(Midi::Pressure),
        (0u8..128, any::<u32>()).prop_map(|(p, v)| Midi::Poly(p, v)),
        (0u8..128, proptest::option::of((0u8..128, 0u8..128)))
            .prop_map(|(p, b)| Midi::Program(p, b)),
    ]
}

/// Up to `n` automation lanes of the kinds in `kinds`, points at distinct
/// whole beats.
pub fn automation(kinds: Vec<Auto>) -> impl Strategy<Value = Vec<(Auto, Vec<(i64, f64)>)>> {
    proptest::collection::vec(
        (
            proptest::sample::select(kinds),
            proptest::collection::btree_map(0i64..20, 0u32..=64, 1..5),
        ),
        0..4,
    )
    .prop_map(|lanes| {
        let mut seen = std::collections::BTreeSet::new();
        lanes
            .into_iter()
            .filter(|(k, _)| seen.insert(*k as u8))
            .map(|(k, pts)| {
                let pts = pts
                    .into_iter()
                    .map(|(beat, v)| {
                        let v = if k == Auto::Mute {
                            f64::from(v % 2)
                        } else {
                            f64::from(v) / 64.0
                        };
                        (beat * Q, v)
                    })
                    .collect();
                (k, pts)
            })
            .collect()
    })
}

pub fn note_track_spec(pickup: i64) -> impl Strategy<Value = NoteTrack> {
    let note = (
        -pickup..24 * Q,
        0u8..128,
        1u16..=u16::MAX,
        any::<u16>(),
        1i64..2 * Q,
        proptest::option::weighted(0.15, (0u8..3, 1u8..16)),
        proptest::collection::vec((0u8..4, 0i64..1_000, any::<u32>()), 0..3),
    )
        .prop_map(
            |(tick, pitch, velocity, release, duration, channel, expression)| NoteSpec {
                tick,
                pitch,
                velocity,
                release,
                duration,
                channel,
                expression,
            },
        );
    let grids = [Q / 4, Q / 3, Q / 2, Q];
    (
        strip_spec(true, true),
        proptest::option::weighted(0.3, (0usize..grids.len(), 1u8..=100)),
        proptest::collection::vec(note, 1..16),
        proptest::collection::vec((-pickup..24 * Q, midi()), 0..8),
        automation(vec![
            Auto::Fader,
            Auto::Pan,
            Auto::Mute,
            Auto::Send,
            Auto::Param,
        ]),
    )
        .prop_map(move |(strip, quantize, notes, midi, automation)| {
            let quantize = quantize.map(|(g, s)| QuantizeSpec::grid(Tick(grids[g]), s));
            // Where a note plays: quantized as playback does.
            let at = |t: i64| quantize.as_ref().map_or(t, |q| q.apply(Tick(t)).0);
            let mut notes = notes;
            notes.sort_by_key(|n| n.tick);
            // No two notes of a pitch overlap where they play; offsets stay
            // inside notes; each expression kind once per offset; the
            // track's main channel (0) has more notes than any other.
            let mut free: BTreeMap<u8, i64> = BTreeMap::new();
            notes.retain(|n| {
                let t = at(n.tick);
                if free.get(&n.pitch).is_some_and(|f| *f > t) {
                    return false;
                }
                free.insert(n.pitch, t + n.duration);
                true
            });
            for n in &mut notes {
                n.expression.iter_mut().for_each(|e| e.1 %= n.duration);
                n.expression.sort_by_key(|e| (e.0, e.1));
                n.expression.dedup_by_key(|e| (e.0, e.1));
            }
            let total = notes.len();
            let mut kept = 0;
            for n in &mut notes {
                if n.channel.is_some() {
                    kept += 1;
                    if kept * 2 >= total {
                        n.channel = None;
                    }
                }
            }
            let mut midi = midi;
            midi.sort_by_key(|m| m.0);
            midi.dedup_by_key(|m| {
                (
                    m.0,
                    match &m.1 {
                        Midi::Cc(i, _) => 1000 + i32::from(*i),
                        Midi::Bend(_) => 1,
                        Midi::Pressure(_) => 2,
                        Midi::Poly(p, _) => 2000 + i32::from(*p),
                        Midi::Program(..) => 3,
                    },
                )
            });
            NoteTrack {
                strip,
                quantize,
                notes,
                midi,
                automation,
            }
        })
}

pub fn audio_track_spec() -> impl Strategy<Value = AudioTrack> {
    let plain = (
        0u64..1_000,
        480u64..=24_000,
        prop_oneof![Just(0.0f32), Just(-6.0), Just(3.0), Just(-12.5)],
        (0u64..240, 0usize..4),
        (0u64..240, 0usize..4),
        proptest::option::of(word()),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(offset, length, gain, fade_in, fade_out, name, muted, wide)| Slot::Plain {
                offset,
                length,
                gain,
                fade_in,
                fade_out,
                name,
                muted,
                wide,
            },
        );
    let pair = (480u64..4_800, 0usize..4).prop_map(|(fade, shape)| Slot::Pair { fade, shape });
    (
        strip_spec(false, false),
        proptest::collection::vec(prop_oneof![4 => plain, 1 => pair], 1..4),
        automation(vec![Auto::Fader, Auto::Pan, Auto::Mute, Auto::Send]),
    )
        .prop_map(|(strip, slots, automation)| AudioTrack {
            strip,
            slots,
            automation,
        })
}

pub fn spec() -> impl Strategy<Value = Spec> {
    (
        (2u8..8, prop_oneof![Just(4u8), Just(8u8)]),
        proptest::option::of((1i64..5, 2u8..8, prop_oneof![Just(4u8), Just(8u8)])),
        proptest::collection::vec((1i64..32 * Q, 30u32..=240, any::<bool>()), 0..4),
        (30u32..=240, any::<bool>()),
        0u8..8,
    )
        .prop_flat_map(|(meter, change, points, (bpm0, ramp0), pickup)| {
            let pickup_beats = pickup % meter.0;
            let beat = 4 * Q / i64::from(meter.1);
            let pickup_ticks = i64::from(pickup_beats) * beat;
            let bar = beat * i64::from(meter.0);
            let change = change
                .filter(|(_, n, d)| (*n, *d) != meter)
                .map(|(k, n, d)| (k * bar, n, d));
            let mut tempo = vec![(0, bpm0, ramp0)];
            let mut points = points;
            points.sort_by_key(|p| p.0);
            points.dedup_by_key(|p| p.0);
            tempo.extend(points);
            (
                proptest::collection::vec(note_track_spec(pickup_ticks), 1..4),
                proptest::collection::vec(audio_track_spec(), 0..3),
                proptest::option::of(strip_spec(false, false)),
                -120i32..=24,
                proptest::collection::vec((-pickup_ticks..24 * Q, word()), 0..4),
                proptest::collection::vec((0usize..13, word()), 0..4),
                any::<bool>(),
            )
                .prop_map(
                    move |(notes, audio, bus, master_gain, markers, info, gain)| {
                        let mut markers = markers;
                        markers.sort_by_key(|m| m.0);
                        markers.dedup_by_key(|m| m.0);
                        let mut info = info;
                        info.sort_by_key(|i| i.0);
                        info.dedup_by_key(|i| i.0);
                        Spec {
                            notes,
                            audio,
                            bus,
                            master_gain,
                            meter,
                            change,
                            tempo: tempo.clone(),
                            pickup_beats,
                            markers,
                            info,
                            gain,
                        }
                    },
                )
        })
}

// ---------------------------------------------------------------- building a spec

pub fn body_of(n: &NoteSpec) -> EventBody {
    EventBody::Note {
        pitch: n.pitch,
        velocity: n.velocity,
        release_velocity: n.release,
        duration: Tick(n.duration),
        attribute: None,
        per_note: n
            .expression
            .iter()
            .map(|(k, offset, value)| PerNoteCtl {
                kind: match k {
                    0 => PerNoteKind::PitchBend,
                    1 => PerNoteKind::Registered(74),
                    2 => PerNoteKind::Registered(7),
                    _ => PerNoteKind::Registered(10),
                },
                offset: Tick(*offset),
                value: *value,
            })
            .collect(),
    }
}

pub fn midi_body(m: &Midi) -> EventBody {
    match m {
        Midi::Cc(index, value) => EventBody::Cc {
            index: *index,
            value: *value,
        },
        Midi::Bend(value) => EventBody::PitchBend { value: *value },
        Midi::Pressure(value) => EventBody::ChannelPressure { value: *value },
        Midi::Poly(pitch, value) => EventBody::PolyPressure {
            pitch: *pitch,
            value: *value,
        },
        Midi::Program(program, bank) => EventBody::Program {
            program: *program,
            bank: *bank,
        },
    }
}

/// Sets `strip`'s mix as `s` says (send and effect aside).
pub fn mix(b: &mut Builder, strip: StripId, s: &StripSpec) {
    b.mixer(MixerCmd::SetGain {
        strip,
        gain_db: f64::from(s.gain) / 2.0,
    })
    .unwrap();
    b.mixer(MixerCmd::SetPan {
        strip,
        pan: f64::from(s.pan) / 16.0,
    })
    .unwrap();
    b.mixer(MixerCmd::SetMute {
        strip,
        mute: s.mute,
    })
    .unwrap();
    b.mixer(MixerCmd::SetSolo {
        strip,
        solo: s.solo,
    })
    .unwrap();
    if let Some(c) = s.colour {
        b.mixer(MixerCmd::SetColour {
            strip,
            colour: Some(SWATCHES[c]),
        })
        .unwrap();
    }
}

pub fn auto_events(
    kinds: &[(Auto, Vec<(i64, f64)>)],
    strip: StripId,
    param: Option<PluginInstanceId>,
    has_send: bool,
) -> Vec<EventSpec> {
    let mut out = Vec::new();
    for (kind, points) in kinds {
        let target = match kind {
            Auto::Fader => AutomationTarget::StripFader(strip),
            Auto::Pan => AutomationTarget::StripPan(strip),
            Auto::Mute => AutomationTarget::StripMute(strip),
            Auto::Send if has_send => AutomationTarget::SendLevel(strip, SendIndex(0)),
            Auto::Param if param.is_some() => {
                AutomationTarget::PluginParam(param.unwrap(), ParamId(7))
            }
            _ => continue,
        };
        for (tick, value) in points {
            out.push(spec_at(
                *tick,
                EventBody::Automation {
                    target,
                    value: *value,
                },
            ));
        }
    }
    out
}

/// The document of `s` and its sequence.
pub fn build(s: &Spec) -> (Document, SeqId) {
    let mut b = Builder::new();
    let mut states: Vec<(PluginInstanceId, Vec<u8>)> = Vec::new();
    let mut seq = None;
    let bus = s.bus.as_ref().map(|spec| {
        let strip = b.bus("Bus").unwrap();
        mix(&mut b, strip, spec);
        strip
    });
    let mut lanes = Vec::new();
    for (i, t) in s.notes.iter().enumerate() {
        let name = format!("Track {i}");
        // Named apart from the track: the import keeps the file's name.
        let (dest, slot) = b
            .plugin_dest(
                &format!("Instrument {i}"),
                clap(&format!("org.example.{name}")),
            )
            .unwrap();
        let lane = match seq {
            None => {
                let (q, lane) = b.sequence("Song", &name, Some(dest)).unwrap();
                seq = Some(q);
                lane
            }
            Some(q) => b.lane(q, &name, Some(dest)).unwrap(),
        };
        if let (Some(q), Some(seq)) = (&t.quantize, seq) {
            b.run(Command::Lane(LaneCmd::SetPlayQuantize {
                seq,
                lane,
                quantize: Some(q.clone()),
            }))
            .unwrap();
        }
        let strip = b.strip_of(StripKind::Instrument(dest)).unwrap();
        mix(&mut b, strip, &t.strip);
        if let (Some(bus), Some((db, pre, enabled))) = (bus, t.strip.send) {
            b.mixer(MixerCmd::AddSend {
                strip,
                send: AuxSend {
                    target: bus,
                    level_db: f64::from(db) / 2.0,
                    pre_fader: pre,
                    enabled,
                },
            })
            .unwrap();
        }
        if let Some(state) = &t.strip.state {
            states.push((slot, state.clone()));
        }
        if let Some((state, bypass)) = &t.strip.insert {
            let fx = b
                .insert(strip, clap(&format!("org.example.fx{i}")))
                .unwrap();
            b.mixer(MixerCmd::SetBypass {
                slot: fx,
                bypass: *bypass,
            })
            .unwrap();
            if let Some(state) = state {
                states.push((fx, state.clone()));
            }
        }
        lanes.push((
            lane,
            strip,
            Some(slot),
            t.strip.send.is_some() && bus.is_some(),
        ));
    }
    let seq = seq.expect("a note track");
    // Audio tracks, with their media.
    let mono = media(&mut b, "mono", 96_000, 1);
    let wide = media(&mut b, "wide", 192_000, 2);
    let mut audio_lanes = Vec::new();
    for (i, t) in s.audio.iter().enumerate() {
        let (lane, strip) = audio_track(&mut b, seq, &format!("Audio {i}"));
        mix(&mut b, strip, &t.strip);
        if let (Some(bus), Some((db, pre, enabled))) = (bus, t.strip.send) {
            b.mixer(MixerCmd::AddSend {
                strip,
                send: AuxSend {
                    target: bus,
                    level_db: f64::from(db) / 2.0,
                    pre_fader: pre,
                    enabled,
                },
            })
            .unwrap();
        }
        for (k, slot) in t.slots.iter().enumerate() {
            let at = k as i64 * 16 * Q;
            match slot {
                Slot::Plain {
                    offset,
                    length,
                    gain,
                    fade_in,
                    fade_out,
                    name,
                    muted,
                    wide: is_wide,
                } => {
                    let fin = fade_in.0.min(length / 2);
                    let fout = fade_out.0.min(length / 2);
                    let m = if *is_wide { wide } else { mono };
                    let e = b
                        .region(
                            seq,
                            lane,
                            at,
                            m,
                            *offset,
                            *length,
                            *gain,
                            Fade {
                                length_frames: fin,
                                shape: SHAPES[fade_in.1],
                            },
                            Fade {
                                length_frames: fout,
                                shape: SHAPES[fade_out.1],
                            },
                        )
                        .unwrap();
                    if let Some(name) = name {
                        b.run(Command::Audio(AudioCmd::SetName {
                            event: e,
                            name: Some(name.clone()),
                        }))
                        .unwrap();
                    }
                    if *muted {
                        b.run(Command::Event(EventCmd::SetMute {
                            events: vec![e],
                            mute: true,
                        }))
                        .unwrap();
                    }
                }
                Slot::Pair { fade, shape } => {
                    for (j, tick) in [at, at + Q].into_iter().enumerate() {
                        let e = b
                            .region(
                                seq,
                                lane,
                                tick,
                                wide,
                                0,
                                144_000,
                                0.0,
                                Fade::default(),
                                Fade::default(),
                            )
                            .unwrap();
                        if j == 1 {
                            b.run(Command::Audio(AudioCmd::Crossfade {
                                event: e,
                                length: CrossfadeLength::UpTo(*fade),
                                shape: SHAPES[*shape],
                            }))
                            .unwrap();
                        }
                    }
                }
            }
        }
        audio_lanes.push((lane, strip, t, bus.is_some() && t.strip.send.is_some()));
    }
    // Events.
    for ((lane, strip, slot, has_send), t) in lanes.iter().zip(&s.notes) {
        let mut events: Vec<EventSpec> = t
            .notes
            .iter()
            .map(|n| {
                let mut e = spec_at(n.tick, body_of(n));
                e.group_channel = n.channel;
                e
            })
            .collect();
        events.extend(t.midi.iter().map(|(tick, m)| spec_at(*tick, midi_body(m))));
        events.extend(auto_events(&t.automation, *strip, *slot, *has_send));
        b.events(seq, *lane, events).unwrap();
    }
    for (lane, strip, t, has_send) in &audio_lanes {
        let events = auto_events(&t.automation, *strip, None, *has_send);
        if !events.is_empty() {
            b.events(seq, *lane, events).unwrap();
        }
    }
    // The maps, markers and song info.
    let master = b.doc().mixer().master();
    b.mixer(MixerCmd::SetGain {
        strip: master,
        gain_db: f64::from(s.master_gain) / 2.0,
    })
    .unwrap();
    b.tempo(seq, &s.tempo).unwrap();
    let mut entries = vec![MeterEntry {
        tick: Tick(0),
        meter: Meter {
            num: s.meter.0,
            den: s.meter.1,
        },
        key: KeySig::default(),
    }];
    if let Some((tick, num, den)) = s.change {
        entries.push(MeterEntry {
            tick: Tick(tick),
            meter: Meter { num, den },
            key: KeySig::default(),
        });
    }
    b.run(Command::Sequence(SequenceCmd::SetMeter {
        seq,
        meter: MeterMap::new(entries).unwrap(),
    }))
    .unwrap();
    let beat = 4 * Q / i64::from(s.meter.1);
    b.run(Command::Sequence(SequenceCmd::SetStartOffset {
        seq,
        offset: Tick(i64::from(s.pickup_beats) * beat),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: s
            .markers
            .iter()
            .map(|(tick, name)| Marker {
                tick: Tick(*tick),
                name: name.clone(),
            })
            .collect(),
    }))
    .unwrap();
    let mut info = SongInfo::default();
    for (field, text) in &s.info {
        match field {
            0 => info.title = Some(text.clone()),
            1 => info.artist = Some(text.clone()),
            2 => info.album = Some(text.clone()),
            3 => info.original_artist = Some(text.clone()),
            4 => info.composer = Some(text.clone()),
            5 => info.songwriter = Some(text.clone()),
            6 => info.producer = Some(text.clone()),
            7 => info.arranger = Some(text.clone()),
            8 => info.year = Some(text.clone()),
            9 => info.genre = Some(text.clone()),
            10 => info.copyright = Some(text.clone()),
            11 => info.website = Some(text.clone()),
            _ => info.comment = Some(text.clone()),
        }
    }
    b.run(Command::Settings(
        core_model::command::SettingsCmd::SetInfo { info },
    ))
    .unwrap();
    let mut b = b;
    for (slot, bytes) in states {
        b = capture(b, slot, &bytes);
    }
    (b.finish(), seq)
}
