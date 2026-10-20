//! Shared test support for the core-ffi integration tests.

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
    limit(60)
}
