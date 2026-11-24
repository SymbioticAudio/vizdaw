//! VST3 modules (host track H1 Task 2): bundle resolution, the module
//! entry, the factory's classes, search folders, sources, and creating a
//! class, against the in-repo `VizDAW Test.vst3`.

use core_host::catalog::Source;
use core_host::{HostError, PluginKind, vst3};
use core_model::mixer::PluginFormat;
use std::path::{Path, PathBuf};
use test_plugins::{
    VST3_BANK_ID, VST3_CONTROLLER_ID, VST3_CRASHER_ID, VST3_GAIN_ID, VST3_HANGER_ID, VST3_NAN_ID,
    VST3_PROCESS_CRASHER_ID, VST3_SYNTH_ID, VST3_VIEW_ID,
};

fn temp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// A fake bundle `name` under `dir` with a file at `rel` inside it.
fn fake_bundle(dir: &Path, name: &str, rel: &str) -> PathBuf {
    let b = dir.join(name);
    let f = b.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, b"not a library").unwrap();
    b
}

#[test]
fn lists_every_plugin_class_of_the_test_bundle() {
    let list = vst3::list_bundle(&test_plugins::vst3_bundle()).unwrap();
    let got: Vec<(&str, &str, PluginKind)> = list
        .iter()
        .map(|(p, k)| (p.id.as_str(), p.name.as_str(), *k))
        .collect();
    // The controller class is not a plug-in; factory order is kept.
    assert_eq!(
        got,
        vec![
            (VST3_GAIN_ID, "Test Gain", PluginKind::Effect),
            (VST3_SYNTH_ID, "Test Synth", PluginKind::Instrument),
            (VST3_CRASHER_ID, "Test Crasher", PluginKind::Effect),
            (VST3_HANGER_ID, "Test Hanger", PluginKind::Effect),
            (
                VST3_PROCESS_CRASHER_ID,
                "Test Process Crasher",
                PluginKind::Effect
            ),
            (VST3_NAN_ID, "Test NaN", PluginKind::Effect),
            (VST3_BANK_ID, "Test Bank", PluginKind::Effect),
            (VST3_VIEW_ID, "Test View", PluginKind::Effect),
        ]
    );
    for (p, _) in &list {
        assert_eq!(p.format, PluginFormat::Vst3);
        assert_eq!(p.vendor, "VizDAW");
        assert_eq!(p.version, "1.0.0");
        // Upper-case hex, as `core_model::mixer::vst3_class_id` spells it.
        assert_eq!(
            core_model::mixer::vst3_class_id(&p.id).as_deref(),
            Some(p.id.as_str())
        );
    }
}

