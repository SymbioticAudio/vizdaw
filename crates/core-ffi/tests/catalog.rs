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
    // Saved: a new catalog reads the same (open replaces the global one,
    // so compare with a snapshot taken before).
    let before = c.rows().unwrap();
    let again = PluginCatalog::open(config(d.path(), Path::new("/nonexistent"))).unwrap();
    assert_eq!(again.rows().unwrap(), before);
    assert!(!before.is_empty());
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
    c.clear_blocklist(vec![key("vizdaw.test.crasher")]).unwrap();
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
fn a_batch_clear_scans_once_and_a_blocklisted_plugin_can_be_switched_off() {
    let _g = serial();
    let (_d, c) = scanned();
    let enabled = |id: &str| {
        c.rows()
            .unwrap()
            .into_iter()
            .find(|r| r.plugin.id == id)
            .unwrap()
            .enabled
    };
    // Off stays off for a blocklisted plugin, and on again works.
    c.set_enabled(key("vizdaw.test.crasher"), false).unwrap();
    assert!(!enabled("vizdaw.test.crasher"));
    assert!(matches!(
        status(&c, "vizdaw.test.crasher"),
        PluginStatusRow::Blocklisted { .. }
    ));
    assert!(enabled("vizdaw.test.gain"));
    c.set_enabled(key("vizdaw.test.crasher"), true).unwrap();
    assert!(enabled("vizdaw.test.crasher"));
    // Several keys (one twice, one not blocklisted) start one scan.
    c.clear_blocklist(vec![
        key("vizdaw.test.crasher"),
        key("vizdaw.test.crasher"),
        key("vizdaw.test.gain"),
    ])
    .unwrap();
    assert!(c.progress().unwrap().running);
    // A second clear while it runs fails up front.
    assert!(c.clear_blocklist(vec![key("vizdaw.test.crasher")]).is_err());
    wait_scan(&c);
    assert!(matches!(
        status(&c, "vizdaw.test.crasher"),
        PluginStatusRow::Blocklisted { .. }
    ));
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

// ---------------------------------------------------------------------------
// Turning a plugin off unloads it from every open project at once; its
// settings are kept (Task 15).

/// A catalog in its own folder knowing the test bundle (no scan).
fn known() -> (tempfile::TempDir, Arc<PluginCatalog>) {
    let (d, clap) = plugins_dir();
    let c = PluginCatalog::open(config(d.path(), &clap)).unwrap();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    (d, c)
}

/// The demo with a Test Gain insert: (session, slot).
fn demo_with_gain() -> (Arc<Session>, u64) {
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
    let id = s.plugin_slots().unwrap()[0].id;
    (s, id)
}

fn slot_status(s: &Session, id: u64) -> SlotStatus {
    s.plugin_slots()
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap()
        .status
}

fn idle_until(sessions: &[&Session], mut done: impl FnMut() -> bool) {
    let t = Instant::now();
    while !done() {
        assert!(t.elapsed() < Duration::from_secs(10), "timed out");
        for s in sessions {
            s.idle().unwrap();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn gain_value(s: &Session, id: u64) -> Option<f64> {
    s.params(id)
        .unwrap()
        .into_iter()
        .find(|p| p.id == 0)
        .map(|p| p.value)
}

const DISABLED: &str = "Disabled in Plugin Manager";

fn disabled() -> SlotStatus {
    SlotStatus::Placeholder {
        reason: DISABLED.into(),
    }
}

#[test]
fn disabling_a_plugin_unloads_it_from_every_open_project_and_keeps_its_settings() {
    let _g = serial();
    let (d, c) = known();
    let path = d.path().join("p.vizdaw");
    let (a, ia) = demo_with_gain();
    a.save_as(path.to_string_lossy().into_owned()).unwrap();
    let (b, ib) = demo_with_gain();
    let song = a
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id;
    a.play(song).unwrap();
    idle_until(&[&a, &b], || {
        slot_status(&a, ia) == SlotStatus::Live
            && slot_status(&b, ib) == SlotStatus::Live
            && a.transport().unwrap().report.is_empty()
    });
    // A setting only the plugin holds (a control not committed).
    a.set_param(ia, 0, 0.25).unwrap();
    idle_until(&[&a, &b], || gain_value(&a, ia) == Some(0.25));
    a.save().unwrap();
    let project = path.join(core_model::file::PROJECT_FILE);
    let saved = std::fs::read_to_string(&project).unwrap();
    let undo = |s: &Session| s.snapshot().unwrap().undo_label;
    let (undo_a, undo_b) = (undo(&a), undo(&b));
    let loads = vizdaw_ffi::host::loads();

    c.set_enabled(key("vizdaw.test.gain"), false).unwrap();
    // The next timer step of each project unloads it.
    a.idle().unwrap();
    b.idle().unwrap();
    assert_eq!(slot_status(&a, ia), disabled());
    assert_eq!(slot_status(&b, ib), disabled());
    assert!(a.params(ia).unwrap().is_empty(), "no instance");
    assert!(vizdaw_ffi::host::loads() > loads, "the engine reloaded");
    let t = a.transport().unwrap();
    assert!(
        t.report.iter().any(|l| l.contains(DISABLED)),
        "{:?}",
        t.report
    );
    assert!(t.faults.is_empty(), "{:?}", t.faults);
    // No undo step, and the project is the one saved: its state was
    // captured, nothing else changed.
    assert_eq!((undo(&a), undo(&b)), (undo_a.clone(), undo_b.clone()));
    assert!(a.debug_slot_state(ia).unwrap().is_some());
    assert!(b.debug_slot_state(ib).unwrap().is_some());
    assert!(!a.snapshot().unwrap().dirty, "same state as saved");
    a.save().unwrap();
    assert_eq!(std::fs::read_to_string(&project).unwrap(), saved);

    // Enabled again: both load at once, with their settings.
    c.set_enabled(key("vizdaw.test.gain"), true).unwrap();
    idle_until(&[&a, &b], || {
        slot_status(&a, ia) == SlotStatus::Live && slot_status(&b, ib) == SlotStatus::Live
    });
    assert_eq!(gain_value(&a, ia), Some(0.25));
    idle_until(&[&a, &b], || a.transport().unwrap().report.is_empty());
    assert_eq!((undo(&a), undo(&b)), (undo_a, undo_b));
}

#[test]
fn a_plugin_disabled_while_the_registry_is_busy_is_unloaded_on_the_next_idle() {
    let _g = serial();
    let (_d, c) = known();
    let (s, id) = demo_with_gain();
    idle_until(&[&s], || slot_status(&s, id) == SlotStatus::Live);
    // A nested run loop inside a plugin call: the change waits.
    vizdaw_ffi::plugins::with(0, |_| {
        c.set_enabled(key("vizdaw.test.gain"), false).unwrap();
        assert!(s.idle().unwrap().problems.is_empty(), "skipped");
    })
    .unwrap();
    assert_eq!(slot_status(&s, id), SlotStatus::Live, "not applied yet");
    let u = s.idle().unwrap();
    assert!(u.slots_changed);
    assert_eq!(slot_status(&s, id), disabled());
    assert!(s.snapshot().unwrap().undo_label.is_some(), "only the add");
    s.undo().unwrap();
    assert!(
        s.snapshot().unwrap().undo_label.is_none(),
        "no step of its own"
    );
    c.set_enabled(key("vizdaw.test.gain"), true).unwrap();
}
