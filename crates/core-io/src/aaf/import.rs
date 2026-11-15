//! An AAF composition mapped onto 6d's import plan (plan 6f Task 3,
//! Ruling 10) and applied by 6d's `apply_tracks` and `arrange_tracks`.
//!
//! **Tracks.** Each audio slot is one audio track; a joined L/R pair
//! ([`pairs`], Ruling 7) is one stereo track named after its stem, and its
//! right slot makes none. A file wider than stereo arrives as mono items
//! (6d's wide-audio rule): a clip of one channel plays that item, a clip of
//! the whole file puts its other channels on new tracks "<name> ch N".
//!
//! **Positions** (Ruling 3). A position `p` at edit rate `n/d` is `p·d/n`
//! seconds (`i128`), placed through the plan's tempo map from the
//! sequence's file start; a region whose start comes back more than one
//! frame away is listed ([`POSITIONS_MOVED`]). Source offsets and fades,
//! in frames at the file's rate, are rescaled to the converted item's
//! rate, rounding half up.
//!
//! **Fades** take Ruling 5's shapes. A `crossfade_in` is a crossfade with
//! `min(L, room)` frames, where `room` is the model's own (the overlap with
//! the region it starts inside, less the later region's fade-out, found by
//! the model's sweep on the same tempo clock); one with no room becomes a
//! plain fade with 6d's [`CROSSFADES_PLAIN`]. A track's regions go in as
//! one insert, so a crossfaded pair is two specs of one command.
//!
//! **Gain and pan** (Ruling 6). Clip gain becomes the region's gain (a
//! varying one its first value, [`CLIP_GAIN_ENVELOPES`]). A track's
//! constant gain and pan set its strip; varying ones become fader and pan
//! automation. Linear curves map as they are; a `Constant` curve's steps
//! become one-tick ramps (6d's [`HELD_STEPS`]); other interpolations are
//! drawn linear ([`CURVES_LINEAR`]).
//!
//! **Applying** (Ruling 8, 9): inside the caller's transaction, the MIDI
//! file beside first, then the tracks and their events, then the markers
//! (those equal by name and tick to one already there are dropped) and, in
//! a new project, the name and song info.

use super::timeline::{Clip, Composition, Curve, Shape, Slot};
use crate::IoError;
use crate::dawproject::import::builder::{
    MediaFacts, PlanBuilder, RegionSpec, plan_info, plan_maps, plan_markers, plan_name, track_count,
};
use crate::dawproject::import::{
    AUDIO_CUT, AUDIO_PAST_FILE, AUDIO_TOO_SHORT, AUDIO_UNLISTED, CROSSFADES_PLAIN, DpImport,
    FADES_CUT, HELD_STEPS, LEVELS_CUT, MAX_SOUNDING, MediaSource, PAST_END, SlotStates, TOO_MUCH,
    VALUES_CLAMPED, WIDE_AUDIO, apply_tracks, arrange_tracks, clean_name,
};
use crate::dawproject::package::AudioRef;
use crate::dawproject::units::{db_of_linear, pan_of_normalized};
use crate::midi_in::{self, MidiImport, Placement};
use crate::report::{ItemKind, Report};
use core_model::automation::fader_norm;
use core_model::command::{Command, SequenceCmd, SettingsCmd};
use core_model::error::ModelError;
use core_model::event::FadeShape;
use core_model::history::History;
use core_model::ids::SeqId;
use core_model::lane::valid_name;
use core_model::meter::MeterMap;
use core_model::mixer::{GAIN_FLOOR_DB, GAIN_MAX_DB};
use core_model::rules::MAX_TICKS;
use core_model::selection::ViewFilter;
use core_model::sequence::{Marker, Sequence};
use core_model::tempo::{Bpm, TempoClock, TempoMap};
use core_model::time::Tick;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

/// The undo label of an import into this project (Ruling 14).
pub const LABEL: &str = "Import AAF";
/// The refusal of a new-project import on a document with tracks.
pub const NOT_NEW_AAF: &str =
    "An AAF opens as a new project, or is added to this one from the import sheet.";
/// The refusal of a composition whose edit rate cannot be read.
pub const BAD_EDIT_RATE: &str = "This AAF file's edit rate cannot be read.";

/// Changed (Ruling 3): a region's start came back more than a frame away.
pub const POSITIONS_MOVED: &str = "Positions moved to VizDAW's resolution by more than one sample";
/// Changed (Ruling 5): `Constant`, `BSpline` and unknown fade shapes.
pub const FADE_SHAPES: &str = "Fade shapes VizDAW does not have became linear";
/// Changed (Ruling 6): a varying clip gain became its first value.
pub const CLIP_GAIN_ENVELOPES: &str =
    "Clip gain envelopes became a constant gain (their first value)";
