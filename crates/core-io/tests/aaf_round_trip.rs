//! AAF round trips (plan 6f Task 4, spec §12: "our reader reads our
//! writer's files for every option"). A document is exported, written into
//! a temp folder with essence from in-memory WAVs, opened with
//! `AafFile::open`, paired, mapped with `Online` items standing for the
//! staged files (a joined pair's left item made stereo, as the job does),
//! and applied to a new project with the MIDI file beside it.
//!
//! **Tolerances**, on `projection(original)` and `projection(imported)`:
//! ticks, offsets, lengths, fades (Ruling 5's shapes), names and crossfade
//! flags exact; region gain within 0.001 dB; fader, pan and their points
//! within 1e-9. A region a later one starts inside keeps its audible part.
//!
//! **Tempos.** Positions come back as the same ticks when a tick is
//! coarser than a frame (spec §7.5: below about 187 BPM at 48 kHz) and the
//! MIDI file carries the tempo map exactly (whole microseconds per quarter,
//! changes on 960-PPQ ticks, no ramps). The property's tempos are drawn
//! from such values; ramps and fine positions are 6a's MIDI round trip.

mod common;

use common::aaf::{Projection, agree, projection, projection_until, wav_pcm, write_export};
use common::generators::*;
use core_engine::fixtures::{Builder, Q};
use core_io::aaf::export::{
    AafExport, AafOptions, EXPONENTIAL, ExportEnv, FADE_OUTS_CUT, Fps, MUTED_REGIONS, SCURVE,
    Stereo, export,
};
use core_io::aaf::import::{
    AafEnv, AafPlace, CLIP_GAIN_CUT, CLIP_GAIN_ENVELOPES, CURVES_LINEAR, FADE_SHAPES, LABEL,
    NOT_JOINED, ONE_CHANNEL, PAIR_MIX, POSITIONS_MOVED, apply, map, pairs, tempo_and_start,
};
use core_io::aaf::timeline::{
    AafFile, BAD_VALUES, CLIP_PAN, EFFECTS, FROM_VIZDAW, NO_SOURCE, SPANS_SOURCES,
};
use core_io::dawproject::export::{LeftOutAudio, PICKUP};
use core_io::dawproject::import::{
    AUDIO_CUT, AUDIO_PAST_FILE, AUDIO_TOO_SHORT, CROSSFADES_PLAIN, FADES_CUT, HELD_STEPS,
    LEVELS_CUT, MediaSource, PAST_END, TOO_MUCH,
};
use core_io::dawproject::package::{AudioPackage, AudioRef};
use core_io::limits::{AafLimits, Limits};
use core_io::midi_in::{ImportOptions, parse};
use core_io::report::{ItemKind, Report};
use core_media::import::Imported;
use core_model::automation::AutomationTarget;
use core_model::command::{
    AudioCmd, Command, CrossfadeLength, EventCmd, LaneCmd, MixerCmd, SequenceCmd,
};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::format::BitDepth;
use core_model::history::History;
use core_model::ids::{AnyId, SeqId};
use core_model::selection::ViewFilter;
use core_model::time::Tick;
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn no() -> AtomicBool {
    AtomicBool::new(false)
}

fn env() -> ExportEnv<'static> {
    ExportEnv {
        app_version: "1.2.3",
        export_id: "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
        modified: 1_700_000_000,
    }
}

fn defaults(seq: SeqId) -> AafOptions {
    AafOptions {
        seq,
        embed: true,
        depth: BitDepth::Int24,
        stereo: Stereo::MonoPairs,
        hour_start: false,
        fps: Fps::F25,
        clip_gain: true,
        automation: true,
        left_out: LeftOutAudio::new(),
    }
}

/// A new project: one sequence with no tracks.
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

/// A converted item standing for staged file `a`.
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

/// One trip's result.
struct Trip {
    x: AafExport,
    doc: Document,
    seq: SeqId,
    /// The import's report.
    report: Report,
}

