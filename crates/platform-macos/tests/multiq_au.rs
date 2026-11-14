//! The bundled Multi-Q AU through a sample-rate change (plan 6g final
//! fixes): the host activates the same instance again, and its high-pass
//! must still play. The CLAP twin is core-ffi's
//! `bundled_multiq_keeps_its_high_pass_through_a_rate_change`.
//!
//! The AU is registered in this process only, from the bundle's own
//! factory function (`AudioComponentRegister`): nothing is installed and
//! the system's component registry is not touched. Each test runs again
//! in a child process whose `HOME` and `CFFIXED_USER_HOME` are a temp
//! folder (`core-host/tests/support/temp_home.rs`): Multi-Q writes its
//! crash-log folder under the home on load.
//!
//! Needs the built plug-in: run `scripts/build-multiq.sh`, then
//! `VIZDAW_BUNDLED_PLUGINS=$PWD/target/multiq/out cargo test -p
//! platform-macos --test multiq_au -- --ignored`.

#![cfg(target_os = "macos")]

#[path = "../../core-host/tests/support/temp_home.rs"]
mod temp_home;

use core_engine::backend::StereoBuf;
use core_engine::fixtures::Builder;
use core_engine::graph::{EffectNode, InsertFactory};
use core_host::PluginHost;
use core_host::bundled::{MULTIQ_AU_BUNDLE, MULTIQ_AU_ID};
use core_model::command::MixerCmd;
use core_model::ids::AnyId;
use core_model::mixer::ParamId;
use objc2_audio_toolbox::{
    AudioComponentDescription, AudioComponentFactoryFunction, AudioComponentRegister,
};
use objc2_core_foundation::{CFBundle, CFString, CFURL};
use platform_macos::au::{AuFormat, component, parse_au_id};
use std::path::PathBuf;
use std::ptr::NonNull;
use std::time::Duration;

const RATE: f64 = 48_000.0;
const MAX_FRAMES: u32 = 4096;
const BLOCK: usize = 512;

/// The bundled `Multi-Q.component` (`VIZDAW_BUNDLED_PLUGINS`), which must
/// exist.
fn au_bundle() -> PathBuf {
    let dir = match std::env::var_os("VIZDAW_BUNDLED_PLUGINS") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => panic!("VIZDAW_BUNDLED_PLUGINS is not set: build it with scripts/build-multiq.sh"),
    };
    let path = dir.join(MULTIQ_AU_BUNDLE);
    assert!(
        path.exists(),
        "{} is missing: build it with scripts/build-multiq.sh",
        path.display()
    );
    path
}

