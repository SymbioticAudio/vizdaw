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
//! Everything the plan cannot hold is listed in the report; each `what` is
//! a constant (or, when it names something, a function) here.

use super::package::{DpPackage, TrackRole, role_of};
use super::plugins::PlistCodec;
use super::schema::*;
use super::units::{
    EXPORT_LINE, TagKind, db_of_linear, nearest_swatch, pan_of_normalized, strip_tags,
};
use crate::IoError;
use crate::report::{ItemKind, Report};
use core_model::command::{
    Command, LaneCmd, MixerCmd, Report as CmdReport, RoutingCmd, SequenceCmd, SettingsCmd,
};
use core_model::document::{DestKind, MAX_INFO_BYTES, SongInfo};
use core_model::error::ModelError;
use core_model::history::History;
use core_model::ids::{AnyId, DestId, PluginInstanceId, SeqId, StripId};
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
use std::collections::{BTreeMap, HashMap};
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
}

/// A mapped import: what [`apply`] will do, and the report so far (the
/// package's reading included).
#[derive(Debug, Clone, PartialEq)]
pub struct DpImport {
    /// What reading and mapping changed, left out or noted.
    pub report: Report,
    plan: Plan,
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
/// Left out: comment text (tags removed) on tracks, channels, devices,
/// clips and markers.
pub const COMMENTS: &str = "Comments on tracks, clips and markers";
/// Left out: colours the project has no place for (markers, and tracks
/// that own no strip).
pub const COLOURS_AND_COMMENTS: &str = "Colours and comments on markers and tracks";
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
/// Note (Ruling 16): the file came from VizDAW.
pub const FROM_VIZDAW: &str =
    "This file came from a VizDAW project; it was opened as a new project";

/// Note: a plug-in that is not installed here.
pub fn not_installed(name: &str) -> String {
    format!("Plug-in '{name}' is not installed; it keeps its settings until it is")
}

/// Left out: a VST plug-in.
pub fn vst(name: &str) -> String {
    format!("Plug-in '{name}' (VST)")
}

/// Left out: a plug-in the file gives no ID for.
pub fn no_plugin_id(name: &str) -> String {
    format!("Plug-in '{name}' (no plug-in ID)")
}

/// Note: a note track with no instrument the project can play.
pub fn plays_builtin(track: &str) -> String {
    format!("Track '{track}' plays the built-in synth")
}

/// Left out: another DAW's built-in effect, with its settings as details.
pub fn builtin_effect(kind: &str, track: &str) -> String {
    format!("Built-in effect '{kind}' on track '{track}'")
}

/// Left out: another DAW's built-in instrument, with its settings as
/// details.
pub fn builtin_instrument(kind: &str, track: &str) -> String {
    format!("Built-in instrument '{kind}' on track '{track}'")
}

/// Left out: an instrument after a track's first, or on a bus.
pub fn extra_instrument(name: &str) -> String {
    format!("Instrument '{name}' (a track plays one instrument)")
}

/// Left out: a note effect.
pub fn note_effect(name: &str) -> String {
    format!("Note effect '{name}'")
}

/// Left out: an analyzer.
pub fn analyzer(name: &str) -> String {
    format!("Analyzer '{name}'")
}

/// Left out (Ruling 8): a plug-in's settings that could not be read.
pub fn plugin_settings(name: &str) -> String {
    format!("Settings of plug-in '{name}'")
}

const NOT_NEW: &str = "A DAWproject opens as a new project.";
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
}

#[derive(Debug, Clone, PartialEq)]
enum Midi {
    Plugin(SlotPlan),
    Builtin,
}

#[derive(Debug, Clone, PartialEq)]
struct TrackPlan {
    name: String,
    midi: Option<Midi>,
    audio: bool,
    /// The settings of the strips the track owns.
    mix: Mix,
    /// Mute and solo kept on the track rather than its strips (it shares
    /// the built-in synth's strip).
    flags: Option<(bool, bool)>,
}

#[derive(Debug, Clone, PartialEq)]
struct BusPlan {
    name: String,
    mix: Mix,
}