/// Changed: clip gain past the model's +24 dB.
pub const CLIP_GAIN_CUT: &str = "Clip gain above +24 dB was lowered to +24 dB";
/// Changed (Task 2 hand-off): track volume and pan curves whose
/// interpolation is `Power`, `Log`, `BSpline` or unknown are drawn linear.
pub const CURVES_LINEAR: &str = "Volume and pan curves VizDAW does not have became linear";
/// Note (Ruling 7): slots named as a pair that were not joined (by stem).
pub const NOT_JOINED: &str =
    "Tracks named as left and right that were not joined (their clips differ)";
/// Changed (Ruling 7): a joined pair whose right slot's volume or pan
/// differed from its left's took the left's.
pub const PAIR_MIX: &str = "Joined left and right tracks took the left track's volume and pan";
/// Changed (Ruling 7): a clip of one channel of a stereo file that the job
/// did not split plays the whole file.
pub const ONE_CHANNEL: &str = "Clips that play one channel of a stereo file play both channels";
/// Note (Ruling 8): a new project with no MIDI file beside the AAF.
pub const NO_MIDI: &str = "No MIDI file was found beside the AAF; positions use 120 BPM";

/// The region gain the model holds at most, in dB.
const MAX_REGION_GAIN_DB: f64 = 24.0;
/// How many items the mapping counts between looks at the cancel flag.
const CANCEL_EVERY: usize = 4096;

/// Note (Ruling 3): the file's start timecode, as `HH:MM:SS:FF` (`;`
/// before the frames when drop-frame).
pub fn timecode_note(tc: &str) -> String {
    format!("The file's timecode starts at {tc}; it was placed at the start of the song")
}

/// What the mapping needs from outside the file.
#[derive(Clone, Copy)]
pub struct AafEnv<'a> {
    /// The project's sample rate.
    pub rate: u32,
    /// The tempo map positions go through: the MIDI file's, else (a new
    /// project) 120 BPM, else (into this project) the project's.
    pub tempo: &'a TempoMap,
    /// Where composition position 0 goes: the sequence's file start, at or
    /// before 0 ([`tempo_and_start`] gives it with `tempo`).
    pub start: Tick,
    /// Join L/R pairs (the sheet's "Join L/R pairs").
    pub join_pairs: bool,
    /// The file's audio refs ([`AafFile::audio`]), for the offline items
    /// and the rates offsets are rescaled from.
    ///
    /// [`AafFile::audio`]: crate::dawproject::package::AudioPackage::audio
    pub audio: &'a [AudioRef],
}

/// Two slots to join into one stereo track (Ruling 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    /// The left slot.
    pub left: usize,
    /// The right slot.
    pub right: usize,
    /// The files to join, (left key, right key), in first-use order; the
    /// job makes `sources[left key]` the stereo file.
    pub media: Vec<(usize, usize)>,
}

/// Where the import lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AafPlace {
    /// The sequence.
    pub seq: SeqId,
    /// The document is a new project made for this file.
    pub new_project: bool,
}

// ---------------------------------------------------------------- pairs

/// The suffix pairs, lowercase (Ruling 7).
const SUFFIXES: [(&str, &str); 3] = [(" l", " r"), (".l", ".r"), (" left", " right")];

/// A name's stem and suffix style, when it ends in one of [`SUFFIXES`]'
/// left (`false`) or right (`true`) side with a non-empty stem.
fn side(name: &str) -> Option<(String, usize, bool)> {
    let lower = name.to_lowercase();
    // The longer suffixes first, so "x left" is not read as "x lef" + "t".
    for (k, (l, r)) in SUFFIXES.iter().enumerate().rev() {
        for (suffix, right) in [(l, false), (r, true)] {
            if let Some(stem) = lower.strip_suffix(suffix)
                && !stem.trim().is_empty()
            {
                let cut = name.len().saturating_sub(suffix.len());
                // Lowercasing kept the byte length when the stem matches.
                let shown = name
                    .get(..cut)
                    .filter(|s| s.to_lowercase() == stem)
                    .unwrap_or(stem);
                return Some((shown.trim_end().to_owned(), k, right));
            }
        }
    }
    None
}

/// The slots named as pairs: (left, right, stem), each slot in one pair at
/// most, the right being the first free one of the same stem and style.
fn name_pairs(c: &Composition) -> Vec<(usize, usize, String)> {
    let sides: Vec<Option<(String, usize, bool)>> = c.slots.iter().map(|s| side(&s.name)).collect();
    // Free right slots by (stem in lower case, style), in slot order.
    let mut rights: HashMap<(String, usize), Vec<usize>> = HashMap::new();
    for (j, sd) in sides.iter().enumerate().rev() {
        if let Some((stem, k, true)) = sd {
            rights.entry((stem.to_lowercase(), *k)).or_default().push(j);
        }
    }
    let mut out = Vec::new();
    for (i, sd) in sides.iter().enumerate() {
        let Some((stem, k, false)) = sd else { continue };
        if let Some(j) = rights
            .get_mut(&(stem.to_lowercase(), *k))
            .and_then(Vec::pop)
        {
            out.push((i, j, stem.clone()));
        }
    }
    out
}

