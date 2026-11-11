//! Limits on hostile input (spec §6.5). Every reader takes a [`Limits`].

/// Largest MIDI file read, in bytes (§6.5).
pub const MAX_MIDI_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Longest variable-length quantity, in bytes (§6.5).
pub const MAX_VLQ_BYTES: usize = 4;
/// Most events in one file, all tracks together (§6.5).
pub const MAX_EVENTS: usize = 10_000_000;
/// Largest system-exclusive message, in bytes (§6.5).
pub const MAX_SYSEX_BYTES: usize = 1024 * 1024;
/// Most MTrk chunks read; more is refused (§6.5).
pub const MAX_TRACKS: usize = 4096;
/// Longest single meta or Flex Data text, in bytes (§6.5).
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
/// Most details a report keeps per item (§6.5).
pub const MAX_DETAILS: usize = 50;

/// The limits a reader applies; tests lower them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_events: usize,
    pub max_sysex_bytes: usize,
    pub max_tracks: usize,
    pub max_text_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_events: MAX_EVENTS,
            max_sysex_bytes: MAX_SYSEX_BYTES,
            max_tracks: MAX_TRACKS,
            max_text_bytes: MAX_TEXT_BYTES,
        }
    }
}
