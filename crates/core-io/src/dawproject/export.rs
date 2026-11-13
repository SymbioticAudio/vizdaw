//! DAWproject export (plan 6d Tasks 4–5): one sequence and the mixer as a
//! [`DpProject`], `metadata.xml`, the plug-in state files, the return record
//! and the snapshot's projection.
//!
//! What is written is what plays: the tracks are read through
//! [`midi_out::source`] with their mute and solo cleared (a muted or
//! unsoloed track is written, with a Note; its channel's `Mute` is its
//! strip's). File time 0 is [`midi_out::file_start`], the pickup's start
//! (Ruling 3).
//!
//! **Structure** (Rulings 5–7): one `Track` per track in sequence order (a
//! track playing notes and audio is two), then the buses and the submixes
//! made for shared audio paths in mixer order, then the master. A track's
//! channel is its strip: the instrument strip of its destination for notes
//! (copied to each track when several share it), the audio path strip of
//! its output for audio (a submix with plain channels into it when several
//! share it). External MIDI and tracks with no strip get a plain channel.
//! IDs come from VizDAW's objects (Ruling 5), so they are unique and valid
//! XML names.
//!
//! Everything the file cannot hold is listed in the report; each `what` is
//! a constant (or, when it names something, a function) here.

use super::export_timeline::arrange;
pub use super::export_timeline::{
    CROSSFADES, EVENTS_OUTSIDE, FADE_SHAPES, MIDI_GROUPS, MUTED_NOTES, NESTED_AUDIO, NESTED_MUTED,
    NOTE_ATTRIBUTES, OTHER_PER_NOTE, OVERLAPS, PLAY_QUANTIZED, REGION_GAIN, SOLO_AUTOMATION, SYSEX,
    TEXTS, TRANSPOSED, UNPLACED_AUTOMATION, VELOCITY_SCALE_AUTOMATION,
};
use super::plugins::{PlistCodec, state_entry};
use super::record::{self, ObjectRef, Record, Snapshot};
use super::schema::*;
use super::units::{EXPORT_LINE, TagKind, beats, linear_gain, pan_normalized, swatch_hex, tag};
use crate::IoError;
use crate::midi_out::{self, file_start, start_bpm, start_meters};
use crate::names::safe_part;
use crate::report::{ItemKind, Report};
use core_model::automation::{AutomationMode, AutomationTarget, automated_targets, slot_params};
use core_model::command::{Command, EventCmd, apply};
use core_model::document::{DestKind, Document, SlotOwner};
use core_model::event::EventBody;
use core_model::ids::{DestId, EventId, LaneId, MediaId, PluginInstanceId, SeqId, StripId};
use core_model::lane::{Lane, MonitorMode};
use core_model::mixer::{
    GAIN_MAX_DB, Output, ParamId, PluginFormat, PluginSlot, Strip, StripKind, Swatch,
};
use core_model::query::ResolvedEvent;
use core_model::sequence::Sequence;
use core_model::time::Tick;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------- the API

/// Whether a region's gain is applied to its exported audio copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionGain {
    /// Each region with gain gets its own copy, with the gain applied.
    Apply,
    /// Regions share their media's copy; the gain is listed as left out.
    LeaveOut,
}

/// How a track's top-level notes are grouped into clips (Ruling 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipRule {
    /// A new clip after a gap of at least this long.
    SilenceGap(Tick),
    /// Windows of this many bars from bar 1.
    FixedBars(u32),
    /// One clip per track.
    WholeTrack,
}

/// Media whose audio the export writes no file for, each with the `what`
/// of its Left out line (the detail is the media item's name). Only the
/// caller knows which media is offline (plan 6d Task 9, S9).
pub type LeftOutAudio = BTreeMap<MediaId, String>;

/// What to export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOptions {
    /// The sequence.
    pub seq: SeqId,
    /// What happens to region gain.
    pub region_gain: RegionGain,
    /// How notes are grouped into clips.
    pub clips: ClipRule,
    /// Media written without audio: its regions become clips with no
    /// content, so `project.xml` names no entry the package lacks; each
    /// item used gets its Left out line once, and no [`AudioFile`].
    pub left_out: LeftOutAudio,
}

/// What the export needs from outside the document.
#[derive(Clone, Copy)]
pub struct ExportEnv<'a> {
    /// The AU property-list conversion.
    pub plist: &'a dyn PlistCodec,
    /// VizDAW's version, for `Application` and the record.
    pub app_version: &'a str,
    /// The project's ID.
    pub project_id: &'a str,
    /// This export's ID.
    pub export_id: &'a str,
    /// When, in Unix seconds.
    pub exported_at: u64,
}

/// An audio entry to write: the media item's audio, as WAV, at `gain_db`.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFile {
    /// The package entry, `audio/001.wav`.
    pub entry: String,
    /// The media item.
    pub media: MediaId,
    /// The gain to apply, in dB.
    pub gain_db: f32,
    /// The human name (`Audio@name`).
    pub name: String,
}

/// An export's files and report. The caller writes them into the package.
#[derive(Debug, Clone, PartialEq)]
pub struct DpExport {
    /// `project.xml`.
    pub project_xml: String,
    /// `metadata.xml`.
    pub metadata_xml: String,
    /// `vizdaw/record.json`.
    pub record_json: String,
    /// The snapshot, `Interchange/<export id>.json` in the project.
    pub snapshot_json: String,
    /// The audio entries.
    pub audio: Vec<AudioFile>,
    /// The plug-in state entries, `(entry, bytes)`, in write order.
    pub states: Vec<(String, Vec<u8>)>,
    /// What changed, was left out or is worth knowing.
    pub report: Report,
    /// The plug-in slots written as devices, each once (copies of a shared
    /// instrument included): the caller adds the lines only it can know
    /// ([`not_loaded`], [`plugin_settings`]) for these.
    pub slots: BTreeSet<PluginInstanceId>,
}

/// A dry run's report and the plug-in slots it would write as devices
/// (as [`DpExport::slots`]).
#[derive(Debug, Clone, PartialEq)]
pub struct DpCheck {
    /// What the export would report.
    pub report: Report,
    /// The plug-in slots the export would write as devices.
    pub slots: BTreeSet<PluginInstanceId>,
}

