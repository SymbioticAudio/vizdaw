//! VST3 state (host track H1 Task 5): both streams (the component's and
//! the controller's own) round trip through one `.vstpreset`-layout blob
//! into a new instance, in the SDK's load order; damaged and foreign
//! blobs are refused before the plug-in sees them; a stream the plug-in
//! refuses puts its old state back; state works while active. The test
//! module records what it was given (`test_plugins::Vst3Probe`); each test
//! loads a private copy of the bundle, a separate loaded image with its
//! own counters.

use core_engine::backend::BlockInfo;
use core_host::vst3::Vst3Instance;
use core_host::{AudioProcessor, HostError, ParamOut, PluginInstance, ProcessCtx};
use core_model::mixer::ParamId;
use test_plugins::{VST3_GAIN_ID, VST3_PARAM_GAIN, VST3_SYNTH_ID, Vst3Mode, Vst3Probe};

/// A private copy of the test bundle, and a probe on its image.
fn private() -> (tempfile::TempDir, std::path::PathBuf, Vst3Probe) {
    let d = tempfile::tempdir().unwrap();
    let copy = d.path().join("Private.vst3");
    test_plugins::copy_vst3_bundle(&copy);
    let probe = Vst3Probe::open(&copy);
    (d, copy, probe)
}

const GAIN: ParamId = ParamId(VST3_PARAM_GAIN as u64);

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Runs one 64-frame block of 1.0 through an active Test Gain and returns
/// the first output sample (the component's gain, linear).
fn level(p: &mut dyn AudioProcessor) -> f32 {
    let mut input: Vec<Box<[f32]>> = (0..2).map(|_| vec![1.0f32; 64].into()).collect();
    let mut sidechain: Vec<Box<[f32]>> = (0..2).map(|_| vec![0.0f32; 64].into()).collect();
    let mut outputs: Vec<Box<[f32]>> = (0..2).map(|_| vec![0.0f32; 64].into()).collect();
    let mut out = ParamOut::default();
    let mut ctx = ProcessCtx {
        frames: 64,
        input: &mut input,
        sidechain: &mut sidechain,
        outputs: &mut outputs,
        events: &[],
        params_in: &[],
        params_out: &mut out,
        transport: &BlockInfo::default(),
    };
    p.process(&mut ctx);
    outputs[0][0]
}

fn db(gain_db: f64) -> f32 {
    10f64.powf(gain_db / 20.0) as f32
}

/// A Test Gain whose component runs at `gain_db` (sent with a first
/// block) and whose controller was made with memo `memo`.
fn gain_at(probe: &Vst3Probe, copy: &std::path::Path, gain_db: f64, memo: u64) -> Vst3Instance {
    probe.set_memo(memo);
    let mut inst = Vst3Instance::load(copy, VST3_GAIN_ID).unwrap();
    inst.set_params(&[(GAIN, gain_db)]).unwrap();
    let mut p = inst.activate(48_000.0, 64).unwrap();
    assert!((level(p.as_mut()) - db(gain_db)).abs() < 1e-5);
    inst.deactivate(p);
    inst
}

#[test]
fn both_streams_round_trip_into_a_new_instance_in_the_sdk_order() {
    let (_d, copy, probe) = private();
    let mut a = gain_at(&probe, &copy, -6.0, 77);
    let blob = a.save_state().unwrap();
    assert_eq!(&blob[..4], b"VST3");
    assert_eq!(&blob[8..40], VST3_GAIN_ID.as_bytes());

    // A new instance, at the defaults and with another memo.
    probe.set_memo(1);
    let mut b = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    assert!(!close(b.param_value(GAIN).unwrap(), -6.0));
    assert_ne!(b.save_state().unwrap(), blob);
    let before = probe.counters();
    b.load_state(&blob).unwrap();
    let c = probe.counters();
    assert_eq!(c.component_set_states, before.component_set_states + 1);
    assert_eq!(c.controller_set_states, before.controller_set_states + 1);
    // Component, then the controller with the component's stream, then
    // the controller's own.
    assert!(before.last_component_set_state < c.last_component_set_state);
    assert!(c.last_component_set_state < c.last_state_sync, "{c:?}");
    assert!(c.last_state_sync < c.last_controller_set_state, "{c:?}");
    // The controller-only value came through the controller's stream…
    assert_eq!(c.last_memo, 77);
    // …the component's gain reached the controller…
    assert!(close(b.param_value(GAIN).unwrap(), -6.0));
    // …and the component.
    let mut p = b.activate(48_000.0, 64).unwrap();
    assert!((level(p.as_mut()) - db(-6.0)).abs() < 1e-5);
    b.deactivate(p);
    // Saving again gives the same blob.
    assert_eq!(b.save_state().unwrap(), blob);
}

#[test]
fn a_single_component_plug_in_round_trips_both_streams() {
    let (_d, copy, probe) = private();
    probe.set_memo(5);
    let mut a = Vst3Instance::load(&copy, VST3_SYNTH_ID).unwrap();
    let blob = a.save_state().unwrap();
    probe.set_memo(6);
    let mut b = Vst3Instance::load(&copy, VST3_SYNTH_ID).unwrap();
    assert_ne!(b.save_state().unwrap(), blob);
    b.load_state(&blob).unwrap();
    let c = probe.counters();
    assert!(c.last_component_set_state < c.last_state_sync, "{c:?}");
    assert!(c.last_state_sync < c.last_controller_set_state, "{c:?}");
    assert_eq!(c.last_memo, 5);
    assert_eq!(b.save_state().unwrap(), blob);
}

