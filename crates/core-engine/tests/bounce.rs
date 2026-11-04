//! The bounce renderer (plan 5f-2 Task 2; spec §9.1, §12.3; Rulings 4, 5,
//! 8, 13; pre-flight C1, I3, I6, I8).

use core_engine::backend::{AudioBackend, MidiPortInfo};
use core_engine::bounce::{
    BounceSpec, BounceStats, HeardStem, SilentMidi, TAIL_QUIET_SECONDS, bounce,
};
use core_engine::engine::{Engine, EngineConfig, SinkFactory};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile};
use core_model::automation::AutomationTarget;
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec, Fade};
use core_model::format::BitDepth;
use core_model::ids::{MediaId, SeqId, StripId};
use core_model::mixer::{AuxSend, Output, StripKind};
use core_model::time::Tick;
use std::path::Path;
use std::sync::atomic::AtomicBool;

const QF: u64 = 24_000;
const FRAMES: u64 = 480_000;
const BLOCK: u32 = 512;

fn value(f: u64) -> f32 {
    ((f % 997) as f32 + 1.0) / 2000.0
}

fn caf(path: &Path) {
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    let data: Vec<f32> = (0..FRAMES).flat_map(|f| [value(f), -value(f)]).collect();
    w.write(&data).unwrap();
    w.finish().unwrap();
}

#[derive(Default)]
struct Opts {
    /// A bus "Latent" with a `latency=` insert (PDC leads on the region's path).
    latency: u32,
    /// An insert on master.
    master: Option<&'static str>,
    /// A post-fader send from the region's path to "Latent".
    send: bool,
    /// A fader ramp on the region's path from beat 1 to beat 5 (a point every
    /// eighth beat: runs split inside blocks), held after.
    ramp: bool,
    /// A written mute of the region's path at this tick.
    mute_at: Option<i64>,
    /// Notes on the built-in synth (its test tone).
    synth: bool,
}

/// One region at beat 1 (frames 100.. of the file) for 6 beats, plus `o`.
fn doc_with(o: Opts) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FRAMES, 2).unwrap();
    b.region(
        seq,
        lane,
        Q,
        m,
        100,
        6 * QF,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let path = path_and_bus(b.doc()).0;
    if o.latency > 0 {
        let bus = b.bus("Latent").unwrap();
        b.insert(bus, TestInserts::plugin(&format!("latency={}", o.latency)))
            .unwrap();
        if o.send {
            b.mixer(MixerCmd::AddSend {
                strip: path,
                send: AuxSend::new(bus),
            })
            .unwrap();
        }
    }
    if let Some(p) = o.master {
        let ms = b.strip_of(StripKind::Master).unwrap();
        b.insert(ms, TestInserts::plugin(p)).unwrap();
    }
    let mut auto = Vec::new();
    if o.ramp {
        for k in 8..=40i64 {
            auto.push(EventSpec::new(
                Tick(k * Q / 8),
                EventBody::Automation {
                    target: AutomationTarget::StripFader(path),
                    value: 0.5 + (k % 8) as f64 / 16.0,
                },
            ));
        }
    }
    if let Some(t) = o.mute_at {
        for (tick, v) in [(0, 0.0), (t, 1.0)] {
            auto.push(EventSpec::new(
                Tick(tick),
                EventBody::Automation {
                    target: AutomationTarget::StripMute(path),
                    value: v,
                },
            ));
        }
    }
    if !auto.is_empty() {
        let l = b.lane(seq, "Auto", None).unwrap();
        b.events(seq, l, auto).unwrap();
    }
    if o.synth {
        let d = b.dest("Synth", "builtin:synth", 0).unwrap();
        let l = b.lane(seq, "Keys", Some(d)).unwrap();
        b.notes(
            seq,
            l,
            &[(Q, 60, 0x8000, 4 * Q), (2 * Q, 67, 0x8000, 2 * Q)],
        )
        .unwrap();
    }
    (b.finish(), seq)
}

fn doc(latency: u32, master: Option<&'static str>) -> (Document, SeqId) {
    doc_with(Opts {
        latency,
        master,
        ..Opts::default()
    })
}

