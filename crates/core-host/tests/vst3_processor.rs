//! VST3 processing (host track H1 Task 4): activation order, audio and
//! the sidechain, parameter values both ways (first block, host values,
//! controller edits, output changes), MIDI notes and `IMidiMapping`
//! controllers, the transport, latency and tail, and the render mode.
//! The test module records what it was given (`test_plugins::Vst3Probe`);
//! each test loads a private copy of the bundle, a separate loaded image
//! with its own counters.

use core_engine::backend::BlockInfo;
use core_engine::graph::ProcessStatus;
use core_host::vst3::Vst3Instance;
use core_host::{
    AudioProcessor, MidiEvent, OfflineRender, ParamChange, ParamEvent, ParamOut, PluginInfo,
    PluginInstance, ProcessCtx, Tail,
};
use core_model::mixer::ParamId;
use test_plugins::{
    VST3_GAIN_ID, VST3_PARAM_BEND, VST3_PARAM_GAIN, VST3_PARAM_METER, VST3_PARAM_MOD,
    VST3_PARAM_MODE, VST3_PARAM_PRESSURE, VST3_PARAM_SECRET, VST3_PARAMS, VST3_SYNTH_ID, Vst3Edit,
    Vst3Mode, Vst3Probe, vst3_restart,
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

fn norm(id: u32, plain: f64) -> f64 {
    VST3_PARAMS
        .iter()
        .find(|p| p.id == id)
        .unwrap()
        .to_normalized(plain)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// The buffers of one block for a plug-in with `info`'s ports.
struct Block {
    input: Vec<Box<[f32]>>,
    sidechain: Vec<Box<[f32]>>,
    outputs: Vec<Box<[f32]>>,
    out: ParamOut,
}

impl Block {
    fn new(info: &PluginInfo, frames: usize) -> Block {
        let chans = |n: usize| -> Vec<Box<[f32]>> {
            (0..n)
                .map(|_| vec![0.0f32; frames].into_boxed_slice())
                .collect()
        };
        let main = info
            .main_input()
            .and_then(|i| info.inputs.get(i))
            .map_or(0, |p| usize::from(p.channels));
        let sc = info
            .sidechain_input()
            .and_then(|i| info.inputs.get(i))
            .map_or(0, |p| usize::from(p.channels));
        Block {
            input: chans(main),
            sidechain: chans(sc),
            outputs: chans(info.output_channels()),
            out: ParamOut::default(),
        }
    }

    fn fill(&mut self, input: f32, sidechain: f32) {
        for c in &mut self.input {
            c.fill(input);
        }
        for c in &mut self.sidechain {
            c.fill(sidechain);
        }
    }

    fn run(
        &mut self,
        p: &mut dyn AudioProcessor,
        frames: u32,
        events: &[MidiEvent],
        params: &[ParamChange],
        transport: &BlockInfo,
    ) -> ProcessStatus {
        self.out.clear();
        let mut ctx = ProcessCtx {
            frames,
            input: &mut self.input,
            sidechain: &mut self.sidechain,
            outputs: &mut self.outputs,
            events,
            params_in: params,
            params_out: &mut self.out,
            transport,
        };
        p.process(&mut ctx)
    }

    fn quiet(&mut self, p: &mut dyn AudioProcessor, frames: u32) -> ProcessStatus {
        self.run(p, frames, &[], &[], &BlockInfo::default())
    }
}

#[test]
fn activation_follows_the_sdk_order_and_deactivation_reverses_it() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    let mut proc = inst.activate(44_100.0, 256).unwrap();
    assert!(inst.is_active());
    let c = probe.counters();
    assert_eq!(f64::from_bits(c.last_setup_rate), 44_100.0, "{c:?}");
    assert_eq!(c.last_setup_block, 256, "{c:?}");
    assert_eq!(c.last_process_mode, 0, "real time: {c:?}");
    assert!(c.arrangements >= 1, "{c:?}");
    // Main in, sidechain in, main out.
    assert_eq!(c.buses_on, 3, "{c:?}");
    assert_eq!(c.sidechain_on, 1, "{c:?}");
    assert_eq!(c.set_active_on, 1, "{c:?}");
    assert!(c.last_setup < c.last_set_active_on, "{c:?}");
    assert_eq!(
        c.processing_on, 0,
        "processing starts on the audio thread: {c:?}"
    );

    let mut b = Block::new(inst.info(), 256);
    assert_eq!(b.quiet(proc.as_mut(), 256), ProcessStatus::Ok);
    assert_eq!(b.quiet(proc.as_mut(), 100), ProcessStatus::Ok);
    let c = probe.counters();
    assert_eq!(c.processing_on, 1, "once: {c:?}");
    assert!(c.last_set_active_on < c.last_processing_on, "{c:?}");
    assert!(c.last_processing_on < c.first_process, "{c:?}");
    assert_eq!(c.process_calls, 2, "{c:?}");
    assert_eq!(c.last_frames, 100, "{c:?}");
    assert_eq!(c.bad_process, 0, "nothing out of order: {c:?}");

    inst.deactivate(proc);
    assert!(!inst.is_active());
    let c = probe.counters();
    assert_eq!((c.processing_off, c.set_active_off), (1, 1), "{c:?}");
    assert!(c.last_processing_off < c.last_set_active_off, "{c:?}");
    assert_eq!(c.bad_process, 0, "{c:?}");
}

#[test]
fn activating_twice_fails_and_a_dropped_processor_is_reclaimed() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    let mut proc = inst.activate(48_000.0, 128).unwrap();
    assert!(inst.activate(48_000.0, 128).is_err(), "a processor is out");
    let mut b = Block::new(inst.info(), 128);
    b.quiet(proc.as_mut(), 128);
    // Dropped without `deactivate` (the engine went away).
    drop(proc);
    let c = probe.counters();
    assert_eq!((c.processing_off, c.set_active_off), (1, 0), "{c:?}");
    inst.idle();
    assert!(!inst.is_active());
    assert_eq!(probe.counters().set_active_off, 1);
    let proc = inst.activate(48_000.0, 128).unwrap();
    assert_eq!(probe.counters().set_active_on, 2);
    // An instance dropped while active is deactivated before it goes.
    drop(proc);
    drop(inst);
    let c = probe.counters();
    assert_eq!(c.set_active_off, 2, "{c:?}");
    assert!(c.last_set_active_off < c.last_component_term, "{c:?}");
}

