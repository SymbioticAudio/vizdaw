//! Importing an AAF composition (plan 6f Task 3): the mapping onto 6d's
//! import plan, L/R pairs (Review Focus 5), other edit rates (Review Focus
//! 1), the MIDI file beside, and the one undo step. Media are `Online`
//! records made without converting, so no audio file is read and Linux runs
//! every test.

mod common;

use common::aaf::{
    ClipSpec, FileSpec, Fx, PICTURE, Seg, SlotKind, SlotSpec, Value, clip, gain, sound,
};
use core_io::aaf::dict::def;
use core_io::aaf::import::{
    AafEnv, AafPlace, CLIP_GAIN_CUT, CLIP_GAIN_ENVELOPES, CURVES_LINEAR, FADE_SHAPES, LABEL,
    NO_MIDI, NOT_JOINED, NOT_NEW_AAF, ONE_CHANNEL, POSITIONS_MOVED, Pair, apply, map, pairs,
    tempo_and_start, timecode_note,
};
use core_io::aaf::store::Auid;
use core_io::aaf::timeline::{
    AafFile, AafMarker, BAD_RATES, BAD_VALUES, CLIP_PAN, Clip, Composition, Curve, EFFECTS,
    FROM_VIZDAW, Fade, NO_SOURCE, NON_AUDIO, POINTS_CUT, SELECTED, SEVERAL_COMPOSITIONS,
    SPANS_SOURCES, Shape, Slot,
};
use core_io::dawproject::import::{
    CROSSFADES_PLAIN, DpImport, HELD_STEPS, LEVELS_CUT, MediaSource, OFFLINE, PAST_END,
};
use core_io::dawproject::package::{AudioPackage, AudioRef, AudioSource};
use core_io::limits::{AafLimits, Limits};
use core_io::midi_in::{ImportOptions, MidiImport, parse};
use core_io::report::{ItemKind, Report};
use core_media::import::Imported;
use core_model::automation::{AutomationTarget, fader_norm};
use core_model::command::{Command, LaneCmd, SequenceCmd};
use core_model::document::Document;
use core_model::event::{EventBody, FadeShape};
use core_model::file::to_json;
use core_model::history::History;
use core_model::ids::{AnyId, SeqId};
use core_model::mixer::StripKind;
use core_model::selection::ViewFilter;
use core_model::tempo::{Bpm, TempoMap};
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

const R48: (i32, i32) = (48_000, 1);

fn no() -> AtomicBool {
    AtomicBool::new(false)
}

fn bpm(n: u32) -> TempoMap {
    TempoMap::constant(Bpm::whole(n)).unwrap()
}

fn has(r: &Report, kind: ItemKind, what: &str) -> bool {
    r.items.iter().any(|i| i.kind == kind && i.what == what)
}

fn open_with(fx: &Fx, limits: &AafLimits) -> AafFile {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("f.aaf");
    fx.write(&p);
    AafFile::open(&p, None, limits, &no()).unwrap()
}

fn open(fx: &Fx) -> AafFile {
    open_with(fx, &AafLimits::default())
}

/// A converted item standing for `a`, at its own rate, without converting.
fn item_of(a: &AudioRef, channels: u16) -> Imported {
    Imported {
        path: format!("Media/{} {}.caf", a.name, a.key),
        name: a.name.clone(),
        original_name: format!("{}.wav", a.name),
        channels,
        sample_rate: a.sample_rate,
        length_frames: a.frames,
        content_hash: format!("hash {}", a.key),
        clipped: 0,
    }
}

/// Every file of `audio` online, as its own channels.
fn online(audio: &[AudioRef]) -> BTreeMap<usize, MediaSource> {
    audio
        .iter()
        .map(|a| (a.key, MediaSource::Online(vec![item_of(a, a.channels)])))
        .collect()
}

