//! How list cells read and parse (Swift `ListFormat.swift`, `PitchMath.swift`):
//! lengths, SMPTE times, MIDI 1.0 views of the 16- and 32-bit values,
//! pitch and key names. Positions are [`crate::position`]'s.

pub use crate::position::{Bbu, UNITS_PER_BEAT, parse_position};

use crate::position::swift_int;

const UNITS: std::ops::RangeInclusive<i64> = 0..=959;

/// Swift's `.whitespaces` trim: spaces and tabs, not line breaks.
pub(crate) fn trim_ws(s: &str) -> &str {
    s.trim_matches(|c: char| {
        c == '\t'
            || (c.is_whitespace()
                && !matches!(
                    c,
                    '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
                ))
    })
}

/// Lengths as beats.units of a beat of `beat` ticks. Ticks off the unit
/// grid lose their remainder on display; the list's unchanged rule relies
/// on that.
pub struct DurationText;

impl DurationText {
    pub const MAX_BEATS: i64 = 1_000_000;

    pub fn text(ticks: i64, beat: i64) -> String {
        let b = beat.max(1);
        format!(
            "{}.{}",
            ticks / b,
            (ticks % b) * i64::from(UNITS_PER_BEAT) / b
        )
    }

    /// The ticks `text` means; an empty part keeps `current`'s; `None`
    /// unless a length of 0 or more.
    pub fn parse(text: &str, current: i64, beat: i64) -> Option<i64> {
        let b = beat.max(1);
        let upb = i64::from(UNITS_PER_BEAT);
        let s = trim_ws(text);
        if s.is_empty() {
            return None;
        }
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() > 2 {
            return None;
        }
        let cur = current.max(0);
        let mut beats = cur / b;
        let mut units = (cur % b) * upb / b;
        if !parts[0].is_empty() {
            let v = swift_int(parts[0]).filter(|v| (0..=Self::MAX_BEATS).contains(v))?;
            beats = v;
            if parts.len() == 1 {
                units = 0;
            }
        }
        if parts.len() == 2 && !parts[1].is_empty() {
            units = swift_int(parts[1]).filter(|u| UNITS.contains(u))?;
        }
        Some(beats * b + (units * b + upb - 1) / upb)
    }
}

/// `hh:mm:ss:ff`.
pub struct Smpte;

impl Smpte {
    pub const RATES: [u32; 3] = [24, 25, 30];

    pub fn text(seconds: f64, fps: u32) -> String {
        if !seconds.is_finite() {
            return "--:--:--:--".into();
        }
        let f = i64::from(fps.max(1));
        let frames = (seconds.abs() * f as f64 + 1e-6).floor().min(9e15) as i64;
        let ff = frames % f;
        let total = frames / f;
        let sign = if seconds < 0. && frames > 0 { "-" } else { "" };
        format!(
            "{sign}{:02}:{:02}:{:02}:{:02}",
            total / 3600,
            (total / 60) % 60,
            total % 60,
            ff
        )
    }

    /// Seconds of a typed SMPTE time; fewer fields count from the right
    /// ("12" is 12 frames, "1:12" one second and 12 frames).
    pub fn parse(text: &str, fps: u32) -> Option<f64> {
        let mut s = trim_ws(text);
        let negative = s.starts_with('-');
        if negative {
            s = &s[1..];
        }
        let parts: Vec<&str> = s.split(':').collect();
        if !(1..=4).contains(&parts.len()) {
            return None;
        }
        let mut values = Vec::with_capacity(4);
        for p in &parts {
            values.push(swift_int(p).filter(|v| (0..=1_000_000).contains(v))?);
        }
        let f = i64::from(fps.max(1));
        let mut v = vec![0_i64; 4 - values.len()];
        v.extend(&values);
        let n = values.len();
        if v[3] >= f || (n >= 3 && v[2] >= 60) || (n >= 4 && v[1] >= 60) {
            return None;
        }
        let secs = (v[0] * 3600 + v[1] * 60 + v[2]) as f64 + v[3] as f64 / f as f64;
        Some(if negative { -secs } else { secs })
    }
}

/// 16-bit velocities shown and typed as MIDI 1.0 values.
pub struct Velocity7;

impl Velocity7 {
    pub fn of(v: u16) -> i64 {
        i64::from(v >> 9)
    }