/// A converted mono item of `key`: (rate, frames).
fn mono_item(sources: &BTreeMap<usize, MediaSource>, key: usize) -> Option<(u32, u64)> {
    match sources.get(&key) {
        Some(MediaSource::Online(items)) => match items.as_slice() {
            [it] if it.channels == 1 => Some((it.sample_rate, it.length_frames)),
            _ => None,
        },
        _ => None,
    }
}

/// Whether two clips are laid out alike: position, length, offset, fades
/// and gain.
fn same_layout(a: &Clip, b: &Clip) -> bool {
    a.start == b.start
        && a.length == b.length
        && a.source_offset == b.source_offset
        && a.fade_in == b.fade_in
        && a.fade_out == b.fade_out
        && a.crossfade_in == b.crossfade_in
        && a.gain.to_bits() == b.gain.to_bits()
        && a.varying_gain == b.varying_gain
        && a.channel.is_none()
        && b.channel.is_none()
}

/// The pair `l`, `r` joins when both are mono, their clips match one for
/// one, each clip pair plays two distinct online mono files of one rate and
/// length that no other slot plays.
fn pair_of(
    c: &Composition,
    sources: &BTreeMap<usize, MediaSource>,
    used_by: &HashMap<usize, BTreeSet<usize>>,
    l: usize,
    r: usize,
) -> Option<Pair> {
    let (a, b) = (c.slots.get(l)?, c.slots.get(r)?);
    if a.channels != 1 || b.channels != 1 || a.clips.len() != b.clips.len() || a.clips.is_empty() {
        return None;
    }
    let mut media: Vec<(usize, usize)> = Vec::new();
    for (x, y) in a.clips.iter().zip(&b.clips) {
        if !same_layout(x, y) || x.media == y.media {
            return None;
        }
        let (fx, fy) = (mono_item(sources, x.media)?, mono_item(sources, y.media)?);
        if fx != fy {
            return None;
        }
        let only = |key: usize, slot: usize| {
            used_by
                .get(&key)
                .is_some_and(|s| s.len() == 1 && s.contains(&slot))
        };
        if !only(x.media, l) || !only(y.media, r) {
            return None;
        }
        match media.iter().find(|(p, _)| *p == x.media) {
            Some(&(_, q)) if q != y.media => return None,
            Some(_) => {}
            None => {
                if media.iter().any(|(_, q)| *q == y.media) {
                    return None;
                }
                media.push((x.media, y.media));
            }
        }
    }
    Some(Pair {
        left: l,
        right: r,
        media,
    })
}

/// Pairs to join (Ruling 7): slots whose names differ only in `" L"/" R"`,
/// `".L"/".R"` or `" Left"/" Right"` (any case), both mono, whose clips
/// match one for one in position, length, offset, fades and gain, and whose
/// paired files are online mono items of the same rate and frame count
/// that no other slot plays. None when `join` is off.
pub fn pairs(c: &Composition, sources: &BTreeMap<usize, MediaSource>, join: bool) -> Vec<Pair> {
    if !join {
        return Vec::new();
    }
    let mut used_by: HashMap<usize, BTreeSet<usize>> = HashMap::new();
    for (i, s) in c.slots.iter().enumerate() {
        for cl in &s.clips {
            used_by.entry(cl.media).or_default().insert(i);
        }
    }
    name_pairs(c)
        .into_iter()
        .filter_map(|(l, r, _)| pair_of(c, sources, &used_by, l, r))
        .collect()
}

// ---------------------------------------------------------------- mapping

fn stop(cancel: &AtomicBool) -> Result<(), IoError> {
    if cancel.load(Ordering::Relaxed) {
        Err(IoError::Cancelled)
    } else {
        Ok(())
    }
}

/// Composition positions to ticks (Ruling 3).
struct Placer {
    clock: TempoClock,
    /// The seconds at the file start.
    start_s: f64,
    /// The earliest tick a part may land on: the file start (a pickup or
    /// the song's start offset lies before 0), at most 0.
    min: i64,
    n: i64,
    d: i64,
    rate: u32,
}

impl Placer {
    /// `units` edit units in seconds.
    fn secs(&self, units: i64) -> f64 {
        (i128::from(units) * i128::from(self.d)) as f64 / self.n as f64
    }

    /// The tick of `units`, when it is on the timeline.
    fn tick(&self, units: i64) -> Option<Tick> {
        let t = self.clock.seconds_to_tick(self.start_s + self.secs(units));
        (self.min..=MAX_TICKS).contains(&t.0).then_some(t)
    }

    /// Whether `tick`, back in frames at the project rate, is more than a
    /// frame from `units`.
    fn moved(&self, units: i64, tick: Tick) -> bool {
        let rate = f64::from(self.rate.max(1));
        let exact = (i128::from(units) * i128::from(self.d) * i128::from(self.rate.max(1))) as f64
            / self.n as f64;
        let back = (self.clock.tick_to_seconds(tick) - self.start_s) * rate;
        (back - exact).abs() > 1.0
    }