/// Sequence `o.seq` of `doc` as a DAWproject's files. Looks at `cancel`
/// every [`CANCEL_EVERY`] events and between stages.
pub fn export(
    doc: &Document,
    o: &ExportOptions,
    env: &ExportEnv<'_>,
    cancel: &AtomicBool,
) -> Result<DpExport, IoError> {
    let built = build(doc, o, env.plist, env.app_version, cancel)?;
    let Built {
        layout,
        project,
        states,
        audio,
        ids,
        mut report,
    } = built;
    let project_xml = write_project_reporting(&project, &mut report);
    drop(project);
    let slots = device_slots(&layout);
    stop(cancel)?;
    let metadata_xml = write_metadata(&metadata(doc, env.export_id));
    let projection = record::project(&layout, cancel)?;
    stop(cancel)?;
    let json = core_model::file::to_json(doc).map_err(unwritable)?;
    stop(cancel)?;
    let document: serde_json::Value = serde_json::from_str(&json).map_err(unwritable)?;
    drop(json);
    stop(cancel)?;
    let seq = o.seq.0;
    let snapshot = Snapshot {
        format: record::FORMAT,
        project_id: env.project_id.to_owned(),
        export_id: env.export_id.to_owned(),
        exported_at: env.exported_at,
        seq,
        origin: layout.origin,
        ids: ids.clone(),
        projection,
    };
    let snapshot_json = serde_json::to_string(&snapshot).map_err(unwritable)?;
    drop(snapshot);
    let rec = Record {
        format: record::FORMAT,
        project_id: env.project_id.to_owned(),
        export_id: env.export_id.to_owned(),
        exported_at: env.exported_at,
        app_version: env.app_version.to_owned(),
        seq,
        origin: layout.origin,
        ids,
        document,
    };
    let record_json = serde_json::to_string(&rec).map_err(unwritable)?;
    stop(cancel)?;
    Ok(DpExport {
        project_xml,
        metadata_xml,
        record_json,
        snapshot_json,
        audio,
        states,
        report,
        slots,
    })
}

/// The report `export` gives and its device slots, without the record
/// (the dry run). It takes the same `plist` codec as the export, so the AU
/// settings lines are the export's: pass
/// [`NoPlist`](super::plugins::NoPlist) only where the export would use it
/// too.
pub fn check(
    doc: &Document,
    o: &ExportOptions,
    plist: &dyn PlistCodec,
    cancel: &AtomicBool,
) -> Result<DpCheck, IoError> {
    let mut built = build(doc, o, plist, env!("CARGO_PKG_VERSION"), cancel)?;
    write_project_reporting(&built.project, &mut built.report);
    stop(cancel)?;
    Ok(DpCheck {
        slots: device_slots(&built.layout),
        report: built.report,
    })
}

/// "<Song>.dawproject": `safe_part` of the title, else the sequence's name, reserving 11 bytes.
pub fn file_name(doc: &Document, seq: SeqId) -> Result<String, IoError> {
    let s = sequence(doc, seq)?;
    let title = doc
        .settings()
        .info
        .title
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(&s.name);
    Ok(format!("{}{SUFFIX}", safe_part(title, SUFFIX.len())))
}

/// The plug-in slots `l` writes as devices, each once.
fn device_slots(l: &Layout<'_>) -> BTreeSet<PluginInstanceId> {
    l.chans
        .iter()
        .flat_map(|c| c.slots().into_iter().map(|(s, _, _)| s.id))
        .collect()
}

/// The package's extension, with its dot.
const SUFFIX: &str = ".dawproject";

/// `export` and `check` look at the cancel flag every this many events.
pub const CANCEL_EVERY: usize = 65_536;

// ---------------------------------------------------------------- the report

const NO_SEQ: &str = "The sequence to export is not in this project.";

/// Left out: takes other than the active one (by track).
pub const INACTIVE_TAKES: &str = "Inactive takes";
/// Left out: sequences the export does not reach (a count).
pub const OTHER_SEQUENCES: &str = "Other sequences (not exported)";
/// Left out: sequences' keyboard slots (by sequence).
pub const KEYBOARD_SLOTS: &str = "Keyboard slots";
/// Left out: MIDI input effects (by track).
pub const INPUT_FX: &str = "Input effects";
/// Left out: track inputs, record arm and monitoring (by track).
pub const INPUTS: &str = "Track inputs and monitoring";
/// Left out: MPE input decoding (by track).
pub const MPE_INPUT: &str = "MPE input settings";
/// Left out: drum rows (by track).
pub const DRUM_ROWS: &str = "Drum rows (the notes are kept)";
/// Left out: automation modes other than Read (by strip).
pub const AUTOMATION_MODES: &str = "Automation modes";
/// Left out: MIDI-learn maps (a count).
pub const MIDI_LEARN: &str = "MIDI learn";
/// Left out: sidechain inputs (by plug-in).
pub const SIDECHAINS: &str = "Sidechains";
/// Left out: strips of a plug-in instrument's extra outputs (by strip).
pub const EXTRA_OUTPUTS: &str = "Instrument extra outputs";
/// Left out: the key signatures of the meter map.
pub const KEY_SIGNATURES: &str = "Key signatures";
/// Left out: automated plug-in parameters whose ID needs more than 32 bits
/// (by plug-in), with their automation.
pub const PARAMS_PAST_32_BITS: &str = "Plug-in parameters past 32 bits";
/// Left out: the sequence's comment (the file has no place for it).
pub const SEQUENCE_COMMENTS: &str = "Sequence comments";
/// Left out: instrument and audio path strips no exported track plays
/// through (by strip).
pub const UNPLAYED_STRIPS: &str = "Strips no track of this sequence plays through";
/// Left out: markers before the pickup's start (by marker).
pub const EARLY_MARKERS: &str = "Markers before the start of the song";
/// Changed: an audio path several tracks share (by strip).
pub const SHARED_AUDIO_PATHS: &str = "Audio paths shared by several tracks became submix channels";
/// Changed: tracks holding notes and audio (by track).
pub const SPLIT_TRACKS: &str = "Tracks with notes and audio were split in two";
/// Note: the song has a pickup (Ruling 3).
pub const PICKUP: &str =
    "The song starts before bar 1; other DAWs show it from the pickup's start.";

/// Changed: instrument strip `strip` several tracks share was copied to
/// each (the detail is "<n> tracks").
pub fn shared_instrument(strip: &str) -> String {
    format!("Instrument '{strip}' shared by several tracks was copied to each")
}

/// Left out: plug-in `name`'s settings could not be written.
pub fn plugin_settings(name: &str) -> String {
    format!("Settings of plug-in '{name}'")
}

/// Note: plug-in `name` has no stored state.
pub fn no_saved_settings(name: &str) -> String {
    format!("Plug-in '{name}' has no saved settings")
}

