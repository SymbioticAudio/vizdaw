//! The composition an AAF file holds (plan 6f Task 2), read out of the
//! object store into a neutral [`Composition`]: its audio slots with their
//! clips, fades, crossfades and gain, the track gain and pan curves, the
//! markers, the start timecode and the song comments, and the audio files
//! the clips play ([`AudioRef`]s, each embedded, external or offline).
//!
//! **Which composition** (Ruling 1): the first `CompositionMob` whose
//! `UsageCode` is `Usage_TopLevel`, else the first that no other mob
//! references, else the first in `ContentStorage` order. More than one
//! candidate gets [`SEVERAL_COMPOSITIONS`].
//!
//! **The walk** (Ruling 4) is iterative: a slot's segment tree is flattened
//! with an explicit stack into fillers, transitions and clips, placed left
//! to right. A `SourceClip`, `Filler` or `OperationGroup` advances the
//! position by its `Length`; a `Transition` moves it back by its own. A
//! transition after a filler is the next clip's fade-in, one between two
//! clips is the later clip's crossfade, and one before a filler (or the end)
//! is the earlier clip's fade-out. `SourceClip` fade properties give the
//! fades no transition gave. `MonoAudioGain` groups multiply into the
//! clip's gain; gain and pan groups around a slot's whole sequence are the
//! track's curves.
//!
//! **Source chains** follow `SourceClip`s through `MasterMob`s to the file
//! `SourceMob`, at most [`AafLimits::max_mob_chain`] mobs with a visited
//! set; each hop's `StartTime` is in its own slot's edit units, counted
//! from the referenced slot's `Origin`, and they add up to the clip's
//! offset in frames at the media's rate. In a master slot's sequence the
//! segment that covers the clip's start is played (fillers count); a clip
//! that plays past that segment's end is listed ([`SPANS_SOURCES`]). Each
//! mob and master slot is read once and memoised, so a clip costs its
//! chain's length, and every clip is charged to the budget. A clip that
//! plays a sub-`CompositionMob` is replaced by that composition's clips in
//! its window, their positions and offsets composed. That is the walk's one
//! recursion: one level per sub-composition, at most
//! [`AafLimits::max_mob_chain`] deep, with a loop check, and every slot's
//! work is counted against one budget.
//!
//! [`AafLimits::max_mob_chain`]: crate::limits::AafLimits::max_mob_chain

use super::dict::{self, class, def, pid};
use super::store::{AafReader, Auid, MobId, NOT_AAF, ObjId, Prop, Store, TOO_LARGE};
use crate::IoError;
use crate::dawproject::import::{NAMES_CUT, clean_name};
use crate::dawproject::package::{
    AudioPackage, AudioRef, AudioSource, external_size, majority_rate, needed_for,
};
use crate::limits::AafLimits;
use crate::read::{URL_NOT_FILE, external_file, is_url};
use crate::report::{ItemKind, Report};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Why MXF media is offline.
pub const MXF: &str = "MXF audio (VizDAW does not read it yet)";
/// Note (Ruling 1): more than one composition could be the one imported.
pub const SEVERAL_COMPOSITIONS: &str =
    "The file holds several compositions; the first top-level one was imported";
/// Left out (Ruling 1): slots of picture, data or other non-sound kinds.
pub const NON_AUDIO: &str = "Picture and other non-audio tracks";
/// Left out (Ruling 13): clips whose mob chain is missing, loops or runs
/// too deep; they play offline media.
pub const NO_SOURCE: &str = "Clips whose source could not be found";
/// Note (Ruling 12): the file's `Identification` names VizDAW.
pub const FROM_VIZDAW: &str =
    "This file came from VizDAW; AAF files open as new projects (they cannot be merged)";
/// Note: a `Selector` played its selected segment.
pub const SELECTED: &str = "Clips with alternatives (the selected one was imported)";
/// Note: an `EssenceGroup` played its first choice.
pub const FIRST_CHOICE: &str = "Media with several versions (the first was imported)";
/// Left out: `ScopeReference`s and the slots of a `NestedScope` before its
/// last.
pub const SCOPES: &str = "Scope references and nested scope tracks";
/// Left out: operations other than gain and pan (their input still plays).
pub const EFFECTS: &str = "Effects other than gain and pan";
/// Left out (Ruling 13): a gain, pan or fade value that is not a usable
/// rational (gain then stays 1).
pub const BAD_VALUES: &str = "Gain and pan values that could not be read";
/// Left out (Ruling 13): slots whose edit rate is not a usable rational.
pub const BAD_RATES: &str = "Tracks whose edit rate could not be read";
/// Left out: control points past [`AafLimits::max_control_points`] per
/// curve.
///
/// [`AafLimits::max_control_points`]: crate::limits::AafLimits::max_control_points
pub const POINTS_CUT: &str = "Automation points past 100 000 per curve";
/// Left out: pan around a clip (VizDAW has no per-region pan).
pub const CLIP_PAN: &str = "Clip pan (VizDAW has no per-region pan)";
/// Left out: the part of a clip that plays past the end of the source
/// segment it starts in (only that segment is played).
pub const SPANS_SOURCES: &str = "Parts of clips that play past the end of their source";
/// Left out: components VizDAW does not know (they become gaps).
pub const UNKNOWN_PARTS: &str = "Parts of the timeline VizDAW does not know";
/// Refusal: no `CompositionMob` with a sound slot to import.
pub const NO_COMPOSITION: &str = "This AAF file holds no composition.";

/// Why a clip's media is offline when its source is missing.
const NO_SOURCE_WHY: &str = "its source could not be found";
/// Why embedded audio is offline when its stream is shorter than declared.
const SHORT_ESSENCE: &str = "its audio is shorter than the file says";
/// Why embedded audio is offline when its format is not WAV, AIFC or PCM.
const ODD_FORMAT: &str = "its audio format is not one VizDAW reads";
/// Why embedded audio is offline when its stream cannot be found.
const NO_ESSENCE: &str = "its audio could not be read";
/// Why audio is offline when the file names no place for it.
const NO_PLACE: &str = "the file does not say where its audio is";
/// The rate assumed when none can be read.
const DEFAULT_RATE: u32 = 48_000;
/// How many work units pass between looks at the cancel flag.
const CANCEL_EVERY: usize = 4096;
/// Work units one clip held costs: its size against the budget.
const CLIP_COST: usize = 8;
/// The 44-byte WAV header's length.
const WAV_HEADER: usize = 44;

/// The refusal for more audio slots than an import builds.
fn too_many_tracks(limit: usize) -> IoError {
    IoError::Refused(format!(
        "This AAF file has more than {limit} tracks, more than VizDAW can open."
    ))
}

/// A fade's shape: the interpolation read (Ruling 5 maps it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `InterpolationDef_Linear` (or a linear-amplitude `FadeType`).
    Linear,
    /// `InterpolationDef_Power` (or a linear-power `FadeType`).
    Power,
    /// `InterpolationDef_Log`.
    Log,
    /// `Constant`, `BSpline` or an interpolation VizDAW does not know.
    Other,
}

/// A fade or crossfade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fade {
    /// Its length, in frames at the clip's media rate.
    pub frames: u64,
    /// Its shape.
    pub shape: Shape,
}

/// One clip of a slot.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// Where it starts, in composition edit units.
    pub start: i64,
    /// How long it plays, in composition edit units.
    pub length: i64,
    /// The key of the [`AudioRef`] it plays.
    pub media: usize,
    /// `Some(k)` when it plays only channel `k` (from 0) of a wider file.
    pub channel: Option<u16>,
    /// Where in the media it starts, in frames at the media's rate.
    pub source_offset: u64,
    /// Its fade-in (no clip before it).
    pub fade_in: Option<Fade>,
    /// Its fade-out.
    pub fade_out: Option<Fade>,
    /// A transition from the clip before it.
    pub crossfade_in: Option<Fade>,
    /// Its gain, linear: the product of the gain groups around it.
    pub gain: f64,
    /// True when a gain around it varies (`gain` holds its first value).
    pub varying_gain: bool,
    /// Its name: the name of the mob it plays, shared by every clip that
    /// plays that mob.
    pub name: Option<Arc<str>>,
}

/// A track's gain or pan.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Curve {
    /// A constant value, if the curve is one.
    pub constant: Option<f64>,
    /// The points of a varying curve: (edit units from the slot's start,
    /// value).
    pub points: Vec<(i64, f64)>,
}

/// One audio slot: a track.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    /// Its name (sanitised, at most 4 096 bytes).
    pub name: String,
    /// Its channels: 1, or the width of the files its clips play whole.
    pub channels: u16,
    /// Its clips, in position order as the sequence gives them.
    pub clips: Vec<Clip>,
    /// The track's gain (linear).
    pub gain: Curve,
    /// The track's pan (0 left, 0.5 centre, 1 right).
    pub pan: Curve,
}

