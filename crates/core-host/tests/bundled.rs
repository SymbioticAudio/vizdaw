//! The bundled plug-in folder (plan 6g, Rulings 3 and 4): where it is
//! found, that it is searched first, and that its copy of a plug-in wins
//! over another copy with the same ID. Temp folders only; runs on Linux.

mod common;

use common::plugin;
use core_host::PluginHost;
use core_host::bundled::dir_from;
use core_host::catalog::{Catalog, CatalogEntry, HostMarks, ScanStatus, Source};
use core_host::clap::search_paths_with;
use core_model::mixer::PluginFormat;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vizdaw-bundled-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn source(path: &Path) -> Source {
    Source {
        format: PluginFormat::Clap,
        path: path.to_path_buf(),
        version: "1.0".into(),
        mtime: 1,
    }
}

fn entry(src: &Source, id: &str, status: ScanStatus) -> CatalogEntry {
    CatalogEntry {
        source: src.clone(),
        plugin: plugin(id),
        kind: None,
        info: None,
        status,
    }
}

const ID: &str = "com.example.shared-eq";

/// The user's copy recorded first, the bundled copy second.
fn two_copies(user_status: ScanStatus) -> (Catalog, PathBuf, PathBuf, PathBuf) {
    let root = temp("two");
    let user = root.join("user/EQ.clap");
    let bundled_dir = root.join("A.app/Contents/PlugIns");
    let bundled = bundled_dir.join("EQ.clap");
    let mut c = Catalog::new();
    let us = source(&user);
    c.record(&us, vec![entry(&us, ID, user_status)]);
    let bs = source(&bundled);
    c.record(&bs, vec![entry(&bs, ID, ScanStatus::Ok)]);
    (c, user, bundled_dir, bundled)
}

#[test]
fn dir_prefers_the_environment() {
    let d = temp("env");
    assert_eq!(dir_from(Some(d.clone().into()), None), Some(d.clone()));
    assert_eq!(
        dir_from(Some(OsString::new()), None),
        None,
        "empty is unset"
    );
    assert_eq!(
        dir_from(Some(d.join("nope").into()), None),
        None,
        "a missing folder is unset"
    );
    // The environment wins over the app folder.
    let root = temp("env-app");
    let exe = root.join("A.app/Contents/MacOS/VizDAW");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::create_dir_all(root.join("A.app/Contents/PlugIns")).unwrap();
    assert_eq!(dir_from(Some(d.clone().into()), Some(&exe)), Some(d));
}

#[test]
fn dir_finds_the_app_plugins_folder() {
    let root = temp("app");
    let exe = root.join("A.app/Contents/MacOS/VizDAW");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"").unwrap();
    assert_eq!(dir_from(None, Some(&exe)), None, "no PlugIns folder yet");
    let plugins = root.join("A.app/Contents/PlugIns");
    std::fs::create_dir_all(&plugins).unwrap();
    let found = dir_from(None, Some(&exe)).expect("the app's PlugIns folder");
    assert_eq!(
        found.canonicalize().unwrap(),
        plugins.canonicalize().unwrap()
    );

    // Not inside `*.app/Contents/MacOS`.
    let loose = root.join("bin/VizDAW");
    std::fs::create_dir_all(loose.parent().unwrap()).unwrap();
    std::fs::create_dir_all(root.join("PlugIns")).unwrap();
    assert_eq!(dir_from(None, Some(&loose)), None);
    let not_app = root.join("B/Contents/MacOS/VizDAW");
    std::fs::create_dir_all(not_app.parent().unwrap()).unwrap();
    std::fs::create_dir_all(root.join("B/Contents/PlugIns")).unwrap();
    assert_eq!(dir_from(None, Some(&not_app)), None, "B is not an .app");
}

#[test]
fn search_paths_put_the_bundled_folder_first() {
    let p = PathBuf::from("/x/A.app/Contents/PlugIns");
    let with = search_paths_with(Some(p.clone()));
    assert_eq!(with[0], p);
    assert_eq!(with[1..], search_paths_with(None)[..]);
}

#[test]
fn bundled_copy_wins_a_shared_id() {
    let (mut c, user, bundled_dir, bundled) = two_copies(ScanStatus::Ok);

    let mut host = PluginHost::new(48_000.0, 512).unwrap();
    c.apply(&mut host);
    assert_eq!(
        host.path_of(PluginFormat::Clap, ID),
        Some(user.as_path()),
        "without a preferred dir the first entry wins"
    );

    c.set_preferred_dir(Some(bundled_dir));
    let mut host = PluginHost::new(48_000.0, 512).unwrap();
    c.apply(&mut host);
    assert_eq!(
        host.path_of(PluginFormat::Clap, ID),
        Some(bundled.as_path())
    );
}

#[test]
fn preferred_skips_a_blocklisted_shadow() {
    let (mut c, _user, bundled_dir, bundled) = two_copies(ScanStatus::Blocklisted {
        reason: "crashed (signal 11)".into(),
    });
    c.set_preferred_dir(Some(bundled_dir));
    let e = c.preferred(PluginFormat::Clap, ID).unwrap();
    assert_eq!(e.source.path, bundled);
    assert_eq!(e.status, ScanStatus::Ok);

    let mut host = PluginHost::new(48_000.0, 512).unwrap();
    let mut marks = HostMarks::default();
    c.apply_to(&mut host, &mut marks);
    assert_eq!(
        host.path_of(PluginFormat::Clap, ID),
        Some(bundled.as_path())
    );
    assert_eq!(
        marks,
        HostMarks::default(),
        "a shadowed blocklisted copy does not mark the ID unavailable"
    );
}