/// Note: a placeholder slot's stored state was written. Only the caller
/// knows which slots are not loaded (plan 6d Task 9).
pub fn not_loaded(name: &str) -> String {
    format!("Plug-in '{name}' is not loaded here; its saved settings were written")
}

/// Note: track `track` plays an external MIDI destination.
pub fn external_midi(track: &str) -> String {
    format!("Track '{track}' plays an external MIDI destination; the file has no instrument for it")
}

/// Note: track `track` plays the built-in synth.
pub fn builtin_synth(track: &str) -> String {
    format!("Track '{track}' plays the built-in synth here")
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

fn unwritable(e: impl std::fmt::Display) -> IoError {
    IoError::Refused(format!("This DAWproject file can't be written: {e}"))
}

fn stop(cancel: &AtomicBool) -> Result<(), IoError> {
    if cancel.load(Ordering::Relaxed) {
        Err(IoError::Cancelled)
    } else {
        Ok(())
    }
}

/// Looks at the cancel flag every [`CANCEL_EVERY`] calls of `tick`.
pub(super) struct Ticker<'a> {
    n: usize,
    cancel: &'a AtomicBool,
}

impl<'a> Ticker<'a> {
    pub(super) fn new(cancel: &'a AtomicBool) -> Self {
        Ticker { n: 0, cancel }
    }

    /// One more item.
    pub(super) fn tick(&mut self) -> Result<(), IoError> {
        #[cfg(feature = "test-hooks")]
        hook::on_tick(self.cancel);
        self.n = self.n.wrapping_add(1);
        if self.n % CANCEL_EVERY == 0 {
            stop(self.cancel)
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "test-hooks")]
mod hook {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, Ordering};

    thread_local! {
        pub(super) static AT: Cell<Option<usize>> = const { Cell::new(None) };
        pub(super) static SEEN: Cell<usize> = const { Cell::new(0) };
        pub(super) static AFTER: Cell<usize> = const { Cell::new(0) };
    }

    /// Counts one item; sets `cancel` at the chosen item, then counts the
    /// items processed after it.
    pub(super) fn on_tick(cancel: &AtomicBool) {
        let Some(at) = AT.with(Cell::get) else { return };
        let n = SEEN.with(|c| {
            c.set(c.get().saturating_add(1));
            c.get()
        });
        if n == at {
            cancel.store(true, Ordering::Relaxed);
        } else if n > at {
            AFTER.with(|c| c.set(c.get().saturating_add(1)));
        }
    }
}

/// Test hook: this thread's next exports and dry runs set their cancel flag
/// at their `items`-th event (counted across stages), and count the events
/// they process after it; `None` turns the hook off. Resets the counts.
#[cfg(feature = "test-hooks")]
pub fn debug_cancel_after(items: Option<usize>) {
    hook::AT.with(|c| c.set(items));
    hook::SEEN.with(|c| c.set(0));
    hook::AFTER.with(|c| c.set(0));
}

/// Test hook: the events processed after [`debug_cancel_after`]'s flag.
#[cfg(feature = "test-hooks")]
pub fn debug_items_after_cancel() -> usize {
    hook::AFTER.with(std::cell::Cell::get)
}

fn sequence(doc: &Document, seq: SeqId) -> Result<&Sequence, IoError> {
    doc.sequence(seq)
        .map_err(|_| IoError::Refused(NO_SEQ.to_owned()))
}

// ---------------------------------------------------------------- the layout

/// One `Track` of the export: a track, or one half of a split track.
#[derive(Debug)]
pub(super) struct Part<'a> {
    /// The track.
    pub(super) lane: &'a Lane,
    /// Its audio half (else its notes).
    pub(super) audio: bool,
    /// Whether the track was split in two.
    pub(super) split: bool,
    /// Its channel, in [`Layout::chans`].
    pub(super) chan: usize,
    /// The colour of the strip it plays through.
    pub(super) colour: Option<Swatch>,
}

impl Part<'_> {
    /// `t<lane>`, or `t<lane>a` for a split track's audio.
    pub(super) fn id(&self) -> String {
        if self.audio && self.split {
            format!("t{}a", self.lane.id.0)
        } else {
            format!("t{}", self.lane.id.0)
        }
    }

    /// The track's name, or "<name> audio" for a split track's audio.
    pub(super) fn name(&self) -> String {
        if self.audio && self.split {
            format!("{} audio", self.lane.name)
        } else {
            self.lane.name.clone()
        }
    }
}

/// Where a channel's `Track` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Home {
    /// In the track of [`Layout::parts`]' entry.
    Part(usize),
    /// In a bus track `b<strip>` of its own.
    Bus,
    /// In the master track.
    Master,
}

/// One `Channel` of the export.
#[derive(Debug)]
pub(super) struct Chan<'a> {
    /// Its ID.
    pub(super) id: String,
    /// Its name.
    pub(super) name: String,
    /// The strip it is (or a copy of); none for a plain channel.
    pub(super) strip: Option<&'a Strip>,
    /// Whether it is one of several copies of an instrument strip.
    pub(super) copy: bool,
    /// The track a plain channel belongs to.
    pub(super) lane: Option<LaneId>,
    /// The plug-in instrument it plays.
    pub(super) instrument: Option<&'a PluginSlot>,
    /// Its role.
    pub(super) role: DpMixerRole,
    /// The ID of the channel it plays into (none for the master).
    pub(super) to: Option<String>,
    /// Where its track is.
    pub(super) home: Home,
}

impl<'a> Chan<'a> {
    /// Its devices in processing order: the instrument, the EQ slot, then
    /// the inserts.
    pub(super) fn slots(&self) -> Vec<(&'a PluginSlot, DpDeviceRole, TagKind)> {
        let mut out = Vec::new();
        out.extend(
            self.instrument
                .map(|s| (s, DpDeviceRole::Instrument, TagKind::Device)),
        );
        if let Some(strip) = self.strip {
            out.extend(
                strip
                    .eq
                    .iter()
                    .map(|s| (s, DpDeviceRole::AudioFx, TagKind::Eq)),
            );
            out.extend(
                strip
                    .inserts
                    .iter()
                    .map(|s| (s, DpDeviceRole::AudioFx, TagKind::Device)),
            );
        }
        out
    }
}

