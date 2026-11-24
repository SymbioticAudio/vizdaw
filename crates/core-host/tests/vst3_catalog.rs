//! VST3 entries in the plug-in catalogue (host track H1 Task 2): they live
//! in `plugins.json` beside CLAP and AU ones, and the host learns their
//! bundle paths.

use core_host::catalog::{Catalog, CatalogEntry, ScanStatus, Source};
use core_host::{PluginHost, PluginKind};
use core_model::mixer::{PluginFormat, PluginRef};
use std::path::{Path, PathBuf};

const VST3_ID: &str = "56495A44565354337E570000ABCD0001";

fn source(format: PluginFormat, path: &str) -> Source {
    Source {
        format,
        path: PathBuf::from(path),
        version: "1.0".into(),
        mtime: 10,
    }
}

fn entry(src: &Source, id: &str) -> CatalogEntry {
    CatalogEntry {
        source: src.clone(),
        plugin: PluginRef {
            format: src.format,
            id: id.into(),
            name: id.into(),
            vendor: "V".into(),
            version: "1.0".into(),
        },
        kind: Some(PluginKind::Effect),
        info: None,
        status: ScanStatus::NotValidated,
    }
}

/// A catalogue with a CLAP and a VST3 plug-in, one of each disabled.
fn mixed() -> Catalog {
    let clap = source(PluginFormat::Clap, "/p/A.clap");
    let vst3 = source(PluginFormat::Vst3, "/p/B.vst3");
    let mut c = Catalog::new();
    c.record(&clap, vec![entry(&clap, "a.fx"), entry(&clap, "a.off")]);
    c.record(
        &vst3,
        vec![entry(&vst3, VST3_ID), entry(&vst3, &"0".repeat(32))],
    );
    c.set_enabled(PluginFormat::Clap, "a.off", false);
    c.set_enabled(PluginFormat::Vst3, &"0".repeat(32), false);
    c
}

#[test]
fn vst3_entries_save_and_load_back() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("plugins.json");
    let c = mixed();
    c.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains(VST3_ID) && text.contains("\"vst3\""),
        "{text}"
    );
    let back = Catalog::load(&path).unwrap();
    assert_eq!(back.rows(), c.rows());
    assert!(back.get(PluginFormat::Vst3, VST3_ID).is_some());
    assert!(
        !back
            .rows()
            .iter()
            .any(|r| r.plugin.id == "0".repeat(32) && r.enabled)
    );
}

#[test]
fn the_host_learns_where_vst3_plugins_live() {
    let c = mixed();
    let mut host = PluginHost::new(48_000.0, 512).unwrap();
    let mut marks = Default::default();
    c.apply_to(&mut host, &mut marks);
    assert_eq!(
        host.path_of(PluginFormat::Vst3, VST3_ID),
        Some(Path::new("/p/B.vst3"))
    );
    assert_eq!(
        host.path_of(PluginFormat::Clap, "a.fx"),
        Some(Path::new("/p/A.clap"))
    );
}
