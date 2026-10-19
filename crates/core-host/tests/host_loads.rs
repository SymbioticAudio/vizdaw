//! When the host loads the engine: a load it owes is never lost (a load
//! finishing while another is pending, an edit deferred by a load that then
//! fails), a graph the engine refuses is not retried (and its processors
//! are not activated again) until something changes, a stale prepared load
//! never finishes a re-added slot, and plugin notifications reach the app.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, demo_document};
use core_engine::graph::ProcessStatus;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_host::{
    AudioPortInfo, AudioProcessor, HostError, InstanceEvents, LoadRequest, ParamEvent,
    PluginEditor, PluginFormat, PluginHost, PluginInfo, PluginInstance, PluginKind, PortRole,
    PrepareJob, Prepared, ProcessCtx, Tail,
};
use core_model::command::{Command, MixerCmd};
use core_model::history::History;
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef, StripKind};
use core_model::selection::ViewFilter;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SR: f64 = 48_000.0;
const WAIT: Duration = Duration::from_secs(10);
/// More latency than the engine's 2 s limit: its graph cannot be planned.
const TOO_LATE: u32 = 200_000;

fn r(id: &str) -> PluginRef {
    PluginRef {
        format: Format::Au,
        id: id.into(),
        name: id.into(),
        vendor: "VizDAW".into(),
        version: "1".into(),
    }
}

/// What the fake plugins do, shared with the test.
#[derive(Default)]
struct Script {
    activations: AtomicUsize,
    restart: AtomicBool,
    params_changed: AtomicBool,
    state_dirty: AtomicBool,
    editor_closes: AtomicUsize,
    ui: Mutex<VecDeque<ParamEvent>>,
}

/// `bad` fails on the loader after 60 ms; `far` has too much latency;
/// anything else loads at once. The prepared value names the plugin it was
/// prepared for, and the instance reports that plugin.
struct F(Arc<Script>);

impl PluginFormat for F {
    fn format(&self) -> Format {
        Format::Au
    }
    fn prepare(&self, req: &LoadRequest) -> PrepareJob {
        let id = req.plugin.id.clone();
        Box::new(move || {
            if id == "bad" {
                std::thread::sleep(Duration::from_millis(60));
                return Err(HostError::LoadFailed("nope".into()));
            }
            Ok(Prepared(Box::new(id)))
        })
    }
    fn finish(
        &mut self,
        _req: &LoadRequest,
        p: Prepared,
    ) -> Result<Box<dyn PluginInstance>, HostError> {
        let id = *p.0.downcast::<String>().unwrap();
        let port = |n: &str| AudioPortInfo {
            name: n.into(),
            channels: 2,
            role: PortRole::Main,
        };
        let info = PluginInfo {
            plugin: r(&id),
            kind: PluginKind::Effect,
            inputs: vec![port("In")],
            outputs: vec![port("Out")],
            note_input: false,
            params: vec![],
            latency: if id == "far" { TOO_LATE } else { 0 },
            tail: Tail::None,
        };
        Ok(Box::new(I {
            info,
            editor: E(self.0.clone()),
            script: self.0.clone(),
            active: false,
        }))
    }
}

/// An editor that counts its closes.
struct E(Arc<Script>);

impl PluginEditor for E {
    fn open(&mut self, _p: core_host::ParentView) -> Result<(u32, u32), HostError> {
        Ok((10, 10))
    }
    fn resize(&mut self, w: u32, h: u32) -> Result<(u32, u32), HostError> {
        Ok((w, h))
    }
    fn can_resize(&mut self) -> bool {
        false
    }
    fn close(&mut self) {
        self.0.editor_closes.fetch_add(1, Ordering::Relaxed);
    }
}

struct I {
    editor: E,
    info: PluginInfo,
    script: Arc<Script>,
    active: bool,
}

struct P;