/// What is exported and where it goes: the tracks, the channels and what
/// each track plays.
#[derive(Debug)]
pub(super) struct Layout<'a> {
    pub(super) doc: &'a Document,
    pub(super) seq: &'a Sequence,
    /// The tick at file time 0.
    pub(super) origin: i64,
    /// What each track plays (with top-level regions and reference uses
    /// unmuted).
    pub(super) played: BTreeMap<LaneId, Vec<ResolvedEvent>>,
    /// The top-level regions and reference uses that are muted in the
    /// project.
    pub(super) muted: BTreeSet<EventId>,
    /// The tracks, in order.
    pub(super) parts: Vec<Part<'a>>,
    /// The channels, in the structure's order: the tracks', then the
    /// buses' and submixes' in mixer order, then the master's.
    pub(super) chans: Vec<Chan<'a>>,
}

impl Layout<'_> {
    /// The channel that stands for `strip` on track `part`: the track's own
    /// when it is that strip (or a copy of it), else the strip's channel
    /// (the first copy, for a shared instrument).
    pub(super) fn chan_of(&self, part: usize, strip: StripId) -> Option<usize> {
        let is = |c: &Chan<'_>| c.strip.is_some_and(|s| s.id == strip);
        if let Some(own) = self.parts.get(part).map(|p| p.chan)
            && self.chans.get(own).is_some_and(is)
        {
            return Some(own);
        }
        self.chans
            .iter()
            .position(|c| is(c) && !c.copy)
            .or_else(|| self.chans.iter().position(is))
    }

    /// The strip plug-in instance `slot` processes on.
    pub(super) fn strip_of_slot(&self, slot: PluginInstanceId) -> Option<StripId> {
        let m = self.doc.mixer();
        match self.doc.find_slot(slot)? {
            SlotOwner::Instrument(d) => m.find_kind(StripKind::Instrument(d)),
            SlotOwner::Insert(s, _) | SlotOwner::Eq(s) => Some(s),
        }
    }

    /// The channel and device position of an automation target as track
    /// `part` writes it; `None` for what the file has no parameter for.
    pub(super) fn target(
        &self,
        part: usize,
        t: AutomationTarget,
    ) -> Option<(usize, Option<usize>)> {
        match t {
            AutomationTarget::StripFader(s)
            | AutomationTarget::StripPan(s)
            | AutomationTarget::StripMute(s)
            | AutomationTarget::SendLevel(s, _) => self.chan_of(part, s).map(|c| (c, None)),
            AutomationTarget::PluginParam(slot, p) => {
                if u32::try_from(p.0).is_err() {
                    return None;
                }
                let c = self.chan_of(part, self.strip_of_slot(slot)?)?;
                let d = self
                    .chans
                    .get(c)?
                    .slots()
                    .iter()
                    .position(|(x, _, _)| x.id == slot)?;
                Some((c, Some(d)))
            }
            AutomationTarget::StripSolo(_) | AutomationTarget::VelocityScale(_) => None,
        }
    }
}

/// A copy of `doc` whose top-level regions and reference uses in `s` play
/// unmuted, and the ones that were muted.
fn unmute_top_level(
    doc: &Document,
    s: &Sequence,
) -> Result<(Document, BTreeSet<EventId>), IoError> {
    let muted: BTreeSet<EventId> = s
        .lanes
        .iter()
        .filter_map(Lane::active)
        .flat_map(|t| t.events.values())
        .filter(|e| {
            e.mute
                && matches!(
                    e.body,
                    EventBody::AudioRegion { .. } | EventBody::Reference { .. }
                )
        })
        .map(|e| e.id)
        .collect();
    if muted.is_empty() {
        return Ok((doc.clone(), muted));
    }
    let cmd = Command::Event(EventCmd::SetMute {
        events: muted.iter().copied().collect(),
        mute: false,
    });
    let copy = apply(doc, &cmd).map_err(unwritable)?.doc;
    Ok((copy, muted))
}

/// How a track's half reaches the mixer.
#[derive(Debug, Clone, Copy)]
enum Feed {
    /// Through this strip.
    Strip(StripId),
    /// An external MIDI destination: a plain channel, with a Note.
    External,
    /// The built-in synth through this strip, with a Note.
    Builtin(StripId),
    /// Nothing: a plain channel.
    Nothing,
}

impl Feed {
    fn strip(self) -> Option<StripId> {
        match self {
            Feed::Strip(s) | Feed::Builtin(s) => Some(s),
            Feed::External | Feed::Nothing => None,
        }
    }
}

/// The feed of a track's notes played to destination `dest`.
fn note_feed(doc: &Document, dest: Option<DestId>) -> Feed {
    let m = doc.mixer();
    let Some(d) = dest.and_then(|d| doc.instruments().get(&d)) else {
        return Feed::Nothing;
    };
    match &d.kind {
        DestKind::PluginInstrument { .. } => m
            .find_kind(StripKind::Instrument(d.id))
            .map_or(Feed::Nothing, Feed::Strip),
        k if k.is_builtin_synth() => m
            .find_kind(StripKind::BuiltinSynth)
            .map_or(Feed::Nothing, Feed::Builtin),
        _ => Feed::External,
    }
}

/// The ID of the channel a strip's output plays into.
fn output_id(doc: &Document, o: Output) -> String {
    match o {
        Output::Master => format!("c{}", doc.mixer().master().0),
        Output::Bus(b) => format!("c{}", b.0),
    }
}

