//! The AU host against Apple's built-in units (never played on a device):
//! enumeration, ports, parameters, processing, sidechain, instruments,
//! state and UI gestures.

use core_engine::backend::BlockInfo;
use core_engine::graph::ProcessStatus;
use core_host::{
    AudioProcessor, MidiEvent, ParamChange, ParamEvent, ParamOut, PluginInstance, PluginKind,
    PortRole, ProcessCtx,
};
use core_model::mixer::ParamId;
use platform_macos::au::{self, AuInstance};
use std::time::{Duration, Instant};

const DELAY: &str = "aufx/dely/appl";
const DYNAMICS: &str = "aufx/dcmp/appl";
const REVERB: &str = "aufx/mrev/appl";
const DLS: &str = "aumu/dls /appl";
const N: usize = 512;

fn load(id: &str) -> AuInstance {
    au::load(id, 48_000.0, N as u32).unwrap()
}

fn bufs(n: usize, v: f32) -> Vec<Box<[f32]>> {
    (0..n).map(|_| vec![v; N].into_boxed_slice()).collect()
}

struct Block {
    input: Vec<Box<[f32]>>,
    sidechain: Vec<Box<[f32]>>,
    outputs: Vec<Box<[f32]>>,
    events: Vec<MidiEvent>,
    params: Vec<ParamChange>,
}

impl Block {
    fn new(inputs: usize, outputs: usize, level: f32) -> Block {
        Block {
            input: bufs(inputs, level),
            sidechain: Vec::new(),
            outputs: bufs(outputs, 9.0),
            events: Vec::new(),
            params: Vec::new(),
        }
    }

    fn run(&mut self, p: &mut dyn AudioProcessor) -> ProcessStatus {
        let info = BlockInfo::default();
        let mut out = ParamOut::default();
        let mut ctx = ProcessCtx {
            frames: N as u32,
            input: &mut self.input,
            sidechain: &mut self.sidechain,
            outputs: &mut self.outputs,
            events: &self.events,
            params_in: &self.params,
            params_out: &mut out,
            transport: &info,
        };
        p.process(&mut ctx)
    }
}

#[test]
fn enumeration_finds_apples_units() {
    let all = au::components();
    let delay = all.iter().find(|c| c.plugin.id == DELAY).unwrap();
    assert_eq!(delay.kind, PluginKind::Effect);
    assert_eq!(delay.plugin.vendor, "Apple");
    assert_eq!(delay.plugin.name, "AUDelay");
    let dls = all.iter().find(|c| c.plugin.id == DLS).unwrap();
    assert_eq!(dls.kind, PluginKind::Instrument);
    assert_eq!(
        au::component(DYNAMICS).unwrap().plugin.name,
        "AUDynamicsProcessor"
    );
    assert!(au::component("aufx/none/appl").is_none());
    assert_eq!(
        au::parse_au_id(DELAY),
        Some((0x61756678, 0x64656c79, 0x6170706c))
    );
    assert!(
        au::sources()
            .iter()
            .any(|s| s.path.to_string_lossy() == format!("au:{DELAY}"))
    );
}

#[test]
fn the_delay_reports_parameters_and_processes() {
    let mut inst = load(DELAY);
    let info = inst.info().clone();
    assert_eq!(info.inputs[0].role, PortRole::Main);
    assert_eq!(info.outputs[0].channels, 2);
    let names: Vec<&str> = info.params.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"Dry/Wet Mix"), "{names:?}");
    // All dry: the input passes unchanged.
    inst.set_params(&[(ParamId(0), 0.0)]).unwrap();
    let mut p = inst.activate(48_000.0, N as u32).unwrap();
    let mut b = Block::new(2, 2, 0.5);
    assert_eq!(b.run(p.as_mut()), ProcessStatus::Ok);
    assert!(
        b.outputs[0].iter().all(|x| (*x - 0.5).abs() < 1e-4),
        "{:?}",
        &b.outputs[0][..4]
    );
    // All wet with a 5 ms delay: silence, then the input 240 frames later.
    b.params = vec![
        ParamChange {
            frame: 0,
            id: ParamId(0),
            value: 100.0,
        },
        ParamChange {
            frame: 0,
            id: ParamId(1),
            value: 0.005,
        },
    ];
    p.reset();
    b.run(p.as_mut());
    b.params.clear();
    b.run(p.as_mut());
    assert!(b.outputs[1].iter().any(|x| x.abs() > 0.1));
    inst.deactivate(p);
}

