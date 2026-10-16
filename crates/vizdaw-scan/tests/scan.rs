//! The scanner in its own process: listing, scanning, crashes, hangs and
//! the catalog cache.

use core_host::catalog::{Catalog, PluginStatus, Source};
use core_host::scan::{ScanOutcome, ScanReply, ScanRequest, run, scan_all};
use core_model::mixer::PluginFormat;
use std::path::Path;
use std::time::Duration;

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_vizdaw-scan"))
}

fn req(id: Option<&str>, timeout: Duration) -> ScanRequest {
    ScanRequest {
        format: PluginFormat::Clap,
        path: test_plugins::bundle(),
        id: id.map(String::from),
        timeout,
    }
}

#[test]
fn lists_and_scans_a_plugin() {
    let ScanOutcome::Done(ScanReply::Listed(list)) =
        run(exe(), &req(None, Duration::from_secs(10)))
    else {
        panic!("no list");
    };
    assert_eq!(list.len(), 6);
    let ScanOutcome::Done(ScanReply::Scanned(info)) = run(
        exe(),
        &req(Some("vizdaw.test.ducker"), Duration::from_secs(10)),
    ) else {
        panic!("no scan");
    };
    assert_eq!(info.plugin.name, "Test Ducker");
    assert_eq!(info.sidechain_input(), Some(1));
}

#[test]
fn crashes_hangs_and_errors_have_reasons() {
    let crash = run(
        exe(),
        &req(Some("vizdaw.test.crasher"), Duration::from_secs(10)),
    );
    assert_eq!(
        crash.reason().as_deref(),
        Some("crashed (signal 11)"),
        "{crash:?}"
    );
    let hang = run(
        exe(),
        &req(Some("vizdaw.test.hanger"), Duration::from_millis(1500)),
    );
    assert_eq!(hang, ScanOutcome::TimedOut);
    assert_eq!(hang.reason().as_deref(), Some("timed out"));
    let nope = run(
        exe(),
        &req(Some("vizdaw.test.nope"), Duration::from_secs(10)),
    );
    let why = nope.reason().unwrap();
    assert!(
        why.starts_with("failed: ") && why.contains("not found"),
        "{why}"
    );
}

#[test]
fn a_full_scan_fills_the_catalog_and_the_cache_follows_the_mtime() {
    let bundle = test_plugins::bundle();
    // Work on a copy so touching it never disturbs other tests.
    let dir = std::env::temp_dir().join(format!("vizdaw-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let copy = dir.join("Copy.clap");
    copy_dir(&bundle, &copy);
    let src = Source::clap_bundle(&copy);
    let mut cat = Catalog::new();
    let todo = cat.mark_found(std::slice::from_ref(&src));
    assert_eq!(todo, vec![src.clone()]);
    let n = scan_all(&mut cat, exe(), &todo, |_| Duration::from_millis(3000));
    assert_eq!(n, 1);
    let status = |id: &str| cat.status(PluginFormat::Clap, id).unwrap();
    for id in ["gain", "ducker", "synth4", "nan"] {
        assert_eq!(
            status(&format!("vizdaw.test.{id}")),
            PluginStatus::Ok,
            "{id}"
        );
    }
    assert_eq!(
        status("vizdaw.test.crasher"),
        PluginStatus::Blocklisted("crashed (signal 11)".into())
    );
    assert_eq!(
        status("vizdaw.test.hanger"),
        PluginStatus::Blocklisted("timed out".into())
    );
    assert!(
        cat.get(PluginFormat::Clap, "vizdaw.test.gain")
            .unwrap()
            .info
            .is_some()
    );
    // Unchanged: nothing to scan. Modified: scan again.
    assert!(cat.mark_found(&[Source::clap_bundle(&copy)]).is_empty());
    let exe_file = copy.join("Contents/MacOS/VizDAW Test");
    let target = if exe_file.exists() {
        exe_file
    } else {
        copy.clone()
    };
    let f = std::fs::File::options().write(true).open(&target).unwrap();
    f.set_modified(std::time::SystemTime::now() + Duration::from_secs(120))
        .unwrap();
    let again = Source::clap_bundle(&copy);
    assert_ne!(again.mtime, src.mtime);
    assert_eq!(cat.mark_found(std::slice::from_ref(&again)), vec![again]);
    assert!(
        cat.get(PluginFormat::Clap, "vizdaw.test.gain").is_none(),
        "the old scan is dropped"
    );
}

fn copy_dir(from: &Path, to: &Path) {
    if from.is_file() {
        std::fs::copy(from, to).unwrap();
        return;
    }
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        copy_dir(&e.path(), &to.join(e.file_name()));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn audio_units_are_listed_and_scanned() {
    use core_host::scan::scan_source;
    let src = Source {
        format: PluginFormat::Au,
        path: "au:aufx/dely/appl".into(),
        version: String::new(),
        mtime: 0,
    };
    let entries = scan_source(exe(), &src, Duration::from_secs(10));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].plugin.name, "AUDelay");
    assert_eq!(entries[0].status, core_host::catalog::ScanStatus::Ok);
    let info = entries[0].info.as_ref().unwrap();
    assert!(info.params.iter().any(|p| p.name == "Delay Time"));
    let missing = run(
        exe(),
        &ScanRequest {
            format: PluginFormat::Au,
            path: "au:aufx/none/appl".into(),
            id: None,
            timeout: Duration::from_secs(10),
        },
    );
    assert!(missing.reason().unwrap().contains("not found"));
}
