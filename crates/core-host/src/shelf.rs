//! Activated processors waiting for the engine: the host puts adapters
//! here, and [`ShelfFactory`] (the engine's `InsertFactory`) takes them when
//! a load needs a new instance (Plan Ruling 3). Control thread only.

use crate::adapt::{PluginEffect, PluginSynth};
use core_engine::backend::InstrumentNode;
use core_engine::graph::{EffectNode, InsertFactory};
use core_model::ids::PluginInstanceId;
use core_model::mixer::{PluginFormat, PluginRef, PluginSlot};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// A ready processor.
pub enum Ready {
    /// For an insert slot.
    Effect(PluginEffect),
    /// For a plugin instrument destination.
    Instrument(PluginSynth),
}

impl Ready {
    /// The instance it serves.
    pub fn id(&self) -> PluginInstanceId {
        match self {
            Ready::Effect(e) => e.id(),
            Ready::Instrument(s) => s.id(),
        }
    }

    /// Unwraps the processor.
    pub fn into_processor(self) -> Option<Box<dyn crate::instance::AudioProcessor>> {
        match self {
            Ready::Effect(e) => e.into_processor(),
            Ready::Instrument(s) => s.into_processor(),
        }
    }
}

struct Entry {
    format: PluginFormat,
    plugin: String,
    max_frames: u32,
    ready: Ready,
}

#[derive(Default)]
struct Inner {
    ready: BTreeMap<PluginInstanceId, Entry>,
    /// Why an instance is not on the shelf (loading, failed, disabled).
    why: BTreeMap<PluginInstanceId, String>,
}

/// The shelf, shared between the host and the engine's factory.
#[derive(Clone, Default)]
pub struct Shelf(Arc<Mutex<Inner>>);

impl std::fmt::Debug for Shelf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Shelf")
    }
}

impl Shelf {
    /// An empty shelf.
    pub fn new() -> Shelf {
        Shelf::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Puts instance `id`'s processor up for `plugin`, made for blocks of
    /// up to `max_frames`; returns the one it replaces.
    pub fn put(
        &self,
        id: PluginInstanceId,
        plugin: &PluginRef,
        max_frames: u32,
        ready: Ready,
    ) -> Option<Ready> {
        let mut g = self.lock();
        g.why.remove(&id);
        g.ready
            .insert(
                id,
                Entry {
                    format: plugin.format,
                    plugin: plugin.id.clone(),
                    max_frames,
                    ready,
                },
            )
            .map(|e| e.ready)
    }

    /// Records why `id` has no processor (shown as the placeholder reason).
    pub fn set_reason(&self, id: PluginInstanceId, why: impl Into<String>) {
        self.lock().why.insert(id, why.into());
    }

    /// Forgets the reason for `id`.
    pub fn clear_reason(&self, id: PluginInstanceId) {
        self.lock().why.remove(&id);
    }

    /// Whether `id` has a processor waiting.
    pub fn has(&self, id: PluginInstanceId) -> bool {
        self.lock().ready.contains_key(&id)
    }

    /// Takes every processor whose instance `keep` rejects (the document no
    /// longer has it).
    pub fn take_unwanted(&self, keep: impl Fn(PluginInstanceId) -> bool) -> Vec<Ready> {
        let mut g = self.lock();
        let gone: Vec<PluginInstanceId> = g.ready.keys().copied().filter(|k| !keep(*k)).collect();
        gone.into_iter()
            .filter_map(|k| g.ready.remove(&k).map(|e| e.ready))
            .collect()
    }

    /// The engine's factory for this shelf.
    pub fn factory(&self) -> ShelfFactory {
        ShelfFactory(self.clone())
    }

    fn take(&self, slot: &PluginSlot, max_frames: u32, instrument: bool) -> Result<Ready, String> {
        let mut g = self.lock();
        let name = &slot.plugin.name;
        let misfit = g.ready.get(&slot.id).and_then(|e| {
            if e.format != slot.plugin.format || e.plugin != slot.plugin.id {
                Some(format!("{name} is waiting as a different plugin"))
            } else if matches!(e.ready, Ready::Instrument(_)) != instrument {
                Some(format!(
                    "{name} is waiting as {}",
                    if instrument {
                        "an effect"
                    } else {
                        "an instrument"
                    }
                ))
            } else if e.max_frames < max_frames {
                Some(format!(
                    "{name} is made for blocks of {} frames, the engine needs {max_frames}",
                    e.max_frames
                ))
            } else {
                None
            }
        });
        if misfit.is_none() {
            if let Some(e) = g.ready.remove(&slot.id) {
                return Ok(e.ready);
            }
        }
        Err(misfit.unwrap_or_else(|| {
            g.why
                .get(&slot.id)
                .cloned()
                .unwrap_or_else(|| format!("{name} is not loaded"))
        }))
    }
}

/// The engine's `InsertFactory`: hands out shelved processors.
#[derive(Clone, Debug)]
pub struct ShelfFactory(Shelf);

impl InsertFactory for ShelfFactory {
    fn make(
        &mut self,
        slot: &PluginSlot,
        _sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn EffectNode>, String> {
        match self.0.take(slot, max_frames, false)? {
            Ready::Effect(e) => Ok(Box::new(e)),
            Ready::Instrument(_) => Err("not an effect".into()),
        }
    }

    fn make_instrument(
        &mut self,
        slot: &PluginSlot,
        _sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn InstrumentNode>, String> {
        match self.0.take(slot, max_frames, true)? {
            Ready::Instrument(s) => Ok(Box::new(s)),
            Ready::Effect(_) => Err("not an instrument".into()),
        }
    }
}
