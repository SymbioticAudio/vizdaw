//! DAWproject import (plan 6d Task 6): an opened [`DpPackage`] mapped to a
//! plan ([`map`]), then the plan applied as commands to a new project
//! ([`apply`]).
//!
//! **Structure** (Rulings 6, 7, 15, spec §7.4), in the file's order, depth
//! first:
//!
//! - a note track (it has notes or an instrument) plays a destination: the
//!   track's first AU or CLAP instrument as a plug-in instrument of its own,
//!   else one shared new built-in-synth destination, "Built-in Synth (ch 1)";
//! - an audio track gets an output path of its own and its strip; a track
//!   with both gets both;
//! - a folder, and every effect or submix channel, becomes a bus; the
//!   folder's tracks output to it unless their channel names another
//!   destination. The master channel sets the master strip; VCAs are left
//!   out.
//! - a track's channel sets the strips it owns (its instrument's and its
//!   audio path's): fader, pan, mute, solo, colour, output, sends and
//!   effects. A track on the shared built-in synth keeps its mute and solo
//!   as the track's own; the synth's strip takes the first such track's
//!   other settings, and later tracks' different ones are listed.
//! - outputs and sends between buses that would make a loop are cut (the
//!   output goes to the master), found in one walk without recursion.
//!
//! **Maps:** the transport's tempo and time signature, then the tempo and
//! time signature automation; seconds go through the tempo map built so
//! far. Then markers, song info and the sequence's name.
//!
//! **The arrangement** (Task 7, `timeline`): clips, notes, controller
//! points, audio, crossfades and automation onto the tracks, shared clips
//! as sequences, the clip launcher's slots as unplaced sequences.
//!
//! Everything the plan cannot hold is listed in the report; each `what` is
//! a constant (or, when it names something, a function) here.

mod apply;
mod maps;
mod timeline;

pub(crate) use apply::{add_buses_with_mix, apply_tracks, arrange_tracks, bus_of_strip};
pub use apply::{apply, apply_tagged};
pub(crate) use maps::clean_name;
pub use timeline::*;

use super::package::{DpPackage, Roles, TrackRole, is_bus};
use super::plugins::PlistCodec;
use super::record::ObjectRef;
use super::schema::*;
#[cfg(doc)]
use super::units::copy_channel_id;
use super::units::{
    EXPORT_LINE, TagKind, copied_channel, db_of_linear, nearest_swatch, pan_of_normalized,
    strip_tags,
};
use crate::IoError;
use crate::report::{ItemKind, Report};
use core_model::command::{
    Command, LaneCmd, MixerCmd, Report as CmdReport, RoutingCmd, SequenceCmd, SettingsCmd,
};
use core_model::document::{DestKind, MAX_INFO_BYTES, SongInfo};
use core_model::error::ModelError;
use core_model::history::History;
use core_model::ids::{AnyId, DestId, EventId, LaneId, PluginInstanceId, SeqId, StripId};
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::mixer::{
    AuxSend, BUILTIN_SYNTH_REF, GAIN_FLOOR_DB, GAIN_MAX_DB, Output, PluginFormat, PluginRef,
    PluginSlot, StripKind, Swatch,
};
use core_model::rules::MAX_TICKS;
use core_model::selection::ViewFilter;
use core_model::sequence::Marker;
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------- the API

/// What became of one audio file of the package ([`DpPackage::audio`]).
#[derive(Debug, Clone, PartialEq)]
pub enum MediaSource {
    /// Converted into the project's media (several mono items for a file
    /// wider than stereo).
    Online(Vec<core_media::import::Imported>),
    /// It could not be read; why.
    Offline {
        /// The reason.
        why: String,
    },
}

/// What the mapping needs from outside the package.
#[derive(Clone, Copy)]
pub struct ImportEnv<'a> {
    /// Whether a plug-in (format, ID) is installed and enabled here.
    pub installed: &'a dyn Fn(PluginFormat, &str) -> bool,
    /// The AU property-list conversion.
    pub plist: &'a dyn PlistCodec,
    /// The new project's sample rate.
    pub rate: u32,
    /// The file's XML IDs and what each stood for, when they are trusted
    /// as identities (plan 6e Ruling 2: the record's head names the
    /// snapshot's export); `None` reads tags alone.
    pub ids: Option<&'a BTreeMap<String, ObjectRef>>,
}

/// What each new object was in the project the file came from, as the
/// file's trusted XML IDs, else its `vz:` tags, name it ([`apply_tagged`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Origins {
    /// Each new track: (its lane there, whether it is the audio part).
    pub lanes: BTreeMap<LaneId, (u64, bool)>,
    /// Each new strip: its strip there.
    pub strips: BTreeMap<StripId, u64>,
    /// Each new top-level event: its clip's `vz:e` (the event there).
    pub events: BTreeMap<EventId, u64>,
    /// Per new marker, in order: its `vz:m` index.
    pub markers: Vec<Option<u64>>,
    /// Each new plug-in slot: its `vz:d` / `vz:q` instance there.
    pub slots: BTreeMap<PluginInstanceId, u64>,
    /// Each new strip whose file channel named effects VizDAW cannot host
    /// here (VST plug-ins, or ones not installed): their names, with the
    /// plug-in ID of one not installed (plan 6e Ruling 10).
    pub unhosted: BTreeMap<StripId, Vec<(String, Option<String>)>>,
    /// Each new audio track's output-path strip: the file's channel name
    /// (the import names the path after the track; plan 6e Task 5).
    pub channel_names: BTreeMap<StripId, String>,
    /// Each new track: the file channel its events without a channel of
    /// their own were on, its main channel ([`MAIN_CHANNEL`], plan 6e
    /// Task 11).
    pub main_channels: BTreeMap<LaneId, u8>,
    /// The strips whose name the import gives, not the file: the built-in
    /// synth's shared strip, which the tracks on the built-in synth play
    /// through (theirs' own, or standing in for an external destination
    /// or an instrument it could not make; plan 6e Task 11).
    pub fallback_strips: BTreeSet<StripId>,
}

/// A mapped import: what [`apply`] will do, and the report so far (the
/// package's reading included).
#[derive(Debug, Clone, PartialEq)]
pub struct DpImport {
    /// What reading and mapping changed, left out or noted.
    pub report: Report,
    plan: Plan,
    trusted: bool,
}

impl DpImport {
    /// Whether the mapping trusted the file's XML IDs
    /// ([`ImportEnv::ids`]), so its [`Origins`] are identities rather than
    /// tags alone (plan 6e Task 4).
    pub fn ids_trusted(&self) -> bool {
        self.trusted
    }
}

/// The new plug-in slots with the state each is to capture, as [`apply`]
/// returns them.
pub type SlotStates = Vec<(PluginInstanceId, Arc<[u8]>)>;

/// How many items the mapping counts between looks at the cancel flag.
pub const CANCEL_EVERY: usize = 65_536;

// ---------------------------------------------------------------- the report

