//! VST3 plug-ins through the format and the plug-in host (host track H1
//! Task 6), mirroring `clap_host.rs` and `host.rs`: loading, playing, the
//! overlay, controller gestures as undoable commands, a latency restart,
//! state through a saved song, offline copies and placeholders. They run
//! on the in-repo Rust test VST3 (a private copy per test) and, where they
//! are built, on the VST3 SDK's sample plug-ins (`common::vst3::sample`).

mod common;

use common::vst3::{Block, load_vst3, private, sample, sdk, vst3_ref};
use core_engine::backend::{AudioBackend, BlockInfo};
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, demo_document};
use core_engine::graph::ProcessStatus;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_host::vst3::{Vst3Format, list_bundle};
use core_host::{
    LoadRequest, MidiEvent, OfflineRender, ParamChange, PluginFormat as _, PluginHost,
    PluginInstance, PluginKind, PortRole,
};
use core_model::command::{Command, MixerCmd};
use core_model::history::History;
use core_model::ids::{PluginInstanceId, SeqId};
use core_model::mixer::{ParamId, PluginFormat, StripKind};
use core_model::selection::ViewFilter;
use std::path::Path;
use std::time::Duration;
use test_plugins::{
    VST3_GAIN_ID, VST3_PARAM_GAIN, VST3_PARAMS, VST3_SYNTH_ID, Vst3Edit, vst3_restart,
};

const SR: f64 = 48_000.0;
const WAIT: Duration = Duration::from_secs(10);

fn pid(id: u32) -> ParamId {
    ParamId(u64::from(id))
}

/// Test Gain's normalised value for `db`.
fn gain_norm(db: f64) -> f64 {
    VST3_PARAMS[0].to_normalized(db)
}

