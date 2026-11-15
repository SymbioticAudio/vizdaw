//! Runs a test that loads the real Multi-Q in a child process whose home
//! folder is a temp folder (plan 6g final review I1).
//!
//! Multi-Q's crash log makes `~/Library/Dusk Audio` (JUCE's
//! `userApplicationDataDirectory` is `~/Library` on macOS) when its first
//! instance is made. JUCE finds `~` through
//! `NSHomeDirectory()`, which ignores `HOME` but honours
//! `CFFIXED_USER_HOME`, and CoreFoundation may cache the answer once
//! anything in the process asked. So setting either variable from inside
//! a test is not enough: the test binary runs the one test again in a
//! child process that starts with both variables naming a temp folder.
//!
//! Shared by the core-host, core-ffi and platform-macos tests through
//! `#[path]`; it uses only `std`.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Set in the child: the temp home it was started with.
const MARK: &str = "VIZDAW_TEST_TEMP_HOME";

/// The folder Multi-Q's crash log makes under a home.
pub fn crash_log_folder(home: &Path) -> PathBuf {
    home.join("Library/Dusk Audio")
}

/// A fresh, private temp folder, removed on drop.
struct TempHome(PathBuf);

impl TempHome {
    fn new(test: &str) -> TempHome {
        let base = std::env::temp_dir();
        for n in 0..1000u32 {
            let p = base.join(format!(
                "vizdaw-home-{}-{}-{n}",
                std::process::id(),
                test.replace("::", "-")
            ));
            // `create_dir` fails on an existing path: never reuse one.
            if std::fs::create_dir(&p).is_ok() {
                return TempHome(p);
            }
        }
        panic!("cannot make a temp home in {}", base.display());
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Whether the caller runs its test body here.
///
/// In the child (started by this function): checks that `HOME` and
/// `CFFIXED_USER_HOME` both name the temp home, then answers `true`.
///
/// Otherwise: runs `test` (its full name, as `cargo test -- --exact`
/// takes it) alone in a child of this test binary, with both variables
/// naming a fresh temp folder; panics unless the child ran it and it
/// passed, and unless Multi-Q's crash-log folder landed in the temp
/// home (so `NSHomeDirectory()` answered it, not the real home). Then
/// answers `false`: the caller returns at once.
pub fn in_temp_home(test: &str) -> bool {
    if let Some(home) = std::env::var_os(MARK) {
        assert_eq!(std::env::var_os("HOME"), Some(home.clone()), "HOME");
        assert_eq!(
            std::env::var_os("CFFIXED_USER_HOME"),
            Some(home),
            "CFFIXED_USER_HOME"
        );
        return true;
    }
    let home = TempHome::new(test);
    let exe = std::env::current_exe().expect("the test binary");
    let out = Command::new(exe)
        .args([
            test,
            "--exact",
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HOME", &home.0)
        .env("CFFIXED_USER_HOME", &home.0)
        .env(MARK, &home.0)
        .output()
        .expect("cannot start the test binary again");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    print!("{stdout}");
    eprint!("{stderr}");
    assert!(
        out.status.success(),
        "{test} failed in its temp home ({})",
        out.status
    );
    assert!(
        stdout.contains("test result: ok. 1 passed"),
        "the child did not run {test} exactly once"
    );
    let folder = crash_log_folder(&home.0);
    assert!(
        folder.is_dir(),
        "Multi-Q's crash-log folder is not in the temp home ({}): it may \
         have gone to the real home",
        folder.display()
    );
    println!("Multi-Q's crash-log folder landed in {}", folder.display());
    false
}
