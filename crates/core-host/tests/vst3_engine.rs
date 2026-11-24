//! VST3 plug-ins playing in the engine's graph through the adapters and
//! the shelf (host track H1 Task 6), mirroring `engine_plugins.rs`:
//! inserts, an instrument with two outputs, a sidechain, a NaN fault,
//! latency compensation, values both ways, processors coming back and
//! automation. They run on the in-repo Rust test VST3 (a private copy per
//! test) and, where they are built, on the VST3 SDK's samples.

mod common;

use common::vst3::{load_vst3, private, sample, sdk, vst3_ref};
use common::{Live, shelve};
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, Q, demo_document};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_host::adapt::Returned;
use core_host::{ParamEvent, PluginInstance};
use core_model::automation::AutomationTarget;
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::{ParamId, StripKind};
use core_model::time::Tick;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};
use test_plugins::{
    VST3_GAIN_ID, VST3_NAN_ID, VST3_PARAM_GAIN, VST3_PARAM_MODE, VST3_PARAM_SECRET, VST3_PARAMS,
    VST3_SYNTH_ID, Vst3Edit,
};

const SR: f64 = 48_000.0;

fn pid(id: u32) -> ParamId {
    ParamId(u64::from(id))
}

fn factor(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

struct Rig {
    engine: Engine,
    be: OfflineBackend,
    shelf: core_host::shelf::Shelf,
    back: Sender<Returned>,
    returned: Receiver<Returned>,
    live: Vec<Live>,
}

fn rig() -> Rig {
    let (mut engine, r) = Engine::new(
        EngineConfig::new(SR),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    let shelf = core_host::shelf::Shelf::new();
    engine.set_insert_factory(Box::new(shelf.factory()));
    let mut be = OfflineBackend::new(SR, 256);
    be.start(r).unwrap();
    let (back, returned) = channel();
    Rig {
        engine,
        be,
        shelf,
        back,
        returned,
        live: Vec::new(),
    }
}

impl Rig {
    /// Shelves `inst` as slot `id` (activated for 48 kHz / 4096).
    fn put(&mut self, id: PluginInstanceId, inst: Box<dyn PluginInstance>, instrument: bool) {
        let live = shelve(&self.shelf, id, inst, instrument, &self.back);
        self.live.push(live);
    }

    fn add(&mut self, id: PluginInstanceId, path: &Path, class: &str, instrument: bool) {
        self.put(id, load_vst3(path, class), instrument);
    }

    /// Plays `frames` from the start and returns the left channel.
    fn play(&mut self, frames: usize) -> Vec<f32> {
        self.engine.play().unwrap();
        let mut out = Vec::new();
        while out.len() < frames {
            self.be.block(256);
            out.extend_from_slice(self.be.output(256).0);
        }
        out
    }
}

fn demo() -> (Builder, SeqId) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    (Builder::from_doc(doc), song)
}

/// The demo with an insert of class `class` on the built-in synth.
fn with_insert(class: &str) -> (Document, SeqId, PluginInstanceId) {
    let (mut b, song) = demo();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, vst3_ref(class, class)).unwrap();
    (b.finish(), song, id)
}

fn dry(frames: usize) -> Vec<f32> {
    let (b, song) = demo();
    let mut plain = rig();
    plain.engine.load(b.doc(), song).unwrap();
    plain.play(frames)
}

fn note_on(ch: u8) -> EventSpec {
    let mut e = EventSpec::new(Tick(0), EventBody::note(69, 0xFFFF, Tick(8 * Q)));
    e.group_channel = Some((0, ch));
    e
}

#[test]
fn a_vst3_gain_insert_scales_the_strip() {
    let p = private();
    let dry = dry(48_000);
    let (doc, song, id) = with_insert(VST3_GAIN_ID);
    let mut r = rig();
    let mut inst = load_vst3(&p.path, VST3_GAIN_ID);
    inst.set_params(&[(pid(VST3_PARAM_GAIN), -6.0)]).unwrap();
    r.put(id, inst, false);
    let report = r.engine.load(&doc, song).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    let wet = r.play(48_000);
    assert!(dry.iter().any(|x| *x != 0.0));
    let g = factor(-6.0);
    for (d, w) in dry.iter().zip(&wet) {
        assert!((*w - *d * g).abs() <= 1e-6, "{w} vs {}", d * g);
    }
    assert!(r.engine.faults().is_empty());
}

