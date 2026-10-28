//! The project's audio format at the FFI (sub-project 5 spec §4.1).

use core_model::format::{AudioFormat, BitDepth};

/// A bit depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BitDepthRow {
    /// 16-bit integer.
    Int16,
    /// 24-bit integer.
    Int24,
    /// 32-bit float.
    Float32,
}

/// A project format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ProjectFormatRow {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Bit depth.
    pub depth: BitDepthRow,
}

/// The project as the format UI shows it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ProjectInfoRow {
    /// The project's format.
    pub format: ProjectFormatRow,
    /// Media items in another format or not VizDAW CAF (the "Convert"
    /// banner).
    pub mismatched: u32,
    /// The device runs at another rate: regions are muted (Ruling 20).
    pub rate_mismatch: bool,
    /// The device's rate (0 before the first Play).
    pub device_rate: f64,
}

impl From<ProjectFormatRow> for AudioFormat {
    fn from(f: ProjectFormatRow) -> AudioFormat {
        AudioFormat {
            sample_rate: f.sample_rate,
            depth: match f.depth {
                BitDepthRow::Int16 => BitDepth::Int16,
                BitDepthRow::Int24 => BitDepth::Int24,
                BitDepthRow::Float32 => BitDepth::Float32,
            },
        }
    }
}

impl From<AudioFormat> for ProjectFormatRow {
    fn from(f: AudioFormat) -> ProjectFormatRow {
        ProjectFormatRow {
            sample_rate: f.sample_rate,
            depth: match f.depth {
                BitDepth::Int16 => BitDepthRow::Int16,
                BitDepth::Int24 => BitDepthRow::Int24,
                BitDepth::Float32 => BitDepthRow::Float32,
            },
        }
    }
}

/// Display text, e.g. "44.1 kHz, 16-bit". Cannot panic, so it is exported
/// without `Result`/`ffi_guard` (accepted, F17).
#[uniffi::export]
pub fn format_label(format: ProjectFormatRow) -> String {
    AudioFormat::from(format).label()
}

/// The allowed project rates. Cannot panic (F17, as `format_label`).
#[uniffi::export]
pub fn project_rates() -> Vec<u32> {
    AudioFormat::RATES.to_vec()
}
