//! The CLAP host against the in-repo test plugins: ports, parameters,
//! notes, multi-out, sidechain and errors.

use core_engine::backend::BlockInfo;
use core_engine::graph::ProcessStatus;
use core_host::clap::{ClapFormat, list_bundle};
use core_host::{
    AudioProcessor, LoadRequest, MidiEvent, ParamChange, ParamOut, PluginFormat, PluginInstance,
    PluginKind, PortRole, ProcessCtx,
};
use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef};

const N: usize = 64;

pub fn load(id: &str) -> Box<dyn PluginInstance> {
    let mut f = ClapFormat::new();
    let req = LoadRequest {
        plugin: PluginRef {
            format: Format::Clap,
            id: id.into(),
            name: id.into(),
            vendor: String::new(),
            version: String::new(),
        },
        path: Some(test_plugins::bundle()),
        sample_rate: 48_000.0,
        max_frames: 512,
    };
    let prepared = (f.prepare(&req))().unwrap();
    f.finish(&req, prepared).unwrap()
}

fn bufs(channels: usize, v: f32) -> Vec<Box<[f32]>> {
    (0..channels)
        .map(|_| vec![v; 512].into_boxed_slice())
        .collect()
}

/// One block of `N` frames.
struct Block {
    input: Vec<Box<[f32]>>,
    sidechain: Vec<Box<[f32]>>,
    outputs: Vec<Box<[f32]>>,
    events: Vec<MidiEvent>,
    params: Vec<ParamChange>,
    out: ParamOut,
}

impl Block {
    fn new(inputs: usize, outputs: usize, level: f32) -> Block {
        Block {
            input: bufs(inputs, level),
            sidechain: Vec::new(),
            outputs: bufs(outputs, 9.0),
            events: Vec::new(),
            params: Vec::new(),
            out: ParamOut::default(),
        }
    }

    fn run(&mut self, p: &mut dyn AudioProcessor) -> ProcessStatus {
        let info = BlockInfo::default();
        let mut ctx = ProcessCtx {
            frames: N as u32,
            input: &mut self.input,
            sidechain: &mut self.sidechain,
            outputs: &mut self.outputs,
            events: &self.events,
            params_in: &self.params,
            params_out: &mut self.out,
            transport: &info,
        };
        p.process(&mut ctx)
    }
}

#[test]
fn the_bundle_lists_kinds() {
    let listed = list_bundle(&test_plugins::bundle()).unwrap();
    let synth = listed
        .iter()
        .find(|(p, _)| p.id == "vizdaw.test.synth4")
        .unwrap();
    assert_eq!(synth.1, PluginKind::Instrument);
    assert_eq!(synth.0.vendor, "VizDAW");
    assert_eq!(synth.0.version, "1.0.0");
    let gain = listed
        .iter()
        .find(|(p, _)| p.id == "vizdaw.test.gain")
        .unwrap();
    assert_eq!(gain.1, PluginKind::Effect);
}

#[test]
fn gain_reports_its_ports_and_parameters_and_processes() {
    let mut inst = load("vizdaw.test.gain");
    let info = inst.info().clone();
    assert_eq!(info.kind, PluginKind::Effect);
    assert_eq!(info.inputs.len(), 1);
    assert_eq!(info.inputs[0].role, PortRole::Main);
    assert_eq!(info.outputs[0].channels, 2);
    let names: Vec<&str> = info.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Gain", "Latency", "Wiggle"]);
    assert!(info.params[1].stepped);
    assert_eq!(inst.param_value(ParamId(0)), Some(1.0));
    let mut p = inst.activate(48_000.0, 512).unwrap();
    assert!(inst.is_active());
    let mut b = Block::new(2, 2, 0.5);
    assert_eq!(b.run(p.as_mut()), ProcessStatus::Ok);
    assert!(b.outputs.iter().all(|c| c[..N].iter().all(|x| *x == 0.5)));
    // A parameter change in the block applies to it.
    b.params.push(ParamChange {
        frame: 0,
        id: ParamId(0),
        value: 0.25,
    });
    b.run(p.as_mut());
    assert!(b.outputs[1][..N].iter().all(|x| *x == 0.125));
    inst.deactivate(p);
    assert!(!inst.is_active());
    assert_eq!(inst.param_value(ParamId(0)), Some(0.25));
}

#[test]
fn the_synth_renders_each_channel_on_its_own_output() {
    let mut inst = load("vizdaw.test.synth4");
    let info = inst.info().clone();
    assert_eq!(info.kind, PluginKind::Instrument);
    assert!(info.note_input && info.inputs.is_empty());
    assert_eq!(info.outputs.len(), 4);
    assert_eq!(info.output_channels(), 8);
    let mut p = inst.activate(48_000.0, 512).unwrap();
    let mut b = Block::new(0, 8, 0.0);
    b.events.push(MidiEvent::new(10, &[0x92, 69, 100]));
    b.run(p.as_mut());
    // Output 2 (channels 4, 5) sounds from frame 10; the others are silent.
    for (ch, buf) in b.outputs.iter().enumerate() {
        let loud = buf[..N].iter().any(|x| *x != 0.0);
        assert_eq!(loud, ch == 4 || ch == 5, "channel {ch}");
    }
    assert!(b.outputs[4][..10].iter().all(|x| *x == 0.0));
    assert!(b.outputs[4][11] != 0.0);
    inst.deactivate(p);
}

