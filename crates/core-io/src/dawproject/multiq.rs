//! A DAWproject `Equalizer` onto the bundled Multi-Q's bands (plan 6g
//! Task 5, Ruling 10): which band each file band takes, its values clamped
//! to Multi-Q's ranges, and the values as Multi-Q's CLAP parameters take
//! them. Pure: the plug-in is never loaded to decide this.
//!
//! The parameter table is typed from Task 1's dump,
//! `crates/core-host/tests/data/multiq-params.json` (a test compares them).

use super::schema::{DpEqBand, DpEqBandType, DpRealParam, DpUnit};
use super::units::db_of_linear;
use core_model::mixer::ParamId;

/// Multi-Q's band count.
pub const BANDS: u8 = 8;

/// How a CLAP parameter's value is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    /// In the parameter's own units.
    Plain,
    /// From 0 to 1 over the parameter's range.
    Normalized,
}

/// The domain Multi-Q's CLAP parameters take (Task 1's dump).
pub const DOMAIN: Domain = Domain::Normalized;

/// A continuous parameter's range, as `MultiQ.cpp` builds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    /// The least value.
    pub start: f64,
    /// The greatest value.
    pub end: f64,
    /// Whether it is a log range (then `skew` is unused).
    pub log: bool,
    /// The step the plug-in snaps to (0: none).
    pub interval: f64,
    /// The skew of a linear range.
    pub skew: f64,
}

impl Range {
    /// `v` (already within the range) as the CLAP value: as it is in the
    /// plain domain; normalised by `ln(v/start)/ln(end/start)` for a log
    /// range, else `((v − start)/(end − start))^skew`.
    pub fn to_clap(&self, v: f64) -> f64 {
        match DOMAIN {
            Domain::Plain => v,
            Domain::Normalized => {
                let n = if self.log {
                    (v / self.start).ln() / (self.end / self.start).ln()
                } else {
                    ((v - self.start) / (self.end - self.start)).powf(self.skew)
                };
                n.clamp(0.0, 1.0)
            }
        }
    }
}

/// Every band's frequency, Hz.
pub const FREQ: Range = Range {
    start: 20.0,
    end: 20_000.0,
    log: true,
    interval: 0.0,
    skew: 1.0,
};
/// Bands 2–7's gain, dB.
pub const GAIN: Range = Range {
    start: -24.0,
    end: 24.0,
    log: false,
    interval: 0.1,
    skew: 1.0,
};
/// Every band's Q.
pub const Q: Range = Range {
    start: 0.1,
    end: 100.0,
    log: true,
    interval: 0.0,
    skew: 1.0,
};

/// One row of Task 1's dump.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Param {
    /// The JUCE parameter ID.
    pub string_id: &'static str,
    /// The CLAP parameter ID ([`juce_hash`] of `string_id`).
    pub clap_id: u32,
    /// The name the plug-in gives.
    pub name: &'static str,
    /// The least CLAP value.
    pub min: f64,
    /// The greatest CLAP value.
    pub max: f64,
    /// The default CLAP value.
    pub default: f64,
    /// Whether the plug-in reports it as stepped.
    pub stepped: bool,
}

const fn row(
    string_id: &'static str,
    clap_id: u32,
    name: &'static str,
    default: f64,
    stepped: bool,
) -> Param {
    Param {
        string_id,
        clap_id,
        name,
        min: 0.0,
        max: 1.0,
        default,
        stepped,
    }
}

const Q_DEFAULT: f64 = 0.283_752_769_231_796_26;
const SLOPE_DEFAULT: f64 = 0.142_857_149_243_354_8;

