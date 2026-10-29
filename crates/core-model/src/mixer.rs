//! The mixer model (spec §4): strips, plugin slots, sends and outputs.
//! Routing rules live in [`crate::mixer_rules`]; edits in
//! `crate::command::mixer`.

use crate::automation::AutomationMode;
use crate::document::{DestKind, Destination, OutputPath};
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, Counters, DestId, OutputPathId, PluginInstanceId, StateBlobId, StripId};
use im::{OrdMap, Vector};
use serde::{Deserialize, Serialize};

/// Endpoint reference that selects Apple's built-in synth (engine spec §5).
pub const BUILTIN_SYNTH_REF: &str = "builtin:synth";
/// Fader and send levels at or below this are silence (−∞ dB).
pub const GAIN_FLOOR_DB: f64 = -144.0;
/// Highest fader and send level.
pub const GAIN_MAX_DB: f64 = 12.0;
/// Name of the master strip.
pub const MASTER_NAME: &str = "Master";
/// Name of the built-in synth's strip.
pub const BUILTIN_SYNTH_STRIP: &str = "Built-in Synth";

/// A plugin parameter ID (CLAP `clap_id`, AU parameter address).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParamId(pub u64);

/// Plugin formats VizDAW hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginFormat {
    /// Audio Unit (v2 or v3).
    Au,
    /// CLAP.
    Clap,
}

/// Which plugin a slot holds. Loading matches on `format` + `id` only; a
/// version change is reported, not refused.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRef {
    /// Format.
    pub format: PluginFormat,
    /// AU: `type/subtype/manufacturer` four-char codes; CLAP: plugin id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Vendor.
    pub vendor: String,
    /// Version as the plugin reports it.
    pub version: String,
}

impl PluginRef {
    /// The reference a version-1 `plugin_ref` string becomes: format `Au`
    /// for an `au:` prefix, else `Clap`; `id` and `name` are the whole
    /// string; vendor and version are empty (Plan Ruling 7).
    pub fn from_legacy(s: &str) -> PluginRef {
        PluginRef {
            format: if s.starts_with("au:") {
                PluginFormat::Au
            } else {
                PluginFormat::Clap
            },
            id: s.to_string(),
            name: s.to_string(),
            vendor: String::new(),
            version: String::new(),
        }
    }
}

/// A plugin instance: an insert, or a plugin instrument's own slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSlot {
    /// Stable instance ID.
    pub id: PluginInstanceId,
    /// The plugin.
    pub plugin: PluginRef,
    /// Bypassed; a bypassed insert keeps its latency.
    pub bypass: bool,
    /// Strip whose post-fader output feeds this insert's sidechain input.
    pub sidechain: Option<StripId>,
    /// The last captured state blob.
    pub state: Option<StateBlobId>,
    /// Values changed through VizDAW since the last capture (the overlay).
    #[serde(with = "param_list")]
    pub params: OrdMap<ParamId, f64>,
}

impl PluginSlot {
    /// An active slot with no sidechain, state or overlay.
    pub fn new(id: PluginInstanceId, plugin: PluginRef) -> PluginSlot {
        PluginSlot {
            id,
            plugin,
            bypass: false,
            sidechain: None,
            state: None,
            params: OrdMap::new(),
        }
    }
}

/// The overlay serializes as `[[param, value], …]` ordered by param, so
/// JSON object keys never have to be numbers.
mod param_list {
    use super::ParamId;
    use im::OrdMap;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(m: &OrdMap<ParamId, f64>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(m.iter().map(|(k, v)| (*k, *v)))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<OrdMap<ParamId, f64>, D::Error> {
        let v: Vec<(ParamId, f64)> = Vec::deserialize(d)?;
        Ok(v.into_iter().collect())
    }
}

/// What feeds a strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StripKind {
    /// The main output of a plugin instrument destination.
    Instrument(DestId),
    /// Extra output `n` (≥ 1) of a multi-output plugin instrument.
    InstrumentOutput(DestId, u16),
    /// The built-in synth: an instrument strip whose instrument is built in
    /// (one per document, Plan Ruling 1).
    BuiltinSynth,
    /// An audio output path (silent source until sub-project 5).
    AudioPath(OutputPathId),
    /// An aux bus.
    Bus,
    /// The master.
    Master,
}

impl StripKind {
    /// True for strips fed by an instrument's main output.
    pub fn is_instrument(self) -> bool {
        matches!(self, StripKind::Instrument(_) | StripKind::BuiltinSynth)
    }
}

