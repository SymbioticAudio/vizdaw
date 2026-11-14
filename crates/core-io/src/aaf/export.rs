//! The AAF export (plan 6f Task 4): one sequence as an Edit Protocol
//! composition, the audio files to stage, and the MIDI file beside it.
//!
//! **What is written** is what plays: the tracks are read through
//! `midi_out::source` with their mute and solo cleared (flattened
//! references, the active take, play quantize applied). Composition
//! position 0 is [`midi_out::file_start`], the pickup's start (a pickup
//! gets 6d's [`PICKUP`] Note).
//!
//! **Positions** (Ruling 3): every slot's edit rate is the project's sample
//! rate and its `Origin` 0, so positions and lengths are frames. A region's
//! start is its tick's seconds on the tempo map, less the file start's,
//! rounded to the frame; source offsets, lengths and fades are rescaled
//! from the media's rate.
//!
//! **Layout** (Ruling 4): on each track the later start takes over. A
//! region is written up to the next region's start; with a crossfade, up to
//! that start plus the crossfade, then a `Transition` (`CutPoint` 0) whose
//! `MonoAudioDissolve` has the shape as its `Level`'s interpolation (Ruling
//! 5). A region that plays on after a later one ends is written again after
//! it, its `StartTime` moved on. A fade-in is `Filler(gap + L)`,
//! `Transition(L)`, clip; a fade-out is clip, `Transition(L)`,
//! `Filler(L + gap)`. Fades a later region covers are cut, and listed.
//!
//! **Gain and pan** (Ruling 6): region gain is a `MonoAudioGain` around the
//! clip, `round(10^(dB/20) × 1 000 000) / 1 000 000`; the track's fader
//! and pan are `MonoAudioGain` and `MonoAudioPan` groups around its whole
//! sequence, constant or varying (points at fractions of the group's
//! length, `Linear`). **Stereo** (Ruling 7): with mono pairs a stereo track
//! is "<track> L" and "<track> R", panned hard left and right, each playing
//! one channel staged as its own mono file for that track.
//!
//! **The file** (Ruling 11): the fixed MetaDictionary
//! ([`super::metadict::write_into`]), the Header with one Identification,
//! the Dictionary of the definitions referenced, one `CompositionMob`
//! (`Usage_TopLevel`), one `MasterMob` per staged file and region name (AAF
//! 1.1's `Component` has no name, so a region's name is its master mob's,
//! as Task 2 reads it), and one file `SourceMob` per staged file with a
//! `WAVEDescriptor` and its `EssenceData` (embedded) or `NetworkLocator`s.
//!
//! [`check`] is [`export`]'s one walk without building the store, so the
//! dry run's report is the export's.

use super::dict::{class, def, pid};
use super::metadict::{self, table_index, utf16z};
use super::store::{AafWriter, Auid, ObjId, Prop, Store, WeakRef};
use crate::IoError;
use crate::dawproject::export::{EARLY_MARKERS, LeftOutAudio, PICKUP, SEQUENCE_COMMENTS};
use crate::dawproject::schema::METADATA_FIELDS;
use crate::dawproject::units::{linear_gain, pan_normalized};
use crate::for_daw::midi_tracks;
use crate::midi_out::{self, OTHER_INFO, SmfOptions, file_start, to_smf_until};
use crate::names::safe_part;
use crate::report::{ItemKind, Report};
use core_model::automation::{AutomationMode, AutomationTarget, fader_db};
use core_model::document::Document;
use core_model::event::{EventBody, FadeShape};
use core_model::format::BitDepth;
use core_model::ids::{LaneId, MediaId, SeqId, StripId};
use core_model::lane::{Lane, MonitorMode};
use core_model::mixer::{Strip, StripKind};
use core_model::query::ResolvedEvent;
use core_model::sequence::Sequence;
use core_model::tempo::TempoClock;
use core_model::time::Tick;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------- the API

/// How a stereo track is written (Ruling 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stereo {
    /// Two mono slots, "<track> L" and "<track> R" (the default).
    MonoPairs,
    /// One two-channel slot.
    Stereo,
}

/// The start timecode's frame rate (Ruling 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fps {
    /// 24 fps.
    F24,
    /// 25 fps (the default).
    F25,
    /// 29.97 fps drop-frame.
    F2997Drop,
    /// 30 fps.
    F30,
}

/// What to export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AafOptions {
    /// The sequence.
    pub seq: SeqId,
    /// Audio inside the `.aaf` (else external WAVs in `Audio/`).
    pub embed: bool,
    /// The staged WAVs' depth: `Int16` or `Int24`.
    pub depth: BitDepth,
    /// How stereo tracks are written.
    pub stereo: Stereo,
    /// The timecode starts at 01:00:00:00 (else 00:00:00:00).
    pub hour_start: bool,
    /// The timecode's rate.
    pub fps: Fps,
    /// Write region gain.
    pub clip_gain: bool,
    /// Write track volume and pan.
    pub automation: bool,
    /// Offline media, from the caller (as 6d): its regions are not
    /// written, and each item used gets its Left out line.
    pub left_out: LeftOutAudio,
}

/// What the export needs from outside the document.
#[derive(Debug, Clone, Copy)]
pub struct ExportEnv<'a> {
    /// VizDAW's version, for the `Identification`.
    pub app_version: &'a str,
    /// This export's ID: the MobIDs and the `GenerationAUID` are made from
    /// it.
    pub export_id: &'a str,
    /// Seconds since 1970: the Header, Identification and mob times.
    pub modified: u64,
}

/// One file to stage: media item `media`, channel `channel` (None: all),
/// named `name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    /// The media item.
    pub media: MediaId,
    /// The channel (from 0) the file holds; `None`: every channel.
    pub channel: Option<u16>,
    /// Its name (the file mob's).
    pub name: String,
    /// Its path in the export folder, `"Audio/001 x.wav"`.
    pub file: String,
}

/// An export: the object tree, the essence to stream into it, the files to
/// stage, the MIDI file beside it and the report.
#[derive(Debug, Clone)]
pub struct AafExport {
    /// The objects to write.
    pub store: Store,
    /// Each `EssenceData` object and the staged file it holds (embedded
    /// audio).
    pub essence: Vec<(ObjId, usize)>,
    /// The files to stage, in write order.
    pub audio: Vec<Staged>,
    /// `<Song> MIDI.mid`, when the sequence has a MIDI track.
    pub midi: Option<Vec<u8>>,
    /// What changed, was left out or is worth knowing.
    pub report: Report,
}

/// Sequence `o.seq` of `doc` as an AAF export. Looks at `cancel` every
/// few thousand items and between stages.
pub fn export(
    doc: &Document,
    o: &AafOptions,
    env: &ExportEnv<'_>,
    cancel: &AtomicBool,
) -> Result<AafExport, IoError> {
    let w = walk(doc, o, cancel)?;
    let midi = match &w.midi {
        Some(f) => Some(crate::smf::write(f)?),
        None => None,
    };
    stop(cancel)?;
    let (store, essence) = build(doc, o, env, &w, cancel)?;
    Ok(AafExport {
        store,
        essence,
        audio: w.staged,
        midi,
        report: w.report,
    })
}

/// The dry run: [`export`]'s report, from the same walk, with no store
/// built and nothing written.
pub fn check(doc: &Document, o: &AafOptions) -> Result<Report, IoError> {
    walk(doc, o, &AtomicBool::new(false)).map(|w| w.report)
}

/// "<Song> AAF" (Ruling 2): `safe_part` of the song title, else the
/// sequence's name, leaving room for " 9999" and the staging folder's
/// `.` and `.partial`.
pub fn folder_name(doc: &Document, seq: SeqId) -> Result<String, IoError> {
    Ok(format!("{}{FOLDER_SUFFIX}", song_part(doc, seq)?))
}

/// The names in the export folder: `<Song>.aaf` and `<Song> MIDI.mid`
/// (the core names every file, as [`folder_name`]).
pub fn file_names(doc: &Document, seq: SeqId) -> Result<(String, String), IoError> {
    let song = song_part(doc, seq)?;
    Ok((format!("{song}.aaf"), format!("{song} MIDI.mid")))
}

/// Writes `x` into the new file `dest` (never replacing one), then streams
/// each embedded staged file `i` into its `EssenceData` with `essence(i,
/// w)`. External audio gets an absolute `file://` locator into the export
/// folder (`dest`'s folder, a staging name `.<name>.partial` read as
/// `<name>`) before the relative one. Looks at `cancel` between files. On
/// any error or cancel `dest` is removed.
pub fn write(
    x: &AafExport,
    dest: &Path,
    essence: &mut dyn FnMut(usize, &mut dyn Write) -> Result<(), IoError>,
    cancel: &AtomicBool,
) -> Result<(), IoError> {
    stop(cancel)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|e| crate::io_err(dest, "written", &e))?;
    let r = write_file(x, file, dest, essence, cancel);
    if r.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    r
}

fn write_file(
    x: &AafExport,
    file: std::fs::File,
    dest: &Path,
    essence: &mut dyn FnMut(usize, &mut dyn Write) -> Result<(), IoError>,
    cancel: &AtomicBool,
) -> Result<(), IoError> {
    let located = absolute_locators(&x.store, dest);
    let store = located.as_ref().unwrap_or(&x.store);
    let mut w = AafWriter::create(file)?;
    w.write_store(store)?;
    for (obj, i) in &x.essence {
        stop(cancel)?;
        let mut s = w.stream(store, *obj, ESSENCE_STREAM)?;
        essence(*i, &mut s)?;
        s.flush().map_err(|e| crate::io_err(dest, "written", &e))?;
    }
    stop(cancel)?;
    let f = w.finish()?;
    f.sync_all().map_err(|e| crate::io_err(dest, "written", &e))
}

// ---------------------------------------------------------------- the report

