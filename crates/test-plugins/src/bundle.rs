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
    let installed = if cfg!(target_os = "macos") {
        bundle.join("Contents").join("MacOS").join("VizDAW Test")
    } else {
        bundle.clone()
    };
    // An identical bundle is left alone: another process may have just
    // returned it and be about to load it.
    if same_bytes(&lib, &installed) {
        return Ok(bundle);
    }
    let _lock = Lock::take(&target.join("bundle.lock"))?;
    // Another process may have installed it while we waited for the lock.
    if same_bytes(&lib, &installed) {
        return Ok(bundle);
    }
    // Build the new bundle beside the old one, then swap it in.
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

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// An exclusive lock file shared by every test process (std only). A lock
/// older than a minute is taken to belong to a killed process.
struct Lock(PathBuf);

impl Lock {
    fn take(path: &Path) -> Result<Lock, String> {
        let start = std::time::Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(_) => return Ok(Lock(path.to_path_buf())),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > std::time::Duration::from_secs(60));
                    if stale || start.elapsed() > std::time::Duration::from_secs(120) {
                        let _ = std::fs::remove_file(path);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => return Err(format!("cannot lock {path:?}: {e}")),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
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

/// The `vizdaw-scan` binary, built once per test process into the same
/// separate target directory (for tests of programs that run it).
pub fn scanner() -> PathBuf {
    static PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    let r = PATH.get_or_init(|| {
        let root = workspace_root();
        let target = root.join("target").join("test-plugins");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = Command::new(cargo)
            .current_dir(&root)
            .args(["build", "--quiet", "-p", "vizdaw-scan", "--target-dir"])
            .arg(&target)
            .status()
            .map_err(|e| format!("cannot run cargo: {e}"))?;
        if !status.success() {
            return Err(format!("building vizdaw-scan failed: {status}"));
        }
        Ok(target.join("debug").join("vizdaw-scan"))
    });
    match r {
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