/// Where a strip's main output goes. Master itself always plays on device
/// channels 1–2 and keeps `Output::Master`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    /// The master strip.
    Master,
    /// An aux bus.
    Bus(StripId),
}

/// An aux send (the spec's `Send`, Plan Ruling 3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuxSend {
    /// The bus it feeds.
    pub target: StripId,
    /// Level in dB (`GAIN_FLOOR_DB` … `GAIN_MAX_DB`).
    pub level_db: f64,
    /// Tapped before the fader (after the inserts) instead of after it.
    pub pre_fader: bool,
    /// Off sends keep their settings but carry nothing.
    pub enabled: bool,
}

impl AuxSend {
    /// A post-fader send at 0 dB, enabled.
    pub fn new(target: StripId) -> AuxSend {
        AuxSend {
            target,
            level_db: 0.0,
            pre_fader: false,
            enabled: true,
        }
    }
}

/// A mixer channel strip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Strip {
    /// Strip ID.
    pub id: StripId,
    /// Display name.
    pub name: String,
    /// What feeds it.
    pub kind: StripKind,
    /// Insert chain, processed in order.
    pub inserts: Vector<PluginSlot>,
    /// Fader in dB (`GAIN_FLOOR_DB` = −∞ … `GAIN_MAX_DB`).
    pub gain_db: f64,
    /// Pan −1 … +1 (constant power; ignored on master).
    pub pan: f64,
    /// Explicit mute (also silences its sidechain feed).
    pub mute: bool,
    /// Solo.
    pub solo: bool,
    /// Aux sends.
    pub sends: Vector<AuxSend>,
    /// Main output.
    pub output: Output,
    /// Its automation mode (sub-project 5 spec §4.6); not written when
    /// Read, so all-Read projects save as before (plan 5e-1 Ruling 2).
    #[serde(default, skip_serializing_if = "AutomationMode::is_read")]
    pub automation: AutomationMode,
}

impl Strip {
    /// A strip at 0 dB, centred, unmuted, with no inserts or sends, to
    /// master.
    pub fn new(id: StripId, name: impl Into<String>, kind: StripKind) -> Strip {
        Strip {
            id,
            name: name.into(),
            kind,
            inserts: Vector::new(),
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            sends: Vector::new(),
            output: Output::Master,
            automation: AutomationMode::Read,
        }
    }
}

/// The mixer: strips by ID, display order and the master. Change it with
/// commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "MixerRepr", try_from = "MixerRepr")]
pub struct Mixer {
    strips: OrdMap<StripId, Strip>,
    order: Vector<StripId>,
    master: StripId,
}

/// The serialized shape of a [`Mixer`]: strips ordered by ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixerRepr {
    /// The master strip's ID.
    pub master: StripId,
    /// Display order of every strip except master.
    pub order: Vec<StripId>,
    /// Every strip, master included.
    pub strips: Vec<Strip>,
}

impl From<Mixer> for MixerRepr {
    fn from(m: Mixer) -> MixerRepr {
        MixerRepr {
            master: m.master,
            order: m.order.iter().copied().collect(),
            strips: m.strips.values().cloned().collect(),
        }
    }
}

impl TryFrom<MixerRepr> for Mixer {
    type Error = String;
    fn try_from(r: MixerRepr) -> std::result::Result<Mixer, String> {
        let mut strips = OrdMap::new();
        for s in r.strips {
            let id = s.id;
            if strips.insert(id, s).is_some() {
                return Err(format!("duplicate strip id {id}"));
            }
        }
        Ok(Mixer {
            strips,
            order: r.order.into_iter().collect(),
            master: r.master,
        })
    }
}

impl Mixer {
    /// A mixer holding only the master strip `master`.
    pub fn new(master: StripId) -> Mixer {
        Mixer {
            strips: OrdMap::unit(master, Strip::new(master, MASTER_NAME, StripKind::Master)),
            order: Vector::new(),
            master,
        }
    }

    /// Strips by ID.
    pub fn strips(&self) -> &OrdMap<StripId, Strip> {
        &self.strips
    }

    /// Display order (every strip except master).
    pub fn order(&self) -> &Vector<StripId> {
        &self.order
    }

    /// The master strip's ID.
    pub fn master(&self) -> StripId {
        self.master
    }

    /// A strip by ID.
    pub fn strip(&self, id: StripId) -> Result<&Strip> {
        self.strips
            .get(&id)
            .ok_or(ModelError::UnknownId(AnyId::Strip(id)))
    }