impl AudioProcessor for P {
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) -> ProcessStatus {
        for (o, i) in ctx.outputs.iter_mut().zip(ctx.input.iter()) {
            let n = ctx.frames as usize;
            o[..n].copy_from_slice(&i[..n]);
        }
        ProcessStatus::Ok
    }
    fn reset(&mut self) {}
}

impl PluginInstance for I {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn activate(&mut self, _sr: f64, _m: u32) -> Result<Box<dyn AudioProcessor>, HostError> {
        assert!(!self.active, "activated twice");
        self.script.activations.fetch_add(1, Ordering::Relaxed);
        self.active = true;
        Ok(Box::new(P))
    }
    fn deactivate(&mut self, _p: Box<dyn AudioProcessor>) {
        self.active = false;
    }
    fn is_active(&self) -> bool {
        self.active
    }
    fn save_state(&mut self) -> Result<Vec<u8>, HostError> {
        Ok(vec![])
    }
    fn load_state(&mut self, _d: &[u8]) -> Result<(), HostError> {
        Ok(())
    }
    fn set_params(&mut self, _v: &[(ParamId, f64)]) -> Result<(), HostError> {
        Ok(())
    }
    fn param_value(&mut self, _id: ParamId) -> Option<f64> {
        None
    }
    fn editor(&mut self) -> Option<&mut dyn PluginEditor> {
        Some(&mut self.editor)
    }
    fn idle(&mut self) -> InstanceEvents {
        InstanceEvents {
            restart: self.script.restart.swap(false, Ordering::Relaxed),
            latency_changed: false,
            params_changed: self.script.params_changed.swap(false, Ordering::Relaxed),
            state_dirty: self.script.state_dirty.swap(false, Ordering::Relaxed),
        }
    }
    fn gestures(&mut self) -> Vec<ParamEvent> {
        self.script.ui.lock().unwrap().drain(..).collect()
    }
}

/// Fields drop in order: the backend and the engine before the host.
struct Rig {
    be: OfflineBackend,
    engine: Engine,
    host: PluginHost,
    hist: History,
    song: SeqId,
    synth: StripId,
    script: Arc<Script>,
}

impl Rig {
    /// The demo with the given inserts on the synth's strip, loaded by the
    /// host (`wait_ready`) but not by the engine.
    fn new(inserts: &[&str]) -> (Rig, Vec<PluginInstanceId>) {
        let doc = demo_document().unwrap();
        let song = find_sequence(&doc, "Song").unwrap();
        let mut b = Builder::from_doc(doc);
        let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
        let ids = inserts
            .iter()
            .map(|p| b.insert(synth, r(p)).unwrap())
            .collect();
        let hist = History::new(b.finish());
        let script = Arc::new(Script::default());
        let mut host = PluginHost::new(SR, 4096).unwrap();
        host.add_format(Box::new(F(script.clone())));
        host.sync(hist.doc());
        host.wait_ready(WAIT);
        let mut cfg = EngineConfig::new(SR);
        cfg.max_frames = 4096;
        let (mut engine, rr) =
            Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
        engine.set_insert_factory(Box::new(host.factory()));
        let mut be = OfflineBackend::new(SR, 256);
        be.start(rr).unwrap();
        (
            Rig {
                hist,
                song,
                synth,
                host,
                engine,
                be,
                script,
            },
            ids,
        )
    }

    fn acts(&self) -> usize {
        self.script.activations.load(Ordering::Relaxed)
    }

    fn edit(&mut self, cmd: MixerCmd) {
        self.hist
            .execute(&Command::Mixer(cmd), &ViewFilter::default())
            .unwrap();
    }

    /// One timer tick: a block, a little time for the loader, `service`.
    fn tick(&mut self) -> core_host::IdleReport {
        self.tick_seq(self.song).unwrap()
    }

    fn tick_seq(&mut self, seq: SeqId) -> Result<core_host::IdleReport, core_engine::EngineError> {
        self.be.block(256);
        std::thread::sleep(Duration::from_millis(5));
        self.host.service(&mut self.engine, self.hist.doc(), seq)
    }
}

