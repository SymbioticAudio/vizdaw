//! The plugin catalog through the FFI (spec §8, §9): scanning in the
//! background with the real scanner against the in-repo test bundle, the
//! Plugin Manager's statuses and actions, the browser's lists, disabling a
//! plugin a project uses, and catalog files the app must not overwrite.
//! Never touches the user's real catalog: every test has its own folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    CatalogConfig, CatalogRowFfi, PluginCatalog, PluginFormatTag, PluginKey, PluginKindTag,
    PluginRefRow, PluginStatusRow, Session, SlotStatus,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

/// A folder with a copy of the test bundle in a `CLAP` subfolder.
fn plugins_dir() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let clap = d.path().join("CLAP");
    std::fs::create_dir_all(&clap).unwrap();
    let bundle = test_plugins::bundle();
    copy_dir(&bundle, &clap.join(bundle.file_name().unwrap()));
    (d, clap)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

fn config(dir: &Path, clap: &Path) -> CatalogConfig {
    CatalogConfig {
        path: dir.join("plugins.json").to_string_lossy().into_owned(),
        scanner: test_plugins::scanner().to_string_lossy().into_owned(),
        clap_dirs: vec![clap.to_string_lossy().into_owned()],
        include_au: false,
        timeout_ms: Some(2_000),
    }
}