/// The layout of sequence `seq`, with the report lines of the structure.
pub(super) fn layout<'a>(
    doc: &'a Document,
    seq: SeqId,
    report: &mut Report,
    cancel: &AtomicBool,
) -> Result<Layout<'a>, IoError> {
    let s = sequence(doc, seq)?;
    stop(cancel)?;
    let origin = file_start(s).0;
    let (copy, muted) = unmute_top_level(doc, s)?;
    let mut scratch = Report::default();
    let played = midi_out::source(&copy, seq, &[], &mut scratch)?.played;
    drop(copy);
    stop(cancel)?;
    // From `source`'s report the track mute and solo Notes (L11), and the
    // muted notes and events outside the sequence it counts; the rest is
    // worded for MIDI files.
    for i in scratch.items.iter().filter(|i| {
        i.kind == ItemKind::LeftOut && [MUTED_NOTES, EVENTS_OUTSIDE].contains(&i.what.as_str())
    }) {
        add_n(report, i.kind, &i.what, i.count);
        if let Some(item) = report.items.iter_mut().find(|x| x.what == i.what) {
            item.details.extend(i.details.iter().cloned());
            item.details.truncate(crate::limits::MAX_DETAILS);
        }
    }
    for lane in &s.lanes {
        for what in [
            midi_out::muted_track(&lane.name),
            midi_out::soloed_out(&lane.name),
        ] {
            if scratch
                .items
                .iter()
                .any(|i| i.kind == ItemKind::Note && i.what == what)
            {
                report.add(ItemKind::Note, &what, None);
            }
        }
    }

    // Each track's halves and how they reach the mixer.
    let mut ticker = Ticker::new(cancel);
    let mut halves: Vec<(&Lane, bool, bool, Feed)> = Vec::new();
    for lane in &s.lanes {
        let events = played.get(&lane.id).map_or(&[][..], Vec::as_slice);
        let (mut midi, mut audio, mut dest) = (false, false, None);
        for r in events {
            ticker.tick()?;
            if r.body.is_midi() {
                midi = true;
                dest = dest.or(r.dest);
            } else if matches!(r.body, EventBody::AudioRegion { .. }) {
                audio = true;
            }
        }
        if !midi && !audio {
            if lane.midi_dest.is_some() || lane.audio_out.is_none() {
                midi = true;
            } else {
                audio = true;
            }
        }
        let split = midi && audio;
        if split {
            report.add(ItemKind::Changed, SPLIT_TRACKS, Some(lane.name.clone()));
        }
        if midi {
            let feed = note_feed(doc, lane.midi_dest.or(dest));
            match feed {
                Feed::External => report.add(ItemKind::Note, &external_midi(&lane.name), None),
                Feed::Builtin(_) => report.add(ItemKind::Note, &builtin_synth(&lane.name), None),
                Feed::Strip(_) | Feed::Nothing => {}
            }
            halves.push((lane, false, split, feed));
        }
        if audio {
            let feed = lane
                .audio_out
                .and_then(|o| doc.mixer().find_kind(StripKind::AudioPath(o)))
                .map_or(Feed::Nothing, Feed::Strip);
            halves.push((lane, true, split, feed));
        }
    }

    let m = doc.mixer();
    let mut users: BTreeMap<StripId, usize> = BTreeMap::new();
    for (_, _, _, f) in &halves {
        if let Some(st) = f.strip() {
            *users.entry(st).or_default() += 1;
        }
    }
    let shared = |st: StripId| users.get(&st).is_some_and(|n| *n > 1);
    let master_id = format!("c{}", m.master().0);

    let mut parts = Vec::new();
    let mut chans = Vec::new();
    for (i, (lane, audio, split, feed)) in halves.into_iter().enumerate() {
        let strip = feed.strip().and_then(|st| m.strip(st).ok());
        let plain = |to: String| Chan {
            id: if audio && split {
                format!("k{}a", lane.id.0)
            } else {
                format!("k{}", lane.id.0)
            },
            name: if audio && split {
                format!("{} audio", lane.name)
            } else {
                lane.name.clone()
            },
            strip: None,
            copy: false,
            lane: Some(lane.id),
            instrument: None,
            role: DpMixerRole::Regular,
            to: Some(to),
            home: Home::Part(i),
        };
        let chan = match strip {
            None => plain(master_id.clone()),
            Some(st) if audio && shared(st.id) => plain(format!("c{}", st.id.0)),
            Some(st) => {
                let copy = shared(st.id);
                let instrument = match (st.kind, audio) {
                    (StripKind::Instrument(d), false) => {
                        match doc.instruments().get(&d).map(|x| &x.kind) {
                            Some(DestKind::PluginInstrument { slot }) => Some(slot),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                Chan {
                    id: if copy {
                        format!("c{}t{}", st.id.0, lane.id.0)
                    } else {
                        format!("c{}", st.id.0)
                    },
                    name: st.name.clone(),
                    strip: Some(st),
                    copy,
                    lane: None,
                    instrument,
                    role: DpMixerRole::Regular,
                    to: Some(output_id(doc, st.output)),
                    home: Home::Part(i),
                }
            }
        };
        parts.push(Part {
            lane,
            audio,
            split,
            chan: chans.len(),
            colour: strip.and_then(|st| st.colour),
        });
        chans.push(chan);
    }
    for (st, n) in &users {
        let Ok(strip) = m.strip(*st) else { continue };
        if *n > 1 {
            if strip.kind.is_instrument() {
                report.add(
                    ItemKind::Changed,
                    &shared_instrument(&strip.name),
                    Some(format!("{n} tracks")),
                );
            } else {
                report.add(
                    ItemKind::Changed,
                    SHARED_AUDIO_PATHS,
                    Some(strip.name.clone()),
                );
            }
        }
    }

    // Buses and the submixes of shared audio paths, in mixer order.
    let outputs_to: BTreeSet<StripId> = m
        .strips()
        .values()
        .filter_map(|x| match x.output {
            Output::Bus(b) => Some(b),
            Output::Master => None,
        })
        .collect();
    for id in m.order().iter() {
        let Ok(st) = m.strip(*id) else { continue };
        let role = match st.kind {
            StripKind::Bus if outputs_to.contains(id) => DpMixerRole::Submix,
            StripKind::Bus => DpMixerRole::Effect,
            StripKind::AudioPath(_) if shared(*id) => DpMixerRole::Submix,
            StripKind::InstrumentOutput(..) => {
                report.add(ItemKind::LeftOut, EXTRA_OUTPUTS, Some(st.name.clone()));
                continue;
            }
            _ => {
                if !users.contains_key(id) {
                    report.add(ItemKind::LeftOut, UNPLAYED_STRIPS, Some(st.name.clone()));
                }
                continue;
            }
        };
        chans.push(Chan {
            id: format!("c{}", id.0),
            name: st.name.clone(),
            strip: Some(st),
            copy: false,
            lane: None,
            instrument: None,
            role,
            to: Some(output_id(doc, st.output)),
            home: Home::Bus,
        });
    }
    if let Ok(st) = m.strip(m.master()) {
        chans.push(Chan {
            id: master_id,
            name: st.name.clone(),
            strip: Some(st),
            copy: false,
            lane: None,
            instrument: None,
            role: DpMixerRole::Master,
            to: None,
            home: Home::Master,
        });
    }
    Ok(Layout {
        doc,
        seq: s,
        origin,
        played,
        muted,
        parts,
        chans,
    })
}

// ---------------------------------------------------------------- the model

/// What one export run builds before it is written.
struct Built<'a> {
    layout: Layout<'a>,
    project: DpProject,
    states: Vec<(String, Vec<u8>)>,
    audio: Vec<AudioFile>,
    ids: BTreeMap<String, ObjectRef>,
    report: Report,
}

/// What writing the structure collects.
struct Ctx<'a, 'p> {
    doc: &'a Document,
    plist: &'p dyn PlistCodec,
    report: Report,
    states: Vec<(String, Vec<u8>)>,
    ids: BTreeMap<String, ObjectRef>,
    automated: BTreeSet<(PluginInstanceId, ParamId)>,
    /// Slots and strips already listed, so copies list them once.
    listed_slots: BTreeSet<PluginInstanceId>,
    listed_sidechains: BTreeSet<PluginInstanceId>,
    listed_params: BTreeSet<(PluginInstanceId, ParamId)>,
    listed_strips: BTreeSet<StripId>,
    plugins: u64,
}

fn obj(kind: &str, id: u64) -> ObjectRef {
    ObjectRef {
        kind: kind.to_owned(),
        id,
        slot: None,
        index: None,
    }
}

fn named(id: impl Into<String>) -> DpNamed {
    DpNamed {
        id: Some(id.into()),
        ..DpNamed::default()
    }
}

fn real(id: String, unit: DpUnit, value: f64, min: f64, max: f64) -> DpRealParam {
    DpRealParam {
        named: named(id),
        parameter_id: None,
        unit,
        value: Some(value),
        min: Some(min),
        max: Some(max),
    }
}

fn boolean(id: String, value: bool) -> DpBoolParam {
    DpBoolParam {
        named: named(id),
        parameter_id: None,
        value: Some(value),
    }
}

fn volume(id: String, db: f64) -> DpRealParam {
    real(
        id,
        DpUnit::Linear,
        linear_gain(db),
        0.0,
        linear_gain(GAIN_MAX_DB),
    )
}

fn non_empty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_owned())
}

