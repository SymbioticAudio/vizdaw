//! Process-wide audio settings (sub-project 5 spec §6.6, §6.9 Settings ›
//! Audio and › Recording; plan 5d-1 Rulings 8–12): Software Monitoring,
//! the I/O buffer size, the streamer's RAM cache and read-ahead, and the
//! monitor mode new lanes get. The app keeps them in its preferences and
//! sets them at launch; the control channel's `audio.set` sets them too.
//! They belong to the machine, not to a project, so none is saved in one.

use crate::edit::MonitorModeRow;
use crate::error::VizError;
use core_media::stream::StreamConfig;
use core_model::lane::MonitorMode;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

/// The I/O buffer sizes a device may be asked for (spec §6.6).
pub const BUFFER_SIZES: [u32; 6] = [32, 64, 128, 256, 512, 1024];
/// The I/O buffer size until one is chosen (spec §6.6: 128).
pub const DEFAULT_BUFFER_FRAMES: u32 = 128;
/// The RAM cache budget's range and default, in MiB (spec §5.5: 512).
pub const CACHE_MB: (u32, u32, u32) = (64, 8192, 512);
/// The disk read-ahead's range and default, in seconds (spec §6.9: 2–8,
/// 3).
pub const READ_AHEAD_S: (u32, u32, u32) = (2, 8, 3);

static SOFTWARE_MONITORING: AtomicBool = AtomicBool::new(true);
static IO_BUFFER: AtomicU32 = AtomicU32::new(DEFAULT_BUFFER_FRAMES);
static CACHE: AtomicU32 = AtomicU32::new(CACHE_MB.2);
static READ_AHEAD: AtomicU32 = AtomicU32::new(READ_AHEAD_S.2);
/// 0 Auto, 1 Input, 2 Off.
static DEFAULT_MONITOR: AtomicU8 = AtomicU8::new(0);
/// Bumped by every change a load publishes (Ruling 12).
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// The audio settings (Settings › Audio and › Recording). New stream
/// sizes (`cache_mb`, `read_ahead_seconds`) apply to the next engine: a
/// holder that keeps monitoring (armed lanes, thru) keeps its old sizes
/// until then, while this reports the new ones.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AudioSettingsRow {
    /// Inputs are played back through their strips (spec §6.6).
    pub software_monitoring: bool,
    /// The I/O buffer size asked of the device, in frames.
    pub buffer_frames: u32,
    /// The RAM cache budget, MiB.
    pub cache_mb: u32,
    /// The disk read-ahead, seconds.
    pub read_ahead_seconds: u32,
    /// The monitor mode Add Lane gives a new lane.
    pub default_monitor: MonitorModeRow,
    /// The recording latency trim, frames (spec §6.3).
    pub latency_trim: i32,
}

/// The audio settings now.
#[uniffi::export]
pub fn audio_settings() -> AudioSettingsRow {
    AudioSettingsRow {
        software_monitoring: software_monitoring(),
        buffer_frames: io_buffer_frames(),
        cache_mb: CACHE.load(Ordering::Relaxed),
        read_ahead_seconds: READ_AHEAD.load(Ordering::Relaxed),
        default_monitor: default_monitor().into(),
        latency_trim: i32::try_from(crate::record::latency_trim()).unwrap_or(0),
    }
}

/// Software Monitoring (spec §6.6, Ruling 8): off, no input is played
/// back; recording and MIDI thru go on. The engine holder hears the change
/// at its session's next idle (Ruling 12).
#[uniffi::export]
pub fn set_software_monitoring(on: bool) {
    if SOFTWARE_MONITORING.swap(on, Ordering::Relaxed) != on {
        EPOCH.fetch_add(1, Ordering::Release);
    }
}

