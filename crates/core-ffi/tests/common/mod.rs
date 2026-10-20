//! Shared test support for the core-ffi integration tests.

use std::path::Path;
use std::time::Duration;

/// `VIZDAW_TEST_WAIT_SECS` when set to a whole number of seconds, else
/// `default`.
fn limit(default: u64) -> Duration {
    Duration::from_secs(
        std::env::var("VIZDAW_TEST_WAIT_SECS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(default),
    )
}

/// The deadline for a job or the overview worker to finish (180 s unless
/// `VIZDAW_TEST_WAIT_SECS` says otherwise). A deadline, never a sleep: a
/// healthy run finishes long before it; it is wide because a freshly
/// built test binary's first audio-file open can wait on the system's
/// code scan under load.
#[allow(dead_code)]
pub fn wait_limit() -> Duration {
    limit(180)
}

/// The deadline for an Audio Unit editor to load (60 s unless
/// `VIZDAW_TEST_WAIT_SECS` says otherwise).
#[allow(dead_code)]
pub fn au_wait_limit() -> Duration {
    assert_exe_dir_small();
    limit(60)
}

/// Most entries the test executable's directory may hold before Audio Unit
/// tests are refused.
const MAX_EXE_DIR_ENTRIES: usize = 50_000;

/// Fails with an explanation when `dir` holds more than `max` entries.
fn check_dir_entries(dir: &Path, max: usize) -> Result<(), String> {
    let n = std::fs::read_dir(dir).map_or(0, |d| d.count());
    if n > max {
        Err(format!(
            "{} has {n} entries - run `cargo clean`; see split-debuginfo in Cargo.toml",
            dir.display()
        ))
    } else {
        Ok(())
    }
}

/// Panics when the test executable's directory (target/debug/deps) has grown
/// huge: CoreFoundation enumerates it as the main bundle on the first Audio
/// Unit call, which otherwise shows up as a mysterious multi-minute timeout.
#[allow(dead_code)]
pub fn assert_exe_dir_small() {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    if let Some(dir) = dir {
        if let Err(m) = check_dir_entries(&dir, MAX_EXE_DIR_ENTRIES) {
            panic!("{m}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_count_guard_trips_over_its_threshold() {
        let d = std::env::temp_dir().join(format!("vizdaw-guard-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for i in 0..3 {
            std::fs::write(d.join(i.to_string()), b"").unwrap();
        }
        let over = check_dir_entries(&d, 2).unwrap_err();
        let under = check_dir_entries(&d, 10);
        std::fs::remove_dir_all(&d).unwrap();
        assert!(
            over.contains("3 entries") && over.contains("cargo clean"),
            "{over}"
        );
        assert!(under.is_ok());
    }
}