/// A marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AafMarker {
    /// Its position, in composition edit units.
    pub position: i64,
    /// Its name (the event's `Comment`).
    pub name: String,
}

/// The composition read.
#[derive(Debug, Clone, PartialEq)]
pub struct Composition {
    /// The composition mob's name.
    pub name: String,
    /// The composition's edit rate: its first audio slot's.
    pub edit_rate: (i32, i32),
    /// The start timecode: frames, frames per second, drop frame.
    pub start_tc: Option<(i64, u16, bool)>,
    /// The audio slots, in slot order.
    pub slots: Vec<Slot>,
    /// The markers.
    pub markers: Vec<AafMarker>,
    /// The mob's tagged comments: (name, value).
    pub comments: Vec<(String, String)>,
    /// True when the file's `Identification` names VizDAW (Ruling 12).
    pub from_vizdaw: bool,
    /// What reading changed, left out or noted.
    pub report: Report,
}

/// What the app asks of an AAF before importing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AafInfo {
    /// The project rate to make (6d Ruling 17).
    pub sample_rate: u32,
    /// Audio slots.
    pub tracks: u32,
    /// Distinct audio files the clips play.
    pub audio_files: u32,
    /// Of those, the ones that will be offline.
    pub offline: u32,
    /// From VizDAW (Ruling 12).
    pub from_vizdaw: bool,
}

/// Embedded audio: where it is and how it is written out.
#[derive(Debug, Clone)]
struct Essence {
    /// The `EssenceData` object.
    obj: ObjId,
    /// Its data stream.
    stream: String,
    /// The stream's length.
    len: u64,
    /// A WAV header written before raw PCM (empty for a whole file).
    header: Vec<u8>,
}

/// An opened AAF file: the store, the composition and the audio it plays.
pub struct AafFile {
    reader: AafReader,
    comp: Composition,
    audio: Vec<AudioRef>,
    essence: BTreeMap<usize, Essence>,
}

impl std::fmt::Debug for AafFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AafFile")
            .field("composition", &self.comp.name)
            .field("audio", &self.audio.len())
            .finish_non_exhaustive()
    }
}

impl AafFile {
    /// Opens the AAF at `path` ([`AafReader::open`]) and reads its
    /// composition. Resolves every locator with [`locate`] (against
    /// `media_folder` too) as it opens; nothing is written.
    pub fn open(
        path: &Path,
        media_folder: Option<&Path>,
        limits: &AafLimits,
        cancel: &AtomicBool,
    ) -> Result<AafFile, IoError> {
        let reader = AafReader::open(path, limits, cancel)?;
        let aaf_dir = path.parent().unwrap_or(Path::new("."));
        let mut rd = Rd {
            s: reader.store(),
            reader: &reader,
            limits,
            cancel,
            report: reader.report().clone(),
            work: 0,
            mobs: HashMap::new(),
            essence: HashMap::new(),
            aaf_dir,
            media_folder,
            audio: Vec::new(),
            essences: BTreeMap::new(),
            by_file: HashMap::new(),
            missing: HashMap::new(),
            memo: HashMap::new(),
            mob_info: HashMap::new(),
            slot_index: HashMap::new(),
        };
        let comp = rd.composition()?;
        let audio = std::mem::take(&mut rd.audio);
        let essence = std::mem::take(&mut rd.essences);
        Ok(AafFile {
            comp,
            audio,
            essence,
            reader,
        })
    }

    /// The composition read.
    pub fn composition(&self) -> &Composition {
        &self.comp
    }

    /// The rate (6d Ruling 17: the one most files have among the project
    /// rates, else 48 kHz), the track and file counts, and where it came
    /// from.
    pub fn info(&self) -> AafInfo {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        AafInfo {
            sample_rate: majority_rate(self.audio.iter().map(|a| a.sample_rate)),
            tracks: count(self.comp.slots.len()),
            audio_files: count(self.audio.len()),
            offline: count(
                self.audio
                    .iter()
                    .filter(|a| matches!(a.source, AudioSource::Offline(_)))
                    .count(),
            ),
            from_vizdaw: self.comp.from_vizdaw,
        }
    }
}

/// Embedded essence → [`AudioSource::Entry`] (`"<master mob name>.wav"` or
/// `".aif"`), which [`AudioPackage::extract`] finds by the ref's key: a
/// `WAVEDescriptor` or `AIFCDescriptor` essence is streamed as it is, and a
/// `PCMDescriptor`'s raw data gets a 44-byte WAV header first. Located
/// files are [`AudioSource::External`] (the resolved path); MXF and the
/// rest are [`AudioSource::Offline`].
impl AudioPackage for AafFile {
    fn audio(&self) -> &[AudioRef] {
        &self.audio
    }

    /// As [`crate::dawproject::package::DpPackage::needed_bytes`]: each
    /// embedded file's copy plus one converted copy per file read; offline
    /// files count nothing.
    fn needed_bytes(&self, depth_bytes: u32, cancel: &AtomicBool) -> Result<u64, IoError> {
        let mut total = 0u64;
        for a in &self.audio {
            if cancel.load(Ordering::Relaxed) {
                return Err(IoError::Cancelled);
            }
            let (size, copied) = match &a.source {
                AudioSource::Entry(_) => (
                    self.essence
                        .get(&a.key)
                        .map(|e| e.len.saturating_add(e.header.len() as u64)),
                    true,
                ),
                AudioSource::External(p) => (external_size(p), false),
                AudioSource::Offline(_) => (None, false),
            };
            let Some(size) = size else { continue };
            total = total.saturating_add(needed_for(a, size, copied, depth_bytes));
        }
        Ok(total)
    }

    fn extract(&self, a: &AudioRef, to: &Path, cancel: &AtomicBool) -> Result<u64, IoError> {
        match &a.source {
            AudioSource::Entry(_) => {
                let e = self
                    .essence
                    .get(&a.key)
                    .ok_or_else(|| IoError::Refused(format!("{}: {NO_ESSENCE}", a.name)))?;
                self.reader
                    .copy_stream_after(e.obj, &e.stream, &e.header, to, e.len, cancel)
            }
            AudioSource::External(p) => Err(IoError::Refused(format!(
                "{} is outside the AAF file; it is read where it is",
                core_media::export::display_name(p)
            ))),
            AudioSource::Offline(why) => Err(IoError::Refused(format!("{}: {why}", a.name))),
        }
    }
}

/// The file a locator names, when it may be read (Review Focus 2):
/// - a `file://` URL (its host ignored, percent-decoded), or a path, made
///   absolute against the AAF's folder `aaf_dir` when relative;
/// - any other scheme is refused, "it is a URL, not a file";
/// - when that file cannot be read, the same file name inside
///   `media_folder`;
/// - checked by [`external_file`] (a regular file, one link at most).
///
/// Nothing is written. The error is the first reason, as a sentence
/// fragment.
pub fn locate(url: &str, aaf_dir: &Path, media_folder: Option<&Path>) -> Result<PathBuf, String> {
    let text = match strip_prefix_ci(url, "file:") {
        Some(rest) => match rest.strip_prefix("//") {
            // The host, up to the next '/', is dropped.
            Some(r) => percent_decode(r.find('/').and_then(|i| r.get(i..)).unwrap_or("")),
            None => percent_decode(rest),
        },
        None if is_url(url) => return Err(URL_NOT_FILE.into()),
        None => percent_decode(url),
    };
    if text.is_empty() {
        return Err(NO_PLACE.into());
    }
    let p = PathBuf::from(&text);
    let first = if p.is_absolute() { p } else { aaf_dir.join(p) };
    let why = match external_file(&first) {
        Ok(real) => return Ok(real),
        Err(why) => why,
    };
    if let (Some(dir), Some(name)) = (media_folder, file_name(&text))
        && let Ok(real) = external_file(&dir.join(name))
    {
        return Ok(real);
    }
    Err(why)
}

/// `s` without `prefix`, compared ignoring ASCII case.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| s.get(prefix.len()..))
        .flatten()
}