/// Changed: folder tracks (a count).
pub const FOLDERS: &str = "Folder tracks became buses";
/// Left out: VCA channels (a count).
pub const VCA: &str = "VCA channels";
/// Left out: send pan (a count).
pub const SEND_PAN: &str = "Send pan";
/// Changed: a channel that outputs to a track's channel (a count).
pub const OUTPUT_TO_TRACK: &str = "Outputs to tracks went to the master";
/// Left out: sends whose destination is not a bus.
pub const SEND_TARGETS: &str = "Sends to parts other than buses";
/// Left out: the master channel's sends.
pub const MASTER_SENDS: &str = "Sends on the master";
/// Changed: bus outputs that would have made a loop (by bus).
pub const LOOP_OUTPUTS: &str = "Outputs that would make a loop went to the master";
/// Left out: bus sends that would have made a loop (by bus).
pub const LOOP_SENDS: &str = "Sends that would make a loop";
/// Left out: colours the project has no place for (markers, tracks that
/// own no strip) and comment text, tags removed (tracks, channels, devices,
/// clips, markers); the place as the detail.
pub const COLOURS_AND_COMMENTS: &str = "Colours and comments on tracks, clips and markers";
/// Changed: a colour that is not one of the swatches (Ruling 15).
pub const COLOURS_NEAREST: &str = "Colours changed to the nearest VizDAW colour";
/// Changed: meter changes off a barline.
pub const METER_MOVED: &str = "Meter changes moved to the next barline";
/// Changed: tempos outside 5 to 999 BPM.
pub const TEMPO_CLAMPED: &str = "Tempos outside 5 to 999 BPM were clamped";
/// Left out: time signatures VizDAW cannot hold.
pub const BAD_METER: &str = "Unreadable time signatures";
/// Left out: tempo and meter points earlier than the one before them.
pub const MAP_ORDER: &str = "Tempo and meter changes out of order";
/// Left out: parts placed before 0 or past the end of the timeline.
pub const PAST_END: &str = "Parts placed past the end of the timeline";
/// Changed: song info fields over [`MAX_INFO_BYTES`] (by field).
pub const INFO_CUT: &str = "Song info cut to 4 096 bytes";
/// Changed: names over [`MAX_INFO_BYTES`].
pub const NAMES_CUT: &str = "Names cut to 4 096 bytes";
/// Changed: fader and send levels over +12 dB.
pub const LEVELS_CUT: &str = "Levels above +12 dB were lowered to +12 dB";
/// Left out: text between the file's elements (a count).
pub const TEXT_DROPPED: &str = "Text between the parts of the file";
/// Left out: settings of tracks that share the built-in synth, past the
/// first (by track).
pub const SHARED_SYNTH: &str = "Mixer settings of tracks that share the built-in synth";
/// Left out: effects and sends of tracks with neither notes nor audio (by
/// track).
pub const NO_STRIP: &str = "Mixer settings of tracks with no instrument or audio";
/// Note: tags or trusted IDs that named nothing, the wrong kind, or a part
/// already named (a count).
pub const TAGS_IGNORED: &str =
    "Tags in the file that named nothing or named one part twice (ignored)";
/// Note (Ruling 16): the file came from VizDAW.
pub const FROM_VIZDAW: &str =
    "This file came from a VizDAW project; it was opened as a new project";

/// Changed: a track with notes and audio has its settings and effects on
/// both its strips (by track).
pub const BOTH_STRIPS: &str =
    "Effects of tracks with notes and audio were put on both their strips";
/// Left out: the master's pan and solo (VizDAW's master has neither).
pub const MASTER_PAN_SOLO: &str = "Pan and solo on the master";

// Each kind below is one item whatever the count, the names as details
// (at most `MAX_DETAILS`), so the report stays small for any file.

/// Note: plug-ins that are not installed here (by name).
pub const NOT_INSTALLED: &str =
    "Plug-ins that are not installed; they keep their settings until they are";
/// Left out: VST plug-ins (by name).
pub const VST: &str = "VST plug-ins";
/// Left out: plug-ins the file gives no ID for (by name).
pub const NO_PLUGIN_ID: &str = "Plug-ins with no plug-in ID";
/// Note: note tracks with no instrument VizDAW can play (by track).
pub const PLAYS_BUILTIN: &str = "Tracks that play the built-in synth";
/// Left out: other DAWs' built-in effects; `<kind> on track '<name>'` and
/// each setting as details.
pub const BUILTIN_EFFECTS: &str = "Built-in effects on tracks";
/// Left out: other DAWs' built-in instruments, as [`BUILTIN_EFFECTS`].
pub const BUILTIN_INSTRUMENTS: &str = "Built-in instruments on tracks";
/// Left out: instruments after a track's first, or on a bus (`<name> on
/// '<owner>'`).
pub const EXTRA_INSTRUMENTS: &str = "Instruments after a track's first (a track plays one)";
/// Left out: note effects (by name).
pub const NOTE_EFFECTS: &str = "Note effects";
/// Left out: analyzers (by name).
pub const ANALYZERS: &str = "Analyzers";
/// Left out (Ruling 8): plug-in settings that could not be read or carried
/// (`<name>: <why>`).
pub const PLUGIN_SETTINGS: &str = "Settings of plug-ins";
/// The reason, in a [`PLUGIN_SETTINGS`] detail, for a state past the total
/// the import holds ([`PackageLimits::max_state_total`](crate::limits::PackageLimits)).
pub const TOO_LARGE_TO_CARRY: &str = "too large to carry";

/// The refusal of an import into a project that has tracks (Ruling 17).
pub const NOT_NEW: &str = "A DAWproject opens as a new project.";
/// The output path reference of a new audio track (as Track Synth makes).
const AUDIO_REFERENCE: &str = "audio:1-2";
const BUILTIN_NAME: &str = "Built-in Synth (ch 1)";

// ---------------------------------------------------------------- the plan

/// Where a strip's output goes in the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Out {
    #[default]
    Master,
    Bus(usize),
}

#[derive(Debug, Clone, PartialEq)]
struct SlotPlan {
    plugin: PluginRef,
    bypass: bool,
    state: Option<Arc<[u8]>>,
    /// The instance it was in the project the file came from.
    tag: Option<u64>,
}