#[test]
fn audio_passes_with_the_gain_and_the_sidechain() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    // 0 dB, set while inactive: it reaches the component with the first
    // block, at offset 0, and only then.
    inst.set_params(&[(pid(VST3_PARAM_GAIN), 0.0)]).unwrap();
    let mut proc = inst.activate(48_000.0, 64).unwrap();
    let mut b = Block::new(inst.info(), 64);
    b.fill(0.25, 0.0);
    b.quiet(proc.as_mut(), 64);
    let c = probe.counters();
    assert!(close(
        c.param_in(VST3_PARAM_GAIN).unwrap(),
        norm(VST3_PARAM_GAIN, 0.0)
    ));
    assert_eq!((c.param_points, c.last_param_offset), (1, 0), "{c:?}");
    assert!(
        b.outputs
            .iter()
            .all(|ch| ch.iter().all(|s| (s - 0.25).abs() < 1e-6))
    );
    // The sidechain bus gets the context's sidechain.
    b.fill(0.0, 0.5);
    b.quiet(proc.as_mut(), 64);
    assert!(
        b.outputs
            .iter()
            .all(|ch| ch.iter().all(|s| (s - 0.5).abs() < 1e-6))
    );
    let c = probe.counters();
    assert_eq!(
        c.param_points, 1,
        "the first block's values are sent once: {c:?}"
    );
    assert_eq!(c.sidechain_blocks, 2, "{c:?}");
    inst.deactivate(proc);
}

#[test]
fn host_values_are_normalised_and_the_controller_follows() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    let mut proc = inst.activate(48_000.0, 64).unwrap();
    let mut b = Block::new(inst.info(), 64);
    b.fill(1.0, 0.0);
    let params = [
        ParamChange {
            frame: 7,
            id: pid(VST3_PARAM_GAIN),
            value: -12.0,
        },
        ParamChange {
            frame: 9,
            id: pid(VST3_PARAM_MODE),
            value: 2.0,
        },
        // Read-only and unknown parameters are not sent.
        ParamChange {
            frame: 9,
            id: pid(VST3_PARAM_METER),
            value: 0.5,
        },
        ParamChange {
            frame: 9,
            id: ParamId(999),
            value: 0.5,
        },
    ];
    b.run(proc.as_mut(), 64, &[], &params, &BlockInfo::default());
    let c = probe.counters();
    assert_eq!(c.param_points, 2, "{c:?}");
    assert!(
        close(c.param_in(VST3_PARAM_GAIN).unwrap(), 48.0 / 72.0),
        "{c:?}"
    );
    assert!(close(c.param_in(VST3_PARAM_MODE).unwrap(), 1.0), "{c:?}");
    assert_eq!(c.param_in(VST3_PARAM_METER), None);
    // The component took -12 dB.
    let g = 10f32.powf(-12.0 / 20.0);
    assert!((b.outputs[0][0] - g).abs() < 1e-5, "{}", b.outputs[0][0]);
    // What the processor reported: Secret (writable) reaches the host as a
    // plain value; Meter (read-only) does not.
    assert_eq!(
        b.out.as_slice(),
        [ParamEvent::Value(pid(VST3_PARAM_SECRET), 1.0)]
    );
    // The controller follows the host's values and the processor's.
    inst.idle();
    assert!(close(
        inst.param_value(pid(VST3_PARAM_GAIN)).unwrap(),
        -12.0
    ));
    assert!(close(inst.param_value(pid(VST3_PARAM_MODE)).unwrap(), 2.0));
    assert!(close(
        inst.param_value(pid(VST3_PARAM_SECRET)).unwrap(),
        1.0
    ));
    let meter = inst.param_value(pid(VST3_PARAM_METER)).unwrap();
    assert!((meter - f64::from(g)).abs() < 1e-5, "{meter}");
    inst.deactivate(proc);
}