#[test]
fn a_finished_load_is_not_lost_when_a_later_load_fails() {
    let (mut rig, _) = Rig::new(&[]);
    rig.engine.load(rig.hist.doc(), rig.song).unwrap();
    // The user adds a good plugin, then one that fails slowly: the good
    // one finishes while the bad one is still loading.
    let synth = rig.synth;
    rig.edit(MixerCmd::AddInsert {
        strip: synth,
        index: None,
        plugin: r("good"),
    });
    rig.edit(MixerCmd::AddInsert {
        strip: synth,
        index: None,
        plugin: r("bad"),
    });
    rig.host.sync(rig.hist.doc());
    let mut compiled = None;
    for _ in 0..40 {
        if let Some(c) = rig.tick().compiled {
            compiled = Some(c);
        }
    }
    let c = compiled.expect("the good plugin never reached the engine");
    assert_eq!(c.graph.placeholders.len(), 1, "{:?}", c.graph);
    assert!(c.graph.placeholders[0].1.contains("nope"));
}

#[test]
fn an_edit_deferred_by_a_failing_load_reaches_the_engine() {
    let (mut rig, _) = Rig::new(&[]);
    rig.engine.load(rig.hist.doc(), rig.song).unwrap();
    let synth = rig.synth;
    rig.edit(MixerCmd::AddInsert {
        strip: synth,
        index: None,
        plugin: r("bad"),
    });
    assert!(
        !rig.host.sync(rig.hist.doc()),
        "the edit waits for the load"
    );
    let mut compiled = 0;
    let mut failed = 0;
    for _ in 0..20 {
        let r = rig.tick();
        failed += r.failed.len();
        compiled += usize::from(r.compiled.is_some());
    }
    assert_eq!(failed, 1);
    assert_eq!(compiled, 1, "the edited document is loaded once");
}

#[test]
fn a_graph_the_engine_refuses_is_not_retried_and_its_plugins_are_not_reactivated() {
    let (mut rig, _) = Rig::new(&["far"]);
    let rep = rig.engine.load(rig.hist.doc(), rig.song).unwrap();
    assert!(rep.graph.error.is_some(), "{:?}", rep.graph);
    let before = rig.acts();
    for _ in 0..10 {
        let r = rig.tick();
        assert!(r.compiled.is_none(), "no retry of an identical load");
    }
    assert_eq!(rig.acts(), before, "no activation churn");
}

#[test]
fn a_refused_load_is_reported_once_and_retried_after_an_edit() {
    let (mut rig, _) = Rig::new(&[]);
    rig.engine.load(rig.hist.doc(), rig.song).unwrap();
    let synth = rig.synth;
    rig.edit(MixerCmd::AddInsert {
        strip: synth,
        index: None,
        plugin: r("far"),
    });
    rig.host.sync(rig.hist.doc());
    let mut refused = 0;
    for _ in 0..20 {
        let r = rig.tick();
        if let Some(c) = r.compiled {
            assert!(c.graph.error.is_some());
            refused += 1;
        }
    }
    assert_eq!(refused, 1, "the refusal is reported once");
    assert_eq!(rig.acts(), 1, "activated once, when it loaded");
    // Removing it is a change: the owed load runs and succeeds.
    let far = rig
        .hist
        .doc()
        .mixer()
        .inserts()
        .next()
        .map(|(_, _, s)| s.id);
    rig.edit(MixerCmd::RemoveInsert { slot: far.unwrap() });
    let mut ok = 0;
    for _ in 0..10 {
        let r = rig.tick();
        if let Some(c) = r.compiled {
            assert!(c.graph.error.is_none(), "{:?}", c.graph);
            ok += 1;
        }
    }
    assert_eq!(ok, 1);
}

