//! The mixer read model (spec §11): strips with their slots, sends and
//! routing, and the levels the mixer window polls at 30 Hz.

use crate::plugins::slot_row;
use crate::snapshot::SlotRow;
use core_host::PluginHost;
use core_model::document::{DestKind, Document};
use core_model::mixer::{Mixer, Output, Strip, StripKind, Swatch};
use core_model::mixer_rules::audible;
use std::collections::BTreeSet;

/// What feeds a strip.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum StripKindRow {
    /// A plugin instrument's main output.
    Instrument {
        /// Its destination.
        dest: u64,
    },
    /// Extra output `index` (≥ 1) of a plugin instrument.
    InstrumentOutput {
        /// Its destination.
        dest: u64,
        /// Output index.
        index: u16,
    },
    /// The built-in synth.
    BuiltinSynth,
    /// An audio output path (silent until sub-project 5).
    AudioPath {
        /// The output path.
        path: u64,
    },
    /// An aux bus.
    Bus,
    /// The master.
    Master,
}

/// The colour a strip's kind gives it (plan 7b Ruling 10, spec §2 item
/// 8). Fader caps always take it; the colour bar takes it when the strip
/// has no colour of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum StripColourRow {
    /// An audio path's channel.
    Channel,
    /// An instrument: the built-in synth, a plugin instrument or one of
    /// its extra outputs.
    Instrument,
    /// A bus fed only by sends.
    AuxReturn,
    /// Any other bus (one a strip's output feeds, or one with no feed).
    Bus,
    /// The master.
    Master,
}

/// A strip's own colour (controller ruling C-1, plan 7b Ruling 12), one
/// to one with `core_model::mixer::Swatch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SwatchRow {
    /// Red.
    Red,
    /// Orange.
    Orange,
    /// Yellow.
    Yellow,
    /// Green.
    Green,
    /// Teal.
    Teal,
    /// Blue.
    Blue,
    /// Purple.
    Purple,
    /// Pink.
    Pink,
}

impl From<Swatch> for SwatchRow {
    fn from(s: Swatch) -> Self {
        match s {
            Swatch::Red => SwatchRow::Red,
            Swatch::Orange => SwatchRow::Orange,
            Swatch::Yellow => SwatchRow::Yellow,
            Swatch::Green => SwatchRow::Green,
            Swatch::Teal => SwatchRow::Teal,
            Swatch::Blue => SwatchRow::Blue,
            Swatch::Purple => SwatchRow::Purple,
            Swatch::Pink => SwatchRow::Pink,
        }
    }
}

impl From<SwatchRow> for Swatch {
    fn from(s: SwatchRow) -> Self {
        match s {
            SwatchRow::Red => Swatch::Red,
            SwatchRow::Orange => Swatch::Orange,
            SwatchRow::Yellow => Swatch::Yellow,
            SwatchRow::Green => Swatch::Green,
            SwatchRow::Teal => Swatch::Teal,
            SwatchRow::Blue => Swatch::Blue,
            SwatchRow::Purple => Swatch::Purple,
            SwatchRow::Pink => Swatch::Pink,
        }
    }
}

/// The colour `s`'s kind gives it in `m` (plan 7b Ruling 10): a bus is a
/// bus when any strip's output goes to it, else an aux return when a send
/// feeds it, else (no feed at all) a bus.
pub fn strip_colour(m: &Mixer, s: &Strip) -> StripColourRow {
    match s.kind {
        StripKind::Master => StripColourRow::Master,
        StripKind::Instrument(_) | StripKind::InstrumentOutput(..) | StripKind::BuiltinSynth => {
            StripColourRow::Instrument
        }
        StripKind::AudioPath(_) => StripColourRow::Channel,
        StripKind::Bus => {
            let all = || m.order().iter().filter_map(|id| m.strip(*id).ok());
            if all().any(|x| x.output == Output::Bus(s.id)) {
                StripColourRow::Bus
            } else if all().any(|x| x.sends.iter().any(|t| t.target == s.id)) {
                StripColourRow::AuxReturn
            } else {
                StripColourRow::Bus
            }
        }
    }
}

/// An aux send.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SendRow {
    /// The bus it feeds.
    pub target: u64,
    /// Level in dB (−144 is −∞).
    pub level_db: f64,
    /// Tapped before the fader.
    pub pre_fader: bool,
    /// On.
    pub enabled: bool,
}