#[test]
fn a_two_output_vst3_synth_feeds_its_output_strips() {
    let p = private();
    let mut b = Builder::new();
    let (d, id) = b
        .plugin_dest("Synth", vst3_ref(VST3_SYNTH_ID, "Test Synth"))
        .unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.events(seq, lane, vec![note_on(0)]).unwrap();
    b.mixer(MixerCmd::AddInstrumentOutput {
        dest: d,
        index: 1,
        name: "Two".into(),
    })
    .unwrap();
    let main = b.strip_of(StripKind::Instrument(d)).unwrap();
    let two = b.strip_of(StripKind::InstrumentOutput(d, 1)).unwrap();
    let mut r = rig();
    r.add(id, &p.path, VST3_SYNTH_ID, true);
    let report = r.engine.load(b.doc(), seq).unwrap();
    assert!(report.unroutable.is_empty(), "{:?}", report.warnings);
    assert!(
        report.graph.warnings.is_empty(),
        "{:?}",
        report.graph.warnings
    );
    r.play(4096);
    let m = r.engine.meters();
    let peak = |s: StripId| m.iter().find(|x| x.strip == s).unwrap().peak_l;
    assert!(peak(main) > 0.1, "the held note on output 1: {m:?}");
    assert!(peak(two) > 0.1, "output 2 (0.25 always): {m:?}");
    assert!(p.probe.counters().note_ons > 0, "the note reached it");
}

/// The built-in synth's strip with a Test Gain insert whose sidechain is
/// fed from a Test Synth strip that holds a note (or not).
fn sidechained(note: bool) -> (Document, SeqId, PluginInstanceId, PluginInstanceId) {
    let (mut b, song) = demo();
    let (d, synth) = b
        .plugin_dest("Key", vst3_ref(VST3_SYNTH_ID, "Test Synth"))
        .unwrap();
    let lane = b.lane(song, "Key", Some(d)).unwrap();
    if note {
        b.events(song, lane, vec![note_on(0)]).unwrap();
    }
    let key = b.strip_of(StripKind::Instrument(d)).unwrap();
    let target = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let gain = b
        .insert(target, vst3_ref(VST3_GAIN_ID, "Test Gain"))
        .unwrap();
    b.mixer(MixerCmd::SetSidechain {
        slot: gain,
        source: Some(key),
    })
    .unwrap();
    // The key feeds the sidechain but is not heard itself.
    let sink = b.bus("Sink").unwrap();
    b.mixer(MixerCmd::SetOutput {
        strip: key,
        output: core_model::mixer::Output::Bus(sink),
    })
    .unwrap();
    b.mixer(MixerCmd::SetGain {
        strip: sink,
        gain_db: core_model::mixer::GAIN_FLOOR_DB,
    })
    .unwrap();
    (b.finish(), song, synth, gain)
}

#[test]
fn the_sidechain_source_reaches_the_vst3_sidechain_bus() {
    // Test Gain adds its sidechain to its output.
    let level = |note: bool| {
        let p = private();
        let (doc, song, synth, gain) = sidechained(note);
        let mut r = rig();
        r.add(synth, &p.path, VST3_SYNTH_ID, true);
        r.add(gain, &p.path, VST3_GAIN_ID, false);
        let report = r.engine.load(&doc, song).unwrap();
        assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
        let out = r.play(24_000);
        assert!(p.probe.counters().sidechain_blocks > 0);
        out.iter().map(|x| x.abs()).sum::<f32>()
    };
    let without = level(false);
    let with = level(true);
    assert!(with > without * 2.0, "with {with} vs without {without}");
}

#[test]
fn a_nan_vst3_is_bypassed_and_reported_by_instance() {
    let p = private();
    let (doc, song, id) = with_insert(VST3_NAN_ID);
    let mut r = rig();
    r.add(id, &p.path, VST3_NAN_ID, false);
    r.engine.load(&doc, song).unwrap();
    let out = r.play(20 * 256);
    assert!(out.iter().all(|x| x.is_finite()));
    assert_eq!(r.engine.plugin_faults(), vec![id]);
    assert!(r.engine.faults().contains(Faults::PLUGIN_FAULT));
}

#[test]
fn vst3_latency_is_compensated() {
    let p = private();
    p.probe.set_timing(300, 0);
    let (doc, song, id) = with_insert(VST3_GAIN_ID);
    let mut r = rig();
    r.add(id, &p.path, VST3_GAIN_ID, false);
    let report = r.engine.load(&doc, song).unwrap();
    assert_eq!(report.graph.latency_frames, 300);
    assert_eq!(r.engine.latency_frames(), 300);
}