impl SlotPlan {
    fn untagged(&self) -> SlotPlan {
        SlotPlan {
            tag: None,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SendPlan {
    bus: usize,
    level_db: f64,
    pre: bool,
    enabled: bool,
}

/// A strip's settings.
#[derive(Debug, Clone, PartialEq, Default)]
struct Mix {
    gain_db: f64,
    pan: f64,
    mute: bool,
    solo: bool,
    colour: Option<Swatch>,
    output: Out,
    sends: Vec<SendPlan>,
    eq: Option<SlotPlan>,
    inserts: Vec<SlotPlan>,
    /// The effects the file names that VizDAW cannot host here (VST
    /// plug-ins, or ones not installed): name, and plug-in ID for one not
    /// installed (plan 6e Ruling 10).
    unhosted: Vec<(String, Option<String>)>,
}

impl Mix {
    /// The same settings without mute and solo.
    fn unflagged(&self) -> Mix {
        Mix {
            mute: false,
            solo: false,
            ..self.clone()
        }
    }

    /// The same settings with no slot's origin, for comparing two
    /// tracks' settings.
    fn untagged(&self) -> Mix {
        Mix {
            eq: self.eq.as_ref().map(SlotPlan::untagged),
            inserts: self.inserts.iter().map(SlotPlan::untagged).collect(),
            unhosted: Vec::new(),
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Midi {
    Plugin(SlotPlan),
    Builtin,
}

#[derive(Debug, Clone, PartialEq)]
struct TrackPlan {
    name: String,
    /// The name of the instrument destination a track on a plug-in makes:
    /// the file's channel name, else its instrument device's name, else the
    /// track's name.
    dest_name: String,
    midi: Option<Midi>,
    audio: bool,
    /// The settings of the strips the track owns.
    mix: Mix,
    /// Mute and solo kept on the track rather than its strips (it shares
    /// the built-in synth's strip).
    flags: Option<(bool, bool)>,
    /// The lane (and whether the audio part) it was where the file came
    /// from.
    tag: Option<(u64, bool)>,
    /// The strip its first own strip was there.
    strip_tag: Option<u64>,
    /// Its channel's own name in the file, when it has one.
    channel_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct BusPlan {
    name: String,
    mix: Mix,
    /// The strip it was where the file came from.
    tag: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
struct Plan {
    name: String,
    info: SongInfo,
    tempo: TempoMap,
    meter: MeterMap,
    markers: Vec<Marker>,
    /// Each marker's `vz:m` index, parallel to `markers`.
    marker_tags: Vec<Option<u64>>,
    buses: Vec<BusPlan>,
    master: Mix,
    builtin: Option<Mix>,
    tracks: Vec<TrackPlan>,
    /// The tracks' clips, the new sequences and the media (Task 7).
    arranged: timeline::Arranged,
}

// ---------------------------------------------------------------- cancel

fn stop(cancel: &AtomicBool) -> Result<(), IoError> {
    if cancel.load(Ordering::Relaxed) {
        Err(IoError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(feature = "test-hooks")]
mod hook {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    pub(super) static FLAG: AtomicUsize = AtomicUsize::new(0);
    pub(super) static AT: AtomicUsize = AtomicUsize::new(0);
    pub(super) static SEEN: AtomicUsize = AtomicUsize::new(0);
    pub(super) static AFTER: AtomicUsize = AtomicUsize::new(0);
    pub(super) static PAUSED: AtomicBool = AtomicBool::new(false);

    pub(super) fn addr(cancel: &AtomicBool) -> usize {
        std::ptr::from_ref(cancel).addr()
    }

    /// Counts one item of a mapping whose flag is the hooked one; at the
    /// chosen item, waits (at most 10 s) for the flag, then counts what
    /// follows.
    pub(super) fn on_tick(cancel: &AtomicBool) {
        if FLAG.load(Ordering::SeqCst) != addr(cancel) {
            return;
        }
        let n = SEEN.fetch_add(1, Ordering::SeqCst).saturating_add(1);
        let at = AT.load(Ordering::SeqCst);
        if n == at {
            PAUSED.store(true, Ordering::SeqCst);
            let start = Instant::now();
            while !cancel.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(10) {
                std::thread::yield_now();
            }
        } else if n > at {
            AFTER.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// Test hook: the next [`map`] given `cancel` pauses at its `item`-th item
/// (from 1) until another thread sets `cancel` (at most 10 s), then counts
/// the items it processes after; `None` turns the hook off. Resets the
/// counts.
#[cfg(feature = "test-hooks")]
pub fn debug_pause_at(cancel: &AtomicBool, item: Option<usize>) {
    hook::FLAG.store(item.map_or(0, |_| hook::addr(cancel)), Ordering::SeqCst);
    hook::AT.store(item.unwrap_or(0), Ordering::SeqCst);
    hook::SEEN.store(0, Ordering::SeqCst);
    hook::AFTER.store(0, Ordering::SeqCst);
    hook::PAUSED.store(false, Ordering::SeqCst);
}

/// Test hook: whether the hooked mapping has reached its pause.
#[cfg(feature = "test-hooks")]
pub fn debug_paused() -> bool {
    hook::PAUSED.load(Ordering::SeqCst)
}

/// Test hook: the items the hooked mapping processed after its pause.
#[cfg(feature = "test-hooks")]
pub fn debug_items_after_pause() -> usize {
    hook::AFTER.load(Ordering::SeqCst)
}

/// Test hook: the items the hooked mapping has processed so far (set a
/// pause past the end to count a whole mapping).
#[cfg(feature = "test-hooks")]
pub fn debug_items_seen() -> usize {
    hook::SEEN.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------- mapping

/// What a channel ID names, for destinations and sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Master,
    Bus(usize),
    /// A note or audio track's channel, or a VCA.
    Other,
}

/// One entry of the structure, in walk order.
enum Node<'p> {
    Track {
        t: &'p DpTrack,
        role: TrackRole,
        /// The folder bus it is inside.
        parent: Option<usize>,
        /// Its own bus, for a folder or a bus.
        bus: Option<usize>,
        master: bool,
    },
    Channel {
        c: &'p DpChannel,
        bus: Option<usize>,
        master: bool,
    },
}

/// A state entry's bytes, or why there are none.
type StateRead = Result<Arc<[u8]>, String>;

struct Cx<'a, 'p> {
    pkg: &'p DpPackage,
    env: &'a ImportEnv<'a>,
    cancel: &'a AtomicBool,
    report: Report,
    n: usize,
    targets: HashMap<&'p str, Target>,
    /// Plug-in states read so far, by entry and format.
    states: HashMap<(String, PluginFormat), StateRead>,
    /// The bytes those states hold.
    held: u64,
    /// Each note or audio track's index in the plan, by track ID (`None`
    /// for the master, folders, buses and VCAs).
    track_index: HashMap<&'p str, Option<usize>>,
    /// Each note or audio track's index in the plan, by its channel's ID.
    chan_track: HashMap<&'p str, usize>,
    /// The slot each device became, by device ID.
    dev_slot: HashMap<&'p str, timeline::SlotAt>,
    /// Each send's channel and its index among the strip's sends, by send
    /// ID (`usize::MAX` once a loop cut it).
    send_at: HashMap<&'p str, (&'p str, usize)>,
    /// What the tags and trusted IDs have named so far.
    claims: Claims,
    /// The shared strip the track channel being mapped copies
    /// ([`Cx::copy_of`]), for its strip's and devices' claims.
    copy: Option<u64>,
}

/// What the file's tags and trusted IDs have named so far, per map, and how
/// many were ignored ([`TAGS_IGNORED`]).
#[derive(Debug, Default)]
struct Claims {
    /// Tracks, by (lane, audio part).
    lanes: Claim<(u64, bool)>,
    strips: Claim<u64>,
    slots: Claim<u64>,
    /// Each event named: the plan track of its clips, the pass of the
    /// last of them (its [`TagKind::Pass`]; `None` for a clip of no pass)
    /// and the number of the `Clips` container of the first claim when it
    /// is on a pass chain.
    events: HashMap<u64, (usize, Option<u64>, Option<usize>)>,
    markers: HashSet<u64>,
    ignored: usize,
}

/// One map's claims. Trusted IDs reserve their objects before any tag
/// claims one ([`Cx::reserve`]): by the element's ID, with the shared strip
/// its channel copies. Then each element claims in file order; the first
/// keeps the object (with the shared strip its channel copies).
#[derive(Debug)]
struct Claim<K> {
    reserved: HashMap<K, (String, Option<u64>)>,
    taken: HashMap<K, Option<u64>>,
}

impl<K> Default for Claim<K> {
    fn default() -> Self {
        Claim {
            reserved: HashMap::new(),
            taken: HashMap::new(),
        }
    }
}

impl<K: std::hash::Hash + Eq> Claim<K> {
    /// Reserves `key` for the element `id` (the first to ask keeps it).
    fn reserve(&mut self, key: K, id: &str, copy: Option<u64>) {
        self.reserved.entry(key).or_insert((id.to_owned(), copy));
    }

    /// Whether the element `id`, naming `key` `by` from a channel copying
    /// shared strip `copy`, keeps it. A reserved object goes only to its
    /// trusted ID (or a copy of the same shared strip, also by ID); an
    /// object already claimed only to a copy of the same shared strip, as
    /// the export writes one per track ([`copy_channel_id`]). Any other
    /// claim is counted in `ignored`.
    fn take(
        &mut self,
        key: K,
        by: &Named,
        id: Option<&str>,
        copy: Option<u64>,
        ignored: &mut usize,
    ) -> bool {
        let copies = |c: &Option<u64>| copy.is_some() && *c == copy;
        if let Some((rid, rcopy)) = self.reserved.get(&key) {
            let mine = matches!(by, Named::Id(..)) && (id == Some(rid.as_str()) || copies(rcopy));
            if !mine {
                *ignored = ignored.saturating_add(1);
                return false;
            }
        }
        match self.taken.get(&key) {
            None => {
                self.taken.insert(key, copy);
                true
            }
            Some(c) if copies(c) => true,
            Some(_) => {
                *ignored = ignored.saturating_add(1);
                false
            }
        }
    }
}

/// What an element names: by its trusted XML ID (the kind the map gives)
/// or by a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Named {
    Id(String, u64),
    Tag(TagKind, u64),
}

impl Named {
    fn number(&self) -> u64 {
        match self {
            Named::Id(_, n) | Named::Tag(_, n) => *n,
        }
    }
}

/// Whether `key` is the first to claim its object in `set`; a later claim
/// is counted in `ignored`.
fn first<K: std::hash::Hash + Eq>(set: &mut HashSet<K>, ignored: &mut usize, key: K) -> bool {
    if set.insert(key) {
        true
    } else {
        *ignored = ignored.saturating_add(1);
        false
    }
}

/// Maps `pkg` to a plan for [`apply`]. Looks at `cancel` before any work and
/// every [`CANCEL_EVERY`] items. The report starts with the package's.
/// `media` (what became of each [`DpPackage::audio`] file) is for the
/// timeline mapping.
pub fn map(
    pkg: &DpPackage,
    media: &BTreeMap<usize, MediaSource>,
    env: &ImportEnv,
    cancel: &AtomicBool,
) -> Result<DpImport, IoError> {
    stop(cancel)?;
    let mut cx = Cx {
        pkg,
        env,
        cancel,
        report: pkg.report.clone(),
        n: 0,
        targets: HashMap::new(),
        states: HashMap::new(),
        held: 0,
        track_index: HashMap::new(),
        chan_track: HashMap::new(),
        dev_slot: HashMap::new(),
        send_at: HashMap::new(),
        claims: Claims::default(),
        copy: None,
    };
    let p = &pkg.project;
    if p.dropped_text > 0 {
        cx.add_n(ItemKind::LeftOut, TEXT_DROPPED, p.dropped_text);
    }
    if pkg.metadata.dropped_text > 0 {
        cx.add_n(ItemKind::LeftOut, TEXT_DROPPED, pkg.metadata.dropped_text);
    }
    cx.reserve()?;
    let (tempo, meter) = cx.maps()?;
    let markers = cx.markers(&tempo)?;
    cx.timelines()?;
    let nodes = cx.nodes()?;
    let mut plan = Plan {
        name: String::new(),
        info: SongInfo::default(),
        tempo,
        meter,
        markers: markers.iter().map(|(m, _)| m.clone()).collect(),
        marker_tags: markers.iter().map(|(_, t)| *t).collect(),
        buses: Vec::new(),
        master: Mix::default(),
        builtin: None,
        tracks: Vec::new(),
        arranged: timeline::Arranged::default(),
    };
    cx.structure(&nodes, &mut plan)?;
    let cut = cut_loops(&mut plan, &mut cx.report);
    cx.sends_cut(&cut);
    plan.arranged = cx.arrangement(&mut plan, media)?;
    let (name, info) = cx.song_info();
    plan.name = name;
    plan.info = info;
    if pkg.ret.from_vizdaw() {
        cx.report.add(ItemKind::Note, FROM_VIZDAW, None);
    }
    if cx.claims.ignored > 0 {
        let n = cx.claims.ignored;
        cx.add_n(ItemKind::Note, TAGS_IGNORED, n);
    }
    stop(cancel)?;
    Ok(DpImport {
        report: cx.report,
        plan,
        trusted: env.ids.is_some(),
    })
}

impl<'a, 'p> Cx<'a, 'p> {
    /// One more item.
    fn tick(&mut self) -> Result<(), IoError> {
        #[cfg(feature = "test-hooks")]
        hook::on_tick(self.cancel);
        self.n = self.n.wrapping_add(1);
        if self.n % CANCEL_EVERY == 0 {
            stop(self.cancel)
        } else {
            Ok(())
        }
    }

    fn add(&mut self, kind: ItemKind, what: &str, detail: Option<String>) {
        self.report.add(kind, what, detail);
    }

    /// `what` counted `n` times.
    fn add_n(&mut self, kind: ItemKind, what: &str, n: usize) {
        self.report.add(kind, what, None);
        let extra = u64::try_from(n.saturating_sub(1)).unwrap_or(u64::MAX);
        if let Some(i) = self
            .report
            .items
            .iter_mut()
            .find(|i| i.kind == kind && i.what == what)
        {
            i.count = i.count.saturating_add(extra);
        }
    }

    /// `what` once, with `details` (up to the report's cap).
    fn add_with(&mut self, kind: ItemKind, what: &str, details: &[String]) {
        self.report.add(kind, what, details.first().cloned());
        if let Some(i) = self
            .report
            .items
            .iter_mut()
            .find(|i| i.kind == kind && i.what == what)
        {
            for d in details.iter().skip(1) {
                if i.details.len() >= crate::limits::MAX_DETAILS {
                    break;
                }
                i.details.push(d.clone());
            }
        }
    }

    /// A name fit for VizDAW: control and zero-width characters dropped,
    /// trimmed, cut to [`MAX_INFO_BYTES`] (Changed); `fallback` when empty.
    fn name(&mut self, raw: Option<&str>, fallback: impl FnOnce() -> String) -> String {
        let (name, cut) = clean_name(raw.unwrap_or(""));
        if cut {
            self.add(ItemKind::Changed, NAMES_CUT, None);
        }
        if name.is_empty() { fallback() } else { name }
    }

    /// The comment's text without tags; listed when any is left.
    fn comment(&mut self, comment: Option<&str>, place: impl FnOnce() -> String) {
        let Some(c) = comment else { return };
        if !strip_tags(c).0.is_empty() {
            self.add(ItemKind::LeftOut, COLOURS_AND_COMMENTS, Some(place()));
        }
    }

    /// What an element names. Its trusted XML ID wins when the ID map
    /// holds it with one of `kinds` (another kind is ignored and counted);
    /// else its comment's first tag of one of `tags`. Every other tag in the
    /// comment is ignored and counted. The caller counts a second claim on
    /// one object.
    fn named(
        &mut self,
        id: Option<&str>,
        kinds: &[&str],
        comment: Option<&str>,
        tags: &[TagKind],
    ) -> Option<Named> {
        self.named_beside(id, kinds, comment, tags, &[])
    }

    /// [`Cx::named`], with tags of the kinds in `beside` (read by the
    /// caller) neither chosen nor counted.
    fn named_beside(
        &mut self,
        id: Option<&str>,
        kinds: &[&str],
        comment: Option<&str>,
        tags: &[TagKind],
        beside: &[TagKind],
    ) -> Option<Named> {
        let mut by_tag = None;
        if let Some(c) = comment {
            for (k, n) in strip_tags(c).1 {
                if beside.contains(&k) {
                    continue;
                }
                if by_tag.is_none() && tags.contains(&k) {
                    by_tag = Some(Named::Tag(k, n));
                } else {
                    self.claims.ignored = self.claims.ignored.saturating_add(1);
                }
            }
        }
        let by_id = match (self.env.ids, id) {
            (Some(ids), Some(id)) => match ids.get(id) {
                Some(r) if kinds.contains(&r.kind.as_str()) => {
                    Some(Named::Id(r.kind.clone(), r.id))
                }
                Some(_) => {
                    self.claims.ignored = self.claims.ignored.saturating_add(1);
                    None
                }
                None => None,
            },
            _ => None,
        };
        by_id.or(by_tag)
    }

    /// The strip a channel names (a plain channel's ID names none), when it
    /// keeps it; `copy` is the shared strip it copies ([`Cx::copy_of`]).
    fn strip_named(&mut self, c: &DpChannel, copy: Option<u64>) -> Option<u64> {
        let id = c.named.id.as_deref();
        let named = self.named(
            id,
            &["strip", "channel"],
            c.named.comment.as_deref(),
            &[TagKind::Strip],
        )?;
        if matches!(&named, Named::Id(k, _) if k == "channel") {
            return None;
        }
        let n = named.number();
        let copy = copy.filter(|s| *s == n);
        let claims = &mut self.claims;
        claims
            .strips
            .take(n, &named, id, copy, &mut claims.ignored)
            .then_some(n)
    }

    /// The strip a channel names, without claiming or counting anything:
    /// its trusted ID's, else its first strip tag.
    fn peek_strip(&self, c: &DpChannel) -> Option<u64> {
        let by_id = self
            .env
            .ids
            .zip(c.named.id.as_deref())
            .and_then(|(ids, id)| ids.get(id))
            .filter(|r| r.kind == "strip")
            .map(|r| r.id);
        by_id.or_else(|| {
            let (_, tags) = strip_tags(c.named.comment.as_deref()?);
            tags.into_iter()
                .find(|(k, _)| *k == TagKind::Strip)
                .map(|(_, n)| n)
        })
    }

    /// The shared strip track `lane`'s channel `c` is a copy of: its ID is
    /// [`copy_channel_id`]`(s, lane)` and it names strip `s`.
    fn copy_of(&self, c: &DpChannel, lane: Option<u64>) -> Option<u64> {
        let (s, l) = copied_channel(c.named.id.as_deref()?)?;
        (Some(l) == lane && self.peek_strip(c) == Some(s)).then_some(s)
    }

    /// Pass one (plan 6e Task 2, I1): every track, strip and slot a trusted
    /// ID names is reserved for that element before any tag claims.
    fn reserve(&mut self) -> Result<(), IoError> {
        let Some(ids) = self.env.ids else {
            return Ok(());
        };
        let p = &self.pkg.project;
        let mut tracks: Vec<&'p DpTrack> = Vec::new();
        let mut chans: Vec<&'p DpChannel> = Vec::new();
        for item in &p.structure {
            match item {
                DpStructureItem::Track(t) => tracks.push(t),
                DpStructureItem::Channel(c) => chans.push(c),
            }
        }
        // Depth first, in file order.
        tracks.reverse();
        let mut order: Vec<&'p DpTrack> = Vec::new();
        while let Some(t) = tracks.pop() {
            self.tick()?;
            order.push(t);
            tracks.extend(t.children.iter().rev());
        }
        let kind = |id: Option<&str>, k: &str| {
            id.and_then(|id| ids.get(id))
                .filter(|r| r.kind == k)
                .map(|r| r.id)
        };
        let all = order
            .iter()
            .map(|t| (Some(*t), t.channel.as_deref()))
            .chain(chans.iter().map(|c| (None, Some(*c))));
        for (t, c) in all {
            self.tick()?;
            if let Some(t) = t
                && let (Some(id), Some(n)) =
                    (t.named.id.as_deref(), kind(t.named.id.as_deref(), "track"))
            {
                let audio = t.content.contains(&DpContentType::Audio)
                    && !t.content.contains(&DpContentType::Notes);
                self.claims.lanes.reserve((n, audio), id, None);
            }
            let Some(c) = c else { continue };
            let cid = c.named.id.as_deref();
            let copy = cid.and_then(copied_channel).map(|(s, _)| s);
            if let (Some(id), Some(n)) = (cid, kind(cid, "strip")) {
                self.claims.strips.reserve(n, id, copy.filter(|s| *s == n));
            }
            for d in &c.devices {
                let did = d.named.id.as_deref();
                if let (Some(id), Some(n)) = (did, kind(did, "device")) {
                    self.claims.slots.reserve(n, id, copy);
                }
            }
        }
        Ok(())
    }

    /// The swatch nearest `hex` (Changed unless exact); a malformed colour
    /// is listed.
    fn colour(&mut self, hex: Option<&str>) -> Option<Swatch> {
        let hex = hex?;
        match nearest_swatch(hex.trim()) {
            Some((s, changed)) => {
                if changed {
                    self.add(ItemKind::Changed, COLOURS_NEAREST, None);
                }
                Some(s)
            }
            None => {
                let shown: String = hex.chars().take(32).collect();
                self.add(
                    ItemKind::LeftOut,
                    UNREADABLE_VALUES,
                    Some(format!("colour '{shown}'")),
                );
                None
            }
        }
    }

    /// Walks every timeline once for the clip comments it lists (the
    /// arrangement itself is `timeline`'s).
    fn timelines(&mut self) -> Result<(), IoError> {
        let p = &self.pkg.project;
        let mut stack: Vec<&'p DpTimeline> = Vec::new();
        for s in p.scenes.iter().rev() {
            stack.extend(&s.content);
        }
        if let Some(l) = p.arrangement.as_ref().and_then(|a| a.lanes.as_ref()) {
            stack.extend(l.items.iter().rev());
        }
        // The reader keeps timelines at most 16 deep.
        while let Some(t) = stack.pop() {
            self.tick()?;
            match t {
                DpTimeline::Lanes(l) => stack.extend(l.items.iter().rev()),
                DpTimeline::Clips(c) => {
                    for clip in c.clips.iter().rev() {
                        self.tick()?;
                        self.comment(clip.named.comment.as_deref(), || {
                            format!("clip '{}'", clip.named.name.as_deref().unwrap_or(""))
                        });
                        stack.extend(clip.content.as_deref());
                    }
                }
                DpTimeline::Notes(n) => {
                    for note in n.notes.iter().rev() {
                        self.tick()?;
                        stack.extend(note.content.as_deref());
                    }
                }
                DpTimeline::Points(pts) => {
                    for _ in &pts.points {
                        self.tick()?;
                    }
                }
                DpTimeline::Warps(w) => stack.push(&w.content),
                DpTimeline::ClipSlot(_, Some(clip)) => {
                    self.comment(clip.named.comment.as_deref(), || {
                        format!("clip '{}'", clip.named.name.as_deref().unwrap_or(""))
                    });
                    stack.extend(clip.content.as_deref());
                }
                DpTimeline::Audio(_)
                | DpTimeline::Markers(..)
                | DpTimeline::ClipSlot(_, None)
                | DpTimeline::Video(_)
                | DpTimeline::Unknown(_) => {}
            }
        }
        Ok(())
    }

    // ---- structure

    /// The structure in walk order, buses numbered, and every channel ID's
    /// target.
    fn nodes(&mut self) -> Result<Vec<Node<'p>>, IoError> {
        let p = &self.pkg.project;
        let mut out = Vec::new();
        let mut buses = 0usize;
        let mut roles = Roles::default();
        let mut next_bus = || {
            let b = buses;
            buses = buses.saturating_add(1);
            b
        };
        enum Item<'p> {
            Track(&'p DpTrack, Option<usize>),
            Channel(&'p DpChannel),
        }
        let mut stack: Vec<Item<'p>> = p
            .structure
            .iter()
            .rev()
            .map(|i| match i {
                DpStructureItem::Track(t) => Item::Track(t, None),
                DpStructureItem::Channel(c) => Item::Channel(c),
            })
            .collect();
        while let Some(item) = stack.pop() {
            self.tick()?;
            match item {
                Item::Track(t, parent) => {
                    let (role, master) = roles.track(t);
                    let bus = is_bus(role).then(&mut next_bus);
                    let target = match role {
                        TrackRole::Master => Target::Master,
                        TrackRole::Folder | TrackRole::Bus => Target::Bus(bus.unwrap_or(0)),
                        TrackRole::Vca | TrackRole::Content => Target::Other,
                    };
                    if let Some(id) = t.channel.as_ref().and_then(|c| c.named.id.as_deref()) {
                        self.targets.insert(id, target);
                    }
                    let inner = if role == TrackRole::Folder {
                        bus
                    } else {
                        parent
                    };
                    for c in t.children.iter().rev() {
                        stack.push(Item::Track(c, inner));
                    }
                    out.push(Node::Track {
                        t,
                        role,
                        parent,
                        bus,
                        master,
                    });
                }
                Item::Channel(c) => {
                    let (master, is_a_bus) = roles.channel(c);
                    let bus = is_a_bus.then(&mut next_bus);
                    let target = match bus {
                        Some(b) => Target::Bus(b),
                        None if master => Target::Master,
                        None => Target::Other,
                    };
                    if let Some(id) = c.named.id.as_deref() {
                        self.targets.insert(id, target);
                    }
                    out.push(Node::Channel { c, bus, master });
                }
            }
        }
        Ok(out)
    }

    fn structure(&mut self, nodes: &[Node<'p>], plan: &mut Plan) -> Result<(), IoError> {
        let buses = nodes
            .iter()
            .filter(|n| match n {
                Node::Track { bus, .. } | Node::Channel { bus, .. } => bus.is_some(),
            })
            .count();
        plan.buses = vec![
            BusPlan {
                name: String::new(),
                mix: Mix::default(),
                tag: None,
            };
            buses
        ];
        let mut tracks = 0usize;
        for node in nodes {
            self.tick()?;
            match *node {
                Node::Track {
                    t,
                    role,
                    parent,
                    bus,
                    master,
                } => {
                    if let Some(id) = t.named.id.as_deref() {
                        let index = (role == TrackRole::Content).then_some(plan.tracks.len());
                        self.track_index.insert(id, index);
                        if let (Some(i), Some(c)) = (
                            index,
                            t.channel.as_ref().and_then(|c| c.named.id.as_deref()),
                        ) {
                            self.chan_track.insert(c, i);
                        }
                    }
                    let default_out = parent.map_or(Out::Master, Out::Bus);
                    match role {
                        TrackRole::Vca => {
                            self.add(ItemKind::LeftOut, VCA, None);
                            self.comment(t.named.comment.as_deref(), || {
                                format!("track '{}'", t.named.name.as_deref().unwrap_or(""))
                            });
                        }
                        TrackRole::Master if master => {
                            let name = t.named.name.as_deref().unwrap_or("Master").to_owned();
                            if let Some(c) = t.channel.as_deref() {
                                plan.master = self.mix(c, &name, Out::Master, true, None)?;
                            }
                            plan.master.colour = self.colour(
                                t.named
                                    .color
                                    .as_deref()
                                    .or(t.channel.as_ref().and_then(|c| c.named.color.as_deref())),
                            );
                            self.comment(t.named.comment.as_deref(), || format!("track '{name}'"));
                        }
                        TrackRole::Master | TrackRole::Folder | TrackRole::Bus => {
                            let b = bus.unwrap_or(0);
                            if role == TrackRole::Folder {
                                self.add(ItemKind::Changed, FOLDERS, None);
                            }
                            let name = self.name(
                                t.named
                                    .name
                                    .as_deref()
                                    .or(t.channel.as_ref().and_then(|c| c.named.name.as_deref())),
                                || format!("Bus {}", b.saturating_add(1)),
                            );
                            let mut mix = match t.channel.as_deref() {
                                Some(c) => self.mix(c, &name, default_out, false, None)?,
                                None => Mix {
                                    output: default_out,
                                    ..Mix::default()
                                },
                            };
                            mix.colour = self.colour(
                                t.named
                                    .color
                                    .as_deref()
                                    .or(t.channel.as_ref().and_then(|c| c.named.color.as_deref())),
                            );
                            self.comment(t.named.comment.as_deref(), || format!("track '{name}'"));
                            let tag = t.channel.as_deref().and_then(|c| self.strip_named(c, None));
                            if let Some(slot) = plan.buses.get_mut(b) {
                                *slot = BusPlan { name, mix, tag };
                            }
                        }
                        TrackRole::Content => {
                            tracks = tracks.saturating_add(1);
                            let n = tracks;
                            let tp = self.track(t, n, default_out, plan)?;
                            plan.tracks.push(tp);
                        }
                    }
                }
                Node::Channel { c, bus, master } => {
                    let name = c.named.name.as_deref().unwrap_or("").to_owned();
                    if master {
                        plan.master = self.mix(c, &name, Out::Master, true, None)?;
                        plan.master.colour = self.colour(c.named.color.as_deref());
                    } else if let Some(b) = bus {
                        let name =
                            self.name(Some(&name), || format!("Bus {}", b.saturating_add(1)));
                        let mut mix = self.mix(c, &name, Out::Master, false, None)?;
                        mix.colour = self.colour(c.named.color.as_deref());
                        let tag = self.strip_named(c, None);
                        if let Some(slot) = plan.buses.get_mut(b) {
                            *slot = BusPlan { name, mix, tag };
                        }
                    } else {
                        self.add(ItemKind::LeftOut, VCA, None);
                    }
                }
            }
        }
        Ok(())
    }

    /// A note or audio track: its destination, audio path and settings.
    fn track(
        &mut self,
        t: &'p DpTrack,
        n: usize,
        default_out: Out,
        plan: &mut Plan,
    ) -> Result<TrackPlan, IoError> {
        let name = self.name(t.named.name.as_deref(), || format!("Track {n}"));
        self.comment(t.named.comment.as_deref(), || format!("track '{name}'"));
        let channel = t.channel.as_deref();
        // The audio part of a track: audio and no notes (a split track's
        // second half, or an audio track).
        let audio_part =
            t.content.contains(&DpContentType::Audio) && !t.content.contains(&DpContentType::Notes);
        let id = t.named.id.as_deref();
        let tag = self
            .named(
                id,
                &["track"],
                t.named.comment.as_deref(),
                &[TagKind::Track],
            )
            .and_then(|named| {
                let key = (named.number(), audio_part);
                let claims = &mut self.claims;
                claims
                    .lanes
                    .take(key, &named, id, None, &mut claims.ignored)
                    .then_some(key)
            });
        self.copy = channel.and_then(|c| self.copy_of(c, tag.map(|(l, _)| l)));
        let devices = channel.map_or(&[][..], |c| c.devices.as_slice());
        let first = devices
            .iter()
            .position(|d| d.role == DpDeviceRole::Instrument);
        let notes = t.content.contains(&DpContentType::Notes) || first.is_some();
        let audio = t.content.contains(&DpContentType::Audio);
        let midi = if notes {
            Some(match first.and_then(|i| devices.get(i)) {
                Some(d) => {
                    let m = self.instrument(d, &name)?;
                    if let (Midi::Plugin(_), Some(id)) = (&m, d.named.id.as_deref()) {
                        self.dev_slot.insert(id, timeline::SlotAt::Instrument);
                    }
                    m
                }
                None => {
                    self.add(ItemKind::Note, PLAYS_BUILTIN, Some(name.clone()));
                    Midi::Builtin
                }
            })
        } else {
            None
        };
        let colour_hex = t
            .named
            .color
            .as_deref()
            .or(channel.and_then(|c| c.named.color.as_deref()));
        let mut mix = match channel {
            Some(c) => self.mix(c, &name, default_out, false, first.filter(|_| notes))?,
            None => Mix {
                output: default_out,
                ..Mix::default()
            },
        };
        let dest_name = if matches!(midi, Some(Midi::Plugin(_))) {
            let device = first.and_then(|i| devices.get(i));
            let own = channel
                .and_then(|c| c.named.name.as_deref())
                .filter(|n| !n.trim().is_empty())
                .or(device.and_then(|d| d.named.name.as_deref()));
            self.name(own, || name.clone())
        } else {
            name.clone()
        };
        let shares = midi == Some(Midi::Builtin);
        let owns = matches!(midi, Some(Midi::Plugin(_))) || audio;
        if matches!(midi, Some(Midi::Plugin(_))) && audio {
            self.add(ItemKind::Changed, BOTH_STRIPS, Some(name.clone()));
        }
        if owns {
            mix.colour = self.colour(colour_hex);
        } else if colour_hex.is_some() {
            self.add(ItemKind::LeftOut, COLOURS_AND_COMMENTS, Some(name.clone()));
        }
        let mut flags = None;
        if shares {
            flags = Some((mix.mute, mix.solo));
            let settings = mix.unflagged();
            match &plan.builtin {
                None => {
                    plan.builtin = Some(Mix {
                        colour: None,
                        ..settings
                    })
                }
                Some(b)
                    if b.untagged()
                        == Mix {
                            colour: None,
                            ..settings.untagged()
                        } => {}
                Some(_) => self.add(ItemKind::LeftOut, SHARED_SYNTH, Some(name.clone())),
            }
        } else if !owns && channel.is_some_and(|c| !c.devices.is_empty() || !c.sends.is_empty()) {
            self.add(ItemKind::LeftOut, NO_STRIP, Some(name.clone()));
        }
        let strip_tag = if owns {
            let copy = self.copy;
            channel.and_then(|c| self.strip_named(c, copy))
        } else {
            None
        };
        self.copy = None;
        let channel_name = channel
            .and_then(|c| c.named.name.clone())
            .filter(|n| !n.trim().is_empty());
        Ok(TrackPlan {
            name,
            dest_name,
            midi,
            audio,
            mix,
            flags,
            tag,
            strip_tag,
            channel_name,
        })
    }

    /// The instrument a track plays from its first instrument device.
    fn instrument(&mut self, d: &DpDevice, track: &str) -> Result<Midi, IoError> {
        let name = d.device_name.clone();
        Ok(match d.kind {
            DpDeviceKind::Au | DpDeviceKind::Clap => match self.slot(d)? {
                Some(s) => Midi::Plugin(s),
                None => Midi::Builtin,
            },
            DpDeviceKind::Vst2 | DpDeviceKind::Vst3 => {
                self.add(ItemKind::LeftOut, VST, Some(name));
                Midi::Builtin
            }
            kind => {
                self.builtin_device(BUILTIN_INSTRUMENTS, kind, track, &d.settings);
                Midi::Builtin
            }
        })
    }

    /// Another DAW's built-in device on `track`, listed once under `what`
    /// with its settings as details.
    fn builtin_device(&mut self, what: &str, kind: DpDeviceKind, track: &str, settings: &[String]) {
        let place = format!("{} on track '{track}'", kind_name(kind));
        let details: Vec<String> = if settings.is_empty() {
            vec![place]
        } else {
            settings
                .iter()
                .take(crate::limits::MAX_DETAILS)
                .map(|s| format!("{place}: {s}"))
                .collect()
        };
        self.add_with(ItemKind::LeftOut, what, &details);
    }

    /// A plug-in state entry, read once per path and format and shared by
    /// every device naming it. The states held together stay within the
    /// limits' `max_state_total`; past it, `Err` is [`TOO_LARGE_TO_CARRY`].
    fn state_of(&mut self, path: &str, format: PluginFormat) -> Result<StateRead, IoError> {
        let key = (path.to_owned(), format);
        if let Some(r) = self.states.get(&key) {
            return Ok(r.clone());
        }
        let limits = self.pkg.limits();
        let room = limits.max_state_total.saturating_sub(self.held);
        let got = if self
            .pkg
            .entry_size(path)
            .is_some_and(|n| n <= limits.max_state && n > room)
        {
            Err(TOO_LARGE_TO_CARRY.to_owned())
        } else {
            match self.pkg.state(path, self.cancel) {
                Err(IoError::Cancelled) => return Err(IoError::Cancelled),
                Err(e) => Err(e.to_string()),
                Ok(raw) => match format {
                    PluginFormat::Clap => Ok(raw),
                    PluginFormat::Au => {
                        let bin = self.env.plist.to_binary(&raw);
                        drop(raw);
                        bin
                    }
                },
            }
        };
        let got = got.and_then(|b| {
            let n = u64::try_from(b.len()).unwrap_or(u64::MAX);
            if n > room {
                Err(TOO_LARGE_TO_CARRY.to_owned())
            } else {
                self.held = self.held.saturating_add(n);
                Ok(Arc::from(b))
            }
        });
        self.states.insert(key, got.clone());
        Ok(got)
    }

    /// An AU or CLAP device as a slot, with its state.
    fn slot(&mut self, d: &DpDevice) -> Result<Option<SlotPlan>, IoError> {
        let name = d.device_name.clone();
        let format = match d.kind {
            DpDeviceKind::Au => PluginFormat::Au,
            DpDeviceKind::Clap => PluginFormat::Clap,
            _ => return Ok(None),
        };
        self.comment(d.named.comment.as_deref(), || format!("plug-in '{name}'"));
        let Some(id) = d.device_id.as_deref().filter(|i| !i.trim().is_empty()) else {
            self.add(ItemKind::LeftOut, NO_PLUGIN_ID, Some(name));
            return Ok(None);
        };
        if !(self.env.installed)(format, id) {
            self.add(ItemKind::Note, NOT_INSTALLED, Some(name.clone()));
        }
        let did = d.named.id.as_deref();
        let tag = self
            .named(
                did,
                &["device"],
                d.named.comment.as_deref(),
                &[TagKind::Device, TagKind::Eq],
            )
            .and_then(|named| {
                let n = named.number();
                let copy = self.copy;
                let claims = &mut self.claims;
                claims
                    .slots
                    .take(n, &named, did, copy, &mut claims.ignored)
                    .then_some(n)
            });
        let state = match &d.state {
            None => None,
            Some(f) if f.external => {
                let why = format!("{name}: it is outside the file");
                self.add(ItemKind::LeftOut, PLUGIN_SETTINGS, Some(why));
                None
            }
            Some(f) => match self.state_of(&f.path, format)? {
                Ok(b) => Some(b),
                Err(why) => {
                    self.add(
                        ItemKind::LeftOut,
                        PLUGIN_SETTINGS,
                        Some(format!("{name}: {why}")),
                    );
                    None
                }
            },
        };
        Ok(Some(SlotPlan {
            plugin: PluginRef {
                format,
                id: id.to_owned(),
                name: name.clone(),
                vendor: d.vendor.clone().unwrap_or_default(),
                version: d.version.clone().unwrap_or_default(),
            },
            bypass: d.enabled.as_ref().and_then(|e| e.value) == Some(false),
            state,
            tag,
        }))
    }

    /// A level parameter in dB: linear through `db_of_linear`, decibels as
    /// they are; cut to +12 dB (Changed). `None` (listed) otherwise.
    fn level(&mut self, p: &DpRealParam, what: &str) -> Option<f64> {
        let v = p.value?;
        let db = match p.unit {
            // A normalised volume is read as a linear gain from 0 to 1.
            DpUnit::Linear | DpUnit::Normalized if !v.is_nan() => db_of_linear(v),
            DpUnit::Decibel if !v.is_nan() => v.max(GAIN_FLOOR_DB),
            _ => {
                self.add(ItemKind::LeftOut, UNREADABLE_VALUES, Some(what.to_owned()));
                return None;
            }
        };
        if db > GAIN_MAX_DB {
            self.add(ItemKind::Changed, LEVELS_CUT, None);
            return Some(GAIN_MAX_DB);
        }
        Some(db)
    }

    /// A pan parameter as −1 … +1: normalised as it is, other units scaled
    /// by their finite `min`/`max`; `None` (listed) otherwise.
    fn pan(&mut self, p: &DpRealParam, what: &str) -> Option<f64> {
        let n = pan_value(p);
        if n.is_none() {
            self.add(ItemKind::LeftOut, UNREADABLE_VALUES, Some(what.to_owned()));
        }
        n
    }

    /// Where a destination ID goes: the master, a bus, or (Changed) the
    /// master in place of another part.
    fn destination(&mut self, id: Option<&str>, default: Out) -> Out {
        let Some(id) = id else { return default };
        match self.targets.get(id).copied() {
            Some(Target::Master) => Out::Master,
            Some(Target::Bus(b)) => Out::Bus(b),
            Some(Target::Other) => {
                self.add(ItemKind::Changed, OUTPUT_TO_TRACK, None);
                Out::Master
            }
            // Listed by the ID check as a missing reference.
            None => Out::Master,
        }
    }

    /// A channel's settings; `skip` is the device a track plays as its
    /// instrument.
    fn mix(
        &mut self,
        c: &'p DpChannel,
        owner: &str,
        default_out: Out,
        master: bool,
        skip: Option<usize>,
    ) -> Result<Mix, IoError> {
        let mut mix = Mix {
            output: if master {
                Out::Master
            } else {
                self.destination(c.destination.as_deref(), default_out)
            },
            ..Mix::default()
        };
        self.comment(c.named.comment.as_deref(), || format!("track '{owner}'"));
        if let Some(v) = c.volume.as_deref() {
            mix.gain_db = self
                .level(v, &format!("volume of '{owner}'"))
                .unwrap_or(0.0);
        }
        if let Some(p) = c.pan.as_deref() {
            mix.pan = self.pan(p, &format!("pan of '{owner}'")).unwrap_or(0.0);
        }
        mix.mute = c.mute.as_ref().and_then(|m| m.value).unwrap_or(false);
        mix.solo = c.solo.unwrap_or(false);
        if master && (mix.pan.abs() > 1e-9 || mix.solo) {
            self.add(ItemKind::LeftOut, MASTER_PAN_SOLO, None);
            mix.pan = 0.0;
            mix.solo = false;
        }
        for s in &c.sends {
            self.tick()?;
            if master {
                self.add(ItemKind::LeftOut, MASTER_SENDS, None);
                continue;
            }
            let target = s
                .destination
                .as_deref()
                .and_then(|d| self.targets.get(d).copied());
            let Some(Target::Bus(bus)) = target else {
                self.add(ItemKind::LeftOut, SEND_TARGETS, Some(owner.to_owned()));
                continue;
            };
            // A send's pan has no place in VizDAW; a centred one loses nothing.
            let off_centre = s
                .pan
                .as_ref()
                .is_some_and(|p| pan_value(p).is_none_or(|v| v.abs() > 1e-9));
            if off_centre {
                self.add(ItemKind::LeftOut, SEND_PAN, Some(owner.to_owned()));
            }
            let level_db = self
                .level(&s.volume, &format!("send level of '{owner}'"))
                .unwrap_or(0.0);
            if let (Some(id), Some(chan)) = (s.named.id.as_deref(), c.named.id.as_deref()) {
                self.send_at.insert(id, (chan, mix.sends.len()));
            }
            mix.sends.push(SendPlan {
                bus,
                level_db,
                pre: s.kind == Some(DpSendType::Pre),
                enabled: s.enable.as_ref().and_then(|e| e.value).unwrap_or(true),
            });
        }
        for (i, d) in c.devices.iter().enumerate() {
            self.tick()?;
            if Some(i) == skip {
                continue;
            }
            let name = d.device_name.clone();
            match d.role {
                DpDeviceRole::Instrument => {
                    let detail = format!("{name} on '{owner}'");
                    self.add(ItemKind::LeftOut, EXTRA_INSTRUMENTS, Some(detail));
                }
                DpDeviceRole::NoteFx => self.add(ItemKind::LeftOut, NOTE_EFFECTS, Some(name)),
                DpDeviceRole::Analyzer => self.add(ItemKind::LeftOut, ANALYZERS, Some(name)),
                DpDeviceRole::AudioFx => match d.kind {
                    DpDeviceKind::Au | DpDeviceKind::Clap => {
                        let eq_tag = d.named.comment.as_deref().is_some_and(|c| {
                            strip_tags(c).1.iter().any(|(k, _)| *k == TagKind::Eq)
                        });
                        if let Some(slot) = self.slot(d)? {
                            if !(self.env.installed)(slot.plugin.format, &slot.plugin.id) {
                                mix.unhosted
                                    .push((name.clone(), Some(slot.plugin.id.clone())));
                            }
                            let at = if eq_tag && mix.eq.is_none() {
                                mix.eq = Some(slot);
                                timeline::SlotAt::Eq
                            } else {
                                mix.inserts.push(slot);
                                timeline::SlotAt::Insert(mix.inserts.len() - 1)
                            };
                            if let Some(id) = d.named.id.as_deref() {
                                self.dev_slot.insert(id, at);
                            }
                        }
                    }
                    DpDeviceKind::Vst2 | DpDeviceKind::Vst3 => {
                        mix.unhosted.push((name.clone(), None));
                        self.add(ItemKind::LeftOut, VST, Some(name));
                    }
                    kind => self.builtin_device(BUILTIN_EFFECTS, kind, owner, &d.settings),
                },
            }
        }
        Ok(mix)
    }
}

/// A pan parameter as −1 … +1: normalised as it is, other units scaled by
/// their finite `min`/`max`; `None` when it cannot be read.
fn pan_value(p: &DpRealParam) -> Option<f64> {
    let v = p.value.filter(|v| v.is_finite())?;
    let n = match (p.unit, p.min, p.max) {
        (DpUnit::Normalized, _, _) => v,
        (_, Some(lo), Some(hi)) if lo.is_finite() && hi.is_finite() && hi > lo => {
            (v - lo) / (hi - lo)
        }
        _ => return None,
    };
    Some(pan_of_normalized(n.clamp(0.0, 1.0)))
}

/// The element name of a built-in device kind.
fn kind_name(kind: DpDeviceKind) -> &'static str {
    match kind {
        DpDeviceKind::Au => "AuPlugin",
        DpDeviceKind::Clap => "ClapPlugin",
        DpDeviceKind::Vst2 => "Vst2Plugin",
        DpDeviceKind::Vst3 => "Vst3Plugin",
        DpDeviceKind::Builtin => "BuiltinDevice",
        DpDeviceKind::Equalizer => "Equalizer",
        DpDeviceKind::Compressor => "Compressor",
        DpDeviceKind::NoiseGate => "NoiseGate",
        DpDeviceKind::Limiter => "Limiter",
        DpDeviceKind::Other => "Device",
    }
}

/// Cuts the bus outputs and sends that would make a loop: one walk, depth
/// first and without recursion, removes every edge that closes a cycle
/// (what is left has none). Returns the sends removed, `(bus, index)`, the
/// last first.
fn cut_loops(plan: &mut Plan, report: &mut Report) -> Vec<(usize, usize)> {
    // Edges: (from, to, None for the output or Some(send index)).
    let n = plan.buses.len();
    let mut edges: Vec<Vec<(usize, Option<usize>)>> = vec![Vec::new(); n];
    for (i, b) in plan.buses.iter().enumerate() {
        let Some(e) = edges.get_mut(i) else { continue };
        if let Out::Bus(to) = b.mix.output {
            e.push((to, None));
        }
        for (k, s) in b.mix.sends.iter().enumerate() {
            e.push((s.bus, Some(k)));
        }
    }
    // 0 unvisited, 1 on the path, 2 done.
    let mut state = vec![0u8; n];
    let mut cut: Vec<(usize, Option<usize>)> = Vec::new();
    for start in 0..n {
        if state.get(start).copied() != Some(0) {
            continue;
        }
        let mut path: Vec<(usize, usize)> = vec![(start, 0)];
        if let Some(s) = state.get_mut(start) {
            *s = 1;
        }
        while let Some(top) = path.last_mut() {
            let (node, at) = *top;
            let Some(&(to, which)) = edges.get(node).and_then(|e| e.get(at)) else {
                if let Some(s) = state.get_mut(node) {
                    *s = 2;
                }
                path.pop();
                continue;
            };
            top.1 = at.saturating_add(1);
            match state.get(to).copied() {
                Some(1) => cut.push((node, which)),
                Some(0) => {
                    if let Some(s) = state.get_mut(to) {
                        *s = 1;
                    }
                    path.push((to, 0));
                }
                _ => {}
            }
        }
    }
    // Sends are removed from the last, so earlier indices stay valid.
    cut.sort_by(|a, b| b.cmp(a));
    let mut sends = Vec::new();
    for (bus, which) in cut {
        let Some(b) = plan.buses.get_mut(bus) else {
            continue;
        };
        match which {
            None => {
                b.mix.output = Out::Master;
                report.add(ItemKind::Changed, LOOP_OUTPUTS, Some(b.name.clone()));
            }
            Some(k) => {
                if k < b.mix.sends.len() {
                    b.mix.sends.remove(k);
                    sends.push((bus, k));
                }
                report.add(ItemKind::LeftOut, LOOP_SENDS, Some(b.name.clone()));
            }
        }
    }
    sends
}