/// Multi-Q's band parameters, in the dump's order.
pub const PARAMS: [Param; 38] = [
    row("band1_enabled", 757_438_110, "Band 1 Enabled", 0.0, true),
    row("band1_freq", 1_472_467_003, "Band 1 Frequency", 0.0, false),
    row("band1_q", 3_957_659_022, "Band 1 Q", Q_DEFAULT, false),
    row(
        "band1_slope",
        2_708_640_840,
        "Band 1 Slope",
        SLOPE_DEFAULT,
        false,
    ),
    row("band2_enabled", 3_244_950_943, "Band 2 Enabled", 1.0, true),
    row(
        "band2_freq",
        1_501_096_154,
        "Band 2 Frequency",
        0.232_989_996_671_676_64,
        false,
    ),
    row("band2_gain", 1_501_109_729, "Band 2 Gain", 0.5, false),
    row("band2_q", 3_957_659_983, "Band 2 Q", Q_DEFAULT, false),
    row("band2_shape", 3_596_011_903, "Band 2 Shape", 0.0, false),
    row("band3_enabled", 1_437_496_480, "Band 3 Enabled", 1.0, true),
    row(
        "band3_freq",
        1_529_725_305,
        "Band 3 Frequency",
        0.333_333_343_267_440_8,
        false,
    ),
    row("band3_gain", 1_529_738_880, "Band 3 Gain", 0.5, false),
    row("band3_q", 3_957_660_944, "Band 3 Q", Q_DEFAULT, false),
    row("band3_shape", 188_548_288, "Band 3 Shape", 0.0, false),
    row("band4_enabled", 3_925_009_313, "Band 4 Enabled", 1.0, true),
    row(
        "band4_freq",
        1_558_354_456,
        "Band 4 Frequency",
        0.465_979_993_343_353_27,
        false,
    ),
    row("band4_gain", 1_558_368_031, "Band 4 Gain", 0.5, false),
    row("band4_q", 3_957_661_905, "Band 4 Q", Q_DEFAULT, false),
    row("band4_shape", 1_076_051_969, "Band 4 Shape", 0.0, false),
    row("band5_enabled", 2_117_554_850, "Band 5 Enabled", 1.0, true),
    row(
        "band5_freq",
        1_586_983_607,
        "Band 5 Frequency",
        0.566_323_339_939_117_4,
        false,
    ),
    row("band5_gain", 1_586_997_182, "Band 5 Gain", 0.5, false),
    row("band5_q", 3_957_662_866, "Band 5 Q", Q_DEFAULT, false),
    row("band5_shape", 1_963_555_650, "Band 5 Shape", 0.0, false),
    row("band6_enabled", 310_100_387, "Band 6 Enabled", 1.0, true),
    row(
        "band6_freq",
        1_615_612_758,
        "Band 6 Frequency",
        0.666_666_686_534_881_6,
        false,
    ),
    row("band6_gain", 1_615_626_333, "Band 6 Gain", 0.5, false),
    row("band6_q", 3_957_663_827, "Band 6 Q", Q_DEFAULT, false),
    row("band6_shape", 2_851_059_331, "Band 6 Shape", 0.0, false),
    row("band7_enabled", 2_797_613_220, "Band 7 Enabled", 1.0, true),
    row(
        "band7_freq",
        1_644_241_909,
        "Band 7 Frequency",
        0.767_009_973_526_001,
        false,
    ),
    row("band7_gain", 1_644_255_484, "Band 7 Gain", 0.5, false),
    row("band7_q", 3_957_664_788, "Band 7 Q", Q_DEFAULT, false),
    row("band7_shape", 3_738_563_012, "Band 7 Shape", 0.0, false),
    row("band8_enabled", 990_158_757, "Band 8 Enabled", 0.0, true),
    row("band8_freq", 1_672_871_060, "Band 8 Frequency", 1.0, false),
    row("band8_q", 3_957_665_749, "Band 8 Q", Q_DEFAULT, false),
    row(
        "band8_slope",
        331_232_015,
        "Band 8 Slope",
        SLOPE_DEFAULT,
        false,
    ),
];

// The choice lists are not in Task 1's dump: they are typed from
// `MultiQ.cpp`'s `createParameterLayout` at the pinned commit, in its
// order (the render check pins Peaking and one slope).

/// Band 2's shapes.
pub const SHAPES_LOW: [&str; 3] = ["Low Shelf", "Peaking", "High Pass"];
/// Bands 3–6's shapes.
pub const SHAPES_MID: [&str; 4] = ["Peaking", "Notch", "Band Pass", "Tilt Shelf"];
/// Band 7's shapes.
pub const SHAPES_HIGH: [&str; 3] = ["High Shelf", "Peaking", "Low Pass"];
/// Bands 1 and 8's slopes, dB/oct.
pub const SLOPES_DB: [u16; 8] = [6, 12, 18, 24, 36, 48, 72, 96];

/// JUCE's `String::hashCode` as CLAP sees it: `h = h·31 + c` over the
/// characters, wrapping.
pub fn juce_hash(id: &str) -> u32 {
    id.chars()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(u32::from(c)))
}