#[test]
fn values_reach_the_vst3_and_its_output_values_reach_the_main_thread() {
    let p = private();
    let (doc, song, id) = with_insert(VST3_GAIN_ID);
    let mut r = rig();
    let mut inst = load_vst3(&p.path, VST3_GAIN_ID);
    inst.set_params(&[(pid(VST3_PARAM_GAIN), 0.0)]).unwrap();
    r.put(id, inst, false);
    r.engine.load(&doc, song).unwrap();
    r.play(256);
    // A host value of Mode; Test Gain echoes each Mode value as Secret
    // through its output parameter changes.
    assert!(r.live[0].params.send(pid(VST3_PARAM_MODE), 2.0));
    r.play(512);
    let events = r.live[0].gestures.drain();
    assert!(
        events.contains(&ParamEvent::Value(pid(VST3_PARAM_SECRET), 1.0)),
        "{events:?}"
    );
    // A controller gesture reaches the main thread and the running
    // processor (Gain at -60 dB quietens the strip).
    let loud = r.play(4096).iter().fold(0.0f32, |m, x| m.max(x.abs()));
    p.probe.edit(Vst3Edit::Begin(VST3_PARAM_GAIN));
    p.probe.edit(Vst3Edit::Perform(
        VST3_PARAM_GAIN,
        VST3_PARAMS[0].to_normalized(-60.0),
    ));
    p.probe.edit(Vst3Edit::End(VST3_PARAM_GAIN));
    let g = r.live[0].inst.gestures();
    assert_eq!(g.len(), 3, "{g:?}");
    assert_eq!(g[0], ParamEvent::Begin(pid(VST3_PARAM_GAIN)));
    assert_eq!(g[2], ParamEvent::End(pid(VST3_PARAM_GAIN)));
    r.play(512);
    let quiet = r.play(4096).iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(quiet < loud * 0.1, "{quiet} vs {loud}");
}

#[test]
fn removed_vst3_instances_come_back_off_the_audio_thread() {
    let p = private();
    let (doc, song, id) = with_insert(VST3_GAIN_ID);
    let mut b = Builder::from_doc(doc.clone());
    let mut r = rig();
    r.add(id, &p.path, VST3_GAIN_ID, false);
    r.engine.load(&doc, song).unwrap();
    r.play(512);
    b.mixer(MixerCmd::RemoveInsert { slot: id }).unwrap();
    r.engine.load(b.doc(), song).unwrap();
    let start = Instant::now();
    let back = loop {
        r.be.block(256);
        if let Ok(x) = r.returned.recv_timeout(Duration::from_millis(5)) {
            break x;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "never returned");
    };
    assert_eq!(back.id, id);
    let live = r.live.pop().unwrap();
    let mut inst = live.inst;
    inst.deactivate(back.processor);
    assert!(!inst.is_active());
    let c = p.probe.counters();
    assert_eq!(c.set_active_on, c.set_active_off, "deactivated");
}

/// The built-in synth holding C4 for 4 bars through an insert of `class`,
/// with automation `points` (tick, normalised value) of parameter `param`.
fn automated(
    class: &str,
    param: ParamId,
    points: &[(i64, f64)],
) -> (Document, SeqId, PluginInstanceId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(
        seq,
        notes,
        &(0..16).map(|k| (k * Q, 60, 0xFFFF, Q)).collect::<Vec<_>>(),
    )
    .unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, vst3_ref(class, class)).unwrap();
    let lane = b.lane(seq, "Auto", None).unwrap();
    let events: Vec<EventSpec> = points
        .iter()
        .map(|(t, v)| {
            EventSpec::new(
                Tick(*t),
                EventBody::Automation {
                    target: AutomationTarget::PluginParam(id, param),
                    value: *v,
                },
            )
        })
        .collect();
    if !events.is_empty() {
        b.events(seq, lane, events).unwrap();
    }
    (b.finish(), seq, id)
}

/// Plays the automated song through `inst` and through `flat` (the same
/// plug-in without automation); returns (wet, flat), 98 304 frames each.
fn ramp(
    class: &str,
    param: ParamId,
    points: &[(i64, f64)],
    flat: Box<dyn PluginInstance>,
    inst: Box<dyn PluginInstance>,
) -> (Vec<f32>, Vec<f32>, Rig) {
    let (flat_doc, seq, id) = automated(class, param, &[]);
    let mut flat_rig = rig();
    flat_rig.put(id, flat, false);
    flat_rig.engine.load(&flat_doc, seq).unwrap();
    let flat = flat_rig.play(98_304);
    let (doc, seq, id) = automated(class, param, points);
    let mut r = rig();
    r.put(id, inst, false);
    let report = r.engine.load(&doc, seq).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    assert_eq!(report.automation_lanes, 1);
    let wet = r.play(98_304);
    assert!(flat.iter().any(|x| x.abs() > 1e-3));
    (wet, flat, r)
}