struct Rig {
    engine: Engine,
    out: OfflineBackend,
    _dir: tempfile::TempDir,
}

fn rig_with(doc: &Document, seq: SeqId, sinks: Box<dyn SinkFactory>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"));
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = DiskMode::Manual;
    let (mut engine, renderer) = Engine::new(cfg, sinks).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_media(vec![MediaFile {
        id: MediaId(0),
        path: dir.path().join("a.caf"),
    }]);
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    Rig {
        engine,
        out,
        _dir: dir,
    }
}

fn rig(doc: &Document, seq: SeqId) -> Rig {
    rig_with(doc, seq, Box::new(OfflineSinks::new(64)))
}

/// Every source's left and right channels; the sink must receive exactly
/// `stats.frames` frames per source (Ruling 4, pre-flight I8).
#[allow(clippy::type_complexity)]
fn run_lr(r: &mut Rig, spec: &BounceSpec) -> (Vec<(Vec<f32>, Vec<f32>)>, BounceStats) {
    let sources = usize::from(spec.mix) + spec.stems.len() + spec.heard_stems.len();
    let mut got = vec![(Vec::new(), Vec::new()); sources];
    let stats = bounce(
        &mut r.engine,
        &mut r.out,
        spec,
        BLOCK,
        &mut |k, l, rt| {
            got[k].0.extend_from_slice(l);
            got[k].1.extend_from_slice(rt);
            Ok(())
        },
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    for (k, g) in got.iter().enumerate() {
        assert_eq!(
            g.0.len() as u64,
            stats.frames,
            "source {k}: exactly the bounce's frames"
        );
        assert_eq!(g.1.len() as u64, stats.frames);
    }
    (got, stats)
}

/// The left channels.
fn run(r: &mut Rig, spec: &BounceSpec) -> (Vec<Vec<f32>>, BounceStats) {
    let (got, stats) = run_lr(r, spec);
    (got.into_iter().map(|g| g.0).collect(), stats)
}

fn spec(end_beats: i64) -> BounceSpec {
    BounceSpec {
        mix: true,
        stems: vec![],
        heard_stems: vec![],
        start: Tick::ZERO,
        end: Tick(end_beats * Q),
        tail_cap: 480_000,
    }
}

/// The region's AudioPath strip and (once added) the "Latent" bus.
fn path_and_bus(d: &Document) -> (StripId, StripId) {
    let find = |want: fn(&StripKind) -> bool| {
        d.mixer()
            .order()
            .iter()
            .copied()
            .find(|s| d.mixer().strip(*s).is_ok_and(|x| want(&x.kind)))
    };
    (
        find(|k| matches!(k, StripKind::AudioPath(_))).unwrap(),
        find(|k| matches!(k, StripKind::Bus)).unwrap_or(StripId(u64::MAX)),
    )
}

#[test]
fn the_mix_is_the_region_as_heard_with_and_without_latency() {
    for l in [0, 1000] {
        let (d, seq) = doc(l, None);
        let mut r = rig(&d, seq);
        let (got, stats) = run(&mut r, &spec(8));
        let mix = &got[0];
        assert_eq!(stats.range, 8 * QF);
        assert_eq!(
            stats.frames,
            8 * QF,
            "silence after the range: no tail (L = {l})"
        );
        assert!(
            mix[..QF as usize].iter().all(|x| *x == 0.0),
            "nothing before beat 1 (L = {l})"
        );
        let k = mix[QF as usize] / value(100);
        assert!(k > 0.1, "the strip's pan law is a constant");
        for i in 0..(6 * QF) {
            let want = k * value(100 + i);
            assert!(
                (mix[(QF + i) as usize] - want).abs() < 1e-6,
                "L = {l}, frame {i}"
            );
        }
    }
}

/// Pre-flight C1: the range ends mid-block (7.5 beats), a fader ramp splits
/// runs inside blocks, and a send feeds the latent bus; every frame of the
/// whole file lines up.
#[test]
fn stems_line_up_and_add_up_to_the_mix() {
    let (d, seq) = doc_with(Opts {
        latency: 1000,
        send: true,
        ramp: true,
        ..Opts::default()
    });
    let (path, bus) = path_and_bus(&d);
    let mut r = rig(&d, seq);
    let end = BounceSpec {
        stems: vec![path, bus],
        end: Tick(15 * Q / 2),
        ..spec(0)
    };
    let (got, stats) = run(&mut r, &end);
    assert_eq!(stats.range, 15 * QF / 2);
    assert_ne!(
        stats.range % u64::from(BLOCK),
        0,
        "the range ends inside a block"
    );
    for (k, stem) in got[1..].iter().enumerate() {
        assert!(
            stem[..QF as usize].iter().all(|x| *x == 0.0),
            "stem {k} starts at beat 1 too"
        );
        assert!(stem[QF as usize] != 0.0, "stem {k} sounds at beat 1");
    }
    for f in 0..got[0].len() {
        let sum: f32 = got[1..].iter().map(|s| s[f]).sum();
        assert!(
            (sum - got[0][f]).abs() < 1e-5,
            "frame {f}: stems sum to the mix at 0 dB master"
        );
    }
    // The ramp really moved the fader: the path's stem is not a constant times the media.
    let k0 = got[1][QF as usize] / value(100);
    // Beat 3.5 is the point k = 28 (0.75); beat 1 is k = 8 (0.5).
    let k1 = got[1][(7 * QF / 2) as usize] / value(100 + 5 * QF / 2);
    assert!((k0 - k1).abs() > 1e-3, "{k0} vs {k1}");
}

/// Spec §12.3 "a bounce equals an offline render", at engine level and
/// deterministic (workers 0, deadline None): the mix equals a plain
/// play-and-render of the same document with its first L frames dropped.
/// Master's `gain=0.5` insert keeps the plain render below its clamp (the
/// ramp's fader values reach above unity: unhalved it peaks at 1.15).
#[test]
fn a_bounce_equals_a_plain_offline_render() {
    let (d, seq) = doc_with(Opts {
        latency: 700,
        master: Some("gain=0.5"),
        ramp: true,
        synth: true,
        ..Opts::default()
    });
    let end = Tick(15 * Q / 2);
    let mut a = rig_with(&d, seq, Box::new(OfflineSinks::new(1 << 12).with_tone()));
    let (got, stats) = run(&mut a, &BounceSpec { end, ..spec(0) });
    let mut b = rig_with(&d, seq, Box::new(OfflineSinks::new(1 << 12).with_tone()));
    b.engine.play().unwrap();
    let l = b.engine.latency_frames();
    assert_eq!(l, 700);
    let mut plain = Vec::new();
    while (plain.len() as u64) < l + stats.range {
        b.engine.streamer().service();
        b.out.block(BLOCK);
        plain.extend_from_slice(b.out.output(BLOCK).0);
    }
    let plain = &plain[l as usize..(l + stats.range) as usize];
    assert!(plain.iter().any(|x| *x != 0.0));
    assert!(
        plain.iter().all(|x| x.abs() < 1.0),
        "below the clamp, so the two compare"
    );
    for (f, (x, y)) in got[0].iter().zip(plain).enumerate() {
        assert_eq!(x, y, "frame {f}");
    }
}

/// Owner answers 2026-11-02 and 2026-11-03 (Ruling 5): a heard stem (Add to
/// project) respects mute and solo along its way to master, also through a
/// muted bus; a file stem does not; unmuted they are equal.
#[test]
fn heard_stems_respect_mute_and_solo_and_file_stems_do_not() {
    for how in ["mute", "solo another strip", "mute the bus it plays into"] {
        let (d, seq) = doc(1000, None);
        let (path, bus) = path_and_bus(&d);
        let mut b = Builder::from_doc(d);
        match how {
            "mute" => b.mixer(MixerCmd::SetMute {
                strip: path,
                mute: true,
            }),
            "solo another strip" => b.mixer(MixerCmd::SetSolo {
                strip: bus,
                solo: true,
            }),
            _ => {
                b.mixer(MixerCmd::SetOutput {
                    strip: path,
                    output: Output::Bus(bus),
                })
                .unwrap();
                b.mixer(MixerCmd::SetMute {
                    strip: bus,
                    mute: true,
                })
            }
        }
        .unwrap();
        let d = b.finish();
        let heard = HeardStem::of(d.mixer(), path);
        if how == "mute the bus it plays into" {
            assert!(heard.chain.contains(&bus), "{heard:?}");
        }
        let mut r = rig(&d, seq);
        let (got, stats) = run(
            &mut r,
            &BounceSpec {
                stems: vec![path],
                heard_stems: vec![heard],
                ..spec(8)
            },
        );
        assert_eq!(stats.frames, 8 * QF, "{how}");
        assert!(
            got[0].iter().all(|x| *x == 0.0),
            "{how}: the mix is as heard"
        );
        assert!(
            got[1][QF as usize] != 0.0,
            "{how}: the file stem ignores it"
        );
        assert!(
            got[2].iter().all(|x| *x == 0.0),
            "{how}: the heard stem is silent"
        );
    }
    let (d, seq) = doc(1000, None);
    let (path, _) = path_and_bus(&d);
    let mut r = rig(&d, seq);
    let heard = HeardStem::of(d.mixer(), path);
    let (got, _) = run(
        &mut r,
        &BounceSpec {
            stems: vec![path],
            heard_stems: vec![heard],
            ..spec(8)
        },
    );
    assert!(got[2][QF as usize] != 0.0);
    assert_eq!(
        got[1], got[2],
        "unmuted, the heard stem is the file stem, sample for sample"
    );
}

/// Pre-flight C1: a mute written inside a block cuts the heard stem where
/// it cuts the mix; the file stem plays on.
#[test]
fn a_written_mute_cuts_the_heard_stem_where_it_cuts_the_mix() {
    // 3 beats + 320 ticks = frame 72 500, inside block 141.
    let at = 3 * Q + 320;
    let (d, seq) = doc_with(Opts {
        latency: 1000,
        mute_at: Some(at),
        ..Opts::default()
    });
    let (path, _) = path_and_bus(&d);
    let mut r = rig(&d, seq);
    let heard = HeardStem::of(d.mixer(), path);
    let (got, _) = run(
        &mut r,
        &BounceSpec {
            stems: vec![path],
            heard_stems: vec![heard],
            ..spec(8)
        },
    );
    let (mix, file, inplace) = (&got[0], &got[1], &got[2]);
    let cut = (QF as usize..mix.len())
        .find(|f| mix[*f..].iter().all(|x| *x == 0.0))
        .unwrap();
    assert!(
        cut.abs_diff(72_500) <= 32,
        "the mix cuts at the written mute: {cut}"
    );
    assert_ne!(cut % BLOCK as usize, 0, "inside a block");
    assert!(
        inplace[cut - 1] != 0.0 && inplace[cut..].iter().all(|x| *x == 0.0),
        "the heard stem cuts at {cut} too"
    );
    assert_eq!(
        &inplace[..cut],
        &file[..cut],
        "and is the file stem before it"
    );
    assert!(
        file[cut] != 0.0 && file[(6 * QF) as usize] != 0.0,
        "the file stem plays on"
    );
}

/// Owner answer 3 (Ruling 5): a mute written on the latent bus a track
/// plays into cuts the track's heard stem on the mix's own frame: each
/// chain strip's gate is read at its own feed arrival (here 1000 frames
/// after the track's). The file stem plays on.
#[test]
fn a_written_bus_mute_cuts_the_heard_stem_where_it_cuts_the_mix() {
    let (d, seq) = doc(1000, None);
    let (path, bus) = path_and_bus(&d);
    let mut b = Builder::from_doc(d);
    b.mixer(MixerCmd::SetOutput {
        strip: path,
        output: Output::Bus(bus),
    })
    .unwrap();
    let l = b.lane(seq, "Auto", None).unwrap();
    let at = 3 * Q + 320;
    let points = [(0, 0.0), (at, 1.0)]
        .into_iter()
        .map(|(t, v)| {
            EventSpec::new(
                Tick(t),
                EventBody::Automation {
                    target: AutomationTarget::StripMute(bus),
                    value: v,
                },
            )
        })
        .collect();
    b.events(seq, l, points).unwrap();
    let d = b.finish();
    let heard = HeardStem::of(d.mixer(), path);
    assert!(heard.chain.contains(&bus), "{heard:?}");
    let mut r = rig(&d, seq);
    let (got, _) = run(
        &mut r,
        &BounceSpec {
            stems: vec![path],
            heard_stems: vec![heard],
            ..spec(8)
        },
    );
    let (mix, file, inplace) = (&got[0], &got[1], &got[2]);
    let cut = (QF as usize..mix.len())
        .find(|f| mix[*f..].iter().all(|x| *x == 0.0))
        .unwrap();
    assert!(
        cut.abs_diff(72_500) <= 32,
        "the mix cuts at the bus's written mute: {cut}"
    );
    assert!(
        inplace[cut - 1] != 0.0 && inplace[cut..].iter().all(|x| *x == 0.0),
        "the heard stem cuts at {cut} too"
    );
    assert_eq!(
        &inplace[..cut],
        &file[..cut],
        "and is the file stem before it"
    );
    assert!(file[cut] != 0.0, "the file stem plays on");
}

#[test]
fn a_silent_end_adds_no_tail() {
    let (d, seq) = doc(0, None);
    let mut r = rig(&d, seq);
    let (_, stats) = run(&mut r, &spec(7));
    assert_eq!(stats.tail, 0, "the range ends where the region does");
    assert_eq!(stats.frames, 7 * QF);
}

#[test]
fn a_ring_is_kept_to_its_last_loud_frame() {
    let (d, seq) = doc(0, Some("ring=12000"));
    let mut r = rig(&d, seq);
    let (got, stats) = run(&mut r, &spec(7));
    assert_eq!(
        stats.tail, 12_000,
        "the ring after the region's end at the range end"
    );
    assert!(got[0][(stats.frames - 1) as usize].abs() > 0.1);
    assert!(stats.frames < 7 * QF + 12_000 + (TAIL_QUIET_SECONDS * 48_000.0) as u64);
}

#[test]
fn endless_noise_stops_at_the_cap() {
    let (d, seq) = doc(0, Some("noise=3"));
    let mut r = rig(&d, seq);
    let (_, stats) = run(
        &mut r,
        &BounceSpec {
            tail_cap: 30_000,
            ..spec(7)
        },
    );
    assert_eq!(stats.tail, 30_000);
    assert_eq!(stats.frames, 7 * QF + 30_000);
}

#[test]
fn a_zero_cap_is_the_range() {
    let (d, seq) = doc(0, Some("noise=3"));
    let mut r = rig(&d, seq);
    let (_, stats) = run(
        &mut r,
        &BounceSpec {
            tail_cap: 0,
            ..spec(7)
        },
    );
    assert_eq!(stats.frames, 7 * QF);
}

#[test]
fn a_selection_starts_mid_song() {
    let (d, seq) = doc(1000, None);
    let mut r = rig(&d, seq);
    // A stem too: it sounds across the end-of-playing split (120 000 frames
    // is not a block multiple), so every run of the last block is in it.
    let stem = path_and_bus(&d).0;
    let (got, stats) = run(
        &mut r,
        &BounceSpec {
            stems: vec![stem],
            start: Tick(3 * Q),
            end: Tick(5 * Q),
            ..spec(0)
        },
    );
    assert_eq!(stats.range, 2 * QF);
    assert_ne!(stats.range % u64::from(BLOCK), 0);
    let k = got[0][0] / value(100 + 2 * QF);
    let ks = got[1][0] / value(100 + 2 * QF);
    for i in 0..(2 * QF) {
        assert!(
            (got[0][i as usize] - k * value(100 + 2 * QF + i)).abs() < 1e-6,
            "frame {i}"
        );
        assert!(
            (got[1][i as usize] - ks * value(100 + 2 * QF + i)).abs() < 1e-6,
            "stem frame {i}"
        );
    }
}

/// Owner answer 1, Ruling 13: the mix is read before the master's hard
/// clamp, both signs.
#[test]
fn a_hot_mix_keeps_its_overs_before_the_clamp() {
    let (d, seq) = doc(0, Some("gain=8"));
    let mut r = rig(&d, seq);
    let (got, stats) = run_lr(&mut r, &spec(7));
    let (l, rt) = &got[0];
    assert!(l.iter().any(|x| *x > 1.0), "positive overs kept");
    assert!(rt.iter().any(|x| *x < -1.0), "negative overs kept");
    assert_eq!(stats.nonfinite, 0);
}

/// Ruling 8 (pre-flight I3): wrapped in `SilentMidi`, a lane on an external
/// MIDI destination sends nothing; the same document unwrapped does.
#[test]
fn external_midi_is_silent_in_a_bounce() {
    let mut b = Builder::new();
    let ext = b.dest("Hardware", "Ext Port", 0).unwrap();
    let (seq, lane) = b.sequence("Song", "Keys", Some(ext)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q), (2 * Q, 64, 0x8000, Q)])
        .unwrap();
    let d = b.finish();
    let port = || {
        OfflineSinks::new(256).with_ports(vec![MidiPortInfo {
            name: "Ext Port".into(),
            unique_id: None,
        }])
    };
    let plain = port();
    let log = plain.log();
    let mut r = rig_with(&d, seq, Box::new(plain));
    run(&mut r, &spec(4));
    assert!(
        log.drain().iter().any(|x| x.bytes().first() == Some(&0x90)),
        "unwrapped, the note-ons go out"
    );
    let silent = port();
    let log = silent.log();
    let mut r = rig_with(&d, seq, Box::new(SilentMidi(silent)));
    let (_, stats) = run(&mut r, &spec(4));
    assert_eq!(stats.range, 4 * QF);
    assert!(log.drain().is_empty(), "nothing reaches the destination");
}