/// `doc` exported with `o`, written, read and imported into a new project.
fn trip(doc: &Document, o: &AafOptions) -> Trip {
    let x = export(doc, o, &env(), &no()).expect("exports");
    let dir = tempfile::tempdir().unwrap();
    let bits = if o.depth == BitDepth::Int16 { 16 } else { 24 };
    if !o.embed {
        for s in &x.audio {
            let m = doc.media().get(&s.media).unwrap();
            let ch = if s.channel.is_some() { 1 } else { m.channels };
            let p = dir.path().join(&s.file);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, wav_pcm(m.sample_rate, ch, m.length_frames, bits)).unwrap();
        }
    }
    let p = write_export(doc, &x, dir.path(), bits);
    let midi = x.midi.as_ref().map(|bytes| {
        parse(
            bytes,
            &ImportOptions {
                split_channels: None,
                mpe: true,
            },
            &Limits::default(),
            &no(),
        )
        .expect("the MIDI file parses")
    });
    let f = AafFile::open(&p, None, &AafLimits::default(), &no()).expect("opens");
    let audio = f.audio().to_vec();
    for a in &audio {
        assert!(
            !matches!(
                a.source,
                core_io::dawproject::package::AudioSource::Offline(_)
            ),
            "{a:?} is offline"
        );
    }
    let mut sources: BTreeMap<usize, MediaSource> = audio
        .iter()
        .map(|a| (a.key, MediaSource::Online(vec![item_of(a, a.channels)])))
        .collect();
    let joined = pairs(f.composition(), &sources, true);
    for p in &joined {
        for (l, _) in &p.media {
            let a = audio.iter().find(|a| a.key == *l).unwrap();
            sources.insert(*l, MediaSource::Online(vec![item_of(a, 2)]));
        }
    }
    let (new, seq) = new_doc();
    let (tempo, start) = tempo_and_start(midi.as_ref(), new.sequence(seq).unwrap(), true).unwrap();
    let imp = map(
        f.composition(),
        &sources,
        &joined,
        &AafEnv {
            rate: 48_000,
            tempo: &tempo,
            start,
            join_pairs: true,
            audio: &audio,
        },
        &no(),
    )
    .expect("maps");
    let mut h = History::new(new);
    let mut report = Report::default();
    h.begin(LABEL).unwrap();
    apply(
        &mut h,
        &imp,
        midi.as_ref(),
        &AafPlace {
            seq,
            new_project: true,
        },
        &mut report,
    )
    .expect("applies");
    h.commit().unwrap();
    Trip {
        x,
        doc: h.doc().clone(),
        seq,
        report,
    }
}

/// The projection of a trip's import, within the original's length (what
/// played there; the import's own length can be shorter, its last point
/// then at its end).
fn back(doc: &Document, seq: SeqId, t: &Trip) -> Projection {
    let end = core_model::query::seq_length(doc, seq).unwrap().0;
    projection_until(&t.doc, t.seq, end)
}

fn has(r: &Report, kind: ItemKind, what: &str) -> bool {
    r.items.iter().any(|i| i.kind == kind && i.what == what)
}

/// The import lists nothing about the audio: no position, fade, gain,
/// pair or media line, and Ruling 12's Note.
fn clean_import(r: &Report) -> Result<(), String> {
    let bad = [
        POSITIONS_MOVED,
        FADE_SHAPES,
        CLIP_GAIN_ENVELOPES,
        CLIP_GAIN_CUT,
        CURVES_LINEAR,
        NOT_JOINED,
        PAIR_MIX,
        ONE_CHANNEL,
        CROSSFADES_PLAIN,
        FADES_CUT,
        LEVELS_CUT,
        HELD_STEPS,
        AUDIO_CUT,
        AUDIO_PAST_FILE,
        AUDIO_TOO_SHORT,
        PAST_END,
        TOO_MUCH,
        NO_SOURCE,
        EFFECTS,
        CLIP_PAN,
        SPANS_SOURCES,
        BAD_VALUES,
    ];
    if let Some(i) = r.items.iter().find(|i| bad.contains(&i.what.as_str())) {
        return Err(format!("the import listed {i:?}"));
    }
    if !has(r, ItemKind::Note, FROM_VIZDAW) {
        return Err("no FROM_VIZDAW".into());
    }
    Ok(())
}

/// The projection a trip with `o` keeps of `p`: no region gain without
/// clip gain; default strips without automation; a stereo track written as
/// mono pairs comes back centred (its halves are panned hard left and
/// right).
fn expected(p: &Projection, o: &AafOptions) -> Projection {
    let mut p = p.clone();
    for t in &mut p.tracks {
        if o.stereo == Stereo::MonoPairs && t.channels == 2 {
            t.pan = Some(0.0);
            t.pan_points.clear();
        }
        if !o.clip_gain {
            t.regions.iter_mut().for_each(|r| r.gain_db = 0.0);
        }
        if !o.automation {
            t.fader = Some(0.0);
            t.pan = Some(0.0);
            t.fader_points.clear();
            t.pan_points.clear();
        }
    }
    p
}

