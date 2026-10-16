//! CLAP test plugins playing in the engine's graph through the adapters
//! and the shelf: inserts, a multi-output instrument, a sidechain, faults,
//! latency, gestures and processors coming back.

mod common;

use common::{Live, load, plugin, shelve};
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, Q, demo_document};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_host::ParamEvent;
use core_host::adapt::Returned;
use core_host::shelf::Shelf;
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::{ParamId, StripKind};
use core_model::time::Tick;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

const SR: f64 = 48_000.0;

struct Rig {
    engine: Engine,
    be: OfflineBackend,
    shelf: Shelf,
    back: std::sync::mpsc::Sender<Returned>,
    returned: Receiver<Returned>,
    live: Vec<Live>,
}

fn rig() -> Rig {
    let (mut engine, r) = Engine::new(
        EngineConfig::new(SR),
        Box::new(OfflineSinks::new(1 << 16).with_tone()),
    )
    .unwrap();
    let shelf = Shelf::new();
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
    fn add(&mut self, id: PluginInstanceId, plugin_id: &str, instrument: bool) {
        let live = shelve(&self.shelf, id, load(plugin_id), instrument, &self.back);
        self.live.push(live);
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

#[test]
fn a_clap_gain_insert_scales_the_strip() {
    let (b, song) = demo();
    let mut plain = rig();
    plain.engine.load(b.doc(), song).unwrap();
    let dry = plain.play(48_000);

    let (mut b, song) = demo();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let mut r = rig();
    let mut inst = load("vizdaw.test.gain");
    inst.set_params(&[(ParamId(0), 0.5)]).unwrap();
    let live = shelve(&r.shelf, id, inst, false, &r.back);
    r.live.push(live);
    let report = r.engine.load(b.doc(), song).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    let wet = r.play(48_000);
    assert!(dry.iter().any(|x| *x != 0.0));
    for (d, w) in dry.iter().zip(&wet) {
        assert_eq!(*w, *d * 0.5);
    }
    assert!(r.engine.faults().is_empty());
}

fn note_on(ch: u8) -> EventSpec {
    let mut e = EventSpec::new(Tick(0), EventBody::note(69, 0xFFFF, Tick(8 * Q)));
    e.group_channel = Some((0, ch));
    e
}

#[test]
fn a_four_output_synth_feeds_its_output_strips() {
    let mut b = Builder::new();
    let (d, id) = b
        .plugin_dest("Synth", plugin("vizdaw.test.synth4"))
        .unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.events(seq, lane, vec![note_on(0), note_on(2)]).unwrap();
    for (index, name) in [(1u16, "Two"), (2, "Three")] {
        b.mixer(MixerCmd::AddInstrumentOutput {
            dest: d,
            index,
            name: name.into(),
        })
        .unwrap();
    }
    let main = b.strip_of(StripKind::Instrument(d)).unwrap();
    let two = b.strip_of(StripKind::InstrumentOutput(d, 1)).unwrap();
    let three = b.strip_of(StripKind::InstrumentOutput(d, 2)).unwrap();
    let mut r = rig();
    r.add(id, "vizdaw.test.synth4", true);
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
    assert!(peak(main) > 0.1, "channel 0 sounds on output 0: {m:?}");
    assert!(peak(three) > 0.1, "channel 2 sounds on output 2: {m:?}");
    assert_eq!(peak(two), 0.0, "nothing on channel 1");
}

/// The built-in synth's strip with a ducker sidechained from a test synth
/// that plays (or not) a long note.
fn ducked(note: bool) -> (Document, SeqId, PluginInstanceId, PluginInstanceId) {
    let (mut b, song) = demo();
    let (d, synth) = b.plugin_dest("Key", plugin("vizdaw.test.synth4")).unwrap();
    let lane = b.lane(song, "Key", Some(d)).unwrap();
    if note {
        b.events(song, lane, vec![note_on(0)]).unwrap();
    }
    let key = b.strip_of(StripKind::Instrument(d)).unwrap();
    let target = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let duck = b.insert(target, plugin("vizdaw.test.ducker")).unwrap();
    b.mixer(MixerCmd::SetSidechain {
        slot: duck,
        source: Some(key),
    })
    .unwrap();
    // The key drives the sidechain post-fader but is not heard: it plays
    // into a bus whose fader is at −∞.
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
    (b.finish(), song, synth, duck)
}

#[test]
fn the_ducker_follows_its_sidechain_source() {
    let level = |note: bool| {
        let (doc, song, synth, duck) = ducked(note);
        let mut r = rig();
        r.add(synth, "vizdaw.test.synth4", true);
        r.add(duck, "vizdaw.test.ducker", false);
        let report = r.engine.load(&doc, song).unwrap();
        assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
        r.play(24_000).iter().map(|x| x.abs()).sum::<f32>()
    };
    let open = level(false);
    let ducked = level(true);
    assert!(open > 0.0);
    assert!(ducked < open * 0.9, "ducked {ducked} vs open {open}");
}

#[test]
fn a_nan_plugin_is_bypassed_and_reported_by_instance() {
    let (mut b, song) = demo();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.nan")).unwrap();
    let mut r = rig();
    r.add(id, "vizdaw.test.nan", false);
    r.engine.load(b.doc(), song).unwrap();
    let out = r.play(20 * 256);
    assert!(out.iter().all(|x| x.is_finite()));
    assert_eq!(r.engine.plugin_faults(), vec![id]);
    assert!(r.engine.faults().contains(Faults::PLUGIN_FAULT));
}

#[test]
fn plugin_latency_is_compensated() {
    let (mut b, song) = demo();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let mut r = rig();
    let mut inst = load("vizdaw.test.gain");
    inst.set_params(&[(ParamId(1), 300.0)]).unwrap();
    let live = shelve(&r.shelf, id, inst, false, &r.back);
    r.live.push(live);
    let report = r.engine.load(b.doc(), song).unwrap();
    assert_eq!(report.graph.latency_frames, 300);
    assert_eq!(r.engine.latency_frames(), 300);
}

#[test]
fn ui_gestures_reach_the_main_thread_and_values_reach_the_plugin() {
    let (mut b, song) = demo();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let mut r = rig();
    r.add(id, "vizdaw.test.gain", false);
    r.engine.load(b.doc(), song).unwrap();
    r.play(256);
    assert!(r.live[0].params.send(ParamId(2), 0.25));
    r.play(512);
    assert_eq!(
        r.live[0].gestures.drain(),
        vec![
            ParamEvent::Begin(ParamId(0)),
            ParamEvent::Value(ParamId(0), 0.25),
            ParamEvent::End(ParamId(0)),
        ]
    );
}

#[test]
fn removed_instances_come_back_off_the_audio_thread() {
    let (mut b, song) = demo();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let mut r = rig();
    r.add(id, "vizdaw.test.gain", false);
    r.engine.load(b.doc(), song).unwrap();
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
    // The main thread deactivates it.
    let live = r.live.pop().unwrap();
    let mut inst = live.inst;
    inst.deactivate(back.processor);
    assert!(!inst.is_active());
}