/// The CLAP parameter for `string_id`, when Multi-Q has it.
pub fn param_id(string_id: &str) -> Option<ParamId> {
    PARAMS
        .iter()
        .find(|p| p.string_id == string_id)
        .map(|p| ParamId(u64::from(p.clap_id)))
}

/// One mapped band's values in Multi-Q's terms, before conversion.
#[derive(Debug, Clone, PartialEq)]
pub struct MqBand {
    /// The band, 1 … 8.
    pub band: u8,
    /// The shape index in the band's list (bands 2–7), when set.
    pub shape: Option<u8>,
    /// The frequency, Hz.
    pub freq: f64,
    /// The gain, dB (bands 2–7), when set.
    pub gain_db: Option<f64>,
    /// The Q, when set (else the default stays).
    pub q: Option<f64>,
    /// The slope index in [`SLOPES_DB`] (bands 1 and 8), when set.
    pub slope: Option<u8>,
    /// Whether the band is on.
    pub enabled: bool,
}

/// What [`map_equalizer`] makes of an `Equalizer`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MqMapping {
    /// All eight bands, in order; a band no file band took is off.
    pub bands: Vec<MqBand>,
    /// The overlay, in Multi-Q's CLAP value domain.
    pub params: Vec<(ParamId, f64)>,
    /// Changed lines (a value clamped or approximated), naming the track
    /// and the band.
    pub changed: Vec<String>,
    /// Left-out lines: file bands that could not be read or had no room.
    pub left_out: Vec<String>,
}

/// A file band whose values could be read, before it has a Multi-Q band.
struct Read {
    kind: DpEqBandType,
    freq: f64,
    gain_db: Option<f64>,
    q: Option<f64>,
    order: Option<i32>,
    enabled: bool,
    /// What reading changed, for the Changed lines.
    changes: Vec<String>,
    /// The file band, for a Left-out line.
    text: String,
}

/// The candidates of a band type, in preference order: (band, shape index).
fn candidates(kind: DpEqBandType) -> &'static [(u8, Option<u8>)] {
    match kind {
        DpEqBandType::HighPass => &[(1, None), (2, Some(2))],
        DpEqBandType::LowPass => &[(8, None), (7, Some(2))],
        DpEqBandType::LowShelf => &[(2, Some(0))],
        DpEqBandType::HighShelf => &[(7, Some(0))],
        DpEqBandType::Bell => &[
            (3, Some(0)),
            (4, Some(0)),
            (5, Some(0)),
            (6, Some(0)),
            (2, Some(1)),
            (7, Some(1)),
        ],
        DpEqBandType::Notch => &[(3, Some(1)), (4, Some(1)), (5, Some(1)), (6, Some(1))],
        DpEqBandType::BandPass => &[(3, Some(2)), (4, Some(2)), (5, Some(2)), (6, Some(2))],
    }
}

fn unit_text(unit: DpUnit) -> &'static str {
    match unit {
        DpUnit::Linear => "linear",
        DpUnit::Normalized => "normalized",
        DpUnit::Percent => "percent",
        DpUnit::Decibel => "dB",
        DpUnit::Hertz => "Hz",
        DpUnit::Semitones => "semitones",
        DpUnit::Seconds => "seconds",
        DpUnit::Beats => "beats",
        DpUnit::Bpm => "bpm",
    }
}

fn is_pass(kind: DpEqBandType) -> bool {
    matches!(kind, DpEqBandType::HighPass | DpEqBandType::LowPass)
}

/// A parameter as text: "1000 Hz", "-2 dB", "0.5 normalized".
fn param_text(p: &DpRealParam) -> String {
    match p.value {
        Some(v) => format!("{v} {}", unit_text(p.unit)),
        None => format!("no value ({})", unit_text(p.unit)),
    }
}

/// The file band as text, for a Left-out line: "bell 9000 Hz, gain 1 dB,
/// Q 1.4, order 2".
fn band_text(b: &DpEqBand) -> String {
    let mut s = match b.kind {
        Some(k) => k.name().to_owned(),
        None => "band of unknown type".to_owned(),
    };
    match &b.freq {
        Some(f) => {
            s.push(' ');
            s.push_str(&param_text(f));
        }
        None => s.push_str(", no frequency"),
    }
    if let Some(g) = &b.gain {
        s.push_str(", gain ");
        s.push_str(&param_text(g));
    }
    if let Some(q) = &b.q {
        s.push_str(", Q ");
        s.push_str(&param_text(q));
    }
    if let Some(o) = b.order {
        s.push_str(&format!(", order {o}"));
    }
    if b.enabled == Some(false) {
        s.push_str(", off");
    }
    s
}

