//! Playback targets: where scheduled events go, and how 2a destinations map
//! onto them.

use crate::backend::MidiPortInfo;
use crate::error::EngineError;
use core_model::document::DestKind;

/// Most targets one engine can drive.
pub const MAX_TARGETS: usize = 16;

/// Index into a [`TargetTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetIdx(pub u8);

/// What a target is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TargetKind {
    /// The built-in synth (one instance, all 16 channels).
    BuiltinSynth,
    /// VizDAW's own CoreMIDI virtual source, "VizDAW Out".
    MidiVirtualSource,
    /// An existing MIDI destination.
    MidiDestination(MidiPortInfo),
}

/// Endpoint reference that selects the built-in synth.
pub const BUILTIN_SYNTH_REF: &str = "builtin:synth";
/// Endpoint reference that selects the virtual source.
pub const VIRTUAL_OUT_REF: &str = "virtual:out";

/// Where `builtin:synth` destinations play (CLI `--to`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BuiltinRoute {
    /// The built-in synth.
    #[default]
    Builtin,
    /// The virtual source instead.
    Virtual,
    /// Both.
    Both,
}

/// The targets of an engine, in creation order. Targets are only ever
/// appended (a failed load rolls back the ones it added before any sink
/// reached the renderer), so an index stays valid for the engine's lifetime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetTable {
    kinds: Vec<TargetKind>,
}

impl TargetTable {
    /// An empty table.
    pub fn new() -> TargetTable {
        TargetTable::default()
    }

    /// The index of `kind`, if present.
    pub fn find(&self, kind: &TargetKind) -> Option<TargetIdx> {
        self.kinds
            .iter()
            .position(|k| k == kind)
            .and_then(|i| u8::try_from(i).ok())
            .map(TargetIdx)
    }

    /// Adds `kind` (or returns its existing index).
    pub fn intern(&mut self, kind: TargetKind) -> Result<TargetIdx, EngineError> {
        if let Some(i) = self.find(&kind) {
            return Ok(i);
        }
        if self.kinds.len() >= MAX_TARGETS {
            return Err(EngineError::TooManyTargets);
        }
        let i = u8::try_from(self.kinds.len()).map_err(|_| EngineError::TooManyTargets)?;
        self.kinds.push(kind);
        Ok(TargetIdx(i))
    }

    /// Drops targets added after the table had `len` entries (rollback of
    /// a failed load whose new targets never got sinks).
    pub(crate) fn truncate(&mut self, len: usize) {
        self.kinds.truncate(len);
    }

    /// The kind at `idx`.
    pub fn get(&self, idx: TargetIdx) -> Option<&TargetKind> {
        self.kinds.get(usize::from(idx.0))
    }

    /// Number of targets.
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    /// True when there are no targets.
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// `(index, kind)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (TargetIdx, &TargetKind)> + '_ {
        self.kinds
            .iter()
            .enumerate()
            .filter_map(|(i, k)| u8::try_from(i).ok().map(|i| (TargetIdx(i), k)))
    }
}

/// The target kinds a destination plays on (up to two, for
/// [`BuiltinRoute::Both`]), or why it cannot play.
pub fn kinds_for(
    kind: &DestKind,
    route: BuiltinRoute,
    ports: &[MidiPortInfo],
) -> Result<Vec<TargetKind>, String> {
    match kind {
        DestKind::PluginInstrument { plugin_ref } => Err(format!(
            "plugin instrument {plugin_ref:?} cannot play until plugin hosting exists"
        )),
        DestKind::MidiEndpoint { endpoint_ref, .. } => match endpoint_ref.as_str() {
            BUILTIN_SYNTH_REF => Ok(match route {
                BuiltinRoute::Builtin => vec![TargetKind::BuiltinSynth],
                BuiltinRoute::Virtual => vec![TargetKind::MidiVirtualSource],
                BuiltinRoute::Both => {
                    vec![TargetKind::BuiltinSynth, TargetKind::MidiVirtualSource]
                }
            }),
            VIRTUAL_OUT_REF => Ok(vec![TargetKind::MidiVirtualSource]),
            name => find_port(name, ports)
                .map(|p| vec![TargetKind::MidiDestination(p.clone())])
                .ok_or_else(|| format!("no MIDI destination named {name:?}")),
        },
    }
}

/// Finds a destination by display name, then by unique ID written as a
/// decimal number.
pub fn find_port<'a>(name: &str, ports: &'a [MidiPortInfo]) -> Option<&'a MidiPortInfo> {
    ports.iter().find(|p| p.name == name).or_else(|| {
        let id: i32 = name.parse().ok()?;
        ports.iter().find(|p| p.unique_id == Some(id))
    })
}