impl Ctx<'_, '_> {
    /// The state entry of `slot`, written (and numbered) now; or the report
    /// line that says why there is none.
    fn state(&mut self, slot: &PluginSlot) -> Option<DpFileRef> {
        let name = &slot.plugin.name;
        let first = self.listed_slots.insert(slot.id);
        let Some(id) = slot.state else {
            if first {
                self.report
                    .add(ItemKind::Note, &no_saved_settings(name), None);
            }
            return None;
        };
        let data = match (self.doc.state_blob(id), slot.plugin.format) {
            (None, _) => Err("its saved settings are missing from the project".to_owned()),
            (Some(b), PluginFormat::Clap) => Ok(b.to_vec()),
            (Some(b), PluginFormat::Au) => self.plist.to_xml(b),
        };
        match data {
            Ok(bytes) => {
                let entry = state_entry(self.states.len().saturating_add(1), slot.plugin.format);
                self.states.push((entry.clone(), bytes));
                Some(DpFileRef {
                    path: entry,
                    external: false,
                })
            }
            Err(why) => {
                if first {
                    self.report
                        .add(ItemKind::LeftOut, &plugin_settings(name), Some(why));
                }
                None
            }
        }
    }

    fn device(
        &mut self,
        chan: &str,
        slot: &PluginSlot,
        role: DpDeviceRole,
        tag_kind: TagKind,
    ) -> DpDevice {
        let p = &slot.plugin;
        let id = format!("{chan}d{}", slot.id.0);
        if slot.sidechain.is_some() && self.listed_sidechains.insert(slot.id) {
            self.report
                .add(ItemKind::LeftOut, SIDECHAINS, Some(p.name.clone()));
        }
        let mut parameters = Vec::new();
        for param in slot_params(&self.automated, slot.id) {
            let Ok(bits) = u32::try_from(param.0) else {
                if self.listed_params.insert((slot.id, param)) {
                    self.report
                        .add(ItemKind::LeftOut, PARAMS_PAST_32_BITS, Some(p.name.clone()));
                }
                continue;
            };
            let pid = format!("{id}q{}", param.0);
            self.ids.insert(
                pid.clone(),
                ObjectRef {
                    slot: Some(slot.id.0),
                    ..obj("parameter", param.0)
                },
            );
            parameters.push(DpRealParam {
                named: named(pid),
                parameter_id: Some(i32::from_ne_bytes(bits.to_ne_bytes())),
                unit: DpUnit::Normalized,
                value: slot.params.get(&param).copied().filter(|v| v.is_finite()),
                min: Some(0.0),
                max: Some(1.0),
            });
        }
        let state = self.state(slot);
        self.ids.insert(id.clone(), obj("device", slot.id.0));
        self.ids
            .insert(format!("{id}e"), obj("device_enable", slot.id.0));
        self.plugins = self.plugins.saturating_add(1);
        DpDevice {
            kind: match p.format {
                PluginFormat::Au => DpDeviceKind::Au,
                PluginFormat::Clap => DpDeviceKind::Clap,
            },
            named: DpNamed {
                id: Some(id.clone()),
                name: non_empty(&p.name),
                color: None,
                comment: Some(tag(tag_kind, slot.id.0)),
            },
            device_id: Some(p.id.clone()),
            device_name: p.name.clone(),
            role,
            vendor: non_empty(&p.vendor),
            version: non_empty(&p.version),
            loaded: Some(true),
            parameters,
            enabled: Some(boolean(format!("{id}e"), !slot.bypass)),
            state,
            settings: Vec::new(),
            other_ids: Vec::new(),
        }
    }

    fn channel(&mut self, c: &Chan<'_>) -> DpChannel {
        let id = &c.id;
        let mut devices = Vec::new();
        for (slot, role, kind) in c.slots() {
            devices.push(self.device(id, slot, role, kind));
        }
        let (gain, pan, mute, solo) = c.strip.map_or((0.0, 0.0, false, false), |s| {
            (s.gain_db, s.pan, s.mute, s.solo)
        });
        let mut sends = Vec::new();
        // The channel and its mixer parameters (a plain channel's by its
        // track).
        let (owner, prefix) = match (c.strip, c.lane) {
            (Some(st), _) => (st.id.0, ""),
            (None, Some(l)) => (l.0, "channel_"),
            (None, None) => (0, "channel_"),
        };
        self.ids.insert(
            id.clone(),
            obj(
                if c.strip.is_some() {
                    "strip"
                } else {
                    "channel"
                },
                owner,
            ),
        );
        for (suffix, kind) in [("v", "volume"), ("p", "pan"), ("m", "mute")] {
            self.ids.insert(
                format!("{id}{suffix}"),
                obj(&format!("{prefix}{kind}"), owner),
            );
        }
        if let Some(st) = c.strip {
            if st.automation != AutomationMode::Read && self.listed_strips.insert(st.id) {
                self.report
                    .add(ItemKind::LeftOut, AUTOMATION_MODES, Some(st.name.clone()));
            }
            for (i, send) in st.sends.iter().enumerate() {
                let sid = format!("{id}s{i}");
                let index = u64::try_from(i).ok();
                for (suffix, kind) in [("", "send"), ("v", "send_volume"), ("e", "send_enable")] {
                    self.ids.insert(
                        format!("{sid}{suffix}"),
                        ObjectRef {
                            index,
                            ..obj(kind, st.id.0)
                        },
                    );
                }
                sends.push(DpSend {
                    named: named(sid.clone()),
                    destination: Some(format!("c{}", send.target.0)),
                    kind: Some(if send.pre_fader {
                        DpSendType::Pre
                    } else {
                        DpSendType::Post
                    }),
                    enable: Some(boolean(format!("{sid}e"), send.enabled)),
                    pan: None,
                    volume: volume(format!("{sid}v"), send.level_db),
                });
            }
        }
        DpChannel {
            named: DpNamed {
                id: Some(id.clone()),
                name: non_empty(&c.name),
                color: None,
                comment: c.strip.map(|s| tag(TagKind::Strip, s.id.0)),
            },
            role: Some(c.role),
            audio_channels: Some(2),
            destination: c.to.clone(),
            solo: Some(solo),
            devices,
            mute: Some(Box::new(boolean(format!("{id}m"), mute))),
            pan: Some(Box::new(real(
                format!("{id}p"),
                DpUnit::Normalized,
                pan_normalized(pan),
                0.0,
                1.0,
            ))),
            sends,
            volume: Some(Box::new(volume(format!("{id}v"), gain))),
        }
    }
}

