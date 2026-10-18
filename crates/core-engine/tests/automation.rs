//! Automation playback (editors spec §4d, §6; Plan 4d Rulings 10–13):
//! renders against the tone sink, compared with the laws.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_model::automation::{AutomationTarget, fader_db, fader_norm, pan_norm};
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{DestId, SeqId, StripId};
use core_model::mixer::{StripKind, db_to_gain};
use core_model::time::Tick;

const SR: f64 = 48_000.0;
const BAR: i64 = 4 * Q;

struct Song {
    doc: Document,
    seq: SeqId,
    synth: StripId,
    dest: DestId,
}

/// The built-in synth holding C4 for 16 bars in "S", plus a lane "Auto"
/// with `points(the synth's strip)`. Every song has the same IDs.
fn song(points: impl Fn(StripId) -> Vec<(i64, AutomationTarget, f64)>) -> Song {
    let mut b = Builder::new();
    let dest = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(dest)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0xFFFF, 16 * BAR)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let auto = b.lane(seq, "Auto", None).unwrap();
    let specs: Vec<EventSpec> = points(synth)
        .into_iter()
        .map(|(t, target, value)| EventSpec::new(Tick(t), EventBody::Automation { target, value }))
        .collect();
    if !specs.is_empty() {
        b.events(seq, auto, specs).unwrap();
    }
    Song {
        doc: b.finish(),
        seq,
        synth,
        dest,
    }
}

struct Rig {
    engine: Engine,
    be: OfflineBackend,
    block: u32,
}

fn rig(doc: &Document, seq: SeqId, block: u32) -> Rig {
    let (mut engine, r) = Engine::new(
        EngineConfig::new(SR),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    engine.load(doc, seq).unwrap();
    let mut be = OfflineBackend::new(SR, block);
    be.start(r).unwrap();
    be.reserve(block);
    Rig { engine, be, block }
}

impl Rig {
    /// The left channel of the next `frames` frames.
    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames + self.block as usize);
        while out.len() < frames {
            self.be.block(self.block);
            out.extend_from_slice(self.be.output(self.block).0);
        }
        out.truncate(frames);
        out
    }
}

fn played(s: &Song, frames: usize) -> Vec<f32> {
    let mut r = rig(&s.doc, s.seq, 256);
    r.engine.play().unwrap();
    r.render(frames)
}

/// `got` is `want` times `gain`, sample by sample.
fn scaled(got: &[f32], want: &[f32], gain: f32) {
    assert!(want.iter().any(|x| x.abs() > 1e-3), "the tone sounds");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(
            (g - w * gain).abs() <= 1e-6,
            "frame {i}: {g} vs {}",
            w * gain
        );
    }
}

#[test]
fn a_fader_ramp_follows_the_lane_between_points() {
    let unity = fader_norm(0.0);
    let dry = played(&song(|_| vec![]), 120_000);
    let wet = played(
        &song(|s| {
            vec![
                (0, AutomationTarget::StripFader(s), 0.0),
                (BAR, AutomationTarget::StripFader(s), unity),
            ]
        }),
        120_000,
    );
    assert!(dry.iter().any(|x| x.abs() > 1e-3));
    // A bar at 120 BPM is 96 000 frames.
    for (i, (w, d)) in wet.iter().zip(&dry).enumerate() {
        let v = unity * (i as f64 / 96_000.0).min(1.0);
        let g = db_to_gain(fader_db(v)) as f32;
        assert!((w - d * g).abs() <= 1e-6, "frame {i}: {w} vs {}", d * g);
    }
    // Frame-exact: the law at each run's ends, linear between (runs are
    // the 256-frame blocks), to a relative tolerance a ramp one frame late
    // exceeds.
    let gain = |p: u64| unity * (p as f64 / 96_000.0).min(1.0);
    let model = ramp_model(wet.len(), 256, &[], |i| i as u64, gain);
    frame_exact(&wet, &dry, &model);
}