fn fade(frames: u64, shape: FadeShape) -> Fade {
    Fade {
        length_frames: frames,
        shape,
    }
}

// ---------------------------------------------------------------- every option

/// A fixed document: a MIDI track, a stereo track "Keys" with a faded,
/// gained and named region, a crossfaded pair and fader automation, a mono
/// track "Bass" panned left, and two markers.
fn fixed() -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane, _, _) = song(&mut b, "Lead");
    b.notes(seq, lane, &[(0, 60, 0x8000, Q), (4 * Q, 64, 0x8000, Q)])
        .unwrap();
    let (keys, keys_strip) = audio_track(&mut b, seq, "Keys");
    let (bass, bass_strip) = audio_track(&mut b, seq, "Bass");
    let wide = media(&mut b, "wide", 480_000, 2);
    let mono = media(&mut b, "mono", 480_000, 1);
    let r = b
        .region(
            seq,
            keys,
            0,
            wide,
            1_000,
            48_000,
            -6.0,
            fade(2_400, FadeShape::EqualPower),
            fade(4_800, FadeShape::Exponential),
        )
        .unwrap();
    b.run(Command::Audio(AudioCmd::SetName {
        event: r,
        name: Some("Intro".into()),
    }))
    .unwrap();
    for (k, tick) in [4 * Q, 5 * Q].into_iter().enumerate() {
        let e = b
            .region(
                seq,
                keys,
                tick,
                wide,
                0,
                72_000,
                0.0,
                Fade::default(),
                Fade::default(),
            )
            .unwrap();
        if k == 1 {
            b.run(Command::Audio(AudioCmd::Crossfade {
                event: e,
                length: CrossfadeLength::UpTo(4_800),
                shape: FadeShape::EqualPower,
            }))
            .unwrap();
        }
    }
    b.region(
        seq,
        bass,
        2 * Q,
        mono,
        0,
        96_000,
        3.0,
        Fade::default(),
        fade(960, FadeShape::Linear),
    )
    .unwrap();
    b.mixer(MixerCmd::SetPan {
        strip: bass_strip,
        pan: -0.5,
    })
    .unwrap();
    b.mixer(MixerCmd::SetGain {
        strip: bass_strip,
        gain_db: -3.5,
    })
    .unwrap();
    b.events(
        seq,
        keys,
        [(0, 0.5), (2 * Q, 0.75), (6 * Q, 0.25)]
            .into_iter()
            .map(|(t, v)| {
                EventSpec::new(
                    Tick(t),
                    EventBody::Automation {
                        target: AutomationTarget::StripFader(keys_strip),
                        value: v,
                    },
                )
            })
            .collect(),
    )
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: [(0, "Start"), (4 * Q, "Verse")]
            .into_iter()
            .map(|(t, n)| core_model::sequence::Marker {
                tick: Tick(t),
                name: n.into(),
            })
            .collect(),
    }))
    .unwrap();
    (b.finish(), seq)
}

#[test]
fn every_option_reads_back() {
    let (doc, seq) = fixed();
    let want = projection(&doc, seq);
    let mut sets = vec![defaults(seq)];
    let alone = |f: &dyn Fn(&mut AafOptions)| {
        let mut o = defaults(seq);
        f(&mut o);
        o
    };
    sets.push(alone(&|o| o.embed = false));
    sets.push(alone(&|o| o.depth = BitDepth::Int16));
    sets.push(alone(&|o| o.stereo = Stereo::Stereo));
    sets.push(alone(&|o| o.hour_start = true));
    sets.push(alone(&|o| o.fps = Fps::F24));
    sets.push(alone(&|o| o.fps = Fps::F2997Drop));
    sets.push(alone(&|o| o.fps = Fps::F30));
    sets.push(alone(&|o| o.clip_gain = false));
    sets.push(alone(&|o| o.automation = false));
    assert_eq!(sets.len(), 10);
    for o in &sets {
        let t = trip(&doc, o);
        let got = back(&doc, seq, &t);
        if let Err(e) = agree(&expected(&want, o), &got) {
            panic!("options {:?}: {e}", describe(o));
        }
        clean_import(&t.report).unwrap();
        assert!(
            has(&t.x.report, ItemKind::Changed, EXPONENTIAL),
            "{:?}",
            t.x.report
        );
    }
}