/// The document-wide lines: VizDAW-only state the file cannot hold.
fn vizdaw_only(doc: &Document, s: &Sequence, report: &mut Report) {
    for l in &s.lanes {
        let name = || Some(l.name.clone());
        if l.takes
            .iter()
            .any(|t| t.id != l.active_take && !t.events.is_empty())
        {
            report.add(ItemKind::LeftOut, INACTIVE_TAKES, name());
        }
        if !l.input_fx.is_empty() {
            report.add(ItemKind::LeftOut, INPUT_FX, name());
        }
        if l.input.is_some() || l.arm || l.monitor != MonitorMode::Auto {
            report.add(ItemKind::LeftOut, INPUTS, name());
        }
        if l.mpe.is_some() {
            report.add(ItemKind::LeftOut, MPE_INPUT, name());
        }
        if l.drum.is_some() {
            report.add(ItemKind::LeftOut, DRUM_ROWS, name());
        }
    }
    // Sequences the export reaches through references in the active takes
    // (what plays).
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
            report.add(ItemKind::LeftOut, OTHER_SEQUENCES, Some(other.name.clone()));
        }
        if other.keyboard_slot.is_some() {
            report.add(ItemKind::LeftOut, KEYBOARD_SLOTS, Some(other.name.clone()));
        }
    }
    let learn = u64::try_from(doc.settings().learn.len()).unwrap_or(u64::MAX);
    add_n(report, ItemKind::LeftOut, MIDI_LEARN, learn);
    if s.meter
        .entries()
        .iter()
        .any(|e| e.key != core_model::meter::KeySig::default())
    {
        report.add(ItemKind::LeftOut, KEY_SIGNATURES, None);
    }
    if !s.comment.trim().is_empty() {
        report.add(ItemKind::LeftOut, SEQUENCE_COMMENTS, Some(s.name.clone()));
    }
    if s.start_offset.0 > 0 {
        report.add(ItemKind::Note, PICKUP, None);
    }
}

/// The transport, the markers and the tempo and meter automation.
fn maps(
    s: &Sequence,
    origin: i64,
    report: &mut Report,
) -> (DpRealParam, DpTimeSigParam, DpArrangement) {
    let points: Vec<_> = s.tempo.points().iter().copied().collect();
    let at_origin = points
        .iter()
        .take_while(|p| p.tick.0 <= origin)
        .last()
        .or(points.first());
    // A ramp in force at file time 0 starts at its exact tempo
    // (`start_bpm` gives a MIDI file's first step, an average).
    let bpm = if at_origin.is_some_and(|p| p.ramp) {
        s.tempo.bpm_at(Tick(origin))
    } else {
        start_bpm(s)
    };
    let tempo = DpRealParam {
        named: named("tempo"),
        parameter_id: None,
        unit: DpUnit::Bpm,
        value: Some(bpm),
        min: Some(5.0),
        max: Some(999.0),
    };
    let (first, bar1) = start_meters(s);
    let time_signature = DpTimeSigParam {
        named: named("timesig"),
        numerator: i32::from(first.num),
        denominator: i32::from(first.den),
    };
    let rel = |t: i64| {
        t.checked_sub(origin)
            .filter(|r| *r >= 0)
            .map(|r| beats(Tick(r)))
    };

    let mut markers = Vec::new();
    for (i, m) in s.markers.iter().enumerate() {
        match rel(m.tick.0) {
            Some(time) => markers.push(DpMarker {
                named: DpNamed {
                    id: None,
                    name: Some(m.name.clone()),
                    color: None,
                    comment: Some(tag(TagKind::Marker, u64::try_from(i).unwrap_or(u64::MAX))),
                },
                time,
            }),
            None => report.add(ItemKind::LeftOut, EARLY_MARKERS, Some(m.name.clone())),
        }
    }
    let tempo_automation = (points.len() > 1).then(|| DpPoints {
        head: DpHead::default(),
        unit: Some(DpUnit::Bpm),
        target: DpTarget {
            parameter: Some("tempo".to_owned()),
            ..DpTarget::default()
        },
        points: points
            .iter()
            .filter_map(|p| {
                let time = rel(p.tick.0)?;
                let how = if p.ramp {
                    DpInterpolation::Linear
                } else {
                    DpInterpolation::Hold
                };
                Some(DpPoint {
                    time,
                    value: DpPointValue::Real(p.bpm.as_f64(), Some(how)),
                })
            })
            .collect(),
    });
    // With a pickup the first point is after file time 0: the tempo there
    // (the first point's, which governs earlier ticks) is held from 0.
    let tempo_automation = tempo_automation.map(|mut t| {
        if t.points.first().is_some_and(|p| p.time > 0.0) {
            t.points.insert(
                0,
                DpPoint {
                    time: 0.0,
                    value: DpPointValue::Real(
                        s.tempo.bpm_at(Tick(origin)),
                        Some(DpInterpolation::Hold),
                    ),
                },
            );
        }
        t
    });
    let entries = s.meter.entries();
    let pickup = s.start_offset.0 > 0;
    let time_signature_automation = (entries.len() > 1 || pickup).then(|| {
        let sig =
            |m: core_model::meter::Meter| DpPointValue::TimeSig(i32::from(m.num), i32::from(m.den));
        let mut pts = vec![DpPoint {
            time: 0.0,
            value: sig(first),
        }];
        if let (Some(b1), Some(time)) = (bar1, rel(0)) {
            pts.push(DpPoint {
                time,
                value: sig(b1),
            });
        }
        for e in entries.iter() {
            if e.tick.0 <= origin || (bar1.is_some() && e.tick.0 == 0) {
                continue;
            }
            if let Some(time) = rel(e.tick.0) {
                pts.push(DpPoint {
                    time,
                    value: sig(e.meter),
                });
            }
        }
        DpPoints {
            head: DpHead::default(),
            unit: None,
            target: DpTarget {
                parameter: Some("timesig".to_owned()),
                ..DpTarget::default()
            },
            points: pts,
        }
    });
    let arrangement = DpArrangement {
        named: DpNamed::default(),
        lanes: None,
        markers_head: DpHead::default(),
        markers,
        tempo_automation,
        time_signature_automation,
    };
    (tempo, time_signature, arrangement)
}

