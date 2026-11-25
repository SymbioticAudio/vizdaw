//! The picker sources on the real core's live backend (slice 8c Task 1).
//! Its own binary: `session_record` sets the offline backend for its
//! whole process, and this one must not.

use vizdaw_app::{CoreCalls, LiveCore};

/// On the Mac the real core lists the device's channels and the system's
/// MIDI sources (possibly none) without panicking; elsewhere it answers
/// no channels and no sources (live input off macOS is H3). Offline (a
/// CI job may set `VIZDAW_AUDIO=offline` for every test) the seam answers
/// what the core's free functions do.
#[test]
fn the_real_core_answers_the_picker_sources() {
    let channels = LiveCore.audio_input_channels();
    let sources = LiveCore.midi_source_ports();
    assert_eq!(sources, vizdaw_ffi::host::midi_source_ports(), "the core's");
    if vizdaw_ffi::host::offline_mode() {
        assert_eq!(channels, vizdaw_ffi::host::audio_input_channels());
        assert!(sources.is_empty());
        return;
    }
    if cfg!(target_os = "macos") {
        if let Some(row) = channels {
            assert_eq!(row.names.len(), row.channels as usize, "a name each");
        }
        assert!(sources.iter().all(|s| !s.is_empty()));
    } else {
        assert_eq!(channels, None);
        assert!(sources.is_empty());
    }
}