#[test]
fn a_parameter_ramp_reaches_a_vst3_gain_insert() {
    // Gain's automation is normalised over -60..+12 dB; Test Gain applies
    // the block's last value to the whole block, the one at frame 224 of
    // each 256-frame block (a value every 32 frames on a ramp).
    let p = private();
    let gain = pid(VST3_PARAM_GAIN);
    let mut flat = load_vst3(&p.path, VST3_GAIN_ID);
    flat.set_params(&[(gain, 0.0)]).unwrap();
    let inst = load_vst3(&p.path, VST3_GAIN_ID);
    let (wet, flat, mut r) = ramp(VST3_GAIN_ID, gain, &[(0, 0.0), (4 * Q, 0.75)], flat, inst);
    for (k, (w, f)) in wet.chunks(256).zip(flat.chunks(256)).enumerate() {
        let at = (k * 256 + 224) as f64;
        // `flat` played at 0 dB.
        let g = factor(-60.0 + 72.0 * 0.75 * (at / 96_000.0).min(1.0));
        for (x, y) in w.iter().zip(f) {
            assert!(
                (x - y * g).abs() <= 1e-4 * y.abs() + 1e-7,
                "block {k}: {x} vs {}",
                y * g
            );
        }
    }
    // Automated values are not gestures.
    assert!(r.live[0].gestures.drain().is_empty());
}

#[test]
fn sdk_again_follows_its_automation_in_the_engine() {
    let Some(path) = sample("again.vst3") else {
        return;
    };
    // Gain's plain value is its factor (0-1). The flat copy plays at 1.
    let gain = ParamId(0);
    let mut flat = load_vst3(&path, sdk::AGAIN);
    flat.set_params(&[(gain, 1.0)]).unwrap();
    let inst = load_vst3(&path, sdk::AGAIN);
    let (wet, flat, _r) = ramp(sdk::AGAIN, gain, &[(0, 0.0), (4 * Q, 0.5)], flat, inst);
    for (k, (w, f)) in wet.chunks(256).zip(flat.chunks(256)).enumerate() {
        // AGain also applies the block's last value to the whole block.
        let at = (k * 256 + 224) as f64;
        let g = (0.5 * (at / 96_000.0).min(1.0)) as f32;
        for (x, y) in w.iter().zip(f) {
            assert!((x - y * g).abs() <= 1e-5, "block {k}: {x} vs {}", y * g);
        }
    }
}

#[test]
fn sdk_note_expression_synth_plays_as_an_instrument() {
    let Some(path) = sample("note-expression-synth.vst3") else {
        return;
    };
    let mut b = Builder::new();
    let (d, id) = b
        .plugin_dest(
            "Synth",
            vst3_ref(sdk::NOTE_EXPRESSION_SYNTH, "Note Expression Synth"),
        )
        .unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.events(seq, lane, vec![note_on(0)]).unwrap();
    let main = b.strip_of(StripKind::Instrument(d)).unwrap();
    let mut r = rig();
    r.add(id, &path, sdk::NOTE_EXPRESSION_SYNTH, true);
    let report = r.engine.load(b.doc(), seq).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    r.play(9600);
    let m = r.engine.meters();
    let peak = m.iter().find(|x| x.strip == main).unwrap().peak_l;
    assert!(peak > 0.01, "{m:?}");
    assert!(r.engine.plugin_faults().is_empty());
}

#[test]
fn sdk_adelay_insert_delays_the_strip() {
    let Some(path) = sample("adelay.vst3") else {
        return;
    };
    let dry = dry(24_000);
    let (doc, song, id) = with_insert(sdk::ADELAY);
    let mut r = rig();
    let mut inst = load_vst3(&path, sdk::ADELAY);
    // 0.0625 s is 3000 frames.
    inst.set_params(&[(ParamId(100), 0.0625)]).unwrap();
    r.put(id, inst, false);
    r.engine.load(&doc, song).unwrap();
    let wet = r.play(24_000);
    assert!(wet[..3000].iter().all(|x| *x == 0.0));
    for (w, d) in wet[3000..].iter().zip(&dry) {
        assert!((w - d).abs() <= 1e-6, "{w} vs {d}");
    }
}
