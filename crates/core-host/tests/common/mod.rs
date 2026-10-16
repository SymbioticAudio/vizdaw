//! Shared test helpers: load a test plugin and shelve its processor.
#![allow(dead_code)]

use core_host::adapt::{PluginEffect, PluginSynth, Returned};
use core_host::clap::ClapFormat;
use core_host::queue::{GestureReceiver, ParamSender, param_queues};
use core_host::shelf::{Ready, Shelf};
use core_host::{LoadRequest, PluginFormat, PluginInstance};
use core_model::ids::PluginInstanceId;
use core_model::mixer::{PluginFormat as Format, PluginRef};
use std::sync::mpsc::Sender;

/// The reference of test plugin `id` (e.g. `vizdaw.test.gain`).
pub fn plugin(id: &str) -> PluginRef {
    PluginRef {
        format: Format::Clap,
        id: id.into(),
        name: id.trim_start_matches("vizdaw.test.").into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

/// Loads test plugin `id` from the test bundle.
pub fn load(id: &str) -> Box<dyn PluginInstance> {
    let mut f = ClapFormat::new();
    let req = LoadRequest {
        plugin: plugin(id),
        path: Some(test_plugins::bundle()),
        sample_rate: 48_000.0,
        max_frames: 4096,
    };
    let prepared = (f.prepare(&req))().unwrap();
    f.finish(&req, prepared).unwrap()
}

/// A loaded, activated, shelved instance and its main-thread ends.
pub struct Live {
    pub inst: Box<dyn PluginInstance>,
    pub params: ParamSender,
    pub gestures: GestureReceiver,
}

/// Activates `inst` for 48 kHz / 4096 and puts it on `shelf` as instance
/// `id` (an insert, or an instrument when `instrument`).
pub fn shelve(
    shelf: &Shelf,
    id: PluginInstanceId,
    mut inst: Box<dyn PluginInstance>,
    instrument: bool,
    back: &Sender<Returned>,
) -> Live {
    let proc = inst.activate(48_000.0, 4096).unwrap();
    let ((params, gestures), audio) = param_queues();
    let info = inst.info().clone();
    let ready = if instrument {
        Ready::Instrument(PluginSynth::new(id, proc, &info, 4096, audio, back.clone()))
    } else {
        Ready::Effect(PluginEffect::new(
            id,
            proc,
            &info,
            4096,
            audio,
            back.clone(),
        ))
    };
    shelf.put(id, &info.plugin, 4096, ready);
    Live {
        inst,
        params,
        gestures,
    }
}