#[test]
fn the_dynamics_processor_processes_and_apples_units_offer_no_sidechain() {
    // None of Apple's built-in effects takes a second input bus on current
    // macOS (Plan Ruling 8), so the AU sidechain path is checked by hand
    // with a third-party unit; here the main path and the port report are.
    let mut inst = load(DYNAMICS);
    assert_eq!(inst.info().sidechain_input(), None);
    let mut p = inst.activate(48_000.0, N as u32).unwrap();
    let mut b = Block::new(2, 2, 0.25);
    for _ in 0..20 {
        assert_eq!(b.run(p.as_mut()), ProcessStatus::Ok);
    }
    assert!(b.outputs[0].iter().all(|x| x.is_finite() && x.abs() > 0.0));
    inst.deactivate(p);
}

#[test]
fn the_matrix_reverb_rings() {
    let mut inst = load(REVERB);
    let mut p = inst.activate(48_000.0, N as u32).unwrap();
    let mut b = Block::new(2, 2, 0.3);
    for _ in 0..10 {
        assert_eq!(b.run(p.as_mut()), ProcessStatus::Ok);
    }
    b.input = bufs(2, 0.0);
    let mut tail = false;
    for _ in 0..10 {
        b.run(p.as_mut());
        tail |= b.outputs[0].iter().any(|x| x.abs() > 1e-6);
    }
    assert!(tail, "it rings after the input stops");
    inst.deactivate(p);
}

#[test]
fn the_dls_synth_plays_a_note() {
    let mut inst = load(DLS);
    let info = inst.info().clone();
    assert_eq!(info.kind, PluginKind::Instrument);
    assert!(info.note_input && info.inputs.is_empty());
    let mut p = inst.activate(48_000.0, N as u32).unwrap();
    let mut b = Block::new(0, info.output_channels(), 0.0);
    b.events.push(MidiEvent::new(0, &[0x90, 60, 110]));
    let mut loud = false;
    for _ in 0..20 {
        b.run(p.as_mut());
        b.events.clear();
        loud |= b.outputs[0].iter().any(|x| x.abs() > 1e-4);
    }
    assert!(loud);
    inst.deactivate(p);
}

#[test]
fn state_round_trips() {
    let mut a = load(DELAY);
    a.set_params(&[(ParamId(1), 0.123)]).unwrap();
    let state = a.save_state().unwrap();
    assert!(state.starts_with(b"bplist00"), "a binary property list");
    let mut b = load(DELAY);
    assert!(
        (b.param_value(ParamId(1)).unwrap() - 1.0).abs() < 1e-6,
        "the default"
    );
    b.load_state(&state).unwrap();
    assert!((b.param_value(ParamId(1)).unwrap() - 0.123).abs() < 1e-6);
    assert!(b.load_state(b"junk").is_err());
}

#[test]
fn ui_gestures_are_captured() {
    let mut inst = load(DELAY);
    inst.simulate_ui_gesture(ParamId(0), 25.0);
    let start = Instant::now();
    let mut got = Vec::new();
    while got.len() < 3 && start.elapsed() < Duration::from_secs(2) {
        got.extend(inst.gestures());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        got,
        vec![
            ParamEvent::Begin(ParamId(0)),
            ParamEvent::Value(ParamId(0), 25.0),
            ParamEvent::End(ParamId(0)),
        ]
    );
    assert_eq!(inst.param_value(ParamId(0)), Some(25.0));
}
