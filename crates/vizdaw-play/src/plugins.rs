//! Plugins for `vizdaw-play`: the catalog (`--scan`, `--plugins`), command
//! line edits that add plugins (`--insert`, `--instrument`) and the plugin
//! host for live playback.

use crate::args::Args;
use core_host::PluginHost;
use core_host::catalog::{Catalog, PluginStatus, Source};
use core_host::scan;
use core_model::command::{Command, MixerCmd, RoutingCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::ids::PluginInstanceId;
use core_model::mixer::{PluginFormat, PluginRef, PluginSlot};
use std::path::PathBuf;
use std::time::Duration;

fn catalog_path(args: &Args) -> PathBuf {
    args.catalog.clone().unwrap_or_else(Catalog::default_path)
}

/// Loads the catalog. A corrupt file has been moved to `plugins.json.corrupt`
/// and a newer one is left untouched; either is an error the caller reports
/// (nothing here ever saves over a file it could not read).
pub fn load_catalog(args: &Args) -> Result<Catalog, String> {
    Catalog::load(&catalog_path(args))
}

/// The catalog for playback: a catalog that cannot be read is reported
/// clearly and playback goes on with no catalog entries (it is not saved).
pub fn load_catalog_or_empty(args: &Args) -> Catalog {
    load_catalog(args).unwrap_or_else(|e| {
        eprintln!("error: plugin catalog not loaded: {e}");
        eprintln!("continuing without catalog entries; the catalog file will not be saved");
        Catalog::new()
    })
}

/// The scanner binary: `--scanner`, else next to this one.
fn scanner(args: &Args) -> Result<PathBuf, String> {
    if let Some(p) = &args.scanner {
        return Ok(p.clone());
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = me
        .parent()
        .map(|d| d.join("vizdaw-scan"))
        .ok_or("cannot find vizdaw-scan")?;
    if exe.exists() {
        Ok(exe)
    } else {
        Err(format!(
            "{} not found (build it with `cargo build -p vizdaw-scan`)",
            exe.display()
        ))
    }
}

fn discover(args: &Args) -> Vec<Source> {
    let dirs = if args.clap_dirs.is_empty() {
        core_host::clap::search_paths()
    } else {
        args.clap_dirs.clone()
    };
    let mut found: Vec<Source> = core_host::clap::find_bundles(&dirs)
        .iter()
        .map(|p| Source::clap_bundle(p))
        .collect();
    #[cfg(target_os = "macos")]
    if !args.no_au {
        found.extend(platform_macos::au::sources());
    }
    found
}

/// The paths of the installed AU v3 components (30 s to scan; v2 and CLAP
/// get 10 s).
fn v3_sources() -> std::collections::HashSet<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        platform_macos::au::components()
            .into_iter()
            .filter(|c| c.v3)
            .map(|c| PathBuf::from(format!("au:{}", c.plugin.id)))
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Default::default()
    }
}

fn timeout_for(s: &Source, v3: &std::collections::HashSet<PathBuf>) -> Duration {
    if s.format == PluginFormat::Au && v3.contains(&s.path) {
        scan::TIMEOUT_AU_V3
    } else {
        scan::TIMEOUT
    }
}

/// `--scan` and `--plugins`.
pub fn run(args: &Args) -> Result<(), String> {
    let mut cat = load_catalog(args)?;
    if args.scan {
        let exe = scanner(args)?;
        let todo = cat.mark_found(&discover(args));
        eprintln!("scanning {} source(s)…", todo.len());
        let fixed = args.scan_timeout_ms.map(Duration::from_millis);
        let v3 = v3_sources();
        scan::scan_all(&mut cat, &exe, &todo, |s: &Source| {
            fixed.unwrap_or_else(|| timeout_for(s, &v3))
        });
        cat.save(&catalog_path(args))?;
    }
    for r in cat.rows() {
        let status = match &r.status {
            PluginStatus::Ok => "OK".to_string(),
            PluginStatus::Disabled => "Disabled".to_string(),
            PluginStatus::Blocklisted(why) => format!("Blocklisted: {why}"),
            PluginStatus::NotValidated => "Not validated".to_string(),
            PluginStatus::Missing => "Missing".to_string(),
        };
        let format = match r.plugin.format {
            PluginFormat::Au => "au",
            PluginFormat::Clap => "clap",
        };
        println!(
            "{format}:{}  {} ({})  {status}",
            r.plugin.id, r.plugin.name, r.plugin.vendor
        );
    }
    Ok(())
}

/// A `FORMAT:ID` plugin, named from the catalog when it knows it.
fn plugin_ref(spec: &str, cat: &Catalog) -> Result<PluginRef, String> {
    let (format, id) = match spec.split_once(':') {
        Some(("au", id)) => (PluginFormat::Au, id),
        Some(("clap", id)) => (PluginFormat::Clap, id),
        _ => return Err(format!("{spec:?}: expected au:<id> or clap:<id>")),
    };
    Ok(cat
        .get(format, id)
        .map(|e| e.plugin.clone())
        .unwrap_or_else(|| PluginRef {
            format,
            id: id.to_string(),
            name: id.to_string(),
            vendor: String::new(),
            version: String::new(),
        }))
}

/// Applies `--insert` and `--instrument` to `doc`.
pub fn edit(doc: &Document, args: &Args, cat: &Catalog) -> Result<Document, String> {
    let mut doc = doc.clone();
    let run = |doc: &Document, cmd: Command| -> Result<Document, String> {
        apply(doc, &cmd).map(|a| a.doc).map_err(|e| e.to_string())
    };
    for spec in &args.instruments {
        let (name, p) = spec
            .split_once('=')
            .ok_or_else(|| format!("{spec:?}: expected DEST=FORMAT:ID"))?;
        let dest = doc
            .instruments()
            .values()
            .find(|d| d.name == name)
            .ok_or_else(|| format!("no destination named {name:?}"))?;
        let kind = DestKind::PluginInstrument {
            slot: PluginSlot::new(PluginInstanceId(0), plugin_ref(p, cat)?),
        };
        let cmd = Command::Routing(RoutingCmd::EditDestination {
            dest: dest.id,
            kind,
            default_patch: dest.default_patch,
        });
        doc = run(&doc, cmd)?;
    }
    for spec in &args.inserts {
        let (name, p) = spec
            .split_once('=')
            .ok_or_else(|| format!("{spec:?}: expected STRIP=FORMAT:ID"))?;
        let strip = doc
            .mixer()
            .strips()
            .values()
            .find(|s| s.name == name)
            .ok_or_else(|| format!("no strip named {name:?}"))?
            .id;
        let cmd = Command::Mixer(MixerCmd::AddInsert {
            strip,
            index: None,
            plugin: plugin_ref(p, cat)?,
        });
        doc = run(&doc, cmd)?;
    }
    Ok(doc)
}

/// A host for `doc`'s plugins, loaded (or failed) before playback starts.
pub fn host(
    doc: &Document,
    cat: &mut Catalog,
    rate: f64,
    max_frames: u32,
) -> Result<PluginHost, String> {
    let mut h = PluginHost::new(rate, max_frames).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    h.add_format(Box::new(platform_macos::au::AuFormat::new()));
    cat.apply(&mut h);
    h.sync(doc);
    let r = h.wait_ready(Duration::from_secs(60));
    for (id, why) in &r.failed {
        eprintln!("warning: plugin {id}: {why}");
    }
    for (id, why) in &r.rejected_state {
        eprintln!("warning: plugin {id}: saved state rejected ({why})");
    }
    Ok(h)
}
