//! Units, colours and identity tags of the DAWproject route (plan 6d
//! Ruling 15): beats and ticks, linear and dB levels, normalised pan, the
//! swatch colours and the `vz:` tags in comments.

use core_model::automation::{pan_norm, pan_of};
use core_model::mixer::{GAIN_FLOOR_DB, Swatch, db_to_gain};
use core_model::rules::MAX_TICKS;
use core_model::time::{PPQ, Tick};

/// The last line of the metadata `Comment`, before the export ID.
pub const EXPORT_LINE: &str = "VizDAW export ";

/// How far off the tick grid a time may be and still count as on it, in
/// ticks.
const GRID_SLACK: f64 = 1e-6;

/// `t` in beats: `t / 15 360`.
pub fn beats(t: Tick) -> f64 {
    t.0 as f64 / PPQ as f64
}

/// The tick nearest `beats` (half away from zero), and whether that moved it
/// (it was off the grid by more than 1e-6 tick). `None` for a value that is
/// not finite or lies past ±[`MAX_TICKS`].
pub fn ticks(beats: f64) -> Option<(Tick, bool)> {
    if !beats.is_finite() {
        return None;
    }
    let exact = beats * PPQ as f64;
    let near = exact.round();
    if !near.is_finite() || near.abs() > MAX_TICKS as f64 {
        return None;
    }
    // In range of `i64` (checked above), so the cast is exact.
    Some((Tick(near as i64), (exact - near).abs() > GRID_SLACK))
}

/// The linear gain of a level in dB: `10^(dB/20)`, 0 at or below
/// [`GAIN_FLOOR_DB`].
pub fn linear_gain(db: f64) -> f64 {
    db_to_gain(db)
}

/// The level in dB of a linear gain: [`GAIN_FLOOR_DB`] for 0 or less, NaN or
/// a level under the floor.
pub fn db_of_linear(l: f64) -> f64 {
    if l.is_nan() || l <= 0.0 {
        return GAIN_FLOOR_DB;
    }
    let db = 20.0 * l.log10();
    if db <= GAIN_FLOOR_DB {
        GAIN_FLOOR_DB
    } else {
        db
    }
}

/// Pan −1 … +1 as the format's normalised 0 … 1: `(p + 1) / 2`.
pub fn pan_normalized(p: f64) -> f64 {
    pan_norm(p)
}

/// A normalised pan 0 … 1 as VizDAW's −1 … +1: `2n − 1`, clamped (NaN
/// gives −1).
pub fn pan_of_normalized(n: f64) -> f64 {
    pan_of(n)
}

/// The swatches with their fixed colours (Ruling 15), independent of the
/// theme.
const SWATCHES: [(Swatch, &str, [u8; 3]); 8] = [
    (Swatch::Red, "#e5484d", [0xe5, 0x48, 0x4d]),
    (Swatch::Orange, "#f76b15", [0xf7, 0x6b, 0x15]),
    (Swatch::Yellow, "#ffc53d", [0xff, 0xc5, 0x3d]),
    (Swatch::Green, "#46a758", [0x46, 0xa7, 0x58]),
    (Swatch::Teal, "#12a594", [0x12, 0xa5, 0x94]),
    (Swatch::Blue, "#0090ff", [0x00, 0x90, 0xff]),
    (Swatch::Purple, "#8e4ec6", [0x8e, 0x4e, 0xc6]),
    (Swatch::Pink, "#d6409f", [0xd6, 0x40, 0x9f]),
];

/// `s` as `#rrggbb`.
pub fn swatch_hex(s: Swatch) -> &'static str {
    SWATCHES
        .iter()
        .find(|(w, _, _)| *w == s)
        .map_or("#0090ff", |(_, hex, _)| hex)
}

/// The swatch nearest `#rrggbb` (either case) by RGB distance, and whether
/// that changed the colour (it is not exactly the swatch's). `None` for
/// anything that is not `#` and six hex digits.
pub fn nearest_swatch(hex: &str) -> Option<(Swatch, bool)> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |i: usize| {
        digits
            .get(i..i + 2)
            .and_then(|d| u8::from_str_radix(d, 16).ok())
    };
    let rgb = [channel(0)?, channel(2)?, channel(4)?];
    let distance = |c: &[u8; 3]| -> u32 {
        rgb.iter()
            .zip(c)
            .map(|(a, b)| u32::from(a.abs_diff(*b)).pow(2))
            .sum()
    };
    SWATCHES
        .iter()
        .min_by_key(|(_, _, c)| distance(c))
        .map(|(s, _, c)| (*s, distance(c) != 0))
}