/// Per output frame, the fader gain of piecewise-linear automation: the
/// output is cut into runs at every `block` frames and at `cuts`; a run
/// from output frame `a` (schedule frame `pos(a)`) ramps from the law at
/// `pos(a)` to the law at `pos(a)` plus its length, as the engine does in
/// `f32`; `value(p)` is the lane's normalised value at schedule frame `p`.
fn ramp_model(
    total: usize,
    block: usize,
    cuts: &[usize],
    pos: impl Fn(usize) -> u64,
    value: impl Fn(u64) -> f64,
) -> Vec<f32> {
    // Whole blocks: the last may be rendered past `total`.
    let blocks = total.div_ceil(block) * block;
    let mut bounds: Vec<usize> = (0..=blocks)
        .step_by(block)
        .chain(cuts.iter().copied())
        .collect();
    bounds.sort_unstable();
    bounds.dedup();
    let mut out = Vec::with_capacity(total);
    for w in bounds.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = b - a;
        let pa = pos(a);
        let ga = db_to_gain(fader_db(value(pa))) as f32;
        let gb = db_to_gain(fader_db(value(pa + n as u64))) as f32;
        let step = (gb - ga) / n as f32;
        out.extend((0..n).map(|k| ga + step * k as f32));
    }
    out.truncate(total);
    out
}

/// `got` is `dry` times `model`, frame by frame, to 2e-6 relative (plus
/// 1e-12, for `f32` rounding near the gain floor).
fn frame_exact(got: &[f32], dry: &[f32], model: &[f32]) {
    let mut checked = 0;
    for (i, ((w, d), m)) in got.iter().zip(dry).zip(model).enumerate() {
        let want = d * m;
        assert!(
            (w - want).abs() <= 2e-6 * want.abs() + 1e-12,
            "frame {i}: {w} vs {want}"
        );
        checked += usize::from(d.abs() > 1e-3);
    }
    assert!(checked > got.len() / 2, "the tone sounds through the ramp");
}

#[test]
fn a_fader_ramp_is_cut_at_the_loop_end_and_restarts_after_the_wrap() {
    let unity = fader_norm(0.0);
    // Three quarters (72 000 frames) loop over a one-bar ramp: the wrap is
    // not on a 256-frame block boundary.
    let looped = |s: &Song| {
        let mut r = rig(&s.doc, s.seq, 256);
        r.engine.set_loop_range(Some((0, 72_000))).unwrap();
        r.engine.play().unwrap();
        r.render(80_000)
    };
    let dry = looped(&song(|_| vec![]));
    let wet = looped(&song(|s| {
        vec![
            (0, AutomationTarget::StripFader(s), 0.0),
            (BAR, AutomationTarget::StripFader(s), unity),
        ]
    }));
    let gain = |p: u64| unity * (p as f64 / 96_000.0).min(1.0);
    let model = ramp_model(wet.len(), 256, &[72_000], |i| (i % 72_000) as u64, gain);
    // The run before the wrap ends at the loop end's value; the one after
    // starts again from the loop start's.
    assert!((model[71_999] - db_to_gain(fader_db(gain(71_999))) as f32).abs() < 1e-4);
    assert_eq!(model[72_000], db_to_gain(fader_db(0.0)) as f32);
    frame_exact(&wet, &dry, &model);
}

#[test]
fn a_mute_point_switches_at_its_frame_inside_a_block() {
    let dry = played(&song(|_| vec![]), 60_000);
    let wet = played(
        &song(|s| {
            vec![
                (0, AutomationTarget::StripMute(s), 0.0),
                (Q, AutomationTarget::StripMute(s), 1.0),
                (2 * Q, AutomationTarget::StripMute(s), 0.0),
            ]
        }),
        60_000,
    );
    // A quarter is 24 000 frames; neither 24 000 nor 48 000 is on a block
    // boundary of 256 for the second, so the block is split there.
    for (i, (w, d)) in wet.iter().zip(&dry).enumerate() {
        if (24_000..48_000).contains(&i) {
            assert_eq!(*w, 0.0, "frame {i} is muted");
        } else {
            assert_eq!(w, d, "frame {i} plays as without automation");
        }
    }
}