fn factor(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// A host that finds the classes `ids` in the bundle at `path`.
fn host_for(path: &Path, ids: &[&str]) -> PluginHost {
    let mut h = PluginHost::new(SR, 4096).unwrap();
    for id in ids {
        h.register_path(PluginFormat::Vst3, id, path.to_path_buf());
    }
    h
}

fn engine(h: &PluginHost) -> (Engine, OfflineBackend) {
    let mut cfg = EngineConfig::new(SR);
    cfg.max_frames = 4096;
    let (mut e, r) = Engine::new(cfg, Box::new(OfflineSinks::new(1 << 16).with_tone())).unwrap();
    e.set_insert_factory(Box::new(h.factory()));
    let mut b = OfflineBackend::new(SR, 256);
    b.start(r).unwrap();
    (e, b)
}

/// The demo with an insert of VST3 class `id` on the built-in synth.
fn with_insert(id: &str, name: &str) -> (History, SeqId, PluginInstanceId) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let slot = b.insert(synth, vst3_ref(id, name)).unwrap();
    (History::new(b.finish()), song, slot)
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

// ---------------------------------------------------------------------
// The Rust test VST3
// ---------------------------------------------------------------------

#[test]
fn the_format_loads_the_test_classes_and_they_process() {
    let p = private();
    let mut gain = load_vst3(&p.path, VST3_GAIN_ID);
    let info = gain.info().clone();
    assert_eq!(info.kind, PluginKind::Effect);
    assert_eq!(info.inputs[0].role, PortRole::Main);
    assert_eq!(info.sidechain_input(), Some(1));
    gain.set_params(&[(pid(VST3_PARAM_GAIN), -6.0)]).unwrap();
    let mut proc = gain.activate(SR, 512).unwrap();
    let mut b = Block::new(&info, 512);
    b.fill(0.5, 0.0);
    assert_eq!(b.quiet(proc.as_mut(), 64), ProcessStatus::Ok);
    let want = 0.5 * factor(-6.0);
    assert!(b.outputs.iter().all(|c| (c[0] - want).abs() < 1e-6));
    gain.deactivate(proc);
    assert_eq!(gain.param_value(pid(VST3_PARAM_GAIN)), Some(-6.0));

    let mut synth = load_vst3(&p.path, VST3_SYNTH_ID);
    let info = synth.info().clone();
    assert_eq!(info.kind, PluginKind::Instrument);
    assert!(info.note_input && info.inputs.is_empty());
    assert_eq!(info.outputs.len(), 2);
    let mut proc = synth.activate(SR, 512).unwrap();
    let mut b = Block::new(&info, 512);
    let on = [MidiEvent::new(0, &[0x90, 60, 100])];
    b.run(proc.as_mut(), 64, &on, &[], &BlockInfo::default());
    assert_eq!(b.outputs[0][0], 0.5, "the held note on output 1");
    assert_eq!(b.outputs[2][0], 0.25, "output 2");
    synth.deactivate(proc);
}

#[test]
fn unknown_classes_and_missing_bundles_are_errors() {
    let mut f = Vst3Format::new();
    let mut req = LoadRequest {
        plugin: vst3_ref(&"0".repeat(32), "Nope"),
        path: Some(test_plugins::vst3_bundle()),
        sample_rate: SR,
        max_frames: 512,
    };
    let prepared = (f.prepare(&req))().unwrap();
    assert!(f.finish(&req, prepared).is_err(), "an unknown class loaded");
    req.path = Some("/nonexistent/X.vst3".into());
    assert!((f.prepare(&req))().is_err());
    req.path = None;
    assert!((f.prepare(&req))().is_err());
}

#[test]
fn the_host_loads_a_documents_vst3_plugins_and_plays_them() {
    let p = private();
    let (hist, song, gain) = with_insert(VST3_GAIN_ID, "Test Gain");
    let mut b = Builder::from_doc(hist.doc().clone());
    let (d, synth) = b
        .plugin_dest("VSynth", vst3_ref(VST3_SYNTH_ID, "Test Synth"))
        .unwrap();
    let lane = b.lane(song, "Keys", Some(d)).unwrap();
    b.notes(song, lane, &[(0, 69, 0xFFFF, 15_360)]).unwrap();
    let doc = b.finish();
    let mut host = host_for(&p.path, &[VST3_GAIN_ID, VST3_SYNTH_ID]);
    host.sync(&doc);
    let r = host.wait_ready(WAIT);
    assert_eq!(r.loaded.len(), 2, "{:?}", r.failed);
    assert_eq!(
        host.instance(synth).unwrap().info().kind,
        PluginKind::Instrument
    );
    let (mut engine, mut be) = engine(&host);
    let report = engine.load(&doc, song).unwrap();
    assert!(report.graph.placeholders.is_empty(), "{:?}", report.graph);
    assert!(report.unroutable.is_empty(), "{:?}", report.warnings);
    engine.play().unwrap();
    assert!(peak(&mut be, 9600) > 0.0);
    assert!(p.probe.counters().process_calls > 0, "the plug-ins ran");
    assert!(host.instance(gain).unwrap().is_active());
    drop(engine);
}

#[test]
fn overlay_edits_and_their_undo_reach_the_running_vst3() {
    let p = private();
    let (mut hist, song, gain) = with_insert(VST3_GAIN_ID, "Test Gain");
    // The base: 0 dB (the component starts at -42 dB).
    run(
        &mut hist,
        MixerCmd::SetPluginParams {
            slot: gain,
            params: vec![(pid(VST3_PARAM_GAIN), 0.0)],
        },
    );
    let mut host = host_for(&p.path, &[VST3_GAIN_ID]);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host);
    engine.load(hist.doc(), song).unwrap();
    engine.play().unwrap();
    peak(&mut be, 24_000);
    let base = peak(&mut be, 24_000);
    assert!(base > 0.0);
    run(
        &mut hist,
        MixerCmd::SetPluginParams {
            slot: gain,
            params: vec![(pid(VST3_PARAM_GAIN), -60.0)],
        },
    );
    host.sync(hist.doc());
    peak(&mut be, 512);
    let quiet = peak(&mut be, 24_000);
    assert!(quiet < base * 0.01, "-60 dB: {quiet} vs {base}");
    hist.undo().unwrap();
    host.sync(hist.doc());
    peak(&mut be, 512);
    let back = peak(&mut be, 24_000);
    assert!(
        back > 0.5 * base,
        "undo restores the base: {back} vs {base}"
    );
    drop(engine);
}