    /// The first strip of `kind`, by ID.
    pub fn find_kind(&self, kind: StripKind) -> Option<StripId> {
        self.strips.values().find(|s| s.kind == kind).map(|s| s.id)
    }

    pub(crate) fn strip_mut(&mut self, id: StripId) -> Result<&mut Strip> {
        self.strips
            .get_mut(&id)
            .ok_or(ModelError::UnknownId(AnyId::Strip(id)))
    }

    /// Every insert as `(strip, position, slot)`, by strip ID then position.
    pub fn inserts(&self) -> impl Iterator<Item = (StripId, usize, &PluginSlot)> + '_ {
        self.strips
            .values()
            .flat_map(|s| s.inserts.iter().enumerate().map(move |(i, p)| (s.id, i, p)))
    }

    /// Adds `strip` at display position `index` (default: the end).
    pub(crate) fn add(&mut self, strip: Strip, index: Option<usize>) {
        let id = strip.id;
        self.strips.insert(id, strip);
        let i = index.unwrap_or(self.order.len()).min(self.order.len());
        self.order.insert(i, id);
    }

    /// Moves a strip to display position `index` (clamped).
    pub(crate) fn move_to(&mut self, id: StripId, index: usize) {
        self.order.retain(|x| *x != id);
        let i = index.min(self.order.len());
        self.order.insert(i, id);
    }

    /// Removes a strip (never master): outputs to it go to master, sends to
    /// it are dropped and sidechains from it are cleared.
    pub(crate) fn remove(&mut self, id: StripId) -> Option<Strip> {
        if id == self.master {
            return None;
        }
        let s = self.strips.remove(&id)?;
        self.order.retain(|x| *x != id);
        let ids: Vec<StripId> = self.strips.keys().copied().collect();
        for k in ids {
            if let Some(st) = self.strips.get_mut(&k) {
                if st.output == Output::Bus(id) {
                    st.output = Output::Master;
                }
                st.sends.retain(|x| x.target != id);
                for slot in st.inserts.iter_mut() {
                    if slot.sidechain == Some(id) {
                        slot.sidechain = None;
                    }
                }
            }
        }
        Some(s)
    }
}

/// Linear gain of a level in dB; at or below [`GAIN_FLOOR_DB`] it is 0.
pub fn db_to_gain(db: f64) -> f64 {
    if db <= GAIN_FLOOR_DB {
        0.0
    } else {
        10f64.powf(db / 20.0)
    }
}

/// Constant-power pan gains `(left, right)`: `θ = (pan + 1)·π/4`, gains
/// `cos θ`, `sin θ` (−3 dB each at centre). `pan` is clamped to ±1.
pub fn pan_gains(pan: f64) -> (f64, f64) {
    let t = (pan.clamp(-1.0, 1.0) + 1.0) * std::f64::consts::FRAC_PI_4;
    (t.cos(), t.sin())
}

/// The mixer a document without one gets (Plan Ruling 6): master; then,
/// in destination order, an `Instrument` strip per plugin instrument and
/// the built-in synth strip at the first `builtin:synth` destination; then
/// an `AudioPath` strip per output path. Everything routes to master.
pub fn derive_mixer<'a>(
    instruments: impl IntoIterator<Item = &'a Destination>,
    outputs: impl IntoIterator<Item = &'a OutputPath>,
    counters: &mut Counters,
) -> Mixer {
    let mut m = Mixer::new(counters.alloc_strip());
    let mut builtin = false;
    for d in instruments {
        match &d.kind {
            DestKind::PluginInstrument { .. } => m.add(
                Strip::new(
                    counters.alloc_strip(),
                    d.name.clone(),
                    StripKind::Instrument(d.id),
                ),
                None,
            ),
            DestKind::MidiEndpoint { endpoint_ref, .. }
                if endpoint_ref == BUILTIN_SYNTH_REF && !builtin =>
            {
                builtin = true;
                m.add(
                    Strip::new(
                        counters.alloc_strip(),
                        BUILTIN_SYNTH_STRIP,
                        StripKind::BuiltinSynth,
                    ),
                    None,
                );
            }
            DestKind::MidiEndpoint { .. } => {}
        }
    }
    for o in outputs {
        m.add(
            Strip::new(
                counters.alloc_strip(),
                o.name.clone(),
                StripKind::AudioPath(o.id),
            ),
            None,
        );
    }
    m
}
