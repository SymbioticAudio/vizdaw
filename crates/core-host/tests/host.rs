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