fn env<'a>(audio: &'a [AudioRef], tempo: &'a TempoMap, join: bool) -> AafEnv<'a> {
    AafEnv {
        rate: 48_000,
        tempo,
        start: Tick(0),
        join_pairs: join,
        audio,
    }
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

struct Done {
    h: History,
    seq: SeqId,
    report: Report,
}

/// Applies `imp` (with `midi`) inside one transaction, as the job will.
fn apply_to(
    doc: Document,
    seq: SeqId,
    imp: &DpImport,
    midi: Option<&MidiImport>,
    new: bool,
) -> Done {
    let mut h = History::new(doc);
    let mut report = Report::default();
    h.begin(LABEL).unwrap();
    apply(
        &mut h,
        imp,
        midi,
        &AafPlace {
            seq,
            new_project: new,
        },
        &mut report,
    )
    .expect("applies");
    h.commit().unwrap();
    Done { h, seq, report }
}

/// `imp` applied to a new project with no MIDI file.
fn applied(imp: &DpImport) -> Done {
    let (doc, seq) = new_doc();
    apply_to(doc, seq, imp, None, true)
}

/// Each track's events: (tick, body), in order.
fn events(d: &Done) -> Vec<Vec<(Tick, EventBody)>> {
    d.h.doc()
        .sequence(d.seq)
        .unwrap()
        .lanes
        .iter()
        .map(|l| {
            l.active()
                .map(|t| {
                    t.events
                        .values()
                        .map(|e| (e.tick, e.body.clone()))
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect()
}

fn regions(d: &Done) -> Vec<Vec<(Tick, EventBody)>> {
    events(d)
        .into_iter()
        .map(|v| {
            v.into_iter()
                .filter(|(_, b)| matches!(b, EventBody::AudioRegion { .. }))
                .collect()
        })
        .collect()
}

fn lane_names(d: &Done) -> Vec<String> {
    d.h.doc()
        .sequence(d.seq)
        .unwrap()
        .lanes
        .iter()
        .map(|l| l.name.clone())
        .collect()
}

/// The audio-path strip of track `i`.
fn strip_of(d: &Done, i: usize) -> core_model::mixer::Strip {
    let doc = d.h.doc();
    let lane = doc.sequence(d.seq).unwrap().lanes.get(i).unwrap().clone();
    let out = lane.audio_out.expect("an audio track");
    let id = doc.mixer().find_kind(StripKind::AudioPath(out)).unwrap();
    doc.mixer().strip(id).unwrap().clone()
}

fn mapped(f: &AafFile, tempo: &TempoMap) -> DpImport {
    let sources = online(f.audio());
    map(
        f.composition(),
        &sources,
        &[],
        &env(f.audio(), tempo, true),
        &no(),
    )
    .expect("maps")
}

// ---------------------------------------------------------------- hand-built compositions

fn aref(key: usize, name: &str, channels: u16) -> AudioRef {
    AudioRef {
        key,
        source: AudioSource::Entry(format!("{name}.wav")),
        channels,
        sample_rate: 48_000,
        frames: 480_000,
        name: name.into(),
    }
}

fn mono_clip(media: usize, start: i64, length: i64, offset: u64) -> Clip {
    Clip {
        start,
        length,
        media,
        channel: None,
        source_offset: offset,
        fade_in: None,
        fade_out: None,
        crossfade_in: None,
        gain: 1.0,
        varying_gain: false,
        name: Some(Arc::from("take")),
    }
}

fn slot(name: &str, clips: Vec<Clip>) -> Slot {
    Slot {
        name: name.into(),
        channels: 1,
        clips,
        gain: Curve::default(),
        pan: Curve::default(),
    }
}

fn comp(rate: (i32, i32), slots: Vec<Slot>) -> Composition {
    Composition {
        name: "Song".into(),
        edit_rate: rate,
        start_tc: None,
        slots,
        markers: Vec::new(),
        comments: Vec::new(),
        from_vizdaw: false,
        report: Report::default(),
    }
}

/// Two slots named `l` and `r` playing files `k` and `k + 1` with the same
/// clips; `nudge` moves the right one's second offset.
fn pair_slots(l: &str, r: &str, k: usize, nudge: u64) -> Vec<Slot> {
    vec![
        slot(
            l,
            vec![mono_clip(k, 0, 1_000, 0), mono_clip(k, 2_000, 500, 100)],
        ),
        slot(
            r,
            vec![
                mono_clip(k + 1, 0, 1_000, 0),
                mono_clip(k + 1, 2_000, 500, 100 + nudge),
            ],
        ),
    ]
}

fn mono_refs(n: usize) -> Vec<AudioRef> {
    (0..n).map(|k| aref(k, &format!("F{k}"), 1)).collect()
}

// ---------------------------------------------------------------- tests

#[test]
fn maps_tracks_regions_and_fades() {
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("Gtr take", 48_000, 1, 200_000));
    let b = fx.file(&FileSpec::wav("Bass take", 48_000, 1, 200_000));
    fx.comp(
        "Song",
        true,
        &[
            sound(
                1,
                "Gtr",
                R48,
                Seg::Seq(vec![
                    Seg::Fill(4_800),
                    common::aaf::trans(4_800, None),
                    clip(&a, 1, 0, 100_800),
                    common::aaf::trans(4_800, Some(0)),
                    clip(&a, 1, 1_000, 48_000),
                ]),
            ),
            sound(
                2,
                "Bass",
                R48,
                Seg::Seq(vec![gain(1, 2, clip(&b, 1, 0, 48_000))]),
            ),
            SlotSpec {
                id: 3,
                name: "Markers".into(),
                rate: R48,
                kind: SlotKind::Markers(vec![(48_000, "Verse".into())]),
            },
        ],
    );
    let f = open(&fx);
    let t120 = bpm(120);
    let d = applied(&mapped(&f, &t120));
    assert_eq!(lane_names(&d), ["Gtr", "Bass"]);
    let regs = regions(&d);
    assert_eq!(regs[0].len(), 2);
    let (t0, x) = &regs[0][0];
    let (t1, y) = &regs[0][1];
    assert_eq!(*t0, t120.seconds_to_tick(0.0));
    assert_eq!(*t1, t120.seconds_to_tick(2.0));
    let EventBody::AudioRegion {
        fade_in,
        crossfade,
        length,
        name,
        ..
    } = x
    else {
        unreachable!()
    };
    assert_eq!(
        (fade_in.length_frames, fade_in.shape, *crossfade, *length),
        (4_800, FadeShape::Linear, false, 100_800)
    );
    assert_eq!(name.as_deref(), Some("Gtr take"));
    let EventBody::AudioRegion {
        fade_in,
        crossfade,
        source_offset,
        ..
    } = y
    else {
        unreachable!()
    };
    assert!(*crossfade);
    assert_eq!((fade_in.length_frames, *source_offset), (4_800, 1_000));
    let EventBody::AudioRegion { gain_db, .. } = &regs[1][0].1 else {
        unreachable!()
    };
    assert!((gain_db + 6.0206).abs() < 0.001, "{gain_db}");
    let s = d.h.doc().sequence(d.seq).unwrap();
    assert_eq!(s.markers.len(), 1);
    assert_eq!(
        (s.markers[0].tick, s.markers[0].name.as_str()),
        (t120.seconds_to_tick(1.0), "Verse")
    );
    assert_eq!(s.name, "Song");
    let sum = &d.report.summary;
    assert_eq!((sum.tracks, sum.regions, sum.markers), (2, 3, 1));
    assert!(has(&d.report, ItemKind::Note, NO_MIDI));
    assert!(!has(&d.report, ItemKind::Changed, CROSSFADES_PLAIN));
    assert!(core_model::rules::validate(d.h.doc()).is_empty());
}

#[test]
fn positions_at_other_edit_rates() {
    let audio = mono_refs(1);
    let sources = online(&audio);
    // 25 fps: edit unit 37 is 37/25 s.
    let c = comp((25, 1), vec![slot("Dialog", vec![mono_clip(0, 37, 25, 0)])]);
    let t120 = bpm(120);
    let imp = map(&c, &sources, &[], &env(&audio, &t120, true), &no()).unwrap();
    let d = applied(&imp);
    let regs = regions(&d);
    assert_eq!(regs[0][0].0, t120.seconds_to_tick(37.0 / 25.0));
    let EventBody::AudioRegion { length, .. } = &regs[0][0].1 else {
        unreachable!()
    };
    assert_eq!(*length, 48_000);

    // 20 BPM: a tick is about 9.4 frames, so frame 48 005 comes back more
    // than a frame away.
    let c = comp(R48, vec![slot("A", vec![mono_clip(0, 48_005, 1_000, 0)])]);
    let t20 = bpm(20);
    let imp = map(&c, &sources, &[], &env(&audio, &t20, true), &no()).unwrap();
    assert!(has(&imp.report, ItemKind::Changed, POSITIONS_MOVED));
    // 120 BPM: a tick is 1.5625 frames, at most 0.79 away.
    let imp = map(&c, &sources, &[], &env(&audio, &t120, true), &no()).unwrap();
    assert!(!has(&imp.report, ItemKind::Changed, POSITIONS_MOVED));
}

#[test]
fn shapes_map_per_ruling_5() {
    let audio = mono_refs(1);
    let sources = online(&audio);
    let faded = |start: i64, shape: Shape| Clip {
        fade_in: Some(Fade { frames: 100, shape }),
        fade_out: Some(Fade { frames: 50, shape }),
        ..mono_clip(0, start, 1_000, 0)
    };
    let c = comp(
        R48,
        vec![slot(
            "A",
            vec![
                faded(0, Shape::Power),
                faded(2_000, Shape::Log),
                faded(4_000, Shape::Linear),
            ],
        )],
    );
    let t120 = bpm(120);
    let imp = map(&c, &sources, &[], &env(&audio, &t120, true), &no()).unwrap();
    assert!(!has(&imp.report, ItemKind::Changed, FADE_SHAPES));
    let d = applied(&imp);
    let shapes: Vec<(FadeShape, FadeShape)> = regions(&d)[0]
        .iter()
        .map(|(_, b)| match b {
            EventBody::AudioRegion {
                fade_in, fade_out, ..
            } => (fade_in.shape, fade_out.shape),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        shapes,
        [
            (FadeShape::EqualPower, FadeShape::EqualPower),
            (FadeShape::Exponential, FadeShape::Exponential),
            (FadeShape::Linear, FadeShape::Linear),
        ]
    );

    // A B-spline (or constant, or unknown) fade becomes linear, listed.
    let c = comp(R48, vec![slot("A", vec![faded(0, Shape::Other)])]);
    let imp = map(&c, &sources, &[], &env(&audio, &t120, true), &no()).unwrap();
    assert!(has(&imp.report, ItemKind::Changed, FADE_SHAPES));
    let d = applied(&imp);
    let EventBody::AudioRegion { fade_in, .. } = &regions(&d)[0][0].1 else {
        unreachable!()
    };
    assert_eq!(fade_in.shape, FadeShape::Linear);
}

#[test]
fn track_gain_and_pan() {
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    let inner = Seg::Seq(vec![clip(&a, 1, 0, 1_000), Seg::Fill(1_000)]);
    let panned = Seg::Op {
        op: def::OPERATION_DEF_MONO_AUDIO_PAN,
        param: def::PARAMETER_DEF_PAN,
        value: Value::Points(
            vec![((0, 1), (0, 1)), ((1, 2), (1, 2)), ((1, 1), (1, 1))],
            def::INTERPOLATION_DEF_LINEAR,
        ),
        input: Box::new(inner),
    };
    fx.comp("C", true, &[sound(1, "T", R48, gain(1, 2, panned))]);
    let f = open(&fx);
    let t120 = bpm(120);
    let imp = mapped(&f, &t120);
    assert!(!has(&imp.report, ItemKind::Changed, CURVES_LINEAR));
    let d = applied(&imp);
    let strip = strip_of(&d, 0);
    assert!(
        (fader_norm(strip.gain_db) - fader_norm(-6.02)).abs() < 1e-3,
        "{}",
        strip.gain_db
    );
    let pans: Vec<(Tick, f64)> = events(&d)[0]
        .iter()
        .filter_map(|(t, b)| match b {
            EventBody::Automation {
                target: AutomationTarget::StripPan(s),
                value,
            } if *s == strip.id => Some((*t, *value)),
            _ => None,
        })
        .collect();
    assert_eq!(
        pans,
        [
            (t120.seconds_to_tick(0.0), 0.0),
            (t120.seconds_to_tick(1_000.0 / 48_000.0), 0.5),
            (t120.seconds_to_tick(2_000.0 / 48_000.0), 1.0),
        ]
    );

    // A gain of 8 (+18 dB) is cut to +12 dB.
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    let inner = Seg::Seq(vec![clip(&a, 1, 0, 1_000), Seg::Fill(1_000)]);
    fx.comp("C", true, &[sound(1, "T", R48, gain(8, 1, inner))]);
    let f = open(&fx);
    let imp = mapped(&f, &t120);
    assert!(has(&imp.report, ItemKind::Changed, LEVELS_CUT));
    let d = applied(&imp);
    assert!((strip_of(&d, 0).gain_db - 12.0).abs() < 1e-9);
}

#[test]
fn track_curves_with_other_interpolations() {
    let audio = mono_refs(1);
    let sources = online(&audio);
    let t120 = bpm(120);
    // Constant interpolation: held steps as one-tick ramps.
    let mut s = slot("A", vec![mono_clip(0, 0, 1_000, 0)]);
    s.gain = Curve {
        points: vec![(0, 1.0), (24_000, 0.5)],
        step: true,
        shape: Shape::Other,
        ..Curve::default()
    };
    let imp = map(
        &comp(R48, vec![s]),
        &sources,
        &[],
        &env(&audio, &t120, true),
        &no(),
    )
    .unwrap();
    assert!(has(&imp.report, ItemKind::Changed, HELD_STEPS));
    let d = applied(&imp);
    let faders: Vec<Tick> = events(&d)[0]
        .iter()
        .filter(|(_, b)| {
            matches!(
                b,
                EventBody::Automation {
                    target: AutomationTarget::StripFader(_),
                    ..
                }
            )
        })
        .map(|(t, _)| *t)
        .collect();
    let half = t120.seconds_to_tick(0.5);
    assert_eq!(faders, [Tick(0), Tick(half.0 - 1), half]);
    // Log interpolation: linear, listed.
    let mut s = slot("A", vec![mono_clip(0, 0, 1_000, 0)]);
    s.pan = Curve {
        points: vec![(0, 0.0), (24_000, 1.0)],
        shape: Shape::Log,
        ..Curve::default()
    };
    let imp = map(
        &comp(R48, vec![s]),
        &sources,
        &[],
        &env(&audio, &t120, true),
        &no(),
    )
    .unwrap();
    assert!(has(&imp.report, ItemKind::Changed, CURVES_LINEAR));
}

#[test]
fn varying_clip_gain_becomes_constant() {
    let audio = mono_refs(1);
    let sources = online(&audio);
    let c = comp(
        R48,
        vec![slot(
            "A",
            vec![Clip {
                gain: 0.25,
                varying_gain: true,
                ..mono_clip(0, 0, 1_000, 0)
            }],
        )],
    );
    let t120 = bpm(120);
    let imp = map(&c, &sources, &[], &env(&audio, &t120, true), &no()).unwrap();
    assert!(has(&imp.report, ItemKind::Changed, CLIP_GAIN_ENVELOPES));
    let d = applied(&imp);
    let EventBody::AudioRegion { gain_db, .. } = &regions(&d)[0][0].1 else {
        unreachable!()
    };
    assert!((gain_db + 12.0412).abs() < 0.001, "{gain_db}");
}

#[test]
fn pairs_join_only_when_identical() {
    let audio = mono_refs(4);
    let sources = online(&audio);
    for (l, r) in [
        ("Gtr L", "Gtr R"),
        ("Gtr.L", "Gtr.R"),
        ("Gtr Left", "Gtr Right"),
        ("gtr l", "GTR R"),
    ] {
        let c = comp(R48, pair_slots(l, r, 0, 0));
        assert_eq!(
            pairs(&c, &sources, true),
            [Pair {
                left: 0,
                right: 1,
                media: vec![(0, 1)],
            }],
            "{l} / {r}"
        );
    }
    // Mixed suffix styles do not pair.
    let c = comp(R48, pair_slots("Gtr L", "Gtr.R", 0, 0));
    assert!(pairs(&c, &sources, true).is_empty());

    // One sample apart in one clip's offset: none, with the Note.
    let c = comp(R48, pair_slots("Gtr L", "Gtr R", 0, 1));
    assert!(pairs(&c, &sources, true).is_empty());
    let t120 = bpm(120);
    let imp = map(&c, &sources, &[], &env(&audio, &t120, true), &no()).unwrap();
    assert!(has(&imp.report, ItemKind::Note, NOT_JOINED));
    // A file of another length: none.
    let mut other = sources.clone();
    if let Some(MediaSource::Online(items)) = other.get_mut(&1) {
        items[0].length_frames += 1;
    }
    let c = comp(R48, pair_slots("Gtr L", "Gtr R", 0, 0));
    assert!(pairs(&c, &other, true).is_empty());

    // A lone "L", and a stem-less " L" / " R": none.
    let c = comp(R48, vec![slot("L", vec![mono_clip(0, 0, 10, 0)])]);
    assert!(pairs(&c, &sources, true).is_empty());
    let c = comp(R48, pair_slots(" L", " R", 0, 0));
    assert!(pairs(&c, &sources, true).is_empty());

    // Turned off: none, and no Note.
    let c = comp(R48, pair_slots("Gtr L", "Gtr R", 0, 0));
    assert!(pairs(&c, &sources, false).is_empty());
    let imp = map(&c, &sources, &[], &env(&audio, &t120, false), &no()).unwrap();
    assert!(!has(&imp.report, ItemKind::Note, NOT_JOINED));

    // Two pairs with one stem: two pairs, "Gtr" and "Gtr 2".
    let mut slots = pair_slots("Gtr L", "Gtr R", 0, 0);
    slots.extend(pair_slots("Gtr L", "Gtr R", 2, 0));
    let c = comp(R48, slots);
    let ps = pairs(&c, &sources, true);
    assert_eq!(
        ps,
        [
            Pair {
                left: 0,
                right: 1,
                media: vec![(0, 1)],
            },
            Pair {
                left: 2,
                right: 3,
                media: vec![(2, 3)],
            },
        ]
    );
    let mut joined = sources.clone();
    for k in [0, 2] {
        joined.insert(k, MediaSource::Online(vec![item_of(&audio[k], 2)]));
    }
    let imp = map(&c, &joined, &ps, &env(&audio, &t120, true), &no()).unwrap();
    assert_eq!(lane_names(&applied(&imp)), ["Gtr", "Gtr 2"]);

    // A file shared with a slot outside the pair: none.
    let mut slots = pair_slots("Gtr L", "Gtr R", 0, 0);
    slots.push(slot("Other", vec![mono_clip(1, 5_000, 10, 0)]));
    assert!(pairs(&comp(R48, slots), &sources, true).is_empty());
}

#[test]
fn joined_pair_is_one_stereo_track() {
    let audio = mono_refs(2);
    let c = comp(R48, pair_slots("Keys L", "Keys R", 0, 0));
    let sources = online(&audio);
    let ps = pairs(&c, &sources, true);
    assert_eq!(ps.len(), 1);
    // The job made file 0 stereo.
    let mut joined = sources.clone();
    joined.insert(0, MediaSource::Online(vec![item_of(&audio[0], 2)]));
    let t120 = bpm(120);
    let imp = map(&c, &joined, &ps, &env(&audio, &t120, true), &no()).unwrap();
    let d = applied(&imp);
    assert_eq!(lane_names(&d), ["Keys"]);
    let regs = regions(&d);
    assert_eq!(regs[0].len(), 2);
    for (_, b) in &regs[0] {
        let EventBody::AudioRegion { media, .. } = b else {
            unreachable!()
        };
        assert_eq!(d.h.doc().media().get(media).unwrap().channels, 2);
    }
    assert_eq!(d.h.doc().media().len(), 1);
}

/// A type 0 SMF at 90 BPM with one note.
fn smf_90() -> Vec<u8> {
    let mut v = b"MThd".to_vec();
    v.extend_from_slice(&[0, 0, 0, 6, 0, 0, 0, 1, 0x03, 0xC0]);
    let track: &[u8] = &[
        0x00, 0xFF, 0x51, 0x03, 0x0A, 0x2C, 0x2B, // 666 667 µs: 90 BPM
        0x00, 0x90, 0x3C, 0x64, // note on
        0x83, 0x60, 0x80, 0x3C, 0x00, // 480 ticks later, off
        0x00, 0xFF, 0x2F, 0x00,
    ];
    v.extend_from_slice(b"MTrk");
    v.extend_from_slice(&(track.len() as u32).to_be_bytes());
    v.extend_from_slice(track);
    v
}

#[test]
fn midi_beside_sets_the_bars() {
    let midi = parse(
        &smf_90(),
        &ImportOptions {
            split_channels: None,
            mpe: true,
        },
        &Limits::default(),
        &no(),
    )
    .unwrap();
    assert!((midi.tempo.bpm_at(Tick(0)) - 90.0).abs() < 1e-6);
    let audio = mono_refs(1);
    let sources = online(&audio);
    let c = comp(R48, vec![slot("Gtr", vec![mono_clip(0, 96_000, 1_000, 0)])]);
    let imp = map(&c, &sources, &[], &env(&audio, &midi.tempo, true), &no()).unwrap();
    let (doc, seq) = new_doc();
    let d = apply_to(doc, seq, &imp, Some(&midi), true);
    let s = d.h.doc().sequence(seq).unwrap();
    assert!((s.tempo.bpm_at(Tick(0)) - 90.0).abs() < 1e-6);
    let lanes = &s.lanes;
    assert_eq!(lanes.len(), 2);
    assert!(lanes[0].midi_dest.is_some() && lanes[0].audio_out.is_none());
    assert!(lanes[1].audio_out.is_some());
    assert_eq!(regions(&d)[1][0].0, midi.tempo.seconds_to_tick(2.0));
    assert!(!has(&d.report, ItemKind::Note, NO_MIDI));
    assert_eq!(d.report.summary.tracks, 2);

    // A pickup of one beat: the MIDI file's position 0 is at -1 beat, and
    // the AAF's clips line up with it, the first inside the pickup.
    let midi = parse(
        &smf_pickup(),
        &ImportOptions {
            split_channels: None,
            mpe: true,
        },
        &Limits::default(),
        &no(),
    )
    .unwrap();
    let beat = core_model::time::PPQ;
    assert_eq!(midi.start_offset, Tick(beat));
    let (doc, seq) = new_doc();
    let (tempo, start) = tempo_and_start(Some(&midi), doc.sequence(seq).unwrap(), true).unwrap();
    assert_eq!(start, Tick(-beat));
    // At 90 BPM a beat is 2/3 s: 32 000 frames.
    let c = comp(
        R48,
        vec![slot(
            "Gtr",
            vec![mono_clip(0, 0, 1_000, 0), mono_clip(0, 32_000, 1_000, 0)],
        )],
    );
    let imp = map(
        &c,
        &sources,
        &[],
        &AafEnv {
            start,
            ..env(&audio, &tempo, true)
        },
        &no(),
    )
    .unwrap();
    assert!(
        !has(&imp.report, ItemKind::LeftOut, PAST_END),
        "{:?}",
        imp.report
    );
    let d = apply_to(doc, seq, &imp, Some(&midi), true);
    let s = d.h.doc().sequence(seq).unwrap();
    assert_eq!(s.start_offset, Tick(beat));
    assert_eq!(s.tempo, tempo);
    let notes: Vec<Tick> = events(&d)[0]
        .iter()
        .filter(|(_, b)| matches!(b, EventBody::Note { .. }))
        .map(|(t, _)| *t)
        .collect();
    assert_eq!(notes, [Tick(-beat), Tick(0)]);
    let ticks: Vec<Tick> = regions(&d)[1].iter().map(|(t, _)| *t).collect();
    assert_eq!(ticks, [Tick(-beat), Tick(0)]);
    assert!(core_model::rules::validate(d.h.doc()).is_empty());
}

#[test]
fn into_a_project_with_a_pickup() {
    let (doc, seq) = new_doc();
    let mut h = History::new(doc);
    let beat = core_model::time::PPQ;
    h.execute(
        &Command::Sequence(SequenceCmd::SetStartOffset {
            seq,
            offset: Tick(beat),
        }),
        &ViewFilter::default(),
    )
    .unwrap();
    let doc = h.doc().clone();
    let (tempo, start) = tempo_and_start(None, doc.sequence(seq).unwrap(), false).unwrap();
    assert_eq!(start, Tick(-beat));
    let audio = mono_refs(1);
    let sources = online(&audio);
    // At 120 BPM a beat is 0.5 s: 24 000 frames.
    let c = comp(
        R48,
        vec![slot(
            "A",
            vec![mono_clip(0, 0, 1_000, 0), mono_clip(0, 24_000, 1_000, 0)],
        )],
    );
    let imp = map(
        &c,
        &sources,
        &[],
        &AafEnv {
            start,
            ..env(&audio, &tempo, true)
        },
        &no(),
    )
    .unwrap();
    let d = apply_to(doc, seq, &imp, None, false);
    let ticks: Vec<Tick> = regions(&d)[0].iter().map(|(t, _)| *t).collect();
    assert_eq!(ticks, [Tick(-beat), Tick(0)]);
    assert!(core_model::rules::validate(d.h.doc()).is_empty());
}

/// A type 0 SMF at 90 BPM whose first bar is a one-beat pickup (1/4, then
/// 4/4), with a note in the pickup and one on the downbeat.
fn smf_pickup() -> Vec<u8> {
    let mut v = b"MThd".to_vec();
    v.extend_from_slice(&[0, 0, 0, 6, 0, 0, 0, 1, 0x03, 0xC0]);
    let track: &[u8] = &[
        0x00, 0xFF, 0x51, 0x03, 0x0A, 0x2C, 0x2B, // 90 BPM
        0x00, 0xFF, 0x58, 0x04, 0x01, 0x02, 0x18, 0x08, // 1/4
        0x00, 0x90, 0x3C, 0x64, // note on, in the pickup
        0x83, 0x60, 0x80, 0x3C, 0x00, // off at 480
        0x83, 0x60, 0xFF, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08, // 4/4 at 960
        0x00, 0x90, 0x40, 0x64, // note on, the downbeat
        0x87, 0x40, 0x80, 0x40, 0x00, // off at 1920
        0x00, 0xFF, 0x2F, 0x00,
    ];
    v.extend_from_slice(b"MTrk");
    v.extend_from_slice(&(track.len() as u32).to_be_bytes());
    v.extend_from_slice(track);
    v
}

#[test]
fn into_this_project_is_one_undo_step() {
    let (doc, seq) = new_doc();
    let mut h = History::new(doc);
    h.execute(
        &Command::Lane(LaneCmd::Add {
            seq,
            name: "Here".into(),
            index: None,
        }),
        &ViewFilter::default(),
    )
    .unwrap();
    let t100 = bpm(100);
    h.execute(
        &Command::Sequence(SequenceCmd::SetTempo {
            seq,
            tempo: t100.clone(),
        }),
        &ViewFilter::default(),
    )
    .unwrap();
    let before = h.doc().clone();
    let json = to_json(&before).unwrap();

    let audio = mono_refs(2);
    let sources = online(&audio);
    let mut c = comp(
        R48,
        vec![
            slot("A", vec![mono_clip(0, 0, 1_000, 0)]),
            slot("B", vec![mono_clip(1, 48_000, 1_000, 0)]),
        ],
    );
    c.markers.push(AafMarker {
        position: 48_000,
        name: "Hit".into(),
    });
    c.comments.push(("Artist".into(), "Someone".into()));
    let imp = map(&c, &sources, &[], &env(&audio, &t100, true), &no()).unwrap();
    let d = apply_to(before.clone(), seq, &imp, None, false);
    let doc = d.h.doc();
    let s = doc.sequence(seq).unwrap();
    assert_eq!(s.tempo, t100);
    assert_eq!(lane_names(&d), ["Here", "A", "B"]);
    assert_eq!(regions(&d)[2][0].0, t100.seconds_to_tick(1.0));
    // Into a project: no name or info, the marker added.
    assert_eq!(s.name, before.sequence(seq).unwrap().name);
    assert!(doc.settings().info.is_empty());
    assert_eq!(s.markers.len(), 1);
    assert!(!has(&d.report, ItemKind::Note, NO_MIDI));
    let mut h = d.h;
    assert_eq!(h.undo_label(), Some("Import AAF"));
    h.undo().unwrap();
    assert_eq!(to_json(h.doc()).unwrap(), json);
}

#[test]
fn new_project_refuses_a_project_with_tracks() {
    let (doc, seq) = new_doc();
    let mut h = History::new(doc);
    h.execute(
        &Command::Lane(LaneCmd::Add {
            seq,
            name: "Here".into(),
            index: None,
        }),
        &ViewFilter::default(),
    )
    .unwrap();
    let audio = mono_refs(1);
    let sources = online(&audio);
    let c = comp(R48, vec![slot("A", vec![mono_clip(0, 0, 1_000, 0)])]);
    let t120 = bpm(120);
    let imp = map(&c, &sources, &[], &env(&audio, &t120, true), &no()).unwrap();
    h.begin(LABEL).unwrap();
    let e = apply(
        &mut h,
        &imp,
        None,
        &AafPlace {
            seq,
            new_project: true,
        },
        &mut Report::default(),
    )
    .unwrap_err();
    assert!(e.to_string().contains(NOT_NEW_AAF), "{e}");
    h.abort().unwrap();
}

#[test]
fn offline_media_become_offline_items() {
    let audio = vec![AudioRef {
        source: AudioSource::Offline("MXF audio (VizDAW does not read it yet)".into()),
        ..aref(0, "Far", 1)
    }];
    let sources: BTreeMap<usize, MediaSource> = [(
        0,
        MediaSource::Offline {
            why: "MXF audio (VizDAW does not read it yet)".into(),
        },
    )]
    .into();
    let c = comp(R48, vec![slot("A", vec![mono_clip(0, 0, 1_000, 10)])]);
    let t120 = bpm(120);
    let imp = map(&c, &sources, &[], &env(&audio, &t120, true), &no()).unwrap();
    assert!(has(&imp.report, ItemKind::Note, OFFLINE));
    let d = applied(&imp);
    let EventBody::AudioRegion {
        media,
        source_offset,
        ..
    } = &regions(&d)[0][0].1
    else {
        unreachable!()
    };
    assert_eq!(*source_offset, 10);
    let m = d.h.doc().media().get(media).unwrap();
    assert_eq!(
        (m.path.as_str(), m.content_hash.as_ref()),
        ("Media/Far.caf", None)
    );
}

#[test]
fn listed() {
    // The exact texts.
    assert_eq!(
        POSITIONS_MOVED,
        "Positions moved to VizDAW's resolution by more than one sample"
    );
    assert_eq!(
        FADE_SHAPES,
        "Fade shapes VizDAW does not have became linear"
    );
    assert_eq!(
        CLIP_GAIN_ENVELOPES,
        "Clip gain envelopes became a constant gain (their first value)"
    );
    assert_eq!(
        NOT_JOINED,
        "Tracks named as left and right that were not joined (their clips differ)"
    );
    assert_eq!(
        NO_MIDI,
        "No MIDI file was found beside the AAF; positions use 120 BPM"
    );
    assert_eq!(
        timecode_note("01:00:00:00"),
        "The file's timecode starts at 01:00:00:00; it was placed at the start of the song"
    );
    assert_eq!(
        FROM_VIZDAW,
        "This file came from VizDAW; AAF files open as new projects (they cannot be merged)"
    );
    assert_eq!(
        SEVERAL_COMPOSITIONS,
        "The file holds several compositions; the first top-level one was imported"
    );
    assert_eq!(NON_AUDIO, "Picture and other non-audio tracks");
    assert_eq!(CLIP_PAN, "Clip pan (VizDAW has no per-region pan)");
    assert_eq!(
        SPANS_SOURCES,
        "Parts of clips that play past the end of their source"
    );
    assert_eq!(
        CURVES_LINEAR,
        "Volume and pan curves VizDAW does not have became linear"
    );
    assert_eq!(
        ONE_CHANNEL,
        "Clips that play one channel of a stereo file play both channels"
    );
    assert_eq!(
        CLIP_GAIN_CUT,
        "Clip gain above +24 dB was lowered to +24 dB"
    );

    // One fixture that gives every line.
    let mut fx = Fx::new();
    fx.product("VizDAW");
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 480_000));
    let (_, a_file) = fx.file_ids(&FileSpec::wav("Two", 48_000, 1, 480_000));
    let two = fx.new_id();
    fx.mob(
        core_io::aaf::dict::class::MASTER_MOB,
        &two,
        "Two",
        false,
        &[sound(
            1,
            "",
            R48,
            Seg::Seq(vec![clip(&a_file, 1, 0, 1_000), clip(&a_file, 1, 0, 1_000)]),
        )],
    );
    let st = fx.file(&FileSpec::wav("Stereo", 48_000, 2, 480_000));
    let gone = fx.new_id();
    let pan_op = |value: Value, input: Seg| Seg::Op {
        op: def::OPERATION_DEF_MONO_AUDIO_PAN,
        param: def::PARAMETER_DEF_PAN,
        value,
        input: Box::new(input),
    };
    let gain_op = |value: Value, input: Seg| Seg::Op {
        op: def::OPERATION_DEF_MONO_AUDIO_GAIN,
        param: def::PARAMETER_DEF_AMPLITUDE,
        value,
        input: Box::new(input),
    };
    let other_op = Auid::from_fields(0x1234_5678, 1, 2, [3; 8]);
    let faded = Seg::Clip(ClipSpec {
        mob: a.clone(),
        slot: 1,
        start: 0,
        len: 1_000,
        fade_in: None,
        fade_out: None,
    });
    fx.comp(
        "Main",
        false,
        &[
            // A clip at frame 48 005 (20 BPM moves it), a B-spline fade, a
            // varying clip gain, a clip gain over +24 dB, clip pan, a span,
            // a missing source, an effect, a selector and a bad value.
            sound(
                1,
                "Gtr L",
                R48,
                Seg::Seq(vec![
                    Seg::Fill(48_005),
                    gain_op(
                        Value::Points(
                            vec![((0, 1), (1, 2)), ((1, 1), (1, 1))],
                            def::INTERPOLATION_DEF_LINEAR,
                        ),
                        clip(&a, 1, 0, 1_000),
                    ),
                    Seg::Trans {
                        len: 100,
                        cut: None,
                        interp: def::INTERPOLATION_DEF_B_SPLINE,
                    },
                    Seg::Fill(1_000),
                    gain(20, 1, faded),
                    pan_op(Value::Const((1, 4)), clip(&a, 1, 0, 100)),
                    clip(&two, 1, 900, 200),
                    clip(&gone, 1, 0, 100),
                    Seg::Op {
                        op: other_op,
                        param: def::PARAMETER_DEF_LEVEL,
                        value: Value::Const((1, 1)),
                        input: Box::new(clip(&a, 1, 0, 100)),
                    },
                    Seg::Select(Box::new(clip(&a, 1, 0, 100)), vec![clip(&a, 1, 0, 100)]),
                    gain(1, 0, clip(&a, 1, 0, 100)),
                ]),
            ),
            // Named as the right of a pair; its clips differ.
            sound(2, "Gtr R", R48, Seg::Seq(vec![clip(&a, 1, 0, 1_000)])),
            // A gain of 8 with Log points, and a held pan cut to 2 points.
            sound(
                3,
                "Curves",
                R48,
                gain_op(
                    Value::Points(
                        vec![((0, 1), (8, 1)), ((1, 1), (1, 1))],
                        def::INTERPOLATION_DEF_LOG,
                    ),
                    pan_op(
                        Value::Points(
                            vec![((0, 1), (0, 1)), ((1, 2), (1, 1)), ((1, 1), (1, 2))],
                            def::INTERPOLATION_DEF_CONSTANT,
                        ),
                        Seg::Seq(vec![clip(&a, 1, 0, 48_000), Seg::Fill(48_000)]),
                    ),
                ),
            ),
            // Channel 2 of a stereo file, online as stereo.
            sound(4, "Right", R48, Seg::Seq(vec![clip(&st, 2, 0, 100)])),
            sound(5, "Bad", (48_000, 0), Seg::Seq(vec![clip(&a, 1, 0, 100)])),
            SlotSpec {
                id: 6,
                name: "V1".into(),
                rate: (25, 1),
                kind: SlotKind::Data(PICTURE, Seg::Seq(vec![Seg::Fill(100)])),
            },
            SlotSpec {
                id: 7,
                name: "TC".into(),
                rate: (25, 1),
                kind: SlotKind::Timecode(90_000, 25, false),
            },
        ],
    );
    fx.comp(
        "Other",
        false,
        &[sound(1, "O", R48, Seg::Seq(vec![clip(&a, 1, 0, 100)]))],
    );
    let limits = AafLimits {
        max_control_points: 2,
        ..AafLimits::default()
    };
    let f = open_with(&fx, &limits);
    assert_eq!(f.composition().name, "Main");
    let t20 = bpm(20);
    let sources = online(f.audio());
    let imp = map(
        f.composition(),
        &sources,
        &[],
        &env(f.audio(), &t20, true),
        &no(),
    )
    .unwrap();
    let d = applied(&imp);
    let r = &d.report;
    let want = [
        (ItemKind::Note, SEVERAL_COMPOSITIONS.to_owned()),
        (ItemKind::LeftOut, NON_AUDIO.to_owned()),
        (ItemKind::Changed, POSITIONS_MOVED.to_owned()),
        (ItemKind::Note, timecode_note("01:00:00:00")),
        (ItemKind::Changed, FADE_SHAPES.to_owned()),
        (ItemKind::Changed, CLIP_GAIN_ENVELOPES.to_owned()),
        (ItemKind::Changed, CLIP_GAIN_CUT.to_owned()),
        (ItemKind::Changed, LEVELS_CUT.to_owned()),
        (ItemKind::Changed, CURVES_LINEAR.to_owned()),
        (ItemKind::Changed, HELD_STEPS.to_owned()),
        (ItemKind::Changed, ONE_CHANNEL.to_owned()),
        (ItemKind::Note, NOT_JOINED.to_owned()),
        (ItemKind::Note, NO_MIDI.to_owned()),
        (ItemKind::Note, FROM_VIZDAW.to_owned()),
        (ItemKind::LeftOut, CLIP_PAN.to_owned()),
        (ItemKind::LeftOut, SPANS_SOURCES.to_owned()),
        (ItemKind::LeftOut, NO_SOURCE.to_owned()),
        (ItemKind::LeftOut, EFFECTS.to_owned()),
        (ItemKind::Note, SELECTED.to_owned()),
        (ItemKind::LeftOut, BAD_VALUES.to_owned()),
        (ItemKind::LeftOut, BAD_RATES.to_owned()),
        (ItemKind::LeftOut, POINTS_CUT.to_owned()),
    ];
    let missing: Vec<_> = want.iter().filter(|(k, w)| !has(r, *k, w)).collect();
    assert!(missing.is_empty(), "missing {missing:?} in {:#?}", r.items);
    assert!(core_model::rules::validate(d.h.doc()).is_empty());
}

#[test]
fn blank_names_fall_back() {
    let audio = mono_refs(1);
    let sources = online(&audio);
    let c = comp(
        R48,
        vec![slot(
            "  ",
            vec![Clip {
                name: Some(Arc::from("   ")),
                ..mono_clip(0, 0, 1_000, 0)
            }],
        )],
    );
    let t120 = bpm(120);
    let imp = map(&c, &sources, &[], &env(&audio, &t120, true), &no()).unwrap();
    let d = applied(&imp);
    assert_eq!(lane_names(&d), ["Track 1"]);
    let EventBody::AudioRegion { name, .. } = &regions(&d)[0][0].1 else {
        unreachable!()
    };
    assert_eq!(name, &None);
}
