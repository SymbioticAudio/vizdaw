//! The plugin host: loads a document's plugins (loader thread, then the
//! main thread), feeds the overlay, turns gestures into undoable commands,
//! captures state, restarts on latency changes and follows the engine's
//! block size.

mod common;

use common::plugin;
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, demo_document};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_host::{PluginHost, PluginKind};
use core_model::command::{Command, MixerCmd};
use core_model::document::Document;
use core_model::history::History;
use core_model::ids::{PluginInstanceId, SeqId};
use core_model::mixer::{ParamId, PluginFormat, StripKind};
use core_model::selection::ViewFilter;
use std::time::Duration;

const SR: f64 = 48_000.0;
const WAIT: Duration = Duration::from_secs(10);

fn host(max: u32) -> PluginHost {
    let mut h = PluginHost::new(SR, max).unwrap();
    for (p, _) in core_host::clap::list_bundle(&test_plugins::bundle()).unwrap() {
        h.register_path(PluginFormat::Clap, &p.id, test_plugins::bundle());
    }
    h
}

fn engine(h: &PluginHost, max: u32) -> (Engine, OfflineBackend) {
    let mut cfg = EngineConfig::new(SR);
    cfg.max_frames = max;
    let (mut e, r) = Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
    e.set_insert_factory(Box::new(h.factory()));
    let mut b = OfflineBackend::new(SR, 256);
    b.start(r).unwrap();
    (e, b)
}

/// The demo with a Test Gain insert on the built-in synth's strip.
fn with_gain() -> (History, SeqId, PluginInstanceId) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    (History::new(b.finish()), song, id)
}

/// Peak of the next `frames` frames.
fn peak(b: &mut OfflineBackend, frames: usize) -> f32 {
    let mut p = 0.0f32;
    let mut done = 0;
    while done < frames {
        b.block(256);
        p = b.output(256).0.iter().fold(p, |m, x| m.max(x.abs()));
        done += 256;
    }
    p
}

fn run(h: &mut History, cmd: MixerCmd) {
    h.execute(&Command::Mixer(cmd), &ViewFilter::default())
        .unwrap();
}

#[test]
fn loads_the_documents_plugins_and_plays_them() {
    let (hist, song, gain) = with_gain();
    let mut b = Builder::from_doc(hist.doc().clone());
    let (d, synth) = b
        .plugin_dest("Synth4", plugin("vizdaw.test.synth4"))
        .unwrap();
    let lane = b.lane(song, "Keys", Some(d)).unwrap();
    b.notes(song, lane, &[(0, 69, 0xFFFF, 15_360)]).unwrap();
    let doc = b.finish();
    let mut host = host(4096);
    assert!(!host.sync(&doc), "two loads in progress");
    assert_eq!(host.pending(), 2);
    let r = host.wait_ready(WAIT);
    assert_eq!(r.loaded.len(), 2, "{:?}", r.failed);
    assert!(r.reload);
    assert_eq!(
        host.instance(synth).unwrap().info().kind,
        PluginKind::Instrument
    );
    let (mut engine, mut be) = engine(&host, 4096);
    let report = engine.load(&doc, song).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    assert!(report.unroutable.is_empty(), "{:?}", report.warnings);
    engine.play().unwrap();
    assert!(peak(&mut be, 9600) > 0.0);
    assert!(host.instance(gain).is_some());
    drop(engine);
    drop(be);
}