    /// `units` edit units as frames at `rate`, rounded half up (0 when not
    /// positive).
    fn frames(&self, units: i64, rate: u32) -> u64 {
        let num =
            i128::from(units) * i128::from(self.d) * i128::from(rate) * 2 + i128::from(self.n);
        let den = i128::from(self.n) * 2;
        u64::try_from(num.div_euclid(den).max(0)).unwrap_or(u64::MAX)
    }
}

/// `x` frames at `from` as frames at `to`, rounded half up.
fn rescale(x: u64, from: u32, to: u32) -> u64 {
    if from == to || from == 0 {
        return x;
    }
    let num = u128::from(x) * u128::from(to) * 2 + u128::from(from);
    u64::try_from(num / (u128::from(from) * 2)).unwrap_or(u64::MAX)
}

/// Ruling 5: VizDAW's shape for a read shape; `Other` is listed.
fn fade_shape(s: Shape, report: &mut Report) -> FadeShape {
    match s {
        Shape::Linear => FadeShape::Linear,
        Shape::Power => FadeShape::EqualPower,
        Shape::Log => FadeShape::Exponential,
        Shape::Other => {
            report.add(ItemKind::Changed, FADE_SHAPES, None);
            FadeShape::Linear
        }
    }
}

/// A linear track gain in dB, cut to +12 dB ([`LEVELS_CUT`]).
fn level(v: f64, report: &mut Report) -> f64 {
    let db = db_of_linear(v);
    if db > GAIN_MAX_DB {
        report.add(ItemKind::Changed, LEVELS_CUT, None);
        GAIN_MAX_DB
    } else {
        db
    }
}

/// A normalised pan (0 left … 1 right), clamped ([`VALUES_CLAMPED`]).
fn pan_value(v: f64, report: &mut Report) -> f64 {
    let v = if v.is_finite() { v } else { 0.5 };
    if !(0.0..=1.0).contains(&v) {
        report.add(ItemKind::Changed, VALUES_CLAMPED, None);
    }
    v.clamp(0.0, 1.0)
}

/// `HH:MM:SS:FF` of a timecode `frames` at nominal `fps` (`;` before the
/// frames when `drop`, counted as drop-frame at multiples of 30).
fn tc_text(frames: i64, fps: u16, drop: bool) -> String {
    if fps == 0 || frames < 0 {
        return format!("frame {frames}");
    }
    let fps = i64::from(fps);
    let mut f = frames;
    if drop && fps % 30 == 0 {
        let dropped = fps / 15;
        let per10 = fps * 600 - dropped * 9;
        let per1 = fps * 60 - dropped;
        let (tens, rest) = (f / per10, f % per10);
        let extra = if rest > dropped {
            dropped * ((rest - dropped) / per1)
        } else {
            0
        };
        f = f
            .saturating_add(dropped.saturating_mul(9).saturating_mul(tens))
            .saturating_add(extra);
    }
    let (secs, ff) = (f / fps, f % fps);
    let sep = if drop { ';' } else { ':' };
    format!(
        "{:02}:{:02}:{:02}{sep}{:02}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60,
        ff
    )
}

/// A region on its way to its track.
struct Reg {
    track: usize,
    tick: Tick,
    rate: u32,
    /// A crossfade's length from the clip before, in frames.
    xf: Option<u64>,
    spec: RegionSpec,
}

struct Mapper<'c, 'e> {
    c: &'c Composition,
    sources: &'c BTreeMap<usize, MediaSource>,
    env: &'e AafEnv<'e>,
    cancel: &'e AtomicBool,
    at: Placer,
    b: PlanBuilder,
    report: Report,
    refs: HashMap<usize, &'e AudioRef>,
    regs: Vec<Reg>,
    /// Each (track, channel part)'s track for the parts of wide files.
    parts: HashMap<(usize, usize), usize>,
    moved: bool,
    n: usize,
}