/// Left out: muted regions (by track).
pub const MUTED_REGIONS: &str = "Muted regions";
/// Left out: track and strip mute and solo (by name).
pub const MUTE_SOLO: &str = "Mute and solo";
/// Left out: sends, buses and where strips output (by strip).
pub const ROUTING: &str = "Sends, buses and output routing";
/// Left out: inserts and the EQ slot (by strip).
pub const INSERTS: &str = "Inserts and the EQ slot";
/// Left out: strip colours (by strip).
pub const COLOURS: &str = "Strip colours";
/// Left out: automation of anything but an audio track's volume and pan
/// (by track).
pub const OTHER_AUTOMATION: &str = "Automation other than volume and pan";
/// Left out: automation modes other than Read (by strip) and MIDI-learn
/// maps (a count).
pub const AUTOMATION_MODES: &str = "Automation modes, MIDI learn and control surfaces";
/// Left out: inactive takes (by track) and sequences the export does not
/// reach (by sequence).
pub const TAKES: &str = "Inactive takes and other sequences";
/// Left out: sequences' keyboard slots (by sequence).
pub const KEYBOARD_SLOTS: &str = "Keyboard slots";
/// Left out: input effects, inputs, arm, monitoring and MPE input (by
/// track).
pub const INPUTS: &str = "Input effects and track inputs";
/// Left out: strips of an instrument's extra outputs (by strip).
pub const EXTRA_OUTPUTS: &str = "Instrument extra outputs";
/// Note (Ruling 8): the MIDI tracks written into `<Song> MIDI.mid` (by
/// track).
pub const MIDI_BESIDE: &str = "MIDI tracks are in the MIDI file beside the AAF";
/// Note (§7.1): tracks playing the built-in synth (by track).
pub const BUILTIN_SYNTH: &str =
    "Tracks playing the built-in synth (their notes are in the MIDI file)";
/// Note (§7.1): tracks playing an external MIDI destination (by track).
pub const EXTERNAL_MIDI: &str = "Tracks playing external MIDI (their notes are in the MIDI file)";
/// Left out (Ruling 6): region gain with `clip_gain` off (a count).
pub const CLIP_GAIN_OFF: &str = "Clip gain (turned off)";
/// Left out (Ruling 6): track volume and pan with `automation` off (by
/// track).
pub const AUTOMATION_OFF: &str = "Track volume and pan (turned off)";
/// Changed (Ruling 4): fade-outs a later region covers (a count).
pub const FADE_OUTS_CUT: &str = "Fade-outs under a later region were cut";
/// Changed (Ruling 4): fade-ins a later region covers (a count).
pub const FADE_INS_CUT: &str = "Fade-ins under a later region were cut";
/// Changed (Ruling 5): exponential fades, written as `Log`.
pub const EXPONENTIAL: &str = "Exponential and S-curve fades are drawn differently in other DAWs";
/// Changed (Ruling 5): S-curve fades, written as `Linear`.
pub const SCURVE: &str = "S-curve fades were written as linear";
/// Changed (Ruling 7): a stereo track written as mono pairs has its halves
/// panned hard left and right, so its own pan is not written (by track).
pub const PAIR_PAN: &str = "Pan of stereo tracks written as left and right tracks";

// ---------------------------------------------------------------- constants

const NO_SEQ: &str = "The sequence to export is not in this project.";
const BAD_DEPTH: &str = "AAF audio is written at 16 or 24 bits.";
const FOLDER_SUFFIX: &str = " AAF";
/// What a folder name leaves (Ruling 2): " AAF", " 9999", the staging
/// folder's `.` and `.partial` (it also covers `<Song> MIDI.mid`).
const FOLDER_RESERVE: usize = FOLDER_SUFFIX.len() + " 9999".len() + ".partial".len() + 1;
/// The data stream of an `EssenceData`.
const ESSENCE_STREAM: &str = "Data-2702";
/// How many items pass between looks at the cancel flag.
const CANCEL_EVERY: usize = 4096;
/// The longest string written, in UTF-16 units (a property holds at most
/// 65 535 bytes).
const MAX_TEXT_UNITS: usize = 16_000;
/// Region gain's denominator (Ruling 6).
const GAIN_DEN: i32 = 1_000_000;
/// The SMPTE UMID label MobIDs start with.
const UMID_LABEL: [u8; 12] = [
    0x06, 0x0a, 0x2b, 0x34, 0x01, 0x01, 0x01, 0x05, 0x01, 0x01, 0x0f, 0x20,
];
/// The type of a rational, for indirect values and parameters.
const RATIONAL_TYPE: Auid = Auid::from_fields(
    0x03010100,
    0x0000,
    0x0000,
    [0x06, 0x0e, 0x2b, 0x34, 0x01, 0x04, 0x01, 0x01],
);
/// The type of a string, for indirect values.
const STRING_TYPE: Auid = Auid::from_fields(
    0x01100200,
    0x0000,
    0x0000,
    [0x06, 0x0e, 0x2b, 0x34, 0x01, 0x04, 0x01, 0x01],
);
/// The MetaDictionary's `TypeDefinitions`, which parameter types name.
const TYPE_DEFINITIONS: [u16; 2] = [
    pid::ROOT_META_DICTIONARY,
    pid::META_DICTIONARY_TYPE_DEFINITIONS,
];

fn stop(cancel: &AtomicBool) -> Result<(), IoError> {
    if cancel.load(Ordering::Relaxed) {
        Err(IoError::Cancelled)
    } else {
        Ok(())
    }
}

/// Counts items, looking at `cancel` every [`CANCEL_EVERY`].
struct Ticker<'a> {
    n: usize,
    cancel: &'a AtomicBool,
}

impl Ticker<'_> {
    fn tick(&mut self) -> Result<(), IoError> {
        self.n = self.n.wrapping_add(1);
        if self.n % CANCEL_EVERY == 0 {
            stop(self.cancel)
        } else {
            Ok(())
        }
    }
}

fn sequence(doc: &Document, seq: SeqId) -> Result<&Sequence, IoError> {
    doc.sequence(seq)
        .map_err(|_| IoError::Refused(NO_SEQ.to_owned()))
}

fn song_part(doc: &Document, seq: SeqId) -> Result<String, IoError> {
    let s = sequence(doc, seq)?;
    let title = doc
        .settings()
        .info
        .title
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(&s.name);
    Ok(safe_part(title, FOLDER_RESERVE))
}

/// Counts `n` occurrences of `what` at once.
fn add_n(report: &mut Report, kind: ItemKind, what: &str, n: u64) {
    if n == 0 {
        return;
    }
    report.add(kind, what, None);
    if let Some(i) = report
        .items
        .iter_mut()
        .find(|i| i.kind == kind && i.what == what)
    {
        i.count = i.count.saturating_add(n - 1);
    }
}

// ---------------------------------------------------------------- the walk

/// One region on a track, in composition units (frames at the project
/// rate) from the file start.
#[derive(Debug, Clone)]
struct Reg {
    start: i64,
    len: i64,
    media: MediaId,
    /// The source offset, in composition units.
    offset: i64,
    fade_in: i64,
    in_shape: FadeShape,
    fade_out: i64,
    out_shape: FadeShape,
    crossfade: bool,
    gain_db: f32,
    name: Option<String>,
}

impl Reg {
    fn end(&self) -> i64 {
        self.start.saturating_add(self.len)
    }
}

/// One part of a region written as a clip, with the transitions around it:
/// (length, shape).
#[derive(Debug, Clone)]
struct Piece {
    reg: usize,
    start: i64,
    end: i64,
    /// A crossfade from the piece before.
    xfade: Option<(i64, FadeShape)>,
    /// A fade-in.
    fade_in: Option<(i64, FadeShape)>,
    /// A fade-out.
    fade_out: Option<(i64, FadeShape)>,
}

/// One audio track: its regions and how they are written.
#[derive(Debug, Clone)]
struct Track {
    name: String,
    strip: Option<StripId>,
    regs: Vec<Reg>,
    pieces: Vec<Piece>,
    /// Written as "<name> L" and "<name> R".
    pairs: bool,
    lane: LaneId,
}

/// Curve points: (composition units, value).
type Points = Vec<(i64, f64)>;

/// A rational: numerator, denominator.
type Ratio = (i32, i32);

/// A track curve: a constant, or points (composition units, value).
#[derive(Debug, Clone)]
enum Curve {
    Constant(f64),
    Points(Points),
}

/// Per track and slot side (one side, or left and right for a pair), the
/// staged file of each region.
type Files = Vec<Vec<Vec<Option<usize>>>>;

/// What the walk found: everything the store is built from, and the report.
struct Walked {
    rate: u32,
    tracks: Vec<Track>,
    /// Per strip, its volume and pan (linear gain, normalised pan).
    curves: BTreeMap<StripId, (Curve, Curve)>,
    markers: Vec<(i64, String)>,
    length: i64,
    staged: Vec<Staged>,
    files: Files,
    midi: Option<crate::smf::SmfFile>,
    report: Report,
}

