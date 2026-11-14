//! The bundled Multi-Q plays the curve `map_equalizer` asks for (plan 6g
//! Task 5, Ruling 10): a bell and a high-pass from a DAWproject
//! `Equalizer`, their overlay applied before activation, measured with
//! sines. This pins the parameter domain, Peaking's shape index and one
//! slope index.
//!
//! Needs the built plug-in: run `scripts/build-multiq.sh`, then
//! `VIZDAW_BUNDLED_PLUGINS=$PWD/target/multiq/out cargo test -p core-ffi
//! --test multiq_render -- --ignored`.

#![cfg(target_os = "macos")]

use core_engine::backend::BlockInfo;
use core_host::clap::ClapFormat;
use core_host::{LoadRequest, ParamOut, PluginFormat, PluginInstance, ProcessCtx};
use core_io::dawproject::multiq::map_equalizer;
use core_io::dawproject::{DpEqBand, DpEqBandType, DpNamed, DpRealParam, DpUnit};
use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef};
use std::path::PathBuf;

const RATE: f64 = 48_000.0;
const MAX_FRAMES: u32 = 4096;
const BLOCK: usize = 512;

/// The bundled `Multi-Q.clap` (`VIZDAW_BUNDLED_PLUGINS`), which must exist.
fn clap_bundle() -> PathBuf {
    let dir = match std::env::var_os("VIZDAW_BUNDLED_PLUGINS") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => panic!("VIZDAW_BUNDLED_PLUGINS is not set: build it with scripts/build-multiq.sh"),
    };
    let path = dir.join("Multi-Q.clap");
    assert!(
        path.exists(),
        "{} is missing: build it with scripts/build-multiq.sh",
        path.display()
    );
    path
}

/// Loads the bundled Multi-Q through `ClapFormat`, as Task 1 does.
fn load() -> Box<dyn PluginInstance> {
    let mut f = ClapFormat::new();
    let req = LoadRequest {
        plugin: PluginRef {
            format: Format::Clap,
            id: "com.dusk-audio.multi-q".into(),
            name: "Multi-Q".into(),
            vendor: "Dusk Audio".into(),
            version: String::new(),
        },
        path: Some(clap_bundle()),
        sample_rate: RATE,
        max_frames: MAX_FRAMES,
    };
    let prepared = (f.prepare(&req))().unwrap();
    f.finish(&req, prepared).unwrap()
}

fn real(unit: DpUnit, v: f64) -> Option<DpRealParam> {
    Some(DpRealParam {
        named: DpNamed::default(),
        parameter_id: None,
        unit,
        value: Some(v),
        min: None,
        max: None,
    })
}

/// The gain, dB, of a freshly loaded Multi-Q (stereo, both channels'
/// mean) for a −18 dBFS sine at `freq`, over one second after a warm-up,
/// with `params` set before activation. (A fresh load each time: a second
/// activation of one instance played flat here, its values unchanged.)
fn gain_at(params: &[(ParamId, f64)], freq: f64) -> f64 {
    let mut inst = load();
    inst.set_params(params).unwrap();
    let amp = 10f64.powf(-18.0 / 20.0);
    let total = RATE as usize;
    let warm_up = 8192;
    let mut p = inst.activate(RATE, MAX_FRAMES).unwrap();
    let info = BlockInfo::default();
    let (mut sum_in, mut sum_out, mut start) = (0.0f64, [0.0f64; 2], 0usize);
    while start < total {
        let frames = BLOCK.min(total - start);
        let sine: Vec<f32> = (start..start + frames)
            .map(|n| (amp * (std::f64::consts::TAU * freq * n as f64 / RATE).sin()) as f32)
            .collect();
        let mut input: Vec<Box<[f32]>> = (0..2)
            .map(|_| {
                let mut b = vec![0.0f32; BLOCK];
                b[..frames].copy_from_slice(&sine);
                b.into_boxed_slice()
            })
            .collect();
        let mut sidechain: Vec<Box<[f32]>> = Vec::new();
        let mut outputs: Vec<Box<[f32]>> = (0..2)
            .map(|_| vec![0.0f32; BLOCK].into_boxed_slice())
            .collect();
        let mut out = ParamOut::default();
        let mut ctx = ProcessCtx {
            frames: frames as u32,
            input: &mut input,
            sidechain: &mut sidechain,
            outputs: &mut outputs,
            events: &[],
            params_in: &[],
            params_out: &mut out,
            transport: &info,
        };
        p.process(&mut ctx);
        for i in 0..frames {
            if start + i >= warm_up {
                sum_in += f64::from(sine[i]).powi(2);
                for (ch, buf) in outputs.iter().enumerate() {
                    assert!(buf[i].is_finite(), "{freq} Hz, channel {ch}");
                    sum_out[ch] += f64::from(buf[i]).powi(2);
                }
            }
        }
        start += frames;
    }
    inst.deactivate(p);
    let db = |s: f64| 10.0 * (s / sum_in).log10();
    (db(sum_out[0]) + db(sum_out[1])) / 2.0
}

