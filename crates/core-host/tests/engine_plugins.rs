//! CLAP test plugins playing in the engine's graph through the adapters
//! and the shelf: inserts, a multi-output instrument, a sidechain, faults,
//! latency, gestures and processors coming back.

mod common;

use common::{Live, load, plugin, shelve};
use core_engine::backend::AudioBackend;
use core_engine::backend::{BlockInfo, InstrumentNode, StereoBuf};
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, Q, demo_document};
use core_engine::graph::ProcessStatus;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_host::adapt::{PluginSynth, Returned};
use core_host::queue::param_queues;
use core_host::shelf::Ready;
use core_host::shelf::Shelf;
use core_host::{
    AudioPortInfo, AudioProcessor, ParamEvent, PluginInfo, PluginKind, PortRole, ProcessCtx, Tail,
};
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

/// A test instrument: 0.5 on both channels while a note is held, or `bad`
/// (NaN) / `Error` status on every block.
struct Held {
    held: i32,
    mode: Mode,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Notes,
    Nan,
    Error,
}

impl AudioProcessor for Held {
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) -> ProcessStatus {
        let mut at = 0usize;
        let n = ctx.frames as usize;
        for ch in ctx.outputs.iter_mut() {
            ch.fill(0.0);
        }
        for e in ctx.events {
            let end = (e.frame as usize).min(n);
            let v = if self.held > 0 { 0.5 } else { 0.0 };
            for ch in ctx.outputs.iter_mut() {
                ch[at..end].fill(v);
            }
            at = at.max(end);
            match e.bytes[0] & 0xF0 {
                0x90 if e.bytes[2] > 0 => self.held += 1,
                0x80 | 0x90 => self.held = (self.held - 1).max(0),
                _ => {}
            }
        }
        let v = if self.held > 0 { 0.5 } else { 0.0 };
        for ch in ctx.outputs.iter_mut() {
            ch[at..n].fill(v);
            if self.mode == Mode::Nan {
                ch[0] = f32::NAN;
            }
        }
        if self.mode == Mode::Error {
            ProcessStatus::Error
        } else {
            ProcessStatus::Ok
        }
    }

    fn reset(&mut self) {
        self.held = 0;
    }
}

fn held_info(id: &str) -> PluginInfo {
    PluginInfo {
        plugin: plugin(id),
        kind: PluginKind::Instrument,
        inputs: vec![],
        outputs: vec![AudioPortInfo {
            name: "Out".into(),
            channels: 2,
            role: PortRole::Main,
        }],
        note_input: true,
        params: vec![],
        latency: 0,
        tail: Tail::None,
    }
}

fn held_synth(
    id: PluginInstanceId,
    mode: Mode,
    back: &std::sync::mpsc::Sender<Returned>,
) -> PluginSynth {
    let (_, audio) = param_queues();
    let proc = Box::new(Held { held: 0, mode });
    PluginSynth::new(
        id,
        proc,
        &held_info("vizdaw.test.synth4"),
        4096,
        audio,
        back.clone(),
    )
}

const ON: [u8; 3] = [0x90, 60, 100];
const OFF: [u8; 3] = [0x80, 60, 0];

#[test]
fn skipped_blocks_flush_midi_but_keep_releases() {
    let (back, _returned) = channel();
    let mut s = held_synth(PluginInstanceId(1), Mode::Notes, &back);
    let mut out = [StereoBuf::new(256)];
    let info = BlockInfo::default();
    s.midi(0, &ON);
    s.render_outputs(256, &mut out, &info);
    assert_eq!(out[0].left[255], 0.5);
    // A note-off at frame 200 and a stale note-on in an unrendered block:
    // the release applies at the start of the next block, the note-on and
    // its offset are gone, and empty messages make no events.
    s.midi(200, &OFF);
    s.midi(210, &ON);
    s.midi(0, &[]);
    s.skip(256);
    s.render_outputs(256, &mut out, &info);
    assert!(
        out[0].left.iter().all(|x| *x == 0.0),
        "{:?}",
        &out[0].left[..8]
    );
    // Nothing from the skipped block replays afterwards.
    s.render_outputs(256, &mut out, &info);
    assert!(out[0].left.iter().all(|x| *x == 0.0));
}

#[test]
fn a_full_midi_queue_keeps_its_note_offs() {
    let (back, _returned) = channel();
    let mut s = held_synth(PluginInstanceId(1), Mode::Notes, &back);
    let mut out = [StereoBuf::new(256)];
    s.midi(0, &ON);
    s.render_outputs(256, &mut out, &BlockInfo::default());
    for _ in 0..core_host::MAX_EVENTS {
        s.midi(10, &[0xB0, 1, 2]);
    }
    s.midi(20, &OFF);
    s.render_outputs(256, &mut out, &BlockInfo::default());
    assert_eq!(out[0].left[19], 0.5);
    assert_eq!(out[0].left[20], 0.0);
}

fn faulty_instrument(
    mode: Mode,
) -> (
    Vec<f32>,
    Vec<PluginInstanceId>,
    PluginInstanceId,
    PluginInstanceId,
) {
    let mut b = Builder::new();
    let (d, id) = b
        .plugin_dest("Synth", plugin("vizdaw.test.synth4"))
        .unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    b.events(seq, lane, vec![note_on(0)]).unwrap();
    let main = b.strip_of(StripKind::Instrument(d)).unwrap();
    let ins = b.insert(main, plugin("vizdaw.test.gain")).unwrap();
    let mut r = rig();
    let synth = held_synth(id, mode, &r.back);
    r.shelf.put(
        id,
        &plugin("vizdaw.test.synth4"),
        4096,
        Ready::Instrument(synth),
    );
    r.add(ins, "vizdaw.test.gain", false);
    r.engine.load(b.doc(), seq).unwrap();
    let out = r.play(40 * 256);
    (out, r.engine.plugin_faults(), id, ins)
}

