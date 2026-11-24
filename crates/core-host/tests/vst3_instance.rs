//! VST3 instances (host track H1 Task 3): component and controller, their
//! connection, the component handler, parameters and the release order
//! (Review Focus 1, 2 and 5). The test module counts its own lifecycle
//! (`test_plugins::Vst3Probe`); each test loads a private copy of the
//! bundle, which is a separate loaded image with its own counters.

use core_host::vst3::{Vst3Format, Vst3Instance};
use core_host::{
    HostError, InstanceEvents, LoadRequest, OfflineRender, ParamEvent, PluginFormat,
    PluginInstance, PluginKind, PortRole,
};
use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef};
use test_plugins::{
    VST3_COMPONENT_GAIN, VST3_GAIN_ID, VST3_PARAM_GAIN, VST3_PARAM_METER, VST3_PARAM_MODE,
    VST3_PARAM_SECRET, VST3_PARAMS, VST3_SYNTH_ID, Vst3Counters, Vst3Edit, Vst3Mode, Vst3Probe,
    vst3_restart,
};

/// A private copy of the test bundle, and a probe on its image.
fn private() -> (tempfile::TempDir, std::path::PathBuf, Vst3Probe) {
    let d = tempfile::tempdir().unwrap();
    let copy = d.path().join("Private.vst3");
    test_plugins::copy_vst3_bundle(&copy);
    let probe = Vst3Probe::open(&copy);
    (d, copy, probe)
}

fn pid(id: u32) -> ParamId {
    ParamId(u64::from(id))
}

fn plain(id: u32, n: f64) -> f64 {
    VST3_PARAMS.iter().find(|p| p.id == id).unwrap().to_plain(n)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// Everything the instance made is gone, and the module exited after it.
fn all_released(c: &Vst3Counters) {
    assert_eq!((c.entries, c.exits), (1, 1), "{c:?}");
    assert_eq!(c.factory_releases, 1, "{c:?}");
    assert!(c.last_factory_release < c.last_exit, "{c:?}");
    assert!(c.last_component_release < c.last_exit, "{c:?}");
    assert!(c.last_controller_release < c.last_exit, "{c:?}");
}

#[test]
fn a_split_plug_in_reads_its_buses_and_parameters() {
    let (_d, copy, _probe) = private();
    let inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    let info = inst.info();
    assert_eq!(info.plugin.format, Format::Vst3);
    assert_eq!(info.plugin.id, VST3_GAIN_ID);
    assert_eq!(info.plugin.name, "Test Gain");
    assert_eq!(info.plugin.vendor, "VizDAW");
    assert_eq!(info.plugin.version, "1.0.0");
    assert_eq!(info.kind, PluginKind::Effect);
    assert!(!info.note_input);
    let ins: Vec<_> = info
        .inputs
        .iter()
        .map(|p| (p.name.as_str(), p.channels, p.role))
        .collect();
    assert_eq!(
        ins,
        [
            ("Input", 2, PortRole::Main),
            ("Sidechain", 2, PortRole::Sidechain)
        ]
    );
    assert_eq!(info.outputs.len(), 1);
    assert_eq!(info.outputs[0].role, PortRole::Main);
    assert_eq!(info.sidechain_input(), Some(1));

    let ids: Vec<ParamId> = info.params.iter().map(|p| p.id).collect();
    assert_eq!(
        ids,
        [
            pid(VST3_PARAM_GAIN),
            pid(VST3_PARAM_MODE),
            pid(VST3_PARAM_METER),
            pid(VST3_PARAM_SECRET)
        ],
        "ParamIDs, not indices"
    );
    let gain = info.param(pid(VST3_PARAM_GAIN)).unwrap();
    assert_eq!(gain.name, "Gain");
    assert_eq!((gain.min, gain.max), (-60.0, 12.0), "plain range");
    assert!(close(gain.default, 0.0), "{}", gain.default);
    assert!(!gain.stepped && gain.automatable && !gain.hidden && !gain.read_only);
    let mode = info.param(pid(VST3_PARAM_MODE)).unwrap();
    assert_eq!((mode.min, mode.max, mode.default), (0.0, 2.0, 0.0));
    assert!(mode.stepped && mode.automatable);
    let meter = info.param(pid(VST3_PARAM_METER)).unwrap();
    assert!(meter.read_only && !meter.automatable && !meter.hidden);
    let secret = info.param(pid(VST3_PARAM_SECRET)).unwrap();
    assert!(secret.hidden && !secret.automatable && !secret.read_only);
}

#[test]
fn a_split_plug_in_is_initialised_connected_and_synchronised() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    let c = probe.counters();
    assert_eq!(
        c.host_contexts, 1,
        "the factory got the host context: {c:?}"
    );
    assert_eq!((c.component_inits, c.controller_inits), (1, 1), "{c:?}");
    assert_eq!(c.connects, 2, "both sides connected: {c:?}");
    assert_eq!(
        c.messages, 1,
        "the component's message, made by the host, reached the controller: {c:?}"
    );
    assert_eq!(c.handler_sets, 1, "{c:?}");
    assert_eq!(c.state_syncs, 1, "setComponentState: {c:?}");
    // The controller took the component's state.
    let g = inst.param_value(pid(VST3_PARAM_GAIN)).unwrap();
    assert!(close(g, plain(VST3_PARAM_GAIN, VST3_COMPONENT_GAIN)), "{g}");
    assert_eq!(probe.counters().off_thread_calls, 0);
}

