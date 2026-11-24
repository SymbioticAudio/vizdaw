//! `vizdaw-scan --format vst3` in its own process (host track H1 Task 2):
//! a bundle lists each of its plug-in classes, each class is checked on
//! its own, and a class that crashes or hangs costs only its scanner.

use core_host::PluginKind;
use core_host::catalog::{Catalog, PluginStatus, ScanStatus, Source};
use core_host::scan::{ScanOutcome, ScanReply, ScanRequest, run, scan_all, scan_source};
use core_model::mixer::PluginFormat;
use std::path::Path;
use std::time::Duration;
use test_plugins::{VST3_CRASHER_ID, VST3_GAIN_ID, VST3_HANGER_ID, VST3_SYNTH_ID};

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_vizdaw-scan"))
}

fn req(id: Option<&str>, timeout: Duration) -> ScanRequest {
    ScanRequest {
        format: PluginFormat::Vst3,
        path: test_plugins::vst3_bundle(),
        id: id.map(String::from),
        timeout,
    }
}

#[test]
fn lists_every_class_and_checks_one() {
    let ScanOutcome::Done(ScanReply::Listed(list)) =
        run(exe(), &req(None, Duration::from_secs(10)))
    else {
        panic!("no list");
    };
    let ids: Vec<(&str, PluginKind)> = list.iter().map(|(p, k)| (p.id.as_str(), *k)).collect();
    assert_eq!(
        ids,
        [
            (VST3_GAIN_ID, PluginKind::Effect),
            (VST3_SYNTH_ID, PluginKind::Instrument),
            (VST3_CRASHER_ID, PluginKind::Effect),
            (VST3_HANGER_ID, PluginKind::Effect),
        ]
    );
    // Until the VST3 instance exists, a class that creates cleanly is
    // checked again when first loaded.
    let gain = run(exe(), &req(Some(VST3_GAIN_ID), Duration::from_secs(10)));
    assert!(
        matches!(gain, ScanOutcome::Done(ScanReply::NotValidated(_))),
        "{gain:?}"
    );
}

#[test]
fn crashes_hangs_and_unknown_classes_have_reasons() {
    let crash = run(exe(), &req(Some(VST3_CRASHER_ID), Duration::from_secs(10)));
    assert_eq!(
        crash.reason().as_deref(),
        Some("crashed (signal 11)"),
        "{crash:?}"
    );
    let hang = run(
        exe(),
        &req(Some(VST3_HANGER_ID), Duration::from_millis(1500)),
    );
    assert_eq!(hang, ScanOutcome::TimedOut);
    let nope = run(
        exe(),
        &req(
            Some("00000000000000000000000000000001"),
            Duration::from_secs(10),
        ),
    );
    let why = nope.reason().unwrap();
    assert!(
        why.starts_with("failed: ") && why.contains("not found"),
        "{why}"
    );
    let not_a_bundle = run(
        exe(),
        &ScanRequest {
            path: "/nonexistent/None.vst3".into(),
            ..req(None, Duration::from_secs(10))
        },
    );
    assert!(
        not_a_bundle.reason().unwrap().contains("load failed"),
        "{not_a_bundle:?}"
    );
}

#[test]
fn a_full_scan_records_each_class() {
    let d = tempfile::tempdir().unwrap();
    let copy = d.path().join("Copy.vst3");
    test_plugins::copy_vst3_bundle(&copy);
    let src = Source::vst3_bundle(&copy);
    let entries = scan_source(exe(), &src, Duration::from_millis(3000));
    assert_eq!(entries.len(), 4);
    let mut cat = Catalog::new();
    assert_eq!(
        cat.mark_found(std::slice::from_ref(&src)),
        vec![src.clone()]
    );
    assert_eq!(
        scan_all(&mut cat, exe(), &[src], |_| Duration::from_millis(3000)),
        1
    );
    let status = |id: &str| cat.status(PluginFormat::Vst3, id).unwrap();
    assert_eq!(status(VST3_GAIN_ID), PluginStatus::NotValidated);
    assert_eq!(status(VST3_SYNTH_ID), PluginStatus::NotValidated);
    assert_eq!(
        status(VST3_CRASHER_ID),
        PluginStatus::Blocklisted("crashed (signal 11)".into())
    );
    assert_eq!(
        status(VST3_HANGER_ID),
        PluginStatus::Blocklisted("timed out".into())
    );
    let synth = cat.get(PluginFormat::Vst3, VST3_SYNTH_ID).unwrap();
    assert_eq!(synth.kind, Some(PluginKind::Instrument));
    assert_eq!(synth.status, ScanStatus::NotValidated);
    let names: Vec<String> = cat
        .browser(PluginKind::Effect)
        .into_iter()
        .map(|r| r.plugin.name)
        .collect();
    assert_eq!(names, ["Test Gain"]);
}