#[test]
fn overlay_edits_and_their_undo_reach_the_running_plugin() {
    let (mut hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    engine.play().unwrap();
    peak(&mut be, 24_000);
    let full = peak(&mut be, 24_000);
    run(
        &mut hist,
        MixerCmd::SetPluginParams {
            slot: gain,
            params: vec![(ParamId(0), 0.0)],
        },
    );
    host.sync(hist.doc());
    assert_eq!(peak(&mut be, 24_000), 0.0, "gain 0 silences it");
    hist.undo().unwrap();
    host.sync(hist.doc());
    let back = peak(&mut be, 24_000);
    assert!(
        back > 0.5 * full,
        "undo restores the base value: {back} vs {full}"
    );
    drop(engine);
}

#[test]
fn ui_gestures_become_undoable_commands() {
    let (mut hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    // "Wiggle" makes the plugin perform a gesture on Gain.
    host.set_param(gain, ParamId(2), 0.4);
    peak(&mut be, 512);
    let r = host.service(&mut engine, hist.doc(), song).unwrap();
    assert_eq!(r.gestures.len(), 1);
    assert_eq!(r.gestures[0].values, vec![(ParamId(0), 0.4)]);
    PluginHost::commit_gestures(&mut hist, &r.gestures).unwrap();
    let slot = hist.doc().plugin_slot(gain).unwrap();
    assert_eq!(slot.params.get(&ParamId(0)), Some(&0.4));
    assert_eq!(hist.undo_label(), Some("Change Plugin Parameter"));
    hist.undo().unwrap();
    assert!(hist.doc().plugin_slot(gain).unwrap().params.is_empty());
    drop(engine);
}

#[test]
fn capture_stores_the_state_and_a_new_host_restores_it() {
    let (mut hist, _song, gain) = with_gain();
    run(
        &mut hist,
        MixerCmd::SetPluginParams {
            slot: gain,
            params: vec![(ParamId(0), 0.3)],
        },
    );
    let depth = hist.undo_depth();
    let doc_before: Document;
    {
        let mut host = host(4096);
        host.sync(hist.doc());
        host.wait_ready(WAIT);
        let blob = host.capture(&mut hist, gain).unwrap();
        let slot = hist.doc().plugin_slot(gain).unwrap();
        assert_eq!(slot.state, Some(blob));
        assert!(
            slot.params.is_empty(),
            "the overlay is folded into the state"
        );
        assert_eq!(hist.undo_depth(), depth, "no undo step");
        doc_before = hist.doc().clone();
    }
    let mut fresh = host(4096);
    fresh.sync(&doc_before);
    let r = fresh.wait_ready(WAIT);
    assert!(r.rejected_state.is_empty());
    assert_eq!(
        fresh.instance(gain).unwrap().param_value(ParamId(0)),
        Some(0.3)
    );
}

#[test]
fn a_latency_change_restarts_the_plugin_and_recompiles() {
    let (hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    assert_eq!(engine.latency_frames(), 0);
    host.set_param(gain, ParamId(1), 256.0);
    let mut latency = 0;
    for _ in 0..500 {
        peak(&mut be, 256);
        host.service(&mut engine, hist.doc(), song).unwrap();
        latency = engine.latency_frames();
        if latency == 256 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(latency, 256);
    assert_eq!(host.instance(gain).unwrap().info().latency, 256);
    drop(engine);
}

#[test]
fn unknown_and_unavailable_plugins_are_placeholders_with_a_reason() {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let nope = b.insert(synth, plugin("vizdaw.test.nope")).unwrap();
    let off = b.insert(synth, plugin("vizdaw.test.gain")).unwrap();
    let doc = b.finish();
    let mut host = host(4096);
    host.set_unavailable(
        PluginFormat::Clap,
        "vizdaw.test.gain",
        Some("Disabled in Plugin Manager".into()),
    );
    host.sync(&doc);
    let r = host.wait_ready(WAIT);
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].1.contains("not found"), "{:?}", r.failed);
    let (mut engine, _be) = engine(&host, 4096);
    let report = engine.load(&doc, song).unwrap();
    let why: Vec<(PluginInstanceId, String)> = report.graph.placeholders.clone();
    assert!(
        why.iter()
            .any(|(id, w)| *id == nope && w.contains("not found"))
    );
    assert!(
        why.iter()
            .any(|(id, w)| *id == off && w == "Disabled in Plugin Manager")
    );
    // Enabling it loads the waiting slot.
    host.set_unavailable(PluginFormat::Clap, "vizdaw.test.gain", None);
    host.sync(&doc);
    assert_eq!(host.wait_ready(WAIT).loaded, vec![off]);
    drop(engine);
}

#[test]
fn removed_instances_are_deactivated_and_dropped() {
    let (mut hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    peak(&mut be, 512);
    run(&mut hist, MixerCmd::RemoveInsert { slot: gain });
    host.sync(hist.doc());
    engine.load(hist.doc(), song).unwrap();
    assert_eq!(host.live_count(), 0);
    assert_eq!(host.retiring(), 1, "its processor is still in the engine");
    for _ in 0..500 {
        peak(&mut be, 256);
        host.idle();
        if host.retiring() == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(host.retiring(), 0, "given back and deactivated");
    drop(engine);
}

#[test]
fn a_larger_block_size_reactivates_plugins() {
    let (hist, song, _gain) = with_gain();
    let mut host = host(512);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 512);
    engine.load(hist.doc(), song).unwrap();
    engine.play().unwrap();
    peak(&mut be, 512);
    let mut r = be.stop().unwrap();
    r.prepare(2048);
    be.start(r).unwrap();
    let mut done = false;
    for _ in 0..500 {
        let rep = host.service(&mut engine, hist.doc(), song).unwrap();
        if let Some(c) = rep.compiled {
            done = host.config().1 == 2048 && c.graph.placeholders.is_empty();
        }
        be.block(2048);
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(done, "plugins re-activated for 2048-frame blocks");
    assert_eq!(engine.counters().xruns, 0);
    drop(engine);
}

// ---------------------------------------------------------------------------
// A scripted plugin (registered as an AU so it does not clash with the CLAP
// test bundle): its processor emits the parameter events a test queues and
// records every value the host sends, so tests can see exactly what the
// plugin holds.

use core_engine::graph::ProcessStatus;
use core_host::queue::PARAM_QUEUE;
use core_host::{
    AudioPortInfo, AudioProcessor, HostError, InstanceEvents, LoadRequest, ParamEvent, ParamInfo,
    PluginEditor, PluginFormat as HostFormat, PluginInfo, PluginInstance, PortRole, PrepareJob,
    Prepared, ProcessCtx, Tail,
};
use core_model::mixer::PluginRef;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const GAIN: ParamId = ParamId(0);
const OTHER: ParamId = ParamId(1);

#[derive(Default)]
struct Script {
    /// What the plugin holds.
    held: Mutex<BTreeMap<ParamId, f64>>,
    /// Events its processor emits at the next block.
    events: Mutex<VecDeque<ParamEvent>>,
    /// Ask for a restart at the next idle.
    restart: AtomicBool,
    /// Events the instance reports from the main thread (as AU listeners
    /// do) at the next `gestures()`.
    ui: Mutex<VecDeque<ParamEvent>>,
}

impl Script {
    fn held(&self, p: ParamId) -> f64 {
        self.held.lock().unwrap()[&p]
    }
    fn emit(&self, events: &[ParamEvent]) {
        self.events.lock().unwrap().extend(events.iter().copied());
    }
}

fn fake_ref() -> PluginRef {
    PluginRef {
        format: PluginFormat::Au,
        id: "test/fake/vizd".into(),
        name: "Fake".into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

fn fake_info() -> PluginInfo {
    let port = |name: &str| AudioPortInfo {
        name: name.into(),
        channels: 2,
        role: PortRole::Main,
    };
    let param = |id, name: &str, default| ParamInfo {
        id,
        name: name.into(),
        module: String::new(),
        min: 0.0,
        max: 10.0,
        default,
        stepped: false,
        automatable: true,
        hidden: false,
    };
    PluginInfo {
        plugin: fake_ref(),
        kind: PluginKind::Effect,
        inputs: vec![port("In")],
        outputs: vec![port("Out")],
        note_input: false,
        params: vec![param(GAIN, "Gain", 1.0), param(OTHER, "Other", 0.0)],
        latency: 0,
        tail: Tail::None,
    }
}

struct FakeFormat(Arc<Script>);

impl HostFormat for FakeFormat {
    fn format(&self) -> PluginFormat {
        PluginFormat::Au
    }
    fn prepare(&self, _req: &LoadRequest) -> PrepareJob {
        Box::new(|| Ok(Prepared(Box::new(()))))
    }
    fn finish(
        &mut self,
        _req: &LoadRequest,
        _prepared: Prepared,
    ) -> Result<Box<dyn PluginInstance>, HostError> {
        *self.0.held.lock().unwrap() = BTreeMap::from([(GAIN, 1.0), (OTHER, 0.0)]);
        Ok(Box::new(FakeInstance {
            script: self.0.clone(),
            info: fake_info(),
            active: false,
        }))
    }
}

struct FakeInstance {
    script: Arc<Script>,
    info: PluginInfo,
    active: bool,
}

struct FakeProcessor {
    script: Arc<Script>,
    blocks: u64,
}

impl AudioProcessor for FakeProcessor {
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) -> ProcessStatus {
        self.blocks += 1;
        {
            let mut held = self.script.held.lock().unwrap();
            for c in ctx.params_in {
                held.insert(c.id, c.value);
            }
        }
        // The user moves the plugin's own controls.
        while let Some(e) = self.script.events.lock().unwrap().pop_front() {
            if let ParamEvent::Value(p, v) = e {
                self.script.held.lock().unwrap().insert(p, v);
            }
            ctx.params_out.push(e);
        }
        for (o, i) in ctx.outputs.iter_mut().zip(ctx.input.iter()) {
            let n = ctx.frames as usize;
            o[..n].copy_from_slice(&i[..n]);
        }
        ProcessStatus::Ok
    }
    fn reset(&mut self) {}
}

impl PluginInstance for FakeInstance {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn activate(&mut self, _sr: f64, _max: u32) -> Result<Box<dyn AudioProcessor>, HostError> {
        assert!(!self.active, "activated twice");
        self.active = true;
        Ok(Box::new(FakeProcessor {
            script: self.script.clone(),
            blocks: 0,
        }))
    }
    fn deactivate(&mut self, _processor: Box<dyn AudioProcessor>) {
        assert!(self.active, "deactivated while inactive");
        self.active = false;
    }
    fn is_active(&self) -> bool {
        self.active
    }
    fn save_state(&mut self) -> Result<Vec<u8>, HostError> {
        let held = self.script.held.lock().unwrap();
        Ok(held
            .iter()
            .map(|(k, v)| format!("{}={v};", k.0))
            .collect::<String>()
            .into_bytes())
    }
    fn load_state(&mut self, data: &[u8]) -> Result<(), HostError> {
        let text = String::from_utf8(data.to_vec()).unwrap();
        let mut held = self.script.held.lock().unwrap();
        for kv in text.split(';').filter(|s| !s.is_empty()) {
            let (k, v) = kv.split_once('=').unwrap();
            held.insert(ParamId(k.parse().unwrap()), v.parse().unwrap());
        }
        Ok(())
    }
    fn set_params(&mut self, values: &[(ParamId, f64)]) -> Result<(), HostError> {
        let mut held = self.script.held.lock().unwrap();
        for (k, v) in values {
            held.insert(*k, *v);
        }
        Ok(())
    }
    fn param_value(&mut self, id: ParamId) -> Option<f64> {
        self.script.held.lock().unwrap().get(&id).copied()
    }
    fn editor(&mut self) -> Option<&mut dyn PluginEditor> {
        None
    }
    fn idle(&mut self) -> InstanceEvents {
        InstanceEvents {
            restart: self.script.restart.swap(false, Ordering::Relaxed),
            ..InstanceEvents::default()
        }
    }
    fn gestures(&mut self) -> Vec<ParamEvent> {
        let events: Vec<ParamEvent> = self.script.ui.lock().unwrap().drain(..).collect();
        for e in &events {
            if let ParamEvent::Value(p, v) = e {
                self.script.held.lock().unwrap().insert(*p, *v);
            }
        }
        events
    }
}

/// The demo with the scripted plugin as an insert on the synth's strip,
/// loaded (its processor on the shelf, no engine yet).
fn with_fake() -> (History, SeqId, PluginInstanceId, PluginHost, Arc<Script>) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let id = b.insert(synth, fake_ref()).unwrap();
    let hist = History::new(b.finish());
    let script = Arc::new(Script::default());
    let mut h = host(4096);
    h.add_format(Box::new(FakeFormat(script.clone())));
    h.sync(hist.doc());
    let r = h.wait_ready(WAIT);
    assert_eq!(r.loaded, vec![id], "{:?}", r.failed);
    (hist, song, id, h, script)
}

/// One timer tick: a few blocks, then `service`.
fn tick(
    host: &mut PluginHost,
    engine: &mut Engine,
    be: &mut OfflineBackend,
    hist: &History,
    song: SeqId,
) -> core_host::IdleReport {
    peak(be, 512);
    host.service(engine, hist.doc(), song).unwrap()
}

fn gain_params(hist: &History, id: PluginInstanceId) -> Vec<(ParamId, f64)> {
    let slot = hist.doc().plugin_slot(id).unwrap();
    slot.params.iter().map(|(k, v)| (*k, *v)).collect()
}

#[test]
fn a_drag_in_the_plugin_is_never_fought_and_its_undo_restores_the_base() {
    let (mut hist, song, id, mut host, script) = with_fake();
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    tick(&mut host, &mut engine, &mut be, &hist, song);
    // The user grabs Gain and moves it; the gesture stays open over ticks.
    script.emit(&[ParamEvent::Begin(GAIN), ParamEvent::Value(GAIN, 0.5)]);
    for _ in 0..4 {
        let r = tick(&mut host, &mut engine, &mut be, &hist, song);
        assert!(r.gestures.is_empty(), "still open: {:?}", r.gestures);
    }
    peak(&mut be, 512);
    assert_eq!(
        script.held(GAIN),
        0.5,
        "the host never sends the base mid-drag"
    );
    script.emit(&[ParamEvent::Value(GAIN, 0.6), ParamEvent::End(GAIN)]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(r.gestures.len(), 1);
    assert_eq!(r.gestures[0].values, vec![(GAIN, 0.6)]);
    // A tick before the caller commits must not echo the base either.
    tick(&mut host, &mut engine, &mut be, &hist, song);
    PluginHost::commit_gestures(&mut hist, &r.gestures).unwrap();
    tick(&mut host, &mut engine, &mut be, &hist, song);
    peak(&mut be, 512);
    assert_eq!(script.held(GAIN), 0.6);
    assert_eq!(gain_params(&hist, id), vec![(GAIN, 0.6)]);
    // Undo of the gesture sends the base value.
    hist.undo().unwrap();
    host.sync(hist.doc());
    peak(&mut be, 512);
    assert_eq!(script.held(GAIN), 1.0);
    drop(engine);
}

#[test]
fn main_thread_gestures_from_the_instance_are_reported_by_idle() {
    let (mut hist, song, id, mut host, script) = with_fake();
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    tick(&mut host, &mut engine, &mut be, &hist, song);
    script.ui.lock().unwrap().extend([
        ParamEvent::Begin(GAIN),
        ParamEvent::Value(GAIN, 0.7),
        ParamEvent::End(GAIN),
    ]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(r.gestures.len(), 1);
    assert_eq!(r.gestures[0].slot, id);
    assert_eq!(r.gestures[0].values, vec![(GAIN, 0.7)]);
    PluginHost::commit_gestures(&mut hist, &r.gestures).unwrap();
    tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(gain_params(&hist, id), vec![(GAIN, 0.7)]);
    assert_eq!(script.held(GAIN), 0.7);
    drop(engine);
}

#[test]
fn a_mixer_value_is_kept_until_the_document_has_it_and_a_newer_edit_wins() {
    let (mut hist, song, id, mut host, script) = with_fake();
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    // A mixer control moving; it commits only on mouse-up.
    host.set_param(id, GAIN, 0.2);
    for _ in 0..4 {
        tick(&mut host, &mut engine, &mut be, &hist, song);
    }
    peak(&mut be, 512);
    assert_eq!(script.held(GAIN), 0.2, "not reverted before the commit");
    run(
        &mut hist,
        MixerCmd::SetPluginParams {
            slot: id,
            params: vec![(GAIN, 0.2)],
        },
    );
    host.sync(hist.doc());
    peak(&mut be, 512);
    assert_eq!(script.held(GAIN), 0.2);
    // Moving it again, then an undo before that commit: the undo is a
    // newer edit of Gain and wins.
    host.set_param(id, GAIN, 0.8);
    tick(&mut host, &mut engine, &mut be, &hist, song);
    peak(&mut be, 512);
    assert_eq!(script.held(GAIN), 0.8);
    hist.undo().unwrap();
    host.sync(hist.doc());
    peak(&mut be, 512);
    assert_eq!(script.held(GAIN), 1.0, "the undo restores the base");
    drop(engine);
}

#[test]
fn a_plugin_gesture_is_not_echoed_before_it_is_committed() {
    let (hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    engine.play().unwrap();
    peak(&mut be, 24_000);
    let full = peak(&mut be, 24_000);
    // "Wiggle" makes the plugin perform a gesture setting Gain to 0.4.
    host.set_param(gain, ParamId(2), 0.4);
    peak(&mut be, 512);
    let r = host.service(&mut engine, hist.doc(), song).unwrap();
    assert_eq!(r.gestures.len(), 1);
    // Not committed yet: further ticks leave the plugin alone.
    for _ in 0..4 {
        peak(&mut be, 512);
        host.service(&mut engine, hist.doc(), song).unwrap();
    }
    peak(&mut be, 24_000);
    let now = peak(&mut be, 24_000);
    assert!(
        now > 0.3 * full && now < 0.5 * full,
        "the plugin keeps its gesture value: {now} vs {full}"
    );
    drop(engine);
}

#[test]
fn a_removed_slot_restored_before_the_engine_reloads_keeps_its_instance() {
    let (mut hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    engine.play().unwrap();
    peak(&mut be, 24_000);
    run(&mut hist, MixerCmd::RemoveInsert { slot: gain });
    host.sync(hist.doc()); // no engine load (another load could be pending)
    assert_eq!(host.retiring(), 1);
    hist.undo().unwrap();
    assert!(
        host.sync(hist.doc()),
        "the instance is revived, not reloaded"
    );
    host.wait_ready(WAIT);
    engine.load(hist.doc(), song).unwrap();
    assert_eq!(host.retiring(), 0);
    assert_eq!(host.live_count(), 1);
    // Parameters reach the processor the engine plays.
    host.set_param(gain, ParamId(0), 0.0);
    peak(&mut be, 24_000);
    assert_eq!(peak(&mut be, 24_000), 0.0);
    drop(engine);
}

#[test]
fn an_undo_past_the_first_capture_restores_the_loaded_values() {
    let (mut hist, song, id, mut host, script) = with_fake();
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    run(
        &mut hist,
        MixerCmd::SetPluginParams {
            slot: id,
            params: vec![(GAIN, 0.3)],
        },
    );
    host.sync(hist.doc());
    peak(&mut be, 512);
    assert_eq!(script.held(GAIN), 0.3);
    host.capture(&mut hist, id).unwrap();
    assert!(hist.doc().plugin_slot(id).unwrap().state.is_some());
    hist.undo().unwrap();
    let slot = hist.doc().plugin_slot(id).unwrap();
    assert!(slot.state.is_none() && slot.params.is_empty());
    host.sync(hist.doc());
    peak(&mut be, 512);
    assert_eq!(
        script.held(GAIN),
        1.0,
        "back to the value it was loaded with"
    );
    // Redo brings the edit back.
    hist.redo().unwrap();
    host.sync(hist.doc());
    peak(&mut be, 512);
    assert_eq!(script.held(GAIN), 0.3);
    drop(engine);
}

#[test]
fn unpaired_gesture_events_still_become_gestures() {
    let (hist, song, id, mut host, script) = with_fake();
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    let values = |r: &core_host::IdleReport| -> Vec<Vec<(ParamId, f64)>> {
        r.gestures
            .iter()
            .inspect(|g| assert_eq!(g.slot, id))
            .map(|g| g.values.clone())
            .collect()
    };
    // A duplicate Begin (real plugins send them).
    script.emit(&[
        ParamEvent::Begin(GAIN),
        ParamEvent::Begin(GAIN),
        ParamEvent::Value(GAIN, 0.5),
        ParamEvent::End(GAIN),
    ]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(values(&r), vec![vec![(GAIN, 0.5)]]);
    // Values outside a gesture are one-value gestures.
    script.emit(&[ParamEvent::Value(OTHER, 3.0)]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(values(&r), vec![vec![(OTHER, 3.0)]]);
    // An End without a Begin closes nothing that is open elsewhere.
    script.emit(&[ParamEvent::Value(GAIN, 0.7), ParamEvent::End(GAIN)]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(values(&r), vec![vec![(GAIN, 0.7)]]);
    // Two parameters moved together are one gesture.
    script.emit(&[
        ParamEvent::Begin(GAIN),
        ParamEvent::Begin(OTHER),
        ParamEvent::Value(GAIN, 0.1),
        ParamEvent::Value(OTHER, 0.2),
        ParamEvent::End(GAIN),
        ParamEvent::End(OTHER),
    ]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(values(&r), vec![vec![(GAIN, 0.1), (OTHER, 0.2)]]);
    // A Begin lost its End (say the queue was full): the next Begin of the
    // same parameter closes it.
    script.emit(&[ParamEvent::Begin(GAIN), ParamEvent::Value(GAIN, 0.3)]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert!(r.gestures.is_empty());
    script.emit(&[
        ParamEvent::Begin(GAIN),
        ParamEvent::Value(GAIN, 0.4),
        ParamEvent::End(GAIN),
    ]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(values(&r), vec![vec![(GAIN, 0.3)], vec![(GAIN, 0.4)]]);
    drop(engine);
}

#[test]
fn a_restart_in_the_middle_of_a_gesture_does_not_lose_it() {
    let (hist, song, _id, mut host, script) = with_fake();
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    script.emit(&[ParamEvent::Begin(GAIN), ParamEvent::Value(GAIN, 0.4)]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert!(r.gestures.is_empty());
    script.restart.store(true, Ordering::Relaxed);
    let mut gestures = Vec::new();
    let mut reloaded = false;
    for _ in 0..500 {
        let r = tick(&mut host, &mut engine, &mut be, &hist, song);
        gestures.extend(r.gestures);
        if r.compiled.is_some_and(|c| c.graph.placeholders.is_empty()) && r.restart.is_empty() {
            reloaded = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(reloaded, "restarted and loaded again");
    let values: Vec<_> = gestures.iter().map(|g| g.values.clone()).collect();
    assert_eq!(
        values,
        vec![vec![(GAIN, 0.4)]],
        "the cut-off gesture is reported"
    );
    peak(&mut be, 512);
    assert_eq!(script.held(GAIN), 0.4, "the new processor keeps the value");
    // Gestures after the restart are grouped normally.
    script.emit(&[
        ParamEvent::End(GAIN),
        ParamEvent::Begin(GAIN),
        ParamEvent::Value(GAIN, 0.9),
        ParamEvent::End(GAIN),
    ]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    let values: Vec<_> = r.gestures.iter().map(|g| g.values.clone()).collect();
    assert_eq!(values, vec![vec![(GAIN, 0.9)]]);
    drop(engine);
}

#[test]
fn a_failed_engine_load_keeps_the_report_for_the_next_service() {
    let (hist, song, _id, mut host, script) = with_fake();
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    // A gesture and a restart in the same tick; the load fails.
    script.emit(&[ParamEvent::Value(GAIN, 0.5)]);
    script.restart.store(true, Ordering::Relaxed);
    peak(&mut be, 512);
    let bad = SeqId(u64::MAX);
    assert!(host.service(&mut engine, hist.doc(), bad).is_err());
    let r = host.service(&mut engine, hist.doc(), song).unwrap();
    assert_eq!(r.gestures.len(), 1, "the gesture survives the failed load");
    assert_eq!(r.gestures[0].values, vec![(GAIN, 0.5)]);
    assert!(r.compiled.is_some(), "the owed load runs");
    drop(engine);
}

#[test]
fn a_base_value_that_did_not_fit_the_queue_is_sent_again() {
    let (mut hist, song, id, mut host, script) = with_fake();
    // No engine yet: the processor waits on the shelf and its queue does
    // not drain.
    run(
        &mut hist,
        MixerCmd::SetPluginParams {
            slot: id,
            params: vec![(GAIN, 0.5)],
        },
    );
    host.sync(hist.doc());
    for i in 0..PARAM_QUEUE {
        host.set_param(id, OTHER, i as f64 / 1000.0);
    }
    hist.undo().unwrap();
    host.sync(hist.doc()); // the base does not fit
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    peak(&mut be, 8192); // drains the queue
    assert_eq!(script.held(GAIN), 0.5);
    host.sync(hist.doc());
    peak(&mut be, 512);
    assert_eq!(script.held(GAIN), 1.0, "the base was sent again");
    drop(engine);
}

// ---------------------------------------------------------------------------
// What the app needs (plan 3c): held slots (safe mode), the plugins loading
// now (crash marker), gestures whose End was lost, closing an editor.

#[test]
fn held_slots_are_placeholders_until_released() {
    let (hist, song, gain) = with_gain();
    let mut host = host(4096);
    host.hold(gain, Some("Plugins off".into()));
    assert!(host.sync(hist.doc()), "a held slot does not load");
    assert_eq!(
        host.state(gain),
        Some(core_host::SlotState::Failed("Plugins off".into()))
    );
    assert_eq!(host.held(), vec![gain]);
    let (mut engine, _be) = engine(&host, 4096);
    let report = engine.load(hist.doc(), song).unwrap();
    assert_eq!(
        report.graph.placeholders,
        vec![(gain, "Plugins off".to_string())]
    );
    host.hold(gain, None);
    assert!(host.held().is_empty());
    assert!(!host.sync(hist.doc()), "released: it loads");
    assert_eq!(host.state(gain), Some(core_host::SlotState::Loading));
    let r = host.wait_ready(WAIT);
    assert_eq!(r.loaded, vec![gain], "{:?}", r.failed);
    assert_eq!(host.state(gain), Some(core_host::SlotState::Live));
    drop(engine);
}

#[test]
fn loading_names_the_plugins_in_flight() {
    let (hist, _song, gain) = with_gain();
    let mut host = host(4096);
    assert!(host.loading().is_empty());
    host.sync(hist.doc());
    let names: Vec<(PluginInstanceId, String)> = host
        .loading()
        .into_iter()
        .map(|(id, p)| (id, p.id))
        .collect();
    assert_eq!(names, vec![(gain, "vizdaw.test.gain".to_string())]);
    host.wait_ready(WAIT);
    assert!(host.loading().is_empty());
    assert_eq!(host.state(PluginInstanceId(999)), None);
}

#[test]
fn a_gesture_whose_end_was_lost_closes_after_the_timeout() {
    let (hist, song, id, mut host, script) = with_fake();
    host.set_gesture_timeout(Duration::from_millis(50));
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    script.emit(&[ParamEvent::Begin(GAIN), ParamEvent::Value(GAIN, 0.5)]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert!(r.gestures.is_empty(), "still open");
    std::thread::sleep(Duration::from_millis(80));
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(r.gestures.len(), 1, "closed by the timeout");
    assert_eq!(r.gestures[0].slot, id);
    assert_eq!(r.gestures[0].values, vec![(GAIN, 0.5)]);
    drop(engine);
}

#[test]
fn closing_the_editor_ends_its_gesture() {
    let (hist, song, id, mut host, script) = with_fake();
    let (mut engine, mut be) = engine(&host, 4096);
    engine.load(hist.doc(), song).unwrap();
    script.emit(&[ParamEvent::Begin(GAIN), ParamEvent::Value(GAIN, 0.25)]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert!(r.gestures.is_empty());
    host.end_gestures(id);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert_eq!(r.gestures.len(), 1);
    assert_eq!(r.gestures[0].values, vec![(GAIN, 0.25)]);
    // A later End of the closed gesture opens nothing.
    script.emit(&[ParamEvent::End(GAIN)]);
    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
    assert!(r.gestures.is_empty());
    drop(engine);
}