#[test]
fn controller_gestures_become_undoable_commands() {
    let p = private();
    let (mut hist, song, gain) = with_insert(VST3_GAIN_ID, "Test Gain");
    let mut host = host_for(&p.path, &[VST3_GAIN_ID]);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host);
    engine.load(hist.doc(), song).unwrap();
    // The controller drags Gain, as its editor would.
    p.probe.edit(Vst3Edit::Begin(VST3_PARAM_GAIN));
    p.probe
        .edit(Vst3Edit::Perform(VST3_PARAM_GAIN, gain_norm(-12.0)));
    p.probe.edit(Vst3Edit::End(VST3_PARAM_GAIN));
    peak(&mut be, 512);
    let r = host.service(&mut engine, hist.doc(), song).unwrap();
    assert_eq!(r.gestures.len(), 1, "{:?}", r.gestures);
    let (id, v) = r.gestures[0].values[0];
    assert_eq!(id, pid(VST3_PARAM_GAIN));
    assert!((v + 12.0).abs() < 1e-9, "plain dB: {v}");
    PluginHost::commit_gestures(&mut hist, &r.gestures).unwrap();
    let slot = hist.doc().plugin_slot(gain).unwrap();
    assert!(slot.params.get(&pid(VST3_PARAM_GAIN)).is_some());
    assert_eq!(hist.undo_label(), Some("Change Plugin Parameter"));
    hist.undo().unwrap();
    assert!(hist.doc().plugin_slot(gain).unwrap().params.is_empty());
    drop(engine);
}

