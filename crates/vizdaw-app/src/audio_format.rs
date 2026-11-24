//! The audio format rows the Bounce and Export for Another DAW sheets
//! share (Swift `AudioFormatChoices`, `AudioFormatRows`): `Format`
//! WAV/AIFF/CAF, `Bits` 16-bit/24-bit/32-bit float, `Rate` six rates and
//! `Other…` (a field from 8,000 to 384,000 Hz), and `Dither` (kept as set,
//! on by default at 16 bits, never sent at 32-bit float).

use vizdaw_ffi::{BitDepthRow, ContainerRow, ProjectFormatRow};

/// The rows' labels.
pub const FORMAT: &str = "Format";
pub const BITS: &str = "Bits";
pub const RATE: &str = "Rate";
pub const DITHER: &str = "Dither";
/// The rate picker's last item.
pub const OTHER: &str = "Other\u{2026}";
/// The "Other…" field's placeholder and accessibility label.
pub const OTHER_PLACEHOLDER: &str = "Rate (Hz)";
pub const OTHER_LABEL: &str = "Other rate in Hz, 8000 to 384000";

/// The six project rates; "Other…" takes 8,000–384,000.
pub const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
/// The "Other…" field's range.
pub const OTHER_RATES: std::ops::RangeInclusive<u32> = 8_000..=384_000;

/// The Format picker's items.
pub const CONTAINERS: [(ContainerRow, &str); 3] = [
    (ContainerRow::Wav, "WAV"),
    (ContainerRow::Aiff, "AIFF"),
    (ContainerRow::Caf, "CAF"),
];
/// The Bits picker's items.
pub const DEPTHS: [(BitDepthRow, &str); 3] = [
    (BitDepthRow::Int16, "16-bit"),
    (BitDepthRow::Int24, "24-bit"),
    (BitDepthRow::Float32, "32-bit float"),
];

/// A Rate item's title: "44100 Hz".
pub fn rate_title(rate: u32) -> String {
    format!("{rate} Hz")
}

/// The rows' choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub container: ContainerRow,
    pub depth: BitDepthRow,
    pub rate: u32,
    /// "Other…" is the chosen rate (a field from 8,000 to 384,000).
    pub other_rate: bool,
    /// Kept as set; on by default at 16 bits; ignored for float.
    pub dither: bool,
}

impl AudioFormat {
    /// The project's format as the rows start: WAV, the project's depth
    /// and rate ("Other…" for a rate not listed), dither on at 16 bits.
    pub fn for_project(f: ProjectFormatRow) -> Self {
        AudioFormat {
            container: ContainerRow::Wav,
            depth: f.depth,
            rate: f.sample_rate,
            other_rate: !RATES.contains(&f.sample_rate),
            dither: f.depth == BitDepthRow::Int16,
        }
    }

    /// The Rate picker's value: a listed rate, or 0 for "Other…".
    pub fn rate_choice(&self) -> u32 {
        if self.other_rate { 0 } else { self.rate }
    }

    /// Chooses a listed rate, or "Other…" (0), which keeps the rate.
    pub fn set_rate_choice(&mut self, choice: u32) {
        if choice == 0 {
            self.other_rate = true;
        } else {
            self.other_rate = false;
            self.rate = choice;
        }
    }

    /// The "Other…" field's value, clamped to 8,000–384,000.
    pub fn set_other_rate(&mut self, hz: i64) {
        let (lo, hi) = (*OTHER_RATES.start(), *OTHER_RATES.end());
        self.rate = hz.clamp(i64::from(lo), i64::from(hi)) as u32;
    }

    /// The dither a request sends: never at 32-bit float.
    pub fn sent_dither(&self) -> bool {
        self.depth != BitDepthRow::Float32 && self.dither
    }
}

/// Choices that hold the format rows (Swift `AudioFormatChoices`): the
/// Bounce sheet's and the Export for Another DAW sheet's.
pub trait AudioFormatChoices {
    fn audio_format(&self) -> AudioFormat;
    fn set_audio_format(&mut self, f: AudioFormat);

    /// Edits the format rows' choices.
    fn edit_audio_format(&mut self, edit: &dyn Fn(&mut AudioFormat)) {
        let mut f = self.audio_format();
        edit(&mut f);
        self.set_audio_format(f);
    }
}
