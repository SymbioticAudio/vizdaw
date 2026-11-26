//! Fader, meter and pan arithmetic (Swift `MixerMath`), the typed-value
//! grammars (Swift `StripEntry`) and the automation modes' titles (Swift
//! `AutomationModes`).

use vizdaw_ffi::automation::AutomationModeRow;
use vizdaw_ffi::mixer::BusRow;

/// Fader, meter and pan arithmetic for the mixer.
pub struct MixerMath;

impl MixerMath {
    /// Fader floor: at or below it the fader is −∞.
    pub const FLOOR_DB: f64 = -144.0;
    pub const MAX_DB: f64 = 12.0;
    /// The lowest dB a fader position other than the bottom shows.
    pub const FADER_BOTTOM_DB: f64 = -60.0;
    /// The largest peak a meter holds (+6 dBFS, the top of its scale).
    pub const MAX_METER_PEAK: f32 = 2.0;
    /// How much a held meter keeps per 30 Hz poll (it falls about 40 dB a
    /// second).
    pub const METER_DECAY: f32 = 0.85;

    /// Fader position 0…1 for `db`: −∞ at 0, then linear in dB from −60 to
    /// +12.
    pub fn fader_position(db: f64) -> f64 {
        if db.is_nan() || db <= Self::FADER_BOTTOM_DB {
            return 0.0;
        }
        ((db - Self::FADER_BOTTOM_DB) / (Self::MAX_DB - Self::FADER_BOTTOM_DB)).min(1.0)
    }

    /// The dB of fader position `p` (the inverse of `fader_position`),
    /// rounded to 0.1.
    pub fn fader_db(p: f64) -> f64 {
        if p.is_nan() || p <= 0.001 {
            return Self::FLOOR_DB;
        }
        let db = Self::FADER_BOTTOM_DB + p.min(1.0) * (Self::MAX_DB - Self::FADER_BOTTOM_DB);
        (db * 10.0).round() / 10.0
    }

    /// "−∞", "0.0", "+3.5", "-12.0".
    pub fn db_label(db: f64) -> String {
        if db.is_nan() || db <= Self::FLOOR_DB {
            return "\u{2212}\u{221E}".into();
        }
        let text = format!("{db:.1}");
        if db > 0.0 { format!("+{text}") } else { text }
    }

    /// Meter fill 0…1 for a linear peak: −60 dBFS to +6 dBFS.
    pub fn meter_fill(peak: f32) -> f64 {
        if peak.is_nan() || peak <= 0.0 || !peak.is_finite() {
            return 0.0;
        }
        let db = 20.0 * f64::from(peak).log10();
        ((db + 60.0) / 66.0).clamp(0.0, 1.0)
    }

    /// "C", "L50", "R25".
    pub fn pan_label(pan: f64) -> String {
        let n = (pan.abs() * 100.0).round() as i64;
        if n == 0 {
            return "C".into();
        }
        format!("{}{n}", if pan < 0.0 { "L" } else { "R" })
    }

    /// NaN reads as silence for that poll; +inf and huge peaks clamp so a
    /// held meter always falls again.
    pub fn sanitized_peak(p: f32) -> f32 {
        if p.is_nan() || p < 0.0 {
            return 0.0;
        }
        p.min(Self::MAX_METER_PEAK)
    }

    /// "Bus N" with the lowest N ≥ 1 that no bus has: the one rule Add
    /// Bus, New Bus and `mixer.new_bus` use.
    pub fn next_bus_name(buses: &[BusRow]) -> String {
        (1u64..)
            .map(|n| format!("Bus {n}"))
            .find(|name| !buses.iter().any(|b| &b.name == name))
            .unwrap_or_else(|| "Bus".into())
    }
}

/// The channel strip's typed values: a level or send level in dB, and a
/// pan as L / C / R. The grammar is strict: ASCII digits only, no
/// exponents, no `nan`, and text longer than [`StripEntry::MAX_LENGTH`] is
/// refused, never cut.
pub struct StripEntry;

const MINUS: char = '\u{2212}';

impl StripEntry {
    pub const LEVEL_HINT: &'static str = "Type a level in dB, like \u{2212}4.2 or \u{2212}inf.";
    pub const PAN_HINT: &'static str = "Type a pan like L 24, C or R 10.";
    pub const TOUCHED_HINT: &'static str = "Release the control first.";
    /// Longest text accepted, in Unicode scalars.
    pub const MAX_LENGTH: usize = 32;

    /// The text trimmed, U+2212 read as "-"; `None` when too long or
    /// holding a control character.
    fn clean(s: &str) -> Option<String> {
        let t = s.trim().replace(MINUS, "-");
        if t.chars().count() > Self::MAX_LENGTH {
            return None;
        }
        // Swift's `controlCharacters`: categories Cc and Cf.
        if t.chars().any(|c| c.is_control() || is_format(c)) {
            return None;
        }
        Some(t)
    }