#[test]
fn a_latency_change_restarts_the_vst3_and_recompiles() {
    let p = private();
    let (hist, song, gain) = with_insert(VST3_GAIN_ID, "Test Gain");
    let mut host = host_for(&p.path, &[VST3_GAIN_ID]);
    host.sync(hist.doc());
    host.wait_ready(WAIT);
    let (mut engine, mut be) = engine(&host);
    engine.load(hist.doc(), song).unwrap();
    assert_eq!(engine.latency_frames(), 0);
    p.probe.set_timing(256, 0);
    p.probe
        .edit(Vst3Edit::Restart(vst3_restart::LATENCY_CHANGED));
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
fn both_state_streams_survive_a_saved_song() {
    let p = private();
    let (mut hist, song, gain) = with_insert(VST3_GAIN_ID, "Test Gain");
    run(
        &mut hist,
        MixerCmd::SetPluginParams {
            slot: gain,
            params: vec![(pid(VST3_PARAM_GAIN), -12.0)],
        },
    );
    // The controller's own state carries a memo no parameter has.
    p.probe.set_memo(41);
    {
        let mut host = host_for(&p.path, &[VST3_GAIN_ID]);
        host.sync(hist.doc());
        host.wait_ready(WAIT);
        // A VST3 component takes values in a processed block, so the
        // engine plays one before the capture (see the Task 6 report).
        let (mut engine, mut be) = engine(&host);
        engine.load(hist.doc(), song).unwrap();
        peak(&mut be, 512);
        let blob = host.capture(&mut hist, gain).unwrap();
        let slot = hist.doc().plugin_slot(gain).unwrap();
        assert_eq!(slot.state, Some(blob));
        assert!(slot.params.is_empty(), "the overlay is in the state");
        drop(engine);
    }
    let d = tempfile::tempdir().unwrap();
    let package = d.path().join("Song.vizdaw");
    core_model::file::save(hist.doc(), &package).unwrap();
    let doc = core_model::file::load(&package).unwrap();
    p.probe.set_memo(0);
    let mut fresh = host_for(&p.path, &[VST3_GAIN_ID]);
    fresh.sync(&doc);
    let r = fresh.wait_ready(WAIT);
    assert!(r.rejected_state.is_empty(), "{:?}", r.rejected_state);
    let v = fresh
        .instance(gain)
        .unwrap()
        .param_value(pid(VST3_PARAM_GAIN))
        .unwrap();
    assert!((v + 12.0).abs() < 1e-9, "{v}");
    assert_eq!(p.probe.counters().last_memo, 41, "the controller's stream");
}

/// Plan 5f-2 Ruling 7, for VST3: a bounce host's copies are set up with
/// `kOffline` before they activate; a live host never asks.
#[test]
fn a_bounce_host_renders_its_vst3_copies_offline() {
    let p = private();
    let (hist, _, gain) = with_insert(VST3_GAIN_ID, "Test Gain");
    let mut copies = host_for(&p.path, &[VST3_GAIN_ID]);
    copies.set_offline_render(true);
    copies.sync(hist.doc());
    copies.wait_ready(WAIT);
    assert_eq!(copies.offline_state(gain), Some(OfflineRender::Taken));
    let offline = 2; // the SDK's kOffline
    assert_eq!(p.probe.counters().last_process_mode, offline);
    let mut live = host_for(&p.path, &[VST3_GAIN_ID]);
    live.sync(hist.doc());
    live.wait_ready(WAIT);
    assert_eq!(live.offline_state(gain), None, "a live host never asks");
}

#[test]
fn an_unknown_vst3_class_is_a_placeholder_with_a_reason() {
    let p = private();
    let nope = "0".repeat(32);
    let (hist, song, slot) = with_insert(&nope, "Nope");
    let mut host = host_for(&p.path, &[&nope]);
    host.sync(hist.doc());
    let r = host.wait_ready(WAIT);
    assert_eq!(r.failed.len(), 1, "{:?}", r.failed);
    let (mut engine, _be) = engine(&host);
    let report = engine.load(hist.doc(), song).unwrap();
    assert!(
        report.graph.placeholders.iter().any(|(id, _)| *id == slot),
        "{:?}",
        report.graph.placeholders
    );
}

// ---------------------------------------------------------------------
// The VST3 SDK samples (skipped when not built; required in CI)
// ---------------------------------------------------------------------

#[test]
fn the_sdk_samples_list_their_classes_and_kinds() {
    let want = [
        ("again.vst3", sdk::AGAIN, PluginKind::Effect),
        ("again.vst3", sdk::AGAIN_SIDECHAIN, PluginKind::Effect),
        (
            "again-sample-accurate.vst3",
            sdk::AGAIN_SAMPLE_ACCURATE,
            PluginKind::Effect,
        ),
        ("adelay.vst3", sdk::ADELAY, PluginKind::Effect),
        (
            "note-expression-synth.vst3",
            sdk::NOTE_EXPRESSION_SYNTH,
            PluginKind::Instrument,
        ),
        // Its sub-categories are `Fx|Instrument`.
        (
            "host-checker.vst3",
            sdk::HOST_CHECKER,
            PluginKind::Instrument,
        ),
        ("panner.vst3", sdk::PANNER, PluginKind::Effect),
    ];
    for (bundle, id, kind) in want {
        let Some(path) = sample(bundle) else { return };
        let list = list_bundle(&path).unwrap();
        let found = list.iter().find(|(p, _)| p.id == id);
        assert_eq!(
            found.map(|(_, k)| *k),
            Some(kind),
            "{bundle} {id}: {list:?}"
        );
    }
}

#[test]
fn sdk_again_scales_by_its_gain_and_its_sidechain_class_has_a_sidechain() {
    let Some(path) = sample("again.vst3") else {
        return;
    };
    let mut inst = load_vst3(&path, sdk::AGAIN);
    let info = inst.info().clone();
    let names: Vec<&str> = info.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Gain", "VuPPM", "Bypass"]);
    assert!(info.params[1].read_only);
    inst.set_params(&[(ParamId(0), 0.25)]).unwrap();
    let mut proc = inst.activate(SR, 512).unwrap();
    let mut b = Block::new(&info, 512);
    b.fill(0.8, 0.0);
    assert_eq!(b.quiet(proc.as_mut(), 256), ProcessStatus::Ok);
    for c in &b.outputs {
        assert!(
            c[..256].iter().all(|x| (x - 0.2).abs() < 1e-6),
            "{:?}",
            &c[..4]
        );
    }
    // A host value in the block applies to it.
    let half = [ParamChange {
        frame: 0,
        id: ParamId(0),
        value: 0.5,
    }];
    b.run(proc.as_mut(), 256, &[], &half, &BlockInfo::default());
    assert!((b.outputs[0][255] - 0.4).abs() < 1e-6);
    inst.deactivate(proc);
    assert_eq!(
        inst.param_value(ParamId(0)),
        Some(0.5),
        "echoed to the controller"
    );

    let mut side = load_vst3(&path, sdk::AGAIN_SIDECHAIN);
    let info = side.info().clone();
    assert_eq!(info.sidechain_input(), Some(1));
    assert_eq!(info.inputs[1].channels, 1);
    let mut proc = side.activate(SR, 512).unwrap();
    let mut b = Block::new(&info, 512);
    b.fill(0.8, 0.3);
    assert_eq!(b.quiet(proc.as_mut(), 256), ProcessStatus::Ok);
    assert!(b.outputs[0][..256].iter().all(|x| x.is_finite()));
    side.deactivate(proc);
}

#[test]
fn sdk_again_sample_accurate_takes_a_change_at_its_frame() {
    let Some(path) = sample("again-sample-accurate.vst3") else {
        return;
    };
    let mut inst = load_vst3(&path, sdk::AGAIN_SAMPLE_ACCURATE);
    let info = inst.info().clone();
    let gain = info.params.iter().find(|p| p.name == "Gain").unwrap().id;
    // Its processor starts at gain 0 until it is given a value (its
    // controller shows 1): the host sends what was set in the first block.
    inst.set_params(&[(gain, 1.0)]).unwrap();
    let mut proc = inst.activate(SR, 512).unwrap();
    let mut b = Block::new(&info, 512);
    b.fill(1.0, 0.0);
    b.quiet(proc.as_mut(), 128);
    assert!(b.outputs[0][..128].iter().all(|x| (x - 1.0).abs() < 1e-6));
    // A ramp from 1 (the last value) to 0.25 at frame 64, then held.
    let change = [ParamChange {
        frame: 64,
        id: gain,
        value: 0.25,
    }];
    b.run(proc.as_mut(), 128, &[], &change, &BlockInfo::default());
    let out = &b.outputs[0];
    assert!(
        out[1] < 1.0 && out[1] > 0.9,
        "ramping from frame 0: {}",
        out[1]
    );
    assert!(
        (out[32] - 0.625).abs() < 0.02,
        "half way at 32: {}",
        out[32]
    );
    assert!(
        out[64..128].iter().all(|x| (x - 0.25).abs() < 1e-6),
        "there at 64: {:?}",
        &out[60..68]
    );
    inst.deactivate(proc);
}

/// ADelay at `delay` seconds; what one second of constant 1.0 gives.
fn adelay_output(inst: &mut dyn PluginInstance) -> Vec<f32> {
    let info = inst.info().clone();
    let mut proc = inst.activate(SR, 512).unwrap();
    let mut b = Block::new(&info, 512);
    b.fill(1.0, 0.0);
    let mut out = Vec::new();
    for _ in 0..12 {
        b.quiet(proc.as_mut(), 512);
        out.extend_from_slice(&b.outputs[0][..512]);
    }
    inst.deactivate(proc);
    out
}

#[test]
fn sdk_adelay_delays_and_its_state_survives_a_saved_song() {
    let Some(path) = sample("adelay.vst3") else {
        return;
    };
    let delay = ParamId(100);
    // 0.0625 s is 3000 frames at 48 kHz (exact in binary).
    let mut inst = load_vst3(&path, sdk::ADELAY);
    inst.set_params(&[(delay, 0.0625)]).unwrap();
    let out = adelay_output(inst.as_mut());
    assert!(
        out[..3000].iter().all(|x| *x == 0.0),
        "silent while delayed"
    );
    assert!(out[3000..].iter().all(|x| *x == 1.0), "then the input");

    // Through the host: the overlay into a captured state, a saved song
    // and a new host.
    let (mut hist, song, slot) = with_insert(sdk::ADELAY, "ADelay");
    run(
        &mut hist,
        MixerCmd::SetPluginParams {
            slot,
            params: vec![(delay, 0.0625)],
        },
    );
    {
        let mut host = host_for(&path, &[sdk::ADELAY]);
        host.sync(hist.doc());
        let r = host.wait_ready(WAIT);
        assert_eq!(r.loaded, vec![slot], "{:?}", r.failed);
        let (mut engine, mut be) = engine(&host);
        engine.load(hist.doc(), song).unwrap();
        peak(&mut be, 512);
        host.capture(&mut hist, slot).unwrap();
        drop(engine);
    }
    let d = tempfile::tempdir().unwrap();
    let package = d.path().join("Delay.vizdaw");
    core_model::file::save(hist.doc(), &package).unwrap();
    let doc = core_model::file::load(&package).unwrap();
    assert!(doc.plugin_slot(slot).unwrap().params.is_empty());
    let mut fresh = host_for(&path, &[sdk::ADELAY]);
    fresh.sync(&doc);
    let r = fresh.wait_ready(WAIT);
    assert!(r.rejected_state.is_empty(), "{:?}", r.rejected_state);
    let inst = fresh.instance(slot).unwrap();
    assert_eq!(inst.param_value(delay), Some(0.0625), "the controller");
    // The component's delay, from the state the host applied.
    let mut copy = load_vst3(&path, sdk::ADELAY);
    let state = fresh.applied_state_bytes(slot).unwrap();
    copy.load_state(&state).unwrap();
    let out = adelay_output(copy.as_mut());
    assert!(out[..3000].iter().all(|x| *x == 0.0));
    assert!(out[3000..].iter().all(|x| *x == 1.0), "the component");
}

#[test]
fn sdk_note_expression_synth_sounds_while_a_note_is_held() {
    let Some(path) = sample("note-expression-synth.vst3") else {
        return;
    };
    let mut inst = load_vst3(&path, sdk::NOTE_EXPRESSION_SYNTH);
    let info = inst.info().clone();
    assert!(info.note_input);
    let mut proc = inst.activate(SR, 512).unwrap();
    let mut b = Block::new(&info, 512);
    b.quiet(proc.as_mut(), 512);
    assert!(b.outputs.iter().all(|c| c.iter().all(|x| *x == 0.0)));
    let on = [MidiEvent::new(100, &[0x90, 69, 100])];
    b.run(proc.as_mut(), 512, &on, &[], &BlockInfo::default());
    assert!(
        b.outputs[0][..100].iter().all(|x| *x == 0.0),
        "silent before"
    );
    assert!(b.outputs[0][100..].iter().any(|x| x.abs() > 0.01), "sounds");
    inst.deactivate(proc);
}

/// The host checker's problems: it reports what its processor saw wrong
/// in each block as bits of eight read-only output parameters
/// (`ProcessWarn1`..`ProcessWarn8`, 24 bits each), which the host hands
/// back to its controller. These are its error and warning log IDs from
/// the processor side (`hostchecker/source/logevents.h` of the pinned SDK;
/// the info IDs, such as "setActive called", are left out).
const HOST_CHECKER_PROBLEMS: [u32; 53] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 22, 23, 24, 25, 26, 27, 28, 29, 30,
    31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 90,
    91, 92, 93,
];