#[test]
fn dense_points_never_split_runs_below_32_frames() {
    // A pan point every 8 ticks (12.5 frames) for two beats.
    let dense = song(|s| {
        (0..3840)
            .map(|k| (k * 8, AutomationTarget::StripPan(s), (k % 2) as f64))
            .collect()
    });
    let mut r = rig(&dense.doc, dense.seq, 512);
    r.engine.play().unwrap();
    r.render(100 * 512);
    let runs = r.engine.counters().runs;
    assert!(runs > 100, "the points split blocks ({runs} runs)");
    assert!(
        runs <= 100 * 512 / 32 + 100,
        "{runs} runs: one shorter than 32 frames"
    );
}

/// Plays for 10 blocks, stops, runs `then`, and returns one block of an
/// auditioned note (left channel).
fn held_after_stop(s: &Song, then: impl FnOnce(&mut Engine)) -> Vec<f32> {
    let mut r = rig(&s.doc, s.seq, 256);
    r.engine.play().unwrap();
    r.render(2_560);
    r.engine.stop().unwrap();
    r.render(256);
    then(&mut r.engine);
    r.render(256);
    r.engine.audition(&s.doc, s.dest, 64, 0xFFFF, true).unwrap();
    r.render(256)
}

#[test]
fn stopping_keeps_the_last_automated_value_until_the_fader_moves() {
    let v = fader_norm(-12.0);
    let wet = song(|s| vec![(0, AutomationTarget::StripFader(s), v)]);
    let dry = song(|_| vec![]);
    let base = held_after_stop(&dry, |_| {});
    let g = db_to_gain(fader_db(v)) as f32;
    scaled(&held_after_stop(&wet, |_| {}), &base, g);
    // An edit that leaves the fader alone keeps the automated value.
    let same = wet.doc.clone();
    scaled(
        &held_after_stop(&wet, |e| {
            e.load(&same, wet.seq).unwrap();
        }),
        &base,
        g,
    );
    // The user moves the fader: the static value plays.
    let mut b = Builder::from_doc(wet.doc.clone());
    b.mixer(MixerCmd::SetGain {
        strip: wet.synth,
        gain_db: -3.0,
    })
    .unwrap();
    let moved = b.finish();
    scaled(
        &held_after_stop(&wet, |e| {
            e.load(&moved, wet.seq).unwrap();
        }),
        &base,
        db_to_gain(-3.0) as f32,
    );
}

#[test]
fn while_playing_automation_wins_over_a_fader_move() {
    let v = fader_norm(-12.0);
    let wet = song(|s| vec![(0, AutomationTarget::StripFader(s), v)]);
    let mut b = Builder::from_doc(wet.doc.clone());
    b.mixer(MixerCmd::SetGain {
        strip: wet.synth,
        gain_db: -3.0,
    })
    .unwrap();
    let moved = b.finish();
    // A recompile releases the played note (every edit does), so an
    // auditioned note carries the sound after the fader move.
    let steps = |s: &Song, load: &Document| {
        let mut r = rig(&s.doc, s.seq, 256);
        r.engine.play().unwrap();
        r.render(2_560);
        r.engine.load(load, s.seq).unwrap();
        r.render(256);
        r.engine.audition(&s.doc, s.dest, 64, 0xFFFF, true).unwrap();
        r.render(2_560)
    };
    let dry = song(|_| vec![]);
    let base = steps(&dry, &dry.doc);
    scaled(&steps(&wet, &moved), &base, db_to_gain(fader_db(v)) as f32);
}

#[test]
fn play_after_the_automation_is_gone_restores_the_static_fader() {
    let wet = song(|s| vec![(0, AutomationTarget::StripFader(s), fader_norm(-12.0))]);
    let dry = song(|_| vec![]);
    let steps = |s: &Song| {
        let mut r = rig(&s.doc, s.seq, 256);
        r.engine.play().unwrap();
        r.render(2_560);
        r.engine.stop().unwrap();
        r.render(256);
        // The automation is deleted (the dry song is the same document
        // without its points).
        r.engine.load(&dry.doc, dry.seq).unwrap();
        r.render(256);
        r.engine.locate_frame(0).unwrap();
        r.engine.play().unwrap();
        r.render(2_560)
    };
    assert_eq!(steps(&wet), steps(&dry));
}

