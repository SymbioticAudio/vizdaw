//! The bundled Multi-Q EQ (plan 6g): it lists, loads, is flat by default,
//! and its band parameters match the committed table
//! `tests/data/multiq-params.json`.
//!
//! These tests need the built plug-in: run `scripts/build-multiq.sh`, then
//! `VIZDAW_BUNDLED_PLUGINS=$PWD/target/multiq/out cargo test -p core-host
//! --test multiq -- --ignored`. With `VIZDAW_MULTIQ_DUMP=<path>` as well,
//! the dump test writes the table to `<path>` instead of checking it.
//! Each test runs again in a child process whose `HOME` and
//! `CFFIXED_USER_HOME` are a temp folder (`support/temp_home.rs`).

#![cfg(target_os = "macos")]

// Each test runs in a child whose home is a temp folder: Multi-Q writes
// its crash-log folder under `NSHomeDirectory()` on load.
#[path = "support/temp_home.rs"]
mod temp_home;

use core_engine::backend::BlockInfo;
use core_host::clap::{ClapFormat, list_bundle};
use core_host::{
    LoadRequest, ParamInfo, ParamOut, PluginFormat, PluginInstance, PluginKind, ProcessCtx,
};
use core_model::mixer::{PluginFormat as Format, PluginRef};
use serde_json::{Value, json};
use std::path::PathBuf;

/// Multi-Q's CLAP ID.
const CLAP_ID: &str = "com.dusk-audio.multi-q";
/// Sample rate and largest block the tests load at.
const RATE: f64 = 48_000.0;
const MAX_FRAMES: u32 = 4096;
/// The block size the render test processes in.
const BLOCK: usize = 512;

/// The folder holding the bundled plug-ins (`VIZDAW_BUNDLED_PLUGINS`).
fn bundled() -> PathBuf {
    match std::env::var_os("VIZDAW_BUNDLED_PLUGINS") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => panic!("VIZDAW_BUNDLED_PLUGINS is not set: build it with scripts/build-multiq.sh"),
    }
}

/// The bundled `Multi-Q.clap`, which must exist.
fn clap_bundle() -> PathBuf {
    let path = bundled().join("Multi-Q.clap");
    assert!(
        path.exists(),
        "{} is missing: build it with scripts/build-multiq.sh",
        path.display()
    );
    path
}

/// JUCE's `String::hashCode` over the UTF-32 characters, which
/// clap-juce-extensions uses as the CLAP parameter ID.
fn juce_hash(id: &str) -> u32 {
    let mut h: u32 = 0;
    for c in id.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    h
}

/// Every band parameter Multi-Q has at the pin (`MultiQ.cpp`,
/// `createParameterLayout`): enabled, freq and Q on all 8 bands; gain and
/// shape on bands 2–7; slope on bands 1 and 8.
fn band_param_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for n in 1..=8 {
        ids.push(format!("band{n}_enabled"));
        ids.push(format!("band{n}_freq"));
        if (2..=7).contains(&n) {
            ids.push(format!("band{n}_gain"));
        }
        ids.push(format!("band{n}_q"));
        if (2..=7).contains(&n) {
            ids.push(format!("band{n}_shape"));
        }
        if n == 1 || n == 8 {
            ids.push(format!("band{n}_slope"));
        }
    }
    ids
}

