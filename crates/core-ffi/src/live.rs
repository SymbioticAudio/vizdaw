//! Live input over the FFI (plan 5c Task 8; sub-project 5 spec §6.1,
//! §6.6, §12.1): what the engine's input offers, and MIDI bytes injected
//! as if a named source sent them (`midi.inject`), checked before they
//! reach the real-time queue.

use crate::error::VizError;
use core_engine::input::channel_message;
use core_engine::midi1::Midi1Msg;

/// The most bytes one injection may carry.
pub const MAX_INJECT_BYTES: usize = 1024;

/// The chosen input device's channels as the input picker offers them
/// (5d-2 final review I4), read without the engine: no device is opened
/// and no microphone asked.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct InputChannelsRow {
    /// Input channels (0: the device has none).
    pub channels: u32,
    /// One name per channel ("Input k" when the device names none).
    pub names: Vec<String>,
}

/// What the audio input offers (`state.get input`).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct InputInfoRow {
    /// The device ("Offline" for the offline backend; "" before the first
    /// Play).
    pub device: String,
    /// Input channels (0: no input).
    pub channels: u32,
    /// Their names.
    pub names: Vec<String>,
    /// Input latency in frames.
    pub input_latency_frames: u32,
    /// Output latency in frames.
    pub output_latency_frames: u32,
    /// The device's I/O buffer in frames (plan 5d-1 Ruling 9; 0 before
    /// the first Play).
    #[uniffi(default)]
    pub buffer_frames: u32,
    /// What a monitored input takes from input to output: both latencies
    /// and a buffer each way.
    #[uniffi(default)]
    pub round_trip_frames: u32,
    /// The input is opening or closing on its own thread (plan 5d-1
    /// Ruling 13).
    #[uniffi(default)]
    pub opening: bool,
    /// The device's rate (0 before the first Play), for the round trip in
    /// milliseconds.
    #[uniffi(default)]
    pub sample_rate: f64,
}

/// The channel messages in `bytes`, one after another (running status,
/// SysEx and system messages are refused), from source `port` (a name:
/// not empty, at most 255 bytes, no control characters).
pub fn parse_injection(port: &str, bytes: &[u8]) -> Result<Vec<Midi1Msg>, VizError> {
    if port.is_empty() || port.len() > 255 || port.chars().any(char::is_control) {
        return Err(crate::events::bad("`port` must name a MIDI source"));
    }
    if bytes.is_empty() || bytes.len() > MAX_INJECT_BYTES {
        return Err(crate::events::bad(&format!(
            "`bytes` must hold 1 to {MAX_INJECT_BYTES} bytes"
        )));
    }
    let mut out = Vec::new();
    let mut rest = bytes;
    while let Some(&status) = rest.first() {
        let len = match status & 0xF0 {
            0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 3,
            0xC0 | 0xD0 => 2,
            _ => {
                return Err(crate::events::bad(&format!(
                    "byte {status:#04x} does not start a channel message"
                )));
            }
        };
        let (msg, tail) = rest
            .split_at_checked(len)
            .ok_or_else(|| crate::events::bad("a message is cut short"))?;
        let m =
            channel_message(msg).ok_or_else(|| crate::events::bad("a data byte is above 0x7f"))?;
        out.push(m);
        rest = tail;
    }
    Ok(out)
}