    /// The MIDI 2.0 upscale: values above 64 repeat their low bits.
    pub fn up(v: i64) -> u16 {
        let c = v.clamp(0, 127) as u16;
        let shifted = c << 9;
        if c <= 64 {
            return shifted;
        }
        let rep = c & 0x3F;
        shifted | (rep << 3) | (rep >> 3)
    }
}

/// 32-bit controller and pressure values shown and typed as 0–127.
pub struct Value7;

impl Value7 {
    pub fn of(v: i64) -> i64 {
        i64::from(v.clamp(0, i64::from(u32::MAX)) as u32 >> 25)
    }

    pub fn up(v: i64) -> u32 {
        let c = v.clamp(0, 127) as u32;
        let shifted = c << 25;
        if c <= 64 {
            return shifted;
        }
        let rep = c & 0x3F;
        shifted | (rep << 19) | (rep << 13) | (rep << 7) | (rep << 1) | (rep >> 5)
    }
}

/// 32-bit pitch bend shown and typed as −8192…8191.
pub struct Bend14;

impl Bend14 {
    pub fn of(v: i64) -> i64 {
        i64::from(v.clamp(0, i64::from(u32::MAX)) as u32 >> 18) - 8192
    }

    pub fn up(signed: i64) -> u32 {
        let u = signed.saturating_add(8192).clamp(0, 16_383) as u32;
        let shifted = u << 18;
        if u <= 8192 {
            return shifted;
        }
        let rep = u & 0x1FFF;
        shifted | (rep << 5) | (rep >> 8)
    }
}

/// Note names as the core prints them: C-1 is 0, middle C (60) is "C4".
pub struct PitchName;

impl PitchName {
    pub const NAMES: [&'static str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];

    pub fn name(p: u8) -> String {
        format!(
            "{}{}",
            Self::NAMES[usize::from(p % 12)],
            i64::from(p / 12) - 1
        )
    }

    /// "C4", "c#4", "Db4", "C-1", or a number 0–127; octaves −2…10.
    pub fn parse(text: &str) -> Option<u8> {
        let s = trim_ws(text);
        if let Some(n) = swift_int(s) {
            return u8::try_from(n).ok().filter(|n| *n <= 127);
        }
        let mut chars = s.chars();
        let base = match chars.next()?.to_ascii_uppercase() {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        let mut rest = chars.as_str();
        let mut pc: i64 = base;
        if let Some(r) = rest.strip_prefix(['#', '♯']) {
            pc += 1;
            rest = r;
        } else if let Some(r) = rest.strip_prefix(['b', '♭']) {
            pc -= 1;
            rest = r;
        }
        let octave = swift_int(rest).filter(|o| (-2..=10).contains(o))?;
        let n = (octave + 1) * 12 + pc;
        u8::try_from(n).ok().filter(|n| *n <= 127)
    }
}

/// Key signatures by name ("D major", "F# minor").
pub struct KeyName;

impl KeyName {
    /// By sharps −7…7.
    const MAJORS: [&'static str; 15] = [
        "Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#",
    ];
    const MINORS: [&'static str; 15] = [
        "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#", "G#", "D#", "A#",
    ];

    pub fn name(sharps: i8, minor: bool) -> String {
        let i = i64::from(sharps) + 7;
        if !(0..15).contains(&i) {
            return format!("{sharps} sharps");
        }
        let names = if minor { Self::MINORS } else { Self::MAJORS };
        let kind = if minor { "minor" } else { "major" };
        format!("{} {kind}", names[i as usize])
    }

    /// The sharps and minor flag a typed key names.
    pub fn parse(text: &str) -> Option<(i8, bool)> {
        let words: Vec<&str> = trim_ws(text).split(' ').filter(|w| !w.is_empty()).collect();
        let [root, kind] = words.as_slice() else {
            return None;
        };
        let minor = match kind.to_lowercase().as_str() {
            "major" => false,
            "minor" => true,
            _ => return None,
        };
        let mut chars = root.chars();
        let first = chars.next()?;
        let root = format!("{}{}", first.to_uppercase(), chars.as_str().to_lowercase());
        let names = if minor { Self::MINORS } else { Self::MAJORS };
        let i = names.iter().position(|n| *n == root)?;
        Some((i as i8 - 7, minor))
    }
}