#[test]
fn the_ducker_follows_its_sidechain() {
    let mut inst = load("vizdaw.test.ducker");
    assert_eq!(inst.info().sidechain_input(), Some(1));
    let mut p = inst.activate(48_000.0, 512).unwrap();
    let mut b = Block::new(2, 2, 0.8);
    b.sidechain = bufs(2, 0.5);
    b.run(p.as_mut());
    assert!(b.outputs[0][..N].iter().all(|x| (*x - 0.4).abs() < 1e-6));
    // Nothing routed: the sidechain port gets silence.
    b.sidechain.clear();
    b.run(p.as_mut());
    assert!(b.outputs[0][..N].iter().all(|x| (*x - 0.8).abs() < 1e-6));
    inst.deactivate(p);
}

#[test]
fn the_nan_plugin_writes_nan() {
    let mut inst = load("vizdaw.test.nan");
    let mut p = inst.activate(48_000.0, 512).unwrap();
    let mut b = Block::new(2, 2, 0.1);
    assert_eq!(b.run(p.as_mut()), ProcessStatus::Ok);
    assert!(b.outputs[0][0].is_nan());
    inst.deactivate(p);
}

#[test]
fn unknown_ids_and_missing_bundles_are_errors() {
    let mut f = ClapFormat::new();
    let mut req = LoadRequest {
        plugin: PluginRef::from_legacy("vizdaw.test.nope"),
        path: Some(test_plugins::bundle()),
        sample_rate: 48_000.0,
        max_frames: 512,
    };
    let prepared = (f.prepare(&req))().unwrap();
    let Err(err) = f.finish(&req, prepared) else {
        panic!("an unknown id loaded");
    };
    assert!(err.to_string().contains("not found"), "{err}");
    req.path = Some("/nonexistent/X.clap".into());
    assert!((f.prepare(&req))().is_err());
    req.path = None;
    assert!((f.prepare(&req))().is_err());
}

#[test]
fn activating_twice_is_refused() {
    let mut inst = load("vizdaw.test.gain");
    let p = inst.activate(48_000.0, 512).unwrap();
    assert!(inst.activate(48_000.0, 512).is_err());
    inst.deactivate(p);
    let p = inst.activate(44_100.0, 256).unwrap();
    inst.deactivate(p);
}

#[test]
fn state_round_trips_and_foreign_state_is_rejected() {
    let mut a = load("vizdaw.test.gain");
    a.set_params(&[(ParamId(0), 0.3), (ParamId(1), 64.0)])
        .unwrap();
    let state = a.save_state().unwrap();
    let mut b = load("vizdaw.test.gain");
    b.load_state(&state).unwrap();
    assert_eq!(b.param_value(ParamId(0)), Some(0.3));
    let p = b.activate(48_000.0, 512).unwrap();
    assert_eq!(b.info().latency, 64, "latency is read at activation");
    b.deactivate(p);
    let err = b.load_state(b"junk").unwrap_err();
    assert!(
        matches!(err, core_host::HostError::StateRejected(_)),
        "{err}"
    );
    assert!(
        load("vizdaw.test.nan").save_state().is_err(),
        "no state extension"
    );
}

#[test]
fn ui_gestures_come_out_of_process() {
    let mut inst = load("vizdaw.test.gain");
    let mut p = inst.activate(48_000.0, 512).unwrap();
    let mut b = Block::new(2, 2, 1.0);
    // "Wiggle" makes the plugin act as if its Gain knob were dragged.
    b.params.push(ParamChange {
        frame: 0,
        id: ParamId(2),
        value: 0.7,
    });
    b.run(p.as_mut());
    use core_host::ParamEvent::{Begin, End, Value};
    assert_eq!(
        b.out.as_slice(),
        &[Begin(ParamId(0)), Value(ParamId(0), 0.7), End(ParamId(0))]
    );
    inst.deactivate(p);
}

#[test]
fn a_latency_change_asks_for_a_restart() {
    let mut inst = load("vizdaw.test.gain");
    let mut p = inst.activate(48_000.0, 512).unwrap();
    assert_eq!(inst.info().latency, 0);
    assert!(!inst.idle().restart);
    let mut b = Block::new(2, 2, 1.0);
    b.params.push(ParamChange {
        frame: 0,
        id: ParamId(1),
        value: 128.0,
    });
    b.run(p.as_mut());
    assert!(inst.idle().restart);
    assert!(!inst.idle().restart, "read once");
    inst.deactivate(p);
    let p = inst.activate(48_000.0, 512).unwrap();
    assert_eq!(inst.info().latency, 128);
    inst.deactivate(p);
}

#[test]
fn plugins_without_a_gui_have_no_editor() {
    let mut inst = load("vizdaw.test.gain");
    assert!(inst.editor().is_none());
}