#[test]
fn listing_twice_in_one_process_works() {
    // The module is entered, used, exited and unloaded each time.
    let a = vst3::list_bundle(&test_plugins::vst3_bundle()).unwrap();
    let b = vst3::list_bundle(&test_plugins::vst3_bundle()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_class_is_checked_by_creating_its_component() {
    let b = test_plugins::vst3_bundle();
    vst3::check_class(&b, VST3_GAIN_ID).unwrap();
    vst3::check_class(&b, VST3_SYNTH_ID).unwrap();
    // Either case names the same class.
    vst3::check_class(&b, &VST3_GAIN_ID.to_lowercase()).unwrap();
    let missing = "00000000000000000000000000000001";
    assert!(matches!(
        vst3::check_class(&b, missing),
        Err(HostError::NotFound(_))
    ));
    // A controller class is not a plug-in.
    assert!(matches!(
        vst3::check_class(&b, VST3_CONTROLLER_ID),
        Err(HostError::NotFound(_))
    ));
    assert!(matches!(
        vst3::check_class(&b, "vizdaw.test.gain"),
        Err(HostError::NotFound(_))
    ));
}

#[test]
fn class_ids_are_32_upper_case_hex_digits() {
    let cid = [
        0x56, 0x49, 0x5a, 0x44, 0x56, 0x53, 0x54, 0x33, 0x7e, 0x57, 0x00, 0x00, 0xab, 0xcd, 0x00,
        0x01,
    ];
    assert_eq!(vst3::class_id_hex(&cid), VST3_GAIN_ID);
    assert_eq!(vst3::parse_class_id(VST3_GAIN_ID), Some(cid));
    assert_eq!(
        vst3::parse_class_id(&VST3_GAIN_ID.to_lowercase()),
        Some(cid)
    );
    assert_eq!(vst3::parse_class_id("ABCD"), None);
    assert_eq!(vst3::parse_class_id(&"G".repeat(32)), None);
}

#[cfg(target_os = "macos")]
#[test]
fn a_mac_bundle_runs_contents_macos_named_by_its_plist() {
    let d = temp();
    let b = fake_bundle(d.path(), "Thing.vst3", "Contents/MacOS/Thing");
    assert_eq!(
        vst3::binary_path(&b).unwrap(),
        b.join("Contents/MacOS/Thing")
    );
    let b = fake_bundle(d.path(), "Other.vst3", "Contents/MacOS/Real Name");
    std::fs::write(
        b.join("Contents/Info.plist"),
        "<plist><dict><key>CFBundleExecutable</key><string>Real Name</string></dict></plist>",
    )
    .unwrap();
    assert_eq!(
        vst3::binary_path(&b).unwrap(),
        b.join("Contents/MacOS/Real Name")
    );
}

/// Converts `bundle`'s Info.plist to the binary format (as many shipped
/// bundles have it).
#[cfg(target_os = "macos")]
fn binary_plist(bundle: &Path) {
    let plist = bundle.join("Contents/Info.plist");
    let ok = std::process::Command::new("plutil")
        .args(["-convert", "binary1"])
        .arg(&plist)
        .status()
        .unwrap()
        .success();
    assert!(ok, "plutil failed");
    assert!(std::fs::read(&plist).unwrap().starts_with(b"bplist"));
}

/// Final review I2: the executable comes from the bundle as macOS reads
/// it (a binary plist, an escaped name), not from a text search.
#[cfg(target_os = "macos")]
#[test]
fn a_mac_bundle_executable_is_read_from_binary_and_escaped_plists() {
    let d = temp();
    let b = fake_bundle(d.path(), "Binary.vst3", "Contents/MacOS/Real Name");
    std::fs::write(
        b.join("Contents/Info.plist"),
        "<plist><dict><key>CFBundleExecutable</key><string>Real Name</string></dict></plist>",
    )
    .unwrap();
    binary_plist(&b);
    assert_eq!(
        vst3::binary_path(&b).unwrap(),
        b.join("Contents/MacOS/Real Name")
    );
    let b = fake_bundle(d.path(), "Escaped.vst3", "Contents/MacOS/A & B");
    std::fs::write(
        b.join("Contents/Info.plist"),
        "<plist><dict><key>CFBundleExecutable</key><string>A &amp; B</string></dict></plist>",
    )
    .unwrap();
    assert_eq!(
        vst3::binary_path(&b).unwrap(),
        b.join("Contents/MacOS/A & B")
    );
}

/// The real test bundles, renamed (their executable keeps its name) and
/// with binary plists, still load: VST3 and CLAP share the lookup.
#[cfg(target_os = "macos")]
#[test]
fn renamed_bundles_with_binary_plists_still_load() {
    let d = temp();
    let v = d.path().join("Renamed.vst3");
    test_plugins::copy_vst3_bundle(&v);
    binary_plist(&v);
    assert!(!vst3::list_bundle(&v).unwrap().is_empty());
    let c = d.path().join("Renamed.clap");
    test_plugins::copy_bundle(&c);
    binary_plist(&c);
    assert!(!core_host::clap::list_bundle(&c).unwrap().is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn a_linux_bundle_runs_the_so_of_this_architecture() {
    let d = temp();
    let arch = format!(
        "Contents/{}-linux/Thing.so",
        match std::env::consts::ARCH {
            "x86" => "i386",
            "arm" => "armv7l",
            a => a,
        }
    );
    let b = fake_bundle(d.path(), "Thing.vst3", &arch);
    assert_eq!(vst3::binary_path(&b).unwrap(), b.join(&arch));
    // Another architecture's binary is not this one's.
    let b = fake_bundle(
        d.path(),
        "Foreign.vst3",
        "Contents/riscv64-linux/Foreign.so",
    );
    assert!(matches!(
        vst3::binary_path(&b),
        Err(HostError::LoadFailed(_))
    ));
}

#[test]
fn a_bundle_without_a_binary_or_entry_fails_to_list() {
    let d = temp();
    let empty = d.path().join("Empty.vst3");
    std::fs::create_dir_all(empty.join("Contents")).unwrap();
    let e = vst3::list_bundle(&empty).unwrap_err();
    assert!(matches!(e, HostError::LoadFailed(_)), "{e}");
    assert!(e.to_string().contains("Empty.vst3"), "{e}");
    let rel = if cfg!(target_os = "macos") {
        "Contents/MacOS/Junk".to_string()
    } else {
        format!("Contents/{}-linux/Junk.so", std::env::consts::ARCH)
    };
    let junk = fake_bundle(d.path(), "Junk.vst3", &rel);
    assert!(matches!(
        vst3::list_bundle(&junk),
        Err(HostError::LoadFailed(_))
    ));
}

#[test]
fn finds_vst3_bundles_without_looking_inside_them() {
    let d = temp();
    let a = fake_bundle(d.path(), "A.vst3", "Contents/Resources/x");
    // A bundle nested in a bundle is part of it, not a plug-in of its own.
    fake_bundle(&a, "Contents/Inner.vst3", "Contents/Resources/x");
    let b = fake_bundle(&d.path().join("Vendor"), "B.vst3", "Contents/Resources/x");
    fake_bundle(d.path(), "C.clap", "Contents/Resources/x");
    let found = vst3::find_bundles(&[d.path().to_path_buf(), d.path().join("missing")]);
    assert_eq!(found, vec![a, b]);
}

#[test]
fn search_paths_are_the_standard_vst3_folders() {
    let first = PathBuf::from("/bundled");
    let paths = vst3::search_paths_with(Some(first.clone()));
    assert_eq!(paths.first(), Some(&first));
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let want: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec![
            home.join("Library/Audio/Plug-Ins/VST3"),
            "/Library/Audio/Plug-Ins/VST3".into(),
        ]
    } else {
        vec![
            home.join(".vst3"),
            "/usr/lib/vst3".into(),
            "/usr/local/lib/vst3".into(),
        ]
    };
    assert_eq!(&paths[1..], &want[..]);
}

#[test]
fn a_vst3_source_has_its_version_and_newest_mtime() {
    let s = Source::vst3_bundle(&test_plugins::vst3_bundle());
    assert_eq!(s.format, PluginFormat::Vst3);
    assert!(s.mtime > 0);
    if cfg!(target_os = "macos") {
        assert_eq!(s.version, "1.0.0");
    }
    // Without a plist, `moduleinfo.json` gives the version.
    let d = temp();
    let b = fake_bundle(d.path(), "M.vst3", "Contents/Resources/moduleinfo.json");
    std::fs::write(
        b.join("Contents/Resources/moduleinfo.json"),
        "{\n  \"Name\": \"M\",\n  \"Version\": \"2.5.1\",\n  \"Factory Info\": {},\n}\n",
    )
    .unwrap();
    assert_eq!(Source::vst3_bundle(&b).version, "2.5.1");
}