/// The I/O buffer size (spec §6.6, Ruling 9): one of [`BUFFER_SIZES`]
/// (else `Model`); `Busy` while any session records or measures. A live device
/// reopens at once with it.
#[uniffi::export]
pub fn set_io_buffer_frames(frames: u32) -> Result<(), VizError> {
    crate::guard::ffi_guard(|| {
        if !BUFFER_SIZES.contains(&frames) {
            return Err(crate::events::bad(
                "the I/O buffer size is 32, 64, 128, 256, 512 or 1024 frames",
            ));
        }
        // One lock for the check and the reopen (pre-flight M4).
        crate::host::set_buffer_frames(frames)?;
        Ok(())
    })
}

/// The RAM cache budget in MiB (64–8192) and the disk read-ahead in
/// seconds (2–8; each stream voice's ring is a second shorter), else
/// `Model` (Ruling 10). They apply to the next engine; an idle one is
/// dropped now so the next Play or arm makes it.
#[uniffi::export]
pub fn set_stream_settings(cache_mb: u32, read_ahead_seconds: u32) -> Result<(), VizError> {
    crate::guard::ffi_guard(|| {
        if !(CACHE_MB.0..=CACHE_MB.1).contains(&cache_mb) {
            return Err(crate::events::bad("the RAM cache is 64 to 8192 MiB"));
        }
        if !(READ_AHEAD_S.0..=READ_AHEAD_S.1).contains(&read_ahead_seconds) {
            return Err(crate::events::bad("the disk read-ahead is 2 to 8 seconds"));
        }
        let a = CACHE.swap(cache_mb, Ordering::Relaxed);
        let b = READ_AHEAD.swap(read_ahead_seconds, Ordering::Relaxed);
        if (a, b) != (cache_mb, read_ahead_seconds) {
            crate::host::drop_if_idle();
        }
        Ok(())
    })
}

/// The monitor mode Add Lane gives new lanes (spec §6.9, Ruling 11).
#[uniffi::export]
pub fn set_default_monitor_mode(mode: MonitorModeRow) {
    let v = match mode {
        MonitorModeRow::Auto => 0,
        MonitorModeRow::Input => 1,
        MonitorModeRow::Off => 2,
    };
    DEFAULT_MONITOR.store(v, Ordering::Relaxed);
}

/// Whether inputs are played back (Ruling 8).
pub fn software_monitoring() -> bool {
    SOFTWARE_MONITORING.load(Ordering::Relaxed)
}

/// Stores the I/O buffer size, returning the one it replaces (the host
/// does this under its lock).
pub(crate) fn swap_io_buffer(frames: u32) -> u32 {
    IO_BUFFER.swap(frames, Ordering::Relaxed)
}

/// The I/O buffer size asked of a device.
pub fn io_buffer_frames() -> u32 {
    IO_BUFFER.load(Ordering::Relaxed)
}

/// The streamer's sizes for the next engine (Ruling 10).
pub fn stream_config() -> StreamConfig {
    let ahead = f64::from(READ_AHEAD.load(Ordering::Relaxed));
    StreamConfig {
        ahead_seconds: ahead,
        voice_seconds: (ahead - 1.0).max(1.0),
        cache_budget_bytes: u64::from(CACHE.load(Ordering::Relaxed)) << 20,
        ..StreamConfig::default()
    }
}

/// The monitor mode new lanes get.
pub fn default_monitor() -> MonitorMode {
    match DEFAULT_MONITOR.load(Ordering::Relaxed) {
        1 => MonitorMode::Input,
        2 => MonitorMode::Off,
        _ => MonitorMode::Auto,
    }
}

/// Changes so far that a load publishes (Ruling 12).
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}

/// Every setting back to its default (test hook: a test binary's tests
/// share the process).
#[cfg(feature = "test-hooks")]
pub fn reset_audio_settings() {
    crate::host::debug_refuse_buffer_sizes(&[]);
    set_software_monitoring(true);
    IO_BUFFER.store(DEFAULT_BUFFER_FRAMES, Ordering::Relaxed);
    CACHE.store(CACHE_MB.2, Ordering::Relaxed);
    READ_AHEAD.store(READ_AHEAD_S.2, Ordering::Relaxed);
    DEFAULT_MONITOR.store(0, Ordering::Relaxed);
}