/// `band`'s overlay, which must map whole.
fn mapped(band: DpEqBand) -> Vec<(ParamId, f64)> {
    let m = map_equalizer(&[band], "T");
    assert!(m.left_out.is_empty() && m.changed.is_empty(), "{m:?}");
    m.params
}

#[test]
#[ignore = "needs scripts/build-multiq.sh and VIZDAW_BUNDLED_PLUGINS"]
fn bundled_multiq_plays_the_mapped_curve() {
    vizdaw_ffi::host::force_offline();
    // Multi-Q may write its settings on load: never into the real home.
    let home = tempfile::tempdir().unwrap();
    // SAFETY: this file's only test; nothing else in this process reads
    // the environment meanwhile.
    unsafe { std::env::set_var("HOME", home.path()) };

    // A bell at 1 kHz, +6 dB, Q 1: band 3, Peaking.
    let bell = mapped(DpEqBand {
        kind: Some(DpEqBandType::Bell),
        order: None,
        freq: real(DpUnit::Hertz, 1000.0),
        gain: real(DpUnit::Decibel, 6.0),
        q: real(DpUnit::Linear, 1.0),
        enabled: None,
    });
    let at = |params: &[(ParamId, f64)], f: f64| {
        let g = gain_at(params, f);
        println!("{f} Hz: {g:.2} dB");
        g
    };
    let g = at(&bell, 1000.0);
    assert!((g - 6.0).abs() <= 0.5, "bell at 1 kHz: {g:.2} dB");
    for f in [100.0, 10_000.0] {
        let g = at(&bell, f);
        assert!(g.abs() <= 0.5, "bell at {f} Hz: {g:.2} dB");
    }

    // A high-pass at 1 kHz, order 4: band 1 at 24 dB/oct.
    let hp = mapped(DpEqBand {
        kind: Some(DpEqBandType::HighPass),
        order: Some(4),
        freq: real(DpUnit::Hertz, 1000.0),
        gain: None,
        q: None,
        enabled: None,
    });
    // 24 dB/oct is −24 dB one octave below and −48 dB two below (−24 dB
    // at 250 Hz would be the 12 dB/oct default).
    let g = at(&hp, 500.0);
    assert!((g + 24.0).abs() <= 3.0, "high-pass at 500 Hz: {g:.2} dB");
    let g = at(&hp, 250.0);
    assert!((g + 48.0).abs() <= 3.0, "high-pass at 250 Hz: {g:.2} dB");
}

/// The gain, dB, of `node` for a −18 dBFS sine at `freq` at `rate`, as
/// [`gain_at`] measures it.
fn node_gain(node: &mut dyn core_engine::graph::EffectNode, rate: f64, freq: f64) -> f64 {
    use core_engine::backend::StereoBuf;
    let amp = 10f64.powf(-18.0 / 20.0);
    let total = rate as usize;
    let warm_up = 8192;
    let (mut sum_in, mut sum_out, mut start) = (0.0f64, 0.0f64, 0usize);
    let mut io = StereoBuf::new(BLOCK as u32);
    while start < total {
        let frames = BLOCK.min(total - start);
        let sine: Vec<f32> = (start..start + frames)
            .map(|n| (amp * (std::f64::consts::TAU * freq * n as f64 / rate).sin()) as f32)
            .collect();
        io.left[..frames].copy_from_slice(&sine);
        io.right[..frames].copy_from_slice(&sine);
        node.process(frames as u32, &mut io, None);
        for (i, ((x, l), r)) in sine
            .iter()
            .zip(io.left.iter())
            .zip(io.right.iter())
            .enumerate()
        {
            if start + i >= warm_up {
                sum_in += 2.0 * f64::from(*x).powi(2);
                sum_out += f64::from(*l).powi(2) + f64::from(*r).powi(2);
            }
        }
        start += frames;
    }
    10.0 * (sum_out / sum_in).log10()
}