/// The plan (Ruling 10): one audio track per slot (a joined pair is one;
/// its media is `sources[left key]`, which the job made stereo). Positions
/// go through `env.tempo` from `env.start` (Ruling 3). The report starts
/// with the composition's. Looks at `cancel` before any work and every few
/// thousand items.
pub fn map(
    c: &Composition,
    sources: &BTreeMap<usize, MediaSource>,
    joined: &[Pair],
    env: &AafEnv,
    cancel: &AtomicBool,
) -> Result<DpImport, IoError> {
    stop(cancel)?;
    let (n, d) = c.edit_rate;
    if n <= 0 || d <= 0 {
        return Err(IoError::Refused(BAD_EDIT_RATE.into()));
    }
    let clock = env.tempo.clock();
    let start_s = clock.tick_to_seconds(env.start);
    let mut m = Mapper {
        c,
        sources,
        env,
        cancel,
        at: Placer {
            clock,
            start_s,
            min: env.start.0.clamp(-MAX_TICKS, 0),
            n: i64::from(n),
            d: i64::from(d),
            rate: env.rate,
        },
        b: PlanBuilder::new(sources, env.tempo.clone(), MeterMap::default()),
        report: c.report.clone(),
        refs: env.audio.iter().map(|a| (a.key, a)).collect(),
        regs: Vec::new(),
        parts: HashMap::new(),
        moved: false,
        n: 0,
    };
    if let Some((frames, fps, drop)) = c.start_tc {
        let what = timecode_note(&tc_text(frames, fps, drop));
        m.report.add(ItemKind::Note, &what, None);
    }
    m.tracks(joined)?;
    m.crossfades()?;
    m.markers()?;
    if m.moved {
        m.report.add(ItemKind::Changed, POSITIONS_MOVED, None);
    }
    m.b.name(c.name.clone());
    m.b.info_from_tags(&c.comments, &mut m.report);
    stop(cancel)?;
    Ok(m.b.build(m.report))
}

impl Mapper<'_, '_> {
    fn tick(&mut self) -> Result<(), IoError> {
        self.n = self.n.wrapping_add(1);
        if self.n % CANCEL_EVERY == 0 {
            stop(self.cancel)
        } else {
            Ok(())
        }
    }

    /// A name fit for a track: cleaned (cut with 6d's line), else
    /// `fallback`.
    fn track_name(&mut self, raw: &str, fallback: impl FnOnce() -> String) -> String {
        let (name, cut) = clean_name(raw);
        if cut {
            self.report.add(
                ItemKind::Changed,
                crate::dawproject::import::NAMES_CUT,
                None,
            );
        }
        match valid_name(&name) {
            Some(n) => n.to_owned(),
            None => fallback(),
        }
    }

    fn tracks(&mut self, joined: &[Pair]) -> Result<(), IoError> {
        let c = self.c;
        let rights: HashSet<usize> = joined.iter().map(|p| p.right).collect();
        let lefts: HashMap<usize, &Pair> = joined.iter().map(|p| (p.left, p)).collect();
        // Pair names: the stem, numbered when one is taken.
        let mut stems: HashMap<String, u32> = HashMap::new();
        let mut pair_names: HashMap<usize, String> = HashMap::new();
        let named = name_pairs(c);
        for (l, r, stem) in &named {
            if lefts.get(l).is_some_and(|p| p.right == *r) {
                let k = stems.entry(stem.to_lowercase()).or_insert(0);
                *k = k.saturating_add(1);
                let name = if *k == 1 {
                    stem.clone()
                } else {
                    format!("{stem} {k}")
                };
                pair_names.insert(*l, name);
            } else if self.env.join_pairs {
                self.report
                    .add(ItemKind::Note, NOT_JOINED, Some(stem.clone()));
            }
        }
        for (i, s) in c.slots.iter().enumerate() {
            self.tick()?;
            if rights.contains(&i) {
                continue;
            }
            let raw = pair_names
                .get(&i)
                .cloned()
                .unwrap_or_else(|| s.name.clone());
            let name = self.track_name(&raw, || format!("Track {}", i.saturating_add(1)));
            let right = lefts.get(&i).and_then(|p| c.slots.get(p.right));
            let (gain, pan) = self.pair_curves(s, right);
            let gain_db = match (gain.constant, gain.points.first()) {
                (Some(v), _) | (None, Some(&(_, v))) => level(v, &mut self.report),
                (None, None) => 0.0,
            };
            let pan_c = match (pan.constant, pan.points.first()) {
                (Some(v), _) | (None, Some(&(_, v))) => {
                    pan_of_normalized(pan_value(v, &mut self.report))
                }
                (None, None) => 0.0,
            };
            let t = self.b.audio_track(name, gain_db, pan_c);
            if gain.constant.is_none() {
                self.curve(t, &gain, true)?;
            }
            if pan.constant.is_none() {
                self.curve(t, &pan, false)?;
            }
            for cl in &s.clips {
                self.tick()?;
                self.clip(t, cl, lefts.contains_key(&i));
            }
        }
        Ok(())
    }

    /// The gain and pan a track takes: a slot's own, or a joined pair's
    /// left's. Mono pairs panned hard left and right take the centre; other
    /// differences are listed ([`PAIR_MIX`]).
    fn pair_curves(&mut self, s: &Slot, right: Option<&Slot>) -> (Curve, Curve) {
        let Some(r) = right else {
            return (s.gain.clone(), s.pan.clone());
        };
        let hard = |c: &Curve, v: f64| c.constant.is_some_and(|x| (x - v).abs() < 1e-6);
        let pan = if hard(&s.pan, 0.0) && hard(&r.pan, 1.0) {
            Curve::default()
        } else {
            if s.pan != r.pan {
                self.report.add(ItemKind::Changed, PAIR_MIX, None);
            }
            s.pan.clone()
        };
        if s.gain != r.gain {
            self.report.add(ItemKind::Changed, PAIR_MIX, None);
        }
        (s.gain.clone(), pan)
    }