/// "IComponent::setActive (true) called": an info ID, which shows the
/// reports reach the controller at all.
const HOST_CHECKER_SET_ACTIVE: u32 = 94;

/// Every log ID the host checker's warn parameters hold now.
fn host_checker_logs(inst: &mut dyn PluginInstance) -> Vec<u32> {
    // Output parameter values reach the controller on the main thread.
    inst.gestures();
    inst.idle();
    let warn: Vec<ParamId> = inst
        .info()
        .params
        .iter()
        .filter(|p| p.name.starts_with("ProcessWarn"))
        .map(|p| p.id)
        .collect();
    assert_eq!(warn.len(), 8);
    let mut found = Vec::new();
    for (k, id) in warn.iter().enumerate() {
        let v = inst.param_value(*id).unwrap();
        let bits = (v * f64::from(1u32 << 24)).round() as u32;
        for bit in 0..24 {
            let log = k as u32 * 24 + bit;
            if bits & (1 << bit) != 0 {
                found.push(log);
            }
        }
    }
    found
}

#[test]
fn sdk_host_checker_finds_no_problem_with_the_host() {
    let Some(path) = sample("host-checker.vst3") else {
        return;
    };
    let mut inst = load_vst3(&path, sdk::HOST_CHECKER);
    let info = inst.info().clone();
    let mut logs = Vec::new();
    for round in 0..2 {
        if round == 1 {
            // A saved state loaded, offline, at another rate and block.
            let state = inst.save_state().unwrap();
            inst.load_state(&state).unwrap();
            assert_eq!(inst.set_offline(true), OfflineRender::Taken);
        }
        let mut proc = inst
            .activate(if round == 0 { SR } else { 44_100.0 }, 1024)
            .unwrap();
        let mut b = Block::new(&info, 1024);
        b.fill(0.25, 0.1);
        let mut t = BlockInfo {
            playing: true,
            ..BlockInfo::default()
        };
        for k in 0..16u32 {
            let frames = [256, 1024, 1, 300][k as usize % 4];
            let notes = [
                MidiEvent::new(0, &[0x90, 60, 100]),
                MidiEvent::new(frames - 1, &[0x80, 60, 0]),
            ];
            let events: &[MidiEvent] = if k % 2 == 0 { &notes } else { &[] };
            let params = [ParamChange {
                frame: frames / 2,
                id: ParamId(1003),
                value: 0.0,
            }];
            let status = b.run(proc.as_mut(), frames, events, &params, &t);
            assert_eq!(status, ProcessStatus::Ok);
            t.frame += u64::from(frames);
            t.beats += f64::from(frames) / SR * 2.0;
            logs.extend(host_checker_logs(inst.as_mut()));
        }
        inst.deactivate(proc);
        logs.extend(host_checker_logs(inst.as_mut()));
    }
    logs.sort_unstable();
    logs.dedup();
    assert!(
        logs.contains(&HOST_CHECKER_SET_ACTIVE),
        "no reports: {logs:?}"
    );
    let problems: Vec<u32> = logs
        .into_iter()
        .filter(|l| HOST_CHECKER_PROBLEMS.contains(l))
        .collect();
    assert!(
        problems.is_empty(),
        "the host checker logged problems {problems:?} (log IDs in hostchecker/source/logevents.h)"
    );
}