/// Plan 6g Task 5 fix I2: a sample-rate change re-activates the same
/// instance (`PluginHost::reconfigure`); the host round-trips its state
/// first, so Multi-Q's pass filter still plays (it stayed flat before).
#[test]
#[ignore = "needs scripts/build-multiq.sh and VIZDAW_BUNDLED_PLUGINS"]
fn bundled_multiq_keeps_its_high_pass_through_a_rate_change() {
    use core_engine::fixtures::Builder;
    use core_engine::graph::InsertFactory;
    use core_host::PluginHost;
    use core_model::command::MixerCmd;
    use core_model::ids::AnyId;
    use std::time::Duration;
    vizdaw_ffi::host::force_offline();
    let home = tempfile::tempdir().unwrap();
    // SAFETY: as in `bundled_multiq_plays_the_mapped_curve`; Rust runs this
    // file's tests on threads, but both only ever set HOME to a temp dir.
    unsafe { std::env::set_var("HOME", home.path()) };

    let plugin = PluginRef {
        format: Format::Clap,
        id: "com.dusk-audio.multi-q".into(),
        name: "Multi-Q".into(),
        vendor: "Dusk Audio".into(),
        version: String::new(),
    };
    let params = mapped(DpEqBand {
        kind: Some(DpEqBandType::HighPass),
        order: Some(4),
        freq: real(DpUnit::Hertz, 1000.0),
        gain: None,
        q: None,
        enabled: None,
    });
    let mut b = Builder::new();
    let master = b.doc().mixer().master();
    let r = b
        .mixer(MixerCmd::SetEq {
            strip: master,
            plugin: Some(plugin.clone()),
        })
        .unwrap();
    let id = r
        .created
        .iter()
        .find_map(|x| match x {
            AnyId::Plugin(p) => Some(*p),
            _ => None,
        })
        .unwrap();
    b.mixer(MixerCmd::SetPluginParams { slot: id, params })
        .unwrap();
    let doc = b.finish();
    let slot = doc.plugin_slot(id).unwrap().clone();

    let mut host = PluginHost::new(RATE, MAX_FRAMES).unwrap();
    host.register_path(Format::Clap, &plugin.id, clap_bundle());
    host.sync(&doc);
    let r = host.wait_ready(Duration::from_secs(20));
    assert_eq!(r.loaded, vec![id], "{:?}", r.failed);
    let mut factory = host.factory();
    let mut take = |host: &mut PluginHost, rate: f64| {
        for _ in 0..2000 {
            host.idle();
            if let Ok(n) = factory.make(&slot, rate, MAX_FRAMES) {
                return n;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("Multi-Q is not ready at {rate} Hz")
    };
    let mut node = take(&mut host, RATE);
    let g = node_gain(node.as_mut(), RATE, 500.0);
    println!("before the rate change, 500 Hz: {g:.2} dB");
    assert!((g + 24.0).abs() <= 3.0, "before: {g:.2} dB");

    // The rate changes while the engine holds the processor; it comes
    // back, and the host activates the same instance again.
    let rate = 44_100.0;
    host.reconfigure(rate, MAX_FRAMES);
    drop(node);
    let mut node = take(&mut host, rate);
    let g = node_gain(node.as_mut(), rate, 500.0);
    println!("after the rate change, 500 Hz: {g:.2} dB");
    assert!((g + 24.0).abs() <= 3.0, "after: {g:.2} dB");
    drop(node);
    host.idle();
}