#[test]
fn a_split_plug_in_is_released_in_order_and_the_module_last() {
    let (_d, copy, probe) = private();
    let inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    assert_eq!(probe.counters().exits, 0, "the instance keeps the module");
    drop(inst);
    let c = probe.counters();
    all_released(&c);
    assert_eq!((c.component_terms, c.controller_terms), (1, 1), "{c:?}");
    assert_eq!(c.disconnects, 2, "{c:?}");
    assert_eq!(
        (c.component_releases, c.controller_releases),
        (1, 1),
        "{c:?}"
    );
    // Disconnect, controller terminate, component terminate, release.
    assert!(c.last_disconnect < c.last_controller_term, "{c:?}");
    assert!(c.last_controller_term < c.last_component_term, "{c:?}");
    assert!(c.last_component_term < c.last_controller_release, "{c:?}");
    assert!(c.last_component_term < c.last_component_release, "{c:?}");
}

#[test]
fn a_single_component_plug_in_is_its_own_controller() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_SYNTH_ID).unwrap();
    let info = inst.info().clone();
    assert_eq!(info.kind, PluginKind::Instrument);
    assert!(info.note_input);
    assert!(info.inputs.is_empty());
    let outs: Vec<_> = info.outputs.iter().map(|p| (p.channels, p.role)).collect();
    assert_eq!(outs, [(2, PortRole::Main), (2, PortRole::Aux)]);
    assert_eq!(info.params.len(), 4);
    let c = probe.counters();
    assert_eq!(c.component_inits, 1, "initialised once: {c:?}");
    assert_eq!(c.controller_inits, 0, "no separate controller: {c:?}");
    assert_eq!(c.connects, 0, "nothing to connect: {c:?}");
    assert_eq!(c.handler_sets, 1, "{c:?}");
    inst.set_params(&[(pid(VST3_PARAM_GAIN), -6.0)]).unwrap();
    assert!(close(inst.param_value(pid(VST3_PARAM_GAIN)).unwrap(), -6.0));
    drop(inst);
    let c = probe.counters();
    assert_eq!(c.component_terms, 1, "terminated once: {c:?}");
    assert_eq!(c.component_releases, 1, "{c:?}");
    assert_eq!((c.entries, c.exits), (1, 1), "{c:?}");
    assert!(c.last_component_release < c.last_exit, "{c:?}");
}

#[test]
fn parameters_convert_between_plain_and_normalised_values() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    inst.set_params(&[
        (pid(VST3_PARAM_GAIN), -6.0),
        (pid(VST3_PARAM_MODE), 2.0),
        (pid(VST3_PARAM_SECRET), 0.75),
    ])
    .unwrap();
    assert!(close(inst.param_value(pid(VST3_PARAM_GAIN)).unwrap(), -6.0));
    assert_eq!(inst.param_value(pid(VST3_PARAM_MODE)), Some(2.0));
    assert!(close(
        inst.param_value(pid(VST3_PARAM_SECRET)).unwrap(),
        0.75
    ));
    // A step value lands on a step.
    inst.set_params(&[(pid(VST3_PARAM_MODE), 1.2)]).unwrap();
    assert_eq!(inst.param_value(pid(VST3_PARAM_MODE)), Some(1.0));
    // Read-only and unknown parameters are left alone.
    inst.set_params(&[(pid(VST3_PARAM_METER), 1.0), (pid(999), 1.0)])
        .unwrap();
    assert_eq!(inst.param_value(pid(VST3_PARAM_METER)), Some(0.0));
    assert_eq!(inst.param_value(pid(999)), None);
    assert_eq!(inst.param_value(ParamId(u64::MAX)), None);
    assert_eq!(probe.counters().off_thread_calls, 0);
}