    /// `digits`, `digits.digits` or `.digits`, ASCII only.
    fn number(s: &str) -> Option<f64> {
        let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
        match s.split_once('.') {
            None => digits(s).then_some(())?,
            Some((whole, frac)) => {
                (digits(frac) && (whole.is_empty() || digits(whole))).then_some(())?
            }
        }
        s.parse().ok()
    }

    /// dB from `text`, rounded to 0.1 and clamped to −144…+12 (−144 is
    /// −∞); `None` when refused.
    pub fn level(text: &str) -> Option<f64> {
        Self::level_max(text, MixerMath::MAX_DB)
    }

    /// As [`StripEntry::level`], clamped to −144…`top` instead of +12.
    pub fn level_max(text: &str, top: f64) -> Option<f64> {
        let mut t = Self::clean(text)?.to_lowercase();
        if let Some(rest) = t.strip_suffix("db") {
            t = rest.strip_suffix(' ').unwrap_or(rest).to_owned();
        }
        let (sign, body) = match t.chars().next() {
            Some('+') => (Some(1.0), &t[1..]),
            Some('-') => (Some(-1.0), &t[1..]),
            _ => (None, &t[..]),
        };
        if body == "inf" || body == "\u{221E}" {
            return (sign == Some(-1.0)).then_some(MixerMath::FLOOR_DB);
        }
        let n = Self::number(body)?;
        let v = (sign.unwrap_or(1.0) * n * 10.0).round() / 10.0;
        if v <= MixerMath::FLOOR_DB {
            return Some(MixerMath::FLOOR_DB);
        }
        if v == 0.0 {
            return Some(0.0);
        }
        Some(v.min(top))
    }

    /// −1…1 from `text` (L negative, whole percent); `None` when refused.
    pub fn pan(text: &str) -> Option<f64> {
        let t = Self::clean(text)?.to_uppercase();
        if t == "C" {
            return Some(0.0);
        }
        let (sign, body) = match t.chars().next() {
            Some(c @ ('L' | 'R')) => {
                let rest = &t[1..];
                (
                    if c == 'L' { -1.0 } else { 1.0 },
                    rest.strip_prefix(' ').unwrap_or(rest),
                )
            }
            Some(c @ ('+' | '-')) => (if c == '-' { -1.0 } else { 1.0 }, &t[1..]),
            _ => (1.0, &t[..]),
        };
        let n = Self::number(body)?;
        let pct = n.round().min(100.0);
        Some(if pct == 0.0 { 0.0 } else { sign * pct / 100.0 })
    }

    /// "−4.2 dB", "+3.0 dB", "0.0 dB", "−∞ dB".
    pub fn level_text(db: f64) -> String {
        Self::level_text_max(db, MixerMath::MAX_DB)
    }

    /// As [`StripEntry::level_text`], clamped at `top` instead of +12.
    pub fn level_text_max(db: f64, top: f64) -> String {
        const FLOOR: &str = "\u{2212}\u{221E} dB";
        if db.is_nan() || db <= MixerMath::FLOOR_DB {
            return FLOOR.into();
        }
        let v = (db.min(top) * 10.0).round() / 10.0;
        if v <= MixerMath::FLOOR_DB {
            return FLOOR.into();
        }
        if v == 0.0 {
            return "0.0 dB".into();
        }
        let sign = if v < 0.0 { MINUS } else { '+' };
        format!("{sign}{:.1} dB", v.abs())
    }

    /// "L 24", "C", "R 10".
    pub fn pan_text(pan: f64) -> String {
        let n = if pan.is_nan() {
            0
        } else {
            (pan.abs().min(1.0) * 100.0).round() as i64
        };
        if n == 0 {
            return "C".into();
        }
        format!("{} {n}", if pan < 0.0 { "L" } else { "R" })
    }
}

/// Unicode general category Cf (format characters) that Swift's
/// `CharacterSet.controlCharacters` also refuses: the common ones.
fn is_format(c: char) -> bool {
    matches!(c as u32,
        0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x08E2 | 0x180E
        | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F
        | 0xFEFF | 0xFFF9..=0xFFFB | 0x110BD | 0x110CD | 0x13430..=0x1343F
        | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F)
}

/// The automation modes as the mixer shows them: their order and titles.
pub struct AutomationModes;

impl AutomationModes {
    /// Every mode in menu order: Off, Read, Touch, Latch, Write, Trim.
    pub const ALL: [AutomationModeRow; 6] = [
        AutomationModeRow::Off,
        AutomationModeRow::Read,
        AutomationModeRow::Touch,
        AutomationModeRow::Latch,
        AutomationModeRow::Write,
        AutomationModeRow::Trim,
    ];

    /// "Off" … "Trim".
    pub fn title(m: AutomationModeRow) -> &'static str {
        match m {
            AutomationModeRow::Off => "Off",
            AutomationModeRow::Read => "Read",
            AutomationModeRow::Touch => "Touch",
            AutomationModeRow::Latch => "Latch",
            AutomationModeRow::Write => "Write",
            AutomationModeRow::Trim => "Trim",
        }
    }

    /// The failure title of a mode change.
    pub const FAILED: &'static str = "Automation Mode Failed";
}