#[test]
fn sdk_host_checker_latency_change_restarts_it_through_the_host() {
    let Some(path) = sample("host-checker.vst3") else {
        return;
    };
    let (hist, song, slot) = with_insert(sdk::HOST_CHECKER, "Host Checker");
    let mut host = host_for(&path, &[sdk::HOST_CHECKER]);
    host.sync(hist.doc());
    let r = host.wait_ready(WAIT);
    assert_eq!(r.loaded, vec![slot], "{:?}", r.failed);
    let (mut engine, mut be) = engine(&host);
    engine.load(hist.doc(), song).unwrap();
    assert_eq!(engine.latency_frames(), 0);
    // Its processor takes Latency (plain = normalised x 10 s) and tells
    // its controller, which asks for a restart; the new latency is read at
    // the next activation. 0.078125 s is 3750 frames.
    host.set_param(slot, ParamId(1002), 0.078125);
    let mut latency = 0;
    for _ in 0..500 {
        peak(&mut be, 256);
        host.service(&mut engine, hist.doc(), song).unwrap();
        latency = engine.latency_frames();
        if latency == 3750 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(latency, 3750);
    drop(engine);
}

#[test]
fn sdk_panner_moves_a_mono_input_between_the_sides() {
    let Some(path) = sample("panner.vst3") else {
        return;
    };
    let mut inst = load_vst3(&path, sdk::PANNER);
    let info = inst.info().clone();
    assert_eq!(info.inputs[0].channels, 1);
    assert_eq!(info.outputs[0].channels, 2);
    let pan = ParamId(102);
    let mut level = |v: f64| {
        inst.set_params(&[(pan, v)]).unwrap();
        let mut proc = inst.activate(SR, 512).unwrap();
        let mut b = Block::new(&info, 512);
        b.fill(0.5, 0.0);
        b.quiet(proc.as_mut(), 512);
        let (l, r) = (b.outputs[0][511], b.outputs[1][511]);
        inst.deactivate(proc);
        (l, r)
    };
    let (l0, r0) = level(0.0);
    let (l1, r1) = level(1.0);
    assert!(l0 > r0, "left: {l0} {r0}");
    assert!(r1 > l1, "right: {l1} {r1}");
}