/// One mixer strip.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct StripRow {
    /// Strip ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// What feeds it.
    pub kind: StripKindRow,
    /// "Bus", "Master", "Built-in Synth", the instrument, "‹instrument›
    /// out 3" or "Audio: ‹output›".
    pub kind_label: String,
    /// The plugin instrument's own slot (instrument strips only).
    pub instrument: Option<SlotRow>,
    /// Inserts, in processing order.
    pub inserts: Vec<SlotRow>,
    /// Fader in dB (−144 is −∞).
    pub gain_db: f64,
    /// Pan −1 … +1 (not applied on master).
    pub pan: f64,
    /// Muted.
    pub mute: bool,
    /// Soloed.
    pub solo: bool,
    /// Silent because other strips are soloed.
    pub solo_muted: bool,
    /// Sends.
    pub sends: Vec<SendRow>,
    /// The bus its output goes to; `None` is master (and master itself).
    pub output: Option<u64>,
    /// Outputs of its multi-output instrument that have no strip yet
    /// ("Add Outputs…").
    pub free_outputs: Vec<u16>,
    /// Its automation mode (sub-project 5 spec §4.6).
    pub automation: crate::automation::AutomationModeRow,
    /// The EQ slot, which processes before the inserts (plan 7b Ruling
    /// 7); `None` when empty.
    pub eq: Option<SlotRow>,
    /// The colour its kind gives it (Ruling 10): fader caps always take
    /// it.
    pub kind_colour: StripColourRow,
    /// Its own colour, if any (C-1): the colour bar takes it when set.
    pub colour: Option<SwatchRow>,
}

/// A destination of the instrument list (lanes play through these).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DestRow {
    /// Destination ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// The plugin instrument it plays, if it is one.
    pub plugin: Option<crate::snapshot::PluginRefRow>,
    /// An external MIDI destination: Track Synth and Measure work with it
    /// (spec §6.7; plan 5c-2 Ruling 2).
    #[uniffi(default)]
    pub external: bool,
    /// Its return latency in ms (spec §4.5a).
    #[uniffi(default)]
    pub return_latency_ms: f32,
}

/// A bus, for output and send menus.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BusRow {
    /// Strip ID.
    pub id: u64,
    /// Name.
    pub name: String,
}

/// The whole mixer.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MixerSnapshot {
    /// The document version it was read at.
    pub version: u64,
    /// Strips in display order, master last.
    pub strips: Vec<StripRow>,
    /// The buses, in display order.
    pub buses: Vec<BusRow>,
}

/// One strip's peaks since the last read (linear, 1.0 is full scale).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct MeterRow {
    /// Strip ID.
    pub strip: u64,
    /// Left peak.
    pub peak_l: f32,
    /// Right peak.
    pub peak_r: f32,
}

/// What the mixer window polls (spec §7: 30 Hz). Empty for a session that
/// does not hold the engine.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MixerLevels {
    /// Peaks per strip.
    pub meters: Vec<MeterRow>,
    /// DSP load of the last block (render time ÷ block time).
    pub dsp_load: f32,
    /// Peak DSP load since the last read.
    pub dsp_peak: f32,
    /// Plugin delay compensation of the graph, in frames.
    pub latency_frames: u64,
    /// Instances bypassed for faults (NaN output, errors), sticky until
    /// the engine is rebuilt.
    pub faulted: Vec<u64>,
}

impl MixerLevels {
    /// Nothing to show.
    pub fn empty() -> MixerLevels {
        MixerLevels {
            meters: Vec::new(),
            dsp_load: 0.0,
            dsp_peak: 0.0,
            latency_frames: 0,
            faulted: Vec::new(),
        }
    }
}

fn kind_row(k: StripKind) -> StripKindRow {
    match k {
        StripKind::Instrument(d) => StripKindRow::Instrument { dest: d.0 },
        StripKind::InstrumentOutput(d, i) => StripKindRow::InstrumentOutput {
            dest: d.0,
            index: i,
        },
        StripKind::BuiltinSynth => StripKindRow::BuiltinSynth,
        StripKind::AudioPath(p) => StripKindRow::AudioPath { path: p.0 },
        StripKind::Bus => StripKindRow::Bus,
        StripKind::Master => StripKindRow::Master,
    }
}