/// What a `vz:` tag names (spec §7.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TagKind {
    /// A track (`t`).
    Track,
    /// A strip (`s`).
    Strip,
    /// An event (`e`).
    Event,
    /// A marker, by index (`m`).
    Marker,
    /// A plug-in instance in the inserts or the instrument (`d`).
    Device,
    /// A plug-in instance in the EQ slot (`q`).
    Eq,
    /// Beside a use's [`TagKind::Event`] on each clip of a use its track's
    /// loop splits: the pass of the loop the clip plays in (`p`), so the
    /// clips' passes rise in time order ([`pass_comment`]).
    Pass,
}

impl TagKind {
    const ALL: [TagKind; 7] = [
        TagKind::Track,
        TagKind::Strip,
        TagKind::Event,
        TagKind::Marker,
        TagKind::Device,
        TagKind::Eq,
        TagKind::Pass,
    ];

    fn letter(self) -> char {
        match self {
            TagKind::Track => 't',
            TagKind::Strip => 's',
            TagKind::Event => 'e',
            TagKind::Marker => 'm',
            TagKind::Device => 'd',
            TagKind::Eq => 'q',
            TagKind::Pass => 'p',
        }
    }
}

/// The ID of the copy of shared strip `strip`'s channel on track `lane`
/// (`c<strip>t<lane>`): a strip several tracks play through is written once
/// per track, each copy with the strip's tag and its devices' tags.
pub fn copy_channel_id(strip: u64, lane: u64) -> String {
    format!("c{strip}t{lane}")
}

/// The (strip, track) of a [`copy_channel_id`]; `None` for any other ID.
pub fn copied_channel(id: &str) -> Option<(u64, u64)> {
    let (x, y) = id.strip_prefix('c')?.split_once('t')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    if !(digits(x) && digits(y)) {
        return None;
    }
    Some((x.parse().ok()?, y.parse().ok()?))
}

/// The comment of the clip of pass `pass` of reference use `event`, one of
/// the clips a looping track splits it into: `vz:e<event> vz:p<pass>`.
pub fn pass_comment(event: u64, pass: u64) -> String {
    format!(
        "{} {}",
        tag(TagKind::Event, event),
        tag(TagKind::Pass, pass)
    )
}

/// The tag `vz:<letter><id>`.
pub fn tag(kind: TagKind, id: u64) -> String {
    format!("vz:{}{id}", kind.letter())
}

/// Whether `token` is a tag (`vz:`, one ASCII letter, at least one digit),
/// and what it names when its letter is one of [`TagKind`]'s and its number
/// fits.
fn as_tag(token: &str) -> Option<Option<(TagKind, u64)>> {
    let rest = token.strip_prefix("vz:")?;
    let mut chars = rest.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    let digits = chars.as_str();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let kind = TagKind::ALL.into_iter().find(|k| k.letter() == letter);
    Some(kind.zip(digits.parse().ok()))
}

/// `comment` without its `vz:<letter><digits>` tokens, and the tags it held
/// (in order; tokens with an unknown letter or too large a number are
/// removed but not returned). Each line keeps its other words, separated by
/// one space; a line that held only tags is dropped; the result is trimmed.
pub fn strip_tags(comment: &str) -> (String, Vec<(TagKind, u64)>) {
    let mut tags = Vec::new();
    let mut lines = Vec::new();
    for line in comment.split('\n') {
        let mut words = Vec::new();
        let mut had_tag = false;
        for word in line.split_whitespace() {
            match as_tag(word) {
                Some(t) => {
                    had_tag = true;
                    tags.extend(t);
                }
                None => words.push(word),
            }
        }
        if had_tag && words.is_empty() {
            continue;
        }
        lines.push(if had_tag {
            words.join(" ")
        } else {
            line.to_owned()
        });
    }
    (lines.join("\n").trim().to_owned(), tags)
}