#[test]
fn a_faulty_plugin_instrument_is_zeroed_and_bypassed_under_its_own_instance() {
    for mode in [Mode::Nan, Mode::Error] {
        let (out, faults, id, ins) = faulty_instrument(mode);
        assert!(out.iter().all(|x| *x == 0.0), "zeroed");
        assert_eq!(
            faults,
            vec![id],
            "reported under the instrument, not {ins:?}"
        );
    }
}

use core_engine::graph::InsertFactory;
use core_model::automation::AutomationTarget;

/// The built-in synth holding C4 for 4 bars through a CLAP gain insert,
/// with automation `points` (tick, normalised value) of its gain (0-2).
fn gain_song(points: &[(i64, f64)]) -> (Document, SeqId, PluginInstanceId) {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0xFFFF, 16 * Q)]).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let lane = b.lane(seq, "Auto", None).unwrap();
    let events: Vec<EventSpec> = points
        .iter()
        .map(|(t, v)| {
            EventSpec::new(
                Tick(*t),
                EventBody::Automation {
                    target: AutomationTarget::PluginParam(id, ParamId(0)),
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

#[test]
fn a_parameter_ramp_reaches_a_clap_gain_insert() {
    // Without automation the gain stays at its default, 1.0.
    let (flat_doc, seq, id) = gain_song(&[]);
    let mut flat_rig = rig();
    flat_rig.add(id, "vizdaw.test.gain", false);
    flat_rig.engine.load(&flat_doc, seq).unwrap();
    let flat = flat_rig.play(98_304);
    let (doc, seq, id) = gain_song(&[(0, 0.0), (4 * Q, 0.5)]);
    let mut r = rig();
    r.add(id, "vizdaw.test.gain", false);
    let report = r.engine.load(&doc, seq).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    assert_eq!(report.automation_lanes, 1);
    let wet = r.play(98_304);
    assert!(flat.iter().any(|x| x.abs() > 1e-3));
    // The test gain applies the last value of a block to the whole block:
    // the one sent at frame 224 of each 256-frame block (a value every 32
    // frames on a ramp, Plan 4d Ruling 12). Four quarters are 96 000 frames.
    for (k, (w, f)) in wet.chunks(256).zip(flat.chunks(256)).enumerate() {
        let at = (k * 256 + 224) as f64;
        let gain = (2.0 * 0.5 * (at / 96_000.0).min(1.0)) as f32;
        for (x, y) in w.iter().zip(f) {
            assert!(
                (x - y * gain).abs() <= 1e-6,
                "block {k}: {x} vs {}",
                y * gain
            );
        }
    }
    // Automated values are not gestures: nothing comes back.
    assert!(r.live[0].gestures.drain().is_empty());
}

#[test]
fn the_shelf_knows_which_parameters_are_stepped() {
    let mut r = rig();
    let id = PluginInstanceId(5);
    r.add(id, "vizdaw.test.gain", false);
    let f = r.shelf.factory();
    assert!(
        f.param_stepped(id, ParamId(1)),
        "the latency parameter is stepped"
    );
    assert!(!f.param_stepped(id, ParamId(0)));
    assert!(!f.param_stepped(PluginInstanceId(6), ParamId(1)));
}

/// Runs one 64-frame block of constant 1.0 through `fx`; returns the left.
fn one_block(fx: &mut core_host::adapt::PluginEffect) -> f32 {
    use core_engine::graph::EffectNode;
    let mut io = StereoBuf::new(64);
    io.left.fill(1.0);
    io.right.fill(1.0);
    fx.process_block(64, &mut io, None, &BlockInfo::default());
    io.left[63]
}

#[test]
fn a_skipped_insert_drops_the_automation_queued_meanwhile() {
    use core_engine::graph::EffectNode;
    let mut inst = load("vizdaw.test.gain");
    let proc = inst.activate(SR, 4096).unwrap();
    let ((_p, _g), audio) = param_queues();
    let info = inst.info().clone();
    let (back, _returned) = channel();
    let mut fx =
        core_host::adapt::PluginEffect::new(PluginInstanceId(1), proc, &info, 4096, audio, back);
    // Control: a queued value (normalised 0 = gain 0) is applied on a run.
    fx.automate(0, ParamId(0), 0.0);
    assert!(one_block(&mut fx).abs() < 1e-6);
    // Bypassed blocks: values pile up, then the engine reports each skip.
    for _ in 0..300 {
        fx.automate(0, ParamId(0), 0.0);
        fx.skipped();
    }
    // Back in the chain: only values queued after the return are applied.
    fx.automate(0, ParamId(0), 0.5); // gain 1.0
    assert!((one_block(&mut fx) - 1.0).abs() < 1e-6);
    // And with nothing queued after a skip, the plugin keeps its value.
    fx.automate(0, ParamId(0), 0.0);
    fx.skipped();
    assert!((one_block(&mut fx) - 1.0).abs() < 1e-6);
}