    /// A varying track curve as fader (`gain`) or pan automation of track
    /// `t`.
    fn curve(&mut self, t: usize, c: &Curve, gain: bool) -> Result<(), IoError> {
        if c.points.is_empty() {
            return Ok(());
        }
        if !c.step && c.shape != Shape::Linear {
            self.report.add(ItemKind::Changed, CURVES_LINEAR, None);
        }
        let mut last: Option<(Tick, f64)> = None;
        let mut held = false;
        for &(units, v) in &c.points {
            self.tick()?;
            let Some(tick) = self.at.tick(units) else {
                self.report.add(ItemKind::LeftOut, PAST_END, None);
                continue;
            };
            let value = if gain {
                fader_norm(level(v, &mut self.report))
            } else {
                pan_value(v, &mut self.report)
            };
            if c.step
                && let Some((t0, v0)) = last
                && tick.0 > t0.0.saturating_add(1)
            {
                held = true;
                self.point(t, Tick(tick.0 - 1), v0, gain);
            }
            last = Some((tick, value));
            self.point(t, tick, value, gain);
        }
        if held {
            self.report.add(ItemKind::Changed, HELD_STEPS, None);
        }
        Ok(())
    }

    fn point(&mut self, t: usize, tick: Tick, value: f64, gain: bool) {
        if gain {
            self.b.fader_point(t, tick, value);
        } else {
            self.b.pan_point(t, tick, value);
        }
    }

    /// The track for channel `part` (from 1) of a wide file on `track`.
    fn part_track(&mut self, track: usize, part: usize) -> usize {
        if let Some(&t) = self.parts.get(&(track, part)) {
            return t;
        }
        let raw = format!("{} ch {}", self.b.track_name(track), part.saturating_add(1));
        let name = self.track_name(&raw, || raw.clone());
        let t = self.b.audio_track(name.clone(), 0.0, 0.0);
        self.parts.insert((track, part), t);
        self.report.add(ItemKind::Changed, WIDE_AUDIO, Some(name));
        t
    }

    /// One clip as its regions (one per channel part it plays).
    fn clip(&mut self, track: usize, cl: &Clip, joined: bool) {
        let Some(aref) = self.refs.get(&cl.media).copied() else {
            self.report.add(ItemKind::LeftOut, AUDIO_UNLISTED, None);
            return;
        };
        let Some(tick) = self.at.tick(cl.start) else {
            self.report.add(ItemKind::LeftOut, PAST_END, None);
            return;
        };
        let items = self
            .b
            .media(aref, self.sources.get(&cl.media), &mut self.report);
        // The (track, item) pairs the clip plays.
        let mut plays: Vec<(usize, usize)> = Vec::new();
        match (cl.channel, items.as_slice()) {
            (_, []) => return,
            (Some(k), many @ [_, _, ..]) => {
                let at = usize::from(k).min(many.len() - 1);
                plays.extend(many.get(at).map(|&m| (track, m)));
            }
            (Some(_), [one]) => {
                if !joined && self.b.media_facts(*one).is_some_and(|f| f.channels > 1) {
                    self.report.add(ItemKind::Changed, ONE_CHANNEL, None);
                }
                plays.push((track, *one));
            }
            (None, many) => {
                for (p, &m) in many.iter().enumerate() {
                    let t = if p == 0 {
                        track
                    } else {
                        self.part_track(track, p)
                    };
                    plays.push((t, m));
                }
            }
        }
        if !self.moved && self.at.moved(cl.start, tick) {
            self.moved = true;
        }
        if cl.varying_gain {
            self.report
                .add(ItemKind::Changed, CLIP_GAIN_ENVELOPES, None);
        }
        let mut gain_db = db_of_linear(cl.gain);
        if gain_db > MAX_REGION_GAIN_DB {
            self.report.add(ItemKind::Changed, CLIP_GAIN_CUT, None);
            gain_db = MAX_REGION_GAIN_DB;
        }
        let gain_db = gain_db.max(GAIN_FLOOR_DB) as f32;
        let fade_in_shape = match (cl.crossfade_in, cl.fade_in) {
            (Some(f), _) | (None, Some(f)) => fade_shape(f.shape, &mut self.report),
            (None, None) => FadeShape::Linear,
        };
        let fade_out_shape = cl
            .fade_out
            .map_or(FadeShape::Linear, |f| fade_shape(f.shape, &mut self.report));
        for (t, m) in plays {
            let Some(MediaFacts { rate, frames, .. }) = self.b.media_facts(m) else {
                continue;
            };
            let from = if aref.sample_rate > 0 {
                aref.sample_rate
            } else {
                rate
            };
            let offset = rescale(cl.source_offset, from, rate);
            let mut length = self.at.frames(cl.length, rate);
            if length == 0 {
                self.report.add(ItemKind::LeftOut, AUDIO_TOO_SHORT, None);
                continue;
            }
            if offset >= frames {
                self.report.add(ItemKind::LeftOut, AUDIO_PAST_FILE, None);
                continue;
            }
            if offset.saturating_add(length) > frames {
                self.report.add(ItemKind::Changed, AUDIO_CUT, None);
                length = frames - offset;
            }
            let fin = cl
                .crossfade_in
                .or(cl.fade_in)
                .map_or(0, |f| rescale(f.frames, from, rate));
            let mut fout = cl.fade_out.map_or(0, |f| rescale(f.frames, from, rate));
            let mut fin_kept = fin;
            if fin.saturating_add(fout) > length {
                self.report.add(ItemKind::Changed, FADES_CUT, None);
                fout = fout.min(length.saturating_sub(fin.min(length)));
                fin_kept = fin.min(length);
            }
            self.regs.push(Reg {
                track: t,
                tick,
                rate,
                xf: cl.crossfade_in.map(|_| fin_kept),
                spec: RegionSpec {
                    media: m,
                    source_offset: offset,
                    length,
                    fade_in: fin_kept,
                    fade_out: fout,
                    fade_in_shape,
                    fade_out_shape,
                    gain_db,
                    name: cl.name.as_deref().and_then(valid_name).map(str::to_owned),
                    crossfade: false,
                },
            });
        }
    }

