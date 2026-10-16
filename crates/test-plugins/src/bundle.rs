//! Builds `VizDAW Test.clap` for tests: runs `cargo build -p test-plugins`
//! into its own target directory (so it never waits on the outer build's
//! lock) and wraps the library in a bundle (macOS) or copies it (elsewhere).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The bundle's file name.
pub const BUNDLE_NAME: &str = "VizDAW Test.clap";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn build() -> Result<PathBuf, String> {
    let root = workspace_root();
    let target = root.join("target").join("test-plugins");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(&root)
        .args([
            "build",
            "--quiet",
            "-p",
            "test-plugins",
            "--lib",
            "--target-dir",
        ])
        .arg(&target)
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !status.success() {
        return Err(format!("building test-plugins failed: {status}"));
    }
    let lib = target.join("debug").join(if cfg!(target_os = "macos") {
        "libtest_plugins.dylib"
    } else if cfg!(windows) {
        "test_plugins.dll"
    } else {
        "libtest_plugins.so"
    });
    let bundle = target.join(BUNDLE_NAME);
    // Build the new bundle beside the old one, then swap it in, so a
    // concurrent test process never sees a half-written bundle.
    let tmp = target.join(format!("{BUNDLE_NAME}.{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_file(&tmp);
    if cfg!(target_os = "macos") {
        let macos = tmp.join("Contents").join("MacOS");
        std::fs::create_dir_all(&macos).map_err(|e| e.to_string())?;
        std::fs::copy(&lib, macos.join("VizDAW Test")).map_err(|e| e.to_string())?;
        std::fs::write(tmp.join("Contents").join("Info.plist"), INFO_PLIST)
            .map_err(|e| e.to_string())?;
    } else {
        std::fs::copy(&lib, &tmp).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(&bundle);
    let _ = std::fs::remove_file(&bundle);
    std::fs::rename(&tmp, &bundle).map_err(|e| e.to_string())?;
    Ok(bundle)
}

/// The path of `VizDAW Test.clap`, built once per test process. Panics
/// (test helper) when the build fails.
pub fn bundle() -> PathBuf {
    static PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    match PATH.get_or_init(build) {
        Ok(p) => p.clone(),
        Err(e) => panic!("{e}"),
    }
}

const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>VizDAW Test</string>
  <key>CFBundleIdentifier</key><string>org.vizdaw.test-plugins</string>
  <key>CFBundleName</key><string>VizDAW Test</string>
  <key>CFBundlePackageType</key><string>BNDL</string>
  <key>CFBundleShortVersionString</key><string>1.0.0</string>
  <key>CFBundleVersion</key><string>1.0.0</string>
</dict>
</plist>
"#;