#[test]
fn edits_from_the_plug_in_reach_the_host_as_plain_gestures() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    assert!(inst.gestures().is_empty());
    assert_eq!(probe.edit(Vst3Edit::Begin(VST3_PARAM_GAIN)), 0);
    assert_eq!(probe.edit(Vst3Edit::Perform(VST3_PARAM_GAIN, 0.5)), 0);
    assert_eq!(probe.edit(Vst3Edit::End(VST3_PARAM_GAIN)), 0);
    let g = inst.gestures();
    assert_eq!(g.len(), 3, "{g:?}");
    assert_eq!(g[0], ParamEvent::Begin(pid(VST3_PARAM_GAIN)));
    let ParamEvent::Value(id, v) = g[1] else {
        panic!("{g:?}");
    };
    assert_eq!(id, pid(VST3_PARAM_GAIN));
    assert!(close(v, plain(VST3_PARAM_GAIN, 0.5)), "plain, not 0.5: {v}");
    assert_eq!(g[2], ParamEvent::End(pid(VST3_PARAM_GAIN)));
    assert!(inst.gestures().is_empty(), "drained");
}

#[test]
fn handler_calls_from_another_thread_are_queued_for_the_main_thread() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    assert_eq!(probe.edit_off_thread(Vst3Edit::Begin(VST3_PARAM_MODE)), 0);
    assert_eq!(
        probe.edit_off_thread(Vst3Edit::Perform(VST3_PARAM_MODE, 1.0)),
        0
    );
    assert_eq!(probe.edit_off_thread(Vst3Edit::End(VST3_PARAM_MODE)), 0);
    assert_eq!(
        probe.edit_off_thread(Vst3Edit::Restart(vst3_restart::LATENCY_CHANGED)),
        0
    );
    // Nothing ran on that thread: the controller was not called there (the
    // conversion to a plain value waits for the main thread).
    assert_eq!(probe.counters().off_thread_calls, 0);
    let g = inst.gestures();
    assert_eq!(
        g,
        [
            ParamEvent::Begin(pid(VST3_PARAM_MODE)),
            ParamEvent::Value(pid(VST3_PARAM_MODE), 2.0),
            ParamEvent::End(pid(VST3_PARAM_MODE)),
        ]
    );
    assert!(inst.idle().latency_changed);
    assert_eq!(probe.counters().off_thread_calls, 0);
}

#[test]
fn restart_flags_and_dirty_reach_idle() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    assert_eq!(inst.idle(), InstanceEvents::default());
    probe.edit(Vst3Edit::Restart(vst3_restart::PARAM_VALUES_CHANGED));
    let ev = inst.idle();
    assert!(
        ev.params_changed && !ev.restart && !ev.latency_changed,
        "{ev:?}"
    );
    assert_eq!(inst.idle(), InstanceEvents::default(), "taken once");
    probe.edit(Vst3Edit::Restart(vst3_restart::RELOAD_COMPONENT));
    assert!(inst.idle().restart);
    probe.edit(Vst3Edit::Restart(vst3_restart::IO_CHANGED));
    let ev = inst.idle();
    assert!(ev.restart && ev.params_changed, "{ev:?}");
    probe.edit(Vst3Edit::Restart(vst3_restart::LATENCY_CHANGED));
    let ev = inst.idle();
    assert!(ev.latency_changed && !ev.params_changed, "{ev:?}");
    probe.edit(Vst3Edit::Restart(vst3_restart::PARAM_TITLES_CHANGED));
    assert!(inst.idle().params_changed);
    // Two requests before one idle: both count.
    probe.edit_off_thread(Vst3Edit::Restart(vst3_restart::LATENCY_CHANGED));
    probe.edit(Vst3Edit::Restart(vst3_restart::PARAM_VALUES_CHANGED));
    let ev = inst.idle();
    assert!(ev.latency_changed && ev.params_changed, "{ev:?}");
    assert_eq!(probe.edit(Vst3Edit::Dirty(true)), 0);
    let ev = inst.idle();
    assert!(ev.state_dirty && !ev.restart, "{ev:?}");
    probe.edit(Vst3Edit::Dirty(false));
    assert!(!inst.idle().state_dirty);
}