/// `export` and `check`'s one walk.
fn walk(doc: &Document, o: &AafOptions, cancel: &AtomicBool) -> Result<Walked, IoError> {
    if !matches!(o.depth, BitDepth::Int16 | BitDepth::Int24) {
        return Err(IoError::Refused(BAD_DEPTH.to_owned()));
    }
    let s = sequence(doc, o.seq)?;
    stop(cancel)?;
    let rate = doc.settings().format.sample_rate.max(1);
    let clock = s.tempo.clock();
    let origin = file_start(s);
    let at = Placer {
        clock: &clock,
        zero: clock.tick_to_seconds(origin),
        rate,
    };
    let mut report = Report::default();
    let mut ticker = Ticker { n: 0, cancel };
    let mut scratch = Report::default();
    let played = midi_out::source(doc, o.seq, &[], &mut scratch)?.played;
    stop(cancel)?;

    // The audio tracks: those that play a region, and those that play no
    // MIDI and have an output and no MIDI destination.
    let midi_lanes: BTreeSet<LaneId> = midi_tracks(doc, o.seq)?.into_iter().collect();
    let m = doc.mixer();
    let mut tracks: Vec<Track> = Vec::new();
    let mut used_strips: BTreeSet<StripId> = BTreeSet::new();
    let mut listed_media: BTreeSet<MediaId> = BTreeSet::new();
    let (mut exp, mut scurve, mut gain_off) = (false, false, 0u64);
    for lane in &s.lanes {
        let events = played.get(&lane.id).map_or(&[][..], Vec::as_slice);
        let audio = events
            .iter()
            .any(|r| matches!(r.body, EventBody::AudioRegion { .. }));
        let no_midi = !events.iter().any(|r| r.body.is_midi());
        let empty = no_midi && lane.audio_out.is_some() && lane.midi_dest.is_none();
        if !(audio || empty) {
            continue;
        }
        let mut regs = Vec::new();
        let mut wide = 1u16;
        for r in events {
            ticker.tick()?;
            let EventBody::AudioRegion {
                media,
                source_offset,
                length,
                gain_db,
                fade_in,
                fade_out,
                name,
                crossfade,
            } = &r.body
            else {
                continue;
            };
            let Some(item) = doc.media().get(media) else {
                continue;
            };
            if let Some(what) = o.left_out.get(media) {
                if listed_media.insert(*media) {
                    report.add(ItemKind::LeftOut, what, Some(item.name.clone()));
                }
                continue;
            }
            wide = wide.max(item.channels);
            let mr = item.sample_rate.max(1);
            let to = |f: u64| rescale(f, mr, rate);
            if *gain_db != 0.0 && !o.clip_gain {
                gain_off = gain_off.saturating_add(1);
            }
            regs.push(Reg {
                start: at.frame(r.tick),
                len: to(*length).max(1),
                media: *media,
                offset: to(*source_offset),
                fade_in: to(fade_in.length_frames),
                in_shape: fade_in.shape,
                fade_out: to(fade_out.length_frames),
                out_shape: fade_out.shape,
                crossfade: *crossfade,
                gain_db: *gain_db,
                name: name.clone(),
            });
        }
        let strip = lane
            .audio_out
            .and_then(|out| m.find_kind(StripKind::AudioPath(out)));
        if let Some(st) = strip {
            used_strips.insert(st);
        }
        let pieces = layout(&regs, &mut report);
        for p in &pieces {
            for f in [p.xfade, p.fade_in, p.fade_out].into_iter().flatten() {
                exp |= f.1 == FadeShape::Exponential;
                scurve |= f.1 == FadeShape::SCurve;
            }
        }
        tracks.push(Track {
            name: lane.name.clone(),
            strip,
            regs,
            pieces,
            pairs: o.stereo == Stereo::MonoPairs && wide == 2,
            lane: lane.id,
        });
    }
    if exp {
        report.add(ItemKind::Changed, EXPONENTIAL, None);
    }
    if scurve {
        report.add(ItemKind::Changed, SCURVE, None);
    }
    add_n(&mut report, ItemKind::LeftOut, CLIP_GAIN_OFF, gain_off);

    // Volume and pan of each strip an audio track plays through.
    let mut points: BTreeMap<StripId, (Points, Points)> = BTreeMap::new();
    for lane in &s.lanes {
        let events = played.get(&lane.id).map_or(&[][..], Vec::as_slice);
        let mut other = false;
        for r in events {
            ticker.tick()?;
            let EventBody::Automation { target, value } = r.body else {
                continue;
            };
            match target {
                AutomationTarget::StripFader(st) if used_strips.contains(&st) => {
                    let v = linear_gain(fader_db(value));
                    points.entry(st).or_default().0.push((at.frame(r.tick), v));
                }
                AutomationTarget::StripPan(st) if used_strips.contains(&st) => {
                    points
                        .entry(st)
                        .or_default()
                        .1
                        .push((at.frame(r.tick), value));
                }
                _ => other = true,
            }
        }
        if other {
            report.add(ItemKind::LeftOut, OTHER_AUTOMATION, Some(lane.name.clone()));
        }
    }
    let mut curves = BTreeMap::new();
    for st in &used_strips {
        let Ok(strip) = m.strip(*st) else { continue };
        let (fp, pp) = points.remove(st).unwrap_or_default();
        let gain = if fp.is_empty() {
            Curve::Constant(linear_gain(strip.gain_db))
        } else {
            Curve::Points(fp)
        };
        let pan = if pp.is_empty() {
            Curve::Constant(pan_normalized(strip.pan))
        } else {
            Curve::Points(pp)
        };
        curves.insert(*st, (gain, pan));
    }
    let moved = |c: &Curve, rest: f64| match c {
        Curve::Constant(v) => (v - rest).abs() > 1e-12,
        Curve::Points(_) => true,
    };
    for t in &tracks {
        let curve = t.strip.and_then(|st| curves.get(&st));
        let mixed = curve.is_some_and(|(g, p)| moved(g, 1.0) || moved(p, 0.5));
        if !o.automation && mixed {
            report.add(ItemKind::LeftOut, AUTOMATION_OFF, Some(t.name.clone()));
        }
        if o.automation && t.pairs && curve.is_some_and(|(_, p)| moved(p, 0.5)) {
            report.add(ItemKind::Changed, PAIR_PAN, Some(t.name.clone()));
        }
    }

    // Markers.
    let mut markers = Vec::new();
    for mk in &s.markers {
        if mk.tick < origin {
            report.add(ItemKind::LeftOut, EARLY_MARKERS, Some(mk.name.clone()));
            continue;
        }
        markers.push((at.frame(mk.tick), mk.name.clone()));
    }
    markers.sort_by_key(|m| m.0);

    // The composition's length: every clip, point and marker inside it.
    let mut length = 0i64;
    for p in tracks.iter().flat_map(|t| t.pieces.iter()) {
        length = length.max(p.end);
    }
    for c in curves.values().flat_map(|(g, p)| [g, p]) {
        if let Curve::Points(v) = c {
            for (f, _) in v {
                length = length.max(f.saturating_add(1));
            }
        }
    }
    for (f, _) in &markers {
        length = length.max(f.saturating_add(1));
    }

    let (staged, files) = stage(doc, &tracks);
    left_out(doc, s, &played, &midi_lanes, &mut report);

    // The MIDI file beside it (Ruling 8): its lines join the report, but
    // not its Notes (worded for a MIDI file alone) or what the AAF's own
    // lines cover.
    let midi = if midi_lanes.is_empty() {
        None
    } else {
        let lanes: Vec<LaneId> = s
            .lanes
            .iter()
            .map(|l| l.id)
            .filter(|l| midi_lanes.contains(l))
            .collect();
        let (f, r) = to_smf_until(
            doc,
            &SmfOptions {
                seq: o.seq,
                lanes,
                type0: false,
                ppq: 960,
                mpe: true,
            },
            cancel,
        )?;
        for i in r.items {
            let covered = i.what == OTHER_INFO || i.what == "Inactive takes";
            if i.kind == ItemKind::Note || covered {
                continue;
            }
            add_n(&mut report, i.kind, &i.what, i.count);
            if let Some(item) = report
                .items
                .iter_mut()
                .find(|x| x.kind == i.kind && x.what == i.what)
            {
                item.details.extend(i.details);
                item.details.truncate(crate::limits::MAX_DETAILS);
            }
        }
        Some(f)
    };
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    let sm = &mut report.summary;
    sm.tracks = tracks.iter().map(|t| if t.pairs { 2u64 } else { 1 }).sum();
    sm.audio_files = count(staged.len());
    sm.regions = tracks.iter().map(|t| count(t.regs.len())).sum();
    sm.markers = count(markers.len());
    sm.automation_points = curves
        .values()
        .flat_map(|(g, p)| [g, p])
        .map(|c| match c {
            Curve::Points(v) => count(v.len()),
            Curve::Constant(_) => 0,
        })
        .sum();
    sm.tempo_map = midi.is_some();
    Ok(Walked {
        rate,
        tracks,
        curves,
        markers,
        length,
        staged,
        files,
        midi,
        report,
    })
}

/// Ticks as composition units.
struct Placer<'a> {
    clock: &'a TempoClock,
    /// The file start's seconds.
    zero: f64,
    rate: u32,
}

impl Placer<'_> {
    /// The frame of `tick` from the file start, rounded (0 when not finite
    /// or before it).
    fn frame(&self, tick: Tick) -> i64 {
        let secs = self.clock.tick_to_seconds(tick) - self.zero;
        let f = (secs * f64::from(self.rate)).round();
        if f.is_finite() && f > 0.0 {
            // `as` saturates.
            f as i64
        } else {
            0
        }
    }
}

/// `x` frames at `from` as frames at `to`, rounded half up, saturating.
fn rescale(x: u64, from: u32, to: u32) -> i64 {
    if from == to || from == 0 {
        return i64::try_from(x).unwrap_or(i64::MAX);
    }
    let num = u128::from(x) * u128::from(to) * 2 + u128::from(from);
    i64::try_from(num / (u128::from(from) * 2)).unwrap_or(i64::MAX)
}

