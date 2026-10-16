//! The catalog: cache keys, blocklist, enabled state, missing plugins and
//! what the host and the plugin browser get from it.

mod common;

use common::plugin;
use core_host::catalog::{Catalog, CatalogEntry, PluginStatus, ScanStatus, Source};
use core_host::{PluginHost, PluginKind};
use core_model::mixer::PluginFormat;
use std::path::PathBuf;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vizdaw-catalog-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn source(path: &str, mtime: u64) -> Source {
    Source {
        format: PluginFormat::Clap,
        path: PathBuf::from(path),
        version: "1.0".into(),
        mtime,
    }
}

fn entry(src: &Source, id: &str, kind: PluginKind, status: ScanStatus) -> CatalogEntry {
    let mut p = plugin(id);
    p.name = id.into();
    CatalogEntry {
        source: src.clone(),
        plugin: p,
        kind: Some(kind),
        info: None,
        status,
    }
}

fn sample() -> (Catalog, Source, Source) {
    let a = source("/p/A.clap", 10);
    let b = source("/p/B.clap", 20);
    let mut c = Catalog::new();
    c.record(
        &a,
        vec![
            entry(&a, "a.synth", PluginKind::Instrument, ScanStatus::Ok),
            entry(&a, "a.fx", PluginKind::Effect, ScanStatus::Ok),
        ],
    );
    c.record(
        &b,
        vec![entry(
            &b,
            "b.crash",
            PluginKind::Effect,
            ScanStatus::Blocklisted {
                reason: "crashed (signal 11)".into(),
            },
        )],
    );
    (c, a, b)
}

#[test]
fn saves_and_loads_and_reports_corrupt_files() {
    let dir = temp("io");
    let path = dir.join("sub/plugins.json");
    assert_eq!(
        Catalog::load(&path).unwrap(),
        Catalog::new(),
        "missing is empty"
    );
    let (mut c, _, _) = sample();
    c.set_enabled(PluginFormat::Clap, "a.fx", false);
    c.save(&path).unwrap();
    let back = Catalog::load(&path).unwrap();
    assert_eq!(back.entries(), c.entries());
    assert_eq!(
        back.status(PluginFormat::Clap, "a.fx"),
        Some(PluginStatus::Disabled)
    );
    std::fs::write(&path, "{ not json").unwrap();
    assert!(Catalog::load(&path).is_err());
}

#[test]
fn only_new_or_changed_sources_are_scanned_and_gone_ones_are_missing() {
    let (mut c, a, b) = sample();
    let new = source("/p/C.clap", 5);
    let touched = source("/p/A.clap", 11);
    let todo = c.mark_found(&[touched.clone(), new.clone()]);
    assert_eq!(todo, vec![touched, new], "A changed, C is new");
    assert!(
        c.get(PluginFormat::Clap, "a.synth").is_none(),
        "A's old scan is dropped"
    );
    assert_eq!(
        c.status(PluginFormat::Clap, "b.crash"),
        Some(PluginStatus::Missing),
        "B was not found"
    );
    // Found again unchanged: nothing to scan.
    let (mut c, _, _) = sample();
    assert!(c.mark_found(&[a, b]).is_empty());
    assert_eq!(
        c.status(PluginFormat::Clap, "a.synth"),
        Some(PluginStatus::Ok)
    );
}

#[test]
fn statuses_browser_and_clearing_the_blocklist() {
    let (mut c, _, b) = sample();
    assert_eq!(
        c.status(PluginFormat::Clap, "b.crash"),
        Some(PluginStatus::Blocklisted("crashed (signal 11)".into()))
    );
    c.set_enabled(PluginFormat::Clap, "a.fx", false);
    let names = |rows: Vec<core_host::catalog::CatalogRow>| -> Vec<String> {
        rows.into_iter().map(|r| r.plugin.id).collect()
    };
    assert_eq!(names(c.rows()), ["a.fx", "a.synth", "b.crash"]);
    assert_eq!(names(c.browser(PluginKind::Effect)), Vec::<String>::new());
    assert_eq!(names(c.browser(PluginKind::Instrument)), ["a.synth"]);
    c.set_enabled(PluginFormat::Clap, "a.fx", true);
    assert_eq!(names(c.browser(PluginKind::Effect)), ["a.fx"]);
    assert_eq!(c.clear_blocklist(PluginFormat::Clap, "b.crash"), Some(b));
    assert!(
        c.get(PluginFormat::Clap, "b.crash").is_none(),
        "rescan pending"
    );
    assert_eq!(c.clear_blocklist(PluginFormat::Clap, "a.fx"), None);
}