#[test]
fn controller_edits_reach_the_component_wherever_it_reads_next() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    let gain = |probe: &Vst3Probe| probe.counters().param_in(VST3_PARAM_GAIN).unwrap();
    // Made while inactive, drained after activation: the next block.
    assert_eq!(probe.edit(Vst3Edit::Perform(VST3_PARAM_GAIN, 0.5)), 0);
    let mut proc = inst.activate(48_000.0, 64).unwrap();
    let mut b = Block::new(inst.info(), 64);
    assert_eq!(probe.counters().param_points, 0);
    assert_eq!(inst.gestures().len(), 1);
    b.quiet(proc.as_mut(), 64);
    assert!(close(gain(&probe), 0.5));
    // Made and drained while active: the next block.
    probe.edit(Vst3Edit::Perform(VST3_PARAM_GAIN, 0.75));
    inst.gestures();
    b.quiet(proc.as_mut(), 64);
    assert!(close(gain(&probe), 0.75));
    // Drained, but the processor went before its next block: the next
    // processor's first block.
    probe.edit(Vst3Edit::Perform(VST3_PARAM_GAIN, 0.4));
    inst.gestures();
    inst.deactivate(proc);
    let mut proc = inst.activate(48_000.0, 64).unwrap();
    b.quiet(proc.as_mut(), 64);
    assert!(close(gain(&probe), 0.4));
    // Made while active, drained after deactivation: the next first block.
    probe.edit(Vst3Edit::Perform(VST3_PARAM_GAIN, 0.6));
    inst.deactivate(proc);
    inst.gestures();
    let points = probe.counters().param_points;
    let mut proc = inst.activate(48_000.0, 64).unwrap();
    b.quiet(proc.as_mut(), 64);
    assert!(close(gain(&probe), 0.6));
    assert_eq!(probe.counters().param_points, points + 1);
    // An edit is not sent back to its controller (which has it): the
    // test controller's own value stays the component state's.
    probe.edit(Vst3Edit::Perform(VST3_PARAM_GAIN, 0.3));
    inst.gestures();
    b.quiet(proc.as_mut(), 64);
    inst.idle();
    let held = -60.0 + 72.0 * test_plugins::VST3_COMPONENT_GAIN;
    assert!(close(inst.param_value(pid(VST3_PARAM_GAIN)).unwrap(), held));
    inst.deactivate(proc);
}

#[test]
fn notes_and_mapped_controllers_reach_an_instrument() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_SYNTH_ID).unwrap();
    let mut proc = inst.activate(48_000.0, 64).unwrap();
    let mut b = Block::new(inst.info(), 64);
    let events = [
        MidiEvent::new(5, &[0x92, 60, 127]),
        MidiEvent::new(10, &[0xB2, 1, 127]),
        MidiEvent::new(20, &[0xE2, 0x7F, 0x7F]),
        MidiEvent::new(30, &[0xD2, 64]),
        MidiEvent::new(31, &[0xA2, 60, 10]),
        // Not assigned: dropped.
        MidiEvent::new(40, &[0xB2, 7, 100]),
    ];
    b.run(proc.as_mut(), 64, &events, &[], &BlockInfo::default());
    let c = probe.counters();
    assert_eq!(c.note_ons, 1, "{c:?}");
    assert_eq!(c.last_note & 0xFF, 60, "{c:?}");
    assert_eq!((c.last_note >> 8) & 0xFF, 2, "channel: {c:?}");
    assert_eq!(c.last_note >> 16, 5, "offset: {c:?}");
    assert!(close(f64::from_bits(c.last_velocity), 1.0), "{c:?}");
    assert_eq!(c.poly_pressures, 1, "{c:?}");
    assert_eq!(c.param_points, 3, "CC 1, bend and pressure only: {c:?}");
    assert!(close(c.param_in(VST3_PARAM_MOD).unwrap(), 1.0), "{c:?}");
    assert!(close(c.param_in(VST3_PARAM_BEND).unwrap(), 1.0), "{c:?}");
    assert!(
        close(c.param_in(VST3_PARAM_PRESSURE).unwrap(), 64.0 / 127.0),
        "{c:?}"
    );
    assert_eq!(c.last_param_offset, 30, "{c:?}");
    // Both output buses get buffers: a held note on the first, the
    // second's constant.
    assert!(b.outputs[..2].iter().all(|ch| ch.iter().all(|s| *s == 0.5)));
    assert!(
        b.outputs[2..]
            .iter()
            .all(|ch| ch.iter().all(|s| *s == 0.25))
    );
    // The controller follows mapped controllers.
    inst.idle();
    assert!(close(inst.param_value(pid(VST3_PARAM_MOD)).unwrap(), 1.0));
    b.run(
        proc.as_mut(),
        64,
        &[MidiEvent::new(0, &[0x82, 60, 0])],
        &[],
        &BlockInfo::default(),
    );
    assert_eq!(probe.counters().note_offs, 1);
    assert!(b.outputs[0].iter().all(|s| *s == 0.0));
    // A note-on with velocity 0 is a note-off.
    b.run(
        proc.as_mut(),
        64,
        &[MidiEvent::new(0, &[0x90, 61, 0])],
        &[],
        &BlockInfo::default(),
    );
    assert_eq!(probe.counters().note_offs, 2);
    inst.deactivate(proc);
}