#[test]
fn automation_of_a_deleted_strip_plays_like_none() {
    let gone = song(|_| {
        vec![
            (0, AutomationTarget::StripFader(StripId(999)), 0.0),
            (0, AutomationTarget::StripMute(StripId(999)), 1.0),
            (0, AutomationTarget::StripPan(StripId(999)), pan_norm(1.0)),
        ]
    });
    let (mut engine, _r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(64))).unwrap();
    let report = engine.load(&gone.doc, gone.seq).unwrap();
    assert_eq!(report.automation_missing, 3);
    assert_eq!(played(&gone, 20_000), played(&song(|_| vec![]), 20_000));
}

/// A song whose synth has a center pan point at `Q` (a run starts there)
/// and a mute point `ticks` after it, unmuted again at `2 * Q`; the first
/// muted frame.
fn first_muted(ticks: i64) -> usize {
    let s = song(|s| {
        vec![
            (0, AutomationTarget::StripMute(s), 0.0),
            (Q, AutomationTarget::StripPan(s), pan_norm(0.0)),
            (Q + ticks, AutomationTarget::StripMute(s), 1.0),
            (2 * Q, AutomationTarget::StripMute(s), 0.0),
        ]
    });
    let dry = played(&song(|_| vec![]), 30_000);
    let wet = played(&s, 30_000);
    let first = wet
        .iter()
        .zip(&dry)
        .position(|(w, d)| w != d)
        .expect("the mute sounds");
    assert!(
        wet[first..].iter().all(|x| *x == 0.0),
        "muted from {first} on"
    );
    first
}

#[test]
fn a_mute_point_inside_a_short_run_switches_at_the_nearest_boundary() {
    // The run from the pan point at frame 24 000 is stretched to 32 frames:
    // a mute 6 frames in switches at its start, one 25 frames in at its
    // end (Plan 4d Ruling 11).
    assert_eq!(first_muted(4), 24_000);
    assert_eq!(first_muted(16), 24_032);
}

/// An insert recording the automated values it is given.
#[derive(Clone, Default)]
struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<(u32, u64, f64)>>>);

impl core_engine::graph::EffectNode for Recorder {
    fn latency(&self) -> u32 {
        0
    }
    fn process(
        &mut self,
        _: u32,
        _: &mut core_engine::backend::StereoBuf,
        _: Option<&core_engine::backend::StereoBuf>,
    ) -> core_engine::graph::ProcessStatus {
        core_engine::graph::ProcessStatus::Ok
    }
    fn reset(&mut self) {}
    fn automate(&mut self, frame: u32, param: core_model::mixer::ParamId, normalized: f64) {
        self.0.lock().unwrap().push((frame, param.0, normalized));
    }
}

impl core_engine::graph::InsertFactory for Recorder {
    fn make(
        &mut self,
        _: &core_model::mixer::PluginSlot,
        _: f64,
        _: u32,
    ) -> Result<Box<dyn core_engine::graph::EffectNode>, String> {
        Ok(Box::new(self.clone()))
    }
}