    /// Each track's crossfades by the model's rule (its sweep, on the same
    /// tempo clock), then every region into the plan. A region that would
    /// start over [`MAX_SOUNDING`] sounding ones is left out ([`TOO_MUCH`]).
    fn crossfades(&mut self) -> Result<(), IoError> {
        let mut regs = std::mem::take(&mut self.regs);
        regs.sort_by_key(|r| (r.track, r.tick));
        let frames_at = |s: f64, rate: f64| (s * rate).round() as i128;
        let mut sounding: Vec<(i64, f64, f64)> = Vec::new();
        let mut track = usize::MAX;
        for r in regs {
            self.tick()?;
            if r.track != track {
                track = r.track;
                sounding.clear();
            }
            let rate = f64::from(r.rate.max(1));
            let secs = self.at.clock.tick_to_seconds(r.tick);
            let start = frames_at(secs, rate);
            let ends_after = |p: &&(i64, f64, f64)| frames_at(p.1 + p.2, rate) > start;
            if sounding.iter().filter(ends_after).count() >= MAX_SOUNDING {
                self.report.add(ItemKind::LeftOut, TOO_MUCH, None);
                continue;
            }
            sounding.retain(|p| ends_after(&p));
            let mut spec = r.spec;
            if let Some(l) = r.xf {
                let room = sounding
                    .iter()
                    .rev()
                    .find(|p| p.0 < r.tick.0)
                    .map_or(0, |p| {
                        let overlap =
                            u64::try_from(frames_at(p.1 + p.2, rate) - start).unwrap_or(0);
                        overlap.min(spec.length.saturating_sub(spec.fade_out))
                    });
                if l >= 1 && room >= 1 {
                    spec.crossfade = true;
                    spec.fade_in = l.min(room);
                } else {
                    self.report.add(ItemKind::Changed, CROSSFADES_PLAIN, None);
                }
            }
            sounding.push((r.tick.0, secs, spec.length as f64 / rate));
            self.b.region(r.track, r.tick, spec);
        }
        Ok(())
    }