/// Loads the bundled Multi-Q through `ClapFormat` at 48 kHz / 4096.
fn load() -> Box<dyn PluginInstance> {
    let mut f = ClapFormat::new();
    let req = LoadRequest {
        plugin: PluginRef {
            format: Format::Clap,
            id: CLAP_ID.into(),
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

/// The live parameter whose CLAP ID is `juce_hash(string_id)`.
fn param<'a>(params: &'a [ParamInfo], string_id: &str) -> Option<&'a ParamInfo> {
    let id = u64::from(juce_hash(string_id));
    params.iter().find(|p| p.id.0 == id)
}

#[test]
#[ignore = "needs scripts/build-multiq.sh and VIZDAW_BUNDLED_PLUGINS"]
fn bundled_multiq_lists_and_loads() {
    if !temp_home::in_temp_home("bundled_multiq_lists_and_loads") {
        return;
    }
    assert_eq!(juce_hash("band1_freq"), 1_472_467_003);
    assert_eq!(juce_hash("band5_gain"), 1_586_997_182);

    let listed = list_bundle(&clap_bundle()).unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    let (plugin, kind) = &listed[0];
    assert_eq!(plugin.id, CLAP_ID);
    assert_eq!(*kind, PluginKind::Effect);
    assert_eq!(plugin.name, "Multi-Q");
    assert_eq!(plugin.vendor, "Dusk Audio");

    let inst = load();
    let params = &inst.info().params;
    let missing: Vec<String> = band_param_ids()
        .into_iter()
        .filter(|s| param(params, s).is_none())
        .collect();
    assert!(missing.is_empty(), "missing band parameters: {missing:?}");
}

#[test]
#[ignore = "needs scripts/build-multiq.sh and VIZDAW_BUNDLED_PLUGINS"]
fn bundled_multiq_is_flat_by_default() {
    if !temp_home::in_temp_home("bundled_multiq_is_flat_by_default") {
        return;
    }
    let mut inst = load();
    let channels = inst.info().inputs.first().map_or(0, |p| p.channels);
    assert_eq!(channels, 2, "main input is not stereo");
    let amp = 10f32.powf(-12.0 / 20.0);
    let total = RATE as usize;
    let warm_up = 4096;
    for freq in [100.0f32, 1_000.0, 10_000.0] {
        let mut p = inst.activate(RATE, MAX_FRAMES).unwrap();
        let info = BlockInfo::default();
        let mut sum_in = 0.0f64;
        let mut sum_out = [0.0f64; 2];
        let mut measured = 0usize;
        let mut start = 0usize;
        while start < total {
            let frames = BLOCK.min(total - start);
            let sine: Vec<f32> = (start..start + frames)
                .map(|n| amp * (std::f32::consts::TAU * freq * n as f32 / RATE as f32).sin())
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
                for (ch, buf) in outputs.iter().enumerate() {
                    assert!(!buf[i].is_nan(), "NaN at {freq} Hz, channel {ch}");
                }
                if start + i >= warm_up {
                    sum_in += f64::from(sine[i]).powi(2);
                    for (ch, buf) in outputs.iter().enumerate() {
                        sum_out[ch] += f64::from(buf[i]).powi(2);
                    }
                    measured += 1;
                }
            }
            start += frames;
        }
        inst.deactivate(p);
        let rms_in = (sum_in / measured as f64).sqrt();
        for (ch, sum) in sum_out.iter().enumerate() {
            let rms_out = (sum / measured as f64).sqrt();
            let db = 20.0 * (rms_out / rms_in).log10();
            assert!(
                db.abs() < 0.05,
                "{freq} Hz, channel {ch}: output is {db:.4} dB from the input"
            );
        }
    }
}

/// The `NormalisableRange` arguments `MultiQ.cpp` gives each band's freq,
/// gain and Q, typed in from the source at the pin.
fn ranges() -> Value {
    json!({
        "freq": { "start": 20.0, "end": 20000.0, "log": true },
        "gain": { "start": -24.0, "end": 24.0, "interval": 0.1, "skew": 1.0 },
        "q": { "start": 0.1, "end": 100.0, "log": true },
    })
}

/// The table of the live band parameters, in `multiq-params.json`'s shape.
fn table(params: &[ParamInfo]) -> Value {
    let freq = param(params, "band1_freq").unwrap();
    let domain = if freq.max > 1.0 {
        "plain"
    } else {
        "normalized"
    };
    let rows: Vec<Value> = (1..=8)
        .flat_map(|n| {
            ["enabled", "freq", "gain", "q", "shape", "slope"]
                .into_iter()
                .map(move |what| format!("band{n}_{what}"))
        })
        .filter_map(|s| {
            param(params, &s).map(|p| {
                json!({
                    "string_id": s,
                    "clap_id": juce_hash(&s),
                    "name": p.name,
                    "min": p.min,
                    "max": p.max,
                    "default": p.default,
                    "stepped": p.stepped,
                })
            })
        })
        .collect();
    json!({ "domain": domain, "ranges": ranges(), "params": rows })
}

#[test]
#[ignore = "needs scripts/build-multiq.sh and VIZDAW_BUNDLED_PLUGINS"]
fn bundled_multiq_dumps_its_parameters() {
    if !temp_home::in_temp_home("bundled_multiq_dumps_its_parameters") {
        return;
    }
    let inst = load();
    let live = table(&inst.info().params);
    match std::env::var_os("VIZDAW_MULTIQ_DUMP") {
        Some(path) if !path.is_empty() => {
            let text = serde_json::to_string_pretty(&live).unwrap() + "\n";
            std::fs::write(&path, text).unwrap();
        }
        _ => {
            let path =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/multiq-params.json");
            let text = std::fs::read_to_string(&path).unwrap();
            let committed: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                live,
                committed,
                "the live parameters differ from {}",
                path.display()
            );
        }
    }
}