/// `v` clamped to `lo..=hi`, with a change line when it moved.
fn clamped(v: f64, lo: f64, hi: f64, what: &str, unit: &str, changes: &mut Vec<String>) -> f64 {
    let c = v.clamp(lo, hi);
    if c != v {
        changes.push(format!("{what} {v}{unit} set to {c}{unit}"));
    }
    c
}

/// Reads a file band's values, clamped (Ruling 10), or why it cannot be.
fn read(b: &DpEqBand) -> Result<Read, &'static str> {
    let kind = b.kind.ok_or("its type is not known")?;
    let mut changes = Vec::new();
    let f = b.freq.as_ref().ok_or("it has no frequency")?;
    let v = f.value.ok_or("it has no frequency")?;
    if !v.is_finite() {
        return Err("its frequency is not a finite number");
    }
    let hz = match (f.unit, f.min, f.max) {
        (DpUnit::Hertz, _, _) => v,
        (DpUnit::Normalized, Some(lo), Some(hi)) if lo.is_finite() && hi.is_finite() && lo < hi => {
            lo + v * (hi - lo)
        }
        (DpUnit::Normalized, _, _) => return Err("its frequency is normalised with no range"),
        _ => return Err("its frequency is not in hertz"),
    };
    let freq = clamped(hz, FREQ.start, FREQ.end, "frequency", " Hz", &mut changes);
    let gain_db = match &b.gain {
        None => None,
        Some(g) => {
            let Some(v) = g.value else {
                return Err("its gain has no readable value or unit");
            };
            if !v.is_finite() {
                return Err("its gain is not a finite number");
            }
            let db = match g.unit {
                DpUnit::Decibel => v,
                DpUnit::Linear => db_of_linear(v),
                _ => return Err("its gain is not in decibels or linear"),
            };
            if is_pass(kind) {
                if db != 0.0 {
                    changes.push(format!("gain {db} dB not used by a {}", kind.name()));
                }
                None
            } else {
                Some(clamped(
                    db,
                    GAIN.start,
                    GAIN.end,
                    "gain",
                    " dB",
                    &mut changes,
                ))
            }
        }
    };
    let q = match &b.q {
        None => None,
        Some(p) => match p.value {
            None => return Err("its Q has no readable value or unit"),
            Some(v) if !v.is_finite() => return Err("its Q is not a finite number"),
            Some(v) if p.unit == DpUnit::Linear => {
                Some(clamped(v, Q.start, Q.end, "Q", "", &mut changes))
            }
            Some(_) => return Err("its Q is not linear"),
        },
    };
    Ok(Read {
        kind,
        freq,
        gain_db,
        q,
        order: b.order,
        enabled: b.enabled != Some(false),
        changes,
        text: band_text(b),
    })
}

/// The slope index nearest to `6·order` dB/oct (the lower on a tie), and
/// whether it is exact.
fn slope_of(order: i32) -> (u8, bool) {
    let want = i64::from(order).saturating_mul(6);
    let mut best = (0u8, i64::MAX);
    for (i, s) in SLOPES_DB.iter().enumerate() {
        let d = (i64::from(*s) - want).abs();
        if d < best.1 {
            best = (u8::try_from(i).unwrap_or(0), d);
        }
    }
    (best.0, best.1 == 0)
}

/// The number of shapes band `band` offers (0: it has no shape).
fn shape_count(band: u8) -> usize {
    match band {
        2 => SHAPES_LOW.len(),
        3..=6 => SHAPES_MID.len(),
        7 => SHAPES_HIGH.len(),
        _ => 0,
    }
}

/// A choice index as its normalised CLAP value, `index/(count − 1)`.
fn choice(index: u8, count: usize) -> f64 {
    match DOMAIN {
        Domain::Plain => f64::from(index),
        Domain::Normalized => {
            let n = u32::try_from(count.saturating_sub(1).max(1)).unwrap_or(1);
            f64::from(index) / f64::from(n)
        }
    }
}