fn rejected(r: Result<(), HostError>, what: &str) {
    assert!(
        matches!(r, Err(HostError::StateRejected(_))),
        "{what}: {r:?}"
    );
}

#[test]
fn damaged_and_foreign_blobs_are_refused_before_the_plug_in_sees_them() {
    let (_d, copy, probe) = private();
    let blob = gain_at(&probe, &copy, -6.0, 3).save_state().unwrap();
    let synth = Vst3Instance::load(&copy, VST3_SYNTH_ID)
        .unwrap()
        .save_state()
        .unwrap();
    let mut inst = gain_at(&probe, &copy, 0.0, 4);
    let mine = inst.save_state().unwrap();

    let mut cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("not a preset", b"some other host's bytes".to_vec()),
        ("another plug-in's", synth),
        ("cut short", blob[..blob.len() - 3].to_vec()),
        ("header only", blob[..48].to_vec()),
    ];
    let mut far = blob.clone();
    let list = usize::try_from(i64::from_le_bytes(blob[40..48].try_into().unwrap())).unwrap();
    // The component chunk's size, past the end.
    far[list + 8 + 12..list + 8 + 20].copy_from_slice(&(1i64 << 32).to_le_bytes());
    cases.push(("a chunk past the end", far));
    let mut list_far = blob.clone();
    list_far[40..48].copy_from_slice(&i64::MAX.to_le_bytes());
    cases.push(("the list past the end", list_far));
    for (what, bad) in cases {
        let before = probe.counters();
        rejected(inst.load_state(&bad), what);
        let c = probe.counters();
        assert_eq!(
            (c.component_set_states, c.controller_set_states),
            (before.component_set_states, before.controller_set_states),
            "{what}: the plug-in was not called"
        );
        assert_eq!(inst.save_state().unwrap(), mine, "{what}");
    }
}

#[test]
fn a_stream_the_plug_in_refuses_puts_its_old_state_back() {
    let (_d, copy, probe) = private();
    let other = gain_at(&probe, &copy, -6.0, 9).save_state().unwrap();
    let mut inst = gain_at(&probe, &copy, -20.0, 5);
    let mine = inst.save_state().unwrap();
    for mode in [
        Vst3Mode::ControllerStateRefused,
        Vst3Mode::ComponentStateRefused,
    ] {
        probe.set_mode(mode);
        rejected(inst.load_state(&other), &format!("{mode:?}"));
        probe.set_mode(Vst3Mode::Normal);
        assert_eq!(inst.save_state().unwrap(), mine, "{mode:?}");
        assert!(close(inst.param_value(GAIN).unwrap(), -20.0), "{mode:?}");
        let mut p = inst.activate(48_000.0, 64).unwrap();
        assert!(
            (level(p.as_mut()) - db(-20.0)).abs() < 1e-5,
            "{mode:?}: the component's gain"
        );
        inst.deactivate(p);
    }
}

#[test]
fn state_saves_and_loads_while_active_and_older_values_do_not_land_on_it() {
    let (_d, copy, probe) = private();
    let other = gain_at(&probe, &copy, -6.0, 9).save_state().unwrap();
    let mut inst = gain_at(&probe, &copy, 0.0, 1);
    let mine = inst.save_state().unwrap();

    // Active: save and load, the processor plays the loaded gain.
    let mut p = inst.activate(48_000.0, 64).unwrap();
    level(p.as_mut());
    assert_eq!(inst.save_state().unwrap(), mine);
    inst.load_state(&other).unwrap();
    assert!((level(p.as_mut()) - db(-6.0)).abs() < 1e-5);
    assert_eq!(inst.save_state().unwrap(), other);
    inst.deactivate(p);

    // A value set while inactive and not yet sent is older than the load:
    // it does not reach the component with the next first block.
    inst.set_params(&[(GAIN, 0.0)]).unwrap();
    inst.load_state(&mine).unwrap();
    let mut p = inst.activate(48_000.0, 64).unwrap();
    assert!((level(p.as_mut()) - db(0.0)).abs() < 1e-5);
    inst.deactivate(p);
    inst.set_params(&[(GAIN, -12.0)]).unwrap();
    inst.load_state(&other).unwrap();
    let mut p = inst.activate(48_000.0, 64).unwrap();
    assert!(
        (level(p.as_mut()) - db(-6.0)).abs() < 1e-5,
        "the loaded gain, not the older -12 dB"
    );
    inst.deactivate(p);
}

#[test]
fn a_plug_in_that_seeks_past_the_end_round_trips() {
    let (_d, copy, probe) = private();
    probe.set_mode(Vst3Mode::StateSeeksPastEnd);
    let mut a = gain_at(&probe, &copy, -6.0, 2);
    let blob = a.save_state().unwrap();
    let mut b = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    // The component wrote 8 bytes, skipped 4 past the end and wrote 4:
    // a 16-byte chunk with a zero gap, which it reads back (and past).
    b.load_state(&blob).unwrap();
    assert_eq!(b.save_state().unwrap(), blob);
    probe.set_mode(Vst3Mode::Normal);
    let normal = b.save_state().unwrap();
    assert_eq!(blob.len(), normal.len() + 8);
    let mut p = b.activate(48_000.0, 64).unwrap();
    assert!((level(p.as_mut()) - db(-6.0)).abs() < 1e-5);
    b.deactivate(p);
}