#[derive(Debug, Clone, PartialEq)]
struct Plan {
    name: String,
    info: SongInfo,
    tempo: TempoMap,
    meter: MeterMap,
    markers: Vec<Marker>,
    buses: Vec<BusPlan>,
    master: Mix,
    builtin: Option<Mix>,
    tracks: Vec<TrackPlan>,
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

struct Cx<'a, 'p> {
    pkg: &'p DpPackage,
    env: &'a ImportEnv<'a>,
    cancel: &'a AtomicBool,
    report: Report,
    n: usize,
    targets: HashMap<&'p str, Target>,
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
    // The timeline mapping (plan 6d Task 7) places the media.
    let _ = media;
    let mut cx = Cx {
        pkg,
        env,
        cancel,
        report: pkg.report.clone(),
        n: 0,
        targets: HashMap::new(),
    };
    let p = &pkg.project;
    if p.dropped_text > 0 {
        cx.add_n(ItemKind::LeftOut, TEXT_DROPPED, p.dropped_text);
    }
    if pkg.metadata.dropped_text > 0 {
        cx.add_n(ItemKind::LeftOut, TEXT_DROPPED, pkg.metadata.dropped_text);
    }
    let (tempo, meter) = cx.maps()?;
    let markers = cx.markers(&tempo)?;
    cx.timelines()?;
    let nodes = cx.nodes()?;
    let mut plan = Plan {
        name: String::new(),
        info: SongInfo::default(),
        tempo,
        meter,
        markers,
        buses: Vec::new(),
        master: Mix::default(),
        builtin: None,
        tracks: Vec::new(),
    };
    cx.structure(&nodes, &mut plan)?;
    cut_loops(&mut plan, &mut cx.report);
    let (name, info) = cx.song_info();
    plan.name = name;
    plan.info = info;
    if pkg.ret.from_vizdaw() {
        cx.report.add(ItemKind::Note, FROM_VIZDAW, None);
    }
    stop(cancel)?;
    Ok(DpImport {
        report: cx.report,
        plan,
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
            self.add(ItemKind::LeftOut, COMMENTS, Some(place()));
        }
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

    // ---- maps

    /// A beats or seconds time as ticks; seconds through `seconds`. `None`
    /// (listed) for a time before 0 or past the timeline.
    fn time(&mut self, t: f64, unit: DpTimeUnit, tempo: &TempoMap) -> Option<Tick> {
        let tick = match unit {
            DpTimeUnit::Beats => super::units::ticks(t).map(|(t, _)| t),
            DpTimeUnit::Seconds if t.is_finite() => Some(tempo.seconds_to_tick(t)),
            DpTimeUnit::Seconds => None,
        };
        match tick {
            Some(k) if (0..=MAX_TICKS).contains(&k.0) => Some(k),
            _ => {
                self.add(ItemKind::LeftOut, PAST_END, None);
                None
            }
        }
    }

    /// A tempo in BPM, clamped to 5–999 (Changed); `None` (listed) when not
    /// a number.
    fn bpm(&mut self, v: f64, what: &str) -> Option<Bpm> {
        if !v.is_finite() {
            self.add(ItemKind::LeftOut, UNREADABLE_VALUES, Some(what.to_owned()));
            return None;
        }
        let c = v.clamp(Bpm::MIN.as_f64(), Bpm::MAX.as_f64());
        if c != v {
            self.add(ItemKind::Changed, TEMPO_CLAMPED, None);
        }
        // Within 5 000–999 000 milli-BPM, so the cast is exact.
        Some(Bpm((c * 1000.0).round() as u32))
    }

    fn meter(&mut self, num: i32, den: i32) -> Option<Meter> {
        let m = u8::try_from(num)
            .ok()
            .zip(u8::try_from(den).ok())
            .map(|(num, den)| Meter { num, den })
            .filter(|m| m.is_valid());
        if m.is_none() {
            self.add(ItemKind::LeftOut, BAD_METER, Some(format!("{num}/{den}")));
        }
        m
    }

    /// The tempo map, then the meter map (meter times in seconds go
    /// through the finished tempo map).
    fn maps(&mut self) -> Result<(TempoMap, MeterMap), IoError> {
        let p = &self.pkg.project;
        let arr = p.arrangement.as_ref();
        let first = p
            .tempo
            .as_ref()
            .and_then(|t| t.value)
            .and_then(|v| self.bpm(v, "Tempo@value"))
            .unwrap_or(Bpm(120_000));
        let mut points = vec![TempoPoint {
            tick: Tick::ZERO,
            bpm: first,
            ramp: false,
        }];
        // Seconds at the last point, for seconds times.
        let mut secs_at_last = 0.0f64;
        if let Some(a) = arr.and_then(|a| a.tempo_automation.as_ref()) {
            let unit = a.head.time_unit.unwrap_or(DpTimeUnit::Beats);
            if a.unit.is_some_and(|u| u != DpUnit::Bpm) {
                self.add(
                    ItemKind::LeftOut,
                    UNREADABLE_VALUES,
                    Some("TempoAutomation@unit".to_owned()),
                );
            } else {
                for pt in &a.points {
                    self.tick()?;
                    let DpPointValue::Real(v, how) = pt.value else {
                        self.add(
                            ItemKind::LeftOut,
                            UNREADABLE_VALUES,
                            Some("TempoAutomation point".to_owned()),
                        );
                        continue;
                    };
                    let Some(bpm) = self.bpm(v, "TempoAutomation point") else {
                        continue;
                    };
                    let ramp = how == Some(DpInterpolation::Linear);
                    let Some(last) = points.last().copied() else {
                        continue;
                    };
                    let tick = match unit {
                        DpTimeUnit::Beats => super::units::ticks(pt.time).map(|(t, _)| t.0),
                        DpTimeUnit::Seconds => {
                            let dt = pt.time - secs_at_last;
                            if dt < 0.0 {
                                self.add(ItemKind::LeftOut, MAP_ORDER, None);
                                continue;
                            }
                            seconds_after(last, bpm, dt).map(|d| last.tick.0.saturating_add(d))
                        }
                    };
                    let Some(tick) = tick.filter(|t| (0..=MAX_TICKS).contains(t)) else {
                        self.add(ItemKind::LeftOut, PAST_END, None);
                        continue;
                    };
                    if unit == DpTimeUnit::Seconds && tick >= last.tick.0 {
                        secs_at_last = pt.time;
                    }
                    let new = TempoPoint {
                        tick: Tick(tick),
                        bpm,
                        ramp,
                    };
                    match points.last_mut() {
                        Some(l) if l.tick.0 == tick => *l = new,
                        Some(l) if l.tick.0 > tick => {
                            self.add(ItemKind::LeftOut, MAP_ORDER, None);
                        }
                        _ => points.push(new),
                    }
                }
            }
        }
        let tempo = TempoMap::new(points)
            .or_else(|_| TempoMap::constant(Bpm(120_000)))
            .unwrap_or_default();

        let key = KeySig::default();
        let m0 = p
            .time_signature
            .as_ref()
            .and_then(|t| self.meter(t.numerator, t.denominator))
            .unwrap_or(Meter { num: 4, den: 4 });
        let mut entries = vec![MeterEntry {
            tick: Tick::ZERO,
            meter: m0,
            key,
        }];
        if let Some(a) = arr.and_then(|a| a.time_signature_automation.as_ref()) {
            let unit = a.head.time_unit.unwrap_or(DpTimeUnit::Beats);
            for pt in &a.points {
                self.tick()?;
                let DpPointValue::TimeSig(num, den) = pt.value else {
                    self.add(
                        ItemKind::LeftOut,
                        UNREADABLE_VALUES,
                        Some("TimeSignatureAutomation point".to_owned()),
                    );
                    continue;
                };
                let Some(meter) = self.meter(num, den) else {
                    continue;
                };
                let Some(tick) = self.time(pt.time, unit, &tempo) else {
                    continue;
                };
                let Some(last) = entries.last().copied() else {
                    continue;
                };
                if tick < last.tick {
                    self.add(ItemKind::LeftOut, MAP_ORDER, None);
                    continue;
                }
                let bar = last.meter.bar_ticks().max(1);
                let rel = tick.0.saturating_sub(last.tick.0);
                let at = rel
                    .checked_add(bar - 1)
                    .map(|r| r / bar * bar)
                    .and_then(|r| r.checked_add(last.tick.0))
                    .filter(|t| *t <= MAX_TICKS);
                let Some(at) = at else {
                    self.add(ItemKind::LeftOut, PAST_END, None);
                    continue;
                };
                if at != tick.0 {
                    self.add(ItemKind::Changed, METER_MOVED, None);
                }
                if at == last.tick.0 {
                    if let Some(l) = entries.last_mut() {
                        l.meter = meter;
                    }
                } else if meter != last.meter {
                    entries.push(MeterEntry {
                        tick: Tick(at),
                        meter,
                        key,
                    });
                }
            }
        }
        let meter = MeterMap::new(entries).unwrap_or_default();
        Ok((tempo, meter))
    }

    fn markers(&mut self, tempo: &TempoMap) -> Result<Vec<Marker>, IoError> {
        let Some(a) = self.pkg.project.arrangement.as_ref() else {
            return Ok(Vec::new());
        };
        let unit = a.markers_head.time_unit.unwrap_or(DpTimeUnit::Beats);
        let mut out = Vec::new();
        for (i, m) in a.markers.iter().enumerate() {
            self.tick()?;
            let name = self.name(m.named.name.as_deref(), || {
                format!("Marker {}", i.saturating_add(1))
            });
            if m.named.color.is_some() {
                self.add(ItemKind::LeftOut, COLOURS_AND_COMMENTS, Some(name.clone()));
            }
            self.comment(m.named.comment.as_deref(), || format!("marker '{name}'"));
            if let Some(tick) = self.time(m.time, unit, tempo) {
                out.push(Marker { tick, name });
            }
        }
        out.sort_by_key(|m| m.tick);
        Ok(out)
    }

    /// Walks every timeline for what this task lists (clip comments); the
    /// timeline mapping itself is plan 6d Task 7's.
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
        let mut master_seen = false;
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
                    let mut role = role_of(t);
                    let master = role == TrackRole::Master && !master_seen;
                    if role == TrackRole::Master && master_seen {
                        role = TrackRole::Bus;
                    }
                    master_seen |= master;
                    let bus =
                        matches!(role, TrackRole::Folder | TrackRole::Bus).then(&mut next_bus);
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
                    let master = c.role == Some(DpMixerRole::Master) && !master_seen;
                    master_seen |= master;
                    let vca = c.role == Some(DpMixerRole::Vca);
                    let bus = (!master && !vca).then(&mut next_bus);
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
                            if let Some(slot) = plan.buses.get_mut(b) {
                                *slot = BusPlan { name, mix };
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
                        if let Some(slot) = plan.buses.get_mut(b) {
                            *slot = BusPlan { name, mix };
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
        let devices = channel.map_or(&[][..], |c| c.devices.as_slice());
        let first = devices
            .iter()
            .position(|d| d.role == DpDeviceRole::Instrument);
        let notes = t.content.contains(&DpContentType::Notes) || first.is_some();
        let audio = t.content.contains(&DpContentType::Audio);
        let midi = if notes {
            Some(match first.and_then(|i| devices.get(i)) {
                Some(d) => self.instrument(d, &name),
                None => {
                    self.add(ItemKind::Note, &plays_builtin(&name), None);
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
        let shares = midi == Some(Midi::Builtin);
        let owns = matches!(midi, Some(Midi::Plugin(_))) || audio;
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
                    if *b
                        == Mix {
                            colour: None,
                            ..settings.clone()
                        } => {}
                Some(_) => self.add(ItemKind::LeftOut, SHARED_SYNTH, Some(name.clone())),
            }
        } else if !owns && channel.is_some_and(|c| !c.devices.is_empty() || !c.sends.is_empty()) {
            self.add(ItemKind::LeftOut, NO_STRIP, Some(name.clone()));
        }
        Ok(TrackPlan {
            name,
            midi,
            audio,
            mix,
            flags,
        })
    }

    /// The instrument a track plays from its first instrument device.
    fn instrument(&mut self, d: &DpDevice, track: &str) -> Midi {
        let name = d.device_name.clone();
        match d.kind {
            DpDeviceKind::Au | DpDeviceKind::Clap => match self.slot(d) {
                Some(s) => Midi::Plugin(s),
                None => Midi::Builtin,
            },
            DpDeviceKind::Vst2 | DpDeviceKind::Vst3 => {
                self.add(ItemKind::LeftOut, &vst(&name), None);
                Midi::Builtin
            }
            kind => {
                let what = builtin_instrument(kind_name(kind), track);
                self.add_with(ItemKind::LeftOut, &what, &d.settings);
                Midi::Builtin
            }
        }
    }

    /// An AU or CLAP device as a slot, with its state.
    fn slot(&mut self, d: &DpDevice) -> Option<SlotPlan> {
        let name = d.device_name.clone();
        let format = match d.kind {
            DpDeviceKind::Au => PluginFormat::Au,
            DpDeviceKind::Clap => PluginFormat::Clap,
            _ => return None,
        };
        self.comment(d.named.comment.as_deref(), || format!("plug-in '{name}'"));
        let Some(id) = d.device_id.as_deref().filter(|i| !i.trim().is_empty()) else {
            self.add(ItemKind::LeftOut, &no_plugin_id(&name), None);
            return None;
        };
        if !(self.env.installed)(format, id) {
            self.add(ItemKind::Note, &not_installed(&name), None);
        }
        let state = match &d.state {
            None => None,
            Some(f) if f.external => {
                self.add(ItemKind::LeftOut, &plugin_settings(&name), None);
                None
            }
            Some(f) => {
                let bytes = self.pkg.state(&f.path).map_err(|e| e.to_string());
                let bytes = match format {
                    PluginFormat::Au => bytes.and_then(|b| self.env.plist.to_binary(&b)),
                    PluginFormat::Clap => bytes,
                };
                match bytes {
                    Ok(b) => Some(Arc::from(b)),
                    Err(why) => {
                        self.add(ItemKind::LeftOut, &plugin_settings(&name), Some(why));
                        None
                    }
                }
            }
        };
        Some(SlotPlan {
            plugin: PluginRef {
                format,
                id: id.to_owned(),
                name: name.clone(),
                vendor: d.vendor.clone().unwrap_or_default(),
                version: d.version.clone().unwrap_or_default(),
            },
            bypass: d.enabled.as_ref().and_then(|e| e.value) == Some(false),
            state,
        })
    }

    /// A level parameter in dB: linear through `db_of_linear`, decibels as
    /// they are; cut to +12 dB (Changed). `None` (listed) otherwise.
    fn level(&mut self, p: &DpRealParam, what: &str) -> Option<f64> {
        let v = p.value?;
        let db = match p.unit {
            DpUnit::Linear if !v.is_nan() => db_of_linear(v),
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
        let v = p.value.filter(|v| v.is_finite());
        let n = match (p.unit, v, p.min, p.max) {
            (DpUnit::Normalized, Some(v), _, _) => Some(v),
            (_, Some(v), Some(lo), Some(hi)) if lo.is_finite() && hi.is_finite() && hi > lo => {
                Some((v - lo) / (hi - lo))
            }
            _ => None,
        };
        if n.is_none() {
            self.add(ItemKind::LeftOut, UNREADABLE_VALUES, Some(what.to_owned()));
        }
        n.map(|n| pan_of_normalized(n.clamp(0.0, 1.0)))
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
            let off_centre = s.pan.as_ref().is_some_and(|p| {
                p.unit != DpUnit::Normalized || p.value.is_some_and(|v| (v - 0.5).abs() > 1e-9)
            });
            if off_centre {
                self.add(ItemKind::LeftOut, SEND_PAN, Some(owner.to_owned()));
            }
            let level_db = self
                .level(&s.volume, &format!("send level of '{owner}'"))
                .unwrap_or(0.0);
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
                    self.add(
                        ItemKind::LeftOut,
                        &extra_instrument(&name),
                        Some(owner.to_owned()),
                    );
                }
                DpDeviceRole::NoteFx => self.add(ItemKind::LeftOut, &note_effect(&name), None),
                DpDeviceRole::Analyzer => self.add(ItemKind::LeftOut, &analyzer(&name), None),
                DpDeviceRole::AudioFx => match d.kind {
                    DpDeviceKind::Au | DpDeviceKind::Clap => {
                        let eq_tag = d.named.comment.as_deref().is_some_and(|c| {
                            strip_tags(c).1.iter().any(|(k, _)| *k == TagKind::Eq)
                        });
                        if let Some(slot) = self.slot(d) {
                            if eq_tag && mix.eq.is_none() {
                                mix.eq = Some(slot);
                            } else {
                                mix.inserts.push(slot);
                            }
                        }
                    }
                    DpDeviceKind::Vst2 | DpDeviceKind::Vst3 => {
                        self.add(ItemKind::LeftOut, &vst(&name), None);
                    }
                    kind => {
                        let what = builtin_effect(kind_name(kind), owner);
                        self.add_with(ItemKind::LeftOut, &what, &d.settings);
                    }
                },
            }
        }
        Ok(mix)
    }

    /// The song info and the sequence's name (the title, else the file's
    /// stem).
    fn song_info(&mut self) -> (String, SongInfo) {
        let m = &self.pkg.metadata;
        let mut info = SongInfo::default();
        for (i, (out, field)) in info.fields_mut().into_iter().zip(&m.fields).enumerate() {
            let Some(text) = field else { continue };
            let mut text = text.as_str();
            let stripped;
            if i == METADATA_FIELDS.len().saturating_sub(1) {
                stripped = without_export_line(text);
                text = &stripped;
            }
            let (kept, cut) = cut_to(text.trim_end(), MAX_INFO_BYTES);
            if cut || m.cut.get(i).copied().unwrap_or(false) {
                let field = METADATA_FIELDS.get(i).copied().unwrap_or("");
                self.add(ItemKind::Changed, INFO_CUT, Some(field.to_owned()));
            }
            if !kept.trim().is_empty() {
                *out = Some(kept.to_owned());
            }
        }
        let stem = self.pkg.stem().to_owned();
        let name = self.name(info.title.as_deref(), || {
            let (s, _) = clean_name(&stem);
            if s.is_empty() {
                "Sequence 1".to_owned()
            } else {
                s
            }
        });
        (name, info)
    }
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

/// `comment` without its last line when that is the export line.
fn without_export_line(comment: &str) -> String {
    let trimmed = comment.trim_end();
    let (head, last) = match trimmed.rfind('\n') {
        Some(i) => (
            trimmed.get(..i).unwrap_or(""),
            trimmed.get(i + 1..).unwrap_or(""),
        ),
        None => ("", trimmed),
    };
    if last.starts_with(EXPORT_LINE) {
        head.trim_end().to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// `s` cut to at most `max` bytes at a character boundary, and whether it
/// was cut.
fn cut_to(s: &str, max: usize) -> (&str, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    (s.get(..end).unwrap_or(""), true)
}

/// `raw` as a track, bus, marker or sequence name: control and zero-width
/// characters dropped (as `core_model::lane::valid_name` wants), trimmed,
/// cut to [`MAX_INFO_BYTES`]; and whether it was cut.
fn clean_name(raw: &str) -> (String, bool) {
    let bad =
        |c: char| c.is_control() || matches!(c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}');
    let mut out = String::new();
    let mut cut = false;
    for c in raw.trim().chars().filter(|c| !bad(*c)) {
        if out.len().saturating_add(c.len_utf8()) > MAX_INFO_BYTES {
            cut = true;
            break;
        }
        out.push(c);
    }
    (out.trim().to_owned(), cut)
}

/// The ticks after a tempo point `last` at which `dt` seconds have passed,
/// when the next point's tempo is `next` (a ramp from `last` reaches it
/// there). `None` for a time before `last` or one that is not a number.
fn seconds_after(last: TempoPoint, next: Bpm, dt: f64) -> Option<i64> {
    if !dt.is_finite() || dt < 0.0 {
        return None;
    }
    let b0 = last.bpm.as_f64();
    let b1 = next.as_f64();
    let ppq = PPQ as f64;
    let ticks = if last.ramp && (b1 - b0).abs() > 1e-9 {
        // A linear ramp over L ticks lasts 60 L ln(b1/b0) / (PPQ (b1 − b0)).
        dt * ppq * (b1 - b0) / (60.0 * (b1 / b0).ln())
    } else {
        dt * ppq * b0 / 60.0
    };
    let t = ticks.round();
    (t.is_finite() && t <= MAX_TICKS as f64).then_some(t as i64)
}

/// Cuts the bus outputs and sends that would make a loop: one walk, depth
/// first and without recursion, removes every edge that closes a cycle
/// (what is left has none).
fn cut_loops(plan: &mut Plan, report: &mut Report) {
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
                }
                report.add(ItemKind::LeftOut, LOOP_SENDS, Some(b.name.clone()));
            }
        }
    }
}

// ---------------------------------------------------------------- applying

fn run(h: &mut History, cmd: Command) -> Result<CmdReport, ModelError> {
    h.execute(&cmd, &ViewFilter::default())
}

fn created<T>(r: &CmdReport, f: impl Fn(&AnyId) -> Option<T>) -> Option<T> {
    r.created.iter().find_map(f)
}

fn created_plugin(r: &CmdReport) -> Result<PluginInstanceId, ModelError> {
    created(r, |id| match id {
        AnyId::Plugin(p) => Some(*p),
        _ => None,
    })
    .ok_or_else(|| ModelError::invalid("no plug-in was created"))
}

fn strip_of(h: &History, kind: StripKind) -> Result<StripId, ModelError> {
    h.doc()
        .mixer()
        .find_kind(kind)
        .ok_or_else(|| ModelError::invalid("the strip was not created"))
}

/// Applies `mix` to `strip`; with `flags`, its mute and solo too; on the
/// master, no output and no sends. New slots with a state join `states`.
fn apply_mix(
    h: &mut History,
    strip: StripId,
    mix: &Mix,
    buses: &[StripId],
    flags: bool,
    master: bool,
    states: &mut SlotStates,
) -> Result<(), ModelError> {
    let mixer = |cmd| Command::Mixer(cmd);
    if mix.gain_db != 0.0 {
        run(
            h,
            mixer(MixerCmd::SetGain {
                strip,
                gain_db: mix.gain_db,
            }),
        )?;
    }
    if mix.pan != 0.0 && !master {
        run(
            h,
            mixer(MixerCmd::SetPan {
                strip,
                pan: mix.pan,
            }),
        )?;
    }
    if flags && mix.mute {
        run(h, mixer(MixerCmd::SetMute { strip, mute: true }))?;
    }
    if flags && mix.solo && !master {
        run(h, mixer(MixerCmd::SetSolo { strip, solo: true }))?;
    }
    if mix.colour.is_some() {
        run(
            h,
            mixer(MixerCmd::SetColour {
                strip,
                colour: mix.colour,
            }),
        )?;
    }
    if !master {
        if let Out::Bus(b) = mix.output
            && let Some(bus) = buses.get(b).copied()
        {
            run(
                h,
                mixer(MixerCmd::SetOutput {
                    strip,
                    output: Output::Bus(bus),
                }),
            )?;
        }
        for s in &mix.sends {
            let Some(target) = buses.get(s.bus).copied() else {
                continue;
            };
            run(
                h,
                mixer(MixerCmd::AddSend {
                    strip,
                    send: AuxSend {
                        target,
                        level_db: s.level_db,
                        pre_fader: s.pre,
                        enabled: s.enabled,
                    },
                }),
            )?;
        }
    }
    let mut slot_done = |h: &mut History, id: PluginInstanceId, plan: &SlotPlan| {
        if plan.bypass {
            run(
                h,
                mixer(MixerCmd::SetBypass {
                    slot: id,
                    bypass: true,
                }),
            )?;
        }
        if let Some(s) = &plan.state {
            states.push((id, Arc::clone(s)));
        }
        Ok::<(), ModelError>(())
    };
    if let Some(eq) = &mix.eq {
        let r = run(
            h,
            mixer(MixerCmd::SetEq {
                strip,
                plugin: Some(eq.plugin.clone()),
            }),
        )?;
        let id = created_plugin(&r)?;
        slot_done(h, id, eq)?;
    }
    for ins in &mix.inserts {
        let r = run(
            h,
            mixer(MixerCmd::AddInsert {
                strip,
                index: None,
                plugin: ins.plugin.clone(),
            }),
        )?;
        let id = created_plugin(&r)?;
        slot_done(h, id, ins)?;
    }
    Ok(())
}

/// The plug-in slots `mix` makes.
fn slots(mix: &Mix) -> u64 {
    u64::try_from(mix.inserts.len())
        .unwrap_or(u64::MAX)
        .saturating_add(u64::from(mix.eq.is_some()))
}

/// Inside the caller's open transaction on a document with no tracks;
/// returns each new plugin slot's state to capture after the commit.
///
/// Makes, in order: the buses, the shared built-in-synth destination when a
/// track needs it, each track (its destination or plug-in instrument, its
/// output path, its strips' settings and, on the shared synth, its mute
/// and solo), the synth's, the buses' and the master's settings, then the
/// tempo and meter maps, the markers, the sequence's name and the song
/// info. Merges the import's report into `report` and fills its summary.
pub fn apply(
    h: &mut History,
    imp: &DpImport,
    seq: SeqId,
    report: &mut Report,
) -> Result<SlotStates, ModelError> {
    let doc = h.doc();
    doc.sequence(seq)?;
    if doc.sequences().values().any(|s| !s.lanes.is_empty()) {
        return Err(ModelError::invalid(NOT_NEW));
    }
    report.merge(imp.report.clone());
    let plan = &imp.plan;
    let mut states = Vec::new();
    let mut plugins = 0u64;

    let mut buses = Vec::with_capacity(plan.buses.len());
    for b in &plan.buses {
        let r = run(
            h,
            Command::Mixer(MixerCmd::AddBus {
                name: b.name.clone(),
                index: None,
            }),
        )?;
        let id = created(&r, |id| match id {
            AnyId::Strip(s) => Some(*s),
            _ => None,
        })
        .ok_or_else(|| ModelError::invalid("no bus was created"))?;
        buses.push(id);
    }

    let mut builtin: Option<DestId> = None;
    if plan.tracks.iter().any(|t| t.midi == Some(Midi::Builtin)) {
        let r = run(
            h,
            Command::Routing(RoutingCmd::AddDestination {
                name: BUILTIN_NAME.to_owned(),
                kind: DestKind::MidiEndpoint {
                    endpoint_ref: BUILTIN_SYNTH_REF.to_owned(),
                    group: 0,
                    channel: 0,
                },
                default_patch: None,
            }),
        )?;
        builtin = created(&r, |id| match id {
            AnyId::Dest(d) => Some(*d),
            _ => None,
        });
    }

    let mut lanes = 0u64;
    for t in &plan.tracks {
        let r = run(
            h,
            Command::Lane(LaneCmd::Add {
                seq,
                name: t.name.clone(),
                index: None,
            }),
        )?;
        let lane = created(&r, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })
        .ok_or_else(|| ModelError::invalid("no track was created"))?;
        lanes = lanes.saturating_add(1);
        let mut owned: Vec<StripId> = Vec::new();
        match &t.midi {
            Some(Midi::Plugin(slot)) => {
                let mut s = PluginSlot::new(PluginInstanceId(0), slot.plugin.clone());
                s.bypass = slot.bypass;
                let r = run(
                    h,
                    Command::Routing(RoutingCmd::AddDestination {
                        name: t.name.clone(),
                        kind: DestKind::PluginInstrument { slot: s },
                        default_patch: None,
                    }),
                )?;
                let dest = created(&r, |id| match id {
                    AnyId::Dest(d) => Some(*d),
                    _ => None,
                })
                .ok_or_else(|| ModelError::invalid("no destination was created"))?;
                let id = created_plugin(&r)?;
                if let Some(st) = &slot.state {
                    states.push((id, Arc::clone(st)));
                }
                plugins = plugins.saturating_add(1);
                run(
                    h,
                    Command::Lane(LaneCmd::SetDest {
                        seq,
                        lane,
                        dest: Some(dest),
                    }),
                )?;
                owned.push(strip_of(h, StripKind::Instrument(dest))?);
            }
            Some(Midi::Builtin) => {
                run(
                    h,
                    Command::Lane(LaneCmd::SetDest {
                        seq,
                        lane,
                        dest: builtin,
                    }),
                )?;
            }
            None => {}
        }
        if t.audio {
            let r = run(
                h,
                Command::Routing(RoutingCmd::AddOutput {
                    name: t.name.clone(),
                    reference: AUDIO_REFERENCE.to_owned(),
                }),
            )?;
            let out = created(&r, |id| match id {
                AnyId::Output(o) => Some(*o),
                _ => None,
            })
            .ok_or_else(|| ModelError::invalid("no output path was created"))?;
            run(
                h,
                Command::Lane(LaneCmd::SetOutput {
                    seq,
                    lane,
                    output: Some(out),
                }),
            )?;
            owned.push(strip_of(h, StripKind::AudioPath(out))?);
        }
        for strip in owned {
            apply_mix(
                h,
                strip,
                &t.mix,
                &buses,
                t.flags.is_none(),
                false,
                &mut states,
            )?;
            plugins = plugins.saturating_add(slots(&t.mix));
        }
        if let Some((mute, solo)) = t.flags
            && (mute || solo)
        {
            run(
                h,
                Command::Lane(LaneCmd::SetFlags {
                    seq,
                    lane,
                    mute: mute.then_some(true),
                    solo: solo.then_some(true),
                    arm: None,
                }),
            )?;
        }
    }
    if let (Some(mix), Some(_)) = (&plan.builtin, builtin) {
        let strip = strip_of(h, StripKind::BuiltinSynth)?;
        apply_mix(h, strip, mix, &buses, false, false, &mut states)?;
        plugins = plugins.saturating_add(slots(mix));
    }
    for (b, id) in plan.buses.iter().zip(buses.clone()) {
        apply_mix(h, id, &b.mix, &buses, true, false, &mut states)?;
        plugins = plugins.saturating_add(slots(&b.mix));
    }
    let master = h.doc().mixer().master();
    apply_mix(h, master, &plan.master, &buses, true, true, &mut states)?;
    plugins = plugins.saturating_add(slots(&plan.master));

    run(
        h,
        Command::Sequence(SequenceCmd::SetTempo {
            seq,
            tempo: plan.tempo.clone(),
        }),
    )?;
    run(
        h,
        Command::Sequence(SequenceCmd::SetMeter {
            seq,
            meter: plan.meter.clone(),
        }),
    )?;
    if !plan.markers.is_empty() {
        run(
            h,
            Command::Sequence(SequenceCmd::SetMarkers {
                seq,
                markers: plan.markers.iter().cloned().collect(),
            }),
        )?;
    }
    run(
        h,
        Command::Sequence(SequenceCmd::Rename {
            seq,
            name: plan.name.clone(),
        }),
    )?;
    if !plan.info.is_empty() {
        run(
            h,
            Command::Settings(SettingsCmd::SetInfo {
                info: plan.info.clone(),
            }),
        )?;
    }
    let s = &mut report.summary;
    s.tracks = s.tracks.saturating_add(lanes);
    s.markers = s
        .markers
        .saturating_add(u64::try_from(plan.markers.len()).unwrap_or(u64::MAX));
    s.plugins = s.plugins.saturating_add(plugins);
    s.tempo_map = true;
    Ok(states)
}