/// Ruling 4's layout of one track: the later start takes over (a region
/// plays on after a later one ends); a crossfade extends the piece it
/// starts in; fades a later region covers are cut and listed.
fn layout(regs: &[Reg], report: &mut Report) -> Vec<Piece> {
    let end_of = |i: usize| regs.get(i).map_or(i64::MIN, Reg::end);
    let mut order: Vec<usize> = (0..regs.len()).collect();
    order.sort_by_key(|i| regs.get(*i).map_or(0, |r| r.start));
    let mut pieces: Vec<Piece> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut cur = i64::MIN;
    // Plays the regions on the stack (the latest start on top) up to `to`.
    let advance = |to: i64, cur: &mut i64, stack: &mut Vec<usize>, pieces: &mut Vec<Piece>| {
        while let Some(&top) = stack.last() {
            let end = end_of(top);
            if end <= *cur {
                stack.pop();
                continue;
            }
            let seg_end = end.min(to);
            if seg_end > *cur {
                pieces.push(Piece {
                    reg: top,
                    start: *cur,
                    end: seg_end,
                    xfade: None,
                    fade_in: None,
                    fade_out: None,
                });
                *cur = seg_end;
            }
            if end <= to {
                stack.pop();
            } else {
                break;
            }
        }
    };
    for i in order {
        let Some(r) = regs.get(i) else { continue };
        advance(r.start, &mut cur, &mut stack, &mut pieces);
        cur = r.start;
        stack.push(i);
    }
    advance(i64::MAX, &mut cur, &mut stack, &mut pieces);

    // Fade-ins and crossfades: a crossfade's partner is the piece its
    // start cut, which plays on through it.
    for k in 0..pieces.len() {
        let Some(p) = pieces.get(k).cloned() else {
            continue;
        };
        let Some(r) = regs.get(p.reg) else { continue };
        if p.start != r.start || r.fade_in <= 0 {
            continue;
        }
        let partner = k
            .checked_sub(1)
            .and_then(|j| pieces.get(j).map(|q| (j, q.clone())))
            .filter(|(_, q)| q.end == p.start && q.reg != p.reg && end_of(q.reg) > p.start);
        let len = p.end.saturating_sub(p.start);
        match (r.crossfade, partner) {
            (true, Some((j, q))) => {
                let l = r
                    .fade_in
                    .min(end_of(q.reg).saturating_sub(p.start))
                    .min(len);
                if let Some(q) = pieces.get_mut(j) {
                    q.end = q.end.saturating_add(l);
                }
                if let Some(p) = pieces.get_mut(k) {
                    p.xfade = Some((l, r.in_shape));
                }
            }
            _ => {
                let l = r.fade_in.min(len);
                if l < r.fade_in {
                    report.add(ItemKind::Changed, FADE_INS_CUT, None);
                }
                if let Some(p) = pieces.get_mut(k) {
                    p.fade_in = (l > 0).then_some((l, r.in_shape));
                }
            }
        }
    }

    // Fade-outs: on a region's last piece, when it ends with the region
    // and no crossfade takes it over.
    for (i, r) in regs.iter().enumerate() {
        if r.fade_out <= 0 {
            continue;
        }
        let Some(k) = pieces.iter().rposition(|p| p.reg == i) else {
            continue;
        };
        let crossed = pieces.get(k + 1).is_some_and(|n| n.xfade.is_some());
        let Some(p) = pieces.get_mut(k) else { continue };
        let free = p
            .end
            .saturating_sub(p.start)
            .saturating_sub(p.fade_in.map_or(0, |f| f.0));
        let covered = p.end != r.end() || crossed;
        if covered || free < r.fade_out {
            report.add(ItemKind::Changed, FADE_OUTS_CUT, None);
        }
        if covered {
            continue;
        }
        let l = r.fade_out.min(free);
        p.fade_out = (l > 0).then_some((l, r.out_shape));
    }
    pieces
}

/// The files to stage, and per track and side the staged file of each
/// region. A track written as a pair stages its channels per track (a mono
/// region's one channel for both sides), so each pair plays files of its
/// own; other tracks share one file per media item.
fn stage(doc: &Document, tracks: &[Track]) -> (Vec<Staged>, Files) {
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum Key {
        Whole(MediaId),
        Side(LaneId, MediaId, u16),
    }
    let mut keys: BTreeMap<Key, usize> = BTreeMap::new();
    let mut raw: Vec<(MediaId, Option<u16>, String)> = Vec::new();
    let mut files = Vec::with_capacity(tracks.len());
    for t in tracks {
        let sides = if t.pairs { 2u16 } else { 1 };
        let mut per_side = Vec::new();
        for side in 0..sides {
            let mut idx = Vec::with_capacity(t.regs.len());
            for r in &t.regs {
                let Some(item) = doc.media().get(&r.media) else {
                    idx.push(None);
                    continue;
                };
                let (key, channel, name) = if t.pairs {
                    let ch = if item.channels >= 2 { side } else { 0 };
                    let suffix = if side == 0 { "L" } else { "R" };
                    (
                        Key::Side(t.lane, r.media, side),
                        Some(ch),
                        format!("{} {suffix}", item.name),
                    )
                } else {
                    (Key::Whole(r.media), None, item.name.clone())
                };
                let next = raw.len();
                let at = *keys.entry(key).or_insert(next);
                if at == next {
                    raw.push((r.media, channel, name));
                }
                idx.push(Some(at));
            }
            per_side.push(idx);
        }
        files.push(per_side);
    }
    let width = raw.len().to_string().len().max(3);
    let staged = raw
        .into_iter()
        .enumerate()
        .map(|(i, (media, channel, name))| {
            let num = format!("{:0width$} ", i.saturating_add(1));
            let reserve = num
                .len()
                .saturating_add(".wav".len())
                .saturating_add(".partial".len())
                .saturating_add(2);
            let file = format!("Audio/{num}{}.wav", safe_part(&name, reserve));
            Staged {
                media,
                channel,
                name,
                file,
            }
        })
        .collect();
    (staged, files)
}

/// The lines of what the file does not carry, and the MIDI tracks' Notes.
fn left_out(
    doc: &Document,
    s: &Sequence,
    played: &BTreeMap<LaneId, Vec<ResolvedEvent>>,
    midi_lanes: &BTreeSet<LaneId>,
    report: &mut Report,
) {
    // Muted regions (top level, in the active takes): by track.
    for l in &s.lanes {
        let mut first = true;
        for e in l.active().into_iter().flat_map(|t| t.events.values()) {
            if e.mute && matches!(e.body, EventBody::AudioRegion { .. }) {
                let detail = first.then(|| l.name.clone());
                first = false;
                report.add(ItemKind::LeftOut, MUTED_REGIONS, detail);
            }
        }
    }
    for l in &s.lanes {
        let name = || Some(l.name.clone());
        if l.mute || l.solo {
            report.add(ItemKind::LeftOut, MUTE_SOLO, name());
        }
        if l.takes
            .iter()
            .any(|t| t.id != l.active_take && !t.events.is_empty())
        {
            report.add(ItemKind::LeftOut, TAKES, name());
        }
        if !l.input_fx.is_empty()
            || l.input.is_some()
            || l.arm
            || l.monitor != MonitorMode::Auto
            || l.mpe.is_some()
        {
            report.add(ItemKind::LeftOut, INPUTS, name());
        }
    }
    // Sequences the export does not reach, and keyboard slots.
    let refs_of = |l: &Lane| -> Vec<SeqId> {
        l.active()
            .into_iter()
            .flat_map(|t| t.events.values())
            .filter_map(|e| match e.body {
                EventBody::Reference { seq, .. } => Some(seq),
                _ => None,
            })
            .collect()
    };
    let mut reached: BTreeSet<SeqId> = BTreeSet::from([s.id]);
    let mut todo: Vec<SeqId> = s.lanes.iter().flat_map(refs_of).collect();
    while let Some(id) = todo.pop() {
        if reached.insert(id)
            && let Ok(t) = doc.sequence(id)
        {
            todo.extend(t.lanes.iter().flat_map(refs_of));
        }
    }
    for other in doc.sequences().values() {
        if !reached.contains(&other.id) {
            report.add(ItemKind::LeftOut, TAKES, Some(other.name.clone()));
        }
        if other.keyboard_slot.is_some() {
            report.add(ItemKind::LeftOut, KEYBOARD_SLOTS, Some(other.name.clone()));
        }
    }
    if !s.comment.trim().is_empty() {
        report.add(ItemKind::LeftOut, SEQUENCE_COMMENTS, Some(s.name.clone()));
    }
    // The mixer, in mixer order, the master last.
    let m = doc.mixer();
    for id in m.order().iter() {
        if let Ok(st) = m.strip(*id) {
            strip_lines(st, report);
        }
    }
    if !m.order().contains(&m.master())
        && let Ok(st) = m.strip(m.master())
    {
        strip_lines(st, report);
    }
    add_n(
        report,
        ItemKind::LeftOut,
        AUTOMATION_MODES,
        u64::try_from(doc.settings().learn.len()).unwrap_or(u64::MAX),
    );
    // The MIDI tracks and what they play (§7.1's AAF column).
    for l in s.lanes.iter().filter(|l| midi_lanes.contains(&l.id)) {
        report.add(ItemKind::Note, MIDI_BESIDE, Some(l.name.clone()));
        let dests: BTreeSet<_> = played
            .get(&l.id)
            .into_iter()
            .flatten()
            .filter(|r| r.body.is_midi())
            .filter_map(|r| r.dest)
            .chain(l.midi_dest)
            .collect();
        let kinds: Vec<(bool, bool)> = dests
            .iter()
            .filter_map(|d| doc.instruments().get(d))
            .map(|d| (d.kind.is_builtin_synth(), d.kind.is_external()))
            .collect();
        if kinds.iter().any(|k| k.0) {
            report.add(ItemKind::Note, BUILTIN_SYNTH, Some(l.name.clone()));
        }
        if kinds.iter().any(|k| k.1) {
            report.add(ItemKind::Note, EXTERNAL_MIDI, Some(l.name.clone()));
        }
    }
    if s.start_offset.0 > 0 {
        report.add(ItemKind::Note, PICKUP, None);
    }
}

/// A strip's lines: extra outputs, mute and solo, routing, inserts, colour
/// and automation mode.
fn strip_lines(st: &Strip, report: &mut Report) {
    let name = || Some(st.name.clone());
    if matches!(st.kind, StripKind::InstrumentOutput(..)) {
        report.add(ItemKind::LeftOut, EXTRA_OUTPUTS, name());
    }
    if st.mute || st.solo {
        report.add(ItemKind::LeftOut, MUTE_SOLO, name());
    }
    if !st.sends.is_empty() || matches!(st.kind, StripKind::Bus) {
        report.add(ItemKind::LeftOut, ROUTING, name());
    }
    if !st.inserts.is_empty() || st.eq.is_some() {
        report.add(ItemKind::LeftOut, INSERTS, name());
    }
    if st.colour.is_some() {
        report.add(ItemKind::LeftOut, COLOURS, name());
    }
    if st.automation != AutomationMode::Read {
        report.add(ItemKind::LeftOut, AUTOMATION_MODES, name());
    }
}

// ---------------------------------------------------------------- values

