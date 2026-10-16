//! Playback targets: where scheduled events go, and how 2a destinations map
//! onto them.

use crate::backend::MidiPortInfo;
use crate::error::EngineError;
use core_model::document::DestKind;
use core_model::ids::PluginInstanceId;

/// Most targets one engine can drive at once (Plan Ruling 4: plugin
/// instruments are targets too).
pub const MAX_TARGETS: usize = 64;

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
    /// A plugin instrument destination's instance (its sink comes from the
    /// engine's plugin factory, `InsertFactory::make_instrument`).
    PluginInstrument(PluginInstanceId),
}

/// Endpoint reference that selects the built-in synth.
pub use core_model::mixer::BUILTIN_SYNTH_REF;
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

/// One entry of a [`TargetTable`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    /// In use.
    Live(TargetKind),
    /// Released by this load: its sink is being removed, and a schedule
    /// still queued may name it, so it is not reused before the next load.
    Released,
    /// Reusable.
    Free,
}

/// The targets of an engine. An index names one target from `intern`
/// until `release`; a released index is reused only from the next load on
/// (`recycle`), after every schedule that named it was replaced (Plan
/// Ruling 4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetTable {
    kinds: Vec<Entry>,
}

impl TargetTable {
    /// An empty table.
    pub fn new() -> TargetTable {
        TargetTable::default()
    }

    /// The index of live `kind`, if present.
    pub fn find(&self, kind: &TargetKind) -> Option<TargetIdx> {
        self.kinds
            .iter()
            .position(|k| matches!(k, Entry::Live(x) if x == kind))
            .and_then(|i| u8::try_from(i).ok())
            .map(TargetIdx)
    }

    /// The index the next new target gets, or `None` when the table is
    /// full.
    pub fn next_index(&self) -> Option<TargetIdx> {
        let i = self
            .kinds
            .iter()
            .position(|k| *k == Entry::Free)
            .unwrap_or(self.kinds.len());
        if i >= MAX_TARGETS {
            return None;
        }
        u8::try_from(i).ok().map(TargetIdx)
    }

    /// Adds `kind` at [`TargetTable::next_index`] (or returns its existing
    /// index).
    pub fn intern(&mut self, kind: TargetKind) -> Result<TargetIdx, EngineError> {
        if let Some(i) = self.find(&kind) {
            return Ok(i);
        }
        let i = self.next_index().ok_or(EngineError::TooManyTargets)?;
        match self.kinds.get_mut(usize::from(i.0)) {
            Some(e) => *e = Entry::Live(kind),
            None => self.kinds.push(Entry::Live(kind)),
        }
        Ok(i)
    }

    /// Stops using `idx` (its sink is being removed).
    pub fn release(&mut self, idx: TargetIdx) {
        if let Some(e) = self.kinds.get_mut(usize::from(idx.0)) {
            if matches!(e, Entry::Live(_)) {
                *e = Entry::Released;
            }
        }
    }

    /// Forgets `idx` at once (a failed load's target that never got a
    /// sink).
    pub(crate) fn forget(&mut self, idx: TargetIdx) {
        if let Some(e) = self.kinds.get_mut(usize::from(idx.0)) {
            *e = Entry::Free;
        }
    }

    /// Makes indices released by earlier loads reusable (start of a load).
    pub fn recycle(&mut self) {
        for e in &mut self.kinds {
            if *e == Entry::Released {
                *e = Entry::Free;
            }
        }
    }

    /// The live kind at `idx`.
    pub fn get(&self, idx: TargetIdx) -> Option<&TargetKind> {
        match self.kinds.get(usize::from(idx.0)) {
            Some(Entry::Live(k)) => Some(k),
            _ => None,
        }
    }

    /// Number of live targets.
    pub fn len(&self) -> usize {
        self.kinds
            .iter()
            .filter(|k| matches!(k, Entry::Live(_)))
            .count()
    }

    /// True when there are no live targets.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `(index, kind)` of every live target, by index.
    pub fn iter(&self) -> impl Iterator<Item = (TargetIdx, &TargetKind)> + '_ {
        self.kinds.iter().enumerate().filter_map(|(i, k)| match k {
            Entry::Live(k) => u8::try_from(i).ok().map(|i| (TargetIdx(i), k)),
            _ => None,
        })
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
        DestKind::PluginInstrument { slot } => Ok(vec![TargetKind::PluginInstrument(slot.id)]),
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