#[test]
fn cancel_and_an_empty_range_stop_cleanly() {
    let (d, seq) = doc(0, None);
    let mut r = rig(&d, seq);
    let e = bounce(
        &mut r.engine,
        &mut r.out,
        &spec(8),
        BLOCK,
        &mut |_, _, _| Ok(()),
        &mut |_| {},
        &AtomicBool::new(true),
    );
    assert!(matches!(
        e,
        Err(core_engine::bounce::BounceError::Cancelled)
    ));
    let e = bounce(
        &mut r.engine,
        &mut r.out,
        &BounceSpec {
            start: Tick(Q),
            end: Tick(Q),
            ..spec(8)
        },
        BLOCK,
        &mut |_, _, _| Ok(()),
        &mut |_| {},
        &AtomicBool::new(false),
    );
    assert!(matches!(e, Err(core_engine::bounce::BounceError::Empty)));
    let e = bounce(
        &mut r.engine,
        &mut r.out,
        &spec(8),
        BLOCK,
        &mut |_, _, _| Err("disk full".into()),
        &mut |_| {},
        &AtomicBool::new(false),
    );
    assert!(matches!(e, Err(core_engine::bounce::BounceError::Sink(m)) if m == "disk full"));
    // A Play that never reaches the renderer has its own words (final
    // review T2-M2), not the engine's Busy.
    assert_eq!(
        core_engine::bounce::BounceError::NotRolling.to_string(),
        "the transport did not start"
    );
}

/// Final review T2-M1 (guards Task 2's deviation 3): a range shorter than
/// a block (100 ticks, under 1024 frames) is exact, the sink gets exactly
/// its frames, and a second bounce on the same engine equals the first.
#[test]
fn a_range_shorter_than_a_block_bounces() {
    let (d, seq) = doc(0, None);
    let mut r = rig(&d, seq);
    let short = BounceSpec {
        start: Tick(2 * Q),
        end: Tick(2 * Q + 100),
        tail_cap: 0,
        ..spec(0)
    };
    let want = r.engine.tick_frame(Tick(2 * Q + 100)) - r.engine.tick_frame(Tick(2 * Q));
    assert!(want > 0 && want < u64::from(BLOCK), "{want} frames");
    let (first, stats) = run_lr(&mut r, &short);
    assert_eq!((stats.range, stats.frames), (want, want));
    assert!(
        first[0].0.iter().any(|x| x.abs() > 1e-3),
        "the region sounds"
    );
    let (second, again) = run_lr(&mut r, &short);
    assert_eq!(again, stats);
    assert_eq!(second, first, "the same engine bounces it again alike");
}