#[test]
fn a_plugin_parameter_ramp_is_sent_every_32_frames_until_it_holds() {
    use core_engine::graph::TestInserts;
    use core_model::mixer::ParamId;
    let mut s = song(|_| vec![]);
    let mut b = Builder::from_doc(s.doc.clone());
    let id = b.insert(s.synth, TestInserts::plugin("rec")).unwrap();
    let auto = b.lane(s.seq, "P", None).unwrap();
    let target = AutomationTarget::PluginParam(id, ParamId(7));
    b.events(
        s.seq,
        auto,
        vec![
            EventSpec::new(Tick(0), EventBody::Automation { target, value: 0.0 }),
            EventSpec::new(Tick(Q), EventBody::Automation { target, value: 1.0 }),
        ],
    )
    .unwrap();
    s.doc = b.finish();
    let rec = Recorder::default();
    let (mut engine, renderer) = Engine::new(
        EngineConfig::new(SR),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    engine.set_insert_factory(Box::new(rec.clone()));
    engine.load(&s.doc, s.seq).unwrap();
    let mut be = OfflineBackend::new(SR, 512);
    be.start(renderer).unwrap();
    be.reserve(512);
    let mut r = Rig {
        engine,
        be,
        block: 512,
    };
    r.render(512);
    assert!(rec.0.lock().unwrap().is_empty(), "nothing while stopped");
    r.engine.play().unwrap();
    // A quarter is 24 000 frames; play 30 000.
    r.render(30_208);
    let got = rec.0.lock().unwrap().clone();
    // Blocks of 512 are single runs but for the one holding the point at
    // frame 24 000, split there: a value every max(32, run / 16) frames of
    // each run, at the frame within it, each only when it changed (so 1.0
    // once, at the point).
    let mut runs: Vec<(u64, u64)> = (0..59u64).map(|k| (k * 512, k * 512 + 512)).collect();
    runs[46] = (23_552, 24_000);
    runs.insert(47, (24_000, 24_064));
    let mut want = Vec::new();
    let mut last = f64::NAN;
    for (a, b) in runs {
        for f in (0..(b - a) as u32).step_by(32) {
            let v = ((a + u64::from(f)) as f64 / 24_000.0).min(1.0);
            if v != last {
                want.push((f, 7, v));
                last = v;
            }
        }
    }
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(&want) {
        assert_eq!((g.0, g.1), (w.0, w.1));
        assert!((g.2 - w.2).abs() < 1e-12, "{g:?} vs {w:?}");
    }
}

/// A plugin instrument sounding 0.1 whatever it is sent, counting the
/// frames it rendered or was told it skipped.
struct Clock(std::sync::Arc<std::sync::atomic::AtomicU64>);

impl core_engine::backend::InstrumentNode for Clock {
    fn midi(&mut self, _: u32, _: &[u8]) {}
    fn render(&mut self, frames: u32, out: &mut core_engine::backend::StereoBuf) {
        let n = frames as usize;
        out.left[..n].fill(0.1);
        out.right[..n].fill(0.1);
        self.0
            .fetch_add(u64::from(frames), std::sync::atomic::Ordering::Relaxed);
    }
    fn reset(&mut self) {}
    fn skip(&mut self, frames: u32) {
        self.0
            .fetch_add(u64::from(frames), std::sync::atomic::Ordering::Relaxed);
    }
}

/// An insert that stalls the run starting at schedule frame 24 000 while
/// playing, until the test sets `release` (at most 20 s): far past any
/// deadline, however loaded the machine.
struct Staller(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl core_engine::graph::EffectNode for Staller {
    fn latency(&self) -> u32 {
        0
    }
    fn process(
        &mut self,
        _: u32,
        _: &mut core_engine::backend::StereoBuf,
        _: Option<&core_engine::backend::StereoBuf>,
    ) -> core_engine::graph::ProcessStatus {
        core_engine::graph::ProcessStatus::Ok
    }
    fn reset(&mut self) {}
    fn process_block(
        &mut self,
        _: u32,
        _: &mut core_engine::backend::StereoBuf,
        _: Option<&core_engine::backend::StereoBuf>,
        info: &core_engine::backend::BlockInfo,
    ) -> core_engine::graph::ProcessStatus {
        if info.playing && info.frame == 24_000 {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while !self.0.load(std::sync::atomic::Ordering::Acquire)
                && std::time::Instant::now() < until
            {
                std::hint::spin_loop();
            }
        }
        core_engine::graph::ProcessStatus::Ok
    }
}

struct StallRig {
    clock: std::sync::Arc<std::sync::atomic::AtomicU64>,
    release: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl core_engine::graph::InsertFactory for StallRig {
    fn make(
        &mut self,
        _: &core_model::mixer::PluginSlot,
        _: f64,
        _: u32,
    ) -> Result<Box<dyn core_engine::graph::EffectNode>, String> {
        Ok(Box::new(Staller(std::sync::Arc::clone(&self.release))))
    }
    fn make_instrument(
        &mut self,
        _: &core_model::mixer::PluginSlot,
        _: f64,
        _: u32,
    ) -> Result<Box<dyn core_engine::backend::InstrumentNode>, String> {
        Ok(Box::new(Clock(std::sync::Arc::clone(&self.clock))))
    }
}

#[test]
fn a_run_missing_its_deadline_skips_the_rest_of_its_block() {
    use core_engine::graph::{TestInserts, TestInstrument};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let mut b = Builder::new();
    let (d, _) = b
        .plugin_dest("Clock", TestInstrument::plugin("synth"))
        .unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0xFFFF, 4 * BAR)]).unwrap();
    let strip = b.strip_of(StripKind::Instrument(d)).unwrap();
    b.insert(strip, TestInserts::plugin("stall")).unwrap();
    // −20 dB until frame 24 000, then a ramp to 0 dB at frame 24 200: block
    // 23 of 1024 frames (23 552 … 24 576) runs 23 552 … 24 000 … 24 200 …
    let low = fader_norm(-20.0);
    let fader = AutomationTarget::StripFader(strip);
    let auto = b.lane(seq, "Auto", None).unwrap();
    b.events(
        seq,
        auto,
        [(0, low), (Q, low), (Q + 128, fader_norm(0.0))]
            .into_iter()
            .map(|(t, value)| {
                EventSpec::new(
                    Tick(t),
                    EventBody::Automation {
                        target: fader,
                        value,
                    },
                )
            })
            .collect(),
    )
    .unwrap();
    let doc = b.finish();
    let clock = Arc::new(AtomicU64::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let mut cfg = EngineConfig::new(SR);
    cfg.workers = 1;
    // 25 blocks of 1024 frames: about 530 ms, a wide margin for the normal
    // runs of a debug build on a loaded machine; the stalled run waits for
    // the test to release it.
    cfg.deadline = Some(25.0);
    let (mut engine, r) = Engine::new(cfg, Box::new(OfflineSinks::new(16))).unwrap();
    engine.set_insert_factory(Box::new(StallRig {
        clock: Arc::clone(&clock),
        release: Arc::clone(&release),
    }));
    engine.set_callback_helps(false);
    engine.load(&doc, seq).unwrap();
    let mut be = OfflineBackend::new(SR, 1024);
    be.start(r).unwrap();
    be.reserve(1024);
    engine.play().unwrap();
    let rendered = std::cell::Cell::new(0u64);
    let block = |be: &mut OfflineBackend| {
        be.block(1024);
        rendered.set(rendered.get() + 1024);
        be.output(1024).0.to_vec()
    };
    for _ in 0..23 {
        block(&mut be);
    }
    assert_eq!(engine.counters().deadline_misses, 0);
    let runs = engine.counters().runs;
    let missed = block(&mut be);
    let c = engine.counters();
    assert_eq!(c.deadline_misses, 1);
    assert_eq!(c.runs - runs, 2, "the third run never started");
    // The first run plays; the missed one and the skipped one are silent.
    assert!(missed[..448].iter().all(|x| x.abs() > 1e-3), "run 1 sounds");
    assert!(missed[448..].iter().all(|x| *x == 0.0), "the rest is lost");
    // The missed run's frames were played by the transport, the skipped
    // run's were not.
    assert_eq!(engine.position_frames(), 24_200);
    // Stopped while the straggler still holds the graph; then it finishes.
    engine.stop().unwrap();
    release.store(true, Ordering::Release);
    let mut recovered = false;
    for _ in 0..2000 {
        let c = engine.counters();
        block(&mut be);
        let d = engine.counters();
        if d.busy_blocks == c.busy_blocks && d.deadline_misses == c.deadline_misses {
            recovered = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(recovered, "recovered");
    // Every frame the device asked for was rendered by the instrument or
    // passed on as skipped (busy blocks and the skipped run).
    assert_eq!(clock.load(Ordering::Relaxed), rendered.get());
    // The missed run's ramp settled once its straggler was done: stopped
    // blocks hold the ramp's end value instead of ramping again.
    let steady: Vec<f32> = (0..3).flat_map(|_| block(&mut be)).collect();
    let level = steady[0];
    assert!(level.abs() > 1e-3);
    assert!(steady.iter().all(|x| *x == level), "steady at {level}");
    assert_eq!(clock.load(Ordering::Relaxed), rendered.get());
}
