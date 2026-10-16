//! `vizdaw-scan`: checks one plugin in its own process (spec §8).
//!
//! ```text
//! vizdaw-scan --format clap --path <bundle> --list
//! vizdaw-scan --format clap --path <bundle> --id <plugin id>
//! vizdaw-scan --format au --path au:<type/subtype/manufacturer> --list
//! vizdaw-scan --format au --id <type/subtype/manufacturer>
//! ```
//!
//! `--list` prints the plugins of the bundle; `--id` loads the plugin,
//! activates it at 48 kHz / 512 frames, processes one silent block,
//! deactivates it and prints its `PluginInfo`. The reply is one JSON line
//! on stdout ([`core_host::scan::ScanReply`]); failures print a message on
//! stderr and exit with status 2. A crash or a hang is the caller's to
//! detect.

use core_engine::backend::BlockInfo;
use core_host::clap::{ClapFormat, list_bundle};
use core_host::scan::ScanReply;
use core_host::{LoadRequest, ParamOut, PluginFormat, PluginInstance, ProcessCtx};
use core_model::mixer::{PluginFormat as Format, PluginRef};
use std::path::PathBuf;

/// Sample rate and block size of the check.
const RATE: f64 = 48_000.0;
const BLOCK: u32 = 512;

struct Args {
    format: Format,
    path: PathBuf,
    id: Option<String>,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let (mut format, mut path, mut id, mut list) = (None, None, None, false);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--format" => {
                format = Some(match it.next().as_deref() {
                    Some("clap") => Format::Clap,
                    Some("au") => Format::Au,
                    other => return Err(format!("unknown format {other:?}")),
                })
            }
            "--path" => path = it.next().map(PathBuf::from),
            "--id" => id = it.next(),
            "--list" => list = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let format = format.ok_or("--format is required")?;
    if list == id.is_some() {
        return Err("give exactly one of --list and --id".into());
    }
    Ok(Args {
        format,
        path: path.unwrap_or_default(),
        id,
    })
}

/// Activates `inst`, processes one silent block and deactivates it.
fn exercise(inst: &mut dyn PluginInstance) -> Result<(), String> {
    let mut proc = inst.activate(RATE, BLOCK).map_err(|e| e.to_string())?;
    let info = inst.info().clone();
    let bufs = |n: usize| -> Vec<Box<[f32]>> {
        (0..n)
            .map(|_| vec![0.0f32; BLOCK as usize].into_boxed_slice())
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
    let (mut input, mut sidechain, mut outputs) =
        (bufs(main), bufs(sc), bufs(info.output_channels()));
    let mut out = ParamOut::default();
    let transport = BlockInfo::default();
    let mut ctx = ProcessCtx {
        frames: BLOCK,
        input: &mut input,
        sidechain: &mut sidechain,
        outputs: &mut outputs,
        events: &[],
        params_in: &[],
        params_out: &mut out,
        transport: &transport,
    };
    let status = proc.process(&mut ctx);
    inst.deactivate(proc);
    match status {
        core_engine::graph::ProcessStatus::Ok => Ok(()),
        core_engine::graph::ProcessStatus::Error => Err("processing failed".into()),
    }
}

fn scan(args: &Args) -> Result<ScanReply, String> {
    match (args.format, &args.id) {
        (Format::Clap, None) => list_bundle(&args.path)
            .map(ScanReply::Listed)
            .map_err(|e| e.to_string()),
        (Format::Clap, Some(id)) => {
            let mut f = ClapFormat::new();
            let req = LoadRequest {
                plugin: PluginRef {
                    format: Format::Clap,
                    id: id.clone(),
                    name: id.clone(),
                    vendor: String::new(),
                    version: String::new(),
                },
                path: Some(args.path.clone()),
                sample_rate: RATE,
                max_frames: BLOCK,
            };
            let prepared = (f.prepare(&req))().map_err(|e| e.to_string())?;
            let mut inst = f.finish(&req, prepared).map_err(|e| e.to_string())?;
            exercise(inst.as_mut())?;
            Ok(ScanReply::Scanned(inst.info().clone()))
        }
        (Format::Au, _) => au::scan(args),
    }
}

#[cfg(target_os = "macos")]
mod au {
    use super::*;
    use platform_macos::au;

    /// AU sources are single components: `--list` reads the registry only;
    /// `--id` loads the unit. A v3 unit that cannot be loaded outside the
    /// app is "not validated" (spec §8).
    pub fn scan(args: &Args) -> Result<ScanReply, String> {
        let id = match &args.id {
            Some(id) => id.clone(),
            None => args
                .path
                .to_string_lossy()
                .strip_prefix("au:")
                .map(String::from)
                .ok_or("an AU path is au:<type/subtype/manufacturer>")?,
        };
        let c = au::component(&id).ok_or_else(|| format!("plugin not found: {id}"))?;
        if args.id.is_none() {
            return Ok(ScanReply::Listed(vec![(c.plugin, c.kind)]));
        }
        match au::load(&id, RATE, BLOCK) {
            Ok(mut inst) => {
                exercise(&mut inst)?;
                Ok(ScanReply::Scanned(inst.info().clone()))
            }
            Err(e) if c.v3 => Ok(ScanReply::NotValidated(e.to_string())),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod au {
    use super::*;
    pub fn scan(_: &Args) -> Result<ScanReply, String> {
        Err("Audio Units exist only on macOS".into())
    }
}

fn main() {
    let result = parse(std::env::args().skip(1)).and_then(|a| scan(&a));
    match result.and_then(|r| serde_json::to_string(&r).map_err(|e| e.to_string())) {
        Ok(line) => println!("{line}"),
        Err(e) => {
            eprintln!("vizdaw-scan: {e}");
            std::process::exit(2);
        }
    }
}