fn wait_scan(c: &PluginCatalog) {
    let t = Instant::now();
    while c.progress().unwrap().running {
        assert!(t.elapsed() < Duration::from_secs(60), "scan never ended");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn status(c: &PluginCatalog, id: &str) -> PluginStatusRow {
    c.rows()
        .unwrap()
        .into_iter()
        .find(|r| r.plugin.id == id)
        .map(|r| r.status)
        .unwrap_or_else(|| panic!("{id} not in the catalog"))
}

fn key(id: &str) -> PluginKey {
    PluginKey {
        format: PluginFormatTag::Clap,
        id: id.into(),
    }
}

fn scanned() -> (tempfile::TempDir, Arc<PluginCatalog>) {
    let (d, clap) = plugins_dir();
    let c = PluginCatalog::open(config(d.path(), &clap)).unwrap();
    assert!(
        c.take_problem().unwrap().is_none(),
        "a missing file is an empty catalog"
    );
    assert!(c.rows().unwrap().is_empty());
    c.scan(false).unwrap();
    wait_scan(&c);
    (d, c)
}

#[test]
fn a_scan_fills_the_manager_and_blocklists_crashers() {
    let _g = serial();
    let (d, c) = scanned();
    assert_eq!(status(&c, "vizdaw.test.gain"), PluginStatusRow::Ok);
    assert_eq!(
        status(&c, "vizdaw.test.crasher"),
        PluginStatusRow::Blocklisted {
            reason: "crashed (signal 11)".into()
        }
    );
    assert_eq!(
        status(&c, "vizdaw.test.hanger"),
        PluginStatusRow::Blocklisted {
            reason: "timed out".into()
        }
    );
    let p = c.progress().unwrap();
    assert_eq!((p.done, p.total), (1, 1), "one bundle");
    // The browser lists usable plugins of one kind.
    let ids = |rows: Vec<CatalogRowFfi>| -> Vec<String> {
        rows.into_iter().map(|r| r.plugin.id).collect()
    };
    let fx = ids(c.browser(PluginKindTag::Effect).unwrap());
    assert!(fx.contains(&"vizdaw.test.gain".to_string()));
    assert!(!fx.contains(&"vizdaw.test.crasher".to_string()));
    assert_eq!(
        ids(c.browser(PluginKindTag::Instrument).unwrap()),
        ["vizdaw.test.synth4"]
    );
    // Saved: a new catalog reads the same.
    let again = PluginCatalog::open(config(d.path(), Path::new("/nonexistent"))).unwrap();
    assert_eq!(again.rows().unwrap(), c.rows().unwrap());
    // Unchanged bundles are not scanned again.
    again.scan(false).unwrap();
    assert!(!again.progress().unwrap().running);
    assert!(
        c.reveal_path(key("vizdaw.test.gain"))
            .unwrap()
            .unwrap()
            .ends_with(".clap")
    );
}

#[test]
fn disabling_a_plugin_makes_its_slots_placeholders_until_enabled() {
    let _g = serial();
    let (_d, c) = scanned();
    c.set_enabled(key("vizdaw.test.gain"), false).unwrap();
    assert_eq!(status(&c, "vizdaw.test.gain"), PluginStatusRow::Disabled);
    assert!(
        !c.browser(PluginKindTag::Effect)
            .unwrap()
            .iter()
            .any(|r| r.plugin.id == "vizdaw.test.gain")
    );
    let s = Session::new_demo().unwrap();
    let synth = s.mixer().unwrap().strips[0].id;
    s.add_insert(
        synth,
        None,
        PluginRefRow {
            format: PluginFormatTag::Clap,
            id: "vizdaw.test.gain".into(),
            name: "Test Gain".into(),
            vendor: "VizDAW".into(),
            version: "1.0.0".into(),
        },
    )
    .unwrap();
    let slot = |s: &Session| s.plugin_slots().unwrap()[0].status.clone();
    assert_eq!(
        slot(&s),
        SlotStatus::Placeholder {
            reason: "Disabled in Plugin Manager".into()
        }
    );
    c.set_enabled(key("vizdaw.test.gain"), true).unwrap();
    let t = Instant::now();
    while slot(&s) != SlotStatus::Live {
        assert!(t.elapsed() < Duration::from_secs(10), "{:?}", slot(&s));
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn clearing_a_blocklist_entry_scans_the_bundle_again() {
    let _g = serial();
    let (_d, c) = scanned();
    c.clear_blocklist(key("vizdaw.test.crasher")).unwrap();
    wait_scan(&c);
    // It crashes again, so it is blocklisted again; the others are back.
    assert!(matches!(
        status(&c, "vizdaw.test.crasher"),
        PluginStatusRow::Blocklisted { .. }
    ));
    assert_eq!(status(&c, "vizdaw.test.gain"), PluginStatusRow::Ok);
    c.rescan(vec![key("vizdaw.test.gain")]).unwrap();
    wait_scan(&c);
    assert_eq!(status(&c, "vizdaw.test.gain"), PluginStatusRow::Ok);
}

#[test]
fn a_newer_catalog_is_reported_and_never_overwritten() {
    let _g = serial();
    let (d, clap) = plugins_dir();
    let path = d.path().join("plugins.json");
    let newer = r#"{"version":99,"entries":[],"disabled":[]}"#;
    std::fs::write(&path, newer).unwrap();
    let c = PluginCatalog::open(config(d.path(), &clap)).unwrap();
    let p = c.take_problem().unwrap().unwrap();
    assert!(p.contains("newer"), "{p}");
    assert!(c.take_problem().unwrap().is_none(), "shown once");
    c.set_enabled(key("vizdaw.test.gain"), false).unwrap();
    let p = c.take_problem().unwrap().unwrap();
    assert!(p.contains("not overwriting"), "{p}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);
}

#[test]
fn a_corrupt_catalog_is_moved_aside_and_reported() {
    let _g = serial();
    let (d, clap) = plugins_dir();
    let path = d.path().join("plugins.json");
    std::fs::write(&path, "{ not json").unwrap();
    let c = PluginCatalog::open(config(d.path(), &clap)).unwrap();
    assert!(c.take_problem().unwrap().unwrap().contains(".corrupt"));
    assert!(d.path().join("plugins.json.corrupt").exists());
    assert!(c.rows().unwrap().is_empty());
    // A missing scanner is an error, not a hang.
    let mut cfg = config(d.path(), &clap);
    cfg.scanner = "/nonexistent/vizdaw-scan".into();
    let c = PluginCatalog::open(cfg).unwrap();
    assert!(c.scan(true).is_err());
    assert!(!c.progress().unwrap().running);
}