fn describe(o: &AafOptions) -> String {
    format!(
        "embed {} depth {:?} stereo {:?} hour {} fps {:?} clip gain {} automation {}",
        o.embed, o.depth, o.stereo, o.hour_start, o.fps, o.clip_gain, o.automation
    )
}

// ---------------------------------------------------------------- fixed cases

/// A song with a MIDI track "Lead" and one audio track "Keys"; `stereo`
/// picks its media's width.
fn keys_song(
    stereo: bool,
) -> (
    Builder,
    SeqId,
    core_model::ids::LaneId,
    core_model::ids::MediaId,
) {
    let mut b = Builder::new();
    let (seq, lane, _, _) = song(&mut b, "Lead");
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let (keys, _) = audio_track(&mut b, seq, "Keys");
    let m = media(&mut b, "keys", 480_000, if stereo { 2 } else { 1 });
    (b, seq, keys, m)
}

fn assert_trip(doc: &Document, seq: SeqId) -> Trip {
    let t = trip(doc, &defaults(seq));
    let want = expected(&projection(doc, seq), &defaults(seq));
    if let Err(e) = agree(&want, &back(doc, seq, &t)) {
        panic!("{e}");
    }
    clean_import(&t.report).unwrap();
    t
}

#[test]
fn a_crossfade_per_shape() {
    for shape in [
        FadeShape::Linear,
        FadeShape::EqualPower,
        FadeShape::Exponential,
        FadeShape::SCurve,
    ] {
        let (mut b, seq, keys, m) = keys_song(false);
        b.region(
            seq,
            keys,
            0,
            m,
            0,
            48_000,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
        let e = b
            .region(
                seq,
                keys,
                Q,
                m,
                0,
                48_000,
                0.0,
                Fade::default(),
                Fade::default(),
            )
            .unwrap();
        b.run(Command::Audio(AudioCmd::Crossfade {
            event: e,
            length: CrossfadeLength::UpTo(6_000),
            shape,
        }))
        .unwrap();
        let doc = b.finish();
        let t = assert_trip(&doc, seq);
        let p = projection(&t.doc, t.seq);
        assert!(p.tracks[0].regions[1].crossfade, "{shape:?}");
        assert_eq!(p.tracks[0].regions[0].length, 24_000 + 6_000, "{shape:?}");
        assert_eq!(
            has(&t.x.report, ItemKind::Changed, EXPONENTIAL),
            shape == FadeShape::Exponential
        );
        assert_eq!(
            has(&t.x.report, ItemKind::Changed, SCURVE),
            shape == FadeShape::SCurve
        );
    }
}

#[test]
fn overlaps_without_crossfade() {
    let (mut b, seq, keys, m) = keys_song(false);
    // A plays 0..96 000 frames with a fade-out; B starts inside it and ends
    // before it, so A's rest plays after B.
    b.region(
        seq,
        keys,
        0,
        m,
        500,
        96_000,
        0.0,
        Fade::default(),
        fade(2_400, FadeShape::Linear),
    )
    .unwrap();
    b.region(
        seq,
        keys,
        Q,
        m,
        7,
        12_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let doc = b.finish();
    let t = trip(&doc, &defaults(seq));
    clean_import(&t.report).unwrap();
    let p = projection(&t.doc, t.seq);
    let r: Vec<(i64, u64, u64, u64)> = p.tracks[0]
        .regions
        .iter()
        .map(|r| (r.tick, r.source_offset, r.length, r.fade_out.0))
        .collect();
    // 24 000 frames is one beat at 120 BPM; A's rest starts at 36 000.
    assert_eq!(
        r,
        [
            (0, 500, 24_000, 0),
            (Q, 7, 12_000, 0),
            (3 * Q / 2, 500 + 36_000, 60_000, 2_400),
        ]
    );
    assert!(!has(&t.x.report, ItemKind::Changed, FADE_OUTS_CUT));
}

#[test]
fn a_muted_region_is_left_out() {
    let (mut b, seq, keys, m) = keys_song(false);
    b.region(
        seq,
        keys,
        0,
        m,
        0,
        24_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let e = b
        .region(
            seq,
            keys,
            4 * Q,
            m,
            0,
            24_000,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    b.run(Command::Event(EventCmd::SetMute {
        events: vec![e],
        mute: true,
    }))
    .unwrap();
    let doc = b.finish();
    let t = assert_trip(&doc, seq);
    assert_eq!(projection(&t.doc, t.seq).tracks[0].regions.len(), 1);
    let item =
        t.x.report
            .items
            .iter()
            .find(|i| i.what == MUTED_REGIONS)
            .unwrap();
    assert_eq!(
        (item.kind, item.details.clone()),
        (ItemKind::LeftOut, vec!["Keys".to_string()])
    );
}

#[test]
fn a_pickup_comes_back() {
    let (mut b, seq, keys, m) = keys_song(false);
    b.run(Command::Sequence(SequenceCmd::SetStartOffset {
        seq,
        offset: Tick(Q),
    }))
    .unwrap();
    b.region(
        seq,
        keys,
        -Q,
        m,
        0,
        24_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    b.region(
        seq,
        keys,
        Q,
        m,
        0,
        24_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let doc = b.finish();
    let t = assert_trip(&doc, seq);
    assert!(has(&t.x.report, ItemKind::Note, PICKUP));
    assert_eq!(projection(&t.doc, t.seq).tracks[0].regions[0].tick, -Q);
}

#[test]
fn mono_pairs_join_back() {
    let (mut b, seq, keys, m) = keys_song(true);
    b.region(
        seq,
        keys,
        0,
        m,
        100,
        24_000,
        -6.0,
        fade(480, FadeShape::Linear),
        Fade::default(),
    )
    .unwrap();
    let doc = b.finish();
    let t = assert_trip(&doc, seq);
    let p = projection(&t.doc, t.seq);
    assert_eq!((p.tracks.len(), p.tracks[0].channels), (1, 2));
    assert_eq!(p.tracks[0].name, "Keys");
}

#[test]
fn notes_and_audio_on_one_track() {
    let (mut b, seq, keys, m) = keys_song(false);
    let (dest, _) = b.plugin_dest("Both", clap("org.example.both")).unwrap();
    b.run(Command::Lane(LaneCmd::SetDest {
        seq,
        lane: keys,
        dest: Some(dest),
    }))
    .unwrap();
    b.notes(seq, keys, &[(2 * Q, 67, 0x8000, Q)]).unwrap();
    b.region(
        seq,
        keys,
        0,
        m,
        0,
        24_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let doc = b.finish();
    let t = assert_trip(&doc, seq);
    // The notes are in the MIDI file, and came back on a MIDI track.
    let s = t.doc.sequence(t.seq).unwrap();
    let notes: Vec<i64> = s
        .lanes
        .iter()
        .filter(|l| l.audio_out.is_none())
        .flat_map(|l| l.active().unwrap().events.values())
        .filter(|e| matches!(e.body, EventBody::Note { pitch: 67, .. }))
        .map(|e| e.tick.0)
        .collect();
    assert_eq!(notes, [2 * Q]);
}

#[test]
fn a_thousand_tracks() {
    let mut b = Builder::new();
    let (seq, lane, _, _) = song(&mut b, "Lead");
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let m = media(&mut b, "short", 4_800, 1);
    for i in 0..1_000 {
        let (l, _) = audio_track(&mut b, seq, &format!("T{i}"));
        b.region(
            seq,
            l,
            (i % 8) * Q,
            m,
            0,
            4_800,
            0.0,
            Fade::default(),
            Fade::default(),
        )
        .unwrap();
    }
    let doc = b.finish();
    // Export, write, read and map: the AAF's own work.
    let t0 = Instant::now();
    let o = defaults(seq);
    let x = export(&doc, &o, &env(), &no()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let p = write_export(&doc, &x, dir.path(), 24);
    let f = AafFile::open(&p, None, &AafLimits::default(), &no()).unwrap();
    let sources: BTreeMap<usize, MediaSource> = f
        .audio()
        .iter()
        .map(|a| (a.key, MediaSource::Online(vec![item_of(a, a.channels)])))
        .collect();
    let (new, nseq) = new_doc();
    let tempo = core_model::tempo::TempoMap::constant(core_model::tempo::Bpm::whole(120)).unwrap();
    let imp = map(
        f.composition(),
        &sources,
        &[],
        &AafEnv {
            rate: 48_000,
            tempo: &tempo,
            start: Tick(0),
            join_pairs: true,
            audio: f.audio(),
        },
        &no(),
    )
    .unwrap();
    let took = t0.elapsed();
    assert_eq!(f.composition().slots.len(), 1_000);
    assert!(took < Duration::from_secs(10), "{took:?}");
    drop((imp, new, nseq));
    // The whole trip (6d's apply adds the tracks one by one; untimed).
    let t = assert_trip(&doc, seq);
    assert_eq!(projection(&t.doc, t.seq).tracks.len(), 1_000);
}

// ---------------------------------------------------------------- the property

/// Tempos a MIDI file carries exactly and whose tick is coarser than a
/// frame at 48 kHz.
const TEMPOS: [u32; 9] = [60, 75, 80, 96, 100, 120, 125, 150, 160];

fn trip_spec() -> impl Strategy<Value = Spec> {
    (
        proptest::collection::vec(audio_track_spec(), 1..3),
        proptest::sample::select(TEMPOS.to_vec()),
        proptest::collection::btree_map(1i64..32, proptest::sample::select(TEMPOS.to_vec()), 0..3),
        0u8..4,
        proptest::collection::btree_map(0i64..24, word(), 0..3),
    )
        .prop_map(|(audio, bpm0, changes, pickup, markers)| {
            let mut tempo = vec![(0, bpm0, false)];
            tempo.extend(changes.into_iter().map(|(beat, b)| (beat * Q, b, false)));
            let quiet = StripSpec {
                gain: 0,
                pan: 0,
                mute: false,
                solo: false,
                colour: None,
                send: None,
                insert: None,
                state: None,
            };
            Spec {
                notes: vec![NoteTrack {
                    strip: quiet,
                    quantize: None,
                    notes: vec![NoteSpec {
                        tick: 0,
                        pitch: 60,
                        velocity: 0x8000,
                        release: 0,
                        duration: Q,
                        channel: None,
                        expression: Vec::new(),
                    }],
                    midi: Vec::new(),
                    automation: Vec::new(),
                }],
                audio,
                bus: None,
                master_gain: 0,
                meter: (4, 4),
                change: None,
                tempo,
                pickup_beats: pickup,
                markers: markers.into_iter().map(|(b, n)| (b * Q, n)).collect(),
                info: Vec::new(),
                gain: true,
            }
        })
}

/// Whether a written region of `doc` has a fade of `shape`.
fn any_shape(doc: &Document, seq: SeqId, shape: FadeShape) -> bool {
    doc.sequence(seq).unwrap().lanes.iter().any(|l| {
        l.active().is_some_and(|t| {
            t.events.values().any(|e| match &e.body {
                EventBody::AudioRegion {
                    fade_in, fade_out, ..
                } if !e.mute => [fade_in, fade_out]
                    .iter()
                    .any(|f| f.length_frames > 0 && f.shape == shape),
                _ => false,
            })
        })
    })
}

fn any_muted(doc: &Document, seq: SeqId) -> bool {
    doc.sequence(seq).unwrap().lanes.iter().any(|l| {
        l.active().is_some_and(|t| {
            t.events
                .values()
                .any(|e| e.mute && matches!(e.body, EventBody::AudioRegion { .. }))
        })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(common::cases(64)))]

    #[test]
    fn round_trips_preserve_the_projection(spec in trip_spec()) {
        let (doc, seq) = build(&spec);
        let t = trip(&doc, &defaults(seq));
        let want = expected(&projection(&doc, seq), &defaults(seq));
        let got = back(&doc, seq, &t);
        if let Err(e) = agree(&want, &got) {
            prop_assert!(false, "{}", e);
        }
        if let Err(e) = clean_import(&t.report) {
            prop_assert!(false, "{}", e);
        }
        let r = &t.x.report;
        prop_assert_eq!(has(r, ItemKind::Changed, EXPONENTIAL), any_shape(&doc, seq, FadeShape::Exponential));
        prop_assert_eq!(has(r, ItemKind::Changed, SCURVE), any_shape(&doc, seq, FadeShape::SCurve));
        prop_assert_eq!(has(r, ItemKind::LeftOut, MUTED_REGIONS), any_muted(&doc, seq));
        prop_assert_eq!(has(r, ItemKind::Note, PICKUP), spec.pickup_beats > 0);
        prop_assert!(!has(r, ItemKind::Changed, FADE_OUTS_CUT));
    }
}