    fn markers(&mut self) -> Result<(), IoError> {
        for mk in &self.c.markers {
            self.tick()?;
            match self.at.tick(mk.position) {
                Some(t) => self.b.marker(t, mk.name.clone()),
                None => self.report.add(ItemKind::LeftOut, PAST_END, None),
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- applying

/// How [`apply`] places the MIDI file beside (Ruling 8): file position 0
/// at the start, every pair on [`suggested_dest`](midi_in::suggested_dest),
/// the file's maps (and pickup) only in a new project.
fn midi_placement(seq: SeqId, new_project: bool) -> Placement {
    Placement {
        seq,
        lane: None,
        tick: Tick(0),
        channel_map: BTreeMap::new(),
        use_tempo: new_project,
        new_project,
    }
}

/// The tempo map and file start [`map`]'s [`AafEnv`] takes, exactly as
/// [`apply`] will leave sequence `seq` (as it is now, before the import):
/// - with a MIDI file, the maps `midi_in::apply` sets (in a new project the
///   file's, shifted back by its pickup) and the tick it puts the MIDI
///   file's position 0 on (`-pickup` in a new project), so the audio lines
///   up with the notes;
/// - a new project without one: 120 BPM from tick 0 (Ruling 8);
/// - into this project without one: the project's map from its file start
///   (`midi_out::file_start`: before 0 when the song has a pickup).
pub fn tempo_and_start(
    midi: Option<&MidiImport>,
    seq: &Sequence,
    new_project: bool,
) -> Result<(TempoMap, Tick), ModelError> {
    match midi {
        Some(m) => {
            let place = midi_placement(seq.id, new_project);
            // `apply` lists these; here they are only computed.
            let mut scratch = Report::default();
            let l = midi_in::landing(m, seq, &place, &mut scratch)?;
            let tempo = if place.new_project || place.use_tempo {
                midi_in::file_maps(m, &seq.tempo, &seq.meter, l.at, l.pickup, &mut scratch).0
            } else {
                seq.tempo.clone()
            };
            Ok((tempo, Tick(l.base)))
        }
        None if new_project => Ok((TempoMap::constant(Bpm::whole(120))?, Tick(0))),
        None => Ok((seq.tempo.clone(), crate::midi_out::file_start(seq))),
    }
}

fn run(h: &mut History, cmd: Command) -> Result<(), ModelError> {
    h.execute(&cmd, &ViewFilter::default()).map(|_| ())
}

/// Inside the caller's open transaction: the MIDI file first (Ruling 8;
/// its maps in a new project, 6a's "tempo map was not used" into this
/// one), then every plan track by 6d's `apply_tracks` + `arrange_tracks`,
/// then the markers (Ruling 9: those equal by name and tick to one already
/// there are dropped) and, in a new project, the name and song info. A new
/// project with no MIDI file takes the plan's maps, with [`NO_MIDI`]. A new
/// project on a document with tracks is refused, [`NOT_NEW_AAF`]. Merges
/// the import's report into `report` and fills its summary.
pub fn apply(
    h: &mut History,
    imp: &DpImport,
    midi: Option<&MidiImport>,
    place: &AafPlace,
    report: &mut Report,
) -> Result<SlotStates, ModelError> {
    let seq = place.seq;
    let doc = h.doc();
    doc.sequence(seq)?;
    if place.new_project && doc.sequences().values().any(|s| !s.lanes.is_empty()) {
        return Err(ModelError::invalid(NOT_NEW_AAF));
    }
    match midi {
        Some(m) => {
            midi_in::apply(h, m, &midi_placement(seq, place.new_project), report)?;
        }
        None if place.new_project => {
            report.add(ItemKind::Note, NO_MIDI, None);
            let (tempo, meter) = plan_maps(imp);
            run(
                h,
                Command::Sequence(SequenceCmd::SetTempo {
                    seq,
                    tempo: tempo.clone(),
                }),
            )?;
            run(
                h,
                Command::Sequence(SequenceCmd::SetMeter {
                    seq,
                    meter: meter.clone(),
                }),
            )?;
            report.summary.tempo_map = true;
        }
        None => {}
    }
    report.merge(imp.report.clone());
    let all: Vec<usize> = (0..track_count(imp)).collect();
    let mut buses = BTreeMap::new();
    let added = apply_tracks(h, imp, seq, &all, &mut buses, &BTreeSet::new())?;
    let placed = arrange_tracks(h, imp, seq, &all, &added, 0, report)?;
    // The plan placed each crossfade by the model's rule; one the model
    // still changed (its tempo map differs from the plan's) is listed.
    for (id, spec) in &placed {
        let kept = h.doc().event(*id).is_ok_and(|(e, _)| e.body == spec.body);
        if !kept {
            report.add(ItemKind::Changed, CROSSFADES_PLAIN, None);
        }
    }
    let s = &mut report.summary;
    s.tracks = s
        .tracks
        .saturating_add(u64::try_from(all.len()).unwrap_or(u64::MAX));

    let existing: Vec<Marker> = h.doc().sequence(seq)?.markers.iter().cloned().collect();
    let have: HashSet<(i64, &str)> = existing
        .iter()
        .map(|m| (m.tick.0, m.name.as_str()))
        .collect();
    let new: Vec<Marker> = plan_markers(imp)
        .iter()
        .filter(|m| !have.contains(&(m.tick.0, m.name.as_str())))
        .cloned()
        .collect();
    if !new.is_empty() {
        let added_n = u64::try_from(new.len()).unwrap_or(u64::MAX);
        let mut markers = existing.clone();
        markers.extend(new);
        markers.sort_by_key(|m| m.tick);
        run(
            h,
            Command::Sequence(SequenceCmd::SetMarkers {
                seq,
                markers: markers.into_iter().collect(),
            }),
        )?;
        report.summary.markers = report.summary.markers.saturating_add(added_n);
    }
    if place.new_project {
        if let Some(name) = valid_name(plan_name(imp)) {
            run(
                h,
                Command::Sequence(SequenceCmd::Rename {
                    seq,
                    name: name.to_owned(),
                }),
            )?;
        }
        let info = plan_info(imp);
        if !info.is_empty() {
            run(
                h,
                Command::Settings(SettingsCmd::SetInfo { info: info.clone() }),
            )?;
        }
    }
    Ok(added.states)
}