/// `x` as a rational: the last convergent of its continued fraction at or
/// below it whose parts fit an `i32` (0/1 for what is not a finite
/// positive number). At or below, so a gain at the fader's top never reads
/// back above it.
fn ratio(x: f64) -> (i32, i32) {
    if !x.is_finite() || x <= 0.0 {
        return (0, 1);
    }
    let max = i128::from(i32::MAX);
    let (mut h0, mut h1, mut k0, mut k1) = (0i128, 1i128, 1i128, 0i128);
    let mut best = (0i128, 1i128);
    let mut y = x;
    for _ in 0..64 {
        let a = y.floor();
        if !(0.0..=2_147_483_647.0).contains(&a) {
            break;
        }
        // In range and whole, so `as` is exact.
        let a = a as i128;
        let (h2, k2) = (a * h1 + h0, a * k1 + k0);
        if h2 > max || k2 > max {
            break;
        }
        (h0, h1, k0, k1) = (h1, h2, k1, k2);
        if k1 > 0 && (h1 as f64) / (k1 as f64) <= x {
            best = (h1, k1);
        }
        let frac = y - y.floor();
        if frac <= 0.0 {
            break;
        }
        y = 1.0 / frac;
        if !y.is_finite() {
            break;
        }
    }
    (
        i32::try_from(best.0).unwrap_or(0),
        i32::try_from(best.1).unwrap_or(1).max(1),
    )
}

fn rational_bytes((n, d): (i32, i32)) -> Vec<u8> {
    let mut v = n.to_le_bytes().to_vec();
    v.extend_from_slice(&d.to_le_bytes());
    v
}

/// An indirect value: byte order, the type's AUID, the value.
fn indirect(t: Auid, value: &[u8]) -> Vec<u8> {
    let mut v = vec![0x4C];
    v.extend_from_slice(&t.0);
    v.extend_from_slice(value);
    v
}

/// A string as stored, cut at a NUL and to what a property holds.
fn text(s: &str) -> Vec<u8> {
    let cut: String = s
        .chars()
        .scan(0usize, |n, c| {
            *n = n.saturating_add(c.len_utf16());
            (*n <= MAX_TEXT_UNITS && c != '\0').then_some(c)
        })
        .collect();
    utf16z(&cut)
}

/// An AAF `TimeStamp` of `secs` since 1970 (UTC): year (i16), month, day,
/// hour, minute, second and fraction (0).
fn timestamp(secs: u64) -> Vec<u8> {
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX / 2);
    let rem = secs % 86_400;
    // Days to a civil date (proleptic Gregorian).
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe.saturating_add(era.saturating_mul(400)) + i64::from(month <= 2);
    let mut v = i16::try_from(year)
        .unwrap_or(i16::MAX)
        .to_le_bytes()
        .to_vec();
    for x in [
        u64::try_from(month).unwrap_or(1),
        u64::try_from(day).unwrap_or(1),
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60,
        0,
    ] {
        v.push(u8::try_from(x).unwrap_or(0));
    }
    v
}

