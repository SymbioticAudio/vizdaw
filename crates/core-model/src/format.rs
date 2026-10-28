//! The project's audio format (sub-project 5 spec §4.1): every media file
//! of a project is stored at this sample rate and bit depth. The engine
//! always processes 32-bit float; the format governs only the files and
//! the rate the device is asked to run at.

use serde::{Deserialize, Serialize};

/// Bit depth of a project's media files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BitDepth {
    /// 16-bit integer (written with TPDF dither).
    Int16,
    /// 24-bit integer.
    Int24,
    /// 32-bit float.
    Float32,
}

impl BitDepth {
    /// Bits per sample.
    pub fn bits(self) -> u32 {
        match self {
            BitDepth::Int16 => 16,
            BitDepth::Int24 => 24,
            BitDepth::Float32 => 32,
        }
    }

    /// Bytes per sample.
    pub fn bytes(self) -> usize {
        match self {
            BitDepth::Int16 => 2,
            BitDepth::Int24 => 3,
            BitDepth::Float32 => 4,
        }
    }

    /// Display name ("24-bit", "32-bit float").
    pub fn label(self) -> &'static str {
        match self {
            BitDepth::Int16 => "16-bit",
            BitDepth::Int24 => "24-bit",
            BitDepth::Float32 => "32-bit float",
        }
    }

    /// Every depth, smallest first.
    pub fn all() -> [BitDepth; 3] {
        [BitDepth::Int16, BitDepth::Int24, BitDepth::Float32]
    }
}

/// A project's audio format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioFormat {
    /// Sample rate in Hz, one of [`AudioFormat::RATES`].
    pub sample_rate: u32,
    /// Bit depth of the media files.
    pub depth: BitDepth,
}

impl AudioFormat {
    /// The allowed project rates.
    pub const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

    /// The default: 48 000 Hz, 24-bit integer.
    pub const DEFAULT: AudioFormat = AudioFormat {
        sample_rate: 48_000,
        depth: BitDepth::Int24,
    };

    /// True when the rate is an allowed project rate.
    pub fn is_valid(&self) -> bool {
        AudioFormat::RATES.contains(&self.sample_rate)
    }

    /// Display text, e.g. "44.1 kHz, 16-bit".
    pub fn label(&self) -> String {
        let khz = f64::from(self.sample_rate) / 1000.0;
        let rate = if self.sample_rate % 1000 == 0 {
            format!("{khz:.0}")
        } else {
            format!("{khz:.1}")
        };
        format!("{rate} kHz, {}", self.depth.label())
    }
}

impl Default for AudioFormat {
    fn default() -> AudioFormat {
        AudioFormat::DEFAULT
    }
}