/// Registers the bundled AU in this process only: loads the bundle's code
/// and hands its factory function (named in its `Info.plist`) to
/// `AudioComponentRegister`. The bundle stays loaded for the process.
fn register_bundled_au() {
    let path = au_bundle();
    let plist = path.join("Contents/Info.plist");
    let read = |key: &str| {
        let out = std::process::Command::new("/usr/bin/plutil")
            .args([
                "-extract",
                &format!("AudioComponents.0.{key}"),
                "raw",
                "-o",
                "-",
            ])
            .arg(&plist)
            .output()
            .expect("plutil");
        assert!(out.status.success(), "no {key} in {}", plist.display());
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    let factory = read("factoryFunction");
    let name = read("name");
    let version: u32 = read("version").parse().expect("version");
    let (kind, subtype, manufacturer) = parse_au_id(MULTIQ_AU_ID).unwrap();

    let url = CFURL::from_directory_path(&path).expect("the bundle's URL");
    let bundle = CFBundle::new(None, Some(&url)).expect("the AU bundle");
    // SAFETY: loading Multi-Q's code runs its initializers; that is what
    // instantiating it does anyway.
    assert!(unsafe { bundle.load_executable() }, "cannot load the AU");
    let f = bundle.function_pointer_for_name(Some(&CFString::from_str(&factory)));
    assert!(!f.is_null(), "no {factory} in the AU");
    // SAFETY: the bundle's `factoryFunction` is an AudioComponent factory
    // (`AudioComponentFactoryFunction`'s signature), and the bundle is
    // never unloaded (forgotten below).
    let factory: AudioComponentFactoryFunction = unsafe { std::mem::transmute(f) };
    std::mem::forget(bundle);
    let mut desc = AudioComponentDescription {
        componentType: kind,
        componentSubType: subtype,
        componentManufacturer: manufacturer,
        componentFlags: 0,
        componentFlagsMask: 0,
    };
    // SAFETY: `desc` and the name are valid for the call; the factory
    // stays loaded.
    let c = unsafe {
        AudioComponentRegister(
            NonNull::from(&mut desc),
            &CFString::from_str(&name),
            version,
            factory,
        )
    };
    assert!(!c.is_null(), "AudioComponentRegister failed");
}

/// The gain, dB, of `node` for a −18 dBFS sine at `freq` at `rate`.
fn node_gain(node: &mut dyn EffectNode, rate: f64, freq: f64) -> f64 {
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

/// A sample-rate change re-activates the same AU instance
/// (`PluginHost::reconfigure`); band 1's 1 kHz high-pass at 24 dB/oct
/// still takes about 24 dB off 500 Hz afterwards.
#[test]
#[ignore = "needs scripts/build-multiq.sh and VIZDAW_BUNDLED_PLUGINS"]
fn bundled_multiq_au_keeps_its_high_pass_through_a_rate_change() {
    if !temp_home::in_temp_home("bundled_multiq_au_keeps_its_high_pass_through_a_rate_change") {
        return;
    }
    register_bundled_au();
    let plugin = component(MULTIQ_AU_ID)
        .expect("the AU registered in this process")
        .plugin;

    // The AU's own parameters (JUCE's AU wrapper: addresses by index, a
    // continuous value normalised to 0–1, a choice as its index).
    let find = |params: &[core_host::ParamInfo], name: &str| {
        params
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("no {name:?}"))
            .id
    };
    let mut h = PluginHost::new(RATE, MAX_FRAMES).unwrap();
    h.add_format(Box::new(AuFormat::new()));

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
    // Its parameter list, to address band 1 by name.
    let doc = b.doc().clone();
    h.sync(&doc);
    let r = h.wait_ready(Duration::from_secs(60));
    assert_eq!(r.loaded, vec![id], "{:?}", r.failed);
    let params = h.instance(id).unwrap().info().params.clone();
    // The host refreshes its continuous parameters only: never a choice
    // or a switch.
    let refresh = h.instance(id).unwrap().values_to_refresh();
    assert!(!refresh.is_empty(), "the bundled Multi-Q AU names none");
    for name in ["Band 1 Enabled", "Band 1 Slope"] {
        assert!(
            !refresh.contains(&find(&params, name)),
            "{name} is refreshed"
        );
    }
    assert!(refresh.contains(&find(&params, "Band 1 Frequency")));
    for p in params.iter().filter(|p| refresh.contains(&p.id)) {
        assert!(!p.stepped && p.automatable && p.max > p.min, "{p:?}");
    }
    // Band 1 on, at 1 kHz (log 20 Hz – 20 kHz), slope index 3 (24 dB/oct).
    let hp: Vec<(ParamId, f64)> = vec![
        (find(&params, "Band 1 Enabled"), 1.0),
        (
            find(&params, "Band 1 Frequency"),
            (1000.0f64 / 20.0).ln() / 1000.0f64.ln(),
        ),
        (find(&params, "Band 1 Slope"), 3.0),
    ];
    b.mixer(MixerCmd::SetPluginParams {
        slot: id,
        params: hp,
    })
    .unwrap();
    let doc = b.finish();
    let slot = doc.plugin_slot(id).unwrap().clone();
    h.sync(&doc);
    h.wait_ready(Duration::from_secs(60));

    let mut factory = h.factory();
    // Every idle report on the way, to check that the refresh told the
    // session nothing (no gesture, automation event or dirty state).
    let mut reports = Vec::new();
    let mut take = |h: &mut PluginHost, rate: f64| {
        for _ in 0..2000 {
            reports.push(h.idle());
            if let Ok(n) = factory.make(&slot, rate, MAX_FRAMES) {
                return n;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("Multi-Q is not ready at {rate} Hz")
    };
    let mut node = take(&mut h, RATE);
    let g = node_gain(node.as_mut(), RATE, 500.0);
    println!("AU before the rate change, 500 Hz: {g:.2} dB");
    assert!((g + 24.0).abs() <= 3.0, "before: {g:.2} dB");

    // The rate changes while the engine holds the processor; it comes
    // back, and the host activates the same instance again.
    let rate = 44_100.0;
    h.reconfigure(rate, MAX_FRAMES);
    drop(node);
    let mut node = take(&mut h, rate);
    let g = node_gain(node.as_mut(), rate, 500.0);
    println!("AU after the rate change, 500 Hz: {g:.2} dB");
    assert!((g + 24.0).abs() <= 3.0, "after: {g:.2} dB");
    drop(node);
    reports.push(h.idle());

    // The refresh came back to every value and told the session nothing.
    let freq = find(&params, "Band 1 Frequency");
    let v = h.instance(id).unwrap().param_value(freq).unwrap();
    assert!(
        (v - (1000.0f64 / 20.0).ln() / 1000.0f64.ln()).abs() < 1e-6,
        "{v}"
    );
    assert!(h.uncommitted(id).is_empty(), "{:?}", h.uncommitted(id));
    for r in &reports {
        assert!(r.gestures.is_empty(), "{:?}", r.gestures);
        assert!(r.param_events.is_empty(), "{:?}", r.param_events);
        assert!(r.state_dirty.is_empty(), "{:?}", r.state_dirty);
    }
}