#[test]
fn the_transport_reaches_the_plug_in() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    let mut proc = inst.activate(48_000.0, 64).unwrap();
    let mut b = Block::new(inst.info(), 64);
    let t = BlockInfo {
        playing: true,
        frame: 0,
        beats: 9.0,
        tempo_bpm: 90.0,
    };
    b.run(proc.as_mut(), 64, &[], &[], &t);
    b.run(proc.as_mut(), 32, &[], &[], &t);
    let c = probe.counters();
    assert_eq!(f64::from_bits(c.last_tempo), 90.0);
    assert_eq!(f64::from_bits(c.last_music), 9.0);
    assert_eq!(f64::from_bits(c.last_bar), 8.0);
    assert_eq!(f64::from_bits(c.last_context_rate), 48_000.0);
    // 9 quarter notes at 90 BPM are 6 s.
    assert_eq!(c.last_project_samples, 288_000);
    assert_eq!(
        c.last_cont_samples, 64,
        "samples processed before this block"
    );
    // kPlaying, kTempoValid, kTimeSigValid, kProjectTimeMusicValid,
    // kBarPositionValid, kContTimeValid.
    for flag in [2, 1024, 8192, 512, 2048, 131_072] {
        assert_ne!(c.last_state & flag, 0, "flag {flag}: {:#x}", c.last_state);
    }
    b.quiet(proc.as_mut(), 32);
    assert_eq!(probe.counters().last_state & 2, 0, "stopped");
    inst.deactivate(proc);
}

#[test]
fn latency_and_tail_are_read_at_activation_and_a_change_asks_for_a_restart() {
    let (_d, copy, probe) = private();
    probe.set_timing(64, 100);
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    let proc = inst.activate(48_000.0, 64).unwrap();
    assert_eq!(inst.info().latency, 64);
    assert_eq!(inst.info().tail, Tail::Frames(100));
    // The plug-in's latency changes (as from its processing thread).
    probe.set_timing(128, u32::MAX);
    probe.edit_off_thread(Vst3Edit::Restart(vst3_restart::LATENCY_CHANGED));
    let ev = inst.idle();
    assert!(ev.latency_changed, "{ev:?}");
    // The host restarts it: the new latency is read.
    inst.deactivate(proc);
    let proc = inst.activate(48_000.0, 64).unwrap();
    assert_eq!(inst.info().latency, 128);
    assert_eq!(inst.info().tail, Tail::Infinite);
    inst.deactivate(proc);
}

#[test]
fn the_render_mode_is_set_up_at_activation_and_kept_when_a_switch_is_refused() {
    let (_d, copy, probe) = private();
    let mut inst = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    assert_eq!(inst.set_offline(true), OfflineRender::Taken);
    // It will not switch back: it stays offline.
    probe.set_mode(Vst3Mode::RealtimeRefused);
    assert_eq!(inst.set_offline(false), OfflineRender::Refused);
    let mut proc = inst.activate(48_000.0, 64).unwrap();
    assert_eq!(probe.counters().last_process_mode, 2, "kOffline");
    let mut b = Block::new(inst.info(), 64);
    b.quiet(proc.as_mut(), 64);
    assert_eq!(probe.counters().last_data_mode, 2, "process in kOffline");
    inst.deactivate(proc);
    probe.set_mode(Vst3Mode::Normal);
    assert_eq!(inst.set_offline(false), OfflineRender::Taken);
    let mut proc = inst.activate(48_000.0, 64).unwrap();
    b.quiet(proc.as_mut(), 64);
    let c = probe.counters();
    assert_eq!((c.last_process_mode, c.last_data_mode), (0, 0), "{c:?}");
    inst.deactivate(proc);
}