/// Ruling 10: the file's bands assigned to Multi-Q's, clamped, and
/// converted to the CLAP value domain of Task 1's table.
///
/// Bands are taken in two passes: first `highPass`, `lowPass`, `lowShelf`
/// and `highShelf` in file order, then `bell`, `notch` and `bandPass`
/// sorted by frequency; each takes its first free candidate. A band that
/// cannot be read, or finds no free band, is Left out. Every Multi-Q band
/// no file band took is turned off.
pub fn map_equalizer(bands: &[DpEqBand], track: &str) -> MqMapping {
    let mut m = MqMapping::default();
    let mut read_ok: Vec<Read> = Vec::new();
    for b in bands {
        match read(b) {
            Ok(r) => read_ok.push(r),
            Err(why) => m.left_out.push(format!(
                "Equalizer band on track '{track}': {} ({why})",
                band_text(b)
            )),
        }
    }
    let (mut first, mut second): (Vec<Read>, Vec<Read>) = read_ok.into_iter().partition(|r| {
        matches!(
            r.kind,
            DpEqBandType::HighPass
                | DpEqBandType::LowPass
                | DpEqBandType::LowShelf
                | DpEqBandType::HighShelf
        )
    });
    second.sort_by(|a, b| a.freq.total_cmp(&b.freq));
    first.append(&mut second);

    let mut taken: [Option<MqBand>; BANDS as usize] = Default::default();
    for r in first {
        let free = candidates(r.kind).iter().find(|(n, _)| {
            taken
                .get(usize::from(*n).saturating_sub(1))
                .is_some_and(Option::is_none)
        });
        let Some(&(n, shape)) = free else {
            m.left_out.push(format!(
                "Equalizer band on track '{track}': {} (Multi-Q has no free band for it)",
                r.text
            ));
            continue;
        };
        let mut changes = r.changes;
        let mut slope = None;
        if let Some(order) = r.order.filter(|_| is_pass(r.kind)) {
            if n == 1 || n == 8 {
                let (i, exact) = slope_of(order);
                if !exact {
                    let got = SLOPES_DB.get(usize::from(i)).copied().unwrap_or(0);
                    changes.push(format!(
                        "slope {} dB/oct set to {got} dB/oct",
                        i64::from(order).saturating_mul(6)
                    ));
                }
                slope = Some(i);
            } else if order != 2 {
                changes.push("slope kept at 12 dB/oct".to_owned());
            }
        }
        for c in changes {
            m.changed.push(format!(
                "Equalizer band on track '{track}', Multi-Q band {n}: {c}"
            ));
        }
        if let Some(t) = taken.get_mut(usize::from(n).saturating_sub(1)) {
            *t = Some(MqBand {
                band: n,
                shape,
                freq: r.freq,
                gain_db: r.gain_db,
                q: r.q,
                slope,
                enabled: r.enabled,
            });
        }
    }

    for (i, t) in taken.into_iter().enumerate() {
        let n = u8::try_from(i + 1).unwrap_or(BANDS);
        let used = t.is_some();
        let b = t.unwrap_or_else(|| MqBand {
            band: n,
            shape: None,
            freq: default_freq(n),
            gain_db: None,
            q: None,
            slope: None,
            enabled: false,
        });
        push_params(&mut m.params, &b, used);
        m.bands.push(b);
    }
    m
}

/// Band `n`'s default frequency, Hz.
fn default_freq(n: u8) -> f64 {
    match n {
        1 => 20.0,
        2 => 100.0,
        3 => 200.0,
        4 => 500.0,
        5 => 1000.0,
        6 => 2000.0,
        7 => 4000.0,
        _ => 20_000.0,
    }
}

/// `b`'s values as CLAP parameters: on/off always; the rest only when a
/// file band took it (`used`).
fn push_params(out: &mut Vec<(ParamId, f64)>, b: &MqBand, used: bool) {
    let mut put = |what: &str, v: f64| {
        if let Some(id) = param_id(&format!("band{}_{what}", b.band)) {
            out.push((id, v));
        }
    };
    put("enabled", if b.enabled { 1.0 } else { 0.0 });
    if !used {
        return;
    }
    put("freq", FREQ.to_clap(b.freq));
    if let Some(g) = b.gain_db {
        put("gain", GAIN.to_clap(g));
    }
    if let Some(q) = b.q {
        put("q", Q.to_clap(q));
    }
    if let Some(s) = b.shape {
        put("shape", choice(s, shape_count(b.band)));
    }
    if let Some(s) = b.slope {
        put("slope", choice(s, SLOPES_DB.len()));
    }
}