#[test]
fn a_persistent_load_error_hands_out_gestures_and_is_not_retried() {
    let (mut rig, _) = Rig::new(&["good"]);
    rig.engine.load(rig.hist.doc(), rig.song).unwrap();
    rig.tick();
    // A gesture and a restart in the same tick; the load cannot succeed
    // while the app asks for an unknown sequence.
    rig.script.ui.lock().unwrap().extend([
        ParamEvent::Begin(ParamId(0)),
        ParamEvent::Value(ParamId(0), 0.5),
        ParamEvent::End(ParamId(0)),
    ]);
    rig.script.restart.store(true, Ordering::Relaxed);
    let bad = SeqId(u64::MAX);
    assert!(rig.tick_seq(bad).is_err(), "the refusal is reported");
    let r = rig.tick_seq(bad).expect("not retried");
    assert_eq!(r.gestures.len(), 1, "gestures are handed out");
    let acts = rig.acts();
    for _ in 0..10 {
        let r = rig.tick_seq(bad).expect("not retried");
        assert!(r.gestures.is_empty() && r.compiled.is_none());
    }
    assert_eq!(rig.acts(), acts, "no activation churn");
    // The right sequence is a change: the owed load runs.
    let mut ok = false;
    for _ in 0..50 {
        let r = rig.tick();
        if r.compiled.is_some_and(|c| c.graph.placeholders.is_empty()) {
            ok = true;
            break;
        }
    }
    assert!(ok, "the restarted plugin is loaded again");
}

#[test]
fn a_stale_prepared_load_does_not_finish_a_reused_slot() {
    // Two slots finish preparing before the host finishes them (one per
    // idle); meanwhile the second slot is given another plugin.
    let doc = demo_document().unwrap();
    let mut b = Builder::from_doc(doc.clone());
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.insert(synth, r("one")).unwrap();
    let base = b.finish();
    let mut b1 = Builder::from_doc(base.clone());
    let id = b1.insert(synth, r("two")).unwrap();
    let doc1 = b1.finish();
    let mut b2 = Builder::from_doc(base);
    assert_eq!(b2.insert(synth, r("other")).unwrap(), id);
    let doc2 = b2.finish();
    let script = Arc::new(Script::default());
    let mut host = PluginHost::new(SR, 4096).unwrap();
    host.add_format(Box::new(F(script)));
    host.sync(&doc1);
    std::thread::sleep(Duration::from_millis(100));
    let r1 = host.idle();
    assert_eq!(r1.loaded.len(), 1, "one finished, one waits");
    host.sync(&doc2);
    let r2 = host.wait_ready(WAIT);
    assert_eq!(r2.loaded, vec![id]);
    assert_eq!(host.instance(id).unwrap().info().plugin.id, "other");
}

#[test]
fn params_changed_and_state_dirty_are_reported() {
    let (mut rig, ids) = Rig::new(&["good"]);
    rig.engine.load(rig.hist.doc(), rig.song).unwrap();
    let r = rig.tick();
    assert!(r.params_changed.is_empty() && r.state_dirty.is_empty());
    rig.script.params_changed.store(true, Ordering::Relaxed);
    rig.script.state_dirty.store(true, Ordering::Relaxed);
    let r = rig.tick();
    assert_eq!(r.params_changed, ids);
    assert_eq!(r.state_dirty, ids);
}

#[test]
fn a_removed_slots_editor_is_closed_even_while_its_processor_is_out() {
    let (mut rig, ids) = Rig::new(&["good"]);
    rig.engine.load(rig.hist.doc(), rig.song).unwrap();
    rig.tick();
    assert_eq!(rig.script.editor_closes.load(Ordering::Relaxed), 0);
    rig.edit(MixerCmd::RemoveInsert { slot: ids[0] });
    rig.host.sync(rig.hist.doc());
    assert_eq!(
        rig.script.editor_closes.load(Ordering::Relaxed),
        1,
        "closed when the slot left, before the instance retires"
    );
    assert!(rig.host.instance(ids[0]).is_none());
}