fn kind_label(doc: &Document, k: StripKind) -> String {
    let dest = |d| {
        doc.instruments()
            .get(&d)
            .map_or_else(|| d.to_string(), |x| x.name.clone())
    };
    match k {
        StripKind::Instrument(d) => dest(d),
        StripKind::InstrumentOutput(d, i) => format!("{} out {}", dest(d), u32::from(i) + 1),
        StripKind::BuiltinSynth => "Built-in Synth".into(),
        StripKind::AudioPath(p) => format!(
            "Audio: {}",
            doc.outputs()
                .get(&p)
                .map_or_else(|| p.to_string(), |o| o.name.clone())
        ),
        StripKind::Bus => "Bus".into(),
        StripKind::Master => "Master".into(),
    }
}

fn strip_row(
    doc: &Document,
    s: &Strip,
    heard: &BTreeSet<core_model::ids::StripId>,
    host: &mut Option<&mut PluginHost>,
) -> StripRow {
    let instrument = match s.kind {
        StripKind::Instrument(d) => match doc.instruments().get(&d).map(|x| &x.kind) {
            Some(DestKind::PluginInstrument { slot }) => {
                Some(slot_row(slot, true, host.as_deref_mut()))
            }
            _ => None,
        },
        _ => None,
    };
    let free_outputs = match (s.kind, &instrument) {
        (StripKind::Instrument(d), Some(slot)) => (1..slot.outputs)
            .filter_map(|i| u16::try_from(i).ok())
            .filter(|i| {
                doc.mixer()
                    .find_kind(StripKind::InstrumentOutput(d, *i))
                    .is_none()
            })
            .collect(),
        _ => Vec::new(),
    };
    StripRow {
        id: s.id.0,
        name: s.name.clone(),
        kind: kind_row(s.kind),
        kind_label: kind_label(doc, s.kind),
        instrument,
        inserts: s
            .inserts
            .iter()
            .map(|slot| slot_row(slot, false, host.as_deref_mut()))
            .collect(),
        gain_db: s.gain_db,
        pan: s.pan,
        mute: s.mute,
        solo: s.solo,
        solo_muted: !heard.contains(&s.id),
        sends: s
            .sends
            .iter()
            .map(|x| SendRow {
                target: x.target.0,
                level_db: x.level_db,
                pre_fader: x.pre_fader,
                enabled: x.enabled,
            })
            .collect(),
        output: match s.output {
            Output::Master => None,
            Output::Bus(b) => Some(b.0),
        },
        free_outputs,
        automation: s.automation.into(),
        eq: s
            .eq
            .as_ref()
            .map(|slot| slot_row(slot, false, host.as_deref_mut())),
        kind_colour: strip_colour(doc.mixer(), s),
        colour: s.colour.map(SwatchRow::from),
    }
}

/// The mixer of `doc` at document `version`, with what `host` knows about
/// each plugin.
pub fn snapshot(doc: &Document, version: u64, mut host: Option<&mut PluginHost>) -> MixerSnapshot {
    let m = doc.mixer();
    let heard = audible(m);
    let mut strips: Vec<StripRow> = m
        .order()
        .iter()
        .filter_map(|id| m.strip(*id).ok())
        .map(|s| strip_row(doc, s, &heard, &mut host))
        .collect();
    if let Ok(master) = m.strip(m.master()) {
        strips.push(strip_row(doc, master, &heard, &mut host));
    }
    let buses = m
        .order()
        .iter()
        .filter_map(|id| m.strip(*id).ok())
        .filter(|s| s.kind == StripKind::Bus)
        .map(|s| BusRow {
            id: s.id.0,
            name: s.name.clone(),
        })
        .collect();
    MixerSnapshot {
        version,
        strips,
        buses,
    }
}

/// Every plugin slot ID of `doc` (instruments' own slots, and every
/// strip's chain: its EQ, then its inserts).
pub fn slot_ids(doc: &Document) -> BTreeSet<core_model::ids::PluginInstanceId> {
    let mut ids: BTreeSet<_> = doc.mixer().slots().map(|(_, s)| s.id).collect();
    for d in doc.instruments().values() {
        if let DestKind::PluginInstrument { slot } = &d.kind {
            ids.insert(slot.id);
        }
    }
    ids
}