/// `%XX` sequences decoded (others kept), as UTF-8 with U+FFFD for what is
/// not.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16);
    while let Some(&c) = b.get(i) {
        if c == b'%'
            && let (Some(h), Some(l)) = (
                b.get(i + 1).copied().and_then(hex),
                b.get(i + 2).copied().and_then(hex),
            )
        {
            out.push(u8::try_from(h * 16 + l).unwrap_or(b'?'));
            i += 3;
            continue;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The last part of a path written with `/` or `\`, when it names a file.
fn file_name(text: &str) -> Option<&str> {
    let n = text.rsplit(['/', '\\']).next()?;
    (!n.is_empty() && n != "." && n != "..").then_some(n)
}

/// `v` edit units at `from` as edit units at `to`, rounding half up, with
/// `i128` and saturating at the `i64` range. Rates are positive.
fn rescale(v: i64, from: (i32, i32), to: (i32, i32)) -> i64 {
    if from == to {
        return v;
    }
    let num = i128::from(v) * i128::from(from.1) * i128::from(to.0);
    let den = i128::from(from.0) * i128::from(to.1);
    if den <= 0 {
        return 0;
    }
    let q = (num * 2 + den).div_euclid(den * 2);
    i64::try_from(q).unwrap_or(if q < 0 { i64::MIN } else { i64::MAX })
}

/// A rate as an edit rate.
fn per_second(rate: u32) -> (i32, i32) {
    (i32::try_from(rate).unwrap_or(i32::MAX).max(1), 1)
}

/// A usable edit rate: both parts positive.
fn good_rate(r: Option<(i32, i32)>) -> Option<(i32, i32)> {
    r.filter(|r| r.0 > 0 && r.1 > 0)
}

/// An AUID from a weak reference's identification, or from data.
fn weak_id(p: Option<&Prop>) -> Option<Auid> {
    match p? {
        Prop::Weak(w) => <[u8; 16]>::try_from(w.id.as_slice()).ok().map(Auid),
        p => dict::auid(p),
    }
}

/// An unsigned integer of 1, 2, 4 or 8 bytes.
fn uint(p: Option<&Prop>) -> Option<u64> {
    let Some(Prop::Data(d)) = p else { return None };
    match d.len() {
        1 => d.first().map(|b| u64::from(*b)),
        2 => <[u8; 2]>::try_from(d.as_slice())
            .ok()
            .map(|b| u64::from(u16::from_le_bytes(b))),
        4 => <[u8; 4]>::try_from(d.as_slice())
            .ok()
            .map(|b| u64::from(u32::from_le_bytes(b))),
        8 => <[u8; 8]>::try_from(d.as_slice())
            .ok()
            .map(u64::from_le_bytes),
        _ => None,
    }
}

/// A rational held directly or as an indirect value (byte order, type
/// AUID, then the value), as `f64`. A zero or negative denominator, or a
/// result that is not finite, is `None`.
fn indirect_ratio(p: Option<&Prop>) -> Option<f64> {
    let Some(Prop::Data(d)) = p else { return None };
    let v = match d.len() {
        8 => d.as_slice(),
        25 if matches!(d.first(), Some(0x4C)) => d.get(17..)?,
        _ => return None,
    };
    let (n, den) = dict::rational(&Prop::Data(v.to_vec()))?;
    let x = f64::from(n) / f64::from(den);
    x.is_finite().then_some(x)
}

/// A string held as an indirect value, or directly.
fn indirect_text(p: Option<&Prop>, limits: &AafLimits) -> String {
    let Some(Prop::Data(d)) = p else {
        return String::new();
    };
    let body = if d.len() >= 17 && d.len() % 2 == 1 && matches!(d.first(), Some(0x4C)) {
        d.get(17..).unwrap_or(&[])
    } else {
        d.as_slice()
    };
    dict::string(&Prop::Data(body.to_vec()), limits).0
}

/// One flattened piece of a sequence.
enum Piece {
    /// A gap.
    Gap(i64),
    /// A transition: its length and shape.
    Trans(i64, Shape),
    /// A source clip: the object, its length, its gain and whether a gain
    /// around it varies.
    Clip(ObjId, i64, f64, bool),
}

/// Where a source clip leads.
enum Target {
    /// A file `SourceMob`: its key, the channel, and the hops' offsets
    /// (value, edit rate), which add up to the clip's offset.
    File(usize, Option<u16>, Vec<(i64, (i32, i32))>, Option<Arc<str>>),
    /// A sound slot of a sub-composition, its mob ID, its edit rate and
    /// its `Origin`.
    Comp(ObjId, MobId, (i32, i32), i64),
    /// Nowhere: the ID it named and why.
    Missing(Vec<u8>, String, Option<Arc<str>>),
}

/// A mob, read once: its class, its slots by `SlotID` (with each sound
/// slot's position among the sound slots), and its cleaned name.
struct MobInfo {
    class: Option<Auid>,
    slots: HashMap<u64, (ObjId, Option<usize>)>,
    sound: usize,
    name: Option<Arc<str>>,
}

/// A master slot's segments, read once: each clip's (start, end, clip)
/// in slot edit units, in order; fillers are the gaps between.
struct SlotIndex {
    parts: Vec<(i64, i64, ObjId)>,
}

/// The read's state.
struct Rd<'a> {
    s: &'a Store,
    reader: &'a AafReader,
    limits: &'a AafLimits,
    cancel: &'a AtomicBool,
    report: Report,
    /// Work units counted against the budget.
    work: usize,
    mobs: HashMap<MobId, ObjId>,
    essence: HashMap<MobId, ObjId>,
    aaf_dir: &'a Path,
    media_folder: Option<&'a Path>,
    audio: Vec<AudioRef>,
    essences: BTreeMap<usize, Essence>,
    by_file: HashMap<MobId, usize>,
    missing: HashMap<Vec<u8>, usize>,
    /// Sub-composition slots already read, in their own edit units.
    memo: HashMap<ObjId, Vec<Clip>>,
    /// Mobs already read.
    mob_info: HashMap<ObjId, Arc<MobInfo>>,
    /// Master slots already indexed.
    slot_index: HashMap<ObjId, Arc<SlotIndex>>,
}

impl Rd<'_> {
    fn prop(&self, id: ObjId, p: u16) -> Option<&Prop> {
        self.s.prop(id, p)
    }

    fn class(&self, id: ObjId) -> Option<Auid> {
        self.s.get(id).map(|o| o.class)
    }

    fn strong(&self, id: ObjId, p: u16) -> Option<ObjId> {
        match self.prop(id, p)? {
            Prop::Strong(c) => Some(*c),
            _ => None,
        }
    }

    fn list(&self, id: ObjId, p: u16) -> Vec<ObjId> {
        match self.prop(id, p) {
            Some(Prop::StrongVec(v) | Prop::StrongSet(v)) => v.clone(),
            Some(Prop::Strong(c)) => vec![*c],
            _ => Vec::new(),
        }
    }

    fn len_of(&self, id: ObjId) -> i64 {
        self.prop(id, pid::COMPONENT_LENGTH)
            .and_then(dict::i64_at)
            .unwrap_or(0)
            .max(0)
    }

    fn text(&self, id: ObjId, p: u16) -> String {
        self.prop(id, p)
            .map(|v| dict::string(v, self.limits).0)
            .unwrap_or_default()
    }

    /// A name for VizDAW: cleaned and cut to 4 096 bytes (listed when cut).
    fn name(&mut self, raw: &str) -> String {
        let (n, cut) = clean_name(raw);
        if cut {
            self.report.add(ItemKind::Changed, NAMES_CUT, None);
        }
        n
    }

    /// Counts `n` units of work; past the budget the file is refused, and
    /// the cancel flag is looked at every [`CANCEL_EVERY`].
    fn tick(&mut self, n: usize) -> Result<(), IoError> {
        let before = self.work / CANCEL_EVERY;
        self.work = self.work.saturating_add(n);
        if self.work > self.limits.max_objects.saturating_mul(4) {
            return Err(IoError::Refused(TOO_LARGE.into()));
        }
        if self.work / CANCEL_EVERY != before && self.cancel.load(Ordering::Relaxed) {
            return Err(IoError::Cancelled);
        }
        Ok(())
    }

    /// The whole read.
    fn composition(&mut self) -> Result<Composition, IoError> {
        let s = self.s;
        let header = self
            .strong(s.root, pid::ROOT_HEADER)
            .ok_or_else(|| IoError::Refused(NOT_AAF.into()))?;
        let from_vizdaw = self
            .list(header, pid::HEADER_IDENTIFICATION_LIST)
            .into_iter()
            .any(|i| self.text(i, pid::IDENTIFICATION_PRODUCT_NAME) == "VizDAW");
        if from_vizdaw {
            self.report.add(ItemKind::Note, FROM_VIZDAW, None);
        }
        let content = self.strong(header, pid::HEADER_CONTENT);
        let mobs = content.map_or_else(Vec::new, |c| self.list(c, pid::CONTENT_STORAGE_MOBS));
        for &m in &mobs {
            if let Some(id) = self.prop(m, pid::MOB_MOB_ID).and_then(dict::mob_id) {
                self.mobs.entry(id).or_insert(m);
            }
        }
        if let Some(c) = content {
            for e in self.list(c, pid::CONTENT_STORAGE_ESSENCE_DATA) {
                if let Some(id) = self
                    .prop(e, pid::ESSENCE_DATA_MOB_ID)
                    .and_then(dict::mob_id)
                {
                    self.essence.entry(id).or_insert(e);
                }
            }
        }
        let comp = self.choose(&mobs)?;
        self.read_comp(comp, from_vizdaw)
    }

    /// Ruling 1's composition.
    fn choose(&mut self, mobs: &[ObjId]) -> Result<ObjId, IoError> {
        let comps: Vec<ObjId> = mobs
            .iter()
            .copied()
            .filter(|m| self.class(*m) == Some(class::COMPOSITION_MOB))
            .collect();
        let top: Vec<ObjId> = comps
            .iter()
            .copied()
            .filter(|m| weak_id(self.prop(*m, pid::MOB_USAGE_CODE)) == Some(def::USAGE_TOP_LEVEL))
            .collect();
        let top_count = top.len();
        let candidates = if top.is_empty() {
            let mut named: HashSet<MobId> = HashSet::new();
            for (i, o) in self.s.objects.iter().enumerate() {
                if i % CANCEL_EVERY == 0 && self.cancel.load(Ordering::Relaxed) {
                    return Err(IoError::Cancelled);
                }
                if o.class == class::SOURCE_CLIP
                    && let Some(id) = o
                        .props
                        .get(&pid::SOURCE_REFERENCE_SOURCE_ID)
                        .and_then(dict::mob_id)
                {
                    named.insert(id);
                }
            }
            let free: Vec<ObjId> = comps
                .iter()
                .copied()
                .filter(|m| {
                    self.prop(*m, pid::MOB_MOB_ID)
                        .and_then(dict::mob_id)
                        .is_none_or(|id| !named.contains(&id))
                })
                .collect();
            if free.is_empty() { comps.clone() } else { free }
        } else {
            top
        };
        let first = candidates
            .first()
            .copied()
            .ok_or_else(|| IoError::Refused(NO_COMPOSITION.into()))?;
        // One top-level composition is the file's own answer; any other
        // choice among several is noted.
        if comps.len() > 1 && top_count != 1 {
            self.report.add(ItemKind::Note, SEVERAL_COMPOSITIONS, None);
        }
        Ok(first)
    }

    /// The chosen composition mob.
    fn read_comp(&mut self, mob: ObjId, from_vizdaw: bool) -> Result<Composition, IoError> {
        let raw = self.text(mob, pid::MOB_NAME);
        let name = self.name(&raw);
        let slots = self.list(mob, pid::MOB_SLOTS);
        // The composition's rate: its first audio slot's.
        let mut edit_rate = None;
        for &sl in &slots {
            if self.class(sl) == Some(class::TIMELINE_MOB_SLOT) && self.is_sound(sl) {
                edit_rate = good_rate(self.slot_rate(sl));
                if edit_rate.is_some() {
                    break;
                }
            }
        }
        let edit_rate = edit_rate.unwrap_or(per_second(DEFAULT_RATE));
        let mut out = Composition {
            name,
            edit_rate,
            start_tc: None,
            slots: Vec::new(),
            markers: Vec::new(),
            comments: Vec::new(),
            from_vizdaw,
            report: Report::default(),
        };
        let mut chain = vec![self.prop(mob, pid::MOB_MOB_ID).and_then(dict::mob_id)];
        for sl in slots {
            self.tick(1)?;
            let cls = self.class(sl);
            if cls == Some(class::EVENT_MOB_SLOT) {
                self.markers(sl, edit_rate, &mut out.markers);
                continue;
            }
            if cls != Some(class::TIMELINE_MOB_SLOT) {
                self.report.add(ItemKind::LeftOut, NON_AUDIO, None);
                continue;
            }
            let dd = self.slot_kind(sl);
            if dd == Some(def::DATA_DEF_TIMECODE) || dd == Some(def::DATA_DEF_LEGACY_TIMECODE) {
                if out.start_tc.is_none() {
                    out.start_tc = self.timecode(sl);
                }
                continue;
            }
            if !self.is_sound(sl) {
                self.report.add(ItemKind::LeftOut, NON_AUDIO, None);
                continue;
            }
            let Some(rate) = good_rate(self.slot_rate(sl)) else {
                self.report.add(ItemKind::LeftOut, BAD_RATES, None);
                continue;
            };
            if out.slots.len() >= self.limits.max_import_tracks {
                return Err(too_many_tracks(self.limits.max_import_tracks));
            }
            let raw = self.text(sl, pid::MOB_SLOT_SLOT_NAME);
            let mut name = self.name(&raw);
            if name.is_empty() {
                name = format!("Track {}", out.slots.len().saturating_add(1));
            }
            let (seg, gain, pan) = self.track_curves(sl);
            let clips = match seg {
                Some(seg) => self.flatten(seg, rate, &mut chain)?,
                None => Vec::new(),
            };
            let clips: Vec<Clip> = clips
                .into_iter()
                .map(|mut c| {
                    c.start = rescale(c.start, rate, edit_rate);
                    c.length = rescale(c.length, rate, edit_rate);
                    c
                })
                .filter(|c| c.length > 0)
                .collect();
            let channels = clips
                .iter()
                .map(|c| match c.channel {
                    Some(_) => 1,
                    None => self.audio.get(c.media).map_or(1, |a| a.channels.max(1)),
                })
                .max()
                .unwrap_or(1);
            let (gain, pan) = (
                self.curve(gain, rate, edit_rate),
                self.curve(pan, rate, edit_rate),
            );
            out.slots.push(Slot {
                name,
                channels,
                clips,
                gain,
                pan,
            });
        }
        out.comments = self.comments(mob);
        out.report = std::mem::take(&mut self.report);
        Ok(out)
    }

    fn slot_rate(&self, sl: ObjId) -> Option<(i32, i32)> {
        self.prop(sl, pid::TIMELINE_MOB_SLOT_EDIT_RATE)
            .or_else(|| self.prop(sl, pid::EVENT_MOB_SLOT_EDIT_RATE))
            .and_then(dict::rational)
    }

    /// The data kind of a slot's segment.
    fn slot_kind(&self, sl: ObjId) -> Option<Auid> {
        let seg = self.strong(sl, pid::MOB_SLOT_SEGMENT)?;
        weak_id(self.prop(seg, pid::COMPONENT_DATA_DEFINITION))
    }

    fn is_sound(&self, sl: ObjId) -> bool {
        matches!(self.slot_kind(sl), Some(d) if d == def::DATA_DEF_SOUND || d == def::DATA_DEF_LEGACY_SOUND)
    }

    /// The first `Timecode` of a timecode slot.
    fn timecode(&self, sl: ObjId) -> Option<(i64, u16, bool)> {
        let seg = self.strong(sl, pid::MOB_SLOT_SEGMENT)?;
        let tc = if self.class(seg) == Some(class::TIMECODE) {
            seg
        } else {
            self.list(seg, pid::SEQUENCE_COMPONENTS)
                .into_iter()
                .find(|c| self.class(*c) == Some(class::TIMECODE))?
        };
        let start = self.prop(tc, pid::TIMECODE_START).and_then(dict::i64_at)?;
        let fps = uint(self.prop(tc, pid::TIMECODE_FPS)).and_then(|f| u16::try_from(f).ok())?;
        let drop = uint(self.prop(tc, pid::TIMECODE_DROP)).is_some_and(|d| d != 0);
        Some((start, fps, drop))
    }

    /// An event slot's markers, at the composition's rate.
    fn markers(&mut self, sl: ObjId, to: (i32, i32), out: &mut Vec<AafMarker>) {
        let Some(rate) = good_rate(self.slot_rate(sl)) else {
            self.report.add(ItemKind::LeftOut, BAD_RATES, None);
            return;
        };
        let Some(seg) = self.strong(sl, pid::MOB_SLOT_SEGMENT) else {
            return;
        };
        let events = if self.class(seg) == Some(class::SEQUENCE) {
            self.list(seg, pid::SEQUENCE_COMPONENTS)
        } else {
            vec![seg]
        };
        for e in events {
            let Some(pos) = self.prop(e, pid::EVENT_POSITION).and_then(dict::i64_at) else {
                continue;
            };
            let raw = self.text(e, pid::EVENT_COMMENT);
            let mut name = self.name(&raw);
            if name.is_empty() {
                name = "Marker".into();
            }
            out.push(AafMarker {
                position: rescale(pos, rate, to),
                name,
            });
        }
    }

    /// The mob's tagged comments.
    fn comments(&mut self, mob: ObjId) -> Vec<(String, String)> {
        self.list(mob, pid::MOB_USER_COMMENTS)
            .into_iter()
            .map(|t| {
                (
                    self.text(t, pid::TAGGED_VALUE_NAME),
                    indirect_text(self.prop(t, pid::TAGGED_VALUE_VALUE), self.limits),
                )
            })
            .collect()
    }

    /// The operation of a group.
    fn op_of(&self, g: ObjId) -> Option<Auid> {
        weak_id(self.prop(g, pid::OPERATION_GROUP_OPERATION))
    }

    /// The group's parameter `want` (or its only one).
    fn param(&self, g: ObjId, want: Auid) -> Option<ObjId> {
        let ps = self.list(g, pid::OPERATION_GROUP_PARAMETERS);
        ps.iter()
            .copied()
            .find(|p| weak_id(self.prop(*p, pid::PARAMETER_DEFINITION)) == Some(want))
            .or_else(|| (ps.len() == 1).then(|| ps.first().copied()).flatten())
    }

    /// Peels the gain and pan groups around a slot's whole sequence:
    /// the segment left, and the gain and pan parameters with the group's
    /// length.
    #[allow(clippy::type_complexity)]
    fn track_curves(
        &mut self,
        sl: ObjId,
    ) -> (Option<ObjId>, Option<(ObjId, i64)>, Option<(ObjId, i64)>) {
        let mut seg = self.strong(sl, pid::MOB_SLOT_SEGMENT);
        let (mut gain, mut pan) = (None, None);
        while let Some(g) = seg {
            if self.class(g) != Some(class::OPERATION_GROUP) {
                break;
            }
            let op = self.op_of(g);
            let is_gain = op == Some(def::OPERATION_DEF_MONO_AUDIO_GAIN);
            let is_pan = op == Some(def::OPERATION_DEF_MONO_AUDIO_PAN);
            let input = self
                .list(g, pid::OPERATION_GROUP_INPUT_SEGMENTS)
                .first()
                .copied();
            let around_clip = input.is_some_and(|i| self.class(i) == Some(class::SOURCE_CLIP));
            if !(is_gain || is_pan) || around_clip {
                break;
            }
            let (want, slot) = if is_gain {
                (def::PARAMETER_DEF_AMPLITUDE, &mut gain)
            } else {
                (def::PARAMETER_DEF_PAN, &mut pan)
            };
            let len = self.len_of(g);
            match (slot.is_none(), self.param(g, want)) {
                (true, Some(p)) => *slot = Some((p, len)),
                (true, None) => {}
                (false, _) => self.report.add(ItemKind::LeftOut, EFFECTS, None),
            }
            seg = input;
        }
        (seg, gain, pan)
    }

    /// A track curve from its parameter, at the composition's rate.
    fn curve(&mut self, p: Option<(ObjId, i64)>, rate: (i32, i32), to: (i32, i32)) -> Curve {
        let Some((p, len)) = p else {
            return Curve::default();
        };
        if self.class(p) == Some(class::CONSTANT_VALUE) {
            return match indirect_ratio(self.prop(p, pid::CONSTANT_VALUE_VALUE)) {
                Some(v) => Curve {
                    constant: Some(v),
                    points: Vec::new(),
                },
                None => {
                    self.report.add(ItemKind::LeftOut, BAD_VALUES, None);
                    Curve::default()
                }
            };
        }
        let cps = self.list(p, pid::VARYING_VALUE_POINT_LIST);
        let mut points = Vec::new();
        for (i, c) in cps.into_iter().enumerate() {
            if i >= self.limits.max_control_points {
                self.report.add(ItemKind::LeftOut, POINTS_CUT, None);
                break;
            }
            let t = self
                .prop(c, pid::CONTROL_POINT_TIME)
                .and_then(dict::rational);
            let v = indirect_ratio(self.prop(c, pid::CONTROL_POINT_VALUE));
            match (t, v) {
                (Some((n, d)), Some(v)) => {
                    let at = i128::from(len) * i128::from(n);
                    let d = i128::from(d);
                    let at = (at * 2 + d).div_euclid(d * 2);
                    let at = i64::try_from(at).unwrap_or(0);
                    points.push((rescale(at, rate, to), v));
                }
                _ => self.report.add(ItemKind::LeftOut, BAD_VALUES, None),
            }
        }
        Curve {
            constant: None,
            points,
        }
    }

    /// A gain group's value: (factor, varying).
    fn gain_of(&mut self, g: ObjId) -> (f64, bool) {
        let Some(p) = self.param(g, def::PARAMETER_DEF_AMPLITUDE) else {
            return (1.0, false);
        };
        let (v, varying) = if self.class(p) == Some(class::VARYING_VALUE) {
            let first = self.list(p, pid::VARYING_VALUE_POINT_LIST).first().copied();
            (
                first.and_then(|c| indirect_ratio(self.prop(c, pid::CONTROL_POINT_VALUE))),
                true,
            )
        } else {
            (
                indirect_ratio(self.prop(p, pid::CONSTANT_VALUE_VALUE)),
                false,
            )
        };
        match v {
            Some(v) if v >= 0.0 => (v, varying),
            _ => {
                self.report.add(ItemKind::LeftOut, BAD_VALUES, None);
                (1.0, varying)
            }
        }
    }

    /// A transition's shape: its `Level` curve's interpolation.
    fn shape_of(&self, t: ObjId) -> Shape {
        let interp = self
            .strong(t, pid::TRANSITION_OPERATION_GROUP)
            .and_then(|g| self.param(g, def::PARAMETER_DEF_LEVEL))
            .filter(|p| self.class(*p) == Some(class::VARYING_VALUE))
            .and_then(|p| weak_id(self.prop(p, pid::VARYING_VALUE_INTERPOLATION)));
        match interp {
            None => Shape::Linear,
            Some(i) if i == def::INTERPOLATION_DEF_LINEAR => Shape::Linear,
            Some(i) if i == def::INTERPOLATION_DEF_POWER => Shape::Power,
            Some(i) if i == def::INTERPOLATION_DEF_LOG => Shape::Log,
            Some(_) => Shape::Other,
        }
    }

    /// A segment flattened into pieces, with an explicit stack.
    fn pieces(&mut self, seg: ObjId) -> Result<Vec<Piece>, IoError> {
        let mut out = Vec::new();
        let mut stack = vec![(seg, 1.0f64, false)];
        while let Some((o, gain, varying)) = stack.pop() {
            self.tick(1)?;
            let len = self.len_of(o);
            match self.class(o) {
                Some(c) if c == class::SEQUENCE => {
                    let kids = self.list(o, pid::SEQUENCE_COMPONENTS);
                    stack.extend(kids.into_iter().rev().map(|k| (k, gain, varying)));
                }
                Some(c) if c == class::FILLER => out.push(Piece::Gap(len)),
                Some(c) if c == class::SOURCE_CLIP => out.push(Piece::Clip(o, len, gain, varying)),
                Some(c) if c == class::TRANSITION => out.push(Piece::Trans(len, self.shape_of(o))),
                Some(c) if c == class::OPERATION_GROUP => {
                    let op = self.op_of(o);
                    let (mut g, mut v) = (gain, varying);
                    if op == Some(def::OPERATION_DEF_MONO_AUDIO_GAIN) {
                        let (f, var) = self.gain_of(o);
                        g *= f;
                        v |= var;
                    } else if op == Some(def::OPERATION_DEF_MONO_AUDIO_PAN) {
                        self.report.add(ItemKind::LeftOut, CLIP_PAN, None);
                    } else {
                        self.report.add(ItemKind::LeftOut, EFFECTS, None);
                    }
                    match self
                        .list(o, pid::OPERATION_GROUP_INPUT_SEGMENTS)
                        .first()
                        .copied()
                    {
                        Some(i) => stack.push((i, g, v)),
                        None => out.push(Piece::Gap(len)),
                    }
                }
                Some(c) if c == class::SELECTOR => match self.strong(o, pid::SELECTOR_SELECTED) {
                    Some(sel) => {
                        self.report.add(ItemKind::Note, SELECTED, None);
                        stack.push((sel, gain, varying));
                    }
                    None => out.push(Piece::Gap(len)),
                },
                Some(c) if c == class::ESSENCE_GROUP => {
                    match self.list(o, pid::ESSENCE_GROUP_CHOICES).first().copied() {
                        Some(ch) => {
                            self.report.add(ItemKind::Note, FIRST_CHOICE, None);
                            stack.push((ch, gain, varying));
                        }
                        None => out.push(Piece::Gap(len)),
                    }
                }
                Some(c) if c == class::NESTED_SCOPE => {
                    let slots = self.list(o, pid::NESTED_SCOPE_SLOTS);
                    if slots.len() > 1 {
                        self.report.add(ItemKind::LeftOut, SCOPES, None);
                    }
                    match slots.last().copied() {
                        Some(last) => stack.push((last, gain, varying)),
                        None => out.push(Piece::Gap(len)),
                    }
                }
                Some(c) if c == class::SCOPE_REFERENCE => {
                    self.report.add(ItemKind::LeftOut, SCOPES, None);
                    out.push(Piece::Gap(len));
                }
                _ => {
                    self.report.add(ItemKind::LeftOut, UNKNOWN_PARTS, None);
                    out.push(Piece::Gap(len));
                }
            }
        }
        Ok(out)
    }

    /// A slot's segment as clips, in that slot's edit units. `chain` holds
    /// the compositions being read, for the loop check.
    fn flatten(
        &mut self,
        seg: ObjId,
        rate: (i32, i32),
        chain: &mut Vec<Option<MobId>>,
    ) -> Result<Vec<Clip>, IoError> {
        let pieces = self.pieces(seg)?;
        let mut clips: Vec<Clip> = Vec::new();
        let mut pos: i64 = 0;
        // The last piece placed: None, a gap, or the index of a clip.
        let mut last: Option<Option<usize>> = None;
        let mut pending: Option<(i64, Shape, Option<Option<usize>>)> = None;
        for p in pieces {
            match p {
                Piece::Gap(len) => {
                    if let Some((l, sh, Some(Some(i)))) = pending.take() {
                        self.set_fade(&mut clips, i, l, sh, rate, FadeSide::Out);
                    }
                    pos = pos.saturating_add(len);
                    last = Some(None);
                }
                Piece::Trans(len, sh) => {
                    pos = pos.saturating_sub(len);
                    pending = Some((len, sh, last));
                }
                Piece::Clip(o, len, gain, varying) => {
                    let made = self.place(o, pos, len, gain, varying, rate, chain)?;
                    let first = clips.len();
                    clips.extend(made);
                    if let Some((l, sh, before)) = pending.take()
                        && first < clips.len()
                    {
                        let side = match before {
                            Some(Some(_)) => FadeSide::Cross,
                            _ => FadeSide::In,
                        };
                        self.set_fade(&mut clips, first, l, sh, rate, side);
                    }
                    pos = pos.saturating_add(len);
                    last = Some(clips.len().checked_sub(1).filter(|_| first < clips.len()));
                }
            }
        }
        if let Some((l, sh, Some(Some(i)))) = pending {
            self.set_fade(&mut clips, i, l, sh, rate, FadeSide::Out);
        }
        Ok(clips)
    }

    /// Sets a transition's fade on clip `i`, in frames at its media's rate.
    fn set_fade(
        &self,
        clips: &mut [Clip],
        i: usize,
        len: i64,
        shape: Shape,
        rate: (i32, i32),
        side: FadeSide,
    ) {
        let Some(c) = clips.get_mut(i) else { return };
        let to = self.media_rate(c.media).unwrap_or(rate);
        let frames = u64::try_from(rescale(len, rate, to)).unwrap_or(0);
        if frames == 0 {
            return;
        }
        let f = Some(Fade { frames, shape });
        match side {
            FadeSide::In => c.fade_in = f,
            FadeSide::Out => c.fade_out = f,
            FadeSide::Cross => c.crossfade_in = f,
        }
    }

    fn media_rate(&self, key: usize) -> Option<(i32, i32)> {
        self.audio
            .get(key)
            .filter(|a| a.sample_rate > 0)
            .map(|a| per_second(a.sample_rate))
    }

    /// The clips one `SourceClip` plays at `pos` in a slot of `rate`.
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        o: ObjId,
        pos: i64,
        len: i64,
        gain: f64,
        varying: bool,
        rate: (i32, i32),
        chain: &mut Vec<Option<MobId>>,
    ) -> Result<Vec<Clip>, IoError> {
        let start = self
            .prop(o, pid::SOURCE_CLIP_START_TIME)
            .and_then(dict::i64_at)
            .unwrap_or(0);
        match self.resolve(o, rate, len)? {
            Target::File(key, channel, hops, name) => {
                let to = self.media_rate(key).unwrap_or(rate);
                let offset = hops
                    .iter()
                    .fold(0i64, |acc, (v, r)| acc.saturating_add(rescale(*v, *r, to)));
                let mut c = Clip {
                    start: pos,
                    length: len,
                    media: key,
                    channel,
                    source_offset: u64::try_from(offset).unwrap_or(0),
                    fade_in: None,
                    fade_out: None,
                    crossfade_in: None,
                    gain,
                    varying_gain: varying,
                    name,
                };
                c.fade_in = self.clip_fade(o, pid::SOURCE_CLIP_FADE_IN_LENGTH, rate, to);
                c.fade_out = self.clip_fade(o, pid::SOURCE_CLIP_FADE_OUT_LENGTH, rate, to);
                self.tick(CLIP_COST)?;
                Ok(vec![c])
            }
            Target::Missing(id, why, name) => {
                self.report.add(ItemKind::LeftOut, NO_SOURCE, Some(why));
                let end = start.saturating_add(len).max(0);
                let key = self.missing_ref(id, name.as_deref(), rate, end);
                let to = self.media_rate(key).unwrap_or(rate);
                self.tick(CLIP_COST)?;
                Ok(vec![Clip {
                    start: pos,
                    length: len,
                    media: key,
                    channel: None,
                    source_offset: u64::try_from(rescale(start, rate, to)).unwrap_or(0),
                    fade_in: None,
                    fade_out: None,
                    crossfade_in: None,
                    gain,
                    varying_gain: varying,
                    name,
                }])
            }
            Target::Comp(slot, id, sub_rate, origin) => {
                if chain.len() >= self.limits.max_mob_chain || chain.contains(&Some(id)) {
                    self.report.add(
                        ItemKind::LeftOut,
                        NO_SOURCE,
                        Some("a composition that plays itself".into()),
                    );
                    return Ok(Vec::new());
                }
                let inner = match self.memo.get(&slot) {
                    Some(c) => c.clone(),
                    None => {
                        let seg = self.strong(slot, pid::MOB_SLOT_SEGMENT);
                        chain.push(Some(id));
                        let r = match seg {
                            Some(seg) => self.flatten(seg, sub_rate, chain),
                            None => Ok(Vec::new()),
                        };
                        chain.pop();
                        let r = r?;
                        self.memo.insert(slot, r.clone());
                        r
                    }
                };
                self.tick(inner.len().saturating_mul(CLIP_COST))?;
                let ws = rescale(start, rate, sub_rate).saturating_add(origin);
                let we = ws.saturating_add(rescale(len, rate, sub_rate));
                let mut out = Vec::new();
                for mut c in inner {
                    let (cs, ce) = (c.start, c.start.saturating_add(c.length));
                    let (s, e) = (cs.max(ws), ce.min(we));
                    if s >= e {
                        continue;
                    }
                    let to = self.media_rate(c.media).unwrap_or(sub_rate);
                    let cut = s.saturating_sub(cs);
                    let moved = u64::try_from(rescale(cut, sub_rate, to)).unwrap_or(0);
                    c.source_offset = c.source_offset.saturating_add(moved);
                    if cut > 0 {
                        c.fade_in = None;
                        c.crossfade_in = None;
                    }
                    if e < ce {
                        c.fade_out = None;
                    }
                    c.start = pos.saturating_add(rescale(s.saturating_sub(ws), sub_rate, rate));
                    c.length = rescale(e.saturating_sub(s), sub_rate, rate);
                    c.gain *= gain;
                    c.varying_gain |= varying;
                    out.push(c);
                }
                Ok(out)
            }
        }
    }

    /// A `SourceClip` fade property, in frames at the media rate `to`.
    fn clip_fade(&self, o: ObjId, len_pid: u16, rate: (i32, i32), to: (i32, i32)) -> Option<Fade> {
        let len = self.prop(o, len_pid).and_then(dict::i64_at)?;
        let ty = uint(self.prop(o, len_pid.saturating_add(1)));
        let shape = match ty {
            Some(0) => return None,
            Some(2) => Shape::Power,
            _ => Shape::Linear,
        };
        let frames = u64::try_from(rescale(len, rate, to))
            .ok()
            .filter(|f| *f > 0)?;
        Some(Fade { frames, shape })
    }

    /// Follows a `SourceClip` of `len` in a slot of `rate` to its file, its
    /// sub-composition, or nowhere. Each hop reads its mob and slot from
    /// the memos, so a clip costs its chain's length, whatever the mobs
    /// hold.
    fn resolve(&mut self, clip: ObjId, rate: (i32, i32), len: i64) -> Result<Target, IoError> {
        let mut seen: HashSet<MobId> = HashSet::new();
        let mut hops: Vec<(i64, (i32, i32))> = Vec::new();
        let mut cur = clip;
        let mut cur_rate = rate;
        let mut len = len;
        let mut top_name: Option<Arc<str>> = None;
        let mut first = true;
        let mut channel: Option<u16> = None;
        loop {
            self.tick(1)?;
            let raw_id = match self.prop(cur, pid::SOURCE_REFERENCE_SOURCE_ID) {
                Some(Prop::Data(d)) if d.len() == 32 => d.clone(),
                _ => Vec::new(),
            };
            let missing = |why: &str, name: &Option<Arc<str>>| {
                Ok(Target::Missing(
                    raw_id.clone(),
                    why.to_owned(),
                    name.clone(),
                ))
            };
            let Some(id) = <[u8; 32]>::try_from(raw_id.as_slice()).ok().map(MobId) else {
                return missing("a clip names no source", &top_name);
            };
            if !seen.insert(id) || seen.len() > self.limits.max_mob_chain {
                return missing("its chain of sources loops or runs too deep", &top_name);
            }
            let Some(&mob) = self.mobs.get(&id) else {
                return missing("its source is not in the file", &top_name);
            };
            let info = self.mob_info(mob)?;
            let slot_id = uint(self.prop(cur, pid::SOURCE_REFERENCE_SOURCE_MOB_SLOT_ID));
            let start = self
                .prop(cur, pid::SOURCE_CLIP_START_TIME)
                .and_then(dict::i64_at)
                .unwrap_or(0);
            if first {
                top_name = info.name.clone();
                channel = self.channel_ids(cur);
            }
            let Some(&(slot, sound_pos)) = slot_id.and_then(|k| info.slots.get(&k)) else {
                return missing("its source track is not in the file", &top_name);
            };
            let Some(slot_rate) = good_rate(self.slot_rate(slot)) else {
                return missing("its source track's edit rate could not be read", &top_name);
            };
            // Positions in a referenced slot count from its Origin.
            let origin = self
                .prop(slot, pid::TIMELINE_MOB_SLOT_ORIGIN)
                .and_then(dict::i64_at)
                .unwrap_or(0);
            match info.class {
                Some(c) if c == class::COMPOSITION_MOB => {
                    if first {
                        return Ok(Target::Comp(slot, id, slot_rate, origin));
                    }
                    return missing("its source is a composition inside a source", &top_name);
                }
                Some(c) if c == class::SOURCE_MOB => {
                    hops.push((start, cur_rate));
                    hops.push((origin, slot_rate));
                    let key = self.file_ref(mob, id, top_name.as_deref());
                    let wide = self.audio.get(key).is_some_and(|a| a.channels > 1);
                    if channel.is_none() && wide && info.sound > 1 {
                        let k = uint(self.prop(slot, pid::MOB_SLOT_PHYSICAL_TRACK_NUMBER))
                            .and_then(|n| n.checked_sub(1))
                            .or(sound_pos.map(|p| p as u64));
                        channel = k.and_then(|k| u16::try_from(k).ok());
                    }
                    if !wide {
                        channel = None;
                    }
                    return Ok(Target::File(key, channel, hops, top_name));
                }
                _ => {}
            }
            // A master mob (or another mob): the segment of its slot that
            // covers the clip's start.
            let pos = rescale(start, cur_rate, slot_rate).saturating_add(origin);
            let here = rescale(len, cur_rate, slot_rate);
            let index = self.slot_index(slot)?;
            let at = index.parts.partition_point(|p| p.0 <= pos);
            let part = at
                .checked_sub(1)
                .and_then(|i| index.parts.get(i))
                .filter(|p| pos < p.1)
                .copied();
            let Some((p_start, p_end, next)) = part else {
                return missing("its source track has nothing to play there", &top_name);
            };
            if pos.saturating_add(here) > p_end {
                self.report.add(ItemKind::LeftOut, SPANS_SOURCES, None);
            }
            hops.push((start, cur_rate));
            hops.push((origin.saturating_sub(p_start), slot_rate));
            cur = next;
            cur_rate = slot_rate;
            len = here;
            first = false;
        }
    }

    /// Mob `mob`, read once (its slots and name), charged to the budget
    /// by its slot count.
    fn mob_info(&mut self, mob: ObjId) -> Result<Arc<MobInfo>, IoError> {
        if let Some(i) = self.mob_info.get(&mob) {
            return Ok(Arc::clone(i));
        }
        let list = self.list(mob, pid::MOB_SLOTS);
        self.tick(list.len().saturating_add(1))?;
        let mut slots = HashMap::with_capacity(list.len());
        let mut sound = 0usize;
        for sl in list {
            let pos = (self.class(sl) == Some(class::TIMELINE_MOB_SLOT)).then(|| {
                sound = sound.saturating_add(1);
                sound - 1
            });
            if let Some(k) = uint(self.prop(sl, pid::MOB_SLOT_SLOT_ID)) {
                slots.entry(k).or_insert((sl, pos));
            }
        }
        let raw = self.text(mob, pid::MOB_NAME);
        let n = self.name(&raw);
        let info = Arc::new(MobInfo {
            class: self.class(mob),
            slots,
            sound,
            name: (!n.is_empty()).then(|| Arc::from(n)),
        });
        self.mob_info.insert(mob, Arc::clone(&info));
        Ok(info)
    }

    /// A master slot's clips, read once with an explicit stack: nested
    /// sequences are flattened, fillers and other components advance the
    /// position, transitions move it back, and an essence group plays its
    /// first choice. Charged to the budget by its components.
    fn slot_index(&mut self, slot: ObjId) -> Result<Arc<SlotIndex>, IoError> {
        if let Some(i) = self.slot_index.get(&slot) {
            return Ok(Arc::clone(i));
        }
        let mut parts = Vec::new();
        let mut pos: i64 = 0;
        let mut stack: Vec<ObjId> = self
            .strong(slot, pid::MOB_SLOT_SEGMENT)
            .into_iter()
            .collect();
        while let Some(o) = stack.pop() {
            self.tick(1)?;
            let len = self.len_of(o);
            match self.class(o) {
                Some(c) if c == class::SOURCE_CLIP => {
                    let end = pos.saturating_add(len);
                    parts.push((pos, end, o));
                    pos = end;
                }
                Some(c) if c == class::SEQUENCE => {
                    let kids = self.list(o, pid::SEQUENCE_COMPONENTS);
                    stack.extend(kids.into_iter().rev());
                }
                Some(c) if c == class::ESSENCE_GROUP => {
                    match self.list(o, pid::ESSENCE_GROUP_CHOICES).first().copied() {
                        Some(ch) => {
                            self.report.add(ItemKind::Note, FIRST_CHOICE, None);
                            stack.push(ch);
                        }
                        None => pos = pos.saturating_add(len),
                    }
                }
                Some(c) if c == class::TRANSITION => pos = pos.saturating_sub(len),
                _ => pos = pos.saturating_add(len),
            }
        }
        // Overlaps (after a transition) keep the later part from its start.
        parts.sort_by_key(|p| p.0);
        let index = Arc::new(SlotIndex { parts });
        self.slot_index.insert(slot, Arc::clone(&index));
        Ok(index)
    }

    /// The one channel a clip's `ChannelIDs` names (1-based), if one.
    fn channel_ids(&self, clip: ObjId) -> Option<u16> {
        let Some(Prop::Data(d)) = self.prop(clip, pid::SOURCE_REFERENCE_CHANNEL_IDS) else {
            return None;
        };
        let b = <[u8; 4]>::try_from(d.as_slice()).ok()?;
        u16::try_from(u32::from_le_bytes(b).checked_sub(1)?).ok()
    }

    /// The key of the offline ref for clips whose source is missing, one
    /// per ID named.
    fn missing_ref(
        &mut self,
        id: Vec<u8>,
        name: Option<&str>,
        rate: (i32, i32),
        end: i64,
    ) -> usize {
        if let Some(k) = self.missing.get(&id) {
            return *k;
        }
        let key = self.audio.len();
        let sample_rate = if rate.1 == 1 && (8_000..=384_000).contains(&rate.0) {
            u32::try_from(rate.0).unwrap_or(DEFAULT_RATE)
        } else {
            DEFAULT_RATE
        };
        let frames = u64::try_from(rescale(end, rate, per_second(sample_rate))).unwrap_or(0);
        self.audio.push(AudioRef {
            key,
            source: AudioSource::Offline(NO_SOURCE_WHY.into()),
            channels: 1,
            sample_rate,
            frames,
            name: name.map_or_else(
                || format!("Missing audio {}", key.saturating_add(1)),
                str::to_owned,
            ),
        });
        self.missing.insert(id, key);
        key
    }

    /// The key of the file source mob `mob`'s ref, made on first use.
    fn file_ref(&mut self, mob: ObjId, id: MobId, top_name: Option<&str>) -> usize {
        if let Some(k) = self.by_file.get(&id) {
            return *k;
        }
        let key = self.audio.len();
        let desc = self.strong(mob, pid::SOURCE_MOB_ESSENCE_DESCRIPTION);
        let cls = desc.and_then(|d| self.class(d));
        let p = |pid: u16| desc.and_then(|d| self.prop(d, pid));
        let rate = p(pid::FILE_DESCRIPTOR_SAMPLE_RATE)
            .and_then(dict::rational)
            .or_else(|| p(pid::SOUND_DESCRIPTOR_AUDIO_SAMPLING_RATE).and_then(dict::rational))
            .filter(|r| r.0 > 0)
            .map_or(0, |(n, d)| {
                u32::try_from((i64::from(n) * 2 + i64::from(d)) / (2 * i64::from(d))).unwrap_or(0)
            });
        let frames = p(pid::FILE_DESCRIPTOR_LENGTH)
            .and_then(dict::i64_at)
            .and_then(|f| u64::try_from(f).ok())
            .unwrap_or(0);
        let summary = match (
            p(pid::WAVE_DESCRIPTOR_SUMMARY),
            p(pid::AIFC_DESCRIPTOR_SUMMARY),
        ) {
            (Some(Prop::Data(d)), _) => Format::of_wav(d),
            (_, Some(Prop::Data(d))) => Format::of_aifc(d),
            _ => None,
        };
        let sound_slots = self
            .list(mob, pid::MOB_SLOTS)
            .into_iter()
            .filter(|s| self.class(*s) == Some(class::TIMELINE_MOB_SLOT))
            .count();
        let declared_ch =
            uint(p(pid::SOUND_DESCRIPTOR_CHANNELS)).and_then(|c| u16::try_from(c).ok());
        let bits = uint(p(pid::SOUND_DESCRIPTOR_QUANTIZATION_BITS))
            .and_then(|b| u16::try_from(b).ok())
            .or(summary.map(|f| f.bits))
            .unwrap_or(0);
        let channels = declared_ch
            .or(summary.map(|f| f.channels))
            .filter(|c| *c > 0)
            .unwrap_or_else(|| u16::try_from(sound_slots.max(1)).unwrap_or(1));
        let container = weak_id(p(pid::FILE_DESCRIPTOR_CONTAINER_FORMAT));
        let raw = if top_name.is_none() {
            self.text(mob, pid::MOB_NAME)
        } else {
            String::new()
        };
        let mut name = top_name.map_or_else(|| self.name(&raw), str::to_owned);
        if name.is_empty() {
            name = format!("Audio {}", key.saturating_add(1));
        }
        let mxf = container.is_some_and(|c| c.fields().0 == 0x0d01_0301);
        let source = if mxf {
            AudioSource::Offline(MXF.into())
        } else if let Some(&e) = self.essence.get(&id) {
            self.embedded(key, e, &name, cls, channels, bits, rate, frames, summary)
        } else {
            self.located(desc)
        };
        self.audio.push(AudioRef {
            key,
            source,
            channels,
            sample_rate: rate,
            frames,
            name,
        });
        self.by_file.insert(id, key);
        key
    }

    /// Embedded essence: an entry, or why it is offline.
    #[allow(clippy::too_many_arguments)]
    fn embedded(
        &mut self,
        key: usize,
        e: ObjId,
        name: &str,
        cls: Option<Auid>,
        channels: u16,
        bits: u16,
        rate: u32,
        frames: u64,
        summary: Option<Format>,
    ) -> AudioSource {
        let (ext, pcm) = match cls {
            Some(c) if c == class::WAVE_DESCRIPTOR => (".wav", false),
            Some(c) if c == class::AIFC_DESCRIPTOR => (".aif", false),
            Some(c) if c == class::PCM_DESCRIPTOR => (".wav", true),
            _ => return AudioSource::Offline(ODD_FORMAT.into()),
        };
        let Some(Prop::Stream(stream)) = self.prop(e, pid::ESSENCE_DATA_DATA).cloned() else {
            return AudioSource::Offline(NO_ESSENCE.into());
        };
        let Ok(len) = self.reader.stream_len(e, &stream) else {
            return AudioSource::Offline(NO_ESSENCE.into());
        };
        let bytes_per = u64::from(bits.div_ceil(8));
        let block = if pcm {
            Some(u64::from(channels).saturating_mul(bytes_per))
        } else {
            summary.map(|f| u64::from(f.block))
        };
        if let Some(b) = block
            && frames.saturating_mul(b) > len
        {
            return AudioSource::Offline(SHORT_ESSENCE.into());
        }
        let header = if pcm {
            if !(1..=32).contains(&bits) || channels == 0 || rate == 0 {
                return AudioSource::Offline(ODD_FORMAT.into());
            }
            match wav_header(channels, bits, rate, len) {
                Some(h) => h,
                None => return AudioSource::Offline(ODD_FORMAT.into()),
            }
        } else {
            Vec::new()
        };
        self.essences.insert(
            key,
            Essence {
                obj: e,
                stream,
                len,
                header,
            },
        );
        let stem = crate::names::safe_part(name, ext.len());
        AudioSource::Entry(format!("{stem}{ext}"))
    }

    /// External essence: the first locator that names a readable file.
    fn located(&mut self, desc: Option<ObjId>) -> AudioSource {
        let locs = desc.map_or_else(Vec::new, |d| self.list(d, pid::ESSENCE_DESCRIPTOR_LOCATOR));
        let mut why: Option<String> = None;
        for l in locs {
            let url = self.text(l, pid::NETWORK_LOCATOR_URL_STRING);
            if url.is_empty() {
                continue;
            }
            let path_part = url.split(['?', '#']).next().unwrap_or("");
            if path_part.to_ascii_lowercase().ends_with(".mxf") {
                return AudioSource::Offline(MXF.into());
            }
            match locate(&url, self.aaf_dir, self.media_folder) {
                Ok(p) => return AudioSource::External(p),
                Err(w) => {
                    why.get_or_insert(w);
                }
            }
        }
        AudioSource::Offline(why.unwrap_or_else(|| NO_PLACE.into()))
    }
}

/// Which fade a transition sets.
#[derive(Clone, Copy)]
enum FadeSide {
    In,
    Out,
    Cross,
}

/// What a descriptor's `Summary` says of the format.
#[derive(Debug, Clone, Copy)]
struct Format {
    channels: u16,
    bits: u16,
    /// Bytes per frame.
    block: u16,
}

impl Format {
    /// The `fmt ` chunk of a WAV header (with or without its RIFF head).
    fn of_wav(d: &[u8]) -> Option<Format> {
        let mut i: usize = if d.get(..4) == Some(b"RIFF") { 12 } else { 0 };
        while let Some(head) = d.get(i..i.checked_add(8)?) {
            let size = u32::from_le_bytes(head.get(4..8)?.try_into().ok()?) as usize;
            let body = i.checked_add(8)?;
            if head.get(..4) == Some(b"fmt ") {
                let f = d.get(body..body.checked_add(16)?)?;
                let u = |a: usize| -> Option<u16> {
                    Some(u16::from_le_bytes(f.get(a..a + 2)?.try_into().ok()?))
                };
                return Some(Format {
                    channels: u(2)?,
                    block: u(12)?,
                    bits: u(14)?,
                });
            }
            i = body.checked_add(size)?.checked_add(size % 2)?;
        }
        None
    }

    /// The `COMM` chunk of an AIFC header (big-endian).
    fn of_aifc(d: &[u8]) -> Option<Format> {
        let mut i: usize = if d.get(..4) == Some(b"FORM") { 12 } else { 0 };
        while let Some(head) = d.get(i..i.checked_add(8)?) {
            let size = u32::from_be_bytes(head.get(4..8)?.try_into().ok()?) as usize;
            let body = i.checked_add(8)?;
            if head.get(..4) == Some(b"COMM") {
                let f = d.get(body..body.checked_add(8)?)?;
                let channels = u16::from_be_bytes(f.get(0..2)?.try_into().ok()?);
                let bits = u16::from_be_bytes(f.get(6..8)?.try_into().ok()?);
                let block = channels.checked_mul(bits.div_ceil(8))?;
                return Some(Format {
                    channels,
                    bits,
                    block,
                });
            }
            i = body.checked_add(size)?.checked_add(size % 2)?;
        }
        None
    }
}

/// A 44-byte WAV header for `data` bytes of little-endian PCM; `None` when
/// the data does not fit a WAV's 32-bit sizes.
fn wav_header(channels: u16, bits: u16, rate: u32, data: u64) -> Option<Vec<u8>> {
    let block = channels.checked_mul(bits.div_ceil(8))?;
    let data = u32::try_from(data).ok()?;
    let riff = data.checked_add(36)?;
    let byte_rate = rate.checked_mul(u32::from(block))?;
    let mut h = Vec::with_capacity(WAV_HEADER);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&riff.to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&channels.to_le_bytes());
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&byte_rate.to_le_bytes());
    h.extend_from_slice(&block.to_le_bytes());
    h.extend_from_slice(&bits.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    Some(h)
}