#[test]
fn a_component_that_fails_to_initialise_leaves_nothing_loaded() {
    let (_d, copy, probe) = private();
    probe.set_mode(Vst3Mode::ComponentInitFails);
    let e = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap_err();
    assert!(matches!(e, HostError::LoadFailed(_)), "{e}");
    let c = probe.counters();
    assert_eq!((c.entries, c.exits), (1, 1), "{c:?}");
    assert_eq!(c.component_inits, 1, "{c:?}");
    assert_eq!(c.component_terms, 0, "never initialised: {c:?}");
    assert_eq!(c.component_releases, 1, "{c:?}");
    assert_eq!(c.controller_releases, 0, "no controller made: {c:?}");
    assert!(c.last_component_release < c.last_exit, "{c:?}");
}

#[test]
fn a_controller_that_fails_to_initialise_leaves_nothing_loaded() {
    let (_d, copy, probe) = private();
    probe.set_mode(Vst3Mode::ControllerInitFails);
    let e = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap_err();
    assert!(matches!(e, HostError::LoadFailed(_)), "{e}");
    assert!(e.to_string().contains("controller"), "{e}");
    let c = probe.counters();
    all_released(&c);
    assert_eq!(c.component_terms, 1, "the component was undone: {c:?}");
    assert_eq!(c.controller_terms, 0, "never initialised: {c:?}");
    assert_eq!(
        (c.component_releases, c.controller_releases),
        (1, 1),
        "{c:?}"
    );
    assert_eq!(c.connects, 0, "{c:?}");
}

#[test]
fn an_unknown_class_is_not_found_and_leaves_nothing_loaded() {
    let (_d, copy, probe) = private();
    let e = Vst3Instance::load(&copy, test_plugins::VST3_CONTROLLER_ID).unwrap_err();
    assert!(matches!(e, HostError::NotFound(_)), "{e}");
    let e = Vst3Instance::load(&copy, "00000000000000000000000000000001").unwrap_err();
    assert!(matches!(e, HostError::NotFound(_)), "{e}");
    let c = probe.counters();
    assert_eq!(c.entries, c.exits, "{c:?}");
    assert_eq!(c.component_releases, 0, "{c:?}");
}

#[test]
fn offline_rendering_goes_through_setup_processing() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    assert_eq!(inst.set_offline(true), OfflineRender::Taken);
    let c = probe.counters();
    assert_eq!((c.setups, c.last_process_mode), (1, 2), "kOffline: {c:?}");
    assert_eq!(inst.set_offline(false), OfflineRender::Taken);
    assert_eq!(probe.counters().last_process_mode, 0, "kRealtime");
    probe.set_mode(Vst3Mode::OfflineRefused);
    assert_eq!(inst.set_offline(true), OfflineRender::Refused);
}

#[test]
fn the_format_shares_one_module_between_instances() {
    let (_d, copy, probe) = private();
    let mut f = Vst3Format::new();
    assert_eq!(f.format(), Format::Vst3);
    let req = |id: &str| LoadRequest {
        plugin: PluginRef {
            format: Format::Vst3,
            id: id.into(),
            name: id.into(),
            vendor: String::new(),
            version: String::new(),
        },
        path: Some(copy.clone()),
        sample_rate: 48_000.0,
        max_frames: 512,
    };
    let load = |f: &mut Vst3Format, id: &str| {
        let r = req(id);
        let prepared = (f.prepare(&r))().unwrap();
        f.finish(&r, prepared).unwrap()
    };
    let a = load(&mut f, VST3_GAIN_ID);
    let b = load(&mut f, VST3_SYNTH_ID);
    assert_eq!(a.info().plugin.id, VST3_GAIN_ID);
    assert_eq!(b.info().plugin.id, VST3_SYNTH_ID);
    assert_eq!(probe.counters().entries, 1, "one module for both");
    drop(a);
    assert_eq!(probe.counters().exits, 0, "still in use");
    drop(b);
    let c = probe.counters();
    assert_eq!((c.entries, c.exits), (1, 1), "{c:?}");
    // Loaded again once nothing used it.
    let a = load(&mut f, VST3_GAIN_ID);
    assert_eq!(probe.counters().entries, 2);
    drop(a);
    drop(f);
    assert_eq!(probe.counters().exits, 2);
    // A request without a path is refused.
    let mut r = req(VST3_GAIN_ID);
    r.path = None;
    assert!(matches!(
        (f_new().prepare(&r))(),
        Err(HostError::NotFound(_))
    ));
}

fn f_new() -> Vst3Format {
    Vst3Format::new()
}