/// 16 bytes of the export ID: the UUID it is, else two FNV-1a hashes.
fn material(export_id: &str) -> [u8; 16] {
    if let Ok(u) = uuid::Uuid::parse_str(export_id) {
        return *u.as_bytes();
    }
    let fnv = |seed: u64| {
        export_id.bytes().fold(seed, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    };
    let mut out = [0u8; 16];
    let (a, b) = (fnv(0xcbf2_9ce4_8422_2325), fnv(0x6c62_272e_07bb_0142));
    for (o, x) in out
        .iter_mut()
        .zip(a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
    {
        *o = x;
    }
    out
}

/// MobID `n` of this export: a SMPTE UMID with `n` as its instance number
/// and the export ID as its material.
fn mob_id(material: &[u8; 16], n: u32) -> Vec<u8> {
    let mut v = UMID_LABEL.to_vec();
    v.push(0x13);
    v.extend_from_slice(n.to_be_bytes().get(1..).unwrap_or(&[0, 0, 0]));
    v.extend_from_slice(material);
    v
}

/// A 44-byte WAV header of `frames` frames (the descriptor's `Summary`).
fn wav_summary(channels: u16, bits: u16, rate: u32, frames: u64) -> Vec<u8> {
    let block = u32::from(channels).saturating_mul(u32::from(bits / 8));
    let data = u32::try_from(frames.saturating_mul(u64::from(block))).unwrap_or(u32::MAX);
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&data.saturating_add(36).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&channels.to_le_bytes());
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&rate.saturating_mul(block).to_le_bytes());
    h.extend_from_slice(&u16::try_from(block).unwrap_or(u16::MAX).to_le_bytes());
    h.extend_from_slice(&bits.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    h
}

/// The interpolation of a fade shape (Ruling 5).
fn interpolation(shape: FadeShape) -> Auid {
    match shape {
        FadeShape::Linear | FadeShape::SCurve => def::INTERPOLATION_DEF_LINEAR,
        FadeShape::EqualPower => def::INTERPOLATION_DEF_POWER,
        FadeShape::Exponential => def::INTERPOLATION_DEF_LOG,
    }
}

/// The timecode's edit rate, FPS, drop flag and the frames of an hour.
fn timecode(fps: Fps) -> ((i32, i32), u16, bool, i64) {
    match fps {
        Fps::F24 => ((24, 1), 24, false, 86_400),
        Fps::F25 => ((25, 1), 25, false, 90_000),
        // 108 000 frames less 2 in each minute but every tenth.
        Fps::F2997Drop => ((30_000, 1_001), 30, true, 107_892),
        Fps::F30 => ((30, 1), 30, false, 108_000),
    }
}

/// `f` frames of `len` as a rational fraction (exact while both fit an
/// `i32`).
fn fraction(f: i64, len: i64) -> (i32, i32) {
    match (i32::try_from(f), i32::try_from(len)) {
        (Ok(n), Ok(d)) if d > 0 => (n, d),
        _ => ratio(f as f64 / len.max(1) as f64),
    }
}

// ---------------------------------------------------------------- the store

/// The store being built, with its weak-reference table entries.
struct B {
    s: Store,
    datadefs: u16,
    opdefs: u16,
    paramdefs: u16,
    interps: u16,
    containers: u16,
    types: u16,
    stamp: Vec<u8>,
}

/// Where a new segment goes: an object, its property and, for a vector,
/// the element.
type Slot = (ObjId, u16, Option<u32>);

impl B {
    fn obj(&mut self, (parent, pid, index): Slot, class: Auid) -> ObjId {
        self.s.add_child(parent, pid, index, class)
    }

    fn set(&mut self, id: ObjId, pid: u16, p: Prop) {
        if let Some(o) = self.s.get_mut(id) {
            o.props.insert(pid, p);
        }
    }

    fn data(&mut self, id: ObjId, pid: u16, d: Vec<u8>) {
        self.set(id, pid, Prop::Data(d));
    }

    /// Sets `at` to the one object `id` (a strong reference, or a vector of
    /// one).
    fn link(&mut self, (parent, pid, index): Slot, id: ObjId) {
        let p = match index {
            Some(_) => Prop::StrongVec(vec![id]),
            None => Prop::Strong(id),
        };
        self.set(parent, pid, p);
    }

    fn weak(table: u16, a: Auid) -> Prop {
        Prop::Weak(Self::weak_ref(table, a))
    }

    fn weak_ref(table: u16, a: Auid) -> WeakRef {
        WeakRef {
            table_index: table,
            id_pid: pid::DEFINITION_OBJECT_IDENTIFICATION,
            id: a.0.to_vec(),
        }
    }

    /// A component's data definition and length.
    fn component(&mut self, id: ObjId, kind: Auid, len: Option<i64>) {
        self.set(
            id,
            pid::COMPONENT_DATA_DEFINITION,
            Self::weak(self.datadefs, kind),
        );
        if let Some(l) = len {
            self.data(id, pid::COMPONENT_LENGTH, l.to_le_bytes().to_vec());
        }
    }

    /// `group`'s one parameter: a `ConstantValue` of `def`, rational `v`.
    fn constant(&mut self, group: ObjId, def: Auid, v: (i32, i32)) {
        let at = (group, pid::OPERATION_GROUP_PARAMETERS, Some(0));
        let p = self.obj(at, class::CONSTANT_VALUE);
        self.data(p, pid::PARAMETER_DEFINITION, def.0.to_vec());
        self.data(
            p,
            pid::CONSTANT_VALUE_VALUE,
            indirect(RATIONAL_TYPE, &rational_bytes(v)),
        );
        self.link(at, p);
    }

    /// `group`'s one parameter: a `VaryingValue` of `def` through `points`
    /// ((time, value) rationals) with interpolation `interp`.
    fn varying(&mut self, group: ObjId, def: Auid, interp: Auid, points: &[(Ratio, Ratio)]) {
        let at = (group, pid::OPERATION_GROUP_PARAMETERS, Some(0));
        let p = self.obj(at, class::VARYING_VALUE);
        self.data(p, pid::PARAMETER_DEFINITION, def.0.to_vec());
        self.set(
            p,
            pid::VARYING_VALUE_INTERPOLATION,
            Self::weak(self.interps, interp),
        );
        let mut cps = Vec::with_capacity(points.len());
        for (i, (t, v)) in (0u32..).zip(points) {
            let c = self.obj(
                (p, pid::VARYING_VALUE_POINT_LIST, Some(i)),
                class::CONTROL_POINT,
            );
            self.data(c, pid::CONTROL_POINT_TIME, rational_bytes(*t));
            self.data(
                c,
                pid::CONTROL_POINT_VALUE,
                indirect(RATIONAL_TYPE, &rational_bytes(*v)),
            );
            cps.push(c);
        }
        self.set(p, pid::VARYING_VALUE_POINT_LIST, Prop::StrongVec(cps));
        self.link(at, p);
    }

    /// An `OperationGroup` of `op`, `len` long, at `at`.
    fn group(&mut self, at: Slot, op: Auid, len: i64) -> ObjId {
        let g = self.obj(at, class::OPERATION_GROUP);
        self.component(g, def::DATA_DEF_SOUND, Some(len));
        self.set(
            g,
            pid::OPERATION_GROUP_OPERATION,
            Self::weak(self.opdefs, op),
        );
        self.link(at, g);
        g
    }

    /// A mob's ID, times and name.
    fn mob(&mut self, m: ObjId, id: &[u8], name: Option<&str>) {
        self.data(m, pid::MOB_MOB_ID, id.to_vec());
        let t = self.stamp.clone();
        self.data(m, pid::MOB_CREATION_TIME, t.clone());
        self.data(m, pid::MOB_LAST_MODIFIED, t);
        if let Some(n) = name {
            self.data(m, pid::MOB_NAME, text(n));
        }
    }

    /// Timeline slot `slot_id` of `rate`, element `i` of mob `m`'s slots.
    fn slot(
        &mut self,
        m: ObjId,
        i: u32,
        slot_id: u32,
        rate: (i32, i32),
        name: Option<&str>,
    ) -> ObjId {
        let sl = self.obj((m, pid::MOB_SLOTS, Some(i)), class::TIMELINE_MOB_SLOT);
        self.data(sl, pid::MOB_SLOT_SLOT_ID, slot_id.to_le_bytes().to_vec());
        if let Some(n) = name {
            self.data(sl, pid::MOB_SLOT_SLOT_NAME, text(n));
        }
        self.data(sl, pid::TIMELINE_MOB_SLOT_EDIT_RATE, rational_bytes(rate));
        self.data(
            sl,
            pid::TIMELINE_MOB_SLOT_ORIGIN,
            0i64.to_le_bytes().to_vec(),
        );
        sl
    }

    /// A source clip of `len` at `at`, playing slot `slot` of mob `source`
    /// from `start`.
    fn clip(&mut self, at: Slot, len: i64, source: &[u8], slot: u32, start: i64) -> ObjId {
        let c = self.obj(at, class::SOURCE_CLIP);
        self.component(c, def::DATA_DEF_SOUND, Some(len));
        self.data(c, pid::SOURCE_REFERENCE_SOURCE_ID, source.to_vec());
        self.data(
            c,
            pid::SOURCE_REFERENCE_SOURCE_MOB_SLOT_ID,
            slot.to_le_bytes().to_vec(),
        );
        self.data(c, pid::SOURCE_CLIP_START_TIME, start.to_le_bytes().to_vec());
        c
    }
}

/// One component of a slot's sequence.
enum Comp {
    Filler(i64),
    /// Length, cut point, shape.
    Transition(i64, i64, FadeShape),
    /// A piece: its length, the master mob it plays, from where, its gain.
    Clip(i64, Vec<u8>, i64, f32),
}

/// The definitions the Dictionary holds: (class, its set, AUID, name).
const DEFINITIONS: [(Auid, u16, Auid, &str); 14] = [
    (
        class::DATA_DEFINITION,
        pid::DICTIONARY_DATA_DEFINITIONS,
        def::DATA_DEF_SOUND,
        "Sound",
    ),
    (
        class::DATA_DEFINITION,
        pid::DICTIONARY_DATA_DEFINITIONS,
        def::DATA_DEF_TIMECODE,
        "Timecode",
    ),
    (
        class::DATA_DEFINITION,
        pid::DICTIONARY_DATA_DEFINITIONS,
        def::DATA_DEF_DESCRIPTIVE_METADATA,
        "DescriptiveMetadata",
    ),
    (
        class::PARAMETER_DEFINITION,
        pid::DICTIONARY_PARAMETER_DEFINITIONS,
        def::PARAMETER_DEF_LEVEL,
        "Level",
    ),
    (
        class::PARAMETER_DEFINITION,
        pid::DICTIONARY_PARAMETER_DEFINITIONS,
        def::PARAMETER_DEF_AMPLITUDE,
        "Amplitude",
    ),
    (
        class::PARAMETER_DEFINITION,
        pid::DICTIONARY_PARAMETER_DEFINITIONS,
        def::PARAMETER_DEF_PAN,
        "Pan",
    ),
    (
        class::OPERATION_DEFINITION,
        pid::DICTIONARY_OPERATION_DEFINITIONS,
        def::OPERATION_DEF_MONO_AUDIO_DISSOLVE,
        "MonoAudioDissolve",
    ),
    (
        class::OPERATION_DEFINITION,
        pid::DICTIONARY_OPERATION_DEFINITIONS,
        def::OPERATION_DEF_MONO_AUDIO_GAIN,
        "MonoAudioGain",
    ),
    (
        class::OPERATION_DEFINITION,
        pid::DICTIONARY_OPERATION_DEFINITIONS,
        def::OPERATION_DEF_MONO_AUDIO_PAN,
        "MonoAudioPan",
    ),
    (
        class::INTERPOLATION_DEFINITION,
        pid::DICTIONARY_INTERPOLATION_DEFINITIONS,
        def::INTERPOLATION_DEF_LINEAR,
        "LinearInterp",
    ),
    (
        class::INTERPOLATION_DEFINITION,
        pid::DICTIONARY_INTERPOLATION_DEFINITIONS,
        def::INTERPOLATION_DEF_POWER,
        "PowerInterp",
    ),
    (
        class::INTERPOLATION_DEFINITION,
        pid::DICTIONARY_INTERPOLATION_DEFINITIONS,
        def::INTERPOLATION_DEF_LOG,
        "LogInterp",
    ),
    (
        class::CONTAINER_DEFINITION,
        pid::DICTIONARY_CONTAINER_DEFINITIONS,
        def::CONTAINER_DEF_AAF,
        "AAF",
    ),
    (
        class::CONTAINER_DEFINITION,
        pid::DICTIONARY_CONTAINER_DEFINITIONS,
        def::CONTAINER_DEF_EXTERNAL,
        "External",
    ),
];

/// The store of walk `w`, and its `EssenceData` objects with their staged
/// files.
fn build(
    doc: &Document,
    o: &AafOptions,
    env: &ExportEnv<'_>,
    w: &Walked,
    cancel: &AtomicBool,
) -> Result<(Store, Vec<(ObjId, usize)>), IoError> {
    let mut s = Store::new(class::ROOT);
    let root = s.root;
    // First, so its weak-reference paths are the table's first entries.
    metadict::write_into(&mut s, root);
    let dict_path = |p: u16| [pid::ROOT_HEADER, pid::HEADER_DICTIONARY, p];
    let mut b = B {
        datadefs: table_index(&mut s, &dict_path(pid::DICTIONARY_DATA_DEFINITIONS)),
        opdefs: table_index(&mut s, &dict_path(pid::DICTIONARY_OPERATION_DEFINITIONS)),
        paramdefs: table_index(&mut s, &dict_path(pid::DICTIONARY_PARAMETER_DEFINITIONS)),
        interps: table_index(
            &mut s,
            &dict_path(pid::DICTIONARY_INTERPOLATION_DEFINITIONS),
        ),
        containers: table_index(&mut s, &dict_path(pid::DICTIONARY_CONTAINER_DEFINITIONS)),
        types: table_index(&mut s, &TYPE_DEFINITIONS),
        s,
        stamp: timestamp(env.modified),
    };
    let material = material(env.export_id);
    let mut ticker = Ticker { n: 0, cancel };

    // The Header and its Identification.
    let at = (root, pid::ROOT_HEADER, None);
    let header = b.obj(at, class::HEADER);
    b.link(at, header);
    let stamp = b.stamp.clone();
    b.data(
        header,
        pid::HEADER_BYTE_ORDER,
        0x4949i16.to_le_bytes().to_vec(),
    );
    b.data(header, pid::HEADER_LAST_MODIFIED, stamp.clone());
    b.data(header, pid::HEADER_VERSION, vec![1, 1]);
    b.data(
        header,
        pid::HEADER_OBJECT_MODEL_VERSION,
        1u32.to_le_bytes().to_vec(),
    );
    b.data(
        header,
        pid::HEADER_OPERATIONAL_PATTERN,
        def::OP_DEF_EDIT_PROTOCOL.0.to_vec(),
    );
    let at = (header, pid::HEADER_IDENTIFICATION_LIST, Some(0));
    let ident = b.obj(at, class::IDENTIFICATION);
    b.link(at, ident);
    b.data(ident, pid::IDENTIFICATION_COMPANY_NAME, text("VizDAW"));
    b.data(ident, pid::IDENTIFICATION_PRODUCT_NAME, text("VizDAW"));
    b.data(
        ident,
        pid::IDENTIFICATION_PRODUCT_VERSION_STRING,
        text(env.app_version),
    );
    b.data(
        ident,
        pid::IDENTIFICATION_PRODUCT_ID,
        def::VIZDAW_PRODUCT_ID.0.to_vec(),
    );
    b.data(ident, pid::IDENTIFICATION_DATE, stamp);
    b.data(ident, pid::IDENTIFICATION_PLATFORM, text("macOS"));
    b.data(
        ident,
        pid::IDENTIFICATION_GENERATION_AUID,
        material.to_vec(),
    );

    // The Dictionary: the definitions the composition references.
    let at = (header, pid::HEADER_DICTIONARY, None);
    let dict = b.obj(at, class::DICTIONARY);
    b.link(at, dict);
    let mut sets: BTreeMap<u16, Vec<ObjId>> = BTreeMap::new();
    for (cls, set_pid, id, name) in DEFINITIONS {
        let i = sets.get(&set_pid).map_or(0, Vec::len);
        let d = b.obj(
            (dict, set_pid, Some(u32::try_from(i).unwrap_or(u32::MAX))),
            cls,
        );
        b.data(d, pid::DEFINITION_OBJECT_IDENTIFICATION, id.0.to_vec());
        b.data(d, pid::DEFINITION_OBJECT_NAME, text(name));
        if cls == class::OPERATION_DEFINITION {
            let snd = B::weak(b.datadefs, def::DATA_DEF_SOUND);
            b.set(d, pid::OPERATION_DEFINITION_DATA_DEFINITION, snd);
            let (inputs, param) = if id == def::OPERATION_DEF_MONO_AUDIO_DISSOLVE {
                (2i32, def::PARAMETER_DEF_LEVEL)
            } else if id == def::OPERATION_DEF_MONO_AUDIO_GAIN {
                (1, def::PARAMETER_DEF_AMPLITUDE)
            } else {
                (1, def::PARAMETER_DEF_PAN)
            };
            b.data(
                d,
                pid::OPERATION_DEFINITION_NUMBER_INPUTS,
                inputs.to_le_bytes().to_vec(),
            );
            b.data(d, pid::OPERATION_DEFINITION_IS_TIME_WARP, vec![0]);
            let defined = Prop::WeakSet(vec![B::weak_ref(b.paramdefs, param)]);
            b.set(d, pid::OPERATION_DEFINITION_PARAMETERS_DEFINED, defined);
        }
        if cls == class::PARAMETER_DEFINITION {
            let t = Prop::Weak(WeakRef {
                table_index: b.types,
                id_pid: pid::META_DEFINITION_IDENTIFICATION,
                id: RATIONAL_TYPE.0.to_vec(),
            });
            b.set(d, pid::PARAMETER_DEFINITION_TYPE, t);
        }
        sets.entry(set_pid).or_default().push(d);
    }
    for (set_pid, v) in sets {
        b.set(dict, set_pid, Prop::StrongSet(v));
    }

    // MobIDs: the file mobs, the master mobs, then the composition.
    let mut next = 0u32;
    let mut new_id = || {
        next = next.saturating_add(1);
        mob_id(&material, next)
    };
    let bits: u16 = if o.depth == BitDepth::Int16 { 16 } else { 24 };
    // Per staged file: its MobID, rate, frames and channels.
    let mut files = Vec::with_capacity(w.staged.len());
    for st in &w.staged {
        ticker.tick()?;
        let (rate, frames, ch) = doc.media().get(&st.media).map_or((w.rate, 0, 1), |m| {
            let ch = if st.channel.is_some() {
                1
            } else {
                m.channels.max(1)
            };
            (m.sample_rate.max(1), m.length_frames, ch)
        });
        files.push((new_id(), rate, frames, ch));
    }
    let mut masters: BTreeMap<(usize, Option<String>), Vec<u8>> = BTreeMap::new();
    let mut master_list: Vec<(Vec<u8>, usize, Option<String>)> = Vec::new();
    for (t, per_side) in w.tracks.iter().zip(&w.files) {
        for idx in per_side {
            for (r, f) in t.regs.iter().zip(idx) {
                let Some(f) = f else { continue };
                let key = (*f, r.name.clone());
                if let std::collections::btree_map::Entry::Vacant(v) = masters.entry(key) {
                    let id = new_id();
                    v.insert(id.clone());
                    master_list.push((id, *f, r.name.clone()));
                }
            }
        }
    }
    let comp_id = new_id();

    // The content: the composition first, then the master and file mobs.
    let at = (header, pid::HEADER_CONTENT, None);
    let content = b.obj(at, class::CONTENT_STORAGE);
    b.link(at, content);
    let mut mobs: Vec<ObjId> = Vec::new();
    let mob_at = |n: usize| {
        (
            content,
            pid::CONTENT_STORAGE_MOBS,
            Some(u32::try_from(n).unwrap_or(u32::MAX)),
        )
    };
    let comp = b.obj(mob_at(0), class::COMPOSITION_MOB);
    mobs.push(comp);
    let seq_name = doc
        .sequence(o.seq)
        .map(|s| s.name.clone())
        .unwrap_or_default();
    b.mob(comp, &comp_id, Some(&seq_name));
    b.data(comp, pid::MOB_USAGE_CODE, def::USAGE_TOP_LEVEL.0.to_vec());
    // Song info as tagged comments (Ruling 9).
    let mut tags = Vec::new();
    for (field, value) in METADATA_FIELDS.iter().zip(doc.settings().info.fields()) {
        let Some(v) = value else { continue };
        let i = u32::try_from(tags.len()).unwrap_or(u32::MAX);
        let tv = b.obj((comp, pid::MOB_USER_COMMENTS, Some(i)), class::TAGGED_VALUE);
        b.data(tv, pid::TAGGED_VALUE_NAME, text(field));
        b.data(tv, pid::TAGGED_VALUE_VALUE, indirect(STRING_TYPE, &text(v)));
        tags.push(tv);
    }
    if !tags.is_empty() {
        b.set(comp, pid::MOB_USER_COMMENTS, Prop::StrongVec(tags));
    }

    // The audio slots.
    let rate = (i32::try_from(w.rate).unwrap_or(i32::MAX), 1);
    let mut slots = Vec::new();
    let mut slot_id = 0u32;
    for (t, per_side) in w.tracks.iter().zip(&w.files) {
        for (side, idx) in per_side.iter().enumerate() {
            ticker.tick()?;
            slot_id = slot_id.saturating_add(1);
            let name = match (t.pairs, side) {
                (true, 0) => format!("{} L", t.name),
                (true, _) => format!("{} R", t.name),
                _ => t.name.clone(),
            };
            let i = u32::try_from(slots.len()).unwrap_or(u32::MAX);
            let sl = b.slot(comp, i, slot_id, rate, Some(&name));
            slots.push(sl);
            let mut at: Slot = (sl, pid::MOB_SLOT_SEGMENT, None);
            // Track volume and pan around the sequence (Ruling 6).
            if o.automation {
                let (gain, pan) = t
                    .strip
                    .and_then(|st| w.curves.get(&st))
                    .cloned()
                    .unwrap_or((Curve::Constant(1.0), Curve::Constant(0.5)));
                let pan = match (t.pairs, side) {
                    (true, 0) => Curve::Constant(0.0),
                    (true, _) => Curve::Constant(1.0),
                    _ => pan,
                };
                for (op, param, c) in [
                    (
                        def::OPERATION_DEF_MONO_AUDIO_GAIN,
                        def::PARAMETER_DEF_AMPLITUDE,
                        gain,
                    ),
                    (
                        def::OPERATION_DEF_MONO_AUDIO_PAN,
                        def::PARAMETER_DEF_PAN,
                        pan,
                    ),
                ] {
                    let g = b.group(at, op, w.length);
                    match c {
                        Curve::Constant(v) => b.constant(g, param, ratio(v)),
                        Curve::Points(pts) => {
                            let pts: Vec<_> = pts
                                .iter()
                                .map(|(f, v)| (fraction(*f, w.length.max(1)), ratio(*v)))
                                .collect();
                            b.varying(g, param, def::INTERPOLATION_DEF_LINEAR, &pts);
                        }
                    }
                    at = (g, pid::OPERATION_GROUP_INPUT_SEGMENTS, Some(0));
                }
            }
            let sq = b.obj(at, class::SEQUENCE);
            b.link(at, sq);
            b.component(sq, def::DATA_DEF_SOUND, Some(w.length));
            let comps = components(t, idx, &masters, w.length);
            let mut kids = Vec::with_capacity(comps.len());
            for (k, c) in (0u32..).zip(comps) {
                ticker.tick()?;
                let at = (sq, pid::SEQUENCE_COMPONENTS, Some(k));
                let id = match c {
                    Comp::Filler(len) => {
                        let f = b.obj(at, class::FILLER);
                        b.component(f, def::DATA_DEF_SOUND, Some(len));
                        f
                    }
                    Comp::Transition(len, cut, shape) => {
                        let tr = b.obj(at, class::TRANSITION);
                        b.component(tr, def::DATA_DEF_SOUND, Some(len));
                        b.data(tr, pid::TRANSITION_CUT_POINT, cut.to_le_bytes().to_vec());
                        let g = b.group(
                            (tr, pid::TRANSITION_OPERATION_GROUP, None),
                            def::OPERATION_DEF_MONO_AUDIO_DISSOLVE,
                            len,
                        );
                        b.varying(
                            g,
                            def::PARAMETER_DEF_LEVEL,
                            interpolation(shape),
                            &[((0, 1), (0, 1)), ((1, 1), (1, 1))],
                        );
                        tr
                    }
                    Comp::Clip(len, master, start, gain_db) if o.clip_gain && gain_db != 0.0 => {
                        let g = b.obj(at, class::OPERATION_GROUP);
                        b.component(g, def::DATA_DEF_SOUND, Some(len));
                        let op = B::weak(b.opdefs, def::OPERATION_DEF_MONO_AUDIO_GAIN);
                        b.set(g, pid::OPERATION_GROUP_OPERATION, op);
                        let amp = (10f64.powf(f64::from(gain_db) / 20.0) * f64::from(GAIN_DEN))
                            .round()
                            .clamp(0.0, f64::from(i32::MAX));
                        // Finite and in range, so `as` is exact.
                        let n = if amp.is_finite() { amp as i32 } else { 0 };
                        b.constant(g, def::PARAMETER_DEF_AMPLITUDE, (n, GAIN_DEN));
                        let input = (g, pid::OPERATION_GROUP_INPUT_SEGMENTS, Some(0));
                        let c = b.clip(input, len, &master, 1, start);
                        b.link(input, c);
                        g
                    }
                    Comp::Clip(len, master, start, _) => b.clip(at, len, &master, 1, start),
                };
                kids.push(id);
            }
            b.set(sq, pid::SEQUENCE_COMPONENTS, Prop::StrongVec(kids));
        }
    }

    // The start timecode (Ruling 3).
    let (tc_rate, fps, drop, hour) = timecode(o.fps);
    slot_id = slot_id.saturating_add(1);
    let i = u32::try_from(slots.len()).unwrap_or(u32::MAX);
    let tsl = b.slot(comp, i, slot_id, tc_rate, Some("Timecode"));
    slots.push(tsl);
    let at = (tsl, pid::MOB_SLOT_SEGMENT, None);
    let tc = b.obj(at, class::TIMECODE);
    b.link(at, tc);
    let tc_len = {
        let num = i128::from(w.length) * i128::from(tc_rate.0);
        let den = i128::from(w.rate.max(1)) * i128::from(tc_rate.1);
        i64::try_from((num + den - 1) / den).unwrap_or(i64::MAX)
    };
    b.component(tc, def::DATA_DEF_TIMECODE, Some(tc_len));
    let start: i64 = if o.hour_start { hour } else { 0 };
    b.data(tc, pid::TIMECODE_START, start.to_le_bytes().to_vec());
    b.data(tc, pid::TIMECODE_FPS, fps.to_le_bytes().to_vec());
    b.data(tc, pid::TIMECODE_DROP, vec![u8::from(drop)]);

    // The markers (Ruling 9).
    if !w.markers.is_empty() {
        slot_id = slot_id.saturating_add(1);
        let i = u32::try_from(slots.len()).unwrap_or(u32::MAX);
        let esl = b.obj((comp, pid::MOB_SLOTS, Some(i)), class::EVENT_MOB_SLOT);
        slots.push(esl);
        b.data(esl, pid::MOB_SLOT_SLOT_ID, slot_id.to_le_bytes().to_vec());
        b.data(esl, pid::MOB_SLOT_SLOT_NAME, text("Markers"));
        b.data(esl, pid::EVENT_MOB_SLOT_EDIT_RATE, rational_bytes(rate));
        let at = (esl, pid::MOB_SLOT_SEGMENT, None);
        let sq = b.obj(at, class::SEQUENCE);
        b.link(at, sq);
        b.component(sq, def::DATA_DEF_DESCRIPTIVE_METADATA, None);
        let mut kids = Vec::with_capacity(w.markers.len());
        for (k, (pos, name)) in (0u32..).zip(&w.markers) {
            ticker.tick()?;
            let e = b.obj(
                (sq, pid::SEQUENCE_COMPONENTS, Some(k)),
                class::DESCRIPTIVE_MARKER,
            );
            b.component(e, def::DATA_DEF_DESCRIPTIVE_METADATA, Some(0));
            b.data(e, pid::EVENT_POSITION, pos.to_le_bytes().to_vec());
            b.data(e, pid::EVENT_COMMENT, text(name));
            kids.push(e);
        }
        b.set(sq, pid::SEQUENCE_COMPONENTS, Prop::StrongVec(kids));
    }
    b.set(comp, pid::MOB_SLOTS, Prop::StrongVec(slots));

    // The master mobs: one slot playing their file mob's.
    for (id, f, name) in &master_list {
        ticker.tick()?;
        let Some((src, mrate, frames, _)) = files.get(*f) else {
            continue;
        };
        let m = b.obj(mob_at(mobs.len()), class::MASTER_MOB);
        mobs.push(m);
        b.mob(m, id, name.as_deref());
        let len = i64::try_from(*frames).unwrap_or(i64::MAX);
        let sl = b.slot(
            m,
            0,
            1,
            (i32::try_from(*mrate).unwrap_or(i32::MAX), 1),
            None,
        );
        let at = (sl, pid::MOB_SLOT_SEGMENT, None);
        let c = b.clip(at, len, src, 1, 0);
        b.link(at, c);
        b.set(m, pid::MOB_SLOTS, Prop::StrongVec(vec![sl]));
    }
    // The file source mobs: a WAVE descriptor, the essence or a locator.
    let mut essence = Vec::new();
    for (i, (st, (id, mrate, frames, ch))) in w.staged.iter().zip(&files).enumerate() {
        ticker.tick()?;
        let m = b.obj(mob_at(mobs.len()), class::SOURCE_MOB);
        mobs.push(m);
        b.mob(m, id, Some(&st.name));
        let len = i64::try_from(*frames).unwrap_or(i64::MAX);
        let mr = (i32::try_from(*mrate).unwrap_or(i32::MAX), 1);
        let sl = b.slot(m, 0, 1, mr, None);
        let at = (sl, pid::MOB_SLOT_SEGMENT, None);
        let c = b.clip(at, len, &[0u8; 32], 0, 0);
        b.link(at, c);
        b.set(m, pid::MOB_SLOTS, Prop::StrongVec(vec![sl]));
        let at = (m, pid::SOURCE_MOB_ESSENCE_DESCRIPTION, None);
        let d = b.obj(at, class::WAVE_DESCRIPTOR);
        b.link(at, d);
        b.data(d, pid::FILE_DESCRIPTOR_SAMPLE_RATE, rational_bytes(mr));
        b.data(d, pid::FILE_DESCRIPTOR_LENGTH, len.to_le_bytes().to_vec());
        b.data(
            d,
            pid::WAVE_DESCRIPTOR_SUMMARY,
            wav_summary(*ch, bits, *mrate, *frames),
        );
        let container = if o.embed {
            def::CONTAINER_DEF_AAF
        } else {
            def::CONTAINER_DEF_EXTERNAL
        };
        b.set(
            d,
            pid::FILE_DESCRIPTOR_CONTAINER_FORMAT,
            B::weak(b.containers, container),
        );
        if o.embed {
            essence.push((id.clone(), i));
        } else {
            let at = (d, pid::ESSENCE_DESCRIPTOR_LOCATOR, Some(0));
            let l = b.obj(at, class::NETWORK_LOCATOR);
            b.link(at, l);
            b.data(
                l,
                pid::NETWORK_LOCATOR_URL_STRING,
                text(&percent_encode(&st.file)),
            );
        }
    }
    b.set(content, pid::CONTENT_STORAGE_MOBS, Prop::StrongSet(mobs));
    // The embedded essence.
    let mut data = Vec::with_capacity(essence.len());
    let mut out = Vec::with_capacity(essence.len());
    for (k, (id, i)) in (0u32..).zip(essence) {
        let e = b.obj(
            (content, pid::CONTENT_STORAGE_ESSENCE_DATA, Some(k)),
            class::ESSENCE_DATA,
        );
        b.data(e, pid::ESSENCE_DATA_MOB_ID, id);
        b.set(
            e,
            pid::ESSENCE_DATA_DATA,
            Prop::Stream(ESSENCE_STREAM.to_owned()),
        );
        data.push(e);
        out.push((e, i));
    }
    if !data.is_empty() {
        b.set(
            content,
            pid::CONTENT_STORAGE_ESSENCE_DATA,
            Prop::StrongSet(data),
        );
    }
    Ok((b.s, out))
}

/// One slot's components (Ruling 4): its pieces with their fades and
/// crossfades, the fillers between, and one at the end up to `length`.
fn components(
    t: &Track,
    files: &[Option<usize>],
    masters: &BTreeMap<(usize, Option<String>), Vec<u8>>,
    length: i64,
) -> Vec<Comp> {
    let mut out = Vec::new();
    // Where the components so far end (a fade-out's transition moves it
    // back).
    let mut pos = 0i64;
    for p in &t.pieces {
        let Some(r) = t.regs.get(p.reg) else { continue };
        let master = files
            .get(p.reg)
            .copied()
            .flatten()
            .and_then(|f| masters.get(&(f, r.name.clone())))
            .cloned()
            .unwrap_or_default();
        match p.xfade {
            Some((l, shape)) => out.push(Comp::Transition(l, 0, shape)),
            None => {
                let fin = p.fade_in.map_or(0, |f| f.0);
                let gap = p.start.saturating_sub(pos).saturating_add(fin);
                if gap > 0 {
                    out.push(Comp::Filler(gap));
                }
                if let Some((l, shape)) = p.fade_in {
                    out.push(Comp::Transition(l, 0, shape));
                }
            }
        }
        let len = p.end.saturating_sub(p.start);
        let start = r.offset.saturating_add(p.start.saturating_sub(r.start));
        out.push(Comp::Clip(len, master, start, r.gain_db));
        pos = p.end;
        if let Some((l, shape)) = p.fade_out {
            out.push(Comp::Transition(l, l, shape));
            pos = pos.saturating_sub(l);
        }
    }
    if length > pos {
        out.push(Comp::Filler(length.saturating_sub(pos)));
    }
    out
}

/// A path as a URL path: bytes other than unreserved characters and `/`
/// percent-encoded.
fn percent_encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The export folder `dest` will be in once committed: its folder (made
/// absolute), a staging name `.<name>.partial` read as `<name>`.
fn final_folder(dest: &Path) -> Option<PathBuf> {
    let dir = std::path::absolute(dest.parent()?).ok()?;
    let name = dir.file_name()?.to_str()?;
    match name
        .strip_prefix('.')
        .and_then(|n| n.strip_suffix(".partial"))
    {
        Some(real) if !real.is_empty() => Some(dir.parent()?.join(real)),
        _ => Some(dir),
    }
}

/// The store with an absolute `file://` locator before each relative one
/// (external audio); `None` when there is none.
fn absolute_locators(s: &Store, dest: &Path) -> Option<Store> {
    let base = percent_encode(&final_folder(dest)?.to_string_lossy());
    let sep = if base.ends_with('/') { "" } else { "/" };
    let mut out = s.clone();
    let mut changed = false;
    let descriptors: Vec<ObjId> = (0..out.objects.len())
        .filter_map(|i| u32::try_from(i).ok().map(ObjId))
        .filter(|id| {
            out.get(*id)
                .is_some_and(|o| o.class == class::WAVE_DESCRIPTOR)
        })
        .collect();
    for d in descriptors {
        let Some(Prop::StrongVec(locs)) = out.prop(d, pid::ESSENCE_DESCRIPTOR_LOCATOR).cloned()
        else {
            continue;
        };
        let Some(Prop::Data(url)) = locs
            .first()
            .and_then(|l| out.prop(*l, pid::NETWORK_LOCATOR_URL_STRING))
            .cloned()
        else {
            continue;
        };
        let units: Vec<u16> = url
            .chunks_exact(2)
            .map(|c| {
                u16::from_le_bytes([
                    c.first().copied().unwrap_or(0),
                    c.get(1).copied().unwrap_or(0),
                ])
            })
            .take_while(|u| *u != 0)
            .collect();
        let rel = String::from_utf16_lossy(&units);
        if rel.contains("://") {
            continue;
        }
        let abs = format!("file://{base}{sep}{rel}");
        // Both locators anew: a vector's storage names follow the index.
        let mut made = Vec::with_capacity(2);
        for (k, u) in (0u32..).zip([abs, rel]) {
            let l = out.add_child(
                d,
                pid::ESSENCE_DESCRIPTOR_LOCATOR,
                Some(k),
                class::NETWORK_LOCATOR,
            );
            if let Some(o) = out.get_mut(l) {
                o.props
                    .insert(pid::NETWORK_LOCATOR_URL_STRING, Prop::Data(text(&u)));
            }
            made.push(l);
        }
        if let Some(o) = out.get_mut(d) {
            o.props
                .insert(pid::ESSENCE_DESCRIPTOR_LOCATOR, Prop::StrongVec(made));
        }
        changed = true;
    }
    changed.then_some(out)
}