#[test]
fn the_host_learns_paths_and_unavailable_plugins() {
    let bundle = test_plugins::bundle();
    let src = Source::clap_bundle(&bundle);
    assert_eq!(src.version, "1.0.0");
    assert!(src.mtime > 0);
    let mut c = Catalog::new();
    c.record(
        &src,
        vec![
            entry(&src, "vizdaw.test.gain", PluginKind::Effect, ScanStatus::Ok),
            entry(&src, "vizdaw.test.nan", PluginKind::Effect, ScanStatus::Ok),
        ],
    );
    c.set_enabled(PluginFormat::Clap, "vizdaw.test.nan", false);
    let mut host = PluginHost::new(48_000.0, 512).unwrap();
    c.apply(&mut host);
    let mut b = core_engine::fixtures::Builder::new();
    let (_, gain) = b.plugin_dest("G", plugin("vizdaw.test.gain")).unwrap();
    let (_, nan) = b.plugin_dest("N", plugin("vizdaw.test.nan")).unwrap();
    host.sync(b.doc());
    host.wait_ready(std::time::Duration::from_secs(10));
    assert!(host.instance(gain).is_some(), "{:?}", host.failure(gain));
    assert_eq!(host.failure(nan), Some("Disabled in Plugin Manager"));
}

#[test]
fn newer_files_are_untouched_and_corrupt_ones_are_moved_aside() {
    let dir = temp("guard");
    let path = dir.join("plugins.json");
    let newer = r#"{"version":99,"future":true}"#;
    std::fs::write(&path, newer).unwrap();
    assert!(Catalog::load(&path).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);
    let (c, _, _) = sample();
    assert!(c.save(&path).is_err(), "save refuses to overwrite");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);

    std::fs::write(&path, "{ not json").unwrap();
    assert!(Catalog::load(&path).is_err());
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("plugins.json.corrupt")).unwrap(),
        "{ not json"
    );
    c.save(&path).unwrap();
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "no tmp left");
}

#[test]
fn apply_clears_only_its_own_reasons() {
    let bundle = test_plugins::bundle();
    let src = Source::clap_bundle(&bundle);
    let mut c = Catalog::new();
    c.record(
        &src,
        vec![entry(
            &src,
            "vizdaw.test.nan",
            PluginKind::Effect,
            ScanStatus::Ok,
        )],
    );
    c.set_enabled(PluginFormat::Clap, "vizdaw.test.nan", false);
    let mut host = PluginHost::new(48_000.0, 512).unwrap();
    c.apply(&mut host);
    let mut b = core_engine::fixtures::Builder::new();
    let (_, nan) = b.plugin_dest("N", plugin("vizdaw.test.nan")).unwrap();
    host.sync(b.doc());
    host.wait_ready(std::time::Duration::from_secs(10));
    assert_eq!(host.failure(nan), Some("Disabled in Plugin Manager"));
    c.set_enabled(PluginFormat::Clap, "vizdaw.test.nan", true);
    c.apply(&mut host);
    host.sync(b.doc());
    host.wait_ready(std::time::Duration::from_secs(10));
    assert!(host.instance(nan).is_some(), "{:?}", host.failure(nan));

    // A plugin the catalog never marked keeps the host's own failure.
    let mut host = PluginHost::new(48_000.0, 512).unwrap();
    let mut c = Catalog::new();
    c.record(
        &src,
        vec![entry(
            &src,
            "vizdaw.test.gain",
            PluginKind::Effect,
            ScanStatus::Ok,
        )],
    );
    let mut b = core_engine::fixtures::Builder::new();
    let (_, ghost) = b.plugin_dest("X", plugin("vizdaw.test.unknown")).unwrap();
    host.sync(b.doc());
    host.wait_ready(std::time::Duration::from_secs(10));
    let before = host.failure(ghost).map(str::to_string);
    assert!(before.is_some());
    c.apply(&mut host);
    assert_eq!(host.failure(ghost).map(str::to_string), before);
}