/// `metadata.xml`'s model: the song info's non-empty fields, the comment
/// ending in the export line (Ruling 15).
fn metadata(doc: &Document, export_id: &str) -> DpMetaData {
    let mut m = DpMetaData::default();
    for (out, field) in m.fields.iter_mut().zip(doc.settings().info.fields()) {
        *out = field.as_deref().and_then(non_empty);
    }
    let line = format!("{EXPORT_LINE}{export_id}");
    let comment = m.fields.last_mut();
    if let Some(c) = comment {
        *c = Some(match c.take() {
            Some(text) => format!("{text}\n{line}"),
            None => line,
        });
    }
    m
}

/// The layout, the project model, the state entries and the audio entries
/// of `o`'s sequence.
fn build<'a>(
    doc: &'a Document,
    o: &ExportOptions,
    plist: &dyn PlistCodec,
    app_version: &str,
    cancel: &AtomicBool,
) -> Result<Built<'a>, IoError> {
    let mut report = Report::default();
    let layout = layout(doc, o.seq, &mut report, cancel)?;
    let s = layout.seq;
    vizdaw_only(doc, s, &mut report);
    let mut ctx = Ctx {
        doc,
        plist,
        report,
        states: Vec::new(),
        ids: BTreeMap::new(),
        automated: automated_targets(doc),
        listed_slots: BTreeSet::new(),
        listed_sidechains: BTreeSet::new(),
        listed_params: BTreeSet::new(),
        listed_strips: BTreeSet::new(),
        plugins: 0,
    };
    stop(cancel)?;

    let mut structure = Vec::new();
    for c in &layout.chans {
        let channel = Some(Box::new(ctx.channel(c)));
        let strip = c.strip;
        let colour = |s: Option<&Strip>| s.and_then(|s| s.colour).map(|w| swatch_hex(w).to_owned());
        let track = match c.home {
            Home::Part(i) => {
                let Some(p) = layout.parts.get(i) else {
                    continue;
                };
                let id = p.id();
                ctx.ids.insert(id.clone(), obj("track", p.lane.id.0));
                DpTrack {
                    named: DpNamed {
                        id: Some(id),
                        name: Some(p.name()),
                        color: p.colour.map(|w| swatch_hex(w).to_owned()),
                        comment: Some(tag(TagKind::Track, p.lane.id.0)),
                    },
                    content: vec![if p.audio {
                        DpContentType::Audio
                    } else {
                        DpContentType::Notes
                    }],
                    loaded: None,
                    channel,
                    children: Vec::new(),
                }
            }
            Home::Bus => {
                let Some(st) = strip else { continue };
                let id = format!("b{}", st.id.0);
                ctx.ids.insert(id.clone(), obj("strip", st.id.0));
                DpTrack {
                    named: DpNamed {
                        id: Some(id),
                        name: non_empty(&st.name),
                        color: colour(strip),
                        comment: None,
                    },
                    content: vec![DpContentType::Audio],
                    loaded: None,
                    channel,
                    children: Vec::new(),
                }
            }
            Home::Master => {
                if let Some(st) = strip {
                    ctx.ids.insert("master".to_owned(), obj("strip", st.id.0));
                }
                DpTrack {
                    named: DpNamed {
                        id: Some("master".to_owned()),
                        name: strip.and_then(|s| non_empty(&s.name)),
                        color: colour(strip),
                        comment: None,
                    },
                    content: vec![DpContentType::Audio, DpContentType::Notes],
                    loaded: None,
                    channel,
                    children: Vec::new(),
                }
            }
        };
        structure.push(DpStructureItem::Track(track));
    }
    stop(cancel)?;

    let (tempo, time_signature, mut arrangement) = maps(s, layout.origin, &mut ctx.report);
    ctx.ids.insert("tempo".to_owned(), obj("tempo", s.id.0));
    ctx.ids.insert("timesig".to_owned(), obj("meter", s.id.0));
    let timeline = arrange(
        &layout,
        o.clips,
        o.region_gain,
        &o.left_out,
        &mut ctx.report,
        &mut Ticker::new(cancel),
    )?;
    stop(cancel)?;
    arrangement.lanes = Some(DpLanes {
        head: DpHead {
            time_unit: Some(DpTimeUnit::Beats),
            ..DpHead::default()
        },
        items: timeline.lanes,
    });
    let markers = u64::try_from(arrangement.markers.len()).unwrap_or(u64::MAX);
    let mut report = ctx.report;
    report.summary.tracks = u64::try_from(layout.parts.len()).unwrap_or(u64::MAX);
    report.summary.notes = timeline.notes;
    report.summary.regions = timeline.regions;
    report.summary.automation_points = timeline.points;
    report.summary.audio_files = u64::try_from(timeline.audio.len()).unwrap_or(u64::MAX);
    report.summary.markers = markers;
    report.summary.plugins = ctx.plugins;
    report.summary.tempo_map = true;
    let project = DpProject {
        version: "1.0".to_owned(),
        application: Some(("VizDAW".to_owned(), app_version.to_owned())),
        tempo: Some(tempo),
        time_signature: Some(time_signature),
        structure,
        arrangement: Some(arrangement),
        scenes: Vec::new(),
        dropped_text: 0,
        model_bytes: 0,
    };
    Ok(Built {
        layout,
        project,
        states: ctx.states,
        audio: timeline.audio,
        ids: ctx.ids,
        report,
    })
}
